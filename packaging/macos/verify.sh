#!/usr/bin/env bash
# Real bundle-copy/receipt path in a temporary HOME on the macOS CI runner.
set -euo pipefail
IMAGE=$(realpath "$1")
TEST_ROOT=$(mktemp -d)
cleanup() { hdiutil detach "$TEST_ROOT/mount" >/dev/null 2>&1 || true; rm -rf "$TEST_ROOT"; }
trap cleanup EXIT
mkdir -p "$TEST_ROOT/mount" "$TEST_ROOT/home" "$TEST_ROOT/arcade/.local/bin"
hdiutil attach "$IMAGE" -readonly -nobrowse -mountpoint "$TEST_ROOT/mount" >/dev/null
export HOME="$TEST_ROOT/home" ARCADE_HOME="$TEST_ROOT/arcade" ARCADE_FIND_HOME="$TEST_ROOT/find"
export PATH="$ARCADE_HOME/.local/bin:$PATH"
"$TEST_ROOT/mount/Arcade Find.app/Contents/MacOS/arcade-find" --install --silent --no-launch
"$ARCADE_HOME/Applications/Arcade Find.app/Contents/MacOS/arcade-find" --integration-status > "$TEST_ROOT/status.json"
python3 - "$TEST_ROOT/status.json" <<'PY'
import json,sys
s=json.load(open(sys.argv[1]))
assert s['receipt']['method'] == 'macos-bundle'
assert s['receipt']['integration']['cli']
assert all(e['exists'] for e in s['entries'])
print('macOS bundle copy, receipt, CLI and integration status passed (CI only)')
PY
"$ARCADE_HOME/Applications/Arcade Find.app/Contents/MacOS/arcade-find" --uninstall --silent
