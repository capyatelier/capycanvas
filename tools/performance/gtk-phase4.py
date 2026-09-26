#!/usr/bin/env python3
"""Serial GTK phase-4 qualification; compile all binaries before invocation."""
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
p.add_argument("fixture", type=Path)
p.add_argument("output", type=Path)
args = p.parse_args()
root = Path(__file__).resolve().parents[2]
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, LAYER_TEST_MONITOR="3840x2160@120", LAYER_TEST_SCALE="2",
           LAYER_HDR_LARGE_INPUT=str(args.fixture.resolve()), LAYER_NAVIGATION_PHOTO="60mp",
           LAYER_NAVIGATION_COMPLETE="1", LAYER_LOCAL_SECONDS="120")
runs = []
for repeat in range(1, 4):
    for name in ["parent", "fixed", "candidate"]:
        runs.append((f"{name}-sdr-{repeat}", getattr(args, name), "native_large_photo_navigation", {}))
for repeat in range(1, 4):
    runs.append((f"local-60mp-{repeat}", args.candidate, "native_local_tone_sustained_qualification", {}))
runs.append(("local-concurrent-60mp", args.candidate, "native_local_tone_sustained_qualification", {"LAYER_LOCAL_CONCURRENT": "1"}))
records = []
with (output / "gpu-samples.csv").open("w") as gpu_log:
    monitor = subprocess.Popen(["nvidia-smi", "--query-gpu=pci.bus_id,memory.used,temperature.gpu,utilization.gpu,power.draw,clocks_event_reasons.active",
                                "--format=csv", "-l", "1"], stdout=gpu_log)
    try:
        for name, binary, test, extra in runs:
            prefix = output / name
            command = ["python3", str(root/"tools/performance/hdr-memory.py"), str(binary.resolve()), test, str(prefix)]
            started = time.time()
            print(name, "starting", flush=True)
            with prefix.with_suffix(".runner.log").open("w") as log:
                result = subprocess.run(command, env=env|extra, cwd=root, stdout=log, stderr=subprocess.STDOUT)
            record = dict(name=name, command=command, started_unix=started,
                          seconds=time.time()-started, exit_code=result.returncode,
                          binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                          environment={k:v for k,v in (env|extra).items() if k.startswith("LAYER_") or k == "LD_LIBRARY_PATH"})
            if result.returncode == 0:
                script = "local-tone-report.py" if test.startswith("native_local") else "photo-navigation-report.py"
                with Path(str(prefix)+".summary.json").open("w") as summary:
                    record["gate_exit_code"] = subprocess.run(["python3", str(root/"tools/performance"/script), str(prefix)+".json"], stdout=summary).returncode
            records.append(record)
            (output/"runs.json").write_text(json.dumps(records, indent=2)+"\n")
            print(name, "exit", result.returncode, "gate", record.get("gate_exit_code"), flush=True)
    finally:
        monitor.terminate()
        monitor.wait()
raise SystemExit(0 if all(r["exit_code"] == 0 and r.get("gate_exit_code", 0) == 0 for r in records) else 1)
