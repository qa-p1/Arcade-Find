//! The overlay's state and keyboard behavior, independent of any window
//! system. Backends feed it key events and draw what it holds; it answers
//! with [`Effect`]s that the controller carries out.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use find_core::kind::Kind;
use find_core::settings::Rows;

use super::input::TextInput;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    /// The folder, shown with `~`.
    pub parent: String,
    pub size: Option<u64>,
    pub mtime: i64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub kind: Kind,
    /// Byte ranges of `name` to highlight.
    pub highlights: Vec<(usize, usize)>,
    pub pinned: bool,
    /// Empty state: "Pinned" or "Recent".
    pub section: Option<&'static str>,
    /// The file no longer exists (empty state only).
    pub missing: bool,
}

impl Row {
    pub fn link_type(&self) -> &'static str {
        self.kind.link_type()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResultsInfo {
    pub matched: usize,
    pub elapsed_ms: f32,
    pub empty_state: bool,
    pub content: bool,
    /// Content search still running.
    pub streaming: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    F1,
    F2,
    F5,
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    pub key: Key,
    /// Text the key produces (with the keyboard layout applied).
    pub text: Option<String>,
    pub mods: Mods,
}

impl KeyEvent {
    pub fn new(key: Key) -> KeyEvent {
        KeyEvent { key, text: None, mods: Mods::default() }
    }
    pub fn ch(c: char) -> KeyEvent {
        KeyEvent { key: Key::Char(c), text: Some(c.to_string()), mods: Mods::default() }
    }
    pub fn with(mut self, m: Mods) -> KeyEvent {
        self.mods = m;
        self
    }
    /// The primary modifier: Cmd on macOS, Ctrl elsewhere.
    pub fn primary(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.mods.logo || self.mods.ctrl
        } else {
            self.mods.ctrl
        }
    }
}

/// Something an action-list entry does.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionId {
    Preview,
    Open,
    Reveal,
    CopyPath,
    CopyFile,
    Rename,
    Pin,
    Unpin,
    Trash,
    Details,
    /// Another Arcade app's action.
    Peer(PeerAction),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeerAction {
    pub app: String,
    pub action: String,
    pub version: u32,
    pub preset: Option<String>,
    pub options: serde_json::Value,
    pub interactive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Preview,
    Open,
    Reveal,
    Copy,
    CopyFile,
    Rename,
    Pin,
    Trash,
    Info,
    /// An Arcade app's glyph by canonical id index.
    App(AppGlyph),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppGlyph {
    Box,
    Look,
    Wheel,
    Clipboard,
    Lens,
    Tools,
    Shelf,
    /// An app without a glyph in the vendored Link assets.
    Other,
}

impl AppGlyph {
    pub fn for_id(id: &str) -> Option<AppGlyph> {
        Some(match id {
            "arcade.box" => AppGlyph::Box,
            "arcade.look" => AppGlyph::Look,
            "arcade.wheel" => AppGlyph::Wheel,
            "arcade.clipboard" => AppGlyph::Clipboard,
            "arcade.lens" => AppGlyph::Lens,
            "arcade.tools" => AppGlyph::Tools,
            "arcade.shelf" => AppGlyph::Shelf,
            _ => return None,
        })
    }

    /// The app's glyph, or the generic one.
    pub fn of(id: &str) -> AppGlyph {
        AppGlyph::for_id(id).unwrap_or(AppGlyph::Other)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionItem {
    pub id: ActionId,
    pub title: String,
    /// A shortcut shown at the right, e.g. "⏎".
    pub hint: Option<String>,
    pub glyph: Glyph,
    /// Sends content off this device (↗ plus a payload preview).
    pub outbound: bool,
    pub detail: Option<String>,
    pub danger: bool,
    pub group: Option<String>,
    /// Shown but can't run; `detail` says why.
    pub disabled: bool,
    /// Extra words the filter matches (the owning app's name).
    pub keywords: String,
}

impl ActionItem {
    pub fn new(id: ActionId, title: impl Into<String>, glyph: Glyph) -> ActionItem {
        ActionItem {
            id,
            title: title.into(),
            hint: None,
            glyph,
            outbound: false,
            detail: None,
            danger: false,
            group: None,
            disabled: false,
            keywords: String::new(),
        }
    }

    pub fn hint(mut self, h: &str) -> ActionItem {
        self.hint = Some(h.into());
        self
    }

    fn matches(&self, f: &str) -> bool {
        f.is_empty() || self.title.to_lowercase().contains(f) || self.keywords.to_lowercase().contains(f)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Results,
    Actions { items: Vec<ActionItem>, all: Vec<ActionItem>, sel: usize, scroll: usize, filter: TextInput, targets: Vec<Row> },
    Rename { row: usize, input: TextInput },
    ConfirmTrash { targets: Vec<Row> },
    Details { row: Row },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    /// A copy finished ("Copied", "Copied 3 paths"): shown for 1.5 s.
    Copied,
    /// Stays until the next key or until the overlay closes.
    Error,
}

/// The index as the overlay shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexState {
    /// The saved index is loading.
    pub loading: bool,
    /// The first scan is running: items so far.
    pub indexing: Option<u64>,
    /// The engine's last error, already worded for people.
    pub error: Option<String>,
}

/// One line of the visible list: a section label or a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Section(&'static str),
    Row(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

/// What the controller should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Search {
        seq: u64,
        text: String,
        hidden: bool,
    },
    Hide,
    Preview(Vec<Row>),
    Open(Vec<Row>),
    Reveal(Row),
    CopyPaths(Vec<Row>),
    CopyFiles(Vec<Row>),
    CopyText(String),
    Rename {
        row: Row,
        new_name: String,
    },
    Trash(Vec<Row>),
    TogglePin(Row),
    Peer {
        action: PeerAction,
        title: String,
        targets: Vec<Row>,
    },
    /// Fetch clipboard text and call [`Overlay::paste`].
    Paste,
    OpenSettings,
    OpenGuide,
    Quit,
    /// Fill the action list for these rows ([`Overlay::set_actions`]).
    WantActions(Vec<Row>),
}

/// Asks the controller things the model can't know.
pub trait Context {
    /// Look is installed, enabled and accepts these rows.
    fn can_preview(&self, rows: &[Row]) -> bool;
}

/// Overlay sizes in logical pixels, all from the Arcade design kit
/// (Template B, launcher variant).
pub mod metrics {
    use arcade_link::design::{radius, size, SPACE};

    pub const WIDTH: f32 = size::LAUNCHER_WIDTH;
    /// The search field.
    pub const BAR: f32 = size::LAUNCHER_SEARCH;
    /// A two-line result or action row.
    pub const ROW: f32 = size::ROW_OVERLAY_2;
    /// Space above and below the list.
    pub const LIST_PAD: f32 = SPACE[2];
    /// A section label ("Pinned", "Recent").
    pub const SECTION: f32 = size::CONTROL_SM;
    /// The footer hint bar.
    pub const FOOTER: f32 = size::HINT_BAR;
    /// The toast strip, which takes the footer's place while shown.
    pub const TOAST: f32 = size::ROW_OVERLAY;
    pub const RADIUS: f32 = radius::LG;
    /// Transparent margin around the panel for `shadow-pop`, where the
    /// window itself can be transparent (layer shell).
    pub const SHADOW: f32 = SPACE[11];
    /// Where the list starts, from the panel's top.
    pub const LIST_TOP: f32 = BAR + LIST_PAD;
}

pub struct Overlay {
    pub visible: bool,
    pub input: TextInput,
    pub rows: Vec<Row>,
    pub info: ResultsInfo,
    pub sel: usize,
    /// Multi-selection anchor (Shift+arrows).
    pub anchor: Option<usize>,
    pub scroll: usize,
    /// Focus moved into the list (Space previews instead of typing).
    pub nav: bool,
    pub mode: Mode,
    pub show_hidden: bool,
    pub seq: u64,
    /// The newest results are for `seq` (the query is settled).
    pub shown_seq: u64,
    pub toast: Option<Toast>,
    /// Loading, first scan and errors of the index.
    pub index: IndexState,
    /// Text an input method is composing (shown at the caret, not yet typed).
    pub preedit: String,
    /// Content search is on in Settings (offered when nothing matches).
    pub content_search: bool,
    pub rows_cfg: Rows,
    /// Show recent items right away (setting).
    pub recent_on_open: bool,
    /// Showing the empty state because the user asked (Down on an empty query).
    pub empty_open: bool,
    /// After results arrive, select this path (find.show "reveal").
    pub want_select: Option<PathBuf>,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay {
            visible: false,
            input: TextInput::default(),
            rows: Vec::new(),
            info: ResultsInfo::default(),
            sel: 0,
            anchor: None,
            scroll: 0,
            nav: false,
            mode: Mode::Results,
            show_hidden: false,
            seq: 0,
            shown_seq: 0,
            toast: None,
            index: IndexState::default(),
            preedit: String::new(),
            content_search: true,
            rows_cfg: Rows::default(),
            recent_on_open: false,
            empty_open: false,
            want_select: None,
        }
    }
}

impl Overlay {
    /// Opens with an optional prefilled query.
    pub fn show(&mut self, query: Option<&str>, reveal: Option<PathBuf>) -> Vec<Effect> {
        self.visible = true;
        self.mode = Mode::Results;
        self.nav = false;
        self.anchor = None;
        self.sel = 0;
        self.scroll = 0;
        self.toast = None;
        self.preedit.clear();
        self.want_select = reveal;
        self.rows.clear();
        self.info = ResultsInfo::default();
        self.empty_open = self.recent_on_open;
        match query {
            Some(q) => {
                self.input.set(q);
                self.input.select_all();
            }
            None => self.input.clear(),
        }
        if self.input.text.is_empty() && !self.empty_open {
            return vec![];
        }
        vec![self.search()]
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.mode = Mode::Results;
        self.rows.clear();
        self.input.clear();
        self.empty_open = false;
        self.want_select = None;
        self.preedit.clear();
        self.toast = None;
    }

    fn search(&mut self) -> Effect {
        self.seq += 1;
        Effect::Search { seq: self.seq, text: self.input.text.clone(), hidden: self.show_hidden }
    }

    /// The query changed: search again (or collapse to the bar).
    fn requery(&mut self) -> Vec<Effect> {
        self.nav = false;
        self.anchor = None;
        self.sel = 0;
        self.scroll = 0;
        if self.input.text.trim().is_empty() && !self.empty_open {
            self.seq += 1;
            self.rows.clear();
            self.info = ResultsInfo::default();
            return vec![];
        }
        vec![self.search()]
    }

    /// Re-run the current query (the index changed, hidden toggled).
    pub fn refresh(&mut self) -> Vec<Effect> {
        if !self.visible || (self.input.text.trim().is_empty() && !self.empty_open) || !matches!(self.mode, Mode::Results) {
            return vec![];
        }
        vec![self.search()]
    }

    pub fn set_results(&mut self, seq: u64, rows: Vec<Row>, info: ResultsInfo) -> bool {
        if seq != self.seq {
            return false;
        }
        // Keep the selection on the same item when results refresh in place.
        let keep = if self.shown_seq == seq || self.nav { self.rows.get(self.sel).map(|r| r.path.clone()) } else { None };
        self.shown_seq = seq;
        self.rows = rows;
        self.info = info;
        if let Some(want) = self.want_select.clone() {
            if let Some(i) = self.rows.iter().position(|r| r.path == want) {
                self.sel = i;
                self.nav = true;
                self.want_select = None;
            }
        } else if let Some(p) = keep {
            self.sel = self.rows.iter().position(|r| r.path == p).unwrap_or(0);
        }
        if self.sel >= self.rows.len() {
            self.sel = self.rows.len().saturating_sub(1);
        }
        self.anchor = self.anchor.filter(|a| *a < self.rows.len());
        self.ensure_visible();
        true
    }

    /// Rows the window shows now (drives the window height).
    pub fn visible_rows(&self) -> usize {
        match &self.mode {
            Mode::Actions { items, .. } => self.rows_cfg.viewport(items.len()),
            Mode::Details { .. } => 4,
            _ => {
                let n = self.list_len();
                self.rows_cfg.viewport(n)
            }
        }
    }

    /// Rows in the list, including the "no matches" line.
    pub fn list_len(&self) -> usize {
        if self.rows.is_empty() && self.shows_list() {
            1
        } else {
            self.rows.len()
        }
    }

    /// Whether the list area is shown at all (the bar alone at rest).
    pub fn shows_list(&self) -> bool {
        match self.mode {
            Mode::Results => !self.rows.is_empty() || (self.shown_seq == self.seq && (!self.input.text.trim().is_empty() || self.empty_open)),
            _ => true,
        }
    }

    /// Section labels the list reserves room for: one per section among the
    /// rows (the empty state's "Pinned" and "Recent").
    pub fn sections(&self) -> usize {
        if matches!(self.mode, Mode::Actions { .. } | Mode::Details { .. }) || self.rows.first().is_none_or(|r| r.section.is_none()) {
            return 0;
        }
        1 + self.rows.windows(2).filter(|w| w[0].section != w[1].section).count()
    }

    /// The list area's height (without its padding).
    pub fn list_height(&self) -> f32 {
        let rows = if self.shows_list() { self.visible_rows() } else { 0 };
        if rows == 0 {
            return 0.0;
        }
        rows as f32 * metrics::ROW + self.sections() as f32 * metrics::SECTION
    }

    /// The strip under the list: a toast while one shows, else the hint bar
    /// whenever the list does. At rest the overlay is just the field.
    pub fn footer_height(&self) -> f32 {
        if self.toast.is_some() {
            metrics::TOAST
        } else if self.list_height() > 0.0 {
            metrics::FOOTER
        } else {
            0.0
        }
    }

    /// Panel height in logical pixels for the current state.
    pub fn height(&self) -> f32 {
        let list = self.list_height();
        let list = if list > 0.0 { list + metrics::LIST_PAD * 2.0 } else { 0.0 };
        metrics::BAR + list + self.footer_height()
    }

    /// The visible list, top to bottom, as (y from the list's top, entry).
    /// The first visible row's section label stays at the top while its
    /// section scrolls; the next section's label sits above its first row.
    pub fn layout(&self) -> Vec<(f32, Entry)> {
        let (len, scroll, rows) = match &self.mode {
            Mode::Actions { items, scroll, .. } => (items.len(), *scroll, false),
            Mode::Details { .. } => return Vec::new(),
            // Results, and the row-level modes drawn over them (rename, trash).
            _ => (self.rows.len(), self.scroll, true),
        };
        let vis = self.visible_rows();
        let mut out = Vec::new();
        let mut y = 0.0;
        for i in scroll..(scroll + vis).min(len) {
            if rows {
                if let Some(label) = self.rows[i].section {
                    if i == scroll || self.rows[i - 1].section != Some(label) {
                        out.push((y, Entry::Section(label)));
                        y += metrics::SECTION;
                    }
                }
            }
            out.push((y, Entry::Row(i)));
            y += metrics::ROW;
        }
        out
    }

    /// The row (absolute index) at `y` logical pixels from the panel's top.
    pub fn row_at(&self, y: f64) -> Option<usize> {
        if !self.shows_list() {
            return None;
        }
        let y = (y - metrics::LIST_TOP as f64) as f32;
        self.layout().into_iter().find_map(|(top, e)| match e {
            Entry::Row(i) if y >= top && y < top + metrics::ROW => Some(i),
            _ => None,
        })
    }

    fn ensure_visible(&mut self) {
        let vis = self.visible_rows().max(1);
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + vis {
            self.scroll = self.sel + 1 - vis;
        }
        let max_scroll = self.rows.len().saturating_sub(vis);
        self.scroll = self.scroll.min(max_scroll);
    }

    /// The rows an action applies to: the multi-selection, or the selected row.
    pub fn targets(&self) -> Vec<Row> {
        if self.rows.is_empty() {
            return Vec::new();
        }
        match self.anchor {
            Some(a) if a != self.sel => {
                let (lo, hi) = (a.min(self.sel), a.max(self.sel));
                self.rows[lo..=hi.min(self.rows.len() - 1)].iter().filter(|r| !r.missing).cloned().collect()
            }
            _ => self.rows.get(self.sel).filter(|r| !r.missing).cloned().into_iter().collect(),
        }
    }

    /// What dragging row `i` carries: the whole selection when `i` is in it,
    /// else just that row. Only result rows can be dragged.
    pub fn drag_rows(&self, i: usize) -> Vec<Row> {
        if !matches!(self.mode, Mode::Results) || i >= self.rows.len() || self.rows[i].missing {
            return Vec::new();
        }
        if self.is_selected(i) {
            self.targets()
        } else {
            vec![self.rows[i].clone()]
        }
    }

    /// Whether pressing row `i` lands inside a multi-row selection.
    pub fn in_multi_selection(&self, i: usize) -> bool {
        matches!(self.mode, Mode::Results) && self.is_selected(i) && self.targets().len() > 1
    }

    pub fn is_selected(&self, i: usize) -> bool {
        match self.anchor {
            Some(a) => i >= a.min(self.sel) && i <= a.max(self.sel),
            None => i == self.sel,
        }
    }

    fn move_sel(&mut self, delta: isize, extend: bool) {
        if self.rows.is_empty() {
            return;
        }
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.sel);
            }
        } else {
            self.anchor = None;
        }
        let n = self.rows.len() as isize;
        let s = (self.sel as isize + delta).clamp(0, n - 1);
        self.sel = s as usize;
        self.nav = true;
        self.ensure_visible();
    }

    /// Shows a toast in the footer strip (STANDARD §5): "Copied" for 1.5 s,
    /// others for 5 s; errors stay until the next key or until closing.
    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        use arcade_link::design::motion;
        let ms = match kind {
            ToastKind::Copied => motion::COPIED_MS,
            ToastKind::Info => motion::TOAST_MS,
            // Far enough away that only a key or hiding clears it.
            ToastKind::Error => 24 * 3600 * 1000,
        };
        self.toast = Some(Toast { text: text.into(), kind, until: Instant::now() + Duration::from_millis(ms) });
    }

    /// Drops an expired toast; returns when the next one expires.
    pub fn tick(&mut self, now: Instant) -> Option<Instant> {
        if let Some(t) = &self.toast {
            if now >= t.until {
                self.toast = None;
            }
        }
        self.toast.as_ref().filter(|t| t.kind != ToastKind::Error).map(|t| t.until)
    }

    /// Text an input method is composing; drawn at the caret until committed.
    pub fn ime_preedit(&mut self, text: &str) {
        self.preedit = text.to_string();
    }

    /// Text an input method committed: typed into the active field.
    pub fn ime_commit(&mut self, text: &str) -> Vec<Effect> {
        self.preedit.clear();
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() {
            return vec![];
        }
        match &mut self.mode {
            Mode::Results => {
                self.input.insert(&text);
                self.requery()
            }
            Mode::Rename { input, .. } => {
                input.insert(&text);
                vec![]
            }
            Mode::Actions { filter, .. } => {
                filter.insert(&text);
                self.filter_actions();
                vec![]
            }
            _ => vec![],
        }
    }

    /// Nothing matched a name query that content search could run on: Enter
    /// searches inside files instead (`/` + the query).
    pub fn offers_content_search(&self) -> bool {
        let text = self.input.text.trim();
        matches!(self.mode, Mode::Results)
            && self.content_search
            && self.rows.is_empty()
            && self.shown_seq == self.seq
            && !text.is_empty()
            && !self.info.content
            && !self.info.empty_state
            && self.info.note.is_none()
            && self.index == IndexState::default()
            && find_core::query::Query::parse_with(&format!("/{text}"), 0, &|_| 0).content.is_some()
    }

    pub fn paste(&mut self, text: &str) -> Vec<Effect> {
        let line = text.lines().next().unwrap_or("").trim_end();
        match &mut self.mode {
            Mode::Results => {
                self.input.insert(line);
                self.requery()
            }
            Mode::Rename { input, .. } => {
                input.insert(line);
                vec![]
            }
            Mode::Actions { .. } => {
                if let Mode::Actions { filter, .. } = &mut self.mode {
                    filter.insert(line);
                }
                self.filter_actions();
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn set_actions(&mut self, targets: Vec<Row>, items: Vec<ActionItem>) {
        if items.is_empty() || targets.is_empty() {
            return;
        }
        self.mode = Mode::Actions { all: items.clone(), items, sel: 0, scroll: 0, filter: TextInput::default(), targets };
    }

    fn filter_actions(&mut self) {
        if let Mode::Actions { items, all, sel, scroll, filter, .. } = &mut self.mode {
            let f = filter.text.to_lowercase();
            *items = all.iter().filter(|a| a.matches(&f)).cloned().collect();
            *sel = 0;
            *scroll = 0;
        }
    }

    /// Runs an action from the list (or a shortcut) on `targets`.
    pub fn run_action(&mut self, id: &ActionId, title: &str, targets: Vec<Row>, ctx: &dyn Context) -> Vec<Effect> {
        if targets.is_empty() {
            return vec![];
        }
        self.mode = Mode::Results;
        let first = targets[0].clone();
        match id {
            ActionId::Preview => {
                if ctx.can_preview(&targets) {
                    vec![Effect::Preview(targets)]
                } else {
                    self.mode = Mode::Details { row: first };
                    vec![]
                }
            }
            ActionId::Open => vec![Effect::Open(targets)],
            ActionId::Reveal => vec![Effect::Reveal(first)],
            ActionId::CopyPath => vec![Effect::CopyPaths(targets)],
            ActionId::CopyFile => vec![Effect::CopyFiles(targets)],
            ActionId::Rename => {
                if let Some(i) = self.rows.iter().position(|r| r.path == first.path) {
                    let mut input = TextInput::with(&first.name);
                    let stem = if first.is_dir { first.name.len() } else { first.name.rfind('.').filter(|&i| i > 0).unwrap_or(first.name.len()) };
                    input.select_range(0, stem);
                    self.sel = i;
                    self.anchor = None;
                    self.mode = Mode::Rename { row: i, input };
                }
                vec![]
            }
            ActionId::Pin | ActionId::Unpin => vec![Effect::TogglePin(first)],
            ActionId::Trash => {
                self.mode = Mode::ConfirmTrash { targets };
                vec![]
            }
            ActionId::Details => {
                self.mode = Mode::Details { row: first };
                vec![]
            }
            ActionId::Peer(p) => vec![Effect::Peer { action: p.clone(), title: title.to_string(), targets }],
        }
    }

    /// Enter: preview in Look, or open with the default app when Look can't.
    fn primary(&mut self, ctx: &dyn Context) -> Vec<Effect> {
        let t = self.targets();
        if t.is_empty() {
            return vec![];
        }
        if ctx.can_preview(&t) {
            vec![Effect::Preview(t)]
        } else {
            vec![Effect::Open(t)]
        }
    }

    pub fn key(&mut self, ev: &KeyEvent, ctx: &dyn Context) -> Vec<Effect> {
        if self.toast.as_ref().is_some_and(|t| t.kind == ToastKind::Error) {
            self.toast = None;
        }
        let primary = ev.primary();
        let shift = ev.mods.shift;
        match ev.key {
            Key::F1 => return vec![Effect::OpenGuide],
            Key::Char('q') if primary => return vec![Effect::Quit],
            Key::Char(',') if primary => return vec![Effect::OpenSettings],
            Key::Char('k') if primary => {
                self.mode = Mode::Results;
                self.nav = false;
                self.input.select_all();
                return vec![];
            }
            _ => {}
        }
        // Mode-specific handling first.
        match &mut self.mode {
            Mode::Rename { row, input } => {
                match ev.key {
                    Key::Escape => self.mode = Mode::Results,
                    Key::Enter => {
                        let name = input.text.trim().to_string();
                        let r = self.rows.get(*row).cloned();
                        self.mode = Mode::Results;
                        if let Some(r) = r {
                            if !name.is_empty() && name != r.name {
                                return vec![Effect::Rename { row: r, new_name: name }];
                            }
                        }
                    }
                    _ => edit(input, ev),
                }
                if matches!(ev.key, Key::Char('v')) && primary {
                    return vec![Effect::Paste];
                }
                return vec![];
            }
            Mode::ConfirmTrash { targets } => {
                let t = std::mem::take(targets);
                self.mode = Mode::Results;
                return match ev.key {
                    Key::Enter | Key::Delete => vec![Effect::Trash(t)],
                    _ => vec![],
                };
            }
            Mode::Details { .. } => {
                match ev.key {
                    Key::Escape | Key::Space | Key::Backspace | Key::Left => self.mode = Mode::Results,
                    Key::Enter => {
                        self.mode = Mode::Results;
                        return self.primary(ctx);
                    }
                    _ => {}
                }
                return vec![];
            }
            Mode::Actions { items, sel, scroll, filter, targets, .. } => {
                let vis = self.rows_cfg.viewport(items.len()).max(1);
                match ev.key {
                    Key::Escape | Key::Tab => self.mode = Mode::Results,
                    Key::Up => {
                        *sel = sel.saturating_sub(1);
                        if *sel < *scroll {
                            *scroll = *sel;
                        }
                    }
                    Key::Down => {
                        if *sel + 1 < items.len() {
                            *sel += 1;
                        }
                        if *sel >= *scroll + vis {
                            *scroll = *sel + 1 - vis;
                        }
                    }
                    Key::Enter => {
                        if let Some(item) = items.get(*sel).cloned() {
                            if item.disabled {
                                let why = item.detail.clone().unwrap_or_else(|| "This action isn't available now.".into());
                                self.toast(why, ToastKind::Error);
                                return vec![];
                            }
                            let t = targets.clone();
                            return self.run_action(&item.id, &item.title, t, ctx);
                        }
                    }
                    Key::Char('v') if primary => return vec![Effect::Paste],
                    _ => {
                        let before = filter.text.clone();
                        edit(filter, ev);
                        if filter.text != before {
                            self.filter_actions();
                        }
                    }
                }
                return vec![];
            }
            Mode::Results => {}
        }

        // Results mode.
        match ev.key {
            Key::Escape => {
                if self.anchor.is_some() {
                    self.anchor = None;
                    return vec![];
                }
                return vec![Effect::Hide];
            }
            Key::Down | Key::Up if self.rows.is_empty() && self.input.text.trim().is_empty() => {
                // Down on an empty query opens recent and pinned items.
                if !self.empty_open {
                    self.empty_open = true;
                    return vec![self.search()];
                }
                return vec![];
            }
            Key::Down => self.move_sel(1, shift),
            Key::Up => self.move_sel(-1, shift),
            Key::Char('n') if ev.mods.ctrl && !shift => self.move_sel(1, false),
            Key::Char('p') if ev.mods.ctrl && !shift => self.move_sel(-1, false),
            Key::PageDown => {
                let v = self.visible_rows().max(1) as isize;
                self.move_sel(v, shift);
            }
            Key::PageUp => {
                let v = self.visible_rows().max(1) as isize;
                self.move_sel(-v, shift);
            }
            Key::Home if self.nav && !self.rows.is_empty() => {
                self.sel = 0;
                self.anchor = None;
                self.ensure_visible();
            }
            Key::End if self.nav && !self.rows.is_empty() => {
                self.sel = self.rows.len() - 1;
                self.anchor = None;
                self.ensure_visible();
            }
            Key::Enter if primary => {
                if let Some(r) = self.targets().into_iter().next() {
                    return vec![Effect::Reveal(r)];
                }
            }
            Key::Enter if shift => {
                let t = self.targets();
                if !t.is_empty() {
                    return vec![Effect::Open(t)];
                }
            }
            Key::Enter if self.offers_content_search() => {
                let text = format!("/{}", self.input.text.trim());
                self.input.set(&text);
                return self.requery();
            }
            Key::Enter => return self.primary(ctx),
            Key::Space if self.nav && !self.rows.is_empty() && !primary => {
                let t = self.targets();
                return self.run_action(&ActionId::Preview, "Quick Look", t, ctx);
            }
            Key::Tab => {
                let t = self.targets();
                if !t.is_empty() {
                    return vec![Effect::WantActions(t)];
                }
            }
            Key::F2 => {
                let t = self.targets();
                if t.len() == 1 {
                    return self.run_action(&ActionId::Rename, "Rename", t, ctx);
                }
            }
            Key::Delete if (self.nav || primary) && !self.rows.is_empty() => {
                let t = self.targets();
                return self.run_action(&ActionId::Trash, "Move to Trash", t, ctx);
            }
            Key::Char('c') | Key::Char('C') if primary && shift => {
                let t = self.targets();
                if !t.is_empty() {
                    return vec![Effect::CopyFiles(t)];
                }
            }
            Key::Char('c') if primary => {
                if let Some(s) = self.input.selected_text() {
                    return vec![Effect::CopyText(s.to_string())];
                }
                let t = self.targets();
                if !t.is_empty() {
                    return vec![Effect::CopyPaths(t)];
                }
            }
            Key::Char('p') | Key::Char('P') if primary && shift => {
                if let Some(r) = self.targets().into_iter().next() {
                    return vec![Effect::TogglePin(r)];
                }
            }
            Key::Char('h') if primary => {
                // The field's "Hidden" badge shows the state.
                self.show_hidden = !self.show_hidden;
                return self.refresh();
            }
            Key::Char(',') if primary => return vec![Effect::OpenSettings],
            Key::Char('v') if primary => return vec![Effect::Paste],
            Key::Char('i') if primary => {
                if let Some(r) = self.targets().into_iter().next() {
                    self.mode = Mode::Details { row: r };
                }
            }
            Key::F5 => return self.refresh(),
            _ => {
                let before = self.input.text.clone();
                edit(&mut self.input, ev);
                if self.input.text != before {
                    return self.requery();
                }
                // Cursor keys go back to typing.
                if matches!(ev.key, Key::Left | Key::Right | Key::Home | Key::End) {
                    self.nav = false;
                }
            }
        }
        vec![]
    }

    /// A click on list row `i` (double-click runs the primary action).
    /// A click on row `i`; `extend` (Shift) extends the selection to it.
    pub fn click(&mut self, i: usize, double: bool, extend: bool, ctx: &dyn Context) -> Vec<Effect> {
        match &mut self.mode {
            Mode::Actions { items, sel, targets, .. } => {
                if i < items.len() {
                    *sel = i;
                    let item = items[i].clone();
                    if item.disabled {
                        let why = item.detail.clone().unwrap_or_else(|| "This action isn't available now.".into());
                        self.toast(why, ToastKind::Error);
                        return vec![];
                    }
                    let t = targets.clone();
                    return self.run_action(&item.id, &item.title, t, ctx);
                }
                vec![]
            }
            Mode::Results if i < self.rows.len() => {
                if extend {
                    self.anchor.get_or_insert(self.sel);
                } else {
                    self.anchor = None;
                }
                self.sel = i;
                self.nav = true;
                if double && !extend {
                    return self.primary(ctx);
                }
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn scroll_by(&mut self, lines: isize) {
        let vis = self.visible_rows().max(1);
        match &mut self.mode {
            Mode::Actions { items, scroll, .. } => {
                let max = items.len().saturating_sub(vis) as isize;
                *scroll = (*scroll as isize + lines).clamp(0, max) as usize;
            }
            _ => {
                let max = self.rows.len().saturating_sub(vis) as isize;
                self.scroll = (self.scroll as isize + lines).clamp(0, max) as usize;
            }
        }
    }
}

/// Plain text editing keys for a field.
fn edit(t: &mut TextInput, ev: &KeyEvent) {
    let word = ev.primary() || ev.mods.alt;
    let shift = ev.mods.shift;
    match ev.key {
        Key::Backspace => t.backspace(word),
        Key::Delete => t.delete(word),
        Key::Left => t.left(word, shift),
        Key::Right => t.right(word, shift),
        Key::Home => t.home(shift),
        Key::End => t.end(shift),
        Key::Char('a') if ev.primary() => t.select_all(),
        Key::Char('w') if ev.mods.ctrl && !cfg!(target_os = "macos") => t.backspace(true),
        Key::Char('u') if ev.mods.ctrl && !cfg!(target_os = "macos") => {
            t.select_range(0, t.cursor);
            t.backspace(false);
        }
        _ if ev.primary() || ev.mods.logo => {}
        Key::Space => t.insert(" "),
        _ => {
            if let Some(s) = &ev.text {
                t.insert(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ctx(bool);
    impl Context for Ctx {
        fn can_preview(&self, _: &[Row]) -> bool {
            self.0
        }
    }

    fn row(name: &str) -> Row {
        Row {
            path: PathBuf::from(format!("/h/{name}")),
            name: name.into(),
            parent: "~".into(),
            size: Some(1),
            mtime: 0,
            is_dir: false,
            is_symlink: false,
            kind: Kind::of(name, false),
            highlights: vec![],
            pinned: false,
            section: None,
            missing: false,
        }
    }

    fn typed(o: &mut Overlay, s: &str, ctx: &dyn Context) -> Vec<Effect> {
        let mut last = vec![];
        for c in s.chars() {
            last = o.key(&if c == ' ' { KeyEvent::new(Key::Space) } else { KeyEvent::ch(c) }, ctx);
        }
        last
    }

    fn results(o: &mut Overlay, n: usize) {
        let rows = (0..n).map(|i| row(&format!("f{i}.txt"))).collect();
        assert!(o.set_results(o.seq, rows, ResultsInfo::default()));
    }

    #[test]
    fn sections_label_pinned_and_recent_and_rows_map_to_clicks() {
        let mut o = Overlay::default();
        o.show(None, None);
        o.empty_open = true;
        let mut rows: Vec<Row> = (0..4).map(|i| row(&format!("f{i}.txt"))).collect();
        for (i, r) in rows.iter_mut().enumerate() {
            r.section = Some(if i < 1 { "Pinned" } else { "Recent" });
        }
        assert!(o.set_results(o.seq, rows, ResultsInfo { empty_state: true, ..Default::default() }));
        assert_eq!(o.sections(), 2);
        let layout = o.layout();
        assert_eq!(layout[0], (0.0, Entry::Section("Pinned")));
        assert_eq!(layout[1], (metrics::SECTION, Entry::Row(0)));
        assert_eq!(layout[2], (metrics::SECTION + metrics::ROW, Entry::Section("Recent")));
        assert_eq!(o.list_height(), 4.0 * metrics::ROW + 2.0 * metrics::SECTION);
        // A click on a label selects nothing; a click on a row selects it.
        let top = metrics::LIST_TOP as f64;
        assert_eq!(o.row_at(top + 4.0), None);
        assert_eq!(o.row_at(top + metrics::SECTION as f64 + 4.0), Some(0));
        assert_eq!(o.row_at(top + (2.0 * metrics::SECTION + metrics::ROW) as f64 + 4.0), Some(1));
        assert_eq!(o.height(), metrics::LIST_TOP + o.list_height() + metrics::LIST_PAD + metrics::FOOTER);
    }

    #[test]
    fn rename_and_trash_keep_the_rows_on_screen() {
        let mut o = Overlay::default();
        o.show(None, None);
        typed(&mut o, "f", &Ctx(true));
        results(&mut o, 3);
        o.key(&KeyEvent::new(Key::F2), &Ctx(true));
        assert!(matches!(o.mode, Mode::Rename { .. }));
        assert_eq!(o.layout().len(), 3, "the rows stay visible while renaming");
        o.key(&KeyEvent::new(Key::Escape), &Ctx(true));
        o.key(&KeyEvent::new(Key::Delete).with(Mods { ctrl: true, ..Mods::default() }), &Ctx(true));
        assert!(matches!(o.mode, Mode::ConfirmTrash { .. }));
        assert_eq!(o.layout().len(), 3, "and while confirming the trash");
    }

    #[test]
    fn enter_on_no_matches_searches_inside_files() {
        let mut o = Overlay::default();
        o.show(None, None);
        typed(&mut o, "todo", &Ctx(false));
        assert!(o.set_results(o.seq, vec![], ResultsInfo::default()));
        assert!(o.offers_content_search());
        let e = o.key(&KeyEvent::new(Key::Enter), &Ctx(false));
        assert_eq!(o.input.text, "/todo");
        assert!(matches!(&e[..], [Effect::Search { text, .. }] if text == "/todo"));
        // Not offered while the index is still being built, or when off.
        assert!(o.set_results(o.seq, vec![], ResultsInfo { content: true, ..Default::default() }));
        assert!(!o.offers_content_search(), "already a content search");
        o.input.set("todo");
        o.index.indexing = Some(10);
        assert!(!o.offers_content_search());
        o.index = IndexState::default();
        o.content_search = false;
        assert!(!o.offers_content_search());
    }

    #[test]
    fn input_methods_commit_into_the_field() {
        let mut o = Overlay::default();
        o.show(None, None);
        o.ime_preedit("にほん");
        assert_eq!(o.preedit, "にほん");
        assert!(o.input.text.is_empty(), "composing text isn't typed yet");
        let e = o.ime_commit("日本");
        assert!(o.preedit.is_empty());
        assert_eq!(o.input.text, "日本");
        assert!(matches!(&e[..], [Effect::Search { text, .. }] if text == "日本"));
        // Into the action filter and the rename field too.
        results(&mut o, 2);
        o.set_actions(o.targets(), vec![ActionItem::new(ActionId::Open, "Öffnen", Glyph::Open)]);
        o.ime_commit("Ö");
        assert!(matches!(&o.mode, Mode::Actions { filter, items, .. } if filter.text == "Ö" && items.len() == 1));
    }

    #[test]
    fn error_toasts_stay_until_a_key() {
        let mut o = Overlay::default();
        o.show(None, None);
        o.toast("Couldn't open it.", ToastKind::Error);
        assert_eq!(o.tick(Instant::now() + Duration::from_secs(60)), None, "no expiry is scheduled");
        assert!(o.toast.is_some());
        o.key(&KeyEvent::ch('a'), &Ctx(true));
        assert!(o.toast.is_none());
        o.toast("Copied", ToastKind::Copied);
        assert!(o.tick(Instant::now() + Duration::from_millis(1600)).is_none() && o.toast.is_none());
    }

    #[test]
    fn rest_is_just_the_bar() {
        let mut o = Overlay::default();
        assert!(o.show(None, None).is_empty());
        assert_eq!(o.height(), metrics::BAR);
        let e = typed(&mut o, "rep", &Ctx(true));
        assert_eq!(e, vec![Effect::Search { seq: 3, text: "rep".into(), hidden: false }]);
        // Until results arrive the bar stays alone.
        assert_eq!(o.height(), metrics::BAR);
        results(&mut o, 3);
        assert_eq!(o.visible_rows(), 3);
        assert!(o.height() > metrics::BAR);
    }

    #[test]
    fn rows_stretch_then_scroll() {
        let mut o = Overlay::default();
        o.show(None, None);
        typed(&mut o, "x", &Ctx(true));
        for (n, vis) in [(1, 1), (5, 5), (6, 6), (7, 7), (8, 5), (100, 5)] {
            results(&mut o, n);
            assert_eq!(o.visible_rows(), vis, "{n} results");
        }
        // Scrolling keeps the selection visible.
        for _ in 0..9 {
            o.key(&KeyEvent::new(Key::Down), &Ctx(true));
        }
        assert_eq!(o.sel, 9);
        assert_eq!(o.scroll, 5);
    }

    #[test]
    fn enter_previews_or_falls_back_to_open() {
        let mut o = Overlay::default();
        o.show(Some("f"), None);
        results(&mut o, 2);
        assert!(matches!(o.key(&KeyEvent::new(Key::Enter), &Ctx(true))[0], Effect::Preview(_)));
        assert!(matches!(o.key(&KeyEvent::new(Key::Enter), &Ctx(false))[0], Effect::Open(_)));
        let shift = Mods { shift: true, ..Mods::default() };
        assert!(matches!(o.key(&KeyEvent::new(Key::Enter).with(shift), &Ctx(true))[0], Effect::Open(_)));
        let ctrl = Mods { ctrl: true, logo: cfg!(target_os = "macos"), ..Mods::default() };
        assert!(matches!(o.key(&KeyEvent::new(Key::Enter).with(ctrl), &Ctx(true))[0], Effect::Reveal(_)));
    }

    #[test]
    fn space_types_until_navigating() {
        let mut o = Overlay::default();
        o.show(None, None);
        typed(&mut o, "a b", &Ctx(true));
        assert_eq!(o.input.text, "a b");
        results(&mut o, 3);
        o.key(&KeyEvent::new(Key::Down), &Ctx(true));
        let e = o.key(&KeyEvent::new(Key::Space), &Ctx(true));
        assert!(matches!(&e[0], Effect::Preview(r) if r[0].name == "f1.txt"));
        // Without Look, Space shows details.
        let e = o.key(&KeyEvent::new(Key::Space), &Ctx(false));
        assert!(e.is_empty());
        assert!(matches!(o.mode, Mode::Details { .. }));
        o.key(&KeyEvent::new(Key::Escape), &Ctx(false));
        assert!(matches!(o.mode, Mode::Results));
        // Typing a letter returns to the query.
        let e = o.key(&KeyEvent::ch('z'), &Ctx(true));
        assert_eq!(o.input.text, "a bz");
        assert!(matches!(e[0], Effect::Search { .. }));
        assert!(!o.nav);
    }

    #[test]
    fn empty_query_down_shows_recent() {
        let mut o = Overlay::default();
        o.show(None, None);
        let e = o.key(&KeyEvent::new(Key::Down), &Ctx(true));
        assert!(matches!(&e[0], Effect::Search { text, .. } if text.is_empty()));
        assert!(o.empty_open);
    }

    #[test]
    fn multi_select_and_actions() {
        let mut o = Overlay::default();
        o.show(Some("f"), None);
        results(&mut o, 4);
        let shift = Mods { shift: true, ..Mods::default() };
        o.key(&KeyEvent::new(Key::Down).with(shift), &Ctx(true));
        o.key(&KeyEvent::new(Key::Down).with(shift), &Ctx(true));
        assert_eq!(o.targets().len(), 3);
        let e = o.key(&KeyEvent::new(Key::Tab), &Ctx(true));
        let Effect::WantActions(t) = &e[0] else { panic!() };
        assert_eq!(t.len(), 3);
        let items = vec![
            ActionItem::new(ActionId::CopyPath, "Copy path", Glyph::Copy),
            ActionItem { danger: true, ..ActionItem::new(ActionId::Trash, "Move to Trash", Glyph::Trash) },
            ActionItem { keywords: "Arcade Shelf".into(), ..ActionItem::new(ActionId::CopyFile, "Add to collection", Glyph::App(AppGlyph::Other)) },
        ];
        o.set_actions(t.clone(), items);
        typed(&mut o, "shel", &Ctx(true));
        if let Mode::Actions { items, .. } = &o.mode {
            assert_eq!(items.len(), 1, "the filter matches the app name");
        }
        for _ in 0..4 {
            o.key(&KeyEvent::new(Key::Backspace), &Ctx(true));
        }
        typed(&mut o, "tra", &Ctx(true));
        if let Mode::Actions { items, .. } = &o.mode {
            assert_eq!(items.len(), 1);
        } else {
            panic!()
        }
        o.key(&KeyEvent::new(Key::Enter), &Ctx(true));
        assert!(matches!(o.mode, Mode::ConfirmTrash { .. }));
        let e = o.key(&KeyEvent::new(Key::Enter), &Ctx(true));
        assert!(matches!(&e[0], Effect::Trash(t) if t.len() == 3));
    }

    #[test]
    fn rename_selects_stem() {
        let mut o = Overlay::default();
        o.show(Some("f"), None);
        results(&mut o, 2);
        o.key(&KeyEvent::new(Key::F2), &Ctx(true));
        let Mode::Rename { input, .. } = &o.mode else { panic!() };
        assert_eq!(input.selected_text(), Some("f0"));
        typed(&mut o, "new", &Ctx(true));
        let e = o.key(&KeyEvent::new(Key::Enter), &Ctx(true));
        assert!(matches!(&e[0], Effect::Rename { new_name, .. } if new_name == "new.txt"));
    }

    #[test]
    fn copy_prefers_text_selection() {
        let mut o = Overlay::default();
        o.show(Some("query"), None);
        results(&mut o, 1);
        let ctrl = Mods { ctrl: true, logo: cfg!(target_os = "macos"), ..Mods::default() };
        // show() selects the prefilled query.
        assert_eq!(o.key(&KeyEvent::ch('c').with(ctrl), &Ctx(true)), vec![Effect::CopyText("query".into())]);
        o.key(&KeyEvent::new(Key::End), &Ctx(true));
        assert!(matches!(o.key(&KeyEvent::ch('c').with(ctrl), &Ctx(true))[0], Effect::CopyPaths(_)));
        let cs = Mods { shift: true, ..ctrl };
        assert!(matches!(o.key(&KeyEvent::ch('C').with(cs), &Ctx(true))[0], Effect::CopyFiles(_)));
    }

    #[test]
    fn stale_results_are_ignored_and_reveal_selects() {
        let mut o = Overlay::default();
        o.show(Some("f"), Some(PathBuf::from("/h/f2.txt")));
        assert!(!o.set_results(o.seq + 5, vec![], ResultsInfo::default()));
        results(&mut o, 4);
        assert_eq!(o.sel, 2);
        assert!(o.nav);
    }

    #[test]
    fn escape_hides() {
        let mut o = Overlay::default();
        o.show(None, None);
        assert_eq!(o.key(&KeyEvent::new(Key::Escape), &Ctx(true)), vec![Effect::Hide]);
    }
}
