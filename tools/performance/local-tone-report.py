#!/usr/bin/env python3
"""Correlate applied local-tone recipes with actual Wayland feedback.

Every request remains in the denominator. Coalesced/unchanged recipes are
reported, not counted as instantaneous responses. No frame-cadence proxy.
"""
import bisect
import collections
import importlib.util
import json
import math
from pathlib import Path
import sys

spec = importlib.util.spec_from_file_location("navigation", Path(__file__).with_name("photo-navigation-report.py"))
navigation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(navigation)
distribution = navigation.distribution


def summarize(report):
    requests = report["requests"]
    by_recipe = collections.defaultdict(list)
    for index, request in enumerate(requests):
        by_recipe[json.dumps(request["recipe"], sort_keys=True)].append((request["requested_ns"], index))
    cpu = {int(row[0]): row for row in report["worker_cpu"]}
    feedback = {row[0]: row for row in report["canvas_presentation"]}
    matched = set()
    matched_presentation_times = []
    latencies, windows = [], collections.defaultdict(list)
    first = requests[0]["requested_ns"]
    for frame, recipe in sorted(report["hdr_views"], key=lambda row: feedback.get(row[0], [0, 0])[1]):
        if frame not in cpu or frame not in feedback or not feedback[frame][3]:
            continue
        candidates = by_recipe[json.dumps(recipe, sort_keys=True)]
        position = bisect.bisect_right(candidates, (cpu[frame][4], float("inf"))) - 1
        if position < 0:
            continue
        requested, index = candidates[position]
        if index in matched:
            continue
        elapsed = (feedback[frame][1] - requested) / 1e6
        assert elapsed >= 0
        matched.add(index)
        matched_presentation_times.append(feedback[frame][1])
        latencies.append(elapsed)
        windows[int((requested-first)/1e9)//10].append(elapsed)
    measured = sorted((p for f, p in feedback.items() if p[3] and f in cpu and cpu[f][4] >= first), key=lambda p: p[1])
    missed = sum(max(0, math.floor((b[1]-a[1])/b[2]+.5)-1) for a, b in zip(measured, measured[1:]) if b[2])
    work = report["camera_work"]
    summary = dict(requests=len(requests), presented_requests=len(matched),
        unchanged_requests=sum(a["recipe"] == b["recipe"] for a, b in zip(requests, requests[1:])),
        unmatched_requests=len(requests)-len(matched), request_to_present=distribution(latencies),
        first_request_to_first_present_ms=(min(matched_presentation_times)-first)/1e6 if matched_presentation_times else None,
        missed_refresh_slots=missed, missed_fraction=missed/(report["seconds"]*120),
        input_cpu=distribution([r["cpu_ms"] for r in requests]),
        worker_cpu=distribution([r[3] for r in report["worker_cpu"]]),
        worker_gpu=distribution([r[1] for r in report["worker_gpu"]]),
        schedule_lateness=distribution([r["lateness_ms"] for r in requests]),
        ten_second_windows={str(k*10): distribution(v) for k, v in windows.items()},
        source_misses=sum(r[4] for r in work),
        composited_pixel_increase=max(r[1] for r in work)-min(r[1] for r in work),
        canvas_ready_ms=report["canvas_ready_ms"], guide_ready_ms=report["guide_ready_ms"],
        renderer_resident_bytes=report["renderer_resident_bytes"], final_memory=report["final_memory"],
        operations=report.get("operations"))
    # Retain raw evidence before reporting a failed predeclared gate.
    budget = 33.4 if report["concurrent"] else 20.
    summary["latency_budget_ms"] = budget
    summary["latency_pass"] = bool(latencies) and distribution(latencies)["p99_ms"] <= budget
    summary["missed_slots_pass"] = summary["missed_fraction"] <= (.02 if report["concurrent"] else .01)
    summary["no_artwork_rebuild_pass"] = summary["source_misses"] == 0 and summary["composited_pixel_increase"] == 0
    summary["worker_budget_pass"] = report["concurrent"] or all(
        summary[k]["p99_ms"] <= 2. for k in ["worker_cpu", "worker_gpu"])
    summary["renderer_memory_pass"] = report["renderer_resident_bytes"] <= 2 * 1024**3
    return summary


if __name__ == "__main__":
    path = Path(sys.argv[1])
    raw = json.loads(path.read_text())
    report = summarize(raw)
    memory = path.with_suffix(".memory.json")
    samples = json.loads(memory.read_text()) if memory.exists() else []
    report["process_memory_pass"] = bool(samples) and max(s["process_tree_rss_kib"] for s in samples) <= (3 if raw["concurrent"] else 2) * 1024**2
    if samples:
        report["sampled_peak_process_tree_kib"] = max(s["process_tree_rss_kib"] for s in samples)
        report["sampled_peak_staging_bytes"] = max(s["staging_bytes"] for s in samples)
        report["sampled_peak_driver_mib"] = max(int(s["gpu_memory"].split()[0]) for s in samples)
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if all(report[k] for k in ["latency_pass", "missed_slots_pass", "no_artwork_rebuild_pass", "worker_budget_pass", "renderer_memory_pass", "process_memory_pass"]) else 1)
