# Rendering and composition

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The renderer has to keep a large, layered document responsive within a limited
graphics-memory budget. Hundreds of layers may contain only small parts of an
illustration. Storing 200 full-size 4096 × 4096 RGBA8 images would require 12.5 GiB
before masks, brush state or intermediate results. Rebuilding those images for
every pen sample would also repeat almost entirely unchanged work.

Capy Canvas allocates painted regions in tiles, retains their GPU textures, and
tracks which composition results an edit invalidates. Shaders draw the marks,
apply effects and combine layers into the visible image.

## Stored pixels and the viewport

[`WgpuRasterizer`](../../crates/layer-render-wgpu/src/lib.rs) stores painted layer
content in sparse 256 × 256 texture pages. Pages are allocated as regions are
touched, rather than reserving a full-size paint texture for every empty layer.
Brushes that need coverage or wetness can allocate additional state alongside
those pages. Composite images and filter intermediates have their own storage,
so sparse paint pages do not make total memory independent of document size.
These pages are software-managed regions of the document, separate from the
small on-chip tiles used internally by some GPU architectures.

### Layer extents and the canvas window

Each layer's pages live in its own local extent,
[`Layer::local_extent`](../../crates/layer-core/src/layers.rs): the canvas size, the
extent a canvas change left behind (`LayerProperties.extent`) and a placed photo's
size, whichever is larger. The canvas is a window over those extents. Composition,
presentation and export cover the canvas only; pixels outside it stay on their
layers, hidden, and count toward the project's tile and byte limits.

Canvas geometry commands build one batch in
[`canvas_geometry.rs`](../../crates/layer-core/src/canvas_geometry.rs):
- A crop only moves root offsets and stores the old extent. It never copies pixels,
  so growing the canvas again shows the hidden pixels.
- Growing the canvas left or up *rebases* a paint layer without a source by whole
  tiles: its tile keys shift, still sharing their backing, and its offset, mask
  offset and mask `initial` coverage move the other way. Tile coordinates stay
  unsigned. A layer with a source never rebases, so the new strip beside a photo is
  not paintable, as beside a moved photo.
- After every geometry edit, each paint layer without a source, and each mask,
  covers the canvas window in its local coordinates.
- A fill, gradient or figure with no selection is bounded to the canvas window on
  a layer with hidden pixels, and never writes past the layer's extent within an
  edge page, so growing the canvas shows transparency there. Brush dabs past the
  canvas edge may write hidden pixels, as they already do on photo layers.
- A turned, flipped or resized canvas (Straighten, Rotate and Flip Image, Image
  Size) is a `linear` map in the plan. Paint layers and masks without a source get
  a pending `Transform` operation into a new local frame that holds the whole
  moved extent; photos move their placement, and the selection, Selection Layers
  and guides are transformed as metadata. Flips and quarter turns sample with
  `Nearest`, so they move pixels exactly. Source and destination share one tile
  grid while the operation runs, so a quarter turn of a non-square layer draws
  into a square extent.
- A `Transform` whose selection takes every pixel its target holds (no
  selection, or a rectangle around all of its pages) leaves the pages outside its
  forward bounds empty. The renderer drops them, in every plane, from the GPU and
  from the data the next publication copies, so a reduction or turn publishes no
  transparent tiles and they don't count toward the tile limit. A photo keeps its
  pages, since they cover its original. A mask's transform creates the pages it
  draws, so its source is only the pages the mask held.
- Delete Cropped Pixels trims each paint layer and mask to the tiles its window
  touches, rebases them to the smallest tile-aligned extent, and erases the edges
  of paint layers with up to four bounded `Erase` operations, so only edge tiles
  are rewritten.
- Pixel operations run in the same undo step as the metadata edits
  (`CanvasEngine::apply_canvas_geometry`), on locked layers too. A target whose
  raster the batch replaced is restored from that raster before its operations run.

A canvas size change resets the renderer's paint pages and restores every layer
from its raster revision, including on undo and redo
([`tests/canvas_geometry.rs`](../../crates/layer-render-wgpu/tests/canvas_geometry.rs)).
Limits are checked before the edit commits, including the device's texture limit
through `CanvasRenderer::max_document_dimension`.

### Merges

A merge inserts its result with a pending `LayerOperationKind::Bake` holding the
merged layers as they were. The frame that runs it composites them with
`Scene::group` over transparency, isolated and moved into the result's pixels,
and copies each tile into the result's pages
([`scene/bake.rs`](../../crates/layer-render-wgpu/src/scene/bake.rs)). Placed
photos are sampled as for export, never from the display's mip levels, and
watercolor settles into the result, which keeps no wet state. The same edit
removes the merged layers, so the engine appends them, hidden, to that frame's
layers, and the renderer keeps their pages, masks and photo tiles until the bake
has run (`with_bake_members` in
[`canvas.rs`](../../crates/layer-engine/src/canvas.rs)). The bake is frame work
on the render owner; the UI thread only plans it.

The Crop tool's shield is drawn by the presentation pass itself. `set_crop_overlay`
passes a `CropOverlay` (the map from document pixels onto the crop's unit square,
and the shield opacity) to the renderer, which folds it into the presentation
uniform. `present.wgsl` dims the canvas outside the crop and fills the part of the
crop beyond the canvas with the transparency checkerboard; it never samples pixels
hidden beyond the canvas. Without a crop the shader skips both on a uniform flag,
and a handle drag changes only that uniform and the guide segments, so it costs a
camera-change repaint. The frame, guides and handles are ordinary `CursorSegment`s.

The viewport is the presentation of that document at the current camera position,
zoom and rotation. The shared
[`ViewportPresenter`](../../crates/layer-render-wgpu/src/present.rs) samples the
composed image into the platform's target. Panning the view does not, by itself,
require repainting committed raster tiles.

Viewport presentation and staging uploads return mapping failures to their host.
A device removed during buffer allocation must not unwind the render owner; the
host can reconstruct its GPU while retaining the shared document session. Uploads
continue to reuse the staging belt and never wait for GPU completion on the UI
thread.

## Native SDR working color

The headless native-document factory is currently a qualification route. GTK's
exposed factory remains sRGB8 until the complete color/photo workflows and memory/
latency gates pass. Native integer8/integer16 documents use their selected RGB
primaries, Float32 color working attachments and Float32 scalar coverage. Native
commit publication quantizes affected pages into the declared backing depth;
view transformations do not change those pages.

[`working_color.wgsl`](../../crates/layer-render-wgpu/src/working_color.wgsl) shares
unassociation, interpolation and perceptual conversion across scene composition,
effects and materials. Positive alpha is divided directly; only zero coverage
returns black. Native scene/image interpolation uses explicit Float32 texel loads.
Ordinary source-over and the existing channel blend formulas operate in **linear
document RGB**, independently of bit depth. Each blend mode states its own bounds;
see [Blend modes](#blend-modes). Native Oklab material mixing converts through
linear sRGB/D65 (including document-white adaptation), uses signed cube roots,
then returns to document primaries without a blanket negative-RGB clamp. Oklab
endpoints retain the selected operand. Region tolerance uses encoded document RGB
weighted by coverage, independent of display/checker colors.

Watercolor keeps its water activation threshold as a material-model parameter.
That threshold no longer rejects faint native pigment. Native transport constrains
coverage while retaining extended RGB between commits; it does not clamp RGB to
alpha. These helpers are not a claim that all brush dynamics and nonlinear tone
controls are fully qualified.

[`view_color.rs`](../../crates/layer-render-wgpu/src/view_color.rs) converts the
composition to explicitly declared sRGB, Display P3 or extended-linear sRGB view
coordinates. Surface format controls output transfer encoding. Application colors
in viewport overlays retain their sRGB definitions. Export/Navigator thumbnail
readbacks are explicitly sRGB8; exact color samples retain document RGB. Native
profiled delivery is a separate output conversion and is still being integrated.

## Blend modes

`LayerBlend` has 24 modes. Each variant's discriminant is its *code*: Normal,
Multiply, Screen, Add, Overlay, Soft Light and Color are 0 to 6, and Darken to
Luminosity follow from 7. Documents store the variant name, not the code.
`LayerBlend::MENU` groups the modes as Photoshop does (Normal, darken, lighten,
contrast, inversion and component modes) for the menus described in
[shared UI](../ui/shared-ui.md#layer-blend-menu).

[`blend_modes.wgsl`](../../crates/layer-render-wgpu/src/blend_modes.wgsl) holds every
formula, on straight colors, and two premultiplied helpers: `blend_composite` puts
a source over its backdrop and `blend_clip` blends a clipped source inside its
base's coverage. They serve every place a layer's blend applies:
- layers, groups and clipping stacks (`scene.wgsl`, op 4), including the cached
  clipping composition of an image filter (`scene_images.rs`);
- a clipping stack's final composite over a constant backdrop, folded into its
  last adjustment (`effects.rs`);
- effect layers over their input (`fx_adjustment` in `effects_color.wgsl`);
- the layered display that places a moving layer during a transform drag
  (`display_layers.wgsl`);
- brushes with a blend mode other than Normal (`material_brush.wgsl`), with the
  brush's mode mapped to the layer mode of the same name. The Normal brush keeps
  its direct source-over form because the general expression draws dark contact
  edges on an Adreno Vulkan driver.

`blend_code` passes a blend to shaders as the mode's code in bits 0-7, the
Perceptual blend space in bit 8 (reserved; always 0 for now) and float documents
in bit 9. Normal is always 0.

**Ranges.** Float documents clamp no result. Modes defined only on [0, 1]
(`BlendRange::Unit`) clamp their operands to [0, 1] in every document, and float
documents leave them out of their menus; a layer that already uses one keeps it.

| Mode | 8- and 16-bit documents | Float documents |
| --- | --- | --- |
| Normal, Multiply, Darken, Lighten, Difference | Unbounded | Unbounded |
| Add | At most 1 | Unbounded |
| Subtract, Linear Burn | At least 0 | At least 0 |
| Linear Light | Clamped to [0, 1] | At least 0 |
| Pin Light | Clamped to [0, 1] | Unbounded |
| Divide | `d / max(s, 2⁻¹⁴)`, at most 1; a zero source gives 1 over any color and 0 over black | `d / max(s, 2⁻¹⁴)` |
| Screen | `s + d − s·d` | `s + d − min(s, 1)·min(d, 1)`, which keeps rising above 1 |
| Overlay, Soft Light, Hard Light, Color Burn, Color Dodge, Vivid Light, Hard Mix, Exclusion | Unit | Unit, not offered |
| Hue, Saturation, Color, Luminosity | W3C `ClipColor` | `ClipColor` without its upper bound |

Soft Light is the W3C formula. Hard Mix is 1 where `s + d ≥ 1`, which matches
Photoshop's threshold of Vivid Light. The component modes use W3C `SetLum`,
`SetSat` and `ClipColor` with the luma weights of the document's primaries (the Y
row of their XYZ matrix). Bit 8 selects Rec. 601 weights, for Photoshop parity
on encoded values in the Perceptual space.

**Existing documents.** These rules change how some saved documents look:
- Color layers weigh luminance by the document's primaries instead of Rec. 601,
  in every document, and no longer clip above 1 in float documents.
- In float documents, Add no longer clamps at 1, Screen follows its extension
  above 1, and Overlay and Soft Light clamp operands outside [0, 1].
- Unit modes clamp operands outside [0, 1] in 8- and 16-bit documents too; there
  only effect layers produce such values.

The oracle `every_blend_mode_matches_the_reference_on_every_path_and_depth`
checks every mode against an independent reference through each path above at
8-bit, 16-bit, half-float and float depths, and checks that export renders the
same composite.

## Incremental composition

The *compositor* combines paint and image layers, groups, masks, clipping and
blend modes. Its [scene code](../../crates/layer-render-wgpu/src/scene.rs) tracks
*damage*: regions whose previously rendered pixels are no longer valid.

A simple paint update rasterizes new dabs into the relevant layer pages and
recomposes affected regions. More complex layer structures need intermediate
results. The renderer retains those results and tracks their dependencies so,
for example, changing a clipping layer does not force unrelated source images
to be rebuilt.

These dependencies branch: a masked adjustment needs the original image, its
filtered result and the mask. Other layers contribute separately to the final
composite. The [README's tile example](../../README.md#rendering-engine) shows a
clipped adjustment between paint and ink. Editing its mask changes the adjustment
and subsequent composition; it does not change the stored paint, ink or background.

[Image-stage caching](../../crates/layer-render-wgpu/src/scene_images.rs) handles
operations that need reusable image inputs. A blur needs pixels outside its output
rectangle, so filter definitions describe their sampling footprint. Global effects,
layer reordering and invalidated caches can require much larger updates than a
single brush mark. Animated effects also need updates without new pen input.

A transform or placement drag skips both the full-resolution pages and
composition when a native document keeps a complete display pyramid and the
moving layer is an unmasked top-level layer under normal static layers.
[`render_display`](../../crates/layer-render-wgpu/src/paint_transform.rs) draws the
moving layer into the level the view samples, at most sixteen layer pixels per
texel side, and the coarser levels are reduced from it. While the Transform is
still, the layer is reduced once per transaction as the exact area mean of its
full-resolution pixels, to the level of its own pixels that matches the display
under its placement. It is reduced a page at a time, decoding a photo's
original tiles as it reaches them. When the selection keeps some pixels in
place, those are reduced apart from the pixels it moves: a page the selection
covers or leaves out entirely is reduced whole, and one its edge crosses is
drawn exactly. A transform that keeps its source, as Move's Leave Copy does,
cuts nothing: its `keep_source` flag makes
[`pixel_transform.wgsl`](../../crates/layer-render-wgpu/src/pixel_transform.wgsl)
leave the original under the moved pixels, and drag frames add the moved
pixels back to the kept ones in place. A whole placed photo is copied instead
from its placement preview when that preview is current and holds the level.
Move opens its transaction at the press, so while Move is active over a
selection the session names the layer and selection a press would move
(`prepare_moving_pixels`), and idle frames capture and reduce them ahead of the
drag; its transaction adopts them when the layer's pixels and the selection are
unchanged, so its first frames draw at once. Paint, a restore or leaving Move
drops them.
Each drag frame
[resamples](../../crates/layer-render-wgpu/src/paint_transform/resample.rs) the
copy with one bilinear sample per texel, the moved pixels through the transform
and the placement and the kept ones through the placement alone. With content
above or below, the
[layered display](../../crates/layer-render-wgpu/src/paint_transform/layers.rs)
composes the static layers once at that level, then places the moving layer
between them with its blend. They are kept for the moving layer and level
while nothing else changes, across drags and transactions. A drag waits for
them and the reduced copy, and so does the frame that ends a drag that waited.
The frame that releases a drag resamples the still preview too. Later frames
draw the preview's pages at full resolution and then recompose what the drag
touched a few tiles at a time, reporting pending work so hosts keep drawing.

A Warp transform is a [mesh](../../crates/layer-render-wgpu/src/paint_transform/mesh.rs)
of Bézier patches. Its pages are drawn a window of four by four pages at a
time: the mesh, tessellated within half a pixel and extended by a skirt past
its edges, is first rasterized into a texture of the source position at each
destination pixel, and the transform pass samples the original there,
averaging a pixel's footprint from its neighbors' positions where the mesh
shrinks it. A region job binds only the source under the part of each
tessellated triangle it covers, so the large flat triangles of an unbent patch
still split into jobs within the pass's texture bindings. Paint, masks and a selection's moved pixels draw this way in the
preview and when applied. A drag rasterizes the mesh at the display level's
texels instead, tessellated within half a texel, and resamples the reduced copy
at those positions. A pixel selection moved by a warp is resampled on the GPU
the same way, a window at a time. What draws meshes compiles in the background
when a warp is first shown, and until then the preview keeps the frame before
it.

A [placement drag](../../crates/layer-render-wgpu/src/placement_drag.rs) is a
frame in which only one layer's placement changed. Its layer's own pixels are
reduced once and kept between drags while they are unchanged, together with the
static layers around it. Until that copy is complete, a lone layer over the
paper is drawn from the display level as the drag began, within the canvas that
level showed. Each drag frame resamples the copy through the new placement, and
the frame's full recomposition is skipped. Frames in which nothing moves keep
the drag; once the placement has stayed still for a few frames, what the drag
drew is recomposed a few tiles at a time. What drags draw with and what
composes placed layers compile in the background while input is quiet after
startup, and that recomposition waits for them.

[Preparation](../../crates/layer-render-wgpu/src/preparation.rs) for a drag and
the work after one, including the layer's copy, the static layers around it, a
still preview's settled pages and the recomposition, is spread over frames by
the GPU time that earlier work of the same kind took, measured with timestamps.
Each measured frame sets the next frames' units from its own cost per unit, to
fit 10 ms of GPU time for work a drag waits for and 5 ms for work after a
release. Timestamps arrive a few frames late, so a count never grows past twice
what the measured frame was allowed, and late measurements do not compound. A
frame also stops preparing once its preparation has taken 4 ms of CPU time,
after at least one unit of each kind; without timestamps that deadline alone
sets how much a frame prepares. A drag that starts meanwhile waits behind at
most a frame of that work. Neither drag frames nor the frame that ends a drag
allocate pages: once the layer is reduced, still frames reserve the pages its
preview settles into, a few each, and settling waits for them.

Drag frames and still previews until they settle resample; pages, Apply,
commits and the settled display are exact.

### Resampling

The [transform pass](../../crates/layer-render-wgpu/src/pixel_transform.wgsl)
samples premultiplied linear pixels and their selection together. `Nearest`
takes the pixel under the sample. `Linear` is bilinear. `Bicubic` is
Catmull-Rom over 4 × 4 taps and `Lanczos` is Lanczos-3 over 6 × 6 taps with
normalized weights; both clamp their overshoot to the range of the four nearest
taps, keep colour at most alpha times the brightest straight colour among them,
and clamp scalar planes to [0, 1], so neither rings past the edges it sharpens.
Where the map minifies, a destination pixel instead averages a grid of bilinear
taps spread over its footprint, as many per axis as source pixels it spans. Each
record carries a cap on that count: drag previews use at most four, and commits,
still previews that Apply may keep, and exact capture use up to sixteen, derived
from the map's Jacobian. A reduction to an eighth therefore averages every
source pixel, as an area reduction would, instead of aliasing. A moving
Bicubic or Lanczos preview draws bilinearly until it stops.

Exact capture (export, snapshots and the artwork readback) draws placed photos
through the same pass with the exact cap, and with `Bicubic` where the placement
magnifies. The live display samples the photo's placement mips instead, and the
fused display path stays bilinear.

## Filters

Filters are stored as effect layers in the document. An adjustment processes the
combined image beneath it within its group. Clipping restricts it to a clipping
stack, and the effect layer's mask and opacity control its influence. Successive
adjustments receive the result of the earlier ones. Generator effects instead
produce new image content that participates in ordinary layer composition.

A clipped adjustment processes its clipping stack before that stack is combined
with the unrelated background. Its mask and opacity mix the original and adjusted
colors while preserving the base layer's coverage. Applying the effect to the
already flattened image would incorrectly include the background. Treating the
adjustment as another ordinary paint layer could also increase opacity where it
overlaps the original.

A runtime filter consists of a JSON definition and WGSL shader code. The definition
describes its parameters, inputs and execution requirements. Shared code validates
it, supplies the control descriptions and prepares the GPU pipeline. The same runtime handles
built-in and imported filters.

Curves and Gradient Map retain specialized Rust interpolation and lookup-table
preparation. They should not be used as examples of a filter that can be expressed
entirely by adding a JSON/WGSL pair.

The [runtime filter reference](../reference/runtime-filters.md) is the contract
for implementing a filter. The [Tent Blur example](../../examples/filters/tent-blur)
provides a small package to study.

## Shader fusion and intermediate images

A pointwise adjustment computes each output pixel from the input at that position.
Compatible chains of these adjustments are fused into a single fragment shader by
[`effects.rs`](../../crates/layer-render-wgpu/src/effects.rs). Each adjustment can
consume the previous one's result without writing it to a texture first. Ordinary
aligned masks can be sampled in the same shader, preserving each effect's mask,
opacity and blend rules. Binding limits and incompatible operations can split a
chain into separate passes.

This reduces intermediate allocations, memory traffic and pass setup. Some
composition steps can also be folded into the effect shader, and tile draws that
share an attachment can share a render pass. Fusion changes execution, not the
order or scope of the layer operations.

Filters that sample neighboring pixels need a different path. The
[image-stage implementation](../../crates/layer-render-wgpu/src/scene_images.rs)
retains reusable GPU images, tracks input changes and reuses compatible preceding
results where possible. Each image has document-coordinate bounds independent of
its texture size. Region capture expands its window by the accumulated declared
filter support, and applies the existing group, clipping, mask and effect logic
inside that window. Shaders keep document coordinates for their calculations;
current-pass and original-input sampling each carry their own texture origin.
Native one-to-one image reads use fragment positions directly, avoiding a
window-size-dependent interpolation error from reconstructed UV coordinates.

Filter previews scan four source tiles per asynchronous completion, including
the probe's corner-sampling halo. Preview rows then share a source crop expanded
by their required support. A document edit cancels an unfinished scan after its
in-flight completion; no result may combine source revisions. Global samplers
retain their full declared input. Their scheduling and allocation limits still
need qualification. Live composition also still uses full-document image stages
and a full composite. Sparse storage and cropped previews therefore do not yet
bound the total cost of large filters or densely painted documents.

The native [snapshot renderer](../../crates/layer-render-wgpu/src/snapshot.rs)
prepares document metadata independently of the live full composite. A file or
inspection worker owns an immutable project snapshot and a native Float32
renderer. Region requests restore only the translated paint, material and mask
pages needed by composition and its halos. Compressed backing remains shared;
restoration uses the same integer decoder as live editing. Initial masks use the
existing GPU crossing/coverage and affine-resampling shaders, with bounded
output rectangles and row slices of immutable packed selection coverage.

Snapshot PNG/TIFF output streams sixteen-row strips through the working-color
encoder and profiled row writers. A matching, unmodified source with default
conversion and no matte bypasses composition to preserve exact integer samples,
including hidden straight RGB. An explicit matte composites in linear document
RGB before encoding. Region captures return linear-premultiplied document values;
they contain no viewing or mask-area overlay. Cancellation and writer failures
return errors; the caller must publish its temporary file only after success.

Capture dependency plans have an explicit byte ceiling checked before restoring
pixels. This planning limit is separate from measured peak process/device memory,
codec buffers and retained compressed sources. Global samplers can exceed it and
still need their scheduled, qualified route. Snapshot capture/output is currently
a headless worker API; GTK export UI, progress/cancellation ownership and recipes
still need integration. This does not yet replace live composite residency.

## GPU resources and unified memory

Committed edits queue exact readback of changed 256² tiles for raster history
and persistence. Mapping and lossless compression run on a bounded worker; GTK
input never waits on the GPU. Move frames and presentation do not perform full
canvas readback. The exposed host export still requests a full image; the native
snapshot API streams bounded strips. Thumbnails and color sampling use separate
bounded requests. Source bytes and immutable compressed
tile backing are shared with save snapshots. See the [raster project contract](../reference/project-format.md).

On unified-memory hardware, CPU and GPU share physical RAM. Keeping separate
copies solely to move an image between processors can waste both memory and
bandwidth; older pipelines built around discrete GPU memory need to account for
this. [Apple's image-processing guidance](https://developer.apple.com/videos/play/wwdc2021/10153/)
describes those costs and the opportunities to remove redundant copies.

Here, "GPU resources" describes how the renderer accesses the pixels, not a
requirement for separate VRAM. Textures still have access rules and layouts,
and CPU/GPU coordination still matters. Capy Canvas keeps the drawing pipeline
in GPU resources through composition and presentation; imports, readbacks and
platform APIs may still need staging or copies.

## Startup

GPU preparation is staged so the platform can display its controls first.
Startup prepares the paper, then the open document, then the selected brush and
eraser; unused brushes and filters compile on first use
([shader readiness](shared-shader-readiness.md)). Required
dependencies are ready before drawing uses them, so pipeline creation never lands
in a small stroke update. A contact that begins before its brush is ready stays
suppressed until release.
Changing paint color or HDR intensity leaves brush readiness intact when the
tip, texture assets and shader pass requirements stay the same.

On Web, GPU initialization waits for the workspace (at most 1 s), and pipelines
are created through the asynchronous WebGPU APIs, a few at a time: synchronous
creation blocks Chrome's GPU process and display callbacks even when JavaScript
yields between jobs. An edit that needs a pipeline whose asynchronous compile is
still in flight creates it synchronously and drops the asynchronous result
(`node apps/layer-web/test.mjs --headless --pipeline-takeover`).

For brush-specific passes, continue with [Brushes](brushes.md). For performance
work, use the [measurement guide](../development/testing.md#performance).
