# Arcade Find and Arcade Shelf

Find and [Arcade Shelf](https://github.com/qa-p1/Arcade-Shelf) work together
through Arcade Link v1 only: neither app has code specific to the other, and
each works fully without the other installed. Updated 2026-10-10.

## What the user can do

| From | Gesture | What happens |
|---|---|---|
| Find | Select results (Shift+arrows), Tab, "Add to Shelf" (or type `shelf`), Enter | The files and folders go onto Shelf's active collection as references. Find stays open and shows Shelf's answer ("Added 4 items to Quick Shelf"). If Shelf isn't running it is started in the background first. |
| Shelf | An item's menu → "Search in Find" | Find opens on it: a file is selected ("reveal"), a folder scopes the search (`in:"~/folder" `), a text note becomes the query. |
| Shelf | A text item's menu → "Find matching files" | Find's matches for that text are added to the collection (by reference, at most 50). |

Nothing is copied or moved: Find only sends paths the user selected, and
Shelf stores them as references and never deletes originals.

## Contract

**Find → Shelf: `shelf.add` v1** (non-interactive, effect `persists`, Shelf's
manifest has no `launch.invoke`, so a stopped Shelf is started with
`--background` and the resident process persists the add).

```json
{"action":"shelf.add","version":1,
 "inputs":[{"type":"file/any[]","paths":["/home/u/p/logo.svg","/home/u/docs/brief.pdf"]},
           {"type":"folder/reference","path":"/home/u/p/fonts"}],
 "options":{},"context":{"source":"arcade.find","interactive":true,"reason":"user-click"}}
```

Files travel as one `file/<kind>[]` (one file: `file/<kind>` with `size`),
then one `folder/reference` per folder; missing and non-UTF-8 paths are left
out and reported. Shelf answers synchronously with a complete sentence in
`message` and `data.{shelf, added, skipped}`; partial success is success.
Find offers the entry by its generic rules (installed, Link on, not switched
off in Connected apps, every value accepted; see [ARCADE_LINK.md](ARCADE_LINK.md)).
It hides its overlay only for actions that open UI, so collecting doesn't
close the search.

**Shelf → Find: `find.show` v1** (interactive, `opens-ui`) and
**`find.search` v1** (non-interactive, one-shot from the saved index, no
history and no content search). Shelf offers them by its own generic rules:
Find's actions take one value, so they appear for a single file, folder or
text item, never for a multi-selection. Shelf imports `find.search`'s outputs
like any peer output.

## Evidence

All on Linux, in isolated profiles (no real desktop, files or keyring):

| Check | Result |
|---|---|
| `scripts/e2e-linux.sh` with `ARCADE_SHELF_BIN` (the real Shelf, offscreen Qt) | 49/49 in headless Sway (layer shell) and Xvfb: a 4-item mixed selection lands in Shelf's SQLite store as references (`owned=0`); Find stays open; a stopped Shelf is launched from its manifest and takes the add; Shelf's exact `find.show` request opens Find with that file selected |
| Arcade Link `tools/e2e.py --only find` (onboarding branch) | 3/3: `find.search` resident and one-shot; `find.show` as Shelf sends it (file and folder); Find's results added to the real Shelf by reference |
| Arcade Link `tools/e2e.py --only shelf` | 3/3 (Shelf's resident adds, background relaunch, picker cancellation) |
| Shelf `tst_PeerHub::findManifestOffersShowAndSearch` | Shelf's offer rules against Find's real manifest (`arcade-find --arcade-manifest`) |
| Shelf `tst_Ui::actionsMenuDrivesFind` | Shelf's real window, clicked: Actions → "Search in Find" sends `find.show` with the selected file; "Find matching files" on a text item sends `find.search` and the returned file lands on the shelf by reference |
| Drag and drop | `scripts/e2e-linux.sh` with the real Shelf as a visible window: two results dragged from Find's overlay onto Shelf land as references, in headless Sway (Wayland data device, virtual pointer) and on Xvfb (XDND); one result dragged from the winit window (GNOME's path, forced on Sway) lands the same way; Find stays open |
| File kinds | Find's extension table equals Link's Rust and Qt tables (142 entries), so Shelf never rejects a Find result as `type_mismatch` |
| Find `tests/link_shelf.rs` | Offer rules, payloads, standard errors and background launch against an in-process peer with Shelf's contract |

Not verified: real desktops (Hyprland, KDE, GNOME, Windows, macOS); dragging
on Windows and macOS (built and type-checked by CI, never run interactively).

## Limits and next steps

- Drag a result (or the selection) from Find's overlay onto Shelf; it
  arrives as `text/uri-list` and is stored by reference. On GNOME (no layer
  shell) the drag comes from Find's winit window through the same Wayland
  data device (tested on Sway, not on Mutter).
- Both apps are in Link's shared catalogs since `v0.2.0`; Tools installs them
  and Wheel slots can hold their actions ([ARCADE_LINK.md](ARCADE_LINK.md#family-onboarding)).
- On Hyprland, Find's overlay takes exclusive keyboard focus while open;
  Shelf's collapsed capsule never takes focus, so they don't fight.
