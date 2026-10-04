#!/usr/bin/env python3
"""Configure the local GTK cache, rebuilding its setup when new options appear."""
import json
from pathlib import Path
import subprocess
import sys

build, source, *options = sys.argv[1:]
mode = []
if (Path(build) / 'build.ninja').is_file():
    configured = json.loads(subprocess.check_output(['meson', 'introspect', '--buildoptions', build], text=True))
    available = {option['name'] for option in configured}
    shared = {option['name'] for option in configured if option['section'] != 'user'}
    requested = {option[2:].split('=', 1)[0] for option in options if option.startswith('-D')}
    missing = any(name not in available and (':' not in name or name.split(':', 1)[1] not in shared) for name in requested)
    mode = ['--wipe' if missing else '--reconfigure']
subprocess.run(['meson', 'setup', *mode, build, source, *options], check=True)
