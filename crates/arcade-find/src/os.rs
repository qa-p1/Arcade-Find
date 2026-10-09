//! Operating-system actions on files: open, reveal, rename (never
//! overwriting), move to the trash (never a hard delete), and the clipboard.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::OnceLock;

/// Opens with the default app, detached.
pub fn open(path: &Path) -> Result<(), String> {
    open::that_detached(path).map_err(|e| format!("Couldn't open {}: {e}", display_name(path)))
}

pub fn display_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Shows the item selected in the file manager.
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if reveal_dbus(path).is_ok() {
            return Ok(());
        }
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        open::that_detached(dir).map_err(|e| format!("Couldn't open the folder: {e}"))
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut arg = std::ffi::OsString::from("/select,\"");
        arg.push(path.as_os_str());
        arg.push("\"");
        std::process::Command::new("explorer.exe").raw_arg(arg).spawn().map(|_| ()).map_err(|e| format!("Couldn't open Explorer: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn().map(|_| ()).map_err(|e| format!("Couldn't open Finder: {e}"))
    }
}

#[cfg(target_os = "linux")]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// `org.freedesktop.FileManager1.ShowItems` (Files, Dolphin, Thunar, Nemo…).
#[cfg(target_os = "linux")]
fn reveal_dbus(path: &Path) -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    conn.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(vec![file_uri(path)], ""),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// A valid new name for an entry (no separators, not `.`/`..`).
pub fn valid_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("The name can't be empty.".into());
    }
    if n == "." || n == ".." {
        return Err("That name isn't allowed.".into());
    }
    if n.contains('/') || n.contains('\0') || (cfg!(windows) && n.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])) {
        return Err("Names can't contain / or other reserved characters.".into());
    }
    if n.len() > 255 {
        return Err("That name is too long.".into());
    }
    Ok(())
}

/// Renames within the same folder; refuses if the new name exists.
pub fn rename_noreplace(from: &Path, new_name: &str) -> Result<PathBuf, String> {
    valid_name(new_name)?;
    let parent = from.parent().ok_or("This item can't be renamed.")?;
    let to = parent.join(new_name.trim());
    if to == from {
        return Ok(to);
    }
    // Case-only renames on case-insensitive file systems point at the same file.
    let same_file = same_entry(from, &to);
    if !same_file && std::fs::symlink_metadata(&to).is_ok() {
        return Err(format!("“{}” already exists here.", new_name.trim()));
    }
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let a = CString::new(from.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let b = CString::new(to.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: valid C strings; RENAME_NOREPLACE fails if `to` exists.
        let r = unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), libc::RENAME_NOREPLACE) };
        if r == 0 {
            return Ok(to);
        }
        let err = std::io::Error::last_os_error();
        // Some file systems don't support the flag: fall back after the check above.
        if err.raw_os_error() != Some(libc::EINVAL) && err.raw_os_error() != Some(libc::ENOSYS) {
            return Err(rename_error(&err, new_name));
        }
    }
    std::fs::rename(from, &to).map_err(|e| rename_error(&e, new_name))?;
    Ok(to)
}

fn same_entry(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

fn rename_error(e: &std::io::Error, name: &str) -> String {
    match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("“{}” already exists here.", name.trim()),
        std::io::ErrorKind::PermissionDenied => "You don't have permission to rename this.".into(),
        _ => format!("Couldn't rename: {e}"),
    }
}

/// Moves items to the trash (recycle bin). Never deletes permanently.
pub fn trash(paths: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(paths).map_err(|e| format!("Couldn't move to Trash: {e}"))
}

enum ClipReq {
    Text(String, mpsc::Sender<Result<(), String>>),
    Files(Vec<PathBuf>, mpsc::Sender<Result<(), String>>),
    Get(mpsc::Sender<Option<String>>),
}

/// The clipboard lives on its own thread for the life of the app: on X11
/// and Wayland the owner must stay alive to serve what it copied.
fn clipboard() -> &'static mpsc::Sender<ClipReq> {
    static TX: OnceLock<mpsc::Sender<ClipReq>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<ClipReq>();
        let _ = std::thread::Builder::new().name("find-clipboard".into()).spawn(move || {
            let mut cb: Option<arboard::Clipboard> = None;
            for req in rx {
                if cb.is_none() {
                    cb = arboard::Clipboard::new().ok();
                }
                match req {
                    ClipReq::Text(t, reply) => {
                        let r = cb.as_mut().ok_or("The clipboard isn't available.".to_string()).and_then(|c| c.set_text(t).map_err(|e| e.to_string()));
                        let _ = reply.send(r);
                    }
                    ClipReq::Files(f, reply) => {
                        let r = cb.as_mut().ok_or("The clipboard isn't available.".to_string()).and_then(|c| c.set().file_list(&f).map_err(|e| e.to_string()));
                        let _ = reply.send(r);
                    }
                    ClipReq::Get(reply) => {
                        let _ = reply.send(cb.as_mut().and_then(|c| c.get_text().ok()));
                    }
                }
            }
        });
        tx
    })
}

pub fn copy_text(text: String) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    clipboard().send(ClipReq::Text(text, tx)).map_err(|e| e.to_string())?;
    rx.recv_timeout(std::time::Duration::from_secs(3)).map_err(|_| "The clipboard didn't respond.".to_string())?
}

pub fn copy_files(paths: Vec<PathBuf>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    clipboard().send(ClipReq::Files(paths, tx)).map_err(|e| e.to_string())?;
    rx.recv_timeout(std::time::Duration::from_secs(3)).map_err(|_| "The clipboard didn't respond.".to_string())?
}

pub fn paste_text() -> Option<String> {
    let (tx, rx) = mpsc::channel();
    clipboard().send(ClipReq::Get(tx)).ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(2)).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("ok.txt").is_ok());
        assert!(valid_name("").is_err());
        assert!(valid_name("..").is_err());
        assert!(valid_name("a/b").is_err());
    }

    #[test]
    fn rename_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("af-rename-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, "A").unwrap();
        std::fs::write(&b, "B").unwrap();
        assert!(rename_noreplace(&a, "b.txt").is_err());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "B");
        let c = rename_noreplace(&a, "c.txt").unwrap();
        assert_eq!(std::fs::read_to_string(c).unwrap(), "A");
        assert!(!a.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn uris_are_escaped() {
        assert_eq!(file_uri(Path::new("/a b/ü#.txt")), "file:///a%20b/%C3%BC%23.txt");
    }
}
