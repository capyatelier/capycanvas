# HDR export: compact output choices

Proposal for user review, 2026-09-18. This describes the next export expansion;
HDR JPEG and gain-map AVIF/HEIF export are **not implemented** in the current
Proof/tone-mapping review build.

## Main page

Keep one **Output** selector above the preview. Replace the separate Dynamic
range selector with these intent-level choices; do not add four permanent cards
or display all format-specific settings at once.

| Output choice | One-line description for the selected choice | Intended file |
| --- | --- | --- |
| **HDR JPEG** | HDR on supported devices; SDR on older viewers. | JPEG, SDR base + one gain map |
| **HDR with transparency** | Keeps transparency. Needs a compatible viewer. | AVIF with alpha and SDR base + one gain map; HEIF only after qualification |
| **HDR native** | HDR pixels for HDR-aware software. | Existing PQ PNG first; AVIF/HEIF can follow |
| **SDR** | Standard image for everyday use. | Existing PNG, JPEG, TIFF |

Use **HDR native**, rather than **Full HDR**: gain-map JPEG is also real HDR, and
“full” can incorrectly imply lossless/unbounded scene data. PQ PNG is already a
useful native HDR route with alpha; it is not a Float32/negative-value archive.
Native `.capy` remains the editable master.

On the main page show only:

1. Output selector and one short description for its current value.
2. The existing image preview. For gain-map output, an **HDR / SDR** preview
   toggle shows reconstructed HDR or the actual encoded SDR base. Retain master
   comparison within the preview. On an SDR screen label the HDR view's mapping.
3. Quality where the selected codec needs it. Do not expose gain-map resolution,
   metadata schemas, bit depth fixed by the format, codec speed or chroma sampling
   by default.
4. Compact navigation rows for **Size**, **Color & transparency**, **Preset**.
   Their summaries show current values. Keep the existing slide-in detail pages.
5. **Choose file** at the bottom.

Do not show a redundant Format selector for HDR JPEG. HDR with transparency has
one qualified initial format too; show its extension in the selected output row.
SDR retains its useful Format choice. Native HDR only needs a Format choice once
more than the existing PNG route is actually supported.

This follows the current export navigation architecture, rather than introducing
another modal wizard or a second export command.

## Defaults, transparency and the SDR base

- An SDR document keeps its existing SDR default. An HDR document with known
  opaque output recommends HDR JPEG; an HDR document with transparency recommends
  HDR with transparency **only once that route is qualified on this host**.
  Respect explicit choices/presets. Do not switch output while the user edits it.
- Determine coverage from the selected export scope, not merely whether the
  source has an alpha channel. Use cached/bounded asynchronous analysis; unknown
  coverage is not permission to flatten.
- Keep HDR JPEG discoverable when transparency exists. Selecting it shows one
  concise inline **Flatten transparency** requirement and a background swatch,
  initially white. Export remains unavailable until flattening is explicit.
  The same background setting is reachable in Color & transparency. Do not hide
  the option inside a disabled choice that gives no way to resolve the problem.
- Hide background/flatten controls for alpha-preserving output. Preserve alpha
  independently of gain calculations, including anti-aliased edges and zero alpha.
- For gain-map formats, show **SDR appearance → Proof** in the SDR preview context.
  The authored Proof rendition is the encoded base, so the fallback is predictable.
  Its sliders stay in Proof; do not duplicate them across export pages.
- For HDR native, omit SDR-base controls: it has no baked SDR fallback. Show one
  short compatibility sentence, not a warning block. The connected display does
  not determine file format, stored reference white or exported peak values.

## What the repository actually supports

`layer-ui::export` and the GTK export navigator already provide SDR PNG/JPEG/TIFF,
background choices and draft-preserving subpages. `photo/hdr_png.rs` supplies
16-bit PQ PNG delivery. Current JPEG gain-map input is explicitly rejected in
`photo/jpeg_markers.rs`; `photo/heif_io.rs` supplies SDR HEIF/AVIF decoding and
rejects HDR. There is no completed gain-map export path to expose today.

The new choices therefore require codec work and round-trip validation, not just
new menu labels. Until ready, the review build keeps its functional SDR/PQ PNG
choices; no clickable placeholders or disguised SDR exports.

## Codec direction and evidence

**JPEG:** Google's [libultrahdr API](https://github.com/google/libultrahdr)
accepts both HDR and authored SDR inputs. Its
[JPEG writer](https://github.com/google/libultrahdr/blob/main/lib/src/jpegr.cpp)
can emit ISO 21496-1 and the older XMP gain-map metadata for the same encoded gain
map. Build/runtime configuration must enable and verify both. “Maximum
compatibility” applies to the SDR JPEG base; HDR reconstruction still depends
on the receiving app and on services preserving the auxiliary data.

**Transparent gain-map HDR:** AVIF is a concrete first candidate, rather than an
unspecified HEIF-or-AVIF promise. [AVIF 1.2](https://aomediacodec.github.io/av1-avif/v1.2.0.html)
defines alpha auxiliary images and tone-map derived items; current
[libavif tests](https://github.com/AOMediaCodec/libavif/blob/main/tests/gtest/avifgainmaptest.cc)
include files containing color, alpha and gain maps together. This establishes a
format/library path, not proof that every browser or editor reconstructs HDR and
alpha correctly. HEIF/HEIC can follow when the encoder and target viewers are
qualified; exposing both initially adds little user value.

Use one shared Rust rendition/gain-map representation: working/input primaries,
reference white, base and alternate headroom, per-channel gain bounds/offsets,
gamma, image dimensions and the gain samples. Derive container metadata from it.
Generate the map from the **edited HDR** and **authored SDR** at the final export
size. JPEG's ISO and XMP blocks describe that same map; never generate competing
maps. For JPEG flattening, first composite in the appropriate linear-light
rendition domains with the same chosen background, then derive the opaque pair.
Do not apply gain to coverage or retain an imported map after arbitrary edits.

## Implementation and qualification

Extend the shared export recipe with an explicit output kind and capabilities;
keep Rust responsible for valid combinations and snapshot/rendition ownership.
Host UI presents only available complete routes. Each codec adapter uses bounded,
cancellable workers, budgeted staging, and atomic publication. Large-document
encoders may need additional admission limits; do not silently reduce precision.

Before enabling each choice, validate:

- Independent decoding of the SDR base, reconstructed HDR, gain-map metadata and
  alpha. ISO and XMP paths should agree within declared codec tolerances.
- Authored SDR changes appear in the file's fallback, and HDR survives the paired
  reconstruction. Resizing, final quality settings and metadata must stay aligned.
- Transparency on light/dark backgrounds, small nonzero alpha, signed/out-of-range
  HDR policy, orientation, cropping, and profile/reference-white interpretation.
- Open the export again in Capy and in independent viewers; qualify at least one
  gain-map-aware desktop/mobile target and legacy SDR JPEG behavior.
- Real browser tests by named version for HDR, SDR fallback and alpha. Current
  format support does not imply gain-map support. Test sharing services separately.
- 60 MP cancellation, memory, responsiveness, disk failure and existing-file
  preservation. A backend without the required encoder reports unsupported output.

Start with HDR JPEG plus reopening it, then transparent gain-map AVIF plus
reopening it. Existing native HDR PNG and SDR routes remain useful throughout.
