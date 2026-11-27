"""Protect SVG compositing semantics at the generated-asset boundary."""
from pathlib import Path
import sys
import struct
import tempfile
import zlib
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from icon_assets import icon_layers, mac_app_icon_entries, write_mac_app_icon


def svg(body, attributes=""):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" {attributes}>{body}</svg>'


class IconAssetTests(unittest.TestCase):
    def test_mac_target_has_app_icon(self):
        project = Path(__file__).resolve().parents[1] / "CapyCanvas.xcodeproj/project.pbxproj"
        self.assertEqual(project.read_text().count('"ASSETCATALOG_COMPILER_APPICON_NAME" = "AppIcon-Mac";'), 2)

    def test_mac_app_icon_has_all_point_sizes_and_scales(self):
        entries = mac_app_icon_entries()
        self.assertEqual({(entry["idiom"], entry["size"], entry["scale"]) for entry in entries},
                         {("mac", f"{size}x{size}", f"{scale}x")
                          for size in (16, 32, 128, 256, 512) for scale in (1, 2)})
        self.assertEqual(len({entry["filename"] for entry in entries}), 10)

    def test_mac_app_icon_renders_with_the_mac_sdk_inside_an_ios_build(self):
        with tempfile.TemporaryDirectory() as directory, \
                mock.patch.dict("os.environ", {"SDKROOT": "/Applications/Xcode.app/iPhoneOS.sdk"}), \
                mock.patch("icon_assets.subprocess.run") as run:
            write_mac_app_icon(Path("AppIcon.png"), Path(directory))
        command, options = run.call_args.args[0], run.call_args.kwargs
        self.assertEqual(command[:4], ["xcrun", "--sdk", "macosx", "swift"])
        self.assertNotIn("SDKROOT", options["env"])

    @unittest.skipUnless(sys.platform == "darwin", "Native icon renderer needs Xcode")
    def test_mac_app_icon_renders_transparent_rounded_art(self):
        source = Path(__file__).resolve().parents[1] / "iOS/AppIcon.png"
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            entries = write_mac_app_icon(source, destination)["images"]
            for entry in entries:
                with self.subTest(entry=entry):
                    size = int(entry["size"].split("x")[0]) * int(entry["scale"][0])
                    data = (destination / entry["filename"]).read_bytes()
                    self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
                    self.assertEqual(struct.unpack(">II", data[16:24]), (size, size))
                    self.assertEqual(data[24:26], bytes((8, 6)))
                    chunks, offset = [], 8
                    while offset < len(data):
                        length = struct.unpack(">I", data[offset:offset + 4])[0]
                        if data[offset + 4:offset + 8] == b"IDAT":
                            chunks.append(data[offset + 8:offset + 8 + length])
                        offset += length + 12
                    self.assertEqual(zlib.decompress(b"".join(chunks))[4], 0)

    def test_symbolic_groups_retain_opacity_as_one_vector(self):
        source = svg('<g opacity=".5"><circle cx="6" cy="6" r="4"/><circle cx="8" cy="8" r="4"/></g>', 'fill="currentColor"')
        self.assertEqual(icon_layers(source), [("template", source.replace("currentColor", "#ffffff"))])

    def test_fixed_paints_retain_color_and_opacity(self):
        source = svg('<circle fill="#33d17a" opacity=".5" cx="8" cy="8" r="4"/>')
        self.assertEqual(icon_layers(source), [("original", source)])

    def test_overlap_retains_svg_painter_order(self):
        source = svg('<rect id="back" fill="#fff"/><path id="outline" fill="none" stroke="currentColor"/><rect id="front" fill="#000"/>')
        layers = icon_layers(source)
        self.assertEqual([mode for mode, _ in layers], ["original", "template", "original"])
        self.assertEqual([child.get("id") for _, text in layers for child in ET.fromstring(text)], ["back", "outline", "front"])

    def test_mixed_shape_preserves_default_fill_before_stroke(self):
        layers = icon_layers(svg('<circle cx="8" cy="8" r="5" fill="#33d17a" stroke="currentColor"/>'))
        self.assertEqual([mode for mode, _ in layers], ["original", "template"])
        fill, stroke = [ET.fromstring(text)[0] for _, text in layers]
        self.assertEqual((fill.get("fill"), fill.get("stroke")), ("#33d17a", "none"))
        self.assertEqual((stroke.get("fill"), stroke.get("stroke")), ("none", "#ffffff"))

    def test_mixed_opacity_requires_group_compositing(self):
        body = '<circle fill="#fff"/><circle fill="currentColor"/>'
        for source in [svg(body, 'opacity=".5"'), svg('<g opacity=".5">'+body+'</g>'),
                       svg('<circle fill="#fff" stroke="currentColor" opacity=".5"/>')]:
            with self.assertRaisesRegex(ValueError, 'compositing'):
                icon_layers(source)

    def test_every_canonical_svg_is_supported(self):
        sources = Path(__file__).resolve().parents[2] / "layer-web" / "icons"
        self.assertTrue(sources.is_dir())
        for source in sources.glob('*.svg'):
            with self.subTest(source=source.name):
                layers = icon_layers(source.read_text())
                self.assertTrue(layers)
                for _, text in layers:
                    self.assertNotIn('currentColor', text)
                    self.assertEqual(ET.fromstring(text).get('viewBox'), ET.fromstring(source.read_text()).get('viewBox'))


if __name__ == '__main__':
    unittest.main()
