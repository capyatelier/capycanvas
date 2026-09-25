#!/usr/bin/env python3
"""Measure cached builds after a real, temporary edit in layer-core.

Run on an otherwise idle checkout; do not edit/build concurrently. Outputs are
ignored artifacts, and the source edit is restored even when a build fails.
The generated Cargo overrides affect workspace packages only, preserving cached
third-party release dependencies. See docs/development/rust-build-times.md.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time
import tomllib


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/layer-core/src/color.rs"
BEFORE = "if value <= 0.04045 {"
AFTER = "if value <= 0.04046 {"
PLATFORMS = {
    "gtk": ("layer-linux", None),
    "web": ("layer-web", "wasm32-unknown-unknown"),
    "android-arm64": ("layer-android", "aarch64-linux-android"),
    "android-x86_64": ("layer-android", "x86_64-linux-android"),
}


def settings(mode):
    if mode == "release":
        return {}
    values = {"incremental": True}
    if mode == "incremental-debug0":
        values["debug"] = 0
    if mode == "incremental-cgu16":
        values["codegen-units"] = 16
    return values


def run(command, env, log):
    start = time.monotonic()
    with log.open("w") as stream:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream,
                                stderr=subprocess.STDOUT)
    elapsed = time.monotonic() - start
    if result.returncode:
        raise RuntimeError(f"Command failed ({result.returncode}): {command}\n"
                           f"{log.read_text()[-6000:]}")
    return elapsed


def benchmark(args):
    os.chdir(ROOT)
    output = (ROOT / args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    members = [tomllib.loads((ROOT / member / "Cargo.toml").read_text())
               ["package"]["name"] for member in manifest["workspace"]["members"]]
    env = os.environ.copy()
    # Do not silently benchmark a different cache/profile than the requested one.
    conflicting_env = {
        "CARGO_INCREMENTAL", "CARGO_TARGET_DIR", "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "CARGO_BUILD_INCREMENTAL",
        "CARGO_BUILD_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
    } | {key for key in env if key.startswith("CARGO_PROFILE_RELEASE_")}
    for key in sorted(conflicting_env):
        if env.get(key):
            raise RuntimeError(f"Unset {key} before benchmarking")
    env["CARGO_TERM_COLOR"] = "never"
    env.setdefault("ANDROID_NDK_HOME", str(Path(env.get("ANDROID_HOME",
                   str(Path.home() / "Android/Sdk"))) / "ndk/29.0.14206865"))
    original = SOURCE.read_text()
    if original.count(BEFORE) != 1 or AFTER in original:
        raise RuntimeError("Benchmark anchor changed; inspect source before running")
    changed = original.replace(BEFORE, AFTER)
    expected = original
    results = []
    (output / "source-before.rs").write_text(original)
    (output / "metadata.json").write_text(json.dumps({
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
        "cargo": subprocess.check_output(["cargo", "-V"], text=True).strip(),
        "logical_cpus": os.cpu_count(),
        "source": str(SOURCE.relative_to(ROOT)),
        "source_sha256": hashlib.sha256(original.encode()).hexdigest(),
        "before": BEFORE, "after": AFTER,
        "arguments": vars(args),
    }, indent=2) + "\n")

    def replace_source(text):
        nonlocal expected
        if SOURCE.read_text() != expected:
            raise RuntimeError("Concurrent source edit detected; refusing to overwrite")
        if text != expected:
            SOURCE.write_text(text)
            expected = text

    try:
        for mode in args.modes:
            config = output / f"{mode}.toml"
            config.write_text("\n".join(
                f'[profile.release.package."{name}"]\n' + "\n".join(
                    f"{key} = {json.dumps(value)}" for key, value in settings(mode).items())
                for name in members) if settings(mode) else "")
            for platform in args.platforms:
                package, target = PLATFORMS[platform]
                command = ["cargo"]
                if platform.startswith("android"):
                    command += ["ndk", "-t", target, "--platform", "29"]
                command += ["build", "--offline", "--locked", "--release",
                            "-p", package, "--timings"]
                if target and not platform.startswith("android"):
                    command += ["--target", target]
                if settings(mode):
                    command += ["--config", str(config)]
                replace_source(original)
                for phase in ["warmup", "noop", *[f"edit-{i + 1}" for i in range(args.repeats)]]:
                    if phase.startswith("edit"):
                        replace_source(changed if expected == original else original)
                    label = f"{platform}-{mode}-{phase}"
                    print(f"START {label}", flush=True)
                    elapsed = run(command, env, output / f"{label}.log")
                    timing = ROOT / "target/cargo-timings/cargo-timing.html"
                    html = timing.read_text()
                    shutil.copyfile(timing, output / f"{label}.html")
                    match = re.search(r"const UNIT_DATA = (\[.*?\]);", html, re.S)
                    if not match:
                        raise RuntimeError("Cargo timing report format changed")
                    units = [u for u in json.loads(match[1]) if u["duration"] > 0]
                    compiled = re.findall(
                        r"^\s*Compiling (\S+) v",
                        (output / f"{label}.log").read_text(), re.M)
                    third_party = sorted(
                        ({u["name"] for u in units} | set(compiled)) - set(members))
                    if phase != "warmup" and third_party:
                        raise RuntimeError(f"External dependencies rebuilt: {third_party}")
                    row = {"platform": platform, "mode": mode, "phase": phase,
                           "wall_s": elapsed, "command": command, "units": units}
                    if platform == "web":
                        row["bindgen_s"] = run([
                            "wasm-bindgen", "--target", "web", "--out-dir",
                            str(output / "web-pkg"),
                            str(ROOT / "target/wasm32-unknown-unknown/release/layer_web.wasm")],
                            env, output / f"{label}-bindgen.log")
                    results.append(row)
                    (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
                    print(f"DONE {label}: {elapsed:.2f}s; " + ", ".join(
                        f'{u["name"]} {u["duration"]:.2f}s' for u in units
                        if u["name"] in members), flush=True)
    finally:
        replace_source(original)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platforms", nargs="+", choices=PLATFORMS,
                        default=list(PLATFORMS))
    parser.add_argument("--modes", nargs="+", choices=(
                            "release", "incremental", "incremental-debug0",
                            "incremental-cgu16"),
                        default=["release", "incremental", "incremental-debug0"])
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--output", default="artifacts/rust-incremental")
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")

    def interrupted(*_):
        raise KeyboardInterrupt()

    signal.signal(signal.SIGTERM, interrupted)
    benchmark(args)
