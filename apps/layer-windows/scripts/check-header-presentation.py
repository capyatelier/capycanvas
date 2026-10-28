#!/usr/bin/env python3
import argparse
import json
import math
from pathlib import Path


EVENT_FIELDS = ("Kind", "Id", "PriorShown", "Target", "Loaded", "Opacity",
                "AnimationsEnabled", "CurrentGeometry", "WorkspaceId", "ModelRevision", "Qpc")
WINDOW_FIELDS = ("name", "kind", "workspace", "begin_qpc", "end_qpc")
WINDOW_KINDS = {"switch", "zen_hide", "zen_reveal"}


def rows(path):
    with path.open(encoding="utf-8-sig") as source:
        for number, line in enumerate(source, 1):
            if line.strip():
                try:
                    row = json.loads(line)
                except json.JSONDecodeError as error:
                    raise ValueError(f"{path}:{number}: {error.msg}") from error
                if not isinstance(row, dict):
                    raise ValueError(f"{path}:{number}: expected a JSON object")
                yield row


def integer(value, minimum=0):
    return type(value) is int and value >= minimum


def validate_event(event):
    missing = set(EVENT_FIELDS) - event.keys()
    if missing:
        raise ValueError(f"HeaderFadeDecision missing {', '.join(sorted(missing))}")
    if (event["Kind"] not in ("Bar", "Item", "Overflow")
            or not integer(event["Id"])
            or type(event["PriorShown"]) is not int or event["PriorShown"] not in (-1, 0, 1)
            or any(type(value) not in (int, bool) or value not in (0, 1) for value in
                   (event["Target"], event["Loaded"], event["AnimationsEnabled"], event["CurrentGeometry"]))
            or type(event["Opacity"]) not in (int, float)
            or not math.isfinite(event["Opacity"]) or not 0 <= event["Opacity"] <= 1
            or not isinstance(event["WorkspaceId"], str)
            or not integer(event["ModelRevision"])
            or not integer(event["Qpc"], 1)):
        raise ValueError("Invalid HeaderFadeDecision fields")


def validate_windows(windows):
    if not windows:
        raise ValueError("No operation windows")
    names = set()
    for window in windows:
        missing = set(WINDOW_FIELDS) - window.keys()
        if missing:
            raise ValueError(f"Operation window missing {', '.join(sorted(missing))}")
        if (not isinstance(window["name"], str) or not window["name"]
                or window["name"] in names):
            raise ValueError("Duplicate or empty operation name")
        names.add(window["name"])
        if (not isinstance(window["kind"], str) or window["kind"] not in WINDOW_KINDS
                or not isinstance(window["workspace"], str) or not window["workspace"]
                or not integer(window["begin_qpc"], 1)
                or not integer(window["end_qpc"], 1)
                or window["begin_qpc"] >= window["end_qpc"]):
            raise ValueError(f"Invalid operation window {window['name']}")
    windows.sort(key=lambda window: window["begin_qpc"])
    if any(before["end_qpc"] > after["begin_qpc"] for before, after in zip(windows, windows[1:])):
        raise ValueError("Overlapping operation windows")
    return windows


def check(events, windows):
    windows = validate_windows(list(windows))
    selected = {window["name"]: [] for window in windows}
    for event in events:
        validate_event(event)
        for window in windows:
            if window["begin_qpc"] <= event["Qpc"] < window["end_qpc"]:
                if event["WorkspaceId"] == window["workspace"]:
                    selected[window["name"]].append(event)
                break
    failures = []
    summary = []
    for window in windows:
        decisions = selected[window["name"]]
        animated = lambda event: event["Loaded"] == 1 and event["AnimationsEnabled"] == 1
        reveals = [event for event in decisions if animated(event)
                   and event["PriorShown"] == 0 and event["Target"] == 1]
        hides = [event for event in decisions if animated(event)
                 and event["PriorShown"] == 1 and event["Target"] == 0]
        if not decisions:
            failures.append(f"{window['name']}: no matching header decisions")
        elif not any(event["CurrentGeometry"] == 1 for event in decisions):
            failures.append(f"{window['name']}: no decision on current geometry")
        if window["kind"] == "switch" and reveals:
            failures.append(f"{window['name']}: {len(reveals)} loaded animated fade-ins during switch")
        if window["kind"] == "zen_hide" and not any(event["CurrentGeometry"] == 1 for event in hides):
            failures.append(f"{window['name']}: no intentional loaded fade-out on current geometry")
        if window["kind"] == "zen_reveal" and not any(event["CurrentGeometry"] == 1 for event in reveals):
            failures.append(f"{window['name']}: no intentional loaded fade-in on current geometry")
        summary.append({"name": window["name"], "kind": window["kind"],
                        "decisions": len(decisions), "animated_reveals": len(reveals),
                        "animated_hides": len(hides)})
    return {"pass": not failures, "windows": summary, "failures": failures}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--events", type=Path, required=True)
    parser.add_argument("--windows", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = check(rows(args.events), rows(args.windows))
    except (OSError, ValueError) as error:
        parser.exit(2, f"Invalid header presentation input: {error}\n")
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result["pass"] else 1)


if __name__ == "__main__":
    main()
