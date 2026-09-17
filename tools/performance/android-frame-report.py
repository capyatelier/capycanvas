#!/usr/bin/env python3
"""Summarize real Android host callbacks separately from renderer sample history."""
import json
import sys


def percentile(values, fraction):
    values = sorted(values)
    return round(values[int((len(values) - 1) * fraction)], 3) if values else None


def report(label, run):
    timeline = run["timeline"]
    frames = timeline["frames"]
    fields = timeline["frame_fields"]
    print(label, "frames:", len(frames))
    for i, name in enumerate(fields):
        if i < 4:
            continue
        values = [row[i] / 1e6 for row in frames]
        print(f"  {name.removesuffix('_ns'):24} p50={percentile(values, .5)} p95={percentile(values, .95)} ms")
    intervals = [(b[0] - a[0]) / 1e6 for a, b in zip(frames, frames[1:])]
    rate = (len(frames) - 1) * 1e9 / (frames[-1][0] - frames[0][0]) if len(frames) > 1 else 0
    print("  vsync interval", percentile(intervals, .5), "ms p50;",
          percentile(intervals, .95), "ms p95;", round(rate, 1), "callbacks/s")
    inputs = timeline["inputs"]
    for name, a, b in [("event to delivery", 0, 1), ("worker queue", 1, 2)]:
        values = [(row[b] - row[a]) / 1e6 for row in inputs]
        print(f"  {name:24} p50={percentile(values, .5)} p95={percentile(values, .95)} ms")
    # SurfaceFlinger's ring can include frames from the preceding operation.
    # Restrict presentation timestamps to this measured input interval.
    presented = []
    for line in (run.get("surface_latency") or "").splitlines()[1:]:
        columns = line.split()
        if len(columns) == 3 and inputs:
            timestamp = int(columns[1])
            if inputs[0][0] <= timestamp <= inputs[-1][0]:
                presented.append(timestamp)
    intervals = [(b - a) / 1e6 for a, b in zip(presented, presented[1:]) if b > a]
    print("  presentation interval", percentile(intervals, .5), "ms;", len(intervals), "intervals")
    print("  renderer rolling history (may include earlier operations):", run.get("cpu_ms"), run.get("gpu_ms"))


def main(path):
    data = json.load(open(path))
    for i, run in enumerate(data["runs"]):
        label = f"run {i}: {run.get('extent', data.get('extent'))} scale={run.get('factor')}"
        if "timeline" in run:
            report(label, run)
        else:
            for motion in run.get("translation", []):
                report(f"{label} move tool={motion['tool']}", motion)
            if "drawing" in run:
                report(f"{label} drawing", run["drawing"])
    if "two_layers" in data:
        report("both layers", data["two_layers"])


if __name__ == "__main__":
    main(sys.argv[1])
