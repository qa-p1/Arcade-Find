//! Measure only added critical-path work; registry/IPC settling is off-thread.
//! Supply ARCADE_HOME/HOME from an isolated runner, never a real profile.
use arcade_link::install::{self, Environment, Runtime};
use arcade_link::trayhost::{TrayHostWatcher, WatcherConfig};
use std::time::Instant;

fn main() {
    assert!(std::env::var_os("ARCADE_HOME").is_some(), "run under an isolated ARCADE_HOME");
    let environment = Environment::discover().unwrap();
    let runtime = Runtime::discover().unwrap();
    measure("dev/no-receipt", &environment, &runtime);
    // Include the installed normal-start case: one small receipt, a stable
    // AppImage and its current mount. This writes only the isolated root.
    let image = environment.applications.join("Arcade-Find.AppImage");
    std::fs::create_dir_all(&environment.applications).unwrap();
    std::fs::write(&image, b"benchmark image").unwrap();
    let appdir = environment.home.join("benchmark-mount");
    let executable = appdir.join("usr/bin/arcade-find");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"benchmark executable").unwrap();
    let now = arcade_link::manifest::now_rfc3339();
    environment
        .receipts()
        .write(&arcade_link::receipt::Receipt {
            schema: 1,
            id: arcade_find::link::ME.into(),
            version: arcade_find::VERSION.into(),
            channel: "stable".into(),
            method: arcade_link::receipt::InstallMethod::Appimage,
            managed_by: arcade_link::receipt::ManagedBy::SelfManaged,
            path: image.clone(),
            integration: Default::default(),
            previous: None,
            installed_at: now.clone(),
            updated_at: now,
        })
        .unwrap();
    let runtime = Runtime { executable, appimage: Some(image), appdir: Some(appdir), dev_build: false };
    assert_eq!(install::detect(&environment, arcade_find::link::ME, &runtime).unwrap().state, install::State::InstalledHere);
    measure("installed/receipt", &environment, &runtime);
}

fn measure(case: &str, environment: &Environment, runtime: &Runtime) {
    let mut samples = Vec::new();
    for _ in 0..100 {
        let (tx, rx) = std::sync::mpsc::channel();
        let started = Instant::now();
        let _ = install::detect(environment, arcade_find::link::ME, runtime).unwrap();
        let watcher = TrayHostWatcher::start(
            WatcherConfig { locations: environment.locations.clone(), app: arcade_find::link::me(), link_enabled: true, has_tray: true },
            move |s| {
                tx.send(s).ok();
            },
        )
        .unwrap();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        drop(watcher);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"case":case,"samples":samples.len(),"p50Ms":samples[50],"p95Ms":samples[95],"maxMs":samples[99],"budgetMs":5,"budgetPassed":samples[99] <= 5.0})
    );
}
