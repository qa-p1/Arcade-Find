# Status

## 2026-10-30: recovery and verification in progress

Continuing draft PR #11 on `claude/arcade-overhaul-q3xegk`. Not released.

- The paused scratch checkout was cleared. Recovered the implementation from
  the preserved checkout and recorded edits onto the existing PR branch.
- Link PR #22 is merged and its `tag.yml` created `v0.4.7` at
  `a30bcc80bffc2c202ee4230f8623fcc45be95223` (run 41002783290).
  Find now pins that real tag and syncs its kit files with recorded hashes.
- Earlier evidence: all 48 Settings captures in run 40917884604 were reviewed.
  Row overlap and long folder names required the shared fix in Link PR #22.
  This head still needs a new real screenshot matrix and visual review.
- Recovered fixed page headings, folder titles, Settings singleton/ordered
  saves, kit install prompt, tray wording/clicks, copy feedback, identifiers,
  migration and project scaffolding. These are implementation, not verification.
- The local sandbox rejects private Unix sockets with EPERM. The full native
  and IPC checks must pass in CI; no blocked check is counted as passed.
- Known remaining native issue: winit cannot hide a Wayland window with
  `set_visible(false)`. Its lifecycle repair and compositor regression remain
  to be recovered and verified before release.
- Release workflow integration, the beta rehearsal, final native/three-OS
  checks, updated screenshots and release/Tools verification remain open.

## 2026-10-29: overhaul (FIND.md) in progress, branch `claude/arcade-overhaul-q3xegk`

Not released. Work so far, checked locally with `cargo fmt --check`, `cargo clippy -D warnings`,
`scripts/check-docs.py`, `scripts/check-vendored.py` and `scripts/isolate.py cargo test --workspace`
(all pass on Linux). CI on Windows and macOS hasn't run on this branch yet.

Done:
- Rust sources formatted at 160 columns (one commit, formatting only).
- Arcade Link pinned to tag `v0.4.6` with `design` and `design-egui`. v0.4.6 adds the egui `glyph_tile`,
  `row_with_glyph`, `checkbox` and `dialog_head`/`dialog_actions` that Find needs (Link PR #21).
- `scripts/sync-design` copies the app glyphs and Find's launcher icon set from the pinned tag; hashes in
  VENDORED.md. The unused `assets/link-tokens.json` and `assets/logo.svg` are gone.
- `theme.rs` builds the overlay palette from the kit (lime accent). Find's file-kind colours are a documented
  content palette in `theme.rs`, with a test that every fill is ≥ 3:1 on the panel, hover and selection
  fills in both themes.
- Overlay (Template B, launcher):
  - kit sizes: 56 px field, 52 px rows, 12 px radius;
  - Inter loaded into cosmic-text; kit search icon; "Content" and "Hidden" badges;
  - accent-soft selection with a 2 px bar; check badges on multi-selected rows;
  - "Pinned"/"Recent" section labels; footer hint bar with key chips;
  - loading/indexing/error/no-matches states (Enter on no matches searches inside files);
  - kit toasts in the footer slot; `shadow-pop` on layer shell (transparent margin with no input);
  - kit relative dates, sizes and counts (`find-core` keeps only exact timestamps).
  - `--snapshot DIR` renders 30 states; reviewed in both themes. The Rename and Trash layout bug found
    there has a regression test.
- IME: winit now enables IME and handles `Ime::Preedit`/`Commit`; layer shell uses `zwp_text_input_v3`.
  Preedit shows underlined at the caret. Model tests cover it. Not yet checked with fcitx5 on Hyprland.
- The settings window was rewritten on the kit's egui components:
  - pages in the standard order (General, Shortcuts, Locations, Index, Connected apps, About);
  - the kit shortcut recorder, and the overlay key list read from `docs/user/shortcuts.json`;
  - Connected apps rows with glyph tiles, badges, Open/Install and Recent activity;
  - About in the standard order, plus Restart;
  - live System theme; Esc and Ctrl+W close it, F1 opens the guide.
  - It builds and its unit tests pass, but **it has not been screenshotted yet**.
- Tray: monochrome glyph helper added; ksni id is now `dev.arcade.Find`. Window and app ids use
  `dev.arcade.Find` (`os::APP_ID`).

Still to do (see FIND.md):
- Screenshot every settings page (light, dark, 720×520, 125 %) and fix what looks wrong.
- Install prompt (`installer_ui.rs`) on `dialog_head`/`checkbox`/`dialog_actions`, with Tools' copy.
- Tray: use `tray_icon_rgba` for the icon, left click opens the overlay, rename "Open Settings" to
  "Settings", and add a separator before Restart.
- Settings window focus on a second launch (service touches `settings.focus`, the window listens).
- Words ("Couldn't …" everywhere, "Copied" toasts).
- `dev.arcade.Find` folders with the verified migration, the desktop entry, and the README and
  packaging icons from `assets/app-icon/`.
- Project standard: rust-toolchain.toml, `docs/dev/` lowercase, `scripts/check`/`dev`/`package`,
  `tag.yml` → `release.yml` with a beta run first, and `e2e-linux.sh` honouring TMPDIR.
- Version 0.5.0, CHANGELOG, screenshots, PR, 3-OS CI, release, and the Tools update check.

## Release status (2026-10-18)

- Latest release: [v0.4.3](https://github.com/qa-p1/Arcade-Find/releases/tag/v0.4.3), published 2026-10-19 (only programs from another image drop the AppImage's descriptors; see CHANGELOG.md). The CI and e2e notes below were recorded for the earlier v0.3.0.
- Previously released: [v0.4.2](https://github.com/qa-p1/Arcade-Find/releases/tag/v0.4.2), published 2026-10-19 (released every inherited descriptor at start, which also cut off helpers from the app's own image).
- Previously released: [v0.4.1](https://github.com/qa-p1/Arcade-Find/releases/tag/v0.4.1), published 2026-10-18 (fixes for what verifying the release before it found).
- Previously released: [v0.4.0](https://github.com/qa-p1/Arcade-Find/releases/tag/v0.4.0), published 2026-10-18 (fixes from the October audit and stress test).
- Previously released: [v0.3.1](https://github.com/qa-p1/Arcade-Find/releases/tag/v0.3.1), published 2026-10-16.
- Previously released: [v0.3.0](https://github.com/qa-p1/Arcade-Find/releases/tag/v0.3.0) with a Linux self-installing AppImage, a per-user Windows installer, a macOS dmg, `arcade-release.json` and `SHA256SUMS.txt`.
- Merged to `main` from PR #2 (706dad1) after CI passed on Linux, Windows and macOS: [run 38600212984](https://github.com/qa-p1/Arcade-Find/actions/runs/38600212984). That covers the build, tests, clippy or analyzers, installer smoke tests on each OS, and the Linux AppImage install, repair and uninstall check in a temporary profile.
- Ecosystem e2e (Arcade Link `tools/e2e.py`, real CI builds of all eight apps): install, tray hosting (hosted by Tools, own icon back within 1 s after Tools is killed, no flash on restart) and shortcuts pass. Tools 0.2.0 adopts, updates, rolls back, uninstalls and backs up Find.
- Not yet verified: a real Windows or macOS desktop (CI runners only), and the owner's real-desktop checks. Builds are unsigned, so SmartScreen and Gatekeeper ask for approval.

Updated 2026-10-10. Version 0.2.1, released from `main` on `qa-p1/Arcade-Find`
(AppImage, Windows installer, universal DMG; unsigned).

## Arcade Tools hub integration (2026-10-12, branch `arcade/tools-hub`)

Find 0.3.0 (unreleased) uses Arcade Link tag `v0.3.0` (`33650a8`). It implements
the seven optional hub methods (Rescan index as its tray extra), the
`TrayHostWatcher` with `status.tray`, the manifest `menu`/`logs`/`docs`/`install`
fields, the self-installing AppImage with `--install`, `--uninstall`, `--repair`,
`--integration-status` and `--restart`, the shared per-user Inno installer
with receipt and PATH shim, the macOS move-to-Applications prompt, portable
settings export/import, and `docs/user/`.

- CI run [38279371804](https://github.com/qa-p1/Arcade-Find/actions/runs/38279371804),
  2026-10-12, all green on Rust 1.88: fmt, clippy `-D warnings`, docs/shortcut
  and vendored-checksum checks and the tests (107 pass on Linux, isolated
  profile) on Linux, Windows and macOS; the headless sway + Xvfb end-to-end
  job against Link v0.3.0 mock peers; AppImage silent install in an isolated
  profile, Windows per-user silent install/uninstall, DMG build and package
  smoke tests.
- Windows and macOS were exercised in CI only. The hub features have not been
  run on a real desktop yet; that check comes with the released packages.

## Earlier evidence (0.2.1, Arcade Link v0.2.0)

Local runs: Linux x86_64 container, 4 vCPU, 16 GiB, Rust 1.99, release
builds unless noted. CI (`.github/workflows/ci.yml`) runs fmt, clippy
`-D warnings` and the tests on Linux, Windows and macOS, the headless e2e,
and builds the AppImage, Inno installer and universal DMG.

| What | How | Result |
|---|---|---|
| Unit and integration tests | `cargo test --workspace` | 48 (find-core) + 36 (app) + 5 (`tests/link_shelf.rs`) pass; also on Windows and macOS in CI |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` for Linux, Windows and macOS targets; `cargo fmt --check` | clean |
| End to end, Wayland layer shell | `scripts/e2e-linux.sh` in headless Sway 1.9 | pass: indexing, search, filters, hidden files, live add/remove, single instance, one-shot `find.search`, overlay shows centered, keys (Down, Tab, Esc), Enter → `look.preview` and Find hides, mixed selection → `shelf.add` and Find stays open, `--quit` |
| End to end, Wayland without layer shell (GNOME's path) | same script in headless Sway with `ARCADE_FIND_BACKEND=winit`, the window floating | same checks pass, plus dragging a result onto the real Shelf |
| End to end, X11 | same script in Xvfb (no window manager) | same checks pass |
| Link peers | `arcade-link mock` (Arcade Link v0.2.0 CLI) as Look; the **real Arcade Shelf** (`ARCADE_SHELF_BIN`, v0.1.0) | 71/71 (59/59 with mock peers; debug build): a mixed selection lands in Shelf's store as references; a stopped Shelf is launched from its manifest and takes the add; Shelf's `find.show` opens Find with that file selected; results dragged onto Shelf's window land as references from the layer-shell overlay, the winit window on Wayland, and X11 ([SHELF.md](SHELF.md)) |
| Ecosystem runner | Arcade Link `tools/e2e.py --only find` (onboarding branch) | 3/3: `find.search` resident and one-shot, `find.show` as Shelf sends it, results into the real Shelf |
| Shelf's own UI | Arcade Shelf `tst_Ui::actionsMenuDrivesFind` | Clicking Shelf's Actions menu sends `find.show` and `find.search` to a peer serving Find's real manifest; the match lands on the shelf |
| Settings window | launched under Xvfb, General and Connected apps pages inspected | renders; ripgrep detected |
| Overlay design | `arcade-find --snapshot DIR` (18 PNGs, dark and light, 1× and 1.5×) | inspected |
| AppImage | `packaging/linux/build-appimage.sh` (appimagetool 1.9.0, SHA-256 pinned) | built (8.2 MB); `--version` and `--arcade-manifest` run from it; the manifest advertises the AppImage path |
| Release manifest | vendored `tools/arcade-release.py` | `arcade-release.json` + `SHA256SUMS.txt` generated for the AppImage |
| Vendored files | `scripts/check-vendored.sh` | match Arcade Link v0.2.0 |

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
  macOS (nothing was run interactively). GNOME's path (the winit window on
  Wayland, dragging included) ran on headless Sway, not on Mutter. In that
  session the selection was not extended by keyboard: modifiers from
  `wtype`'s short-lived virtual keyboards don't reach winit there. The Windows installer and macOS DMG
  are built only by CI.
- Global shortcut registration (native and Hyprland runtime bind) on a real
  session; tray on a real StatusNotifier host; start-at-login entries at an
  actual login.
- Real peers other than Shelf: Arcade Look, Box, Wheel, Clipboard, Lens
  builds (mock Look only). Lens's **Search in Find** entry is tested in Lens
  against a fixture manifest, not against a running Find.
- A real (cold-cache) crawl of a large home directory; inotify limit
  degradation on a real system (unit-tested only).
- IME input (no text-input protocol support yet); right-to-left text.

## Not done

- Windows USN-journal helper (optional in the brief): not implemented;
  Windows uses ReadDirectoryChangesW plus rescans.
- No exe icon resource on Windows (the installer and shortcuts carry the icon);
  macOS app not signed or notarized.
- Cancelling a running peer call from Find.

## Documents

[README](../README.md) · [Architecture](ARCHITECTURE.md) ·
[Arcade Link](ARCADE_LINK.md) · [Hyprland](HYPRLAND.md) ·
[Decisions](DECISIONS.md) · [Arcade Shelf](SHELF.md) ·
[Changelog](../CHANGELOG.md) · [Vendored](../VENDORED.md) ·
[Third-party notices](../THIRD_PARTY_NOTICES.md)
