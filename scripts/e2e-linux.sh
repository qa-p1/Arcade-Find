#!/usr/bin/env bash
# End-to-end checks of the real binary in isolated headless sessions:
# Wayland (headless sway, layer shell) and X11 (Xvfb). Nothing touches the
# live desktop: private profile roots, private runtime dirs, private D-Bus.
#
#   scripts/e2e-linux.sh [path/to/arcade-find] [out-dir]
#
# Needs: sway, grim, wtype (Wayland); Xvfb, xdotool, import (X11).
set -euo pipefail

BIN=$(realpath "${1:-target/release/arcade-find}")
OUT=$(realpath -m "${2:-target/e2e}")
mkdir -p "$OUT"
T=$(mktemp -d /tmp/af-e2e.XXXXXX)
PIDS=()
cleanup() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done
  sleep 0.3
  rm -rf "$T"
}
trap cleanup EXIT

pass=0; fail=0
ok() { echo "  ok    $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL  $1"; fail=$((fail + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

# A small tree to index.
TREE="$T/files"
mkdir -p "$TREE/Documents/Reports" "$TREE/Projects/site/src" "$TREE/Pictures" "$TREE/.config/app" "$TREE/node_modules/pkg"
echo report >"$TREE/Documents/Reports/report-2024-q3.pdf"
echo 'fn main() {} // TODO find me' >"$TREE/Projects/site/src/report.rs"
echo x >"$TREE/Pictures/report-cover.png"
echo x >"$TREE/.config/app/hidden-report.txt"
echo x >"$TREE/node_modules/pkg/report-in-node-modules.js"

export ARCADE_FIND_HOME="$T/find"
export ARCADE_HOME="$T/arcade"
export XDG_RUNTIME_DIR="$T/run"
mkdir -p "$ARCADE_FIND_HOME/config" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
cat >"$ARCADE_FIND_HOME/config/settings.json" <<EOF
{ "schema": 1, "roots": ["$TREE"], "contentSearch": true }
EOF

af() { "$BIN" "$@"; }

wait_ready() {
  for _ in $(seq 1 100); do
    if af --status 2>/dev/null | grep -q '"phase": "ready"'; then return 0; fi
    sleep 0.1
  done
  return 1
}

rss_kib() { awk '/VmRSS/ {print $2}' "/proc/$1/status"; }
cpu_ticks() { awk '{print $14 + $15}' "/proc/$1/stat"; }

echo "== CLI without a running instance"
check "--version" '[[ "$(af --version)" == arcade-find* ]]'
check "--arcade-manifest is JSON with find.search" 'af --arcade-manifest | grep -q "\"find.search\""'
check "--status reports not running (exit 3)" '! af --status >/dev/null; [[ $? -eq 0 ]] || true; af --status >/dev/null 2>&1; [[ $? -eq 3 ]]'

run_session() {
  local name="$1"
  echo "== $name: resident instance"
  # A real Shelf started by Find (launch.background) inherits Find's
  # environment: give each session its own Shelf profile, offscreen.
  export ARCADE_SHELF_HOME="$T/shelf-$name" QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software
  ARCADE_FIND_DEBUG=1 "$BIN" --background >"$OUT/$name.log" 2>&1 &
  PIDS+=($!)
  check "$name: index ready" wait_ready
  local pid; pid=$(af --status | sed -n 's/^  "pid": \([0-9]*\),/\1/p')
  check "$name: second --background exits at once" 'timeout 5 "$BIN" --background'
  check "$name: --search finds by name" 'af --search report | grep -q "report-2024-q3.pdf"'
  check "$name: excluded node_modules stays out" '! af --search report | grep -q node_modules'
  check "$name: hidden files only with --hidden" '! af --search hidden-report | grep -q . && af --search hidden-report --hidden | grep -q hidden-report.txt'
  check "$name: filters (ext:rs)" '[[ "$(af --search "report ext:rs")" == *report.rs ]]'
  echo new >"$TREE/Documents/fresh-report.md"
  sleep 0.6
  check "$name: live update picks up a new file" 'af --search fresh-report | grep -q fresh-report.md'
  rm "$TREE/Documents/fresh-report.md"
  sleep 0.6
  check "$name: live update drops a deleted file" '! af --search fresh-report | grep -q fresh-report.md'

  # Link: the manifest and endpoint exist; find.search answers resident and one-shot.
  check "$name: Link manifest written" '[[ -f "$ARCADE_HOME/apps/arcade.find.json" ]]'
  local req='{"v":1,"id":1,"method":"invoke","params":{"action":"find.search","inputs":[{"type":"text/plain","text":"report ext:pdf"}],"options":{"limit":5},"context":{"source":"e2e","interactive":false,"reason":"test"}}}'
  check "$name: one-shot find.search" 'echo "$req" | af --arcade-invoke | grep -q "report-2024-q3.pdf"'

  # Idle cost after settling.
  sleep 2
  local c0; c0=$(cpu_ticks "$pid")
  sleep 10
  local c1; c1=$(cpu_ticks "$pid")
  echo "  info  $name idle CPU: $((c1 - c0)) ticks / 10 s; RSS $(rss_kib "$pid") KiB"
  check "$name: idle CPU ≤ 2 ticks in 10 s" '[[ $((c1 - c0)) -le 2 ]]'
}

# Real Arcade Link calls from the overlay, against `arcade-link mock` peers
# (set ARCADE_LINK_CLI to the CLI from Arcade Link v0.1.0): a mock Look,
# and either the real Arcade Shelf (ARCADE_SHELF_BIN) or a mock publishing
# its `shelf.add` contract.
link_checks() {
  local name="$1" shot="$2" typer="$3" texter="$4"
  [[ -n "${ARCADE_LINK_CLI:-}" && -x "${ARCADE_LINK_CLI:-}" ]] || { echo "  skip  $name: Link peer checks (ARCADE_LINK_CLI not set)"; return; }
  cat >"$T/look.json" <<'J'
{ "id": "arcade.look", "name": "Arcade Look", "version": "0.0.0-mock",
  "actions": [ { "id": "look.preview", "title": "Quick Look", "verb": "preview",
                 "accepts": ["file/*", "file/*[]", "folder/reference", "text/url"],
                 "effects": ["opens-ui"], "interactive": true, "mock": { "result": { "message": "Previewing" } } } ] }
J
  cat >"$T/shelf.json" <<'J'
{ "id": "arcade.shelf", "name": "Arcade Shelf", "version": "0.0.0-mock",
  "actions": [ { "id": "shelf.add", "title": "Add to Shelf", "verb": "add",
                 "accepts": ["file/*", "file/*[]", "folder/reference", "text/plain", "text/url", "text/rich"],
                 "effects": ["persists"], "mock": { "result": { "message": "Added 2 items to Quick Shelf" } } } ] }
J
  rm -f "$T/look.log" "$T/shelf.log"
  ARCADE_MOCK_LOG="$T/look.log" "$ARCADE_LINK_CLI" mock --as arcade.look --actions "$T/look.json" >/dev/null 2>&1 &
  local look=$!
  PIDS+=("$look")
  local shelf=""
  if [[ -z "${ARCADE_SHELF_BIN:-}" ]]; then
    ARCADE_MOCK_LOG="$T/shelf.log" "$ARCADE_LINK_CLI" mock --as arcade.shelf --actions "$T/shelf.json" >/dev/null 2>&1 &
    shelf=$!
    PIDS+=("$shelf")
  fi
  sleep 1
  # Enter previews the selected result in Look and hides Find.
  af --show "dir: Reports"
  sleep 0.6
  $typer FOCUS
  $typer Return
  sleep 1
  check "$name: Enter sends look.preview" 'grep -q "\"look.preview\"" "$T/look.log" && grep -q "\"folder/reference\"" "$T/look.log"'
  check "$name: Find hides for the preview" '! "$BIN" --status | grep -q "\"visible\": true"'
  # A mixed multi-selection goes to shelf.add by reference; Find stays open.
  af --show report
  sleep 0.6
  $typer FOCUS
  $typer shift+Down
  $typer shift+Down
  $typer shift+Down
  $typer Tab
  sleep 0.4
  $texter shelf
  sleep 0.4
  $shot "$OUT/$name-shelf-entry.png" || true
  if [[ -n "${ARCADE_SHELF_BIN:-}" ]]; then
    real_shelf_checks "$name" "$shot" "$typer" "$texter"
  else
    $typer Return
    sleep 1
    check "$name: shelf.add received a folder and a file by reference" 'grep -q "\"shelf.add\"" "$T/shelf.log" && grep -q "\"type\":\"file/any\[\]\"" "$T/shelf.log" && grep -q "\"type\":\"folder/reference\"" "$T/shelf.log" && grep -q "\"source\":\"arcade.find\"" "$T/shelf.log"'
    check "$name: Find stays open after a non-interactive peer action" '"$BIN" --status | grep -q "\"visible\": true"'
    $shot "$OUT/$name-shelf-done.png" || true
  fi
  $typer Escape
  kill "$look" $shelf 2>/dev/null || true
  sleep 0.3
}

# The real Arcade Shelf (ARCADE_SHELF_BIN, offscreen Qt) in place of the mock:
# items land in its SQLite store as references; a stopped Shelf is started
# through its manifest's launch.background; and Shelf's own find.show request
# (one file, "Search in Find") opens Find with that file selected.
shelf_rows() {  # prints "type|path|owned" per item in the Quick Shelf
  python3 -I - "$SHELF_HOME/shelf.sqlite3" <<'PY'
import sqlite3, sys
db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
for t, p, o in db.execute("SELECT type, path, owned FROM items WHERE shelf='quick' ORDER BY position"):
    print(f"{t}|{p}|{o}")
PY
}
shelf_ready() {
  for _ in $(seq 1 50); do "$ARCADE_LINK_CLI" status arcade.shelf >/dev/null 2>&1 && return 0; sleep 0.1; done
  return 1
}
real_shelf_checks() {
  local name="$1" shot="$2" typer="$3" texter="$4"
  SHELF_HOME="$ARCADE_SHELF_HOME"
  "$ARCADE_SHELF_BIN" --background >"$OUT/$name-shelf.log" 2>&1 &
  PIDS+=($!)
  check "$name: real Shelf resident and published" 'shelf_ready && [[ -f "$ARCADE_HOME/apps/arcade.shelf.json" ]]'
  sleep 0.5  # Find's registry watch picks the new manifest up
  # The action list was opened before Shelf registered: reopen it.
  $typer Escape
  sleep 0.3
  $typer Tab
  sleep 0.4
  $texter shelf
  sleep 0.4
  $shot "$OUT/$name-real-shelf-entry.png" || true
  $typer Return
  sleep 1.5
  shelf_rows >"$T/rows" 2>/dev/null || true
  check "$name: real Shelf stored the folder and files as references" 'grep -q "^folder/reference|$TREE/" "$T/rows" && grep -q "^file/[a-z]*|$TREE/" "$T/rows" && ! grep -q "|1$" "$T/rows"'
  check "$name: Find stays open after shelf.add" '"$BIN" --status | grep -q "\"visible\": true"'
  $shot "$OUT/$name-real-shelf-done.png" || true
  # Stopped Shelf: Find starts it with --background and the add still lands.
  "$ARCADE_LINK_CLI" quit arcade.shelf >/dev/null 2>&1 || true
  for _ in $(seq 1 30); do "$ARCADE_LINK_CLI" status arcade.shelf >/dev/null 2>&1 || break; sleep 0.1; done
  echo x >"$TREE/Pictures/shelf-launch-$name.png"  # not on the shelf yet
  sleep 0.6
  $typer Escape
  sleep 0.3
  af --show "shelf-launch-$name"
  sleep 0.6
  $typer FOCUS
  $typer Tab
  sleep 0.4
  $texter shelf
  sleep 0.4
  $typer Return
  sleep 3
  shelf_rows >"$T/rows" 2>/dev/null || true
  check "$name: stopped Shelf launched in the background and took the add" 'shelf_ready && grep -q "|$TREE/Pictures/shelf-launch-$name.png|0$" "$T/rows"'
  # Shelf → Find: exactly what Shelf's "Search in Find" sends for one file.
  $typer Escape
  sleep 0.4
  local target="$TREE/Documents/Reports/report-2024-q3.pdf"
  "$ARCADE_LINK_CLI" invoke arcade.find find.show --input-json "{\"type\":\"file/document\",\"path\":\"$target\"}" >/dev/null 2>&1 || true
  sleep 1
  check "$name: Shelf's find.show opens Find on that file" '"$BIN" --status | grep -q "\"visible\": true" && "$BIN" --status | grep -q "\"selectedPath\": \"$target\""'
  $shot "$OUT/$name-find-show-from-shelf.png" || true
  "$ARCADE_LINK_CLI" quit arcade.shelf >/dev/null 2>&1 || true
}

overlay_checks() {
  local name="$1" shot="$2" typer="$3"
  af --show report
  sleep 0.8
  $typer FOCUS
  check "$name: overlay shows (screenshot)" "$shot '$OUT/$name-results.png'"
  $typer Down
  sleep 0.3
  $shot "$OUT/$name-nav.png" || true
  $typer Tab
  sleep 0.4
  check "$name: action list (screenshot)" "$shot '$OUT/$name-actions.png'"
  check "$name: overlay visible with the action list" '"$BIN" --status | grep -q "\"mode\": \"actions\""'
  $typer Escape
  sleep 0.3
  $typer Escape
  sleep 0.4
  $shot "$OUT/$name-hidden.png" || true
  check "$name: Esc hides the overlay" '! "$BIN" --status | grep -q "\"visible\": true"'
  af --quit
  sleep 0.5
  check "$name: --quit stops the instance" '! "$BIN" --status >/dev/null 2>&1'
}

# ---- Wayland (headless sway, layer shell) ----
if command -v sway >/dev/null && command -v grim >/dev/null && command -v wtype >/dev/null; then
  cat >"$T/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1280x800 bg #3a4a5a solid_color
EOF
  WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman dbus-run-session -- sway -c "$T/sway.conf" >"$OUT/sway.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 50); do ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-? | head -1)")
  unset DISPLAY
  # Headless sway has no keyboard: keep one virtual keyboard for the whole
  # run, so the seat doesn't lose (and the overlay doesn't see focus leave)
  # each time a short-lived wtype exits.
  wtype -s 3600000 &
  PIDS+=($!)
  run_session wayland
  # A fresh virtual keyboard needs a moment before its first key arrives.
  wl_key() {
    if [[ "$1" == FOCUS ]]; then return; fi
    if [[ "$1" == shift+* ]]; then wtype -s 400 -M shift -k "${1#shift+}" -m shift; else wtype -s 400 -k "$1"; fi
  }
  wl_text() { wtype -s 400 "$1"; }
  link_checks wayland "grim" wl_key wl_text
  overlay_checks wayland "grim" wl_key
  unset WAYLAND_DISPLAY
else
  echo "  skip  Wayland (sway, grim or wtype missing)"
fi

# ---- X11 (Xvfb, winit backend) ----
if command -v Xvfb >/dev/null && command -v xdotool >/dev/null; then
  Xvfb :91 -screen 0 1280x800x24 >"$OUT/xvfb.log" 2>&1 &
  PIDS+=($!)
  export DISPLAY=:91
  sleep 0.5
  run_session x11
  x_shot() { import -window root "$1"; }
  # No window manager: give the overlay input focus directly.
  x_key() {
    if [[ "$1" == FOCUS ]]; then xdotool search --name '^Arcade Find$' windowfocus --sync 2>/dev/null || true; else xdotool key "$1"; fi
  }
  x_text() { xdotool type "$1"; }
  link_checks x11 x_shot x_key x_text
  overlay_checks x11 x_shot x_key
else
  echo "  skip  X11 (Xvfb or xdotool missing)"
fi

echo "== $pass passed, $fail failed (screenshots and logs in $OUT)"
[[ $fail -eq 0 ]]
