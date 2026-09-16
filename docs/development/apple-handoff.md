# Apple port handoff

For the next color-management work, start with the short
[phase 2 macOS/iPadOS handoff](color-management-m2-apple-handoff.md). The shared
SDR renderer, existing effect/gradient controls, New Drawing options and tagged
paint/palette workflows, retained photo Open/Place/Paste and document profile/depth
editing, retained-source editing, ICC file import, histogram/sampling and retained
photo corrections are integrated; the remaining host workflows and device
acceptance stay open. Earlier acceptance below
does not qualify all of those new contracts.

## Goal

Ship complete, visually consistent native iPadOS and macOS apps with fast,
readable workflows, shared maintainable code, and validated drawing performance.

The 2026-09-16 user confirmations close the XP-Pen manual-prediction check:
64 ms visibly leads farther than 0 ms in the fixed Mac review app. The user also
accepts the native iPadOS window control for full-screen parity. Preserve the
unsupported in-app command's unavailable state. Older pending references below
are historical; neither item should reopen without a new regression. Physical
iPad switch interaction and independent-window drawing/history now also pass by
direct user confirmation. Selecting an occupied workspace brings its owning
window forward; it does not change both layouts. Remaining scene/lifecycle cases
stay scoped in the release checklist.

Full acceptance remains in [the Apple goal tracker](../history/apple-acceptance.md#goal):
all exposed features, menus and actions; shared main-editor geometry and canvas
behind the header; iPad 120 Hz and current Mac 90 Hz performance targets. Mac
120 Hz testing is deferred. The overall goal is **incomplete**.

Use the [current release checklist](apple-release-checklist.md) for the remaining
gates and feature closure map. The older milestone sections below retain their
original evidence and limits; their remaining-work paragraphs are not separate
new test plans. The pre-SDR shared-model enumeration confirms its 63-command
scope with no command-list drift; the new color/photo workflows extend that scope. The input-cleanup/native-acceptance milestone
groups the context-handler removals and Mac refinement/transform checks below;
its evidence and publication verification are under
`artifacts/apple-context-cleanup-v1/`. The preceding milestone `7672213` groups
prediction/Settings fixes, link error reporting and local document failure/retry
acceptance; the remote main revision was verified. Its predecessor is `3d00463`.

The closure audit also reproduces and fixes silently rejected Help links. A small
shared SwiftUI completion helper surfaces the error through the existing error
presentation. The mounted editor fails before the fix and passes afterward for
both Apple policies: Website/Source Code rejection, retry success, completed
requests and unchanged drawing state. This supplies native OpenURLAction results,
without opening a browser or using Metal. It does not establish physical iPad
browser delivery. The focused check and model audit are retained under
`artifacts/apple-release-closure-v1/`. The Mac review retains the installed
prediction build used for the now-passing XP-Pen check; the iPad review is now on
the startup-progress fix after the hardware checks below. Both final Release builds
pass without compiler warnings in separate build directories. The closure work
is grouped with the Settings/prediction follow-up.

The next closure check passes local file failure/retry without a product change.
The native Mac File menu, panels and sheets preserve the unsaved drawing, title,
layers and sampled pixels when Open fails after Discard approval. Undo/Redo,
another unsaved prompt, Cancel, successful valid-file retry and unchanged source
files also pass. The existing Swift/Metal owner fixture adds matching unsaved
failure/history/retry checks on both Apple policies. Evidence is
`artifacts/apple-file-failure-v1/` (`mac-v3`, `owner-v2.log`); the initial fixture
failures targeted a hidden duplicate menu item, treated Mac sheets as XCTest
alerts, or retained the old location-choice count. No runtime workaround was
added. The final test build has no compiler warnings; its single main-thread
responsiveness warning remains unattributed and does not establish a performance
failure or pass. Captures are reviewed, generated files are removed and test
processes are stopped. The same running prediction review was brought forward
without restarting it; the iPad was untouched. Physical UIKit picker, provider
and interruption acceptance remain separate.

Settings link handoffs now also report rejection inline and clear that message
after a successful retry, while preserving native `Link` controls. Both final
Release builds pass without warnings. The mounted Settings activation fixture
was discarded after it failed to reach the links; direct rejection/retry UI
acceptance remains open. This is a test limitation, not a new browser-delivery
defect. Evidence is `artifacts/apple-settings-links-v1/`. The installed prediction
review apps are retained with the user's drawings; the XP-Pen check now passes.

## Prediction policy during document adoption

The user reports iPadOS prediction using the saved manual 64 ms amount after
opening a drawing. The shared regression reproduces this: the prepared document
is a generic session, so applying settings there disables native prediction
before its engine enters the iPad window. Two assignments preserve the receiving
window's platform and live capability before applying settings. No predictor,
input adapter or scheduling workaround is added.

The new regression covers 64 open/recovery, host, capability and saved-amount
combinations; all 401 shared UI tests pass. The actual Apple/Metal bridge also
passes native versus manual source selection, rendered preview pixels, pen-up
removal and exact Undo/Redo on both Apple policies. Both Release builds pass
without warnings. The fixed iPad Release is installed in place with all eight
recovery drawings preserved and saved files byte-identical across installation.
The Mac review remains untouched. Evidence is
`artifacts/apple-prediction-adoption-v1/`. The user confirms native prediction
now looks correct in the updated iPad app; this reported regression is closed.

## Native control and modifier closure

UIKit Fill/Auto Select refinement controls now pass fourteen native edits with
retention after tool switching; both final captures are reviewed. An unnecessary
Mac artwork seed was removed from the UIKit controls-only fixture after it
encountered debug startup workspace protection. The AppKit editor also passes
line/rectangle/ellipse Shift delivery, independent constraint geometry and exact
PNG history in twenty-four new cases across both Apple policies. Its existing
native fixture completes all forty-six groups. These are test-only changes;
physical input and provider/lifecycle gates remain explicit in the release list.
Evidence is `artifacts/apple-ipad-refinement-v1/` and
`artifacts/apple-native-modifiers-v1/`. Main was fetched at base `53c560a`; these supporting checks are grouped with
the prediction-adoption and panel-parity milestone.

The generic selection-modifier gap was corrected against current shared source:
Lasso and Auto Select replace selection on Web, Android and Apple. No exposed
add/subtract/intersect modifier feature was removed. Applicable figure, ruler
and transform modifiers remain in scope.

## Visual evidence reuse

Current-source AppKit/Web tool-action captures now pass normal-size perceptual
review in both themes at 120 and 226 points. All 96 bounds match exactly and all
eight decoded images reproduce the retained captures. Current shared palettes,
font size and action metadata match the fixtures; raw comparison failures remain
reported. Color and Tool Set source checks preserve their earlier scoped passes.
The release checklist records the coverage boundaries and next missing states.

The component capture initially exposed an unnecessary source dependency:
`ColorSwatch.swift` also contained the editor-store-dependent `BrushColorButton`.
That unchanged button now lives in `PanelControls.swift`, restoring independent
component compilation without a stub or new runtime path. Both complete host
source sets typecheck without warnings. No simulator or artist review app was
launched. Evidence is `artifacts/apple-visual-closure-v1/`; these supporting
changes are grouped with the prediction-adoption milestone.

The next component review accepts six Brush size and six inactive Diagnostics
AppKit/Web pairs in both themes at 140/184/242 points. Diagnostics now matches
the centered chart proportions, dashed budget rule and narrow-label truncation.
This does not qualify nonempty traces, UIKit appearance or full-editor state.
The unfinished full-editor capture fixture hit the system iCloud prompt and is
removed; its logs and captures remain private. Evidence is
`artifacts/apple-panel-contents-v1/`.

## Panel spacing and UIKit appearance

The AppKit/Web Filters comparison covers both themes, 168/226-point widths,
All/Tone categories, Blur search and empty results. Small shared SwiftUI changes
remove the extra list gaps and search-button selection background, align label
and category heights, outline the search field and align the empty message.
Panel measurements follow the new spacing; no renderer or prediction path changes.
Matching-size raw preview silhouettes are identical, so no GPU workaround is
needed. Evidence is `artifacts/apple-filter-panel-parity-v1/`.

The UIKit follow-up passes all sixteen Filters pairs plus six Brush size and six
inactive Diagnostics pairs. It uses production components, real filter GPU
previews and the existing simulator, with memory-only storage and disposable
capture apps. Diagnostics has a small accumulating line-height mismatch; one
shared fixed row height corrects it, with six final captures on each Apple host.
All final sheets pass normal-size review. The native search field retains minor
palette/clipping differences; no per-platform rendering workaround is added.
Evidence is `artifacts/apple-uikit-panels-v1/`. The initial standalone build's
newer deployment-default warning is corrected in the fixture before launch.
Artist review apps remain untouched. Full-editor and physical-device appearances
remain explicit release cases; the following checks close the remaining Tool
actions and Diagnostics component states.

These changes and the physical prediction confirmation form the grouped
panel-parity milestone. Both final Release builds pass without compiler warnings.
Publication verification is retained with the UIKit evidence; no individual-task
commit is made. Installed artist review apps retain the preceding prediction fix.

## Tool actions and populated Diagnostics

UIKit Tool actions now passes all 96 native/Web bounds exactly and all four
normal-size light/dark pairs at 120/226 points, reusing source-qualified Web
captures. The fixture compiles only the production control dependencies and uses
a disposable simulator app. No product change is required. Evidence is
`artifacts/apple-uikit-actions-v1/`.

Populated Diagnostics also passes six pairs on each native host at 140/184/242
points in both themes. Each real owner renders 128 synthetic G-Pen strokes;
its final 120 CPU samples and metric rows drive the production Web comparison.
Mac GPU values are populated; simulator GPU timing is unavailable. All charts,
narrow labels and values pass normal-size review without another code change.
These short offscreen debug captures establish visual behavior, not performance
or physical latency. Evidence is `artifacts/apple-active-stats-v1/`. Both artist
review apps remain untouched. These records join the following grouped visual
milestone; remaining window/transient and physical-input coverage stays open.

## Preset editor baseline and shared SDR integration

Full Sketch/Photo comparisons now pass in both themes: eight AppKit pairs at
1200×870 and 700×650, plus four full-screen UIKit simulator pairs at 1376×1032.
All shared layout and fitted-camera geometry matches Web. Real compositor
captures include the Metal canvas; each host measures its own controls. Two
shared SwiftUI lines align the overflow glyph and center the overlapping paint
swatches. Normal-size review accepts remaining native chrome, live clock text
and rasterization differences. Both pre-integration Release builds pass without
warnings. Evidence is `artifacts/apple-preset-editor-v1/`; artist review apps and
drawings remain untouched. iPad windowed and transient states remain open.

These captures qualify the `9b7c4eb7` baseline plus the two appearance fixes.
An occasional fetch then brought in 118 commits through `7d2511e5`, including
the shared SDR color/photo work. Main fast-forwards without conflicts and keeps
the pending visual changes. The integrated Apple Rust bridge compiles without
warnings, but compilation does not qualify the new color/renderer contracts.
Next, follow the phase 2 handoff above: repair existing tagged effect/gradient
controls, adopt the native SDR renderer, then complete the new host workflows.
Do not install this unqualified integration over the physical review drawings or
count earlier visual/performance results as acceptance of the new contracts.

## Native SDR foundation

Apple now constructs the native SDR renderer for initial attachment, prepared
Open/New/recovery documents and replacement devices. It requests supported
Float32 and native publication capabilities, retaining the shared portable
fallback. Prepared documents start deferred compilation before waiting for
readiness; the existing atomic adoption and prediction policy remain intact.

Effect and gradient color editors use the shared tagged form instead of the
obsolete array sliders. Unchanged and alpha-only edits preserve exact RGB values
and their space; explicit RGB edits use the document space. Shared conversion
supplies sRGB swatches and document-space gradient interpolation. The title-bar
paint swatches also consume the tagged values correctly.

All 55 Apple bridge checks pass across the full run and the corrected focused
follow-up. Shared UI/host checks pass 456 tests with one existing hardware-only
case ignored. Metal coverage includes P3/U8 and ProPhoto/U16 painting, exact integer
backing through Undo/Redo, save/open, recovery and GPU replacement. The native
Swift/Metal file-owner fixture passes both Apple policies. Sixteen real AppKit
color-entry cases pass, as do gradient position/opacity, one-step history,
cancellation and retired-draft checks on both policies. The broader slider run
could not locate an unrelated brush-opacity field; that result remains recorded
and is not a pass. Both final Release builds pass without compiler warnings.
Evidence and initial failed attempts are under `artifacts/apple-sdr-controls-v1/`.

The export fixture failure was early input, not lost pixels: it sent a stroke
before the prepared renderer reconciled the receiving brush. It now waits for
readiness and requires real ink before checking snapshot isolation. The new
color-history fixture also now accounts for monotonic document revisions while
still comparing every retained sample. Neither required a product workaround.

This checkpoint does not qualify new document/color/photo interfaces, wide-gamut
or 16-bit export, managed display changes, UIKit form delivery, modal dismissal,
physical SDR workflows or sustained performance. Continue with steps 3–4 of the
phase 2 handoff. Both installed review apps and the user's drawings are preserved;
the new Release products have not been installed over them. Main was fetched
without incoming changes before this grouped milestone.

## SDR creation and paint workflows

New Drawing now carries the complete shared creation options through the native
worker: presets, dimensions, background and independent RGB-space/bit-depth
choices. Optional preset/default changes use shared validation; conflicts stay
in the form for correction. Captured defaults are stable while a worker runs.
The Color panel, brush controls and brush popup expose the shared tagged editor
and workspace palettes. Obsolete inline RGB fields are removed. Wheel fields,
hue guides and image-cache keys now include the document's RGB space.

All 56 Apple bridge checks pass. Local Metal checks cover all 32 creation combinations across both Apple policies,
captured defaults and preservation after invalid input. The Swift/Metal owner
workflow passes preset conflict/retry, cancellation, all four tagged palette
spaces, document replacement and exact fresh-owner persistence. Shared UI/host
checks pass 456 tests with one existing hardware-only case ignored. Converted
wheel pixels and cache invalidation also pass. Evidence and failed local control
fixture attempts are retained under `artifacts/apple-sdr-workflows-v1/`.

The full Mac XCTest workflow passes native presets, dimensions, background,
depth, saved defaults, tagged paint alpha and palette storage/use/reopening.
Reviewed captures show readable forms. Its initial failure reproduced a modal
shortcut conflict: the editor retained Command-A while New Drawing was open.
The existing menu now releases its shortcuts during document dialogs; the fixed
workflow passes. Both final Release builds pass without compiler warnings.

The local control fixture could read native fields but omitted SwiftUI buttons
from its in-process accessibility traversal; it is replaced by native XCTest.
Full-app simulator checks stop before the forms because its Metal adapter lacks
the Float32 filtering required by the shared SDR renderer. The original GPU
error was captured directly; longer startup waits did not solve it. No reduced
precision renderer or simulator-specific application path is added.

The physical M4 iPad then reproduced a separate startup failure. The captured
original error rejects the SDR composition pipeline because wgpu's iPad
`Rgba32Float` format omits blending, despite advertising `FLOAT32_BLENDABLE`.
Its Float32 format filtering also used a macOS-only condition. The narrow fix
in the already-vendored Metal capability table includes blending and uses the
existing device filtering query for R32/RG32/RGBA32 Float formats. Apple's
[capability tables](https://developer.apple.com/metal/capabilities/) support both
changes. Texture precision and shared rendering/publication paths are unchanged;
see [vendor provenance](../../vendor/README.md).

After the fix, the same physical device opens the native New Drawing form with
P3/U16/transparent defaults; its capture is reviewed. The isolated Debug app also
completes the existing 2K predicted-ink workload (ten-second warmup, five-second
measurement), with visible ink, Navigator and layer thumbnails, no frame errors,
no rejected input and no recorded workload failure. This is a startup/painting
smoke check, not sustained performance or physical Pencil acceptance. The final
Mac Metal bridge suite still passes all 56 checks, and both final Release builds
pass without warnings.

The physical XCTest workflow cannot install its extra runner because the device
already has the three apps allowed by its free development profile. No app is
removed to make room. Full UIKit form/palette interaction remains unqualified;
the reviewed form and shared-owner results do not close that gap. Both artist
review binaries and drawings are preserved; only the separate performance-test
app is updated. Main was fetched without incoming changes for this grouped
milestone.

Profile/depth changes, retained photo import/editing, inspection, ICC/export
workflows, managed displays and physical SDR acceptance remain in phase 2 scope.

## Retained photo Open, Place and Paste

Apple now uses the shared PNG/JPEG/TIFF decoder inside the existing document
worker. Open retains source depth/profile, chooses the shared working space and
keeps Save separate from the source photograph. Place/Paste preserve the receiving
document's color and retain the original samples in one undoable layer. The
shared missing-profile and edit-depth settings are enabled on both hosts; Ask
pauses the same job for a native interpretation form, including Cancel and retry.
The old ImageIO-to-sRGB8 decoder, import ABI and separate layer picker are removed.

Mac Metal checks preserve P3/U8 and ProPhoto/U16 samples through painting,
Undo/Redo, native save/reopen and GPU recovery. They reject cancellation and
stale document, target or device results. The Swift coordinator passes both
Apple policies: coordinated read, picker cancellation, encoded Paste, failure
preservation, safe Save/reopen and missing-profile cancellation/retry. Shared
codec, host and UI checks pass 517 cases in aggregate, with five pre-existing
ignored cases. The Apple suite passes 58 cases in aggregate. Two obsolete test
assertions were updated and rerun separately: retained sources are no longer
packed image assets, and Apple now supports Color preferences.

A tiny TIFF also reproduced a shared macOS admission-budget failure: sysinfo's
available-memory calculation subtracted compressed pages twice and returned
zero on this machine. The macOS branch uses its total-minus-used readings;
iOS retains its process allowance and other hosts retain their existing queries.
The budget fractions and zero-exhaustion policy are unchanged. Both Release
builds pass without compiler warnings. The existing Swift document workflow
also passes on both Apple policies, including failed Open after Discard,
unsaved-history preservation, cancellation and retry. The Mac native picker
workflow passes selection/cancellation, rendered pixels, layer naming, exact
Undo/Redo and unchanged source bytes. Captures are reviewed; an unrelated iCloud
access prompt covers one corner, with no permission changed. This is local-file
acceptance only. Evidence, original failures and scoped validation are under
`artifacts/apple-photo-workflows-v1/`.

Native clipboard-provider delivery, physical iPad photo workflows, the 61 MP
journey, source repair/rasterization, profile/depth editing, inspection, ICC/export
controls, managed displays and final device/performance acceptance remain open.
Both artist review apps and drawings remain preserved. Main was fetched without
incoming changes during this milestone.

## Document profile and bit-depth workflows

Assign Profile, Convert Color Space, Change Bit Depth and Document Properties
now use the same native document worker. Shared code owns conversion semantics
and exact history. Preparation captures a stable project, computes complete
before/after previews and builds the replacement renderer off the owner; adoption
publishes the document and renderer together. Cancel and stale document/device
results preserve the active drawing. Retained original photo samples are unchanged.
Renderer replacement also retires the old layer/filter preview readbacks.

The native forms distinguish assignment from conversion, expose integer depth
and dither, and require a prepared comparison before Apply. Conversion can save
a flattened native copy while retaining the editable original and its history.
The copy cannot replace its current master. AppKit uses the normal Save panel;
iPad chooses a folder and filename, rejecting an existing destination. Properties
formats shared document/source metadata on the worker, including profile parsing.

All 60 Apple bridge tests and 456 active shared UI/host tests pass, with one
pre-existing shared case ignored. The Metal fixture covers painted P3/U16 data,
assignment/conversion/depth, exact document samples and rendered pixels through
Undo/Redo, retained sources, save/reopen, continued painting, copies and rejected
adoption. Both Apple policies pass the Swift owner workflow, including failure
and retry, cancellation, source/existing-file protection and usable thumbnails
after history. The Mac native UI workflow passes Preview, Cancel, Apply, exact
sampled-pixel Undo/Redo, depth/Properties and conversion through the real controls
(one test, no failures or skips). The three dialog captures are reviewed. An
unrelated iCloud prompt still covers a corner; no permission is changed. These
checks do not qualify managed canvas/display appearance. Both final Release
builds pass without compiler warnings, including an explicit explanation that
editable conversion can change blending and effects.

Initial native paint fixtures started input before deferred brush preparation;
initial Swift/UI fixtures filled without a selection, and a corrected UI startup
fill still left its captured document blank. The tests now prepare the brush or
select the canvas explicitly; UI artwork is created through the ready native
menus and verified before testing color changes. Original failures are retained,
and no runtime workaround was added. Evidence is `artifacts/apple-document-color-v1/`.
Physical UIKit forms and folder/provider delivery remain unqualified. Artist
review binaries and drawings are preserved; the new Release products are not
installed over them. Main was fetched without incoming changes.

Next are source repair/rasterization, histogram/sampling, retained photo
corrections/masks, ICC library and profiled export, managed displays and the
remaining device/performance gates. This checkpoint does not close the overall
Apple goal or qualify the 61 MP workflow.

## Retained-source editing and ICC import

Repair Source Profile and Rasterize Source now use the shared source-edit rules
on both Apple policies. Untouched source repair keeps exact sample tiles and
changes interpretation. A repaired photo with committed pixel edits receives a
separate corrected-original layer; the existing paint, masks and adjustments
stay intact. Rasterization converts the complete retained extent, including
off-canvas pixels, to document space/depth while preserving layer edits and masks.
Both operations use complete Before/After, Cancel and one-step Undo/Redo.

The existing color controller, form, worker and preview packing also serve
source edits. Source conversion and GPU comparisons run off the render owner;
shared candidate validation/publication stay on it. Jobs reject cancelled or
stale document/device results. Native ICC file import is shared by repair and
missing-profile interpretation. Coordinated reads are bounded to the shared
16 MiB limit; shared CMM validation runs on the worker and retains imported
bytes exactly. Current-source metadata does not copy embedded ICC payloads into
the UI, and selection/import notifications avoid serializing imported profiles
during SwiftUI layout. Saved profile-library management remains a later workflow.

Both complete Swift host source sets typecheck. All 63 Apple bridge tests and
456 active shared UI/host tests pass, with one pre-existing ignored case. The focused Metal source cases
pass full-extent U16 retention, original sample identity, masks, painted source
repair, exact artwork/history, rasterization/save/reopen, continued painting,
invalid ICC data and stale/cancel rejection on both Apple policies. The Swift
source owner workflow and existing document-color owner workflow pass on both
policies. The final native Mac workflow passes the actual ICC picker and its
cancellation, comparisons, repair, painted-source disclosure/new layer,
rasterization, exact sampled history and unchanged input files. Replacing an
imported ICC also correctly invalidates the preceding preview. The final run has
one passing test, no failures or skips; three dialog captures are reviewed. An
unrelated iCloud prompt covers a corner; no permission is changed. Both final
Release builds pass without compiler warnings. Evidence is
`artifacts/apple-source-edit-v1/`.

Initial failures were a missing Rust import, a Swift fixture array-construction
error and a history assertion that expected layer-ID reuse. The corrected test
preserves the shared monotonic-ID contract while comparing exact artwork; no
runtime history workaround is added. Physical UIKit forms/picker delivery,
managed display appearance, 61 MP and sustained SDR performance remain open.
Artist review apps and drawings are preserved. The isolated Mac test app is
closed after testing. Main was fetched with no incoming changes before this
grouped milestone.

The subsequent inspection milestone below closes histogram/sampling. Retained
photo corrections/masks, ICC library/profiled export, managed displays and
remaining device/performance acceptance stay open.
The overall Apple goal remains incomplete.

## Histogram and sample-area workflows

Both Apple hosts now expose the shared full-resolution histogram in a nonmodal
inspector: RGB/individual channels/luminance, log scale, clipping/endpoints,
manual Refresh and debounced Auto update. Immutable project/GPU capture stays
on the existing owner/worker boundary. Document changes, closure and canvas
loss cancel stale work; animated effects disclose the captured time. Visible
paper participates while transparent pixels and display overlays do not.
Point, 3×3 and 5×5 controls now use the existing shared eyedropper model for
both Visible color and Layer color; no host-side averaging is added.

The known-pixel Metal fixture exposed a real shared readback defect: an omitted
row pitch loses lower rows of area samples. Direct GPU requests reproduce it
independently of pointer delivery. One explicit aligned row pitch fixes the
existing per-row copies while retaining tightly packed, bounded storage.
Before/after raw texels are retained with the failed run; temporary diagnostics
are removed. Coverage-weighted document-linear averages, tagged paint,
independent brush opacity and unchanged artwork/history now pass on both Apple
policies. Histogram tests also cover P3/U8 and ProPhoto/U16, all captured pixels,
partial/zero alpha, visible paper, immutable revision identity and cancellation.

All 65 Apple bridge tests, 456 active shared UI/host tests and five existing
renderer sampling tests pass; one pre-existing shared case remains ignored.
The Swift/Metal workflow passes both policies, including manual and automatic
refresh, Undo, close/reopen with late worker results, and sample-control selection.
Evidence is `artifacts/apple-histogram-v1/`.

The native Mac workflow passes editing with the inspector open, manual/automatic
refresh, channels/log scale, close/reopen and all sampling choices. The first UI
fixture read AppKit text labels instead of their values; the corrected check
passes. Its isolated window is resized clear of the deferred iCloud prompt,
without interacting with that permission. Captures expose a minor shadow issue;
the final presentation limits the shadow to the card background. Both final
Release builds pass without compiler warnings. The final native run passes one
test with no failures/skips; all three final captures are reviewed. Physical UIKit
controls and sustained inspection performance remain unqualified. The installed
artist review apps and drawings are preserved. Remaining phase 2 work starts with photo
corrections/masks, ICC library/profiled export and managed displays; provider,
61 MP and sustained performance acceptance remain open.

## Retained photo corrections and masks

The existing shared Filters/Properties path already exposes Exposure, White
Balance, Levels, Curves, Hue / Saturation and Color Balance. No additional Apple
editing path is needed. A new native Metal workflow passes on both Apple
policies with P3/U8 and ProPhoto/U16 drawings and a larger retained U16 photo.
It verifies visible edits, reset, bypass, exact Undo/Redo, local selection masks,
unaffected pixels outside those masks, worker save/reopen and later re-editing
of all six corrections and masks. Metadata, mask backing and original source
samples remain exact, including off-canvas pixels; no source is baked.

The native Mac workflow also passes all six controls, representative numeric
and curve reset/history/bypass, mask creation/inversion, local Save As/Open and
re-editing every reopened correction (one test, no failures or skips). Its final
captures are reviewed. This checkpoint changes only tests/docs; the previously
qualified Release runtime is unchanged. Evidence is
`artifacts/apple-photo-corrections-v1/` (`native-v3.log`, `mac-ui-v3.xcresult`).
The initial native fixtures redundantly
deselected an already-consumed mask selection and required bit equality for a
neutral White Balance conversion differing by about 7e-8. Neutral comparison now
allows 1e-6 Float32 error; exact history, persistence and backing checks remain.
The initial UI attempts used the wrong Hue / Saturation search spelling and
required extra scroll space around an edge-aligned row. The fixture now reveals
the inset thumbnail target. Original failures remain recorded; no runtime
workaround is added.
Artist apps and drawings remain untouched. Physical SDR workflows, profiled
export/ICC library, managed display and final performance acceptance stay open.

## Native provider acceptance

The first Mac iCloud run reaches real native panels with disposable generated
files. Image-import data/history, unchanged source bytes and provider upload
assertions pass, but reviewed captures remain covered by macOS's iCloud access
prompt. Project Save/reopen and invalid-Open checks time out at that prompt.
Full provider UI acceptance remains open; no app defect or permission workaround
is claimed. The original local file passes remain applicable.

Automatic approval review rejected granting broader iCloud access. On
2026-09-16 the user explicitly asks to ignore iCloud because it does not work in
their environment. No permission is granted and no further cloud run is pending.
iCloud acceptance is deferred, not passed. The unused provider test additions
and consent handler are removed; passing local-file tests stay unchanged.
Evidence is `artifacts/apple-provider-native-v1/` and the later user decision in
`artifacts/apple-prediction-adoption-v1/user-acceptance.json`.

## Settings and lifecycle acceptance

Settings now has a complete row map: sixteen rows across five pages, eleven
editable on iPad and ten on Mac with unsupported native prediction off/disabled.
The existing persistence fixture passes every editable row's value change,
fresh-owner restoration and exact durable Reset, plus its prior cross-owner and
failed-save cases. Retained native text, numeric and image-choice XCTest results
are verified without repeating their UI runs. This closes unspecified row-routing
and storage coverage; broader text/keyboard behavior and Settings link rejection
remain scoped separately.
Evidence is `artifacts/apple-preference-coverage-v1/`. The connected iPad review
process was verified unchanged before requesting the physical New Window,
independent drawing/history and continued-input check; the user now confirms
these pass. Further scene restoration and OS lifecycle cases remain open.

The native Settings follow-up reproduces a UIKit cursor picker that does not
open, including a direct touch at its visible control. Its custom icon labels
are replaced with native image labels using the same assets, and the current
selection is exposed to accessibility. All three theme and five cursor choices,
including Done/reopen retention, now work on both hosts. Mac prediction enable,
dependent controls, manual amount editing and Done/reopen also pass. Both final
Release builds have zero compiler warnings. Evidence, captures and milestone
publication verification are under `artifacts/apple-settings-dropdowns-v1/`.

UIKit prediction-switch taps still fail to change the control in the simulator;
their initial enabled/hidden state and native-owner dependency checks pass.
The unreliable switch fixture is removed instead of adding an app workaround.
The user now confirms the physical iPad dependency check passes. This does not reopen
the user's physical smooth-drawing/prediction pass. The Settings, thumbnail and
background-expiration fixes form one milestone following `d96a0a9`; review
drawings and the installed physical review apps are preserved.

The iPad background-expiration audit removes an unnecessary queued `Task` from
the expiration handler. UIKit invokes this handler synchronously on MainActor;
the existing lease now ends before it returns, and later persistence completion
remains idempotent. The exact production helper fails the focused callback-order
check before the change and passes all five groups afterward. That focused change
builds for iPad without warnings; the later Settings follow-up rebuilds both
current Releases. No app installation, simulator or physical OS
expiration was used, so R4's physical expiration/interruption gate remains open.
Evidence is `artifacts/apple-background-expiration-v1/`; reproduction and the
API contract are in [Persistence](../../apps/layer-apple/PERSISTENCE.md#artwork-recovery).

## Startup progress without presentation callbacks

A focused real AppKit/Metal test reproduces startup stopping after three missing
presentation callbacks. The custom drawable counter and its callback retry are
removed; native CAMetalLayer acquisition manages availability, while the serial
owner and one-pending-frame driver still bound submission. Both Apple policies
now reach readiness and pass actual thumbnail artwork/Undo/Redo checks despite
64/66 withheld callbacks. Fast driver lifecycle checks pass. This removes 108
net production lines plus obsolete gate tests, without adding a timer fallback.

Both Release builds pass without warnings. Both physical hosts complete ten
measured minutes of 4K, eight-layer Wet Watercolor with prediction and the full
workspace. Mac/iPad long active intervals are 0.959%/1.997%, with p99/max
11.111/33.334 ms and 16.667/25.000 ms respectively. There is no rejected input,
renderer error, recorder overflow or missing/zero measured presentation callback.
Thermals remain nominal; the canvas sleeps 34.651/24.202 ms after pen-up and
submits no frames in the final five idle seconds. Captures show correct canvas,
Navigator and painted layer thumbnails. These scoped results accept the fix;
other R1–R6 requirements remain open.

The unchanged `4b1837a` iPad baseline also completed its retained 600-second run,
confirming the original startup failure was intermittent. It does not invalidate
the missing-callback reproduction. Both baseline and candidate workload processes
are closed. The normal iPad review is restored on the fix, with all seven recovery
drawings and every backed-up artist file unchanged. The original Mac prediction
review remains running and is brought forward without restarting. Main was
fetched and current. Evidence and publication verification are retained under
`artifacts/apple-startup-stall-v1/`; [timings and limits](../../apps/layer-apple/PERFORMANCE.md#startup-progress-without-presentation-callbacks--2026-09-16).

## Sustained performance evidence review

Current `4b1837a` Mac Release completes ten minutes of 4K, eight-layer Wet
Watercolor with the full workspace and correct painted layer thumbnail. Long
active intervals are 0.937%, thermal samples remain nominal and the canvas sleeps
after pen-up. Two measured presentation callbacks have zero timestamps within
one 33.334 ms gap; they remain explicit limitations. Short current-Release ink
checks also complete on Mac and physical iPad, with correct thumbnails, no rejected
input/renderer errors and no missing/zero measured presentation callbacks.

The corresponding iPad ten-minute attempt is invalid: startup never reaches
shader readiness or drawing, and presentation admission stalls before its
120-second startup timeout. A short subsequent probe starts normally before
any intervention; it does not reproduce or explain that failure. This leads to
the missing-callback reproduction and verified simplification above. Subsequent
runs verify actual startup and measurement rather than process liveness. See the
[current results and limits](../../apps/layer-apple/PERFORMANCE.md#current-4k-watercolor-with-layer-previews--2026-09-16)
and `artifacts/performance/final-4b1837a/`. The installed iPad Release was updated
in place with verified artwork backups; the original Mac review process is
preserved. These validation notes are included in the startup-progress milestone.

Current `d96a0a9` Mac Release ink/watercolor each complete ten measured minutes
without rejected input, renderer errors or missing measured presentations.
Long active intervals are 0.857%/0.939%, thermal samples are nominal and the
canvas sleeps within about 50 ms after measurement. Memory/idle analysis and
exact source/binary identities are retained under
`artifacts/performance/final-d96a0a9/`. Both captures have blank layer thumbnails.
The follow-up reproduces a pending preview readback surviving replacement of its
document/renderer. Benchmark setup bypassed the reset in normal New/Open.
The reset now belongs to shared document publication, and the file-dialog-specific
call is removed. The regression fails before and passes on both Apple policies
afterward; this is AppKit/Metal evidence, not physical UIKit. Both Release builds
pass without warnings, and the short final Mac capture has correct painted and
Paper thumbnails. Evidence is `artifacts/apple-thumbnail-epoch-v1/` and the
retained regression is `apps/layer-apple/tests/layer-thumbnails.swift`.
The ten-minute timings retain their pre-fix thumbnail limitation; the short
capture is not a new sustained pass. Neither result justifies a scheduler
experiment or completes R6. See the [current measurements](../../apps/layer-apple/PERFORMANCE.md#current-mac-ink-and-watercolor--2026-09-16).

The retained ten-minute physical 4K ink pair has no rejected input, renderer
errors or missing/zero measured presentations, with nominal thermal samples.
Rare long intervals (Mac 1.824%, iPad 1.086%) alone no longer block acceptance.
The memory curves slow late in the run, release footprint after pen-up and show
canvas idle within 50 ms on both hosts. The shared history budget is unchanged
from the recorded source. No memory workaround or scheduling experiment is
justified by these observations. They retain their original source scope;
source-qualified final profiles, recorder-off resource/idle-resume behavior,
instrumentation overhead and physical latency evidence remain separate. See
[the detailed review](../../apps/layer-apple/PERFORMANCE.md#sustained-memory-and-idle-review--2026-09-16)
and `artifacts/apple-performance-closure-v1/`. No new hardware run accompanied
this read-only review.

## Drawing-tool coverage reconciliation

All 34 current brush presets map to retained passing native workflows: thirty
painting/erasing presets and four Blend/Liquify presets. The Mac cases verify
mouse artwork and one-step Undo/Redo; the UIKit cases verify native selection,
scrolling and size editing. The current catalog matches those retained results.
The existing Apple bridge suite also passes dynamic catalog-wide reachability
and numeric setting edits on both policies; the inventory contains 317 brush
setting routes across 19 numeric IDs. This closes the unspecified catalog/setting
reconciliation item without repeating a broad UI sweep. It does not claim new
physical Pencil, sensor or interruption acceptance. The 90 total tool entries
include root/group selections and non-brush subtools, whose remaining cases stay
in their own checklist rows. Evidence is `artifacts/apple-tool-coverage-review-v1/`.

## Native refinement and transform acceptance

The remaining Mac native refinement case passes through actual fields, scrolling,
menus, canvas clicks and file panels. Ten cases cover Fill/Auto select expansion,
contraction and smoothing plus leaking/closed-gap Fill. Exported pixels and Undo
are exact; Redo also restores the softened result for both tools. Representative
captures are reviewed. No runtime change was needed. The first fixture failure
targeted a field below the scroll viewport; the existing reveal helper resolves
it. Evidence is `artifacts/apple-native-refinement-v1/mac-v2/`, with one passing
workflow, no failures/skips and no compiler warnings. Retained responsiveness/QoS
warnings do not establish a performance result. UIKit refinement and physical
pen delivery remain open under the release checklist; this passing Mac workflow
does not require a routine rerun.

The existing Mac transform workflow now covers all four mouse corner handles,
verifying scale/position, the fixed opposite corner, independent artwork samples,
Apply and one-step Undo/Redo. All four captures are reviewed. Its edge, rotation,
modifier and cancellation cases also pass. `mac-v3` passes one workflow with no
failures/skips and no compiler warnings; its app executable is identical to the
refinement run, with only the test bundle changed. Physical tablet/Pencil and
interrupted contacts remain separate. These tests join the input-cleanup
milestone; neither review app was replaced.

## Title-bar input cleanup

The cleanup removes the item-level context gesture from Customize
Title Bar. Its native root already owns secondary click, touch/pen holds and
same-contact dragging; the extra UIKit SwiftUI long press did not classify the
device. No replacement input path is added. The mounted actual header passes
AppKit mouse/tablet checks for all three sizes on both Apple policies, including
secondary click, held-menu continuation, narrow overflow, cancellation, keyboard
movement and exact layout history. All twelve shared surface/device combinations
pass, including touch on the header-editor surface. Both Release builds pass
without warnings. Physical Pencil and UIKit recognition remain separately scoped.
Evidence is `artifacts/apple-header-single-input-v1/`. This joins the workspace
cleanup below; the installed review apps are unchanged.

## Workspace context input cleanup

Both places that disable workspace gestures also disable hit testing: covered
groups during panel expansion and retained closing drawers. Their fallback
context-menu branch was therefore unreachable. Removing it also removes the
duplicate tap-menu model/query and obsolete UIKit long press/AppKit right-click
overlay. Interactive sources retain the native workspace root; header context
menus retain the existing native presenter and stale-request guards. Disabled
sources publish no hit geometry and have no tap gesture. Chrome contact observers
remain, in accurately named platform files; the Xcode project is regenerated.

Existing checks pass for drawer geometry/reorder/reopen/closing retirement,
twelve AppKit mouse/tablet groups on both Apple policies, and two native menu
loading/action/late-reply teardown groups. The first drawer check also fails
against the published source: it assumed individual-panel drawers, while the
current preset opens whole columns. The focused fixtures now explicitly select
drawer mode, matching the existing full-app workflow, and compare measured
header width with current shared geometry. No runtime workaround was added.
Both final Release builds pass without compiler warnings. Evidence is
`artifacts/apple-context-cleanup-v1/`; physical UIKit recognition retains its
existing scope, and the review drawings/apps are preserved.

## Prediction Settings follow-up

The shared preferences model hides Prediction amount (including its search
result) on iPad while iPadOS prediction is selected. Unavailable native
prediction displays off and disabled on Mac, while retaining the saved choice
for a supported device. This changes presentation only, not the drawing engine.
The existing prediction dependency/capability tests and both Release builds
pass. The native snapshot transport confirms the iPad hidden row and Mac
off/disabled switch. Updated review apps preserve the artwork; evidence is in
`artifacts/apple-prediction-settings-v1/`. These fixes are grouped with the
manual-lookahead follow-up; the preceding milestone is `3d00463`.

The subsequent Mac report exposed a separate manual-lookahead bug: display
timing capped 8/16/64 ms selections to the same next-refresh lead. The shared
engine now uses the selected manual lookahead while native prediction retains
its presentation target. Constant 400 px/s replay changes 16/64 ms from the old
3.18 px lead to 6.37/25.49 px. All 54 engine tests pass. The native Mac Metal
check exercises both pen and mouse through the C ABI and Settings action: a
pixel 20 px ahead of the real cursor is painted at 64 ms, absent at 0 ms, then
absent after pen-up, with exact Undo/Redo. This is supplied-input rendering
evidence; direct XP-Pen confirmation of the updated app remains separate.
Both Release builds pass without warnings. Updated review apps preserve the
artwork. Evidence and app identities are in `artifacts/apple-mac-prediction-v1/`.

## Resolved iPad Pencil prediction stall

The physical cause is now reproduced from the user's captured Pencil input.
The verified live-stream recorder contains a 14.12-second stroke with 3,390 real
samples, all awaiting sensor updates, and **zero raw UIKit update callbacks**.
The engine let the earliest unresolved estimate hold the committed prefix at
zero, so prediction repainted the growing stroke on every frame. The iPadOS
prediction switch cannot avoid that shared preview work. The earlier
index-matching cleanup did not fix this physical failure; no update callbacks
arrived for it to match. Earlier builds are not established smooth baselines.

The shared engine now bounds the sensor wait with its existing maximum feedback
window (50 ms). It retains correction tokens and uses the existing persistent
stroke rebuild for late updates. No prediction mode, brush fidelity or sensor
correction support is removed. A regression test fails before the change and
passes afterward; all 53 engine tests pass. The native Metal sensor oracle also
passes for G-Pen, Pencil, watercolor and smudge on both Apple policies after
moving its correction beyond that window, including exact Undo/Redo.

Actual-input replay at regular 120 Hz reduces total preview dabs from 5,335,713
to 48,752 and bounds the unfinished tail at 13 samples. Replaying the captured
frame schedule on Mac Metal reduces prediction-enabled CPU-plus-GPU frame p99
from 1,392.12 to 30.54 ms. That schedule retains the original stalls and large
input batches; it is causal before/after evidence, not iPad cadence acceptance.
The full physical trace, decoded samples, replay sources/binaries and results
are under `artifacts/apple-lag-stream-v3/`. See [performance observations](../../apps/layer-apple/PERFORMANCE.md#captured-pencil-root-cause-and-bounded-preview--2026-09-15).

The user confirms **smooth drawing in both cases** on the corrected iPad Release:
Stroke Prediction enabled, with iPadOS Prediction off and on. This closes the
reported physical lag blocker. Both Apple Release builds pass without warnings;
the temporary iPad recorder has been replaced with the production app and the
latest artwork preserved. Normal review launches have tracing disabled.
The Mac review app is also updated, with its recovery drawings preserved and
its Recovered Drawings screen verified.
Builds, exact owned-process identities, artwork backups and the direct user
acceptance are under `artifacts/apple-prediction-bounded-v1/`. The overall Apple
goal remains incomplete; the bounded fix does not claim unmeasured sensor modes
or complete the remaining provider/window/feature acceptance.

## Physical drawing and panel parity milestone

The user's direction remains to finish blockers with simple solutions and
minimal simulator dependence. Most feature implementation and requested visual
changes are published. Do not turn every open acceptance item into another
implementation, callback test or full-suite rerun.

| Area | Status and remaining evidence |
| --- | --- |
| Mac 90 Hz / iPad 120 Hz performance | User accepts smooth drawing with rare measured misses. The captured iPad prediction stall is fixed and directly rechecked with both prediction-source settings. Retain the measured short/sustained distributions; strict p99 misses alone no longer block release. No further cadence experiment is justified without visible stutter or a substantive slowdown. |
| Physical input | Direct drawing, pressure, Undo/Redo, iPad palm rejection and two-finger navigation pass by user report on both hosts, using Pencil and an XP-Pen 14-inch Ultra. Remaining sensor/shortcut/interruption checks should address specific unverified behavior. No hardware keyboard is connected to the iPad; inconclusive injected keys are not application defects. |
| Documents and lifecycle | Physical unsaved background/return passes on both hosts. Review artwork is preserved. Representative provider, process recovery, scene/window and interruption coverage remains scoped separately. Preserve the native results below instead of repeating shared permutations. |
| Final parity | Floating-panel parity passes focused checks. GPU diagnostics show numbers on both hosts, and the resulting investigation's separate prediction lag blocker is closed by physical user confirmation. Resolve concrete missing behavior in the existing inventory. The overall goal remains incomplete. |

GPU diagnostics now reuse the shared asynchronous timer, replacing duplicate
empty-pass timing. Nonempty timestamp markers stay within the existing drawing
submission, avoiding two extra submissions and excluding CPU encoding delay.
The regression reproduces the blank value before the fix and passes afterward,
including results after drawing stops. Hardware timer tests (including a 200 ms
CPU pause), all four native Navigator/Diagnostics tests, both Release builds and
the WebAssembly check pass. The user confirms GPU values on both physical hosts.

The earlier supplied-input probes did not reproduce the user's Pencil failure
and are not physical acceptance of that path. The final raw capture and direct
recheck above close it. Detailed failed attempts and measurements remain in
[performance observations](../../apps/layer-apple/PERFORMANCE.md). Keep that causal
evidence instead of repeating timing or simulator experiments. The completed
physical review's drawings are backed up locally; both original artist app
descriptors are unchanged.

The current floating-panel parity change enables Apple's use of the existing
shared frozen-preview and release-sizing policy. Mounted native controls now
report scrollable height, fixed controls and actual row height through the
existing measurement action. A pre-fix regression reproduces Apple's edge
clamping; all 400 shared UI tests pass afterward. The invisible AppKit fixture
passes both Apple policies, including native measurements, frozen preview,
fitted scrolling release and one-step Undo/Redo. This is local geometry and
shared-policy validation; both current Release builds pass without warnings.
The existing native motion fixture also passes both policies: moving hit targets,
tab clips, resize handles and live Navigator geometry retain their identities.
Evidence is `artifacts/apple-floating-panel-parity-v1/`.

The painted-recovery workflow now fills through enabled native menus, backgrounds
and returns to the same editor scene, verifies unchanged sampled artwork and
fill Undo/Redo, then restarts and opens Recovered Drawings. The restored pixels,
three layers and visible 42% opacity pass on Mac and the existing iPad simulator:
one workflow, no failures or skips on each. Both test builds have zero compiler
warnings. Mac retains one main-thread responsiveness warning without an
attributed application cause; UIKit has none. Final captures are reviewed.
Evidence is `artifacts/apple-background-recovery-v1/mac-v3` and `simulator-v4`.
Initial failures reflected the test undoing Deselect rather than Fill; Deselect
is now performed after the history check. No application workaround was added.
The earlier painted restart-only evidence remains under
`artifacts/apple-recovery-artwork-v1/`; its simulator-v1 run exercised an older
blank-layer check and is excluded from painted-content acceptance.

The physical iPad's current Release app also preserves a generated saved drawing
across an OS switch to Settings and back: same process, matching visible artwork,
Navigator, three layers, thumbnails and camera, with unchanged source bytes.
Settings values are untouched. The first capture preceded file readiness; the
second background/return cycle uses the confirmed painted baseline in the same
process. This is saved-document foreground recovery, not unsaved crash recovery
or physical stroke interruption. Evidence is the `device/` directory in the same
artifact root. The disposable app is removed, the original stopped runner
restored, and both artist editor descriptors are unchanged.

The physical title-bar reproduction is closed by the user's direct touch report:
Space can be dragged from the bank into the bar, then onto the canvas, without
moving the window. Evidence is `artifacts/apple-header-physical-final-v1/`.
Keep the retained simulator top-edge failure separate. Physical XCTest was not
retried; its automation-initialization failure remains unexplained.

The existing simulator two-finger workflow also passes pinch-in, rotation,
a fresh pinch-out, fixed window bounds, unchanged artwork history and Fit
restoration. It hides panels through ordinary workspace customization so
XCTest's corner-starting contacts reach the canvas. No input adapter change or
test-only hit target is added. Evidence is
`artifacts/apple-touch-navigation-v1/simulator-v2/`. Physical contacts and sensors
remain separate. Owned Mac/simulator workflow apps and runners are stopped.

Two small runtime changes accompany this grouped acceptance: Tool Settings
headings use Web's bold text and four-point vertical spacing, and Mac assigns
its Metal layer before enabling `wantsLayer`, following AppKit's hosting contract.
Both-theme component captures and all twenty-two existing assembled Mac canvas
groups pass in their respective scopes. Neither is claimed as a cadence fix.
Current warning-free Release build metadata is under
`artifacts/apple-layer-thumbnail-followup-v1/restored/`.

The subsequent local Files workflow passes on the existing iPad simulator:
native menu painting, Save As, process restart, Open with three layers and
matching sampled artwork, PNG export and picker cancellation. The actual
delivered PNG retains all 3,145,728 opaque blue pixels at 2048×1536. Captures
are reviewed; the passing workflow has no runtime warnings and its build has
no compiler warnings. Evidence is `artifacts/apple-files-artwork-v1/simulator-v4`
and `png-check.json` in that root. This is UIKit local-provider evidence, not
physical or cloud-provider acceptance. Earlier fixture failures concern Files'
changed accessibility labels, not application behavior. No runtime change is
needed, and the passed round trip should not be repeated for cleanup or docs.
The separate cleanup also passes (`simulator-v6`), removes the verified two-item
folder through Files and leaves no generated provider folder. Both owned test
processes are stopped.
These test/documentation changes accompany the prediction, diagnostics and
floating-panel parity milestone.

The narrower Layers observation experiment is reverted after smaller-workload
captures revealed absent thumbnails; the rebuilt Mac comparison shows the
painted preview and white Paper thumbnail using the established revision refresh.
The recovery-observation experiment is also reverted: it brought no measured
cadence improvement. Rejected source and observations remain in ignored
artifacts. No additional invalidation, scheduling or presentation workaround
is retained. Earlier paper-loss interpretation in Zen was incorrect; sampled
paper pixels match across those captures.

Three missing short workload profiles complete on each physical host: ink,
predicted ink and watercolor. All six finish without renderer errors, rejected
input or missing measured presentations. Mac misses its cadence p99 in all
three; iPad ink fits the interval at p99, while watercolor does not. Longer
intervals remain in every run. These results include the subsequently reverted
Layers experiment and are not relabeled as restored-source or sustained passes.
See `artifacts/performance/remaining-workloads-v1/` and
[the performance observations](../../apps/layer-apple/PERFORMANCE.md).
The bounded Animation Hitches and GPU follow-ups establish no new cause; avoid
another diagnostic run without a specific decision it can resolve.

Repeated fixture work, inconclusive input injection and open-ended coverage
expansion have been the major avoidable slowdown. Use focused local checks for
implementation and group necessary native acceptance. The catalog audit covers
all 63 commands on both hosts; it is not behavioral acceptance by itself.

## Current input-state milestone

This batch fixes stale Mac modifiers from other editor
controls. A plain mouse or standalone tablet contact now refreshes every modifier
through the existing shared key route. The assembled Mac fixture reproduces
unwanted Shift-constrained ruler editing before the fix and passes afterward:
twenty-two workflow groups across both Apple policies, including supplied tablet
callbacks. UIKit coverage now passes eighty groups on simulator and physical
iPad GPU, adding edge/corner scaling, Shift proportions/movement/rotation, Alt
centered scaling, cancellation, Apply and exact decoded PNG Undo/Redo. These are
supplied contacts, not physical sensor/key or visible UIKit hit-target acceptance.

Evidence is under `artifacts/apple-transform-input-v1/`: `mac-before.log`,
`mac-final.log`, `transforms-v1` and `device-run-final`. The first physical run
exited cleanly but lost its console output and remains unverified. The fixture
now writes a result bound to a unique launch ID; the final physical run verifies
all eighty groups directly from that report. The simulator pass precedes only
this reporting change. Both disposable fixtures are removed, the original stopped iPad runner
is restored, and both artist app descriptors are unchanged. The contact and
color milestone is published as `9cb252a`, following `f3214d6`.

The shared color-control fix rejects an unfinished foreground RGB
entry after the background paint slot is selected. The existing mounted-control
fixture reproduces a changed background before the fix. RGB fields now take the
paint slot as their existing SwiftUI identity and reject callbacks from another
slot, matching the established brush-field pattern. The focused `brush-color`
case in `tests/property-slider-input.swift` passes on both Apple policies:
retired-field rejection, unchanged foreground/background, fresh background edits
and preservation of an active draft during ordinary same-slot value updates.
Evidence is under `artifacts/apple-color-draft-v1/` (`before.log`, `after.log`).
These are AppKit-hosted shared-control checks, not UIKit input acceptance. No
simulator/device workflow is repeated. Both final production Release builds pass
with zero compiler warnings; current metadata is under
`artifacts/apple-input-state-milestone-v1/release/`. These builds include both
fixes. Main was fetched and already matched the published base. Full feature/
visual, physical input/provider/lifecycle and sustained Mac 90 Hz/iPad 120 Hz
acceptance remain open; the overall goal is incomplete.

The latest status review identifies excessive dependence on simulator/device
setup and inconclusive input automation as a slowdown. Return to specific
remaining feature and perceptible UI gaps, using focused local checks during
implementation and grouped native acceptance. Do not extend callback coverage
as a substitute for closing the remaining physical-input, provider/lifecycle
and sustained-performance gates.

## Previous UIKit canvas-input milestone

UIKit canvas contacts omitted event modifier flags. A supplied Shift-mouse
contact reproduces an unconstrained saved ruler before the fix. Contacts and
hover now use the existing shared key route, as Mac does; each new contact
refreshes every flag because other editor controls can update shared input
without updating the canvas cache. A separate regression reproduces that stale
cache case. Interrupted touch identities also remained ignored on a new began
phase; a new contact now retires that stale entry before ordinary palm rejection.
Constraint and history behavior remain in Rust; no workaround or retry is added.

`tests/canvas-modifiers.swift` passes sixty groups on both the existing simulator
and the connected physical iPad GPU. Supplied mouse/Pencil contacts cover Shift
at start/movement/release, stale control flags, interruption/identity reuse, saved
ruler geometry, and palm rejection in both arrival orders. Artwork checks cover
all seven figure shape/paint combinations with and without Shift, all four
gradients, cancellation, constrained/free painting with all three ruler types,
and exact decoded PNG Undo/Redo. These exercise production UIKit callbacks,
the serial Rust owner and Metal; physical sensors/key delivery, visible editor
hit targets, and full visual parity remain separate acceptance work.

Evidence is under `artifacts/apple-contact-modifiers-v1/`: `before-v4`,
`focus-before` and `interruption-before` reproduce the product failures;
`artwork-scene-final` and `device-run-v2` pass all sixty groups. The final simulator
check takes about fifty seconds including compilation; the physical suite takes
about eighteen seconds to run. Neither uses XCTest or Files. The initial device
fixture terminated before testing because it lacked the scene lifecycle required
by UIKit. Its crash report identifies that cause; the fixture now uses a normal
UIKit scene. The shipping app already did. Earlier local fixture setup errors
and standalone SDK link warnings remain recorded, not treated as product bugs.

Both final Release builds pass with zero compiler warnings; metadata is under
`release/`. The physical run uses a separate callback app, not the Release editor.
The disposable apps are removed and the original stopped iPad test runner is
restored, with both artist app descriptors unchanged. Main is fetched before
publication; the preceding published milestone is `ece8cd3`. Full feature/visual,
physical input/provider/lifecycle and sustained Mac 90 Hz/iPad 120 Hz acceptance
remain open. The goal is incomplete.

## Current image-import milestone

An image chosen for one drawing could be inserted into its replacement when
background decoding finished late. The picker now captures the existing document
epoch, dismisses on replacement and passes that epoch through decode to the Rust
bridge, which rejects stale results before changing layers. Ordinary edits in
the same drawing remain allowed; no revision lock or retry path is added.

Image decoding also bypassed file coordination and could read incomplete bytes
while a coordinated writer updated the file. It now uses the existing scoped,
coordinated reader shared with projects and workspace packages. That helper
returns its operation's value; packages no longer need a mutable result variable,
and the pixel decoder no longer owns file access or duplicates the layer name.
The layer uses the originally selected filename even if coordination supplies a
different read URL. Decoding stays off both the UI and render-owner queues.

Both local regressions reproduce their failures before the fixes. The final
owner suite passes on both Apple policies: writer exclusion, stale-document
rejection, missing/invalid files, fresh import after ordinary edits, one-step
Undo/Redo and unchanged source bytes. The complete affected project-file and
workspace-manager/package suites also pass on both policies. The standalone
orientation/sRGB/alpha/size decoder and existing Metal import/thumbnail/exact-pixel
history checks pass. Evidence is under `artifacts/apple-image-import-ownership-v1/`:
`before-v2.log`, `coordination-before.log`, `coordination-after-v2.log`,
`project-files.log`, `workspace-manager.log` and `native-image-import.log`.
The initial `before.log` omitted offscreen GPU preparation and is not product
failure evidence; its correction is retained. Final local checks have no compiler
warnings; the earlier coordination fixture's semaphore warning is corrected.

Two native Mac workflows pass without failures or skips: actual image selection,
cancellation, filename, sampled artwork and Undo/Redo; plus Layout History
restoration using the corrected capture helper. Reviewed captures show the blue
imported artwork, its layer/thumbnail and the restored full editor window without
unrelated desktop content. These do not establish UIKit picker behavior or full
visual parity. Final Mac/iPad Release builds pass without compiler warnings;
current metadata is in `release-final/`. No simulator or physical run is repeated.
Owned Mac apps are stopped. Main was fetched before publication; the preceding
published milestone is `cbfc58e`. Full feature/visual, physical input/provider/
lifecycle and sustained-performance acceptance remain open. The goal is incomplete.

## Current workspace-storage simplification milestone

Both shipping apps use the shared SQLite workspace library. The unused Apple
JSON workspace writer/retry state, obsolete-file migration scan, migration-only
scene/root fields and native migration requests are removed. Settings retain
atomic JSON storage, synchronization and retry, with one error value replacing
the old two-store error map. Current SQLite scene bindings, working values and
layout history remain authoritative. Obsolete JSON files are left untouched;
shared migration code used by other hosts is unchanged.

A temporary corrupt legacy file blocks startup before the cleanup. The new
coordinator fixture checks that those files are ignored and unchanged, while
current database corruption still reports an error and preserves its bytes.
The three native library checks and complete settings, coordinator, document
and recovery suites pass on both Apple policies. Evidence is under
`artifacts/apple-workspace-persistence-cleanup-v1/`; the initial post-cleanup
fixture assumed synchronous SQLite failure and is retained separately.
Both native Workspace Switcher and Layout History workflows pass on Mac and the
existing iPad simulator: four workflows, no failures or skips. Both final Release
builds pass without compiler warnings; current metadata is under this evidence
root's `release/`. All owned test apps are verified stopped. Main was fetched
and current. No artist storage or physical iPad app is changed.
Mac captures show restored layout and retained workspace pins. Two UIKit captures
use `app.screenshot()` and have a black upper area and right-edge clipping.
The same run's `workspace-history-selected` image uses the existing
`attachEditor` helper and shows the full landscape editor. Other fixture comments
already document the application screenshot API's landscape crop. Workspace
and recovery attachments now reuse that helper, which also excludes unrelated
desktop windows on Mac; no product-layout change or new capture machinery is
needed. The current image-import batch verifies the corrected Mac History
capture. UIKit and recovery replacement call sites still need capture review
with their next relevant grouped workflow. The preceding published runtime
is `60204ed`; full feature/visual, physical input/provider/lifecycle and sustained
performance acceptance remain open. The overall goal remains incomplete.

## Current canvas-input milestone

Mac did not register the existing shared input-interruption callback. A lifecycle
blur or renderer restart could leave its captured contact active when the old
mouse-up never arrived, preventing the next lasso from starting. The corrected
local reproduction fails before the fix. Mac now clears contact/modifier state
through the same callback as UIKit; window focus/close also uses that shared path.
No new input state or workaround is added.

The existing native fixture now mounts the assembled `EditorView`, including
visible panels and overlays, and verifies actual canvas hit targets. All 22
workflow groups pass on the two Apple policies running on Mac: concave lasso
selection/direct fill, all three ruler types, creation/edit cancellation,
fresh-contact recovery, applicable Shift, exact PNG Undo/Redo, saved ruler
geometry, suspend/resume entry points and actual Metal restart without mouse-up.
Window bounds remain fixed. Navigation also checks precise/coarse scroll units,
Shift/Control, pinch/rotation anchors, contact exclusion and recovery after a
cancellation frame. Gesture values are supplied to the actual AppKit callbacks;
lasso/ruler contacts use the native event queue. This does not establish physical
trackpad/tablet/Pencil delivery, UIKit input, OS sleep or OS menu navigation.

Final evidence is under `artifacts/apple-mac-navigation-v1/` (`run-v3.log`), with
22 passing groups and no compiler warnings. Two earlier combined runs fail the
immediate navigation-after-interruption check. A bounded navigation-only diagnostic
confirms both contact callbacks and camera updates. The final fixture completes
a real renderer frame before testing resumed navigation; it adds no product
change. The exact timing of those earlier failures remains unproven.
The Mac contact regression is under `artifacts/apple-mac-contact-interruption-v1/`;
its initial missing-Redo precondition failure is retained separately from the
corrected reproduction. Earlier assembled-editor evidence and its native-alert
fixture correction remain under `artifacts/apple-assembled-canvas-v1/`.

The final fixture closes its owned windows and removes its temporary documents.
Main was fetched and current. Both final Release builds pass with no compiler
warnings; current on-disk metadata is under
`artifacts/apple-canvas-input-milestone-v1/release/`. The preceding published
runtime is `64244d9`. No simulator/device or performance run is repeated for this
Mac-only input fix. Full feature/visual, physical input/provider/lifecycle and
sustained Mac 90 Hz/iPad 120 Hz acceptance remain open; the overall goal is incomplete.

## Current input and scene-recovery milestone

Scene teardown could abandon an accepted lifecycle flush before artwork recovery
began: the native flush callback held the editor weakly. The existing barrier now
retains the editor through the final recovery acknowledgement. The local
reproduction fails before this change. Afterward, both Apple configurations
complete the flush after their last external editor reference is released,
release the editor after completion, and reopen the archive with its named layer
and unsaved state intact. The complete recovery suite also passes. Evidence is
under `artifacts/apple-scene-flush-v1/`; this tests owner lifetime and real file
storage, not OS background-task expiration or forced process termination.

The grouped UIKit hover fix sends the existing shared cancel phase on recognizer
cancellation as well as normal exit. Its standalone callback check fails before
the fix and passes afterward, including active-Pencil exclusion, fresh hover and
unchanged artwork history. Each executable attempt takes about 37 seconds
including compilation, using the existing booted simulator without XCTest or
editor UI automation. Evidence is under `artifacts/apple-hover-cancel-v1/`; the
disposable callback app is stopped and removed. Its fixture compile correction
and SDK deployment link warnings are retained separately.

Both final Release builds pass without compiler warnings. Current on-disk
Release metadata is under `artifacts/apple-input-lifecycle-milestone-v1/release/`.
Main was fetched and was current. No physical-device, broad UI or drawing
performance run is repeated for these focused lifetime/callback fixes. Full
feature/visual, physical input/provider/lifecycle and sustained Mac 90 Hz/iPad
120 Hz acceptance remain open; the overall goal is incomplete.

The keyboard review found no new delivery evidence beyond the retained missing
Command-A/Command-Z callbacks, so no speculative routing fix or repeated keyboard
run was added. Current SDK headers still provide no native iPad fullscreen
request. The preceding published OS file-launch milestone is `8bc6322`.

## Current OS file-launch milestone

Cold OS file delivery reproduced an iPad failure: the drawing was rejected with
"Finish the current canvas operation first" while the blank editor remained.
Metal attachment had enabled Open before first-frame bundled-filter validation
started. External Open now waits for the existing full startup-ready flag and
workspace readiness before submitting its reserved request. The fix changes two
guards; it adds no timer, retry loop or second file-opening path.

The ordered local regression fails before the fix. Complete project-file and
recovery suites pass afterward on both Apple configurations, including existing
overlap, cancellation, save-before-replacement and retry cases. Offscreen fixtures
now drive first-frame catalog completion, and the optional test bundle includes
the app's filter resources. Both Release builds pass without compiler warnings.

Actual cold and warm OS URL delivery passes on Mac and the physical iPad with
two synthetic painted documents. Final captures show the expected artwork;
warm delivery retains the process on each host. Mac opens a second independent
document window and retains the first. Source files remain byte-identical,
including iPad container readback. The device uses `devicectl --payload-url` and
native screenshot capture; no Files authentication, XCTest or simulator is needed.
Evidence is under ignored `artifacts/apple-os-file-launch-v1/`, with that milestone's
Release metadata and final captures in `after/`. All owned processes are closed;
the disposable iPad app is removed and its original stopped test runner restored,
with both artist editor descriptors unchanged. Main was fetched and was current.

Full provider/lifecycle coverage, remaining feature/visual/keyboard/window gaps,
physical input and sustained Mac 90 Hz/iPad 120 Hz acceptance remain open. OS URL
delivery does not prove the complete provider matrix or fix physical XCTest
startup. Close concrete parity gaps with local checks first; reserve grouped
native runs for questions those checks cannot answer. Do not repeat passing
workflows or failed performance experiments without a relevant change or new
actionable hypothesis. The overall goal remains incomplete.

## GPU timing investigation included in this milestone

The bounded timing investigation is complete. Optional GPU recording now retains
raw endpoints and paired Metal clocks; the analyzer interpolates only within
recorded samples and reports missing coverage and sampling uncertainty. Both
Release builds, the actual hardware timer test, native bridge check, Swift
recorder fixture and seventeen analyzer tests pass. Ordinary drawing and frame
scheduling are unchanged. These changes are grouped with the OS file-launch fix
above as one milestone.

One 45-second GPU-timing-on/off pair completes on each physical host. Both cadence
gates still fail. All 54 Mac long intervals and 26 of 31 iPad long intervals in
the timing-enabled runs have GPU completion before their presentation target.
The other five iPad frames were admitted only 1–3 ms before target. This narrows
the timing question but does not establish a scheduling fix or calibrated total
recorder overhead. All measured workloads have complete presentation callbacks
and no zero-time presentations, rejected input, renderer errors or overflow.
One measured iPad GPU observation is skipped and remains explicitly missing.
Setup zero-time presentation callbacks are retained separately. See
[performance observations](../../apps/layer-apple/PERFORMANCE.md#gpu-endpoint-correlation--2026-09-15)
for the full comparison and limits.

That investigation's Release metadata, logs, traces, scripts and source hashes
are under `artifacts/performance/gpu-clock-correlation-v1/`. The current on-disk
apps were rebuilt for the OS file-launch fix above; no performance run is
repeated for that admission-only change. All four
workload processes are closed; the disposable iPad app is removed and its
original stopped test runner restored, with both artist editor descriptors
unchanged. Main was fetched and was current before the runs. No simulator,
XCTest, broad profiler or ten-minute workload is repeated. Return to remaining
feature/state acceptance; do not repeat cadence experiments without a new,
actionable hypothesis. The workspace-command coverage entry also no longer
incorrectly describes its complete manager fixture as unfinished.

## Current workspace-manager cleanup milestone

The current native catalog exposes workspaces, this workspace's toolbars and the
toolbar library. Swift still routed removed storage, metadata/version, trash and
package-picker/export pages, and retained four published values with no view
consumers. Those branches and the unused Back action are removed. Layout History
uses its existing single preview/Cancel/Restore path. External workspace/toolbar
imports keep their coordinated file-queue reader and Rust validation; the unused
native package pickers/exporters and `.capytemplate` app registrations are removed.
The lower-level storage backup/serialization APIs remain covered independently.

The existing manager fixture now exercises supported external URL imports and
permanent deletion instead of metadata restoration, trash recovery and obsolete
picker/export actions. Its current run is recorded under
`artifacts/apple-workspace-manager-cleanup-v1/`. The complete local fixture now
passes on both Apple configurations (`component-v3.log`), including forms and
inline validation, preview/history, copying from another live window, toolbar
actions, permanent deletion, package import and scene teardown. These are
owner/service checks with temporary storage, not physical input/provider proof.
Both existing Mac and UIKit Workspace Switcher and Layout History workflows
pass: four native workflows, no failures or skips. Both Release builds pass
without compiler warnings. That milestone's Release metadata is under
`artifacts/apple-workspace-manager-cleanup-v1/release/`; the preceding document
milestone's build metadata is historical. All owned app/runner processes are
verified stopped. The renderer is unchanged and no physical/performance run is
repeated. Full feature/visual, keyboard, OS file-launch/provider/lifecycle,
physical-input and sustained Mac 90 Hz/iPad 120 Hz acceptance remain open.
The old fixture hang is now identified: it tried to rename an included workspace,
then waited indefinitely after Rust reopened the validation form. Its existing
form helper now has bounded completion and reports that error; the obsolete
rename expectation is replaced by a check of the included workspace's protected
actions. The stopped first run and explicit second-run failure are retained.
The keyboard source review found no new delivery evidence for the retained UIKit
Command-A/Command-Z failures; no workaround or simulator rerun was added.

## Current document Open milestone

File launch now retains its URL through editor startup. The local reproduction
delivers a saved drawing before the first snapshot; the former guard reports
"Finish the canvas interaction before opening a drawing" and loses the file.
The existing pending URL now records whether Open has been submitted, waits for
Metal and workspace readiness, and resumes through existing publications and
workspace initialization. No startup timer, polling loop or second picker is
added. Both initial-state and pre-attachment delivery pass on both Apple policies
with temporary managed workspaces, including overlapping-URL rejection and the
saved drawing's identity/layers. The full project-file suite also passes.
Evidence is under `artifacts/apple-startup-open-v1/` (`after-v3.log`). Two
intermediate attempts timed out: the offscreen fixture first needed the existing
recovery preparation step, and submission also needed workspace initialization's
completion. These checks do not prove OS-level file-launch delivery.

External Open and recovery now reserve their destination before Rust publishes
the request's busy state. The reproduction submits a valid URL followed by an
invalid URL in one MainActor turn; previously the second URL replaced the first
and produced a file-read error. The existing pending URL now also prevents
overlapping recovery and window close. External Open uses the existing edit
completion to retire its reservation and report a document error when a queued
command makes Open unavailable, preserving subsequent Open attempts.

The complete local project-file and recovery fixtures pass on both Apple policies,
including both Open/recovery arrival orders, cancellation, queued rejection and
retry, Save/Export/Open and recovery save-before-replacement. Evidence is under
`artifacts/apple-external-open-admission-v1/`; the final source also passes the
full recovery suite in `artifacts/apple-startup-open-v1/recovery.log`. These are
native file/owner checks with temporary storage and offscreen Metal surfaces,
not provider UI or physical lifecycle acceptance.

Both Release builds pass without compiler warnings. Milestone Release
metadata is under `artifacts/apple-document-open-milestone-v1/release/`;
the renderer is unchanged and no physical drawing run is repeated. Both existing
Mac document workflows pass: native Save As/Save, process restart/Open and exact
decoded PNG preservation, plus New Drawing validation and export cancellation.
The existing UIKit New Drawing/export-picker cancellation workflow also passes:
three native workflows total, no failures or skips. Neither host reports XCTest
responsiveness/QoS warnings in these runs. All owned app/runner processes are
verified stopped. Grouped evidence is under
`artifacts/apple-document-open-milestone-v1/` (`mac-v1`, `simulator-v1`, `release/`).
No physical iPad test is repeated. OS-level file-launch delivery, the full
provider/lifecycle and physical-input matrices, remaining feature/visual/keyboard
parity and sustained cadence acceptance remain open. Continue the remaining
parity/keyboard gaps without repeating these passing checks absent a relevant
change. The overall goal remains incomplete.

## Current tool controls and color preview milestone

Tool settings now include the shared document epoch in their existing editing
context. A local reproduction retains an Auto Select tolerance draft, creates a
new drawing and reselects Auto Select on the reused layer ID: the former field's
late commit changes the new document's default from 10% to 37% before the fix.
The existing component fixture now verifies rejection on both Apple policies,
ordinary draft preservation and accepted edits through the replacement field.
It uses the production document replacement path with an offscreen Metal surface
and in-memory storage. Evidence is under `artifacts/apple-tool-document-context-v1/`.
Both final cases pass; this is AppKit component/owner evidence, not UIKit or
physical input acceptance.

The compact Brush color panel now shows its current color above the precise RGB
fields. Its swatch opens the existing color picker and shares BrushColorButton
with panel configuration, removing the duplicate button body. Both actual Mac
and UIKit panel-configuration workflows pass both swatches/picker selection,
visibility changes, scrolling to RGB editing and subsequent group dragging:
two workflows, no failures or skips. Final native captures are reviewed; the
UIKit capture shows the new swatch, while Mac's short panel is scrolled to RGB.
Mac XCTest reports one responsiveness warning; UIKit reports none.

Both Release builds pass without compiler warnings. Grouped evidence and
**milestone Release metadata** are under
`artifacts/apple-tool-color-milestone-v1/` (`mac-v5`, `simulator-v5`, `release/`).
Earlier brush-state Release hashes no longer describe these rebuilt apps. Four
earlier Mac workflow failures are retained: the configuration ancestor identifier
was replaced by the workspace group, immediate assertions raced shared state
publication, and a live control was outside its short panel's scroll viewport.
The fixture now uses the containing scroller, condition-based state expectations
and the existing reveal helper. Production sources did not change during those
fixture corrections. All owned app/runner processes are stopped. No physical
device or drawing benchmark is repeated for this native-control batch. Full
feature/visual, physical input/provider/lifecycle and sustained performance
acceptance remain open; the overall goal is incomplete.

## Current brush-state milestone

Brush size and opacity fields now capture the selected preset, discard their
draft when it changes and reject retired callbacks. The component reproduction
changes Pencil's 14-pixel size to 37 through the former G-Pen field. Four mounted
cases across both Apple policies pass, including preservation of active drafts
through ordinary value updates and subsequent accepted edits. An intermediate
fixture called the control factory outside observed PanelControls and retained
its initial capture; mounting the real panel resolves that fixture failure.
Evidence is under `artifacts/apple-brush-draft-context-v1/`.

The shared renderer skips private coverage allocation/copying when the existing
single-batch prediction pass reads committed coverage directly. A two-page Metal
regression reproduces the unused resources before the change; afterwards preview
storage falls from 768 to 512 KiB, with exact image equality through multiple-to-
single preview transitions, cancellation and commit. Multiple-batch/watercolor
prediction keeps its private state. Duplicate initial coverage clears and their
two flags are removed: the stroke-owner clear and batch copy already initialize
both surfaces before use. All thirteen final Metal checks pass, including 120
material image comparisons with zero channel difference, coverage reset,
watercolor, contact and project/mask save/reopen/Undo/Redo. Focused evidence is
under `artifacts/performance/preview-coverage-retirement-v1/`.

Both actual Mac and UIKit numeric workflows pass valid/invalid Brush size draft
switching and destination-brush preservation, together with their existing
expression, validation and brush-memory checks: two workflows, no failures or
skips. Both Release builds pass without compiler warnings. The final native
captures are reviewed; Mac XCTest retains two responsiveness/QoS warnings,
UIKit none. Grouped results and **current on-disk Release metadata** are under
`artifacts/apple-brush-state-milestone-v1/` (`mac-v1`, `simulator-v1`, `release/`).
Earlier editing/dialog Release hashes no longer describe these rebuilt apps.

The final Release apps each complete the existing 45-second eight-layer 4K G-Pen
workload, serially after builds and UI automation. Both accept all input, finish
the postlude and report no renderer errors, overflow or missing/zero-time measured
presentations. The Mac artwork and live Navigator capture are reviewed. Cadence
still fails: Mac has 49 long intervals out of 3,748; iPad has 41 out of 5,046.
These are current short runs, not a controlled before/after improvement or
sustained acceptance. Full results and limitations are in PERFORMANCE.md and
this milestone's `physical/` evidence. All owned processes are stopped; the
physical diagnostic is removed and its prior test runner restored, preserving
both artist editor descriptors. All eleven recovery stashes remain. Full feature/
visual, physical input/provider/lifecycle and sustained performance gates remain
open; the overall goal is incomplete.

## Current native feature milestone

Prioritize visible Web/Android parity gaps and simple shared solutions. Keep the
routine implementation loop in focused shared, bridge and native component
checks. The user's latest clarification also places simulator runs outside that
routine loop: name the UIKit-specific question before each run, reuse the existing
build when its source is unchanged, and group workflow acceptance at milestones.
Reserve physical iPad runs for major milestones and hardware-specific Pencil,
provider, lifecycle and performance acceptance. Build both Release targets for
the completed batch; avoid repeating passing checks without a relevant change.
For a simple presentation edit, inspect the existing focused capture instead of
building new accessibility or geometry probes. After an inconclusive UI failure,
require new evidence and a specific hypothesis before another run or input
workaround. Keep unresolved acceptance explicit while continuing independent
parity fixes. Commit only major milestones. All eleven recovery stashes remain.

Do not build new automation infrastructure for each control. Simulator startup
and XCTest fixture repair have consumed too much of the critical path. Keep
unresolved native delivery cases explicit and continue independent parity fixes.

The editing/dialog milestone groups the property fixes below with simpler Layout
History selection and footer actions. History reuses the existing selected-row
style and exposes the selected version to accessibility; its extra check glyph
is removed. Cancel/Restore reuse the workspace manager's solid footer style,
with two-line labels preventing narrow-window truncation. New Drawing's Width
and Height fields now use their catalog labels for accessibility.

Both Mac and UIKit pass property Reset and Layout History selection/restoration:
four workflows, no failures or skips. Mac runs the final source; UIKit's only
subsequent presentation change permits footer text wrapping, covered by final
AppKit captures and UIKit compilation. Both final Release builds pass without
compiler warnings. Native History/opacity captures and the narrow wrapped footer
are reviewed. Mac XCTest retains responsiveness/QoS warnings; UIKit reports none.
Current results and Release metadata are under
`artifacts/apple-editing-dialog-milestone-v1/` (`mac-v2`, `simulator-v1`,
`simulator-v2` build-only, `manager-captures`, `release`). The exact test apps and
runner are verified stopped. No physical iPad workflow is repeated.

The broader manager component fixture was unfinished at that milestone: its
later attempts produced eight AppKit browser/history captures but did not
complete. Close-confirmation hypotheses did not resolve the wait and their edits
were removed. Those attempts remain retained. The workspace-manager cleanup
above identifies the obsolete included-workspace rename expectation and now
passes the complete supported manager fixture on both Apple configurations.
Full feature/visual, device/provider/lifecycle and sustained-performance gates
remain open.

The property Reset fix closes an actual native-menu failure: Reset
restores opacity to 100% but leaves the focused draft visible. Property editors
now retire their drafts and reject late edit callbacks when Reset runs, following
the existing Settings approach. The gradient's footer and context-menu Reset
share this action; its duplicate reset dispatch and layer/epoch/key plumbing are
removed. Ordinary value updates preserve drafts and gesture cancellation remains.
The existing Mac opacity workflow passes idle, valid and invalid-draft Reset,
error removal and one-step Undo/Redo. Its unchanged failing run is retained.

The same batch closes a gradient-structure failure: inserting a
quarter stop reuses the selected middle stop's index, allowing its unfinished
field to edit the new stop. One field-group revision retires drafts when the stop
count changes, while ordinary value/position updates retain their identity. It
replaces three separate field IDs with one group ID and keeps the existing
selection/cancellation guards. Both Reset routes use the parent revision above.

All eighteen component cases and twenty additional callback/history assertion
groups pass across both Apple policies after the Reset routing change. They cover
opacity/color insertion at a reused index, visible draft removal, Undo/Redo and
late callbacks; opacity also covers the native Remove button and repeated Reset
with the same stop count. Initial focused evidence is under
`artifacts/apple-property-reset-state-v1/` (`component`, `mac-v1` failure,
`mac-v2` pass, `simulator-v1` build-only); the earlier gradient reproduction remains
under `artifacts/apple-gradient-structure-state-v1/`. The grouped native acceptance
and current Release products are recorded above. Do not expand the completed
Reset checks into more menu/input setup; continue the broader parity and
acceptance inventory.

The property-state milestone prevents a retired native field from editing its
former layer. The existing component fixture reproduces a delayed commit changing
that layer's opacity from 80% to 37% after Add layer switches the target. All
property controls now share the existing layer-opacity document/layer guard;
the duplicate header guard is removed. Property view identities also include
the document epoch. Former-layer cancellation still reaches Rust in the same
document so interrupted previews can be restored.

A related component failure shows a retired gradient-opacity field changing the
gradient after another stop is selected. Gradient fields now capture their stop
index and reject edits after selection moves to another index. The same guard
covers position and color; no new selection-state or input system is added.
The existing component suite passes all eighteen cases across both Apple
policies, plus six layer/epoch/history and four gradient-opacity/color callback
assertion groups. These are AppKit callback checks, not physical-key or actual
document-replacement workflows. Retained reproductions and final component
results are under `artifacts/apple-property-target-state-v1/` and
`artifacts/apple-gradient-target-state-v1/` respectively.

The same batch refines toolbar dialogs with the existing selected-row style,
native bordered actions, white confirmation labels and red deletion. Four
intermediate captures extend the existing customization workflow. Both final
Mac and UIKit artwork/history and toolbar workflows pass: four workflows,
no failures or skips. Final gradient and toolbar captures are reviewed on both
hosts. Both Release builds pass without compiler warnings. Mac XCTest retains
responsiveness/QoS warnings; these passes do not establish performance acceptance.
Evidence and that milestone's Release metadata are under
`artifacts/apple-property-state-milestone-v1/` (`mac-v1`, `simulator-v1`,
`simulator-v2`, `release`). The exact test apps and runner are verified stopped.
No physical iPad workflow is repeated. All eleven recovery stashes remain.

Manager-to-delete confirmation still retains the larger manager sheet size,
including white margins on Mac. Removing content identity and using item-based
sheets both failed to fix it; both experiments are removed. The earlier UIKit
`v4` toolbar attempt was canceled after over eight minutes without a result.
Its evidence remains under `artifacts/apple-workspace-dialog-presentation-v1/`.
The successful milestone artwork run establishes working UIKit test delivery
before reusing that build for the final toolbar check; no setup workaround is
added. Do not repeat the sizing investigation without new evidence. The later
component checks and Mac Reset workflow above cover further property states.
Full feature/visual, physical
input/provider/lifecycle and sustained Mac 90 Hz/iPad 120 Hz gates remain open.

The Settings text milestone consumes the shared placeholder and seven-character
limit and disables spelling/capitalization assistance for the hex fields.
Observing the focused commit callback in the Done button instead of the entire
form resolves the observed UIKit update loop when a text field gains focus.
Inactive catalog shortcuts no longer reserve Command-A while Mac Settings is
open. A simple bounded field keeps the label available for the native Reset
menu; its explicit accessibility label preserves the field's name on UIKit.
No input adapter, gesture override or new preference behavior is introduced.

Mac `mac-v4` and UIKit `simulator-v6` pass the actual focused text workflow:
selection, shared length limit, Done/reopen, valid/invalid-draft Reset and
retaining defaults after reopening for both theme colors. Mac uses Command-A;
UIKit uses native touch selection. Numeric Done/navigation also passes on both
hosts in `v3` after the focused-value refactor; subsequent changes affect only
text rows. Both final Release builds pass without compiler warnings, and final
Settings captures are reviewed. Evidence and stopped-app checks are under
`artifacts/apple-settings-text-state-v1/`. The final Release metadata is in its
`release/` directory; older performance build hashes do not describe these files.

Retained failures explain the fixes: Mac `v1` reserves Command-A, UIKit `v2`
hangs before typing, `v4` cannot reach the label under the editor's hit area,
and `v5` exposes the missing field accessibility name after the layout change.
UIKit `v3` still fails Command-A; `testSettingsTextSelectionShortcut` retains
that separate acceptance case. Do not expand this completed batch into another
keyboard investigation. Full feature/visual coverage, compact-menu Command-Z,
windowed-iPad behavior and physical input/provider/lifecycle gates remain open.
The shared tile-hashing optimization below is grouped with this milestone;
sustained drawing performance still fails on both hosts.

The numeric Settings Reset milestone fixes an actual native-menu failure:
Reset left the focused expression `2 + 0.25` in the field. Settings now changes
only that number editor's identity when its explicit Reset action runs; a guard
rejects late edit callbacks from the discarded editor. Ordinary numeric snapshot,
expression and draft semantics are unchanged. No new input adapter or preference
row extraction is added. The final actual-menu workflow covers idle, valid and
invalid drafts, error removal and Done/reopen retaining the shared default.

Both final grouped Mac and UIKit workflows pass numeric Reset and ordinary
numeric expression/Done, search/sidebar navigation and keyboard dismissal: four
workflows, no failures or skips. Both Release builds pass without compiler
warnings. Mac XCTest retains responsiveness/QoS warnings; these results do not
establish performance acceptance. Final Settings captures are reviewed on both
hosts. Evidence is under `artifacts/apple-settings-native-reset-v1/`; final
results are `mac-v3`, `simulator-v1` and `release`. The exact test apps and runner
are stopped. The first Mac run assumed Settings reopened on Pen & Input; its
hierarchy shows Appearance. The corrected test explicitly opens Pen & Input,
and `mac-v2` then reproduces the stale draft before the production fix. Those
failures remain recorded. Main fetch finds no new commits; eleven recovery
stashes remain. No physical workflow is repeated. The later text milestone above
closes focused UIKit text Reset; remaining feature/window/keyboard states and
hardware/performance gates stay open.

The same batch fixes the explicit color popup at narrow widths: its fixed
320-point wheel consumed the 24-point side margins and clipped the foreground
swatch outline in a 320-point sheet. A maximum width lets the existing
`ColorPanel` shrink while retaining its normal size in wider dialogs. Eight
AppKit-hosted captures cover both Apple policies, light/dark and 320/500-point
sheets; representative captures are reviewed. The toolbar picker fits at the
narrow width and is unchanged. Evidence is under
`artifacts/apple-workspace-dialog-fit-v1/` (`before-v2`, `after`). The initial
unconstrained fixture host failed to capture a window; the existing fixed-size
host pattern and absolute capture paths resolve the fixture. This retained
failure establishes no product defect. No separate UIKit color-popup workflow
or full-editor pixel comparison is claimed. The command-coverage file records
the published Layers, Settings and property acceptance and this Reset workflow;
its audit uses the retained inventory, without claiming fresh GPU enumeration.
Only explicit Brush Color opens the popup; toolbar Color/Opacity use drawers.
Do not restore the obsolete opacity popup fixture or add another popup path.

The Layers milestone replaces three partial
`LayerPanel` instances with the compact selection menu, percentage opacity field
and four labeled catalog actions used by Web/Android. This restores direct Raise
and Lower actions and removes extra thumbnail/drag owners from configuration.
It reuses `EditorChoice`, `LayerOpacityField`, `ConfigurationFlow` and Rust edits;
ordinary layer-header opacity keeps its compact formatting and existing guards.
A real editor capture also finds the camera badge covering the configuration
slider's trailing button. Moving workspace panels after the badge in the same
SwiftUI stack fixes the overlap without a new layout or input adapter.

Opening configuration also exposes a thumbnail request ownership bug: a new
panel appears before the old one disappears, whose cleanup removes the new
panel's layer request. The real mounted editor reproduces pending thumbnails
with no registered visible layers. `LayerThumbnails` now registers each row
using the existing panel token and layer ID, following the filter-preview
ownership pattern. A focused mounted-editor check passes on both Apple policies
after the fix; the existing renderer supplies the checkerboard pixels unchanged.

The final Mac and UIKit `testLayerConfiguration` pass: selection, Add/Delete/Raise/Lower,
exact order Undo/Redo, opacity edit/history, close/reopen, lock/unlock and Paper's
editable opacity/protected deletion. Both now require visible thumbnail readiness
before and after configuration changes. The grouped hand/eyedropper workflow also
passes on both hosts, covering navigation after the stack-order change: four
workflows total, no failures or skips. Final configuration and panned-editor
captures are reviewed on both hosts; the controls are readable, layer previews
are present, and the badge no longer covers the slider. Both Release builds pass
without compiler warnings. Mac XCTest records runtime responsiveness/QoS warnings;
these workflow passes do not establish performance acceptance.

Final evidence, source hashes and cleanup are under
`artifacts/apple-layers-milestone-v1/` (`mac-v1`, `simulator-v1`, `release`).
The exact test apps and runner are stopped; all eleven stashes remain. No
physical workflow is repeated for this milestone. The fresh Web reference under
`artifacts/apple-layer-configuration-v1/` shows the matching configuration
structure. Its earlier full-editor diagnostic diff retains header/camera/then-blank
thumbnail differences and does not establish full-editor parity. Earlier
configuration and thumbnail failures remain under that directory and
`artifacts/apple-thumbnail-cache-v1/`; the final milestone supersedes their
validation status, without erasing those failures.

The component fixture's native accessibility traversal omits scroll-view children;
its failed checks do not establish actions and must not be repeated without new
evidence. The Mac v1 test uses an overridden container identifier; v2 overlooks
configuration's existing outside-click dismissal before toolbar Undo; v3 wrongly
expects Paper opacity to be disabled, contrary to `LayerControls::for_layer`.
Those failures are retained. The final test locates the actual scroll container
and explicitly closes/reopens configuration around toolbar history. No product
focus, accessibility or gesture workaround is added. The fresh Web build uses
Homebrew LLVM's `clang` and `llvm-ar` through the target-specific compiler env;
Apple clang cannot build its Wasm C dependency.

The Settings presentation milestone replaces Zen's text-only icon picker with
four shared image tiles, matching Web/Android's 48-point previews and 64-point
controls. It reuses the existing icon-tile control, shared choice metadata and
Rust edit/reset actions; ordinary native pickers retain supplied option icons.
Four AppKit-hosted cases pass across both Apple policies and light/dark themes:
clicking each preview updates selection and the live Zen command icon;
Done/reopen retains it and shared Reset restores the default. Representative
captures are reviewed under `artifacts/apple-preference-choices-v1/`.

The same batch fixes narrow shortcut dialogs. The old 420-point minimum clips
the editor's title and actions in a 320-point window. Both shortcut sheets now
allow 320 points while retaining their normal ideal widths; the editor reuses
`ConfigurationFlow` to wrap its footer. Sixteen subsequent AppKit-hosted captures
cover the editor/recorder, 320/420 points, both themes and both Apple policies.
Representative captures show complete labels and actions. Evidence is under
`artifacts/apple-shortcut-fit-v1/`. Later grouped UIKit review finds that its
recorder still occupies most of the screen and inherits plain text buttons.
Standard bordered buttons and native fitted presentation sizing resolve those
visible differences. No custom presentation controller or sizing observer is
added. [Apple's fitted sizing](https://developer.apple.com/documentation/swiftui/presentationsizing/fitted)
uses the content's ideal size; the existing minimum/ideal frames remain.

Both final grouped editor workflows pass with no failures or skips: Mac and
UIKit image selection, the actual Reset context menu, close/reopen, shortcut
search, editor/recorder opening and nested cancellation. Final recorder captures
are reviewed on both hosts and show compact sheets with distinct action buttons.
Both final Release builds pass without warnings. Evidence is under
`artifacts/apple-settings-choice-milestone-v1/`; final results are `mac-v2`,
`simulator-v2` and `release-final`. The test apps and simulator runner are stopped.
No physical test is run for this batch. These results do not establish every
Settings state, focused text/numeric Reset, physical keyboard delivery or full
windowed-iPad acceptance. Main fetch finds no newer commits; all eleven recovery
stashes remain.

The earlier numeric Settings Reset component fixture remains inconclusive: its
field click/key checks and native-menu observation never established a product
failure. Its temporary preference-row extraction stays removed. Preserve
`artifacts/apple-settings-number-reset-v1/` without repeating that fixture;
the later actual-menu milestone above reproduces and resolves the focused
numeric Reset case. The earlier image-choice component fixture also needed capture-based
clicks because its in-process Form accessibility traversal omitted rows; its
failures remain recorded. Neither investigation adds a product input workaround.

A later compact-menu routing diagnostic still fails Command-Z and does not
establish its cause. The capture remains first responder in the key window;
the trace shows an empty default undo manager and ordinary editing-availability
queries, with no Command-Z press or target callback. Temporary source logging
is removed. Evidence is under `artifacts/apple-menu-routing-v1/`. Its isolated
simulator app and runner are stopped. The later Settings milestone replaces that
installed diagnostic product with the reviewed `simulator-v2` source above.
Do not repeat the compact-menu investigation without new evidence.

The property/menu milestone fixes curve-point and gradient-stop selection. Adding
a stop previously left the old endpoint selected, so Position stayed disabled
and Color/Opacity edits targeted the wrong stop. A new curve point could not be
removed without another selection tap. Both views now select the inserted
position from Rust's published list, including single-point history restoration,
without predicting an insertion index. The existing native property fixture
reproduces both failures before their fixes; all eighteen cases pass afterward
across both Apple policies, covering curve insertion/removal/history and gradient
position/color/opacity, one-step history, cancellation and locking. Property
colors reuse the existing RGBA conversion, removing its duplicate helper.

The same batch adds Home/End to the existing shared menu key handler, matching
Web's first/last-item navigation. The native menu fixture reproduces End leaving
the last row offscreen before the fix. It now passes Home/End and last-row
activation at all three window sizes, plus existing submenu, shortcut and
dismissal checks. No native key adapter or input workaround changes.

Evidence is under `artifacts/apple-property-selection-v1/`. All four final grouped
editor workflows pass with no failures or skips: Mac filter artwork/history and
blend choices, and simulator filter artwork/history and submenu shortcuts. Both
filter workflows assert selection immediately after insertion, before another
tap; they retain curve/gradient dragging, removal/reset and sampled artwork
Undo/Redo. Representative Mac/simulator curve and gradient captures plus the
Mac blend menu are reviewed. Results and cleanup are under `editor/`; the
isolated Mac app, simulator app and simulator runner are all stopped. These
focused captures do not establish every visual state or a new exhaustive Web
pixel comparison. No physical workflow is repeated for this batch.

Both Release builds pass (`release/mac` and `release-final/ipad` reports). The
iPad compiler exposed an implicit strong capture warning while preparing scene
closure; making the existing capture explicit preserves its lifetime and removes
the warning. Main fetch finds no newer commits. The compact-menu UIKit shortcut,
remaining feature/visual and physical input/provider/lifecycle cases, and
sustained Mac 90 Hz/iPad 120 Hz gates remain open.

The published refinement batch fixes the iPad toolbar's Zen transition. A direct owner
check on both Apple policies demonstrates that a contact after Zen activation
reveals chrome again, while contact before activation leaves it hidden. The
first `SpatialEventGesture` candidate passes simulator Zen/Tab but subsequently
fails Mac mask-menu and UIKit canvas-pan acceptance. A controlled Mac comparison
with the old observer passes the mask workflow. Those candidates are rejected;
do not restore them based on the earlier Zen-only pass.

The current `editorChromeContact` helper keeps Mac's existing tap observer.
UIKit uses a passive native recognizer delegate to report chrome contacts before
button activation and always declines gesture recognition. It excludes the
native canvas, whose C ABI already reports and can consume a contact before
drawing. The gesture-state contact set is removed. All eight final grouped
workflows pass with no failures or skips: Mac Zen/Tab, mouse stroke/history/mask
actions, and drawer dragging; UIKit Zen/Tab, finger navigation, drawer dragging,
layer grips and held layer-menu dragging. Both final Release builds pass.
Representative owned-window Mac and simulator drawer/navigation captures are
reviewed. The drawer fixture explicitly selects individual-panel mode: its old
setup opened the whole column and then waited for drawer-only tabs. Evidence,
retained failures and final source hashes are under
`artifacts/apple-chrome-milestone-v1/`; final results are `mac-v4`, `simulator-v2`
and `passive-release`. Earlier Zen evidence is under
`artifacts/apple-zen-contact-v1/`. A subsequent physical milestone attempt uses
the signed published source after Xcode setup, with the known iPad connected,
paired and Developer Mode enabled. XCTest again times out enabling automation
before any workflow executes. The review app is restored, its runner is stopped,
and the artist app descriptor is unchanged. Evidence is under
`artifacts/apple-chrome-device-v1/`; this is a runner-initialization failure,
not a product workflow result. Do not repeat device setup without new evidence.
A later read-only lock-state query reports `unlockedSinceBoot: true` and
`passcodeRequired: false`; it does not explain that earlier timeout or establish
physical workflow acceptance. No unlock request or test retry follows it.
Pencil, keyboard, provider/lifecycle and sustained performance acceptance remain
open. These results do not claim every visual state or every native workflow.

The same batch removes the unused native row-menu adapter, its menu projection
and always-false native-drag flags. Both native input adapters already use the
retained shared row menus; only obsolete fixture calls remained. Layer menu
loading stays in the existing guarded query. The two affected fixtures now use
the current menu entry point and retain document-replacement capture coverage.
All three existing AppKit-hosted input fixtures pass on both Apple policies:
layer pickup/menu/history, layer lifecycle/hierarchy, and workspace input/scrolling.
These component checks exercise local native mouse/tablet events; the later
editor workflows above cover the root observer. Neither establishes physical
Pencil acceptance. The row cleanup removes 57 net production lines.
Evidence and source hashes are under `artifacts/apple-row-cleanup-v1/`.

The batch also fixes focused Settings text fields retaining an obsolete
draft after Reset to Default. The shared reset already publishes the default;
`PreferenceText` now accepts that updated value while focused. A native AppKit
reproduction fails before this one-line change. The existing text fixture now
passes ordinary typing/Done/reopen and focused Reset/Done/reopen for both theme
colors on both Apple policies (four cases). It sends the same shared reset action
as the context-menu item; it does not automate the native menu or establish UIKit
delivery. Evidence is under `artifacts/apple-settings-reset-v1/`. Both final
Release builds include this fix; UIKit focused text Reset remains unqualified.
Image-choice Reset is covered by the later grouped Settings workflow above.

The shared menu handler also fixes accelerator lookup across submenu
pages. `AppleContextMenu` exposes its leaf actions; the existing menu key handler
finds enabled bindings in that complete model instead of only the visible page.
The native keyboard fixture fails before the fix and passes afterward, including
a root command from a child page, an unopened child command, and an ignored
disabled child shortcut. Existing navigation, selection, dismissal and three
window-fit cases still pass. Both final Release builds include this change.

The last iPad `testCompactMenuShortcutAcrossPages` result fails: Command-Z
does not dismiss the File page or undo the layer created by the fixture. The
existing `testSubmenusAndShortcuts` passes in the same grouped simulator run.
A direct owner check confirms correct compact-menu bindings and availability
after Add/Undo/Redo on the iPad policy; its subsequent Mac setup fails because
it assumes an iPad-only header item, so it is not a passing two-policy check.
The later v6 diagnostic confirms that `CaptureView` retains first-responder
ownership through submenu updates, with no Command-Z callback. Focus restoration
is therefore not a supported fix. The earlier explicit UIKit key-command
registration also received no callback. In v7, removing the duplicate SwiftUI
row shortcut registration still leaves compact-menu acceptance failing while
the ordinary menu workflow passes. Both experiments and all temporary diagnostics
are removed. These observations do not establish the delivery failure's cause or
physical keyboard behavior. Keep that acceptance open; do not resume speculative
registration/focus workarounds or restart the simulator. Evidence and retained
failures are under `artifacts/apple-menu-accelerators-v1/`. This case is not
repeated for the final passive-contact change without a new keyboard-delivery
hypothesis; its acceptance remains open. The installed simulator app/runner now
contain the reviewed passive-contact source (`simulator-v2` above), and the
Mac test product is `mac-v4`. The shared menu source matches the prior passing
native keyboard check. Keep the failing compact-menu regression visible in the
test suite and do not count it among the eight passing workflows above.

The shortcut editor needs no added Escape handler. A focused AppKit check of
the real shared sheets and serial owner passes on both Apple policies: native
typing records a shortcut, Escape cancels recording, and another Escape closes
the shortcut editor while retaining Settings and all original bindings.
Evidence is under `artifacts/apple-shortcut-dismissal-v1/`; it does not establish
UIKit or physical-keyboard delivery.

The batch also fixes the recovery picker's narrow-window
clipping with one standard frame constraint: minimum width is 320 instead of
450 points, while its normal ideal width remains 600. The original 360-point
owned-window capture clips the heading, Done and Discard. Eight subsequent
AppKit-hosted captures cover 360/600 points, both themes and both Apple policies;
representative captures are reviewed and show the full heading and actions.
Both Release builds pass. These are shared-view presentation checks with
disposable recovery metadata, not new UIKit/provider or artwork-restoration
acceptance. The grouped editor runs do not exercise the recovery picker or its
native file providers.

Evidence is under `artifacts/apple-recovery-fit-v1/`. Initial accessibility
probes could not resolve the SwiftUI Done button, and a geometry-preference
probe reported zero; neither establishes an action or bounds result. Those
failures remain recorded. The final capture-only fixture stays in ignored
artifacts, and temporary product identifiers are removed. Do not resume that
probe for this one-line sizing change. Main fetch finds no newer commits, and
all eleven recovery stashes remain.

The menu milestone follows `7bc66f4`. Shared editor menus retain the parent row
index with each submenu page; returning restores the row that opened the page
instead of jumping to the first enabled item. Their frames also shrink to the
available window bounds. The previous fixed 560-point scroller extended above
and below a valid 700×500 Mac window. Standard flexible frame constraints fix
this without changing the popup presenter or input adapters.

The existing native keyboard fixture reproduces both failures before their
fixes. It now passes nested navigation, selected choices, disabled rows,
shortcuts and Escape, plus constrained scrolling and last-row activation at
700×500, 360×500 and 700×760. All three owned-window captures are reviewed.
Both Release builds pass. Two grouped Mac editor workflows pass with no failures
or skips: choice menus and toolbar style/actions with full Zen/Tab restoration.
The simulator choice workflow passes, including menu bounds and Undo/Redo.
Representative Mac and simulator menu captures are reviewed. No physical-device
run accompanies this menu milestone.

The grouped checks exposed an invalid Mac capture-label lookup after a modal
menu hid its source from accessibility, and an obsolete partial-Zen expectation
in the toolbar fixture. The fixture now caches that label before opening and
expects full Zen, restoring through Tab as in the existing dedicated Zen check.
At that checkpoint the simulator toolbar workflow passed menu bounds and the
style action but failed at Zen. Matching built/installed bundles and stopped
old runners ruled out the proposed stale-runner explanation. The contact-order
fix and passing Zen/Tab result above supersede that unresolved transition.
The production source stays unchanged through these editor runs; only the
affected workflows are repeated after fixture corrections.

Inspection of the retained simulator toolbar recording shows a black frame
after the Zen tap, before teardown. A subsequent direct launch of the existing
compiled app into Zen, without XCTest, produces a normal visible canvas with
chrome hidden. Its bounded trace and capture are retained under
`artifacts/apple-zen-canvas-v1/`; the direct-launch process has finished and its
owned app was terminated. Startup in Zen does not validate the toolbar transition
or Tab restoration, and the recording alone does not establish a renderer fault.
The later contact-order fix resolves the tested transition without a rendering
change; retain these earlier observations as evidence of the investigation.

Evidence is under `artifacts/apple-menu-navigation-v1/`,
`artifacts/apple-menu-fit-v1/` and `artifacts/apple-menu-milestone-v1/`.
Both owned test apps are stopped. The repository fetch finds no newer commits
on main; all eleven recovery stashes remain. Full feature/visual, physical input,
provider/lifecycle and sustained Mac 90 Hz / iPad 120 Hz acceptance remain open.

The preceding milestone `494e8b3` publishes the filter/control and slider-history
batch below. The Settings/dialog milestone now adds:

- Done submits valid focused text and numeric drafts before Settings closes,
  using one standard SwiftUI focused-value action. Typed theme colors and the
  pressure expression `1 + 0.25` survive close/reopen.
- Result and sidebar navigation release search focus. Shared text fields ignore
  unchanged native callbacks, preventing focus loss from restoring a cleared
  query. Decorative sidebar icons no longer duplicate the row's accessibility
  identifier, and standard native column sizing keeps page names readable.
- The unused modal opacity route is removed from the shared control action and
  Apple dialog. Color/Opacity toolbar and header controls already open retained
  drawers; explicit color configuration still opens its color popup. The existing
  visibility guard during dialog dismissal is preserved.

All 400 shared UI tests pass, including drawer/color-popup behavior across all
seven platform configurations. Four native AppKit theme-text cases pass across
both Apple policies. Focused Mac and simulator Settings workflows pass with no
failures or skips, covering numeric draft preservation and result/sidebar
navigation; simulator also verifies both keyboard dismissals. The subsequent
width adjustment is reviewed in four native Mac captures at 560/660 points in
both themes. Both final Release builds and the physical-iPad Debug test build
pass. Unchanged input workflows are not repeated for the width/dead-route cleanup.

The known authorized iPad is connected. Its focused Settings attempt stops before
executing the workflow because XCTest times out enabling UI automation; the
result reports a runner-initialization failure. No old runner was live before the
attempt, so restarting it is not an evidence-based remedy. The review app and
runner are updated to this milestone, the review namespace is restored, and the
artist app descriptor is unchanged. Physical Settings acceptance remains open;
no hardware-input or performance claim is made by these component/simulator
checks. The next native feature batch should address remaining visible parity
and workflow gaps, keeping hardware setup troubleshooting out of routine edits.

Evidence is under `artifacts/apple-settings-text-v1/`,
`artifacts/apple-settings-search-v1/`, `artifacts/apple-control-dialogs-v1/` and
`artifacts/apple-settings-milestone-v1/`. Use `testNumericSettingsDone` for the
numeric Settings route: the old component readout probe could not deliver its
click in the split view and is not maintained. For Settings captures, use the
fixture's owned-window capture; hosting-view bitmap caching omits the native
sidebar. The initial opacity-sheet probe also does not establish a draft bug;
tracing actual UI entry points instead exposed the obsolete route removed here.
The command reference audit passes against
`artifacts/apple-mac-files-v1/inventory.json` (retained inventory, not fresh GPU
enumeration). Main fetch finds no newer commits and all eleven stashes remain.

This control milestone follows published `c6f9926`, the input/navigation batch
described below, including integration of the shared capy mark update.

The control batch replaces the Filters category's separate
native menu with the existing shared choice control. Its separate category icon,
bold choice label, 34-point field and 48-point search button follow Web/Android.
Shared menus now start keyboard navigation at the selected enabled row, including
submenus; disabled choices remain excluded. The existing native menu fixture
fails before that fix and passes after it. The focused Filters component check
then reproduces Escape leaving search open; a standard SwiftUI key handler fixes
it. All eight categories, native search typing, Escape dismissal and unchanged
document state pass in both themes and Apple presets (32 category selections).
Twelve component captures are retained and representative open/search states are
reviewed. These are local AppKit mouse/keyboard events through the real shared
views and Apple owner. The grouped editor and Metal acceptance below extends
this evidence; physical acceptance remains separate. Sources and retained
failures/results are under
`artifacts/apple-filter-controls-v1/`; the reproducible fixture is
`apps/layer-apple/tests/filter-controls.swift`.

The same batch fixes numeric property sliders creating an Undo entry
for every move. A native opacity drag from 100% to 80% previously undid only to
65%. NumberControl now sends the existing effect gesture phases; shared Rust
extends that transaction to numeric/color properties and paint/Paper opacity.
Gradient position, opacity and RGBA sliders use the same path. Discrete numeric
edits retain existing validation; previews follow published values without a
second optimistic-state mechanism. All 400 shared UI tests pass. Sixteen native
component cases pass across both Apple presets, covering one-step history,
view-removal cancellation, late release and locked input (Paper has no lock
case). The first expanded fixture failed by looking for an unpublished layer
`name`; it now identifies Paper by the existing bottom-anchor flag. The Metal
regression passes for paint, Paper, blur and color effects on both configurations,
checking complete pixel equality through preview, history and cancellation.
Both Release builds and Debug test builds pass. Two grouped editor workflows
pass with no failures or skips on Mac (165 seconds) and iPad simulator (162
seconds): filter category selection/search/previews and curve/gradient property
edits; inline opacity expressions, layer switching and one-step drag Undo/Redo.
Representative captures are reviewed, including iPad keyboard clearance. The
source remains unchanged through these runs. The command audit passes against
the retained inventory (reference checking, not a new GPU inventory). Evidence
is under `artifacts/apple-property-sliders-v1/` and its `app-qualification-v1/`
subdirectory. No physical-device run accompanies this milestone. Remaining native
feature/visual cases, full hardware acceptance and sustained Mac 90 Hz / iPad
120 Hz performance remain open.

Published milestone `be1f27d` matches the latest Web toolset and open-drawer styling,
qualifies native region/ruler workflows, and simplifies shared renderer page
preparation. Grouped Mac app acceptance passes painting controls/artwork,
figures/gradients and title-bar tool drawers. Simulator painting controls and
figures/gradients pass in version 1; its drawer bounds assertion mixed application
and screen coordinates in a windowed scene. Using the editor window on both
platforms removes the test helper's platform branch, and the focused version-2
drawer rerun passes. There is no product input change for that fixture failure.
Mac and iPad Debug/Release builds pass. Representative full-editor captures are
reviewed; no physical iPad run accompanies this milestone. Evidence is under
`artifacts/apple-toolset-parity-v1/app-qualification-v1/`.

That toolset milestone's broad UI runs take about 19 minutes on Mac and 10 minutes on
simulator; the focused drawer rerun takes 24 seconds. Region bridge checks take
about 10 seconds including incremental compilation, and sixteen native canvas
workflow groups take 84 seconds including their build. Keep broad UI sweeps and
automation troubleshooting out of the critical implementation loop. Use the
focused commands in `apps/layer-apple/README.md` for relevant changes, and existing
component captures for visual questions. After a fixture correction, rerun only
the affected workflow. Broader UI runs belong at major milestones, with physical
checks reserved for the evidence they alone provide. Do not repeat rejected
input/profile experiments without a concrete new hypothesis. Shared/component
results do not close native and physical acceptance gates.

The user's latest direction is to minimize simulator dependence in the critical
development loop. Implement one concrete parity gap at a time using existing
shared/bridge checks and native component captures. Batch UIKit-only callback
checks, reuse compiled apps where sources permit, and reserve full-editor
automation for a specific unresolved native behavior or a completed milestone.
After an automation failure, separate fixture/setup faults from product faults;
do not repeat a broad sweep or add product workarounds without new evidence.
Hardware setup and profiler troubleshooting have consumed too much time relative
to closing visible gaps. The input batch below now passes grouped acceptance;
return to the remaining native menu/control and perceptible visual gaps before
another performance batch.

Milestone `be1f27d` is published and verified on main. The following input milestone
fixes UIKit key cancellation: the canvas handled press/release but omitted
cancelled presses, leaving Space-to-pan active for the next contact. A four-line
override reuses the existing release route and forwards to UIKit. A focused
UIKit callback check fails before the fix and passes afterward; normal release
also passes. It uses supplied key/press values, the real canvas and serial Apple
owner, and shared contact-routing replies, without a GPU workload or XCTest
editor sweep. The final build/run takes 32 seconds. This is callback/routing
evidence, not physical keyboard or rendered-artwork acceptance. The disposable
fixture and logs are under `artifacts/apple-key-cancellation-v1/`. This fix is
grouped with the pointer/navigation changes below; broader physical gates remain.

The same input batch fixes indirect-pointer buttons on iPad. All contacts
previously reached Rust as primary, so a supplied right-button drag failed to pan.
The existing contact now retains UIKit's press button through movement and release;
Rust keeps its primary-paint, right/middle-pan and ignored-other-button policy.
Eight focused UIKit callback cases pass for those four buttons with release and
cancellation, including a cleared terminal button mask. Shared routing replies
distinguish paint from ignored input, and exact camera deltas verify pan. The
final build/run takes 32 seconds. These are supplied native-value/owner checks,
not physical mouse/trackpad or rendered-artwork acceptance. The disposable app is
removed; fixture, initial failure and results are retained under
`artifacts/apple-pointer-buttons-v1/`. Both fixes are grouped with indirect navigation.
Main is integrated through `de623a9` (Web favicon packaging and public profile
artwork only); Apple/shared sources, all four local paths and eleven stashes are preserved.

The input milestone also adds standard UIKit indirect scroll, pinch and
rotation recognizers, retaining the existing finger/Pencil contact route. Focused
callbacks verify direction, density, modifiers, anchored transforms, cancellation
and active-contact exclusion in 42 seconds including build/run. Shared camera
gestures now ignore input while painting is pending, matching scroll behavior;
the new regression fails before the fix and passes afterward. All 400 shared UI
tests pass in 1.45 seconds of test execution. The Metal camera bridge check passes
for both Apple configurations with exact artwork and Undo/Redo preservation;
both Release builds pass. Evidence is under
`artifacts/apple-indirect-gestures-v1/`. These supplied-event checks do not prove
physical trackpad/keyboard delivery. Grouped navigation and keyboard editor checks
pass on Mac (two tests, 176 seconds) and simulator (two tests, 124 seconds), with
no failures or skips. Both verify Hand pan/Fit, flip state and keyboard focus after
controls/Settings; Mac also verifies visible/layer eyedropper colors. Representative
captures are reviewed, both Debug test builds pass, and the command audit passes
against the retained inventory (reference validation, not a fresh GPU inventory).
The source remains unchanged through these editor runs. Their base is `de623a9`;
all eleven stashes remain. Evidence is under
`artifacts/apple-indirect-gestures-v1/app-qualification-v1/`. No physical device
test accompanies this batch; full input, provider/lifecycle, feature/visual and
sustained performance gates remain open.

Final integration includes `1b04c9c`'s larger, raised capy mark and shared Zen icon
size. Apple resources are refreshed from the shared SVG; its header uses the same
tile-relative icon proportions as Web and Android. All 400 shared UI tests and
both Release builds pass after integration. Twelve focused shared header captures
verify native/shared allocation at three sizes in both themes and Apple presets;
representative captures are reviewed. These are AppKit-hosted shared components,
not new UIKit pixels. The input sources remain identical to the grouped editor
tests, so those workflows are not repeated for this icon-only integration.
Evidence is under `artifacts/apple-indirect-gestures-v1/main-integration-v1/`.

The previously published mask/layer/lifecycle milestone `1e41766` fixes activation ordering with the
existing suspension state, qualifies mask and layer-content workflows on Mac
and simulator, and removes the unused UIKit menu converter. Both coordinator
configurations, affected shared checks and both Debug/Release builds pass.
The detailed scope and retained failures are recorded below. This closes the
queued layer-content batch; remaining native feature/visual, physical input,
provider/lifecycle and sustained performance gates remain open.

This milestone's focused performance cleanup removes the shared destination-texture
helper's whole-document scan and two temporary collections. Fourteen relevant
Metal contact, history, destination-brush and sparse-page checks pass; both Apple
Release builds pass. A short Mac run without a sampler completes correctly but
still misses 53 of 3,740 continuous display intervals. The valid CPU profile
points mainly to wgpu command encoding/finish; it does not establish the cause
of cadence misses. Failed sampler runs are retained and must not be used for
before/after claims. No simulator or physical-iPad run accompanies this batch.
See `apps/layer-apple/PERFORMANCE.md` and ignored
`artifacts/performance/contact-page-preparation-v1/`. This is a simpler preparation
path, not a demonstrated frame-rate improvement.

The following native canvas component batch passes ten workflows using the real
AppKit canvas, local event queue, serial owner and Metal with both shared Apple
configurations. Concave freehand selection/direct fill, Escape/focus/tool
cancellation, next-contact recovery, all three rulers' constrained/free painting
and complete exported-PNG pixel Undo/Redo pass. Native window bounds stay fixed.
The final build/run takes 63 seconds with no simulator or physical-iPad launch.
These are AppKit component results, not UIKit/Pencil or full-editor menu acceptance.
Retained failures exposed an artificial focus-loss step that left the fixture
window inactive, and carrying Snap-off into the next new document. Waiting for
published state alone did not fix the focus issue. It now verifies event delivery
and key-window ownership, uses two
owned windows for real focus changes, and restores the Snap preference. Temporary
input logging is removed; no product input change is required. The new fixture is
`tests/canvas-native-input.swift`; logs and failures are ignored under
`artifacts/apple-canvas-native-input-v1/`. The extended fixture below supersedes
this initial ten-workflow scope and is included in this milestone.

The extended canvas batch now passes sixteen workflow groups in 84 seconds,
including all three rulers' create/edit cancellation by Escape, actual focus loss
and tool change, preserved Redo, next-contact recovery and exact saved geometry
history. Straight/parallel handle edits respond to native Shift press/release;
ruler operations preserve every artwork pixel. The fixture uses normal Save As
to inspect committed project geometry. Earlier failures requested a save task
without a pending Save and grabbed the radial ruler's initial rather than final
center; both fixture mistakes are corrected, with no product input change.
Evidence is under `artifacts/apple-ruler-native-input-v1/`. UIKit/tablet delivery,
stationary Shift preview pixels and full-editor ruler acceptance remain separate.

Main is now integrated through `81ed348`, adding the Web packaging fix, refreshed
reference screenshots and GTK/Web/Android open-tool drawer backgrounds. Apple
subtools use the new Web vertical preview/right-aligned single-line label layout,
two-point row spacing and matching minimum height. Eight direct SwiftUI/Web
comparisons cover Pen/Figure at narrow/wide widths in both themes; they retain
every pixel and have mean channel differences of 0.69–3.38 on a 0–255 scale.
Normal-size review accepts the remaining native text/image rasterization
differences. Open tool controls now reuse the shared button style to fill with
the adjoining panel color, retaining selected-state priority. Six further native
component captures verify both header button paths in light/dark, open/closed
states and selected-state styling. Both current macOS and iPadOS Release builds
pass; the command audit passes against the retained inventory (reference checking,
not a fresh GPU inventory). Captures/builds are retained under
`artifacts/apple-toolset-parity-v1/`. The grouped app-level layout/scrolling
acceptance recorded above completes this batch. Complete feature/menu coverage,
windowed iPad input, physical Pencil/keyboard, provider/interruption and sustained
Mac 90 Hz / iPad 120 Hz acceptance remain open.

Earlier published Apple milestone `1952f31` fixed Color wheel height allocation, the
collapsed-column footer grip rotation, and gray header backgrounds chosen by
the user. Paint and Photo default-column workflows, physical Color contacts,
title-bar customization and tool drawers passed. Its eight unmasked native/Web
comparisons are retained under `artifacts/apple-editor-parity-v1/`.

Published major milestone `0431c36` groups the native control, keyboard,
document, startup and redraw fixes below with artwork/Metal qualification and
current hardware measurements. All 523 integrated regressions, seven Metal
contact/project checks, command/property audits and both Release builds pass.
The ten-minute workloads complete but still fail sustained cadence. Publication
to main is verified; private evidence and all eleven stashes remain local.

The published milestone integrates main through `1fd753a`. Recent pulls add
Windows docking and settings-save recovery, GPU color-test precision bounds,
the equivalent host drawer fixture fix, and Windows brush icons, control contrast,
capture and multiwindow-recovery updates. The latest integration adds only
Windows independent-filter qualification documentation, oracle tooling and
reference provenance, Windows document-automation/menu fixes and Windows-only
header/capture/MSIX tooling, then Windows Snap/painting acceptance and canvas
shortcut focus restoration after toolbar clicks. The subsequent pull adds the
shared swept-contact pencil/ink renderer, ten more presets and regenerated Web
brush previews, plus Windows touch-navigation acceptance. Apple resources are
refreshed with the existing `scripts/prepare.py`; no extra Apple rendering path
is added. Earlier brush acceptance below predates this renderer update.
The following `bf95a48` pull adds only Windows native selection/transform
acceptance; Apple and shared sources are unchanged.
The next `fb81ebe` update is also Windows documentation only.
The `1fd753a` integration includes Windows header surfaces, a Web suspended-GPU
Navigator guard, and explicit grouping of the contact shader's existing integer
hash expression. All seven final Metal contact/project checks and both Release
builds pass after that integration.
The subsequent `1b2e97e` fast-forward adds Windows layer/keyboard changes and a
shared regression for every contact preset painting on masks. Both the new mask
case and the refactored ordinary-contact history case pass on Metal. Apple and
shared production sources are unchanged by that pull; all five follow-up paths
are preserved.
The `f170041` fast-forward adds only Windows Preferences focus/theme choices,
test tooling and Narrator acceptance. Apple and shared sources are unchanged,
and the local gradient-control work is preserved.
The `5aeab5e` fast-forward adds Windows curve gesture/history and its package
acceptance, plus Windows coverage in the existing shared effect-gesture test.
Apple and shared production sources are unchanged. The local lasso regression
merges without conflicts, and all eleven stashes remain intact.
The `c1b290b` fast-forward adds Windows native transform/lasso acceptance,
README workspace images and Windows coverage in the shared stationary-lasso
regression. That regression passes after integration; Apple and shared production
sources are unchanged. The five local mask-test/documentation paths and all
eleven stashes are preserved.
The duplicate local host fix is
removed; all eleven stashes remain. Light header controls
now use Web's half-opacity gray surface; Menu Labels share one rounded surface.
Hover/press/selection feedback layers over that surface. The workspace switcher
uses the shared tab-bar color in both themes. Empty header space retains the live
canvas; Mac application menus remain in the OS menu bar. Native translucent fills
reuse the existing button style, without adding a custom backdrop renderer.
Web's three-pixel backdrop blur is not reproduced over the native Metal canvas.

About now labels Website and Source code beside their tinted native links. Both
bundle names use the public name Capy Canvas; the Mac OS menu and About command
show it. Closing Settings after a search exposed a late native binding callback
that submitted a preference action after closure. The shared Settings view ignores
that callback once its model is gone; no extra presentation state is introduced.

The GPU inventory passes command and property audits: 63 commands in 15 groups,
90 resolved tool choices, and 43 property scenarios with 160 edit/history/reset
routes per host. Catalog coverage does not establish complete native behavior.
The integrated Apple/host/UI/workspace suite previously passed 549 tests:
Apple 48, host 26, UI 389 and native workspace 86, with one host benchmark
ignored. After the Windows-only docking change, all 397 UI tests passed at
`d4877e3`. Four drawer regressions
now explicitly enable drawer mode; main changed the default to whole-column
opening. Product defaults and shared validation remain intact.
The current host suite at `0a760c0` passes 26 tests, with one benchmark ignored.

Mac and simulator version-4 complete-editor light/dark workflows pass. Eight
fresh Web comparisons retain every pixel, with mean channel differences of
0.90–1.49 on the simulator and 1.62–4.58 on Mac (0–255). Reviewed states show the
matching gray header surfaces and workspace pill. OS menus/window controls,
status choices and remaining panel differences are still represented in those
numbers. These captures do not establish all editor states or backdrop-blur parity.

Simulator version 5 passes About information, settings search/close and actual
Safari handoff from both Help and About for both public links. Mac version 6
passes its actual default-browser handoff; version 7 passes About information,
public OS app name and settings search/close. Earlier fixture failures are retained:
native labeled rows combine accessibility labels, Safari's compact address hides
the repository path, and Mac section headings use a different element type.
Physical milestone acceptance remains unrun: both attempts failed before UI testing.

The shared selection/transform workflow uses real menus, numeric readouts and
panel scrolling. Mac and simulator version 5 each pass Select all, Fill,
Deselect, proportional and independent scale previews, zero-scale rejection and
correction, translation, Cancel restoration and Apply/Undo/Redo. Exact 8-by-8
sRGB canvas samples check artwork/history alongside full screenshots; this does
not claim all-pixel coverage. Additional rotation/handle coverage is recorded
below; the full selection-mode matrix remains open. Evidence is under
`artifacts/apple-feature-completion-v1/`.

The updated signed review app and runner are installed on the physical iPad.
Both current attempts timed out enabling automation before any UI test ran.
The second attempt reused the installed binaries without reinstalling. After
each attempt, the review app was restored in its saved namespace and all three
review/runner/artist installed descriptors verified. Do not repeat startup retries
without new evidence of a device-state change. A subsequent scoped lock-state
query reports no passcode requirement; do not ask the user to unlock the iPad
based on these runner timeouts.
The current installed baseline is
`artifacts/apple-application-info-v1/device-v1/apps-installed.json`.
A later read-only CoreDevice query confirms wired connectivity, available DDI
services and enabled Developer Mode. It does not establish recovery of XCTest
startup; no physical app or test action accompanied that query. Its details are
ignored under `artifacts/apple-mac-files-v1/device-status/`.
All current milestone builds, UI results, retained failures, native/Web captures
and audit output are ignored under `artifacts/apple-application-info-v1/`.

## Ongoing canvas tool workflows

After integrating `f051e61`, all seven Metal contact/project checks pass with
no skips: paper/pressure/tilt behavior, ink continuity, spacing, prediction and
frame grouping, all twelve contact presets' archive roundtrip and exact raster
Undo/Redo, plus the existing project/upload/fill checks. Both native builds pass;
all 68 generated Apple brush previews match the shared Web PNG sources. The
fresh GPU command/property audit resolves 90 tool choices per Apple host.
The subsequent version-6 Mac and simulator native sweeps each pass with no
failures or skips. All thirty current painting/erasing presets are selected
through their actual toolbar/group/brush controls, including scrolling the
expanded lists and editing size through the native number field. Mac creates
real mouse strokes (or erasure), checks sampled artwork and one-step
Undo/Redo, and retains the OS-window bounds. All twelve contact-brush captures
were reviewed: grain, nib edges and ink strands are visible, with readable
labels/previews and live Navigator results. The simulator's Pen, Pencil and
Pastel captures show the expanded controls and previews. Its checks do not
substitute touch for Pencil artwork. No new Apple brush/rendering path is
needed. Physical Pencil, sensors, interruption and performance remain open.
Evidence is ignored under `artifacts/apple-mac-files-v1/`.

Native painting version 1 passes all twenty painting/erasing presets on Mac and
the simulator. Both hosts select the actual toolbar, group and brush rows, edit
brush size through the native number field, and preserve OS-window bounds.
Mac creates real mouse strokes, checks their sampled blue artwork (or white
erasure through a blue fill), and restores each stroke with one-step Undo/Redo.
Six reviewed Mac captures cover Pencil, Wet Watercolor, Loaded Oil, Spray,
Dual Texture and Eraser. Simulator captures show readable Paint and Oil paint
choices and size controls; these checks do not substitute touch for Pencil.
The existing transform-control scrolling helper is shared with these tests;
no product changes are needed. Both version-1 builds and GUI tests pass with no
skips.

Version 2 also passes Natural Blender, Smudge, Liquify Push and Liquify Twirl on
both hosts, bringing this batch to all twenty-four presets in the eight drawing
tool families. Mac applies each effect to a contrasting edge created through
native Fill and Rectangle controls. The changed area restores exactly through
sampled Undo/Redo, while two outside samples remain unchanged. All four reviewed
captures show the expected local effect and live Navigator. Simulator checks
selected states, native size edits and the applicable Flow/Strength controls;
physical Pencil artwork, sensors, interrupted strokes and sustained performance
remain separate. Both version-2 builds and GUI tests pass with no skips. No
product changes or new input/rendering paths are added. GUI runs are serial and
all jobs are terminal. Evidence is ignored under `artifacts/apple-paint-tools-v1/`.

Native Mac figure/gradient versions 2 and 6 pass all seven shape/paint combinations
and all four gradient variants. Real mouse drags produce expected sampled edge,
center, outside and endpoint colors, followed by one-step Undo/Redo/restoration.
Full screenshots show the resulting artwork and live Navigator. The sampler now
reads multiple points from one frame and is shared with selection/transform
checks, removing duplicated screenshot/color-conversion code.

The captures exposed Rectangle breaking across lines in a narrow Tool Set group.
Group buttons now let their centered captions use the available tile width;
subtool padding is unchanged. This needs no custom text or layout renderer.
Simulator version 5 passes all figure/gradient choices and the three ruler choices,
including selected state, applicable numeric controls and snapping toggles. Its
capture confirms Rectangle fits on one line. The simulator does not draw these
operations, because a single finger intentionally cannot stand in for Pencil.
Mac version 6 confirms all artwork variants after the padding change. Version 7
passes all three ruler workflows: mouse creation, handle editing, Show/Snap
controls, deletion and sampled-overlay Undo/Redo. Constrained painting, modifiers,
cancellation and physical Pencil ruler interaction remain open.

Keep earlier results distinct: version-1 builds caught a Swift numeric conversion;
simulator version 2 selected zero tests and establishes no acceptance. Native
bundle enumeration found the new tests, and later simulator runs execute both.
Mac ruler version 3 failed its immediate Radial-selected assertion after passing
the first two ruler workflows; selection now waits for the shared owner update.
That Mac run briefly overlapped simulator testing, so the final runs are serial.
Version 6 then sampled the original press position for Radial, whose single center
follows the creation drag to its release. Version 7 corrects the test's sampling
and edit origin without changing ruler behavior. No controller remains paused;
all runs are terminal. Evidence and the current checkpoint are ignored under
`artifacts/apple-canvas-tools-v1/`.

The fresh Metal filter-library suite passes 21 checks (including new negative
color-oracle probes), has three ignored benchmarks, and fails the strict encoded
sRGB8 PNG reference. Forty-three of 160 filter scopes differ above one byte;
maximum straight-channel error is 56. The old retained Metal baseline uses a
different fixture and is not a valid unchanged-output comparison. Mean differences
over black/white are small. The white-background overview shows no obvious
difference, but its 64-by-48 tiles do not establish parity at artwork viewing size.
The strict byte comparison is diagnostic; the user's acceptance criterion remains
perceptual parity. The subsequent [independent Metal comparison](apple-filter-qualification.md)
passes the original sampled one-byte limit with exact agreement. Source/import
pixels match, and all 160 full-size images differ in only thirteen of 15,728,640
pixels: eleven by one channel level and two translucent blur-edge pixels by two.
Compositing over black/white reduces the maximum displayed difference to one.
Six full-size pairs show no perceptible mismatch. The independent Metal sheet
has the same Linux-reference differences as the current sheet, so those sampled
differences also exist in the old algorithms. No production renderer, fixture
or tolerance changes are adopted. The existing reference test can optionally
export full images; the shared independent capture patch accepts Metal.
Other GPUs, complete cross-backend artwork and physical iPad/input/performance
acceptance remain separate. Evidence is under
`artifacts/apple-filter-qualification-v1/`.

## Native filter artwork and history

Earlier simulator version 5 and Mac version 6 each pass the complete native artwork
workflow with no failures or skips. Both create blue artwork through Select/Fill,
add filters through search and ready previews, and check brightness expressions,
Red-channel curve insertion/reset, Gradient Map reversal and endpoint color
editing. Exact 8-by-8 displayed canvas samples validate one-step Undo/Redo and
filter deletion/restoration. Six final full-editor captures are reviewed: bright
blue, Red-curve violet and gradient-stop red, each with matching live Navigator,
readable controls and unchanged OS-window bounds. These are focused native
workflows, not all-pixel filter or physical Pencil acceptance.

Earlier fixture failures are retained. Search state must survive tab changes;
the curve can be taller than its scroll viewport; touch scrolling must begin
outside the curve's editing surface; and layer assertions wait for shared state.
The iPad switch's accessible row includes blank space, so the test targets its
visible trailing control. UIKit exposes its on/off value as text and AppKit as
a number. No product input or rendering workaround is added. The obsolete
direct-event Mac filter probe still expected the old curve accessibility values;
it is removed and the README points to the maintained native tests.

All jobs are terminal. The final Mac-only assertion change leaves the passing
simulator branch unchanged. No physical iPad is accessed in this follow-up.
Evidence and retained failures are ignored under
`artifacts/apple-filter-artwork-v1/`; these focused tests and cleanup are grouped
with the curve fixes below.

The curve surface now uses Web's 12%-text background and square corners through
one shared SwiftUI modifier. Both native Debug builds pass, and all four fresh
Mac/simulator light/dark editor captures are reviewed. These are focused surface
checks; their camera framing is not a new full-editor pixel comparison. This
surface-only change leaves input unchanged; the subsequent gesture correction
is validated separately below.

The Web reference exposed a related hit-target bug: its square SVG plot was
inset by 21 points inside the 242-point-wide input surface. Clicking the visible
endpoint inserted a third point. Setting `preserveAspectRatio="none"` makes the
plot fill its existing hit area, matching Apple's coordinate mapping. The
endpoint regression is part of the existing Web filter checks and passes pickup
and actual movement in both themes; the original failing check is retained.
The probe targets the exposed handle, clear of the overlapping panel scrollbar.
Both corrected Web captures are reviewed. No shared renderer or gesture
workaround is added. All jobs are terminal and the disposable native apps are
closed. Evidence is ignored under `artifacts/apple-curve-surface-v1/`.

The extended curve workflow exposed a real history bug on both native hosts:
version 1 moves the existing Red point correctly, but the first Undo fails to
restore the pre-drag artwork. Every movement had created a separate layer edit.
Version 2 passes on both Mac and simulator, including actual mouse/touch point
movement, one-step Undo/Redo, point removal/restoration, Reset/history and the
preceding brightness/Gradient Map workflows. Both full curve-drag captures are
reviewed with matching live Navigator and unchanged OS-window bounds.

Curve contacts now wrap the existing effect actions in a shared gesture. Rust
uses the engine's established preview, restore and commit operations; release
creates one history entry. Native controls retain their SwiftUI drag gesture
and report cancellation when its contact or view disappears. The shared cases
cover curve/gradient values, multiple updates, empty gestures, exact restoration,
earlier Redo, Escape, blur, invalid input and the document-snapshot idle gate.
Read-only ownership loss cancels a continuing contact, and late terminal events
after renderer suspension remain harmless. The initial guard failure is retained.

At that milestone, the native integration adopts this shared path for curve
points. Gradient-stop dragging still uses individual edits; its native
movement/history acceptance is not inferred from the shared cases.
Final shared/Apple/engine/host checks pass 524 tests, with one host benchmark
ignored, and both current Release builds pass. The first sandboxed integration
could not acquire a Metal adapter; its failures are retained separately from
the passing authorized Metal run. The native version-2 workflows precede the
final ownership-loss guard and guards against cancelled or zero-size release;
the shared regression covers recovery, while native interruption acceptance
remains open. These filter tests, visual corrections, curve history and obsolete
probe removal form one milestone. Physical Pencil acceptance remains separate.
All jobs are terminal. Evidence and retained failures are ignored under
`artifacts/apple-curve-drag-v1/`.
The accumulated filter milestone is published as `15ee5fd`; GitHub main was
verified at that exact commit before the subsequent Windows-only integration.

The subsequent gradient workflow reproduces the same first-Undo failure on
Mac version 1. Native gradient stops now use the existing shared gesture path,
retain their initial grab position, and cancel when the contact/view disappears.
The common effect helper also retains numeric controls' validation callbacks;
the separate gradient action/receipt construction is removed. No Rust protocol,
history, renderer or native recognizer changes are needed.

Mac version 3 and simulator version 4 pass insertion, ordinary selection without
nudging the stop, actual mouse/touch movement, one-step Undo/Redo, removal,
Reset and deletion/restoration. Their activity records include the new steps
and both full gradient-drag captures are reviewed. Earlier curve and brightness
workflows also pass in those runs. Both final Debug builds pass.

Simulator version 3 reports a passing test but executes only the older workflow;
its missing gradient-drag steps mean it establishes no new gradient acceptance.
Built and installed test-bundle hashes match and both contain the new code, so
the cause is unproven. Explicit installation of the current disposable app and
runner precedes version 4, which executes the full new workflow. No product
workaround is added; the private runner now requires its new capture in the
activity record before reporting acceptance.

The native ramp now has Web's rounded corners, colored circular stops and
selection ring, drawn in its existing Canvas. Fresh Web captures expose generic
button minimum height stretching its stops into ovals over the Position row.
One CSS declaration restores their intended size. Both corrected Web themes
pass circular/nonoverlapping geometry checks. Two native dark captures and both
final Web captures are reviewed alongside the light native workflows; the ramp
and handles agree at normal size. Different camera framing and surrounding
property controls remain visible; these are focused control comparisons.

All jobs are terminal and disposable capture apps are closed. No physical iPad
is accessed, and native interruption/Pencil acceptance remains open. These
changes are grouped with the input-control milestone below. Evidence and
retained failures are ignored under `artifacts/apple-gradient-drag-v1/`.

A follow-up curve pickup check reproduces another native defect on simulator
version 1: tapping near an existing handle changes the artwork.
Web already leaves that point unchanged on a click. Apple now retains the
original point and applies only drag translation, using the same established
SwiftUI contact and shared history path as gradient stops. No new recognizer
or effect protocol is added. Mac and simulator version 2 each pass ordinary
selection, preserved Redo, offset dragging, one-step Undo/Redo, removal and Reset,
alongside the full brightness and gradient workflows. Both results verify the
new steps in their activity records, with no failures or skips. All four final
selection/drag captures are reviewed, with matching live Navigator results.
Both Debug and Release builds pass; all 49 Apple bridge tests pass on real
Metal, including the stationary-lasso regression. All 399 shared UI tests pass
after the final main integration.
The curve pickup, gradient appearance/history and empty-lasso fixes form one
input-control milestone. Physical Pencil, full freehand OS delivery, remaining
feature/lifecycle coverage and sustained cadence are still open. All jobs are
terminal. Evidence and the original failure are ignored under
`artifacts/apple-curve-pickup-v1/`.

## Ongoing native selection and fill workflows

The shared icon button now exposes its selected state through the native
accessibility trait. This covers reference-layer and other selected icon controls
with one shared modifier. The Mac
reference-layer workflow verifies that selected state before using the reference.

Native selection inversion passes on both hosts: fill the full selection, invert
to an empty selection and attempt a contrasting fill, then invert back and fill.
Sampled artwork remains unchanged for the empty selection; the final fill restores
through one-step Undo/Redo. Mac version 4 and simulator version 1 pass. The menu
helper is shared with the existing selection/transform workflow.

Simulator version 2 passes all three Fill and Auto select source choices,
selected state, slider/text-field controls, applicable opacity and tolerance
edits/restoration. Mac version 5 passes the six source workflows and an inverted
region. Version 6 strengthens the fixture with a reference outline, an unmarked
divider and a separate target layer, all created through native controls/mouse
input. Visible artwork fills one half, references fill the whole outlined region,
and the empty editing layer fills the canvas. All six sampled-color workflows,
inversion and one-step Undo/Redo pass; full captures also show the live Navigator.
Final Mac version 6 and simulator version 3 builds succeed at `0a760c0`; the
simulator's version-3 build does not repeat its passing UI checks for this Mac-only
fixture change. Pencil region interaction, edge-refinement behavior, lasso,
modifiers and cancellation remain open.

Retain the fixture failures: version 1/3 looked for the integer fields as buttons;
the percent readout expectation also needed one decimal place. Mac version 4
then found both Select-menu and Layer-menu copies of Invert selection; the query
now scopes to Select. Mac version 3 briefly overlapped a build of the next test
bundle; version 4 reran both checks after that build was terminal. Final GUI
workflows ran serially and all current jobs are terminal. Evidence and the current
checkpoint are ignored under
`artifacts/apple-region-selection-v1/`.

The focused Apple region bridge batch passes two Metal tests for both host
configurations without launching either app UI. Thirty-two pen/mouse cases cover
Fill and Auto select expansion, contraction and smoothing. The closed-outline
fixture checks every output pixel, the selected mask's bounds, softened corners
and exact whole-image Undo/Redo; selection history is checked separately from
paint history. Four gap-closing cases distinguish a leaking two-pixel break from
a contained fill. Thirty-two cancellation cases cover contact cancellation,
Escape press/release, blur, tool changes and setting changes before release or
while released-region processing still blocks a document snapshot. They preserve
the artwork, empty selection and existing Redo, then regain snapshot readiness.
No production change is needed for these covered behaviors. The final focused
run takes nine seconds after compilation, with no failures or skips. These are
actual Apple ABI and Metal checks on the Mac; they do not replace native widget,
physical Pencil/keyboard or device lifecycle acceptance. Keep the existing
passing GUI results and group the remaining native refinement cases with the
next feature milestone. Tests are in `native/src/region_tests.rs`; private logs
and the checkpoint are under `artifacts/apple-region-refinement-v1/`.

## Ongoing native canvas navigation

Mac version 5 and simulator version 4 pass single-activation Hand panning,
Fit restoration, unchanged artwork history and stable OS-window bounds. Both
eyedropper choices are reachable, and Navigator flip selected state toggles
correctly. Selected icon accessibility is now owned only by the shared icon
button; duplicate Navigator and collapsed-column modifiers are removed.

Mac versions 2 and 5 also pass actual color sampling: a half-opacity red layer
over blue returns the independently calculated linear-light composite in Visible
color mode, and the red source in Layer color mode. A transparent layer pixel
preserves the existing color; Visible color samples the blue layer beneath it.
Full captures retain artwork, the Color panel, layers and live Navigator.
Simulator version 2 and Mac version 5 pass lasso selection/direct-fill menu
routes and applicable controls. These do not establish native lasso artwork.

Earlier simulator Hand failures sampled portrait storage using landscape
coordinates. The PNG carries EXIF orientation 8; merely decoding it does not
apply that orientation. The shared pixel sampler now uses ImageIO's existing
orientation transform at full resolution. Version 4 passes with the sample
initially verified on white paper and without a second Hand activation.
Simulator version 5 also passes the existing selection/transform and inversion
artwork workflows with this corrected sampler. No canvas input changes were needed.

Keep native lasso artwork and cancellation open. Mac version 3 identified that
the XCTest runner lacks Quartz event-posting permission. That unsupported test
path is removed. The bounded standalone native-event follow-up also did not
produce the expected selection/fill; its results are not acceptance. Its owned
disposable editor is verified closed, and no event-forwarding, window-input or
permission workaround is adopted. Physical testing was not retried. Current
jobs from that navigation checkpoint are all terminal. Its fetch confirmed `0a760c0`;
all eleven stashes remain. Evidence and the checkpoint are ignored under
`artifacts/apple-canvas-navigation-v1/`.

The subsequent stationary-lasso check finds a shared pen-input error: a contact
with too few points reports that a selection needs a closed area. Lasso selection
and direct fill now share one release branch, remove consecutive duplicate
positions, and ignore unfinished paths. This preserves the existing selection
and Redo without an error or history entry; valid enclosed paths still use the
existing selection/fill operations. No native input workaround is introduced.

All 399 shared UI tests pass. The Apple pointer ABI regression passes both host
configurations on real Metal: stationary and cancelled contacts preserve exact
document pixels, selection, Redo and save readiness; enclosed paths produce blue
interiors with unchanged pixels beyond a two-pixel boundary margin and exact
whole-image Undo/Redo. The same regression fails with an error state on the
original implementation, which is retained as a negative control. The first
pixel fixture used viewport coordinates for document-sized readback; correcting
that fixture requires no renderer or tolerance change.

Both candidate Debug builds pass. Mac version 3 passes the native lasso controls,
ordinary clicks, subsequent layer editing and Redo. The original implementation
also passes that mouse check, so it is preservation evidence, not reproduction
of the pen defect. Freehand AppKit/UIKit event delivery, physical Pencil and the
full interruption matrix remain open. All jobs are terminal, no physical iPad is
accessed. These changes are grouped with the gradient and curve pickup fixes
in the input-control milestone above. Evidence is ignored under
`artifacts/apple-lasso-contact-v1/`.

## Ongoing native transform input

Mac and simulator version 2 pass numeric 90-degree rotation, Cancel/Apply and
one-step Undo/Redo, checked with five 8-by-8 artwork samples. Mac also passes
actual mouse right-edge scaling, Shift proportional scaling, Option centered
scaling, Shift constrained movement, Escape cancellation, and a rotation-handle
drag snapped to 75 degrees with Shift. Captures retain artwork, transform grips
and the live Navigator. These establish representative edge/rotation handles;
every corner, tablet/Pencil and interrupted contacts
remain open.
Both runs used `0a760c0`; subsequent integrations through `50e3acd` change Windows only.

The scroll regression passes on Mac version 3 but fails on simulator versions
3–7: after Return, the Tool panel jumps to its top and hides Angle. A live native
trace shows an unchanged scroll view/content size, with an erroneous keyboard
inset whose removal resets the offset. Explicit Return resignation and SwiftUI's
size-change scroll anchor do not fix it; both candidates and all temporary
logging are removed.

Simulator version 8 passes a correction using UIKit's per-window keyboard layout
guide and the existing shared `measure_workspace_bottom` action. Panels reserve
space above the keyboard while the Metal drawable keeps full-window coordinates;
the ordinary control scroller ignores the duplicate automatic keyboard inset.
Simulator version 9 passes all four final workflows: Angle scroll visibility and
unchanged canvas bounds during/after editing, numeric transform rejection and
correction with sampled artwork/history, inline layer opacity/draft cancellation
when switching layers, and filter search/preview/property controls. Reviewed
captures show the edited field and layer target above the software keyboard.
The old layer test's hide-keyboard workaround is removed. The existing Rust
workspace-clearance history/switching regression also passes. Both builds pass
at `f6285ba`; the Mac version-4 scroll/canvas-bounds check passes after the
Windows-only integration to `50e3acd`. All jobs are terminal. Floating/hardware
keyboards and the complete physical/windowed matrix remain open.
No physical device is accessed. Evidence and the checkpoint are
ignored under `artifacts/apple-transform-input-v1/`.

Mac Move/cancellation version 3 passes actual mouse layer movement, release-time
commit and one-step Undo/Redo with five artwork samples. Switching to Pen and
hiding/reopening the Mac app both discard an unapplied transform and preserve
the preceding artwork edit as the next Undo. OS-window bounds remain unchanged.
The test reuses the existing blue-paper geometry and pixel predicates.

Simulator Move/cancellation versions 2–5 expose a black XCTest full-display
capture after Home/activation. Stronger version-3 queries confirm the canvas
still exists; temporary scene logging confirms the same owner survives the
inactive/background/active transitions. The earlier claim of an absent editor
hierarchy was incorrect. All temporary logging is removed. Version-4 canvas
captures contain the editor but show white artwork and Navigator despite a blue
committed layer thumbnail. Opening the Simulator host and repeating the same
binary in version 5 does not resolve the failing display-pixel check.

Simulator versions 6 and 8 pass Move control selection, tool-change and
Home/activation cancellation, the preceding edit's Undo/Redo, and stable window
bounds. Mac version 4 passes the same cancellation checks plus actual mouse Move
release/history. Full captures show the restored artwork and live Navigator.
The shared sampler uses native canvas-element captures on iOS, retaining its
full-resolution orientation correction.

The redraw change is necessary: version 7 removes it while keeping the corrected
capture path and again fails artwork restoration after background return.
The accepted fix requests one fresh frame through the existing native owner
when returning from occlusion/suspension. It sets the shared host's dirty flag;
no document, camera, history, renderer rebuild or new presentation state is
introduced. The existing windowless owner check passes both platform cases for
real Metal attachment, pending admission, resize/resume, callback fencing,
detach and replacement. Both final host builds pass. All jobs are terminal;
no physical device was accessed. Earlier fixture failures are retained: the
toolbar category is Operation, with Move inside, and the suspended application
state exists only on iOS. Evidence and the current checkpoint are ignored under
`artifacts/apple-move-cancellation-v1/`.


## Ongoing native mask and group workflows

Simulator version 4 passes two-child grouping, target selection, collapse/expand,
visibility and ungrouping with sampled artwork and one-step Undo/Redo. Full
captures show both separated pieces and the live Navigator restored. It does
not substitute a finger for Pencil group movement. The initial fixture reused
the first transform's changed pixel selection; it now selects the full canvas
again before filling the second layer. Readout assertions use the shared
one-decimal pixel format. On Mac, a redundant activation of an already-selected
Operation tile opened its drawer over the attempted Move; that test step is
removed.

Simulator versions 1–4 reveal a real narrow mask-link input failure: the recorded
touch lands at its center but selects the neighboring content thumbnail.
Changing thumbnail hit-shape placement and link z-index does not help; both
experiments are removed. The regular 24-point layer-button width fixes unlinking
in version 5, which then passes both independent mask/content transform cases
but fails relinking through the faded button. Fading only its foreground color
keeps the button interactive. Version 6 passes all four linked/unlinked
content/mask numeric previews, Apply/Cancel and sampled-artwork Undo/Redo, with
stable window bounds. The icon retains its dim unlinked state; no custom hit
routing, gesture or new presentation state is added. The now-unused private
width option is removed, keeping every layer button at the same 24-point width.
The existing simulator layer grip/child workflow passes in versions 3–5.

Both final host builds pass. Mac version 7 passes all four mask/content numeric
and actual mouse Move cases, with Apply/Cancel and sampled-artwork Undo/Redo.
Reviewed captures show the independent-content and linked-mask moves alongside
the live Navigator. Its group check stops at an unavailable Mac container
accessibility value: the capture shows Group selected, its target corners and
Properties title. The test now reads the thumbnail's existing selected trait,
as the mask/content checks already do. Mac version 8 passes group target
selection, collapse/expand, actual mouse movement of both children, visibility,
ungrouping and sampled-artwork Undo/Redo. Reviewed captures show both pieces
moved together and restored after ungrouping, alongside the live Navigator.
Final Mac version-9 and simulator version-7 builds pass after removing the unused
width option; this leaves the accepted geometry and behavior unchanged. All
jobs are terminal. No physical device was accessed. Evidence and retained
failures are recorded under `artifacts/apple-mask-group-transforms-v1/`.

The subsequent mask-action workflow passes on simulator version 3 with no
failures or skips. It covers reveal/hide selection, selection-based replacement,
copy while preserving Redo, enable/invert, reveal/hide all, mask application and
deletion, mask-area inspection and replacing/pasting a copied mask onto another
layer. Four 8-by-8 canvas samples check blue/white coverage and exact Undo/Redo;
mask presence checks distinguish applying a mask from leaving it editable. Four
reviewed full captures show the expected regions and matching live Navigator,
including the purple inspection tint only in the hidden area. Activity records
verify the new selection and cross-layer paste steps.

The first simulator fixture tried transforming an empty paint layer. Native
accessibility and shared command policy correctly report that action disabled.
The fixture now fills its temporary layer before transforming it, then removes
that layer to leave the selection over the original artwork. The Mask submenu's
back button shares its visible label, so navigation uses the actual submenu
identifier. Versions 1–3 build on both hosts; version 2 is superseded before UI.
No production code changes are needed by the passing simulator workflow.

Mac version 3 remains unqualified. Xcode was externally replaced while its test
controller started: the log records a missing service and mixed-version framework
symbols, followed by a plug-in assertion. The controller is terminal and its
owned editor is not running. The new Xcode 27.0 license temporarily blocked result
tools. After the user accepted setup, the first-launch check passes and the result
tool confirms the interrupted bundle is incomplete. Both version-4 Mac and
simulator builds pass with Xcode 27.0. The existing iOS 26.5 simulator runtime
remains available while Xcode downloads the new runtime; its passing version-3
workflow needs no repetition for this unchanged test source.

Mac version 4 then passes the complete mask-action workflow with no failures or
skips, including the new selection and cross-layer paste activities. Four reviewed
captures show reveal/hide coverage, mask inspection and the copied mask on the
second layer, each with matching Navigator artwork. No production change is
needed on either host. All jobs are terminal; physical input and the broader
feature/lifecycle/performance gates remain separate and incomplete.
These checks are grouped with the layer and lifecycle milestone below. Evidence
and retained failures are ignored under `artifacts/apple-mask-actions-v1/`.

The layer-content workflow covers duplicate/clear independence, alpha and
editing locks, clipping layers and bulk clipping-stack duplication/deletion. Its
first simulator run reaches duplication/history, then stops because the test's
Clear layer label also matches a toolbar button. The shared test helper now uses
the existing menu-action identifier. The next run stops at startup with workspace
ownership recovery before these actions. After the lifecycle fix below, Mac
version 3 passes startup and duplication/history but cannot activate Clear layer
below the menu's visible scroll area. Its retained recording confirms ordinary
menu scrolling is needed. The test now reuses the existing control-reveal helper;
no product layout or input workaround is added.

Mac and simulator version 4 each pass the complete workflow with no failures or
skips. Five reviewed full editor captures per host show the restored blue copy,
alpha-locked red fill, clipped red copy, two clipping stacks and preserved original
blue layer. Navigator matches each result. Layer counts, editing-lock capabilities,
exact four-point 8-by-8 samples and one-step Undo/Redo pass with unchanged window
bounds and no Canvas error. These native workflows supplement the focused shared
bulk-clipping and Apple layer-policy checks, which also pass. Evidence and earlier
failures remain under `artifacts/apple-layer-content-v1/`.

That startup capture prompted a focused coordinator regression: after immediate
suspend/resume, Swift reports editing enabled but a delayed suspension write makes
the Rust editor read-only again. The original coordinator fails an actual edit
after resume. The fix checks the existing suspension state before delayed writes,
records activation before waiting for startup/storage, and ignores a resume
superseded by suspension. No new state or task abstraction is added. Both Apple
configurations pass the complete coordinator check, including startup activation,
editing after resume, storage responsiveness, ownership recovery and restart.
The follow-up check also queues activation behind a blocked save, suspends again,
and verifies that completing the save leaves both Swift and Rust read-only until
a fresh activation. Both configurations pass without opening an app window.
Both current Debug app builds and the Mac/simulator layer workflows pass after
this fix. The full physical lifecycle and interruption matrix remains separate.
The older takeover fixture also predates native kernel locks; it now uses actual
detach/reopen instead of expiring a timestamp to steal a suspended owner's lock.
The exact timing of the earlier simulator startup failure remains unproven.
Failures and evidence remain private under `artifacts/apple-workspace-activation-v1/`.
The fix and coordinator checks are grouped with the mask/layer milestone.

The unused UIKit `AppleContextMenu.nativeMenu()` converter is removed together
with its generated-project and isolated-row compile references. UIKit already
uses the shared editor menu; the AppKit converter remains used by native context
sources. The existing UIKit row fixture compiles without the removed converter,
and the final simulator Debug and iPad Release builds pass. Mac Release also
passes; its compiled sources are unchanged by this iPad-only removal. The passing UI workflows use the
same active menu sources. Complete native feature/visual coverage, windowed iPad
input, physical Pencil/keyboard/provider/lifecycle and sustained Mac 90 Hz/iPad
120 Hz acceptance remain open. The overall goal is **incomplete**.

## Native Mac full-screen editor

Mac full-screen version 2 passes the actual AppKit View-menu transitions,
full-window Metal canvas bounds, retained scene identity, visible panel/workspace
controls and live Navigator. A real mouse Pen stroke commits and restores with
one-step Undo/Redo. The preceding full-canvas fill also restores through history
inside full screen and after returning to the exact original window bounds.
Clock visibility follows the native transition callbacks. Reviewed full-screen
artwork/stroke and returned-window captures show the expected layout and artwork.

Only the shared test file's Mac branch and its Mac wrapper are added; no product
change is needed. The first build's nested pixel predicate exceeded Swift's
type-checking limit and was replaced by ordinary loops. The version-2 signed
build and its one GUI test pass with no skips. This covers the light Paint editor
on the current display; complete preset, multi-display and interruption coverage
remain separate. All jobs are terminal, with no physical iPad access. Evidence
and the retained build failure are ignored under
`artifacts/apple-fullscreen-editor-v1/`.

## Native keyboard focus and text history

Version-3 native checks find that Return closes a number field but leaves
printable canvas shortcuts inactive on both hosts. A shared completion request
now returns focus to the existing native canvas after the field closes. Native
adapters restrict that request to the active window without a presented sheet;
Tab keeps native focus traversal. Settings dismissal uses the same request.
No global key monitor or extra focus state is introduced.

Mac numeric Command-Z also reached artwork history instead of the field's text.
The native field now gives text Undo/Redo precedence through its existing AppKit
undo manager. Mac version 5 passes both workflows: shortcuts after toolbar
activation, Return, Escape and Settings search/close; native text selection and
Undo/Redo preserving sampled artwork; and artwork Undo/Redo after text editing
ends. Reviewed captures retain the expected field value, artwork and Navigator.

Simulator version 6 passes toolbar/Return/Settings shortcut routing with stable
window bounds. Version 5 also passes the existing Angle keyboard-clearance and
scroll-retention regression. Physical Escape remains separate: the earlier
first-responder probe did not receive XCTest's Escape injection. Numeric text
history still fails at Command-A replacement (`12396` instead of `96`). The
version-7 trace records initial automatic Select All before typing, but no field
press or Select All callback for the later Command-A injection. Extra UIKit
Select All/Undo/Redo commands did not resolve it and are removed. Keep native
iPad modifier-key delivery and text history open; this is not a passing result
or proof of physical-keyboard behavior.

All temporary logging is removed and sources match version 6. The final Mac
version-6 and simulator version-8 builds pass; all jobs are terminal, with no
physical device access. Broader text-menu validation, Tab traversal and hardware
keyboard combinations remain separate. Evidence and retained failures are
ignored under `artifacts/apple-keyboard-focus-v1/`.

## Native Mac File menu and project roundtrip

The native roundtrip exposed a missing OS File menu: replacing its initial
New group with an empty, unfocused editor projection let SwiftUI remove the
menu before the scene connected. The Mac command group now offers New Window
through SwiftUI's existing window-opening action when no editor is focused;
the focused editor supplies the shared catalog. The ordinary native menu
appears between the app and Edit menus. No extra presentation state, menu
coordinates or event forwarding is added.

Version 5 passes the actual File menu, Save As, Save after adding a layer,
process restart, Open and two PNG exports, with no failures or skips. The
restored project contains all three layers and matching sampled blue artwork.
Every decoded pixel of the 2048×1536 PNG matches before/after reopening, and
Open/export leave the saved project bytes unchanged. Both reviewed captures
show the artwork, layers, live Navigator and refreshed ink previews. The test
uses a new temporary UUID directory and removes its generated files. The
earlier accepted physical-iPad Files roundtrip is retained without repetition.

Keep the failed iterations distinct: version 1 exceeded Swift's expression
type-checking budget; version 2 queried the Mac title label instead of its value;
version 3 established the missing File menu. Version 4 reaches native Save As
after the product fix, then targets a hidden Touch Bar Save copy. Version 5
scopes confirmation to native panel windows. Version-6 builds pass on Mac and
simulator, followed by the thirty-preset native checks recorded above.

Version 7 replaces the temporary disabled New item with the working windowless
action. Its native window test passes, but visual review reveals a blank,
red-outlined layer-opacity readout. Editor controls could mount before their
shared catalog arrived, and the failed initial numeric formatting remained.
EditorView now mounts controls when both state and catalog are present. One
shared readiness check covers both hosts; the full-editor fixture also requires
a nonempty opacity readout.

Version 8 passes the Mac build and strengthened window regression with no
failures or skips. Closing the last window retains the app; both the File menu
item and Command-N open a distinct editor scene with two initial layers, ready
Metal and formatted controls. The reviewed final capture restores the 100
opacity readout and full slider without the error outline. The file test's
temporary full-app dialog hierarchy dumps are removed; functional assertions
remain. The simulator build and both light/dark startup checks also pass with
no failures or skips; their opacity readouts and captures are reviewed. All
jobs are terminal. Physical work in this follow-up is limited to the read-only
connection query above; no commit or push is part of this batch. Evidence is
ignored under `artifacts/apple-mac-files-v1/`.

## Current physical drawing performance

The shared CPU change enables sha2's existing runtime-detected
AArch64 SHA backend through its dependency feature, without changing tile data
or capture scheduling. Six local tile-encoding cases improve 4.5–5.6 times with
identical digests and compressed bytes. The 48 default core/workspace checks,
86 native workspace checks and iOS core compilation pass; dependency resolution
keeps the feature off Wasm and Intel targets. Both changed Release builds pass
without compiler warnings. One 45-second before/after pair completes on each
physical host, with no rejected input, renderer errors, overflow or missing/
zero-time measured presentation callbacks. Mac long intervals change from
57/3,734 to 49/3,772; iPad changes from 28/5,017 to 18/5,008. Presentation p99
is unchanged on both hosts, and both cadence gates still fail. The pairs do not
establish reliable cadence or memory improvement; do not repeat ten-minute runs
or CPU sampling on this evidence. Mac captures show the expected artwork and
Navigator. No simulator or XCTest setup is needed for these direct workloads.
Evidence is under `artifacts/performance/tile-sha-acceleration-v1/`; details are in
[performance observations](../../apps/layer-apple/PERFORMANCE.md).
This validated optimization is grouped with the Settings text milestone above.
Resume the remaining feature/state acceptance.
Reuse the saved benchmark results: rebuilding its baseline with the changed
core manifest would enable acceleration. The current Release products include
this change; their old pre-change build metadata is not suitable for launching
them. Use the Settings text milestone's final Release metadata above.

Both current Release builds pass after the shared contact-renderer integration.
Serial 45-second `layered-4k` runs use the unchanged eight-paint-layer G-Pen
fixture, pressure variation, prediction and 240 Hz synthetic input. Both native
hosts complete with no rejected input, renderer errors, recorder overflow or
missing/zero-time measured presentations. The Mac capture shows the expected
layered artwork and live Navigator. Actual cadence still fails both targets:
62 of 3,728 continuous intervals are long on Mac, and 35 of 5,035 on iPad.

The same physical iPad binary completes 600.008 measured seconds and the normal
postlude. All 135,003 nonpredicted samples are accepted; 67,295 presentations
are recorded without missing or zero-time measured callbacks. CPU p99 is
9.245 ms, and 727 of 66,919 continuous intervals exceed the 120 Hz cadence
threshold. Measured footprint grows 291.28 MiB, with nominal thermal state.
This is completed workload evidence with failing performance, not acceptance.
The benchmark process is closed, its disposable app removed, and the existing
test runner restored. Both editor descriptors are unchanged; the review app
is not updated or restarted. Direct workload launch does not establish recovery
of physical XCTest startup, which is not retried.

The same Mac binary completes 600.001 measured seconds with 135,001 accepted
nonpredicted samples and 50,213 actual presentations. CPU p99 is 4.073 ms, but
909 of 49,837 continuous intervals exceed the 90 Hz cadence threshold. Measured
footprint grows 326.47 MiB with nominal thermal state. The postlude completes
without renderer errors, recorder overflow or missing/zero-time measured
presentations; its full-window artwork capture is reviewed. The owned process
is closed. Both hardware runs are terminal and both cadence targets remain open.
Results, reproduction scripts and the checkpoint are ignored under
`artifacts/performance/contact-layered4k-fb81ebe/`. See
[performance observations](../../apps/layer-apple/PERFORMANCE.md) for the complete
short-run figures and the distinction between pen-up CPU tails and presentation
gaps during drawing. No renderer, admission, fidelity or snapshot change is
adopted from these measurements.

## Final integration checks for the editor milestone

The integrated Apple suite exposed a real contact-brush mask failure. Mask
painting removed the paper texture while leaving the new Pencil contact model's
paper strength enabled, producing an invalid advanced-brush error. The shared
engine now disables that material when converting the brush for coverage,
retaining its contact geometry and dynamics. The existing Metal project test
again paints Pencil into masks, applies/transforms them and reopens their exact
raster backing on a fresh GPU.

Two workspace checks failed on a one-ULP JSON number round trip in the new
brush controls. They now compare the serialized snapshot against the same
Value-to-JSON round trip, preserving exact wire, geometry and model assertions.
Both focused checks pass. The independent host/UI run also passes: 26 host
tests with one benchmark ignored, and 397 UI tests.

The estimated-sensor oracle's new G-Pen comparison differs in five backing and
five composited bytes by one encoding level on each Apple preset. Pencil,
Watercolor Wash and Smudge remain exact. Four full-document G-Pen captures were
reviewed with no perceptible mismatch. The test compares every rendered byte
within one encoding level, while tile structure/formats, watercolor state and
each result's Undo/Redo remain exact. This follows the user's perceptual
criterion; no rendering change is made to eliminate those five byte differences.
Temporary diagnostic logging/export code is removed. The earlier strict
failures and captures remain in the ignored performance checkpoint above.

The final integrated suite passes 523 tests: Apple 48, engine 52, host 26 and
UI 397, with one host benchmark ignored. All seven Metal contact/project checks
and both Release builds pass after the final main integration. The ten-minute
measurements precede the mask-only engine change and explicit shader-hash
grouping; ordinary color-painting arithmetic and the measured workload are
unchanged. Command and property-evidence audits pass on both host inventories.

## Unresolved native input and acceptance

The window-drag investigation adopted no product changes. Three placements of
UIKit's public window-drag failure relationship either failed to fix top-edge
customization or interfered with canvas contacts; all are removed. Restored
production code passes simulator canvas-contact and tool-drawer checks but
still fails title-bar customization by resizing the OS window. Center taps on
a left header menu also opened its neighbor; lower-button taps reach the correct
menu in the artwork workflow. Keep top-edge input acceptance open. Evidence is
under `artifacts/apple-windowed-input-v1/`; the matching earlier physical title-bar
workflows pass. No event-forwarding or window-input workaround is adopted.

The follow-up probe rules out stale source validation for the reproduced top-edge
failure: the direct touch reaches the correct item, the contact remains valid at
UIKit's begin decision, and no hold is required, but the pan never begins. The
full-window capture reports no top safe-area inset, so fixed top padding is not
justified. An interactive overlay with the public window-drag failure relationship
also fails the bank-add step and is removed. A small standalone UIKit fixture
then reproduces a top-edge failure on iOS 26.5 with the status bar hidden and
the pan and window-drag interaction on the same header view. Removing the hold
recognizer and custom simultaneous-gesture policy still produces no completed
drag. The iOS 27 comparison with the app-like policy begins and then cancels its
pan. Initial fixture runs left the status bar visible and do not match editor
presentation; keep that distinction in the retained results. These checks do not
establish a new physical-iPad regression or an accepted fix. All production
input changes and temporary test drivers are removed. Evidence, the small UIKit
reproduction and rejected patches are under `artifacts/apple-header-edge-v1/`.
Do not automatically repeat full editor workflows or rejected window-input
experiments. Continue remaining feature/menu gaps with focused shared, bridge and
component checks; return to this native issue with a concrete new hypothesis or
grouped physical acceptance.

Complete feature/menu coverage, windowed iPad input, physical Pencil/keyboard,
provider/interruption and sustained Mac 90 Hz / iPad 120 Hz acceptance remain
open. The completed editor changes, integration fixes and retained acceptance
evidence form one major milestone. Physical workload processes are closed and
device cleanup is complete. The overall goal is **incomplete**. The subsequent
input-control milestone closes effect pickup/history and stationary lasso gaps.
Continue the remaining native feature/menu inventory and full freehand lasso
workflows; keep physical keyboard/Pencil acceptance distinct from simulator results.

## Retained renderer and committed recovery

This milestone follows published column-stack commit `7d5d59b` and integrates
shared main through `9c89f7a`, including Android header transparency and the new
Paint arrangement. Paint keeps Color/Diagnostics, Properties/Filters and Layers
in its outer right column. Tool Set, Tool/Brush size and Navigator occupy the
closed secondary strip immediately inward. All eleven recovery stashes remain.

Both Apple hosts suspend and replace failed GPUs through the shared session API.
Device loss, uncaptured validation errors and rendering failures retain the CPU
document, embedded sources, committed rasters, history and working settings. An
unfinished contact is cancelled. Restart Canvas reconstructs that session;
Save As remains available. Device-specific callbacks cannot stop a replacement,
and thumbnail/filter-preview generations reset. A nonblocking owner check
observes failure after the display link goes idle. Healthy surface attachment
retains its GPU. Direct renderer assignment and the deprecated input-only
recovery barrier are removed.

Recovery preparation submits queued pen-up without a drawable and validates a
committed snapshot. Active ink can retain its preceding committed pixels. The
complete store barrier waits for preferences, workspace writes, the project
worker and atomic recovery-manifest publication. Manual save shares preparation;
recovery can proceed during a pending file request. The existing project codec
and atomic recovery writer are reused.

Native testing found two additional defects. Replacement GPUs remained behind
a catalog-startup gate already completed by the retired device, leaving their
thumbnails pending indefinitely. Startup now completes for each device using
the retained catalog. Mac relaunch also kept the visible scene identifier while
opening a different workspace owner. Editor construction now waits for resolved
scene storage, with its identity boundary inside a stable container.

The integrated Apple/host/UI/workspace suite passes 528 tests, with one existing
host benchmark ignored. Hardware renderer iteration 6 covers both Apple presets
across explicit suspension, real device destruction and uncaptured validation:
cancelled contact, exact source/raster reconstruction, saving while stopped,
pending Save retention, settings/camera, stale callbacks/releases, Undo/Redo and
later painting. Real-owner recovery iteration 4 passes both presets, including
queued ink/pen-up with no following drawable and durable atomic recovery.
The updated column-stack persistence fixture passes both presets with the new
Paint default, customized stacks/width/preferences, switching and history.

Both signed iteration-12 builds pass. Native iteration 12 passes five workflows
per host with no failures or skips: renderer/thumbnail recovery, completed-copy
relaunch, column stacks, Metal/layer controls (with Mac mouse drawing), and the
new Paint geometry/opening/relaunch check. Mac restores Paint in the same scene
without a switch action. The iPad run creates a new scene and opens the retained
Paint workspace through the normal switcher. These do not establish the complete
system window/scene restoration matrix. Reviewed recovery captures show restored
blue thumbnails and transparent checkerboards. Paint captures show the revised
columns; the short top Color group requires scrolling, so verify shared fit/scroll
behavior during the remaining full-editor visual review.

Earlier failures remain in the evidence: early raster/fixture readiness checks,
landscape screenshot cropping and foreground loss, asynchronous width assertions,
the real startup-gate failure, the new Paint fixture's incorrect native command
field, and the Mac scene-owner mismatch. Full iPad app inventory queries can time
out after successful tests. Scoped per-bundle queries restore the review session
and verify the artist app descriptor unchanged. Use
`native-v12/ipad-scoped2-apps-after.json` as the current device baseline.

Evidence is ignored under `artifacts/apple-renderer-recovery-v1/`. Physical
background expiration, interrupted provider access, the complete feature and
window/surface inventory, physical Pencil/keyboard coverage, full-editor visual
parity and sustained Mac 90 Hz/iPad 120 Hz workloads remain open. The overall
goal is **incomplete**.

## Shared column stacks

This milestone follows published title-bar commit `9996f51` and integrates shared
main through `e46f271`, including raster failure handling and diagnostics
visibility. All eleven recovery stashes are retained; the latest preserves the
column-stack work before that integration.

Both Apple hosts now open complete stack members with the ordinary SwiftUI dock
groups and split dividers. Rust owns membership, targets, layout, preferences
and history. Existing drawer connectors join every active group's sidebar icon
to its open column. Grip menus expose Open individual panels, Auto-hide and
Apply to all columns. Closed multi-member stacks have no resize source; open
members retain their own canvas-facing width handle. Fresh Paint opens its right
stack. Saved custom stacks start closed. The obsolete Apple drawer fallback,
shared opt-in method and always-true platform guards are removed. Header and
sidebar buttons share one joined-edge shape.

The integrated Apple/core/host/UI suite passes 487 tests, with one existing host
benchmark ignored. Native ABI checks cover both Apple presets, ordinary group
publication, one-step stack history, compact drawers, and a hardware-GPU
auto-hide contact test: the entire dismissing down/move/up contact leaves pixels
unchanged; the next contact paints and document Undo restores the original.
Popup and nested-drawer facts preserve the open column. The incoming diagnostics
test also covers visibility and renderer replacement on both Apple presets.

AppKit input iteration 4 passes eight checks across mouse/tablet contacts and
both presets. It covers immediate grips/tabs, held icons, panel/group/toolbar
member insertion, the trailing group target, member switching, fixed closed
width, resizing, focus cancellation, late releases and exact Undo/Redo. The
two real-file persistence checks pass fresh Paint defaults, workspace switching,
relaunch, membership/preferences/width, working brush values and persisted
Undo/Redo; open-state changes never persist. Initial fixture failures (stale
group IDs, missing native event loop/viewport and incomplete Chrome facts) remain
in the evidence and are not counted as passes.

Both signed iteration-3 builds pass. Native iteration 3 passes three tests on
each host with no failures or skips: light/dark stack pickup, member switching,
open-member resize and Undo/Redo, grip preferences, auto-hide and compact drawers;
plus Metal launch/layer controls and mouse drawing/document Undo/Redo on Mac.
The iPad's final app query timed out after successful tests; a scoped retry
restored its review namespace and verified the artist app descriptor unchanged.
The current device descriptor baseline is the iteration-3 retry result. Files
no longer requires authentication. Web Wasm checking passes with Homebrew LLVM;
the first check used Apple's clang, which does not support the Wasm C target.

All four native light/dark captures were inspected at normal size. The open
member's standard groups, selected icons, connectors, gaps and resized width are
readable on both hosts. These isolated workspace fixtures do not establish the
complete managed-editor/native-Web visual gate.

Evidence is ignored under `artifacts/apple-column-stacks-v1/` and the integration
snapshot under `artifacts/apple-main-integration-e46f271/`. The overall goal is
**incomplete**: the complete feature/menu/panel inventory, retained renderer
replacement and durable lifecycle recovery, provider/interruption workflows,
physical Pencil/keyboard coverage, complete visual comparison and sustained
Mac 90 Hz/iPad 120 Hz workloads remain open.

## Shared title bar milestone

This milestone integrates shared main through `1c83a95`, following `9d67041`
(Files completion and raster integration). All ten recovery stashes are retained.
The final pulls change Android header rendering, Android/Windows validation
and documentation; the Apple and shared Rust source validated against
`ba77f9b` is unaffected.

Apple now uses the same typed title-bar model and native drag protocol introduced
by the Android port. Rust owns projection, geometry, overflow, frozen drag,
validation and history. AppKit/UIKit own timing, slop and contact capture. Apple
resolves and applies each release action together on its serial render owner.
The fixed header, retired Zen-button fallback, separate clock-visibility view,
unused switcher sizing mode and duplicate Main Menu assembly are removed.
Mac preserves OS application menus and portable saved arrangements. Fresh Sketch
uses individual header tools without supporting toolbar bands; the existing
untouched-default update policy preserves working values and edited histories.

The editor includes all three sizes, footer choice, keyboard selection/movement,
drag-only bank components, shared tool-picker destinations, editable hidden-item
overflow and drawer anchors. Native testing found and fixed two product defects:
clock measurement must match its monospaced digits so minute ticks cannot cancel
a drag, and nested popup sources must preserve child requests under an inactive
context-menu wrapper. Native fullscreen observation also uses scene/display
geometry so the iPad's full-display scene exposes Clock/Battery correctly.

After integration, 483 Apple/core/host/UI tests pass with one existing host
benchmark ignored; the workspace default-update test also passes on both Apple
presets. Pixel tests wait for shared asynchronous raster restoration before exact
comparisons, retaining individual frames for active-input checks. Web Wasm
checking passes. Native AppKit fixture iteration 9 passes both presets at all
sizes: inert bank clicks/holds, immediate mouse/pen pickup, held context,
same-contact movement, detach/re-entry, minute changes, cancellation, keyboard
movement, one-step history and actual narrow overflow popup drags. Six managed
persistence cases pass switching, unfinished-preview close/restart and persisted
Undo/Redo. These AppKit contacts do not establish physical Pencil coverage.

Both signed iteration-65 builds pass. Native iteration 64 passes Metal,
customization and fullscreen status on each host, including iPad Main Menu →
File → Recovered Drawings and Main Menu → Window → Customize Title Bar.
Iteration 65 corrects the new drawer test's stale button names and passes fresh
Sketch Color/Brush/Layers switching and toggling on both hosts. The iPad review
namespace is restored and the artist app descriptor is unchanged. Files remains
free of authentication prompts.

Capture review caught the Mac brush cursor remaining beneath transparent title
bar controls. The unchanged failure repeats in iteration 66; an isolated full
SwiftUI editor capture is clean. Mac now checks the native hit target before
forwarding idle hover, preserving active-contact completion. Signed build 68 and
its drawer workflow pass; reviewed captures retain the textured cursor over the
canvas and clear it over Layers. Native iteration 69 then passes Mac Metal
launch, mouse drawing and exact document Undo/Redo with no failures or skips.

The visual matrix contains 144 native and 144 Web captures. Each native item
matches shared allocation within 0.5 logical pixels. Matching menu-label spacing
fixes premature Apple compaction, and removing Web's obsolete narrow switcher
rule eliminates mismatched overflow. The comparison has no unmatched items and
a largest geometry difference of 4.14 logical pixels (clock padding/measurement).
Native contrast, selected backgrounds and live paint icons were inspected at
normal size. Web retains solid tile backgrounds while GTK/Apple use transparent
controls: this is diagnostic evidence, not full visual acceptance. Web menu and
workspace-switcher interaction checks pass. Native capture iteration 6 has
identical header geometry and PNG bytes across all 144 fixtures after the shared
API/default integration; Sketch now has no fallback toolbar bands or footer.

Evidence and failed iterations stay ignored under `artifacts/apple-titlebar-v1/`.
Integration snapshots remain under `artifacts/apple-main-integration-6ddfb38/`,
`artifacts/apple-main-integration-23bc780/` and
`artifacts/apple-main-integration-ba77f9b/`. Remaining goal work includes the full
feature/menu/panel inventory, retained renderer replacement
and durable lifecycle recovery, provider/interruption checks, physical Pencil
and keyboard input, full-editor visual review and sustained Mac 90 Hz/iPad
120 Hz workloads with current encoded-sRGB8 raster baselines.

## Native Files and raster integration

Local main includes `39a772c`: shared title-bar editing, per-workspace kernel
locks, immutable raster projects and the Web capture follow-up. All seven
recovery stashes are preserved. This work is grouped for one Apple milestone.

Native Files exposed a real callback race: SwiftUI dismissed the picker sheet
before UIKit delivered the selected URL. The sheet's cancellation handler cleared
the pending operation, so Open ignored the valid selection and Save could deliver
a file without acknowledging its location. Both Apple file services now finish
through the document-picker delegate and share one native picker adapter.
Temporary callback instrumentation is removed. Evidence remains ignored under
`artifacts/apple-native-files-v1/`. Native roundtrip iteration 43 established
the fix before raster integration; iteration 50 repeats it with the new format.
The fresh process restores all three layers and artwork, Files contains the
generated PNG, and the editable project title remains unchanged. Files no longer
requires authentication. Its hidden non-button Cancel element is excluded from
native test queries. Both file services use normal delegate cancellation.

Apple now follows shared save/recovery policy during drawing: capture the last
committed raster boundary while active ink remains dirty. Opening still waits
for the contact. Exact raster samples, pressure/tilt/twist corrections, mask
coverage, fresh-GPU restoration and Undo/Redo pass on both Apple presets.
Tests compare loaded content instead of process-local raster identities and
wait for completed input when capture pressure defers pen-up. No stroke-history
reader or old project codec is retained. Apple also removes the retired partial
Zen projection and guards; full Zen/Tab and Change icon remain covered.

Against `c6a6587`, 469 Apple/host/core/UI regressions pass with one existing host
benchmark ignored. The Swift file-service fixture passes both Apple presets.
Both signed iteration-50 builds pass. Native iteration 50 passes five Mac and
six iPad workflows, with no failures or skips: Metal launch/drawing as supported,
artwork recovery, New/export cancellation, workspace order/pin persistence and
full Zen/Tab, plus the iPad Files roundtrip. The review namespace is restored
and the artist app descriptor is unchanged. The iteration-51 runner adds an
opt-in fixture cleanup check; its production executable is unchanged.
The generated Files folder is removed by iteration 51, with the review app
restored. Final `39a772c` integration passes 113 Apple/host/core checks with one
existing benchmark ignored; the unchanged UI suite's 356 passes remain applicable.
Both signed iteration-52 builds pass. Each host passes its final native Metal
launch/layer workflow, including mouse drawing and Undo/Redo on Mac, with no
failures or skips. The current iPad review app and runner are installed, its
review namespace is restored and the artist descriptor is unchanged. Evidence
is under the matching main-integration folders and `apple-native-files-v1/`.

The shared title-bar projection is now in progress as recorded above. Full feature,
provider/interruption, visual, physical-input and sustained-performance acceptance
remains incomplete. The new encoded-sRGB8 raster format also requires current
visual and performance baselines; older linear8 filter evidence is historical.

## Native file-dialog cancellation

- This earlier cancellation checkpoint followed published milestone `f60ae15`.
  The Files callback and raster integration above supersede its source baseline.
- The original New/Export cancellation workflow now passes on both native hosts
  with no failures or skips. It rejects invalid dimensions, creates a 63×47
  drawing, cancels the native export picker and retains the drawing. The iPad
  also closes the editor afterward, confirming the file operation released it.
- Files exposes a non-button element named Cancel ahead of its real close
  buttons. The old generic query selected that element with unusable bounds;
  the corrected test selects a native button. Production picker code is unchanged.
  An authenticated diagnostic also confirms cancellation re-enables Export.
- Both signed iteration-32 builds pass, with production executables identical
  to iteration 29. Temporary probe source is removed. The current test runner
  is installed, the iPad review namespace is restored and the artist app
  descriptor is unchanged. Evidence and earlier failures remain under
  `artifacts/apple-export-cancel-v1/`; final results are in `validation-v32/`.
- Authentication is no longer a test blocker. Use existing isolated dialog
  fixtures for routine file regressions and real picker checks for native
  acceptance or presentation changes; avoid unnecessary authentication retries.
  The full provider/interruption, feature, visual and performance gates remain open.

## Native docking validation

- Milestone `d520d49` groups the docking fixes with their native validation.
  Main advanced before publication; the clean integration now includes
  `688fd76`, preserving both branches and every recovery stash.
- Both hosts pass the drawer-tab reorder, tear-off, redock and group tear-off
  workflow, collapsed/nested drawers, and panel configuration followed by a live
  group drag. The iPad also passes held toolbar tiles with exact Undo/Redo and
  attached-column width/split resizing with history.
- The first Mac drawer test failed because successive injected mouse-down
  events reused event number zero. The native adapter incorrectly retained the
  preceding collapsed icon's hold policy for the next drawer-tab contact.
  It now identifies the mouse-down event object shared by pan and press.
  An unreachable event-type branch is removed; tablet subtype still distinguishes
  pen from mouse. Shared Rust movement, docking and history are unchanged.
- Panel-control tests scope the duplicate Color swatch to its popup and locate
  Brush Size's containing group through its visible tab, replacing a stale
  preset group ID. No product UI workaround is added. Native input fixtures use
  the shared popup host and permit only sub-millionth-point event round-trip
  rounding while keeping device, button, window and event checks.
- Native AppKit workspace checks pass on both Apple presets with increasing and
  repeated zero event numbers. Mouse and pen drawer-tab drags include continuous
  movement and exact Undo/Redo. Earlier full-editor and real-canvas isolation
  checks also pass. These injected contacts do not establish physical Pencil
  coverage. Both signed iteration-28 builds pass; the iPad app executable is
  identical to the installed iteration-23 review app.
- Native layer and workspace-list regressions also pass on both presets,
  including pen holds, immediate mouse rows/grips, history, keyboard menus,
  scrolling, focus loss and source removal. The workspace-list fixture now mounts
  the shared popup host; its earlier keyboard failure remains recorded.
- The final signed Mac batch passes all four workflows with no failures or
  skips: drawer docking, panel configuration/live dragging, collapsed/nested
  drawers and workspace-switcher order/pin persistence across restart.
- Evidence, original failures and temporary diagnostic cleanup are recorded in
  `artifacts/apple-docking-workflows-v1/checkpoint.json`. The review namespace is
  restored and the artist app descriptor remains unchanged. After the user
  opened Files, the regenerated export test reached the visible picker without
  an authentication prompt. Its Cancel tap did not dismiss the picker; XCTest
  reported no usable hit point for that remote element. Cancellation remains
  unverified. See `files-unlocked-v28/` for the failure and restored review state;
  no further Files-unlock confirmation is pending.
- Incoming shared GPU uploads now return mapping failures to the host; Apple's
  blank presentation forwards those errors. Both signed iteration-29 builds
  pass. Mac mouse drawing/Undo/Redo/layers and iPad Metal launch/layers each
  pass with no failures or skips. The new review app and runner are installed,
  the review namespace is restored and the artist descriptor is unchanged.
- The integrated regression has 529 passes and 19 existing hardware/benchmark
  skips. Its one strict filter-reference failure remains: all decoded input,
  output and reference pixels, plus the per-case error table, exactly match the
  retained Metal baseline. No reference or tolerance is changed. Integration
  evidence is under `artifacts/apple-main-integration-688fd76/`; final native
  results and installed descriptors are under `integrated-v29/` in the docking
  evidence folder. Overall acceptance remains incomplete.

## Workspace recovery and editor workflow milestone

- The Color milestone is published as `6c36d0a`. The periodic pull now includes
  `19d6722`; all integrations preserve working paths and every recovery stash.
  Milestone `072c7b7` adds the lifecycle and feature fixes below. The integrated
  upstream code keeps command icons steady during strokes and adds shared CPU
  input retirement for renderer failure.
- Quick restart tests exposed a shared native ownership bug: a killed process
  left Illustrator claimed, so restart selected Painter and hid the expected
  Color/Layers panels. The initial connection-lifetime lock has since been
  replaced by per-workspace kernel locks, preserving live owners through missed
  heartbeats and reclaiming dead owners while other clients remain open. See
  [native ownership](workspace-ownership.md) for lifecycle, protocol and tests.
  This later shared change has Linux acceptance; it does not imply a new Apple
  device acceptance run.
- Included workspace Layout History now permits restoring an earlier layout.
  Its names and deletion remain protected, as do competing owners and busy or
  current history entries. Both native hosts pass the history workflow.
- Curve point and gradient stop counts are included in their native accessibility
  labels. Separate value attributes were not exposed on either host and were
  removed; no custom accessibility wrapper or gesture change was needed.
  Both hosts pass filter search, previews, numeric edits, curve insertion/reset
  and gradient insertion/position/reset.
- All six broader feature workflows pass on both hosts across the retained
  runs. The initial iPad shortcut test typed “Zen” but the search field contained
  “Zn”. Shortcut, settings and filter searches now reuse the existing local-draft
  text field to avoid replacing newer input with delayed
  snapshots. The old workspace-specific helper is removed. Both hosts pass
  complete query entry, conflicting shortcut capture/replacement and editor
  activation; toolbar editing also passes through the same helper.
- Integrated shared regression passes 476 tests: 42 Apple, 25 host, 343 UI and
  66 workspace checks, with one existing hardware-only host check ignored.
  Workspace coverage includes a real child-process kill, live-owner exclusion,
  saved contents, successor fencing, built-in history and protected backup paths.
  Both signed iteration-21 builds pass. Each host passes all six integrated
  native workflows with no failures or skips: Color, filters, shortcuts, toolbar
  editing, restart and history. All six fresh live Color checks pass; guide error
  is at most one channel level and field error is zero. Normal-size inspection
  shows the accepted Color layout retained. The final command-presentation pull
  also passes the 410 Apple/host/UI regressions and both signed iteration-22
  builds. Mac mouse drawing/Undo/Redo/layers and iPad Metal launch/layers each
  pass their native follow-up with no failures or skips. The final integration
  through `19d6722` passes 411 Apple/host/UI checks and the focused input-retirement
  regression, both signed iteration-23 builds and the same two native follow-ups.
- Mac passes all four lifecycle workflows: settings/workspace restart, artwork
  recovery, independent windows and New/Export cancellation. The iPad passes
  the first three. The later unlocked Files follow-up reaches the picker but
  still fails cancellation, as recorded above. All initial failures remain
  recorded.
- Installed-device XCTest uses `UseDestinationArtifacts` without local
  `DependentProductPaths` or bundle paths. Original `app.launch()` works,
  including terminate/relaunch. The unsuccessful explicit-bundle experiment and
  obsolete Color attach branch are removed. See the Apple README for setup.
- The iteration-23 iPad review app and runner are installed, its saved review
  namespace is restored and the artist descriptor remains unchanged. Earlier
  post-test process-query timeouts are retained with their verified recovery.
  No component-app exchange was needed. Old iteration-17 picker scripts must be
  regenerated against current validated products and installed descriptors.
- Evidence: `artifacts/apple-lifecycle-workflows-v1/checkpoint.json`,
  `artifacts/apple-feature-workflows-v1/checkpoint.json`,
  `artifacts/apple-main-integration-b56bca3/` and
  `artifacts/apple-main-integration-65a9855/` and
  `artifacts/apple-main-integration-19d6722/`. The full feature, visual,
  physical-input, lifecycle/expiration and sustained-performance gates remain open.

## Compact Color panel milestone

- `main` includes the periodic integration through `b8ba188`. All 29 existing
  working paths were preserved, with only the expected shared UI export merge.
  The exact recovery stash remains under
  `artifacts/apple-main-integration-b8ba188/`; do not pop or drop it or earlier
  recovery stashes. Publish completed major milestones, not individual fixes.
- The user approved testing on the connected iPad, including the pending review
  update/restart. That approval is resolved; do not ask again for the same testing.
  The user also clarified that visual acceptance means perceptual parity at
  normal viewing size. Imperceptible pixel differences are acceptable. Fix
  visible mismatches and simple refinements; prioritize shared, simple code and
  removal of dead, deprecated or unnecessary paths over exact PNG identity.
- Both hosts use the shared compact Okhsv circle, HSV square and HLS triangle,
  overlapping paint swatches, transparent paint, Swap, shape icons and curved
  shape/RGB readouts. Rust owns geometry, picking, conversion and readout text.
  Field and guide caches retain separate shared RGBA8 images; native clips,
  markers and buttons remain at display resolution. The old HLS-only bridge and
  cache are removed. Header and curved-readout metrics share one helper.
- Wheel painting uses panel coordinates and Web's rounded destination edges.
  Swatch selection borders sit behind paint. Each styled swatch has an explicit
  circular hit region: the first physical iPad editor test showed foreground
  corners intercepting a tap on the visible background swatch. The one-modifier
  correction passes the unchanged workflow on both hosts.
- Both signed Debug builds pass as version 14 against current main. The shared
  regression passes 42 Apple bridge, 25 host and 340 UI tests (407 total), with
  one existing hardware-only host check ignored. The Metal-analysis checks pass
  all 22 tests. No production drawing-performance change is included.
- Final Mac and physical iPad editor workflows each pass with no skips. They
  cover all three shapes, readout switching without changing paint, paint slots,
  transparency, Swap, continuous contacts and empty-corner behavior. All six
  live captures pass the unchanged color oracle: guide error at most one channel
  level, field error zero. The earlier failing swatch attempt remains recorded.
- Both 216-case component matrices pass the unchanged two-level color tolerance
  (guide maximum one, field maximum two). UIKit and AppKit frames agree exactly;
  Chrome differs by at most 0.00521 points. Both themes, presets, all shapes,
  readouts, paint slots and 128/160/226-point widths are covered. The final hit
  shape changes input only; the validated drawing paths are unchanged.
- Eighteen native Mac/Web hover, press and cancellation captures retain identical
  geometry and pass cancellation checks. Keyboard focus coverage is not inferred
  from those captures. The standard Mac-only `drawingGroup()` before the two
  shape-icon rotations improves every full panel; applying it on UIKit regressed
  its results and was rejected. No custom icon renderer was added.
- Full comparisons retain text, edge and compositing differences. Normal-size
  inspection, including the highest-mean-error UIKit case, shows no material
  layout or color mismatch. Mean channel error across the matrices is 0.619 on
  Mac and 0.859 on iPad, on a 0–255 scale; no exact PNG matches are claimed.
  Generic primitive substitutions changed at most one channel level in 19 pixels
  per probe cell and were rejected. A further compositing probe was prepared but
  not run after the user's simplicity clarification. Full-editor visual acceptance
  remains separate from this Color milestone.
- The final review app and test runner are installed on the iPad; the review
  namespace is restored and the artist app descriptor is unchanged. Temporary
  component apps are removed. Use fresh isolated namespaces for future tests.
  Keep device, signing and storage details in ignored artifacts.
- Final integrated builds, results, live color checks and deployment state are
  under `artifacts/apple-color-milestone-v1/`. The swatch diagnosis and first
  successful device workflow are under `artifacts/apple-color-device-final-v1/`.
  Original component captures, the portable `color-review.html` and earlier
  failures remain under `artifacts/apple-compact-color-v1/`. See the milestone
  checkpoint before continuing; retain all underlying evidence.

## Latest editor milestone

- Branch `main`. The shared flat menus, attached column panels and icon integration
  are grouped into one major milestone. Preserve any later working-tree changes.
- The Edit trigger is fixed with a full label hit shape. `ViewThatFits` is retained.
  Popup content now inherits the root's current theme; copying the invoking
  control's complete environment had made dark-menu text unreadable.
- The embedded UIKit context source also transfers only appearance and enabled
  state. This restores Zen accessibility on the connected iPad. Existing UIKit
  hold/pan recognizers retain the original contact. `EditorActionMenu`,
  `EditorMenuButton` and `editorPopover` share vertical, opaque presentation.
  No native context-menu/drag-session handoff or extra popup window remains.
- Menus reuse native keyboard capture, with shared navigation/actions and UIKit
  responder restoration. macOS retains its OS application menu bar and native
  secondary-click adapter. Its Select All action forwards to the focused text
  editor before canvas dispatch. Toolbar grip labels include their toolbar names.

## Verified progress and limits

- Connected iPad: both-theme main menu/Undo; layer/mask/footer anchors/actions;
  upward layer dragging with exact Undo/Redo; submenus, arrow navigation and
  command shortcuts; keyboard routing after dismissal; workspace menu actions
  and held dragging in both directions. Five tests pass together. Zen passes
  separately after its accessibility fix, including hold/tap suppression and
  the resulting Total Zen action. No skips; the earlier failure stays recorded.
- Mac: an isolated editor passes both blend choices and Undo/Redo. Actual native
  events in a separate fixture verify submenu/back navigation, disabled rows,
  Return, Escape and shifted shortcuts. Neither test addresses system-menu coordinates.
- Both hosts pass workspace pin/order persistence, toolbar styles/Zen and full
  toolbar creation/rename/duplicate/delete. Final customization follow-ups verify
  Mac Command-A routing and accessible toolbar names; both tests use native
  Select All text replacement, avoiding caret-dependent backspace counts.
- UIKit callback checks pass touch/pen/mouse classification, edge scrolling,
  hold/lift retention, immediate grips and cancellation. These are not a new
  physical Pencil pass. Attached-column and icon evidence is preserved below.
- Both signed Debug hosts build; 41 Apple bridge, 25 host and 306 UI tests pass,
  with one existing hardware-only host test ignored.
- iPad Escape/Return remain unverified. A first-responder simulator probe received
  a printable key but no XCTest Escape press or key-command callback; the physical
  Escape check also failed, and simulator Return did not execute a menu action.
  Do not add a product workaround just to accept missing synthetic input.
- Exact results and deployment state: `artifacts/apple-flat-menus-v1/checkpoint.json`.
  Earlier column/icon evidence: `artifacts/apple-column-panels-v1/` and
  `artifacts/apple-icons-checkpoint.json`. Raw evidence stays out of Git.

## Continue toward full acceptance

Use the goal tracker's complete inventory and remaining gates. Current performance
still fails sustained presentation cadence: Mac has residual long intervals and
iPad retains drawable-acquisition stalls. The other drawing workloads, calibrated
CPU/GPU/input-latency measurements, complete feature/lifecycle workflows and the
full visual matrix remain open. See [performance evidence](../../apps/layer-apple/PERFORMANCE.md)
and [Apple testing instructions](../../apps/layer-apple/README.md).

The subsequent reserved-drawable-slot experiment is rejected: eight matched
short physical runs show worse cadence on both hosts despite improved iPad CPU
tails. Production code/tests are restored to `232cd2b`; evidence and the next
diagnostic hypothesis are in `artifacts/performance/drawable-reserve-232cd2b/checkpoint.json`.
That experiment restored the iPad review app. The subsequent native diagnostic
temporarily replaced only the isolated review bundle; its process is closed.
After reconnection, the validated review app was restored. The regular artist
apps/data are preserved. The user's new compact-color request then took priority.

The presentation-notification diagnostic does not justify a renderer change.
Corrected Mac compute runs present all 1080 admitted frames in both modes while
GPU completion crosses the CPU deadline. Direct presentation must wait for
command scheduling; the first shared-event fixture did not, and its iPad GPU
timeout is retained as an invalid diagnostic. The corrected iPad executable now
completes all four cases in a separate disposable test identity: each presents
1440/1440 measured frames, with no zero/missing callbacks or GPU errors. Direct
and queued compute modes have 1425 and 1440 qualifying GPU-deadline crossings;
all present successfully. Each run retains one skipped warm-up presentation
outside measurement. This is a native Metal contract check, not application
cadence or drawing-latency acceptance. Its first directory-listing failure was
recovered from the same live process without repeating the measurement. All
diagnostic processes are closed, its app is removed, and the version-9 owned
runner is restored. The artist and version-5 review editors remain unchanged.
All 160 skipped presentations in the three older application iPad display-link
traces had completed CPU owner service at least 3 ms before the deadline;
their actual Metal scheduling times were not recorded. See the new performance
section, `artifacts/performance/metal-notification-2447108-v3/checkpoint.json`, and
the completed iPad `artifacts/performance/metal-notification-ipad-isolated-v1/checkpoint.json`.
The attempted real-viewport observer through `CommandEncoder::as_hal_mut` was
rejected: the actual Mac editor cannot mix normal wgpu encoding with raw encoder
access. Standalone callback tests and successful builds missed this restriction.
Both failed diagnostics are retained; all nine changed source/test files were
restored exactly to the preceding Color milestone state. No iPad deployment or
valid performance measurement occurred. Do not deploy the rejected observer
builds or revive that path; see
`artifacts/performance/viewport-commands-v1/checkpoint.json`. Existing Instruments
captures can supply diagnostic GPU execution observations, but actual unprofiled
viewport scheduling/completion is still missing. Do not infer that a
presentation-backend replacement will fix the retained stalls.

Reanalysis of the earlier real-app Instruments captures now joins all 6 Mac and
122 iPad recorded presentation requests to native frames. Requests and the last
observed GPU completion precede their frame targets, but actual presentation is
one refresh later. The new optional request analysis in `metal_frames.py` rejects
ambiguous identities, multiple requests and invalid ordering; all 22 Metal
analysis checks pass. These are small profiled samples from the earlier
`4a0a808` run, with incomplete capture coverage. They neither explain the current
stalls nor establish input latency; see
`artifacts/performance/retained-command-timeline-v1/checkpoint.json`.

Current-source Release recordings now complete on both hosts in separate
diagnostic identities. All 9 Mac and 97 iPad associated frames fall inside the
measured ink phase. Every captured Mac request/GPU end precedes its target, but
presentation follows two or three refreshes later. On iPad, 95/97 are early;
two late requests coincide with 8.692 ms and 13.197 ms drawable-acquisition stalls.
The requested rolling window retained only 89.236 ms of target GPU work on Mac
and 916.941 ms on iPad, so these are narrow profiled observations, not cadence
or physical-input acceptance. No renderer change is adopted. Earlier reductions
to the drawable pool and admission limit both regressed cadence; do not repeat
them merely because these traces show delay. The current recording checkpoint is
`artifacts/performance/current-command-timeline-v1/checkpoint.json`.

The original current iPad run completed but used an identifier Instruments could
not resolve; the successful follow-up uses the same device's hardware UDID.
An unexpected new disposable-app process was observed after the original closed;
the launch guard stopped before starting another run, and the owned process was
recorded and closed. Its cause remains unknown. One optional export also required
a retry; all failures remain recorded. The diagnostic app is removed and the
version-9 owned runner restored, with both existing editor descriptors unchanged.
The version-5 review editor has still not been updated or restarted.

The earlier authorized SSH pull incorporated the shared color-picker and
GTK/Web refinements; the current revision and color checks are recorded above.
No production presentation-scheduler change is present. These acceptance
notes remain for the next major milestone; do not commit an experiment separately.

Read `AGENTS.md` and the shared drag convention. Keep vertical menus, opaque
shared colors, simple native adapters and regular artist app data. Do not ask
for another speculative physical retry. Keep automation focused and never
coordinate-test the Mac system menu bar; check editor effects directly.

Fetch/integrate other ports periodically and before publishing. Commit and push
only completed major milestones to `main`, using author and committer
**Zack Drach <zackdrach@gmail.com>**. Public HTTPS fetching works; the verified
repository SSH-agent socket is recorded only in the ignored local checkpoint
folder. Publish source/docs only, without device/account identifiers, signing
details, raw logs or captures. No overall-goal completion is claimed here.
