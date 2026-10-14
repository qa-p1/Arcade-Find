---
title: Getting started
order: 2
since: 0.3.0
---

# Getting started

## Install Arcade Find

Install Find from **Arcade Tools → Apps**, where Tools can install, update, and remove it. While Tools runs, it can host the app’s tray icon; Find keeps its own icon otherwise.

### Linux

Download the AppImage and run it. On first launch, choose **Install** to add Find to your app launcher, place it in **~/Applications/Arcade/**, add **arcade-find** in **~/.local/bin**, and choose whether it starts at login. Choose **Just run it once** to use Find without installing it, or **Don’t ask again for this file** to remember that choice. Start at login defaults on; removing the downloaded file also defaults on. The installed copy launches after setup. Background launches and Link/diagnostic commands never show this dialog.

To uninstall, run **arcade-find --uninstall**, use the launcher’s **Uninstall** action, or remove Find in Arcade Tools. Settings stay unless you choose to remove them. If **~/.local/bin** is not on PATH, Find shows the exact line to add for your shell.

If another copy is installed, an older version offers **Update**; the same or a newer version offers **Open installed**. You can always run this copy once. If Tools manages Find and is installed, Update hands the request to Tools for confirmation.

For scripted installation use `Arcade-Find.AppImage --install --silent`; add `--no-launch` to install without starting Find or `--channel nightly` to record a different channel. Use `--repair` from a moved AppImage to repoint the launcher and terminal command. `--integration-status` reports the receipt, missing entries and PATH hint as JSON without changing anything.

### Windows

Run the installer. Find installs for your user without administrator access. It adds a Start menu entry, offers an unchecked desktop shortcut and start-at-login option (on by default), and adds an **arcade-find.cmd** shim in **%LOCALAPPDATA%\Arcade\bin** to the user PATH. `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART` uses the same setup without starting Find; `/MERGETASKS=arcade-startup` selects startup. If SmartScreen warns about the unsigned installer, choose **More info → Run anyway**. Uninstall from **Settings → Apps** or Arcade Tools.

### macOS

Open the dmg and drag Arcade Find to Applications. If you run Find from the dmg or Downloads folder, it offers to move itself to Applications. The move copies the bundle to **~/Applications** while preserving quarantine, then relaunches it and records a receipt. The dialog offers a terminal-command symlink; if the command folder is not on PATH, Settings shows the exact commands you can run yourself. Find never edits shell profiles. Windows/macOS integration is build/CI only, not verified on a real desktop. The app is not notarized, so right-click it and choose **Open** the first time.

## Find your first file

Press **Ctrl+Alt+F**. Type part of a file name, such as **invoice**, or a path fragment, such as **Projects/report**. Use the up and down arrows to choose a result.

Press **Enter** to Quick Look in Arcade Look when Look is available; otherwise Find opens the selected item with your default app. Press **Shift+Enter** to always open with the default app.

Find indexes your home folder by default. Its first crawl can take a little time; you can search while it runs. Choose **Settings → Locations** to add another folder or drive.

## Search inside files

Install ripgrep with your package manager, then start a query with **/**, for example **/TODO ext:rs**. Find only reads file contents for this kind of query; ordinary name searches use the local index.

## Set up the shortcut

Find uses **Ctrl+Alt+F** by default. Change it in **Settings → Shortcut** if another app owns it. On Linux Wayland desktops where Find cannot register a shortcut, bind **arcade-find --toggle** in the desktop keyboard settings.

On Hyprland, see the [Hyprland guide](../HYPRLAND.md) for runtime bindings and login behavior. Find never edits your Hyprland configuration.
