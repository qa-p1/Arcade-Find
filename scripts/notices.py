#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.md: every third-party part that ships inside Arcade Find, with its licence text.

  scripts/notices.py                          # rewrite THIRD_PARTY_NOTICES.md
  scripts/notices.py --check                  # fail if it is out of date (scripts/check.py and CI run this)
  scripts/notices.py --check-appimage FILE    # fail if the AppImage bundles a library or program it does not list

Rust: the normal (non-dev, non-build) dependency graph of the shipped packages, for every target platform, from
`cargo metadata --locked`; each crate's own licence files from its source (a git dependency's from its checkout
root). A crate that declares a licence but publishes no file gets the standard SPDX text from scripts/licences/.
Fonts: Inter (carried by Arcade Link's design kit) and the fonts egui ships (their texts come with epaint_default_fonts).
Vendored Arcade Link and Arcade Lens files that ship (not the tools/ scripts CI runs): read from VENDORED.md, so the pins named there are the ones printed here.
Native libraries of the Linux AppImage: none. build-appimage.sh packs only arcade-find, and --check-appimage fails
if a built image holds any other ELF file, so a bundled library cannot appear without a notice.
Identical licence texts are printed once, with every component that uses them.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_NOTICES.md"
SHIPPED = ("arcade-find", "find-core")
STANDARD = ROOT / "scripts" / "licences"
PREFERRED = ("MIT", "Apache-2.0", "Zlib", "BSD-3-Clause", "BSL-1.0", "MPL-2.0", "Unlicense", "CC0-1.0")
LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright)([-._].*)?$", re.I)
# The only program an AppImage built by packaging/linux/build-appimage.sh holds.
APPIMAGE_ELF = ("usr/bin/arcade-find",)


def normalized(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def licence_texts(folder, declared=None):
    files = []
    if declared:
        files.append(folder / declared)
    if folder.is_dir():
        files += sorted(p for p in folder.iterdir() if p.is_file() and LICENCE_FILE.match(p.name))
        for sub in ("LICENSES", "licenses"):
            if (folder / sub).is_dir():
                files += sorted(p for p in (folder / sub).iterdir() if p.is_file())
        # epaint_default_fonts keeps the licence of every font next to it (Hack, Ubuntu, Noto Emoji, emoji-icon-font).
        if (folder / "fonts").is_dir():
            files += sorted(p for p in (folder / "fonts").iterdir() if p.is_file() and p.suffix == ".txt")
    seen, texts = set(), []
    for path in files:
        if path.resolve() in seen or not path.is_file():
            continue
        seen.add(path.resolve())
        text = normalized(path.read_text(encoding="utf-8", errors="replace"))
        if text:
            texts.append(text)
    return texts


def checkout_root(folder):
    """The repository root of a git dependency (licence files usually live there, not in the crate's folder)."""
    for parent in folder.parents:
        if (parent / ".git").exists() or parent.parent.name == "checkouts":
            return parent
    return None


def standard_text(expression, authors, name):
    """The SPDX text of the first preferred licence the crate offers, with its authors as copyright holders."""
    offered = re.split(r"\s+OR\s+|/", expression or "")
    # "Apache-2.0 WITH LLVM-exception": the licence text, then the exception's.
    exceptions = dict(term.split(" WITH ", 1) for term in offered if " WITH " in term)
    offered += list(exceptions)
    for spdx in PREFERRED:
        if spdx in offered:
            holders = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in authors) or f"the {name} developers"
            text = (STANDARD / f"{spdx}.txt").read_text(encoding="utf-8")
            text = re.sub(r"Copyright \(c\) <year> <(copyright holders|owner)>\.?", f"Copyright (c) {holders}", text)
            if spdx in exceptions:
                text += "\n\n" + (STANDARD / f"{exceptions[spdx]}.txt").read_text(encoding="utf-8")
            return [normalized(f"{name} is offered under {expression}; it publishes no licence file, so the standard "
                               f"{spdx} text follows.\n\n{text}")]
    return []


def metadata():
    return json.loads(subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--all-features"],
        # cargo prints UTF-8; without the encoding, Windows decodes it as cp1252 and garbles authors such as "Döring".
        cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8").stdout)


def rust_crates(meta):
    packages = {p["id"]: p for p in meta["packages"]}
    members = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    todo = [i for i in members if packages[i]["name"] in SHIPPED]
    if len(todo) != len(SHIPPED):
        sys.exit(f"notices: expected workspace crates {SHIPPED}")
    reached = set()
    while todo:
        node = nodes[todo.pop()]
        for dep in node["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in reached:
                reached.add(dep["pkg"])
                todo.append(dep["pkg"])
    crates = []
    for pid in reached - members:
        p = packages[pid]
        folder = Path(p["manifest_path"]).parent
        texts = licence_texts(folder, p.get("license_file"))
        if not texts and p["source"] and p["source"].startswith("git+"):
            root = checkout_root(folder)
            texts = licence_texts(root) if root else []
        if not texts:
            texts = standard_text(p.get("license"), p.get("authors") or [], p["name"])
        crates.append({
            "label": f'{p["name"]} {p["version"]}',
            "licence": p.get("license") or ("see licence file" if p.get("license_file") else "unspecified"),
            "home": p.get("repository") or p.get("homepage") or "",
            "texts": texts,
        })
    return crates, packages


def link_checkout(meta):
    for p in meta["packages"]:
        if p["name"] == "arcade-link":
            return p["version"], p["source"], checkout_root(Path(p["manifest_path"]).parent)
    sys.exit("notices: arcade-link is not a dependency")


def vendored(meta):
    """Arcade Link and Arcade Lens files copied into this repository, as VENDORED.md records them."""
    version, source, link_root = link_checkout(meta)
    text = (ROOT / "VENDORED.md").read_text(encoding="utf-8")
    kit = re.search(r"Copied unchanged from \[Arcade Link\]\([^)]*\) tag `v([0-9.]+)` \(commit\s+`([0-9a-f]{40})`\)", text)
    if not kit:
        sys.exit("notices: VENDORED.md has no 'Arcade design kit' section naming a Link tag and commit")
    if kit.group(1) != version or not (source or "").endswith("#" + kit.group(2)):
        sys.exit(f"notices: VENDORED.md says the design kit is Link v{kit.group(1)} ({kit.group(2)}), but Cargo.lock pins "
                 f"arcade-link {version} ({source}); run scripts/sync-design")
    link_texts = licence_texts(link_root) if link_root else []
    lens_mit = normalized((STANDARD / "MIT.txt").read_text(encoding="utf-8").replace(
        "Copyright (c) <year> <copyright holders>", "Copyright (c) 2026 qa-p1 and Arcade Lens contributors"))
    items = []
    for line in text.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if not line.startswith("|") or len(cells) != 4 or cells[0] in ("Files here", "Code here") or set(cells[0]) <= {"-"}:
            continue
        files, source_cell, pin, licence = cells
        if files.startswith("`tools/"):
            continue  # Release tooling run by CI; none of it is in a package.
        link = "Arcade Link" in source_cell
        if not link and "Arcade Lens" not in source_cell:
            sys.exit(f"notices: VENDORED.md row for {files} names neither Arcade Link nor Arcade Lens")
        if "the tag" in pin:
            pin = f"tag `v{kit.group(1)}` (commit `{kit.group(2)[:7]}`)"
        origin = re.sub(r"\[([^]]*)\]\([^)]*\)", r"\1", source_cell)
        items.append({
            "label": f"{files} (from {origin}, {pin})",
            "licence": licence,
            "home": "https://github.com/qa-p1/Arcade-Link" if link else "https://github.com/qa-p1/Arcade-lens",
            "texts": link_texts if link else [lens_mit],
        })
    if not items:
        sys.exit("notices: no vendored files found in VENDORED.md")
    return items


def render():
    meta = metadata()
    crates, _ = rust_crates(meta)
    inter = normalized((ROOT / "assets/design-licenses/Inter-OFL.txt").read_text(encoding="utf-8"))
    fonts = [{"label": "Inter (typeface, carried by Arcade Link's design kit)", "licence": "OFL-1.1",
              "home": "https://rsms.me/inter/", "texts": [inter]}]
    mit = (STANDARD / "MIT.txt").read_text(encoding="utf-8")
    packaging = [
        {"label": "AppImage type2-runtime (Linux AppImage)", "licence": "MIT",
         "home": "https://github.com/AppImage/type2-runtime",
         "texts": [normalized(mit.replace("<year> <copyright holders>", "the AppImage project contributors"))]},
    ]
    sections = [("Fonts", fonts), ("Arcade Link and Arcade Lens files in this repository", vendored(meta)),
                ("Rust crates", crates), ("Packaging", packaging)]
    missing = [c["label"] for _, group in sections for c in group if not c["texts"]]
    if missing:
        sys.exit("notices: no licence text found for " + ", ".join(sorted(missing)))

    lines = [
        "# Third-party notices",
        "",
        "Arcade Find is licensed MIT OR Apache-2.0 (see `LICENSE`). It includes the open-source parts below, each under",
        "its own licence. This file is generated by `scripts/notices.py`; do not edit it by hand.",
        "",
        "The Rust crates are those of every platform's build (each build includes only its platform's subset).",
        "",
    ]
    by_text = {}
    for title, group in sections:
        lines += [f"## {title}", ""]
        for c in sorted(group, key=lambda c: c["label"].lower()):
            home = f" — {c['home']}" if c["home"] else ""
            lines.append(f"- {c['label']} ({c['licence']}){home}")
            for text in c["texts"]:
                by_text.setdefault(text, []).append(c["label"])
        lines.append("")
    lines += [
        "## Native libraries in the Linux AppImage",
        "",
        "None. The AppImage holds only `usr/bin/arcade-find` (plus its documents and icons). It links the system's",
        "libxkbcommon, libgcc_s, libm and libc, and loads X11, Wayland and OpenGL libraries at run time from the system;",
        "none of them is part of the image. `scripts/notices.py --check-appimage` fails CI if a built image ever holds",
        "another program or library. Windows and macOS packages hold only Find's own program and the files listed above.",
        "",
        "## Licence texts",
        "",
    ]
    for n, (text, users) in enumerate(sorted(by_text.items(), key=lambda kv: (sorted(kv[1])[0].lower(), kv[0])), 1):
        users = sorted(set(users), key=str.lower)
        lines += [f"### {n}. Used by {', '.join(users)}", "", "```text", text.replace("```", "'''"), "```", ""]
    return "\n".join(lines)


def is_elf(path):
    try:
        with open(path, "rb") as f:
            return f.read(4) == b"\x7fELF"
    except OSError:
        return False


def unexpected_elf(root):
    """ELF files under an extracted AppImage that the notices do not account for."""
    root = Path(root)
    found = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and not path.is_symlink() and is_elf(path):
            relative = path.relative_to(root).as_posix()
            if relative not in APPIMAGE_ELF:
                found.append(relative)
    return found


def check_appimage(appimage):
    with tempfile.TemporaryDirectory() as work:
        image = Path(work) / "image.AppImage"
        shutil.copy2(appimage, image)
        image.chmod(0o755)
        subprocess.run([str(image), "--appimage-extract"], cwd=work, check=True, stdout=subprocess.DEVNULL,
                       env=dict(os.environ, APPIMAGE_EXTRACT_AND_RUN=""))
        root = Path(work) / "squashfs-root"
        if not (root / APPIMAGE_ELF[0]).is_file():
            sys.exit(f"notices: {APPIMAGE_ELF[0]} is not in the AppImage")
        extra = unexpected_elf(root)
    if extra:
        sys.exit("notices: the AppImage bundles programs or libraries THIRD_PARTY_NOTICES.md does not list:\n  "
                 + "\n  ".join(extra) + "\nList them (and their licences) in scripts/notices.py first.")
    print("The AppImage bundles no library or program besides usr/bin/arcade-find")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if THIRD_PARTY_NOTICES.md is out of date")
    ap.add_argument("--check-appimage", type=Path, metavar="FILE", help="fail if the built AppImage bundles a library")
    args = ap.parse_args()
    if args.check_appimage:
        check_appimage(args.check_appimage.resolve())
        return
    text = render()
    if args.check:
        current = normalized(OUT.read_text(encoding="utf-8")) if OUT.exists() else ""
        if current != normalized(text):
            sys.exit("THIRD_PARTY_NOTICES.md is out of date: run scripts/notices.py")
        print("THIRD_PARTY_NOTICES.md is current")
        return
    OUT.write_text(text + "\n", encoding="utf-8", newline="\n")
    print(f"Wrote {OUT.name}: {len(text) // 1024} KiB")


if __name__ == "__main__":
    main()
