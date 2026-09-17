#!/usr/bin/env python3
"""Serial native HDR qualification. Build first; never compile during timing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("candidate", type=Path)
p.add_argument("parent", type=Path)
p.add_argument("fixed", type=Path)
p.add_argument("output", type=Path)
args = p.parse_args()
root = Path(__file__).resolve().parents[2]
args.output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, LAYER_TEST_MONITOR="3840x2160@120", LAYER_TEST_SCALE="2",
           GSK_RENDERER="vulkan", LAYER_NAVIGATION_PHOTO="60mp",
           LAYER_NAVIGATION_COMPLETE="1", LAYER_NAVIGATION_MAXIMIZE="1")
proof = "/usr/share/color/icc/krita/cmyk.icm"
runs = []
for repeat in range(1, 4):
    for arm in ["parent", "fixed", "candidate"]:
        runs.append((f"{arm}-sdr60-{repeat}", getattr(args, arm), "native_large_photo_navigation", {}))
for size in ["24mp", "45mp", "60mp"]:
    runs.append((f"hdr-{size}", args.candidate, "native_large_photo_navigation",
                 dict(LAYER_NAVIGATION_HDR="1", LAYER_NAVIGATION_PHOTO=size)))
runs += [
    ("hdr-proof60", args.candidate, "native_large_photo_navigation",
     dict(LAYER_NAVIGATION_HDR="1", LAYER_BENCH_PROOF=proof)),
    ("hdr-stress60", args.candidate, "native_large_photo_navigation",
     dict(LAYER_NAVIGATION_HDR="1", LAYER_NAVIGATION_LONG_CHAIN="1",
          LAYER_NAVIGATION_PHYSICAL="1", LAYER_NAVIGATION_CONCURRENT="1")),
    ("parent-drawing", args.parent, "native_penup_and_following_strokes", {}),
    ("candidate-drawing", args.candidate, "native_penup_and_following_strokes", {}),
    ("hdr-drawing", args.candidate, "native_penup_and_following_strokes", dict(LAYER_DRAWING_HDR="1")),
    ("hdr-proof-drawing", args.candidate, "native_penup_and_following_strokes",
     dict(LAYER_DRAWING_HDR="1", LAYER_BENCH_PROOF=proof)),
]
records = []
gpu_log = (args.output / "gpu-samples.csv").open("w")
monitor = subprocess.Popen(["nvidia-smi", "--query-gpu=pci.bus_id,memory.used,temperature.gpu,utilization.gpu,power.draw,clocks_event_reasons.active",
                            "--format=csv", "-l", "1"], stdout=gpu_log, stderr=subprocess.STDOUT)
try:
    for name, binary, test, extra in runs:
        prefix = (args.output / name).resolve()
        command = ["/usr/bin/time", "-v", "-o", str(prefix) + ".time.txt", "bash",
                   str(root / "tools/performance/gtk-raster.sh"), str(binary.resolve()), test, str(prefix)]
        start = time.time()
        print(name, "starting", flush=True)
        with Path(str(prefix) + ".console.log").open("w") as log:
            result = subprocess.run(command, env=env | extra, cwd=root, stdout=log, stderr=subprocess.STDOUT)
        records.append(dict(name=name, command=command, environment=env | extra,
                            start_unix=start, elapsed_seconds=time.time()-start, status=result.returncode,
                            executable_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()))
        # Retain only explicit workload variables, never ambient credentials.
        records[-1]["environment"] = {k:v for k,v in (env | extra).items()
                                       if k in extra or k in {"LAYER_TEST_MONITOR", "LAYER_TEST_SCALE", "GSK_RENDERER", "LAYER_NAVIGATION_PHOTO", "LAYER_NAVIGATION_COMPLETE", "LAYER_NAVIGATION_MAXIMIZE"}}
        (args.output / "runs.json").write_text(json.dumps(records, indent=2))
        print(name, "exit", result.returncode, flush=True)
        if result.returncode:
            raise SystemExit(result.returncode)
        if test == "native_large_photo_navigation":
            with Path(str(prefix) + ".summary.json").open("w") as out:
                subprocess.run(["python3", str(root / "tools/performance/photo-navigation-report.py"),
                                str(prefix) + ".json"], stdout=out, check=True)
finally:
    monitor.terminate()
    monitor.wait()
    gpu_log.close()
