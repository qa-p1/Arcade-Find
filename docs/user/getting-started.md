---
title: Getting started
order: 2
since: 0.2.1
---

# Getting started

## Install Arcade Find

Install Find from **Arcade Tools → Apps**, where Tools can install, update, and remove it. While Tools runs, it can host the app’s tray icon; Find keeps its own icon otherwise.

### Linux

Download the AppImage and run it. On first launch, choose **Install** to add Find to your app launcher, place it in **~/Applications/Arcade/**, add **arcade-find** in **~/.local/bin**, and choose whether it starts at login. Choose **Just run it once** to use Find without installing it.

To uninstall, run **arcade-find --uninstall**, use the launcher’s **Uninstall** action, or remove Find in Arcade Tools. Settings stay unless you choose to remove them. If **~/.local/bin** is not on PATH, Find shows the exact line to add for your shell.

### Windows

Run the installer. Find installs for your user without administrator access. It adds a Start menu entry, offers a desktop shortcut and start-at-login options, and adds **arcade-find** to PATH. If SmartScreen warns about the unsigned installer, choose **More info → Run anyway**. Uninstall from **Settings → Apps** or Arcade Tools.

### macOS

Open the dmg and drag Arcade Find to Applications. If you run Find from the dmg or Downloads folder, it offers to move itself to Applications. The app is not notarized, so right-click it and choose **Open** the first time.

## Find your first file

Press **Ctrl+Alt+F**. Type part of a file name, such as **invoice**, or a path fragment, such as **Projects/report**. Use the up and down arrows to choose a result.

Press **Enter** to Quick Look in Arcade Look when Look is available; otherwise Find opens the selected item with your default app. Press **Shift+Enter** to always open with the default app.

Find indexes your home folder by default. Its first crawl can take a little time; you can search while it runs. Choose **Settings → Locations** to add another folder or drive.

## Search inside files

Install ripgrep with your package manager, then start a query with **/**, for example **/TODO ext:rs**. Find only reads file contents for this kind of query; ordinary name searches use the local index.

## Set up the shortcut

Find uses **Ctrl+Alt+F** by default. Change it in **Settings → Shortcut** if another app owns it. On Linux Wayland desktops where Find cannot register a shortcut, bind **arcade-find --toggle** in the desktop keyboard settings.
