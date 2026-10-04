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
        registry = (ROOT / "crates/layer-ui/src/localization_languages.rs").read_text()
        tags = dict(re.findall(r'\("(\w+)", "([^"\n]+)",', registry))
        names = re.findall(r"UiLanguage::(\w+)", inventory)
        if names == ["ALL"]:
            names = list(tags)
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

    def test_both_apps_ship_one_identity_privacy_manifest_and_mac_sandbox(self):
        project = (APPLE / "CapyCanvas.xcodeproj/project.pbxproj").read_text()
        ident = lambda name: hashlib.sha256(name.encode()).hexdigest()[:24].upper()
        for scheme in ("CapyCanvas-iPad", "CapyCanvas-Mac"):
            phase = re.search(r'"' + ident(scheme + "resources") + r'" = \{(.+?)\n\t\t\};', project, re.S).group(1)
            self.assertIn(ident(scheme + "Shared/PrivacyInfo.xcprivacy"), phase)
        self.assertEqual(set(re.findall(r'"CAPY_APPLE_BUNDLE_ID" = "([^"]+)"', project)),
            {"art.capycanvas.CapyCanvas", "art.capycanvas.CapyCanvas.dev"})
        self.assertEqual(project.count('"CODE_SIGN_ENTITLEMENTS" = "macOS/App/CapyCanvas.entitlements"'), 2)
        self.assertEqual(project.count('"ENABLE_HARDENED_RUNTIME" = "YES"'), 2)
        entitlements = plistlib.loads((APPLE / "macOS/App/CapyCanvas.entitlements").read_bytes())
        self.assertEqual(entitlements, {"com.apple.security.app-sandbox": True,
            "com.apple.security.files.user-selected.read-write": True})
        manifest = plistlib.loads((APPLE / "Shared/PrivacyInfo.xcprivacy").read_bytes())
        self.assertFalse(manifest["NSPrivacyTracking"])
        self.assertEqual(manifest["NSPrivacyCollectedDataTypes"], [])


if __name__ == "__main__":
    unittest.main()
