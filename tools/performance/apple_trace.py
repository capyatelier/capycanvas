#!/usr/bin/env python3
"""Summarize local Apple JSONL traces; no external packages or uploads.

Receipts associated with frames do not prove input pixels reached the screen.
GPU queue spans include submission gaps; they are not isolated GPU busy time.
"""
import argparse
import bisect
import collections
import json
import math
from pathlib import Path
from android_brush_metrics import completion_window, input_completions

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
    reasons = {1: "inactive", 2: "owner_pending"}
    counts = collections.Counter(reasons.get(r[3], "unclassified") for r in records if not r[2])
    return {name: counts[name] for name in [*reasons.values(), "unclassified"]}


def calibrate_gpu(clocks, samples):
    """Bracket device timestamps with paired Metal CPU/GPU clock samples.

    Metal CPU time is nanoseconds, but its epoch need not match the recorder.
    Each call's recorder bounds constrain that offset. Interpolate within two
    samples only; these bounds cover sampling latency, not unknown clock drift.
    """
    clocks = sorted(clocks, key=lambda r: r[0])
    invalid = sum(not r[1] or not r[2] or r[3] < r[0] for r in clocks)
    discontinuities = sum(b[0] <= a[0] or b[1] <= a[1] or b[2] <= a[2]
                          for a, b in zip(clocks, clocks[1:]))
    diagnostics = {"samples": len(clocks), "invalid_samples": invalid,
                   "discontinuities": discontinuities,
                   "sample_call_ms": distribution(r[3] - r[0] for r in clocks if r[3] >= r[0])}
    if len(clocks) < 2 or invalid or discontinuities:
        return {}, diagnostics
    ticks = [r[2] for r in clocks]
    def convert(tick):
        if tick < ticks[0] or tick > ticks[-1]:
            return None
        index = min(bisect.bisect_right(ticks, tick), len(ticks) - 1)
        left, right = clocks[index - 1], clocks[index]
        fraction = (tick - left[2]) / (right[2] - left[2])
        cpu = left[1] + fraction * (right[1] - left[1])
        return tuple(cpu + (left[i] - left[1]) + fraction *
                     ((right[i] - right[1]) - (left[i] - left[1])) for i in (0, 3))
    calibrated = {}
    for sample in samples.values():
        if sample[2] != 1 or not sample[1] or not 0 < sample[3] < sample[4]:
            continue
        start, end = convert(sample[3]), convert(sample[4])
        if start is not None and end is not None:
            calibrated[sample[0]] = (start, end)
    return calibrated, diagnostics


def consumed_paint_progress(grouped, begin=None, end=None):
    """Associate engine-consumed timestamps with actual drawable presentations."""
    def scoped(time):
        return (begin is None or time >= begin) and (end is None or time < end)
    def presented_in_scope(time):
        return (begin is None or time >= begin) and (end is None or time <= end)
    frames, samples, callbacks = (collections.defaultdict(list) for _ in range(3))
    for row in grouped[1]:
        frames[row[0]].append(row)
    for row in grouped[15]:
        samples[row[0]].append(row)
    for row in grouped[4]:
        callbacks[(row[0], row[3])].append(row)
    acquired = {(r[0], r[3]) for r in grouped[3] if r[4] and scoped(r[0])}
    counts = collections.Counter()
    valid, advancing = {}, set()
    high_water = 0
    for admission in sorted(frames.keys() | samples.keys()):
        frame_rows, counter_rows = frames[admission], samples[admission]
        reasons = []
        if len(frame_rows) != 1:
            reasons.append("missing_frame" if not frame_rows else "duplicate_frame")
        if len(counter_rows) != 1:
            reasons.append("missing_counter" if not counter_rows else "duplicate_counter")
        value = counter_rows[0][1] if len(counter_rows) == 1 else None
        if value is not None:
            if not value:
                reasons.append("zero_counter")
            elif len(frame_rows) == 1 and value > frame_rows[0][3]:
                reasons.append("future_counter")
            elif value < high_water:
                reasons.append("regressing_counter")
            elif value == high_water:
                reasons.append("repeated_counter")
            elif len(frame_rows) == 1:
                high_water = value
                if scoped(admission):
                    advancing.add(value)
            if len(frame_rows) == 1 and value and not any(
                    reason in reasons for reason in ("future_counter", "regressing_counter")):
                valid[admission] = value
        if scoped(admission):
            counts.update(reasons)
            counts["frame_admissions"] += bool(frame_rows)
            counts["counter_records"] += len(counter_rows)
    candidates = []
    for key, records in sorted(callbacks.items()):
        admission, drawable = key
        if not scoped(admission):
            counts["presentations_from_outside_frame_interval"] += sum(
                r[1] > 0 and presented_in_scope(r[1]) for r in records)
            continue
        counts["presentation_callback_records"] += len(records)
        counts["duplicate_presentation_records"] += len(records) - 1
        counts["zero_time_presentations"] += sum(not r[1] for r in records)
        times = {r[1] for r in records}
        if len(times) != 1:
            counts["conflicting_presentation_identities"] += 1
        presented = records[0][1]
        if admission not in frames:
            counts["presentations_without_frame"] += 1
        if admission not in valid:
            counts["presentations_without_valid_counter"] += 1
        elif presented and (presented < admission or presented < valid[admission]):
            counts["noncausal_presentations"] += 1
        elif presented and not presented_in_scope(presented):
            counts["presentations_outside_interval"] += 1
        elif presented and len(times) == 1:
            candidates.append((presented, admission, drawable, valid[admission]))
    counts["missing_presentation_callbacks"] = len(acquired - callbacks.keys())
    progress, shown_high_water = [], 0
    for presented, admission, drawable, value in sorted(candidates):
        reason = None
        if value <= shown_high_water or value not in advancing:
            reason = "repeated_presented_counter" if value >= shown_high_water else "regressing_presented_counter"
            counts[reason] += 1
        else:
            progress.append((presented, value))
        shown_high_water = max(shown_high_water, value)
    names = ("frame_admissions", "counter_records", "missing_frame", "duplicate_frame", "missing_counter",
             "duplicate_counter", "zero_counter", "future_counter", "regressing_counter", "repeated_counter",
             "presentation_callback_records", "duplicate_presentation_records", "zero_time_presentations",
             "conflicting_presentation_identities", "presentations_without_frame", "presentations_without_valid_counter",
             "noncausal_presentations", "presentations_outside_interval", "presentations_from_outside_frame_interval",
             "missing_presentation_callbacks", "repeated_presented_counter", "regressing_presented_counter")
    return {"availability": "observed" if grouped[15] else "unavailable",
            "counts": {name: counts[name] for name in names},
            "distinct_advancing_consumed_timestamps": len(advancing),
            "presented_engine_consumed_progress": len(progress),
            "consumed_timestamp_to_present_ms": distribution(presented - value for presented, value in progress),
            "presented_progress_intervals_ms": distribution(b[0] - a[0] for a, b in zip(progress, progress[1:])),
            "scope": "Engine-consumed Down/Move timestamp progress associated with actual presented callbacks; not physical input-to-pixel latency or guaranteed fresh pixels"}


def drawable_presentation(frame, acquired, callbacks):
    if len(acquired) != 1:
        return None, "ambiguous_drawable"
    key = (frame[0], acquired[0][3])
    if len(callbacks[key]) != 1:
        return None, "missing_or_duplicate_callback"
    presented = callbacks[key][0][1]
    if not presented:
        return None, "zero_presentation"
    if presented <= frame[2]:
        return None, "noncausal_presentation"
    return (key[1], presented), None


def encoded_camera_presentations(header, grouped):
    if header.get("encoded_camera_revision") is not True:
        return {"availability": "unavailable", "reason": "No encoded camera revision metadata", "presentations": []}
    frames, metadata, acquired, callbacks = (collections.defaultdict(list) for _ in range(4))
    for row in grouped[1]: frames[row[0]].append(row)
    for row in grouped[15]: metadata[row[0]].append(row)
    for row in grouped[3]:
        if row[4]: acquired[row[0]].append(row)
    for row in grouped[4]: callbacks[(row[0], row[3])].append(row)
    errors, counts, presentations = collections.Counter(), collections.Counter(), []
    for identity in sorted(frames.keys() | metadata.keys()):
        if len(frames[identity]) != 1 or len(metadata[identity]) != 1:
            errors["ambiguous_frame_or_camera_metadata"] += 1
            continue
        frame, camera = frames[identity][0], metadata[identity][0]
        revision, outcome = camera[3:5]
        if type(revision) is not int or revision < 0 or type(outcome) is not int or outcome not in (1, 2):
            errors["invalid_camera_metadata"] += 1
            continue
        counts["valid_frame_metadata"] += 1
        if not acquired[identity]:
            counts["metadata_without_drawable"] += 1
            continue
        presentation, error = drawable_presentation(frame, acquired[identity], callbacks)
        if error:
            errors[error] += 1
            continue
        drawable, presented = presentation
        callback = callbacks[(identity, drawable)][0]
        if presented < acquired[identity][0][2] or callback[2] < presented:
            errors["noncausal_presentation"] += 1
            continue
        presentations.append({"frame_id": identity, "owner_begin_ns": frame[2],
            "drawable_id": drawable, "presented_ns": presented, "camera_revision": revision, "native_frame_result": outcome - 1})
    times = collections.Counter(row["presented_ns"] for row in presentations)
    duplicate_times = sum(count - 1 for count in times.values() if count > 1)
    if duplicate_times: errors["nonunique_presentation_time"] = duplicate_times
    presentations = [row for row in presentations if times[row["presented_ns"]] == 1]
    joined = {(r["frame_id"], r["drawable_id"]) for r in presentations}
    counts["callbacks_without_camera_join"] = sum(len(rows) for key, rows in callbacks.items() if key not in joined)
    if counts["callbacks_without_camera_join"]:
        errors["unjoined_presentation_callbacks"] = counts["callbacks_without_camera_join"]
    availability = "invalid" if errors or header.get("dropped_records") else "observed" if presentations else "unavailable"
    return {"availability": availability, "reason": None if presentations else "No actual encoded-camera presentations",
            "errors": dict(errors), "counts": dict(counts), "dropped_records": header.get("dropped_records", 0),
            "presentations": sorted(presentations, key=lambda row: row["presented_ns"]),
            "scope": "Encoded canvas camera state identity joined to actual drawable presentation; revision changes alone do not establish visible motion"}


def camera_contacts(header, camera_report, witness):
    if header.get("encoded_camera_revision") is not True:
        return {"availability": "unavailable", "reason": camera_report["reason"], "contacts": []}
    def invalid(reason):
        return {"availability": "invalid", "errors": {reason: 1}, "contacts": []}
    if not isinstance(witness, dict):
        return invalid("invalid_camera_witness")
    if (type(witness.get("process_identifier")) is not int or witness["process_identifier"] <= 0
            or witness["process_identifier"] != header.get("process_identifier")):
        return invalid("witness_process_mismatch")
    if witness.get("clock") != "CACurrentMediaTime nanoseconds" or witness["clock"] != header.get("clock"):
        return invalid("witness_clock_mismatch")
    if not isinstance(witness.get("poses"), list) or not isinstance(witness.get("contacts"), list) or not witness["contacts"]:
        return invalid("missing_poses_or_contacts")
    def integer(value):
        return type(value) is int and value >= 0
    def number(value):
        return type(value) in (int, float) and math.isfinite(value)
    def geometry(camera):
        viewport, translation, flipped = (camera.get(key) for key in ("viewport", "translation", "flipped"))
        if (not isinstance(viewport, list) or len(viewport) != 2 or not all(integer(v) and v > 0 for v in viewport)
                or not isinstance(translation, list) or len(translation) != 2 or not all(map(number, translation))
                or not isinstance(flipped, list) or len(flipped) != 2 or not all(type(v) is bool for v in flipped)
                or not number(camera.get("zoom")) or camera["zoom"] <= 0 or not number(camera.get("rotation"))):
            return None
        return (tuple(viewport), tuple(translation), camera["zoom"], camera["rotation"], tuple(flipped))
    poses, pose_times, conflicts = {}, collections.defaultdict(list), set()
    errors, previous_time = collections.Counter(), 0
    for pose in witness["poses"]:
        if not isinstance(pose, dict) or not isinstance(pose.get("camera"), dict):
            errors["invalid_pose"] += 1
            continue
        camera = pose.get("camera", {})
        drawing, time, revision = pose.get("drawing_identity"), pose.get("time_ns"), camera.get("revision")
        value = geometry(camera)
        if not isinstance(drawing, str) or not drawing or not integer(time) or not integer(revision) or value is None:
            errors["invalid_pose"] += 1
            continue
        if time < previous_time: errors["nonmonotonic_pose_time"] += 1
        previous_time = time
        key = drawing, revision
        if key in poses and poses[key] != value:
            conflicts.add(key)
            errors["conflicting_revision_geometry"] += 1
        poses[key] = value
        pose_times[key].append(time)
    results, previous_end, identifiers = [], 0, set()
    for contact in witness["contacts"]:
        if not isinstance(contact, dict):
            errors["invalid_or_overlapping_contact"] += 1
            continue
        begin, end = contact.get("full_contact_begin_ns"), contact.get("full_contact_end_ns")
        drawing, revision, identity = (contact.get(key) for key in ("drawing_identity", "baseline_revision", "id"))
        if (not isinstance(identity, str) or not identity or identity in identifiers
                or not isinstance(drawing, str) or not drawing or not integer(revision)
                or not integer(begin) or not integer(end) or not 0 < begin < end or begin < previous_end):
            errors["invalid_or_overlapping_contact"] += 1
            continue
        previous_end = end
        identifiers.add(identity)
        if begin < header["started_ns"] or end > header["started_ns"] + header["duration_seconds"] * 1e9:
            errors["contact_outside_recording"] += 1
        if any(pose.get("drawing_identity") != drawing and begin <= pose.get("time_ns", -1) <= end
               for pose in witness["poses"] if isinstance(pose, dict) and integer(pose.get("time_ns"))):
            errors["drawing_changed_during_contact"] += 1
        baseline_key = drawing, revision
        baseline = poses.get(baseline_key) if baseline_key not in conflicts else None
        baseline_observed = baseline is not None and any(t <= begin for t in pose_times[baseline_key])
        if not baseline_observed: errors["unknown_contact_baseline"] += 1
        shown = [row for row in camera_report["presentations"] if begin <= row["presented_ns"] <= end]
        times = [row["presented_ns"] for row in shown]
        unknown, different, unchanged, prior_frames = collections.Counter(), [], 0, 0
        progress, onset, successive_changes = [], [], 0
        static, same_geometry_new_revision, late_geometry, unknown_comparisons = 0, 0, 0, 0
        previous_geometry, previous_revision, previous_timely = baseline, revision, baseline_observed
        for row in shown:
            key = drawing, row["camera_revision"]
            value = poses.get(key)
            reason = ("conflicting_revision_geometry" if key in conflicts else "unmapped_revision" if value is None
                      else "unknown_contact_baseline" if not baseline_observed else None)
            if reason:
                unknown[reason] += 1
                previous_geometry, previous_timely = None, False
                continue
            timely = min(pose_times[key]) <= row["presented_ns"]
            late_geometry += not timely
            if value == baseline: unchanged += 1
            else:
                different.append(row)
                if row["owner_begin_ns"] < begin: prior_frames += 1
                elif timely: onset.append(row)
            if previous_geometry is None: unknown_comparisons += 1
            elif value == previous_geometry:
                static += 1
                same_geometry_new_revision += row["camera_revision"] != previous_revision
            else:
                successive_changes += 1
                if timely and previous_timely and row["owner_begin_ns"] >= begin: progress.append(row)
            previous_geometry, previous_revision, previous_timely = value, row["camera_revision"], timely
        gaps = [b - a for a, b in zip([begin] + times, times + [end])]
        progress_times = [row["presented_ns"] for row in progress]
        progress_gaps = [b - a for a, b in zip([begin] + progress_times, progress_times + [end])]
        results.append({"contact": contact, "actual_presentations": len(shown),
            "full_contact_seconds": (end - begin) / 1e9,
            "actual_presentations_per_s": len(shown) * 1e9 / (end - begin),
            "first_any_canvas_present_ms": (times[0] - begin) / NS_PER_MS if times else None,
            "leading_gap_ms": ((times[0] if times else end) - begin) / NS_PER_MS,
            "trailing_gap_ms": (end - (times[-1] if times else begin)) / NS_PER_MS,
            "longest_gap_including_contact_edges_ms": max(gaps) / NS_PER_MS,
            "interior_interval_ms": distribution(b - a for a, b in zip(times, times[1:])),
            "known_baseline_pose_presentations": unchanged, "known_nonbaseline_pose_presentations": len(different),
            "precontact_nonbaseline_frame_presentations": prior_frames,
            "successive_geometry_changes": successive_changes, "static_geometry_presentations": static,
            "same_geometry_new_revision_presentations": same_geometry_new_revision,
            "late_geometry_observation_presentations": late_geometry, "unknown_geometry_comparisons": unknown_comparisons,
            "timely_witness_geometry_progress_presentations": len(progress),
            "geometry_progress_per_s_lower_bound": len(progress) * 1e9 / (end - begin),
            "geometry_progress_interval_ms": distribution(b - a for a, b in zip(progress_times, progress_times[1:])),
            "geometry_progress_leading_gap_ms": ((progress_times[0] if progress_times else end) - begin) / NS_PER_MS,
            "geometry_progress_trailing_gap_ms": (end - (progress_times[-1] if progress_times else begin)) / NS_PER_MS,
            "geometry_progress_longest_gap_including_contact_edges_ms": max(progress_gaps) / NS_PER_MS,
            "unknown_geometry_presentations": sum(unknown.values()), "unknown_geometry_reasons": dict(unknown),
            "first_proven_changed_pose_upper_bound_ms": (onset[0]["presented_ns"] - begin) / NS_PER_MS if onset else None,
            "baseline_geometry_observed_before_contact": baseline_observed})
    partial = any(row["unknown_geometry_presentations"] or row["late_geometry_observation_presentations"]
                  or row["unknown_geometry_comparisons"] for row in results)
    availability = "invalid" if errors or camera_report["availability"] == "invalid" else camera_report["availability"]
    if availability == "observed" and partial: availability = "partial"
    return {"availability": availability, "camera_availability_reason": camera_report.get("reason"),
            "errors": dict(errors), "camera_errors": camera_report.get("errors", {}), "contacts": results,
            "scope": "Nonbaseline geometry is not motion. Progress requires distinct successive presented geometry with timely witnesses; unknowns break adjacency. Late revision mappings attribute immutable state only, never causal onset. Full-contact bounds retain static/snapping gaps. No moving-frame target, physical latency or native overlay qualification."}


def canvas_update_progress(header, grouped, intervals):
    if not header.get("raster_update_counter"):
        return {"availability": "unavailable", "reason": "No renderer update counter"}
    if not intervals or any(not 0 < a < b for a, b in intervals) or any(
            a[1] > b[0] for a, b in zip(intervals, intervals[1:])):
        return {"availability": "unavailable", "reason": "No complete active-input intervals"}
    frames, counters, acquired, callbacks = (collections.defaultdict(list) for _ in range(4))
    for row in grouped[1]: frames[row[0]].append(row)
    for row in grouped[15]: counters[row[0]].append(row)
    for row in grouped[3]:
        if row[4]: acquired[row[0]].append(row)
    for row in grouped[4]: callbacks[(row[0], row[3])].append(row)
    completions, presentations, consumed = [], [], set()
    initial_raster = None
    last_raster = last_input_stamp = 0
    invalid = collections.Counter()
    outside_errors = collections.Counter()
    begin, end = intervals[0][0], intervals[-1][1]
    baseline = next((key for key in sorted(frames, reverse=True) if frames[key][0][2] < begin), None)
    if baseline is None:
        return {"availability": "unavailable", "reason": "No pre-input counter baseline"}
    identities = [baseline] + [key for key in sorted(frames) if begin <= frames[key][0][2] < end]
    for identity in identities:
        if len(frames[identity]) != 1 or len(counters[identity]) != 1:
            invalid["ambiguous_frame_or_counter"] += 1
            continue
        frame, counter = frames[identity][0], counters[identity][0]
        raster, stamp = counter[2], counter[1]
        if initial_raster is None: initial_raster = raster
        advanced = raster > last_raster and raster > initial_raster
        if raster < last_raster or stamp < last_input_stamp or stamp > frame[3]:
            invalid["regressing_counter_or_future_input"] += 1
        last_raster = max(last_raster, raster)
        last_input_stamp = max(last_input_stamp, stamp)
        if stamp: consumed.add(stamp)
        if not acquired[identity]: continue
        errors = invalid if any(a <= frame[2] < b for a, b in intervals) else outside_errors
        presentation, error = drawable_presentation(frame, acquired[identity], callbacks)
        if error:
            errors[error] += 1
            continue
        _, presented = presentation
        presentations.append([identity, frame[2], presented, raster, stamp])
        if advanced: completions.append(presentations[-1])
    if any(a[2] >= b[2] for a, b in zip(completions, completions[1:])):
        invalid["nonincreasing_presentations"] += 1
    if invalid or header.get("dropped_records"):
        return {"availability": "invalid", "errors": dict(invalid),
                "dropped_records": header.get("dropped_records", 0)}
    if not presentations:
        return {"availability": "unavailable", "reason": "No joined drawable presentations"}
    snapshot = lambda at: {"submitted_frames": sum(r[1] < at for r in completions),
                           "completed_frames": sum(r[2] < at for r in completions)}
    report = {"motion": {"begin_ns": begin, "end_ns": end, "active_intervals_ns": intervals},
              "completions": completions, "renderer_before": {"rows": [{"label": "Frames", "value": initial_raster or 0}]},
              "display_before": snapshot(begin), "display_after_input": snapshot(end)}
    progress = completion_window(report)
    for key in ["snapshot_submitted_per_s", "snapshot_completed_per_s", "snapshot_pending"]:
        del progress[key]
    fresh = input_completions(report, sorted(consumed))
    gaps = progress.pop("completion_gaps_ms")
    return {"availability": "observed", **progress,
            "outside_contact_presentation_errors": dict(outside_errors),
            "completion_gap_ms": distribution(gaps, divisor=1),
            "input_updates_per_s_lower_bound": sum(map(len, fresh)) / progress["active_input_seconds"],
            "first_fresh_update_completion_upper_bound_ms": [
                (rows[0][2] - interval[0]) / NS_PER_MS if rows else None
                for rows, interval in zip(fresh, intervals)],
            "input_completion_gap_ms": distribution(
                b[2] - a[2] for rows in fresh for a, b in zip(rows, rows[1:])),
            "scope": "Presentation bounds queue completion above; rates bound completed nonempty submissions below, not exact GPU finish times or guaranteed input pixels"}


def workload_contact_intervals(markers, expected):
    contacts = collections.defaultdict(list)
    for row in markers:
        if row[1] in (9, 10): contacts[row[3]].append(row)
    if not expected or len(contacts) != expected: return []
    intervals = []
    for rows in contacts.values():
        if len(rows) != 2 or [r[1] for r in rows] != [9, 10]: return []
        intervals.append([rows[0][4], rows[1][4]])
    return sorted(intervals)


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
    calibrated_gpu, gpu_clocks = calibrate_gpu(grouped[14], gpu)
    submitted = [r for r in frames.values() if r[6] > 0]
    ready_states = [r[0] for r in grouped[9] if r[2] & 9 == 9 and not r[3]]
    ready_at = min(ready_states, default=None)
    state_errors = sum(bool(r[3]) for r in grouped[9])
    stats = sorted(grouped[8], key=lambda r: r[0])
    last_stats = stats[-1] if stats else [0] * 10
    displays = sorted({(r[1], r[2], r[3] / 1000, r[4]) for r in grouped[6]})
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
    visible_frames = [(r, frames[r[0]]) for r in visible.values() if r[0] in frames]
    def gpu_completion_metrics(records):
        identities = {r[0] for r in records}
        ends = {key: bounds[1] for key, bounds in calibrated_gpu.items() if key in identities}
        targets = [(end, frames[key][1]) for key, end in ends.items() if frames[key][1]]
        return {
            "calibrated_frames": len(ends),
            "frames_without_calibrated_gpu_times": len(identities) - len(ends),
            "sampling_uncertainty_ms": distribution(hi - lo for lo, hi in ends.values()),
            "gpu_end_minus_presentation_target_ms": distribution((lo + hi) / 2 - target for (lo, hi), target in targets),
            "gpu_end_before_target": sum(hi <= target for (_, hi), target in targets),
            "gpu_end_after_target": sum(lo > target for (lo, _), target in targets),
            "gpu_end_overlaps_target": sum(lo <= target < hi for (lo, hi), target in targets),
            "gpu_end_to_presentation_ms": distribution(r[1] - sum(ends[r[0]]) / 2
                                                       for r in visible.values() if r[0] in ends),
        }
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
    gpu_timing_requested = header["gpu_timing_requested"]
    if not gpu_timing_requested:
        warnings.append("GPU timing intentionally disabled; this trace measures CPU and presentation only.")
        if gpu or last_stats[2] or grouped[14]:
            warnings.append("GPU observations contradict the disabled instrumentation setting.")
    elif last_stats[1] != 1:
        warnings.append("GPU timestamps are unavailable or not initialized.")
    zero_gpu = sum(r[2] == 1 and r[1] == 0 for r in gpu.values())
    if last_stats[3] or last_stats[4] or last_stats[5] or any(r[6] for r in stats) or zero_gpu:
        warnings.append("GPU observations include skipped, invalid, pending or failed polls.")
    if gpu_clocks["invalid_samples"] or gpu_clocks["discontinuities"]:
        warnings.append("GPU clock samples are invalid or discontinuous; endpoint calibration is unavailable.")
    if calibrated_gpu:
        warnings.append("Calibrated GPU endpoints use interpolation and sampling bounds; presentation targets are not Metal commit deadlines.")
    if not displays or all(d[3] < target_hz for d in displays):
        warnings.append(f"No observed display configuration advertises {target_hz:g} Hz.")
    if ready_at is None:
        warnings.append("No error-free state with canvas and shaders ready was observed.")
    warnings.extend(["Input receipt association does not establish physical input-to-pixel latency.",
                     "GPU queue spans include submission gaps and profiler submissions; overhead is not calibrated.",
                     "This trace alone does not establish the required workload matrix or ten-minute acceptance."])
    result = {
        "schema": 1, "platform": "iPadOS" if header["platform"] == 0 else "macOS",
        "input_source": header["input_source"],
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
        "display_configurations": [{"pixels": list(d[:2]), "scale": d[2], "maximum_hz": d[3]} for d in displays],
        "recorder_reserved_bytes": header["capacity"] * header["record_stride_bytes"],
        "ready_seconds_from_start": None if ready_at is None else (ready_at - header["started_ns"]) / 1e9,
        "all_submitted_frames": frame_metrics(submitted),
        "frames_after_readiness": frame_metrics([r for r in submitted if ready_at is not None and r[0] > ready_at]),
        "gpu_queue_span_ms": distribution(r[1] for r in gpu.values() if r[2] == 1 and r[1] > 0),
        "gpu_clock_calibration": gpu_clocks,
        "gpu_completion": gpu_completion_metrics(submitted),
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
        "engine_consumed_paint_progress": consumed_paint_progress(grouped),
        "encoded_camera": encoded_camera_presentations(header, grouped),
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
                  "postlude_observed": any(r[1] == 4 for r in markers), "failure_recorded": failed,
                  "canvas_update_progress": {"availability": "unavailable", "reason": "No complete workload interval"},
                  "engine_consumed_paint_progress": {"availability": "unavailable", "reason": "No complete workload interval"}}
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
                "canvas_update_progress": canvas_update_progress(header, grouped,
                    workload_contact_intervals(markers, header["workload"].get("measured_contacts", 0))),
                "engine_consumed_paint_progress": consumed_paint_progress(grouped, begin[0], end[0]),
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
                "gpu_queue_span_ms": distribution(r[1] for key, r in gpu.items() if key in identities and r[2] == 1 and r[1] > 0),
                "gpu_samples_missing": sum(key not in gpu for key in identities),
                "gpu_completion": gpu_completion_metrics(rows),
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


def motion_windows(header, events, windows, target_hz):
    grouped = collections.defaultdict(list)
    for event in events:
        grouped[event[0]].append(event[1:])
    results = []
    for window in windows:
        begin, end = window["begin_ns"], window["end_ns"]
        if not isinstance(begin, int) or not isinstance(end, int) or not 5e9 <= end - begin <= 10e9:
            raise ValueError("Each observed motion window must span 5–10 seconds")
        if not window.get("motion_witness"):
            raise ValueError("Observed motion witness is required; scheduled time alone is insufficient")
        acquired = {(e[1], e[4]) for e in events if e[0] == 3 and e[5] and begin <= e[1] < end}
        raw = [e for e in events if e[0] == 4]
        presented = {(e[1], e[4]): e for e in raw}
        visible = {key: e for key, e in presented.items() if begin <= e[2] <= end and e[2] > 0}
        scoped = acquired | visible.keys()
        duplicates = sum((e[1], e[4]) in scoped for e in raw) - len(scoped & presented.keys())
        missing = len(acquired - presented.keys())
        zero = sum(key in acquired and e[2] == 0 for key, e in presented.items())
        times = sorted(e[2] for e in visible.values())
        ready = [e for e in events if e[0] == 9 and e[1] <= begin]
        prior = max(ready, key=lambda e: e[1], default=None)
        during = [e for e in events if e[0] == 9 and begin <= e[1] <= end]
        memory = [e for e in events if e[0] == 5 and begin <= e[1] <= end]
        intervals = distribution(b - a for a, b in zip(times, times[1:]))
        full_ready = bool(prior and prior[3] & 9 == 9 and not prior[4])
        readiness_losses = sum(e[3] & 9 != 9 for e in during)
        errors = sum(bool(e[4]) for e in during)
        thermal_zero = bool(memory) and all(e[4] == 0 and e[5] == 0 for e in memory)
        hz = len(times) * 1e9 / (end - begin)
        budget = 1000 / target_hz
        coverage = bool(times and times[0] - begin <= budget * 2e6 and end - times[-1] <= budget * 2e6)
        results.append({"window": window, "presented_frames": len(times), "presentations_per_s": hz,
            "engine_consumed_paint_progress": consumed_paint_progress(grouped, begin, end),
            "canvas_update_progress": canvas_update_progress(header, grouped, [[begin, end]]),
            "interval_ms": intervals, "fully_ready_at_begin": full_ready, "readiness_loss_records": readiness_losses,
            "frame_error_records": errors, "missing_presentation_callbacks": missing,
            "zero_time_presentations": zero, "duplicate_presentation_records": duplicates,
            "thermal_zero_observed": thermal_zero, "boundary_coverage": coverage,
            "configuration": header.get("configuration"),
            "instrumentation_eligible": bool(header.get("configuration") == "release" and not header.get("dropped_records")
                and full_ready and not (readiness_losses or errors or missing or zero or duplicates)
                and thermal_zero and coverage and intervals["count"]),
            "scope": "Metal canvas only; human-reviewed motion witness is scope evidence, not automatic movement or target proof"})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-hz", type=float, default=120,
                        help="Refresh rate to evaluate (default: 120; use 90 for the current Mac display)")
    parser.add_argument("--motion-windows", type=Path, help="Observed native-clock motion windows JSON; no synthetic markers")
    parser.add_argument("--camera-witness", type=Path, help="Owned PID/clock, timestamped camera poses and complete native contact boundaries JSON")
    args = parser.parse_args()
    with args.trace.open() as source:
        header = json.loads(next(source))
        events = [json.loads(line) for line in source if line.strip()]
    try:
        report = analyze(header, events, target_hz=args.target_hz)
        if args.motion_windows:
            report["observed_motion_windows"] = motion_windows(header, events, json.loads(args.motion_windows.read_text()), args.target_hz)
        if args.camera_witness:
            report["observed_camera_contacts"] = camera_contacts(header, report["encoded_camera"], json.loads(args.camera_witness.read_text()))
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
