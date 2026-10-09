//! The in-memory index: one compact record per file or folder.
//!
//! Records are stored as a struct of arrays (about 19 bytes per entry plus
//! the name bytes) and refer to their parent folder by id. A parent is always
//! inserted before its children, so `parent[i] < i` holds for every entry;
//! removals mark a subtree deleted in one forward pass and compaction keeps
//! the order. Only folders are hashed by path (for watcher lookups), which
//! keeps the per-file cost to the arrays themselves.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};

/// "No parent": a root entry.
pub const NONE: u32 = u32::MAX;

/// Entry flags.
pub mod flag {
    pub const DIR: u8 = 1;
    /// Hidden itself or inside a hidden folder.
    pub const HIDDEN: u8 = 2;
    pub const SYMLINK: u8 = 4;
    pub const DELETED: u8 = 8;
    pub const ROOT: u8 = 16;
}

/// The metadata stored for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Meta {
    pub size: u64,
    /// Seconds since the Unix epoch (clamped to 0..=u32::MAX).
    pub mtime: i64,
    pub flags: u8,
}

impl Meta {
    pub fn is_dir(&self) -> bool {
        self.flags & flag::DIR != 0
    }
}

/// Sizes up to 2 GiB are exact; larger ones are stored in KiB (exact to 1 KiB, up to 2 TiB... and saturating beyond).
pub fn encode_size(size: u64) -> u32 {
    if size < 0x8000_0000 {
        size as u32
    } else {
        0x8000_0000 | ((size >> 10).min(0x7FFF_FFFF) as u32)
    }
}

pub fn decode_size(v: u32) -> u64 {
    if v & 0x8000_0000 == 0 {
        v as u64
    } else {
        ((v & 0x7FFF_FFFF) as u64) << 10
    }
}

fn encode_time(t: i64) -> u32 {
    t.clamp(0, u32::MAX as i64) as u32
}

/// FNV-1a, used to key folders by path. Streaming: the key of a child is the
/// key of its parent extended with `/` and the child's name.
pub const KEY_SEED: u64 = 0xcbf2_9ce4_8422_2325;

pub fn key_extend(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn key_child(parent: u64, name: &[u8]) -> u64 {
    key_extend(key_extend(parent, b"/"), name)
}

pub fn key_root(path: &Path) -> u64 {
    key_extend(KEY_SEED, path.as_os_str().as_encoded_bytes())
}

/// The key is already a hash; spread it once and use it as is.
#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (v ^ (v >> 29)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    }
    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64)
    }
}

pub type KeyMap<V> = HashMap<u64, V, BuildHasherDefault<KeyHasher>>;
pub type IdSet = HashSet<u32, BuildHasherDefault<KeyHasher>>;
pub type IdMap<V> = HashMap<u32, V, BuildHasherDefault<KeyHasher>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub id: u32,
    pub path: PathBuf,
}

#[derive(Default, Clone)]
pub struct Index {
    pub(crate) parent: Vec<u32>,
    pub(crate) name_off: Vec<u32>,
    pub(crate) name_len: Vec<u16>,
    pub(crate) flags: Vec<u8>,
    pub(crate) size: Vec<u32>,
    pub(crate) mtime: Vec<u32>,
    pub(crate) names: Vec<u8>,
    pub(crate) roots: Vec<Root>,
    dirs: KeyMap<u32>,
    deleted: usize,
    garbage: usize,
    generation: u64,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index").field("len", &self.len()).field("live", &self.live()).field("roots", &self.roots).finish()
    }
}

impl Index {
    pub fn new() -> Index {
        Index::default()
    }

    /// Records ever inserted (including deleted ones not yet compacted).
    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live() == 0
    }

    /// Entries that exist.
    pub fn live(&self) -> usize {
        self.parent.len() - self.deleted
    }

    pub fn deleted(&self) -> usize {
        self.deleted
    }

    /// Bumped by every change; caches compare it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    /// Approximate heap bytes used by the index.
    pub fn heap_bytes(&self) -> usize {
        self.parent.capacity() * 4
            + self.name_off.capacity() * 4
            + self.name_len.capacity() * 2
            + self.flags.capacity()
            + self.size.capacity() * 4
            + self.mtime.capacity() * 4
            + self.names.capacity()
            + self.dirs.capacity() * 13
    }

    pub fn parent(&self, id: u32) -> u32 {
        self.parent[id as usize]
    }

    pub fn flags(&self, id: u32) -> u8 {
        self.flags[id as usize]
    }

    pub fn is_deleted(&self, id: u32) -> bool {
        self.flags[id as usize] & flag::DELETED != 0
    }

    pub fn is_dir(&self, id: u32) -> bool {
        self.flags[id as usize] & flag::DIR != 0
    }

    pub fn name(&self, id: u32) -> &[u8] {
        let off = self.name_off[id as usize] as usize;
        &self.names[off..off + self.name_len[id as usize] as usize]
    }

    pub fn name_os(&self, id: u32) -> &OsStr {
        // SAFETY: names are stored exactly as `as_encoded_bytes` produced them.
        unsafe { OsStr::from_encoded_bytes_unchecked(self.name(id)) }
    }

    pub fn meta(&self, id: u32) -> Meta {
        let i = id as usize;
        Meta { size: decode_size(self.size[i]), mtime: self.mtime[i] as i64, flags: self.flags[i] }
    }

    pub fn size(&self, id: u32) -> u64 {
        decode_size(self.size[id as usize])
    }

    pub fn mtime(&self, id: u32) -> i64 {
        self.mtime[id as usize] as i64
    }

    /// The root this entry belongs to.
    pub fn root_of(&self, mut id: u32) -> Option<&Root> {
        while self.parent[id as usize] != NONE {
            id = self.parent[id as usize];
        }
        self.roots.iter().find(|r| r.id == id)
    }

    /// Depth below its root (a root is 0).
    pub fn depth(&self, mut id: u32) -> u32 {
        let mut d = 0;
        while self.parent[id as usize] != NONE {
            id = self.parent[id as usize];
            d += 1;
        }
        d
    }

    /// The full path of an entry.
    pub fn path(&self, id: u32) -> PathBuf {
        let mut chain = Vec::with_capacity(16);
        let mut cur = id;
        while self.parent[cur as usize] != NONE {
            chain.push(cur);
            cur = self.parent[cur as usize];
        }
        let mut p = match self.roots.iter().find(|r| r.id == cur) {
            Some(r) => r.path.clone(),
            None => PathBuf::from(self.name_os(cur)),
        };
        for &c in chain.iter().rev() {
            p.push(self.name_os(c));
        }
        p
    }

    /// The path key of a folder (or any entry).
    pub fn key(&self, id: u32) -> u64 {
        let mut chain = Vec::with_capacity(16);
        let mut cur = id;
        while self.parent[cur as usize] != NONE {
            chain.push(cur);
            cur = self.parent[cur as usize];
        }
        let mut h = match self.roots.iter().find(|r| r.id == cur) {
            Some(r) => key_root(&r.path),
            None => KEY_SEED,
        };
        for &c in chain.iter().rev() {
            h = key_child(h, self.name(c));
        }
        h
    }

    fn push_raw(&mut self, parent: u32, name: &[u8], meta: Meta) -> u32 {
        let id = self.parent.len() as u32;
        assert!(id < NONE - 1, "index full");
        let name = &name[..name.len().min(u16::MAX as usize)];
        self.parent.push(parent);
        self.name_off.push(self.names.len() as u32);
        self.name_len.push(name.len() as u16);
        self.names.extend_from_slice(name);
        self.flags.push(meta.flags & !flag::DELETED);
        self.size.push(encode_size(meta.size));
        self.mtime.push(encode_time(meta.mtime));
        self.generation += 1;
        id
    }

    /// Adds (or revives) a root folder. Returns its id.
    pub fn add_root(&mut self, path: &Path, meta: Meta) -> u32 {
        if let Some(r) = self.roots.iter().find(|r| r.path == path) {
            if !self.is_deleted(r.id) {
                return r.id;
            }
        }
        self.roots.retain(|r| r.path != path);
        let name = path.file_name().map(|n| n.as_encoded_bytes().to_vec()).unwrap_or_else(|| path.as_os_str().as_encoded_bytes().to_vec());
        let id = self.push_raw(NONE, &name, Meta { flags: meta.flags | flag::DIR | flag::ROOT, ..meta });
        self.roots.push(Root { id, path: path.to_path_buf() });
        self.dirs.insert(key_root(path), id);
        id
    }

    /// Removes a root and everything below it.
    pub fn remove_root(&mut self, path: &Path) {
        if let Some(r) = self.roots.iter().find(|r| r.path == path).cloned() {
            self.remove_many(&[r.id]);
            self.roots.retain(|x| x.path != path);
        }
    }

    /// Appends a child of `parent`. `key` is the child's path key (needed for folders).
    pub fn push(&mut self, parent: u32, name: &[u8], meta: Meta, key: u64) -> u32 {
        let mut meta = meta;
        if self.flags[parent as usize] & flag::HIDDEN != 0 {
            meta.flags |= flag::HIDDEN;
        }
        let id = self.push_raw(parent, name, meta);
        if meta.flags & flag::DIR != 0 {
            self.dirs.insert(key, id);
        }
        id
    }

    pub fn set_meta(&mut self, id: u32, size: u64, mtime: i64) {
        let i = id as usize;
        let (s, t) = (encode_size(size), encode_time(mtime));
        if self.size[i] != s || self.mtime[i] != t {
            self.size[i] = s;
            self.mtime[i] = t;
            self.generation += 1;
        }
    }

    /// The folder with this path key, if indexed and live.
    pub fn dir_by_key(&self, key: u64) -> Option<u32> {
        self.dirs.get(&key).copied().filter(|&id| (id as usize) < self.len() && !self.is_deleted(id) && self.is_dir(id))
    }

    /// The path key for `path` if it is inside one of the roots.
    pub fn key_for_path(&self, path: &Path) -> Option<(u64, u32)> {
        let root = self.roots.iter().filter(|r| path.starts_with(&r.path)).max_by_key(|r| r.path.as_os_str().len())?;
        let rest = path.strip_prefix(&root.path).ok()?;
        let mut h = key_root(&root.path);
        for c in rest.components() {
            match c {
                std::path::Component::Normal(n) => h = key_child(h, n.as_encoded_bytes()),
                std::path::Component::CurDir => {}
                _ => return None,
            }
        }
        Some((h, root.id))
    }

    /// The live folder at `path`.
    pub fn dir_at(&self, path: &Path) -> Option<u32> {
        let (key, _) = self.key_for_path(path)?;
        let id = self.dir_by_key(key)?;
        // Guard against a (vanishingly unlikely) key collision.
        (self.path(id) == path).then_some(id)
    }

    /// The live entry at `path` (folder or file).
    pub fn lookup(&self, path: &Path) -> Option<u32> {
        if let Some(id) = self.dir_at(path) {
            return Some(id);
        }
        let parent = self.dir_at(path.parent()?)?;
        self.find_child(parent, path.file_name()?.as_encoded_bytes())
    }

    /// A live child of `parent` with this exact name (linear scan after `parent`).
    pub fn find_child(&self, parent: u32, name: &[u8]) -> Option<u32> {
        let start = parent as usize + 1;
        (start..self.len())
            .find(|&i| self.parent[i] == parent && self.flags[i] & flag::DELETED == 0 && self.name(i as u32) == name)
            .map(|i| i as u32)
    }

    /// Live children of every folder in `dirs`, in one pass.
    pub fn children_of(&self, dirs: &IdSet) -> IdMap<Vec<u32>> {
        let mut out: IdMap<Vec<u32>> = IdMap::default();
        if dirs.is_empty() {
            return out;
        }
        let start = dirs.iter().min().map_or(0, |&m| m as usize + 1);
        for i in start..self.len() {
            let p = self.parent[i];
            if p != NONE && dirs.contains(&p) && self.flags[i] & flag::DELETED == 0 {
                out.entry(p).or_default().push(i as u32);
            }
        }
        out
    }

    /// Marks entries and everything below them deleted.
    pub fn remove_many(&mut self, ids: &[u32]) {
        let mut min = usize::MAX;
        let mut any_dir = false;
        for &id in ids {
            let i = id as usize;
            if i >= self.len() || self.flags[i] & flag::DELETED != 0 {
                continue;
            }
            self.mark_deleted(i);
            any_dir |= self.flags[i] & flag::DIR != 0;
            min = min.min(i);
        }
        if any_dir && min != usize::MAX {
            for i in min + 1..self.len() {
                let p = self.parent[i];
                if p != NONE && self.flags[i] & flag::DELETED == 0 && self.flags[p as usize] & flag::DELETED != 0 {
                    self.mark_deleted(i);
                }
            }
        }
        if min != usize::MAX {
            self.generation += 1;
        }
    }

    fn mark_deleted(&mut self, i: usize) {
        self.flags[i] |= flag::DELETED;
        self.deleted += 1;
        self.garbage += self.name_len[i] as usize;
    }

    /// Whether compaction is worth it.
    pub fn needs_compaction(&self) -> bool {
        self.deleted > 50_000.max(self.len() / 4) || (self.deleted > 0 && self.deleted == self.len())
    }

    /// Drops deleted entries and rebuilds the folder table. Ids change;
    /// returns old id → new id (`NONE` for dropped entries).
    pub fn compact(&mut self) -> Vec<u32> {
        let n = self.len();
        let mut remap = vec![NONE; n];
        let mut out = Index { roots: Vec::new(), generation: self.generation + 1, ..Index::default() };
        let live = self.live();
        out.parent.reserve_exact(live);
        out.name_off.reserve_exact(live);
        out.name_len.reserve_exact(live);
        out.flags.reserve_exact(live);
        out.size.reserve_exact(live);
        out.mtime.reserve_exact(live);
        out.names.reserve_exact(self.names.len().saturating_sub(self.garbage));
        // Keys of live folders, computed from parents (parents come first).
        let mut keys: IdMap<u64> = IdMap::default();
        for i in 0..n {
            if self.flags[i] & flag::DELETED != 0 {
                continue;
            }
            let p = self.parent[i];
            let np = if p == NONE { NONE } else { remap[p as usize] };
            if p != NONE && np == NONE {
                continue; // orphan of a deleted parent (shouldn't happen)
            }
            let id = out.push_raw(np, self.name(i as u32), Meta { size: 0, mtime: 0, flags: self.flags[i] });
            out.size[id as usize] = self.size[i];
            out.mtime[id as usize] = self.mtime[i];
            remap[i] = id;
            if self.flags[i] & flag::DIR != 0 {
                let key = if p == NONE {
                    match self.roots.iter().find(|r| r.id == i as u32) {
                        Some(r) => key_root(&r.path),
                        None => continue,
                    }
                } else {
                    match keys.get(&np) {
                        Some(&pk) => key_child(pk, self.name(i as u32)),
                        None => continue,
                    }
                };
                keys.insert(id, key);
                out.dirs.insert(key, id);
            }
        }
        out.roots = self
            .roots
            .iter()
            .filter_map(|r| (remap[r.id as usize] != NONE).then(|| Root { id: remap[r.id as usize], path: r.path.clone() }))
            .collect();
        *self = out;
        remap
    }

    /// Rebuilds the folder table (after loading from disk).
    pub(crate) fn rebuild_dirs(&mut self) {
        self.dirs = KeyMap::default();
        let mut keys: IdMap<u64> = IdMap::default();
        self.deleted = 0;
        self.garbage = 0;
        for i in 0..self.len() {
            if self.flags[i] & flag::DELETED != 0 {
                self.deleted += 1;
                self.garbage += self.name_len[i] as usize;
                continue;
            }
            if self.flags[i] & flag::DIR == 0 {
                continue;
            }
            let p = self.parent[i];
            let key = if p == NONE {
                match self.roots.iter().find(|r| r.id == i as u32) {
                    Some(r) => key_root(&r.path),
                    None => continue,
                }
            } else {
                match keys.get(&p) {
                    Some(&pk) => key_child(pk, self.name(i as u32)),
                    None => continue,
                }
            };
            keys.insert(i as u32, key);
            self.dirs.insert(key, i as u32);
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let n = self.len();
        if [self.name_off.len(), self.name_len.len(), self.flags.len(), self.size.len(), self.mtime.len()].iter().any(|&l| l != n) {
            return Err("array lengths differ".into());
        }
        for i in 0..n {
            let p = self.parent[i];
            if p != NONE && p as usize >= i {
                return Err(format!("entry {i} has parent {p} after it"));
            }
            if self.name_off[i] as usize + self.name_len[i] as usize > self.names.len() {
                return Err(format!("entry {i} name out of range"));
            }
            // Search scans names in id order.
            if i > 0 && self.name_off[i] < self.name_off[i - 1] + self.name_len[i - 1] as u32 {
                return Err(format!("entry {i} name out of order"));
            }
        }
        for r in &self.roots {
            if r.id as usize >= n || self.parent[r.id as usize] != NONE {
                return Err("invalid root".into());
            }
        }
        Ok(())
    }

    pub(crate) fn bump(&mut self) {
        self.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Meta {
        Meta { size: 0, mtime: 1, flags: flag::DIR }
    }
    fn file(size: u64) -> Meta {
        Meta { size, mtime: 2, flags: 0 }
    }

    fn sample() -> (Index, u32, u32, u32) {
        let mut ix = Index::new();
        let root = Path::new("/home/u");
        let r = ix.add_root(root, Meta::default());
        let rk = key_root(root);
        let docs = ix.push(r, b"Docs", dir(), key_child(rk, b"Docs"));
        let f = ix.push(docs, b"report.pdf", file(1234), 0);
        (ix, r, docs, f)
    }

    #[test]
    fn paths_and_lookup() {
        let (ix, r, docs, f) = sample();
        assert_eq!(ix.path(f), PathBuf::from("/home/u/Docs/report.pdf"));
        assert_eq!(ix.dir_at(Path::new("/home/u/Docs")), Some(docs));
        assert_eq!(ix.dir_at(Path::new("/home/u")), Some(r));
        assert_eq!(ix.lookup(Path::new("/home/u/Docs/report.pdf")), Some(f));
        assert_eq!(ix.lookup(Path::new("/home/u/Docs/missing")), None);
        assert_eq!(ix.lookup(Path::new("/elsewhere/x")), None);
        assert_eq!(ix.key(docs), key_child(key_root(Path::new("/home/u")), b"Docs"));
        assert_eq!(ix.depth(f), 2);
    }

    #[test]
    fn remove_subtree_and_compact() {
        let (mut ix, r, docs, _f) = sample();
        let other = ix.push(r, b"notes.txt", file(5), 0);
        ix.remove_many(&[docs]);
        assert_eq!(ix.live(), 2);
        assert!(ix.dir_at(Path::new("/home/u/Docs")).is_none());
        ix.compact();
        assert_eq!(ix.len(), 2);
        let n = ix.lookup(Path::new("/home/u/notes.txt")).unwrap();
        assert_eq!(ix.size(n), 5);
        assert!(other >= n);
        ix.validate().unwrap();
    }

    #[test]
    fn size_encoding() {
        for s in [0u64, 1, 4096, 0x7FFF_FFFF] {
            assert_eq!(decode_size(encode_size(s)), s);
        }
        let big = 5 * 1024 * 1024 * 1024u64 + 1000;
        assert_eq!(decode_size(encode_size(big)), big & !1023);
    }

    #[test]
    fn hidden_inherits() {
        let mut ix = Index::new();
        let r = ix.add_root(Path::new("/r"), Meta::default());
        let h = ix.push(r, b".config", Meta { flags: flag::DIR | flag::HIDDEN, ..Meta::default() }, 1);
        let c = ix.push(h, b"x.toml", file(1), 0);
        assert!(ix.flags(c) & flag::HIDDEN != 0);
    }

    #[test]
    fn children_in_one_pass() {
        let (mut ix, r, docs, f) = sample();
        let g = ix.push(docs, b"b.txt", file(1), 0);
        let set: IdSet = [docs, r].into_iter().collect();
        let ch = ix.children_of(&set);
        assert_eq!(ch[&docs], vec![f, g]);
        assert_eq!(ch[&r], vec![docs]);
    }
}
