#!/usr/bin/env python3
"""Run one Android device's bench session: bench.py starts it through tools/devices/devices.py run with RUN_DIR/DEVICE/session.json."""
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import time
import uuid
import bench_fixtures
from android_adaptive import cleanup_steps, enable_adaptive, restore_adaptive
from bench_builds import SCRATCH, sha256
import bench_perfetto

ROOT = Path(__file__).resolve().parents[2]
DEVICES_TOOL = ROOT / "tools/devices/devices.py"
_spec = importlib.util.spec_from_file_location("devices", DEVICES_TOOL)
devices = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(devices)

CANVAS = {"low": (4248, 2832), "mid": (6000, 4000), "top": (9504, 6336)}
PHOTO_MP = {"low": 12, "mid": 24, "top": 61}
DEVICES = {name: dict(tier=tier, refresh_hz=hz, photo=f"/data/local/tmp/capy-tier-{PHOTO_MP[tier]}mp.jpg", adaptive=name == "movinkpad11")
           for name, tier, hz in [("tcl", "low", 60), ("movinkpad11", "mid", 90), ("movinkpad14", "top", 120), ("sm_n970u1", "low", 60),
                                  ("huion", "low", 60), ("xppen", "low", 60), ("sm_t878u", "low", 60)]}
SOURCE_PHOTO = Path.home() / "Downloads/sony_a7r_v_29.jpg"
PHOTOS = Path.home() / "capy-bench/photos"
REMOTE_CHAIN = "/data/local/tmp/capy-bench-chain.json"
RUNNER = "art.capycanvas.BenchChainInstrumentation"
PASS_TIMEOUT_S = 1800
PASS_PROGRESS_TIMEOUT_S = 150
PASS_EXIT_TIMEOUT_S = 30
READY_TIMEOUT_S = 600


def guarantee_size(width, height):
    pixels = width * height
    return 1024 if pixels < 20_000_000 else 1536 if pixels < 40_000_000 else 2048


def tier_photo(tier):
    path = PHOTOS / f"capy-tier-{PHOTO_MP[tier]}mp.jpg"
    if not path.exists():
        from PIL import Image
        path.parent.mkdir(parents=True, exist_ok=True)
        partial = path.with_suffix(".part")
        with Image.open(SOURCE_PHOTO) as image:
            if image.size == CANVAS[tier]:
                shutil.copyfile(SOURCE_PHOTO, partial)
            else:
                image.resize(CANVAS[tier], Image.LANCZOS).save(partial, "JPEG", quality=95, icc_profile=image.info.get("icc_profile"))
        partial.replace(path)
    return path


def documents(chain, steps, spec):
    width, height = CANVAS[spec["tier"]]
    result = {}
    for step in chain["steps"]:
        if step["do"] != "open" or steps is not None and step["id"] not in steps:
            continue
        document = chain["documents"][step["document"]]
        if document["kind"] == "photo":
            result[step["document"]] = dict(local=str(tier_photo(spec["tier"])), remote=spec["photo"])
        else:
            local = bench_fixtures.ensure_fixtures(width, height)[document["fixture"]]
            result[step["document"]] = dict(local=str(local), remote=f"/data/local/tmp/capy-bench-{document['fixture']}-{width}x{height}.capy")
    return result


def plan(device, run_dir, chain_path, builds, order, steps, adaptive):
    spec = DEVICES[device]
    chain = json.loads(Path(chain_path).read_text())
    width, height = CANVAS[spec["tier"]]
    record = dict(run_id=run_dir.name, device=device, tier=spec["tier"], refresh_hz=spec["refresh_hz"], canvas=[width, height],
                  guarantee_size=guarantee_size(width, height), chain=str(chain_path), steps=steps, passes=order,
                  documents=documents(chain, steps, spec),
                  versions={build["label"]: build for build in builds}, adaptive=adaptive if spec["adaptive"] else None,
                  trace=str(SCRATCH / "traces" / run_dir.name / f"{device}.perfetto-trace"))
    directory = run_dir / device
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "session.json").write_text(json.dumps(record, indent=2) + "\n")
    return directory


class Adb:
    def __init__(self, serial):
        self.prefix = [devices.adb(), "-s", serial]

    def __call__(self, *args, check=True, **options):
        return subprocess.run(self.prefix + list(args), check=check, capture_output=True, text=True, **options).stdout


def remote_sha(adb, path):
    fields = adb("shell", "sha256sum", path, check=False).split()
    return fields[0] if fields and re.fullmatch(r"[0-9a-f]{64}", fields[0]) else None


def place(adb, local, remote):
    digest = sha256(local)
    if remote_sha(adb, remote) == digest:
        return "kept"
    pattern = f"{PurePosixPath(remote).parent}/*{PurePosixPath(remote).suffix}"
    match = next((fields[1] for fields in map(str.split, adb("shell", f"sha256sum {pattern}", check=False).splitlines())
                  if len(fields) == 2 and fields[0] == digest), None)
    if match:
        adb("shell", "cp", match, remote)
        return "copied"
    adb("push", str(local), remote)
    return "pushed"


def install(adb, app, apk, digest):
    paths = [line[len("package:"):] for line in adb("shell", "pm", "path", app, check=False).splitlines() if line.startswith("package:")]
    if len(paths) == 1 and remote_sha(adb, paths[0]) == digest:
        return False
    adb("install", "-r", "--no-incremental", str(apk))
    return True


def display_state(adb):
    return dict(wm_size=adb("shell", "wm", "size").strip(), wm_density=adb("shell", "wm", "density").strip(),
                font_scale=adb("shell", "settings", "get", "system", "font_scale").strip())


def display_commands(state):
    factory, restore = [], []
    for name, pattern in [("size", r"Override size: (\S+)"), ("density", r"Override density: (\d+)")]:
        if match := re.search(pattern, state[f"wm_{name}"]):
            factory.append(["wm", name, "reset"])
            restore.append(["wm", name, match[1]])
    if state["font_scale"] not in ("null", "") and float(state["font_scale"]) != 1:
        factory.append(["settings", "put", "system", "font_scale", "1.0"])
        restore.append(["settings", "put", "system", "font_scale", state["font_scale"]])
    return factory, restore


def thermal_status(adb):
    match = re.search(r"Thermal Status: (\d+)", adb("shell", "dumpsys", "thermalservice"))
    return int(match[1]) if match else None


def instrument_args(record, entry, token=None):
    version = record["versions"][entry["version"]]
    extras = {"chain": REMOTE_CHAIN, "pass": entry["pass"], "cold": str(entry["cold"]).lower(), "device": record["device"],
              "version": entry["version"], "commit": version["commit"], **{name: document["remote"] for name, document in record["documents"].items()}}
    if record["steps"] is not None:
        extras["steps"] = ",".join(record["steps"])
    if token:
        extras.update(waitForGo="true", benchmarkReadyToken=token, expectedRefreshHz=record["refresh_hz"])
    return ["shell", "am", "instrument", "-w", *[part for key, value in extras.items() for part in ("-e", key, str(value))],
            f"{version['app_id']}/{RUNNER}"]


def wait_for_progress(process, log):
    started = last_progress = time.monotonic()
    size = log.stat().st_size
    while True:
        try:
            process.wait(timeout=1)
            return
        except subprocess.TimeoutExpired:
            now = time.monotonic()
            current_size = log.stat().st_size
            if current_size != size:
                size, last_progress = current_size, now
            if now - last_progress >= PASS_PROGRESS_TIMEOUT_S:
                raise RuntimeError(f"No instrumentation progress for {PASS_PROGRESS_TIMEOUT_S} s") from None
            if now - started >= PASS_TIMEOUT_S:
                raise RuntimeError(f"Instrumentation exceeded the {PASS_TIMEOUT_S} s pass limit") from None


def instrumentation_error(log, missing):
    codes = re.findall(r"^INSTRUMENTATION_CODE: (-?\d+)$", log, re.M)
    if codes and codes[-1] == "-1":
        return f"Instrumentation ended before {missing}" if missing else None
    messages = re.findall(r"^INSTRUMENTATION_RESULT: shortMsg=(.+)$", log, re.M)
    if messages:
        return messages[-1]
    return f"Instrumentation result code {codes[-1]}" if codes else "Instrumentation did not report a result code"


def run_pass(adb, record, entry, directory):
    version = record["versions"][entry["version"]]
    app, n = version["app_id"], entry["pass"]
    output = directory / entry["version"] / f"pass-{n}"
    output.mkdir(parents=True, exist_ok=True)
    remote = f"/sdcard/Android/data/{app}/files/bench-chain"
    adb("shell", "rm", "-rf", f"{remote}/pass-{n}", f"{remote}/ready", f"{remote}/go")
    adaptive = record["adaptive"]
    token = uuid.uuid4().hex if adaptive else None
    result = dict(entry, thermal_before=thermal_status(adb))
    started = time.monotonic()
    print(f"pass {n} of {entry['version']}{' (cold)' if entry['cold'] else ''}", flush=True)
    log_path = output / "instrument.log"
    try:
        with log_path.open("w") as log:
            process = subprocess.Popen(adb.prefix + instrument_args(record, entry, token), stdout=log, stderr=subprocess.STDOUT)
            with cleanup_steps(
                lambda: adb("shell", "am", "force-stop", app) if process.poll() is None else None,
                lambda: process.wait(timeout=PASS_EXIT_TIMEOUT_S),
                lambda: restore_adaptive(adb.prefix, app, adaptive["script"], output, version["app_label"], version["apk"], adaptive["aapt"]) if adaptive else None,
            ):
                if adaptive:
                    deadline = time.monotonic() + READY_TIMEOUT_S
                    while adb("shell", f"test -f {remote}/ready && echo ready", check=False).strip() != "ready":
                        if process.poll() is not None or time.monotonic() > deadline:
                            raise RuntimeError(f"{app} pass {n} never became ready for Adaptive; see {output / 'instrument.log'}")
                        time.sleep(.5)
                    enable_adaptive(adb.prefix, app, adaptive["script"], output, version["app_label"], version["apk"], adaptive["aapt"], remote, token)
                    adb("shell", "touch", f"{remote}/go")
                wait_for_progress(process, log_path)
    except RuntimeError as error:
        result["error"] = "\n".join([str(error), *getattr(error, "__notes__", [])])
    log = log_path.read_text(errors="replace")
    pulled = subprocess.run(adb.prefix + ["pull", f"{remote}/pass-{n}/pass.json", str(output / "pass.json")], capture_output=True).returncode == 0
    completed = set(re.findall(r"^BENCH_STEP (\S+) ", log, re.M))
    planned = record["steps"]
    if planned is None:
        planned = [step["id"] for step in json.loads(Path(record["chain"]).read_text())["steps"]]
    blocked = next((step for step in planned if step not in completed), None)
    error = result.get("error") or instrumentation_error(log, blocked)
    if error is None and (not pulled or process.returncode):
        error = f"adb exited {process.returncode}" if process.returncode else "Instrumentation did not save pass.json"
    if error:
        result.update(error=error, blocked_step=blocked)
        print(f"pass {n} of {entry['version']} stopped at {blocked}: {error}", flush=True)
    result.update(returncode=process.returncode, ok=error is None, seconds=round(time.monotonic() - started))
    if not result["ok"]:
        (output / "logcat.txt").write_text(adb("logcat", "-d", "-v", "threadtime", check=False))
    return result


def session(directory, adb):
    path = directory / "session.json"
    record = json.loads(path.read_text())

    def save():
        path.write_text(json.dumps(record, indent=2) + "\n")

    if "isKeyguardShowing=true" in adb("shell", "dumpsys", "window"):
        raise SystemExit(f"{record['device']} is locked; unlock it and run again")
    record["display"] = dict(before=display_state(adb))
    factory, restore = display_commands(record["display"]["before"])
    trace = Path(record["trace"])
    trace.parent.mkdir(parents=True, exist_ok=True)
    remote_trace = f"/data/misc/perfetto-traces/capy-bench-{record['run_id']}.perfetto-trace"
    key = f"capy-bench-{record['run_id']}"

    def finish_trace():
        adb("shell", "perfetto", f"--attach={key}", "--stop", check=False)
        adb("pull", remote_trace, str(trace))
        adb("shell", "rm", "-f", remote_trace)
        (directory / "trace.perfetto-trace").unlink(missing_ok=True)
        (directory / "trace.perfetto-trace").symlink_to(trace)

    def restore_display():
        for command in restore:
            adb("shell", *command)
        record["display"]["after"] = display_state(adb)

    with cleanup_steps(restore_display, save):
        for command in factory:
            adb("shell", *command)
        record["display"]["measured"] = display_state(adb)
        installed = [label for label, version in record["versions"].items() if install(adb, version["app_id"], version["apk"], version["sha256"])]
        if installed:
            time.sleep(5)
        record["placed"] = {remote: place(adb, local, remote) for local, remote in
                            [(record["chain"], REMOTE_CHAIN)] + [(d["local"], d["remote"]) for d in record["documents"].values()]}
        record["installed"] = installed
        save()
        pass_limit = PASS_TIMEOUT_S + PASS_EXIT_TIMEOUT_S + (READY_TIMEOUT_S if record["adaptive"] else 0)
        trace_ms = len(record["passes"]) * pass_limit * 1000
        adb("shell", "perfetto", "--txt", "-c", "-", "-o", remote_trace, f"--detach={key}",
            input=bench_perfetto.trace_config(trace_ms))
        with cleanup_steps(finish_trace):
            for index, entry in enumerate(record["passes"]):
                try:
                    record["passes"][index] = run_pass(adb, record, entry, directory)
                except (RuntimeError, ValueError, subprocess.SubprocessError) as error:
                    print(f"pass {entry['pass']} of {entry['version']} failed: {error}", flush=True)
                    record["passes"][index] = dict(entry, ok=False, error=str(error))
                save()
    return 0 if all(entry.get("ok") for entry in record["passes"]) else 1


def run_device(directory, processor):
    device = directory.name
    command = [sys.executable, str(DEVICES_TOOL), "run", device, "--", sys.executable, str(Path(__file__).resolve()), str(directory)]
    with (directory / "session.log").open("w") as log:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        for line in process.stdout:
            log.write(line)
            print(f"{device}: {line}", end="", flush=True)
    code = process.wait()
    if (directory / "trace.perfetto-trace").exists():
        try:
            bench_perfetto.convert(directory, processor)
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            print(f"{device}: trace conversion failed: {error}", flush=True)
            code = code or 1
    return code


def run_devices(directories, processor):
    with ThreadPoolExecutor(len(directories)) as pool:
        return dict(zip((directory.name for directory in directories), pool.map(lambda directory: run_device(directory, processor), directories)))


def main():
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: devices.py run DEVICE -- {Path(__file__).name} RUN_DIR/DEVICE")
    serial = os.environ.get("CAPY_ANDROID_SERIAL") or sys.exit("Run through tools/devices/devices.py run")
    sys.exit(session(Path(sys.argv[1]), Adb(serial)))


if __name__ == "__main__":
    main()
