# Apple port handoff

## Goal

Ship complete, visually consistent native iPadOS and macOS apps with fast,
readable workflows, shared maintainable code, and validated drawing performance.

Full acceptance remains in [the Apple goal tracker](../history/apple-acceptance.md#goal):
all exposed features, menus and actions; shared main-editor geometry and canvas
behind the header; iPad 120 Hz and current Mac 90 Hz performance targets. Mac
120 Hz testing is deferred. The overall goal is **incomplete**.

## Current native feature milestone

Prioritize visible Web/Android parity gaps and simple shared solutions. Use the
iPad simulator for routine UI iteration; reserve physical iPad runs for major
milestones and hardware-specific Pencil, provider, lifecycle and performance
acceptance. Avoid repeating passing checks without a relevant change. Commit
only major milestones. All eleven recovery stashes remain.

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

The native integration currently adopts this shared path for curve points.
Gradient-stop dragging still uses individual edits and remains a follow-up;
its native movement/history acceptance is not inferred from the shared cases.
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

Complete feature/menu coverage, windowed iPad input, physical Pencil/keyboard,
provider/interruption and sustained Mac 90 Hz / iPad 120 Hz acceptance remain
open. The completed editor changes, integration fixes and retained acceptance
evidence form one major milestone. Physical workload processes are closed and
device cleanup is complete. The overall goal is **incomplete**. Next feature
work should close native filter-control and lasso gaps with the existing shared
paths; keep physical keyboard/Pencil acceptance distinct from simulator results.

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
