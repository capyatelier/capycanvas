#!/usr/bin/env python3
"""Run bench-chain-windows.ps1 passes on the Windows PCs over SSH in palindrome order.

The PC must be signed in and unlocked: each pass runs in that desktop session.
Pass directories land in ~/capy-bench/runs/<run_id>/<device>/<version>/pass-<n>/.

  tools/performance/bench_windows.py --devices surfacelaptop \\
    --exe 'v1.0.15=C:\\Users\\hogba\\builds\\v1.0.15\\CapyCanvas.exe' \\
    --exe 'g0123456789=C:\\Users\\hogba\\builds\\g0123456789\\CapyCanvas.exe' \\
    --photo PHOTO.jpg --heavy heavy.capy --vector vector.capy
"""
import argparse
import base64
import concurrent.futures
import datetime
import hashlib
import json
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from bench import pass_order, step_closure

REPO = Path(__file__).resolve().parents[2]
HOSTS = ('surfacelaptop', 'surfacepro', 'p43s')
HOME = 'C:\\Users\\hogba'
ROOT = 'AppData/Local/CapyCanvasAgent/bench'
SOURCES = (
    'tools/performance/bench-chain-windows.ps1', 'tools/performance/WindowsCanvasMotion.ps1', 'tools/performance/WindowsPenMotion.cs',
    'apps/layer-windows/scripts/CapyUia.ps1', 'apps/layer-windows/scripts/CapyEnvironment.ps1', 'apps/layer-windows/scripts/RowPointerDriver.cs',
    'apps/layer-windows/scripts/open-application-menu.ps1', 'apps/layer-windows/scripts/probe-displays.ps1')
PASS_MINUTES = 6


def palindrome(versions, passes, quick):
    return [(entry["version"], entry["pass"], entry["cold"]) for entry in pass_order(versions, 1 if quick else passes, quick)]


def remote(*parts):
    return '\\'.join((HOME, *ROOT.split('/'), *(part for path in parts for part in path.split('/'))))


def transfer(*parts):
    return '/'.join((ROOT, *parts))


def quote(text):
    return "'" + str(text).replace("'", "''") + "'"


def powershell_command(host, script):
    body = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';" + script
    return ['ssh', host, 'powershell', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
            '-EncodedCommand', base64.b64encode(body.encode('utf-16-le')).decode()]


def parse_executables(values):
    executables = {}
    for value in values:
        label, separator, path = value.partition('=')
        if not separator or not label or not path.lower().endswith('.exe') or label in executables:
            raise ValueError(f'--exe needs a unique LABEL=C:\\path\\CapyCanvas.exe, got {value!r}')
        executables[label] = path
    return executables


def invocation(run_id, device, label, executable, pass_number, cold, fixtures, steps):
    log = remote('runs', run_id, f'{label}-pass-{pass_number}')
    arguments = ' '.join([
        '-Executable', quote(executable), '-Chain', quote(remote('runs', run_id, 'chain.json')), '-Pass', str(pass_number),
        *(['-Cold'] if cold else []), '-Device', quote(device), '-Version', quote(label),
        '-Photo', quote(fixtures['photo']), '-Heavy', quote(fixtures['heavy']), '-Vector', quote(fixtures['vector']),
        '-OutputDirectory', quote(remote('versions', label)), *(['-Steps', quote(','.join(steps))] if steps else [])])
    return '\n'.join([
        "$ErrorActionPreference='Stop'", f'$log={quote(log)}', "Start-Transcript -LiteralPath ($log+'.log')|Out-Null", '$code=1',
        f"try{{& {quote(remote('runs', run_id, 'src/tools/performance/bench-chain-windows.ps1'))} {arguments};$code=0}}",
        'catch{Write-Host ($_|Out-String);Write-Host $_.ScriptStackTrace}',
        "finally{Stop-Transcript|Out-Null;[IO.File]::WriteAllText($log+'.exit',[string]$code)}", 'exit $code', ''])


def task_script(name, script, elevated):
    argument = f'"powershell.exe" -NoProfile -Sta -ExecutionPolicy Bypass -File "{script}"'
    return ';'.join([
        "if(!(Get-Process explorer -ErrorAction SilentlyContinue)){throw 'No signed-in Windows desktop: sign in on the PC first'}",
        "if(Get-Process LogonUI -ErrorAction SilentlyContinue){throw 'The Windows desktop is locked: unlock it on the PC first'}",
        f'$action=New-ScheduledTaskAction -Execute conhost.exe -Argument {quote(argument)}',
        f"$principal=New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel {'Highest' if elevated else 'Limited'}",
        '$settings=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 2)',
        f'Register-ScheduledTask -TaskName {quote(name)} -Action $action -Principal $principal -Settings $settings -Force|Out-Null',
        f'Start-ScheduledTask -TaskName {quote(name)}'])


def run(command, timeout=600):
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"{' '.join(command[:3])} failed ({result.returncode}): {(result.stderr or result.stdout).strip()[-2000:]}")
    return result.stdout


def powershell(host, script, timeout=600):
    return run(powershell_command(host, script), timeout)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as source:
        for block in iter(lambda: source.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def upload_fixture(host, path):
    digest = sha256(path)
    target = remote('fixtures', digest[:16], path.name)
    probe = f'if(Test-Path -LiteralPath {quote(target)}){{(Get-FileHash -LiteralPath {quote(target)}).Hash}}'
    if powershell(host, probe).strip().lower() != digest:
        powershell(host, f"New-Item -ItemType Directory -Force {quote(remote('fixtures', digest[:16]))}|Out-Null")
        run(['scp', '-q', str(path), f'{host}:{transfer("fixtures", digest[:16], path.name)}'], timeout=3600)
        if powershell(host, probe).strip().lower() != digest:
            raise RuntimeError(f'{host}: the copied fixture {path.name} does not match its local sha256')
    return target


def upload_run(host, run_id, chain, invocations):
    with tempfile.TemporaryDirectory(prefix='capy-bench-windows-') as folder:
        staging = Path(folder)
        for source in SOURCES:
            (staging / 'src' / source).parent.mkdir(parents=True, exist_ok=True)
            (staging / 'src' / source).write_bytes((REPO / source).read_bytes())
        (staging / 'chain.json').write_bytes(chain.read_bytes())
        for name, text in invocations.items():
            (staging / name).write_text(text, encoding='utf-8-sig')
        powershell(host, f"New-Item -ItemType Directory -Force {quote(remote('runs', run_id))}|Out-Null")
        run(['scp', '-q', '-r', *[str(path) for path in sorted(staging.iterdir())], f'{host}:{transfer("runs", run_id)}/'])


def wait_pass(host, task, log, timeout_minutes):
    status = (f'$task=Get-ScheduledTask -TaskName {quote(task)} -ErrorAction SilentlyContinue;$exit={quote(log + ".exit")};'
              "@{state=[string]$task.State;code=$(if(Test-Path -LiteralPath $exit){[IO.File]::ReadAllText($exit).Trim()})}|ConvertTo-Json -Compress")
    deadline, stopped = time.monotonic() + timeout_minutes * 60, 0
    while time.monotonic() < deadline:
        time.sleep(5)
        state = json.loads(powershell(host, status))
        if state['code'] is not None:
            return int(state['code'])
        stopped = stopped + 1 if state['state'] not in ('Running', 'Queued') else 0
        if stopped > 2:
            raise RuntimeError(f'{host}: scheduled task {task} stopped ({state["state"]}) without an exit file; see {log}.log on the PC')
    raise TimeoutError(f'{host}: pass did not finish within {timeout_minutes} minutes; see {log}.log')


def cleanup_script(task, executable, directory):
    pid = quote(directory + '\\process-id.txt')
    return ';'.join([
        f"if(Get-ScheduledTask -TaskName {quote(task)} -ErrorAction SilentlyContinue){{Stop-ScheduledTask -TaskName {quote(task)};Unregister-ScheduledTask -TaskName {quote(task)} -Confirm:$false}}",
        f"if(Test-Path -LiteralPath {pid}){{$app=Get-Process -Id ([int](Get-Content -LiteralPath {pid})) -ErrorAction SilentlyContinue;if($app -and $app.Path -eq {quote(executable)}){{Stop-Process -Id $app.Id -Force}}}}"])


def remove_script(*paths):
    return f"foreach($path in {','.join(quote(path) for path in paths)}){{if(Test-Path -LiteralPath $path){{Remove-Item -LiteralPath $path -Recurse -Force}}}}"


def run_device(args, device, executables, fixtures, record, lock, out):
    run_id, order, state = record['run_id'], palindrome(list(executables), args.passes, args.quick), record['devices'][device]
    missing = ';'.join(f'if(!(Test-Path -LiteralPath {quote(path)})){{throw "Missing executable for {label}: {path}"}}' for label, path in executables.items())
    powershell(device, missing)
    placed = {name: upload_fixture(device, Path(path).expanduser()) for name, path in fixtures.items()}
    upload_run(device, run_id, args.chain, {f'{label}-pass-{number}.ps1': invocation(run_id, device, label, executables[label], number, cold, placed, args.steps)
                                            for label, number, cold in order})
    task = f'CapyBench-{run_id}-{device}'
    for label, number, cold in order:
        directory, log = remote('versions', label, f'pass-{number}'), remote('runs', run_id, f'{label}-pass-{number}')
        local = out / device / label / f'pass-{number}'
        print(f'{device}: {label} pass {number}{" (cold)" if cold else ""}', flush=True)
        started, code, error = time.monotonic(), None, None
        powershell(device, remove_script(directory, log + '.exit'))
        powershell(device, task_script(task, log + '.ps1', args.elevated))
        try:
            code = wait_pass(device, task, log, args.timeout_minutes)
        except (RuntimeError, TimeoutError) as failure:
            error = str(failure)
        finally:
            powershell(device, cleanup_script(task, executables[label], directory))
        if powershell(device, f'Test-Path -LiteralPath {quote(directory)}').strip() == 'True':
            local.parent.mkdir(parents=True, exist_ok=True)
            run(['scp', '-q', '-r', f'{device}:{transfer("versions", label, f"pass-{number}")}', str(local.parent)], timeout=3600)
        local.mkdir(parents=True, exist_ok=True)
        run(['scp', '-q', f'{device}:{transfer("runs", run_id, f"{label}-pass-{number}.log")}', str(local / 'runner.log')])
        powershell(device, remove_script(directory, log + '.log', log + '.exit'))
        with lock:
            state['passes'].append({'version': label, 'pass': number, 'cold': cold, 'exit_code': code, 'error': error,
                                    'seconds': round(time.monotonic() - started, 1), 'directory': f'{device}/{label}/pass-{number}'})
            write_record(out, record)
        print(f'{device}: {label} pass {number} exited {code} after {time.monotonic() - started:.0f} s; log {local / "runner.log"}' + (f'; {error}' if error else ''), flush=True)
    powershell(device, remove_script(remote('runs', run_id)))


def write_record(out, record):
    (out / 'run.json').write_text(json.dumps(record, indent=2) + '\n')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--exe', action='append', required=True, help='LABEL=C:\\path\\CapyCanvas.exe on every selected PC; repeat per version')
    parser.add_argument('--devices', required=True, help=f'comma-separated SSH hosts from {", ".join(HOSTS)}')
    parser.add_argument('--photo', required=True)
    parser.add_argument('--heavy', required=True)
    parser.add_argument('--vector', required=True)
    parser.add_argument('--chain', type=Path, default=REPO / 'tools/performance/bench-chain.json')
    parser.add_argument('--passes', type=int, default=2)
    parser.add_argument('--quick', action='store_true', help='one warm pass per version')
    parser.add_argument('--steps', type=lambda text: [step for step in text.split(',') if step], default=[])
    parser.add_argument('--run-id', default=datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
    parser.add_argument('--elevated', action='store_true', help='run passes elevated so PresentMon can record presents')
    parser.add_argument('--timeout-minutes', type=int, default=45)
    args = parser.parse_args(argv)
    try:
        executables = parse_executables(args.exe)
    except ValueError as error:
        parser.error(str(error))
    fixtures = {'photo': args.photo, 'heavy': args.heavy, 'vector': args.vector}
    for path in [args.chain, *map(Path, fixtures.values())]:
        if not path.expanduser().is_file():
            parser.error(f'missing file: {path}')
    if args.steps:
        try:
            args.steps = step_closure(json.loads(args.chain.read_text())['steps'], args.steps)
        except ValueError as error:
            parser.error(str(error))
    devices = [device for device in args.devices.split(',') if device]
    if not devices or set(devices) - set(HOSTS):
        parser.error(f'--devices must name hosts from {", ".join(HOSTS)}')
    out = Path.home() / 'capy-bench' / 'runs' / args.run_id
    out.mkdir(parents=True, exist_ok=True)
    existing = json.loads((out / 'run.json').read_text()) if (out / 'run.json').exists() else {}
    order = palindrome(list(executables), args.passes, args.quick)
    record = {**existing, 'schema': 1, 'run_id': args.run_id,
              'versions': list(dict.fromkeys([*existing.get('versions', []), *executables])),
              'devices': {**existing.get('devices', {}), **{device: {
                  'host': 'windows', 'ssh': device, 'executables': executables, 'chain_sha256': sha256(args.chain), 'steps': args.steps,
                  'order': [{'version': label, 'pass': number, 'cold': cold} for label, number, cold in order], 'passes': [], 'error': None}
                  for device in devices}}}
    write_record(out, record)
    print(f'Run {args.run_id}: {len(order)} passes per device on {", ".join(devices)}, about {len(order) * PASS_MINUTES} min; output {out}', flush=True)
    started, lock = time.monotonic(), threading.Lock()
    with concurrent.futures.ThreadPoolExecutor(len(devices)) as pool:
        jobs = {pool.submit(run_device, args, device, executables, fixtures, record, lock, out): device for device in devices}
        for job in concurrent.futures.as_completed(jobs):
            if job.exception():
                with lock:
                    record['devices'][jobs[job]]['error'] = str(job.exception())
                    write_record(out, record)
                print(f'{jobs[job]}: {job.exception()}', file=sys.stderr, flush=True)
    failed = [device for device in devices if record['devices'][device]['error'] or any(p['exit_code'] != 0 for p in record['devices'][device]['passes'])]
    print(f'Finished in {(time.monotonic() - started) / 60:.1f} min; {out / "run.json"}' + (f'; failures on {", ".join(failed)}' if failed else ''), flush=True)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
