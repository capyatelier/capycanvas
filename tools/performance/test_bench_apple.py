import contextlib
import io
import json
import plistlib
import signal
from unittest.mock import Mock, patch
import tempfile
import unittest
from pathlib import Path

import bench_apple


CHAIN = {"schema": 1, "motion_frames": 150, "steps": [
    {"id": "launch", "do": "launch"}, {"id": "open.photo", "do": "open", "document": "photo"},
    {"id": "draw.pencil", "do": "stroke"}, {"id": "undo", "do": "repeat"},
    {"id": "open.heavy", "do": "open", "document": "heavy"}, {"id": "heavy.draw", "do": "stroke"}]}


class LaunchChecks(unittest.TestCase):
    documents = {"photo": "photo.jpg", "heavy": "heavy.capy"}

    def test_cold_pass_names_no_root_and_warm_pass_reuses_it(self):
        cold = bench_apple.pass_environment("out/p1", 1, True, "bench-main", "macmini", "main", None, None, self.documents)
        self.assertEqual(cold["CAPY_WORKLOAD_COLD"], "1")
        self.assertNotIn("CAPY_WORKLOAD_ROOT", cold)
        self.assertNotIn("CAPY_WORKLOAD_STEPS", cold)
        self.assertEqual(cold["CAPY_WORKLOAD_PHOTO"], "Documents/capy-bench/photo.jpg")
        self.assertEqual(cold["CAPY_WORKLOAD_HEAVY"], "Documents/capy-bench/heavy.capy")
        self.assertEqual(cold["CAPY_TRACE_GPU"], "0")
        warm = bench_apple.pass_environment("out/p2", 2, False, "ROOT", "macmini", "main", "abc", ["launch", "undo"], self.documents)
        self.assertEqual((warm["CAPY_WORKLOAD_COLD"], warm["CAPY_WORKLOAD_ROOT"], warm["CAPY_WORKLOAD_PASS"]), ("0", "ROOT", "2"))
        self.assertEqual((warm["CAPY_WORKLOAD_STEPS"], warm["CAPY_WORKLOAD_COMMIT"]), ("launch,undo", "abc"))

    def test_open_starts_a_new_instance_with_each_variable(self):
        command = bench_apple.open_command("/A.app", {"CAPY_WORKLOAD": "chain", "CAPY_WORKLOAD_PASS": "1"})
        self.assertEqual(command, ["open", "-n", "--env", "CAPY_WORKLOAD=chain", "--env", "CAPY_WORKLOAD_PASS=1",
                                   "/A.app", "--args", "-ApplePersistenceIgnoreState", "YES"])

    def test_stage_copies_only_changed_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "chain.json"
            source.write_text("one")
            bench_apple.stage(root / "data", {"chain.json": source})
            staged = root / "data" / bench_apple.STAGE / "chain.json"
            before = staged.stat().st_mtime_ns
            bench_apple.stage(root / "data", {"chain.json": source})
            self.assertEqual(staged.stat().st_mtime_ns, before)
            source.write_text("two")
            bench_apple.stage(root / "data", {"chain.json": source})
            self.assertEqual(staged.read_text(), "two")

    def test_wait_reports_a_missing_pass_with_its_path(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(TimeoutError, "pass.json"):
                bench_apple.wait_for_pass(directory, "out", 0, sleep=lambda _: None)


    def test_wait_confirms_exit_before_the_root_can_be_reused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / "out").mkdir()
            info = {"pid": 1234}; (root / "out/pass.json").write_text(json.dumps(info))
            checks = Mock(side_effect=[True] * 63 + [False]); sleep = Mock()
            with patch("bench_apple.os.kill") as kill:
                self.assertEqual(bench_apple.wait_for_pass(root, "out", 100, process_alive=checks, sleep=sleep), info)
            kill.assert_called_once_with(1234, signal.SIGTERM)
            self.assertEqual(checks.call_count, 64)
            self.assertEqual(sleep.call_count, 63)

    def test_wait_checks_exit_at_the_end_of_the_shutdown_grace(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / "out").mkdir()
            info = {"pid": 1234}; (root / "out/pass.json").write_text(json.dumps(info))
            checks = Mock(side_effect=[True] * 70 + [False]); sleep = Mock()
            with patch("bench_apple.os.kill"):
                self.assertEqual(bench_apple.wait_for_pass(root, "out", 100, process_alive=checks, sleep=sleep), info)
            self.assertEqual(checks.call_count, 71)
            self.assertEqual(sleep.call_count, 70)

    def test_wait_refuses_storage_reuse_when_the_signaled_app_stays_alive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / "out").mkdir()
            (root / "out/pass.json").write_text(json.dumps({"pid": 1234}))
            with patch("bench_apple.os.kill") as kill, self.assertRaisesRegex(TimeoutError, "refusing to reuse"):
                bench_apple.wait_for_pass(root, "out", 100, process_alive=lambda _: True, sleep=lambda _: None)
            kill.assert_called_once_with(1234, signal.SIGTERM)


class RunChecks(unittest.TestCase):
    def test_run_collects_passes_in_order_and_reuses_cold_roots(self):
        self.check_run("main")

    def test_run_collects_git_ref_labels_for_the_shared_analyzer(self):
        self.check_run("origin/main")

    def test_run_reports_error_steps(self):
        self.check_run("main", status="error")

    def check_run(self, main, status="failed"):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            inputs = home / "inputs"
            (inputs / "fixtures").mkdir(parents=True)
            for name in ["fixtures/heavy.capy", "fixtures/vector.capy", "photo.JPG"]:
                (inputs / name).write_bytes(name.encode())
            (inputs / "chain.json").write_text(json.dumps(CHAIN))
            apps = {}
            for label in ["v1", main]:
                app = home / f"{label}.app/Contents"
                app.mkdir(parents=True)
                with open(app / "Info.plist", "wb") as file:
                    plistlib.dump({"CFBundleIdentifier": f"art.capycanvas.bench.{label.replace('/', '_')}"}, file)
                apps[label] = app.parent
            launches = []

            def launch(app, env):
                app = Path(app)
                data = bench_apple.container(home, bench_apple.bundle_id(app))
                output = data / env["CAPY_WORKLOAD_OUTPUT"]
                output.mkdir(parents=True)
                launches.append((app.name, env))
                if "prime" in env["CAPY_WORKLOAD_OUTPUT"]:
                    (output / "pass.json").write_text(json.dumps({"error": "missing chain", "pid": 0}))
                    return
                self.assertTrue((data / bench_apple.STAGE / "photo.jpg").is_file())
                root = env.get("CAPY_WORKLOAD_ROOT", f"cold-{app.stem}")
                (data / "Library/Caches/CapyPerformanceSessions" / root).mkdir(parents=True, exist_ok=True)
                (output / "frames.jsonl").write_text("")
                (output / "pass.json").write_text(json.dumps({"root": root, "pid": 0, "error": None, "steps": [
                    {"id": "undo", "status": status, "reason": "Command undo is disabled"}]}))

            args = bench_apple.parse(["--device", "macmini", "--photo", str(inputs / "photo.JPG"),
                                      "--fixtures", str(inputs / "fixtures"), "--chain", str(inputs / "chain.json"),
                                      "--bundle", f"v1={apps['v1']}", "--bundle", f"{main}={apps[main]}",
                                      "--steps", "undo", "--run-id", "r1", "--commit", f"{main}=bench-chain"])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                report = bench_apple.run(args, launch=launch, home=home, process_alive=lambda _: False,
                                         sleep=lambda _: None, commit=lambda version: f"commit-{version}")
            passes = [(app, env["CAPY_WORKLOAD_PASS"]) for app, env in launches if "prime" not in env["CAPY_WORKLOAD_OUTPUT"]]
            self.assertEqual(passes, [("v1.app", "1"), ("main.app", "1"), ("main.app", "2"), ("v1.app", "2")])
            self.assertEqual(sum("prime" in env["CAPY_WORKLOAD_OUTPUT"] for _, env in launches), 2)
            warm = [env for app, env in launches if env.get("CAPY_WORKLOAD_PASS") == "2" and app == "v1.app"][0]
            self.assertEqual(warm["CAPY_WORKLOAD_ROOT"], "cold-v1")
            self.assertEqual(warm["CAPY_WORKLOAD_STEPS"], "launch,open.photo,undo")
            run_dir = home / "capy-bench/runs/r1"
            self.assertTrue((run_dir / "macmini" / main / "pass-2/frames.jsonl").is_file())
            saved = json.loads((run_dir / "run.json").read_text())
            self.assertEqual(saved["passes"][0]["failed_steps"], ["undo"])
            self.assertEqual([item["cold"] for item in saved["order"]], [True, True, False, False])
            self.assertEqual(report["versions"], ["v1", main])
            self.assertEqual(report["builds"][main]["commit"], "commit-bench-chain")
            main_launch = next(env for app, env in launches if app == "main.app" and "CAPY_WORKLOAD_PASS" in env)
            self.assertEqual(main_launch["CAPY_WORKLOAD_COMMIT"], "commit-bench-chain")
            self.assertEqual(report["builds"][main]["app_id"], f"art.capycanvas.bench.{main.replace('/', '_')}")
            summary = json.loads((run_dir / "summary.json").read_text())
            self.assertEqual(summary["devices"]["macmini"]["steps"]["undo"]["problems"][main]["status"], status)
            self.assertIn("macmini: top tier", output.getvalue())
            self.assertIn("undo", output.getvalue())
            for label in ["v1", main]:
                data = bench_apple.container(home, f"art.capycanvas.bench.{label.replace('/', '_')}")
                self.assertFalse((data / "Library/Caches/CapyPerformanceSessions" / f"cold-{apps[label].stem}").exists())
                self.assertEqual(list((data / bench_apple.STAGE / "out").rglob("pass.json")), [])
                self.assertFalse((data / bench_apple.STAGE / "prime").exists())


if __name__ == "__main__":
    unittest.main()
