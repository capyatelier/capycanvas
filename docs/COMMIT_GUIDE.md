# Commit guide

[Technical documentation](README.md)

## Review the diff

Review your whole diff before every commit, and review another agent's diffs
the same way before accepting its results. Tests, benchmark harnesses, tools and
docs get the same review as production code. Look for:

- a workaround that hides a cause, such as replaying, retrying, waiting or
  re-reporting, instead of fixing the cause;
- edits outside the task or made by accident;
- a separate code path per variant where one path with a parameter would do;
- a helper, harness or collector that duplicates an existing one, or that copies
  another component's logic instead of calling it;
- copy-pasted blocks inside one function or across tests;
- policy implemented once per host instead of once in Rust;
- test-only switches in production types;
- explanatory or narrative comments;
- dead code: unused parameters, modes, flags and report fields, leftovers from
  earlier attempts and probes from a finished investigation;
- a regression test that only runs behind a flag or a special build;
- new UI-thread work, blocking calls, per-frame allocations, copies or repeated
  computation on a frame path, unmeasured against the
  [performance targets](PERFORMANCE_TARGETS.md);
- docs that log results or explain internals instead of stating how-tos
  ([writing](development/writing.md)).

Delete superseded code in the same change. Refactors should not add lines.

## Commit messages

- The subject is an imperative sentence in sentence case, without a prefix or a
  trailing period, and says what changes for the user or the code, for example
  "Keep drag release frames free of allocation".
- The body explains what changed and why, in prose or bullets.
- End with the evidence: a `Tests:` line naming the suites run, for performance
  changes `Measured on <device> at <commit>: before -> after`, and the net line
  count for production, tests and tools, and docs. Explain any growth.

## Sharing `main`

Work lands on `origin/main` directly, and other sessions push to it while you
work. There are no long-lived port or feature branches.

- Branch from a fresh `origin/main` in your own worktree.
- Commit only complete milestones: one fix or feature with its tests that
  builds, passes its checks and leaves the feature usable. Squash fixups and
  experiments locally. Push each one as soon as its checks pass; never hold
  finished commits for later work. When in doubt, push first, then validate
  further and fix what you find in a separate commit.
- Before pushing, run the [checks](development/testing.md) for every affected
  area, exercise affected UI and input on each affected host, and report what
  was not verified and any remaining failures.
- Fetch and rebase onto `origin/main` and resolve conflicts by keeping both
  sides' intent. Rerun the checks for conflicted files or code that upstream
  also changed; don't restart heavy validation for unrelated upstream changes.
- Never force-push `main`, reset another session's branch, or drop commits you
  did not write. Never use a bare `git stash`; the stash is shared by every
  worktree.

## Hooks and attribution

Do not attribute commits to AI assistants or coding agents with `Co-Authored-By`
trailers, including Claude, Codex, Grok, ChatGPT, Gemini, Copilot and Cursor.
Human coauthors and ordinary mentions of an agent in the description are allowed.

Install the guards from the repository root after cloning, and again after the
tracked hooks change; on Windows, use Git Bash. The installer shares them across
linked worktrees. It refuses to replace unrelated hooks or override
`core.hooksPath`; integrate the guards into existing hooks then. Do not bypass
them.

```sh
sh tools/git/install-hooks.sh
```

`commit-msg` rejects agent attribution. `pre-push` checks the full ancestry of
every pushed commit or tag, including merge parents; remove prohibited trailers
from that history before pushing. To audit a branch or test the guards:

```sh
sh .githooks/check-commit-messages.sh history HEAD
python3 -m unittest discover -s tools/git -p 'test_*.py'
```
