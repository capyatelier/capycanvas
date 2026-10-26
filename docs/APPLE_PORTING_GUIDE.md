# Apple porting guide

[Technical documentation](README.md) · [Apple development](development/apple.md)

Bring macOS and iPadOS through a recorded `origin/main` revision with missing
behavior ported, encountered parity gaps fixed and obsolete rollout code removed.
Both targets share `apps/layer-apple/Shared` and `native/`; verify each separately.
Follow [AGENTS.md](../AGENTS.md) and the [commit guide](COMMIT_GUIDE.md).

## 1. Inspect the commits

1. Fetch `origin/main` and find the latest Apple completion watermark with
   `git log origin/main --grep='^Ported Apple origin/main through '`. If none
   exists, inspect earlier Apple port messages for their upstream revision.
   Verify that coverage in the code and carry forward unfinished gaps.
2. Record the fetched target SHA. Inspect every commit after the watermark,
   including infrastructure, merge and integration changes. Read the diffs and
   conflict resolutions, trace shared Rust through the reference hosts into both
   Apple clients, and record each commit's disposition.
3. Check shared snapshots, actions, queries, host requests and platform gates for
   changed contracts and missing consumers. Shared code alone does not establish
   native coverage. Check affected workspace defaults on every platform.
4. Use current shared behavior as the semantic authority and Web as the visual
   reference; GTK and Android help explain integration. Resolve disagreements
   from code and tests. Link superseded commits to the final behavior to port.
   For collapsed-column joins, follow the [native bridge integration](ui/stacked-columns.md#native-bridge-integration)
   entry points and acceptance checks; both Apple targets already have adapters
   for the shared GPU renderer.

## 2. Keep a working ledger

Keep one Markdown ledger in ignored `artifacts/apple/`. At its top, record the
completion watermark, fetched target, audited-through SHA, current milestone and
next action. Keep implementation and validation status separate:

| Record | Required information |
| --- | --- |
| Source commit | SHA, behavior or contract change, disposition, source evidence and linked gap IDs. |
| Gap | ID, source commits, expected behavior, missing behavior, shared/native owner, cleanup, required checks, fix commit and open/in-progress/closed status. |
| Check | Gap IDs, command or manual journey, target/theme/input, tested revision/build, pass/fail/blocked/unverified result and evidence path. |

Use dispositions such as needs work, already covered, superseded or not
applicable. Each needs evidence; not applicable needs a platform reason.
For already covered changes, verify that both hosts expose and consume the
behavior. Planned checks and unavailable hardware are not passing evidence.

Update the ledger after inspection, implementation and validation, and before
switching milestones. Read it after resuming or context compaction. Record and
resolve additional parity gaps and visual glitches encountered along the way,
including older gaps; a port does not require an unrestricted historical audit.

## 3. Implement and clean up

- Keep business rules, validation, state, history, UI text and semantic layout
  in shared Rust. Swift handles native presentation, widget allocation, focus,
  accessibility, input capture, timing, surfaces, lifecycle and OS services.
- Fix the class of shared defect in the appropriate Rust crate with a headless
  regression test that fails without the fix. Reuse existing helpers and
  harnesses; consolidate duplicated host policy.
- Delete superseded implementations, obsolete actions and snapshot handling,
  temporary rollout paths, unused helpers and stale tests in the same milestone.
  Remove unnecessary compatibility code; add no old-format readers unless asked.
  Preserve the replaced tests' useful assertions. Review the final diff for
  duplicate logic and dead code; explain growth in refactors.
- Take control sizes from shared metrics, not Swift literals: `store.panelMetrics`
  for panel controls and `store.barMetrics` (the Small tile `style`) for toolbars
  and the canvas bar, where icon actions stay tiles and worded controls are
  panel-height controls centred in their tile. Pass the metric to views that
  have no store.
- Match Web panel rows: choice labels may use two lines within the 45% label
  column, compact choices use bold values, and Tool Set captions align right
  except for color-picker rows.
- Enable `Platform::Mac` and `Platform::Ios` gates with their native presentation
  and shared platform tests. Do not hide a feature to claim parity. Follow the
  [drag convention](ui/drag-and-reorder.md) for mouse, touch and Pencil.
- When changing menus, settings, publications or layers, read the
  [Apple integration rules](development/apple.md#editor-integration). Preserve
  intentional OS differences and native keyboard and accessibility conventions.
- The iPad app supports iPadOS 17.5 and later. Gate SwiftUI and UIKit APIs newer
  than 17.5 with an `if #available` fallback that keeps the feature, as the
  [Apple guide](development/apple.md) describes for scrolling and sheets.

## 4. Validate the gaps

- Choose required checks by [change type](development/testing.md#checks-by-change-type)
  and record them against each gap. Run affected shared and Rust bridge tests,
  Swift fixtures, command coverage and targeted XCTest journeys. Build both
  targets; walk affected native journeys in light and dark themes with relevant
  inputs. See [Apple tests](development/apple.md#tests) for commands and limitations.
- Check macOS and iPadOS independently. Mounted fixtures, full-app journeys and
  physical input prove different things; simulator or injected input does not
  establish physical Pencil behavior. Record unavailable checks using the
  [device guide](development/devices.md#apple), without claiming they passed.
- Review matched Web captures using [visual acceptance](../tools/visual/README.md#comparing-images).
  Fix visible workspace and canvas differences. Menus, dialogs and settings may
  use native styling while preserving shared layout, sizing and behavior. Keep
  exact artwork, Undo/Redo and specified-color assertions.
- For changes affecting frames or latency, capture a comparable optimized
  baseline before editing and measure the affected paths after the port using
  [Apple performance](../apps/layer-apple/PERFORMANCE.md) and the
  [measurement rules](performance/measuring.md). Separate upstream shifts from
  local regressions; record results and limits in the affected tier tables.
  Fix reproducible port-caused regressions. Qualify [targets](PERFORMANCE_TARGETS.md)
  only with current reference-hardware measurements, not a no-regression comparison.
- Reuse passing evidence while its code and build remain applicable. Rerun
  checks invalidated by your changes, conflicts or upstream changes to the same
  code; unrelated upstream commits don't invalidate them. Investigate failures
  with evidence and a specific hypothesis before retrying; reuse existing
  automation.

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
work closed, using `Ported Apple origin/main through <sha>` in the commit body.
An audit or partial milestone does not advance it. Follow the commit guide for
tests and evidence, update affected guides, and keep working reports local.
