---
title: Works with other Arcade apps
order: 4
since: 0.2.1
---

# Works with other Arcade apps

Turn on **Connect with other Arcade apps** in Find’s Connected apps settings. Use the per-app switch to disable a connection. Find shows only actions that accept the selected items.

| App | What Find adds or receives |
|---|---|
| Arcade Look | Press Enter or choose Quick Look to preview selected files. If Look cannot handle them, Enter falls back to the default app. |
| Arcade Box | Choose Box tools and presets for selected files, or run a saved Box pipeline from the action list. |
| Arcade Shelf | Add selected files or folders to Shelf by reference; Find stays open. Shelf can also ask Find to search for an item. |
| Arcade Wheel | Add a selected file or a compatible action to Wheel. |
| Arcade Clipboard | Send selected files or text to your paired devices. |
| Arcade Lens | For an image, choose Lens analysis or pin actions. Lens can ask Find to search for text or a path. |
| Arcade Tools | Tools can install, update, or remove Find in its **Apps** tab and can host the Arcade tray icon while running. |

Other apps can call **find.search** to get matching file paths without opening the overlay, or **find.show** to open Find on a query or item. A peer action appears only when the app is installed, enabled, and available for that selection.

Find’s Shelf flow has been tested with the real Shelf app in an isolated runner. The other peer integrations are not all verified against their current desktop builds; see [STATUS](../STATUS.md).
