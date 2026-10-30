#!/usr/bin/env python3
"""Build the shipped installers from the single workspace version."""
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
os.chdir(ROOT)
version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
target = Path(os.environ.get('CARGO_TARGET_DIR', 'target')).resolve()

def run(*args): subprocess.run(args, check=True)

if sys.platform == 'darwin':
    for arch in ('x86_64-apple-darwin', 'aarch64-apple-darwin'):
        run('rustup', 'target', 'add', arch)
        run('cargo', 'build', '--locked', '--release', '-p', 'arcade-find', '--target', arch)
    (target / 'release').mkdir(exist_ok=True)
    run('lipo', '-create', '-output', str(target / 'release/arcade-find'), *(str(target / arch / 'release/arcade-find') for arch in ('x86_64-apple-darwin', 'aarch64-apple-darwin')))
    run('packaging/macos/build-app.sh', str(target / 'release/arcade-find'), version, 'universal', 'dist')
elif sys.platform == 'win32':
    run('cargo', 'build', '--locked', '--release', '-p', 'arcade-find')
    binary = target / 'release/arcade-find.exe'
    run(str(binary), '--export-icon', str(target / 'release/arcade-find.ico'))
    compiler = Path(os.environ['ProgramFiles(x86)']) / 'Inno Setup 6/ISCC.exe'
    run(str(compiler), f'/DVersion={version}', f'/DSource={target / "release"}', 'packaging/windows/arcade-find.iss')
else:
    run('cargo', 'build', '--locked', '--release', '-p', 'arcade-find')
    run('packaging/linux/build-appimage.sh', str(target / 'release/arcade-find'), 'dist')
