"""Reject missing, mismatched and unmeasured phone allocations."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

from phone_audit import compare, compare_matrix, index


class PhoneAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.gtk = {"schema_version": 1, "host": "gtk", "state": "root", "viewport": [412, 891],
            "dpr": 1, "theme": "light", "text_scale": 1, "preset": "Sketch", "phone_active": True,
            "status": "captured", "missing": [], "elements": [
                {"id": "chip-tools", "role": "chip", "bounds": [8, 8, 396, 48], "visible": True}]}
        self.web = copy.deepcopy(self.gtk)
        self.web["host"] = "web"

    def result(self):
        paths = [self.root / "gtk.json", self.root / "web.json"]
        for path, value in zip(paths, [self.gtk, self.web]):
            path.write_text(json.dumps(value))
        return compare(*paths)

    def test_one_pixel_is_the_limit(self):
        self.web["elements"][0]["bounds"][0] += 1
        self.assertTrue(self.result()["passed"])
        self.web["elements"][0]["bounds"][0] += .01
        result = self.result()
        self.assertFalse(result["passed"])
        self.assertEqual(result["allocation_differences"][0]["id"], "chip-tools")

    def test_missing_elements_cannot_pass(self):
        self.web["elements"] = []
        self.assertEqual(self.result()["missing"], ["chip-tools"])
        self.assertFalse(self.result()["passed"])

    def test_unavailable_or_unmatched_state_cannot_pass(self):
        self.web["status"] = "gated"
        self.assertFalse(self.result()["passed"])
        self.web["status"] = "captured"
        self.web["theme"] = "dark"
        self.assertEqual(self.result()["metadata_mismatches"], ["theme"])

    def test_duplicate_and_nonfinite_bounds_are_rejected(self):
        self.web["elements"].append(copy.deepcopy(self.web["elements"][0]))
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            self.result()
        self.web["elements"].pop()
        self.web["elements"][0]["bounds"][0] = float("nan")
        with self.assertRaisesRegex(ValueError, "Invalid bounds"):
            self.result()

    def test_visibility_is_compared_even_when_bounds_match(self):
        self.web["elements"][0]["visible"] = False
        self.assertFalse(self.result()["passed"])

    def test_unmapped_widgets_have_no_fabricated_allocation(self):
        for value in (self.gtk, self.web):
            value["elements"][0].update(visible=False, bounds=None)
        self.assertTrue(self.result()["passed"])
        self.web["elements"][0]["visible"] = True
        with self.assertRaisesRegex(ValueError, "Invalid bounds"):
            self.result()

    def test_required_capture_metadata_cannot_be_omitted(self):
        del self.web["dpr"]
        with self.assertRaisesRegex(ValueError, "Missing metadata dpr"):
            self.result()

    def test_zero_sized_or_unknown_visibility_cannot_qualify(self):
        self.web["elements"][0]["bounds"][2] = 0
        with self.assertRaisesRegex(ValueError, "Empty visible allocation"):
            self.result()
        self.web["elements"][0]["visible"] = None
        with self.assertRaisesRegex(ValueError, "Invalid element identity/visibility"):
            self.result()

    def test_native_window_elements_are_reported_separately(self):
        self.gtk["elements"].append({"id": "title", "role": "window-move", "bounds": [64, 0, 300, 24], "visible": True})
        result = self.result()
        self.assertTrue(result["passed"])
        self.assertEqual(result["host_elements"]["gtk"][0]["id"], "title")
        self.web["elements"][0]["bounds"][2] += 2
        self.assertFalse(self.result()["passed"])

    def test_cli_writes_native_size_sheet_and_rejects_wrong_capture_size(self):
        for name, value in (("gtk", self.gtk), ("web", self.web)):
            value["viewport"] = [3, 2]
            (self.root / f"{name}.allocations.json").write_text(json.dumps(value))
            Image.new("RGB", (3, 2), (20, 40, 60)).save(self.root / f"{name}.png")
        command = [sys.executable, str(Path(__file__).with_name("phone_audit.py")),
            str(self.root / "gtk.allocations.json"), str(self.root / "web.allocations.json"),
            "--output", str(self.root / "result")]
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads(result.stdout)["passed"])
        with Image.open(self.root / "result/sheet.png") as sheet:
            self.assertEqual(sheet.size, (9, 2))
        for name, value in (("gtk", self.gtk), ("web", self.web)):
            value["viewport"] = [412, 891]
            (self.root / f"{name}.allocations.json").write_text(json.dumps(value))
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("do not match viewport/DPR", result.stderr)

    def test_matrix_matches_numeric_metadata_and_rejects_missing_states(self):
        for name, value in (("gtk", self.gtk), ("web", self.web)):
            directory = self.root / name
            directory.mkdir()
            value["viewport"] = [3, 2]
            (directory / "one.allocations.json").write_text(json.dumps(value))
            Image.new("RGB", (3, 2), (20, 40, 60)).save(directory / "one.png")
        self.web["text_scale"] = 1.0
        (self.root / "web/one.allocations.json").write_text(json.dumps(self.web))
        result = compare_matrix(self.root / "gtk", self.root / "web", self.root / "result")
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["states"]), 1)
        (self.root / "web/index.json").write_text(json.dumps({"unreachable": [{"state": "menu"}]}))
        result = compare_matrix(self.root / "gtk", self.root / "web", self.root / "result")
        self.assertFalse(result["passed"])
        self.assertEqual(result["unavailable_states"], [{"state": "menu"}])
        (self.root / "web/index.json").unlink()
        (self.root / "gtk/index.json").write_text(json.dumps({"failed_cases": [{"case": "enlarged"}]}))
        result = compare_matrix(self.root / "gtk", self.root / "web", self.root / "result")
        self.assertFalse(result["passed"])
        self.assertEqual(result["failed_reference_cases"], [{"case": "enlarged"}])
        self.web["state"] = "other"
        (self.root / "web/one.allocations.json").write_text(json.dumps(self.web))
        result = compare_matrix(self.root / "gtk", self.root / "web", self.root / "result")
        self.assertFalse(result["passed"])
        self.assertEqual(len(result["unpaired_gtk"]), 1)
        self.assertEqual(len(result["unpaired_web"]), 1)

    def test_manifest_pairs_android_captures_and_reports_missing_references(self):
        reference = self.root / "gtk/rest.png"
        reference.parent.mkdir()
        Image.new("RGBA", (3, 2), (20, 40, 60, 255)).save(reference)
        (self.root / "gtk/rest.json").write_text(json.dumps({"pixel_encoding": "gamma22-on-srgb"}))
        key = {"family": "frame", "state": "rest", "case": "3x2-light", "theme": "light", "text_size": 11, "fixture": "builtin:workspace:painter"}
        (self.root / "references.json").write_text(json.dumps({"references": [
            dict(key, png=str(self.root / "gtk/repeat.png"), duplicate_of=str(reference)), dict(key, png=str(reference))]}))
        for case, theme in (("3x2-light", "light"), ("3x2-dark", "dark")):
            directory = self.root / "android" / case
            directory.mkdir(parents=True)
            (directory / "admission.json").write_text(json.dumps(dict(key, schema_version=1, host="android", case=case, theme=theme, text_size=11.0)))
            (directory / "run.json").write_text(json.dumps({"case": case, "passed": True}))
            Image.new("RGB", (3, 2), (20, 40, 60)).save(directory / "admission.png")
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("phone_audit.py")), "--reference", str(self.root / "references.json"),
            "--candidate", str(self.root / "android"), "--output", str(self.root / "sheets")], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        states = json.loads(result.stdout)["states"]
        self.assertEqual([(state["case"], state["status"]) for state in states], [("3x2-dark", "missing"), ("3x2-light", "compared")])
        self.assertEqual(states[1]["reference"], str(reference))
        with Image.open(self.root / "sheets/frame/3x2-light/rest/sheet.png") as sheet:
            self.assertEqual(sheet.size, (9, 2))
        comparison = json.loads((self.root / "sheets/frame/3x2-light/rest/comparison.json").read_text())
        self.assertIn("gamma22-on-srgb", comparison["input_profiles"][0])
        self.assertIn("| frame | 3x2-dark | rest | missing |", (self.root / "sheets/index.md").read_text())

    def test_duplicate_capture_states_are_rejected(self):
        for name in ("one", "two"):
            (self.root / f"{name}.allocations.json").write_text(json.dumps(self.gtk))
        with self.assertRaisesRegex(ValueError, "Duplicate capture state"):
            index(self.root)


if __name__ == "__main__":
    unittest.main()
