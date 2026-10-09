//! `arcade-find`: the command line. Without arguments it opens the overlay
//! (starting the resident instance if needed); a second launch hands its
//! command to the running instance and exits.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use arcade_find::instance::{self, Command};
use arcade_find::service::{Service, UiMsg};
use arcade_find::{autostart, hotkey, link, theme, VERSION};
use find_core::paths::AppPaths;
use find_core::Settings;

const HELP: &str = "Arcade Find: press a shortcut, type, and any file or folder shows up.

Usage: arcade-find [OPTION]

  (no option)          Open the search overlay
  --show [QUERY]       Open the overlay, optionally with a query
  --toggle             Open or close the overlay (bind this to a shortcut)
  --hide               Close the overlay
  --background         Start without showing anything (login)
  --settings           Open Settings
  --search QUERY       Print matching paths (--limit N, --hidden, --json)
  --status             Print the running instance's status as JSON
  --rescan             Rescan all indexed folders
  --restart            Restart the running instance
  --quit               Quit the running instance
  --version            Print the version
  --arcade-manifest    Print the Arcade Link manifest
  --arcade-invoke      Serve one Arcade Link request from stdin

Environment: ARCADE_FIND_HOME (isolated data root), ARCADE_FIND_PROFILE,
ARCADE_HOME (Arcade Link locations).";

#[derive(Debug, Default)]
struct Args {
    action: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
    hidden: bool,
    json: bool,
    successor_of: Option<u32>,
    dir: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--limit" => a.limit = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--limit needs a number")?),
            "--hidden" => a.hidden = true,
            "--json" => a.json = true,
            "--successor-of" => a.successor_of = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--successor-of needs a process id")?),
            "--show" | "--search" => {
                a.action = Some(arg.clone());
                if it.peek().is_some_and(|n| !n.starts_with("--")) {
                    a.query = it.next();
                }
            }
            "--snapshot" => {
                a.action = Some(arg.clone());
                a.dir = it.next().map(PathBuf::from);
            }
            s if s.starts_with("--") => {
                if a.action.is_some() && arg != "--background" {
                    return Err(format!("only one action at a time ({s})"));
                }
                if a.action.is_none() || arg != "--background" {
                    a.action = Some(arg.clone());
                }
            }
            other => return Err(format!("unexpected argument \"{other}\" (see --help)")),
        }
    }
    Ok(a)
}

/// Settings for read-only uses (no recovery rename, no writes).
fn settings_readonly(p: &AppPaths) -> Settings {
    std::fs::read(p.settings_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn print_search(v: &serde_json::Value, json: bool) -> ExitCode {
    if json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    if v["ok"] != true {
        eprintln!("arcade-find: {}", v["error"].as_str().unwrap_or("search failed"));
        return ExitCode::FAILURE;
    }
    for r in v["results"].as_array().into_iter().flatten() {
        println!("{}", r["path"].as_str().unwrap_or_default());
    }
    ExitCode::SUCCESS
}

/// `--search` without a running instance: the saved index, read-only.
fn search_offline(p: &AppPaths, query: &str, limit: usize, hidden: bool) -> serde_json::Value {
    use serde_json::json;
    let q = find_core::Query::parse_at(query, find_core::now_secs(), find_core::local_offset_secs());
    if q.content.is_some() {
        return json!({"ok": false, "error": "Content search isn't available from the command line"});
    }
    if q.is_empty() {
        return json!({"ok": false, "error": q.errors.first().cloned().unwrap_or_else(|| "Empty query".into())});
    }
    let Ok((index, _)) = find_core::persist::load(&p.index_file()) else {
        return json!({"ok": false, "error": "Arcade Find hasn't indexed your files yet. Run arcade-find once to start indexing."});
    };
    let engine = find_core::Engine::offline(index, settings_readonly(p));
    let opts = find_core::SearchOptions { limit, show_hidden: hidden, now: find_core::now_secs(), ..Default::default() };
    let (hits, r) = engine.search_hits(&q, &opts);
    json!({ "ok": true, "matched": r.matched, "elapsedMs": r.elapsed.as_secs_f64() * 1000.0, "offline": true,
            "results": hits.iter().map(|h| json!({"path": h.path.to_string_lossy(), "isDir": h.is_dir, "size": h.size, "modified": h.mtime})).collect::<Vec<_>>() })
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("arcade-find: {e}");
            return ExitCode::from(2);
        }
    };
    let paths = AppPaths::discover();
    let action = args.action.clone().unwrap_or_default();
    match action.as_str() {
        "--help" | "-h" => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        "--version" => {
            println!("arcade-find {VERSION}");
            return ExitCode::SUCCESS;
        }
        "--arcade-manifest" => {
            println!("{}", link::manifest(&settings_readonly(&paths)).to_json());
            return ExitCode::SUCCESS;
        }
        "--arcade-invoke" => return ExitCode::from(link::serve_oneshot(paths) as u8),
        "--settings-window" => return arcade_find::settings_ui::run(paths),
        "--snapshot" => return arcade_find::snapshot::run(args.dir.unwrap_or_else(|| PathBuf::from("snapshots"))),
        _ => {}
    }

    // Commands for the running instance.
    let cmd = match action.as_str() {
        "" => Some(Command::Show { query: None, reveal: None }),
        "--show" => Some(Command::Show { query: args.query.clone(), reveal: None }),
        "--toggle" => Some(Command::Toggle),
        "--hide" => Some(Command::Hide),
        "--settings" => Some(Command::Settings),
        "--quit" => Some(Command::Quit),
        "--restart" => Some(Command::Restart),
        "--status" => Some(Command::Status),
        "--rescan" => Some(Command::Rescan),
        "--reload" => Some(Command::Reload),
        "--search" => Some(Command::Search { query: args.query.clone().unwrap_or_default(), limit: args.limit, hidden: Some(args.hidden) }),
        "--background" => None,
        other => {
            eprintln!("arcade-find: unknown option {other} (see --help)");
            return ExitCode::from(2);
        }
    };
    if args.successor_of.is_none() {
        if let Some(c) = &cmd {
            if let Ok(reply) = instance::send(&paths, c, Duration::from_secs(10)) {
                return match c {
                    Command::Search { .. } => print_search(&reply, args.json),
                    Command::Status => {
                        println!("{}", serde_json::to_string_pretty(&reply).unwrap_or_default());
                        ExitCode::SUCCESS
                    }
                    _ if reply["ok"] == true => ExitCode::SUCCESS,
                    _ => {
                        eprintln!("arcade-find: {}", reply["error"].as_str().unwrap_or("the running instance refused the command"));
                        ExitCode::FAILURE
                    }
                };
            }
        } else if instance::running(&paths) {
            return ExitCode::SUCCESS;
        }
    }

    // Nothing is running.
    let initial = match &cmd {
        Some(Command::Search { query, limit, hidden }) => {
            return print_search(&search_offline(&paths, query, limit.unwrap_or(20), hidden.unwrap_or(false)), args.json);
        }
        Some(Command::Status) => {
            println!("{}", serde_json::json!({ "ok": false, "running": false }));
            return ExitCode::from(3);
        }
        Some(Command::Quit | Command::Hide | Command::Rescan | Command::Reload) => {
            eprintln!("arcade-find: Arcade Find isn't running.");
            return ExitCode::FAILURE;
        }
        Some(Command::Show { query, reveal }) => Some(UiMsg::Show { query: query.clone(), reveal: reveal.clone() }),
        Some(Command::Toggle) => Some(UiMsg::Show { query: None, reveal: None }),
        _ => None,
    };
    let open_settings = matches!(cmd, Some(Command::Settings));
    let background = initial.is_none();
    run_resident(paths, background, initial, open_settings, args.successor_of)
}

fn run_resident(paths: AppPaths, background: bool, initial: Option<UiMsg>, open_settings: bool, successor_of: Option<u32>) -> ExitCode {
    let lock = match successor_of {
        Some(_) => instance::acquire_waiting(&paths, Duration::from_secs(15)),
        None => instance::acquire(&paths),
    };
    let lock = match lock {
        Ok(Some(l)) => l,
        Ok(None) => {
            // Another instance won the race: hand it our command.
            if let Some(UiMsg::Show { query, reveal }) = initial {
                let _ = instance::send(&paths, &Command::Show { query, reveal }, Duration::from_secs(5));
            }
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("arcade-find: can't start: {e}");
            return ExitCode::FAILURE;
        }
    };

    // First run of an installed copy: start at login (shown in Settings).
    {
        let (mut s, _) = Settings::load(&paths.settings_file());
        let before = s.start_at_login;
        if let Err(e) = autostart::sync(&paths, &mut s) {
            eprintln!("arcade-find: start at login: {e}");
        }
        if s.start_at_login != before {
            let _ = s.save(&paths.settings_file());
        }
    }

    let svc = Service::start(paths.clone(), background);
    let handler_svc = Arc::downgrade(&svc);
    if let Err(e) = instance::serve(
        &paths,
        &lock,
        Arc::new(move |c| match handler_svc.upgrade() {
            Some(s) => s.command(c),
            None => serde_json::json!({ "ok": false, "error": "shutting down" }),
        }),
    ) {
        eprintln!("arcade-find: command channel unavailable: {e}");
    }

    // Light/dark: read once off the UI thread, then follow changes.
    {
        let weak = Arc::downgrade(&svc);
        std::thread::spawn(move || {
            theme::refresh_system_theme();
            if let Some(s) = weak.upgrade() {
                s.send(UiMsg::Theme);
            }
            theme::watch_system_theme(move || {
                theme::refresh_system_theme();
                if let Some(s) = weak.upgrade() {
                    s.send(UiMsg::Theme);
                }
            });
        });
    }

    let state = hotkey::apply(&svc);
    if let hotkey::State::Failed(e) = &state {
        eprintln!("arcade-find: shortcut: {e}");
    }
    if open_settings {
        svc.open_settings();
    }

    let result = run_ui(&svc, initial);
    hotkey::release();
    svc.shutdown();
    drop(lock);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arcade-find: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn run_ui(svc: &Arc<Service>, initial: Option<UiMsg>) -> Result<(), String> {
    if let Err(e) = arcade_find::tray::start(svc) {
        eprintln!("arcade-find: tray: {e}");
    }
    if std::env::var_os("ARCADE_FIND_BACKEND").is_none_or(|b| b == "wayland") && arcade_find::ui::wayland::available() {
        return arcade_find::ui::wayland::run(svc.clone(), initial);
    }
    arcade_find::ui::desktop::run(svc.clone(), initial, None)
}

#[cfg(not(target_os = "linux"))]
fn run_ui(svc: &Arc<Service>, initial: Option<UiMsg>) -> Result<(), String> {
    let tray_svc = svc.clone();
    let on_start: Box<dyn FnOnce()> = Box::new(move || {
        if let Err(e) = arcade_find::tray::start(&tray_svc) {
            eprintln!("arcade-find: tray: {e}");
        }
    });
    arcade_find::ui::desktop::run(svc.clone(), initial, Some(on_start))
}
