# Windows porting guide

[Documentation](README.md) · [Windows development](development/windows.md)

How to bring the Windows app (`apps/layer-windows`) up to date with work that
landed first on GTK, Web and Android. The goal is parity with those hosts and no
performance regression.

## 1. Audit

1. Fetch `origin/main` and find the port watermark: the upstream commit named in
   the newest Windows port commit message (see [committing](#4-validate-and-commit)).
   `git log -i --grep='Ported origin/main through' --grep='Windows port through'`
   lists them; older ports used the second form.
   Confirm that the commits just before the watermark were really ported or are
   specific to another host.
2. Review every commit on `origin/main` after the watermark, including merge and
   integration commits: their conflict resolutions can change behavior or shared
   interfaces.
3. For each user-visible change, record:
   - the behavior, device rules (mouse, touch, pen) and layout sizes;
   - the source commits and the GTK, Web and Android code to mirror;
   - the shared Rust API the hosts consume;
   - the Windows status (missing, partial, handled by shared code, or not
     applicable), with evidence from the Windows source;
   - the Windows files and bridge functions to change, performance risks and
     tests to add.
4. Audit the shared interfaces as well as the features. WinUI code reads snapshot
   JSON by field name and sends action names as strings, so a renamed or removed
   shared field fails silently instead of breaking the build. Compare every field
   and action Windows uses with the serialized shared types, and list new panels,
   controls, inputs, host requests and settings that Windows does not consume yet.
5. Include older gaps: the [known drag gaps](ui/drag-and-reorder.md#known-gaps) and features
   present on GTK, Web or Android but missing on Windows.
6. Keep the inventory in ignored `artifacts/windows/`. It is a working report, not
   documentation.

## 2. Record a baseline

Before changing Windows code, build current `main` in Release and keep a copy of
the executable under `artifacts/windows/`. Record the unit test results, strict
Windows Clippy, and the Windows brush and pen latency workloads from
[measuring](performance/measuring.md). Upstream changes can shift these numbers
before any Windows work starts; the baseline separates those shifts from
regressions caused by the port. A regression against it blocks the port.

## 3. Port

- Follow [AGENTS.md](../AGENTS.md), including the
  [drag and reorder convention](ui/drag-and-reorder.md).
- Keep business rules, validation, state transitions and history in shared Rust.
  Windows code presents shared state and forwards native input; native timing and
  input capture stay in the host. Do not copy logic that another host already
  consumes from a shared crate.
- **Workspace and canvas** match the Web and GTK behavior and visuals: layout,
  docking, hit targets, gestures, canvas overlays, cursors and corner shapes.
  Check against the Web app with [matched editor captures](development/windows.md#matched-editor-captures).
  The bar is visual equivalence at normal viewing size. Do not add rendering
  dependencies, font workarounds or browser-specific pixel corrections only to
  satisfy a pixel comparator.
- **Menus, dialogs and settings** may use native WinUI styling, provided their
  layout, sizing and behavior match the other hosts.
- **Upstream keeps moving.** Re-fetch `origin/main` at least at every milestone
  and before every push, add new commits to the inventory, and port them before
  calling the work finished.

## 4. Validate and commit

- Build with `apps/layer-windows/scripts/build.ps1`, then run the shared and
  Windows unit tests, strict Windows Clippy and the `exercise-*.ps1` fixtures for
  the affected areas ([testing](development/windows.md#test)).
- Guard performance: no new UI-thread work, blocking calls or per-frame
  allocations in pointer, brush or presentation paths. Repeat the baseline
  measurements on the ported build, isolated from compiles and other GPU work,
  and compare them with the [performance targets](PERFORMANCE_TARGETS.md).
- Commit on top of `origin/main` following [the commit guide](COMMIT_GUIDE.md).
  Rebase onto new upstream commits, rebuild and rerun the affected checks before
  each push, and push to `origin/main` after each significant milestone: a
  complete feature area that builds and passes its checks.
- Name the watermark in the milestone's commit message with a body line
  `Ported origin/main through <sha>`, as Apple ports do, so the next audit starts
  there.
- Update the Windows guides the change affects. Results go in commit messages and
  performance numbers in the tier tables; logs, captures and profiles stay in
  `artifacts/windows/`.
- Finish only when every inventory item is ported or recorded as not applicable
  with a reason, and there is no performance regression.
