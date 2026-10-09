#!/usr/bin/env python3
"""Whole-app benchmark on a real tree: crawl time, resident memory, warm
start, search latency and idle CPU of the actual `arcade-find` binary.

    python3 scripts/bench_real.py [--files 1000000] [--bin target/release/arcade-find] [--keep]

Builds a synthetic home-like tree of empty files (names only matter),
runs Find in an isolated profile (ARCADE_FIND_HOME, ARCADE_HOME, its own
runtime dir) without any display, and prints a JSON report. Nothing here
touches the real profile or desktop.
"""

import argparse
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time

WORDS = ("report invoice photo img scan notes draft final budget plan design logo "
         "project client meeting summary data export backup config readme test "
         "build release video music track album holiday family receipt contract "
         "letter resume slides chart model texture shader script module index").split()
EXTS = ("pdf docx xlsx txt md png jpg jpeg mp4 mp3 flac rs py js ts json yaml toml "
        "zip tar.gz svg psd csv html css go c h").split()


def make_tree(root, total, seed=7):
    rnd = random.Random(seed)
    dirs_top = ["Documents", "Pictures", "Music", "Videos", "Projects", "Downloads", "Desktop", ".config", ".local/share"]
    made = 0
    d = 0
    while made < total:
        top = dirs_top[d % len(dirs_top)]
        sub = os.path.join(root, top, f"{rnd.choice(WORDS)}-{d // len(dirs_top)}", f"{rnd.choice(WORDS)}{d % 13}")
        os.makedirs(sub, exist_ok=True)
        n = min(500, total - made)
        for i in range(n):
            name = f"{rnd.choice(WORDS)}_{rnd.choice(WORDS)}-{rnd.randint(1, 9999)}.{rnd.choice(EXTS)}"
            if i == 0 and d == 777:
                name = "needle-unique-file.pdf"
            open(os.path.join(sub, name), "wb").close()
        made += n
        d += 1
    return d


def status(bin_, env):
    r = subprocess.run([bin_, "--status"], env=env, capture_output=True, text=True)
    return json.loads(r.stdout) if r.returncode == 0 else None


def proc_stat(pid):
    with open(f"/proc/{pid}/status") as f:
        rss = next(int(l.split()[1]) for l in f if l.startswith("VmRSS"))
    with open(f"/proc/{pid}/stat") as f:
        parts = f.read().rsplit(")", 1)[1].split()
    ticks = int(parts[11]) + int(parts[12])
    return rss, ticks


def wait_ready(bin_, env, timeout=900):
    t0 = time.time()
    while time.time() - t0 < timeout:
        s = status(bin_, env)
        if s and s["engine"]["phase"] == "ready":
            return s, time.time() - t0
        time.sleep(0.05)
    raise SystemExit("timed out waiting for the index")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--files", type=int, default=1_000_000)
    ap.add_argument("--bin", default="target/release/arcade-find")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    bin_ = os.path.abspath(a.bin)
    work = tempfile.mkdtemp(prefix="af-bench-")
    tree = os.path.join(work, "home")
    try:
        t0 = time.time()
        dirs = make_tree(tree, a.files)
        made_s = time.time() - t0
        env = dict(os.environ)
        for k in ("WAYLAND_DISPLAY", "DISPLAY"):
            env.pop(k, None)
        env.update(ARCADE_FIND_HOME=os.path.join(work, "find"), ARCADE_HOME=os.path.join(work, "arcade"), XDG_RUNTIME_DIR=os.path.join(work, "run"))
        os.makedirs(env["XDG_RUNTIME_DIR"], mode=0o700)
        os.makedirs(os.path.join(env["ARCADE_FIND_HOME"], "config"))
        with open(os.path.join(env["ARCADE_FIND_HOME"], "config", "settings.json"), "w") as f:
            json.dump({"schema": 1, "roots": [tree]}, f)

        report = {"files": a.files, "dirs": dirs, "treeCreateS": round(made_s, 1)}
        # First run: the crawl. No display: the resident runs headless (the
        # overlay backend fails to start, so use the service through --status).
        p = subprocess.Popen([bin_, "--background"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        s, crawl_s = wait_ready(bin_, env)
        pid = s["pid"]
        report["crawlS"] = round(crawl_s, 2)
        report["entries"] = s["engine"]["entries"]
        report["indexHeapMiB"] = round(s["engine"]["indexBytes"] / 2**20, 1)
        report["watch"] = s["engine"]["watch"]["mode"]
        time.sleep(3)
        rss, t1 = proc_stat(pid)
        report["rssAfterCrawlMiB"] = round(rss / 1024, 1)
        time.sleep(30)
        rss2, t2 = proc_stat(pid)
        report["idleTicksPer30s"] = t2 - t1
        report["rssIdleMiB"] = round(rss2 / 1024, 1)
        # Search latency through the CLI (includes process start + IPC).
        lat = {}
        for q in ["re", "report", "invoice 2024", "needle-unique", "photo/img", "ext:pdf budget", "zzzz"]:
            times = []
            for _ in range(5):
                t = time.perf_counter()
                r = subprocess.run([bin_, "--search", q, "--json"], env=env, capture_output=True, text=True)
                times.append((time.perf_counter() - t) * 1000)
                engine_ms = json.loads(r.stdout)["elapsedMs"]
            lat[q] = {"engineMs": round(engine_ms, 2), "cliRoundTripMsP50": round(sorted(times)[2], 1)}
        report["search"] = lat
        subprocess.run([bin_, "--quit"], env=env)
        p.wait(timeout=60)
        index_file = os.path.join(env["ARCADE_FIND_HOME"], "data", "index.bin")
        report["indexFileMiB"] = round(os.path.getsize(index_file) / 2**20, 1)
        # Second run: warm start from the saved index.
        p = subprocess.Popen([bin_, "--background"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        t = time.time()
        while True:
            s = status(bin_, env)
            if s:
                break
            time.sleep(0.01)
        report["warmStartToAnswerMs"] = round((time.time() - t) * 1000)
        report["warmLoadMs"] = s["engine"]["loadMs"]
        s, _ = wait_ready(bin_, env)
        time.sleep(3)
        rss, _ = proc_stat(s["pid"])
        report["rssWarmMiB"] = round(rss / 1024, 1)
        subprocess.run([bin_, "--quit"], env=env)
        p.wait(timeout=60)
        print(json.dumps(report, indent=2))
    finally:
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
