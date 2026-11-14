# Spec 6: Arcade Find 1.0.0

**Outcome:** Find 1.0.0. It delivers:
- Escape clears before it closes;
- the CLI is script-safe (honest exit codes and errors);
- content search handles every filename;
- the popup always fits the screen;
- Settings tell the truth for the current desktop.

Starting point: `qa-p1/Arcade-Find`, Find 0.5.4. Read `AUDIT/find.md`, `AUDIT/real-desktop.md` §Find, and
`AUDIT/FINDINGS.md` F1 (downgraded: shutdown noise; Find re-registers).

## Findings to fix

| ID | P | What | Where |
|---|---|---|---|
| FD-7 / RD-FIND-1 | P1 | One Escape with text typed hides the popup and drops the query. The first Escape clears the text; the second closes. Update the locking test at `ui/model.rs:984` and the footer hint. | `crates/arcade-find/src/ui/model.rs:982-990` |
| FD-2 | P1 | On X11 the popup hides 150 ms after focus leaves (the fleet popup rule). Wayland is fine thanks to the exclusive keyboard grab. | `crates/arcade-find/src/ui/desktop.rs:378,443-446`, `ui/mod.rs:24`, `ui/wayland.rs:581` |
| FD-3 | P1 | Content search misses files with a newline in their name. Use NUL-separated ripgrep output. | `crates/find-core/src/content.rs:134-207` |
| FD-1 | P1 | `--json` failures exit 0. | `crates/arcade-find/src/main.rs:120` |
| FD-5 | P1 | An invalid filter next to a valid term is silently dropped (`size:>1GBB ext:txt` returns 0-B files). Report the error. | `src/service.rs:1029-1045`, `main.rs:143-159` |
| FD-4 | P1 | Allowed row counts can push the window off-screen. Clamp to the work area and scroll. | `crates/find-core/src/settings.rs:69-79,210-211`, `ui/desktop.rs:165-201` |
| FD-6 | P2 | `--limit 0` returns one result; limits over 1000 are clamped silently. | CLI |
| FD-8 | P2 | A stale "Some folders can't be watched" message, and memory not returned after removing a huge location. | index |
| FD-9 | P2 | The Hyprland binding section shows on every Linux desktop. | `settings_ui.rs:959` |
| FD-10 | P2 | Locations accepts `/` and nested duplicates without a warning. | settings |
| RD-FIND-2/3/4 | P2 | "Start at login" ON while the text says Hyprland ignores autostart; drives shown by raw UUID path; "Took 0 s" for a 500 ms scan. | settings / index |

## Memory

Find costs about 46 PSS / 51 RSS in the background, 22 MB fresh, and 68 MB after indexing and then removing `/`.
Items U1, W0 and FD1 (`shrink_to_fit` after load or rescan). Budget: background 42 PSS / 48 RSS. Apply the Find items from `AUDIT/memory.md`: release index memory after a location is removed, plus the startup trim.

## CI

Measured: main CI takes 7–82 minutes. Apply `PLAN.md` step 6.

## Acceptance

All `PLAN.md` exit gates, plus:
- An Escape ladder test.
- A CLI exit-code matrix.
- A newline-filename content test.
- Window placement at 20 rows on 1080p.
- Automated drag and drop of a result row to a target (the audit couldn't drive DnD with a virtual pointer).
