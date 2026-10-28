import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("android_bubbles", Path(__file__).with_name("android-bubbles.py"))
bubbles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bubbles)


class BubblePresentsTest(unittest.TestCase):
    def write_motion_fixture(self, directory):
        contacts = [dict(run=i, warmup=i == 0, begin_ns=i * 10_000_000_000, end_ns=i * 10_000_000_000 + 6_000_000_000,
                         distinct_centers=[20, 2, 3, 4, 5]) for i in range(4)]
        directory.joinpath("result.json").write_text(json.dumps(dict(contacts=contacts)))
        times = [c["begin_ns"] + n * 10_000_000 for c in contacts for n in range(601)]
        capture = dict(surface="owned.app/MainActivity#1", samples=[dict(text="10000000\n" + "\n".join(f"1 {t} 3" for t in times))])
        directory.joinpath("surface-latency.json").write_text(json.dumps(capture))
        return contacts, capture

    def test_factory_display_settings_are_recorded(self):
        self.assertEqual(bubbles.factory_display_settings("Physical size: 1440x2200\n", "Physical density: 240\n", "1.0\n"),
                         dict(size=[1440, 2200], density=240, font_scale=1))

    def test_display_overrides_are_rejected_even_when_matching_physical_values(self):
        for size, density in [("Physical size: 1440x2200\nOverride size: 1440x2200", "Physical density: 240"),
                              ("Physical size: 1440x2200", "Physical density: 240\nOverride density: 280")]:
            with self.subTest(size=size, density=density), self.assertRaisesRegex(ValueError, "overrides"):
                bubbles.factory_display_settings(size, density, "1")

    def test_nonfactory_or_malformed_display_settings_are_rejected(self):
        for size, density, font in [("", "Physical density: 240", "1"),
                                    ("Physical size: 1440x0", "Physical density: 240", "1"),
                                    ("Physical size: 1440x2200", "Physical density: null", "1"),
                                    ("Physical size: 1440x2200", "Physical density: 240", ".85"),
                                    ("Physical size: 1440x2200", "Physical density: 240", "null"),
                                    ("Physical size: 1440x2200", "Physical density: 240", "nan")]:
            with self.subTest(size=size, density=density, font=font), self.assertRaises(ValueError):
                bubbles.factory_display_settings(size, density, font)

    def test_surface_names_accept_android_16_requested_state_wrappers(self):
        name = "owned.app/art.capycanvas.MainActivity#12"
        self.assertEqual(bubbles.surface_names(name + "\nRequestedLayerState{" + name + " parentId=9 z=1}\n"), [name, name])

    def test_pending_slots_and_duplicates_do_not_count_as_presents(self):
        self.assertEqual(bubbles.latency("11111111\n0 0 0\n1 9223372036854775807 3\n1 123 3\n2 123 4\n"), (11111111, [123]))
        with self.assertRaises(ValueError):
            bubbles.latency("11111111\n1 2\n")

    def test_private_activity_identity_uses_the_resumed_exact_component(self):
        text = "topResumedActivity=ActivityRecord{abc u0 owned.app/art.capycanvas.MainActivity t123}"
        self.assertEqual(bubbles.activity_identity(text, "owned.app"), ("abc", 123))
        with self.assertRaises(ValueError):
            bubbles.activity_identity(text, "other.app")

    def test_short_palette_diagnostic_retains_poses_without_fps(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            contact = dict(diagnostic=True, warmup=False, begin_ns=0, end_ns=20_000_000,
                           moving_windows_ns=[[0, 20_000_000]], drawn_centers=[[0] + [0] * 10, [10_000_000, 1] + [0] * 9])
            directory.joinpath("result.json").write_text(json.dumps(dict(contacts=[contact])))
            directory.joinpath("instrumentation.log").write_text("OK (1 test)")
            directory.joinpath("surface-latency.json").write_text(json.dumps(dict(surface="owned", samples=[dict(text="10000000\n1 5000000 3\n1 15000000 3\n")])))
            result = bubbles.report(directory)
            self.assertNotIn("presented_hz", result)
            self.assertEqual(result["transitions"][0]["distinct_drawn_centers"], [2, 1, 1, 1, 1])
            self.assertEqual(result["transitions"][0]["actual_present_timestamps_ns"], [5_000_000, 15_000_000])

    def test_long_gaps_are_preserved(self):
        result = bubbles.summarize([0, 10_000_000, 20_000_000, 120_000_000])
        self.assertEqual(result["gap_p99_ms"], 100)
        self.assertEqual(result["presented_hz"], 25)

    def test_report_excludes_warmup_and_rejects_ring_rollover(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            contacts, capture = self.write_motion_fixture(directory)
            result = bubbles.report(directory)
            self.assertEqual([c["run"] for c in result["contacts"]], [1, 2, 3])
            self.assertEqual(result["contacts"][0]["presented_hz"], 100)
            contacts[1]["reset_end_ns"] = contacts[1]["begin_ns"] + 1
            directory.joinpath("result.json").write_text(json.dumps(dict(contacts=contacts)))
            with self.assertRaisesRegex(ValueError, "reset overlaps"):
                bubbles.report(directory)
            del contacts[1]["reset_end_ns"]
            directory.joinpath("result.json").write_text(json.dumps(dict(contacts=contacts)))
            capture["samples"] = [dict(text="10000000\n1 11000000000 3\n"), dict(text="10000000\n1 14000000000 3\n")]
            directory.joinpath("surface-latency.json").write_text(json.dumps(capture))
            with self.assertRaisesRegex(ValueError, "ring rollover"):
                bubbles.report(directory)

    def test_report_requires_recorded_factory_settings_for_qualification(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.write_motion_fixture(directory)
            self.assertTrue(bubbles.report(directory)["qualification"].startswith("Diagnostic:"))
            settings = dict(size="Physical size: 1440x2200", density="Physical density: 240", font_scale="1")
            for stage in ["before", "after"]:
                directory.joinpath(f"display-settings-{stage}.json").write_text(json.dumps(settings))
            self.assertTrue(bubbles.report(directory)["qualification"].startswith("Factory display settings verified"))
            settings["density"] += "\nOverride density: 280"
            directory.joinpath("display-settings-after.json").write_text(json.dumps(settings))
            with self.assertRaisesRegex(ValueError, "overrides"):
                bubbles.report(directory)

    def test_tier_gate_rejects_real_double_vsync_gaps_at_a_passing_rate(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            contacts, _ = self.write_motion_fixture(directory)
            times = []
            for contact in contacts:
                contact["display_hz"] = 89.58
                timestamp = contact["begin_ns"]
                for index in range(520):
                    times.append(timestamp)
                    timestamp += 22_333_000 if index % 26 == 0 else 11_163_000
            directory.joinpath("result.json").write_text(json.dumps(dict(contacts=contacts)))
            capture = dict(surface="owned", samples=[dict(text="11163000\n" + "\n".join(f"1 {timestamp} 3" for timestamp in times))])
            directory.joinpath("surface-latency.json").write_text(json.dumps(capture))
            directory.joinpath("instrumentation.log").write_text("OK (1 test)")
            settings = dict(size="Physical size: 1440x2200", density="Physical density: 240", font_scale="1")
            for stage in ["before", "after"]:
                directory.joinpath(f"display-settings-{stage}.json").write_text(json.dumps(settings))
            result = bubbles.report(directory, 90)
            self.assertFalse(result["target_met"])
            self.assertTrue(all(contact["presented_hz"] > 85.5 for contact in result["contacts"]))
            self.assertTrue(all(contact["gap_p99_ms"] == 22.333 for contact in result["contacts"]))
            self.assertEqual(result["target"]["maximum_gap_p99_ms"], 22.2)
            result = bubbles.report(directory, 60)
            self.assertTrue(result["target_met"])
            directory.joinpath("display-settings-after.json").unlink()
            self.assertFalse(bubbles.report(directory, 60)["target_met"])

    def test_hdr_intensity_requires_visible_values_instead_of_neighbor_motion(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            contacts = [dict(run=i, warmup=False, workload="hdr-intensity", begin_ns=i * 10_000_000_000,
                             end_ns=i * 10_000_000_000 + 6_000_000_000, distinct_centers=[1] * 5,
                             distinct_intensities=24) for i in range(3)]
            directory.joinpath("result.json").write_text(json.dumps(dict(contacts=contacts)))
            times = [c["begin_ns"] + n * 10_000_000 for c in contacts for n in range(601)]
            directory.joinpath("surface-latency.json").write_text(json.dumps(dict(surface="owned",
                samples=[dict(text="10000000\n" + "\n".join(f"1 {t} 3" for t in times))])))
            self.assertEqual(len(bubbles.report(directory)["contacts"]), 3)
            contacts[1]["distinct_intensities"] = 1
            directory.joinpath("result.json").write_text(json.dumps(dict(contacts=contacts)))
            with self.assertRaisesRegex(ValueError, "visibly change intensity"):
                bubbles.report(directory)


if __name__ == "__main__":
    unittest.main()
