#!/usr/bin/env python3
"""Correlate one Metal capture with the same process's native frame JSONL.

Use clock anchors exported from that capture, not wall-clock approximations.
CPU encoder intervals must fit wholly inside a serial native frame. GPU identity
joins retain execution after CPU completion, and overlapping stages are unioned.
Unmatched work stays visible; an observed match is not proof of full coverage.
"""
import argparse
import bisect
import collections
from fractions import Fraction
import json
from pathlib import Path

from apple_trace import distribution
from metal_trace import Table, integer, text, union_ns


def clock_offset(path):
    table = Table(path, "time-info")
    offsets = set()
    for row in table.rows:
        fields = [integer(table.resolve(field)) for field in row["timebase-info"]]
        if len(fields) != 2 or any(v is None or v <= 0 for v in fields):
            raise ValueError("Invalid mach timebase")
        epoch, update = integer(row["mabs-epoch"]), integer(row["update-time"])
        if epoch is None or update is None or epoch < 0 or update < 0:
            raise ValueError("Invalid clock anchor")
        offsets.add(Fraction(epoch * fields[0], fields[1]) - update)
    if len(offsets) != 1:
        raise ValueError("Missing or changing clock anchors; split the capture at clock changes")
    return offsets.pop()


def native_frames(path, pid):
    with path.open() as source:
        header = json.loads(next(source))
        events = [json.loads(line) for line in source if line.strip()]
    if header.get("schema") != 1 or header.get("clock") != "CACurrentMediaTime nanoseconds":
        raise ValueError("Unsupported native trace clock/schema")
    if header.get("process_identifier", pid) != pid:
        raise ValueError("Native trace process does not match requested PID")
    if any(len(row) != 11 or any(type(v) is not int or v < 0 for v in row) for row in events):
        raise ValueError("Invalid native trace record")
    frames = sorted((r for r in events if r[0] == 1), key=lambda r: r[3])
    if not frames or len({r[1] for r in frames}) != len(frames):
        raise ValueError("Missing or duplicate native frame identities")
    if any(r[4] < r[3] for r in frames) or any(a[4] > b[3] for a, b in zip(frames, frames[1:])):
        raise ValueError("Invalid or overlapping serial frame intervals")
    presented = collections.defaultdict(list)
    for row in events:
        if row[0] == 4:
            presented[row[1]].append(row[2])
    return header, frames, presented


def correlate(gpu_path, encoder_path, time_path, frame_path, pid):
    offset = clock_offset(time_path)
    header, frames, presented = native_frames(frame_path, pid)
    starts = [r[3] for r in frames]
    gpu = Table(gpu_path, "metal-gpu-intervals")
    cpu = Table(encoder_path, "metal-application-encoders-list")
    encoders = collections.defaultdict(list)
    counts = collections.Counter()
    for row in cpu.rows:
        if cpu.pid(row) == pid:
            encoders[integer(row["encoder-id"])].append(row)
    matched, by_frame = {}, collections.defaultdict(set)
    for identity, rows in encoders.items():
        ambiguous = identity is None or identity <= 0 or len(rows) != 1
        if ambiguous:
            counts["ambiguous_cpu_encoder_rows"] += len(rows)
        for row in rows:
            start, duration = integer(row["start"]), integer(row["duration"])
            if start is None or duration is None or start < 0 or duration < 0:
                counts["invalid_cpu_encoder_intervals"] += 1
                continue
            begin, end = offset + start, offset + start + duration
            index = bisect.bisect_right(starts, begin) - 1
            if index < 0 or end > frames[index][4]:
                counts["cpu_encoders_outside_native_frames"] += 1
                continue
            frame = frames[index][1]
            by_frame[frame].add(identity)
            if not ambiguous:
                matched[identity] = (frame, integer(row["cmdbuffer-id"]), start + duration)
    intervals = collections.defaultdict(list)
    observed = set()
    invalid_encoders = set()
    for row in gpu.rows:
        if gpu.pid(row) != pid:
            counts["other_or_unattributed_gpu_rows"] += 1
            continue
        counts["target_gpu_rows"] += 1
        if text(row["state"]) != "Active":
            continue
        start, duration = integer(row["start"]), integer(row["duration"])
        identity = integer(row["encoder-id"])
        if start is None or duration is None or start < 0 or duration <= 0:
            counts["invalid_active_gpu_intervals"] += 1
            invalid_encoders.add(identity)
            continue
        if identity not in matched:
            counts["active_gpu_intervals_without_native_frame"] += 1
            continue
        frame, buffer, encoding_end = matched[identity]
        if buffer is None or buffer <= 0 or integer(row["cmdbuffer-id"]) != buffer or start < encoding_end:
            counts["gpu_identity_or_order_conflicts"] += 1
            invalid_encoders.add(identity)
            continue
        observed.add(identity)
        intervals[frame].append((start, start + duration))
        counts["matched_active_gpu_intervals"] += 1
    result = []
    for native in frames:
        identity = native[1]
        if identity not in by_frame:
            continue
        rows = intervals[identity]
        expected = by_frame[identity]
        missing = expected - observed
        invalid = expected & invalid_encoders
        display = presented[identity]
        # Duplicate/multiple/zero-time presentations cannot supply a unique
        # endpoint. Keep this distinct from missing CPU/GPU work.
        shown = display[0] if len(display) == 1 and display[0] > 0 else None
        gpu_end = offset + max((b for _, b in rows), default=0) if rows else None
        result.append({"frame": identity, "cpu_owner_service_ms": (native[4] - native[3]) / 1e6,
            "cpu_encoders": len(expected), "cpu_encoders_without_gpu": len(missing),
            "encoders_with_invalid_gpu_intervals": len(invalid),
            "all_observed_encoders_have_valid_gpu": not missing and not invalid,
            "active_gpu_intervals": len(rows),
            "gpu_active_union_ms": union_ns(rows) / 1e6 if rows else None,
            "gpu_stage_sum_ms": sum(b - a for a, b in rows) / 1e6 if rows else None,
            "frame_admission_to_present_ms": (shown - identity) / 1e6 if shown is not None else None,
            "gpu_end_to_present_ms": float((shown - gpu_end) / 1e6) if shown is not None and gpu_end is not None else None})
    if not result:
        raise ValueError("No CPU encoder fits a native frame; verify process, clock and capture pairing")
    # Partial frames remain in the individual rows and counts. Do not count them
    # as zero-work frames or fold them into the covered-observation distribution.
    covered = [r for r in result if r["all_observed_encoders_have_valid_gpu"]]
    warnings = ["Observed encoder coverage does not prove complete frame coverage; capture windows or trace loss can omit both CPU and GPU work.",
        "GPU timing is profiled execution, not a calibrated uninstrumented cost. Presentation association does not establish physical input-to-pixel latency."]
    if "process_identifier" not in header:
        warnings.append("Native trace has no PID field; the caller must verify that both captures belong to the same process.")
    if header.get("dropped_records", 0):
        warnings.append("Native recorder overflow; frame coverage is incomplete.")
    if any(r["gpu_end_to_present_ms"] is not None and r["gpu_end_to_present_ms"] < 0 for r in result):
        warnings.append("Some associated GPU work ends after presentation; do not interpret negative intervals as presentation latency.")
    return {"schema": 1, "pid": pid, "clock_offset_ns": {"numerator": offset.numerator, "denominator": offset.denominator},
        "sources": [path.name for path in [gpu_path, encoder_path, time_path, frame_path]],
        "counts": {key: counts[key] for key in ["target_gpu_rows", "other_or_unattributed_gpu_rows",
            "ambiguous_cpu_encoder_rows", "invalid_cpu_encoder_intervals", "cpu_encoders_outside_native_frames",
            "invalid_active_gpu_intervals", "gpu_identity_or_order_conflicts",
            "active_gpu_intervals_without_native_frame", "matched_active_gpu_intervals"]},
        "native_frames": len(frames), "frames_with_cpu_encoders": len(result),
        "frames_with_all_observed_encoders_matched": len(covered),
        "observed_covered_frame_gpu_active_ms": distribution((r["gpu_active_union_ms"] for r in covered), divisor=1),
        "frames": result, "warnings": warnings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["gpu_xml", "encoder_xml", "time_xml", "frame_jsonl"]:
        parser.add_argument(name, type=Path)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = correlate(args.gpu_xml, args.encoder_xml, args.time_xml, args.frame_jsonl, args.pid)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
