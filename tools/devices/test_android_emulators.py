import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("emulators", Path(__file__).with_name("android-emulators.py"))
emulators = importlib.util.module_from_spec(spec)
spec.loader.exec_module(emulators)


class EmulatorTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.harness = emulators.Harness(self.root, Path("/sdk"))
        self.record = {"pid": 123, "start_ticks": "456", "serial": "emulator-5560", "port": 5560, "profile": "phone412", "avd": "probe412"}
        self.harness.state["emulators"].append(self.record)

    def start(self, fresh):
        self.harness.state["emulators"] = []
        events = []
        def spawn(command, name, env):
            events.append(("spawn", name))
            return {"pid": len(events), "start_ticks": name}
        def boot(record):
            self.assertIn(record, json.loads(self.harness.path.read_text())["emulators"])
            events.append(("boot", record["serial"]))
        with patch.object(self.harness, "display", return_value={}), patch.object(self.harness, "avd", return_value=fresh), patch.object(self.harness, "spawn", side_effect=spawn), patch.object(self.harness, "boot", side_effect=boot), patch.object(emulators, "free_port", return_value=True):
            self.harness.start(3, "phone412", 5560)
        self.assertEqual([r["serial"] for r in json.loads(self.harness.path.read_text())["emulators"]], ["emulator-5560", "emulator-5562", "emulator-5564"])
        return [kind for kind, _ in events]

    def test_start_records_processes_before_waiting_for_boot(self):
        self.assertEqual(self.start(False), ["spawn"] * 3 + ["boot"] * 3)

    def test_fresh_avd_boots_one_instance_before_sharing_it(self):
        self.assertEqual(self.start(True), ["spawn", "boot"] + ["spawn"] * 2 + ["boot"] * 2)

    def test_avd_config_uses_the_api_35_image_and_refuses_stale_directories(self):
        self.assertTrue(self.harness.avd("phone393"))
        config = dict(line.split("=", 1) for line in (self.root / "avd/phone393.avd/config.ini").read_text().splitlines())
        self.assertEqual((config["image.sysdir.1"], config["hw.lcd.width"], config["hw.lcd.height"], config["hw.lcd.density"]),
                         ("system-images/android-35/google_apis_playstore_tablet/x86_64/", "1080", "2340", "420"))
        self.assertIn("path=" + str(self.root / "avd/phone393.avd"), (self.root / "avd/phone393.ini").read_text())
        (self.root / "avd/phone393.avd/userdata-qemu.img").touch()
        self.assertFalse(self.harness.avd("phone393"))
        (self.root / "avd/phone393.avd/config.ini").write_text("image.sysdir.1=system-images/android-37.0/google_apis/x86_64/\n")
        with self.assertRaisesRegex(RuntimeError, "another configuration"):
            self.harness.avd("phone393")

    def boot(self, values, polls):
        calls = []
        polls, values = iter(polls), iter(values)
        def adb(serial, *args, check=True):
            calls.append(args)
            if args[-1].startswith("getprop sys.boot_completed"):
                return next(polls)
            return next(values) if args[1:3] == ("device_config", "get") else ""
        with patch.object(emulators, "alive", return_value=True), patch.object(emulators.time, "sleep"), patch.object(self.harness, "adb", side_effect=adb):
            self.harness.boot(self.record)
        return [args for args in calls if not args[-1].startswith("getprop")]

    def test_boot_disables_desktop_windowing_and_waits_for_the_next_boot(self):
        get = ("shell", "device_config", "get", *emulators.DESKTOP_WINDOWING)
        self.assertEqual(self.boot(["true", "false"], ["", "1 a", "1 a", "1 b"]), [
            get, ("shell", "device_config", "put", *emulators.DESKTOP_WINDOWING, "false"), ("shell", "svc", "power", "reboot"), get])
        self.assertEqual(self.boot(["false"], ["1 a"]), [get])

    def test_boot_fails_when_desktop_windowing_survives_the_reboot(self):
        with self.assertRaisesRegex(RuntimeError, "kept desktop windowing"):
            self.boot(["true", "true"], ["1 a", "1 b"])

    def test_refuses_physical_unrecorded_and_reused_processes(self):
        with patch.object(emulators, "run") as run, patch.object(emulators, "alive", return_value=False):
            for serial in ("physical-tablet", "emulator-5554", "emulator-5560"):
                with self.assertRaises(ValueError):
                    self.harness.adb(serial, "shell", "getprop")
            run.assert_not_called()

    def test_stop_only_uses_recorded_serial_and_display_pids(self):
        self.harness.state["display"] = [{"pid": 10}, {"pid": 11}]
        with patch.object(emulators, "alive", return_value=True), patch.object(emulators, "wait_for"), patch.object(self.harness, "adb") as adb, patch.object(self.harness, "terminate") as terminate:
            self.harness.stop()
        adb.assert_called_once_with("emulator-5560", "emu", "kill")
        self.assertEqual([call.args[0]["pid"] for call in terminate.call_args_list], [11, 10])
        self.assertEqual(json.loads(self.harness.path.read_text())["emulators"], [])

    def test_timeout_terminates_only_recorded_emulator(self):
        with patch.object(emulators, "alive", return_value=True), patch.object(emulators, "wait_for", side_effect=RuntimeError("timeout")), patch.object(self.harness, "adb"), patch.object(self.harness, "terminate") as terminate:
            self.harness.stop()
        terminate.assert_called_once_with(self.record)

    def configure(self, size="412x891", density="160", font="1.0", navigation="2"):
        def adb(serial, *args):
            return {("shell", "wm", "size"): "Physical size: 412x891\nOverride size: " + size,
                    ("shell", "wm", "density"): "Physical density: " + density,
                    ("shell", "settings", "get", "system", "font_scale"): font,
                    ("shell", "settings", "get", "secure", "navigation_mode"): navigation}.get(args, "")
        return patch.object(self.harness, "adb", side_effect=adb)

    def test_configure_verifies_readback(self):
        with self.configure(navigation="0") as adb:
            self.harness.configure("emulator-5560", "412x891", "light", navigation="threebutton")
        for name in ("window_animation_scale", "transition_animation_scale", "animator_duration_scale"):
            self.assertIn(("emulator-5560", "shell", "settings", "put", "global", name, "0"), [call.args for call in adb.call_args_list])
        self.assertEqual(json.loads(self.harness.path.read_text())["emulators"][0]["configuration"]["density"], 160)
        self.assertIn(("emulator-5560", "shell", "cmd", "overlay", "enable", "com.android.internal.systemui.navbar.threebutton"), [call.args for call in adb.call_args_list])
        self.assertIn(("emulator-5560", "shell", "cmd", "overlay", "disable", "com.android.internal.systemui.navbar.gestural"), [call.args for call in adb.call_args_list])
        self.assertEqual(self.record["configuration"]["navigation"], "threebutton")
        for overrides in ({"size": "320x320", "navigation": "0"}, {"density": "420", "navigation": "0"}, {"font": "1.4", "navigation": "0"}, {"navigation": "2"}):
            with self.configure(**overrides), self.assertRaisesRegex(RuntimeError, "readback differs"):
                self.harness.configure("emulator-5560", "412x891", "light", navigation="threebutton")

    def test_gestural_navigation_is_selected_and_verified(self):
        with self.configure(navigation="2") as adb:
            self.harness.configure("emulator-5560", "412x891", "light", navigation="gestural")
        self.assertIn(("emulator-5560", "shell", "cmd", "overlay", "enable", "com.android.internal.systemui.navbar.gestural"), [call.args for call in adb.call_args_list])
        self.assertIn(("emulator-5560", "shell", "cmd", "overlay", "disable", "com.android.internal.systemui.navbar.threebutton"), [call.args for call in adb.call_args_list])
        self.assertEqual(self.record["configuration"]["navigation"], "gestural")
        with self.assertRaisesRegex(ValueError, "navigation"):
            self.harness.configure("emulator-5560", "412x891", "light", navigation="invalid")

    def test_tablet_size_and_port_limits(self):
        with self.configure(size="1280x800"):
            self.harness.configure("emulator-5560", "tablet1280", "dark")
        self.assertEqual(self.record["configuration"]["size"], "1280x800")
        for count, port in ((0, 5560), (1, 5554), (2, 5584), (1, 5561)):
            with self.assertRaises(ValueError):
                self.harness.start(count, "phone412", port)


if __name__ == "__main__":
    unittest.main()
