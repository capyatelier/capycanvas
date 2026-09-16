#!/usr/bin/env python3
"""Kill owned writers at observed disk boundaries; reopen with production code.

Uses no UI, Metal surface or artist storage. SIGSTOP acknowledges the exact
observed state before SIGKILL, so scheduler races cannot qualify a missed phase.
"""
import argparse
import contextlib
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import tempfile
import time
import uuid


ROOT = Path(__file__).resolve().parents[3]
APPLE = ROOT / "apps/layer-apple"
SENTINELS = {"keep.capy": b"Unrelated file", ".keep.capy-tmp": b"Not our UUID", ".keep.tmp": b"Retain"}


def ready(process):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        assert process.poll() is None, "Writer stopped before readiness"
        if select.select([process.stdout], [], [], 0.1)[0]:
            if process.stdout.readline().strip() == "READY":
                return
    raise RuntimeError("Writer readiness deadline")


def stop(process):
    os.kill(process.pid, signal.SIGSTOP)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        pid, status = os.waitpid(process.pid, os.WUNTRACED | os.WNOHANG)
        if pid:
            assert os.WIFSTOPPED(status), "Writer exited while stopping"
            return
        time.sleep(0.001)
    raise RuntimeError("Writer stop not acknowledged")


def observe(folder):
    record = json.loads((folder / "current.json").read_text()).get("record")
    files = {}
    for path in folder.iterdir():
        try:
            files[path.name] = path.stat().st_size
        except FileNotFoundError:
            pass  # The live writer may finish publication during observation.
    return record, files


def matches(stage, folder, record, files):
    if stage == "archive":
        return any(name not in SENTINELS and name.endswith(".capy-tmp") and size > 0
                   for name, size in files.items())
    if stage == "manifest":
        for name, size in files.items():
            if name in SENTINELS or not name.endswith(".tmp") or size == 0:
                continue
            try:
                pending = json.loads((folder / name).read_text()).get("record")
                if record and pending and record["generation"] != pending["generation"]:
                    return True
            except (FileNotFoundError, json.JSONDecodeError):
                pass
        return False
    if stage == "published":
        return record is not None and record["title"].startswith("Candidate")
    if stage == "discard":
        return record is None
    raise AssertionError(stage)


def retained_files(folder, published):
    record, files = observe(folder)
    assert bool(record) == published
    assert set(files) == {"current.json", *SENTINELS, *([record["generation"] + ".capy"] if record else [])}, files
    for name, content in SENTINELS.items():
        assert (folder / name).read_bytes() == content


def run(output):
    env = os.environ.copy()
    env.setdefault("DEVELOPER_DIR", "/Applications/Xcode.app/Contents/Developer")
    env.setdefault("MACOSX_DEPLOYMENT_TARGET", "15.0")
    with (output / "build.log").open("w") as log:
        subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "layer-apple", "--target", "aarch64-apple-darwin"],
                       cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        sources = [APPLE / "Shared/Bridge" / (name + ".swift")
                   for name in ["JSON", "AtomicJSONFile", "ProjectFileIO", "RecoveryFiles"]]
        target = Path(env.get("CARGO_TARGET_DIR", ROOT / "target"))
        subprocess.run(["xcrun", "--sdk", "macosx", "swiftc", "-module-cache-path", str(output / "modules"),
                        "-import-objc-header", str(APPLE / "native/include/CapyApple.h"),
                        *map(str, sources), str(APPLE / "tests/recovery-interruption.swift"),
                        "-L", str(target / "aarch64-apple-darwin/debug"), "-llayer_apple", "-lc++",
                        "-framework", "Metal", "-framework", "QuartzCore", "-framework", "Security",
                        "-framework", "AppKit", "-o", str(output / "check")],
                       cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    results = []
    for platform in [0, 1]:
        for stage in ["archive", "manifest", "published", "discard"]:
            root = output / f"{platform}-{stage}"
            root.mkdir()
            scene = str(uuid.uuid4()).upper()
            args = [str(platform), str(root), scene]

            def invoke(mode, arguments=args):
                return subprocess.check_output([str(output / "check"), mode, *arguments], text=True, env=env).strip()

            assert invoke("seed") == "SAVED"
            folder = root / "recovery" / scene
            for name, content in SENTINELS.items():
                (folder / name).write_bytes(content)
            with (root / "child.log").open("w") as log:
                process = subprocess.Popen([str(output / "check"), "discard" if stage == "discard" else "write", *args],
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, env=env)
                try:
                    ready(process)
                    process.stdin.write("GO\n")
                    process.stdin.flush()
                    deadline = time.monotonic() + 20
                    observations = 0
                    while time.monotonic() < deadline:
                        assert process.poll() is None, "Writer exited before observed interruption"
                        record, files = observe(folder)
                        if matches(stage, folder, record, files):
                            stop(process)
                            observations += 1
                            record, files = observe(folder)
                            if matches(stage, folder, record, files):
                                break
                            os.kill(process.pid, signal.SIGCONT)
                    else:
                        raise RuntimeError(f"{stage}: stopped phase not observed")
                    process.kill()
                    assert process.wait(timeout=5) == -signal.SIGKILL
                    result = invoke("probe")
                    assert result == "NONE" if stage == "discard" else result in ["OLD", "NEW"]
                    if stage == "published":
                        assert result == "NEW"
                    if stage != "discard":
                        assert (root / "expected-old.capy").read_bytes() != (root / "expected-new.capy").read_bytes()
                        # Both recovery migration/discard and same-owner retry
                        # must reclaim the actual interrupted write's leftovers.
                        discarded = output / f"{platform}-{stage}-removed"
                        shutil.copytree(root, discarded)
                        removed_args = [str(platform), str(discarded), scene]
                        assert invoke("remove", removed_args) == "REMOVED"
                        assert invoke("probe", removed_args) == "NONE"
                        retained_files(discarded / "recovery" / scene, False)
                    assert invoke("retry") == "SAVED"
                    assert invoke("probe") == "NEW"
                    assert invoke("stale") == "REMOVED"
                    assert invoke("probe") == "NEW", "Stale removal deleted the newer recovery"
                    retained_files(folder, True)
                    row = {"platform": platform, "phase": stage, "stop_observations": observations,
                           "record_at_stop": record, "files_at_stop": files, "fresh_process": result,
                           "retry": "exact NEW", "cleanup": "passed", "stale_removal": "preserved"}
                    results.append(row)
                    (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
                    print(f"PASS policy {platform}: killed during {stage}; fresh recovery {result}, cleanup, retry and stale removal", flush=True)
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait(timeout=5)
    print("PASS: eight observed-phase process interruptions; no Metal or UI", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="Retain private evidence in a new directory")
    args = parser.parse_args()
    with contextlib.ExitStack() as stack:
        if args.output:
            output = args.output.resolve()
            output.mkdir(parents=True, exist_ok=False)
        else:
            output = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="capy-recovery-interruption-")))
        run(output)
