"""Exercise the installed hooks against disposable repositories and real pushes."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "tools/git/install-hooks.sh"


class CommitHooksTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo"
        self.remote = Path(self.temp.name) / "remote.git"
        self.env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.run_command(["git", "init", "--initial-branch=main", str(self.repo)])
        self.run_command(["git", "init", "--bare", str(self.remote)])
        self.git("config", "user.name", "Hook Test")
        self.git("config", "user.email", "test@example.com")
        self.run_command(["sh", str(INSTALLER)], cwd=self.repo)
        self.git("remote", "add", "origin", str(self.remote))
        self.git("commit", "--allow-empty", "-m", "Initial commit")
        self.git("push", "origin", "main")

    def run_command(self, args, *, cwd=None, input=None, ok=True):
        result = subprocess.run(
            args, cwd=cwd, env=self.env, input=input, text=True, capture_output=True
        )
        if ok:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def git(self, *args, **kwargs):
        return self.run_command(["git", *args], cwd=self.repo, **kwargs)

    def raw_commit(self, message, ref="refs/heads/main", parents=None):
        # Plumbing creates deliberately invalid fixtures without invoking hooks.
        if parents is None:
            parents = [self.git("rev-parse", ref).stdout.strip()]
        tree = self.git("rev-parse", "HEAD^{tree}").stdout.strip()
        args = ["commit-tree", tree]
        for parent in parents:
            args.extend(["-p", parent])
        oid = self.git(*args, input=message + "\n").stdout.strip()
        self.git("update-ref", ref, oid)
        return oid

    def assert_rejected(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("AI agents must not appear", result.stderr)

    def test_commit_rejects_major_agents_case_insensitively(self):
        agents = [
            "Claude Opus 5.5", "Codex", "Grok", "ChatGPT", "GPT-5", "GPT5",
            "OpenAI", "Anthropic", "Gemini", "GitHub Copilot", "Cursor",
            "Windsurf", "Codeium", "Devin", "Aider", "Cline", "Roo Code",
            "DeepSeek", "Qwen", "Kimi", "Mistral", "Perplexity", "Jules",
            "OpenHands", "SWE-agent", "Amazon Q", "CodeWhisperer", "Augment Code",
            "Tabnine", "Replit Agent", "Goose", "Factory Droid", "Continue",
            "Bolt", "Lovable", "v0", "AI assistant",
        ]
        before = self.git("rev-parse", "HEAD").stdout
        for agent in agents:
            with self.subTest(agent=agent):
                result = self.git(
                    "commit", "--allow-empty", "-m",
                    f"Change\n\n  cO-aUtHoReD-bY : {agent.lower()} <bot@example.com>",
                    ok=False,
                )
                self.assert_rejected(result)
                self.assertEqual(self.git("rev-parse", "HEAD").stdout, before)

    def test_humans_and_prose_are_allowed(self):
        self.git("commit", "--allow-empty", "-m",
                 "Discuss Codex and Claude\n\nCo-Authored-By: Jane Smith <jane@example.com>")
        self.git("push", "origin", "main")

    def test_agent_email_and_crlf_message_are_rejected(self):
        message = Path(self.temp.name) / "message"
        message.write_bytes(b"Change\r\n\r\nCo-Authored-By: Assistant <noreply@anthropic.com>\r\n")
        self.assert_rejected(self.git("commit", "--allow-empty", "-F", str(message), ok=False))

    def test_missing_message_fails_closed(self):
        result = self.run_command(
            ["sh", str(self.repo / ".git/hooks/commit-msg"), "missing-message"],
            cwd=self.repo, ok=False,
        )
        self.assertNotEqual(result.returncode, 0)

    def test_push_checks_noncurrent_branch_and_merged_ancestors(self):
        clean = self.git("rev-parse", "HEAD").stdout.strip()
        bad = self.raw_commit("Old change\n\nCo-Authored-By: Codex <bot@example.com>",
                              "refs/heads/topic", [clean])
        self.raw_commit("Merge topic", "refs/heads/to-push", [clean, bad])
        self.assert_rejected(self.git("push", "origin", "main", "to-push", ok=False))
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/to-push").stdout, "")

    def test_push_rejects_already_published_bad_ancestry(self):
        self.raw_commit("Old change\n\nCo-Authored-By: Grok <bot@example.com>")
        # Seed the disposable remote as if this history predated the policy.
        self.run_command(["git", "--git-dir", str(self.remote), "fetch", str(self.repo),
                          "main:refs/heads/main"])
        self.git("commit", "--allow-empty", "-m", "Clean tip")
        self.assert_rejected(self.git("push", "origin", "main", ok=False))

    def test_tags_check_commit_ancestry_and_allow_noncommit_objects(self):
        self.git("tag", "-a", "clean-tag", "-m", "Clean tag")
        self.git("push", "origin", "clean-tag")
        tree = self.git("rev-parse", "HEAD^{tree}").stdout.strip()
        self.git("tag", "-a", "tree-tag", tree, "-m", "Tree tag")
        self.git("push", "origin", "tree-tag")
        self.raw_commit("Bad change\n\nCo-Authored-By: Gemini <bot@example.com>")
        self.git("tag", "-a", "bad-tag", "-m", "Bad tag")
        self.assert_rejected(self.git("push", "origin", "bad-tag", ok=False))

    def test_force_push_is_checked_and_deletion_is_allowed(self):
        self.git("push", "origin", "main:refs/heads/topic")
        self.raw_commit("Replacement\n\nCo-Authored-By: Copilot <bot@example.com>",
                        "refs/heads/topic", [])
        self.assert_rejected(self.git("push", "--force", "origin", "topic", ok=False))
        self.git("push", "origin", ":refs/heads/topic")

    def commit_file(self, path, text, message="Write fixture"):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        self.git("add", path)
        self.git("commit", "-m", message)

    def released_input(self):
        path = "crates/layer-ui/tests/compat/1.0.15/settings.json"
        self.commit_file(path, "released bytes")
        self.git("push", "origin", "main")
        return path, self.git("rev-parse", "HEAD").stdout.strip()

    def assert_input_rejected(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("released compatibility inputs", result.stderr)

    def assert_expectation_rejected(self, result, commit):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            'error: expected compatibility values changed without a "Compatibility:" '
            'line saying which values changed and why no released value was lost '
            f'({commit}).', result.stderr,
        )

    def test_release_inputs_allow_additions_and_reviewed_expectations(self):
        self.released_input()
        expected = "crates/layer-ui/tests/compat/expected/1.0.15/settings.json"
        for value in ["first expectation", "reviewed expectation"]:
            self.commit_file(expected, value, "Write fixture\n\nCompatibility: Review saved settings values")
            self.git("push", "origin", "main")
        self.commit_file("crates/layer-core/tests/compat/1.0.16/artwork.capy", "next release")
        self.git("push", "origin", "main")

    def test_expected_additions_and_edits_require_compatibility_line(self):
        expected = "crates/layer-ui/tests/compat/expected/1.0.15/settings.json"
        guard = self.repo / ".git/hooks/check-commit-messages.sh"
        for value in ["first expectation", "reviewed expectation"]:
            with self.subTest(value=value):
                base = self.git("rev-parse", "HEAD").stdout.strip()
                self.commit_file(expected, value)
                commit = self.git("rev-parse", "HEAD").stdout.strip()
                self.assert_expectation_rejected(self.git("push", "origin", "main", ok=False), commit)
                self.assert_expectation_rejected(self.run_command(
                    ["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo, ok=False,
                ), commit)
                self.git("commit", "--amend", "-m", "Write fixture\n\nCompatibility: Review saved settings values")
                self.run_command(["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo)
                self.git("push", "origin", "main")

    def test_commits_without_expected_changes_need_no_compatibility_line(self):
        base = self.git("rev-parse", "HEAD").stdout.strip()
        self.commit_file("docs/example.md", "Documentation")
        guard = self.repo / ".git/hooks/check-commit-messages.sh"
        self.run_command(["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo)
        self.git("push", "origin", "main")

    def test_expected_compatibility_line_checks_merges_and_ancestors(self):
        base = self.git("rev-parse", "HEAD").stdout.strip()
        expected = "crates/layer-ui/tests/compat/expected/1.0.15/settings.json"
        guard = self.repo / ".git/hooks/check-commit-messages.sh"
        message = "Review fixture\n\nCompatibility: Retain saved settings values"
        self.git("checkout", "-b", "reviewed")
        self.commit_file(expected, "reviewed expectation", message)
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "reviewed", "-m", "Merge reviewed fixture")
        merge = self.git("rev-parse", "HEAD").stdout.strip()
        self.assert_expectation_rejected(self.git("push", "origin", "main", ok=False), merge)
        self.assert_expectation_rejected(self.run_command(
            ["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo, ok=False,
        ), merge)
        self.git("commit", "--amend", "-m", message)
        self.run_command(["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo)
        self.git("push", "origin", "main")
        base = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("checkout", "-b", "unreviewed")
        self.commit_file(expected, "unreviewed expectation")
        bad = self.git("rev-parse", "HEAD").stdout.strip()
        self.commit_file(expected, "reviewed expectation", message)
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "unreviewed", "-m", message)
        self.assert_expectation_rejected(self.git("push", "origin", "main", ok=False), bad)
        self.assert_expectation_rejected(self.run_command(
            ["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo, ok=False,
        ), bad)

    def test_release_input_edits_deletions_and_renames_are_rejected(self):
        path, base = self.released_input()
        for change in ["edit", "delete", "rename"]:
            with self.subTest(change=change):
                self.git("checkout", "-b", change, base)
                if change == "edit":
                    (self.repo / path).write_text("rewritten bytes")
                    self.git("add", path)
                elif change == "delete":
                    self.git("rm", path)
                else:
                    self.git("mv", path, path + ".renamed")
                self.git("commit", "-m", "Change released input")
                self.assert_input_rejected(self.git("push", "origin", change, ok=False))

    def test_ci_release_input_range_checks_intermediate_edits(self):
        path, base = self.released_input()
        guard = self.repo / ".git/hooks/check-commit-messages.sh"
        self.commit_file("crates/layer-ui/tests/compat/1.0.16/settings.json", "new input")
        self.run_command(["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo)
        self.commit_file(path, "rewritten")
        self.commit_file(path, "released bytes")
        self.assert_input_rejected(self.run_command(
            ["sh", str(guard), "release-inputs", base, "HEAD"], cwd=self.repo, ok=False,
        ))

    def test_release_input_guard_reads_all_push_refs_once(self):
        path, base = self.released_input()
        self.git("checkout", "-b", "clean", base)
        self.git("commit", "--allow-empty", "-m", "Clean branch")
        self.git("checkout", "-b", "bad", base)
        self.commit_file(path, "rewritten")
        self.assert_input_rejected(self.git("push", "origin", "clean", "bad", ok=False))
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/clean").stdout, "")

    def test_new_ref_does_not_recheck_already_published_input_changes(self):
        path, _ = self.released_input()
        self.commit_file(path, "historically published replacement")
        self.run_command(["git", "--git-dir", str(self.remote), "fetch", str(self.repo), "main:refs/heads/main"])
        self.git("fetch", "origin")
        self.git("checkout", "-b", "new-ref")
        self.git("commit", "--allow-empty", "-m", "New work")
        self.git("push", "origin", "new-ref")

    def test_release_input_guard_checks_merged_history_and_tags(self):
        path, base = self.released_input()
        self.git("checkout", "-b", "topic", base)
        self.commit_file(path, "rewritten")
        self.git("tag", "-a", "bad-input", "-m", "Changed input")
        self.assert_input_rejected(self.git("push", "origin", "bad-input", ok=False))
        self.commit_file(path, "released bytes")
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "topic", "-m", "Merge restored input")
        self.assert_input_rejected(self.git("push", "origin", "main", ok=False))

    def test_installed_hooks_cover_older_linked_worktrees(self):
        worktree = Path(self.temp.name) / "worktree"
        self.git("worktree", "add", "--detach", str(worktree), "HEAD")
        result = self.run_command(
            ["git", "commit", "--allow-empty", "-m",
             "Change\n\nCo-Authored-By: Claude <bot@example.com>"],
            cwd=worktree, ok=False,
        )
        self.assert_rejected(result)

    def test_installer_updates_own_hooks_but_preserves_custom_hooks(self):
        self.run_command(["sh", str(INSTALLER)], cwd=self.repo)
        hook = self.repo / ".git/hooks/commit-msg"
        hook.write_text("#!/bin/sh\n# Custom hook\nexit 0\n")
        result = self.run_command(["sh", str(INSTALLER)], cwd=self.repo, ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Custom hook", hook.read_text())
        self.git("config", "core.hooksPath", "custom-hooks")
        result = self.run_command(["sh", str(INSTALLER)], cwd=self.repo, ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.git("config", "core.hooksPath").stdout.strip(), "custom-hooks")


if __name__ == "__main__":
    unittest.main()
