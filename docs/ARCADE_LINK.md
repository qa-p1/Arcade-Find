# Arcade Find and Arcade Link

Canonical ID `arcade.find`, protocol 1, manifest schema 1, using
`arcade-link` v0.1.0 (`Cargo.lock` pins commit `337b85f`). Code:
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
| `find.search` "Search files" | 1 | `text/plain` (the query), or `options.query` | `file/*[]`, `folder/reference` | — | no | yes |
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

Master switch, one row per Arcade app (the five family apps plus any other
installed `arcade.*` app) with its state and a "Use with Arcade Find" toggle,
"Get" (Arcade Tools' `tools.install` when installed, otherwise the releases
page), and diagnostics (registry, runtime, endpoint, last error).

## Verification

- `cargo test -p arcade-find`: offer rules, encoding, `find.search`/`find.show`
  argument handling.
- `crates/arcade-find/tests/link_shelf.rs`: against an in-process mock peer
  (real `arcade_link::Server`): discovery, toggles, `linkEnabled`, missing
  executables, `maxBytes`, by-reference payloads, standard errors, launching a
  stopped peer with `launch.background`.
- `scripts/e2e-linux.sh` with `ARCADE_LINK_CLI` (the v0.1.0 `arcade-link`
  CLI): the real binary in headless Sway and Xvfb sends `look.preview` on
  Enter and a mixed selection to a mock `shelf.add` peer; one-shot
  `find.search`.
- Not yet run against the real Look, Box, Wheel, Clipboard or Lens builds,
  or in Arcade Link's ecosystem runner (`tools/e2e.py`), which doesn't know
  `arcade.find` yet.

## Family onboarding (pending)

Link's shared lists don't include `arcade.find` yet: `ids`, display name,
pitch and releases URL, a glyph in `assets/glyphs/`, an accent in
`assets/tokens.json` (Find uses `#22C55E`), fixtures, the action and shortcut
catalog, e2e and benchmark app lists; Arcade Tools' app list and install
mappings; and peers that list apps explicitly. Until then Find works with
peers through its manifest alone, but peers' Connected apps pages won't list
it. None of these changes exist yet (deferred by the owner: "we will wire
this app into other apps later"). When they're made, they go on reviewable
branches; tagging a Link release, bumping consumers and releasing need the
owner's approval.
