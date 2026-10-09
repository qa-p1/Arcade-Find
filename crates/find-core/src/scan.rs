//! Crawling and reconciling: list folders on worker threads and bring the
//! index in line with what's on disk.
//!
//! The same code does the first crawl (every folder "fresh": nothing to
//! compare), the warm-start and periodic reconciling rescans (compare each
//! folder's listing with its indexed children), and watcher-triggered
//! rescans of single folders. Symlinks are indexed but never followed, and
//! each real folder (device + inode) is visited once, so a link such as
//! `~/Data → /mnt/Data` never doubles a drive that is also a root.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};

use crate::exclude::Excludes;
use crate::index::{flag, key_child, key_root, IdMap, IdSet, Index, Meta, NONE};

/// Live counters for the status line.
#[derive(Debug, Default)]
pub struct Progress {
    pub running: AtomicBool,
    pub dirs: AtomicU64,
    pub entries: AtomicU64,
    pub added: AtomicU64,
    pub removed: AtomicU64,
    pub changed: AtomicU64,
    pub errors: AtomicU64,
    pub current: Mutex<String>,
}

impl Progress {
    pub fn reset(&self) {
        for c in [&self.dirs, &self.entries, &self.added, &self.removed, &self.changed, &self.errors] {
            c.store(0, Ordering::Relaxed);
        }
    }
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            running: self.running.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            entries: self.entries.load(Ordering::Relaxed),
            added: self.added.load(Ordering::Relaxed),
            removed: self.removed.load(Ordering::Relaxed),
            changed: self.changed.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            current: self.current.lock().map(|s| s.clone()).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    pub running: bool,
    pub dirs: u64,
    pub entries: u64,
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
    pub errors: u64,
    pub current: String,
}

/// Called for every folder the scan visits (watchers register here).
pub type DirHook = Arc<dyn Fn(u32, &Path) + Send + Sync>;

pub struct Scanner {
    pub index: Arc<RwLock<Index>>,
    pub excludes: Arc<Excludes>,
    pub roots: Vec<PathBuf>,
    pub progress: Arc<Progress>,
    pub cancel: Arc<AtomicBool>,
    pub on_dir: Option<DirHook>,
    pub threads: usize,
    /// Lower the worker threads' CPU and I/O priority.
    pub background: bool,
}

#[derive(Debug, Clone)]
struct Job {
    id: u32,
    path: PathBuf,
    key: u64,
    /// Newly inserted: no indexed children to compare against.
    fresh: bool,
    /// Also reconcile existing subfolders (full rescans), not only new ones.
    recurse: bool,
    dev: u64,
}

/// One directory entry as listed.
struct Item {
    name: OsString,
    meta: Meta,
    dev: u64,
    ino: u64,
}

/// Where a reconciling job finds a folder's indexed children.
enum Children {
    None,
    /// Compressed parent → children table built when the scan started.
    Table {
        offsets: Vec<u32>,
        ids: Vec<u32>,
    },
    Map(IdMap<Vec<u32>>),
}

impl Children {
    fn table(ix: &Index) -> Children {
        let n = ix.len();
        let mut counts = vec![0u32; n + 1];
        for i in 0..n {
            let p = ix.parent[i];
            if p != NONE && ix.flags[i] & flag::DELETED == 0 {
                counts[p as usize + 1] += 1;
            }
        }
        for i in 1..=n {
            counts[i] += counts[i - 1];
        }
        let mut fill = counts.clone();
        let mut ids = vec![0u32; counts[n] as usize];
        for i in 0..n {
            let p = ix.parent[i];
            if p != NONE && ix.flags[i] & flag::DELETED == 0 {
                ids[fill[p as usize] as usize] = i as u32;
                fill[p as usize] += 1;
            }
        }
        Children::Table { offsets: counts, ids }
    }

    fn of(&self, ix: &Index, dir: u32) -> Vec<u32> {
        match self {
            Children::None => Vec::new(),
            Children::Table { offsets, ids } => {
                let d = dir as usize;
                if d + 1 >= offsets.len() {
                    return Vec::new();
                }
                ids[offsets[d] as usize..offsets[d + 1] as usize].iter().copied().filter(|&c| !ix.is_deleted(c)).collect()
            }
            Children::Map(m) => m.get(&dir).map(|v| v.iter().copied().filter(|&c| !ix.is_deleted(c)).collect()).unwrap_or_default(),
        }
    }
}

struct Queue {
    jobs: Mutex<(Vec<Job>, usize)>, // pending, in-flight
    cv: Condvar,
}

#[cfg(unix)]
fn item_meta(md: &std::fs::Metadata, name: &[u8]) -> (Meta, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let ft = md.file_type();
    let mut flags = 0;
    if ft.is_dir() {
        flags |= flag::DIR;
    }
    if ft.is_symlink() {
        flags |= flag::SYMLINK;
    }
    if name.first() == Some(&b'.') {
        flags |= flag::HIDDEN;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt as _;
        if md.st_flags() & 0x8000 != 0 {
            flags |= flag::HIDDEN; // UF_HIDDEN
        }
    }
    let size = if ft.is_dir() { 0 } else { md.size() };
    (Meta { size, mtime: md.mtime(), flags }, md.dev(), md.ino())
}

#[cfg(windows)]
fn item_meta(md: &std::fs::Metadata, name: &[u8]) -> (Meta, u64, u64) {
    use std::os::windows::fs::MetadataExt;
    let ft = md.file_type();
    let mut flags = 0;
    if ft.is_dir() {
        flags |= flag::DIR;
    }
    if ft.is_symlink() || md.file_attributes() & 0x400 != 0 {
        flags |= flag::SYMLINK; // reparse point: never followed
    }
    if name.first() == Some(&b'.') || md.file_attributes() & 0x2 != 0 {
        flags |= flag::HIDDEN;
    }
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
    let size = if ft.is_dir() { 0 } else { md.len() };
    (Meta { size, mtime, flags }, 0, 0)
}

/// Metadata of one path (not following symlinks).
pub fn stat(path: &Path) -> std::io::Result<(Meta, u64, u64)> {
    let md = std::fs::symlink_metadata(path)?;
    let name = path.file_name().map(|n| n.as_encoded_bytes()).unwrap_or_default();
    Ok(item_meta(&md, name))
}

fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain syscalls on the calling thread.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
        // ioprio_set(IOPRIO_WHO_PROCESS, tid, IOPRIO_CLASS_BE << 13 | 7)
        libc::syscall(libc::SYS_ioprio_set, 1, tid as libc::c_long, ((2 << 13) | 7) as libc::c_long);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub dirs: u64,
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
    pub errors: u64,
    pub cancelled: bool,
}

impl Scanner {
    /// A full pass over every root: adds new roots, drops removed ones, and
    /// reconciles everything (a first crawl when the index is empty).
    pub fn full(&self) -> ScanStats {
        let mut jobs = Vec::new();
        {
            let mut ix = self.index.write().unwrap_or_else(|e| e.into_inner());
            let stale: Vec<PathBuf> = ix.roots().iter().filter(|r| !self.roots.contains(&r.path)).map(|r| r.path.clone()).collect();
            for p in stale {
                ix.remove_root(&p);
            }
            for root in &self.roots {
                let Ok((meta, dev, _)) = stat_dir(root) else {
                    // A missing root (unplugged drive) keeps nothing.
                    if let Some(r) = ix.roots().iter().find(|r| &r.path == root).cloned() {
                        let kids: Vec<u32> = (r.id + 1..ix.len() as u32).filter(|&i| ix.parent(i) == r.id).collect();
                        ix.remove_many(&kids);
                    }
                    self.progress.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let existed = ix.roots().iter().any(|r| &r.path == root && !ix.is_deleted(r.id));
                let id = ix.add_root(root, Meta { flags: 0, ..meta });
                ix.set_meta(id, 0, meta.mtime);
                jobs.push(Job { id, path: root.clone(), key: key_root(root), fresh: !existed, recurse: true, dev });
            }
        }
        let children = if jobs.iter().any(|j| !j.fresh) {
            let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
            Children::table(&ix)
        } else {
            Children::None
        };
        self.run(jobs, children)
    }

    /// Reconciles specific folders (not recursing into existing subfolders;
    /// new subfolders are crawled).
    pub fn dirs(&self, dirs: &[(u32, PathBuf)]) -> ScanStats {
        let (jobs, children) = {
            let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
            let mut set = IdSet::default();
            let mut jobs = Vec::new();
            for (id, path) in dirs {
                if (*id as usize) < ix.len() && !ix.is_deleted(*id) && ix.is_dir(*id) && set.insert(*id) {
                    let dev = stat_dir(path).map(|(_, d, _)| d).unwrap_or(0);
                    jobs.push(Job { id: *id, path: path.clone(), key: ix.key(*id), fresh: false, recurse: false, dev });
                }
            }
            let ch = ix.children_of(&set);
            (jobs, Children::Map(ch))
        };
        self.run(jobs, children)
    }

    fn run(&self, jobs: Vec<Job>, children: Children) -> ScanStats {
        if jobs.is_empty() {
            return ScanStats::default();
        }
        let before = self.progress.snapshot();
        let queue = Arc::new(Queue { jobs: Mutex::new((jobs, 0)), cv: Condvar::new() });
        let visited: Mutex<HashSet<(u64, u64)>> = Mutex::new(HashSet::new());
        let children = &children;
        let threads = self.threads.max(1);
        let active = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for t in 0..threads {
                let queue = queue.clone();
                let visited = &visited;
                let active = &active;
                std::thread::Builder::new()
                    .name(format!("find-scan-{t}"))
                    .spawn_scoped(s, move || {
                        if self.background {
                            lower_priority();
                        }
                        loop {
                            let job = {
                                let mut q = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                                loop {
                                    if self.cancel.load(Ordering::Relaxed) {
                                        q.0.clear();
                                    }
                                    if let Some(j) = q.0.pop() {
                                        q.1 += 1;
                                        break Some(j);
                                    }
                                    if q.1 == 0 {
                                        break None;
                                    }
                                    q = queue.cv.wait(q).unwrap_or_else(|e| e.into_inner());
                                }
                            };
                            let Some(job) = job else {
                                queue.cv.notify_all();
                                break;
                            };
                            active.fetch_add(1, Ordering::Relaxed);
                            let more = self.process(&job, children, visited);
                            active.fetch_sub(1, Ordering::Relaxed);
                            let mut q = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                            q.0.extend(more);
                            q.1 -= 1;
                            queue.cv.notify_all();
                        }
                    })
                    .expect("spawn scan thread");
            }
        });
        let after = self.progress.snapshot();
        ScanStats {
            dirs: after.dirs - before.dirs,
            added: after.added - before.added,
            removed: after.removed - before.removed,
            changed: after.changed - before.changed,
            errors: after.errors - before.errors,
            cancelled: self.cancel.load(Ordering::Relaxed),
        }
    }

    /// Lists one folder, applies the differences, returns subfolder jobs.
    fn process(&self, job: &Job, children: &Children, visited: &Mutex<HashSet<(u64, u64)>>) -> Vec<Job> {
        let p = &self.progress;
        let n = p.dirs.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(256) {
            if let Ok(mut c) = p.current.lock() {
                *c = job.path.display().to_string();
            }
        }
        if let Some(hook) = &self.on_dir {
            hook(job.id, &job.path);
        }
        let mut items: Vec<Item> = Vec::new();
        let mut gone = false;
        match std::fs::read_dir(&job.path) {
            Ok(rd) => {
                for e in rd {
                    let Ok(e) = e else {
                        p.errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    let name = e.file_name();
                    let nb = name.as_encoded_bytes();
                    if self.excludes.name_excluded(nb) {
                        continue;
                    }
                    let md = match e.metadata() {
                        Ok(m) => m,
                        Err(_) => {
                            p.errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };
                    let (meta, dev, ino) = item_meta(&md, nb);
                    if meta.flags & flag::DIR != 0 {
                        let child = job.path.join(&name);
                        if self.excludes.path_excluded(&child, &self.roots) {
                            continue;
                        }
                        // A different device is a mount point: skip network filesystems.
                        if dev != job.dev && dev != 0 && self.excludes.network_mount(&child) {
                            continue;
                        }
                        // Another root covers this folder: it is indexed there.
                        if self.roots.iter().any(|r| r == &child) {
                            continue;
                        }
                    }
                    items.push(Item { name, meta, dev, ino });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => gone = true,
            Err(_) => {
                p.errors.fetch_add(1, Ordering::Relaxed);
                return Vec::new();
            }
        }
        p.entries.fetch_add(items.len() as u64, Ordering::Relaxed);
        let mut out = Vec::new();
        let mut ix = self.index.write().unwrap_or_else(|e| e.into_inner());
        if job.id as usize >= ix.len() || ix.is_deleted(job.id) {
            return out;
        }
        if gone {
            if ix.parent(job.id) != NONE {
                ix.remove_many(&[job.id]);
                p.removed.fetch_add(1, Ordering::Relaxed);
            }
            return out;
        }
        let existing: HashMap<Vec<u8>, u32> =
            if job.fresh { HashMap::new() } else { children.of(&ix, job.id).into_iter().map(|c| (ix.name(c).to_vec(), c)).collect() };
        let mut seen: HashSet<u32> = HashSet::with_capacity(existing.len());
        let mut remove = Vec::new();
        for it in items {
            let nb = it.name.as_encoded_bytes();
            let is_dir = it.meta.flags & flag::DIR != 0;
            if is_dir && it.ino != 0 && !visited.lock().map(|mut v| v.insert((it.dev, it.ino))).unwrap_or(true) {
                continue; // the same folder reached twice (bind mount)
            }
            let child_path = job.path.join(&it.name);
            let key = key_child(job.key, nb);
            if let Some(&id) = existing.get(nb) {
                seen.insert(id);
                let old = ix.meta(id);
                if old.flags & (flag::DIR | flag::SYMLINK) != it.meta.flags & (flag::DIR | flag::SYMLINK) {
                    // Changed kind (file ↔ folder): replace.
                    remove.push(id);
                } else {
                    if old.size != crate::index::decode_size(crate::index::encode_size(it.meta.size))
                        || old.mtime != it.meta.mtime.clamp(0, u32::MAX as i64)
                    {
                        ix.set_meta(id, it.meta.size, it.meta.mtime);
                        p.changed.fetch_add(1, Ordering::Relaxed);
                    }
                    if is_dir && job.recurse {
                        out.push(Job { id, path: child_path, key, fresh: false, recurse: true, dev: it.dev });
                    }
                    continue;
                }
            }
            let id = ix.push(job.id, nb, it.meta, key);
            p.added.fetch_add(1, Ordering::Relaxed);
            if is_dir {
                out.push(Job { id, path: child_path, key, fresh: true, recurse: true, dev: it.dev });
            }
        }
        for (_, id) in existing {
            if !seen.contains(&id) {
                remove.push(id);
            }
        }
        if !remove.is_empty() {
            p.removed.fetch_add(remove.len() as u64, Ordering::Relaxed);
            ix.remove_many(&remove);
        }
        out
    }
}

fn stat_dir(path: &Path) -> std::io::Result<(Meta, u64, u64)> {
    let md = std::fs::metadata(path)?;
    if !md.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a folder"));
    }
    let name = path.file_name().map(|n| n.as_encoded_bytes()).unwrap_or_default();
    Ok(item_meta(&md, name))
}

/// Applies watcher events: `(folder, name)` pairs that may have been
/// created, changed or removed. New folders are crawled.
pub fn apply_names(scanner: &Scanner, changes: &[(u32, Vec<u8>)]) -> ScanStats {
    if changes.is_empty() {
        return ScanStats::default();
    }
    let mut by_dir: IdMap<Vec<&[u8]>> = IdMap::default();
    for (d, n) in changes {
        by_dir.entry(*d).or_default().push(n);
    }
    // Resolve paths and current children once.
    let (dirs, kids) = {
        let ix = scanner.index.read().unwrap_or_else(|e| e.into_inner());
        let set: IdSet = by_dir.keys().copied().filter(|&d| (d as usize) < ix.len() && !ix.is_deleted(d)).collect();
        let kids = ix.children_of(&set);
        let dirs: IdMap<(PathBuf, u64)> = set.iter().map(|&d| (d, (ix.path(d), ix.key(d)))).collect();
        (dirs, kids)
    };
    let p = &scanner.progress;
    let mut new_dirs = Vec::new();
    {
        // Stat outside the lock, then apply.
        let mut ops: Vec<(u32, Vec<u8>, u64, Option<(Meta, u64)>)> = Vec::new();
        for (d, names) in &by_dir {
            let Some((dpath, dkey)) = dirs.get(d) else { continue };
            for n in names {
                if scanner.excludes.name_excluded(n) {
                    continue;
                }
                // SAFETY: names come from the OS via `as_encoded_bytes`.
                let os = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(n) };
                let path = dpath.join(os);
                let st =
                    stat(&path).ok().filter(|(m, _, _)| m.flags & flag::DIR == 0 || !scanner.excludes.path_excluded(&path, &scanner.roots));
                ops.push((*d, n.to_vec(), key_child(*dkey, n), st.map(|(m, dev, _)| (m, dev))));
            }
        }
        let mut ix = scanner.index.write().unwrap_or_else(|e| e.into_inner());
        let mut remove = Vec::new();
        let mut done: HashSet<(u32, Vec<u8>)> = HashSet::new();
        for (d, name, key, st) in ops {
            if !done.insert((d, name.clone())) || ix.is_deleted(d) {
                continue;
            }
            let existing = kids.get(&d).and_then(|v| v.iter().copied().find(|&c| !ix.is_deleted(c) && ix.name(c) == name.as_slice()));
            match (existing, st) {
                (Some(id), Some((m, _))) if ix.meta(id).flags & (flag::DIR | flag::SYMLINK) == m.flags & (flag::DIR | flag::SYMLINK) => {
                    ix.set_meta(id, m.size, m.mtime);
                    p.changed.fetch_add(1, Ordering::Relaxed);
                }
                (Some(id), Some((m, dev))) => {
                    remove.push(id);
                    let nid = ix.push(d, &name, m, key);
                    p.added.fetch_add(1, Ordering::Relaxed);
                    if m.flags & flag::DIR != 0 {
                        new_dirs.push((nid, key, dev));
                    }
                }
                (Some(id), None) => remove.push(id),
                (None, Some((m, dev))) => {
                    let nid = ix.push(d, &name, m, key);
                    p.added.fetch_add(1, Ordering::Relaxed);
                    if m.flags & flag::DIR != 0 {
                        new_dirs.push((nid, key, dev));
                    }
                }
                (None, None) => {}
            }
        }
        if !remove.is_empty() {
            p.removed.fetch_add(remove.len() as u64, Ordering::Relaxed);
            ix.remove_many(&remove);
        }
        // Paths for the new folders.
        let jobs: Vec<Job> =
            new_dirs.iter().map(|&(id, key, dev)| Job { id, path: ix.path(id), key, fresh: true, recurse: true, dev }).collect();
        drop(ix);
        if !jobs.is_empty() {
            return scanner.run(jobs, Children::None);
        }
    }
    ScanStats::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn scanner(root: &Path, index: Arc<RwLock<Index>>) -> Scanner {
        let s = Settings { roots: vec![root.display().to_string()], skip_network_mounts: false, ..Settings::default() };
        Scanner {
            index,
            excludes: Arc::new(Excludes::from_settings(&s)),
            roots: s.root_paths(),
            progress: Arc::new(Progress::default()),
            cancel: Arc::new(AtomicBool::new(false)),
            on_dir: None,
            threads: 3,
            background: false,
        }
    }

    fn touch(p: &Path, bytes: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn crawl_reconcile_and_excludes() {
        let dir = crate::test_dir("scan");
        let root = dir.join("home");
        touch(&root.join("Docs/report.pdf"), 10);
        touch(&root.join("Docs/deep/a/b/c.txt"), 3);
        touch(&root.join("proj/node_modules/pkg/index.js"), 1);
        touch(&root.join("proj/.git/HEAD"), 1);
        touch(&root.join("proj/src/main.rs"), 5);
        touch(&root.join(".config/app.toml"), 1);
        touch(&root.join(".snapshots/1/snapshot/Docs/report.pdf"), 10);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("Docs"), root.join("DocsLink")).unwrap();

        let index = Arc::new(RwLock::new(Index::new()));
        let sc = scanner(&root, index.clone());
        let st = sc.full();
        assert_eq!(st.errors, 0);
        {
            let ix = index.read().unwrap();
            assert!(ix.lookup(&root.join("Docs/deep/a/b/c.txt")).is_some());
            assert!(ix.lookup(&root.join("proj/node_modules")).is_none());
            assert!(ix.lookup(&root.join("proj/.git")).is_none());
            assert!(ix.lookup(&root.join(".snapshots")).is_none());
            let cfg = ix.lookup(&root.join(".config/app.toml")).unwrap();
            assert!(ix.flags(cfg) & flag::HIDDEN != 0);
            #[cfg(unix)]
            {
                let l = ix.lookup(&root.join("DocsLink")).unwrap();
                assert!(ix.flags(l) & flag::SYMLINK != 0);
                assert!(ix.lookup(&root.join("DocsLink/report.pdf")).is_none(), "symlinks are not followed");
            }
            assert_eq!(ix.size(ix.lookup(&root.join("Docs/report.pdf")).unwrap()), 10);
        }
        let live_before = index.read().unwrap().live();

        // Change things on disk and reconcile.
        std::fs::remove_dir_all(root.join("Docs/deep")).unwrap();
        touch(&root.join("Docs/report.pdf"), 20);
        touch(&root.join("new/thing.md"), 1);
        let st = sc.full();
        assert!(st.removed >= 1 && st.added >= 2 && st.changed >= 1, "{st:?}");
        {
            let ix = index.read().unwrap();
            assert!(ix.lookup(&root.join("Docs/deep")).is_none());
            assert!(ix.lookup(&root.join("Docs/deep/a/b/c.txt")).is_none());
            assert!(ix.lookup(&root.join("new/thing.md")).is_some());
            assert_eq!(ix.size(ix.lookup(&root.join("Docs/report.pdf")).unwrap()), 20);
            assert_eq!(ix.live(), live_before - 4 + 2);
        }
        // A third pass changes nothing.
        let st = sc.full();
        assert_eq!((st.added, st.removed, st.changed), (0, 0, 0));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn watcher_style_updates() {
        let dir = crate::test_dir("scan-names");
        let root = dir.join("r");
        touch(&root.join("a.txt"), 1);
        let index = Arc::new(RwLock::new(Index::new()));
        let sc = scanner(&root, index.clone());
        sc.full();
        let rid = index.read().unwrap().dir_at(&root).unwrap();
        touch(&root.join("b.txt"), 2);
        touch(&root.join("sub/c.txt"), 3);
        std::fs::remove_file(root.join("a.txt")).unwrap();
        apply_names(&sc, &[(rid, b"a.txt".to_vec()), (rid, b"b.txt".to_vec()), (rid, b"sub".to_vec()), (rid, b"b.txt".to_vec())]);
        let ix = index.read().unwrap();
        assert!(ix.lookup(&root.join("a.txt")).is_none());
        assert!(ix.lookup(&root.join("b.txt")).is_some());
        assert!(ix.lookup(&root.join("sub/c.txt")).is_some());
        assert_eq!(ix.live(), 4); // root, b.txt, sub, c.txt
        drop(ix);
        // Single-folder reconcile catches a change with no event.
        touch(&root.join("d.txt"), 1);
        sc.dirs(&[(rid, root.clone())]);
        assert!(index.read().unwrap().lookup(&root.join("d.txt")).is_some());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn roots_added_and_removed() {
        let dir = crate::test_dir("scan-roots");
        touch(&dir.join("one/x.txt"), 1);
        touch(&dir.join("two/y.txt"), 1);
        let index = Arc::new(RwLock::new(Index::new()));
        let mut sc = scanner(&dir.join("one"), index.clone());
        sc.full();
        sc.roots = vec![dir.join("two")];
        sc.full();
        let ix = index.read().unwrap();
        assert!(ix.lookup(&dir.join("one/x.txt")).is_none());
        assert!(ix.lookup(&dir.join("two/y.txt")).is_some());
        std::fs::remove_dir_all(dir).ok();
    }
}
