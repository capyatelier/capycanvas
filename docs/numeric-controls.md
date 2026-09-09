# Numeric controls

GTK, web and Android share the same touch-first numeric controls.

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
  bounds. Native GTK spin controls use their standard invalid-input behavior.

## One numeric policy

`layer-ui::NumericControl` owns the control kind, hard and soft bounds, step,
resolution, digits, display scale/unit and range mapping. Its constructor
defaults to a spin control for whole numbers with at most 64 step intervals,
otherwise a slider; definitions can explicitly override the kind. Prediction
horizon (0–64 ms) is a spin row. Brush size is a logarithmic slider over
0.5–2048 px. Opacity is displayed as a percentage; pressure response is linear.

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
The resulting number goes through the existing typed app/preference action,
which remains authoritative for availability, dependencies, application and
persistence. The separate legacy preference-slider action has been removed.

GTK calls the policy directly; web uses `WebApp.number_input`; Android uses
the stateless `Native.number` JNI call. No GPU handle/lock or UI-state snapshot
is needed to evaluate a number. Hosts own native focus, gesture capture,
unfinished text and transient display state only.

## Validation

`native_number_controls` is an ignored Wayland widget test and produces a
dark/light review sheet in `artifacts/ui/numeric/`. Settings screenshots are
in `artifacts/ui/preferences/`; Android instrumented tests also cover native
editing, expression evaluation, slider geometry and settings input isolation.
`native_slider_feedback` covers fractional model echoes and a full forward/back
brush-size sweep through the GTK session. Model refreshes do not emit edits;
deferred GTK range changes compare values at the core's numeric resolution,
avoiding f64/f32 rounding feedback loops on the main thread.
No drawing renderer or input hot path changes are part of this UI work.

References: [GTK Scale](https://docs.gtk.org/gtk4/class.Scale.html),
[Adwaita SpinRow](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.SpinRow.html),
[Compose Slider](https://developer.android.com/develop/ui/compose/components/slider).

## Entering Zen

Enabling Zen immediately hides editor chrome. The core suppresses hover reveal
inside a fixed 200 × 200 logical-pixel top-left guard until the pointer leaves
it; this prevents the activating button from revealing itself again. This is
not configurable. A fresh deliberate contact re-enables normal edge reveal for
touch users. Subsequent docking, drawer and drag visibility use the existing
shared interaction rules.
