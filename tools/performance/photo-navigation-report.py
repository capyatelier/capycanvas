#!/usr/bin/env python3
"""Summarize a native_large_photo_navigation JSON capture; no external packages."""
import collections
import json
import math
import sys


def distribution(values):
    values = sorted(values)
    if not values:
        return {"n": 0}
    return {
        "n": len(values),
        **{
            name: values[math.ceil((len(values) - 1) * q)]
            for name, q in [("p50_ms", 0.5), ("p95_ms", 0.95), ("p99_ms", 0.99)]
        },
        "max_ms": values[-1],
        "over_120hz_budget": sum(v > 1000 / 120 for v in values),
    }


def summarize(report):
    cpu = {int(row[0]): row for row in report["worker_cpu"]}
    presented = {row[0]: row for row in report["canvas_presentation"]}
    first_request = min(r["requested_ns"] for r in report["requests"])
    # Feedback from a startup frame can arrive after Stats is cleared. Its old
    # presentation timestamp must not turn pre-gesture idle time into missed
    # navigation refreshes. Keep unmatched requests and the first response below.
    measured_feedback = {
        frame: p for frame, p in presented.items()
        if frame in cpu and cpu[frame][4] >= first_request
    }
    by_matrix = collections.defaultdict(list)
    for request in report["requests"]:
        by_matrix[tuple(request["matrix"])].append(request)
    phases = collections.defaultdict(list)
    matched = set()
    latency = []
    enqueue_delay = []
    queued_to_present = []
    first_response = None
    # A burst timer can present the final pose again while going idle. Input
    # latency ends at its first presentation; repeated display is still cadence.
    views = sorted(report["camera_views"], key=lambda v: presented.get(v[0], [0, 0])[1])
    for frame, matrix, _revision in views:
        if frame not in cpu or frame not in presented or not presented[frame][3]:
            continue
        # Repeated poses can have identical matrices. Select the last matching
        # request admitted before this frame was enqueued, never a later request.
        request = next(
            (r for r in reversed(by_matrix[tuple(matrix)]) if r["requested_ns"] <= cpu[frame][4]),
            None,
        )
        if request is None or request["requested_ns"] in matched:
            continue
        elapsed = (presented[frame][1] - request["requested_ns"]) / 1e6
        assert elapsed >= 0, "presentation/request clocks must share a monotonic origin"
        phases[request["phase"]].append(elapsed)
        latency.append(elapsed)
        enqueue_delay.append((cpu[frame][4] - request["requested_ns"]) / 1e6)
        queued_to_present.append((presented[frame][1] - cpu[frame][4]) / 1e6)
        first_response = min(first_response or presented[frame][1], presented[frame][1])
        matched.add(request["requested_ns"])
    feedback = sorted((p for p in measured_feedback.values() if p[3]), key=lambda p: p[1])
    cadence = [(b[1] - a[1]) / 1e6 for a, b in zip(feedback, feedback[1:])]
    # Small compositor timestamp jitter around 8.333 ms is not a lost refresh.
    # Retain raw cadence percentiles as well as gaps rounded to reported slots.
    missed_slots = sum(
        max(0, math.floor((b[1] - a[1]) / b[2] + 0.5) - 1)
        for a, b in zip(feedback, feedback[1:]) if b[2]
    )
    return {
        "extent": report["extent"], "viewport": report["viewport"],
        "space": report["space"], "depth": report["depth"],
        "gtk_renderer": report["gtk_renderer"],
        "requests": len(report["requests"]),
        "presented_requests": len(matched),
        "requests_without_matching_presentation": len(report["requests"]) - len(matched),
        "requests_without_matching_presentation_by_phase": dict(collections.Counter(
            r["phase"] for r in report["requests"] if r["requested_ns"] not in matched
        )),
        "discarded_feedback": sum(not p[3] for p in measured_feedback.values()),
        "feedback_outside_measured_frames": len(presented) - len(measured_feedback),
        "missed_refresh_slots": missed_slots,
        "first_request_to_first_present_ms": (
            (first_response - first_request) / 1e6 if first_response is not None else None
        ),
        "request_to_present": distribution(latency),
        "request_to_enqueue": distribution(enqueue_delay),
        "enqueue_to_present": distribution(queued_to_present),
        "request_to_present_by_phase": {phase: distribution(v) for phase, v in phases.items()},
        "worker_cpu": distribution([r[3] for r in report["worker_cpu"]]),
        "worker_gpu": distribution([r[1] for r in report["worker_gpu"]]),
        "frame_handler_cpu": distribution(report["frame_handler_cpu"]),
        "presentation_cadence": distribution(cadence),
        "request_schedule_lateness": distribution([r["lateness_ms"] for r in report["requests"]]),
    }


if __name__ == "__main__":
    with open(sys.argv[1], encoding="utf-8") as source:
        print(json.dumps(summarize(json.load(source)), indent=2))
