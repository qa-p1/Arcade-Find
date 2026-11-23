//! Arcade Link: Find's manifest and actions (`find.search`, `find.show`,
//! `find.reveal`), one-shot mode, and the peer actions Find offers for
//! selected results.
//!
//! Peer entries come from Arcade Link's offers (SPEC §1.2, §5.2): the same
//! list, order, renames and user choices a file manager's "Arcade" menu
//! shows. Nothing here is specific to one peer except Look's preview (Find's
//! own "Quick Look") and Box's pipeline plumbing, which is never offered.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use arcade_link::client::{invoke_action, CallOptions};
use arcade_link::content::{self, Content};
use arcade_link::error::{standard_message, ErrorCode};
use arcade_link::manifest::{surface, Shortcut};
use arcade_link::offers::{self, Prefs, Rules};
use arcade_link::{Action, Handler, InvokeContext, InvokeRequest, InvokeResult, LinkError, Locations, Manifest, PeerInfo, Registry, Reply};
use serde_json::{json, Value};

use find_core::{Hit, Query, Settings};

/// The other Arcade apps, from the shared catalog Arcade Tools keeps (Link's
/// built-in list when there is none yet).
pub mod apps {
    use arcade_link::catalog::Catalog;
    use arcade_link::manifest::ids;
    use arcade_link::Locations;

    pub const SHELF: &str = ids::SHELF;

    /// The apps Connected apps always lists (with "Install" when missing).
    pub fn known(locations: &Locations) -> Vec<String> {
        Catalog::load(locations).app_ids().into_iter().filter(|id| id != ids::FIND).collect()
    }
    pub fn name(locations: &Locations, id: &str) -> String {
        Catalog::load(locations).name(id)
    }
    pub fn releases_url(locations: &Locations, id: &str) -> String {
        Catalog::load(locations).releases_url(id)
    }
}

pub const LOOK: &str = "arcade.look";
pub const BOX: &str = "arcade.box";

/// Most results `find.search` returns.
pub const SEARCH_MAX: usize = 1000;
const SEARCH_DEFAULT: usize = 50;
/// Most entries one app adds to the action list (Link orders them by rank).
const PER_APP_MAX: usize = 5;

/// The actions Find exposes.
pub fn actions() -> Vec<Action> {
    let mut search = Action::new("find.search", "Find matching files", "search").accepts(&["text/plain"]).produces(&["file/*[]", "folder/reference"]);
    search.surfaces = vec!["internal".into()];
    // Shelf's "Search in Find" hands over a file or folder, so those stay accepted; menus show it for selected text.
    let mut show = Action::new("find.show", "Search my files for this", "search")
        .accepts(&["text/plain", "file/*", "folder/reference"])
        .effects(&["opens-ui"])
        .interactive(true);
    show.surfaces = vec![surface::BROWSER_SELECTION.into(), surface::PALETTE.into()];
    show.keywords = ["find", "search", "files", "locate"].map(String::from).to_vec();
    let mut reveal =
        Action::new("find.reveal", "Reveal in Find", "search").accepts(&["file/*", "file/*[]", "folder/reference"]).effects(&["opens-ui"]).interactive(true);
    reveal.surfaces = vec![surface::FILE_MENU.into(), surface::FOLDER_MENU.into()];
    reveal.max_items = Some(1);
    reveal.rank.insert("*".into(), 10);
    vec![search, show, reveal]
}

pub fn me() -> PeerInfo {
    PeerInfo { id: crate::identity::ID.into(), version: crate::VERSION.into() }
}

/// Find's manifest for these settings.
pub fn manifest(settings: &Settings) -> Manifest {
    let mut m = Manifest::new(crate::identity::ID, crate::VERSION, &arcade_link::manifest::current_executable());
    m.name = crate::identity::NAME.into();
    m.launch.background = vec!["--background".into()];
    m.launch.invoke = Some(vec![arcade_link::oneshot::FLAG.into()]);
    if !settings.shortcut.trim().is_empty() {
        m.shortcuts.push(Shortcut {
            id: "show".into(),
            accelerator: arcade_link::accelerator::normalize(&settings.shortcut).unwrap_or_else(|_| settings.shortcut.trim().into()),
        });
    }
    m.settings.link_enabled = settings.link.enabled;
    m.actions = actions();
    m
}

/// The schema-1 extension wrapper is the common shape Rust apps should use
/// with Presence::start_document/update_document and --arcade-manifest.
pub fn document(settings: &Settings, paths: &find_core::paths::AppPaths) -> arcade_link::manifest::ManifestDocument {
    use arcade_link::manifest::{Docs, ManifestAdditions, ManifestDocument};
    ManifestDocument {
        manifest: manifest(settings),
        additions: ManifestAdditions {
            menu: menu_items(),
            logs: Some(paths.log_dir().to_string_lossy().into()),
            docs: Some(Docs { version: crate::VERSION.into() }),
            install: Some(crate::installer::method()),
            ..Default::default()
        },
    }
}

pub const RESCAN_MENU_ID: &str = "rescan";
pub fn menu_items() -> Vec<arcade_link::manifest::MenuItem> {
    use arcade_link::manifest::{MenuItem, MenuKind};
    vec![MenuItem {
        id: RESCAN_MENU_ID.into(),
        title: "Rescan index".into(),
        kind: MenuKind::Action,
        checked: None,
        enabled: true,
        effects: vec!["persists".into()],
    }]
}

// ---------------------------------------------------------------------------
// Selection → Link content

/// One selected result, as the index knows it (no disk access).
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    pub path: &'a Path,
    pub is_dir: bool,
    /// `file/<kind>` or `folder/reference`.
    pub link_type: &'static str,
    pub size: Option<u64>,
}

impl<'a> Item<'a> {
    pub fn from_hit(h: &'a Hit) -> Item<'a> {
        Item { path: &h.path, is_dir: h.is_dir, link_type: link_type(&h.path, h.is_dir), size: (!h.is_dir).then_some(h.size) }
    }
}

const FILE_TYPES: [&str; 21] = [
    "file/svg",
    "file/csv",
    "file/json",
    "file/html",
    "file/markdown",
    "file/ebook",
    "file/subtitle",
    "file/image",
    "file/video",
    "file/audio",
    "file/pdf",
    "file/document",
    "file/spreadsheet",
    "file/presentation",
    "file/archive",
    "file/text",
    "file/code",
    "file/font",
    "file/model",
    "file/any",
    "folder/reference",
];

/// The Link type of a path: Link's own kinds (`file/csv`, `file/svg`, …), so
/// Find offers what a file manager's menu offers for the same file.
pub fn link_type(path: &Path, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder/reference";
    }
    let kind = content::file_kind_for_path(path);
    FILE_TYPES.iter().copied().find(|t| t.strip_prefix("file/") == Some(kind)).unwrap_or("file/any")
}

/// A selection encoded as Link inputs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    /// Files as one value (`file/<kind>` or `file/<kind>[]`), then one
    /// `folder/reference` per folder, each in selection order.
    pub inputs: Vec<Content>,
    /// Summed file sizes, for `maxBytes`.
    pub bytes: u64,
    /// Items left out because their path can't travel as JSON text.
    pub skipped: usize,
    /// Display names in selection order (payload previews).
    pub names: Vec<String>,
    /// Each file as Link's offers see it (type and size), in selection order.
    pub files: Vec<offers::Item>,
    /// How many folders are selected.
    pub folders: usize,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    /// "a.png, b.pdf +4".
    pub fn preview(&self) -> String {
        let shown: Vec<&str> = self.names.iter().take(2).map(String::as_str).collect();
        let more = self.names.len().saturating_sub(shown.len());
        if more > 0 {
            format!("{} +{more}", shown.join(", "))
        } else {
            shown.join(", ")
        }
    }

    pub fn count(&self) -> usize {
        self.names.len()
    }
}

/// Encodes selected results. Paths travel by reference; nothing is read.
pub fn selection<'a>(items: impl IntoIterator<Item = Item<'a>>) -> Selection {
    let mut files: Vec<(String, &'static str, Option<u64>)> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut sel = Selection::default();
    for it in items {
        // Link paths are JSON strings: a non-UTF-8 path would arrive as a
        // different (lossy) path, so it is left out and reported instead.
        let Some(p) = it.path.to_str() else {
            sel.skipped += 1;
            continue;
        };
        sel.names.push(it.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string()));
        if it.is_dir {
            folders.push(p.to_string());
        } else {
            sel.bytes += it.size.unwrap_or(0);
            sel.files.push(offers::Item { kind: it.link_type.to_string(), size: it.size, hints: Vec::new() });
            files.push((p.to_string(), it.link_type, it.size));
        }
    }
    match files.len() {
        0 => {}
        1 => {
            let (path, kind, size) = files.pop().unwrap_or_default();
            sel.inputs.push(Content { kind: kind.to_string(), path: Some(path), size, ..Default::default() });
        }
        _ => {
            let first = files[0].1;
            let kind = if files.iter().all(|f| f.1 == first) { first } else { "file/any" };
            sel.inputs.push(Content { kind: format!("{kind}[]"), paths: files.into_iter().map(|f| f.0).collect(), ..Default::default() });
        }
    }
    sel.folders = folders.len();
    sel.inputs.extend(folders.into_iter().map(|p| Content { kind: "folder/reference".into(), path: Some(p), ..Default::default() }));
    sel
}

// ---------------------------------------------------------------------------
// Offers

#[derive(Debug, Clone, PartialEq)]
pub enum Offer {
    Ready,
    /// Shown, but can't run for this selection (the reason is shown).
    Disabled(String),
}

fn base_pattern(p: &str) -> &str {
    p.split(';').next().unwrap_or("").trim()
}

/// An action that only returns data for a caller (`look.inspect`,
/// `lens.recognize`): nothing for a person to pick from a menu.
fn data_only(a: &Action) -> bool {
    !a.interactive && a.effects.is_empty() && !a.produces.iter().any(|t| t.starts_with("file/") || t.starts_with("folder/"))
}

/// Whether `a` (from app `m`) is offered for `sel`, following SPEC §1.2:
/// enabled, available here, and accepting every value of the selection.
pub fn offer(m: &Manifest, a: &Action, sel: &Selection) -> Option<Offer> {
    if !m.settings.link_enabled || !a.available || !a.on_this_platform() || a.accepts.is_empty() || sel.is_empty() || data_only(a) {
        return None;
    }
    // An action without an array pattern takes one value: never hand it several.
    let takes_arrays = a.accepts.iter().any(|p| base_pattern(p).ends_with("[]"));
    let takes_anything = a.accepts.iter().any(|p| base_pattern(p) == "*");
    if !takes_arrays && (sel.inputs.len() != 1 || (!takes_anything && sel.inputs[0].kind.ends_with("[]"))) {
        return None;
    }
    if !sel.inputs.iter().all(|c| content::accepts_content(&a.accepts, c)) {
        return None;
    }
    if let Some(max) = a.max_bytes {
        if sel.bytes > max {
            return Some(Offer::Disabled(standard_message(ErrorCode::TooLarge, &m.name, None, Some(max))));
        }
    }
    Some(Offer::Ready)
}

/// A peer action to show for a selection.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerOffer {
    pub app: String,
    pub app_name: String,
    /// The manifest entry (for a pipeline: `box.pipeline.run`).
    pub action: Action,
    pub title: String,
    pub offer: Offer,
    pub options: Value,
}

impl PeerOffer {
    /// Opens a window in the peer: hide Find's overlay first.
    pub fn opens_ui(&self) -> bool {
        self.action.interactive || self.action.has_effect("opens-ui")
    }

    /// Sends content off this device (↗ plus a payload preview).
    pub fn outbound(&self) -> bool {
        ["network", "uploads-content", "sends-to-device"].iter().any(|e| self.action.has_effect(e)) || self.action.privacy != "local"
    }

    /// Keeps the content somewhere (a payload preview, SPEC §1.8).
    pub fn persists(&self) -> bool {
        self.action.has_effect("persists")
    }
}

/// Never offered: Find's own "Quick Look", and Box's pipeline plumbing.
fn skipped(app: &str, action: &str) -> bool {
    (app == LOOK && action == "look.preview") || (app == BOX && (action == "box.pipelines" || action == "box.pipeline.run"))
}

/// Link's offers for the selection: files as a file menu sees them, folders
/// as a folder menu does; a mixed selection gets what both lists share.
fn link_offers(sel: &Selection, cards: &[Manifest], rules: &Rules, prefs: &Prefs) -> Vec<offers::Offer> {
    let for_files = (!sel.files.is_empty()).then(|| offers::offers(&sel.files, surface::FILE_MENU, cards, rules, prefs));
    // One folder stands for all of them: `offer` below checks the whole selection.
    let for_folders = (sel.folders > 0).then(|| offers::offers(&[offers::Item::new("folder/reference")], surface::FOLDER_MENU, cards, rules, prefs));
    match (for_files, for_folders) {
        (Some(f), Some(d)) => f.into_iter().filter(|o| d.iter().any(|x| x.app == o.app && x.action == o.action)).collect(),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => Vec::new(),
    }
}

/// Every peer action to offer for `sel`, from the cached registry, the
/// catalog's rules and the user's choices (pinned first, then by rank).
/// No disk or IPC work: safe to call when the action list opens.
pub fn peer_offers(registry: &Registry, rules: &Rules, prefs: &Prefs, disabled: &[String], sel: &Selection) -> Vec<PeerOffer> {
    if sel.is_empty() {
        return Vec::new();
    }
    let cards: Vec<Manifest> = registry.peers(crate::identity::ID).filter(|m| !disabled.contains(&m.id) && m.settings.link_enabled).cloned().collect();
    offers_for(&cards, rules, prefs, sel)
}

/// [`peer_offers`] for these cards.
fn offers_for(cards: &[Manifest], rules: &Rules, prefs: &Prefs, sel: &Selection) -> Vec<PeerOffer> {
    let mut out: Vec<PeerOffer> = Vec::new();
    for o in link_offers(sel, cards, rules, prefs) {
        // Install entries belong to the menus; Find's list holds what can run now.
        if o.install.is_some() || skipped(&o.app, &o.action) || out.iter().filter(|p| p.app == o.app).count() >= PER_APP_MAX {
            continue;
        }
        let Some(m) = cards.iter().find(|m| m.id == o.app) else { continue };
        let Some(a) = m.usable_actions().find(|a| a.id == o.action) else { continue };
        // Link's offers judge each item; this checks the selection as one request (arrays, total size).
        let Some(mut offer) = offer(m, a, sel) else { continue };
        if !o.enabled {
            offer = Offer::Disabled(o.reason.clone().unwrap_or_else(|| format!("{} can't do this right now.", m.name)));
        }
        out.push(PeerOffer { app: m.id.clone(), app_name: m.name.clone(), action: a.clone(), title: o.title, offer, options: json!({}) });
    }
    out
}

/// Whether Look previews this selection (Enter's primary action).
pub fn look_previews(registry: &Registry, disabled: &[String], sel: &Selection) -> bool {
    if disabled.iter().any(|d| d == LOOK) {
        return false;
    }
    registry
        .get(LOOK)
        .and_then(|m| m.usable_actions().find(|a| a.id == "look.preview").map(|a| (m, a)))
        .is_some_and(|(m, a)| offer(m, a, sel) == Some(Offer::Ready))
}

/// A request for `action` (its `#preset` split off) with these inputs.
pub fn request(action: &Action, inputs: Vec<Content>, options: Value) -> InvokeRequest {
    let base = action.id.split('#').next().unwrap_or(&action.id);
    let mut r = InvokeRequest::new(base, crate::identity::ID).preset(action.preset.as_deref()).options(options);
    r.version = Some(action.version);
    r.inputs = inputs;
    r
}

/// Runs a peer action following SPEC §7 (running instance, one-shot, or
/// launch in the background). Blocking: call it from a worker thread.
pub fn invoke<'a>(
    locations: &Locations,
    manifest: &Manifest,
    req: &InvokeRequest,
    cancel: Option<&'a AtomicBool>,
    on_launching: Option<&'a mut dyn FnMut()>,
) -> Result<InvokeResult, LinkError> {
    invoke_action(locations, &me(), manifest, req, CallOptions { on_progress: None, cancel, on_launching })
}

// ---------------------------------------------------------------------------
// Serving find.search and find.show

/// What `find.search` returns from the host.
#[derive(Debug, Clone, Default)]
pub struct SearchOut {
    pub hits: Vec<Hit>,
    pub matched: usize,
    pub elapsed_ms: f64,
    /// The first crawl is still running: results may be incomplete.
    pub indexing: bool,
}

/// The app side of Find's actions.
pub trait Host: Send + Sync + 'static {
    /// A name search without frecency, history or content search.
    fn search(&self, query: &Query, limit: usize, hidden: bool) -> Result<SearchOut, LinkError>;
    /// Shows the overlay with a query, selecting `reveal` when it appears.
    fn show(&self, query: Option<String>, reveal: Option<PathBuf>) -> Result<(), LinkError>;
    fn methods(&self) -> Vec<String> {
        Vec::new()
    }
    fn settings(&self) -> Result<(), LinkError> {
        Err(LinkError::unavailable("one-shot mode has no settings window"))
    }
    fn restart_ready(&self, _mode: &str) -> Result<(), LinkError> {
        Err(LinkError::unavailable("one-shot mode cannot restart"))
    }
    fn restart(&self, _mode: &str) -> Result<(), LinkError> {
        Err(LinkError::unavailable("one-shot mode cannot restart"))
    }
    fn menu_invoke(&self, _id: &str) -> Result<(), LinkError> {
        Err(LinkError::unavailable("one-shot mode has no tray"))
    }
    fn shortcuts_set(&self, _id: &str, _key: &str) -> Result<arcade_link::app::ShortcutSetResult, LinkError> {
        Err(LinkError::unavailable("one-shot mode cannot bind shortcuts"))
    }
    fn settings_export(&self) -> Result<Vec<Content>, LinkError> {
        Err(LinkError::unavailable("export requires the resident app"))
    }
    fn settings_import(&self, _inputs: &[Content]) -> Result<arcade_link::app::ImportResult, LinkError> {
        Err(LinkError::unavailable("import requires the resident app"))
    }
    fn status(&self) -> Value {
        Value::Null
    }
    fn quit(&self) -> Result<(), LinkError> {
        Err(LinkError::unavailable("Arcade Find can't be quit over the Link"))
    }
}

pub struct FindHandler {
    pub host: Arc<dyn Host>,
    pub background: bool,
}

const QUERY_MAX: usize = 1024;

fn input_text(c: &Content) -> Option<&str> {
    c.text.as_deref()
}

/// The query for `find.search`: the text input, else `options.query`.
pub fn search_query(req: &InvokeRequest) -> Result<String, LinkError> {
    let text = match req.inputs.first() {
        Some(c) if content::type_matches("text/plain", &c.kind) => {
            input_text(c).ok_or_else(|| LinkError::unsupported("Arcade Find needs the query as inline text"))?.to_string()
        }
        Some(_) => return Err(LinkError::unsupported("Arcade Find searches with a text query")),
        None => req.options.get("query").and_then(Value::as_str).unwrap_or_default().to_string(),
    };
    let q = text.lines().next().unwrap_or("").trim();
    if q.is_empty() {
        return Err(LinkError::unsupported("Arcade Find needs a query"));
    }
    if q.len() > QUERY_MAX {
        return Err(LinkError::unsupported("The query is too long"));
    }
    Ok(q.to_string())
}

/// Runs `find.search` against `host`.
pub fn run_search(host: &dyn Host, req: &InvokeRequest) -> Result<InvokeResult, LinkError> {
    let text = search_query(req)?;
    let query = Query::parse_local(&text, find_core::now_secs());
    if query.content.is_some() {
        return Err(LinkError::unsupported("Content search runs only from Arcade Find's own window"));
    }
    if let Some(e) = query.errors.first() {
        return Err(LinkError::unsupported(e.clone()));
    }
    if query.is_empty() {
        return Err(LinkError::unsupported("Arcade Find needs a query"));
    }
    let limit = req.options.get("limit").and_then(Value::as_u64).map_or(SEARCH_DEFAULT, |n| (n as usize).clamp(1, SEARCH_MAX));
    let hidden = req.options.get("hidden").and_then(Value::as_bool).unwrap_or(false);
    let out = host.search(&query, limit, hidden)?;
    let sel = selection(out.hits.iter().map(Item::from_hit));
    let results: Vec<Value> = out
        .hits
        .iter()
        .filter(|h| h.path.to_str().is_some())
        .map(|h| {
            json!({
                "path": h.path.to_string_lossy(),
                "name": h.name,
                "parent": h.parent.to_string_lossy(),
                "type": link_type(&h.path, h.is_dir),
                "isDir": h.is_dir,
                "isSymlink": h.is_symlink,
                "size": if h.is_dir { Value::Null } else { json!(h.size) },
                "modified": h.mtime,
                "score": h.score,
            })
        })
        .collect();
    let message = match out.matched {
        0 => "No matches".to_string(),
        1 => "1 match".to_string(),
        n => format!("{} matches", arcade_link::design::format::format_count(n as i64)),
    };
    Ok(InvokeResult {
        outputs: sel.inputs,
        message: Some(message),
        data: Some(json!({
            "query": text,
            "matched": out.matched,
            "truncated": out.matched > results.len(),
            "indexing": out.indexing,
            "elapsedMs": (out.elapsed_ms * 10.0).round() / 10.0,
            "results": results,
        })),
    })
}

fn quote_scope(path: &Path) -> Result<String, LinkError> {
    let t = find_core::paths::tilde(path);
    if t.contains('"') {
        return Err(LinkError::unsupported("This folder's name can't be used as a search scope"));
    }
    Ok(format!("in:\"{t}\" "))
}

/// The query and the item to select for `find.show`.
pub fn show_args(req: &InvokeRequest) -> Result<(Option<String>, Option<PathBuf>), LinkError> {
    let extra = req.options.get("query").and_then(Value::as_str).map(|s| s.lines().next().unwrap_or("").trim().to_string()).filter(|s| !s.is_empty());
    if extra.as_ref().is_some_and(|s| s.len() > QUERY_MAX) {
        return Err(LinkError::unsupported("The query is too long"));
    }
    let Some(c) = req.inputs.first() else { return Ok((extra, None)) };
    if content::type_matches("text/plain", &c.kind) {
        let t = input_text(c).ok_or_else(|| LinkError::unsupported("Arcade Find needs the query as inline text"))?;
        let q = t.lines().next().unwrap_or("").trim();
        if q.len() > QUERY_MAX {
            return Err(LinkError::unsupported("The query is too long"));
        }
        return Ok((Some(q.to_string()).filter(|s| !s.is_empty()).or(extra), None));
    }
    let path =
        c.path.as_deref().or(c.paths.first().map(String::as_str)).map(PathBuf::from).ok_or_else(|| LinkError::unsupported("Arcade Find needs a path"))?;
    if !path.is_absolute() {
        return Err(LinkError::unsupported("Arcade Find needs an absolute path"));
    }
    if c.kind == "folder/reference" {
        let q = quote_scope(&path)? + extra.as_deref().unwrap_or("");
        return Ok((Some(q), None));
    }
    if content::type_matches("file/*[]", &c.kind) {
        return Ok((Some(name_query(&path)?), Some(path)));
    }
    Err(LinkError::unsupported("Arcade Find can't search for this kind of content"))
}

/// The item's name as a query (quoted when it has spaces).
fn name_query(path: &Path) -> Result<String, LinkError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| LinkError::unsupported("Arcade Find needs a file name"))?;
    Ok(if name.chars().any(char::is_whitespace) && !name.contains('"') { format!("\"{name}\"") } else { name })
}

/// The query and the item to select for `find.reveal`: the file or folder by
/// its name, selected when it appears.
pub fn reveal_args(req: &InvokeRequest) -> Result<(Option<String>, Option<PathBuf>), LinkError> {
    let c = req.inputs.first().ok_or_else(|| LinkError::unsupported("Arcade Find needs a file or folder to reveal"))?;
    if !(c.kind == "folder/reference" || content::type_matches("file/*[]", &c.kind)) {
        return Err(LinkError::unsupported("Arcade Find reveals files and folders"));
    }
    if c.paths.len() > 1 || req.inputs.len() > 1 {
        return Err(LinkError::unsupported("Arcade Find reveals one item at a time"));
    }
    let path =
        c.path.as_deref().or(c.paths.first().map(String::as_str)).map(PathBuf::from).ok_or_else(|| LinkError::unsupported("Arcade Find needs a path"))?;
    if !path.is_absolute() {
        return Err(LinkError::unsupported("Arcade Find needs an absolute path"));
    }
    Ok((Some(name_query(&path)?), Some(path)))
}

impl FindHandler {
    pub fn handle(&self, req: InvokeRequest) -> Result<InvokeResult, LinkError> {
        match req.action.as_str() {
            "find.search" => run_search(self.host.as_ref(), &req),
            "find.show" => {
                let (query, reveal) = show_args(&req)?;
                self.host.show(query, reveal)?;
                Ok(InvokeResult::message("Showing Arcade Find"))
            }
            "find.reveal" => {
                let (query, reveal) = reveal_args(&req)?;
                self.host.show(query, reveal)?;
                Ok(InvokeResult::message("Showing Arcade Find"))
            }
            other => Err(LinkError::unavailable(format!("Arcade Find has no action {other}"))),
        }
    }
}

impl Handler for FindHandler {
    fn methods(&self) -> Vec<String> {
        self.host.methods()
    }
    fn settings(&self) -> Result<(), LinkError> {
        self.host.settings()
    }
    fn restart_ready(&self, mode: &str) -> Result<(), LinkError> {
        self.host.restart_ready(mode)
    }
    fn restart(&self, mode: &str) -> Result<(), LinkError> {
        self.host.restart(mode)
    }
    fn menu(&self) -> Result<Vec<arcade_link::manifest::MenuItem>, LinkError> {
        Ok(menu_items())
    }
    fn menu_invoke(&self, id: &str) -> Result<(), LinkError> {
        self.host.menu_invoke(id)
    }
    fn shortcuts_set(&self, id: &str, key: &str) -> Result<arcade_link::app::ShortcutSetResult, LinkError> {
        self.host.shortcuts_set(id, key)
    }
    fn settings_export(&self) -> Result<Vec<Content>, LinkError> {
        self.host.settings_export()
    }
    fn settings_import(&self, inputs: &[Content]) -> Result<arcade_link::app::ImportResult, LinkError> {
        self.host.settings_import(inputs)
    }
    fn describe(&self) -> Vec<Action> {
        actions()
    }

    fn invoke(&self, request: InvokeRequest, _ctx: &InvokeContext) -> Result<Reply, LinkError> {
        self.handle(request).map(Reply::Done)
    }

    fn status(&self) -> Value {
        let mut s = json!({ "mode": if self.background { "background" } else { "foreground" } });
        if let (Some(obj), Value::Object(extra)) = (s.as_object_mut(), self.host.status()) {
            obj.extend(extra);
        }
        s
    }

    fn activate(&self) -> Result<(), LinkError> {
        self.host.show(None, None)
    }

    fn quit(&self) -> Result<(), LinkError> {
        self.host.quit()
    }
}

/// `--arcade-invoke`: answers `find.search` from the saved index, with no
/// window, tray, shortcut, listener or manifest write (SPEC §4.4).
pub struct OfflineHost {
    pub paths: find_core::paths::AppPaths,
}

impl Host for OfflineHost {
    fn search(&self, query: &Query, limit: usize, hidden: bool) -> Result<SearchOut, LinkError> {
        let (settings, _) = Settings::load(&self.paths.settings_file());
        let (index, _) = find_core::persist::load(&self.paths.index_file())
            .map_err(|_| LinkError::unavailable("Arcade Find hasn't indexed your files yet. Open it once to start indexing"))?;
        let engine = find_core::Engine::offline(index, settings.clone());
        let opts = find_core::SearchOptions { limit, show_hidden: hidden || settings.show_hidden, now: find_core::now_secs(), ..Default::default() };
        let (hits, r) = engine.search_hits(query, &opts);
        Ok(SearchOut { hits, matched: r.matched, elapsed_ms: r.elapsed.as_secs_f64() * 1000.0, indexing: false })
    }

    fn show(&self, _query: Option<String>, _reveal: Option<PathBuf>) -> Result<(), LinkError> {
        Err(LinkError::new(ErrorCode::NotRunning, "one-shot mode has no window"))
    }
}

pub fn serve_oneshot(paths: find_core::paths::AppPaths) -> i32 {
    arcade_link::oneshot::serve(&FindHandler { host: Arc::new(OfflineHost { paths }), background: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, is_dir: bool, size: u64) -> (PathBuf, bool, u64) {
        (PathBuf::from(path), is_dir, size)
    }

    fn sel_of(items: &[(PathBuf, bool, u64)]) -> Selection {
        selection(items.iter().map(|(p, d, s)| Item { path: p, is_dir: *d, link_type: link_type(p, *d), size: (!*d).then_some(*s) }))
    }

    #[test]
    fn selection_encoding() {
        let one = sel_of(&[item("/h/a.png", false, 10)]);
        assert_eq!(one.inputs.len(), 1);
        assert_eq!(one.inputs[0].kind, "file/image");
        assert_eq!(one.inputs[0].path.as_deref(), Some("/h/a.png"));
        assert_eq!(one.inputs[0].size, Some(10));

        let same = sel_of(&[item("/h/a.png", false, 1), item("/h/b.jpg", false, 2)]);
        assert_eq!(same.inputs[0].kind, "file/image[]");
        assert_eq!(same.inputs[0].paths, vec!["/h/a.png", "/h/b.jpg"]);
        assert_eq!(same.bytes, 3);

        let mixed = sel_of(&[item("/h/d1", true, 0), item("/h/a.png", false, 1), item("/h/b.pdf", false, 2), item("/h/d2", true, 0)]);
        let kinds: Vec<&str> = mixed.inputs.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, vec!["file/any[]", "folder/reference", "folder/reference"]);
        assert_eq!(mixed.inputs[1].path.as_deref(), Some("/h/d1"));
        assert_eq!(mixed.inputs[2].path.as_deref(), Some("/h/d2"));
        assert_eq!(mixed.preview(), "d1, a.png +2");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_skipped() {
        use std::os::unix::ffi::OsStrExt;
        let bad = PathBuf::from(std::ffi::OsStr::from_bytes(b"/h/\xff.txt"));
        let s = sel_of(&[(bad, false, 1), item("/h/ok.txt", false, 1)]);
        assert_eq!(s.skipped, 1);
        assert_eq!(s.inputs.len(), 1);
        assert_eq!(s.inputs[0].kind, "file/text");
    }

    fn app(id: &str, actions: Vec<Action>) -> Manifest {
        let mut m = Manifest::new(id, "1.0.0", "/bin/sh");
        m.name = format!("App {id}");
        m.actions = actions;
        m
    }

    #[test]
    fn offer_rules() {
        let files = sel_of(&[item("/h/a.png", false, 1), item("/h/b.png", false, 1)]);
        let one = sel_of(&[item("/h/a.png", false, 1)]);
        let mixed = sel_of(&[item("/h/a.png", false, 1), item("/h/d", true, 0)]);
        let folders = sel_of(&[item("/h/d", true, 0), item("/h/e", true, 0)]);

        let collect = Action::new("x.add", "Add", "add").accepts(&["file/*", "file/*[]", "folder/reference"]).effects(&["persists"]);
        let m = app("arcade.x", vec![collect.clone()]);
        for s in [&files, &one, &mixed, &folders] {
            assert_eq!(offer(&m, &collect, s), Some(Offer::Ready));
        }

        // No folder pattern: mixed selections don't get it.
        let files_only = Action::new("x.f", "F", "f").accepts(&["file/*[]"]).effects(&["writes-files"]);
        assert_eq!(offer(&m, &files_only, &mixed), None);
        assert_eq!(offer(&m, &files_only, &files), Some(Offer::Ready));

        // No array pattern: exactly one value.
        let single = Action::new("x.s", "S", "s").accepts(&["file/image", "folder/reference"]).interactive(true);
        assert_eq!(offer(&m, &single, &one), Some(Offer::Ready));
        assert_eq!(offer(&m, &single, &files), None);
        assert_eq!(offer(&m, &single, &folders), None);

        // `*` takes one value of any type, arrays included.
        let any = Action::new("x.open", "More", "open").accepts(&["*"]).interactive(true);
        assert_eq!(offer(&m, &any, &files), Some(Offer::Ready));
        assert_eq!(offer(&m, &any, &mixed), None);

        // Data-only actions aren't menu entries.
        let inspect = Action::new("x.i", "Inspect", "inspect").accepts(&["file/*"]).produces(&["structured/file-info"]);
        assert_eq!(offer(&m, &inspect, &one), None);

        // Unavailable, other platform, link off.
        assert_eq!(offer(&m, &collect.clone().unavailable("nope"), &one), None);
        let other_os = if cfg!(windows) { "linux" } else { "windows" };
        assert_eq!(offer(&m, &collect.clone().platforms(&[other_os]), &one), None);
        let mut off = m.clone();
        off.settings.link_enabled = false;
        assert_eq!(offer(&off, &collect, &one), None);

        // maxBytes disables with the standard message.
        let mut small = collect.clone();
        small.max_bytes = Some(1);
        assert!(matches!(offer(&m, &small, &files), Some(Offer::Disabled(r)) if r.contains("Too large for App arcade.x")));
    }

    #[test]
    fn link_types_are_links_own_kinds() {
        for (name, kind) in [
            ("a.csv", "file/csv"),
            ("a.SVG", "file/svg"),
            ("a.png", "file/image"),
            ("a.epub", "file/ebook"),
            ("a.srt", "file/subtitle"),
            ("a.xyz", "file/any"),
            ("Makefile", "file/any"),
        ] {
            assert_eq!(link_type(Path::new(name), false), kind, "{name}");
        }
        assert_eq!(link_type(Path::new("/h/d.png"), true), "folder/reference");
        // Every kind Link names has a static type here (else it would fall back to file/any).
        for ext in ["svg", "csv", "json", "html", "md", "epub", "srt", "png", "mp4", "mp3", "pdf", "docx", "xlsx", "pptx", "zip", "txt", "rs", "ttf", "stl"] {
            let p = PathBuf::from(format!("a.{ext}"));
            assert_eq!(link_type(&p, false), format!("file/{}", content::file_kind_for_path(&p)), "{ext}");
        }
    }

    #[test]
    fn the_card_says_where_finds_actions_are_offered() {
        let a = actions();
        let by = |id: &str| a.iter().find(|x| x.id == id).unwrap().clone();
        assert_eq!(by("find.search").surfaces, vec!["internal"]);
        let show = by("find.show");
        assert_eq!(show.title, "Search my files for this");
        assert!(show.on_surface(surface::BROWSER_SELECTION) && show.on_surface(surface::PALETTE) && !show.on_surface(surface::FILE_MENU));
        // Shelf hands find.show a file or a folder.
        assert!(content::accepts_content(&show.accepts, &Content { kind: "file/pdf".into(), path: Some("/h/a.pdf".into()), ..Default::default() }));
        let reveal = by("find.reveal");
        assert!(reveal.on_surface(surface::FILE_MENU) && reveal.on_surface(surface::FOLDER_MENU));
        assert_eq!(reveal.max_items, Some(1));
        let m = manifest(&Settings::default());
        let menu = offers::offers(&[offers::Item::new("file/pdf")], surface::FILE_MENU, &[m.clone()], &Rules::default(), &Prefs::default());
        assert_eq!(menu.iter().map(|o| o.action.as_str()).collect::<Vec<_>>(), ["find.reveal"]);
        let two =
            offers::offers(&[offers::Item::new("file/pdf"), offers::Item::new("file/png")], surface::FILE_MENU, &[m], &Rules::default(), &Prefs::default());
        assert!(!two[0].enabled && two[0].reason.as_deref() == Some("Takes at most 1 item"));
    }

    #[test]
    fn reveal_selects_the_item_by_name() {
        let pdf = Content { kind: "file/pdf".into(), path: Some("/h/my report.pdf".into()), ..Default::default() };
        let r = InvokeRequest::new("find.reveal", "t").input(pdf);
        assert_eq!(reveal_args(&r).unwrap(), (Some("\"my report.pdf\"".into()), Some(PathBuf::from("/h/my report.pdf"))));
        let dir = Content { kind: "folder/reference".into(), path: Some("/h/photos".into()), ..Default::default() };
        assert_eq!(reveal_args(&InvokeRequest::new("find.reveal", "t").input(dir)).unwrap(), (Some("photos".into()), Some(PathBuf::from("/h/photos"))));
        let two = Content { kind: "file/pdf[]".into(), paths: vec!["/h/a.pdf".into(), "/h/b.pdf".into()], ..Default::default() };
        assert!(reveal_args(&InvokeRequest::new("find.reveal", "t").input(two)).is_err());
        assert!(reveal_args(&InvokeRequest::new("find.reveal", "t").input(Content::plain("x"))).is_err());
        let rel = Content { kind: "file/pdf".into(), path: Some("a.pdf".into()), ..Default::default() };
        assert!(reveal_args(&InvokeRequest::new("find.reveal", "t").input(rel)).is_err());
    }

    #[test]
    fn the_action_list_is_links_answer_with_the_catalog_rules_and_the_users_choices() {
        let mut add = Action::new("shelf.add", "Add to Shelf", "add").accepts(&["file/*", "file/*[]", "folder/reference"]).effects(&["persists"]);
        add.rank.insert("*".into(), 20);
        let mut convert =
            Action::new("box:arcade.image.convert#webp", "Convert to WebP", "convert").accepts(&["file/image", "file/image[]"]).effects(&["writes-files"]);
        convert.rank.insert("file/image".into(), 80);
        let csv = Action::new("box:arcade.csv.json", "CSV to JSON", "convert").accepts(&["file/csv"]).effects(&["writes-files"]);
        let mut small = Action::new("x.small", "Small only", "s").accepts(&["file/*", "file/*[]"]).effects(&["writes-files"]);
        small.max_bytes = Some(5);
        let needs = Action::new("x.ffmpeg", "Needs ffmpeg", "n").accepts(&["file/*"]).effects(&["writes-files"]).unavailable("Install ffmpeg");
        let mut needs = needs;
        needs.requires = vec!["engine:ffmpeg".into()];
        let preview = Action::new("look.preview", "Quick Look", "preview").accepts(&["file/*"]).interactive(true);
        let pipelines = Action::new("box.pipeline.run", "Run pipeline", "run").accepts(&["file/*"]).effects(&["writes-files"]);
        let cards = vec![app(apps::SHELF, vec![add]), app(BOX, vec![convert, csv, pipelines]), app("arcade.x", vec![small, needs]), app(LOOK, vec![preview])];
        let ids = |v: &[PeerOffer]| v.iter().map(|o| o.action.id.clone()).collect::<Vec<_>>();

        let photos = sel_of(&[item("/h/a.png", false, 3), item("/h/b.png", false, 3)]);
        let got = offers_for(&cards, &Rules::default(), &Prefs::default(), &photos);
        // Rank first; Look's preview is Find's own Quick Look; the pipeline plumbing and install entries are left out.
        assert_eq!(ids(&got), ["box:arcade.image.convert#webp", "shelf.add", "x.small"]);
        // maxBytes is checked against the whole selection (6 bytes here).
        assert!(matches!(&got[2].offer, Offer::Disabled(_)));

        // Link's own kinds: a CSV gets the CSV tool.
        let table = sel_of(&[item("/h/t.csv", false, 1)]);
        assert!(ids(&offers_for(&cards, &Rules::default(), &Prefs::default(), &table)).contains(&"box:arcade.csv.json".to_string()));

        // Mixed files and folders: only what takes both.
        let mixed = sel_of(&[item("/h/a.png", false, 1), item("/h/d", true, 0)]);
        assert_eq!(ids(&offers_for(&cards, &Rules::default(), &Prefs::default(), &mixed)), ["shelf.add"]);

        // The catalog renames and hides; the user's pins lead.
        let mut rules = Rules::default();
        rules.rename.insert("arcade.shelf/shelf.add".into(), "Keep on Shelf".into());
        rules.hide.push("arcade.x/x.small".into());
        let prefs = Prefs { pinned: vec!["arcade.shelf/shelf.add".into()], ..Prefs::default() };
        let got = offers_for(&cards, &rules, &prefs, &photos);
        assert_eq!(ids(&got), ["shelf.add", "box:arcade.image.convert#webp"]);
        assert_eq!(got[0].title, "Keep on Shelf");
    }

    #[test]
    fn requests_split_presets_and_carry_versions() {
        let mut a = Action::new("box:arcade.image.convert#webp", "Convert to WebP", "convert").accepts(&["file/image"]);
        a.preset = Some("webp".into());
        a.version = 3;
        let r = request(&a, vec![Content::plain("x")], json!({}));
        assert_eq!(r.action, "box:arcade.image.convert");
        assert_eq!(r.preset.as_deref(), Some("webp"));
        assert_eq!(r.version, Some(3));
        assert_eq!(r.context.source, crate::identity::ID);
        assert!(r.context.interactive);
    }

    #[test]
    fn show_arguments() {
        let r = InvokeRequest::new("find.show", "t").options(json!({"query": "ext:png"}));
        assert_eq!(show_args(&r).unwrap(), (Some("ext:png".into()), None));
        let r = InvokeRequest::new("find.show", "t").input(Content::plain("report 2024\nignored"));
        assert_eq!(show_args(&r).unwrap(), (Some("report 2024".into()), None));
        let home = find_core::paths::home_dir();
        let folder = Content { kind: "folder/reference".into(), path: Some(home.join("My Stuff").to_string_lossy().into_owned()), ..Default::default() };
        let r = InvokeRequest::new("find.show", "t").input(folder).options(json!({"query": "ext:png"}));
        let (q, reveal) = show_args(&r).unwrap();
        assert_eq!(q.as_deref(), Some(format!("in:\"{}\" ext:png", find_core::paths::tilde(&home.join("My Stuff"))).as_str()));
        assert!(reveal.is_none());
        let pdf = home.join("my report.pdf");
        let file = Content { kind: "file/pdf".into(), path: Some(pdf.to_string_lossy().into_owned()), ..Default::default() };
        let r = InvokeRequest::new("find.show", "t").input(file);
        assert_eq!(show_args(&r).unwrap(), (Some("\"my report.pdf\"".into()), Some(pdf)));
        let rel = Content { kind: "file/pdf".into(), path: Some("x.pdf".into()), ..Default::default() };
        assert!(show_args(&InvokeRequest::new("find.show", "t").input(rel)).is_err());
        let color = Content::structured("color", json!({"hex": "#fff"}));
        assert!(show_args(&InvokeRequest::new("find.show", "t").input(color)).is_err());
    }

    struct FakeHost(Vec<Hit>);

    impl Host for FakeHost {
        fn search(&self, q: &Query, limit: usize, _hidden: bool) -> Result<SearchOut, LinkError> {
            assert!(q.content.is_none());
            Ok(SearchOut { hits: self.0.iter().take(limit).cloned().collect(), matched: self.0.len(), elapsed_ms: 1.25, indexing: false })
        }
        fn show(&self, _: Option<String>, _: Option<PathBuf>) -> Result<(), LinkError> {
            Ok(())
        }
    }

    fn hit(path: &str, is_dir: bool) -> Hit {
        let p = PathBuf::from(path);
        Hit {
            id: 0,
            name: p.file_name().unwrap().to_string_lossy().into_owned(),
            parent: p.parent().unwrap().to_path_buf(),
            path: p,
            size: 5,
            mtime: 1,
            is_dir,
            is_symlink: false,
            hidden: false,
            score: 100,
        }
    }

    #[test]
    fn search_results() {
        let h = FindHandler { host: Arc::new(FakeHost(vec![hit("/h/a.png", false), hit("/h/d", true), hit("/h/b.pdf", false)])), background: true };
        let r = h.handle(InvokeRequest::new("find.search", "t").input(Content::plain("a")).options(json!({"limit": 2}))).unwrap();
        assert_eq!(r.message.as_deref(), Some("3 matches"));
        let d = r.data.unwrap();
        assert_eq!(d["truncated"], true);
        assert_eq!(d["results"].as_array().unwrap().len(), 2);
        assert_eq!(d["results"][1]["type"], "folder/reference");
        assert_eq!(d["results"][1]["size"], Value::Null);
        assert_eq!(r.outputs[0].kind, "file/image");
        assert_eq!(r.outputs[1].kind, "folder/reference");
        // Content search, empty queries and non-text input are refused.
        for bad in [Content::plain("/TODO"), Content::plain("   "), Content::structured("color", json!({}))] {
            let e = h.handle(InvokeRequest::new("find.search", "t").input(bad)).unwrap_err();
            assert_eq!(e.code, ErrorCode::UnsupportedInput);
        }
        // options.query works without an input.
        assert!(h.handle(InvokeRequest::new("find.search", "t").options(json!({"query": "a"}))).is_ok());
        assert_eq!(h.status()["mode"], "background");
    }
}
