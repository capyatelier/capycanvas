# Apple input

UIKit and AppKit capture numeric samples immediately and enqueue them to the
same serial Rust owner. Nine-double records contain physical x/y, normalized
pressure, x/y tilt in radians, barrel rotation in radians, distance, monotonic
nanoseconds and phase. The original camera revision travels with each batch.
Ordinary Mac mouse/tablet input continues through this path; no native event
object, device serial or vendor identifier crosses the queue.

Mac uses the shared engine predictor when Enable stroke prediction is on;
the unavailable macOS prediction switch refers only to system-supplied samples.
Manual Prediction amount controls engine lookahead even when the host supplies
a display timestamp. Native iPadOS samples still use presentation timing.
The cursor tracks real input, so predicted ink can extend ahead of it while
moving. Existing turn, speed, distance and pen-lift limits still apply. Predicted
ink is replaced by real samples and is never committed beyond the real endpoint.

Both native canvas adapters register the shared input-interruption callback.
On Mac it clears the captured contact and modifier state before lifecycle blur
or renderer restart, so a missing mouse-up cannot block the next press.
`tests/canvas-native-input.swift` checks the application's suspend/resume entry
points and an actual Metal restart in the assembled AppKit editor, including
stale movement, a fresh lasso and exact artwork/history. Both Apple policies
run on Mac; physical sleep, tablet and UIKit interruption remain separate checks.

The same assembled-editor fixture supplies AppKit wheel, pinch and rotation
values to the real canvas callbacks. It checks precise/coarse scroll units,
Shift/Control behavior, physical anchoring, contact exclusion and navigation
after a cancellation frame, with exact artwork Undo/Redo. These supplied values
do not establish physical trackpad recognition or OS gesture delivery.

On iPad, the contact retains the mouse button selected at press time. Primary
contacts paint, right/middle contacts pan, and other buttons follow the shared
ignore policy. Normal and cancelled keyboard releases both end held shortcuts,
including Space-to-pan.

Pencil hover ends on both normal recognizer exit and cancellation. Both send the
existing shared cancel phase to clear the cursor; an active Pencil contact still
suppresses hover updates. `tests/canvas-hover.swift` runs the actual UIKit callback
and records its accepted native input, covering exit, cancellation, contact
exclusion, fresh hover and unchanged artwork history. Supplied recognizer states
do not establish physical Pencil recognition or rendered cursor appearance.

Trackpad and mouse-wheel navigation uses standard UIKit pan, pinch and rotation
recognizers with the app's existing indirect-input opt-in. They accept scroll
and transform events; finger and Pencil contacts keep their existing routing.
Scrolling pans, Shift-scroll pans horizontally and Control-scroll zooms. Pinch
and rotation can combine around the pointer anchor. Native deltas are consumed
once, including when cancellation or an active contact suppresses them. Rust
owns camera scaling and document-idle checks; a gesture arriving during paint
leaves the camera unchanged without reporting a canvas error. See Apple's
[trackpad input guidance](https://developer.apple.com/videos/play/wwdc2020/10094/).
Focused callback and Metal checks cover routing, camera transforms and exact
artwork/history preservation; physical trackpad/keyboard delivery remains open.

UIKit coalesced observations are real input. Predictions remain visual-only.
`touchesEstimatedPropertiesUpdated` now updates previously delivered Pencil
location, pressure, altitude/azimuth and roll estimates. Apple documents these
as delayed observations, distinct from prediction; only properties still
expecting updates are retained. See Apple's [estimated properties](https://developer.apple.com/documentation/uikit/uitouch/estimatedproperties)
and [update callback](https://developer.apple.com/documentation/uikit/uiresponder/touchesestimatedpropertiesupdated(_:)) documentation.

`EstimatedInput` retains numeric fields, original scale, camera revision and
contact identity. UIKit’s unique, monotonically increasing `estimationUpdateIndex`
identifies the observation, following Apple’s [correlation contract](https://developer.apple.com/documentation/uikit/uitouch/estimationupdateindex).
A later callback’s timestamp must not prevent an update from matching. The saved
observation retains its original timestamp; a local token identifies it in the
shared engine. Partial updates
cannot overwrite already resolved fields. Corrections keep the original time
and phase and survive pen-up. Repeated terminal observations share the token.
`tests/pencil-estimates.swift` exercises the actual UIKit callbacks with changed
callback timestamps, partial/final updates and a retired index. It runs from app
launch without depending on a restored scene. Blur releases retained tokens and clears native contact ownership before the
shared interruption policy. Cancellation removes that stroke's estimates.

The C ABI's optional token/expecting pairs leave existing pointer callers
unchanged. Corrections bypass pointer/chrome routing, cursor movement and active
contact changes. Only a previously admitted estimated painting sample can be
amended. The shared engine retains its resolved transform, ruler projection,
layer offset and pressure curve, so later camera or brush-setting changes do
not reinterpret it. Stationary airbrush samples derived from an estimate and
repeated terminal copies follow the same correction.

Pending input stays in the existing replaceable brush tail for at most the
shared maximum feedback window (50 ms of real input). An unresolved estimate
must not keep the entire growing stroke in the preview. Tokens remain retained
for later corrections after that ink becomes persistent.
Watercolor material-update boundaries follow the original real observations,
independent of sensor latency. Correcting persistent ink rebuilds the original
stroke, including stateful pigment/smudge work. A committed correction amends
the original document/history point without adding an Undo step or discarding
Redo. Existing project snapshots retain their previous immutable point storage.
Saved checkpoints containing corrected data receive new identities; checkpoints
before that stroke remain unchanged. Matching the previous point prevents a
late sensor callback from overwriting an explicit subsequent point edit.

Retention is bounded at 4096 UIKit observations and 8192 engine tokens. On adapter
overflow, the oldest token is released with its last known values; subsequent
updates cannot target a different stroke. The adapter counts expiry and the
engine exposes correction/expiry counters in `EngineMetrics`. This fallback is
not evidence that final sensor data arrived. Physical callback loss, overflow,
orientation changes and the complete interruption matrix still need validation.
Corrections racing explicit edits to point data also require broader workflow
acceptance; the matching guard currently preserves the explicit edit.

Fast checks from the repository root:

```sh
xcrun swiftc apps/layer-apple/Shared/Bridge/EstimatedInput.swift \
  apps/layer-apple/tests/estimated-input.swift -o /tmp/capy-estimated-input
/tmp/capy-estimated-input
cargo test -p layer-core corrections_preserve_history
cargo test -p layer-engine
cargo test -p layer-apple estimated_input_abi -- --test-threads=1
```

The Metal check exercises both Apple policies and compares exact final pixels
and stored strokes against input that had final sensor values from the start.
Its fixtures use G Pen, Pencil, watercolor and smudge over existing artwork,
with partial updates, pen-up correction and exact Undo/Redo. These are synthetic
sensor observations through the real ABI. They do not establish physical Pencil
delivery, palm rejection, tablet sensors or input-to-display latency.

Late corrections to persistent ink currently replay the scene. Cost on long
strokes, 4K multilayer documents and sustained hardware workloads remains open;
no 120 Hz claim follows from these correctness checks. The shared trace analyzer
reports correction receipts separately from ordinary and predicted batches.

## Physical Pencil smoke check

On the attached iPad, the user confirmed light/firm pressure response, strokes
remaining after lifting, drawing with a resting palm and the expected Undo/Redo
result. The four-minute Debug trace includes about nine seconds of pointer
activity: five Pencil contacts with five pen-up batches, alongside hover and
touch activity. It records 730 real pointer batches and 417 prediction batches,
with no recorder overflow or frame errors. No correction batches were observed,
so this session does not validate physical estimated-property delivery. Sensor
ranges, orientation, interruption/recovery and the complete brush matrix still
need physical coverage. The local trace contains no coordinates or artwork.

This short user check establishes the reported basic behavior only. The Debug
timings and uncalibrated presentation receipt proxies do not establish sustained
120 Hz performance or physical input-to-pixel latency.
