#!/usr/bin/env python3
"""Require one visible destination layout in each instrumented workspace switch."""

import argparse
import csv
import json
import sys
from pathlib import Path


def frames_for(folder):
    with (folder / "frames.csv").open(newline="", encoding="utf-8-sig") as file:
        return list(csv.DictReader(file))


def layout(event):
    groups = []
    for group in event.get("groups", []):
        actual = group.get("actual") or {}
        if (group.get("loaded") and not group.get("hidden")
                and group.get("opacity", 0) > 0
                and actual.get("width", 0) > 0 and actual.get("height", 0) > 0):
            groups.append((group["id"], tuple(round(float(actual[axis]), 1)
                                               for axis in ("x", "y", "width", "height"))))
    return tuple(sorted(groups))


def check_switch(folder, events):
    timing = json.loads((folder / "input.json").read_text(encoding="utf-8-sig"))
    invoked, frequency = int(timing["invoked"]), int(timing["frequency"])
    rows = [row for row in frames_for(folder) if int(row["begin"]) >= invoked]
    first = next((row for row in rows if int(row["old_diff"]) > 1000), None)
    settled = next((row for i, row in enumerate(rows)
                    if all(int(later["final_diff"]) <= 100 for later in rows[i:])), None)
    target = folder.name.split("-", 1)[1]
    destination = "builtin:workspace:" + target
    relevant = [event for event in events if event.get("kind") == "rendering"
                and event.get("workspace") == destination
                and invoked <= event.get("qpc", -1) <= (int(rows[-1]["end"]) if rows else invoked)]
    result = {"operation": folder.name, "destination": destination,
              "rendering_samples": len(relevant), "layout_count": 0,
              "first_final_geometry_qpc": None, "first_final_geometry_after_invoke_ms": None,
              "layouts": [], "pass": False}
    if not rows or first is None or settled is None:
        result["error"] = "capture has no first major change or strict settled frame"
        return result
    if not relevant or relevant[0]["qpc"] > int(first["end"]) or relevant[-1]["qpc"] < int(settled["end"]):
        result["error"] = "Rendering trace does not span first change through strict settlement"
        return result
    last_at_settlement = None
    for event in relevant:
        key = layout(event)
        if not key:
            continue
        if event["qpc"] <= int(settled["end"]):
            last_at_settlement = key
        if not result["layouts"] or key != result["layouts"][-1]["key"]:
            result["layouts"].append({"qpc": event["qpc"], "key": key})
    result["layout_count"] = len(result["layouts"])
    if result["layouts"]:
        first_final = result["layouts"][-1]["qpc"]
        result["first_final_geometry_qpc"] = first_final
        result["first_final_geometry_after_invoke_ms"] = round((first_final - invoked) * 1000 / frequency, 3)
    if result["layouts"] and last_at_settlement != result["layouts"][-1]["key"]:
        result["error"] = "final visible geometry was not observed by strict settlement"
        return result
    result["pass"] = result["layout_count"] == 1
    if not result["pass"]:
        result["error"] = f"expected one visible destination layout, observed {result['layout_count']}"
    return result


def check_run(run):
    trace = run / "audit-native-details.jsonl"
    if not trace.is_file():
        return [{"run": str(run), "pass": False, "error": "missing audit-native-details.jsonl"}]
    events = []
    with trace.open(encoding="utf-8-sig") as file:
        for line in file:
            if line.strip():
                event = json.loads(line)
                if event.get("kind") == "rendering":
                    events.append(event)
    events.sort(key=lambda event: event["qpc"])
    switches = [folder for folder in sorted(run.iterdir()) if folder.is_dir()
                and folder.name[:2].isdigit() and int(folder.name[:2]) > 0
                and folder.name.split("-", 1)[-1] in ("painter", "photographer")
                and (folder / "input.json").is_file() and (folder / "frames.csv").is_file()]
    if not switches:
        return [{"run": str(run), "pass": False, "error": "no switch capture folders"}]
    return [dict(run=str(run), **check_switch(folder, events)) for folder in switches]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runs", type=Path, nargs="+", help="run folders with audit-native-details.jsonl and switch captures")
    parser.add_argument("--output", type=Path, help="also write the JSON result to this path")
    args = parser.parse_args()
    operations = [result for run in args.runs for result in check_run(run)]
    result = {"pass": bool(operations) and all(row["pass"] for row in operations),
              "operations": operations, "time_scope": "CompositionTarget.Rendering callback QPC; not screenshot capture or presentation time"}
    encoded = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    sys.stdout.write(encoded)
    return 0 if result["pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
