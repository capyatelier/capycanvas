import importlib.util
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import gpu_pool as pool
from test_gpu_pool import PoolFixture


spec = importlib.util.spec_from_file_location("gpu_host", Path(__file__).with_name("gpu-host.py"))
host = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host)


class ManagedFileTests(unittest.TestCase):
    def test_rewriting_a_shorter_managed_file_removes_old_tail_and_sets_mode(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "nested/config"
            host.write_file(path, "old value\n" * 50)
            host.write_file(path, "new\n", 0o600)
            self.assertEqual(path.read_text(), "new\n")
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_managed_writer_refuses_a_symlink_without_changing_its_target(self):
        with TemporaryDirectory() as directory:
            target = Path(directory) / "unrelated"
            target.write_text("keep")
            path = Path(directory) / "config"
            path.symlink_to(target)
            with self.assertRaises(OSError):
                host.write_file(path, "replace", 0o600)
            self.assertEqual(target.read_text(), "keep")


class HostSizingTests(unittest.TestCase):
    def test_defaults_share_host_resources_and_cap_large_hosts(self):
        for cpus, gib, cards, expected in ((192, 512, 3, (32, "64G")),
                                            (16, 64, 3, (4, "16G")),
                                            (8, 16, 1, (4, "8G"))):
            def sysconf(name):
                return gib * 1024 ** 3 // 4096 if name == "SC_PHYS_PAGES" else 4096
            with self.subTest(cpus=cpus, gib=gib, cards=cards), \
                    patch.object(host.os, "cpu_count", return_value=cpus), \
                    patch.object(host.os, "sysconf", side_effect=sysconf):
                self.assertEqual(host.resource_defaults(cards), expected)

    def test_permission_rules_follow_selected_pci_devices_across_group_renumbering(self):
        devices = [SimpleNamespace(address="0000:03:00.0", group="38"),
                   SimpleNamespace(address="0000:04:00.0", group="64")]
        rules = host.permission_rules(devices, 1234)
        self.assertEqual(len(rules.splitlines()), 2)
        for device in devices:
            self.assertIn(f'TEST=="/sys/kernel/iommu_groups/%k/devices/{device.address}"', rules)
        self.assertEqual(rules.count('RUN+="/usr/bin/setfacl -m u:1234:rw /dev/vfio/%k"'), 2)
        self.assertNotIn("OWNER=", rules)
        self.assertNotIn("GROUP=", rules)
        self.assertNotIn("MODE=", rules)
        self.assertNotIn("/38/", rules)
        self.assertNotIn("/64/", rules)


class HostConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.fixture = PoolFixture(self.directory.name)
        self.first = self.fixture.add()
        self.second = self.fixture.add("0000:04:00.0", "39")
        self.user = SimpleNamespace(pw_uid=os.getuid(), pw_gid=os.getgid(), pw_name="test-user")
        self.args = SimpleNamespace(user=self.user.pw_name, state_dir=self.fixture.state,
                                    devices=self.fixture.addresses, cpus=None, memory=None,
                                    artifacts=self.fixture.root / "large-output", apply=True)
        self.calls = []
        self.writes = []
        self.real_writer = host.write_file
        self.patches = [
            patch.multiple(pool, PCI=self.fixture.pci, VFIO=self.fixture.vfio, LOCKS=self.fixture.locks),
            patch.object(host.shutil, "which", side_effect=lambda name: "/usr/bin/" + name),
            patch.object(host.pwd, "getpwnam", return_value=self.user),
            patch.object(host.os, "geteuid", return_value=0),
            patch.object(host.os, "chown"),
            patch.object(host.os, "chmod"),
            patch.object(host, "run", side_effect=self.command),
            patch.object(host, "write_file", side_effect=self.write),
            patch("builtins.print"),
        ]
        for item in self.patches:
            item.start()
            self.addCleanup(item.stop)

    def command(self, *args, **kwargs):
        self.calls.append(tuple(map(str, args)))
        if args[0] == "driverctl":
            address = args[2]
            driver = self.fixture.pci / address / "driver"
            driver.unlink(missing_ok=True)
            driver.symlink_to(self.fixture.root / "drivers/vfio-pci", target_is_directory=True)
        result = "23456\n" if args[:2] == ("systemctl", "show") else ""
        return subprocess.CompletedProcess(args, 0, result, "")

    def write(self, path, content, mode=0o644, owner=None):
        path = Path(path)
        self.writes.append((str(path), content, mode, owner))
        destination = self.fixture.root / "etc" / path.relative_to("/etc") if path.is_relative_to("/etc") else path
        self.real_writer(destination, content, mode, owner)

    def bind_native(self, path):
        driver = path / "driver"
        driver.unlink()
        native = self.fixture.root / "drivers/nvidia"
        native.mkdir(exist_ok=True)
        driver.symlink_to(native, target_is_directory=True)
        audio = self.fixture.pci / (path.name[:-1] + "1") / "driver"
        audio.unlink()
        audio.symlink_to(native, target_is_directory=True)

    def test_existing_live_vfio_card_is_persisted_without_rebinding_or_triggering_it(self):
        self.bind_native(self.second)
        seen = []
        def holders(device):
            seen.append(device.address)
            if device.address == self.first.name:
                self.fail("The live VFIO device must not be probed for host users")
            return []
        with patch.object(host, "holders", side_effect=holders):
            host.configure(self.args)
        rebind = [call for call in self.calls if call[0] == "driverctl"]
        self.assertEqual(rebind, [("driverctl", "set-override", "0000:04:00.1", "vfio-pci"),
                                  ("driverctl", "set-override", "0000:04:00.0", "vfio-pci")])
        self.assertFalse(any(call[:2] == ("udevadm", "trigger") for call in self.calls))
        for address in ("0000:03:00.0", "0000:03:00.1"):
            persisted = self.fixture.root / "etc/driverctl.d" / f"pci-{address}"
            self.assertEqual(persisted.read_text(), "vfio-pci\n")
        saved = json.loads((self.fixture.state / "gpu-pool.json").read_text())
        self.assertEqual(saved["devices"], self.fixture.addresses)
        self.assertIn(("prlimit", "--pid", "23456", "--memlock=unlimited:unlimited"), self.calls)

    def test_busy_native_gpu_aborts_before_driver_or_configuration_changes(self):
        self.bind_native(self.first)
        self.bind_native(self.second)
        with patch.object(host, "holders", side_effect=lambda device: [123] if device.address == self.second.name else []), \
                self.assertRaisesRegex(RuntimeError, "no devices detached"):
            host.configure(self.args)
        self.assertEqual(self.writes, [])
        self.assertFalse(any(call[0] in ("driverctl", "modprobe", "prlimit", "udevadm") for call in self.calls))
        self.assertEqual((self.first / "driver").resolve().name, "nvidia")
        self.assertEqual((self.second / "driver").resolve().name, "nvidia")

    def test_device_permissions_are_changed_only_for_selected_groups(self):
        self.args.devices = list(self.fixture.addresses)
        self.fixture.add("0000:05:00.0", "40")
        host.configure(self.args)
        owners = {Path(call.args[0]) for call in host.os.chown.call_args_list}
        self.assertEqual(owners, {self.args.artifacts})
        self.assertNotIn(self.fixture.vfio / "40", owners)
        self.assertEqual(host.os.chmod.call_args_list, [])
        grants = [call for call in self.calls if call[0] == "setfacl"]
        self.assertEqual(grants, [("setfacl", "-m", f"u:{self.user.pw_uid}:rw", str(self.fixture.vfio / group))
                                 for group in ("38", "39")])

    def test_driverctl_success_without_a_real_binding_never_publishes_configuration(self):
        self.bind_native(self.second)
        before = (self.fixture.state / "gpu-pool.json").read_bytes()
        with patch.object(host, "holders", return_value=[]), patch.object(host, "run", return_value=(
                subprocess.CompletedProcess([], 0, "23456\n", ""))), \
                self.assertRaisesRegex(RuntimeError, "did not bind to VFIO"):
            host.configure(self.args)
        self.assertEqual((self.fixture.state / "gpu-pool.json").read_bytes(), before)
        self.assertFalse(any(Path(path) == self.fixture.state / "gpu-pool.json" for path, *_ in self.writes))

    def test_read_only_plan_never_changes_binding_or_managed_files(self):
        self.args.apply = False
        self.bind_native(self.second)
        with patch.object(host, "holders", return_value=[123]):
            host.configure(self.args)
        self.assertEqual(self.calls, [])
        self.assertEqual(self.writes, [])
        self.assertEqual((self.second / "driver").resolve().name, "nvidia")

    def test_host_display_or_unrelated_iommu_member_prevents_all_mutation(self):
        for invalid in ("display", "group"):
            with self.subTest(invalid=invalid):
                if invalid == "display":
                    status = self.second / "drm/card9/card9-DP-1/status"
                    status.parent.mkdir(parents=True)
                    status.write_text("connected\n")
                else:
                    status.unlink()
                    (self.second / "iommu_group/devices/unrelated").touch()
                with self.assertRaises(ValueError):
                    host.configure(self.args)
                self.assertEqual(self.calls, [])
                self.assertEqual(self.writes, [])

    def test_missing_prerequisites_and_root_authority_fail_before_changes(self):
        for missing in ("fuser", "setfacl", "driverctl"):
            with self.subTest(missing=missing), patch.object(host.shutil, "which", side_effect=(
                    lambda name: None if name == missing else "/usr/bin/" + name)), \
                    self.assertRaises(RuntimeError):
                host.configure(self.args)
            self.assertEqual(self.calls, [])
            self.assertEqual(self.writes, [])
        with patch.object(host.os, "geteuid", return_value=self.user.pw_uid or 1234), \
                self.assertRaisesRegex(RuntimeError, "needs root"):
            host.configure(self.args)
        self.assertEqual(self.writes, [])

    def test_non_audio_function_one_prevents_all_host_changes(self):
        (self.fixture.pci / (self.second.name[:-1] + "1") / "class").write_text("0x0c0330\n")
        with self.assertRaisesRegex(ValueError, "not an audio"):
            host.configure(self.args)
        self.assertEqual(self.calls, [])
        self.assertEqual(self.writes, [])

    def test_unknown_fuser_result_cannot_be_treated_as_an_idle_device(self):
        device = pool.Device(self.second.name)
        with patch.object(host, "device_nodes", return_value=[self.fixture.vfio / "39"]), \
                patch.object(host.subprocess, "run", return_value=subprocess.CompletedProcess(
                    [], 1, "", "permission denied")), self.assertRaisesRegex(RuntimeError, "Cannot inspect GPU users"):
            host.holders(device)

    def test_native_driver_without_inspectable_nodes_is_not_assumed_idle(self):
        self.bind_native(self.second)
        device = pool.Device(self.second.name)
        with patch.object(host, "device_nodes", return_value=[]), \
                self.assertRaisesRegex(RuntimeError, "No inspectable device nodes"):
            host.holders(device)

    def test_host_holders_collect_unique_pids_without_treating_fuser_headers_as_errors(self):
        device = pool.Device(self.second.name)
        with patch.object(host, "device_nodes", return_value=[self.fixture.vfio / "39"]), \
                patch.object(host.subprocess, "run", return_value=subprocess.CompletedProcess(
                    [], 0, " 456 123 456 ", "/dev/vfio/39:")):
            self.assertEqual(host.holders(device), [123, 456])


if __name__ == "__main__":
    unittest.main()
