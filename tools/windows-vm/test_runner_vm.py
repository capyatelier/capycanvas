import importlib.util
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("runner_vm", Path(__file__).with_name("windows-vm.py"))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)


class RunnerVmTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        runtime = self.root / "runtime"
        runtime.mkdir()
        state = self.root / "state"
        state.mkdir()
        base = state / "base"
        base.mkdir()
        (base / "disk.qcow2").touch()
        firmware = state / "firmware.json"
        firmware.write_text(json.dumps({"code": "code.fd", "code_format": "raw", "vars_format": "raw"}))
        patcher = patch.multiple(vm, ROOT=self.root / "worktree", STATE=state, VMS=state / "vms",
                                 RUNTIME=runtime, BASE=base, FIRMWARE=firmware)
        patcher.start()
        self.addCleanup(patcher.stop)
        vm.VMS.mkdir()
        self.primary = vm.Machine("owner", vm.VMS / "owner")

    def test_runner_names_are_owned_and_long_names_remain_distinct(self):
        first = vm.runner_machine(self.primary, 1)
        self.assertEqual(first.name, "owner-r1")
        self.assertEqual(first.runner_owner, {"primary": "owner", "worktree": str(vm.ROOT.resolve())})
        names = [vm.runner_machine(vm.Machine("a" * 31 + suffix, vm.VMS / suffix), 1).name for suffix in ("b", "c")]
        self.assertNotEqual(*names)
        self.assertTrue(all(len(name) <= 32 for name in names))

    def test_existing_ordinary_or_foreign_vm_is_never_adopted(self):
        path = vm.VMS / "owner-r1"
        path.mkdir()
        with self.assertRaisesRegex(SystemExit, "not an owned CPU runner"):
            vm.runner_machine(self.primary, 1)
        (path / "runner.json").write_text(json.dumps({"primary": "owner", "worktree": "another-worktree"}))
        with self.assertRaisesRegex(SystemExit, "creating worktree"):
            vm.boot_locked(vm.runner_machine(self.primary, 1), False, None)

    def test_qemu_retains_cpu_slot_descriptor_and_uses_pool_size(self):
        machine = vm.runner_machine(self.primary, 1)
        machine.directory.mkdir()
        with open(self.root / "reservation", "w+") as reservation, patch.object(vm, "terminate"), \
                patch.object(vm, "free_port", return_value=2200), \
                patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            vm.launch(machine, reservation=reservation, cpus=4, memory="8G")
            call = next(call for call in run.call_args_list if call.args[0] == "qemu-system-x86_64")
            self.assertEqual(call.kwargs["pass_fds"], (reservation.fileno(),))
            self.assertIn(f"fd={reservation.fileno()},set=99,opaque=capycanvas-runner-owner-r1", call.args)
            self.assertIn("cpus=4,sockets=1,cores=4,threads=1", call.args)
            self.assertEqual(call.args[call.args.index("-m") + 1], "8G")
            self.assertFalse(any(str(arg).startswith("vfio-pci") for arg in call.args))

    def test_foreign_runner_cannot_boot_or_reset(self):
        machine = vm.Machine("foreign-r1", vm.VMS / "foreign-r1")
        machine.directory.mkdir()
        machine.runner.write_text(json.dumps({"primary": "foreign", "worktree": "another"}))
        with self.assertRaisesRegex(SystemExit, "creating worktree"):
            vm.boot_locked(machine, False, None)
        with patch.object(vm, "selected", return_value=machine), self.assertRaisesRegex(SystemExit, "another worktree"):
            vm.reset(SimpleNamespace())

    def test_reset_refuses_live_cpu_lease_even_when_runtime_directory_differs(self):
        machine = vm.runner_machine(self.primary, 1)
        machine.directory.mkdir()
        machine.runner.write_text(json.dumps(machine.runner_owner))
        with patch.object(vm, "selected", return_value=machine), patch.object(vm, "running", return_value=False), \
                patch.object(vm, "runner_reserved", return_value=True), patch.object(vm, "remove") as remove, \
                self.assertRaisesRegex(SystemExit, "Stop the VM first"):
            vm.reset(SimpleNamespace())
        remove.assert_not_called()

    def build_record(self):
        files = {"CapyCanvas.exe": "exe", "layer_windows.dll": "rust", "Assets/icon.svg": "svg",
                 "resources.pri": "pri", "app.runtimeconfig.json": "config"}
        return {"profile": "dev-perf", "hardware_gpu": False, "inputs": {"Cargo.lock": "lock"},
                "files": files, "controls": {kind: files for kind in ("Feedback", "Popup", "LayerDrop")}}

    def test_control_fixtures_build_once_and_no_build_checks_each_hash(self):
        self.primary.directory.mkdir()
        files = self.build_record()["files"]
        with patch.object(vm, "guest") as guest, patch.object(vm, "publish_build_record") as publish, \
                patch.object(vm, "build_files", return_value=files):
            directory, record = vm.fixture_build(self.primary, {"Cargo.lock": "lock"}, False, False, "dev-perf", ("Feedback", "Popup", "LayerDrop"))
            self.assertEqual(guest.call_count, 4)
            self.assertEqual(set(record["controls"]), set(("Feedback", "Popup", "LayerDrop")))
            publish.assert_called_once_with(self.primary, directory, record)
            guest.reset_mock()
            self.assertEqual(vm.fixture_build(self.primary, {"Cargo.lock": "lock"}, True, False, "dev-perf", ("Feedback", "Popup", "LayerDrop")), (directory, record))
            guest.assert_not_called()
        with patch.object(vm, "build_files", side_effect=[files, files, {**files, "resources.pri": "stale"}]), \
                self.assertRaisesRegex(SystemExit, "Popup control fixture"):
            vm.fixture_build(self.primary, {"Cargo.lock": "lock"}, True, False, "dev-perf", ("Feedback", "Popup", "LayerDrop"))

    def test_whole_build_directories_are_shipped(self):
        self.primary.runtime.mkdir(parents=True)
        self.primary.ssh_port.write_text("2200")
        record = self.build_record()
        with patch.object(vm, "run") as run:
            vm.export_fixture_build(self.primary, vm.SOFTWARE_BUILD + "-dev-perf", record, self.root)
        command = run.call_args.args[-1]
        self.assertNotIn(" -T ", command)
        for root in ("SoftwareAdapter-dev-perf", "SoftwareAdapter-dev-perf-Feedback", "SoftwareAdapter-dev-perf-Popup", "SoftwareAdapter-dev-perf-LayerDrop"):
            self.assertIn(root, command)

    def test_plain_navigator_builds_and_validates_no_controls(self):
        self.primary.directory.mkdir()
        files = self.build_record()["files"]
        with patch.object(vm, "guest") as guest, patch.object(vm, "publish_build_record"), \
                patch.object(vm, "build_files", return_value=files) as hashes:
            directory, record = vm.fixture_build(self.primary, {"Cargo.lock": "lock"}, False, False, "dev-perf", controls=[])
            self.assertEqual(guest.call_count, 1)
            self.assertEqual(record["controls"], {})
            hashes.reset_mock()
            vm.fixture_build(self.primary, {"Cargo.lock": "lock"}, True, False, "dev-perf", controls=[])
            hashes.assert_called_once_with(self.primary, directory)

    def test_import_replaces_owned_runner_build_directories_and_rejects_stale_hashes(self):
        machine = vm.runner_machine(self.primary, 1)
        machine.directory.mkdir()
        archive = self.root / "build.tar.gz"
        archive.touch()
        record = self.build_record()
        with patch.object(vm, "guest", return_value=SimpleNamespace(stdout="False\n")) as guest, \
                patch.object(vm, "run"), patch.object(vm, "ssh_command", return_value=["ssh"]), \
                patch.object(vm, "publish_build_record") as publish, \
                patch.object(vm, "build_files", return_value=record["files"]):
            vm.import_fixture_build(machine, vm.SOFTWARE_BUILD + "-dev-perf", record, archive, record["inputs"], "dev-perf")
        publish.assert_called_once_with(machine, vm.SOFTWARE_BUILD + "-dev-perf", record)
        removals = [call.args[1] for call in guest.call_args_list if "Remove-Item" in call.args[1]]
        self.assertEqual(len(removals), 4)
        path = vm.build_record_path(machine, False, "dev-perf")
        self.assertTrue(path.exists())
        with patch.object(vm, "guest"), patch.object(vm, "run"), patch.object(vm, "ssh_command", return_value=["ssh"]), \
                patch.object(vm, "build_files", return_value={}), self.assertRaises(SystemExit):
            vm.import_fixture_build(machine, vm.SOFTWARE_BUILD + "-dev-perf", record, archive, record["inputs"], "dev-perf")
        self.assertFalse(path.exists())

    def test_sharded_cli_rejects_gpu_and_requires_explicit_sharding_for_keep(self):
        with patch.object(vm, "boot") as boot:
            with self.assertRaisesRegex(ValueError, "cannot be used"):
                vm.sharded_fixtures(SimpleNamespace(hardware_gpu=True))
            with self.assertRaisesRegex(ValueError, "requires --runners"):
                vm.fixtures(SimpleNamespace(keep_runners=True))
            boot.assert_not_called()
        with patch.object(vm.sys, "argv", ["windows-vm.py", "fixtures", "--runners", "3", "--keep-runners", "popup:default"]), \
                patch.object(vm, "fixtures") as fixtures:
            vm.main()
        args = fixtures.call_args.args[0]
        self.assertEqual(args.runners, 3)
        self.assertTrue(args.keep_runners)
        self.assertEqual(args.names, ["popup:default"])

    def test_shards_reuse_one_build_preserve_defaults_and_release_partial_capacity(self):
        self.primary.directory.mkdir()
        artifacts = self.root / "evidence"
        args = SimpleNamespace(vm="owner", names=["popup:default", "header:pen"], runners=2, keep_runners=False,
                               hardware_gpu=False, no_build=True, profile="dev-perf", timeout_minutes=20)
        record = self.build_record()
        failed = False
        clock = [0]
        def shutdown(machine):
            clock[0] += 37
        def boot(machine, **options):
            if machine.name.endswith("-r2"):
                raise vm.cpu_pool.Unavailable("pool full")
        def execute(machine, args, directory, provenance, names):
            self.assertEqual(names, ["popup:default", "header:pen"])
            return {"vm": machine.name, "results": [{"name": "popup", "exit": int(failed)}, {"name": "header:pen", "exit": 0}],
                    "complete": True, "failure": None, "evidence": "shard-evidence"}
        with patch.object(vm, "ARTIFACTS", artifacts), patch.object(vm, "selected", return_value=self.primary), \
                patch.object(vm, "boot", side_effect=boot), patch.object(vm, "sync_machine", return_value=record["inputs"]), \
                patch.object(vm, "fixture_build", return_value=(vm.SOFTWARE_BUILD + "-dev-perf", record)) as build, \
                patch.object(vm, "planned_fixtures", return_value={"runs": ["popup", "header:pen"], "controls": ["Popup"]}), \
                patch.object(vm, "export_fixture_build", return_value=self.root / "archive"), \
                patch.object(vm, "import_fixture_build") as ship, patch.object(vm, "fixture_durations", return_value={}), \
                patch.object(vm, "execute_fixtures", side_effect=execute), \
                patch.object(vm, "run", return_value=SimpleNamespace(stdout="revision\n")), \
                patch.object(vm, "time") as timing, patch.object(vm, "stop_machine", side_effect=shutdown) as stop:
            timing.strftime.side_effect = ["success", "failure"]
            timing.monotonic.side_effect = lambda: clock[0]
            vm.sharded_fixtures(args)
            failed = True
            with self.assertRaisesRegex(RuntimeError, "popup"):
                vm.sharded_fixtures(args)
        self.assertEqual(build.call_count, 2)
        build.assert_called_with(self.primary, record["inputs"], True, False, "dev-perf", controls=["Popup"])
        self.assertEqual(ship.call_count, 2)
        self.assertEqual([call.args[0].name for call in stop.call_args_list], ["owner-r1", "owner-r1"])
        output = artifacts / "owner" / "shards-success"
        self.assertTrue((output / "complete").exists())
        self.assertEqual(len((output / "results.jsonl").read_text().splitlines()), 2)
        self.assertEqual(json.loads((output / "summary.json").read_text())["wall_seconds"], 37)
        failure = artifacts / "owner" / "shards-failure"
        self.assertFalse((failure / "complete").exists())
        self.assertEqual(json.loads((failure / "summary.json").read_text())["wall_seconds"], 37)

    def test_failed_final_shutdown_is_reported(self):
        machine = vm.runner_machine(self.primary, 1)
        with patch.object(vm, "process", return_value=123), patch.object(vm, "qmp") as qmp, \
                patch.object(vm, "wait_for_exit", return_value=False), \
                self.assertRaisesRegex(RuntimeError, "still running after shutdown"):
            vm.stop_machine(machine)
        self.assertEqual([call.args[1] for call in qmp.call_args_list], ["system_powerdown", "quit"])

    def test_command_lock_precedes_runner_creation_and_blocks_other_runtime(self):
        first = vm.runner_machine(self.primary, 1)
        second = vm.runner_machine(self.primary, 1)
        second.runtime = self.root / "other-runtime"
        with vm.claim(first):
            with self.assertRaisesRegex(SystemExit, "Another command"):
                vm.claim(second)
        with vm.claim(second):
            self.assertFalse(first.directory.exists())

    def test_runner_reset_deletes_only_stopped_owned_disks_and_refuses_live_leases(self):
        machine = vm.runner_machine(self.primary, 1)
        machine.directory.mkdir()
        machine.runner.write_text(json.dumps(machine.runner_owner))
        foreign = vm.Machine("foreign-r1", vm.VMS / "foreign-r1")
        foreign.directory.mkdir()
        foreign.runner.write_text(json.dumps({"primary": "foreign", "worktree": "another"}))
        settings = {"vcpu_budget": 4, "memory_budget_gib": 8, "runner_cpus": 4,
                    "runner_memory_gib": 8, "per_owner_limit": 1}
        with patch.object(vm, "selected", return_value=self.primary), patch.object(vm, "running", return_value=False), \
                patch.object(vm, "cpu_pool") as pool, patch.object(vm, "remove") as remove:
            pool.describe.return_value = (settings, [{"slot": 0, "status": "free", "owner": None}])
            vm.runners(SimpleNamespace(reset=True))
            remove.assert_called_once_with(machine.directory)
            remove.reset_mock()
            pool.describe.return_value = (settings, [{"slot": 0, "status": "reserved", "owner": {
                "vm": machine.name, "state": str(vm.STATE.resolve())}}])
            with self.assertRaisesRegex(SystemExit, "Stop the owner-r1 runner"):
                vm.runners(SimpleNamespace(reset=True))
            remove.assert_not_called()


if __name__ == "__main__":
    unittest.main()
