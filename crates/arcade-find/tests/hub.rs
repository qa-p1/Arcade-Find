//! Protocol dispatch and the real resident app, always under explicit roots.
use arcade_find::{link, portable_settings, service::Service};
use arcade_link::manifest::{write_document, ManifestAdditions, ManifestDocument, TrayHostSettings};
use arcade_link::server::ServerConfig;
use arcade_link::trayhost::{TrayHostServer, TrayState};
use arcade_link::{Client, Content, Handler, InvokeContext, InvokeRequest, LinkError, Locations, Manifest, PeerInfo, Reply, Server};
use find_core::frecency::Pins;
use find_core::paths::AppPaths;
use find_core::{Hit, Query, Settings};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("find-hub-{name}-{}", arcade_link::endpoint::new_token().unwrap()));
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn config(root: &Path) -> AppPaths {
    let paths = AppPaths::under(&root.join("find"), "");
    let s = Settings { roots: vec![root.join("files").to_string_lossy().into()], ..Settings::default() };
    std::fs::create_dir_all(root.join("files")).unwrap();
    s.save(&paths.settings_file()).unwrap();
    paths
}
fn peer() -> PeerInfo {
    PeerInfo { id: "arcade.test".into(), version: "0.3.0".into() }
}

struct Controls {
    paths: AppPaths,
    locations: Locations,
    calls: Mutex<Vec<String>>,
}
impl link::Host for Controls {
    fn search(&self, _q: &Query, _limit: usize, _hidden: bool) -> Result<link::SearchOut, LinkError> {
        Ok(link::SearchOut { hits: Vec::<Hit>::new(), matched: 0, elapsed_ms: 0.0, indexing: false })
    }
    fn show(&self, _q: Option<String>, _p: Option<PathBuf>) -> Result<(), LinkError> {
        Ok(())
    }
    fn methods(&self) -> Vec<String> {
        arcade_link::wire::method::OPTIONAL.iter().map(|m| (*m).into()).collect()
    }
    fn settings(&self) -> Result<(), LinkError> {
        self.calls.lock().unwrap().push("settings".into());
        Ok(())
    }
    fn restart_ready(&self, mode: &str) -> Result<(), LinkError> {
        if mode == "force" {
            Ok(())
        } else {
            Err(LinkError::busy())
        }
    }
    fn restart(&self, mode: &str) -> Result<(), LinkError> {
        self.calls.lock().unwrap().push(format!("restart:{mode}"));
        Ok(())
    }
    fn menu_invoke(&self, id: &str) -> Result<(), LinkError> {
        if id != link::RESCAN_MENU_ID {
            return Err(LinkError::denied("unknown menu item"));
        }
        self.calls.lock().unwrap().push(id.into());
        Ok(())
    }
    fn shortcuts_set(&self, id: &str, key: &str) -> Result<arcade_link::app::ShortcutSetResult, LinkError> {
        if id != "show" {
            return Err(LinkError::denied("unknown shortcut"));
        }
        let key = arcade_find::hotkey::validate(key)?;
        let (mut s, _) = Settings::load(&self.paths.settings_file());
        s.shortcut = key;
        s.save(&self.paths.settings_file()).unwrap();
        write_document(&self.locations, &link::document(&s, &self.paths)).unwrap();
        Ok(arcade_link::app::ShortcutSetResult {
            applied: false,
            via: arcade_link::app::ShortcutVia::Manual,
            hint: Some("Bind arcade-find --toggle in Settings → Keyboard".into()),
        })
    }
    fn settings_export(&self) -> Result<Vec<Content>, LinkError> {
        portable_settings::export_file(
            &self.locations,
            &Settings::load(&self.paths.settings_file()).0,
            &Pins::default(),
            &self.paths.config,
        )
    }
    fn settings_import(&self, inputs: &[Content]) -> Result<arcade_link::app::ImportResult, LinkError> {
        let doc = portable_settings::read_import(inputs)?;
        let (s, pins) = doc.apply_to(&Settings::load(&self.paths.settings_file()).0, &self.paths.config);
        portable_settings::save_import(&self.paths, &s, &pins).map_err(|e| LinkError::unavailable(e.to_string()))?;
        Ok(arcade_link::app::ImportResult { imported: true, restart_required: false })
    }
    fn status(&self) -> serde_json::Value {
        serde_json::json!({"tray":"own"})
    }
}

#[test]
fn every_optional_method_dispatches_and_validates_over_ipc() {
    let root = root("methods");
    let paths = config(&root);
    let loc = Locations::under(&root);
    let controls = Arc::new(Controls { paths: paths.clone(), locations: loc.clone(), calls: Mutex::new(Vec::new()) });
    let handler = Arc::new(link::FindHandler { host: controls.clone(), background: true });
    let _server = Server::start(ServerConfig { app: link::me(), locations: loc.clone() }, handler).unwrap();
    let mut client = Client::connect(&loc, link::ME, &peer()).unwrap();
    for method in arcade_link::wire::method::OPTIONAL {
        assert!(client.describe_full().unwrap().supports(method));
    }
    client.settings().unwrap();
    assert_eq!(client.restart(None).unwrap_err().code, arcade_link::ErrorCode::Busy);
    client.restart(Some("force")).unwrap();
    assert_eq!(client.menu().unwrap(), link::menu_items());
    client.menu_invoke("rescan").unwrap();
    assert_eq!(client.menu_invoke("bogus").unwrap_err().code, arcade_link::ErrorCode::Denied);
    for key in ["Ctrl++", "Alt+Tab", "Ctrl+Alt+Delete", "F"] {
        assert_eq!(client.shortcuts_set("show", key).unwrap_err().code, arcade_link::ErrorCode::Denied);
    }
    assert_eq!(client.shortcuts_set("show", "Ctrl+K Ctrl+S").unwrap_err().code, arcade_link::ErrorCode::Unavailable);
    let result = client.shortcuts_set("show", "alt+ctrl+g").unwrap();
    assert_eq!(result.via, arcade_link::app::ShortcutVia::Manual);
    assert!(result.hint.is_some());
    assert_eq!(Settings::load(&paths.settings_file()).0.shortcut, "Ctrl+Alt+G");
    assert_eq!(arcade_link::Registry::load(&loc).get(link::ME).unwrap().shortcuts[0].accelerator, "Ctrl+Alt+G");
    let outputs = client.settings_export().unwrap();
    assert_eq!(outputs[0].kind, "file/any");
    let exported = std::fs::read_to_string(outputs[0].path.as_ref().unwrap()).unwrap();
    assert!(!exported.contains("frecency"));
    assert!(!exported.contains("index"));
    client.shortcuts_set("show", "Ctrl+Alt+H").unwrap();
    let imported = client.settings_import(&outputs).unwrap();
    assert!(imported.imported);
    assert!(!imported.restart_required);
    assert_eq!(Settings::load(&paths.settings_file()).0.shortcut, "Ctrl+Alt+G");
    assert!(paths.config.join("backups").is_dir());
    assert_eq!(client.settings_import(&[]).unwrap_err().code, arcade_link::ErrorCode::Denied);
    assert_eq!(client.status().unwrap()["status"]["tray"], "own");
    // The restart callback runs after its acknowledgment; this round trip
    // waits for that callback before asserting its effects.
    assert!(controls.calls.lock().unwrap().contains(&"restart:force".into()));
    drop(client);
    drop(_server);
    std::fs::remove_dir_all(root).unwrap();
}

struct Tools;
impl Handler for Tools {
    fn describe(&self) -> Vec<arcade_link::Action> {
        vec![]
    }
    fn invoke(&self, _r: InvokeRequest, _c: &InvokeContext) -> Result<Reply, LinkError> {
        Err(LinkError::unavailable("test host"))
    }
}
fn tools(loc: &Locations, host: Option<TrayHostServer>) -> Server {
    let exe = std::env::current_exe().unwrap();
    let doc = ManifestDocument {
        manifest: Manifest::new("arcade.tools", "0.3.0", exe.to_str().unwrap()),
        additions: ManifestAdditions { tray_host: Some(TrayHostSettings { enabled: true, excluded: vec![] }), ..Default::default() },
    };
    let config = ServerConfig { app: PeerInfo { id: "arcade.tools".into(), version: "0.3.0".into() }, locations: loc.clone() };
    let server = match host {
        Some(host) => Server::start_with_tray_host(config, Arc::new(Tools), host),
        None => Server::start(config, Arc::new(Tools)),
    }
    .unwrap();
    write_document(loc, &doc).unwrap();
    server
}
fn next(rx: &mpsc::Receiver<TrayState>, expected: TrayState) {
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), expected);
}

#[test]
fn find_watcher_handles_startup_host_handback_crash_restart_and_link_off() {
    let root = root("tray");
    let paths = config(&root);
    let loc = Locations::under(&root);
    let svc = Service::start_at(paths, true, loc.clone());
    let host = TrayHostServer::new(true);
    let server = tools(&loc, Some(host.clone()));
    let (tx, rx) = mpsc::channel();
    let state_svc = Arc::downgrade(&svc);
    let watcher = arcade_find::tray::watch(&svc, move |state| {
        if let Some(s) = state_svc.upgrade() {
            s.note_tray(state);
        }
        tx.send(state).ok();
    })
    .unwrap();
    next(&rx, TrayState::Hosted);
    assert_eq!(svc.tray_state(), TrayState::Hosted);
    host.set_hosted(false);
    next(&rx, TrayState::Own);
    host.set_hosted(true);
    next(&rx, TrayState::Hosted);
    host.announce_restarting();
    drop(server);
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "restart grace must not flash");
    let host2 = TrayHostServer::new(true);
    let server2 = tools(&loc, Some(host2.clone()));
    // Registry/endpoint replacement reconnects without a second Hosted event.
    let until = Instant::now() + Duration::from_secs(2);
    while host2.subscribers().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!host2.subscribers().is_empty());
    assert!(rx.try_recv().is_err());
    watcher.set_link_enabled(false);
    next(&rx, TrayState::Own);
    watcher.set_link_enabled(true);
    next(&rx, TrayState::Hosted);
    let crashed = Instant::now();
    drop(server2);
    next(&rx, TrayState::Own);
    assert!(crashed.elapsed() < Duration::from_secs(1));
    drop(watcher);
    svc.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_gate_is_bounded_and_link_off_owns_the_icon() {
    let root = root("gate");
    let paths = config(&root);
    let loc = Locations::under(&root);
    let svc = Service::start_at(paths, true, loc.clone());
    let _server = tools(&loc, None);
    let (tx, rx) = mpsc::channel();
    let before = Instant::now();
    let watcher = arcade_find::tray::watch(&svc, move |state| {
        tx.send(state).ok();
    })
    .unwrap();
    next(&rx, TrayState::Own);
    assert!(before.elapsed() < Duration::from_millis(350));
    drop(watcher);
    drop(_server);
    // Link off is immediate even when an enabled Tools host exists.
    let mut settings = svc.settings();
    settings.link.enabled = false;
    settings.save(&svc.paths.settings_file()).unwrap();
    svc.reload_settings();
    let _server = tools(&loc, Some(TrayHostServer::new(true)));
    let (tx, rx) = mpsc::channel();
    let before = Instant::now();
    let watcher = arcade_find::tray::watch(&svc, move |state| {
        tx.send(state).ok();
    })
    .unwrap();
    next(&rx, TrayState::Own);
    assert!(before.elapsed() < Duration::from_millis(300));
    drop(watcher);
    drop(_server);
    svc.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn restart_grace_expires_when_tools_does_not_return() {
    let root = root("grace");
    let paths = config(&root);
    let loc = Locations::under(&root);
    let svc = Service::start_at(paths, true, loc.clone());
    let host = TrayHostServer::new(true);
    let server = tools(&loc, Some(host.clone()));
    let (tx, rx) = mpsc::channel();
    let watcher = arcade_find::tray::watch(&svc, move |state| {
        tx.send(state).ok();
    })
    .unwrap();
    next(&rx, TrayState::Hosted);
    let before = Instant::now();
    host.announce_restarting();
    drop(server);
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err());
    assert_eq!(rx.recv_timeout(Duration::from_secs(6)).unwrap(), TrayState::Own);
    assert!(before.elapsed() >= Duration::from_secs(4));
    drop(watcher);
    svc.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn real_resident_rebinds_with_fake_hyprctl_and_persists_manifest_and_import() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    let root = root("resident");
    let paths = config(&root);
    let loc = Locations::under(&root);
    let fake = root.join("bin");
    std::fs::create_dir_all(&fake).unwrap();
    let hyprctl = fake.join("hyprctl");
    std::fs::write(&hyprctl, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$FAKE_HYPR_LOG\"\nprintf 'ok\\n'\n").unwrap();
    std::fs::set_permissions(&hyprctl, std::fs::Permissions::from_mode(0o755)).unwrap();
    let log = root.join("hyprctl.log");
    let output = std::fs::File::create(root.join("find.log")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_arcade-find"));
    command
        .arg("--background")
        .env("HOME", &root)
        .env("ARCADE_HOME", &root)
        .env("ARCADE_FIND_HOME", root.join("find"))
        .env("XDG_CONFIG_HOME", root.join(".config"))
        .env("XDG_DATA_HOME", root.join(".local/share"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("HYPRLAND_INSTANCE_SIGNATURE", "fake-find-test")
        .env("FAKE_HYPR_LOG", &log)
        .env(
            "PATH",
            std::env::join_paths(std::iter::once(fake).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())))
                .unwrap(),
        )
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}/no-bus", root.display()))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output);
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = ChildGuard(command.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(8);
    let mut client = loop {
        if let Ok(c) = Client::connect(&loc, link::ME, &peer()) {
            break c;
        }
        assert!(Instant::now() < until, "resident did not start: {:?}", child.0.try_wait());
        std::thread::sleep(Duration::from_millis(20));
    };
    let result = client.shortcuts_set("show", "Alt+Ctrl+G").unwrap();
    assert!(result.applied);
    assert_eq!(result.via, arcade_link::app::ShortcutVia::HyprlandRuntime);
    assert_eq!(Settings::load(&paths.settings_file()).0.shortcut, "Ctrl+Alt+G");
    assert_eq!(arcade_link::Registry::load(&loc).get(link::ME).unwrap().shortcuts[0].accelerator, "Ctrl+Alt+G");
    for key in ["Ctrl++", "Alt+Tab", "Ctrl+Alt+Delete"] {
        assert_eq!(client.shortcuts_set("show", key).unwrap_err().code, arcade_link::ErrorCode::Denied);
    }
    assert_eq!(client.shortcuts_set("missing", "Ctrl+Alt+G").unwrap_err().code, arcade_link::ErrorCode::Denied);
    let exported = client.settings_export().unwrap();
    client.shortcuts_set("show", "Ctrl+Alt+H").unwrap();
    assert!(client.settings_import(&exported).unwrap().imported);
    assert_eq!(Settings::load(&paths.settings_file()).0.shortcut, "Ctrl+Alt+G");
    assert!(std::fs::read_to_string(log).unwrap().contains("CTRL + ALT + G"));
    client.menu_invoke("rescan").unwrap();
    let before = client.status().unwrap()["pid"].as_u64().unwrap();
    client.restart(Some("force")).unwrap();
    drop(client);
    let until = Instant::now() + Duration::from_secs(8);
    let mut successor = loop {
        if let Ok(mut c) = Client::connect(&loc, link::ME, &peer()) {
            if c.status().unwrap()["pid"].as_u64().unwrap() != before {
                break c;
            }
        }
        assert!(Instant::now() < until, "successor did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    successor.call(arcade_link::wire::method::APP_QUIT, serde_json::json!({"force":true})).unwrap();
    child.0.wait().unwrap();
    // Successor teardown removes its endpoint; don't remove its profile early.
    let until = Instant::now() + Duration::from_secs(3);
    while loc.endpoint(link::ME).exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::remove_dir_all(root).unwrap();
}
