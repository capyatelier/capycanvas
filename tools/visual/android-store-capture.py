import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid


ROOT = Path(__file__).resolve().parents[2]


def run(args, execute=subprocess.run):
    def device(*command):
        result = execute([sys.executable, str(ROOT / 'tools/devices/devices.py'), 'run', args.device,
                          '--', 'adb', *command], cwd=ROOT, capture_output=True, text=True, check=True)
        return result.stdout

    owned = execute([sys.executable, str(ROOT / 'tools/devices/devices.py'), 'appid'],
                    cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
    if args.application_id != owned or owned in ('art.capycanvas', 'art.capycanvas.editor'):
        raise ValueError('Use exactly the application ID assigned by devices.py appid')
    if args.output.exists():
        raise ValueError('Use a new capture output directory')
    for apk, expected in [(args.apk, owned), (args.test_apk, owned + '.test')]:
        badging = execute([str(args.build_tools / 'aapt2'), 'dump', 'badging', str(apk.resolve())],
                          capture_output=True, text=True, check=True).stdout
        if not badging.startswith(f"package: name='{expected}' "):
            raise ValueError('An APK has a different application ID; refusing installation')
    manifest = execute([str(args.build_tools / 'aapt2'), 'dump', 'xmltree', str(args.test_apk.resolve()),
                        '--file', 'AndroidManifest.xml'], capture_output=True, text=True, check=True).stdout
    if re.findall(r':targetPackage\([^)]+\)="([^"]+)"', manifest) != [owned]:
        raise ValueError('The instrumentation APK must target the assigned application ID')
    args.output.mkdir(parents=True)
    for apk in (args.apk, args.test_apk):
        if 'Success' not in device('install', '-r', str(apk.resolve())):
            raise RuntimeError('APK installation did not succeed')
    original_size = device('shell', 'wm', 'size') if args.size else None
    try:
        if args.size:
            device('shell', 'wm', 'size', args.size)
        run_id = uuid.uuid4().hex
        for scene in args.scenes.split(','):
            output = device('shell', 'am', 'instrument', '-w', '-e', 'class', 'art.capycanvas.AndroidStoreShowcaseTest',
                '-e', 'storeRun', run_id,
                '-e', 'storeRecipe', 'store-recipe.json', '-e', 'scene', scene, '-e', 'tags', args.languages,
                '-e', 'themes', args.themes, '-e', 'kind', args.kind,
                owned + '.test/androidx.test.runner.AndroidJUnitRunner')
            (args.output / (scene + '.log')).write_text(output, encoding='utf-8')
            if not re.search(r'OK \(1 test\)', output) or 'FAILURES' in output or 'INSTRUMENTATION_FAILED' in output:
                raise RuntimeError(f'{scene}: native capture failed; see the local instrumentation log')
        remote = f'/sdcard/Android/data/{owned}/files/validation/store/{run_id}'
        device('pull', remote + '/' + args.kind, str(args.output))
        device('shell', 'rm', '-rf', remote)
    finally:
        if original_size is not None:
            override = re.search(r'Override size: (\d+x\d+)', original_size)
            device('shell', 'wm', 'size', override.group(1) if override else 'reset')
            if device('shell', 'wm', 'size').strip() != original_size.strip():
                raise RuntimeError('The original Android display size was not restored')



def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description='Capture native store scenes on an already reserved Android device.')
    parser.add_argument('--device', required=True)
    parser.add_argument('--application-id', required=True)
    parser.add_argument('--apk', type=Path, required=True)
    parser.add_argument('--test-apk', type=Path, required=True)
    parser.add_argument('--build-tools', type=Path, required=True, help='Android SDK build-tools directory containing aapt2')
    parser.add_argument('--kind', choices=['phone', 'tablet'], required=True)
    parser.add_argument('--languages', required=True)
    parser.add_argument('--size', help='Optional Android viewport override WIDTHxHEIGHT, restored after capture')
    parser.add_argument('--scenes', default='paint,sketch,photo')
    parser.add_argument('--themes', default='light,dark')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    for value in [*args.languages.split(','), *args.scenes.split(',')]:
        if not re.fullmatch(r'[a-zA-Z0-9-]+', value):
            parser.error('Languages and scenes must be comma-separated tags')
    if not set(args.themes.split(',')) <= {'light', 'dark'}:
        parser.error('Themes must be light and/or dark')
    if args.size:
        match = re.fullmatch(r'(\d+)x(\d+)', args.size)
        if not match:
            parser.error('--size must be WIDTHxHEIGHT')
        width, height = map(int, match.groups())
        if min(width, height) < 320 or max(width, height) > 3840 or max(width, height) > 2 * min(width, height):
            parser.error('--size must meet Play dimensions: 320–3840 pixels per side, aspect ratio at most 2:1')
    run(args)


if __name__ == '__main__':
    main()
