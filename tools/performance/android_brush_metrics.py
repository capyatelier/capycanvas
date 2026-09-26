"""Account for completed, nonempty canvas updates inside the input window."""


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
