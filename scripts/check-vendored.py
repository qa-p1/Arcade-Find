#!/usr/bin/env python3
"""Verify the checksum inventory used by review and all three CI OSes."""
import hashlib
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parent.parent
entries = re.findall(r"^([0-9a-f]{64})  (\S+)$", (root / "VENDORED.md").read_text(encoding="utf-8"), re.M)
if not entries:
    sys.exit("VENDORED.md contains no checksums")
failed = []
for expected, name in entries:
    path = root / name
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        failed.append(name)
if failed:
    sys.exit("vendored checksum drift: " + ", ".join(failed))
print(f"vendored checksums: {len(entries)} files verified")
