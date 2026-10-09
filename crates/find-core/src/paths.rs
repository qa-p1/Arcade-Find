//! Where Arcade Find keeps its settings, index and runtime files.
//!
//! | | Settings | Data (index, frecency, pins) | Runtime (instance socket) |
//! |---|---|---|---|
//! | Linux | `$XDG_CONFIG_HOME/arcade-find` | `$XDG_DATA_HOME/arcade-find` | `$XDG_RUNTIME_DIR/arcade-find` |
//! | Windows | `%APPDATA%\Arcade\Arcade Find` | `%LOCALAPPDATA%\Arcade\Arcade Find` | named pipe |
//! | macOS | `~/Library/Application Support/Arcade Find` | same | `$TMPDIR/arcade-find` |
//!
//! `ARCADE_FIND_HOME=<dir>` moves all three under `<dir>` (tests, portable
//! use); `ARCADE_FIND_PROFILE=<name>` keeps a separate profile beside the
//! default one. Link's own locations follow `ARCADE_HOME` (see the Link crate).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let home = home_dir();
        #[cfg(target_os = "linux")]
        let (config, data, runtime) = {
            let config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")).join("arcade-find");
            let data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")).join("arcade-find");
            let runtime = env_path("XDG_RUNTIME_DIR").map(|d| d.join("arcade-find")).unwrap_or_else(|| data.join("run"));
            (config, data, runtime)
        };
        #[cfg(target_os = "macos")]
        let (config, data, runtime) = {
            let support = home.join("Library/Application Support/Arcade Find");
            let runtime = env_path("TMPDIR").unwrap_or_else(std::env::temp_dir).join("arcade-find");
            (support.clone(), support, runtime)
        };
        #[cfg(windows)]
        let (config, data, runtime) = {
            let roaming = env_path("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
            let local = env_path("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
            let data = local.join("Arcade").join("Arcade Find");
            (roaming.join("Arcade").join("Arcade Find"), data.clone(), data.join("run"))
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let (config, data, runtime) = (home.join(".config/arcade-find"), home.join(".local/share/arcade-find"), home.join(".local/share/arcade-find/run"));
        let mut p = AppPaths { config, data, runtime, profile: String::new(), isolated: false };
        if !profile.is_empty() {
            p.config = p.config.join("profiles").join(&profile);
            p.data = p.data.join("profiles").join(&profile);
            p.runtime = p.runtime.join(&profile);
            p.profile = profile;
        }
        p
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
        self.data.join("arcade-find.log")
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
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let r = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    match r {
        Ok(()) => std::fs::rename(&tmp, path),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
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
    fn tilde_roundtrip() {
        let h = home_dir();
        assert_eq!(expand_tilde("~"), h);
        assert_eq!(expand_tilde("~/Projects"), h.join("Projects"));
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
        assert!(tilde(&h.join("a")).starts_with('~'));
    }
}
