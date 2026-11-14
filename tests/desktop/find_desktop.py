#!/usr/bin/env python3
"""Tier 1 (headless Hyprland, Link's desktop harness) for Arcade Find.

    find_desktop.py <AppImage>

In a private profile (ARCADE_FIND_HOME) with a fixture folder as the only location:
  * the popup rule: the overlay stays open when the pointer leaves it and when another window takes the focus;
  * the Escape ladder (FD-7, RD-FIND-1): the first Escape clears the text, the second closes;
  * drag and drop OUT (spec acceptance, owner checklist): a press on a result row, motion and a release over another
    window deliver that file to the window as a text/uri-list drop. Find has no drop-IN surface (files are pasted
    with Ctrl+V; see docs/v1/DECISIONS.md D6), so there is nothing to drop onto it.
  * the window placement: the panel stays inside the monitor with 20 visible rows (FD-4).

A check named "KNOWN BUG <id>" must find the bug (XFAIL) until the fix lands, then it reports XPASS and fails the run
until the marker is removed (docs/v1/DECISIONS.md D2). Exit 0 when every check is as expected, 1 otherwise.

Not run locally (no compositor in the authoring sandbox); it runs in CI through Link's desktop-e2e workflow.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, ".arcade-link/tools/desktop")
from arcade_desktop import Desktop  # noqa: E402

LAYER = "dev.arcade.Find"
SHADOW, BAR, LIST_PAD, ROW = 40, 56, 6, 52  # ui/model.rs metrics (logical pixels)
d = Desktop()
home = Path(tempfile.mkdtemp(prefix="find-desktop-"))
fixture = home / "fixture"
fixture.mkdir()
for name in ("report-alpha.txt", "report-beta.txt", "report-gamma.txt"):
    (fixture / name).write_text(name)
(home / "config").mkdir()
(home / "config/settings.json").write_text(json.dumps({"roots": [str(fixture)], "rows": {"visible": 20, "stretch": 30}}))
app = str(home / "Arcade-Find.AppImage")
shutil.copy2(sys.argv[1], app)
os.chmod(app, 0o755)
env = {**os.environ, "HOME": str(home), "ARCADE_FIND_HOME": str(home)}
log = open(d.artifact("find.log"), "wb")
results = []


def run(*args, timeout=30):
    return subprocess.run([app, *args], env=env, capture_output=True, text=True, timeout=timeout)


def check(name, ok, detail=""):
    if name.startswith("KNOWN BUG"):
        status = "XFAIL" if not ok else "XPASS"
    else:
        status = "PASS" if ok else "FAIL"
    if status in ("FAIL", "XFAIL"):
        d.screenshot(name.split(":")[0].replace(" ", "-").lower())
    results.append((status, name, detail))
    print(f"{status} {name}{': ' + detail if detail else ''}", flush=True)
    return ok


def ui():
    out = run("--status")
    try:
        return json.loads(out.stdout).get("ui") or {}
    except ValueError:
        return {}


def layer():
    return d.find_layer(LAYER)


def shown(timeout=8):
    try:
        return d.wait(lambda: layer() and ui().get("visible") and layer(), timeout, "overlay shown")
    except AssertionError:
        return None


def hidden(timeout=3):
    try:
        d.wait(lambda: not ui().get("visible"), timeout, "overlay hidden")
        return True
    except AssertionError:
        return False


def results_ready(timeout=8):
    d.wait(lambda: ui().get("rows", 0) >= 3, timeout, "three results")


resident = subprocess.Popen([app, "--background"], env=env, stdout=log, stderr=subprocess.STDOUT)
target_result = home / "dropped.json"
target = subprocess.Popen(["/usr/bin/python3.12", str(Path(__file__).with_name("drop_target.py")), str(target_result)],
                          stdout=log, stderr=subprocess.STDOUT)
try:
    d.wait(lambda: run("--status").returncode == 0, 30, "Find to answer --status")
    window = d.wait_window(cls="arcade-drop-target")

    # --- the Escape ladder and the popup rule -----------------------------------------------------------------
    run("--show", "report")
    check("overlay opens with --show", shown() is not None)
    results_ready()
    panel = layer()
    monitor0 = next(m for m in d.monitors() if m["name"] == panel["monitor"])
    d.pointer_move(monitor0["x"] + 10, monitor0["y"] + 10)
    time.sleep(0.5)
    check("popup stays open when the pointer is outside it", ui().get("visible") is True)
    d.focus(window["address"])
    time.sleep(0.8)
    check("popup stays open when another window takes the focus", ui().get("visible") is True and ui().get("query") == "report")
    d.escape(None)
    time.sleep(0.4)
    state = ui()
    check("first Escape clears the text and keeps the popup", state.get("visible") is True and state.get("query") == "")
    d.escape(None)
    check("second Escape closes", hidden())

    # --- window placement with 20 rows (FD-4) -----------------------------------------------------------------
    run("--show", "report")
    panel = shown()
    monitor = next(m for m in d.monitors() if m["name"] == panel["monitor"])
    top, bottom = panel["y"], panel["y"] + panel["h"]
    check("KNOWN BUG FD-4: the panel stays inside the monitor", monitor["y"] <= top and bottom <= monitor["y"] + monitor["height"],
          f"y={top}..{bottom} on a {monitor['height']} px monitor")
    d.escape(None)
    d.escape(None)
    hidden()

    # --- drag and drop out (spec acceptance) ------------------------------------------------------------------
    run("--show", "report-alpha")
    panel = shown()
    d.wait(lambda: ui().get("rows", 0) >= 1, 8, "one result")
    row_x = panel["x"] + SHADOW + 300
    row_y = panel["y"] + SHADOW + BAR + LIST_PAD + ROW // 2
    drop_at = (window["at"][0] + 120, window["at"][1] + 120)
    d.drag((row_x, row_y), drop_at, steps=20)
    got = None
    try:
        got = d.wait(lambda: target_result.exists() and json.loads(target_result.read_text()), 8, "the drop")
    except AssertionError:
        pass
    expected = (fixture / "report-alpha.txt").as_uri()
    check("a result row dragged onto another window arrives as a uri-list drop", bool(got) and expected in got.get("uris", []),
          json.dumps(got))
finally:
    for proc in (target, resident):
        proc.terminate()
    log.flush()

bad = [r for r in results if r[0] in ("FAIL", "XPASS")]
print(f"{len(results) - len(bad)}/{len(results)} as expected")
sys.exit(1 if bad else 0)
