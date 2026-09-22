#!/usr/bin/env python3
"""Summarize raw BrushBenchmarkInstrumentation results without claiming scanout FPS."""
import argparse
import csv
import json
import math
import pathlib
import statistics
import subprocess
import sys
from android_brush_metrics import completion_window


def distribution(values):
    if not values:
        return {"n": 0}
    v = sorted(values)
    return {"n": len(v), "mean": statistics.mean(v), "p50": statistics.median(v),
            **{key: v[min(len(v) - 1, math.ceil(len(v) * q) - 1)]
               for key, q in [("p95", .95), ("p99", .99)]}, "max": max(v)}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("directory", type=pathlib.Path)
    p.add_argument("--processor", default="trace_processor")
    p.add_argument("--package", default="art.capycanvas.brushbench")
    args = p.parse_args()
    summaries = []
    for infofile in sorted(args.directory.glob("*-info.json")):
        label = infofile.name.removesuffix("-info.json")
        if not (args.directory / f"{label}-complete.json").is_file():
            continue
        info = json.loads(infofile.read_text())
        runs = []
        markers = []
        for i in range(info["repeats"]):
            data = json.loads((args.directory / f"{label}-{i}.json").read_text())
            motion = data["motion"]
            begin, end = motion["begin_ns"], motion["end_ns"]
            markers += [f"stroke{i}_start {motion['begin_boot_ns']}", f"stroke{i}_end {motion['end_boot_ns']}"]
            fields = data["frame_fields"]
            callbacks = [dict(zip(fields, r)) for r in data["frames"] if begin <= r[1] < end]
            # Early retry callbacks do not submit a viewport; retain them in the
            # aggregate owner cost but exclude them from per-update quantiles.
            frames = [f for f in callbacks if f["queue_present_ns"] > 0]
            cpu = {field.removesuffix("_ns"): distribution([f[field] / 1e6 for f in frames])
                   for field in fields if field.endswith("_ns") and field not in ["vsync_ns", "start_ns", "expected_presentation_ns"]}
            inputs = [r for r in data["inputs"] if begin <= r[0] < end]
            rows_before = {r["label"]: r["value"] for r in data["renderer_before"]["rows"]}
            rows_after = {r["label"]: r["value"] for r in data["renderer_after"]["rows"]}
            progress = completion_window(data)
            submitted = progress["submitted"]
            stamps = int(rows_after["Dabs"]) - int(rows_before["Dabs"])
            times = [f["start_ns"] for f in frames]
            runs.append({"run": i, **progress, "cpu_update_count": len(frames),
                "callback_count": len(callbacks), "cpu_ms": cpu,
                "owner_core_occupancy": sum(f["owner_thread_cpu_ns"] for f in callbacks) / (end - begin),
                "update_start_gap_ms": distribution([(b - a) / 1e6 for a, b in zip(times, times[1:])]),
                "input_queue_ms": distribution([(r[2] - r[1]) / 1e6 for r in inputs]),
                "input_delivery_ms": distribution([(r[1] - r[0]) / 1e6 for r in inputs]),
                "input_injected": len(motion["injected"]), "input_records": sum(r[4] for r in inputs),
                "dabs": stamps, "dabs_per_submitted_update": stamps / max(1, submitted),
                "raster_updates": int(rows_after["Frames"]) - int(rows_before["Frames"]),
                "rolling_gpu_ms": distribution(data["renderer_after"]["gpu_samples"]),
                "viewport_gpu_ms": distribution([r[1] / 1e6 for r in data["presentation"] if r[2] == 1]),
                "resident_bytes": data["renderer_after"]["resident_bytes"],
                "process_mappings": data["resources_after"]["process_mappings"]})
        trace = args.directory / f"{label}.perfetto-trace"
        trace_data = None
        if trace.exists():
            markerfile = args.directory / f"{label}-markers.txt"
            markerfile.write_text("\n".join(markers) + "\n")
            reportfile = args.directory / f"{label}-trace-report.json"
            if not reportfile.exists():
                with reportfile.open("w") as out:
                    subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("android-pen-report.py")),
                        str(trace), str(markerfile), "--processor", args.processor, "--package", args.package],
                        check=True, stdout=out)
            trace_data = json.loads(reportfile.read_text())
        summary = {"label": label, "preset": info["preset"], "size": info["brush_size"], "mode": info["mode"],
            "prediction": info["prediction"], "speed": info["speed"], "trace": bool(trace_data),
            "trace_kind": info.get("host_trace_kind", "full" if trace_data else "none"),
            "completed_per_s_median": statistics.median(r["completed_per_s"] for r in runs),
            "completed_per_s_range": [min(r["completed_per_s"] for r in runs), max(r["completed_per_s"] for r in runs)],
            "runs": runs}
        if trace_data:
            summary["trace_errors"] = trace_data["trace_errors"]
            summary["trace_actions"] = trace_data["actions"]
        summaries.append(summary)
        print(f"{label:48} updates/s={summary['completed_per_s_median']:.1f} "
              f"CPU={statistics.median(r['cpu_ms']['cpu_callback']['p50'] for r in runs):.2f}ms "
              f"rolling GPU={statistics.median(r['rolling_gpu_ms'].get('p50',0) for r in runs):.2f}ms "
              f"dabs/update={statistics.median(r['dabs_per_submitted_update'] for r in runs):.1f}")
    (args.directory / "summary.json").write_text(json.dumps(summaries, indent=2))
    with (args.directory / "summary.csv").open("w", newline="") as out:
        columns = ["label", "preset", "size", "mode", "prediction", "trace", "speed", "completed_updates_per_s",
                   "minimum_run_updates_per_s", "maximum_run_updates_per_s", "cpu_callback_p50_ms",
                   "update_start_gap_p95_ms", "update_start_gap_p99_ms", "input_queue_p95_ms",
                   "rolling_gpu_p50_ms", "viewport_gpu_p50_ms", "dabs_per_update", "resident_mib"]
        writer = csv.DictWriter(out, fieldnames=columns)
        writer.writeheader()
        for s in summaries:
            median = lambda f: statistics.median(f(r) for r in s["runs"])
            def observed_median(field):
                values = [r[field]["p50"] for r in s["runs"] if r[field].get("n")]
                return statistics.median(values) if values else ""
            writer.writerow({**{k: s[k] for k in columns[:7]},
                "completed_updates_per_s": s["completed_per_s_median"],
                "minimum_run_updates_per_s": s["completed_per_s_range"][0],
                "maximum_run_updates_per_s": s["completed_per_s_range"][1],
                "cpu_callback_p50_ms": median(lambda r: r["cpu_ms"]["cpu_callback"]["p50"]),
                "update_start_gap_p95_ms": median(lambda r: r["update_start_gap_ms"]["p95"]),
                "update_start_gap_p99_ms": median(lambda r: r["update_start_gap_ms"]["p99"]),
                "input_queue_p95_ms": median(lambda r: r["input_queue_ms"]["p95"]),
                "rolling_gpu_p50_ms": observed_median("rolling_gpu_ms"),
                "viewport_gpu_p50_ms": observed_median("viewport_gpu_ms"),
                "dabs_per_update": median(lambda r: r["dabs_per_submitted_update"]),
                "resident_mib": median(lambda r: r["resident_bytes"] / 1048576)})


if __name__ == "__main__":
    main()
