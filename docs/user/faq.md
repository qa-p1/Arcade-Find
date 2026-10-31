---
title: FAQ
order: 5
since: 0.2.1
---

# Frequently asked questions

## Does Find scan the contents of every file?

No. Normal searches use a local index of names and metadata. File contents are read only for a query that starts with **/**, using ripgrep.

## Why can’t I find a drive or folder?

Find indexes your home folder by default. Add other drives or folders under **Settings → Locations**. Network mounts are skipped by default. Symbolic links are not followed. You can also check whether the path or a matching name is in the exclusion lists.

## What happens to deleted files?

Delete asks for confirmation and moves the selection to Trash. Find does not permanently delete files.

## Does Find need ripgrep?

Only for searches inside file contents. Name and path searches work without ripgrep. Find detects the program if you install it and never downloads it.

## Has Find been run on a real desktop?

No. The current STATUS says no real desktop was used for Hyprland, GNOME, X11 window managers, Windows, or macOS. UI paths were checked in headless or isolated sessions. Windows and macOS are built by CI only, not tested interactively.

## Can I use Find without Tools?

Yes. It retains its own tray, Settings, global shortcut, installer and command line. Tools only hosts the tray and sends requests directly to Find. Turning Link off keeps Find’s tray visible.

## Will Find change my shell or compositor configuration?

No. Installation owns generated launcher/login entries and a terminal symlink/shim, and shows a PATH hint. Hyprland bindings are runtime-only. See [the Hyprland guide](https://github.com/qa-p1/Arcade-Find/blob/main/docs/dev/hyprland.md); user-written configuration stays under your control.
