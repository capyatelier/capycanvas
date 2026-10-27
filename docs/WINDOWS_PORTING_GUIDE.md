# Windows porting guide

[Technical documentation](README.md) · [Windows development](development/windows.md)

Bring Windows through a recorded `origin/main` revision with missing behavior
ported, encountered parity gaps fixed and obsolete rollout code removed.
Follow [AGENTS.md](../AGENTS.md) and the [commit guide](COMMIT_GUIDE.md).

## 1. Inspect the commits

1. Fetch `origin/main` and find the latest Windows completion watermark with
   `git log origin/main --grep='^Ported Windows origin/main through '`. If none
   exists, inspect earlier Windows port messages for their upstream revision.
   Verify that coverage in the code and carry forward unfinished gaps.
2. Record the fetched target SHA. Inspect every commit after the watermark,
   including infrastructure, merge and integration changes. Read the diffs and
   conflict resolutions, trace shared Rust through the reference hosts into
   Windows, and record each commit's disposition.
3. Audit snapshots, actions, queries, host requests and platform gates. Windows
   reads JSON fields and sends action names as strings: removed or renamed
   contracts can fail silently. Check changed interfaces against their native
   consumers and list missing controls, inputs and settings. Check affected
   workspace defaults on every platform.
4. Use current shared behavior as the semantic authority and Web as the visual
   reference; GTK and Android help explain integration. Resolve disagreements
   from code and tests. Link superseded commits to the final behavior to port.
   For collapsed-column joins, follow the [native bridge integration](ui/stacked-columns.md#native-bridge-integration)
   entry points and acceptance checks; Windows already has an adapter for the
   shared GPU renderer.

## 2. Keep a working ledger

Keep one Markdown ledger in ignored `artifacts/windows/`. At its top, record the
completion watermark, fetched target, audited-through SHA, current milestone and
next action. Keep implementation and validation status separate:

| Record | Required information |
| --- | --- |
| Source commit | SHA, behavior or contract change, disposition, source evidence and linked gap IDs. |
| Gap | ID, source commits, expected behavior, missing behavior, shared/native owner, cleanup, required checks, fix commit and open/in-progress/closed status. |
| Check | Gap IDs, command or manual journey, target/theme/input, tested revision/build, pass/fail/blocked/unverified result and evidence path. |

Use dispositions such as needs work, already covered, superseded or not
applicable. Each needs evidence; not applicable needs a platform reason.
For already covered changes, verify that Windows exposes and consumes the
behavior. Planned checks and unavailable hardware are not passing evidence.

Update the ledger after inspection, implementation and validation, and before
switching milestones. Read it after resuming or context compaction. Record and
resolve additional parity gaps and visual glitches encountered along the way,
including older gaps; a port does not require an unrestricted historical audit.

## 3. Implement and clean up

- Keep business rules, validation, state, history, UI text and semantic layout
  in shared Rust. C++ and WinUI handle native presentation, widget allocation,
  focus, accessibility, input capture, timing, surfaces, lifecycle and OS services.
- Fix the class of shared defect in the appropriate Rust crate with a headless
  regression test that fails without the fix. Reuse existing helpers and
  harnesses; consolidate duplicated host policy.
- Delete superseded implementations, obsolete actions and snapshot handling,
  temporary rollout paths, unused helpers and stale tests in the same milestone.
  Remove unnecessary compatibility code; add no old-format readers unless asked.
  Preserve the replaced tests' useful assertions. Review the final diff for
  duplicate logic and dead code; explain growth in refactors.
- Size controls from shared metrics, not literal pixels: panel controls from the
  catalog's `panel_metrics` (`panelMetric`, `controlHeight`, `controlCorner`),
  toolbar tiles, gaps and icons from the `style` toolbar query, and dividers with
  `dividerLine`. A shared metric change then reaches Windows without a port;
  window chrome keeps its own sizes.
- Keep custom-color field spacing inside its animated page. Verify closed,
  open and closed-again Preferences allocations in both themes: the collapsed
  field takes no space, and Accent's 12-unit gap clips with the field.
- Enable platform gates with their native presentation and shared platform tests.
  Do not hide a feature to claim parity. Follow the
  [drag convention](ui/drag-and-reorder.md) for mouse, touch and pen, including
  [known gaps](ui/drag-and-reorder.md#known-gaps) encountered in affected paths.

## 4. Validate the gaps

- Choose required checks by [change type](development/testing.md#checks-by-change-type)
  and record them against each gap. Build with `apps/layer-windows/scripts/build.ps1`;
  run affected shared and Windows tests, strict Windows Clippy,
  `apps/layer-windows/scripts/test-without-gpu.ps1` and targeted `exercise-*.ps1`
  fixtures. Walk affected native journeys in light and dark themes with relevant inputs. See
  [Windows tests](development/windows.md#test) for commands and fixture rules.
- Compare with Web using [matched captures](development/windows.md#matched-editor-captures)
  and [visual acceptance](../tools/visual/README.md#comparing-images). Fix visible
  workspace and canvas differences. Menus, dialogs and settings may use native
  styling while preserving shared layout, sizing and behavior. Keep exact artwork,
  Undo/Redo and specified-color assertions.
- For changes affecting frames or latency, keep a comparable Release baseline
  build and measure affected paths before and after the port using
  [measurement rules](performance/measuring.md). Preserve build, revision and
  hardware provenance; separate upstream shifts from local regressions. Keep
  expensive work and blocking calls off UI and input threads, and review hot
  paths for allocations and copies. Record results in the affected tier tables.
- Fix reproducible port-caused regressions. Qualify [targets](PERFORMANCE_TARGETS.md)
  only with current reference-hardware measurements, not a no-regression comparison.
  [VMs](development/devices.md#windows) check builds and fixtures, not hardware
  performance or physical pen behavior. Record unavailable checks explicitly.
- Look beyond the existing fixtures. Compare with Web, and drive what the host
  owns: losing pointer capture or focus mid-gesture, minimize, maximize and snap,
  display scale and theme changes, IME, clipboard, drag-and-drop and file pickers.
- Reuse passing evidence while its code and build remain applicable. Rerun
  checks invalidated by your changes, conflicts or upstream changes to the same
  code; unrelated upstream commits don't invalidate them. Investigate failures
  with evidence and a specific hypothesis before retrying; reuse existing
  automation.
- Run `exercise-vector-layers.ps1 -Journey editing` for native vector arrow and
  Shift-arrow nudges, Alt-drag copying, rotation, clipboard, front/back arrangement
  and mask painting. Each authored edit must restore its exact drawing checkpoint
  and sampled artwork pixels with one Undo and one Redo. Run this scope in both
  themes through `test-desktop.ps1`; retain its per-edit history JSON and composed
  captures. The default and `-ToolsOnly` scopes retain their existing journeys.
- Run `exercise-liquify-pixels.ps1` for the focused Liquify seed, deformation,
  history, Adjust and Reset checks; it reuses an existing private desktop
  and otherwise creates one with a 900-second deadline. The idle-brush fixture's
  `-LiquifyOnly` scope selects the same checks. Add `-SessionsOnly` for live
  G-Pen and Hand round trips with exact committed pixels and history, retained
  Adjust amount, and Adjust/Reset Undo/Redo. Repeat in both themes;
  `-Theme sessions-light` selects the light session scope for fixture discovery.
  Split Tilt runs with
  `exercise-tilt.ps1 -Journey baseline|cold-controls|first-curve|deferred|deferred-prior-edit|response|overlays`
  under `test-desktop.ps1` with a 600-second deadline per journey and repeat each
  in both themes. Omitting `-Journey` retains the full suite; a focused result
  reports only its completed journeys.
- Use `exercise-workspace-layout.ps1 -Journey diagnostics-fps` under the private
  desktop runner to check Diagnostics independently of preset grouping and menu
  layout. It retains submitted-frame counts, cached pan, idle zero FPS, localized
  row identity, GPU recovery, resize and visibility/Undo assertions. The default
  `all` journey still runs the complete workspace suite; repeat either scope in
  light and dark themes.
- Run `exercise-manager.ps1 -PinsOnly` and `-Theme pins-light` for native pin
  toggles, rapid consecutive activations, unchanged workspace/layout and restart
  persistence. The result records the actual activation interval; it does not
  qualify a hardware performance target.
- Run `exercise-documents.ps1 -Journey gpu-unavailable` with a 900-second
  private-desktop deadline for saving after exhausted GPU recovery, queued
  contact cancellation, durable reopen, exact exported pixels and Cancel/Discard.
- Run `exercise-photo-memory.ps1` through `test-desktop.ps1` with a 600-second
  deadline to generate and open a streamed 12000 by 8000 PNG in a private profile.
  It checks the four colored quadrants and observes only the owned app and its
  `capy-photo-reader.exe` children. `results.json` records the decoder's actual
  source/codec admission arguments, observed process peaks and sampled combined
  private memory. Sampling can miss exit-time growth; these measurements do not
  qualify a hardware target or impose a whole-editor cap. Shared photo budgets
  bound codec admission and compressed source ownership. The generated PNG is
  removed after the run; smaller `-Width` and `-Height` values support smoke checks.
- Use `exercise-tool-bubbles.ps1 -Variant hdr-f16-dark` and `hdr-f16-light`
  for the focused HDR paper picker preview, live intensity and ramp pixels,
  and exact cancellation. Failure receipts retain the live picker state before
  Escape closes it; the full HDR journey remains available with `-HdrOnly`.

## 5. Close and commit

Keep working until every source commit is accounted for and every in-scope gap
has its implementation, cleanup and required checks complete.
Required failed or blocked checks keep the gap open: record the cause and next
action, and report unfinished verification. Documented unavailable hardware
checks need not block an otherwise passing implementation milestone, but remain
unverified and cannot support a claim of fully verified parity.

Commit complete feature milestones and push each as soon as its relevant checks
pass; `origin/main` changes faster than heavy suites run. Fetch and rebase before
pushing, inspect new upstream commits, update the target and ledger, and rerun
only the checks that conflicts or upstream changes to the same code affect, never
heavy validation because unrelated commits landed. When in doubt, push first,
then validate further and fix what you find in a separate commit.
Finish against the final fetched target SHA; later commits belong to the next
batch. Advance the watermark only through a contiguous range with all required
work closed, using `Ported Windows origin/main through <sha>` in the commit body.
An audit or partial milestone does not advance it. Follow the commit guide for
tests and evidence, update affected guides, and keep working reports local.
