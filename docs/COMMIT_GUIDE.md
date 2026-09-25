# Commit and push checks

Do not attribute commits to AI assistants or coding agents with `Co-Authored-By`
trailers. This includes Claude, Codex, Grok, ChatGPT, Gemini, Copilot, Cursor, and
other agents. Human coauthors are allowed; ordinary mentions of an agent in the
commit description are allowed.

After cloning, install the local guards from the repository root:

```sh
sh tools/git/install-hooks.sh
```

The installer shares the guards across linked worktrees, including older branches.
Run it again after updating the tracked hooks. It refuses to replace unrelated
hooks or override `core.hooksPath`; integrate the guards into existing hooks in
that case. Git does not install hooks automatically in other clones, and these
local checks are not a server-side branch rule. Do not bypass them.

`commit-msg` rejects agent attribution before a commit is created. `pre-push`
checks the complete ancestry of every pushed commit or tag, including merge
parents, so merging an old branch cannot restore prohibited trailers. Remove
the trailers from the affected history before pushing. Ref deletions and tags
pointing at non-commit objects do not introduce commit messages.

To audit a branch manually or validate changes to the guards:

```sh
sh .githooks/check-commit-messages.sh history HEAD
python3 -m unittest discover -s tools/git -p 'test_*.py'
```

# Before merging to main

- **Replace obsolete paths.** Refactor or delete superseded code instead of
  layering another implementation alongside it. For pure refactors, aim for
  neutral or fewer lines of code; explain any necessary growth.
- **Keep logic shared.** Put business rules, state transitions, validation, and
  history in the shared Rust core. Keep platform UI code focused on presenting
  core state and forwarding native input, with native timing and capture in the
  host where required.
- **Protect responsiveness.** Review changes for unintended work on the UI
  thread, blocking calls, allocations, copies, synchronization, and repeated
  computation. Check affected brush and frame generation paths for regressions;
  measure relevant timings when performance could change.
- **Keep durable documentation.** Commit documentation and work logs only when
  they will help future contributors. Prefer concise explanations of behavior,
  decisions, and reproducible checks. Exclude transient logs, massive text or
  code dumps, generated artifacts, and duplicated source.
- **Validate the final diff.** Run relevant tests, builds, and required checks;
  inspect affected UI and input behavior. Review every changed file for scope,
  accidental edits, dead code, and missing cleanup. Record material limitations
  and explain any remaining failures before merging.
