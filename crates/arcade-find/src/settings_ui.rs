//! The Settings window: a separate egui process (`--settings-window`) that
//! runs only while open, so the resident overlay never carries a GPU
//! context. Template A from the Arcade design kit (`design::egui`): sidebar,
//! pages in the standard order, and the kit's rows and controls. Changes are
//! saved at once and the running instance reloads.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use arcade_link::client::{app_state, AppState};
use arcade_link::design::egui::components::{ButtonKind, Kit, NavItem, ShortcutRecorder, ShortcutSpec, Toast, Tone};
use arcade_link::design::format::{format_count, format_relative, format_size};
use arcade_link::design::size;
use arcade_link::manifest::ids;
use arcade_link::{Locations, Registry};
use eframe::egui;
use notify::Watcher;
use serde_json::Value;

use find_core::paths::{expand_tilde, tilde, AppPaths};
use find_core::Settings;

use crate::instance::{self, Command};
use crate::{autostart, hotkey, link, theme, VERSION};

/// The one-line description under every app name (README, Tools, tray).
pub const TAGLINE: &str = "Press a shortcut, type, and any file or folder shows up.";
/// The user guide, shipped with the app and published here.
pub const GUIDE_URL: &str = "https://github.com/qa-p1/Arcade-Find/tree/main/docs/user";
const CONNECTED_GUIDE_URL: &str = "https://github.com/qa-p1/Arcade-Find/blob/main/docs/user/with-other-apps.md";
const LICENCES_URL: &str = "https://github.com/qa-p1/Arcade-Find/blob/main/THIRD_PARTY_NOTICES.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Shortcuts,
    Locations,
    Index,
    Connected,
    About,
}

/// The standard order (STANDARD §6): General, Shortcuts, Find's own pages,
/// Connected apps, About.
const PAGES: [(Page, &str, &str); 6] = [
    (Page::General, "General", "settings"),
    (Page::Shortcuts, "Shortcuts", "keyboard"),
    (Page::Locations, "Locations", "folder"),
    (Page::Index, "Index", "list"),
    (Page::Connected, "Connected apps", "apps"),
    (Page::About, "About", "info"),
];

/// Results of background work.
enum Bg {
    LoginState(Option<bool>),
    LoginSaved(Result<bool, String>),
    Settings(Settings),
    Integration {
        hints: Vec<String>,
        channel: Option<String>,
    },
    Status(Option<Value>),
    Registry(Registry, Vec<(String, AppState)>),
    Ripgrep(Option<find_core::content::Ripgrep>),
    Saved(Result<(), String>),
    Toast(String, Tone),
    /// Another launch asked for Settings: come to the front.
    Focus,
}

struct SettingsApp {
    login_state: Option<bool>,
    login_pending: bool,
    _watch: Option<notify::RecommendedWatcher>,
    integration_hints: Vec<String>,
    channel: Option<String>,
    paths: AppPaths,
    locations: Locations,
    settings: Settings,
    page: usize,
    /// The running instance's status; `None` before the first answer.
    status: Option<Option<Value>>,
    status_at: Option<Instant>,
    registry: Option<Registry>,
    states: Vec<(String, AppState)>,
    ripgrep: Option<Option<find_core::content::Ripgrep>>,
    tx: Sender<Bg>,
    rx: Receiver<Bg>,
    new_root: String,
    new_exclude: String,
    exclude_names: String,
    recorder: ShortcutRecorder,
    toast: Option<Toast>,
    drives: Vec<PathBuf>,
    glyphs: Vec<(String, egui::TextureHandle)>,
    rescanning: bool,
}

/// The file another launch touches to bring an open Settings window forward.
pub fn focus_request(paths: &AppPaths) -> PathBuf {
    paths.runtime.join("settings.focus")
}

pub fn run(paths: AppPaths) -> ExitCode {
    let (settings, _) = Settings::load(&paths.settings_file());
    theme::refresh_system_theme();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Settings — Arcade Find")
            .with_app_id(crate::os::APP_ID)
            .with_inner_size([size::SETTINGS_WIDTH, size::SETTINGS_HEIGHT])
            .with_min_inner_size([size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT])
            .with_icon(egui::IconData { rgba: crate::ui::icons::app_icon_rgba(64), width: 64, height: 64 }),
        ..Default::default()
    };
    let r = eframe::run_native(
        "Arcade Find",
        options,
        Box::new(move |cc| {
            arcade_link::design::egui::install_inter(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx, settings.theme);
            // Explicit text scaling for the native UI matrix (also useful on
            // desktops that do not publish a system text-scale preference).
            if let Ok(scale) = std::env::var("ARCADE_FIND_UI_SCALE") {
                if let Ok(scale) = scale.parse::<f32>() {
                    if (1.0..=1.25).contains(&scale) {
                        cc.egui_ctx.set_zoom_factor(scale);
                    }
                }
            }
            Ok(Box::new(SettingsApp::new(paths, settings, cc.egui_ctx.clone())))
        }),
    );
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arcade-find: settings: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Applies the kit for the Appearance setting. "System" is resolved here
/// (the XDG portal, the registry or `defaults`), since winit can't report
/// the system theme on every Linux desktop.
fn apply_theme(ctx: &egui::Context, t: find_core::settings::Theme) {
    let resolved = if theme::kit_theme(t).is_dark() { "dark" } else { "light" };
    arcade_link::design::apply_egui(ctx, resolved, link::ME);
}

fn texture(ctx: &egui::Context, name: &str, rgba: Vec<u8>, px: u32) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_unmultiplied([px as usize, px as usize], &rgba);
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

impl SettingsApp {
    fn new(paths: AppPaths, settings: Settings, ctx: egui::Context) -> SettingsApp {
        let (tx, rx) = channel();
        let exclude_names = settings.exclude_names.join(", ");
        let (watch_tx, settings_path, focus_path, watch_ctx) = (tx.clone(), paths.settings_file(), focus_request(&paths), ctx.clone());
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            if event.paths.contains(&settings_path) {
                if let Some(s) = std::fs::read(&settings_path).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
                    let _ = watch_tx.send(Bg::Settings(s));
                    watch_ctx.request_repaint();
                }
            }
            if event.paths.contains(&focus_path) {
                let _ = watch_tx.send(Bg::Focus);
                watch_ctx.request_repaint();
            }
        })
        .ok();
        for dir in [&paths.config, &paths.runtime] {
            if std::fs::create_dir_all(dir).is_ok() {
                if let Some(w) = &mut watcher {
                    let _ = w.watch(dir, notify::RecursiveMode::NonRecursive);
                }
            }
        }
        // The system theme changes live (the XDG portal on Linux).
        let theme_ctx = ctx.clone();
        theme::watch_system_theme(move || theme_ctx.request_repaint());
        let mut glyphs = vec![("brand".to_string(), texture(&ctx, "brand", crate::ui::icons::glyph_mask_rgba(link::ME, 64), 64))];
        for id in link::apps::known() {
            glyphs.push((id.to_string(), texture(&ctx, id, crate::ui::icons::glyph_mask_rgba(id, 64), 64)));
        }
        let app = SettingsApp {
            login_state: None,
            login_pending: false,
            _watch: watcher,
            integration_hints: Vec::new(),
            channel: None,
            locations: Locations::discover(),
            paths,
            settings,
            page: 0,
            status: None,
            status_at: None,
            registry: None,
            states: Vec::new(),
            ripgrep: None,
            tx,
            rx,
            new_root: String::new(),
            new_exclude: String::new(),
            exclude_names,
            recorder: ShortcutRecorder::default(),
            toast: None,
            drives: find_core::mounts::MountTable::read().suggested_drives(),
            glyphs,
            rescanning: false,
        };
        app.refresh_status(&ctx);
        app.refresh_login(&ctx);
        app.refresh_registry(&ctx);
        let tx = app.tx.clone();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Bg::Ripgrep(find_core::content::detect(&[])));
            let mut hints = Vec::new();
            let mut channel = None;
            if let Ok(environment) = arcade_link::install::Environment::discover() {
                if let Ok(status) = arcade_link::install::integration_status(&environment, link::ME, &std::env::var("SHELL").unwrap_or_default()) {
                    if let Some(hint) = status.path_hint {
                        hints.push(hint);
                    }
                    channel = status.receipt.as_ref().map(|r| r.channel.clone());
                    #[cfg(target_os = "macos")]
                    if let Some(receipt) = status.receipt {
                        if receipt.integration.cli.is_none() {
                            hints.push(arcade_link::install::platform::macos::cli_symlink_offer(
                                &environment,
                                crate::installer::CLI_NAME,
                                &crate::installer::launch_path(&receipt),
                            ));
                        }
                    }
                }
            }
            let _ = tx.send(Bg::Integration { hints, channel });
            c.request_repaint();
        });
        app
    }

    fn refresh_status(&self, ctx: &egui::Context) {
        let (tx, p, c) = (self.tx.clone(), self.paths.clone(), ctx.clone());
        std::thread::spawn(move || {
            let s = instance::send(&p, &Command::Status, Duration::from_secs(2)).ok();
            let _ = tx.send(Bg::Status(s));
            c.request_repaint();
        });
    }

    fn refresh_login(&self, ctx: &egui::Context) {
        let (tx, paths, ctx) = (self.tx.clone(), self.paths.clone(), ctx.clone());
        std::thread::spawn(move || {
            let state = autostart::allowed(&paths).then(autostart::enabled);
            let _ = tx.send(Bg::LoginState(state));
            ctx.request_repaint();
        });
    }

    fn refresh_registry(&self, ctx: &egui::Context) {
        let (tx, loc, c) = (self.tx.clone(), self.locations.clone(), ctx.clone());
        std::thread::spawn(move || {
            let reg = Registry::load(&loc);
            let me = link::me();
            let states = app_ids(&reg).into_iter().map(|id| (id.clone(), app_state(&loc, &reg, &id, &me))).collect();
            let _ = tx.send(Bg::Registry(reg, states));
            c.request_repaint();
        });
    }

    /// Saves and tells the running instance (off the UI thread).
    fn save(&mut self, ctx: &egui::Context) {
        self.settings.sanitize();
        let (tx, p, s, c) = (self.tx.clone(), self.paths.clone(), self.settings.clone(), ctx.clone());
        apply_theme(ctx, s.theme);
        std::thread::spawn(move || {
            let r = s.save(&p.settings_file()).map_err(|_| "Couldn't save settings. Check that the settings folder is writable.".to_string());
            if r.is_ok() {
                let _ = instance::send(&p, &Command::Reload, Duration::from_secs(3));
            }
            let _ = tx.send(Bg::Saved(r));
            c.request_repaint();
        });
    }

    fn command(&self, ctx: &egui::Context, cmd: Command, done: Option<&'static str>) {
        let (tx, p, c) = (self.tx.clone(), self.paths.clone(), ctx.clone());
        std::thread::spawn(move || {
            match instance::send(&p, &cmd, Duration::from_secs(3)) {
                Ok(v) if v["ok"] == false => {
                    let why = v["error"].as_str().unwrap_or("It's busy").trim_end_matches('.').to_string();
                    let _ = tx.send(Bg::Toast(format!("Couldn't do that now. {why}. Try again in a moment."), Tone::Danger));
                }
                Ok(_) => {
                    if let Some(done) = done {
                        let _ = tx.send(Bg::Toast(done.to_string(), Tone::Ok));
                    }
                }
                Err(_) => {
                    let _ = tx.send(Bg::Toast("Arcade Find isn't running. Start it from your apps menu.".to_string(), Tone::Danger));
                }
            }
            c.request_repaint();
        });
    }

    fn toast(&mut self, message: impl Into<String>, tone: Tone) {
        self.toast = Some(Toast::new(message, tone));
    }

    fn copy(&mut self, ctx: &egui::Context, text: String) {
        ctx.copy_text(text);
        self.toast = Some(Toast::copied());
    }

    fn open_url(&mut self, url: &str) {
        if open::that_detached(url).is_err() {
            self.toast("Couldn't open the browser. Copy the address from the user guide instead.", Tone::Danger);
        }
    }

    fn open_path(&mut self, path: &Path) {
        if path.extension().is_none() {
            let _ = std::fs::create_dir_all(path);
        }
        if open::that_detached(path).is_err() {
            self.toast("Couldn't open it. Check that a file manager is installed.", Tone::Danger);
        }
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Bg::LoginState(state) => self.login_state = state,
                Bg::LoginSaved(result) => {
                    self.login_pending = false;
                    match result {
                        Ok(on) => {
                            self.login_state = Some(on);
                            self.settings.start_at_login = Some(on);
                            self.save(ctx);
                        }
                        Err(_) => self.toast("Couldn't change Start at login. Check your desktop's autostart folder.", Tone::Danger),
                    }
                }
                Bg::Settings(s) => {
                    if s != self.settings {
                        self.exclude_names = s.exclude_names.join(", ");
                        self.settings = s;
                        apply_theme(ctx, self.settings.theme);
                        self.refresh_status(ctx);
                    }
                }
                Bg::Integration { hints, channel } => {
                    self.integration_hints = hints;
                    self.channel = channel;
                }
                Bg::Status(s) => {
                    if self.rescanning
                        && s.as_ref().is_some_and(|s| s["engine"]["phase"] != "crawling")
                        && self.status_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3))
                    {
                        self.rescanning = false;
                    }
                    self.status = Some(s);
                    self.status_at = Some(Instant::now());
                }
                Bg::Registry(r, st) => {
                    self.registry = Some(r);
                    self.states = st;
                }
                Bg::Ripgrep(r) => self.ripgrep = Some(r),
                Bg::Saved(Err(e)) => self.toast(e, Tone::Danger),
                Bg::Toast(m, tone) => self.toast(m, tone),
                Bg::Saved(Ok(())) => {}
                Bg::Focus => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
    }

    fn glyph(&self, id: &str) -> Option<egui::TextureId> {
        self.glyphs.iter().find(|(g, _)| g == id).map(|(_, t)| t.id())
    }

    fn page_changed(&mut self, ctx: &egui::Context) {
        match PAGES[self.page].0 {
            Page::General => self.refresh_login(ctx),
            Page::Index | Page::Shortcuts | Page::About => self.refresh_status(ctx),
            Page::Connected => {
                self.refresh_registry(ctx);
                self.refresh_status(ctx);
            }
            Page::Locations => {}
        }
    }
}

/// The apps the Connected apps page lists: the family, plus any other
/// installed Arcade app (a new app shows up without a Find update).
fn app_ids(reg: &Registry) -> Vec<String> {
    let mut v: Vec<String> = link::apps::known().iter().map(|s| s.to_string()).collect();
    for m in reg.apps() {
        if m.id != link::ME && m.id != ids::TOOLS && !v.contains(&m.id) && m.id.starts_with("arcade.") {
            v.push(m.id.clone());
        }
    }
    v
}

fn path_valid(p: &Path) -> bool {
    p.is_absolute() && p.is_dir()
}

/// Small muted text under a list (`.section-note`).
fn note(ui: &mut egui::Ui, kit: &Kit, text: &str) {
    ui.add_space(8.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(arcade_link::design::egui::font(arcade_link::design::text::SMALL))
                .color(arcade_link::design::egui::colour(kit.palette.muted)),
        )
        .wrap(),
    );
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain(ctx);
        // Follow a System appearance live.
        if self.settings.theme == find_core::settings::Theme::System {
            let dark = theme::system_dark();
            if ctx.style().visuals.dark_mode != dark {
                apply_theme(ctx, self.settings.theme);
            }
        }
        let page = PAGES[self.page].0;
        // While open, the Index page refreshes every 2 s.
        if page == Page::Index {
            if self.status_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
                self.status_at = Some(Instant::now());
                self.refresh_status(ctx);
            }
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        self.keys(ctx);
        let kit = Kit::for_theme(if ctx.style().visuals.dark_mode { arcade_link::design::Theme::Dark } else { arcade_link::design::Theme::Light }, link::ME);
        let items: Vec<NavItem> = PAGES.iter().map(|&(_, label, icon)| NavItem { label, icon, count: None }).collect();
        let brand = self.glyph("brand");
        if kit.sidebar_with_brand(ctx, "Arcade Find", VERSION, brand, &items, &mut self.page) {
            self.page_changed(ctx);
        }
        kit.page(ctx, |ui| match page {
            Page::General => self.general(ui, &kit, ctx),
            Page::Shortcuts => self.shortcuts(ui, &kit, ctx),
            Page::Locations => self.locations_page(ui, &kit, ctx),
            Page::Index => self.index(ui, &kit, ctx),
            Page::Connected => self.connected(ui, &kit, ctx),
            Page::About => self.about(ui, &kit, ctx),
        });
        if let Some(t) = &self.toast {
            if !t.show(ctx, &kit) {
                self.toast = None;
            }
        }
    }
}

impl SettingsApp {
    /// Esc closes the window (unless a control uses it), F1 opens the guide,
    /// Ctrl+W closes.
    fn keys(&mut self, ctx: &egui::Context) {
        let editing = ctx.wants_keyboard_input();
        let (esc, f1, close) =
            ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::F1), i.modifiers.command && i.key_pressed(egui::Key::W)));
        if (esc && !editing && !self.recorder.recording) || close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if f1 {
            self.open_url(GUIDE_URL);
        }
    }

    fn general(&mut self, ui: &mut egui::Ui, kit: &Kit, ctx: &egui::Context) {
        use find_core::settings::Theme;
        kit.page_head(ui, "General", "How Find looks, starts and lists results.");
        let mut changed = false;
        kit.list(ui, |ui| {
            kit.row(ui, "Appearance", "Follow the system, or always use light or dark.", |ui| {
                changed |= kit.segmented(ui, &mut self.settings.theme, &[(Theme::System, "System"), (Theme::Light, "Light"), (Theme::Dark, "Dark")]).changed();
            });
            let allowed = self.login_state.is_some() && !self.login_pending;
            // The login entry is the truth (Arcade Tools can switch it too).
            let mut login = self.login_state.unwrap_or(self.settings.start_at_login.unwrap_or(false));
            let description = if self.login_state.is_none() && !self.login_pending {
                "Available for installed copies, not development builds or test profiles."
            } else if cfg!(target_os = "linux") && hotkey::hyprland::active() {
                "Hyprland doesn't run autostart entries itself: add exec-once = arcade-find --background to your Hyprland config."
            } else {
                "Starts in the background, with the tray icon only."
            };
            let r = ui.add_enabled_ui(allowed, |ui| kit.switch_row(ui, &mut login, "Start at login", description)).inner;
            if r.changed() {
                self.login_pending = true;
                let (tx, ctx) = (self.tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let result = autostart::set(login).map(|_| login);
                    let _ = tx.send(Bg::LoginSaved(result));
                    ctx.request_repaint();
                });
            }
            changed |= kit
                .switch_row(ui, &mut self.settings.show_recent_on_open, "Show recent and pinned on open", "Lists them as soon as the search opens.")
                .changed();
            changed |= kit.switch_row(ui, &mut self.settings.show_hidden, "Show hidden files", "Ctrl+H switches this in the search too.").changed();
        });

        kit.section_head(ui, "Results");
        kit.list(ui, |ui| {
            let visible: Vec<(u32, String)> = (3..=12).map(|n| (n, format!("{n} rows"))).collect();
            let options: Vec<(u32, &str)> = visible.iter().map(|(n, l)| (*n, l.as_str())).collect();
            kit.row(ui, "Rows shown", "How many results show before the list scrolls.", |ui| {
                changed |= kit.select(ui, "rows-visible", &mut self.settings.rows.visible, &options).changed();
            });
            let stretch: Vec<(u32, String)> = (self.settings.rows.visible..=16).map(|n| (n, format!("{n} rows"))).collect();
            let options: Vec<(u32, &str)> = stretch.iter().map(|(n, l)| (*n, l.as_str())).collect();
            kit.row(ui, "Growing up to", "The list grows to this many rows before it scrolls.", |ui| {
                changed |= kit.select(ui, "rows-stretch", &mut self.settings.rows.stretch, &options).changed();
            });
            let rg = match &self.ripgrep {
                None => "Looking for ripgrep…".to_string(),
                Some(Some(rg)) => format!("Start a search with / to look inside files. Uses ripgrep {}.", rg.version),
                Some(None) => "Start a search with / to look inside files. Needs ripgrep.".to_string(),
            };
            changed |= kit.switch_row(ui, &mut self.settings.content_search, "Search inside files", &rg).changed();
        });
        if matches!(self.ripgrep, Some(None)) && self.settings.content_search {
            ui.add_space(10.0);
            kit.notice(ui, "ripgrep isn't installed. Install the \u{201c}ripgrep\u{201d} package to search inside files; Find never downloads it.", Tone::Warn);
        }

        if !self.integration_hints.is_empty() {
            kit.section_head(ui, "Terminal command");
            let hints = self.integration_hints.clone();
            let mut copy = None;
            kit.list(ui, |ui| {
                for hint in &hints {
                    kit.row(ui, hint, "Run this yourself to use arcade-find in a terminal. Find never edits shell profiles.", |ui| {
                        if kit.button_state(ui, "Copy", ButtonKind::Default, true, false, true).clicked() {
                            copy = Some(hint.clone());
                        }
                    });
                }
            });
            if let Some(c) = copy {
                self.copy(ctx, c);
            }
        }
        if changed {
            self.save(ctx);
        }
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui, kit: &Kit, ctx: &egui::Context) {
        kit.page_head(ui, "Shortcuts", "The shortcut that opens Find, and the keys inside the search.");
        let state = self.status.clone().flatten().map(|s| s["shortcut"].clone());
        let status = match state.as_ref().and_then(|s| s["mode"].as_str()) {
            Some("native") => "Registered with the system.".to_string(),
            Some("hyprland") => "Bound in the running Hyprland session.".to_string(),
            Some("manual") | Some("failed") => "Not active yet: see below.".to_string(),
            Some(_) => String::new(),
            None if self.status.is_none() => "Checking…".to_string(),
            None => "Arcade Find isn't running, so the shortcut isn't active.".to_string(),
        };
        let registry = self.registry.clone();
        let mut changed = false;
        kit.list(ui, |ui| {
            let spec = ShortcutSpec { title: "Open Arcade Find", description: &status, default: find_core::settings::DEFAULT_SHORTCUT, allow_clear: false };
            changed |= self.recorder.row(ui, kit, &spec, &mut self.settings.shortcut, |accelerator| {
                registry
                    .as_ref()
                    .and_then(|r| r.shortcut_owner(link::ME, accelerator))
                    .or_else(|| hotkey::known_conflict(accelerator).map(|what| format!("{what} (usually)")))
            });
        });
        if let Some(what) = hotkey::known_conflict(&self.settings.shortcut) {
            ui.add_space(10.0);
            kit.notice(ui, &format!("Usually used by {what}."), Tone::Warn);
        }
        if let Some(st) = state.as_ref().filter(|s| matches!(s["mode"].as_str(), Some("manual" | "failed"))) {
            let why = st["reason"].as_str().or(st["error"].as_str()).unwrap_or("The desktop didn't accept the shortcut.").to_string();
            ui.add_space(10.0);
            kit.notice(ui, &why, Tone::Warn);
            if let Some(cmd) = st["command"].as_str().map(str::to_string) {
                ui.add_space(10.0);
                let mut copy = false;
                kit.list(ui, |ui| {
                    kit.row(ui, &cmd, "Bind this command to a shortcut in your desktop's keyboard settings.", |ui| {
                        copy = kit.button_state(ui, "Copy", ButtonKind::Default, true, false, true).clicked();
                    });
                });
                if copy {
                    self.copy(ctx, cmd);
                }
            }
        }
        if cfg!(target_os = "linux") {
            kit.section_head(ui, "Hyprland");
            let line = hotkey::hyprland::config_line(&self.settings.shortcut, &hotkey::toggle_command());
            let mut copy = None;
            kit.list(ui, |ui| {
                changed |= kit
                    .switch_row(
                        ui,
                        &mut self.settings.hyprland_runtime_bind,
                        "Add as a runtime key binding",
                        "Only for this Hyprland session: Find never edits your Hyprland config.",
                    )
                    .changed();
                if let Some(line) = &line {
                    kit.row(ui, line, "Or add this line to your Hyprland config yourself.", |ui| {
                        if kit.button_state(ui, "Copy", ButtonKind::Default, true, false, true).clicked() {
                            copy = Some(line.clone());
                        }
                    });
                }
            });
            if let Some(c) = copy {
                self.copy(ctx, c);
            }
        }
        kit.section_head(ui, "Keys in the search");
        kit.list(ui, |ui| {
            for (title, keys) in overlay_keys() {
                kit.row(ui, &title, "", |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, combo) in keys.iter().enumerate() {
                        if i > 0 {
                            ui.label(
                                egui::RichText::new("or")
                                    .font(arcade_link::design::egui::font(arcade_link::design::text::CAPTION))
                                    .color(arcade_link::design::egui::colour(kit.palette.muted)),
                            );
                        }
                        for key in combo.split('+') {
                            kit.key_chip(ui, &crate::ui::render::key_label(key));
                        }
                    }
                });
            }
        });
        if changed {
            self.save(ctx);
            self.status_at = None;
            self.refresh_status(ctx);
        }
    }

    fn locations_page(&mut self, ui: &mut egui::Ui, kit: &Kit, ctx: &egui::Context) {
        kit.page_head(ui, "Locations", "The folders Find indexes, and what it skips.");
        let mut changed = false;
        // Folders dropped on the window are added to the index.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in dropped.into_iter().filter(|p| path_valid(p)) {
            let t = tilde(&p);
            if !self.settings.roots.contains(&t) {
                self.settings.roots.push(t);
                changed = true;
            }
        }
        kit.section_head(ui, "Folders to index");
        let mut remove = None;
        let only_one = self.settings.roots.len() <= 1;
        kit.list(ui, |ui| {
            for (i, r) in self.settings.roots.iter().enumerate() {
                let shown = expand_tilde(r);
                kit.row_with_glyph(ui, None, "folder", r, &shown.display().to_string(), |ui| {
                    let label = if only_one { "The last folder can't be removed" } else { "Remove" };
                    if kit.button_state(ui, "Remove", ButtonKind::Ghost, !only_one, false, true).on_hover_text(label).clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some(i) = remove {
            self.settings.roots.remove(i);
            changed = true;
        }
        ui.add_space(10.0);
        let p = expand_tilde(self.new_root.trim());
        let invalid =
            (!self.new_root.trim().is_empty() && !path_valid(&p)).then_some("Couldn't find that folder. Type a full path, such as /mnt/Data or ~/Projects.");
        let mut add = false;
        kit.list(ui, |ui| {
            kit.row(ui, "Add folder", "Type a path, or drop a folder on this window.", |ui| {
                add = kit.button_state(ui, "Add folder", ButtonKind::Primary, path_valid(&p), false, true).clicked();
            });
            let r = kit.text_input(ui, &mut self.new_root, "/mnt/Data or ~/Projects", invalid);
            add |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && path_valid(&p);
            ui.add_space(10.0);
        });
        if add {
            self.settings.roots.push(tilde(&p));
            self.new_root.clear();
            changed = true;
        }
        let suggestions: Vec<PathBuf> = self.drives.iter().filter(|d| !self.settings.roots.iter().any(|r| expand_tilde(r) == **d)).cloned().collect();
        if !suggestions.is_empty() {
            kit.section_head(ui, "Other drives");
            kit.list(ui, |ui| {
                for d in suggestions {
                    let shown = d.display().to_string();
                    kit.row_with_glyph(ui, None, "monitor", &shown, "Not indexed unless you add it.", |ui| {
                        if kit.button_state(ui, "Add", ButtonKind::Default, true, false, true).clicked() {
                            self.settings.roots.push(shown.clone());
                            changed = true;
                        }
                    });
                }
            });
        }

        kit.section_head(ui, "Skip");
        kit.list(ui, |ui| {
            kit.row(ui, "Names", "Skipped wherever they appear. Separate with commas; * and ? work.", |_| ());
            let r = kit.text_input(ui, &mut self.exclude_names, "node_modules, *.tmp", None);
            if r.lost_focus() {
                let names: Vec<String> = self.exclude_names.split([',', '\n']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                if names != self.settings.exclude_names {
                    self.settings.exclude_names = names;
                    changed = true;
                }
            }
            ui.add_space(10.0);
            changed |= kit.switch_row(ui, &mut self.settings.skip_network_mounts, "Skip network drives", "NFS, SMB, SSHFS and other network mounts.").changed();
        });

        kit.section_head(ui, "Skipped folders");
        let mut remove = None;
        kit.list(ui, |ui| {
            for (i, r) in self.settings.exclude_paths.iter().enumerate() {
                kit.row_with_glyph(ui, None, "folder", r, "Skipped with everything inside it.", |ui| {
                    if kit.button_state(ui, "Remove", ButtonKind::Ghost, true, false, true).clicked() {
                        remove = Some(i);
                    }
                });
            }
            let p = expand_tilde(self.new_exclude.trim());
            let mut add = false;
            kit.row(ui, "Skip a folder", "Type its full path.", |ui| {
                add = kit.button_state(ui, "Add", ButtonKind::Default, p.is_absolute(), false, true).clicked();
            });
            let r = kit.text_input(ui, &mut self.new_exclude, "~/Downloads/old", None);
            add |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && p.is_absolute();
            ui.add_space(10.0);
            if add {
                self.settings.exclude_paths.push(tilde(&p));
                self.new_exclude.clear();
                changed = true;
            }
        });
        if let Some(i) = remove {
            self.settings.exclude_paths.remove(i);
            changed = true;
        }
        note(ui, kit, "Symbolic links are never followed, so nothing is indexed twice.");
        if changed {
            self.save(ctx);
        }
    }

    fn index(&mut self, ui: &mut egui::Ui, kit: &Kit, ctx: &egui::Context) {
        let Some(status) = self.status.clone() else {
            kit.page_head(ui, "Index", "What Find knows about your files.");
            kit.list(ui, |ui| {
                for _ in 0..3 {
                    kit.skeleton_row(ui);
                }
            });
            return;
        };
        let Some(st) = status else {
            kit.page_head(ui, "Index", "What Find knows about your files.");
            let mut start = false;
            let mut retry = false;
            kit.empty_state(ui, "search", "Arcade Find isn't running", "Start it to build and update the index.", |ui| {
                ui.horizontal(|ui| {
                    start = kit.button(ui, "Start Arcade Find", ButtonKind::Primary).clicked();
                    retry = kit.button(ui, "Retry", ButtonKind::Default).clicked();
                });
            });
            if start {
                let exe = arcade_link::manifest::current_executable();
                let mut command = std::process::Command::new(exe);
                if arcade_link::childenv::detach_from_image(&mut command).arg("--background").spawn().is_err() {
                    self.toast("Couldn't start Arcade Find. Open it from your apps menu.", Tone::Danger);
                }
                self.status_at = Some(Instant::now() - Duration::from_secs(1));
            }
            if retry {
                self.refresh_status(ctx);
            }
            return;
        };
        let e = &st["engine"];
        let phase = e["phase"].as_str().unwrap_or("");
        let entries = e["entries"].as_u64().unwrap_or(0);
        let count = |n: u64| format_count(n.min(i64::MAX as u64) as i64);
        let headline = match phase {
            "crawling" => format!("Indexing… {} files so far", count(e["progress"]["entries"].as_u64().unwrap_or(entries))),
            "loading" => "Loading the saved index…".to_string(),
            "reconciling" => format!("{} files · checking for changes", count(entries)),
            _ => format!("{} files indexed", count(entries)),
        };
        let busy = self.rescanning || phase == "crawling";
        let mut rescan = false;
        kit.page_head_with(ui, "Index", &headline, |ui| {
            rescan = kit.button_state(ui, if busy { "Rescanning" } else { "Rescan now" }, ButtonKind::Primary, true, busy, false).clicked();
        });
        if rescan {
            self.rescanning = true;
            self.command(ctx, Command::Rescan, None);
        }
        if let Some(err) = e["lastError"].as_str() {
            kit.notice(ui, err, Tone::Danger);
            ui.add_space(10.0);
        }
        let now = find_core::now_secs();
        let offset = (find_core::local_offset_secs() / 60) as i32;
        let mut rescan_hours = self.settings.rescan_hours;
        let mut copy = None;
        kit.list(ui, |ui| {
            if let Some(cur) = e["progress"]["current"].as_str().filter(|c| !c.is_empty() && phase == "crawling") {
                kit.row(ui, "Now scanning", cur, |_| ());
            }
            kit.row(ui, "Memory used", "The index in memory, while Find runs.", |ui| {
                ui.label(format_size(e["indexBytes"].as_u64().unwrap_or(0)));
            });
            if let Some(f) = e["indexFile"].as_str() {
                let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
                kit.row(ui, "Saved index", f, |ui| {
                    ui.label(format_size(size));
                });
            }
            if let Some(t) = e["lastFullScan"].as_i64().filter(|t| *t > 0) {
                let took = arcade_link::design::format::format_duration(
                    (e["lastScanMs"].as_u64().unwrap_or(0) / 1000) as f64,
                    arcade_link::design::format::DurationStyle::Job,
                );
                kit.row(ui, "Last full scan", &format!("Took {took}."), |ui| {
                    ui.label(format_relative(t * 1000, now * 1000, offset));
                });
            }
            let hours = [1u32, 2, 4, 6, 12, 24, 48, 72, 168];
            let labels: Vec<(u32, String)> = hours.iter().map(|&h| (h, if h == 1 { "Every hour".into() } else { format!("Every {h} hours") })).collect();
            let options: Vec<(u32, &str)> = labels.iter().map(|(h, l)| (*h, l.as_str())).collect();
            kit.row(ui, "Full rescan", "Live updates keep the index current in between.", |ui| {
                kit.select(ui, "rescan", &mut rescan_hours, &options);
            });
        });
        kit.section_head(ui, "Live updates");
        let w = &e["watch"];
        kit.list(ui, |ui| {
            kit.row(ui, w["detail"].as_str().unwrap_or("Watching for changes"), "", |_| ());
            if let Some(cmd) = w["fixCommand"].as_str() {
                kit.row(ui, cmd, "Raise the limit to watch every folder. Find never changes system settings itself.", |ui| {
                    if kit.button_state(ui, "Copy", ButtonKind::Default, true, false, true).clicked() {
                        copy = Some(cmd.to_string());
                    }
                });
            }
        });
        if let Some(c) = copy {
            self.copy(ctx, c);
        }
        if rescan_hours != self.settings.rescan_hours {
            self.settings.rescan_hours = rescan_hours;
            self.save(ctx);
        }
    }

    fn connected(&mut self, ui: &mut egui::Ui, kit: &Kit, ctx: &egui::Context) {
        kit.page_head(ui, "Connected apps", "Which Arcade apps work with Find, and how.");
        let mut changed = false;
        if !self.settings.link.enabled {
            let mut on = false;
            kit.notice_action(ui, "Connections are off in this app", Tone::Neutral, |ui| {
                on = kit.button_state(ui, "Turn on", ButtonKind::Primary, true, false, true).clicked();
            });
            if on {
                self.settings.link.enabled = true;
                changed = true;
            }
            ui.add_space(10.0);
        }
        kit.list(ui, |ui| {
            changed |= kit
                .switch_row(
                    ui,
                    &mut self.settings.link.enabled,
                    "Connect with other Arcade apps",
                    "Their actions show for your results (Tab), and they can search with Find.",
                )
                .changed();
        });
        kit.section_head(ui, "Apps");
        if self.registry.is_none() {
            kit.list(ui, |ui| {
                for _ in 0..3 {
                    kit.skeleton_row(ui);
                }
            });
        }
        let tools_installed = self.registry.as_ref().is_some_and(|r| r.get(ids::TOOLS).is_some());
        let mut install = None;
        let mut open_app = None;
        let states = self.states.clone();
        let link_on = self.settings.link.enabled;
        if !states.is_empty() {
            kit.list(ui, |ui| {
                for (id, state) in &states {
                    let name = self.registry.as_ref().and_then(|r| r.get(id).map(|m| m.name.clone())).unwrap_or_else(|| link::apps::name(id).to_string());
                    let installed = !matches!(state, AppState::NotInstalled);
                    let mut use_it = !self.settings.link.disabled_peers.contains(id);
                    let (badge, tone) = match state {
                        AppState::Running { .. } if link_on && use_it => ("Connected", Tone::Ok),
                        AppState::NotInstalled => ("Not installed", Tone::Neutral),
                        _ => ("Not running", Tone::Neutral),
                    };
                    let glyph = self.glyph(id);
                    kit.row_with_glyph(ui, glyph, "apps", &name, link::apps::pitch(id), |ui| {
                        kit.badge(ui, badge, tone);
                        if installed {
                            let r = ui.add_enabled_ui(link_on, |ui| kit.switch(ui, &mut use_it, &format!("Use {name} with Arcade Find"))).inner;
                            if r.changed() {
                                self.settings.link.disabled_peers.retain(|p| p != id);
                                if !use_it {
                                    self.settings.link.disabled_peers.push(id.clone());
                                }
                                changed = true;
                            }
                            if kit.button_state(ui, "Open", ButtonKind::Default, true, false, true).clicked() {
                                open_app = Some(id.clone());
                            }
                        } else if link::apps::known().contains(&id.as_str())
                            && kit.button_state(ui, "Install", ButtonKind::Default, true, false, true).clicked()
                        {
                            install = Some(id.clone());
                        }
                    });
                }
            });
        }
        if let Some(id) = install {
            self.install_app(ctx, &id, tools_installed);
        }
        if let Some(id) = open_app {
            self.open_app(&id);
        }
        ui.add_space(16.0);
        let heading = |t: &str| egui::RichText::new(t).font(arcade_link::design::egui::font(arcade_link::design::text::HEADING));
        let activity: Vec<(i64, String)> = self
            .status
            .clone()
            .flatten()
            .and_then(|s| s["activity"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|a| Some((a["at"].as_i64()?, a["text"].as_str()?.to_string())))
            .collect();
        egui::CollapsingHeader::new(heading("Recent activity")).id_salt("activity").show(ui, |ui| {
            if activity.is_empty() {
                kit.empty_state(ui, "clock", "No activity yet", "Use another app's action on a result to see it here.", |_| ());
            } else {
                let now = find_core::now_secs() * 1000;
                let offset = (find_core::local_offset_secs() / 60) as i32;
                kit.list(ui, |ui| {
                    for (at, text) in &activity {
                        kit.row(ui, text, &format_relative(at * 1000, now, offset), |_| ());
                    }
                });
            }
        });
        let diagnostics: Vec<(String, String)> =
            arcade_link::presence::diagnostics(None, &self.locations).into_iter().filter(|(k, _)| k != "Endpoint" && k != "Last error").collect();
        let (endpoint, last) = match self.status.clone().flatten().map(|s| s["link"].clone()) {
            Some(l) => (if l["listening"] == true { "Listening" } else { "Not listening" }.to_string(), l["lastError"].as_str().unwrap_or("None").to_string()),
            None => ("Arcade Find isn't running".to_string(), "None".to_string()),
        };
        egui::CollapsingHeader::new(heading("Diagnostics")).id_salt("diagnostics").show(ui, |ui| {
            kit.list(ui, |ui| {
                for (k, v) in &diagnostics {
                    kit.row(ui, k, v, |_| ());
                }
                kit.row(ui, "Endpoint", &endpoint, |_| ());
                kit.row(ui, "Last error", &last, |_| ());
            });
        });
        ui.add_space(10.0);
        if kit.button(ui, "How it works", ButtonKind::Ghost).clicked() {
            self.open_url(CONNECTED_GUIDE_URL);
        }
        if changed {
            self.save(ctx);
            self.refresh_registry(ctx);
        }
    }

    fn open_app(&mut self, id: &str) {
        let Some(m) = self.registry.as_ref().and_then(|r| r.get(id)).cloned() else {
            return self.toast("Couldn't open the app. It isn't installed any more.", Tone::Danger);
        };
        let mut command = std::process::Command::new(&m.executable);
        if arcade_link::childenv::detach_from_image(&mut command).spawn().is_err() {
            self.toast(format!("Couldn't open {}. Try opening it from your apps menu.", m.name), Tone::Danger);
        }
    }

    /// "Install": Arcade Tools when installed (the user confirms there), else the releases page.
    fn install_app(&self, ctx: &egui::Context, id: &str, tools: bool) {
        let (tx, loc, reg, id, c) = (self.tx.clone(), self.locations.clone(), self.registry.clone(), id.to_string(), ctx.clone());
        std::thread::spawn(move || {
            let via_tools = tools
                && reg.as_ref().and_then(|r| r.get(ids::TOOLS)).and_then(|m| m.action("tools.install").map(|a| (m.clone(), a.clone()))).is_some_and(
                    |(m, a)| {
                        let req = link::request(&a, vec![arcade_link::Content::plain(id.clone())], serde_json::json!({ "app": id }));
                        link::invoke(&loc, &m, &req, None, None).is_ok()
                    },
                );
            if !via_tools && open::that_detached(link::apps::releases_url(&id)).is_err() {
                let _ = tx.send(Bg::Toast("Couldn't open the browser. Install the app from Arcade Tools instead.".into(), Tone::Danger));
            }
            c.request_repaint();
        });
    }

    fn about(&mut self, ui: &mut egui::Ui, kit: &Kit, ctx: &egui::Context) {
        kit.page_head(ui, "About", "Version, updates, help and licences.");
        let tools = self.registry.as_ref().and_then(|r| r.get(ids::TOOLS)).is_some();
        let channel = match self.channel.as_deref() {
            Some("stable") => "Stable",
            Some("beta") => "Beta",
            Some("nightly") => "Nightly",
            Some(_) => "Other channel",
            None => "Not installed",
        };
        let brand = self.glyph("brand");
        let mut action = None;
        kit.list(ui, |ui| {
            kit.row_with_glyph(ui, brand, "search", &format!("Version {VERSION}"), TAGLINE, |ui| {
                kit.badge(ui, channel, Tone::Neutral);
            });
            if tools {
                kit.row(ui, "Updates are managed by Arcade Tools", "Tools checks for new versions and installs them.", |ui| {
                    if kit.button_state(ui, "Open Tools", ButtonKind::Default, true, false, true).clicked() {
                        action = Some("tools");
                    }
                });
            } else {
                kit.row(ui, "Updates", "Install Arcade Tools to get updates automatically.", |ui| {
                    if kit.button_state(ui, "Check for updates", ButtonKind::Default, true, false, true).clicked() {
                        action = Some("releases");
                    }
                });
            }
            kit.row(ui, "User guide", "How to search, filter and use Find with other apps.", |ui| {
                if kit.button_state(ui, "Open user guide", ButtonKind::Default, true, false, true).clicked() {
                    action = Some("guide");
                }
            });
            kit.row(ui, "Logs", &self.paths.log_dir().display().to_string(), |ui| {
                if kit.button_state(ui, "Open logs folder", ButtonKind::Default, true, false, true).clicked() {
                    action = Some("logs");
                }
            });
            kit.row(ui, "Diagnostics", "App, version, desktop and Link details for a bug report. No file names.", |ui| {
                if kit.button_state(ui, "Copy diagnostics", ButtonKind::Default, true, false, true).clicked() {
                    action = Some("diagnostics");
                }
            });
            kit.row(ui, "Licences", "MIT OR Apache-2.0. Third-party licences ship with the app.", |ui| {
                if kit.button_state(ui, "Open", ButtonKind::Default, true, false, true).clicked() {
                    action = Some("licences");
                }
            });
            kit.row(ui, "Restart Arcade Find", "Reloads the index and the shortcut.", |ui| {
                if kit.button_state(ui, "Restart", ButtonKind::Default, true, false, true).clicked() {
                    action = Some("restart");
                }
            });
        });
        match action {
            Some("tools") => self.open_app(ids::TOOLS),
            Some("releases") => self.open_url(link::apps::releases_url(link::ME)),
            Some("guide") => self.open_url(GUIDE_URL),
            Some("logs") => {
                let dir = self.paths.log_dir();
                self.open_path(&dir);
            }
            Some("diagnostics") => {
                let text = diagnostics(&self.locations, self.channel.as_deref(), self.status.clone().flatten());
                self.copy(ctx, text);
            }
            Some("licences") => {
                let local =
                    PathBuf::from(arcade_link::manifest::current_executable()).parent().map(|d| d.join("THIRD_PARTY_NOTICES.md")).filter(|p| p.is_file());
                match local {
                    Some(p) => self.open_path(&p),
                    None => self.open_url(LICENCES_URL),
                }
            }
            Some("restart") => self.command(ctx, Command::Restart, Some("Restarting Arcade Find…")),
            _ => {}
        }
    }
}

/// The overlay's keys, from the same file Arcade Tools' shortcut sheet
/// reads (`docs/user/shortcuts.json`): (title, alternative combos).
fn overlay_keys() -> Vec<(String, Vec<String>)> {
    let doc: Value = serde_json::from_str(include_str!("../../../docs/user/shortcuts.json")).unwrap_or(Value::Null);
    let mut out = Vec::new();
    for group in doc["groups"].as_array().into_iter().flatten().filter(|g| g["context"] != "global") {
        for s in group["shortcuts"].as_array().into_iter().flatten() {
            let keys = &s["keys"];
            let platform = if cfg!(target_os = "macos") { "macos" } else { "default" };
            let mut combos: Vec<String> = keys[platform].as_str().or(keys["default"].as_str()).map(str::to_string).into_iter().collect();
            combos.extend(keys["alternatives"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)));
            if let (Some(title), false) = (s["title"].as_str(), combos.is_empty()) {
                out.push((title.to_string(), combos));
            }
        }
    }
    out
}

/// "Copy diagnostics": Link's shared header, then Find's lines. No queries,
/// file names or settings payloads.
pub fn diagnostics(locations: &Locations, channel: Option<&str>, status: Option<Value>) -> String {
    let mut header = arcade_link::diagnostics::Header::new(link::ME, VERSION, channel.unwrap_or("development"));
    header.peers_from(locations);
    let rg = find_core::content::detect(&[]);
    header.engine("ripgrep", &rg.map(|r| r.version.to_string()).unwrap_or_else(|| "not installed".into()));
    let mut out = header.render();
    match status {
        Some(s) => {
            let e = &s["engine"];
            out.push_str(&format!(
                "Index: {} entries, phase {}, watch {}\n",
                e["entries"].as_u64().unwrap_or(0),
                e["phase"].as_str().unwrap_or("?"),
                e["watch"]["mode"].as_str().unwrap_or("?")
            ));
            out.push_str(&format!("Shortcut: {}\n", s["shortcut"]["mode"].as_str().unwrap_or("?")));
            out.push_str(&format!("Tray: {}\n", s["tray"].as_str().unwrap_or("?")));
            out.push_str(&format!("Link endpoint: {}\n", if s["link"]["listening"] == true { "listening" } else { "not listening" }));
        }
        None => out.push_str("Running: no\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_follow_the_standard_order() {
        let names: Vec<&str> = PAGES.iter().map(|p| p.1).collect();
        assert_eq!(names, ["General", "Shortcuts", "Locations", "Index", "Connected apps", "About"]);
        for (_, _, icon) in PAGES {
            assert!(arcade_link::design::icon(icon).is_some(), "{icon} is a kit icon");
        }
    }

    #[test]
    fn the_key_list_comes_from_the_shortcut_sheet() {
        let keys = overlay_keys();
        assert!(keys.iter().any(|(t, k)| t == "Select the next result" && k[0] == "Down"));
        assert!(!keys.iter().any(|(t, _)| t == "Show or hide Find"), "the global shortcut has its own row");
    }

    #[test]
    fn diagnostics_have_the_shared_header_and_no_paths() {
        let d = diagnostics(&Locations::discover(), Some("stable"), None);
        assert!(d.contains("arcade.find") || d.contains("Arcade Find"));
        assert!(d.contains(VERSION));
        assert!(d.contains("Running: no"));
    }
}
