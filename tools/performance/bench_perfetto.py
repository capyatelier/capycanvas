#!/usr/bin/env python3
"""Write each Android bench pass's frames.jsonl and memory.jsonl from the device session's Perfetto trace."""
import argparse
import csv
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess

_spec = importlib.util.spec_from_file_location("android_viewport_presents", Path(__file__).with_name("android-viewport-presents.py"))
viewport = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(viewport)

LATENCY_TAIL_NS = 2_000_000_000
MEMORY_FIELDS = {"MemAvailable": "os_available_kib", "SwapFree": "swap_free_kib", "mem.rss": "app_rss_kib", "GPU Memory": "gpu_kib"}


def trace_config(duration_ms):
    config = viewport.trace_config("", duration_ms=duration_ms)
    config = config.replace('ftrace_events: "ftrace/print"', 'ftrace_events: "ftrace/print" ftrace_events: "gpu_mem/gpu_mem_total"')
    return config + '''data_sources { config { name: "linux.process_stats" process_stats_config { scan_all_processes_on_start: true proc_stats_poll_ms: 1000 } } }
data_sources { config { name: "linux.sys_stats" sys_stats_config { meminfo_period_ms: 1000 meminfo_counters: MEMINFO_MEM_AVAILABLE meminfo_counters: MEMINFO_SWAP_FREE } } }
data_sources { config { name: "android.gpu.memory" } }
'''


def query(processor, trace, sql, first):
    out = subprocess.run([processor, "query", "--format", "csv", str(trace), sql], capture_output=True, text=True, check=True).stdout
    return list(csv.DictReader(io.StringIO(out[out.index(f'"{first}"'):]))) if f'"{first}"' in out else []


def frame_timeline_times(processor, trace, package, surface):
    app = package.replace("'", "''")
    if surface == "canvas":
        attribution = f"""WITH buffers AS (
SELECT c.ts, c.value, LAG(c.value) OVER (PARTITION BY c.track_id ORDER BY c.ts, c.id) previous
FROM counter c JOIN counter_track t ON c.track_id = t.id
WHERE t.name GLOB 'BufferTX*SurfaceView*{app}/*'
), tokens AS (
SELECT CAST(SUBSTR(dispatch.name, 21) AS INT) AS token FROM buffers b
JOIN slice latch ON latch.name = 'latchBuffer' AND b.ts BETWEEN latch.ts AND latch.ts + latch.dur
JOIN ancestor_slice(latch.id) dispatch ON dispatch.name GLOB 'onMessageInvalidate [0-9]*'
WHERE b.value < b.previous
)"""
    else:
        attribution = f"""WITH tokens AS (
SELECT frame.display_frame_token AS token FROM actual_frame_timeline_slice frame JOIN process p USING (upid)
WHERE p.name = '{app}' AND frame.layer_name IS NOT NULL AND INSTR(frame.layer_name, 'SurfaceView') = 0
AND frame.layer_name NOT GLOB 'Bounds for *'
AND frame.present_type IN ('On-time Present', 'Late Present', 'Early Present')
)"""
    sql = attribution + """
SELECT DISTINCT display.ts + display.dur AS presented_at FROM actual_frame_timeline_slice display
WHERE display.layer_name IS NULL AND display.dur > 0 AND display.display_frame_token IN (SELECT token FROM tokens)
ORDER BY presented_at"""
    return [int(row["presented_at"]) for row in query(processor, trace, sql, "presented_at")]


def present_times(processor, trace, package):
    result = {}
    for surface in ("canvas", "window"):
        times = {time for _, time in viewport.present_rows(processor, trace, package, surface == "window")}
        result[surface] = sorted(times) if times else frame_timeline_times(processor, trace, package, surface)
    return result


def memory_rows(processor, trace, package):
    app = package.replace("'", "''")
    sql = f"""SELECT c.ts ts, t.name name, c.value value FROM counter c JOIN counter_track t ON c.track_id = t.id
WHERE t.name IN ('MemAvailable', 'SwapFree') OR t.id IN (SELECT pt.id FROM process_counter_track pt JOIN process p USING (upid)
WHERE p.name = '{app}' AND pt.name IN ('mem.rss', 'GPU Memory')) ORDER BY c.ts"""
    return [(int(row["ts"]), row["name"], float(row["value"])) for row in query(processor, trace, sql, "ts")]


def pass_window(record):
    times = [step[key] for step in record["steps"] for key in ("begin_ns", "end_ns", "settled_ns")
             if isinstance(step.get(key), int) and step[key] > 0]
    return min(times), max(times) + LATENCY_TAIL_NS


def frames(times, begin, end):
    return sorted(({"t_ns": time, "surface": surface} for surface, values in times.items() for time in values if begin <= time <= end),
                  key=lambda row: (row["t_ns"], row["surface"]))


def memory(rows, begin, end, thermal=None):
    samples = {}
    for ts, name, value in rows:
        if begin <= ts <= end:
            samples.setdefault(ts, {"t_ns": ts})[MEMORY_FIELDS[name]] = round(value / 1024)
    first = [{"t_ns": begin, "thermal": thermal}] if thermal is not None else []
    return first + [samples[ts] for ts in sorted(samples)]


def write_jsonl(path, rows):
    path.write_text("".join(json.dumps(row) + "\n" for row in rows))


def convert(device_dir, processor):
    session = json.loads((device_dir / "session.json").read_text())
    trace = device_dir / "trace.perfetto-trace"
    cache = {}
    for entry in session.get("passes", []):
        directory = device_dir / entry["version"] / f"pass-{entry['pass']}"
        if not (directory / "pass.json").exists():
            continue
        package = session["versions"][entry["version"]]["app_id"]
        if package not in cache:
            cache[package] = present_times(processor, trace, package), memory_rows(processor, trace, package)
        times, rows = cache[package]
        begin, end = pass_window(json.loads((directory / "pass.json").read_text()))
        write_jsonl(directory / "frames.jsonl", frames(times, begin, end))
        write_jsonl(directory / "memory.jsonl", memory(rows, begin, end, entry.get("thermal_before")))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device_dir", type=Path, help="RUN_DIR/DEVICE, holding session.json and trace.perfetto-trace")
    parser.add_argument("--processor", default=os.environ.get("CAPY_TRACE_PROCESSOR"),
                        help="trace_processor_shell (default: $CAPY_TRACE_PROCESSOR)")
    args = parser.parse_args()
    if not args.processor:
        parser.error("pass --processor or set CAPY_TRACE_PROCESSOR")
    convert(args.device_dir, args.processor)


if __name__ == "__main__":
    main()
