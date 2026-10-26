import fcntl
import json
import os
from pathlib import Path
import select
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import gpu_pool as pool


class PoolFixture:
    def __init__(self, root):
        self.root = Path(root)
        self.pci = self.root / "pci"
        self.vfio = self.root / "vfio"
        self.state = self.root / "state"
        self.locks = self.root / "locks"
        for path in (self.pci, self.vfio, self.state):
            path.mkdir()
        self.addresses = []

    def add(self, address="0000:03:00.0", group="38", audio=True):
        path = self.pci / address
        path.mkdir()
        (path / "class").write_text("0x030000\n")
        (path / "boot_vga").write_text("0\n")
        driver = self.root / "drivers/vfio-pci"
        driver.mkdir(parents=True, exist_ok=True)
        (path / "driver").symlink_to(driver, target_is_directory=True)
        group_path = self.root / "groups" / group
        (group_path / "devices").mkdir(parents=True)
        (group_path / "devices" / address).symlink_to(path, target_is_directory=True)
        (path / "iommu_group").symlink_to(group_path, target_is_directory=True)
        if audio:
            audio_address = address[:-1] + "1"
            audio_path = self.pci / audio_address
            audio_path.mkdir()
            (audio_path / "class").write_text("0x040300\n")
            (audio_path / "driver").symlink_to(driver, target_is_directory=True)
            (group_path / "devices" / audio_address).symlink_to(audio_path, target_is_directory=True)
        (self.vfio / group).touch()
        self.addresses.append(address)
        (self.state / "gpu-pool.json").write_text(json.dumps({"devices": self.addresses}))
        return path


def contender(path):
    code = """import fcntl, sys
with open(sys.argv[1], 'r+') as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(23)
"""
    return subprocess.run([sys.executable, "-c", code, str(path)], capture_output=True).returncode


class DeviceSafetyTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.fixture = PoolFixture(self.directory.name)
        self.device_path = self.fixture.add()
        patcher = patch.multiple(pool, PCI=self.fixture.pci, VFIO=self.fixture.vfio, LOCKS=self.fixture.locks)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_display_and_all_audio_functions_must_belong_to_one_isolated_group(self):
        device = pool.Device(self.fixture.addresses[0])
        self.assertEqual(device.functions, ["0000:03:00.0", "0000:03:00.1"])
        self.assertTrue(device.bound())
        (self.device_path / "iommu_group/devices/0000:05:00.0").touch()
        with self.assertRaisesRegex(ValueError, "shares its IOMMU group"):
            pool.Device(device.address)

    def test_host_boot_gpu_is_refused(self):
        (self.device_path / "boot_vga").write_text("1\n")
        with self.assertRaisesRegex(ValueError, "host boot GPU"):
            pool.Device(self.fixture.addresses[0])

    def test_connected_host_display_is_refused(self):
        status = self.device_path / "drm/card8/card8-DP-1/status"
        status.parent.mkdir(parents=True)
        status.write_text("connected\n")
        with self.assertRaisesRegex(ValueError, "connected host display"):
            pool.Device(self.fixture.addresses[0])

    def test_non_display_and_missing_iommu_devices_are_refused(self):
        (self.device_path / "class").write_text("0x020000\n")
        with self.assertRaisesRegex(ValueError, "Not a PCI display controller"):
            pool.Device(self.fixture.addresses[0])
        (self.device_path / "class").write_text("0x030200\n")
        (self.device_path / "iommu_group").unlink()
        with self.assertRaisesRegex(ValueError, "no IOMMU group"):
            pool.Device(self.fixture.addresses[0])

    def test_pool_rejects_duplicate_malformed_and_non_function_zero_addresses(self):
        for addresses in ([self.fixture.addresses[0]] * 2, [["nested"]], "0000:03:00.0",
                          ["0000:03:00.1"], ["03:00.0"], ["../../host"]):
            with self.subTest(addresses=addresses):
                (self.fixture.state / "gpu-pool.json").write_text(json.dumps({"devices": addresses}))
                with self.assertRaises(ValueError):
                    pool.devices(self.fixture.state)

    def test_configuration_rejects_wrong_root_and_invalid_resources(self):
        for value in ([], "pool", None, {"cpus": 0}, {"cpus": "16"},
                      {"memory": "0G"}, {"memory": 16}, {"memory": "16GB"}):
            with self.subTest(value=value):
                (self.fixture.state / "gpu-pool.json").write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    pool.configuration(self.fixture.state)

    def test_three_dimensional_display_controller_without_audio_is_supported(self):
        path = self.fixture.add("0000:04:00.0", "39", audio=False)
        (path / "class").write_text("0x030200\n")
        (path / "boot_vga").unlink()
        device = pool.Device("0000:04:00.0")
        self.assertEqual(device.functions, ["0000:04:00.0"])
        self.assertTrue(device.bound())

    def test_unbound_audio_makes_the_whole_gpu_unavailable(self):
        audio = self.fixture.pci / "0000:03:00.1/driver"
        audio.unlink()
        self.assertFalse(pool.Device(self.fixture.addresses[0]).bound())
        with self.assertRaisesRegex(RuntimeError, "No GPU is free"):
            with pool.reserve(self.fixture.state, "review"):
                self.fail("A partially bound IOMMU group was claimed")

    def test_function_one_must_be_audio_before_the_group_can_be_claimed(self):
        path = self.fixture.pci / "0000:03:00.1/class"
        for pci_class in ("0x020000", "0x0c0330", "0x030000"):
            with self.subTest(pci_class=pci_class):
                path.write_text(pci_class)
                with self.assertRaisesRegex(ValueError, "audio"):
                    pool.Device(self.fixture.addresses[0])
                with patch.object(pool, "lock_device") as lock, self.assertRaises(ValueError):
                    with pool.reserve(self.fixture.state, "review"):
                        self.fail("A non-audio sibling was reserved")
                lock.assert_not_called()
        for pci_class in ("0x040100", "0x040300"):
            with self.subTest(pci_class=pci_class):
                path.write_text(pci_class)
                self.assertEqual(pool.Device(self.fixture.addresses[0]).functions,
                                 ["0000:03:00.0", "0000:03:00.1"])

    def test_fuser_errors_never_make_an_unknown_device_appear_free(self):
        device = pool.Device(self.fixture.addresses[0])
        for result in (subprocess.CompletedProcess([], 2, "", "permission denied"),
                       subprocess.CompletedProcess([], 1, "", "device disappeared")):
            with self.subTest(code=result.returncode), patch.object(pool.subprocess, "run", return_value=result):
                with self.assertRaisesRegex(RuntimeError, "Cannot inspect VFIO users"):
                    device.in_use()

    def test_missing_fuser_names_the_required_package(self):
        device = pool.Device(self.fixture.addresses[0])
        with patch.object(pool.subprocess, "run", side_effect=FileNotFoundError):
            with self.assertRaisesRegex(RuntimeError, "psmisc"):
                device.in_use()


class ReservationTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.fixture = PoolFixture(self.directory.name)
        self.fixture.add()
        self.fixture.add("0000:04:00.0", "39")
        patcher = patch.multiple(pool, PCI=self.fixture.pci, VFIO=self.fixture.vfio, LOCKS=self.fixture.locks)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = patch.object(pool.Device, "in_use", return_value=False)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_agents_claim_distinct_cards_and_a_third_claim_cannot_steal_them(self):
        with pool.reserve(self.fixture.state, "first") as (first, _):
            with pool.reserve(self.fixture.state, "second") as (second, _):
                self.assertNotEqual(first.address, second.address)
                self.assertEqual(contender(self.fixture.locks / first.address), 23)
                self.assertEqual(contender(self.fixture.locks / second.address), 23)
                with self.assertRaisesRegex(RuntimeError, "No GPU is free"):
                    with pool.reserve(self.fixture.state, "third"):
                        self.fail("The third agent stole a reservation")
            self.assertEqual(contender(self.fixture.locks / second.address), 0)
        self.assertEqual(contender(self.fixture.locks / first.address), 0)

    def test_claim_is_shared_across_state_directories(self):
        other = self.fixture.root / "other-state"
        other.mkdir()
        (other / "gpu-pool.json").write_text(json.dumps({"devices": [self.fixture.addresses[0]]}))
        with pool.reserve(self.fixture.state, "first", self.fixture.addresses[0]):
            with self.assertRaisesRegex(RuntimeError, "No GPU is free"):
                with pool.reserve(other, "other"):
                    self.fail("Another state directory stole the physical card")

    def test_simultaneous_independent_agents_cannot_receive_the_same_card(self):
        code = """import json, pathlib, sys
sys.path.insert(0, sys.argv[1])
import gpu_pool
root = pathlib.Path(sys.argv[2])
gpu_pool.PCI = root / 'pci'
gpu_pool.VFIO = root / 'vfio'
gpu_pool.LOCKS = root / 'locks'
gpu_pool.Device.in_use = lambda self: False
with gpu_pool.reserve(root / 'state', sys.argv[3]) as (device, lock):
    print(json.dumps({'address': device.address}), flush=True)
    sys.stdin.buffer.read()
"""
        children = []
        try:
            for name in ("first", "second"):
                children.append(subprocess.Popen(
                    [sys.executable, "-c", code, str(Path(pool.__file__).parent), str(self.fixture.root), name],
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
            addresses = []
            for child in children:
                ready, _, _ = select.select([child.stdout], [], [], 5)
                self.assertTrue(ready, "The reservation contender did not finish within five seconds")
                line = child.stdout.readline()
                self.assertTrue(line, "The reservation contender exited before claiming a card")
                addresses.append(json.loads(line)["address"])
            self.assertEqual(set(addresses), set(self.fixture.addresses))
            with self.assertRaisesRegex(RuntimeError, "No GPU is free"):
                with pool.reserve(self.fixture.state, "third"):
                    self.fail("The third agent stole an independent process's reservation")
        finally:
            for child in children:
                child.stdin.close()
            for child in children:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.terminate()
                    child.wait(timeout=5)
                child.stdout.close()
                child.stderr.close()
        for address in self.fixture.addresses:
            self.assertEqual(contender(self.fixture.locks / address), 0)

    def test_failed_launch_releases_the_reservation(self):
        address = self.fixture.addresses[0]
        with self.assertRaisesRegex(RuntimeError, "launch failed"):
            with pool.reserve(self.fixture.state, "failed", address):
                raise RuntimeError("launch failed")
        with pool.reserve(self.fixture.state, "retry", address) as (device, _):
            self.assertEqual(device.address, address)

    def test_external_vfio_owner_is_skipped_without_stealing_the_card(self):
        first = self.fixture.addresses[0]
        with patch.object(pool.Device, "in_use", lambda device: device.address == first):
            with pool.reserve(self.fixture.state, "review") as (device, _):
                self.assertEqual(device.address, self.fixture.addresses[1])
                rows = pool.describe(self.fixture.state)
                self.assertEqual(rows[0]["status"], "external")
        self.assertEqual(contender(self.fixture.locks / first), 0)

    def test_inspection_failure_releases_the_attempted_lock(self):
        address = self.fixture.addresses[0]
        with patch.object(pool.Device, "in_use", side_effect=RuntimeError("cannot inspect owner")):
            with self.assertRaisesRegex(RuntimeError, "cannot inspect owner"):
                with pool.reserve(self.fixture.state, "review", address):
                    self.fail("An unknown owner was treated as free")
        self.assertEqual(contender(self.fixture.locks / address), 0)

    def test_qemu_child_can_retain_claim_after_launcher_closes_its_copy(self):
        address = self.fixture.addresses[0]
        child = None
        try:
            with pool.reserve(self.fixture.state, "review", address) as (_, lock):
                child = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.buffer.read()"],
                                         stdin=subprocess.PIPE, pass_fds=(lock.fileno(),))
            self.assertEqual(contender(self.fixture.locks / address), 23)
            child.stdin.close()
            child.wait(timeout=5)
            self.assertEqual(contender(self.fixture.locks / address), 0)
        finally:
            if child is not None and child.poll() is None:
                child.terminate()
                child.wait(timeout=5)

    def test_status_reports_owner_only_while_the_lock_is_live(self):
        with pool.reserve(self.fixture.state, "paint", self.fixture.addresses[0]):
            row = pool.describe(self.fixture.state)[0]
            self.assertEqual(row["status"], "reserved")
            self.assertEqual(row["owner"], {"vm": "paint", "state": str(self.fixture.state.resolve())})
        row = pool.describe(self.fixture.state)[0]
        self.assertEqual(row["status"], "free")
        self.assertIsNone(row["owner"])

    def test_lock_file_symlink_cannot_overwrite_an_unrelated_file(self):
        self.fixture.locks.mkdir()
        target = self.fixture.root / "unrelated"
        target.write_text("keep")
        (self.fixture.locks / self.fixture.addresses[0]).symlink_to(target)
        with self.assertRaises(OSError):
            with pool.reserve(self.fixture.state, "review"):
                self.fail("A symlink was followed")
        self.assertEqual(target.read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
