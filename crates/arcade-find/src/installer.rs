//! Shared integration plus app-owned policy. Every function takes explicit
//! environment/runtime/profile paths; tests never need a real user profile.
//! Call mutations on a worker (CLI commands have no UI event loop).
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use arcade_link::install::{self, AppInfo, Detection, Environment, Runtime, State};
#[cfg(not(windows))]
use arcade_link::receipt::ManagedBy;
use arcade_link::receipt::{InstallMethod, Receipt};
use find_core::paths::AppPaths;
use find_core::Settings;
use serde::{Deserialize, Serialize};

pub const CLI_NAME: &str = "arcade-find";
pub const DESKTOP_ID: &str = "arcade-find";

pub fn app_info() -> AppInfo {
    AppInfo {
        id: crate::link::ME.into(),
        name: crate::link::NAME.into(),
        filename: "Arcade-Find.AppImage".into(),
        desktop_id: DESKTOP_ID.into(),
        cli_name: CLI_NAME.into(),
        background_args: vec!["--background".into()],
        legacy_desktop_ids: vec![DESKTOP_ID.into()],
        autostart_id: DESKTOP_ID.into(),
    }
}

pub fn method() -> InstallMethod {
    let detection = Environment::discover().and_then(|e| Runtime::discover().and_then(|r| install::detect(&e, crate::link::ME, &r)));
    match detection {
        Ok(d) if d.state == State::DevBuild => InstallMethod::Dev,
        Ok(d) => d
            .receipt
            .as_ref()
            .filter(|_| d.state == State::InstalledHere)
            .map_or(if d.is_app_image { InstallMethod::Appimage } else { InstallMethod::Manual }, |r| r.method),
        Err(_) => InstallMethod::Manual,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DialogState {
    Install,
    Update { from: String },
    OpenInstalled { version: String },
    MoveToApplications,
}

/// Pure state selection, shared by the automatic prompt and explicit CLI UI.
pub fn dialog_state(d: &Detection, version: &str) -> Option<DialogState> {
    match &d.state {
        State::DevBuild | State::InstalledHere => None,
        State::NotInstalled => Some(DialogState::Install),
        State::InstalledElsewhere { version: installed } => {
            if semver::Version::parse(installed).ok().zip(semver::Version::parse(version).ok()).is_some_and(|(old, new)| old < new) {
                Some(DialogState::Update { from: installed.clone() })
            } else {
                Some(DialogState::OpenInstalled { version: installed.clone() })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub channel: String,
    pub start_at_login: bool,
    pub remove_download: bool,
    /// On macOS this is an explicit offer in the dialog. Silent installs
    /// create the CLI only when its directory is already on PATH.
    pub install_cli: bool,
}

impl Options {
    pub fn defaults(environment: &Environment, paths: &AppPaths) -> Self {
        let settings = fs::read(paths.settings_file()).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default();
        let receipt = environment.receipts().read(crate::link::ME).ok().flatten();
        let start_at_login = receipt
            .as_ref()
            .map_or(settings.start_at_login.unwrap_or(true), |r| r.integration.autostart.as_ref().is_some_and(|p| p.exists()));
        let install_cli = environment.path.as_ref().is_some_and(|p| std::env::split_paths(p).any(|p| p == environment.bin));
        Self { channel: "stable".into(), start_at_login, remove_download: true, install_cli }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileIdentity {
    path: PathBuf,
    size: u64,
    modified: u128,
}

fn identity(path: &Path) -> io::Result<FileIdentity> {
    let meta = fs::metadata(path)?;
    Ok(FileIdentity {
        path: path.into(),
        size: meta.len(),
        modified: meta.modified()?.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
    })
}

pub fn dismissed(paths: &AppPaths, path: &Path) -> bool {
    let Ok(id) = identity(path) else { return false };
    fs::read(paths.config.join("install-dismissed.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<FileIdentity>>(&b).ok())
        .is_some_and(|v| v.contains(&id))
}

pub fn dismiss(paths: &AppPaths, path: &Path) -> io::Result<()> {
    let id = identity(path)?;
    let file = paths.config.join("install-dismissed.json");
    let mut ids: Vec<FileIdentity> = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    ids.retain(|i| i.path != id.path);
    // A bounded file, even if somebody repeatedly runs copies from Downloads.
    if ids.len() >= 128 {
        ids.remove(0);
    }
    ids.push(id);
    arcade_link::paths::write_atomic(&file, &serde_json::to_vec(&ids).map_err(io::Error::other)?, true)
}

/// Fast normal-start gate: no environment discovery, receipt read or dialog
/// initialization for dev binaries/commands or ordinary installed executables.
pub fn automatic_prompt(paths: &AppPaths, args: &[String]) -> io::Result<Option<(Environment, Runtime, Detection, DialogState)>> {
    if std::env::var("ARCADE_NO_INSTALL_PROMPT").as_deref() == Ok("1")
        || args.iter().any(|a| {
            [
                "--background",
                "--arcade-invoke",
                "--arcade-manifest",
                "--version",
                "-V",
                "--install",
                "--uninstall",
                "--repair",
                "--integration-status",
                "--settings-window",
                "--toggle",
                "--hide",
                "--status",
                "--search",
                "--quit",
                "--restart",
                "--rescan",
                "--reload",
                "--help",
                "-h",
            ]
            .contains(&a.split('=').next().unwrap_or(a))
        })
    {
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() || std::env::var_os("APPDIR").is_none() {
        return Ok(None);
    }
    #[cfg(windows)]
    {
        let _ = paths;
        Ok(None)
    }
    #[cfg(not(windows))]
    {
        let runtime = Runtime::discover()?;
        #[cfg(target_os = "linux")]
        if !runtime.is_appimage() {
            return Ok(None);
        }
        let environment = Environment::discover()?;
        let detection = install::detect(&environment, crate::link::ME, &runtime)?;
        if detection.state == State::DevBuild {
            return Ok(None);
        }
        #[cfg(target_os = "macos")]
        {
            use install::platform::macos;
            let Some(bundle) = macos::bundle_for(&runtime.executable) else { return Ok(None) };
            if macos::in_applications(&environment, &bundle) {
                // Dragging a bundle to Applications is consent; record it once.
                if detection.receipt.is_none() {
                    let receipt = macos_receipt(&environment, &bundle, None, None, &Options::defaults(&environment, paths))?;
                    environment.receipts().write(&receipt)?;
                }
                return Ok(None);
            }
            let needs_move = macos::is_translocated(&bundle)
                || bundle.starts_with(environment.home.join("Downloads"))
                || bundle.starts_with("/Volumes")
                || fs::metadata(&bundle)?.permissions().readonly();
            if !needs_move || dismissed(paths, &bundle) {
                return Ok(None);
            }
            let state =
                dialog_state(&detection, crate::VERSION)
                    .map(|s| if s == DialogState::Install { DialogState::MoveToApplications } else { s });
            Ok(state.map(|s| (environment, runtime, detection, s)))
        }
        #[cfg(target_os = "linux")]
        {
            if !install::should_prompt(&detection, args, false, dismissed(paths, &detection.running_from)) {
                return Ok(None);
            }
            Ok(dialog_state(&detection, crate::VERSION).map(|s| (environment, runtime, detection, s)))
        }
    }
}

#[cfg(target_os = "linux")]
pub fn install_copy(environment: &Environment, runtime: &Runtime, paths: &AppPaths, options: &Options) -> io::Result<Receipt> {
    if !runtime.is_appimage() {
        // A second install window, after the first one installed (and removed) this download.
        if runtime.appimage.as_ref().is_some_and(|p| !p.exists()) {
            if let Some(receipt) = environment.receipts().read(crate::link::ME)?.filter(|r| r.version == crate::VERSION && r.path.is_file())
            {
                return Ok(receipt);
            }
            return Err(io::Error::other(
                "This downloaded file is gone (it may have been installed from another window). Open Arcade Find from the app launcher.",
            ));
        }
        return Err(io::Error::other("--install requires a real AppImage, not an ordinary or development binary"));
    }
    // Render the bundled icon into a private, short-lived staging directory.
    // All sizes use Find's existing icon source; installation owns final paths.
    struct Icons(PathBuf);
    impl Drop for Icons {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let stage = Icons(paths.data.join("install-icons").join(arcade_link::endpoint::new_token()?));
    find_core::paths::ensure_private_dir(&stage.0)?;
    let mut icons = Vec::new();
    for size in [16, 22, 24, 32, 48, 64, 128, 256, 512] {
        let source = stage.0.join(format!("{size}.png"));
        fs::write(&source, crate::ui::icons::app_icon_png(size))?;
        icons.push(install::Icon { source, size: install::IconSize::Pixels(size as u16) });
    }
    let source = stage.0.join("icon.svg");
    fs::write(&source, crate::ui::icons::app_icon_svg())?;
    icons.push(install::Icon { source, size: install::IconSize::Scalable });
    let existing = environment.receipts().read(crate::link::ME)?;
    let managed_by = existing.as_ref().map_or(ManagedBy::SelfManaged, |r| r.managed_by);
    let mut receipt = install::install(
        environment,
        install::InstallOptions {
            app: &app_info(),
            runtime,
            version: crate::VERSION,
            channel: &options.channel,
            managed_by,
            icons: &icons,
            start_at_login: options.start_at_login,
            remove_download: options.remove_download,
            refresh_caches: true,
        },
    )?;
    if !options.start_at_login {
        if let Some(path) = receipt.integration.autostart.take() {
            remove_file(&path)?;
            environment.receipts().write(&receipt)?;
        }
    }
    // The install choice is reflected in Settings on the next resident start.
    let (mut settings, _) = Settings::load(&paths.settings_file());
    settings.start_at_login = Some(options.start_at_login);
    settings.save(&paths.settings_file())?;
    Ok(receipt)
}

#[cfg(windows)]
pub fn install_copy(_e: &Environment, _r: &Runtime, _p: &AppPaths, _o: &Options) -> io::Result<Receipt> {
    Err(io::Error::other("Use the per-user Inno Setup installer to install Arcade Find on Windows"))
}

#[cfg(target_os = "macos")]
pub fn install_copy(environment: &Environment, runtime: &Runtime, paths: &AppPaths, options: &Options) -> io::Result<Receipt> {
    use install::platform::macos;
    let d = install::detect(environment, crate::link::ME, runtime)?;
    if d.state == State::DevBuild {
        return Err(io::Error::other("development builds cannot be installed"));
    }
    let bundle = macos::bundle_for(&runtime.executable).ok_or_else(|| io::Error::other("--install requires an app bundle"))?;
    let target = bundle_destination(environment, d.receipt.as_ref(), "Arcade Find.app")?;
    let root = target.parent().unwrap().to_path_buf();
    let mut previous = d.receipt.as_ref().and_then(|r| r.previous.clone());
    if fs::canonicalize(&target).ok() != fs::canonicalize(&bundle).ok() || !target.exists() {
        // ditto preserves extended attributes and quarantine. Stage before
        // replacing a receipt-owned bundle; leave the downloaded copy intact.
        let stage = BundleStage(root.join(format!(".arcade-find-{}", arcade_link::endpoint::new_token()?)));
        let staged = macos::move_to_applications(environment, &bundle, Some(&stage.0))?;
        if target.exists() {
            let old = d
                .receipt
                .as_ref()
                .filter(|r| r.path == target)
                .ok_or_else(|| io::Error::other("Applications already contains a bundle without Find's receipt"))?;
            let prev = root.join(".previous/Arcade Find.app");
            fs::create_dir_all(prev.parent().unwrap())?;
            if prev.exists() {
                fs::remove_dir_all(&prev)?;
            }
            fs::rename(&target, &prev)?;
            previous = Some(arcade_link::receipt::Previous { version: old.version.clone(), path: prev });
        }
        if let Err(e) = fs::rename(&staged, &target) {
            if let Some(prev) = &previous {
                let _ = fs::rename(&prev.path, &target);
            }
            drop(stage);
            return Err(e);
        }
        drop(stage);
    }
    let exe = target.join("Contents/MacOS/arcade-find");
    if let Some(old) = &d.receipt {
        if !options.install_cli {
            if let Some(p) = &old.integration.cli {
                if fs::read_link(p).is_ok_and(|p| p == launch_path(old)) {
                    remove_file(p)?;
                }
            }
        }
        if !options.start_at_login {
            if let Some(p) = &old.integration.autostart {
                remove_file(p)?;
            }
        }
    }
    let cli = if options.install_cli { Some(macos::install_cli(environment, CLI_NAME, &exe)?) } else { None };
    let autostart = if options.start_at_login {
        let path = environment.home.join("Library/LaunchAgents/arcade.find.plist");
        arcade_link::paths::write_atomic(&path, crate::autostart::plist(&exe.to_string_lossy()).as_bytes(), true)?;
        Some(path)
    } else {
        None
    };
    let receipt = macos_receipt(environment, &target, cli, autostart, options).map(|mut r| {
        r.previous = previous;
        r
    })?;
    environment.receipts().write(&receipt)?;
    let (mut settings, _) = Settings::load(&paths.settings_file());
    settings.start_at_login = Some(options.start_at_login);
    settings.save(&paths.settings_file())?;
    Ok(receipt)
}

#[cfg(target_os = "macos")]
fn macos_receipt(
    environment: &Environment,
    bundle: &Path,
    cli: Option<PathBuf>,
    autostart: Option<PathBuf>,
    options: &Options,
) -> io::Result<Receipt> {
    let old = environment.receipts().read(crate::link::ME)?;
    let now = arcade_link::manifest::now_rfc3339();
    let receipt = Receipt {
        schema: 1,
        id: crate::link::ME.into(),
        version: crate::VERSION.into(),
        channel: options.channel.clone(),
        method: InstallMethod::MacosBundle,
        managed_by: old.as_ref().map_or(ManagedBy::SelfManaged, |r| r.managed_by),
        path: bundle.into(),
        integration: arcade_link::receipt::Integration { cli, autostart, ..Default::default() },
        previous: None,
        installed_at: old.as_ref().map_or_else(|| now.clone(), |r| r.installed_at.clone()),
        updated_at: now,
    };
    Ok(receipt)
}

pub fn launch_path(receipt: &Receipt) -> PathBuf {
    if receipt.method == InstallMethod::MacosBundle {
        receipt.path.join("Contents/MacOS/arcade-find")
    } else {
        receipt.path.clone()
    }
}

pub fn repair(environment: &Environment, runtime: &Runtime) -> io::Result<Receipt> {
    let d = install::detect(environment, crate::link::ME, runtime)?;
    #[cfg(target_os = "linux")]
    {
        // An explicitly invoked --repair from a moved AppImage is consent to
        // repoint this receipt; a dev CLI repairs the recorded copy only.
        let moved = (d.state != State::DevBuild && d.is_app_image).then_some(d.running_from.as_path());
        install::repair(environment, &app_info(), moved)
    }
    #[cfg(target_os = "macos")]
    {
        let mut r = d.receipt.ok_or_else(|| io::Error::other("no receipt"))?;
        if d.state != State::DevBuild && d.running_from.extension().is_some_and(|e| e == "app") {
            r.path = d.running_from;
        }
        let exe = launch_path(&r);
        if !exe.is_file() {
            return Err(io::Error::other("installed executable is missing"));
        }
        if r.integration.cli.is_some() {
            r.integration.cli = Some(install::platform::macos::install_cli(environment, CLI_NAME, &exe)?);
        }
        if let Some(p) = &r.integration.autostart {
            arcade_link::paths::write_atomic(p, crate::autostart::plist(&exe.to_string_lossy()).as_bytes(), true)?;
        }
        r.updated_at = arcade_link::manifest::now_rfc3339();
        environment.receipts().write(&r)?;
        Ok(r)
    }
    #[cfg(windows)]
    {
        let mut r = d.receipt.ok_or_else(|| io::Error::other("no receipt"))?;
        if d.state != State::DevBuild && d.running_from.file_name() == r.path.file_name() {
            r.path = d.running_from;
            if let Some(old) = &r.integration.uninstaller {
                let moved = r.path.parent().unwrap().join(old.file_name().unwrap_or_default());
                if moved.is_file() {
                    r.integration.uninstaller = Some(moved);
                }
            }
        }
        if !r.path.is_file() {
            return Err(io::Error::other("installed executable is missing"));
        }
        r.integration.cli = Some(install::platform::windows::write_shim(environment, CLI_NAME, &r.path)?);
        if let Some(path) = &r.integration.desktop_entry {
            write_windows_shortcut(path, &r.path, "")?;
            let settings = path.parent().unwrap().join("Arcade Find Settings.lnk");
            if settings.exists() {
                write_windows_shortcut(&settings, &r.path, "--settings")?;
            }
        }
        if let Some(path) = &r.integration.autostart {
            write_windows_shortcut(path, &r.path, "--background")?;
        }
        r.updated_at = arcade_link::manifest::now_rfc3339();
        environment.receipts().write(&r)?;
        Ok(r)
    }
}

pub fn uninstall(environment: &Environment, paths: &AppPaths, silent: bool, remove_data: bool) -> io::Result<bool> {
    if environment.receipts().read(crate::link::ME)?.is_none() {
        return Ok(false);
    }
    // Stop only this isolated/profile instance before removing its receipt,
    // and retain its lock throughout deletion. A timeout must not uninstall a
    // running copy or race a second resident launch.
    let _lock = stop_and_lock(paths, std::time::Duration::from_secs(15))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Arcade Find is still running"))?;
    #[cfg(windows)]
    {
        let Some(r) = environment.receipts().read(crate::link::ME)? else { return Ok(false) };
        let uninstaller = r.integration.uninstaller.ok_or_else(|| io::Error::other("receipt has no Windows uninstaller"))?;
        let mut cmd = std::process::Command::new(uninstaller);
        cmd.args(["/SUPPRESSMSGBOXES", "/NORESTART"]);
        if silent {
            cmd.arg("/VERYSILENT");
        }
        let status = cmd.status()?;
        if !status.success() {
            return Err(io::Error::other("Windows uninstaller failed"));
        }
        if remove_data {
            for path in paths.data_folders() {
                if path.exists() {
                    fs::remove_dir_all(path)?;
                }
            }
        }
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = silent;
        #[cfg(target_os = "macos")]
        if let Some(r) = environment.receipts().read(crate::link::ME)? {
            if let Some(p) = &r.integration.cli {
                if fs::read_link(p).is_ok_and(|p| p == launch_path(&r)) {
                    remove_file(p)?;
                }
            }
        }
        install::uninstall(environment, crate::link::ME, install::UninstallOptions { remove_data, data_folders: &paths.data_folders() })
    }
}

/// Asks the resident instance to quit and takes its lock. A copy that is still
/// starting (e.g. just restarted by Arcade Tools) holds the lock before it
/// answers commands, so the request is repeated until it lets go.
fn stop_and_lock(paths: &AppPaths, timeout: std::time::Duration) -> io::Result<Option<crate::instance::InstanceLock>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(lock) = crate::instance::acquire(paths)? {
            return Ok(Some(lock));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
        let _ = crate::instance::send(paths, &crate::instance::Command::Quit, std::time::Duration::from_millis(500));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Before the freshly installed copy starts: an older one still running would
/// otherwise keep the single-instance lock and the new copy would just show it.
pub fn stop_running_copy(paths: &AppPaths) {
    if crate::instance::running(paths) {
        drop(stop_and_lock(paths, std::time::Duration::from_secs(10)));
    }
}

#[cfg(windows)]
pub(crate) fn write_windows_shortcut(path: &Path, executable: &Path, arguments: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath={}; $s.Arguments={}; $s.Save()",
        quote(&path.to_string_lossy()),
        quote(&executable.to_string_lossy()),
        quote(arguments)
    );
    let status = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("Couldn't create the Windows shortcut"))
    }
}

#[cfg(not(windows))]
fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Tools is only an update owner here, never an action hub. Launch it only
/// after the user clicks Install/Update, with the published tools.install action.
pub fn tools_installed(environment: &Environment) -> bool {
    arcade_link::Registry::load(&environment.locations)
        .get(arcade_link::manifest::ids::TOOLS)
        .is_some_and(|m| m.executable_exists() && m.settings.link_enabled)
}

pub fn handoff_to_tools(environment: &Environment) -> Result<(), arcade_link::LinkError> {
    let registry = arcade_link::Registry::load(&environment.locations);
    let tools = registry
        .get(arcade_link::manifest::ids::TOOLS)
        .ok_or_else(|| arcade_link::LinkError::unavailable("Arcade Tools isn't installed"))?;
    let action = tools
        .action("tools.install")
        .filter(|a| a.available)
        .ok_or_else(|| arcade_link::LinkError::unavailable("This version of Arcade Tools cannot install Find"))?;
    let request = crate::link::request(
        action,
        vec![arcade_link::Content::text("text/plain", crate::link::ME)],
        serde_json::json!({"app": crate::link::ME}),
    );
    crate::link::invoke(&environment.locations, tools, &request, None, None).map(|_| ())
}

/// Strip installer controls from explicit CLI relaunches; automatic first-run
/// installation passes the user's original arguments unchanged.
pub fn launch_args(args: &[String]) -> Vec<OsString> {
    let mut result = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--channel" => {
                it.next();
            }
            "--install" | "--silent" | "--no-launch" => {}
            _ => result.push(OsString::from(arg)),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn detection(state: State) -> Detection {
        Detection { state, running_from: PathBuf::from("/copy.AppImage"), is_app_image: true, receipt: None, installed_path: None }
    }
    #[test]
    fn dialog_states_cover_first_run_updates_and_dev_skip() {
        assert_eq!(dialog_state(&detection(State::NotInstalled), "0.3.0"), Some(DialogState::Install));
        assert_eq!(
            dialog_state(&detection(State::InstalledElsewhere { version: "0.2.1".into() }), "0.3.0"),
            Some(DialogState::Update { from: "0.2.1".into() })
        );
        for v in ["0.3.0", "0.4.0", "unknown"] {
            assert_eq!(
                dialog_state(&detection(State::InstalledElsewhere { version: v.into() }), "0.3.0"),
                Some(DialogState::OpenInstalled { version: v.into() })
            );
        }
        assert_eq!(dialog_state(&detection(State::DevBuild), "0.3.0"), None);
        assert_eq!(dialog_state(&detection(State::InstalledHere), "0.3.0"), None);
        let d = detection(State::NotInstalled);
        for arg in ["--background", "--arcade-invoke", "--arcade-manifest", "--version", "--install", "--integration-status"] {
            assert!(!install::should_prompt(&d, &[arg.into()], false, false));
        }
        assert!(!install::should_prompt(&d, &[], false, true));
        assert!(!install::should_prompt(&detection(State::DevBuild), &[], false, false));
    }
    #[test]
    fn relaunch_does_not_reenter_the_installer() {
        assert_eq!(
            launch_args(&["--install", "--silent", "--channel", "nightly", "--no-launch"].map(String::from)),
            Vec::<OsString>::new()
        );
        assert_eq!(launch_args(&["--show", "report"].map(String::from)), [OsString::from("--show"), OsString::from("report")]);
    }
}

// Keep an adopted native installation in the Applications folder the user chose.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn bundle_destination(environment: &Environment, receipt: Option<&Receipt>, name: &str) -> io::Result<PathBuf> {
    if let Some(receipt) = receipt.filter(|r| r.method == InstallMethod::MacosBundle) {
        receipt.validate_native_paths()?;
        if receipt.path.file_name().is_some_and(|n| n == name)
            && receipt.path.parent().is_some_and(|p| p == environment.home.join("Applications") || p == Path::new("/Applications"))
        {
            return Ok(receipt.path.clone());
        }
        return Err(io::Error::other("the recorded bundle is outside its Applications folder"));
    }
    Ok(environment.home.join("Applications").join(name))
}

#[cfg(target_os = "macos")]
struct BundleStage(PathBuf);
#[cfg(target_os = "macos")]
impl Drop for BundleStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(all(test, unix))]
mod bundle_destination_tests {
    use super::*;

    #[test]
    fn native_updates_keep_the_adopted_applications_folder() {
        let environment = Environment::under(&std::env::temp_dir().join("arcade-bundle-destination"));
        let name = "Arcade Find.app";
        let default = environment.home.join("Applications").join(name);
        assert_eq!(bundle_destination(&environment, None, name).unwrap(), default);
        let mut receipt = Receipt {
            schema: 1,
            id: "arcade.find".into(),
            version: "0.2.0".into(),
            channel: "stable".into(),
            method: InstallMethod::MacosBundle,
            managed_by: ManagedBy::Tools,
            path: Path::new("/Applications").join(name),
            integration: Default::default(),
            previous: None,
            installed_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(bundle_destination(&environment, Some(&receipt), name).unwrap(), receipt.path);
        receipt.path = default.clone();
        assert_eq!(bundle_destination(&environment, Some(&receipt), name).unwrap(), default);
        receipt.path = environment.home.join("Downloads").join(name);
        assert!(bundle_destination(&environment, Some(&receipt), name).is_err());
        receipt.path = Path::new("/Applications/Other.app").into();
        assert!(bundle_destination(&environment, Some(&receipt), name).is_err());
    }
}
