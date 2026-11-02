#!/usr/bin/env python3
"""Drive the opt-in release brush runner; preserve raw device results/traces.

Build/install the benchmark variant with application id art.capycanvas.brushbench
and push the 9504x6336 Sony photo to /data/local/tmp/capy-brush-photo.jpg first.
--photo selects another JPEG under /data/local/tmp; the canvas takes its size.
Liquify presets are 12,13,36,37,38,39,63,64. Reconstruct (64) requires the
runner's settled Push seed; setup validation rejects missing seed evidence.
"""
import argparse
import json
import math
import pathlib
import shlex
import subprocess
import time
import uuid
from android_adaptive import cleanup_steps, enable_adaptive, restore_adaptive, validate_identity
from android_brush_metrics import completion_window, validate_setup

PRESETS = {
    1: "gpen", 2: "pencil", 3: "eraser", 4: "paintbrush", 5: "airbrush",
    6: "chalk", 7: "marker", 8: "spray", 9: "dual-texture", 10: "smudge",
    11: "wet-round", 12: "liquify-push", 13: "liquify-twirl-ccw", 14: "multiply-glaze",
    15: "textured-flat", 16: "dry-scumble", 17: "pastel-block", 18: "transparent-glaze",
    19: "opaque-gouache", 20: "watercolor-wash", 21: "wet-watercolor", 22: "loaded-oil",
    23: "palette-knife", 24: "natural-blender",
    25: "pointy-pencil", 26: "shading-pencil", 27: "charcoal", 28: "rough-gpen",
    29: "calligraphy-pen", 30: "antique-pen", 31: "realistic-pen", 32: "wet-ink",
    33: "blotty-ink", 34: "brushed-ink", 35: "bristle-paintbrush", 36: "liquify-twirl-cw", 37: "liquify-pinch",
    38: "liquify-expand", 39: "liquify-crystals", 40: "clone-stamp", 41: "healing-brush",
    42: "spot-healing-brush", 45: "soft-blender", 46: "blur", 47: "bristle-blender", 48: "grainy-blender",
    49: "scatter-blender", 50: "painterly-blender", 51: "crystal-blender", 52: "marbling", 53: "comb-blender",
    55: "dither-blender", 56: "blending-knife", 57: "wet-blender", 58: "color-mixer",
    59: "mixing-brush", 60: "sharpen", 61: "texture-keeping-blender", 62: "hue-blender",
    63: "liquify-edge", 64: "liquify-reconstruct", 65: "blending-stump", 66: "tissue",
}
DRY_PRESETS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 18, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("output", type=pathlib.Path)
    p.add_argument("--adb", default="adb")
    p.add_argument("--serial", required=True, help="Target adb device serial")
    p.add_argument("--package", default="art.capycanvas.brushbench")
    p.add_argument("--adaptive-script", type=pathlib.Path, help="Configure the owned app's refresh policy before the prime stroke")
    p.add_argument("--adaptive-label")
    p.add_argument("--adaptive-apk", type=pathlib.Path)
    p.add_argument("--adaptive-aapt", type=pathlib.Path)
    p.add_argument("--target-hz", type=float)
    p.add_argument("--presets", default=",".join(map(str, DRY_PRESETS)))
    p.add_argument("--wait-for-warmup", action="store_true", help="Wait for the full shader catalogue after early photo adoption")
    p.add_argument("--probe-presets", help="Measure selected-brush preparation before the motion workload")
    p.add_argument("--warmup-idle-ms", type=int, help="Select a brush after this startup idle delay, without waiting for all shaders")
    p.add_argument("--size-idle-ms", type=int, default=0, help="Idle delay between selecting the brush and setting --size")
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--duration", type=int, default=10000)
    p.add_argument("--size", type=float, default=1000)
    p.add_argument("--mode", default="constant")
    p.add_argument("--pause-ms", type=int, default=100, help="Gap between identical contacts in pauses mode")
    p.add_argument("--contact-ms", type=int, default=100, help="Contact duration in pauses mode")
    p.add_argument("--settle-delay-ms", type=int, default=0, help="Delay before navigation in settle mode")
    p.add_argument("--paint-load", type=float, help="Bristle paint supply, 0 to 1; omit for the preset default")
    for name, help in (("strength", "Brush Strength"), ("opacity", "Brush Opacity"), ("smoothing", "Curve fit Smoothing")):
        p.add_argument(f"--{name}", type=float, help=f"{help}, 0 to 1; omit for the preset default")
    p.add_argument("--tilt", type=float, help="Fixed pen tilt in radians, 0 to 1.57")
    p.add_argument("--speed", type=float, default=1)
    p.add_argument("--prediction", choices=["true", "false"], default="true")
    p.add_argument("--horizon", type=int, default=16, help="Engine prediction lookahead in ms")
    p.add_argument("--radius-x", type=float, default=520, help="Ellipse radius in surface pixels")
    p.add_argument("--radius-y", type=float, default=299, help="Ellipse radius in surface pixels")
    p.add_argument("--photo", default="/data/local/tmp/capy-brush-photo.jpg", help="Tier JPEG or authored .capy fixture")
    p.add_argument("--canvas-width", type=int)
    p.add_argument("--canvas-height", type=int)
    p.add_argument("--navigation-between-strokes", action="store_true")
    p.add_argument("--navigation-settle-ms", type=int, default=750)
    p.add_argument("--zoom", type=float, help="Absolute view scale; default fits the canvas")
    p.add_argument("--blending", choices=["linear", "perceptual"], help="Document blend space; default uses the imported document")
    p.add_argument("--photo-layers", type=int, default=1, help="Photo layer count, with translucent duplicates")
    p.add_argument("--paint-layer-index", type=int, default=0, help="Paint layer index from the top, above the opaque base photo")
    p.add_argument("--paint-layer-name", help="Paint on this authored fixture layer instead of a new empty one")
    p.add_argument("--fixture-layers", type=int, help="Layer count of an authored fixture after setup, when it differs from the photo-layer model")
    p.add_argument("--color-mode", choices=["full_color", "grayscale", "two_tone"], default="full_color")
    p.add_argument("--workload", choices=["ordinary", "clipped", "blurred-base", "objects", "objects-effects"], default="ordinary")
    p.add_argument("--image-count", type=int, default=4)
    p.add_argument("--image-sources", choices=["shared", "unshared"], default="shared")
    p.add_argument("--effect-radius", type=float, default=8, help="Gaussian sigma in document pixels for blurred-base")
    p.add_argument("--live-filter", help="Built-in effect attached to the changing paint input")
    p.add_argument("--live-filter-values", default="{}", help="JSON object of keyed typed effect values")
    p.add_argument("--live-filter-disabled", action="store_true", help="Matched graph with its live filter disabled")
    tracing = p.add_mutually_exclusive_group()
    tracing.add_argument("--trace", action="store_true", help="Full CPU/GPU phase attribution")
    tracing.add_argument("--presentation-trace", action="store_true",
                         help="SurfaceFlinger/gfx only, without per-brush app trace scopes or GPU phase readbacks")
    p.add_argument("--profile", action="store_true", help="Also sample the isolated process with simpleperf")
    p.add_argument("--memory", action="store_true", help="Sample GPU allocations outside rate qualification runs")
    p.add_argument("--memory-idle-ms", type=int, default=0, help="Retain the final stroke while sampling idle memory")
    p.add_argument("--coverage-min-available-mib", type=int, default=1024,
                   help="Stop guarded coverage before the next stripe if memory headroom is too low")
    p.add_argument("--coverage-max-stripes", type=int, default=32)
    p.add_argument("--stats", action="store_true", help="Open Stats and enable GPU timing; omit for the default workspace")
    p.add_argument("--prefix", default="screen")
    args = p.parse_args()
    if args.adaptive_script and (not args.adaptive_script.is_file() or not args.adaptive_label or not args.adaptive_label.strip()
            or not args.adaptive_apk or not args.adaptive_aapt or not args.target_hz or not math.isfinite(args.target_hz)
            or args.target_hz <= 0 or args.mode in ("pinch", "vector")):
        p.error("Adaptive brush setup requires a helper, unique label, APK, aapt, positive target rate and brush motion")
    if (args.canvas_width is None) != (args.canvas_height is None):
        p.error("specify both --canvas-width and --canvas-height")
    if args.photo.endswith(".capy") and args.canvas_width is None:
        p.error("authored fixtures require --canvas-width and --canvas-height")
    if args.canvas_width is not None and min(args.canvas_width, args.canvas_height) <= 0:
        p.error("canvas dimensions must be positive")
    if not 0 <= args.navigation_settle_ms <= 5000:
        p.error("--navigation-settle-ms must be between 0 and 5000")
    if not 1 <= args.image_count <= 32:
        p.error("--image-count must be between 1 and 32")
    if not 0 <= args.memory_idle_ms <= 120000 or (args.memory_idle_ms and (not args.memory or args.mode == "pinch")):
        p.error("--memory-idle-ms requires --memory, a stroke mode, and 0..120000 ms")
    if args.mode == "coverage" and (args.presets != "12" or not args.memory or args.repeats != 1
                                    or args.paint_layer_name != "Photo"):
        p.error("--mode coverage requires --presets 12 --memory --repeats 1 --paint-layer-name Photo")
    if not 512 <= args.coverage_min_available_mib <= 8192 or not 1 <= args.coverage_max_stripes <= 32:
        p.error("coverage guards require 512..8192 MiB available and 1..32 stripes")
    for name in ("paint_load", "strength", "opacity", "smoothing"):
        if getattr(args, name) is not None and not 0 <= getattr(args, name) <= 1:
            p.error(f"--{name.replace('_', '-')} must be between 0 and 1")
    if args.tilt is not None and not 0 <= args.tilt <= 1.57:
        p.error("--tilt must be between 0 and 1.57")
    if not 0 <= args.paint_layer_index < args.photo_layers:
        p.error("--paint-layer-index must be between 0 and --photo-layers minus 1")
    if not 0 < args.effect_radius <= 85:
        p.error("--effect-radius must be greater than 0 and at most 85")
    if args.workload in ("clipped", "blurred-base") and (args.photo_layers != 1 or args.paint_layer_index != 0 or args.mode == "pinch"):
        p.error("attachment workloads require one photo, top paint and a brush motion")
    if args.workload.startswith("objects") and (not args.photo.endswith(".capy") or args.photo_layers != 2):
        p.error("object workloads require an authored fixture and --photo-layers 2")
    if args.mode == "object-affine" and not args.workload.startswith("objects"):
        p.error("object-affine motion requires an object workload")
    if args.wait_for_warmup and args.warmup_idle_ms is None:
        args.warmup_idle_ms = 0
    args.output.mkdir(parents=True, exist_ok=True)
    adb = [args.adb, "-s", args.serial]
    if args.adaptive_script:
        validate_identity(adb, args.package, args.adaptive_label, args.adaptive_apk, args.adaptive_aapt)
    remote = f"/sdcard/Android/data/{args.package}/files/brush-benchmark"
    failures = []

    def run(*items, **kw):
        return subprocess.run(adb + list(items), check=True, **kw)

    for preset in map(int, args.presets.split(",")):
        size_label = str(args.size).replace(".", "p")
        label = f"{args.prefix}-{PRESETS[preset]}-{size_label}-{args.mode}"
        if args.workload != "ordinary":
            label += f"-{args.workload}"
        if args.live_filter:
            label += f"-{args.live_filter}" + ("-disabled" if args.live_filter_disabled else "")
        requested = dict(preset=preset, brush_size=args.size, mode=args.mode,
                         prediction=args.prediction == "true", speed=args.speed,
                         duration_ms=args.duration, repeats=args.repeats,
                         radii=[args.radius_x, args.radius_y], photo_layers=args.photo_layers,
                         paint_layer_index=args.paint_layer_index,
                         horizon=args.horizon, zoom=args.zoom, blending=args.blending, stats_panel=args.stats)
        requested.update(wait_for_warmup=args.wait_for_warmup, warmup_idle_ms=args.warmup_idle_ms, size_idle_ms=args.size_idle_ms,
                         probe_presets=[int(value) for value in args.probe_presets.split(",")] if args.probe_presets else [])
        requested.update(workload=args.workload, effect_radius=args.effect_radius, color_mode=args.color_mode)
        requested.update({name: getattr(args, name) for name in ("strength", "opacity", "tilt", "smoothing") if getattr(args, name) is not None})
        if args.workload.startswith("objects"):
            requested.update(image_count=args.image_count, image_sources=args.image_sources)
        if args.canvas_width is not None:
            requested["canvas"] = [args.canvas_width, args.canvas_height]
        if args.fixture_layers is not None:
            requested["fixture_layers"] = args.fixture_layers
        if args.paint_layer_name:
            del requested["paint_layer_index"]
            requested["paint_layer_name"] = args.paint_layer_name
        if args.navigation_between_strokes:
            requested.update(navigation_between_strokes=True, navigation_settle_ms=args.navigation_settle_ms)
        if args.live_filter:
            requested.update(live_filter=args.live_filter, live_filter_values=json.loads(args.live_filter_values),
                             live_filter_disabled=args.live_filter_disabled)
        if args.mode == "pauses":
            requested["pause_ms"] = args.pause_ms
            requested["contact_ms"] = args.contact_ms
        if args.mode == "settle":
            requested["settle_delay_ms"] = args.settle_delay_ms
        if (args.output / f"{label}-complete.json").exists():
            if args.mode != "pinch":
                validate_setup(json.loads((args.output / f"{label}-info.json").read_text()), requested)
            print(f"SKIP completed {label}", flush=True)
            continue
        print(f"START {label}", flush=True)
        # Delete only this runner's synchronization files, never app documents.
        run("shell", f"rm -f {remote}/{label}-ready {remote}/{label}-go")
        if args.adaptive_script:
            run("shell", "rm", "-f", *(f"{remote}/{name}" for name in ["ready", "go", "refresh-before-primer.json"]))
        with (args.output / f"{label}-environment-before.txt").open("w") as out:
            run("shell", "cat /proc/meminfo; dumpsys thermalservice; dumpsys battery", stdout=out)
        cmd = adb + ["shell", "am", "instrument", "-w", "-r", "-e", "brushBenchmark", "true"]
        for key, value in dict(label=label, preset=preset, brushSize=args.size,
                               durationMs=args.duration, repeats=args.repeats, mode=args.mode,
                               speed=args.speed, prediction=args.prediction, horizon=args.horizon,
                               radiusX=args.radius_x, radiusY=args.radius_y, photo=args.photo,
                               photoLayers=args.photo_layers, paintLayerIndex=args.paint_layer_index,
                               workload=args.workload, effectRadius=args.effect_radius,
                               imageCount=args.image_count, imageSources=args.image_sources,
                               pauseMs=args.pause_ms, contactMs=args.contact_ms,
                               settleDelayMs=args.settle_delay_ms,
                               navigationBetweenStrokes=str(args.navigation_between_strokes).lower(),
                               navigationSettleMs=args.navigation_settle_ms,
                               memorySnapshots=str(args.memory).lower(), memoryIdleMs=args.memory_idle_ms, statsPanel=str(args.stats).lower(),
                               coverageMinAvailableMiB=args.coverage_min_available_mib,
                               coverageMaxStripes=args.coverage_max_stripes,
                               waitForTrace="true").items():
            cmd += ["-e", key, str(value)]
        if args.wait_for_warmup:
            cmd += ["-e", "waitForWarmup", "true"]
        if args.probe_presets:
            cmd += ["-e", "probePresets", args.probe_presets]
        if args.warmup_idle_ms is not None:
            cmd += ["-e", "warmupIdleMs", str(args.warmup_idle_ms)]
        if args.size_idle_ms:
            cmd += ["-e", "sizeIdleMs", str(args.size_idle_ms)]
        cmd += ["-e", "colorMode", args.color_mode]
        if args.live_filter:
            cmd += ["-e", "liveFilter", args.live_filter, "-e", "liveFilterValues", shlex.quote(args.live_filter_values),
                    "-e", "liveFilterDisabled", str(args.live_filter_disabled).lower()]
        if args.paint_load is not None:
            cmd += ["-e", "paintLoad", str(args.paint_load)]
        for name in ("strength", "opacity", "tilt", "smoothing"):
            if getattr(args, name) is not None:
                cmd += ["-e", name, str(getattr(args, name))]
        if args.paint_layer_name:
            cmd += ["-e", "paintLayerName", args.paint_layer_name]
        if args.zoom is not None:
            cmd += ["-e", "zoom", str(args.zoom)]
        if args.blending is not None:
            cmd += ["-e", "blending", args.blending]
        if args.canvas_width is not None:
            cmd += ["-e", "width", str(args.canvas_width), "-e", "height", str(args.canvas_height)]
        cmd += [f"{args.package}/art.capycanvas.BrushBenchmarkInstrumentation"]
        if args.adaptive_script:
            token = uuid.uuid4().hex
            cmd[-1:-1] = ["-e", "waitForGo", "true", "-e", "benchmarkReadyToken", token, "-e", "expectedRefreshHz", str(args.target_hz)]
        trace = None
        profile = None
        process = None
        with (args.output / f"{label}-instrumentation.txt").open("w") as log, cleanup_steps(
            lambda: run("shell", "am", "force-stop", args.package) if args.adaptive_script and process is not None and process.poll() is None else None,
            lambda: process.wait(timeout=30) if args.adaptive_script and process is not None and process.poll() is None else None,
            lambda: run("pull", f"{remote}/refresh-before-primer.json", str(args.output)) if args.adaptive_script else None,
            lambda: restore_adaptive(adb, args.package, args.adaptive_script, args.output, args.adaptive_label, args.adaptive_apk, args.adaptive_aapt) if args.adaptive_script else None,
        ):
            process = subprocess.Popen(cmd, stdout=log, stderr=log)
            deadline = time.monotonic() + 600
            prepared = not args.adaptive_script
            while process.poll() is None:
                if not prepared and subprocess.run(adb + ["shell", "test", "-f", f"{remote}/ready"], capture_output=True).returncode == 0:
                    enable_adaptive(adb, args.package, args.adaptive_script, args.output, args.adaptive_label, args.adaptive_apk, args.adaptive_aapt, remote, token)
                    run("shell", "touch", f"{remote}/go")
                    prepared = True
                check = subprocess.run(adb + ["shell", f"test -f {remote}/{label}-ready"], capture_output=True)
                if check.returncode == 0:
                    break
                if time.monotonic() > deadline:
                    run("pull", remote, str(args.output / "failed-setup"))
                    raise RuntimeError(f"Runner setup timed out: {label}")
                time.sleep(1)
            if process.poll() is not None:
                run("pull", remote, str(args.output / "failed-setup"))
                raise RuntimeError(f"Runner exited during setup: {label}; inspect instrumentation log")
            if args.mode != "pinch":
                info_path = args.output / f"{label}-info.json"
                run("pull", f"{remote}/{label}-info.json", str(info_path), stdout=subprocess.DEVNULL)
                try:
                    validate_setup(json.loads(info_path.read_text()), requested)
                except (ValueError, KeyError):
                    run("shell", "am", "force-stop", args.package)
                    process.wait(timeout=30)
                    raise
            if args.trace or args.presentation_trace:
                milliseconds = args.repeats * (args.duration + (15000 if args.mode == "settle" else 3500)) + args.memory_idle_ms + 8000
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
                trace_path = f"/data/misc/perfetto-traces/{args.package}-{label}.perfetto-trace"
                trace_log = (args.output / f"{label}-trace.log").open("w")
                trace = subprocess.Popen(adb + ["shell", "perfetto", "--txt", "-c", "-", "-o",
                    trace_path], stdin=subprocess.PIPE,
                    stdout=trace_log, stderr=trace_log)
                trace.stdin.write(config.encode())
                trace.stdin.close()
                time.sleep(1)
            if args.profile:
                seconds = math.ceil(args.repeats * (args.duration / 1000 + 3.5) + args.memory_idle_ms / 1000 + 3)
                profile_log = (args.output / f"{label}-profile.log").open("w")
                profile = subprocess.Popen(adb + ["shell", "simpleperf", "record", "--app", args.package,
                    "-e", "cpu-clock", "-f", "199", "--call-graph", "dwarf,16384", "--duration",
                    str(seconds), "-o", "/data/local/tmp/capy-brush.perf.data"],
                    stdout=profile_log, stderr=profile_log)
                time.sleep(1)
            run("shell", f"touch {remote}/{label}-go")
            process.wait(timeout=(900 if args.mode == "coverage" else
                                  args.repeats * (args.duration / 1000 + 15) + args.memory_idle_ms / 1000 + 120))
            if args.memory:
                run("pull", f"{remote}/{label}-memory.jsonl", str(args.output / f"{label}-memory.jsonl"))
            if trace:
                trace.wait(timeout=60)
                trace_log.close()
                run("pull", trace_path, str(args.output / f"{label}.perfetto-trace"))
            if profile:
                if profile.wait(timeout=60):
                    raise RuntimeError(f"CPU profile failed: {label}")
                profile_log.close()
                run("pull", "/data/local/tmp/capy-brush.perf.data", str(args.output / f"{label}.perf.data"))
        log = (args.output / f"{label}-instrumentation.txt").read_text()
        complete = "PINCH_COMPLETE" if args.mode == "pinch" else "BRUSH_COMPLETE"
        if f"{complete} {label}" not in log or process.returncode != 0:
            # A native crash may leave no screenshot or completed stroke report.
            # Preserve its evidence and continue the remaining preset matrix.
            for suffix in ["-info.json", "-coverage.json", "-error.txt", "-failed-state.json", "-failure-state.json",
                    "-failure-measurements.json", "-failure-gpu.json", "-memory.jsonl"] + [suffix for i in range(args.repeats)
                    for suffix in (f"-{i}.json", f"-{i}-diagnostic.json")] + (["-cold-setup.json", "-prime.json"] if args.workload.startswith("objects") else []):
                subprocess.run(adb + ["pull", f"{remote}/{label}{suffix}",
                    str(args.output / f"{label}{suffix}")], capture_output=True)
            with (args.output / f"{label}-crash-logcat.txt").open("w") as out:
                run("logcat", "-d", "-v", "threadtime", stdout=out)
            with (args.output / f"{label}-exit-state.txt").open("w") as out:
                run("shell", f"dumpsys activity exit-info {args.package}; dumpsys thermalservice", stdout=out)
            (args.output / f"{label}-failed.json").write_text(json.dumps({
                "label": label, "preset": preset, "returncode": process.returncode,
                "reason": f"Runner did not report {complete}; inspect instrumentation and crash logs"}, indent=2))
            print(f"FAILED {label}; crash evidence retained", flush=True)
            failures.append(label)
            continue
        # Pull reports individually; prior benchmark outputs remain on-device.
        suffixes = (["-info.json", "-before.png", "-after.png", "-measurements.json", "-complete.json"]
                    if args.mode == "pinch" else ["-info.json", "-coverage.json"] if args.mode == "coverage" else ["-info.json", ".png"]
                    + (["-detail.png"] if args.mode == "visual" else []) + [f"-{i}.json" for i in range(args.repeats)])
        if args.warmup_idle_ms is not None or args.workload.startswith("objects"):
            suffixes += ["-prime.json"]
        if args.workload.startswith("objects"):
            suffixes += ["-cold-setup.json"]
        if args.memory_idle_ms:
            suffixes += ["-idle-before.json", "-idle-after.json"]
        for suffix in suffixes:
            run("pull", f"{remote}/{label}{suffix}", str(args.output / f"{label}{suffix}"), stdout=subprocess.DEVNULL)
        info_path = args.output / f"{label}-info.json"
        info = json.loads(info_path.read_text())
        info["host_trace_kind"] = "full" if args.trace else "presentation" if args.presentation_trace else "none"
        info_path.write_text(json.dumps(info, indent=2))
        with (args.output / f"{label}-environment-after.txt").open("w") as out:
            run("shell", "cat /proc/meminfo; dumpsys thermalservice; dumpsys battery", stdout=out)
        if args.mode == "pinch":
            print(f"DONE {label} navigation measurements retained", flush=True)
            continue
        if args.mode == "coverage":
            coverage = json.loads((args.output / f"{label}-coverage.json").read_text())
            (args.output / f"{label}-complete.json").write_text(json.dumps(coverage, indent=2))
            print(f"DONE {label} geometric support upper bound={coverage['geometric_support_upper_bound_tiles']}/{coverage['possible_tile_cells']}"
                  f" stripes={len(coverage['stripes'])}/{coverage['rows_planned']} stop={coverage['stop_reason']}", flush=True)
            continue
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
