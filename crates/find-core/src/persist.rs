//! Saving and loading the index (`index.bin` in the app data folder).
//!
//! Layout (little-endian): magic, format version, a JSON header (roots, the
//! settings fingerprint, timestamps), then the arrays in order, then a CRC32
//! of everything before it. A file that fails any check is ignored and the
//! index is rebuilt by crawling; it is never "repaired" in place.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::index::{Index, Root};

const MAGIC: &[u8; 8] = b"ARCFIND\0";
pub const FORMAT: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub roots: Vec<(u32, PathBuf)>,
    /// Hash of the settings that shape the index (roots, excludes).
    pub fingerprint: u64,
    /// When the last complete crawl or reconcile finished (Unix seconds).
    pub complete_at: i64,
    pub saved_at: i64,
    pub entries: u64,
    pub names: u64,
}

struct CrcWriter<W: Write> {
    inner: W,
    crc: crc32fast::Hasher,
}

impl<W: Write> Write for CrcWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct CrcReader<R: Read> {
    inner: R,
    crc: crc32fast::Hasher,
}

impl<R: Read> Read for CrcReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.crc.update(&buf[..n]);
        Ok(n)
    }
}

fn write_u32s(w: &mut impl Write, v: &[u32]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(64 * 1024);
    for chunk in v.chunks(16 * 1024) {
        buf.clear();
        for x in chunk {
            buf.extend_from_slice(&x.to_le_bytes());
        }
        w.write_all(&buf)?;
    }
    Ok(())
}

fn write_u16s(w: &mut impl Write, v: &[u16]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(64 * 1024);
    for chunk in v.chunks(32 * 1024) {
        buf.clear();
        for x in chunk {
            buf.extend_from_slice(&x.to_le_bytes());
        }
        w.write_all(&buf)?;
    }
    Ok(())
}

fn read_u32s(r: &mut impl Read, n: usize) -> io::Result<Vec<u32>> {
    let mut out = Vec::with_capacity(n);
    let mut buf = vec![0u8; 64 * 1024];
    let mut left = n;
    while left > 0 {
        let take = left.min(buf.len() / 4);
        r.read_exact(&mut buf[..take * 4])?;
        out.extend(buf[..take * 4].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])));
        left -= take;
    }
    Ok(out)
}

fn read_u16s(r: &mut impl Read, n: usize) -> io::Result<Vec<u16>> {
    let mut out = Vec::with_capacity(n);
    let mut buf = vec![0u8; 64 * 1024];
    let mut left = n;
    while left > 0 {
        let take = left.min(buf.len() / 2);
        r.read_exact(&mut buf[..take * 2])?;
        out.extend(buf[..take * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
        left -= take;
    }
    Ok(out)
}

fn read_bytes(r: &mut impl Read, n: usize) -> io::Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    r.read_exact(&mut out)?;
    Ok(out)
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Writes the index atomically (temporary file, fsync, rename).
pub fn save(path: &Path, index: &Index, fingerprint: u64, complete_at: i64) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let result = (|| {
        let file = File::create(&tmp)?;
        let mut w = CrcWriter { inner: BufWriter::with_capacity(256 * 1024, file), crc: crc32fast::Hasher::new() };
        let header = Header {
            roots: index.roots.iter().map(|r| (r.id, r.path.clone())).collect(),
            fingerprint,
            complete_at,
            saved_at: crate::now_secs(),
            entries: index.len() as u64,
            names: index.names.len() as u64,
        };
        let hj = serde_json::to_vec(&header).map_err(|e| invalid(e.to_string()))?;
        w.write_all(MAGIC)?;
        w.write_all(&FORMAT.to_le_bytes())?;
        w.write_all(&(hj.len() as u32).to_le_bytes())?;
        w.write_all(&hj)?;
        write_u32s(&mut w, &index.parent)?;
        write_u32s(&mut w, &index.name_off)?;
        write_u16s(&mut w, &index.name_len)?;
        w.write_all(&index.flags)?;
        write_u32s(&mut w, &index.size)?;
        write_u32s(&mut w, &index.mtime)?;
        w.write_all(&index.names)?;
        let crc = w.crc.clone().finalize();
        let mut inner = w.inner;
        inner.write_all(&crc.to_le_bytes())?;
        let file = inner.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(())
    })();
    match result {
        Ok(()) => std::fs::rename(&tmp, path),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Reads only the header (cheap; for status and one-shot checks).
pub fn read_header(path: &Path) -> io::Result<Header> {
    let mut r = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("not an Arcade Find index"));
    }
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4)?;
    if u32::from_le_bytes(b4) != FORMAT {
        return Err(invalid("index format changed"));
    }
    r.read_exact(&mut b4)?;
    let hl = u32::from_le_bytes(b4) as usize;
    if hl > 64 * 1024 * 1024 {
        return Err(invalid("header too large"));
    }
    let hj = read_bytes(&mut r, hl)?;
    serde_json::from_slice(&hj).map_err(|e| invalid(e.to_string()))
}

/// Loads and verifies an index.
pub fn load(path: &Path) -> io::Result<(Index, Header)> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut r = CrcReader { inner: BufReader::with_capacity(256 * 1024, file), crc: crc32fast::Hasher::new() };
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("not an Arcade Find index"));
    }
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4)?;
    if u32::from_le_bytes(b4) != FORMAT {
        return Err(invalid("index format changed"));
    }
    r.read_exact(&mut b4)?;
    let hl = u32::from_le_bytes(b4) as usize;
    if hl as u64 > file_len {
        return Err(invalid("truncated header"));
    }
    let hj = read_bytes(&mut r, hl)?;
    let header: Header = serde_json::from_slice(&hj).map_err(|e| invalid(e.to_string()))?;
    let n = header.entries as usize;
    let names = header.names as usize;
    // 19 bytes per entry plus names; refuse sizes the file can't hold.
    if (n as u64) * 19 + names as u64 > file_len {
        return Err(invalid("truncated index"));
    }
    let mut ix = Index::new();
    ix.parent = read_u32s(&mut r, n)?;
    ix.name_off = read_u32s(&mut r, n)?;
    ix.name_len = read_u16s(&mut r, n)?;
    ix.flags = read_bytes(&mut r, n)?;
    ix.size = read_u32s(&mut r, n)?;
    ix.mtime = read_u32s(&mut r, n)?;
    ix.names = read_bytes(&mut r, names)?;
    let expected = r.crc.clone().finalize();
    let mut c = [0u8; 4];
    r.inner.read_exact(&mut c)?;
    if u32::from_le_bytes(c) != expected {
        return Err(invalid("checksum mismatch"));
    }
    ix.roots = header.roots.iter().map(|(id, p)| Root { id: *id, path: p.clone() }).collect();
    ix.validate().map_err(invalid)?;
    ix.rebuild_dirs();
    ix.bump();
    Ok((ix, header))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{flag, key_child, key_root, Meta};

    #[test]
    fn roundtrip_and_corruption() {
        let dir = crate::test_dir("persist");
        let mut ix = Index::new();
        let root = Path::new("/data/root");
        let r = ix.add_root(root, Meta::default());
        let d = ix.push(r, "Ünïcode dir".as_bytes(), Meta { flags: flag::DIR, mtime: 5, size: 0 }, key_child(key_root(root), "Ünïcode dir".as_bytes()));
        for i in 0..1000 {
            ix.push(d, format!("file-{i}.txt").as_bytes(), Meta { size: i, mtime: 100 + i as i64, flags: 0 }, 0);
        }
        ix.remove_many(&[5]);
        let p = dir.join("index.bin");
        save(&p, &ix, 42, 7).unwrap();
        let (back, h) = load(&p).unwrap();
        assert_eq!(h.fingerprint, 42);
        assert_eq!(h.complete_at, 7);
        assert_eq!(back.len(), ix.len());
        assert_eq!(back.live(), ix.live());
        assert_eq!(back.dir_at(&root.join("Ünïcode dir")), Some(d));
        assert_eq!(back.lookup(&root.join("Ünïcode dir/file-999.txt")).map(|i| back.size(i)), Some(999));
        assert_eq!(read_header(&p).unwrap().entries, ix.len() as u64);

        // Flip one byte in the body: the checksum must catch it.
        let mut bytes = std::fs::read(&p).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        std::fs::write(&p, &bytes).unwrap();
        assert!(load(&p).is_err());

        // Truncated file.
        std::fs::write(&p, &bytes[..bytes.len() / 3]).unwrap();
        assert!(load(&p).is_err());
        std::fs::write(&p, b"garbage").unwrap();
        assert!(load(&p).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
