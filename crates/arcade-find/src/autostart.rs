//! Start at login, with the same per-user locations Arcade Tools uses:
//! `~/.config/autostart/arcade-find.desktop` (Linux), a receipt-owned Startup
//! shortcut (Windows), `~/Library/LaunchAgents/dev.arcade.Find.plist`
//! (macOS). Development builds and isolated test profiles never register.

#[cfg(any(windows, target_os = "macos"))]
use std::path::PathBuf;

use find_core::paths::AppPaths;
use find_core::Settings;

pub const BACKGROUND_FLAG: &str = "--background";

// Link v0.3.0's typed receipt omits extension fields on serialization. Retain
// the original JSON when a login toggle updates an existing installation.
fn read_receipt(environment: &arcade_link::install::Environment) -> Result<Option<(arcade_link::receipt::Receipt, serde_json::Value)>, String> {
    let path = environment.receipts().path(crate::link::ME).map_err(|e| e.to_string())?;
    let text = match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => result.map_err(|e| e.to_string())?,
    };
    let receipt = arcade_link::receipt::Receipt::from_json(&text).map_err(|e| e.to_string())?;
    receipt.validate_native_paths().map_err(|e| e.to_string())?;
    if receipt.id != crate::link::ME || receipt.method == arcade_link::receipt::InstallMethod::Dev {
        return Err("Not a Find installation receipt".into());
    }
    Ok(Some((receipt, serde_json::from_str(&text).map_err(|e| e.to_string())?)))
}

fn write_receipt(
    environment: &arcade_link::install::Environment,
    receipt: &arcade_link::receipt::Receipt,
    mut document: serde_json::Value,
) -> Result<(), String> {
    receipt.validate_native_paths().map_err(|e| e.to_string())?;
    let mut updated = serde_json::to_value(receipt).map_err(|e| e.to_string())?;
    let integration = updated.as_object_mut().unwrap().remove("integration").unwrap();
    document["integration"].as_object_mut().unwrap().extend(integration.as_object().unwrap().clone());
    document.as_object_mut().unwrap().extend(updated.as_object().unwrap().clone());
    let store = environment.receipts();
    arcade_link::paths::ensure_private_dir(store.dir()).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?;
    arcade_link::paths::write_atomic(&store.path(crate::link::ME).map_err(|e| e.to_string())?, &bytes, true).map_err(|e| e.to_string())
}

fn disable_in(environment: &arcade_link::install::Environment) -> Result<(), String> {
    let Some((mut receipt, document)) = read_receipt(environment)? else { return Ok(()) };
    if let Some(path) = receipt.integration.autostart.take() {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
        receipt.updated_at = arcade_link::manifest::now_rfc3339();
        write_receipt(environment, &receipt, document)?;
    }
    Ok(())
}

/// The stable executable to start at login, or `None` for development
/// builds (inside a Cargo `target` folder).
pub fn installed_executable() -> Option<String> {
    let environment = arcade_link::install::Environment::discover().ok()?;
    let runtime = arcade_link::install::Runtime::discover().ok()?;
    let detection = arcade_link::install::detect(&environment, crate::link::ME, &runtime).ok()?;
    if detection.state != arcade_link::install::State::InstalledHere {
        return None;
    }
    Some(crate::installer::launch_path(detection.receipt.as_ref()?).to_string_lossy().into())
}

/// Whether registration is allowed for this process.
pub fn allowed(paths: &AppPaths) -> bool {
    !paths.isolated && installed_executable().is_some()
}

/// On first run of an installed build, registers (the default is on, visible
/// and reversible in Settings → General). Afterwards the login entry itself is
/// the truth: Arcade Tools can switch it too, so the setting follows it, and
/// an existing entry is refreshed in case the executable moved.
pub fn sync(paths: &AppPaths, settings: &mut Settings) -> Result<(), String> {
    if !allowed(paths) {
        return Ok(());
    }
    let on = enabled();
    settings.start_at_login = Some(on);
    if on {
        set(true)
    } else {
        Ok(())
    }
}

pub fn set(enabled: bool) -> Result<(), String> {
    let exe = installed_executable().ok_or("Start at login is only available for installed copies")?;
    if enabled {
        enable(&exe)
    } else {
        disable()
    }
}

#[cfg(target_os = "linux")]
fn enable(_exe: &str) -> Result<(), String> {
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    let runtime = arcade_link::install::Runtime::discover().map_err(|e| e.to_string())?;
    enable_linux(&environment, &runtime)
}

#[cfg(target_os = "linux")]
fn enable_linux(environment: &arcade_link::install::Environment, runtime: &arcade_link::install::Runtime) -> Result<(), String> {
    let (old, document) = read_receipt(environment)?.ok_or("No install receipt")?;
    // Shared installer owns all desktop-entry quoting and autostart repair.
    let receipt = arcade_link::install::install(
        environment,
        arcade_link::install::InstallOptions {
            app: &crate::installer::app_info(),
            runtime,
            version: &old.version,
            channel: &old.channel,
            managed_by: old.managed_by,
            icons: &[],
            start_at_login: true,
            remove_download: false,
            refresh_caches: false,
        },
    )
    .map_err(|e| e.to_string())?;
    crate::installer::finish_desktop_metadata(&receipt).map_err(|e| e.to_string())?;
    write_receipt(environment, &receipt, document)
}

#[cfg(target_os = "linux")]
fn disable() -> Result<(), String> {
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    disable_in(&environment)
}

#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    arcade_link::install::Environment::discover()
        .ok()
        .and_then(|e| e.receipts().read(crate::link::ME).ok().flatten())
        .and_then(|r| r.integration.autostart)
        // An entry switched off in the desktop's startup settings won't run.
        .is_some_and(|p| arcade_link::install::autostart_enabled(&p))
}

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    find_core::paths::home_dir().join("Library/LaunchAgents/dev.arcade.Find.plist")
}

pub fn plist(exe: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>dev.arcade.Find</string>\n  <key>ProgramArguments</key>\n  <array><string>{}</string><string>{BACKGROUND_FLAG}</string></array>\n  <key>RunAtLoad</key><true/>\n  <key>ProcessType</key><string>Interactive</string>\n</dict>\n</plist>\n",
        esc(exe)
    )
}

#[cfg(target_os = "macos")]
fn enable(exe: &str) -> Result<(), String> {
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    let (mut receipt, document) = read_receipt(&environment)?.ok_or("No install receipt")?;
    let p = plist_path();
    let want = plist(exe);
    if std::fs::read_to_string(&p).ok().as_deref() != Some(want.as_str()) {
        arcade_link::paths::write_atomic(&p, want.as_bytes(), true).map_err(|e| e.to_string())?;
    }
    if receipt.integration.autostart.as_ref() != Some(&p) {
        let previous = receipt.integration.autostart.replace(p.clone());
        if let Some(old) = previous.filter(|old| old != &p && old.file_name().is_some_and(|name| name == "arcade.find.plist")) {
            if std::fs::read_to_string(&p).ok().as_deref() == Some(want.as_str()) {
                let _ = std::fs::remove_file(old);
            }
        }
        receipt.updated_at = arcade_link::manifest::now_rfc3339();
        write_receipt(&environment, &receipt, document)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn disable() -> Result<(), String> {
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    disable_in(&environment)
}

#[cfg(target_os = "macos")]
pub fn enabled() -> bool {
    arcade_link::install::Environment::discover()
        .ok()
        .and_then(|e| e.receipts().read(crate::link::ME).ok().flatten())
        .and_then(|r| r.integration.autostart)
        .is_some_and(|p| p.is_file())
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    pub const NAME: &str = "ArcadeFind";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn delete() -> Result<(), String> {
        let (k, n) = (wide(KEY), wide(NAME));
        // SAFETY: NUL-terminated wide strings.
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
        if r == ERROR_SUCCESS || r == 2 {
            Ok(())
        } else {
            Err(format!("Couldn't update the Run key (error {r})"))
        }
    }

    pub fn exists() -> bool {
        let (k, n) = (wide(KEY), wide(NAME));
        let mut size = 0u32;
        // SAFETY: size query with a null buffer.
        unsafe {
            RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) == ERROR_SUCCESS
        }
    }
}

#[cfg(windows)]
fn enable(exe: &str) -> Result<(), String> {
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    let (mut receipt, document) = read_receipt(&environment)?.ok_or("No install receipt")?;
    let startup =
        std::env::var_os("APPDATA").map(PathBuf::from).ok_or("APPDATA isn't available")?.join("Microsoft/Windows/Start Menu/Programs/Startup/Arcade Find.lnk");
    crate::installer::write_windows_shortcut(&startup, std::path::Path::new(exe), BACKGROUND_FLAG).map_err(|e| e.to_string())?;
    receipt.integration.autostart = Some(startup);
    receipt.updated_at = arcade_link::manifest::now_rfc3339();
    write_receipt(&environment, &receipt, document)?;
    win::delete()
}

#[cfg(windows)]
fn disable() -> Result<(), String> {
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    disable_in(&environment)?;
    win::delete()
}

#[cfg(windows)]
pub fn enabled() -> bool {
    let startup = arcade_link::install::Environment::discover()
        .ok()
        .and_then(|e| e.receipts().read(crate::link::ME).ok().flatten())
        .and_then(|r| r.integration.autostart)
        .is_some_and(|p| p.is_file());
    startup || win::exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_login_item_quotes_xml() {
        assert!(plist("/Applications/Arcade Find.app/Contents/MacOS/arcade-find").contains("<string>dev.arcade.Find</string>"));
        assert!(plist("/a&b").contains("/a&amp;b"));
    }

    #[test]
    fn dev_builds_never_register() {
        // Tests run from target/debug/deps: not an installed copy.
        let p = AppPaths::under(&std::env::temp_dir().join("af-auto"), "");
        assert!(!allowed(&p));
        let mut s = Settings::default();
        sync(&p, &mut s).unwrap();
        assert_eq!(s.start_at_login, None, "nothing decided for a dev build");
    }

    #[test]
    fn login_toggles_preserve_receipt_extensions() {
        let root = std::env::temp_dir().join(format!("find-login-{}-{}", std::process::id(), arcade_link::endpoint::new_token().unwrap()));
        let environment = arcade_link::install::Environment::under(&root);
        let executable = environment.applications.join("Arcade-Find.AppImage");
        let startup = environment.config.join("autostart/arcade-find.desktop");
        let document = serde_json::json!({
            "schema": 1, "id": crate::link::ME, "version": "0.3.0", "channel": "stable",
            "method": "appimage", "managedBy": "tools", "path": executable,
            "integration": {"icons": [], "autostart": startup, "futureIntegration": {"keep": true}},
            "toolsExtension": {"keep": true},
            "installedAt": arcade_link::manifest::now_rfc3339(), "updatedAt": arcade_link::manifest::now_rfc3339()
        });
        let store = environment.receipts();
        arcade_link::paths::write_atomic(&store.path(crate::link::ME).unwrap(), &serde_json::to_vec(&document).unwrap(), true).unwrap();
        for _ in 0..2 {
            arcade_link::paths::write_atomic(&startup, b"login fixture", true).unwrap();
            let (mut receipt, original) = read_receipt(&environment).unwrap().unwrap();
            receipt.integration.autostart = Some(startup.clone());
            write_receipt(&environment, &receipt, original).unwrap();
            disable_in(&environment).unwrap();
            let (receipt, changed) = read_receipt(&environment).unwrap().unwrap();
            assert!(receipt.integration.autostart.is_none());
            assert!(!startup.exists());
            assert_eq!(changed["toolsExtension"], document["toolsExtension"]);
            assert_eq!(changed["integration"]["futureIntegration"], document["integration"]["futureIntegration"]);
            assert_eq!(changed["managedBy"], "tools");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(store.path(crate::link::ME).unwrap()).unwrap().permissions().mode() & 0o777, 0o600);
            }
        }
        #[cfg(target_os = "linux")]
        {
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, b"installed fixture").unwrap();
            // The same installed file is refreshed; no source copying or GUI.
            let runtime = arcade_link::install::Runtime { executable: executable.clone(), appimage: None, appdir: None, dev_build: false };
            enable_linux(&environment, &runtime).unwrap();
            let (receipt, changed) = read_receipt(&environment).unwrap().unwrap();
            let new_startup = environment.config.join("autostart/dev.arcade.Find.desktop");
            assert_eq!(receipt.integration.autostart.as_ref(), Some(&new_startup));
            assert!(new_startup.is_file());
            assert!(!startup.exists());
            let launcher = std::fs::read_to_string(receipt.integration.desktop_entry.as_ref().unwrap()).unwrap();
            assert!(launcher.contains("StartupWMClass=dev.arcade.Find"));
            assert!(launcher.contains("[Desktop Action Settings]\nName=Settings"));
            assert_eq!(changed["toolsExtension"], document["toolsExtension"]);
            assert_eq!(changed["integration"]["futureIntegration"], document["integration"]["futureIntegration"]);
            disable_in(&environment).unwrap();
            assert!(!new_startup.exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
