import copy
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import android_apk


VERSION = "1.0.9"
BADGING = """package: name='art.capycanvas.editor' versionCode='1000009' versionName='1.0.9' platformBuildVersionName='17'
minSdkVersion:'29'
targetSdkVersion:'37'
native-code: 'arm64-v8a'
"""
MANIFEST = """E: manifest
  E: application
    E: activity
      A: android:name="art.capycanvas.MainActivity"
    E: meta-data
      A: android:name="com.android.vending.derived.apk.id"
    E: meta-data
      A: android:name="com.android.stamp.source"
      A: android:value="https://play.google.com/store"
"""
SIGNATURE = f"V3.0 Signer: certificate SHA-256 digest: {android_apk.SIGNING_CERTIFICATE}\n"
METADATA = {"generatedApks": [{
    "certificateSha256Hash": android_apk.SIGNING_CERTIFICATE,
    "generatedUniversalApk": {"downloadId": "protected"},
    "generatedStandaloneApks": [{"downloadId": "protected-standalone", "variantId": 1}],
    "unprotectedGeneratedSplitApks": [{"downloadId": "base-split", "variantId": 1, "moduleName": "base", "splitId": ""}],
    "unprotectedGeneratedStandaloneApks": [{"downloadId": "unprotected", "variantId": 1}],
}]}


def apk_bytes(dex=b"Lart/capycanvas/MainActivity;", renderer=True):
    data = io.BytesIO()
    with zipfile.ZipFile(data, "w") as archive:
        archive.writestr("classes.dex", dex)
        if renderer:
            archive.writestr("lib/arm64-v8a/liblayer_android.so", b"renderer")
    return data.getvalue()


class AndroidApkTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.apk = self.root / "app.apk"
        self.apk.write_bytes(apk_bytes())
        self.tools = self.root / "build-tools"
        self.badging = BADGING
        self.manifest = MANIFEST
        self.signature = SIGNATURE
        self.commands = []
        self.enterContext(patch.object(android_apk.subprocess, "check_output", side_effect=self.tool))

    def tool(self, command, **kwargs):
        self.commands.append(command)
        if command[1:3] == ["dump", "badging"]:
            return self.badging
        if command[1:3] == ["dump", "xmltree"]:
            return self.manifest
        return self.signature

    def verify(self):
        android_apk.verify_apk(self.apk, VERSION, self.tools)

    def test_selects_unprotected_standalone_instead_of_protected_universal(self):
        self.assertEqual(android_apk.candidates(METADATA), ["unprotected"])

    def test_split_only_protection_never_falls_back_to_protected_universal(self):
        metadata = copy.deepcopy(METADATA)
        del metadata["generatedApks"][0]["unprotectedGeneratedStandaloneApks"]
        self.assertEqual(android_apk.candidates(metadata), [])

    def test_universal_is_available_when_protection_is_disabled(self):
        metadata = {"generatedApks": [{"generatedUniversalApk": {"downloadId": "universal"}}]}
        self.assertEqual(android_apk.candidates(metadata), ["universal"])
        self.assertEqual(android_apk.candidates({}), [])

    def test_checks_every_signing_group(self):
        metadata = copy.deepcopy(METADATA)
        metadata["generatedApks"].insert(0, {"certificateSha256Hash": "new-key"})
        self.assertEqual(android_apk.candidates(metadata), ["unprotected"])

    def test_accepts_play_signature_and_store_metadata_without_runtime_checks(self):
        self.apk.write_bytes(apk_bytes(b"com.google.android.gms.provider.action.PICK_IMAGES\0"
                                      b"com.google.android.gms.provider.extra.PICK_IMAGES_MAX"))
        self.verify()
        self.assertIn([str(self.tools / "apksigner"), "verify", "--min-sdk-version", "29",
                       "--max-sdk-version", "29", "--print-certs", str(self.apk)], self.commands)
        self.assertIn([str(self.tools / "apksigner"), "verify", str(self.apk)], self.commands)

    def test_rejects_injected_application_and_license_permission(self):
        for marker in ("com.pairip.application.Application", "com.android.vending.CHECK_LICENSE"):
            with self.subTest(marker=marker):
                self.manifest = MANIFEST + marker
                with self.assertRaisesRegex(ValueError, "requires Google Play"):
                    self.verify()

    def test_rejects_google_runtime_code_in_secondary_dex(self):
        for marker in (b"Lcom/pairip/licensecheck/LicenseClient;", b"com.android.vending.licensing.ILicensingService",
                       b"Lcom/google/android/gms/common/GoogleApiAvailability;"):
            with self.subTest(marker=marker):
                self.apk.write_bytes(apk_bytes())
                with zipfile.ZipFile(self.apk, "a") as archive:
                    archive.writestr("classes2.dex", marker)
                with self.assertRaisesRegex(ValueError, "contains Google"):
                    self.verify()

    def test_rejects_wrong_version_package_or_device_coverage(self):
        for badging in (BADGING.replace("1000009", "1000008"), BADGING.replace("1.0.9", "1.0.8"),
                        BADGING.replace("art.capycanvas.editor", "art.capycanvas.dev"),
                        BADGING.replace("minSdkVersion:'29'", "minSdkVersion:'37'"),
                        BADGING + "maxSdkVersion:'36'\n", BADGING.replace("arm64-v8a", "x86_64")):
            with self.subTest(badging=badging):
                self.badging = badging
                with self.assertRaises(ValueError):
                    self.verify()

    def test_rejects_split_base_and_config_apks(self):
        for manifest in (MANIFEST + "E: uses-split", MANIFEST + "A: android:isSplitRequired(0x1)=true"):
            with self.subTest(manifest=manifest):
                self.manifest = manifest
                with self.assertRaisesRegex(ValueError, "additional split APKs"):
                    self.verify()
        self.manifest = MANIFEST
        self.badging = BADGING.replace("platformBuildVersionName", "split='config.arm64_v8a' platformBuildVersionName")
        with self.assertRaisesRegex(ValueError, "additional split APKs"):
            self.verify()

    def test_rejects_missing_renderer(self):
        self.apk.write_bytes(apk_bytes(renderer=False))
        with self.assertRaisesRegex(ValueError, "missing the Android renderer"):
            self.verify()

    def test_rejects_wrong_certificate_even_if_source_stamp_matches(self):
        self.signature = "Signer #1 certificate SHA-256 digest: " + "a" * 64 + "\nSource Stamp Signer: " + SIGNATURE
        with self.assertRaisesRegex(ValueError, "break updates"):
            self.verify()

    def test_signature_failure_is_not_accepted(self):
        def invalid_signature(command, **kwargs):
            if Path(command[0]).name == "apksigner":
                raise subprocess.CalledProcessError(1, command)
            return self.tool(command, **kwargs)
        with patch.object(android_apk.subprocess, "check_output", side_effect=invalid_signature):
            with self.assertRaises(subprocess.CalledProcessError):
                self.verify()

    def download(self, metadata, content):
        urls = []
        def fetch(url, token):
            self.assertEqual(token, "private-token")
            urls.append(url)
            return io.BytesIO(content if ":download?" in url else json.dumps(metadata).encode())
        with patch.object(android_apk, "fetch", side_effect=fetch), patch.object(android_apk.time, "sleep"):
            android_apk.download_apk(VERSION, self.root / "release.apk", self.tools, "private-token")
        return urls

    def test_download_publishes_only_verified_unprotected_apk(self):
        content = apk_bytes()
        urls = self.download(METADATA, content)
        self.assertEqual(len(urls), 2)
        self.assertTrue(urls[1].endswith("/downloads/unprotected:download?alt=media"))
        self.assertEqual((self.root / "release.apk").read_bytes(), content)

    def test_rejected_download_preserves_existing_output(self):
        output = self.root / "release.apk"
        output.write_bytes(b"previous verified APK")
        with self.assertRaisesRegex(ValueError, "Turn off Automatic protection"):
            self.download(METADATA, apk_bytes(b"Lcom/pairip/licensecheck/LicenseClient;"))
        self.assertEqual(output.read_bytes(), b"previous verified APK")
        self.assertEqual(sorted(path.name for path in self.root.iterdir()), ["app.apk", "release.apk"])

    def test_waits_for_unprotected_generation(self):
        responses = iter([io.BytesIO(b'{}'), io.BytesIO(json.dumps(METADATA).encode()), io.BytesIO(apk_bytes())])
        with patch.object(android_apk, "fetch", side_effect=lambda *args: next(responses)), \
                patch.object(android_apk.time, "sleep") as sleep:
            android_apk.download_apk(VERSION, self.root / "release.apk", self.tools, "private-token")
        sleep.assert_called_once_with(20)


if __name__ == "__main__":
    unittest.main()
