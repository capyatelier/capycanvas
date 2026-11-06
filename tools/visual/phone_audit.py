#!/usr/bin/env python3
"""Compare paired GTK/Web phone captures and their actual widget bounds, or candidate captures against a reference manifest."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import sys


def load(path):
    value = json.loads(path.read_text())
    if value["schema_version"] != 1:
        raise ValueError(f"Unsupported audit schema: {path}")
    for key in ("host", "state", "viewport", "dpr", "theme", "text_scale", "preset", "status", "phone_active"):
        if key not in value:
            raise ValueError(f"Missing metadata {key}: {path}")
    elements = {}
    for element in value["elements"]:
        key = element["id"]
        if not isinstance(key, str) or not key or not isinstance(element["role"], str) or not element["role"] or type(element["visible"]) is not bool:
            raise ValueError(f"Invalid element identity/visibility: {path}")
        if key in elements:
            raise ValueError(f"Duplicate element {key}: {path}")
        bounds = element["bounds"]
        if bounds is None and element["visible"] is False:
            elements[key] = element
            continue
        if not isinstance(bounds, list) or len(bounds) != 4 or not all(type(v) in (int, float) and math.isfinite(v) for v in bounds) or min(bounds[2:]) < 0:
            raise ValueError(f"Invalid bounds for {key}: {path}")
        if element["visible"] and min(bounds[2:]) <= 0:
            raise ValueError(f"Empty visible allocation for {key}: {path}")
        elements[key] = element
    return value, elements


def compare(reference, candidate):
    gtk, left = load(reference)
    web, right = load(candidate)
    host_roles = {"native-reservation", "window-move"}
    host_elements = {"gtk": [element for element in left.values() if element["role"] in host_roles],
        "web": [element for element in right.values() if element["role"] in host_roles]}
    left = {key: element for key, element in left.items() if element["role"] not in host_roles}
    right = {key: element for key, element in right.items() if element["role"] not in host_roles}
    metadata = [key for key in ("state", "viewport", "dpr", "theme", "text_scale", "preset") if gtk.get(key) != web.get(key)]
    if gtk.get("host") != "gtk" or web.get("host") != "web":
        metadata.append("host")
    missing = sorted(set(left) ^ set(right) | set(gtk.get("missing", [])) | set(web.get("missing", [])))
    differences = []
    for key in sorted(set(left) & set(right)):
        a, b = left[key], right[key]
        delta = [y - x for x, y in zip(a["bounds"], b["bounds"])] if a["bounds"] is not None and b["bounds"] is not None else []
        if a["visible"] != b["visible"] or a["role"] != b["role"] or any(abs(d) > 1 for d in delta):
            differences.append({"id": key, "gtk": a, "web": b, "delta": delta})
    unavailable = [entry.get("host") for entry in (gtk, web) if entry.get("status") != "captured" or not entry.get("phone_active")]
    return {"metadata_mismatches": metadata, "missing": missing, "unavailable": unavailable,
        "host_elements": host_elements,
        "allocation_differences": differences, "tolerance_pixels": 1,
        "passed": not (metadata or missing or differences or unavailable) and bool(left) and bool(right)}


def image_path(path):
    suffix = ".allocations.json"
    return path.with_name(path.name[:-len(suffix)] + ".png") if path.name.endswith(suffix) else path.with_suffix(".png")


def compare_pixels(reference, candidate, output):
    output.mkdir(parents=True, exist_ok=True)
    result = subprocess.run([sys.executable, str(Path(__file__).with_name("compare.py")),
        str(reference), str(candidate), "--output", str(output), "--sheet"], capture_output=True, text=True)
    if result.returncode not in (0, 1) or not result.stdout.strip():
        raise ValueError(result.stderr.strip() or "Pixel comparison failed")
    return json.loads(result.stdout)


def compare_pair(gtk, web, output):
    geometry = compare(gtk, web)
    pixels = compare_pixels(image_path(gtk), image_path(web), output)
    metadata, _ = load(gtk)
    expected = [round(value * metadata["dpr"]) for value in metadata["viewport"]]
    if pixels["dimensions"] != expected:
        raise ValueError(f"Capture dimensions {pixels['dimensions']} do not match viewport/DPR {expected}")
    report = {"gtk": str(gtk), "web": str(web), "geometry": geometry,
        "pixels": pixels, "passed": geometry["passed"] and pixels["passed"]}
    (output / "phone-comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def index(directory):
    records = {}
    for path in sorted(directory.rglob("*.allocations.json")):
        value, _ = load(path)
        key = json.dumps([value["state"], value["viewport"], float(value["dpr"]), value["theme"],
            float(value["text_scale"]), value["preset"]], separators=(",", ":"))
        if key in records:
            raise ValueError(f"Duplicate capture state: {records[key]} and {path}")
        records[key] = path
    return records


def compare_matrix(gtk, web, output):
    left, right = index(gtk), index(web)
    reports = []
    for key in sorted(left.keys() & right.keys()):
        state, viewport, _, theme, scale, _ = json.loads(key)
        stem = re.sub(r"[^A-Za-z0-9_.-]", "_", f"{viewport[0]}x{viewport[1]}-{theme}-{scale:g}-{state}")[:120]
        directory = output / (stem + "-" + hashlib.sha256(key.encode()).hexdigest()[:8])
        result = compare_pair(left[key], right[key], directory)
        reports.append({"key": json.loads(key), "report": str(directory / "phone-comparison.json"), "passed": result["passed"]})
    manifest = web / "index.json"
    unavailable = json.loads(manifest.read_text()).get("unreachable", []) if manifest.exists() else []
    reference_manifest = gtk / "index.json"
    reference_failures = json.loads(reference_manifest.read_text()).get("failed_cases", []) if reference_manifest.exists() else []
    report = {"states": reports, "unpaired_gtk": [str(left[key]) for key in sorted(left.keys() - right.keys())],
        "unpaired_web": [str(right[key]) for key in sorted(right.keys() - left.keys())],
        "unavailable_states": unavailable, "failed_reference_cases": reference_failures,
        "passed": bool(reports) and not (unavailable or reference_failures) and all(result["passed"] for result in reports) and left.keys() == right.keys()}
    output.mkdir(parents=True, exist_ok=True)
    (output / "matrix.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def capture_key(value):
    return (value["family"], value["state"], value["case"], value["theme"], float(value["text_size"]), value["fixture"])


def compare_manifest(manifest, candidates, output):
    references = {}
    for entry in sorted(json.loads(manifest.read_text())["references"], key=lambda entry: entry.get("duplicate_of") is not None):
        references.setdefault(capture_key(entry), entry)
    states = []
    for path in sorted(candidates.rglob("*.json")):
        value = json.loads(path.read_text())
        if not isinstance(value, dict) or value.get("host") != "android" or value.get("schema_version") != 1:
            continue
        state = {key: value[key] for key in ("family", "case", "state", "theme", "text_size", "fixture")}
        state["candidate"] = str(path.with_suffix(".png"))
        entry = references.get(capture_key(value))
        if entry is None:
            state["status"] = "missing"
        else:
            directory = output / value["family"] / value["case"] / value["state"]
            state["reference"] = entry["png"]
            try:
                pixels = compare_pixels(Path(entry["png"]), path.with_suffix(".png"), directory)
                state.update(status="compared", sheet=str((directory / "sheet.png").relative_to(output)),
                             different_fraction=pixels["exact_different_fraction"], passed=pixels["passed"])
            except ValueError as error:
                state.update(status="error", error=str(error))
        states.append(state)
    output.mkdir(parents=True, exist_ok=True)
    report = {"manifest": str(manifest), "candidates": str(candidates), "states": states,
              "passed": bool(states) and all(state.get("passed") is True for state in states)}
    (output / "index.json").write_text(json.dumps(report, indent=2) + "\n")
    rows = ["| Family | Case | State | Status | Different pixels | Sheet |", "| --- | --- | --- | --- | --- | --- |"]
    for state in states:
        fraction = f"{state['different_fraction']:.4f}" if "different_fraction" in state else ""
        sheet = f"[sheet]({state['sheet']})" if "sheet" in state else state.get("error", "")
        rows.append(f"| {state['family']} | {state['case']} | {state['state']} | {state['status']} | {fraction} | {sheet} |")
    (output / "index.md").write_text("\n".join(rows) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gtk", nargs="?", type=Path, help="GTK allocation JSON or capture directory")
    parser.add_argument("web", nargs="?", type=Path, help="Web allocation JSON or capture directory")
    parser.add_argument("--reference", type=Path, help="Reference manifest JSON")
    parser.add_argument("--candidate", type=Path, help="Candidate capture directory")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        if args.reference or args.candidate:
            if not (args.reference and args.candidate) or args.gtk or args.web:
                raise ValueError("Use --reference with --candidate and no GTK/Web captures")
            report = compare_manifest(args.reference, args.candidate, args.output)
        elif not (args.gtk and args.web):
            raise ValueError("Provide GTK and Web captures, or --reference with --candidate")
        elif args.gtk.is_dir() != args.web.is_dir():
            raise ValueError("Provide two allocation JSON files or two capture directories")
        else:
            report = compare_matrix(args.gtk, args.web, args.output) if args.gtk.is_dir() else compare_pair(args.gtk, args.web, args.output)
        print(json.dumps(report, indent=2))
        return 0 if report["passed"] else 1
    except (ValueError, KeyError, OSError, TypeError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    raise SystemExit(main())
