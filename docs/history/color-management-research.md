# Color-management and raster-project implementation plan

**Goal:** Support the color workflows expected by prosumer painters,
photographers, and comic artists: reliable SDR editing, wide-gamut color,
profile-aware interchange, print proofing, and a staged HDR workflow. Preserve
low memory use, responsive 120 Hz drawing, and one maintainable GPU renderer.

**Direction:** Deliver normal 8-bit and 16-bit integer SDR editing, independently
of supported RGB gamut. Preserve source precision and reversible adjustments for
photo workflows. Use lossless layer/mask rasters and live adjustment metadata as
the durable project content. Linear RGBA16F is an HDR/extended-range candidate;
it is not a substitute for the precision contract of 16-bit integer photography.
Share Float32 processing and one renderer across storage modes. Remove the
current linear RGBA8 painting path. Full RGBA32F documents remain separately
scoped; higher-precision processing buffers do not require that document mode.

**No legacy support:** The raster-project replacement is a deliberate format/API
break. Do not build an old-project importer, migration framework, compatibility
renderer or old filter-ABI adapters. Removing legacy and unused code is an
explicit goal and a required exit condition of every replacement phase.

The raster-project and cleanup directions are user requirements. This plan
replaces the earlier open-ended research handoff and incorporates the
[code and product review](color-management-review.md) and the subsequent
16-bit-depth discussion. It defines work to implement and validate; it does not
claim that the new format, color pipeline, or performance targets already exist.
The proposed New default is encoded 8-bit sRGB, with independent profile and
depth choices. Open preserves supported source depth/profile by default; a photo
preset or an explicit preference can choose 16-bit SDR. HDR has its own range
contract. All modes and their workload limits need implementation validation;
normal 8-bit and 16-bit SDR support are product scope, not optional experiments.

The [color-management user journeys](../ui/color-management.md) specify the
proposed New/Open/Import, color entry, Export, proofing, repair and HDR flows,
including menu placement, defaults and ownership of settings. Treat those flows
as the UI acceptance contract alongside the technical gates in this plan.
The [four delivery milestones](color-management-milestones.md) define outcomes,
success criteria, replacement cutovers and correctness/performance gates.

Repository observations were checked at local commit `ebafa44` on 2026-09-13.
The review includes an analytical reproduction of import quantization and raw
allocation calculations. No new GPU performance or physical-display measurements
have been performed. Revalidate source observations on the implementation branch.

**Product scope and priorities**

**Reassessment:** This plan now starts from user outcomes. The earlier reasoning
overgeneralized a current linear-byte quantization defect, then overcorrected
defaults without examining other assumptions. The following decisions replace
those directions; current code below is implementation evidence, not a ceiling
on product requirements.

| Area audited | Decision after reassessment |
| --- | --- |
| Encoded 8-bit was an optional experiment; wide gamut implied float editing. | 8-bit and 16-bit SDR are ordinary choices. P3/Adobe RGB are independent gamut choices; ProPhoto merits a 16-bit recommendation. |
| All photographs should enter FP16 editing. | Preserve source profile/depth on Open. Ordinary 16-bit SDR precision and half-float range are different contracts; never silently substitute one for the other. |
| Source preservation versus reversible editing. | Make the saved adjustment workflow explicit: users can revise adjustments/masks after reopening. Keep source-backed image data separately from baked pixel edits. |
| Storage encoding versus artistic blending. | Specify blend/adjustment domains deliberately; changing bit depth must not change artistic blending policy. |
| Histograms and richer sampling mainly belonged to HDR. | SDR photo editing needs histograms, clipping inspection, averaged sampling and reversible tonal/color controls. |
| Export should normally adopt the proof profile. | Proof-only lab profiles and delivery profiles are distinct. Follow the destination's requirements; never infer conversion from importing a proof profile. |
| RAW and layered interchange were grouped with specialist VFX work. | They are real future audience needs, separately scoped after reliable developed-image editing and interchange. |
| UI scope and discoverability. | Put controls at New/Open, adjustments, color selection, Export and Proof. Reuse presets and normal document-copy actions. |
| Memory and latency claims. | Retain the plan's all-allocation budgets and measured latency gates. Source/cache/undo copies and scheduling matter; raw pixel-size ratios are not whole-app measurements. |

The [app audience](../../README.md) includes painters, photographers and comic
artists, with Linux first and native/mobile efficiency as a design goal.
Documented workflows in established editors inform these priorities; they are
not a substitute for feedback from our own users.

| Priority | User outcome | Scope |
| --- | --- | --- |
| Core SDR | Import, adjust, paint, reopen and export without unexpected color shifts or avoidable banding. | Profile-aware decoding; 8/16-bit SDR editing; editable adjustments/masks; histogram/clipping inspection; portable profiles; exact raster persistence; managed viewing; point/average sampling and numeric color entry. |
| Wide-gamut SDR | Use P3 colors and exchange photographs in common RGB spaces. | sRGB/P3 creation presets, Adobe RGB and ProPhoto photographic workflows, independent depth choices, explicit assign/convert policies and matching export metadata. |
| Print | Preview output limitations and deliver files for a specified printer/paper workflow. | ICC soft proof, rendering intent, black-point compensation (BPC), output-gamut warnings and paper simulation; separate proof and delivery settings. |
| HDR | Preserve and edit modern HDR photos, and publish intentional HDR and SDR versions. | Gain-map-aware input/output, reference-white semantics, HDR tool ranges, exposure/histograms, display headroom and SDR preview. |
| Future audience workflows | Develop camera originals and exchange editable artwork with other editors. | RAW development and scoped layered interchange (including PSD). The initial handoff is a developed, profiled 16-bit TIFF; do not claim that is a complete RAW or layered-exchange workflow. |
| Specialist/deferred | Prepress and production extensions. | Native CMYK layers, spot inks/separations, OCIO/ACES and full 32-bit-float documents. Preserve extension points without making these prerequisites. |

Procreate exposes sRGB, P3 and custom profiles; Affinity distinguishes opening an
image in its profile from placing it into an existing document; Krita provides
printer-profile proofing controls. Implement complete workflows around those
needs, rather than treating a wider color wheel as completion.
[Procreate profiles](https://help.procreate.com/procreate/handbook/colors/colors-profiles),
[Affinity color management](https://affinity.help/photo2/English.lproj/pages/Clr/ClrProfiles.html),
[Krita soft proofing](https://docs.krita.org/en/user_manual/soft_proofing.html).

**Verified implementation baseline**

| Area | Current behavior and starting points |
| --- | --- |
| Document | [Document](../../crates/layer-core/src/lib.rs) has no working-space or precision field. [Projects](../../crates/layer-core/src/project.rs) store editable operations/strokes and packed assets, then reconstruct artwork. Replace this format and remove its obsolete persistence/reconstruction paths. |
| Image import | Hosts pass bytes as `Rgba8Srgb`; assets support only that format and `R8Unorm`, without source-profile fields. Several hosts intentionally convert to sRGB before Rust receives pixels. |
| Paint/composite | [Renderer](../../crates/layer-render-wgpu/src/lib.rs): linear `Rgba8Unorm`, sparse 256×256 paint pages, lazy destination companions and separate scalar state. The composite occupies the full document dimensions. |
| Imported image residency | `prepare_owned_asset` retains CPU source bytes plus an immutable GPU source texture. Initialization allocates paint pages covering the image and copies decoded/premultiplied samples into them. |
| Effects | [effects.rs](../../crates/layer-render-wgpu/src/effects.rs) quantizes at physical pass boundaries; [scene.wgsl](../../crates/layer-render-wgpu/src/scene.wgsl) quantizes import initialization. [Filter helpers](../../assets/filters/effects.wgsl) include sRGB transfer functions, fixed luma coefficients, bounded HSL/curve domains and clamps. |
| Brush math | [material_brush.wgsl](../../crates/layer-render-wgpu/src/material_brush.wgsl) has linear/Oklab mixing with sRGB-specific matrices. [brush.rs](../../crates/layer-engine/src/brush.rs) has sRGB/HSL color dynamics. |
| Sampling | [ColorSampler](../../crates/layer-render-wgpu/src/color_sample.rs) copies four bytes and asserts RGBA8; [UI eyedropper](../../crates/layer-ui/src/eyedropper.rs) clamps to 0–1 and encodes sRGB. |
| Export/previews | [ReadbackImage](../../crates/layer-render/src/lib.rs) describes RGBA8 without color metadata. [PNG export](../../crates/layer-render/src/png_export.rs) writes tagged sRGB8. [Thumbnails](../../crates/layer-render-wgpu/src/thumbnails.rs) share that export conversion. |
| Presentation | [ViewportPresenter](../../crates/layer-render-wgpu/src/present.rs) selects sRGB encoding from texture format. Host setup uses default surface color configuration; this is not a complete document-to-display pipeline. |
| Picker | [ColorState](../../crates/layer-ui/src/color.rs) stores encoded sRGB; [Okhsv](../../crates/layer-ui/src/color/okhsv.rs) is fitted to sRGB; the wheel raster is opaque sRGB bytes. |

Implementation owners should use [architecture](../architecture.md),
[rendering](../internals/rendering.md), [project format](../reference/project-format.md),
[runtime filters](../reference/runtime-filters.md), and the
[Linux](../development/linux.md) / [Web](../development/web.md) guides.
Update those references when behavior ships.

**1. Establish color semantics independently of storage**

Create a small shared Rust color module. Keep document interpretation,
pixel representation, processing rules, and viewing settings distinct.

| Contract | Required information |
| --- | --- |
| Document color | Supported working RGB primaries/white point, color-model identity, range/reference-white interpretation and color-semantics version. A monitor profile is never the permanent definition of artwork colors. |
| Pixel layout | Channel model/order, integer or float sample type, byte order, transfer encoding, alpha association and checked row/tile sizes. Define bytes per sample/pixel separately from channel count. |
| Profiles | Built-in identifier plus definition version, or embedded ICC bytes and content identity. Deduplicate profile blobs; bound profile/parser/LUT resources and preserve source provenance. |
| Editing rules | Compositing and filter domains, quantization policy, valid RGB/alpha ranges and current effect-ABI identifier. |
| View | Document-to-output transform, optional proof configuration, exposure/tone mapping and current surface/display capabilities. Keep per-monitor state outside artwork history. |
| Output | Destination profile/encoding, bit depth, alpha handling, intent/BPC, gamut/tone mapping, dither and applicable metadata. |

Use document-native RGB coordinates with declared processing domains as the
first prototype; storage encoding is a separate choice. Offer sRGB and Display P3
creation presets, with sRGB as the safe initial color-space preset; add Adobe RGB
as an advanced photo/print option. Compare a canonical extended internal RGB
space only against concrete quality, complexity and performance evidence before
freezing the raster schema. Input/output ICC support does not require accepting
arbitrary device ICC profiles as painting spaces.

Honor supported source profiles and depth on image-open. An 8-bit JPEG does not
require automatic promotion; a 16-bit SDR TIFF must not be reduced to FP16 merely
because it is convenient for the renderer. Support deliberate promotion for later
work. Convert placed/pasted image colors into the existing document's editing
space, while retained image layers keep their native source data. Rasterizing or
merging into a lower-depth/narrower-range document is an explicit commitment.
If an input profile is unsupported as an editing space, preserve its source
samples/profile and choose a documented
supported working representation. Support a 16-bit ProPhoto SDR workflow without
first clipping to bounded sRGB; a native ProPhoto editing space is the direct
product representation. An alternate internal space must satisfy the same
precision/gamut and round-trip requirements. A developed ProPhoto image is
not a RAW-processing workflow.

Keep conversion and gamut mapping distinct. Preserve supported extended RGB
during editing; map only at explicitly bounded import/conversion/output
boundaries. Specify rendering intent and BPC per conversion/proof/export
transform. Use relative colorimetric plus BPC as the initial user-facing
conversion preset where applicable, with alternatives and preview. Platform
display conversion has its own policy; do not apply BPC twice.

Define white-point adaptation, including D50/D65 crossings. Linear primary/white
adaptation matrices can operate on premultiplied RGB; nonlinear transfer
functions, ICC LUTs and tone maps generally require straight RGB, followed by
reassociation when needed. Alpha is coverage, remains 0–1 and is never gamma
encoded. Define alpha-zero RGB and stable near-zero handling, finite-value
validation, float overflow and supported negative values. Do not clamp RGB to
alpha in an extended-range buffer. Standard conversion references include
white adaptation and distinguish encoded from linear spaces.
[CSS Color conversion reference](https://www.w3.org/TR/css-color-4/#color-conversion-code).

Apply this meaning to primary/secondary brush colors, gradients, fills, figures,
paper, saved swatches/presets, color dynamics, effect parameters, sampling and
cross-document transfers. Every numerical RGB readout must name its coordinate
space; conventional hex values retain a clearly defined sRGB meaning.

**2. Specify SDR precision independently from HDR range and computation**

The product requires normal 8-bit and 16-bit integer SDR workflows. Both support
sRGB and wider RGB spaces; P3 is not inherently a 16-bit or HDR mode. Recommend
16-bit for heavy editing and ProPhoto; retain user control. A 16-bit SDR source
must survive identity import/save/export without being narrowed to half-float.
Edited results must meet the declared 16-bit SDR precision contract throughout
processing and output, not merely reside in a 16-bit file container.

GIMP explicitly separates integer/float storage, encoding and Float32 processing.
Lightroom Classic documents 8/16-bit SDR exchange and recommends 16-bit ProPhoto
for detailed external editing. These support the required workflows, not any
particular GPU allocation design.
[GIMP encoding](https://docs.gimp.org/3.0/en/gimp-image-encoding.html),
[Lightroom external editing](https://helpx.adobe.com/lightroom-classic/desktop/work-with-external-editors/external-editing-preferences.html).

Choose native-depth integer SDR raster backing as the baseline design, using
Float32 processing and bounded intermediate caches. GPU formats/packing and
intermediate precision are implementation choices; an FP16-only intermediate
must not become a precision bottleneck for the promised integer16 path. Source
retention supports reversible workflows but cannot repair precision already
discarded in the edited result. Ordinary 16-bit SDR and half-float both require
eight raw bytes per RGBA pixel; selecting half-float alone offers no payload
memory saving over integer16.

`Rgba16Float` remains a useful HDR/extended-range candidate. Half-float offers
about 1,024 steps per exposure stop over 30 normal stops, plus subnormal range;
it does not uniformly represent 65,536 SDR levels. Full Float32 document storage
is not a prerequisite for ordinary photography or initial HDR, but arbitrary
32-bit source values cannot be claimed lossless through FP16.
[OpenEXR technical introduction](https://openexr.com/en/latest/TechnicalIntroduction.html).

| Representation | Purpose | Raw RGBA bytes/pixel |
| --- | --- | --- |
| Encoded RGBA8 | Standard SDR drawing/editing in supported RGB spaces. | 4 |
| Encoded integer16 RGBA | High-precision SDR raster backing/editing, including photographic round trips; choose actual GPU representation against this contract. | 8 |
| Linear RGBA16F | HDR/extended-range storage candidate; optional intermediates only where their precision is sufficient. | 8 |
| Native source-depth integer/float data | Preserve imported photo samples independently of edited raster precision; integer16 PNG/TIFF export is a separate encoding. | Depends on channels/sample type |
| Selective RGBA32F scratch | Numerically sensitive accumulation or algorithms demonstrated to need it. Not a full document mode. | 16 |

The current linear RGBA8 quantizer is inadequate as the unquestioned photo
default. Reproducing `scene.wgsl` import quantization gives only nine distinct
stored values for sRGB gray codes 0–50, and 183 for codes 0–255. For example,
codes 7 and 16 both become linear byte 1, approximately sRGB code 13 on export.
This is an analytical check, not a GPU capture. Premultiplication further reduces
straight-color resolution at low alpha.

Implement encoded SDR storage with Float32 arithmetic in the appropriate domain,
checking texture transfer behavior, alpha association, interpolation, destination reads,
readbacks and all pass boundaries. The `Srgb` texture suffix does not define
document primaries. Do not substitute formats without validating the complete
path. Never silently reduce a document's depth or range because of memory pressure.
The numerical problem above concerns the current linear-byte quantizer; it is
not evidence against encoded 8-bit sRGB. Test each mode against its stated
precision contract rather than requiring 8-bit to match 16-bit editing.

Keep Float32 computation common across modes; float16 texture storage does not
require `f16` shader arithmetic. Centralize format/size/transfer helpers and
small quantization/range-policy helpers. Cache compiled pipelines by actual
attachment/layout state, compile required variants outside stroke processing,
and pass transforms as shared uniforms/LUTs instead of generating shader families
for each profile/display. [WGSL texel formats](https://gpuweb.github.io/gpuweb/wgsl/#texel-formats),
[wgpu color targets](https://docs.rs/wgpu/30.0.1/wgpu/struct.ColorTargetState.html).

Choose active-stroke/filter/composite precision separately from durable tiles.
Float32 arithmetic does not eliminate quantization at intermediate stores.
Keep adjustment chains live and evaluate from their source state rather than
baking on every slider update. Use Float32 intermediates wherever required to
meet SDR integer16 or HDR contracts; bounded regions, reuse and lifetime control
limit their cost. Dither at intentional final precision reductions; it cannot
restore lost
detail or make bounded storage retain arbitrary HDR values. Keep masks, wetness,
coverage and other scalar state independently sized.

**3. Make the new project format raster-based**

Raster persistence does not imply flattening live photo adjustments or retaining
only a rendered result. Store editable exposure, white-balance/tint or neutral
point, Levels/Curves, hue/saturation and color-balance parameters with masks.
Keep source-backed image rasters and separate paint/retouch layers; distinguish
reversible adjustment state from undo snapshots and optional original containers.
Opening and revising an adjustment after saving is a core acceptance case.

Persist **lossless layer pixels and masks**, not a flattened composite alone and
not a requirement to replay all historical strokes. Retain layer/group order,
opacity, blend modes, clipping, transforms, selections and guides, and editable
adjustment definitions/parameters where supported. A saved composite/thumbnail
is a disposable preview; layer rasters and required metadata are authoritative.

The document-format design must include:

- Independently addressable raster tiles or chunks, explicit extents and pixel
  descriptors, sparse empty regions, lossless compression and an indexed
  manifest. Evaluate suitable containers and random access before choosing a
  codec/container; monolithic full-layer decoded buffers are not required.
- Embedded document/profile definitions, per-raster encoding/range/alpha
  metadata, processing versions for live adjustments, and future HDR metadata.
  Save actual committed samples exactly; saving must not convert to the current
  monitor or requantize artwork.
- Source-depth image records for nondestructive placed-photo workflows, including
  necessary original/gain-map data. Compress or back them on disk and bound
  decoded residency. Optional original containers are distinct from rendered
  layer tiles; avoid permanently retaining every full-size representation.
- Versioned manifests, checked dimensions/byte counts, decompression/profile
  limits, integrity validation, corruption handling and unsupported-feature
  errors. Include current `ProjectLimits` replacements and mobile/WASM limits.
- A stable save revision, asynchronous dirty-tile capture, unchanged-tile reuse,
  bounded worker queues, cancellation and atomic publication. A failed or
  cancelled save must not mark the document clean.
- An undo strategy based on affected raster regions, immutable tile references
  or lossless deltas plus metadata edits. Bound/compress/spill undo storage;
  retain one-step user operations. Device recovery uses committed raster state
  and any bounded unsaved recovery journal, not an archival replay requirement.

This changes a key performance property: current project saves need no canvas
readback because they serialize operations. Raster saves may require dirty-tile
GPU-to-CPU transfers unless those exact samples already exist in backing storage.
Schedule copies in GPU order, capture a consistent revision during continued
painting, and complete mapping/compression/file I/O off the input owner. Measure
save/autosave contention, readback staging peaks and repeated-save behavior.
Do not read back the whole document on every drawing frame.

Retain stroke/dab generation and state needed by live brushes. Remove historical
stroke reconstruction from document persistence, undo and device recovery once
raster state supplies those responsibilities. Do not preserve replay plumbing
for hypothetical future features. Transient wet-brush state must either settle
before a consistent save boundary or be explicitly persisted if resuming that
state is supported.

Reject unsupported project/schema/ABI identifiers with a clear error before
replacing the current document. Identifiers and integrity checks allow reliable
validation; they do not imply readers or migrations for previous formats.

**Required cleanup when each replacement lands**

- Delete superseded project codecs, stroke-archive serialization and replay-only
  state, operations, caches and entry points. Replace the current project-format
  reference with the new raster contract when it ships.
- Replace fixed RGBA8-only upload/readback/color interfaces across shared Rust,
  FFI and every host together. Remove redundant wrappers, old endpoints and
  temporary feature switches instead of keeping parallel compatibility APIs.
- Replace obsolete quantizers, shader variants, pipeline preparation and filter
  ABI handling. Keep only the new supported editing modes and current filter
  contract. SDR8 interchange, image previews and scalar masks remain valid
  features where specified; they do not justify an obsolete painting path.
- Remove unused dependencies, configuration fields, fixtures, scripts and tests
  tied solely to removed behavior. Replace relevant tests with new raster/color
  contracts and retain useful performance/correctness baselines.
- Audit all call sites, exports, host bindings and current documentation; build
  and run the appropriate shared/platform checks. A replacement phase is not
  complete with a dormant fallback or unreferenced implementation left behind.

**4. Define assignment, conversion and precision changes on raster states**

Use explicit commands and one-step undoable metadata/raster transactions.

| Action | Pixel and appearance contract |
| --- | --- |
| Assign profile | Preserve RGB numbers in the declared document numeric representation and change their profile interpretation. Preview appearance; do not equate this with leaving internal GPU bytes unchanged when storage encoding differs. |
| Convert editable layers | Transform raster content and color-bearing parameters into the destination editing space. Preserve originals through undo and preview the resulting complete stack; channel-based blend/effect behavior may change. |
| Convert flattened copy | Transform the current complete composition into a new raster document/output to preserve composite appearance subject to gamut/tone mapping. Keep the layered original. This is an explicitly selected workflow, not silent flattening. |
| Promote precision | Promote committed tiles without rerasterizing strokes or changing blend/adjustment domains. Preserve current artwork within the declared tolerance and apply higher precision to subsequent work. Do not claim to restore lost detail. |
| Reduce precision/range | Preview clipping/mapping/dither, convert deliberately, retain exact preconversion state through undo and persist the chosen result. |

A color transform does not generally commute with multiply/screen, HSL dynamics,
pigment mixing or nonlinear adjustments. Raster persistence removes dependence
on historical brush replay, but transforming each layer still does not guarantee
an unchanged blended stack. If a future feature promises both full editability
and preserved stack appearance across spaces, design explicit color-space
boundaries/isolated groups and validate that promise separately.

Conversion must be cancellable and atomic: prepare results within a peak-memory
budget, publish together, and preserve originals until success. Verify undo/redo,
save/reopen and device recovery from resulting raster states. Undo should restore
samples, not reverse a lossy color conversion mathematically.

**5. Remove import losses before pixels enter Rust**

| Host | Current boundary | Planned change |
| --- | --- | --- |
| GTK | [layers.rs](../../apps/layer-linux/src/layers.rs): GDK download to `R8g8b8a8`, uploaded as `Rgba8Srgb`. | Preserve source profile/depth before conversion; select download format and color state explicitly. GDK exposes both independently. |
| Web | [layers.js](../../apps/layer-web/layers.js): `createImageBitmap` → default 2D canvas → byte `ImageData`. | Use explicit color/depth-aware decoding where supported, and shared decoding where needed for consistent metadata and high-bit-depth input. Do not funnel all images through default SDR8. |
| Android | [Layers.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt): `BitmapFactory` → `getPixels(IntArray)` → RGBA bytes. | Preserve actual bitmap/source color space, depth and gain maps; avoid `getPixels` for richer input because that contract returns packed sRGB. |
| Apple | [LayerImageImport.swift](../../apps/layer-apple/Shared/Bridge/LayerImageImport.swift): 8-bit sRGB CGContext followed by byte unpremultiplication. | Extract source metadata and select an appropriate decode path first; avoid a forced premultiplied SDR8 intermediate. |
| Windows | [image_import.rs](../../apps/layer-windows/native/src/image_import.rs): `Rgba8`, straight alpha, `ColorManageToSRgb`. | Request source-appropriate samples and record conversion ownership; mandatory sRGB8 output cannot preserve wide-gamut/high-depth input. |
| Shared | [ProjectAsset](../../crates/layer-core/src/project.rs), [HostImage](../../crates/layer-render/src/lib.rs), FFI and upload methods. | Carry sample format, profile/encoding, alpha, dimensions/stride and applicable HDR/source metadata consistently; replace channel-count-as-byte-count assumptions. |

[GDK downloader contract](https://docs.gtk.org/gdk4/struct.TextureDownloader.html)
and [Android bitmap contract](https://developer.android.com/reference/android/graphics/Bitmap)
support the host-boundary distinctions above. Apple/Windows already perform
intentional source-to-sRGB conversion; never attach the original profile to
those converted bytes.

For ordinary untagged SDR RGB, assume sRGB and expose that assumption in image
properties with an assignment override. For malformed or unsupported tagged
input, retain the current document and report the unsupported interpretation;
allow an explicit user-selected fallback rather than silently discarding the
tag. Apply format-specific precedence for conflicting metadata and preserve
provenance. For example, a supported PNG `cICP` takes precedence over other
color chunks. [PNG 3 metadata](https://www.w3.org/TR/png-3/#11cICP).

Keep orientation and alpha correct through decoding and color conversion.
Bound decoded bytes before allocation, especially in mobile/browser hosts.
Perform profile transforms during ingestion or tile materialization, cache
results, and avoid full-document conversion during drawing. Retained raster
samples make subsequent reopening independent of decoder changes; optional
redecoding of original assets must remain an explicit versioned operation.

**6. Complete export, sampling, previews and interchange**

Replace the fixed RGBA8 `ReadbackImage` assumption with explicit pixel/color
descriptors. Keep document sample values separate from display/proof/export
results. Preserve asynchronous point sampling and latest-request coalescing;
update sample byte size and decoding for each mode, remove unintended RGB
clamps, and invalidate stale requests when document/color revisions change.

Add point/averaged sampling and ordinary SDR histograms/clipping indicators.
Define current-layer/visible-composite sources, area averaging, alpha handling
and whether an inspector reports document or output coordinates. Histogram and
sample computation must be asynchronous/bounded; HDR extends these tools later.

Use this initial format matrix as delivery scope. Each shipping row requires
cross-host fixtures, matching metadata and tested fallback behavior.

| Format/path | Planned support | Color/alpha policy |
| --- | --- | --- |
| New editable raster project | Lossless layer/mask tiles and editable structure. | Embedded working/profile definitions; preserve committed sample format/range exactly; optional native-depth photo sources. |
| PNG SDR | 8/16-bit integer import/export. | Tagged RGB/gray conversion, straight alpha, consistent ICC/standard chunks; explicit destination encoding and intentional 8-bit dithering. |
| JPEG SDR | Ordinary 8-bit interchange. | Honor/embed ICC; explicitly flatten transparency against the chosen background; named sRGB/P3/Adobe RGB destinations as supported. |
| TIFF photo/print | 8/16-bit integer RGB/gray import/export for photo handoff; profiled CMYK interchange for explicitly scoped print workflows. | Preserve source depth and honor the actual delivery profile. A supplied proof-only profile must not automatically become the file profile. Native CMYK editing is deferred. Specify supported variants and clear errors. |
| HDR JPEG/HEIF gain maps | HDR-stage ingestion and export selection. | Preserve base/map/metadata before edits; regenerate appropriate HDR/SDR relationship after edits, rather than reattaching a stale map. |
| AVIF/other HDR containers | Evaluate and select supported HDR interchange in the HDR stage. | Declare transfer, primaries, reference luminance and applicable metadata; platform/codec capability detection. |
| RAW development / layered PSD interchange | Future audience workflows with separate scope. | Developed, profiled TIFF is the initial photography handoff; it is not a RAW processor or editable-layer exchange. Define supported layers/adjustments and camera-development behavior before advertising those future routes. |
| EXR/other specialist interchange | Separately scoped production work. | Do not infer arbitrary source range/precision support from an FP16 texture. |

Document-open, place, drag/drop and clipboard image paths must share conversion
policies. Keep internal raster copying exact when source/destination meanings
match; convert deliberately when they differ. Preserve useful photo/print
metadata such as resolution and orientation where supported; distinguish
preserved metadata from tags invalidated by edits.

Export transforms depend on the selected destination, never on the current
monitor, checkerboard, selection overlay or proof-view toggle. Use bounded strip
or tile conversion/readback and streaming encoding where codecs permit. Capture
a consistent export revision without blocking input or requiring another
full-size CPU canvas. Never advertise a bit depth/profile unless the complete
path preserves it.

Thumbnails, navigator, filter previews, swatches and histograms need
explicit policies. Artwork previews use the same document viewing intent, with
an explicitly mapped SDR fallback where the UI surface cannot show HDR/wide
gamut. Histograms/readouts identify document versus output values; the document
eyedropper samples before viewing transforms. Include document/profile/view
revisions in relevant caches and asynchronous result identities.

**7. Keep one color engine and one current effect contract**

Evaluate LittleCMS as the reference ICC engine and moxcms as a Rust candidate.
Test v2/v4 matrix and LUT profiles, RGB/gray/CMYK conversion, intents/BPC,
proofing, signed/extended inputs, transform accuracy, WASM integration, licensing,
allocation peaks and throughput before selecting the production dependency.
LittleCMS documents v2/v4 and proofing support; moxcms lists RGB/gray/Lab/CMYK
transforms. [LittleCMS](https://www.littlecms.com/color-engine/),
[moxcms](https://github.com/awxkee/moxcms).

Use analytic standard-space transforms where appropriate and cached LUTs for
general profile viewing. Generate/profile-check LUTs outside the stroke path.
Specify domain/shaper, interpolation, error tolerance and behavior outside the
domain. A 0–1 LUT must not silently clip quality/HDR data. Cache identity includes
source/destination profile contents, intent/BPC, encoding/range, proof/view
settings and transform implementation version as applicable.

Inventory built-in and serialized custom WGSL by processing domain: linear RGB,
encoded RGB, perceptual color, scalar coverage, and obsolete bounded assumptions.
Parameterize appropriate transforms and luminance coefficients. Define HDR
semantics for blend modes, inversion, curves, LUTs, Oklab/pigment mixing and
filters individually; retain valid alpha/coverage constraints.

Treat artistic blending as a separate policy from storage depth and profile
conversion. Linear-light resampling/compositing, encoded-channel artistic blend
formulas and perceptual/pigment mixing have different purposes. Select and
document domains per operation; switching 8-bit to 16-bit must not implicitly
switch their artistic behavior. Compare common painting/compositing tasks, not
just physical-light formulas. Advanced blend-space controls are creative options
only where justified, not an old-renderer compatibility mode. Affinity exposes
blend gamma and GIMP separates blend from composite space.
[Affinity blend controls](https://affinity.help/photo2/English.lproj/pages/Layers/layerBlendRanges.html),
[GIMP layer defaults](https://www.gimp.org/man/gimprc.html).

Define a new runtime-filter ABI with explicit space/range expectations. Update
built-in filters, custom-filter examples, validation and all consumers together;
reject unsupported ABI versions and remove their handlers. New raster saves
remove historical brush replay, but live adjustments still need their exact
current programs/parameters and defined processing semantics. Preserve source
fusion and test fused/unfused paths against declared tolerances; physical pass
partitioning must not cause unexplained changes. Delete obsolete quantization
helpers and shader variants after their replacements pass.

**8. Negotiate presentation through the existing wgpu APIs**

Use pinned wgpu 30.0.1's `SurfaceConfiguration::color_space`,
`SurfaceCapabilities::format_capabilities`, `SurfaceColorSpace`, and
`Surface::display_hdr_info`. Select a supported format/color-space pair and
supply pixels in its required encoding. These APIs configure output, not a
complete source/document/display color transform.
[wgpu surface API](https://docs.rs/wgpu/30.0.1/wgpu/struct.Surface.html),
[wgpu color spaces](https://docs.rs/wgpu/30.0.1/wgpu/enum.SurfaceColorSpace.html).

| Host | Implementation direction | Validation required |
| --- | --- | --- |
| GTK/Wayland/Vulkan | Coordinate GTK color states and the app-owned canvas subsurface; negotiate surface descriptions through supported WSI/compositor paths. Inspect actual driver signaling before adding custom protocol plumbing. | Named output and custom monitor profiles; compositor capabilities; window spanning/moving monitors; matching GTK artwork controls and child surface. |
| WebGPU | Use supported sRGB/P3 and extended canvas modes; coordinate separate 2D contexts/`ImageData`. Extended browser sRGB/P3 uses encoded values, unlike native linear scRGB. | Browser/OS/display combinations, metadata-preserving previews, SDR fallback; `Auto` remains standard output even with FP16. |
| macOS/iPadOS/Metal | Coordinate CAMetalLayer color space/EDR with native artwork controls. Marshal macOS HDR queries from the main thread to the serial renderer; supplement missing iPad capability information. | Current headroom, brightness, display move, suspend/resume and mixed UI/canvas output. |
| Windows/D3D12 | Evaluate FP16 scRGB presentation for P3/HDR documents; DX12 has no wgpu Display P3 surface mode. Respect Advanced Color/ICC ownership and OS SDR white. | SDR/HDR/ACM transitions, monitor profiles and mixed displays, with no duplicate app/OS conversion. |
| Android/Vulkan | Coordinate supported surface modes with Android window/display color capabilities and source decoding. | Native wide-color/HDR output and SDR fallback on actual devices; unknown Vulkan luminance information is not proof of SDR. |

The table is an implementation/validation list, not a declaration of tested
device support. Wayland allows surface descriptions and compositor conversion
across outputs; supported description types vary. Windows' Advanced Color
pipeline changes display-profile responsibilities.
[Wayland color management](https://wayland.freedesktop.org/docs/book/Color.html),
[Windows Advanced Color](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range).

Refresh capability/view state after monitor moves, profile changes, HDR toggles,
resume and headroom/brightness changes. Distinguish unknown, unavailable and
supported-but-inactive capabilities. Prefer OS/compositor display conversion
where its contract is defined; use application transforms where required.
Honor calibrated monitor profiles without building an app-owned calibration
system. Define an explicit sRGB fallback and expose limitations accurately.

Keep view updates out of artwork history and avoid rerasterizing document tiles.
Update uniforms/LUTs where possible; surface reconfiguration can wait for GPU
idle and must not happen every frame. Validate transparent window edges,
checkerboards, overlays and UI reference white separately from artwork mapping.
Physical sRGB displays cannot reproduce every P3 color; preserve document values
and map the view. Screenshot bytes without reliable metadata are not physical
color-accuracy evidence.

**9. Complete the picker and print workflow**

Implement the [user-facing journeys](../ui/color-management.md) through shared
commands and native sheets. Expand the current dimensions-only New dialog,
project-only Open and PNG-only Export. Keep routine tagged-image handling
automatic, save opened photos into the new raster format without overwriting
their sources, and make the active document/view/output color state inspectable.
Source-profile repair must use retained source samples; once an image has baked
pixel edits, offer a corrected source as a new layer instead of replacing edits
or reconstructing them from old strokes. Share a profile chooser across document,
source, export and proof settings with role-appropriate validation.

Preserve the approved compact panel: circle is a smooth Okhsv field with OKLCH
readout, square HSV/HSB, triangle HLS; shape changes restore shape units and
saved RGB preference remains supported. Keep fixed digit placement, spacing,
small-panel behavior and remembered coordinates at black/neutral colors. Add
precise numeric color entry through a suitable additional control without
replacing the compact readout design.

Recalculate gamut boundaries for supported working spaces. Changing only the
sRGB matrix does not validate Okhsv's fitted coefficients. Test forward/inverse
mapping, boundary colors, neutral/black stability and cached raster performance.
The original article discusses the sRGB fit and wider-gamut/HDR follow-ups.
[Okhsv/Okhsl reference](https://bottosson.github.io/posts/colorpicker/).

Retain the hue guide's intent separately from actual selection: the current
smooth `get_ST_mid` guide, 5% margin, linear normalization and 24° rotation are
visual choices, not proof of a valid wider-gamut boundary or an HDR requirement.
Wheel pixels and swatches should match an unmodified opaque canvas patch through
the declared viewing transform. Keep RGB readouts in document coordinates and
the sample path independent of monitor conversion. Profile saved swatches and
define cross-document preset conversion. A temporary sRGB-only picker is a
prototype limitation, not completed wide-gamut editing.

Implement soft proof as a view configuration, with embedded/deduplicated printer
ICC, intent/BPC and separate proof-to-display settings. Include output-gamut
warnings, paper-white/black simulation, and comparison with an unproofed view.
Keep proof and delivery recipes distinct. A screen/export simulation can use the
export transform, but a printer/paper ICC may be supplied only for proofing.
Importing it must not convert the document or configure device-profile export.
WhiteWall explicitly prohibits converting/embedding its proof profiles; Saal
likewise specifies RGB delivery. Store their requested delivery profile/depth
separately, and require deliberate adoption when a destination really requests
device-profile conversion. Paper simulation and warning overlays never enter
exports. Reuse Save As or ordinary duplication for print-specific editable copies.

Printer ownership is not a prerequisite for implementation or software acceptance.
Profile import and proof/export must work with no installed printers. Validate
representative profiles against reference-CMM results and the managed display
path. Physical print comparison is an optional separate exercise using a lab or
external tester; record it as unverified until performed, without blocking the
software proofing milestone or claiming a verified screen-to-print match.
[WhiteWall profile instructions](https://service.whitewall.com/hc/en-us/articles/213813645-Does-WhiteWall-offer-color-management-ICC-color-profiles),
[Saal proof and delivery](https://www.saal-digital.eu/service/professional-zone/soft-proof-in-lightroom-photoshop-and-other-programs/).

Validate proofing of all SDR modes and mapped HDR output; do not assume CMM
gamut alarms accept arbitrary extended values. HDR-to-print proofing starts from
the authored SDR/print rendition. Validate profile-dependent behavior using a
reference CMM and real prints where available. Preserve working rasters throughout.

**10. Stage HDR editing and interchange**

Use a shared renderer with an extended-range storage path for HDR; this need not
be the same representation as integer16 SDR artwork. HDR alone does not require
a 32-bit document mode. Define the first HDR workflow around display-referred
photographs and painted content. Keep scene-referred inputs explicitly identified
and require a defined rendering transform before enabling that separate workflow.

Before exposing HDR editing, specify what document RGB 1.0 means, reference-white
units/defaults, source PQ/HLG/gain-map normalization, view exposure, tone mapping,
gamut mapping and export luminance metadata. Document reference settings are
portable; current display brightness/headroom is temporary view state. Do not
assume a fixed monitor peak or equate API support with available HDR headroom.

Provide HDR-aware curves/levels, exposure, sampling/numeric entry, luminance
histograms and clipping/gamut indicators. Ensure RGB above 1 and supported
negative values survive all intended tool/filter paths. An SDR hue/saturation
wheel alone does not expose HDR brightness.

Preserve base image, gain map and necessary metadata on import, even when only an
explicit SDR preview is currently available. For editing, reconstruct a defined
HDR raster representation; after arbitrary painting/filtering, generate the
gain map from the edited HDR and authored SDR renditions. Do not reuse a stale
source map. Lightroom's HDR export and Android's editing guidance demonstrate
why both renditions need deliberate handling.
[Lightroom HDR](https://helpx.adobe.com/lightroom/desktop/edit-photos/hdr-output.html),
[Android gain-map editing](https://developer.android.com/media/grow/ultra-hdr/edit).

Ship at least one tested HDR interchange route and intentional SDR export before
declaring HDR support complete. Specify supported containers/codecs per host and
clear import/export limitations, including tone-map behavior when sharing to SDR.

**11. Budget all retained data and measure the interaction envelope**

Raw payload of one full RGBA texture, excluding padding/mips/driver overhead:

| Dimensions | RGBA8 | RGBA16 integer or float |
| --- | --- | --- |
| 4,096 × 4,096 | 64 MiB | 128 MiB |
| 8,192 × 8,192 | 256 MiB | 512 MiB |
| 16,384 × 16,384 | 1 GiB | 2 GiB |

The current single-image 8192×8192 case can retain 256 MiB each of CPU source,
GPU source, paint pages and composite: **1 GiB** before filters and other costs.
Promoting only paint/composite to FP16 makes that subtotal **1.5 GiB**. These are
allocation calculations, not measured physical residency. The full composite and
full-size source textures also enforce device dimension limits.

Build an allocation ledger covering sources, paint, companions, scalar state,
composite, [image-stage caches](../../crates/layer-render-wgpu/src/scene_images.rs),
previews, surface buffers, source transforms/LUTs, undo, raster save snapshots,
export staging, decode/encode buffers and driver/process overhead. Existing
telemetry includes scene scratch but omits some CPU/GPU sources and external
readback tickets. Budget RAM and GPU allocations together on unified-memory
devices and measure actual process/system pressure.

Prioritize these independent experiments by that ledger:

- Source-backed tiles and copy-on-write painting versus eager materialization of
  every imported image; compressed/disk-backed sources with bounded decode cache.
- Tiled composite residency and lower-resolution zoomed-out representations,
  separately from tiled neighborhood/global filter caches.
- Bounded import, conversion, save/autosave and streaming export, including
  queue depth, cancellation and simultaneous old/new raster states.
- Lossless cold-tile/undo storage, eviction/prefetch and recovery. Derived
  composite tiles may be rebuilt; authoritative edited tiles must be retained
  losslessly or durably backed before eviction.
- Mip/filter-halo invalidation and export correctness. Tiling does not reduce
  fully painted pixel count; zoomed-out views expose the entire document and
  arbitrary filters may require global work.

The raster format's random access and tile reuse should support this memory
strategy. A universal cache rewrite is not required before a color prototype;
a large-document quality default does require demonstrated memory limits.
Apply backpressure or explicit workload limits before allocation failure;
never discard authoritative samples or silently lower precision.

A raw 33³ RGBA16F LUT is about 0.274 MiB and a 65³ LUT about 2.095 MiB.
These are sizing examples, not accuracy prescriptions. Bound and share LUT
caches; large raster representations are the primary allocation concern.

120 Hz allows approximately 8.33 ms per frame. Doubling storage increases
bandwidth, but does not establish a 2× frame-time cost. Measure CPU input-to-submit,
GPU completion, presented cadence and input-to-present latency separately,
including p95/p99/max, missed deadlines and sustained thermals. Define actual
device/workload envelopes and numeric memory/latency gates before changing
shipping defaults.

Use [GPU benchmark guidance](../development/gpu-raster-benchmarks.md) and
[GPU brush reference](../reference/gpu-brush-engine.md). Include the existing warm
incremental/no-readback drawing baseline, sparse and dense canvases, many layers,
large brushes, smudge/wet media, filters, pan/zoom, cold tiles, proof LUTs,
conversions, export and raster save/autosave contention. Measure initialization,
pipeline creation and surface changes separately from warm strokes. Offscreen
timings do not establish on-screen 120 Hz behavior.

Include representative 24, 45 and 60 megapixel photo tasks, large illustrations
and multiple open documents in the workload definition. These are test targets,
not a claim about measured device support or audience usage shares. Do not
inherit the current 8192-pixel import/UI limit as the product's photography
requirement. Budget tile residency, retained sources, undo and background saving
before deciding supported limits; a raw bytes-per-pixel ratio is not a whole-app
memory or speed ratio.

**Integer16 versus half-float performance assessment (2026-09-13)**

Integer16 SDR can cost more than linear FP16 in the proposed architecture, but
there is no inherent 2× storage or arithmetic penalty between them. Both RGBA
formats occupy eight raw bytes per pixel and can use the same Float32 shader
math. UNORM texture access supplies normalized floating-point values; FP16
texture access supplies floating-point values too. Neither requires `f16`
arithmetic. Equal payload and shader arithmetic do not establish equal texture,
blend or whole-frame throughput.
[WGSL texel formats](https://www.w3.org/TR/WGSL/#texel-formats).

Separate three costs: the hardware format, encoded versus linear processing,
and the precision required at intermediate stores. Comparing encoded integer16
with linear FP16 changes all three; do not attribute every difference to integer
storage. These are code-based performance risks and analytical payload estimates,
not measured slowdowns; this research environment exposes no hardware GPU.

| Work | Expected difference and design consequence |
| --- | --- |
| Pointwise curves/color adjustments | Shared Float32 math and equal raw source/destination size give a similar starting cost. Transfer conversions depend on the effect's domain; encoded-space operations may instead need extra conversions from linear FP16. Fuse compatible operations. |
| Linear-light resizing, transforms, blur and smudge | Encoded integer16 needs decoding before interpolation, using explicit taps or a decoded cache. Linear premultiplied FP16 can use hardware filtering directly where its precision suffices. Repeated sampling is a major comparison workload. |
| Repeated brush dabs and normal layer blending | Encoded targets cannot directly use fixed-function blending for linear-light results. Preserve batching through appropriately precise working tiles; avoid a decode/blend/encode pass for every dab. |
| Multi-pass adjustments and compositing | Preserving integer16 precision may require wider intermediate textures than an FP16 document's contract. This can increase bandwidth and reduce cache residency. Both modes may need Float32 accumulation for low-flow painting or numerically sensitive effects. |
| Pan/zoom with resident display tiles | Reuse the same managed display-cache path where the view permits it. Rebuilding document layers solely for camera movement is avoidable. Cold tiles, mip changes and invalidated adjustments have separate costs. |
| Save/export | Native-depth raster tiles permit direct lossless persistence; conversion, compression and staging depend on output and contents. Neither format has a universal speed or compression advantage. |

An FP16 cache used only for display must not become the source for integer16
edits, document-value sampling or export. If both modes require Float32 working
tiles for a given operation's quality, much of the intermediate-buffer cost is
shared rather than an integer16-specific penalty.

`Rgba16Unorm` normalizes integers; it does not decode a transfer curve. There is
no corresponding automatic sRGB16 texture format in the pinned API. For
linear-light filtering, `decode(lerp(encoded))` is not
`lerp(decode(each sample))`: halfway between encoded sRGB black and white decodes
to approximately 0.214 linear, whereas their linear average is 0.5. Straight
source colors also need premultiplication before coverage-aware interpolation.
Four explicit bilinear taps add shader work, but do not imply four times the
memory traffic: hardware bilinear filtering already accesses neighboring texels.
Do not change artistic blend domains to make one storage mode benchmark faster.

This matters in our code: [dry-brush pipelines](../../crates/layer-render-wgpu/src/lib.rs)
use fixed-function source-over, [scene jobs](../../crates/layer-render-wgpu/src/scene.rs)
batch consecutive draws into one attachment pass, and
[effect sampling](../../crates/layer-render-wgpu/src/effects.rs) uses hardware
filtering. General shader-based layer blending already reads front/back inputs;
its incremental cost differs from replacing the inexpensive dry-brush path.

For a simple full-coverage shader pass reading one layer and a separate
accumulator and writing a new accumulator, logical pixel payload is:

| Layer / accumulator / output | Bytes per processed pixel |
| --- | --- |
| RGBA16 / RGBA16 / RGBA16 | 24 |
| RGBA16 / RGBA32F / RGBA32F | 40 |
| RGBA32F / RGBA32F / RGBA32F | 48 |

The mixed case is 1.67× and the all-Float32 case 2× that pass's logical payload;
these are not frame-time or physical-DRAM ratios. Caches, on-chip attachment
storage, compression, overdraw and pass fusion change actual traffic. A 256²
RGBA32F scratch tile is 1 MiB versus 0.5 MiB for either RGBA16 format. Bound the
number and lifetime of working tiles, including halos. Do not permanently
duplicate every layer in Float32, or assume every integer16 operation needs a
Float32 texture. Fused operations can retain Float32 values in shader registers;
materialized intermediates need a precision choice based on their results.

Validate capabilities before selecting the fast paths. The pinned wgpu 30.0.1
labels `TEXTURE_FORMAT_16BIT_NORM` native-only; its guaranteed `Rgba16Unorm`
usages omit render attachments. Query adapter-specific format usages and enable
the required features rather than changing `COLOR_FORMAT` alone. `Rgba16Uint`
can preserve integer samples but lacks ordinary hardware filtering/blending.
Float32 filtering and blending are separate optional capabilities; an explicit
shader compositor does not require fixed-function Float32 blending.
[wgpu features](https://docs.rs/wgpu/30.0.1/wgpu/struct.Features.html),
[format capability API](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html#method.guaranteed_format_features).

This is not a universal hardware limitation: Apple's tables list filtering and
blending for both RGBA16Unorm and RGBA16Float, and Chrome 142 introduced
`texture-formats-tier1` with normalized16 render/blend support. Evaluate the
actual Rust/browser binding and device support, including a dependency update
if appropriate, before choosing a permanent fallback. Capability tables do not
prove throughput parity.
[Metal capabilities](https://developer.apple.com/metal/Metal-Feature-Set-Tables.pdf),
[Chrome texture formats](https://developer.chrome.com/blog/new-in-webgpu-142).

Add two benchmark comparisons through [milestones 1, 2 and 4](color-management-milestones.md):
first isolate formats using matched domains, pass counts, sampling and
intermediate precision; then compare complete
integer16 SDR and FP16 pipelines against their respective quality contracts.
Include cold/warm 24–60 MP adjustments, transformed layers, blur/smudge,
low-opacity repeated dabs, deep layer stacks and save/export contention. Record
format capabilities, pass counts, peak/cache allocations, GPU completion and
p95/p99 input-to-present latency on discrete/integrated desktop GPUs, Apple,
Android and actual browser paths. The current benchmark harness exercises the
existing renderer, not an implemented integer16/FP16 comparison. Specify quality
error limits alongside latency gates so a faster result cannot pass by losing
the integer16 precision, alpha behavior or blend semantics being promised.

**12. Delivery milestones, owners and exit gates**

Shared Rust owns document/color semantics, validation, raster history and
rendering. Hosts own native decoding/file transport, surfaces and display events.

Use the [milestone map](color-management-milestones.md#milestone-map) and its
explicit success criteria as the execution plan. There are four milestones and
no prescribed PR count:

1. **Raster foundation and efficient SDR8.** Establish baselines and measure the
   working-buffer strategy before freezing the format. Replace save/undo/recovery
   and encoded sRGB8 rendering together, deleting their predecessors.
2. **Complete SDR color and photo editing.** Bound large-document residency, then
   complete integer16, wide-gamut viewing/editing, photo controls/inspection,
   assignment/conversion and profiled interchange.
3. **Print proofing.** Deliver reference-checked simulation and independent
   proof/delivery settings.
4. **HDR editing and delivery.** Deliver half-float editing, HDR-aware tools,
   intentional SDR renditions, display integration and tested HDR interchange.

Measurement, host qualification and cleanup are part of each milestone rather
than separate milestones. Split supporting changes only where useful for review;
keep each production replacement coherent and main usable after each merge.

Each merge has relevant correctness, memory and latency evidence. Compare with
both its parent and the fixed program baseline; a succession of small regressions
must not escape detection. New precision modes have explicit device/workload
budgets rather than a claim of equal cost. Cleanup accompanies each replacement;
audit current callers and remove unselected prototypes before declaring it done.

**Acceptance corpus and completion criteria**

| Area | Required cases |
| --- | --- |
| Profiles/metadata | Tagged and untagged sRGB/P3/Adobe RGB/ProPhoto; v2/v4 matrix and LUT ICC; RGB/gray/CMYK sources; D50/D65 neutrals; malformed/unsupported/conflicting tags; metadata matching exported samples. |
| Precision | Identity native 8/16-bit SDR round trips without hidden FP16 reduction; 8-bit P3; opaque/transparent ramps, low-flow accumulation, aggressive curves, repeated adjustments, gradients, resizing and long chains against mode-appropriate references. Distinguish integer16 precision from half-float range. |
| Raster persistence | Exact layer/mask/sample/profile round trips for each supported new format, sparse/dense content, tile edges, concurrent save/edit, corrupt/truncated files, cancellation, limits and autosave/recovery. Reject unsupported identifiers; no hidden stroke dependency. |
| Conversion/history | Assignment in declared numeric coordinates versus conversion; layered versus flattened-copy behavior; precision changes without changing blend domains; source correction; live adjustment editing after reopen; one-step undo/redo and device recovery. |
| Rendering | Fused/unfused effects, current custom WGSL ABI and unsupported-ABI rejection, all relevant blend domains, Oklab/pigment dynamics, extended/negative RGB, alpha-zero/near-zero, finite/overflow checks, tiled/filter dependency boundaries. |
| Viewing/tools | Picker versus opaque patch, swatches, thumbnails/navigator, point/average samples, SDR/HDR histograms, numeric entry, separate proof/delivery recipes, multi-monitor/profile/HDR changes and native/browser fallbacks. |
| Interchange/HDR | 16-bit source preservation and export, orientation/alpha, print profile output, gain-map reconstruction/regeneration, authored SDR rendition, matching luminance/transfer metadata and target-app reopen. |
| Efficiency | Measured peak/steady RAM/GPU allocations, undo/source/cache limits, dense photos and sparse paintings, cold tiles, save/autosave/export contention, conversions and multiple documents; sustained frame/latency results on named devices. |
| Cleanup | Superseded project/replay/color/filter paths, old API exports, unused dependencies and obsolete tests/fixtures/docs removed; all current consumers use the replacement contracts; unselected prototypes deleted. |

Preserve the existing renderer's incremental work and GPU-resident drawing path.
Add only tests that establish the new contracts or guard identified quality and
data-integrity risks. Record measurements, supported combinations and remaining
limits with each milestone; do not mark color management complete from a texture
format change or a successful P3/HDR presentation demo.
