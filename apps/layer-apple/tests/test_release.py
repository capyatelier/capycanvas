"""Exercise archive and upload boundaries without Xcode or signing services."""
import json
import os
import plistlib
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="capy release ")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.script = self.root / "apps/layer-apple/scripts/release.sh"
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / "apps/layer-apple/scripts/release.sh", self.script)
        (self.root / "Cargo.toml").write_text('version = "1.2.3"\n')
        self.archive = self.root / "dist/apple-ipad/CapyCanvas-iPad.xcarchive"
        self.log = self.root / "xcode.json"
        self.dmg_log = self.root / "hdiutil.json"
        tools = self.root / "bin"
        tools.mkdir()
        for name in ("python3", "node"):
            path = tools / name
            path.write_text('#!/bin/sh\n[ "${CAPY_TEST_PUBLISH:-}" != 1 ] || exit 1\n'
                            + (f'if [ "$1" = - ]; then exec "{sys.executable}" "$@"; fi\n' if name == "python3" else ''))
            path.chmod(0o755)
        xcode = tools / "xcodebuild"
        xcode.write_text(f"#!{sys.executable}\n"
                         "import json, os, sys\nfrom pathlib import Path\n"
                         "Path(os.environ['CAPY_TEST_LOG']).write_text(json.dumps(sys.argv[1:]))\n"
                         "if sys.argv[1] == 'archive':\n"
                         "    archive = Path(sys.argv[sys.argv.index('-archivePath') + 1])\n"
                         "    (archive / 'Products/Applications' / (archive.stem + '.app')).mkdir(parents=True)\n"
                         "status = int(os.environ.get('CAPY_TEST_XCODE_STATUS', '0'))\n"
                         "if status: print('fixture archive diagnostic')\n"
                         "sys.exit(status)\n")
        xcode.chmod(0o755)
        ditto = tools / "ditto"
        ditto.write_text(f"#!{sys.executable}\nimport os, shutil, sys\nfrom pathlib import Path\n"
                         "shutil.copytree(sys.argv[1], sys.argv[2])\n"
                         "if 'CAPY_TEST_PAYLOAD_BYTES' in os.environ:\n"
                         "    resource = Path(sys.argv[2]) / 'Contents/Resources/large sparse asset.car'\n"
                         "    resource.parent.mkdir(parents=True)\n"
                         "    with resource.open('wb') as stream: stream.truncate(int(os.environ['CAPY_TEST_PAYLOAD_BYTES']))\n"
                         "    external = Path(sys.argv[2]).parent.parent / 'external files'\n"
                         "    external.mkdir()\n"
                         "    with (external / 'large asset').open('wb') as stream: stream.truncate(1073741824)\n"
                         "    (resource.parent / 'outside directory').symlink_to(external, target_is_directory=True)\n"
                         "    (resource.parent / 'outside file').symlink_to(external / 'large asset')\n")
        ditto.chmod(0o755)
        hdiutil = tools / "hdiutil"
        hdiutil.write_text(f"#!{sys.executable}\n"
                          "import json, os, sys\nfrom pathlib import Path\n"
                          "log = Path(os.environ['CAPY_TEST_DMG_LOG'])\n"
                          "calls = json.loads(log.read_text()) if log.exists() else []\n"
                          "calls.append(sys.argv[1:]); log.write_text(json.dumps(calls))\n"
                          "action = sys.argv[1]\n"
                          "if '-quiet' not in sys.argv: print('fixture DMG ' + action + ' diagnostic')\n"
                          "if action == 'create':\n"
                          "    if 'CAPY_TEST_PAYLOAD_BYTES' in os.environ:\n"
                          "        if '-size' not in sys.argv or '-fs' not in sys.argv: sys.exit(28)\n"
                          "        if sys.argv[sys.argv.index('-fs') + 1] != 'HFS+': sys.exit(28)\n"
                          "        capacity = sys.argv[sys.argv.index('-size') + 1]\n"
                          "        if not capacity.endswith('m'): sys.exit(28)\n"
                          "        if int(capacity[:-1]) * 1048576 < int(os.environ['CAPY_TEST_PAYLOAD_BYTES']) + 33554432: sys.exit(28)\n"
                          "    status = int(os.environ.get('CAPY_TEST_DMG_STATUS', '0'))\n"
                          "    if not status: Path(sys.argv[-1]).write_bytes(b'fixture DMG')\n"
                          "    sys.exit(status)\n"
                          "if action == 'verify': sys.exit(int(os.environ.get('CAPY_TEST_VERIFY_STATUS', '0')))\n")
        hdiutil.chmod(0o755)
        self.environment = {key: value for key, value in os.environ.items() if not key.startswith("CAPY_APPLE_")}
        self.environment.update(PATH=str(tools) + os.pathsep + os.environ["PATH"],
                                CAPY_TEST_LOG=str(self.log), CAPY_TEST_DMG_LOG=str(self.dmg_log))

    def run_release(self, *arguments, team=None):
        environment = dict(self.environment)
        if team:
            key = self.root / 'AuthKey.p8'
            key.touch()
            environment.update(CAPY_APPLE_TEAM=team, CAPY_APPLE_KEY=str(key),
                               CAPY_APPLE_KEY_ID='KEY', CAPY_APPLE_ISSUER='ISSUER')
        return subprocess.run(["bash", str(self.script), *arguments], env=environment, capture_output=True, text=True)

    def test_unsigned_archive_cannot_upload(self):
        result = self.run_release("ipad")
        self.assertEqual(result.returncode, 0, result.stderr)
        call = json.loads(self.log.read_text())
        self.assertEqual(call[0], "archive")
        self.assertIn("CODE_SIGNING_ALLOWED=NO", call)
        self.assertNotIn("-exportArchive", call)

    def test_signed_archive_still_requires_explicit_publish(self):
        result = self.run_release("ipad", team="TEAM")
        self.assertEqual(result.returncode, 0, result.stderr)
        call = json.loads(self.log.read_text())
        self.assertEqual(call[0], "archive")
        self.assertIn("DEVELOPMENT_TEAM=TEAM", call)
        self.assertFalse((self.archive.parent / "export.plist").exists())

    def test_failed_archive_preserves_diagnostics_and_exit_status(self):
        self.environment["CAPY_TEST_XCODE_STATUS"] = "65"
        result = self.run_release("ipad")
        self.assertEqual(result.returncode, 65, result.stderr)
        self.assertIn("fixture archive diagnostic", (self.archive.parent / "archive.log").read_text())
        self.assertIn("Apple release failed at line", result.stderr)
        self.assertIn("exit 65", result.stderr)
        self.assertFalse((self.archive.parent / "export.plist").exists())

    def test_store_exports_preserve_archives_and_upload_only_when_requested(self):
        for platform, scheme in [('ipad', 'CapyCanvas-iPad'), ('mac-store', 'CapyCanvas-Mac')]:
            archive = self.root / f'dist/apple-{platform}/{scheme}.xcarchive'
            archive.mkdir(parents=True)
            marker = archive / 'preserved'
            marker.write_bytes(b'original archive')
            self.environment['CAPY_TEST_PUBLISH'] = '1'
            for action, destination in [('export', 'export'), ('publish', 'upload')]:
                with self.subTest(platform=platform, action=action):
                    result = self.run_release(platform, action, team='TEAM')
                    self.assertEqual(result.returncode, 0, result.stderr)
                    call = json.loads(self.log.read_text())
                    self.assertEqual(call[0], '-exportArchive')
                    self.assertEqual(Path(call[call.index('-archivePath') + 1]).resolve(), archive)
                    self.assertIn('-allowProvisioningUpdates', call)
                    options = plistlib.loads((archive.parent / 'export.plist').read_bytes())
                    self.assertEqual(options, {'method': 'app-store-connect', 'destination': destination,
                                              'teamID': 'TEAM', 'signingStyle': 'automatic',
                                              'manageAppVersionAndBuildNumber': False})
                    self.assertEqual(marker.read_bytes(), b'original archive')
                    self.assertFalse(self.dmg_log.exists())

    def test_mac_store_archive_is_separate_from_direct_distribution(self):
        for team in (None, 'TEAM'):
            with self.subTest(team=team):
                result = self.run_release('mac-store', team=team)
                self.assertEqual(result.returncode, 0, result.stderr)
                call = json.loads(self.log.read_text())
                archive = self.root / 'dist/apple-mac-store/CapyCanvas-Mac.xcarchive'
                self.assertTrue(archive.is_dir())
                self.assertEqual(call[call.index('-scheme') + 1], 'CapyCanvas-Mac')
                self.assertNotIn('CODE_SIGN_IDENTITY=Developer ID Application', call)
                self.assertIn('CODE_SIGN_IDENTITY=Apple Development' if team else 'CODE_SIGNING_ALLOWED=NO', call)
                self.assertFalse((archive.parent / 'export.plist').exists())
                self.assertFalse(self.dmg_log.exists())
                self.assertFalse((self.root / 'dist/apple-mac').exists())

    def test_unsigned_mac_package_verifies_the_disk_image(self):
        result = self.run_release("mac")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([call[0] for call in json.loads(self.dmg_log.read_text())], ["info", "create", "verify"])
        self.assertEqual((self.root / "dist/capycanvas-1.2.3-macos-arm64.dmg").read_bytes(), b"fixture DMG")

    def test_failed_disk_image_preserves_diagnostics_and_stops_before_verification(self):
        self.environment["CAPY_TEST_DMG_STATUS"] = "17"
        result = self.run_release("mac")
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertIn("fixture DMG create diagnostic", (self.root / "dist/apple-mac/dmg.log").read_text())
        self.assertEqual([call[0] for call in json.loads(self.dmg_log.read_text())], ["info", "create"])
        self.assertIn("exit 17", result.stderr)

    def test_mac_disk_image_capacity_covers_logical_payload_without_following_applications(self):
        self.environment['CAPY_TEST_PAYLOAD_BYTES'] = str(257 * 1048576)
        result = self.run_release('mac')
        self.assertEqual(result.returncode, 0, result.stderr)
        staged = self.root / 'dist/apple-mac/dmg'
        self.assertEqual((staged / 'Capy Canvas.app/Contents/Resources/large sparse asset.car').stat().st_size, 257 * 1048576)
        self.assertEqual(os.readlink(staged / 'Applications'), '/Applications')
        calls = json.loads(self.dmg_log.read_text())
        self.assertEqual([call[0] for call in calls], ['info', 'create', 'verify'])
        create = calls[1]
        self.assertLess(int(create[create.index('-size') + 1][:-1]), 1024)

    def test_failed_disk_image_verification_rejects_the_package(self):
        self.environment["CAPY_TEST_VERIFY_STATUS"] = "73"
        result = self.run_release("mac")
        self.assertEqual(result.returncode, 73, result.stderr)
        self.assertIn("fixture DMG verify diagnostic", (self.root / "dist/apple-mac/dmg.log").read_text())
        self.assertIn("exit 73", result.stderr)

    def test_publish_refuses_missing_credentials_or_archive(self):
        for platform in ('ipad', 'mac-store'):
            for team in (None, 'TEAM'):
                with self.subTest(platform=platform, team=team):
                    result = self.run_release(platform, 'publish', team=team)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(self.log.exists())

    def test_invalid_action_and_mac_upload_are_rejected(self):
        for arguments in (("ipad", "unknown"), ("mac", "publish"), ("mac", "export")):
            with self.subTest(arguments=arguments):
                result = self.run_release(*arguments, team="TEAM")
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
