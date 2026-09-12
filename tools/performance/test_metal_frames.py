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

    def report(self, gpu=None, cpu=None):
        if gpu is None:
            # GPU runs after CPU frame completion; clip-to-CPU would be wrong.
            gpu = [gpu_row(2000, 1000, process(42), 1, 10),
                   gpu_row(2500, 1500, process(42), 1, 10, channel="Fragment"),
                   gpu_row(1000, 1_000_000, process(99), 1, 10)]
        table(self.gpu, "metal-gpu-intervals", ["start", "duration", "process", "state", "channel-name", "encoder-id", "cmdbuffer-id", "start-latency"], gpu)
        table(self.cpu, "metal-application-encoders-list", ["start", "duration", "process", "encoder-id", "cmdbuffer-id"], cpu if cpu is not None else [self.encoder()])
        self.native.write_text("\n".join(json.dumps(row) for row in [self.header, *self.events]) + "\n")
        return correlate(self.gpu, self.cpu, self.clock, self.native, 42)

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


if __name__ == "__main__":
    unittest.main()
