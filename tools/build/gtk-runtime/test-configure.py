#!/usr/bin/env python3
"""Exercise new subproject options and incremental reuse with real Meson."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class ConfigureTest(unittest.TestCase):
    def test_new_subproject_options_then_incremental_reconfigure(self):
        with tempfile.TemporaryDirectory(prefix='capy-gtk-configure-') as temporary:
            root = Path(temporary)
            source, build = root / 'source', root / 'build'
            codec = source / 'subprojects' / 'codec'
            codec.mkdir(parents=True)
            (source / 'meson.build').write_text("project('cache-probe')\nif get_option('codec')\nsubproject('codec')\nendif\n")
            (source / 'meson.options').write_text("option('codec', type: 'boolean', value: false)\n")
            (codec / 'meson.build').write_text("project('codec-probe')\n")
            (codec / 'meson.options').write_text("option('extra', type: 'feature', value: 'auto')\n")
            command = [sys.executable, str(Path(__file__).with_name('configure.py')), str(build), str(source)]
            for options in [[], ['-Dcodec=true', '-Dcodec:extra=disabled', '-Dcodec:default_library=static']]:
                result = subprocess.run([*command, *options], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            sentinel = build / 'retained-build-output'
            sentinel.write_text('keep incremental artifacts')
            result = subprocess.run([*command, '-Dcodec=true', '-Dcodec:extra=enabled', '-Dcodec:default_library=static'], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue(sentinel.is_file())
            options = json.loads(subprocess.check_output(['meson', 'introspect', '--buildoptions', str(build)], text=True))
            self.assertEqual(next(option['value'] for option in options if option['name'] == 'codec:extra'), 'enabled')


if __name__ == '__main__':
    unittest.main()
