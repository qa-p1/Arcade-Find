#![cfg(target_os = "linux")]
//! Synthetic AppImage mount for CLI logic; the packaged AppImage gets a
//! separate isolated end-to-end check. No real desktop paths are involved.
use std::path::{Path, PathBuf};
use std::process::Command;

struct Fixture {
    root: PathBuf,
    binary: PathBuf,
    appdir: PathBuf,
    image: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("find-install-cli-{}", arcade_link::endpoint::new_token().unwrap()));
        let appdir = root.join("mount");
        std::fs::create_dir_all(appdir.join("usr/bin")).unwrap();
        let binary = appdir.join("usr/bin/arcade-find");
        std::fs::copy(env!("CARGO_BIN_EXE_arcade-find"), &binary).unwrap();
        let image = root.join("Downloaded.AppImage");
        std::fs::write(&image, b"synthetic AppImage bytes").unwrap();
        Self { root, binary, appdir, image }
    }
    fn command(&self, binary: &Path) -> Command {
        let mut c = Command::new(binary);
        c.env("HOME", &self.root)
            .env("ARCADE_HOME", self.root.join("arcade"))
            .env("ARCADE_FIND_HOME", self.root.join("find"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("APPDIR", &self.appdir)
            .env("APPIMAGE", &self.image)
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .env_remove("ARCADE_DEV_BUILD");
        c
    }
    fn run(&self, args: &[&str]) -> serde_json::Value {
        let out = self.command(&self.binary).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

#[test]
fn cli_install_status_move_repair_and_receipt_uninstall() {
    let mut fixture = Fixture::new();
    let status = fixture.run(&["--integration-status"]);
    assert!(status["receipt"].is_null());
    assert!(!fixture.root.join("arcade").exists(), "status must have no side effects");
    let receipt = fixture.run(&["--install", "--silent", "--no-launch", "--channel", "nightly"]);
    assert_eq!(receipt["id"], "arcade.find");
    assert_eq!(receipt["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(receipt["channel"], "nightly");
    assert_eq!(receipt["method"], "appimage");
    assert_eq!(receipt["managedBy"], "self");
    let installed = PathBuf::from(receipt["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&installed).unwrap(), b"synthetic AppImage bytes");
    // Outside Downloads the user's file stays (only downloads are removed).
    assert!(fixture.image.exists());
    let desktop = PathBuf::from(receipt["integration"]["desktopEntry"].as_str().unwrap());
    let entry = std::fs::read_to_string(&desktop).unwrap();
    assert!(entry.contains("X-Arcade-Id=arcade.find"));
    assert!(entry.contains("--uninstall"));
    assert!(entry.contains("--settings"));
    assert_eq!(receipt["integration"]["icons"].as_array().unwrap().len(), 10);
    let cli = PathBuf::from(receipt["integration"]["cli"].as_str().unwrap());
    assert_eq!(std::fs::read_link(&cli).unwrap(), installed);
    let status = fixture.run(&["--integration-status"]);
    assert!(status["entries"].as_array().unwrap().iter().all(|e| e["exists"] == true));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(fixture.root.join("arcade/installs/arcade.find.json")).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&installed).unwrap().permissions().mode() & 0o777, 0o755);
    }
    let moved = fixture.root.join("Moved Find.AppImage");
    std::fs::rename(&installed, &moved).unwrap();
    fixture.image = moved.clone();
    std::fs::remove_file(&desktop).unwrap();
    let repaired = fixture.run(&["--repair"]);
    assert_eq!(repaired["path"], moved.to_str().unwrap());
    assert!(std::fs::read_to_string(desktop).unwrap().contains("Moved Find.AppImage"));
    assert_eq!(std::fs::read_link(cli).unwrap(), moved);
    std::fs::create_dir_all(fixture.root.join("find/data")).unwrap();
    std::fs::write(fixture.root.join("find/data/index.bin"), b"keep my data").unwrap();
    assert_eq!(fixture.run(&["--uninstall", "--silent"])["uninstalled"], true);
    assert!(!moved.exists());
    assert!(!fixture.root.join("arcade/installs/arcade.find.json").exists());
    assert_eq!(std::fs::read(fixture.root.join("find/data/index.bin")).unwrap(), b"keep my data");
}

#[test]
fn remove_data_is_explicit_and_dev_or_inherited_appimage_never_installs() {
    let fixture = Fixture::new();
    fixture.run(&["--install", "--silent", "--no-launch"]);
    fixture.run(&["--uninstall", "--silent", "--remove-data"]);
    assert!(!fixture.root.join("find/config").exists());
    assert!(!fixture.root.join("find/data").exists());
    let output = fixture.command(Path::new(env!("CARGO_BIN_EXE_arcade-find"))).args(["--install", "--silent", "--no-launch"]).output().unwrap();
    assert!(!output.status.success(), "a dev binary must not install an inherited AppImage");
    assert!(!fixture.root.join("arcade/installs/arcade.find.json").exists());
    let output = fixture.command(Path::new(env!("CARGO_BIN_EXE_arcade-find"))).arg("--arcade-manifest").output().unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest["install"], "dev");
}
