#!/usr/bin/env python3
"""Summarize local Apple JSONL traces; no external packages or uploads.

Receipts associated with frames do not prove input pixels reached the screen.
GPU queue spans include submission gaps; they are not isolated GPU busy time.
"""
import argparse
import collections
import json
import math
from pathlib import Path

NS_PER_MS = 1_000_000
BUDGET_NS = 1_000_000_000 / 120


def distribution(values, divisor=NS_PER_MS):
    values = sorted(values)
    if not values:
        return {"count": 0, "p50": None, "p95": None, "p99": None, "max": None}
    def percentile(fraction):
        index = (len(values) - 1) * fraction
        lower = math.floor(index)
        return (values[lower] + (values[math.ceil(index)] - values[lower]) * (index - lower)) / divisor
    return {"count": len(values), "p50": percentile(.5), "p95": percentile(.95),
            "p99": percentile(.99), "max": values[-1] / divisor}


def admission_denials(records):
    reasons = {1: "inactive", 2: "owner_pending", 3: "drawable_capacity"}
    counts = collections.Counter(reasons.get(r[3], "unclassified") for r in records if not r[2])
    return {name: counts[name] for name in [*reasons.values(), "unclassified"]}


def analyze(header, events, target_hz=120):
    if header.get("schema") != 1:
        raise ValueError("Unsupported trace schema")
    if not math.isfinite(target_hz) or not 0 < target_hz <= 1000:
        raise ValueError("Target refresh rate must be finite and between 0 and 1000 Hz")
    target_budget_ns = 1_000_000_000 / target_hz
    grouped = collections.defaultdict(list)
    for event in events:
        if len(event) != 11 or not all(isinstance(x, int) and x >= 0 for x in event):
            raise ValueError("Invalid trace record")
        grouped[event[0]].append(event[1:])
    frames = {r[0]: r for r in grouped[1]}
    inputs = {r[0]: r for r in grouped[2]}
    drawables = {(r[0], r[3]) for r in grouped[3] if r[4]}
    presented = {(r[0], r[3]): r for r in grouped[4]}
    visible = {key: r for key, r in presented.items() if r[1] > 0}
    gpu = {r[0]: r for r in grouped[7]}
    schedules = {r[0]: r for r in grouped[12]}
    submitted = [r for r in frames.values() if r[6] > 0]
    ready_states = [r[0] for r in grouped[9] if r[2] & 11 == 11 and not r[3]]
    ready_at = min(ready_states, default=None)
    state_errors = sum(bool(r[3]) for r in grouped[9])
    stats = sorted(grouped[8], key=lambda r: r[0])
    last_stats = stats[-1] if stats else [0] * 10
    displays = sorted({(r[1], r[2], r[3] / 1000, r[4], r[5]) for r in grouped[6]})
    memory = sorted((r for r in grouped[5] if r[4] == 0), key=lambda r: r[0])
    times = sorted(r[1] for r in visible.values())
    ticks = grouped[0]
    activity = sorted(grouped[10], key=lambda r: r[0])
    cycles = {}
    active, cycle, cursor = False, 0, 0
    for frame in sorted(frames):
        while cursor < len(activity) and activity[cursor][0] <= frame:
            next_active = bool(activity[cursor][1])
            if next_active and not active:
                cycle += 1
            active = next_active
            cursor += 1
        cycles[frame] = cycle if active else None
    shown = sorted(visible.values(), key=lambda r: r[1])
    def active_intervals(records):
        return [right[1] - left[1] for left, right in zip(records, records[1:])
                if cycles.get(left[0]) is not None and cycles.get(left[0]) == cycles.get(right[0])]
    continuous_intervals = active_intervals(shown)
    associations = {}
    correction_associations = {}
    for event in visible.values():
        frame = frames.get(event[0])
        source = inputs.get(frame[9]) if frame else None
        if source and source[6] in (0, 2) and source[9] and event[1] >= source[0]:
            target = correction_associations if source[6] == 2 else associations
            target[source[0]] = min(target.get(source[0], math.inf), event[1] - source[0])
    def frame_metrics(records):
        return {
            "owner_queue_age_ms": distribution(r[2] - r[0] for r in records if r[2] >= r[0]),
            "owner_service_ms": distribution(r[3] - r[2] for r in records if r[3] >= r[2]),
            "cpu_stages_ms": {name: distribution(r[index] for r in records)
                              for index, name in enumerate(["prepare", "acquire", "viewport", "present_call", "poll"], start=4)},
            "owner_service_over_8_33ms": sum(r[3] - r[2] > BUDGET_NS for r in records),
            "owner_service_over_target_budget": sum(r[3] - r[2] > target_budget_ns for r in records),
        }
    def schedule_metrics(records):
        recorded = [schedules[r[0]] for r in records if r[0] in schedules]
        deadlines = [(r, schedules[r[0]][1]) for r in records if r[0] in schedules and schedules[r[0]][1]]
        return {
            "legacy_frames": sum(r[3] == 0 for r in recorded),
            "supplied_drawables_accepted": sum(r[3] == 1 for r in recorded),
            "stale_drawables_rejected": sum(r[3] == 2 for r in recorded),
            # The owner returns after polling and snapshot publication. This is
            # an upper bound, not the timestamp of the actual Metal present call.
            "owner_completion_after_commit_deadline_ms": distribution(max(0, r[3] - deadline) for r, deadline in deadlines),
            "owners_completing_after_commit_deadline": sum(r[3] > deadline for r, deadline in deadlines),
            "presentation_target_after_commit_deadline_ms": distribution(r[2] - r[1] for r in recorded if r[1] and r[2] >= r[1]),
        }
    visible_frames = [(r, frames[r[0]]) for r in visible.values() if r[0] in frames]
    warnings = []
    if header.get("configuration") != "release":
        warnings.append("Debug build; not performance acceptance evidence.")
    if header.get("dropped_records", 0):
        warnings.append("Recorder overflow; distributions may be biased.")
    if not visible:
        warnings.append("No actual drawable presentations observed.")
    if not activity:
        warnings.append("No display-link activity records; idle-separated cadence cannot be measured.")
    if drawables - presented.keys():
        warnings.append("Acquired drawables lack completion callbacks.")
    gpu_timing_requested = header.get("gpu_timing_requested", True)
    if not gpu_timing_requested:
        warnings.append("GPU timing intentionally disabled; this trace measures CPU and presentation only.")
        if gpu or last_stats[2]:
            warnings.append("GPU observations contradict the disabled instrumentation setting.")
    elif last_stats[1] != 1:
        warnings.append("GPU timestamps are unavailable or not initialized.")
    zero_gpu = sum(r[2] == 1 and r[1] == 0 for r in gpu.values())
    if last_stats[3] or last_stats[4] or last_stats[5] or any(r[6] for r in stats) or zero_gpu:
        warnings.append("GPU observations include skipped, invalid, pending or failed polls.")
    if not displays or all(d[3] < target_hz for d in displays):
        warnings.append(f"No observed display configuration advertises {target_hz:g} Hz.")
    if ready_at is None:
        warnings.append("No fully ready canvas/shaders/filter catalog state observed.")
    warnings.extend(["Input receipt association does not establish physical input-to-pixel latency.",
                     "GPU queue spans include submission gaps and profiler submissions; overhead is not calibrated.",
                     "This trace alone does not establish the required workload matrix or ten-minute acceptance."])
    result = {
        "schema": 1, "platform": "iPadOS" if header["platform"] == 0 else "macOS",
        "input_source": header.get("input_source", "platform"),
        "gpu_timing_requested": gpu_timing_requested,
        "evaluation": {"target_hz": target_hz, "frame_budget_ms": target_budget_ns / NS_PER_MS,
                       "cadence_tolerance_percent": 5},
        "configuration": header.get("configuration"), "duration_seconds": header["duration_seconds"],
        "counts": {"events": len(events), "dropped_records": header.get("dropped_records", 0),
                   "real_input_batches": sum(r[6] == 0 for r in inputs.values()),
                   "predicted_input_batches": sum(r[6] == 1 for r in inputs.values()),
                   "correction_input_batches": sum(r[6] == 2 for r in inputs.values()),
                   "ticks": len(ticks), "ticks_denied_admission": sum(not r[2] for r in ticks),
                   "ticks_denied_by_reason": admission_denials(ticks),
                   "frames": len(frames), "viewport_submissions": len(submitted), "acquired_drawables": len(drawables),
                   "presented_drawables": len(visible), "zero_time_presentations": sum(not r[1] for r in presented.values()),
                   "missing_presentation_callbacks": len(drawables - presented.keys()),
                   "duplicate_frame_records": len(grouped[1]) - len(frames),
                   "gpu_requests": last_stats[2], "gpu_skipped": last_stats[3], "gpu_invalid": last_stats[4],
                   "gpu_false_zero_samples": zero_gpu,
                   "gpu_pending_at_last_poll": last_stats[5], "gpu_poll_errors": sum(bool(r[6]) for r in stats),
                   "frames_without_gpu_sample": sum(r[0] not in gpu for r in submitted), "frame_errors": state_errors},
        "display_configurations": [{"pixels": list(d[:2]), "scale": d[2], "maximum_hz": d[3],
                                    "metal_preferred_frame_latency": d[4] or None} for d in displays],
        "recorder_reserved_bytes": header["capacity"] * header["record_stride_bytes"],
        "ready_seconds_from_start": None if ready_at is None else (ready_at - header["started_ns"]) / 1e9,
        "all_submitted_frames": frame_metrics(submitted),
        "display_scheduling": schedule_metrics(list(frames.values())),
        "frames_after_readiness": frame_metrics([r for r in submitted if ready_at is not None and r[0] > ready_at]),
        "gpu_queue_span_ms": distribution(r[1] for r in gpu.values() if r[2] == 1 and r[1] > 0),
        "presentation": {
            "frame_admission_to_present_ms": distribution(r[1] - f[0] for r, f in visible_frames if r[1] >= f[0]),
            "all_intervals_including_idle_ms": distribution(b - a for a, b in zip(times, times[1:])),
            "continuous_active_intervals_ms": distribution(continuous_intervals),
            "continuous_intervals_over_120hz_budget": sum(v > BUDGET_NS * 1.05 for v in continuous_intervals),
            "continuous_intervals_over_target_budget": sum(v > target_budget_ns * 1.05 for v in continuous_intervals),
            "positive_target_lateness_ms": distribution(max(0, r[1] - f[1]) for r, f in visible_frames if f[1]),
            "targets_exceeded_by_over_1ms": sum(r[1] > f[1] + NS_PER_MS for r, f in visible_frames if f[1]),
            "first_associated_present_per_owner_receipt_proxy_ms": distribution(associations.values()),
            "first_associated_present_per_correction_receipt_proxy_ms": distribution(correction_associations.values()),
        },
        "input_owner_queue_ms": distribution(r[1] - r[0] for r in inputs.values() if r[1] >= r[0]),
        "correction_owner_queue_ms": distribution(r[1] - r[0] for r in inputs.values() if r[6] == 2 and r[1] >= r[0]),
        "memory": {"samples": len(memory), "footprint_bytes": distribution([r[1] for r in memory], divisor=1),
                   "first_to_last_growth_bytes": memory[-1][1] - memory[0][1] if memory else None,
                   "thermal_states": sorted({r[3] for r in memory})},
        "warnings": warnings,
    }
    if header.get("workload"):
        markers = sorted(grouped[11], key=lambda r: r[0])
        begins = [r for r in markers if r[1] == 2]
        ends = [r for r in markers if r[1] == 3]
        failed = any(r[1] == 5 for r in markers)
        complete = len(begins) == len(ends) == 1 and not failed and ends[0][0] > begins[0][0]
        report = {"specification": header["workload"], "measurement_completed": complete,
                  "postlude_observed": any(r[1] == 4 for r in markers), "failure_recorded": failed}
        if complete:
            begin, end = begins[0], ends[0]
            admitted = [r for r in frames.values() if begin[0] <= r[0] < end[0]]
            rows = [r for r in submitted if begin[0] <= r[0] < end[0]]
            identities = {r[0] for r in rows}
            shown_rows = sorted((r for r in visible.values() if r[0] in identities), key=lambda r: r[1])
            measured_intervals = active_intervals(shown_rows)
            acquired = {key for key in drawables if begin[0] <= key[0] < end[0]}
            observed_times = sorted(r[1] for r in visible.values() if begin[0] <= r[1] <= end[0])
            measured_ticks = [r for r in ticks if begin[0] <= r[0] < end[0]]
            memory_rows = [r for r in memory if begin[0] <= r[0] <= end[0]]
            scheduler = [r[5] for r in markers if r[1] in (3, 6) and begin[0] <= r[0] <= end[0]]
            report.update({
                "measured_seconds": (end[0] - begin[0]) / 1e9,
                "delivered_nonpredicted_samples": end[3] - begin[3],
                "delivered_nonpredicted_batches": end[4] - begin[4],
                "rejected_input_batches": sum(not r[9] for r in inputs.values() if begin[0] <= r[0] <= end[0]),
                "ticks": len(measured_ticks),
                "ticks_denied_admission": sum(not r[2] for r in measured_ticks),
                "ticks_denied_by_reason": admission_denials(measured_ticks),
                "admitted_frames_without_viewport": len(admitted) - len(rows),
                "missing_presentation_callbacks": len(acquired - presented.keys()),
                "zero_time_presentations": sum(key in acquired and not r[1] for key, r in presented.items()),
                # Include the edges: good cadence among a few early frames must
                # not conceal a window becoming occluded for the rest of a run.
                "first_presentation_after_start_ms": (observed_times[0] - begin[0]) / NS_PER_MS if observed_times else None,
                "last_presentation_before_end_ms": (end[0] - observed_times[-1]) / NS_PER_MS if observed_times else None,
                "producer_interval_maximum_lateness_ms": distribution(scheduler),
                "frames": frame_metrics(rows),
                "display_scheduling": schedule_metrics(admitted),
                "gpu_queue_span_ms": distribution(r[1] for key, r in gpu.items() if key in identities and r[2] == 1 and r[1] > 0),
                "gpu_samples_missing": sum(key not in gpu for key in identities),
                "actual_presentations": len(shown_rows),
                "frame_admission_to_present_ms": distribution(r[1] - r[0] for r in shown_rows if r[1] >= r[0]),
                # The fixed workload includes explicit pen-up gaps. Preserve
                # all intervals as well as the display-link-cycle metric above.
                "presentation_intervals_including_pen_up_ms": distribution(b[1] - a[1] for a, b in zip(shown_rows, shown_rows[1:])),
                "continuous_active_intervals_ms": distribution(measured_intervals),
                "continuous_intervals_over_target_budget": sum(v > target_budget_ns * 1.05 for v in measured_intervals),
                "positive_target_lateness_ms": distribution(max(0, r[1] - frames[r[0]][1]) for r in shown_rows),
                "targets_exceeded_by_over_1ms": sum(r[1] > frames[r[0]][1] + NS_PER_MS for r in shown_rows),
                "footprint_bytes": distribution((r[1] for r in memory_rows), divisor=1),
                "first_to_last_growth_bytes": memory_rows[-1][1] - memory_rows[0][1] if memory_rows else None,
                "thermal_states": sorted({r[3] for r in memory_rows}),
            })
            requested = header["workload"].get("measurement_seconds")
            if requested is not None and report["measured_seconds"] < requested:
                warnings.append("Workload measurement is shorter than its declared duration.")
            if ready_at is None or ready_at > begin[0]:
                warnings.append("Workload measurement began before recorded full readiness.")
            if report["rejected_input_batches"] or state_errors or not shown_rows:
                warnings.append("Workload input/render validation failed or actual presentations are missing.")
        else:
            warnings.append("Synthetic workload has no complete measurement interval; do not treat it as a successful run.")
        result["workload"] = report
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-hz", type=float, default=120,
                        help="Refresh rate to evaluate (default: 120; use 90 for the current Mac display)")
    args = parser.parse_args()
    with args.trace.open() as source:
        header = json.loads(next(source))
        events = [json.loads(line) for line in source if line.strip()]
    try:
        report = analyze(header, events, target_hz=args.target_hz)
    except ValueError as error:
        parser.error(str(error))
    result = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(result)
    else:
        print(result, end="")


if __name__ == "__main__":
    main()
