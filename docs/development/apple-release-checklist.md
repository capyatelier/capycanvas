# Apple release checklist

Current closure list, 2026-09-16. The [goal](../history/apple-acceptance.md#goal)
still applies in full to **both** hosts. Historical checkpoint lists in the
[handoff](apple-handoff.md) are evidence references, not additional independent
test plans. The goal is incomplete.

## Acceptance rules

- Fix perceptible differences from Web/Android; preserve the intentional Mac OS
  menu bar and native Settings. Imperceptible rasterization differences do not
  justify extra code.
- Accept smooth drawing with rare measured misses at the current Mac 90 Hz and
  iPad 120 Hz targets. Mac 120 Hz remains deferred. Visible stalls, rejected input,
  corruption, unbounded work and reduced brush fidelity are not acceptable.
- Reuse passing evidence unless changed code or a reproduced failure invalidates
  it. Shared correctness checks, mounted native controls, full applications and
  physical input prove different things; combine their evidence explicitly.
- Implement in shared Rust or existing native adapters. Remove obsolete paths;
  avoid test-only product behavior and speculative input/rendering workarounds.
- Keep drawings, private traces, signing material and machine/account/device
  identifiers out of commits. Publish completed major milestones to main.

## Closed reported blockers

| Result | Evidence and boundary |
| --- | --- |
| Color wheel sizing, collapsed-column grip rotation and gray title-bar backgrounds | Published parity milestones; component and editor comparisons in the handoff. |
| iPad title-bar item removal without moving the window | Direct user touch confirmation on the Release app. Retained simulator top-edge failures do not reopen this physical case. |
| Basic Pencil and XP-Pen drawing, pressure and Undo/Redo | Direct user confirmation on both devices; iPad palm rejection and two-finger zoom/rotation also pass. This does not cover every sensor or interruption. |
| Unsaved background/return | Direct user confirmation on both devices, followed by another working stroke. Process termination is a different case. |
| GPU milliseconds in Diagnostics | Hardware timer/native checks and direct confirmation on both devices. This measures GPU work, not physical pen-to-screen latency. |
| Severe iPad prediction stall | Actual Pencil capture proves unresolved estimates kept the entire stroke in preview. The bounded-tail fix has regression/pixel/history checks and direct smooth-drawing confirmation with iPadOS prediction both off and on. |

The prior physical-drawing milestone is `3d00463`. This follow-up groups the
installed prediction Settings/manual-lookahead fixes, link error reporting and
local failed-Open acceptance. Direct XP-Pen confirmation remains pending.

## Remaining gates

Rows below group the remaining work; they are not counts of known defects.
Close each listed case with scoped evidence or an explicit user scope decision.
Do not replace a missing result with a catalog entry or build success.

| ID | Requirement and current limit | Next action / proof needed |
| --- | --- | --- |
| R1 | **Manual prediction and Settings.** iPad hides manual amount while native prediction is selected; unsupported Mac native prediction displays off/disabled. Manual 64 ms previously stopped at the next display refresh. Current engine and Mac Metal checks prove the selected lead, preview removal on pen-up and exact history for supplied pen/mouse input. Both Release builds pass. | Direct XP-Pen comparison of 0/64 ms in the installed Mac review app is pending. Preserve the existing iPad lag pass; investigate only a new reported regression. |
| R2 | **Complete native feature behavior.** The table below assigns all command groups and dynamic controls to specific remaining cases. Most implementation and many workflows already pass. | Reconcile each case against its existing test result, then group genuinely missing native interactions. Fix reproduced failures with focused local regressions. |
| R3 | **Physical input and drag contract.** Basic drawing/navigation passes. Supplied AppKit/UIKit callbacks and shared history/pixel checks cover many constraints and cancellations. | Verify supported tilt/rotation/hover/proximity, interrupted contacts and recovery, physical trackpad/button navigation, Pencil eyedropper, and representative tool handles. Finish the device-specific tile/row/grip cases in the [drag inventory](../ui/drag-inventory.md#apple-macos-and-ipados). Physical iPad shortcut checks require a hardware keyboard, which is currently absent. Retained injected-key failures are unresolved evidence, not a proven application cause. |
| R4 | **Documents, recovery and native windows.** Local save/reopen/PNG, native cold/warm URL delivery, painted process recovery on Mac/simulator and physical background/return have scoped passes. | Finish provider access/destination restoration, conflict/error/cancel/interruption and image-import delivery; interrupted recovery/background-task expiration; physical iPad multiple-scene isolation/restoration; Mac sleep/wake and window/display/surface transitions. Verify artwork, settings/workspace, continued input and independent history. Preserve the open limits in [Persistence](../../apps/layer-apple/PERSISTENCE.md). |
| R5 | **Perceptual visual parity.** Main editor/component comparisons and reported fixes exist. | Map the retained normal-size light/dark, preset and narrow/windowed captures to the final feature inventory; inspect missing visible states on both native hosts against Web. Correct straightforward visible mismatches. Do not rerun every pixel comparison for an unrelated engine fix. |
| R6 | **Sustained hardware performance and measurement.** Short workloads, ten-minute ink/watercolor/4K runs, thermal/memory records and physical smooth-drawing reports exist at different revisions. Rare measured misses alone do not fail acceptance. | Review the five-profile coverage and retained source revisions before scheduling missing sustained checks. Resolve unqualified memory/storage growth, idle/resume behavior, instrumentation overhead and physical input-to-display measurement. Use [Performance](../../apps/layer-apple/PERFORMANCE.md); its CPU/GPU/presentation proxies are not physical latency. Do not restart profiler experiments without a concrete unresolved requirement or visible regression. |
| R7 | **Integration and delivery.** Main was fetched before this follow-up and matched `3d00463`. This milestone groups prediction/Settings fixes, link error reporting and local document checks. Both final Releases build without warnings; review artwork is preserved. | Integrate later main changes and repeat only checks affected by subsequent fixes. Publish completed milestones and verify the remote revision. The final release and overall goal still require R1–R6. |

## Feature closure map

The source catalog is [command-coverage.json](../../apps/layer-apple/command-coverage.json).
The current shared-model enumeration has **63 commands in 15 groups**, **90 tool
choices**, **11 panels**, **five Settings pages** and **43 layer/filter property
scenarios** per Apple policy. There is no command-list drift from the retained GPU
inventory. Current panel/workspace schemas also match. Settings models differ
and require the newer evidence. These counts establish scope, not acceptance.

Fresh model evidence: `artifacts/apple-release-closure-v1/`. The fresh GPU
enumeration did not execute: the sandbox lacks a Metal adapter and both requests
for approval outside it timed out. Retained GPU results remain separately scoped.

| Catalog group | Existing evidence to retain | Remaining behavior, excluding shared R3–R6 checks |
| --- | --- | --- |
| Document transport | Local native save/open/export/cancel; painted recovery; OS URL delivery; native Mac invalid-Open preservation/retry and both-policy owner checks below | Provider/destination/interruption cases are owned by R4. |
| Drawing tools | All thirty painting/erasing preset selection/control checks; Mac mouse artwork/history; Blend/Liquify and shared pixel oracles | Reconcile additional catalog choices and edited brush dynamics with existing results; physical sensors belong to R3. |
| Selection, fill and shapes | Native menus/settings; Mac figure/gradient/fill artwork; Apple Metal region refinement; supplied UIKit shape/gradient pixel/history checks | Native refinement interactions and modifier behavior; physical/iPad canvas hit targets for freehand selection and shapes. |
| Object transforms | Numeric validation, linked/unlinked content/mask transforms; Mac mouse Move/edge scaling; UIKit callback corner/modifier checks | Remaining native corner/tablet handle interactions, iPad mask/group Move and interrupted transforms. |
| Hand and eyedropper | Native Hand/Fit; Mac visible/layer sampling including transparency; physical iPad two-finger navigation | Pencil sampling and physical trackpad/button navigation (R3). |
| Rulers | All three choices; Mac mouse creation/handle editing; constrained/free pixel/history and UIKit modifier callback checks | Physical constrained painting/handles and stationary modifier-preview behavior. |
| Artwork history | Exact pixel/history checks in each edit family and physical drawing Undo/Redo | Reconcile all remaining edit families in this table with history evidence; do not create a duplicate standalone matrix. |
| Layer operations | Mask/link/group artwork/history, scrolling/reorder, Layers configuration and previews; coordinated image decode/ownership | Native image-provider delivery, remaining hierarchy/interruption interactions and physical row continuation. |
| Pixel selection actions | Native Select All/Fill/Deselect/Invert; Mac assembled freehand cancellation and exact history | Native lasso/modifier cases shared with Selection above. |
| Workspace and Zen | Shared topology/history, native drawers/styles/configuration; complete local manager workflows; mounted floating size/preview checks | Retained tile/drawer/column presentations with real devices, cancellation, Zen and persisted layout across native window transitions. |
| Title-bar customization | Shared geometry/history/persistence, native mouse/pen fixtures and direct physical iPad item removal | Remaining actual window/overflow/state combinations and Pencil delivery. |
| Camera | Native Hand/Fit/flip; direct Navigator and camera checks; supplied scroll/pinch/rotate; physical iPad touch navigation | Physical indirect input and interruption (R3). |
| Preferences and shortcuts | Numeric/text/image-choice editing, Reset/Done/reopen, search, persistence, shortcut forms/conflicts | Reconcile every exposed preference; native text/menu traversal and hardware key combinations. UIKit compact-menu Command-Z and text Command-A remain unresolved. |
| Native windows | Mac/simulator independent windows; Mac last-window reopen and full-screen artwork/history | Physical scene/display/lifecycle checks (R4). UIKit exposes no programmatic full-screen toggle in the current SDK; this capability remains explicitly unresolved, not silently counted as parity. |
| Application information and links | Mac/simulator About and actual browser handoff; mounted editor rejection/retry check below | Settings-link rejection/retry UI acceptance (callback handling is implemented), alternate-handler OS delivery and physical iPad link delivery. |

Dynamic panel controls, all property kinds and their edit/reset/history/locked
states, workspace service actions, menus and Settings rows are part of R2 even
when they are not standalone commands. The coverage file maps their native
handlers and focused checks. Missing references, catalog changes or a failing
native case reopen the affected row only.

The release review reproduced a silent Help-link failure: the native callback
completed its shared request with an error, but Apple never presented that
error. `EditorView` now routes both URL failure branches through one completion
helper and the existing dismissible error presentation. The mounted editor
regression fails before the fix and passes afterward on both Apple policies,
covering Website and Source Code rejection, subsequent accepted handoff, completed
requests and unchanged drawing state. It supplies `OpenURLAction` results without
opening a browser or attaching Metal; it does not claim physical iPad delivery.
Both final Release builds pass without warnings in separate build directories;
the installed review apps remain on the prediction build awaiting its physical
check. Run from the repository root:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/application-links.swift
```

Evidence is in `artifacts/apple-release-closure-v1/`.

Settings also forwards its native links through `OpenURLAction`, displaying a
rejected handoff in the existing inline error style and clearing it after a
successful retry. Both final Release builds pass without warnings. A mounted
Settings fixture could not activate the SwiftUI links, so no rejection/retry UI
pass is claimed; the incomplete fixture was discarded. Existing real-browser
acceptance remains separately scoped. Evidence is `artifacts/apple-settings-links-v1/`.

The local invalid-file path is now qualified separately from provider failures.
The Mac native workflow passes error presentation after Discard approval,
unchanged unsaved artwork/name/layers, Undo/Redo, another unsaved prompt, Cancel
and successful valid-file retry. Both source files remain unchanged. The existing
Swift/Metal file fixture also passes failed-Open preservation, dirty state,
history and retry cancellation for both Apple policies. No production change
was needed. The Mac run has one retained main-thread responsiveness warning,
without an attributed application cause; it is not performance acceptance.
Both before/after artwork captures are reviewed, disposable files are removed
and test processes are stopped. Evidence is `artifacts/apple-file-failure-v1/`
(`mac-v3` and `owner-v2.log`). R4's cloud/provider, interruption and physical
UIKit cases remain open; do not repeat this passing local workflow for them.

## Execution order

1. Finish R1's physical confirmation while resolving R2/R5 evidence gaps locally.
2. Group the remaining native feature, input and document/window interactions.
   Keep keyboard-dependent checks explicit until hardware is available.
3. Review retained sustained results under the accepted performance standard;
   run only measurements still needed to answer R6.
4. Complete the final integration/publication gate. Keep this list current;
   historical checkpoint paragraphs must not become an expanding backlog.
