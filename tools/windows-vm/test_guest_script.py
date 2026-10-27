import base64
import importlib.util
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("windows_vm", Path(__file__).with_name("windows-vm.py"))
vm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vm)


class GuestScriptTests(unittest.TestCase):
    def test_long_unicode_script_streams_without_growing_command(self):
        scripts = ["Write-Output 'short'", "$value = @'\n" + "日本語 😀 ' $ `\n" * 12000 + "'@\nWrite-Output $value"]
        calls = []
        machine = SimpleNamespace(name="owned")
        with patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 0)) as guest:
            for script in scripts:
                self.assertIsNone(vm.guest_script(machine, script, capture_output=True, timeout=9))
                calls.append(guest.call_args)
        self.assertEqual(calls[0].args, calls[1].args)
        self.assertLess(len(calls[1].args[1]), 1024)
        self.assertTrue(calls[1].args[1].startswith("$input | powershell "))
        loader = base64.b64decode(calls[1].args[1].split(" -EncodedCommand ")[1].split(";")[0]).decode("utf-16-le")
        self.assertTrue(calls[1].args[1].endswith("; exit $LASTEXITCODE"))
        self.assertIn("[Console]::In.ReadLine()", loader)
        self.assertIn("[ScriptBlock]::Create(", loader)
        for script, call in zip(scripts, calls):
            self.assertTrue(call.kwargs["input"].endswith(b"\n\n"))
            self.assertLessEqual(max(map(len, call.kwargs["input"].splitlines())), 1024)
            self.assertEqual(base64.b64decode(call.kwargs["input"]).decode("utf-16-le"), script)
            self.assertEqual({key: value for key, value in call.kwargs.items() if key != "input"},
                             {"capture_output": True, "timeout": 9, "check": False})

    def test_text_options_keep_payload_compatible_with_subprocess(self):
        for options in ({"text": True}, {"universal_newlines": True}, {"encoding": "utf-8"}, {"errors": "replace"}):
            with self.subTest(options=options), patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 0)) as guest:
                vm.guest_script(SimpleNamespace(name="owned"), "Write-Output 'é'", **options)
                self.assertIsInstance(guest.call_args.kwargs["input"], str)
                for key, value in options.items():
                    self.assertEqual(guest.call_args.kwargs[key], value)

    def test_remote_failure_and_transport_exception_propagate(self):
        machine = SimpleNamespace(name="owned")
        with patch.object(vm, "guest", return_value=subprocess.CompletedProcess([], 37)):
            with self.assertRaisesRegex(SystemExit, "owned VM exited with status 37"):
                vm.guest_script(machine, "exit 37")
        failure = subprocess.TimeoutExpired("ssh", 3)
        with patch.object(vm, "guest", side_effect=failure), self.assertRaises(subprocess.TimeoutExpired) as raised:
            vm.guest_script(machine, "Write-Output 'waiting'", timeout=3)
        self.assertIs(raised.exception, failure)


if __name__ == "__main__":
    unittest.main()
