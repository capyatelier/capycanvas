import importlib.util
import fcntl
import io
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


spec = importlib.util.spec_from_file_location("windows_vm", Path(__file__).with_name("windows-vm.py"))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)


class CredentialTests(unittest.TestCase):
    def test_setup_restricts_existing_state_directory(self):
        with TemporaryDirectory() as directory:
            state = Path(directory)
            iso = state / "windows.iso"
            iso.touch()
            state.chmod(0o755)
            with patch.multiple(vm, STATE=state, ISO_RECORD=state / "iso"), \
                    patch.object(vm, "install_packages"), patch.object(vm, "ensure_kvm_access"), \
                    patch.object(vm.shutil, "which", return_value=None), patch("builtins.print"):
                vm.setup(SimpleNamespace(iso=iso))
            self.assertEqual(state.stat().st_mode & 0o777, 0o700)

    def test_installer_credentials_are_private_with_permissive_umask(self):
        for existing in (False, True):
            with self.subTest(existing=existing), TemporaryDirectory() as directory:
                state = Path(directory)
                paths = {"STATE": state, "BASE": state / "base", "INSTALL": state / "install",
                         "ISO_RECORD": state / "iso", "FIRMWARE": state / "firmware.json",
                         "KEY": state / "id_ed25519", "PASSWORD": state / "password"}
                iso = state / "windows.iso"
                iso.touch()
                paths["ISO_RECORD"].write_text(str(iso))
                paths["KEY"].touch()
                paths["KEY"].with_suffix(".pub").write_text("test-public-key")
                variables = state / "vars"
                variables.touch()
                state.chmod(0o755)
                if existing:
                    paths["PASSWORD"].write_text("old" * 40)
                    paths["PASSWORD"].chmod(0o666)
                previous_umask = os.umask(0)
                try:
                    with patch.multiple(vm, **paths), patch.object(vm, "running", return_value=False), \
                            patch.object(vm, "firmware", return_value={"vars": str(variables)}), \
                            patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0)), \
                            patch.object(vm, "launch", side_effect=InterruptedError), \
                            self.assertRaises(InterruptedError):
                        vm.create(SimpleNamespace(gui=False))
                finally:
                    os.umask(previous_umask)
                answer = paths["INSTALL"] / "seed/autounattend.xml"
                self.assertEqual(state.stat().st_mode & 0o777, 0o700)
                for path in (paths["PASSWORD"], answer):
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                value = paths["PASSWORD"].read_text().strip()
                self.assertEqual(len(value), 24)
                self.assertIn(f"<Value>{value}</Value>", answer.read_text())
                self.assertNotIn("@PASSWORD@", answer.read_text())

    def test_private_write_rejects_symlinks(self):
        with TemporaryDirectory() as directory:
            target = Path(directory) / "unrelated"
            target.write_text("preserved")
            link = Path(directory) / "credential"
            link.symlink_to(target)
            with self.assertRaises(OSError):
                vm.write_private(link, "replacement")
            self.assertEqual(target.read_text(), "preserved")


class CheckTests(unittest.TestCase):
    def test_cli_defaults_to_release_and_debug_is_explicit(self):
        for options, configuration in (([], "Release"), (["--debug"], "Debug")):
            with self.subTest(configuration=configuration), \
                    patch.object(sys, "argv", ["windows-vm.py", "check", *options]), \
                    patch.object(vm, "selected", return_value=None), patch.object(vm, "boot"), \
                    patch.object(vm, "claim"), patch.object(vm, "sync_machine"), \
                    patch.object(vm, "guest") as guest:
                vm.main()
            self.assertTrue(guest.call_args.args[1].endswith(f"-Configuration {configuration}"))


class GpuLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        runtime = self.root / "runtime"
        runtime.mkdir()
        firmware = self.root / "firmware.json"
        firmware.write_text(json.dumps({"code": "code.fd", "code_format": "raw", "vars_format": "raw"}))
        base = self.root / "base"
        base.mkdir()
        (base / "disk.qcow2").touch()
        patcher = patch.multiple(vm, RUNTIME=runtime, FIRMWARE=firmware, BASE=base, CPUS="12", MEMORY="24G")
        patcher.start()
        self.addCleanup(patcher.stop)
        self.machine = vm.Machine("review", self.root / "review")
        self.machine.directory.mkdir()
        self.machine.runtime.mkdir()

    def test_cli_gpu_claim_is_opt_in_and_accepts_an_explicit_pool_address(self):
        for arguments, expected in (([], None), (["--gpu"], "auto"),
                                    (["--gpu", "0000:03:00.0"], "0000:03:00.0")):
            with self.subTest(arguments=arguments), \
                    patch.object(sys, "argv", ["windows-vm.py", "--vm", "review", "start", *arguments]), \
                    patch.object(vm, "start") as start:
                vm.main()
            self.assertEqual(start.call_args.args[0].gpu, expected)

    def test_launch_passes_exact_gpu_functions_and_keeps_reservation_fd_in_qemu(self):
        gpu = SimpleNamespace(address="0000:03:00.0", functions=["0000:03:00.0", "0000:03:00.1"])
        with open(self.root / "reservation", "w+") as reservation, \
                patch.object(vm, "terminate"), patch.object(vm, "free_port", return_value=2200), \
                patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            vm.launch(self.machine, gpu=gpu, reservation=reservation)
            command = next(call for call in run.call_args_list if call.args[0] == "qemu-system-x86_64")
            self.assertEqual(command.kwargs["pass_fds"], (reservation.fileno(),))
            args = command.args
            self.assertEqual(args[args.index("-smp") + 1], "cpus=12,sockets=1,cores=12,threads=1")
            self.assertIn("q35-pcihost.pci-hole64-size=256G", args)
            self.assertIn("name=opt/ovmf/X-PciMmio64Mb,string=262144", args)
            self.assertIn(f"fd={reservation.fileno()},set=99,opaque=capycanvas-gpu-review", args)
            functions = [arg for arg in args if isinstance(arg, str) and arg.startswith("vfio-pci,")]
            self.assertEqual(functions, [
                "vfio-pci,host=0000:03:00.0,bus=gpuroot,addr=0x0.0,multifunction=on",
                "vfio-pci,host=0000:03:00.1,bus=gpuroot,addr=0x0.1,multifunction=on"])
        self.assertEqual(self.machine.gpu.read_text(), "0000:03:00.0\n")

    def test_plain_launch_clears_old_gpu_metadata_and_inherits_no_reservation(self):
        self.machine.gpu.write_text("0000:03:00.0\n")
        with patch.object(vm, "terminate"), patch.object(vm, "free_port", return_value=2200), \
                patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            vm.launch(self.machine)
        command = next(call for call in run.call_args_list if call.args[0] == "qemu-system-x86_64")
        self.assertNotIn("pass_fds", command.kwargs)
        self.assertFalse(any(isinstance(arg, str) and arg.startswith("vfio-pci,") for arg in command.args))
        self.assertFalse(self.machine.gpu.exists())

    def test_failed_qemu_launch_cleans_up_its_tpm(self):
        def run(*args, **kwargs):
            if args[0] == "qemu-system-x86_64":
                raise subprocess.CalledProcessError(1, args)
            return subprocess.CompletedProcess(args, 0)
        with patch.object(vm, "terminate") as terminate, patch.object(vm, "free_port", return_value=2200), \
                patch.object(vm, "run", side_effect=run), self.assertRaises(subprocess.CalledProcessError):
            vm.launch(self.machine)
        self.assertEqual(terminate.call_count, 2)
        self.assertEqual(terminate.call_args.args, (self.machine.tpm_pid, "swtpm"))

    def test_two_starting_processes_cannot_modify_the_same_vm(self):
        with open(self.machine.runtime / "boot-lock", "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with patch.object(vm, "boot_locked") as boot, self.assertRaisesRegex(SystemExit, "already starting"):
                vm.boot(self.machine, False, "auto")
            boot.assert_not_called()

    def test_two_service_launchers_cannot_stop_or_replace_each_other(self):
        with open(self.machine.runtime / "service-lock", "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with patch.object(vm, "start_gpu_service_locked") as start, \
                    self.assertRaisesRegex(SystemExit, "already starting"):
                vm.start_gpu_service(self.machine, SimpleNamespace(gpu="auto", gui=False))
            start.assert_not_called()

    def test_running_software_vm_cannot_claim_a_gpu_without_stopping(self):
        with patch.object(vm, "running", return_value=True), \
                self.assertRaisesRegex(SystemExit, "running without a reserved GPU"):
            vm.boot_locked(self.machine, False, "auto")

    def test_running_gpu_vm_cannot_silently_change_an_explicit_card_request(self):
        self.machine.gpu.write_text("0000:03:00.0\n")
        with patch.object(vm, "running", return_value=True):
            vm.boot_locked(self.machine, False, "auto")
            vm.boot_locked(self.machine, False, "0000:03:00.0")
            with self.assertRaises(SystemExit):
                vm.boot_locked(self.machine, False, "0000:04:00.0")

    def test_insufficient_locked_memory_fails_before_claiming_a_card_or_launching(self):
        with patch.object(vm, "running", return_value=False), \
                patch.object(vm.resource, "getrlimit", return_value=(8 * 1024 * 1024,) * 2), \
                patch.object(vm.gpu_pool, "reserve") as reserve, patch.object(vm, "launch") as launch, \
                self.assertRaisesRegex(SystemExit, "unlimited locked memory"):
            vm.boot_locked(self.machine, False, "auto")
        reserve.assert_not_called()
        launch.assert_not_called()

    def test_gpu_shutdown_timeout_does_not_force_qmp_quit(self):
        self.machine.gpu.write_text("0000:03:00.0\n")
        with patch.object(vm, "process", return_value=123), patch.object(vm, "wait_for_exit", return_value=False), \
                patch.object(vm, "qmp") as qmp, self.assertRaisesRegex(SystemExit, "reservation remain running"):
            vm.stop_machine(self.machine)
        self.assertEqual([call.args[1] for call in qmp.call_args_list], ["system_powerdown"])

    def test_software_shutdown_timeout_retains_existing_bounded_force_quit(self):
        with patch.object(vm, "process", return_value=123), patch.object(vm, "wait_for_exit", side_effect=[False, True]), \
                patch.object(vm, "qmp") as qmp:
            vm.stop_machine(self.machine)
        self.assertEqual([call.args[1] for call in qmp.call_args_list], ["system_powerdown", "quit"])

    def test_live_gpu_vm_is_retained_when_startup_checks_fail(self):
        args = SimpleNamespace(vm="review", gpu="auto", gui=False)
        with patch.object(vm, "selected", return_value=self.machine), patch.object(vm, "running", return_value=True), \
                patch.dict(os.environ, {"CAPYCANVAS_VM_GPU_SERVICE": "1"}), \
                patch.object(vm, "boot", side_effect=SystemExit("SSH did not become available")), \
                patch.object(vm, "stop_machine") as stop, patch.object(sys, "stderr"):
            vm.start(args)
        self.assertIn("SSH did not become available", (self.machine.runtime / "startup-error").read_text())
        stop.assert_not_called()

    def test_failed_start_without_a_running_qemu_fails_and_records_its_cause(self):
        args = SimpleNamespace(vm="review", gpu="auto", gui=False)
        with patch.object(vm, "selected", return_value=self.machine), patch.object(vm, "running", return_value=False), \
                patch.dict(os.environ, {"CAPYCANVAS_VM_GPU_SERVICE": "1"}), \
                patch.object(vm, "boot", side_effect=RuntimeError("No GPU is free")), \
                self.assertRaisesRegex(RuntimeError, "No GPU is free"):
            vm.start(args)
        self.assertEqual((self.machine.runtime / "startup-error").read_text(), "No GPU is free")

    def test_full_pool_creates_no_vm_disk(self):
        machine = vm.Machine("fresh", self.root / "fresh")
        with patch.object(vm, "running", return_value=False), \
                patch.object(vm.resource, "getrlimit", return_value=(vm.resource.RLIM_INFINITY,) * 2), \
                patch.object(vm.gpu_pool, "reserve", side_effect=RuntimeError("No GPU is free")), \
                patch.object(vm, "run") as run, patch.object(vm, "launch") as launch, \
                self.assertRaisesRegex(RuntimeError, "No GPU is free"):
            vm.boot_locked(machine, False, "auto")
        self.assertFalse(machine.directory.exists())
        run.assert_not_called()
        launch.assert_not_called()

    def test_gpu_start_uses_a_service_once_and_software_start_does_not(self):
        for gpu, service_environment, use_service in (("auto", False, True),
                                                     ("0000:03:00.0", True, False),
                                                     (None, False, False)):
            with self.subTest(gpu=gpu, service_environment=service_environment), \
                    patch.dict(os.environ, {"CAPYCANVAS_VM_GPU_SERVICE": "1"} if service_environment else {}, clear=True), \
                    patch.object(vm, "selected", return_value=self.machine), patch.object(vm, "running", return_value=False), \
                    patch.object(vm, "start_gpu_service") as service, patch.object(vm, "boot") as boot, patch("builtins.print"):
                args = SimpleNamespace(vm="review", gpu=gpu, gui=False)
                vm.start(args)
            if use_service:
                service.assert_called_once_with(self.machine, args)
                boot.assert_not_called()
            else:
                service.assert_not_called()
                boot.assert_called_once_with(self.machine, False, gpu)

    def test_running_vm_is_never_replaced_by_a_new_service(self):
        with patch.object(vm, "running", return_value=True), patch.object(vm, "start_gpu_service_locked") as start, \
                self.assertRaisesRegex(SystemExit, "another process"):
            vm.start_gpu_service(self.machine, SimpleNamespace(gpu="auto", gui=False))
        start.assert_not_called()

    def test_gpu_service_preserves_qemu_on_failed_stop_and_propagates_resources(self):
        state = self.root / "state $dollar with spaces"
        error = self.machine.runtime / "startup-error"
        error.write_text("old startup failure")
        with patch.multiple(vm, STATE=state, DISPLAY="2560x1600"), \
                patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            vm.start_gpu_service_locked(self.machine, SimpleNamespace(gpu="0000:03:00.0", gui=True))
        self.assertFalse(error.exists())
        self.assertEqual(run.call_args_list[0].args[:3], ("systemctl", "--user", "stop"))
        command = run.call_args.args
        self.assertEqual(command[0], "systemd-run")
        for property in ("Type=oneshot", "RemainAfterExit=yes", "LimitMEMLOCK=infinity", "KillMode=process",
                         "TimeoutStartSec=45min"):
            self.assertIn(f"--property={property}", command)
        self.assertIn(f"--setenv=CAPYCANVAS_VM_DIR={state}", command)
        self.assertIn("--setenv=CAPYCANVAS_VM_CPUS=12", command)
        self.assertIn("--setenv=CAPYCANVAS_VM_MEMORY=24G", command)
        self.assertIn("--setenv=CAPYCANVAS_VM_GPU_SERVICE=1", command)
        self.assertEqual(command[-4:], ("start", "--gpu", "0000:03:00.0", "--gui"))

    def test_gpu_service_reports_child_startup_error_after_systemd_success(self):
        def run(*args, **kwargs):
            if args[0] == "systemd-run":
                (self.machine.runtime / "startup-error").write_text("GPU driver install failed")
            return subprocess.CompletedProcess(args, 0)
        with patch.object(vm, "run", side_effect=run), self.assertRaisesRegex(SystemExit, "driver install failed"):
            vm.start_gpu_service_locked(self.machine, SimpleNamespace(gpu="auto", gui=False))

    def test_gpu_service_failure_reports_the_child_cause(self):
        def run(*args, **kwargs):
            if args[0] == "systemd-run":
                (self.machine.runtime / "startup-error").write_text("No GPU is free and accessible")
                return subprocess.CompletedProcess(args, 1)
            return subprocess.CompletedProcess(args, 0)
        with patch.object(vm, "run", side_effect=run), self.assertRaisesRegex(SystemExit, "^No GPU is free"):
            vm.start_gpu_service_locked(self.machine, SimpleNamespace(gpu="auto", gui=False))
        with patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 1)), \
                self.assertRaisesRegex(SystemExit, "journalctl --user -u capy-vm-"):
            vm.start_gpu_service_locked(self.machine, SimpleNamespace(gpu="auto", gui=False))

    def test_gpu_unit_is_distinct_for_each_state_directory(self):
        with patch.object(vm, "STATE", self.root / "one"):
            first = vm.gpu_unit(self.machine)
        with patch.object(vm, "STATE", self.root / "two"):
            second = vm.gpu_unit(self.machine)
        self.assertNotEqual(first, second)


class GpuDriverTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.driver = self.root / "vendor bundle"
        self.driver.mkdir()
        self.inf = self.driver / "display.INF"
        self.inf.write_text("vendor display driver")
        (self.driver / "display.cat").write_bytes(b"catalog")
        self.machine = SimpleNamespace(directory=self.root / "machine")
        self.machine.directory.mkdir()
        self.stamp = self.machine.directory / "gpu-driver"
        self.archive = Mock(stdout=io.BytesIO(b"driver archive"))
        self.archive.wait.return_value = 0
        for patcher in (patch.object(vm.gpu_pool, "configuration", return_value={"guest_driver": str(self.driver)}),
                        patch.object(vm.subprocess, "Popen", return_value=self.archive),
                        patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 0)),
                        patch.object(vm, "guest_script"), patch.object(vm, "restart")):
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_unconfigured_driver_does_not_touch_the_guest(self):
        with patch.object(vm.gpu_pool, "configuration", return_value={}), patch("builtins.print"):
            vm.prepare_gpu_driver(self.machine)
        vm.guest.assert_not_called()
        vm.restart.assert_not_called()
        vm.subprocess.Popen.assert_not_called()

    def test_missing_inf_fails_before_copy_or_guest_changes(self):
        self.inf.unlink()
        with self.assertRaisesRegex(SystemExit, "bundle is unavailable"):
            vm.prepare_gpu_driver(self.machine)
        vm.guest.assert_not_called()
        vm.guest_script.assert_not_called()
        self.assertFalse(self.stamp.exists())

    def test_successful_install_is_cached_until_vendor_catalog_changes(self):
        vm.prepare_gpu_driver(self.machine)
        installed = self.stamp.read_text()
        self.assertEqual(len(installed.strip()), 64)
        self.assertTrue(self.archive.stdout.closed)
        self.archive.wait.assert_called_once_with()
        vm.guest_script.assert_called_once()
        vm.restart.assert_called_once_with(self.machine)
        for mock in (vm.guest, vm.guest_script, vm.restart, vm.subprocess.Popen):
            mock.reset_mock()
        vm.prepare_gpu_driver(self.machine)
        for mock in (vm.guest, vm.guest_script, vm.restart, vm.subprocess.Popen):
            mock.assert_not_called()
        (self.driver / "display.cat").write_bytes(b"updated vendor catalog")
        self.archive.stdout = io.BytesIO(b"updated archive")
        vm.prepare_gpu_driver(self.machine)
        self.assertNotEqual(self.stamp.read_text(), installed)
        vm.restart.assert_called_once_with(self.machine)

    def test_failed_guest_staging_closes_and_reaps_archive_without_stamping(self):
        vm.guest.side_effect = [subprocess.CompletedProcess([], 0), subprocess.CalledProcessError(255, "ssh")]
        with self.assertRaises(subprocess.CalledProcessError):
            vm.prepare_gpu_driver(self.machine)
        self.assertTrue(self.archive.stdout.closed)
        self.archive.wait.assert_called_once_with()
        vm.guest_script.assert_not_called()
        vm.restart.assert_not_called()
        self.assertFalse(self.stamp.exists())

    def test_failed_archive_or_guest_copy_cannot_install_or_stamp_the_driver(self):
        for archive_code, guest_code in ((1, 0), (0, 1)):
            with self.subTest(archive=archive_code, guest=guest_code):
                self.archive.stdout = io.BytesIO(b"archive")
                self.archive.wait.return_value = archive_code
                vm.guest.return_value = subprocess.CompletedProcess([], guest_code)
                with self.assertRaisesRegex(SystemExit, "Could not stage"):
                    vm.prepare_gpu_driver(self.machine)
                self.assertFalse(self.stamp.exists())
                vm.guest_script.assert_not_called()
                vm.restart.assert_not_called()

    def test_failed_install_or_restart_never_publishes_a_completed_driver_stamp(self):
        for stage in (vm.guest_script, vm.restart):
            with self.subTest(stage=stage):
                self.archive.stdout = io.BytesIO(b"archive")
                stage.side_effect = RuntimeError("driver preparation failed")
                try:
                    with self.assertRaisesRegex(RuntimeError, "preparation failed"):
                        vm.prepare_gpu_driver(self.machine)
                    self.assertFalse(self.stamp.exists())
                finally:
                    stage.side_effect = None


class ProcessOwnershipTests(unittest.TestCase):
    def test_same_process_name_is_insufficient_to_claim_a_reused_pid(self):
        pidfile = Path("/owned/vm/qemu.pid")
        def text(path, *args, **kwargs):
            return "123\n" if path == pidfile else "qemu-system-x86\n"
        for command, expected in ((b"qemu-system-x86_64\0-pidfile\0/owned/vm/qemu.pid\0", 123),
                                  (b"qemu-system-x86_64\0-pidfile\0/other/vm/qemu.pid\0", None),
                                  (b"qemu-system-x86_64\0-pidfile\0/owned/vm/qemu.pid-copy\0", None)):
            with self.subTest(command=command), patch.object(vm.Path, "read_text", text), \
                    patch.object(vm.Path, "read_bytes", return_value=command):
                self.assertEqual(vm.process(pidfile, "qemu-system-x86"), expected)

    def test_swtpm_ownership_uses_the_full_pidfile_argument(self):
        pidfile = Path("/owned/vm/swtpm.pid")
        def text(path, *args, **kwargs):
            return "456\n" if path == pidfile else "swtpm\n"
        with patch.object(vm.Path, "read_text", text), patch.object(vm.Path, "read_bytes", return_value=(
                b"swtpm\0socket\0--pid\0file=/owned/vm/swtpm.pid\0")):
            self.assertEqual(vm.process(pidfile, "swtpm"), 456)


class FixtureBuildTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.machine = SimpleNamespace(directory=Path(self.directory.name))
        self.hashes = {"apps/layer-windows/CanvasWindow.cpp": "native", "Cargo.lock": "rust"}
        self.files = {"CapyCanvas.exe": "executable", "layer_windows.dll": "core"}

    def test_build_modes_keep_separate_outputs_and_provenance(self):
        for hardware, output, filename in ((False, vm.SOFTWARE_BUILD, "software-build.json"),
                                           (True, vm.HARDWARE_BUILD, "hardware-build.json")):
            with self.subTest(hardware=hardware), patch.object(vm, "guest") as guest, \
                    patch.object(vm, "build_files", return_value=self.files) as files:
                directory, record = vm.fixture_build(self.machine, self.hashes, False, hardware)
            self.assertEqual(directory, output)
            command = guest.call_args.args[1]
            self.assertIn("-Configuration Release", command)
            self.assertIn(f"-OutputDirectory '{output}'", command)
            self.assertEqual("-SoftwareAdapterTests" in command, not hardware)
            self.assertEqual(record, {"inputs": self.hashes, "files": self.files})
            self.assertEqual(json.loads((self.machine.directory / filename).read_text()), record)
            files.assert_called_once_with(self.machine, output)

    def test_failed_rebuild_invalidates_old_record_before_launching_build(self):
        for hardware, filename in ((False, "software-build.json"), (True, "hardware-build.json")):
            with self.subTest(hardware=hardware):
                path = self.machine.directory / filename
                path.write_text(json.dumps({"inputs": self.hashes, "files": self.files}))
                def build(*args, **kwargs):
                    self.assertFalse(path.exists())
                    raise subprocess.CalledProcessError(1, "build")
                with patch.object(vm, "guest", side_effect=build), patch.object(vm, "build_files") as files, \
                        self.assertRaises(subprocess.CalledProcessError):
                    vm.fixture_build(self.machine, self.hashes, False, hardware)
                self.assertFalse(path.exists())
                files.assert_not_called()

    def test_no_build_never_reuses_another_adapter_modes_record(self):
        (self.machine.directory / "software-build.json").write_text(json.dumps({"inputs": self.hashes, "files": self.files}))
        with patch.object(vm, "guest") as guest, patch.object(vm, "build_files", return_value=self.files), \
                self.assertRaisesRegex(SystemExit, "rerun without --no-build"):
            vm.fixture_build(self.machine, self.hashes, True, True)
        guest.assert_not_called()

    def test_no_build_requires_matching_synced_inputs_and_current_guest_binaries(self):
        record = {"inputs": self.hashes, "files": self.files}
        path = self.machine.directory / "hardware-build.json"
        path.write_text(json.dumps(record))
        with patch.object(vm, "guest") as guest, patch.object(vm, "build_files", return_value=self.files):
            self.assertEqual(vm.fixture_build(self.machine, self.hashes, True, True), (vm.HARDWARE_BUILD, record))
            with self.assertRaises(SystemExit):
                vm.fixture_build(self.machine, {**self.hashes, "Cargo.lock": "changed"}, True, True)
        with patch.object(vm, "guest") as guest, patch.object(vm, "build_files", return_value={**self.files, "layer_windows.dll": "stale"}), \
                self.assertRaises(SystemExit):
            vm.fixture_build(self.machine, self.hashes, True, True)
        guest.assert_not_called()
        self.assertEqual(json.loads(path.read_text()), record)


class GpuCheckTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.local = Path(self.directory.name)
        self.machine = SimpleNamespace(name="review")
        self.args = SimpleNamespace(vm="review", no_build=True)
        self.hashes = {"crates/layer-ui/src/session.rs": "synced"}
        self.record = {"inputs": self.hashes, "files": {"CapyCanvas.exe": "native"}}
        for patcher in (patch.object(vm, "start"), patch.object(vm, "selected", return_value=self.machine),
                        patch.object(vm, "claim", return_value=io.StringIO()),
                        patch.object(vm, "run", return_value=subprocess.CompletedProcess([], 0, "revision\n")),
                        patch.object(vm, "sync_machine", return_value=self.hashes),
                        patch.object(vm, "fixture_build", return_value=(vm.HARDWARE_BUILD, self.record)),
                        patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 0)),
                        patch.object(vm, "retrieve", return_value=self.local), patch("builtins.print")):
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_gpu_check_claims_hardware_and_preserves_complete_build_provenance(self):
        vm.gpu_check(self.args)
        startup = vm.start.call_args.args[0]
        self.assertEqual((startup.vm, startup.gpu, startup.gui), ("review", "auto", False))
        vm.claim.assert_called_once_with(self.machine)
        vm.fixture_build.assert_called_once_with(self.machine, self.hashes, True, True)
        command = vm.guest.call_args.args[1]
        self.assertIn("test-with-gpu.ps1", command)
        self.assertIn(f"-BuildDirectory '{vm.HARDWARE_BUILD}'", command)
        self.assertFalse(vm.guest.call_args.kwargs["check"])
        self.assertEqual(json.loads((self.local / "provenance.json").read_text()),
                         {"revision": "revision", "source": self.hashes, "build": self.record})

    def test_failed_hardware_suite_still_retrieves_evidence_and_provenance(self):
        vm.guest.return_value = subprocess.CompletedProcess([], 9)
        with self.assertRaisesRegex(SystemExit, "Hardware GPU tests failed.*9"), \
                patch.object(vm, "stop_machine") as stop:
            vm.gpu_check(self.args)
        vm.retrieve.assert_called_once()
        self.assertTrue((self.local / "provenance.json").exists())
        stop.assert_not_called()

    def test_exclusive_claim_failure_prevents_sync_build_and_guest_test_commands(self):
        vm.claim.side_effect = SystemExit("Another command is using this VM")
        with self.assertRaisesRegex(SystemExit, "Another command"):
            vm.gpu_check(self.args)
        for mock in (vm.sync_machine, vm.fixture_build, vm.guest, vm.retrieve):
            mock.assert_not_called()

    def test_failed_build_never_runs_or_reports_hardware_tests(self):
        vm.fixture_build.side_effect = subprocess.CalledProcessError(1, "build")
        with self.assertRaises(subprocess.CalledProcessError):
            vm.gpu_check(self.args)
        vm.guest.assert_not_called()
        vm.retrieve.assert_not_called()
        self.assertFalse((self.local / "provenance.json").exists())


class FixtureRunnerTests(unittest.TestCase):
    def test_cli_timeout_defaults_and_bounded_override(self):
        for arguments, minutes in (([], 20), (["--timeout-minutes", "90"], 90)):
            with patch.object(sys, "argv", ["windows-vm.py", "fixtures", "localization", *arguments]), \
                    patch.object(vm, "fixtures") as fixtures:
                vm.main()
            args = fixtures.call_args.args[0]
            self.assertEqual(args.timeout_minutes, minutes)
            self.assertEqual(args.names, ["localization"])
        for value in ("0", "-1", "1441", "invalid"):
            with patch.object(sys, "argv", ["windows-vm.py", "fixtures", "--timeout-minutes", value]), \
                    patch.object(vm, "fixtures") as fixtures, patch.object(sys, "stderr"), \
                    self.assertRaises(SystemExit):
                vm.main()
            fixtures.assert_not_called()

    def test_no_build_rejects_changed_sources_and_binaries(self):
        inputs = vm.build_inputs({"apps/layer-windows/CanvasWindow.cpp": "original",
                                  "assets/locales/ja/commands.ftl": "original-catalog"})
        files = {"CapyCanvas.exe": "exe", "layer_windows.dll": "rust"}
        record = {"inputs": inputs, "files": files}
        vm.validate_build(record, inputs, files)
        for candidate, artifacts in (({**inputs, "Cargo.lock": "new"}, files),
                                     (vm.build_inputs({**inputs, "assets/locales/ja/commands.ftl": "edited-catalog"}), files),
                                     (inputs, {**files, "layer_windows.dll": "changed"})):
            with self.assertRaises(SystemExit):
                vm.validate_build(record, candidate, artifacts)
        with self.assertRaises(SystemExit):
            vm.validate_build(None, inputs, files)

    def test_fixture_only_edits_can_reuse_builds(self):
        source = {"Cargo.lock": "lock", "crates/layer-ui/src/lib.rs": "rust",
                  "apps/layer-windows/UiControls.h": "header",
                  "apps/layer-windows/scripts/build.ps1": "build",
                  "apps/layer-windows/scripts/stage-assets.ps1": "staging",
                  "apps/layer-web/icons/layer-select-symbolic.svg": "icon",
                  "apps/layer-web/filter-previews/curves.png": "preview",
                  "assets/locales/en/commands.ftl": "catalog",
                  "apps/layer-windows/scripts/exercise-layers.ps1": "fixture"}
        changed = {**source, "apps/layer-windows/scripts/exercise-layers.ps1": "fixed",
                   "apps/layer-windows/scripts/CapyUia.ps1": "helper",
                   "apps/layer-windows/scripts/CanvasTouchDriver.cs": "input"}
        self.assertEqual(vm.build_inputs(source), vm.build_inputs(changed))
        for name in list(source)[:-1]:
            self.assertNotEqual(vm.build_inputs(source), vm.build_inputs({**source, name: "changed"}))

    def test_empty_missing_and_duplicate_results_cannot_pass(self):
        plan = {"runs": ["layers", "header:pen"]}
        vm.validate_results(plan, [{"name": name} for name in plan["runs"]])
        for names in ([], ["layers"], ["layers", "layers"], ["layers", "header:pen", "layers"]):
            with self.assertRaises(SystemExit):
                vm.validate_results(plan, [{"name": name} for name in names])
        with self.assertRaises(SystemExit):
            vm.validate_results({"runs": []}, [])

    def test_monitor_reads_powershell_raw_log_and_requires_completion(self):
        plan = {"runs": ["layers", "header:pen"], "timeout_minutes": 20}
        rows = [{"name": name, "exit": 0, "seconds": 1} for name in plan["runs"]]
        progress = {"plan": plan, "log": "\r\n".join(map(json.dumps, rows)), "complete": True}
        output = subprocess.CompletedProcess([], 0, json.dumps(progress))
        with patch.object(vm.time, "sleep"), patch.object(vm, "guest", return_value=output), \
                patch.object(vm, "desktop_running", return_value=False):
            results, actual_plan, failure = vm.wait_for_fixtures(None, "review")
        self.assertEqual(results, rows)
        self.assertEqual(actual_plan, plan)
        self.assertIsNone(failure)
        progress["complete"] = False
        output.stdout = json.dumps(progress)
        with patch.object(vm.time, "sleep"), patch.object(vm, "guest", return_value=output), \
                patch.object(vm, "desktop_running", return_value=False):
            _, _, failure = vm.wait_for_fixtures(None, "review")
        self.assertIn("stopped before completing", failure)

    def test_runner_error_is_reported_without_hiding_the_failure(self):
        progress = {"plan": None, "log": None, "complete": False, "error": "Unknown fixture: absent"}
        with patch.object(vm.time, "sleep"), \
                patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 0, json.dumps(progress))), \
                patch.object(vm, "desktop_running", return_value=False):
            results, _, failure = vm.wait_for_fixtures(None, "review")
        self.assertEqual(results, [])
        self.assertEqual(failure, progress["error"])

    def test_monitor_reads_completion_after_observing_task_stop(self):
        stopped = []
        def status(*args, **kwargs):
            self.assertTrue(stopped)
            return subprocess.CompletedProcess([], 0, json.dumps({
                "plan": {"runs": ["layers"], "timeout_minutes": 20},
                "log": '{"name":"layers","exit":0,"seconds":1}', "complete": True}))
        with patch.object(vm.time, "sleep"), patch.object(vm, "guest", side_effect=status), \
                patch.object(vm, "desktop_running", side_effect=lambda _: stopped.append(True) or False):
            results, _, failure = vm.wait_for_fixtures(None, "review")
        self.assertIsNone(failure)
        self.assertEqual(len(results), 1)

    def test_task_query_failure_is_not_a_running_task(self):
        with patch.object(vm, "guest", side_effect=subprocess.CalledProcessError(1, "Get-ScheduledTask")):
            with self.assertRaises(subprocess.CalledProcessError):
                vm.desktop_running(None)
        for state, running in (("Running", True), ("Queued", True), ("Ready", False)):
            with patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 0, state)):
                self.assertEqual(vm.desktop_running(None), running)


if __name__ == "__main__":
    unittest.main()
