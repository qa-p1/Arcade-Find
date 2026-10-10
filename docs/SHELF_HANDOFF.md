# ARCADE FIND × ARCADE SHELF — FINAL INTEGRATION HANDOFF

Written 2026-10-09 by the Arcade Find implementer; updated the same day after Find's side was implemented and tested. Each statement has one of these labels:

- **[CODE]**: the code exists in the Find working tree and was compiled/tested where stated.
- **[DESIGNED]**: decided, not yet implemented (only a few items remain; marked where they occur).
- **[PROPOSED]**: something Shelf owns. It is a recommendation awaiting Shelf's implementation.
- **[LINK]**: verified by reading Arcade Link `v0.1.0` (tag commit `337b85f`; `main` = `92aa03c` changes only docs and tools, not code).

---

## 0. Corrections to the Shelf brief (read first)

1. **Find never produces URLs or text.** Find indexes filesystem entries only, so a Find result is always a file or a folder (including symlinks, which are not followed). "Add URLs from Find" cannot happen. Shelf still needs `text/url` and `text/plain` for Lens, Clipboard, browsers and DnD, just not for Find.
2. **Find's side is implemented** on `qa-p1/Arcade-Find` branch `feature/find-v0.1` (see `docs/STATUS.md` for the tested commit). Shelf doesn't exist yet, so every Find↔Shelf check ran against a mock peer publishing the contract in §C.2.
3. **Link has no ordered mixed array type.** `inputs` is an array of `Content`, but one `Content` can hold several paths only as `file/<kind>[]`. Folders are always single `folder/reference` values. A mixed selection therefore arrives as several inputs (§C.3), and the order between files and folders is not preserved.
4. **`launch.invoke` is per manifest, not per action [LINK].** In `client::invoke_action`, a *non-interactive* action of a stopped app runs **one-shot** whenever the manifest has `launch.invoke`. If Shelf advertised `--arcade-invoke`, a `shelf.add` from Find would run in a throwaway process that never updates the resident UI. **Shelf must not set `launch.invoke`** unless it also handles a one-shot `shelf.add` correctly.
5. **Link's spec requires confirmation for persistence.** SPEC §1.8: persistence is confirmed "in the owner's UI, or the caller marks them with a payload preview". Find does the second: the entry shows a payload preview (§D). Shelf adds an Undo (§E.1). Neither side shows a modal prompt.

---

## A. Actual Arcade Find architecture

**Language and stack.**
- The whole app is Rust (edition 2021, MSRV 1.89, license MIT OR Apache-2.0).
- The overlay is software-rendered with tiny-skia (via `resvg`) and cosmic-text.
- On Wayland the overlay uses a **wlr-layer-shell** surface (smithay-client-toolkit 0.19). On X11, Windows and macOS it uses **winit 0.30.13 + softbuffer**.
- Settings is a **separate egui/eframe 0.36 process** (`--settings-window`) that runs only while open.
- Why: the 1M-file index alone is about 37 MiB, so a GL or Qt overlay would break the memory goal of staying well under Look's about 75 MiB.

**Workspace.**

| Path | Role | State |
|---|---|---|
| `crates/find-core/src/index.rs` | Struct-of-arrays index (parent, name offset/length, flags, size, mtime) plus a name arena; tombstones and compaction | [CODE] tested |
| `crates/find-core/src/scan.rs`, `watch.rs`, `engine.rs` | Background crawl with progress; inotify (Linux), `notify` (FSEvents/RDCW); reconciling rescans; an engine thread that is event-driven with no idle polling | [CODE] tested |
| `crates/find-core/src/query.rs`, `matcher.rs`, `search.rs` | Query language (`ext:` `dir:` `in:` `size:` `modified:` `hidden:` and `/content`), fuzzy and substring scoring, parallel top-K search | [CODE] tested |
| `crates/find-core/src/frecency.rs` | Local frecency and pins (history: **never exported over Link**) | [CODE] tested |
| `crates/find-core/src/persist.rs`, `settings.rs`, `paths.rs` | Index file with CRC (atomic), versioned settings with recovery, per-profile paths (`ARCADE_FIND_HOME`, `ARCADE_FIND_PROFILE`) | [CODE] tested |
| `crates/find-core/src/kind.rs` | File kind, using the same extension table as Link's `content.rs`; `Kind::link_type()` gives `file/<kind>` or `folder/reference` | [CODE] |
| `crates/arcade-find/src/instance.rs` | Single instance per profile (lock file, local socket, 32-byte token) | [CODE] tested |
| `crates/arcade-find/src/ui/model.rs` | Window-system-independent overlay state: keys, multi-selection (Shift+arrows), action list, effects | [CODE] 10 tests |
| `crates/arcade-find/src/ui/render.rs`, `text.rs`, `icons.rs`, `theme.rs` | Software renderer, Link glyphs (vendored from v0.1.0), light/dark/system themes | [CODE] tested |
| `crates/arcade-find/src/os.rs` | Open, reveal, no-overwrite rename, trash (never a hard delete), clipboard thread | [CODE] tested |
| `crates/arcade-find/src/link.rs` | Manifest, `find.search` and `find.show` handler, one-shot mode, generic peer offers, selection-to-`Content` encoding | [CODE] tested |
| `crates/arcade-find/src/service.rs` | Controller: search worker, results, effects, peer invocation off the UI thread | [CODE] tested end to end |
| `crates/arcade-find/src/main.rs` | Standard CLI (`--version --background --settings --quit --arcade-manifest --arcade-invoke`, plus `--show [q] --toggle --hide --search --status --rescan --restart`) | [CODE] tested end to end |
| `crates/arcade-find/src/ui/wayland.rs`, `ui/desktop.rs` | Overlay backends: wlr-layer-shell; winit + softbuffer | [CODE] tested in headless Sway and Xvfb |
| `hotkey.rs`, `tray.rs`, `autostart.rs`, `settings_ui.rs` | Shortcut (native or Hyprland runtime bind), tray, start at login, Settings window (egui) | [CODE] unit-tested; Settings rendered under Xvfb |
| `packaging/`, `.github/workflows/ci.yml` | AppImage (built locally), Inno per-user installer and universal DMG (CI only) | [CODE] |

**Process model.**
- There is one resident instance per profile.
- A second launch forwards its command over the instance socket and exits.
- The Link endpoint is published with `arcade_link::Presence` off the first-frame path.
- `--arcade-invoke` serves `find.search` from the saved index without any UI, tray, shortcut, listener or manifest write.

**Measured** (real tree of 1M empty files plus 4,010 folders, release build, 4 vCPU container, headless; `scripts/bench_real.py`):
- Whole-app RSS 52 MiB; idle CPU 0 ticks over 30 s.
- Crawl 0.79 s (warm disk cache); warm start 33–37 ms; index file 38.5 MiB.
- Keystrokes while typing: the first letter 15–23 ms (half the index matches), later letters 0–14 ms (narrowing).

**Relationship with Look** [CODE]:
- **Enter** previews in Look (`look.preview`). It falls back to the default app when Look is missing, disabled, or doesn't accept the item.
- **Space**, after arrow navigation, also previews.
- **Shift+Enter** opens with the default app; **Ctrl+Enter** reveals in the folder.
- A Look preview counts toward frecency.
- Find hides its overlay before invoking Look. If the preview fails, Find comes back with the standard Link error.
- Tested end to end against `arcade-link mock` standing in for Look (Enter → `look.preview` with `folder/reference`); not yet against the real Look build.

**Arcade Link client/server** [CODE]:
- Dependency: `arcade-link = { git = "https://github.com/qa-p1/Arcade-Link", tag = "v0.1.0", features = ["watch"] }`.
- Discovery: `SharedRegistry::load` plus `watch` (OS notifications, no polling).
- Offers are computed from the in-memory snapshot. Opening the action list does no disk or IPC work.
- Calls: `arcade_link::invoke_action` on a worker thread.
- Serving: `Presence::start` plus a `Handler`.

---

## B. Verified implementation status

| Area | Status |
|---|---|
| find-core (index, crawl, watchers, persist, query, ranking, narrowing, frecency, settings) | Implemented; 48 unit tests pass; 1M-file benchmark above |
| App: CLI, single instance, service, overlay (model, renderer, Wayland layer shell, X11), shortcut, tray, start at login, Settings | Implemented; 34 unit tests pass; `scripts/e2e-linux.sh` 43/43 checks in headless Sway and Xvfb |
| Link layer (`find.search`, `find.show`, one-shot, peer offers, selection encoding) | Implemented; unit tests plus `tests/link_shelf.rs` (5 tests, in-process mock peer) plus e2e against `arcade-link mock` peers |
| Windows, macOS | Type-check (`cargo check`) passes for both; CI builds and tests them; **not run interactively** |
| Packaging | AppImage built and smoke-tested locally; Inno installer and universal DMG built only in CI; `arcade-release.json` generated with the vendored Link tool |
| Family onboarding (Link IDs/glyph/tokens, Tools mappings, consumer lists) | **Not started** (deferred by the owner); tags and releases need the owner's approval |
| Real desktops (Hyprland, KDE, GNOME, Windows, macOS) | **Not validated interactively** |
| Repository | `qa-p1/Arcade-Find`, branch `feature/find-v0.1`; the tested commit is recorded in `docs/STATUS.md` |

---

## C. Final Find ↔ Shelf contract

### C.1 What Find exposes (Shelf may call these) [CODE]

**`find.show`**, version 1:
- Title "Search in Find", verb `search`.
- Accepts **nothing**, `text/plain`, `file/*` or `folder/reference`. Zero inputs is allowed even though `accepts` is non-empty.
- Effects `["opens-ui"]`, interactive `true`, privacy `local`. Not one-shot.
- Options: `{ "query"?: string }`.
- Behavior:

  | Input | Result |
  |---|---|
  | none | Opens the overlay with `options.query`, or empty |
  | `text/plain` | The first line becomes the query (at most 1024 chars) |
  | `folder/reference` | The query becomes `in:"~/that/folder" ` plus `options.query` |
  | file | The query becomes the file name, and that file is selected when it appears ("Reveal in Find") |

- Result: `{ "message": "Showing Arcade Find" }`.
- Errors: `unsupported_input` for other types; `denied/disabled` when Find's Link is off.

```json
{"v":1,"id":7,"method":"invoke","params":{"action":"find.show","version":1,
 "inputs":[{"type":"folder/reference","path":"/home/u/Projects/site"}],
 "options":{"query":"ext:png"},
 "context":{"source":"arcade.shelf","interactive":true,"reason":"user-click"}}}
```

**`find.search`**, version 1:
- Title "Find matching files", verb `search`.
- Accepts `text/plain`, which is the query; `options.query` is used if there is no input.
- Produces `file/*[]` and `folder/reference`.
- Effects `[]`, interactive `false`, privacy `local`. **One-shot supported** (`launch.invoke = ["--arcade-invoke"]`).
- Options: `{ "query": string, "limit"?: 1..1000 (default 50), "hidden"?: bool (default false) }`.
- **Not** included:
  - Frecency boosts.
  - Recent or pinned items. An empty query gives `unsupported_input`.
  - Content (`/pattern`) search. It is refused with `unsupported_input`, because file contents are read only when the user asks in Find's own window.
- Result:

```json
{"outputs":[{"type":"file/any[]","paths":["/home/u/a.png","/home/u/b.pdf"]},
            {"type":"folder/reference","path":"/home/u/assets"}],
 "message":"3 matches",
 "data":{"query":"logo","matched":3,"truncated":false,"indexing":false,"elapsedMs":4.2,
  "results":[{"path":"/home/u/a.png","name":"a.png","parent":"/home/u","type":"file/image",
              "isDir":false,"isSymlink":false,"size":20480,"modified":1760000000,"score":812}]}}
```

- `data.results` is the ranked order. `outputs` uses the encoding in §C.3.
- `indexing: true` means the first crawl is still running and results may be incomplete.
- Shelf does **not** need `find.search` for any flow in the brief. Use `find.show` for "Shelf → Find → Shelf". Never duplicate Find's index.

### C.2 What Shelf exposes (Find consumes) [PROPOSED; Find's consumer side is CODE]

**`shelf.add`**, version 1. This is the only action Find needs.

```json
{"id":"shelf.add","version":1,"title":"Add to Shelf","verb":"add",
 "accepts":["file/*","file/*[]","folder/reference","text/plain","text/url","text/rich"],
 "produces":[],"effects":["persists"],"interactive":false,"privacy":"local"}
```

- **`interactive: false`, effects `["persists"]` only.**
  - Find keeps its overlay open after a non-interactive action that has no `opens-ui` effect, so the user can keep collecting results.
  - If Shelf declares `opens-ui` or `interactive: true`, Find will hide its overlay before calling (the same rule Find applies to Look and Box).
  - `shelf.add` should only pulse or update the capsule, never steal focus.
- **No `launch.invoke` in Shelf's manifest** (see §0.4). A stopped Shelf is started with `launch.background` (`["--background"]`). The Link client waits up to 3 s for the endpoint, then reports `launch_failed`.
- Options, all optional. Find sends `{}`:
  - `shelf` (string): the target shelf ID. Default is the active shelf.
  - `reveal` (bool, default `true`): show the capsule or count change without taking focus.
- Request from Find for a six-item mixed selection:

```json
{"v":1,"id":3,"method":"invoke","params":{"action":"shelf.add","version":1,
 "inputs":[
   {"type":"file/any[]","paths":["/home/u/p/logo.svg","/home/u/docs/brief.pdf","/home/u/p/hero.png","/home/u/p/notes.md"]},
   {"type":"folder/reference","path":"/home/u/p/fonts"},
   {"type":"folder/reference","path":"/home/u/Downloads/refs"}],
 "options":{},
 "context":{"source":"arcade.find","interactive":true,"reason":"user-click"}}}
```

- Success: reply directly with `Reply::Done`, no job, ideally within 100 ms:

```json
{"message":"Added 5 items to Quick Shelf · 1 already there",
 "data":{"shelf":{"id":"quick","name":"Quick Shelf"},
         "added":[{"item":"it_8f2c","path":"/home/u/p/logo.svg"}],
         "skipped":[{"path":"/home/u/p/notes.md","reason":"duplicate"}]}}
```

  - Find shows `message` verbatim as a toast, so make it a complete user-facing sentence.
  - `data` is for other callers and tests. Readers ignore unknown fields.
- Errors use only Link codes:

  | Code | When |
  |---|---|
  | `unsupported_input` | Nothing usable was found, e.g. every path is missing |
  | `too_large` + `limit` | Staged text or bytes exceed the quota |
  | `denied` / `disabled` | Shelf's Link is off |
  | `busy` | Shelf is busy |
  | `internal` | Anything else |

  - Partial success is a success with `skipped`, never an error.
- Cancellation:
  - Because it answers synchronously, there is nothing to cancel.
  - If Shelf ever uses a job (for example copying a large handoff), it must honor `job.cancel` and remove partial staging.
  - Find doesn't cancel peer calls yet [DESIGNED]: it runs one peer call at a time on a worker and waits for the answer (Link's 30 s call timeout applies).

**`shelf.show`**, version 1 [PROPOSED]:
- Accepts nothing; options `{ "shelf"?: id }`; effects `["opens-ui"]`; interactive.
- Used by Wheel slots and Tools.

**`shelf.pick`**, version 1 [PROPOSED]:
- Accepts nothing; produces `file/*[]`, `folder/reference`, `text/plain`, `text/url`; effects `["opens-ui"]`; interactive.
- Options `{ "multiple"?: bool, "types"?: [accept pattern] }`.
- Returns only what the user picked. If the user cancels, return `denied` with reason `user_cancelled`.

**No `shelf.open`.** It would duplicate `shelf.show` with `options.shelf`.

### C.3 Selection → Link content (how Find encodes results) [CODE]

Find builds `Content` values from its in-memory rows with **no disk access**. Kind and size come from the index.

| Selection | `inputs` |
|---|---|
| 1 file | `[{"type":"file/<kind>","path":P,"size":N}]` |
| n ≥ 2 files | `[{"type":"file/<kind>[]","paths":[…]}]`; the kind is shared, or `any` if mixed (same rule as `Content::files`) |
| 1 folder | `[{"type":"folder/reference","path":P}]` |
| Files and folders | The file array first (selection order kept among files), then one `folder/reference` per folder in selection order |
| Missing rows (stale recent items) | Excluded, and the toast says "1 item no longer exists" |
| Non-UTF-8 path (Linux) | Excluded and reported. Link paths are JSON strings, and lossy conversion would point to a different file |

- `<kind>` comes from Link's extension table: image, video, audio, pdf, document, spreadsheet, presentation, archive, text, code, font, model, any.
- Symlinks are sent as the link path itself, unresolved.

### C.4 Capability discovery and offer rules (Find side) [CODE]

An entry is shown for the current selection only if all of these hold:

1. A manifest exists and its `executable` exists. This is Link's `Registry` rule.
2. `settings.linkEnabled` is true, and the peer isn't switched off in Find's Connected apps.
3. The action is `available`, `on_this_platform`, and its `accepts` is non-empty.
4. **Every** input value matches one of `accepts` (Link's `content_matches`, including array and hint rules).
5. If the action declares **no array pattern**, it is shown only when the selection is exactly one value. Peers built for one input are never handed several.
6. `maxBytes`, if set, is checked against the summed file sizes. If exceeded, the entry is shown disabled with "Too large for Arcade Shelf (limit N MB)."
7. Pure data actions (non-interactive, no effects, producing no file or folder types, e.g. `look.inspect`) are not listed.

Other rules:
- `look.preview` is not listed again, because it is Find's own "Quick Look" entry.
- Box shows its `featuredFor` tools, its pipelines and "More in Arcade Box…".
- **Find has no Shelf-specific code path**:
  - Any peer action that satisfies these rules appears, `shelf.add` included.
  - Find relays the manifest's `version` unchanged.
  - Find sends no Shelf options.
- Rename or re-version `shelf.add` freely; Find follows the manifest.

### C.5 Ownership

- **From Find**: only existing user paths. Find never creates handoff files for Shelf, and **Shelf must never copy, move or delete these paths**. It stores references only.
- **From other peers** (a Lens capture, Box output, large Clipboard text): content in another app's handoff directory belongs to its creator and is deleted after the job [LINK §5.3]. Shelf copies it into its own private staging **before** replying success.
- **From Shelf**: content Shelf hands to others, e.g. a staged image going to Box, is passed by its staging path. Shelf may only delete that staging file once no running job uses it.

---

## D. Find-side implementation details [CODE]

| File | What it does for Shelf (and every other peer) |
|---|---|
| `crates/arcade-find/src/link.rs` | `selection()` (encoding, §C.3), `offer()` and `peer_offers()` (§C.4), `request()` and `invoke()` (Link calls), `FindHandler` (`find.search`, `find.show`), `serve_oneshot()` |
| `crates/arcade-find/src/service.rs` | `actions_for()` builds the Tab list from the cached registry; `run_peer()` calls the peer on a worker, hides first only for interactive or `opens-ui` actions, shows the result `message` or the standard error |
| `crates/arcade-find/src/ui/model.rs` | Multi-selection; the action filter also matches the app name ("shelf" finds "Add to Shelf"); disabled entries show their reason |
| `crates/arcade-find/src/ui/icons.rs` | A generic app glyph for peers without a vendored Link glyph |
| `crates/arcade-find/src/settings_ui.rs` | Connected apps lists the five family apps plus any other installed `arcade.*` app, so Shelf gets a "Use with Arcade Find" toggle |
| `crates/arcade-find/tests/link_shelf.rs` | Mock-peer tests (§F) |
| `scripts/e2e-linux.sh` | Real binary + `arcade-link mock` peers (§F) |

**How the user adds results to Shelf:** select one or more results
(Shift+arrows), press Tab, choose "Add to Shelf" (or type `shelf`), Enter.
There's no dedicated key, to avoid coupling.

**Entry:** the manifest action's `title`; the manifest `name` for errors and
filtering; a generic glyph until Link ships one for `arcade.shelf`; because
the action has `persists`, a payload preview line ("logo.svg, brief.pdf +4"),
no ↗ (that's for outbound effects).

**Call:** on a worker thread (search keeps working); one peer call at a time;
if Shelf has to be started, Find shows "Starting Arcade Shelf…"; on success
the peer's `message` as a toast and the selection stays; on failure the
standard SPEC §6 text. Items left out (stale or non-UTF-8 paths) are noted
in the toast.

**Missing or disabled Shelf:** no entry, no promotion.

---

## E. Required Shelf-side implementation (checklist for ChatGPT)

### E.1 `shelf.add`

1. **Manifest:** use exactly §C.2. Do **not** set `launch.invoke`. Set `launch.background = ["--background"]`, and publish shortcuts in `shortcuts`.
2. **Validate each input; never trust it.** A Link peer is only "the same OS user".
   - The type must match `accepts`.
   - Paths must be absolute and must `lstat` successfully. Store symlinks as given.
   - Reject at most 1000 values per request, NUL bytes and empty strings.
   - `text/*` up to 256 KiB arrives inline. Above that it arrives as a handoff `path` that belongs to the caller: copy it to staging and apply your quota.
3. **Store** references in SQLite in one transaction, with a stable item ID, timestamps, type, display name and size. Never open or execute file content during an add.
4. **Duplicates:** if the same path is already on the target shelf, report it in `skipped` with reason `duplicate`, without an error.
5. **Feedback:**
   - Pulse the capsule and update its count without focus. Offer **Undo** for the last add in Shelf's UI; this is the owner-side confirmation.
   - Return `message` as a complete sentence.
   - Answer synchronously.
6. **Respect Link off:** no listener, and actions removed from the manifest (`Presence` / Qt `Server` handle this).
7. **Tests:**
   - Single file, a file array, mixed files and folders, missing paths (partial success), only missing paths (`unsupported_input`).
   - Text above 256 KiB through a handoff (copied, still valid after the caller deletes it).
   - Duplicates; Link disabled; a launch from stopped (`--background`, endpoint within 3 s).

### E.2 `shelf.show`, `shelf.pick`

- These run in Shelf's UI (`interactive: true`).
- `shelf.pick` returns only the chosen items, never whole shelves, and answers `denied`/`user_cancelled` on Esc.
- Bounded output: files by path; text inline up to 256 KiB, otherwise a Shelf-owned handoff that Shelf cleans up after 24 h.

### E.3 Using Find from Shelf (optional)

- Offer "Find related" on a folder item (`find.show` + `folder/reference`) or on any item (`find.show` + `options.query`).
- Discover it via the registry like any action. Show it only when `arcade.find` advertises `find.show`.

### E.4 Platform and Link notes

- **Qt:** vendor `qt/ArcadeLink.{h,cpp}` from Link `v0.1.0` with provenance and a drift check, as Wheel does.
  - The classes are `Locations`, `Registry` (watch + `changed()`), `Server` / `Responder`, `Client` and `Endpoint`.
  - Pass `spec/vectors/` (content matching, errors, manifest, accelerators).
- Honor `ARCADE_HOME` and a Shelf profile override in tests.
- **Shortcut:** Find's default is **Ctrl+Alt+F**. Also avoid Box Ctrl+Alt+Space (macOS Cmd+Shift+Space), Look Ctrl+Alt+Shift+Space, Lens Ctrl+Alt+Shift+L, Clipboard Ctrl+Shift+Space (Win Ctrl+Alt+V, macOS Cmd+Shift+V), and Wheel F8. Check clashes with `Registry` shortcuts.
- **Drag-and-drop:** Shelf must accept `text/uri-list` (Nautilus, and Find later) on Wayland, X11, Windows and macOS. Find v0.1 has no drag source (see G).

---

## F. Integration testing

**In Find (all passing):**

| Suite | Covers | Command |
|---|---|---|
| `link.rs` unit tests | Encoding (single, homogeneous, mixed, non-UTF-8), offer rules (array patterns, `*`, data-only, unavailable, other OS, Link off, `maxBytes`), preset/version in requests, `find.show` arguments, `find.search` results and refusals | `cargo test -p arcade-find --lib link` |
| `tests/link_shelf.rs` | In-process mock `arcade.shelf` (real `arcade_link::Server`, `Locations::under(tmp)`): offered only when installed, Link on, not disabled in Find, accepting the selection, executable present; `maxBytes` disabled text; exact `inputs`, `version: 1`, `options: {}`, `context.source: arcade.find`; nothing copied; standard errors (`unsupported_input`, `denied`, `busy`); a stopped peer without `launch.invoke` is started with `--background` and `launch_failed` reads "Arcade Shelf didn't start." | `cargo test -p arcade-find --test link_shelf` |
| `scripts/e2e-linux.sh` | The release binary in headless Sway (layer shell) and Xvfb (X11) with `arcade-link mock` peers: Enter sends `look.preview` and hides Find; a 4-item mixed selection → Tab → "shelf" → Enter sends `shelf.add` with `file/any[]` + `folder/reference` from `arcade.find`, and Find stays open showing the peer's message; one-shot `find.search` | `ARCADE_LINK_CLI=…/arcade-link scripts/e2e-linux.sh target/release/arcade-find` |

Mock fixture used for Shelf (the §C.2 contract):

```json
{ "id": "arcade.shelf", "name": "Arcade Shelf", "version": "0.0.0-mock",
  "actions": [ { "id": "shelf.add", "title": "Add to Shelf", "verb": "add",
                 "accepts": ["file/*", "file/*[]", "folder/reference", "text/plain", "text/url", "text/rich"],
                 "effects": ["persists"], "mock": { "result": { "message": "Added 2 items to Quick Shelf" } } } ] }
```

Run it yourself: `arcade-link mock --as arcade.shelf --actions shelf.json`
(from Arcade Link v0.1.0's `arcade-link-cli`), with `ARCADE_MOCK_LOG=file`
to record what Find sends.

**Not covered yet:** a peer crashing mid-call (Link reports "… isn't
running."), a stopped peer that has `launch.invoke` (one-shot path), and
cancelling a running peer call from Find.

**Only possible once Shelf exists:**
- Real Find → Shelf with Shelf stopped, running, Link-disabled, or crashing during the call.
- Shelf → Find (`find.show`).
- Adding `arcade.find` and `arcade.shelf` groups to Link's `tools/e2e.py`.
- Interactive Hyprland validation.

---

## G. Risks and compatibility

| Issue | Recommendation |
|---|---|
| File-versus-folder order is lost in mixed selections (§0.3) | Shelf appends in input order. Order matters little for a collection; don't invent a new content type |
| A peer with `launch.invoke` gets non-interactive calls one-shot | Shelf omits `launch.invoke` (§0.4) |
| Link has no glyph, ID, display name, release URL or accent for `arcade.shelf` (or `arcade.find`) | One Link onboarding branch adding both apps: `ids`, `app_name`, `app_pitch`, `releases_url`, `assets/glyphs/*.svg`, tokens, fixtures, catalog rows; then Tools mappings. **The tag and consumer bumps need the owner's approval.** Until then, both apps work through manifests alone |
| Persistence confirmation (SPEC §1.8) | Find's payload preview plus Shelf's Undo; no modal dialogs |
| Same-user IPC isn't a user gesture | Shelf validates every input, caps counts and sizes, never executes, and always shows the result with Undo |
| Peer-owned handoffs disappear | Copy before acknowledging success (§C.5) |
| Non-UTF-8 Linux paths can't travel in JSON | Both apps skip and report them instead of converting lossily |
| Deleted or moved originals | Shelf marks the item unavailable and never substitutes another file. Find's own results are live from its index |
| No Find → Shelf drag in v0.1 | Later Find can start a `wl_data_device` drag with `text/uri-list` from the layer surface; Shelf already needs to accept it from file managers. Validate on Hyprland |
| Hyprland: Find's overlay takes keyboard focus (layer-shell exclusive) | Shelf's capsule must not use exclusive keyboard focus while collapsed, or the two would fight |
| Look's multi-input preview | Look already collects paths from every input in order, so the same encoding works for Look and Shelf |

---

## H. Final integration decisions

| Question | Final decision | Basis |
|---|---|---|
| How does Find send results to Shelf? | Generic registry-driven peer action: `invoke_action` → `shelf.add` v1, options `{}`, context `{source:"arcade.find", interactive:true, reason:"user-click"}` | CODE; tested against mock peers |
| How are multiple results represented? | Files as one `file/<kind>[]`, then one `folder/reference` per folder; missing and non-UTF-8 entries are excluded and reported | CODE; tested |
| What does Shelf expose? | `shelf.add` (non-interactive, `persists`), `shelf.show`, `shelf.pick`; no `shelf.open`; no `launch.invoke` | PROPOSED |
| What does Find expose to Shelf? | `find.show` (query, folder scope, reveal); `find.search` for headless use (no history, no content search) | CODE; `find.search` tested resident and one-shot |
| Who owns temporary files? | Their creator. Find sends only user paths (never copied). Shelf copies peer handoffs into its own staging before success and cleans up its own | Link SPEC §5.3 + PROPOSED |
| How are missing peers handled? | No entry, no promotion; "Get" only on Connected apps; Find works fully standalone | CODE; tested |
| How are errors returned? | Standard Link codes; partial success is a success with `data.skipped`; Find shows `message` or the standard SPEC §6 text | Find: CODE, tested; Shelf: PROPOSED |
| How are user permissions enforced? | Find: explicit selection, payload preview, peer toggles. Shelf: input validation, quotas, no execution, Undo, Link switch | Find: CODE; Shelf: PROPOSED |
| What must ChatGPT implement next? | Shelf phases 0–3 (independent of Find); then `shelf.add`, `shelf.show` and `shelf.pick` per §E with the tests in §E.1; then the joint Link onboarding branch for `arcade.shelf` | PROPOSED |

Find's side is confirmed by code and tests against mock peers. Nothing is confirmed across the two real apps yet; that needs Shelf's `shelf.add`.
