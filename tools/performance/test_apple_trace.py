import unittest
from apple_trace import analyze, calibrate_gpu, motion_windows, canvas_update_progress, workload_contact_intervals, camera_contacts
from copy import deepcopy
from collections import defaultdict


def record(kind, *values):
    return [kind, *values, *([0] * (10 - len(values)))]


def frame(time, receipt=0):
    return record(1, time, time + 8_000_000, time + 1_000, time + 2_000_000,
                  500_000, 500_000, 500_000, 200_000, 100_000, receipt)


def header(**changes):
    return {**dict(schema=1, platform=0, configuration="release", duration_seconds=10, started_ns=0,
                   capacity=100, record_stride_bytes=88, dropped_records=0,
                   gpu_timing_requested=True, input_source="platform"), **changes}


class CanvasUpdateChecks(unittest.TestCase):
    begin, end = 1_000_000_000, 7_000_000_000

    def events(self, values):
        result = []
        for index, (time, raster, stamp, presented) in enumerate(values):
            result += [frame(time), record(15, time, stamp, raster),
                       record(3, time, time, time, index + 1, 1),
                       record(4, time, presented, presented + 1, index + 1)]
        return result

    def report(self, events, **changes):
        grouped = defaultdict(list)
        for event in events: grouped[event[0]].append(event[1:])
        return canvas_update_progress(header(raster_update_counter=True, **changes), grouped, [[self.begin, self.end]])

    def baseline(self):
        return [(500_000_000, 5, 400_000_000, 503_000_000)]

    def test_viewport_only_and_stale_refinement_do_not_count_as_fresh_input(self):
        viewport = self.report(self.events(self.baseline() + [
            (1_100_000_000, 5, 1_050_000_000, 1_103_000_000)]))
        self.assertEqual(viewport["completed"], 0)
        self.assertEqual(viewport["input_updates_per_s_lower_bound"], 0)
        refinement = self.report(self.events(self.baseline() + [
            (1_100_000_000, 6, 400_000_000, 1_103_000_000)]))
        self.assertEqual(refinement["completed"], 1)
        self.assertEqual(refinement["input_updates_per_s_lower_bound"], 0)

    def test_later_raster_work_can_complete_previously_consumed_input_once(self):
        result = self.report(self.events(self.baseline() + [
            (1_100_000_000, 5, 1_050_000_000, 1_103_000_000),
            (1_200_000_000, 6, 1_050_000_000, 1_203_000_000),
            (1_300_000_000, 7, 1_050_000_000, 1_303_000_000)]))
        self.assertEqual(result["availability"], "observed")
        self.assertEqual(result["completed"], 2)
        self.assertAlmostEqual(result["completed_per_s"], 2 / 6)
        self.assertAlmostEqual(result["input_updates_per_s_lower_bound"], 1 / 6)
        self.assertEqual(result["completion_gap_ms"]["max"], 100)

    def test_unpresented_advance_cannot_be_credited_to_later_input(self):
        values = self.baseline() + [
            (900_000_000, 6, 400_000_000, 903_000_000),
            (1_100_000_000, 6, 1_050_000_000, 1_103_000_000)]
        events = [r for r in self.events(values) if not (r[0] in (3, 4) and r[1] == 900_000_000)]
        result = self.report(events)
        self.assertEqual(result["availability"], "observed")
        self.assertEqual(result["completed"], 0)
        self.assertEqual(result["input_updates_per_s_lower_bound"], 0)
        later = self.events(values + [(1_200_000_000, 7, 1_050_000_000, 1_203_000_000)])
        result = self.report([r for r in later if not (r[0] in (3, 4) and r[1] == 900_000_000)])
        self.assertEqual(result["completed"], 1)
        self.assertAlmostEqual(result["input_updates_per_s_lower_bound"], 1 / 6)

    def test_setup_presentation_failure_is_retained_outside_contact_validity(self):
        values = [(500_000_000, 5, 400_000_000, 0),
                  (1_100_000_000, 6, 1_050_000_000, 1_103_000_000)]
        result = self.report(self.events(values))
        self.assertEqual(result["availability"], "observed")
        self.assertEqual(result["completed"], 1)
        self.assertEqual(result["outside_contact_presentation_errors"], {"zero_presentation": 1})

    def test_completed_workload_reports_counter_with_source_contact_boundaries(self):
        events = self.events(self.baseline() + [(1_100_000_000, 6, 1_050_000_000, 1_103_000_000)])
        events += [record(11, self.begin - 1, 2, 21),
                   record(11, self.begin + 20, 9, 21, 1, self.begin),
                   record(11, self.end + 30, 10, 21, 1, self.end),
                   record(11, self.end + 40, 3, 21)]
        result = analyze(header(raster_update_counter=True, workload={
            "name": "photo", "measurement_seconds": 6, "measured_contacts": 1}), events)
        progress = result["workload"]["canvas_update_progress"]
        self.assertEqual(progress["availability"], "observed")
        self.assertEqual(progress["active_input_seconds"], 6)
        self.assertEqual(progress["completed"], 1)

    def test_fresh_completion_gaps_have_millisecond_units(self):
        result = self.report(self.events(self.baseline() + [
            (1_100_000_000, 6, 1_050_000_000, 1_103_000_000),
            (1_200_000_000, 7, 1_150_000_000, 1_203_000_000)]))
        self.assertEqual(result["input_completion_gap_ms"]["max"], 100)
        self.assertAlmostEqual(result["input_updates_per_s_lower_bound"], 2 / 6)

    def test_completion_after_input_window_is_pending(self):
        result = self.report(self.events(self.baseline() + [
            (6_900_000_000, 6, 6_850_000_000, 7_010_000_000)]))
        self.assertEqual(result["submitted"], 1)
        self.assertEqual(result["completed"], 0)
        self.assertEqual(result["pending_at_input_end"], 1)
        self.assertEqual(result["input_updates_per_s_lower_bound"], 0)

    def test_ambiguous_missing_zero_and_regressing_observations_are_invalid(self):
        events = self.events(self.baseline() + [(1_100_000_000, 6, 1_050_000_000, 1_103_000_000)])
        variants = [events + [events[-1]], events[:-1],
                    events[:-1] + [record(4, 1_100_000_000, 0, 1_104_000_000, 2)],
                    self.events(self.baseline() + [(1_100_000_000, 4, 1_050_000_000, 1_103_000_000)])]
        for variant in variants:
            with self.subTest(variant=variant):
                result = self.report(variant)
                self.assertEqual(result["availability"], "invalid")
                self.assertNotIn("completed_per_s", result)

    def test_setup_renderer_reset_uses_the_last_precontact_baseline(self):
        events = self.events([(200_000_000, 136, 0, 203_000_000)] + self.baseline() + [
            (1_100_000_000, 6, 1_050_000_000, 1_103_000_000)])
        result = self.report(events)
        self.assertEqual(result["availability"], "observed")
        self.assertEqual(result["completed"], 1)
        events += self.events([(1_200_000_000, 2, 1_050_000_000, 1_203_000_000)])
        self.assertEqual(self.report(events)["availability"], "invalid")

    def test_callback_delivery_order_does_not_change_frame_accounting(self):
        events = self.events(self.baseline() + [(1_100_000_000, 6, 1_050_000_000, 1_103_000_000)])
        self.assertEqual(self.report(events), self.report(list(reversed(events))))

    def test_actual_contact_timestamps_and_complete_boundaries_are_required(self):
        markers = [[self.begin + 20, 9, 21, 1, self.begin],
                   [self.end + 30, 10, 21, 1, self.end]]
        self.assertEqual(workload_contact_intervals(markers, 1), [[self.begin, self.end]])
        self.assertEqual(workload_contact_intervals(markers[:-1], 1), [])
        self.assertEqual(workload_contact_intervals(markers + [markers[-1]], 1), [])
        grouped = defaultdict(list)
        self.assertEqual(canvas_update_progress(header(), grouped, [[self.begin, self.end]])["availability"], "unavailable")
        self.assertEqual(canvas_update_progress(header(raster_update_counter=True), grouped, [])["availability"], "unavailable")


class ReportChecks(unittest.TestCase):
    def window_fixture(self):
        begin, end = 1_000_000_000, 7_000_000_000
        events = [record(9, begin - 1, 0, 11), record(5, begin + 1, 1, 1, 0, 0)]
        for i, time in enumerate([begin + 1, begin + 3_000_000_000, end - 1]):
            events += [record(3, time, time, time, i + 1, 1), record(4, time, time, time + 1, i + 1)]
        return events, {"begin_ns": begin, "end_ns": end, "motion_witness": "retained native input observations"}

    def test_motion_window_deduplicates_frame_drawable_identity_and_rejects_missing_zero(self):
        events, window = self.window_fixture()
        base = motion_windows(header(), events, [window], 90)[0]
        self.assertEqual(base["presented_frames"], 3)
        self.assertTrue(base["instrumentation_eligible"])
        duplicate = motion_windows(header(), events + [events[-1]], [window], 90)[0]
        self.assertEqual(duplicate["presented_frames"], 3)
        self.assertEqual(duplicate["duplicate_presentation_records"], 1)
        self.assertFalse(duplicate["instrumentation_eligible"])
        missing = motion_windows(header(), events[:-1], [window], 90)[0]
        self.assertEqual(missing["missing_presentation_callbacks"], 1)
        self.assertFalse(missing["instrumentation_eligible"])
        zero_events = [*events[:-1], record(4, events[-1][1], 0, events[-1][3], events[-1][4])]
        zero = motion_windows(header(), zero_events, [window], 90)[0]
        self.assertEqual(zero["zero_time_presentations"], 1)
        self.assertFalse(zero["instrumentation_eligible"])

    def test_motion_window_rejects_readiness_loss_even_without_error_and_keeps_global_report(self):
        events, window = self.window_fixture()
        events += [record(9, window["begin_ns"] + 1_000_000, 0, 3, 0),
                   record(9, window["begin_ns"] + 2_000_000, 0, 11, 0)]
        result = motion_windows(header(), events, [window], 90)[0]
        self.assertTrue(result["fully_ready_at_begin"])
        self.assertEqual(result["frame_error_records"], 0)
        self.assertEqual(result["readiness_loss_records"], 1)
        self.assertFalse(result["instrumentation_eligible"])
        self.assertEqual(analyze(header(), events)["counts"]["presented_drawables"], 3)

    def test_motion_window_readiness_ignores_retired_catalog_bit(self):
        for flags in [9, 11, 13, 15]:
            with self.subTest(flags=flags):
                events, window = self.window_fixture()
                events[0][3] = flags
                events.append(record(9, window["begin_ns"] + 1_000_000, 0, flags ^ 2, 0))
                result = motion_windows(header(), events, [window], 90)[0]
                self.assertTrue(result["fully_ready_at_begin"])
                self.assertEqual(result["readiness_loss_records"], 0)
                self.assertTrue(result["instrumentation_eligible"])

    def test_motion_window_requires_canvas_and_shaders_at_start_and_during_motion(self):
        for flags in [0, 1, 2, 3, 8, 10]:
            with self.subTest(flags=flags):
                events, window = self.window_fixture()
                events[0][3] = flags
                result = motion_windows(header(), events, [window], 90)[0]
                self.assertFalse(result["fully_ready_at_begin"])
                self.assertFalse(result["instrumentation_eligible"])
                events[0][3] = 9
                events.append(record(9, window["begin_ns"] + 1_000_000, 0, flags, 0))
                result = motion_windows(header(), events, [window], 90)[0]
                self.assertTrue(result["fully_ready_at_begin"])
                self.assertEqual(result["readiness_loss_records"], 1)
                self.assertFalse(result["instrumentation_eligible"])

    def test_gpu_clocks_translate_epochs_rates_and_preserve_target_uncertainty(self):
        clocks = [record(14, 0, 500_000_000, 100, 2_000_000),
                  record(14, 100_000_000, 600_000_000, 300, 102_000_000)]
        events = clocks + [record(11, 5_000_000, 2, 1), record(11, 25_000_000, 3, 1)]
        for time, start, end in [(10_000_000, 120, 130), (20_000_000, 140, 160), (30_000_000, 160, 174)]:
            events += [frame(time), record(7, time, (end - start) * 500_000, 1, start, end),
                       record(4, time, time + 20_000_000)]
        result = analyze(header(workload={"name": "ink", "measurement_seconds": .02}), events)
        timing = result["gpu_completion"]
        self.assertEqual(timing["calibrated_frames"], 3)
        self.assertEqual(timing["frames_without_calibrated_gpu_times"], 0)
        self.assertEqual(timing["gpu_end_before_target"], 1)
        self.assertEqual(timing["gpu_end_after_target"], 1)
        self.assertEqual(timing["gpu_end_overlaps_target"], 1)
        self.assertEqual(timing["sampling_uncertainty_ms"]["max"], 2)
        self.assertEqual(timing["gpu_end_minus_presentation_target_ms"]["p50"], 0)
        self.assertEqual(timing["gpu_end_minus_presentation_target_ms"]["max"], 3)
        self.assertEqual(timing["gpu_end_to_presentation_ms"]["p50"], 12)
        self.assertEqual(result["workload"]["gpu_completion"]["calibrated_frames"], 2)
        self.assertEqual(result["workload"]["gpu_completion"]["gpu_end_overlaps_target"], 0)
        self.assertTrue(any("not Metal commit deadlines" in w for w in result["warnings"]))

    def test_gpu_clock_calibration_never_extrapolates_or_fabricates_endpoints(self):
        clocks = [[10, 1000, 100, 12], [110, 1100, 200, 112]]
        samples = {i: [i, 10, 1, start, end] for i, (start, end) in enumerate(
            [(100, 200), (99, 150), (150, 201), (0, 0), (160, 150)])}
        samples[5] = [5, 0, 2, 120, 130]
        calibrated, diagnostics = calibrate_gpu(clocks, samples)
        self.assertEqual(calibrated, {0: ((10, 12), (110, 112))})
        self.assertEqual(diagnostics["invalid_samples"], 0)
        old = analyze(header(), [frame(1), record(7, 1, 10, 1)])
        self.assertEqual(old["gpu_completion"]["frames_without_calibrated_gpu_times"], 1)
        self.assertIsNone(old["gpu_completion"]["gpu_end_to_presentation_ms"]["max"])

    def test_invalid_or_discontinuous_gpu_clocks_prevent_calibration(self):
        first = [10, 1000, 100, 12]
        for last in [[110, 0, 200, 112], [110, 1100, 0, 112], [110, 1100, 200, 109],
                     [110, 1000, 200, 112], [110, 1100, 100, 112], [110, 900, 90, 112]]:
            with self.subTest(last=last):
                calibrated, diagnostics = calibrate_gpu([first, last], {1: [1, 10, 1, 120, 130]})
                self.assertFalse(calibrated)
                self.assertGreater(diagnostics["invalid_samples"] + diagnostics["discontinuities"], 0)
        conflicting = analyze(header(gpu_timing_requested=False), [record(14, *first)])
        self.assertTrue(any("contradict" in w for w in conflicting["warnings"]))

    def test_admission_backpressure_remains_visible_and_separate_from_cpu_work(self):
        events = [record(0, 1, 2, 0, 1), record(11, 10, 2, 1),
                  record(0, 11, 12, 0, 2), record(0, 13, 14, 0, 3),
                  record(0, 15, 16, 0), record(0, 17, 18, 1), record(11, 20, 3, 1)]
        result = analyze(header(workload={"name": "ink", "measurement_seconds": .00000001}), events)
        self.assertEqual(result["counts"]["ticks_denied_admission"], 4)
        self.assertEqual(result["counts"]["ticks_denied_by_reason"],
                         {"inactive": 1, "owner_pending": 1, "unclassified": 2})
        self.assertEqual(result["workload"]["ticks_denied_by_reason"],
                         {"inactive": 0, "owner_pending": 1, "unclassified": 2})
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

    def test_ready_subset_requires_error_free_canvas_and_shaders_without_catalog_bit(self):
        for ready_flags in (9, 13):
            with self.subTest(ready_flags=ready_flags):
                events = [record(9, 10, 1, 1), frame(20),
                          record(9, 30, 20, 8), frame(40),
                          record(9, 50, 40, 9, 1), frame(60)]
                unready = analyze(header(), events)
                self.assertIsNone(unready["ready_seconds_from_start"])
                self.assertEqual(unready["frames_after_readiness"]["owner_service_ms"]["count"], 0)
                warning = "No error-free state with canvas and shaders ready was observed."
                self.assertIn(warning, unready["warnings"])
                events += [record(9, 70, 60, ready_flags), frame(80)]
                result = analyze(header(), events)
                self.assertEqual(result["frames_after_readiness"]["owner_service_ms"]["count"], 1)
                self.assertEqual(result["ready_seconds_from_start"], 70 / 1e9)
                self.assertNotIn(warning, result["warnings"])

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


class EncodedCameraChecks(unittest.TestCase):
    def events(self, revision=0, outcome=1):
        time, presented = 10_000_000, 13_000_000
        return [frame(time), record(15, time, 0, 0, revision, outcome),
                record(3, time, time + 10, time + 20, 7, 1),
                record(4, time, presented, presented + 1, 7)]

    def report(self, events, **changes):
        return analyze(header(encoded_camera_revision=True, **changes), events)["encoded_camera"]

    def test_old_metadata_is_unavailable_and_zero_revision_is_valid(self):
        self.assertEqual(analyze(header(), self.events())["encoded_camera"]["availability"], "unavailable")
        result = self.report(self.events())
        self.assertEqual(result["availability"], "observed")
        self.assertEqual(result["presentations"][0]["camera_revision"], 0)

    def test_again_requires_actual_presentation_but_is_not_a_timeout_flag(self):
        result = self.report(self.events(revision=4, outcome=2))
        self.assertEqual(len(result["presentations"]), 1)
        self.assertEqual(result["presentations"][0]["native_frame_result"], 1)
        result = self.report(self.events(revision=4, outcome=2)[:2])
        self.assertEqual(result["presentations"], [])
        self.assertEqual(result["counts"]["metadata_without_drawable"], 1)

    def test_invalid_result_never_inherits_a_later_successful_camera(self):
        for outcome in [0, 3, True]:
            with self.subTest(outcome=outcome):
                result = self.report(self.events(revision=12, outcome=outcome))
                self.assertEqual(result["presentations"], [])
                self.assertEqual(result["errors"]["invalid_camera_metadata"], 1)

    def test_duplicate_frame_metadata_drawable_and_callback_are_ambiguous(self):
        for index in range(4):
            with self.subTest(duplicated_kind=index):
                events = self.events()
                result = self.report(events + [events[index]])
                self.assertEqual(result["availability"], "invalid")
                self.assertEqual(result["presentations"], [])

    def test_zero_noncausal_and_conflicting_presentations_are_rejected(self):
        for presented, callback in [(0, 13_000_001), (10_000_010, 13_000_001), (13_000_000, 12_000_000)]:
            with self.subTest(presented=presented, callback=callback):
                result = self.report(self.events()[:3] + [record(4, 10_000_000, presented, callback, 7)])
                self.assertEqual(result["presentations"], [])
                self.assertEqual(result["availability"], "invalid")
        result = self.report(self.events() + [record(4, 10_000_000, 14_000_000, 14_000_001, 7)])
        self.assertEqual(result["presentations"], [])

    def test_callback_identity_must_match_the_acquired_drawable(self):
        result = self.report(self.events()[:3] + [record(4, 10_000_000, 13_000_000, 13_000_001, 8)])
        self.assertEqual(result["presentations"], [])
        self.assertEqual(result["errors"]["missing_or_duplicate_callback"], 1)
        self.assertEqual(result["counts"]["callbacks_without_camera_join"], 1)

    def test_camera_is_attached_to_encoded_frame_not_latest_model_state(self):
        early = self.events(revision=2)
        later = [frame(20_000_000), record(15, 20_000_000, 0, 0, 9, 2)]
        result = self.report(early + later)
        self.assertEqual([r["camera_revision"] for r in result["presentations"]], [2])
        self.assertEqual(result["counts"]["metadata_without_drawable"], 1)

    def test_different_drawables_at_one_presentation_time_do_not_inflate_count(self):
        second = frame(11_000_000)
        second[3:5] = [12_001_000, 12_500_000]
        events = self.events() + [second, record(15, 11_000_000, 0, 0, 2, 1),
            record(3, 11_000_000, 12_010_000, 12_100_000, 8, 1),
            record(4, 11_000_000, 13_000_000, 13_000_001, 8)]
        result = self.report(events)
        self.assertEqual(result["presentations"], [])
        self.assertEqual(result["errors"]["nonunique_presentation_time"], 1)


class CameraWitnessChecks(unittest.TestCase):
    def fixture(self):
        events = []
        for index, (time, revision) in enumerate([(5_000_000, 0), (10_000_000, 1), (20_000_000, 2), (30_000_000, 3)], 1):
            events += [frame(time), record(15, time, 0, 0, revision, 2),
                       record(3, time, time + 10, time + 20, index, 1),
                       record(4, time, time + 3_000_000, time + 3_000_001, index)]
        h = header(encoded_camera_revision=True, clock="CACurrentMediaTime nanoseconds", process_identifier=42)
        poses = []
        for revision, time, x in [(0, 1_000_000, 0), (1, 11_000_000, 0), (2, 21_000_000, 10), (3, 31_000_000, 10)]:
            poses.append({"time_ns": time, "drawing_identity": "1:4", "camera": {
                "revision": revision, "viewport": [1200, 900], "translation": [x, 0],
                "zoom": 1, "rotation": 0, "flipped": [False, False]}})
        witness = {"process_identifier": 42, "clock": h["clock"], "poses": poses,
            "contacts": [{"id": "pan1", "drawing_identity": "1:4", "baseline_revision": 0,
                          "full_contact_begin_ns": 9_000_000, "full_contact_end_ns": 40_000_000}]}
        return h, events, witness

    def report(self, witness=None, events=None):
        h, original_events, original_witness = self.fixture()
        return camera_contacts(h, analyze(h, original_events if events is None else events)["encoded_camera"],
                               original_witness if witness is None else witness)

    def test_exact_contact_denominator_and_edges_are_preserved(self):
        result = self.report()
        self.assertEqual(result["availability"], "observed")
        contact = result["contacts"][0]
        self.assertEqual(contact["actual_presentations"], 3)
        self.assertAlmostEqual(contact["actual_presentations_per_s"], 3 / .031)
        self.assertEqual(contact["leading_gap_ms"], 4)
        self.assertEqual(contact["trailing_gap_ms"], 7)
        self.assertEqual(contact["longest_gap_including_contact_edges_ms"], 10)
        self.assertEqual(contact["known_baseline_pose_presentations"], 1)
        self.assertEqual(contact["known_nonbaseline_pose_presentations"], 2)
        self.assertEqual(contact["successive_geometry_changes"], 1)
        self.assertEqual(contact["static_geometry_presentations"], 2)
        self.assertEqual(contact["same_geometry_new_revision_presentations"], 2)
        self.assertEqual(contact["timely_witness_geometry_progress_presentations"], 1)
        self.assertEqual(contact["geometry_progress_trailing_gap_ms"], 17)
        self.assertEqual(contact["first_proven_changed_pose_upper_bound_ms"], 14)

    def test_identity_revision_does_not_prove_onset(self):
        _, _, witness = self.fixture()
        for pose in witness["poses"]: pose["camera"]["translation"] = [0, 0]
        contact = self.report(witness)["contacts"][0]
        self.assertEqual(contact["known_baseline_pose_presentations"], 3)
        self.assertIsNone(contact["first_proven_changed_pose_upper_bound_ms"])

    def test_unmapped_pose_is_unknown_and_onset_remains_an_upper_bound(self):
        _, _, witness = self.fixture()
        witness["poses"] = [pose for pose in witness["poses"] if pose["camera"]["revision"] != 2]
        result = self.report(witness)
        self.assertEqual(result["availability"], "partial")
        self.assertEqual(result["contacts"][0]["unknown_geometry_reasons"], {"unmapped_revision": 1})
        self.assertEqual(result["contacts"][0]["first_proven_changed_pose_upper_bound_ms"], 24)

    def test_conflicting_geometry_cannot_be_replaced_by_latest_model(self):
        _, _, witness = self.fixture()
        later = deepcopy(witness["poses"][2]); later["time_ns"] = 35_000_000
        later["camera"]["translation"] = [999, 0]
        witness["poses"].append(later)
        result = self.report(witness)
        self.assertEqual(result["availability"], "invalid")
        self.assertEqual(result["errors"]["conflicting_revision_geometry"], 1)
        self.assertEqual(result["contacts"][0]["unknown_geometry_reasons"], {"conflicting_revision_geometry": 1})

    def test_later_unpresented_model_revision_does_not_relabel_earlier_frame(self):
        _, _, witness = self.fixture()
        later = deepcopy(witness["poses"][-1]); later["time_ns"] = 35_000_000
        later["camera"]["revision"] = 99; later["camera"]["translation"] = [999, 0]
        witness["poses"].append(later)
        self.assertEqual(self.report(witness)["contacts"][0]["first_proven_changed_pose_upper_bound_ms"], 14)

    def test_pid_clock_and_drawing_must_match(self):
        for field, value, error in [("process_identifier", 43, "witness_process_mismatch"),
                                    ("clock", "wall clock", "witness_clock_mismatch")]:
            with self.subTest(field=field):
                _, _, witness = self.fixture(); witness[field] = value
                self.assertEqual(self.report(witness)["errors"], {error: 1})
        _, _, witness = self.fixture()
        witness["contacts"][0]["drawing_identity"] = "different:4"
        result = self.report(witness)
        self.assertEqual(result["contacts"][0]["unknown_geometry_presentations"], 3)
        self.assertIn("unknown_contact_baseline", result["errors"])

    def test_missing_baseline_up_and_overlapping_contacts_are_invalid(self):
        for field in ["baseline_revision", "full_contact_end_ns"]:
            with self.subTest(field=field):
                _, _, witness = self.fixture(); del witness["contacts"][0][field]
                self.assertEqual(self.report(witness)["availability"], "invalid")
        _, _, witness = self.fixture(); extra = deepcopy(witness["contacts"][0])
        extra.update(id="pan2", full_contact_begin_ns=20_000_000)
        witness["contacts"].append(extra)
        self.assertEqual(self.report(witness)["errors"]["invalid_or_overlapping_contact"], 1)

    def test_invalid_geometry_and_unobserved_baseline_remain_unknown(self):
        _, _, witness = self.fixture()
        witness["poses"][2]["camera"]["zoom"] = float("nan")
        result = self.report(witness)
        self.assertEqual(result["errors"]["invalid_pose"], 1)
        self.assertEqual(result["contacts"][0]["unknown_geometry_presentations"], 1)
        _, _, witness = self.fixture()
        witness["poses"][0]["time_ns"] = 10_000_000
        result = self.report(witness)
        self.assertFalse(result["contacts"][0]["baseline_geometry_observed_before_contact"])
        self.assertIsNone(result["contacts"][0]["first_proven_changed_pose_upper_bound_ms"])

    def test_recording_edges_and_long_contact_stall_are_not_trimmed(self):
        _, _, witness = self.fixture(); witness["contacts"][0]["full_contact_end_ns"] = 400_000_000
        contact = self.report(witness)["contacts"][0]
        self.assertAlmostEqual(contact["actual_presentations_per_s"], 3 / .391)
        self.assertEqual(contact["longest_gap_including_contact_edges_ms"], 367)
        witness["contacts"][0]["full_contact_end_ns"] = 11_000_000_000
        self.assertIn("contact_outside_recording", self.report(witness)["errors"])

    def test_no_actual_presentation_keeps_full_contact_gap_without_zero_latency(self):
        _, events, _ = self.fixture()
        result = self.report(events=[row for row in events if row[0] in (1, 15)])
        self.assertEqual(result["availability"], "unavailable")
        contact = result["contacts"][0]
        self.assertEqual(contact["actual_presentations"], 0)
        self.assertEqual(contact["longest_gap_including_contact_edges_ms"], 31)
        self.assertIsNone(contact["first_any_canvas_present_ms"])
        self.assertIsNone(contact["first_proven_changed_pose_upper_bound_ms"])

    def test_precontact_frame_cannot_prove_new_input_onset(self):
        _, events, witness = self.fixture()
        events = [row for row in events if row[1] in (5_000_000, 20_000_000)]
        for row in events:
            if row[1] == 20_000_000:
                row[1] = 7_000_000
                if row[0] == 1: row[3] = 7_001_000
        contact = self.report(witness, events)["contacts"][0]
        self.assertEqual(contact["precontact_nonbaseline_frame_presentations"], 1)
        self.assertIsNone(contact["first_proven_changed_pose_upper_bound_ms"])

    def test_frozen_nonbaseline_camera_does_not_make_repeated_frames_motion(self):
        _, _, witness = self.fixture()
        for pose in witness["poses"][1:]: pose["camera"]["translation"] = [10, 0]
        contact = self.report(witness)["contacts"][0]
        self.assertEqual(contact["known_nonbaseline_pose_presentations"], 3)
        self.assertEqual(contact["successive_geometry_changes"], 1)
        self.assertEqual(contact["static_geometry_presentations"], 2)
        self.assertEqual(contact["timely_witness_geometry_progress_presentations"], 1)
        self.assertEqual(contact["geometry_progress_longest_gap_including_contact_edges_ms"], 27)

    def test_late_geometry_witness_can_attribute_state_but_cannot_prove_onset(self):
        _, _, witness = self.fixture()
        for pose in witness["poses"][1:]: pose["time_ns"] += 30_000_000
        contact = self.report(witness)["contacts"][0]
        self.assertEqual(contact["known_nonbaseline_pose_presentations"], 2)
        self.assertEqual(contact["late_geometry_observation_presentations"], 3)
        self.assertEqual(contact["timely_witness_geometry_progress_presentations"], 0)
        self.assertIsNone(contact["first_proven_changed_pose_upper_bound_ms"])
        self.assertEqual(contact["geometry_progress_longest_gap_including_contact_edges_ms"], 31)


class ConsumedPaintChecks(unittest.TestCase):
    def fixture(self, values=(9_000_000, 19_000_000, 29_000_000)):
        events = []
        for index, value in enumerate(values, 1):
            time = index * 10_000_000
            events += [frame(time), record(15, time, value),
                       record(3, time, 0, 0, index, 1), record(4, time, time + 3_000_000, 0, index)]
        return events

    def report(self, events):
        return analyze(header(), events)["engine_consumed_paint_progress"]

    def test_repeated_consumption_and_duplicate_drawables_do_not_inflate_progress(self):
        events = self.fixture((9_000_000, 9_000_000, 29_000_000))
        events += [events[3], record(4, 30_000_000, 34_000_000, 0, 4)]
        result = self.report(events)
        self.assertEqual(result["presented_engine_consumed_progress"], 2)
        self.assertEqual(result["counts"]["repeated_counter"], 1)
        self.assertEqual(result["counts"]["duplicate_presentation_records"], 1)
        self.assertEqual(result["counts"]["repeated_presented_counter"], 2)
        self.assertEqual(result["consumed_timestamp_to_present_ms"]["max"], 4)
        delayed = [e for e in events if not (e[0] == 4 and e[1] == 10_000_000)]
        result = self.report(delayed)
        self.assertEqual(result["presented_engine_consumed_progress"], 2)
        self.assertEqual(result["counts"]["missing_presentation_callbacks"], 1)
        self.assertEqual(result["consumed_timestamp_to_present_ms"]["max"], 14)

    def test_missing_zero_and_duplicate_counter_samples_remain_explicit(self):
        events = [e for e in self.fixture((0, 19_000_000, 29_000_000)) if not (e[0] == 15 and e[1] == 20_000_000)]
        events.append(record(15, 30_000_000, 29_000_000))
        result = self.report(events)
        self.assertEqual(result["presented_engine_consumed_progress"], 0)
        self.assertEqual(result["counts"]["zero_counter"], 1)
        self.assertEqual(result["counts"]["missing_counter"], 1)
        self.assertEqual(result["counts"]["duplicate_counter"], 1)
        self.assertEqual(result["counts"]["presentations_without_valid_counter"], 3)

    def test_dropped_zero_and_conflicting_presentations_are_not_zero_latency(self):
        events = [e for e in self.fixture() if e[0] != 4]
        events += [record(4, 20_000_000, 0, 0, 2),
                   record(4, 30_000_000, 33_000_000, 0, 3), record(4, 30_000_000, 34_000_000, 0, 3)]
        result = self.report(events)
        self.assertEqual(result["counts"]["missing_presentation_callbacks"], 1)
        self.assertEqual(result["counts"]["zero_time_presentations"], 1)
        self.assertEqual(result["counts"]["conflicting_presentation_identities"], 1)
        self.assertEqual(result["presented_engine_consumed_progress"], 0)
        self.assertIsNone(result["consumed_timestamp_to_present_ms"]["max"])

    def test_callback_arrival_order_is_not_presentation_order(self):
        events = self.fixture()
        callbacks = [e for e in events if e[0] == 4]
        events = [e for e in events if e[0] != 4] + callbacks[::-1]
        self.assertEqual(self.report(events)["presented_engine_consumed_progress"], 3)
        events = [e for e in events if e[0] != 4] + [record(4, 10_000_000, 40_000_000, 0, 1), *callbacks[1:]]
        result = self.report(events)
        self.assertEqual(result["presented_engine_consumed_progress"], 2)
        self.assertEqual(result["counts"]["regressing_presented_counter"], 1)

    def test_regression_future_and_noncausal_times_remain_visible(self):
        events = self.fixture((9_000_000, 8_000_000, 33_000_000))
        result = self.report(events)
        self.assertEqual(result["counts"]["regressing_counter"], 1)
        self.assertEqual(result["counts"]["future_counter"], 1)
        self.assertEqual(result["presented_engine_consumed_progress"], 1)
        events = [e for e in self.fixture() if not (e[0] == 4 and e[1] == 10_000_000)]
        events.append(record(4, 10_000_000, 8_000_000, 0, 1))
        self.assertEqual(self.report(events)["counts"]["noncausal_presentations"], 1)
        after_admission = self.report(self.fixture((11_000_000, 21_000_000, 31_000_000)))
        self.assertEqual(after_admission["presented_engine_consumed_progress"], 3)
        self.assertEqual(after_admission["counts"]["future_counter"], 0)

    def test_old_trace_is_unavailable_and_receipt_proxy_stays_separate(self):
        events = [record(2, 100, 200, 300, 90, 95, 1, 0, 2, 0, 1),
                  frame(400, receipt=100), record(4, 400, 1_000_100)]
        result = analyze(header(), events)
        self.assertEqual(result["engine_consumed_paint_progress"]["availability"], "unavailable")
        self.assertEqual(result["engine_consumed_paint_progress"]["presented_engine_consumed_progress"], 0)
        self.assertIsNone(result["engine_consumed_paint_progress"]["consumed_timestamp_to_present_ms"]["max"])
        self.assertEqual(result["presentation"]["first_associated_present_per_owner_receipt_proxy_ms"]["count"], 1)

    def test_workload_and_motion_windows_keep_prior_counter_baseline_and_edges(self):
        begin, end = 20_000_000, 40_000_000
        events = self.fixture((9_000_000, 9_000_000, 29_000_000))
        events += [record(11, begin, 2, 1), record(11, end, 3, 1)]
        result = analyze(header(workload={"name": "ink"}), events)
        self.assertEqual(result["engine_consumed_paint_progress"]["presented_engine_consumed_progress"], 2)
        self.assertEqual(result["workload"]["engine_consumed_paint_progress"]["presented_engine_consumed_progress"], 1)
        scale = 300
        scaled = []
        for event in events:
            if event[0] in (1, 15, 3, 4):
                row = event.copy()
                for index in {1: (1, 2, 3, 4), 15: (1, 2), 3: (1,), 4: (1, 2)}[row[0]]:
                    row[index] *= scale
                scaled.append(row)
        window = {"begin_ns": begin * scale, "end_ns": end * scale, "motion_witness": "retained"}
        observed = motion_windows(header(), scaled, [window], 90)[0]["engine_consumed_paint_progress"]
        self.assertEqual(observed["presented_engine_consumed_progress"], 1)
        late = [e for e in self.fixture() if not (e[0] == 4 and e[1] == 30_000_000)]
        late += [record(4, 30_000_000, end + 1, 0, 3), record(11, begin, 2, 1), record(11, end, 3, 1)]
        scoped = analyze(header(workload={"name": "ink"}), late)["workload"]["engine_consumed_paint_progress"]
        self.assertEqual(scoped["counts"]["presentations_outside_interval"], 1)


if __name__ == "__main__":
    unittest.main()
