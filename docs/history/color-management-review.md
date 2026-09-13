# Color-management research review

**Superseded directions:** After this review, the user specified a raster-based
project format, no legacy support, and explicit removal of legacy/unused code.
The [implementation plan](color-management-research.md) incorporates those
requirements. Recommendations below to preserve old project replay, old rendering
modes or compatibility paths are historical findings, not implementation tasks.
The later [product reassessment](../ui/color-management.md) also supersedes the
optional-8-bit recommendation, the FP16-only SDR direction, and any assumption
that a proof profile should automatically be used for delivery. Read this review
as the original audit record, not the current feature/default decision.

Reviewed 2026-09-13 against local commit `ebafa44`, the working-tree
[research handoff](color-management-research.md), application source, the installed
wgpu 30.0.1 source, and the primary sources linked below. Line references to the
handoff describe its 254-line version at review time.

**Recommendation:** Retain the shared RGB renderer and separation of color
meaning from storage. Strengthen the proposal around reliable SDR photo editing,
profile-aware interchange, print proofing, and replay compatibility. Prototype
linear RGBA16F for quality editing, and evaluate an encoded 8-bit economical mode;
the current linear RGBA8 path should be treated as a legacy/limited-quality mode.
Choose shipping defaults only after measuring representative memory and latency.

This review includes source inspection and a small numerical reproduction of
the import quantizer. It does not include new GPU performance measurements,
physical display validation, or a working color-management prototype. Product
priorities below are recommendations inferred from the app's stated audience and
documented editor workflows, not results of a user survey.

**What the intended audience will need**

The [README](../../README.md) explicitly targets painters, photographers, and
comic artists, with Linux first and mobile efficiency as a design goal. That
calls for more than selecting P3 colors.

| Workflow | Recommended scope and evidence |
| --- | --- |
| Draw for screens | sRGB and Display P3 documents, colors consistent between tools and artwork, portable saved profiles, and explicit sRGB/P3 export. Procreate already exposes both spaces and custom profiles. [Procreate profiles](https://help.procreate.com/procreate/handbook/5.2/colors/colors-profiles). |
| Edit photographs | Preserve imported profiles and high-bit-depth source data; support Adobe RGB/ProPhoto inputs, smooth shadows, repeatable adjustments, and high-bit-depth interchange. Affinity distinguishes opening an image in its profile from placing it into an existing document's space. Adopt that distinction when adding ordinary image-open workflows. [Affinity color management](https://affinity.help/photo2/English.lproj/pages/Clr/ClrProfiles.html). |
| Prepare prints/comics | Printer/paper ICC soft proof, output-gamut warnings, rendering intent and black-point compensation, plus a specified print export route. Krita documents these controls, including paper-white/black simulation. RGB editing with proofing can serve this workflow before a native CMYK editing engine. [Krita soft proofing](https://docs.krita.org/en/user_manual/soft_proofing.html). |
| Move art between apps/devices | Assign versus convert, embedded profiles, explicit missing/unsupported-profile policies, and predictable paste/place/export behavior. Photoshop exposes conversion intent, black-point compensation, preview, and dithering. [Photoshop profile conversion](https://helpx.adobe.com/photoshop/desktop/adjust-color/color-profiles/change-color-profile-for-documents.html). |
| Edit/share modern phone photographs | Preserve HDR information even on an SDR editing display, then provide HDR and authored SDR output. Lightroom documents HDR editing, SDR preview, and gain-map JPEG export. Android warns that compositing/filtering can invalidate an unchanged source gain map. [Lightroom HDR](https://helpx.adobe.com/lightroom/desktop/edit-photos/hdr-output.html), [Android HDR editing](https://developer.android.com/media/grow/ultra-hdr/edit). |

Native CMYK layers, spot inks, prepress separations, RAW development, and
OCIO/ACES production workflows should have explicit deferred scope. Their needs
should inform extensible metadata, but this upgrade does not need to implement
all of them. In particular, supporting a developed ProPhoto TIFF is distinct
from building a camera RAW processor.

**1. High priority: make SDR precision a quality requirement**

Handoff lines 89–130 correctly distinguish float storage from arithmetic, but
the label “Economical SDR” understates the current format's quality cost.
[Image initialization](../../crates/layer-render-wgpu/src/scene.wgsl), operation
`8u`, explicitly decodes sRGB, premultiplies, and rounds to 8-bit linear storage.
This loses shadow detail even for an opaque, ordinary 8-bit sRGB photograph.

A numerical reproduction of that shader's quantization, using the standard sRGB
transfer function and nearest rounding, gives:

| Original sRGB gray byte | Stored linear byte | Approximate re-encoded sRGB byte |
| --- | --- | --- |
| 6 | 0 | 0 |
| 7 | 1 | 13 |
| 16 | 1 | 13 |
| 24 | 2 | 22 |

Across input codes 0–50 inclusive, only **nine** distinct linear bytes survive;
across 0–255, only 183 survive. These are analytical quantizer results, not a
captured GPU image. Premultiplied low-opacity paint has an additional problem:
at fixed nonzero alpha, a one-byte RGB change corresponds to approximately
`1 / (255 * alpha)` in straight linear color.

Change the document to require shadow ramps, transparent gradients, low-flow
airbrush accumulation, and repeated adjustments as acceptance cases. Compare:

| Candidate | Assessment |
| --- | --- |
| Existing linear RGBA8 | Preserve for old-project rendering; unsuitable as the unquestioned photo-quality default. |
| Encoded RGBA8 storage with linear shader math | Worth prototyping at four bytes/pixel. Evaluate hardware sRGB texture transfer behavior, premultiplication, filtering, destination reads, and every pass boundary. The texture's `Srgb` suffix controls transfer behavior; document primaries still need explicit meaning. This is not a drop-in format substitution. |
| Linear RGBA16F | Strong first candidate for quality SDR and future HDR. Greater range and reduced quantization, at eight bytes/pixel. Benchmark before choosing defaults. |

Use Float32 shader arithmetic across supported modes. Consider higher-precision
scratch/active-stroke accumulation independently from persistent storage, while
recognizing that writing back to RGBA8 still loses information. Apply dithering
at intentional final precision reduction; it cannot recover already lost detail.
Krita's own workflow guidance also treats linear editing as requiring greater
than 8-bit precision. [Krita color-managed workflow](https://docs.krita.org/en/general_concepts/colors/color_managed_workflow.html).

**2. High priority: replace the import question with a verified boundary inventory**

Handoff lines 28 and 61 leave host decoding largely for later investigation.
The current paths already establish where information disappears:

| Host/boundary | Verified implementation | Required change |
| --- | --- | --- |
| GTK | [layers.rs](../../apps/layer-linux/src/layers.rs), around line 956: GDK decode, `TextureDownloader`, `R8g8b8a8`, then `Rgba8Srgb`; no profile is transported. | Select and record the download color state and depth explicitly; preserve source metadata before conversion. GDK exposes color-state selection independently of pixel format. [GDK downloader](https://docs.gtk.org/gdk4/struct.TextureDownloader.html). |
| Web | [layers.js](../../apps/layer-web/layers.js), around line 52: `createImageBitmap`, default 2D `OffscreenCanvas`, default `getImageData`, byte upload. | Avoid forcing every input through the default SDR byte path; preserve metadata and specify decode/conversion ownership. Browser image decoding alone does not supply a portable source-profile record. |
| Android | [Layers.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt), around line 131: `BitmapFactory`, `getPixels(IntArray)`, then RGBA bytes. | Preserve source precision/color space and gain-map data. `getPixels` explicitly returns packed sRGB colors regardless of the bitmap's richer representation. [Android Bitmap contract](https://developer.android.com/reference/android/graphics/Bitmap#getPixels(int[],%20int,%20int,%20int,%20int,%20int,%20int)). |
| Apple | [LayerImageImport.swift](../../apps/layer-apple/Shared/Bridge/LayerImageImport.swift), around line 27: 8-bit sRGB CGContext, then byte unpremultiplication. | Obtain source profile/depth/HDR information before this conversion. Avoid the low-alpha precision loss of a forced premultiplied 8-bit intermediate. |
| Windows | [image_import.rs](../../apps/layer-windows/native/src/image_import.rs), around line 142: `Rgba8`, straight alpha, `ColorManageToSRgb`. | Retain actual source color metadata and request an appropriate decode format instead of mandatory sRGB8 output. |
| Shared asset/project | [project.rs](../../crates/layer-core/src/project.rs): only `R8Unorm` and `Rgba8Srgb`; asset fields are extent, format, bytes. | Add sample representation, encoding/profile identity, alpha convention, and applicable HDR metadata. Update transfers, validation, serialization, FFI and limits together. |

Apple/Windows already perform an intentional conversion to sRGB; the problem
is loss of gamut/depth and source identity, not simply an absence of all color
management. Do not attach the original P3/Adobe RGB profile to bytes that a host
has already converted to sRGB.

Specify policies for untagged, malformed, unsupported, and conflicting metadata.
An ordinary untagged SDR fallback may be sRGB, but unsupported tagged data should
not silently be treated as an untagged image. Define format-specific precedence:
for example, PNG 3 gives a supported `cICP` chunk precedence over other color
chunks. Profiles alone are insufficient for every modern image format.
[PNG 3 color metadata](https://www.w3.org/TR/png-3/#11cICP).

Preserve an authoritative source representation before destructive conversion.
Decide whether that is the original encoded file, lossless decoded samples plus
metadata, or both on demand. Retaining original files can save decoded memory,
but replay must also address decoder/version differences; retaining every
representation indefinitely would undermine the memory goal.

**3. High priority: export, sampling, and previews need typed color contracts**

Handoff lines 65 and 202–215 state the desired behavior without capturing the
current shared API restrictions:

- [ReadbackImage](../../crates/layer-render/src/lib.rs) is explicitly RGBA8 and
  has no profile, pixel format, or transfer metadata.
- [PNG export](../../crates/layer-render/src/png_export.rs) always writes 8-bit
  RGBA and a matching sRGB chunk. Existing exports are tagged; the gap is that
  there is only this SDR route.
- [ColorSampler](../../crates/layer-render-wgpu/src/color_sample.rs) asserts
  RGBA8 and copies four bytes. The [UI eyedropper](../../crates/layer-ui/src/eyedropper.rs)
  then clamps RGB to 0–1 and encodes sRGB. Its asynchronous/coalesced scheduling
  is worth retaining while replacing the value contract.
- [Thumbnails and UI readbacks](../../crates/layer-render-wgpu/src/thumbnails.rs)
  share the export shader and fixed sRGB output. Navigators, filter previews,
  swatches, and any future histograms need an explicit viewing policy too.

Add an import/export matrix listing format, bit depth, alpha, color metadata,
HDR behavior, and support per host. Recommended initial targets are tagged
sRGB/P3 PNG, conventional JPEG interchange, and a 16-bit PNG/TIFF path for photo
work. TIFF/other codecs are new product work, not features implied by FP16 GPU
support. Add gain-map JPEG/HEIF or AVIF as evaluated HDR follow-ups.

Separate document sampling, display preview, soft-proof preview, and export
transforms. Display profile changes must never change the RGB returned by the
document eyedropper or the export. Keep transparent export independent of the
viewport's checkerboard, overlays, and proof-view setting.

**4. High priority: use the color APIs already present in wgpu 30.0.1**

Handoff lines 208–215 omit a useful existing dependency capability. Both the
installed source and published 30.0.1 documentation expose
`SurfaceConfiguration::color_space`, `SurfaceCapabilities::format_capabilities`,
`SurfaceColorSpace`, and `Surface::display_hdr_info`. The inspected host setup
uses default configurations without selecting document-aware output color
spaces. Start with these APIs and add host integration for the remaining gaps.
[wgpu surface API](https://docs.rs/wgpu/30.0.1/wgpu/struct.Surface.html),
[wgpu color spaces](https://docs.rs/wgpu/30.0.1/wgpu/enum.SurfaceColorSpace.html).

| Platform | Integration direction and limitation |
| --- | --- |
| GTK/Wayland/Vulkan | Negotiate supported format/color-space pairs for the app-owned child surface. Inspect driver/compositor signaling and GTK color states together; the child does not inherit GTK's image transform. Handle preferred-description changes and missing protocol capabilities. Wayland supports surface descriptions and multi-monitor conversion, with optional ICC/parametric support. [Wayland color management](https://wayland.freedesktop.org/docs/book/Color.html). |
| WebGPU | Use wgpu's supported sRGB/P3 and extended canvas modes. The browser's extended sRGB/P3 output is encoded, unlike native linear scRGB. `Auto` retains standard browser output even for an FP16 canvas. Coordinate separate 2D UI canvases. |
| macOS/iPadOS/Metal | Coordinate layer color space, EDR enablement and UI previews. The app's Swift canvas layers currently declare sRGB. In pinned wgpu, macOS HDR queries require the main thread; iOS does not supply the same headroom information. Transfer capability updates to the serial render owner. |
| Windows/D3D12 | Evaluate FP16 scRGB for wide-gamut/HDR presentation. wgpu does not report a Display P3 surface mode on DX12; P3 document colors can still be converted into scRGB. Account for Advanced Color/ICC ownership and OS SDR-white settings. [Windows Advanced Color](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range). |
| Android/Vulkan | Query surface support and integrate Android wide-color/HDR window and display state as needed. Do not infer presentation capability from successful high-precision bitmap decoding; pinned wgpu's non-Windows Vulkan HDR information may be unavailable. |

The platform directions above are API/source findings, not a tested support
matrix. Unknown display information must remain distinct from known SDR.
Re-evaluate on monitor moves, profile changes, HDR toggles, resume, and brightness
or headroom changes. Keep these events out of document history. Rebuild viewing
LUTs or presentation state without rerasterizing the artwork. Surface
reconfiguration can wait for GPU idle, so it must not be a per-frame operation.

**5. High priority: conversion and replay need an explicit appearance contract**

Handoff lines 244–247 acknowledge replay but understate the design consequence.
[CanvasEngine undo/redo](../../crates/layer-engine/src/canvas.rs) marks image
changes for reconstruction, and projects save strokes, operations, sources, and
filter programs. Updating texture bytes is therefore insufficient: a later undo,
reopen, or device recovery may reconstruct different colors.

Nor is converting each historical brush color generally equivalent to converting
the finished painting. Channel-wise blend modes, HSL dynamics, bounded
adjustments, pigment mixing, and quantization do not generally commute with a
change of RGB basis. Converting every layer separately also need not preserve
the appearance of the blended stack.

Specify three separate actions: assign a profile, change editing space, and
change precision. For each, state what happens to existing appearance, editable
history, and future painting. Preserve old processing semantics using versioned
render rules and source/profile data, or design explicit conversion boundaries
or baked snapshots where needed. Do not promise both arbitrary space conversion
and identical fully editable replay without a demonstrated design.

Precision promotion alone needs the same decision: replaying all old strokes at
FP16 removes previous rounding and can change existing art. Test conversion,
undo/redo, save/reopen and device recovery as a single workflow. Persist profile
bytes or stable built-in definitions; a profile name installed on one machine
does not make a project portable. Include transform/render-version changes in
the compatibility policy.

**6. Medium priority: make transform math and filter semantics explicit**

Expand handoff lines 52–82 and 116–121 into a small semantic contract:

- Describe working primaries, white point, numerical encoding, RGB range, alpha
  association, and processing version independently. Define white adaptation
  when crossing D50/D65 spaces, rather than assuming every conversion is only a
  3×3 primary change. Standard-space conversion examples include this step.
  [CSS Color conversion reference](https://www.w3.org/TR/css-color-4/#color-conversion-code).
- Apply nonlinear transfer functions, ICC LUTs, and tone maps to straight color
  with defined zero/near-zero-alpha behavior, then reassociate when required.
  A linear matrix can operate directly on premultiplied RGB; a nonlinear
  transform generally cannot. Alpha remains coverage and is not gamma encoded.
- Preserve legitimate extended/negative RGB values in the quality path until
  a specified mapping boundary. Bound alpha separately, reject nonfinite values,
  and define overflow behavior. LUT domains must cover supported values; a
  default 0–1 LUT can quietly defeat the extended-range pipeline.
- Specify color meaning for gradients, fills, paper, secondary brush colors,
  saved swatches/presets, and effect color parameters. The current
  [filter library](../../assets/filters/effects.wgsl) uses sRGB transfer functions,
  fixed luma coefficients, HSL operations, and bounded curve lookups; higher
  precision alone will not update those semantics.
- Version the runtime-filter ABI and declare supported working space/range and
  processing domain. Keep documented legacy behavior; validate a new filter path
  with both fused and unfused execution. Current quantization at physical pass
  boundaries makes pass partitioning relevant to exact results.

Prefer a limited set of supported linear RGB working spaces first. sRGB and
Display P3 are practical creation presets; Adobe RGB is useful for photo/print
interchange, while ProPhoto inputs need a defined high-precision handling policy.
Arbitrary input/output ICC support does not require allowing every device ICC
profile to become the painting space. Compare document-native primaries with a
canonical extended internal space in the prototype; unrestricted signed float
RGB can encode colors outside its nominal positive-primary triangle.

For general ICC handling, evaluate a shared engine outside the stroke path.
LittleCMS is an MIT-licensed baseline with v2/v4, proofing and BPC support;
moxcms is a pure Rust candidate whose upstream lists RGB/CMYK/gray/Lab transforms.
Benchmark representative profiles and validate required intents, proofing,
extended values, WASM behavior, memory, and GPU-LUT accuracy before selection.
[LittleCMS capabilities](https://www.littlecms.com/color-engine/),
[moxcms upstream](https://github.com/awxkee/moxcms).

**7. High priority: budget the actual retained representations and peak memory**

Handoff lines 140–162 correctly call out the composite but should add the current
image-import allocation chain. [prepare_owned_asset](../../crates/layer-render-wgpu/src/lib.rs)
retains CPU source bytes and an immutable full-size GPU source texture. On reset,
the renderer allocates paint pages covering the imported image and initializes
them from that source. The full document composite is another allocation.

For one 8192×8192 imported image filling an equally sized document, these four
RGBA8 representations alone account for **1 GiB** of raw allocated payload:
256 MiB CPU source + 256 MiB GPU source + 256 MiB paint pages + 256 MiB composite.
Promoting only paint and composite to FP16 makes the same subtotal **1.5 GiB**.
This is allocation arithmetic from the code, not measured physical residency;
it excludes decode temporaries, surfaces, filters, history and driver overhead.

[Image stages](../../crates/layer-render-wgpu/src/scene_images.rs) also retain
full-resolution inputs, outputs, masks, backdrops and scratch for relevant
filters. [Export](../../crates/layer-render-wgpu/src/lib.rs) creates a full-size
output texture and staging buffer; [readback completion](../../crates/layer-render-wgpu/src/export_readback.rs)
then allocates packed CPU output. A future atomic document conversion must budget
for keeping old and new resources live. Current renderer telemetry includes scene scratch,
but does not account for all source CPU/GPU storage or external export tickets.
It must not be presented as total process or total system memory.

Add these decisions to the document:

- Count persistent, temporary and peak allocations by purpose and owner; include
  decoded assets, export buffers, retained save snapshots and platform copies.
  `ProjectLimits::asset_bytes` currently defaults to 512 MiB. New pixel formats
  must update byte-stride/size logic and intentional load limits.
- Compare lazy source-backed image tiles and copy-on-write paint with eager
  materialization. This can target photo memory costs independently of replacing
  the whole compositor, but must preserve sampling, transforms and replay.
- Make a tiled composite and tiled/filter caches separate measured choices.
  Streaming export and incremental import/conversion need bounded queues and
  cancellation, not just a tiled display.
- If adding eviction, decide disk backing, compression, prefetch and replay
  checkpoints. Derived composite tiles can be discarded and rebuilt; paint
  state has different persistence/reconstruction costs, including wet media.
- Define zoomed-out resolution and invalidation. Mip pyramids add storage;
  global effects and filter halos still require dependencies outside the viewport.

Profile metadata and modest transform LUTs are unlikely to be the dominant
allocations: a raw 33³ RGBA16F LUT is about 0.274 MiB; a 65³ one about 2.095 MiB.
These calculated sizes are not accuracy recommendations. Share transforms across
views, bound the cache, and key it by profiles, intent/BPC, encoding, range,
view settings and implementation version as applicable.

**8. Medium priority: add acceptance gates and reorder delivery**

The handoff's benchmark guidance is sound. Add a color-quality corpus and
concrete workflow completion criteria, so a fast P3 swatch demo cannot stand in
for a color-managed editor.

| Gate | Required evidence |
| --- | --- |
| Profile correctness | Tagged/untagged sRGB, P3, Adobe RGB and ProPhoto fixtures; supported v2/v4 matrix and LUT profiles; D50/D65 neutrals; clear unsupported-profile results; exported metadata matching exported samples. |
| Editing quality | Shadow/alpha ramps, low-flow strokes, repeated adjustments, gradients and resampling; comparison to a higher-precision numerical reference with defined tolerances. |
| Appearance consistency | Picker, opaque canvas patches, thumbnails, navigator, proof preview and export agree under their declared transforms. HDR uses defined reference white, exposure, SDR preview and an appropriate comparison method. |
| Compatibility | Old projects, profile/precision conversion, undo/redo, reopen, and device loss retain their specified results, including custom WGSL and stateful brushes. |
| Memory | Measured steady and peak RAM/GPU allocations on sparse drawing and dense photo workloads, cold imports, filters, export, conversion and multiple open documents. |
| Responsiveness | Existing warm drawing baseline plus display transform/proof LUT, cold tiles, export contention, monitor changes and sustained mobile runs; CPU submit, GPU completion, cadence and latency measured separately. |

Suggested delivery order:

1. **Semantics and baseline:** version color/replay contracts, add profile/sample
   metadata, inventory and measure total allocations, establish the numerical and
   image corpus. Preserve legacy document behavior explicitly.
2. **Quality prototype:** profile-preserving import → FP16 editing → managed SDR
   presentation → high-bit-depth/profiled export. Compare encoded RGBA8 and FP16
   on the same shadow, brush and dense-photo cases; use results to set defaults.
   Include Linux/Wayland and at least one constrained mobile target early.
3. **Usable wide-gamut SDR:** ship supported creation spaces, profile assignment
   and conversion policy, wider color selection and precise numeric entry,
   swatches/previews, interoperability, and monitor-change handling. Preserve
   the compact picker design; exact color entry can be an additional control.
4. **Print workflow:** add proof configuration, rendering intent/BPC, output
   gamut warnings, paper simulation and a matching export route. Apply proofing
   in the view; avoid implementing it as an ordinary effect that may be exported
   accidentally. Test float-document proofing rather than assuming CMM gamut
   alarms work over arbitrary extended values.
5. **HDR editing and interchange:** define whether content is scene- or
   display-referred, reference-white mapping, changing display headroom, HDR
   tools/histograms and authored SDR output. Decode source gain maps into the
   editing representation and regenerate an appropriate gain map on export;
   do not reattach the original map after arbitrary painting/filtering.

Memory reductions can proceed alongside these stages, prioritized by measured
allocations. A universal cache rewrite is not a prerequisite for proving basic
color correctness. Conversely, a high-precision default for large documents
should not ship before its memory and interaction envelope is demonstrated.
