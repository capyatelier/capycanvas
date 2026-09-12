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
    def test_admission_backpressure_remains_visible_and_separate_from_cpu_work(self):
        events = [record(0, 1, 2, 0, 1), record(11, 10, 2, 1),
                  record(0, 11, 12, 0, 2), record(0, 13, 14, 0, 3),
                  record(0, 15, 16, 0), record(0, 17, 18, 1), record(11, 20, 3, 1)]
        result = analyze(header(workload={"name": "ink", "measurement_seconds": .00000001}), events)
        self.assertEqual(result["counts"]["ticks_denied_admission"], 4)
        self.assertEqual(result["counts"]["ticks_denied_by_reason"],
                         {"inactive": 1, "owner_pending": 1, "drawable_capacity": 1, "unclassified": 1})
        self.assertEqual(result["workload"]["ticks_denied_by_reason"],
                         {"inactive": 0, "owner_pending": 1, "drawable_capacity": 1, "unclassified": 1})
        self.assertEqual(result["workload"]["frames"]["owner_service_ms"]["count"], 0)

    def test_90hz_evaluation_retains_real_misses_and_120hz_diagnostics(self):
        events = [record(6, 0, 2400, 1740, 2000, 90), record(10, 0, 1),
                  record(11, 1, 2, 1)]
        for time in [1_000_000, 12_111_111, 34_333_333]:
            cpu = frame(time)
            cpu[4] = cpu[3] + 9_000_000
            events += [cpu, record(4, time, time + 10_000_000)]
        events.append(record(11, 100_000_001, 3, 1))
        source = header(workload={"name": "ink", "measurement_seconds": .1})
        result = analyze(source, events, target_hz=90)
        self.assertEqual(result["evaluation"]["target_hz"], 90)
        self.assertAlmostEqual(result["evaluation"]["frame_budget_ms"], 1000 / 90)
        self.assertEqual(result["presentation"]["continuous_intervals_over_target_budget"], 1)
        self.assertEqual(result["presentation"]["continuous_intervals_over_120hz_budget"], 2)
        self.assertEqual(result["all_submitted_frames"]["owner_service_over_target_budget"], 0)
        self.assertEqual(result["all_submitted_frames"]["owner_service_over_8_33ms"], 3)
        measured = result["workload"]
        self.assertEqual(measured["continuous_active_intervals_ms"]["count"], 2)
        self.assertEqual(measured["continuous_intervals_over_target_budget"], 1)
        self.assertEqual(measured["frames"]["owner_service_over_target_budget"], 0)
        self.assertFalse(any("advertises" in warning for warning in result["warnings"]))
        original = analyze(source, events)
        self.assertEqual(original["evaluation"]["target_hz"], 120)
        self.assertEqual(original["presentation"]["continuous_intervals_over_target_budget"], 2)
        self.assertTrue(any("advertises 120 Hz" in warning for warning in original["warnings"]))

    def test_invalid_refresh_targets_are_rejected(self):
        for target in [0, -90, float("nan"), float("inf"), 1001]:
            with self.subTest(target=target), self.assertRaisesRegex(ValueError, "Target refresh"):
                analyze(header(), [], target_hz=target)

    def test_disabled_gpu_instrumentation_does_not_claim_gpu_measurements(self):
        result = analyze(header(gpu_timing_requested=False), [frame(100)])
        self.assertFalse(result["gpu_timing_requested"])
        self.assertEqual(result["gpu_queue_span_ms"]["count"], 0)
        self.assertIsNone(result["gpu_queue_span_ms"]["p50"])
        self.assertTrue(any("intentionally disabled" in warning for warning in result["warnings"]))
        conflicting = analyze(header(gpu_timing_requested=False), [record(7, 100, 1000, 1)])
        self.assertTrue(any("contradict" in warning for warning in conflicting["warnings"]))

    def test_commit_deadline_is_distinct_from_presentation_target(self):
        events = [record(6, 1, 2400, 1800, 2000, 120, 2),
                  frame(100_000_000), record(12, 100_000_000, 101_000_000, 108_000_000, 1),
                  record(4, 100_000_000, 108_000_000),
                  frame(200_000_000), record(12, 200_000_000, 203_000_000, 208_000_000, 2)]
        result = analyze(header(), events)
        self.assertEqual(result["display_configurations"][0]["metal_preferred_frame_latency"], 2)
        schedule = result["display_scheduling"]
        self.assertEqual(schedule["supplied_drawables_accepted"], 1)
        self.assertEqual(schedule["stale_drawables_rejected"], 1)
        self.assertEqual(schedule["owners_completing_after_commit_deadline"], 1)
        self.assertEqual(schedule["owner_completion_after_commit_deadline_ms"]["max"], 1)
        self.assertEqual(schedule["presentation_target_after_commit_deadline_ms"]["max"], 7)
        self.assertEqual(result["presentation"]["positive_target_lateness_ms"]["max"], 0)
        self.assertEqual(result["presentation"]["frame_admission_to_present_ms"]["max"], 8)

    def test_workload_interval_excludes_setup_and_reports_synthetic_source(self):
        events = [record(9, 1, 1, 11), frame(10),
                  record(11, 100_000_000, 2, 1, 200, 100),
                  frame(110_000_000), record(7, 110_000_000, 4_000_000, 1),
                  record(4, 110_000_000, 120_000_000),
                  frame(160_000_000), record(4, 160_000_000, 170_000_000),
                  record(11, 200_000_000, 3, 1, 240, 120),
                  frame(210_000_000), record(11, 220_000_000, 4, 1)]
        result = analyze(header(input_source="synthetic", workload={"name": "ink", "measurement_seconds": .1}), events)
        self.assertEqual(result["input_source"], "synthetic")
        workload = result["workload"]
        self.assertTrue(workload["measurement_completed"])
        self.assertTrue(workload["postlude_observed"])
        self.assertEqual(workload["frames"]["owner_service_ms"]["count"], 2)
        self.assertEqual(workload["gpu_samples_missing"], 1)
        self.assertEqual(workload["gpu_queue_span_ms"]["max"], 4)
        self.assertEqual(workload["delivered_nonpredicted_samples"], 40)
        self.assertEqual(workload["presentation_intervals_including_pen_up_ms"]["max"], 50)
        self.assertEqual(workload["first_presentation_after_start_ms"], 20)
        self.assertEqual(workload["last_presentation_before_end_ms"], 30)
        self.assertEqual(workload["frame_admission_to_present_ms"]["max"], 10)

    def test_completed_producer_does_not_conceal_missing_render_observations(self):
        no_viewport = frame(300_000_000)
        no_viewport[7] = 0
        events = [record(11, 100_000_000, 2, 1), frame(110_000_000),
                  record(4, 110_000_000, 120_000_000), no_viewport,
                  frame(400_000_000), record(3, 400_000_000, 0, 0, 1, 1),
                  frame(500_000_000), record(3, 500_000_000, 0, 0, 2, 1),
                  record(4, 500_000_000, 0, 0, 2),
                  record(0, 600_000_000, 0, 0), record(11, 700_000_000, 3, 1)]
        result = analyze(header(workload={"name": "ink", "measurement_seconds": .6}), events)["workload"]
        self.assertTrue(result["measurement_completed"])
        self.assertEqual(result["actual_presentations"], 1)
        self.assertEqual(result["last_presentation_before_end_ms"], 580)
        self.assertEqual(result["admitted_frames_without_viewport"], 1)
        self.assertEqual(result["missing_presentation_callbacks"], 1)
        self.assertEqual(result["zero_time_presentations"], 1)
        self.assertEqual(result["ticks_denied_admission"], 1)

    def test_aborted_or_truncated_workload_is_not_a_completed_measurement(self):
        source = header(input_source="synthetic", workload={"name": "ink", "measurement_seconds": 600})
        for events in [[], [record(11, 100, 2, 1)],
                       [record(11, 100, 2, 1), record(11, 200, 3, 1), record(11, 210, 5, 1)]]:
            result = analyze(source, events)
            self.assertFalse(result["workload"]["measurement_completed"])
            self.assertTrue(any("no complete measurement" in warning for warning in result["warnings"]))

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

    def test_corrections_have_separate_receipt_proxies_and_predictions_are_excluded(self):
        events = []
        for kind in (0, 1, 2):
            time = (kind + 1) * 1_000_000
            events += [record(2, time, time + 10, time + 20, 90, 95, 1, kind, 1, 0, 1),
                       frame(time + 100, receipt=time), record(4, time + 100, time + 1_000_000)]
        result = analyze(header(), events)
        self.assertEqual(result["counts"]["correction_input_batches"], 1)
        self.assertEqual(result["presentation"]["first_associated_present_per_owner_receipt_proxy_ms"]["count"], 1)
        self.assertEqual(result["presentation"]["first_associated_present_per_correction_receipt_proxy_ms"]["count"], 1)
        self.assertEqual(result["correction_owner_queue_ms"]["count"], 1)


if __name__ == "__main__":
    unittest.main()
