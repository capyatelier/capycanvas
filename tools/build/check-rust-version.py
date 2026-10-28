#!/usr/bin/env python3
"""Reject Rust toolchains older than the workspace minimum before native setup."""
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


root = Path(__file__).resolve().parents[2]
required = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['rust-version']
compiler = os.environ.get('RUSTC', 'rustc')
try:
    release = subprocess.check_output([compiler, '--version'], text=True).strip()
except (OSError, subprocess.CalledProcessError) as error:
    sys.exit(f'Could not check Rust version with {compiler}: {error}')
match = re.match(r'^rustc (\d+)\.(\d+)\.(\d+)', release)
if not match:
    sys.exit(f'Could not read Rust version from {compiler}: {release}')
if tuple(map(int, match.groups())) < tuple(map(int, required.split('.'))):
    sys.exit(f'Capy Canvas requires Rust {required} or newer; found {release}. Update your Rust toolchain before building.')
