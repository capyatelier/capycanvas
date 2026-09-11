# Apple input

UIKit and AppKit capture numeric samples immediately and enqueue them to the
same serial Rust owner. Nine-double records contain physical x/y, normalized
pressure, x/y tilt in radians, barrel rotation in radians, distance, monotonic
nanoseconds and phase. The original camera revision travels with each batch.
Ordinary Mac mouse/tablet input continues through this path; no native event
object, device serial or vendor identifier crosses the queue.

UIKit coalesced observations are real input. Predictions remain visual-only.
`touchesEstimatedPropertiesUpdated` now updates previously delivered Pencil
location, pressure, altitude/azimuth and roll estimates. Apple documents these
as delayed observations, distinct from prediction; only properties still
expecting updates are retained. See Apple's [estimated properties](https://developer.apple.com/documentation/uikit/uitouch/estimatedproperties)
and [update callback](https://developer.apple.com/documentation/uikit/uiresponder/touchesestimatedpropertiesupdated(_:)) documentation.

`EstimatedInput` retains numeric fields, original scale, camera revision and
contact identity. A source index plus original timestamp guards against reused
indices; a local token identifies the shared-engine observation. Partial updates
cannot overwrite already resolved fields. Corrections keep the original time
and phase and survive pen-up. Repeated terminal observations share the token.
Blur releases retained tokens and clears native contact ownership before the
shared interruption policy. Cancellation removes that stroke's estimates.

The C ABI's optional token/expecting pairs leave existing pointer callers
unchanged. Corrections bypass pointer/chrome routing, cursor movement and active
contact changes. Only a previously admitted estimated painting sample can be
amended. The shared engine retains its resolved transform, ruler projection,
layer offset and pressure curve, so later camera or brush-setting changes do
not reinterpret it. Stationary airbrush samples derived from an estimate and
repeated terminal copies follow the same correction.

Pending input stays in the existing replaceable brush tail where possible.
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
cargo test -p layer-engine estimated_
cargo test -p layer-engine repeated_terminal_estimates
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
