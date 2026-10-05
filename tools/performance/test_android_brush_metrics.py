import copy
import csv
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from android_brush_metrics import completion_window, contact_latencies, input_completions, object_completions, validate_setup


class ReportTests(unittest.TestCase):
    def test_affine_retries_without_submissions_preserve_zero_fresh_poses(self):
        info = dict(preset=1, brush_size=1024, mode="object-affine", prediction=True,
                    speed=1, repeats=1)
        display = dict(submitted_frames=1, completed_frames=1)
        renderer = dict(rows=[dict(label="Frames", value="1"), dict(label="Dabs", value="0")],
                        gpu_samples=[], resident_bytes=0)
        report = dict(motion=dict(begin_ns=10, end_ns=100, begin_boot_ns=10, end_boot_ns=100,
                                  object_edits=[[11, 15, 2]], injected=[]),
                      frame_fields=["start_ns", "vsync_ns", "queue_present_ns", "cpu_callback_ns", "owner_thread_cpu_ns"],
                      frames=[[20, 20, 0, 3, 2]], inputs=[], input_fields=[],
                      completions=[[2, 110, 120, 2, 0, 2]], presentation=[],
                      display_before=display, display_after_input=display,
                      renderer_before=renderer, renderer_after=renderer,
                      resources_after=dict(process_mappings=0), settled_ns=125)
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            for suffix, value in [("info", info), ("0", report), ("complete", {})]:
                (directory / f"retry-{suffix}.json").write_text(json.dumps(value))
            result = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name("android-brush-report.py")),
                                     str(directory)], capture_output=True, text=True, check=True)
            self.assertIn("CPU=not sampled", result.stdout)
            summary = json.loads((directory / "summary.json").read_text())[0]
            self.assertEqual(summary["object_completed_per_s_median"], 0)
            self.assertEqual(summary["runs"][0]["object_completion_gap_ms"], dict(n=0))
            with (directory / "summary.csv").open() as output:
                row = next(csv.DictReader(output))
            self.assertEqual(row["cpu_callback_p50_ms"], "")
            self.assertEqual(row["update_start_gap_p99_ms"], "")
            self.assertEqual(row["fresh_object_gap_p99_ms"], "")


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

    def test_reduced_color_mode_requires_the_live_layer_to_match(self):
        self.requested["color_mode"] = "grayscale"
        self.info.update(color_mode="grayscale")
        self.info["state"]["layer_tools"] = {"color_mode": {"value": "Grayscale"}}
        validate_setup(self.info, self.requested)
        self.info["state"]["layer_tools"]["color_mode"]["value"] = "Full color"
        with self.assertRaisesRegex(ValueError, "layer_color_mode"):
            validate_setup(self.info, self.requested)

    def test_properties_color_mode_requires_the_live_choice_to_match(self):
        self.requested["color_mode"] = "grayscale"
        self.info.update(color_mode="grayscale")
        control = dict(key="color_mode", kind=dict(kind="choice", options=["Full color", "Grayscale", "Two-tone (black & white)"]),
                       value=dict(kind="choice", value=1))
        self.info["state"]["layer_properties"] = dict(controls=[control])
        validate_setup(self.info, self.requested)
        for index in (0, 2, -1, 3, None, True):
            with self.subTest(index=index):
                control["value"]["value"] = index
                with self.assertRaisesRegex(ValueError, "layer_color_mode"):
                    validate_setup(self.info, self.requested)

    def test_attachment_workloads_require_the_authored_order_and_parameters(self):
        self.requested.update(photo_layers=1, paint_layer_index=0, effect_radius=8)
        for workload in ("clipped", "blurred-base"):
            effect = workload == "blurred-base"
            self.requested["workload"] = workload
            self.info.update(workload=workload, attachment_fixture=dict(paint=3, base=2, effect=4 if effect else None,
                             effect_id="gaussian_blur" if effect else None, sigma=8 if effect else None))
            self.info["state"]["layers"] = [dict(id=3, clipped=True, selected=True)] + (
                [dict(id=4, clipped=True)] if effect else []) + [dict(id=2, clipped=False), dict(id=1, clipped=False)]
            validate_setup(self.info, self.requested)
            for change, field in [(lambda d: d.pop("workload"), "workload"),
                                  (lambda d: d.pop("attachment_fixture"), "attachment_handles"),
                                  (lambda d: d["state"]["layers"][0].update(clipped=False), "attached_rows"),
                                  (lambda d: d["state"]["layers"].reverse(), "attachment_order")]:
                with self.subTest(workload=workload, field=field):
                    info = copy.deepcopy(self.info)
                    change(info)
                    with self.assertRaisesRegex(ValueError, field):
                        validate_setup(info, self.requested)
            if effect:
                for change, field in [(lambda d: d["attachment_fixture"].update(effect_id="exposure"), "effect_id"),
                                      (lambda d: d["attachment_fixture"].update(sigma=3), "effect_radius"),
                                      (lambda d: d["attachment_fixture"].update(effect=3), "attachment_handles")]:
                    info = copy.deepcopy(self.info)
                    change(info)
                    with self.assertRaisesRegex(ValueError, field):
                        validate_setup(info, self.requested)

    def test_requires_the_paint_layer_at_the_requested_position(self):
        self.requested["paint_layer_index"] = 3
        self.info["state"]["layers"] = [dict(selected=i == 3) for i in range(6)]
        validate_setup(self.info, self.requested)
        for selected in ([0], [], [0, 3]):
            self.info["state"]["layers"] = [dict(selected=i in selected) for i in range(6)]
            with self.assertRaisesRegex(ValueError, "paint_layer_index"):
                validate_setup(self.info, self.requested)

    def test_rejects_missing_or_unmatched_contact_timing(self):
        for key in ("pause_ms", "contact_ms", "settle_delay_ms"):
            self.requested[key] = 1100
            for duration in (None, 100):
                self.info[key] = duration
                with self.assertRaisesRegex(ValueError, key):
                    validate_setup(self.info, self.requested)
            self.info[key] = 1100
            validate_setup(self.info, self.requested)

    def test_requires_the_active_canvas_and_navigation_handoff(self):
        self.requested.update(canvas=[4248, 2832], navigation_between_strokes=True, navigation_settle_ms=0)
        self.info.update(navigation_between_strokes=True, navigation_settle_ms=0)
        self.info["state"]["tabs"] = [dict(active=True, width=4248, height=2832)]
        validate_setup(self.info, self.requested)
        for change, field in [(lambda d: d["state"]["tabs"][0].update(width=6000), "canvas"),
                              (lambda d: d["state"]["tabs"][0].update(active=False), "canvas"),
                              (lambda d: d.pop("navigation_between_strokes"), "navigation_between_strokes"),
                              (lambda d: d.update(navigation_settle_ms=750), "navigation_settle_ms")]:
            with self.subTest(field=field):
                info = copy.deepcopy(self.info)
                change(info)
                with self.assertRaisesRegex(ValueError, field):
                    validate_setup(info, self.requested)

    def test_object_workloads_require_actual_image_identity_sharing(self):
        self.requested.update(workload="objects",photo_layers=2,image_count=4,image_sources="shared",paint_layer_index=1)
        self.info.update(workload="objects",object_fixture=[dict(image="same",source_owner=0,paint_base_image_shared=True,paint_base_source_shared=True) for _ in range(4)])
        self.info["state"]["layers"] = [dict(selected=i==1) for i in range(4)]
        validate_setup(self.info,self.requested)
        missing_owner = copy.deepcopy(self.info)
        for image in missing_owner["object_fixture"]:
            image.pop("source_owner")
        with self.assertRaisesRegex(ValueError,"source_owners_known"):
            validate_setup(missing_owner,self.requested)
        self.requested["image_sources"] = "unshared"
        with self.assertRaisesRegex(ValueError,"image_identities"):
            validate_setup(self.info,self.requested)
        self.info["object_fixture"] = [dict(image=str(i),source_owner=i,paint_base_image_shared=False,paint_base_source_shared=False) for i in range(4)]
        validate_setup(self.info,self.requested)
        self.requested["workload"] = "objects-effects"
        self.info["workload"] = "objects-effects"
        self.info["state"]["layers"] = [dict(selected=i==2) for i in range(5)]
        validate_setup(self.info,self.requested)
        self.info["object_fixture"].pop()
        with self.assertRaisesRegex(ValueError,"image_count"):
            validate_setup(self.info,self.requested)

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
    def test_object_completions_require_evaluated_new_poses_inside_motion(self):
        report = {
            "motion":{"begin_ns":10,"end_ns":100,"object_edits":[[11,14,4],[20,23,5],[40,42,6],[70,73,7]]},
            "completions":[[1,15,18,1,0,3],[2,16,19,2,0,4],[3,24,28,3,0,4],
                           [4,30,35,4,0,5],[5,43,45,5,0,6],[6,47,55,6,0,6],[7,74,102,7,0,7]],
        }
        self.assertEqual([row[5] for row,edit in object_completions(report)],[4,5,6])
        report["completions"] = [row[:5] for row in report["completions"]]
        self.assertEqual(object_completions(report),[])

    def test_evaluated_pose_can_submit_before_instrumentation_receives_edit_reply(self):
        report = {"motion":{"begin_ns":10,"end_ns":100,"object_edits":[[11,30,4]]},
                  "completions":[[1,18,24,1,0,4],[2,32,38,2,0,4]]}
        self.assertEqual(object_completions(report),[(report["completions"][0],report["motion"]["object_edits"][0])])

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
        self.assertEqual(result["completion_gaps_ms"], [])

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
        self.assertEqual(result["completion_gaps_ms"], [60e-6])


if __name__ == "__main__":
    unittest.main()
