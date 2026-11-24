#!/usr/bin/env python3
"""Run the bench chain in a browser against each version's Web build, in palindrome pass order.

Each version is built once on the Linux workstation with apps/layer-web/build.sh and the web-release
profile and served on a port derived from its device and label, so all of its passes share one origin. bench-web.mjs
runs every pass; this script picks the host from --device and the platform:

  linux    Chrome or Edge in the private Mutter display of workspace-motion.sh, one fresh profile per pass
  desktop  Chrome or Edge in a normal window on Windows or macOS, one fresh profile per pass, from bundles
  android  Chrome on a reserved tablet over adb-forwarded DevTools, one fresh incognito context per pass

On linux and desktop a cold pass starts with empty GPU caches and a warm pass copies that version's caches
from its latest cold pass. Chrome on Android keeps its own caches, so its passes are all warm. Passes land in
~/capy-bench/runs/<run_id>/<device>-<browser>/<version>/pass-<n>/ (web/ for local Chrome on Linux) beside
run.json, then the shared analyzer prints the comparison.

  flock -s /mnt/localssd/capycanvas7-gpu-validation.lock python3 tools/performance/bench_web.py run \\
    --versions v1.0.15,origin/main --photo ~/capy-bench/photos/capy-tier-61mp.jpg \\
    --fixtures ~/capy-bench/fixtures/9504x6336
  tools/devices/devices.py reserve movinkpad14
  python3 tools/performance/bench_web.py run --device movinkpad14 --versions v1.0.15,origin/main \\
    --photo ~/capy-bench/photos/capy-tier-61mp.jpg --fixtures ~/capy-bench/fixtures/9504x6336
  python3 tools/performance/bench_web.py bundle --versions v1.0.15,origin/main --out ~/capy-bench/web-bundles
  python3 tools/performance/bench_web.py run --bundles BUNDLES --device surfacepro --browser edge \\
    --photo PHOTO --fixtures FIXTURES
"""
import argparse
import datetime
import functools
import hashlib
import http.server
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from pathlib import Path

import bench_builds
from bench import pass_order, step_closure
from bench_analyze import analyze_run, format_table

TOOLS = Path(__file__).resolve().parent
DEVICES_TOOL = TOOLS.parents[1] / "tools/devices/devices.py"
SCRATCH = Path("/mnt/localssd/capy-bench/web") if Path("/mnt/localssd").is_dir() else Path.home() / "capy-bench/web"
RUNS = Path.home() / "capy-bench/runs"
PROFILE = "web-release"
VIEWPORT = "1600x1000"
REFRESH_HZ = 120
PORTS = (42000, 2000)
PASS_MINUTES = 6
MEMORY_INTERVAL_MS = {"linux": 1000, "desktop": 1000, "android": 5000}
CACHES = ("GPUPersistentCache", "GrShaderCache", "GraphiteDawnCache", "ShaderCache",
          "Default/GPUCache", "Default/DawnWebGPUCache", "Default/DawnGraphiteCache")
ANDROID_CHROME = "com.android.chrome/com.google.android.apps.chrome.Main"
ANDROID_PASS = f"""adb() {{ "$ADB" "$@"; }}
adb shell dumpsys power | grep -q 'mWakefulness=Awake' || {{ echo 'The tablet is asleep: wake and unlock it, then run again' >&2; exit 3; }}
if adb shell dumpsys window | grep -q 'isKeyguardShowing=true'; then echo 'The tablet is locked: unlock it, then run again' >&2; exit 3; fi
adb reverse tcp:$BENCH_WEB_PORT tcp:$BENCH_WEB_PORT >/dev/null && adb forward tcp:$CAPY_CDP_PORT localabstract:chrome_devtools_remote >/dev/null &&
  adb shell am start -n {ANDROID_CHROME} >/dev/null || exit 3
"$@"; code=$?
adb forward --remove tcp:$CAPY_CDP_PORT; adb reverse --remove tcp:$BENCH_WEB_PORT; adb shell input keyevent KEYCODE_HOME
exit $code"""


def port_for(label):
    return PORTS[0] + int(hashlib.sha256(label.encode()).hexdigest()[:8], 16) % PORTS[1]


def build(version, scratch=SCRATCH):
    target = bench_builds.resolve(version)
    tree, out = bench_builds.source_tree(target), scratch / "build" / target["label"]
    wasm, record_path = tree / "apps/layer-web/pkg/layer_web_bg.wasm", out / "build.json"
    if record_path.exists() and wasm.exists():
        record = json.loads(record_path.read_text())
        if record["commit"] == target["commit"] and record["wasm_sha256"] == bench_builds.sha256(wasm):
            print(f"cached {target['label']}: {wasm}", flush=True)
            return record
    out.mkdir(parents=True, exist_ok=True)
    log = out / "build.log"
    print(f"building {target['label']} ({target['commit'][:9]}); log: {log}", flush=True)
    with log.open("w") as stream:
        if subprocess.run(["bash", "apps/layer-web/build.sh", "apps/layer-web/pkg", PROFILE], cwd=tree, stdout=stream, stderr=subprocess.STDOUT,
                          env=dict(os.environ, CARGO_TARGET_DIR=str(out / "cargo"))).returncode:
            raise SystemExit(f"Build of {target['label']} failed; see {log}")
    record = dict(version=version, label=target["label"], commit=target["commit"], root=str(tree / "apps/layer-web"), profile=PROFILE,
                  wasm_sha256=bench_builds.sha256(wasm), built=datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"))
    record_path.write_text(json.dumps(record, indent=2) + "\n")
    return record


def bundle(args, build=build):
    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    records = [build(version) for version in split(args.versions)]
    for record in records:
        site = out / record["label"] / "site"
        if site.exists():
            shutil.rmtree(site)
        shutil.copytree(record["root"], site, ignore=shutil.ignore_patterns("src", "link", "Cargo.toml", "*.test.mjs"))
    manifest = {"versions": [record["label"] for record in records],
                "builds": {record["label"]: {key: record[key] for key in ("version", "commit", "wasm_sha256", "profile")} for record in records}}
    (out / "bundles.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(out)
    return manifest


def load_bundles(directory):
    directory = Path(directory).expanduser()
    manifest = json.loads((directory / "bundles.json").read_text())
    builds = {}
    for label in manifest["versions"]:
        site = directory / label / "site"
        if bench_builds.sha256(site / "pkg/layer_web_bg.wasm") != manifest["builds"][label]["wasm_sha256"]:
            raise SystemExit(f"{site} does not hold the {label} build that bundles.json records")
        builds[label] = dict(manifest["builds"][label], label=label, root=str(site))
    return builds


class Site(http.server.SimpleHTTPRequestHandler):
    documents = {}

    def translate_path(self, path):
        name = urllib.parse.unquote(urllib.parse.urlsplit(path).path).removeprefix("/capy-bench/")
        return str(self.documents[name]) if name in self.documents else super().translate_path(path)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args):
        pass


def serve(root, port, documents):
    handler = functools.partial(type("BenchSite", (Site,), {"documents": documents}), directory=str(root))
    try:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    except OSError as error:
        raise SystemExit(f"Port {port} for {root} is busy ({error}); each version keeps its own origin, so free that port") from error
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def seed(profile, store):
    copied = []
    for name in CACHES:
        if (Path(store) / name).is_dir():
            shutil.copytree(Path(store) / name, Path(profile) / name)
            copied.append(name)
    return copied


def harvest(profile, store):
    store = Path(store)
    staging = store.with_name(store.name + ".partial")
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    for name in CACHES:
        if (Path(profile) / name).is_dir():
            shutil.copytree(Path(profile) / name, staging / name)
    if store.exists():
        shutil.rmtree(store)
    staging.rename(store)


def android_devices():
    import bench_android
    return bench_android.DEVICES


def android_env(port):
    import bench_android
    return {"BENCH_WEB_PORT": str(port), "ADB": bench_android.devices.adb()}


def host_of(device, platform=sys.platform):
    if not platform.startswith("linux"):
        return "desktop"
    return "android" if device in android_devices() else "linux"


def device_dir(device, browser):
    return f"{device}-{browser}" if device else "web" if browser == "chrome" else f"web-{browser}"


def browser_executable(browser, platform=sys.platform, env=os.environ, exists=os.path.exists, which=shutil.which):
    if env.get("CHROME"):
        return env["CHROME"]
    if platform == "win32":
        roots = {"chrome": ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"], "edge": ["PROGRAMFILES(X86)", "PROGRAMFILES"]}[browser]
        tail = {"chrome": r"Google\Chrome\Application\chrome.exe", "edge": r"Microsoft\Edge\Application\msedge.exe"}[browser]
        candidates = [f"{env[root]}\\{tail}" for root in roots if env.get(root)]
    elif platform == "darwin":
        candidates = [{"chrome": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                       "edge": "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"}[browser]]
    else:
        candidates = [path for name in {"chrome": ["google-chrome", "google-chrome-stable", "chromium"],
                                        "edge": ["microsoft-edge", "microsoft-edge-stable"]}[browser] if (path := which(name))]
    found = next((path for path in candidates if exists(path)), None)
    if not found:
        raise SystemExit(f"No {browser} executable found; set CHROME to its path")
    return found


def node_command(host, name, chain, record, url, output, number, cold, steps, documents, shader_cache, browser, probe, profile=None, executable=None, device=None,
                 trace_steps=(), trace_categories=()):
    return [shutil.which("node") or "node", str(TOOLS / "bench-web.mjs"), "--host", host, "--device", name, "--browser", browser,
            "--url", url, "--chain", str(chain), "--output", str(output), "--pass", str(number), *(["--cold"] if cold else []),
            "--version", record["label"], "--commit", record["commit"], "--wasm-sha256", record["wasm_sha256"], "--shader-cache", shader_cache,
            "--probe", json.dumps(probe), "--memory-interval-ms", str(MEMORY_INTERVAL_MS[host]),
            *(["--profile", str(profile)] if profile else []), *(["--executable", executable] if executable else []),
            *(["--tier", device["tier"], "--refresh-hz", str(device["refresh_hz"])] if device else []), *(["--window", VIEWPORT] if host == "linux" else []),
            *[part for name, file in documents.items() for part in ("--document", f"{name}={file}")],
            *(["--steps", ",".join(steps)] if steps else []),
            *(["--trace-steps", ",".join(trace_steps)] if trace_steps else []),
            *(["--trace-categories", ",".join(trace_categories)] if trace_categories else [])]


def pass_command(host, node, device=None):
    if host == "linux":
        return ["bash", str(TOOLS / "workspace-motion.sh"), "command", *node]
    if host == "android":
        return [sys.executable, str(DEVICES_TOOL), "run", device, "--", "sh", "-c", ANDROID_PASS, "bench-web", *node]
    return node


def probe_command(host):
    return [sys.executable, str(Path(__file__).resolve()), "probe", *(["--android"] if host == "android" else [])]


def launch_pass(command, log, timeout, env=None, scratch=SCRATCH):
    (scratch / "tmp").mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=scratch / "tmp") as temporary, open(log, "w") as stream:
        env = dict(os.environ, **(env or {}), TMPDIR=temporary, LAYER_MOTION_VIEWPORT=VIEWPORT, LAYER_MOTION_REFRESH=str(REFRESH_HZ))
        process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT, env=env, start_new_session=sys.platform != "win32")
        try:
            return process.wait(timeout)
        except subprocess.TimeoutExpired:
            if sys.platform == "win32":
                subprocess.run(["taskkill", "/T", "/F", "/PID", str(process.pid)], capture_output=True)
            else:
                os.killpg(process.pid, signal.SIGTERM)
            return process.wait(30)


def meminfo_available_kib(text):
    match = re.search(r"^MemAvailable:\s+(\d+) kB", text, re.M)
    return int(match[1]) if match else None


def vm_rss_kib(text):
    match = re.search(r"^VmRSS:\s+(\d+) kB", text, re.M)
    return int(match[1]) if match else None


def ps_rss_kib(text):
    return {int(pid): int(rss) for pid, rss in re.findall(r"^\s*(\d+)\s+(\d+)\s*$", text, re.M)}


def vm_stat_available_kib(text):
    size = re.search(r"page size of (\d+) bytes", text)
    pages = [re.search(rf"^Pages {name}:\s+(\d+)\.", text, re.M) for name in ("free", "inactive", "speculative")]
    return sum(int(match[1]) for match in pages if match) * int(size[1]) // 1024 if size else None


def nvidia_kib(text):
    return {int(pid): int(mib) * 1024 for pid, mib in re.findall(r"^\s*(\d+),\s*(\d+)\s*$", text, re.M)}


def typeperf_kib(line):
    cells = [cell.strip().strip('"').strip() for cell in line.strip().split('","')]
    if len(cells) < 2 or cells[0].startswith("(PDH-CSV"):
        return None
    try:
        return int(sum(float(cell) for cell in cells[1:]) / 1024)
    except ValueError:
        return None


def dumpsys_memory(text):
    total = re.search(r"TOTAL PSS:\s+(\d+)", text) or re.search(r"^\s*TOTAL\s+(\d+)", text, re.M)
    graphics = re.search(r"^\s*Graphics:\s+(\d+)", text, re.M)
    return (int(total[1]) if total else None), (int(graphics[1]) if graphics else None)


def android_script(pids):
    return "cat /proc/meminfo" + "".join(f"; echo @@{pid}; dumpsys meminfo --local {pid}" for pid in pids)


def android_reply(text, gpu):
    meminfo, *sections = text.split("@@")
    processes, gpu_kib = {}, None
    for section in sections:
        pid, _, body = section.partition("\n")
        pss, graphics = dumpsys_memory(body)
        if pss is not None:
            processes[int(pid)] = pss
        if gpu is not None and int(pid) == gpu:
            gpu_kib = graphics
    return {"metric": "pss", "os_available_kib": meminfo_available_kib(meminfo), "processes": processes, "gpu_kib": gpu_kib,
            "note": "app_rss_kib is renderer plus GPU process PSS from dumpsys meminfo; gpu_kib is the GPU process Graphics row"}


def android_sample(request, run=subprocess.run):
    pids = [*request["renderers"], *([request["gpu"]] if request["gpu"] else [])]
    return android_reply(run([os.environ.get("ADB", "adb"), "shell", android_script(pids)], capture_output=True, text=True, timeout=25).stdout, request["gpu"])


class Typeperf:
    def __init__(self, popen=subprocess.Popen):
        self.popen, self.pid, self.process, self.value = popen, None, None, None

    def __call__(self, pid):
        if pid != self.pid:
            if self.process:
                self.process.kill()
            self.pid, self.value, self.process = pid, None, None
            if pid:
                counters = [rf"\GPU Process Memory(pid_{pid}_*)\{name} Usage" for name in ("Dedicated", "Shared")]
                self.process = self.popen(["typeperf", *counters, "-si", "1"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
                threading.Thread(target=self.read, args=(self.process,), daemon=True).start()
        return self.value

    def read(self, process):
        for line in process.stdout:
            value = typeperf_kib(line)
            if value is not None and process is self.process:
                self.value = value


def local_sampler(platform=sys.platform, run=subprocess.run, read=lambda path: Path(path).read_text(), which=shutil.which):
    try:
        import psutil
    except ImportError:
        psutil = None

    def processes(pids):
        if psutil:
            found = {}
            for pid in pids:
                try:
                    found[pid] = psutil.Process(pid).memory_info().rss // 1024
                except psutil.Error:
                    pass
            return found, psutil.virtual_memory().available // 1024
        if platform == "darwin":
            listing = run(["ps", "-o", "pid=,rss=", "-p", ",".join(map(str, pids))], capture_output=True, text=True).stdout if pids else ""
            return ps_rss_kib(listing), vm_stat_available_kib(run(["vm_stat"], capture_output=True, text=True).stdout)
        if platform == "win32":
            return windows_memory(pids)
        found = {}
        for pid in pids:
            try:
                found[pid] = vm_rss_kib(read(f"/proc/{pid}/status"))
            except OSError:
                pass
        return {pid: kib for pid, kib in found.items() if kib is not None}, meminfo_available_kib(read("/proc/meminfo"))

    typeperf = Typeperf() if platform == "win32" else None

    def gpu(pid):
        if typeperf:
            return typeperf(pid), None
        if platform.startswith("linux") and which("nvidia-smi"):
            listing = run(["nvidia-smi", "--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"], capture_output=True, text=True, timeout=5).stdout
            return nvidia_kib(listing).get(pid), None
        return None, "this platform exposes no per-process GPU memory to the runner"

    def sample(request):
        pids = [*request["renderers"], *([request["gpu"]] if request["gpu"] else [])]
        found, available = processes(pids)
        gpu_kib, note = gpu(request["gpu"])
        return {"metric": "rss", "os_available_kib": available, "processes": found, "gpu_kib": gpu_kib, "note": note}
    return sample


def windows_memory(pids):
    import ctypes
    from ctypes import wintypes

    class Counters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage",
                                                 "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]

    class Status(ctypes.Structure):
        _fields_ = [("dwLength", wintypes.DWORD), ("dwMemoryLoad", wintypes.DWORD)] + [
            (name, ctypes.c_ulonglong) for name in ("ullTotalPhys", "ullAvailPhys", "ullTotalPageFile", "ullAvailPageFile",
                                                    "ullTotalVirtual", "ullAvailVirtual", "ullAvailExtendedVirtual")]
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    found = {}
    for pid in pids:
        handle = kernel32.OpenProcess(0x1000, False, pid)
        if not handle:
            continue
        counters = Counters(cb=ctypes.sizeof(Counters))
        if kernel32.K32GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
            found[pid] = counters.WorkingSetSize // 1024
        kernel32.CloseHandle(handle)
    status = Status(dwLength=ctypes.sizeof(Status))
    return found, status.ullAvailPhys // 1024 if kernel32.GlobalMemoryStatusEx(ctypes.byref(status)) else None


def probe(args, stdin=sys.stdin, stdout=sys.stdout):
    sample = android_sample if args.android else local_sampler()
    for line in stdin:
        print(json.dumps(sample(json.loads(line))), file=stdout, flush=True)


def split(text):
    return list(dict.fromkeys(item.strip() for item in text.split(",") if item.strip()))


def run(args, build=build, launch=launch_pass, serve=serve, scratch=SCRATCH, runs=RUNS, platform=sys.platform):
    chain = Path(args.chain)
    photo, fixtures = Path(args.photo).expanduser(), Path(args.fixtures).expanduser()
    sources = {"photo": photo, "heavy": fixtures / "heavy.capy", "vector": fixtures / "vector.capy"}
    if missing := [str(path) for path in [chain, *sources.values()] if not path.is_file()]:
        raise SystemExit(f"Missing input files: {', '.join(missing)}")
    chain_steps = json.loads(chain.read_text())["steps"]
    steps = step_closure(chain_steps, split(args.steps)) if args.steps else None
    trace_steps, trace_categories = split(args.trace_steps or ""), split(args.trace_categories or "")
    included = steps or [item["id"] for item in chain_steps]
    if untraced := [step for step in trace_steps if step not in included]:
        raise SystemExit(f"--trace-steps names steps this run does not include: {', '.join(untraced)}")
    host = host_of(args.device, platform)
    if host == "desktop" and not args.bundles:
        raise SystemExit("Windows and macOS run prebuilt bundles: give --bundles (bench_web.py bundle makes them on Linux)")
    executable = None if host == "android" else browser_executable(args.browser, platform)
    tablet = android_devices()[args.device] if host == "android" else None
    documents = {name: path.name for name, path in sources.items()}
    builds = load_bundles(args.bundles) if args.bundles else {record["label"]: record for record in map(build, split(args.versions))}
    name = device_dir(args.device, args.browser)
    servers = {label: serve(record["root"], port_for(f"{name}/{label}"), {path.name: path for path in sources.values()}) for label, record in builds.items()}
    urls = {label: f"http://127.0.0.1:{server.server_address[1]}/" for label, server in servers.items()}
    order = pass_order(list(builds), 1 if args.quick else args.passes, args.quick or host == "android")
    run_dir = runs / args.run_id
    (run_dir / name).mkdir(parents=True, exist_ok=True)
    report = {"schema": 1, "run_id": args.run_id, "host": "web", "web_host": host, "chain": str(chain), "chain_sha256": bench_builds.sha256(chain),
              "steps": steps, "trace_steps": trace_steps, "trace_categories": trace_categories, "versions": list(builds),
              "builds": {label: {key: record.get(key) for key in ("commit", "wasm_sha256", "root", "profile")} | {"url": urls[label]}
                         for label, record in builds.items()},
              "devices": {name: {"tier": tablet["tier"] if tablet else "top", "browser": args.browser, "executable": executable, "device": args.device}},
              "order": order, "started": datetime.datetime.now(datetime.timezone.utc).isoformat(), "passes": []}
    write = lambda: (run_dir / "run.json").write_text(json.dumps(report, indent=2) + "\n")
    write()
    print(f"{run_dir / name}: {len(order)} passes on {host}, about {len(order) * PASS_MINUTES} min", flush=True)
    started = time.monotonic()
    try:
        for entry in order:
            label, number, cold = entry["version"], entry["pass"], entry["cold"]
            profile, store = scratch / "profiles" / args.run_id / f"{label}-pass-{number}", scratch / "gpu-cache" / args.browser / label
            seeded = []
            if host != "android":
                if profile.exists():
                    shutil.rmtree(profile)
                profile.mkdir(parents=True)
                seeded = [] if cold or not store.is_dir() else seed(profile, store)
            output = run_dir / name / label / f"pass-{number}"
            if output.exists():
                shutil.rmtree(output)
            output.mkdir(parents=True)
            print(f"{label} pass {number} ({'cold' if cold else 'warm'})", flush=True)
            began = time.monotonic()
            node = node_command(host, name, chain, builds[label], urls[label], output, number, cold, steps, documents,
                                "device" if host == "android" else "seeded" if seeded else "empty", args.browser, probe_command(host),
                                profile=None if host == "android" else profile, executable=executable, device=tablet,
                                trace_steps=trace_steps, trace_categories=trace_categories)
            code = launch(pass_command(host, node, args.device), output / "runner.log", args.timeout * 60,
                          android_env(servers[label].server_address[1]) if host == "android" else None)
            if host != "android":
                if cold or not store.is_dir():
                    harvest(profile, store)
                shutil.rmtree(profile)
            result = json.loads((output / "pass.json").read_text()) if (output / "pass.json").exists() else {"error": f"no pass.json; see {output / 'runner.log'}"}
            failed = [step["id"] for step in result.get("steps", []) if step.get("status") == "error"]
            skipped = [step["id"] for step in result.get("steps", []) if step.get("status") == "skipped"]
            print(f"  exit {code}; {(result.get('error') or 'ok').splitlines()[0]}; failed: {', '.join(failed) or 'none'}; skipped: {', '.join(skipped) or 'none'}", flush=True)
            report["passes"].append({"version": label, "pass": number, "cold": cold, "dir": str(output), "exit_code": code,
                                     "error": result.get("error"), "failed_steps": failed, "skipped_steps": skipped,
                                     "shader_cache": seeded, "seconds": round(time.monotonic() - began, 1)})
            write()
    finally:
        for server in servers.values():
            server.shutdown()
            server.server_close()
        if (scratch / "profiles" / args.run_id).is_dir():
            shutil.rmtree(scratch / "profiles" / args.run_id)
    report["finished"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    write()
    print(f"Done in {(time.monotonic() - started) / 60:.1f} min: {run_dir}", flush=True)
    print(format_table(analyze_run(run_dir)))
    return report


def parse(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True, metavar="command")
    versions = dict(help="comma-separated tags or git refs, first is the baseline (v1.0.15,origin/main)")
    command = commands.add_parser("run", help="run the chain in palindrome pass order, then analyze")
    sources = command.add_mutually_exclusive_group(required=True)
    sources.add_argument("--versions", **versions)
    sources.add_argument("--bundles", help="directory written by the bundle command, instead of building")
    command.add_argument("--device", help="devices.py tablet name, or a name for this machine such as surfacepro (default: web)")
    command.add_argument("--browser", choices=["chrome", "edge"], default="chrome")
    command.add_argument("--photo", required=True, help="tier JPEG opened by open.photo")
    command.add_argument("--fixtures", required=True, help="directory with heavy.capy and vector.capy for the tier canvas")
    command.add_argument("--chain", default=str(TOOLS / "bench-chain.json"))
    command.add_argument("--passes", type=int, default=2, help="passes per version (default 2: A B B A)")
    command.add_argument("--quick", action="store_true", help="one warm pass per version, no cold pass")
    command.add_argument("--steps", help="comma-separated steps; launch and the opens they need are added")
    command.add_argument("--trace-steps", help="comma-separated steps whose Chrome trace each pass saves as trace-STEP.json")
    command.add_argument("--trace-categories", help="comma-separated Chrome trace categories added while --trace-steps run")
    command.add_argument("--run-id", default=datetime.datetime.now().strftime("%Y%m%d-%H%M%S"))
    command.add_argument("--timeout", type=int, default=45, help="minutes to wait for each pass")
    command = commands.add_parser("bundle", help="build each version and copy its Web tree for another machine")
    command.add_argument("--versions", required=True, **versions)
    command.add_argument("--out", required=True, help="directory to write; copy it with the photo and fixtures to the machine")
    command = commands.add_parser("probe", help="answer bench-web.mjs memory requests on stdin")
    command.add_argument("--android", action="store_true", help="read the tablet through adb inside devices.py run")
    args = parser.parse_args(argv)
    if args.command == "run" and args.passes < 1:
        parser.error("--passes must be at least 1")
    if args.command == "run" and args.trace_categories and not args.trace_steps:
        parser.error("--trace-categories needs --trace-steps")
    return args


if __name__ == "__main__":
    args = parse()
    if args.command == "probe":
        probe(args)
    elif args.command == "bundle":
        bundle(args)
    else:
        report = run(args)
        raise SystemExit(int(any(item["error"] or item["exit_code"] for item in report["passes"])))
