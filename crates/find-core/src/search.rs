//! Searching the index: filters, matching and ranking, in parallel chunks
//! with a bounded top-K per chunk.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::index::{flag, IdMap, IdSet, Index, NONE};
use crate::matcher;
use crate::query::{KindFilter, Query, TermKind};

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub limit: usize,
    pub show_hidden: bool,
    /// Frecency boosts by entry id.
    pub boosts: IdMap<i32>,
    pub now: i64,
    /// Worker threads (0 = automatic).
    pub threads: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scored {
    pub id: u32,
    pub score: i32,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    /// Best first.
    pub hits: Vec<Scored>,
    /// How many entries matched in total (before the limit).
    pub matched: usize,
    pub elapsed: Duration,
    pub generation: u64,
}

/// A result ready to show or send.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: u32,
    pub path: PathBuf,
    pub name: String,
    pub parent: PathBuf,
    pub size: u64,
    pub mtime: i64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub hidden: bool,
    pub score: i32,
}

impl Hit {
    pub fn from_index(ix: &Index, s: Scored) -> Hit {
        let path = ix.path(s.id);
        let m = ix.meta(s.id);
        Hit {
            id: s.id,
            name: ix.name_os(s.id).to_string_lossy().into_owned(),
            parent: path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
            path,
            size: m.size,
            mtime: m.mtime,
            is_dir: m.flags & flag::DIR != 0,
            is_symlink: m.flags & flag::SYMLINK != 0,
            hidden: m.flags & flag::HIDDEN != 0,
            score: s.score,
        }
    }
}

/// Per-query tables computed once before the parallel scan.
struct Prepared {
    /// `in:` — entries inside the requested folders.
    within: Option<Vec<bool>>,
    /// Path words: a KMP matcher per word and, per folder, the matcher state
    /// after reading "folder path/" (bit 15 set once the word was found).
    path_kmp: Vec<Kmp>,
    path_states: Vec<Vec<u16>>,
    /// Name words that are ASCII (fast path).
    ascii: Vec<bool>,
}

fn prepare(ix: &Index, q: &Query) -> Prepared {
    let mut within = None;
    if !q.within.is_empty() {
        let mut targets: IdSet = IdSet::default();
        let mut prefixes: Vec<Vec<u8>> = Vec::new();
        for w in &q.within {
            match ix.dir_at(w) {
                Some(id) => {
                    targets.insert(id);
                }
                None => prefixes.push(w.as_os_str().as_encoded_bytes().to_ascii_lowercase()),
            }
        }
        let mut under = vec![false; ix.len()];
        if !prefixes.is_empty() {
            // Folders not in the index by exact path: compare path strings (case-insensitively).
            for i in 0..ix.len() as u32 {
                if ix.is_dir(i) && !ix.is_deleted(i) {
                    let p = ix.path(i);
                    let pb = p.as_os_str().as_encoded_bytes().to_ascii_lowercase();
                    if prefixes
                        .iter()
                        .any(|pre| pb.starts_with(pre) && (pb.len() == pre.len() || pb[pre.len()] == std::path::MAIN_SEPARATOR as u8))
                    {
                        targets.insert(i);
                    }
                }
            }
        }
        for i in 0..ix.len() {
            let p = ix.parent[i];
            under[i] = (p != NONE && (under[p as usize] || targets.contains(&p))) || false;
        }
        within = Some(under);
    }
    let mut path_kmp = Vec::new();
    let mut path_states = Vec::new();
    for t in q.terms.iter().filter(|t| t.kind == TermKind::Path) {
        let kmp = Kmp::new(t.text.as_bytes());
        let mut states = vec![0u16; ix.len()];
        let sep = std::path::MAIN_SEPARATOR as u8;
        for r in &ix.roots {
            if !ix.is_deleted(r.id) {
                let mut st = 0u16;
                for &b in r.path.as_os_str().as_encoded_bytes() {
                    st = kmp.feed(st, b);
                }
                if r.path.as_os_str().as_encoded_bytes().last() != Some(&sep) {
                    st = kmp.feed(st, sep);
                }
                states[r.id as usize] = st;
            }
        }
        // Parents come first, so one forward pass fills every folder.
        for i in 0..ix.len() {
            let p = ix.parent[i];
            if p == NONE || ix.flags[i] & (flag::DIR | flag::DELETED) != flag::DIR {
                continue;
            }
            let mut st = states[p as usize];
            for &b in ix.name(i as u32) {
                st = kmp.feed(st, b);
            }
            states[i] = kmp.feed(st, sep);
        }
        path_kmp.push(kmp);
        path_states.push(states);
    }
    Prepared { within, path_kmp, path_states, ascii: q.terms.iter().map(|t| t.is_ascii()).collect() }
}

const FOUND: u16 = 1 << 15;

/// Case-insensitive (ASCII) substring matcher that can resume from a state,
/// so a folder's state is computed once from its parent's.
struct Kmp {
    pat: Vec<u8>,
    fail: Vec<u16>,
    /// Length of the word up to and including its last separator (0 if none).
    head: usize,
}

impl Kmp {
    fn new(pat: &[u8]) -> Kmp {
        let pat: Vec<u8> = pat.iter().take(4096).map(|b| b.to_ascii_lowercase()).collect();
        let mut fail = vec![0u16; pat.len()];
        let mut k = 0usize;
        for i in 1..pat.len() {
            while k > 0 && pat[i] != pat[k] {
                k = fail[k - 1] as usize;
            }
            if pat[i] == pat[k] {
                k += 1;
            }
            fail[i] = k as u16;
        }
        let sep = std::path::MAIN_SEPARATOR as u8;
        let head = pat.iter().rposition(|&b| b == sep).map_or(0, |i| i + 1);
        Kmp { pat, fail, head }
    }

    /// Does the word span from a folder path in state `st` into `name`?
    #[inline]
    fn spans(&self, st: u16, name: &[u8]) -> bool {
        if self.head == 0 || self.head == self.pat.len() {
            return false;
        }
        let tail = &self.pat[self.head..];
        if name.len() < tail.len() || !name[..tail.len()].iter().zip(tail).all(|(a, b)| a.to_ascii_lowercase() == *b) {
            return false;
        }
        // Is `head` among the prefix lengths that end the folder path?
        let mut k = st as usize;
        while k > self.head {
            k = self.fail[k - 1] as usize;
        }
        k == self.head
    }

    /// Next state after byte `b`. Once found, the state stays found.
    #[inline]
    fn feed(&self, st: u16, b: u8) -> u16 {
        if st & FOUND != 0 || self.pat.is_empty() {
            return FOUND;
        }
        let b = b.to_ascii_lowercase();
        let mut k = st as usize;
        while k > 0 && self.pat[k] != b {
            k = self.fail[k - 1] as usize;
        }
        if self.pat[k] == b {
            k += 1;
        }
        if k == self.pat.len() {
            FOUND
        } else {
            k as u16
        }
    }
}

#[inline]
fn ext_matches(name: &[u8], exts: &[String]) -> bool {
    let Some(dot) = memchr::memrchr(b'.', name) else { return false };
    if dot == 0 {
        return false;
    }
    let ext = &name[dot + 1..];
    exts.iter().any(|e| e.as_bytes().eq_ignore_ascii_case(ext))
}

/// How an entry fares against a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Eval {
    /// Filtered out, or a word doesn't occur in it at all.
    No,
    /// Every word occurs (in order), but a fuzzy match scored below the
    /// floor: not a result, yet a longer word could still match it.
    Near,
    Yes(i32),
}

/// Scores entry `i` against the query (without frecency boosts).
#[inline]
fn evaluate(ix: &Index, q: &Query, prep: &Prepared, opts: &SearchOptions, show_hidden: bool, i: usize) -> Eval {
    let f = ix.flags[i];
    if f & flag::DELETED != 0 || (!show_hidden && f & flag::HIDDEN != 0) {
        return Eval::No;
    }
    let is_dir = f & flag::DIR != 0;
    match q.kind {
        KindFilter::Dirs if !is_dir => return Eval::No,
        KindFilter::Files if is_dir => return Eval::No,
        _ => {}
    }
    if let Some(w) = &prep.within {
        if !w[i] {
            return Eval::No;
        }
    }
    let name = ix.name(i as u32);
    if !q.exts.is_empty() && (is_dir || !ext_matches(name, &q.exts)) {
        return Eval::No;
    }
    if let Some((lo, hi)) = q.size {
        if is_dir {
            return Eval::No;
        }
        let s = crate::index::decode_size(ix.size[i]);
        if s < lo || s > hi {
            return Eval::No;
        }
    }
    if let Some((lo, hi)) = q.modified {
        let t = ix.mtime[i] as i64;
        if t < lo || t > hi {
            return Eval::No;
        }
    }
    let mut score = 0i32;
    let mut near = false;
    let mut pi = 0;
    for (ti, t) in q.terms.iter().enumerate() {
        let s = match t.kind {
            TermKind::Name => {
                let r = if prep.ascii[ti] {
                    matcher::score_ascii_raw(name, t.text.as_bytes(), !t.exact)
                } else {
                    matcher::score_unicode_raw(name, &t.text, !t.exact)
                };
                let Some(s) = r else { return Eval::No };
                near |= s < matcher::FUZZY_MIN;
                s
            }
            TermKind::Glob => match matcher::score_glob(name, &t.text) {
                Some(s) => s,
                None => return Eval::No,
            },
            TermKind::Path => {
                let kmp = &prep.path_kmp[pi];
                let parent = ix.parent[i];
                let st = if parent == NONE { 0 } else { prep.path_states[pi][parent as usize] };
                pi += 1;
                // Either the word is inside the folder path, or it spans the
                // last separator: "…X/" ends the folder path and the name
                // starts with what follows the word's last separator.
                if st & FOUND == 0 && !kmp.spans(st, name) {
                    return Eval::No;
                }
                300
            }
        };
        score += s;
    }
    if near {
        return Eval::Near;
    }
    if q.terms.is_empty() {
        // Filters only: prefer recent, then shallow.
        score = 100;
    }
    score -= (name.len() as i32) / 10;
    let age = opts.now - ix.mtime[i] as i64;
    if age < 86_400 {
        score += 20;
    } else if age < 7 * 86_400 {
        score += 10;
    }
    Eval::Yes(score)
}

/// The rarest ASCII character that every match must contain (from the
/// name words), as (lower, upper) bytes. `None` when the query has no
/// such word (filters only, globs, paths, non-ASCII words).
fn prefilter_char(q: &Query) -> Option<(u8, u8)> {
    // Rough rarity in file names (lower is rarer).
    fn freq(c: u8) -> u32 {
        match c {
            b'e' => 100,
            b'a' | b't' | b'o' | b'i' | b'n' | b's' | b'r' => 80,
            b'l' | b'c' | b'd' | b'm' | b'h' | b'u' | b'p' | b'g' => 55,
            b'.' | b'_' | b'-' | b' ' => 70,
            b'0'..=b'9' => 50,
            b'f' | b'b' | b'w' | b'y' | b'v' => 35,
            b'k' => 20,
            b'x' | b'j' | b'q' | b'z' => 8,
            _ => 30,
        }
    }
    let mut best: Option<(u32, u8)> = None;
    for t in q.terms.iter().filter(|t| t.kind == TermKind::Name && t.is_ascii()) {
        for &c in t.text.as_bytes() {
            let f = freq(c);
            if best.is_none_or(|(bf, _)| f < bf) {
                best = Some((f, c));
            }
        }
    }
    best.map(|(_, c)| (c, c.to_ascii_uppercase()))
}

/// Shallower entries rank slightly higher (computed only for candidates).
#[inline]
fn depth_penalty(ix: &Index, i: usize) -> i32 {
    let mut depth = 0;
    let mut p = ix.parent[i];
    while p != NONE && depth < 30 {
        depth += 1;
        p = ix.parent[p as usize];
    }
    depth * 3
}

/// The previous query's candidates, so typing more letters only re-checks
/// those (every match of "repo" is a match or near-match of "rep").
#[derive(Debug, Default)]
pub struct Narrowing {
    query: Option<Query>,
    generation: u64,
    show_hidden: bool,
    /// Entries that matched or nearly matched, in index order.
    ids: Vec<u32>,
}

/// Most candidates kept for narrowing (4 bytes each).
const NARROW_MAX: usize = 4_000_000;

impl Narrowing {
    pub fn clear(&mut self) {
        self.query = None;
        self.ids = Vec::new();
    }

    /// Can `q` be answered from the candidates of the previous query?
    fn covers(&self, ix: &Index, q: &Query, show_hidden: bool) -> bool {
        let Some(old) = &self.query else { return false };
        if self.generation != ix.generation() || self.show_hidden != show_hidden {
            return false;
        }
        if old.exts != q.exts
            || old.kind != q.kind
            || old.within != q.within
            || old.size != q.size
            || old.modified != q.modified
            || old.hidden != q.hidden
        {
            return false;
        }
        if q.content.is_some() || old.content.is_some() || old.terms.is_empty() || q.terms.len() < old.terms.len() {
            return false;
        }
        // Each old word must be a prefix of the new word in its place (same
        // kind); extra words only narrow further. Globs aren't monotonic.
        old.terms
            .iter()
            .zip(&q.terms)
            .all(|(a, b)| a.kind == b.kind && a.exact == b.exact && a.kind != TermKind::Glob && b.text.starts_with(&a.text))
            && q.terms.iter().all(|t| t.kind != TermKind::Glob)
    }
}

type Heap = BinaryHeap<Reverse<(i32, Reverse<u32>)>>;

fn push_top(heap: &mut Heap, limit: usize, s: i32, id: u32) {
    if heap.len() < limit {
        heap.push(Reverse((s, Reverse(id))));
    } else if let Some(Reverse((min, _))) = heap.peek() {
        if s > *min {
            heap.pop();
            heap.push(Reverse((s, Reverse(id))));
        }
    }
}

pub fn search(ix: &Index, q: &Query, opts: &SearchOptions) -> SearchResult {
    search_narrowing(ix, q, opts, None)
}

/// Searches, re-using (and updating) `narrow` when given.
pub fn search_narrowing(ix: &Index, q: &Query, opts: &SearchOptions, mut narrow: Option<&mut Narrowing>) -> SearchResult {
    let start = Instant::now();
    let limit = opts.limit.max(1);
    let show_hidden = q.hidden.unwrap_or(opts.show_hidden);
    let prep = prepare(ix, q);
    let n = ix.len();
    let from: Option<Vec<u32>> = match narrow.as_deref_mut() {
        Some(nw) if nw.covers(ix, q, show_hidden) => Some(std::mem::take(&mut nw.ids)),
        _ => None,
    };
    let keep = narrow.is_some() && !q.terms.is_empty() && q.terms.iter().all(|t| t.kind != TermKind::Glob) && q.content.is_none();
    let work = from.as_ref().map_or(n, Vec::len);
    let threads = if opts.threads > 0 { opts.threads } else { std::thread::available_parallelism().map_or(4, |p| p.get()).min(8) };
    let threads = if work < 50_000 { 1 } else { threads };
    let chunk = work.div_ceil(threads.max(1)).max(1);
    let key = prefilter_char(q);
    let run = |lo: usize, hi: usize| -> (Heap, usize, Vec<u32>) {
        let mut heap: Heap = BinaryHeap::with_capacity(limit + 1);
        let mut matched = 0usize;
        let mut cands: Vec<u32> = Vec::new();
        let mut consider = |i: usize, heap: &mut Heap, cands: &mut Vec<u32>| {
            match evaluate(ix, q, &prep, opts, show_hidden, i) {
                Eval::No => {}
                Eval::Near => {
                    if keep {
                        cands.push(i as u32);
                    }
                }
                Eval::Yes(s) => {
                    if keep {
                        cands.push(i as u32);
                    }
                    matched += 1;
                    // The depth penalty only lowers a score: skip it for entries that can't make the cut.
                    if heap.len() >= limit && heap.peek().is_some_and(|Reverse((min, _))| s <= *min) {
                        return;
                    }
                    push_top(heap, limit, s - depth_penalty(ix, i), i as u32);
                }
            }
        };
        if let Some(ids) = &from {
            for &i in &ids[lo..hi] {
                consider(i as usize, &mut heap, &mut cands);
            }
            return (heap, matched, cands);
        }
        match key {
            Some((a, b)) if lo < hi => {
                // Every match contains this character: find it in the
                // contiguous name bytes and evaluate only those entries.
                let start = ix.name_off[lo] as usize;
                let end = ix.name_off[hi - 1] as usize + ix.name_len[hi - 1] as usize;
                let hay = &ix.names[start..end];
                let mut pos = 0usize;
                let mut id = lo;
                while pos < hay.len() {
                    let found = if a == b { memchr::memchr(a, &hay[pos..]) } else { memchr::memchr2(a, b, &hay[pos..]) };
                    let Some(off) = found else { break };
                    let at = start + pos + off;
                    // Advance to the entry whose name contains `at`.
                    while id < hi && ix.name_off[id] as usize + ix.name_len[id] as usize <= at {
                        id += 1;
                    }
                    if id >= hi {
                        break;
                    }
                    if (ix.name_off[id] as usize) <= at {
                        consider(id, &mut heap, &mut cands);
                    }
                    pos = ix.name_off[id] as usize + ix.name_len[id] as usize - start;
                    id += 1;
                }
            }
            _ => {
                for i in lo..hi {
                    consider(i, &mut heap, &mut cands);
                }
            }
        }
        (heap, matched, cands)
    };
    let parts: Vec<(Heap, usize, Vec<u32>)> = if threads <= 1 {
        vec![run(0, work)]
    } else {
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|t| (t * chunk, ((t + 1) * chunk).min(work)))
                .filter(|(lo, hi)| lo < hi)
                .map(|(lo, hi)| s.spawn(move || run(lo, hi)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
        })
    };
    let matched = parts.iter().map(|p| p.1).sum();
    let mut cands_total: Vec<u32> = Vec::new();
    let mut overflow = !keep;
    let mut all: Vec<Scored> = Vec::new();
    for (heap, _, cands) in parts {
        all.extend(heap.into_iter().map(|Reverse((score, Reverse(id)))| Scored { id, score }));
        if !overflow && cands_total.len() + cands.len() <= NARROW_MAX {
            cands_total.extend(cands);
        } else {
            overflow = true;
            cands_total = Vec::new();
        }
    }
    // Frecency: score only the boosted entries again (a few thousand at
    // most) instead of a hash lookup for every match.
    if !opts.boosts.is_empty() {
        let mut boosted: IdMap<i32> = IdMap::default();
        for (&id, &b) in &opts.boosts {
            if (id as usize) < n {
                if let Eval::Yes(s) = evaluate(ix, q, &prep, opts, show_hidden, id as usize) {
                    boosted.insert(id, s + b - depth_penalty(ix, id as usize));
                }
            }
        }
        all.retain(|h| !boosted.contains_key(&h.id));
        all.extend(boosted.into_iter().map(|(id, score)| Scored { id, score }));
    }
    all.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| ix.name(a.id).len().cmp(&ix.name(b.id).len())).then_with(|| a.id.cmp(&b.id)));
    all.truncate(limit);
    if let Some(nw) = narrow {
        if !overflow {
            // Chunks are in index order, so candidates stay sorted.
            nw.query = Some(q.clone());
            nw.generation = ix.generation();
            nw.show_hidden = show_hidden;
            nw.ids = cands_total;
        } else {
            nw.clear();
        }
    }
    SearchResult { hits: all, matched, elapsed: start.elapsed(), generation: ix.generation() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{key_child, key_root, Meta};
    use std::path::Path;

    fn build() -> Index {
        let mut ix = Index::new();
        let root_s = crate::paths::test_abs("/home/u");
        let root = Path::new(&root_s);
        let rk = key_root(root);
        let r = ix.add_root(root, Meta::default());
        let dir = |ix: &mut Index, p: u32, pk: u64, n: &str, hidden: bool| {
            let k = key_child(pk, n.as_bytes());
            (ix.push(p, n.as_bytes(), Meta { flags: flag::DIR | if hidden { flag::HIDDEN } else { 0 }, mtime: 1000, size: 0 }, k), k)
        };
        let (docs, dk) = dir(&mut ix, r, rk, "Documents", false);
        let (proj, pk) = dir(&mut ix, r, rk, "Projects", false);
        let (src, _sk) = dir(&mut ix, proj, pk, "src", false);
        let (cfg, _ck) = dir(&mut ix, r, rk, ".config", true);
        let _ = dk;
        let file = |ix: &mut Index, p: u32, n: &str, size: u64, mtime: i64| ix.push(p, n.as_bytes(), Meta { size, mtime, flags: 0 }, 0);
        file(&mut ix, docs, "annual-report.pdf", 200 * 1024 * 1024, 5000);
        file(&mut ix, docs, "report.docx", 10_000, 9000);
        file(&mut ix, src, "main.rs", 500, 9000);
        file(&mut ix, src, "report_gen.rs", 800, 100);
        file(&mut ix, cfg, "report.toml", 10, 9000);
        ix
    }

    fn names(ix: &Index, q: &str, hidden: bool) -> Vec<String> {
        let q = Query::parse_at(q, 10_000, 0);
        let r = search(ix, &q, &SearchOptions { limit: 50, show_hidden: hidden, now: 10_000, ..Default::default() });
        r.hits.iter().map(|h| ix.name_os(h.id).to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn ranks_and_filters() {
        let ix = build();
        let r = names(&ix, "report", false);
        assert_eq!(r[0], "report.docx");
        assert!(r.contains(&"annual-report.pdf".to_string()));
        assert!(!r.contains(&"report.toml".to_string()), "hidden excluded by default");
        assert!(names(&ix, "report", true).contains(&"report.toml".to_string()));
        assert_eq!(names(&ix, "report ext:pdf", false), ["annual-report.pdf"]);
        assert_eq!(names(&ix, "report size:>100mb", false), ["annual-report.pdf"]);
        assert_eq!(names(&ix, "dir: proj", false), ["Projects"]);
        let projects = crate::paths::test_abs("/home/u/Projects");
        assert_eq!(names(&ix, &format!("report in:{projects}"), false), ["report_gen.rs"]);
        assert_eq!(names(&ix, "report modified:>2h", false), ["report_gen.rs"]);
        assert_eq!(names(&ix, "projects/src/ma", false), ["main.rs"]);
        assert_eq!(names(&ix, "src/", false), ["main.rs", "report_gen.rs"]);
        assert!(names(&ix, "hidden:yes report", false).contains(&"report.toml".to_string()));
        assert!(names(&ix, "zzz", false).is_empty());
    }

    #[test]
    fn narrowing_matches_full_search() {
        let mut ix = Index::new();
        let r = ix.add_root(Path::new("/r"), Meta::default());
        let words = ["report", "rapport", "repo", "r-e-p-o-r-t", "prepare", "xrexpxoxrxtx", "README", "zebra", "rep_2024", "a"];
        for i in 0..60_000u32 {
            let w = words[i as usize % words.len()];
            ix.push(
                r,
                format!("{w}-{}.txt", i % 911).as_bytes(),
                Meta { size: i as u64, mtime: 1, flags: if i % 17 == 0 { flag::HIDDEN } else { 0 } },
                0,
            );
        }
        let mut nw = Narrowing::default();
        let mut typed = String::new();
        for c in "report 9".chars() {
            typed.push(c);
            let q = Query::parse_at(&typed, 10, 0);
            let opts = SearchOptions { limit: 300, now: 10, threads: 3, ..Default::default() };
            let a = search_narrowing(&ix, &q, &opts, Some(&mut nw));
            let b = search(&ix, &q, &opts);
            assert_eq!(a.matched, b.matched, "{typed}");
            assert_eq!(a.hits, b.hits, "{typed}");
        }
        assert!(nw.query.is_some(), "the last query is kept for narrowing");
        // A changed filter or a new index generation starts over.
        let q = Query::parse_at("report ext:md", 10, 0);
        assert!(!nw.covers(&ix, &q, false));
        ix.push(r, b"report-new.txt", Meta::default(), 0);
        let q = Query::parse_at("report 91", 10, 0);
        assert!(!nw.covers(&ix, &q, false));
    }

    #[test]
    fn frecency_boost_reorders() {
        let ix = build();
        let q = Query::parse_at("report", 10_000, 0);
        let gen = ix.lookup(Path::new(&crate::paths::test_abs("/home/u/Projects/src/report_gen.rs"))).unwrap();
        let mut boosts = IdMap::default();
        boosts.insert(gen, 900);
        let r = search(&ix, &q, &SearchOptions { limit: 5, boosts, now: 10_000, ..Default::default() });
        assert_eq!(r.hits[0].id, gen);
    }

    #[test]
    fn parallel_matches_serial() {
        let mut ix = Index::new();
        let r = ix.add_root(Path::new("/r"), Meta::default());
        for i in 0..120_000u32 {
            ix.push(r, format!("file-{i}-{}.txt", i % 97).as_bytes(), Meta { size: i as u64, mtime: 1, flags: 0 }, 0);
        }
        let q = Query::parse_at("file-1 txt", 10, 0);
        let a = search(&ix, &q, &SearchOptions { limit: 100, threads: 1, now: 10, ..Default::default() });
        let b = search(&ix, &q, &SearchOptions { limit: 100, threads: 4, now: 10, ..Default::default() });
        assert_eq!(a.matched, b.matched);
        assert_eq!(a.hits, b.hits);
    }
}
