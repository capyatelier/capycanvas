#!/usr/bin/env python3
r"""Measure five-bubble Android motion using app-window actual presents.

Build/install the optimized benchmark APK and test APK under the worktree's
application ID first. Run this collector through devices.py run; it never builds
or installs. The fixture uses a prescribed tier photo at Fit and private test
storage. Finish installs and stop the app before enabling Adaptive on MovinkPad
11: installing or force-stopping clears its app-specific switch. The collector
checks that the app is already stopped and never force-stops it before launch.
Instrumentation startup also resets Adaptive. On that firmware, --adaptive-script
runs a retained exact-row helper after private fixture setup, then resumes the
existing private Activity and releases the benchmark-only ready/go barrier.
FrameMetrics and drawn-center rows are attribution, not presented FPS.
--workload palette-opening records three short transitions with production-active
window boundaries, drawn poses and actual presents; it never reports FPS.
--workload hdr-drag and hdr-intensity use an F16 tier drawing with the reference
photo, measuring the color bubble's native rim drag or constant-speed EV sweep.

  tools/devices/devices.py run movinkpad11 -- python3 tools/performance/android-bubbles.py \
    artifacts/bubbles/android-mid --photo /data/local/tmp/capy-tier-24mp.jpg --width 6000 --height 4000 \
    --target-hz 90 --adaptive-script artifacts/bubbles/android-mid-adaptive.py

  python3 tools/performance/android-bubbles.py artifacts/bubbles/android-mid --report-only
"""
import argparse
import json
import math
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time


def surface_names(text):
    return [re.sub(r"^RequestedLayerState\{(.*?)(?: parentId=.*)?\}$", r"\1", line) for line in text.splitlines()]


def latency(text):
    lines = text.strip().splitlines()
    period = int(lines[0])
    if period <= 0:
        raise ValueError("SurfaceFlinger refresh period is unavailable")
    times = set()
    for line in lines[1:]:
        fields = line.split()
        if len(fields) != 3:
            raise ValueError("Malformed SurfaceFlinger latency row")
        actual = int(fields[1])
        if 0 < actual < (1 << 63) - 1:
            times.add(actual)
    return period, sorted(times)


def summarize(times):
    times = sorted(set(times))
    gaps = sorted((b - a) / 1e6 for a, b in zip(times, times[1:]))
    def quantile(q):
        return gaps[math.ceil(len(gaps) * q) - 1] if gaps else None
    return dict(presents=len(times), presented_hz=(len(times) - 1) * 1e9 / (times[-1] - times[0]) if len(times) > 1 else None,
                gap_p50_ms=quantile(.5), gap_p95_ms=quantile(.95), gap_p99_ms=quantile(.99))


def report(directory, target_hz=None):
    fixture = json.loads((directory / "result.json").read_text())
    capture = json.loads((directory / "surface-latency.json").read_text())
    settings_stages = []
    for stage in ["before", "before-motion", "after"]:
        path = directory / f"display-settings-{stage}.json"
        if path.is_file():
            settings = json.loads(path.read_text())
            factory_display_settings(**{key: settings[key] for key in ["size", "density", "font_scale"]})
            settings_stages.append(stage)
    settings_verified = all(stage in settings_stages for stage in ["before", "after"])
    qualification = "Factory display settings verified; apply tier rate and p99 limits" if settings_verified else "Diagnostic: missing recorded factory display settings"
    parsed = [latency(sample["text"]) for sample in capture["samples"]]
    all_times = sorted({t for _, times in parsed for t in times})
    contacts = []
    for contact in fixture["contacts"]:
        if contact["warmup"]:
            continue
        if contact.get("reset_end_ns", contact["begin_ns"]) > contact["begin_ns"]:
            raise ValueError("Position reset overlaps the measured motion window")
        if contact.get("diagnostic"):
            windows = contact["moving_windows_ns"]
            transitions = []
            for begin, end in windows:
                presents = [t for t in all_times if begin <= t <= end]
                draws = [row for row in contact["drawn_centers"] if begin <= row[0] <= end]
                poses = [len({(row[index * 2 + 1], row[index * 2 + 2]) for row in draws}) for index in range(5)]
                transitions.append(dict(begin_ns=begin, end_ns=end, production_active_ms=(end - begin) / 1e6,
                                        actual_present_timestamps_ns=presents, drawn_rows=len(draws), distinct_drawn_centers=poses))
            result = dict(measurement="Diagnostic palette opening; production-active windows are not sustained motion; no FPS qualification",
                          qualification=qualification,
                          surface=capture["surface"], instrumentation_passed="OK (1 test)" in (directory / "instrumentation.log").read_text(),
                          display_hz=contact.get("display_hz"), thermal_before=contact.get("thermal_before"), thermal_after=contact.get("thermal_after"), transitions=transitions)
            (directory / "presents.json").write_text(json.dumps(result, indent=2) + "\n")
            return result
        if "moving_windows_ns" in contact:
            raise ValueError("Palette active-state windows do not qualify sustained-motion FPS; use the short diagnostic")
        begin, end = contact["begin_ns"] + 100_000_000, contact["end_ns"] - 100_000_000
        if end - begin < 5_000_000_000:
            raise ValueError("Motion window is shorter than five seconds")
        rollover = []
        for index, ((_, before), (_, after)) in enumerate(zip(parsed, parsed[1:])):
            if before and after and not set(before).intersection(after) and before[-1] < end and after[0] > begin:
                rollover.append(index)
        if rollover:
            raise ValueError(f"SurfaceFlinger ring rollover in contact {contact['run']}: {rollover}")
        times = [t for t in all_times if begin <= t <= end]
        if len(times) < 2:
            raise ValueError("Actual app-window presents are unavailable")
        if contact.get("workload") == "hdr-intensity":
            if contact.get("distinct_intensities", 0) <= 10:
                raise ValueError("The HDR gesture did not visibly change intensity")
        elif len(contact["distinct_centers"]) != 5 or min(contact["distinct_centers"][1:]) <= 1:
            raise ValueError("The gesture did not displace all four neighbors")
        contacts.append(dict(run=contact["run"], workload=contact.get("workload", "drag"), distinct_intensities=contact.get("distinct_intensities"), begin_ns=begin, end_ns=end,
                             distinct_centers=contact["distinct_centers"], reset_end_ns=contact.get("reset_end_ns"), display_hz=contact.get("display_hz"),
                             thermal_before=contact.get("thermal_before"), thermal_after=contact.get("thermal_after"), **summarize(times)))
    if len(contacts) < 3:
        raise ValueError("Three measured gestures are required")
    log = directory / "instrumentation.log"
    passed = "OK (1 test)" in log.read_text() if log.is_file() else None
    result = dict(measurement="SurfaceFlinger app-window actual presents; MONOTONIC clock", instrumentation_passed=passed,
                  qualification=qualification,
                  surface=capture["surface"], refresh_period_ns=sorted({period for period, _ in parsed}), contacts=contacts)
    if target_hz is not None:
        floor, gap = target_hz * .95, round(2000 / target_hz, 1)
        result["target"] = dict(hz=target_hz, minimum_presented_hz=floor, maximum_gap_p99_ms=gap)
        result["target_met"] = bool(settings_verified and passed and all(
            contact.get("display_hz", 0) >= floor
            and contact["presented_hz"] >= (floor if contact["display_hz"] <= target_hz * 1.05 else target_hz)
            and contact["gap_p99_ms"] <= gap for contact in contacts))
    (directory / "presents.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def activity_identity(text, package):
    component = package + "/art.capycanvas.MainActivity"
    lines = [line for line in text.splitlines() if component in line and ("topResumedActivity=" in line or "mResumedActivity:" in line)]
    matches = [re.search(r"ActivityRecord\{(\S+).*?\bt(\d+)\b", line) for line in lines]
    identities = {(match.group(1), int(match.group(2))) for match in matches if match}
    if len(identities) != 1:
        raise ValueError("Expected one resumed private MainActivity identity")
    return identities.pop()


def factory_display_settings(size, density, font_scale):
    def physical(text, label, value):
        match = re.fullmatch(rf"Physical {label}: ({value})\s*", text.strip())
        if not match:
            raise ValueError(f"Factory {label} required; overrides or malformed settings: {text.strip()!r}")
        return match.group(1)
    size_value = physical(size, "size", r"[1-9]\d*x[1-9]\d*")
    density_value = physical(density, "density", r"[1-9]\d*")
    try:
        scale = float(font_scale.strip())
    except ValueError as error:
        raise ValueError(f"Malformed font scale: {font_scale.strip()!r}") from error
    if scale != 1:
        raise ValueError(f"Factory font scale 1 required; got {font_scale.strip()!r}")
    return dict(size=[int(value) for value in size_value.split("x")], density=int(density_value), font_scale=scale)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("output", type=Path)
    parser.add_argument("--report-only", action="store_true")
    parser.add_argument("--adaptive-script", type=Path, help="Run this exact-row settings helper with 'enable' at the fixture's ready/go barrier")
    parser.add_argument("--package", default=os.environ.get("CAPY_APPLICATION_ID"))
    parser.add_argument("--photo")
    parser.add_argument("--width", type=int)
    parser.add_argument("--height", type=int)
    parser.add_argument("--duration-ms", type=int, default=6000)
    parser.add_argument("--target-hz", type=float, choices=[60, 90, 120], help="Require the reference refresh rate, actual motion rate and p99 gap limit")
    parser.add_argument("--palette-transitions", type=int, choices=range(1, 11), default=3, help="Short diagnostic only; do not calculate FPS")
    parser.add_argument("--workload", choices=["drag", "palette-opening", "hdr-drag", "hdr-intensity"], default="drag")
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    def finish_report():
        result = report(args.output, args.target_hz)
        print(json.dumps(result, indent=2))
        if result.get("target_met") is False:
            raise RuntimeError("Bubble motion misses the tier qualification; raw data and presents.json retained")
    if args.report_only:
        finish_report(); return
    if not os.environ.get("CAPY_ANDROID_SERIAL") or args.package != os.environ.get("CAPY_APPLICATION_ID"):
        parser.error("Run through devices.py run with the worktree's own application ID")
    if not args.photo or not args.width or not args.height or not 5200 <= args.duration_ms <= 10000 or args.repeats < 3:
        parser.error("A tier photo/extent and at least three 5.2–10 second gestures are required")
    adb = shutil.which("adb") or str(Path.home() / "Android/Sdk/platform-tools/adb")
    def command(*parts, **kwargs):
        return subprocess.run([adb, *parts], check=True, text=True, capture_output=True, **kwargs).stdout
    args.output.mkdir(parents=True, exist_ok=False)
    def record_display_settings(stage):
        settings = dict(size=command("shell", "wm", "size"), density=command("shell", "wm", "density"),
                        font_scale=command("shell", "settings", "get", "system", "font_scale"))
        path = args.output / f"display-settings-{stage}.json"
        path.write_text(json.dumps(settings, indent=2) + "\n")
        settings["factory"] = factory_display_settings(**settings)
        path.write_text(json.dumps(settings, indent=2) + "\n")
    record_display_settings("before")
    (args.output / "hardware-before.txt").write_text(command("shell", "dumpsys thermalservice; dumpsys display"))
    running = subprocess.run([adb, "shell", "pidof", args.package], text=True, capture_output=True)
    if running.stdout.strip():
        parser.error("Stop the owned app before enabling Adaptive; force-stop clears that setting")
    cmd = [adb, "shell", "am", "instrument", "-w", "-r", "-e", "class", "art.capycanvas.AndroidBubbleBenchmarkTest#fiveBubbleMotion",
           "-e", "workload", args.workload, "-e", "bubbleBenchmark", "true", "-e", "photo", args.photo, "-e", "width", str(args.width), "-e", "height", str(args.height),
           "-e", "durationMs", str(args.duration_ms), "-e", "repeats", str(args.repeats), f"{args.package}.test/androidx.test.runner.AndroidJUnitRunner"]
    if args.workload == "palette-opening":
        cmd[4:4] = ["-e", "paletteTransitions", str(args.palette_transitions)]
    if args.target_hz:
        cmd[4:4] = ["-e", "expectedRefreshHz", str(args.target_hz)]
    remote = f"/sdcard/Android/data/{args.package}/files/bubble-benchmark"
    command("shell", "rm", "-f", *(f"{remote}/{name}" for name in ["ready", "go", "result.json", "fixture.json"]))
    if args.adaptive_script:
        if not args.adaptive_script.is_file():
            parser.error("Adaptive helper does not exist")
        cmd[4:4] = ["-e", "waitForGo", "true"]
    prepared = not args.adaptive_script
    samples, surface = [], None
    with (args.output / "instrumentation.log").open("w") as log:
        process = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 900
        try:
            while process.poll() is None:
                if time.monotonic() > deadline:
                    raise TimeoutError("Bubble benchmark exceeded 15 minutes")
                if not prepared:
                    ready = subprocess.run([adb, "shell", "test", "-f", f"{remote}/ready"], capture_output=True)
                    if ready.returncode != 0:
                        time.sleep(.5)
                        continue
                    before = activity_identity(command("shell", "dumpsys", "activity", "activities"), args.package)
                    subprocess.run([sys.executable, str(args.adaptive_script), "enable"], check=True)
                    command("shell", "am", "task", "focus", str(before[1]))
                    resume_deadline = time.monotonic() + 10
                    while True:
                        try:
                            after = activity_identity(command("shell", "dumpsys", "activity", "activities"), args.package)
                        except ValueError:
                            after = None
                        if after == before:
                            break
                        if time.monotonic() >= resume_deadline:
                            raise ValueError(f"Private activity changed during policy setup: {before} -> {after}")
                        time.sleep(.1)
                    (args.output / "private-activity.json").write_text(json.dumps(dict(before=before, after=after)))
                    (args.output / "hardware-before-motion.txt").write_text(command("shell", "dumpsys thermalservice; dumpsys display"))
                    record_display_settings("before-motion")
                    command("shell", "touch", f"{remote}/go")
                    prepared = True
                if surface is None:
                    candidates = [name for name in surface_names(command("shell", "dumpsys", "SurfaceFlinger", "--list"))
                                  if name.startswith(args.package + "/") and "/art.capycanvas.MainActivity#" in name]
                    if len(candidates) == 1:
                        surface = candidates[0]
                    elif len(candidates) > 1:
                        raise ValueError(f"App-window surface is ambiguous: {candidates}")
                if surface:
                    started = time.monotonic_ns()
                    text = command("shell", "dumpsys", "SurfaceFlinger", "--latency", surface)
                    try:
                        latency(text)
                    except ValueError:
                        pass
                    else:
                        samples.append(dict(begin_host_ns=started, end_host_ns=time.monotonic_ns(), text=text))
                time.sleep(.5)
        finally:
            if process.poll() is None:
                command("shell", "am", "force-stop", args.package)
                process.wait(timeout=30)
            (args.output / "surface-latency.json").write_text(json.dumps(dict(surface=surface, samples=samples)))
            (args.output / "hardware-after.txt").write_text(command("shell", "dumpsys thermalservice; dumpsys display"))
            record_display_settings("after")
    if "OK (1 test)" not in (args.output / "instrumentation.log").read_text():
        raise RuntimeError("Instrumentation failed; see instrumentation.log; raw surface samples retained")
    for name in ["fixture.json", "result.json"]:
        command("pull", f"{remote}/{name}", str(args.output / name))
    finish_report()


if __name__ == "__main__":
    main()
