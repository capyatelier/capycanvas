import json
import tempfile
import unittest
from pathlib import Path

from metal_frames import correlate
from test_metal_trace import gpu_row, process, table, value


class MetalFrameChecks(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.gpu, self.cpu, self.clock, self.native = [self.root / name for name in ["gpu.xml", "cpu.xml", "time.xml", "frames.jsonl"]]
        self.header = {"schema": 1, "clock": "CACurrentMediaTime nanoseconds", "process_identifier": 42}
        # Non-unit native timebase: absolute tick 240 converts to 10,000 ns.
        self.anchor()
        self.events = [self.frame(10100, 10200, 11000), self.frame(11900, 12000, 13000),
                       [4, 10100, 18000, 18010, 1, 0, 0, 0, 0, 0, 0]]

    @staticmethod
    def frame(identity, begin, end):
        return [1, identity, identity + 1000, begin, end, 1, 1, 1, 1, 1, 0]

    def anchor(self, anchors=(240,), denominator=3):
        rows = []
        for epoch in anchors:
            ratio = value("mach-timebase-info")
            ratio.extend([value("field", 125), value("field", denominator)])
            rows.append([value("sample-time", 0), value("mach-absolute-time", epoch), ratio])
        table(self.clock, "time-info", ["update-time", "mabs-epoch", "timebase-info"], rows)

    @staticmethod
    def encoder(start=250, duration=100, identity=1, buffer=10, pid=42):
        return [value("start-time", start), value("duration", duration), process(pid),
                value("encoder-id", identity), value("buffer-id", buffer)]

    def report(self, gpu=None, cpu=None, **request_paths):
        if gpu is None:
            # GPU runs after CPU frame completion; clip-to-CPU would be wrong.
            gpu = [gpu_row(2000, 1000, process(42), 1, 10),
                   gpu_row(2500, 1500, process(42), 1, 10, channel="Fragment"),
                   gpu_row(1000, 1_000_000, process(99), 1, 10)]
        table(self.gpu, "metal-gpu-intervals", ["start", "duration", "process", "state", "channel-name", "encoder-id", "cmdbuffer-id", "start-latency"], gpu)
        table(self.cpu, "metal-application-encoders-list", ["start", "duration", "process", "encoder-id", "cmdbuffer-id"], cpu if cpu is not None else [self.encoder()])
        self.native.write_text("\n".join(json.dumps(row) for row in [self.header, *self.events]) + "\n")
        return correlate(self.gpu, self.cpu, self.clock, self.native, 42, **request_paths)

    @staticmethod
    def submission(start=500, duration=250, buffer=30, pid=42):
        return [value("start-time", start), value("duration", duration), process(pid), value("buffer-id", buffer)]

    @staticmethod
    def request(timestamp=1500, buffer=30, pid=42):
        # A requested at-time is deliberately unrelated to the real endpoint.
        return [value("start-time", timestamp), value("sentinel") if buffer is None else value("buffer-id", buffer),
                process(pid), value("start-time", 999999)]

    def request_report(self, submissions=None, requests=None, **options):
        submission_path, request_path = self.root / "submissions.xml", self.root / "requests.xml"
        table(submission_path, "metal-application-command-buffer-submissions",
              ["start", "duration", "process", "cmdbuffer-id"],
              [self.submission()] if submissions is None else submissions)
        table(request_path, "ca-client-present-request", ["timestamp", "cmdbuffer-id", "process", "at-time"],
              [self.request()] if requests is None else requests)
        return self.report(submission_path=submission_path, request_path=request_path, **options)["presentation_requests"]

    def test_clock_identity_union_and_execution_after_cpu_return(self):
        report = self.report()
        frame = report["frames"][0]
        self.assertEqual(report["clock_offset_ns"], {"numerator": 10000, "denominator": 1})
        self.assertEqual(report["counts"]["other_or_unattributed_gpu_rows"], 1)
        self.assertEqual(frame["frame"], 10100)
        self.assertAlmostEqual(frame["gpu_active_union_ms"], .002)
        self.assertAlmostEqual(frame["gpu_stage_sum_ms"], .0025)
        self.assertAlmostEqual(frame["gpu_end_to_present_ms"], .004)
        self.assertEqual(report["frames_with_all_observed_encoders_matched"], 1)

    def test_missing_encoder_execution_excludes_partial_frame_from_distribution(self):
        report = self.report(cpu=[self.encoder(), self.encoder(500, identity=2)])
        self.assertEqual(report["frames"][0]["cpu_encoders_without_gpu"], 1)
        self.assertEqual(report["frames_with_all_observed_encoders_matched"], 0)
        self.assertIsNone(report["observed_covered_frame_gpu_active_ms"]["p50"])
        self.assertAlmostEqual(report["frames"][0]["gpu_active_union_ms"], .002)

    def test_straddling_encoder_does_not_join_by_start_alone(self):
        report = self.report(cpu=[self.encoder(), self.encoder(900, 1500, identity=2)])
        self.assertEqual(report["counts"]["cpu_encoders_outside_native_frames"], 1)
        self.assertEqual(report["frames"][0]["cpu_encoders"], 1)

    def test_duplicate_encoder_marks_observed_frame_incomplete(self):
        report = self.report(cpu=[self.encoder(), self.encoder()])
        self.assertEqual(report["counts"]["ambiguous_cpu_encoder_rows"], 2)
        self.assertEqual(report["frames_with_all_observed_encoders_matched"], 0)
        self.assertIsNone(report["frames"][0]["gpu_active_union_ms"])

    def test_wrong_buffer_and_gpu_before_encoding_are_not_valid_work(self):
        for start, buffer in [(2000, 99), (0, 10)]:
            with self.subTest(start=start, buffer=buffer):
                report = self.report(gpu=[gpu_row(start, 500, process(42), 1, buffer)])
                self.assertEqual(report["counts"]["gpu_identity_or_order_conflicts"], 1)
                self.assertEqual(report["frames_with_all_observed_encoders_matched"], 0)

    def test_invalid_gpu_row_invalidates_encoder_even_with_another_good_stage(self):
        report = self.report(gpu=[gpu_row(2000, 500, process(42), 1, 10), gpu_row(2600, 0, process(42), 1, 10)])
        self.assertEqual(report["counts"]["invalid_active_gpu_intervals"], 1)
        self.assertEqual(report["frames_with_all_observed_encoders_matched"], 0)

    def test_wrong_process_clock_and_nonoverlapping_capture_rejected(self):
        self.header["process_identifier"] = 99
        with self.assertRaisesRegex(ValueError, "process"):
            self.report()
        self.header["process_identifier"] = 42
        self.header["clock"] = "wall time"
        with self.assertRaisesRegex(ValueError, "clock"):
            self.report()
        self.header["clock"] = "CACurrentMediaTime nanoseconds"
        with self.assertRaisesRegex(ValueError, "No CPU encoder"):
            self.report(cpu=[self.encoder(9000)])

    def test_changing_anchor_invalid_timebase_and_overlapping_frames_rejected(self):
        self.anchor((240, 300))
        with self.assertRaisesRegex(ValueError, "clock anchors"):
            self.report()
        self.anchor(denominator=0)
        with self.assertRaisesRegex(ValueError, "timebase"):
            self.report()
        self.anchor()
        self.events.append(self.frame(10500, 10500, 11500))
        with self.assertRaisesRegex(ValueError, "overlapping"):
            self.report()

    def test_missing_duplicate_and_zero_presentations_are_not_latency_samples(self):
        for extra in [[], [[4, 10100, 0, 18010, 1, 0, 0, 0, 0, 0, 0]], [self.events[2], self.events[2]]]:
            with self.subTest(extra=extra):
                self.events = self.events[:2] + extra
                report = self.report()
                self.assertIsNone(report["frames"][0]["gpu_end_to_present_ms"])

    def test_legacy_pid_and_recorder_loss_are_explicit(self):
        del self.header["process_identifier"]
        self.header["dropped_records"] = 1
        report = self.report()
        self.assertTrue(any("no PID" in message for message in report["warnings"]))
        self.assertTrue(any("overflow" in message for message in report["warnings"]))

    def test_request_clock_join_uses_callback_and_native_display_endpoint(self):
        report = self.request_report(requests=[self.request(), self.request(pid=99)])
        self.assertEqual(report["counts"]["mapped_requests"], 1)
        self.assertEqual(report["counts"]["other_or_unattributed_requests"], 1)
        row = report["frames"][0]
        self.assertAlmostEqual(row["request_after_owner_ms"], .0005)
        self.assertAlmostEqual(row["request_to_frame_target_ms"], -.0004)
        self.assertAlmostEqual(row["request_to_present_ms"], .0065)
        self.assertAlmostEqual(row["request_to_last_observed_gpu_end_ms"], .0025)

    def test_duplicate_submission_or_multiple_requests_cannot_select_an_endpoint(self):
        report = self.request_report(submissions=[self.submission(), self.submission()])
        self.assertEqual(report["counts"]["requests_with_ambiguous_submission"], 1)
        self.assertEqual(report["frames"], [])
        self.assertIsNone(report["distributions_ms"]["request_to_present_ms"]["p50"])
        for requests in [[self.request(), self.request()], [self.request(), self.request(1600, buffer=31)]]:
            report = self.request_report(submissions=[self.submission(), self.submission(buffer=31)], requests=requests)
            self.assertEqual(report["counts"]["frames_with_multiple_requests"], 1)
            self.assertEqual(report["frames"][0]["request_count"], 2)
            self.assertIsNone(report["frames"][0]["request_to_present_ms"])

    def test_missing_foreign_or_straddling_submission_does_not_join(self):
        for submissions in [[], [self.submission(pid=99)]]:
            report = self.request_report(submissions=submissions)
            self.assertEqual(report["counts"]["requests_without_submission"], 1)
        report = self.request_report(submissions=[self.submission(start=900, duration=300)])
        self.assertEqual(report["counts"]["requests_outside_native_frames"], 1)
        self.assertEqual(report["frames"], [])

    def test_invalid_request_or_submission_order_is_not_a_timing_sample(self):
        for request in [self.request(timestamp=-1), self.request(buffer=0), self.request(buffer=None)]:
            report = self.request_report(requests=[request])
            self.assertEqual(report["counts"]["invalid_requests"], 1)
        for submission in [self.submission(duration=-1), self.submission(duration=2000)]:
            report = self.request_report(submissions=[submission])
            self.assertEqual(report["counts"]["requests_with_invalid_submission_order"], 1)
        report = self.request_report(requests=[self.request(timestamp=9000)])
        self.assertEqual(report["counts"]["frames_presented_before_request"], 1)
        self.assertIsNone(report["frames"][0]["request_to_present_ms"])

    def test_partial_gpu_and_missing_display_have_separate_request_coverage(self):
        report = self.request_report(cpu=[self.encoder(), self.encoder(500, identity=2)])
        self.assertFalse(report["frames"][0]["all_observed_encoders_have_valid_gpu"])
        self.assertIsNone(report["frames"][0]["request_to_last_observed_gpu_end_ms"])
        self.assertAlmostEqual(report["frames"][0]["request_to_present_ms"], .0065)
        # The request is valid even when the trace omitted this frame's encoders.
        report = self.request_report(cpu=[self.encoder(2100, identity=2)])
        self.assertAlmostEqual(report["frames"][0]["request_to_present_ms"], .0065)
        for extra in [[], [[4, 10100, 0, 18010, 1, 0, 0, 0, 0, 0, 0]], [self.events[2], self.events[2]]]:
            self.events = self.events[:2] + extra
            report = self.request_report()
            self.assertIsNone(report["frames"][0]["request_to_present_ms"])
            self.assertAlmostEqual(report["frames"][0]["request_after_owner_ms"], .0005)

    def test_optional_request_tables_must_be_paired(self):
        for paths in [{"submission_path": self.root / "missing.xml"}, {"request_path": self.root / "missing.xml"}]:
            with self.assertRaisesRegex(ValueError, "supplied together"):
                self.report(**paths)

    def test_missing_native_target_is_not_a_negative_deadline_sample(self):
        self.events[0][2] = 0
        report = self.request_report()
        self.assertIsNone(report["frames"][0]["request_to_frame_target_ms"])
        self.assertEqual(report["counts"]["frames_without_target"], 1)
        self.assertAlmostEqual(report["frames"][0]["request_to_present_ms"], .0065)


if __name__ == "__main__":
    unittest.main()
