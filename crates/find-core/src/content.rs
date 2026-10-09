//! Optional content search through ripgrep (`rg`), when the user installed
//! it. Never bundled and never downloaded: Find looks in `PATH`, the usual
//! install folders and Arcade's shared `engines/bin`, checks that the program
//! really is ripgrep, and otherwise reports content search as unavailable.
//!
//! `rg` runs as an executable plus argument array (no shell), lists matching
//! files only (`--files-with-matches`), is bounded in results and time, and
//! is killed on cancel.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ripgrep {
    pub path: PathBuf,
    pub version: String,
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "rg.exe"
    } else {
        "rg"
    }
}

fn candidates(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    #[cfg(unix)]
    {
        let home = crate::paths::home_dir();
        dirs.extend([home.join(".cargo/bin"), home.join(".local/bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin"), PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/opt/local/bin")]);
    }
    #[cfg(windows)]
    {
        let home = crate::paths::home_dir();
        dirs.push(home.join(".cargo\\bin"));
        dirs.push(home.join("scoop\\shims"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(l).join("Microsoft\\WinGet\\Links"));
        }
    }
    dirs.extend(extra.iter().cloned());
    dirs.into_iter().map(|d| d.join(exe_name())).collect()
}

/// Finds ripgrep and checks its identity (`rg --version` says "ripgrep N.N").
/// Blocking (runs a process): call it off the UI thread and cache the result.
pub fn detect(extra_dirs: &[PathBuf]) -> Option<Ripgrep> {
    for c in candidates(extra_dirs) {
        if !c.is_file() {
            continue;
        }
        if let Some(v) = identify(&c) {
            return Some(Ripgrep { path: c, version: v });
        }
    }
    None
}

fn identify(path: &Path) -> Option<String> {
    let mut child = Command::new(path).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    let first = out.lines().next()?.trim();
    let v = first.strip_prefix("ripgrep ")?;
    let version = v.split_whitespace().next()?.to_string();
    let major: u32 = version.split('.').next()?.parse().ok()?;
    // --files-with-matches and --glob have existed since 0.x; require a modern build.
    (major >= 11).then_some(version)
}

#[derive(Debug, Clone)]
pub struct ContentRequest {
    pub pattern: String,
    pub dirs: Vec<PathBuf>,
    pub exts: Vec<String>,
    pub exclude_names: Vec<String>,
    pub hidden: bool,
    pub limit: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentEnd {
    Done,
    Limit,
    Timeout,
    Cancelled,
    Failed(String),
}

/// The arguments for `rg` (no shell; the pattern is passed after `--`).
pub fn args(req: &ContentRequest) -> Vec<String> {
    let mut a: Vec<String> =
        ["--files-with-matches", "--no-messages", "--smart-case", "--fixed-strings", "--max-filesize", "64M", "--no-config", "--no-ignore-vcs"].iter().map(|s| s.to_string()).collect();
    if req.hidden {
        a.push("--hidden".into());
    }
    for n in &req.exclude_names {
        a.push("--glob".into());
        a.push(format!("!{n}"));
    }
    for e in &req.exts {
        a.push("--glob".into());
        a.push(format!("*.{e}"));
    }
    a.push("--".into());
    a.push(req.pattern.clone());
    for d in &req.dirs {
        a.push(d.display().to_string());
    }
    a
}

/// Runs a content search, calling `on_file` for each matching file as it
/// arrives. Blocking; returns how it ended.
pub fn run(rg: &Ripgrep, req: &ContentRequest, cancel: &AtomicBool, mut on_file: impl FnMut(PathBuf)) -> ContentEnd {
    if req.pattern.is_empty() || req.dirs.is_empty() {
        return ContentEnd::Done;
    }
    let mut child = match Command::new(&rg.path).args(args(req)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(e) => return ContentEnd::Failed(e.to_string()),
    };
    let Some(stdout) = child.stdout.take() else { return ContentEnd::Failed("no output".into()) };
    let (tx, rx) = mpsc::channel::<Option<PathBuf>>();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            match r.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                        line.pop();
                    }
                    if line.len() > 8192 {
                        continue;
                    }
                    // SAFETY: rg prints paths as the OS gave them.
                    let p = PathBuf::from(unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(&line) });
                    if tx.send(Some(p)).is_err() {
                        break;
                    }
                }
            }
        }
        let _ = tx.send(None);
    });
    let deadline = Instant::now() + req.timeout;
    let mut count = 0;
    let end = loop {
        if cancel.load(Ordering::Relaxed) {
            break ContentEnd::Cancelled;
        }
        let now = Instant::now();
        if now >= deadline {
            break ContentEnd::Timeout;
        }
        // Wakes at most every 50 ms while a search runs (to see cancel), never when idle.
        match rx.recv_timeout((deadline - now).min(Duration::from_millis(50))) {
            Ok(Some(p)) => {
                on_file(p);
                count += 1;
                if count >= req.limit {
                    break ContentEnd::Limit;
                }
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break ContentEnd::Done,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_safe_arguments() {
        let req = ContentRequest {
            pattern: "--danger; rm -rf".into(),
            dirs: vec![PathBuf::from("/a b")],
            exts: vec!["rs".into()],
            exclude_names: vec![".git".into()],
            hidden: false,
            limit: 10,
            timeout: Duration::from_secs(1),
        };
        let a = args(&req);
        let dash = a.iter().position(|x| x == "--").unwrap();
        assert_eq!(a[dash + 1], "--danger; rm -rf");
        assert_eq!(a[dash + 2], "/a b");
        assert!(a.contains(&"!.git".to_string()));
        assert!(a.contains(&"*.rs".to_string()));
        assert!(!a.contains(&"--hidden".to_string()));
    }

    #[test]
    fn runs_when_installed() {
        let Some(rg) = detect(&[]) else { return };
        let dir = crate::test_dir("content");
        std::fs::write(dir.join("a.txt"), "hello needle world").unwrap();
        std::fs::write(dir.join("b.txt"), "nothing here").unwrap();
        let req = ContentRequest { pattern: "needle".into(), dirs: vec![dir.clone()], exts: vec![], exclude_names: vec![], hidden: false, limit: 10, timeout: Duration::from_secs(10) };
        let mut found = Vec::new();
        let end = run(&rg, &req, &AtomicBool::new(false), |p| found.push(p));
        assert_eq!(end, ContentEnd::Done);
        assert_eq!(found, vec![dir.join("a.txt")]);
        let cancelled = run(&rg, &req, &AtomicBool::new(true), |_| {});
        assert_eq!(cancelled, ContentEnd::Cancelled);
        std::fs::remove_dir_all(dir).ok();
    }
}
