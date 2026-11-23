---
title: Works with other Arcade apps
order: 4
since: 0.2.1
---

# Works with other Arcade apps

Turn on **Connect with other Arcade apps** in Find’s Connected apps settings. Use the per-app switch to disable a connection. Press Tab on a result for the action list: other apps' actions there are the same ones, in the same order and with the same names, as Arcade's entries in your file manager's menu, best matches first. Pins, hidden actions and defaults you chose in that menu apply here too.

| App | What Find adds or receives |
|---|---|
| Arcade Look | Press Enter or choose Quick Look to preview selected files. If Look cannot handle them, Enter falls back to the default app. |
| Arcade Box | Box tools and presets that suit the selected files, and your saved Box pipelines. |
| Arcade Shelf | Add selected files or folders to Shelf by reference; Find stays open. Shelf can also ask Find to search for an item. |
| Arcade Wheel | Add a selected file or a compatible action to Wheel. |
| Arcade Clipboard | Send selected files or text to your paired devices. |
| Arcade Lens | For an image, choose Lens analysis or pin actions. Lens can ask Find to search for text or a path. |
| Arcade Tools | Tools can install, update, or remove Find in its **Apps** tab and can host the Arcade tray icon while running. |

In your file manager, Arcade's menu has **Reveal in Find** for a file or folder: Find opens on it, selected. Selected text in the browser extension or the command palette has **Search my files for this**.

Other apps can call **find.search** to get matching file paths without opening the overlay, or **find.show** to open Find on a query or item. A peer action appears only when the app is installed, enabled, and available for that selection.

Find’s Shelf flow has been tested with the real Shelf app in an isolated runner. The other peer integrations are not all verified against their current desktop builds; see [STATUS](https://github.com/qa-p1/Arcade-Find/blob/main/docs/STATUS.md).
