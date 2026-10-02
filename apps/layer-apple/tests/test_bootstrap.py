"""Check Apple bundle advertisement against the shared shipping inventory."""
from pathlib import Path
import hashlib
import plistlib
import re
import unittest

ROOT = Path(__file__).resolve().parents[3]
APPLE = ROOT / "apps/layer-apple"

class BootstrapMetadataTests(unittest.TestCase):
    def test_bundle_languages_match_shared_shipping_inventory(self):
        source = (ROOT / "crates/layer-ui/src/localization.rs").read_text()
        inventory = re.search(r"pub const SHIPPED_LANGUAGES[^;]+;", source).group()
        tags = dict(re.findall(r'Self::(\w+) => "([^"\n]+)"', source.split("pub const fn native_name")[0]))
        names = re.findall(r"UiLanguage::(\w+)", inventory)
        if names == ["ALL"]:
            inventory = re.search(r"pub const ALL[^=]+=\s*\[([^\]]+)\]", source).group(1)
            names = re.findall(r"Self::(\w+)", inventory)
        shipped = [tags[name] for name in names]
        self.assertTrue(shipped)
        for platform in ("iOS", "macOS"):
            with self.subTest(platform=platform):
                info = plistlib.loads((APPLE / platform / "App/Info.plist").read_bytes())
                self.assertEqual(info["CFBundleLocalizations"], shipped)

    def test_native_text_context_is_in_both_generated_targets(self):
        project = (APPLE / "CapyCanvas.xcodeproj/project.pbxproj").read_text()
        self.assertIn("NativeTextContext.swift", project)
        self.assertEqual(project.count("NativeTextContext.swift"), 1)
        for scheme in ("CapyCanvas-iPad", "CapyCanvas-Mac"):
            source_id = hashlib.sha256((scheme + "Shared/Bridge/NativeTextContext.swift").encode()).hexdigest()[:24].upper()
            phase_id = hashlib.sha256((scheme + "sources").encode()).hexdigest()[:24].upper()
            phase = re.search(r'"' + phase_id + r'" = \{(.+?)\n\t\t\};', project, re.S).group(1)
            self.assertIn(source_id, phase)


if __name__ == "__main__":
    unittest.main()
