//! Small event-only logs: no queries, file names, content or settings payloads.
use find_core::paths::AppPaths;
use std::io::{self, Write};

pub fn event(paths: &AppPaths, event: &str) -> io::Result<()> {
    let dir = paths.log_dir();
    find_core::paths::ensure_private_dir(&dir)?;
    let file = dir.join("arcade-find.log");
    if std::fs::metadata(&file).is_ok_and(|m| m.len() >= 1024 * 1024) {
        let _ = std::fs::remove_file(dir.join("arcade-find.log.2"));
        let _ = std::fs::rename(dir.join("arcade-find.log.1"), dir.join("arcade-find.log.2"));
        std::fs::rename(&file, dir.join("arcade-find.log.1"))?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    writeln!(opts.open(file)?, "{} {} {}", arcade_link::manifest::now_rfc3339(), crate::VERSION, event)
}
