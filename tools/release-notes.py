#!/usr/bin/env python3
"""Print one version's section of CHANGELOG.md (for the GitHub release body)."""
import re
import sys

path, version = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read()
match = re.search(rf"^## {re.escape(version)}\b[^\n]*\n(.*?)(?=^## |\Z)", text, re.M | re.S)
if not match:
    sys.exit(f"{path} has no section for {version}")
print(match.group(1).strip())
