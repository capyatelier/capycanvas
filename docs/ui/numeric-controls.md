# Numeric controls

[Workspace and UI](README.md)

Every host shares the same numeric policy and editable values.

Common layer opacity targets the occurrence, including filter layers; effect
parameters use their own keys. Apple sends `EffectAction::LayerOpacity` through
the shared property gesture protocol. Contacts match both the occurrence and
action kind: cancellation restores the occurrence, and release adds one history
entry without changing the effect application.

- Small-range integers: label left, a conventional spin control right. GTK
  uses `GtkSpinButton` in panels and `AdwSpinRow` in Preferences.
  Compact workspace spin controls have a 26-unit base height, with compact step buttons
  and a value field sized from the shared formatted range and units.
  Web and Apple measure that range's shaped text with tabular digits; browser
  input character counts must not choose compact field widths.
- Wider integers and continuous values: label left and a plain, tappable value
  right; a native slider underneath. Tapping the value enters text editing.
  Panel values also accept vertical mouse, touch and pen drags: up increases,
  down decreases, four logical pixels equal one field step, and the value snaps
  as the slider does. Holding Shift during the drag, as in Blender and Krita,
  moves ten times slower and keeps fractions; pressing or releasing Shift
  mid-drag continues from the current value. Escape or native cancellation
  restores the starting value.
  Edit Color numbers adjust by vertical drag without a slider
  ([color picking](color-picker.md#edit-color)).
- Directions that wrap use a dial in a slider row. The label sits at the top
  left and the plain, tappable value at the bottom right, as for a slider; in
  place of the track, a round dial ends where tracks end. The dial is the row's
  height less the 4-unit control spacing above and below. Its face uses the
  track color and a dot in the slider fill grey points toward the angle.
  Pressing or dragging points the dial where the pointer is. Within twice the
  dial's radius it snaps to 15° steps; farther out it turns freely, and a press
  at its center keeps the value. Values wrap at 360°. Double-clicking the label
  or the dial restores the default. Arrow keys and the scroll wheel step a
  focused dial; Shift multiplies arrow steps by ten. Native accessibility
  adjustment uses the same shared numeric operation. Paper's Light is a dial.
- In Preferences the complete name/description/value header is above the
  slider. Rows expand for descriptions; the numeric content is capped at
  600 logical pixels. Surrounding groups share the same width so labels align.
  Android uses 48 dp settings targets; GTK and web use 32-pixel settings tracks.
- Titles stay on one line with ellipsis (desktop/web hover shows the full name).
  Values align right. Settings values center against the full title/description block.
  Stacked panel sliders sit just below the text, within a control with a 36-unit base height.
  Panel captions align with the top of that row, above the slider hit area.
  The 4-pixel track has a symmetric 16-pixel hit area starting just below the
  visible bottom of the label. Its left inset is 36 pixels; its fixed right
  inset reserves room for ordinary values and units: 76 pixels. Panels have no
  step buttons. Panel values and text editors align with the bottom of the
  slider hit area. Windows grows the caption and row for system text scaling,
  preserving the 16-unit track allocation and bottom-aligned editor.
  GTK panel readouts align shaped text within their allocated line width so
  units stay anchored when the digit count changes.
  The panel editor is 68 pixels wide with a 26-unit base height, keeping an 8-pixel gap
  from the slider hit area. Readouts use their text's natural width and retain
  the same right inset. Long expressions scroll within the field.
  Double-clicking a tool setting or property label restores its shared default.
  Workspace panels use the [panel spacing roles](README.md#compact-panel-spacing):
  adjacent independent controls have a 4-pixel gap below that hit area; the
  compound transform X/Y fields keep a 2-pixel gap. Properties uses the same
  4-pixel gaps between independent numeric controls.
  Brush value popovers use this same presentation. Native size, export and
  preview dialogs may retain their larger platform fields within the slider layout.
  Layers opacity, toolbar tiles, view readouts, Curves point
  coordinates, tonal ranges and color controls keep their specialized layouts.
  GTK, Web, Android, Windows, macOS and iPadOS share this panel geometry
  (logical pixels, dp on Android). Preferences retain step buttons and larger
  targets.
  Panel fill is a theme-aware grey halfway between
  panel background and text. Settings keep larger targets, accent fill and a
  visible thumb. Their values/editors use standard Adwaita input sizing (34px
  high, 9px horizontal padding) on GTK/web; Android keeps 48dp targets with 12dp
  inset. Text editing uses a rounded field; panel editors reserve their full
  width while editing, and other fields size to the value. Descriptions remain
  wrapped and can increase row height.
  Workspace labels align with the panel's 8-unit content inset, without another
  label indent. Readouts and text editors use 6-unit horizontal padding, including
  inline fields; settings preserve the common label alignment with nonnumeric rows.
- Text commits on Enter/IME Done or focus loss. Escape cancels. Invalid
  expressions keep the previous value; typed finite results clamp to hard
  bounds. Refused text stays in the editor with its localized error. Pending
  commits must succeed before step buttons or dialog Apply can proceed. Native
  composition holds text and value until the IME finishes.

Number readouts and their text editors accept wheel steps after committing any
pending text. Refused text consumes the wheel without changing the value.
Standalone slider readouts use a normalized vertical drag: 200 logical pixels
cover the slider range. Panel readouts keep the field-step drag described above.
Hosts forward these operations to Rust, including touch and pen contacts, and
cancel unfinished gestures on capture loss, focus loss or control retirement.

## One numeric policy

`layer-ui::NumericControl` owns the control kind, hard and soft bounds, step,
resolution, slider snapping, digits, display scale/unit and range mapping. Its constructor
defaults to a spin control for whole numbers with at most 64 step intervals,
otherwise a slider; definitions can explicitly override the kind. Prediction
time (0–64 ms) uses a slider, with the unit beside the value. Brush size is a
logarithmic slider over 0.5–2048 px. Opacity is displayed as a percentage;
the pressure slider uses a linear range.
Dither's Cell size is a whole-pixel slider from 1 to 50, starting at 4 px, with
Smoothing's normalized logarithmic curve. Small sizes occupy more of its travel.
Dither's Spread uses the same curve over its percentage range. Tool Settings
places Cell size immediately below Spread.
Control schema comparisons include the logarithmic offset as part of the slider curve.

`NumericOperation` handles formatting, stepping, value scrubbing with its
`fine` flag, native values, normalized slider positions, dial points and typed
expressions. Sliders
send positions in 0–1; dials send the pointer's offset from their center and
their radius, and `NumericKind::Dial` wraps every result into its range.
Mapping, quantization, validation and
formatting happen in Rust; hosts never reproduce those formulas. Buttons and
value scrubs operate in the field's units, independently of its slider curve.
A signed power mapping is also supported: exponent 2 on brush diameter means equal increments
of brush area, not quadratic diameter response.

Pixel, percentage and angle sliders snap in displayed units. Pixels keep tenths
through 32 px, matching the brush-size threshold, then snap to whole pixels.
Percentages keep tenths within five percentage points of either soft-range edge
and snap to whole percentages elsewhere. Angles snap to whole degrees.
The canvas rotation slider also follows Snap rotation from its menu: its
slider-origin action applies the shared quarter-turn attraction curve, while
text and step inputs keep their exact values. Native controls with a separate
position handler display the published model value during motion, so a stationary
snap holds both the thumb and the readout at the canvas angle.
Other units retain their declared resolution; whole-count fields stay integers.
Text entry retains fractional values independently of slider snapping. Value
scrubs snap like the slider unless the field declares a `scrub_snap` increment.
Shift scrubs keep fractions, except pixel
magnitudes above 1000 px snap to whole pixels. Sliders and value drags use the
same shared readout during motion and after release: omit trailing fractional
zeroes and retain actual fractions from fine ranges, Shift drags and text entry.
Compact readouts keep at most one fractional digit, including for manually
entered large pixel values.

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
uses `capy_number`. Unchanged slider positions preserve fine text/scrub values
and authored values beyond the soft range in Rust, including f32 model echoes.
Native numeric callers retain the prepared launch language
context; they do not select a language for each request. No GPU handle/lock or UI-state snapshot
is needed to evaluate a number. Hosts own native focus, gesture capture,
unfinished text and transient display state only.

Apple's `NumericTextField` owns native first-responder changes and takes an
ordinary `Binding<Bool>` for focus. Callers keep that value in `@State`;
`@FocusState` bindings belong to SwiftUI controls using `.focused(...)`.

Tool settings with `preview: true` forward Down/Move/Up/Cancel through
`ToolSettingContact`. The shared editor decides which values are previews and
when to commit or restore them. Liquify Amount uses this contract to keep
authored pixels and history unchanged during motion, then reconstruct at full
resolution on release. Other settings keep their existing direct-value editing.

## Properties and Curves

Changing conditional Properties fields retains compatible common widgets, their
keyboard focus and unfinished numeric text. Native buttons and switches consume
their activation keys before canvas shortcuts.

A property scrub uses the existing shared gesture transaction: press begins a
preview, release commits one undo step, and cancellation restores the original
value and redo history. Hosts keep the gesture open through native release
handling. Unchanged model publications preserve unfinished text and focus.
Panel value drags measure native surface coordinates so focus scrolling does not
change the fine adjustment distance. Native keyboard routing retains the active
readout contact so Escape can cancel before release. Losing contact, retiring
the control or disabling it also cancels the preview.

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
or document are ignored. Cancelling a contact or live edit does not make the
graph's queued input stale, so a double-tap whose second contact is cancelled
still removes its point.

GTK, Web, Android, Windows, macOS and iPadOS present the same shared page
selector, graph axes and point fields. Native controls retain unfinished text while the model refreshes; page,
layer and document changes retire the old contact. Android keeps numeric values
as doubles through JNI so HDR coordinates retain the shared field's precision.
Its native numeric draft, selection and edit focus use Compose saved state across
Activity recreation; an in-progress IME composition is still owned by the input
method.

Hue / Saturation uses Master and six hue-range pages. Colorize keeps the same
Hue, Saturation and Lightness positions, replacing the first two with its stored
colorization values. Inactive range pages disappear without losing their values.
Center is a hue angle; Width is the full-strength span and Feather is the falloff
on either side. Threshold uses a soft 0–1 slider with finite extended entry in
floating documents. Existing floating bounds and values survive depth changes.
Photo Filter uses the shared tagged color editor, Density and Preserve luminosity.
Invert and Desaturate apply directly without numeric controls.

Selective Color keeps its Relative/Absolute choice visible on every ink page.
Channel Mixer's Monochrome switch changes between the stored RGB output rows and
the Gray row. Switching pages or modes preserves hidden values and the common
control's keyboard focus. Each scrub remains one undo step.

GTK, Web, Android and Windows Levels have RGB, Red, Green and Blue pages. Channel stages run before RGB;
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

Levels and Curves show live input statistics in GTK, Web, Android and Windows Properties. RGB displays
the channel-corrected input before master; individual pages display their input
before correction. Curves uses its selected Encoded RGB or Log HDR domain.
Statistics updates retain numerical drafts and focus. The same
[calibration controls](color-picker.md#levels-and-curves) serve both adjustments.
These hosts keep channel selection and actions in one row and use compact panel
sliders for ordinary number fields. Curves puts Input and Output in two equal
columns below
its graph, with labels above the fields so translations fit a narrow panel.
Reset and clipping buttons share the status row. Unfinished numbers keep
their editor and focus when statistics or the selected graph point changes.
Changing the adjustment's mask, opacity or blend preserves these input
statistics; changes below the adjustment update them.

## Checks

`native_number_controls` (an ignored Wayland widget test) writes a dark/light
review sheet to `artifacts/ui/numeric/`. `native_slider_feedback` sweeps brush
size forward and back through the GTK session: model refreshes do not emit
edits, and deferred GTK range changes compare values at the core's numeric
fine resolution in Rust before slider snapping, avoiding f64/f32 rounding loops and
preserving fractional text/scrub values and values beyond the soft bounds.
`native_panel_slider_input` checks
native drags along both vertical edges of the panel slider hit area, value
editing with fractions above the pixel threshold, whole-pixel slider snapping
and label double-click reset in both themes, with workspace captures
in `artifacts/ui/panel-sliders/`. `native_panel_value_scrub_input` checks mouse
and touch value scrubs in both themes, whole steps without Shift and tenths
with it, integer and fractional readouts during motion and after release,
precise Escape cancellation, click editing, Properties
label reset and one Undo per
scrub. Select one device with `LAYER_PANEL_CONTACT=mouse`, `touch` or `pen`;
add `--tablet` for pen contacts. The virtual tablet serials cannot authorize
native clipboard selection.
`native_panel_slider_motion` records moving GTK presentation rates and
p99 gaps for three sustained slider and value scrubs each of size, opacity
and flow. Run it through the private-display runner with
`LAYER_NATIVE_EVENT_MS=4`, setting `CAPY_NATIVE_TEST_THEME=light` or `dark`.
Its desktop fixture is a diagnostic, not reference-tablet qualification.
Shared numeric tests cover the 32 px transition, five-point percentage edges,
degree scaling, fine edits and readout formatting. Selection tests also apply
resolved slider, scrub and expression values to whole-pixel region/refinement
controls, preserving their integer admission rules.
Android instrumented tests cover
native editing, expression evaluation, slider geometry and settings input
isolation. `panelValuesScrubFineAndCancelAcrossContacts` and
`propertyValueScrubsHaveOneUndoAcrossContacts` cover mouse, touch and pen value
drags in both themes, integer and fractional readouts, native cancellation and property history.
`native_numeric_preedit_guard` covers both GTK widget branches and
step buttons using native preedit signals; actual IME journeys remain separate.
`native_numeric_size_apply_refuses_uncommitted_text` checks Canvas Size and Image
Size Apply in both themes, including refusal without document edits.
Web's `--pointwise-effects` journey checks compact panel geometry, editor
clearance, value scrubbing and label reset. Its `--curves` journey covers graph
contacts, page changes, exact readouts and
numeric cancellation. Android's
`AndroidHostTest#curvesPagesNativeContactsAndExactCoordinates` exercises native
contacts and history; `AndroidTextCompositionTest#curveCoordinatesKeepNativeCompositionAndUnchangedPrecision`
checks InputConnection composition and unchanged commits. Windows
`exercise-effects.ps1` covers mouse, pen and touch contacts, double-click
insertion and removal, held arrows, Delete, exact and unchanged Output text,
Log HDR readouts in a float drawing and one Undo per property slider scrub.
Its `-PropertyLayout` check also covers editor clearance, mouse,
pen and touch value drags in both themes, settled release, Escape cancellation,
label reset after an invalid draft and one Undo per scrub.
On macOS and iPadOS, `EditorLaunchTests/testFilterArtworkAndHistory` covers
page changes, contacts, point selection, drags and double-click removal with
history, and the `tests/property-slider-input.swift` fixture removes a selected
point with the native Delete key.

References: [GTK Scale](https://docs.gtk.org/gtk4/class.Scale.html),
[Adwaita SpinRow](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.SpinRow.html),
[Compose Slider](https://developer.android.com/develop/ui/compose/components/slider).
