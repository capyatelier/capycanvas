# A durable foundation for the Capy Canvas file format

[Design history](README.md)

Research date: 2026-10-02, including the adversarial review in section 5. Code
baseline: `2e574cb20`; storage and effect findings rechecked at `9f8f5a919`.
This is a design recommendation, not an adopted format specification or an
implementation plan for the future features discussed below. Current-format
findings come from the reader, writer, model types and a checked-in file, rather
than earlier plans.
External sources include specifications, developer documentation, product manuals
and explicitly identified third-party investigations. Tradeoffs and
recommendations are our analysis of those sources.

**Recommendation:** keep `.capy` as one self-contained artwork file, using a
restricted ZIP container with ZIP64 support, a small JSON manifest, a flat table
of typed objects with globally unique IDs, binary resources and a fixed
portable-preview convention. A missing preview must not prevent saving otherwise
complete source data. Keep today's sparse, lossless tile storage and reuse of
compressed data, but check stored blocks with CRC-32 and defer decoding until
needed. Replace serialization of the runtime `Document` with an
explicit file model. Write every reference in one form that a reader can find
without understanding the referring type. Let composition types own their
coordinate and evaluation rules, and outputs own delivery intent. Signal
compatibility through the content itself rather than through capability lists.
Keep today's raster layouts in a versioned resource type, rather than building
an arbitrary channel schema now. Use one authored graph with layers and nodes
as views of the same representation, starting with structured stack operations.
The [implementation plan](../development/capy-format.md) defines the staged
file/runtime boundary and its correctness and performance gates. The byte-level
schema remains provisional until those gates pass.

The lasting commitment is **new readers preserve the meaning of old files**.
Old readers cannot edit arbitrary future features correctly. They should show a
saved representation, preserve the source, and avoid silently saving a damaged
document. No container or capability list can replace the ongoing work of
maintaining old semantics and testing them. The five feature stress tests in
section 3 refine the initial proposal. The adversarial review in section 5
simplifies its compatibility mechanism and closes gaps found by comparing more
artist tools. Its [boundary cases](#workflows-that-test-the-abstraction-boundaries)
also cover cross-layer mattes, editable fill regions, typography, texture sets
and drawings in 3D. Section 4 incorporates the accepted changes.

Read the [current-format audit](#1-current-format-and-problems-to-correct),
[external research](#2-what-other-formats-teach-us),
[future feature analysis](#3-future-features-and-their-implications),
[final recommendation](#4-minimal-recommended-foundation) and
[adversarial review](#5-adversarial-review).
Focused assessments cover [animation workflows](#animation-workflows-the-foundation-must-accommodate),
[five feature stress tests](#five-iterative-feature-stress-tests) and
[whether to adopt a general node graph now](#should-the-file-already-be-a-general-node-graph).

## 1. Current format and problems to correct

### What is actually on disk

The authoritative implementation is
[`project_storage.rs`](../../crates/layer-core/src/project_storage.rs), supported
by [`project.rs`](../../crates/layer-core/src/project.rs) and the
[source](../../crates/layer-core/src/project_storage/sources.rs),
[selection](../../crates/layer-core/src/project_storage/selections.rs) and
[photo-metadata](../../crates/layer-core/src/project_storage/photo_metadata.rs)
indices. The current format is already a custom indexed container:

```text
12 bytes   CAPYRASTER\x0e\0
 8 bytes   JSON byte length, little endian
32 bytes   SHA-256 of the JSON bytes
 N bytes   JSON manifest
remaining  compressed tile blobs, ICC profiles, photo metadata
```

Only version 14 is accepted. The manifest contains `document`, `tile_size`,
`rasters`, `blobs`, `tiled_sources`, `selections` and optional `metadata`.
Payload offsets are relative to the end of the manifest. The reader requires
contiguous indexed payloads, all expected raster targets, valid references and no
unused blobs or trailing data. There is no preview entry or capability inventory.

Tiles are fixed at 256 × 256 pixels. Each unique tile is a lossless LZ4 block;
multibyte samples are byte-shuffled before compression. A tile's SHA-256 covers
its serialized pixel descriptor and original decoded bytes. The writer
deduplicates tiles and streams already compressed backing. The reader loads the
payload sequentially and decodes each tile to validate it before adoption; the
presence of offsets does not make the current reader lazy. See
[`raster.rs`](../../crates/layer-core/src/raster.rs).

The maintained regression fixture
[`published-v15-choice.capy`](../../crates/layer-core/tests/fixtures/published-v15-choice.capy)
now uses the current container version; it retains the active editing state and
ID allocator checks described here. This inspection establishes the representation, not a performance
result or qualification of every host's save workflow.

| Area | Persisted today |
| --- | --- |
| Document | Identity, dimensions, color space/depth, blend space, print resolution, SDR rendition, proof recipe (stored inside the source index) and descriptive photo metadata. |
| Composition | Ordered flat layer array with parent references; paint, paper, group, effect and selection kinds; names, visibility, opacity, blend, clipping, paper color, offsets, placement with interpolation, extents and masks. |
| Masks | Enabled, linked, inverted, placement, extent, offset, an initial selection and a default coverage for tiles that were never painted. |
| Paint | Committed sparse color and coverage tiles; wetness planes and live watercolor edge settings. No saved stroke replay or undo history. |
| Imported images | Tiled source samples, independent interpretation/profile, resolution and Original/Rasterized role; painted tiles override source regions. These are retained decoded samples, not necessarily the original imported file bytes. |
| Effects | Embedded resolved WGSL, ABI, entry points, passes, lookups, sampling declarations, parameters, constraints, localized label IDs and values. Each instance serializes its own copy of the program. |
| Editing aids | Rulers, reference-layer flags, saved selections (contours or packed coverage), current selection, layer locks and alpha locks. |
| Runtime/editor details | Document revision, next layer/stroke IDs, active layer/mask, mask inspection flag, and selection display settings. |
| Not in the document | Custom brush textures, pending operations and stroke data. Palettes, swatches and tool memory belong to the workspace. |

These fields are visible in
[`Document` and `Layer`](../../crates/layer-core/src/lib.rs),
[`LayerProperties` and `LayerMask`](../../crates/layer-core/src/layers.rs),
[`effects.rs`](../../crates/layer-core/src/effects.rs) and the storage indices.
The code supports U8, U16, F16 and F32 samples; float paint is linear and straight,
and float-document coverage uses U16. Preserve that existing HDR support when
defining the replacement. The
[current format guide](../reference/project-format.md) still emphasizes integer
storage and describes some historical host layouts, so its wording alone is
not a sufficient inventory. Check the
[color descriptors](../../crates/layer-core/src/color.rs) and
[depth definitions](../../crates/layer-core/src/color/profile.rs).

### Specific problems

1. **The file schema is coupled to the live model.** `Manifest<D = Document>`
   serializes a cloned `Document` directly. Rust field names, enum variants and
   nested implementation structures become the file contract. A harmless model
   refactor can therefore change persistence. Use dedicated file types and
   explicit conversion at the shared Rust boundary.

2. **Compatibility is an exact-version gate.** The reader's `READABLE` list
   contains only the current magic. Top-level transport records use
   `deny_unknown_fields`; several nested model types do not, so unknown data can
   instead disappear during deserialization. Neither behavior is a capability
   policy. There is no opaque preservation mechanism or distinction between an
   unsupported feature and an invalid document.

3. **Editor state can invalidate otherwise intact artwork.** Validation rejects
   invalid `active_layer`, `active_mask`, allocator values and an exhausted
   revision counter. None is necessary to interpret the picture. Remove these
   from the artwork contract, derive allocation state after reading, and keep
   active targets, inspection state and the current selection in local session
   state keyed by the document ID. A later ancillary record can carry them
   between devices without making them a condition of opening artwork.

4. **The outer format assumes the current layer implementation.** Every layer
   and mask needs a raster target record, even kinds without raster content.
   Sources bind specifically to `LayerId`; selections have `Current`, `Layer`
   and `Mask` target variants; paper must occupy the last array position. Most
   relationships are array positions: z-order, the clipping base found by
   scanning down the array, tiles to blobs, sources to images, embedded profiles,
   selection pixels, effect values matched to parameters by position and Choice
   values stored as option indexes. These can be valid rules of today's
   layer-stack type, but should not be rules of the container or of every future
   document.

5. **Binary resources are special-purpose sections.** Tiles, profiles and photo
   metadata each have their own indexing machinery. Adding fonts, movie clips,
   meshes or stroke samples would otherwise require another special section.
   Use one resource addressing mechanism, with interpretation owned by each
   resource's type. Keep images, masks and their coverage semantics explicit.

6. **There is no portable saved appearance.** An unknown effect, future layer
   kind, unsupported codec or unavailable GPU feature can stop useful opening.
   Add a baseline preview that does not require reconstructing the artwork.
   Preserving opaque bytes and showing a preview solve different problems; both
   matter.

7. **Effect compatibility has another exact-version gate.**
   `EffectInstance::validate` requires `program.abi == EFFECT_ABI`, currently 3.
   Embedded source protects against catalog replacement, but does not preserve
   the shader's host interface, language behavior or color semantics forever.
   Parameter values are positional, and the program includes labels and rendering
   hints. Each instance serializes its own copy of the program, including linked
   WGSL of up to 1 MiB, and sharing is not restored on load. A parameter's unit
   is a display string: Image Size rescales a value only when its unit label is
   exactly `px`. Version the durable evaluation contract independently, use stable
   parameter identities and declared dimensions in files, and keep UI labels from
   determining whether or how an effect is evaluated. Replacing a built-in
   implementation must not silently change a saved picture.

8. **Storage conventions need a written, stable meaning.** The tile digest
   currently incorporates `serde_json::to_vec(descriptor)`: changing serialization
   spelling or omission rules can change identity without changing pixels.
   Fixed tiles and packed selection words are not inherently wrong, but belong
   to a named storage encoding, independent of GPU pages and in-memory packing.
   Hash inputs, endianness, padding, channel order and missing-tile behavior must
   be defined independently of Rust serialization details. The digest is the
   only check, and it covers decoded pixels: the reader decompresses, unshuffles,
   hashes and validates every tile before opening, and no checksum covers the
   stored bytes. That rules out verifying or copying a block without decoding it.

9. **Application limits and format limits are entangled.** Default admission
   limits include 64 MiB metadata, 512 MiB source ownership, 1 GiB raster data,
   16,384 tile instances, 32,768 pixels per axis and 4,096 layers. They are useful
   defenses, not a suitable permanent ceiling for animation and video. Distinguish
   structural validity from a particular device's editable working set. Do not
   remove resource limits to claim extensibility.

10. **Paint-tile color and composition geometry are canvas-global.** The reader
    requires paint/material tile descriptors to match the document's plane
    descriptors. Retained imported sources already have independent profiles and
    depths; ordinary paint layers do not. The roadmap's per-layer linear blending
    needs an override of today's document-wide blend policy. Root offsets carry
    the canvas origin, crop rewrites them, effects are evaluated over the canvas,
    and effect lengths are canvas pixels. Layer pixels are already independent of canvas
    pixels through placement. These are valid rules of today's layer stack, but
    they belong to that composition rather than to the document.
    See [`canvas_geometry.rs`](../../crates/layer-core/src/canvas_geometry.rs) and
    the [photo-editing roadmap](../development/photo-editing-roadmap.md).

### What should stay

Keep exact editable pixels, non-destructive source data, explicit profiles and
alpha interpretation, out-of-canvas extents, independent masks, transform
geometry, group/clipping semantics and embedded effect definitions. Keep
bounded validation, integrity checks, compressed-data reuse and immutable save
snapshots. Keep undo history, predicted pen input, pending operations, GPU handles,
device capabilities and workspace layout out of the artwork.

Do not delete wetness simply because it resembles a renderer buffer: it affects
later paint, while live watercolor edges affect current composition. Express it
as versioned paint-material state. Similarly, named selections, authored rulers,
reference designations, locks, proof intent and print resolution are useful
document information even when they do not directly contribute visible pixels.
Separate authored aids from temporary inspection. The current active selection
moves to session state; saved selection objects and mask coverage must remain
portable.

The existing
[atomic file writer](../../crates/layer-core/src/atomic_file.rs) and
[GTK save worker](../../apps/layer-linux/src/files.rs) are also worth preserving.
Changing packaging does not replace snapshot, cancellation or durable-publication
rules.

## 2. What other formats teach us

### Patterns in established and modern systems

| Format/system | Documented design | Lesson for Capy Canvas |
| --- | --- | --- |
| OpenRaster | ZIP with a layer manifest, separate images, a required thumbnail and merged PNG. ZIP64 is explicitly relevant to large files. [File layout](https://www.openraster.org/baseline/file-layout-spec.html). | A small inspectable package and rendered view are practical. Its baseline layered-image model is too narrow to make our permanent document model. Animation and other proposals listed by the project are not automatically an interoperable standard. [Specification index](https://www.openraster.org/). |
| Krita KRA | ZIP-based native document; `mergedimage.png` permits other software to consume the appearance. KRZ omits that image and always compresses, trading interchange for size. [Krita manual](https://docs.krita.org/en/general_concepts/file_formats/file_kra.html). | Preview policy has real size and save-time costs. A familiar container does not make application-specific editing data universally understood. |
| Photoshop PSD/PSB | Layer/channel records and extensible tagged blocks coexist with composite image data. PSB widens selected lengths and increases size limits. [Adobe specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/). | Length-delimited additions and a composite are useful. Avoid accumulating per-field width exceptions and a second large-document format; choose large offsets at the foundation. Merely skipping an unknown block does not establish safe editing. |
| Inkscape SVG | Standard SVG appearance accompanies namespaced editing data. Inkscape documents a case where regenerating a path from private parameters overwrites edits made to the visible path elsewhere. [Inkscape documentation](https://wiki.inkscape.org/wiki/Inkscape_SVG_vs._plain_SVG). | Preserve editable source and portable appearance, but make their authority explicit. Editing a fallback must never quietly masquerade as editing the original object. |
| glTF/GLB | A JSON description references binary data. `extensionsUsed` inventories extensions; `extensionsRequired` identifies those necessary to load/render correctly. [Khronos specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html). | Declare capabilities based on actual content, not the writer's app version. glTF is a useful delivery format for 3D resources, not a complete authoring format for this editor. Its renderability distinction does not by itself guarantee lossless editing. |
| Blender | SDNA describes stored structures; loading compares schemas and applies conversions. Blender also maintains explicit versioning code and compatibility policy. [DNA](https://developer.blender.org/docs/features/core/dna/), [compatibility policy](https://developer.blender.org/docs/handbook/guidelines/compatibility_handling_for_blend_files/). | Self-description helps parse data; semantic conversion still needs maintained code. Avoid tying Capy's durable representation to runtime structures or compiler layouts. Blender's system is evidence of the maintenance involved, not a small design to copy wholesale. |
| OpenUSD/USDZ | USD offers typed scene data and authored time samples; unknown schema types can have declared fallback types. USDZ packages resources in uncompressed ZIP entries to permit direct access. [Data types](https://openusd.org/release/api/_usd__page__datatypes.html), [fallback prim types](https://openusd.org/release/api/_usd__page__object_model.html), [USDZ specification](https://openusd.org/release/spec_usdz.html). | Separate scene meaning, resources and transport. Store already compressed media without another compression layer. Avoid importing USD's entire composition system or its packaging restrictions into a painting format. |
| Aseprite | Length-delimited chunks describe frames, layers, image cels, linked cels, palettes and tilemaps; frame duration is stored in integer milliseconds. [Aseprite specification](https://github.com/aseprite/aseprite/blob/main/docs/ase-file-specs.md). | Distinguish shared cel content from where and when it appears. Integer milliseconds cannot exactly represent every film/video frame duration, so do not make them our master timeline unit. |
| OpenTimelineIO | Types carry schema names and versions; registered conversions upgrade old data. Timelines reference media and express times/ranges with rate information. [Schema versioning](https://opentimelineio.readthedocs.io/en/latest/tutorials/versioning-schemas.html), [file format](https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-file-format-specification.html). | Version individual meanings and retain conversion tests. Learn from the separation of timeline and media; a timeline interchange model does not define our painting, compositing or procedural evaluator. |
| PNG | Distinguishes critical/ancillary chunks and whether unknown ancillary chunks remain safe to copy after image changes. [PNG specification, editor behavior](https://www.w3.org/TR/png-3/#14EditorsExt). | Rendering without understanding data and modifying a file without corrupting its meaning are separate capabilities. Unknown content that depends on changed artwork cannot simply be copied and assumed valid. |
| Affinity Photo | A native file preserves project information; saving undo history is optional and can substantially increase size. [Affinity Photo 2 manual](https://affinity.help/photo2/en-US.lproj/pages/GetStarted/save.html). | A unified editing experience does not require persisted session history. The cited manual documents behavior, not binary internals; this report does not infer an undocumented Affinity container design. |

Two further examples matter for future scope. MaterialX separates node
definitions, graph implementations and target-specific implementations, showing
why a semantic filter interface is more durable than compiled GPU artifacts.
[MaterialX specification](https://github.com/AcademySoftwareFoundation/MaterialX/blob/main/documents/Specification/MaterialX.Specification.md).
VRM layers avatar meaning onto glTF, including humanoid and expression data,
with related extensions for secondary motion.
[VRM specification](https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm-1.0/README.md).
Live2D distinguishes editable `.cmo3` models from exported runtime `.moc3` data.
[Live2D file types](https://docs.live2d.com/en/cubism-editor-manual/file-type-and-extension/).
Embedding a delivery asset is useful, but is not equivalent to preserving the
authoring project that produced it.

### Failures to design against

The common pitfalls are design tradeoffs, not claims that every program above
has the same bugs:

- **A version number without semantic stewardship.** Parsing old fields does
  not preserve blend formulas, font shaping, brush algorithms or shader inputs.
- **Two sources of truth.** A stale raster fallback and edited source can disagree.
  State which is authoritative, what the fallback represents and when it expires.
- **Silent degradation on save.** An editor may display a believable flattened
  image while discarding vectors, animation, rigging or unknown extension data.
- **Unbounded packaging.** One file per dab or tiny tile can produce millions of
  directory entries. One compressed stream for the whole project prevents useful
  random access. Neither extreme is necessary.
- **Accidental environmental dependencies.** Installed fonts, filter catalogs,
  linked files, network services and local absolute paths can change an image
  without the project changing.
- **Treating an archive as a transaction system.** ZIP central-directory damage,
  interrupted replacement and storage-provider failures still need handling.
  Appending new entries is not automatically a safe incremental-save protocol.

Relevant boundary cases include:

| Case | Required distinction |
| --- | --- |
| Unknown adjustment, blend mode or pass-through group | Omitting it can change everything below it. Use a valid composite fallback of the affected scope, not a transparent placeholder. |
| Unknown hidden object | It may contain editable work or become visible later. Visibility is not permission to discard it. |
| Missing font, shader ABI, ICC support or video decoder | The package may be valid even when the reader cannot evaluate it. Report unsupported content separately from corruption. |
| Corrupt payload, duplicate IDs, dangling references or cyclic hierarchy | Reject invalid native content; offer a separately verified preview or explicit recovery, never silently manufacture a successful load. |
| Cropped canvas, negative placement or sparse empty area | The canvas is the composition's frame: offsets, effects and paper are defined relative to it, but it does not bound the stored content. Outputs frame a composition without redefining it. Absence must have type-defined meaning. |
| Huge or adversarial file | Check arithmetic, decoded budgets, nesting, counts and codec output sizes. Deduplicated bytes can still describe an enormous decoded working set. |
| Reordered or deleted objects with unknown metadata | Stable IDs help, but do not prove that unknown relationships remain valid after edits. |
| Damaged thumbnail only | Artwork validity is independent; a supported reader can regenerate a preview from intact content. |

## 3. Future features and their implications

These are possibilities to keep open, not a proposed feature schedule. The file
should acquire a feature's detailed schema only when that feature is designed.

| Feature area | Artwork that eventually needs preserving | Foundation needed now |
| --- | --- | --- |
| Raster/photo editing | Exact pixels, retained originals, masks, live adjustments, HDR, proof intent; later RAW development and spot channels. | Typed binary resources, explicit color/alpha semantics, authored parameters and immutable source identity. A future RAW resource must retain original bytes if redevelopment is promised. |
| Vector illustration and typography | Paths, reusable shapes, text, styles, gradients, boolean operations, variable-width strokes, layout constraints. | Stable object IDs, typed content and reusable resources. Fonts/shaping versions and outlined or raster appearances need explicit handling when text arrives. |
| Editable brush strokes | Authoritative curves or samples, pressure, brush definition/assets, seed and evaluation version. | A new content type can coexist with today's raster type. Do not turn all raster paint into a saved input-event log. |
| Animation and motion graphics | Cels/exposures, animated properties, tracks, clips, transitions, easing, audio, markers and nested compositions. | Content identity independent of layer position and time; reusable resources; compositions that can reference and contain each other. |
| Video compositing and rotoscoping | Encoded media, stream selection, time mapping, tracking/masks, color interpretation and proxies. | Seekable large binary resources and explicit dependencies. A proxy cannot silently become the master. |
| 3D, texture painting and mixed 2D/3D | Geometry, UVs, cameras, lights, materials, scene relationships, animation and source textures. | Objects and resources not restricted to RGBA layers; local coordinates with explicit units/conventions per type. GLB/USD can be resources without becoming the outer document. |
| Node-based composition | Typed ports, connections, evaluation rules, reusable subgraphs and designated outputs. | A composition is a typed object. The package does not assume every edge is layer parenting. |
| Custom shader filters | Source modules, parameters, textures, read/write color spaces, sampling and evaluation contract. | Versioned executable-content capabilities and saved appearance. Compiled pipelines stay disposable. |
| Procedural or generative content | Generator definition/version, parameters, inputs, seeds, model identity where relevant, and accepted result. | Source/result authority and embedded dependencies. A seed or remote model name alone does not guarantee reproducibility. |
| Publishing and document layouts | Pages/artboards, physical units, text flow, fonts, linked images, masters, bleed, trim and output intents. | Content and composition separated; later a page collection can reference compositions. No global assumption of exactly one forever canvas. |
| VTuber and puppet rigging | Meshes/deformers, parameter bindings, expressions, constraints, physics and motions. | Stable IDs below the layer level, typed relationships and animation references. Save authoring data separately from runtime exports. |
| Comics, storyboards, sprites and tilemaps | Page/shot order, reusable cels/symbols, palettes, tilesets, export regions and timing. | Shared content and ordered compositions; storage deduplication must not imply editing linkage. |
| Collaboration and variants | Authored alternatives, comments, provenance and shared resources. | Stable identity helps. Do not burden the first file format with a CRDT, operation log, tombstones or network protocol. |

### Animation is the most consequential extension

**Separate an object, its content and its occurrences.** A paint object can refer
to a raster resource today. Later, an exposure track can choose which raster
content that object displays, and a property track can animate its transform or
opacity. Holding one cel for twelve frames should reference one content object,
not copy twelve images. Two equal images may nevertheless be independently
editable: byte deduplication is a storage optimization; linked cels are an
authored relationship.

**Keep time out of pixels and object identity.** Do not bake frame numbers into
layer IDs, resource addresses or tile keys. Add timeline tracks and content
references later. A still composition remains a still composition; it should
not need empty tracks or a fake one-frame animation now.

**Choose exact time when animation is introduced.** Use integer ticks with an
explicit rational timebase, or an equivalent exact rational representation;
separate playback frame rate from timestamp representation. Define interval
endpoints, subframes, source offsets, retiming and rounding. Account for rates
such as 24000/1001, variable-frame-rate video and audio sample clocks. A float
`seconds` value or integer milliseconds is not a sufficient universal time
contract. This is a recommendation for Capy, not a claim that OTIO's numerical
representation itself guarantees exact rational arithmetic.

**Evaluation must work at an arbitrary requested time.** A procedural frame at
time T must not depend accidentally on which frames the user previously played.
Simulation features need declared initial state, solver/version and appropriate
checkpoints or baked output. Current
[`EffectClock`](../../crates/layer-core/src/effects.rs) integrates elapsed playback
phase and is not serialized; that is live playback state, not a future authored
timeline. Preserve explicit effect parameters today, and introduce durable time
mapping when authoring animation becomes a feature.

**Budget for time, not only space.** Load the requested interval and nearby
frames, sharing unchanged resources. Long recordings require indexes and bounded
decoding, not one giant array or a complete-project decode before showing frame
one. Tile packs and generic binary resources allow that evolution, but the
initial still-image reader need not implement timeline streaming.

**A poster is not an animation fallback.** A poster can identify a project when
the timeline is unsupported. A future playback fallback needs a stated range,
rate, dimensions, audio policy and color interpretation; it may be a proxy movie
or cached frames. It must never be reported as an editable timeline, and a single
frame must never stand in silently for an entire export.

### Animation workflows the foundation must accommodate

Photoshop distinguishes frame animation and timeline animation. Clip Studio Paint
separates cel creation from cel assignment, supports holds/repeated assignments
and gaps, and has distinct camera, movie and audio tracks.
[Photoshop animation overview](https://helpx.adobe.com/uk/photoshop/using/video-animation-overview.html),
[cel assignment](https://help.clip-studio.com/en-us/manual_en/600_animation/Assigning_cels_to_the_timeline.htm),
[track operations](https://help.clip-studio.com/en-us/manual_en/600_animation/Track_operations.htm).
These are different authoring flows over related content, not one interchangeable
list of rendered frames. The following is the proposed Capy design story; it
does not claim these schemas or features exist today.

| Artist's workflow | Future representation and editing behavior | Failure the design must avoid |
| --- | --- | --- |
| Hand-drawn animation on ones, twos or irregular holds | A cel is identified content, possibly a composition of line/color/shadow layers. An exposure track references it over explicit intervals; blank intervals are distinct from holds. Editing a held cel updates its intentional uses; duplicating independently creates new content identity. | Copying whole documents per frame; using layer names as cel identity; interpreting a gap as an implicit hold forever. |
| Sprite loops and replacement animation | Clips reference reusable cel sequences with explicit durations and repeat behavior. Palette, tilemap or mouth-shape substitutions use discrete bindings. Delivery loop metadata belongs to the output's authored playback intent. | Equating repeated byte content with linked editing; confusing the editor's temporary loop-preview range with the saved animation. |
| Keyframed layers, cameras, text and FX | Property tracks target stable instance/application IDs and semantic keys; authored curves define interpolation, units and pre/post behavior. Camera motion changes a view over content, not the source pixels. | Keying array positions; treating every value as a linearly interpolated float; losing animations when a layer or effect is renamed. |
| Vector morphing and animated brush drawing | A geometry type defines correspondence for morphs and topology changes. Stroke reveal uses authored progress/timing over source geometry; brush/input timestamps remain separate unless explicitly mapped to the timeline. | Assuming identical point counts imply valid correspondence; confusing recorded pen time with movie time; rebuilding final paint from unspecified brush behavior. |
| Cutout animation, lip-sync and VTuber performance | Reusable rigs and content have instance-specific pose/expression bindings. Replacement cels can coexist with continuous deformation. Recorded control/audio streams become typed authored resources when the artist keeps a take. | Saving a webcam or microphone connection as reproducible artwork; losing the take when live tracking is absent; applying one occurrence's pose to all instances. |
| Nested shots, reusable motion and multiple takes | A clip occurrence references a composition/action and maps parent time to its local time; trim, slip, repeat, reverse and speed changes are explicit. Alternatives remain identified authored content. | Destructively retiming shared keys for one occurrence; conflating a trim with deletion of source content. |
| Video compositing and rotoscoping | Retain encoded media, selected streams, presentation timestamps and source intervals; animate masks/tracking in a defined space. Decoding uses indexed access and bounded pre-roll to a requested interval. | Converting variable-rate timestamps to guessed frame indexes; decoding the entire movie on open; storing each video frame as paint tiles. |
| Audio, dialogue and music-driven motion | Audio clips retain source sample rate/layout and timing, offsets, trims and authored gain/mute. Phoneme markers or analysis results have explicit time references; waveforms are derived caches. | Reinterpreting sample positions as document frames; silently changing duration when export FPS changes; calling a silent proxy a complete audiovisual fallback. |
| Procedural motion and custom shader animation | Time is an explicit evaluation input, mapped per occurrence. Curves/expressions have stable targets and evaluator versions; randomness and phase have declared scope. | Depending on wall-clock time, playback order or mutable global uniforms; overwriting a shared object's time when rendering a second instance. |
| Physics, particles, cloth and secondary rig motion | A feature defines initial conditions, solver/version, time steps and checkpoint/bake resources. Seeking can replay from a validated checkpoint; nondeterministic accepted results need retained output. | Promising stateless arbitrary-time evaluation for stateful simulation; requiring playback from frame zero for every seek; sharing mutable simulation state between occurrences. |
| Timelapse and drawing-process films | Explicitly retained recorded frames, stroke-performance data or an authored reconstruction track form a separate media/animation resource. A finished raster painting remains a valid still without them. | Treating the undo stack or every editing gesture as the mandatory document history; promising to reconstruct deleted input from saved pixels. |

Reusable motion and retiming have established counterparts in Blender's action
strips; its simulation documentation separately describes cached and baked state.
Those are useful distinctions, not a reason to adopt Blender's entire animation
model. [NLA strip timing](https://docs.blender.org/manual/en/dev/editors/nla/sidebar.html),
[simulation caching analysis](https://developer.blender.org/docs/features/nodes/proposals/caching/).
Media timestamp and seeking semantics belong to media resources as well as to
the editorial timeline; Matroska documents timestamp scales and cue indexes.
[Timestamp notes](https://www.matroska.org/technical/notes.html),
[cue indexes](https://www.matroska.org/technical/cues.html).

#### Shared rules across those flows

**Separate source time, composition time and output sampling.** A cel sequence
may be authored at 24 fps, a video have its own timestamps, and audio run at
48 kHz, while an export samples the composition at 30 fps. Changing export rate
should resample the same duration; an intentional speed change edits time mapping.
For example, a two-frame hold at 24 fps lasts exactly 1/12 second. Sampling it at
30 fps does not redefine that duration as two 30-fps frames. A future UI may offer
either preserve-duration or preserve-frame-count changes, but the saved result
must state the resulting timing unambiguously.

Use half-open intervals or another explicitly specified endpoint convention.
Define blank intervals, hold behavior, negative time/pre-roll, zero-duration
markers and end-of-clip behavior. A timeline may show frame 1 as its first label
without making its mathematical origin one frame after zero. Drop-frame timecode
is a display/counting convention, not an instruction to drop media samples.
Do not force every source into one integer frame grid or accumulate rounded
floating-point frame durations.

**Separate base values from animation bindings.** A static layer's opacity is
an authored base value. A track can override or combine with that value under a
defined binding rule. When multiple clips, expressions or rig controls influence
one property, their composition order and mode must be specified; map iteration
order cannot decide the result. Transform, angle, quaternion, color, discrete
choice and geometry interpolation need their own semantics. This does not require
implementing every interpolation type or a universal value engine now.

**Evaluate in an occurrence-specific context.** Two references to the same
character may require different local times, poses, cameras and effect bindings.
Runtime caches must distinguish those contexts and evaluator/resource versions;
the file must retain the authored inputs needed to reconstruct them. It should
not serialize cache keys, evaluated GPU buffers or a mutable singleton current
frame. For temporal effects and motion blur, the feature declares its required
time interval and sampling behavior, not only a spatial damage radius. Reversed
time for video, audio and stateful simulation needs separately defined behavior.

**Give animated compositions a time base and keep clips apart from their targets.**
After Effects stores frame rate, duration, start time and shutter on each
composition and decides per composition whether nested frame rates are preserved;
Clip Studio Paint creates each timeline with its own rate and length.
[CompItem](https://ae-scripting.docsforadobe.dev/item/compitem/),
[CSP timelines](https://help.clip-studio.com/en-us/manual_en/600_animation/Timeline_Palette.htm).
Spine, Rive, Live2D and Moho keep many named animations beside one rig. A clip
therefore targets properties through paths relative to the content it is applied
to, so one clip can drive several occurrences. Reuse across different rigs needs
compatible declared interfaces or an explicit retargeting map; relative paths
alone do not provide that mapping. Some animation is keyed
on a parameter rather than on time: Live2D keyforms, Moho smart-bone actions and
Rive joysticks. Curves are functions of an evaluation-context input, of which
time is one. [Spine skins](https://esotericsoftware.com/spine-skins),
[Live2D parameters](https://docs.live2d.com/en/cubism-editor-manual/parameter/),
[Rive joysticks](https://rive.app/docs/editor/manipulating-shapes/joysticks).

**Interactive output depends on history, not only on time.** Rive state machines
and listeners, dotLottie state machines and Spine's animation mixing produce
frames from the inputs received so far. An interactive output declares an initial
state and its inputs, and its preview shows that initial state. The arbitrary-time
rule above applies to time-sampled outputs.
[Rive states](https://rive.app/docs/editor/state-machine/states),
[dotLottie](https://dotlottie.io/spec/),
[Spine mixing](https://esotericsoftware.com/spine-applying-animations).

**Retain authored work that is not currently played.** Unassigned cels, unused
takes, disabled tracks, off-range keys and alternate poses are still artwork.
Their object-table entries retain them even when no current output or root
references them. Deleting an exposure is not deleting its cel. Remove authored
objects only through an explicit deletion operation; collect payloads only when
no retained object, output or preserved ancillary record needs them. This
distinguishes retention from render reachability without requiring a new
container section for every kind of unused work.

**Separate persistence from rendering policy.** Onion skins, the playhead,
temporary solo controls, the light table and scrubbing caches are editor state.
Authored visibility, sound muting and output ranges affect the work and persist.
Real-time preview may skip frames according to product policy; final export must
evaluate every requested output sample. Saving should capture one consistent
authored revision without rendering a full film. Even a small poster can require
an expensive simulation or full-resolution effect; its pixel dimensions do not
bound the work needed to produce it. The [save contract](#clean-degradation-without-silent-loss)
permits a source save with no available preview. Expensive playback proxies are
optional, versioned by their source snapshot and range, and must not block
ordinary source saves indefinitely.

#### Animation acceptance cases for the eventual features

These cases are requirements to exercise when implementing animation, not test
results for the current editor:

| Case | Expected result |
| --- | --- |
| Edit a cel held on several intervals; make one occurrence independent | Intentional uses update; the independent copy does not. An unassigned original survives saving. |
| Reorder layers/effects and rename cels after keying them | All bindings still target the same identities and properties. |
| Mix 24-fps cels, 24000/1001 video and 48-kHz audio; export at another rate | Timing follows exact source/composition mappings with defined sampling; no cumulative drift from rounded frame durations. |
| Render a frame after seeking backwards, immediately after open and after continuous playback | Stateless results agree; stateful results follow declared checkpoint/replay or baked-output rules. |
| Put the same nested clip in two places with different offsets, masks and speeds | Neither occurrence overwrites the other's time, local properties or caches. |
| Change a cel at one time used by blur, a repeated clip and a simulation | Dependency rules invalidate all affected temporal/spatial results; stale cached output is never presented as current. |
| Open without a codec, rig solver or animation type | Valid posters/proxies remain accessible; their frame/range/audio limits are explicit; native source is preserved. |
| Save during playback and immediately quit/reopen | The committed authored work survives, including off-range data; UI playhead state does not determine the saved artwork. |
| Open a long film and jump near its end | Metadata and needed resources load within bounded budgets; no mandatory full-film decode or complete proxy render. |

The required foundation is content/occurrence identity, stable property keys,
typed relationships, binary resources and output contexts. The timeline later
supplies timing/binding semantics through its own types. Adding an empty timeline
or universal `frame` field to today's files would provide little of that support.

### Vector and stroke content should retain intent

A vector type can store authored geometry and resources without forcing the
outer format to be SVG. SVG is useful for interchange or fallback where its
supported subset matches the object. Text also needs font identity, embedding
policy, fallback behavior and shaping/layout semantics; substituting a font can
change the artwork even when the text remains readable.

Stroke content needs an explicit authority rule. Input samples plus a brush
definition may be authoritative initially; after curve editing, the edited curve
may be authoritative instead. Do not silently replay the old samples over the
edited result. Pin the brush evaluator, assets and randomness, and retain a
rendered representation for unsupported evaluators. Full replay equivalence
across changing GPU implementations is a separate, difficult promise.

The current raster-only paint representation is therefore a good baseline.
Stroke editability can be added as a distinct type, with rasterization producing
an explicit conversion rather than a second competing source of truth.

### Code, procedural content and external dependencies

Embedded shader source is authored data, not an instruction to run arbitrary
application code during opening. A future evaluator needs bounded resource use,
a defined interface and a policy for untrusted programs; unavailable or disabled
execution should still permit preview viewing. Do not embed native plugins,
driver binaries or credentials as artwork dependencies.

For current filters, retain exact source and parameters, but pin the shader ABI,
language subset and helper/color semantics. Store repeated definitions once and
reference them from instances. Separate stable parameter keys from localized
labels; optional editor metadata may describe controls without governing pixel
meaning. Deterministic generators require more than a seed: algorithms, inputs
and dependency versions matter. For nondeterministic or remote generation,
preserve the accepted result independently of whether regeneration is possible.

Default to embedded, self-contained resources. When linked-media workflows are
introduced, define portable references, expected content identity, missing-media
behavior and relinking explicitly. Do not fetch URLs or resolve arbitrary local
paths merely because an opened document names them.

### Five iterative feature stress tests

These are paper design tests, not implemented features or measured results.
The cited manuals establish the reference products' behavior; the Capy journeys
and model sketches are hypothetical. Each test starts from the preceding
candidate, tries to break it, makes only a general correction, and checks the
earlier cases again. Passing means preserving artwork meaning, editable identity
and safe degradation without redefining the container or old types. It does not
mean the feature can be implemented without new engine code or new schemas.

The initial candidate has a typed composition root, identified artwork objects,
resources, global capability inventories and one default preview. That is a good
transport foundation, but leaves several important semantic boundaries vague.

#### Test 1: Photoshop-style live effects on a layer

**Reference behavior.** Photoshop offers drop shadows, outlines and other layer
styles, including a shared Global Light angle. Fill opacity can hide the layer's
content while its effects remain visible.
[Layer style options](https://helpx.adobe.com/uk/photoshop/desktop/create-manage-layers/apply-layer-effects/layer-style-effects-and-options-overview.html),
[fill versus layer opacity](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/apply-layer-effects/set-layer-opacity-and-blending-modes.html).

**Hypothetical journey.** Add an outline and two shadows to a painted title.
Hide the fill, mask part of the title, resize it, change shared lighting and move
it into a pass-through group. Save, reopen, and edit each shadow independently.
The same feature should later apply to text, vector artwork and rendered 3D.

**Attempted extension.** Add a style object and reference it from the layer.
The package accepts it, but this alone does not determine the picture. Which
alpha drives the outline when fill is zero? Does a mask clip the shadow or only
its source? Does resizing scale the blur? Which backdrop does the style blend
against? Is the light copied into each style or referenced as shared state?

**Valid issue.** A generic effect blob, or simply reusing today's adjustment layer,
is insufficient. Today's adjustment operates on the stack below it; layer FX
may operate on an object's pre-fill coverage and emit pixels beyond its bounds.
A single undifferentiated `effects` list would hide materially different rules.
This is a missing evaluation contract, not a reason to discard typed objects.

**Revision 1.** Effects are identified *applications* of a definition to explicit
inputs. The owning composition/type defines evaluation order, coordinate space,
coverage, masks and backdrop scope. Future layer FX can use a versioned wrapper
or required extension at that composition boundary. Effect applications need
identities separate from reusable definitions so two shadows can be edited and
later animated independently. Shared lighting is an ordinary referenced authored
value. Expanded output bounds belong to evaluation, not to the source extent.

Do not add shadow, outline, fill-opacity or Photoshop-specific fields to the
envelope. Do not change `layer-stack/1` semantics silently: introduce the FX
capability and its composition rules. Existing simple files retain the original
stack contract. A wrapper is appropriate only where its input/output boundary
can express those rules; backdrop-sensitive behavior needs the composition's
explicit support, not an arbitrary post-processing attachment.

**Recheck.** Raster pixels remain authoritative for the title; effects are live
authored operations. An older reader shows the saved composition and cannot
overwrite it with an unstyled title. A backdrop-dependent style requires a
composition fallback, not an isolated shadow PNG. No container change is needed.

#### Test 2: Smart Objects, shared instances and masked Smart Filters

**Reference behavior.** Photoshop Smart Objects retain source content, support
non-destructive transformations and can link external files. Smart Filters have
editable ordering and masking.
[Smart Objects](https://helpx.adobe.com/uk/photoshop/desktop/create-manage-layers/smart-objects/smart-objects-overview-and-benefits.html),
[Smart Filters](https://helpx.adobe.com/photoshop/using/applying-smart-filters.html).

**Hypothetical journey.** Place the same editable logo twice. Warp one placement,
blur and mask the other, then edit the source logo so both update. Make a third
independent copy that initially looks identical. Reopen while an external source
is missing. Finally try to place the document inside its own source.

**Attempted extension.** Let three layers reference the same raster blobs. This
deduplicates storage, but does not say whether an edit updates all three. Putting
the transforms and filters on the source incorrectly applies them to every
placement. Treating each embedded project as a recursive `.capy` ZIP also creates
avoidable nested readers, ambiguous identity and repeated resource storage.

**Valid issue.** The earlier object's identity was too vague about *definition*
versus *occurrence*. Immutable byte sharing does not express live editable
sharing. A layer's parent is also not the ownership rule for all referenced data.

**Revision 2.** A placed instance references an identified content object or
composition. The instance owns its placement and local effect applications;
the content owns its editable source. Intentional sharing uses content identity.
An independent copy gets new authored identities even when its bytes remain
deduplicated. Today's source-plus-raster-overrides representation fits this
separation without implementing Smart Objects now.

Nested Capy content can reference objects in the same object/resource address
space. Imported foreign source files can remain opaque resources. Linking an
external document is a separate feature with explicit resolution and update
rules. A cached source snapshot must be identified as that snapshot; finding a
different file at the same path must not silently replace it. Persist neither
host reference counts nor allocator ownership as document relationships.

**Recheck, including FX.** Each logo occurrence can have its own shadow and filter
applications, while a shared style definition can still be reused deliberately.
Source edits invalidate dependent appearances, including both placements. Deleting
one occurrence does not delete content used by the others. A self-containing
composition is invalid unless a future type supplies explicit bounded recursive
semantics; ordinary references alone do not imply those semantics. Unknown source
types still lead to preserved, read-only fallback viewing.

#### Test 3: Clip Studio-style cel animation with camera and audio

**Reference behavior.** Clip Studio Paint assigns cels to animation tracks and
supports keyframed layer/camera properties and audio volume.
[Animation folders and cels](https://help.clip-studio.com/en-us/manual_en/600_animation/Animation_folders_and_cels.htm),
[keyframes](https://help.clip-studio.com/en-us/manual_en/600_animation/Using_keyframes.htm).

**Hypothetical journey.** Hold one drawing for twelve frames, reuse it later,
pan a camera over a background, and animate a title's shadow distance. Reuse the
same animated character twice at different time offsets. Reorder layers, rename
the shadow, change the delivery frame rate, and scrub backwards while audio stays
aligned. Save and seek directly to the middle after reopening.

**Attempted extension.** Replace the layer-stack root with a timeline root and
add keys addressed by paths such as `layers[3].effects[0].distance`. This breaks
when either list is reordered. An exclusive choice of timeline *or* layer stack
also fails to express a timed object inside a stack or a stack inside a clip.

**Valid issue.** Generic object IDs are insufficient if animated targets are
anonymous properties or positional array entries. Animation is also an evaluation
dimension that composes with structure, not a replacement for all structure.

**Revision 3.** Keep stacks, timelines and future graphs composable through typed
references. A time mapping belongs to the relevant occurrence, allowing the same
content to be evaluated at two times. Targets use stable object/application IDs
and versioned semantic property keys, not display names, serialized field paths
or list indexes. Independently addressable subelements receive identities when
their feature is introduced; ordinary vertices and samples do not all need IDs.
Define units and property meaning within the feature's schema. Exact time and
deterministic seeking follow the animation requirements above.

**Recheck, including shared content and FX.** Held cels reuse content; independent
copies remain independent. A shadow can be animated without changing its sibling
shadow or definition. Reordering preserves targets. Instance-specific time avoids
mutating a shared source to express an offset. A frozen thumbnail is explicitly
one moment, while native playback and export require timeline capability.
No timeline fields or key arrays are added to every still image now.

#### Test 4: Clip Studio-style posed 3D figures inside a 2D scene

**Reference behavior.** Clip Studio Paint supports posable 3D figures, body-shape
adjustment, camera manipulation and lighting controls.
[3D tools](https://help.clip-studio.com/en-us/manual_en/660_3d/3D_Tools.htm),
[posing figures](https://help.clip-studio.com/en-us/manual_en/660_3d/Posing_3D_drawing_figures_and_3D_character_materials.htm),
[editing 3D materials](https://help.clip-studio.com/en-us/manual_en/660_3d/Editing_a_3D_material.htm).

**Hypothetical journey.** Place two instances of a character with different poses,
put a prop in one hand, share its material, and frame the scene through a camera.
Apply a 2D outline to the rendered scene. Extend this later with keyframed pose
and a simulated hair rig. Reopen where the mesh is supported but the rig solver
or material evaluator is unavailable.

**Attempted extension.** Store a GLB resource and a `scene-3d` object. The revised
content/instance model handles mesh sharing and different poses. It does not,
by itself, specify rig evaluation, material behavior, camera conventions or
simulation state. Declaring only `scene-3d` supported would be a false positive.
Rejecting cycles in *all* references would also reject harmless metadata/back
references or solver relationships alongside actual illegal evaluation cycles.

**Assessment.** No additional universal object category is needed. The valid
remaining gaps are capability closure and relationship-specific validation.

**Revision 4.** Required capabilities include dependencies actually used by
authored content: a supported scene container does not imply a supported rig,
material, codec or nested effect. Feature schemas distinguish containment,
instancing, evaluation inputs and other references, and define cycle rules for
each. Immediate render dependencies must be well-founded; feedback/simulation
needs an explicit solver/time contract rather than an accidental reference loop.
The baseline does not acquire a generic solver, universal DAG restriction or a
set of speculative rigging fields.

**Recheck, including animation and FX.** Pose state belongs to each instance;
geometry/resources may be shared. Stable application/property identities support
animated poses later. The 2D outline sees a declared scene output and alpha,
without needing knowledge of bones. Its fallback depends on camera, lights, pose,
materials and relevant simulation state. With an unsupported solver, the whole
document remains safely viewable from its saved representation. A cached image
does not claim to preserve rig editability. This case confirms the revised
abstractions rather than justifying a larger universal scene format.

#### Test 5: Multi-page comics and artboards sharing editable artwork

**Reference behavior.** Clip Studio Paint manages multiple page files through a
management file, and its Story Editor works with text across pages. Photoshop
can export multiple artboards.
[Management/page files](https://help.clip-studio.com/en-us/manual_en/570_pages/Management_Files_and_Page_Files.htm),
[Story Editor](https://help.clip-studio.com/en-us/manual_en/570_pages/Use_Story_Editor.htm),
[artboard PDF export](https://helpx.adobe.com/photoshop/desktop/save-and-export/export-files-to-different-formats/export-artboards-as-pdf.html).

**Hypothetical journey.** Build a comic with several pages and a cover. Reuse a
vector emblem, show a posed character in one panel and an animation excerpt in
a digital edition. Reorder pages, edit a shared emblem, and export print and web
outputs with different framing/color intentions. Open it in an older reader
that knows none of the page-management features. These combinations are the
Capy thought experiment, not claims about the cited products' exact feature set.

**Attempted extension.** Make a page-collection root reference compositions. This
works for native editing, but the original single default preview can show only
the cover. A reader cannot even discover other renderable outputs without knowing
the new collection type. A single document-wide viewport/color context would also
conflate authored source color with each delivery's settings.

**Valid issue.** The initial fallback contract guaranteed access to one picture,
not discoverability of a multi-output work. Strengthening that guarantee requires
an explicit output boundary, not adding `pages` to the container.

**Revision 5.** Add a small, stable inventory of identified outputs to the envelope,
with a default output. Each points to a composition/view and can reference saved
representations. Today's document has exactly one. Future feature types own page
order, print geometry, time ranges and delivery settings; the inventory does not
become a second authoritative page-order list. An old reader can list named
outputs and available previews even if it cannot interpret their source types.
An output without a fallback remains visibly unavailable, not omitted. The minimum
uses the fixed default-preview convention when a preview is available; future
multi-output capabilities should define their own preview coverage promises.

**Recheck across all earlier cases.** Reordering pages changes collection order,
not content IDs or animation targets. Each output evaluates shared content under
an explicit view/context; its preview depends on that context. A 3D camera view,
a timeline poster and a print page are output uses of the same relationship,
not special archive sections. A reduced preview is not a valid print master.
The conservative unsupported-capability policy keeps the complete source intact.

#### Final retest and limits

The five tests change the recommendation's abstraction boundaries. They do not
justify embedding a universal scene graph, animation engine or application schema
in the envelope. The resulting model is: **identified typed artwork, explicit
instances and relationships, reusable resources, identified outputs and scoped
saved representations**. Only current feature types need implementations now.

| Retest | Result after the revisions | Feature-specific work deliberately deferred |
| --- | --- | --- |
| Layer FX on paint, vector or a 3D view | Definition/application separation and explicit evaluation scope avoid assuming that every effect consumes the stack below. | Exact FX stages, masks, bounds, blending and scale behavior. |
| Shared source with independent placements | Content identity and occurrence identity preserve both intentional linkage and independent copies. | Smart-object UI, linked-file resolution and override semantics. |
| Animated instances with live effects | Stable property targets and local time mappings survive reordering and reuse. | Tracks, exact timebase, interpolation and seeking implementation. |
| Posed/simulated 3D inside a 2D composition | Typed relationships and transitive capabilities distinguish storage support from actual evaluation support. | Scene/rig/material schemas and bounded solver semantics. |
| Multi-page work with different output views | A small output inventory exposes available views without teaching the envelope about pages or cameras. | Page management, typography, physical geometry and export recipes. |

A combined retest places two time-offset instances of the same rigged character
on different pages, applies an animated outline to only one, edits shared source
content and saves. The identities, relationships and output contexts above locate
what changes without duplicating source content or rewriting the other instance's
local state. All affected fallbacks must be regenerated or marked unavailable;
unsupported readers preserve the whole package and expose only valid saved views.
This reasoning finds no additional envelope change for the five scenarios.

The negative control is a procedural node that recursively requests its own
current output without a solver or time delay. The format should reject it,
not accept it merely because references are generic. Likewise, changing an
unknown effect's input cannot be made safe by retaining the old effect bytes.
Extensibility permits a future feature to define new semantics; it does not make
undefined semantics executable or unknown semantics safely editable.

The [adversarial review](#5-adversarial-review) later tested the model against
more tools and found changes these five scenarios did not exercise: references
that type-blind readers can find, ID paths into shared content, per-composition
frames and color, declared channel layouts and stricter container rules.

## 4. Minimal recommended foundation

### Choose a restricted standard container

| Candidate | Advantages | Costs and conclusion |
| --- | --- | --- |
| Evolve the current custom container | Reuses the compact index, streaming writer, tile checks and compressed backing. A generic resource directory could make it extensible. | We own every inspection/recovery tool and directory rule. A sound alternative if measured ZIP integration costs prove material; capabilities do not inherently require ZIP. |
| ZIP with ZIP64 support | Standard directory and offsets, independent resources, preview extraction and a single transferable file. Krita, Sketch, Penpot, Procreate and dotLottie use ZIP. | Central directory is normally read from the end; safe updates generally rewrite the package. Restrict the ZIP feature set and pack tiny blocks. **Recommended default.** |
| SQLite | Transactions, partial updates, indexes and a mature application-file story. [SQLite application files](https://www.sqlite.org/appfileformat.html). A [reverse-engineered parser](https://pypi.org/project/clipparse/) shows Clip Studio Paint keeping metadata in an embedded SQLite database beside offset-addressed pixel blocks. | Publishing a live database requires a consistent snapshot and journal handling; deleted data can remain in free pages without sanitization or compaction. [Corruption](https://www.sqlite.org/howtocorrupt.html), [pragmas](https://www.sqlite.org/pragma.html). SQLite's older large-blob benchmark favors separate files above roughly 100 KB on its tested setup, not universally. [Benchmark and caveats](https://www.sqlite.org/intern-v-extern-blob.html). A valid alternative, especially for a working store; ZIP better matches this proposal's member extraction and compressed-block reuse without a database layer. No Capy benchmark establishes a performance winner. |
| Directory bundle, tar or whole-file compression | A directory is convenient for development and large projects; Toon Boom Harmony and OpenToonz keep scenes as folders. Sequential containers are easy to stream. | A directory is awkward to transfer through mobile providers, and Apple warns that iWork packages can be damaged by browser uploads or email. [Apple](https://support.apple.com/en-us/HT202887). Tar needs another index for random access; whole-file compression couples unrelated media. None is the default user-facing file. |

ZIP64 must be supported from the first reader, including large entry counts and
large offsets; this is support for ZIP's large-file records, not a demand to use
them in every tiny archive. The
[PKWARE specification](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)
defines the transport. Use a maintained implementation and a deliberately small
accepted subset. Do not promise that operating-system archive tools open every
package: macOS Archive Utility has written invalid archives above 4 GiB or 65,535
entries. [yauzl-mac](https://github.com/overlookmotel/yauzl-mac). ZIP parsers
disagree about almost every ambiguous archive, so strictness is what keeps every
Capy reader seeing the same members.
[ZipDiff](https://www.usenix.org/conference/usenixsecurity25/presentation/you).

- **STORED members only**, with lossless LZ4 inside raster packs and existing
  compression inside PNG, video and similar media. This avoids double
  compression and preserves seekability within a pack. The tradeoff is
  uncompressed JSON and text resources; measure that cost before adding another
  mandatory codec.
- **Sizes and CRC-32 in every local header; no data descriptors.** APPNOTE allows
  descriptors with any method, but Java's `ZipInputStream` rejects them on
  STORED entries. [OpenJDK reader](https://github.com/openjdk/jdk/blob/master/src/java.base/share/classes/java/util/zip/ZipInputStream.java).
  Block checksums are CRC-32s ([raster storage](#retain-efficient-raster-storage-normalize-its-contract)),
  so a writer can combine already computed checksums and lengths in physical
  byte order, including any pack headers or padding, into a member's CRC.
  [zlib `crc32_combine`](https://www.zlib.net/manual.html).
  A non-seekable output works only after each member's size and checksum are
  known. Newly encoded previews, media or blocks must first be buffered or
  spooled to bounded private storage. Descriptors normally enable unknown-length
  streaming; excluding them is an interoperability tradeoff, not what enables
  streaming. CRC combination does not verify bytes that have not been read.
- **One canonical rule for classic/ZIP64 fields**, members without gaps or
  overlaps and no duplicate names. Specify when to use ZIP64 size/offset fields
  and require one terminal directory, with its end record and, when needed, the
  ZIP64 end record and locator. Multiple competing archive tails are forbidden.
- **Canonical relative ASCII member names that stay distinct when case is
  ignored.** Treat the package as an abstract set of members with ZIP as its
  exchange form, as EPUB defines its container separately from the ZIP mapping.
  [EPUB OCF](https://w3.org/publishing/epub32/epub-ocf.html). A folder form for
  development, very large projects or a working store then remains lossless on
  case-insensitive file systems, without being promised now.
- **`META-INF/` is reserved for signatures.** C2PA content credentials store a
  STORED manifest at `META-INF/content_credential.c2pa` whose hashes cover the
  other members and the central directory.
  [C2PA specification](https://spec.c2pa.org/specifications/specifications/2.2/specs/C2PA_Specification.html),
  [c2pa-rs ZIP handler](https://github.com/contentauth/c2pa-rs/blob/main/sdk/src/asset_handlers/zip_io.rs).
  Readers accept bounded optional members there without treating their presence
  as proof of authenticity. C2PA specifies zero CRC fields for its manifest;
  that exact member needs a documented exception to the normal member-CRC check,
  with structural/bounds checks still enforced. An edited save drops signatures;
  signing and verification remain separate future features. Reservation alone
  does not establish C2PA interoperability, especially for ZIP64.

An illustrative package, not final member names or a byte-level specification:

```text
artwork.capy
  mimetype             package identity, first and small
  manifest.json        envelope, objects, resources and outputs
  preview.png          current portable view of the default output, when available
  data/tiles-1.bin     indexed independent LZ4 blocks, when raster data exists
  data/<resource-id>   profiles, metadata, shader definitions and other assets
  META-INF/            optional signatures, never needed to open the artwork
```

Write one tile pack initially, but let the resource table list several and
locate each block by pack and range, so later partitioning does not change
artwork identity. The ZIP directory locates members; the pack index locates
blocks *inside* a member. Do not also duplicate ZIP's absolute file offsets in
JSON. Avoid one member per tile, stroke, vertex or animation sample. A future type
can put large arrays or time indexes in a binary resource without extending the
container grammar.

The pack index is authoritative. Readers accept byte ranges inside a pack that
no index entry refers to; first-format writers never leave any. That tolerance
keeps a later clone-and-append save possible without old readers rejecting its
files. Substance 3D Painter's incremental saves fragment its project archive
until the artist chooses Save and Compact.
[Adobe](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/technical-support/workflow-issues/project-issues/projects-are-really-big).
Whether an ordinary save may ever keep deleted bytes is a privacy decision for
that feature: Word's fast save and PDF incremental updates both exposed deleted
content. [Office Watch](https://office-watch.com/2003/sources-of-embarrassing-information/),
[recovering PDF versions](https://eclecticlight.co/2019/03/11/pdf-without-adobe-17-unredacting-manaforts-documents-and-recovering-pdf-versions/).
Shared and exported copies are always compacted.

ZIP makes no promise of zero-copy access through every host. A non-seekable
provider may need a bounded stream to private disk storage before random access;
the web host needs a suitable Blob/range or private-storage path. Saving must
write a complete replacement off the input thread and publish it through the
host's supported mechanism. Reuse compressed blocks, but acknowledge that a ZIP
rewrite still copies bytes. Large-video incremental editing may eventually need
a separate working store or explicit linked media. Recovery and autosave write
ordinary packages to private storage; only their publication and retention differ.

### Keep the file model small and independent

The following stack types and ordering rules describe the first supported subset
of one authored graph. The [implementation plan](../development/capy-format.md)
defines the source/occurrence boundary, typed ports, runtime alternatives and
qualification gates. Layers and nodes do not have separate saved authorities.

The revised durable model needs five concepts:

1. **Document envelope:** format identity and envelope version, a document ID, a
   reference to the authored root, the outputs and portable metadata. The root is
   what the editor opens: today a composition whose result is a structured stack,
   later perhaps a page collection or a project bin.
2. **A flat table of typed objects:** every independent artwork record is a top-level
   object with a globally unique ID and a namespaced type. Relationships are
   references, never array positions or duplicate embedded definitions. Ordinary
   values such as colors, bounds and transforms remain inline; they need no IDs
   or separate records. Types own their fields, the stable keys of addressable
   properties and the meaning of their references. Identity is independent of
   array position, display name, runtime allocator and content hash. Every object in the table is
   retained authored work, including hidden layers and, later, unassigned cels and
   unused takes. Deletion is explicit: an object is not invalid or collectible
   merely because the root and outputs do not reach it. Validate every retained
   object's compatibility and references, including unplayed work. Future types
   may define local subelement identities without making each point or sample a
   document object.
3. **Resources:** immutable binary or text payloads in one resource table. Objects
   reference resource IDs; the table gives each payload's location (today a
   member, or a pack and range), encoding, length and checksum. Several objects
   can reference the same payload. Pixel interpretation belongs to the resource's
   versioned resource type, not to a filename, a GPU texture format or a
   document-wide setting. The transport does not impose a plane or channel
   schema on resources.
4. **Outputs:** a small inventory of named results and a default, independent
   of layer or page order. Each names a source composition, its evaluation
   context, its delivery intent and its saved representations. Today there is one
   canvas output, which owns the SDR rendition and proof recipe. Later outputs can
   add page framing, cameras, time ranges, parameter values or an interactive
   initial state without adding those fields to every document. Settings have one
   owner and are referenced where shared. A nonempty inventory identifies one
   default from its entries. The envelope also permits an empty inventory,
   with no default: a future palette, pose or motion library need not invent a
   canvas. Today's layer-stack document still requires its one canvas output.
   An output's type defines whether it is an image, channel set, audio or something
   else; a PNG is only its saved visual representation.
5. **Saved representations:** a conventional preview and, later, optional richer
   fallbacks, each belonging to an output. A save includes a preview of the exact
   saved snapshot or omits it, so the first format needs no staleness
   bookkeeping. Future optional movies or scoped fallbacks need dependency and
   snapshot rules; omit or invalidate a stale proxy instead of blocking an
   ordinary save on a full-film render. Source data remains authoritative unless
   the artist explicitly converts it.

**A composition type owns its evaluation domain.** Its frame is today's canvas:
size and origin, physical resolution, working color space and depth, and blend
space. The composition references its result; a stack owns order and inherits
that context. Offsets, effects and paper are defined relative to the composition,
and content may extend beyond it. A nested composition later brings its own
frame, and a timeline adds a time base and duration, as After Effects compositions
do. Other types
may define a 3D world, a texture set or an audio domain without a dummy pixel
canvas. Image outputs frame and scale a composition rather than redefining it.
Today's effect lengths are in canvas pixels. Every parameter declares its
dimension and reference space in the schema. A source-pixel radius, a
composition-space distance and a normalized fraction are different meanings;
Image Size rules belong to those meanings. Rendering at another scale,
half-resolution proxies and Image Size then handle lengths without consulting
display labels. Color belongs to the composition that blends and to each
resource that stores samples, so the roadmap's per-layer linear blending becomes
an override of the composition's default rather than a conflict with a document
setting.

**Separate editable content from occurrences in the baseline.** A paint source
owns its local domain, imported base, sparse overrides and material state; an
occurrence owns placement and compositing controls. A mask use references a
coverage source. Carry these identities into the shared editor and renderer, so
loading does not rebuild a second layer authority and painting does not resolve
ownership through a file-only wrapper. The initial editable subset uses at most
one occurrence per editable source; linked uses remain a later feature. Independent
Duplicate assigns new source identity while sharing immutable backing. Store
each effect program once as a resource, and let effect applications reference it
with values keyed by stable parameter keys.

Store order once as a stack's ordered occurrence references. Do not add an
independent authoritative parent/order table. Distinguish composition membership
and stacking order from transform parenting, instancing, resource references and
evaluation inputs. A future track matte can reference a sibling independently
of z-order; a transform parent need not own the child or its compositing scope.
Today's mask ownership and clipping rules remain rules of the stack type.
Known types validate their own relationship/cycle rules; do not assume all
references form one tree or one executable DAG. The baseline gives evaluation
connections typed endpoints with stable port keys and defines scoped backdrop
and clipping inputs for stacks. Future operations extend those interfaces; a
timeline can reference compositions and media; a page collection can reference
multiple compositions. These can nest rather than being mutually exclusive root
modes. Existing objects need not become fictitious layers in these models.

Effect applications refer to definitions and explicit inputs. A type's evaluation
contract defines ordering, coordinate/color space, mask/coverage behavior and any
backdrop dependency. Keep those semantics in versioned feature schemas; do not
standardize an all-purpose list of modifier stages in the envelope. The first
implementation supports today's effect layers and embedded filter programs;
layer FX and other application types arrive with their features.

Use thin wire adapters in shared Rust, with validation and one-time resolution
of portable IDs to compact runtime handles. File and editor share semantic
ownership; wire spelling does not dictate arena layout or execution plans. Keep
semantic rules and compatibility checks shared across hosts. JSON
is adequate for the small structural description; reject duplicate JSON keys
before interpreting a record. Avoid large numeric/byte arrays, base64 media,
serialized pointers and runtime enum ordinals. IDs are opaque
strings. Define lossless handling of large integers so JavaScript tooling cannot
round offsets or future time values above its exact-integer range. Binary offsets
remain 64-bit; JSON representations must not rely on an imprecise number parser.

Freeze the meaning of coordinate systems, child order, transforms, interpolation,
blend spaces, blend formulas, clipping and pass-through behavior for the baseline
type. That includes rules that today live only in code: the clipping base is the
next lower sibling that is neither clipped nor a selection layer and must be
paint; a clipped pass-through group composites as an isolated Normal group;
linked mask geometry applies on top of the owner's placement; mask coverage
combines the initial selection or default coverage with painted overrides before
inversion. See [groups and clipping](../internals/documents.md#groups-and-pass-through).
Keep existing projective/mesh placement as semantic geometry, not a snapshot of
GPU control buffers. This is the unavoidable specification work for today's
features; generic extensibility is not a substitute for defining them.

### References, paths and evaluation context

**References are visible without knowing the type.** Every reference to an object
or resource uses one reserved JSON form, such as `{"ref": "<id>"}`, wherever it
appears. Reserve this form so literal user data cannot be mistaken for a
reference; imported arbitrary JSON can remain an opaque resource. Paths use
visible references at cross-object steps; property keys and type-owned
subelement selectors remain local selectors. Binary/code payloads use local
dependency slots, with the owning record binding those slots through visible
references. They must not also embed Capy object/resource IDs that require
rewriting when their owner is copied. A dependency list beside hidden IDs would
permit retention but still require a format-specific binary rewrite on paste.
Mesh indices, palette indices and IDs internal to an embedded foreign file may
remain local to that resource; its Capy-facing dependencies use the binding rule.
An expression accesses declared inputs, which may include an explicitly bound
collection for procedural queries. It does not construct hidden cross-document
references from names or IDs inside code. A reader can then retain dependencies
without understanding the type. This is necessary, but insufficient, for safe
copying or editing: dependency discovery alone does not define ownership,
evaluation or cloning.
glTF hides references inside extension data, so glTF Transform does not write
unregistered extensions and proposes passthrough
only for extensions without references.
[Writer](https://app.unpkg.com/@gltf-transform/core@4.2.0/files/src/io/writer.ts),
[issue 1856](https://github.com/donmccurdy/glTF-Transform/issues/1856).
OPC keeps references in separate relationship parts, and USD makes paths a
data-model type that namespace editing repairs without knowing the schema.
[OPC relationships](https://python-pptx.readthedocs.io/en/latest/dev/resources/about_relationships.html),
[USD namespace editing](https://openusd.org/release/user_guides/namespace_editing.html).
Without a visible form, the rule that old readers must not collect resources
they cannot see could never be relaxed.

**IDs are globally unique and never reused.** Writers generate random 128-bit IDs
for the document and its objects and keep no allocator state in the file. This
avoids allocator coordination across documents; readers still reject duplicate
IDs. Independent duplication or repeated paste creates new authored identities
and remaps references within the copied set. Intentional links retain source
identity, while branches can share IDs but hold different revisions: an ID is
not a content hash or a conflict-resolution policy. Figma puts a per-client ID
into every object ID to coordinate offline creation; Sketch, Penpot and Krita
use UUIDs; Aseprite added layer UUIDs
later behind a header flag.
[Figma multiplayer](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/),
[Sketch format](https://developer.sketch.com/file-format/),
[Penpot format](https://help.penpot.app/technical-guide/developer/data-model/penpot-file-format/),
[Aseprite specification](https://github.com/aseprite/aseprite/blob/main/docs/ase-file-specs.md).
Today's `LayerId(u64)` is a JSON number that can exceed JavaScript's exact range.

**An ID names an object; a path selects it through instances.** When content is shared,
each of its objects appears in every occurrence. A key, override or mask aimed at
one occurrence's part addresses it by the path of IDs from its scope through each
occurrence, and a one-element path means the local object. Figma's override
`GUIDPath`, Sketch's UUID-path override names and Rive's `dataBindPathIds` work
this way, while USD's expanded prototype paths are documented as unstable.
[Figma instances](https://developers.figma.com/docs/plugins/api/InstanceNode/),
[Sketch overrides](https://developer.sketch.com/reference/api/symbol-override.txt),
[USD instancing](https://openusd.org/release/api/_usd__page__scenegraph_instancing.html).
Paths cross instancing boundaries, not every grouping or transform-parent edge.
Inside a content scope the target is identified directly by ID, so regrouping
within that scope preserves the target. Moving across content scopes needs an
explicit retargeting operation; stable IDs alone cannot preserve its evaluation
context. Paths survive reordering and renaming; names and indexes do not.

Identity must not silently double as a procedural seed. Independent duplication
remaps IDs while retaining authored seed values and initially preserving the
picture. A feature may offer a separate randomize operation. After Effects
documents seeds derived partly from layer identity, illustrating the coupling
this rule avoids. [Expression random-number methods](https://helpx.adobe.com/after-effects/desktop/work-with-expressions/expression-language-reference/expression-language-reference.html).
Copying an object still needs its known type's clone rules, including which
bindings are shared or copied; visible references do not decide those rules.

**Context, not copies, makes occurrences differ.** An occurrence references content
and supplies a context: placement and local effect applications today, and later
a time mapping and parameter values. Content may later declare an interface of
named, typed parameters with defaults; occurrences override by parameter, and
outputs supply a context of their own. One mechanism then covers Smart Object
and component variants, After Effects Essential Properties, Cavalry pre-comp
overrides, Rive inputs and data binding, Live2D parameters, Figma variable modes
such as theme or language, Photoshop data sets and layer comps.
[Figma modes](https://help.figma.com/hc/en-us/articles/15343816063383),
[Cavalry referencing](https://cavalry.studio/docs/user-interface/menus/window-menu/assets-window/referencing/),
[Rive stateful components](https://rive.app/docs/editor/data-binding/stateful-components),
[Photoshop data-driven graphics](https://helpx.adobe.com/photoshop/using/creating-data-driven-graphics.html).
Prefer declared interfaces to arbitrary deep overrides: Rive makes each nesting
level re-bind, and Blender's library overrides need a resync when the library's
hierarchy changes.
[Blender library overrides](https://docs.blender.org/manual/en/latest/files/linked_libraries/library_overrides.html).

**Bindings are separate records.** Base values stay literal. A binding record
targets a path and property key and supplies the value from a swatch, variable,
curve, expression or rig driver under a defined combination rule. An older reader
sees an unknown binding type and stops editing, rather than editing a literal
that is no longer authoritative. This keeps first-format property schemas free of
value unions and still allows palette recoloring as in Toon Boom Harmony, where
a cloned palette keeps its color IDs so a scene can switch to a night palette.
[Harmony palettes](https://docs.toonboom.com/help/harmony-24/paint/colour/clone-palette.html).
A raster encoding can likewise refer to a palette, as OpenToonz's ink, paint and
tone pixels do. [OpenToonz styles](https://opentoonz.readthedocs.io/en/latest/managing_palettes_and_styles.html).

**Resources can later live elsewhere.** A resource-table entry, not the reference
to it, says where a payload lives. A later linked-media or library feature adds
an external location with a document ID, object path, expected hash and an
embedded cached copy, without changing any reference. A missing link can show
the cache; native editability depends on what source was retained and the
feature's linking policy. A flattened cache cannot restore editable source.
Clip Studio Paint keeps a comic as a management file plus one file per page, InDesign books
reference separate documents, Harmony and OpenToonz share project folders across
scenes, and Sketch caches the library symbols a document uses.
[CSP page files](https://help.clip-studio.com/en-us/manual_en/570_pages/Management_Files_and_Page_Files.htm),
[InDesign books](https://helpx.adobe.com/indesign/using/creating-book-files.html),
[OpenToonz projects](https://opentoonz.readthedocs.io/en/latest/managing_projects.html).
Self-contained remains the default; one file need not mean one project.

### Should the file already be a general node graph?

**Selected direction: one authoritative authored graph, with layer and node
editing as views of that representation.** Avoid separate saved layer and node
models that must be synchronized. Start with structured stacks, separate editable
sources and occurrences, and typed interfaces. This replaces both the deferred
content split and the idea of adding a separate graph composition model later.

The [node/layer assessment](authored-graph-research.md) supplies the workflow
requirements. The [implementation plan](../development/capy-format.md) specifies
the baseline ownership, code changes, extension cases and acceptance gates.
The byte schema and runtime layout remain provisional until their prototypes
pass correctness and performance qualification. Selecting the direction does
not establish implementation or measured feasibility.

Graphite demonstrates the product direction: layers and nodes are two views of
one document, and canvas edits modify the graph. Its developer guide describes
reusable node networks with explicit inputs/outputs, including a time input for
rendering animation. Its feature page also distinguishes current capabilities
from its roadmap, so it is not evidence that every proposed editing workflow is
already solved. [Graphite features](https://graphite.art/features/),
[networks and nodes](https://graphite.art/volunteer/guide/graphene/networks-and-nodes/).
MaterialX likewise gives node instances typed inputs and connections to declared
definitions. [MaterialX node model](https://materialx.org/docs/api/class_node.html).

Three different graphs must not be conflated:

- The **artwork reference graph** describes identities and relationships: an
  instance uses content; a mask belongs to an occurrence; a composition contains
  occurrences. The recommended file can represent this from the start.
- An **authored evaluation graph** is a program: named typed ports, connections,
  parameter bindings, outputs and defined evaluation semantics. It is the
  common representation for layer and node editing. Structured stacks are its
  first supported subset; arbitrary graph editing is a later feature.
- The **runtime execution graph** schedules tiles, passes, cache reuse and GPU
  work. It is derived and can change with optimizations; it should not be saved
  as the artwork's durable meaning.

A generic `{id, type, inputs, parameters}` record looks small, but the difficult
decisions are what its edges mean. Is an image a finite raster, an unbounded field
or a context-dependent function? Does an input carry alpha, color interpretation,
units, local coordinates and time? Are masks applied before resampling? Are
multiple consumers evaluating a shared object in the same context? How do
parameter bindings address subelements? What happens on a cycle, unavailable node
or unknown port type? Leaving these undefined gives us a graph-shaped file, not
a portable general node schema.

Today's groups expose the problem without any speculative features. For an
isolated group, children compose against transparency. For a pass-through group,
children evaluate against the incoming backdrop B; group opacity and mask fade
between B and that result. In the current semantic contract this is conceptually
`mix(B, group(B), opacity * mask)`, in the applicable blend domain. An ordinary
`Group(children) -> image` node cannot express this correctly without a backdrop
input or an explicit scoped evaluation rule. Layer FX also may require both
pre-fill coverage and final appearance; representing every operation as one
RGBA-in/RGBA-out shader is too restrictive.
[Existing group semantics](../internals/documents.md#groups-and-pass-through).

The node assessment addresses how the layer view handles sharing, fan-out and
graphs without a single layer ordering, and what reorder, group and delete mean
in those cases. The unified representation does not imply that every graph can
be fully edited through a layer list; later editing features still need their
own workflow qualification.

Before landing an implementation, require evidence that it preserves current
masks, clipping, isolated/pass-through groups and live adjustments; shared
content evaluated in two contexts; and layer edits without lost authored data.
Validate pixel equivalence and bounded incremental rendering. Follow the plan's
prototype and cutover stages; do not persist runtime scheduling or require
general node editing to ship the baseline format.

### Compatibility signalled by content

Use **two levels of versioning**, not a minimum application release:

- A rarely changed envelope version defines how to locate and parse the package.
- Type names carry a major version, such as `capy.layer-stack/1`,
  `capy.raster-tiles/1`, `capy.paint-material/1` and `capy.effect-wgsl/1`. The
  registry and spelling can be decided later. A major version changes only when
  an existing meaning changes; additive features need no version number.

Three rules make additions visible without capability lists:

- **Writers omit every field at its default.** A new field appears in a file only
  when the artwork uses its feature, so a plain raster drawing saved by a future
  animation release contains nothing an older reader lacks. Defaults are part of
  a type's frozen meaning. A newly added field's absence must preserve the old
  behavior; UI defaults may change, but then the writer emits the non-default
  wire value. Never omit a field just because it equals today's UI default.
- **Unknown is not invalid.** An unknown type, field, enumeration value, resource
  encoding or location kind in a record the reader must interpret makes the
  document *unsupported*, not corrupt; malformed known data is invalid.
  Requirements close over nested content, effect definitions and resource
  encodings because each of those is its own record: a supported 3D object does
  not imply a supported rig or material. A structurally valid combination of
  known records can also exceed the editor's supported subset, such as linked
  editable sources before shared-source editing exists. Test support separately
  from structural validity; preserve unsupported combinations in preview mode.
- **Records say whether they can be ignored.** Following PNG's chunk properties,
  a record may be `ancillary`, so a reader that does not understand it can still
  display and edit the document, and `copy_safe`, so that reader may keep it
  unchanged after edits. An unknown ancillary record that is not copy-safe is
  dropped on an edited save, and so is a kept one whose references no longer
  resolve. Default both marks to false, and reject marks that contradict a known
  type's schema. Required artwork never depends on ancillary records.
  For the baseline, ancillary records may reference artwork and their payloads,
  but not other ancillary records. Copy safety must remain true after arbitrary
  allowed artwork edits, not just while referenced IDs still exist. This mark
  permits retention in the same document, not attaching unknown metadata to an
  independently duplicated object. The baseline does not clone unknown records.
  [PNG chunk naming](https://www.w3.org/TR/png-3/#5Chunk-naming-conventions).

The flat tables are the capability inventory. A reader checks the fields and
values of every retained non-ancillary object and the encodings of resources it
needs. Resource payloads used only by ignored ancillary records require bounded
transport validation and preservation, not decoder support. The generic
reference form makes those dependencies discoverable. There is no
separate `used`/`required` list to keep consistent with the contents, and no
table of which feature needs which minimum version. OpenTimelineIO illustrates
another tradeoff: when a schema version advances, writing its older form uses
registered downgrade functions and version targets. That explicit migration
model also handles semantic changes that omission alone cannot handle.
[OTIO versioning](https://opentimelineio.readthedocs.io/en/latest/tutorials/versioning-schemas.html).
Protobuf's open enumerations similarly treat an unknown value as one from a
newer schema rather than as a parse error. [Enum behavior](https://protobuf.dev/programming-guides/enum). Rive's
runtime format skips unknown properties as no-ops, which suits a player but not
an editor that would then save a damaged document; adding state machines still
needed an incompatible major version.
[Rive format](https://rive.app/docs/runtimes/advanced-topic/format).

**Interpreting a record means faithful native rendering and safe editing**,
including hidden authored content, not only displaying a flattened appearance.
A preview never makes authoring data ancillary. Ancillary records are genuinely
disposable or independent, such as view settings, tags or review comments; they
must not secretly affect rendering or future edits. Keep the known schema strict
against typos: a misspelled field from a faulty writer is reported as
unsupported content, and the preview path still applies. Do not globally turn
off validation to accept future files.

After the compatibility reset, new code must continue to understand the baseline
and every subsequently supported type version, directly or through lossless
conversion. A converter that changes appearance or removes editability is not
backward compatibility. Changed shader helpers, brush evaluators or blend
formulas require compatibility implementations or demonstrably equivalent
translation. A preview protects access to appearance when evaluation is
unavailable; it does not satisfy the full old-file editing commitment.

This does **not** require maintaining today's pre-reset v14 reader: that can be
removed at the authorized compatibility break. The maintenance commitment begins
with the new baseline. Do not promise lossless writing to arbitrary older
versions; omitting defaults already writes the smallest representation of a
document, while lossy conversion should be explicit.

### Clean degradation without silent loss

| What the reader encounters | Initial behavior |
| --- | --- |
| Supported envelope and every non-ancillary record understood | Validate and open for native editing. |
| Unknown ancillary record | Ignore it for display and editing. Keep it unchanged when it is copy-safe; otherwise drop it on an edited save. A no-edit copy keeps everything. |
| Unknown type, field, value, codec or location in a record that must be interpreted | List identified outputs and show their available saved representations; retain the source, with native editing disabled. |
| Known format exceeding the device's resources or evaluator support | Offer the same preview path; distinguish these limits from invalid artwork. |
| Unsupported envelope | Try only the fixed baseline preview convention within a safely parsed package; otherwise give a clear unsupported-format result. |
| Corrupt native content | Refuse normal editing; show a verified preview only as a recovered view. |

The minimum safe first policy is **whole-document preview mode when any record
that must be interpreted is unsupported**. Do not build a speculative partial-edit
dependency system now. It should allow exporting the available representation to
a new file and keeping/copying the original package, without overwriting the
source with the fallback.

Opaque preservation must include payloads, IDs and every resource they refer to;
visible references identify those resources without understanding the type. A
no-edit copy can preserve the original package byte-for-byte. Generic retention
is not proof of semantic validity after mutation, so unknown non-ancillary
content leaves the document read-only, and unknown ancillary content survives an
edit only when it declares itself copy-safe. Microsoft's Open XML SDK documents
that compatibility preprocessing can remove unknown markup and that only the
remaining markup is saved. This illustrates why parsing around unfamiliar
fields is not a lossless round-trip policy; it does not establish that all Office
preservation mechanisms were abandoned.
[Markup compatibility](https://learn.microsoft.com/en-us/office/open-xml/general/introduction-to-markup-compatibility).

Later, partial editing can be introduced for well-defined isolated scopes, and
visible references give a future reader the dependencies it needs without a
format change. A vector object may have a raster fallback; an unsupported effect
that reads its backdrop may require a fallback for an entire composition. Unknown
blend semantics cannot be replaced by Normal. Future fallbacks must identify
scope, resolution, bounds, color/alpha interpretation and, for animation,
time/range. Their validity is tied to the exact source snapshot and dependencies.
Never silently keep a stale fallback after changing an input.

Define one fixed, bounded, standard sRGB PNG convention for the default output
in its default context: a representative time for a timeline, the initial state for an
interactive output. For HDR, use the authored SDR rendition and identify the
preview as SDR. It should contain no checkerboard, selection overlay or editor
chrome. When an output is not one color image, such as a texture channel set,
spot plates or an interactive scene, the preview identifies it and does not
stand in for it. Exact size is a later tuning decision; readers must expose when
only reduced resolution or one frame is available. This small baseline does not
promise full-resolution recovery or playback. Additional full-resolution
composites, object fallbacks or animation proxies are optional resources,
avoiding compulsory duplication of every layer/frame. The output inventory
permits discovery of future pages/views even without their native types. A
missing representation is reported for that output; the default preview must not
imply that it represents the entire multi-output work. For an unsupported
envelope, only the fixed default-preview convention is safe to rely on, not an
unfamiliar output inventory.

Ordinary supported saves should generate the preview from the same committed
snapshot as the artwork, using asynchronous GPU work and worker encoding. An
existing preview can be reused only when the writer establishes it represents
that exact snapshot and output context. It must not add UI-thread readback or
make manual-save completion refer to a newer/different image.

Preview generation is not a condition of source publication. If source capture,
validation and writing succeed but rendering is unavailable or exceeds its work
budget, save the source with no preview member or representation reference and
report that distinction. Never retain an older image as the current preview.
A known reader can edit intact source without it; an unsupported reader can
still retain/copy the package and list outputs, but has no image to show. This
deliberately weakens guaranteed fallback availability to protect saving work.
It also keeps recovery and manual saves on one package validity contract.
Failures of source capture, integrity or publication still fail the save and
leave the prior published file intact where the host supports atomic replacement;
preview omission cannot rescue paint that was never successfully captured.

### Retain efficient raster storage, normalize its contract

Keep 256-pixel tiles and LZ4 as the first raster encoding. That is a deliberately
small baseline, not a rule that all future content has 256-pixel tiles. A later
encoding can change tile sizes, codecs or layouts as a new encoding type without
changing the resource mechanism. Do not add several mandatory codecs now.

Retain U8/U16/F16/F32, source profiles and HDR interpretation. The baseline
raster type defines today's color, coverage and material layouts, including
sample order and type, byte order, color/alpha interpretation, finite-value rules
and missing-tile meaning. Constants belong to that encoding's specification;
do not repeat fixed channel names or a general layout description on every tile.
Resource records carry only the varying descriptors and profile references.
The baseline composition type may require paint to match its working format, but
the container no longer compares every tile with a document-wide color.

Do not implement an arbitrary plane/channel description language in the first
format. New resource types can add layouts without changing resource addressing.
Substance 3D Painter paints base color, roughness, metallic, normal and height
with a blend mode per channel; Photoshop has spot channels; OpenEXR supports
typed channels and variable-length deep samples; OpenToonz packs ink, paint and
tone indexes into a pixel.
[Substance blending](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/layer-stack/blending-modes),
[OpenEXR](https://openexr.com/en/latest/TechnicalIntroduction.html),
[OpenToonz CM32](https://github.com/opentoonz/opentoonz/blob/master/toonz/sources/include/tpixelcm.h).
Those need new resource/evaluation types, not a universal pixel structure or a
new resource mechanism. Color transforms do not apply to IDs, normals or ink
coverage merely because they are samples. A future texture set may reference
several rasters with different dimensions; a UV tile is not a 256-pixel storage
tile. Prefer one portable straight-color representation for ordinary saved
paint; this is not a requirement to convert every foreign image or future
compositing resource to that representation. Host attachment formats should not
create alternate artwork meanings.
A tile's default is context-dependent: absent paint may be transparent, an
absent override may reveal source pixels, and an absent mask tile may mean the
mask's declared coverage. Preserve those distinctions.

Check each stored block with a CRC-32 of its stored bytes, kept in the pack
index. A reader can then check stored-byte integrity, or copy a checked block
into a new save, without decompressing it. This detects accidental corruption;
it neither authenticates content nor proves decoded samples valid. Zarr
checksums encoded chunks and Parquet checksums each
compressed page for the same reason.
[Zarr crc32c](https://zarr-specs.readthedocs.io/en/latest/v3/codecs/crc32c/),
[Parquet checksums](https://parquet.apache.org/docs/file-format/data-pages/checksumming/).
Use CRC-32 rather than CRC-32C or xxHash because block values combine into the
ZIP member's CRC. The lazy-read benefit comes from checking stored bytes and
deferring decode, not from CRC instead of SHA-256: a stored-byte hash could also
be checked lazily, with a different integrity/cost tradeoff. CRCs have weaker
collision resistance; never use them alone to deduplicate different blocks.
Validate the manifest/index CRC before trusting descriptors, and block CRCs
before decoding or reusing their bytes. Do not scan an entire pack merely to
open one tile; the member CRC can be checked on a full read. Enforce decoded
lengths, bounded LZ4 decoding and sample rules such as finite floats on use.
If a later read finds damage, report it and preserve the original instead of
silently saving replacement pixels. Content hashes for deduplication
are runtime identity; if a later format records them, define their input bytes
explicitly rather than as Serde output. Keep integrity checking per independently
accessed block; a whole-file checksum alone would require scanning huge media to
verify one frame. Neither CRCs nor hashes authenticate an untrusted file. Do not
confuse resource checksums with authored object IDs or require rewriting
references after every edit.

Named selection coverage can share the same resource system. Its on-disk packing
must be a defined coverage encoding, not whatever word packing the runtime happens
to use. Preserve contours where they are authoritative and all non-raster mask
semantics rather than baking them merely to simplify storage.

### Admission, publication and the evidence needed before landing

Accept only the ZIP subset above: reject duplicates, ambiguous path spellings,
names that collide when case is ignored, traversal, symlinks, encrypted/multipart
archives, data descriptors and unsupported methods. Read members through an
archive abstraction rather than extracting arbitrary paths. Check the central
directory against local headers, ranges and lengths; bound metadata, nesting,
counts and all decoded resources. Unknown payloads still need bounded retention
and transport-integrity checks. Do not execute code, access the network or
compile every shader merely to show a preview.

Save a consistent snapshot to a new package; validate completion before durable
publication. Recovery and autosave reuse the package but never claim to be a
published save. Atomic replacement guarantees depend on the host/provider; the
container must not claim guarantees a mobile provider or browser download does
not offer.

No runtime change or performance measurement accompanies this report. Before
committing to the format implementation, require evidence for:

1. **Today's artwork round-trips:** all existing layer/group/clipping modes,
   linked/unlinked masks, transforms/meshes, out-of-canvas data, saved selections,
   sources/profiles, HDR, proof/rendition, material state and embedded effects.
   Compare semantic data and rendered output, not just successful parsing.
2. **Compatibility and preservation:** a permanent baseline fixture corpus; old
   type versions opened by new code; synthetic unknown types, fields and
   enumeration values reported as unsupported; unknown ancillary records kept or
   dropped by their copy-safe mark; references from unknown records keeping their
   resources; unsupported shader/codec; no-edit copying and no silent lossy
   overwrite. Test frozen wire defaults independently of changed UI defaults,
   unused retained objects, opaque ancillary-only resources, independent paste
   with remapped IDs, binary dependency slots, regrouping within a content scope,
   authored seeds preserved through duplication, missing dependencies and stale
   optional previews. Exercise an empty output inventory with a synthetic future
   root; the current stack must still require its canvas output. Reject duplicate
   JSON keys before resolving references.
3. **Transport failures:** truncated or conflicting directories, duplicate
   IDs/member names, case-colliding names, data descriptors, unreferenced pack
   ranges, corrupt blocks, overflow, oversized decode claims, failed writes,
   cancelled saves, provider failure during publication, and signature members
   removed by an edited save. Exercise precomputed sizes/checksums on non-seekable
   outputs, bounded spooling, classic/ZIP64 boundaries and C2PA's zero-CRC
   manifest exception before claiming interoperability.
4. **Measured cost:** file size, save/open time, peak RAM, preview cost, unchanged
   save/recompression behavior, opening a large document without decoding every
   tile, large resource access and animation-shaped resource counts. Compare ZIP
   against the existing container on representative artwork. Measure any affected
   frame paths under the [performance rules](../performance/measuring.md); do not
   infer tier compliance from a container choice.
5. **Host journeys:** save/open/save-as, recovery, continued painting during save
   and preview-only opening on GTK, Web, Android, Apple and Windows. After source
   capture succeeds, force preview rendering/encoding failure: the source still
   saves and reopens, the old preview is absent, and the result identifies the
   unavailable preview. Source-capture or publication failure must still fail
   the save. Exercise non-seekable provider streams and large offsets, not only local seekable
   files, with the [checks appropriate to the implementation](../development/testing.md).

The first format should contain today's artwork as structured authored graph
types in a flat object table: composition context, ordered stacks, separate
sources and occurrences, stable typed endpoints, random IDs and visible
references. Keep a resource table with today's typed layouts and per-block
CRC-32s, one output owning delivery intent, content-signalled compatibility with
ancillary and copy-safe records, and an optional current preview at a fixed
location. Leave reusable graph interfaces, bindings, timeline schemas, vector
geometry, 3D scenes, rigs, linked resources, incremental archive updates and
collaboration protocols to their features. Follow the
[implementation plan](../development/capy-format.md) before freezing the wire
schema. Adding those features should extend authored types without replacing
the container, maintaining a second layer document or reinterpreting artwork.

## 5. Adversarial review

The review challenged the recommendation two ways: what the first format can
drop, and which features of established tools its abstractions cannot express.
A change was accepted only if it makes today's model simpler or more robust, or
if leaving it out would be hard to correct once files exist. Sources are product
manuals and specifications; Clip Studio Paint, Procreate and Figma file internals
come from reverse-engineered descriptions. Section 4 incorporates the accepted
changes. The envelope and typed-reference architecture is sufficient for the
reviewed feature families. It needs the following boundary corrections before
implementation, not a larger universal scene model. These are design arguments,
not proof of implementation or performance.

### Simplifications adopted

| Change | Replaces | Reason |
| --- | --- | --- |
| Omit defaults; unknown means unsupported | `used`/`required` inventories and per-type minor versions | One source of truth. Frozen wire defaults let older readers keep editing newer files that use no new feature; semantic changes still need new type versions and maintained readers. |
| `ancillary` and `copy_safe` marks | Preserved optional attachments, and read-only mode when one might depend on edits | PNG's long-tested rule. An older reader can still edit and knows what to keep. |
| Separate sources and occurrences in file and editor | Deferring the split until editable content is shared | Resolves paint-target versus placement identity once. Immutable backing remains shared; no per-frame conversion to an older layer model. General shared-source commands remain deferred. |
| No editing-state record | An optional transferable editing-state attachment | Active layer, mask inspection, the current selection and selection display stay in local session state keyed by document ID; an ancillary record can carry them later. |
| Current preview or no preview | Mandatory rendering before source publication, and first-format staleness bookkeeping | A bounded image does not imply bounded evaluation. Missing rendering support must not prevent saving captured source; absence is simpler and safer than retaining stale pixels. |
| Today's raster layouts in a versioned type | Arbitrary channel/plane descriptors in the first implementation | Transport extensibility already comes from typed resources. UDIM sets and deep samples are not solved by adding channel names to today's pixels. |
| Inline compound values | Reading “flat object table” as “every structure needs an ID” | Colors, transforms and bounds have no independent identity or lifetime. Keep only independently addressed records in the table. |
| CRC-32 per stored block and deferred decoding | SHA-256 over decoded pixels and eager tile validation | Stored-byte checks permit copying without decoding; CRCs also combine into ZIP CRCs. Weaker collision resistance is a deliberate tradeoff, not a deduplication guarantee. |
| Retention by table membership | Retention traced from rendered outputs or the root | Unassigned cels and unused takes stay because they exist; object deletion is explicit. Missing reference targets remain invalid. |
| One package for recovery and saves | A separate recovery contract | One writer and reader; only publication differs. |

### Gaps found

| Use case and evidence | Gap | Change | When it must be decided |
| --- | --- | --- | --- |
| Keeping unknown data with its dependencies: glTF Transform drops unregistered extensions; OPC and USD make references visible. | References hidden in type-specific data. | One reserved reference form. | Now: old readers must recognize it. |
| Paste, libraries, branching and collaboration: Figma, Sketch, Penpot and Krita IDs; Aseprite's late UUID flag. | ID form unspecified; allocators in the file. | Random 128-bit string IDs. | Now. |
| Overrides and keys on parts of shared content: Figma `GUIDPath`, Sketch override paths, Rive `dataBindPathIds`, USD instancing. | Targets were single object IDs. | ID paths through occurrences. | The rule now; the schema with the first sharing feature. |
| Variants, layer comps, data sets, poses, theme and language: After Effects Essential Properties, Cavalry, Rive, Live2D, Figma modes, Photoshop data sets. | Occurrence-specific state named, with no common mechanism. | Occurrence context, declared interfaces and outputs with a context. | The principle now; schemas are additive. |
| Palette recoloring and global swatches: Harmony, OpenToonz, Illustrator. | No place for value references. | Binding records; raster encodings may refer to palettes. | Additive. |
| Parameter-keyed animation and clips reused across rigs: Live2D, Moho, Rive joysticks, Spine. | Curves assumed to be time-only; clip ownership unspecified. | Curves over context inputs; clips separate from what they animate. | With the animation feature. |
| Interactive output: Rive and dotLottie state machines, Spine mixing. | Evaluation assumed to be a function of time. | Outputs declare how they are evaluated; previews show the initial state. | Additive; current outputs are stills. |
| Per-composition frames and time bases: After Effects, Clip Studio timelines, and today's canvas-relative offsets and effects. | Canvas described as a view boundary; frame unowned. | Composition owns frame, units, physical resolution and color; outputs frame it. | Now: it decides where today's fields live. |
| Units that survive rescaling: Image Size rescales only parameters labelled `px`. | Units carried by display strings. | Parameters declare dimensions in composition units. | Now. |
| Mixed color: Krita per-layer profiles, InDesign RGB with CMYK, the roadmap's per-layer linear blending. | Color document-global; tiles must match it. | Color on compositions and resources; delivery intent on outputs. | Now. |
| Channels beyond RGBA: Substance channels, spot channels, OpenEXR, OpenToonz CM32. | Fixed plane vocabulary at the container boundary. | Resource types own layouts; retain only current layouts initially. | Type boundary now; additional layouts with their features. |
| Duplicating expressions, rigged content or shader inputs. | Visible dependency lists can coexist with unrewritable IDs in binary/code payloads. | Resource-local slots bound by visible references; clone rules remain type-owned. | Now, before opaque formats embed Capy IDs. |
| Grouping and copying animated or procedural artwork. | A full hierarchy path breaks on regrouping; a seed derived from identity changes on paste. | Paths cross instance boundaries only; seeds are authored values separate from IDs. | Target and identity rules now; specific bindings later. |
| Pose, palette and motion libraries. | Mandatory canvas/default output for content that is useful only when applied elsewhere. | Empty output inventory is valid at the envelope level; composition domains are type-owned. | Cardinality now; library types later. |
| Multi-file projects and libraries: Clip Studio page files, InDesign books, Harmony and OpenToonz projects, Sketch's cached library symbols. | "One file" could be read as "one project". | Locations in the resource table; external entries with expected hashes and cached copies later. | The indirection now; links later. |
| Large, frequent saves: Substance's fragmentation and Save and Compact; Procreate Dreams advertises no save times. | One pack; pack internals implicitly strict. | Several packs; readers tolerate unreferenced pack ranges; folder-safe member names. | Reader rules now; incremental saving later. |
| Non-seekable outputs: Android provider streams may be pipes; Java rejects descriptors on STORED members. | ZIP options unspecified. | No descriptors; precompute member sizes and CRCs, spooling new payloads when necessary. Combine known block CRCs in physical byte order. | Now. |
| Content Credentials: C2PA's ZIP embedding; Photoshop attaches credentials at export. | A strict reader would reject the signature member or its mandated zero CRC. | Reserve `META-INF/` and define the exact signature-member checksum exception; validate interoperability before signing support. | Reader rules now; signing later. |

Sources not linked in section 4:
[Krita KRA source](https://github.com/KDE/krita/tree/master/plugins/impex/libkra),
[InDesign color settings](https://community.adobe.com/t5/indesign-discussions/how-to-set-default-color-mode-in-indesign/m-p/9206530),
[Illustrator global swatches](https://helpx.adobe.com/illustrator/how-to/global-color-swatch.html),
[Procreate Dreams announcement](https://www.malaymail.com/news/money/mediaoutreach/2023/09/09/procreate-announces-its-revolutionary-new-ipad-app-procreate-dreams-featuring-groundbreaking-new-animation-tools-made-for-everyone/241537),
[Android `ContentResolver`](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/main/core/java/android/content/ContentResolver.java),
[Photoshop Content Credentials](https://www.adobe.com/learn/photoshop/web/apply-content-credentials-photoshop),
[Moho switch layers](https://www.lostmarble.com/moho/manual/switch_layers.html).

### Workflows that test the abstraction boundaries

The following workflows were not established by the original five stress tests.
They distinguish a missing foundation rule from a feature that simply needs its
own schema. None requires implementing its evaluator in the format rewrite.

| Concrete workflow and evidence | Adversarial test | Assessment |
| --- | --- | --- |
| Use one animated text layer as the matte for several layers while parenting it to a different layer. [After Effects track mattes](https://helpx.adobe.com/after-effects/desktop/work-with-transparency-and-compositing/work-with-track-mattes-and-traveling-mattes/track-mattes-and-traveling-mattes.html) supports nonadjacent, shared alpha/luma mattes and separate parenting. | Reorder the stack and hide the matte's direct image. Does the matte still evaluate, and does transform inheritance stay independent of stacking? | **Clarify the relationship boundary now.** One generic `parent` must not mean ownership, transform inheritance and composition order. Typed input references suffice; future matte schemas specify channel, space and evaluation stage. Today's single owned mask need not become an arbitrary mask graph. |
| Color a drawing using invisible boundaries and editable fill hints. [Krita Colorize Mask](https://docs.krita.org/en/reference_manual/tools/colorize_mask.html), [Illustrator Live Paint](https://helpx.adobe.com/illustrator/desktop/paint-and-fill/learn-painting-basics/about-live-paint.html) and its [unpainted gap-closing paths](https://helpx.adobe.com/in/illustrator/desktop/paint-and-fill/learn-painting-basics/find-and-close-gaps-in-live-paint-groups.html) preserve different forms of this intent. | Edit an unpainted boundary or hint, then regenerate. A flattened fill or visible-path-only model has lost the input. | **Sufficient with typed content.** Persist authored construction geometry, hints and face/edge paint assignments even when they emit no pixels. Region correspondence and split/merge behavior belong to a future fill type; no universal region IDs or tool-event log now. A control called a mask need not produce coverage. |
| Draw strokes through a 3D scene and switch between drawing order and depth order. [Grease Pencil depth ordering](https://docs.blender.org/manual/en/3.4/grease_pencil/properties/strokes.html) explicitly distinguishes them. | Move a camera so a stroke crosses in front of and behind a mesh. Flattening each object to RGBA before the scene sees it cannot preserve occlusion. | **Sufficient if composition interfaces remain typed.** Geometry, depth and material inputs may stay inside a future scene composition until projection. Do not make every composition boundary an RGBA image. A 3D scene can still expose an ordinary image to today's stack. |
| Paint across UV tiles with different resolutions. [Substance UV Tiles](https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/uv-tiles/uv-tiles) keeps several textures in one set and supports per-tile resolution. | Change one UV tile's resolution without moving its surface coordinates or resizing its neighbors. | **Simplify the baseline.** UV address, raster dimensions and storage-tile address are separate. A future texture-set type references several rasters and the mesh/UV mapping; no document-global dimensions or arbitrary channel language is needed now. |
| Composite render passes, stereo views and deep images. [OpenEXR](https://openexr.com/en/latest/TechnicalIntroduction.html) supports different data windows, sampling rates and variable sample counts per pixel. | Treat every image as equal-sized dense planes with one color/alpha interpretation. Depth, IDs and deep samples no longer fit. | **Sufficient through new resource types.** Keep generic resource transport separate from today's fixed raster layout. Define depth/ID interpretation, view relationships and deep compositing with their feature, not as speculative baseline fields. |
| Deliver illustration to spot-ink printing with overprint. [Illustrator overprinting](https://helpx.adobe.com/ca/illustrator/using/overprinting.html) distinguishes overprint from knockout and provides a simulated preview. | Two artworks look alike in RGB but require different ink plates. A color profile plus extra unnamed channels cannot reconstruct the plates. | **Sufficient, but channels alone are not the feature.** A future print type must preserve named inks, tint/coverage and overprint/knockout semantics; outputs define separation intent. The RGB preview is not the print master. No ink schema now. |
| Reflow one story through several page frames and keep overset text. [InDesign threading](https://helpx.adobe.com/indesign/desktop/add-and-manage-text/add-and-import-text/thread-text-frames.html) retains text when frames are unthreaded. | Remove or resize a frame, change language/font and reopen. Storing only per-frame visible glyphs loses text and flow order. | **Sufficient with content and occurrence separation when text arrives.** The story owns text and styles; frames reference it and have a flow relationship separate from page order. Layout/glyph runs are derived; font identity and shaping remain part of the text feature's contract. |
| Share poses or motion assets without a finished picture. [Blender pose libraries](https://docs.blender.org/manual/en/3.6/animation/armatures/posing/editing/pose_library.html) keep reusable actions with optional character context for previews. | Remove the demonstration character: the pose data is still useful but has no independent image output. | **Small envelope correction now.** Permit no outputs/default and keep domain requirements in types. A library can add demonstration outputs, but source retention must not require one. No library or brush-package feature is being added now. |
| Copy a rig, expression or filter with its inputs; rename and regroup it. [After Effects expression errors](https://helpx.adobe.com/after-effects/desktop/work-with-expressions/edit-expressions/troubleshooting-expressions.html) documents failures from name changes and precomposition. | Updating a manifest dependency list does not repair an ID or name embedded inside program bytes. | **Reference rule now.** Bind local program/resource inputs in the manifest. Copying remaps those bindings, not opaque code. Reparenting inside one scope does not change instance paths; moving across scopes needs a defined retargeting operation. |
| Continue organizing a project when source footage is missing. [After Effects footage management](https://helpx.adobe.com/after-effects/desktop/work-with-footage-items/manage-footage-items/footage-items.html) preserves effects and placements around missing footage. | The authoring structure is known but the complete image cannot currently be rendered. Equating renderability with source validity prevents useful future offline work. | **Preservation and evaluation are different.** Keep today's conservative read-only rule for unknown semantics. A future known linked-media type can define editable missing-media state without weakening validation or substituting its proxy as source. The baseline source-save contract must already permit absent previews. |

### Why the accepted corrections matter now

**Saving must not depend on successful evaluation.** The current
[`Project::write`](../../crates/layer-core/src/project.rs) and
[`project_storage::write`](../../crates/layer-core/src/project_storage.rs) wait
for captured backing and encode source; they do not request a composite render.
Requiring a freshly rendered PNG would add a new failure condition after source
capture. A device loss, unavailable evaluator or expensive simulation can block
a poster even when all authored bytes are safely writable. The accepted rule is
current preview or absence, with the source-save result stated accurately. It
does not promise recovery of uncaptured GPU pixels. This decision is necessary
in the first reader's validity rules; it is not a deferred animation optimization.

**A dependency list is not a relocation mechanism.** Consider code that embeds
an object's UUID and also lists that UUID in JSON. Independent duplication must
change the authored identity. Remapping only JSON now leaves the code pointing
at the original object; rewriting code requires understanding its language and
can invalidate integrity checks. Local slots with manifest bindings remove that
second representation. Today's filter applications already bind parameter values
to definitions, so defining this boundary does not require a general expression
engine. Foreign formats keep their internal address spaces behind the boundary.

**Generic transport should not become a speculative generic pixel model.**
The current [`PixelDescriptor`](../../crates/layer-core/src/color.rs) has a small
set of supported layouts. A versioned wire equivalent, independent of the Rust
struct, preserves them without a registry of arbitrary channel semantics.
The reviewed workflows require more than additional color planes; they are
better served by new resource types. Keep current layout validators local to
that type so adding a new encoding does not entail rewriting the container.

The implementation boundary is therefore small: define transport, object and
reference envelopes, current wire types, output cardinality, and source/preview
publication separately. Leave animation binding, text layout, region topology,
matte graphs, 3D depth composition, inks and texture-set semantics to actual
features. A generic object table permits these additions; it does not specify
or prove their behavior.

### Other design choices

| Proposal | Decision |
| --- | --- |
| SQLite as the package, as in Clip Studio Paint | Not preferred here: ZIP directly exposes independently readable members and compressed blocks. Consistent database snapshots can be valid exchange files; performance and sanitization depend on workload and publication policy. SQLite remains a candidate working store. |
| Incremental or append saves now | Deferred. Only reader tolerance is decided now. ZipDiff shows how stale ZIP structures confuse parsers, and retained deleted bytes are a privacy hazard. |
| Editing around unknown types now, as Rive's runtime does | Deferred. Visible references supply what a future reader needs; the first reader uses preview mode. |
| A literal-or-reference union in every property, as Figma stores a variable beside a fallback color | Rejected. The fallback literal is a second source of truth that an older editor would change without the binding. Binding records keep fields literal. |
| Preserving unknown fields inside known records while editing | Deferred because preserving bytes does not establish edit safety. Open XML preprocessing illustrates possible loss on save, not a universal failure of unknown-field preservation. |
| A mandatory full-resolution merged image, like Krita's `mergedimage.png` | Rejected as mandatory, since Krita's KRZ omits it to save space. Allowed as an optional saved representation. |
| Fractional order keys on disk, as [Figma](https://www.figma.com/blog/realtime-editing-of-ordered-sequences/) and [Excalidraw](https://plus.excalidraw.com/docs/api/scene-content-schema) use for sync | Not required. A stack stores ordered occurrence references once; synchronization is a separate feature. |
| A unified authored graph with layer and node views | Selected direction with structured stacks first. The [implementation plan](../development/capy-format.md) requires semantic prototypes and runtime qualification before schema freeze. |
| Opening `.capy` files in operating-system archive tools | Not promised; their ZIP64 support is unreliable. |

### Capy Canvas's own planned features

| Planned feature | Where it fits |
| --- | --- |
| [Document palettes](color-palettes-research.md) and [document color samplers](../development/photo-editing-m5-m6.md) | Palette and sampler objects; swatches become bindable later. |
| [Color Lookup with imported `.cube` files](../development/photo-editing-m5-m6-execution.md) | Resources referenced by effect applications, kept after the source file is deleted. |
| [Per-layer linear blending](../development/photo-editing-roadmap.md), Blend If, mask density and feather ([research](photo-editing-research.md)) | Occurrence fields omitted at their defaults; a per-layer blend space overrides the composition's. |
| [Layer comps and tags](layers-research.md) | Comps are outputs with a context; tags are fields or copy-safe ancillary records. |
| [Persisted history snapshots](photo-editing-research.md) | Explicitly retained snapshot objects outside the stack, with their own budget; ordinary undo history remains session state. |
| [Vector strokes](vector-layers-research.md) with binary samples, embedded brushes and sub-path IDs | A typed content object with binary resources; sub-path IDs are local and addressed by path. Cross-record inputs use manifest bindings. Whether to keep an outline fallback is that feature's decision; the foundation defines only the portable output-preview convention. |
| [Paper surface and dry-media material](dry-media-brush-design.md) | Paint-material fields. |

### Not verified

Adobe help pages for After Effects Essential Properties and Photoshop data sets
were read only through search excerpts. The internals of Procreate Dreams,
TVPaint, Affinity and Substance project files are unpublished. Whether C2PA's
central-directory hash covers the ZIP64 end records is ambiguous in the
specification text. Blender's pose-library and depth-order descriptions were
available through indexed official manual text; direct English manual fetches
failed. No prototype or measurement tests the recommendations; the evidence
list in section 4 still applies. In particular, source saving without a preview,
generic reference remapping and future feature schemas are proposed contracts,
not verified behavior of Capy Canvas.
