"""Account for completed, nonempty canvas updates inside the input window."""
import math
import bisect


def input_completions(report):
    fields = report["input_fields"]
    events = [dict(zip(fields, row)) for row in report["inputs"]]
    processed = sorted(event["event_ns"] for event in events if event["phase"] in (1, 2))
    motion = report["motion"]
    intervals = motion.get("active_intervals_ns", [[motion["begin_ns"], motion["end_ns"]]])
    last_input = -1
    last_raster = int(next(row["value"] for row in report["renderer_before"]["rows"] if row["label"] == "Frames"))
    completed = [[] for _ in intervals]
    for row in report["completions"]:
        _, queued, done, raster, consumed = row
        if raster <= last_raster:
            continue
        last_raster = raster
        current = bisect.bisect_right(processed, consumed) - 1
        if current <= last_input:
            continue
        last_input = current
        for index, (begin, end) in enumerate(intervals):
            if begin <= processed[current] <= queued < done < end:
                completed[index].append(row)
                break
    return completed


def contact_latencies(report):
    fields = report["input_fields"]
    begin, end = report["motion"]["begin_ns"], report["motion"]["end_ns"]
    contacts = []
    for row in report["inputs"]:
        event = dict(zip(fields, row))
        if event.get("phase") != 1 or not begin <= event["arrival_ns"] < end:
            continue
        finished = event["worker_start_ns"] + event["cpu_input_ns"]
        next_frame = next((r for r in report["completions"] if r[1] >= finished and r[4] >= event["event_ns"]), None)
        contacts.append(dict(pending_composition=bool(event.get("pending_composition")),
                             queue_ms=(event["worker_start_ns"] - event["arrival_ns"]) / 1e6,
                             present_queued_ms=(next_frame[1] - event["arrival_ns"]) / 1e6 if next_frame else None,
                             next_gpu_ms=(next_frame[2] - event["arrival_ns"]) / 1e6 if next_frame else None))
    return contacts


def validate_setup(info, requested):
    state = info["state"]
    camera = state["camera"]
    expected = {key: requested[key] for key in
                ("preset", "brush_size", "mode", "prediction", "speed", "duration_ms", "repeats")}
    actual = {key: info.get(key) for key in expected}
    if "stats_panel" in requested:
        expected["stats_panel"] = requested["stats_panel"]
        actual["stats_panel"] = info.get("stats_panel", True)
    for axis, (radius, extent) in enumerate(zip(requested["radii"], camera["work_area"][2:])):
        expected[f"radius_{axis}"] = min(radius, extent * .45)
        actual[f"radius_{axis}"] = info["radii"][axis]
    expected.update(layers=requested["photo_layers"] + 2,
                    diameter=requested["brush_size"], selected_preset=requested["preset"],
                    feedback=requested["prediction"])
    actual.update(layers=len(state["layers"]), diameter=state["brush"]["diameter"],
                  selected_preset=state["brush"]["preset"],
                  feedback=state["settings"]["feedback"])
    if requested["prediction"]:
        expected["horizon"] = requested["horizon"]
        actual["horizon"] = state["settings"]["prediction_ms"]
    if requested["zoom"] is not None:
        expected["zoom"] = requested["zoom"]
        actual["zoom"] = camera["zoom"]
    if requested.get("blending") is not None:
        expected["blending"] = ["blend_" + requested["blending"]]
        actual["blending"] = [command["id"] for command in state.get("commands", [])
                              if command.get("id") in ("blend_linear", "blend_perceptual")
                              and command.get("selected") is True]
    mismatches = []
    for key, value in expected.items():
        observed = actual[key]
        matches = (isinstance(observed, (float, int)) and math.isclose(observed, value, rel_tol=1e-5, abs_tol=1e-5)
                   if isinstance(value, (float, int)) else observed == value)
        if not matches:
            mismatches.append(f"{key}: requested {value!r}, observed {observed!r}")
    if mismatches:
        raise ValueError("Benchmark setup mismatch: " + "; ".join(mismatches))


def completion_window(report):
    begin, end = report["motion"]["begin_ns"], report["motion"]["end_ns"]
    seconds = (end - begin) / 1e9
    intervals = report["motion"].get("active_intervals_ns", [[begin, end]])
    active_seconds = sum(b - a for a, b in intervals) / 1e9
    before, after = report["display_before"], report["display_after_input"]
    snapshot_submitted = after["submitted_frames"] - before["submitted_frames"]
    snapshot_completed = after["completed_frames"] - before["completed_frames"]
    result = {"input_seconds": seconds,
              "snapshot_submitted_per_s": snapshot_submitted / seconds,
              "snapshot_completed_per_s": snapshot_completed / seconds,
              "snapshot_pending": after["submitted_frames"] - after["completed_frames"]}
    last_raster = int(next(row["value"] for row in report["renderer_before"]["rows"]
                           if row["label"] == "Frames"))
    submitted = completed = empty = pending = 0
    for row in report["completions"]:
        _, queued_ns, done_ns, raster = row[:4]
        changed = raster > last_raster
        last_raster = max(last_raster, raster)
        interval_end = next((b for a, b in intervals if a <= queued_ns < b), None)
        if interval_end is None:
            continue
        if not changed:
            empty += 1
            continue
        submitted += 1
        if done_ns < interval_end:
            completed += 1
        else:
            pending += 1
    return dict(result, accounting="active-input-intervals-nonempty" if "active_intervals_ns" in report["motion"] else "input-window-nonempty",
                active_input_seconds=active_seconds,
                submitted=submitted, completed=completed, pending_at_input_end=pending,
                empty_updates=empty, submitted_per_s=submitted / active_seconds,
                completed_per_s=completed / active_seconds)
