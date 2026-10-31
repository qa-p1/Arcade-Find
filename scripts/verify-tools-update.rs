// Copied temporarily into Arcade-tools v0.4.1/tests by verify-release.py.
// This tests the released Tools code; it is never committed to that repo.
mod support;

use arcade_tools_core::{
    catalog::Catalog,
    paths::Paths,
    release::{self, Arch, Channel, Os},
    source::{CheckedRelease, Source},
    updates::{CheckContext, InstalledApp, Scheduler, SystemClock, Trigger, UpdateSettings},
};
use std::{path::PathBuf, sync::atomic::AtomicBool};

#[test]
fn published_find_release_is_selected_and_verified_by_tools_041() {
    let evidence = PathBuf::from(std::env::var_os("FIND_RELEASE_EVIDENCE").unwrap());
    let catalog_bytes = std::fs::read(evidence.join("catalog.json")).unwrap();
    let catalog = Catalog::parse(&catalog_bytes, "0.4.1").unwrap();
    let entry = catalog.app("arcade.find").unwrap();
    assert_eq!(entry.repo, "qa-p1/Arcade-Find");
    let server = support::FakeRelease::new();
    let manifest_bytes = std::fs::read(evidence.join("arcade-release.json")).unwrap();
    let manifest = release::Release::parse(&manifest_bytes).unwrap();
    let mut api: serde_json::Value = serde_json::from_slice(&std::fs::read(evidence.join("release-api.json")).unwrap()).unwrap();
    assert_eq!(api["tag_name"], format!("v{}", manifest.version));
    assert_eq!(api["draft"], false);
    assert_eq!(api["prerelease"], false);
    for asset in api["assets"].as_array_mut().unwrap() {
        let name = asset["name"].as_str().unwrap().to_owned();
        let route = format!("assets/{name}");
        // Preserve the real API response and bytes; replace only the transport
        // URL so Tools consumes those exact files from its normal fake server.
        asset["browser_download_url"] = format!("{}{route}", server.url).into();
        server.routes.lock().unwrap().insert(format!("/{route}"), std::fs::read(evidence.join(name)).unwrap());
    }
    server.routes.lock().unwrap().insert(format!("/repos/{}/releases/latest", entry.repo), serde_json::to_vec(&api).unwrap());
    server.routes.lock().unwrap().insert("/catalog.json".into(), catalog_bytes);
    let installed =
        [InstalledApp { id: "arcade.find".into(), version: "0.4.3".into(), channel: Channel::Stable, selected_channel: Channel::Stable, sha256: None }];
    let settings = UpdateSettings::default();
    for (index, (os, arch)) in [(Os::Linux, Arch::X64), (Os::Windows, Arch::X64), (Os::Macos, Arch::X64), (Os::Macos, Arch::Arm64)].into_iter().enumerate() {
        let source = Source::test_server(&server.url).unwrap();
        let paths = Paths::under(&server.root.path().join(format!("scheduler-{index}")));
        let mut scheduler = Scheduler::new(paths, SystemClock, source.clone()).unwrap();
        let outcome = scheduler
            .run(Trigger::Manual, CheckContext { settings: &settings, catalog: &catalog, installed: &installed, peers: &[], online: true, os, arch })
            .unwrap();
        assert!(outcome.failure.is_none(), "{:?}", outcome.failure);
        let update = &scheduler.state().apps["arcade.find"];
        assert!(update.update_available, "{os:?}/{arch:?}: {update:?}");
        assert_eq!(update.installed_version.as_deref(), Some("0.4.3"));
        let latest = &update.latest["stable"];
        assert!(latest.error.is_none(), "{:?}", latest.error);
        let cache = latest.cache.as_ref().unwrap();
        assert_eq!(cache.release.version, manifest.version);
        let checked = CheckedRelease { release: cache.release.clone(), asset: cache.asset.clone(), url: cache.download_url.clone() };
        let downloaded = source.download(&checked, &server.root.path().join("downloads"), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        release::verify(downloaded.path(), &checked.asset).unwrap();
        println!("Tools 0.4.1: Find 0.4.3 → {} {os:?}/{arch:?}: update_available and {} verified", manifest.version, checked.asset.file);
    }
}
