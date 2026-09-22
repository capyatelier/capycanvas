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
