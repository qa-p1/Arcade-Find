# Vendored files

Copied unmodified from other Arcade repositories, pinned to a commit. A
drift check is `scripts/check-vendored.sh` (compares against the pinned tag).

| Files here | Source | Pin | License |
|---|---|---|---|
| `assets/glyphs/*.svg` | [Arcade Link](https://github.com/qa-p1/Arcade-Link) `assets/glyphs/` | tag `v0.1.0` (commit `337b85f`) | MIT OR Apache-2.0 |
| `assets/link-tokens.json` | Arcade Link `assets/tokens.json` | tag `v0.1.0` (commit `337b85f`) | MIT OR Apache-2.0 |
| `tools/arcade-release.py` | Arcade Link `tools/arcade-release.py` | tag `v0.1.0` (commit `337b85f`) | MIT OR Apache-2.0 |

Adapted (not verbatim):

| Code here | Source | Pin | License |
|---|---|---|---|
| `crates/arcade-find/src/hotkey.rs` (`hyprland` module) | [Arcade Lens](https://github.com/qa-p1/Arcade-lens) `crates/lens-platform/src/hyprland.rs` | branch `arcade/link`, commit `26b3532` | MIT OR Apache-2.0 |

SHA-256 of the vendored copies:

```text
11405845a97dcec4150fb3943d03dbd11eeb086ded31a5cab27aa92b620838c1  assets/glyphs/arcade.box.svg
bbf1518dc224aa8f8df59f432cf5eb8b6e25b1f4a4e4676fe231d84770dac156  assets/glyphs/arcade.clipboard.svg
ba869f44636f28ea28ebd71c8e00b8206fd1eaa28ce23cfc628b8feac1bb556b  assets/glyphs/arcade.lens.svg
ce15684dfd003b0beb7e21a6e8c5a080433a0e25b804e48f024f3ba17794279b  assets/glyphs/arcade.look.svg
dd2aa2c70bb8ebe8b597eb8440dfc032a548ff43f90f2ffd08ec00cd02b4fac2  assets/glyphs/arcade.tools.svg
6718ccee834c345aac4b4a71ea83461412e5f4ae6f24179ab897192cf243ce63  assets/glyphs/arcade.wheel.svg
68050c1d76b5da6542c65879b092c8254bff9429396cd3b0742401026daa44d0  assets/link-tokens.json
52042f7a9ecac497e69b5e6a73b4fa9541c27cf725f80016466202ab076f6ae0  tools/arcade-release.py
```

When Arcade Link publishes glyphs for `arcade.find` and `arcade.shelf`, add
them here and map them in `crates/arcade-find/src/ui/icons.rs` (peers without
a glyph use a neutral app glyph until then).
