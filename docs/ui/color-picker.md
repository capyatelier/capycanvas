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
Pressing another tool also cancels and restores the previous tool. Sketch uses
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
request. A finished older hue may appear during movement, but results for a
previous size, shape or rendition, or arriving after cancellation, are rejected.
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

## Scope

Navigator sampling is explicitly excluded because it would conflict with its
existing navigation. Reference-panel sampling is a future extension. Desktop
screen capture, persistent measurement pins, palette extraction and Krita-style
color mixing are separate workflows and are deferred rather than crowding this
drawer. The existing color panel already supplies numeric inspection and color
library access.

## Checks

```sh
LAYER_NATIVE_EVENT_MS=60 tools/performance/workspace-motion.sh gtk --native-test=native_color_picker_input --tablet
LAYER_NATIVE_EVENT_MS=8 LAYER_MOTION_VIEWPORT=4800x3000 LAYER_MOTION_SCALE=3 \
  tools/performance/workspace-motion.sh gtk --native-test=native_color_picker_preview_pacing
tools/performance/workspace-motion.sh gtk --color-panel      # or web
node apps/layer-web/test.mjs --color-picker                   # device.test.mjs --color-picker on a tablet
./apps/layer-windows/scripts/exercise-color-picker.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe
```

- `native_color_picker_input` drives mouse, pen and touch in both themes. Popup
  checks run before synthetic tablet injection, whose serials cannot authorize
  compositor popup grabs.
- `native_color_picker_preview_pacing` moves across painted hues with the wheel
  hidden and shown, in SDR and Float16 documents, and records compositor
  presentation intervals and GTK paint time; counting field rebuilds alone does
  not detect lag. Use `3200x2000` and scale `2` for the 2× check.
  `native_solid_colors_match_tagged_textures` compares native fills with managed
  textures.
- Android runs `AndroidColorPanelTest#glassPickerInputAndSettings`,
  `#pickerWheelPreviewPerformance` and `#pickerRetiresRestingContactsAndPendingHolds`
  with `-e systemInput true`. Apple runs the `picker` and `inspection` tests in
  `cargo test --locked -p layer-apple --target aarch64-apple-darwin --lib` and the
  `testColorPicker` journey.
- Web pen timing uses `tools/performance/web-pen.mjs --os-input --picker`
  (`LAYER_PICKER_SAMPLE_SIZE=101` for the largest sample); see
  [measuring](../performance/measuring.md). Large-area averaging still costs
  throughput on tablets, so the Web port keeps display-paced updates.
