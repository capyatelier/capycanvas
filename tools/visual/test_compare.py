"""Guard the acceptance tool against hidden pixel errors and orientation drift."""
import base64
import json
import struct
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

SCRIPT = Path(__file__).with_name("compare.py")
GAMMA22_PROFILE = base64.b64decode(
    "AAAB4AAAAAAEMAAAbW50clJHQiBYWVogB+AAAQABAAAAAAAAYWNzcAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAPbWAAEAAAAA0y0AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAJZGVzYwAAAPAAAABUclhZWgAAAUQAAAAUZ1hZWgAAAVgAAAAUYlhZWgAAAWwAAAAUd3RwdAAAAYAAAAAUclRSQwAAAZQAAAAQZ1RSQwAAAZQAAAAQYlRSQwAAAZQAAAAQY3BydAAAAaQAAAA8bWx1YwAAAAAAAAABAAAADGVuVVMAAAA4AAAAHABzAFIARwBCACAARwBhAG0AdQB0ACAAdwBpAHQAaAAgADIALgAyACAAVAByAGEAbgBzAGYAZQByWFlaIAAAAAAAAG+iAAA49QAAA5BYWVogAAAAAAAAYpkAALeFAAAY2lhZWiAAAAAAAAAkoAAAD4QAALbPWFlaIAAAAAAAAPbWAAEAAAAA0y1wYXJhAAAAAAAAAAAAAjMzbWx1YwAAAAAAAAABAAAADGVuVVMAAAAgAAAAHABHAG8AbwBnAGwAZQAgAEkAbgBjAC4AIAAyADAAMQA2")

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

    def declare_native_encoding(self, **changes):
        encoding = {"primaries": 1, "transfer": 4, "matrix": 1, "range": 0, **changes}
        (self.root / "reference.json").write_text(json.dumps({"colorimetry": encoding}))

    def test_equal_raw_pixels_with_declared_native_encoding_use_the_same_icc_transform(self):
        self.reference = Image.new("RGB", (256, 4))
        self.reference.putdata([(r, g, b) for value in range(256) for r, g, b in
            [(value, value, value), (0, value, value), (value, 0, 255 - value), (255 - value, value, 0)]])
        self.reference.save(self.root / "reference.png")
        self.declare_native_encoding()
        result = self.run_compare(self.reference, icc_profile=GAMMA22_PROFILE)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["exact_different_pixels"], 0)
        self.assertIn("declared BT.709 gamma 2.2", report["input_profiles"][0])

    def profile_tag(self, name):
        for index in range(struct.unpack_from(">I", GAMMA22_PROFILE, 128)[0]):
            tag, offset, size = struct.unpack_from(">4sII", GAMMA22_PROFILE, 132 + index * 12)
            if tag == name:
                return offset, size
        self.fail(name)

    def test_mismatched_gamma_is_refused_even_with_the_same_description(self):
        self.declare_native_encoding()
        for name in (b"rTRC", b"gTRC", b"bTRC"):
            with self.subTest(tag=name):
                profile = bytearray(GAMMA22_PROFILE)
                offset, size = self.profile_tag(name)
                curve = profile[offset:offset + size]
                struct.pack_into(">i", curve, 12, round(2.4 * 65536))
                entry = next(132 + index * 12 for index in range(struct.unpack_from(">I", profile, 128)[0])
                    if profile[132 + index * 12:136 + index * 12] == name)
                struct.pack_into(">I", profile, entry + 4, len(profile))
                profile.extend(curve)
                struct.pack_into(">I", profile, 0, len(profile))
                result = self.run_compare(self.reference, icc_profile=bytes(profile))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unsupported encoding", result.stderr)
                self.assertFalse((self.root / "result/difference.png").exists())

    def test_mismatched_primaries_are_refused_even_with_the_same_description(self):
        self.declare_native_encoding()
        for name in (b"rXYZ", b"gXYZ", b"bXYZ"):
            with self.subTest(tag=name):
                profile = bytearray(GAMMA22_PROFILE)
                offset, _ = self.profile_tag(name)
                value, = struct.unpack_from(">i", profile, offset + 8)
                struct.pack_into(">i", profile, offset + 8, value + 2)
                result = self.run_compare(self.reference, icc_profile=bytes(profile))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unsupported encoding", result.stderr)

    def test_single_gamma_curve_representation_is_supported(self):
        self.declare_native_encoding()
        profile = bytearray(GAMMA22_PROFILE)
        offset, size = self.profile_tag(b"rTRC")
        profile[offset:offset + size] = b"curv" + bytes(4) + struct.pack(">IH", 1, round(2.2 * 256)) + bytes(2)
        result = self.run_compare(self.reference, icc_profile=bytes(profile))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unsupported_native_encoding_or_missing_paired_profile_is_refused(self):
        for change in ({"primaries": 9}, {"transfer": 7}, {"matrix": 4}, {"range": 2}):
            with self.subTest(change=change):
                self.declare_native_encoding(**change)
                result = self.run_compare(self.reference, icc_profile=GAMMA22_PROFILE)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unsupported encoding", result.stderr)
        self.declare_native_encoding()
        result = self.run_compare(self.reference)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported encoding", result.stderr)

    def test_invalid_icc_tag_bounds_are_refused(self):
        self.declare_native_encoding()
        profile = bytearray(GAMMA22_PROFILE)
        struct.pack_into(">I", profile, 136, len(profile) + 4)
        result = self.run_compare(self.reference, icc_profile=bytes(profile))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported encoding", result.stderr)

    def test_declared_native_candidate_uses_the_reference_profile(self):
        self.reference.save(self.root / "reference.png", icc_profile=GAMMA22_PROFILE)
        (self.root / "candidate.json").write_text(json.dumps({"colorimetry": {
            "primaries": 1, "transfer": 4, "matrix": 1, "range": 0}}))
        result = self.run_compare(self.reference)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("declared BT.709 gamma 2.2", json.loads(result.stdout)["input_profiles"][1])

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

    def test_sheet_keeps_all_three_images_at_native_size(self):
        result = self.run_compare(self.reference, arguments=("--sheet",))
        self.assertEqual(result.returncode, 0, result.stderr)
        with Image.open(self.root / "result/sheet.png") as sheet:
            self.assertEqual(sheet.size, (9, 2))
            self.assertIn("icc_profile", sheet.info)
            self.assertEqual(sheet.getpixel((0, 0)), (20, 40, 60))
            self.assertEqual(sheet.getpixel((6, 0)), (0, 0, 0))

if __name__ == "__main__":
    unittest.main()
