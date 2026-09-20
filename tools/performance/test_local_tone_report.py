import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("local_tone", Path(__file__).with_name("local-tone-report.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class LocalToneCorrelation(unittest.TestCase):
    def test_future_identical_input_and_discarded_frames_cannot_reduce_latency(self):
        report = dict(requests=[
            dict(requested_ns=1_000_000, recipe={"exposure": 1}, cpu_ms=.2, lateness_ms=0),
            dict(requested_ns=5_000_000, recipe={"exposure": 2}, cpu_ms=.2, lateness_ms=0),
            dict(requested_ns=9_000_000, recipe={"exposure": 1}, cpu_ms=.2, lateness_ms=0)],
            worker_cpu=[[1, 0, 0, .3, 2_000_000], [2, 0, 0, .3, 6_000_000], [3, 0, 0, .3, 3_000_000]],
            worker_gpu=[[1, .1]], hdr_views=[[1, {"exposure": 1}], [2, {"exposure": 2}], [3, {"exposure": 1}]],
            canvas_presentation=[[1, 10_000_000, 8_333_333, 1], [2, 11_000_000, 8_333_333, 0], [3, 12_000_000, 8_333_333, 1]],
            camera_work=[[1, 10, 0, 0, 0]], seconds=1, concurrent=False,
            canvas_ready_ms=1, guide_ready_ms=2, renderer_resident_bytes=10, final_memory=[])
        result = module.summarize(report)
        # Frame 1 must match input at 1 ms, never the identical future input at
        # 9 ms. Frame 3 repeats that already displayed input; frame 2 was discarded.
        self.assertEqual(result["request_to_present"]["p99_ms"], 9)
        self.assertEqual(result["presented_requests"], 1)
        self.assertEqual(result["unmatched_requests"], 2)
        self.assertEqual(result["first_request_to_first_present_ms"], 9)


if __name__ == "__main__":
    unittest.main()
