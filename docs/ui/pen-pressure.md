# Pen pressure

[Workspace and UI](README.md)

In **Preferences → Input → Pen response**, **Pen pressure → Adjust…** closes
Preferences and opens a modeless, draggable utility above the canvas. The row
appears immediately above Stroke prediction.
The command is also available in command search and toolbar customization. Its
bold title and inset Close button distinguish it from dockable workspace panels.
It has no tabs, docking targets, workspace persistence or workspace undo entry.
Each client reuses its existing panel surfaces, theme roles and curve widget. The Close
button uses the shared window-close glyph in a 16px icon box, with a compact
squircle hover area and space around it in the title bar.
Its opaque background prevents inner controls from requesting canvas backdrops.

The panel shows the curve, its control polygon, selectable handles and a live
pressure marker. Input is centered below the horizontal axis; Output is centered
beside the vertical axis, rotated sideways. The axes have fixed 0% and 100% labels, without numeric
coordinate fields or changing numeric readouts. Endpoint input coordinates
are fixed, and the final endpoint stays at 100% input and output. Click to add
an interior control, drag to reshape it, and double-click or press Delete to
remove it. Dragging an interior control more than 24 logical pixels outside the
plot removes it immediately during the drag. Further motion cannot move another
control; cancelling the drag or pressing Escape restores it. Endpoints cannot be removed.
Arrow keys move the selected control; Shift increases the step.
Handles have enough inset to remain visible at graph edges.

**Firmer** lowers the starting output by 2.5 percentage points. **Lighter** raises
it by the same amount. Both select the starting control and preserve every
other control. The output stays between zero and the next control's output.
**Reset** restores the built-in soft curve. These actions preview the response
without saving settings. **Apply** saves and closes; **Cancel** and Close discard
the preview. Escape cancels an active editor or panel drag before closing the
panel.

Draw on the ordinary canvas to test the response. Test strokes use normal
artwork history; discarding calibration does not erase those strokes. The engine
captures the curve when each contact starts, including queued contacts. Real
samples, native predictions, corrections and the contact cursor retain that
curve until the contact ends. Changes affect subsequent contacts. Prediction
pressure observations retain the raw sensor pressure.

## Shared model and editor

`layer_core::PressureResponse` stores normalized authored control points. It
validates finite values, bounds, monotone input and output coordinates, and fixed
endpoint inputs. Evaluation uses a midpoint quadratic spline, evaluated by
de Casteljau interpolation. Each interior authored point is a quadratic control;
adjacent segments meet at the midpoint between neighboring controls. The first
segment starts at the first endpoint; the final segment uses the final endpoint
as both control and destination. The joins have matching parametric derivatives.
The underlying segments are [quadratic Bézier curves](https://www.cs.cmu.edu/~sangria/tumble/doc/classQBSpline.html).
Two endpoints without interior controls give a straight line. The input
coordinate is inverted to obtain output for a measured pressure. The algorithm
is shared Rust policy; native clients normalize sensor values and present controls.

The default control polygon is:

```text
(0, .125), (.25, 1), (1, 1)
```

The default has one middle control. The first quadratic segment
ends at the derived midpoint (.625, 1); the final segment is horizontal. It
produces approximately 73.5% output at 25% input, 97.4% at 50%, and 99.9% at
60%, reaching 100% at 62.5%. This is a CSP-inspired approximation, not a claim
about CSP's undocumented spline algorithm. The response matches the reference
graph within about two pixels without fitted extra handles. CELSYS
[confirms a curved iPad default with Wacom stylus mode
disabled](https://www.clip-studio.com/clip_site/support/request/detail/svc/54/tid/99624).
The shape was checked against the [iPad graph shown after resetting app-wide
pen pressure](https://ask.clip-studio.com/en-us/detail?id=81468).

`layer_engine::PressureCurve` compiles the authored response into shared immutable
samples. Interpolation is used only when the interval's output span is at most
0.001; steeper intervals use direct inversion. Monotonicity bounds the lookup
error by that span. Settings and recordings store controls, never compiled tables.

`CurveEditorView` separates evaluated paths, editable handles and a live marker.
Its `CurveControls` provides control guides, inset and optional coordinate readouts. Pressure omits the readouts;
tonal Curves keeps its numeric editing fields. `CurveEditorTarget` routes editor
actions to the owner. Tonal Curves retains its existing interpolating function;
pressure supplies its spline constraints. Native widgets do not evaluate either
algorithm or own document history and calibration transactions.

## Validation

Shared regressions cover quadratic segments and joins, constraints, the default,
steep and vertical responses, settings transactions, contact snapshots and
recording metadata. Run `native_pressure_calibration` through the private GTK compositor
with `--tablet`, in light and dark themes. It checks mouse, touch and pen graph
and frame input, axis label placement, title and Close geometry, painting while
open, launching from Preferences, drag-out removal, live pressure, Apply, Cancel, Close, Reset and Escape. Also run the tonal Curves
journey after changing the shared widget.

Set `LAYER_PRESSURE_MOTION=1` and `LAYER_NATIVE_EVENT_MS=8` for three sustained
panel drags and three curve drags. Panel motion follows a 200 × 100 logical-pixel
ellipse so pixel snapping does not suppress movement at 120 Hz. The JSON report
records actual snapped widget positions and uses native presentation timestamps
inside those observed motion windows. Desktop measurements do not qualify
the reference-tablet performance targets.
The current [GTK diagnostic](../performance/top-tier.md#gtk-pen-pressure-utility)
records the current panel and curve motion measurements.
Set `LAYER_NATIVE_CAPTURE_DIR` to capture the default panel and live pen marker
from the compositor. Keep active-contact captures on this path: the toolkit
snapshot helper replaces the canvas paintable while taking its screenshot.

Web's `--pressure-calibration` journey checks both themes, mouse, touch and pen
contacts, canvas painting, the live marker and saving across reload. Android's
`AndroidHostTest#penPressureCalibrationUsesNativeCurveAndPersistsAppliedResponse`
checks the same controls and native input paths on a reserved tablet. Apple's
`testPressureCalibrationLight` and `testPressureCalibrationDark` cover native
controls on macOS and iPadOS and canvas painting on macOS; they require Xcode and
a supported GPU. Check physical Pencil input separately on an iPad. Windows uses
`exercise-pressure.ps1 -Theme light|dark`; the VM
can check control behavior, while physical GPU and pen performance need hardware.
