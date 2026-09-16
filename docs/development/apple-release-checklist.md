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
| R1: Manual prediction and Settings implementation | The engine and native Metal checks cover selected lookahead, preview removal and exact history. Both Release builds pass. On 2026-09-16 the user confirms a clear lead at 64 ms versus 0 ms with the XP-Pen in the fixed Mac review app. Prediction Settings implementation is qualified; physical iPad switch interaction also passes. The later document-adoption prediction regression is tracked under R2. |
| iPad prediction after document opening/recovery | The generic preparation policy incorrectly selected the saved manual amount. Two shared assignments preserve the receiving platform/capability; focused engine/Metal regressions and direct fixed-iPad confirmation pass. Published in `23de9b5`. |
| Physical iPad prediction controls and independent windows | The user confirms dependent Settings controls, independent drawings/Undo history and continued input. Selecting an occupied workspace focuses its owning window. Prediction runtime after document adoption is separately tracked below. |
| iPad full-screen capability | On 2026-09-16 the user accepts the native iPadOS window control. Keep the unavailable in-app command disabled; no substitute app API is required. Physical scene/lifecycle checks remain under R4. |

Published follow-up `7672213` groups the installed prediction Settings/manual-
lookahead fixes, link error reporting and local failed-Open acceptance. Its remote
main revision is verified. Direct XP-Pen confirmation now passes.

The following input-cleanup/native-acceptance milestone groups the title-bar
duplicate-handler removal, unreachable workspace menu-path removal and Mac
refinement/corner-handle acceptance. Its evidence and publication verification
are under `artifacts/apple-context-cleanup-v1/`.

## Remaining gates

Rows below group the remaining work; they are not counts of known defects.
Close each listed case with scoped evidence or an explicit user scope decision.
Do not replace a missing result with a catalog entry or build success.

| ID | Requirement and current limit | Next action / proof needed |
| --- | --- | --- |
| R2 | **Complete native feature behavior.** The table below assigns all command groups and dynamic controls to specific remaining cases. Most implementation and many workflows already pass. The reported iPad native-prediction error after document adoption is reproduced and fixed in shared policy; the user confirms the fixed iPad now uses the correct native prediction. | Reconcile each case against its existing test result, then group genuinely missing native interactions. Fix reproduced failures with focused local regressions. |
| R3 | **Physical input and drag contract.** Basic drawing/navigation passes. Supplied AppKit/UIKit callbacks and shared history/pixel checks cover many constraints and cancellations. | Verify supported tilt/rotation/hover/proximity, interrupted contacts and recovery, physical trackpad/button navigation, Pencil eyedropper, and representative tool handles. Finish the device-specific tile/row/grip cases in the [drag inventory](../ui/drag-inventory.md#apple-macos-and-ipados). Physical iPad shortcut checks require a hardware keyboard, which is currently absent. Retained injected-key failures are unresolved evidence, not a proven application cause. |
| R4 | **Documents, recovery and native windows.** Local save/reopen/PNG, native cold/warm URL delivery, painted process recovery on Mac/simulator and physical background/return have scoped passes. iPad expiration now ends its lease synchronously; a focused production-helper check proves callback ordering and exactly-once completion, with a clean iPad Release build. | The user defers iCloud acceptance. Finish applicable native destination restoration, error/cancel/interruption and image-import delivery; interrupted recovery and actual OS background-task expiration; remaining physical iPad scene restoration/lifecycle (independent drawings/history/input now pass); Mac sleep/wake and window/display/surface transitions. Verify artwork, settings/workspace, continued input and independent history. Preserve the open limits in [Persistence](../../apps/layer-apple/PERSISTENCE.md). |
| R5 | **Perceptual visual parity.** Main editor/component comparisons and reported fixes exist. Benchmark document replacement exposed stranded layer-preview readbacks; central invalidation now passes both-policy AppKit/Metal regression and a final Mac Release capture. | Map retained normal-size light/dark, preset and narrow/windowed captures to the final feature inventory; inspect missing visible states on both native hosts against Web. Correct straightforward visible mismatches. Do not rerun every pixel comparison for an unrelated engine fix. |
| R6 | **Sustained hardware performance and measurement.** The missing-callback startup stall is reproduced and fixed by removing the custom presentation counter/retry. Both physical fixed Releases complete ten minutes of 4K watercolor with correct thumbnails, nominal thermals and prompt canvas idle. Long active intervals are 0.959% Mac / 1.997% iPad, with no missing/zero measured callbacks, rejected input or renderer errors. Current and retained ink results remain separately scoped. Rare measured misses alone do not fail acceptance. | Preserve the startup regression and these completed runs. Finish only outstanding source-qualified platform/profile, recorder-off resource/storage and idle/resume checks, instrumentation overhead and physical input-to-display measurement. The retained footprint curves do not justify a memory workaround. Use [Performance](../../apps/layer-apple/PERFORMANCE.md#startup-progress-without-presentation-callbacks--2026-09-16); its CPU/GPU/presentation proxies are not physical latency. Do not restart profiler experiments without a concrete unresolved requirement or visible regression. |
| R7 | **Integration and delivery.** The grouped panel-parity milestone aligns Filters and Diagnostics, adds UIKit component acceptance for three panels, and records the user's passing native-prediction confirmation. Both final Releases build without compiler warnings. Existing physical review apps and drawings remain intact; no installation or new physical-input pass is inferred. Evidence/publication record: `artifacts/apple-uikit-panels-v1/`. The preceding prediction-adoption and sustained-performance results remain separately scoped. | Integrate later main changes and repeat only checks affected by subsequent fixes. Publish completed milestones and verify the remote revision. The final release and overall goal still require R2–R6. |

## Feature closure map

### Retained visual evidence

The current-source tool-action check closes the AppKit component's perceptual
review: six actions, four enabled/selected combinations, two themes and two
widths give 96 exactly matching native/Web bounds. All four normal-size pairs
pass. Eight newly captured images reproduce the retained decoded RGBA bytes
exactly; current shared colors, font size and both platforms' action metadata
also match. Raw zero-tolerance failures remain unchanged. UIKit appearance and
full-editor coverage are separate. Evidence is
`artifacts/apple-visual-closure-v1/`.

| Retained result | Reuse and boundary |
| --- | --- |
| Paint editor, both themes, fitted and zoomed canvas, both physical hosts | Retain layout/canvas and footer evidence from `apple-editor-parity-v1/native-v5`; use the later component evidence for subsequently changed controls. |
| Color wheel matrix and accepted physical appearance | The Color panel, wheel drawing and both native input files are unchanged since `6c36d0a`. Later font-cache and Rust iteration changes preserve the exercised rendering paths. Reuse `apple-color-milestone-v1`. |
| Tool Set, Pen/Figure, both themes, 168/226-point widths | The captured Tool Set layout, tile content and workspace-panel source are unchanged since `be1f27d`. Reuse all eight accepted pairs in `apple-toolset-parity-v1`; later Tool Settings heading/title-bar changes are outside this component result. |
| Tool actions, both themes, 120/226-point widths | Current AppKit/Web recapture and normal-size acceptance above; no new rendering adjustment is needed. |

Brush size presets now pass six mounted AppKit/Web pairs at 140/184/242 points
in both themes, covering the two/three/four-column layouts and selected value.
Six inactive Diagnostics pairs also pass after matching the centered 200×46
chart, dashed budget line and narrow-label truncation. These captures use actual
production components and identical row models; GPU is unavailable and samples
are empty in the native fixture. Nonempty traces and full-editor content remain
separate; the subsequent UIKit component check below closes these three panel
appearances. The full Mac capture attempt hit the system iCloud dialog; its
unfinished helper is removed rather than counted as a pass. Evidence is
`artifacts/apple-panel-contents-v1/`.

Filters now passes sixteen AppKit/Web component pairs: both themes, 168/226-point
widths, All/Tone categories, Blur search and empty results, with actual GPU
previews. Small SwiftUI changes match spacing, label heights, search-button state
and empty-message alignment. Matching-size raw preview silhouettes are identical;
no renderer workaround is needed. The native search field retains the shared
input/text palette, with minor system-field shading and glyph rasterization
differences. Evidence is `artifacts/apple-filter-panel-parity-v1/`.

The UIKit follow-up now passes the same sixteen Filters cases plus six Brush
size and six inactive Diagnostics pairs, using real production components on
the existing iPad simulator. The capture exposed cumulative Diagnostics row-height
drift; one shared fixed line height aligns it with Web, with six final recaptures
on each Apple host. All final normal-size sheets are reviewed. These checks use
memory-only storage and disposable apps; artist review apps are untouched.
Diagnostics models remain empty/unavailable, so this adds no timing or nonempty-
trace acceptance. Evidence is `artifacts/apple-uikit-panels-v1/`.

The grouped panel-parity milestone combines these small visual fixes and records
the user's passing physical iPad native-prediction confirmation. Remaining visual
work includes UIKit Tool actions, nonempty Diagnostics traces, full Sketch/Photo,
narrow/windowed and transient-state coverage. Keep the full-editor and physical-
device limits explicit; component captures do not close those cases. Do not
repeat unchanged Color or Tool Set matrices while completing these missing states.

### Commands and controls

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
| Drawing tools | All 34 current presets mapped to retained native control passes: 30 painting/erasing and four Blend/Liquify; Mac artwork/history and catalog-wide numeric bridge edits | No unaccounted catalog brush/group or setting-dispatch route remains. Physical sensors, hover/proximity and interruption belong to R3; perceptual coverage belongs to R5. |
| Selection, fill and shapes | Native menus/settings; Mac figure/gradient/fill artwork; Mac native expansion/contraction, smoothing and gap-closing artwork/history; UIKit refinement controls and retention; AppKit shape modifier geometry/history; Apple Metal region refinement; supplied UIKit shape/gradient pixel/history checks | Physical/iPad canvas hit targets for freehand selection and shapes; applicable shape modifier delivery is tracked below and in R3. |
| Object transforms | Numeric validation, linked/unlinked content/mask transforms; Mac mouse Move, edge scaling and all four corner handles with artwork/history; UIKit callback corner/modifier checks | Physical tablet/Pencil handle delivery, iPad mask/group Move and interrupted transforms. |
| Hand and eyedropper | Native Hand/Fit; Mac visible/layer sampling including transparency; physical iPad two-finger navigation | Pencil sampling and physical trackpad/button navigation (R3). |
| Rulers | All three choices; Mac mouse creation/handle editing; constrained/free pixel/history and UIKit modifier callback checks | Physical constrained painting/handles and stationary modifier-preview behavior. |
| Artwork history | Exact pixel/history checks in each edit family and physical drawing Undo/Redo | Reconcile all remaining edit families in this table with history evidence; do not create a duplicate standalone matrix. |
| Layer operations | Mask/link/group artwork/history, scrolling/reorder, Layers configuration and previews; coordinated image decode/ownership | Native image-provider delivery, remaining hierarchy/interruption interactions and physical row continuation. |
| Pixel selection actions | Native Select All/Fill/Deselect/Invert; Mac assembled freehand cancellation and exact history | Native lasso delivery shared with Selection above; there are no shared add/subtract selection modifiers. |
| Workspace and Zen | Shared topology/history, native drawers/styles/configuration; complete local manager workflows; mounted floating size/preview checks | Retained tile/drawer/column presentations with real devices, cancellation, Zen and persisted layout across native window transitions. |
| Title-bar customization | Shared geometry/history/persistence, native mouse/pen fixtures and direct physical iPad item removal; duplicate-handler removal passes mounted header secondary click, holds, dragging and history | Remaining actual window/overflow/state combinations and Pencil delivery. |
| Camera | Native Hand/Fit/flip; direct Navigator and camera checks; supplied scroll/pinch/rotate; physical iPad touch navigation | Physical indirect input and interruption (R3). |
| Preferences and shortcuts | All 16 Settings rows reconciled; every editable row passes native-owner edit, fresh-owner restore and exact durable Reset on both policies. Retained native numeric/text/image-choice editing, Reset/Done/reopen, search and shortcut forms/conflicts pass. Native theme/cursor choices and Done/reopen pass on both hosts; Mac prediction dependencies and amount editing also pass. | Physical iPad prediction dependencies pass by user confirmation; broader native text/menu traversal and hardware key combinations. UIKit compact-menu Command-Z and text Command-A remain unresolved. |
| Native windows | Mac/simulator independent windows; Mac last-window reopen and full-screen artwork/history; native iPadOS full-screen control accepted by the user | Physical iPad independent drawing/history and continued input pass; remaining scene/display/lifecycle checks (R4). Keep the unsupported UIKit in-app toggle unavailable; the accepted native control satisfies the capability requirement. |
| Application information and links | Mac/simulator About and actual browser handoff; mounted editor rejection/retry check below | Settings-link rejection/retry UI acceptance (callback handling is implemented), alternate-handler OS delivery and physical iPad link delivery. |

The former generic selection-modifier gap was broader than the actual shared
feature set. Lasso and Auto Select replace the selection through `SetSelection`;
neither exposes add/subtract/intersect modifier behavior. Web and Android forward
canvas input to the same `UiSession` routes. This is a source-scope correction,
not removal of an exposed feature or a claim that physical input passes. Keep
applicable Shift/Option behavior for figures, rulers and transforms, and their
remaining native/physical delivery checks, in scope. Source references and hashes
are retained in `artifacts/apple-native-modifiers-v1/selection-scope.json`.

The drawing-tool reconciliation accounts for all 90 tool entries: 18 root tools,
13 drawing groups, 34 brush presets and 25 other subtools. The current brush
catalog exactly matches the retained GPU inventory, and every brush label maps
to one of the executed native painting or Blend/Liquify workflows. Recorded
XCTest case results confirm both workflows pass on Mac and iPad Simulator; the
Mac workflows also check actual mouse artwork and Undo/Redo. The iPad results
cover native controls, not physical Pencil strokes for every preset.

The retained Apple bridge suite dynamically edits every visible brush setting
and checks the resulting value on both policies. The unchanged catalog contains
317 brush-setting routes across 19 numeric IDs. Shared native `NumberControl`
receives each row and dispatches its exact setting ID; existing native numeric
and stale-draft checks remain applicable. This combines catalog/bridge and widget
evidence, rather than claiming 317 separate native-widget interactions. Physical
sensor and interruption checks remain under R3. Reconciliation evidence is in
`artifacts/apple-tool-coverage-review-v1/`; no repeated UI sweep was needed.

The Settings reconciliation covers five pages and sixteen rows: three choices,
two text fields, four numbers, two switches, three information rows and two links.
The existing native-owner persistence fixture now edits each enabled preference,
restores its value in a fresh owner and verifies an exact durable Reset without
altering unrelated settings. Eleven editable routes pass on iPad policy, including
manual prediction after disabling native prediction. Ten pass on Mac, with the
unavailable native-prediction row separately verified off and disabled. Existing
concurrent-owner and failed-save retry checks also pass. This is bridge/storage
evidence, not sixteen native-widget interactions. Retained XCTest results verify
the numeric, text and image-choice workflows on Mac and UIKit Simulator. The
follow-up native dropdown workflows and Mac prediction dependencies pass too.
The UIKit cursor menu needed native image labels; its former custom icon labels
prevented activation. The displayed selection now has an accessibility value.
Simulator taps still do not change the prediction switches, so that fixture is
removed; the user now confirms the physical dependency check passes. No app
workaround is added for that delivery failure. Evidence is `artifacts/apple-settings-dropdowns-v1/`;
Remaining keyboard and link acceptance stays explicit in the table.
Evidence is `artifacts/apple-preference-coverage-v1/`; no new app or simulator
launch accompanied this reconciliation.

Dynamic panel controls, all property kinds and their edit/reset/history/locked
states, workspace service actions, menus and Settings rows are part of R2 even
when they are not standalone commands. The coverage file maps their native
handlers and focused checks. Missing references, catalog changes or a failing
native case reopen the affected row only.

The Mac refinement workflow now passes through native fields, scrolling, menus,
canvas clicks, image import and PNG export. Fill and Auto select each produce
the expected full 64×64 image at zero, positive and negative expansion and at
full edge smoothing; only the four expected corner pixels soften. Closing a
two-pixel gap keeps Fill inside the outline, while zero gap closing leaks outside.
Undo restores every pixel in all ten cases, and Redo restores each tool's softened
result exactly. Representative captures are reviewed. The first fixture attempt
tried to edit Expansion below the scroll viewport; it now uses the existing
reveal helper. No production fix was needed. Mac `v2` passes one native workflow
with no failures or skips; retained runtime responsiveness/QoS warnings are not
attributed performance failures or performance acceptance. Evidence is
`artifacts/apple-native-refinement-v1/`. UIKit and physical pen delivery remain
separate; do not repeat this Mac case to stand in for them.

The UIKit refinement follow-up passes all fourteen native edits across Fill and
Auto select: gap closing, positive/negative expansion and smoothing. Each edit
survives switching to Brush and back, proving publication beyond the local field
draft. Both final control captures are reviewed, with no canvas error. The one
workflow passes without failures or skips. UIKit needs no artwork-color startup
seed; removing that unnecessary Mac setup avoids a debug startup ownership error
that was already present before any field edit. No production code changed.
Evidence is `artifacts/apple-ipad-refinement-v1/` (`simulator-v2` and
`verification.json`). This closes native refinement controls, not physical Pencil
artwork or hardware-key delivery.

The assembled AppKit editor now also passes figure modifier delivery through its
visible canvas: line, rectangle and ellipse with Shift held before contact,
pressed or released after movement stops, and a following unmodified contact.
Independent geometry samples distinguish 45-degree lines, squares and circles;
full decoded PNG Undo/Redo is exact in all twenty-four policy/case combinations.
The existing fixture finishes all forty-six groups with no failures or compiler
warnings, including its prior lasso, navigation, ruler and interruption checks.
No product fix or new simulator run was needed. Evidence is
`artifacts/apple-native-modifiers-v1/`. This establishes AppKit event delivery
and committed artwork, not physical tablet/keyboard delivery, UIKit key delivery
or the intermediate stationary preview's appearance.

The existing Mac transform workflow also passes with all four corner handles.
Real mouse drags produce the expected 75% width/height and position while keeping
the opposite corner fixed; independent artwork samples, Apply and one-step
Undo/Redo agree. All four captures are reviewed. The existing edge, rotation,
modifier and cancellation cases in that workflow also pass. Mac `v3` has one
passing workflow, no failures/skips and the same app executable as refinement
`v2`; only the test bundle changed. Both builds have zero compiler warnings.
Evidence is in the same artifact directory. This closes Mac mouse corner
acceptance, not physical tablet/Pencil or interrupted-contact acceptance.

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
(`mac-v3` and `owner-v2.log`). R4's remaining interruption and physical
UIKit cases stay open; iCloud acceptance is now deferred by the user; do not repeat this passing local workflow for them.

The first native iCloud run uses only generated drawings and images in a newly
created, subsequently removed provider folder. The image-import case passes its
native selection/cancellation, sampled artwork, selected filename, Undo/Redo,
unchanged source bytes and iCloud recognition/upload assertions. Capture review
shows the system access-consent dialog still covering the editor, so this is
scoped data/history evidence, not complete provider UI acceptance. Save/reopen
and invalid-Open workflows time out while the same visible system prompt awaits
permission; no application data defect is established. All test processes finish
and generated provider folders are removed.

Automatic approval review rejected the broad OS grant. The user subsequently
asks to ignore iCloud because it is unavailable in their environment. Defer
cloud-provider acceptance; do not request or grant permission or retry these
runs. The unused provider fixture additions and consent handler are removed.
Existing local-file acceptance remains valid. Evidence is
`artifacts/apple-provider-native-v1/` and the recorded user scope decision in
`artifacts/apple-prediction-adoption-v1/`.

The iPad prediction report exposed a shared document-adoption bug: a generic
prepared session applied settings before receiving the destination platform's
prediction capability. With native prediction selected and a saved 64 ms amount,
it produced 100 engine-predicted frames and no native-predicted frames. Preserving
the receiving platform/capability before settings application fixes both Open
and recovery. All 401 shared UI tests pass, including 64 regression combinations.
Actual Metal tests on both Apple policies verify preview pixels, prediction
source, pen-up removal and exact Undo/Redo. Both Releases build without warnings;
the fixed iPad app is installed with all eight recovery drawings preserved.
The user confirms native prediction now looks correct on the fixed iPad Release.
This closes the reported document-adoption regression.

## Execution order

1. Preserve R1's completed physical confirmation and resolve the remaining R2/R5 evidence gaps locally.
2. Group the remaining native feature, input and document/window interactions.
   Keep keyboard-dependent checks explicit until hardware is available.
3. Review retained sustained results under the accepted performance standard;
   run only measurements still needed to answer R6.
4. Complete the final integration/publication gate. Keep this list current;
   historical checkpoint paragraphs must not become an expanding backlog.
