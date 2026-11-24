#!/usr/bin/env python3
"""Run the bench chain in the GTK app on a private headless Mutter display, in palindrome order.

  tools/performance/bench_gtk.py --versions v1.0.15,origin/main

Each version builds its release test executable from a detached tree under
/mnt/localssd/capy-bench/gtk/src/LABEL, with this tree's
apps/layer-linux/src/bench_chain_tests.rs journey copied in. The journey runs the
production application setup and drives it with native pen (tablet proxy, with
pressure), touch and keys through workspace-motion.sh. Each pass gets fresh app
storage; a version keeps its shader caches between passes, and a cold pass starts
without them. Passes land in ~/capy-bench/runs/RUN/gtk/LABEL/pass-N.
"""
import argparse
import bisect
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import xml.etree.ElementTree as ElementTree

from bench import PASS_MINUTES, pass_order, step_closure
from bench_analyze import analyze_run, format_table
from bench_android import CANVAS, documents
from bench_builds import resolve, sha256

ROOT = Path(__file__).resolve().parents[2]
CHAIN = Path(__file__).with_name("bench-chain.json")
JOURNEY = "apps/layer-linux/src/bench_chain_tests.rs"
TESTS = "apps/layer-linux/src/tests.rs"
PATCHES = Path(__file__).with_name("bench-patches")
REGISTRATION = '#[path = "bench_chain_tests.rs"]\nmod bench_chain;\n'
TEST = "native_bench_chain"
HELPER = "capy-photo-reader"
SCRATCH = Path("/mnt/localssd/capy-bench/gtk")
RUNS = Path.home() / "capy-bench/runs"
GPU_LOCK = Path("/mnt/localssd/capycanvas7-gpu-validation.lock")
DEVICE = "gtk"
APP_ID = "art.capycanvas.CapyCanvas.Devel"
EVENT_MS = 4
LOGS = re.compile(r"^Workspace test logs: (.+)$")


def git(*args, cwd=ROOT):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def overlay(tree, label):
    patch = PATCHES / f"{label}-gtk.patch"
    if patch.exists() and subprocess.run(["git", "apply", "--reverse", "--check", str(patch)], cwd=tree, capture_output=True).returncode:
        git("apply", str(patch), cwd=tree)
    shutil.copy2(ROOT / JOURNEY, tree / JOURNEY)
    tests = tree / TESTS
    if REGISTRATION not in tests.read_text():
        with tests.open("a") as stream:
            stream.write(REGISTRATION)


def test_executable(messages):
    for line in messages.splitlines():
        message = json.loads(line) if line.startswith("{") else {}
        target = message.get("target", {})
        if message.get("reason") == "compiler-artifact" and message.get("profile", {}).get("test") \
                and target.get("name") == "layer_linux" and "lib" in target.get("kind", []):
            return message["executable"]
    raise SystemExit("cargo built no layer_linux test executable")


def build(version):
    target = resolve(version)
    tree, cargo = SCRATCH / "src" / target["label"], SCRATCH / "target" / target["label"]
    if not tree.exists():
        tree.parent.mkdir(parents=True, exist_ok=True)
        git("worktree", "add", "--detach", str(tree), target["commit"])
    elif git("rev-parse", "HEAD", cwd=tree) != target["commit"]:
        raise SystemExit(f"{tree} is not at {target['commit']}; remove it with git worktree remove")
    overlay(tree, target["label"])
    log = SCRATCH / f"build-{target['label']}.log"
    print(f"building {target['label']} ({target['commit'][:9]}); log: {log}", flush=True)
    with log.open("w") as stream:
        result = subprocess.run(["cargo", "test", "--locked", "--release", "-p", "layer-linux", "--no-run", "--message-format=json-render-diagnostics"],
                                cwd=tree, env=dict(os.environ, CARGO_TARGET_DIR=str(cargo)), stdout=subprocess.PIPE, stderr=stream, text=True)
    if result.returncode:
        raise SystemExit(f"Build of {target['label']} failed; see {log}")
    executable = test_executable(result.stdout)
    helper = Path(executable).with_name(HELPER)
    helper.unlink(missing_ok=True)
    os.link(cargo / "release" / HELPER, helper)
    rustc = subprocess.run(["rustc", "--version"], cwd=tree, capture_output=True, text=True).stdout.strip()
    return dict(target, executable=executable, sha256=sha256(executable), journey_sha256=sha256(tree / JOURNEY),
                patch_sha256=sha256(PATCHES / f"{target['label']}-gtk.patch") if (PATCHES / f"{target['label']}-gtk.patch").exists() else None, rustc=rustc)


def pass_environment(build, entry, profile, scratch, out, inputs, steps, viewport, scale, timeout_s):
    env = {key: value for key, value in os.environ.items() if not key.startswith(("CAPY_", "LAYER_", "WAYLAND_", "XDG_"))}
    env.update(TMPDIR=str(scratch), XDG_RUNTIME_DIR=str(scratch / "run"), GVFS_DISABLE_FUSE="1", LAYER_NATIVE_TEST_EXECUTABLE=build["executable"], LAYER_NATIVE_EVENT_MS=str(EVENT_MS),
               LAYER_NATIVE_INPUT_TRACE="1", LAYER_NATIVE_TEST_TIMEOUT_MS=str(timeout_s * 1000), LAYER_MOTION_VIEWPORT=viewport,
               LAYER_MOTION_SCALE=str(scale), LAYER_MOTION_REFRESH="120", LAYER_PACING_GPU_TIMESTAMPS="0",
               XDG_CONFIG_HOME=str(profile / "config"), XDG_CACHE_HOME=str(profile / "cache"), XDG_STATE_HOME=str(profile / "state"),
               __GL_SHADER_DISK_CACHE="1", __GL_SHADER_DISK_CACHE_PATH=str(profile / "nvidia"), __GL_SHADER_DISK_CACHE_SKIP_CLEANUP="1",
               CAPY_BENCH_CHAIN=str(inputs["chain"]), CAPY_BENCH_OUTPUT=str(out), CAPY_BENCH_SHADERS=str(profile / "shaders"),
               RUST_BACKTRACE="1")
    env.update({f"CAPY_BENCH_{name.upper()}": str(path) for name, path in inputs["documents"].items()})
    if steps:
        env["CAPY_BENCH_STEPS"] = ",".join(steps)
    return env


def meminfo_kib(text, field):
    match = re.search(rf"^{field}:\s+(\d+) kB", text, re.M)
    return int(match.group(1)) if match else None


def gpu_kib(xml, pid):
    for process in ElementTree.fromstring(xml).iter("process_info"):
        if process.findtext("pid") == str(pid):
            used = re.match(r"(\d+)\s*MiB", process.findtext("used_memory") or "")
            return int(used.group(1)) * 1024 if used else None
    return None


def memory_sample(pid):
    row = {"t_ns": time.monotonic_ns(), "os_available_kib": meminfo_kib(Path("/proc/meminfo").read_text(), "MemAvailable")}
    try:
        row["app_rss_kib"] = meminfo_kib(Path(f"/proc/{pid}/status").read_text(), "VmRSS")
    except FileNotFoundError:
        return None
    if row["app_rss_kib"] is None:
        return None
    smi = subprocess.run(["nvidia-smi", "-q", "-x"], capture_output=True, text=True)
    row["gpu_kib"] = gpu_kib(smi.stdout, pid) if smi.returncode == 0 else None
    return row


def sample_memory(out, rows, done):
    pid_file = out / "pid"
    while not done.is_set() and not pid_file.exists():
        done.wait(0.1)
    if done.is_set():
        return
    pid = int(pid_file.read_text())
    while not done.wait(1):
        row = memory_sample(pid)
        if row is None:
            return
        rows.append(row)


def completions(step, frames):
    samples = sorted(step.get("extra", {}).pop("samples_ns", []))
    contacts = step.get("contacts") or []
    if not samples or not contacts:
        return []
    begin, end = contacts[0]["down_ns"], step.get("settled_ns") or step["end_ns"]
    result, consumed = [], -1
    for presented, queued in frames:
        if queued is None or not begin <= presented <= end:
            continue
        index = bisect.bisect_right(samples, queued) - 1
        if index > consumed:
            consumed = index
            result.append({"complete_ns": presented, "input_ns": samples[index]})
    step["extra"]["pen_samples"] = len(samples)
    return result


def normalize(journey, info, memory):
    canvas = [(row[0], row[1]) for row in journey.get("canvas_frames", [])]
    steps = journey.get("steps", [])
    for step in steps:
        step["completions"] = completions(step, canvas) if step.get("status") == "ok" else []
        step.get("extra", {}).pop("samples_ns", None)
    close = next((step["begin_ns"] for step in steps if step["id"] == "close"), None)
    before_close = [row for row in memory if close is None or row["t_ns"] <= close]
    window = journey.get("window_frames", [])
    frames = sorted([{"t_ns": t, "surface": "canvas"} for t, _ in canvas] + [{"t_ns": t, "surface": "window"} for t in window],
                    key=lambda row: (row["t_ns"], row["surface"]))
    note = "Headless Mutter virtual monitor: presentation times come from the compositor's frame clock, not scanout"
    record = dict(info, refresh_hz=journey.get("refresh_hz"), canvas=journey.get("canvas"), scale=journey.get("scale"),
                  event_ms=journey.get("event_ms"), frames_source="wayland-presentation", frames_evidence="presented",
                  frames_note=note if window else note + "; GTK reported no window presentation times",
                  completion_source="presented canvas frames that consumed a newer injected pen sample",
                  steps=steps, memory_end={key: value for key, value in before_close[-1].items() if key != "t_ns"} if before_close else None,
                  error=info.get("error") or journey.get("error"))
    return record, frames


def write_jsonl(path, rows):
    path.write_text("".join(json.dumps(row) + "\n" for row in rows))


def stop(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass


def wait_for_group(group, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            os.killpg(group, 0)
        except ProcessLookupError:
            return True
        time.sleep(0.1)
    return False


def run_pass(build, entry, run_dir, inputs, args, steps):
    label, number = build["label"], entry["pass"]
    directory = run_dir / DEVICE / label / f"pass-{number}"
    if directory.exists():
        raise SystemExit(f"{directory} already exists; choose another --run-id")
    directory.mkdir(parents=True)
    profile = SCRATCH / "profile" / label
    if entry["cold"] and profile.exists():
        shutil.rmtree(profile)
    for folder in ("config", "cache", "state", "nvidia", "shaders"):
        (profile / folder).mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="capy-gtk-", dir=SCRATCH.parents[1]))
    staged = scratch / "documents"
    staged.mkdir()
    (scratch / "run").mkdir(mode=0o700)
    pass_inputs = dict(chain=inputs["chain"], documents={name: shutil.copy2(path, staged / Path(path).name) for name, path in inputs["documents"].items()})
    out = scratch / "out"
    env = pass_environment(build, entry, profile, scratch, out, pass_inputs, steps, args.viewport, args.scale, args.timeout * 60)
    command = ["bash", str(ROOT / "tools/performance/workspace-motion.sh"), "gtk", f"--native-test={TEST}",
               "--tablet", "--native-storage", "--native-recovery"]
    info = {"schema": 1, "host": "gtk", "device": DEVICE, "version": label, "commit": build["commit"], "build": build["sha256"],
            "app_id": APP_ID, "pass": number, "cold": entry["cold"], "clock": "monotonic", "tier": args.tier,
            "viewport": args.viewport, "chain_sha256": sha256(inputs["chain"]), "executable": build["executable"]}
    rows, done, logs = [], threading.Event(), None
    sampler = threading.Thread(target=sample_memory, args=(out, rows, done), daemon=True)
    started = time.monotonic()
    print(f"{label} pass {number} ({'cold' if entry['cold'] else 'warm'})", flush=True)
    with GPU_LOCK.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_SH if args.shared_lock else fcntl.LOCK_EX)
        sampler.start()
        with (directory / "runner.log").open("w") as log:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                       start_new_session=True)
            timer = threading.Timer(args.timeout * 60 + 60, stop, args=(process,))
            timer.start()
            for line in process.stdout:
                log.write(line)
                if line.startswith("BENCH_STEP"):
                    print("  " + line[len("BENCH_STEP "):].rstrip(), flush=True)
                match = LOGS.match(line.strip())
                logs = logs or (Path(match.group(1)) if match else None)
            code = process.wait()
            timer.cancel()
            if not wait_for_group(process.pid):
                stop(process)
                wait_for_group(process.pid)
        done.set()
        sampler.join()
    info.update(exit_code=code, seconds=round(time.monotonic() - started, 1),
                error=None if code == 0 else f"workspace-motion.sh exited {code}; see runner.log")
    journey_path = out / "journey.json"
    journey = json.loads(journey_path.read_text()) if journey_path.exists() else {"error": "The journey wrote no journey.json"}
    record, frames = normalize(journey, info, rows)
    (directory / "pass.json").write_text(json.dumps(record, indent=1) + "\n")
    write_jsonl(directory / "frames.jsonl", frames)
    write_jsonl(directory / "memory.jsonl", rows)
    if journey_path.exists():
        shutil.copy2(journey_path, directory / "journey.json")
    if logs and (logs / "mutter.log").exists():
        shutil.copy2(logs / "mutter.log", directory / "mutter.log")
    shutil.rmtree(scratch)
    failed = [step["id"] for step in record["steps"] if step.get("status") == "error"]
    return dict(version=label, pass_number=number, cold=entry["cold"], dir=str(directory), exit_code=code,
                error=record["error"], failed_steps=failed, seconds=info["seconds"])


def gpu_name():
    result = subprocess.run(["nvidia-smi", "--query-gpu=name", "--format=csv,noheader"], capture_output=True, text=True)
    return result.stdout.strip() or None


def run(args):
    chain = json.loads(Path(args.chain).read_text())
    steps = step_closure(chain["steps"], [s.strip() for s in args.steps.split(",") if s.strip()]) if args.steps else None
    versions = [v.strip() for v in args.versions.split(",") if v.strip()]
    builds = {target["label"]: target for target in map(build, versions)}
    width, height = CANVAS[args.tier]
    inputs = dict(chain=Path(args.chain).resolve(),
                  documents={name: Path(spec["local"]) for name, spec in documents(chain, steps, {"tier": args.tier, "photo": None}).items()})
    order = pass_order(list(builds), 1 if args.quick else args.passes, args.quick)
    run_dir = RUNS / args.run_id
    run_dir.mkdir(parents=True, exist_ok=True)
    existing = json.loads((run_dir / "run.json").read_text()) if (run_dir / "run.json").exists() else {}
    record = {**existing, "schema": 1, "run_id": args.run_id, "date": time.strftime("%Y-%m-%d"),
              "versions": list(dict.fromkeys([*existing.get("versions", []), *builds])), "chain_sha256": sha256(args.chain),
              "builds": {**existing.get("builds", {}), **builds},
              "devices": {**existing.get("devices", {}), DEVICE: {"host": "gtk", "tier": args.tier, "refresh_hz": 120, "canvas": [width, height],
                                                                  "viewport": args.viewport, "scale": args.scale, "gpu": gpu_name(),
                                                                  "steps": steps, "order": order, "passes": []}}}
    (run_dir / "run.json").write_text(json.dumps(record, indent=2) + "\n")
    print(f"{run_dir}: {len(order)} passes, about {len(order) * PASS_MINUTES} min", flush=True)
    started = time.monotonic()
    for entry in order:
        result = run_pass(builds[entry["version"]], entry, run_dir, inputs, args, steps)
        record["devices"][DEVICE]["passes"].append(result)
        (run_dir / "run.json").write_text(json.dumps(record, indent=2) + "\n")
        print(f"  {result['error'] or 'ok'}; failed steps: {', '.join(result['failed_steps']) or 'none'}; {result['seconds']} s", flush=True)
    print(f"Done in {(time.monotonic() - started) / 60:.1f} min: {run_dir}")
    print(format_table(analyze_run(run_dir)))
    return int(any(p["exit_code"] or p["error"] for p in record["devices"][DEVICE]["passes"]))


def parse(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--versions", required=True, help="comma-separated tags or git refs, first is the baseline (v1.0.15,origin/main)")
    parser.add_argument("--passes", type=int, default=2, help="passes per version (default 2: A B B A)")
    parser.add_argument("--quick", action="store_true", help="one warm pass per version, no cold pass")
    parser.add_argument("--steps", help="comma-separated steps; launch and the opens they need are added")
    parser.add_argument("--chain", default=str(CHAIN))
    parser.add_argument("--tier", choices=sorted(CANVAS), default="top", help="tier photo and fixture canvas (default top, 61 MP)")
    parser.add_argument("--viewport", default="1600x1000", help="private monitor size in pixels (default 1600x1000)")
    parser.add_argument("--scale", type=int, default=1, help="private monitor scale (default 1)")
    parser.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    parser.add_argument("--timeout", type=int, default=40, help="minutes allowed for each pass (default 40)")
    parser.add_argument("--shared-lock", action="store_true", help="hold the GPU lock shared; such passes do not qualify timing")
    args = parser.parse_args(argv)
    if args.passes < 1:
        parser.error("--passes must be at least 1")
    if args.steps:
        try:
            step_closure(json.loads(Path(args.chain).read_text())["steps"], [s.strip() for s in args.steps.split(",") if s.strip()])
        except ValueError as error:
            parser.error(str(error))
    return args


if __name__ == "__main__":
    sys.exit(run(parse()))
