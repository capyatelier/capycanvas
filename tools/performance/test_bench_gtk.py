import contextlib
import io
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bench_analyze
import bench_gtk

STEPS = [{"id": "launch", "do": "launch"}, {"id": "open.photo", "do": "open", "document": "photo"},
         {"id": "draw.pencil", "do": "stroke"}, {"id": "nav.pan", "do": "gesture"},
         {"id": "open.heavy", "do": "open", "document": "heavy"}, {"id": "save", "do": "save", "document": "heavy"},
         {"id": "close", "do": "close"}]


def step(identity, begin, end, **fields):
    return {"id": identity, "status": "ok", "reason": None, "input": "real", "begin_ns": begin, "end_ns": end, "surface": "canvas",
            "motion": [], "contacts": [], "completions": [], "settled_ns": None, "latency": [], "extra": {}, **fields}


def journey():
    ms = 1_000_000
    stroke = step("draw.pencil", 1000 * ms, 3000 * ms, contacts=[{"down_ns": 1100 * ms, "up_ns": 2100 * ms}],
                  motion=[{"begin_ns": 1100 * ms, "end_ns": 2100 * ms}], settled_ns=2200 * ms,
                  extra={"samples_ns": [1100 * ms + 4 * i * ms for i in range(251)]})
    canvas = [[1100 * ms + 8 * i * ms + 5 * ms, 1100 * ms + 8 * i * ms + 2 * ms] for i in range(125)]
    canvas.insert(3, [canvas[2][0] + ms, canvas[2][1]])
    return {"pid": 42, "refresh_hz": 120.0, "canvas": [9504, 6336], "scale": 1.0, "event_ms": 4,
            "steps": [step("launch", 0, 900 * ms, latency=[{"name": "launch", "input_ns": 0, "ready_ns": 800 * ms, "settled_ns": 850 * ms, "surface": "canvas"}]),
                      stroke, step("close", 3100 * ms, 3200 * ms, surface="window")],
            "canvas_frames": canvas, "window_frames": [3150 * ms], "error": None}


class StepMapping(unittest.TestCase):
    def test_every_chain_step_kind_has_a_journey_handler(self):
        source = (bench_gtk.ROOT / bench_gtk.JOURNEY).read_text()
        handled = set(re.findall(r'^\s*"([a-z]+)" => self\.', source.split("fn perform_step")[1].split("fn canvas_frames")[0], re.M))
        kinds = {step["do"] for step in json.loads(bench_gtk.CHAIN.read_text())["steps"]}
        self.assertEqual(kinds - handled, set())

    def test_overlay_copies_the_journey_and_applies_the_version_patch_once(self):
        with tempfile.TemporaryDirectory() as directory:
            tree, patches = Path(directory) / "tree", Path(directory) / "patches"
            tests = tree / bench_gtk.TESTS
            tests.parent.mkdir(parents=True)
            tests.write_text("mod other;\n")
            (tree / "lib.rs").write_text("fn run() {}\n")
            subprocess.run(["git", "init", "-q"], cwd=tree, check=True)
            patches.mkdir()
            (patches / "v9-gtk.patch").write_text("Subject: hook\n\n--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1,2 @@\n+fn start_process() {}\n fn run() {}\n")
            with patch.object(bench_gtk, "PATCHES", patches):
                for label in ("v9", "v9", "gabc"):
                    bench_gtk.overlay(tree, label)
            self.assertEqual((tree / "lib.rs").read_text(), "fn start_process() {}\nfn run() {}\n")
            self.assertEqual(tests.read_text(), "mod other;\n" + bench_gtk.REGISTRATION)
            self.assertEqual((tree / bench_gtk.JOURNEY).read_bytes(), (bench_gtk.ROOT / bench_gtk.JOURNEY).read_bytes())

    def test_v1_0_15_patch_adds_the_start_process_hook_the_journey_calls(self):
        self.assertIn("crate::start_process();", (bench_gtk.ROOT / bench_gtk.JOURNEY).read_text())
        self.assertIn("+fn start_process() {", (bench_gtk.PATCHES / "v1.0.15-gtk.patch").read_text())

    def test_cargo_messages_name_the_library_test_executable(self):
        messages = "\n".join(json.dumps(m) for m in [
            {"reason": "compiler-artifact", "target": {"name": "layer_linux", "kind": ["lib"]}, "profile": {"test": False}, "executable": None},
            {"reason": "compiler-artifact", "target": {"name": "photo_reader", "kind": ["test"]}, "profile": {"test": True}, "executable": "/t/photo"},
            {"reason": "compiler-artifact", "target": {"name": "layer_linux", "kind": ["lib"]}, "profile": {"test": True}, "executable": "/t/lib"},
            {"reason": "build-finished", "success": True}])
        self.assertEqual(bench_gtk.test_executable("Compiling\n" + messages), "/t/lib")
        with self.assertRaises(SystemExit):
            bench_gtk.test_executable(messages.splitlines()[0])

    def test_environment_is_private_and_names_steps_and_documents(self):
        build = {"executable": "/b/layer_linux-1"}
        inputs = {"chain": Path("/c/chain.json"), "documents": {"photo": Path("/d/photo.jpg"), "heavy": Path("/d/heavy.capy")}}
        user = {"CAPY_STORAGE_DIR": "/home/me/real", "WAYLAND_DISPLAY": "wayland-0", "XDG_CONFIG_HOME": "/home/me/.config",
                "LAYER_NATIVE_EVENT_MS": "60", "PATH": "/usr/bin"}
        with patch.dict(bench_gtk.os.environ, user, clear=True):
            env = bench_gtk.pass_environment(build, {}, Path("/p"), Path("/s"), Path("/s/out"), inputs, ["launch", "open.photo"], "1600x1000", 1, 600)
        self.assertNotIn("CAPY_STORAGE_DIR", env)
        self.assertNotIn("WAYLAND_DISPLAY", env)
        self.assertEqual(env["PATH"], "/usr/bin")
        self.assertEqual((env["XDG_CONFIG_HOME"], env["XDG_CACHE_HOME"], env["CAPY_BENCH_SHADERS"]), ("/p/config", "/p/cache", "/p/shaders"))
        self.assertEqual((env["CAPY_BENCH_PHOTO"], env["CAPY_BENCH_HEAVY"], env["CAPY_BENCH_STEPS"]), ("/d/photo.jpg", "/d/heavy.capy", "launch,open.photo"))
        self.assertEqual((env["LAYER_NATIVE_EVENT_MS"], env["LAYER_NATIVE_TEST_EXECUTABLE"], env["TMPDIR"]), ("4", "/b/layer_linux-1", "/s"))
        self.assertEqual((env["XDG_RUNTIME_DIR"], env["GVFS_DISABLE_FUSE"]), ("/s/run", "1"))
        self.assertEqual((env["LAYER_PACING_GPU_TIMESTAMPS"], env["LAYER_NATIVE_TEST_TIMEOUT_MS"]), ("0", "600000"))
        with patch.dict(bench_gtk.os.environ, {}, clear=True):
            self.assertNotIn("CAPY_BENCH_STEPS", bench_gtk.pass_environment(build, {}, Path("/p"), Path("/s"), Path("/o"), inputs, None, "1600x1000", 1, 60))


class Cleanup(unittest.TestCase):
    def test_scratch_waits_for_every_process_of_the_pass_session(self):
        process = subprocess.Popen(["sh", "-c", "sleep 0.3 & exit 0"], start_new_session=True)
        process.wait()
        self.assertFalse(bench_gtk.wait_for_group(process.pid, seconds=0.05))
        self.assertTrue(bench_gtk.wait_for_group(process.pid, seconds=5))


class OutputSchema(unittest.TestCase):
    info = {"schema": 1, "host": "gtk", "device": "gtk", "version": "v1.0.15", "commit": "abc", "build": "sha", "app_id": bench_gtk.APP_ID,
            "pass": 1, "cold": True, "clock": "monotonic", "tier": "top", "exit_code": 0, "error": None}

    def test_completions_count_only_frames_that_consumed_a_newer_sample(self):
        record, _ = bench_gtk.normalize(journey(), self.info, [])
        stroke = record["steps"][1]
        inputs = [row["input_ns"] for row in stroke["completions"]]
        self.assertEqual(len(inputs), 125)
        self.assertEqual(inputs, sorted(set(inputs)))
        self.assertTrue(all(row["complete_ns"] > row["input_ns"] for row in stroke["completions"]))
        self.assertEqual(stroke["extra"], {"pen_samples": 251})

    def test_pass_record_frames_and_memory_end_follow_the_shared_schema(self):
        memory = [{"t_ns": 3000_000_000 + i * 50_000_000, "os_available_kib": 100 - i, "app_rss_kib": 10 + i, "gpu_kib": 5} for i in range(5)]
        record, frames = bench_gtk.normalize(journey(), self.info, memory)
        self.assertEqual((record["frames_evidence"], record["host"], record["refresh_hz"], record["canvas"]), ("presented", "gtk", 120.0, [9504, 6336]))
        self.assertEqual(record["memory_end"], {"os_available_kib": 98, "app_rss_kib": 12, "gpu_kib": 5})
        self.assertEqual({row["surface"] for row in frames}, {"canvas", "window"})
        self.assertEqual([row["t_ns"] for row in frames], sorted(row["t_ns"] for row in frames))
        for entry in record["steps"]:
            self.assertLessEqual({"id", "status", "begin_ns", "end_ns", "surface", "motion", "contacts", "completions", "latency"}, entry.keys())

    def test_analyzer_reads_normalized_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            for version in ("v1.0.15", "gabc"):
                record, frames = bench_gtk.normalize(journey(), dict(self.info, version=version), [])
                folder = run / "gtk" / version / "pass-1"
                folder.mkdir(parents=True)
                (folder / "pass.json").write_text(json.dumps(record))
                bench_gtk.write_jsonl(folder / "frames.jsonl", frames)
                bench_gtk.write_jsonl(folder / "memory.jsonl", [])
            (run / "run.json").write_text(json.dumps({"versions": ["v1.0.15", "gabc"], "devices": {"gtk": {}}}))
            summary = bench_analyze.analyze_run(run)
        device = summary["devices"]["gtk"]
        self.assertEqual((device["tier"], device["target_hz"]), ("top", 120))
        metrics = device["steps"]["draw.pencil"]["metrics"]
        self.assertGreater(metrics["fps"]["versions"]["v1.0.15"]["value"], 100)
        self.assertGreater(metrics["updates_per_s"]["versions"]["gabc"]["value"], 100)
        self.assertEqual(device["steps"]["launch"]["metrics"]["cold_latency_ms"]["versions"]["v1.0.15"]["evidence"], "presented")

    def test_memory_comes_from_proc_and_nvidia_smi(self):
        self.assertEqual(bench_gtk.meminfo_kib("Name:\tx\nVmRSS:\t  123456 kB\n", "VmRSS"), 123456)
        self.assertIsNone(bench_gtk.meminfo_kib("Name:\tx\n", "VmRSS"))
        xml = ("<nvidia_smi_log><gpu><processes><process_info><pid>7</pid><type>G</type><used_memory>18 MiB</used_memory></process_info>"
               "<process_info><pid>42</pid><type>C+G</type><used_memory>1522 MiB</used_memory></process_info></processes></gpu></nvidia_smi_log>")
        self.assertEqual(bench_gtk.gpu_kib(xml, 42), 1522 * 1024)
        self.assertIsNone(bench_gtk.gpu_kib(xml, 9))


class PassOrder(unittest.TestCase):
    def run_order(self, *argv):
        builds = {version: {"label": version, "commit": version * 4, "sha256": "s", "executable": "/e"} for version in ("A", "B")}
        order = []
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(bench_gtk, "RUNS", Path(directory)), \
                patch.object(bench_gtk, "build", lambda version: builds[version]), \
                patch.object(bench_gtk, "documents", lambda chain, steps, spec: {}), \
                patch.object(bench_gtk, "run_pass", lambda build, entry, *_: order.append((build["label"], entry["pass"], entry["cold"])) or
                             dict(entry, exit_code=0, error=None, failed_steps=[], seconds=1)), \
                patch.object(bench_gtk, "analyze_run", lambda run_dir: {}), patch.object(bench_gtk, "format_table", lambda summary: ""), \
                patch.object(bench_gtk, "gpu_name", lambda: None), contextlib.redirect_stdout(io.StringIO()):
            code = bench_gtk.run(bench_gtk.parse(["--versions", "A,B", "--run-id", "r", *argv]))
            record = json.loads((Path(directory) / "r/run.json").read_text())
        return code, order, record

    def test_two_versions_run_a_b_b_a_with_cold_first_passes(self):
        code, order, record = self.run_order()
        self.assertEqual(code, 0)
        self.assertEqual(order, [("A", 1, True), ("B", 1, True), ("B", 2, False), ("A", 2, False)])
        self.assertEqual((record["versions"], record["devices"]["gtk"]["tier"]), (["A", "B"], "top"))

    def test_quick_runs_one_warm_pass_per_version_with_the_step_closure(self):
        _, order, record = self.run_order("--quick", "--steps", "nav.pan")
        self.assertEqual(order, [("A", 1, False), ("B", 1, False)])
        self.assertEqual(record["devices"]["gtk"]["steps"], ["launch", "open.photo", "nav.pan"])


if __name__ == "__main__":
    unittest.main()
