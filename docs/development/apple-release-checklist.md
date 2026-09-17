# Apple release checklist

Current closure list, 2026-09-16. The [goal](../history/apple-acceptance.md#goal)
still applies in full to **both** hosts. Historical checkpoint lists in the
[handoff](apple-handoff.md) are evidence references, not additional independent
test plans. The goal is incomplete.

Main now includes shared SDR color/photo work through `7d2511e5`. Its
[Apple integration handoff](color-management-m2-apple-handoff.md) is the immediate
implementation priority. Existing effect/gradient controls and native SDR
constructors now pass scoped local checks and both Release builds. New Drawing
options/presets/defaults, tagged paint entry, workspace palettes and document-space
wheel previews also pass shared/native-owner and Mac UI checks; see the
[workflow coverage](apple-handoff.md#sdr-creation-and-paint-workflows).
Retained photo Open/Place/Paste and missing-profile choices now pass local
shared/native-owner checks; see the [photo record](apple-handoff.md#retained-photo-open-place-and-paste).
Profile/depth editing, complete conversion previews, exact history, flattened
copies and Document Properties also pass local checks; see the
[document color record](apple-handoff.md#document-profile-and-bit-depth-workflows).
Source repair/rasterization and native ICC import now pass shared/native-owner
and Mac UI checks, with both final Release builds passing; see the
[source record](apple-handoff.md#retained-source-editing-and-icc-import).
Histogram and sampling now pass shared/native-owner and Mac control checks; see the
[inspection record](apple-handoff.md#histogram-and-sample-area-workflows).
Retained photo corrections and masks pass both-policy Metal and native Mac
control/save/reopen/re-edit checks; see the [correction record](apple-handoff.md#retained-photo-corrections-and-masks).
Profiled export/presets and the ICC library pass shared-snapshot, both-policy
owner and native Mac form/file checks; see the [export record](apple-handoff.md#profiled-export-and-icc-library).
Managed canvas/control integration now passes scoped shared and native Metal
checks; see the [display record](apple-handoff.md#managed-sdr-canvas-and-controls).
The existing recovery coordinator also preserves full SDR source, paint,
correction and mask data through a no-drawable flush and fresh-owner restore on
Mac Metal with both Apple policies. Both physical review apps are updated with
previous drawings preserved; see the [recovery record](apple-handoff.md#sdr-recovery-and-current-review-builds).
Grouped physical drawing, background/return and local save/reopen is pending.
A separate 9504×6336 synthetic-JPEG regression passes G-Pen photo preservation,
exact history/save/reopen and GPU destruction/replacement on Mac Metal with both
Apple policies. It does not qualify physical iPad or large-photo performance.
Next are remaining provider/background workflows and physical SDR acceptance. Earlier
passes remain scoped to their recorded sources; they do not qualify the entire
new feature scope. See the [foundation record](apple-handoff.md#native-sdr-foundation).

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
| R4 | **Documents, recovery and native windows.** Local save/reopen/PNG, native cold/warm URL delivery, painted process recovery on Mac/simulator and physical background/return have scoped passes. Eight local process-kill cases now prove complete old/new recovery publication, discard, cleanup and retry through the production file helpers on both Apple policies. iPad expiration ends its lease synchronously; a focused production-helper check proves callback ordering and exactly-once completion. | The user defers iCloud acceptance. Finish applicable native destination restoration, error/cancel/interruption and image-import delivery; actual physical lifecycle interruption and OS background-task expiration; remaining physical iPad scene restoration/lifecycle (independent drawings/history/input now pass); Mac sleep/wake and window/display/surface transitions. Verify artwork, settings/workspace, continued input and independent history. Preserve the open limits in [Persistence](../../apps/layer-apple/PERSISTENCE.md). |
| R5 | **Perceptual visual parity.** Main editor/component comparisons and reported fixes exist. Benchmark document replacement exposed stranded layer-preview readbacks; central invalidation now passes both-policy AppKit/Metal regression and a final Mac Release capture. | Map retained normal-size light/dark, preset and narrow/windowed captures to the final feature inventory; inspect missing visible states on both native hosts against Web. Correct straightforward visible mismatches. Do not rerun every pixel comparison for an unrelated engine fix. |
| R6 | **Sustained hardware performance and measurement.** SDR layered-4K completes ten minutes with 1.016% long intervals on Mac after upload accounting, and 0.576% on iPad after native tile content reuse. Artwork/previews, nominal thermals, prompt idle and absence of errors/rejected input/missing measured callbacks are verified. Cache and staging limits remain unchanged. A further display-batch correction removes terminal CPU waits, but current short heavy-watercolor runs still have 12.803%/28.347% long intervals on Mac/iPad. Earlier performance evidence remains separately scoped. | Prioritize the remaining heavy-watercolor GPU/display cost. Preserve passing layered-ink evidence and the accepted rare-miss standard. Finish other current SDR profiles and ProPhoto/U16 qualification, recorder-off resource/storage and idle/resume checks, instrumentation overhead and physical input-to-display measurement. The retained footprint curves do not justify a memory workaround. See [Performance](../../apps/layer-apple/PERFORMANCE.md#native-tile-reuse-and-final-display-batches--2026-09-16); CPU/GPU/presentation proxies are not physical latency. |
| R7 | **Integration and delivery.** Creation/tagged-paint `9e3d2567`, retained photo `d614f7b4` and document color `0290c196` milestones are published. Source repair/rasterization and ICC file import now reuse the same worker/comparison UI and shared source/history rules. Scoped Metal/shared/Swift checks pass. Physical UIKit workflows and SDR requalification remain open. Full iPad XCTest is blocked by its extra runner's free-profile app limit; preserve artist apps/drawings. Histogram/sampling uses shared workers and corrected Metal row copies; local checks pass. Evidence: `artifacts/apple-source-edit-v1/` and `artifacts/apple-histogram-v1/`. | Complete physical managed-display/SDR, provider and background acceptance; profiled export and ICC/preset storage now pass local worker/owner checks. Retained photo corrections/masks pass scoped local checks recorded above. The final goal still requires R2–R6 and the new feature scope. |

## Feature closure map

### Retained visual evidence

The current-source tool-action check closes the AppKit component's perceptual
review: six actions, four enabled/selected combinations, two themes and two
widths give 96 exactly matching native/Web bounds. All four normal-size pairs
pass. Eight newly captured images reproduce the retained decoded RGBA bytes
exactly; current shared colors, font size and both platforms' action metadata
also match. Raw zero-tolerance failures remain unchanged. The UIKit follow-up
now also passes all 96 bounds exactly and all four normal-size pairs against the
source-qualified Web captures, without a product change. Full-editor and physical
input coverage stay separate. Evidence is `artifacts/apple-visual-closure-v1/`
and `artifacts/apple-uikit-actions-v1/`.

| Retained result | Reuse and boundary |
| --- | --- |
| Paint editor, both themes, fitted and zoomed canvas, both physical hosts | Retain layout/canvas and footer evidence from `apple-editor-parity-v1/native-v5`; use the later component evidence for subsequently changed controls. |
| Color wheel matrix and accepted physical appearance | Reuse `apple-color-milestone-v1` for the retained geometry/input behavior. The SDR workflow milestone changes the raster gamut/cache key and adds color-entry/palette controls; its new pixel/cache and UI checks qualify those scoped changes. Managed display values/tags now pass scoped local checks; physical SDR appearance remains open. |
| Tool Set, Pen/Figure, both themes, 168/226-point widths | The captured Tool Set layout, tile content and workspace-panel source are unchanged since `be1f27d`. Reuse all eight accepted pairs in `apple-toolset-parity-v1`; later Tool Settings heading/title-bar changes are outside this component result. |
| Tool actions, both themes, 120/226-point widths | AppKit and UIKit/Web component captures each pass 96 exact bounds and four normal-size pairs; no new rendering adjustment is needed. |

Brush size presets now pass six mounted AppKit/Web pairs at 140/184/242 points
in both themes, covering the two/three/four-column layouts and selected value.
Six inactive Diagnostics pairs also pass after matching the centered 200×46
chart, dashed budget line and narrow-label truncation. These captures use actual
production components and identical row models; GPU is unavailable and samples
are empty in this earlier native fixture. The subsequent UIKit and populated-
trace checks below extend that result; full-editor content remains separate. The full Mac capture attempt hit the system iCloud dialog; its
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
These Diagnostics models remain empty/unavailable; this batch adds no timing
acceptance. Evidence is `artifacts/apple-uikit-panels-v1/`.

The populated Diagnostics follow-up now passes six AppKit and six UIKit/Web pairs
at the same widths and themes. Each owner renders 128 actual synthetic strokes;
its final 120 CPU samples and metric values drive the matching production Web
component. Charts, timing labels and narrow truncation pass normal-size review.
Mac GPU values are populated; simulator GPU timing reports unavailable. No product
change is needed. This is component visual acceptance, not sustained performance
or physical latency. Evidence is `artifacts/apple-active-stats-v1/`.

The next grouped visual milestone adds twelve full Sketch/Photo pairs, both
themes: AppKit at 1200×870 and 700×650, UIKit Simulator full-screen at 1376×1032.
Shared layout and fitted-camera geometry matches Web exactly. Normal-size review
passes after two shared title-bar fixes: the canonical overflow menu glyph and
the overlapping paint icon's alignment. Actual compositor captures include the
Metal canvas and use isolated managed workspaces. Native chrome and live clock
text remain host-owned. Both pre-integration Release builds pass without warnings.
Evidence is `artifacts/apple-preset-editor-v1/`.

This establishes the initial-document baseline at `9b7c4eb7` plus the two fixes;
the subsequently integrated SDR contracts need separate qualification. Remaining
visual cases include native iPad windowed/narrow layouts, transient states and
new color/photo UI. Keep physical-device limits explicit and reuse unaffected
evidence instead of repeating whole component matrices.

The SDR follow-up adds 36 UIKit form captures in both themes at 340×480 and
600×720 points: creation, numeric color, populated palettes, missing-profile
choice, empty ICC library, document profile/depth choices and properties. The
captures reproduced hidden picker labels; one native-picker wrapper now keeps
those labels visible on iPad, including the corresponding export choices.
Final captured layouts and horizontal text-field bounds pass, with both Release
builds clean. These are actual shared models without Metal; export previews,
open menus, scrolled content and physical interactions are outside this batch.
The New Drawing fixture deliberately uses invalid dimensions and an error state.
Evidence: `artifacts/apple-sdr-form-fit-v1/`. Full editor/window and managed SDR
appearance acceptance remain separate.

### Commands and controls

The source catalog is [command-coverage.json](../../apps/layer-apple/command-coverage.json).
The current SDR CPU enumeration has **72 commands in 18 groups**, **11 panels**,
**six Settings pages with 21 rows**, and **43 layer/filter property scenarios**
per Apple policy. Nine color/photo commands now have explicit workflow/test
references. Both policies retain their expected unavailable commands and accept
every initially enabled command at the shared-model boundary. The catalog also
records non-command creation/export options, tagged paint/palettes, corrections
and masks, color policies, display details and the ICC library.

The inventory generator had still treated tagged effect colors and gradient
stops as RGBA arrays. It now uses the shared typed values; 162 edit/Undo/Redo/Reset
routes pass per policy, and the existing verifier rejects all ten corrupted-
evidence probes per policy. No application runtime path changes. Evidence is
`artifacts/apple-sdr-inventory-v1/`.

The hardware follow-up now supplies actual rendered content for scale/rotate and
passes the complete catalog auditor on both policies: **93 resolved tool choices**,
28 setting IDs, 14 panel control types, six preference kinds and nine workspace
service commands. All 72 commands and 162 property edit/history/reset routes also
pass. The three added choices are Point/3×3/5×5 color sampling; the previous 90
tool entries remain present. These are shared-model routing results with hardware
content for transforms, not native-widget, visual or performance acceptance.
Evidence is `artifacts/apple-sdr-inventory-v1/`; the original no-GPU failure is
retained alongside the passing hardware run.

| Catalog group | Existing evidence to retain | Remaining behavior, excluding shared R3–R6 checks |
| --- | --- | --- |
| Document transport | Local native save/open/export/cancel; painted recovery; OS URL delivery; native Mac invalid-Open preservation/retry and both-policy owner checks below | Provider/destination/interruption cases are owned by R4. |
| Retained photo input | Shared/native-owner profile/depth/source retention through Open/Place/Paste, missing-profile choice, cancellation and stale adoption; 61 MP class synthetic-JPEG painting/history/save/reopen/GPU recovery on Mac Metal | Physical UIKit picker/clipboard delivery and large-photo execution; provider/lifecycle cases are shared with R4. |
| Document color and properties | Shared/native-owner profile/depth operations, complete comparisons, atomic adoption, exact history and flattened master preservation; native Mac forms | Physical UIKit property/color forms and SDR appearance; lifecycle cases remain under R4. |
| Retained source editing | Source-profile repair, ICC import, full-extent rasterization, exact history/save/reopen and native Mac controls | Physical UIKit source/ICC workflows and provider delivery. |
| Drawing tools | All 34 current presets mapped to retained native control passes: 30 painting/erasing and four Blend/Liquify; Mac artwork/history and catalog-wide numeric bridge edits | No unaccounted catalog brush/group or setting-dispatch route remains. Physical sensors, hover/proximity and interruption belong to R3; perceptual coverage belongs to R5. |
| Selection, fill and shapes | Native menus/settings; Mac figure/gradient/fill artwork; Mac native expansion/contraction, smoothing and gap-closing artwork/history; UIKit refinement controls and retention; AppKit shape modifier geometry/history; Apple Metal region refinement; supplied UIKit shape/gradient pixel/history checks | Physical/iPad canvas hit targets for freehand selection and shapes; applicable shape modifier delivery is tracked below and in R3. |
| Object transforms | Numeric validation, linked/unlinked content/mask transforms; Mac mouse Move, edge scaling and all four corner handles with artwork/history; UIKit callback corner/modifier checks | Physical tablet/Pencil handle delivery, iPad mask/group Move and interrupted transforms. |
| Hand, eyedropper and histogram | Native Hand/Fit; Mac visible/layer sampling including transparency; physical iPad two-finger navigation; full-resolution document-space histogram and Point/3×3/5×5 sampling pass Metal checks on both policies and native Mac controls | Pencil sampling and physical trackpad/button navigation (R3); physical UIKit histogram/sampling controls. |
| Rulers | All three choices; Mac mouse creation/handle editing; constrained/free pixel/history and UIKit modifier callback checks | Physical constrained painting/handles and stationary modifier-preview behavior. |
| Artwork history | Exact pixel/history checks in each edit family and physical drawing Undo/Redo | Reconcile all remaining edit families in this table with history evidence; do not create a duplicate standalone matrix. |
| Layer operations | Mask/link/group artwork/history, scrolling/reorder, Layers configuration and previews; coordinated image decode/ownership | Native image-provider delivery, remaining hierarchy/interruption interactions and physical row continuation. |
| Pixel selection actions | Native Select All/Fill/Deselect/Invert; Mac assembled freehand cancellation and exact history | Native lasso delivery shared with Selection above; there are no shared add/subtract selection modifiers. |
| Workspace and Zen | Shared topology/history, native drawers/styles/configuration; complete local manager workflows; mounted floating size/preview checks | Retained tile/drawer/column presentations with real devices, cancellation, Zen and persisted layout across native window transitions. |
| Title-bar customization | Shared geometry/history/persistence, native mouse/pen fixtures and direct physical iPad item removal; duplicate-handler removal passes mounted header secondary click, holds, dragging and history | Remaining actual window/overflow/state combinations and Pencil delivery. |
| Camera | Native Hand/Fit/flip; direct Navigator and camera checks; supplied scroll/pinch/rotate; physical iPad touch navigation | Physical indirect input and interruption (R3). |
| Preferences and shortcuts | All 16 pre-SDR Settings rows reconciled; every editable row passes native-owner edit, fresh-owner restore and exact durable Reset on both policies. The five added SDR policy rows have the scoped checks referenced in the color workflow milestones. Retained native numeric/text/image-choice editing, Reset/Done/reopen, search and shortcut forms/conflicts pass. Native theme/cursor choices and Done/reopen pass on both hosts; Mac prediction dependencies and amount editing also pass. | Physical iPad prediction dependencies pass by user confirmation; broader native text/menu traversal and hardware key combinations. UIKit compact-menu Command-Z and text Command-A remain unresolved. |
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

The drawing-tool reconciliation accounts for the retained 90 tool entries: 18 root
tools, 13 drawing groups, 34 brush presets and 25 other subtools. The current
93-entry hardware enumeration adds the three color-sample areas above. The brush
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
