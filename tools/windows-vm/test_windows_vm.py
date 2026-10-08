import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch


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
