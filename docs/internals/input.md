# Input and stroke feedback

[Technical documentation](../README.md) · [Architecture](../architecture.md)

Pen APIs deliver more than a stream of cursor positions. A callback may include
older samples collected between frames, predicted future positions or corrections
to an earlier estimate. The input layer preserves those distinctions so the shared
engine can produce consistent strokes without discarding useful device data.

## Platform records

The [input contract](../../crates/layer-engine/src/input.rs) represents pen phases,
position, pressure, available angular axes, timestamps and flags. Each adapter
converts its native units into that contract and preserves chronological order.
History delivered with an event is often called *coalesced input*; it is measured
input that should not be confused with prediction.

Input also identifies the camera transform used when it was collected. Queued
samples must not move to a different document position merely because the camera
changed before they were processed. The shared engine converts surface positions
into document coordinates using the appropriate view information.

The platform collects input without waiting for a completed GPU frame. A bounded
queue carries drawing records to the engine owner. On the single-threaded browser
client, the same responsibilities run on one event loop; a thread is not required
by the shared contract. Hosts must explicitly handle queue saturation and gesture
cancellation rather than silently losing a stroke boundary.

## Drawing and navigation

`UiSession` determines whether an interaction belongs to a drawing tool, camera
navigation or a native control. Tool selection, pressure response and navigation
rules remain shared. Native widgets retain their own focus, text editing and
accessibility behavior.

Native control pickup follows the [drag and reorder convention](../ui/drag-and-reorder.md).
Preserve actual device identity: pen requires the touch-style hold before list
reordering, mouse does not, and every device must hold before reordering a tile.
Handles and title/tab bars drag without a hold. These UI rules do not delay
painting, canvas navigation, or direct manipulation controls.

Once a gesture becomes a stroke, the [brush engine](brushes.md) evaluates its
sensors and places dabs. Adapters should not add another layer of brush smoothing
or pressure behavior that changes the brush between platforms.

## Prediction and corrections

Prediction estimates where the pen will be when the next frame is displayed.
The shared [feedback implementation](../../crates/layer-engine/src/feedback.rs)
can use platform predictions where supplied and shared prediction otherwise.
Preferences control this behavior.

Predicted contacts affect a temporary preview, not saved artwork or undo history.
When real samples arrive, the preview is replaced. For brushes that read existing
paint, temporary feedback must also avoid changing committed wetness or carried
paint state.

An estimated-sample correction is different: it updates an earlier observation,
such as Apple Pencil pressure or position that UIKit initially estimated. The
[correction model](../../crates/layer-core/src/input_corrections.rs) preserves sample
identity and allows the affected stroke data to be revised and replayed.

The [detailed feedback reference](../reference/instant-stroke-feedback.md) records
input mappings and acceptance criteria. Physical pen testing remains necessary:
injected pointer events do not establish that a driver supplies real pressure,
tilt, hover or correction records correctly.
