#!/usr/bin/env python3
"""Exercise the real package under a disposable profile; no GUI or live bus.

Usage: python3 scripts/verify-appimage.py dist/Arcade-Find-x86_64.AppImage
"""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import re
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
VERSION = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(encoding="utf-8"), re.M).group(1)


def main():
    if os.environ.get("ARCADE_E2E_INNER") != "1":
        raise SystemExit("Run through Link tools/e2e.py run --")
    artifact = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="find-appimage-") as temporary:
        root = Path(temporary)
        env = dict(os.environ)
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK", "APPIMAGE", "APPDIR", "ARCADE_DEV_BUILD"):
            env.pop(key, None)
        for key, directory in (("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                               ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "run"),
                               ("ARCADE_HOME", "arcade"), ("ARCADE_FIND_HOME", "find")):
            env[key] = str(root / directory)
        (root / "run").mkdir(mode=0o700)
        env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={root}/no-bus"
        env["APPIMAGE_EXTRACT_AND_RUN"] = "1"  # Works without a FUSE mount.
        source = root / "Downloads/Arcade Find.AppImage"
        source.parent.mkdir()
        shutil.copy2(artifact, source)

        def run(binary, *args):
            result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, (args, result.stderr)
            return json.loads(result.stdout) if result.stdout.strip() else None

        assert run(source, "--integration-status")["receipt"] is None
        assert not (root / "arcade").exists(), "read-only status created integration"
        receipt = run(source, "--install", "--silent", "--no-launch")
        installed = Path(receipt["path"])
        assert receipt["method"] == "appimage" and receipt["version"] == VERSION
        assert not source.exists(), "download removal default did not apply"
        assert installed.read_bytes() == artifact.read_bytes()
        paths = [installed, Path(receipt["integration"]["desktopEntry"]), Path(receipt["integration"]["cli"]),
                 Path(receipt["integration"]["autostart"]), *map(Path, receipt["integration"]["icons"]),
                 root / "arcade/installs/arcade.find.json"]
        assert all(p.exists() and p.is_relative_to(root) for p in paths)
        assert installed.stat().st_mode & 0o777 == 0o755
        assert paths[-1].stat().st_mode & 0o777 == 0o600
        status = run(installed, "--integration-status")
        assert all(e["exists"] for e in status["entries"])
        manifest = run(installed, "--arcade-manifest")
        assert manifest["executable"] == str(installed) and manifest["install"] == "appimage"
        print("installed files:", flush=True)
        for p in paths:
            print("  " + str(p.relative_to(root)), flush=True)
        print("receipt: " + json.dumps(receipt, sort_keys=True), flush=True)

        moved = root / "Moved Find.AppImage"
        installed.rename(moved)
        Path(receipt["integration"]["desktopEntry"]).unlink()
        repaired = run(moved, "--repair")
        assert repaired["path"] == str(moved)
        assert Path(repaired["integration"]["cli"]).readlink() == moved
        assert str(moved) in Path(repaired["integration"]["desktopEntry"]).read_text()
        assert run(moved, "--uninstall", "--silent")["uninstalled"]
        assert not moved.exists(), "receipt uninstall did not remove the moved copy"
        assert all(not p.exists() and not p.is_symlink() for p in paths), "uninstall retained integration"

        # Prove --install --silent re-execs the installed file. --background
        # is retained; installer controls are removed from the relaunch.
        source.parent.mkdir(exist_ok=True)
        shutil.copy2(artifact, source)
        settings = root / "find/config/settings.json"
        files = root / "files"
        files.mkdir()
        settings.write_text(json.dumps({"schema": 1, "roots": [str(files)]}))
        with (root / "resident.log").open("w") as output:
            child = subprocess.Popen([str(source), "--install", "--silent", "--background"], env=env, stdout=output, stderr=output,
                                     start_new_session=True)
            try:
                deadline = time.monotonic() + 20
                while True:
                    if not installed.exists():
                        assert child.poll() is None, (root / "resident.log").read_text()
                        assert time.monotonic() < deadline, "installed copy was not created"
                        time.sleep(0.05)
                        continue
                    result = subprocess.run([str(installed), "--status"], env=env, capture_output=True, text=True, timeout=15)
                    if result.returncode == 0 and json.loads(result.stdout).get("engine", {}).get("phase") == "ready":
                        break
                    assert child.poll() is None, (root / "resident.log").read_text()
                    assert time.monotonic() < deadline, "installed successor did not become ready"
                    time.sleep(0.05)
                run(installed, "--quit")
                child.wait(timeout=15)
            finally:
                if child.poll() is None:
                    if installed.exists():
                        subprocess.run([str(installed), "--quit"], env=env, capture_output=True, timeout=15)
                    if child.poll() is None:
                        os.killpg(child.pid, signal.SIGTERM)
                    child.wait(timeout=10)
        assert run(installed, "--uninstall", "--silent")["uninstalled"]
        assert not installed.exists() and not moved.exists()
        assert not (root / "arcade/installs/arcade.find.json").exists()
        assert all(not p.exists() and not p.is_symlink() for p in paths), "uninstall retained integration"
        assert settings.exists(), "uninstall removed settings without --remove-data"
        print("AppImage: silent install, files/receipt, moved repair, installed re-exec and receipt uninstall passed")


if __name__ == "__main__":
    main()
