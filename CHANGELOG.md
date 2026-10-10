# Changelog

## 0.2.0 — 2026-10-10

- Drag results out of the overlay: one row, or the whole multi-selection
  when the drag starts inside it, into file managers, editors or Arcade
  Shelf. Wayland (layer shell) and X11 show a drag image with the first
  name and "+N"; Windows uses the shell's drag loop (Explorer's image and
  cursors); macOS an AppKit dragging session with Finder icons. Copy or
  link only.
- Shift+click extends the selection with the mouse.
- `examples/e2e-pointer` (a virtual pointer for headless compositors) and
  drag checks in `scripts/e2e-linux.sh`: with the real Shelf, two results
  dragged onto its window land there by reference, on Wayland and X11.

## 0.1.0 — 2026-10-10

First version.

- Search overlay on a global shortcut (Ctrl+Alt+F): a compact bar at the
  screen center that grows with results; every keystroke searches.
- Enter previews in Arcade Look (default app without Look); Shift+Enter
  opens; Ctrl+Enter shows in the folder; Space previews from the list.
- Actions: open, show in folder, copy path, copy file, rename (never
  overwrites), move to Trash (never deletes permanently), pin, details, plus
  other Arcade apps' actions for the selection (Tab).
- Query language: words (fuzzy), "phrases", path words, wildcards, `ext:`,
  `dir:`/`file:`, `in:`, `size:`, `modified:`, `hidden:`, and `/text` content
  search with ripgrep when installed.
- Index of names and metadata, saved to disk (warm start in tens of
  milliseconds), first crawl in the background with progress, live updates
  (inotify, FSEvents, ReadDirectoryChangesW) and reconciling rescans; honest
  degradation when the inotify watch limit is reached.
- Ranking by match quality, recency, depth and local frecency; recent and
  pinned items on an empty query.
- Hidden files toggle (Ctrl+H); default excludes; opt-in extra drives;
  network mounts skipped; symlinks never followed.
- Light, dark and system themes; Wayland layer shell (Hyprland first), X11,
  Windows and macOS windows.
- Settings: General, Locations, Shortcut (clash warnings, Hyprland runtime
  binding), Index (status, watch health), Connected apps, About.
- Arcade Link: `find.search` "Find matching files" (also one-shot) and
  `find.show` "Search in Find"; peers' actions offered generically.
- Works with Arcade Shelf both ways: "Add to Shelf" for a selection (a
  stopped Shelf starts in the background), and Shelf's "Search in Find".
  Connected apps lists Shelf with a Get button before it's installed.
- Start at login: after the first run the login entry is the truth, so
  Arcade Tools and Find's Settings can both switch it.
- Tray menu, start at login (installed copies only), single instance,
  standard command line.
