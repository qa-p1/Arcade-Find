//! The engine: owns the index, runs crawls and rescans, applies watcher
//! events, and saves the index. One maintenance thread sleeps until there
//! is work (an event, a save deadline, or the next periodic rescan); there
//! are no polling timers while idle.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::exclude::Excludes;
use crate::index::{IdMap, IdSet, Index, NONE};
use crate::persist;
use crate::query::Query;
use crate::scan::{apply_names, Progress, ProgressSnapshot, ScanStats, Scanner};
use crate::search::{search, Hit, Scored, SearchOptions, SearchResult};
use crate::settings::Settings;
use crate::watch::{Pending, Sink, WatchMode, WatchStatus, Watcher};

/// How long watcher events are gathered before they're applied.
const DEBOUNCE: Duration = Duration::from_millis(150);
/// Save a changed index at most this often.
const SAVE_DELAY: Duration = Duration::from_secs(120);
/// Rescan interval while watching is limited by the OS.
const LIMITED_RESCAN: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Loading the saved index.
    Loading,
    /// First crawl (no saved index): results grow as it runs.
    Crawling,
    /// Checking the saved index against the disk.
    Reconciling,
    Ready,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub phase: Phase,
    pub entries: u64,
    pub progress: ProgressSnapshot,
    pub watch: WatchStatus,
    pub loaded_from_disk: bool,
    pub load_ms: u64,
    pub last_full_scan: i64,
    pub last_scan_ms: u64,
    pub last_scan: Option<ScanSummary>,
    pub index_bytes: u64,
    pub index_file: PathBuf,
    pub generation: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub dirs: u64,
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
    pub errors: u64,
}

impl From<&ScanStats> for ScanSummary {
    fn from(s: &ScanStats) -> Self {
        ScanSummary { dirs: s.dirs, added: s.added, removed: s.removed, changed: s.changed, errors: s.errors }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The index changed (results may be stale).
    Changed,
    /// Crawl progress (at most every 250 ms while scanning).
    Progress,
    /// A crawl or rescan finished.
    ScanDone,
    /// Watch or error status changed.
    Status,
}

pub type Listener = Arc<dyn Fn(EngineEvent) + Send + Sync>;

#[derive(Default)]
struct Work {
    rescan_all: bool,
    settings: Option<Settings>,
    save_now: bool,
    quit: bool,
}

struct State {
    phase: Phase,
    loaded: bool,
    load_ms: u64,
    last_full: i64,
    last_full_at: Option<Instant>,
    last_scan_ms: u64,
    last_scan: Option<ScanSummary>,
    dirty_since: Option<Instant>,
    saved_generation: u64,
    last_error: Option<String>,
    fingerprint: u64,
}

pub struct Engine {
    pub index: Arc<RwLock<Index>>,
    settings: RwLock<Settings>,
    excludes: RwLock<Arc<Excludes>>,
    progress: Arc<Progress>,
    watcher: Mutex<Option<Arc<Watcher>>>,
    pending: Arc<Pending>,
    work: Mutex<Work>,
    wake: Condvar,
    state: Mutex<State>,
    cancel: Arc<AtomicBool>,
    listener: Listener,
    index_file: PathBuf,
    scan_threads: usize,
    events_applied: AtomicU64,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

struct EngineSink {
    pending: Arc<Pending>,
    engine: std::sync::Weak<Engine>,
}

impl EngineSink {
    fn poke(&self) {
        self.pending.count.fetch_add(1, Ordering::SeqCst);
        if let Some(e) = self.engine.upgrade() {
            let _g = e.work.lock();
            e.wake.notify_all();
        }
    }
}

impl Sink for EngineSink {
    fn changed(&self, dir: u32, name: Vec<u8>) {
        if let Ok(mut n) = self.pending.names.lock() {
            if n.len() < 200_000 {
                n.push((dir, name));
            } else {
                self.pending.all.store(true, Ordering::SeqCst);
            }
        }
        self.poke();
    }
    fn rescan_dir(&self, dir: u32) {
        if let Ok(mut d) = self.pending.dirs.lock() {
            d.push(dir);
        }
        self.poke();
    }
    fn rescan_all(&self) {
        self.pending.all.store(true, Ordering::SeqCst);
        self.poke();
    }
}

impl Engine {
    /// Loads the saved index (if any) synchronously — results are usable as
    /// soon as this returns — then starts the watcher and the maintenance
    /// thread, which reconciles or crawls in the background.
    pub fn start(index_file: PathBuf, settings: Settings, listener: Listener) -> Arc<Engine> {
        let t0 = Instant::now();
        let fingerprint = settings.index_fingerprint();
        let (index, loaded, last_full, err) = match persist::load(&index_file) {
            Ok((ix, h)) => (ix, true, h.complete_at, None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Index::new(), false, 0, None),
            Err(e) => (Index::new(), false, 0, Some(format!("The saved index couldn't be read ({e}); rebuilding it."))),
        };
        let load_ms = t0.elapsed().as_millis() as u64;
        let saved_generation = index.generation();
        let excludes = Arc::new(Excludes::from_settings(&settings));
        let engine = Arc::new(Engine {
            index: Arc::new(RwLock::new(index)),
            settings: RwLock::new(settings),
            excludes: RwLock::new(excludes),
            progress: Arc::new(Progress::default()),
            watcher: Mutex::new(None),
            pending: Arc::new(Pending::default()),
            work: Mutex::new(Work::default()),
            wake: Condvar::new(),
            state: Mutex::new(State {
                phase: if loaded { Phase::Reconciling } else { Phase::Crawling },
                loaded,
                load_ms,
                last_full,
                last_full_at: None,
                last_scan_ms: 0,
                last_scan: None,
                dirty_since: None,
                saved_generation,
                last_error: err,
                fingerprint,
            }),
            cancel: Arc::new(AtomicBool::new(false)),
            listener,
            index_file,
            scan_threads: std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 6),
            events_applied: AtomicU64::new(0),
            thread: Mutex::new(None),
        });
        let sink = Arc::new(EngineSink { pending: engine.pending.clone(), engine: Arc::downgrade(&engine) });
        *engine.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Watcher::start(sink, engine.index.clone());
        let me = engine.clone();
        let h = std::thread::Builder::new().name("find-engine".into()).spawn(move || me.main_loop()).expect("spawn engine thread");
        *engine.thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(h);
        engine
    }

    /// An engine over an already-built index with no threads (one-shot searches, tests).
    pub fn offline(index: Index, settings: Settings) -> Arc<Engine> {
        let generation = index.generation();
        Arc::new(Engine {
            excludes: RwLock::new(Arc::new(Excludes::from_settings(&settings))),
            index: Arc::new(RwLock::new(index)),
            settings: RwLock::new(settings),
            progress: Arc::new(Progress::default()),
            watcher: Mutex::new(None),
            pending: Arc::new(Pending::default()),
            work: Mutex::new(Work::default()),
            wake: Condvar::new(),
            state: Mutex::new(State {
                phase: Phase::Ready,
                loaded: true,
                load_ms: 0,
                last_full: 0,
                last_full_at: None,
                last_scan_ms: 0,
                last_scan: None,
                dirty_since: None,
                saved_generation: generation,
                last_error: None,
                fingerprint: 0,
            }),
            cancel: Arc::new(AtomicBool::new(false)),
            listener: Arc::new(|_| {}),
            index_file: PathBuf::new(),
            scan_threads: 1,
            events_applied: AtomicU64::new(0),
            thread: Mutex::new(None),
        })
    }

    fn emit(&self, e: EngineEvent) {
        (self.listener)(e);
    }

    fn scanner(&self) -> Scanner {
        let settings = self.settings.read().unwrap_or_else(|e| e.into_inner()).clone();
        let watcher = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Scanner {
            index: self.index.clone(),
            excludes: self.excludes.read().unwrap_or_else(|e| e.into_inner()).clone(),
            roots: settings.root_paths(),
            progress: self.progress.clone(),
            cancel: self.cancel.clone(),
            on_dir: watcher.map(|w| Arc::new(move |id: u32, p: &Path| w.add(id, p)) as crate::scan::DirHook),
            threads: self.scan_threads,
            background: true,
        }
    }

    fn set_phase(&self, p: Phase) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).phase = p;
    }

    fn mark_dirty(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.dirty_since.is_none() {
            st.dirty_since = Some(Instant::now());
        }
    }

    fn full_scan(self: &Arc<Engine>) {
        self.progress.reset();
        self.progress.running.store(true, Ordering::SeqCst);
        if let Some(w) = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            w.reset_unwatched();
        }
        // While the scan runs, report progress every 250 ms (only while scanning).
        let ticker = {
            let me = Arc::downgrade(self);
            let progress = self.progress.clone();
            std::thread::Builder::new().name("find-progress".into()).spawn(move || {
                while progress.running.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(250));
                    match me.upgrade() {
                        Some(e) => {
                            e.emit(EngineEvent::Progress);
                            e.emit(EngineEvent::Changed);
                        }
                        None => break,
                    }
                }
            })
        };
        let t0 = Instant::now();
        let stats = self.scanner().full();
        self.progress.running.store(false, Ordering::SeqCst);
        if let Ok(t) = ticker {
            let _ = t.join();
        }
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.last_scan_ms = t0.elapsed().as_millis() as u64;
            st.last_scan = Some(ScanSummary::from(&stats));
            if !stats.cancelled {
                st.last_full = crate::now_secs();
                st.last_full_at = Some(Instant::now());
            }
            st.phase = Phase::Ready;
        }
        if let Some(w) = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
            w.prune(&ix);
        }
        self.maybe_compact();
        if !stats.cancelled {
            self.save();
        }
        self.emit(EngineEvent::Changed);
        self.emit(EngineEvent::ScanDone);
        self.emit(EngineEvent::Status);
    }

    fn apply_pending(self: &Arc<Engine>) {
        let (names, dirs, all) = self.pending.take();
        if all {
            self.full_scan();
            return;
        }
        if names.is_empty() && dirs.is_empty() {
            return;
        }
        let sc = self.scanner();
        let n = names.len() + dirs.len();
        let mut stats = apply_names(&sc, &names);
        if !dirs.is_empty() {
            let paths: Vec<(u32, PathBuf)> = {
                let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
                let set: IdSet = dirs.iter().copied().filter(|&d| (d as usize) < ix.len() && !ix.is_deleted(d)).collect();
                set.into_iter().map(|d| (d, ix.path(d))).collect()
            };
            let s2 = sc.dirs(&paths);
            stats.added += s2.added;
            stats.removed += s2.removed;
            stats.changed += s2.changed;
        }
        self.events_applied.fetch_add(n as u64, Ordering::Relaxed);
        if let Some(w) = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if stats.removed > 0 {
                let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
                w.prune(&ix);
            }
        }
        self.maybe_compact();
        self.mark_dirty();
        self.emit(EngineEvent::Changed);
    }

    fn maybe_compact(&self) {
        let mut ix = self.index.write().unwrap_or_else(|e| e.into_inner());
        if !ix.needs_compaction() {
            return;
        }
        let remap = ix.compact();
        drop(ix);
        if let Some(w) = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            w.remap(|old| remap.get(old as usize).copied().filter(|&n| n != NONE));
        }
        self.mark_dirty();
    }

    /// Saves the index if it changed since the last save.
    pub fn save(&self) {
        if self.index_file.as_os_str().is_empty() {
            return;
        }
        let (fingerprint, last_full) = {
            let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            (st.fingerprint, st.last_full)
        };
        let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
        let generation = ix.generation();
        if self.state.lock().map(|s| s.saved_generation == generation && s.loaded).unwrap_or(false) && self.index_file.exists() {
            return;
        }
        let r = persist::save(&self.index_file, &ix, fingerprint, last_full);
        drop(ix);
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match r {
            Ok(()) => {
                st.saved_generation = generation;
                st.loaded = true;
                st.dirty_since = None;
            }
            Err(e) => st.last_error = Some(format!("Couldn't save the index: {e}")),
        }
    }

    fn rescan_interval(&self) -> Duration {
        let hours = self.settings.read().map(|s| s.rescan_hours).unwrap_or(6).max(1) as u64;
        let normal = Duration::from_secs(hours * 3600);
        let limited = self.watcher.lock().ok().and_then(|w| w.as_ref().map(|w| w.status().mode != WatchMode::Full)).unwrap_or(true);
        if limited {
            normal.min(LIMITED_RESCAN)
        } else {
            normal
        }
    }

    fn main_loop(self: Arc<Engine>) {
        self.full_scan();
        loop {
            // Decide what to do, or how long to sleep.
            let mut work = self.work.lock().unwrap_or_else(|e| e.into_inner());
            if work.quit {
                break;
            }
            if let Some(s) = work.settings.take() {
                drop(work);
                self.apply_settings_now(s);
                continue;
            }
            if std::mem::take(&mut work.rescan_all) {
                drop(work);
                self.full_scan();
                continue;
            }
            if std::mem::take(&mut work.save_now) {
                drop(work);
                self.save();
                continue;
            }
            let now = Instant::now();
            let mut sleep = Duration::from_secs(24 * 3600);
            if !self.pending.is_empty() {
                drop(work);
                std::thread::sleep(DEBOUNCE);
                self.apply_pending();
                continue;
            }
            let (dirty, last_full_at) = {
                let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
                (st.dirty_since, st.last_full_at)
            };
            if let Some(since) = dirty {
                let due = since + SAVE_DELAY;
                if now >= due {
                    drop(work);
                    self.save();
                    continue;
                }
                sleep = sleep.min(due - now);
            }
            let due = last_full_at.unwrap_or(now) + self.rescan_interval();
            if now >= due {
                drop(work);
                self.full_scan();
                continue;
            }
            sleep = sleep.min(due - now);
            let (w, _) = self.wake.wait_timeout(work, sleep).unwrap_or_else(|e| e.into_inner());
            drop(w);
        }
        if let Some(w) = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            w.stop();
        }
        self.save();
    }

    fn apply_settings_now(self: &Arc<Engine>, s: Settings) {
        let fp = s.index_fingerprint();
        let changed = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let changed = st.fingerprint != fp;
            st.fingerprint = fp;
            changed
        };
        *self.excludes.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(Excludes::from_settings(&s));
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = s;
        if changed {
            self.set_phase(Phase::Reconciling);
            self.emit(EngineEvent::Status);
            self.full_scan();
        }
    }

    /// New settings: a changed root or exclusion triggers a reconciling rescan.
    pub fn set_settings(&self, s: Settings) {
        let mut w = self.work.lock().unwrap_or_else(|e| e.into_inner());
        w.settings = Some(s);
        self.wake.notify_all();
    }

    /// Rescan everything now.
    pub fn request_rescan(&self) {
        let mut w = self.work.lock().unwrap_or_else(|e| e.into_inner());
        w.rescan_all = true;
        self.wake.notify_all();
    }

    pub fn request_save(&self) {
        let mut w = self.work.lock().unwrap_or_else(|e| e.into_inner());
        w.save_now = true;
        self.wake.notify_all();
    }

    /// Stops background work and saves the index. Blocks until done (bounded by the current folder).
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        {
            let mut w = self.work.lock().unwrap_or_else(|e| e.into_inner());
            w.quit = true;
            self.wake.notify_all();
        }
        let h = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(h) = h {
            let _ = h.join();
        } else {
            self.save();
        }
    }

    /// Tells the engine that something changed at `path` (after our own
    /// rename/trash), so results update without waiting for the watcher.
    pub fn touched(&self, path: &Path) {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else { return };
        let dir = self.index.read().ok().and_then(|ix| ix.dir_at(parent));
        if let Some(d) = dir {
            let sc = self.scanner();
            apply_names(&sc, &[(d, name.as_encoded_bytes().to_vec())]);
            self.mark_dirty();
            self.emit(EngineEvent::Changed);
        }
    }

    pub fn search(&self, q: &Query, opts: &SearchOptions) -> SearchResult {
        let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
        search(&ix, q, opts)
    }

    pub fn hits(&self, r: &[Scored]) -> Vec<Hit> {
        let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
        r.iter().filter(|s| (s.id as usize) < ix.len() && !ix.is_deleted(s.id)).map(|&s| Hit::from_index(&ix, s)).collect()
    }

    /// Search and materialize in one step, re-validating ids under one lock.
    pub fn search_hits(&self, q: &Query, opts: &SearchOptions) -> (Vec<Hit>, SearchResult) {
        let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
        let r = search(&ix, q, opts);
        let hits = r.hits.iter().map(|&s| Hit::from_index(&ix, s)).collect();
        (hits, r)
    }

    /// Resolves many paths to entry ids in one pass (for frecency boosts).
    pub fn resolve(&self, paths: &[PathBuf]) -> Vec<Option<u32>> {
        let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
        resolve_paths(&ix, paths)
    }

    pub fn generation(&self) -> u64 {
        self.index.read().map(|i| i.generation()).unwrap_or(0)
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn watch_status(&self) -> WatchStatus {
        match self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(w) => w.status(),
            None => WatchStatus {
                mode: WatchMode::Unavailable,
                watched: 0,
                unwatched: 0,
                limit: crate::watch::read_inotify_limit(),
                detail: "Live updates aren't available; the index is refreshed by periodic rescans.".into(),
                fix_command: None,
            },
        }
    }

    pub fn status(&self) -> EngineStatus {
        let (entries, bytes, generation) = {
            let ix = self.index.read().unwrap_or_else(|e| e.into_inner());
            (ix.live() as u64, ix.heap_bytes() as u64, ix.generation())
        };
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        EngineStatus {
            phase: st.phase,
            entries,
            progress: self.progress.snapshot(),
            watch: self.watch_status(),
            loaded_from_disk: st.loaded,
            load_ms: st.load_ms,
            last_full_scan: st.last_full,
            last_scan_ms: st.last_scan_ms,
            last_scan: st.last_scan.clone(),
            index_bytes: bytes,
            index_file: self.index_file.clone(),
            generation,
            last_error: st.last_error.clone(),
        }
    }

    pub fn phase(&self) -> Phase {
        self.state.lock().map(|s| s.phase).unwrap_or(Phase::Ready)
    }

    /// Blocks until the engine is ready (tests, `--search` right after start).
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if self.phase() == Phase::Ready && self.pending.is_empty() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

/// Resolves paths to ids: folders by key, files by one pass over the
/// children of their (resolved) parent folders.
pub fn resolve_paths(ix: &Index, paths: &[PathBuf]) -> Vec<Option<u32>> {
    let mut out = vec![None; paths.len()];
    let mut want: IdMap<Vec<(usize, Vec<u8>)>> = IdMap::default();
    for (i, p) in paths.iter().enumerate() {
        if let Some(d) = ix.dir_at(p) {
            out[i] = Some(d);
            continue;
        }
        let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else { continue };
        if let Some(pd) = ix.dir_at(parent) {
            want.entry(pd).or_default().push((i, name.as_encoded_bytes().to_vec()));
        }
    }
    if !want.is_empty() {
        let set: IdSet = want.keys().copied().collect();
        let kids = ix.children_of(&set);
        for (d, items) in want {
            if let Some(ch) = kids.get(&d) {
                for (i, name) in items {
                    out[i] = ch.iter().copied().find(|&c| ix.name(c) == name.as_slice());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_with_live_updates() {
        let dir = crate::test_dir("engine");
        let root = dir.join("home");
        std::fs::create_dir_all(root.join("Docs")).unwrap();
        std::fs::write(root.join("Docs/alpha.txt"), "a").unwrap();
        let settings = Settings { roots: vec![root.display().to_string()], skip_network_mounts: false, ..Settings::default() };
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let index_file = dir.join("data/index.bin");
        let e = Engine::start(index_file.clone(), settings.clone(), Arc::new(move |x| ev.lock().unwrap().push(x)));
        assert!(e.wait_ready(Duration::from_secs(20)));
        let find = |e: &Engine, q: &str| {
            e.search_hits(&Query::parse(q), &SearchOptions { limit: 10, now: crate::now_secs(), ..Default::default() }).0
        };
        assert_eq!(find(&e, "alpha").len(), 1);
        assert!(index_file.exists(), "saved after the first crawl");

        // A new file appears through the watcher (Linux) or a requested rescan.
        std::fs::write(root.join("Docs/beta.txt"), "b").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while Instant::now() < deadline {
            if !find(&e, "beta").is_empty() {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !found {
            e.request_rescan();
            std::thread::sleep(Duration::from_millis(200));
            assert!(e.wait_ready(Duration::from_secs(10)));
            assert_eq!(find(&e, "beta").len(), 1);
        }
        #[cfg(target_os = "linux")]
        assert!(found, "inotify should deliver the change");

        // Our own rename is reflected immediately.
        std::fs::rename(root.join("Docs/alpha.txt"), root.join("Docs/gamma.txt")).unwrap();
        e.touched(&root.join("Docs/alpha.txt"));
        e.touched(&root.join("Docs/gamma.txt"));
        assert!(find(&e, "alpha").is_empty());
        assert_eq!(find(&e, "gamma").len(), 1);
        e.shutdown();
        assert!(events.lock().unwrap().contains(&EngineEvent::ScanDone));

        // Warm start loads the saved index before any crawl.
        let e2 = Engine::start(index_file, settings, Arc::new(|_| {}));
        assert!(e2.status().loaded_from_disk);
        assert_eq!(find(&e2, "gamma").len(), 1);
        e2.shutdown();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resolves_paths_in_batch() {
        use crate::index::{flag, key_child, key_root, Meta};
        let mut ix = Index::new();
        let root = Path::new("/r");
        let r = ix.add_root(root, Meta::default());
        let d = ix.push(r, b"d", Meta { flags: flag::DIR, ..Meta::default() }, key_child(key_root(root), b"d"));
        let f = ix.push(d, b"f.txt", Meta::default(), 0);
        let got = resolve_paths(&ix, &[PathBuf::from("/r/d/f.txt"), PathBuf::from("/r/d"), PathBuf::from("/r/x"), PathBuf::from("/q")]);
        assert_eq!(got, vec![Some(f), Some(d), None, None]);
    }
}
