# Capy Canvas agent guide

Capy Canvas is a painting and photo editor with a shared Rust core (`crates/`) and
native clients (`apps/`) for GTK, Web, Android, Apple and Windows. Read
[architecture](docs/architecture.md) before your first change. The rules below
apply to every task; the [map](#where-to-find-the-rest) points to the guides for
specific tasks.

## Hard rules

### Working alongside other sessions

- Other people and agents commit, build and use devices on this machine and push
  to `origin/main` while you work. Use the worktree named by the user or provided
  as the task's working directory; it already provides isolation.
  Create another worktree only when none is assigned or the user requests one,
  and put it under `/home`, never `/tmp`
  ([environment](docs/development/environment.md)).
- Fetch and rebase onto `origin/main` before pushing, then rerun your checks.
  Never force-push `main`, `git reset --hard` or bare `git stash` in a shared
  checkout, or delete another session's worktree, branch, build, install or files.
- Commit complete milestones that build and pass, not steps. Push when your task
  says to.
- Install the hooks with `sh tools/git/install-hooks.sh` and never bypass them.
  No `Co-Authored-By` trailers for AI assistants or agents; human coauthors are
  fine. Follow the [commit guide](docs/COMMIT_GUIDE.md).

### Architecture

- Business rules, state, validation, undo history and UI text live in shared
  Rust. Hosts present that state and forward native input; native timing and
  input capture stay in the host. Never implement a policy once per host.
- Fix a bug that one host reveals in shared Rust, with a test that runs without
  a window, not with a host workaround.
- Never wait on the GPU, block on I/O, decode images or copy pixels on a UI or
  input thread. Painting requires a hardware GPU; there is no CPU fallback.

### Code

- Replace, don't layer: delete superseded code in the same change, reuse existing
  helpers and test harnesses, and keep refactors from adding lines.
- Fix the class of failure, not the instance. Remove earlier fixes that did not
  work, and add a regression test that fails without your fix. No special-case
  patches.
- Write self-documenting code. No explanatory, narrative or temporal comments.
  No test-only switches in production types.
- Artwork stores authored data, not built-in shaders, UI metadata or GPU layouts.
  Keep stable filter/parameter/choice IDs and all values, including defaults.
  A concrete data conversion needs a fixed-file regression test; do not add
  shader generations or generic schema migration machinery. Follow the
  [package contract](docs/reference/capy-package.md).
- The app is pre-release: no backward compatibility, migrations or readers for
  old formats unless the task asks for them.
- Add dependencies or vendored code only when necessary, and never copy GPL code
  or another app's artwork ([publication](docs/development/publication.md)).
- Never run `cargo fmt` across the workspace; most files are not rustfmt-formatted,
  and doing so rewrites hundreds of them.

### Performance

- Every motion must hold its tier's frame rate on the tier's canvas; only frames
  where something moves count. See [performance targets](docs/PERFORMANCE_TARGETS.md).
- Check every change that can affect how frames are produced against the targets,
  measure the affected rows by [its rules](docs/performance/measuring.md), and
  record the results in the tier tables. Report a target as met only from current
  measurements on that tier's reference hardware.

### Finishing work

- Do what was asked. Offer anything else as a suggestion instead of doing it.
  Decide how you will know the task is done, and stop when that check passes.
- Done means you walked the real user journeys on every affected host, in light
  and dark themes for UI, and ran the [checks for your change](docs/development/testing.md).
  Report anything you did not verify and every failure.
- Plans, handoffs and history docs are not evidence that something works. Check
  the code.

### Devices and user data

- Reserve a tablet before using it and run every device command through
  `tools/devices/devices.py run` ([devices](docs/development/devices.md)). Use
  only a device you were assigned or reserved; leave alone any device you are
  told belongs to someone else.
- Install test builds under your own application ID. Never uninstall or clear
  `art.capycanvas`, or touch another session's installs, ports or browser tabs.
- Never run tests against real user settings, documents or profiles, and inject
  input only into the private test display. Close only processes you started.

### Docs and text

- Update the guide a change affects in the same commit. Commit only docs that
  stay useful; logs, measurements, inventories and handoffs stay in `artifacts/`
  or `*.local.md` ([writing](docs/development/writing.md)).
- UI text uses plain words a painter knows. Don't add settings, menu items or
  explanatory text where better behaviour would do.
- Update every registered Fluent catalog when adding UI text; run the
  [localization checks](docs/ui/localization.md#adding-ui-text).

## Where to find the rest

| When you | Read |
| --- | --- |
| Start a task | [Developer guide](docs/development/README.md), [environment](docs/development/environment.md) |
| Build, run or test a client | [Linux](docs/development/linux.md), [Web](docs/development/web.md), [Android](docs/development/android.md), [Apple](docs/development/apple.md), [Windows](docs/development/windows.md), [Windows VM](docs/development/windows-vm.md) |
| Choose the checks for a change | [Testing](docs/development/testing.md) |
| Use a tablet, VM, Mac or iPad | [Devices](docs/development/devices.md) |
| Commit, rebase or push | [Commit guide](docs/COMMIT_GUIDE.md) |
| Touch brushes, shaders or anything that moves on screen | [Performance targets](docs/PERFORMANCE_TARGETS.md), [measuring](docs/performance/measuring.md) |
| Change UI | [Workspace and UI](docs/ui/README.md); draggable UI must follow the [drag convention](docs/ui/drag-and-reorder.md) |
| Port a feature to Apple or Windows | [Apple porting](docs/APPLE_PORTING_GUIDE.md), [Windows porting](docs/WINDOWS_PORTING_GUIDE.md) |
| Write docs, UI text or a handoff | [Writing](docs/development/writing.md) |
| Understand a subsystem | [Technical documentation](docs/README.md) |
| Add a dependency, vendor code or publish | [Publication](docs/development/publication.md), [vendored crates](vendor/README.md) |
