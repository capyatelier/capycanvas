#!/usr/bin/env python3
"""Summarize pulled AndroidViewportBenchmarkTest JSON files (directory argument)."""

import argparse
import json
from pathlib import Path


def quantiles(values):
    values = sorted(values)
    if not values:
        return None
    return {
        "n": len(values),
        **{
            name: round(values[int((len(values) - 1) * fraction)], 3)
            for name, fraction in [("p50", 0.5), ("p95", 0.95), ("p99", 0.99)]
        },
    }


def summarize(directory):
    result = {}
    for info_path in sorted(directory.glob("*-info.json")):
        label = info_path.name.removesuffix("-info.json")
        info = json.loads(info_path.read_text())
        duration = info["duration_ms"] / 1000
        runs = [
            json.loads(path.read_text())
            for path in sorted(directory.glob(label + "-[0-9].json"))
        ]
        if not runs:
            continue
        seconds = duration * len(runs)
        frames = [
            frame
            for run in runs
            for frame in run["frames"]
            if run["begin_ns"] <= frame[1] < run["begin_ns"] + duration * 1e9
        ]
        # viewport_ns is nonzero only after a successful surface acquisition
        # and presentation encoding. GPU-busy retries have zero frame costs.
        submitted = [frame for frame in frames if frame[6] > 0]
        result[label] = {
            "mode": info["display"]["present_mode"],
            "viewport_gpu_ms": quantiles(
                sample[1] / 1e6
                for run in runs
                for batch in run["presentation"]
                for sample in batch
                if sample[2] == 1
            ),
            "cpu_callback_ms": quantiles(frame[10] / 1e6 for frame in submitted),
            "owner_cpu_ms": quantiles(frame[17] / 1e6 for frame in submitted),
            "acquire_ms": quantiles(frame[5] / 1e6 for frame in frames),
            "present_call_ms": quantiles(frame[7] / 1e6 for frame in frames),
            "submitted_hz": round(len(submitted) / seconds, 1),
            "attempt_hz": round(len(frames) / seconds, 1),
            "owner_cpu_fraction": round(
                sum(frame[17] for frame in frames) / (seconds * 1e9), 3
            ),
        }
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(args.directory), indent=2))
