---
title: Features
order: 3
since: 0.2.1
---

# Features

## Find by name, path, or file details

Type as you search. Unquoted words match names in any order; matching is fuzzy. Add a path fragment to search folder paths, or use filters for extension, file/folder kind, size, and modification date.

| Query | Result |
|---|---|
| report 2024 | Names matching both terms |
| "q3 report" | The exact phrase |
| src/main | Paths containing src/ and a name beginning with main |
| *.tar.gz | A wildcard filename |
| ext:pdf,docx | Files with either extension |
| dir: or file: | Only folders or only files |
| in:~/Projects | Items inside a folder |
| size:>100mb or size:1k..10m | Filter by size |
| modified:<7d, modified:today, or modified:>=2026-01-01 | Filter by modification date |
| hidden:yes | Include hidden files for this query |

Search terms combine: all words must match, in any order.

## Search inside file contents

Start a query with **/**, such as **/TODO ext:rs**. This uses the optional ripgrep program; Find never downloads it. Content search is configurable in Settings and can be disabled.

## Choose which folders are indexed

Find indexes your home folder by default. Add other folders and drives in **Settings → Locations**; extra drives are opt-in. Exclude folder names or specific paths. Common build and cache folders are excluded by default. Network mounts are skipped, and symbolic links are not followed.

The index stores names and metadata, not file contents. The first crawl runs in the background, and filesystem notifications keep the index current. The **Settings → Index** page shows index and watcher status and lets you rescan.

## Review and act on results

- Use the arrow keys, Page Up, and Page Down to move through results. Hold Shift with the arrows to select several items.
- Press Enter to preview in Look when it can open the selection, or use the default app when it cannot. Shift+Enter always uses the default app; Ctrl+Enter shows the item in its folder.
- Press Tab to browse actions for the current selection, including actions from connected Arcade apps.
- Rename with F2. Find asks for the new name and will not overwrite an existing file.
- Delete moves selected items to Trash after confirmation. Find does not permanently delete them.
- Copy a path with Ctrl+C, or copy the selected file or files with Ctrl+Shift+C. Paste files into Find with Ctrl+V.
- Pin an item with Ctrl+Shift+P. Press Down on an empty query to show recent and pinned items. Choose whether to show these items on opening in Settings.
- Press Ctrl+H to toggle hidden items, Ctrl+I for details, Ctrl+, for Settings, or F5 to refresh.
- Drag a result into another app to copy it or add it there. Find does not move the source file.

On macOS, Cmd replaces Ctrl for primary shortcuts.

## Change how Find looks and starts

Settings include theme, start-at-login, result rows, whether recent items appear on open, hidden-file defaults, content search, the global shortcut, indexed locations, exclusions, and connected apps.

## Use the command line

The **arcade-find** command can show or hide the overlay, run a name search, report status, rescan the index, or start a Link request. Run **arcade-find --help** for the full option list.
