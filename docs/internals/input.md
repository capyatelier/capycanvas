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

Shared numerical admission drops samples with non-finite coordinates or pen
axes before cursor, tool and recording consumers. A damaged real release
cancels its contact using the last valid sample. Damaged predictions and
corrections leave the real contact intact. The session retains at most sixteen
contacts in storage allocated once when it starts. Packed batches preserve
valid siblings; malformed native structures cancel their matching real contact.
GTK routes a damaged current Move through its latest eligible valid historical
sample and retains historical paint order.

`PenEvent.tilt_radians` contains signed barrel angles projected onto the surface
XZ and YZ planes, as in [Pointer Events](https://www.w3.org/TR/pointerevents/#converting-between-tiltx-tilty-and-altitudeangle-azimuthangle).
Android converts its scalar inclination and tip orientation into those projected
angles; UIKit derives them from altitude and azimuth. Shared `to_stroke_point`
converts the projections into a barrel-direction vector whose length is the
physical inclination from the surface normal, in radians. Camera rotation and
reflection act on that vector without changing its length. Painting, hover,
prediction and estimate corrections share this conversion. Authored `StrokePoint`
tilt retains that vector, so replay does not convert it again. At exactly flat
dual-axis projections, the otherwise ambiguous direction is the signed diagonal.
The [pen tilt editor](../ui/pen-tilt.md) applies per-device response and axis
correction after physical tilt conversion and before the canvas transform.
Each contact captures that calibration with its pressure response.

Input also identifies the camera transform used when it was collected. Queued
samples must not move to a different document position merely because the camera
changed before they were processed. The shared engine converts surface positions
into document coordinates using the appropriate view information.

The platform collects input without waiting for a completed GPU frame. A bounded
queue carries drawing records to the engine owner. On the single-threaded browser
client, the same responsibilities run on one event loop; a thread is not required
by the shared contract. Hosts must explicitly handle queue saturation and gesture
cancellation rather than silently losing a stroke boundary.

Touch timestamps are ordered within each contact. Coalesced histories for
different fingers can overlap in time when a host delivers each pointer's batch
separately. Canvas and layer gesture recognition validate each finger's timestamp
independently and keep the greatest timestamp for group timing. Reversing time
within one contact still cancels recognition.
Hosts can omit unchanged layer-contact positions while preserving every changed
position, press, release and cancellation. Holds use the shared frame clock;
stationary native reports must not fill the command queue.

## Drawing and navigation

`UiSession` determines whether an interaction belongs to a drawing tool, camera
navigation or a native control. Tool selection, pressure response and navigation
rules remain shared. Native widgets retain their own focus, text editing and
accessibility behavior.
The shared [navigation contract](../ui/shared-ui.md#drawing-and-navigation) also
keeps zoom/rotate/pan contacts owned until release, independently of held keys.

Pen side buttons arrive as `pen_button` input, never as pen samples, and do
nothing until the artist binds them ([pen buttons](../ui/settings.md#touch-gestures-and-pen-buttons)).
Pressing or releasing one must not interrupt, finish or restart a tip or eraser
stroke, or start mouse navigation; a bound tool change applies after the stroke
ends. Adapters filter native button transitions before gesture arbitration and
preserve tip and eraser identity. Apple Pencil double-taps and squeezes arrive
the same way as `stylus_action` input, carrying the system's choice
([Pencil gestures](../ui/settings.md#touch-gestures-and-pen-buttons)).

Some pens, such as the Wacom Pro Pen 3 on Android, report the final up event at
the last move's position with nonzero pressure. Adapters keep the raw values;
the brush's pressure-fall limit shapes the lift taper.

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

Each stroke captures its input policy at pen-down. Mouse and confirmed
screenless tablet input have no prediction lead beyond the latest real sample,
while enabled [smoothing preview](brushes.md#stroke-smoothing) still compensates
for the curve's delay. Display pens keep the configured lead. Turning Stroke
prediction off disables both the input lead and smoothing preview; committed
smoothing still applies.

GTK uses Linux tablet properties, Windows uses the native pointer device type,
and Android uses [indirect stylus sources](https://developer.android.com/reference/android/view/InputDevice#SOURCE_MOUSE)
to identify screenless tablets. [Browser Pointer Events](https://www.w3.org/TR/pointerevents/#dom-pointerevent-pointertype)
and [AppKit](https://developer.apple.com/documentation/appkit/nsevent) do not
identify whether a pen tablet has a display; unclassified pens keep the configured
lead. Adapters preserve pressure, tilt and eraser identity for indirect pens.

Predicted contacts affect a temporary preview, not saved artwork or undo history.
When real samples arrive, the preview is replaced. For brushes that read existing
paint, temporary feedback must also avoid changing committed wetness or carried
paint state.

An estimated-sample correction is different: it updates an earlier observation,
such as Apple Pencil pressure or position that UIKit initially estimated. The
[correction model](../../crates/layer-engine/src/corrections.rs) preserves sample
identity and allows the affected stroke data to be revised and replayed.

The [detailed feedback reference](../reference/instant-stroke-feedback.md) records
input mappings and acceptance criteria. Physical pen testing remains necessary:
injected pointer events do not establish that a driver supplies real pressure,
tilt, hover or correction records correctly.
