import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
import sqlite3
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("android_viewport_presents", Path(__file__).with_name("android-viewport-presents.py"))
viewport = importlib.util.module_from_spec(spec)
spec.loader.exec_module(viewport)


class WorkspaceWindowTest(unittest.TestCase):
    def run_data(self):
        return dict(begin_ns=1_000_000_000, end_ns=7_000_000_000,
                    begin_boot_ns=10_000_000_050, end_boot_ns=16_000_000_050,
                    begin_mono_after_boot_ns=1_000_000_100, end_mono_after_boot_ns=7_000_000_100,
                    trace_name="workspace-contact", run=0, thermal_before=0, thermal_after=0,
                    frame_rate=60, vsync_interval_p99_ms=16.667)

    def shown(self, begin=10_000_000_050):
        return {1: begin + 50_000_000, 2: begin + 200_000_000, 3: begin + 300_000_000,
                4: begin + 500_000_000, 5: begin + 500_000_000, 6: begin + 5_950_000_000}

    def test_normal_workspace_uses_explicit_boot_window_without_app_trace(self):
        with patch.object(viewport.subprocess, "run", side_effect=AssertionError("No async trace query expected")):
            result = viewport.workspace_summary("unused", "unused", self.run_data(), self.shown())
        self.assertEqual(result["window_source"], "explicit boot timestamps")
        self.assertEqual(result["full_contact_presents"], 5)
        self.assertEqual(result["presented_gap_p99_ms"], 200)
        self.assertEqual(len(result["actual_present_timestamps_ns"]), 3)
        self.assertEqual(result["onset_ms"], 50)

    def test_profile_keeps_async_window(self):
        output = SimpleNamespace(stdout='"ts","dur"\n10000000050,6000000000\n')
        with patch.object(viewport.subprocess, "run", return_value=output) as query:
            result = viewport.workspace_summary("processor", "trace", self.run_data(), self.shown(), profile=True)
        self.assertEqual(result["window_source"], "profile async trace")
        query.assert_called_once()

    def test_legacy_report_keeps_async_window(self):
        run = self.run_data()
        for key in ["begin_boot_ns", "end_boot_ns", "begin_mono_after_boot_ns", "end_mono_after_boot_ns"]:
            del run[key]
        output = SimpleNamespace(stdout='"ts","dur"\n10000000050,6000000000\n')
        with patch.object(viewport.subprocess, "run", return_value=output):
            result = viewport.workspace_summary("processor", "trace", run, self.shown())
        self.assertEqual(result["window_source"], "legacy async trace")

    def test_rejects_offset_change_outside_read_brackets(self):
        run = self.run_data()
        run["end_boot_ns"] += 1_000_000
        with self.assertRaisesRegex(ValueError, "offset changed"):
            viewport.workspace_boot_window(run)

    def test_rejects_partial_or_noninteger_clock_samples(self):
        for key, value in [("end_boot_ns", None), ("begin_ns", True), ("end_ns", 7e9)]:
            run = self.run_data()
            run[key] = value
            with self.assertRaises(ValueError):
                viewport.workspace_boot_window(run)

    def test_rejects_reversed_clock_bounds(self):
        run = self.run_data()
        run["end_ns"] = run["begin_ns"]
        with self.assertRaisesRegex(ValueError, "ordered"):
            viewport.workspace_boot_window(run)

    def test_normal_workspace_never_enables_app_atrace(self):
        self.assertNotIn("atrace_apps", viewport.trace_config("owned.app", workspace=True))
        self.assertIn('atrace_apps: "owned.app"', viewport.trace_config("owned.app", workspace=True, profile=True))

    def test_navigation_reports_full_contact_and_moving_canvas_presents(self):
        run = self.run_data()
        run.update(completions=[[1, 1_100_000_000], [2, 1_200_000_000]],
                   presentation=[[[1, 2_000_000, 1]]])
        result = viewport.summarize(run, self.shown())
        self.assertEqual(result["full_contact_presents"], 5)
        self.assertEqual(result["onset_ms"], 50)
        self.assertEqual(result["full_contact_longest_gap_ms"], 5450)
        self.assertEqual(len(result["actual_present_timestamps_ns"]), 3)
        self.assertEqual(result["viewport_gpu_p50_ms"], 2)
        self.assertEqual(result["submitted_per_s"], 2 / 6)

    def test_legacy_navigation_keeps_completion_attribution(self):
        run = self.run_data()
        del run["begin_mono_after_boot_ns"]
        del run["end_mono_after_boot_ns"]
        run.update(completions=[[1, 1_100_000_000], [2, 1_200_000_000]], presentation=[])
        result = viewport.summarize(run, self.shown())
        self.assertNotIn("window_source", result)
        self.assertAlmostEqual(result["presented_per_s"], 1e9 / 150_000_000)


class WorkspaceWorkloadTest(unittest.TestCase):
    def test_existing_bridge_paths_commands_and_reports_remain(self):
        for workload, method in [("gap", "columnBridgeGapFrameTiming"), ("panel", "columnBridgePanelFrameTiming")]:
            self.assertEqual(viewport.workspace_directory(workload), "workspace-bridge-benchmark")
            self.assertEqual(viewport.workspace_arguments(workload, "junit"),
                             ["-e", "class", f"art.capycanvas.AndroidWorkspacePerformanceTest#{method}"])
            self.assertEqual(viewport.workspace_arguments(workload, "self"), ["-e", "workspaceWorkload", workload])
            self.assertEqual(viewport.workspace_reports(workload, "owned", 3),
                             ["owned-0.json", "owned-1.json", "owned-2.json"])

    def test_column_uses_bridge_panel_runner_with_column_drag(self):
        self.assertEqual(viewport.workspace_arguments("column", "junit"),
                         ["-e", "class", "art.capycanvas.AndroidWorkspacePerformanceTest#columnBridgePanelFrameTiming", "-e", "columnDrag", "true"])
        self.assertEqual(viewport.workspace_arguments("column", "self"),
                         ["-e", "workspaceWorkload", "panel", "-e", "columnDrag", "true"])
        self.assertEqual(viewport.workspace_directory("column"), "workspace-bridge-benchmark")
        self.assertEqual(viewport.workspace_reports("column", "owned", 3), [f"owned-{index}.json" for index in range(3)])

    def test_nonbridge_commands_enable_tier_photo_and_select_matching_methods(self):
        methods = dict(resize="continuousResizeFrameTiming", drag="continuousDragFrameTiming",
                       wheel="colorWheelFrameTiming", **{"edit-wheel": "editColorWheelFrameTiming",
                       "color-overlap": "colorPanelOverlapFrameTiming"})
        for workload, method in methods.items():
            self.assertEqual(viewport.workspace_directory(workload), "workspace-tab-benchmark")
            self.assertEqual(viewport.workspace_arguments(workload, "junit"),
                             ["-e", "class", f"art.capycanvas.AndroidWorkspacePerformanceTest#{method}", "-e", "tierPhoto", "true"])
            with self.assertRaises(ValueError):
                viewport.workspace_arguments(workload, "self")

    def test_report_names_cover_every_source_mode_and_measured_repeat(self):
        for workload, modes in [("resize", ["brushes", "properties", "navigator", "toolbar"]),
                                ("drag", ["attached", "floating", "destination"]),
                                ("wheel", ["color-wheel"]), ("edit-wheel", ["color-editor-wheel"]),
                                ("color-overlap", ["color-overlap"])]:
            reports = viewport.workspace_reports(workload, "trial", 3)
            expected = {f"trial-{mode}-{source}-{run}.json" for mode in modes
                        for source in ["mouse", "touch"] for run in range(3)}
            self.assertEqual(set(reports), expected)
            self.assertEqual(len(reports), len(expected))
            self.assertFalse(any("--1.json" in filename for filename in reports))

    def test_mode_filter_keeps_both_input_sources_and_all_repeats(self):
        self.assertEqual(viewport.workspace_reports("resize", "trial", 2, "navigator"),
                         ["trial-navigator-mouse-0.json", "trial-navigator-mouse-1.json",
                          "trial-navigator-touch-0.json", "trial-navigator-touch-1.json"])
        with self.assertRaises(ValueError):
            viewport.workspace_reports("wheel", "trial", 3, "navigator")


class ChromeWorkloadTest(unittest.TestCase):
    def test_scroll_and_layer_reports_use_fixed_three_measured_repeats(self):
        self.assertEqual(viewport.chrome_reports("switcher-scroll", 7),
                         [f"{mode}-{run}.json" for mode in ["choices", "options"] for run in range(1, 4)])
        self.assertEqual(viewport.chrome_reports("filter-reorder", 7),
                         [f"layer-reorder-{run}.json" for run in range(1, 4)])
        args = viewport.chrome_arguments("filter-reorder", "light", 3)
        self.assertIn("layerRelationshipBenchmark", args)
        self.assertIn("layerBenchmarkTheme", args)
        self.assertIn("art.capycanvas.AndroidTitleBarTest#layerSwipeFrameTiming", args)

    def test_canvas_reports_select_native_ui_fixture_and_requested_repeat_count(self):
        args = viewport.chrome_arguments("ui-brush-flow-value", "dark", 4)
        self.assertIn("art.capycanvas.AndroidCanvasBarBenchmarkTest#canvasBarFrameTiming", args)
        self.assertIn("ui-brush-flow-value", args)
        self.assertIn("uiRepeats", args)
        self.assertEqual(viewport.chrome_reports("ui-brush-flow-value", 4),
                         [f"ui-brush-flow-value-{run}.json" for run in range(1, 5)])

    def test_discrete_windows_never_join_idle_gaps_or_report_sustained_rate(self):
        run = dict(active_windows=[dict(begin_boot_ns=1_000_000_000, end_boot_ns=1_200_000_000),
                                   dict(begin_boot_ns=9_000_000_000, end_boot_ns=9_200_000_000)])
        shown = {1: 1_010_000_000, 2: 1_030_000_000, 3: 5_000_000_000, 4: 9_010_000_000, 5: 9_040_000_000}
        result = viewport.chrome_summary(run, shown)
        self.assertFalse(result["sustained"])
        self.assertEqual(len(result["windows"]), 2)
        self.assertEqual([window["longest_gap_ms"] for window in result["windows"]], [20, 30])
        self.assertTrue(all("presented_per_s" not in window for window in result["windows"]))
        self.assertEqual([window["onset_ms"] for window in result["windows"]], [10, 10])

    def test_sustained_scroll_uses_bracketed_boot_window(self):
        fixture = WorkspaceWindowTest()
        result = viewport.chrome_summary(fixture.run_data(), fixture.shown())
        self.assertTrue(result["sustained"])
        self.assertAlmostEqual(result["windows"][0]["presented_per_s"], 2 / .3)

    def test_missing_clocks_empty_motion_and_invalid_windows_fail(self):
        for run in [{}, dict(WorkspaceWindowTest().run_data(), motion="layer-properties"), {"active_windows": []}, {"active_windows": [dict(begin_boot_ns=2, end_boot_ns=1)]}]:
            with self.assertRaises(ValueError):
                viewport.chrome_summary(run, {})


class AttributionTraceTest(unittest.TestCase):
    def test_rate_capture_stays_lean_and_attribution_adds_app_and_scheduler_sources(self):
        lean = viewport.trace_config("owned.app", workspace=True)
        diagnostic = viewport.trace_config("owned.app", workspace=True, attribution=True, gpu_renderstages=True)
        for source in ["linux.process_stats", "scan_all_processes_on_start: true", "flush_period_ms: 2000", "sched/sched_switch", "sched/sched_waking", "power/cpu_frequency", "gpu.renderstages",
                       'atrace_categories: "view"', 'atrace_categories: "input"', 'atrace_categories: "dalvik"',
                       'atrace_apps: "owned.app"']:
            self.assertNotIn(source, lean)
            self.assertIn(source, diagnostic)
        self.assertIn('android.surfaceflinger.frametimeline', diagnostic)
        self.assertNotIn('gpu.renderstages', viewport.trace_config("owned.app", attribution=True))

    def test_gpu_source_requires_positive_perfetto_capability_query(self):
        for response, expected in [(SimpleNamespace(returncode=0, stdout="gpu.renderstages", stderr=""), "supported"),
                                   (SimpleNamespace(returncode=0, stdout="linux.ftrace", stderr=""), "unsupported or unavailable"),
                                   (SimpleNamespace(returncode=1, stdout="gpu.renderstages", stderr="denied"), "unsupported or unavailable")]:
            with patch.object(viewport.subprocess, "run", return_value=response):
                result = viewport.attribution_capabilities(["adb", "-s", "owned"])
            self.assertEqual(result["gpu_renderstages"], expected)
            self.assertEqual(result["query_stderr"], response.stderr)

    def test_diagnostic_summary_cannot_claim_qualification(self):
        result = viewport.diagnostic_summary({"wheel": {"runs": []}}, {"gpu_renderstages": "unsupported or unavailable"})
        self.assertFalse(result["qualification"])
        self.assertIn("Diagnostic", result["measurement"])
        self.assertEqual(result["workloads"], {"wheel": {"runs": []}})


class ChromeCoverageTest(unittest.TestCase):
    def test_ui_labels_select_exact_fixture_and_three_repeats(self):
        for label in ["ui-layer-opacity-value", "ui-layers-scroll", "ui-brushes-scroll", "ui-filters-scroll",
                      "ui-zen-transition", "ui-bar-show-hide", "ui-bar-show-hide-zen"]:
            self.assertIn(label, viewport.chrome_arguments(label, "light", 3))
            self.assertEqual(viewport.chrome_reports(label, 3), [f"{label}-{index}.json" for index in range(1, 4)])
            with self.assertRaises(ValueError):
                viewport.chrome_arguments(label, "light", 2)

    def test_discrete_label_stays_diagnostic_even_in_long_window(self):
        fixture = WorkspaceWindowTest()
        run = fixture.run_data()
        run["label"] = "ui-zen-transition"
        run["active_windows"] = [dict(begin_boot_ns=run["begin_boot_ns"], end_boot_ns=run["end_boot_ns"])]
        result = viewport.chrome_summary(run, fixture.shown())
        self.assertFalse(result["sustained"])
        self.assertNotIn("presented_per_s", result["windows"][0])
        self.assertEqual(result["windows"][0]["qualification"], "diagnostic")
        self.assertEqual(len(result["windows"][0]["actual_present_timestamps_ns"]), 5)

    def test_workspace_preserves_fresh_geometry_rates(self):
        fixture = WorkspaceWindowTest()
        run = dict(fixture.run_data(), changed_bounds=120, changed_bounds_rate=20, drawn_revisions=100, drawn_update_rate=16.7)
        result = viewport.workspace_summary("unused", "unused", run, fixture.shown())
        for key in ["changed_bounds", "changed_bounds_rate", "drawn_revisions", "drawn_update_rate"]:
            self.assertEqual(result[key], run[key])

    def test_geometry_checks_actual_draw_times_and_observed_request_coverage(self):
        run = dict(draw_geometry_columns=["draw_mono_ns", "draw_boot_ns"],
                   draw_geometry=[[10, 100], [20, 110]], moving_draw_mono_ns=[20],
                   motion="layer-properties", transitions_ns=[9],
                   active_windows=[dict(begin_boot_ns=99, end_boot_ns=111)])
        self.assertEqual(viewport.geometry_completeness(run)["moving_draw_samples"], 1)
        run["active_windows"] = []
        with self.assertRaisesRegex(ValueError, "requests"):
            viewport.geometry_completeness(run)
        run["motion"] = "layer-swipe"
        run["draw_geometry"] = [[20, 110], [10, 100]]
        with self.assertRaisesRegex(ValueError, "ordered"):
            viewport.geometry_completeness(run)


class BatchAndFailureEvidenceTest(unittest.TestCase):
    def test_batch_omits_label_filter_and_collects_each_native_ui_report(self):
        args = viewport.chrome_arguments("ui-batch", "dark", 3)
        self.assertNotIn("labels", args)
        self.assertIn("ui", args)
        reports = viewport.chrome_reports("ui-batch", 3)
        expected = {filename for label, (selector, _, _, _) in viewport.CHROME_WORKLOADS.items()
                    if selector.startswith("AndroidCanvasBar") and label != "ui-batch"
                    for filename in viewport.chrome_reports(label, 3)}
        self.assertEqual(set(reports), expected)
        self.assertEqual(len(reports), len(expected))

    def test_failed_instrumentation_pulls_only_existing_specific_reports(self):
        commands = []
        def adb_call(command, **kwargs):
            commands.append(command)
            return SimpleNamespace(returncode=1 if "missing.json" in command[-1] else 0)
        with patch.object(viewport.subprocess, "run", side_effect=adb_call):
            retained = viewport.retain_failed_reports(["adb", "-s", "owned"], "/private/files", Path("output"),
                                                      ["complete-1.json", "missing.json", "readiness-failure.json"])
        self.assertEqual(retained, ["complete-1.json", "readiness-failure.json"])
        self.assertEqual(commands, [
            ["adb", "-s", "owned", "shell", "test", "-f", "/private/files/complete-1.json"],
            ["adb", "-s", "owned", "pull", "/private/files/complete-1.json", "output"],
            ["adb", "-s", "owned", "shell", "test", "-f", "/private/files/missing.json"],
            ["adb", "-s", "owned", "shell", "test", "-f", "/private/files/readiness-failure.json"],
            ["adb", "-s", "owned", "pull", "/private/files/readiness-failure.json", "output"]])


class PrimerEvidenceTest(unittest.TestCase):
    def test_workspace_primers_are_separate_from_rate_reports(self):
        self.assertEqual(viewport.workspace_reports("column", "trial", 3, warmup=True), ["trial--1.json"])
        self.assertEqual(viewport.workspace_reports("resize", "trial", 3, "navigator", warmup=True),
                         ["trial-navigator-mouse--1.json", "trial-navigator-touch--1.json"])
        for workload in viewport.WORKSPACE_WORKLOADS:
            self.assertTrue(set(viewport.workspace_reports(workload, "trial", 3, warmup=True)).isdisjoint(
                viewport.workspace_reports(workload, "trial", 3)))

    def test_canvas_failure_interactions_are_specific_expected_filenames(self):
        self.assertEqual(viewport.chrome_interactions("ui-bar-move", 3),
                         ["ui-bar-move-primer-interaction.json"] +
                         [f"ui-bar-move-contact-{index}-interaction.json" for index in range(1, 4)])
        batch = viewport.chrome_interactions("ui-batch", 3)
        self.assertEqual(len(batch), len(viewport.CHROME_WORKLOADS["ui-batch"][3]) * 4)
        self.assertTrue(all("*" not in filename and "/" not in filename for filename in batch))
        self.assertEqual(viewport.chrome_interactions("layer-swipe", 3), [])


class InstantSelectionObservationsTest(unittest.TestCase):
    def test_instant_selection_reports_observations_without_motion_rate_or_latency(self):
        fixture = WorkspaceWindowTest()
        run = dict(fixture.run_data(), motion="layer-selection",
                   transition_observations_boot_ns=[10_100_000_050, 10_100_000_050, 15_990_000_050])
        result = viewport.chrome_summary(run, fixture.shown())
        self.assertFalse(result["qualification"])
        self.assertFalse(result["sustained"])
        self.assertEqual(len(result["observations"]), 2)
        self.assertEqual(result["observations"][0]["next_window_present_boot_ns"], 10_200_000_050)
        self.assertIsNone(result["observations"][1]["next_window_present_boot_ns"])
        self.assertNotIn("presented_per_s", result)
        self.assertNotIn("windows", result)
        self.assertNotIn("latency_ms", result["observations"][0])

    def test_selection_markers_must_be_in_contact_integer_clock_domain(self):
        fixture = WorkspaceWindowTest()
        for markers in [[], [0], [17_000_000_000], [True], [10_100_000_050.0]]:
            run = dict(fixture.run_data(), motion="layer-selection", transition_observations_boot_ns=markers)
            with self.assertRaisesRegex(ValueError, "inside the contact"):
                viewport.chrome_summary(run, fixture.shown())


class OwnedWindowPresentsTest(unittest.TestCase):
    def test_workspace_query_owns_popup_process_and_retains_buffer_collisions(self):
        output = SimpleNamespace(stdout='"latch","presented_at"\n"latchBuffer owned.app/art.capycanvas.MainActivity#12 - 1",100\n"latchBuffer PopupWindow:abc#13 - 1",200\n')
        with patch.object(viewport.subprocess, "run", return_value=output) as process:
            result = viewport.presents("processor", "trace", "owned.app", workspace=True)
        self.assertEqual(len(result), 2)
        self.assertEqual(set(result.values()), {100, 200})
        query = process.call_args.args[0][-1]
        owned = query[:query.index("SELECT s.name latch")]
        database = sqlite3.connect(":memory:")
        database.execute("CREATE TABLE actual_frame_timeline_slice(upid INTEGER, layer_name TEXT)")
        database.executemany("INSERT INTO actual_frame_timeline_slice VALUES (?, ?)", [
            (3, "TX - owned.app/art.capycanvas.MainActivity#12"), (3, "TX - PopupWindow:abc#13"),
            (3, "SurfaceView[owned.app/art.capycanvas.MainActivity]#14"),
            (4, "other.app/art.capycanvas.MainActivity#15"), (4, "PopupWindow:other#16"),
            (5, "owned.app/art.capycanvas.MainActivityExtra#17"), (5, "PopupWindow:spoof#18")])
        layers = {row[0] for row in database.execute(owned + "SELECT layer FROM owned_layers")}
        self.assertEqual(layers, {"owned.app/art.capycanvas.MainActivity#12", "PopupWindow:abc#13"})
        self.assertIn("ancestor_slice(s.id)", query)
        self.assertIn("f.ts + f.dur presented_at", query)
        self.assertIn("SUBSTR(s.name, 13, LENGTH(o.layer)) = o.layer", query)

    def test_navigation_preserves_integer_canvas_buffer_identity(self):
        output = SimpleNamespace(stdout='"latch","presented_at"\n"latchBuffer SurfaceView[owned.app/art.capycanvas.MainActivity]#12 - 7",100\n')
        with patch.object(viewport.subprocess, "run", return_value=output) as process:
            result = viewport.presents("processor", "trace", "owned.app")
        self.assertEqual(result, {7: 100})
        self.assertNotIn("owned_layers", process.call_args.args[0][-1])


class BatchSubsetAndWindowTest(unittest.TestCase):
    def test_batch_subset_filters_arguments_reports_and_failure_evidence(self):
        labels = ["ui-menu-open", "ui-bar-move"]
        args = viewport.chrome_arguments("ui-batch", "light", 3, labels)
        self.assertIn("ui-menu-open,ui-bar-move", args)
        self.assertEqual(viewport.chrome_reports("ui-batch", 3, labels),
                         [f"{label}-{index}.json" for label in labels for index in range(1, 4)])
        self.assertEqual(len(viewport.chrome_interactions("ui-batch", 3, labels)), 8)
        for workload, selected in [("ui-batch", ["unknown"]), ("ui-bar-move", labels), ("ui-batch", [])]:
            with self.assertRaises(ValueError):
                viewport.chrome_arguments(workload, "light", 3, selected)

    def test_layer_row_opacity_is_in_native_ui_batch_with_target_window_metadata(self):
        self.assertIn("ui-layer-row-opacity", viewport.CHROME_WORKLOADS["ui-batch"][3])
        self.assertIn("ui-layer-row-opacity", viewport.chrome_arguments("ui-layer-row-opacity", "light", 3))
        self.assertEqual(viewport.chrome_reports("ui-layer-row-opacity", 3),
                         [f"ui-layer-row-opacity-{index}.json" for index in range(1, 4)])
        _, metadata = viewport.window_presents({}, {"latchBuffer Main#1 - 1": 100})
        self.assertIn("aggregate", metadata["window_selection"])

    def test_literal_target_popup_excludes_faster_main_root_and_similar_titles(self):
        shown = {"latchBuffer owned/MainActivity#1 - 1": 100,
                 "latchBuffer PopupWindow:a.b[1]#2 - 1": 200,
                 "latchBuffer PopupWindow:a.b[1]extra#3 - 1": 300,
                 "latchBuffer owned/MainActivity#1 - 2": 400}
        selected, diagnostics = viewport.window_presents({"fixture": {"window_title": "PopupWindow:a.b[1]"}}, shown)
        self.assertEqual(list(selected.values()), [200])
        self.assertEqual(len(diagnostics["owned_surface_present_counts"]), 3)
        with self.assertRaises(ValueError):
            viewport.window_presents({"window_title": "absent"}, shown)


class TransitionCompletionObservationTest(unittest.TestCase):
    def test_short_draw_window_can_observe_subsequent_present_before_next_request(self):
        run = dict(label="ui-menu-open", motion=dict(end_boot_ns=300), active_windows=[
            dict(begin_boot_ns=100, end_boot_ns=110), dict(begin_boot_ns=200, end_boot_ns=210)])
        result = viewport.chrome_summary(run, {1: 120, 2: 220})
        first = result["windows"][0]
        self.assertEqual(first["observed_draw_end_boot_ns"], 110)
        self.assertEqual(first["completion_present_observation_boot_ns"], 120)
        self.assertEqual(first["actual_present_timestamps_ns"], [120])
        self.assertFalse(result["sustained"])

    def test_next_request_or_contact_end_prevents_unrelated_later_present_assignment(self):
        for run, shown in [
            (dict(label="ui-menu-open", motion=dict(end_boot_ns=300), active_windows=[
                dict(begin_boot_ns=100, end_boot_ns=110), dict(begin_boot_ns=150, end_boot_ns=160)]), {1: 155}),
            (dict(label="ui-menu-open", motion=dict(end_boot_ns=115), active_windows=[
                dict(begin_boot_ns=100, end_boot_ns=110)]), {1: 120})]:
            with self.assertRaisesRegex(ValueError, "presentations unavailable"):
                viewport.chrome_summary(run, shown)


class CaptureBudgetTest(unittest.TestCase):
    def test_batch_budget_scales_with_selected_rows_and_bounds_trace_lifetime(self):
        subset = viewport.capture_budget_seconds("ui-batch", 3, 6000, ["ui-menu-open"])
        full = viewport.capture_budget_seconds("ui-batch", 3, 6000)
        self.assertGreater(full, subset)
        self.assertGreater(viewport.capture_budget_seconds("ui-batch", 5, 10000), full)
        self.assertIn(f"duration_ms: {int((full + 30) * 1000)}", viewport.trace_config("owned", duration_ms=int((full + 30) * 1000)))
        self.assertEqual(viewport.capture_budget_seconds("column", 3, 6000), 900)


class ScalarDrawGateTest(unittest.TestCase):
    def test_quantized_repeated_draws_do_not_count_as_fresh_values(self):
        run = dict(motion=dict(contact_begin_boot_ns=0, contact_end_boot_ns=6_000_000_000),
                   scalar_draw_samples=[dict(boot_ns=t, delivered_scalar="99", visual_signature=value)
                       for t, value in [(0,"1"),(1_000_000_000,"1"),(2_000_000_000,"2"),(3_000_000_000,"2"),(4_000_000_000,"1")]])
        result = viewport.scalar_draw_summary(run)
        self.assertEqual(result["fresh_draw_value_changes"], 2)
        self.assertEqual(result["fresh_draw_value_changes_per_s"], 1/3)
        self.assertEqual(result["fresh_draw_value_gap_p99_ms"], 2000)

    def test_old_raw_report_does_not_invent_fresh_value_rate(self):
        self.assertNotIn("fresh_draw_value_changes_per_s", viewport.scalar_draw_summary({}))

    def test_out_of_order_samples_fail(self):
        with self.assertRaisesRegex(ValueError, "monotonic"):
            viewport.scalar_draw_summary(dict(motion=dict(contact_begin_boot_ns=0, contact_end_boot_ns=6), scalar_draw_samples=[dict(boot_ns=2,visual_signature="1",delivered_scalar="1"),dict(boot_ns=1,visual_signature="2",delivered_scalar="2")]))

    def test_prefix_suffix_and_independent_scalar_revisits(self):
        rows = [(0,"0","0"),(10,"1","1"),(20,"2","1"),(30,"1","2"),(40,"3","3")]
        run = dict(motion=dict(contact_begin_boot_ns=10,contact_end_boot_ns=30),
                   scalar_draw_samples=[dict(boot_ns=t,delivered_scalar=value,visual_signature=visual) for t,value,visual in rows])
        result = viewport.scalar_draw_summary(run)
        self.assertEqual(result["delivered_scalar_changes"],2)
        self.assertEqual(result["fresh_draw_value_changes"],1)
        self.assertEqual(result["delivered_scalar_timestamps_boot_ns"],[20,30])
        self.assertEqual(result["scalar_observation_count"],3)
        self.assertEqual(result["scalar_observation_onset_ms"],0)
        self.assertEqual(result["scalar_observation_trailing_ms"],0)

    def test_input_positions_and_observer_overhead_remain_distinct_from_values(self):
        run = dict(motion=dict(contact_begin_boot_ns=0,contact_end_boot_ns=6_000_000_000),
                   scalar_draw_samples=[dict(boot_ns=1,delivered_scalar="1",visual_signature="1")],
                   scalar_observer_overhead=dict(samples=1,wall_ms=dict(p99=.1)),
                   scalar_input_samples=[dict(begin_boot_ns=t,end_boot_ns=t+1,x=x,y=0,action=2) for t,x in [(0,0),(1,0),(2,1),(3,0)]])
        result = viewport.scalar_draw_summary(run)
        self.assertEqual(result["injected_scalar_input_events"],4)
        self.assertEqual(result["injected_scalar_position_changes"],2)
        self.assertEqual(result["fresh_draw_value_changes"],0)
        self.assertEqual(result["scalar_observer_overhead"],run["scalar_observer_overhead"])
        run["scalar_input_samples"][0]["end_boot_ns"] = -1
        with self.assertRaisesRegex(ValueError,"injection intervals"):
            viewport.scalar_draw_summary(run)

    def test_shared_implied_quantization_excludes_turnaround_without_demand(self):
        run = dict(motion=dict(contact_begin_boot_ns=0,contact_end_boot_ns=1_000_000_000),
                   scalar_origin=dict(delivered_scalar=.5), scalar_replay=dict(status="exact"),
                   scalar_implied_samples=[dict(boot_ns=t,value=v) for t,v in [(0,.5),(100_000_000,.6),(800_000_000,.6),(900_000_000,.5)]])
        run["scalar_display_opportunities_boot_ns"]=[0,500_000_000,1_000_000_000]
        result = viewport.scalar_implied_summary(run,[110_000_000,910_000_000])
        self.assertEqual(result["implied_value_changes"],2)
        self.assertEqual(result["display_eligible_implied_changes"],2)
        self.assertEqual(result["delivered_to_implied_change_ratio"],1)
        self.assertEqual(result["demand_conditioned_delivery_gap_p99_ms"],10)
        run["scalar_replay"]["status"]="assumed"
        self.assertNotIn("implied_value_changes",viewport.scalar_implied_summary(run,[]))

    def test_automatic_shared_replay_uses_origin_and_actual_float_input(self):
        import json
        run = dict(scalar_origin=dict(boot_ns=0,delivered_scalar=.5,density=2,touch_slop_px=8,
                    resolved=dict(fill=.5),control_geometry=dict(screen_bounds=[0,0,100,20]),
                    mapping=dict(control=dict(kind="slider"),operation="scrub",host_control="material_number",callback_float32=True)),
                   scalar_input_samples=[dict(begin_boot_ns=t,action=a,injected_screen_x=50,injected_screen_y=y) for t,a,y in [(1,0,100),(2,2,96),(3,2,80)]])
        def resolver(command, **kwargs):
            rows=[json.loads(line) for line in kwargs["input"].splitlines()]
            self.assertEqual(rows[0]["request"]["operation"],dict(type="format"))
            self.assertEqual(rows[1]["request"]["operation"],dict(type="scrub",origin=.5,pixels=10,fine=False))
            return SimpleNamespace(stdout="\n".join(json.dumps(dict(boot_ns=r["boot_ns"],resolved=dict(value=.525),float_value=.5249999761581421))for r in rows))
        with patch.object(viewport.subprocess,"run",side_effect=resolver):
            viewport.replay_numeric(run,"native-replay")
        self.assertEqual(run["scalar_replay"]["status"],"exact")
        self.assertEqual(run["scalar_implied_samples"][-1]["value"],.5249999761581421)
        viewport.replay_numeric({},"unused")

    def test_shared_replay_errors_do_not_qualify(self):
        run=dict(scalar_origin=dict(boot_ns=0,delivered_scalar=.5,density=1,touch_slop_px=0,resolved=dict(fill=.5),
                 control_geometry=dict(screen_bounds=[0,0,100,20]),mapping=dict(control={},operation="scrub",host_control="material_number")),
                 scalar_input_samples=[dict(begin_boot_ns=0,action=0,injected_screen_x=0,injected_screen_y=0)])
        with patch.object(viewport.subprocess,"run",return_value=SimpleNamespace(stdout='{"boot_ns":0,"error":"invalid"}')):
            with self.assertRaisesRegex(ValueError,"resolver failed"):
                viewport.replay_numeric(run,"native-replay")

    def test_material_replay_keeps_initial_signed_slop_on_turnaround(self):
        import json
        run=dict(scalar_origin=dict(boot_ns=0,delivered_scalar=.5,density=1,touch_slop_px=5,resolved=dict(fill=.5),
                 control_geometry=dict(screen_bounds=[0,0,200,20]),mapping=dict(control={},operation="position",host_control="material_number",show_thumb=False,track_inset_dp=0)),
                 scalar_input_samples=[dict(begin_boot_ns=i,action=a,injected_screen_x=x,injected_screen_y=0) for i,(a,x) in enumerate([(0,60),(2,64),(2,65),(2,72),(2,75),(2,50),(1,50)],1)])
        def resolver(command,**kwargs):
            rows=[json.loads(line)for line in kwargs["input"].splitlines()]
            positions=[r["request"]["operation"]["position"]for r in rows[1:]]
            for actual,expected in zip(positions,[.3,.335,.35,.225]): self.assertAlmostEqual(actual,expected,places=6)
            self.assertEqual(len(positions),4)
            return SimpleNamespace(stdout="\n".join(json.dumps(dict(boot_ns=r["boot_ns"],resolved=dict(value=.5)))for r in rows))
        with patch.object(viewport.subprocess,"run",side_effect=resolver): viewport.replay_numeric(run,"native-replay")

    def test_toolbar_replay_routes_down_to_shared_bookmarks_and_moves_to_numeric(self):
        import json
        run=dict(scalar_origin=dict(boot_ns=0,delivered_scalar=.5,density=1,touch_slop_px=5,resolved=dict(fill=.5),
                 control_geometry=dict(screen_bounds=[0,0,116,20]),mapping=dict(control={},operation="position",host_control="toolbar_slider",axis="horizontal",callback_float32=True,
                 toolbar_control=dict(kind="brush_size_slider"),bookmark_values=[1,10])),
                 scalar_input_samples=[dict(begin_boot_ns=i,action=a,injected_screen_x=x,injected_screen_y=10) for i,(a,x) in enumerate([(0,58),(2,59),(2,68),(1,68)],1)])
        def resolver(command,**kwargs):
            rows=[json.loads(line)for line in kwargs["input"].splitlines()]
            self.assertEqual(len(rows),3)
            self.assertEqual(rows[1]["toolbar_request"]["position"],.5)
            self.assertEqual(rows[1]["toolbar_request"]["travel"],100)
            self.assertAlmostEqual(rows[2]["request"]["operation"]["position"],.6,places=6)
            self.assertTrue(rows[2]["previous_float_value"])
            return SimpleNamespace(stdout="\n".join(json.dumps(dict(boot_ns=r["boot_ns"],resolved=dict(value=.5),float_value=.5))for r in rows))
        with patch.object(viewport.subprocess,"run",side_effect=resolver): viewport.replay_numeric(run,"native-replay")

    def test_down_boundary_normalization_preserves_raw_and_initial_demand(self):
        import json
        run=dict(motion=dict(contact_begin_boot_ns=10,contact_end_boot_ns=100),
                 scalar_origin=dict(boot_ns=0,delivered_scalar=.5,density=1,touch_slop_px=5,resolved=dict(fill=.5),
                 control_geometry=dict(screen_bounds=[0,0,116,20]),mapping=dict(control={},operation="position",host_control="toolbar_slider",axis="horizontal",callback_float32=True,
                 toolbar_control=dict(kind="brush_size_slider"),bookmark_values=[1])),
                 scalar_input_samples=[dict(begin_boot_ns=9,end_boot_ns=11,action=0,injected_screen_x=58,injected_screen_y=10),
                                       dict(begin_boot_ns=20,end_boot_ns=22,action=2,injected_screen_x=68,injected_screen_y=10)])
        def resolver(command,**kwargs):
            rows=[json.loads(line)for line in kwargs["input"].splitlines()]
            self.assertEqual([r["boot_ns"]for r in rows],[0,10,20])
            return SimpleNamespace(stdout="\n".join(json.dumps(dict(boot_ns=r["boot_ns"],resolved=dict(value=v),float_value=v))for r,v in zip(rows,[.5,1,1])))
        with patch.object(viewport.subprocess,"run",side_effect=resolver): viewport.replay_numeric(run,"native-replay")
        self.assertEqual(run["scalar_input_samples"][0]["begin_boot_ns"],9)
        self.assertEqual(len(run["scalar_replay"]["down_boundary_normalizations"]),1)
        result=viewport.scalar_implied_summary(run,[30])
        self.assertEqual(result["implied_value_changes"],1)
        self.assertEqual(result["demand_conditioned_delivery_gap_p99_ms"],.00002)


if __name__ == "__main__":
    unittest.main()
