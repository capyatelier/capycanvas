#!/usr/bin/env python3
"""Run AndroidViewportBenchmarkTest under a SurfaceFlinger trace and report
presented canvas frames, presentation gaps and the viewport pass's GPU time.

Install the benchmark APK and its benchmark test APK under your own
application ID first. Presents join the canvas SurfaceView's buffer numbers to
SurfaceFlinger's actual display-frame times, so no clock conversion is needed.
"""
import argparse
import uuid
from android_adaptive import cleanup_steps, enable_adaptive, restore_adaptive, validate_identity
import csv
import io
import json
import math
import pathlib
import shutil
import subprocess
import struct
import time


def quantile(values, q):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * q) - 1)] if values else None


def presents(processor, trace, package, workspace=False):
    identity = (package + "/art.capycanvas.MainActivity").replace("'", "''")
    owned = ""
    if workspace:
        owned = f"""WITH owned_frames AS (
SELECT upid, CASE WHEN SUBSTR(layer_name, 1, 5) = 'TX - ' THEN SUBSTR(layer_name, 6) ELSE layer_name END layer
FROM actual_frame_timeline_slice WHERE layer_name IS NOT NULL
), app_processes AS (
SELECT DISTINCT upid FROM owned_frames
WHERE SUBSTR(layer, 1, INSTR(layer, '#') - 1) = '{identity}'
AND SUBSTR(layer, INSTR(layer, '#') + 1) != ''
AND SUBSTR(layer, INSTR(layer, '#') + 1) NOT GLOB '*[^0-9]*'
), owned_layers AS (
SELECT DISTINCT layer FROM owned_frames WHERE upid IN (SELECT upid FROM app_processes)
AND INSTR(layer, 'SurfaceView') = 0
)
"""
        match = "EXISTS (SELECT 1 FROM owned_layers o WHERE SUBSTR(s.name, 13, LENGTH(o.layer)) = o.layer AND SUBSTR(s.name, 13 + LENGTH(o.layer), 3) = ' - ' AND SUBSTR(s.name, 1, 12) = 'latchBuffer ')"
    else:
        match = f"s.name GLOB 'latchBuffer *SurfaceView*{package.replace(chr(39), chr(39) * 2)}/*'"
    query = owned + f"""SELECT s.name latch, f.ts + f.dur presented_at
FROM slice s JOIN ancestor_slice(s.id) a ON a.name GLOB 'commit [0-9]*'
JOIN actual_frame_timeline_slice f ON f.layer_name IS NULL AND CAST(f.name AS INT) = CAST(SUBSTR(a.name, 8) AS INT)
WHERE {match} ORDER BY presented_at"""
    out = subprocess.run([processor, "query", str(trace), query], capture_output=True, text=True, check=True).stdout
    rows = list(csv.DictReader(io.StringIO(out[out.index('"latch"'):]))) if '"latch"' in out else []
    return {(r["latch"] if workspace else int(r["latch"].rsplit(" ", 1)[1])): int(r["presented_at"]) for r in rows}


def presentation_stats(times):
    gaps = [(b - a) / 1e6 for a, b in zip(times, times[1:])]
    return dict(presented_per_s=(len(times) - 1) * 1e9 / (times[-1] - times[0]) if len(times) > 1 and times[-1] > times[0] else None,
                presented_gap_p99_ms=quantile(gaps, .99), actual_present_timestamps_ns=times)


def contact_summary(shown, begin, end, sustained=True, trim=True):
    contact = sorted({time for time in shown.values() if begin <= time <= end})
    moving = [time for time in contact if begin + 100_000_000 <= time <= end - 100_000_000]
    if not contact or sustained and (end - begin < 5_000_000_000 or len(moving) < 2):
        raise ValueError("Actual window presentations unavailable inside chrome motion")
    result = presentation_stats(moving if sustained else contact)
    if not sustained:
        result.pop("presented_per_s")
    if not trim:
        result["actual_present_timestamps_ns"] = contact
    return dict(result, full_contact_presents=len(contact), full_contact_ms=(end - begin) / 1e6,
                onset_ms=(contact[0] - begin) / 1e6,
                full_contact_longest_gap_ms=max([(b - a) / 1e6 for a, b in zip(contact, contact[1:])], default=None))

def summarize(run, shown):
    begin, end = run["begin_ns"], run["end_ns"]
    frames = sorted(row[0] for row in run["completions"] if begin <= row[1] <= end)
    times = sorted({shown[f] for f in frames if f in shown})
    gpu = [sample[1] / 1e6 for batch in run["presentation"] for sample in batch if sample[2] == 1]
    result = dict(presentation_stats(times), submitted_per_s=len(frames) / ((end - begin) / 1e9),
                  viewport_gpu_p50_ms=quantile(gpu, .5), viewport_gpu_p95_ms=quantile(gpu, .95))
    boot = workspace_boot_window(run) if any(key in run for key in ["begin_mono_after_boot_ns", "end_mono_after_boot_ns"]) else None
    if boot is not None:
        result.update(attributed_presented_per_s=result["presented_per_s"], **contact_summary(shown, *boot), window_source="explicit boot timestamps")
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


def trace_config(package, workspace=False, profile=False, attribution=False, gpu_renderstages=False, duration_ms=600000):
    config = '''buffers { size_kb: 65536 fill_policy: RING_BUFFER }
duration_ms: 600000
write_into_file: true
file_write_period_ms: 2000
data_sources { config { name: "linux.ftrace" ftrace_config { ftrace_events: "ftrace/print" atrace_categories: "gfx" } } }
data_sources { config { name: "android.surfaceflinger.frametimeline" } }
'''
    config = config.replace("duration_ms: 600000", f"duration_ms: {duration_ms}")
    if workspace and profile:
        config = config.replace('atrace_categories: "gfx"', f'atrace_categories: "gfx" atrace_apps: "{package}"')
    if attribution:
        config += 'flush_period_ms: 2000\ndata_sources { config { name: "linux.process_stats" process_stats_config { scan_all_processes_on_start: true proc_stats_poll_ms: 1000 } } }\n'
        config = config.replace('atrace_categories: "gfx"',
            f'atrace_categories: "gfx" atrace_categories: "view" atrace_categories: "input" atrace_categories: "dalvik" atrace_apps: "{package}" '
            'ftrace_events: "sched/sched_switch" ftrace_events: "sched/sched_waking" ftrace_events: "power/cpu_frequency"')
        if gpu_renderstages:
            config += 'data_sources { config { name: "gpu.renderstages" } }\n'
    return config


def attribution_capabilities(adb):
    result = subprocess.run(adb + ["shell", "perfetto", "--query"], capture_output=True, text=True)
    return dict(gpu_renderstages="supported" if result.returncode == 0 and "gpu.renderstages" in result.stdout else "unsupported or unavailable",
                query_returncode=result.returncode, query_stdout=result.stdout, query_stderr=result.stderr)


def diagnostic_summary(summary, capabilities):
    return dict(measurement="Diagnostic attribution trace; no FPS qualification", qualification=False,
                attribution_capabilities=capabilities, workloads=summary)


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
    return dict(contact_summary(shown, begin, end), run=run["run"],
                measurement="SurfaceFlinger app-window latches joined to actual display-frame presentations", window_source=window_source,
                thermal_before=run["thermal_before"], thermal_after=run["thermal_after"],
                frame_metrics_proxy_hz=run["frame_rate"], frame_metrics_proxy_gap_p99_ms=run["vsync_interval_p99_ms"],
                **{key: run[key] for key in ["changed_bounds", "changed_bounds_rate", "drawn_revisions", "drawn_update_rate"] if key in run})


WORKSPACE_WORKLOADS = {
    "gap": ("columnBridgeGapFrameTiming", None),
    "panel": ("columnBridgePanelFrameTiming", None),
    "column": ("columnBridgePanelFrameTiming", None),
    "resize": ("continuousResizeFrameTiming", ["brushes", "properties", "navigator", "toolbar"]),
    "drag": ("continuousDragFrameTiming", ["attached", "floating", "destination"]),
    "wheel": ("colorWheelFrameTiming", ["color-wheel"]),
    "edit-wheel": ("editColorWheelFrameTiming", ["color-editor-wheel"]),
    "color-overlap": ("colorPanelOverlapFrameTiming", ["color-overlap"]),
}


CHROME_WORKLOADS = {
    "switcher-scroll": ("AndroidWorkspacePerformanceTest#workspaceSwitcherScrollFrameTiming", "workspace-switcher-benchmark", {"switcherBenchmark": "true"}, ["choices", "options"]),
    "grouped-scroll": ("AndroidWorkspacePerformanceTest#groupedDrawerScrollFrameTiming", "grouped-drawer-benchmark", {"groupedToolBenchmark": "true"}, ["grouped-drawer"]),
    "layer-swipe": ("AndroidTitleBarTest#layerSwipeFrameTiming", "", {"layerSwipeBenchmark": "true"}, ["layer-swipe"]),
    "layer-reorder": ("AndroidTitleBarTest#layerSwipeFrameTiming", "", {"layerReorderBenchmark": "true"}, ["layer-reorder"]),
    "filter-reorder": ("AndroidTitleBarTest#layerSwipeFrameTiming", "", {"layerReorderBenchmark": "true", "layerRelationshipBenchmark": "true"}, ["layer-reorder"]),
    "layer-selection": ("AndroidTitleBarTest#layerSwipeFrameTiming", "", {"layerSelectionBenchmark": "true"}, ["layer-selection"]),
    "layer-properties": ("AndroidTitleBarTest#layerSwipeFrameTiming", "", {"layerPropertiesBenchmark": "true"}, ["layer-properties"]),
}

DISCRETE_CHROME_LABELS = {"ui-drawer-open-close", "ui-panel-change", "ui-bar-show-hide", "ui-bar-show-hide-zen", "ui-zen-transition", "ui-menu-open", "ui-menu-submenu", "ui-dialog-transition", "ui-grouped-tool-menu", "ui-tile-drag"}

CHROME_WORKLOADS["ui-batch"] = ("AndroidCanvasBarBenchmarkTest#canvasBarFrameTiming", "canvas-bar-benchmark",
                                {"canvasBarBenchmark": "true", "scenarios": "ui"},
                                ["ui-drawer-open-close", "ui-bar-move", "ui-panel-change", "ui-bar-show-hide", "ui-bar-show-hide-zen", "ui-zen-transition",
                     "ui-layer-opacity-value", "ui-layers-scroll", "ui-brushes-scroll", "ui-filters-scroll",
                     "ui-menu-open", "ui-menu-submenu", "ui-dialog-transition", "ui-grouped-tool-menu", "ui-tile-drag",
                     "ui-brush-size-slider", "ui-brush-size-value", "ui-brush-opacity-slider", "ui-brush-opacity-value",
                     "ui-brush-flow-slider", "ui-brush-flow-value", "ui-layer-opacity-slider", "ui-layer-row-opacity"])


def chrome_labels(workload, selected=None):
    if selected is None:
        return CHROME_WORKLOADS[workload][3]
    if workload != "ui-batch" or not selected or any(label not in CHROME_WORKLOADS["ui-batch"][3] for label in selected):
        raise ValueError("--labels requires ui-batch and known native UI labels")
    return list(dict.fromkeys(selected))


def chrome_arguments(workload, theme, repeats, selected=None):
    selected = chrome_labels(workload, selected) if selected is not None else None
    selector, _, arguments, _ = CHROME_WORKLOADS[workload]
    arguments = dict(arguments, theme=theme, layerBenchmarkTheme=theme)
    if selected is not None:
        arguments["labels"] = ",".join(selected)
    if selector.startswith("AndroidCanvasBar"):
        if repeats < 3:
            raise ValueError("CanvasBar UI workloads require at least three measured repeats")
        arguments.update(uiRepeats=str(repeats), zoomOut="false")
    return ["-e", "class", f"art.capycanvas.{selector}"] + [part for key, value in arguments.items() for part in ["-e", key, value]]


def chrome_reports(workload, repeats, selected=None):
    selector = CHROME_WORKLOADS[workload][0]
    labels = chrome_labels(workload, selected)
    count = repeats if selector.startswith("AndroidCanvasBar") else 3
    return [f"{label}-{run}.json" for label in labels for run in range(1, count + 1)]


def geometry_completeness(run):
    if "moving_draw_mono_ns" not in run or not run.get("draw_geometry"):
        return {}
    columns = run["draw_geometry_columns"]
    mono_index = columns.index("draw_mono_ns")
    boot_index = columns.index("draw_boot_ns")
    draws = run["draw_geometry"]
    monotonic = [row[mono_index] for row in draws]
    boots = [row[boot_index] for row in draws]
    if any(type(time) is not int for time in monotonic + boots) or any(b < a for a, b in zip(monotonic, monotonic[1:])) or any(b < a for a, b in zip(boots, boots[1:])):
        raise ValueError("Draw geometry requires ordered monotonic and boot timestamps")
    moving = set(run["moving_draw_mono_ns"])
    if not moving or not moving.issubset(monotonic):
        raise ValueError("Draw geometry must contain changing drawn poses")
    transitions = run.get("transitions_ns", [])
    windows = run.get("active_windows", [])
    if run.get("motion") == "layer-properties" and (not transitions or len(windows) < len(transitions) * .95):
        raise ValueError("Native drawer requests must produce observed geometry transitions")
    for window in windows:
        if not any(window["begin_boot_ns"] <= time <= window["end_boot_ns"] for time in boots):
            raise ValueError("Drawer active windows require actual draw coverage")
    return dict(draw_geometry_samples=len(draws), moving_draw_samples=len(moving),
                observed_geometry_transitions=len(windows), native_requests=len(transitions))


def selection_observations(run, shown):
    boot = workspace_boot_window(run)
    if boot is None:
        raise ValueError("Selection observations require explicit boot clock samples")
    begin, end = boot
    markers = run.get("transition_observations_boot_ns")
    if not isinstance(markers, list) or not markers or any(type(marker) is not int or not begin <= marker <= end for marker in markers):
        raise ValueError("Selection observations must be integer boot timestamps inside the contact")
    markers = sorted(set(markers))
    times = sorted({time for time in shown.values() if begin <= time <= end})
    return dict(measurement="Diagnostic instantaneous selection state observations and app-window presentations; no fresh-selection latency or sustained FPS qualification",
                qualification=False, sustained=False, begin_boot_ns=begin, end_boot_ns=end,
                actual_present_timestamps_ns=times,
                observations=[dict(observed_state_change_boot_ns=marker,
                                   next_window_present_boot_ns=next((time for time in times if time >= marker), None)) for marker in markers],
                **{key: run[key] for key in ["run", "motion", "theme", "thermal_before", "thermal_after"] if key in run})


def window_presents(run, shown):
    fixture = run.get("fixture") or {}
    title = run.get("window_title") or fixture.get("window_title")
    counts = {}
    selected = {}
    for latch, time in shown.items():
        if not isinstance(latch, str):
            selected[latch] = time
            continue
        layer = latch[len("latchBuffer "):].rsplit(" - ", 1)[0]
        counts[layer] = counts.get(layer, 0) + 1
        layer_title = layer.rsplit("#", 1)[0] if layer.rsplit("#", 1)[-1].isdigit() else layer
        if title is None or layer_title == title:
            selected[latch] = time
    if title is not None and not selected:
        raise ValueError(f"Actual presentations unavailable for window title: {title}")
    return selected, dict(window_title=title, window_selection="literal native target window" if title is not None else "app-owned aggregate; target window unspecified", owned_surface_present_counts=counts)


def replay_numeric(run, resolver):
    origin = run.get("scalar_origin", {})
    mapping = origin.get("mapping")
    inputs = run.get("scalar_input_samples")
    if not mapping or not inputs or not origin.get("resolved"):
        run["scalar_replay"] = dict(status="unavailable", reason="Exact control/origin/input geometry missing")
        return
    toolbar = mapping.get("host_control") == "toolbar_slider"
    if toolbar and (not mapping.get("toolbar_control") or mapping.get("bookmark_values") is None):
        run["scalar_replay"] = dict(status="unavailable", reason="Toolbar DOWN bookmark control/values missing")
        return
    def f32(value):
        return struct.unpack("f", struct.pack("f", value))[0]
    control = mapping["control"]
    value = origin["delivered_scalar"]
    density = origin["density"]
    down = inputs[0]
    operation = mapping["operation"]
    moved = False
    material_slop = 0.0
    down_normalizations = []
    requests = [dict(boot_ns=origin["boot_ns"], request=dict(control=control, value=value, operation=dict(type="format")))]
    bounds = origin["control_geometry"]["screen_bounds"]
    for row in inputs:
        timestamp = row["begin_boot_ns"]
        contact_begin = run.get("motion", {}).get("contact_begin_boot_ns")
        if row["action"] == 0 and type(contact_begin) is int and timestamp < contact_begin <= row.get("end_boot_ns", timestamp):
            down_normalizations.append(dict(input_begin_boot_ns=timestamp, input_end_boot_ns=row["end_boot_ns"],
                replay_boot_ns=contact_begin, reason="DOWN injection interval contains the recorded contact-start marker"))
            timestamp = contact_begin
        if row["action"] == 3:
            continue
        if "injected_screen_x" not in row or "injected_screen_y" not in row:
            run["scalar_replay"] = dict(status="unavailable", reason="Actual Float pointer coordinates missing")
            return
        if toolbar:
            if row["action"] == 1:
                continue
            if row["action"] != 0 and not moved:
                distance = math.hypot(row["injected_screen_x"]-down["injected_screen_x"], row["injected_screen_y"]-down["injected_screen_y"])
                if distance < 3*density:
                    continue
                moved = True
            vertical = mapping["axis"] == "vertical"
            travel = f32(f32((bounds[3]-bounds[1] if vertical else bounds[2]-bounds[0])-16*density)/density)
            local = f32((bounds[3]-row["injected_screen_y"] if vertical else row["injected_screen_x"]-bounds[0])-8*density)
            position = f32(max(0,min(1,f32(local/f32(travel*density)))))
            if row["action"] == 0:
                requests.append(dict(boot_ns=timestamp, toolbar_request=dict(type="slider_bookmark_value", control=mapping["toolbar_control"],
                    values=[f32(item) for item in mapping["bookmark_values"]], position=position, travel=travel)))
                continue
            op = dict(type="position", position=position)
        elif operation in ["scrub", "relative_position"]:
            pixels = f32(f32(down["injected_screen_y"]-row["injected_screen_y"])/density)
            if not moved and abs(pixels) <= origin["touch_slop_px"]/density:
                continue
            moved = True
            op = dict(type="scrub", origin=value, pixels=pixels, fine=False) if operation == "scrub" else dict(type="position", position=f32(f32(origin["resolved"]["fill"])+f32(pixels/200)))
        elif operation == "position":
            if row["action"] == 0:
                continue
            if row["action"] == 1:
                if moved:
                    continue
                local = f32(down["injected_screen_x"]-bounds[0])
            else:
                delta = f32(row["injected_screen_x"]-down["injected_screen_x"])
                if not moved:
                    if abs(delta) < origin["touch_slop_px"]:
                        continue
                    material_slop = math.copysign(origin["touch_slop_px"], delta)
                    moved = True
                local = f32(f32(row["injected_screen_x"]-bounds[0])-material_slop)
            op = dict(type="position", position=f32(max(0, min(1, f32(local/f32(bounds[2]-bounds[0]))))))
        else:
            run["scalar_replay"] = dict(status="unavailable", reason="Unsupported native numeric operation")
            return
        requests.append(dict(boot_ns=timestamp, previous_float_value=True, request=dict(control=control, value=value, operation=op)))
    result = subprocess.run([str(resolver)], input="".join(json.dumps(row)+"\n" for row in requests), text=True, capture_output=True, check=True)
    resolved = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    if len(resolved) != len(requests) or any("error" in row for row in resolved):
        raise ValueError("Shared numeric resolver failed or returned incomplete replay")
    run["scalar_implied_samples"] = [dict(boot_ns=row["boot_ns"], value=row["float_value"]) for row in resolved]
    run["scalar_replay"] = dict(status="exact", resolver=str(resolver), source="captured native control/origin/Float input; shared NumericRequest resolver", operation=operation, down_boundary_normalizations=down_normalizations)


def scalar_changes(rows, begin, end, timestamp, value, previous=None):
    times, observations = [], []
    last = None
    for row in rows:
        time = timestamp(row)
        if type(time) is not int or last is not None and time < last:
            raise ValueError("Scalar timestamps must be monotonic")
        last = time
        current = value(row)
        if not begin <= time <= end:
            continue
        observations.append(time)
        if current is None or current == "":
            raise ValueError("Scalar target has no observable value")
        if previous is not None and current != previous:
            times.append(time)
        previous = current
    return times, observations


def scalar_change_stats(times, begin, end, prefix):
    stats = presentation_stats(times)
    return {prefix+"_changes": len(times), prefix+"_changes_per_s": len(times)*1e9/(end-begin),
            prefix+"_gap_p99_ms": stats["presented_gap_p99_ms"], prefix+"_timestamps_boot_ns": times}


def scalar_implied_summary(run, delivered_times):
    rows = run.get("scalar_implied_samples")
    proof = run.get("scalar_replay", {})
    if rows is None or proof.get("status") != "exact":
        return dict(implied_value_gate="unavailable: exact shared-resolver replay required")
    begin, end = run["motion"]["contact_begin_boot_ns"], run["motion"]["contact_end_boot_ns"]
    implied, _ = scalar_changes(rows, begin, end, lambda row: row["boot_ns"], lambda row: row["value"],
                                run.get("scalar_origin", {}).get("delivered_scalar"))
    response_gaps = []
    for before, after in zip([begin] + delivered_times, delivered_times + [end]):
        demand = next((time for time in implied if (before < time or before == begin == time) and time <= after), None)
        if demand is not None:
            response_gaps.append((after-demand)/1e6)
    count = len(implied)
    eligible = []
    previous = run.get("scalar_origin", {}).get("delivered_scalar")
    cursor = 0
    value = previous
    for opportunity in run.get("scalar_display_opportunities_boot_ns", []):
        while cursor < len(rows) and rows[cursor]["boot_ns"] <= opportunity:
            value = rows[cursor]["value"]
            cursor += 1
        if begin <= opportunity <= end and previous is not None and value != previous:
            eligible.append(opportunity)
        previous = value
    return dict(implied_value_gate="shared-resolver demand; raw ratio is descriptive, not refresh-qualified or pixel latency",
                display_eligible_implied_changes=len(eligible) if "scalar_display_opportunities_boot_ns" in run else None,
                display_eligible_implied_changes_per_s=len(eligible)*1e9/(end-begin) if "scalar_display_opportunities_boot_ns" in run else None,
                display_eligible_implied_timestamps_boot_ns=eligible,
                display_opportunity_proof=run.get("scalar_display_opportunity_proof", "unavailable"),
                delivered_to_display_eligible_change_ratio=len(delivered_times)/len(eligible) if eligible else None,
                **scalar_change_stats(implied, begin, end, "implied_value"),
                delivered_to_implied_change_ratio=len(delivered_times)/count if count else None,
                demand_conditioned_delivery_gap_p99_ms=quantile(response_gaps, .99),
                demand_conditioned_delivery_gap_max_ms=max(response_gaps, default=None))


def scalar_draw_summary(run):
    samples = run.get("scalar_draw_samples")
    if samples is None:
        return dict(fresh_value_gate="unavailable: no scalar draw observations")
    contact = run["motion"]
    begin, end = contact["contact_begin_boot_ns"], contact["contact_end_boot_ns"]
    if type(begin) is not int or type(end) is not int or end <= begin:
        raise ValueError("Scalar draw observations require an ordered contact")
    origin = run.get("scalar_origin", {})
    changes = {}
    for key in ["delivered_scalar", "visual_signature"]:
        changes[key], observations = scalar_changes(samples, begin, end, lambda row: row["boot_ns"],
                                                    lambda row: row[key], origin.get(key))
    if not observations:
        raise ValueError("No scalar draw observations inside contact")
    result = dict(fresh_value_gate="delivered scalar and composed semantics observed at draw; not pixel/SF identity or accepted-input rate",
                  scalar_observation_count=len(observations), scalar_contact_span_ms=(end-begin)/1e6,
                  scalar_observation_onset_ms=(observations[0]-begin)/1e6,
                  scalar_observation_trailing_ms=(end-observations[-1])/1e6)
    for key, prefix in [("delivered_scalar", "delivered_scalar"), ("visual_signature", "fresh_draw_value")]:
        result.update(scalar_change_stats(changes[key], begin, end, prefix))
    result.update(scalar_implied_summary(run, changes["delivered_scalar"]))
    if "scalar_observer_overhead" in run:
        result["scalar_observer_overhead"] = run["scalar_observer_overhead"]
    inputs = run.get("scalar_input_samples")
    if inputs is not None:
        for row in inputs:
            if type(row["end_boot_ns"]) is not int or type(row["begin_boot_ns"]) is not int or row["end_boot_ns"] < row["begin_boot_ns"]:
                raise ValueError("Scalar input timestamps must be ordered injection intervals")
        changed, observations = scalar_changes(inputs, begin, end, lambda row: row["begin_boot_ns"], lambda row: (row["x"], row["y"]))
        stats = scalar_change_stats(changed, begin, end, "injected_scalar_position")
        stats.pop("injected_scalar_position_timestamps_boot_ns")
        result.update(stats, injected_scalar_input_events=len(observations))
    return result


def chrome_summary(run, shown):
    shown, surfaces = window_presents(run, shown)
    if run.get("motion") == "layer-selection":
        return dict(selection_observations(run, shown), **surfaces)
    if run.get("scalar_implied_samples") is not None and isinstance(run.get("motion"), dict):
        begin, end = run["motion"]["contact_begin_boot_ns"], run["motion"]["contact_end_boot_ns"]
        anchor = next((time for time in sorted(shown.values()) if begin <= time <= end), None)
        hz = run.get("display_hz")
        if anchor is not None and isinstance(hz, (int, float)) and hz > 0:
            period = 1e9 / hz
            first = anchor - math.floor((anchor-begin)/period)*period
            run = dict(run, scalar_display_opportunities_boot_ns=[round(first+i*period) for i in range(int((end-first)/period)+1)],
                       scalar_display_opportunity_proof="inferred display-mode lattice anchored to actual literal-target SF completion; not fresh buffer identity")
    completeness = geometry_completeness(run)
    discrete = run.get("qualification") == "diagnostic" or run.get("label") in DISCRETE_CHROME_LABELS or run.get("motion") in ["layer-selection", "layer-properties"]
    windows = run.get("active_windows")
    if windows is None:
        if discrete:
            raise ValueError("Discrete chrome recordings require active_windows")
        boot = workspace_boot_window(run)
        if boot is None:
            raise ValueError("Chrome recording requires explicit boot clock samples or active_windows")
        windows = [dict(begin_boot_ns=boot[0], end_boot_ns=boot[1])]
    if not windows:
        raise ValueError("Chrome recording has no active motion windows")
    summaries = []
    for index, window in enumerate(windows):
        begin, end = window["begin_boot_ns"], window["end_boot_ns"]
        if type(begin) is not int or type(end) is not int or end <= begin:
            raise ValueError("Chrome active window must have ordered integer boot timestamps")
        observed_end = end
        diagnostic = discrete or window.get("qualification") == "diagnostic"
        contact = run.get("motion") if isinstance(run.get("motion"), dict) else run
        limit = windows[index + 1]["begin_boot_ns"] if index + 1 < len(windows) else contact.get("end_boot_ns")
        completion = min((time for time in shown.values() if diagnostic and type(limit) is int and end < time <= limit), default=None)
        if completion is not None:
            end = completion
        sustained = end - begin >= 5_000_000_000 and not diagnostic
        summary = dict(contact_summary(shown, begin, end, sustained, trim=False), begin_boot_ns=begin, end_boot_ns=end,
                       observed_draw_end_boot_ns=observed_end, completion_present_observation_boot_ns=completion,
                       active_ms=(end - begin) / 1e6, qualification="diagnostic" if diagnostic else "sustained motion candidate")
        summary["longest_gap_ms"] = summary.pop("full_contact_longest_gap_ms")
        summaries.append(summary)
    return dict(measurement="SurfaceFlinger app-window actual presentations in separate active windows",
                sustained=all("presented_per_s" in window for window in summaries), windows=summaries, **completeness, **surfaces,
                **scalar_draw_summary(run),
                **{key: run[key] for key in ["run", "label", "mode", "presentation", "motion", "theme", "thermal_before", "thermal_after", "changed_bounds", "changed_bounds_rate", "column_docking_witness"] if key in run})


def workspace_directory(workload):
    return "workspace-bridge-benchmark" if WORKSPACE_WORKLOADS[workload][1] is None else "workspace-tab-benchmark"


def workspace_arguments(workload, runner):
    method, modes = WORKSPACE_WORKLOADS[workload]
    if runner == "self":
        if modes is not None:
            raise ValueError("The self workspace runner supports only gap and panel")
        return ["-e", "workspaceWorkload", "panel" if workload == "column" else workload] + (["-e", "columnDrag", "true"] if workload == "column" else [])
    return ["-e", "class", f"art.capycanvas.AndroidWorkspacePerformanceTest#{method}"] + (["-e", "columnDrag", "true"] if workload == "column" else []) + (["-e", "tierPhoto", "true"] if modes is not None else [])


def workspace_reports(workload, label, repeats, selected_mode=None, warmup=False):
    runs = [-1] if warmup else range(repeats)
    modes = WORKSPACE_WORKLOADS[workload][1]
    if modes is None:
        return [f"{label}-{run}.json" for run in runs]
    if selected_mode is not None:
        if selected_mode not in modes:
            raise ValueError(f"Unknown {workload} benchmarkMode: {selected_mode}")
        modes = [selected_mode]
    return [f"{label}-{mode}-{source}-{run}.json" for source in ["mouse", "touch"] for mode in modes for run in runs]


def chrome_interactions(workload, repeats, selected=None):
    selector, _, _, labels = CHROME_WORKLOADS[workload]
    if not selector.startswith("AndroidCanvasBar"):
        return []
    return [filename for label in chrome_labels(workload, selected) for filename in [f"{label}-primer-interaction.json"] +
            [f"{label}-contact-{run}-interaction.json" for run in range(1, repeats + 1)]]


def retain_failed_reports(adb, remote, output, reports):
    retained = []
    for filename in reports:
        exists = subprocess.run(adb + ["shell", "test", "-f", f"{remote}/{filename}"], capture_output=True)
        if exists.returncode == 0:
            subprocess.run(adb + ["pull", f"{remote}/{filename}", str(output)], check=True, capture_output=True)
            retained.append(filename)
    return retained


def capture_budget_seconds(workload, repeats, duration_ms, selected=None):
    if workload in CHROME_WORKLOADS and CHROME_WORKLOADS[workload][0].startswith("AndroidCanvasBar"):
        rows = len(chrome_labels(workload, selected))
        return max(900, 180 + rows * (60 + (repeats + 1) * (duration_ms / 1000 + 3)))
    return 900


def write_run_summary(path, summary, capabilities=None):
    path.write_text(json.dumps(diagnostic_summary(summary, capabilities) if capabilities is not None else summary, indent=2))
    if any(row.get("instrumentation_passed") is False for row in summary.values()):
        raise SystemExit(1)


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
    p.add_argument("--duration-ms", type=int, default=6000, help="Gesture length; slow tiers need more than 100 GPU samples per run")
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--extra", default="", help="Further instrumentation arguments as key=value,key=value")
    p.add_argument("--workspace-workload", choices=list(WORKSPACE_WORKLOADS) + list(CHROME_WORKLOADS))
    p.add_argument("--workspace-runner", choices=["junit", "self"], default="junit", help="Self runs inside the benchmark APK and supports R8 without a JUnit APK")
    p.add_argument("--viewport-runner", choices=["junit", "self"], default="junit", help="Self reuses the viewport helper inside an optimized benchmark APK")
    p.add_argument("--default-canvas", action="store_true", help="Navigate the actual startup canvas and paper, without creating or importing artwork")
    p.add_argument("--theme", choices=["light", "dark"], default="light")
    p.add_argument("--profile", action="store_true", help="Workspace diagnostic: Compose trace and 199 Hz app-only CPU stacks during the first measured contact; not FPS qualification")
    p.add_argument("--attribution", action="store_true", help="Diagnostic app/UI/scheduler attribution trace; no FPS qualification")
    p.add_argument("--adaptive-script", type=pathlib.Path)
    p.add_argument("--adaptive-label", help="Exact unique app label in the verified own APK")
    p.add_argument("--adaptive-apk", type=pathlib.Path)
    p.add_argument("--adaptive-aapt", type=pathlib.Path)
    p.add_argument("--numeric-resolver", type=pathlib.Path, help="Shared NumericRequest JSONL replay executable")
    p.add_argument("--labels", help="Comma-separated native UI labels for ui-batch: " + ", ".join(CHROME_WORKLOADS["ui-batch"][3]))
    args = p.parse_args()
    if args.adaptive_script and (not args.adaptive_script.is_file() or not args.adaptive_label or not args.adaptive_label.strip() or not args.adaptive_apk or not args.adaptive_aapt or args.profile or args.attribution or not args.workspace_workload):
        p.error("Adaptive requires the verified own APK, aapt, and a clean workspace run")
    if args.numeric_resolver and not args.numeric_resolver.is_file():
        p.error("Shared numeric resolver executable is unavailable")
    selected = args.labels.split(",") if args.labels is not None else None
    if args.attribution and (not args.workspace_workload or args.profile):
        p.error("--attribution requires a workspace workload and cannot combine with --profile")
    if args.workspace_runner == "self" and not args.workspace_workload:
        p.error("--workspace-runner self requires --workspace-workload")
    chrome = args.workspace_workload in CHROME_WORKLOADS
    if args.workspace_workload and (chrome or WORKSPACE_WORKLOADS[args.workspace_workload][1] is not None) and (args.workspace_runner == "self" or args.profile):
        p.error("Nonbridge workspace workloads require the JUnit runner without --profile")
    if args.default_canvas and args.workspace_workload:
        p.error("--default-canvas is only for viewport navigation")
    extra = dict(item.split("=", 1) for item in filter(None, args.extra.split(",")))
    if args.adaptive_script:
        try:
            refresh = float(extra.get("expectedRefreshHz", "0"))
        except ValueError:
            p.error("Adaptive requires a positive --extra expectedRefreshHz")
        if not math.isfinite(refresh) or refresh <= 0:
            p.error("Adaptive requires a positive --extra expectedRefreshHz")
    if chrome or selected is not None:
        try:
            chrome_arguments(args.workspace_workload, args.theme, args.repeats, selected)
        except ValueError as error:
            p.error(str(error))
    if args.workspace_workload and not chrome:
        try:
            workspace_reports(args.workspace_workload, args.label, args.repeats, extra.get("benchmarkMode"))
        except ValueError as error:
            p.error(str(error))
    adb = [shutil.which("adb") or str(pathlib.Path.home() / "Android/Sdk/platform-tools/adb"), "-s", args.serial]
    if args.adaptive_script:
        validate_identity(adb, args.package, args.adaptive_label, args.adaptive_apk, args.adaptive_aapt)
    args.output.mkdir(parents=True, exist_ok=True)
    remote = f"/sdcard/Android/data/{args.package}/files/" + (CHROME_WORKLOADS[args.workspace_workload][1] if chrome else workspace_directory(args.workspace_workload) if args.workspace_workload else "viewport-benchmark")
    budget = capture_budget_seconds(args.workspace_workload, args.repeats, args.duration_ms, selected)
    capabilities = attribution_capabilities(adb) if args.attribution else None
    config = trace_config(args.package, bool(args.workspace_workload), args.profile, args.attribution,
                          capabilities is not None and capabilities["gpu_renderstages"] == "supported", duration_ms=int((budget + 30) * 1000))
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
            selector = chrome_arguments(motion, args.theme, args.repeats, selected) if chrome else workspace_arguments(motion, args.workspace_runner)
            cmd = adb + ["shell", "am", "instrument", "-w"] + selector + [
                         "-e", "workspaceBenchmark", "true", "-e", "theme", args.theme, "-e", "photo", args.photo,
                         "-e", "width", str(args.width), "-e", "height", str(args.height), "-e", "durationMs", str(args.duration_ms),
                         "-e", "repeats", str(args.repeats), "-e", "label", label]
            if args.profile:
                cmd += ["-e", "composeTrace", "true", "-e", "profileBarrier", "true"]
        if args.attribution and (not chrome or CHROME_WORKLOADS[motion][0].startswith("AndroidCanvasBar")):
            cmd += ["-e", "composeTrace", "true"]
        if max(args.width, args.height) > 8192:
            cmd += ["-e", "openQueryPhoto", "true"]
        for item in filter(None, args.extra.split(",")):
            key, value = item.split("=", 1)
            cmd += ["-e", key, value]
        cmd += [f"{args.package}/art.capycanvas.UiStartupInstrumentation" if (args.workspace_workload and args.workspace_runner == "self") or (not args.workspace_workload and args.viewport_runner == "self")
                else f"{args.package}.test/androidx.test.runner.AndroidJUnitRunner"]
        trace = f"/data/misc/perfetto-traces/{args.package}-{label}.perfetto-trace"
        session = f"{args.package}-{label}"
        if args.adaptive_script:
            token = uuid.uuid4().hex
            cmd[-1:-1] = ["-e", "waitForGo", "true", "-e", "benchmarkReadyToken", token]
            subprocess.run(adb + ["shell", "rm", "-f", f"{remote}/ready", f"{remote}/go", f"{remote}/refresh-before-primer.json"], check=True, capture_output=True)
        if args.profile:
            subprocess.run(adb + ["shell", "rm", "-f", f"{remote}/{label}-profile-ready", f"{remote}/{label}-profile-go"], check=True, capture_output=True)
        subprocess.run(adb + ["shell", "perfetto", "--txt", "-c", "-", "-o", trace, f"--detach={session}"],
                       input=config, text=True, check=True, capture_output=True)
        with cleanup_steps(
            lambda: subprocess.run(adb + ["shell", "perfetto", f"--attach={session}", "--stop"], capture_output=True),
            lambda: subprocess.run(adb + ["pull", f"{remote}/refresh-before-primer.json", str(args.output)], check=True, capture_output=True) if args.adaptive_script else None,
            lambda: restore_adaptive(adb, args.package, args.adaptive_script, args.output, args.adaptive_label, args.adaptive_apk, args.adaptive_aapt) if args.adaptive_script else None,
        ):
            time.sleep(1)
            print(f"Running {label}; instrumentation log streams to {args.output}", flush=True)
            with (args.output / f"{label}-instrument.log").open("w") as stream:
                if not args.adaptive_script and not args.profile:
                    subprocess.run(cmd, stdout=stream, stderr=subprocess.STDOUT, text=True, timeout=budget)
                else:
                    process = subprocess.Popen(cmd, stdout=stream, stderr=subprocess.STDOUT, text=True)
                    profile = None
                    perf = f"/data/local/tmp/{args.package}-{label}.perf.data"
                    ready_file, go_file = ("ready", "go") if args.adaptive_script else (f"{label}-profile-ready", f"{label}-profile-go")
                    with cleanup_steps(
                        lambda: subprocess.run(adb + ["shell", "am", "force-stop", args.package], check=True, capture_output=True) if process.poll() is None else None,
                        lambda: process.wait(timeout=30) if process.poll() is None else None,
                        lambda: profile.wait(timeout=15) if profile is not None and profile.poll() is None else None,
                    ):
                        deadline = time.monotonic() + budget
                        while process.poll() is None:
                            if time.monotonic() >= deadline:
                                raise TimeoutError("Fixture readiness exceeded capture budget")
                            if subprocess.run(adb + ["shell", "test", "-f", f"{remote}/{ready_file}"], capture_output=True).returncode == 0:
                                if args.adaptive_script:
                                    enable_adaptive(adb, args.package, args.adaptive_script, args.output, args.adaptive_label, args.adaptive_apk, args.adaptive_aapt, remote, token)
                                if args.profile:
                                    profile_log = (args.output / "simpleperf.log").open("w")
                                    try:
                                        profile = subprocess.Popen(adb + ["shell", "simpleperf", "record", "--app", args.package,
                                            "-e", "cpu-clock", "-f", "199", "--call-graph", "dwarf,16384", "--duration", "8", "-o", perf], stdout=profile_log, stderr=subprocess.STDOUT)
                                        time.sleep(1)
                                        if profile.poll() is not None and profile.returncode:
                                            raise RuntimeError("App-only simpleperf refused sampling; see simpleperf.log")
                                    finally:
                                        profile_log.close()
                                subprocess.run(adb + ["shell", "touch", f"{remote}/{go_file}"], check=True, capture_output=True)
                                if args.profile:
                                    profile.wait(timeout=15)
                                    if profile.returncode:
                                        raise RuntimeError("App-only simpleperf failed; see simpleperf.log")
                                    subprocess.run(adb + ["pull", perf, str(args.output / "perf.data")], check=True, capture_output=True)
                                    subprocess.run(adb + ["shell", "rm", "-f", perf], check=True, capture_output=True)
                                break
                            time.sleep(.1)
                        process.wait(timeout=max(1, deadline - time.monotonic()))
            log = (args.output / f"{label}-instrument.log").read_text()
        (args.output / f"{label}-instrument.log").write_text(log)
        local = args.output / f"{label}.perfetto-trace"
        subprocess.run(adb + ["pull", trace, str(local)], check=True, capture_output=True)
        subprocess.run(adb + ["shell", "rm", "-f", trace], capture_output=True)
        complete = f"WORKSPACE_COMPLETE {label}" if args.workspace_workload and args.workspace_runner == "self" else f"VIEWPORT_COMPLETE {label}" if not args.workspace_workload and args.viewport_runner == "self" else "OK (1 test)"
        reports = chrome_reports(motion, args.repeats, selected) if chrome else workspace_reports(motion, label, args.repeats, extra.get("benchmarkMode")) if args.workspace_workload else [f"{label}-{suffix}.json" for suffix in ["info"] + [str(i) for i in range(args.repeats)]]
        primers = workspace_reports(motion, label, args.repeats, extra.get("benchmarkMode"), warmup=True) if args.workspace_workload and not chrome else []
        interactions = chrome_interactions(motion, args.repeats, selected) if chrome else []
        if complete not in log:
            candidates = reports + primers + interactions + (["ui-failure-state.json"] if chrome and CHROME_WORKLOADS[motion][0].startswith("AndroidCanvasBar") else []) + (["readiness-failure.json"] if motion == "switcher-scroll" else [])
            retained = retain_failed_reports(adb, remote, args.output, candidates)
            summary[motion] = {"error": log[-2000:], "instrumentation_passed": False, "qualification": False,
                               "retained_reports": retained}
            continue
        for filename in reports + primers:
            subprocess.run(adb + ["pull", f"{remote}/{filename}", str(args.output)], check=True, capture_output=True)
        shown = presents(args.processor, local, args.package, bool(args.workspace_workload))
        if args.workspace_workload:
            captures = args.output / "captures"
            captures.mkdir(exist_ok=True)
            if motion == "gap":
                keys = f"{label}-keyframes.json"
                subprocess.run(adb + ["pull", f"{remote}/{keys}", str(captures / keys)], check=True, capture_output=True)
                names = [row["capture"] for row in json.loads((captures / keys).read_text())]
            elif motion in ["panel", "column"]:
                names = [f"{label}-panel.png"]
            else:
                names = []
            for name in names:
                subprocess.run(adb + ["pull", f"{remote}/{name}", str(captures / name)], check=True, capture_output=True)
            runs = []
            for filename in reports:
                raw = json.loads((args.output / filename).read_text())
                if args.numeric_resolver and raw.get("scalar_draw_samples") is not None:
                    replay_numeric(raw, args.numeric_resolver)
                    (args.output / filename).write_text(json.dumps(raw, indent=2) + "\n")
                result = chrome_summary(raw, shown) if chrome or motion == "column" else workspace_summary(args.processor, local, raw, shown, args.profile)
                result.update({key: raw[key] for key in ["mode", "mouse"] if key in raw})
                runs.append(result)
            summary[motion] = {"theme": args.theme, "runs": runs}
            print(motion, json.dumps(summary[motion]), flush=True)
            continue
        info = json.loads((args.output / f"{label}-info.json").read_text())
        runs = [summarize(json.loads((args.output / f"{label}-{i}.json").read_text()), shown) for i in range(args.repeats)]
        summary[motion] = {"zoom": info["state"]["camera"]["zoom"], "work_area": info["state"]["camera"]["work_area"],
                           "glass_regions": info["display"].get("glass_regions"), "thermal_status": info.get("thermal_status"),
                           "runs": runs}
        print(motion, json.dumps(summary[motion]), flush=True)
    write_run_summary(args.output / f"{args.label}-summary.json", summary, capabilities if args.attribution else None)


if __name__ == "__main__":
    main()
