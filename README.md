# Arcade Find

Press a shortcut, type, and any file or folder shows up instantly.
**Enter** previews it in [Arcade Look](https://github.com/qa-p1/arcade-look)
(or opens it with the default app when Look isn't there).

Arcade Find keeps an in-memory index of your files (names and metadata only),
saved to disk so it's ready in well under a second after login, and kept
current by file-system notifications. It is local only: no network access,
and file contents are read only when you search inside files with `/`.

## Install

| Platform | Package | Where it goes |
|---|---|---|
| Linux (x86_64) | `Arcade-Find-x86_64.AppImage` | `~/Applications/Arcade/Arcade-Find.AppImage` (what Arcade Tools uses) |
| Windows (x64) | `Arcade-Find-<version>-x64-setup.exe` (per-user, no admin) | `%LOCALAPPDATA%\Programs\Arcade Find` |
| macOS 11+ (universal) | `Arcade-Find-<version>-universal.dmg` | `/Applications/Arcade Find.app` (Arcade Tools: `~/Applications`) |

Get them from [Releases](https://github.com/qa-p1/Arcade-Find/releases) or install
with Arcade Tools. Packages are unsigned; the macOS app isn't notarized. Content search needs
[ripgrep](https://github.com/BurntSushi/ripgrep) installed (it is detected,
never bundled or downloaded).

## Use

The default shortcut is **Ctrl+Alt+F** (change it in Settings). The overlay
opens at the center of the screen; at rest it's only the search bar, and it
grows up and down around the center as results arrive.

| Key | Does |
|---|---|
| type | Search (every keystroke) |
| ↑ ↓, PgUp PgDn | Move; with Shift, select several |
| **Enter** | Quick Look in Arcade Look (default app without Look) |
| Shift+Enter | Open with the default app |
| Ctrl+Enter | Show in the folder |
| Space | Quick Look (after moving into the list; while typing it types a space) |
| Tab | All actions for the selection, including other Arcade apps' |
| F2 | Rename (never overwrites) |
| Delete | Move to Trash (asks first; never deletes permanently) |
| Ctrl+C / Ctrl+Shift+C | Copy path / copy the file |
| Ctrl+Shift+P | Pin (pinned and recent items show on Down with an empty query) |
| Ctrl+H | Show hidden files |
| Ctrl+I | Details |
| Ctrl+, | Settings |
| Esc | Close (focus returns to the previous app) |

On macOS, Cmd replaces Ctrl.

### Query language

| Example | Finds |
|---|---|
| `report 2024` | Names containing both words (any order; fuzzy) |
| `"q3 report"` | The exact phrase |
| `src/main` | Paths containing `src/` followed by a name starting with `main` |
| `*.tar.gz` | Wildcard on the name |
| `ext:pdf,docx` | By extension |
| `dir:` / `file:` | Only folders / only files |
| `in:~/Projects` | Inside a folder |
| `size:>100mb`, `size:1k..10m` | By size |
| `modified:<7d`, `modified:today`, `modified:>=2026-01-01` | By date |
| `hidden:yes` | Include hidden files for this query |
| `/TODO ext:rs` | Inside files (ripgrep) |

## Settings

General (theme, start at login, rows, content search), Locations (folders to
index; extra drives such as `/mnt/Data` are opt-in; excluded names and
folders; network drives skipped), Shortcut, Index (status, live-update
health, rescan), Connected apps, About.

Excluded by default: `.git`, `node_modules`, `target`, `.cache`,
`__pycache__`, `.venv`, `.snapshots` (Btrfs), `/proc`, `/sys`, `/dev`,
`/run`, the Trash (Linux, macOS) and network mounts. Symbolic links are never
followed, so nothing is indexed twice.

On Linux, live updates use inotify. If `fs.inotify.max_user_watches` is too
low for your folders, Find watches what it can, rescans the rest every hour,
and shows the command to raise the limit (it never changes system settings).

## Command line

```text
arcade-find [--show [QUERY] | --toggle | --hide | --background | --settings |
             --search QUERY [--limit N] [--hidden] [--json] | --status |
             --rescan | --restart | --quit | --version |
             --arcade-manifest | --arcade-invoke]
```

`arcade-find --toggle` is what desktop shortcuts should run where Find can't
register one itself (see [docs/HYPRLAND.md](docs/HYPRLAND.md) for Hyprland).

## Platforms

| | Status |
|---|---|
| Linux, Wayland with layer shell (Hyprland, Sway, KDE, …) | Overlay, shortcut (Hyprland runtime binding; elsewhere bind `arcade-find --toggle`), tray. Tested in headless Sway. |
| Linux, X11 | Overlay, native shortcut, tray. Tested in Xvfb. |
| Linux, GNOME Wayland (no layer shell) | Normal window; bind `arcade-find --toggle` in Settings → Keyboard. Not tested. |
| Windows 10/11 | Builds (CI); not tested interactively. |
| macOS 11+ | Builds (CI); not tested interactively. |

See [docs/STATUS.md](docs/STATUS.md) for what was verified and how.

## Arcade apps

Find works on its own. With other Arcade apps installed it adds their
actions for your results (Tab), e.g. Shelf's "Add to Shelf", Box's tools,
Wheel's "Add to Wheel", Clipboard's "Send to my devices ↗", and exposes
`find.search` and `find.show` to them: in Arcade Shelf, "Search in Find"
opens Find on an item. See [docs/ARCADE_LINK.md](docs/ARCADE_LINK.md) and
[docs/SHELF.md](docs/SHELF.md).

## Build

```sh
# Linux: sudo apt install libxkbcommon-dev libwayland-dev
cargo build --release -p arcade-find      # target/release/arcade-find
cargo test --workspace
scripts/e2e-linux.sh target/release/arcade-find   # needs sway, grim, wtype, Xvfb, xdotool
python3 scripts/bench_real.py --files 1000000     # whole-app benchmark
```

Rust 1.89 or later. Data lives in `~/.local/share/arcade-find` (Linux),
`~/Library/Application Support/Arcade Find` (macOS) or
`%LOCALAPPDATA%\Arcade\Arcade Find` (Windows). `ARCADE_FIND_HOME` moves
everything (tests), `ARCADE_FIND_PROFILE` keeps a separate profile.

## License

MIT OR Apache-2.0. Third-party licenses: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
Vendored files: [VENDORED.md](VENDORED.md).
