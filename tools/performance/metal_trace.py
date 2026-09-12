#!/usr/bin/env python3
"""Summarize one process's Instruments GPU execution intervals from local XML.

Export metal-gpu-intervals and metal-application-encoders-list from the same run.
GPU stages can overlap. Union intervals instead of summing stage durations or
calling CPU encoding/submission latency GPU time. This is diagnostic profiling;
command buffers and Instruments frame numbers are not app drawing-frame IDs.
"""
import argparse
import collections
import json
from pathlib import Path
import xml.etree.ElementTree as ET

from apple_trace import distribution


class Table:
    def __init__(self, path, schema):
        root = ET.parse(path).getroot()
        schemas = list(root.iter("schema"))
        if len(schemas) != 1 or schemas[0].get("name") != schema:
            raise ValueError("Expected exactly one " + schema + " table")
        columns = [col.findtext("mnemonic") for col in schemas[0].findall("col")]
        if None in columns or len(set(columns)) != len(columns):
            raise ValueError("Invalid or duplicate column names")
        self.ids = {}
        for element in root.iter():
            identity = element.get("id")
            if identity:
                if identity in self.ids:
                    raise ValueError("Duplicate XML value definition")
                self.ids[identity] = element
        self.rows = []
        for row in root.iter("row"):
            if len(row) != len(columns):
                raise ValueError("Row does not match the exported schema")
            self.rows.append(dict(zip(columns, (self.resolve(n) for n in row))))

    def resolve(self, element):
        seen = set()
        while element is not None and element.get("ref"):
            identity = element.get("ref")
            if identity in seen or identity not in self.ids:
                raise ValueError("Unresolved or cyclic XML value reference")
            seen.add(identity)
            element = self.ids[identity]
        return element

    def pid(self, row):
        value = self.resolve(row["process"].find("pid"))
        return None if value is None else int(value.text)


def integer(element):
    if element.tag == "sentinel":
        return None
    return int(element.text)


def text(element):
    return element.get("fmt", element.text or "")


def union_ns(intervals):
    ordered = sorted(intervals)
    total = 0
    if ordered:
        left, right = ordered[0]
        for start, end in ordered[1:]:
            if start > right:
                total += right - left
                left = start
            right = max(right, end)
        total += right - left
    return total


def summarize(gpu_path, encoder_path, pid):
    gpu = Table(gpu_path, "metal-gpu-intervals")
    cpu = Table(encoder_path, "metal-application-encoders-list")
    required = {"start", "duration", "process", "state", "channel-name", "encoder-id", "cmdbuffer-id", "start-latency"}
    if any(not required.issubset(row) for row in gpu.rows):
        raise ValueError("GPU table lacks required execution fields")
    if any(not {"process", "encoder-id", "encoder-label"}.issubset(row) for row in cpu.rows):
        raise ValueError("Encoder table lacks required identity fields")
    selected = [row for row in gpu.rows if gpu.pid(row) == pid]
    if not selected:
        raise ValueError("No GPU execution rows for the requested process")
    encoders = {}
    ambiguous_encoders = set()
    duplicate_encoders = 0
    for row in cpu.rows:
        if cpu.pid(row) == pid:
            key = integer(row["encoder-id"])
            if key is None:
                raise ValueError("CPU encoder has no identity")
            if key in encoders:
                duplicate_encoders += 1
                ambiguous_encoders.add(key)
            encoders[key] = text(row["encoder-label"])
    intervals, channels, by_encoder, by_buffer, by_label = [], collections.defaultdict(list), collections.defaultdict(list), collections.defaultdict(list), collections.defaultdict(list)
    invalid = 0
    missing_identity = 0
    latencies = []
    observed_encoders = set()
    unmatched = 0
    for row in selected:
        if text(row["state"]) != "Active":
            continue
        start, duration = integer(row["start"]), integer(row["duration"])
        if start is None or duration is None or start < 0 or duration <= 0:
            invalid += 1
            continue
        interval = (start, start + duration)
        intervals.append(interval)
        channels[text(row["channel-name"])].append(interval)
        encoder, buffer = integer(row["encoder-id"]), integer(row["cmdbuffer-id"])
        if encoder is None or buffer is None:
            missing_identity += 1
        if buffer is not None:
            by_buffer[buffer].append(interval)
        if encoder is not None:
            observed_encoders.add(encoder)
            by_encoder[encoder].append(interval)
        if encoder in encoders and encoder not in ambiguous_encoders:
            by_label[encoders[encoder]].append(interval)
        else:
            unmatched += 1
        latency = integer(row["start-latency"])
        if latency is not None and latency >= 0:
            latencies.append(latency)
    def summary(values):
        return {"intervals": len(values), "active_union_ms": union_ns(values) / 1e6,
                "raw_stage_sum_ms": sum(b - a for a, b in values) / 1e6,
                "stage_duration_ms": distribution(b - a for a, b in values)}
    warnings = ["Profiled GPU execution intervals are diagnostic. No calibrated instrumentation overhead or app-frame/presentation acceptance is established.",
                "The trace window can omit earlier CPU encoding or later GPU execution; retain unmatched identities and partial intervals."]
    if invalid or missing_identity or duplicate_encoders:
        warnings.append("Invalid intervals or incomplete/duplicate identities prevent complete coverage claims.")
    if unmatched or encoders.keys() - observed_encoders:
        warnings.append("CPU encoder and GPU execution coverage differ in this capture window.")
    if not intervals:
        warnings.append("No positive Active GPU execution intervals are available for this process.")
    return {"schema": 1, "pid": pid, "units": "milliseconds; source timestamps/durations are nanoseconds",
            "source_tables": [gpu_path.name, encoder_path.name],
            "total_gpu_rows": len(gpu.rows), "target_gpu_rows": len(selected),
            "other_process_rows": sum(gpu.pid(row) not in (None, pid) for row in gpu.rows),
            "unattributed_process_rows": sum(gpu.pid(row) is None for row in gpu.rows),
            "states": dict(collections.Counter(text(row["state"]) for row in selected)),
            "invalid_active_intervals": invalid, "intervals_missing_identity": missing_identity,
            "duplicate_cpu_encoder_rows": duplicate_encoders,
            "cpu_encoders": len(encoders), "gpu_encoders": len(observed_encoders),
            "cpu_encoders_without_gpu_intervals": len(encoders.keys() - observed_encoders),
            "gpu_intervals_without_cpu_encoder": unmatched,
            "gpu": summary(intervals),
            "observed_gpu_extent_ns": [min(a for a, _ in intervals), max(b for _, b in intervals)] if intervals else None,
            "channels": {key: summary(values) for key, values in sorted(channels.items())},
            "encoder_labels": {key: summary(values) for key, values in sorted(by_label.items())},
            "per_encoder_active_union_ms": distribution(union_ns(v) for v in by_encoder.values()),
            "per_command_buffer_active_union_ms": distribution(union_ns(v) for v in by_buffer.values()),
            "cpu_to_gpu_start_latency_ms": distribution(latencies), "warnings": warnings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gpu_xml", type=Path)
    parser.add_argument("encoder_xml", type=Path)
    parser.add_argument("--pid", type=int, required=True, help="Target process from this recording")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(summarize(args.gpu_xml, args.encoder_xml, args.pid), indent=2) + "\n")


if __name__ == "__main__":
    main()
