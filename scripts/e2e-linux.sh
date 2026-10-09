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

overlay_checks() {
  local name="$1" shot="$2" typer="$3"
  af --show report
  sleep 0.8
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
  wl_key() { wtype -s 400 -k "$1"; }
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
  x_key() { xdotool search --name '^Arcade Find$' windowfocus --sync key "$1" 2>/dev/null || xdotool key "$1"; }
  overlay_checks x11 x_shot x_key
else
  echo "  skip  X11 (Xvfb or xdotool missing)"
fi

echo "== $pass passed, $fail failed (screenshots and logs in $OUT)"
[[ $fail -eq 0 ]]
