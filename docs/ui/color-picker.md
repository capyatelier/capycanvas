# Color picking

[Workspace and UI](README.md) · [Color palettes](color-palettes.md) ·
[Color-management journeys](color-management.md)

## Research

| Application | Useful behavior and options |
| --- | --- |
| [Photoshop](https://helpx.adobe.com/photoshop/desktop/adjust-color/choose-colors/set-foreground-and-background-colors.html) | Point or square averages from 3 to 101 pixels; current layer, all layers, current-and-below, and variants excluding adjustments. Foreground/background destinations and drag sampling. Its documented [sampling ring](https://helpx.adobe.com/archive/en/photoshop/cc/2015/photoshop_reference.pdf) compares new and current colors. |
| [GIMP](https://docs.gimp.org/3.0/en/gimp-tool-color-picker.html) | Selected/merged sampling, an adjustable average radius, foreground/background/palette destinations and an information window. Ctrl temporarily samples while painting. |
| [Procreate](https://help.procreate.com/procreate/handbook/5.3/colors/colors-interface) | Touch and hold, drag, then lift to accept. The loupe shows the candidate above the current color. Sidebar Modify can invoke the floating picker; gesture preferences configure access. |
| [Krita](https://docs.krita.org/en/reference_manual/tools/color_sampler.html) | Temporary Ctrl access, visible/active-layer sampling, average radius, foreground/palette destinations and channel information. Blend percentage mixes the sample with the current paint. |
| [Clip Studio Paint](https://help.clip-studio.com/en-us/manual_en/810_subtools/E.htm) | Current layer, top painted layer, or image sampling; layer exclusions and an adjustable average area. Optional magnifying circle follows hover, with sampled color above and current paint below. |

## Color panel

The Color panel offers an Okhsv circle, an HSV square and an HLS triangle;
switching shapes keeps the sRGB paint. Only the circle's hue ring is rotated,
24° counterclockwise, so its blue sits where HSV's does. The readout shows the
shape's units or RGB, and tapping its label toggles them. Picker coordinates
are kept per paint, so neutral colors keep their hue. Hit testing, conversions,
component values and the selected swatch live in Rust
([`color.rs`](../../crates/layer-ui/src/color.rs)).

GTK prepares the field and managed hue ring on one worker with one replaceable
pending request. Panel resizing scales only the retained hue ring and field;
markers, readout text and buttons follow the current layout on every frame.
The HDR intensity ramp also retains its color texture during resize, draws its
marker and caption at the current geometry, and prepares committed and preview
rasters on its worker. Release and cancellation prepare the final physical
rasters. The ring texture includes its antialiased silhouette; the circle
field uses GTK's rounded clip. Shared adaptive hue stops retain display precision
without evaluating the hue conversion at every pixel.
`native_color_wheel_resize_input` and `native_hdr_color_wheel_resize_input`
exercise sustained mouse and touch resizing, compare control pixels before and
after release, and check retained rasters, final sizes and workspace undo/redo
on the private display. Run them in both themes and at 1× and 2× scale.

Web also prepares committed and preview fields on its existing Wasm worker,
retaining and scaling compatible completed fields while newer sizes are pending.
Its hue ring uses a native conic gradient. `test.mjs --color-wheel-resize` checks
column and floating-panel resize in both themes with mouse and touch, including
the absence of synchronous field raster calls. Android already uses background
field rendering and retains its bitmap across size changes.

Every host places the selected paint circle above the other
circle, including its border and pointer target. Hover leaves that order
unchanged. Transparent paint keeps the remembered paint circle in front;
temporary black or white uses the primary circle. Rust publishes the front
swatch with the color panel view.
The visible circular rim belongs to the button's hit area, and the selection or
hover border covers the paint fill.

Live paint icons in the toolbar and window bar use
the same front swatch as the panel. Each circle has an opaque transparency checker
beneath its paint, so the rear circle cannot show through it. Selecting a paint
updates retained icons even when neither paint color changes. HDR rendition
changes update their displayed colors too. Panel-category icons remain static
symbols.

Compact color fields show and edit the remembered paint while transparency is
selected, including independent temporary black or white. Mask editing uses the
mask's own paints and depth, so HDR artwork rendition does not tone-map mask
controls or add HDR intensity to their editor. Shared `PaintPairView` supplies the
active definition, front swatch, display previews and linear-light checker
composites; hosts present these values and submit the shared color-definition
action. Picker hover previews stay in the Color panel until accepted.

The HDR intensity arc uses the shared round-cap geometry at both ends. Android
receives its pointer contact through the color panel so an empty corner of the
wheel's rectangular view does not hide either cap; occupied wheel and swatch
regions keep their own contacts.

## Edit Color

Edit Color is one page under a centered title, in two columns: the panel's
circle, hue ring and, in HDR drawings, intensity arc on the left with an OKLCH /
HSB / HLS shape choice below; on the right, one row with Current and New, the
eyedropper and a large hex, then three rows of numbers. Current and New match
the eyedropper's height, and the eyedropper and hex center on them, not on
their captions. In narrow windows the page stacks: Current, New and hex, then
the wheel, then the rows; a short window scrolls the page.

Shared [`ColorEditor`](../../crates/layer-ui/src/color/editor.rs) owns the
draft. It wraps a copy of the panel's `ColorState`, so the wheel, hue memory and
HDR base-and-intensity model are the panel's. Only **Use Color** publishes, and
an untouched draft keeps the exact stored definition. GTK calls it directly;
Web, Android, Apple and Windows use the stateless `editor_open`, `editor`,
`editor_strip` and `strip_placement` requests of `color_ui`.

- Each row's name is a format menu: RGB, RGB 0–1 or Linear RGB; HSB or HSL;
  OKLCH or OKLab. Rows show the base color and name the drawing's RGB space; HDR
  drawings add an Intensity (EV) row. Row formats and the swatch search are
  remembered in `ColorState` and saved when the dialog closes
  (`ColorAction::EditorMemory`), for paint and mask colors alike.
- Tapping a number opens it for typing; Enter or leaving the field commits and
  Escape cancels. A refused value stays open with its message and blocks Use
  Color. Any field, and paste anywhere on the dialog, also accepts a whole
  color: hex, CSS names, `rgb()`, `hsl()`, `hsb()`, `oklch()`, `oklab()` and
  `color()` in the four RGB spaces. Colors have no alpha.
- Dragging a number up or down adjusts it from its value at the start of the
  drag (Shift for larger steps, Alt or Ctrl for finer); arrow keys step it.
- Each row and the hex have a copy button. Copies use standard notations:
  `rgb()` and `hsl()` in sRGB drawings, `color()` with the drawing's space
  otherwise, and `oklch()`.
- Current shows the starting color; tapping it reverts. Hex outside sRGB is
  marked ≈ nearest sRGB; in HDR drawings it describes the base color.
- The bottom row shows recent colors and, in its last cell, a chevron that
  slides a sheet up over the page, and back down when closed, with every recent
  color and palette, searchable by palette name, color name or hex
  (`SwatchSheetView`), with + to add the new color to a palette. The sheet's list
  scrolls down to the divider above the buttons.
- The eyedropper hides the dialog and starts the session's editor-mode picker
  (`ColorPickerAction::Editor`). The paint and color history do not change; a
  pick publishes `color_picker.picked` and the dialog returns with it. A strip
  in a corner of the canvas work area, between the panels, shows the hex and
  the current shape's values. It starts top-right and stays in its corner until
  `color_picker.sample_point` or a hovering mouse or pen covers it; it then
  moves to the first free corner of top-right, top-left, bottom-right and
  bottom-left, so the bottom corners serve work areas too narrow for two strips
  side by side (`ColorStripPlacement`). A corner is free only when it covers
  neither point, so the strip cannot oscillate. Escape, or tapping the strip,
  returns without a change.

On the OKLCH circle, dragging from the field out past the hue ring snaps to
white, full color or black within 18° of their directions. The panel and the
dialog share this geometry.

Every host reuses its own controls: the shape choice is the workspace
switcher's well of pills, recent and sheet swatches are palette tiles, and value
cells are number controls. Titles are bold, the hex medium and other text
regular. Format names and value cells reserve their widest text, so changing a
format or showing an error never resizes the page.

GTK presents an `adw::Dialog` and converts its body, footer and sheet content
to squircles; the sheet's scroll bar stays outside the converter, and the sheet
slides inside a clipped overlay. Current and New snap their shared edge to
device pixels at fractional display scales. Picking
closes the dialog and presents it again afterwards. Web closes its modal
`<dialog>` while picking so the canvas receives input. Android draws the editor
in the workspace tree, because a dialog window would take the canvas's input;
an editor opened from another dialog window uses a dialog window and hides the
eyedropper, as GTK does when another dialog is open. Apple and Windows show the
value rows without the wheel, eyedropper or sheet.

## Picker

One tool family has two presentations: **Color Picker** (default glass loupe)
and **Eyedropper** (small pipette cursor). Sample area and source are settings,
not additional tools. The useful common minimum is **Source** (Visible color /
Selected layer) and **Sample size** (1, 5, 15, 51 or 101 document pixels).
Multi-pixel samples use circular footprints. Layer sampling reads raw paint,
before opacity, masks and effects, and is available only for a paintable layer.
Transparent samples do not replace the current color.
While editing Quick Mask or a selection layer, the picker previews and updates
the mask's painting colors, preserving the artwork colors. Returning to artwork
cancels any pending sample.

The Sketch toolbar has a picker with a plain squircle icon between size and
opacity, followed by Undo and Redo below opacity. Its tile uses the standard
toolbar styling and joins the open drawer with square corners. Press it
or **I** to enter temporary picking; mouse/pen hover previews, mouse press
accepts, and pen contact previews until release accepts. The previous tool
returns. Press the button/I again, Escape, or tap with a finger to cancel.
Pressing another tool cancels picking and activates the requested tool. Sketch uses
a standalone Color Picker control that always starts the glass loupe; its
double-press drawer contains only Source and Sample size. Paint and Photo use
the Eyedropper category and pipette icon, with both Color Picker and Eyedropper
on the left of its settings drawer. Neither picker drawer dismisses on an
outside UI contact. Dropdown popups remain interactive, and no instructional
hint text occupies the settings panel.
GTK, Android and Windows count a settings popup as focus within the same window;
opening a dropdown must not trigger canvas blur. Leaving the application still
cancels temporary picking.

Touch and hold on the canvas starts the same picker with the loupe lifted above
the finger. The sampled point and magnified view are both centered at the visible
crosshair above the finger, including when the loupe is clamped at a viewport
edge. Lifting accepts.
A second finger toggles visible/selected-layer sampling when a paintable layer
is selected. A fine stacked-layer mark above and to the right of the crosshair indicates raw layer
sampling, whether changed by touch or the Source dropdown. Changing source keeps
the settings drawer open. Native hold timing, movement slop and sequence
ownership belong to each host; sampling, preview, acceptance and cancellation belong
to shared Rust.

Entering the picker retires existing navigation contacts, including a resting
palm. Their later movement cannot resume navigation, and their pending hold
callbacks cannot take over the picker. Shared Rust accepts a native hold only
for the sole live, unclaimed navigation contact. Touch release/cancellation
always retires navigation state, including during tool or workspace transitions.
Consumed contacts are identified by pointer kind and ID together; a touch must
not capture a mouse or pen that has the same numeric ID.
Android retains native contact identities until release: Compose may synthesize
an empty cancellation event, which ends those contacts without inventing a pen.
Focus loss and surface teardown also cancel captured native contacts.
Windows cancels every contact its core has seen begin on blur, renderer suspension
and surface replacement. The lift or cancellation of such a contact still reaches
Rust while workspace input is blocked; a new press with a reused ID starts fresh.
Its hold uses a native `GestureRecognizer` on the canvas input thread, and the
lift offset is 10 mm at the monitor's raw DPI, clamped to 36–64 logical pixels.

The loupe has a 2× interior, a tiny central cross, and a 14-logical-pixel split ring with the
candidate on top and the original selected color below. Fine edge reflections
give it a glass appearance, with no thick outer stroke or shadow. The interior
circle uses analytic antialiasing, and the tiny crosshair has a thin white
keyline. Hover updates the color wheel as a reversible preview and does not
change brush state or color history. GTK batches wheel previews within 17 ms and
renders preview fields and the HDR intensity arc off the UI thread, with one job
in flight and one replaceable latest request per raster. Completed fields continue to appear during
motion; stopping always finishes the latest hue. Cancellation, pointer exit and
committed colors invalidate pending preview results and refresh synchronously.
Wheel marker outlines and curved readouts use retained native drawing/text nodes
rather than uploading transparent Cairo surfaces on each movement. Solid colors
within sRGB use native fills; colors outside sRGB and physical HDR retain managed
textures. The HDR arc's antialiased coverage is baked into its background raster
after color mapping, avoiding a changing texture under a GTK stroke mask.
Keep the wheel's fill/ring paths and HDR zero-tick path alive until their geometry
changes: [GTK's stroke cache](https://github.com/GNOME/gtk/blob/4.22.0/gsk/gpu/gskgpucachedstroke.c)
keys on path identity. Rebuilding equal paths defeats the cache and causes
repeated mask rasterization/upload on the UI thread, even with cached colors.
This avoids blocking input on rasterization; the former 33 ms coalescing alone
did not address those stalls. The shorter panel budget reads the latest sample
without postponing its deadline, independently of full-rate loupe movement.
The same renderer serves docked and drawer color panels, at native display DPI.
Web and Android publish preview colors separately from retained workspace and
panel models. Web sends field raster requests to a dedicated Wasm worker;
Android uses a conflated coroutine channel and pure JNI raster functions on a
background dispatcher. Both keep one raster in flight and the latest pending
request. A finished older hue may appear during movement. GTK and Web also accept
compatible older sizes and scale them while the latest request finishes; Android
retains its last bitmap and rejects older sizes. Results for a previous shape or
rendition, or arriving after cancellation, are rejected.
Apple stages motion-published previews into their own snapshot field, so only the
color panel re-evaluates, and rasterizes preview wheel fields on a serial worker with
the same one-running, one-pending policy. The Mac and iPad hosts send `cursor_leave`
when hover ends (a pointer cancel would end picking) and arm the finger hold with
UIKit's 0.5 s timing and 10 pt slop. Windows applies preview packets to its
retained color views without a workspace rebuild, at most once per UI dispatch,
and rasterizes preview fields on a background worker with the same policy. The loupe stays
on the canvas GPU path; preview work never mutates brush colors or history.
Sampling uses artwork coordinates independent of canvas zoom, rotation, flips,
selection boundaries and display/proof transforms. The zoomed interior is a
view of the visible artwork, including when sampling raw layer paint.

“Show footer” controls both canvas information and the HDR/color-space status
badge. Hiding it also keeps the badge hidden through subsequent color updates.

Area sampling averages alpha-weighted [Oklab](https://bottosson.github.io/posts/oklab/) coordinates,
the Cartesian counterpart of OKLCH. This avoids averaging hue angles across the wrap at 360°,
uses a perceptually uniform space, and gives transparent pixels no weight.
Samples are unassociated, converted from document primaries to linear sRGB,
transformed to Oklab, averaged, then converted back to document-linear RGB.
Point sampling bypasses this conversion and stays exact, including HDR values.
Unrelated colors can still lose saturation: a true average cannot promise to
preserve the saturation of arbitrary contrasting colors.

## White Balance

GTK, Web, Android, macOS and iPadOS Properties offer an eyedropper button, **Pick neutral point**, for White Balance. The canvas bar
keeps Sample size and Cancel available; calibration starts at 5 pixels. A mouse
or pen release samples the input before that adjustment, including its actual
group and clipping scope. Touch uses the lifted contact point. Its glass shows
the sampled input color, with a checker while the asynchronous sample is pending.
This color swatch stays independent of the corrected canvas and paint color wheel.

Calibration averages alpha-weighted linear document RGB, using circular widths
1, 5, 15, 51 or 101. Empty, nonpositive or unrepresentable samples leave the picker
armed with a reason. A successful correction changes Temperature and Tint
together as one undo step, preserving the paint color and returning to the prior
tool. Exact Temperature/Tint values can extend beyond the sliders' usual
−100…100 range, up to ±1000 and ±800 respectively.

Released contacts wait for their own frozen sample. Source edits, another
property or tool, document replacement, Escape and focus loss retire pending
results. Advancing animation alone does not discard a released correction.
Save waits for pending sampling to finish or be cancelled. Native hold timing
stays in the host; source selection, validation and the correction live in Rust.

## Levels and Curves

GTK, Web, Android, macOS and iPadOS Properties group black, neutral and white point calibration in one
eyedropper menu beside the channel selector. These actions
use the same sample sizes, contact ownership and exact pre-adjustment input as
White Balance. The RGB page corrects the three channels together while preserving
master settings; a channel page changes only that channel. Black and white target
linear zero and one. Neutral retains the processed sample's brightness, using
encoded luminance for Levels and linear luminance for Curves. Unreachable targets
leave the adjustment unchanged and keep the picker armed.

Curves also offers targeted adjustment. Press on the image and drag vertically
to adjust the sampled tone; up raises the curve. RGB targets the luminance after
the channel curves, while a channel page targets that channel's input. The sampled
input uses a five-pixel circle and remains fixed throughout the contact,
including changes in canvas zoom or rotation. Each 255 logical pixels moves
through the full output range. Release
commits one edit; Escape or cancelled input restores the starting curve. The mode
stays armed between contacts, with Done in the canvas bar. Sampling may finish
after release without losing the accepted drag distance.

## Scope

Navigator sampling is explicitly excluded because it would conflict with its
existing navigation. Reference-panel sampling is a future extension. Desktop
screen capture, persistent measurement pins, palette extraction and Krita-style
color mixing are separate workflows and are deferred rather than crowding this
drawer. The existing color panel already supplies numeric inspection and color
library access.

## Checks

```sh
LAYER_NATIVE_EVENT_MS=8 tools/performance/workspace-motion.sh gtk --native-test=native_color_swatch_overlap_input --tablet
LAYER_NATIVE_EVENT_MS=60 tools/performance/workspace-motion.sh gtk --native-test=native_color_picker_input --tablet
LAYER_NATIVE_EVENT_MS=8 LAYER_MOTION_VIEWPORT=4800x3000 LAYER_MOTION_SCALE=3 \
  tools/performance/workspace-motion.sh gtk --native-test=native_color_picker_preview_pacing
tools/performance/workspace-motion.sh gtk --color-panel      # or web
node apps/layer-web/test.mjs --color-picker                   # device.test.mjs --color-picker on a tablet
tools/windows-vm/windows-vm.py fixtures compact-color:dark compact-color:light
./apps/layer-windows/scripts/exercise-color-picker.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe
```

- `native_color_swatch_overlap_input` checks circle borders, overlap ownership,
  selection and hover with mouse, pen and touch in both themes. The regular
  `--color-panel` journey also checks rim hit areas at five panel widths.
- `native_color_picker_input` drives mouse, pen and touch in both themes. Popup
  checks run before synthetic tablet injection, whose serials cannot authorize
  compositor popup grabs.
- `native_color_picker_preview_pacing` moves across painted hues with the wheel
  hidden and shown, in SDR and Float16 documents, and records compositor
  presentation intervals and GTK paint time; counting field rebuilds alone does
  not detect lag. Use `3200x2000` and scale `2` for the 2× check.
  `native_solid_colors_match_tagged_textures` compares native fills with managed
  textures.
- Windows `compact-color:dark` and `compact-color:light` check composed overlap
  and rim pixels, native mouse/pen/touch selection, mouse/pen hover, transparent
  memory and retained keyboard focus. WARP results do not establish physical pen
  or hardware timing.
- Android `AndroidColorPanelTest#selectedSwatchOwnsOverlapAndKeepsItsRim` checks
  both themes, overlap pixels and contacts, circular clipping and hover transfer.
  Android also runs `AndroidColorPanelTest#glassPickerInputAndSettings`,
  `#pickerWheelPreviewPerformance` and `#pickerRetiresRestingContactsAndPendingHolds`
  with `-e systemInput true`. Apple runs the `picker` tests in
  `cargo test --locked -p layer-apple --target aarch64-apple-darwin --lib` and the
  `testColorPicker` journey; `testPhotoScopes` covers the calibration menu.
- Edit Color: `native_color_editor_rows_sheet_and_canvas_pick` (GTK, real
  pointer drags and canvas picks), `native_color_editor_live_language` (all
  languages, SDR and HDR, large text), `native_color_editor_visual_audit`
  (captures every format, shape, editing, error, sheet, narrow, German and SDR
  state, keeps the page one size across formats and shapes, and checks the
  Current/New seam; run it also with `LAYER_MOTION_SCALE=1.25`), `test.mjs --color-editor` and
  `--live-language-color` (Web), `node --test apps/layer-web/color-controls-copy.test.mjs`
  and `color-button-lifecycle.test.mjs`, and Android
  `AndroidColorPanelTest#editColorRowsSheetAndCanvasPick`. Each runs in both themes.
- Web pen timing uses `tools/performance/web-pen.mjs --os-input --picker`
  (`LAYER_PICKER_SAMPLE_SIZE=101` for the largest sample); see
  [measuring](../performance/measuring.md). Large-area averaging still costs
  throughput on tablets, so the Web port keeps display-paced updates.
