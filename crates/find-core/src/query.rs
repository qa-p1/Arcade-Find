//! The query language.
//!
//! Free words match names (all must match, in any order). A word containing
//! `/` (or `\` on Windows) matches the full path; `*` and `?` make a word a
//! wildcard pattern on the name; `"quoted words"` match as one phrase.
//!
//! Filters: `ext:pdf,docx` · `dir:` / `file:` (also `type:dir`) · `in:~/Projects`
//! · `size:>100mb`, `size:<1k`, `size:10mb..1gb` · `modified:<7d` (within 7 days),
//! `modified:>1y` (older than a year), `modified:today`, `modified:>=2026-01-01`,
//! `modified:2026-01-01..2026-02-01` · `hidden:yes|no`.
//!
//! A query starting with `/` whose first word has no other `/` searches file
//! contents with ripgrep (`/TODO ext:rs in:~/code`); `content:` does the same.

use std::path::{Path, PathBuf};

use crate::paths::expand_tilde;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    Any,
    Dirs,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKind {
    Name,
    Path,
    Glob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// Lower-cased text used for matching.
    pub text: String,
    pub kind: TermKind,
    /// From a quoted phrase (no fuzzy matching).
    pub exact: bool,
}

impl Term {
    pub fn is_ascii(&self) -> bool {
        self.text.is_ascii()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub terms: Vec<Term>,
    /// Lower-cased extensions without the dot.
    pub exts: Vec<String>,
    pub kind: KindFilter,
    pub within: Vec<PathBuf>,
    /// Inclusive byte bounds.
    pub size: Option<(u64, u64)>,
    /// Inclusive Unix-second bounds.
    pub modified: Option<(i64, i64)>,
    pub hidden: Option<bool>,
    /// A ripgrep pattern (content search).
    pub content: Option<String>,
    /// Problems with filters, shown to the user.
    pub errors: Vec<String>,
}

impl Query {
    /// No words and no filters: the empty state.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.exts.is_empty()
            && self.kind == KindFilter::Any
            && self.within.is_empty()
            && self.size.is_none()
            && self.modified.is_none()
            && self.content.is_none()
    }

    pub fn has_path_terms(&self) -> bool {
        self.terms.iter().any(|t| t.kind == TermKind::Path)
    }

    pub fn parse(input: &str) -> Query {
        Query::parse_at(input, crate::now_secs(), crate::local_offset_secs())
    }

    /// Parses with an explicit clock (Unix seconds) and UTC offset, for tests.
    pub fn parse_at(input: &str, now: i64, utc_offset: i64) -> Query {
        let mut q = Query::default();
        let trimmed = input.trim_start();
        let mut content_words: Option<Vec<String>> = None;
        let mut rest = trimmed;
        if let Some(after) = trimmed.strip_prefix('/') {
            let first = after.split_whitespace().next().unwrap_or("");
            if !after.is_empty() && !after.starts_with(' ') && !first.contains('/') {
                content_words = Some(Vec::new());
                rest = after;
            }
        }
        for token in tokenize(rest) {
            let (word, quoted) = token;
            if !quoted {
                if let Some((key, value)) = split_filter(&word) {
                    if q.apply_filter(&key, &value, now, utc_offset) {
                        continue;
                    }
                }
            }
            if word.is_empty() {
                continue;
            }
            if let Some(words) = content_words.as_mut() {
                words.push(word);
                continue;
            }
            q.terms.push(make_term(&word, quoted));
        }
        if let Some(words) = content_words {
            let pat = words.join(" ");
            if !pat.is_empty() {
                q.content = Some(pat);
            }
        }
        q
    }

    fn apply_filter(&mut self, key: &str, value: &str, now: i64, offset: i64) -> bool {
        match key {
            "ext" => {
                for e in value.split([',', ';', '|']) {
                    let e = e.trim().trim_start_matches('.').to_lowercase();
                    if !e.is_empty() {
                        self.exts.push(e);
                    }
                }
                true
            }
            "dir" | "folder" | "dirs" | "folders" => {
                self.kind = KindFilter::Dirs;
                if !value.is_empty() {
                    self.terms.push(make_term(value, false));
                }
                true
            }
            "file" | "files" => {
                self.kind = KindFilter::Files;
                if !value.is_empty() {
                    self.terms.push(make_term(value, false));
                }
                true
            }
            "type" => {
                match value.to_lowercase().as_str() {
                    "dir" | "folder" | "d" => self.kind = KindFilter::Dirs,
                    "file" | "f" => self.kind = KindFilter::Files,
                    other => self.errors.push(format!("Unknown type “{other}”; use type:dir or type:file")),
                }
                true
            }
            "in" | "path" | "under" => {
                if value.is_empty() {
                    return true;
                }
                let p = expand_tilde(value);
                if p.is_absolute() {
                    self.within.push(crate::settings::normalize(&p));
                } else {
                    // A relative `in:` is a folder-name fragment of the path.
                    self.terms.push(Term {
                        text: format!("{}{}", value.to_lowercase(), std::path::MAIN_SEPARATOR),
                        kind: TermKind::Path,
                        exact: true,
                    });
                }
                true
            }
            "size" => {
                match parse_size_filter(value) {
                    Some(r) => self.size = Some(r),
                    None => self.errors.push(format!("Can't read size “{value}”; try size:>100mb")),
                }
                true
            }
            "modified" | "mod" | "date" | "dm" => {
                match parse_time_filter(value, now, offset) {
                    Some(r) => self.modified = Some(r),
                    None => self.errors.push(format!("Can't read date “{value}”; try modified:<7d")),
                }
                true
            }
            "hidden" => {
                self.hidden = Some(!matches!(value.to_lowercase().as_str(), "no" | "false" | "0" | "off"));
                true
            }
            "content" | "text" => {
                if !value.is_empty() {
                    self.content = Some(value.to_string());
                }
                true
            }
            _ => false,
        }
    }
}

fn make_term(word: &str, quoted: bool) -> Term {
    let lower = word.to_lowercase();
    let is_path = word.contains('/') || (cfg!(windows) && word.contains('\\'));
    let kind = if is_path {
        TermKind::Path
    } else if !quoted && (word.contains('*') || word.contains('?')) {
        TermKind::Glob
    } else {
        TermKind::Name
    };
    let text = if is_path && cfg!(windows) { lower.replace('/', "\\") } else { lower };
    Term { text, kind, exact: quoted }
}

/// `key:value` with a known key (keys are case-insensitive).
fn split_filter(word: &str) -> Option<(String, String)> {
    let (k, v) = word.split_once(':')?;
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    // `C:\x` on Windows is a path, not a filter.
    if k.len() == 1 && (v.starts_with('\\') || v.starts_with('/')) {
        return None;
    }
    let v = v.trim_matches('"').to_string();
    Some((k.to_ascii_lowercase(), v))
}

/// Splits on whitespace, keeping `"quoted phrases"` and `key:"quoted values"` whole.
fn tokenize(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut quoted_token = false;
    for c in s.chars() {
        match c {
            '"' => {
                if in_quote {
                    in_quote = false;
                } else {
                    in_quote = true;
                    if cur.is_empty() {
                        quoted_token = true;
                    }
                }
                if !quoted_token {
                    cur.push(c);
                }
            }
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push((std::mem::take(&mut cur), quoted_token));
                }
                quoted_token = false;
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push((cur, quoted_token));
    }
    out
}

/// A size in bytes: `100`, `100b`, `4k`, `4kb`, `1.5mb`, `2g`, `1t` (binary units).
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();
    let idx = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    let n: f64 = num.parse().ok()?;
    let mult: u64 = match unit.trim() {
        "" | "b" | "bytes" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        "t" | "tb" | "tib" => 1 << 40,
        _ => return None,
    };
    if !(0.0..1e19).contains(&n) {
        return None;
    }
    Some((n * mult as f64) as u64)
}

fn parse_size_filter(v: &str) -> Option<(u64, u64)> {
    if let Some((a, b)) = v.split_once("..") {
        let lo = if a.is_empty() { 0 } else { parse_size(a)? };
        let hi = if b.is_empty() { u64::MAX } else { parse_size(b)? };
        return Some((lo, hi));
    }
    if let Some(x) = v.strip_prefix(">=") {
        return Some((parse_size(x)?, u64::MAX));
    }
    if let Some(x) = v.strip_prefix("<=") {
        return Some((0, parse_size(x)?));
    }
    if let Some(x) = v.strip_prefix('>') {
        return Some((parse_size(x)?.saturating_add(1), u64::MAX));
    }
    if let Some(x) = v.strip_prefix('<') {
        return Some((0, parse_size(x)?.saturating_sub(1)));
    }
    if let Some(x) = v.strip_prefix('=') {
        let n = parse_size(x)?;
        return Some((n, n));
    }
    Some((parse_size(v)?, u64::MAX))
}

/// A duration: `30s`, `15min`, `2h`, `7d`, `3w`, `6mo`, `1y`.
pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim().to_lowercase();
    let idx = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    let n: i64 = num.parse().ok()?;
    let secs: i64 = match unit {
        "s" | "sec" | "secs" => 1,
        "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hour" | "hours" => 3600,
        "d" | "day" | "days" | "" => 86_400,
        "w" | "week" | "weeks" => 7 * 86_400,
        "mo" | "mon" | "month" | "months" => 30 * 86_400,
        "y" | "yr" | "year" | "years" => 365 * 86_400,
        _ => return None,
    };
    n.checked_mul(secs)
}

/// `YYYY-MM-DD` → local midnight as Unix seconds.
pub fn parse_date(s: &str, utc_offset: i64) -> Option<i64> {
    let mut it = s.trim().split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1970..=2200).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400 - utc_offset)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Local midnight of the day containing `now`.
pub fn local_midnight(now: i64, utc_offset: i64) -> i64 {
    (now + utc_offset).div_euclid(86_400) * 86_400 - utc_offset
}

fn parse_time_filter(v: &str, now: i64, off: i64) -> Option<(i64, i64)> {
    let v = v.trim().to_lowercase();
    match v.as_str() {
        "today" => return Some((local_midnight(now, off), i64::MAX)),
        "yesterday" => {
            let m = local_midnight(now, off);
            return Some((m - 86_400, m - 1));
        }
        "thisweek" | "week" => return Some((now - 7 * 86_400, i64::MAX)),
        "thismonth" | "month" => return Some((now - 30 * 86_400, i64::MAX)),
        "thisyear" | "year" => return Some((now - 365 * 86_400, i64::MAX)),
        _ => {}
    }
    if let Some((a, b)) = v.split_once("..") {
        let lo = if a.is_empty() { i64::MIN } else { parse_date(a, off)? };
        let hi = if b.is_empty() { i64::MAX } else { parse_date(b, off)? + 86_399 };
        return Some((lo, hi));
    }
    let (op, rest) = if let Some(r) = v.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = v.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = v.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = v.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = v.strip_prefix('=') {
        ("=", r)
    } else {
        ("", v.as_str())
    };
    if let Some(day) = parse_date(rest, off) {
        return Some(match op {
            ">" => (day + 86_400, i64::MAX),
            ">=" => (day, i64::MAX),
            "<" => (i64::MIN, day - 1),
            "<=" => (i64::MIN, day + 86_399),
            _ => (day, day + 86_399),
        });
    }
    // Relative: `<7d` = newer than 7 days ago; `>7d` = older.
    let dur = parse_duration(rest)?;
    let edge = now - dur;
    Some(match op {
        ">" | ">=" => (i64::MIN, edge),
        _ => (edge, i64::MAX),
    })
}

/// Does `path` lie inside any of `within`?
pub fn path_within(path: &Path, within: &[PathBuf]) -> bool {
    within.is_empty() || within.iter().any(|w| path.starts_with(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_000_000; // 2026-10-03

    fn p(s: &str) -> Query {
        Query::parse_at(s, NOW, 0)
    }

    #[test]
    fn words_and_filters() {
        let q = p("annual Report ext:pdf,DOCX size:>100mb modified:<7d");
        assert_eq!(q.terms.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["annual", "report"]);
        assert_eq!(q.exts, ["pdf", "docx"]);
        assert_eq!(q.size, Some((100 * 1024 * 1024 + 1, u64::MAX)));
        assert_eq!(q.modified, Some((NOW - 7 * 86_400, i64::MAX)));
        assert!(q.errors.is_empty());
    }

    #[test]
    fn dir_in_and_hidden() {
        let q = p("dir: in:~/Projects hidden:yes src");
        assert_eq!(q.kind, KindFilter::Dirs);
        assert_eq!(q.within, vec![crate::paths::home_dir().join("Projects")]);
        assert_eq!(q.hidden, Some(true));
        assert_eq!(q.terms.len(), 1);
        let q = p("dir:src");
        assert_eq!(q.kind, KindFilter::Dirs);
        assert_eq!(q.terms[0].text, "src");
        assert_eq!(p("type:file").kind, KindFilter::Files);
    }

    #[test]
    fn quoted_and_paths_and_globs() {
        let q = p(r#""my notes" proj/src *.rs in:"/tmp/a b""#);
        assert_eq!(q.terms[0], Term { text: "my notes".into(), kind: TermKind::Name, exact: true });
        assert_eq!(q.terms[1].kind, TermKind::Path);
        assert_eq!(q.terms[2].kind, TermKind::Glob);
        assert_eq!(q.within, vec![PathBuf::from("/tmp/a b")]);
    }

    #[test]
    fn content_prefix() {
        let q = p("/TODO fixme ext:rs");
        assert_eq!(q.content.as_deref(), Some("TODO fixme"));
        assert_eq!(q.exts, ["rs"]);
        assert!(q.terms.is_empty());
        // An absolute path is a path query, not content search.
        let q = p("/home/u/doc");
        assert!(q.content.is_none());
        assert_eq!(q.terms[0].kind, TermKind::Path);
        assert!(p("/").content.is_none());
        assert_eq!(p("content:\"a b\"").content.as_deref(), Some("a b"));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("1.5mb"), Some(1_572_864));
        assert_eq!(parse_size("4k"), Some(4096));
        assert_eq!(parse_size("12"), Some(12));
        assert_eq!(parse_size("1x"), None);
        assert_eq!(parse_size_filter("10mb..1gb"), Some((10 << 20, 1 << 30)));
        assert_eq!(parse_size_filter("<1k"), Some((0, 1023)));
        assert_eq!(parse_size_filter("=0"), Some((0, 0)));
        assert!(p("size:huge").errors.len() == 1);
    }

    #[test]
    fn dates() {
        let day = parse_date("2026-01-01", 0).unwrap();
        assert_eq!(day, 1_767_225_600);
        assert_eq!(p("modified:>=2026-01-01").modified, Some((day, i64::MAX)));
        assert_eq!(p("modified:<2026-01-01").modified, Some((i64::MIN, day - 1)));
        assert_eq!(p("modified:2026-01-01").modified, Some((day, day + 86_399)));
        assert_eq!(p("modified:>1y").modified, Some((i64::MIN, NOW - 365 * 86_400)));
        assert_eq!(p("mod:today").modified, Some((local_midnight(NOW, 0), i64::MAX)));
        assert_eq!(local_midnight(86_400 + 5, 3600), 86_400 - 3600);
        assert!(p("modified:soon").errors.len() == 1);
    }

    #[test]
    fn unknown_keys_are_words() {
        let q = p("foo:bar http://x");
        assert_eq!(q.terms.len(), 2);
        assert_eq!(q.terms[0].text, "foo:bar");
        assert!(p("").is_empty());
        assert!(!p("ext:pdf").is_empty());
    }
}
