"""Guard the acceptance tool against hidden pixel errors and orientation drift."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

SCRIPT = Path(__file__).with_name("compare.py")

class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.reference = Image.new("RGB", (3, 2), (20, 40, 60))
        self.reference.save(self.root / "reference.png")

    def run_compare(self, candidate, arguments=(), **save_options):
        candidate.save(self.root / "candidate.png", **save_options)
        return subprocess.run([sys.executable, str(SCRIPT), str(self.root / "reference.png"),
            str(self.root / "candidate.png"), "--output", str(self.root / "result"), *arguments], capture_output=True, text=True)

    def test_single_pixel_error_is_not_lost_in_global_average(self):
        candidate = self.reference.copy()
        candidate.putpixel((2, 1), (27, 40, 60))
        result = self.run_compare(candidate)
        self.assertEqual(result.returncode, 1)
        report = json.loads(result.stdout)
        self.assertEqual(report["exact_different_pixels"], 1)
        self.assertEqual(report["maximum_channel_error"], 7)
        self.assertEqual(report["difference_bounds_pixels"], [2, 1, 3, 2])
        self.assertTrue((self.root / "result/difference.png").exists())

    def test_declared_orientation_is_applied_without_resampling(self):
        self.reference.putpixel((0, 1), (100, 150, 200))
        self.reference.save(self.root / "reference.png")
        stored = self.reference.transpose(Image.Transpose.ROTATE_90)
        exif = Image.Exif(); exif[274] = 6
        result = self.run_compare(stored, exif=exif)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["exact_different_pixels"], 0)

    def test_mismatched_capture_geometry_is_rejected(self):
        result = self.run_compare(Image.new("RGB", (2, 3), (20, 40, 60)))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Capture dimensions differ", result.stderr)
        self.assertFalse((self.root / "result/difference.png").exists())

    def test_raw_device_rotation_preserves_and_reports_every_pixel(self):
        self.reference.putpixel((0, 1), (100, 150, 200))
        self.reference.save(self.root / "reference.png")
        stored = self.reference.transpose(Image.Transpose.ROTATE_270)
        stored.putpixel((0, 2), (27, 40, 60))
        result = self.run_compare(stored, arguments=("--candidate-rotation", "90"))
        self.assertEqual(result.returncode, 1)
        report = json.loads(result.stdout)
        self.assertEqual(report["candidate_rotation_counterclockwise"], 90)
        self.assertEqual(report["dimensions"], [3, 2])
        self.assertEqual(report["exact_different_pixels"], 1)
        self.assertEqual(report["maximum_channel_error"], 7)

    def test_transparency_cannot_conceal_missing_canvas(self):
        result = self.run_compare(Image.new("RGBA", (3, 2), (20, 40, 60, 0)))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected an opaque", result.stderr)

if __name__ == "__main__":
    unittest.main()
