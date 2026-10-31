//! Find's small native install/confirmation window. The model is pure;
//! install, dismissal and Tools handoff work runs on a worker. It wakes egui
//! when finished, with no polling and no installer work in update().
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use crate::installer::{self, DialogState, Options};
use arcade_link::design::egui::components::{ButtonKind, Kit, Toast, Tone};
use arcade_link::design::size;
use arcade_link::install::{self, strings, Detection, Environment, Runtime};
use arcade_link::receipt::ManagedBy;
use eframe::egui;
use find_core::paths::AppPaths;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Continue,
    Launch(PathBuf),
    Exit,
}

// Keep the dialog in a short-lived helper process, as Find does for Settings.
// winit permits one event loop per process; "run once" must still be able to
// create the resident overlay after this window closes. Requests travel over
// a private pipe, never through generated startup/environment files.
#[derive(Serialize, Deserialize)]
struct PromptEnvironment {
    home: PathBuf,
    data: PathBuf,
    config: PathBuf,
    applications: PathBuf,
    bin: PathBuf,
    local_app_data: PathBuf,
    registry: PathBuf,
    runtime: PathBuf,
    handoff: PathBuf,
    path: Option<String>,
    windows_environment_key: String,
}
impl From<Environment> for PromptEnvironment {
    fn from(e: Environment) -> Self {
        Self {
            home: e.home,
            data: e.data,
            config: e.config,
            applications: e.applications,
            bin: e.bin,
            local_app_data: e.local_app_data,
            registry: e.locations.registry,
            runtime: e.locations.runtime,
            handoff: e.locations.handoff,
            path: e.path.map(|s| s.to_string_lossy().into()),
            windows_environment_key: e.windows_environment_key,
        }
    }
}
impl From<PromptEnvironment> for Environment {
    fn from(e: PromptEnvironment) -> Self {
        Self {
            home: e.home,
            data: e.data,
            config: e.config,
            applications: e.applications,
            bin: e.bin,
            local_app_data: e.local_app_data,
            locations: arcade_link::Locations { registry: e.registry, runtime: e.runtime, handoff: e.handoff },
            path: e.path.map(Into::into),
            windows_environment_key: e.windows_environment_key,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct PromptRuntime {
    executable: PathBuf,
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
    dev_build: bool,
}
impl From<Runtime> for PromptRuntime {
    fn from(r: Runtime) -> Self {
        Self { executable: r.executable, appimage: r.appimage, appdir: r.appdir, dev_build: r.dev_build }
    }
}
impl From<PromptRuntime> for Runtime {
    fn from(r: PromptRuntime) -> Self {
        Self { executable: r.executable, appimage: r.appimage, appdir: r.appdir, dev_build: r.dev_build }
    }
}
#[derive(Serialize, Deserialize)]
enum Prompt {
    Install { environment: Box<PromptEnvironment>, runtime: PromptRuntime, paths: AppPaths, detection: Box<Detection>, state: DialogState, options: Options },
    Uninstall { environment: Box<PromptEnvironment>, paths: AppPaths, remove_data: bool },
}

fn helper(prompt: Prompt) -> Result<Outcome, String> {
    use std::io::Write;
    let mut child = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--install-dialog")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&prompt).map_err(|e| e.to_string())?;
    child.stdin.take().ok_or("dialog pipe is unavailable")?.write_all(&bytes).map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Couldn't open the install dialog. Try again.".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid dialog reply: {e}"))
}

pub fn serve_dialog() -> Result<(), String> {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    std::io::stdin().take(4 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let prompt: Prompt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let outcome = match prompt {
        Prompt::Install { environment, runtime, paths, detection, state, options } => {
            let environment: Environment = (*environment).into();
            let managed = detection.receipt.as_ref().is_some_and(|r| r.managed_by == ManagedBy::Tools) && installer::tools_installed(&environment);
            run(paths, Mode::Install { environment: Box::new(environment), runtime: runtime.into(), detection, state, options, managed })?
        }
        Prompt::Uninstall { environment, paths, remove_data } => run(paths, Mode::Uninstall { environment: Box::new((*environment).into()), remove_data })?,
    };
    let bytes = serde_json::to_vec(&outcome).map_err(|e| e.to_string())?;
    std::io::stdout().write_all(&bytes).map_err(|e| e.to_string())
}

#[derive(Clone)]
enum Mode {
    Install { environment: Box<Environment>, runtime: Runtime, detection: Box<Detection>, state: DialogState, options: Options, managed: bool },
    Uninstall { environment: Box<Environment>, remove_data: bool },
}

struct Completed {
    outcome: Outcome,
    hint: Option<String>,
}
type WorkerResult = Result<Completed, String>;

struct Dialog {
    paths: AppPaths,
    mode: Mode,
    result: Arc<Mutex<Option<Outcome>>>,
    tx: mpsc::Sender<WorkerResult>,
    rx: mpsc::Receiver<WorkerResult>,
    busy: bool,
    error: Option<String>,
    completed: Option<Completed>,
    /// Older pre-installer copies this install replaces (named in the prompt).
    legacy: Vec<PathBuf>,
    toast: Option<Toast>,
    theme: arcade_link::design::Theme,
    window_state: crate::window_state::WindowState,
}

fn format(s: &str, version: &str, from: &str) -> String {
    strings::format(s, crate::link::NAME, version, from, crate::VERSION)
}

/// Shared contract strings with the standard's current button wording.
pub fn text(state: &DialogState) -> (String, Option<String>, Vec<String>) {
    let all = strings::get();
    let newer = |installed: &str| {
        semver::Version::parse(installed).ok().zip(semver::Version::parse(crate::VERSION).ok()).is_some_and(|(installed, this)| installed > this)
    };
    let (dialog, version, from) = match state {
        DialogState::Install => (&all.not_installed, crate::VERSION, ""),
        DialogState::Update { from } => (&all.older, crate::VERSION, from.as_str()),
        // Say so when this file is the older one, not just "already installed".
        DialogState::OpenInstalled { version } if newer(version) => {
            (all.newer_installed.as_ref().unwrap_or(&all.same_or_newer), version.as_str(), crate::VERSION)
        }
        DialogState::OpenInstalled { version } => (&all.same_or_newer, version.as_str(), ""),
        DialogState::MoveToApplications => (&all.macos, crate::VERSION, ""),
    };
    let buttons = dialog.buttons.iter().map(|s| if s == "Just run it once" { "Just run it".into() } else { s.clone() }).collect();
    (format(&dialog.title, version, from), dialog.body.as_ref().map(|s| format(s, version, from)), buttons)
}

pub fn run_install(
    environment: Environment,
    runtime: Runtime,
    paths: AppPaths,
    detection: Detection,
    state: DialogState,
    options: Options,
) -> Result<Outcome, String> {
    helper(Prompt::Install { environment: Box::new(environment.into()), runtime: runtime.into(), paths, detection: Box::new(detection), state, options })
}

pub fn run_uninstall(environment: Environment, paths: AppPaths, remove_data: bool) -> Result<Outcome, String> {
    helper(Prompt::Uninstall { environment: Box::new(environment.into()), paths, remove_data })
}

fn run(paths: AppPaths, mode: Mode) -> Result<Outcome, String> {
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    #[cfg(target_os = "linux")]
    let legacy = match &mode {
        Mode::Install { environment, .. } => install::legacy_copies(environment, &installer::app_info()),
        _ => Vec::new(),
    };
    #[cfg(not(target_os = "linux"))]
    let legacy = Vec::new();
    let title = if matches!(mode, Mode::Uninstall { .. }) { "Uninstall — Arcade Find" } else { "Install — Arcade Find" };
    crate::theme::refresh_system_theme();
    let window_state = crate::window_state::WindowState::new(
        &paths,
        if matches!(mode, Mode::Uninstall { .. }) { "uninstall" } else { "install" },
        [size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT],
    );
    let options = eframe::NativeOptions {
        viewport: window_state.viewport(
            egui::ViewportBuilder::default()
                .with_title(title)
                .with_app_id(crate::os::APP_ID)
                .with_inner_size([size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT])
                .with_min_inner_size([size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT])
                .with_icon(egui::IconData { rgba: crate::ui::icons::app_icon_rgba(64), width: 64, height: 64 }),
        ),
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            arcade_link::design::egui::install_inter(&cc.egui_ctx);
            let appearance = find_core::Settings::load(&paths.settings_file()).0.theme;
            let theme = crate::theme::kit_theme(appearance);
            arcade_link::design::apply_egui(&cc.egui_ctx, if theme.is_dark() { "dark" } else { "light" }, crate::link::ME);
            if let Ok(scale) = std::env::var("ARCADE_FIND_UI_SCALE") {
                if let Ok(scale) = scale.parse::<f32>() {
                    if (1.0..=1.25).contains(&scale) {
                        cc.egui_ctx.set_zoom_factor(scale);
                    }
                }
            }
            let ctx = cc.egui_ctx.clone();
            crate::theme::watch_system_theme(move || ctx.request_repaint());
            let (tx, rx) = mpsc::channel();
            Ok(Box::new(Dialog { paths, mode, result, tx, rx, busy: false, error: None, completed: None, legacy, toast: None, theme, window_state }))
        }),
    )
    .map_err(|e| e.to_string())?;
    let outcome = output.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or(Outcome::Continue);
    Ok(outcome)
}

impl Dialog {
    fn finish(&self, ctx: &egui::Context, outcome: Outcome) {
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn start(&mut self, ctx: &egui::Context, action: usize) {
        self.busy = true;
        self.error = None;
        let mode = self.mode.clone();
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| match mode {
                Mode::Install { environment, runtime, detection, state, options, managed } => {
                    if action == 2 {
                        installer::dismiss(&paths, &detection.running_from).map_err(|e| e.to_string())?;
                        return Ok(Completed { outcome: Outcome::Continue, hint: None });
                    }
                    if matches!(state, DialogState::OpenInstalled { .. }) {
                        let r = detection.receipt.ok_or("No installed copy was found")?;
                        return Ok(Completed { outcome: Outcome::Launch(installer::launch_path(&r)), hint: None });
                    }
                    if managed {
                        installer::handoff_to_tools(&environment).map_err(|e| e.to_string())?;
                        return Ok(Completed { outcome: Outcome::Exit, hint: None });
                    }
                    let r = installer::install_copy(&environment, &runtime, &paths, &options).map_err(|e| e.to_string())?;
                    // The installed copy replaces an older one still running.
                    installer::stop_running_copy(&paths);
                    let status =
                        install::integration_status(&environment, crate::link::ME, &std::env::var("SHELL").unwrap_or_default()).map_err(|e| e.to_string())?;
                    let hint = status.path_hint.map(|command| strings::format(&strings::get().path_hint, crate::link::NAME, crate::VERSION, "", &command));
                    Ok(Completed { outcome: Outcome::Launch(installer::launch_path(&r)), hint })
                }
                Mode::Uninstall { environment, remove_data } => {
                    installer::uninstall(&environment, &paths, true, remove_data).map_err(|e| e.to_string())?;
                    Ok(Completed { outcome: Outcome::Exit, hint: None })
                }
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for Dialog {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.window_state.observe(ctx);
        if let Ok(result) = self.rx.try_recv() {
            self.busy = false;
            match result {
                Ok(done) if done.hint.is_none() => self.finish(ctx, done.outcome),
                Ok(done) => {
                    *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(done.outcome.clone());
                    self.completed = Some(done);
                }
                Err(error) => self.error = Some(error),
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            if self.busy {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.completed.is_none() {
                self.result.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(Outcome::Exit);
            }
        }
        if !self.busy && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.finish(ctx, Outcome::Exit);
        }
        crate::theme::follow_native_theme(ctx);
        let appearance = std::fs::read(self.paths.settings_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<find_core::Settings>(&bytes).ok())
            .map(|s| s.theme)
            .unwrap_or_default();
        let theme = crate::theme::kit_theme(appearance);
        if theme != self.theme {
            arcade_link::design::apply_egui(ctx, if theme.is_dark() { "dark" } else { "light" }, crate::link::ME);
            self.theme = theme;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let kit = Kit::for_theme(theme, crate::link::ME);
        kit.page(ctx, |ui| {
            kit.glyph_tile(ui, None, "search");
            ui.add_space(16.0);
            if let Some(done) = &self.completed {
                kit.dialog_head(ui, "Arcade Find is installed", done.hint.as_deref().unwrap_or_default());
                kit.dialog_actions(ui, |ui| {
                    if kit.button(ui, "Open Arcade Find", ButtonKind::Primary).clicked() {
                        self.finish(ctx, done.outcome.clone());
                    }
                });
                return;
            }
            let mut clicked = None;
            ui.add_enabled_ui(!self.busy, |ui| match &mut self.mode {
                Mode::Install { state, options, environment, managed, .. } => {
                    let (title, body, buttons) = text(state);
                    kit.dialog_head(ui, &title, body.as_deref().unwrap_or_default());
                    if *managed {
                        kit.notice(ui, &format(&strings::get().managed_by_tools, crate::VERSION, ""), Tone::Neutral);
                    }
                    if let (Some(note), Some(old), DialogState::Install | DialogState::Update { .. }) =
                        (&strings::get().replaces_legacy, self.legacy.first(), &state)
                    {
                        kit.notice(ui, &format(note, crate::VERSION, &old.display().to_string()), Tone::Neutral);
                    }
                    ui.add_space(10.0);
                    if !*managed && matches!(state, DialogState::Install | DialogState::Update { .. } | DialogState::MoveToApplications) {
                        kit.checkbox(ui, &mut options.start_at_login, "Start at login");
                        #[cfg(target_os = "linux")]
                        kit.checkbox(ui, &mut options.remove_download, "Remove the downloaded file");
                        #[cfg(target_os = "macos")]
                        {
                            kit.checkbox(ui, &mut options.install_cli, "Add the arcade-find terminal command");
                            if !options.install_cli {
                                let executable = environment.home.join("Applications/Arcade Find.app/Contents/MacOS/arcade-find");
                                kit.notice(ui, &install::platform::macos::cli_symlink_offer(environment, installer::CLI_NAME, &executable), Tone::Neutral);
                            }
                        }
                    }
                    let _ = environment;
                    kit.dialog_actions(ui, |ui| {
                        for (index, label) in buttons.iter().enumerate().rev() {
                            let kind = match index {
                                0 => ButtonKind::Primary,
                                1 => ButtonKind::Default,
                                _ => ButtonKind::Ghost,
                            };
                            let label = if index == 0 && *managed { "Open Tools" } else { label };
                            if kit.button_state(ui, label, kind, true, self.busy && index == 0, false).clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
                }
                Mode::Uninstall { remove_data, .. } => {
                    kit.dialog_head(ui, "Uninstall Arcade Find?", "Remove this installation, launcher, command and start-at-login entry.");
                    ui.add_space(10.0);
                    kit.checkbox(ui, remove_data, "Remove settings and data (index, history and pins)");
                    kit.dialog_actions(ui, |ui| {
                        if kit.button(ui, "Cancel", ButtonKind::Default).clicked() {
                            clicked = Some(1);
                        }
                        if kit.button_state(ui, "Uninstall", ButtonKind::Destructive, true, self.busy, false).clicked() {
                            clicked = Some(0);
                        }
                    });
                }
            });
            match clicked {
                Some(1) => self.finish(ctx, Outcome::Continue),
                Some(index) => self.start(ctx, index),
                None => {}
            }
            if self.busy {
                kit.notice(ui, "Working…", Tone::Neutral);
            }
            if let Some(error) = &self.error {
                let message = if matches!(self.mode, Mode::Uninstall { .. }) {
                    "Couldn't uninstall Arcade Find. Check the app folder's permissions, then try again."
                } else {
                    "Couldn't install Arcade Find. Check the app folder's permissions, then try again."
                };
                kit.notice_action(ui, message, Tone::Danger, |ui| {
                    if kit.button(ui, "Copy details", ButtonKind::Ghost).clicked() {
                        ctx.copy_text(error.clone());
                        self.toast = Some(Toast::copied());
                    }
                });
            }
        });
        if let Some(toast) = &self.toast {
            if !toast.show(ctx, &kit) {
                self.toast = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_shared_copy_and_standard_button_wording() {
        let (title, body, buttons) = text(&DialogState::Install);
        assert_eq!(title, "Install Arcade Find?");
        assert_eq!(body.as_deref(), Some("This adds Arcade Find to the app launcher and your apps folder."));
        assert_eq!(buttons, ["Install", "Just run it", "Don't ask again for this file"]);
        let update = text(&DialogState::Update { from: "0.2.1".into() }).0;
        assert!(update.contains(&format!("0.2.1 → {}", env!("CARGO_PKG_VERSION"))), "{update}");
        let (title, body, _) = text(&DialogState::OpenInstalled { version: "99.0.0".into() });
        assert_eq!(title, "A newer Arcade Find is already installed.");
        assert!(body.unwrap().contains(&format!("99.0.0; this file is the older {}", env!("CARGO_PKG_VERSION"))));
        assert_eq!(
            text(&DialogState::OpenInstalled { version: env!("CARGO_PKG_VERSION").into() }).0,
            format!("Arcade Find {} is already installed.", env!("CARGO_PKG_VERSION"))
        );
    }
}
