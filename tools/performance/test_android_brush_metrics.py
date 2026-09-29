import copy
import unittest
from android_brush_metrics import completion_window, validate_setup


class SetupTests(unittest.TestCase):
    def setUp(self):
        self.requested = dict(preset=1, brush_size=1024, mode="constant", prediction=True,
                              speed=1, duration_ms=5000, repeats=3, radii=[240, 140],
                              photo_layers=4, horizon=16, zoom=1)
        self.info = {key: self.requested[key] for key in
                     ("preset", "brush_size", "mode", "prediction", "speed", "duration_ms", "repeats")}
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
                              (lambda d: d["state"]["settings"].update(feedback=False), "feedback")]:
            with self.subTest(field=field):
                info = copy.deepcopy(self.info)
                change(info)
                with self.assertRaisesRegex(ValueError, field):
                    validate_setup(info, self.requested)

    def test_rejects_reusing_a_completed_run_for_different_settings(self):
        self.requested["duration_ms"] = 10000
        with self.assertRaisesRegex(ValueError, "duration_ms"):
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
