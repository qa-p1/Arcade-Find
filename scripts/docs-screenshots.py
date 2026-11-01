#!/usr/bin/env python3
"""Retakes the README and user-guide screenshots: the search overlay with
results and the Settings General page, in light and dark.

    dbus-run-session -- python3 scripts/docs-screenshots.py target/release/arcade-find docs/user/images <work dir>

Runs the real binary on a private Xvfb (with xcompmgr for the translucent
overlay) in a throwaway profile whose only indexed folder is a generated
demo tree, so nothing from this machine appears. Needs Xvfb, xcompmgr,
xdotool and ImageMagick 6 (`convert`, `import`, `display`).
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

W, H = 1440, 900
DISPLAY = ":78"
QUERY = "report"
# The demo tree the overlay searches: name → contents.
FILES = {
    "Documents/Reports/quarterly-report-2026-q3.pdf": "%PDF-1.4\n",
    "Documents/Reports/annual-report-2025.odt": "annual report\n",
    "Documents/Reports/report-figures.ods": "figures\n",
    "Documents/Notes/weekly-report.md": "# Weekly report\n",
    "Projects/arcade-site/src/report.ts": "export const report = 1;\n",
    "Projects/arcade-site/README.md": "# arcade-site\n",
    "Downloads/expense-report-october.csv": "date,amount\n",
    "Pictures/report-cover.png": "",
    "Music/Field Recordings/rain.flac": "",
}


def sh(*args, **kw):
    return subprocess.run([str(a) for a in args], check=True, **kw)


def backdrop(path, dark):
    """A plain desktop with a text editor window, so the overlay has context."""
    bg1, bg2 = ("#1d2733", "#121820") if dark else ("#dfe7ef", "#c9d4e0")
    page, ink, muted, bar = (("#1b1f24", "#e6e9ee", "#9aa3ad", "#252a31") if dark
                             else ("#ffffff", "#1d232b", "#5d6773", "#eef1f4"))
    x0, y0, x1, y1 = 200, 120, 1240, 840
    cmd = ["convert", "-size", f"{W}x{H}", f"gradient:{bg1}-{bg2}",
           "-fill", page, "-draw", f"roundrectangle {x0},{y0} {x1},{y1} 10,10",
           "-fill", bar, "-draw", f"roundrectangle {x0},{y0} {x1},{y0 + 44} 10,10",
           "-draw", f"rectangle {x0},{y0 + 30} {x1},{y0 + 44}",
           "-font", "DejaVu-Sans", "-fill", muted, "-pointsize", "15",
           "-annotate", f"+{x0 + 440}+{y0 + 28}", "weekly-report.md — Notes",
           "-font", "DejaVu-Sans-Mono", "-fill", ink, "-pointsize", "18"]
    lines = ["# Weekly report", "", "- Shipped the new Arcade look to Find and Lens.",
             "- Settings pages now follow the shared layout.", "- Next: Shelf and Wheel.", "",
             "## Numbers", "", "| Week | Builds | Releases |", "|------|--------|----------|",
             "| 43   | 41     | 3        |", "| 44   | 37     | 2        |"]
    for i, line in enumerate(lines):
        cmd += ["-annotate", f"+{x0 + 40}+{y0 + 100 + i * 30}", line]
    cmd.append(str(path))
    sh(*cmd)


def main():
    binary, out, work = Path(sys.argv[1]).resolve(strict=True), Path(sys.argv[2]), Path(sys.argv[3])
    out.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    xvfb = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", f"{W}x{H}x24", "-nolisten", "tcp"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    comp = subprocess.Popen(["xcompmgr", "-n"], env=dict(os.environ, DISPLAY=DISPLAY),
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    try:
        for theme in ("light", "dark"):
            shoot(binary, out, work / theme, theme)
    finally:
        comp.terminate()
        xvfb.terminate()


def shoot(binary, out, root, theme):
    home = root / "home"
    for d in ("home", "find/config", "xdg-config", "xdg-data", "xdg-cache", "runtime"):
        (root / d).mkdir(parents=True, exist_ok=True)
    os.chmod(root / "runtime", 0o700)
    for name, text in FILES.items():
        f = home / name
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
    (root / "find/config/settings.json").write_text(json.dumps(
        {"schema": 1, "welcomeComplete": True, "theme": theme, "roots": [str(home)]}))
    env = dict(os.environ, DISPLAY=DISPLAY, HOME=str(home), ARCADE_FIND_HOME=str(root / "find"),
               ARCADE_HOME=str(root / "arcade"), XDG_CONFIG_HOME=str(root / "xdg-config"),
               XDG_DATA_HOME=str(root / "xdg-data"), XDG_CACHE_HOME=str(root / "xdg-cache"),
               XDG_RUNTIME_DIR=str(root / "runtime"), ARCADE_NO_INSTALL_PROMPT="1")
    env.pop("WAYLAND_DISPLAY", None)

    def x(*args):
        return subprocess.run(["xdotool", *map(str, args)], env=env, capture_output=True, text=True).stdout.strip()

    def find(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=20)

    def status():
        o = find("--status").stdout.strip()
        return json.loads(o) if o.startswith("{") else {}

    def wait(check, what, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            v = check()
            if v:
                return v
            time.sleep(0.1)
        raise SystemExit(f"FAIL {theme}: {what}")

    bg = root / "backdrop.png"
    backdrop(bg, theme == "dark")
    procs = [subprocess.Popen(["display", "-geometry", "+0+0", "-borderwidth", "0", "-title", "backdrop", str(bg)],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)]
    time.sleep(1.0)
    log = open(root / "find.log", "w")
    procs.append(subprocess.Popen([str(binary), "--background"], env=env, stdout=log, stderr=log))
    try:
        wait(lambda: status().get("engine", {}).get("phase") == "ready", "the index did not finish")
        find("--show", QUERY)
        wait(lambda: status().get("ui", {}).get("rows", 0) >= 6, "the results did not populate")
        # --show selects the query so typing replaces it; End shows it as typed.
        overlay = x("search", "--onlyvisible", "--class", "^dev.arcade.Find$").split()[-1]
        x("windowfocus", "--sync", overlay)
        x("key", "--window", overlay, "End")
        x("mousemove", 5, 5)
        time.sleep(1.0)
        sh("import", "-window", "root", str(out / f"overlay-{theme}.png"), env=env)
        find("--hide")
        time.sleep(0.5)
        settings = subprocess.Popen([str(binary), "--settings-window"], env=env, stdout=log, stderr=log)
        procs.append(settings)
        win = wait(lambda: x("search", "--onlyvisible", "--pid", settings.pid).split()[-1:], "Settings did not open")[0]
        time.sleep(2.0)
        sh("import", "-window", win, str(out / f"settings-{theme}.png"), env=env)
    finally:
        find("--quit")
        time.sleep(0.5)
        for p in reversed(procs):
            p.terminate()
            try:
                p.wait(5)
            except subprocess.TimeoutExpired:
                p.kill()
        log.close()


if __name__ == "__main__":
    main()
