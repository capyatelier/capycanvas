#!/usr/bin/env python3
"""Build, run, analyze and publish the bench chain; see docs/performance/measuring.md#bench-chain."""
import argparse
import json
import os
from pathlib import Path
import time

ROOT = Path(__file__).resolve().parents[2]
CHAIN = Path(__file__).with_name("bench-chain.json")
RUNS = Path.home() / "capy-bench/runs"
PASS_MINUTES = 3


def pass_order(versions, passes, quick=False):
    return [{"version": version, "pass": round + 1, "cold": round == 0 and not quick}
            for round in range(passes) for version in (versions if round % 2 == 0 else versions[::-1])]


def step_closure(steps, wanted):
    ids = [step["id"] for step in steps]
    unknown = sorted(set(wanted) - set(ids))
    if unknown:
        raise ValueError("Unknown steps: " + ", ".join(unknown))
    keep, opens, recent = set(wanted), {}, None
    for step in steps:
        if step["do"] == "launch":
            keep.add(step["id"])
        elif step["do"] == "open":
            opens[step["document"]] = recent = step["id"]
        elif step["id"] in wanted:
            keep.update(filter(None, [recent, *(opens.get(tab) for tab in step.get("tabs", []))]))
    return [identity for identity in ids if identity in keep]


def split(text):
    return list(dict.fromkeys(item.strip() for item in text.split(",") if item.strip()))


def document_files(chain, steps, photo, fixtures):
    from bench_fixtures import file_name
    names = dict.fromkeys(step["document"] for step in chain["steps"] if step["do"] == "open" and (steps is None or step["id"] in steps))
    files, missing = {}, []
    for name in names:
        fixture = chain["documents"][name].get("fixture")
        if fixture:
            path, source = fixtures and Path(fixtures) / file_name(fixture), f"--fixtures DIR with {file_name(fixture)}"
        else:
            path, source = photo and Path(photo), "--photo"
        if not path or not path.is_file():
            missing.append(f"{name} ({path or source})")
        files[name] = path
    if missing:
        raise ValueError("Missing documents: " + ", ".join(missing))
    return files


def check_free_disk(device, free_bytes, files, chain, copies=1):
    need = copies * sum(Path(path).stat().st_size for path in files) + chain.get("free_disk_gib", 0) * (1 << 30)
    if free_bytes < need:
        raise SystemExit(f"{device} has {free_bytes / (1 << 30):.1f} GiB free but this chain needs {need / (1 << 30):.1f} GiB "
                         f"({copies}x the documents and {chain.get('free_disk_gib', 0)} GiB for the app); free some space first")


def build(args):
    import bench_builds
    for version in split(args.versions):
        bench_builds.build(version, args.dry_run)


def run(args, parser):
    import bench_android
    import bench_builds
    devices = split(args.devices)
    unknown = [device for device in devices if device not in bench_android.DEVICES]
    if unknown:
        parser.error(f"unknown devices {', '.join(unknown)}; known: {', '.join(bench_android.DEVICES)}")
    if not args.processor or not os.access(args.processor, os.X_OK):
        parser.error("pass --processor or set CAPY_TRACE_PROCESSOR to trace_processor_shell")
    if args.passes is not None and args.passes < 1:
        parser.error("--passes must be at least 1")
    if any(bench_android.DEVICES[device]["adaptive"] for device in devices) and not args.adaptive_script:
        parser.error("movinkpad11 needs --adaptive-script to switch on its 90 Hz Adaptive policy")
    chain = json.loads(args.chain.read_text())
    try:
        steps = step_closure(chain["steps"], split(args.steps)) if args.steps else None
    except ValueError as error:
        parser.error(str(error))
    builds = {build["label"]: build for build in map(bench_builds.build, split(args.versions))}
    order = pass_order(list(builds), args.passes or (1 if args.quick else chain.get("passes", 2)), args.quick)
    now = time.localtime()
    run_dir = RUNS / time.strftime("%Y%m%d-%H%M%S", now)
    adaptive = dict(script=str(args.adaptive_script), aapt=str(args.aapt)) if args.adaptive_script else None
    directories = [bench_android.plan(device, run_dir, args.chain, builds.values(), order, steps, adaptive) for device in devices]
    record = {"date": time.strftime("%Y-%m-%d", now), "run_id": run_dir.name, "versions": list(builds),
              "builds": {label: {"commit": build["commit"], "apk_sha256": build["sha256"], "app_id": build["app_id"]} for label, build in builds.items()},
              "devices": {device: {key: bench_android.DEVICES[device][key] for key in ("tier", "refresh_hz")} for device in devices},
              "chain": str(args.chain), "chain_sha256": bench_builds.sha256(args.chain)}
    (run_dir / "run.json").write_text(json.dumps(record, indent=2) + "\n")
    print(f"{run_dir}: {len(order)} passes on each of {len(devices)} devices, about {len(order) * chain.get('pass_minutes', PASS_MINUTES)} min", flush=True)
    started = time.monotonic()
    codes = bench_android.run_devices(directories, args.processor)
    print(f"finished in {(time.monotonic() - started) / 60:.1f} min; " +
          ", ".join(f"{device} {'ok' if code == 0 else 'failed'}" for device, code in codes.items()), flush=True)
    for directory in directories:
        session = json.loads((directory / "session.json").read_text())
        display = session.get("display", {})
        record["devices"][directory.name].update(display_before=display.get("before"), display_restored=display.get("after"),
                                                 free_disk_kib=session.get("free_disk_kib"))
    (run_dir / "run.json").write_text(json.dumps(record, indent=2) + "\n")
    analyze(argparse.Namespace(run_dir=run_dir))
    return int(any(codes.values()))


def analyze(args):
    from bench_analyze import analyze_run, format_table
    print(format_table(analyze_run(args.run_dir)))


def publish(args):
    from bench_results import publish
    for path in publish(args.run_dir, ROOT):
        print(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(required=True, metavar="command")
    versions = dict(required=True, help="comma-separated tags or git refs, first is the baseline (v1.0.15,origin/main); one version is judged on the tier targets alone")
    command = commands.add_parser("build", help="build and cache each version's benchmark APK")
    command.add_argument("--versions", **versions)
    command.add_argument("--dry-run", action="store_true", help="print the commands without running them")
    command.set_defaults(handler=build)
    command = commands.add_parser("run", help="run the chain on devices in palindrome pass order, then analyze")
    command.add_argument("--versions", **versions)
    command.add_argument("--devices", required=True, help="comma-separated devices.py names (movinkpad14,sm_n970u1)")
    command.add_argument("--chain", type=Path, default=CHAIN, help="chain file (default bench-chain.json; bench-gigapixel.json is the gigapixel chain)")
    command.add_argument("--quick", action="store_true", help="one warm pass per version, no cold pass")
    command.add_argument("--steps", help="run only these steps, with the launch and open steps they need")
    command.add_argument("--passes", type=int, help="passes per version (default: the chain's passes, else 2; 1 with --quick)")
    command.add_argument("--processor", default=os.environ.get("CAPY_TRACE_PROCESSOR"),
                         help="Perfetto trace_processor_shell (default: $CAPY_TRACE_PROCESSOR)")
    command.add_argument("--adaptive-script", type=Path, help="Adaptive refresh helper for movinkpad11 (see android_adaptive.py)")
    command.add_argument("--aapt", type=Path, default=Path(os.environ.get("ANDROID_HOME") or Path.home() / "Android/Sdk") / "build-tools/37.0.0/aapt",
                         help="aapt for the Adaptive helper (default: ANDROID_HOME build-tools 37.0.0)")
    command.set_defaults(handler=lambda args: run(args, parser))
    for name, handler, summary in [("analyze", analyze, "summarize a run and print the comparison table"),
                                   ("publish", publish, "append a run's rows to docs/performance/results/<device>.csv")]:
        command = commands.add_parser(name, help=summary)
        command.add_argument("run_dir", type=Path)
        command.set_defaults(handler=handler)
    args = parser.parse_args()
    raise SystemExit(args.handler(args))


if __name__ == "__main__":
    main()
