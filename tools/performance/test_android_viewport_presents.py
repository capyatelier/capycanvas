import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("android_viewport_presents", Path(__file__).with_name("android-viewport-presents.py"))
viewport = importlib.util.module_from_spec(spec)
spec.loader.exec_module(viewport)


class WorkspaceWindowTest(unittest.TestCase):
    def run_data(self):
        return dict(begin_ns=1_000_000_000, end_ns=7_000_000_000,
                    begin_boot_ns=10_000_000_050, end_boot_ns=16_000_000_050,
                    begin_mono_after_boot_ns=1_000_000_100, end_mono_after_boot_ns=7_000_000_100,
                    trace_name="workspace-contact", run=0, thermal_before=0, thermal_after=0,
                    frame_rate=60, vsync_interval_p99_ms=16.667)

    def shown(self, begin=10_000_000_050):
        return {1: begin + 50_000_000, 2: begin + 200_000_000, 3: begin + 300_000_000,
                4: begin + 500_000_000, 5: begin + 500_000_000, 6: begin + 5_950_000_000}

    def test_normal_workspace_uses_explicit_boot_window_without_app_trace(self):
        with patch.object(viewport.subprocess, "run", side_effect=AssertionError("No async trace query expected")):
            result = viewport.workspace_summary("unused", "unused", self.run_data(), self.shown())
        self.assertEqual(result["window_source"], "explicit boot timestamps")
        self.assertEqual(result["full_contact_presents"], 5)
        self.assertEqual(result["presented_gap_p99_ms"], 200)
        self.assertEqual(len(result["actual_present_timestamps_ns"]), 3)
        self.assertEqual(result["onset_ms"], 50)

    def test_profile_keeps_async_window(self):
        output = SimpleNamespace(stdout='"ts","dur"\n10000000050,6000000000\n')
        with patch.object(viewport.subprocess, "run", return_value=output) as query:
            result = viewport.workspace_summary("processor", "trace", self.run_data(), self.shown(), profile=True)
        self.assertEqual(result["window_source"], "profile async trace")
        query.assert_called_once()

    def test_legacy_report_keeps_async_window(self):
        run = self.run_data()
        for key in ["begin_boot_ns", "end_boot_ns", "begin_mono_after_boot_ns", "end_mono_after_boot_ns"]:
            del run[key]
        output = SimpleNamespace(stdout='"ts","dur"\n10000000050,6000000000\n')
        with patch.object(viewport.subprocess, "run", return_value=output):
            result = viewport.workspace_summary("processor", "trace", run, self.shown())
        self.assertEqual(result["window_source"], "legacy async trace")

    def test_rejects_offset_change_outside_read_brackets(self):
        run = self.run_data()
        run["end_boot_ns"] += 1_000_000
        with self.assertRaisesRegex(ValueError, "offset changed"):
            viewport.workspace_boot_window(run)

    def test_rejects_partial_or_noninteger_clock_samples(self):
        for key, value in [("end_boot_ns", None), ("begin_ns", True), ("end_ns", 7e9)]:
            run = self.run_data()
            run[key] = value
            with self.assertRaises(ValueError):
                viewport.workspace_boot_window(run)

    def test_rejects_reversed_clock_bounds(self):
        run = self.run_data()
        run["end_ns"] = run["begin_ns"]
        with self.assertRaisesRegex(ValueError, "ordered"):
            viewport.workspace_boot_window(run)

    def test_normal_workspace_never_enables_app_atrace(self):
        self.assertNotIn("atrace_apps", viewport.trace_config("owned.app", workspace=True))
        self.assertIn('atrace_apps: "owned.app"', viewport.trace_config("owned.app", workspace=True, profile=True))

    def test_navigation_reports_full_contact_and_moving_canvas_presents(self):
        run = self.run_data()
        run.update(completions=[[1, 1_100_000_000], [2, 1_200_000_000]],
                   presentation=[[[1, 2_000_000, 1]]])
        result = viewport.summarize(run, self.shown())
        self.assertEqual(result["full_contact_presents"], 5)
        self.assertEqual(result["onset_ms"], 50)
        self.assertEqual(result["full_contact_longest_gap_ms"], 5450)
        self.assertEqual(len(result["actual_present_timestamps_ns"]), 3)
        self.assertEqual(result["viewport_gpu_p50_ms"], 2)
        self.assertEqual(result["submitted_per_s"], 2 / 6)

    def test_legacy_navigation_keeps_completion_attribution(self):
        run = self.run_data()
        del run["begin_mono_after_boot_ns"]
        del run["end_mono_after_boot_ns"]
        run.update(completions=[[1, 1_100_000_000], [2, 1_200_000_000]], presentation=[])
        result = viewport.summarize(run, self.shown())
        self.assertNotIn("window_source", result)
        self.assertAlmostEqual(result["presented_per_s"], 1e9 / 150_000_000)


if __name__ == "__main__":
    unittest.main()
