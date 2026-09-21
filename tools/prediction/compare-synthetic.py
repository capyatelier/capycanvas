#!/usr/bin/env python3
"""Compare CAPY_PREDICTION_SCORE=1 engine test logs without dropping hard cases.

Run each revision with --nocapture --test-threads=1. Counts use the same
actuals-relative error-step definition and physical pixel bands as the replay API.
Usage: compare-synthetic.py BEFORE_LOG AFTER_LOG OUTPUT_JSON [--check]
--check rejects aggregate regressions for each algorithm separately, including
small flicker, accuracy, severe errors, prediction coverage and useful lead.
"""
import argparse
import csv
import json
import math
from pathlib import Path


FIELDS = ("queries", "transitions", "tiny", "qualifying", "large", "severe",
          "extreme", "error_squared", "step_squared", "lead_sum_ms",
          "moving_queries", "predicted_queries", "worst_jump_px")


def read(path):
    rows = {}
    for line in Path(path).read_text().splitlines():
        if "prediction-score," not in line:
            continue
        fields = next(csv.reader([line[line.index("prediction-score,"):]]))
        if len(fields) != 22:
            raise ValueError("Truncated/interleaved score; rerun with --test-threads=1")
        key = tuple(fields[1:9])
        if key in rows:
            raise ValueError(f"Duplicate case: {key}")
        row = dict(zip(FIELDS, map(float, fields[9:])))
        assert all(math.isfinite(v) for v in row.values())
        assert 0 <= row["extreme"] <= row["severe"] <= row["large"] <= row["qualifying"]
        assert row["transitions"] == row["queries"] - 1
        assert 0 <= row["predicted_queries"] <= row["moving_queries"]
        rows[key] = row
    if not rows:
        raise ValueError(f"No prediction scores: {path}")
    return rows


def aggregate(rows):
    sums = {field: sum(row[field] for row in rows) for field in FIELDS}
    pairs, queries = sums["transitions"], sums["queries"]
    moving = sums["moving_queries"]
    return dict(
        cases=len(rows), queries=int(queries), transitions=int(pairs),
        tiny=int(sums["tiny"]), small=int(sums["qualifying"] - sums["large"]),
        medium=int(sums["large"] - sums["severe"]), severe=int(sums["severe"]),
        extreme=int(sums["extreme"]), qualifying=int(sums["qualifying"]),
        qualifying_rate_percent=100 * sums["qualifying"] / pairs,
        error_rms_px=math.sqrt(sums["error_squared"] / queries),
        step_rms_px=math.sqrt(sums["step_squared"] / pairs),
        mean_lead_ms=sums["lead_sum_ms"] / moving,
        coverage=sums["predicted_queries"] / moving,
        worst_jump_px=max(row["worst_jump_px"] for row in rows),
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    before, after = map(read, [args.before, args.after])
    if before.keys() != after.keys():
        raise ValueError(f"Case mismatch: {len(before.keys()-after.keys())} lost, "
                         f"{len(after.keys()-before.keys())} added; no silent exclusions")
    for key in before:
        for field in ("queries", "transitions", "moving_queries"):
            assert before[key][field] == after[key][field], (key, field)
    groups = {}
    for family in ["all"] + sorted({key[0] for key in before}):
        for algorithm in ["all"] + sorted({key[-1] for key in before}):
            keys = [k for k in before if (family == "all" or k[0] == family)
                    and (algorithm == "all" or k[-1] == algorithm)]
            if keys:
                groups[f"{family}/{algorithm}"] = {
                    name: aggregate([rows[k] for k in keys])
                    for name, rows in [("before", before), ("after", after)]}
    output = dict(groups=groups, matched_cases=len(before),
                  severe_case_regressions=[dict(case=k, before=before[k], after=after[k])
                      for k in before if after[k]["severe"] > before[k]["severe"]])
    if args.check:
        checks = {}
        for algorithm in sorted({key[-1] for key in before}):
            pair = groups[f"all/{algorithm}"]
            b, a = pair["before"], pair["after"]
            for field in ("tiny", "qualifying", "severe", "extreme", "error_rms_px",
                          "step_rms_px", "worst_jump_px"):
                checks[f"{algorithm}/{field}"] = a[field] <= b[field] + 1e-9
            checks[f"{algorithm}/medium_and_severe"] = a["medium"] + a["severe"] <= b["medium"] + b["severe"]
            checks[f"{algorithm}/mean_lead_ms"] = a["mean_lead_ms"] >= b["mean_lead_ms"] * 0.99
            checks[f"{algorithm}/coverage"] = a["coverage"] >= b["coverage"] - 0.001
        output["regression_checks"] = checks
    args.output.write_text(json.dumps(output, indent=2) + "\n")
    print(json.dumps(groups["all/all"], indent=2))
    if args.check and not all(output["regression_checks"].values()):
        raise SystemExit("Synthetic regression checks failed: " + ", ".join(
            key for key, passed in output["regression_checks"].items() if not passed))


if __name__ == "__main__":
    main()
