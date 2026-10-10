//! The Settings window: a separate egui process (`--settings-window`) that
//! runs only while open, so the resident overlay never carries a GPU
//! context. Changes are saved at once and the running instance reloads.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use arcade_link::client::{app_state, AppState};
use arcade_link::manifest::{app_name, app_pitch, ids, releases_url};
use arcade_link::{Locations, Registry};
use eframe::egui;
use serde_json::Value;

use find_core::paths::{expand_tilde, tilde, AppPaths};
use find_core::settings::Theme;
use find_core::Settings;

use crate::instance::{self, Command};
use crate::{autostart, hotkey, link, theme, VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Locations,
    Shortcut,
    Index,
    Connected,
    About,
}

const PAGES: [(Page, &str); 6] = [
    (Page::General, "General"),
    (Page::Locations, "Locations"),
    (Page::Shortcut, "Shortcut"),
    (Page::Index, "Index"),
    (Page::Connected, "Connected apps"),
    (Page::About, "About"),
];

/// Results of background work.
enum Bg {
    Status(Option<Value>),
    Registry(Registry, Vec<(String, AppState)>),
    Ripgrep(Option<find_core::content::Ripgrep>),
    Saved(Result<(), String>),
    Message(String),
}

struct SettingsApp {
    paths: AppPaths,
    locations: Locations,
    settings: Settings,
    page: Page,
    status: Option<Value>,
    status_at: Option<Instant>,
    registry: Option<Registry>,
    states: Vec<(String, AppState)>,
    ripgrep: Option<Option<find_core::content::Ripgrep>>,
    tx: Sender<Bg>,
    rx: Receiver<Bg>,
    new_root: String,
    new_exclude: String,
    exclude_names: String,
    recording: bool,
    message: Option<(String, Instant)>,
    drives: Vec<PathBuf>,
}

pub fn run(paths: AppPaths) -> ExitCode {
    let (settings, _) = Settings::load(&paths.settings_file());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Arcade Find Settings")
            .with_app_id("arcade-find-settings")
            .with_inner_size([780.0, 560.0])
            .with_min_inner_size([620.0, 440.0])
            .with_icon(egui::IconData { rgba: crate::ui::icons::app_icon_rgba(64), width: 64, height: 64 }),
        ..Default::default()
    };
    let r = eframe::run_native(
        "Arcade Find Settings",
        options,
        Box::new(move |cc| {
            theme::refresh_system_theme();
            apply_visuals(&cc.egui_ctx, settings.theme);
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

fn apply_visuals(ctx: &egui::Context, t: Theme) {
    let dark = match t {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => theme::system_dark(),
    };
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let accent = theme::ACCENT;
    let a = egui::Color32::from_rgb((accent >> 16) as u8, (accent >> 8) as u8, accent as u8);
    v.selection.bg_fill = a.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.hyperlink_color = a;
    ctx.set_visuals(v);
}

impl SettingsApp {
    fn new(paths: AppPaths, settings: Settings, ctx: egui::Context) -> SettingsApp {
        let (tx, rx) = channel();
        let exclude_names = settings.exclude_names.join(", ");
        let app = SettingsApp {
            locations: Locations::discover(),
            paths,
            settings,
            page: Page::General,
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
            recording: false,
            message: None,
            drives: find_core::mounts::MountTable::read().suggested_drives(),
        };
        app.refresh_status(&ctx);
        app.refresh_registry(&ctx);
        let tx = app.tx.clone();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Bg::Ripgrep(find_core::content::detect(&[])));
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
        apply_visuals(ctx, s.theme);
        std::thread::spawn(move || {
            let r = s.save(&p.settings_file()).map_err(|e| format!("Couldn't save settings: {e}"));
            if r.is_ok() {
                let _ = instance::send(&p, &Command::Reload, Duration::from_secs(3));
            }
            let _ = tx.send(Bg::Saved(r));
            c.request_repaint();
        });
    }

    fn command(&self, ctx: &egui::Context, cmd: Command, done: &'static str) {
        let (tx, p, c) = (self.tx.clone(), self.paths.clone(), ctx.clone());
        std::thread::spawn(move || {
            let msg = match instance::send(&p, &cmd, Duration::from_secs(3)) {
                Ok(_) => done.to_string(),
                Err(_) => "Arcade Find isn't running.".to_string(),
            };
            let _ = tx.send(Bg::Message(msg));
            c.request_repaint();
        });
    }

    fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Bg::Status(s) => {
                    self.status = s;
                    self.status_at = Some(Instant::now());
                }
                Bg::Registry(r, st) => {
                    self.registry = Some(r);
                    self.states = st;
                }
                Bg::Ripgrep(r) => self.ripgrep = Some(r),
                Bg::Saved(Err(e)) | Bg::Message(e) => self.message = Some((e, Instant::now())),
                Bg::Saved(Ok(())) => {}
            }
        }
    }
}

/// The apps the Connected apps page lists: the family, plus any other
/// installed Arcade app (a new app shows up without a Find update).
fn app_ids(reg: &Registry) -> Vec<String> {
    let mut v: Vec<String> = ids::APPS.iter().map(|s| s.to_string()).collect();
    for m in reg.apps() {
        if m.id != link::ME && m.id != ids::TOOLS && !v.contains(&m.id) && m.id.starts_with("arcade.") {
            v.push(m.id.clone());
        }
    }
    v
}

fn heading(ui: &mut egui::Ui, t: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(t).size(18.0).strong());
    ui.add_space(8.0);
}

fn note(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t).weak());
}

fn path_valid(p: &Path) -> bool {
    p.is_absolute() && p.is_dir()
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        let ctx = ui.ctx().clone();
        // While indexing, keep the Index page current (only while it's open).
        if self.page == Page::Index && self.status_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            self.status_at = Some(Instant::now());
            self.refresh_status(&ctx);
        }
        if self.page == Page::Index {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        egui::Panel::left("nav").exact_size(170.0).show(ui, |ui| {
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Arcade Find").size(16.0).strong());
            ui.add_space(10.0);
            for (p, name) in PAGES {
                if ui.selectable_label(self.page == p, name).clicked() {
                    self.page = p;
                    if p == Page::Index || p == Page::Shortcut {
                        self.refresh_status(&ctx);
                    }
                    if p == Page::Connected {
                        self.refresh_registry(&ctx);
                    }
                }
            }
        });
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.page {
                Page::General => self.general(ui, &ctx),
                Page::Locations => self.locations_page(ui, &ctx),
                Page::Shortcut => self.shortcut(ui, &ctx),
                Page::Index => self.index(ui, &ctx),
                Page::Connected => self.connected(ui, &ctx),
                Page::About => self.about(ui),
            });
            if let Some((m, t)) = &self.message {
                if t.elapsed() < Duration::from_secs(5) {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(m).color(ui.visuals().warn_fg_color));
                    ctx.request_repaint_after(Duration::from_secs(5));
                }
            }
        });
    }
}

impl SettingsApp {
    fn general(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        heading(ui, "General");
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Theme");
            for (t, name) in [(Theme::System, "System"), (Theme::Light, "Light"), (Theme::Dark, "Dark")] {
                changed |= ui.radio_value(&mut self.settings.theme, t, name).changed();
            }
        });
        ui.add_space(6.0);
        let allowed = autostart::allowed(&self.paths);
        // The login entry is the truth (Arcade Tools can switch it too).
        let mut login = if allowed { autostart::enabled() } else { self.settings.start_at_login.unwrap_or(false) };
        let r = ui.add_enabled(allowed, egui::Checkbox::new(&mut login, "Start at login"));
        if r.changed() {
            match autostart::set(login) {
                Ok(()) => {
                    self.settings.start_at_login = Some(login);
                    changed = true;
                }
                Err(e) => self.message = Some((e, Instant::now())),
            }
        }
        if !allowed {
            note(ui, "Available for installed copies (not development builds or test profiles).");
        } else if cfg!(target_os = "linux") && hotkey::hyprland::active() {
            note(ui, "Hyprland doesn't run desktop autostart entries by itself. Add `exec-once = arcade-find --background` to your Hyprland config, or use a session that runs XDG autostart.");
        }
        changed |= ui.checkbox(&mut self.settings.show_recent_on_open, "Show recent and pinned items when the search opens").changed();
        changed |= ui.checkbox(&mut self.settings.show_hidden, "Show hidden files (toggle any time with Ctrl+H)").changed();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Rows shown");
            changed |= ui.add(egui::DragValue::new(&mut self.settings.rows.visible).range(3..=12)).changed();
            ui.label("growing up to");
            changed |= ui.add(egui::DragValue::new(&mut self.settings.rows.stretch).range(self.settings.rows.visible..=16)).changed();
            ui.label("before scrolling");
        });
        ui.add_space(6.0);
        changed |= ui.checkbox(&mut self.settings.content_search, "Search inside files with / (uses ripgrep)").changed();
        match &self.ripgrep {
            None => note(ui, "Looking for ripgrep…"),
            Some(Some(rg)) => note(ui, &format!("ripgrep {} found at {}", rg.version, rg.path.display())),
            Some(None) => note(ui, "ripgrep isn't installed. Install it from your package manager (package \"ripgrep\") to search file contents; Find never downloads it."),
        }
        if changed {
            self.save(ctx);
        }
    }

    fn locations_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        heading(ui, "Locations");
        let mut changed = false;
        ui.label(egui::RichText::new("Folders to index").strong());
        let mut remove = None;
        for (i, r) in self.settings.roots.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.monospace(r);
                if self.settings.roots.len() > 1 && ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.settings.roots.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_root).hint_text("/mnt/Data or ~/Projects").desired_width(320.0));
            let p = expand_tilde(self.new_root.trim());
            if ui.add_enabled(path_valid(&p), egui::Button::new("Add folder")).clicked() {
                self.settings.roots.push(tilde(&p));
                self.new_root.clear();
                changed = true;
            }
        });
        let suggestions: Vec<PathBuf> =
            self.drives.iter().filter(|d| !self.settings.roots.iter().any(|r| expand_tilde(r) == **d)).cloned().collect();
        if !suggestions.is_empty() {
            note(ui, "Other drives (not indexed unless you add them):");
            for d in suggestions {
                ui.horizontal(|ui| {
                    ui.monospace(d.display().to_string());
                    if ui.small_button("Add").clicked() {
                        self.settings.roots.push(d.display().to_string());
                        changed = true;
                    }
                });
            }
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Skip these names everywhere").strong());
        let r = ui.add(egui::TextEdit::multiline(&mut self.exclude_names).desired_rows(2).desired_width(f32::INFINITY));
        if r.lost_focus() {
            self.settings.exclude_names =
                self.exclude_names.split([',', '\n']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            changed = true;
        }
        note(ui, "Comma-separated; * and ? work (e.g. node_modules, *.tmp).");
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Skip these folders").strong());
        let mut remove = None;
        for (i, r) in self.settings.exclude_paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.monospace(r);
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.settings.exclude_paths.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_exclude).hint_text("~/Downloads/old").desired_width(320.0));
            let p = expand_tilde(self.new_exclude.trim());
            if ui.add_enabled(p.is_absolute(), egui::Button::new("Add")).clicked() {
                self.settings.exclude_paths.push(tilde(&p));
                self.new_exclude.clear();
                changed = true;
            }
        });
        ui.add_space(6.0);
        changed |= ui.checkbox(&mut self.settings.skip_network_mounts, "Skip network drives (NFS, SMB, SSHFS…)").changed();
        note(ui, "Symbolic links are never followed, so nothing is indexed twice.");
        if changed {
            self.save(ctx);
        }
    }

    fn shortcut(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        heading(ui, "Shortcut");
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Open Arcade Find");
            if self.recording {
                ui.label(egui::RichText::new("Press the new shortcut…").italics());
                let pressed = ui.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Key { key, pressed: true, modifiers, .. } if *key != egui::Key::Escape => Some((*key, *modifiers)),
                        _ => None,
                    })
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) || ui.button("Cancel").clicked() {
                    self.recording = false;
                } else if let Some((key, m)) = pressed {
                    let mut parts = Vec::new();
                    if m.ctrl {
                        parts.push("Ctrl".to_string());
                    }
                    if m.alt {
                        parts.push("Alt".to_string());
                    }
                    if m.shift {
                        parts.push("Shift".to_string());
                    }
                    if m.mac_cmd || (m.command && !m.ctrl) {
                        parts.push("Super".to_string());
                    }
                    parts.push(key.name().to_string());
                    let accel = parts.join("+");
                    if hotkey::parse(&accel).is_ok() {
                        self.settings.shortcut = accel;
                        self.recording = false;
                        changed = true;
                    }
                }
            } else {
                ui.monospace(&self.settings.shortcut);
                if ui.button("Change…").clicked() {
                    self.recording = true;
                }
                if self.settings.shortcut != find_core::settings::DEFAULT_SHORTCUT && ui.button("Reset").clicked() {
                    self.settings.shortcut = find_core::settings::DEFAULT_SHORTCUT.into();
                    changed = true;
                }
            }
        });
        if let Some(what) = hotkey::known_conflict(&self.settings.shortcut) {
            ui.label(egui::RichText::new(format!("Usually used by {what}.")).color(ui.visuals().warn_fg_color));
        }
        if let Some(owner) = self.registry.as_ref().and_then(|r| r.shortcut_owner(link::ME, &self.settings.shortcut)) {
            ui.label(egui::RichText::new(format!("Used by {owner}.")).color(ui.visuals().warn_fg_color));
        }
        ui.add_space(8.0);
        if cfg!(target_os = "linux") {
            changed |=
                ui.checkbox(&mut self.settings.hyprland_runtime_bind, "On Hyprland, add the shortcut as a runtime key binding").changed();
            note(ui, "Runtime only: Find never edits your Hyprland config. Without it, add this line yourself:");
            if let Some(line) = hotkey::hyprland::config_line(&self.settings.shortcut, &hotkey::toggle_command()) {
                ui.horizontal(|ui| {
                    ui.monospace(&line);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(line.clone());
                    }
                });
            }
        }
        ui.add_space(8.0);
        match self.status.as_ref().map(|s| &s["shortcut"]) {
            Some(st) => match st["mode"].as_str() {
                Some("native") => note(ui, "Working: registered with the system."),
                Some("hyprland") => note(ui, "Working: bound in the running Hyprland session."),
                Some("manual") | Some("failed") => {
                    let why = st["reason"].as_str().or(st["error"].as_str()).unwrap_or_default();
                    ui.label(egui::RichText::new(why).color(ui.visuals().warn_fg_color));
                    if let Some(cmd) = st["command"].as_str() {
                        note(ui, "Bind this command to a shortcut in your desktop's keyboard settings:");
                        ui.horizontal(|ui| {
                            ui.monospace(cmd);
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(cmd.to_string());
                            }
                        });
                    }
                }
                _ => {}
            },
            None => note(ui, "Arcade Find isn't running, so the shortcut isn't active."),
        }
        note(ui, "Inside the search: ⏎ Quick Look · ⇧⏎ open · Ctrl+⏎ show in folder · Tab actions · Ctrl+H hidden files.");
        if changed {
            self.save(ctx);
            self.status_at = None;
            self.refresh_status(ctx);
        }
    }

    fn index(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        heading(ui, "Index");
        let Some(st) = self.status.clone() else {
            note(ui, "Arcade Find isn't running. Start it to build and update the index.");
            return;
        };
        let e = &st["engine"];
        let phase = e["phase"].as_str().unwrap_or("?");
        let entries = e["entries"].as_u64().unwrap_or(0);
        let label = match phase {
            "crawling" => format!("Indexing… {} items so far", find_core::fmt::count(e["progress"]["entries"].as_u64().unwrap_or(entries))),
            "loading" => "Loading the saved index…".to_string(),
            "reconciling" => format!("{} items · checking for changes", find_core::fmt::count(entries)),
            _ => format!("{} items indexed", find_core::fmt::count(entries)),
        };
        ui.label(egui::RichText::new(label).strong());
        if let Some(cur) = e["progress"]["current"].as_str().filter(|c| !c.is_empty() && phase == "crawling") {
            note(ui, cur);
        }
        ui.add_space(6.0);
        ui.label(format!("Memory used by the index: {}", find_core::fmt::size(e["indexBytes"].as_u64().unwrap_or(0))));
        if let Some(f) = e["indexFile"].as_str() {
            let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
            ui.label(format!("Saved index: {} ({})", f, find_core::fmt::size(size)));
        }
        if e["loadedFromDisk"] == true {
            ui.label(format!("Loaded in {} ms at startup", e["loadMs"].as_u64().unwrap_or(0)));
        }
        if let Some(t) = e["lastFullScan"].as_i64().filter(|t| *t > 0) {
            ui.label(format!(
                "Last full scan: {} ({} s)",
                find_core::fmt::datetime(t, find_core::local_offset_secs()),
                e["lastScanMs"].as_u64().unwrap_or(0) / 1000
            ));
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Live updates").strong());
        let w = &e["watch"];
        ui.label(w["detail"].as_str().unwrap_or(""));
        if let Some(cmd) = w["fixCommand"].as_str() {
            note(ui, "To watch every folder, raise the limit (Find never changes system settings itself):");
            ui.horizontal(|ui| {
                ui.monospace(cmd);
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(cmd.to_string());
                }
            });
        }
        if let Some(err) = e["lastError"].as_str() {
            ui.label(egui::RichText::new(err).color(ui.visuals().warn_fg_color));
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Rescan now").clicked() {
                self.command(ctx, Command::Rescan, "Rescanning…");
            }
            ui.label("Full rescan every");
            let mut h = self.settings.rescan_hours;
            if ui.add(egui::DragValue::new(&mut h).range(1..=168).suffix(" h")).changed() {
                self.settings.rescan_hours = h;
                self.save(ctx);
            }
        });
    }

    fn connected(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        heading(ui, "Connected apps");
        let mut changed = ui.checkbox(&mut self.settings.link.enabled, "Connect with other Arcade apps").changed();
        note(ui, "Find shows other Arcade apps' actions for your results (Tab) and lets them search with Find.");
        ui.add_space(8.0);
        let tools_installed = self.registry.as_ref().is_some_and(|r| r.get(ids::TOOLS).is_some());
        for (id, state) in self.states.clone() {
            let name = self.registry.as_ref().and_then(|r| r.get(&id).map(|m| m.name.clone())).unwrap_or_else(|| app_name(&id).to_string());
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&name).strong());
                    let st = match &state {
                        AppState::Running { version } => format!("Running · v{version}"),
                        AppState::Installed { .. } => "Installed".to_string(),
                        AppState::NotInstalled => "Not installed".to_string(),
                    };
                    ui.label(egui::RichText::new(st).weak());
                });
                match state {
                    AppState::NotInstalled => {
                        let pitch = app_pitch(&id);
                        if !pitch.is_empty() {
                            note(ui, pitch);
                        }
                        if ids::APPS.contains(&id.as_str()) && ui.button("Get").clicked() {
                            self.get_app(ctx, &id, tools_installed);
                        }
                    }
                    _ => {
                        let mut on = !self.settings.link.disabled_peers.contains(&id);
                        if ui.add_enabled(self.settings.link.enabled, egui::Checkbox::new(&mut on, "Use with Arcade Find")).changed() {
                            self.settings.link.disabled_peers.retain(|p| p != &id);
                            if !on {
                                self.settings.link.disabled_peers.push(id.clone());
                            }
                            changed = true;
                        }
                    }
                }
            });
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Diagnostics").show(ui, |ui| {
            for (k, v) in arcade_link::presence::diagnostics(None, &self.locations) {
                if k == "Endpoint" || k == "Last error" {
                    continue;
                }
                ui.label(format!("{k}: {v}"));
            }
            match self.status.as_ref().map(|s| &s["link"]) {
                Some(l) => {
                    ui.label(format!("Endpoint: {}", if l["listening"] == true { "listening" } else { "not listening" }));
                    ui.label(format!("Last error: {}", l["lastError"].as_str().unwrap_or("none")));
                }
                None => {
                    ui.label("Endpoint: Arcade Find isn't running");
                }
            }
            if ui.small_button("Refresh").clicked() {
                self.refresh_registry(ctx);
                self.refresh_status(ctx);
            }
        });
        if changed {
            self.save(ctx);
        }
    }

    /// "Get": Arcade Tools when installed (the user confirms there), else the releases page.
    fn get_app(&self, ctx: &egui::Context, id: &str, tools: bool) {
        let (tx, loc, reg, id, c) = (self.tx.clone(), self.locations.clone(), self.registry.clone(), id.to_string(), ctx.clone());
        std::thread::spawn(move || {
            let via_tools = tools
                && reg
                    .as_ref()
                    .and_then(|r| r.get(ids::TOOLS))
                    .and_then(|m| m.action("tools.install").map(|a| (m.clone(), a.clone())))
                    .is_some_and(|(m, a)| {
                        let req = link::request(&a, vec![arcade_link::Content::plain(id.clone())], serde_json::json!({ "app": id }));
                        link::invoke(&loc, &m, &req, None, None).is_ok()
                    });
            if !via_tools {
                if let Err(e) = open::that_detached(releases_url(&id)) {
                    let _ = tx.send(Bg::Message(format!("Couldn't open the browser: {e}")));
                }
            }
            c.request_repaint();
        });
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        heading(ui, "About");
        ui.label(egui::RichText::new(format!("Arcade Find {VERSION}")).strong());
        note(ui, "Find any file or folder instantly. Local only: no network access, and file contents are read only when you search inside files.");
        ui.add_space(8.0);
        for (k, v) in [
            ("Settings", self.paths.settings_file()),
            ("Data", self.paths.data.clone()),
            ("Index", self.paths.index_file()),
            ("Runtime", self.paths.runtime.clone()),
        ] {
            ui.horizontal(|ui| {
                ui.label(format!("{k}:"));
                ui.monospace(v.display().to_string());
            });
        }
        ui.add_space(8.0);
        note(ui, "License: MIT OR Apache-2.0. Third-party licenses are listed in THIRD_PARTY_NOTICES.md, shipped with the app. Glyphs and tokens from Arcade Link v0.1.0; Hyprland binding code adapted from Arcade Lens.");
        if ui.link("github.com/qa-p1/Arcade-Find").clicked() {
            let _ = open::that_detached("https://github.com/qa-p1/Arcade-Find");
        }
    }
}
