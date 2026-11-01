# Status

## 2026-10-31 (night): PR #11 merged; CI no longer publishes on tags

PR #11 merged as `8d25c46` (CI run 41233227380 green on head `5d85b0f`).

- The first beta rehearsal (Release run 41234323506) failed at startup. release.yml calls
  ci.yml from a job with `contents: read`, and ci.yml still had its old tag-push `publish` job
  asking for `contents: write`. GitHub checks nested permissions before any job runs, even for
  jobs that would be skipped. release.yml is the only publisher now, so ci.yml lost the `v*` tag
  trigger and that job, matching Box, Look and Clipboard. Tags made by tag.yml use the workflow
  token, so they never triggered that job anyway.

## 2026-10-31 (evening): both run-106 failures diagnosed and fixed; awaiting CI

Resumed PR #11 from GPT's checkpoint (`5103750`). Not merged or released.

- **Upgrade check (`check-upgrade.py:112`)** — real bug, in Link. `cleanup_duplicates` keeps
  owned launchers that run the *same* executable, and 0.4.3 and 0.5.0 both install to
  `~/Applications/Arcade/Arcade-Find.AppImage`, so `arcade-find.desktop` stayed next to
  `dev.arcade.Find.desktop`; the old login entry was never looked at either (Find would have
  started twice at login). Fixed in Link PR #27 / **v0.4.12** (merge `aab6569`): install replaces
  the launcher and login entry the old receipt recorded under another desktop id, carries Start
  at login and the desktop's switched-off flag over, and leaves an entry another app now owns.
  New Link regression test fails on the old code and passes now. Find pins v0.4.12.
- **Copy path (`os-handoffs.py:95`)** — test bug, not an app bug. `--show 'handoff ext:txt'`
  opens with the query selected, and Ctrl+C on selected search text copies that text, as
  documented in `docs/user/shortcuts.md` and covered by the `copy_prefers_text_selection` unit
  test. The test now presses End first (like a user who typed the query) and its failure message
  includes what the clipboard actually held.
- Exact detail timestamps use Link's new `format_datetime`; `find_core::fmt` stays public for
  compatibility.
- Settings key list: the reader now follows the shortcut schema (a binding can be a list; an
  explicit `null` means none on that platform, previously it fell back to the default keys on
  macOS). "Confirm moving items to Trash" is one row with Enter or Delete.
- Reviewed the run-106 Settings captures (all six pages; light 760×560 and dark 720×520 at
  125 %): no layout defects beyond the duplicate Trash row above.
- Local: docs, vendored (v0.4.12), fmt, clippy `-D warnings`, all workspace tests, 190 contrast
  pairs pass. Native and upgrade checks need CI.
- **macOS runners**: runs 107 and 108 (both attempts) had `check (macos-14)` cancelled before a
  runner started. The job annotation names the cause: a scheduled macOS 14 image brownout; GitHub
  retires macOS 14 on 2026-11-02 (actions/runner-images#13518). CI now uses `macos-15` for checks
  and the universal DMG, builds with `MACOSX_DEPLOYMENT_TARGET=11.0` (the app's
  `LSMinimumSystemVersion`), and the DMG step fails unless both slices require macOS 11.0.
- **Run 109** (first run with macOS back): two test bugs, both reproduced locally and fixed.
  - `os-handoffs.py:109` "Open with default app": Ubuntu 24.04's xdg-utils 1.1.3 word-splits the
    handler's `Exec` without honouring quotes, so the test's quoted helper path reached python3
    with its quote characters and the handler never ran. Find's open worked (no error logged;
    `xdg-open` and `gio open` both launch the handler with the exact Unicode path once the helper's
    `Exec` is unquoted). The test now writes a plain helper path and asserts it needs no quoting.
  - `check-upgrade.py:109` login entry: the file existed; the check required the text
    `dev.arcade.Find` in it, which only the launcher has (Icon, StartupWMClass). The check now
    asserts each entry's own lines: launcher icon, window class and owner; login entry owner and
    `Exec` of the installed copy with `--background`. Real 0.4.3 → this branch upgrade passes
    locally under Link's e2e isolation.
- **Run 110**: upgrade, packages and manifest green. `accessibility.py` ran for the first time in
  these runs (it comes after `os-handoffs.py`; the `accessibility/` files in run 109's artifact
  were stale, restored with the cached `target/`). It typed with xdotool without giving Find's
  window the keyboard; Xvfb has no window manager and X focus follows the pointer, so keys
  reached Find only when its window happened to be under the pointer. The test now focuses the
  window first, as `os-handoffs.py` does. Reproduced locally (old test fails the same way; fixed
  test passes with the 2, 1 and 0 result announcements). Also ran locally, all passing:
  background-notice, check-native (53 screenshots), check-install-ui, tray-registration (9 checks),
  real-peers with the released Look 0.4.0 and Box 0.4.0.

## 2026-10-31: paused by owner; not release-ready

Implementation is preserved in PR #11 at `843a8cd2ee2bbbbb5554506d7fe64a1ec67d27d1`.
Latest release is still v0.4.3; proposed 0.5.0 is neither merged nor released.
Run 41169413705 passed three-OS source checks, Settings captures and Windows/macOS
installers, but failed native Unicode Copy path and old desktop-entry removal
in the previous-release upgrade. Root causes remain undiagnosed. No final
acceptance or Tools update verification is claimed. See the complete
[handoff](dev/handoff-2026-10-31.md) for implemented work, exact failures, evidence,
release sequence and all remaining checks. Older entries below are historical.


## 2026-10-31: final acceptance gates

PR #11 remains open; no new release has been published. Run 41053930348 passed
all three OS source suites, the 96-view Settings matrix, Windows/macOS installer
checks and the AppImage install/repair/uninstall check. The new native AT-SPI
regression stopped at an incorrect test bus object path; the package upgrade
correctly refused to install a build still under the checkout's `.git` tree.
Run 41168494645 confirmed native result announcements, complete names and safe
focus. Its next assertion requested AT-SPI EditableText, which the pinned Unix
adapter does not implement; the test now uses its supported focus/Text interfaces
and real keyboard search. The upgrade now reaches the settings check: installation
reserializes defaults and Start at login, so every old setting value is checked
while pins/history remain byte-for-byte checks. Both fixes require a green run.

The remaining standard code now includes Quit in About, a one-time background
notice after Settings closes, and waiting for resident command acknowledgments
before the Settings helper exits. Native notice/Quit checks are being added.
The bundled Inter OFL licence is synced unchanged from Link and included in the
shipped third-party notices. No unfinished native check is called passed.


## 2026-10-30: Find 0.5.0 final verification in progress

Continuing PR #11 on `claude/arcade-overhaul-q3xegk`. Not merged or released.

- Run [41045708438](https://github.com/qa-p1/Arcade-Find/actions/runs/41045708438)
  passed every job: Linux, Windows and macOS source checks/tests; 96 real
  Settings captures; native lifecycle/live themes/geometry; 16 install-dialog
  and four welcome captures; real clipboard/file actions; nine tray checks;
  eight Wayland fallback mapping/recreate/shutdown cycles; real Shelf handoff,
  background relaunch and file drops on all three backends; 72 end-to-end
  assertions; all three installers and the release manifest.
- Later run [41049863733](https://github.com/qa-p1/Arcade-Find/actions/runs/41049863733)
  passed three-OS source checks, the Settings matrix and Windows/macOS installer
  checks. Native long-name tooltips passed in both themes at 125%; both captures
  were inspected. The real Look 0.4.0 preview also worked. The added real Box
  conversion check needs libvips in its runner; the new previous-release upgrade
  check exposed a missing XKB X11 library. Both prerequisites are repaired in
  CI; neither new check is claimed passed yet.
- Find now pins and syncs Link v0.4.11 at
  `047b686d9b02de6dcf7bdfa77c85bf7eefbf7264`. Shared fixes shipped through Link
  PRs #22–#26: measured row layout, disabled-text and checkbox contrast, welcome
  and disclosure controls, and the neutral welcome brand mark. Link's PR and
  merge checks were read before its immutable tags were dispatched.
- The overlay now supplies a native accessibility tree on both backends, full
  names/paths, editable search, safe focus actions and polite result-count
  announcements. Settings enables eframe's AccessKit support. Native AT-SPI
  evidence is pending; compilation alone is not a screen-reader pass.
- Full native/IPC checks cannot run in this local environment: private Unix
  sockets return EPERM. CI runs those checks in Link's isolated D-Bus/Xvfb and
  headless Sway runner. No local native pass is claimed.

Still required: the expanded suite green on the final head, screenshot review
and refreshed guide images, complete spec acceptance evidence, ready PR and merge,
a verified draft beta through the new release workflow, stable release and real
Tools 0.4.1 update-decision/download verification.

Physical-desktop checks remain explicit: Hyprland global shortcut, placement,
warm-open latency, focus/tray interaction, fcitx5/dead-key input; Windows/macOS
login items, macOS permission re-grant, Windows AppUserModelID; and an actual
owner-machine update through Tools. CI cannot substitute for those desktops.

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
| Link peers | `arcade-link mock` (Arcade Link v0.2.0 CLI) as Look; the **real Arcade Shelf** (`ARCADE_SHELF_BIN`, v0.1.0) | 71/71 (59/59 with mock peers; debug build): a mixed selection lands in Shelf's store as references; a stopped Shelf is launched from its manifest and takes the add; Shelf's `find.show` opens Find with that file selected; results dragged onto Shelf's window land as references from the layer-shell overlay, the winit window on Wayland, and X11 ([SHELF.md](dev/shelf.md)) |
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

[README](../README.md) · [Architecture](dev/architecture.md) ·
[Arcade Link](dev/link.md) · [Hyprland](dev/hyprland.md) ·
[Decisions](dev/decisions.md) · [Arcade Shelf](dev/shelf.md) ·
[Changelog](../CHANGELOG.md) · [Vendored](../VENDORED.md) ·
[Third-party notices](../THIRD_PARTY_NOTICES.md)
