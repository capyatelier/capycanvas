import importlib.util
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from urllib.request import Request, urlopen


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
                patch.object(vm, "qmp", return_value="TCP[HOST_FORWARD] 12 127.0.0.1 2200 10.0.2.15 22 0 0"), \
                patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            vm.launch(machine, reservations=(reservation,), cpus=4, memory="8G")
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

    def test_missing_result_retains_prior_positive_duration(self):
        artifacts = self.root / "evidence"
        for index, rows in enumerate(([
                {"name": "long", "exit": 0, "seconds": 100},
                {"name": "short", "exit": 0, "seconds": 5},
        ], [{"name": "long", "exit": "missing", "seconds": 0}])):
            results = artifacts / "runner" / str(index) / "results.jsonl"
            results.parent.mkdir(parents=True)
            results.write_text("".join(json.dumps(row) + "\n" for row in rows))
            os.utime(results, (100 + index, 100 + index))
        with patch.object(vm, "ARTIFACTS", artifacts), patch.object(vm, "SETTINGS", {"artifacts": str(artifacts)}):
            durations = vm.fixture_durations()
        self.assertEqual(durations, {"long": 100, "short": 5})

    def test_latest_failed_and_timed_out_durations_start_before_short_fixtures(self):
        artifacts = self.root / "evidence"
        results = artifacts / "runner" / "run" / "results.jsonl"
        results.parent.mkdir(parents=True)
        rows = [
            {"name": "fast", "exit": 0, "seconds": 30},
            {"name": "file-activation", "exit": 0, "seconds": 10},
            {"name": "file-activation", "exit": 1, "seconds": 1202},
            {"name": "file-activation:light", "exit": "timeout", "seconds": 1201},
        ]
        results.write_text("".join(json.dumps(row) + "\n" for row in rows) + 'not json\n{"name":"incomplete"}\n')
        with patch.object(vm, "ARTIFACTS", artifacts), patch.object(vm, "SETTINGS", {"artifacts": str(artifacts)}):
            durations = vm.fixture_durations()
        selectors = ["fast", "file-activation", "file-activation:light", "unknown"]
        with vm.fixture_shards.Queue(selectors, ["r1"], durations, artifacts / "plan.json") as queue:
            for expected in ["file-activation", "file-activation:light", "unknown", "fast"]:
                request = Request(f"http://127.0.0.1:{queue.server.server_port}/r1", method="POST",
                                  headers={"Authorization": "Bearer " + queue.tokens["r1"]})
                with urlopen(request, timeout=5) as response:
                    self.assertEqual(json.load(response)["selector"], expected)
        self.assertEqual(durations, {"fast": 30, "file-activation": 1202, "file-activation:light": 1201})

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
        cleanup_failed = False
        clock = [0]
        def shutdown(machine):
            clock[0] += 37
        def boot(machine, **options):
            if machine.name.endswith("-r2"):
                raise vm.cpu_pool.Unavailable("pool full")
        @contextmanager
        def configuration(machine, queue):
            yield queue
            if cleanup_failed:
                raise RuntimeError("queue config cleanup failed")
        def execute(machine, args, directory, provenance, names, queue=None):
            self.assertEqual(names, ["popup:default", "header:pen"])
            claims = []
            while True:
                request = Request(f"http://127.0.0.1:{queue.server.server_port}/{machine.name}", method="POST",
                                  headers={"Authorization": "Bearer " + queue.tokens[machine.name]})
                with urlopen(request, timeout=5) as response:
                    selector = json.load(response)["selector"]
                if selector is None:
                    break
                claims.append(selector)
            self.assertEqual(claims, ["popup", "header:pen"])
            return {"vm": machine.name, "results": [{"name": "popup", "exit": int(failed)}, {"name": "header:pen", "exit": 0}],
                    "complete": True, "failure": None, "evidence": "shard-evidence", "plan": {"runs": claims}}
        with patch.object(vm, "ARTIFACTS", artifacts), patch.object(vm, "selected", return_value=self.primary), \
                patch.object(vm, "boot", side_effect=boot), patch.object(vm, "sync_machine", return_value=record["inputs"]), \
                patch.object(vm, "fixture_build", return_value=(vm.SOFTWARE_BUILD + "-dev-perf", record)) as build, \
                patch.object(vm, "planned_fixtures", return_value={"runs": ["popup", "header:pen"], "controls": ["Popup"],
                        "requirements": {"popup": {"hardware_gpu": False, "controls": ["Popup"]}, "header:pen": {"hardware_gpu": False, "controls": []}}}), \
                patch.object(vm, "running", return_value=False), \
                patch.object(vm.cpu_pool, "describe", return_value=({"per_owner_limit": 2, "runner_memory_gib": 8}, [{"status": "free", "owner": None}] * 2)), \
                patch.object(vm, "export_fixture_build", return_value=self.root / "archive"), \
                patch.object(vm, "import_fixture_build") as ship, patch.object(vm, "fixture_durations", return_value={}), \
                patch.object(vm, "execute_fixtures", side_effect=execute), \
                patch.object(vm, "fixture_queue_config", side_effect=configuration), \
                patch.object(vm, "run", return_value=SimpleNamespace(stdout="revision\n")), \
                patch.object(vm, "time") as timing, patch.object(vm, "stop_machine", side_effect=shutdown) as stop:
            timing.strftime.side_effect = ["success", "failure", "cleanup-failure"]
            timing.monotonic.side_effect = lambda: clock[0]
            vm.sharded_fixtures(args)
            failed = True
            with self.assertRaisesRegex(RuntimeError, "popup"):
                vm.sharded_fixtures(args)
            failed = False
            cleanup_failed = True
            with self.assertRaisesRegex(RuntimeError, "queue config cleanup failed"):
                vm.sharded_fixtures(args)
        self.assertEqual(build.call_count, 3)
        build.assert_called_with(self.primary, record["inputs"], True, False, "dev-perf", controls=["Popup"])
        self.assertEqual(ship.call_count, 3)
        self.assertEqual([call.args[0].name for call in stop.call_args_list], ["owner", "owner-r1"] * 3)
        output = artifacts / "owner" / "shards-success"
        self.assertTrue((output / "complete").exists())
        self.assertEqual(len((output / "results.jsonl").read_text().splitlines()), 2)
        self.assertEqual(json.loads((output / "summary.json").read_text())["wall_seconds"], 74)
        failure = artifacts / "owner" / "shards-failure"
        self.assertFalse((failure / "complete").exists())
        self.assertEqual(json.loads((failure / "summary.json").read_text())["wall_seconds"], 74)

        cleanup = artifacts / "owner" / "shards-cleanup-failure"
        self.assertFalse((cleanup / "complete").exists())
        shards = json.loads((cleanup / "shards.json").read_text())
        self.assertEqual(shards[0]["results"], [{"name": "popup", "exit": 0}, {"name": "header:pen", "exit": 0}])
        self.assertTrue(shards[0]["complete"])
        self.assertEqual(shards[0]["evidence"], "shard-evidence")
        self.assertEqual(shards[0]["plan"], {"runs": ["popup", "header:pen"]})
        self.assertEqual(shards[0]["failure"], "queue config cleanup failed")
        self.assertTrue(all(row["exit"] == 0 for row in map(json.loads, (cleanup / "results.jsonl").read_text().splitlines())))

    def test_queue_secret_uses_stdin_outside_evidence_and_exact_cleanup_on_failure(self):
        queue = SimpleNamespace(configuration=lambda runner: {"url": "http://10.0.2.2:1234/" + runner, "token": "secret"})
        with patch.object(vm, "guest") as guest:
            with self.assertRaisesRegex(RuntimeError, "fixture failed"):
                with vm.fixture_queue_config(self.primary, queue) as path:
                    self.assertNotIn("vm-fixtures", path)
                    raise RuntimeError("fixture failed")
        write, cleanup = guest.call_args_list
        self.assertEqual(json.loads(write.kwargs["input"])["token"], "secret")
        self.assertNotIn("secret", write.args[1])
        self.assertNotIn("secret", cleanup.args[1])
        self.assertIn(path, cleanup.args[1])
        self.assertIn("-LiteralPath", cleanup.args[1])

    def test_mixed_round_partitions_controls_preserves_full_plan_and_always_stops_gpu(self):
        from contextlib import ExitStack
        for cpu, gpu, failure in ((True, True, None), (True, False, None), (False, True, None),
                                  (False, True, "startup"), (False, True, "scale"), (True, True, "row"), (True, True, "shutdown")):
            with self.subTest(cpu=cpu, gpu=gpu, failure=failure), ExitStack() as stack:
                artifacts = self.root / f"mixed-{cpu}-{gpu}-{failure}"
                self.primary.directory.mkdir(exist_ok=True)
                selected = (["popup"] if cpu else []) + (["paint:light"] if gpu else [])
                requirements = {name: {"hardware_gpu": name != "popup", "controls": ["Popup"] if name == "popup" else []} for name in selected}
                plan = {"runs": selected, "controls": ["Popup"] if cpu else [], "requirements": requirements}
                records = {}
                phase = {"gpu_started": False, "shutdown_failed": False}
                def build(machine, hashes, no_build, hardware, profile, controls):
                    self.assertEqual(profile, "dev-perf")
                    self.assertEqual(controls, [] if hardware else ["Popup"])
                    record = {"inputs": hashes, "files": {"CapyCanvas.exe": str(hardware)}, "profile": profile,
                              "hardware_gpu": hardware, "controls": {kind: {"exe": "hash"} for kind in controls}}
                    records[hardware] = record
                    return vm.fixture_directory(hardware, profile), record
                def execute(machine, args, directory, provenance, names, queue=None):
                    hardware = machine is self.primary
                    self.assertEqual(names, ["paint:light"] if hardware else ["popup:default"])
                    self.assertEqual(provenance["hardware_gpu"], hardware)
                    self.assertEqual(provenance["build"], records[hardware])
                    if hardware:
                        selection = ["paint:light"]
                    else:
                        selection = list(queue.selectors)
                        queue.assignments[machine.name].extend(selection)
                    return {"vm": machine.name, "results": [{"name": name, "exit": int(hardware and failure == "row")} for name in selection],
                            "complete": True, "failure": "Original GPU failure" if hardware and failure == "shutdown" else None, "evidence": str(machine.directory), "plan": {"runs": selection}}
                def start(args):
                    self.assertTrue(args.round_gpu)
                    if failure == "startup":
                        raise RuntimeError("GPU startup failed after launch")
                    phase["gpu_started"] = True
                    record = {"success": True, "display": {"adapter": "NVIDIA assigned", "primary": True,
                              "width": 2560, "height": 1600, "refreshHz": 120, "scalePercent": 125 if failure == "scale" else 150}}
                    (self.primary.directory / "display.json").write_text(json.dumps(record))
                    (self.primary.directory / "fixture-scale-change.json").write_text(json.dumps({"scale_change": {"current_scale": 125, "target_scale": 150}}))
                def shutdown(machine):
                    if machine is self.primary and failure == "shutdown" and phase["gpu_started"] and not phase["shutdown_failed"]:
                        phase["shutdown_failed"] = True
                        raise RuntimeError("GPU stop refused")
                @contextmanager
                def configuration(machine, queue):
                    yield queue
                mocks = {
                    "ARTIFACTS": artifacts, "CPUS": "8", "MEMORY": "16G", "DISPLAY": "2560x1600",
                }
                stack.enter_context(patch.multiple(vm, **mocks))
                for name, options in {
                    "selected": {"return_value": self.primary}, "running": {"return_value": False}, "boot": {},
                    "sync_machine": {"return_value": {"Cargo.lock": "source"}},
                    "planned_fixtures": {"return_value": plan}, "fixture_build": {"side_effect": build},
                    "export_fixture_build": {"return_value": self.root / "archive"}, "import_fixture_build": {},
                    "fixture_durations": {"return_value": {}}, "execute_fixtures": {"side_effect": execute},
                    "fixture_queue_config": {"side_effect": configuration}, "start": {"side_effect": start},
                    "publish_build_record": {}, "run": {"return_value": SimpleNamespace(stdout="revision\n")},
                }.items():
                    stack.enter_context(patch.object(vm, name, **options))
                stop = stack.enter_context(patch.object(vm, "stop_machine", side_effect=shutdown))
                memory = stack.enter_context(patch.object(vm, "fixture_memory_admission"))
                stack.enter_context(patch.object(vm.cpu_pool, "describe", return_value=(
                    {"per_owner_limit": 24, "runner_memory_gib": 8}, [{"status": "free", "owner": None}] * 24)))
                args = SimpleNamespace(names=selected, runners=24, keep_runners=True, hardware_gpu=False,
                                       no_build=False, profile="dev-perf", timeout_minutes=20)
                if failure:
                    with self.assertRaisesRegex(RuntimeError, "GPU startup|verified|paint:light|GPU shutdown"):
                        vm.sharded_fixtures(args)
                else:
                    vm.sharded_fixtures(args)
                output = next((artifacts / "owner").glob("shards-*"))
                self.assertEqual({key: value for key, value in json.loads((output / "plan.json").read_text()).items() if key != "shards"}, plan)
                self.assertEqual([json.loads(line)["name"] for line in (output / "results.jsonl").read_text().splitlines()], selected)
                self.assertEqual((output / "cpu-plan.json").exists(), cpu and failure not in ("startup", "scale"))
                self.assertEqual([call.args[0] for call in stop.call_args_list], [self.primary] * (3 if failure == "shutdown" else 2 if gpu else 1))
                self.assertEqual((output / "complete").exists(), failure is None)
                if failure == "shutdown":
                    shard = next(row for row in json.loads((output / "shards.json").read_text()) if row["vm"] == self.primary.name)
                    self.assertEqual(shard["failure"], "Original GPU failure; GPU shutdown: GPU stop refused")
                    self.assertEqual(shard["results"], [{"name": "paint:light", "exit": 0}])
                self.assertEqual(set(records), ({False} if cpu else set()) | ({True} if gpu else set()))
                self.assertEqual([call.args[1] for call in memory.call_args_list], ["build-primary"] + (["gpu-primary"] if gpu else []) + (["cpu-batch"] if cpu and failure not in ("startup", "scale") else []))

    def test_round_display_rejects_wrong_scale_mode_and_unapplied_registry(self):
        base = {"success": True, "display": {"adapter": "NVIDIA", "primary": True, "refreshHz": 120,
                "width": 2560, "height": 1600, "scalePercent": 150}}
        vm.validate_round_display(base)
        for key, value in (("scalePercent", 125), ("width", 1920), ("primary", False), ("adapter", "WARP")):
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "verified"):
                vm.validate_round_display({**base, "display": {**base["display"], key: value}})
        with self.assertRaises(ValueError):
            vm.validate_round_display({**base, "restart_required": True})

    def test_gpu_phase_releases_primary_card_and_slots_while_cpu_is_still_running(self):
        from contextlib import ExitStack
        from threading import Event, Thread
        self.primary.directory.mkdir()
        cpu_started, cpu_release, gpu_finished, gpu_stopped, round_done = (Event() for _ in range(5))
        resources = {"card": False, "slots": 0}
        failures, stopped_under_claim, primary_stops = [], [], []
        plan = {"runs": ["cpu", "gpu"], "requirements": {
            "cpu": {"hardware_gpu": False, "controls": []}, "gpu": {"hardware_gpu": True, "controls": []}}}
        def build(machine, hashes, no_build, hardware, profile, controls):
            return vm.fixture_directory(hardware, profile), {"inputs": hashes, "hardware_gpu": hardware, "controls": {}}
        def start(args):
            resources.update(card=True, slots=2)
            (self.primary.directory / "display.json").write_text(json.dumps({"success": True, "display": {
                "adapter": "NVIDIA", "primary": True, "width": 2560, "height": 1600, "refreshHz": 120, "scalePercent": 150}}))
            (self.primary.directory / "fixture-scale-change.json").write_text("{}")
        def stop(machine):
            if machine is self.primary:
                primary_stops.append(machine)
            if machine is self.primary and resources["card"]:
                try:
                    other = vm.claim(machine)
                except SystemExit:
                    stopped_under_claim.append(True)
                else:
                    other.close()
                    stopped_under_claim.append(False)
                resources.update(card=False, slots=0)
                gpu_stopped.set()
        def execute(machine, args, directory, provenance, names, queue=None):
            if machine is self.primary:
                if not cpu_started.wait(3):
                    raise RuntimeError("CPU did not reach its controlled fixture barrier")
                selection = ["gpu"]
                gpu_finished.set()
            else:
                cpu_started.set()
                if not cpu_release.wait(10):
                    raise RuntimeError("CPU barrier was not released by the test")
                selection = ["cpu"]
                queue.assignments[machine.name].extend(selection)
            return {"vm": machine.name, "results": [{"name": name, "exit": 0} for name in selection],
                    "complete": True, "failure": None, "plan": {"runs": selection}}
        @contextmanager
        def configuration(machine, queue):
            yield queue
        args = SimpleNamespace(names=["cpu", "gpu"], runners=1, keep_runners=False, hardware_gpu=False,
                               no_build=False, profile="release", timeout_minutes=20)
        def run_round():
            try:
                vm.sharded_fixtures(args)
            except BaseException as error:
                failures.append(error)
            finally:
                round_done.set()
        with ExitStack() as stack:
            stack.enter_context(patch.multiple(vm, ARTIFACTS=self.root / "release", CPUS="8", MEMORY="16G", DISPLAY="2560x1600"))
            for name, options in {
                "selected": {"return_value": self.primary}, "running": {"return_value": False}, "boot": {},
                "sync_machine": {"return_value": {"Cargo.lock": "source"}}, "planned_fixtures": {"return_value": plan},
                "fixture_build": {"side_effect": build}, "export_fixture_build": {"return_value": self.root / "archive"},
                "import_fixture_build": {}, "fixture_memory_admission": {}, "fixture_durations": {"return_value": {}},
                "execute_fixtures": {"side_effect": execute}, "fixture_queue_config": {"side_effect": configuration},
                "start": {"side_effect": start}, "stop_machine": {"side_effect": stop}, "publish_build_record": {},
                "run": {"return_value": SimpleNamespace(stdout="revision\n")},
            }.items():
                stack.enter_context(patch.object(vm, name, **options))
            stack.enter_context(patch.object(vm.cpu_pool, "describe", return_value=(
                {"per_owner_limit": 24, "runner_memory_gib": 8}, [{"status": "free", "owner": None}] * 24)))
            worker = Thread(target=run_round)
            worker.start()
            try:
                self.assertTrue(cpu_started.wait(3), "CPU shard did not start")
                self.assertTrue(gpu_finished.wait(3), "GPU shard did not finish")
                self.assertTrue(gpu_stopped.wait(3), "Completed GPU phase retained its primary, card and two CPU slots until the CPU round finished")
                self.assertFalse(round_done.is_set(), "The CPU barrier did not keep the round open")
                self.assertEqual(resources, {"card": False, "slots": 0})
                self.assertEqual(stopped_under_claim, [True])
            finally:
                cpu_release.set()
                worker.join(5)
            self.assertFalse(worker.is_alive(), "Fixture coordinator did not finish after the CPU barrier released")
        self.assertEqual(failures, [])
        self.assertEqual(stopped_under_claim, [True])
        self.assertEqual(len(primary_stops), 2)

    def test_plain_named_cpu_fixture_uses_warm_primary_without_shipment_or_runner(self):
        self.primary.directory.mkdir()
        plan = {"runs": ["navigator"], "controls": [], "requirements": {
            "navigator": {"hardware_gpu": False, "controls": []}}}
        record = {"profile": "dev-perf", "hardware_gpu": False, "controls": {}}
        result = {"vm": self.primary.name, "results": [{"name": "navigator", "exit": 0}],
                  "complete": True, "failure": None, "plan": {"runs": ["navigator"]}}
        args = SimpleNamespace(names=["navigator:default"], runners=None, keep_runners=False,
                               hardware_gpu=False, no_build=False, profile="dev-perf", timeout_minutes=20)
        with patch.object(vm, "ARTIFACTS", self.root / "plain"), \
                patch.object(vm, "selected", return_value=self.primary), \
                patch.object(vm, "running", return_value=True), patch.object(vm, "boot") as boot, \
                patch.object(vm, "sync_machine", return_value={"Cargo.lock": "source"}) as sync, \
                patch.object(vm, "planned_fixtures", return_value=plan) as listing, \
                patch.object(vm, "fixture_build", return_value=(vm.SOFTWARE_BUILD, record)) as build, \
                patch.object(vm, "execute_fixtures", return_value=result) as execute, \
                patch.object(vm, "export_fixture_build") as archive, patch.object(vm, "import_fixture_build") as ship, \
                patch.object(vm, "runner_machine") as runner, patch.object(vm, "start") as gpu, \
                patch.object(vm, "stop_machine") as stop, \
                patch.object(vm.cpu_pool, "describe", side_effect=AssertionError("Plain fixture used the CPU pool")), \
                patch.object(vm, "run", return_value=SimpleNamespace(stdout="revision\n")), \
                patch.object(vm.time, "strftime", side_effect=["build", "reuse"]):
            vm.fixtures(args)
            args.no_build = True
            result["results"][0]["exit"] = 1
            with self.assertRaisesRegex(RuntimeError, "navigator"):
                vm.fixtures(args)
        self.assertEqual(boot.call_args_list, [unittest.mock.call(self.primary, gui=False)] * 2)
        self.assertEqual(sync.call_count, 2)
        self.assertEqual(listing.call_count, 2)
        self.assertEqual(build.call_count, 2)
        self.assertEqual([call.args[2] for call in build.call_args_list], [False, True])
        self.assertEqual(execute.call_count, 2)
        self.assertTrue(all(call.args[0] is self.primary and call.kwargs.get("queue") is None for call in execute.call_args_list))
        for unused in (archive, ship, runner, gpu, stop):
            unused.assert_not_called()

    def test_actual_primary_memory_is_admitted_before_any_boot(self):
        self.primary.directory.mkdir()
        args = SimpleNamespace(names=["navigator:default"], runners=None, keep_runners=False,
                               hardware_gpu=False, no_build=False, profile="release", timeout_minutes=20)
        for memory, gib in (("32G", 32), ("64G", 64)):
            with self.subTest(memory=memory), \
                    patch.object(vm, "ARTIFACTS", self.root / memory), patch.object(vm, "MEMORY", memory), \
                    patch.object(vm, "selected", return_value=self.primary), patch.object(vm, "running", return_value=False), \
                    patch.object(vm, "boot", side_effect=AssertionError("Boot preceded memory admission")) as boot:
                with patch.object(vm.Path, "read_text", return_value=f"MemAvailable: {(gib + 32) * 1024 ** 2} kB\n"), \
                        self.assertRaisesRegex(RuntimeError, "MemAvailable"):
                    vm.fixtures(args)
                boot.assert_not_called()
            output = next((self.root / memory / "owner").glob("shards-*"))
            row = json.loads((output / "memory-admission.jsonl").read_text())
            self.assertEqual(row["vm_sizes_gib"], [gib])
            self.assertEqual(row["required_bytes"], (gib + 40) * 1024 ** 3)
            self.assertFalse(row["admitted"])

    def test_memory_batch_admission_records_exact_threshold_and_refusal(self):
        for sizes in ([16], [8] * 22):
            required = (sum(sizes) + 40) * 1024 ** 3
            for available in (required, required - 1024):
                with self.subTest(sizes=sizes, available=available):
                    with patch.object(vm.Path, "read_text", return_value=f"MemAvailable: {available // 1024} kB\n"):
                        if available < required:
                            with self.assertRaisesRegex(RuntimeError, "MemAvailable"):
                                vm.fixture_memory_admission(self.root, "round", sizes)
                        else:
                            vm.fixture_memory_admission(self.root, "round", sizes)
                    row = json.loads((self.root / "memory-admission.jsonl").read_text().splitlines()[-1])
                    self.assertEqual((row["available_bytes"], row["required_bytes"], row["admitted"]),
                                     (available, required, available >= required))

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
