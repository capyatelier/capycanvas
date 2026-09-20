#!/usr/bin/env python3
"""Measure native canvas progress, not Compose/UI FPS, in a Perfetto trace.

Run during continuous navigation. --check rejects a stalled canvas even when
vkQueuePresentKHR keeps succeeding. Timestamps are latches, not photons or
input-to-display latency. Gaps are measured across the active submission window;
idle time before its first and after its last call is reported separately.
"""
import argparse
import csv
import io
import json
import math
import subprocess


def summarize(rows):
    bounds = next(row for row in rows if row["kind"] == "bounds")
    trace_start, trace_end = int(bounds["ts"]), int(bounds["dur"])
    presents = [row for row in rows if row["kind"] == "present"]
    start = min(int(row["ts"]) for row in presents) if presents else trace_start
    end = max(int(row["ts"]) + max(0, int(row["dur"])) for row in presents) if presents else trace_end
    end = max(start + 1, end)
    latches = sorted(int(row["ts"]) for row in rows if row["kind"] == "latch" and start <= int(row["ts"]) <= end)
    zoom = [float(row["dur"]) / 1000 for row in rows if row["kind"] == "zoom"]
    # Include submission-window edges: continuously presenting a frozen image
    # must fail even when there are no latch events at all.
    intervals = [(b - a) / 1e6 for a, b in zip([start] + latches, latches + [end])]
    intervals.sort()
    calls = sorted(int(row["dur"]) / 1e6 for row in presents if int(row["dur"]) >= 0)

    def percentile(values, fraction):
        return round(values[min(len(values) - 1, math.ceil(len(values) * fraction) - 1)], 3) if values else None

    return {
        "retained_seconds": round((trace_end - trace_start) / 1e9, 3),
        "active_submission_seconds": round((end - start) / 1e9, 3),
        "present_calls": len(presents),
        "canvas_latches": len(latches),
        "latches_per_second": round(len(latches) / ((end - start) / 1e9), 2),
        "latch_gap_ms": {"p50": percentile(intervals, .5), "p99": percentile(intervals, .99), "max": percentile(intervals, 1)},
        "present_call_ms": {"p50": percentile(calls, .5), "p99": percentile(calls, .99), "max": percentile(calls, 1)},
        "zoom_percent": {"min": min(zoom), "max": max(zoom)} if zoom else None,
        "trace_errors": [row["name"] for row in rows if row["kind"] == "error"],
    }


def check(report):
    return (report["active_submission_seconds"] >= 5 and report["present_calls"] >= 30
            and report["canvas_latches"] >= report["present_calls"] * .5
            and report["latch_gap_ms"]["max"] < 500 and not report["trace_errors"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace")
    parser.add_argument("--processor", required=True, help="Perfetto trace_processor executable")
    parser.add_argument("--canvas-tid", type=int, help="Explicit canvas thread ID if a wrapped trace lost process names")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    identity = f"t.tid={args.canvas_tid}" if args.canvas_tid is not None else "p.name='art.capycanvas'"
    query = f"""
      SELECT 'bounds' kind, start_ts ts, end_ts dur, '' name FROM trace_bounds
      UNION ALL
      SELECT 'present', s.ts, s.dur, s.name FROM slice s
        JOIN thread_track tt ON tt.id=s.track_id JOIN thread t ON t.utid=tt.utid
        LEFT JOIN process p ON p.upid=t.upid
        WHERE s.name='QueuePresentKHR' AND ({identity})
      UNION ALL
      SELECT 'latch', ts, dur, name FROM slice WHERE name GLOB 'latchBuffer SurfaceView*art.capycanvas/*'
      UNION ALL
      SELECT 'zoom', c.ts, c.value, t.name FROM counter c JOIN counter_track t ON t.id=c.track_id
        WHERE t.name='Capy canvas zoom milli-percent'
      UNION ALL
      SELECT 'error', 0, value, name FROM stats WHERE severity='error' AND value>0
    """
    result = subprocess.run([args.processor, "query", args.trace, query], check=True, capture_output=True, text=True)
    report = summarize(list(csv.DictReader(io.StringIO(result.stdout))))
    report["progress_check_passed"] = check(report)
    print(json.dumps(report, indent=2))
    if args.check and not report["progress_check_passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
