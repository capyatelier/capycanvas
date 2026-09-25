# Color picking

Implemented on GTK, Web, Android, macOS, iPadOS and Windows, September 2026.
GTK was reviewed before the Web and Android rollout.

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

## Validation

- Resting-contact regressions run in the shared picker tests,
  `native_color_picker_input` on GTK, `--color-picker` in the Web desktop/device
  harnesses, and Android's
  `AndroidColorPanelTest#pickerRetiresRestingContactsAndPendingHolds` with
  `-e systemInput true`. They cover toolbar entry during contact, pending holds,
  release/cancellation, inert subsequent single-finger drags, and continued
  two-finger navigation. Shared tests also cover pointer-kind ID collisions and
  terminal cleanup during blocked routing.

- GTK release build: `cargo build --locked --release -p layer-linux`.
- Shared UI/host/workspace library coverage includes reversible hover,
  exact acceptance during rapid motion, stale readbacks, transparent
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
- Apple: the `picker` and `inspection` bridge tests in `cargo test --locked -p layer-apple
  --target aarch64-apple-darwin --lib` cover mouse hover preview and press acceptance,
  pen contact preview and lift acceptance, `cursor_leave` versus pointer cancel,
  finger holds sampling above the contact, second-finger source toggling, sole-contact
  admission, preview publication and circular/point samples from visible and layer
  sources. The `testColorPicker` XCUITest journey (`ColorPickerChecks.swift`) runs on
  macOS and a physical iPad: toolbar and Sketch entry, hover preview while leaving the
  canvas, mouse acceptance, finger-tap cancellation, long-press sampling, neutral
  shortcuts, Sketch's toolbar order and the double-press settings drawer.
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
- Web release build: `apps/layer-web/build.sh`. The shared
  `apps/layer-web/color-picker.test.mjs` runs through `test.mjs --color-picker`
  on a composited desktop and `device.test.mjs --color-picker` on the Huion.
  It checks keyboard entry/cancel, Sketch toolbar order, both drawer layouts,
  settings, real red-paint sampling, pen-up acceptance, touch offset/source/cancel,
  and a visible wheel preview without synchronous field or workspace rebuilds.
  The existing color-panel regression covers 60 layouts, both themes, three
  shapes, and mouse/touch/pen/keyboard input. Use a composited browser for GPU
  screenshots; headless captures can omit the canvas surface.
- Web performance uses `tools/performance/web-pen.mjs --os-input --picker`
  with a dedicated tablet test origin and CDP endpoint. Build/push
  `tools/performance/AndroidPenMotion.java` as described in
  [the Huion pen guide](../development/web-pen-huion-2026-09-20.md), or set
  `LAYER_PEN_HELPER` to the dedicated device dex path. The replay remains clear
  of the floating color panel. `LAYER_PICKER_SAMPLE_SIZE=101` exercises the
  largest sample. On the 90 Hz Huion, three alternating five-second point-sample
  runs measured 78.5–79.9 canvas submissions/sec with the wheel closed and
  73.3–73.9 open; input-to-submit p95 was 26.3–26.5 ms closed and 27.2–27.8 ms
  open. Frame CPU p95 was 2.3–2.8 ms. These are submission and input timings,
  not compositor presentation measurements. No synchronous wheel rasters or
  workspace model rebuilds occurred during these runs. A five-second 101-pixel
  run measured 75.5 submissions/sec closed and 64.7 open, with input-to-submit
  p95 of 27.9 and 27.6 ms respectively. Large-area averaging and browser
  composition still cost throughput on this tablet; an additional preview timer
  did not recover it, so the Web port retains display-paced updates.
- Windows: `apps/layer-windows/scripts/exercise-color-picker.ps1 -Executable
  artifacts/windows/Release/CapyCanvas.exe` drives the production app with
  OS-injected mouse, pen and touch. It checks **I**/Escape, Sketch toolbar order,
  double-press into the explicit Sketch drawer and the two-column Paint drawer,
  the Source and Sample size choices, reversible pen and mouse hover, pen-lift
  and mouse-press acceptance, finger-tap cancellation, touch-and-hold with the
  lifted sample and second-finger source toggle, and a Paint hover sweep with no
  workspace rebuilds or UI-thread field rasters. Synthetic pens leave range
  without frames, so it keeps them hovering while waiting, as a physical pen does.
  `exercise-canvas-touch.ps1` covers a finger lifted under a modal dialog.
- Android builds with `:app:assembleDebug :app:assembleDebugAndroidTest`.
  `AndroidColorPanelTest#glassPickerInputAndSettings` and
  `#pickerWheelPreviewPerformance`, with instrumentation argument
  `-e systemInput true`, exercise production Compose/Rust and OS-injected
  stylus, touch and keyboard input on the Huion. The performance test alternates
  a visible and hidden wheel at the 101-pixel setting with 200 Hz hover input;
  it records frame CPU time and publication counters in the app's validation
  directory. On the Huion, 101-pixel samples measured median frame CPU time of
  4.6 ms closed and 4.8–5.1 ms open, with p95 below 9.6 ms and no full snapshot
  or panel-content publications during hover. Test workspace/settings storage
  is isolated from user preferences.
  The existing `touchPenAndMousePickWithoutHoldAndKeepCapture` regression also
  passes through View dispatch, including intentionally invalid post-cancel
  motion that Android's OS injector correctly refuses to deliver.
  These device checks use typed input replay, not a person moving the pen, and
  do not qualify physical HDR output or very large documents.
