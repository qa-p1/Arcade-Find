# Arcade Find v1 decisions

Decisions the plan left open. Each one cites the part of the catalog `dev/v1/README.md` it follows. Newest last.

## D1 - Spec and audit copied from the catalog

`docs/v1/SPEC.md` is `specs/FIND.md`; `docs/v1/audit/` holds `AUDIT/find.md` and the text evidence from `AUDIT/find/`
(JSON, JSONL and text only, no screenshots). Line references in the spec are for Find 0.5.4.

## D2 - Known-bug tests

A behaviour-lock test for a known bug is `#[should_panic(expected = "KNOWN BUG <id>")]`, and the failing assertion
names the finding. It passes while the bug is there. Once the fix lands the test stops panicking and fails, so the
marker has to go in the same commit as the fix (PLAN.md step 1). The CLI tests in `crates/arcade-find/tests/` use the same marker.

## D3 - Commit trailers

The brief asks for an `Opus 5.5` co-author trailer, while the session's own system instruction names the model that is
actually running. The session instruction wins; the trailer in this branch names that model.

## D4 - The popup no longer hides on focus loss anywhere (FD-2)

The fleet rule allows Escape, a choice, the shortcut again, or an explicit outside click. Neither backend can see an
outside click (the X11 window takes no pointer grab, and a Wayland layer surface only gets input over itself), so the
popup now stays until Escape, a choice or the shortcut. The focus-loss timer is removed from both backends, not just
X11: on Wayland it fired whenever another layer surface or a workspace switch took the keyboard. Adding an outside-click
grab would be a new feature; it is not in the spec.

## D5 - One `--search` path for the running instance and the offline index (FD-1, FD-5, FD-6)

`service::cli_query` and `service::cli_search` serve both. Any filter error fails the search, even next to valid terms;
the GUI keeps showing the same errors as a note. `--limit 0` is a usage error (exit 2). The CLI limit is no longer
capped at 1000 by the running instance (the offline path never was); Link `find.search` keeps its 1000 cap, which is
part of that protocol. Exit codes are documented in `--help`: 0 ok, 1 failure (also with `--json`), 2 usage, 3
`--status` with nothing running.

## D6 - Escape ladder in the results view (FD-7)

Escape steps back: a Shift range selection, then the query text, then the popup. The footer hint names the step it
will take ("Clear selection", "Clear", "Close"). Clearing the query with "Show recent items on open" on shows the recent
list again; the next Escape closes.

## D7 - Row count capped by the screen, not by Settings (FD-4)

The Settings limits (visible 1-20, stretch up to 30) stay as they are. The overlay caps the rows it shows so the panel
plus its shadow margin fits the monitor height (X11: the monitor under the pointer; Wayland: the shortest output,
because the compositor chooses the output). The rest scrolls. At least one row always shows.

## D8 - Locations: nested roots refused, parents replace children, whole disk asks twice (FD-10)

A folder inside an indexed folder is refused ("Already indexed: it's inside ..."). A parent of indexed folders replaces
them. `/` (or a drive root) needs a second Add after a warning about time and memory. Dropped folders follow the same
rules; a dropped drive root is ignored.

## D9 - Start at login text on Hyprland (RD-FIND-2)

Hyprland runs XDG autostart entries only through a session manager (uwsm or systemd's autostart generator). If Find, or
Tools that started it, runs in an `@autostart.service` cgroup, autostart demonstrably works and the normal text shows.
Otherwise the text explains the session-manager condition and the `exec-once` alternative, instead of claiming that
Hyprland never runs autostart entries.

## D10 - Memory after removing a large location (FD-8, memory FD1)

After every full crawl the index returns spare capacity (`Index::shrink_to_fit`) and glibc's `malloc_trim(0)` hands
freed pages back. Since Find pins Link 1.0.7 this is `arcade_link::runtime::trim()`, which also drops clean file pages; the same call runs once 5 s after the resident starts (W0).

## D11 - Untangle (step 2): one place for Find's identity

Hidden link: `"dev.arcade.Find"` was defined twice (`installer::DESKTOP_ID` for the desktop entry, `os::APP_ID` for the
window, tray and notifications), and the desktop matches a window to its launcher only while the two agree; nothing
said they had to. Needless link: the tagline lived in `settings_ui`, so the installer (which writes it as the desktop
entry's `Comment`) and the tray imported the 1,800-line Settings window for one string. The Link id and display name
lived in `link`, and "Arcade Find" was repeated as a literal in the tray, notifications and Settings.

Fix: `identity.rs` holds the Link id, name, OS identifier, CLI name and tagline, and everything reads them there.
The Link id itself is `find_core::paths::LINK_ID`, because `find-core` names Find's config and data folders after it
and sits below the app; `identity::ID` re-exports it. Old ids (`legacy_desktop_ids`) stay literal: they are history,
not identity.

## D12 - Untangle (step 2): one answer to "which desktop session"

Copies: "is this Wayland" was read from `WAYLAND_DISPLAY` in the shortcut (`hotkey`), window placement
(`window_state`) and the overlay (`ui::wayland`); "is this Hyprland" lived in `hotkey::hyprland`, so window placement
imported the shortcut module to ask it, and placement ran its own `hyprctl -j` beside the shortcut's. `session.rs` now
answers `wayland()`, `hyprland()` and `hyprctl_json(query)` for all of them. Left as they are: the Hyprland socket path
(needs the signature's value, not whether it is set) and `main`'s "no display at all" check, which asks something else.

## D13 - Untangle (step 2): two cycles kept on purpose

- `installer` ↔ `autostart`: the login item is part of the install integration (the macOS install writes the login
  plist; enabling start at login reuses the installer's launch path and Windows shortcut). They change together.
- `hotkey` ↔ `service`: `hotkey` is the service's shortcut subsystem. A key press sends `Toggle` to the service and the
  service hands it a new accelerator; a callback layer between them would change whenever both do.

## D14 - Untangle (step 2): the event log and the stderr log were one file

Hidden link and bug: `diagnostics::event` appended to `<logs>/arcade-find.log` and rotated it at 1 MiB (`.1`, `.2`),
while Link's `capture_stderr`, started by `main` with the CLI name, redirects stderr to the same file and rotates it
its own way. An event rotation moved the live stderr log aside (stderr kept writing to the renamed file) and a later
one deleted it; Link's start-up rotation overwrote the events' `.1`. Events now go to `events.log` in the same folder
(test: `rotating_the_event_log_leaves_the_stderr_log_alone`, which fails on the old code). `find-core`'s unused
`log_file()`, a third copy of the name, is gone. Also: the user-guide URL moved to `identity`, so the resident no longer
imports the Settings window for it.

## D15 - Dead code (step 4)

rustc and clippy never flag `pub` items of a library crate, so the search was done by hand: every `pub fn`, `struct`,
`enum`, `const`, `static`, `trait`, `type` and `union` in `arcade-find` and `find-core` was looked up as a whole word
with `git grep -w` over the entire repository (both crates with their tests, examples, benches and `build.rs`,
`tests/`, `scripts/`, `tools/`, `packaging/`, workflows and docs). `git grep` reads the `cfg(windows)` and
`cfg(target_os = "macos")` branches as text, so code only those platforms compile counted as a use.

Removed (the only hit was the definition):

- `link::apps::pitch`, `os::paste_text`, `theme::setting_name` (`arcade-find`).
- `Engine::request_save`, `Index::root_of`, `matcher::fuzzy_ascii`, `Query::has_path_terms` (`find-core`). With
  `request_save` gone nothing could set `Work::save_now`, so the field and its branch in the worker loop went too; the
  periodic and shutdown saves call `Engine::save` directly.
- `tools/release-notes.py`: no workflow, script or doc runs it (releases link to `CHANGELOG.md`).

Kept on purpose:

- `fmt::datetime`, `matcher::score_name`, `persist::read_header`: used only by their own unit tests. They are small,
  tested API of `find-core`; removing them means rewriting tests for no gain.
- `crates/arcade-find/examples/hub-startup.rs` and `crates/find-core/examples/typing.rs`: measurement tools that no
  script names. Cargo builds them (clippy `--all-targets`), and their headers say how an isolated runner uses them.
- `tests/desktop/*`, `tools/third_party_notices.py` (regenerates `THIRD_PARTY_NOTICES.md`, run by hand), and the
  Cargo-discovered tests and benches.
- Vendored files in `VENDORED.md`, the design-kit assets, and `scripts/verify-tools-update.rs`.
- Feature flags: neither crate declares a `[features]` table, so none can be unused. The arcade-link features in
  `arcade-find`'s Cargo.toml are Link's own and `cargo machete` reports no unused dependency.

## D16 - Memory (step 5): the resident runs from the unpacked copy

AUDIT/memory.md lists three items for Find: W0 (trim after startup), FD1 (shrink the index after a load or
rescan) and U1 (the unpacked runtime). W0 and FD1 landed with the fixes (06f86f8, 51c69a0); a removed location's
index memory goes back to the system the same way (FD-8). U1 now too: a resident start in an AppImage relaunches
from `<data>/arcade/runtime/find-<version>-<key>/` through Link's `runtime::relaunch_unpacked` and the launcher exits
0, so no FUSE helper stays resident (RT-7). One-shot calls (`--search`, `--status`, `--quit`, and a command a
running instance takes) never relaunch. The budget (background 42 PSS / 48 RSS) is measured on the published rc
by the gates' budget job.

