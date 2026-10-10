# Arcade Find and Arcade Link

Canonical ID `arcade.find`, protocol 1, manifest schema 1, using
`arcade-link` v0.2.0 (`Cargo.lock` pins commit `bedee71`). Code:
`crates/arcade-find/src/link.rs` (actions, offers, encoding) and
`crates/arcade-find/src/service.rs` (presence, registry, calls).

## Manifest

Written by `Presence` off the first-frame path, on a worker thread:

```json
{ "schema": 1, "id": "arcade.find", "name": "Arcade Find", "version": "0.1.0",
  "link": { "protocol": [1] },
  "executable": "/home/u/Applications/Arcade/Arcade-Find.AppImage",
  "launch": { "background": ["--background"], "invoke": ["--arcade-invoke"] },
  "shortcuts": [{ "id": "show", "accelerator": "Ctrl+Alt+F" }],
  "settings": { "linkEnabled": true },
  "actions": [ "find.search", "find.show" ] }
```

`arcade-find --arcade-manifest` prints it with no side effects. With
"Connect with other Arcade apps" off, the manifest stays (installed) with no
actions and nothing listens.

## Actions Find exposes

| Action | v | Accepts | Produces | Effects | Interactive | One-shot |
|---|---|---|---|---|---|---|
| `find.search` "Find matching files" | 1 | `text/plain` (the query), or `options.query` | `file/*[]`, `folder/reference` | — | no | yes |
| `find.show` "Search in Find" | 1 | nothing, `text/plain`, `file/*`, `folder/reference` | — | `opens-ui` | yes | no |

**`find.search`** options: `query`, `limit` (1–1000, default 50), `hidden`
(default false). Same query language as the overlay, but no frecency, no
recent/pinned items, and no content search (`/…` and empty queries answer
`unsupported_input`). Result:

```json
{ "outputs": [ { "type": "file/any[]", "paths": ["/home/u/a.png", "/home/u/b.pdf"] },
               { "type": "folder/reference", "path": "/home/u/assets" } ],
  "message": "3 matches",
  "data": { "query": "logo", "matched": 3, "truncated": false, "indexing": false, "elapsedMs": 4.2,
            "results": [ { "path": "/home/u/a.png", "name": "a.png", "parent": "/home/u", "type": "file/image",
                           "isDir": false, "isSymlink": false, "size": 20480, "modified": 1760000000, "score": 812 } ] } }
```

`data.results` is ranked; `outputs` uses the selection encoding below.
`indexing: true` means the first crawl is still running.

**`find.show`**: no input opens the overlay with `options.query`; text sets
the query; a folder sets `in:"~/that/folder" ` plus `options.query`; a file
searches its name and selects it when it appears ("Reveal in Find").

## Peers Find uses

Peer entries come from the cached registry (refreshed by a directory watch,
never read when the action list opens) and are shown only if the peer is
installed (its executable exists), its Link is on, it isn't switched off in
Find's Connected apps, the action is available on this OS, and every value of
the selection matches its `accepts` (SPEC §5.2). An action with no array
pattern is offered only for a single value. `maxBytes` disables the entry
with the standard "Too large for …" text. Data-only actions (no effects, not
interactive, no file outputs, e.g. `look.inspect`) aren't listed.

| Peer | Where it shows | Calls |
|---|---|---|
| Look | **Enter**, Space, "Quick Look" | `look.preview` with the selection; falls back to the default app when Look can't take it |
| Box | Tab: featured tools for the selection's type, pipelines, "More in Arcade Box…" | `box:<tool>` (`#preset`), `box.pipeline.run` with `options.pipeline`, `box.open`; `box.pipelines` is read only while Box is running |
| Shelf | Tab: "Add to Shelf" for any selection, with a payload preview; Find stays open | `shelf.add` (see [SHELF.md](SHELF.md)) |
| Wheel | Tab: "Add to Wheel" (single file) | `wheel.add_action` |
| Clipboard | Tab: "Send to my devices ↗" with a payload preview | `clipboard.add` |
| Lens | Tab, for one image: its analyze / pin actions | `lens.analyze`, `lens.pin` |
| any other app | Tab, by the same rules | its action, unchanged |

Calls run on a worker (`arcade_link::invoke_action`: running instance,
one-shot, or launch in the background with a 3 s limit). If the action opens
UI, the overlay hides first and comes back with the error if it fails;
otherwise it stays and shows the peer's `message`. Errors use the standard
texts (`LinkError::user_message`).

### Selection encoding

Files travel by reference (never copied): one file as `file/<kind>` with
`path` and `size`; several files as one `file/<kind>[]` (`file/any[]` when
mixed); each folder as `folder/reference`, after the files. Non-UTF-8 paths
are left out and reported (JSON can't carry them exactly).

## Connected apps (Settings)

Master switch, one row per Arcade app (the family apps, Shelf included, plus
any other installed `arcade.*` app) with its state and a "Use with Arcade Find" toggle,
"Get" (Arcade Tools' `tools.install` when installed, otherwise the releases
page), and diagnostics (registry, runtime, endpoint, last error).

## Verification

- `cargo test -p arcade-find`: offer rules, encoding, `find.search`/`find.show`
  argument handling.
- `crates/arcade-find/tests/link_shelf.rs`: against an in-process peer with
  Shelf's contract (real `arcade_link::Server`): discovery, toggles,
  `linkEnabled`, missing executables, `maxBytes`, by-reference payloads,
  standard errors, launching a stopped peer with `launch.background`.
- `scripts/e2e-linux.sh` with `ARCADE_LINK_CLI` (the v0.2.0 `arcade-link`
  CLI): the real binary in headless Sway and Xvfb sends `look.preview` on
  Enter (mock Look); one-shot `find.search`. With `ARCADE_SHELF_BIN` it runs
  the **real Arcade Shelf**: a mixed selection lands in Shelf's store as
  references, a stopped Shelf is launched and takes the add, and Shelf's
  `find.show` request opens Find on that file, and two results dragged onto
  Shelf's window land as references on Wayland and X11 (53/53).
- Arcade Link's ecosystem runner (`tools/e2e.py --only find`) runs the real
  Find and Shelf: 3/3.
- Not yet run against the real Look, Box, Wheel, Clipboard or Lens builds.

## Family onboarding

Find is a registered family app since Arcade Link `v0.2.0` (2026-10-10):

| Repository | Find's part | Merged |
|---|---|---|
| Arcade Link | `ids::FIND`, name, pitch, releases URL (Rust and Qt), glyph `arcade.find.svg`, accent `#22C55E`, `fixtures/find.json`, `find.search`/`find.show` in the SPEC catalog, Find in `tools/e2e.py` and the benchmarks; tag `v0.2.0` | [#1](https://github.com/qa-p1/Arcade-Link/pull/1) |
| Arcade Tools | Install (AppImage, Inno, universal DMG), data folders, login entries, `tools.install` | [#1](https://github.com/qa-p1/Arcade-tools/pull/1) |
| Arcade Wheel | Connected apps row, glyph, accent; slots can hold Find's actions | [#5](https://github.com/qa-p1/Arcade-wheel/pull/5) |
| Arcade Box | Link `v0.2.0`: Find in Connected apps, glyph, accent (0.1.1) | [#1](https://github.com/qa-p1/Arcade-box/pull/1) |
| Arcade Lens | Link `v0.2.0`: Find in Connected apps; **Search in Find** for text and paths (0.1.1) | [#1](https://github.com/qa-p1/Arcade-lens/pull/1) |
| Arcade Look | Link `v0.2.0`: Find in Connected apps, glyph, accent (0.1.1) | [#1](https://github.com/qa-p1/Arcade-look/pull/1) |
| Arcade Clipboard | Link `v0.2.0`: Find in Connected apps, glyph, accent | [#1](https://github.com/qa-p1/Arcade-clipboard/pull/1) |

Find itself uses Link `v0.2.0`, so `link::apps` and the Shelf glyph come from
Link. Each consumer picks the peer actions it shows from its own list, so the
pin alone adds Find to Connected apps, not to their menus. Of the four, only
Lens offers one of Find's actions (**Search in Find**); Box, Look and
Clipboard list Find without an entry.
