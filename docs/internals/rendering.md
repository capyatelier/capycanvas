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

Paint and coverage pages live in explicit source-local domains,
`PaintSource.domain` and `CoverageSource.domain` in the
[authored model](../reference/authored-model.md). A placed photo also retains its
independent original extent. `SceneView::local_extent` and `target_extent` expose
these domains in [`authored/scene.rs`](../../crates/layer-core/src/authored/scene.rs).
The composition frame is a window over those pixels; changing the canvas does not
discard pixels outside it. Retained pixels count toward tile and byte limits.

`Rect::from_extent` gives the local pixel bounds used by geometry, merges and sampling.

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

A merge plans a `RasterOperationKind::Bake` with an immutable `SceneSnapshot`
and explicit `SceneScope`. Its `SourceTarget` selects the new paint source. The
frame composites the retained scope over transparency, isolated and moved into
the result's pixels, in the document's blend space. It copies each tile into the
result's pages, decoded to linear pixels in a Perceptual document so the result
looks the same ([`scene/bake.rs`](../../crates/layer-render-wgpu/src/scene/bake.rs)).
Placed photos are sampled as for export, never from the display's mip levels, and
watercolor settles into the result, which keeps no wet state. The authored edit
removes the merged occurrences; the accepted bake snapshot retains their sources,
masks and original image tiles until the job finishes. The bake runs on the
render owner; the UI thread only plans it. Large bakes advance through
512 × 512 regions. Each submission captures its native tiles before cache
eviction can discard them. Intermediate native backing belongs to the renderer;
history publishes the complete edit after the last region. Hosts poll readiness
without waiting on the GPU and keep the previous artwork visible during the bake.

A bake keeps no filter images. When its members' filters would need more
image memory than the default image budget, it runs them in the bounded windows
the display uses for filters (`windows::Plan`), each with its filters' halos
and retired before the next, so a windowed bake stores the same pixels as a
whole one. Pages keep only the part inside the result's extent, with coverage
kept within 0–1, which a blur of translucent pixels can round past. A new,
empty layer that an operation writes into reserves in history only the pages
it can write (`RasterRevision::pending_within`).

Frequency Separation bakes Low with an attached Gaussian blur, then computes High
from the original and the captured Low with `FrequencyDetail`. High stores
`0.5 + (original - Low) / 2` in the document's blend space; its Linear Light blend
reconstructs the original. The blur runs once, and High uses Low's native
quantization. Both layers fit one undo step because they reserve only their
pages. The dialog's blur preview is
an immutable typed preview scene supplied by `CanvasEngine::set_scene_preview`.
`ScenePreview` retains that scene and the target occurrence; it does not add an
authored occurrence or history entry to the live document.

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

### Image object layers

Object layers compose ordered immutable images over transparency before applying
the layer mask, attached effects, opacity and clipping. Both the native tile
compositor and the display graph evaluate this content directly; an object result
has no writable `SourceTarget`. `SceneScope::RawObjects`, `ArtworkSource::Objects`
and `RegionSource::Objects` select its internal pixels independently of the
owner's visibility, mask and presentation properties.

Object affines are composed and inverted in binary64 before projecting into a
nearby output window. The image reader accepts explicit output origin and pixel
density, including densities above native resolution. Source decoding precedes
premultiplied working-linear interpolation. Ordered composition converts each
sample into the document's blend domain at the composition boundary.

Nearest prepares exact binary64 floor coordinates on a worker in chunks of at
most 65,536 output pixels. Geometry is keyed by output mapping and window
independently of source identity, retained across source-tile contributions and
uploaded for GPU level-zero gathers. Linear evaluates
the scale-adapted tent over its complete lattice support, including transparent
samples beyond the image rectangle in the normalization. Decoded immutable source
tiles share the bounded source cache across objects and paint bases; affine edits
keep those source entries. Object metadata includes ordered children, image
identity and interpretation, visibility, affine and interpolation. Edits damage
the old and new bounds with sampling and declared effect support.

Moving smooth display can use a temporary bilinear result from shared immutable
image mip levels. The source cache reserves their storage from its byte budget;
native source preparation runs on a worker and builds bounded tiles between
frames. Image identity and source interpretation select the pyramid independently
of object placement. Nearest continues reading level zero.

Canonical object collections accumulate privately across frames, consuming children
in their authored order. A window retains one child's sampling state and three
rotating surfaces for the sampled child, its converted color and the isolated
collection prefix. Each child enters the selected composition color space before
source-over. Only the complete collection becomes a reusable result. This bounds
working storage independently of the number of overlapping children.
Ready mip previews admit at most 64 dispatches across the complete collection
window and share one queue-ordered transient sampling workspace. A private batch
records at most eight sampling/composition steps and waits for GPU completion
before another batch is recorded. Each private cache admits buffers, output
textures, prepared geometry and queued metadata within 64 MiB. Live display and
exact snapshot queries use separate caches, so concurrent evaluators can retain
up to two such allowances. Resources retired by an edit remain charged through
GPU completion. Completed results remain reusable across deferred frame retries
until their copies are encoded. Hidden owners cancel display work; raw object
queries retain their independent visibility semantics.
Replacing an affine retires its unfinished result. Direct coarse completion damages
its evaluated density; native completion also invalidates consumers of native image
inputs. Main and overview refinement preserve each other's completed pixels. Canonical
captures use the authored kernel, submit bounded worker chunks and check capture
cancellation between chunks. Display approximations never become capture inputs.
Snapshot region planning defers pending object work to a separate exact-result
cache. Each drain records at most eight sampling steps, submits and awaits GPU
completion, then checks cancellation before resuming. Successful source commands
remain in queue order during retries; histogram consumers and final readback run
only after the requested pixels are complete. The live compose context follows
filter input captures so those captures retain the display scheduler. Quiet retouch
and thumbnail reads use the prepared-source worker and private scheduler with the
canonical kernel. A pending read preserves its request and resumes after submitted
work; it cannot decode image pixels on the initiating thread. Object captures
encode and release completed results one native page at a time.
Native filter input gathers retain completed pages across these retries; a
partial input never marks its filter output valid. Authored edits, new contacts
and changes to input interpretation discard that unfinished gather.
Before a cold live object can interrupt composition, the renderer copies the
accepted viewport inputs on the GPU and retains their geometry. Source uploads,
paint and private sampling can submit while later filter windows await prepared
image tiles. The viewport uses those retained inputs until a complete composition
publishes; its evaluated object revision advances with that publication. Completed
internal regions remain valid through submitted retries and release copied
private results. Discarded submissions still invalidate their cache writes. Jobs
that defer release their abandoned native scratch slots and material-page
bindings before the next attempt. Completed GPU commands retain their resources;
the reusable scratch pool does not grow with the number of cold retries.
Retained pixels remain charged through GPU completion. Web worker completion wakes the
host when a tile is available; native hosts retain paced pending-work polling.

Eight-bit canvas surfaces use deterministic triangular dithering after resampling
and display color mapping. The noise uses logical screen coordinates and the
encoded attachment domain. Noisy values are rounded to byte code centers before
attachment conversion, including the decode for hardware sRGB attachments. It
does not change artwork or exports. Floating surfaces, window edges, surrounding UI and
diagnostic overlays retain their existing values. Source gradient dithering
remains in document coordinates so saved pixels do not depend on the camera.

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
Before interpolating extreme components, the sampler scales both endpoints by
one binary exponent and bounds the result to their interval before restoring
that scale. Opposite finite HDR endpoints cannot overflow their difference.
Layer pixels are **linear document RGB**, independently of bit depth. Layers
combine in the document's [blend space](#blend-space): linear document RGB, or
the document's encoded values in a Perceptual document. Each blend mode states
its own bounds; see [Blend modes](#blend-modes). Region tolerance uses encoded document RGB
weighted by coverage, independent of display/checker colors.

Paint color mixing (`mix_color` in
[`material_brush.wgsl`](../../crates/layer-render-wgpu/src/material_brush.wgsl):
Smudge and blender pickups, the Wet reservoir exchange and Watercolor's
same-layer exchange) follows the brush's Color mixing choice (`ColorMixSpace`),
never the document's blending. The choice is a style uniform, so no brush
pipeline is specialized for it:

- **Oklab** converts through linear sRGB/D65 (including document-white
  adaptation), uses signed cube roots, then returns to document primaries
  without a blanket negative-RGB clamp.
- **Linear light** mixes linear document RGB.
- **Classic** mixes values encoded with the document's own transfer curve
  (`sdr_encode` and `sdr_decode` in `sdr_color.wgsl`), as Clip Studio Paint does.

A mix at either endpoint returns that operand exactly in every space.

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

`LayerBlend` has 24 pixel blend modes and Pass Through for groups. Each variant's discriminant is its *code*: Normal,
Multiply, Screen, Add, Overlay, Soft Light and Color are 0 to 6, and Darken to
Luminosity follow from 7. Documents store the variant name, not the code.
`LayerBlend::MENU` groups the modes as Photoshop does (Normal, darken, lighten,
contrast, inversion and component modes) for the menus described in
[shared UI](../ui/shared-ui.md#layer-blend-menu).

[`blend_modes.wgsl`](../../crates/layer-render-wgpu/src/blend_modes.wgsl) holds every
formula, on straight colors, and two premultiplied helpers: `blend_composite` puts
a source over its backdrop and `blend_clip` blends a clipped source inside its
base's coverage. They serve every place a layer's blend applies:
- layers, groups and common-base clipping runs (`scene.wgsl`, op 4);
- effect interpolation, preserving or filtering alpha as declared
  (`fx_adjustment` and `fx_filter` in `effects_color.wgsl`);
- region and scale composition, including transform previews
  (`scene/scale/compose.wgsl`);
- brushes with a blend mode other than Normal (`material_brush.wgsl`), with the
  brush's mode mapped to the layer mode of the same name. The Normal brush keeps
  its direct source-over form because the general expression draws dark contact
  edges on an Adreno Vulkan driver.

`blend_code` passes a blend to shaders as the mode's code in bits 0-7, the
Perceptual blend space in bit 8 and float documents in bit 9. A layer's Normal is
always 0; a brush sets bit 8 for Normal too, because its dabs lay over paint in
the document's blend space ([brushes](#brushes-and-healing)).

**Pass Through** has no pixel blend formula. The shared traversal in
[`scene/stack.rs`](../../crates/layer-render-wgpu/src/scene/stack.rs) composes a group's layers
onto a running composite, so a Pass Through group
([documents](documents.md#groups-and-pass-through)) continues its parent's
composite with its own layers, and a composition that stops before a layer
(`stop_before`) stops inside it too. At opacity below 1 or with a mask, the group
retains the backdrop and then fades to its result,
`backdrop + opacity × mask × (result − backdrop)`. Exact tiles and reduced graph
expressions evaluate the same weighted sums and coverage product. Direct clipping
and effect attachment require an explicitly isolated group; admission rejects a
Pass Through group with those relationships.

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

Soft Light is the W3C formula in Linear documents and Photoshop's in Perceptual
ones: with backdrop `a` and source `b`, `2ab + a²(1 − 2b)` where `b ≤ 0.5`, else
`2a(1 − b) + √a(2b − 1)`. Hard Mix is 1 where `s + d ≥ 1`, which matches
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
8-bit, 16-bit, half-float and float depths, in both blend spaces at 8 and 16
bits, and checks that export renders the same composite.

## Blend space

A document's Blending (`Document.blend_space`) is **Perceptual** or **Linear
light**. Layer pixels are linear premultiplied in both. In a Perceptual document
the *composite* holds the document's encoded values, premultiplied:
`enc(c / a) · a`, where `enc` is the document's transfer curve (`sdr_encode` with
`WORKING_SPACE`): sRGB for sRGB and Display P3, 563/256 for Adobe RGB, and 1.8
with a linear toe for ProPhoto. Only 8- and 16-bit documents blend perceptually;
float documents are Linear. `FramePacket.scene.composition().blend` supplies the
domain to the renderer, and a change recomposes everything.

Normal layers keep hardware premultiplied blending, which is correct on encoded
values. Blend modes, clipping, masks, opacity and groups work on the encoded
values directly. Scene draws convert behind a uniform; effect shaders are
compiled once per blend space, because a branch in every filter pixel costs a
Linear document time. Either way a Linear document runs the arithmetic it ran
before the setting existed. Resampling stays linear: layer pixels are sampled or
reduced first and converted after.

**Writers.** These put layer pixels into the composite and encode them:
- scene draws of layer pixels: `scene_space` in
  [`scene.wgsl`](../../crates/layer-render-wgpu/src/scene.wgsl) and
  `scene_constant.wgsl`, selected by the draw's `Convert` (ops 1, 7, 12, 13,
  14 and 15, and the mask-area tint, op 5). A Flow preview lies on its layer's
  pixels before they are encoded, as its committed stroke will;
- placed layers and watercolor layers, which the renderer draws linear first
  and then converts in one draw (`Scene::converted`);
- effects ([`effects.rs`](../../crates/layer-render-wgpu/src/effects.rs)): a
  filter that reads linear values receives decoded input and its result is
  encoded before it blends onto its input, inline in fused chains and in the
  last pass of an image filter. A filter that follows the document's Blending
  ([filter spaces](../reference/runtime-filters.md#filter-spaces)) reads and
  writes the composite's encoded values with no conversion;
- constant fill colors and backdrops, encoded on the CPU
  (`BlendSpace::composite`);
- drag frames that draw a moving layer into the display
  ([`display_resample.wgsl`](../../crates/layer-render-wgpu/src/display_resample.wgsl)
  and `display_main` in `pixel_transform.wgsl`), which encode the layer's
  resampled color when `DisplayLevel::encode` is set.

**What holds the composite.** Group and clipping scratch tiles, retained display windows and their mips,
image-filter outputs and checkpoints
(`scene_images`), retained graph branches, the folded constant backdrop, and the backdrop
and result a Pass Through group fades between hold the document's composite
values. Their caches include the blend space
(`ImageStages`, `artwork::Frame`, the filter-preview source key and the
retouch reference cache's key). An image
filter's input window is captured once per pixel in the filter's declared space:
decoded for a filter that reads linear values, as the composite holds it for one
that follows the document's Blending. Adjacent filters share an image only when
the upper one reads the composite as it is.

**Readers.** Each decodes where it needs linear values:

| Reader | Where | Reads |
| --- | --- | --- |
| Canvas presentation, Navigator, color picker loupe, screen check, backdrop blur | `canvas_linear` in [`present.wgsl`](../../crates/layer-render-wgpu/src/present.wgsl) and `present_screen.wgsl` | decoded before proofing and SDR or HDR mapping |
| Export and snapshot rows, Copy and Copy Merged, histogram, color conversion previews | `Scene::capture_region` in [`snapshot.rs`](../../crates/layer-render-wgpu/src/snapshot.rs) | decoded per tile |
| Eyedropper, Wand and Fill on the composite or the reference layers, tonal selection, whole-image readback | [`artwork.rs`](../../crates/layer-render-wgpu/src/artwork.rs) captures | decoded |
| Clone Stamp, Healing and Spot Healing: the reference layers below the target | reference cache in [`retouch_sources.rs`](../../crates/layer-render-wgpu/src/retouch_sources.rs), captured through `artwork.rs` | composed in the document space, cached decoded; `retouch_source` in [`retouch_sample.wgsl`](../../crates/layer-render-wgpu/src/retouch_sample.wgsl) lays the target over them in the blend space |
| Filter previews | `capture_filter_source` in [`filter_previews.rs`](../../crates/layer-render-wgpu/src/filter_previews.rs) | decoded; encoded once more for filters that follow the document's Blending, whose previews decode their result |
| Merges | [`scene/bake.rs`](../../crates/layer-render-wgpu/src/scene/bake.rs) | composed in the document space, stored decoded |
| Image filter input windows | `capture_tile` in [`scene_images.rs`](../../crates/layer-render-wgpu/src/scene_images.rs) | decoded, or composite values for filters that follow the document's Blending |
| Layered display during drags | [`scene/scale/compose.wgsl`](../../crates/layer-render-wgpu/src/scene/scale/compose.wgsl) | composite values; blends with the document's blend code |
| Layer thumbnails, brushes | layer pages | linear layer pixels, not the composite; brushes encode them to lay dabs over them ([brushes](#brushes-and-healing)) |

The export matte, and resizing on export, apply to the decoded rows in linear
light.

### Brushes and healing

Layer pixels stay linear, but in a Perceptual document a dab lays over paint on
the document's encoded values, as in Photoshop and Clip Studio Paint: soft edges,
opacity and flow build up in the blend space. `source_over` in
[`material_brush.wgsl`](../../crates/layer-render-wgpu/src/material_brush.wgsl)
serves the dry deposit and coverage passes, the retouching deposit, the Wet
deposit and Watercolor's final deposit; the pass encodes a destination pixel the
first time a dab deposits on it, lays the dabs over it, and decodes it once. Dry
dabs are uploaded with their colors already encoded (`prepare_colors` in
[`dry_material.rs`](../../crates/layer-render-wgpu/src/dry_material.rs)), so no
pixel converts a dab's color.
At native resolution, in-place uniform Normal deposition checks retained stroke
coverage before reading color. Fully covered pixels keep their existing color;
the coverage pass still copies their scalar coverage into its next output.
Stroke starts, bristles and other blend modes retain their ordinary evaluation.
Brush blend modes call the layer formulas in the same space, so an Overlay brush
matches an Overlay layer. The brush's blend code carries the space (bit 8), a
uniform, so no pipeline is specialized for it. Edged brushes that Linear light
documents draw with hardware blending (`BrushPassPlan::direct`) run the material
pass instead, and their kernels compile before pen-down. `DabStyle.blend_space`
is the document's Blending for artwork and Linear for masks and selections; a
stroke keeps the space it started with for replays.

These stay the same in both spaces: erasing, which scales color and alpha
together; paint color mixing, which follows the brush's Color mixing choice;
Watercolor's pigment transport; Liquify; and wet or burnt stroke edges.

Healing ([`heal.wgsl`](../../crates/layer-render-wgpu/src/heal.wgsl)) computes
its tone correction in the document's Blending: the membrane matches `B − S` on
encoded values in a Perceptual document and is added to the copy there.
Spot Healing scores its candidates on linear values in both spaces.

## Incremental composition

The *compositor* combines paint and image layers, groups, masks, clipping and
blend modes. Its [scene code](../../crates/layer-render-wgpu/src/scene.rs) tracks
*damage*: regions whose previously rendered pixels are no longer valid.
Stroke-finalization passes, including healing and wet edges, invalidate every
page they revisit. Destination storage, redraw regions and reduced source caches
use the same stroke coverage pages, including pages far from the final dab.

Eligible paint stacks use [region and scale composition](../rendering/display-composition.md).
The scene retains local source levels across camera and placement changes. Reduced
views compose at the requested resolution; native views fill a bounded viewport
window and reuse its overlap while panning. A lone affine source over a constant
backdrop can be sampled directly by the presenter and Navigator. These display
pixels never become document, history or export backing.

A simple paint update rasterizes new dabs into the relevant layer pages and
recomposes affected regions. More complex layer structures need intermediate
results. The renderer retains those results and tracks their dependencies so,
for example, changing a clipping layer does not force unrelated source images
to be rebuilt.

Attached adjustments receive their owner's masked content, followed by the
preceding attached effects in bottom-to-top order. An owner's completed local
result enters ordinary blending or common-base clipping once. An unattached
adjustment receives the lower stack through Pass Through groups up to the nearest
isolated scope. Only an unattached completed spatial result can checkpoint a stack
prefix; a local result belongs to its owner.
Filter-picker previews and their analysis candidates use this same input scope
when replacing an attached adjustment. Owner opacity, blending and outer clipping
apply after that input, just as they do on the canvas.

Cache dependencies use occurrence handles and local chain inputs. Moving an
unrelated row retains valid spatial stages; a downstream parameter edit keeps
valid upstream stages. Source changes retain separate rectangles through placement,
filter support and page invalidation, so distant contacts do not invalidate the
pages between them. Masks branch from the same dependencies at their boundary.
Invalidated branches reuse retired textures with the same region and resolution,
clearing their valid regions before evaluation. Parameter edits replace cached
pixels without allocating another full-size intermediate image.

[Image-stage caching](../../crates/layer-render-wgpu/src/scene_images.rs) handles
operations that need reusable image inputs. A blur needs pixels outside its output
rectangle, so filter definitions describe their sampling footprint. Global effects,
layer reordering and invalidated caches can require much larger updates than a
single brush mark. Visible animated effects also need updates without new pen
input; hiding an attached effect's owner stops those animation requests.

Pointwise filters declaring display-resolution support use effect nodes in the
shared region graph. These nodes reuse the same fused shaders as native tiles;
changing a parameter invalidates the effect result while retaining unchanged
paint inputs. Masks are sampled in the output grid and shader positions remain
document coordinates. Exact queries evaluate document-resolution pixels.
The exact presentation executor remains for other effects and persistent watercolor.
Active paint transforms use the shared region graph. A transaction captures
immutable original tiles and reduces its moving pixels and any unselected
remainder once per input level. Whole-layer transactions can reuse a current
reduced source. The scene's [resampler](../../crates/layer-render-wgpu/src/scene/resample.rs)
places these inputs into requested graph regions, with opacity and a constant
backdrop folded into the same pass. Groups, masks, clipping and other blends
use the ordinary composition rules and branch cache. Mask transactions still
evaluate native coverage before that composition.

Display reconstruction selects prefiltered source detail from the local
transform footprint. Split selections and footprint boundaries use four samples;
partial edge texels retain their actual centers. Mesh triangles draw color
directly into the graph target, with later triangles replacing earlier ones
where the mesh folds. Reconstruction remains approximate while a transform is
active. Native views evaluate native preview pixels;
exact queries materialize the source tiles their dependency window reads and
retire previous query tiles. Apply evaluates the authoritative transform, and
Cancel restores only native pixels that a preview or query changed. There is
no separate placement-drag composite or post-release settling queue.

Move's Leave Copy retains the original under the moved selection. Selected and
unselected captures reconstruct that original without another image allocation.
While Move is active over a selection, `prepare_moving_pixels` names the layer
and selection for the next press. Idle frames prepare those inputs within the
composition budget, and the transaction adopts them only while their source
identity and selection match. Painting, restoring or leaving Move releases
prepared inputs. There is no post-release settling work.

A Warp transform is a [mesh](../../crates/layer-render-wgpu/src/paint_transform/mesh.rs)
of Bézier patches with explicit source breakpoints. Exact splits subdivide whole
rows or columns with de Casteljau; reopening and mode changes never refit controls.
An outer homography follows the mesh. Source lookup uses the same row-major
triangle order as rasterization, with the later triangle winning at folds.
Regional draws use that window's ordered triangles; draws spanning windows use
one copy of each triangle in original order.
Its pages are drawn a window of four by four pages at a
time: the mesh, tessellated within half a pixel and extended by a skirt past
its edges, is first rasterized into a texture of the source position at each
destination pixel, and the transform pass samples the original there,
averaging a pixel's footprint from its neighbors' positions where the mesh
shrinks it. A region job unions the source pages reached by each clipped
triangle independently, including neighboring positions and interpolation
support. Folds retain separate source neighborhoods; pages in the rectangle
between unrelated branches consume no bindings. Traversal stops when the
portable view limit requires another destination split. Large flat triangles
still split into jobs within that limit. Paint, masks and a selection's moved pixels draw this way in the
preview and when applied. A drag rasterizes the mesh at the display level's
texels instead, tessellated within half a texel, and resamples the reduced copy
at those positions. A pixel selection moved by a warp is resampled on the GPU
the same way, a window at a time. What draws meshes compiles in the background
when a warp is first shown, and until then the preview keeps the frame before
it.

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
through the same pass with the exact cap and the persisted interpolation choice.
Layer reads for sampling, Wand, Fill and selection coverage use complete placed
geometry in document coordinates. Raw target bounds retain their local coordinates.
Display composition uses the scene's reduced source levels. Placed sources keep
a level finer than the projected pixel footprint; small changes around unit scale
do not force a second unnecessary level of detail. Mesh footprint planning reuses
the scene's cached tessellation, and source preparation and regional composition
share the resolved footprint. Translated material neighborhoods reuse the owner's
geometry buffers. Admission counts the allocated vertex and index buffers;
source magnification bounds preserve the signed Bézier derivatives.
Affine display sampling counts taps independently along each output axis,
preserving edges under uneven scaling, and accounts for partially covered edge texels.
The direct Navigator subdivides footprints larger than its retained
coarse source can represent with one sample grid.
Placed pigment and scalar pages share that sampler through batched compute
dispatches, including each region's clear and clip. Mapping, watercolor on a
transparent target and reduction share ordered compute batches; scratch pages
can be reused after their reduction. Their pipelines are prepared with the
document. Projective display resampling uses the same mapped sampler in the
existing compute batches. Consecutive mesh regions sharing an output use one
render pass, preserving weighted source positions and triangle order.
Display resampling and composition share ordered batches too. A placed
layer over a constant backdrop resamples directly into its final output.

Material coverage clips mesh triangles to the raw material's source bounds before
mapping them into document space. A small wet area therefore requests only the
regions it reaches, including the interpolation border.

Placed watercolor maps raw pigment and scalar wetness into document coordinates
before evaluating its material appearance. The existing watercolor pass reads
the center and four cardinal neighbors of both planes, so edge widths remain
document-sized under nonuniform scale. Visible-content bounds include that
destination halo. Masks, opacity and blending follow that evaluation. Reduced display levels
reuse cached raw pigment and add the material appearance difference within the wet
region and its halo. One reduction subtracts the already mapped pigment from
the native appearance, converting each first in Perceptual blending, so dry photo
pixels keep their cached values. Complete native blocks in Linear blending use
the existing Float32 sampler to average four texels per tap. Power-of-two texture
dimensions keep these samples exactly centered; partial edges, preview blending,
color conversion and float-document material differences retain per-pixel
reduction. Subtracting before averaging preserves small HDR corrections. Moving sparse
wet paint does not rebuild the full native canvas. An active placement keeps
the same raw source representation when it crosses the identity pose.
Each cached source level records its color and material representation. Returning
to ordinary display can retain a finer raw level within the existing budget;
reopening Transform repairs its changed pages instead of decoding the whole photo.
Reduction and direct sampling reuse only matching representations.
Neighbor mappings share at most 64 scratch pages within a frame. Neighbor color
is mapped only where wetness can contribute, and the material pass borrows that
color only for empty centers. Tiles outside mapped wetness bounds take the
pigment-only path. Display and snapshot admission include
this bounded scratch allowance. On devices without Float32 attachment blending,
a source over a newly cleared transparent target renders directly into that
target; only an existing backdrop needs the portable blend pass.

**Apply Transform to Pixels** uses the same raw-plane sampler and native tile
encoder. Its private snapshot retains Color, Wetness, WatercolorWetness and linked
Mask separately; it never stores evaluated watercolor appearance as pigment.
The shared session publishes one replacement after every output tile succeeds.
Cancellation, renderer replacement and failure discard that private result.
A scalar-only mask bake uses that worker and preserves its owner, default
coverage and inversion. A linked mask under nonlinear paint geometry maps from
the winning owner source position through its independent premap; applying it
bakes the owner and mask together. The capture freezes each input's local extent
before growing the output canvas, so default mask coverage and hidden source
domains do not change during a bake.

## Filters

Filters are effect occurrences in the document. Attached adjustments process one
paint occurrence or isolated group after its content mask, bottom to top, before
owner opacity, blending and outer clipping. The base's locally filtered alpha is
the common clipping shape for every member of its run. A member's local effects
process only that member; effects on a containing isolated group process its
completed inner composition. Hidden owners hide their complete chains, and hiding
an effect bypasses it without changing ownership. Unattached adjustments process
the lower stack and cannot split a clipping run. Generators produce ordinary
content rather than attached processing.

At Normal blending an effect mask and opacity interpolate premultiplied input and
result, including alpha for programs declaring `EffectAlpha::Filter`. Full strength
returns the filtered result once. Spatial effects can expand coverage beyond the
owner's original shape; masks and clipping outside that owner constrain the
completed result at their own boundaries. Declared sampling support governs
capture halos, damage and window budgets along actual dependency paths.

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
results where possible. Signed document regions retain dependencies before zero
and beyond the composition frame. Unsigned texture rectangles describe only
bounded allocations. Region capture walks upstream pass dependencies, expands
the requested region by each pass footprint, and clamps reads at that pass's true
finite support. Directional Gaussian passes retain their mapped axis footprints;
large saved kernels can reuse finite edge tails without allocating empty halos. Native
captures rebase their signed window into local tile coordinates with an exact
64-bit scene offset; the display graph carries signed regions through source
requests, sampling and damage expansion. Retained paint bases remain readable
when a capture's source has no live mutable layer pages. Identity paint sources
clip the signed request to their finite domain before adding the existing mip halo and page
alignment; interpolation padding applies only to transformed sources.

The composition frame, bake destination and temporary image edge do not limit
source reads. Global filters request the finite retained input domain, subject to
the same allocation allowance and GPU dimension checks. Output, current-pass
input and original input carry their own region and texel footprint through the
shared image-grid descriptor. Filter sampling clamps at the declared finite input
support; samples beyond an allocated dependency window are transparent. Reduced inputs use
the centers of their actual covered cells, including partial boundary cells.
Native composition batches up to sixteen source tiles even when a full display
pyramid is not admitted. A bounded horizontal strip replaces the single-tile
working image; each reduction reads its own document-space offset. Admission
includes the strip's one to sixteen MiB, according to document width. Full
pyramids retain their existing destination and batching. Root reductions share
one compute pass per batch, then update the adjacent display level together.
Sparse batches reduce only their covered source rectangles.

Filter-picker previews use the same coordinate contract. Native one-to-one
image reads use fragment positions directly, avoiding a
window-size-dependent interpolation error from reconstructed UV coordinates.

Filter previews scan four source tiles per asynchronous completion, including
the probe's corner-sampling halo. Preview rows then share a source crop expanded
by their required support. A document edit cancels an unfinished scan after its
in-flight completion; no result may combine source revisions. Global samplers
retain their full declared input. Live display composition uses bounded windows
and reduced sources; native-resolution filter dependencies execute through the
same region executor and publish into that display cache. Native filter images
and display pixels share one composition allowance. Oversized global dependencies
are rejected before a frame changes the document. See
[display composition](../rendering/display-composition.md) for admission, moving
previews and exact idle refinement.

Native filter windows admit output sections up to 2048 pixels per side against
the complete halo and image-storage bound, falling back to smaller page-aligned
sections when needed. Each section retires its temporary images after queue
completion. Larger admitted sections reduce overlapping halo work and repeated
source composition without raising the memory allowance.

The native [snapshot renderer](../../crates/layer-render-wgpu/src/snapshot.rs)
prepares document metadata independently of the live display cache. A file or
inspection worker owns an immutable `ArtworkCapture` or scoped `SceneSnapshot`
and a native Float32
renderer. Content-bounds capture rebases the signed scene offset while retaining
the original composition frame and authored effect mapping; its output allocation
may include retained off-frame images and filter support. Bounds remain signed
integers until the legacy `Rect` result rounds outward, so distant finite images
retain conservative nonempty hulls without claiming Float32 pixel precision. Region requests restore
only the translated paint, material and mask pages needed by composition and its halos. Compressed backing remains shared;
restoration uses the same integer decoder as live editing. Initial masks use the
existing GPU crossing/coverage and affine-resampling shaders, with bounded
output rectangles and row slices of immutable packed selection coverage.
Waiting for pending raster roots or selected tiles observes worker cancellation;
retiring a reader leaves the producer's publication available to other readers.

Exact artwork samples use the same snapshot worker and region admission. Their
typed source selects visible artwork, reference composition, placed layer content,
an adjustment's input, or a substituted gesture baseline. Input scope is applied
after region preparation, which resets composition state. Raw layer content skips
its mask, opacity and effect stack. A request retains immutable backing and a
frozen animation time; native snapshot handles also retain effect clocks, while
Web serializes the captured phases to its worker.
Reference composition uses the shared relationship closure, retaining referenced
group contents, clipping bases and attached effect chains. Selection, color
sampling and histogram queries resolve the same reference membership.

The GPU reduces a circular footprint in two passes: covered RGB/alpha maxima and
normalized sums. Only a 32-byte summary reaches the CPU, which unscales it in
f64 and validates the returned straight RGB. Covered transparent pixels affect
mean alpha but carry no color weight. Empty and outside results are distinct.
Calibration uses these linear samples; the paint picker's Oklab average remains
on its existing request path.

Histogram uses GPU reductions on the same immutable queries. Preview samples a
256-by-256 stratified grid in original document coordinates; Exact visits every
pixel. Prepared source windows cover at most 1024-by-1024 pixels and split under
the capture budget. Exact submits and waits between native 256-pixel tiles so
navigation and cancellation can proceed without repeating source preparation.
Four channel histograms and coverage/clipping counts occupy a 4 KiB readback;
no full-resolution image reaches the panel. Positive alpha counts once, including
subnormal coverage. Nonfinite color rejects the result. RGB bin boundaries use
exact ratio comparisons, and ambiguous luminance boundaries use a separate wide
integer reduction so ordinary pixels avoid its register cost.

Waveform optionally extends that same scan with four 256-by-256 count planes.
Columns preserve document x; rows use the histogram's existing RGB and luminance
bins. Eight GPU counter shards add 8 MiB of scratch, and their final fold adds a
1 MiB summary. Histogram-only requests allocate neither. The immutable result
shares its histogram with both panels; only Waveform's changed result, channel,
scale or theme regenerates its small display image. No source pixels are read back.
Web transfers the count planes as one typed buffer from its snapshot worker.
Incremental UI publication retains graph and image objects on status-only changes;
shared Rust prepares channel normalization and straight or premultiplied RGBA for
the native drawing API. A successful browser analysis worker and its same-color
GPU pipelines can serve the next request; at most one remains idle, for five
seconds. Cancellation and failures retire their own worker, and a Wasm heap over
256 MiB is released after its result transfers. Concurrent requests retain
separate cancellation owners.

Auto Levels has a separate two-pass GPU summary: encoded extrema and 4096 bins
per channel over each channel's observed range. Both summaries share bounded
source capture and fold cumulative GPU counters once per pass. Min/max submits
at most four native tiles together; the heavier bin pass yields after each tile.
RGB queries apply channel stages before master; individual queries inspect the
original adjustment input. Shared Rust selects the .1% and 99.9% nearest ranks
from those bins, with value error at
most range/4096, validates the candidate and commits once. Explicit calibration
and Auto preempt live statistics. Hidden panels release statistics and resume
only on demand; old owner, page, source and device results cannot publish.

Snapshot PNG/TIFF output streams sixteen-row strips through the working-color
encoder and profiled row writers using borrowed `WorkingRowsOptions`. Gain-map
JPEG and AVIF share borrowed `GainMapRender` settings and generic row callbacks.
A matching, unmodified source with default
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
tile backing are shared with save snapshots. See the [artwork capture and package contract](../reference/project-format.md).

Live composite color samples and region classification retain their frame while
cold image-object captures prepare on workers. Polling advances prepared GPU
sampling without inline source decoding or GPU waits. Region classification
retains completed batches across deferred captures; cancellation discards the
unfinished request before a replacement can publish.

Layer-thumbnail preparation shares a four-page budget across original
photo tiles, painted overrides, alpha-bounds scans and thumbnail drawing.
Incomplete images stay private. Artwork revisions, selection paint and SDR
rendition changes invalidate prepared work; discarded command buffers invalidate
their cache entries. Each host poll advances one request; up to eight partial
requests can be retained, and layers sharing a photo reuse its integrated
original contributions. Generator thumbnails share the filter-picker
preview execution path, sampling document coordinates on a grid of at most
32 × 32 pixels. They compile asynchronously and retain at most two temporary
images per request. Shared layer revisions include generator parameters, so all
hosts request new pixels after a parameter edit or undo. Content requests use
occurrence IDs; mask requests use encoded coverage source IDs. GTK, Web and
native bridges share `ThumbnailTarget::from_wire_id` to decode both targets.

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
Startup prepares the canvas, then the open document, then the selected brush and
eraser; unused brushes and filters compile on first use
([shader readiness](shared-shader-readiness.md)). Required
dependencies are ready before drawing uses them, so pipeline creation never lands
in a small stroke update. A contact that begins before its brush is ready stays
suppressed until release.
Changing paint color or HDR intensity leaves brush readiness intact when the
tip, texture assets and shader pass requirements stay the same.

Headless capture compiles the dependencies its requested regions execute.
Creating a read-only snapshot does not compile paint-publication kernels.
Interactive brush readiness still prepares those kernels before accepting paint.

On Web, GPU initialization waits for the workspace (at most 1 s), and pipelines
are created through the asynchronous WebGPU APIs, a few at a time: synchronous
creation blocks Chrome's GPU process and display callbacks even when JavaScript
yields between jobs. An edit that needs a pipeline whose asynchronous compile is
still in flight creates it synchronously and drops the asynchronous result
(`node apps/layer-web/test.mjs --headless --pipeline-takeover`).

For brush-specific passes, continue with [Brushes](brushes.md). For performance
work, use the [measurement guide](../development/testing.md#performance).
