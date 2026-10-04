# Apple input

[Capy Canvas for macOS and iPadOS](README.md)

UIKit and AppKit capture numeric samples immediately and enqueue them to the
editor's serial Rust owner. Each record is nine doubles: physical x/y, normalized
pressure, x/y tilt in radians, barrel rotation in radians, distance, monotonic
nanoseconds and phase. The camera revision at capture travels with each batch.
No native event object, device serial or vendor identifier crosses the queue.

## Prediction

Mac uses the shared engine predictor when Enable stroke prediction is on; the
unavailable macOS prediction switch refers only to system-supplied samples. The
manual Prediction amount controls engine lookahead even when the host supplies a
display timestamp. Native iPadOS predictions use presentation timing. Predicted
ink is visual only: real samples replace it and it is never committed beyond the
real endpoint. The cursor tracks real input, so predicted ink can run ahead of it.

## Contacts, cursor and navigation

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
- **Tilt.** UIKit tilt comes from the Pencil's altitude and azimuth and points
  toward the barrel. `NSEvent.tilt` has its y axis pointing up, so AppKit negates
  it, as GTK's macOS backend and Qt do.
- **Hover.** Pencil hover ends on recognizer exit and on cancellation; both send
  the shared cancel phase to clear the cursor. An active Pencil contact
  suppresses hover updates.
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
iPad callbacks. None of these replace a physical Pencil or tablet check.
Ruler fixtures read the new package manifest with `tests/support/PackageManifest.swift`.
