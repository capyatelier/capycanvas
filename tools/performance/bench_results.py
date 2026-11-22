#!/usr/bin/env python3
"""Append a bench chain run to docs/performance/results/<device>.csv."""
import argparse
import csv
import random
from pathlib import Path

from android_brush_metrics import quantile
from bench_analyze import analyze_run, statistic

HEADER = ("date", "run_id", "device", "tier", "host", "step", "version", "commit", "build", "metric", "unit",
          "evidence", "n", "median", "ci_low", "ci_high", "noise", "target", "status", "flag")
RESAMPLES = 1000
SEED = 1015


def bootstrap(kind, samples):
    rng = random.Random(SEED)
    values = sorted(value for value in (statistic(kind, rng.choices(samples, k=len(samples))) for _ in range(RESAMPLES))
                    if value is not None)
    return quantile(values, .025), quantile(values, .975)


def cell(value):
    return None if value is None else round(value, 3)


def rows(summary, device, data):
    for step_id, step in data["steps"].items():
        for name, metric in (step["metrics"] or {"": {"versions": {}}}).items():
            for version in summary["versions"]:
                result = metric["versions"].get(version)
                problem = step["problems"].get(version, {}).get("status")
                if result is None and problem not in ("skipped", "error"):
                    continue
                build = data["builds"].get(version, {})
                row = dict(date=summary["date"], run_id=summary["run_id"], device=device, tier=data["tier"],
                           host=build.get("host"), step=step_id, version=version, commit=build.get("commit"),
                           build=build.get("build") or build.get("app_id"), metric=name)
                if result is None:
                    yield dict(row, n=0, status="unsupported" if problem == "skipped" else "error")
                    continue
                low, high = bootstrap(metric["kind"], result["samples"])
                yield dict(row, unit=metric["unit"], evidence=result["evidence"], n=result["n"], median=cell(result["value"]),
                           ci_low=cell(low), ci_high=cell(high), noise=cell(metric["noise"]), target=cell(metric["target"]),
                           status=result["status"], flag=result.get("flag"))


def publish(run_dir: Path, repo_root: Path) -> list[Path]:
    summary = analyze_run(Path(run_dir))
    written = []
    for device, data in summary["devices"].items():
        path = Path(repo_root) / "docs" / "performance" / "results" / f"{device}.csv"
        path.parent.mkdir(parents=True, exist_ok=True)
        fresh = not path.exists()
        with path.open("a", newline="") as stream:
            writer = csv.DictWriter(stream, HEADER)
            if fresh:
                writer.writeheader()
            writer.writerows(rows(summary, device, data))
        written.append(path)
    return written


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=Path)
    for path in publish(parser.parse_args().run_dir, Path(__file__).resolve().parents[2]):
        print(path)
