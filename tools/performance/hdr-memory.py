#!/usr/bin/env python3
"""Supplemental per-process NVML/RSS sampling; not a latency benchmark.

Pass TEST_BINARY TEST_FILTER OUTPUT_PREFIX. Workload environment is inherited.
NVML graphics residency includes driver allocation; renderer accounting remains
in the application's pacing JSON. Sampling can miss shorter allocation peaks.
"""
import json
from pathlib import Path
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

binary = Path(sys.argv[1]).resolve()
prefix = Path(sys.argv[3]).resolve()
prefix.parent.mkdir(parents=True, exist_ok=True)
runner = Path(__file__).with_name("gtk-raster.sh")
samples = []
with Path(str(prefix) + ".console.log").open("w") as log:
    process = subprocess.Popen(["/usr/bin/time", "-v", "-o", str(prefix) + ".time.txt",
                                "bash", str(runner), str(binary), sys.argv[2], str(prefix)],
                               stdout=log, stderr=subprocess.STDOUT)
    while process.poll() is None:
        start = time.time()
        xml = ET.fromstring(subprocess.check_output(["nvidia-smi", "-q", "-x"]))
        for gpu in xml.findall("gpu"):
            for entry in gpu.findall("processes/process_info"):
                pid = entry.findtext("pid")
                try:
                    if Path(f"/proc/{pid}/exe").resolve() != binary:
                        continue
                    status = Path(f"/proc/{pid}/status").read_text()
                except OSError:
                    continue
                samples.append(dict(unix_time=start, gpu=gpu.attrib["id"], pid=pid,
                                    gpu_memory=entry.findtext("used_memory"),
                                    process_memory=[line for line in status.splitlines()
                                                    if line.startswith(("VmRSS:", "VmHWM:"))]))
        time.sleep(max(0, .5 - (time.time() - start)))
    code = process.wait()
Path(str(prefix) + ".memory.json").write_text(json.dumps(samples, indent=2))
assert samples, "No application graphics memory observations were available"
raise SystemExit(code)
