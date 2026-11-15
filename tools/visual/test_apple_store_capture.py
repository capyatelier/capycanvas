import importlib.util
from pathlib import Path
import plistlib
import struct
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('apple_store', Path(__file__).with_name('apple-store-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class AppleStoreCapture(unittest.TestCase):
    def test_capture_requires_the_worktrees_owned_bundle_before_install_or_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / 'Capture.app'
            app.mkdir()
            for bundle in ['art.capycanvas.CapyCanvas', 'art.capycanvas.CapyCanvas.dev',
                           'art.capycanvas.editor', 'art.capycanvas.apple.ipad', 'art.capycanvas.other']:
                (app / 'Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier': bundle}))
                with self.assertRaisesRegex(ValueError, 'bundle ID assigned'):
                    capture.validate_identity(app, 'art.capycanvas.fixture')
            (app / 'Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier': 'art.capycanvas.fixture'}))
            self.assertEqual(capture.validate_identity(app, 'art.capycanvas.fixture')['CFBundleIdentifier'], 'art.capycanvas.fixture')

    def test_receipts_reject_stale_variants_unready_pixels_and_missing_font_checks(self):
        job = {'device': 'ipad', 'languages': ['ja'], 'themes': ['dark'], 'scene': 'photo'}
        report = {'token': 'ja/photo-dark', 'device': 'ipad', 'language': 'ja', 'scene': 'photo', 'theme': 'dark',
                  'recipe_sha256': 'a' * 64, 'canvas_ready': True, 'brush_ready': True, 'rendering_pending': False,
                  'previews_ready': True, 'glyphs_checked': 20, 'missing_glyphs': [], 'exact_scopes': ['histogram']}
        self.assertEqual(capture.validate_receipt(report, job, 'a' * 64), ('ja', 'photo', 'dark'))
        for key, value in [('token', 'en/photo-dark'), ('device', 'mac'), ('theme', 'light'), ('scene', 'paint'),
                           ('recipe_sha256', 'b' * 64), ('canvas_ready', False), ('brush_ready', False),
                           ('rendering_pending', True), ('previews_ready', False), ('glyphs_checked', 0),
                           ('missing_glyphs', ['missing']), ('exact_scopes', [])]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                capture.validate_receipt(dict(report, **{key: value}), job, 'a' * 64)
        for key in ['rendering_pending', 'missing_glyphs', 'glyphs_checked', 'previews_ready']:
            invalid = dict(report)
            del invalid[key]
            with self.subTest(missing=key), self.assertRaises(ValueError):
                capture.validate_receipt(invalid, job, 'a' * 64)
        phone = dict(job, device='iphone')
        phone_report = dict(report, device='iphone', exact_scopes=[])
        with self.assertRaisesRegex(ValueError, 'declare native scope visibility'):
            capture.validate_receipt(phone_report, phone, 'a' * 64)
        phone_report['native_adaptation'] = {'histogram': 'Native Properties drawer shows Vibrance controls.'}
        self.assertEqual(capture.validate_receipt(phone_report, phone, 'a' * 64), ('ja', 'photo', 'dark'))

    def test_native_dimensions_are_checked_without_resizing_the_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            png = Path(directory) / 'capture.png'
            for family, width, height in [('mac', 2560, 1600), ('ipad', 2752, 2064), ('iphone', 1206, 2622)]:
                data = b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR' + struct.pack('>II', width, height)
                png.write_bytes(data)
                self.assertEqual(capture.dimensions(png, family), (width, height))
                self.assertEqual(png.read_bytes(), data)
            png.write_bytes(data[:16] + struct.pack('>II', 1000, 1000))
            with self.assertRaisesRegex(ValueError, 'do not resize'):
                capture.dimensions(png, 'iphone')

    def test_reused_binary_metadata_must_match_the_actual_executable_and_family(self):
        binary = {'sha256': 'a' * 64, 'platform': 'iphoneos', 'family': 'ipad'}
        capture.validate_binary(binary, 'a' * 64, 'iphoneos', 'ipad')
        for key, value in [('sha256', 'b' * 64), ('platform', 'iphonesimulator'), ('family', 'iphone')]:
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'binary metadata'):
                capture.validate_binary(dict(binary, **{key: value}), 'a' * 64, 'iphoneos', 'ipad')

    def test_color_encoding_keeps_the_native_capture_and_its_dimensions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            png, native = root / 'capture.png', root / 'native.png'
            data = b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR' + struct.pack('>II', 2752, 2064)
            for profile in ['sRGB IEC61966-2.1', 'Display P3']:
                png.write_bytes(data)
                calls = []
                def execute(command):
                    calls.append(command)
                    if '-g' in command:
                        return f'{native}\n  profile: {profile}\n'
                    self.assertEqual(command, ['/usr/bin/sips', '--matchTo', str(capture.SRGB), str(png)])
                    png.write_bytes(data + b'color-encoded')
                    return ''
                report = capture.encode_capture(png, native, 'ipad', execute)
                self.assertEqual(native.read_bytes(), data)
                self.assertEqual(report['native_capture_sha256'], capture.digest(native))
                self.assertEqual(report['color_encoding'], 'sRGB')
                self.assertEqual(capture.dimensions(png, 'ipad'), (2752, 2064))
                self.assertEqual(len(calls), 1 if profile.startswith('sRGB') else 2)


if __name__ == '__main__':
    unittest.main()
