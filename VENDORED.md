# Vendored files

Copied unmodified from other Arcade repositories, pinned to a commit. The offline
drift check is `scripts/check-vendored.py` (verifies the SHA-256 inventories below).

| Files here | Source | Pin | License |
|---|---|---|---|
| `assets/glyphs/*.svg`, `assets/app-icon/` | [Arcade Link](https://github.com/qa-p1/Arcade-Link) `design/glyphs/`, `design/app-icons/find/` | the tag `crates/arcade-find/Cargo.toml` pins (see "Arcade design kit" below) | MIT OR Apache-2.0 |
| `tools/arcade-release.py`, `tools/validate_shortcuts.py` | Arcade Link `tools/` | tag `v0.3.0` (commit `33650a8`; unchanged in `v0.4.5`) | MIT OR Apache-2.0 |
| `packaging/windows/arcade.iss`, `arcade-integration.ps1` | Arcade Link `packaging/windows/` | tag `v0.3.0` (commit `33650a8`; unchanged in `v0.4.5`) | MIT OR Apache-2.0 |

Adapted (not verbatim):

| Code here | Source | Pin | License |
|---|---|---|---|
| `crates/arcade-find/src/hotkey.rs` (`hyprland` module) | [Arcade Lens](https://github.com/qa-p1/Arcade-lens) `crates/lens-platform/src/hyprland.rs` | branch `arcade/link`, commit `26b3532` | MIT OR Apache-2.0 |

`scripts/sync-design` refreshes the design kit files from the pinned Link tag
and rewrites their section below. To refresh another copy, copy it verbatim
from the recorded upstream commit, update its hash and source pin here, then
run `scripts/check-vendored.py`. See Link `packaging/windows/README.md` for
include/task/silent-mode contracts. The PowerShell helper is bundled, never
downloaded at install time.

SHA-256 of the other vendored copies:

```text
8f360855419930730c486d0978acaecf7a8179cc063465c9f8324878dcf6c2fd  tools/arcade-release.py
3b5d2981ab87fdf07437daa294dce825d59b63580430e9ffa2b58e87d5d72cf0  tools/validate_shortcuts.py
fe152028ff674fdacdef4aed8970bb0a7afeb2dd67d2a3af075d7d95683bf96b  packaging/windows/arcade.iss
2b2a8b699dc1e6c81c02bb9ad9be1ddf2f8a5447541eb1dfe886056aa5dc4fd6  packaging/windows/arcade-integration.ps1
```

## Arcade design kit

Copied unchanged from [Arcade Link](https://github.com/qa-p1/Arcade-Link/tree/v0.4.8) tag `v0.4.8` (commit
`390f5ede7d5f448e544cf0768d8a0a4c1946345e`) by `scripts/sync-design`; never edit these files here.
License: MIT OR Apache-2.0.

```text
11405845a97dcec4150fb3943d03dbd11eeb086ded31a5cab27aa92b620838c1  assets/glyphs/arcade.box.svg
bbf1518dc224aa8f8df59f432cf5eb8b6e25b1f4a4e4676fe231d84770dac156  assets/glyphs/arcade.clipboard.svg
54c0b3e869912313a7b543175ad6bda5853b0bc8118d753e2c67de6dfdd07df9  assets/glyphs/arcade.find.svg
ba869f44636f28ea28ebd71c8e00b8206fd1eaa28ce23cfc628b8feac1bb556b  assets/glyphs/arcade.lens.svg
ce15684dfd003b0beb7e21a6e8c5a080433a0e25b804e48f024f3ba17794279b  assets/glyphs/arcade.look.svg
42ff3fbca18365d18875061617446aca6876bf5f740160412663d3460f81e0cc  assets/glyphs/arcade.shelf.svg
dd2aa2c70bb8ebe8b597eb8440dfc032a548ff43f90f2ffd08ec00cd02b4fac2  assets/glyphs/arcade.tools.svg
6718ccee834c345aac4b4a71ea83461412e5f4ae6f24179ab897192cf243ce63  assets/glyphs/arcade.wheel.svg
65f265bb6fffa0e770976d69f7e7324bc27009c70a08c5dc31c7b04deed5d83a  assets/app-icon/icon.svg
31f13bd053d5fbf7959edbd93d2493d7c6ac4a2647ed3122ee6677087cac705a  assets/app-icon/arcade-find.ico
ed6ed9b223aecea9b5b58910422d85acf3aec92de7167ab1ed7f49cecdfc276a  assets/app-icon/arcade-find.icns
a533048fab4cb0264f9b151527fba9e8d4094508c07eb339f2a41f106b26edac  assets/app-icon/png/1024.png
3e68d2af4cd2ef56f101c7ecc8e1e41c12e0179f23595d042f291cc722abda1e  assets/app-icon/png/128.png
b392f3ad032cb6f17d22aecc40f7c4c9ea55965d3613b01f096ecefb99baaeba  assets/app-icon/png/16.png
b854d4a9e62afc78e488ad67d8f6196bbab1e4c11019775ee2597e228b0f3a64  assets/app-icon/png/24.png
3d73fde11a11ed8337da442264b51dc20ba5105076226665ad952343a46f1870  assets/app-icon/png/256.png
7dfa8cc7752330b5f33468f821f939c3240a029199b75166983f8798dfe575be  assets/app-icon/png/32.png
b5fdbe1be7abfaa4ce6011cf995ca494726bc1e3d0f528ef491fa71e1f677f52  assets/app-icon/png/48.png
4a749e4ed78522093fb9b69d8f61e09d53b8e833946c728eb8b93d862235221a  assets/app-icon/png/512.png
decbb63db71a2a2a5a516c7089fb48bacea1e094d2805bd8d723584d4f4b6e70  assets/app-icon/png/64.png
```
