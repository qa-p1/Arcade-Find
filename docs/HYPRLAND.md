# Arcade Find on Hyprland

Find never edits `hyprland.conf`, shell profiles or other compositor files.

## Shortcut

With **Settings → Shortcut → "On Hyprland, add the shortcut as a runtime key
binding"** on (the default), Find adds the binding to the running session
with `hyprctl` (`hyprctl eval hl.bind(…)` on Lua-configured Hyprland ≥ 0.55,
`hyprctl keyword bind …` before) and adds it again whenever Hyprland reloads
its config. It's removed when Find quits.

To bind it yourself instead, turn that switch off and add (Settings shows the
exact line with your executable path):

```ini
bind = CTRL ALT, F, exec, arcade-find --toggle
```

## Start at login

Hyprland doesn't run XDG autostart entries unless your session does (for
example with uwsm or `dex`). Add:

```ini
exec-once = arcade-find --background
```

## Appearance

The overlay is a layer surface with the namespace `arcade-find` and a
fully opaque background in both light and dark themes. Its corners remain
rounded.
