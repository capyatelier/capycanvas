import copy
import unittest
from android_brush_metrics import completion_window, contact_latencies, input_completions, validate_setup


class SetupTests(unittest.TestCase):
    def setUp(self):
        self.requested = dict(preset=1, brush_size=1024, mode="constant", prediction=True,
                              speed=1, duration_ms=5000, repeats=3, radii=[240, 140],
                              photo_layers=4, horizon=16, zoom=1, stats_panel=False)
        self.info = {key: self.requested[key] for key in
                     ("preset", "brush_size", "mode", "prediction", "speed", "duration_ms", "repeats", "stats_panel")}
        self.info.update(radii=[240, 140], state={"camera": {"work_area": [632, 153, 754, 1037], "zoom": 1},
                         "layers": [{}] * 6, "brush": {"diameter": 1024, "preset": 1},
                         "settings": {"feedback": True, "prediction_ms": 16}})

    def test_accepts_matched_and_work_area_clamped_trajectories(self):
        validate_setup(self.info, self.requested)
        self.requested["radii"] = [520, 299]
        self.info["radii"] = [754 * .45, 299]
        validate_setup(self.info, self.requested)

    def test_rejects_ignored_arguments_in_old_device_runners(self):
        for change, field in [(lambda d: d.update(radii=[754 * .45, 299]), "radius"),
                              (lambda d: d["state"].update(layers=[{}] * 3), "layers"),
                              (lambda d: d["state"]["camera"].update(zoom=.16), "zoom"),
                              (lambda d: d["state"]["brush"].update(preset=5), "selected_preset"),
                              (lambda d: d["state"]["settings"].update(prediction_ms=64), "horizon"),
                              (lambda d: d["state"]["settings"].update(feedback=False), "feedback"),
                              (lambda d: d.update(stats_panel=True), "stats_panel"),
                              (lambda d: d.pop("stats_panel"), "stats_panel")]:
            with self.subTest(field=field):
                info = copy.deepcopy(self.info)
                change(info)
                with self.assertRaisesRegex(ValueError, field):
                    validate_setup(info, self.requested)

    def test_rejects_reusing_a_completed_run_for_different_settings(self):
        self.requested["duration_ms"] = 10000
        with self.assertRaisesRegex(ValueError, "duration_ms"):
            validate_setup(self.info, self.requested)

    def test_rejects_missing_or_unmatched_contact_timing(self):
        for key in ("pause_ms", "contact_ms"):
            self.requested[key] = 1100
            for duration in (None, 100):
                self.info[key] = duration
                with self.assertRaisesRegex(ValueError, key):
                    validate_setup(self.info, self.requested)
            self.info[key] = 1100
            validate_setup(self.info, self.requested)

    def test_requires_the_requested_blend_space_to_be_selected(self):
        for blending in ("linear", "perceptual"):
            self.requested["blending"] = blending
            self.info["state"]["commands"] = [dict(id="blend_" + space, selected=space == blending)
                                               for space in ("linear", "perceptual")]
            validate_setup(self.info, self.requested)
            for commands in ([], [dict(id="blend_" + blending, selected=False)],
                             [dict(id="blend_linear", selected=True), dict(id="blend_perceptual", selected=True)],
                             [dict(id="blend_" + ("linear" if blending == "perceptual" else "perceptual"), selected=True)]):
                with self.subTest(blending=blending, commands=commands):
                    info = copy.deepcopy(self.info)
                    info["state"]["commands"] = commands
                    with self.assertRaisesRegex(ValueError, "blending"):
                        validate_setup(info, self.requested)
        del self.info["state"]["commands"]
        with self.assertRaisesRegex(ValueError, "blending"):
            validate_setup(self.info, self.requested)


class CompletionWindowTests(unittest.TestCase):
    def test_input_completions_exclude_refinement_duplicates_hover_and_drain(self):
        report = {
            "motion": {"begin_ns": 0, "end_ns": 100, "active_intervals_ns": [[10, 40], [60, 90]]},
            "input_fields": ["event_ns", "worker_start_ns", "cpu_input_ns", "phase"],
            "inputs": [[11, 9, 2, 1], [21, 19, 2, 2], [29, 28, 1, 0], [39, 38, 1, 3], [61, 58, 3, 1], [86, 84, 2, 2]],
            "renderer_before": {"rows": [{"label": "Frames", "value": "0"}]},
            "completions": [[1, 12, 18, 1, 11], [2, 19, 20, 2, 11], [3, 22, 25, 2, 11],
                            [4, 26, 28, 3, 21], [5, 32, 38, 4, 21], [6, 39, 48, 5, 21],
                            [7, 62, 70, 6, 61], [8, 72, 78, 7, 61], [9, 87, 92, 8, 86]],
        }
        self.assertEqual(input_completions(report), [[report["completions"][0], report["completions"][3]],
                                                     [report["completions"][6]]])

    def test_queued_input_does_not_turn_idle_refinement_into_paint(self):
        report = {
            "motion": {"begin_ns": 0, "end_ns": 100},
            "input_fields": ["event_ns", "arrival_ns", "worker_start_ns", "cpu_input_ns", "phase"],
            "inputs": [[10, 10, 10, 1, 1], [40, 40, 40, 1, 1]],
            "renderer_before": {"rows": [{"label": "Frames", "value": "0"}]},
            "completions": [[0, 8, 9, 1, 0], [1, 12, 18, 2, 0], [2, 20, 30, 3, 10],
                            [3, 42, 44, 4, 10], [4, 50, 60, 5, 40], [5, 70, 80, 6, 40]],
        }
        self.assertEqual(input_completions(report), [[report["completions"][2], report["completions"][4]]])
        self.assertEqual([c["next_gpu_ms"] for c in contact_latencies(report)], [20e-6, 20e-6])

    def test_contacts_use_a_frame_queued_after_the_input_was_processed(self):
        report = {
            "motion": {"begin_ns": 10, "end_ns": 100},
            "input_fields": ["event_ns", "arrival_ns", "worker_start_ns", "cpu_input_ns", "phase", "pending_composition"],
            "inputs": [[9, 14, 20, 5, 1, 1], [30, 31, 32, 2, 2, 0], [80, 81, 85, 2, 1, 0]],
            "completions": [[1, 24, 40, 1, 9], [2, 26, 50, 2, 9]],
        }
        contacts = contact_latencies(report)
        self.assertEqual(contacts, [dict(pending_composition=True, queue_ms=6e-6, present_queued_ms=12e-6, next_gpu_ms=36e-6),
                                    dict(pending_composition=False, queue_ms=4e-6, present_queued_ms=None, next_gpu_ms=None)])

    def test_pauses_exclude_refinement_and_completion_after_each_contact(self):
        report = {
            "motion": {"begin_ns": 0, "end_ns": 100, "active_intervals_ns": [[10, 30], [60, 90]]},
            "display_before": {"submitted_frames": 0, "completed_frames": 0},
            "display_after_input": {"submitted_frames": 6, "completed_frames": 6},
            "renderer_before": {"rows": [{"label": "Frames", "value": "0"}]},
            "completions": [[1, 10, 20, 1], [2, 25, 32, 2], [3, 35, 45, 3],
                            [4, 65, 75, 4], [5, 85, 90, 5], [6, 95, 99, 6]],
        }
        result = completion_window(report)
        self.assertEqual(result["submitted"], 4)
        self.assertEqual(result["completed"], 2)
        self.assertEqual(result["pending_at_input_end"], 2)
        self.assertEqual(result["active_input_seconds"], 50 / 1e9)
        self.assertEqual(result["completed_per_s"], 2 / (50 / 1e9))
        self.assertEqual(result["accounting"], "active-input-intervals-nonempty")

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
