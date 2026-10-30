//! Where Arcade Find keeps its settings, index and runtime files.
//!
//! | | Settings | Data (index, frecency, pins) | Runtime (instance socket) |
//! |---|---|---|---|
//! | Linux | `$XDG_CONFIG_HOME/arcade/find` | `$XDG_DATA_HOME/arcade/find` | `$XDG_RUNTIME_DIR/arcade/find` |
//! | Windows | `%APPDATA%\Arcade\Find` | `%APPDATA%\Arcade\Find` | named pipe |
//! | macOS | `~/Library/Application Support/Arcade/Find` | same | `$TMPDIR/arcade-find` |
//!
//! `ARCADE_FIND_HOME=<dir>` moves all three under `<dir>` (tests, portable
//! use); `ARCADE_FIND_PROFILE=<name>` keeps a separate profile beside the
//! default one. Link's own locations follow `ARCADE_HOME` (see the Link crate).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub runtime: PathBuf,
    /// The profile name ("" for the default profile).
    pub profile: String,
    /// True when `ARCADE_FIND_HOME` is set: never register with the desktop.
    pub isolated: bool,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        env_path("USERPROFILE").unwrap_or_else(|| PathBuf::from("C:\\"))
    }
    #[cfg(not(windows))]
    {
        env_path("HOME").unwrap_or_else(|| PathBuf::from("/"))
    }
}

fn valid_profile(p: &str) -> bool {
    !p.is_empty() && p.len() <= 64 && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl AppPaths {
    pub fn discover() -> AppPaths {
        let profile = std::env::var("ARCADE_FIND_PROFILE").ok().filter(|p| valid_profile(p)).unwrap_or_default();
        if let Some(root) = env_path("ARCADE_FIND_HOME") {
            return AppPaths::under(&root, &profile);
        }
        let dirs = arcade_link::paths::app_dirs("arcade.find");
        let config = dirs.config;
        let data = dirs.data;
        let runtime = arcade_link::paths::runtime_dir().join("find");
        let mut p = AppPaths { config, data, runtime, profile: String::new(), isolated: false };
        if !profile.is_empty() {
            p.config = p.config.join("profiles").join(&profile);
            p.data = p.data.join("profiles").join(&profile);
            p.runtime = p.runtime.join(&profile);
            p.profile = profile;
        }
        p
    }

    /// Link performs copy → verify → remove. A conflict stops startup rather
    /// than silently loading defaults or discarding the user's old state.
    pub fn migrate(&self) -> std::io::Result<bool> {
        if self.isolated {
            return Ok(false);
        }
        let report = arcade_link::paths::migrate_app_dirs("arcade.find");
        if !report.kept.is_empty() {
            return Err(std::io::Error::other(format!(
                "Couldn't migrate Find's folders. The originals were kept. {}",
                report.kept.iter().map(|(path, reason)| format!("{}: {reason}", path.display())).collect::<Vec<_>>().join("; ")
            )));
        }
        Ok(!report.moved.is_empty())
    }

    pub fn under(root: &Path, profile: &str) -> AppPaths {
        let base = if profile.is_empty() { root.to_path_buf() } else { root.join("profiles").join(profile) };
        AppPaths { config: base.join("config"), data: base.join("data"), runtime: base.join("run"), profile: profile.to_string(), isolated: true }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn index_file(&self) -> PathBuf {
        self.data.join("index.bin")
    }
    pub fn frecency_file(&self) -> PathBuf {
        self.data.join("frecency.json")
    }
    pub fn pins_file(&self) -> PathBuf {
        self.data.join("pins.json")
    }
    pub fn log_file(&self) -> PathBuf {
        self.log_dir().join("arcade-find.log")
    }
    pub fn log_dir(&self) -> PathBuf {
        self.data.join("logs")
    }
    /// The explicit data allowlist for --uninstall --remove-data. Deduplicated
    /// because config and data share a directory on macOS.
    pub fn data_folders(&self) -> Vec<PathBuf> {
        let mut folders = vec![self.config.clone(), self.data.clone()];
        folders.sort();
        folders.dedup();
        folders
    }
    /// A short name for the instance channel, unique per user and profile.
    pub fn instance_name(&self) -> String {
        let h = crate::index::key_extend(crate::index::KEY_SEED, self.runtime.as_os_str().as_encoded_bytes());
        format!("arcade-find-{:012x}", h & 0xFFFF_FFFF_FFFF)
    }
}

/// Expands a leading `~` to the home folder.
pub fn expand_tilde(s: &str) -> PathBuf {
    let s = s.trim();
    if s == "~" {
        return home_dir();
    }
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }
    PathBuf::from(s)
}

/// Shows a path with `~` for the home folder.
pub fn tilde(path: &Path) -> String {
    let home = home_dir();
    if home.as_os_str().len() > 1 {
        if path == home {
            return "~".into();
        }
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
        }
    }
    path.display().to_string()
}

/// Creates `dir` (and parents) private to the user where the OS supports it.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(dir)?;
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::other(format!("{} is a symlink", dir.display())));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Writes a file atomically: a temporary sibling, fsync, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    arcade_link::paths::write_atomic(path, bytes, true)
}

/// An absolute path for tests: `/a/b` on Unix, `C:\a\b` on Windows (where a
/// path without a drive isn't absolute).
#[cfg(test)]
pub(crate) fn test_abs(p: &str) -> String {
    if cfg!(windows) {
        format!("C:{}", p.replace('/', "\\"))
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_layout() {
        let p = AppPaths::under(Path::new("/x"), "");
        assert_eq!(p.settings_file(), PathBuf::from("/x/config/settings.json"));
        assert_eq!(p.index_file(), PathBuf::from("/x/data/index.bin"));
        let q = AppPaths::under(Path::new("/x"), "work");
        assert_eq!(q.data, PathBuf::from("/x/profiles/work/data"));
        assert_ne!(p.instance_name(), q.instance_name());
        assert!(valid_profile("a-b_1"));
        assert!(!valid_profile("../x"));
    }

    #[test]
    fn concurrent_atomic_writes_never_share_a_temporary_file() {
        let root = std::env::temp_dir().join(format!("find-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = std::sync::Arc::new(root.join("settings.json"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|id| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for seq in 0..32 {
                        let value = serde_json::json!({"writer": id, "sequence": seq, "payload": "x".repeat(4096)});
                        write_atomic(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&*path).unwrap()).unwrap();
        assert_eq!(value["sequence"], 31);
        assert_eq!(value["payload"].as_str().unwrap().len(), 4096);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tilde_roundtrip() {
        let h = home_dir();
        assert_eq!(expand_tilde("~"), h);
        assert_eq!(expand_tilde("~/Projects"), h.join("Projects"));
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
        assert!(tilde(&h.join("a")).starts_with('~'));
    }
}
