# Arcade Find 0.5.4 audit

Method: installed AppImage, isolated HOME/XDG/runtime/private D-Bus, Xvfb :92 (no WM). Only fixture root `$S/fx/find` is configured, verified by status. Existing scratch fixture contains 20,254 entries / 220 folders; additional known files in `audit/`. S is the user-specified scratch root. Evidence: `find/`.

## Coverage
| Feature | Tested? / method / inputs | Result |
|---|---|---|
| --help/version/status/manifest/background/show | CLI + Xvfb | pass after disposable-home receipt setup |
| Prefix/fuzzy, all words/exact phrase/path/wildcard | CLI: Alpha, AlpRpt, Report 2026, "Alpha Report", audit/Alpha, *.tar.gz | pass |
| ext:, dir:, file:, in:, size:, modified:today | CLI, known 2048-B file and fixture folder | pass |
| Unicode names | CLI: café, résumé, 日本, हिन्दी, 🧪 | pass; search-tests.json |
| Hidden query + excluded node_modules + symlink not followed | CLI | pass |
| Invalid size and CLI content query | CLI JSON errors | FD-1 |
| Arrows/Shift-select/Escape one-step/Tab actions/Ctrl+I details | Xvfb keys + --status UI snapshot | pass |
| 200-result window scrolling/Page/Home/End; pin/recent; rename/trash/copy/paste/open/reveal | enumerated from source/shortcuts | pass; watcher-tests.json / GUI screenshots |
| GUI content search; create/rename/delete/subtree watchers | enumerated | pass; watcher-tests.json / GUI screenshots |
| General/Shortcuts/Locations/Index/Connected apps/About pages; shortcut recorder (Change, Esc cancel, Ctrl+Shift+K accepted, bare key rejected, Reset); theme System/Light (light search + settings) | Xvfb GUI clicks, settings.json diff; evidence find/fs-*.png | pass except FD-9; light/dark both render |
| Locations: add (valid, missing, duplicate, nested, file path, "/"), remove, default excludes (node_modules), live status | Xvfb GUI + CLI search + --status | FD-10 |
| Focus-loss/pointer-leave | Xvfb | FD-2 |
| Escape hierarchy (search results/actions/anchor; settings recorder, settings window) | Xvfb keys + status + source | FD-7; settings Esc closes window, Esc in recorder cancels only recording (pass) |
| Idle and heavy resources: 20,815-entry fixture, then "/" (786k entries) | /proc rss/threads/fd/ticks, --status watch block, inotify fdinfo | 20.8k: RSS 22 MB, 27 threads, 225 watches, 250 ms scan, idle 0.1% CPU. 786k: scan 4.5 s, RSS 69-101 MB, 79,799 watches (limit 524,288), index 38 MB. FD-8 |
| Logs/write locations | find of HOME/XDG after run | pass: only arcade/ dirs, autostart/.desktop/icons from install, mesa shader cache (GPU lib, not Find) |
| Startup time | --background to phase ready with saved index | 0.35 s; --show to visible 70 ms; pass |
| Large result sets (423k matches) | GUI + CLI | count shown, 200 rows reachable; FD-6 |
| CLI edge cases (--bogus, empty query, --limit 0/abc/-1/100000) | CLI | --bogus rc2 and --search "" rc1 pass; FD-6 |

## Findings
### FD-1 — wrong result — JSON search failures exit successfully
- Method: CLI + code reading.
- Steps: `arcade-find --search size:nope --json`; also `--search /needle_AUDIT --json`.
- Expected: invalid query / unsupported operation reports failure with nonzero status for scripts.
- Actual: JSON correctly says `ok:false` but exit status is 0. Non-JSON path checks ok before returning success.
- Evidence: `find/search-tests.json` (both responses and exit codes).
- Source: `crates/arcade-find/src/main.rs:120` (`print_search` returns SUCCESS immediately for JSON before checking `ok`).

### FD-2 — UX — Search popup closes on focus loss and discards the query
- Method: GUI in Xvfb + CLI status, code reading.
- Steps: `--show café`, focus the Find window, wait >400 ms, then `xdotool windowfocus` the private GTK Audit Target window. Wait 0.5 s.
- Expected: owner rule requires the popup to stay open across focus changes.
- Actual: Find hides; status changes to `visible:false`, query becomes empty. Pointer-leave alone stays open.
- Evidence: `find/focus-status.json`, `find/focus-loss-reproduced.png`; `find/pointer-leave.png` for the passing pointer-only case.
- Source: `crates/arcade-find/src/ui/desktop.rs:443-446`, `:378` and `ui/mod.rs:24` schedule Hide 150 ms after focus leaves; Wayland has a matching close path in `ui/wayland.rs:581` (not exercised).

### FD-3 — wrong result — Content search cannot find files with newline in their name
- Method: GUI Xvfb + CLI ripgrep oracle + source.
- Steps: create `$S/fx/find/audit/line\nbreak.txt` (actual newline in basename), containing `NEWLINE_NEEDLE_AUDIT`; show `/NEWLINE_NEEDLE_AUDIT`.
- Expected: one match, just as raw ripgrep finds this existing file.
- Actual: no matches. The raw ripgrep output contains the file name broken across lines; Find treats each as a separate nonexistent path and filters both away.
- Evidence: `find/content-newline.png`, `find/newline-status.json`, `find/newline-rg.txt`.
- Source: `crates/find-core/src/content.rs:134-164` does not request NUL-terminated results, then `:186-207` splits on newline; `arcade-find/src/service.rs:1229` rejects missing paths.

### FD-4 — UX — Allowed row count can put the popup outside the screen
- Method: isolated config + CLI reload, GUI Xvfb geometry/screenshot.
- Steps: set `rows.visible=20`, `rows.stretch=30` in disposable settings (values accepted by Settings::sanitize), `--reload`, then `--show report` with 200 results on 1920×1080.
- Expected: clamp visible height to monitor/work area and scroll remaining rows.
- Actual: 680×1136 window at x620,y-28; top 28 px and bottom 28 px are offscreen. Restored config after test. UI itself offers 3–12 visible / up to 16 stretch; this reproduction uses supported hand-edited bounds, not the narrower dropdown.
- Evidence: `find/tall-window.txt`, `find/twenty-rows-clipped.png`.
- Source: `crates/find-core/src/settings.rs:69-79`, `:210-211` permits 20/30; `crates/arcade-find/src/ui/desktop.rs:165-201` centers logical height without screen-height cap.

### FD-5 — wrong result — CLI silently discards an invalid filter when another valid term exists
- Method: real CLI JSON queries + source.
- Steps: `--search 'size:nope file:' --json --limit 2`; `--search 'size:>1GBB ext:txt' --json --limit 2`.
- Expected: invalid size produces a clear query error; never claim successful size-filtered results.
- Actual: `ok:true`, empty stderr, exit 0, thousands of matches; 0-B / 4-B files returned for the malformed >1GB request. GUI has a note path, CLI loses it.
- Evidence: `find/filter-errors.json` records exact commands/responses.
- Source: `src/service.rs:1029-1045`, which only emits q.errors when q.is_empty. Offline `main.rs:143-159` has the same omission.

### FD-6 — wrong result — CLI `--limit 0` returns one result and `--limit` above 1000 is silently clamped
- Method: CLI on the 786k-entry index.
- Steps: `--search a --limit 0`; `--search a --json --limit 100000`.
- Expected: 0 results (or an error); a requested 100000 either honoured or reported as capped.
- Actual: limit 0 prints 1 path; limit 100000 returns exactly 1000 results with `matched` 381517 and no truncation field/warning. `--limit abc` and `-1` correctly error.
- Evidence: commands in session; outputs reproduced twice. Source: not located (limit clamp in `crates/arcade-find/src/main.rs` / service search).

### FD-7 — UX — Escape with text in the search box closes the popup instead of clearing the query first
- Method: Xvfb GUI, `--show e`, key Escape, `--status`; source.
- Steps: show with query `e` (423k results), press Escape once.
- Expected (owner rule): Escape goes back one step; only an empty top view closes. First Escape should clear the query.
- Actual: one Escape hides the window and discards query and results (`visible:false, query:""`). Footer says "Esc Close", so hint matches behaviour but behaviour breaks the rule. Escape in Actions/details already steps back correctly.
- Evidence: `find/fd-bigset.png` (footer), status output. Source: `crates/arcade-find/src/ui/model.rs:984-990` (results-mode Escape only clears anchor, otherwise Hide); test `escape_hides` at :1539 locks it in.

### FD-8 — polish — Stale watcher message and memory not returned after removing a huge location
- Method: GUI add `/` then Remove, `--status`, /proc.
- Steps: Locations, add `/` (accepted), wait for scan, remove it again.
- Expected: status/Index page back to "Watching 225 folders"; memory drops toward the 22 MB baseline.
- Actual: `watch.detail` still says "Some folders can't be watched (Permission denied...)" although `unwatched:0, mode:full`; RSS stays 68 MB (baseline 22 MB, peak 101 MB) after the index shrank from 786k to 20.8k entries. Allocator retention is likely; the stale text is a real state bug.
- Evidence: `find/fs-after-remove.png`; numbers above. Source: watch detail string set when unwatched>0 and not cleared (not located).

### FD-9 — UX — Hyprland key-binding section is shown on every Linux session
- Method: Xvfb X11 session with HYPRLAND_INSTANCE_SIGNATURE unset (verified in the process environment).
- Steps: Settings, Shortcuts.
- Expected: Hyprland-only controls hidden when `hotkey::hyprland::active()` is false.
- Actual: "Hyprland / Add as a runtime key binding" switch (on by default) and a `bind = ...` line show; copy is wrong for GNOME/KDE/X11 users.
- Evidence: `find/fs-rec.png`. Source: `crates/arcade-find/src/settings_ui.rs:959` gates on `cfg!(target_os = "linux")` only (the active() check is at :822 for another block).

### FD-10 — UX — Locations accepts `/` and nested duplicate roots without warning
- Method: Xvfb GUI.
- Steps: add `<root>/out/extra`, then its child `<root>/out/extra/sub`; add `/`.
- Expected: nested path rejected or merged; `/` warned (whole disk) or refused.
- Actual: both accepted as separate rows (nested does not double-index results, only the UI row is redundant). `/` is accepted at once: 786k entries, 79,799 inotify watches, ~100 MB RSS, with no confirmation. Missing path and a file path show "Couldn't find that folder" (pass; the file-path message wording is misleading).
- Evidence: `find/fs-bad2.png`, `find/fs-try2.png`, `find/fs-addloc.png`.

## Unconfirmed / needs real desktop
Wayland text composition and layer-shell behavior, compositor transparency, tray hosting, drag to a real peer, and cross-OS behavior remain untested. Unclosed quotation currently accepted as a literal query; not counted without a stronger intended contract.
