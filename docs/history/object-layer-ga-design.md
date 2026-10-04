# Object layers: GA contract and long-term design

[Design history](README.md)

Design date: 2026-10-04. Code inspected at
`69c21ae03` on `origin/main`, including the format fixes through `d20862721`.

This record defines the intended GA state, the long-term direction, and the work
needed to reach GA. It is a design decision, not a claim that these capabilities
are implemented or performance-qualified. The implementation remains described by
the [architecture](../architecture.md), [authored model](../reference/authored-model.md),
[package contract](../reference/capy-package.md), and
[renderer guide](../internals/rendering.md). Update those guides as the work lands.

This decision supersedes earlier proposals to retain general paint-layer
placement, to require a permanent document-resolution bitmap for object layers,
or to make replay of the current painting engine the universal vector format.
Other historical reports remain historical evidence, not additional requirements.

For review, sections 3–8 define the GA behavior, section 9 defines the rendering
boundary, sections 10–11 define the format cleanup, section 12 tests the future
direction, and sections 13–14 define implementation and acceptance.

## 1. Decision and compatibility commitment

Keep two different content models behind one layer framework:

- **Paint layers** own editable, sparse pixel surfaces aligned with the document
  pixel grid. Their only retained positioning is an integer offset.
- **Object layers** own ordered collections of typed objects. Image objects are
  the first implemented type. Each image object retains an immutable image
  reference and its own affine placement. Later paths, strokes and text use the
  same object-layer ownership, ordering, selection and rendering boundary.

Object layers always composite their contents in isolation. Isolation defines
compositing semantics; it does not require a full-size allocation or fix the
resolution of an intermediate image. Object-layer caches are derived renderer
data, never authoritative artwork and never implicit paint targets.

Layer-stack groups and object-layer containers have integer translation only.
They do not retain rotation, scale, perspective, mesh deformation, interpolation,
or a hidden general transform. Moving a container preserves paint-grid alignment.
Commands that transform several descendants edit those descendants atomically;
they do not introduce a retained container transform.

The GA compatibility promise is **new readers continue to interpret GA artwork
with its original meaning**. Future features can add typed records, resource
encodings and explicitly versioned semantics. GA readers preserve unsupported
future artwork instead of discarding it. The promise does not mean that an old
application can edit new vector, animation or text types.

No design can prove compatibility with every unspecified future feature. The
requirement here is narrower and testable: none of the concrete scenarios in
section 12 requires changing the meaning of GA paint surfaces, object-layer
membership, object placement, or stack isolation. Future implementations still
need new evaluators and tests. No unimplemented feature is declared qualified.

## 2. Evidence and what it establishes

Primary product documentation supports the separation of editable objects from
paintable pixels, but does not establish one universal layer model:

| Source | Observed behavior | Decision it informs |
| --- | --- | --- |
| [Krita vector layers](https://docs.krita.org/en/reference_manual/layers_and_masks/vector_layers.html) | A vector layer contains shapes with their own ordering and selection. | Several objects can belong to one layer; one object need not become one global compositor layer. |
| [Affinity Move tool](https://affinity.help/designer2/en-US.lproj/pages/Tools/tools_move.html) | Move selects objects and supports moving, resizing and rotating them. | Object selection can use an existing Move tool; a second tool with duplicate transformation behavior is not inherently necessary. |
| [Procreate Copy/Paste](https://help.procreate.com/procreate/handbook/5.4/interface-gestures/copypaste) and [layer options](https://help.procreate.com/procreate/handbook/5.4/layers/layers-options) | Copied paint or a selection can become a new layer whose pixels can be edited. | Internal pixel paste remains immediately paintable; placement of an external image retains an object. |
| [Photoshop rasterization](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/get-started-layers/rasterize-layers.html) | Vector/Smart Object content is rasterized to document pixels for direct pixel editing. | Make conversion to paint explicit. Preserve source objects until that conversion. |
| [Photoshop mask linkage](https://helpx.adobe.com/photoshop/desktop/create-masks/layer-masks/unlink-layers-and-masks.html) | Linked layer and mask move together; unlinking allows independent movement. | Linkage means following a movement while retaining alignment, not forcing identical origins. |
| [SVG rendering](https://www.w3.org/TR/SVG2/render.html) and [coordinates](https://www.w3.org/TR/SVG2/coords.html) | Geometry, coordinate mappings, stacking and intermediate compositing are distinct concepts. | Keep authored geometry independent of output sampling density and cache size. |
| [Clip Studio SVG export](https://support.clip-studio.com/en-us/faq/articles/20200156) | Selected vector layers can be exported together as SVG. | A document's object layers can be an export scope without first becoming paint. |

These are precedents, not a survey proving that every artist prefers the chosen
interaction. The concrete journeys in section 14 test Capy's implementation.

Current-code findings relevant to this decision:

| Finding | Evidence | Consequence |
| --- | --- | --- |
| Occurrences currently own general placement; paint originals are inline source records. | [Authored records](../../crates/layer-core/src/authored/artwork.rs), [package adapters](../../crates/layer-core/src/package/artwork_records.rs). | Replace these representations before GA. |
| Several raster-operation identity checks apply to the operation's map, not the layer's placement. | [Raster operations](../../crates/layer-core/src/layers.rs). | Do not claim these checks prove that all affine paint placement makes incremental rendering impossible. The decision removes complexity and inconsistent editing paths. |
| Import placement edits paint occurrences and uses retained transform operations. | [Placement interaction](../../crates/layer-ui/src/operation/placement.rs). | Replace that import destination with image objects. |
| Each serialized paint source still repeats its original's tile references; admission now counts tile bindings separately from evaluation edges. | [Admission regression](../../crates/layer-core/src/package/codec/admission.rs), [writer](../../crates/layer-core/src/package/artwork_records.rs), [manifest limits](../../crates/layer-core/src/package/manifest.rs). | Shared images remove repetition and unify ownership; they are no longer needed to fix the reported duplication failure. |
| Composition origin participates in history navigation and renderer rebuilding. | [Engine history](../../crates/layer-engine/src/canvas.rs), [camera following](../../crates/layer-ui/src/canvas_size.rs). | Replace those history responsibilities before removing the portable field. |
| Selection-based mask creation assigns `initial`, and the writer serializes it. | [Mask creation](../../crates/layer-ui/src/selection_pixels.rs), [writer](../../crates/layer-core/src/package/artwork_records.rs). | Materialize authored mask coverage when creating it; this is not deletion of an unused field. |
| A clipping base's effects run before clipped members enter the run. | [Stack evaluator](../../crates/layer-render-wgpu/src/scene/stack.rs). | Preserve the ordering in section 7. |
| Current presentation grids support native resolution and reductions. | [Display plans](../../crates/layer-render-wgpu/src/display_mips.rs). | Magnified object rendering needs renderer work, even though the file model can support it. |
| Move operates on layer/pixel targets; `PickLayer` is a color-picking mode. | [Tools and input](../../crates/layer-ui/src/art_layers.rs). | Add genuine object selection state and picking; existing tools are not sufficient unchanged. |
| Internal pixel clips retain a nonce, full-depth source and copied position; external image paste has a separate entry point. | [Clipboard](../../crates/layer-ui/src/clipboard.rs). | Source-aware paste extends an existing distinction without adding provenance to artwork. |

The following work is already on this baseline. Preserve it while replacing the
model; do not schedule it again as an unfixed defect:

| Completed change | Evidence | Remaining object-layer work |
| --- | --- | --- |
| `272f669fb`: separate tile-binding accounting and capability-limit handling. | `shared_original_references_do_not_consume_evaluation_graph_edges` and boundary cases in [admission.rs](../../crates/layer-core/src/package/codec/admission.rs). | Extend the accounting and regressions to image records, object lists and new placements. |
| `d6b1c9325`: optional representations and extra archive members do not gate editable artwork. | [Optional-content regressions](../../crates/layer-core/src/package/codec/optional.rs). | Keep these passing with the replacement record versions. |
| `cc061dbda`: permanent `application/vnd.capycanvas` identity. | [Archive constant](../../crates/layer-core/src/package/archive.rs) and host registrations. | Retain this identity; no suffix change or old alias is needed. |
| `e66d40f5a`: drift contract and 140 hardware render cases. | [Render contracts](../../crates/layer-render-wgpu/src/package_render_tests.rs) and [baseline index](../../crates/layer-render-wgpu/src/fixtures/authored-renders.tsv). | Replace the eight `placement/*` cases, preserve the other 132, and add the missing object and destructive-transform cases. |
| `d20862721`: fresh bounded save previews. | [Preview capture](../../crates/layer-render-wgpu/src/snapshot/package_preview.rs). | Include object content in the existing capture path without making preview success a save requirement. |

## 3. GA state and deliberately deferred work

| Area | Required for GA | Long-term end state |
| --- | --- | --- |
| Paint | Sparse editable color/material surfaces; integer positioning; direct incremental painting. | Same model and performance boundary. |
| Objects | Multiple affine image objects per object layer, independently selected and ordered; `nearest` and one smooth (`linear`) interpolation contract. | Images, paths, shapes, editable strokes, text and further typed objects in the same layer framework. |
| Source storage | One immutable referenced image record shared by image objects and paint bases. | Additional resource types and explicitly authored sharing features as needed. |
| Rendering | Read-only region/output-mapping interface, implemented for images; cached isolated results. Native-resolution display is an acceptable first implementation. | Visible vectors evaluated at screen density; raster export at requested density; bounded caches at multiple resolutions. |
| Tools | Object selection through Move, existing transform handles adapted to object targets, accessible object list and ordering. | Same whole-object manipulation plus type-specific tools such as node and text editing. |
| Conversion | Convert to Object and Rasterize Layer, explicit and undoable. | Additional type-specific conversions with declared information loss. |
| Persistence | New records, removal of obsolete placement grammar, shared validation, safe unsupported handling. | Additive typed extensions; GA meanings remain readable. |
| Vector interchange | No SVG implementation required. | Geometry-driven SVG export with explicit handling of unsupported appearances. |

Out of scope for GA: path/text/stroke authoring; retained perspective or mesh
image objects; vector masks; per-object masks, effects, blend modes and opacity;
per-object edit locks and object-to-object snapping;
symbols and linked editable instances; nested object groups; SVG import/export;
arbitrary mixed-content group transforms; animation, video, 3D, artboards and a
node editor; object marquee selection, selection cycling and new numeric
placement controls. Do not reserve unused fields, empty operator lists or placeholder
evaluators for these features. Layer opacity and edit lock remain available,
including a separate object layer for an independently faded or protected
reference image. Reuse existing canvas/ruler snapping for Move where applicable;
do not expand GA into a new snapping system. Section 8.2 defines the scope.

Existing paint perspective and mesh operations remain available through temporary
previews and committed pixel edits. Their absence from retained image objects
does not justify removing them from the paint tool implementation.

Group commands in GA move by integer offsets only. Group scale, rotation, flip
and warp are absent; do not implement arbitrary descendant-transform lowering
for them. Canvas-wide Image Size and image orientation remain required and
include mixed content. This is a scope decision, not a claim that artists never
transform groups.

## 4. Coordinates, bounds and numerical representation

### 4.1 Coordinate ownership

The composition owns its frame size, working color/depth, blend space and optional
physical resolution. One composition unit corresponds to one native paint pixel.
Geometry can occupy fractional composition units. There is no requirement to
snap object coordinates to the paint grid.

The frame is a window over retained content. An object layer has a local
coordinate space but no persisted canvas-sized raster domain. Its intrinsic
bounds are derived from its objects. Ordinary crop does not remove off-frame
objects or clamp their placements. Only the requested output region is clipped
for display or export, with additional dependency regions for filters.

Paint and coverage retain explicit finite storage domains with zero local origin
and existing unsigned storage-tile coordinates. Layer/container offsets are
signed exact integers. Use the package's canonical signed-64-bit decimal-string
convention for these new offset fields, with checked arithmetic and separately
bounded editor admission. Do not serialize integer offsets as float matrices.
Large-offset projection must subtract a nearby origin before floating conversion;
accepting an integer on disk does not imply that every renderer can evaluate it.
This is the selected encoding, not an unresolved choice between strings and JSON
integers. `MAX_EXTENT = 32,768` bounds a storage dimension, not the position of an
off-frame object, layer or accumulated ancestor offset. Use strings such as
`["-256","128"]`; reject noncanonical spellings and signed-64 overflow.
An i32 JSON encoding would cover current ordinary canvases, but does not remove
the need for wider checked accumulation, rebasing and device admission. The
existing package already uses exact integer strings; keep that convention.
For small offsets the two quoted components add four bytes per pair relative
to JSON integers, about 16 KiB over 4,096 such pairs before ZIP compression.
This minor representation cost is preferable to making i32 a permanent spatial
constraint. It does not justify admitting unsupported giant coordinates.

No occurrence offset is stored for effects, generators or saved selections.
Their meaning belongs to effect parameters or selection geometry. A containing
group's integer coordinate frame still applies to the descendants it positions;
moving a group is distinct from changing a frame-relative effect parameter.

### 4.2 Image-object affine

An image's source rectangle is `[0,width) × [0,height)`, with sample centres at
`(x + 0.5, y + 0.5)`. Its affine maps source coordinates into object-layer local
coordinates. Use six coefficients `[a,b,c,d,tx,ty]` with:

```text
x_layer = a*x_image + c*y_image + tx
y_layer = b*x_image + d*y_image + ty
```

The composition mapping adds the object layer's and ancestor groups' integer
offsets. Camera mapping is a separate rendering input. Y increases downward.
Mirrors are ordinary invertible affines with negative determinant. Identity is
the omission default. Placement does not depend on the canvas size or a fitted
bounding box. A temporary pivot belongs to the tool, not the file.

Store and edit new object affines in finite binary64 precision, using JSON numbers
that round-trip binary64. Do not parse them through the existing F32 affine
adapter. Compose and invert in sufficient precision before producing camera- or
tile-relative GPU coordinates. Existing pixel samples and paint buffers do not
need to become doubles.
Existing guides, selections and effect parameters keep their present F32
contracts. This change does not imply equivalent high-coordinate precision for
those older types or require widening all artwork numbers.

Non-finite values and mathematically singular image placements are invalid for
this image-object type. An invertible but ill-conditioned or too-large placement
that the implementation cannot safely evaluate is unsupported. UI gestures must
not publish a singular intermediate record when crossing through a flip.

This precision choice has a concrete basis. At coordinate 32,768, adjacent F32
values are 1/256 unit apart: half that spacing becomes 0.5 screen pixel at 256×
zoom. At coordinate 1,048,576 it becomes 16 screen pixels. The corresponding F64
rounding bounds are below `3e-8` screen pixel in both examples. These are
floating-point spacing calculations, not end-to-end renderer error bounds.
Six doubles cost 24 more bytes than six floats; 100,000 placements add about
2.29 MiB of coefficient storage before container overhead. That is a reasonable
cost for preserving authored coordinates. GPU accuracy still needs tests.

### 4.3 Paint base within a growing surface

A paint surface may reference one immutable base image and hold sparse painted
overrides. The binding includes an integer base offset in paint-local space.
For local pixel `p`, the base sample is `image[p - base_offset]` when that sample
exists; otherwise it is transparent. A present override tile replaces the base
over that tile's valid pixels, including explicitly transparent painted pixels.

Require the complete base-image rectangle to fit inside the paint domain. The
domain can be larger. Rendering, merge, export and admission use this same rule.
Reject inconsistent records instead of rendering a clipped base in one consumer
and its full extent in another.

Growing left/up can rebase the paint domain by whole storage tiles: shift tile
keys and the base offset together, compensate the layer offset, and preserve mask
alignment. This preserves immutable backing instead of copying the photograph.
Opening a photo must not retain today's special case that prevents painting in
a newly exposed strip beside its original extent. Explicit deletion of cropped
pixels creates consistent replacement content; it cannot leave a hidden base
outside the declared domain.

### 4.4 Mask placement and coverage

Masks contain scalar coverage, not image luminance or an implicit image alpha.
Their only placement is an integer offset. Let `P` be the parent's accumulated
offset, `L` the owner's local offset (zero for owners without one), and `M` the
stored mask offset:

```text
linked:   mask_document_origin = P + L + M
unlinked: mask_document_origin = P + M
```

Unlinking replaces `M` with `L + M`; relinking replaces it with `M - L`.
Both preserve appearance. An unlinked mask does not follow its owner's movement,
but does follow movement of their containing group. Moving an image object
inside an object layer does not move the layer's mask. This makes Paste Into a
stable frame whose image can be repositioned behind it.

Authored coverage uses sparse tiles plus `default_coverage`. Missing pixels,
including outside the stored domain, evaluate to the default; stored pixels
override it. Inversion is applied afterward, and a disabled mask contributes
coverage one. No result may depend on whether a neighboring pixel happened to
allocate the same 256-pixel page.

Remove authored mask `initial`. Creating a mask from a selection evaluates that
selection into stored coverage through the existing asynchronous GPU path.
Transient operation selections and saved-selection geometry remain supported.
Scaling/rotating a painted mask is a pixel operation, never retained placement.

## 5. Minimal GA records and ownership

Retain the current restricted ZIP/ZIP64 envelope, typed record table, typed binary
resources, stable random 128-bit IDs and discoverable `{"ref":"…"}` references.
The envelope's `objects` table contains all record kinds; it is not synonymous
with the drawable objects inside an object layer.

Introduce three record roles, with the following registry names for the new
types. Replace implementation adapters without retaining a legacy interpreter,
but give changed wire contracts distinct versions:

| Changed record | GA version | Reason |
| --- | --- | --- |
| Occurrence | `capy.occurrence/3` | Object content alternative, integer offsets, owner-relative linked mask offsets, and removal of retained placement/inapplicable flags. |
| Composition | `capy.composition/2` | Remove portable origin. |
| Paint source | `capy.paint-source/2` | Referenced base image, base offset and paint-use color policy replace inline originals. |
| Coverage source | `capy.coverage-source/2` | Materialized coverage replaces authored `initial`. |
| Output | `capy.output/2` | Remove unevaluated frame and scale. |
| Built-in effect application | `capy.effect/2` | Inline the built-in descriptor and remove unused input/binding reservations and portable definition indirection. |

Keep the ZIP envelope version and unchanged record/resource versions. Do not
reuse an old record ID for changed field meanings, even before GA. Remove the
superseded types from the known registry: a structurally valid old package then
uses the generic unsupported/Copy Original path, without a migration reader.
Test this for each removed type, including old linked masks and fractional
translations, so neither is silently reinterpreted or classified as damaged.
Likewise test new record versions through an older-type registry. Shape checking,
reference traversal and support classification must agree with the decoder.

| Record | Required authored data | Optional authored data / frozen default |
| --- | --- | --- |
| `capy.image/1` | Extent; tile references; channels, depth and profile interpretation. | `profile_assumed:false`; physical resolution unspecified. |
| `capy.object-layer/1` | Generic `children` list of ordered drawable-object references, front to back. | Empty list is permitted. No transform, raster extent, cache or independent color space. |
| `capy.image-object/1` | Image reference. | Name empty; visible true; affine identity; interpolation `linear`. ID is in the ordinary record header. |

Add one object-content alternative to occurrences referencing the object-layer
record. The occurrence continues to own the layer name, visibility, opacity,
blend, edit lock, reference designation, attachment, integer offset and optional
mask. The object-layer record does not duplicate those properties.

Each drawable object has one owning collection in the GA editable subset, and
each object-layer record has one layer occurrence. Duplicating objects allocates
new object IDs and shares immutable image IDs. Duplicating a paint layer creates
a new editable paint source with shared immutable backing. Neither operation
creates linked editing. Referenced immutable image records may have many users.

The same drawable ID cannot occupy two collection slots: that violates identity.
Future linked instances use a separate instance object referencing a definition,
not repeated insertion of one editable object ID. Ownership, appearance order,
resource sharing and future transform inheritance are distinct relationships.

An image record stores retained decoded samples and their interpretation; it does
not promise retention of the original JPEG/HEIF/other compressed file. Storage
repacking or deduplication does not change image identity. Editing an immutable
image publishes a replacement image record and updates only the intended uses.
Import/decode stays off input and UI threads.

Reuse the currently supported source channel/depth/profile combinations and
complete source-tile indexing rules. Extents are positive integer sample counts;
every source tile in the declared rectangle is accounted for, including valid
edge padding. Integer source samples use their declared profile encoding; floating
RGB source samples use linear values with explicit primaries. Source alpha, when
present, is straight; premultiplication is an evaluation step. Do not introduce
an arbitrary channel schema or silently reinterpret tile descriptors. Additional
sample layouts belong to future typed image/resource contracts.

### 5.1 Paint originals use the same image record

Replace inline paint originals with a base-image reference, base offset, and the
paint-use color policy needed by current behavior:

- **Source-profile base:** preserve the image's original interpretation through
  document color changes, as current imported originals do.
- **Working-pixel base:** treat the base as part of the editable paint surface;
  its interpretation follows explicit working-space/depth changes with the
  overrides, as current rasterized bases do.

This policy replaces `SourceKind` on the shared image; it is not a second image
representation. The source-profile policy is the default. Working-pixel bases
must match the composition's declared working color/depth. A color change
replaces affected image references without mutating an image still used by an
object. Objects always retain the referenced image's own interpretation.

The distinction is necessary, not disposable provenance:
[current color edits](../../crates/layer-core/src/color_edit.rs) preserve imported
originals but update rasterized bases. Test Assign Profile, Convert Profile and
depth changes before replacing this behavior.

### 5.2 Fields that remain out of the file

Do not add a common object wrapper, universal property bag, dormant transform
stack, cache resource reference, rasterization scale, renderer generation,
tessellation, GPU layout, hit-test proxy, spatial index, active object, selection
marquee, tool pivot, drag preview or undo history to these records. A layer's
thumbnail is an optional derived representation, not a second source of truth.

Raster overrides/material state remain authored where they affect current
appearance or later painting. Wetness is not a cache merely because the GPU
stores it. Profiles, physical resolution, named selections, rulers and meaningful
locks/reference flags also remain. Trimming removes redundant or undefined data,
not artists' work or documented color behavior.

### 5.3 Portable built-in effects

In portable `capy.effect/2`, store `builtin`, parameter-data `version`, and keyed
`values` directly in the application record. Keep the application ID: occurrence
references and evaluation phases address that identity. Preserve all authored
values, including defaults, and every existing built-in ID/data-version meaning.
Do not confuse the record version `/2` with a particular built-in's data version.

Remove `capy.effect-definition/1` from the portable known registry, and do not
write a separate portable record/ID solely to hold `{builtin, version}`. Built-in
programs resolve through the catalog; shared runtime program objects need no
authored definition identity. The current
[definition adapter](../../crates/layer-core/src/package/effect_records.rs)
contains no editable built-in definition beyond that descriptor. Private custom
filter definitions/programs remain in private session/worker contracts as needed;
they do not justify the portable indirection. Keep those private workflows tested
and remove obsolete portable last-use bookkeeping without breaking private uses.

Remove empty `inputs`/`bindings` reservations from the portable effect contract.
They are not implemented graph editing; current readers already reject nonempty
maps. Future object-local filters, reusable editable styles and explicit effect
graphs get implemented typed records with declared coordinate/evaluation edges.
Keep the reference-envelope and endpoint mechanisms that actual composition and
output records use. Do not reserve graph fields on every built-in application.

## 6. Image evaluation and sampling

GA image objects evaluate bottom-to-top with source-over in the composition's
selected linear or perceptual blend domain, initially against transparency.
They use their image alpha and visibility. The layer's mask and attached effects
apply after its internal composition, according to section 7. The collection
orders typed drawing operations; it does not require every future object type
to use the image object's default appearance. Future typed appearance may add
internal blend/backdrop dependencies while the layer's outer isolation stays
unchanged. This is not an unused appearance field on GA images.

Images retain their own channels, bit depth, profile and assumed-profile flag.
Use the existing explicit source-to-working color conversion policy. Do not
reinterpret imported image samples as document-profile bytes. Interpolation uses
premultiplied working-linear color and alpha; scalar coverage remains linear.
Conversion into the selected blend domain belongs at the compositing boundary.
Sampling beyond the image rectangle is transparent, not edge clamping.

Keep authored interpolation on image objects, with exactly two GA choices:
`nearest` and `linear`, with `linear` the frozen default and smooth mode.
Newly placed images also start with `linear`; copying an object preserves its
authored choice. Do not switch insertion to bicubic/Lanczos merely to address
minification: all current smooth modes use bilinear taps when integrating a
larger footprint. Kernel choice and antialiasing of that footprint are distinct.
Nearest preserves the deliberate pixel-art choice. Bilinear sampling already has
[independent sampling oracles](../../crates/layer-render-wgpu/src/transform_oracle_tests.rs).
Move the applicable definition into the image-object contract rather than
retaining a general occurrence-placement grammar.
Do not expose bicubic or Lanczos on GA image objects; the GA reader treats these
unimplemented values as Unsupported. They can become additional interpolation values
after their complete contracts are implemented. Existing paint-transform and
export choices may continue using those kernels: those operations commit pixels
or produce an output and do not retain a kernel name in portable paint artwork.

Before freezing those names, specify and test the complete sampling behavior:
sample centres, transform direction, transparent edges, premultiplication,
overshoot handling, minification and rounding. Current smooth minification uses
a grid of bilinear samples; current exact and moving passes have different tap
budgets. Those implementation choices must not accidentally become synonymous
with a kernel name or with authored cache quality. Retain the existing exact
reference as the baseline while recording its minification behavior explicitly;
intentional corrections happen before the fixture freeze. Preview approximations
may use lower quality, but canonical document-grid operations and exports must
match the frozen reference within their declared numerical tolerances.

Test severe and anisotropic reductions as well as magnification: checkerboards,
thin lines, transparent edges and the existing zone-plate reference. A 4096-wide
photo reduced to 512 spans eight source pixels per output pixel in each axis;
one bilinear sample cannot represent that 8×8 footprint. Increasing a
reconstruction kernel's name alone supplies no such guarantee. Keep the existing
preview-versus-commit distinction explicit and qualify the actual footprint
filter, including reductions beyond its current tap cap, before GA. Do not
declare every reduction antialiased from the one 8× case.

This is a release gate, not a claim that current sampling already meets an
unmeasured quality target. New algorithms after GA must preserve the frozen
meaning or introduce a new authored interpolation value/version. Runtime cache
resolution and optimization limits are not serialized per object.
Complete that reference and quality gate in milestone 2, before the new object
authoring UI ships. The plan does not bless a capped undersampling algorithm as
the permanent smooth contract. Footprint filtering and bounded mip/anisotropic
implementations are distinct concerns, as described in the primary
[PBRT texture-filtering reference](https://www.pbr-book.org/4ed/Textures_and_Materials/Image_Texture);
adopting one requires both reference-image and tier-performance evidence.

## 7. Layer-stack, clipping, masks and effects

Keep the existing shared stack evaluator. An object layer is eligible wherever
paint or an isolated group can own a mask, serve as a clipping base, or own an
attached adjustment chain. It cannot be pass-through. A group may keep the
existing explicit pass-through mode; that mode's restrictions remain.

Resolve relationships bottom-to-top from sibling order. An ordinary unclipped
content entry sets the potential clipping base to itself if eligible, otherwise
clears it. Do not search past an intervening ineligible content entry. Attached
adjustments belong to their contiguous eligible owner. Unattached adjustments
operate on the lower scoped composite and cannot split a clipping run. Saved
selection rows do not become clipping bases; the existing editor normalization
keeps them outside owner/effect chains.

The evaluation order is:

1. Evaluate an owner's content, then its enabled mask.
2. Evaluate that owner's attached effects in stack order.
3. Assemble the clipping run using the base result and each clipped member's
   independently evaluated content/mask/effect result.
4. Apply the base's opacity and blend to the completed run against its backdrop.

The base's attached effects do **not** process the assembled clipping run.
For example, a blur attached to the base does not blur images clipped above it.
Object-layer opacity is applied once to the isolated result, never separately
to every image. This distinction changes overlap appearance.

Existing attached filters remain composition-frame operations after the owner's
mask. Pixel lengths refer to composition units, not image-source pixels or
physical screen pixels. Frame-relative centres continue to re-anchor when crop
changes the frame. A photograph moved behind a frame-relative effect does not
carry that effect's centre along as an object-local filter would.

Implicit distances and periods in native-grid built-ins are also composition
units. In [filter_library.wgsl](../../assets/filters/filter_library.wgsl), Denoise's
radius 2, VHS's seven-unit row bands and CRT's three-unit stripe period are not
physical output pixels. At 2× output density their document-space appearance
must stay anchored; merely running the same constants in output texels changes
the effect. Test 1×/2× evaluations and crop/frame anchoring.

The 768-edge [local tone guide](../../crates/layer-core/src/color/hdr/local.rs)
is different: it caps the analysis grid's sample count, not an effect radius of
768 composition units. Its grid and mapping derive from the document frame,
independently of zoom/export dimensions. Keep that existing approximation and
its conformance tests; do not relabel every shader constant as a distance or
rebuild the guide against the magnified viewport. The same distinction applies
to the separately defined Dehaze guide. Preserve their documented semantics in
the [filter reference](../reference/runtime-filters.md).

Correct misleading `source_pixels` declarations to the domain actually used.
Do not freeze an undefined `normalized` length. Built-in parameter meanings come
from the catalog's explicit semantic definitions; private custom-filter grammar
belongs in private-format documentation. Future object-local effects or
scale-independent effects receive their own declared evaluation semantics,
without changing the meaning of GA attached raster filters.

Well-formed relationships outside the supported subset are `Unsupported`.
Malformed references, duplicate ownership identities and prohibited cycles are
`Invalid`. A future relaxation of an editor restriction must not make a GA reader
misdiagnose a well-formed future document as corrupt.

## 8. GA user journeys and tool decision

### 8.1 Creation and conversion

| Action | GA behavior |
| --- | --- |
| Open Image | Create a paint layer at identity with a referenced imported base. It is immediately paintable. |
| Place Image, image file drop, external clipboard image | Insert image objects into the active editable object layer; otherwise create an object layer immediately above the selected layer in its parent stack. Preserve placement preview and cancellation for Place and ordinary external Paste. |
| Paste a second external image | The first paste leaves its object layer active, so the second normally enters the same object layer. Both remain independently selectable and ordered. No one-image-per-layer rule. |
| Paste / Paste in Place from Capy's pixel clipboard | Create a new paint layer above the selected layer in its parent stack, regardless of the active content kind. Preserve the copied position for Paste in Place; ordinary Paste keeps the existing visible-position/otherwise-view-centre rule. Align placement to the document pixel grid. The result can be painted or erased immediately. |
| Paste copied Capy objects | Preserve their object structure and share/remap immutable resources correctly. Allocate new object identities. |
| Paste Into a pixel selection | For internal or external pixel images, create a new object layer with a stored coverage mask made from that selection. Put the pasted image inside it. Moving the image changes its position behind the fixed layer mask. Structured object paste into a selection preserves those objects inside the new masked layer. |
| Paint or erase while object content is active | Refuse the stroke without changing artwork. Offer direct actions to Add/Edit Mask, New Paint Layer, and Rasterize Layer; name the selected object layer as the affected scope. Never modify a cache or silently rasterize. |
| Convert to Object | Replace a paint layer's content with an object layer containing an image of its current raw paint appearance. Preserve the occurrence and its presentation properties. |
| Rasterize Layer | Evaluate the object's internal content at the document pixel grid into a paint surface. Preserve the occurrence's mask, attached effects, opacity, blend and attachments so they apply exactly once. |

Copying a layer remains a layer operation, distinct from copying selected objects
or selected pixels. Command targets must be explicit in shared state.
Ordinary Paste does not implicitly turn an existing pixel selection into a mask;
Paste Into is the explicit masked operation. A locked or inadmissible insertion
parent causes a refusal before mutation, not insertion into an unrelated group.

This distinction follows the existing `PixelClip` versus external-source routes
in [clipboard.rs](../../crates/layer-ui/src/clipboard.rs). Copy, Cut and Copy
Merged that explicitly capture pixels produce the internal pixel flavor, even
when Copy Merged contains rendered objects. A selected-object Copy produces a
structured object flavor with an image fallback for other applications. Prefer
the richest valid Capy flavor when available; an expired/mismatched nonce or an
external clipboard offering only image bytes follows the external-image rule.
Never guess provenance from pixel equality, file name or a stale clipboard.
Preserve full-depth/color interpretation and the existing cross-document color
policy. Clipboard provenance is transient and adds no field to `.capy` artwork.

Internal pixel paste does not require a resample simply to insert it. Reuse a
compatible immutable image as the paint base and choose an integer offset;
view-centred placement rounds to the nearest document pixel, with half values
away from zero. Subsequent fractional/rotated/scaled paint moves use section
8.3's single-commit operation. External Paste in Place has no copied document
position, so retain the current view-centred full-size placement without an
interactive preview. Object-to-object paste maps through document coordinates
into the destination layer's local frame; do not copy a local matrix unchanged
between translated parents. Cancel releases unpublished records and creates no
undo entry; Cut removes content only after successful clipboard publication.

Add/Edit Mask selects the layer's coverage target (creating reveal-all coverage
if absent); it hides parts of the whole object layer, not just one selected
image. New Paint Layer creates and selects paint above the object layer.
Rasterize Layer is the explicit destructive conversion of the whole object
layer. Do not replay the refused stroke automatically after any action. Keep
these choices in the existing shared refusal/action UI rather than persistent
instructional text in the canvas.

Convert to Object captures current paint overrides and resolved material
appearance, not just the imported base. It does not turn paint into editable
vector strokes or preserve future wet-paint behavior in the resulting image.
Undo restores the original paint content and material state. Rasterizing an
object layer removes its object editability from the resulting layer; undo
restores the complete records and references.
Keep Convert to Object in GA. Open Image and internal pixel paste deliberately
produce paint; conversion is their explicit entry to retained lossless placement
without exporting/reimporting or recovering an obsolete original. It shares the
raw-content capture and bounded publication work needed by clipboard, merges
and Rasterize Layer. Removing it would leave that chosen workflow incomplete.

Conversion bypasses the occurrence's visibility, opacity, mask and external
effect chain while evaluating its raw content, then preserves those occurrence
properties on the replacement. Otherwise rasterizing a hidden layer would
incorrectly produce empty paint. Internal object visibility still contributes to
the layer's current content; restoring hidden objects after rasterization requires
undo, just as restoring their other editability does.

Neither conversion is generally a reference swap. Identity/compatible-content
cases may reuse existing image data. Multiple objects, transformed sampling,
color conversion and paint overrides can require GPU rendering and publication.
Conversion retains off-frame content within admission limits; it must not
silently discard it by capturing only the viewport. If the complete required
extent cannot be admitted, refuse before committing.
If conversion rebases the occurrence to a different integer origin, compensate
linked mask offsets to preserve their document position. A change in storage
origin must not move the mask, guides or frame-relative attached effects.

### 8.2 Existing tools need object behavior, not a duplicate toolbar tool

**GA requires object selection and manipulation. Implement it through Move and
the existing transform controls; do not add a separate Object toolbar tool with
the same behavior.** Current Move is insufficient unchanged: it starts
layer/pixel transactions and has no independent object-selection model.

This choice follows the [Affinity Move-tool precedent](https://affinity.help/designer2/en-US.lproj/pages/Tools/tools_move.html)
and reuses Capy's existing Move/Scale–Rotate interaction vocabulary. It is a
product decision, not a requirement of the file format. Later node editing is a
distinct tool because editing a path's points is a different operation.

Required shared UI behavior:

- With object targeting active, an ordinary Move click searches visible,
  editable object layers in front-to-back stack order, including nested groups.
  Clicking an image in another layer activates that layer and replaces the old
  object selection. Hidden or locked ancestors exclude their objects. Empty
  space clears object selection without changing the active layer. Show
  the selected objects' bounds and reuse existing affine handles for resize,
  rotation and flips. Preserve a clear explicit layer/mask target for the
  existing Move Layer/Mask commands; canvas object picking must not silently
  turn those commands into a different edit.
- Keep object selection, layer-row selection and pixel coverage selection as
  separate typed state. Object selection never becomes a raster selection.
  A stale pixel selection must not unexpectedly cut an object during a move.
- Support click and additive selection, Select All within the object
  layer, deselection, delete, duplicate, copy/cut/paste and
  object ordering. Operations on several selected images commit as one edit.
  Shift-click adds/removes an object on desktop; the object list provides a
  touch-accessible additive selection action. Additive selection stays in the
  active object layer for GA; cross-layer multi-object transforms are deferred.
  Existing explicit paint/layer/mask Move
  targets keep their behavior and do not auto-switch into object picking.
- Include compact object child rows within an expandable object-layer entry,
  with names, selection and visibility. These are object rows, not new layer
  occurrences or separate global compositor entries. They provide access to
  obscured, hidden and fully transparent images. Ordering commands and touch
  access must not depend exclusively on desktop modifier keys.
- GA canvas picking uses the inverse affine and the image rectangle, frontmost
  first. It does not synchronously decode or read back alpha. Bounds-based
  picking is deterministic; the object list handles
  overlap. Future type-specific hit testing can improve picking without
  changing artwork. Hidden objects are selected through the list, not canvas.
  This is bounds picking, not exact visible-pixel picking through masks, effects
  or overlying paint. The selection outline and list identify the chosen image;
  overlapping bounds are resolved explicitly through accessible object rows.
- Entering placement after Paste/Place selects the inserted object and its
  transform controls. Brush and selection tools keep their established pixel
  behavior on paint layers. Integer layer movement and fractional object
  movement must be visibly distinguished by the selected target.
- Perspective/mesh controls remain available for paint pixel operations and
  unavailable for GA image objects. Never silently rasterize an image to make
  an unsupported retained transform appear to work.

Object selection, focus, temporary handles and hit-test structures live in
working/session state. UI controls and native transport do not add `.capy`
fields. Rules and text remain in shared Rust; hosts forward input and present
the same views. Follow the [drag convention](../ui/drag-and-reorder.md) and
[localization rules](../ui/localization.md).
Defer marquee selection, canvas selection cycling and new numeric-placement
controls. Rows plus click/additive selection and transform handles complete the
GA image journeys without those features. Existing paint controls remain;
none of these deferred UI features needs a new portable record or field.

Object child rows show object properties; the layer row shows layer opacity,
blend, mask and lock. Do not make an object opacity control edit its container.
Creating an empty object layer, cutting selected objects and pasting them into
another object layer provides an explicit way to separate reference images for
independent layer opacity/locking. Moving objects between layers can change
appearance because isolation and layer properties change; it is not promised
as a lossless conversion of the previous composite.

Reuse Move's [snapping implementation](../../crates/layer-ui/src/operation/snapping.rs)
for canvas/ruler candidates and the current modifier/guide behavior. Feed it the
selected objects' transformed bounds without adding per-object snap fields.
Individual object locks, opacity, and snapping against other objects remain
deferred; later lock/opacity defaults are false/one and do not alter GA objects.
Their usefulness is supported by products such as Affinity, but implementation
cost is not established by that precedent. Affinity's own
[snapping guide](https://affinity.help/designer2/en-US.lproj/pages/DesignAids/snapping.html)
warns about all-layer candidate costs. Reusing rulers does not supply bounded
object discovery, exclusions, coordinate precision or gesture tests for free.

### 8.3 Transform and canvas operations

Paint transform previews resample the current committed content captured at
transaction start. All pointer updates use that same snapshot; commit is one
pixel operation. Repeated committed transforms can lose sampling detail. An
imported original is a valid shortcut only when it is equivalent to the current
content; using it after painting would discard edits.

Whole-pixel translation changes the layer offset. Quarter turns and flips use
exact pixel permutations when aligned to the grid. Other paint rotation, scale,
straighten, perspective and mesh operations resample once at commit. Preserve
the relevant mask/material behavior and one-step undo.

Whole-layer exact turns/flips of a paint base must permute retained samples in
their own interpretation, along with paint overrides and material planes; do not replace
this with a working-color render that loses original depth/profile values.
An immutable base whose sample axes change needs a new image record (or proven
identical existing data), leaving other image users unchanged. Preserve the base
binding and rebase its offset/domain consistently. Quarter-turns swap physical
resolution axes where applicable. Build replacement image tiles in bounded
windows on workers, with no whole-image allocation or UI-thread pixel loop.
Rotating only the base would discard/misalign painted edits. Test the largest
admitted photo, an edited photo and a shared image still used by an object;
larger-than-admitted requests must refuse atomically.

Destructive transforms also resample scalar watercolor wetness. The current
[membership threshold](../../crates/layer-render-wgpu/src/watercolor_floor.wgsl)
is `2/255`, so values around it can change membership after resampling. Retain
the scalar-plane contract, quantization and resolved appearance in tests; do
not claim material-edge invariance for arbitrary rotations. Exact grid turns
must preserve the scalar samples exactly. Include subsequent wet painting and
undo, not just the immediate color screenshot.

Canvas Size and crop change the frame/window and necessary integer origins;
they do not resample paint. Image Size scales paint pixels and masks and edits
object affines. Physical-resolution metadata changes alone do not change pixels.
Canvas-wide rotation/flip and other already supported image operations need
atomic mixed-content handling as part of GA, not silent omission of object
layers. An unsupported descendant or exceeded limit refuses the whole operation.

Group Move changes integer offsets only. Remove group scale/rotate/flip/warp
commands from GA availability rather than maintaining descendant-rewrite code
for arbitrary group gestures. Canvas-wide operations still update all relevant
paint, objects, masks, selections and guides atomically. Object multi-selection
transforms edit the selected affines together without giving the object-layer
container a matrix.

### 8.4 Commands that consume or inspect layers

Add object content to the shared semantic enums and make content-kind dispatch
exhaustive. In matches deciding rendering, command availability, mutation,
serialization or properties, enumerate every known variant; no wildcard may
silently treat a new kind as paint, empty, or generally editable. This is not a
ban on unknown-wire handling or unrelated catch-all branches. Also audit
`matches!`, `if let`, `LayerKind` comparisons, `paint_source`, `source_target`,
`SceneScope::Raw` and early-return guards: compiler exhaustiveness does not find
these omissions. Search shared consumers and native view projections, not just
the enum definition. The review's match count is a useful scope warning, not a
complete semantic inventory or an acceptance metric.

| Command / consumer | Object-layer policy for GA |
| --- | --- |
| Merge Down / clipping-stack merge | Accept wherever existing blend, attachment, visibility and lock rules permit. Evaluate the complete affected run, including object content, directly into one paint result at the document grid. Do not rasterize each layer as separate undo steps. |
| Merge Visible | Include visible object content in the paint result. Consume the participating root subtrees at the existing merge anchor; retain other roots and release affected clipping attachments under the current rules. Preserve off-frame visible content. |
| Stamp Visible | Create a new paint result from visible content, including off-frame content, while retaining source layers. This differs from Merge Visible's replacement. |
| Flatten | Include visible object content in one document-grid paint result. Preserve the existing explicit discard handling for hidden artwork and restrictions on saved selections; do not silently bake selection geometry. Undo restores consumed records. Flatten is not an implicit crop. |
| Rasterize Group / bake a subtree | Include object descendants. Bake exactly the declared subtree scope; preserve outer presentation properties once. Retain existing pass-through/backdrop and lock restrictions. |
| Apply/bake an attached effect | Accept when the complete required owner/clip scope can be baked with the existing ordering and backdrop semantics. Result is paint; consume the effects actually baked, preserve unbaked relationships, and apply remaining properties once. Refuse unsupported scopes before mutation. |
| Rasterize Layer | Accept; bake raw internal object content as section 8.1 specifies, preserving the outer mask/effect chain. Existing paint-original rasterization keeps its separate source-to-working purpose. |
| Apply Mask | On objects, expose the explicit action as Rasterize and Apply Mask. Bake raw content and its enabled layer mask into paint in one undo step, remove that mask, preserve outer effects/opacity/blend once. A disabled or absent mask keeps its existing refusal. Editing a mask alone never rasterizes objects. |
| Select Layer Alpha | Accept through read-only raw content evaluation at the document grid. Use the internal composite alpha, including object visibility, before layer visibility/opacity/mask/effects/clipping. Produce a pixel selection without converting the layer. Load Mask continues to read coverage. |
| Copy pixels / Copy Merged / color and region sampling | Accept through the appropriate read-only raw/composite scope, independent of viewport resolution. Explicit pixel Copy is distinct from selected-object Copy. Cut by pixel coverage is a paint operation; object Cut removes whole selected objects. |
| Brush, eraser, smudge, fill, pixel clear/cut, retouch and pixel adjustments | Accept a paint or coverage target where the command supports it; refuse an object target with the actions in section 8.1. Do not invent a paint target or silently modify immutable image pixels. |
| Add, edit, copy, paste, invert, link, disable or delete a layer mask | Accept for object layers under the same ownership rules as isolated groups. Materialize coverage and use integer geometry. Per-object masks are deferred. |
| Attach effect / clipping / layer blend | Accept object layers as eligible isolated content. Offer supported layer blends; never pass-through. Preserve section 7's ordering and relationship checks. |
| Layer properties, menus and thumbnails | Show object-layer name, visibility, opacity, blend, reference flag, lock and mask. Hide paint alpha lock and source-only controls that do not apply. Show image interpretation and affine/interpolation for the selected object in the appropriate object view; do not expose container placement. Generate thumbnails from read-only content. |
| Duplicate, delete, reorder, group and ungroup layer rows | Preserve object-layer membership, object IDs and source sharing according to section 5. Duplicate allocates new editable identities. Removing layers removes their owned content and retains only required image dependencies. Layer grouping does not flatten the internal object list. |
| Crop / Canvas Size / Image Size / image orientation | Include object layers under section 8.3. Any unsupported descendant, lock restriction or admission failure aborts the complete operation. |
| Save, recovery, tab parking, transfer, clipboard, raster export and previews | Enumerate object/image dependencies from immutable captures; do not rely on paint targets to discover all content. Use the existing job/checkpoint and publication mechanism. |

Bounds are a shared read-only content query, not an inference from paint targets.
For every destructive bake/merge involving objects, take the complete union of
the finite contributing paint and transformed object bounds, plus required effect
output support, and round outward to the document grid. Masks may reduce
appearance but do not justify clipping surviving content to the current frame.
Store the result with an integer offset and admitted finite paint domain. Hidden
objects are not made visible by merging; their editability returns through undo.

The current [merge implementation](../../crates/layer-core/src/merge.rs) is not
uniformly frame-clipped: `bake_extent` can include off-frame paint, but
`merge_plan` selects frame-only extents for canvas/effect cases, and `bake_bounds`
intersects its computed bounds with the supplied bake extent. Update all those
paths and the renderer's region/scope evaluation together. Merely teaching
`bake_extent` about objects is insufficient.

Frame-defined generators contribute their existing frame-domain output; they
must not force the rest of a finite content union back into that frame. Preserve
existing frame-relative filter coordinates rather than treating an expanded
bake rectangle as a new composition frame. Where output support is unbounded or
cannot be conservatively represented within admission, refuse the bake with a
specific reason; do not silently clamp it. Dependency input halos may extend
beyond output bounds and must be fetched without becoming an accidental crop.

Merging must preserve the current composite on the affected support. Include
the required masks, attached effects and clipping members, and retain current
refusals for blends/backdrops that cannot be represented by the proposed scope.
Do not remove safety conditions just because object content can render. Check
admission before starting a bounded worker job, validate its checkpoint before
publication, and commit removals/replacements atomically. Failure or cancellation
leaves the original records, working targets and undo history intact. A new paint
result selects its paint target; an undo cannot leave dangling object selections.

## 9. Rendering boundary and latency

### 9.1 Authored source versus evaluated result

The read-only content interface receives a region in document coordinates, an
explicit document-to-output mapping and requested quality/evaluation context.
It returns a premultiplied result with its actual region and pixel mapping, or
scheduled work to produce it. This describes a contract, not a prescribed Rust
trait or asynchronous runtime. Reuse the existing frame/job scheduler.

Writable targets remain paint surfaces and scalar coverage, including selection
mask editing. Object-layer render results have no paint-target identity.
The compositor can read both source kinds
without converting objects into editable raster sources or requiring a CPU
pixel copy.

The interface must express scales above and below one from GA. Image rendering
and tests exercise those mappings even if GA viewport policy initially requests
native resolution. Returning one permanently native-resolution texture with a
scale argument that is ignored is not the required boundary.

The same authored objects serve different consumers:

| Consumer | Sampling request / source |
| --- | --- |
| Viewport | Visible region at requested physical screen density, accounting for zoom and display density. |
| Pixel tools sampling visible artwork | Canonical document-grid evaluation; results do not depend on zoom, viewport cache age or preview quality. |
| Rasterize / paint conversion | Explicit document-grid output and complete required bounds. |
| Raster export | Explicit output dimensions and mapping, never a capture of the display cache. |
| SVG export | Authored objects and their geometry/appearance semantics, bypassing raster caches. |

High-resolution object results must remain at the requested evaluation density
through the relevant compositing chain. Downsampling them into a native pixel
composite before enlarging the screen loses the benefit. A final vector overlay
also fails when paint, masks or blending belong above the vectors. Evolve the
existing shared stack traversal to compose the appropriate region at output
density; keep pixel-grid effects as explicit evaluation boundaries.

### 9.2 Incremental painting and invalidation

With a fixed view and unchanged object content, brush samples update paint tiles
and dependent composition regions. They do not replay every object. Retain cached
isolated object results or composition checkpoints at the useful scale, subject
to memory admission. Painting above and below objects uses the same ordering.

Cache identity includes content/resource revision, region, output mapping or
scale level, color/evaluation context, and relevant dependencies. A view change
requests a different derived result; it is not an authored edit or an undo step.
Stable document-anchored tiles allow panning to reuse existing regions.

Object movement damages the old and new bounds, expanded for sampling and
effects. Resolve intersecting objects through a derived spatial index as object
counts grow. Do not iterate the entire document object list for every paint dab.
An edited vector's live feedback must update the appropriate layer region, not
wait for camera settling or draw above unrelated layers.

Sampling tools such as merged-source cloning remain tied to a consistent
document-grid evaluation. A better viewport image must not change what a brush
picks up. This separation is also necessary for deterministic exports.

### 9.3 Pan, zoom, rotation and refinement

1. Present the best available cache immediately, reprojected to the new view.
2. During a pan at unchanged density, reuse covered tiles and schedule newly
   exposed regions. Do not wait for settling to fill missing areas.
3. During zoom/rotation, refine visible regions as the motion budget allows.
   A simple scene may remain sharp throughout; complex scenes may temporarily
   show reprojected pixels.
4. After settling, complete the requested density from authored sources.
5. New input takes priority. Bound GPU submissions as well as CPU tasks; a large
   background submission can block drawing even if submitted by another thread.
   Cancel/coalesce obsolete work and never publish stale results over newer edits.

Use selected resolution levels and visible tiles with filter halos. Do not keep
a full-document bitmap for every layer at every zoom. Pin only the working set
needed for the admitted interaction; evict and regenerate derived data. The
performance boundary does not promise unlimited layers or unlimited pinned caches.

### 9.4 Size calculations, not performance measurements

At the renderer's current RGBA32Float intermediate precision:

| Allocation | Pixel bytes alone |
| --- | --- |
| One 4096 × 4096 surface | 256 MiB |
| 200 such layer surfaces | 50 GiB |
| One 4096 × 4096 document rasterized completely at 8× zoom | 16 GiB |
| One 2560 × 1600 visible surface | 62.5 MiB |
| One 3840 × 2160 visible surface | 126.5625 MiB |

The calculation is `width × height × 16 bytes`. Masks, halos, alignment,
intermediates, source residency and simultaneous revisions add to it. Even
viewport-sized caches cannot be retained independently for every layer without
a budget. Tiles, scoped evaluation and reuse are requirements, not optional
optimizations justified only by large documents.

These calculations do not establish frame time or choose a universal cache
budget. Use the actual [tier targets](../PERFORMANCE_TARGETS.md),
[measurement rules](../performance/measuring.md) and reference devices. Keep
results in the tier tables, not this design record.

## 10. Portable-format cleanup before GA

| Existing or proposed baggage | GA disposition | Replacement / reason |
| --- | --- | --- |
| Occurrence projective placement, cubic mesh, interpolation | Remove from portable artwork and runtime layer/container state. | Integer offsets; image-object affine and interpolation. |
| Separate placement translation plus matrix translation | Remove the redundant layer representation. | One integer layer offset; one complete affine per image object. |
| Mask projective placement and linked pre-map | Remove. | Integer linkage rule in section 4.4. |
| Inline original image per paint source | Remove. | Shared immutable image record and paint-base binding. |
| Original/Rasterized role on image samples | Move the required behavior to paint-base use; do not duplicate it. | Explicit source-profile versus working-pixel policy. |
| Output frame and scale | Remove currently unevaluated fields. | Export requests own delivery size; composition owns its frame. Future artboard/output features get implemented schemas. |
| Composition origin | Remove from portable artwork after replacing runtime/history uses. | Root offsets carry artwork positioning; explicit private history/view information preserves crop navigation and rebuilding. |
| Alpha lock on nonpaint occurrences | Remove applicability; do not silently accept a meaningful true value. | Alpha lock remains an authored paint-layer behavior. |
| Authored coverage `initial` | Remove. | Materialized coverage tiles and per-pixel default. |
| Saved-selection occurrence presentation flags with fixed values | Omit and validate centrally. | Selection geometry and meaningful name/edit lock remain; overlay presentation stays private. |
| Crop-written offsets on Paper/effects/selections | Remove occurrence writes. | Update frame-dependent evaluation or actual selection geometry, and independently preserve mask positioning. |
| `source_pixels` for frame-evaluated lengths; undefined normalized units | Correct actual semantic domains; remove undefined portable vocabulary. | Explicit catalog dimensions and per-parameter meanings. |
| Built-in WGSL, shader ABI, GPU layouts, labels and control layout | Keep outside portable artwork; trim private grammar out of the GA package specification. | Stable built-in IDs/data versions and all authored parameter values, including defaults. |
| Portable built-in definition wrapper and unused effect inputs/bindings | Remove. | Inline the descriptor in `capy.effect/2`; resolve programs from the catalog. Future implemented graph/definition types remain possible. |
| Four retained image interpolation names | Narrow to `nearest` and `linear`. | Freeze one smooth contract; extra paint/export kernels need not become permanent object fields. |
| Object cache pixels, target resolution and renderer generations | Do not add. | Derived renderer state and optional bounded previews. |
| Universal object property bags, empty future feature blocks, capability inventories | Do not add. | Typed records and support determined from actual content. |
| Old pre-release transform readers/migrations | Do not add. | Replace fixtures before release. |

Do not remove composition ownership, source/occurrence identity, typed resources,
discoverable references, meaningful output color/proof/context state, authored
material state or retained off-frame content to make the schema look smaller.
Those boundaries already solve concrete problems and prevent future rewrites.
Likewise, preserve known unplaced artwork records by table membership; reaching
an output is not the only reason artwork is retained. Immutable image records
are dependencies, not independent retention roots. Their lifetime is defined
below; presence in the image table alone does not justify saving them.

Delete obsolete retained-layer editing functions and renderer caches only after
their replacement consumers exist. Keep reusable affine, perspective and mesh
sampling required by temporary previews and pixel commits. Remove obsolete
"apply transform first" refusals once their underlying states no longer exist.

### 10.1 Image dependency lifetime

A portable save starts from all retained artwork records, including known
unplaced artwork and retained copy-safe ancillary records, then follows their
discoverable references. Write an image record and its tile/profile dependencies
only if this closure references it. Follow references from every retained
record, not just visible objects, the active composition or placed paint sources.
An unused image in the image table is omitted. Writers compact unreferenced tile
payloads under the existing package resource rules.

Deleting the last object must remove its owned object record too; otherwise the
orphan record would still retain the image. Deleting a layer likewise removes its
owned object collection and objects. Conversion replaces/removes the consumed
records. Do not use an orphaning bug as an implicit asset library. A deliberate
unplaced artwork record remains a real root and can still reference an image.
Future asset libraries need explicit authored ownership, not table membership
for every decoded image ever imported.

Private undo/redo, parked tabs and in-flight immutable captures retain the image
versions they need through their own roots. Portable-save pruning does not
mutate that history or invalidate an accepted job. Reclaim private backing only
when neither current artwork, history nor a live capture references it. Undoing
deletion restores the same image/object identities. Test last-use deletion,
remaining shared uses, hidden objects, unplaced artwork, copy-safe references,
conversion, cancellation, portable save/reopen and private recovery separately.

## 11. Reader, writer and compatibility rules

### 11.1 Bounded and consistent admission

Use shared definitions and accounting for record counts, references, image tiles,
image records, per-layer object counts, metadata bytes and expansion/dependency
work. Editor mutation, capture/writer and reader use them consistently. Structural
parsing limits and device memory/performance admission are different layers of
checking; one monolithic validator is not required.

For settled artwork accepted under a given supported schema and limits, saving
and reopening under those same limits must succeed. Test this at boundaries.
Saving need not imply that a weaker device can edit the document. Never mark a
save successful before complete publication, or publish partial conversion data.

The reported inline-original duplication failure is fixed by `272f669fb`.
Its regression admits and round-trips 1,025 paint uses of a shared 4096² image.
The remaining reason for referenced images is one ownership/interpretation
model and a smaller manifest: `(4096/256)^2 = 256` source tiles repeated across
1,025 uses require 262,400 tile bindings. One image record needs 256 tile
bindings plus 1,025 image-use references, 1,281 bindings for this part of the
serialized graph. These are now different accounting categories, not a renewed
claim that current graph admission fails. Count per-object/list metadata and
evaluation expansion separately even when image storage is shared.

### 11.2 Unsupported versus invalid

Unknown required record types, fields, semantic variants and resource encodings
preserve the whole artwork as unsupported. Unknown vector objects do not vanish
from an otherwise editable layer. Copy Original preserves the original bytes;
an available independently validated preview can be shown, but accurate partial
rendering of unknown content is not promised.

Malformed syntax, duplicate IDs/keys, dangling references, resource ranges outside
the archive, format-defined integer overflow, integrity failures and forbidden
ownership/evaluation cycles are invalid.
Mathematically well-formed counts, ranges, combinations or placements beyond the
implementation's capabilities are unsupported. Capacity errors must identify a
limit; they do not necessarily mean a newer file version. Check limits before
expensive traversal, allocation or decompression.
An implementation's inability to accumulate or project otherwise valid large
coordinates is a capability limit, not proof that their authored values are
mathematically invalid.

Unknown additions to understood required records are not ignored. A future field
with a frozen absence default can preserve old meanings, while an older reader
still refuses to edit a file that actually uses that unknown field. New enum
variants need the same behavior. Test unknown-child types through the existing
object-layer record, not only a wholly unknown root.

### 11.3 Optional previews and archive extensions

A missing, oversized, undecodable or unsupported preview/representation is treated
as no preview. It must not make valid artwork fail to open. This relaxation does
not excuse malformed required manifest structure or ambiguous archive bounds.
Do not require rendering a preview before saving complete authored source.
This behavior and the extension-member behavior below are already implemented
by `d6b1c9325`; preserve their regressions as record adapters change. Fresh preview
capture from `d20862721` must also include objects. A failed new preview must not
leave an old preview described as current.

Ignore bounded, unreferenced extension members outside reserved `data/` and core
members. Do not extract or interpret them. Drop unknown unreferenced members on
an edited save; retain the original package for Copy Original. Keep existing
ancillary/copy-safe reference rules distinct from arbitrary ZIP-member retention.
Required resources, duplicate names, overlap, unsafe paths and archive structure
continue to receive integrity checks. Audit archive admission as well as the
manifest namespace check so optional members do not fail earlier inconsistently.

### 11.4 Rendering semantics and MIME identity

Built-in IDs, parameter keys, choice IDs, units, accepted ranges and all saved
values retain their meaning. The blanket permission for shader drift was already
removed by `e66d40f5a`. Preserve that rule: permit optimizations and numerical
differences only within declared conformance tolerances; semantic changes need a new supported
data version. Do not archive shader generations or build generic migration
machinery. A golden image is evidence, not permission to bless any new output by
regenerating the expected image.

The final package identity is **`application/vnd.capycanvas`**, already adopted
by `cc061dbda` in the package marker, host associations and fixtures. Keep it.
Do not add `+zip`, restore `application/x-capy-canvas`, or introduce a compatibility
alias as part of object layers. A ZIP suffix is not required to implement this
container. This records the repository's MIME choice, not a claim about external
registry registration. Normal publication checks still verify all hosts use
the same identity.

## 12. Long-term end state and extension stress tests

The end state is one composition framework that combines paint and scalable
objects. A file may contain only object layers. At high zoom, vector geometry
is evaluated for the visible region at screen density. At raster export, it is
evaluated at the requested output resolution. SVG export visits the objects
directly. Paint retains its native samples in every case.

Integer layer-container positioning remains sufficient: fractional movement,
rotation and scale of a selected set can update its objects' affines in one undo
step. Future reusable symbols or composite drawable objects may have their own
instance placement and internal structure as typed object content. They are not
arbitrary transforms on paint-layer containers. The GA flat object list does not
prohibit a future object type from owning a hierarchy.

| Future requirement / adversarial case | Extension route | What remains unchanged / limitation |
| --- | --- | --- |
| Thousands of editable vector ink strokes in one layer | Typed stroke records with authoritative geometry, width/style data and type-specific editing. Derived spatial index and local damage. | Object-layer ordering and cached compositing boundary. A stroke is not a global layer. |
| Variable-width strokes and node editing | Preserve editable centreline/width intent in the stroke type; compute filled outlines for compatible export where necessary. | No GA stroke replay schema or brush-engine generation is imposed. |
| Text, fonts, shaping and fallback | Typed text objects and referenced font/shaping resources with explicit fallback/portability rules when implemented. | Generic resource transport and object selection. Text fidelity is not established by an affine alone. |
| Pure-vector SVG | Export groups, paths, fills/strokes and transforms from authored objects; use a viewBox and explicit physical-size policy when specified. | No viewport cache dependency. SVG remains interchange, not the universal native object representation. |
| Parametric shapes, path booleans, fills, stroke dashes, markers and variable width | Typed geometry/appearance and marker definitions; bulk control points in typed binary resources. | Path commands or mesh vertices do not become layer occurrences. Destructive booleans and retained live booleans have different authored contracts. |
| Gradients, gradient meshes, patterns, swatches and graphic styles | Shared editable definition records referenced by objects; edits invalidate their users. | Sharing is explicit and does not turn all duplicated images or paint surfaces into linked editing. A gradient mesh is not the discarded occurrence warp grammar. |
| Live path effects, offsets and construction operators | Versioned geometry operators with keyed parameters and typed dependencies. | Reuse catalog/version/conformance discipline, without pretending geometry operators are GA raster filters. |
| Images mixed with vectors in one object layer | Both appear in the existing ordered object list. | No paint surface is interleaved secretly between objects. Embedded images remain raster in SVG. |
| Pixel masks or Capy effects on vector content | Evaluate at an explicitly defined raster boundary or map an equivalent SVG operation. | Pure geometry does not guarantee an all-vector result after raster-only effects. Export reports unsupported appearance or uses a declared raster fallback. |
| Perspective/mesh-warped image objects | A new typed appearance/placement contract or record version, with its own bounds, hit-testing and export behavior. | No return of layer/container mesh placement. GA affine objects retain their meaning. |
| Per-object effects, opacity, blend or masks | Typed additions with declared evaluation stage, bounds and isolated/backdrop dependencies. | Defaults keep old objects unchanged; GA readers preserve unsupported additions. No assumption that every object can be cached independently of neighbors. |
| Symbols and linked editable instances | Separate definition identity and instance objects, with instance paths for targeted overrides when needed. | Immutable-image sharing is not retroactively changed into linked editing. |
| Nested groups and smart embedded artwork | Composite object types own their internal coordinate/evaluation rules and expose a 2D result to an object layer. | Existing layer-stack groups retain integer positioning. Nested scene semantics require feature-specific work. |
| Cross-layer alpha/luma mattes | Explicit typed input bindings with declared coordinate space and evaluation stage. | Sibling clipping remains its existing relation; ownership and transform inheritance are not overloaded. |
| Object clip paths, masks and SVG filter graphs | Typed object appearance references, object-local mappings and explicit evaluation edges/ports. | Implement these when needed; reserved empty maps in GA built-in effects are unnecessary. |
| Live fills, construction geometry and hidden boundaries | Future objects retain nonpainted geometry, hints and fill assignments. | Visibility does not determine whether an authored record is retained. |
| Artboards, pages, print sizes and multiple outputs | Additional composition/output types and contexts, reusing object/resource references. | A composition owns units/color/frame; an object layer does not freeze canvas-sized pixels. |
| CMYK, spot inks and overprint | Typed color/image/output contracts plus suitable proof, separation and compositing evaluators. | RGB viewport pixels cannot preserve ink separations alone. This is more work than adding a color enum, but does not change GA RGB or object ownership meanings. |
| Grids, guides and snapping | Retain current rulers; add authored grid types and runtime candidate indexes when implemented. | A snap setting does not belong on every GA image object. |
| Animation, held cels, cameras and audio | Typed tracks/contexts/instances/resources, preserving exact identities and time semantics when introduced. | No GA timestamp on every image or mandatory event replay. Retained unplaced records are not deleted as unreachable output data. |
| 3D, video, deep images, texture sets or extra channels | New typed scenes/resources/evaluators; projection into a 2D result where appropriate. | The 2D object-layer endpoint is not claimed to preserve depth interactions. These features can require different composition types without rewriting GA 2D types. |
| Collaboration, paste and independently duplicated content | Stable IDs, discoverable references and explicit record ownership; later operation/conflict semantics. | UI indices and resource equality never substitute for authored identity. |
| Extremely large geometry / zoom | F64 object geometry, exact integer origins, camera-relative GPU work and bounded requests. | Device/precision admission remains necessary; finite precision is not an infinite-canvas guarantee. |

Raster-only paint mixing, smudge, watercolor and similar effects should not be
promised as lossless editable vector strokes. A future procedural-stroke type
must define its own durable brush/style data and appearance contract. Neither a
saved GPU mesh nor replay against whatever brush engine ships later is a stable
substitute for that contract.

These extension routes are architectural inferences, not proof of compatibility
with every Inkscape behavior. The [Inkscape SVG overview](https://inkscape.org/en/develop/about-svg/)
and [live path effects documentation](https://gitlab.com/inkscape/inkscape-docs/manuals/blob/master/Inkscape-Beginners-Guide/source/live-path-effects.rst)
illustrate the distinction between interchange appearance and retained native
editing. Typography, booleans, print separation and complex effect graphs still
need their own evaluators, drift contracts and acceptance work.

### 12.1 Rules that keep extensions within one model

**Reuse appearance meanings.** Future object groups and other composite objects
reuse the existing blend names/formulas, source-over ordering, coverage/alpha
concepts and stable effect catalog where the semantics match. Declare evaluation
space, stage, bounds and backdrop dependencies explicitly. Do not reinterpret
composition-frame raster effects as object-local effects or build a second
incompatible blend vocabulary. Reuse the generic `children` ordering convention
for future object groups; their permitted child types and ownership still belong
to their own schema. Semantic reuse does not require an unused common appearance
wrapper or universal object superclass in GA.

**Distinguish ownership from sharing.** Single-owner editable content, many-use
immutable images, and future many-use editable definitions have different edit
and invalidation rules. Enforce those rules by record type, not a universal
"every referenced record is single-use unless it is an image" rule. Current
`Shape::Definition` already allows multiple references; it does not supply all
future editable-swatches/symbols behavior. New definition types add explicit
update propagation, expansion/evaluation edges and cycle checks. Instances need
identity paths for per-instance overrides or phases. No dormant definition store
or generalized graph editor is needed for GA image objects.

**Keep bulk geometry outside JSON.** The current 64 MiB metadata limit, 65,536
record limit and 262,144 evaluation-edge limit are reader/editor limits, not
artwork meaning. Binary resource descriptors and discoverable references allow
future path/stroke geometry chunks without embedding millions of numbers in the
manifest. Larger admitted record counts remain Unsupported on older readers.
They still require efficient indexes, incremental edits, bounded decoding,
history and renderer work. This plan does not qualify 100,000-object performance
by declaring a new type or increasing a limit.

**Preserve text meaning deliberately.** Point text, flowed text, text on paths,
rich spans, variable fonts and shaping need font/resource identity, layout
inputs, geometry dependencies and an appearance-drift policy. Font substitution
must be explicit; SVG export may need outlines when editable text cannot retain
appearance. This is analogous to the effect drift requirement, not a promise
that any later shaping engine can reproduce the same glyphs automatically.

**Treat SVG color and units as export semantics.** An sRGB composition using
perceptual blending is a useful default for ordinary SVG-like vector work;
"vector-first implies perceptual everywhere" is too strong. SVG defaults
[ordinary interpolation/compositing to sRGB](https://www.w3.org/TR/SVG2/painting.html#ColorInterpolation)
but [filter primitives to linearRGB](https://www.w3.org/TR/filter-effects-1/#propdef-color-interpolation-filters),
and permits explicit overrides. Capy's float documents currently require linear
blending. An exporter must preserve the authored spaces, geometry and effect
meanings, with an explicit fallback when the target cannot represent them;
changing the composition setting alone does not establish fidelity. New
object-local color policies may need typed appearance additions.

Use composition resolution for physical size when supplied; source-image
resolution must not rescale object coordinates implicitly. Without an authored
physical size, a future SVG exporter uses the standard
[96 CSS pixels per inch](https://www.w3.org/TR/css-values-4/#absolute-lengths)
for its default size and an explicit `viewBox`. Screen zoom or display DPI never
changes the saved document units. This export convention does not add a GA field.
Export must distinguish a pure-vector delivery from an explicit raster fallback.
Do not silently strip unsupported appearance to obtain an `.svg` file; lossless
reimport of native Capy editing through arbitrary SVG editors is not promised.

**Keep editable paint on its integer grid.** Future transformed vector-object
groups can contain image objects, including images made from paint. They cannot
contain the existing directly editable paint surface under an arbitrary affine.
That is a deliberate product/performance boundary. Supporting locally paintable
raster content inside transformed vector groups would require a separate future
content/interaction contract and qualification; it must not silently restore
general placement to GA paint layers. This plan does not promise that workflow.

### 12.2 Complexity retained deliberately

| Existing concept | GA decision and reason |
| --- | --- |
| Stack-derived clipping, attached effects and adjustments | Keep their implemented painter workflows and exact order. Future object-local relationships use explicit typed references; they need not overload sibling inference. Future code may change representation through an explicit conversion, but GA files always retain the frozen stack meaning. |
| Saved selections as occurrences | Keep in GA. They already have separate selection records, but occurrences supply names, edit locks, ordering, group ownership, group-relative geometry, duplication and deletion. [Selection editing](../../crates/layer-core/src/selection.rs) and [shared selection UI](../../crates/layer-ui/src/selection_masks.rs) depend on this. Moving them into a collection is not merely removing flags; it must replace those relationships and host journeys. Strip meaningless presentation fields now; a separate future panel can project the existing records without changing the file. |
| Built-in definition-only records | Remove from portable artwork under section 5.3. They carry catalog identity without user-editable definition semantics. Private custom programs and future shared editable styles are separate requirements. |
| Pass-through groups, reference sampling closure, per-application phases and frame-relative generators | Keep: they control existing authored appearance or tool behavior. Qualify their interaction with object content. A record or field is not baggage solely because it requires code. |

## 13. Changes required to reach GA

These are complete implementation milestones, not instructions to commit
non-building intermediate states. Keep production changes and current guides
together. Do not implement deferred vector features to complete this plan.
This order is intentional: the renderer and layer consumers precede user-facing
object creation, and object creation precedes deleting retained paint placement.
Each milestone lands on shared main with existing journeys usable. Update
affected fixtures as soon as a schema changes; milestone 7 is final qualification,
not permission to leave earlier commits with broken fixtures.

| Milestone | Required changes | Exit condition |
| --- | --- | --- |
| 1. Shared model and package records | Add immutable image records, paint-base references/policy/offset, typed object-layer/image-object records, object IDs and F64 affine adapters. Extend topology, resource inventory, admission, immutable capture, edits/undo, session and transfer encoding. Replace inline-image ownership everywhere. Add object dependency lifetime rules. | Two independently editable objects and two paint uses can share one image through model save/open/copy/undo, with color/extent and retention semantics checked. Existing paint/import paths still work. New object authoring is not exposed before its consumers exist. |
| 2. Image rendering and early performance gate | Implement read-only region/output-mapping evaluation and bounds for images; connect isolated results, clipping, effects, masks, scoped snapshots and damage tracking. Reuse sampling helpers; exercise scales above/below one. Keep the still-used old paint-placement path until milestone 5. | Reference renders and raw/composite sampling agree. Object results are never paint targets. On the lowest-tier reference tablet, measure painting above/below a multi-image layer and cache pressure before proceeding to UI integration. Resolve failures of the caching assumption here. |
| 3. Layer consumers and conversions | Complete section 8.4's command dispatch, masks, merge/bake bounds, alpha selection, source properties and read-only sampling. Implement Convert to Object/Rasterize Layer and explicit apply-mask conversion; adapt canvas-wide image size/orientation to mixed content. Prepare shared object selection/undo state needed by these operations. | No supported consumer omits objects or invents a paint target. Off-frame content, current edited appearance, color/material semantics and outer properties survive the specified conversions. Jobs are bounded, atomic and cancellable. Existing unsupported scopes have specific refusals. |
| 4. Tools and creation | Implement section 8's source-aware clipboard, object insertion, Move picking/handles/list/order and actionable painting refusals. First review the GTK interactions, then complete all host projections. Replace external Place/Paste destinations with objects; preserve internal pixel paste as paint. | Real multi-image, internal copy/paste/paint, cross-layer picking, overlap, mask, touch, cancel and undo journeys work on every affected host. All replacement consumers are available before retained paint placement is removed. |
| 5. Integer-only paint/container/mask cutover | Replace retained occurrence/mask placement and its callers with integer offsets and pixel-preview/commit transactions. Replace crop navigation's portable origin with private history/view data. Complete exact base-image permutations and mixed image operations. Remove old placement fields/adapters/caches/refusals in this same milestone. | Open, Place, every paste flavor, paint transforms, crop/grow, image resize/orientation, conversions and undo all remain usable. Paint/mask grids stay aligned and no general retained layer/container placement remains. |
| 6. Remaining format cleanup and reader audit | Complete section 10's removals and version table, including inline built-in descriptors and removal of portable definition/input/binding baggage. Centralize saved-selection/property applicability checks; correct relationship classification and unit definitions. Audit all new admission and dependency paths. Preserve already-fixed preview/extension-member/limit behavior and permanent MIME identity. | Same-limit admission/save/reopen and malformed-versus-unsupported fixtures pass, including each removed record version. No discarded image data survives portable save solely through image-table membership. Private custom filters still work. Current package/model documentation specifies the implemented wire fields and defaults. |
| 7. Semantic freeze and full qualification | Finish object and destructive-transform fixtures, replace obsolete placement baselines, retain unaffected render contracts, settle sampling conformance, complete all host journeys and tier performance rows. Update current guides. | Every section 14 gate has implementation evidence. Unmet or unmeasured gates block GA; passing the design review only permits starting implementation. |

Version each wire contract when its meaning changes, using section 5's final
registry names. Do not reuse a final version for two incompatible intermediate
meanings. If a dependency requires changes to land together, combine complete
milestones or stage internal helpers until the atomic cutover; do not publish a
temporary portable schema or add legacy readers. Keeping an old path until its
last caller switches is temporary sequencing, not part of the GA contract.
In particular, adding object stores does not license an interim
`capy.occurrence/3` that still has float offsets or parent-relative linked masks.
Stage runtime helpers before that record's complete wire cutover, or combine the
dependent tools and integer-contract milestones into one usable landing. The
same rule applies to the coverage/composition/output replacements. An unchanged
pre-release adapter can remain only until its replacement and callers switch;
it is removed before the GA fixture freeze.

The early hardware gate uses the [low-tier reference](../performance/low-tier.md),
currently the TCL TAB 11 Gen 2 (9465X), with the tier's 4248×2832 canvas and 60 fps
target. Reserve it through the [device workflow](../development/devices.md).
Compare matched paint-only controls with multiple images sharing and not sharing
sources, warm/cold object results, painting above/below, repeated eviction, and
the first stroke after view changes. Record frame times, latency and peak/cache
residency under the [measurement rules](../performance/measuring.md). A desktop
measurement or the allocation arithmetic in section 9.4 cannot pass this gate.
An unavailable reference tablet means the gate remains unmeasured, not passed.

Primary implementation locations:

| Area | Current code to replace or extend |
| --- | --- |
| Authored stores and topology | [artwork.rs](../../crates/layer-core/src/authored/artwork.rs), [topology.rs](../../crates/layer-core/src/authored/topology.rs), [scene.rs](../../crates/layer-core/src/authored/scene.rs), [occurrence edits](../../crates/layer-core/src/authored/occurrence_edits.rs). |
| Sources, color and admission | [image samples](../../crates/layer-core/src/color/source.rs), [color edits](../../crates/layer-core/src/color_edit.rs), [project.rs](../../crates/layer-core/src/project.rs). |
| Layer transforms and canvas geometry | [layers.rs](../../crates/layer-core/src/layers.rs), [transform_pixels.rs](../../crates/layer-core/src/transform_pixels.rs), [canvas_geometry.rs](../../crates/layer-core/src/canvas_geometry.rs). |
| Layer consumers and bounds | [merge.rs](../../crates/layer-core/src/merge.rs), [layer actions/properties](../../crates/layer-ui/src/art_layers.rs), [alpha/coverage selection](../../crates/layer-ui/src/selection_masks.rs), [scoped scene capture](../../crates/layer-core/src/authored/scene.rs). |
| Writer/reader/private persistence | [package directory](../../crates/layer-core/src/package), including artwork records, manifest, values, codec, transfer, session and session transfer. |
| Renderer | [scene placement](../../crates/layer-render-wgpu/src/scene/placement.rs), [stack](../../crates/layer-render-wgpu/src/scene/stack.rs), [scale evaluator](../../crates/layer-render-wgpu/src/scene/scale.rs), [mask pages](../../crates/layer-render-wgpu/src/layer_masks.rs), [pixel transforms](../../crates/layer-render-wgpu/src/pixel_transform.rs). |
| Tools and native views | [art_layers.rs](../../crates/layer-ui/src/art_layers.rs), [operation.rs](../../crates/layer-ui/src/operation.rs), [placement](../../crates/layer-ui/src/operation/placement.rs), [clipboard](../../crates/layer-ui/src/clipboard.rs), [selection masks](../../crates/layer-ui/src/selection_pixels.rs), [shared host](../../crates/layer-host/src). |
| Crop history/view behavior | [engine canvas](../../crates/layer-engine/src/canvas.rs), [UI canvas size](../../crates/layer-ui/src/canvas_size.rs), [UI session](../../crates/layer-ui/src/session.rs). |

Remove `retained_transform_targets`, `retained_transform_edit`, mask pre-map math,
paint-specific imported-original placement exceptions and obsolete placement
sampling caches as their callers are replaced. Do not preserve a hidden adapter
that reconstructs old transformed paint layers from image objects. Reuse existing
job publication, undo, resampling and test harnesses.

### 13.1 Starting implementation in a fresh session

Read this record in full, then the current `AGENTS.md`, architecture, authored
model, package, testing and performance guides linked here. Fetch current main
and check which milestones have actually landed; history text is not evidence
of implementation. Start at the first incomplete milestone. Sections 3–8 settle
the GA product decisions, section 5 settles changed record versions, and sections
9–11 constrain every implementation. Section 12 is extension guidance, not a
request to implement its deferred features.

Before changing the model, trace a source through `Artwork`, record edits,
`RootInventory`, scene capture, admission, writer/reader, session/transfer,
renderer and shared UI. New record handles must survive every boundary. Include
read-only render targets in scopes/consumer APIs without extending `SourceTarget`
to writable objects. Update all content-kind projections; an exhaustive core
enum alone does not update host menus or pixel-only predicates.

Use the [testing matrix](../development/testing.md) for each changed crate and
host, including package codec/session/effect records and immutable publication
paths. Run the existing hardware `saved_artwork_render_contracts` suite when
rendering semantics change. Add focused regressions for the new contracts in
section 14, not a second parallel harness. GTK-first review is a UI sequencing
step; no shared model or policy belongs in GTK. Do not call a milestone complete
with the other affected hosts left broken.

Complete milestone commits, fetch/rebase before each authorized push, and update
the current owning guides in the same change. Keep measurements in the tier
tables and detailed logs in `artifacts/`. Do not change this design's future
promises merely to make a failing implementation test pass. If code reveals a
real contradiction, resolve and document the affected contract before freezing
it; there is no unspecified owner decision blocking milestone 1.

## 14. Acceptance and release gates

### 14.1 Format and semantic fixtures

Save, reopen, edit and undo fixtures cover:

- Two image objects sharing one image, and image objects sharing an image with
  independent paint bases. Duplicate editing and color changes do not affect
  the other users accidentally.
- Rotated, mirrored, fractional, minified and off-frame images; explicit
  interpolation choices; identity and exact quarter-turn cases; high-coordinate
  F64 placement round trips and renderer-relative coordinate checks.
- Paint domains larger than imported bases, growth left/up, override tiles with
  transparent pixels, and identical render/merge/export coverage.
- Crop and grow preserving objects and hidden paint, including undo/redo camera
  behavior after private-session recovery.
- Linked and unlinked masks, relinking without movement, default coverage across
  page boundaries, inversion and selection-created masks.
- Convert to Object and Rasterize Layer with edits, material appearance, masks,
  effects, clipping, opacity and undo; off-frame content retained or atomic
  refusal when its full conversion cannot be admitted; converting a hidden layer
  and showing it afterward must retain its content.
- Paste Into creating a new masked object layer; repositioning its image leaves
  the mask frame fixed.
- A clipping run based on an object layer, including effects on the base and
  clipped members; the ordering in section 7 is tested explicitly.
- Paint → edit imported photograph → transform: committed edits survive.
  A long preview gesture matches one transform from its initial current pixels.
- An object-only document with no dummy paint source; canonical evaluation
  independent of camera scale; object-layer rendering requested at several
  densities with consistent document-space placement.
- Same-limit admission/save/reopen at object, image, reference, tile and metadata
  boundaries, including the shared-original duplication case.
- Last-use deletion omits the unused image and payloads on portable save while
  undo/private recovery retains them; shared, hidden, unplaced and copy-safe
  referenced content remains. Cancelled jobs release unpublished image records.
- Internal pixel Copy/Cut/Copy Merged → Paste → move → paint/erase, compared with
  external bitmap/file placement and structured object copy. Test all paste
  modes, nonce mismatch, cross-document color and translated destination parents.
- Merge Down, Merge Visible, Stamp Visible, Flatten, group/effect bake and
  Rasterize and Apply Mask with objects, off-frame content, hidden descendants,
  attached effects and clipping. Check outer properties exactly once and
  oversized/unsupported scopes refusing without partial changes.
- Select Layer Alpha from hidden/masked/translucent object layers, using raw
  internal alpha independently of camera and occurrence properties; subsequent
  painting through the resulting pixel selection remains a paint operation.
- Exact turns/flips of large imported bases, shared by an object and an edited
  paint layer, with source interpretation, resolution axes and overrides intact.
  Include the largest admitted photo and refusal beyond admission, bounded
  worker publication, cancellation, save/reopen and undo.
- Destructive watercolor rotation/minification around wetness values 1, 2 and 3
  out of 255, exact turns, subsequent wet painting and undo. Check material bytes
  and the appearance they generate, not only a frozen color image.
- Each removed record version opens as Unsupported through generic preservation,
  including old fractional translations and linked-mask offsets. New built-in
  application records preserve IDs, parameter versions/values and phase bindings;
  private custom definitions still round-trip without entering portable artwork.
- Two-kernel image-object decoding and defaults, including Unsupported for an
  unimplemented interpolation value; implicit filter periods at 1× and 2×,
  document-derived guide coordinates and fixed approximation behavior.
- Unknown drawable types in a known object layer, unknown future fields and
  encodings, unsupported relationships/limits, valid unknown extension members,
  malformed required records and unusable optional previews.

Regenerate [authored-filters.capy](../../crates/layer-core/src/package/codec/fixtures/authored-filters.capy),
including its Independent copy's projective/mesh placement, to the GA records.
Keep independent assertions for exact pixels, material data, filter parameters
and source interpretation. The existing
[140-case render suite](../../crates/layer-render-wgpu/src/package_render_tests.rs)
already covers all 52 built-ins, blend modes, watercolor, SDR rendition and FBM.
Replace its eight `placement/*` cases with image-object references covering both
GA kernels and affine/mirrored/minified/fractional cases. Preserve the other 132
baselines and their per-case tolerances; add the missing object, scale and
destructive-transform cases above. Keep transient paint perspective/mesh tests
even though those placements no longer occur in the file. Changing expected
images alone does not validate an effect change. Replace obsolete pre-release
representations without adding old readers.

### 14.2 Real host journeys

On GTK, Web, Android, Apple and Windows, exercise supported mouse, pen and touch
paths as applicable, in light and dark themes:

1. Open a photo, paint, grow the canvas, paint beside the original, transform,
   save, reopen and undo/redo the relevant edits.
2. Paste two external images, independently select/move/resize them, select an obscured
   image through the object list, reorder, hide, duplicate and delete it.
3. Cancel placement and transformation without residual records or history.
4. Paste Into a selection, move the image behind its mask, edit/unlink/relink the
   mask, and verify the target shown by the controls.
5. Paint above and below the object layer; convert in both directions and undo;
   verify that masks/effects/opacity are not applied twice.
6. Switch tools and layers with both object and pixel selections present. Verify
   that actions target the visibly selected kind and never write a render cache.
7. Save/open, autosave/restart, cross-document clipboard and export with the new
   records, including cancellation and write failure.
8. Copy/Cut a paint region, Paste or Paste in Place, move it, then paint and erase
   immediately. Compare with external paste and explicit Paste Into.
9. Use Move to select an object in another visible editable object layer, test
   additive selection and hidden/locked parents, and reach overlapping images
   through the list with keyboard and touch. The layer/mask target stays clear.
10. Attempt paint/erase on object content; exercise Add/Edit Mask, New Paint Layer
    and Rasterize Layer independently. No refused stroke replays unexpectedly.
11. Exercise section 8.4's merges, apply-mask conversion, alpha selection, property
    panels and off-frame bakes. Verify both cancellation and one-step undo.

Use private settings/documents, assigned devices and the existing host test
harnesses. Follow the [testing guide](../development/testing.md); documentation
or model tests cannot replace these journeys.
Implement and review object rows, Move selection/handles and refusal actions in
GTK first, in both themes, then complete native projections and repeat affected
journeys on every host. This does not authorize platform-local policy or defer
the other hosts past completion of the UI milestone.

### 14.3 Performance qualification

Measure the affected rows on each tier's reference hardware under the
[measurement rules](../performance/measuring.md): painting above and below a
multi-image object layer, object-transform motion, masks/clipping/effects,
pan/zoom/rotation, cache pressure and interrupted refinement. Include source
upload/cold-cache cases and the first stroke after a view change. Account for
conversion/admission failure without input stalls.
The milestone 2 tablet prototype is an earlier prerequisite, not a substitute
for these final measurements after the complete model/tool cutover.

For future vectors, extend the same workloads with many small strokes, one
complex path, active vector editing, high zoom, newly exposed regions and mixed
paint/vector compositions. Counts are workload inputs to qualify, not universal
performance promises. Record measurements in the tier tables and distinguish
unmet or unmeasured targets from passes.

### 14.4 What permits the freeze

Freeze only after the replacement schema and evaluators agree; sampling and
filter meanings are explicit and covered by fixtures; same-limit save/reopen
invariants hold; the adopted MIME identity stays consistent; real affected host
journeys pass; and required frame-time/latency evidence exists. Keep unsupported future records
preserved and existing GA meanings readable as features are added.

The architecture review identifies no required rewrite of the GA layer framework
for the listed vector/object workflows. It does not certify unimplemented
renderers, future typography/export fidelity, arbitrary resource usage or
unspecified product requirements. Those claims remain tied to the implementation
and acceptance evidence for each feature.
