import fcntl
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPT = Path(__file__).resolve().parent / "devices.py"
LISTING = """List of devices attached
LOW0001                device usb:1-1 product:Ares model:9465X device:Ares transport_id:1
MID0001                device usb:1-2 product:RosePlus model:DTHA116 device:RosePlus transport_id:2
TOP0001                device usb:1-3 product:pro model:DTHA140 device:pro transport_id:3
OFF0001                unauthorized usb:1-4 transport_id:4
"""


def load_module():
    spec = importlib.util.spec_from_file_location("devices", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DevicesTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.state = Path(self.directory.name)
        adb = self.state / "adb"
        adb.write_text(f"#!/bin/sh\ncat <<'EOF'\n{LISTING}EOF\n")
        adb.chmod(0o755)
        self.environment = dict(os.environ, ADB=str(adb), CAPY_DEVICE_STATE_DIR=str(self.state))

    def tearDown(self):
        self.directory.cleanup()

    def devices(self, owner, *arguments):
        environment = dict(self.environment, CAPY_DEVICE_OWNER=owner)
        return subprocess.run([sys.executable, SCRIPT, *arguments], env=environment,
                              capture_output=True, text=True)

    def test_list_names_attached_tablets_by_model_and_tier(self):
        output = self.devices("alpha", "list").stdout
        self.assertRegex(output, r"tcl\s+9465X\s+low tier\s+LOW0001\s+free")
        self.assertRegex(output, r"movinkpad11\s+DTHA116\s+mid tier")
        self.assertRegex(output, r"movinkpad14\s+DTHA140\s+top tier")
        self.assertNotIn("OFF0001", output)

    def test_reservation_excludes_other_owners_until_released(self):
        self.assertEqual(self.devices("alpha", "reserve", "movinkpad14", "--note", "photo").returncode, 0)
        refused = self.devices("beta", "reserve", "movinkpad14")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("reserved by alpha", refused.stderr)
        self.assertEqual(self.devices("alpha", "reserve", "TOP0001", "--hours", "8").returncode, 0)
        self.devices("beta", "release")
        self.assertIn("reserved by alpha", self.devices("gamma", "list").stdout)
        self.devices("alpha", "release")
        self.assertEqual(self.devices("beta", "reserve", "movinkpad14").returncode, 0)

    def test_tier_reservation_picks_a_free_tablet_of_that_tier(self):
        self.assertIn("movinkpad11", self.devices("alpha", "reserve", "--tier", "mid").stdout)
        refused = self.devices("beta", "reserve", "--tier", "mid")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("No free mid tier tablet", refused.stderr)

    def test_expired_reservations_are_ignored(self):
        record = {"owner": "alpha", "note": "", "expires": time.time() - 1}
        (self.state / "capy-TOP0001.reservation.json").write_text(json.dumps(record))
        self.assertEqual(self.devices("beta", "reserve", "movinkpad14").returncode, 0)

    def test_run_refuses_a_tablet_reserved_by_another_owner(self):
        self.devices("alpha", "reserve", "tcl")
        refused = self.devices("beta", "run", "tcl", "--", "true")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("reserved by alpha", refused.stderr)

    def test_run_requires_a_current_reservation(self):
        refused = self.devices("alpha", "run", "tcl", "--", "true")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("Reserve tcl", refused.stderr)
        self.devices("alpha", "reserve", "tcl", "--hours", "-1")
        refused = self.devices("alpha", "run", "tcl", "--", "true")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("Reserve tcl", refused.stderr)

    def test_run_holds_the_lock_and_exports_the_device(self):
        self.devices("capycanvas-2", "reserve", "movinkpad11")
        probe = (
            "import fcntl, os, sys\n"
            "lock = open(sys.argv[1], 'a+')\n"
            "try:\n"
            "    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)\n"
            "    print('unlocked')\n"
            "except BlockingIOError:\n"
            "    print('locked')\n"
            "print(os.environ['ANDROID_SERIAL'], os.environ['CAPY_ANDROID_SERIAL'],"
            " os.environ['CAPY_APPLICATION_ID'])\n"
        )
        lock = self.state / "capy-MID0001.lock"
        result = self.devices("capycanvas-2", "run", "movinkpad11", "--",
                              sys.executable, "-c", probe, str(lock))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split(), ["locked", "MID0001", "MID0001", "art.capycanvas.capycanvas_2"])
        with open(lock, "a+") as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_run_returns_the_command_status(self):
        self.devices("alpha", "reserve", "tcl")
        self.assertEqual(self.devices("alpha", "run", "tcl", "--", "false").returncode, 1)

    def test_appid_prints_the_owner_application_id(self):
        self.assertEqual(self.devices("capycanvas-2", "appid").stdout.strip(), "art.capycanvas.capycanvas_2")

    def test_application_ids_are_valid_package_names(self):
        devices = load_module()
        self.assertEqual(devices.application_id("capycanvas1"), "art.capycanvas.capycanvas1")
        self.assertEqual(devices.application_id("agent-a02513e0"), "art.capycanvas.agent_a02513e0")
        self.assertEqual(devices.application_id("2nd tree"), "art.capycanvas.w2nd_tree")

    def test_owners_get_distinct_stable_ports(self):
        devices = load_module()
        self.assertEqual(devices.ports("capycanvas1"), devices.ports("capycanvas1"))
        self.assertNotEqual(devices.ports("capycanvas1"), devices.ports("capycanvas2"))

    def test_apple_reservation_and_device_environment(self):
        listing = {"result": {"devices": [
            {"hardwareProperties": {"deviceType": "iPad", "reality": "physical", "productType": "iPad16,6", "udid": "IPAD0001"},
             "connectionProperties": {"pairingState": "paired"}},
            {"hardwareProperties": {"deviceType": "iPhone", "reality": "physical", "udid": "PHONE0001"},
             "connectionProperties": {"pairingState": "paired"}},
            {"hardwareProperties": {"deviceType": "iPad", "reality": "physical", "udid": "UNPAIRED0001"},
             "connectionProperties": {"pairingState": "unpaired"}},
            {"hardwareProperties": {"deviceType": "iPad", "reality": "simulated", "udid": "SIM0001"},
             "connectionProperties": {"pairingState": "paired"}},
        ]}}
        xcrun = self.state / "xcrun"
        xcrun.write_text(f"#!{sys.executable}\nimport pathlib, sys\npathlib.Path(sys.argv[-1]).write_text({json.dumps(listing)!r})\n")
        xcrun.chmod(0o755)
        self.environment["PATH"] = str(self.state) + os.pathsep + os.environ["PATH"]
        output = self.devices("alpha", "--platform", "apple", "list").stdout
        self.assertRegex(output, r"ipad\s+iPad16,6\s+no tier\s+IPAD0001\s+free")
        self.assertNotIn("PHONE0001", output)
        self.assertNotIn("UNPAIRED0001", output)
        self.assertNotIn("SIM0001", output)
        refused = self.devices("alpha", "--platform", "apple", "run", "ipad", "--", "true")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("Reserve ipad", refused.stderr)
        reserved = self.devices("alpha", "--platform", "apple", "reserve", "ipad")
        self.assertEqual(reserved.returncode, 0, reserved.stderr)
        refused = self.devices("beta", "--platform", "apple", "run", "IPAD0001", "--", "true")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("reserved by alpha", refused.stderr)
        probe = "import os; print(os.environ['CAPY_APPLE_DEVICE_ID'], os.environ['CAPY_APPLE_BUNDLE_ID'])"
        result = self.devices("alpha", "--platform", "apple", "run", "ipad", "--", sys.executable, "-c", probe)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "IPAD0001 art.capycanvas.alpha")
        self.devices("alpha", "--platform", "apple", "release", "ipad")
        self.assertIn("free", self.devices("beta", "--platform", "apple", "list").stdout)

    def test_ambiguous_device_names_require_a_serial(self):
        devices = load_module()
        tablets = [devices.Device(serial, "iPad16,6", "apple") for serial in ["IPAD0001", "IPAD0002"]]
        with self.assertRaisesRegex(SystemExit, "use its serial"):
            devices.find(tablets, "ipad")
        self.assertEqual(devices.find(tablets, "IPAD0002").serial, "IPAD0002")


if __name__ == "__main__":
    unittest.main()
