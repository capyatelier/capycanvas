#!/usr/bin/env python3
"""Run AndroidViewportBenchmarkTest under a SurfaceFlinger trace and report
presented canvas frames, presentation gaps and the viewport pass's GPU time.

Install the benchmark APK and its benchmark test APK under your own
application ID first. Presents join the canvas SurfaceView's buffer numbers to
SurfaceFlinger's actual display-frame times, so no clock conversion is needed.
"""
import argparse
import csv
import io
import json
import math
import pathlib
import shutil
import subprocess
import time


def quantile(values, q):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * q) - 1)] if values else None


def presents(processor, trace, package, workspace=False):
    match = f"s.name GLOB 'latchBuffer *{package}/art.capycanvas.MainActivity*' AND s.name NOT GLOB '*SurfaceView*'" if workspace else f"s.name GLOB 'latchBuffer *SurfaceView*{package}/*'"
    query = f"""SELECT s.name latch, f.ts + f.dur presented_at
FROM slice s JOIN ancestor_slice(s.id) a ON a.name GLOB 'commit [0-9]*'
JOIN actual_frame_timeline_slice f ON f.layer_name IS NULL AND CAST(f.name AS INT) = CAST(SUBSTR(a.name, 8) AS INT)
WHERE {match} ORDER BY presented_at"""
    out = subprocess.run([processor, "query", str(trace), query], capture_output=True, text=True, check=True).stdout
    rows = list(csv.DictReader(io.StringIO(out[out.index('"latch"'):]))) if '"latch"' in out else []
    return {int(r["latch"].rsplit(" ", 1)[1]): int(r["presented_at"]) for r in rows}


def summarize(run, shown):
    begin, end = run["begin_ns"], run["end_ns"]
    frames = sorted(row[0] for row in run["completions"] if begin <= row[1] <= end)
    times = sorted({shown[f] for f in frames if f in shown})
    gaps = [(b - a) / 1e6 for a, b in zip(times, times[1:])]
    gpu = [sample[1] / 1e6 for batch in run["presentation"] for sample in batch if sample[2] == 1]
    result = dict(presented_per_s=(len(times) - 1) * 1e9 / (times[-1] - times[0]) if len(times) > 1 else None,
                presented_gap_p99_ms=quantile(gaps, .99), submitted_per_s=len(frames) / ((end - begin) / 1e9),
                viewport_gpu_p50_ms=quantile(gpu, .5), viewport_gpu_p95_ms=quantile(gpu, .95))
    boot = workspace_boot_window(run) if any(key in run for key in ["begin_mono_after_boot_ns", "end_mono_after_boot_ns"]) else None
    if boot is not None:
        boot_begin, boot_end = boot
        contact = sorted({t for t in shown.values() if boot_begin <= t <= boot_end})
        moving = [t for t in contact if boot_begin + 100_000_000 <= t <= boot_end - 100_000_000]
        if len(moving) < 2:
            raise ValueError("Actual canvas presentations unavailable inside the gesture")
        result.update(attributed_presented_per_s=result["presented_per_s"],
                      presented_per_s=(len(moving) - 1) * 1e9 / (moving[-1] - moving[0]),
                      presented_gap_p99_ms=quantile([(b - a) / 1e6 for a, b in zip(moving, moving[1:])], .99),
                      actual_present_timestamps_ns=moving, full_contact_presents=len(contact),
                      full_contact_ms=(boot_end - boot_begin) / 1e6,
                      onset_ms=(contact[0] - boot_begin) / 1e6 if contact else None,
                      full_contact_longest_gap_ms=max([(b - a) / 1e6 for a, b in zip(contact, contact[1:])], default=None),
                      window_source="explicit boot timestamps")
    return result


def workspace_boot_window(run):
    fields = ["begin_boot_ns", "end_boot_ns", "begin_ns", "end_ns", "begin_mono_after_boot_ns", "end_mono_after_boot_ns"]
    if not any(field in run for field in fields[:2]):
        return None
    if any(type(run.get(field)) is not int for field in fields):
        raise ValueError("Workspace boot/monotonic clock samples must all be integer timestamps")
    begin, end = run["begin_boot_ns"], run["end_boot_ns"]
    mono_begin, mono_end = run["begin_ns"], run["end_ns"]
    begin_after, end_after = run["begin_mono_after_boot_ns"], run["end_mono_after_boot_ns"]
    if end <= begin or mono_end <= mono_begin or begin_after < mono_begin or end_after < mono_end:
        raise ValueError("Workspace clock bounds must be ordered")
    lower = max(begin - begin_after, end - end_after)
    upper = min(begin - mono_begin, end - mono_end)
    if lower > upper:
        raise ValueError("Workspace boot/monotonic clock offset changed outside the sampled read brackets")
    return begin, end


def trace_config(package, workspace=False, profile=False):
    config = '''buffers { size_kb: 65536 fill_policy: RING_BUFFER }
duration_ms: 600000
write_into_file: true
file_write_period_ms: 2000
data_sources { config { name: "linux.ftrace" ftrace_config { ftrace_events: "ftrace/print" atrace_categories: "gfx" } } }
data_sources { config { name: "android.surfaceflinger.frametimeline" } }
'''
    if workspace and profile:
        config = config.replace('atrace_categories: "gfx"', f'atrace_categories: "gfx" atrace_apps: "{package}"')
    return config


def workspace_summary(processor, trace, run, shown, profile=False):
    name = run["trace_name"].replace("'", "''")
    boot = workspace_boot_window(run)
    if boot is not None and not profile:
        begin, end = boot
        window_source = "explicit boot timestamps"
    else:
        query = f"SELECT ts, dur FROM slice WHERE name = '{name}' AND dur > 0"
        out = subprocess.run([processor, "query", str(trace), query], capture_output=True, text=True, check=True).stdout
        rows = list(csv.DictReader(io.StringIO(out[out.index('"ts"'):]))) if '"ts"' in out else []
        if len(rows) != 1:
            raise ValueError(f"Expected one native motion trace window: {name}: {rows}")
        begin, duration = int(rows[0]["ts"]), int(rows[0]["dur"])
        end = begin + duration
        window_source = "profile async trace" if profile else "legacy async trace"
    duration = end - begin
    contact = sorted({t for t in shown.values() if begin <= t <= end})
    moving = [t for t in contact if begin + 100_000_000 <= t <= end - 100_000_000]
    if duration < 5_000_000_000 or len(moving) < 2:
        raise ValueError(f"Actual window presentations unavailable inside a sustained gesture: {name}")
    gaps = [(b - a) / 1e6 for a, b in zip(moving, moving[1:])]
    return dict(run=run["run"], measurement="SurfaceFlinger app-window latches joined to actual display-frame presentations", window_source=window_source,
                presented_per_s=(len(moving) - 1) * 1e9 / (moving[-1] - moving[0]),
                presented_gap_p99_ms=quantile(gaps, .99), actual_present_timestamps_ns=moving,
                full_contact_presents=len(contact), full_contact_ms=duration / 1e6,
                onset_ms=(contact[0] - begin) / 1e6 if contact else None,
                full_contact_longest_gap_ms=max([(b - a) / 1e6 for a, b in zip(contact, contact[1:])], default=None),
                thermal_before=run["thermal_before"], thermal_after=run["thermal_after"],
                frame_metrics_proxy_hz=run["frame_rate"], frame_metrics_proxy_gap_p99_ms=run["vsync_interval_p99_ms"])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("output", type=pathlib.Path)
    p.add_argument("--serial", required=True)
    p.add_argument("--package", required=True)
    p.add_argument("--processor", required=True, help="Perfetto trace_processor_shell")
    p.add_argument("--photo", required=True)
    p.add_argument("--width", type=int, required=True)
    p.add_argument("--height", type=int, required=True)
    p.add_argument("--motions", default="pan,pinch,rotate")
    p.add_argument("--label", required=True)
    p.add_argument("--duration-ms", type=int, default=5000, help="Gesture length; slow tiers need more than 100 GPU samples per run")
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--extra", default="", help="Further instrumentation arguments as key=value,key=value")
    p.add_argument("--workspace-workload", choices=["gap", "panel"])
    p.add_argument("--workspace-runner", choices=["junit", "self"], default="junit", help="Self runs inside the benchmark APK and supports R8 without a JUnit APK")
    p.add_argument("--viewport-runner", choices=["junit", "self"], default="junit", help="Self reuses the viewport helper inside an optimized benchmark APK")
    p.add_argument("--default-canvas", action="store_true", help="Navigate the actual startup canvas and paper, without creating or importing artwork")
    p.add_argument("--theme", choices=["light", "dark"], default="light")
    p.add_argument("--profile", action="store_true", help="Workspace diagnostic: Compose trace and 199 Hz app-only CPU stacks during the first measured contact; not FPS qualification")
    args = p.parse_args()
    if args.workspace_runner == "self" and not args.workspace_workload:
        p.error("--workspace-runner self requires --workspace-workload")
    if args.default_canvas and args.workspace_workload:
        p.error("--default-canvas is only for viewport navigation")
    args.output.mkdir(parents=True, exist_ok=True)
    adb = [shutil.which("adb") or str(pathlib.Path.home() / "Android/Sdk/platform-tools/adb"), "-s", args.serial]
    remote = f"/sdcard/Android/data/{args.package}/files/" + ("workspace-bridge-benchmark" if args.workspace_workload else "viewport-benchmark")
    config = trace_config(args.package, bool(args.workspace_workload), args.profile)
    summary = {}
    for motion in [args.workspace_workload] if args.workspace_workload else args.motions.split(","):
        label = f"{args.label}-{motion}"
        cmd = adb + ["shell", "am", "instrument", "-w", "-e", "class", "art.capycanvas.AndroidViewportBenchmarkTest#retainedViewport",
                     "-e", "viewportBenchmark", "true", "-e", "motion", motion, "-e", "photo", args.photo,
                     "-e", "width", str(args.width), "-e", "height", str(args.height), "-e", "durationMs", str(args.duration_ms),
                     "-e", "repeats", str(args.repeats), "-e", "label", label]
        if not args.workspace_workload and args.viewport_runner == "self":
            index = cmd.index("class") - 1
            del cmd[index:index + 3]
        if args.default_canvas:
            for key in ["photo", "width", "height"]:
                index = cmd.index(key) - 1
                del cmd[index:index + 3]
            cmd += ["-e", "defaultCanvas", "true"]
        if args.workspace_workload:
            method = "columnBridgeGapFrameTiming" if motion == "gap" else "columnBridgePanelFrameTiming"
            selector = [] if args.workspace_runner == "self" else ["-e", "class", f"art.capycanvas.AndroidWorkspacePerformanceTest#{method}"]
            cmd = adb + ["shell", "am", "instrument", "-w"] + selector + [
                         "-e", "workspaceBenchmark", "true", "-e", "theme", args.theme, "-e", "photo", args.photo,
                         "-e", "width", str(args.width), "-e", "height", str(args.height), "-e", "durationMs", str(args.duration_ms),
                         "-e", "repeats", str(args.repeats), "-e", "label", label]
            if args.profile:
                cmd += ["-e", "composeTrace", "true", "-e", "profileBarrier", "true"]
            if args.workspace_runner == "self":
                cmd += ["-e", "workspaceWorkload", motion]
        if max(args.width, args.height) > 8192:
            cmd += ["-e", "openQueryPhoto", "true"]
        for item in filter(None, args.extra.split(",")):
            key, value = item.split("=", 1)
            cmd += ["-e", key, value]
        cmd += [f"{args.package}/art.capycanvas.UiStartupInstrumentation" if (args.workspace_workload and args.workspace_runner == "self") or (not args.workspace_workload and args.viewport_runner == "self")
                else f"{args.package}.test/androidx.test.runner.AndroidJUnitRunner"]
        trace = f"/data/misc/perfetto-traces/{args.package}-{label}.perfetto-trace"
        session = f"{args.package}-{label}"
        if args.profile:
            subprocess.run(adb + ["shell", "rm", "-f", f"{remote}/{label}-profile-ready", f"{remote}/{label}-profile-go"], check=True, capture_output=True)
        subprocess.run(adb + ["shell", "perfetto", "--txt", "-c", "-", "-o", trace, f"--detach={session}"],
                       input=config, text=True, check=True, capture_output=True)
        try:
            time.sleep(1)
            print(f"Running {label}; instrumentation log streams to {args.output}", flush=True)
            with (args.output / f"{label}-instrument.log").open("w") as stream:
                if not args.profile:
                    subprocess.run(cmd, stdout=stream, stderr=subprocess.STDOUT, text=True, timeout=900)
                else:
                    process = subprocess.Popen(cmd, stdout=stream, stderr=subprocess.STDOUT, text=True)
                    profile = None
                    perf = f"/data/local/tmp/{args.package}-{label}.perf.data"
                    try:
                        deadline = time.monotonic() + 600
                        while process.poll() is None:
                            if time.monotonic() > deadline:
                                raise TimeoutError("Workspace profile readiness exceeded ten minutes")
                            ready = subprocess.run(adb + ["shell", "test", "-f", f"{remote}/{label}-profile-ready"], capture_output=True)
                            if ready.returncode == 0:
                                with (args.output / "simpleperf.log").open("w") as profile_log:
                                    profile = subprocess.Popen(adb + ["shell", "simpleperf", "record", "--app", args.package,
                                        "-e", "cpu-clock", "-f", "199", "--call-graph", "dwarf,16384", "--duration", "8", "-o", perf], stdout=profile_log, stderr=subprocess.STDOUT)
                                    time.sleep(1)
                                    if profile.poll() is not None and profile.returncode:
                                        raise RuntimeError("App-only simpleperf refused sampling; see simpleperf.log")
                                    subprocess.run(adb + ["shell", "touch", f"{remote}/{label}-profile-go"], check=True, capture_output=True)
                                    profile.wait(timeout=15)
                                if profile.returncode:
                                    raise RuntimeError("App-only simpleperf failed; see simpleperf.log")
                                subprocess.run(adb + ["pull", perf, str(args.output / "perf.data")], check=True, capture_output=True)
                                subprocess.run(adb + ["shell", "rm", "-f", perf], check=True, capture_output=True)
                                break
                            time.sleep(.1)
                        process.wait(timeout=900)
                    finally:
                        if process.poll() is None:
                            subprocess.run(adb + ["shell", "am", "force-stop", args.package], capture_output=True)
                            process.wait(timeout=30)
                        if profile is not None and profile.poll() is None:
                            profile.wait(timeout=15)
            log = (args.output / f"{label}-instrument.log").read_text()
        finally:
            subprocess.run(adb + ["shell", "perfetto", f"--attach={session}", "--stop"], capture_output=True)
        (args.output / f"{label}-instrument.log").write_text(log)
        local = args.output / f"{label}.perfetto-trace"
        subprocess.run(adb + ["pull", trace, str(local)], check=True, capture_output=True)
        subprocess.run(adb + ["shell", "rm", "-f", trace], capture_output=True)
        complete = f"WORKSPACE_COMPLETE {label}" if args.workspace_workload and args.workspace_runner == "self" else f"VIEWPORT_COMPLETE {label}" if not args.workspace_workload and args.viewport_runner == "self" else "OK (1 test)"
        if complete not in log:
            summary[motion] = {"error": log[-2000:]}
            continue
        for suffix in ([] if args.workspace_workload else ["info"]) + [str(i) for i in range(args.repeats)]:
            subprocess.run(adb + ["pull", f"{remote}/{label}-{suffix}.json", str(args.output)], check=True, capture_output=True)
        shown = presents(args.processor, local, args.package, bool(args.workspace_workload))
        if args.workspace_workload:
            captures = args.output / "captures"
            captures.mkdir(exist_ok=True)
            if motion == "gap":
                keys = f"{label}-keyframes.json"
                subprocess.run(adb + ["pull", f"{remote}/{keys}", str(captures / keys)], check=True, capture_output=True)
                names = [row["capture"] for row in json.loads((captures / keys).read_text())]
            else:
                names = [f"{label}-panel.png"]
            for name in names:
                subprocess.run(adb + ["pull", f"{remote}/{name}", str(captures / name)], check=True, capture_output=True)
            runs = [workspace_summary(args.processor, local, json.loads((args.output / f"{label}-{i}.json").read_text()), shown, args.profile) for i in range(args.repeats)]
            summary[motion] = {"theme": args.theme, "runs": runs}
            print(motion, json.dumps(summary[motion]), flush=True)
            continue
        info = json.loads((args.output / f"{label}-info.json").read_text())
        runs = [summarize(json.loads((args.output / f"{label}-{i}.json").read_text()), shown) for i in range(args.repeats)]
        summary[motion] = {"zoom": info["state"]["camera"]["zoom"], "work_area": info["state"]["camera"]["work_area"],
                           "glass_regions": info["display"].get("glass_regions"), "thermal_status": info.get("thermal_status"),
                           "runs": runs}
        print(motion, json.dumps(summary[motion]), flush=True)
    (args.output / f"{args.label}-summary.json").write_text(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
