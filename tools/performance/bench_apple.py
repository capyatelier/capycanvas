#!/usr/bin/env python3
"""Run the bench chain in Capy Canvas for macOS, one bundle per version, in palindrome order.

The chain runs inside the app (CAPY_WORKLOAD=chain). Each pass writes pass.json,
frames.jsonl and memory.jsonl to its output directory in the app container and
the app quits. Passes are collected into
~/capy-bench/runs/<run_id>/<device>/<version>/pass-<n>/ with run.json beside them.

Build one Release bundle per version, each with its own bundle id, from a worktree of
that version (not under /tmp). A version that predates the chain runner needs the
runner and a release-native rendering-pending probe. For v1.0.15 apply the exact
bench-patches/v1.0.15-apple.patch overlay to the release instead of the generic
runner diff below; it includes the regenerated project:
  git worktree add --detach ../capy-bench-src/LABEL VERSION && cd ../capy-bench-src/LABEL
  git diff RUNNER^ RUNNER -- apps/layer-apple ':!apps/layer-apple/CapyCanvas.xcodeproj' | git apply -3
  python3 apps/layer-apple/scripts/project.py
  xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj -scheme CapyCanvas-Mac \\
    -configuration Release -destination 'platform=macOS,arch=arm64' \\
    -derivedDataPath apps/layer-apple/DerivedData/Bench \\
    CAPY_APPLE_BUNDLE_ID=art.capycanvas.bench.LABEL CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES build
  # -> apps/layer-apple/DerivedData/Bench/Build/Products/Release/CapyCanvas-Mac.app

One Mac pass by hand (macOS creates the container on the bundle's first launch; open and quit it once):
  C=~/Library/Containers/art.capycanvas.bench.LABEL/Data
  mkdir -p $C/Documents/capy-bench
  cp tools/performance/bench-chain.json $C/Documents/capy-bench/chain.json
  cp PHOTO.jpg $C/Documents/capy-bench/photo.jpg
  cp FIXTURES/heavy.capy FIXTURES/vector.capy $C/Documents/capy-bench/
  open -n --env CAPY_WORKLOAD=chain --env CAPY_WORKLOAD_CHAIN=Documents/capy-bench/chain.json \\
    --env CAPY_WORKLOAD_OUTPUT=Documents/capy-bench/out/pass-1 --env CAPY_WORKLOAD_PASS=1 \\
    --env CAPY_WORKLOAD_COLD=1 --env CAPY_WORKLOAD_PHOTO=Documents/capy-bench/photo.jpg \\
    --env CAPY_WORKLOAD_HEAVY=Documents/capy-bench/heavy.capy \\
    --env CAPY_WORKLOAD_VECTOR=Documents/capy-bench/vector.capy --env CAPY_TRACE_GPU=0 \\
    --env CAPY_WORKLOAD_DEVICE=macmini --env CAPY_WORKLOAD_VERSION=LABEL \\
    APP.app --args -ApplePersistenceIgnoreState YES
  # Wait for $C/Documents/capy-bench/out/pass-1/pass.json. A warm pass sets
  # CAPY_WORKLOAD_COLD=0 and CAPY_WORKLOAD_ROOT to the "root" that pass 1 recorded.

Two versions on the Mac (A B B A):
  python3 tools/performance/bench_apple.py --device macmini --photo PHOTO.jpg \\
    --fixtures ~/capy-bench/fixtures/WxH --bundle origin/main=A.app --bundle CHANGE=B.app

The gigapixel chain, main only, one cold pass (fixtures from bench_fixtures.py gigapixel):
  python3 tools/performance/bench_apple.py --device macmini --chain tools/performance/bench-gigapixel.json \\
    --fixtures /mnt/localssd/capy-bench/fixtures/gigapixel --bundle origin/main=A.app
Each chain document is staged as Documents/capy-bench/NAME.EXT and named by CAPY_WORKLOAD_NAME:
a document with a fixture comes from --fixtures, the others from --photo.

One iPad pass (Release build with a signing team; keep the iPad unlocked, the app in front):
  xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj -scheme CapyCanvas-iPad \\
    -configuration Release -destination 'generic/platform=iOS' \\
    -derivedDataPath apps/layer-apple/DerivedData/BenchIPad \\
    CAPY_APPLE_BUNDLE_ID=art.capycanvas.bench.LABEL DEVELOPMENT_TEAM=TEAM build
  xcrun devicectl device install app --device DEVICE \\
    apps/layer-apple/DerivedData/BenchIPad/Build/Products/Release-iphoneos/CapyCanvas-iPad.app
  for f in tools/performance/bench-chain.json:chain.json PHOTO.jpg:photo.jpg FIXTURES/heavy.capy:heavy.capy \\
      FIXTURES/vector.capy:vector.capy; do
    xcrun devicectl device copy to --device DEVICE --domain-type appDataContainer \\
      --domain-identifier art.capycanvas.bench.LABEL --source ${f%%:*} --destination Documents/capy-bench/${f##*:}
  done
  xcrun devicectl device process launch --device DEVICE --terminate-existing --environment-variables \\
    '{"CAPY_WORKLOAD":"chain","CAPY_WORKLOAD_CHAIN":"Documents/capy-bench/chain.json",
      "CAPY_WORKLOAD_OUTPUT":"Documents/capy-bench/out/pass-1","CAPY_WORKLOAD_PASS":"1","CAPY_WORKLOAD_COLD":"1",
      "CAPY_WORKLOAD_PHOTO":"Documents/capy-bench/photo.jpg","CAPY_WORKLOAD_HEAVY":"Documents/capy-bench/heavy.capy",
      "CAPY_WORKLOAD_VECTOR":"Documents/capy-bench/vector.capy","CAPY_TRACE_GPU":"0",
      "CAPY_WORKLOAD_DEVICE":"ipad","CAPY_WORKLOAD_VERSION":"LABEL"}' art.capycanvas.bench.LABEL
  # Repeat until the copied directory contains pass.json:
  xcrun devicectl device copy from --device DEVICE --domain-type appDataContainer \\
    --domain-identifier art.capycanvas.bench.LABEL --source Documents/capy-bench/out/pass-1 \\
    --destination ~/capy-bench/runs/RUN/ipad/LABEL/pass-1

Do not set CAPY_TRACE_SECONDS: the chain needs its own trace. CAPY_WORKLOAD_SECONDS (default 1200)
bounds a pass; bench_apple.py sets it to 1.5 times a chain's pass_minutes. Keep the window visible
and the Mac idle. Paths in CAPY_WORKLOAD_* are absolute or relative to the app container's home. CAPY_WORKLOAD_STEPS limits a pass to the
named steps. A pass that cannot start records "error" in pass.json; skipped and failed steps
carry a one-line "reason".
"""
import argparse
import hashlib
import json
import os
import plistlib
import shutil
import signal
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

from bench import PASS_MINUTES, check_free_disk, document_files, pass_order, step_closure
from bench_analyze import analyze_run, format_table

TOOLS = Path(__file__).resolve().parent
STAGE = "Documents/capy-bench"


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def bundle_id(app):
    with open(Path(app) / "Contents/Info.plist", "rb") as file:
        return plistlib.load(file)["CFBundleIdentifier"]


def container(home, app_id):
    return Path(home) / "Library/Containers" / app_id / "Data"


def stage(data, files):
    """Copy {name: source} into the container stage unless an identical copy (size, mtime) is there."""
    target = Path(data) / STAGE
    target.mkdir(parents=True, exist_ok=True)
    for name, source in files.items():
        destination, stat = target / name, Path(source).stat()
        if not destination.exists() or (destination.stat().st_size, destination.stat().st_mtime_ns) != (stat.st_size, stat.st_mtime_ns):
            shutil.copy2(source, destination)


def pass_environment(output, number, cold, root, device, version, commit, steps, documents):
    env = {"CAPY_WORKLOAD": "chain", "CAPY_WORKLOAD_CHAIN": f"{STAGE}/chain.json", "CAPY_WORKLOAD_OUTPUT": output,
           "CAPY_WORKLOAD_PASS": str(number), "CAPY_WORKLOAD_COLD": "1" if cold else "0",
           "CAPY_WORKLOAD_DEVICE": device, "CAPY_WORKLOAD_VERSION": version, "CAPY_TRACE_GPU": "0"}
    env.update({f"CAPY_WORKLOAD_{key.upper()}": f"{STAGE}/{name}" for key, name in documents.items()})
    if not cold:
        env["CAPY_WORKLOAD_ROOT"] = root
    if commit:
        env["CAPY_WORKLOAD_COMMIT"] = commit
    if steps:
        env["CAPY_WORKLOAD_STEPS"] = ",".join(steps)
    return env


def open_command(app, env):
    return ["open", "-n", *[part for key, value in env.items() for part in ("--env", f"{key}={value}")],
            str(app), "--args", "-ApplePersistenceIgnoreState", "YES"]


def launch_mac(app, env):
    subprocess.run(open_command(app, env), check=True)


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def commit_of(version):
    result = subprocess.run(["git", "-C", str(TOOLS), "rev-parse", "--verify", "--quiet", f"{version}^{{commit}}"],
                            capture_output=True, text=True)
    return result.stdout.strip() or None


def wait_for_pass(data, output, timeout, process_alive=alive, sleep=time.sleep):
    """Wait for pass.json, then for that app process to quit; stop the recorded pid after a minute."""
    path = Path(data) / output / "pass.json"
    deadline = time.monotonic() + timeout
    while not path.exists():
        if time.monotonic() > deadline:
            raise TimeoutError(f"No pass.json at {path} after {timeout} s; check the app window and Console for 'Capy chain'")
        sleep(1)
    result = json.loads(path.read_text())
    pid = int(result.get("pid") or 0)
    for _ in range(60):
        if pid <= 0 or not process_alive(pid):
            return result
        sleep(1)
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return result
    for _ in range(10):
        if not process_alive(pid):
            return result
        sleep(1)
    if not process_alive(pid):
        return result
    raise TimeoutError(f"Bench app {pid} did not exit after SIGTERM; refusing to reuse its storage")


def prepare_container(app, app_id, home, timeout, launch, process_alive, sleep):
    """macOS creates the sandbox container on first launch; a launch without a staged chain quits at once."""
    data = container(home, app_id)
    if not data.exists():
        output = f"{STAGE}/prime"
        launch(app, {"CAPY_WORKLOAD": "chain", "CAPY_WORKLOAD_CHAIN": f"{STAGE}/missing.json", "CAPY_WORKLOAD_OUTPUT": output})
        wait_for_pass(data, output, timeout, process_alive, sleep)
        shutil.rmtree(data / output)
    return data


def run(args, launch=launch_mac, home=Path.home(), process_alive=alive, sleep=time.sleep, commit=commit_of):
    chain = json.loads(Path(args.chain).read_text())
    steps = step_closure(chain["steps"], args.steps.split(",")) if args.steps else None
    bundles = dict(item.split("=", 1) for item in args.bundle)
    if not bundles:
        raise SystemExit("Give at least one --bundle VERSION=APP")
    commits = dict(item.split("=", 1) for item in args.commit)
    if unknown := commits.keys() - bundles.keys():
        raise SystemExit(f"Commit overrides name no bundle: {', '.join(sorted(unknown))}")
    try:
        sources = document_files(chain, steps, args.photo, args.fixtures)
    except ValueError as error:
        raise SystemExit(str(error)) from None
    documents = {name: name + path.suffix.lower() for name, path in sources.items()}
    files = {"chain.json": args.chain, **{documents[name]: path for name, path in sources.items()}}
    free_disk = {"before": shutil.disk_usage(home).free // 1024}
    check_free_disk(args.device, free_disk["before"] * 1024, sources.values(), chain)
    versions = {label: {"bundle": str(Path(app).resolve()), "app_id": bundle_id(app), "commit": commit(commits.get(label, label))}
                for label, app in bundles.items()}
    if unresolved := [label for label in commits if not versions[label]["commit"]]:
        raise SystemExit(f"Cannot resolve build commits for: {', '.join(unresolved)}")
    ids = [value["app_id"] for value in versions.values()]
    if len(set(ids)) < len(ids):
        print("Warning: versions share a bundle id, so they share a container and the OS Metal shader cache")
    passes = 1 if args.quick else args.passes or chain.get("passes", 2)
    bound = 90 * chain["pass_minutes"] if "pass_minutes" in chain else None
    order = [(entry["version"], entry["pass"], entry["cold"]) for entry in pass_order(list(bundles), passes, args.quick)]
    run_dir = Path(home) / "capy-bench/runs" / args.run_id / args.device
    run_dir.mkdir(parents=True, exist_ok=True)
    print(f"{len(order)} passes, about {len(order) * chain.get('pass_minutes', PASS_MINUTES)} min")
    started = time.monotonic()
    report = {"schema": 1, "run_id": args.run_id, "host": "apple", "device": args.device, "chain": str(args.chain),
              "chain_sha256": sha256(args.chain), "steps": steps, "versions": list(bundles),
              "builds": versions, "devices": {args.device: {"tier": "top"}},
              "order": [{"version": version, "pass": number, "cold": cold} for version, number, cold in order],
              "free_disk_kib": free_disk, "started": datetime.now(timezone.utc).isoformat(), "passes": []}
    roots = {label: f"bench-{label.replace('/', '_')}" for label in bundles}
    cold_roots = []
    for version, number, cold in order:
        app, app_id = bundles[version], versions[version]["app_id"]
        data = prepare_container(app, app_id, home, args.timeout, launch, process_alive, sleep)
        stage(data, files)
        output = f"{STAGE}/out/{args.run_id}/{version.replace('/', '_')}/pass-{number}"
        if (data / output).exists():
            shutil.rmtree(data / output)
        env = pass_environment(output, number, cold, roots[version], args.device, version,
                               versions[version]["commit"], steps, documents)
        if bound:
            env["CAPY_WORKLOAD_SECONDS"] = str(bound)
        began = time.monotonic()
        print(f"{version} pass {number} ({'cold' if cold else 'warm'})", flush=True)
        launch(app, env)
        result = wait_for_pass(data, output, max(args.timeout, (bound or 0) + 300), process_alive, sleep)
        if cold and result.get("root"):
            roots[version] = result["root"]
            cold_roots.append(data / "Library/Caches/CapyPerformanceSessions" / result["root"])
        destination = run_dir / version / f"pass-{number}"
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(data / output, destination)
        shutil.rmtree(data / output)
        failed = [step["id"] for step in result.get("steps", []) if step["status"] in {"failed", "error"}]
        print(f"  {result.get('error') or 'ok'}; failed steps: {', '.join(failed) or 'none'}")
        report["passes"].append({"version": version, "pass": number, "cold": cold, "dir": str(destination),
                                 "error": result.get("error"), "failed_steps": failed,
                                 "seconds": round(time.monotonic() - began, 1)})
    for session in cold_roots:
        if session.exists():
            shutil.rmtree(session)
    free_disk["after"] = shutil.disk_usage(home).free // 1024
    report["finished"] = datetime.now(timezone.utc).isoformat()
    (run_dir.parent / "run.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Done in {(time.monotonic() - started) / 60:.1f} min: {run_dir.parent}")
    print(format_table(analyze_run(run_dir.parent)))
    return report


def parse(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bundle", action="append", default=[], metavar="VERSION=APP",
                        help="a Release CapyCanvas-Mac.app for a version label (tag or git ref); repeat per version")
    parser.add_argument("--commit", action="append", default=[], metavar="VERSION=REF",
                        help="record the built source commit when a bundle label differs from its Git ref")
    parser.add_argument("--device", required=True, help="device name for the run directory, such as macmini")
    parser.add_argument("--photo", help="tier JPEG for chain documents without a fixture (open.photo)")
    parser.add_argument("--fixtures", help="directory with the chain's fixture files (heavy.capy and vector.capy for the tier canvas)")
    parser.add_argument("--chain", default=str(TOOLS / "bench-chain.json"), help="chain file (bench-gigapixel.json is the gigapixel chain)")
    parser.add_argument("--passes", type=int, help="passes per version (default: the chain's passes, else 2: A B B A)")
    parser.add_argument("--quick", action="store_true", help="one warm pass per version, no cold pass")
    parser.add_argument("--steps", help="comma-separated steps; launch and the opens they need are added")
    parser.add_argument("--run-id", default=datetime.now().strftime("%Y%m%d-%H%M%S"))
    parser.add_argument("--timeout", type=int, default=1500, help="seconds to wait for each pass")
    return parser.parse_args(argv)


if __name__ == "__main__":
    run(parse())
