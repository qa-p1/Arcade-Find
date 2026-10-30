#!/usr/bin/env python3
"""Read the single workspace version."""
from pathlib import Path
import tomllib
print(tomllib.loads((Path(__file__).resolve().parents[1] / 'Cargo.toml').read_text())['workspace']['package']['version'])
