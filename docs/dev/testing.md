# Testing

Use `scripts/check`, the same entry point as `.github/workflows/ci.yml`.
`--core-only` runs documentation and vendor checks, Python regressions,
Rustfmt, Clippy with zero warnings, isolated Rust tests and the renderer's
actual contrast pairs. Windows and macOS run that source suite plus their
native installer checks in CI. The Rust toolchain is pinned to 1.88.0.

## Native Linux checks

All GUI work runs through the pinned Arcade Link `tools/e2e.py run --` runner.
It creates a private HOME, XDG directories, Link registry, D-Bus and Xvfb.
The Wayland checks start their own headless Sway within that session. Never
run the GUI scripts against your working desktop.

Install the packages in CI's `Session tools` step, including Sway, grim,
wtype, Xvfb, xdotool, xclip, ImageMagick, D-Bus Python bindings, ripgrep, CJK
fonts and Shelf's Qt runtime dependencies. Build Find and `e2e-pointer`:

```sh
cargo build --locked -p arcade-find --bin arcade-find --example e2e-pointer
```

Set `ARCADE_LINK_DIR` to a checkout at the app's current Link tag and
`ARCADE_LINK_CLI` to that checkout's built `arcade-link` binary. Set
`ARCADE_SHELF_BIN` to the real released Shelf binary. CI obtains Shelf with
`scripts/fetch-peer.py`; it verifies the release manifest, checksums, asset
size and GitHub digest before extracting the AppImage.

```sh
scripts/check --native-only --binary target/debug/arcade-find
```

The native suite exercises the real clipboard, file actions, Settings
singleton and lifecycle, live themes, stored geometry, welcome Skip,
install dialogs, tray registration and restart, all three overlay backends,
Link handoffs and real Shelf drops. The fallback lifecycle and clipboard
regressions also run against the pre-fix binary in CI and must fail for the
specific old defect. A different error is a failure of the regression check.

The separate `Settings screenshots (real eframe)` CI job captures every page
at its top and bottom in both themes, at 720×520 and 760×560, with 100% and
125% text. Download the evidence and inspect every capture. Automated bounds
checks do not replace that review. Native evidence is under `target/e2e`.

## Desktop-only checks

The real Hyprland shortcut, cursor-monitor placement, warm-open budget,
focus timing and physical IME composition need the owner's desktop.
Windows/macOS login items, macOS permission re-grant and Windows identity
need their real sessions. A real Tools self-update also remains a desktop
check; release selection and installer verification run separately.
