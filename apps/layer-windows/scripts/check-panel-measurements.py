#!/usr/bin/env python3
import argparse
import csv
import json
import math
from pathlib import Path


def check(path, scale):
    if not math.isfinite(scale) or not 0 < scale <= 16:
        raise ValueError("Invalid raster scale")
    failures = []
    panels = set()
    count = 0
    with path.open(encoding="utf-8-sig", newline="") as source:
        reader = csv.DictReader(source)
        required = {"qpc", "panel", "request_width", "reported_width"}
        if (not reader.fieldnames or len(reader.fieldnames) != len(set(reader.fieldnames))
                or not required.issubset(reader.fieldnames)):
            raise ValueError("Missing or duplicate measurement columns")
        for number, row in enumerate(reader, 2):
            try:
                qpc = int(row["qpc"])
                panel = row["panel"]
                requested = float(row["request_width"])
                reported = float(row["reported_width"])
                if (qpc <= 0 or not panel or not math.isfinite(requested)
                        or not math.isfinite(reported) or requested <= 0 or reported <= 0):
                    raise ValueError()
            except (ValueError, TypeError) as error:
                raise ValueError(f"Invalid measurement at line {number}") from error
            count += 1
            panels.add(panel)
            error_px = abs(requested - reported) * scale
            if error_px > 1:
                failures.append({"qpc": qpc, "panel": panel, "requested": requested,
                                 "reported": reported, "error_px": error_px})
    if not count:
        raise ValueError("No panel measurements")
    return {"pass": not failures, "measurements": count, "panels": sorted(panels),
            "raster_scale": scale, "tolerance_px": 1, "failures": failures}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--raster-scale", type=float, required=True)
    args = parser.parse_args()
    try:
        result = check(args.input, args.raster_scale)
    except (OSError, ValueError, csv.Error) as error:
        parser.exit(2, f"Invalid panel measurement input: {error}\n")
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result["pass"] else 1)


if __name__ == "__main__":
    main()
