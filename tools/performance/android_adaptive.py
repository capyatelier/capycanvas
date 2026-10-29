from contextlib import contextmanager
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path


def activity_identity(text, package):
    lines = [line for line in text.splitlines() if 'topResumedActivity=' in line or 'mResumedActivity:' in line]
    records = [re.search(r'ActivityRecord\{(\S+).*?\s(\S+)/art\.capycanvas\.MainActivity\s+t(\d+)\b', line) for line in lines]
    if not records or any(m is None or m[2] != package for m in records):
        raise ValueError('Expected the private MainActivity in the foreground')
    identities = {(m[1], int(m[3])) for m in records}
    if len(identities) != 1:
        raise ValueError('Ambiguous private Activity identity')
    return identities.pop()


def validate_identity(adb, package, label, apk, aapt):
    serial = os.environ.get('CAPY_ANDROID_SERIAL')
    if not package or package in {'art.capycanvas', 'art.capycanvas.editor'} or package != os.environ.get('CAPY_APPLICATION_ID') or not serial:
        raise ValueError('Collector package must match the reserved private package')
    if adb[-2:] != ['-s', serial] or not label or not label.strip():
        raise ValueError('Collector serial and unique label must match the helper')
    badging = subprocess.check_output([str(aapt), 'dump', 'badging', str(apk)], text=True)
    if not re.search(r"^package: name='" + re.escape(package) + r"'", badging, re.M):
        raise ValueError('APK package mismatch')
    labels = re.findall(r"^application-label(?:-[^:]+)?:'([^']*)'$", badging, re.M)
    if not labels or set(labels) != {label}:
        raise ValueError('APK must have the unique private app label in every locale')
    paths = subprocess.check_output(adb + ['shell', 'pm', 'path', package], text=True).strip().splitlines()
    if len(paths) != 1 or not paths[0].startswith('package:/data/app/') or not paths[0].endswith('/base.apk'):
        raise ValueError('Expected one installed private APK')
    installed = subprocess.check_output(adb + ['shell', 'sha256sum', paths[0][8:]], text=True).split()[0]
    if installed != hashlib.sha256(Path(apk).read_bytes()).hexdigest():
        raise ValueError('Installed APK differs from verified collector APK')
    return serial


def validate_ready(text, package, token, pid):
    ready = json.loads(text)
    if ready != dict(package=package, token=token, pid=int(pid)):
        raise ValueError('Stale or mismatched READY identity')


def enable_adaptive(adb, package, script, output, label, apk, aapt, remote, token):
    validate_identity(adb, package, label, apk, aapt)
    def command(*args):
        return subprocess.check_output(adb + list(args), text=True)
    before = activity_identity(command('shell', 'dumpsys', 'activity', 'activities'), package)
    pid = command('shell', 'pidof', package).strip()
    if not pid.isdecimal():
        raise ValueError('Expected one private application process')
    validate_ready(command('shell', 'cat', remote + '/ready'), package, token, pid)
    subprocess.run([sys.executable, str(script), 'enable', '--serial', adb[-1], '--package', package,
                    '--label', label, '--apk', str(apk), '--aapt', str(aapt), '--output', str(output)], check=True)
    command('shell', 'am', 'task', 'focus', str(before[1]))
    deadline = time.monotonic() + 10
    while True:
        try:
            after = activity_identity(command('shell', 'dumpsys', 'activity', 'activities'), package)
        except ValueError:
            after = None
        if after == before:
            break
        if after is not None or time.monotonic() >= deadline:
            raise ValueError(f'Private activity changed during policy setup: {before} -> {after}')
        time.sleep(.1)
    if command('shell', 'pidof', package).strip() != pid:
        raise ValueError('Private process changed during policy setup')
    validate_ready(command('shell', 'cat', remote + '/ready'), package, token, pid)
    (output / 'private-activity.json').write_text(json.dumps(dict(before=before, after=after, pid=pid, token=token)))
    (output / 'hardware-before-motion.txt').write_text(command('shell', 'dumpsys thermalservice; dumpsys display'))


def restore_adaptive(adb, package, script, output, label, apk, aapt):
    if not (output / 'adaptive-initial.json').exists():
        return
    validate_identity(adb, package, label, apk, aapt)
    subprocess.run([sys.executable, str(script), 'restore', '--serial', adb[-1], '--package', package,
                    '--label', label, '--apk', str(apk), '--aapt', str(aapt), '--output', str(output)], check=True)


@contextmanager
def cleanup_steps(*steps):
    primary = cleanup_error = None
    try:
        yield
    except BaseException as error:
        primary = error
        raise
    finally:
        for step in steps:
            try:
                step()
            except Exception as error:
                if primary is None:
                    primary = cleanup_error = error
                else:
                    primary.add_note(f"Cleanup also failed: {error!r}")
        if cleanup_error is not None:
            raise cleanup_error
