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
