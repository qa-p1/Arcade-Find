//! Local frecency (how often and how recently you opened something) and
//! pinned items. Stored as small JSON files in the app data folder; only
//! paths and timestamps are kept, never contents.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::write_atomic;

/// Keep at most this many items (the least valuable are dropped).
pub const MAX_ITEMS: usize = 2000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Visit {
    pub count: u32,
    /// Recent open times (Unix seconds), newest last, at most 10.
    pub times: Vec<i64>,
}

impl Visit {
    pub fn last(&self) -> i64 {
        self.times.last().copied().unwrap_or(0)
    }

    /// Firefox-style frecency: recent visits weigh more, scaled by the total count.
    pub fn score(&self, now: i64) -> f32 {
        if self.times.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0f32;
        for &t in &self.times {
            let age_days = ((now - t).max(0) as f32) / 86_400.0;
            sum += if age_days < 4.0 {
                100.0
            } else if age_days < 14.0 {
                70.0
            } else if age_days < 31.0 {
                50.0
            } else if age_days < 90.0 {
                30.0
            } else {
                10.0
            };
        }
        sum * self.count as f32 / self.times.len() as f32
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrecencyFile {
    schema: u32,
    items: HashMap<PathBuf, Visit>,
}

#[derive(Debug, Clone, Default)]
pub struct Frecency {
    items: HashMap<PathBuf, Visit>,
    /// Bumped on every change, so callers can cache derived data.
    pub generation: u64,
}

impl Frecency {
    pub fn load(path: &Path) -> Frecency {
        let items =
            std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<FrecencyFile>(&b).ok()).map(|f| f.items).unwrap_or_default();
        Frecency { items, generation: 1 }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let f = FrecencyFile { schema: 1, items: self.items.clone() };
        write_atomic(path, &serde_json::to_vec(&f).map_err(std::io::Error::other)?)
    }

    pub fn record(&mut self, path: &Path, now: i64) {
        let v = self.items.entry(path.to_path_buf()).or_default();
        v.count = v.count.saturating_add(1);
        v.times.push(now);
        if v.times.len() > 10 {
            v.times.remove(0);
        }
        self.generation += 1;
        if self.items.len() > MAX_ITEMS {
            let mut scored: Vec<(PathBuf, f32)> = self.items.iter().map(|(p, v)| (p.clone(), v.score(now))).collect();
            scored.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (p, _) in scored.into_iter().take(self.items.len() - MAX_ITEMS) {
                self.items.remove(&p);
            }
        }
    }

    pub fn forget(&mut self, path: &Path) {
        if self.items.remove(path).is_some() {
            self.generation += 1;
        }
    }

    /// Moves history along with a rename.
    pub fn rename(&mut self, from: &Path, to: &Path) {
        if let Some(v) = self.items.remove(from) {
            self.items.insert(to.to_path_buf(), v);
            self.generation += 1;
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.generation += 1;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, path: &Path) -> Option<&Visit> {
        self.items.get(path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &Visit)> {
        self.items.iter()
    }

    /// Most recently opened first.
    pub fn recent(&self, limit: usize) -> Vec<PathBuf> {
        let mut v: Vec<(&PathBuf, i64)> = self.items.iter().map(|(p, v)| (p, v.last())).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v.into_iter().take(limit).map(|(p, _)| p.clone()).collect()
    }

    /// Ranking boost for a frecency score (0..=250).
    pub fn boost(score: f32) -> i32 {
        if score <= 0.0 {
            0
        } else {
            ((score + 1.0).ln() * 38.0).min(250.0) as i32
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Pins {
    pub items: Vec<PathBuf>,
}

impl Pins {
    pub fn load(path: &Path) -> Pins {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    pub fn contains(&self, p: &Path) -> bool {
        self.items.iter().any(|x| x == p)
    }

    /// Pins or unpins; returns whether it is now pinned.
    pub fn toggle(&mut self, p: &Path) -> bool {
        if let Some(i) = self.items.iter().position(|x| x == p) {
            self.items.remove(i);
            false
        } else {
            self.items.push(p.to_path_buf());
            true
        }
    }

    pub fn rename(&mut self, from: &Path, to: &Path) {
        for x in &mut self.items {
            if x == from {
                *x = to.to_path_buf();
            }
        }
    }

    pub fn remove(&mut self, p: &Path) {
        self.items.retain(|x| x != p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_and_count_raise_score() {
        let now = 10_000_000;
        let mut f = Frecency::default();
        f.record(Path::new("/a"), now - 100 * 86_400);
        f.record(Path::new("/b"), now - 3600);
        f.record(Path::new("/c"), now - 3600);
        f.record(Path::new("/c"), now - 1800);
        let s = |p: &str| f.get(Path::new(p)).unwrap().score(now);
        assert!(s("/b") > s("/a"));
        assert!(s("/c") > s("/b"));
        assert_eq!(f.recent(2), vec![PathBuf::from("/c"), PathBuf::from("/b")]);
        assert!(Frecency::boost(s("/c")) > Frecency::boost(s("/a")));
        assert_eq!(Frecency::boost(0.0), 0);
    }

    #[test]
    fn persist_rename_and_pins() {
        let dir = crate::test_dir("frecency");
        let p = dir.join("f.json");
        let mut f = Frecency::default();
        f.record(Path::new("/x/a.txt"), 5);
        f.rename(Path::new("/x/a.txt"), Path::new("/x/b.txt"));
        f.save(&p).unwrap();
        let g = Frecency::load(&p);
        assert!(g.get(Path::new("/x/b.txt")).is_some());
        assert!(g.get(Path::new("/x/a.txt")).is_none());
        // Garbage files load as empty.
        std::fs::write(&p, "nope").unwrap();
        assert!(Frecency::load(&p).is_empty());

        let mut pins = Pins::default();
        assert!(pins.toggle(Path::new("/p")));
        assert!(pins.contains(Path::new("/p")));
        pins.save(&dir.join("pins.json")).unwrap();
        assert_eq!(Pins::load(&dir.join("pins.json")), pins);
        assert!(!pins.toggle(Path::new("/p")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn bounded() {
        let mut f = Frecency::default();
        for i in 0..(MAX_ITEMS + 50) {
            f.record(&PathBuf::from(format!("/f{i}")), i as i64);
        }
        assert_eq!(f.len(), MAX_ITEMS);
    }
}
