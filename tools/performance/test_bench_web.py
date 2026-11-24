import contextlib
import io
import json
import sys
import tempfile
import unittest
import urllib.request
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import bench_web

CHAIN = {"schema": 1, "motion_frames": 150, "documents": {"photo": {"kind": "photo"}, "heavy": {"kind": "capy", "fixture": "heavy"}},
         "steps": [{"id": "launch", "do": "launch"}, {"id": "open.photo", "do": "open", "document": "photo"},
                   {"id": "nav.pan", "do": "gesture", "gesture": "pan"}, {"id": "open.heavy", "do": "open", "document": "heavy"},
                   {"id": "ui.panel-drag", "do": "drag", "target": "grip:layers", "path": "float-and-return"}]}
RECORD = {"label": "v1.0.15", "commit": "abc", "wasm_sha256": "f" * 64}
DUMPSYS = """Applications Memory Usage (in Kilobytes):
 App Summary
                       Pss(KB)                        Rss(KB)
                        ------                         ------
           Java Heap:        0                              0
            Graphics:    81234                          81234
           TOTAL PSS:   250000            TOTAL RSS:   400000       TOTAL SWAP PSS:      13
"""
LEGACY_DUMPSYS = """                   Pss  Private  Private  SwapPss     Heap
                 Total    Dirty    Clean    Dirty     Size
        TOTAL   123456    100000     2000       10    50000
 App Summary
            Graphics:     4096
"""


def option(command, name):
    return command[command.index(name) + 1] if name in command else None


def node(host, **extra):
    return bench_web.node_command(host, "web", "chain.json", RECORD, "http://127.0.0.1:1/", "out", 1, True, None,
                                  {"photo": "p.jpg", "heavy": "heavy.capy"}, "empty", "chrome", ["python3", "bench_web.py", "probe"], **extra)


class HostTest(unittest.TestCase):
    def test_each_version_keeps_one_origin(self):
        self.assertEqual(bench_web.port_for("v1.0.15"), bench_web.port_for("v1.0.15"))
        self.assertNotEqual(bench_web.port_for("v1.0.15"), bench_web.port_for("gbce8d8d40"))
        self.assertTrue(all(42000 <= bench_web.port_for(label) < 44000 for label in ["v1.0.15", "g0123456789", "x"]))

    def test_hosts_follow_the_device_and_platform(self):
        self.assertEqual(bench_web.host_of("movinkpad14", "linux"), "android")
        self.assertEqual(bench_web.host_of(None, "linux"), "linux")
        self.assertEqual(bench_web.host_of("surfacepro", "win32"), "desktop")
        self.assertEqual(bench_web.host_of("macmini", "darwin"), "desktop")
        self.assertEqual([bench_web.device_dir(None, "chrome"), bench_web.device_dir(None, "edge"), bench_web.device_dir("surfacepro", "edge")],
                         ["web", "web-edge", "surfacepro-edge"])

    def test_linux_passes_run_in_the_private_display(self):
        command = bench_web.pass_command("linux", node("linux", profile="profile", executable="/usr/bin/google-chrome"))
        self.assertEqual(command[:3], ["bash", str(bench_web.TOOLS / "workspace-motion.sh"), "command"])
        self.assertEqual(command[4:7], [str(bench_web.TOOLS / "bench-web.mjs"), "--host", "linux"])
        self.assertEqual((option(command, "--window"), option(command, "--profile"), option(command, "--memory-interval-ms")), (bench_web.VIEWPORT, "profile", "1000"))
        self.assertEqual(json.loads(option(command, "--probe")), ["python3", "bench_web.py", "probe"])
        self.assertIn("--cold", command)
        for name in ["--steps", "--trace-steps", "--trace-categories"]:
            self.assertNotIn(name, command)
        self.assertEqual([command[i + 1] for i, part in enumerate(command) if part == "--document"], ["photo=p.jpg", "heavy=heavy.capy"])

    def test_desktop_passes_start_the_browser_directly(self):
        command = bench_web.pass_command("desktop", node("desktop", profile="C:\\p", executable="C:\\msedge.exe"))
        self.assertEqual(command[1:4], [str(bench_web.TOOLS / "bench-web.mjs"), "--host", "desktop"])
        self.assertEqual((option(command, "--executable"), option(command, "--profile")), ("C:\\msedge.exe", "C:\\p"))
        self.assertNotIn("--window", command)

    def test_android_passes_run_inside_the_reservation_with_their_own_forwards(self):
        command = bench_web.pass_command("android", node("android", device={"tier": "top", "refresh_hz": 120}), "movinkpad14")
        self.assertEqual(command[1:5], [str(bench_web.DEVICES_TOOL), "run", "movinkpad14", "--"])
        self.assertEqual(command[5:7], ["sh", "-c"])
        script = command[7]
        for part in ['adb() { "$ADB" "$@"; }', "isKeyguardShowing=true", "mWakefulness=Awake", "adb reverse tcp:$BENCH_WEB_PORT tcp:$BENCH_WEB_PORT",
                     "adb forward tcp:$CAPY_CDP_PORT localabstract:chrome_devtools_remote", "am start -n " + bench_web.ANDROID_CHROME,
                     'adb forward --remove tcp:$CAPY_CDP_PORT; adb reverse --remove tcp:$BENCH_WEB_PORT']:
            self.assertIn(part, script)
        self.assertNotIn("pm clear", script)
        self.assertEqual((option(command, "--tier"), option(command, "--refresh-hz"), option(command, "--memory-interval-ms"), option(command, "--profile")),
                         ("top", "120", "5000", None))
        self.assertIn("keyevent KEYCODE_HOME", script)
        self.assertEqual(bench_web.probe_command("android")[-2:], ["probe", "--android"])
        self.assertEqual(bench_web.probe_command("desktop")[-1], "probe")

    def test_browser_lookup_per_platform(self):
        env = {"PROGRAMFILES": r"C:\Program Files", "PROGRAMFILES(X86)": r"C:\Program Files (x86)", "LOCALAPPDATA": r"C:\Users\a\AppData\Local"}
        present = {r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", r"C:\Users\a\AppData\Local\Google\Chrome\Application\chrome.exe"}
        self.assertEqual(bench_web.browser_executable("edge", "win32", env, present.__contains__), r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
        self.assertEqual(bench_web.browser_executable("chrome", "win32", env, present.__contains__), r"C:\Users\a\AppData\Local\Google\Chrome\Application\chrome.exe")
        self.assertEqual(bench_web.browser_executable("chrome", "darwin", {}, lambda path: True), "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        self.assertEqual(bench_web.browser_executable("edge", "linux", {}, lambda path: True, {"microsoft-edge-stable": "/usr/bin/microsoft-edge-stable"}.get),
                         "/usr/bin/microsoft-edge-stable")
        self.assertEqual(bench_web.browser_executable("edge", "darwin", {"CHROME": "/opt/browser"}, lambda path: False), "/opt/browser")
        with self.assertRaisesRegex(SystemExit, "No chrome executable"):
            bench_web.browser_executable("chrome", "win32", env, lambda path: False)


class MemoryTest(unittest.TestCase):
    def test_platform_tool_output_parses_to_kib(self):
        self.assertEqual(bench_web.meminfo_available_kib("MemTotal: 10 kB\nMemAvailable:   12345 kB\n"), 12345)
        self.assertEqual(bench_web.vm_rss_kib("Name: chrome\nVmRSS:\t  2048 kB\n"), 2048)
        self.assertIsNone(bench_web.vm_rss_kib(""))
        self.assertEqual(bench_web.ps_rss_kib("  101  5000\n 202 70\n"), {101: 5000, 202: 70})
        vm_stat = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:      100.\nPages active:  9.\nPages inactive:  50.\nPages speculative:   10.\n"
        self.assertEqual(bench_web.vm_stat_available_kib(vm_stat), 160 * 16)
        self.assertEqual(bench_web.nvidia_kib("12, 541\n13, 7\nbad\n"), {12: 541 * 1024, 13: 7 * 1024})
        self.assertIsNone(bench_web.typeperf_kib('"(PDH-CSV 4.0)","\\\\PC\\GPU Process Memory(pid_9_luid_0x1_phys_0)\\Dedicated Usage"'))
        self.assertEqual(bench_web.typeperf_kib('"11/23/2026 10:00:00.000","1048576.000000","2048.000000"\n'), 1026)
        self.assertIsNone(bench_web.typeperf_kib('"11/23/2026 10:00:00.000"," "'))

    def test_android_reads_pss_and_graphics_in_one_shell_call(self):
        self.assertEqual(bench_web.dumpsys_memory(DUMPSYS), (250000, 81234))
        self.assertEqual(bench_web.dumpsys_memory(LEGACY_DUMPSYS), (123456, 4096))
        self.assertEqual(bench_web.android_script([5, 9]), "cat /proc/meminfo; echo @@5; dumpsys meminfo --local 5; echo @@9; dumpsys meminfo --local 9")
        calls = []

        def run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(stdout=f"MemAvailable:  777 kB\n@@5\n{LEGACY_DUMPSYS}@@9\n{DUMPSYS}@@11\nNo process found for: 11\n")
        reply = bench_web.android_sample({"renderers": [5, 11], "gpu": 9}, run)
        self.assertEqual(calls, [["adb", "shell", bench_web.android_script([5, 11, 9])]])
        self.assertEqual((reply["metric"], reply["os_available_kib"], reply["processes"], reply["gpu_kib"]), ("pss", 777, {5: 123456, 9: 250000}, 81234))

    def test_local_fallbacks_read_the_platform_tools(self):
        with patch.dict(sys.modules, {"psutil": None}):
            files = {"/proc/meminfo": "MemAvailable: 64 kB\n", "/proc/5/status": "VmRSS:  10 kB\n", "/proc/9/status": "VmRSS:  20 kB\n"}

            def read(path):
                if path not in files:
                    raise FileNotFoundError(path)
                return files[path]
            nvidia = lambda command, **kwargs: SimpleNamespace(stdout="9, 3\n")
            sample = bench_web.local_sampler("linux", nvidia, read, lambda name: "/usr/bin/nvidia-smi")
            self.assertEqual(sample({"renderers": [5, 6], "gpu": 9}),
                             {"metric": "rss", "os_available_kib": 64, "processes": {5: 10, 9: 20}, "gpu_kib": 3072, "note": None})
            outputs = {"ps": " 5 300\n 9 400\n", "vm_stat": "(page size of 4096 bytes)\nPages free: 2.\nPages inactive: 2.\n"}
            calls = []

            def run(command, **kwargs):
                calls.append(command)
                return SimpleNamespace(stdout=outputs[command[0]])
            reply = bench_web.local_sampler("darwin", run)({"renderers": [5], "gpu": 9})
            self.assertEqual(calls[0], ["ps", "-o", "pid=,rss=", "-p", "5,9"])
            self.assertEqual((reply["processes"], reply["os_available_kib"], reply["gpu_kib"]), ({5: 300, 9: 400}, 16, None))
            self.assertIn("no per-process GPU memory", reply["note"])

    def test_psutil_reports_working_sets_when_installed(self):
        class Error(Exception):
            pass

        def process(pid):
            if pid == 6:
                raise Error()
            return SimpleNamespace(memory_info=lambda: SimpleNamespace(rss=pid * 2048))
        psutil = SimpleNamespace(Error=Error, Process=process, virtual_memory=lambda: SimpleNamespace(available=4096))
        with patch.dict(sys.modules, {"psutil": psutil}):
            reply = bench_web.local_sampler("darwin", None)({"renderers": [5, 6], "gpu": 9})
        self.assertEqual((reply["processes"], reply["os_available_kib"]), ({5: 10, 9: 18}, 4))

    def test_typeperf_follows_the_gpu_process(self):
        started = []

        class Process:
            def __init__(self, command, **kwargs):
                started.append(command)
                self.stdout = io.StringIO('"(PDH-CSV 4.0)","a","b"\n"t","2048","1024"\n' if len(started) == 1 else '')

            def kill(self):
                started.append("killed")
        reader = bench_web.Typeperf(Process)
        reader(9)
        reader.read(reader.process)
        self.assertEqual(reader(9), 3)
        self.assertEqual(started[0], ["typeperf", r"\GPU Process Memory(pid_9_*)\Dedicated Usage", r"\GPU Process Memory(pid_9_*)\Shared Usage", "-si", "1"])
        self.assertIsNone(reader(12))
        self.assertEqual(started[1], "killed")

    def test_probe_answers_one_line_per_request(self):
        stdout = io.StringIO()
        with patch.object(bench_web, "android_sample", lambda request: {"echo": request["renderers"]}):
            bench_web.probe(SimpleNamespace(android=True), io.StringIO('{"renderers": [1], "gpu": null}\n{"renderers": [2], "gpu": 3}\n'), stdout)
        self.assertEqual(stdout.getvalue().splitlines(), ['{"echo": [1]}', '{"echo": [2]}'])


class FileTest(unittest.TestCase):
    def test_warm_profiles_receive_only_the_gpu_caches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "cold/Default/DawnWebGPUCache").mkdir(parents=True)
            (root / "cold/Default/DawnWebGPUCache/data_0").write_text("pipelines")
            (root / "cold/GPUPersistentCache").mkdir()
            (root / "cold/Default/IndexedDB").mkdir()
            bench_web.harvest(root / "cold", root / "store")
            bench_web.harvest(root / "cold", root / "store")
            self.assertEqual(sorted(path.relative_to(root / "store").as_posix() for path in (root / "store").rglob("*")),
                             ["Default", "Default/DawnWebGPUCache", "Default/DawnWebGPUCache/data_0", "GPUPersistentCache"])
            self.assertEqual(bench_web.seed(root / "warm", root / "store"), ["GPUPersistentCache", "Default/DawnWebGPUCache"])
            self.assertEqual((root / "warm/Default/DawnWebGPUCache/data_0").read_text(), "pipelines")
            self.assertFalse((root / "warm/Default/IndexedDB").exists())

    def test_bundles_carry_the_served_tree_without_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for label in ["v1", "g2"]:
                site = root / "trees" / label
                (site / "pkg").mkdir(parents=True)
                (site / "src").mkdir()
                (site / "pkg/layer_web_bg.wasm").write_bytes(label.encode())
                for name in ["index.html", "app.js", "editor.test.mjs", "Cargo.toml", "src/lib.rs"]:
                    (site / name).write_text(name)

            def build(version):
                label = "g2" if version == "origin/main" else version
                site = root / "trees" / label
                return {"version": version, "label": label, "commit": f"c-{label}", "root": str(site), "profile": "web-release",
                        "wasm_sha256": bench_web.bench_builds.sha256(site / "pkg/layer_web_bg.wasm")}
            with contextlib.redirect_stdout(io.StringIO()):
                bench_web.bundle(bench_web.parse(["bundle", "--versions", "v1,origin/main", "--out", str(root / "out")]), build)
            self.assertEqual(sorted(path.name for path in (root / "out/g2/site").iterdir()), ["app.js", "index.html", "pkg"])
            builds = bench_web.load_bundles(root / "out")
            self.assertEqual(list(builds), ["v1", "g2"])
            self.assertEqual((builds["g2"]["commit"], builds["g2"]["root"]), ("c-g2", str(root / "out/g2/site")))
            (root / "out/v1/site/pkg/layer_web_bg.wasm").write_bytes(b"changed")
            with self.assertRaisesRegex(SystemExit, "does not hold the v1 build"):
                bench_web.load_bundles(root / "out")

    def test_site_serves_the_tree_and_the_fixtures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "site").mkdir()
            (root / "site/index.html").write_text("app")
            (root / "photo one.jpg").write_bytes(b"jpeg")
            server = bench_web.serve(root / "site", 0, {"photo one.jpg": root / "photo one.jpg"})
            try:
                base = f"http://127.0.0.1:{server.server_address[1]}"
                with urllib.request.urlopen(base + "/") as response:
                    self.assertEqual((response.read(), response.headers["Cache-Control"]), (b"app", "no-store"))
                with urllib.request.urlopen(base + "/capy-bench/photo%20one.jpg") as response:
                    self.assertEqual(response.read(), b"jpeg")
            finally:
                server.shutdown()
                server.server_close()

    def test_launch_removes_the_pass_temporary_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = 'mkdir "$TMPDIR/capy-workspace-motion.abc"; echo "logs $TMPDIR $BENCH_WEB_PORT"; exit 3'
            self.assertEqual(bench_web.launch_pass(["bash", "-c", script], root / "runner.log", 30, {"BENCH_WEB_PORT": "42"}, scratch=root), 3)
            self.assertIn(f"logs {root / 'tmp'}/", (root / "runner.log").read_text())
            self.assertIn(" 42", (root / "runner.log").read_text())
            self.assertEqual(list((root / "tmp").iterdir()), [])


class RunTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        (self.root / "fixtures").mkdir()
        for name in ["fixtures/heavy.capy", "fixtures/vector.capy", "tier.jpg"]:
            (self.root / name).write_bytes(name.encode())
        (self.root / "chain.json").write_text(json.dumps(CHAIN))
        self.launches, self.served = [], []

    def tearDown(self):
        self.directory.cleanup()

    def build(self, version):
        return {"label": version.replace("/", "_"), "commit": f"commit-{version}", "wasm_sha256": "0" * 64, "root": str(self.root), "profile": "web-release"}

    def serve(self, site, port, documents):
        self.served.append((site, port, sorted(documents)))
        return SimpleNamespace(server_address=("127.0.0.1", port), shutdown=lambda: None, server_close=lambda: None)

    def launch(self, command, log, timeout, env):
        output = Path(option(command, "--output"))
        profile = option(command, "--profile")
        cold = "--cold" in command
        self.launches.append(SimpleNamespace(command=command, env=env, version=option(command, "--version"), number=option(command, "--pass"),
                                             cold=cold, cache=option(command, "--shader-cache"), steps=option(command, "--steps")))
        if profile:
            profile = Path(profile)
            self.assertEqual((profile / "Default/DawnWebGPUCache/data_0").exists(), not cold)
            (profile / "Default/DawnWebGPUCache").mkdir(parents=True, exist_ok=True)
            (profile / "Default/DawnWebGPUCache/data_0").write_text("pipelines")
        Path(log).write_text("runner")
        frames = [{"t_ns": t * 8_000_000, "surface": "canvas"} for t in range(200)]
        (output / "frames.jsonl").write_text("".join(json.dumps(row) + "\n" for row in frames))
        (output / "memory.jsonl").write_text(json.dumps({"t_ns": 1, "app_rss_kib": 5}) + "\n")
        (output / "pass.json").write_text(json.dumps({"pass": int(option(command, "--pass")), "cold": cold, "host": "web", "commit": option(command, "--commit"),
            "refresh_hz": 120, "tier": option(command, "--tier"), "frames_evidence": "presented", "steps": [
                {"id": "nav.pan", "status": "ok", "surface": "canvas", "begin_ns": 0, "end_ns": 1_600_000_000,
                 "motion": [{"begin_ns": 0, "end_ns": 1_600_000_000}], "contacts": [], "completions": [], "latency": []},
                {"id": "ui.panel-drag", "status": "skipped", "reason": "no right column in this workspace"}]}))
        return 0

    def run_bench(self, argv, platform="linux"):
        args = bench_web.parse(["run", "--photo", str(self.root / "tier.jpg"), "--fixtures", str(self.root / "fixtures"),
                                "--chain", str(self.root / "chain.json"), "--run-id", "r1", *argv])
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed), patch.object(bench_web, "browser_executable", lambda browser, platform: f"/bin/{browser}"):
            report = bench_web.run(args, build=self.build, launch=self.launch, serve=self.serve, scratch=self.root / "scratch",
                                   runs=self.root / "runs", platform=platform)
        return report, printed.getvalue()

    def test_linux_run_mirrors_passes_seeds_warm_caches_and_analyzes(self):
        report, printed = self.run_bench(["--versions", "v1.0.15,origin/main", "--steps", "nav.pan,ui.panel-drag"])
        self.assertEqual([(item.version, item.number, item.cold) for item in self.launches],
                         [("v1.0.15", "1", True), ("origin_main", "1", True), ("origin_main", "2", False), ("v1.0.15", "2", False)])
        self.assertEqual([item.cache for item in self.launches], ["empty", "empty", "seeded", "seeded"])
        self.assertEqual({item.steps for item in self.launches}, {"launch,open.photo,nav.pan,open.heavy,ui.panel-drag"})
        self.assertEqual({item.command[0] for item in self.launches}, {"bash"})
        self.assertEqual({option(item.command, "--executable") for item in self.launches}, {"/bin/chrome"})
        self.assertEqual([port for _, port, _ in self.served], [bench_web.port_for("web/v1.0.15"), bench_web.port_for("web/origin_main")])
        self.assertEqual(self.served[0][2], ["heavy.capy", "tier.jpg", "vector.capy"])
        run_dir = self.root / "runs/r1"
        saved = json.loads((run_dir / "run.json").read_text())
        self.assertEqual((saved["versions"], list(saved["devices"]), saved["web_host"]), (["v1.0.15", "origin_main"], ["web"], "linux"))
        self.assertEqual(saved["passes"][0]["skipped_steps"], ["ui.panel-drag"])
        self.assertEqual(report["builds"]["v1.0.15"]["url"], f"http://127.0.0.1:{bench_web.port_for('web/v1.0.15')}/")
        self.assertTrue((run_dir / "web/origin_main/pass-2/runner.log").is_file())
        self.assertFalse((self.root / "scratch/profiles/r1").exists())
        self.assertTrue((self.root / "scratch/gpu-cache/chrome/v1.0.15/Default/DawnWebGPUCache/data_0").is_file())
        web = json.loads((run_dir / "summary.json").read_text())["devices"]["web"]
        self.assertEqual((web["tier"], web["refresh_hz"]), ("top", 120))
        self.assertAlmostEqual(web["steps"]["nav.pan"]["metrics"]["fps"]["versions"]["v1.0.15"]["value"], 125.0)
        self.assertEqual(web["steps"]["ui.panel-drag"]["problems"]["v1.0.15"]["status"], "skipped")
        self.assertIn("web: top tier", printed)

    def test_android_run_uses_the_reservation_and_only_warm_passes(self):
        self.run_bench(["--versions", "v1.0.15,origin/main", "--device", "tcl"])
        self.assertEqual([(item.version, item.number, item.cold) for item in self.launches],
                         [("v1.0.15", "1", False), ("origin_main", "1", False), ("origin_main", "2", False), ("v1.0.15", "2", False)])
        first = self.launches[0]
        self.assertEqual((first.command[2:4], option(first.command, "--device")), (["run", "tcl"], "tcl-chrome"))
        self.assertEqual((first.env["BENCH_WEB_PORT"], first.cache, option(first.command, "--tier"), option(first.command, "--refresh-hz")),
                         (str(bench_web.port_for("tcl-chrome/v1.0.15")), "device", "low", "60"))
        self.assertTrue(first.env["ADB"].endswith("adb"))
        self.assertIsNone(option(first.command, "--profile"))
        self.assertFalse((self.root / "scratch/gpu-cache").exists())
        summary = json.loads((self.root / "runs/r1/summary.json").read_text())
        self.assertEqual(summary["devices"]["tcl-chrome"]["tier"], "low")

    def test_traced_steps_reach_every_pass_and_must_run(self):
        report, _ = self.run_bench(["--versions", "v1.0.15", "--trace-steps", "nav.pan", "--trace-categories", "gpu,v8"])
        self.assertEqual({(option(item.command, "--trace-steps"), option(item.command, "--trace-categories")) for item in self.launches},
                         {("nav.pan", "gpu,v8")})
        self.assertEqual((report["trace_steps"], report["trace_categories"]), (["nav.pan"], ["gpu", "v8"]))
        with self.assertRaisesRegex(SystemExit, "does not include: open.heavy"):
            self.run_bench(["--versions", "v1.0.15", "--steps", "nav.pan", "--trace-steps", "nav.pan,open.heavy"])
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            bench_web.parse(["run", "--versions", "v1.0.15", "--photo", "p", "--fixtures", "f", "--trace-categories", "gpu"])

    def test_desktop_runs_need_bundles(self):
        with self.assertRaisesRegex(SystemExit, "give --bundles"):
            self.run_bench(["--versions", "v1.0.15", "--device", "surfacepro"], platform="win32")


if __name__ == "__main__":
    unittest.main()
