"""Timing-boundary checks: python3 -m unittest discover -s tools/performance."""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "navigation_report", pathlib.Path(__file__).with_name("photo-navigation-report.py")
)
navigation_report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(navigation_report)


class NavigationReportTests(unittest.TestCase):
    def report(self):
        return {
            "extent": [100, 100], "viewport": [100, 100],
            "space": "ProPhoto", "depth": 16, "gtk_renderer": "test",
            "requests": [
                {"requested_ns": 100_000_000, "matrix": [1], "phase": "start", "lateness_ms": 0},
                {"requested_ns": 108_000_000, "matrix": [2], "phase": "move", "lateness_ms": 0},
                {"requested_ns": 116_000_000, "matrix": [3], "phase": "move", "lateness_ms": 0},
            ],
            "worker_cpu": [
                [10, 0, 0, 1, 110_000_000], [11, 0, 0, 1, 118_000_000],
            ],
            "camera_views": [[10, [2], 0], [11, [3], 0]],
            "canvas_presentation": [
                [8, 0, 8_000_000, 0], [9, 60_000_000, 8_000_000, 1],
                [10, 112_000_000, 8_000_000, 1], [11, 120_000_000, 8_000_000, 1],
            ],
            "worker_gpu": [[10, 1], [11, 1]], "frame_handler_cpu": [0.1, 0.1],
        }

    def test_startup_feedback_excluded_but_first_response_and_lost_input_remain(self):
        result = navigation_report.summarize(self.report())
        self.assertEqual(result["feedback_outside_measured_frames"], 2)
        self.assertEqual(result["discarded_feedback"], 0)
        self.assertEqual(result["missed_refresh_slots"], 0)
        self.assertEqual(result["first_request_to_first_present_ms"], 12)
        self.assertEqual(result["requests_without_matching_presentation_by_phase"], {"start": 1})
        self.assertEqual(result["request_to_enqueue"]["p99_ms"], 2)
        self.assertEqual(result["enqueue_to_present"]["p99_ms"], 2)

    def test_measured_discards_and_refresh_gaps_are_retained(self):
        report = self.report()
        report["worker_cpu"].append([12, 0, 0, 1, 120_000_000])
        report["canvas_presentation"].extend([
            [12, 0, 8_000_000, 0], [13, 130_000_000, 8_000_000, 1],
        ])
        report["canvas_presentation"][3][1] = 128_000_000
        result = navigation_report.summarize(report)
        self.assertEqual(result["discarded_feedback"], 1)
        self.assertEqual(result["missed_refresh_slots"], 1)

    def test_repeated_pose_does_not_match_a_future_request(self):
        report = self.report()
        report["requests"][2]["matrix"] = [2]
        report["camera_views"][1][1] = [2]
        result = navigation_report.summarize(report)
        self.assertEqual(result["presented_requests"], 2)
        self.assertEqual(result["request_to_present"]["p99_ms"], 4)


if __name__ == "__main__":
    unittest.main()
