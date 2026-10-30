//! The settings handoff is an allowlist, independent of the on-disk settings
//! schema. Never serialize the engine, history, login policy or peer state.
//! File operations belong on a Link/CLI worker, not the UI thread.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use arcade_link::{Content, Handoff, LinkError, Locations};
use find_core::frecency::Pins;
use find_core::paths::AppPaths;
use find_core::settings::{Rows, Theme};
use find_core::Settings;
use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_ITEMS: usize = 4096;

/// Home-relative paths travel between profiles. Other absolute paths are
/// explicitly marked and apply only on the originating OS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortablePath {
    pub path: String,
    pub machine_specific: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

impl PortablePath {
    fn export(path: &Path, home: &Path) -> Self {
        if let Ok(rest) = path.strip_prefix(home) {
            let rest = rest.to_string_lossy().replace('\\', "/");
            return Self { path: if rest.is_empty() { "~".into() } else { format!("~/{rest}") }, machine_specific: false, platform: None };
        }
        Self { path: path.to_string_lossy().into(), machine_specific: true, platform: Some(arcade_link::paths::current_platform().into()) }
    }

    fn from_setting(value: &str, home: &Path) -> Self {
        if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
            Self { path: value.replace('\\', "/"), machine_specific: false, platform: None }
        } else {
            Self::export(Path::new(value), home)
        }
    }

    fn validate(&self) -> Result<(), LinkError> {
        if !safe_text(&self.path) {
            return Err(LinkError::denied("invalid portable path"));
        }
        let home_relative = self.path == "~" || self.path.starts_with("~/");
        let absolute = Path::new(&self.path).is_absolute()
            || self.path.starts_with("\\\\")
            || (self.path.len() > 2 && self.path.as_bytes()[1] == b':' && matches!(self.path.as_bytes()[2], b'/' | b'\\'));
        if (!home_relative && !absolute)
            || (home_relative && (self.machine_specific || self.platform.is_some()))
            || self.path.starts_with("~//")
            || self.path.strip_prefix("~/").is_some_and(|p| p.len() > 1 && p.as_bytes()[1] == b':')
            || (!home_relative && (!self.machine_specific || self.platform.is_none()))
            || self.platform.as_deref().is_some_and(|p| !["linux", "windows", "macos"].contains(&p))
            || self.path.split(['/', '\\']).any(|c| c == "..")
        {
            return Err(LinkError::denied("absolute paths must be marked machineSpecific with a platform"));
        }
        Ok(())
    }

    fn usable(&self) -> bool {
        self.platform.as_deref().is_none_or(|p| p == arcade_link::paths::current_platform())
    }

    fn expand(&self, home: &Path) -> PathBuf {
        if self.path == "~" {
            home.into()
        } else if let Some(rest) = self.path.strip_prefix("~/") {
            home.join(rest)
        } else {
            PathBuf::from(&self.path)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableSettings {
    pub schema: u32,
    pub app: String,
    pub version: String,
    pub roots: Vec<PortablePath>,
    pub exclude_names: Vec<String>,
    pub exclude_paths: Vec<PortablePath>,
    pub pins: Vec<PortablePath>,
    pub theme: Theme,
    #[serde(default = "notifications_default")]
    pub notifications: bool,
    pub shortcut: String,
    pub show_hidden: bool,
    pub show_recent_on_open: bool,
    pub skip_network_mounts: bool,
    pub content_search: bool,
    pub rescan_hours: u32,
    pub rows: Rows,
}

fn notifications_default() -> bool {
    true
}

fn safe_text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 32768 && !s.chars().any(char::is_control)
}

impl PortableSettings {
    pub fn export(settings: &Settings, pins: &Pins, home: &Path) -> Self {
        Self {
            schema: 1,
            app: crate::link::ME.into(),
            version: crate::VERSION.into(),
            roots: settings.roots.iter().map(|p| PortablePath::from_setting(p, home)).collect(),
            exclude_names: settings.exclude_names.clone(),
            exclude_paths: settings.exclude_paths.iter().map(|p| PortablePath::from_setting(p, home)).collect(),
            pins: pins.items.iter().map(|p| PortablePath::export(p, home)).collect(),
            theme: settings.theme,
            notifications: settings.notifications,
            shortcut: settings.shortcut.clone(),
            show_hidden: settings.show_hidden,
            show_recent_on_open: settings.show_recent_on_open,
            skip_network_mounts: settings.skip_network_mounts,
            content_search: settings.content_search,
            rescan_hours: settings.rescan_hours,
            rows: settings.rows.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), LinkError> {
        if self.schema != 1
            || self.app != crate::link::ME
            || !safe_text(&self.version)
            || self.roots.is_empty()
            || [self.roots.len(), self.exclude_names.len(), self.exclude_paths.len(), self.pins.len()].iter().any(|&n| n > MAX_ITEMS)
            || !(1..=168).contains(&self.rescan_hours)
            || !(1..=20).contains(&self.rows.visible)
            || !(self.rows.visible..=30).contains(&self.rows.stretch)
            || self.exclude_names.iter().any(|n| !safe_text(n))
        {
            return Err(LinkError::denied("invalid Find settings document"));
        }
        for p in self.roots.iter().chain(&self.exclude_paths).chain(&self.pins) {
            p.validate()?;
        }
        crate::hotkey::validate(&self.shortcut)?;
        Ok(())
    }

    /// Preserve local integration preferences and expand pins against the
    /// receiving home. Paths for another OS are skipped; keep current roots
    /// if none of the imported roots can be used here.
    pub fn apply_to(&self, current: &Settings, home: &Path) -> (Settings, Pins) {
        let mut s = current.clone();
        let roots: Vec<_> = self.roots.iter().filter(|p| p.usable()).map(|p| p.path.clone()).collect();
        if !roots.is_empty() {
            s.roots = roots;
        }
        s.exclude_names = self.exclude_names.clone();
        s.exclude_paths = self.exclude_paths.iter().filter(|p| p.usable()).map(|p| p.path.clone()).collect();
        s.theme = self.theme;
        s.notifications = self.notifications;
        s.shortcut = arcade_link::accelerator::normalize(&self.shortcut).expect("validated shortcut");
        s.show_hidden = self.show_hidden;
        s.show_recent_on_open = self.show_recent_on_open;
        s.skip_network_mounts = self.skip_network_mounts;
        s.content_search = self.content_search;
        s.rescan_hours = self.rescan_hours;
        s.rows = self.rows.clone();
        let pins = Pins { items: self.pins.iter().filter(|p| p.usable()).map(|p| p.expand(home)).collect() };
        (s, pins)
    }
}

pub fn export_file(locations: &Locations, settings: &Settings, pins: &Pins, home: &Path) -> Result<Vec<Content>, LinkError> {
    if pins.items.iter().any(|p| p.to_str().is_none()) {
        return Err(LinkError::denied("A pinned file name cannot travel as JSON text"));
    }
    let document = PortableSettings::export(settings, pins, home);
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(&document).map_err(|e| LinkError::internal(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let handoff = Handoff::create(locations, crate::link::ME).map_err(io_error)?;
    let mut file = handoff.file("arcade-find-settings.json", &bytes).map_err(io_error)?;
    file.kind = "file/any".into();
    handoff.keep();
    Ok(vec![file])
}

pub fn read_import(inputs: &[Content]) -> Result<PortableSettings, LinkError> {
    let [input] = inputs else {
        return Err(LinkError::denied("import requires one settings file"));
    };
    if input.kind != "file/any" || !input.paths.is_empty() {
        return Err(LinkError::denied("import requires a file/any path"));
    }
    let path = input.path.as_deref().map(Path::new).filter(|p| p.is_absolute()).ok_or_else(|| LinkError::denied("import requires an absolute path"))?;
    let file = fs::File::open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(LinkError::denied("import requires a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let doc: PortableSettings = serde_json::from_slice(&bytes).map_err(|e| LinkError::denied(&format!("invalid settings: {e}")))?;
    doc.validate()?;
    Ok(doc)
}

/// Back up raw files before applying either change. On partial failure restore
/// both files; the private backup remains available for manual recovery.
pub fn save_import(paths: &AppPaths, settings: &Settings, pins: &Pins) -> io::Result<PathBuf> {
    let backup = paths.config.join("backups").join(arcade_link::endpoint::new_token()?);
    find_core::paths::ensure_private_dir(&backup)?;
    let mut original = Vec::new();
    for (name, path) in [("settings.json", paths.settings_file()), ("pins.json", paths.pins_file())] {
        let bytes = match fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if let Some(b) = &bytes {
            arcade_link::paths::write_atomic(&backup.join(name), b, true)?;
        }
        original.push((path, bytes));
    }
    let result = settings.save(&paths.settings_file()).and_then(|_| pins.save(&paths.pins_file()));
    if let Err(e) = result {
        for (path, bytes) in original {
            if let Some(bytes) = bytes {
                let _ = arcade_link::paths::write_atomic(&path, &bytes, true);
            } else {
                let _ = fs::remove_file(path);
            }
        }
        return Err(e);
    }
    Ok(backup)
}

pub(crate) fn io_error(e: io::Error) -> LinkError {
    LinkError::unavailable(e.to_string())
}

pub fn restore_backup(paths: &AppPaths, backup: &Path) -> io::Result<()> {
    for (name, path) in [("settings.json", paths.settings_file()), ("pins.json", paths.pins_file())] {
        match fs::read(backup.join(name)) {
            Ok(bytes) => arcade_link::paths::write_atomic(&path, &bytes, true)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            },
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_roundtrip_excludes_local_state_and_rebases_home() {
        let home = std::env::temp_dir().join("portable-source");
        let dest = std::env::temp_dir().join("portable-destination");
        let s = Settings {
            roots: vec![home.join("Docs").to_string_lossy().into(), "~".into()],
            theme: Theme::Dark,
            start_at_login: Some(true),
            ..Settings::default()
        };
        let pins = Pins { items: vec![home.join("Docs/pin.txt")] };
        let doc = PortableSettings::export(&s, &pins, &home);
        doc.validate().unwrap();
        let text = serde_json::to_string(&doc).unwrap();
        for forbidden in ["index", "frecency", "secret", "startAtLogin", "hyprlandRuntimeBind", "disabledPeers"] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
        let parsed: PortableSettings = serde_json::from_str(&text).unwrap();
        let local = Settings { start_at_login: Some(false), ..Settings::default() };
        let (restored, pins) = parsed.apply_to(&local, &dest);
        assert_eq!(restored.roots, ["~/Docs", "~"]);
        assert_eq!(restored.theme, Theme::Dark);
        assert_eq!(restored.start_at_login, Some(false));
        assert_eq!(pins.items, [dest.join("Docs/pin.txt")]);
    }

    #[test]
    fn validation_and_backup_leave_the_index_untouched() {
        let root = std::env::temp_dir().join(format!("find-import-{}", arcade_link::endpoint::new_token().unwrap()));
        let paths = AppPaths::under(&root, "");
        fs::create_dir_all(&paths.data).unwrap();
        fs::write(paths.index_file(), b"index must not change").unwrap();
        let old = Settings::default();
        old.save(&paths.settings_file()).unwrap();
        let mut doc = PortableSettings::export(&old, &Pins::default(), &root);
        doc.shortcut = "Alt+Tab".into();
        assert!(doc.validate().is_err());
        doc.shortcut = "Ctrl+Alt+G".into();
        doc.validate().unwrap();
        let (new, pins) = doc.apply_to(&old, &root);
        let backup = save_import(&paths, &new, &pins).unwrap();
        assert_eq!(Settings::load(&backup.join("settings.json")).0, old);
        assert_eq!(Settings::load(&paths.settings_file()).0, new);
        assert_eq!(fs::read(paths.index_file()).unwrap(), b"index must not change");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_paths_that_escape_the_portable_home() {
        let home = std::env::temp_dir().join("portable-home");
        let mut doc = PortableSettings::export(&Settings::default(), &Pins::default(), &home);
        for path in ["~/../secret", "~//etc/passwd", "~/C:/outside", "relative/file"] {
            doc.roots = vec![PortablePath { path: path.into(), machine_specific: false, platform: None }];
            assert!(doc.validate().is_err(), "{path}");
        }
    }
}
