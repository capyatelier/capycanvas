#!/usr/bin/env python3
"""Summarize raw browser/Android HDR qualification without hiding failed gates."""
import argparse
import json
import math
from pathlib import Path


def summary(values):
    values = sorted(values)
    if not values:
        return None
    return {"count": len(values), **{name: values[min(len(values)-1, math.floor((len(values)-1)*q+.5))]
            for name, q in [("p50", .5), ("p95", .95), ("p99", .99), ("max", 1)]}}


def browser(path):
    report = json.loads(path.read_text())
    rows = []
    for run in report["runs"]:
        samples = run.get("memory", []) + [run.get("cold", {}), run.get("concurrent", {})] + run.get("interactions", [])
        rows.append({"name": run["name"], "open_ms": run.get("open_ms"), "ready_ms": run.get("ready_ms"),
                     "peak_process_pss_bytes": max((s.get("pss_bytes") or 0 for s in samples), default=0),
                     "cold_heartbeat_ms": run.get("cold", {}).get("heartbeat_ms"),
                     "warm": [{k: s.get(k) for k in ("device", "submission_ms", "render_cpu_ms", "gpu_ms", "host_frame_ms", "heartbeat_ms", "renderer_bytes")} for s in run.get("interactions", [])],
                     "histogram_cancel": run.get("histogram_cancel"), "open_cancel_ms": run.get("open_cancel_ms"),
                     "export_cancel_ms": run.get("export_cancel_ms"), "error": run.get("error")})
    return {"source": str(path), "browser": report["browser"], "runs": rows}


def android(path):
    report = json.loads(path.read_text())
    rows = []
    for run in report["runs"]:
        row = {k: v for k, v in run.items() if k != "runs"}
        row["warm"] = []
        for motion in run["runs"]:
            t = motion["timeline"]
            inputs = [dict(zip(t["input_fields"], x)) for x in t["inputs"]]
            frames = [dict(zip(t["frame_fields"], x)) for x in t["frames"]]
            # CPU frame submission can be related to scheduled presentation;
            # SurfaceFlinger feedback has no per-input token. Do not manufacture
            # input-to-photon or exact input-to-present latency from this trace.
            row["warm"].append({"tool": motion["tool"], "input_cpu_ms": summary([i["cpu_input_ns"]/1e6 for i in inputs]),
                                "input_queue_ms": summary([(i["worker_start_ns"]-i["arrival_ns"])/1e6 for i in inputs]),
                                "event_to_arrival_ms": summary([(i["arrival_ns"]-i["event_ns"])/1e6 for i in inputs]),
                                "cpu_ms": motion["cpu_ms"], "gpu_ms": motion["gpu_ms"],
                                "cpu_frames_after_scheduled_present": sum(f["start_ns"]+f["cpu_render_present_ns"]>f["expected_presentation_ns"] for f in frames),
                                "frame_count": len(frames), "tracked_canvas_bytes": motion["tracked_canvas_bytes"]})
        rows.append(row)
    return {"source": str(path), "device": report["device"], "runs": rows}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    root = args.directory
    browser_names = ["browser-parent", "browser-final-performance", "browser-qualified", "browser-f32", "tablet-browser-qualified", "tablet-browser-f32"]
    result = {"caveats": ["PSS is sampled residency, not all GPU driver allocations; transient peaks can be missed.",
                           "Tablet Chrome PSS includes other existing tabs; the earlier tablet-browser-performance main-process-only samples are invalid as total memory.",
                           "Input CPU/queue and scheduled frame deadlines are not measured physical pen latency or exact input-to-present."],
              "browser": [browser(root/name/"performance.json") for name in browser_names if (root/name/"performance.json").exists()],
              "android": android(root/"android-final-performance.json")}
    print(json.dumps(result, indent=2))
