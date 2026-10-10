# Status

Updated 2026-10-10. Branch `feature/find-v0.1` on `qa-p1/Arcade-Find`. Not
merged, not tagged, not released.

## Evidence

Local runs: Linux x86_64 container, 4 vCPU, 16 GiB, Rust 1.99, release
builds unless noted. CI (`.github/workflows/ci.yml`) runs fmt, clippy
`-D warnings` and the tests on Linux, Windows and macOS, the headless e2e,
and builds the AppImage, Inno installer and universal DMG.

| What | How | Result |
|---|---|---|
| Unit and integration tests | `cargo test --workspace` | 48 (find-core) + 36 (app) + 5 (`tests/link_shelf.rs`) pass; also on Windows and macOS in CI |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` for Linux, Windows and macOS targets; `cargo fmt --check` | clean |
| End to end, Wayland layer shell | `scripts/e2e-linux.sh` in headless Sway 1.9 | pass: indexing, search, filters, hidden files, live add/remove, single instance, one-shot `find.search`, overlay shows centered, keys (Down, Tab, Esc), Enter → `look.preview` and Find hides, mixed selection → `shelf.add` and Find stays open, `--quit` |
| End to end, X11 | same script in Xvfb (no window manager) | same checks pass |
| Link peers | `arcade-link mock` (Arcade Link v0.1.0 CLI) as Look; the **real Arcade Shelf** (`ARCADE_SHELF_BIN`, `feature/shelf-v0.1`) | 49/49: a mixed selection lands in Shelf's store as references; a stopped Shelf is launched from its manifest and takes the add; Shelf's `find.show` opens Find with that file selected ([SHELF.md](SHELF.md)) |
| Ecosystem runner | Arcade Link `tools/e2e.py --only find` (onboarding branch) | 3/3: `find.search` resident and one-shot, `find.show` as Shelf sends it, results into the real Shelf |
| Settings window | launched under Xvfb, General and Connected apps pages inspected | renders; ripgrep detected |
| Overlay design | `arcade-find --snapshot DIR` (18 PNGs, dark and light, 1× and 1.5×) | inspected |
| AppImage | `packaging/linux/build-appimage.sh` (appimagetool 1.9.0, SHA-256 pinned) | built (8.2 MB); `--version` and `--arcade-manifest` run from it; the manifest advertises the AppImage path |
| Release manifest | vendored `tools/arcade-release.py` | `arcade-release.json` + `SHA256SUMS.txt` generated for the AppImage |
| Vendored files | `scripts/check-vendored.sh` | match Arcade Link v0.1.0 |

## Performance (1M files)

`python3 scripts/bench_real.py --files 1000000`: a real tree of 1,000,000
empty files in 4,010 folders, crawled by the release binary, headless (no
overlay, so no font or window memory in these numbers).

| Measure | Result | Target |
|---|---|---|
| Whole-app RSS after crawl / idle / after warm start | 52.0 / 52.0 / 52.1 MiB | well under Look's ~75 MiB |
| Index heap (estimate) | 51.1 MiB | |
| Index file | 38.5 MiB | |
| First crawl (files just created, warm disk cache) | 0.79 s | background, usable during |
| Warm start: load / first answer | 33–37 ms / 43–45 ms | < 1 s |
| Idle CPU | 0 ticks in 30 s (e2e: 0–1 ticks in 10 s with the overlay backend) | ~0 |
| Search while typing (`cargo run --release -p find-core --example typing`) | first letter 15–23 ms, later letters 0–14 ms (most < 5 ms) | < 16 ms per keystroke |
| CLI search round trip (process start + IPC + search) | 5–23 ms | |

Single-letter queries miss the 16 ms target on this 4-vCPU machine (about
half of all entries match); search uses up to 8 threads, so typical 8-thread
desktops should be under it, but that is not measured. With the Wayland
overlay running the e2e RSS was 13–15 MiB on a tiny test tree (fonts and
renderer included), so expect roughly 52 MiB + 10 MiB at 1M files; not
measured together.

## Not verified

- Real desktops: Hyprland, KDE, GNOME, other X11 window managers, Windows,
  macOS (nothing was run interactively). The Windows installer and macOS DMG
  are built only by CI.
- Global shortcut registration (native and Hyprland runtime bind) on a real
  session; tray on a real StatusNotifier host; start-at-login entries at an
  actual login.
- Real peers other than Shelf: Arcade Look, Box, Wheel, Clipboard, Lens
  builds (mock Look only).
- A real (cold-cache) crawl of a large home directory; inotify limit
  degradation on a real system (unit-tested only).
- IME input (no text-input protocol support yet); right-to-left text.

## Not done

- Family onboarding is on reviewable branches (Link #1, Tools #1, Wheel #5;
  see [ARCADE_LINK.md](ARCADE_LINK.md#family-onboarding-open-for-review)), not
  merged. A Link tag, the consumer bumps (Box, Look, Lens, Clipboard list
  apps from Link) and releases need the owner's approval.
- Windows USN-journal helper (optional in the brief): not implemented;
  Windows uses ReadDirectoryChangesW plus rescans.
- No drag-and-drop out of the overlay; no exe icon resource on Windows (the
  installer and shortcuts carry the icon); macOS app not signed or notarized.
- Cancelling a running peer call from Find.

## Documents

[README](../README.md) · [Architecture](ARCHITECTURE.md) ·
[Arcade Link](ARCADE_LINK.md) · [Hyprland](HYPRLAND.md) ·
[Decisions](DECISIONS.md) · [Arcade Shelf](SHELF.md) ·
[Changelog](../CHANGELOG.md) · [Vendored](../VENDORED.md) ·
[Third-party notices](../THIRD_PARTY_NOTICES.md)
