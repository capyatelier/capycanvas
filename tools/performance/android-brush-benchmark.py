#!/usr/bin/env python3
"""Drive the opt-in release brush runner; preserve raw device results/traces.

Build/install the benchmark variant with application id art.capycanvas.brushbench
and push the 9504x6336 Sony photo to /data/local/tmp/capy-brush-photo.jpg first.
"""
import argparse
import json
import math
import pathlib
import subprocess
import time
from android_brush_metrics import completion_window

PRESETS = {
    1: "gpen", 2: "pencil", 3: "eraser", 4: "paintbrush", 5: "airbrush",
    6: "chalk", 7: "marker", 8: "spray", 9: "dual-texture", 15: "textured-flat",
    16: "dry-scumble", 17: "pastel-block", 18: "transparent-glaze",
    25: "pointy-pencil", 26: "shading-pencil", 27: "charcoal", 28: "rough-gpen",
    29: "calligraphy-pen", 30: "antique-pen", 31: "realistic-pen", 32: "wet-ink",
    33: "blotty-ink", 34: "brushed-ink",
}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("output", type=pathlib.Path)
    p.add_argument("--adb", default="adb")
    p.add_argument("--serial", required=True, help="Target adb device serial")
    p.add_argument("--package", default="art.capycanvas.brushbench")
    p.add_argument("--presets", default=",".join(map(str, PRESETS)))
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--duration", type=int, default=10000)
    p.add_argument("--size", type=int, default=1000)
    p.add_argument("--mode", default="constant")
    p.add_argument("--speed", type=float, default=1)
    p.add_argument("--prediction", choices=["true", "false"], default="true")
    tracing = p.add_mutually_exclusive_group()
    tracing.add_argument("--trace", action="store_true", help="Full CPU/GPU phase attribution")
    tracing.add_argument("--presentation-trace", action="store_true",
                         help="SurfaceFlinger/gfx only, without per-brush app trace scopes or GPU phase readbacks")
    p.add_argument("--profile", action="store_true", help="Also sample the isolated process with simpleperf")
    p.add_argument("--prefix", default="screen")
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    adb = [args.adb, "-s", args.serial]
    remote = f"/sdcard/Android/data/{args.package}/files/brush-benchmark"
    failures = []

    def run(*items, **kw):
        return subprocess.run(adb + list(items), check=True, **kw)

    for preset in map(int, args.presets.split(",")):
        label = f"{args.prefix}-{PRESETS[preset]}-{args.size}-{args.mode}"
        if (args.output / f"{label}-complete.json").exists():
            print(f"SKIP completed {label}", flush=True)
            continue
        print(f"START {label}", flush=True)
        # Delete only this runner's synchronization files, never app documents.
        run("shell", f"rm -f {remote}/{label}-ready {remote}/{label}-go")
        with (args.output / f"{label}-environment-before.txt").open("w") as out:
            run("shell", "cat /proc/meminfo; dumpsys thermalservice; dumpsys battery", stdout=out)
        cmd = adb + ["shell", "am", "instrument", "-w", "-r", "-e", "brushBenchmark", "true"]
        for key, value in dict(label=label, preset=preset, brushSize=args.size,
                               durationMs=args.duration, repeats=args.repeats, mode=args.mode,
                               speed=args.speed, prediction=args.prediction,
                               waitForTrace="true").items():
            cmd += ["-e", key, str(value)]
        cmd += [f"{args.package}/art.capycanvas.BrushBenchmarkInstrumentation"]
        trace = None
        profile = None
        with (args.output / f"{label}-instrumentation.txt").open("w") as log:
            process = subprocess.Popen(cmd, stdout=log, stderr=log)
            deadline = time.monotonic() + 240
            while process.poll() is None:
                check = subprocess.run(adb + ["shell", f"test -f {remote}/{label}-ready"], capture_output=True)
                if check.returncode == 0:
                    break
                if time.monotonic() > deadline:
                    raise RuntimeError(f"Runner setup timed out: {label}")
                time.sleep(1)
            if process.poll() is not None:
                run("pull", remote, str(args.output / "failed-setup"))
                raise RuntimeError(f"Runner exited during setup: {label}; inspect instrumentation log")
            if args.trace or args.presentation_trace:
                milliseconds = args.repeats * (args.duration + 3500) + 8000
                app_trace = (f'ftrace_events: "sched/sched_switch"\n'
                             f'ftrace_events: "sched/sched_waking"\n'
                             f'atrace_apps: "{args.package}"' if args.trace else "")
                config = f'''buffers {{ size_kb: 131072 fill_policy: RING_BUFFER }}
incremental_state_config {{ clear_period_ms: 1000 }}
duration_ms: {milliseconds}
data_sources {{ config {{ name: "linux.ftrace" ftrace_config {{
ftrace_events: "ftrace/print"
atrace_categories: "gfx"
{app_trace}
}} }} }}
data_sources {{ config {{ name: "linux.process_stats" process_stats_config {{ scan_all_processes_on_start: true }} }} }}
data_sources {{ config {{ name: "android.surfaceflinger.frametimeline" }} }}
'''
                (args.output / f"{label}.pbtxt").write_text(config)
                trace_log = (args.output / f"{label}-trace.log").open("w")
                trace = subprocess.Popen(adb + ["shell", "perfetto", "--txt", "-c", "-", "-o",
                    "/data/misc/perfetto-traces/capy-brush.perfetto-trace"], stdin=subprocess.PIPE,
                    stdout=trace_log, stderr=trace_log)
                trace.stdin.write(config.encode())
                trace.stdin.close()
                time.sleep(1)
            if args.profile:
                seconds = math.ceil(args.repeats * (args.duration / 1000 + 3.5) + 3)
                profile_log = (args.output / f"{label}-profile.log").open("w")
                profile = subprocess.Popen(adb + ["shell", "simpleperf", "record", "--app", args.package,
                    "-e", "cpu-clock", "-f", "199", "--call-graph", "dwarf,16384", "--duration",
                    str(seconds), "-o", "/data/local/tmp/capy-brush.perf.data"],
                    stdout=profile_log, stderr=profile_log)
                time.sleep(1)
            run("shell", f"touch {remote}/{label}-go")
            process.wait(timeout=args.repeats * (args.duration / 1000 + 15) + 120)
            if trace:
                trace.wait(timeout=60)
                trace_log.close()
                run("pull", "/data/misc/perfetto-traces/capy-brush.perfetto-trace", str(args.output / f"{label}.perfetto-trace"))
            if profile:
                if profile.wait(timeout=60):
                    raise RuntimeError(f"CPU profile failed: {label}")
                profile_log.close()
                run("pull", "/data/local/tmp/capy-brush.perf.data", str(args.output / f"{label}.perf.data"))
        log = (args.output / f"{label}-instrumentation.txt").read_text()
        if f"BRUSH_COMPLETE {label}" not in log or process.returncode != 0:
            # A native crash may leave no screenshot or completed stroke report.
            # Preserve its evidence and continue the remaining preset matrix.
            for suffix in ["-info.json"] + [f"-{i}.json" for i in range(args.repeats)]:
                subprocess.run(adb + ["pull", f"{remote}/{label}{suffix}",
                    str(args.output / f"{label}{suffix}")], capture_output=True)
            with (args.output / f"{label}-crash-logcat.txt").open("w") as out:
                run("logcat", "-d", "-v", "threadtime", stdout=out)
            with (args.output / f"{label}-exit-state.txt").open("w") as out:
                run("shell", f"dumpsys activity exit-info {args.package}; dumpsys thermalservice", stdout=out)
            (args.output / f"{label}-failed.json").write_text(json.dumps({
                "label": label, "preset": preset, "returncode": process.returncode,
                "reason": "Runner did not report BRUSH_COMPLETE; inspect instrumentation and crash logs"}, indent=2))
            print(f"FAILED {label}; crash evidence retained", flush=True)
            failures.append(label)
            continue
        # Pull reports individually; prior benchmark outputs remain on-device.
        for suffix in ["-info.json", ".png"] + (["-detail.png"] if args.mode == "visual" else []) + [f"-{i}.json" for i in range(args.repeats)]:
            run("pull", f"{remote}/{label}{suffix}", str(args.output / f"{label}{suffix}"), stdout=subprocess.DEVNULL)
        info_path = args.output / f"{label}-info.json"
        info = json.loads(info_path.read_text())
        info["host_trace_kind"] = "full" if args.trace else "presentation" if args.presentation_trace else "none"
        info_path.write_text(json.dumps(info, indent=2))
        with (args.output / f"{label}-environment-after.txt").open("w") as out:
            run("shell", "cat /proc/meminfo; dumpsys thermalservice; dumpsys battery", stdout=out)
        summary = []
        for i in range(args.repeats):
            report = json.loads((args.output / f"{label}-{i}.json").read_text())
            summary.append({"run": i, **completion_window(report)})
        (args.output / f"{label}-complete.json").write_text(json.dumps(summary, indent=2))
        print(f"DONE {label} completed/s={[round(r['completed_per_s'], 2) for r in summary]}", flush=True)
    if failures:
        raise SystemExit(f"Failed benchmarks: {', '.join(failures)}")


if __name__ == "__main__":
    main()
