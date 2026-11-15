#!/usr/bin/env python3
"""Capture the production Apple editor and Metal canvas with staged store scenes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import struct
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
APPLE = ROOT / 'apps/layer-apple'
DEVICES = ROOT / 'tools/devices/devices.py'
SRGB = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read_info(app):
    return plistlib.loads((app / ('Contents/Info.plist' if (app / 'Contents').exists() else 'Info.plist')).read_bytes())


def validate_identity(app, owner):
    info = read_info(app)
    if info['CFBundleIdentifier'] != owner or not owner.startswith('art.capycanvas.') or owner in {
            'art.capycanvas.editor', 'art.capycanvas.CapyCanvas', 'art.capycanvas.CapyCanvas.dev',
            'art.capycanvas.apple.mac', 'art.capycanvas.apple.ipad'}:
        raise ValueError('Use only the bundle ID assigned to this worktree by devices.py appid')
    return info


def validate_receipt(report, job, recipe_hash):
    token = report.get('token', '')
    language, _, variant = token.partition('/')
    scene, _, theme = variant.rpartition('-')
    if (language not in job['languages'] or scene != job['scene'] or theme not in job['themes'] or
            any(report.get(k) != v for k, v in {'device': job['device'], 'language': language,
                'scene': scene, 'theme': theme, 'recipe_sha256': recipe_hash}.items())):
        raise ValueError('Receipt does not match the selected recipe and variant')
    if (report.get('canvas_ready') is not True or report.get('brush_ready') is not True or
            report.get('rendering_pending') is not False or report.get('previews_ready') is not True or
            not report.get('glyphs_checked') or report.get('missing_glyphs') != []):
        raise ValueError('Native canvas, previews or fonts are not ready')
    if scene == 'photo':
        if job['device'] == 'iphone':
            if not report.get('native_adaptation', {}).get('histogram'):
                raise ValueError('Phone captures must declare native scope visibility')
        elif 'histogram' not in report.get('exact_scopes', []):
            raise ValueError('Photo requires its visible composite Histogram to be exact')
    return language, scene, theme


def validate_binary(binary, executable_hash, platform, family):
    if any(binary.get(key) != value for key, value in
           {'sha256': executable_hash, 'platform': platform, 'family': family}.items()):
        raise ValueError('Capture binary metadata does not match the executable, platform and device family')


def dimensions(path, device):
    with path.open('rb') as stream:
        header = stream.read(24)
    if header[:16] != b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR':
        raise ValueError('Capture must be a native PNG')
    size = struct.unpack('>II', header[16:24])
    accepted = {'mac': {(1280, 800), (1440, 900), (2560, 1600), (2880, 1800)},
                'ipad': {(2752, 2064), (2732, 2048)}, 'iphone': {(1206, 2622), (1179, 2556), (1320, 2868)}}
    if size not in accepted[device]:
        raise ValueError(f'Native {device} screenshot has unsupported dimensions {size}; do not resize it')
    return size


def encode_capture(path, native, device, execute):
    size = dimensions(path, device)
    shutil.copyfile(path, native)
    profile = execute(['/usr/bin/sips', '-g', 'profile', str(native)]).split('profile:', 1)[1].strip()
    if profile != 'sRGB IEC61966-2.1':
        execute(['/usr/bin/sips', '--matchTo', str(SRGB), str(path)])
    if dimensions(path, device) != size:
        raise ValueError('Color encoding changed the native screenshot dimensions')
    return {'native_capture_sha256': digest(native), 'native_color_profile': profile, 'color_encoding': 'sRGB'}


class Capture:
    def __init__(self, args):
        self.args = args
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.logs = self.output / 'diagnostics'
        self.logs.mkdir()
        self.env = dict(os.environ, DEVELOPER_DIR=os.environ.get('DEVELOPER_DIR', '/Applications/Xcode.app/Contents/Developer'))
        self.owner = self.run(['python3', str(DEVICES), 'appid']).strip()
        self.revision = self.run(['git', 'rev-parse', 'HEAD']).strip()
        self.dirty = bool(self.run(['git', 'status', '--porcelain', '--untracked-files=no']).strip())
        self.info = validate_identity(args.assets_app.resolve(), self.owner)
        self.platform = self.info['DTPlatformName']
        if self.platform not in {'macosx', 'iphoneos', 'iphonesimulator'}:
            raise ValueError('Expected a built native Apple app')
        if (args.device == 'mac') != (self.platform == 'macosx'):
            raise ValueError('Device family and binary platform differ')
        if self.platform == 'iphoneos' and not args.physical:
            raise ValueError('A physical build needs a reserved --physical device')
        if self.platform == 'iphonesimulator' and not args.simulator:
            raise ValueError('A simulator build needs an owned --simulator')
        if self.platform != 'macosx' and self.info.get('UIDeviceFamily') != [1, 2]:
            raise ValueError('The iOS binary must target device families 1,2')

    def run(self, command, *, check=True, env=None):
        result = subprocess.run(command, env=env or self.env, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.counter = getattr(self, 'counter', 0) + 1
        (self.logs / f'{self.counter:04d}-{Path(command[0]).name}.log').write_text(result.stdout)
        if check and result.returncode:
            raise RuntimeError(f'Command failed ({result.returncode}): {command[0]}; {self.logs}')
        return result.stdout

    def device(self, *command, check=True):
        return self.run(['python3', str(DEVICES), '--platform', 'apple', 'run', self.args.physical,
                         '--', 'xcrun', 'devicectl', 'device', *command], check=check)

    def transfer(self, direction, source, target, check=True):
        return self.device('copy', direction, '--device', self.args.physical, '--domain-type', 'appDataContainer',
            '--domain-identifier', self.owner, '--source', str(source), '--destination', str(target), check=check)

    def build(self):
        mac = self.platform == 'macosx'
        rust_target = {'macosx': 'aarch64-apple-darwin', 'iphoneos': 'aarch64-apple-ios', 'iphonesimulator': 'aarch64-apple-ios-sim'}[self.platform]
        rust_root = ROOT / ('target' if mac else 'target/ios-17.5')
        sources = [p for group in ['Shared', 'macOS' if mac else 'iOS'] for p in (APPLE / group).rglob('*.swift')
                   if 'Tests' not in p.parts and p.name not in {'CapyCanvasApp.swift', 'CapyCanvasMacApp.swift'}]
        sources += [APPLE / 'tests/support/editor-store.swift', APPLE / 'tests/store-showcase.swift']
        source_hash = hashlib.sha256(json.dumps({str(p.relative_to(ROOT)): digest(p) for p in sorted(sources)}, sort_keys=True).encode()).hexdigest()
        if self.args.fixture_app:
            self.app = self.args.fixture_app.resolve()
            validate_identity(self.app, self.owner)
            resources = self.app / 'Contents/Resources' if mac else self.app
            provenance = json.loads((resources / 'capture-provenance.json').read_text())
            if (provenance['swift_sources_sha256'] != source_hash or
                    provenance['rust_archive_sha256'] != digest(rust_root / rust_target / 'debug/liblayer_apple.a') or
                    digest(resources / 'store-recipe.json') != digest(self.args.inputs / 'store-recipe.json')):
                raise ValueError('The supplied capture fixture has stale Swift, Rust or recipe inputs')
            executable = self.app / ('Contents/MacOS/' if mac else '') / self.info['CFBundleExecutable']
            self.executable_hash = digest(executable)
            validate_binary(json.loads((self.app.parent / 'binary.json').read_text()),
                            self.executable_hash, self.platform, self.args.device)
            shutil.copyfile(self.app.parent / 'binary.json', self.output / 'binary.json')
            return
        app = self.output / 'StoreCapture.app'
        shutil.copytree(self.args.assets_app, app)
        resources = app / 'Contents/Resources' if mac else app
        executable = app / ('Contents/MacOS/' if mac else '') / self.info['CFBundleExecutable']
        for path in self.args.inputs.iterdir():
            if path.is_file():
                shutil.copyfile(path, resources / path.name)
        sdk = self.run(['xcrun', '--sdk', self.platform, '--show-sdk-path']).strip()
        target = 'arm64-apple-macosx15.0' if mac else 'arm64-apple-ios17.5' + ('-simulator' if self.platform == 'iphonesimulator' else '')
        command = ['xcrun', '--sdk', self.platform, 'swiftc', '-parse-as-library', '-module-name', 'CapyStoreCapture',
            '-sdk', sdk, '-target', target, '-D', 'DEBUG', '-whole-module-optimization',
            '-import-objc-header', str(APPLE / 'native/include/CapyApple.h'), *map(str, sorted(sources)),
            '-L', str(rust_root / rust_target / 'debug'), '-llayer_apple', '-lsqlite3', '-lc++',
            '-framework', 'Metal', '-framework', 'QuartzCore', '-framework', 'Security',
            '-framework', 'SwiftUI', '-framework', 'AppKit' if mac else 'UIKit', '-o', str(executable)]
        self.run(command)
        (resources / 'capture-provenance.json').write_text(json.dumps({'swift_sources_sha256': source_hash,
            'rust_archive_sha256': digest(rust_root / rust_target / 'debug/liblayer_apple.a')}, indent=2))
        info_path = app / ('Contents/Info.plist' if mac else 'Info.plist')
        info = dict(self.info)
        if not mac:
            info['UIApplicationSceneManifest'] = {'UIApplicationSupportsMultipleScenes': False,
                'UISceneConfigurations': {'UIWindowSceneSessionRoleApplication': [{'UISceneConfigurationName': 'StoreCapture'}]}}
            info['UIRequiresFullScreen'] = True
            info['UISupportedInterfaceOrientations'] = ['UIInterfaceOrientationPortrait'] if self.args.device == 'iphone' else ['UIInterfaceOrientationLandscapeLeft', 'UIInterfaceOrientationLandscapeRight']
            info['UISupportedInterfaceOrientations~ipad'] = info['UISupportedInterfaceOrientations']
        info_path.write_bytes(plistlib.dumps(info))
        entitlements = self.output / 'entitlements.plist'
        if self.platform == 'iphoneos':
            if not self.args.signing_identity:
                raise ValueError('A physical capture app needs --signing-identity')
            data = subprocess.run(['codesign', '-d', '--entitlements', ':-', str(self.args.assets_app)], capture_output=True, check=True).stdout
            entitlements.write_bytes(data)
            self.run(['codesign', '--force', '--sign', self.args.signing_identity, '--entitlements', str(entitlements), str(app)])
        else:
            self.run(['codesign', '--force', '--sign', '-', str(app)])
        self.app, self.executable_hash = app, digest(executable)
        self.run(['codesign', '--verify', '--deep', '--strict', str(app)])
        (self.output / 'binary.json').write_text(json.dumps({'sha256': self.executable_hash,
            'production_executable_sha256': digest(self.args.assets_app / ('Contents/MacOS/' if mac else '') / self.info['CFBundleExecutable']),
            'platform': self.platform, 'family': self.args.device, 'fixture': 'tests/store-showcase.swift'}, indent=2))

    def capture(self):
        recipe_hash = digest(self.args.inputs / 'store-recipe.json')
        installed = False
        process = None
        try:
            if self.platform == 'iphoneos':
                self.device('install', 'app', '--device', self.args.physical, str(self.app)); installed = True
            elif self.platform == 'iphonesimulator':
                self.run(['xcrun', 'simctl', 'install', self.args.simulator, str(self.app)]); installed = True
            for scene in self.args.scenes.split(','):
                run = uuid.uuid4().hex
                job = {'run': run, 'device': self.args.device, 'scene': scene,
                    'languages': self.args.languages.split(','), 'themes': self.args.themes.split(',')}
                local = self.logs / run
                local.mkdir()
                job_path = local / 'job.json'
                job_path.write_text(json.dumps(job))
                if self.platform == 'macosx':
                    remote = local / run
                    remote.mkdir(parents=True)
                    process = subprocess.Popen([str(self.app / 'Contents/MacOS' / self.info['CFBundleExecutable']), '--job', str(job_path)],
                        env=dict(self.env, CAPY_STORAGE_DIR=str(remote / 'storage')), stdout=(local / 'runtime.log').open('w'), stderr=subprocess.STDOUT)
                elif self.platform == 'iphonesimulator':
                    container = Path(self.run(['xcrun', 'simctl', 'get_app_container', self.args.simulator, self.owner, 'data']).strip())
                    remote = container / 'Documents/StoreCapture' / run
                    remote.mkdir(parents=True)
                    shutil.copyfile(job_path, remote.parent / 'job.json')
                    self.run(['xcrun', 'simctl', 'launch', self.args.simulator, self.owner, '--job', 'job.json'])
                else:
                    remote = Path('Documents/StoreCapture') / run
                    self.transfer('to', job_path, 'Documents/StoreCapture/job.json')
                    self.device('process', 'launch', '--device', self.args.physical, '--terminate-existing', self.owner, '--', '--job', 'job.json')
                seen = set()
                deadline = time.monotonic() + 240
                while True:
                    if time.monotonic() > deadline:
                        raise RuntimeError(f'Native capture readiness deadline: {scene}; inspect {local}')
                    ready = local / 'ready.json'
                    if self.platform == 'iphoneos':
                        ready.unlink(missing_ok=True)
                        self.transfer('from', remote / 'ready.json', ready, check=False)
                    elif (remote / 'ready.json').exists():
                        shutil.copyfile(remote / 'ready.json', ready)
                    report = json.loads(ready.read_text()) if ready.exists() else {}
                    if 'error' in report:
                        raise RuntimeError(report['error'])
                    if report.get('complete'):
                        if len(seen) != len(job['languages']) * len(job['themes']):
                            raise RuntimeError('Native fixture completed without all selected variants')
                        break
                    token = report.get('token')
                    if not token or token in seen:
                        time.sleep(0.25 if self.platform != 'iphoneos' else 1)
                        continue
                    language, _, theme = validate_receipt(report, job, recipe_hash)
                    output = self.output / self.args.device / language
                    output.mkdir(parents=True, exist_ok=True)
                    image = output / f'{scene}-{theme}.png'
                    if image.exists():
                        raise ValueError('Capture outputs must be new')
                    if self.platform == 'macosx':
                        self.run(['swift', str(ROOT / 'tools/visual/mac-capture.swift'), str(local / token.replace('/', '-')),
                            '--pid', str(process.pid)])
                        shutil.copyfile(local / token.replace('/', '-') / 'native-mac.png', image)
                    elif self.platform == 'iphonesimulator':
                        self.run(['xcrun', 'simctl', 'io', self.args.simulator, 'screenshot', str(image)])
                    else:
                        self.device('capture', 'screenshot', '--device', self.args.physical, '--destination', str(image))
                    width, height = dimensions(image, self.args.device)
                    report.update(encode_capture(image, local / (token.replace('/', '-') + '-native.png'), self.args.device, self.run))
                    report.update(executable_sha256=self.executable_hash, width=width, height=height,
                        png_sha256=digest(image), simulator=self.platform == 'iphonesimulator',
                        app_revision=self.revision, checkout_dirty=self.dirty)
                    image.with_suffix('.json').write_text(json.dumps(report, indent=2, ensure_ascii=False))
                    seen.add(token)
                    ack = local / 'ack.json'
                    ack.write_text(token)
                    if self.platform == 'iphoneos':
                        self.transfer('to', ack, remote / 'ack.json')
                    else:
                        shutil.copyfile(ack, remote / 'ack.json')
                    print(f'CAPY_STORE_CAPTURE {self.args.device} {token}', flush=True)
                    deadline = time.monotonic() + 240
                if process:
                    process.wait(timeout=30); process = None
                elif self.platform == 'iphonesimulator':
                    self.run(['xcrun', 'simctl', 'terminate', self.args.simulator, self.owner], check=False)
        finally:
            if process and process.poll() is None:
                process.terminate(); process.wait(timeout=30)
            if installed and self.platform == 'iphonesimulator':
                self.run(['xcrun', 'simctl', 'uninstall', self.args.simulator, self.owner])
            elif installed:
                self.device('uninstall', 'app', '--device', self.args.physical, self.owner)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets-app', type=Path, required=True)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--device', choices=['mac', 'ipad', 'iphone'], required=True)
    parser.add_argument('--physical')
    parser.add_argument('--simulator')
    parser.add_argument('--signing-identity')
    parser.add_argument('--fixture-app', type=Path, help='Reuse an owned capture fixture with matching sources and recipe')
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--scenes', default='paint,sketch,photo')
    parser.add_argument('--languages', default='en')
    parser.add_argument('--themes', default='light,dark')
    args = parser.parse_args()
    for value in args.languages.split(',') + args.themes.split(',') + args.scenes.split(','):
        if not re.fullmatch(r'[a-zA-Z0-9-]+', value):
            parser.error('Expected comma-separated locale, scene and theme slugs')
    capture = Capture(args)
    capture.build()
    if not args.build_only:
        capture.capture()


if __name__ == '__main__':
    main()
