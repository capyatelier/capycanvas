# A durable foundation for the Capy Canvas file format

[Design history](README.md)

Research date: 2026-10-02. Code baseline: `2e574cb20`. This is a design
recommendation, not an adopted format specification or an implementation plan
for the future features discussed below. Current-format findings come from the
reader, writer, model types and a checked-in file, rather than earlier plans.
External sources are primary specifications, developer documentation and product
manuals. Tradeoffs and recommendations are our analysis of those sources.

**Recommendation:** keep `.capy` as one self-contained artwork file, using a
restricted ZIP container with ZIP64 support, a small JSON manifest, independently
versioned artwork types, binary resources and a mandatory portable preview.
Keep today's sparse, lossless tile storage and reuse of compressed data. Replace
serialization of the runtime `Document` with an explicit file model. Separate
reusable content, placed instances, evaluation relationships and output views.
Describe today's layer stack as one composition type, so later timelines and
graphs can compose with existing content without changing the package foundation.

The lasting commitment is **new readers preserve the meaning of old files**.
Old readers cannot edit arbitrary future features correctly. They should show a
saved representation, preserve the source, and avoid silently saving a damaged
document. No container or capability list can replace the ongoing work of
maintaining old semantics and testing them. The five feature stress tests in
section 3 refine the initial proposal; section 4 incorporates their conclusions.

Read the [current-format audit](#1-current-format-and-problems-to-correct),
[external research](#2-what-other-formats-teach-us),
[future feature analysis](#3-future-features-and-their-implications) and
[final recommendation](#4-minimal-recommended-foundation).
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

The checked-in
[`published-v14-choice.capy`](../../crates/layer-core/tests/fixtures/published-v14-choice.capy)
confirms the header and manifest shape, including active editing state and ID
allocators. This inspection establishes the representation, not a performance
result or qualification of every host's save workflow.

| Area | Persisted today |
| --- | --- |
| Document | Identity, dimensions, color space/depth, blend space, print resolution, SDR rendition, proof recipe and descriptive photo metadata. |
| Composition | Ordered flat layer array with parent references; paint, paper, group, effect and selection kinds; visibility, opacity, blend, clipping, transforms, extents and masks. |
| Paint | Committed sparse color and coverage tiles; wetness planes and live watercolor edge settings. No saved stroke replay or undo history. |
| Imported images | Tiled source samples, independent interpretation/profile, resolution and Original/Rasterized role; painted tiles override source regions. These are retained decoded samples, not necessarily the original imported file bytes. |
| Effects | Embedded resolved WGSL, ABI, entry points, passes, sampling declarations, parameters, values and supporting definition metadata. |
| Editing aids | Rulers, reference-layer flags, saved selections, current selection, layer locks and alpha locks. |
| Runtime/editor details | Document revision, next layer/stroke IDs, active layer/mask, mask inspection flag, and selection display settings. |

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
   active targets and inspection state in local session persistence. An optional
   transferable editing-state attachment can preserve convenience without making
   it a condition of opening artwork.

4. **The outer format assumes the current layer implementation.** Every layer
   and mask needs a raster target record, even kinds without raster content.
   Sources bind specifically to `LayerId`; selections have `Current`, `Layer`
   and `Mask` target variants; paper must occupy the last array position. These
   can be valid rules of today's layer-stack type, but should not be rules of
   the container or of every future document.

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
   hints. Version the durable evaluation contract independently, use stable
   parameter identities in files, and keep UI labels from determining whether an
   effect can be evaluated. Replacing a built-in implementation must not silently
   change a saved picture.

8. **Storage conventions need a written, stable meaning.** The tile digest
   currently incorporates `serde_json::to_vec(descriptor)`: changing serialization
   spelling or omission rules can change identity without changing pixels.
   Fixed tiles and packed selection words are not inherently wrong, but belong
   to a named storage encoding, independent of GPU pages and in-memory packing.
   Hash inputs, endianness, padding, channel order and missing-tile behavior must
   be defined independently of Rust serialization details.

9. **Application limits and format limits are entangled.** Default admission
   limits include 64 MiB metadata, 512 MiB source ownership, 1 GiB raster data,
   16,384 tile instances, 32,768 pixels per axis and 4,096 layers. They are useful
   defenses, not a suitable permanent ceiling for animation and video. Distinguish
   structural validity from a particular device's editable working set. Do not
   remove resource limits to claim extensibility.

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
can move to optional editing state; saved selection objects and mask coverage
must remain portable.

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
| Cropped canvas, negative placement or sparse empty area | Canvas size is a view/output boundary; it does not define the full stored content extent. Absence must have type-defined meaning. |
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

**Retain authored work that is not currently played.** Unassigned cels, unused
takes, disabled tracks, off-range keys and alternate poses are still artwork.
The root's authored ownership must include them; tracing references only from
rendered outputs would lose them. Deleting an exposure is not deleting its cel.
Only data unowned by all authored records, with no preserved unknown attachment
depending on it, is eligible for removal. This animation audit strengthens the
root/outputs distinction in the recommendation without requiring a new container
section for every kind of unused work.

**Separate persistence from rendering policy.** Onion skins, the playhead,
temporary solo controls, the light table and scrubbing caches are editor state.
Authored visibility, sound muting and output ranges affect the work and persist.
Real-time preview may skip frames according to product policy; final export must
evaluate every requested output sample. Saving should capture one consistent
authored revision without rendering a full film. A mandatory poster is bounded;
expensive playback proxies are optional, versioned by their source snapshot and
range, and must not block ordinary source saves indefinitely.

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
supplies timing/binding semantics through capabilities. Adding an empty timeline
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
still requires only the default preview; future multi-output capabilities should
define their own preview coverage promises.

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

## 4. Minimal recommended foundation

### Choose a restricted standard container

| Candidate | Advantages | Costs and conclusion |
| --- | --- | --- |
| Evolve the current custom container | Reuses the compact index, streaming writer, tile checks and compressed backing. A generic resource directory could make it extensible. | We own every inspection/recovery tool and directory rule. A sound alternative if measured ZIP integration costs prove material; capabilities do not inherently require ZIP. |
| ZIP with ZIP64 support | Standard directory and offsets, independent resources, ordinary inspection tools, preview extraction and a single transferable file. | Central directory is normally read from the end; safe updates generally rewrite the package. Restrict the ZIP feature set and pack tiny blocks. **Recommended default.** |
| SQLite | Transactions, partial updates, indexes and a mature application-file story. [SQLite application files](https://www.sqlite.org/appfileformat.html). | A database runtime, page/journal behavior and host-specific storage integration are more than the current transport needs. Attractive for a working/recovery store if incremental saves become essential; not the initial interchange package. SQLite archives also have overhead relative to ZIP. [SQLite archives](https://www.sqlite.org/sqlar.html). |
| Directory bundle, tar or whole-file compression | A directory is convenient for development; sequential containers are easy to stream. | A directory is awkward to transfer through mobile providers; tar needs another index for efficient random access; whole-file compression couples unrelated media. None is the default user-facing file. |

ZIP64 must be supported from the first reader, including large entry counts and
large offsets; this is support for ZIP's large-file records, not a demand to use
them in every tiny archive. The
[PKWARE specification](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)
defines the transport. Use a maintained implementation and a deliberately small
accepted subset. Initially use **STORED archive members**, with lossless LZ4
inside raster packs and existing compression inside PNG/video/etc. This avoids
double compression and preserves seekability within a pack. The tradeoff is
uncompressed JSON and text resources; measure that cost before adding another
mandatory codec.

An illustrative package, not final member names or a byte-level specification:

```text
artwork.capy
  mimetype             package identity, first and small
  manifest.json        schema, capabilities, objects, resources and output inventory
  preview.png          portable saved view of the default output
  data/tiles.bin       indexed independent LZ4 blocks, when raster data exists
  data/<resource-id>   profiles, metadata, shader definitions and other assets
```

Use one tile pack initially; allow resource references to name a pack and range
so later partitioning does not change artwork identity. The ZIP directory locates
members; the tile index locates blocks *inside* a member. Do not also duplicate
ZIP's absolute file offsets in JSON. Avoid one member per tile, stroke, vertex or
animation sample. A future type can put large arrays or time indexes in a binary
resource without extending the container grammar.

ZIP makes no promise of zero-copy access through every host. A non-seekable
provider may need a bounded stream to private disk storage before random access;
the web host needs a suitable Blob/range or private-storage path. Saving must
write a complete replacement off the input thread and publish it through the
host's supported mechanism. Reuse compressed blocks, but acknowledge that a ZIP
rewrite still copies bytes. Large-video incremental editing may eventually need
a separate working store or explicit linked media.

### Keep the file model small and independent

The revised durable model needs five concepts:

1. **Document envelope:** format identity/version, an authored root reference,
   capability declarations and portable metadata. The root describes the work's
   organization and retained authored content, including work not used by any
   current output; today it is a layer stack, including hidden layers.
2. **Typed objects with stable IDs:** each has a namespaced type/version and
   type-owned data/references. Distinguish editable content, placed instances and
   effect applications; an instance's transform/mask is not a property of its
   shared source. Paint, images, groups, paper, masks, effects and saved coverage
   retain their existing meanings. Object identity is independent of array
   position, display name, runtime allocator and content hash. Type schemas own
   the stable keys for addressable properties and the meaning of relationships.
   Later animation bindings refer to these identities without making every
   baseline field a predesigned animated-value union.
3. **Resources:** binary or textual payloads addressed through one mechanism,
   with declared encoding, length and integrity information. Multiple objects
   can reference the same immutable content. Pixel interpretation belongs to the
   image/plane description, not to a guessed filename or GPU texture format.
4. **Identified outputs:** a small inventory names renderable views and a default
   output, independently of layer/page order. Each refers to its source and
   authored view/settings; today there is one canvas output. Later types can
   express page framing, camera, time or delivery intent without adding those
   fields to every document. Settings have one authoritative owner and are
   referenced where shared, not copied into conflicting document/output records.
5. **Saved representations:** a baseline preview and, where useful, explicitly
   scoped richer fallbacks. Source data remains authoritative unless the artist
   explicitly converts it. Each representation identifies its output or object
   scope, applicable context and source snapshot.

Store ordered children in the layer-stack type, not both child lists and an
independent authoritative parent/order table. Distinguish composition membership
from instancing, resource references and evaluation inputs. Known types validate
their own relationship/cycle rules; do not assume all references form one tree
or one executable DAG. A future graph can contain ports and connections; a
timeline can reference compositions and media; a page collection can reference
multiple compositions. These can nest rather than being mutually exclusive root
modes. Existing objects need not become fictitious layers in these models.

Effect applications refer to definitions and explicit inputs. A type's evaluation
contract defines ordering, coordinate/color space, mask/coverage behavior and any
backdrop dependency. Keep those semantics in versioned feature schemas; do not
standardize an all-purpose list of modifier stages in the envelope. The first
implementation supports today's effect layers and embedded filter programs;
layer FX and other application types arrive with their features.

Use dedicated wire types in shared Rust, with validated conversion to runtime
types. Keep semantic rules and capability evaluation shared across hosts. JSON
is adequate for the small structural description; avoid large numeric/byte arrays,
base64 media, serialized pointers and runtime enum ordinals. IDs should be opaque
strings. Define lossless handling of large integers so JavaScript tooling cannot
round offsets or future time values above its exact-integer range. Binary offsets
remain 64-bit; JSON representations must not rely on an imprecise number parser.

Freeze the meaning of coordinate systems, child order, transforms, interpolation,
blend spaces, blend formulas, clipping and pass-through behavior for the baseline
type. Keep existing projective/mesh placement as semantic geometry, not a snapshot
of GPU control buffers. This is the unavoidable specification work for today's
features; generic extensibility is not a substitute for defining them.

### Should the file already be a general node graph?

**Recommendation: adopt a graph-capable object/reference model now, but do not
make a general executable node graph the mandatory serialized artwork model yet.**
A layer UI is fully compatible with a node-based engine. The reason to defer a
universal evaluation schema is the semantic commitment, not the interface.

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
  parameter bindings, outputs and defined evaluation semantics. This can become
  a versioned composition type when it is a designed feature.
- The **runtime execution graph** schedules tiles, passes, cache reuse and GPU
  work. It is derived and can change with optimizations; it should not be saved
  as the artwork's durable meaning.

| Choice | Why choose it | Principal cost |
| --- | --- | --- |
| A single layer tree as the entire file model | Smallest mapping from the current runtime. | Repeats the current coupling and makes reuse, nested timelines and scene relationships awkward. Reject. |
| A general evaluation graph now, with layers as its UI projection | Gives sharing, fan-out and arbitrary composition one authored representation from the beginning. Attractive if the engine is also being redesigned around it now. | Requires permanent port, value, scope, time and evaluation semantics before the relevant features are designed; preserving a layer projection is additional product work. |
| Typed artwork/reference graph, with today's stack as one composition type | Separates durable identity/resources from current composition semantics and permits graph types later. | A future graph evaluator must still interpret or losslessly translate the old stack type. **Recommended for the current scope.** |

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

It is possible to define that graph correctly now. The strongest reason to do
so would be a committed near-term node engine: the wire schema could then be
tested against real evaluation, sharing, input validation and layer operations.
It is not necessary merely to allow future graphs. A future `composition-graph/1`
object can use the same IDs, resources, outputs and capability mechanism, and
nest a `layer-stack/1` object as a semantic operation. Alternatively, proven
conversion can expand old stacks into graph operations. Keep the old meaning
supported either way; do not save both a layer tree and a graph as competing
authorities. Node editor positions are optional presentation state.

The five stress tests specifically support this middle choice. FX require
evaluation contracts, Smart Objects require instancing, animation requires time
and stable property targets, rigs require scoped solver rules, and pages require
output views. None forces a universal socket/value system into the envelope.
Simply adding generic nodes would not solve any of those semantic requirements.

Before choosing the full-graph alternative, require one small executable
demonstration of current masks, clipping, isolated/pass-through groups and live
adjustments; shared content evaluated in two contexts; and a layer reorder that
updates the graph without losing authored data. Validate pixel equivalence and
bounded incremental rendering. This is an acceptance condition for that larger
design decision, not extra implementation proposed by this report.

### Capabilities and versioning

Use **two levels of versioning**, not a minimum application release as the primary
compatibility mechanism:

- A rarely changed envelope version defines how to locate and parse the package.
- Namespaced capability/type versions define artwork interpretation. Illustrative
  names are `capy.layer-stack/1`, `capy.raster-tiles/1`, `capy.paint-material/1` and
  `capy.effect-wgsl/1`. The actual registry and spelling can be decided later.

Include `used` and `required` inventories, with `required` a subset of `used`.
Types imply capabilities; the inventories are a derived preflight summary that
validation checks against the contents, not a second hand-maintained truth.
Declare only features the file actually contains. A plain raster drawing saved
by a future animation release should not require animation support. Compute the
closure of requirements across nested content, effect definitions and resource
encodings: a supported 3D object does not imply a supported rig or material.
Readers verify the dependencies of understood types and treat unknown required
types conservatively; they cannot validate semantics inside an opaque future type.

For Capy's initial policy, **required means necessary for faithful native
interpretation and safe editing**, including hidden authored content. This is
deliberately stricter than only asking whether a flattened appearance can be
displayed. Capabilities for authoring data must remain required even when a
preview exists. Optional capabilities are genuinely ancillary, such as disposable
view settings; they must not secretly affect rendering or future edits.

Additive optional fields must have a defined omission meaning and preservation
rule. Any addition that changes artwork interpretation requires a capability;
incompatible semantics require a new version. Keep the known core schema strict
against typos, but provide explicit extension locations that retain opaque data.
Do not globally turn off validation to accept future files.

After the compatibility reset, new code must continue to understand the baseline
and every subsequently supported capability version, directly or through
lossless conversion. A converter that changes appearance or removes editability
is not backward compatibility. Changed shader helpers, brush evaluators or blend
formulas require compatibility implementations or demonstrably equivalent
translation. A preview protects access to appearance when evaluation is
unavailable; it does not satisfy the full old-file editing commitment.

This does **not** require maintaining today's pre-reset v14 reader: that can be
removed at the authorized compatibility break. The maintenance commitment begins
with the new baseline. Do not promise lossless writing to arbitrary older
versions; writing the lowest capability set that actually represents a document
is useful, while lossy conversion should be explicit.

### Clean degradation without silent loss

| What the reader encounters | Initial behavior |
| --- | --- |
| Supported envelope and all required capabilities | Validate and open for native editing. |
| Unknown optional, independent attachment | Preserve its raw payload and attachment identity; ignore its presentation. |
| Unknown required type, evaluation version or codec | List identified outputs and show their available saved representations; retain the source, with native editing disabled. |
| Known format exceeding the device's resources | Offer the same preview path; distinguish resource limits from invalid artwork. |
| Unsupported envelope | Try only the fixed baseline preview convention within a safely parsed package; otherwise give a clear unsupported-format result. |
| Corrupt native content | Refuse normal editing; show a verified preview only as a recovered view. |

The minimum safe first policy is **whole-document preview mode when a required
capability is unsupported**. Do not build a speculative partial-edit dependency
system now. It should allow exporting the available representation to a new file
and keeping/copying the original package, without overwriting the source with
the fallback.

Opaque preservation must include payloads, IDs and every resource they may refer
to. A no-edit copy can preserve the original package byte-for-byte. Unknown
attachments may survive edits only under an explicit contract that makes them
independent of those edits. Otherwise keep the document read-only; do not garbage
collect resources merely because the old reader cannot see their references.
Generic retention is not proof of semantic validity after mutation.

Later, partial editing can be introduced for well-defined isolated scopes.
A vector object may have a raster fallback; an unsupported effect that reads
its backdrop may require a fallback for an entire composition. Unknown blend
semantics cannot be replaced by Normal. Future fallbacks must identify scope,
resolution, bounds, color/alpha interpretation and, for animation, time/range.
Their validity is tied to the exact source snapshot and dependencies. Never
silently keep a stale fallback after changing an input.

Require one bounded, standard sRGB PNG preview of the default output at an
explicit representative time when relevant. For HDR, use the authored SDR
rendition and identify the preview as SDR. It should contain no checkerboard,
selection overlay or editor chrome. Exact size is a later tuning decision;
readers must expose when only reduced resolution or one frame is available.
This small baseline does not promise full-resolution recovery or playback.
Additional full-resolution composites, object fallbacks or animation proxies
are optional resources, avoiding compulsory duplication of every layer/frame.
The output inventory permits discovery of future pages/views even without their
native capability. A missing representation is reported for that output; the
default preview must not imply that it represents the entire multi-output work.
For an unsupported envelope, only the fixed default-preview convention is safe
to rely on, not an unfamiliar output inventory.

Generate the preview from the same committed snapshot as the artwork, using
asynchronous GPU work and worker encoding. It must not add UI-thread readback or
make manual-save completion refer to a newer/different image. A failed mandatory
preview generation should leave the previous manual save intact; private recovery
can still retain source snapshots without claiming to be a complete published
package.

### Retain efficient raster storage, normalize its contract

Keep 256-pixel tiles and LZ4 as the first raster encoding. That is a deliberately
small baseline, not a rule that all future content has 256-pixel tiles. A later
encoding can change tile sizes, codecs or layouts through a capability without
changing the resource mechanism. Do not add several mandatory codecs now.

Retain U8/U16/F16/F32, source profiles and HDR interpretation. Specify channel
order, byte order, transfer function, alpha association, finite-value rules,
coverage and default values. Prefer one portable straight-color representation
for ordinary saved paint; host attachment formats should not create alternate
artwork meanings. A tile's default is context-dependent: absent paint may be
transparent, an absent override may reveal source pixels, and an absent mask
tile may mean the mask's declared coverage. Preserve those distinctions.

Use a stable hash preimage for tile identity instead of incidental Serde output.
Keep integrity checking per independently accessed block; a whole-file checksum
alone would require scanning huge media to verify one frame. ZIP CRCs help detect
transport corruption; hashes used for deduplication and verification do not
authenticate an untrusted file. Do not confuse resource hashes with authored
object IDs or require rewriting references after every edit.

Named selection coverage can share the same resource system. Its on-disk packing
must be a defined coverage encoding, not whatever word packing the runtime happens
to use. Preserve contours where they are authoritative and all non-raster mask
semantics rather than baking them merely to simplify storage.

### Admission, publication and the evidence needed before landing

Use canonical relative member names and reject duplicates, ambiguous path
spellings, traversal, symlinks, encrypted/multipart archives and unsupported ZIP
methods. Read members through an archive abstraction rather than extracting
arbitrary paths. Check the central directory against local headers, ranges and
lengths; bound metadata, nesting, counts and all decoded resources. Unknown
payloads still need bounded retention and transport-integrity checks. Do not
execute code, access the network or compile every shader merely to show a preview.

Save a consistent snapshot to a new package; validate completion before durable
publication. Keep recovery, autosave and temporary working storage separate from
the exchange contract. Atomic replacement guarantees depend on the host/provider;
the container must not claim guarantees a mobile provider or browser download
does not offer.

No runtime change or performance measurement accompanies this report. Before
committing to the format implementation, require evidence for:

1. **Today's artwork round-trips:** all existing layer/group/clipping modes,
   linked/unlinked masks, transforms/meshes, out-of-canvas data, saved selections,
   sources/profiles, HDR, proof/rendition, material state and embedded effects.
   Compare semantic data and rendered output, not just successful parsing.
2. **Compatibility and preservation:** a permanent baseline fixture corpus;
   old capability versions opened by new code; synthetic unknown objects and
   attachments; unsupported shader/codec; no-edit copying and no silent lossy
   overwrite. Test missing dependencies and stale previews explicitly.
3. **Transport failures:** truncated or conflicting directories, duplicate
   IDs/member names, corrupt blocks, overflow, oversized decode claims, failed
   writes, cancelled saves and provider failure during publication.
4. **Measured cost:** file size, save/open time, peak RAM, preview cost, unchanged
   save/recompression behavior, large resource access and animation-shaped
   resource counts. Compare ZIP against the existing container on representative
   artwork. Measure any affected frame paths under the
   [performance rules](../performance/measuring.md); do not infer tier compliance
   from a container choice.
5. **Host journeys:** save/open/save-as, recovery, continued painting during save
   and preview-only opening on GTK, Web, Android, Apple and Windows. Exercise
   provider streams and large offsets, not only local seekable files, with the
   [checks appropriate to the implementation](../development/testing.md).

The first format should contain today's artwork types with explicit content and
occurrence identities, versioning, resource references, a one-entry output
inventory, extension preservation and preview behavior. Leave timeline schemas,
vector geometry, 3D scenes, rigs, a universal node evaluation schema, incremental
archive updates and collaboration protocols to their respective features. The
foundation is successful when adding one of those types no longer requires
replacing the container or reinterpreting the meaning of existing artwork.
