# Color picking

GTK design and review scope, September 2026. Other hosts remain pending review.

## Research

| Application | Useful behavior and options |
| --- | --- |
| [Photoshop](https://helpx.adobe.com/photoshop/desktop/adjust-color/choose-colors/set-foreground-and-background-colors.html) | Point or square averages from 3 to 101 pixels; current layer, all layers, current-and-below, and variants excluding adjustments. Foreground/background destinations and drag sampling. Its documented [sampling ring](https://helpx.adobe.com/archive/en/photoshop/cc/2015/photoshop_reference.pdf) compares new and current colors. |
| [GIMP](https://docs.gimp.org/3.0/en/gimp-tool-color-picker.html) | Selected/merged sampling, an adjustable average radius, foreground/background/palette destinations and an information window. Ctrl temporarily samples while painting. |
| [Procreate](https://help.procreate.com/procreate/handbook/5.3/colors/colors-interface) | Touch and hold, drag, then lift to accept. The loupe shows the candidate above the current color. Sidebar Modify can invoke the floating picker; gesture preferences configure access. |
| [Krita](https://docs.krita.org/en/reference_manual/tools/color_sampler.html) | Temporary Ctrl access, visible/active-layer sampling, average radius, foreground/palette destinations and channel information. Blend percentage mixes the sample with the current paint. |
| [Clip Studio Paint](https://help.clip-studio.com/en-us/manual_en/810_subtools/E.htm) | Current layer, top painted layer, or image sampling; layer exclusions and an adjustable average area. Optional magnifying circle follows hover, with sampled color above and current paint below. |

## End state

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

The Sketch toolbar gains a picker with a plain squircle icon between size and
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
GTK counts focus in a native popup as focus within the same window; opening a
dropdown must not trigger canvas blur. Moving focus to another window still
cancels temporary picking.

Touch and hold on the canvas starts the same picker with the loupe lifted above
the finger. The sampled point and magnified view are both centered at the visible
crosshair above the finger, including when the loupe is clamped at a viewport
edge. Lifting accepts.
A second finger toggles visible/selected-layer sampling when a paintable layer
is selected. A fine stacked-layer mark above and to the right of the crosshair indicates raw layer
sampling, whether changed by touch or the Source dropdown. Changing source keeps
the settings drawer open. Native hold timing, movement slop and sequence
ownership belong to GTK; sampling, preview, acceptance and cancellation belong
to shared Rust.

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

## Changes from the previous implementation

- Replace visible/layer and small-area subtool entries with presentation styles
  and actual settings.
- Replace contact-only color updates with separate hover preview and commit.
- Add temporary tool restoration, touch hold arbitration and the glass loupe.
- Retain the bounded asynchronous GPU sampler and coalesce hover requests;
  reject stale results after source changes, cancellation and tool switches.
  Hover displays completed samples while coalescing movement; acceptance uses
  the exact contact point, so rapid hover cannot starve the preview.
- Add the Sketch picker and history buttons conservatively to untouched included
  layouts; preserve customized workspaces.

Navigator sampling is explicitly excluded because it would conflict with its
existing navigation. Reference-panel sampling is a future extension. Desktop
screen capture, persistent measurement pins, palette extraction and Krita-style
color mixing are separate workflows and are deferred rather than crowding this
drawer. The existing color panel already supplies numeric inspection and color
library access.

GTK implementation and visual/input checks must be reviewed before rollout to
other platforms.

## Validation

- GTK release build: `cargo build --locked --release -p layer-linux`.
- Shared UI/workspace library tests: 564 passed. Coverage includes reversible
  hover, exact acceptance during rapid motion, stale readbacks, transparent
  samples, tool restoration, touch ownership, source switching and Navigator
  navigation. Temporary picking is omitted from saved workspace tool state.
- GPU checks cover circular Oklab averages, sRGB/Display P3, alpha weighting,
  signed/HDR values and exact points. Existing point/area, imported-source and
  cold-paint sampling regressions pass.
- Native GTK checks use the isolated Mutter driver with a virtual tablet:
  `LAYER_NATIVE_EVENT_MS=60 tools/performance/workspace-motion.sh gtk --native-test=native_color_picker_input --tablet`.
  They exercise real mouse/pen/touch, pen-up acceptance, crosshair-centered touch,
  source feedback, double press, both themes, native dropdown clicks/taps,
  Sketch's settings-only drawer, Paint/Photo categories, retained settings, Undo/Redo,
  live wheel preview, the traditional cursor, and hold-before-drag for all three
  devices, plus cancellation when another window gains focus. Popup checks run
  before synthetic tablet injection, whose serials cannot authorize compositor
  popup grabs. Generated screenshots belong under ignored `artifacts/`, not in git.
- The existing `native_drawer_dismissal_input` regression also passes in both
  themes with mouse and touch, preserving other drawers' dismissal behavior.
- `native_color_picker_preview_pacing` moves continuously across painted hues
  with the wheel hidden and visible, in SDR and Float16 documents. It checks the
  final field texture, preview restoration, and Show footer's HDR badge behavior.
  It records actual compositor presentation intervals and GTK paint time, as well
  as raster CPU time; counting fewer field rebuilds alone did not detect the lag.
  Reproduce at 3× scaling with `LAYER_NATIVE_EVENT_MS=8
  LAYER_MOTION_VIEWPORT=4800x3000 LAYER_MOTION_SCALE=3
  tools/performance/workspace-motion.sh gtk --native-test=native_color_picker_preview_pacing`.
  Use `3200x2000` and scale `2` for the corresponding 2× check. These are small
  drawing fixtures on a virtual SDR display, not physical HDR-display or
  large-document qualification. With a 120 Hz virtual monitor at 3×, the final
  run measured 120 fps with the panel closed, 120 fps with the SDR wheel open,
  and 119 fps with the HDR wheel open; presentation-gap p95 stayed at 8.5 ms.
  GTK paint p95 was 3.1 ms for SDR and 5.2 ms for HDR on the NVIDIA Vulkan host.
- `native_solid_colors_match_tagged_textures` compares native fills against
  managed textures through GTK's GPU renderer, including Display P3, transparent
  colors, and HDR-to-SDR rendition. The existing native color-panel and HDR-picker
  regressions cover the retained readouts, controls, and field/arc rendering.
- Application/library Clippy completes with existing repository warnings.
  All-target Clippy is blocked by the existing approximate-TAU error in
  `layer-render-wgpu/examples/contact_gallery.rs:381`.
- A broader GPU run also selects
  `spatial_filters_match_linear_sampling_oracles`, which fails its Ripple oracle
  with 91,837 differing pixels. The identical failure was reproduced from an
  untouched `dc27e04d` snapshot; it is outside this picker change.
