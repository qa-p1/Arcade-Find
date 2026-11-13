#![cfg(unix)]
//! The command line as a script sees it: exit codes, JSON errors and limits,
//! with no instance running (the saved index is read-only). Each test uses its
//! own folder under the temp dir; nothing touches the real data.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use find_core::index::{flag, Index, Meta};

struct Env {
    root: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let root = std::env::temp_dir().join(format!("find-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("run")).unwrap();
        let env = Env { root };
        env.save_index(&["alpha.txt", "alpha-report.txt", "notes.md"]);
        env
    }

    fn save_index(&self, names: &[&str]) {
        let files = self.root.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let mut ix = Index::new();
        let r = ix.add_root(&files, Meta { flags: flag::DIR, ..Meta::default() });
        for (i, n) in names.iter().enumerate() {
            ix.push(r, n.as_bytes(), Meta { size: 100 + i as u64, mtime: 1_700_000_000, flags: 0 }, 0);
        }
        find_core::persist::save(&self.root.join("data/index.bin"), &ix, 0, 1).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_arcade-find"))
            .args(args)
            .env("HOME", &self.root)
            .env("ARCADE_FIND_HOME", &self.root)
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn json(o: &Output) -> serde_json::Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&o.stdout)))
}

#[test]
fn a_search_prints_paths_and_exits_zero() {
    let env = Env::new("ok");
    let o = env.run(&["--search", "alpha"]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.lines().any(|l| Path::new(l).ends_with("files/alpha.txt")), "{out}");
}

#[test]
fn json_success_reports_ok_true_and_results() {
    let env = Env::new("json");
    let o = env.run(&["--search", "alpha", "--json"]);
    assert_eq!(code(&o), 0);
    let v = json(&o);
    assert_eq!(v["ok"], true);
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
}

#[test]
fn usage_errors_exit_two() {
    let env = Env::new("usage");
    assert_eq!(code(&env.run(&["--bogus"])), 2);
    assert_eq!(code(&env.run(&["stray"])), 2);
    assert_eq!(code(&env.run(&["--search", "a", "--limit", "abc"])), 2);
    assert_eq!(code(&env.run(&["--search", "a", "--limit", "-1"])), 2);
    assert_eq!(code(&env.run(&["--silent"])), 2);
}

#[test]
fn an_empty_query_fails() {
    let env = Env::new("empty");
    assert_eq!(code(&env.run(&["--search", ""])), 1);
}

#[test]
fn without_an_index_a_search_fails() {
    let env = Env::new("noindex");
    std::fs::remove_file(env.root.join("data/index.bin")).unwrap();
    let o = env.run(&["--search", "alpha"]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stderr).contains("indexed"));
}

#[test]
fn status_without_an_instance_is_json_and_exits_three() {
    let env = Env::new("status");
    let o = env.run(&["--status"]);
    assert_eq!(code(&o), 3);
    let v = json(&o);
    assert_eq!(v["running"], false);
    assert_eq!(v["ok"], false);
}

#[test]
fn commands_for_a_running_instance_fail_when_none_runs() {
    let env = Env::new("norun");
    for c in ["--quit", "--hide", "--rescan", "--reload"] {
        assert_eq!(code(&env.run(&[c])), 1, "{c}");
    }
}

#[test]
fn version_and_help_exit_zero() {
    let env = Env::new("info");
    let v = env.run(&["--version"]);
    assert_eq!(code(&v), 0);
    assert!(String::from_utf8_lossy(&v.stdout).starts_with("arcade-find "));
    assert_eq!(code(&env.run(&["--help"])), 0);
}

/// FD-1: a failed search says `ok:false` and must also exit nonzero.
#[test]
#[should_panic(expected = "KNOWN BUG FD-1")]
fn json_failures_exit_nonzero() {
    let env = Env::new("fd1");
    let o = env.run(&["--search", "size:nope", "--json"]);
    assert_eq!(json(&o)["ok"], false);
    assert_ne!(code(&o), 0, "KNOWN BUG FD-1: --json reported ok:false and exited 0");
}

/// FD-5: an invalid filter next to a valid term is an error, not a silent drop.
#[test]
#[should_panic(expected = "KNOWN BUG FD-5")]
fn an_invalid_filter_next_to_a_valid_term_is_an_error() {
    let env = Env::new("fd5");
    let o = env.run(&["--search", "size:>1GBB ext:txt", "--json"]);
    let v = json(&o);
    assert_eq!(v["ok"], false, "KNOWN BUG FD-5: the malformed size filter was dropped and the search succeeded: {v}");
    assert_ne!(code(&o), 0);
}

/// FD-6: `--limit 0` is not a request for one result.
#[test]
#[should_panic(expected = "KNOWN BUG FD-6")]
fn limit_zero_is_rejected() {
    let env = Env::new("fd6a");
    let o = env.run(&["--search", "alpha", "--limit", "0"]);
    assert_eq!(code(&o), 2, "KNOWN BUG FD-6: --limit 0 exited {} and printed {:?}", code(&o), String::from_utf8_lossy(&o.stdout));
}

/// The offline search (no instance running) honours a limit above 1000; the
/// silent 1000 cap of FD-6 is the running instance's, tested under the harness.
#[test]
fn an_offline_limit_above_a_thousand_is_honoured() {
    let names: Vec<String> = (0..1200).map(|i| format!("cap-{i}.txt")).collect();
    let env = Env::new("fd6b");
    env.save_index(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let o = env.run(&["--search", "cap", "--json", "--limit", "100000"]);
    assert_eq!(code(&o), 0);
    assert_eq!(json(&o)["results"].as_array().unwrap().len(), 1200);
}
