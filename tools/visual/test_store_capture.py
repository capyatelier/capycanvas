import argparse
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


spec = importlib.util.spec_from_file_location('android_store', Path(__file__).with_name('android-store-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class AndroidStoreSafety(unittest.TestCase):
    def arguments(self, root):
        return argparse.Namespace(device='fixture', application_id='art.capycanvas.fixture',
            apk=root / 'app.apk', test_apk=root / 'test.apk', build_tools=root,
            output=root / 'output', scenes='paint', languages='ja', themes='light,dark', kind='tablet', size=None)

    def test_production_or_other_owner_identity_is_rejected_before_any_device_command(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.arguments(Path(directory))
            for identity in ['art.capycanvas', 'art.capycanvas.editor', 'art.capycanvas.anotherowner']:
                args.application_id = identity
                calls = []
                def execute(command, **kwargs):
                    calls.append(command)
                    return subprocess.CompletedProcess(command, 0, 'art.capycanvas.fixture\n')
                with self.assertRaisesRegex(ValueError, 'application ID assigned'):
                    capture.run(args, execute)
                self.assertEqual(len(calls), 1)

    def test_apk_identity_is_checked_instead_of_trusting_the_application_id_argument(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.arguments(Path(directory))
            calls = []
            def execute(command, **kwargs):
                calls.append(command)
                output = 'art.capycanvas.fixture\n' if command[-1] == 'appid' else "package: name='art.capycanvas.editor' versionCode='1'"
                return subprocess.CompletedProcess(command, 0, output)
            with self.assertRaisesRegex(ValueError, 'different application ID'):
                capture.run(args, execute)
            self.assertFalse(any('run' in command for command in calls))

    def test_instrumentation_exit_zero_without_ok_does_not_pull_or_claim_a_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.arguments(Path(directory))
            calls = []
            def execute(command, **kwargs):
                calls.append(command)
                if command[-1] == 'appid': output = args.application_id
                elif 'badging' in command:
                    package = args.application_id + ('.test' if command[-1].endswith('test.apk') else '')
                    output = f"package: name='{package}' versionCode='1'"
                elif 'xmltree' in command: output = f':targetPackage(0x01010021)="{args.application_id}"'
                elif 'install' in command: output = 'Success'
                else: output = 'INSTRUMENTATION_FAILED: runner unavailable'
                return subprocess.CompletedProcess(command, 0, output)
            with self.assertRaisesRegex(RuntimeError, 'native capture failed'):
                capture.run(args, execute)
            self.assertFalse(any('pull' in command for command in calls))

    def test_partial_captures_use_a_unique_remote_root_instead_of_reusing_old_variants(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runs = []
            for index in range(2):
                args = self.arguments(root)
                args.output = root / str(index)
                args.themes = 'light'
                calls = []
                def execute(command, **kwargs):
                    calls.append(command)
                    if command[-1] == 'appid': output = args.application_id
                    elif 'badging' in command:
                        package = args.application_id + ('.test' if command[-1].endswith('test.apk') else '')
                        output = f"package: name='{package}' versionCode='1'"
                    elif 'xmltree' in command: output = f':targetPackage(0x01010021)="{args.application_id}"'
                    elif 'install' in command: output = 'Success'
                    elif 'instrument' in command: output = 'OK (1 test)'
                    else: output = ''
                    return subprocess.CompletedProcess(command, 0, output)
                capture.run(args, execute)
                instrument = next(command for command in calls if 'instrument' in command)
                run_id = instrument[instrument.index('storeRun') + 1]
                self.assertRegex(run_id, r'^[a-f0-9]{32}$')
                pull = next(command for command in calls if 'pull' in command)
                self.assertTrue(pull[-2].endswith(f'/validation/store/{run_id}/tablet'))
                self.assertEqual(calls[-1][-1], pull[-2].removesuffix('/tablet'))
                runs.append(run_id)
            self.assertNotEqual(*runs)

    def test_display_override_is_restored_after_a_failed_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.arguments(Path(directory))
            args.size = '1080x2160'
            calls = []
            current = 'Physical size: 1440x3040\nOverride size: 1080x2280\n'
            def execute(command, **kwargs):
                nonlocal current
                calls.append(command)
                output = ''
                if command[-1] == 'appid': output = args.application_id
                elif 'badging' in command:
                    package = args.application_id + ('.test' if command[-1].endswith('test.apk') else '')
                    output = f"package: name='{package}' versionCode='1'"
                elif 'xmltree' in command: output = f':targetPackage(0x01010021)="{args.application_id}"'
                elif 'install' in command: output = 'Success'
                elif command[-3:] == ['shell', 'wm', 'size']: output = current
                elif command[-4:-1] == ['shell', 'wm', 'size']: current = 'Physical size: 1440x3040\nOverride size: ' + command[-1] + '\n'
                else: output = 'FAILURES!!!'
                return subprocess.CompletedProcess(command, 0, output)
            with self.assertRaisesRegex(RuntimeError, 'native capture failed'):
                capture.run(args, execute)
            self.assertIn('Override size: 1080x2280', current)
            self.assertEqual(calls[-2][-1], '1080x2280')

    def test_matching_test_package_cannot_instrument_a_production_app(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.arguments(Path(directory))
            calls = []
            def execute(command, **kwargs):
                calls.append(command)
                if command[-1] == 'appid': output = args.application_id
                elif 'badging' in command:
                    package = args.application_id + ('.test' if command[-1].endswith('test.apk') else '')
                    output = f"package: name='{package}' versionCode='1'"
                else: output = ':targetPackage(0x01010021)="art.capycanvas.editor"'
                return subprocess.CompletedProcess(command, 0, output)
            with self.assertRaisesRegex(ValueError, 'must target the assigned'):
                capture.run(args, execute)
            self.assertFalse(any('install' in command or 'run' in command for command in calls))


if __name__ == '__main__':
    unittest.main()
