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
        _, queued, done, raster, consumed = row[:5]
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


def object_completions(report):
    begin, end = report["motion"]["begin_ns"], report["motion"]["end_ns"]
    edits = {edit[2]: edit for edit in report["motion"].get("object_edits", [])}
    last_pose = -1
    rows = []
    for row in report["completions"]:
        if len(row) < 6:
            continue
        revision = row[5]
        edit = edits.get(revision)
        if edit is None or revision <= last_pose:
            continue
        last_pose = revision
        if begin <= edit[0] <= row[1] < row[2] < end:
            rows.append((row, edit))
    return rows


def input_feedback(report):
    events = sorted((dict(zip(report["input_fields"], row)) for row in report["inputs"]),
                    key=lambda event: event["arrival_ns"])
    raster = int(next(row["value"] for row in report["renderer_before"]["rows"] if row["label"] == "Frames"))
    completions = []
    for row in sorted(report["completions"], key=lambda row: row[2]):
        if row[3] > raster:
            completions.append(row)
            raster = row[3]
    begin, end = report["motion"]["begin_ns"], report["motion"]["end_ns"]
    event_index = completion_index = 0
    contact = latest = visible = None
    ages = []
    for now in range(begin, end, 1_000_000):
        while event_index < len(events) and events[event_index]["arrival_ns"] <= now:
            event = events[event_index]
            event_index += 1
            if event["phase"] == 1:
                contact, latest, visible = event, event, None
            elif event["phase"] == 2 and contact is not None:
                latest = event
            elif event["phase"] == 3:
                contact = None
        while completion_index < len(completions) and completions[completion_index][2] <= now:
            row = completions[completion_index]
            completion_index += 1
            if contact is not None and row[4] >= contact["event_ns"]:
                visible = max(visible or row[4], row[4])
        if contact is not None:
            ages.append(max(0, latest["event_ns"] - visible) / 1e6 if visible is not None
                        else (now - contact["arrival_ns"]) / 1e6)
    latency = []
    for event in events:
        if event["phase"] not in (1, 2) or not begin <= event["arrival_ns"] < end:
            continue
        done = next((row[2] for row in completions if row[2] >= event["arrival_ns"] and row[4] >= event["event_ns"]), None)
        if done is not None:
            latency.append((done - event["event_ns"]) / 1e6)
    return dict(age_ms=ages, input_to_gpu_ms=latency, sample_period_ms=1,
                attribution="GPU completion of synchronous visible output; no scanout timestamps")


def validate_setup(info, requested):
    state = info["state"]
    camera = state["camera"]
    expected = {key: requested[key] for key in
                ("preset", "brush_size", "mode", "prediction", "speed", "duration_ms", "repeats")}
    actual = {key: info.get(key) for key in expected}
    for key in ("pause_ms", "contact_ms", "settle_delay_ms"):
        if key in requested:
            expected[key] = requested[key]
            actual[key] = info.get(key)
    if "color_mode" in requested:
        expected["color_mode"] = requested["color_mode"]
        actual["color_mode"] = info.get("color_mode", "full_color")
        expected["layer_color_mode"] = {"full_color": "Full color", "grayscale": "Grayscale", "two_tone": "Two-tone (black & white)"}[requested["color_mode"]]
        control = next((control for control in state.get("layer_properties", {}).get("controls", [])
                        if control.get("key") == "color_mode"), None)
        if control is not None:
            index = control.get("value", {}).get("value")
            options = control.get("kind", {}).get("options", [])
            actual["layer_color_mode"] = options[index] if type(index) is int and 0 <= index < len(options) else None
        else:
            actual["layer_color_mode"] = state.get("layer_tools", {}).get("color_mode", {}).get("value")
    if "stats_panel" in requested:
        expected["stats_panel"] = requested["stats_panel"]
        actual["stats_panel"] = info.get("stats_panel", True)
    for key in ("navigation_between_strokes", "navigation_settle_ms"):
        if key in requested:
            expected[key] = requested[key]
            actual[key] = info.get(key)
    if "canvas" in requested:
        expected["canvas"] = [requested["canvas"]]
        actual["canvas"] = [[tab.get("width"), tab.get("height")] for tab in state.get("tabs", []) if tab.get("active")]
    for axis, (radius, extent) in enumerate(zip(requested["radii"], camera["work_area"][2:])):
        expected[f"radius_{axis}"] = min(radius, extent * .45)
        actual[f"radius_{axis}"] = info["radii"][axis]
    workload = requested.get("workload", "ordinary")
    effect_count = int(workload in ("blurred-base", "objects-effects")) + int(bool(requested.get("live_filter")))
    expected.update(layers=requested["photo_layers"] + 2 + effect_count,
                    diameter=requested["brush_size"], selected_preset=requested["preset"],
                    feedback=requested["prediction"])
    actual.update(layers=len(state["layers"]), diameter=state["brush"]["diameter"],
                  selected_preset=state["brush"]["preset"],
                  feedback=state["settings"]["feedback"])
    expected["workload"] = workload
    actual["workload"] = info.get("workload", "ordinary")
    if requested.get("live_filter"):
        fixture=info.get("live_filter_fixture") or {}
        expected.update(live_filter=requested["live_filter"], live_filter_disabled=requested["live_filter_disabled"],
                        live_filter_values=requested["live_filter_values"], changing_filter_input=True)
        actual.update(live_filter=fixture.get("effect_id"), live_filter_disabled=fixture.get("disabled"),
                      live_filter_values=fixture.get("values"),
                      changing_filter_input=fixture.get("paint")==state["layer_properties"].get("layer"))
        rows=state["layers"]
        expected["live_filter_attachment"] = True
        actual["live_filter_attachment"] = any(row.get("id")==fixture.get("effect") and row.get("relationship")==dict(kind="effect",target=fixture.get("paint"))
                                              and row.get("visible")== (not requested["live_filter_disabled"]) for row in rows)
    if workload in ("clipped", "blurred-base"):
        fixture = info.get("attachment_fixture") or {}
        paint, base, effect = (fixture.get(key) for key in ("paint", "base", "effect"))
        expected["attachment_order"] = [paint] + ([effect] if effect_count else []) + [base]
        actual["attachment_order"] = [layer.get("id") for layer in state["layers"][:2 + effect_count]]
        expected["attached_rows"] = [dict(kind="clip",target=base)] + ([dict(kind="effect",target=base)] if effect_count else []) + [None]
        actual["attached_rows"] = [layer.get("relationship") for layer in state["layers"][:2 + effect_count]]
        expected["attachment_handles"] = True
        actual["attachment_handles"] = paint is not None and base is not None and paint != base and (not effect_count or effect is not None and effect not in (paint, base))
        if effect_count:
            expected.update(effect_id="gaussian_blur", effect_radius=requested["effect_radius"])
            actual.update(effect_id=fixture.get("effect_id"), effect_radius=fixture.get("sigma"))
    if workload.startswith("objects"):
        objects = info.get("object_fixture") or []
        expected.update(image_count=requested["image_count"], image_identities=1 if requested["image_sources"] == "shared" else requested["image_count"])
        actual.update(image_count=len(objects), image_identities=len({value.get("image") for value in objects}))
        expected["source_owners"] = expected["image_identities"]
        actual["source_owners"] = len({value.get("source_owner") for value in objects})
        expected["source_owners_known"] = True
        actual["source_owners_known"] = all(value.get("source_owner") is not None for value in objects)
        for key in ("paint_base_image_shared", "paint_base_source_shared"):
            expected[key] = [requested["image_sources"] == "shared"] * requested["image_count"]
            actual[key] = [value.get(key) for value in objects]
    if "paint_layer_index" in requested:
        index = requested["paint_layer_index"]
        expected["paint_layer_index"] = [index + int(workload == "objects-effects" and index > 0) + int(bool(requested.get("live_filter")))]
        actual["paint_layer_index"] = [i for i, layer in enumerate(state["layers"]) if layer.get("selected")]
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
    last_completed = {}
    gaps = []
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
            if interval_end in last_completed:
                gaps.append((done_ns - last_completed[interval_end]) / 1e6)
            last_completed[interval_end] = done_ns
        else:
            pending += 1
    return dict(result, accounting="active-input-intervals-nonempty" if "active_intervals_ns" in report["motion"] else "input-window-nonempty",
                active_input_seconds=active_seconds,
                submitted=submitted, completed=completed, pending_at_input_end=pending,
                empty_updates=empty, submitted_per_s=submitted / active_seconds,
                completed_per_s=completed / active_seconds, completion_gaps_ms=gaps)
