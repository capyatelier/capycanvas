# Windows porting guide

How to bring the Windows app (`apps/layer-windows`) back to parity with the
GTK, Web and Android apps after upstream work lands on `main`. The goal is
parity with those hosts and no performance regression.

## 1. Audit

1. Fetch and fast-forward `main`. Find the last commit that ported upstream work
   to Windows (the most recent `Port … to Windows` commit, or the upstream
   reference recorded in the newest `docs/development/windows-*.md` note).
   Confirm that commits just before that point were really ported or are
   specific to another host.
2. Review every commit on `origin/main` after that point. Include merge commits
   and integration commits: their conflict resolutions can change behavior or
   shared interfaces.
3. For each user-visible feature or behavior change, record:
   - the behavior, device rules (mouse, touch, pen) and layout sizes;
   - the source commits and the GTK/Web/Android implementations to mirror;
   - the shared Rust API that the hosts consume;
   - the current Windows status (missing, partial, handled by shared code, or
     not applicable), with evidence from the Windows source;
   - the Windows files and bridge functions to change, performance risks and
     tests to add.
4. Check the shared interfaces as well as the features. The WinUI code reads
   snapshot JSON fields and sends action names as strings, so a renamed or
   removed shared field fails silently instead of breaking the build. Compare
   the fields and actions Windows uses with the serialized shared types, and
   list new panels, controls, inputs, host requests and settings that Windows
   does not consume yet.
5. Include parity gaps that predate the reviewed commits: open items in the
   Windows development notes, the [drag inventory](ui/drag-inventory.md), and
   features present on GTK, Web or Android but missing on Windows.
6. Keep this inventory outside version control, under the ignored
   `artifacts/windows/` directory. It is a working report, not documentation.

## 2. Record a baseline

Before changing Windows code, build the current `main` in Release mode and keep
a copy of the executable under `artifacts/windows/`. Record the unit test
results, strict Windows Clippy, the `brush_frames` renderer timings, and the
real-window pen latency workload from
[the pen latency note](development/windows-pen-latency-20260920.md). Upstream
changes can shift these numbers before any Windows work starts. The baseline
separates those shifts from regressions caused by the port.

## 3. Port

- Follow [the commit guide](COMMIT_GUIDE.md) and [AGENTS.md](../AGENTS.md),
  including the [drag and reorder convention](ui/drag-and-reorder.md).
- Keep business rules, validation, state transitions and history in shared Rust.
  Windows code presents shared state and forwards native input; native timing
  and input capture stay in the host. Do not copy logic that another host
  already consumes from a shared crate.
- **Workspace and canvas:** match the Web/GTK behavior and visuals (layout,
  docking, hit targets, gestures, canvas overlays, cursors, corner shapes).
  Check visual parity against the Web app with the matched-capture tooling in
  [windows.md](development/windows.md#debugging-and-visual-checks) when needed.
- **Menus, dialogs and settings:** WinUI-native styling is acceptable, provided
  the general layout, sizing and behavior match the other hosts.
- **Upstream keeps moving:** re-fetch `origin/main` throughout the port,
  at least at every milestone and before every push. Add new commits to the
  inventory and port them before calling the work finished.

## 4. Validate and commit

- Build with `apps/layer-windows/scripts/build.ps1` (Rust and WinUI together),
  run the shared and Windows unit tests, strict Windows Clippy, and the native
  `exercise-*.ps1` fixtures for the affected areas.
- Guard performance: no new UI-thread work, blocking calls or per-frame
  allocations in pointer, brush or presentation paths. Repeat the baseline
  measurements on the ported build, isolated from compiles and other GPU work.
  A regression blocks the port.
- Commit and push to `origin/main` after each significant milestone (a complete
  feature area that builds and passes its checks), not after each small change.
  Rebase onto new upstream commits and rebuild before pushing.
- Finish only when every inventory item is ported or recorded as not applicable
  with a reason, and there is no performance regression.
- Record durable results (behavior, decisions, reproducible checks, measured
  timings) in a concise `docs/development/windows-*.md` note. Keep logs,
  captures and profiles in `artifacts/windows/`.
