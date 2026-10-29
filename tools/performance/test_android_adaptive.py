import hashlib
import importlib.util
import json
import os
import runpy
import subprocess
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
from android_adaptive import activity_identity, cleanup_steps, enable_adaptive, validate_identity, validate_ready


PACKAGE = 'art.capycanvas.other_worktree'
LABEL = 'OtherWorktreeUiMotion'


class AdaptiveIdentityTest(unittest.TestCase):
    def setUp(self):
        self.env = patch.dict(os.environ, CAPY_ANDROID_SERIAL='serial', CAPY_APPLICATION_ID=PACKAGE)
        self.env.start()
        self.addCleanup(self.env.stop)
        self.adb = ['adb', '-s', 'serial']
        self.foreground = f'topResumedActivity=ActivityRecord{{abc u0 {PACKAGE}/art.capycanvas.MainActivity t42}}'
        self.ready = json.dumps(dict(package=PACKAGE, token='fresh', pid=123))

    def test_rejects_collector_identity_mismatch_before_subprocess(self):
        for adb, package, label in [(self.adb, 'art.capycanvas.editor', LABEL), (['adb','-s','other'],PACKAGE,LABEL), (self.adb,PACKAGE,'')]:
            with self.subTest(adb=adb,package=package,label=label), patch('android_adaptive.subprocess.check_output') as command:
                with self.assertRaises(ValueError):validate_identity(adb,package,label,Path('apk'),Path('aapt'))
                command.assert_not_called()

    def test_protected_packages_rejected_even_if_environment_matches(self):
        for package in ['art.capycanvas','art.capycanvas.editor','']:
            with self.subTest(package=package), patch.dict(os.environ,CAPY_APPLICATION_ID=package), patch('android_adaptive.subprocess.check_output') as command:
                with self.assertRaises(ValueError):validate_identity(self.adb,package,LABEL,Path('apk'),Path('aapt'))
                command.assert_not_called()

    def test_installed_apk_must_match_package_label_and_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            apk=Path(directory)/'own.apk';apk.write_bytes(b'own apk')
            badging=f"package: name='{PACKAGE}'\napplication-label:'{LABEL}'\n"
            valid=[badging,'package:/data/app/own/base.apk\n',hashlib.sha256(apk.read_bytes()).hexdigest()+'  /data/app/own/base.apk']
            with patch('android_adaptive.subprocess.check_output',side_effect=valid):
                self.assertEqual(validate_identity(self.adb,PACKAGE,LABEL,apk,Path('aapt')),'serial')
            for outputs in [[badging.replace(PACKAGE,'other')],[badging.replace(LABEL,'Other')],valid[:2]+['0'*64+' base.apk']]:
                with patch('android_adaptive.subprocess.check_output',side_effect=outputs), self.assertRaises(ValueError):
                    validate_identity(self.adb,PACKAGE,LABEL,apk,Path('aapt'))

    def test_ready_rejects_stale_token_wrong_package_and_pid(self):
        validate_ready(self.ready,PACKAGE,'fresh','123')
        for package,token,pid in [(PACKAGE,'stale','123'),('other','fresh','123'),(PACKAGE,'fresh','124')]:
            with self.subTest(package=package,token=token,pid=pid), self.assertRaises(ValueError):validate_ready(self.ready,package,token,pid)

    def test_foreign_foreground_is_rejected(self):
        with self.assertRaises(ValueError):activity_identity(self.foreground.replace(PACKAGE,'other'),PACKAGE)
        with self.assertRaises(ValueError):activity_identity(self.foreground+'\nmResumedActivity: ActivityRecord{def u0 other/OtherActivity t43}',PACKAGE)

    def test_retains_exact_activity_and_task(self):
        outputs=[self.foreground,'123',self.ready,'',self.foreground,'123',self.ready,'hardware']
        with tempfile.TemporaryDirectory() as directory, patch('android_adaptive.subprocess.check_output',side_effect=outputs) as command, patch('android_adaptive.subprocess.run') as helper:
            enable_adaptive(self.adb,PACKAGE,Path('helper'),Path(directory),LABEL,Path('apk'),Path('aapt'),'remote','fresh')
            self.assertIn(self.adb+['shell','am','task','focus','42'],[call.args[0] for call in command.call_args_list])
            argv=helper.call_args.args[0]
            self.assertEqual(argv[argv.index('--package')+1],PACKAGE)
            self.assertEqual(argv[argv.index('--serial')+1],'serial')
            self.assertTrue(Path(directory,'private-activity.json').exists())

    def test_stale_ready_never_opens_settings(self):
        with tempfile.TemporaryDirectory() as directory, patch('android_adaptive.subprocess.check_output',side_effect=[self.foreground,'123',self.ready.replace('fresh','old')]), patch('android_adaptive.subprocess.run') as helper:
            with self.assertRaisesRegex(ValueError,'READY'):enable_adaptive(self.adb,PACKAGE,Path('helper'),Path(directory),LABEL,Path('apk'),Path('aapt'),'remote','fresh')
            helper.assert_not_called()

    def test_recreation_rejects_release(self):
        outputs=[self.foreground,'123',self.ready,'',self.foreground.replace('{abc','{def')]
        with tempfile.TemporaryDirectory() as directory, patch('android_adaptive.subprocess.check_output',side_effect=outputs), patch('android_adaptive.subprocess.run'):
            with self.assertRaisesRegex(ValueError,'activity changed'):enable_adaptive(self.adb,PACKAGE,Path('helper'),Path(directory),LABEL,Path('apk'),Path('aapt'),'remote','fresh')
            self.assertFalse(Path(directory,'private-activity.json').exists())


    def test_collectors_validate_identity_before_device_mutation_at_each_rate(self):
        for rate in [60,90,120]:
            for name in ['android-bubbles.py','android-viewport-presents.py']:
                with self.subTest(rate=rate,collector=name), tempfile.TemporaryDirectory() as directory:
                    script=Path(__file__).parent/name
                    shared=['--package',PACKAGE,'--photo','photo','--width','100','--height','100','--adaptive-script',__file__,'--adaptive-label',LABEL,'--adaptive-apk','own.apk','--adaptive-aapt','aapt']
                    argv=[str(script),directory]+shared
                    if name=='android-bubbles.py':argv+=['--target-hz',str(rate)]
                    else:argv+=['--serial','serial','--processor','processor','--label','gap','--workspace-workload','gap','--workspace-runner','self','--extra','expectedRefreshHz='+str(rate)]
                    with patch.object(sys,'argv',argv), patch('android_adaptive.validate_identity',side_effect=ValueError('identity gate')) as identity, patch('subprocess.run') as mutation, patch('subprocess.Popen') as launch:
                        with self.assertRaisesRegex(ValueError,'identity gate'):runpy.run_path(str(script),run_name='__main__')
                        identity.assert_called_once()
                        mutation.assert_not_called()
                        launch.assert_not_called()


class AdaptiveCleanupTest(unittest.TestCase):
    def test_cleanup_attempts_every_step_and_preserves_the_benchmark_failure(self):
        primary = subprocess.CalledProcessError(1, ['adb', 'motion'])
        cleanup = subprocess.CalledProcessError(2, ['adb', 'restore'])
        with patch('subprocess.run', side_effect=[cleanup, None]) as command:
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                with cleanup_steps(lambda: subprocess.run(['adb', 'restore'], check=True),
                                   lambda: subprocess.run(['adb', 'remaining'], check=True)):
                    raise primary
        self.assertIs(caught.exception, primary)
        self.assertIn(repr(cleanup), '\n'.join(primary.__notes__))
        self.assertEqual(command.call_count, 2)

    def test_cleanup_failure_is_raised_after_remaining_steps_when_motion_passes(self):
        first = subprocess.CalledProcessError(1, ['adb', 'stop'])
        second = subprocess.CalledProcessError(2, ['adb', 'restore'])
        with patch('subprocess.run', side_effect=[first, second, None]) as command:
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                with cleanup_steps(*(lambda index=index: subprocess.run(['adb', str(index)], check=True) for index in range(3))):
                    pass
        self.assertIs(caught.exception, first)
        self.assertIn(repr(second), '\n'.join(first.__notes__))
        self.assertEqual(command.call_count, 3)

    def run_collector_failure(self, name, failure):
        script = Path(__file__).with_name(name)
        spec = importlib.util.spec_from_file_location('collector_failure_test', script)
        collector = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(collector)
        primary = subprocess.CalledProcessError(1, ['adb', 'shell', 'touch', 'go'])
        secondary = subprocess.CalledProcessError(2, ['adb', failure])
        process = Mock()
        process.poll.return_value = None
        def wait(**kwargs):
            if failure == 'wait':
                raise secondary
            process.poll.return_value = 0
            return 0
        process.wait.side_effect = wait
        hardware_queries = 0
        display_queries = 0
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'capture'
            shared = ['--package', PACKAGE, '--photo', 'photo', '--width', '100', '--height', '100',
                      '--adaptive-script', __file__, '--adaptive-label', LABEL, '--adaptive-apk', 'own.apk', '--adaptive-aapt', 'aapt']
            argv = [str(script), str(output)] + shared
            if name == 'android-bubbles.py':
                argv += ['--target-hz', '90']
            else:
                argv += ['--serial', 'serial', '--processor', 'processor', '--label', 'trial',
                         '--workspace-workload', 'gap', '--workspace-runner', 'self', '--extra', 'expectedRefreshHz=90']
            def run(command, **kwargs):
                nonlocal hardware_queries, display_queries
                parts = command[3:] if command[1:3] == ['-s', 'serial'] else command[1:]
                text = ''
                if parts[:2] == ['shell', 'touch']:
                    raise primary
                if parts[:3] == ['shell', 'am', 'force-stop'] and failure == 'force-stop':
                    raise secondary
                if parts[:2] == ['shell', 'perfetto'] and '--stop' in parts and failure == 'trace-stop':
                    raise secondary
                if parts == ['shell', 'dumpsys thermalservice; dumpsys display']:
                    hardware_queries += 1
                    if hardware_queries == 2 and failure == 'hardware':
                        raise secondary
                if parts == ['shell', 'wm', 'size']:
                    display_queries += 1
                    if display_queries == 3 and failure == 'display-settings':
                        raise secondary
                    text = 'Physical size: 100x100'
                if parts == ['shell', 'wm', 'density']:
                    text = 'Physical density: 240'
                if parts == ['shell', 'settings', 'get', 'system', 'font_scale']:
                    text = '1'
                if parts[0] == 'pull' and parts[1].endswith('/refresh-before-primer.json'):
                    if failure == 'pull':
                        raise secondary
                    Path(parts[2], 'refresh-before-primer.json').write_text('{"rate_hz": 90}')
                return subprocess.CompletedProcess(command, 0, stdout=text)
            original_write = Path.write_text
            def write(path, *args, **kwargs):
                if path.name == 'surface-latency.json' and failure == 'write':
                    raise OSError('surface capture write failed')
                return original_write(path, *args, **kwargs)
            with patch.dict(os.environ, CAPY_ANDROID_SERIAL='serial', CAPY_APPLICATION_ID=PACKAGE), \
                    patch.object(sys, 'argv', argv), patch.object(collector, 'validate_identity'), \
                    patch.object(collector, 'enable_adaptive') as enable, \
                    patch.object(collector, 'restore_adaptive', side_effect=secondary if failure == 'restore' else None) as restore, \
                    patch.object(collector.subprocess, 'run', side_effect=run) as command, \
                    patch.object(collector.subprocess, 'Popen', return_value=process), \
                    patch.object(collector.time, 'sleep'), patch.object(Path, 'write_text', write):
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    collector.main()
            self.assertIs(caught.exception, primary)
            enable.assert_called_once()
            restore.assert_called_once()
            process.wait.assert_called_once()
            pulls = [call.args[0] for call in command.call_args_list
                     if 'pull' in call.args[0] and any(part.endswith('/refresh-before-primer.json') for part in call.args[0])]
            self.assertEqual(len(pulls), 1)
            removals = [call.args[0] for call in command.call_args_list if 'rm' in call.args[0]]
            self.assertTrue(any(any(part.endswith('/refresh-before-primer.json') for part in command) for command in removals))
            if failure != 'pull':
                self.assertEqual(json.loads((output / 'refresh-before-primer.json').read_text()), dict(rate_hz=90))
            if failure:
                notes = '\n'.join(primary.__notes__)
                self.assertIn('surface capture write failed' if failure == 'write' else repr(secondary), notes)

    def test_bubble_failure_retains_barrier_and_restores_despite_each_cleanup_failure(self):
        for failure in ['', 'force-stop', 'wait', 'write', 'hardware', 'display-settings', 'pull', 'restore']:
            with self.subTest(failure=failure):
                self.run_collector_failure('android-bubbles.py', failure)

    def test_viewport_failure_retains_barrier_and_restores_despite_each_cleanup_failure(self):
        for failure in ['', 'force-stop', 'wait', 'trace-stop', 'pull', 'restore']:
            with self.subTest(failure=failure):
                self.run_collector_failure('android-viewport-presents.py', failure)


if __name__ == '__main__':unittest.main()
