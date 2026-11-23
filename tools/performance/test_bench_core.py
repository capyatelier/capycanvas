import hashlib
import json
from pathlib import Path
import subprocess
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

import bench
import bench_android
import bench_builds
import bench_perfetto

CHAIN = json.loads(bench.CHAIN.read_text())


def order(entries):
    return [(entry["version"], entry["pass"], entry["cold"]) for entry in entries]


class PassOrderTest(unittest.TestCase):
    def test_two_versions_run_a_b_b_a_with_cold_first_passes(self):
        self.assertEqual(order(bench.pass_order(["A", "B"], 2)), [("A", 1, True), ("B", 1, True), ("B", 2, False), ("A", 2, False)])

    def test_three_versions_mirror(self):
        self.assertEqual([version for version, _, _ in order(bench.pass_order(["A", "B", "C"], 2))], list("ABCCBA"))

    def test_quick_runs_one_warm_pass_per_version(self):
        self.assertEqual(order(bench.pass_order(["A", "B"], 1, quick=True)), [("A", 1, False), ("B", 1, False)])

    def test_more_passes_keep_alternating(self):
        self.assertEqual(order(bench.pass_order(["A", "B"], 3)),
                         [("A", 1, True), ("B", 1, True), ("B", 2, False), ("A", 2, False), ("A", 3, False), ("B", 3, False)])


class StepClosureTest(unittest.TestCase):
    def closure(self, *wanted):
        return bench.step_closure(CHAIN["steps"], list(wanted))

    def test_step_needs_launch_and_most_recent_open(self):
        self.assertEqual(self.closure("heavy.draw"), ["launch", "open.heavy", "heavy.draw"])
        self.assertEqual(self.closure("draw.liquify"), ["launch", "open.photo", "draw.liquify"])
        self.assertEqual(self.closure("vector.pan", "undo"), ["launch", "open.photo", "undo", "open.vector", "vector.pan"])

    def test_open_needs_only_launch(self):
        self.assertEqual(self.closure("open.heavy"), ["launch", "open.heavy"])

    def test_tab_switch_needs_both_documents(self):
        self.assertEqual(self.closure("switch.tab"), ["launch", "open.photo", "open.heavy", "switch.tab"])

    def test_unknown_step_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "draw.nothing"):
            self.closure("draw.nothing")


class VersionTest(unittest.TestCase):
    def test_tag_keeps_its_name_and_ref_uses_short_commit(self):
        self.assertEqual(bench_builds.label("v1.0.15", "1" * 40, True), "v1.0.15")
        self.assertEqual(bench_builds.label("origin/main", "5a0ba630621130e9f6fb3baeea83bca9c6a815b6", False), "g5a0ba6306")

    def test_app_id_and_label(self):
        self.assertEqual(bench_builds.app_id("v1.0.15"), "art.capycanvas.bench.v1_0_15")
        self.assertEqual(bench_builds.app_id("v1.1.0-rc1"), "art.capycanvas.bench.v1_1_0_rc1")
        self.assertEqual(bench_builds.app_id("g5a0ba6306"), "art.capycanvas.bench.g5a0ba6306")
        self.assertEqual(bench_builds.app_label("g5a0ba6306"), "Capy Bench g5a0ba6306")


class DeviceTest(unittest.TestCase):
    def test_guarantee_size_follows_canvas_pixels(self):
        self.assertEqual([bench_android.guarantee_size(*bench_android.CANVAS[tier]) for tier in ("low", "mid", "high", "top")], [1024, 1536, 2048, 2048])
        self.assertEqual(bench_android.guarantee_size(5000, 4000), 1536)
        self.assertEqual(bench_android.guarantee_size(8000, 5000), 2048)

    def test_device_table(self):
        self.assertEqual({name: (spec["tier"], spec["refresh_hz"]) for name, spec in bench_android.DEVICES.items() if spec["adaptive"]},
                         {"movinkpad11": ("mid", 90)})
        self.assertEqual(bench_android.DEVICES["sm_n970u1"]["photo"], "/data/local/tmp/capy-tier-12mp.jpg")
        self.assertEqual(bench_android.DEVICES["movinkpad14"]["photo"], "/data/local/tmp/capy-tier-61mp.jpg")

    def test_display_is_reset_to_factory_and_restored(self):
        state = dict(wm_size="Physical size: 1080x2280\nOverride size: 900x1900", wm_density="Physical density: 420",
                     font_scale="1.1")
        self.assertEqual(bench_android.display_commands(state), (
            [["wm", "size", "reset"], ["settings", "put", "system", "font_scale", "1.0"]],
            [["wm", "size", "900x1900"], ["settings", "put", "system", "font_scale", "1.1"]]))
        factory = dict(wm_size="Physical size: 1080x2280", wm_density="Physical density: 420", font_scale="null")
        self.assertEqual(bench_android.display_commands(factory), ([], []))
        self.assertEqual(bench_android.display_commands(dict(factory, font_scale="1.0")), ([], []))

    def test_instrumentation_arguments(self):
        record = dict(device="movinkpad11", refresh_hz=90, steps=["launch", "open.photo"],
                      versions={"v1.0.15": dict(app_id="art.capycanvas.bench.v1_0_15", commit="1eb7d6d16975cb08f72da1df3fc6fb5d8b08f519")},
                      documents={"photo": dict(remote="/data/local/tmp/capy-tier-24mp.jpg")})
        args = bench_android.instrument_args(record, dict(version="v1.0.15", **{"pass": 1}, cold=True), "token")
        extras = dict(zip(args[5::3], args[6::3]))
        self.assertEqual(args[:4], ["shell", "am", "instrument", "-w"])
        self.assertEqual(args[-1], "art.capycanvas.bench.v1_0_15/art.capycanvas.BenchChainInstrumentation")
        self.assertEqual(extras, {"chain": "/data/local/tmp/capy-bench-chain.json", "pass": "1", "cold": "true", "device": "movinkpad11",
                                  "version": "v1.0.15", "commit": "1eb7d6d16975cb08f72da1df3fc6fb5d8b08f519",
                                  "photo": "/data/local/tmp/capy-tier-24mp.jpg", "steps": "launch,open.photo",
                                  "waitForGo": "true", "benchmarkReadyToken": "token", "expectedRefreshHz": "90"})


class PassProgressTest(unittest.TestCase):
    def test_android_result_code_distinguishes_a_crash_from_success(self):
        self.assertIsNone(bench_android.instrumentation_error("INSTRUMENTATION_CODE: -1\n", None))
        log = "BENCH_STEP launch ok\nINSTRUMENTATION_RESULT: shortMsg=Process crashed.\nINSTRUMENTATION_CODE: 0\n"
        self.assertEqual(bench_android.instrumentation_error(log, "open.photo"), "Process crashed.")
        self.assertEqual(bench_android.instrumentation_error("BENCH_STEP launch ok\n", "open.photo"),
                         "Instrumentation did not report a result code")

    def test_a_success_code_cannot_hide_an_early_end(self):
        log = "BENCH_STEP select.wand error Canvas GPU stopped\nINSTRUMENTATION_CODE: -1\n"
        self.assertEqual(bench_android.instrumentation_error(log, "fill"), "Instrumentation ended before fill")

    def test_completed_pass_saves_logcat_on_step_error(self):
        entry = {"version": "A", "pass": 1, "cold": False}
        record = {"adaptive": None, "steps": ["launch"], "device": "tcl", "documents": {},
                  "versions": {"A": {"app_id": "art.capycanvas.bench.a", "commit": "a"}}}
        for status in ("ok", "error"):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as tmp:
                adb = FakeAdb({("logcat", "-d", "-v", "threadtime"): "device log"})
                adb.prefix = ["adb", "-s", "test"]
                process = SimpleNamespace(poll=lambda: 0, wait=lambda **_: 0, returncode=0)

                def instrument(args, stdout, **options):
                    stdout.write(f"BENCH_STEP launch {status} \nINSTRUMENTATION_CODE: -1\n")
                    return process

                with patch.object(bench_android.subprocess, "Popen", side_effect=instrument), \
                     patch.object(bench_android.subprocess, "run", return_value=SimpleNamespace(returncode=0)):
                    result = bench_android.run_pass(adb, record, entry, Path(tmp))
                self.assertTrue(result["ok"])
                logcat = Path(tmp) / "A/pass-1/logcat.txt"
                if status == "error":
                    self.assertEqual(logcat.read_text(), "device log")
                else:
                    self.assertFalse(logcat.exists())

    def test_quiet_instrumentation_is_bounded(self):
        process = SimpleNamespace(wait=lambda **_: self.expire())
        log = SimpleNamespace(stat=lambda: SimpleNamespace(st_size=12))
        with patch.object(bench_android.time, "monotonic", side_effect=[0, bench_android.PASS_PROGRESS_TIMEOUT_S]):
            with self.assertRaisesRegex(RuntimeError, "No instrumentation progress"):
                bench_android.wait_for_progress(process, log)

    def test_a_completed_step_renews_the_progress_deadline(self):
        process = SimpleNamespace(wait=lambda **_: self.expire())
        sizes = iter((0, 12, 12, 12))
        log = SimpleNamespace(stat=lambda: SimpleNamespace(st_size=next(sizes)))
        deadline = bench_android.PASS_PROGRESS_TIMEOUT_S
        with patch.object(bench_android.time, "monotonic", side_effect=[0, deadline - 1, deadline, 2 * deadline - 1]):
            with self.assertRaisesRegex(RuntimeError, "No instrumentation progress"):
                bench_android.wait_for_progress(process, log)

    def expire(self):
        raise subprocess.TimeoutExpired("am instrument", 1)


class FakeAdb:
    def __init__(self, outputs):
        self.outputs = outputs
        self.calls = []

    def __call__(self, *args, check=True, **options):
        self.calls.append(args)
        return self.outputs.get(args, "")


class TransferTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.local = Path(directory.name) / "photo.jpg"
        self.local.write_bytes(b"photo")
        self.digest = hashlib.sha256(b"photo").hexdigest()
        self.remote = "/data/local/tmp/capy-tier-61mp.jpg"

    def test_matching_file_is_kept(self):
        adb = FakeAdb({("shell", "sha256sum", self.remote): f"{self.digest}  {self.remote}\n"})
        self.assertEqual(bench_android.place(adb, self.local, self.remote), "kept")
        self.assertEqual(adb.calls, [("shell", "sha256sum", self.remote)])

    def test_matching_file_under_another_name_is_copied_on_device(self):
        other = "/data/local/tmp/capy-brush-photo.jpg"
        adb = FakeAdb({("shell", "sha256sum", self.remote): "sha256sum: missing: No such file or directory\n",
                       ("shell", "sha256sum /data/local/tmp/*.jpg"): f"{'0' * 64}  /data/local/tmp/a.jpg\n{self.digest}  {other}\n"})
        self.assertEqual(bench_android.place(adb, self.local, self.remote), "copied")
        self.assertEqual(adb.calls[-1], ("shell", "cp", other, self.remote))

    def test_changed_file_is_pushed(self):
        adb = FakeAdb({("shell", "sha256sum", self.remote): f"{'0' * 64}  {self.remote}\n"})
        self.assertEqual(bench_android.place(adb, self.local, self.remote), "pushed")
        self.assertEqual(adb.calls[-1], ("push", str(self.local), self.remote))

    def test_install_only_when_installed_apk_differs(self):
        path = "/data/app/~~x/art.capycanvas.bench.v1_0_15-y/base.apk"
        outputs = {("shell", "pm", "path", "art.capycanvas.bench.v1_0_15"): f"package:{path}\n",
                   ("shell", "sha256sum", path): f"{self.digest}  {path}\n"}
        adb = FakeAdb(outputs)
        self.assertFalse(bench_android.install(adb, "art.capycanvas.bench.v1_0_15", self.local, self.digest))
        self.assertFalse(any(call[0] == "install" for call in adb.calls))
        adb = FakeAdb(outputs)
        self.assertTrue(bench_android.install(adb, "art.capycanvas.bench.v1_0_15", self.local, "1" * 64))
        self.assertEqual(adb.calls[-1], ("install", "-r", "--no-incremental", str(self.local)))
        adb = FakeAdb({})
        self.assertTrue(bench_android.install(adb, "art.capycanvas.bench.v1_0_15", self.local, self.digest))


CANVAS_CSV = '''Loading trace\n"latch","presented_at"
"latchBuffer SurfaceView[app/art.capycanvas.MainActivity]#3 - 7",1000
"latchBuffer SurfaceView[app/art.capycanvas.MainActivity]#3 - 8",1016
"latchBuffer SurfaceView[app/art.capycanvas.MainActivity]#9 - 7",5000
'''
WINDOW_CSV = '''"latch","presented_at"
"latchBuffer app/art.capycanvas.MainActivity#2 - 1",1010
"latchBuffer PopupWindow:abc#4 - 1",1010
"latchBuffer app/art.capycanvas.MainActivity#2 - 2",5010
'''
MEMORY_CSV = '''"ts","name","value"
1000,"MemAvailable",2147483648.0
1000,"mem.rss",524288000
1005,"SwapFree",1048576
1005,"GPU Memory",10240.0
6000,"MemAvailable",1073741824
'''


def processor(command, **options):
    sql = command[-1]
    return SimpleNamespace(stdout=MEMORY_CSV if "counter_track" in sql else WINDOW_CSV if "owned_layers" in sql else CANVAS_CSV)


class PerfettoTest(unittest.TestCase):
    def test_trace_keeps_lean_capture_and_adds_memory_sources(self):
        config = bench_perfetto.trace_config(60_000)
        self.assertNotIn("atrace_apps", config)
        for source in ('"linux.process_stats"', "proc_stats_poll_ms: 1000", '"linux.sys_stats"', "meminfo_period_ms: 1000",
                       '"android.gpu.memory"', '"gpu_mem/gpu_mem_total"', '"android.surfaceflinger.frametimeline"', "duration_ms: 60000"):
            self.assertIn(source, config)

    def test_session_trace_keeps_repeated_buffer_numbers(self):
        with patch("subprocess.run", side_effect=processor):
            times = bench_perfetto.present_times("shell", "trace", "app")
        self.assertEqual(times, {"canvas": [1000, 1016, 5000], "window": [1010, 5010]})

    def test_unlabeled_surface_flinger_latches_use_display_frame_tokens(self):
        def process(command, **options):
            sql = command[-1]
            self.assertIn("display.display_frame_token IN (SELECT token FROM tokens)", sql)
            self.assertEqual(command[1:4], ["query", "--format", "csv"])
            if "LAG(c.value)" in sql:
                self.assertIn("dispatch.name GLOB 'onMessageInvalidate [0-9]*'", sql)
                self.assertIn("BufferTX*SurfaceView*app/*", sql)
                return SimpleNamespace(stdout='"presented_at"\n1010\n1020\n')
            self.assertIn("frame.display_frame_token", sql)
            self.assertIn("p.name = 'app'", sql)
            return SimpleNamespace(stdout='"presented_at"\n1030\n')

        with patch.object(bench_perfetto.viewport, "present_rows", return_value=[]), patch("subprocess.run", side_effect=process):
            times = bench_perfetto.present_times("shell", "trace", "app")
        self.assertEqual(times, {"canvas": [1010, 1020], "window": [1030]})

    def test_frames_and_memory_are_sliced_to_the_pass(self):
        times = {"canvas": [1000, 1016, 5000], "window": [1010, 5010]}
        self.assertEqual(bench_perfetto.frames(times, 1000, 1500),
                         [{"t_ns": 1000, "surface": "canvas"}, {"t_ns": 1010, "surface": "window"}, {"t_ns": 1016, "surface": "canvas"}])
        with patch("subprocess.run", side_effect=processor):
            rows = bench_perfetto.memory_rows("shell", "trace", "app")
        self.assertEqual(bench_perfetto.memory(rows, 900, 5500, thermal=1), [
            {"t_ns": 900, "thermal": 1},
            {"t_ns": 1000, "os_available_kib": 2097152, "app_rss_kib": 512000},
            {"t_ns": 1005, "swap_free_kib": 1024, "gpu_kib": 10}])

    def test_pass_window_spans_steps_and_latency_tail(self):
        record = {"steps": [{"begin_ns": 3000, "end_ns": 4000}, {"begin_ns": None, "status": "skipped"},
                            {"begin_ns": 4100, "end_ns": 4200, "settled_ns": 4500}]}
        self.assertEqual(bench_perfetto.pass_window(record), (3000, 4500 + bench_perfetto.LATENCY_TAIL_NS))

    def test_convert_writes_each_pass(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        device = Path(directory.name)
        (device / "session.json").write_text(json.dumps({
            "versions": {"v1": {"app_id": "app"}},
            "passes": [{"version": "v1", "pass": 1, "cold": True, "thermal_before": 0}, {"version": "v1", "pass": 2, "cold": False}]}))
        for n, steps in [(1, [{"begin_ns": 1000, "end_ns": 1016}]), (2, [{"begin_ns": 2_000_005_000, "end_ns": 2_000_006_000}])]:
            (device / "v1" / f"pass-{n}").mkdir(parents=True)
            (device / "v1" / f"pass-{n}" / "pass.json").write_text(json.dumps({"steps": steps}))
        with patch("subprocess.run", side_effect=processor) as query:
            bench_perfetto.convert(device, "shell")
        self.assertEqual(query.call_count, 3)
        read = lambda n, name: [json.loads(line) for line in (device / "v1" / f"pass-{n}" / name).read_text().splitlines()]
        self.assertEqual([row["t_ns"] for row in read(1, "frames.jsonl")], [1000, 1010, 1016, 5000, 5010])
        self.assertEqual(read(2, "frames.jsonl"), [])
        self.assertEqual(read(1, "memory.jsonl")[0], {"t_ns": 1000, "thermal": 0})
        self.assertNotIn("thermal", read(2, "memory.jsonl")[0] if read(2, "memory.jsonl") else {})


if __name__ == "__main__":
    unittest.main()
