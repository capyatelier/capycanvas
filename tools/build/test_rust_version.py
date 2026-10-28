import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CHECK = ROOT / 'tools/build/check-rust-version.py'
RUN = ROOT / 'apps/layer-linux/run.sh'


class RustVersionGateTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='capy-rust-version-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.compiler = self.root / 'rustc'

    def run_with_version(self, version, command):
        self.compiler.write_text(f'#!/bin/sh\nprintf "rustc {version} (test)\\n"\n')
        self.compiler.chmod(0o755)
        env = {**os.environ, 'RUSTC': str(self.compiler),
               'CAPY_GTK_BUILD_DIR': str(self.root / 'gtk-runtime')}
        return subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True,
                              timeout=10)

    def test_old_rust_stops_launcher_before_gtk_build(self):
        result = self.run_with_version('1.94.1', ['bash', str(RUN)])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Capy Canvas requires Rust 1.99.0 or newer; found rustc 1.94.1', result.stderr)
        self.assertFalse((self.root / 'gtk-runtime').exists())

    def test_minimum_and_newer_rust_pass(self):
        for version in ['1.99.0', '1.100.0']:
            with self.subTest(version=version):
                result = self.run_with_version(version, [sys.executable, str(CHECK)])
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_every_workspace_package_declares_the_minimum(self):
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'],
            cwd=ROOT, text=True))
        self.assertTrue(metadata['packages'])
        for package in metadata['packages']:
            with self.subTest(package=package['name']):
                self.assertEqual(package['rust_version'], '1.99.0')


if __name__ == '__main__':
    unittest.main()
