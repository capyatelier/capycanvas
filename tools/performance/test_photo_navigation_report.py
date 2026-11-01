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
            "motion_begin_ns": 100_000_000, "motion_end_ns": 128_000_000,
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
        camera = result["instrumented_camera"]
        self.assertEqual(result["presented_frames"], 2)
        self.assertEqual(camera["discarded_feedback"], 0)
        self.assertEqual(result["missed_refresh_slots"], 0)
        self.assertEqual(camera["first_request_to_first_attributed_present_ms"], 12)
        self.assertEqual(camera["requests_without_matching_presentation_by_phase"], {"start": 1})
        self.assertEqual(camera["request_to_enqueue"]["p99_ms"], 2)
        self.assertEqual(camera["enqueue_to_present"]["p99_ms"], 2)

    def test_measured_discards_and_refresh_gaps_are_retained(self):
        report = self.report()
        report["worker_cpu"].append([12, 0, 0, 1, 120_000_000])
        report["canvas_presentation"].extend([
            [12, 0, 8_000_000, 0], [13, 130_000_000, 8_000_000, 1],
        ])
        report["canvas_presentation"][3][1] = 128_000_000
        result = navigation_report.summarize(report)
        self.assertEqual(result["instrumented_camera"]["discarded_feedback"], 1)
        self.assertEqual(result["missed_refresh_slots"], 1)

    def test_repeated_pose_does_not_match_a_future_request(self):
        report = self.report()
        report["requests"][2]["matrix"] = [2]
        report["camera_views"][1][1] = [2]
        result = navigation_report.summarize(report)["instrumented_camera"]
        self.assertEqual(result["presented_requests"], 2)
        self.assertEqual(result["request_to_present"]["p99_ms"], 4)

    def test_idle_repeat_does_not_count_as_another_input_response(self):
        report = self.report()
        report["worker_cpu"].append([12, 0, 0, 1, 126_000_000])
        report["camera_views"].insert(0, [12, [3], 0])
        report["canvas_presentation"].append([12, 128_000_000, 8_000_000, 1])
        result = navigation_report.summarize(report)
        camera = result["instrumented_camera"]
        self.assertEqual(camera["presented_requests"], 2)
        self.assertEqual(camera["request_to_present"]["n"], 2)
        self.assertEqual(camera["request_to_present"]["max_ms"], 4)
        self.assertEqual(result["presentation_cadence"]["n"], 2)

    def test_cached_presentations_count_once_per_timestamp_within_motion(self):
        report = self.report()
        report["canvas_presentation"].extend([
            [0, 96_000_000, 8_000_000, 1],
            [0, 104_000_000, 8_000_000, 1],
            [0, 116_000_000, 8_000_000, 1],
            [0, 116_000_000, 8_000_000, 1],
            [0, 132_000_000, 8_000_000, 1],
        ])
        result = navigation_report.summarize(report)
        self.assertEqual(result["presentation_cadence"]["n"], 3)
        self.assertEqual(result["presentation_cadence"]["max_ms"], 8)
        self.assertEqual(result["presented_frames"], 4)
        self.assertAlmostEqual(result["presented_fps"], 4 / 0.028)
        self.assertEqual(result["instrumented_camera"]["presented_requests"], 2)

    def test_full_contact_cadence_requires_recorded_motion_bounds(self):
        report = self.report()
        del report["motion_end_ns"]
        with self.assertRaises(KeyError):
            navigation_report.summarize(report)


if __name__ == "__main__":
    unittest.main()
