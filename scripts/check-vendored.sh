#!/usr/bin/env bash
# Fails if a vendored file differs from Arcade Link at the pinned tag.
set -euo pipefail
TAG=v0.1.0
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
git -c advice.detachedHead=false clone -q --depth 1 --branch "$TAG" https://github.com/qa-p1/Arcade-Link "$T/link"
status=0
for f in assets/glyphs/*.svg; do cmp -s "$f" "$T/link/$f" || { echo "drift: $f"; status=1; }; done
cmp -s assets/link-tokens.json "$T/link/assets/tokens.json" || { echo "drift: assets/link-tokens.json"; status=1; }
cmp -s tools/arcade-release.py "$T/link/tools/arcade-release.py" || { echo "drift: tools/arcade-release.py"; status=1; }
[[ $status -eq 0 ]] && echo "vendored files match Arcade Link $TAG"
exit $status
