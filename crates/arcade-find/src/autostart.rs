//! Start at login, with the same per-user locations Arcade Tools uses:
//! `~/.config/autostart/arcade-find.desktop` (Linux), the HKCU `Run` value
//! `ArcadeFind` (Windows), `~/Library/LaunchAgents/arcade.find.plist`
//! (macOS). Development builds and isolated test profiles never register.

use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;

use find_core::paths::AppPaths;
use find_core::Settings;

pub const BACKGROUND_FLAG: &str = "--background";

/// The stable executable to start at login, or `None` for development
/// builds (inside a Cargo `target` folder).
pub fn installed_executable() -> Option<String> {
    let exe = arcade_link::manifest::current_executable();
    let p = Path::new(&exe);
    let parts: Vec<&std::ffi::OsStr> = p.components().map(|c| c.as_os_str()).collect();
    // `…/target/{debug,release}/…` or `…/target/<triple>/{debug,release}/…`.
    let dev =
        parts.iter().enumerate().any(|(i, c)| *c == "target" && parts[i + 1..].iter().take(2).any(|n| *n == "debug" || *n == "release"));
    (!dev && !exe.is_empty()).then_some(exe)
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
    let on = match settings.start_at_login {
        None => true,
        Some(_) => enabled(),
    };
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
fn entry_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| find_core::paths::home_dir().join(".config"));
    base.join("autostart").join("arcade-find.desktop")
}

/// Quotes an argument for a desktop entry's `Exec` key.
pub fn desktop_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

pub fn desktop_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Arcade Find\nComment=Find any file or folder instantly\nExec={} {BACKGROUND_FLAG}\nIcon=arcade-find\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
        desktop_quote(exe)
    )
}

#[cfg(target_os = "linux")]
fn enable(exe: &str) -> Result<(), String> {
    let p = entry_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let want = desktop_entry(exe);
    if std::fs::read_to_string(&p).ok().as_deref() == Some(want.as_str()) {
        return Ok(());
    }
    find_core::paths::write_atomic(&p, want.as_bytes()).map_err(|e| format!("Couldn't write {}: {e}", p.display()))
}

#[cfg(target_os = "linux")]
fn disable() -> Result<(), String> {
    match std::fs::remove_file(entry_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    entry_path().is_file()
}

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    find_core::paths::home_dir().join("Library/LaunchAgents/arcade.find.plist")
}

pub fn plist(exe: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>arcade.find</string>\n  <key>ProgramArguments</key>\n  <array><string>{}</string><string>{BACKGROUND_FLAG}</string></array>\n  <key>RunAtLoad</key><true/>\n  <key>ProcessType</key><string>Interactive</string>\n</dict>\n</plist>\n",
        esc(exe)
    )
}

#[cfg(target_os = "macos")]
fn enable(exe: &str) -> Result<(), String> {
    let p = plist_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let want = plist(exe);
    if std::fs::read_to_string(&p).ok().as_deref() == Some(want.as_str()) {
        return Ok(());
    }
    find_core::paths::write_atomic(&p, want.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn disable() -> Result<(), String> {
    match std::fs::remove_file(plist_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(target_os = "macos")]
pub fn enabled() -> bool {
    plist_path().is_file()
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    pub const NAME: &str = "ArcadeFind";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set(value: &str) -> Result<(), String> {
        let (k, n, v) = (wide(KEY), wide(NAME), wide(value));
        // SAFETY: NUL-terminated wide strings; the size includes the terminator.
        let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) };
        if r == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("Couldn't update the Run key (error {r})"))
        }
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
            RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
                == ERROR_SUCCESS
        }
    }
}

#[cfg(windows)]
fn enable(exe: &str) -> Result<(), String> {
    win::set(&format!("\"{exe}\" {BACKGROUND_FLAG}"))
}

#[cfg(windows)]
fn disable() -> Result<(), String> {
    win::delete()
}

#[cfg(windows)]
pub fn enabled() -> bool {
    win::exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entries_quote_paths() {
        assert_eq!(desktop_quote("/a b/$x\"y"), "\"/a b/\\$x\\\"y\"");
        let e = desktop_entry("/home/u/Applications/Arcade/Arcade-Find.AppImage");
        assert!(e.contains("Exec=\"/home/u/Applications/Arcade/Arcade-Find.AppImage\" --background\n"));
        assert!(plist("/Applications/Arcade Find.app/Contents/MacOS/arcade-find").contains("<string>arcade.find</string>"));
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
}
