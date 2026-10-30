# Pen tilt

[Workspace and UI](README.md) · [Pen pressure](pen-pressure.md)

Every client exposes **Preferences → Input → Pen response → Pen tilt → Adjust…** directly
after Pen pressure. It closes Preferences and opens the same modeless, draggable
utility used for pressure. Only one pen response editor opens at a time; switching
editors discards the previous draft.

Curve, sensitivity, direction and Reset controls work immediately, including
before any pen has been used. Hover or press the pen anywhere over the
dialog, including the direction circle, to identify the device and enable Apply.
The circle and graph preview the sensor without painting, changing a selection
or moving the canvas. Canvas input also identifies the pen; no stroke is required,
even with the brush cursor hidden.
The caption identifies the pen being adjusted. Edits made before identification
are retained; saved response or direction settings that were not edited remain
intact. An open editor keeps its target when another pen is used; close and reopen
it to adjust the last pen used. Drawing stays available.
First identification preserves the measured window and any active window drag.
The pen contact that identifies the device can edit the curve immediately.
When a curve gesture began before identification, binding waits until that
gesture ends. Cancelling restores the pen's saved response unless earlier
explicit edits are still pending.

The graph maps physical lean from upright, with Input and Output axes from 0° to
90°. The neutral response is a straight line. The origin stays at zero; interior
controls use the pressure editor's add, drag, delete, drag-out removal and keyboard
behavior. The final control moves along the top or right graph boundary. Moving
it rescales the other controls while preserving the normalized curve shape.
**More sensitive** moves it left along the top edge, reaching 90° output earlier.
**Less sensitive** reverses that movement, then moves down the right edge to
reduce output even from the neutral response. Each step changes an endpoint
coordinate by 2.5 percentage points; its minimum is 2.25°.

The direction preview shows input in gray and adjusted tilt in blue. Its 72-unit
circle overlays the graph's bottom-right corner with an 8-unit inset. Below the
graph, **Sensitivity** has joined minus and plus buttons, followed on the same
row by **Direction** and the joined segmented icon bar used by Tool Options.
Each caption stays 4 units from its bar. Sensitivity aligns with the left content
edge and Direction with the right. Spare width goes between the groups, with an
8-unit minimum gap.
Sensitivity buttons retain Less sensitive and More sensitive in tooltips and
accessible labels. The direction bar's independent **Reverse horizontal**, **Reverse vertical** and
**Swap axes** toggles retain their text in tooltips and accessible labels. They
can be combined to represent all eight signed axis mappings.
Swapping happens before the reflections. Direction correction preserves the lean
magnitude; the response curve changes magnitude without changing direction.

Both calibration utilities use a 384-unit width, bounded by the viewport, and 16-unit
content margins. The pen name sits below the title; the title and Close share a
vertical center. Both units use the same square plot size, with equal side gutters
measured from the common maximum-label width in the current font. Sensitivity
and footer actions share the 36-pixel toolbar tile height. Direction segments
have 36-pixel square targets and 16-pixel icons, joined outer corners and the
normal selection fill.
Body groups have 12-pixel gaps; sections and the footer have 16-pixel separation.
Joined direction controls touch, and other adjacent actions have 8-pixel gaps.
Apply is the suggested action.
The utility is shown after shared placement accepts its measured native size.
Like pressure, it opens within the clear canvas with a 16-pixel inset and stays
above expanded workspace columns and drawer connections.

**Reset** previews the neutral response and direction. **Apply** saves this pen's
profile and closes. **Cancel**, Close and Escape discard the draft. Escape first
cancels an active point or frame drag. Test strokes stay in ordinary artwork
history. Calibration itself does not edit artwork or workspace history.

## Shared implementation and coordinate frames

`layer_core::TiltResponse` stores authored controls and reuses `PressureResponse`
for the normalized spline and constraints. `TiltAxes` owns signed permutations,
composition, inverse and frame conversion. `TiltCalibration` stores the response
and direction. Settings keep profiles by stable input-source key.

The shared response editor supplies curve actions, sensitivity, window placement
and view projection for both pressure and tilt. Each host reuses its pressure utility and curve widget. Hosts register `PenDeviceInfo` and forward native samples; they do
not evaluate response curves or own calibration transactions.

`PenDeviceInfo.native_to_surface` describes the stable input frame in current
surface axes. Shared code stores a correction C in the input frame and presents
R C R⁻¹ in surface axes. A horizontal reflection therefore becomes a vertical
reflection after a quarter turn, retaining its physical meaning. All eight
mappings and four quarter-turn frames have headless regression coverage. Canvas
rotation, zoom and mirroring happen after calibration and do not change it.

Native source registration is separate from contact IDs, which may change or be
reused between strokes. An open draft owns its source key and follows that
source's new contacts without following another pen that reuses an ID.

| Client | Saved source key | Calibration frame |
| --- | --- | --- |
| GTK | Tablet vendor/product/name plus pen serial or hardware ID when available | Delivered logical-tablet axes |
| Web | One `web:pen` profile | Browser surface axes |
| Android | `InputDevice.descriptor`, with a shared pen fallback | Native display axes, reprojected using the current display rotation |
| macOS | Proximity unique ID, with vendor/serial fallback | Delivered logical-tablet axes |
| iPadOS | One `apple:pencil` profile | Fixed portrait coordinates converted into canvas coordinates |
| Windows | WinUI pen GUID when available, then product/shared fallback | Delivered surface axes |

Indistinguishable pens share a profile. Browsers and iPadOS do not expose a durable
physical pen identity; browser pointer IDs identify contacts, and
`persistentDeviceId` does not persist across browser sessions. GTK, macOS and
Windows do not provide a verified physical tablet-to-surface rotation mapping
through these adapters. Their correction follows the delivered logical axes;
automatic reprojection after a compositor or tablet-driver mapping change is not
established. Screen dimensions or display orientation must not be used to guess it.

GTK forwards typed native samples directly. Other hosts enqueue the shared
`pen_input` action before ordinary canvas input to register source metadata.
An optional nine-number sample carries x, y, pressure, projected tilt X/Y in
radians, twist, distance, nanosecond time and phase for preview-only sensing.
The action bypasses document-edit admission and never enters the painting,
selection or camera input path. Native adapters observe utility input before
child widgets handle it, so the direction circle and ordinary controls both
identify the pen without requiring a canvas stroke.

`layer_engine::TiltCurve` decodes projected sensor angles to physical radial tilt,
applies the response and direction, then lets the camera map direction into the
document. Each stroke captures its calibration at Down, including input held
before painting becomes ready. Queued samples, native predictions, estimated
corrections and contact cursors retain that calibration. Hover and later contacts
preview new settings immediately. Compiled curves are runtime data only; direction
changes reuse their response tables, and curve edits leave other profiles’ tables
intact.

## Validation

Run the core tilt tests, engine tests and UI tilt, pressure calibration, held
contact and localization tests. Run `native_tilt_calibration` through
`tools/performance/workspace-motion.sh gtk --native-test=native_tilt_calibration --tablet`
in both themes. Run `native_short_tilt_calibration` with
`LAYER_MOTION_VIEWPORT=640x480` through the same harness to check the compact
layout and pen controls on a short screen. The full journey covers Preferences,
mouse/touch/pen controls, native tilt
metadata, editing before first pen input, hover with a hidden cursor, retained
window geometry on first input with Lasso, Pencil and Hand, cancelled unbound
curve gestures, deferred identification during a curve drag, preview-only input
over the dialog and circle, native measurement recovery, retained
drafts, independent icon toggles, tooltips, matched square plots, header alignment,
footer geometry, clear-canvas placement, stacking and opaque pixels above an
expanded column and connector, live arrows, graph and
window dragging, canvas painting, Apply, Reset,
Cancel, Close and Escape. Run the pressure and tonal Curves journeys after changes
to their shared widgets. Set `LAYER_TILT_MOTION=1` and `LAYER_NATIVE_EVENT_MS=8`
for three five-second frame and curve drags with native presentation timestamps.
These desktop diagnostics do not qualify reference-tablet painting performance.

Web's `--tilt-calibration` journey checks both themes, native browser pointer
input, identification over the utility, independent direction controls, matched
pressure geometry, stacking, window dragging and settings across reload.
Android's `AndroidHostTest` contains separate light and dark tilt journeys;
Apple's `testTiltCalibrationLight` and `testTiltCalibrationDark` exercise the
shared SwiftUI utility on macOS and iPadOS. Windows uses
`exercise-tilt.ps1 -Theme light|dark`. Native Apple UI checks require Xcode;
Windows VM checks do not establish physical pen or GPU performance. See each
[platform guide](../platforms/README.md) for the host test commands.
