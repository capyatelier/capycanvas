"""The bundle must preserve every canonical SVG, including paint and viewBox."""
import importlib.util
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("web_icons", Path(__file__).with_name("web-icons.py"))
icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icons)


class IconBundleTest(unittest.TestCase):
    def test_exact_source_tree_and_complete_catalog(self):
        directory = ROOT / "apps/layer-web/icons"
        bundled = ET.fromstring(icons.bundle(directory))
        actual = {node.attrib.pop("data-asset"): node for node in bundled}
        originals = list(directory.glob("layer-*-symbolic.svg"))
        self.assertEqual(len(actual), len(originals))
        self.assertEqual(len(bundled), len(originals))
        for path in originals:
            name = path.name.removeprefix("layer-").removesuffix("-symbolic.svg")
            expected = ET.fromstring(path.read_text())
            # Only the outer bundle adds inter-icon whitespace.
            actual[name].tail = expected.tail
            self.assertEqual(ET.tostring(actual[name]), ET.tostring(expected), name)
        self.assertEqual(icons.bundle(directory), icons.bundle(directory))


if __name__ == "__main__":
    unittest.main()
