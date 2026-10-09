//! Mount points and which of them are network filesystems.
//!
//! Linux reads `/proc/self/mountinfo` once per crawl. macOS asks `statfs` for
//! `MNT_LOCAL` and Windows asks `GetDriveTypeW` when a crawl crosses into a
//! new device or drive.

use std::path::{Path, PathBuf};

/// Filesystem types treated as network (or otherwise remote) on Linux.
pub const NETWORK_FS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "ncpfs",
    "afs",
    "9p",
    "ceph",
    "glusterfs",
    "lustre",
    "gfs2",
    "ocfs2",
    "davfs",
    "sshfs",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.davfs2",
    "fuse.gvfsd-fuse",
    "fuse.s3fs",
    "fuse.glusterfs",
    "fuse.cephfs",
    "fuse.smbnetfs",
    "fuse.curlftpfs",
    "fuse.gcsfuse",
    "fuse.goofys",
    "fuse.mergerfs-remote",
    "fuse.juicefs",
    "fuse.onedriver",
    "fuse.google-drive-ocamlfuse",
];

/// Pseudo filesystems never worth indexing.
pub const PSEUDO_FS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "securityfs",
    "debugfs",
    "tracefs",
    "pstore",
    "bpf",
    "mqueue",
    "hugetlbfs",
    "configfs",
    "fusectl",
    "autofs",
    "binfmt_misc",
    "efivarfs",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub path: PathBuf,
    pub fstype: String,
    pub source: String,
}

impl Mount {
    pub fn is_network(&self) -> bool {
        is_network_fs(&self.fstype)
            || self.source.starts_with("//")
            || (self.fstype.starts_with("fuse") && self.source.contains(':') && !self.source.starts_with('/'))
    }
    pub fn is_pseudo(&self) -> bool {
        PSEUDO_FS.contains(&self.fstype.as_str())
    }
}

pub fn is_network_fs(fstype: &str) -> bool {
    NETWORK_FS.contains(&fstype)
}

#[derive(Debug, Clone, Default)]
pub struct MountTable {
    pub mounts: Vec<Mount>,
}

impl MountTable {
    #[cfg(target_os = "linux")]
    pub fn read() -> MountTable {
        std::fs::read_to_string("/proc/self/mountinfo").map(|t| MountTable::parse_mountinfo(&t)).unwrap_or_default()
    }

    #[cfg(not(target_os = "linux"))]
    pub fn read() -> MountTable {
        MountTable::default()
    }

    /// Parses `/proc/self/mountinfo`.
    pub fn parse_mountinfo(text: &str) -> MountTable {
        let mut mounts = Vec::new();
        for line in text.lines() {
            let Some((left, right)) = line.split_once(" - ") else { continue };
            let l: Vec<&str> = left.split(' ').collect();
            let r: Vec<&str> = right.split(' ').collect();
            if l.len() < 5 || r.len() < 2 {
                continue;
            }
            mounts.push(Mount { path: PathBuf::from(unescape(l[4])), fstype: r[0].to_string(), source: unescape(r[1]) });
        }
        MountTable { mounts }
    }

    /// The mount that contains `path` (longest mount point prefix).
    pub fn mount_of(&self, path: &Path) -> Option<&Mount> {
        self.mounts.iter().filter(|m| path.starts_with(&m.path)).max_by_key(|m| m.path.as_os_str().len())
    }

    /// Whether `path` lives on a network (or pseudo) filesystem.
    pub fn is_network(&self, path: &Path) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.mount_of(path).is_some_and(|m| m.is_network() || m.is_pseudo())
        }
        #[cfg(target_os = "macos")]
        {
            let _ = self;
            !macos_is_local(path)
        }
        #[cfg(windows)]
        {
            let _ = self;
            windows_is_remote(path)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (self, path);
            false
        }
    }

    /// Local, real mount points outside `/` that a user may want to add as
    /// roots (`/mnt/Data`, `/run/media/u/Disk`, `/Volumes/X`, `D:\`).
    pub fn suggested_drives(&self) -> Vec<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            let mut v: Vec<PathBuf> = self
                .mounts
                .iter()
                .filter(|m| !m.is_network() && !m.is_pseudo())
                .filter(|m| ["/mnt/", "/media/", "/run/media/", "/data"].iter().any(|p| m.path.to_string_lossy().starts_with(p)))
                .filter(|m| !["tmpfs", "overlay", "squashfs", "iso9660"].contains(&m.fstype.as_str()))
                .map(|m| m.path.clone())
                .collect();
            v.sort();
            v.dedup();
            v
        }
        #[cfg(target_os = "macos")]
        {
            let _ = self;
            std::fs::read_dir("/Volumes")
                .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && macos_is_local(p) && !p.read_link().is_ok()).collect())
                .unwrap_or_default()
        }
        #[cfg(windows)]
        {
            let _ = self;
            (b'C'..=b'Z')
                .map(|c| PathBuf::from(format!("{}:\\", c as char)))
                .filter(|p| p.exists() && !windows_is_remote(p))
                .filter(|p| !crate::paths::home_dir().starts_with(p))
                .collect()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = self;
            Vec::new()
        }
    }
}

/// mountinfo escapes space, tab, newline and backslash as octal.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "macos")]
fn macos_is_local(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return true };
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid C string and out pointer.
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return true;
    }
    st.f_flags & (libc::MNT_LOCAL as u32) != 0
}

#[cfg(windows)]
fn windows_is_remote(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let s = path.to_string_lossy();
    if s.starts_with("\\\\") && !s.starts_with("\\\\?\\") {
        return true; // UNC share
    }
    let Some(drive) = s.get(..2).filter(|d| d.ends_with(':')) else { return false };
    let wide: Vec<u16> = std::ffi::OsStr::new(&format!("{drive}\\")).encode_wide().chain([0]).collect();
    // SAFETY: NUL-terminated wide string.
    let t = unsafe { GetDriveTypeW(wide.as_ptr()) };
    t == 4 // DRIVE_REMOTE
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/nvme0n1p2 rw,ssd
23 22 0:22 / /proc rw,nosuid shared:5 - proc proc rw
40 22 0:40 / /mnt/Data rw,relatime shared:20 - ext4 /dev/sdb1 rw
41 22 0:41 / /mnt/nas rw,relatime shared:21 - nfs4 nas:/export rw
42 22 0:42 / /home/u/remote\\040box rw - fuse.sshfs u@host:/ rw
43 22 0:43 / /home/u/.snapshots rw - btrfs /dev/nvme0n1p2 rw
44 22 0:44 / /mnt/share rw - cifs //server/share rw
";

    #[test]
    fn parses_mountinfo_and_flags_network() {
        let t = MountTable::parse_mountinfo(SAMPLE);
        assert_eq!(t.mounts.len(), 7);
        let by = |p: &str| t.mount_of(Path::new(p)).unwrap().clone();
        assert!(!by("/mnt/Data/x").is_network());
        assert!(by("/mnt/nas/x").is_network());
        assert!(by("/home/u/remote box/f").is_network());
        assert!(by("/mnt/share").is_network());
        assert!(by("/proc/1").is_pseudo());
        assert_eq!(by("/home/u/doc").path, PathBuf::from("/"));
        #[cfg(target_os = "linux")]
        {
            assert!(t.is_network(Path::new("/mnt/nas/a")));
            assert!(!t.is_network(Path::new("/home/u")));
            assert_eq!(t.suggested_drives(), vec![PathBuf::from("/mnt/Data")]);
        }
    }

    #[test]
    fn unescapes_octal() {
        assert_eq!(unescape("a\\040b\\134c"), "a b\\c");
        assert_eq!(unescape("plain"), "plain");
    }
}
