import unittest
from apple_trace import analyze


def record(kind, *values):
    return [kind, *values, *([0] * (10 - len(values)))]


def frame(time, receipt=0):
    return record(1, time, time + 8_000_000, time + 1_000, time + 2_000_000,
                  500_000, 500_000, 500_000, 200_000, 100_000, receipt)


def header(**changes):
    return dict(schema=1, platform=0, configuration="release", duration_seconds=10,
                started_ns=0, capacity=100, record_stride_bytes=88, dropped_records=0, **changes)


class ReportChecks(unittest.TestCase):
    def test_idle_gap_is_excluded_but_active_missed_frame_is_retained(self):
        times = [1_000_000, 9_000_000, 33_000_000, 1_000_000_000]
        events = [record(10, 0, 1), record(10, 40_000_000, 0), record(10, 999_000_000, 1)]
        for index, time in enumerate(times):
            events += [frame(time), record(3, time, time, time, index, 1), record(4, time, time + 8_000_000, time, index)]
        result = analyze(header(), events)
        intervals = result["presentation"]["continuous_active_intervals_ms"]
        self.assertEqual(intervals["count"], 2)
        self.assertEqual(intervals["max"], 24)
        self.assertEqual(result["presentation"]["continuous_intervals_over_120hz_budget"], 1)
        self.assertGreater(result["presentation"]["all_intervals_including_idle_ms"]["max"], 900)

    def test_missing_skipped_invalid_samples_do_not_become_zero_timings(self):
        events = [frame(1), frame(2), frame(3), record(3, 1, 0, 0, 1, 1),
                  record(3, 2, 0, 0, 2, 1), record(4, 2, 0, 0, 2),
                  record(7, 1, 1_000_000, 1), record(7, 2, 0, 2), record(8, 10, 1, 3, 1, 1, 0)]
        result = analyze(header(), events)
        self.assertEqual(result["counts"]["missing_presentation_callbacks"], 1)
        self.assertEqual(result["counts"]["zero_time_presentations"], 1)
        self.assertEqual(result["counts"]["presented_drawables"], 0)
        self.assertEqual(result["gpu_queue_span_ms"]["count"], 1)
        self.assertEqual(result["gpu_queue_span_ms"]["p50"], 1)
        self.assertIsNone(result["presentation"]["positive_target_lateness_ms"]["p50"])
        self.assertEqual(result["counts"]["frames_without_gpu_sample"], 1)
        self.assertTrue(any("GPU observations" in w for w in result["warnings"]))
        zeros = analyze(header(), [frame(1), record(7, 1, 0, 1)])
        self.assertEqual(zeros["counts"]["gpu_false_zero_samples"], 1)
        self.assertIsNone(zeros["gpu_queue_span_ms"]["p50"], "False zero GPU spans are not measurements")

    def test_receipt_proxy_uses_first_associated_presentation_once(self):
        events = [record(2, 100, 200, 300, 90, 95, 1, 0, 2, 0, 1), frame(400, receipt=100),
                  frame(500, receipt=100), record(4, 400, 1_000_100), record(4, 500, 9_000_100)]
        result = analyze(header(), events)
        metric = result["presentation"]["first_associated_present_per_owner_receipt_proxy_ms"]
        self.assertEqual(metric["count"], 1)
        self.assertEqual(metric["p50"], 1)
        self.assertTrue(any("input-to-pixel" in w for w in result["warnings"]))

    def test_ready_subset_requires_shader_completion(self):
        events = [record(9, 10, 1, 3), frame(20), record(9, 30, 20, 11), frame(40)]
        result = analyze(header(), events)
        self.assertEqual(result["frames_after_readiness"]["owner_service_ms"]["count"], 1)
        self.assertEqual(result["ready_seconds_from_start"], 30 / 1e9)


if __name__ == "__main__":
    unittest.main()
