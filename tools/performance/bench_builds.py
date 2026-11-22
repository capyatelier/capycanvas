"""Cached optimized benchmark APKs for bench versions, built in detached source trees with the current bench runner."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BUILDS = Path.home() / "capy-bench/builds"
SCRATCH = Path("/mnt/localssd/capy-bench")
RUNNER = "apps/layer-android/app/src/benchmark/java/art/capycanvas/BenchChainInstrumentation.kt"
MANIFEST = "apps/layer-android/app/src/benchmark/AndroidManifest.xml"
APK = "app/outputs/apk/benchmark/app-benchmark.apk"


def git(*args, cwd=ROOT, check=True):
    return subprocess.run(["git", *args], cwd=cwd, check=check, capture_output=True, text=True).stdout.strip()


def sha256(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def label(version, commit, tagged):
    return version if tagged else "g" + commit[:9]


def app_id(name):
    return "art.capycanvas.bench." + re.sub(r"[.-]", "_", name)


def app_label(name):
    return f"Capy Bench {name}"


def resolve(version):
    commit = git("rev-parse", "--verify", f"{version}^{{commit}}")
    name = label(version, commit, bool(git("rev-parse", "--verify", "--quiet", f"refs/tags/{version}", check=False)))
    return dict(version=version, label=name, commit=commit, app_id=app_id(name), app_label=app_label(name))


def source_tree(target):
    tree = SCRATCH / "src" / target["label"]
    if not tree.exists():
        git("worktree", "add", "--detach", str(tree), target["commit"])
    elif git("rev-parse", "HEAD", cwd=tree) != target["commit"]:
        raise SystemExit(f"{tree} is not at {target['commit']}; remove it with git worktree remove")
    return tree


def toolchain(tree):
    def first_line(command):
        result = subprocess.run(command, cwd=tree, capture_output=True, text=True)
        return (result.stdout or result.stderr).strip().splitlines()[0] if result.returncode == 0 else None
    return dict(rustc=first_line(["rustc", "--version"]), java=first_line(["java", "-version"]))


def build(version, dry_run=False):
    target = resolve(version)
    cache = BUILDS / target["label"]
    if (cache / "build.json").exists():
        record = json.loads((cache / "build.json").read_text())
        if (ROOT / RUNNER).exists() and record["runner_sha256"] != sha256(ROOT / RUNNER):
            raise SystemExit(f"The bench runner changed since {target['label']} was built; remove {cache} to rebuild it")
        print(f"cached {target['label']}: {record['apk']}")
        return record
    tree, out = SCRATCH / "src" / target["label"], SCRATCH / "build" / target["label"]
    overlay = [RUNNER] + ([MANIFEST] if "BenchChainInstrumentation" not in git("show", f"{target['commit']}:{MANIFEST}") else [])
    init = out / "init.gradle"
    script = f"allprojects {{ layout.buildDirectory.set(file('{out / 'gradle'}/' + project.name)) }}\n"
    env = dict(ANDROID_HOME=str(Path.home() / "Android/Sdk"), CARGO_TARGET_DIR=str(out / "cargo"))
    gradle = ["./gradlew", "--offline", "--init-script", str(init), ":app:assembleBenchmark", "-PcapyAbi=arm64-v8a",
              "-PcapyOptimize", f"-PcapyApplicationId={target['app_id']}", f"-PcapyAppLabel={target['app_label']}"]
    apk = cache / "app-benchmark.apk"
    if dry_run:
        if not tree.exists():
            print(shlex.join(["git", "worktree", "add", "--detach", str(tree), target["commit"]]))
        for path in overlay:
            print(shlex.join(["cp", str(ROOT / path), str(tree / path)]))
        print(f"mkdir -p {shlex.quote(str(out))} && echo {shlex.quote(script.strip())} > {shlex.quote(str(init))}")
        print(f"cd {shlex.quote(str(tree / 'apps/layer-android'))} && " + shlex.join([f"{k}={v}" for k, v in env.items()] + gradle))
        print(shlex.join(["cp", str(out / "gradle" / APK), str(apk)]))
        return dict(target, apk=str(apk))
    if "BenchChainInstrumentation" not in (ROOT / MANIFEST).read_text():
        raise SystemExit(f"{MANIFEST} in {ROOT} does not register BenchChainInstrumentation")
    source_tree(target)
    for path in overlay:
        shutil.copy2(ROOT / path, tree / path)
    out.mkdir(parents=True, exist_ok=True)
    init.write_text(script)
    log = out / "build.log"
    print(f"building {target['label']} ({target['commit'][:9]}); log: {log}", flush=True)
    with log.open("w") as stream:
        if subprocess.run(gradle, cwd=tree / "apps/layer-android", env=dict(os.environ, **env), stdout=stream, stderr=subprocess.STDOUT).returncode:
            raise SystemExit(f"Build of {target['label']} failed; see {log}")
    cache.mkdir(parents=True, exist_ok=True)
    shutil.copy2(out / "gradle" / APK, apk)
    record = dict(target, apk=str(apk), sha256=sha256(apk), runner_sha256=sha256(tree / RUNNER), toolchain=toolchain(tree),
                  built=datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"))
    (cache / "build.json").write_text(json.dumps(record, indent=2) + "\n")
    return record
