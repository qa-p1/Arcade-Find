//! User settings (`settings.json`), versioned, written atomically, and
//! recovered from malformed files (the bad file is kept beside it).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::{expand_tilde, write_atomic};

pub const SCHEMA: u32 = 1;

/// The default global shortcut (no clash with Box, Look, Lens, Clipboard or Wheel).
pub const DEFAULT_SHORTCUT: &str = "Ctrl+Alt+F";

pub fn default_exclude_names() -> Vec<String> {
    [".git", "node_modules", "target", ".cache", "__pycache__", ".venv", ".snapshots"].iter().map(|s| s.to_string()).collect()
}

pub fn default_exclude_paths() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        vec!["/proc".into(), "/sys".into(), "~/.local/share/Trash".into()]
    }
    #[cfg(target_os = "macos")]
    {
        vec!["~/.Trash".into(), "~/Library/Caches".into()]
    }
    #[cfg(windows)]
    {
        vec!["~/AppData/Local/Temp".into()]
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        vec![]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rows {
    /// Rows shown while the list scrolls.
    pub visible: u32,
    /// Up to this many results the list grows instead of scrolling.
    pub stretch: u32,
}

impl Default for Rows {
    fn default() -> Self {
        Rows { visible: 5, stretch: 7 }
    }
}

impl Rows {
    /// Rows the window shows for `count` results.
    pub fn viewport(&self, count: usize) -> usize {
        let visible = self.visible.clamp(1, 20) as usize;
        let stretch = (self.stretch as usize).max(visible);
        if count <= stretch {
            count
        } else {
            visible
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkSettings {
    /// "Connect with other Arcade apps".
    pub enabled: bool,
    /// Peers switched off in this app ("Use with Arcade Find" off).
    pub disabled_peers: Vec<String>,
}

impl Default for LinkSettings {
    fn default() -> Self {
        LinkSettings { enabled: true, disabled_peers: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: u32,
    /// Folders to index. `~` is the home folder. Extra drives (`/mnt/Data`) are opt-in here.
    pub roots: Vec<String>,
    /// Names skipped wherever they appear (`*` and `?` wildcards allowed).
    pub exclude_names: Vec<String>,
    /// Absolute folders skipped with everything below them.
    pub exclude_paths: Vec<String>,
    /// Skip NFS, SMB/CIFS, SSHFS and other network mounts.
    pub skip_network_mounts: bool,
    /// Show hidden files without pressing the toggle.
    pub show_hidden: bool,
    pub theme: Theme,
    pub shortcut: String,
    /// `None` until decided (first run); then the user's choice.
    pub start_at_login: Option<bool>,
    /// Full reconciling rescan interval while watchers work, in hours.
    pub rescan_hours: u32,
    /// Allow `/pattern` content search with ripgrep when it's installed.
    pub content_search: bool,
    /// Show recent and pinned items as soon as the overlay opens.
    pub show_recent_on_open: bool,
    pub rows: Rows,
    /// On Hyprland, add the shortcut as a runtime key binding (never written to the config).
    pub hyprland_runtime_bind: bool,
    pub link: LinkSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: SCHEMA,
            roots: vec!["~".into()],
            exclude_names: default_exclude_names(),
            exclude_paths: default_exclude_paths(),
            skip_network_mounts: true,
            show_hidden: false,
            theme: Theme::System,
            shortcut: DEFAULT_SHORTCUT.into(),
            start_at_login: None,
            rescan_hours: 6,
            content_search: true,
            show_recent_on_open: false,
            rows: Rows::default(),
            hyprland_runtime_bind: true,
            link: LinkSettings::default(),
        }
    }
}

/// What happened while loading settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadNote {
    Fresh,
    Loaded,
    Migrated(u32),
    Recovered(PathBuf),
}

impl Settings {
    pub fn load(path: &Path) -> (Settings, LoadNote) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return (Settings::default(), LoadNote::Fresh),
        };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) if v.is_object() => {
                let schema = v.get("schema").and_then(|s| s.as_u64()).unwrap_or(0) as u32;
                let v = migrate(v, schema);
                match serde_json::from_value::<Settings>(v) {
                    Ok(mut s) => {
                        s.sanitize();
                        if schema < SCHEMA {
                            s.schema = SCHEMA;
                            let _ = s.save(path);
                            (s, LoadNote::Migrated(schema))
                        } else {
                            (s, LoadNote::Loaded)
                        }
                    }
                    Err(_) => (Settings::default(), LoadNote::Recovered(keep_bad(path))),
                }
            }
            _ => (Settings::default(), LoadNote::Recovered(keep_bad(path))),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut s = self.clone();
        s.schema = SCHEMA;
        let mut text = serde_json::to_string_pretty(&s).map_err(std::io::Error::other)?;
        text.push('\n');
        write_atomic(path, text.as_bytes())
    }

    /// Clamp values a hand-edited file could make unusable.
    pub fn sanitize(&mut self) {
        self.rescan_hours = self.rescan_hours.clamp(1, 24 * 7);
        self.rows.visible = self.rows.visible.clamp(1, 20);
        self.rows.stretch = self.rows.stretch.clamp(self.rows.visible, 30);
        self.roots.retain(|r| !r.trim().is_empty());
        self.roots.dedup();
        if self.shortcut.trim().is_empty() {
            self.shortcut = DEFAULT_SHORTCUT.into();
        }
    }

    /// Absolute root folders, with nested and duplicate roots removed.
    pub fn root_paths(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = self.roots.iter().map(|r| normalize(&expand_tilde(r))).filter(|p| p.is_absolute()).collect();
        roots.sort_by_key(|p| p.as_os_str().len());
        let mut out: Vec<PathBuf> = Vec::new();
        for r in roots {
            if !out.iter().any(|o| r.starts_with(o)) {
                out.push(r);
            }
        }
        out
    }

    /// A hash of everything that changes what gets indexed.
    pub fn index_fingerprint(&self) -> u64 {
        let mut h = crate::index::KEY_SEED;
        for r in self.root_paths() {
            h = crate::index::key_extend(h, r.as_os_str().as_encoded_bytes());
            h = crate::index::key_extend(h, b"\0");
        }
        for n in &self.exclude_names {
            h = crate::index::key_extend(h, n.as_bytes());
            h = crate::index::key_extend(h, b"\x01");
        }
        for n in &self.exclude_paths {
            h = crate::index::key_extend(h, n.as_bytes());
            h = crate::index::key_extend(h, b"\x02");
        }
        crate::index::key_extend(h, &[self.skip_network_mounts as u8])
    }
}

/// Removes `.` and `..` components lexically (roots are user-typed).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn migrate(v: serde_json::Value, _from: u32) -> serde_json::Value {
    // Schema 0 (no field) and 1 share a shape; later schemas add steps here.
    v
}

fn keep_bad(path: &Path) -> PathBuf {
    let bad = path.with_extension(format!("bad-{}.json", crate::now_secs()));
    let _ = std::fs::rename(path, &bad);
    bad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_roundtrip() {
        let dir = crate::test_dir("settings");
        let p = dir.join("settings.json");
        let (s, note) = Settings::load(&p);
        assert_eq!(note, LoadNote::Fresh);
        assert_eq!(s.shortcut, "Ctrl+Alt+F");
        assert!(s.exclude_names.contains(&".snapshots".to_string()));
        let mut s2 = s.clone();
        s2.show_hidden = true;
        s2.save(&p).unwrap();
        let (s3, note) = Settings::load(&p);
        assert_eq!(note, LoadNote::Loaded);
        assert!(s3.show_hidden);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn malformed_is_kept_and_defaults_used() {
        let dir = crate::test_dir("settings-bad");
        let p = dir.join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        let (s, note) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        match note {
            LoadNote::Recovered(bad) => assert!(bad.exists()),
            other => panic!("{other:?}"),
        }
        // Wrong types are recovered too.
        std::fs::write(&p, r#"{"schema":1,"roots":"oops"}"#).unwrap();
        assert!(matches!(Settings::load(&p).1, LoadNote::Recovered(_)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn migrates_schema_zero_and_ignores_unknown_fields() {
        let dir = crate::test_dir("settings-migrate");
        let p = dir.join("settings.json");
        std::fs::write(&p, r#"{"showHidden":true,"futureThing":3,"rows":{"visible":0,"stretch":2}}"#).unwrap();
        let (s, note) = Settings::load(&p);
        assert_eq!(note, LoadNote::Migrated(0));
        assert!(s.show_hidden);
        assert_eq!(s.rows.visible, 1);
        assert_eq!(Settings::load(&p).1, LoadNote::Loaded);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rows_viewport() {
        let r = Rows::default();
        assert_eq!(r.viewport(0), 0);
        assert_eq!(r.viewport(3), 3);
        assert_eq!(r.viewport(5), 5);
        assert_eq!(r.viewport(7), 7);
        assert_eq!(r.viewport(8), 5);
        assert_eq!(r.viewport(500), 5);
    }

    #[test]
    fn nested_roots_collapse() {
        use crate::paths::test_abs as abs;
        let s = Settings { roots: vec![abs("/a/b"), abs("/a"), abs("/c/./d/../e"), abs("/a")], ..Settings::default() };
        assert_eq!(s.root_paths(), vec![PathBuf::from(abs("/a")), PathBuf::from(abs("/c/e"))]);
    }
}
