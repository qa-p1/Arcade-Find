//! Find as a Link consumer, against a mock peer that publishes the proposed
//! `arcade.shelf` / `shelf.add` contract (docs/SHELF_HANDOFF.md §C.2).
//! The real Shelf doesn't exist yet: these tests pin down Find's side only.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use arcade_link::manifest::write_manifest;
use arcade_link::server::ServerConfig;
use arcade_link::{
    Action, ErrorCode, Handler, InvokeContext, InvokeRequest, InvokeResult, LinkError, Locations, Manifest, PeerInfo, Registry, Reply,
    Server,
};
use serde_json::json;

use arcade_find::link::{self, Item, Offer, Selection};
use arcade_find::service::selection_of;
use arcade_find::ui::model::Row;
use find_core::kind::Kind;

const SHELF: &str = "arcade.shelf";

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("af-link-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn shelf_add() -> Action {
    Action::new("shelf.add", "Add to Shelf", "add")
        .accepts(&["file/*", "file/*[]", "folder/reference", "text/plain", "text/url", "text/rich"])
        .effects(&["persists"])
}

fn shelf_manifest(executable: &str) -> Manifest {
    let mut m = Manifest::new(SHELF, "0.1.0", executable);
    m.name = "Arcade Shelf".into();
    m.launch.background = vec!["--background".into()];
    m.launch.invoke = None;
    m.actions = vec![shelf_add()];
    m
}

/// Records every request and answers with a scripted result.
struct MockShelf {
    seen: Mutex<Vec<InvokeRequest>>,
    fail: Option<LinkError>,
}

impl Handler for MockShelf {
    fn describe(&self) -> Vec<Action> {
        vec![shelf_add()]
    }
    fn invoke(&self, request: InvokeRequest, _ctx: &InvokeContext) -> Result<Reply, LinkError> {
        let n: usize = request.inputs.iter().map(|c| c.all_paths().len().max(c.text.is_some() as usize)).sum();
        self.seen.lock().unwrap().push(request);
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        Ok(Reply::Done(InvokeResult {
            message: Some(format!("Added {n} items to Quick Shelf")),
            outputs: vec![],
            data: Some(json!({ "added": n })),
        }))
    }
}

fn start_mock(loc: &Locations, fail: Option<LinkError>) -> (Server, Arc<MockShelf>) {
    let exe = std::env::current_exe().unwrap();
    write_manifest(loc, &shelf_manifest(exe.to_str().unwrap())).unwrap();
    let mock = Arc::new(MockShelf { seen: Mutex::new(Vec::new()), fail });
    let server =
        Server::start(ServerConfig { app: PeerInfo { id: SHELF.into(), version: "0.1.0".into() }, locations: loc.clone() }, mock.clone())
            .unwrap();
    (server, mock)
}

/// Real files, so sizes and kinds are what the index would report.
fn fixture(root: &Path) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    for (name, dir) in
        [("logo.svg", false), ("brief.pdf", false), ("fonts", true), ("hero.png", false), ("refs", true), ("notes.md", false)]
    {
        let p = root.join(name);
        if dir {
            std::fs::create_dir_all(&p).unwrap();
        } else {
            std::fs::write(&p, b"data").unwrap();
        }
        out.push((p, dir));
    }
    out
}

fn sel(items: &[(PathBuf, bool)]) -> Selection {
    link::selection(items.iter().map(|(p, d)| Item {
        path: p,
        is_dir: *d,
        link_type: Kind::of(&p.file_name().unwrap().to_string_lossy(), *d).link_type(),
        size: (!*d).then_some(4),
    }))
}

fn shelf_offer(loc: &Locations, disabled: &[String], s: &Selection) -> Option<link::PeerOffer> {
    let reg = Registry::load(loc);
    link::peer_offers(&reg, disabled, s, &[]).into_iter().find(|o| o.app == SHELF)
}

#[test]
fn offered_only_when_installed_enabled_and_accepting() {
    let root = temp("offer");
    let loc = Locations::under(&root.join("arcade"));
    let files = fixture(&root);
    let s = sel(&files);

    // Not installed: nothing.
    assert!(shelf_offer(&loc, &[], &s).is_none());

    let exe = std::env::current_exe().unwrap();
    write_manifest(&loc, &shelf_manifest(exe.to_str().unwrap())).unwrap();
    let o = shelf_offer(&loc, &[], &s).expect("offered for a mixed selection");
    assert_eq!(o.title, "Add to Shelf");
    assert_eq!(o.app_name, "Arcade Shelf");
    assert_eq!(o.offer, Offer::Ready);
    assert!(o.persists() && !o.outbound() && !o.opens_ui(), "non-interactive: Find stays open");

    // Switched off in Find's Connected apps.
    assert!(shelf_offer(&loc, &[SHELF.to_string()], &s).is_none());

    // Shelf's own Link switch off.
    let mut off = shelf_manifest(exe.to_str().unwrap());
    off.settings.link_enabled = false;
    write_manifest(&loc, &off).unwrap();
    assert!(shelf_offer(&loc, &[], &s).is_none());

    // An action that doesn't take folders isn't offered for a mixed selection,
    // but is for files alone.
    let mut files_only = shelf_manifest(exe.to_str().unwrap());
    files_only.actions = vec![Action::new("shelf.add", "Add to Shelf", "add").accepts(&["file/*", "file/*[]"]).effects(&["persists"])];
    write_manifest(&loc, &files_only).unwrap();
    assert!(shelf_offer(&loc, &[], &s).is_none());
    let only_files: Vec<_> = files.iter().filter(|f| !f.1).cloned().collect();
    assert!(shelf_offer(&loc, &[], &sel(&only_files)).is_some());

    // A manifest whose executable is gone is ignored (SPEC §3).
    write_manifest(&loc, &shelf_manifest("/nonexistent/arcade-shelf")).unwrap();
    assert!(shelf_offer(&loc, &[], &s).is_none());

    // maxBytes: shown disabled with the standard message.
    let mut small = shelf_manifest(exe.to_str().unwrap());
    small.actions[0].max_bytes = Some(10);
    write_manifest(&loc, &small).unwrap();
    let o = shelf_offer(&loc, &[], &s).unwrap();
    assert_eq!(o.offer, Offer::Disabled("Too large for Arcade Shelf (limit 10 bytes).".into()));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn sends_the_selection_by_reference() {
    let root = temp("send");
    let loc = Locations::under(&root.join("arcade"));
    let files = fixture(&root);
    let (_server, mock) = start_mock(&loc, None);
    let s = sel(&files);
    let reg = Registry::load(&loc);
    let o = link::peer_offers(&reg, &[], &s, &[]).into_iter().find(|o| o.app == SHELF).unwrap();
    let m = reg.get(SHELF).unwrap().clone();
    let req = link::request(&o.action, s.inputs.clone(), o.options.clone());
    let r = link::invoke(&loc, &m, &req, None, None).expect("invoke");
    assert_eq!(r.message.as_deref(), Some("Added 6 items to Quick Shelf"));

    let seen = mock.seen.lock().unwrap();
    let got = &seen[0];
    assert_eq!(got.action, "shelf.add");
    assert_eq!(got.version, Some(1));
    assert_eq!(got.options, json!({}));
    assert_eq!(got.context.source, "arcade.find");
    assert!(got.context.interactive);
    assert_eq!(got.context.reason, "user-click");
    // Files first as one array (selection order), then each folder.
    let p = |n: &str| root.join(n).to_string_lossy().into_owned();
    assert_eq!(got.inputs.len(), 3);
    assert_eq!(got.inputs[0].kind, "file/any[]");
    assert_eq!(got.inputs[0].paths, vec![p("logo.svg"), p("brief.pdf"), p("hero.png"), p("notes.md")]);
    assert_eq!(got.inputs[1].kind, "folder/reference");
    assert_eq!(got.inputs[1].path.as_deref(), Some(p("fonts").as_str()));
    assert_eq!(got.inputs[2].path.as_deref(), Some(p("refs").as_str()));
    // Nothing was copied: every path is the user's original.
    for c in &got.inputs {
        for path in c.all_paths() {
            assert!(path.starts_with(root.to_str().unwrap()));
        }
        assert!(c.owner.is_none(), "Find never creates handoff files for results");
    }
    drop(seen);

    // One file travels as a single typed value.
    let one = sel(&files[1..2]);
    let req = link::request(&o.action, one.inputs.clone(), json!({}));
    link::invoke(&loc, &m, &req, None, None).unwrap();
    let seen = mock.seen.lock().unwrap();
    assert_eq!(seen[1].inputs.len(), 1);
    assert_eq!(seen[1].inputs[0].kind, "file/pdf");
    assert_eq!(seen[1].inputs[0].size, Some(4));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn missing_rows_are_left_out() {
    let root = temp("missing");
    let files = fixture(&root);
    let mut rows: Vec<Row> = files
        .iter()
        .map(|(p, d)| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            Row {
                path: p.clone(),
                parent: "~".into(),
                size: (!*d).then_some(4),
                mtime: 0,
                is_dir: *d,
                is_symlink: false,
                kind: Kind::of(&name, *d),
                name,
                highlights: vec![],
                pinned: false,
                section: None,
                missing: false,
            }
        })
        .collect();
    rows[0].missing = true;
    let s = selection_of(&rows);
    assert_eq!(s.count(), 5);
    assert!(!s.inputs.iter().any(|c| c.all_paths().iter().any(|p| p.ends_with("logo.svg"))));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn peer_errors_use_the_standard_messages() {
    let root = temp("errors");
    let loc = Locations::under(&root.join("arcade"));
    let files = fixture(&root);
    let s = sel(&files);
    let cases = [
        (LinkError::unsupported("nothing usable"), ErrorCode::UnsupportedInput, "Arcade Shelf can't open this kind of content."),
        (LinkError::denied("disabled"), ErrorCode::Denied, "Arcade Shelf has connections to other Arcade apps turned off."),
        (LinkError::busy(), ErrorCode::Busy, "Arcade Shelf is busy. Try again when its current job finishes."),
    ];
    for (err, code, text) in cases {
        let (server, _mock) = start_mock(&loc, Some(err));
        let reg = Registry::load(&loc);
        let m = reg.get(SHELF).unwrap().clone();
        let req = link::request(&shelf_add(), s.inputs.clone(), json!({}));
        let e = link::invoke(&loc, &m, &req, None, None).unwrap_err();
        assert_eq!(e.code, code);
        assert_eq!(e.user_message(&m.name), text);
        server.stop();
    }
    std::fs::remove_dir_all(root).ok();
}

/// A stopped peer without `launch.invoke` is started with `launch.background`
/// (SPEC §7); one whose executable never answers reports `launch_failed`.
#[cfg(unix)]
#[test]
fn stopped_peer_is_launched() {
    let root = temp("launch");
    let loc = Locations::under(&root.join("arcade"));
    let files = fixture(&root);
    let marker = root.join("launched");
    let script = root.join("fake-shelf.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    write_manifest(&loc, &shelf_manifest(script.to_str().unwrap())).unwrap();
    let reg = Registry::load(&loc);
    let m = reg.get(SHELF).unwrap().clone();
    let req = link::request(&shelf_add(), sel(&files).inputs, json!({}));
    let mut launching = 0;
    let mut on_launch = || launching += 1;
    let e = link::invoke(&loc, &m, &req, None, Some(&mut on_launch)).unwrap_err();
    assert_eq!(e.code, ErrorCode::LaunchFailed);
    assert_eq!(e.user_message(&m.name), "Arcade Shelf didn't start.");
    assert_eq!(launching, 1);
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "--background");
    std::fs::remove_dir_all(root).ok();
}
