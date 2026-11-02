import argparse
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import run


class RunnerTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.args = argparse.Namespace(mode="rust", releases="all", work=self.base / "work", keep=False)
        self.console = contextlib.redirect_stdout(io.StringIO())
        self.console.__enter__()
        self.addCleanup(self.console.__exit__, None, None, None)

    def repository(self):
        repo = self.base / "repo"
        repo.mkdir()
        subprocess.run(["git", "init", "--initial-branch=main"], cwd=repo, check=True, capture_output=True)
        for key, value in [("user.name", "Compatibility Test"), ("user.email", "compat@example.com"),
                           ("commit.gpgsign", "false")]:
            subprocess.run(["git", "config", key, value], cwd=repo, check=True)
        subprocess.run(["git", "commit", "--allow-empty", "-m", "Fixture"],
                       cwd=repo, check=True, capture_output=True)
        return repo

    def test_failures_keep_the_report_and_log_but_remove_scratch(self):
        runner = run.Runner(self.args)
        self.assertFalse(runner.check("failing", [sys.executable, "-c", "print('evidence'); exit(7)"], self.base))
        self.assertEqual(runner.finish(), 1)
        report = json.loads((runner.work / "report.json").read_text())
        self.assertEqual(report["checks"][0]["status"], "FAIL")
        self.assertIn("evidence", (runner.work / report["checks"][0]["log"]).read_text())
        self.assertFalse(runner.scratch.exists())

    def test_keep_retains_inputs_and_existing_scratch_is_never_deleted(self):
        self.args.keep = True
        runner = run.Runner(self.args)
        marker = runner.scratch / "keep"
        marker.write_text("retained")
        self.assertEqual(runner.finish(), 0)
        with self.assertRaises(FileExistsError):
            run.Runner(self.args)
        self.assertEqual(marker.read_text(), "retained")

    def test_default_releases_use_numeric_order_and_exclude_older_and_prerelease_tags(self):
        repo = self.repository()
        for tag in ["v1.0.14", "v1.10.0", "v1.2.0", "v1.0.15", "v1.2.0-beta"]:
            subprocess.run(["git", "tag", tag], cwd=repo, check=True)
        runner = run.Runner(self.args, repo)
        runner.select_releases()
        self.assertEqual(runner.releases, ["v1.0.15", "v1.2.0", "v1.10.0"])
        runner.finish()

    def test_release_worktree_is_removed_even_when_its_check_raises(self):
        repo = self.repository()
        subprocess.run(["git", "tag", "v1.0.15"], cwd=repo, check=True)
        runner = run.Runner(self.args, repo)
        self.args.keep = True
        with self.assertRaisesRegex(RuntimeError, "fixture failure"):
            with runner.release_tree("v1.0.15") as tree:
                self.assertTrue((tree / ".git").exists())
                raise RuntimeError("fixture failure")
        self.assertFalse(tree.exists())
        self.assertEqual(runner.git("worktree", "list", "--porcelain").count("worktree "), 1)
        runner.finish()

    def test_explicit_releases_require_tags_instead_of_revision_expressions(self):
        repo = self.repository()
        subprocess.run(["git", "tag", "v1.0.15"], cwd=repo, check=True)
        self.args.releases = "v1.0.15"
        runner = run.Runner(self.args, repo)
        runner.select_releases()
        self.assertEqual(runner.releases, ["v1.0.15"])
        self.args.releases = "v1.0.15~0"
        with self.assertRaises(subprocess.CalledProcessError):
            runner.select_releases()
        runner.finish()

    def test_a_successful_command_that_generates_no_inputs_fails(self):
        runner = run.Runner(self.args)
        output = runner.scratch / "empty"
        output.mkdir()
        self.assertFalse(runner.check("empty", [sys.executable, "-c", "pass"], self.base, output=output))
        self.assertEqual(runner.finish(), 1)

    def test_inherited_compatibility_controls_cannot_bless_or_redirect_the_run(self):
        with patch.dict(run.os.environ, CAPY_BLESS="1", CAPY_COMPAT_IN="elsewhere", CAPY_COMPAT_OUT="elsewhere"):
            runner = run.Runner(self.args)
        self.assertNotIn("CAPY_BLESS", runner.env)
        self.assertNotIn("CAPY_COMPAT_IN", runner.env)
        self.assertNotIn("CAPY_COMPAT_OUT", runner.env)
        self.assertTrue(Path(runner.env["CAPY_STORAGE_DIR"]).is_relative_to(runner.work))
        runner.finish()

    def test_release_reports_reject_resets_but_user_reports_keep_the_outcome(self):
        report = dict(startup="stored", notices=[], editing_dropped=False,
                      stores={"workspaces": dict(loaded=True)}, sessions=[dict(loaded=True)])
        log = self.base / "native.json"
        for field, value in [("startup", "set aside"), ("startup", "in memory"), ("editing_dropped", True),
                             ("sessions", [dict(loaded=False)]), ("stores", {"workspaces": dict(loaded=False)})]:
            with self.subTest(field=field, value=value):
                damaged = dict(report, **{field:value})
                log.write_text(json.dumps(damaged))
                self.assertTrue(run.saved_data_result(log, True)[0])
                reason, details = run.saved_data_result(log, False)
                self.assertEqual(reason, "")
                self.assertEqual(details["report"], damaged)
        log.write_text(json.dumps(report))
        self.assertEqual(run.saved_data_result(log, True)[0], "")

    def test_release_assembly_has_native_paths_and_preserves_source_bytes(self):
        destination = self.base / "assembled"
        source = run.ROOT / "crates/layer-workspace/tests/compat/1.0.15/workspaces.sqlite3"
        before = source.read_bytes()
        run.assemble_release(run.ROOT, "v1.0.15", destination)
        self.assertEqual((destination / "data/workspaces/workspaces.sqlite3").read_bytes(), before)
        self.assertEqual(source.read_bytes(), before)
        for name in ["config/settings.json", "config/export-presets", "data/papers/index.json",
                     "state/sessions/session.json", "state/sessions/drawing-a/head.json",
                     "state/sessions/session-icc.json", "state/sessions/drawing-icc/head.json"]:
            self.assertTrue((destination / name).is_file(), name)
        self.assertTrue(list((destination / "data/color-profiles").glob("*.icc")))

    def test_windows_without_a_local_pool_reports_a_skip(self):
        runner = run.Runner(self.args)
        with patch.object(run.os, "access", return_value=False), patch.object(run.subprocess, "run") as command:
            runner.windows()
        command.assert_not_called()
        self.assertEqual(runner.checks[0]["status"], "SKIP")
        self.assertIn("Linux/KVM", runner.checks[0]["reason"])
        self.assertEqual(runner.finish(), 0)

    def test_windows_uses_one_cpu_runner_and_stops_only_the_primary_it_started(self):
        for running in [False, True]:
            with self.subTest(running=running):
                runner = run.Runner(self.args)
                runner.env["CAPYCANVAS_VM"] = "compat-test"
                listing = "compat-test\t" + ("running, ssh port 12345" if running else "stopped")
                responses = [subprocess.CompletedProcess([], 0, stdout=listing),
                             subprocess.CompletedProcess([], 7)]
                if not running:
                    responses.append(subprocess.CompletedProcess([], 0))
                with patch.object(run.sys, "platform", "linux"), patch.object(run.os, "access", return_value=True), \
                     patch.object(Path, "is_file", return_value=True), \
                     patch.object(run.subprocess, "run", side_effect=responses) as command:
                    runner.windows()
                fixture = command.call_args_list[1]
                self.assertEqual(fixture.args[0][-4:], ["fixtures", "--runners", "1", "saved-data"])
                self.assertEqual(fixture.args[0][2:4], ["--vm", "compat-test"])
                self.assertEqual(fixture.kwargs["env"]["CAPYCANVAS_VM_ARTIFACTS"], str(runner.work / "windows"))
                self.assertEqual(command.call_count, 2 if running else 3)
                if not running:
                    self.assertEqual(command.call_args.args[0][-1], "stop")
                self.assertEqual(runner.finish(), 1)


if __name__ == "__main__":
    unittest.main()
