# Changelog

## 0.5.0 — 2026-10-30

- New lime accent, Inter and the shared Arcade kit across search, Settings and the install prompt.
- Settings uses the standard page order, fixed headings, readable folder rows, a single helper window and ordered saves that finish before close.
- Composed input is supported through winit IME and Wayland text-input-v3.
- Standard application identifiers and verified migration of existing folders, desktop and login entries. Existing shortcuts and preferences stay.
- Monochrome tray glyph, left-click search, standard menu wording and shared Copied feedback.
- Shared developer entry points, pinned Rust, lowercase developer docs and manual release preparation.

## 0.4.3 — 2026-10-19

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.
- Restarting no longer keeps the previous image mounted.

## 0.4.2 — 2026-10-19

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.

## 0.4.1 — 2026-10-18

- Updating or installing from your Downloads folder, restarting into the
  background and starting other Arcade apps no longer leave the old AppImage
  mounted and its runtime (about 20 MB) running. A program the app started
  inherited an open handle on its AppImage mount and kept it busy.

## 0.4.0 — 2026-10-18

- Changing Find's shortcut over Arcade Link (for example from Tools) takes effect
  right away; before, the new chord did nothing until Find was restarted.
- Date filters stay correct across daylight-saving changes, and content search
  keeps to the folders you chose.
- Installer: updating replaces the running copy, uninstalling right after start
  works, and the install prompt matches the other Arcade apps (centred, Install
  on the right, closing it installs nothing, Start at login ticked). Upgrading
  from a copy installed before the shared installer names that copy and takes
  over its menu and login entries.
- Built against Arcade Link commit `2fc7c91`.

## 0.3.1 — 2026-10-16

- On X11, restoring a Tools backup (or saving Settings) with the global
  shortcut Find already holds no longer fails with "Ctrl+Alt+F is already
  used by another application". Find keeps its existing registration instead
  of grabbing the same key again from a new hotkey manager while the old one
  is still releasing it. Found by the Arcade Link real-fleet backup check.

## 0.3.0 — 2026-10-14

- Arcade Tools can open Settings, restart Find (with busy/force handling),
  rescan from its hosted menu, change the global shortcut live, and export
  or import portable settings. Imports validate and back up settings/pins;
  the index, history and local integration preferences are excluded.
- Live tray hosting with a bounded startup gate and immediate hand-back
  after Tools exits or crashes; Link off keeps Find’s own icon.
- A shared first-run AppImage installer, receipt, launcher/actions, hicolor
  icons, terminal command and optional login integration. Added --install,
  --uninstall, --repair and read-only --integration-status. Restarts and
  re-execution use the stable AppImage path. No shell profiles are edited.
- Per-user Inno installer with optional shortcuts/startup, PATH shim and
  receipts; macOS move-to-Applications, quarantine-preserving copy and CLI
  offer. Native Windows/macOS execution is not verified on a real desktop.
- Manifest menu, docs, logs and install metadata; small rotating event logs.
- Checksum-pinned shared packaging/release/shortcut tools, generated shortcut
  docs, local doc-link checks and isolated tests on the Rust 1.88 CI matrix.

## 0.2.1 — 2026-10-10

- Dragging out works on GNOME Wayland (and any Wayland session without
  layer shell, where Find uses its winit window): Find runs a second client
  on winit's own Wayland connection, with its own pointer for the press
  serial, and drags a `wl_data_source` from the window, with the drag image.
- The winit window no longer hides when focus drops and returns within
  150 ms (a keyboard device change, a grab); it still hides when another
  window keeps focus. Same rule as the layer-shell overlay.
- `scripts/e2e-linux.sh` adds a `wayland-winit` session (headless Sway,
  `ARCADE_FIND_BACKEND=winit`, the window floating as on GNOME): a result
  dragged onto the real Shelf lands by reference. Waits for Shelf's start
  and stop and for `--quit` poll instead of sleeping.

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
