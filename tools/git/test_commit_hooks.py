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
            [str(self.repo / ".git/hooks/commit-msg"), "missing-message"],
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
