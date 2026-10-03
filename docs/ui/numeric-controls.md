# Numeric controls

[Workspace and UI](README.md)

Every host shares the same touch-first numeric controls.

- Small-range integers: label left, a conventional spin control right. GTK
  uses `GtkSpinButton` in panels and `AdwSpinRow` in Preferences.
- Wider integers and continuous values: label left and a plain, tappable value
  right; a native slider underneath with minus and plus at its ends. Tapping
  the value enters text editing. There are no hidden drag gestures on it.
- In Preferences the complete name/description/value header is above the
  slider. Rows expand for descriptions; the numeric content is capped at
  600 logical pixels. Surrounding groups share the same width so labels align.
  Android uses 48 dp settings targets; GTK and web use 32-pixel settings tracks.
- Titles stay on one line with ellipsis (desktop/web hover shows the full name).
  Values align right and center against the full title/description block.
  Panel sliders use a 24-pixel value row above a matching 24-pixel track/button
  row with no visible thumb (dp on Android). There is a 6-pixel gap between
  each step button and the bar. Panel fill is a theme-aware grey halfway between
  panel background and text. Settings keep larger targets, accent fill and a
  visible thumb. Their values/editors use standard Adwaita input sizing (34px
  high, 9px horizontal padding) on GTK/web; Android keeps 48dp targets with 12dp
  inset. Text editing uses a rounded field sized to the value, not a permanently
  reserved wide input. Descriptions remain wrapped and can increase row height.
  Panel labels have a 6-pixel left inset matching the value's right inset;
  settings preserve the common label alignment with nonnumeric rows.
- Text commits on Enter/IME Done or focus loss. Escape cancels. Invalid
  expressions keep the previous value; typed finite results clamp to hard
  bounds. Refused text stays in the editor with its localized error. Pending
  commits must succeed before step buttons or dialog Apply can proceed. Native
  composition holds text and value until the IME finishes.

## One numeric policy

`layer-ui::NumericControl` owns the control kind, hard and soft bounds, step,
resolution, digits, display scale/unit and range mapping. Its constructor
defaults to a spin control for whole numbers with at most 64 step intervals,
otherwise a slider; definitions can explicitly override the kind. Prediction
time (0–64 ms) uses a slider, with the unit beside the value. Brush size is a
logarithmic slider over 0.5–2048 px. Opacity is displayed as a percentage;
the pressure slider uses a linear range.

`NumericOperation` handles formatting, stepping, native values, normalized
slider positions and typed expressions. Sliders send positions in 0–1.
Mapping, quantization, validation and
formatting happen in Rust; hosts never reproduce those formulas. Buttons
step in the field's units, independently of its slider curve. A signed power
mapping is also supported: exponent 2 on brush diameter means equal increments
of brush area, not quadratic diameter response.

`fasteval` (MIT) evaluates bounded mathematical expressions such as `85/2`,
`sqrt(2)` and `pi`. Unit suffixes are accepted. Nonfinite values, string
literals and diagnostic printing are rejected. The evaluator has no host I/O.
Numeric expressions retain their ASCII number, operator and function syntax;
full-width and compatibility number forms are rejected. Leading and trailing
Unicode whitespace and the field's exact unit suffix, including pressure's ×,
remain accepted. The raw
expression is limited to 256 UTF-8 bytes. Entered text stays literal during
editing. Settings text keeps its strict number grammar and range refusal; it
does not accept expressions or unit suffixes. Search normalization is independent
of numeric parsing.

The resulting number goes through the existing typed app/preference action,
which remains authoritative for availability, dependencies, application and
persistence.

Numeric refusals carry `NumericError`, with literal labels or explicit static
catalog references. The shared UI formats the whole refusal with its active
`Localizer`; successful values, stepping and slider motion do not format error
messages. Workspace admission uses the same validation without a language
context. Storage preserves typed numeric detail until the shared UI presents it.

GTK calls the policy directly; web uses `WebApp.number_input`; Android uses
the stateless `Native.number` JNI call, Apple uses `capy_apple_numeric`, and Windows
uses `capy_number`. Native numeric callers retain the prepared launch language
context; they do not select a language for each request. No GPU handle/lock or UI-state snapshot
is needed to evaluate a number. Hosts own native focus, gesture capture,
unfinished text and transient display state only.

## Properties and Curves

A property scrub uses the existing shared gesture transaction: press begins a
preview, release commits one undo step, and cancellation restores the original
value and redo history. Hosts keep the gesture open through native release
handling. Unchanged model publications preserve unfinished text and focus.

Curves keeps Input and Output in place, blank and disabled until a point is
selected, so selecting a point does not move the graph. Encoded RGB
uses a 0–255 readout with three decimals. Log HDR uses physical linear values,
including small positive values in scientific notation, with a separate EV
readout. Zero displays Black in the EV readout. The shared view supplies exact
editable text; formatting never changes a point. Endpoint Input is read-only.

Arrow keys move a selected point by 1/255 of the graph range; Shift moves it by
10/255. A held key is one edit. Delete removes an interior point, and Escape
cancels its live edit. Point selection and the active channel belong to the
session and do not add undo entries. Stale contacts from a previous page, layer
or document are ignored.

GTK, Web, Android and Windows present the same shared page selector, graph axes
and point fields. Native controls retain unfinished text while the model refreshes; page,
layer and document changes retire the old contact. Android keeps numeric values
as doubles through JNI so HDR coordinates retain the shared field's precision.
Its native numeric draft, selection and edit focus use Compose saved state across
Activity recreation; an in-progress IME composition is still owned by the input
method.

GTK Levels has RGB, Red, Green and Blue pages. Channel stages run before RGB;
the two clipping controls apply to every stage. Floating documents allow input
and output anchors from −65504 to 65504, with the slider concentrated on 0–1.
Input white must exceed black by at least .001, including after Float32 rounding.
Output endpoints may cross to invert the result.
New integer-document adjustments use 0–1 anchor bounds. An adjustment created
with floating ranges retains those ranges when the document depth changes, so
conversion does not silently clamp its existing values.

Auto analyzes the full adjustment input for the selected page. RGB includes the
channel corrections before stretching all three channels together. It changes
input black, input white and gamma as one edit, preserving output anchors and
other pages. Empty, constant or unrepresentable results leave the layer unchanged.
The button becomes Cancel while analysis is pending. Switching pages, hiding
Properties or changing the source retires that result.

Levels and Curves show live input statistics in GTK Properties. RGB displays
the channel-corrected input before master; individual pages display their input
before correction. Curves uses its selected Encoded RGB or Log HDR domain.
Statistics updates retain numerical drafts and focus. The same
[calibration controls](color-picker.md#levels-and-curves) serve both adjustments.
Changing the adjustment's mask, opacity or blend preserves these input
statistics; changes below the adjustment update them.

## Checks

`native_number_controls` (an ignored Wayland widget test) writes a dark/light
review sheet to `artifacts/ui/numeric/`. `native_slider_feedback` sweeps brush
size forward and back through the GTK session: model refreshes do not emit
edits, and deferred GTK range changes compare values at the core's numeric
resolution, avoiding f64/f32 rounding loops. Android instrumented tests cover
native editing, expression evaluation, slider geometry and settings input
isolation. `native_numeric_preedit_guard` covers both GTK widget branches and
step buttons using native preedit signals; actual IME journeys remain separate.
`native_numeric_size_apply_refuses_uncommitted_text` checks Canvas Size and Image
Size Apply in both themes, including refusal without document edits.
Web's `--curves` journey covers graph contacts, page changes, exact readouts and
numeric cancellation. Android's
`AndroidHostTest#curvesPagesNativeContactsAndExactCoordinates` exercises native
contacts and history; `AndroidTextCompositionTest#curveCoordinatesKeepNativeCompositionAndUnchangedPrecision`
checks InputConnection composition and unchanged commits. Windows
`exercise-effects.ps1` covers mouse, pen and touch contacts, double-click
insertion and removal, held arrows, Delete, exact and unchanged Output text,
Log HDR readouts in a float drawing and one Undo per property slider scrub.

References: [GTK Scale](https://docs.gtk.org/gtk4/class.Scale.html),
[Adwaita SpinRow](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.SpinRow.html),
[Compose Slider](https://developer.android.com/develop/ui/compose/components/slider).
