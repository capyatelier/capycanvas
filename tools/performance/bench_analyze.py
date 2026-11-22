#!/usr/bin/env python3
"""Summarize bench chain passes per device and flag differences from the first listed version."""
import argparse
import bisect
import itertools
import json
import math
import statistics
from datetime import date
from pathlib import Path

from android_brush_metrics import quantile

TRIM_NS = 100_000_000
PRESENTED_WITHIN_NS = 2_000_000_000
LOW_MEMORY_KIB = 1 << 20
TIER_RATES = {"low": 60, "mid": 90, "top": 120}
TIERS = {"tcl": ("low", 60.0), "movinkpad11": ("mid", 90.0), "movinkpad14": ("top", 120.0), "sm_n970u1": ("low", 60.0)}
MEMORY = (("os_available_min_kib", "os_available_kib", min), ("app_rss_max_kib", "app_rss_kib", max),
          ("gpu_max_kib", "gpu_kib", max))
UNITS = {"fps": "fps", "s": "1/s", "ms": "ms", "kib": "KiB", "bytes": "bytes"}


def statistic(kind, samples):
    if not samples:
        return None
    if kind == "rate":
        return 1000 * len(samples) / sum(samples)
    if kind == "ratio":
        count, ms = map(sum, zip(*samples))
        return 1000 * count / ms if ms else None
    return quantile(samples, .99) if kind == "p99" else statistics.median(samples)


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()] if path.exists() else []


def gaps_ms(times):
    return [(b - a) / 1e6 for a, b in zip(times, times[1:])]


def between(times, begin, end):
    return times[bisect.bisect_left(times, begin):bisect.bisect_right(times, end)]


def evidence_of(parts):
    return "+".join(sorted({part for joined in parts if joined for part in joined.split("+")}))


def response(record, shown, shown_evidence):
    if record.get("evidence") == "completed":
        return (record["ready_ns"] - record["input_ns"]) / 1e6, "completed"
    index = bisect.bisect_left(shown, record["ready_ns"])
    if index < len(shown) and shown[index] - record["ready_ns"] <= PRESENTED_WITHIN_NS:
        return (shown[index] - record["input_ns"]) / 1e6, shown_evidence
    return (record["ready_ns"] - record["input_ns"]) / 1e6, "completed"


def step_metrics(step, frames, memory, first_use, shown_evidence="presented"):
    surface = step.get("surface") or "canvas"
    windows = step.get("motion") or []
    contacts = step.get("contacts") or []
    done = sorted(step.get("completions") or [], key=lambda row: row["complete_ns"])
    metrics = {}

    def add(name, kind, samples, evidence):
        if samples:
            metrics[name] = {"kind": kind, "samples": samples, "evidence": evidence}

    shown = frames.get(surface, [])
    intervals = [gap for w in windows for gap in gaps_ms(between(shown, w["begin_ns"] + TRIM_NS, w["end_ns"] - TRIM_NS))]
    add("fps", "rate", intervals, shown_evidence)
    add("gap_p99_ms", "p99", intervals, shown_evidence)
    if contacts and done:
        times = [row["complete_ns"] for row in done]
        held = [between(times, contact["down_ns"], contact["up_ns"]) for contact in contacts]
        add("updates_per_s", "ratio", [[len(t), (c["up_ns"] - c["down_ns"]) / 1e6] for c, t in zip(contacts, held)], "completed")
        add("update_gap_p99_ms", "p99", [gap for t in held for gap in gaps_ms(t)], "completed")
        downs = [next(((row["complete_ns"] - c["down_ns"]) / 1e6 for row in done
                       if row["complete_ns"] >= c["down_ns"] and row["input_ns"] >= c["down_ns"]), None) for c in contacts]
        if first_use:
            add("first_use_pen_down_ms", "median", [value for value in downs[:1] if value is not None], "completed")
            downs = downs[1:]
        add("pen_down_ms", "median", [value for value in downs if value is not None], "completed")
    ends = [contact["up_ns"] for contact in contacts] or [window["end_ns"] for window in windows]
    if step.get("settled_ns") and ends:
        add("catch_up_ms", "median", [(step["settled_ns"] - max(ends)) / 1e6], "completed")
    records = [response(record, frames.get(record.get("surface") or surface, []), shown_evidence)
               for record in step.get("latency") or [] if record.get("ready_ns") is not None]
    add("latency_ms", "median", [value for value, _ in records], evidence_of(evidence for _, evidence in records))
    rows = [row for row in memory if step["begin_ns"] <= row["t_ns"] <= step["end_ns"]]
    for name, field, pick in MEMORY:
        values = [row[field] for row in rows if row.get(field) is not None]
        add(name, "median", values and [pick(values)], "sampled")
    return metrics


def read_pass(directory):
    info = json.loads((directory / "pass.json").read_text())
    frames = {}
    for row in read_jsonl(directory / "frames.jsonl"):
        frames.setdefault(row.get("surface", "canvas"), set()).add(row["t_ns"])
    frames = {surface: sorted(times) for surface, times in frames.items()}
    memory = read_jsonl(directory / "memory.jsonl")
    steps = {}
    for step in info.get("steps", []):
        if step.get("status") != "ok":
            steps[step["id"]] = {"status": step.get("status"), "reason": step.get("reason"), "metrics": {}}
            continue
        cold = "cold_" if info.get("cold") and (step["id"] == "launch" or step["id"].startswith("open.")) else ""
        metrics = step_metrics(step, frames, memory, info.get("pass") == 1, info.get("frames_evidence", "presented"))
        steps[step["id"]] = {"status": "ok", "metrics": {cold + name: metric for name, metric in metrics.items()}}
    end = {name: {"kind": "median", "samples": [value], "evidence": "sampled"}
           for name, value in (info.get("memory_end") or {}).items() if value is not None}
    if end:
        steps["memory_end"] = {"status": "ok", "metrics": end}
    return info, steps


def one_shot(step_id, name):
    return name.removeprefix("cold_") == "latency_ms" and (step_id in ("launch", "save", "switch.resume") or step_id.startswith("open."))


def rule(step_id, name, base, few, rate, period):
    metric = name.removeprefix("cold_")
    budget = 2000 / rate
    if metric in ("fps", "updates_per_s"):
        target = rate * (.95 if metric == "fps" else 1)
        return True, target, target, .05 * base
    if metric in ("gap_p99_ms", "update_gap_p99_ms"):
        return False, budget, budget, period
    if metric.endswith("pen_down_ms"):
        return False, budget, budget, max(.1 * base, period)
    if one_shot(step_id, name):
        return False, None, None, max(.2 * base, 200) if few else .1 * base
    if metric in ("latency_ms", "catch_up_ms"):
        return False, 100, None, max(.15 * base, 20)
    if metric == "os_available_min_kib":
        return True, LOW_MEMORY_KIB, None, math.inf
    return False, None, None, .1 * base


def meets(value, limit, higher):
    return value >= limit if higher else value <= limit


def judge(step_id, name, base, new, noise, rate, period):
    if base["value"] is None or new["value"] is None:
        return None, None
    delta = new["value"] - base["value"]
    few = one_shot(step_id, name) and min(base["n"], new["n"]) <= 2
    higher, limit, _, threshold = rule(step_id, name, base["value"], few, rate, period)
    percent = 100 * delta / base["value"] if base["value"] else None
    crossed = limit is not None and meets(base["value"], limit, higher) != meets(new["value"], limit, higher)
    if delta == 0 or not crossed and abs(delta) < threshold:
        return percent, None
    if not few and (noise is None or abs(delta) <= 2 * noise):
        return percent, "ambiguous"
    return percent, "improved" if (delta > 0) == higher else "regressed"


def compare(step_id, name, by_version, versions, rate, period):
    results = {version: {"n": len(slot["samples"]), "value": statistic(slot["kind"], slot["samples"]),
                         "passes": slot["passes"], "evidence": evidence_of(slot["evidence"]), "samples": slot["samples"]}
               for version, slot in by_version.items()}
    spreads = [statistics.mean(abs(a - b) for a, b in itertools.combinations(values, 2))
               for values in ([value for value in result["passes"] if value is not None] for result in results.values())
               if len(values) > 1]
    noise = statistics.mean(spreads) if spreads else None
    higher, _, target, _ = rule(step_id, name, 0, False, rate, period)
    base = results.get(versions[0])
    for version, result in results.items():
        result["status"] = "n/a" if target is None or result["value"] is None else "met" if meets(result["value"], target, higher) else "miss"
        if base is not None and result is not base:
            result["delta_pct"], result["flag"] = judge(step_id, name, base, result, noise, rate, period)
    kind = next(iter(by_version.values()))["kind"]
    return {"unit": UNITS.get(name.rsplit("_", 1)[-1], ""), "kind": kind, "target": target, "noise": noise,
            "versions": {version: results[version] for version in versions if version in results}}


def device_summary(device, versions, runs):
    first = next(info for version in versions for info, _ in runs[version])
    tier, refresh = TIERS.get(device, ("top", 120.0))
    tier, refresh = first.get("tier") or tier, first.get("refresh_hz") or refresh
    rate = TIER_RATES[tier]
    steps = {}
    for version in versions:
        for _, pass_steps in runs[version]:
            for step_id, step in pass_steps.items():
                entry = steps.setdefault(step_id, {"problems": {}, "metrics": {}})
                if step["status"] != "ok":
                    entry["problems"].setdefault(version, {"status": step["status"], "reason": step["reason"]})
                for name, metric in step["metrics"].items():
                    slot = entry["metrics"].setdefault(name, {}).setdefault(
                        version, {"kind": metric["kind"], "samples": [], "passes": [], "evidence": []})
                    slot["samples"] += metric["samples"]
                    slot["passes"].append(statistic(metric["kind"], metric["samples"]))
                    slot["evidence"].append(metric["evidence"])
    for step_id, entry in steps.items():
        entry["metrics"] = {name: compare(step_id, name, by_version, versions, rate, 1000 / refresh)
                            for name, by_version in entry["metrics"].items()}
    builds = {version: {key: runs[version][0][0].get(key) for key in ("host", "commit", "build", "app_id")}
              for version in versions if runs[version]}
    return {"tier": tier, "target_hz": rate, "refresh_hz": refresh, "builds": builds, "steps": steps}


def pass_dirs(directory):
    return sorted((path for path in directory.glob("pass-*") if (path / "pass.json").exists()),
                  key=lambda path: int(path.name.split("-", 1)[1]))


def analyze_run(run_dir: Path) -> dict:
    run_dir = Path(run_dir)
    run = json.loads((run_dir / "run.json").read_text())
    versions = run["versions"]
    devices = {}
    for device in run["devices"]:
        runs = {version: [read_pass(path) for path in pass_dirs(run_dir / device / version)] for version in versions}
        if any(runs.values()):
            devices[device] = device_summary(device, versions, runs)
    summary = {"run_id": run_dir.name, "versions": versions, "devices": devices,
               "date": run.get("date") or date.fromtimestamp((run_dir / "run.json").stat().st_mtime).isoformat()}
    (run_dir / "summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    return summary


def fmt(value, spec=".1f"):
    return "-" if value is None else format(value, spec)


def format_table(summary: dict) -> str:
    versions = summary["versions"]
    blocks = []
    for device, data in summary["devices"].items():
        rows = [["step", "metric", *versions, "Δ%", "flag"]]
        for step_id, step in data["steps"].items():
            for name, metric in (step["metrics"] or {"-": {"versions": {}}}).items():
                results = metric["versions"]
                others = [results.get(version, {}) for version in versions[1:]]
                rows.append([step_id, name,
                             *(fmt(results[version]["value"]) if version in results
                               else step["problems"].get(version, {}).get("status", "-") for version in versions),
                             "/".join(fmt(result.get("delta_pct"), "+.1f") for result in others),
                             "/".join(result.get("flag") or "-" for result in others)])
        widths = [max(len(row[column]) for row in rows) for column in range(len(rows[0]))]
        blocks.append(f"{device}: {data['tier']} tier, {data['target_hz']} fps target, {data['refresh_hz']:g} Hz display\n"
                      + "\n".join("  ".join(cell.ljust(width) for cell, width in zip(row, widths)).rstrip() for row in rows))
    return "\n\n".join(blocks)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=Path)
    print(format_table(analyze_run(parser.parse_args().run_dir)))
