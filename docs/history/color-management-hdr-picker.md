# HDR intensity picker review

Source milestone: `d568c44a7c1702f8c66336fc4f3932f49785b789` in
`~/code/capycanvas3`, following the user's choice of an always-visible horizontal
HDR ramp above the color shape. Nothing is pushed.

## Behavior

Only HDR documents show the new control. The thick, current-color ramp has a
circular thumb matching the wheel markers, a neutral tick, an editable EV value
and the existing Edit Color button. The ordinary drag range is −2 through +6 EV;
exact entry and sampling can extend it. Unsupported storage values are rejected,
and rejected edits restore the displayed control value. The compact header fits
a 160px panel in light and dark themes. SDR documents keep their original layout,
field textures and hue guide.

EV is a separate linear-light multiplier of the remembered base color: +2 EV
multiplies RGB by four, preserves alpha and leaves black black. EV changes retain
the field marker; hue, saturation and field/value picking retain EV. Foreground
and background keep independent bases/intensities, including swap and workspace
serialization. Sampling or exact entry preserves the definition and derives a
base/intensity decomposition. Signed values survive intensity changes; explicit
field picking chooses a bounded document-gamut base.

The circle, square and triangle display the selected intensity. Their retained
Float32 base field is keyed by size, shape, hue and document RGB space. EV and
display changes reuse that base and replace only the viewing derivative. GTK
receives tagged Rec.2100-linear half-float textures on an HDR-capable GPU display;
SDR-only displays use the saved SDR rendition. The stable hue guide remains an
SDR reference. Actual paint and file values never come from these derivatives.

GtkScale/GtkRange retains pointer capture, keyboard behavior and accessibility.
The custom snapshot supplies the managed ramp, circular thumb and focus ring.
The slider follows ordinary value-control interaction, not the reorder hold rule.
Both docked panels and retained color drawers use this component. Capability
changes invalidate the display derivative without changing the selected paint.

## Evidence

Artifacts are under `artifacts/color-m4/hdr-picker-*`; the runnable package is
`artifacts/color-m4/review/`. Its manifest records executable hashes and the
previous review build. Validation uses the bundled GTK 4.22.4 tablet-pad fix,
Fedora 44, NVIDIA RTX PRO 6000 Blackwell Max-Q, driver 610.57.04, and Vulkan.

- `shared.log`: all 458 shared UI tests passed. `final-color.log`: all 42 color
  tests passed after adding the retained base field. New numerical checks cover
  all working spaces and all three shapes, ×4 RGB, unchanged alpha, black,
  stable markers, picking/rendering agreement, exact signed entry, serialization,
  swaps, invalid input and returning to SDR.
- `workspace-check.log`, `web-check.log`: workspace/all-target checks and the
  wasm32 browser host compile. Windows emits its existing unused-code warnings.
- `native-input.log`, `2x.log`: real injected mouse/touch and keyboard input
  through isolated 120Hz Mutter at 1× and 2× scale. The circular thumb redraws,
  the field follows EV, transparency disables intensity, and SDR field picking
  remains operable for circle/square/triangle. Wide/narrow and light/dark captures
  are retained. This is virtual touch delivery, not physical touch/pen qualification.
- `desktop/test.log`: actual HDR desktop. At +2 EV the three sRGB field textures
  have peaks about 4.0013 after half-float/matrix roundoff; GSK retains the ramp's
  above-white values (peak about 28.6984 for the test color/range). Injected
  capability loss/restoration switches the derivative and preserves paint.
- `60mp/test.log`, `60mp/process-memory.log`: the retained 8192×7324 ProPhoto
  half-float document with 20 effects completes picker workflows, three shapes,
  capability switching and narrow layouts. The 60-update smoke loop took
  326.11ms; peak process RSS was 1,556,648 KiB (about 1,520 MiB), including file
  loading, screenshot/readback work and both test windows. Field caches depend
  on widget size, not document size.
- `journey-final.log`: native HDR input, exact color entry, exposure/curves,
  painting, undo/redo, saved SDR appearance, save/reopen and HDR/SDR export pass.
  Reusing the earlier output directory hit GTK's overwrite-confirmation dialog;
  the clean-output rerun passed. That timeout is retained in `journey.log`.

The desktop/60MP/journey runs use the preserved
`hdr-picker-tests-before-header-spacing` executable. The only subsequent source
change reduced the header gap from 6 to 2px; final 1×/2× native input/layout runs
use `hdr-picker-tests`. Both hashes are in the review manifest.

### Baseline and limitations

The pre-change source (`3cf9e5e9`) and test executable are retained under
`hdr-picker-baseline/`; the previous runnable app is
`review/capycanvas-gtk-hdr.before-hdr-slider`. The older SDR hue-guide comparison
fails identically before and after this change: 3/255 versus its 2/255 tolerance
against a native conic gradient. Its assertion was not weakened. Of 24 matching
SDR screenshots produced before that stop, 23 PNG files are byte-identical;
the initial frame differs. See `hdr-picker-sdr-baseline-comparison.json`.
The separate final native test verifies SDR picking despite that older test stop.

The timed update loops include event pumping and coalesced rendering; they are
not input-to-present or p95/p99 measurements. The 2× SDR-fallback loop is slower
(about 1.6s for 60 updates). This update does not claim 120Hz qualification, a
new steady GPU-memory budget, or an improvement against a comparable 60MP
parent baseline. The earlier phase-4 export/memory evidence remains separate.

Emitted luminance/calibration, mixed-monitor moves, physical touch/pen,
constrained/mobile devices and other native hosts remain unqualified. GTK is the
review implementation; browser HDR remains explicitly unsupported. The full
phase-4 device/performance gates are still outstanding. RAW, layered PSD, native
CMYK, gain maps, OCIO/ACES and Float32 document storage remain outside this change.
