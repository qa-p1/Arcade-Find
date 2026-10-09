//! Live updates from the OS.
//!
//! Linux uses inotify directly: one watch per indexed folder, added as the
//! crawl visits it. When `fs.inotify.max_user_watches` runs out, watching
//! stops growing, the status says so honestly (with the command to raise the
//! limit; nothing is changed automatically) and the periodic rescan covers
//! the unwatched folders more often. Windows (ReadDirectoryChangesW) and
//! macOS (FSEvents) watch each root recursively through `notify`.
//!
//! Events are reduced to `(folder id, name)` pairs and handed to a sink;
//! "rescan everything" is requested when the OS reports lost events.

use std::path::Path;
#[cfg(not(target_os = "linux"))]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::index::Index;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchMode {
    /// Not started yet.
    Off,
    /// Every indexed folder is watched.
    Full,
    /// The OS limit was reached; some folders rely on periodic rescans.
    Limited,
    /// Watching isn't possible here; rescans only.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatus {
    pub mode: WatchMode,
    pub watched: u64,
    pub unwatched: u64,
    /// `fs.inotify.max_user_watches` on Linux.
    pub limit: Option<u64>,
    pub detail: String,
    /// What the user can run to raise the limit (never run by the app).
    pub fix_command: Option<String>,
}

/// Where watchers deliver changes.
pub trait Sink: Send + Sync + 'static {
    /// Something named `name` in folder `dir` changed.
    fn changed(&self, dir: u32, name: Vec<u8>);
    /// A folder's contents changed in ways we can't itemize.
    fn rescan_dir(&self, dir: u32);
    /// Events were lost; reconcile everything.
    fn rescan_all(&self);
}

pub fn read_inotify_limit() -> Option<u64> {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches").ok().and_then(|s| s.trim().parse().ok())
}

pub fn fix_command(new_limit: u64) -> String {
    format!("echo fs.inotify.max_user_watches={new_limit} | sudo tee /etc/sysctl.d/90-arcade-find.conf && sudo sysctl --system")
}

#[cfg(target_os = "linux")]
pub use linux::Watcher;
#[cfg(not(target_os = "linux"))]
pub use portable::Watcher;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::index::{IdMap, NONE};
    use inotify::{Inotify, WatchDescriptor, WatchMask};
    use std::collections::HashMap;

    struct State {
        watches: inotify::Watches,
        by_wd: HashMap<WatchDescriptor, u32>,
        by_dir: IdMap<WatchDescriptor>,
        full: bool,
        error: Option<String>,
    }

    pub struct Watcher {
        state: Mutex<State>,
        watched: AtomicU64,
        unwatched: AtomicU64,
        limit: Option<u64>,
        stopped: Arc<AtomicBool>,
    }

    fn mask() -> WatchMask {
        WatchMask::CREATE
            | WatchMask::DELETE
            | WatchMask::MOVED_FROM
            | WatchMask::MOVED_TO
            | WatchMask::CLOSE_WRITE
            | WatchMask::ATTRIB
            | WatchMask::DELETE_SELF
            | WatchMask::MOVE_SELF
            | WatchMask::ONLYDIR
            | WatchMask::DONT_FOLLOW
            | WatchMask::EXCL_UNLINK
    }

    impl Watcher {
        /// Starts the reader thread. `None` if inotify can't be used.
        pub fn start(sink: Arc<dyn Sink>, _index: Arc<RwLock<Index>>) -> Option<Arc<Watcher>> {
            let ino = Inotify::init().ok()?;
            let w = Arc::new(Watcher {
                state: Mutex::new(State {
                    watches: ino.watches(),
                    by_wd: HashMap::new(),
                    by_dir: IdMap::default(),
                    full: false,
                    error: None,
                }),
                watched: AtomicU64::new(0),
                unwatched: AtomicU64::new(0),
                limit: read_inotify_limit(),
                stopped: Arc::new(AtomicBool::new(false)),
            });
            let me = Arc::downgrade(&w);
            let stopped = w.stopped.clone();
            let mut ino = ino;
            std::thread::Builder::new()
                .name("find-inotify".into())
                .spawn(move || {
                    let mut buf = vec![0u8; 64 * 1024];
                    // Blocks in read(2) until the kernel has events: no polling.
                    while let Ok(events) = ino.read_events_blocking(&mut buf) {
                        if stopped.load(Ordering::Relaxed) {
                            break;
                        }
                        let Some(w) = me.upgrade() else { break };
                        for ev in events {
                            if ev.mask.contains(inotify::EventMask::Q_OVERFLOW) {
                                sink.rescan_all();
                                continue;
                            }
                            let dir = {
                                let st = w.state.lock().unwrap_or_else(|e| e.into_inner());
                                st.by_wd.get(&ev.wd).copied()
                            };
                            let Some(dir) = dir else { continue };
                            if ev.mask.contains(inotify::EventMask::IGNORED) {
                                let mut st = w.state.lock().unwrap_or_else(|e| e.into_inner());
                                st.by_wd.remove(&ev.wd);
                                if st.by_dir.get(&dir) == Some(&ev.wd) {
                                    st.by_dir.remove(&dir);
                                }
                                w.watched.fetch_sub(1, Ordering::Relaxed);
                                continue;
                            }
                            if ev.mask.intersects(inotify::EventMask::DELETE_SELF | inotify::EventMask::MOVE_SELF) {
                                sink.rescan_dir(dir);
                                continue;
                            }
                            match ev.name {
                                Some(n) => sink.changed(dir, n.as_encoded_bytes().to_vec()),
                                None => sink.rescan_dir(dir),
                            }
                        }
                    }
                })
                .ok()?;
            Some(w)
        }

        /// Watches folder `dir` at `path` (called by the crawler).
        pub fn add(&self, dir: u32, path: &Path) {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.full {
                self.unwatched.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if st.by_dir.contains_key(&dir) {
                return;
            }
            match st.watches.add(path, mask()) {
                Ok(wd) => {
                    if let Some(old) = st.by_wd.insert(wd.clone(), dir) {
                        // The same inode under a new id (moved folder): keep the newest.
                        if st.by_dir.get(&old) == Some(&wd) {
                            st.by_dir.remove(&old);
                        }
                    } else {
                        self.watched.fetch_add(1, Ordering::Relaxed);
                    }
                    st.by_dir.insert(dir, wd);
                }
                Err(e) if e.raw_os_error() == Some(libc::ENOSPC) => {
                    st.full = true;
                    self.unwatched.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) if e.raw_os_error() == Some(libc::ENOENT) || e.raw_os_error() == Some(libc::ENOTDIR) => {}
                Err(e) => {
                    st.error = Some(e.to_string());
                    self.unwatched.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        /// Stops watching folders that left the index.
        pub fn prune(&self, index: &Index) {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let dead: Vec<u32> = st.by_dir.keys().copied().filter(|&d| d as usize >= index.len() || index.is_deleted(d)).collect();
            for d in dead {
                if let Some(wd) = st.by_dir.remove(&d) {
                    st.by_wd.remove(&wd);
                    let _ = st.watches.remove(wd);
                    self.watched.fetch_sub(1, Ordering::Relaxed);
                }
            }
        }

        /// Ids changed (compaction): remap every watch.
        pub fn remap(&self, old_to_path: impl Fn(u32) -> Option<u32>) {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let pairs: Vec<(WatchDescriptor, u32)> = st.by_wd.iter().map(|(w, d)| (w.clone(), *d)).collect();
            st.by_wd.clear();
            st.by_dir = IdMap::default();
            for (wd, d) in pairs {
                match old_to_path(d) {
                    Some(nd) if nd != NONE => {
                        st.by_wd.insert(wd.clone(), nd);
                        st.by_dir.insert(nd, wd);
                    }
                    _ => {
                        let _ = st.watches.remove(wd);
                        self.watched.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
        }

        /// Forget everything (before a full re-crawl re-adds watches).
        pub fn reset_unwatched(&self) {
            self.unwatched.store(0, Ordering::Relaxed);
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.full = false;
        }

        pub fn status(&self) -> WatchStatus {
            let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let watched = self.watched.load(Ordering::Relaxed);
            let unwatched = self.unwatched.load(Ordering::Relaxed);
            let limited = st.full || unwatched > 0;
            let want = (watched + unwatched).max(1);
            let suggestion = (want * 2).next_power_of_two().max(524_288);
            WatchStatus {
                mode: if limited { WatchMode::Limited } else { WatchMode::Full },
                watched,
                unwatched,
                limit: self.limit,
                detail: if st.full {
                    format!("Live updates cover {watched} of {} folders: the inotify watch limit was reached. The rest are rescanned periodically.", watched + unwatched)
                } else if let Some(e) = &st.error {
                    format!("Some folders can't be watched ({e}); they are rescanned periodically.")
                } else {
                    format!("Watching {watched} folders.")
                },
                fix_command: st.full.then(|| fix_command(suggestion)),
            }
        }

        pub fn stop(&self) {
            self.stopped.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod portable {
    use super::*;
    use notify::{EventKind, RecursiveMode, Watcher as _};

    pub struct Watcher {
        inner: Mutex<Option<notify::RecommendedWatcher>>,
        index: Arc<RwLock<Index>>,
        roots: Mutex<Vec<PathBuf>>,
        error: Mutex<Option<String>>,
    }

    impl Watcher {
        pub fn start(sink: Arc<dyn Sink>, index: Arc<RwLock<Index>>) -> Option<Arc<Watcher>> {
            let ix = index.clone();
            let handler = move |res: notify::Result<notify::Event>| {
                let Ok(ev) = res else {
                    sink.rescan_all();
                    return;
                };
                if ev.need_rescan() {
                    sink.rescan_all();
                    return;
                }
                if matches!(ev.kind, EventKind::Access(_)) {
                    return;
                }
                let ix = ix.read().unwrap_or_else(|e| e.into_inner());
                for p in &ev.paths {
                    let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else { continue };
                    if let Some(dir) = ix.dir_at(parent) {
                        sink.changed(dir, name.as_encoded_bytes().to_vec());
                    } else if let Some(dir) = ix.dir_at(p) {
                        sink.rescan_dir(dir);
                    }
                }
            };
            let w = notify::recommended_watcher(handler).ok()?;
            Some(Arc::new(Watcher { inner: Mutex::new(Some(w)), index, roots: Mutex::new(Vec::new()), error: Mutex::new(None) }))
        }

        /// Roots are watched recursively; other folders need nothing.
        pub fn add(&self, dir: u32, path: &Path) {
            let is_root = self.index.read().map(|ix| ix.roots().iter().any(|r| r.id == dir)).unwrap_or(false);
            if !is_root {
                return;
            }
            let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
            if roots.iter().any(|r| r == path) {
                return;
            }
            if let Some(w) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                match w.watch(path, RecursiveMode::Recursive) {
                    Ok(()) => roots.push(path.to_path_buf()),
                    Err(e) => *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string()),
                }
            }
        }

        pub fn prune(&self, index: &Index) {
            let live: Vec<PathBuf> = index.roots().iter().map(|r| r.path.clone()).collect();
            let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(w) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                for r in roots.iter().filter(|r| !live.contains(r)) {
                    let _ = w.unwatch(r);
                }
            }
            roots.retain(|r| live.contains(r));
        }

        pub fn remap(&self, _f: impl Fn(u32) -> Option<u32>) {}

        pub fn reset_unwatched(&self) {}

        pub fn status(&self) -> WatchStatus {
            let roots = self.roots.lock().map(|r| r.len() as u64).unwrap_or(0);
            let err = self.error.lock().ok().and_then(|e| e.clone());
            WatchStatus {
                mode: if err.is_some() { WatchMode::Limited } else { WatchMode::Full },
                watched: roots,
                unwatched: 0,
                limit: None,
                detail: match err {
                    Some(e) => format!("Some folders can't be watched ({e}); they are rescanned periodically."),
                    None => format!("Watching {roots} locations."),
                },
                fix_command: None,
            }
        }

        pub fn stop(&self) {
            self.inner.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
    }
}

/// A sink that collects changes until the engine applies them.
#[derive(Default)]
pub struct Pending {
    pub names: Mutex<Vec<(u32, Vec<u8>)>>,
    pub dirs: Mutex<Vec<u32>>,
    pub all: AtomicBool,
    pub count: AtomicU64,
}

impl Pending {
    pub fn take(&self) -> (Vec<(u32, Vec<u8>)>, Vec<u32>, bool) {
        let names = std::mem::take(&mut *self.names.lock().unwrap_or_else(|e| e.into_inner()));
        let dirs = std::mem::take(&mut *self.dirs.lock().unwrap_or_else(|e| e.into_inner()));
        let all = self.all.swap(false, Ordering::SeqCst);
        self.count.store(0, Ordering::SeqCst);
        (names, dirs, all)
    }

    pub fn is_empty(&self) -> bool {
        self.count.load(Ordering::SeqCst) == 0 && !self.all.load(Ordering::SeqCst)
    }
}
