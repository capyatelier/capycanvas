# Apple input

[Capy Canvas for macOS and iPadOS](README.md)

UIKit and AppKit capture numeric samples immediately and enqueue them to the
editor's serial Rust owner. Each record is nine doubles: physical x/y, normalized
pressure, x/y tilt in radians, barrel rotation in radians, distance, monotonic
nanoseconds and phase. The camera revision at capture travels with each batch.
No native event object, device serial or vendor identifier crosses the queue.

Native control focus records how it arrived. Readouts use the shared
`control_takes_key` rule: Enter activates, while Space activates only after
keyboard arrival. Pointer presses clear that arrival state, including on an
already focused control. Unhandled control keys go to the canvas in the same
window; curves forward unhandled keys through the editor input route. Touching
the iPad canvas restores its first responder. The `property-slider-input.swift`
fixture checks pointer and keyboard readout activation; the editor keyboard-focus
journey covers returning from text entry to canvas shortcuts.

## Prediction

Mac uses the shared engine predictor when Enable stroke prediction is on; the
unavailable macOS prediction switch refers only to system-supplied samples. The
manual Prediction amount controls engine lookahead even when the host supplies a
display timestamp. Native iPadOS predictions use presentation timing. Predicted
ink is visual only: real samples replace it and it is never committed beyond the
real endpoint. The cursor tracks real input, so predicted ink can run ahead of it.

## Contacts, cursor and navigation

- **Edge reach.** Workspace markers classify native controls and forward eligible
  contacts through `EdgeReachCoordinator`. Rust decides admission, recognition,
  ownership and catch-up. A claim cancels the canvas contact without blurring
  the editor; pending vertical reaches withhold motion while preserving taps.
  Resize cursors take precedence over the canvas cursor until release.
- **Bubble pickup.** Source contacts keep native scrolling until shared
  `can_take_bubble` admits the current canvas position. UIKit claims the live
  contact on that reply; retired contacts cannot take a later pointer.
- **Interruption.** Both canvas adapters register the shared input-interruption
  callback. It clears the captured contact and modifier state before lifecycle
  blur or renderer restart, so a missing mouse-up cannot block the next press.
- **Buttons.** On iPad a contact keeps the mouse button selected at press time:
  primary paints, right and middle pan, other buttons follow the shared ignore
  policy. Normal and cancelled key releases both end held shortcuts such as
  Space-to-pan.
- **Cursor.** The shared GPU cursor replaces the system pointer over the canvas.
  AppKit shows a blank cursor there and the open hand while panning; leaving the
  canvas restores the arrow unless a contact is captured. iPadOS hides the
  pointer the same way and shows the system pointer while panning.
- **Tilt.** UIKit derives projected XZ/YZ barrel angles from the Pencil's altitude
  and azimuth. `NSEvent.tilt` has its y axis pointing up, so AppKit negates it.
  Shared Rust converts projected angles into a physical inclination and barrel
  direction before camera transforms, brush evaluation and cursor geometry.
- **Hover.** Pencil hover ends on recognizer exit and on cancellation; both send
  the shared cancel phase to clear the cursor. An active Pencil contact
  suppresses hover updates. Pencil and trackpad pointer hover highlight bubble
  items, halves and faces; exit and cancellation clear the highlight.
- **Trackpad and wheel.** UIKit uses standard pan, pinch and rotation
  recognizers with indirect-input support; AppKit uses the canvas view's wheel,
  magnify and rotate callbacks. Scrolling pans, Shift-scroll pans horizontally
  and Control-scroll zooms; pinch and rotation combine around the pointer. Each
  native delta is consumed once, even when cancellation or an active contact
  suppresses it. Rust owns camera scaling; a gesture during paint leaves the
  camera unchanged.

## Estimated Pencil properties

UIKit coalesced touches are real input. `touchesEstimatedPropertiesUpdated`
later corrects previously delivered location, pressure, altitude, azimuth and
roll, as Apple documents in
[estimated properties](https://developer.apple.com/documentation/uikit/uitouch/estimatedproperties).

- `EstimatedInput` keeps each pending observation's numeric fields, scale,
  camera revision and contact, keyed by UIKit's `estimationUpdateIndex`. A later
  callback's timestamp does not prevent a match; the observation keeps its
  original time and phase, and a local token identifies it in the shared engine.
  Partial updates never overwrite resolved fields.
- The C ABI carries optional token and expecting-update pairs beside the samples.
  Corrections bypass pointer and chrome routing, cursor movement and contact
  changes, and amend only a previously admitted painting sample. The engine keeps
  the sample's resolved transform, ruler projection, layer offset and pressure
  curve, so later camera or brush changes do not reinterpret it.
- Unresolved estimates stay in the replaceable brush tail only for the shared
  maximum feedback window; they never hold the whole stroke in preview. Tokens
  outlive that window for later corrections.
- Correcting persistent ink rebuilds the original stroke, including stateful
  pigment and smudge work, and amends the original history point without adding
  an Undo step or discarding Redo. Saved checkpoints that contain corrected data
  get new identities. A correction never overwrites an explicit later edit of the
  same point.
- Retention is bounded (4096 UIKit observations, 8192 engine tokens). On
  overflow the oldest token is released with its last known values. Blur and
  cancellation release the contact's tokens.

## Keyboard and system gestures

AppKit classifies standalone tablet events by type and reads `subtype` only on
mouse events. Keyboard, wheel and gesture events pass through window monitors
without accessing tablet-only event properties.

AppKit forwards the system's key repeats. UIKit reports one press per key, so the
iPad canvas repeats a held non-modifier key itself after 0.4 s and then every
0.1 s while it keeps key focus; release, a modifier change, focus loss and scene
deactivation stop the repeat. Shared Rust decides which actions accept repeats. The canvas opts out
of the system's three-finger editing gestures, so finger taps reach the shared
touch gestures.

AppKit forwards indirect trackpad contacts with stable IDs, native timestamps
and positions in the touch device's point coordinates. Shared Rust recognizes
the [three-finger scrub](../../docs/ui/settings.md#touch-gestures-and-pen-buttons)
and quick inward-pinch fit and reverse-pinch restore on both Apple hosts.
Trackpad contacts support held Undo/Redo but leave taps with the operating
system. They do not duplicate native two-finger pan, pinch and rotation.
Native navigation pauses while three
or more fingers are down and until a recognized scrub's last finger lifts.
System-reserved trackpad gestures must reach the app for recognition to work.

## Text composition

Native marked text takes precedence over shortcut capture and numeric submit,
cancel, stepping and artwork undo. Search and rename key handlers also defer to
native composition. UIKit's numeric field carries the active interface tag in
its accessibility language and text-input context identifier; system keyboard
selection and system font fallback remain native. The SwiftUI locale carries the active tag for interface text and font fallback.
System status formatters and shared numeric strings retain their existing formatting.

## Checks

```sh
xcrun swiftc -parse-as-library apps/layer-apple/Shared/Bridge/EstimatedInput.swift \
  apps/layer-apple/tests/estimated-input.swift -o "$TMPDIR/capy-estimated-input" && "$TMPDIR/capy-estimated-input"
cargo test -p layer-engine
cargo test -p layer-apple estimated_input_abi -- --test-threads=1
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/canvas-native-input.swift
```

The Metal check compares final pixels and stored strokes for G-Pen, Pencil,
watercolor and smudge against input that had final sensor values from the start.
`canvas-native-input.swift` drives the assembled AppKit editor with local mouse,
key, wheel, pinch and rotation events, application suspend and resume, and a
Metal restart. The UIKit fixtures `canvas-hover.swift`, `pencil-estimates.swift`
and `canvas-modifiers.swift` supply recognizer states and touches to the real
iPad callbacks. The AppKit input fixture and UIKit modifier fixture check wheel
navigation while mouse buttons are held, continued contact after scrolling,
cancellation, and exclusion during painting. None of these replace a physical
Pencil or tablet check.

The existing `canvas-modifiers.swift` path checks direct finger quick pinch fit
and restore, three-finger scrub with exact artwork Undo, and repeated held
two-finger Undo and three-finger Redo in both themes. `--pen-response` and
`pencil-estimates.swift` retain their separate pen response and delayed estimate
coverage.

`CAPY_VECTOR_INPUT_ONLY=1` runs the UIKit modifier fixture's vector journeys in
both themes: ink, Move, Properties, Arrange, Path, Edit Points, masks,
Rasterize, save and reopen, with exact pixel and Undo/Redo checks.

AppKit cursor assertions run before yielding after a supplied hover event, since the
system pointer can update the cursor independently of that event.
UIKit file fixtures supply the export-delivery callback for project saves as
well as image exports. Before capturing pixels, they wait for completed input
through the native snapshot barrier; one renderer frame does not guarantee that
queued artwork edits have finished.

Transform contact cancellation restores the pose at the start of that drag;
the fixture then cancels the open transform before checking unchanged pixels.
Transform checks wait for published controls after content measurement, target
the painted bounds, and check X/Y as the reference point's document position.
Ruler fixtures read the new package manifest with `tests/support/PackageManifest.swift`.
