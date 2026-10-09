//! Exclusion rules: names skipped anywhere, folders skipped with their
//! contents, system pseudo-filesystems, and network mounts.

use std::path::{Path, PathBuf};

use crate::mounts::MountTable;
use crate::paths::expand_tilde;
use crate::settings::{normalize, Settings};

/// Always skipped, whatever the settings say: kernel and device trees.
#[cfg(target_os = "linux")]
const SYSTEM: &[&str] = &["/proc", "/sys", "/dev", "/run"];
#[cfg(target_os = "macos")]
const SYSTEM: &[&str] = &["/dev", "/System/Volumes/Data/.Spotlight-V100", "/.Spotlight-V100", "/.fseventsd"];
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const SYSTEM: &[&str] = &[];

#[derive(Debug, Clone, Default)]
pub struct Excludes {
    exact: Vec<Vec<u8>>,
    globs: Vec<Vec<u8>>,
    paths: Vec<PathBuf>,
    pub skip_network: bool,
    pub mounts: MountTable,
}

impl Excludes {
    pub fn from_settings(s: &Settings) -> Excludes {
        let mut e = Excludes { skip_network: s.skip_network_mounts, ..Default::default() };
        for n in &s.exclude_names {
            let n = n.trim();
            if n.is_empty() {
                continue;
            }
            if n.contains('*') || n.contains('?') {
                e.globs.push(n.as_bytes().to_vec());
            } else {
                e.exact.push(n.as_bytes().to_vec());
            }
        }
        e.paths = s.exclude_paths.iter().map(|p| normalize(&expand_tilde(p))).filter(|p| p.is_absolute()).collect();
        e.paths.extend(SYSTEM.iter().map(PathBuf::from));
        if e.skip_network {
            e.mounts = MountTable::read();
        }
        e
    }

    /// Whether an entry named `name` is skipped by name.
    pub fn name_excluded(&self, name: &[u8]) -> bool {
        self.exact.iter().any(|x| x.as_slice() == name) || self.globs.iter().any(|g| glob_match(g, name))
    }

    /// Whether the folder at `path` is skipped by path. Roots themselves can be
    /// opted in below an excluded path (e.g. a root at `/run/media/u/Disk`).
    pub fn path_excluded(&self, path: &Path, roots: &[PathBuf]) -> bool {
        if roots.iter().any(|r| r == path) {
            return false;
        }
        self.paths.iter().any(|p| path.starts_with(p) && !roots.iter().any(|r| r.starts_with(p) && path.starts_with(r) && r != p))
    }

    /// Whether `path` is the mount point of a network filesystem.
    pub fn network_mount(&self, path: &Path) -> bool {
        self.skip_network && self.mounts.is_network(path)
    }
}

/// `*` and `?` wildcards over bytes, case-sensitive (file systems mostly are).
pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = p;
            mark = t;
            p += 1;
        } else if star != usize::MAX {
            p = star + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Case-insensitive (ASCII) glob, for queries.
pub fn glob_match_ci(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = p;
            mark = t;
            p += 1;
        } else if star != usize::MAX {
            p = star + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_globs() {
        let s = Settings {
            exclude_names: vec![".git".into(), "*.tmp".into(), "cache?".into()],
            skip_network_mounts: false,
            ..Settings::default()
        };
        let e = Excludes::from_settings(&s);
        assert!(e.name_excluded(b".git"));
        assert!(!e.name_excluded(b".github"));
        assert!(e.name_excluded(b"a.tmp"));
        assert!(e.name_excluded(b"cache1"));
        assert!(!e.name_excluded(b"cache12"));
    }

    #[test]
    fn paths_respect_opt_in_roots() {
        let s = Settings { exclude_paths: vec!["/mnt".into()], skip_network_mounts: false, ..Settings::default() };
        let e = Excludes::from_settings(&s);
        let roots = vec![PathBuf::from("/home/u"), PathBuf::from("/mnt/Data")];
        assert!(e.path_excluded(Path::new("/mnt/Other"), &roots));
        assert!(!e.path_excluded(Path::new("/mnt/Data"), &roots));
        assert!(!e.path_excluded(Path::new("/mnt/Data/x"), &roots));
        #[cfg(target_os = "linux")]
        assert!(e.path_excluded(Path::new("/proc/1"), &roots));
    }

    #[test]
    fn glob_ci() {
        assert!(glob_match_ci(b"*.PDF", b"report.pdf"));
        assert!(glob_match_ci(b"r?p*", b"Report"));
        assert!(!glob_match_ci(b"*.pdf", b"report.pdfx"));
    }
}
