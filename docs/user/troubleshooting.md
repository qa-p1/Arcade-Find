---
title: Troubleshooting
order: 6
since: 0.2.1
---

# Troubleshooting

## The shortcut does not open Find

Check **Settings → Shortcut** for the current binding and status. The default is **Ctrl+Alt+F**. A desktop or another application may already own it.

On Hyprland, Find can add a runtime binding for **arcade-find --toggle**. On other Wayland desktops, add that command in **Settings → Keyboard → Custom Shortcuts** (the exact path varies by desktop). The current STATUS has not verified shortcut registration in a real desktop session.

## Search does not find an item

Check that its folder is listed in **Settings → Locations**, and that the file or folder is not excluded. Use **Settings → Index** to see crawl and watcher status, then rescan if needed.

Hidden files are off by default. Press **Ctrl+H** to include them in results, or use **hidden:yes** in a query.

## A content search returns nothing

Install ripgrep and confirm content search is enabled in Settings. Start the query with **/**. Find does not read contents during ordinary name searches.

## Results stop updating after the initial crawl

On Linux, live updates use inotify. If the system watch limit is too low for the indexed folders, Find watches what it can and rescans the remainder hourly. The Index page shows watcher health and the command for raising the limit; Find does not change system settings for you.

## The search field does not handle my writing system

Find’s custom input does not yet support IME text input or right-to-left text. The current STATUS records this as not implemented.

## I am on Windows or macOS

The app builds on both platforms in CI, but it has not been run interactively on a real desktop. If you encounter a platform issue, include the operating-system version, the steps you took, and the relevant status output.
