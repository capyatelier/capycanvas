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

def descendants_memory(parent):
    # Codec processes can be spawned by worker threads. Scan PPid rather than
    # only /proc/PID/task/PID/children, which omits those children.
    processes = {}
    for directory in Path("/proc").iterdir():
        if not directory.name.isdigit():
            continue
        try:
            values = dict(line.split(":", 1) for line in (directory / "status").read_text().splitlines() if ":" in line)
            processes[int(directory.name)] = (int(values["PPid"]), int(values.get("VmRSS", "0 kB").split()[0]))
        except (OSError, KeyError, ValueError):
            continue
    family = {int(parent)}
    while True:
        expanded = family | {pid for pid, (ppid, _) in processes.items() if ppid in family}
        if expanded == family:
            break
        family = expanded
    children = [{"pid": pid, "rss_kib": processes[pid][1]} for pid in sorted(family) if pid != int(parent) and pid in processes]
    staged = 0
    for directory in Path("/tmp").glob(f"capy-hdr-{parent}-*"):
        try:
            for entry in directory.iterdir():
                try:
                    staged += entry.stat().st_size
                except OSError:
                    pass
        except OSError:
            pass
    return {"children": children, "process_tree_rss_kib": sum(processes[pid][1] for pid in family if pid in processes), "staging_bytes": staged}

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
                                    **descendants_memory(pid),
                                    process_memory=[line for line in status.splitlines()
                                                    if line.startswith(("VmRSS:", "VmHWM:"))]))
        time.sleep(max(0, .5 - (time.time() - start)))
    code = process.wait()
Path(str(prefix) + ".memory.json").write_text(json.dumps(samples, indent=2))
assert samples, "No application graphics memory observations were available"
raise SystemExit(code)
