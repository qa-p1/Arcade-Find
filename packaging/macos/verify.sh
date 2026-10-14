#!/usr/bin/env bash
# Real bundle-copy/receipt path in a temporary HOME on the macOS CI runner.
set -euo pipefail
: "${RUNNER_TEMP:?Run this only in the macOS CI runner}"
IMAGE=$(realpath "$1")
TEST_ROOT=$(mktemp -d)
cleanup() { hdiutil detach "$TEST_ROOT/mount" >/dev/null 2>&1 || true; rm -rf "$TEST_ROOT"; }
trap cleanup EXIT
mkdir -p "$TEST_ROOT/mount" "$TEST_ROOT/download" "$TEST_ROOT/home" "$TEST_ROOT/arcade/.local/bin"
hdiutil attach "$IMAGE" -readonly -nobrowse -mountpoint "$TEST_ROOT/mount" >/dev/null
[[ -L "$TEST_ROOT/mount/Applications" && "$(readlink "$TEST_ROOT/mount/Applications")" == /Applications ]]
export HOME="$TEST_ROOT/home" ARCADE_HOME="$TEST_ROOT/arcade" ARCADE_FIND_HOME="$TEST_ROOT/find"
export PATH="$ARCADE_HOME/.local/bin:$PATH"
ditto "$TEST_ROOT/mount/Arcade Find.app" "$TEST_ROOT/download/Arcade Find.app"
"$TEST_ROOT/download/Arcade Find.app/Contents/MacOS/arcade-find" --install --silent --no-launch
"$ARCADE_HOME/Applications/Arcade Find.app/Contents/MacOS/arcade-find" --integration-status > "$TEST_ROOT/status.json"
python3 - "$TEST_ROOT/status.json" <<'PY'
import json,sys
s=json.load(open(sys.argv[1]))
assert s['receipt']['method'] == 'macos-bundle'
assert s['receipt']['integration']['cli']
assert all(e['exists'] for e in s['entries'])
PY
"$ARCADE_HOME/Applications/Arcade Find.app/Contents/MacOS/arcade-find" --uninstall --silent
python3 - "$TEST_ROOT/status.json" "$ARCADE_HOME" <<'PY'
import json,sys
from pathlib import Path
receipt=json.load(open(sys.argv[1]))['receipt']
paths=[Path(receipt['path']), Path(receipt['integration']['cli']), Path(sys.argv[2])/'installs/arcade.find.json']
if receipt['integration'].get('autostart'):
    paths.append(Path(receipt['integration']['autostart']))
for path in paths:
    assert not path.exists() and not path.is_symlink(), f'Uninstall retained {path}'
print('macOS DMG mount, copy, install, receipt, CLI and uninstall passed (CI only)')
PY
