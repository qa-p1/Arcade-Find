#!/usr/bin/env bash
# Offline: compare every vendored copy against VENDORED.md's pinned SHA-256.
set -euo pipefail
python3 scripts/check-vendored.py
