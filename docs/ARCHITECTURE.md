# Architecture

Two crates:

| Crate | What |
|---|---|
| `crates/find-core` | The index, crawler, watchers, query language, matching and ranking, frecency, settings and paths. No UI, no networking. |
| `crates/arcade-find` | The app: CLI, single instance, service, overlay (model, renderer, two window backends), Arcade Link, shortcut, tray, start at login, Settings window. A library (`src/lib.rs`) plus the binary (`src/main.rs`). |

## Process and threads

One resident process per profile (`instance.rs`: an exclusive lock on
`<runtime>/instance.lock`, a local socket or named pipe, a 32-byte token in
`instance.json`). A second launch sends its command as one JSON line and
exits. Settings (`--settings-window`) and one-shot Link calls
(`--arcade-invoke`) are short-lived separate processes.

Threads in the resident, all blocked while idle (measured: 0 CPU ticks over
30 s with 1M entries indexed):

| Thread | Blocks on |
|---|---|
| main (UI) | the window system (calloop for layer shell, winit elsewhere) |
| `find-engine` | a condition variable with a deadline: debounced watch batches, delayed saves, the periodic rescan |
| `find-search` | the next search request (only the newest is run) |
| watcher (inotify / notify) | the kernel |
| Link accept + registry watch | `accept` / directory notifications |
| `find-instance` | `accept` on the instance socket |
| tray, clipboard owner, Hyprland reload listener | D-Bus / a channel / Hyprland's event socket |

Crawls, content searches (ripgrep), peer calls and file operations run on
short-lived workers; the UI thread never touches the disk or IPC (the action
list is built from the cached registry).

## Index (`find-core/src/index.rs`)

A struct of arrays indexed by entry id: `parent: u32`, `name_off: u32`,
`name_len: u16`, `flags: u8` (dir, hidden, symlink, deleted, root),
`size: u32` (exact below 2 GiB, then in KiB up to 2 TiB), `mtime: u32`, plus
one contiguous name arena. Parents always come before children, so paths are
rebuilt by walking up, and per-folder state (e.g. `in:` filters, path-word
matching) is computed in one forward pass. Folders are found by an FNV path
hash. Deletions are tombstones, compacted when they pile up.

Persisted as `index.bin` (magic, format version, JSON header with roots and a
settings fingerprint, the arrays, CRC32), written atomically. At 1M entries:
38.5 MiB on disk, ~33–37 ms to load.

## Crawl and live updates

`scan.rs` walks roots with a work queue on a few threads at low priority
(`nice 10` and the lowest best-effort I/O priority on Linux), skipping excluded names, excluded and
system paths, network mounts and already-visited `(device, inode)` pairs;
symlinks are recorded but never followed. Results are usable while the first
crawl runs (progress in the bar). A saved index is reconciled against the
disk in the background after loading.

`watch.rs`: inotify on Linux (one watch per folder; on `ENOSPC` it degrades to
"limited", reports the limit and the command to raise it, and rescans hourly);
FSEvents (macOS) and ReadDirectoryChangesW (Windows) through `notify`,
watching roots recursively. Changes are debounced 150 ms and applied by
re-listing only the affected names. A full reconciling rescan runs every
`rescanHours` (default 6).

## Search (`query.rs`, `matcher.rs`, `search.rs`)

The query is parsed into name words, path words, globs and filters. Each
search splits the arena across up to 8 threads; when a word contains a
character every match must have, `memchr` over the arena finds candidates
without visiting the rest. Each thread keeps a bounded top-K heap; the depth
penalty is computed only for entries that can make the cut. Ranking: exact >
stem > prefix > word boundary > substring > fuzzy, plus coverage, recency,
depth, and local frecency (applied to the few boosted entries after the scan).

While typing, `Narrowing` keeps the previous query's matches and near-matches
(fuzzy matches below the score floor): when the new query only extends the
words, only those are re-checked. This is exact (a test compares it with full
searches).

## Overlay (`ui/`)

`model.rs` is the whole behavior (keys, selection, modes, actions) with no
window system; backends feed it events and carry out its `Effect`s through
`service.rs`. `render.rs` draws it with tiny-skia and cosmic-text into a
pixmap: no GPU context. Backends:

- `wayland.rs`: wlr-layer-shell on the overlay layer, no anchors (the
  compositor centers it, so it grows symmetrically), exclusive keyboard while
  shown, destroyed on hide (focus returns). Fractional scaling through
  `wp_fractional_scale_v1` + `wp_viewporter`.
- `desktop.rs`: winit + softbuffer (X11, Windows, macOS, GNOME Wayland): a
  borderless always-on-top window, centered and re-centered as it grows,
  hidden on focus loss.

`--snapshot DIR` renders every state to PNG without a window.

## Service (`service.rs`)

Owns the engine, frecency, pins, the Link presence and registry, Box's cached
pipelines, the search worker, and the overlay's narrowing state. Messages to
the UI thread are `UiMsg`s through a channel the backend owns.

## Platform integration

| | Linux | Windows | macOS |
|---|---|---|---|
| Shortcut | Hyprland runtime bind; X11 native; other Wayland: manual `--toggle` | native | native |
| Tray | StatusNotifierItem (ksni) | tray-icon | menu bar (tray-icon) |
| Start at login | `~/.config/autostart/arcade-find.desktop` | HKCU `Run` value `ArcadeFind` | `~/Library/LaunchAgents/arcade.find.plist` |
| Reveal | `org.freedesktop.FileManager1.ShowItems`, else open the folder | `explorer /select,` | `open -R` |
| Trash | `trash` crate (freedesktop trash) | Recycle Bin | Finder Trash |
| Theme | portal `color-scheme` (watched) | `AppsUseLightTheme` | `AppleInterfaceStyle` |

## Dependencies

Rust core; `resvg`/tiny-skia and cosmic-text for drawing; smithay-client-toolkit
(Wayland), winit + softbuffer (others); eframe/egui for Settings only;
`global-hotkey`, `ksni`/`tray-icon`, `arboard`, `trash`, `open`, `interprocess`;
`arcade-link` v0.2.0 for Link. No browser engine, no runtime downloads.
ripgrep is optional and detected.
