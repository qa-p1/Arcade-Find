//! One resident instance per profile.
//!
//! The resident holds an exclusive lock on `<runtime>/instance.lock` and
//! listens on a local socket (Unix domain socket in the private runtime
//! folder; a named pipe on Windows). A second launch sends its command as
//! one JSON line, with the token from `<runtime>/instance.json`, and exits.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::{prelude::*, ListenerOptions, Stream};
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use find_core::paths::{ensure_private_dir, write_atomic, AppPaths};

/// A command for the resident instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Command {
    Ping,
    Show {
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        reveal: Option<PathBuf>,
    },
    Toggle,
    Hide,
    Settings,
    Quit,
    Restart,
    Status,
    Rescan,
    Reload,
    Search {
        query: String,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        hidden: Option<bool>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    token: String,
    #[serde(flatten)]
    command: Command,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstanceInfo {
    pid: u32,
    token: String,
    #[serde(default)]
    version: String,
}

/// Holds the instance lock for the life of the resident process.
pub struct InstanceLock {
    _file: std::fs::File,
    pub token: String,
}

fn lock_path(p: &AppPaths) -> PathBuf {
    p.runtime.join("instance.lock")
}

fn info_path(p: &AppPaths) -> PathBuf {
    p.runtime.join("instance.json")
}

#[cfg(unix)]
fn socket_path(p: &AppPaths) -> std::io::Result<PathBuf> {
    let path = p.runtime.join("instance.sock");
    // sun_path is ~104 bytes: fall back to a short private folder under /tmp.
    let path = if path.as_os_str().len() > 100 {
        let dir = std::env::temp_dir().join(format!("arcade-find-{}", unsafe { libc::getuid() }));
        ensure_private_dir(&dir)?;
        dir.join(format!("{}.sock", p.instance_name()))
    } else {
        path
    };
    Ok(path)
}

#[cfg(unix)]
fn socket_name(p: &AppPaths) -> std::io::Result<interprocess::local_socket::Name<'static>> {
    socket_path(p)?.to_fs_name::<GenericFilePath>().map(|n| n.into_owned())
}

#[cfg(windows)]
fn socket_name(p: &AppPaths) -> std::io::Result<interprocess::local_socket::Name<'static>> {
    p.instance_name().to_ns_name::<GenericNamespaced>().map(|n| n.into_owned())
}


fn new_token() -> String {
    let mut b = [0u8; 32];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Tries to become the resident instance. `None` if another one holds the lock.
pub fn acquire(p: &AppPaths) -> std::io::Result<Option<InstanceLock>> {
    ensure_private_dir(&p.runtime)?;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path(p))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
        Err(std::fs::TryLockError::Error(e)) => return Err(e),
    }
    let token = new_token();
    let info = InstanceInfo { pid: std::process::id(), token: token.clone(), version: crate::VERSION.into() };
    write_atomic(&info_path(p), serde_json::to_string(&info).unwrap_or_default().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(info_path(p), std::fs::Permissions::from_mode(0o600));
    }
    Ok(Some(InstanceLock { _file: file, token }))
}

/// Waits (bounded) for the lock: a restart's successor waiting for the old instance.
pub fn acquire_waiting(p: &AppPaths, timeout: Duration) -> std::io::Result<Option<InstanceLock>> {
    let end = std::time::Instant::now() + timeout;
    loop {
        if let Some(l) = acquire(p)? {
            return Ok(Some(l));
        }
        if std::time::Instant::now() >= end {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub type HandlerFn = Arc<dyn Fn(Command) -> Value + Send + Sync>;

/// Serves commands on the instance socket (one blocking accept thread).
pub fn serve(p: &AppPaths, lock: &InstanceLock, handler: HandlerFn) -> std::io::Result<()> {
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket_path(p)?);
    let name = socket_name(p)?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    let token = lock.token.clone();
    std::thread::Builder::new().name("find-instance".into()).spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let handler = handler.clone();
            let token = token.clone();
            let _ = std::thread::Builder::new().name("find-instance-conn".into()).spawn(move || handle(conn, &token, handler));
        }
    })?;
    Ok(())
}

fn handle(conn: Stream, token: &str, handler: HandlerFn) {
    let _ = conn.set_recv_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(&conn);
    let mut line = String::new();
    // One request per connection, at most 64 KiB.
    let mut limited = std::io::Read::take(&mut reader, 64 * 1024);
    if BufRead::read_line(&mut BufReader::new(&mut limited), &mut line).unwrap_or(0) == 0 {
        return;
    }
    let reply = match serde_json::from_str::<Envelope>(&line) {
        Ok(env) if constant_eq(env.token.as_bytes(), token.as_bytes()) => handler(env.command),
        Ok(_) => json!({ "ok": false, "error": "denied" }),
        Err(e) => json!({ "ok": false, "error": format!("bad request: {e}") }),
    };
    let mut out = serde_json::to_string(&reply).unwrap_or_default();
    out.push('\n');
    let _ = (&conn).write_all(out.as_bytes());
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Sends a command to the running instance. `Err` if none answers.
pub fn send(p: &AppPaths, command: &Command, timeout: Duration) -> std::io::Result<Value> {
    let info: InstanceInfo = serde_json::from_slice(&std::fs::read(info_path(p))?).map_err(std::io::Error::other)?;
    let name = socket_name(p)?;
    let conn = Stream::connect(name)?;
    let _ = conn.set_recv_timeout(Some(timeout));
    let mut line = serde_json::to_string(&Envelope { token: info.token, command: command.clone() }).map_err(std::io::Error::other)?;
    line.push('\n');
    (&conn).write_all(line.as_bytes())?;
    let mut reader = BufReader::new(&conn);
    let mut reply = String::new();
    reader.read_line(&mut reply)?;
    serde_json::from_str(&reply).map_err(std::io::Error::other)
}

/// Is a resident instance running for this profile?
pub fn running(p: &AppPaths) -> bool {
    send(p, &Command::Ping, Duration::from_millis(500)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_roundtrip() {
        let root = std::env::temp_dir().join(format!("af-inst-{}", std::process::id()));
        let p = AppPaths::under(&root, "");
        let lock = acquire(&p).unwrap().expect("first acquire");
        assert!(acquire(&p).unwrap().is_none(), "second acquire must fail");
        serve(
            &p,
            &lock,
            Arc::new(|c| match c {
                Command::Ping => json!({"ok": true, "pong": true}),
                Command::Search { query, .. } => json!({"ok": true, "q": query}),
                _ => json!({"ok": false}),
            }),
        )
        .unwrap();
        assert!(running(&p));
        let r = send(&p, &Command::Search { query: "x y".into(), limit: None, hidden: None }, Duration::from_secs(2)).unwrap();
        assert_eq!(r["q"], "x y");
        // A wrong token is refused.
        let mut info: serde_json::Value = serde_json::from_slice(&std::fs::read(info_path(&p)).unwrap()).unwrap();
        info["token"] = json!("bad");
        std::fs::write(info_path(&p), info.to_string()).unwrap();
        let r = send(&p, &Command::Ping, Duration::from_secs(2)).unwrap();
        assert_eq!(r["error"], "denied");
        drop(lock);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn commands_serialize() {
        let c = Command::Show { query: Some("a".into()), reveal: None };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#"{"cmd":"show","query":"a","reveal":null}"#);
        let back: Command = serde_json::from_str(r#"{"cmd":"show"}"#).unwrap();
        assert_eq!(back, Command::Show { query: None, reveal: None });
    }
}
