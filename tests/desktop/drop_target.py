#!/usr/bin/python3.12
"""A window that accepts a drop and records what it received, for the desktop harness.

    drop_target.py <result.json>

Needs GTK 3 with its Python bindings (python3-gi, gir1.2-gtk-3.0). The window's app id is "arcade-drop-target". On a
drop it writes {"uris": [...], "text": "..."} to the result file and keeps running until the harness kills it.
"""
import json
import sys

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

GLib.set_prgname("arcade-drop-target")
result = sys.argv[1]
window = Gtk.Window(title="Drop target")
window.set_default_size(900, 600)
label = Gtk.Label(label="Drop files here")
window.add(label)
targets = [Gtk.TargetEntry.new("text/uri-list", 0, 0), Gtk.TargetEntry.new("text/plain;charset=utf-8", 0, 1)]
window.drag_dest_set(Gtk.DestDefaults.ALL, targets, 1 | 2)  # copy | move
received = {"uris": [], "text": ""}


def on_data(_widget, _context, _x, _y, data, info, _time):
    if info == 0:
        received["uris"] = list(data.get_uris())
    else:
        received["text"] = data.get_text() or ""
    with open(result, "w") as handle:
        json.dump(received, handle)
    label.set_text("Dropped: " + ", ".join(received["uris"]) + received["text"])


window.connect("drag-data-received", on_data)
window.connect("destroy", Gtk.main_quit)
window.show_all()
Gtk.main()
