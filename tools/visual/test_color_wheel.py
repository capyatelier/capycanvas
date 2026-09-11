"""Ensure sampled color failures and missing pixels cannot become a pass."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

SCRIPT = Path(__file__).with_name("check_color_wheel.py")

class ColorWheelCheckTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="capy-color-")
        self.root = Path(self.temp.name).resolve()
        def cleanup():
            # Verify the owned resolved temporary directory before recursive removal.
            self.assertEqual(self.root.resolve(), self.root)
            self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
            self.assertTrue(self.root.name.startswith("capy-color-"))
            self.temp.cleanup()
        self.addCleanup(cleanup)
        # A deterministic oracle double isolates report/coordinate handling
        # from Rust's separately tested picker. Both regions need coverage.
        self.oracle = self.root / "oracle"
        self.oracle.write_text(f"#!{sys.executable}\n" + '''import json, sys
request = json.load(sys.stdin)
print(json.dumps({"model": {"hue_marker": [-10,-10], "field_marker": [-10,-10]},
    "samples": [{"part": "hue" if p[0] < 0.5 else "field",
        "rgba": [1 if p[0] < 0.5 else 0,0.25,0.5,1]} for p in request["points"]]}))
''')
        self.oracle.chmod(0o700)
        self.fixture = self.root / "fixture.json"
        self.fixture.write_text(json.dumps({"space":"hsv", "rgba":[1,0.25,0.5,1],
            "viewport":[128,128], "wheel":[0,0,128,128]}))
        self.image = Image.new("RGBA", (128,128), (0,64,128,255))
        self.image.paste((255,64,128,255), (0,0,64,128))

    def check_image(self):
        source = self.root / "source.png"
        self.image.save(source)
        result = subprocess.run([sys.executable, str(SCRIPT), str(source), str(self.fixture),
            "--oracle", str(self.oracle), "--oracle-interpreter", sys.executable,
            "--output", str(self.root / "report.json")],
            capture_output=True, text=True)
        return result, json.loads(result.stdout) if result.stdout else None

    def test_correct_pixels_pass_with_both_regions_sampled(self):
        result, report = self.check_image()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(report["passed"])
        self.assertGreaterEqual(min(report["sampled_pixels"].values()), 20)

    def test_one_wrong_sample_is_retained(self):
        self.image.putpixel((11,11), (255,64,137,255))
        result, report = self.check_image()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(report["pixels_above_tolerance"], 1)
        self.assertEqual(report["maximum_channel_error"]["hue"], 9)
        self.assertEqual(report["worst_samples"][0]["pixel"], [11,11])

    def test_transparency_cannot_hide_a_missing_sample(self):
        self.image.putpixel((11,11), (255,64,128,0))
        result, report = self.check_image()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(report["maximum_channel_error"]["hue"], 255)

    def test_remembered_hue_reaches_the_oracle(self):
        fixture = json.loads(self.fixture.read_text())
        fixture["hue"] = 269
        self.fixture.write_text(json.dumps(fixture))
        self.oracle.write_text(self.oracle.read_text().replace('print(json.dumps(',
            'assert request.get("hue") == 269\nprint(json.dumps('))
        result, report = self.check_image()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(report["passed"])

    def test_mismatched_orientation_is_rejected(self):
        self.image = Image.new("RGB", (128,256))
        result, _ = self.check_image()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("viewport dimensions do not match", result.stderr)

if __name__ == "__main__":
    unittest.main()
