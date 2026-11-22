import csv
import json
import tempfile
import unittest
from pathlib import Path

from bench_analyze import analyze_run, format_table
from bench_results import HEADER, publish

MS = 1_000_000


def step(step_id, **fields):
    return {"id": step_id, "status": "ok", "reason": None, "input": "real", "begin_ns": 0, "end_ns": 60_000 * MS,
            "surface": "canvas", "motion": [], "contacts": [], "completions": [], "latency": [], "extra": {}, **fields}


def motion_pass(intervals_ms, step_id="nav.pan"):
    times = [200 * MS]
    for gap in intervals_ms:
        times.append(times[-1] + round(gap * MS))
    window = {"begin_ns": 0, "end_ns": times[-1] + 200 * MS}
    return {"steps": [step(step_id, motion=[window])], "frames": [{"t_ns": t, "surface": "canvas"} for t in times]}


def latency_pass(step_id, input_ms, ready_ms, cold=False):
    return {"cold": cold, "steps": [step(step_id, latency=[{"name": step_id, "input_ns": input_ms * MS,
                                                            "ready_ns": ready_ms * MS, "surface": "canvas"}])]}


def write_run(root, passes, device="pc"):
    root.mkdir(parents=True, exist_ok=True)
    (root / "run.json").write_text(json.dumps({"versions": list(passes), "devices": {device: {}}, "date": "2026-11-21"}))
    for version, specs in passes.items():
        for number, spec in enumerate(specs, 1):
            directory = root / device / version / f"pass-{number}"
            directory.mkdir(parents=True)
            info = {"schema": 1, "host": "android", "device": device, "version": version, "commit": f"commit-{version}",
                    "app_id": f"app.{version}", "pass": number, "cold": spec.get("cold", False), "clock": "boottime",
                    "refresh_hz": 60.0, "steps": spec["steps"], "memory_end": spec.get("memory_end", {}),
                    **({"frames_evidence": spec["frames_evidence"]} if "frames_evidence" in spec else {})}
            (directory / "pass.json").write_text(json.dumps(info))
            for name in ("frames", "memory"):
                (directory / f"{name}.jsonl").write_text("".join(json.dumps(row) + "\n" for row in spec.get(name, [])))
    return root


class AnalyzeTests(unittest.TestCase):
    def analyze(self, passes, device="pc"):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        return analyze_run(write_run(Path(directory.name), passes, device))

    def metric(self, summary, step_id, name, version="a", device="pc"):
        return summary["devices"][device]["steps"][step_id]["metrics"][name]["versions"][version]

    def test_fps_and_gap_p99_come_from_frame_intervals(self):
        summary = self.analyze({"a": [motion_pass([10] * 98 + [40, 40])]})
        self.assertAlmostEqual(self.metric(summary, "nav.pan", "fps")["value"], 100 / 1.06)
        self.assertEqual(self.metric(summary, "nav.pan", "gap_p99_ms")["value"], 40)

    def test_edges_are_trimmed_and_windows_never_join(self):
        frames = [{"t_ns": t * 10 * MS, "surface": "canvas"} for t in range(301)]
        frames += [{"t_ns": 500 * MS + 5 * MS, "surface": "window"}]
        windows = [{"begin_ns": 0, "end_ns": 1000 * MS}, {"begin_ns": 2000 * MS, "end_ns": 3000 * MS}]
        summary = self.analyze({"a": [{"steps": [step("nav.pan", motion=windows)], "frames": frames}]})
        fps = self.metric(summary, "nav.pan", "fps")
        self.assertEqual(fps["n"], 160)
        self.assertAlmostEqual(fps["value"], 100)
        self.assertEqual(self.metric(summary, "nav.pan", "gap_p99_ms")["value"], 10)

    def test_stroke_updates_gaps_pen_down_and_catch_up(self):
        completions = [{"input_ns": (t - 15) * MS, "complete_ns": t * MS} for t in range(1020, 2001, 10)]
        completions += [{"input_ns": 1990 * MS, "complete_ns": 2500 * MS}, {"input_ns": 2990 * MS, "complete_ns": 3010 * MS}]
        completions += [{"input_ns": (t - 15) * MS, "complete_ns": t * MS} for t in range(3030, 3991, 20)]
        contacts = [{"down_ns": 1000 * MS, "up_ns": 2000 * MS}, {"down_ns": 3000 * MS, "up_ns": 4000 * MS}]
        stroke = step("draw.pencil", contacts=contacts, completions=completions, settled_ns=4500 * MS)
        summary = self.analyze({"a": [{"steps": [stroke]}]})
        self.assertAlmostEqual(self.metric(summary, "draw.pencil", "updates_per_s")["value"], 149 / 2)
        self.assertEqual(self.metric(summary, "draw.pencil", "update_gap_p99_ms")["value"], 20)
        self.assertEqual(self.metric(summary, "draw.pencil", "first_use_pen_down_ms")["value"], 20)
        self.assertEqual(self.metric(summary, "draw.pencil", "pen_down_ms")["value"], 30)
        self.assertEqual(self.metric(summary, "draw.pencil", "catch_up_ms")["value"], 500)

    def test_latency_counts_from_the_shown_result_and_settle_from_the_quiet_renderer(self):
        frames = [{"t_ns": 1080 * MS, "surface": "canvas"}]
        latency = [{"name": "magic_wand", "input_ns": 1000 * MS, "ready_ns": 1050 * MS, "settled_ns": 9000 * MS, "surface": "canvas"}]
        summary = self.analyze({"a": [{"steps": [step("select.wand", latency=latency)], "frames": frames}]})
        self.assertEqual(self.metric(summary, "select.wand", "latency_ms")["value"], 80)
        self.assertEqual(self.metric(summary, "select.wand", "settle_ms")["value"], 8000)

    def test_latency_uses_presented_frame_else_completion(self):
        frames = [{"t_ns": 1060 * MS, "surface": "canvas"}, {"t_ns": 7100 * MS, "surface": "canvas"}]
        records = {"undo": (1000, 1050), "fill": (5000, 5040)}
        steps = [step(step_id, latency=[{"name": step_id, "input_ns": i * MS, "ready_ns": r * MS, "surface": "canvas"}])
                 for step_id, (i, r) in records.items()]
        summary = self.analyze({"a": [{"steps": steps, "frames": frames}, latency_pass("launch", 0, 900, cold=True)]})
        self.assertEqual(self.metric(summary, "undo", "latency_ms")["value"], 60)
        self.assertEqual(self.metric(summary, "undo", "latency_ms")["evidence"], "presented")
        self.assertEqual(self.metric(summary, "fill", "latency_ms")["value"], 40)
        self.assertEqual(self.metric(summary, "fill", "latency_ms")["evidence"], "completed")
        self.assertEqual(self.metric(summary, "launch", "cold_latency_ms")["value"], 900)

    def test_submitted_frames_keep_their_evidence(self):
        frames = [{"t_ns": t * MS, "surface": "canvas"} for t in range(1000, 2001, 10)]
        steps = [step("nav.pan", motion=[{"begin_ns": 1000 * MS, "end_ns": 2000 * MS}])]
        summary = self.analyze({"a": [{"steps": steps, "frames": frames, "frames_evidence": "submitted"}]})
        self.assertEqual(self.metric(summary, "nav.pan", "fps")["evidence"], "submitted")

    def test_latency_keeps_completed_evidence_from_the_runner(self):
        frames = [{"t_ns": 1060 * MS, "surface": "canvas"}]
        latency = [{"name": "fit", "input_ns": 1000 * MS, "ready_ns": 1030 * MS, "surface": "canvas", "evidence": "completed"}]
        summary = self.analyze({"a": [{"steps": [step("nav.fit", latency=latency)], "frames": frames}]})
        self.assertEqual(self.metric(summary, "nav.fit", "latency_ms")["value"], 30)
        self.assertEqual(self.metric(summary, "nav.fit", "latency_ms")["evidence"], "completed")

    def test_memory_keeps_extremes_and_leaves_missing_fields_missing(self):
        memory = [{"t_ns": 1000 * MS, "os_available_kib": 500_000, "app_rss_kib": 100},
                  {"t_ns": 2000 * MS, "os_available_kib": 400_000, "app_rss_kib": 300},
                  {"t_ns": 3000 * MS, "app_rss_kib": 200}, {"t_ns": 90_000 * MS, "os_available_kib": 1}]
        summary = self.analyze({"a": [{"steps": [step("fill")], "memory": memory, "memory_end": {"app_pss_kib": 1234}}]})
        metrics = summary["devices"]["pc"]["steps"]["fill"]["metrics"]
        self.assertEqual(metrics["os_available_min_kib"]["versions"]["a"]["value"], 400_000)
        self.assertEqual(metrics["app_rss_max_kib"]["versions"]["a"]["value"], 300)
        self.assertNotIn("gpu_max_kib", metrics)
        self.assertEqual(list(summary["devices"]["pc"]["steps"]["memory_end"]["metrics"]), ["app_pss_kib"])

    def flag(self, base, new):
        passes = {"a": [motion_pass([1000 / fps] * 100) for fps in base],
                  "b": [motion_pass([1000 / fps] * 100) for fps in new]}
        return self.metric(self.analyze(passes), "nav.pan", "fps", "b").get("flag")

    def test_flags_follow_perceptible_step_and_noise(self):
        self.assertEqual(self.flag([100, 100], [90, 90]), "regressed")
        self.assertEqual(self.flag([90, 90], [100, 100]), "improved")
        self.assertEqual(self.flag([100, 80], [80, 80]), "ambiguous")
        self.assertIsNone(self.flag([100, 100], [97, 97]))

    def test_one_shot_steps_with_two_samples_need_twenty_percent_and_200_ms(self):
        def flag(base, new):
            summary = self.analyze({"a": [latency_pass("save", 0, value) for value in base],
                                    "b": [latency_pass("save", 0, value) for value in new]})
            return self.metric(summary, "save", "latency_ms", "b").get("flag")
        self.assertIsNone(flag([1000], [1150]))
        self.assertEqual(flag([1000], [1300]), "regressed")
        self.assertEqual(flag([1000] * 3, [1150] * 3), "regressed")

    def test_skipped_steps_are_unsupported(self):
        skipped = dict(step("ui.panel-drag"), status="skipped", reason="no floating panels")
        passes = {"a": [motion_pass([10] * 50, "ui.panel-drag")], "b": [{"steps": [skipped]}]}
        with tempfile.TemporaryDirectory() as directory:
            run = write_run(Path(directory) / "run", passes)
            self.assertIn("skipped", format_table(analyze_run(run)))
            [path] = publish(run, Path(directory) / "repo")
            statuses = {(row["metric"], row["version"]): row["status"] for row in csv.DictReader(path.read_text().splitlines())}
        self.assertEqual(statuses, {("fps", "a"): "miss", ("fps", "b"): "unsupported",
                                    ("gap_p99_ms", "a"): "met", ("gap_p99_ms", "b"): "unsupported"})


class PublishTests(unittest.TestCase):
    def test_csv_header_rows_and_deterministic_ci(self):
        passes = {version: [motion_pass([10 + (i * offset) % 13 for i in range(100)]) for offset in (3, 5)]
                  for version in ("a", "b")}
        with tempfile.TemporaryDirectory() as directory:
            run = write_run(Path(directory) / "run", passes, "movinkpad14")
            first = publish(run, Path(directory) / "one")
            publish(run, Path(directory) / "one")
            second = publish(run, Path(directory) / "two")
            lines = first[0].read_text().splitlines()
            self.assertEqual(lines[0], ",".join(HEADER))
            self.assertEqual(len(lines), 1 + 2 * 4)
            self.assertEqual(second[0].read_text().splitlines(), lines[:5])
            for row in csv.DictReader(lines):
                self.assertLessEqual(float(row["ci_low"]), float(row["median"]))
                self.assertLessEqual(float(row["median"]), float(row["ci_high"]))
                self.assertEqual((row["tier"], row["status"]), ("top", "miss"))


if __name__ == "__main__":
    unittest.main()
