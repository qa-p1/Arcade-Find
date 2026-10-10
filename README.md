# Arcade Find

Arcade Find opens a local file search with a shortcut. Type a few letters to find files and folders, preview a result, or send it to another Arcade app.

## Install

Install Find from **Arcade Tools → Apps**, or install it by hand using the [getting-started guide](docs/user/getting-started.md).

## Works with other Arcade apps

Find can preview with Look, process results with Box, and send or save selections with other Arcade apps. See [how Find works with other Arcade apps](docs/user/with-other-apps.md).

Read the [Arcade Find user guide](docs/user/) for setup, search examples, features, shortcuts, and help.

## Development

### Build and run

Requires Rust 1.89 or later. On Linux, install the Wayland and XKB development packages with `sudo apt install libxkbcommon-dev libwayland-dev`.

    cargo build --release -p arcade-find
    cargo test --workspace
    scripts/e2e-linux.sh target/release/arcade-find
    python3 scripts/bench_real.py --files 1000000

The end-to-end script needs Sway, grim, wtype, Xvfb, and xdotool.

### Command line

    arcade-find [--show [QUERY] | --toggle | --hide | --background | --settings |
                 --search QUERY [--limit N] [--hidden] [--json] | --status |
                 --rescan | --restart | --quit | --version |
                 --arcade-manifest | --arcade-invoke]

Use **arcade-find --toggle** for a desktop shortcut on Wayland desktops where Find cannot register a global shortcut. See [Hyprland integration](docs/HYPRLAND.md).

### Data and configuration

Find indexes file and folder names and metadata locally. It reads file contents only when you search inside files. On Linux its data is under **~/.local/share/arcade-find**; on macOS under **~/Library/Application Support/Arcade Find**; on Windows under **%LOCALAPPDATA%/Arcade/Arcade Find**. **ARCADE_FIND_HOME** overrides the data directory, and **ARCADE_FIND_PROFILE** selects a separate profile.

The index excludes common build/cache folders, network mounts, and system paths by default. Symbolic links are not followed.

### Technical references

- [Architecture](docs/ARCHITECTURE.md): index, query, matching, and overlay.
- [Arcade Link](docs/ARCADE_LINK.md): actions Find exposes and uses.
- [Hyprland](docs/HYPRLAND.md): runtime binding and compositor setup.
- [Status](docs/STATUS.md): verification and known limits.
- [Shelf integration](docs/SHELF.md): selection handoff details.
- [Third-party notices](THIRD_PARTY_NOTICES.md) and [vendored files](VENDORED.md).

The project is licensed under MIT OR Apache-2.0.
