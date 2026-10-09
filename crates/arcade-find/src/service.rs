//! The resident service: owns the engine, frecency, pins, the Link presence
//! and registry, and carries out what the overlay asks for. Work happens on
//! worker threads; results come back to the UI thread as [`UiMsg`]s.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant};

use arcade_link::{Locations, Presence, SharedRegistry};
use serde_json::{json, Value};

use find_core::content::{self as rg, ContentEnd, ContentRequest, Ripgrep};
use find_core::engine::EngineEvent;
use find_core::frecency::{Frecency, Pins};
use find_core::index::IdMap;
use find_core::kind::Kind;
use find_core::paths::{tilde, AppPaths};
use find_core::query::{path_within, KindFilter, TermKind};
use find_core::{matcher, Engine, Hit, Phase, Query, SearchOptions, Settings};

use crate::instance::Command;
use crate::link::{self, Item, PeerOffer, Pipeline, Selection};
use crate::ui::model::{ActionId, ActionItem, AppGlyph, Context, Effect, Glyph, Overlay, PeerAction, ResultsInfo, Row, ToastKind};

/// Results the overlay keeps per query (it scrolls beyond the visible rows).
pub const RESULT_LIMIT: usize = 200;
const EMPTY_STATE_MAX: usize = 30;
const CONTENT_LIMIT: usize = 500;
const CONTENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Messages for the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum UiMsg {
    Show {
        query: Option<String>,
        reveal: Option<PathBuf>,
    },
    Toggle,
    Hide,
    Results {
        seq: u64,
        rows: Vec<Row>,
        info: ResultsInfo,
    },
    Status(Option<String>),
    Toast(String, ToastKind),
    /// The index changed: re-run the query.
    Refresh,
    Paste(String),
    /// Settings were reloaded: re-read display settings.
    Settings,
    /// The system theme changed.
    Theme,
    Quit,
}

pub type UiSender = Arc<dyn Fn(UiMsg) + Send + Sync>;

/// What the window should do after a message or effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    pub redraw: bool,
    pub show: bool,
    pub hide: bool,
    pub quit: bool,
}

impl Outcome {
    fn redraw() -> Outcome {
        Outcome { redraw: true, ..Default::default() }
    }
    fn merge(&mut self, o: Outcome) {
        self.redraw |= o.redraw;
        self.show |= o.show;
        self.hide |= o.hide;
        self.quit |= o.quit;
    }
}

/// Shared between the engine listener and the service.
#[derive(Default)]
struct Hub {
    ui: OnceLock<UiSender>,
    engine: OnceLock<Weak<Engine>>,
    visible: AtomicBool,
    last_refresh: Mutex<Option<Instant>>,
}

impl Hub {
    fn send(&self, m: UiMsg) {
        if let Some(f) = self.ui.get() {
            f(m);
        }
    }

    fn on_engine(&self, ev: EngineEvent) {
        let Some(engine) = self.engine.get().and_then(Weak::upgrade) else { return };
        match ev {
            EngineEvent::Progress | EngineEvent::Status => self.send(UiMsg::Status(status_line(&engine))),
            EngineEvent::ScanDone => {
                self.send(UiMsg::Status(status_line(&engine)));
                if self.visible.load(Ordering::Relaxed) {
                    self.send(UiMsg::Refresh);
                }
            }
            EngineEvent::Changed => {
                if !self.visible.load(Ordering::Relaxed) {
                    return;
                }
                // While crawling the index changes constantly: refresh at most
                // every 400 ms (ScanDone brings the final refresh).
                if engine.phase() == Phase::Crawling {
                    let mut last = self.last_refresh.lock().unwrap_or_else(|e| e.into_inner());
                    if last.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
                        return;
                    }
                    *last = Some(Instant::now());
                }
                self.send(UiMsg::Refresh);
            }
        }
    }
}

/// The bar's index status: only while the index is not yet usable or complete.
pub fn status_line(engine: &Engine) -> Option<String> {
    match engine.phase() {
        Phase::Loading => Some("Loading index".into()),
        Phase::Crawling => {
            let st = engine.status();
            Some(format!("Indexing · {}", find_core::fmt::count(st.entries.max(st.progress.entries))))
        }
        Phase::Reconciling | Phase::Ready => None,
    }
}

struct SearchReq {
    seq: u64,
    text: String,
    hidden: bool,
}

#[derive(Default)]
struct SearchSlot {
    next: Mutex<Option<SearchReq>>,
    wake: Condvar,
    /// Cancels the running content search.
    cancel: Mutex<Arc<AtomicBool>>,
    latest: AtomicU64,
}

pub struct Service {
    pub paths: AppPaths,
    pub locations: Locations,
    pub background: bool,
    settings: RwLock<Settings>,
    pub engine: Arc<Engine>,
    frecency: Mutex<Frecency>,
    pins: Mutex<Pins>,
    registry: OnceLock<SharedRegistry>,
    presence: Mutex<Option<Arc<Presence>>>,
    pipelines: RwLock<Vec<Pipeline>>,
    hub: Arc<Hub>,
    search: SearchSlot,
    last_query: Mutex<String>,
    boosts: Mutex<(u64, u64, Arc<IdMap<i32>>)>,
    ripgrep: OnceLock<Option<Ripgrep>>,
    /// A peer action is running (one at a time).
    peer_busy: AtomicBool,
    /// How the global shortcut works now (for Settings and `--status`).
    shortcut: Mutex<Value>,
}

impl Service {
    /// Loads settings, frecency and pins, and starts the engine (which loads
    /// the saved index synchronously; the crawl runs in the background).
    pub fn start(paths: AppPaths, background: bool) -> Arc<Service> {
        let (settings, _) = Settings::load(&paths.settings_file());
        let hub = Arc::new(Hub::default());
        let listener_hub = hub.clone();
        let engine = Engine::start(paths.index_file(), settings.clone(), Arc::new(move |ev| listener_hub.on_engine(ev)));
        let _ = hub.engine.set(Arc::downgrade(&engine));
        let svc = Arc::new(Service {
            locations: Locations::discover(),
            background,
            frecency: Mutex::new(Frecency::load(&paths.frecency_file())),
            pins: Mutex::new(Pins::load(&paths.pins_file())),
            settings: RwLock::new(settings),
            engine,
            registry: OnceLock::new(),
            presence: Mutex::new(None),
            pipelines: RwLock::new(Vec::new()),
            hub,
            search: SearchSlot::default(),
            last_query: Mutex::new(String::new()),
            boosts: Mutex::new((0, 0, Arc::new(IdMap::default()))),
            ripgrep: OnceLock::new(),
            peer_busy: AtomicBool::new(false),
            shortcut: Mutex::new(json!({ "mode": "off" })),
            paths,
        });
        let worker = Arc::downgrade(&svc);
        let _ = std::thread::Builder::new().name("find-search".into()).spawn(move || search_loop(worker));
        svc
    }

    /// Connects the UI. Starts Link off the first-frame path.
    pub fn attach_ui(self: &Arc<Self>, ui: UiSender) {
        let _ = self.hub.ui.set(ui);
        self.hub.send(UiMsg::Status(status_line(&self.engine)));
        let me = self.clone();
        let _ = std::thread::Builder::new().name("find-link".into()).spawn(move || me.start_link());
    }

    fn start_link(self: &Arc<Self>) {
        let registry = SharedRegistry::load(&self.locations);
        let weak = Arc::downgrade(self);
        registry.watch(move |_| {
            if let Some(s) = weak.upgrade() {
                s.refresh_pipelines();
            }
        });
        let _ = self.registry.set(registry);
        let settings = self.settings();
        let handler = Arc::new(link::FindHandler { host: Arc::new(LinkHost(Arc::downgrade(self))), background: self.background });
        let presence = Presence::start(self.locations.clone(), link::manifest(&settings), handler);
        *self.presence.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(presence));
        self.refresh_pipelines();
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn registry(&self) -> Option<&SharedRegistry> {
        self.registry.get()
    }

    pub fn presence(&self) -> Option<Arc<Presence>> {
        self.presence.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_visible(&self, v: bool) {
        self.hub.visible.store(v, Ordering::Relaxed);
    }

    pub fn send(&self, m: UiMsg) {
        self.hub.send(m);
    }

    pub fn set_shortcut_state(&self, v: Value) {
        *self.shortcut.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }

    pub fn shortcut_state(&self) -> Value {
        self.shortcut.lock().map(|v| v.clone()).unwrap_or(Value::Null)
    }

    /// Re-reads settings from disk (after the settings window saved them).
    pub fn reload_settings(&self) {
        let (s, _) = Settings::load(&self.paths.settings_file());
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = s.clone();
        self.engine.set_settings(s.clone());
        if let Some(p) = self.presence() {
            p.update(link::manifest(&s));
        }
        self.send(UiMsg::Settings);
    }

    /// Box's pipelines, cached for the action list. Only asked while Box is
    /// running (never launches it), on start and when the registry changes.
    fn refresh_pipelines(&self) {
        let Some(reg) = self.registry.get() else { return };
        let settings = self.settings();
        if !settings.link.enabled || settings.link.disabled_peers.iter().any(|p| p == link::BOX) {
            return;
        }
        let Some(m) = reg.with(|r| r.get(link::BOX).cloned()) else {
            self.pipelines.write().unwrap_or_else(|e| e.into_inner()).clear();
            return;
        };
        let Some(action) = m.usable_actions().find(|a| a.id == "box.pipelines").cloned() else { return };
        let Ok(mut c) = arcade_link::Client::connect(&self.locations, link::BOX, &link::me()) else { return };
        let req = link::request(&action, vec![], json!({}));
        if let Ok(r) = c.invoke(&req, &mut |_| {}, None) {
            *self.pipelines.write().unwrap_or_else(|e| e.into_inner()) = link::parse_pipelines(&r);
        }
    }

    // -----------------------------------------------------------------------
    // Search

    pub fn request_search(&self, seq: u64, text: String, hidden: bool) {
        self.search.latest.store(seq, Ordering::SeqCst);
        // Stop a running content search right away.
        self.search.cancel.lock().unwrap_or_else(|e| e.into_inner()).store(true, Ordering::SeqCst);
        *self.search.next.lock().unwrap_or_else(|e| e.into_inner()) = Some(SearchReq { seq, text, hidden });
        self.search.wake.notify_one();
    }

    fn boosts(&self, now: i64) -> Arc<IdMap<i32>> {
        let fgen = self.frecency.lock().map(|f| f.generation).unwrap_or(0);
        let igen = self.engine.generation();
        let mut cache = self.boosts.lock().unwrap_or_else(|e| e.into_inner());
        if cache.0 == fgen && cache.1 == igen {
            return cache.2.clone();
        }
        let (paths, scores): (Vec<PathBuf>, Vec<i32>) = {
            let f = self.frecency.lock().unwrap_or_else(|e| e.into_inner());
            f.iter().map(|(p, v)| (p.clone(), Frecency::boost(v.score(now)))).filter(|(_, b)| *b > 0).unzip()
        };
        let ids = self.engine.resolve(&paths);
        let map: IdMap<i32> = ids.into_iter().zip(scores).filter_map(|(id, b)| id.map(|i| (i, b))).collect();
        *cache = (fgen, igen, Arc::new(map));
        cache.2.clone()
    }

    fn run_search(&self, req: SearchReq) {
        let now = find_core::now_secs();
        let started = Instant::now();
        if req.text.trim().is_empty() {
            let rows = self.empty_state();
            let info = ResultsInfo { matched: rows.len(), empty_state: true, elapsed_ms: ms(started), ..Default::default() };
            self.send(UiMsg::Results { seq: req.seq, rows, info });
            return;
        }
        let q = Query::parse_at(&req.text, now, find_core::local_offset_secs());
        let note = q.errors.first().cloned();
        if q.content.is_some() {
            self.content_search(req, q, note);
            return;
        }
        if q.is_empty() {
            let info = ResultsInfo { note, ..Default::default() };
            self.send(UiMsg::Results { seq: req.seq, rows: vec![], info });
            return;
        }
        let opts = SearchOptions { limit: RESULT_LIMIT, show_hidden: req.hidden, boosts: (*self.boosts(now)).clone(), now, threads: 0 };
        let (hits, r) = self.engine.search_hits(&q, &opts);
        if self.search.latest.load(Ordering::SeqCst) != req.seq {
            return;
        }
        let pins = self.pins.lock().map(|p| p.clone()).unwrap_or_default();
        let rows = hits.iter().map(|h| row_from_hit(h, &q, &pins)).collect();
        let note =
            note.or_else(|| (self.engine.phase() == Phase::Crawling).then(|| "Still indexing: more results will appear".to_string()));
        let info = ResultsInfo { matched: r.matched, elapsed_ms: r.elapsed.as_secs_f32() * 1000.0, note, ..Default::default() };
        self.send(UiMsg::Results { seq: req.seq, rows, info });
    }

    /// Pinned, then recent items (the empty query).
    fn empty_state(&self) -> Vec<Row> {
        let pins = self.pins.lock().map(|p| p.clone()).unwrap_or_default();
        let recent = self.frecency.lock().map(|f| f.recent(EMPTY_STATE_MAX)).unwrap_or_default();
        let mut rows = Vec::new();
        for p in &pins.items {
            rows.push(row_from_path(p, Some("Pinned"), true));
        }
        for p in recent.iter().filter(|p| !pins.contains(p)) {
            if rows.len() >= EMPTY_STATE_MAX {
                break;
            }
            rows.push(row_from_path(p, Some("Recent"), false));
        }
        rows
    }

    fn content_search(&self, req: SearchReq, q: Query, note: Option<String>) {
        let settings = self.settings();
        let fail = |msg: &str| {
            let info = ResultsInfo { content: true, note: Some(msg.to_string()), ..Default::default() };
            self.send(UiMsg::Results { seq: req.seq, rows: vec![], info });
        };
        if !settings.content_search {
            return fail("Content search is turned off in Settings");
        }
        let Some(rg) = self.ripgrep.get_or_init(|| rg::detect(&[])).clone() else {
            return fail("Content search needs ripgrep (rg). Install it from your package manager");
        };
        let dirs = if q.within.is_empty() { settings.root_paths() } else { q.within.clone() };
        let creq = ContentRequest {
            pattern: q.content.clone().unwrap_or_default(),
            dirs,
            exts: q.exts.clone(),
            exclude_names: settings.exclude_names.clone(),
            hidden: req.hidden,
            limit: CONTENT_LIMIT,
            timeout: CONTENT_TIMEOUT,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        *self.search.cancel.lock().unwrap_or_else(|e| e.into_inner()) = cancel.clone();
        if self.search.latest.load(Ordering::SeqCst) != req.seq {
            return;
        }
        let started = Instant::now();
        let pins = self.pins.lock().map(|p| p.clone()).unwrap_or_default();
        let mut rows: Vec<Row> = Vec::new();
        let mut last_sent = Instant::now();
        let mut sent = 0;
        let send = |rows: &Vec<Row>, streaming: bool, note: Option<String>| {
            let info = ResultsInfo { matched: rows.len(), elapsed_ms: ms(started), content: true, streaming, note, ..Default::default() };
            self.send(UiMsg::Results { seq: req.seq, rows: rows.clone(), info });
        };
        send(&rows, true, note.clone());
        let end = rg::run(&rg, &creq, &cancel, |path| {
            let mut r = row_from_path(&path, None, pins.contains(&path));
            if !content_filters_pass(&q, &r) {
                return;
            }
            r.section = None;
            rows.push(r);
            if last_sent.elapsed() > Duration::from_millis(120) && rows.len() > sent {
                sent = rows.len();
                last_sent = Instant::now();
                send(&rows, true, None);
            }
        });
        let note = match end {
            ContentEnd::Cancelled => return,
            ContentEnd::Limit => Some(format!("Showing the first {CONTENT_LIMIT} files")),
            ContentEnd::Timeout => Some("Stopped after 20 seconds".into()),
            ContentEnd::Failed(e) => Some(format!("ripgrep failed: {e}")),
            ContentEnd::Done => note,
        };
        send(&rows, false, note);
    }

    // -----------------------------------------------------------------------
    // Rows → Link

    fn disabled_peers(&self) -> Vec<String> {
        let s = self.settings.read().unwrap_or_else(|e| e.into_inner());
        if s.link.enabled {
            s.link.disabled_peers.clone()
        } else {
            vec!["*".into()]
        }
    }

    fn link_on(&self) -> bool {
        self.settings.read().map(|s| s.link.enabled).unwrap_or(true)
    }

    pub fn can_preview(&self, rows: &[Row]) -> bool {
        if !self.link_on() {
            return false;
        }
        let Some(reg) = self.registry.get() else { return false };
        let sel = selection_of(rows);
        reg.with(|r| link::look_previews(r, &self.disabled_peers(), &sel))
    }

    /// Peer actions for these rows, from the cached registry (no disk or IPC).
    pub fn peer_offers(&self, rows: &[Row]) -> Vec<PeerOffer> {
        if !self.link_on() {
            return vec![];
        }
        let Some(reg) = self.registry.get() else { return vec![] };
        let sel = selection_of(rows);
        let pipelines = self.pipelines.read().map(|p| p.clone()).unwrap_or_default();
        reg.with(|r| link::peer_offers(r, &self.disabled_peers(), &sel, &pipelines))
    }

    /// The action list for `rows` (Tab).
    pub fn actions_for(&self, rows: &[Row]) -> Vec<ActionItem> {
        let n = rows.len();
        let one = n == 1;
        let mut items = Vec::new();
        let preview = self.can_preview(rows);
        if preview {
            items.push(ActionItem::new(ActionId::Preview, "Quick Look", Glyph::Preview).hint("⏎"));
        }
        items.push(ActionItem::new(ActionId::Open, if one { "Open" } else { "Open all" }, Glyph::Open).hint(if preview {
            "⇧⏎"
        } else {
            "⏎"
        }));
        items.push(ActionItem::new(ActionId::Reveal, "Show in folder", Glyph::Reveal).hint(if cfg!(target_os = "macos") {
            "⌘⏎"
        } else {
            "Ctrl ⏎"
        }));
        let primary = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl " };
        items.push(
            ActionItem::new(ActionId::CopyPath, if one { "Copy path".to_string() } else { format!("Copy {n} paths") }, Glyph::Copy)
                .hint(&format!("{primary}C")),
        );
        if rows.iter().all(|r| !r.is_dir) || !cfg!(windows) {
            items.push(
                ActionItem::new(ActionId::CopyFile, if one { "Copy file".to_string() } else { format!("Copy {n} files") }, Glyph::CopyFile)
                    .hint(&format!("{primary}⇧C")),
            );
        }
        let sel = selection_of(rows);
        for o in self.peer_offers(rows) {
            let disabled = matches!(o.offer, link::Offer::Disabled(_));
            let detail = match &o.offer {
                link::Offer::Disabled(why) => Some(why.clone()),
                link::Offer::Ready if o.outbound() || o.persists() => Some(sel.preview()),
                _ => None,
            };
            items.push(ActionItem {
                outbound: o.outbound(),
                detail,
                group: o.action.group.clone(),
                disabled,
                keywords: o.app_name.clone(),
                ..ActionItem::new(
                    ActionId::Peer(PeerAction {
                        app: o.app.clone(),
                        action: o.action.id.clone(),
                        version: o.action.version,
                        preset: o.action.preset.clone(),
                        options: o.options.clone(),
                        interactive: o.opens_ui(),
                    }),
                    o.title.clone(),
                    Glyph::App(AppGlyph::of(&o.app)),
                )
            });
        }
        if one {
            items.push(ActionItem::new(ActionId::Rename, "Rename", Glyph::Rename).hint("F2"));
            let pinned = rows[0].pinned;
            items.push(
                ActionItem::new(if pinned { ActionId::Unpin } else { ActionId::Pin }, if pinned { "Unpin" } else { "Pin" }, Glyph::Pin)
                    .hint(&format!("{primary}⇧P")),
            );
            items.push(ActionItem::new(ActionId::Details, "Details", Glyph::Info).hint(&format!("{primary}I")));
        }
        items.push(ActionItem { danger: true, ..ActionItem::new(ActionId::Trash, "Move to Trash", Glyph::Trash).hint("Del") });
        items
    }

    // -----------------------------------------------------------------------
    // Effects (called on the UI thread; slow work moves to threads)

    fn record(&self, rows: &[Row]) {
        let now = find_core::now_secs();
        let mut f = self.frecency.lock().unwrap_or_else(|e| e.into_inner());
        for r in rows {
            f.record(&r.path, now);
        }
        let _ = f.save(&self.paths.frecency_file());
    }

    fn worker(self: &Arc<Self>, name: &str, f: impl FnOnce(&Arc<Service>) + Send + 'static) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name(name.into()).spawn(move || f(&me));
    }

    fn toast(&self, text: impl Into<String>, kind: ToastKind) {
        self.send(UiMsg::Toast(text.into(), kind));
    }

    /// Carries out one effect from the overlay.
    pub fn effect(self: &Arc<Self>, o: &mut Overlay, e: Effect) -> Outcome {
        let mut out = Outcome::redraw();
        match e {
            Effect::Search { seq, text, hidden } => self.request_search(seq, text, hidden),
            Effect::Hide => out.hide = true,
            Effect::Preview(rows) => {
                *self.last_query.lock().unwrap_or_else(|e| e.into_inner()) = o.input.text.clone();
                out.hide = true;
                self.worker("find-preview", move |s| s.preview(rows));
            }
            Effect::Open(rows) => {
                out.hide = true;
                self.worker("find-open", move |s| {
                    for r in &rows {
                        if let Err(e) = crate::os::open(&r.path) {
                            s.toast(e, ToastKind::Error);
                        }
                    }
                    s.record(&rows);
                });
            }
            Effect::Reveal(row) => {
                out.hide = true;
                self.worker("find-reveal", move |s| {
                    if let Err(e) = crate::os::reveal(&row.path) {
                        s.toast(e, ToastKind::Error);
                    }
                    s.record(std::slice::from_ref(&row));
                });
            }
            Effect::CopyPaths(rows) => {
                let text = rows.iter().map(|r| r.path.to_string_lossy()).collect::<Vec<_>>().join("\n");
                let n = rows.len();
                self.worker("find-copy", move |s| match crate::os::copy_text(text) {
                    Ok(()) => s.toast(if n == 1 { "Copied path".to_string() } else { format!("Copied {n} paths") }, ToastKind::Info),
                    Err(e) => s.toast(e, ToastKind::Error),
                });
            }
            Effect::CopyFiles(rows) => {
                let paths: Vec<PathBuf> = rows.iter().map(|r| r.path.clone()).collect();
                let n = paths.len();
                self.worker("find-copy", move |s| match crate::os::copy_files(paths) {
                    Ok(()) => s.toast(if n == 1 { "Copied file".to_string() } else { format!("Copied {n} files") }, ToastKind::Info),
                    Err(e) => s.toast(e, ToastKind::Error),
                });
            }
            Effect::CopyText(t) => self.worker("find-copy", move |s| {
                if let Err(e) = crate::os::copy_text(t) {
                    s.toast(e, ToastKind::Error);
                }
            }),
            Effect::Rename { row, new_name } => {
                self.worker("find-rename", move |s| match crate::os::rename_noreplace(&row.path, &new_name) {
                    Ok(to) => {
                        s.engine.touched(&row.path);
                        s.engine.touched(&to);
                        if let Ok(mut f) = s.frecency.lock() {
                            f.rename(&row.path, &to);
                            let _ = f.save(&s.paths.frecency_file());
                        }
                        if let Ok(mut p) = s.pins.lock() {
                            p.rename(&row.path, &to);
                            let _ = p.save(&s.paths.pins_file());
                        }
                        s.toast(format!("Renamed to {}", crate::os::display_name(&to)), ToastKind::Info);
                        s.send(UiMsg::Refresh);
                    }
                    Err(e) => s.toast(e, ToastKind::Error),
                })
            }
            Effect::Trash(rows) => self.worker("find-trash", move |s| {
                let paths: Vec<PathBuf> = rows.iter().map(|r| r.path.clone()).collect();
                match crate::os::trash(&paths) {
                    Ok(()) => {
                        for p in &paths {
                            s.engine.touched(p);
                            if let Ok(mut f) = s.frecency.lock() {
                                f.forget(p);
                            }
                            if let Ok(mut pins) = s.pins.lock() {
                                pins.remove(p);
                            }
                        }
                        if let Ok(f) = s.frecency.lock() {
                            let _ = f.save(&s.paths.frecency_file());
                        }
                        if let Ok(p) = s.pins.lock() {
                            let _ = p.save(&s.paths.pins_file());
                        }
                        let n = paths.len();
                        s.toast(if n == 1 { "Moved to Trash".to_string() } else { format!("Moved {n} items to Trash") }, ToastKind::Info);
                        s.send(UiMsg::Refresh);
                    }
                    Err(e) => s.toast(e, ToastKind::Error),
                }
            }),
            Effect::TogglePin(row) => {
                let pinned = {
                    let mut p = self.pins.lock().unwrap_or_else(|e| e.into_inner());
                    let now = p.toggle(&row.path);
                    let _ = p.save(&self.paths.pins_file());
                    now
                };
                o.toast(if pinned { "Pinned" } else { "Unpinned" }, ToastKind::Info);
                for e in o.refresh() {
                    out.merge(self.effect(o, e));
                }
            }
            Effect::Peer { action, title, targets } => {
                *self.last_query.lock().unwrap_or_else(|e| e.into_inner()) = o.input.text.clone();
                if action.interactive {
                    out.hide = true;
                }
                self.worker("find-peer", move |s| s.run_peer(action, title, targets));
            }
            Effect::Paste => self.worker("find-paste", move |s| {
                if let Some(t) = crate::os::paste_text() {
                    s.send(UiMsg::Paste(t));
                }
            }),
            Effect::OpenSettings => {
                out.hide = true;
                self.open_settings();
            }
            Effect::WantActions(rows) => {
                let items = self.actions_for(&rows);
                o.set_actions(rows, items);
            }
        }
        out
    }

    /// Look's preview (Enter / Space). On failure Find comes back with the
    /// standard message.
    fn preview(self: &Arc<Self>, rows: Vec<Row>) {
        let sel = selection_of(&rows);
        let Some(m) = self.registry.get().and_then(|r| r.with(|r| r.get(link::LOOK).cloned())) else {
            return self.fail_and_reshow("Arcade Look isn't installed.".into(), &rows);
        };
        let Some(action) = m.action("look.preview").cloned() else {
            return self.fail_and_reshow("Arcade Look can't do this right now.".into(), &rows);
        };
        let req = link::request(&action, sel.inputs, json!({}));
        match link::invoke(&self.locations, &m, &req, None, None) {
            Ok(_) => self.record(&rows),
            Err(e) => self.fail_and_reshow(e.user_message(&m.name), &rows),
        }
    }

    fn fail_and_reshow(&self, msg: String, rows: &[Row]) {
        let query = self.last_query.lock().map(|q| q.clone()).unwrap_or_default();
        self.send(UiMsg::Show { query: Some(query).filter(|q| !q.is_empty()), reveal: rows.first().map(|r| r.path.clone()) });
        self.toast(msg, ToastKind::Error);
    }

    fn run_peer(self: &Arc<Self>, pa: PeerAction, title: String, rows: Vec<Row>) {
        let Some(m) = self.registry.get().and_then(|r| r.with(|r| r.get(&pa.app).cloned())) else {
            let name = arcade_link::manifest::app_name(&pa.app).to_string();
            return self.peer_failed(&pa, format!("{name} isn't installed."), &rows);
        };
        let lookup = pa.preset.as_ref().map(|p| format!("{}#{p}", pa.action.split('#').next().unwrap_or(&pa.action)));
        let Some(mut action) = lookup.and_then(|id| m.action(&id).cloned()).or_else(|| m.action(&pa.action).cloned()) else {
            return self.peer_failed(&pa, format!("{} can't do this right now.", m.name), &rows);
        };
        action.version = pa.version;
        if self.peer_busy.swap(true, Ordering::SeqCst) {
            self.toast("Another action is still running", ToastKind::Info);
            return;
        }
        let sel = selection_of(&rows);
        if sel.skipped > 0 && sel.is_empty() {
            self.peer_busy.store(false, Ordering::SeqCst);
            return self.peer_failed(&pa, "These items' names can't be sent to other apps.".into(), &rows);
        }
        let req = link::request(&action, sel.inputs.clone(), pa.options.clone());
        let ui = self.clone();
        let name = m.name.clone();
        let mut launching = move || ui.toast(format!("Starting {name}…"), ToastKind::Info);
        let result = link::invoke(&self.locations, &m, &req, None, Some(&mut launching));
        self.peer_busy.store(false, Ordering::SeqCst);
        match result {
            Ok(r) => {
                let mut msg = r.message.unwrap_or_else(|| format!("{title}: done"));
                if sel.skipped > 0 {
                    msg.push_str(&format!(" · {} not sent (unsupported file name)", sel.skipped));
                }
                if !pa.interactive {
                    self.toast(msg, ToastKind::Info);
                }
                self.record(&rows);
            }
            Err(e) => self.peer_failed(&pa, e.user_message(&m.name), &rows),
        }
    }

    fn peer_failed(&self, pa: &PeerAction, msg: String, rows: &[Row]) {
        if pa.interactive {
            self.fail_and_reshow(msg, rows);
        } else {
            self.toast(msg, ToastKind::Error);
        }
    }

    pub fn open_settings(&self) {
        let exe = std::env::current_exe().unwrap_or_default();
        if let Err(e) = std::process::Command::new(exe).arg("--settings-window").stdin(std::process::Stdio::null()).spawn().map(|mut c| {
            std::thread::spawn(move || {
                let _ = c.wait();
            })
        }) {
            self.toast(format!("Couldn't open Settings: {e}"), ToastKind::Error);
        }
    }

    // -----------------------------------------------------------------------
    // UI messages (on the UI thread)

    /// Applies a message to the overlay.
    pub fn on_msg(self: &Arc<Self>, o: &mut Overlay, msg: UiMsg) -> Outcome {
        let mut out = Outcome::redraw();
        match msg {
            UiMsg::Show { query, reveal } => {
                out.show = true;
                for e in o.show(query.as_deref(), reveal) {
                    out.merge(self.effect(o, e));
                }
            }
            UiMsg::Toggle => {
                if o.visible {
                    out.hide = true;
                } else {
                    out.show = true;
                    for e in o.show(None, None) {
                        out.merge(self.effect(o, e));
                    }
                }
            }
            UiMsg::Hide => out.hide = true,
            UiMsg::Results { seq, rows, info } => out.redraw = o.set_results(seq, rows, info),
            UiMsg::Status(s) => o.status = s,
            UiMsg::Toast(t, k) => o.toast(t, k),
            UiMsg::Refresh => {
                for e in o.refresh() {
                    out.merge(self.effect(o, e));
                }
            }
            UiMsg::Paste(t) => {
                for e in o.paste(&t) {
                    out.merge(self.effect(o, e));
                }
            }
            UiMsg::Settings => {
                let s = self.settings();
                o.rows_cfg = s.rows.clone();
                o.recent_on_open = s.show_recent_on_open;
                o.show_hidden = s.show_hidden;
                // On the UI (main) thread, as global shortcuts require.
                crate::hotkey::apply(self);
            }
            UiMsg::Theme => {}
            UiMsg::Quit => out.quit = true,
        }
        out
    }

    /// Sets up a fresh overlay from settings.
    pub fn configure(&self, o: &mut Overlay) {
        let s = self.settings();
        o.rows_cfg = s.rows.clone();
        o.recent_on_open = s.show_recent_on_open;
        o.show_hidden = s.show_hidden;
        o.status = status_line(&self.engine);
    }

    // -----------------------------------------------------------------------
    // Instance commands (from a second launch or the settings window)

    pub fn command(self: &Arc<Self>, c: Command) -> Value {
        match c {
            Command::Ping => json!({ "ok": true, "version": crate::VERSION }),
            Command::Show { query, reveal } => {
                self.send(UiMsg::Show { query, reveal });
                json!({ "ok": true })
            }
            Command::Toggle => {
                self.send(UiMsg::Toggle);
                json!({ "ok": true })
            }
            Command::Hide => {
                self.send(UiMsg::Hide);
                json!({ "ok": true })
            }
            Command::Settings => {
                self.open_settings();
                json!({ "ok": true })
            }
            Command::Quit => {
                self.send(UiMsg::Quit);
                json!({ "ok": true })
            }
            Command::Restart => match restart_successor() {
                Ok(()) => {
                    self.send(UiMsg::Quit);
                    json!({ "ok": true })
                }
                Err(e) => json!({ "ok": false, "error": e }),
            },
            Command::Status => {
                let st = self.engine.status();
                json!({ "ok": true, "version": crate::VERSION, "pid": std::process::id(), "background": self.background,
                        "engine": st, "shortcut": self.shortcut_state(),
                        "link": { "listening": self.presence().map(|p| p.listening()).unwrap_or(false),
                                  "lastError": self.presence().and_then(|p| p.last_error()) } })
            }
            Command::Rescan => {
                self.engine.request_rescan();
                json!({ "ok": true })
            }
            Command::Reload => {
                self.reload_settings();
                json!({ "ok": true })
            }
            Command::Search { query, limit, hidden } => {
                let q = Query::parse_at(&query, find_core::now_secs(), find_core::local_offset_secs());
                if q.content.is_some() {
                    return json!({ "ok": false, "error": "Content search isn't available from the command line" });
                }
                if q.is_empty() {
                    return json!({ "ok": false, "error": q.errors.first().cloned().unwrap_or_else(|| "Empty query".into()) });
                }
                let opts = SearchOptions {
                    limit: limit.unwrap_or(20).clamp(1, link::SEARCH_MAX),
                    show_hidden: hidden.unwrap_or(false),
                    now: find_core::now_secs(),
                    ..Default::default()
                };
                let (hits, r) = self.engine.search_hits(&q, &opts);
                json!({ "ok": true, "matched": r.matched, "elapsedMs": r.elapsed.as_secs_f64() * 1000.0,
                        "results": hits.iter().map(|h| json!({"path": h.path.to_string_lossy(), "isDir": h.is_dir, "size": h.size, "modified": h.mtime})).collect::<Vec<_>>() })
            }
        }
    }

    /// Saves frecency and the index before exit.
    pub fn shutdown(&self) {
        if let Ok(f) = self.frecency.lock() {
            let _ = f.save(&self.paths.frecency_file());
        }
        if let Some(p) = self.presence() {
            p.stop();
        }
        self.engine.shutdown();
    }
}

impl Context for Service {
    fn can_preview(&self, rows: &[Row]) -> bool {
        Service::can_preview(self, rows)
    }
}

/// Starts a successor that waits for this instance to exit.
pub fn restart_successor() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .args(["--background", "--successor-of", &std::process::id().to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't restart: {e}"))
}

fn ms(t: Instant) -> f32 {
    t.elapsed().as_secs_f32() * 1000.0
}

fn search_loop(svc: Weak<Service>) {
    loop {
        let Some(s) = svc.upgrade() else { return };
        let req = {
            let mut next = s.search.next.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(r) = next.take() {
                    break r;
                }
                // Don't keep the service alive while waiting.
                let (g, _) = s.search.wake.wait_timeout(next, Duration::from_secs(3600)).unwrap_or_else(|e| e.into_inner());
                next = g;
            }
        };
        if req.seq == s.search.latest.load(Ordering::SeqCst) {
            s.run_search(req);
        }
    }
}

/// Encodes overlay rows for Link.
pub fn selection_of(rows: &[Row]) -> Selection {
    link::selection(rows.iter().filter(|r| !r.missing).map(|r| Item {
        path: &r.path,
        is_dir: r.is_dir,
        link_type: r.link_type(),
        size: r.size,
    }))
}

fn highlights(name: &str, q: &Query) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for t in &q.terms {
        let text = match t.kind {
            TermKind::Name => t.text.as_str(),
            TermKind::Path => t.text.rsplit(['/', '\\']).next().unwrap_or(""),
            TermKind::Glob => continue,
        };
        spans.extend(matcher::highlight(name, text));
    }
    spans.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

pub fn row_from_hit(h: &Hit, q: &Query, pins: &Pins) -> Row {
    Row {
        path: h.path.clone(),
        name: h.name.clone(),
        parent: tilde(&h.parent),
        size: (!h.is_dir).then_some(h.size),
        mtime: h.mtime,
        is_dir: h.is_dir,
        is_symlink: h.is_symlink,
        kind: Kind::of(&h.name, h.is_dir),
        highlights: highlights(&h.name, q),
        pinned: pins.contains(&h.path),
        section: None,
        missing: false,
    }
}

/// A row for a path outside a search (recent, pinned, content hits).
pub fn row_from_path(p: &Path, section: Option<&'static str>, pinned: bool) -> Row {
    let meta = std::fs::symlink_metadata(p).ok();
    let is_symlink = meta.as_ref().is_some_and(|m| m.file_type().is_symlink());
    let target = if is_symlink { std::fs::metadata(p).ok() } else { meta.clone() };
    let is_dir = target.as_ref().is_some_and(|m| m.is_dir());
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    let mtime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    Row {
        path: p.to_path_buf(),
        parent: p.parent().map(tilde).unwrap_or_default(),
        size: target.as_ref().filter(|m| m.is_file()).map(|m| m.len()),
        mtime,
        is_dir,
        is_symlink,
        kind: Kind::of(&name, is_dir),
        name,
        highlights: Vec::new(),
        pinned,
        section,
        missing: meta.is_none(),
    }
}

/// Filters ripgrep can't apply itself.
fn content_filters_pass(q: &Query, r: &Row) -> bool {
    if r.missing || q.kind == KindFilter::Dirs {
        return false;
    }
    if !q.within.is_empty() && !path_within(&r.path, &q.within) {
        return false;
    }
    if let Some((lo, hi)) = q.size {
        let s = r.size.unwrap_or(0);
        if s < lo || s > hi {
            return false;
        }
    }
    if let Some((lo, hi)) = q.modified {
        if r.mtime < lo || r.mtime > hi {
            return false;
        }
    }
    true
}

/// Serves `find.search` and `find.show` for the resident instance.
struct LinkHost(Weak<Service>);

impl link::Host for LinkHost {
    fn search(&self, query: &Query, limit: usize, hidden: bool) -> Result<link::SearchOut, arcade_link::LinkError> {
        let s = self.0.upgrade().ok_or_else(|| arcade_link::LinkError::new(arcade_link::ErrorCode::NotRunning, "shutting down"))?;
        // No frecency boosts: ranking must not reveal what the user opened.
        let opts = SearchOptions { limit, show_hidden: hidden, now: find_core::now_secs(), ..Default::default() };
        let (hits, r) = s.engine.search_hits(query, &opts);
        Ok(link::SearchOut {
            hits,
            matched: r.matched,
            elapsed_ms: r.elapsed.as_secs_f64() * 1000.0,
            indexing: s.engine.phase() == Phase::Crawling,
        })
    }

    fn show(&self, query: Option<String>, reveal: Option<PathBuf>) -> Result<(), arcade_link::LinkError> {
        let s = self.0.upgrade().ok_or_else(|| arcade_link::LinkError::new(arcade_link::ErrorCode::NotRunning, "shutting down"))?;
        s.send(UiMsg::Show { query, reveal });
        Ok(())
    }

    fn status(&self) -> Value {
        match self.0.upgrade() {
            Some(s) => {
                let st = s.engine.status();
                json!({ "phase": st.phase, "entries": st.entries })
            }
            None => Value::Null,
        }
    }

    fn quit(&self) -> Result<(), arcade_link::LinkError> {
        if let Some(s) = self.0.upgrade() {
            s.send(UiMsg::Quit);
        }
        Ok(())
    }
}
