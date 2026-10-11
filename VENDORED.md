# Vendored files

Copied unmodified from other Arcade repositories, pinned to a commit. The offline
drift check is `scripts/check-vendored.sh` (verifies the SHA-256 inventory below).

| Files here | Source | Pin | License |
|---|---|---|---|
| `assets/glyphs/*.svg` | [Arcade Link](https://github.com/qa-p1/Arcade-Link) `assets/glyphs/` | tag `v0.2.0` (commit `bedee71`) | MIT OR Apache-2.0 |
| `assets/link-tokens.json` | Arcade Link `assets/tokens.json` | tag `v0.2.0` (commit `bedee71`) | MIT OR Apache-2.0 |
| `tools/arcade-release.py`, `tools/validate_shortcuts.py` | Arcade Link `tools/` | branch `arcade/tools-hub`, commit `01d50d8611830cfcb63660635ff4958e3b05fc6d` | MIT OR Apache-2.0 |
| `packaging/windows/arcade.iss`, `arcade-integration.ps1` | Arcade Link `packaging/windows/` | same commit `01d50d8611830cfcb63660635ff4958e3b05fc6d` | MIT OR Apache-2.0 |

Adapted (not verbatim):

| Code here | Source | Pin | License |
|---|---|---|---|
| `crates/arcade-find/src/hotkey.rs` (`hyprland` module) | [Arcade Lens](https://github.com/qa-p1/Arcade-lens) `crates/lens-platform/src/hyprland.rs` | branch `arcade/link`, commit `26b3532` | MIT OR Apache-2.0 |

SHA-256 of the vendored copies:

```text
8f360855419930730c486d0978acaecf7a8179cc063465c9f8324878dcf6c2fd  tools/arcade-release.py
3b5d2981ab87fdf07437daa294dce825d59b63580430e9ffa2b58e87d5d72cf0  tools/validate_shortcuts.py
8a3419c09b0f0a4ca16cac91420512d4ed450ed968e65011421dfc5a46ee2ae8  packaging/windows/arcade.iss
42941e61b3da59bdf58c7df30098bb7bf75b827d6f81e4d5fb62e0f75fdfae5b  packaging/windows/arcade-integration.ps1
11405845a97dcec4150fb3943d03dbd11eeb086ded31a5cab27aa92b620838c1  assets/glyphs/arcade.box.svg
bbf1518dc224aa8f8df59f432cf5eb8b6e25b1f4a4e4676fe231d84770dac156  assets/glyphs/arcade.clipboard.svg
ba869f44636f28ea28ebd71c8e00b8206fd1eaa28ce23cfc628b8feac1bb556b  assets/glyphs/arcade.lens.svg
ce15684dfd003b0beb7e21a6e8c5a080433a0e25b804e48f024f3ba17794279b  assets/glyphs/arcade.look.svg
dd2aa2c70bb8ebe8b597eb8440dfc032a548ff43f90f2ffd08ec00cd02b4fac2  assets/glyphs/arcade.tools.svg
6718ccee834c345aac4b4a71ea83461412e5f4ae6f24179ab897192cf243ce63  assets/glyphs/arcade.wheel.svg
3adb83dc119bc39b3658347da718ee24a5c866acb6f36a0325be7b0f00c28ab5  assets/link-tokens.json
```

The hub scripts/includes are pinned to the reviewed branch commit until the
orchestrator replaces the dependency branch with tag `v0.3.0`. To refresh a
copy, copy it verbatim from the recorded upstream commit, update its hash
and source pin here, then run `scripts/check-vendored.sh`. See Link
`packaging/windows/README.md` for include/task/silent-mode contracts. The
PowerShell helper is bundled, never downloaded at install time.

When Arcade Link publishes glyphs for `arcade.find` and `arcade.shelf`, add
them here and map them in `crates/arcade-find/src/ui/icons.rs` (peers without
a glyph use a neutral app glyph until then).
