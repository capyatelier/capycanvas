"""Account for completed, nonempty canvas updates inside the input window."""
import math


def validate_setup(info, requested):
    state = info["state"]
    camera = state["camera"]
    expected = {key: requested[key] for key in
                ("preset", "brush_size", "mode", "prediction", "speed", "duration_ms", "repeats")}
    actual = {key: info.get(key) for key in expected}
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
    for _, queued_ns, done_ns, raster in report["completions"]:
        changed = raster > last_raster
        last_raster = max(last_raster, raster)
        if not begin <= queued_ns < end:
            continue
        if not changed:
            empty += 1
            continue
        submitted += 1
        if done_ns < end:
            completed += 1
        else:
            pending += 1
    return dict(result, accounting="input-window-nonempty",
                submitted=submitted, completed=completed, pending_at_input_end=pending,
                empty_updates=empty, submitted_per_s=submitted / seconds,
                completed_per_s=completed / seconds)
