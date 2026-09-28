# Commit guide

[Technical documentation](README.md)

## Hooks and attribution

Do not attribute commits to AI assistants or coding agents with `Co-Authored-By`
trailers. This includes Claude, Codex, Grok, ChatGPT, Gemini, Copilot, Cursor and
other agents. Human coauthors are allowed; ordinary mentions of an agent in the
commit description are allowed.

After cloning, install the local guards from the repository root:

```sh
sh tools/git/install-hooks.sh
```

The installer shares the guards across linked worktrees. Run it again after the
tracked hooks change. It refuses to replace unrelated hooks or override
`core.hooksPath`; integrate the guards into existing hooks in that case. Do not
bypass them.

`commit-msg` rejects agent attribution before a commit is created. `pre-push`
checks the complete ancestry of every pushed commit or tag, including merge
parents, so merging an old branch cannot restore prohibited trailers. Remove the
trailers from the affected history before pushing. To audit a branch or test the
guards:

```sh
sh .githooks/check-commit-messages.sh history HEAD
python3 -m unittest discover -s tools/git -p 'test_*.py'
```

## Sharing `main`

Work lands on `origin/main` directly, and other sessions push to it while you
work. There are no long-lived port or feature branches.

- Branch from a fresh `origin/main` in your own worktree.
- Before pushing, fetch and rebase onto `origin/main`, resolve conflicts by
  keeping both sides' intent, and rerun your checks on the rebased tree.
- Never force-push `main`, reset another session's branch, or drop commits you
  did not write. Never use a bare `git stash`; the stash is shared by every
  worktree.

## Commit messages

- The subject is an imperative sentence in sentence case, without a prefix or a
  trailing period, and says what changes for the user or the code, for example
  "Keep drag release frames free of allocation".
- The body explains what changed and why, in prose or bullets.
- End with the evidence: a `Tests:` line naming the suites run, and for
  performance changes `Measured on <device> at <commit>: before -> after`. For
  refactors, give the net line count.

## Before merging to main

- **Commit milestones, not steps.** Commit only complete, self-contained
  milestones that build, pass their checks and leave the feature usable. Finish
  or squash small tasks, fixups and experiments locally first.
- **Replace obsolete paths.** Delete superseded code in the same change instead
  of layering another implementation beside it. Refactors should not add lines;
  explain any growth.
- **Keep logic shared.** Business rules, state transitions, validation, history
  and UI text belong in shared Rust. Host code presents that state and forwards
  native input.
- **Protect responsiveness.** Look for new work on the UI thread, blocking calls,
  per-frame allocations, copies, synchronization and repeated computation.
  Measure affected frame paths against the
  [performance targets](PERFORMANCE_TARGETS.md).
- **Review the diff for these recurring problems:**
  - a separate code path per variant where one path with a parameter would do;
  - a new helper, pipeline or test harness that duplicates an existing one;
  - copy-pasted blocks inside one function or across tests;
  - policy implemented once per host instead of once in Rust;
  - test-only switches in production types;
  - explanatory or narrative comments;
  - dead code, unused parameters and leftovers from earlier attempts.
- **Keep documentation durable.** Update the guides the change affects and follow
  [writing](development/writing.md): no work logs, dumps or generated output.
- **Validate the final diff.** Run the checks in [testing](development/testing.md)
  for every affected area, and exercise affected UI and input on each affected
  host. Review every changed file for scope and accidental edits. Report what
  was not verified and any remaining failures.
