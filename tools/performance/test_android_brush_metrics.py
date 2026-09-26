import unittest
from android_brush_metrics import completion_window


class CompletionWindowTests(unittest.TestCase):
    def test_excludes_boundary_and_empty_updates_and_retains_pending(self):
        report = {
            "motion": {"begin_ns": 100, "end_ns": 200},
            "display_before": {"submitted_frames": 9, "completed_frames": 8},
            "display_after_input": {"submitted_frames": 14, "completed_frames": 13},
            "renderer_before": {"rows": [{"label": "Frames", "value": "6"}]},
            "completions": [[9, 90, 110, 6], [10, 100, 130, 7],
                            [11, 140, 160, 7], [12, 150, 190, 8],
                            [13, 180, 200, 9], [14, 200, 220, 10]],
        }
        result = completion_window(report)
        self.assertEqual(result["submitted"], 3)
        self.assertEqual(result["completed"], 2)
        self.assertEqual(result["pending_at_input_end"], 1)
        self.assertEqual(result["empty_updates"], 1)
        self.assertEqual(result["snapshot_pending"], 1)
        self.assertEqual(result["accounting"], "input-window-nonempty")


if __name__ == "__main__":
    unittest.main()
