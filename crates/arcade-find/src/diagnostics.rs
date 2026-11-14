//! Small event-only logs: no queries, file names, content or settings payloads.
//!
//! They go to `events.log`, beside the `arcade-find.log` that Link's `capture_stderr` writes and rotates: two writers
//! with two rotation policies on one file moved the live stderr log aside and later deleted it.
use find_core::paths::AppPaths;
use std::io::{self, Write};

const FILE: &str = "events.log";
const MAX_BYTES: u64 = 1024 * 1024;

pub fn event(paths: &AppPaths, event: &str) -> io::Result<()> {
    let dir = paths.log_dir();
    find_core::paths::ensure_private_dir(&dir)?;
    let file = dir.join(FILE);
    if std::fs::metadata(&file).is_ok_and(|m| m.len() >= MAX_BYTES) {
        let _ = std::fs::remove_file(dir.join(format!("{FILE}.2")));
        let _ = std::fs::rename(dir.join(format!("{FILE}.1")), dir.join(format!("{FILE}.2")));
        std::fs::rename(&file, dir.join(format!("{FILE}.1")))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_the_event_log_leaves_the_stderr_log_alone() {
        let root = std::env::temp_dir().join(format!("find-event-log-{}", arcade_link::endpoint::new_token().unwrap()));
        let paths = AppPaths::under(&root, "");
        let dir = paths.log_dir();
        find_core::paths::ensure_private_dir(&dir).unwrap();
        let stderr_log = dir.join(format!("{}.log", crate::identity::CLI_NAME));
        std::fs::write(&stderr_log, "captured stderr\n").unwrap();
        std::fs::write(dir.join(FILE), vec![b'x'; MAX_BYTES as usize]).unwrap();
        event(&paths, "started").unwrap();
        assert_eq!(std::fs::read_to_string(&stderr_log).unwrap(), "captured stderr\n");
        assert!(dir.join(format!("{FILE}.1")).exists());
        assert!(std::fs::read_to_string(dir.join(FILE)).unwrap().ends_with(" started\n"));
        let _ = std::fs::remove_dir_all(root);
    }
}
