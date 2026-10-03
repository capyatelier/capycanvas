# Capy package contract

[Technical documentation](../README.md)

This is the target contract for the [format cutover](../development/capy-format.md).
It fixes M1's package, resource and capture boundaries. The running application
still uses the pre-release format described in [project format](project-format.md)
until M3 replaces it. This contract is not evidence of an implemented codec,
qualified transport, host journey or performance result.

## Envelope and references

The package identity is `application/x-capy-canvas`. Its UTF-8 `manifest.json`
contains one JSON object with these fields:

| Field | Meaning |
| --- | --- |
| `format` | Required literal `capy.canvas`. |
| `version` | Required integer `1`; versions describe the envelope, not the app. |
| `document` | Required portable document ID. |
| `root` | Required object reference to the authored root. |
| `objects` | Required array of object records; table order has no meaning. |
| `resources` | Required array of resource records. |
| `outputs` | Required array of output-object references; may be empty. |
| `default_output` | Required exactly when `outputs` is nonempty; names one listed output. |
| `metadata` | Optional photo metadata bindings `exif`, `xmp`, `iptc`, each a resource reference. Omit when empty. |

A portable ID is exactly 32 lowercase ASCII hexadecimal digits encoding a random
128-bit value. The document, every object and every resource have different IDs.
IDs survive editing, save/open, reordering and renaming; positions, names, decoded
hashes, archive offsets and runtime handles are not portable IDs. Independent
copies receive new object/source IDs without changing authored random seeds.
Equal bytes or resource sharing never imply shared editing.

Every cross-object or resource reference has exactly the form
`{"ref":"0123456789abcdef0123456789abcdef"}`. A JSON object containing `ref`
must have that one field and a valid ID. A reference resolves to the proper table
for the known relationship; the document ID is not an object or resource target.
The parser discovers these forms recursively, including inside unknown records.
ID strings elsewhere are literals, never alternate references. Imported arbitrary
JSON belongs in a binary resource when it could contain the reserved form.

An evaluation endpoint is `{"object":{"ref":"…"},"port":"color"}`. Port
keys are stable strings owned by the object type. `color` is a premultiplied color
result in the applicable composition's blend domain; `coverage` is scalar linear
coverage independent of image alpha. Stack `backdrop` and `clip` inputs are scoped
operations, resolved from stack order; they are not separately persisted edges.
Known endpoint schemas require `port`; absence never selects the first port.
An unknown port or semantic value is unsupported, not an instruction to guess.
Binary and code resources use local slot keys with visible bindings in records;
they do not embed Capy IDs that need rewriting on duplication or paste.

All JSON values representing `u64` are canonical decimal strings: `"0"` or a
nonzero digit followed by digits, at most `18446744073709551615`. No sign, leading
zero, fractional or exponent spelling is accepted. The same rule applies to
future signed 64-bit fields, with a leading minus only for negative values and
the signed range enforced by that field. Counts/dimensions explicitly declared
`u32` and schema versions are JSON integers. Floating values are finite JSON
numbers with type-defined ranges; stored pixel floats remain binary. JSON
objects cannot repeat keys, even if their values agree. Reject repeated keys
before interpreting or discarding any record.

Unknown envelope fields or an unknown envelope version prevent interpretation of
the manifest; only the fixed preview convention and original package remain
available. Malformed JSON, IDs, references, lengths and known required fields are
invalid. Metadata size, nesting, total reference edges and reference-expansion
work are bounded before adoption independently of device pixel admission.

## Object records and defaults

An object is `{"id":"…","type":"capy.stack/1","data":{…}}`. Optional
`ancillary` and `copy_safe` flags default to false. `copy_safe:true` requires
`ancillary:true`; known artwork types reject either flag. Required artwork cannot
depend on ancillary records. Ancillary records may reference artwork/resources,
not other ancillary records. Unknown ancillary data is retained after edits only
when copy-safe and every reference still resolves; otherwise it is dropped on an
edited save. No-edit copying retains the original package. Unknown records are
not cloned onto independently duplicated objects.

The initial registry is:

| Type | Owned data and wire relationship |
| --- | --- |
| `capy.composition/1` | `frame` with required pixel `size`, optional `origin`; `resolution`, `color`, `blend`, and required `result` endpoint. |
| `capy.stack/1` | `entries`, an ordered array of occurrence references, front to back. |
| `capy.occurrence/1` | Required `content`; `name`, `visible`, `opacity`, `blend`, `locked`, `alpha_locked`, `reference`, `clipped`, `placement`, and optional `mask`. |
| `capy.paint-source/1` | Required pixel `domain`; optional `original`, sparse `tiles`, `material`. The original retains its role, extent, interpretation, resolution and tile references independently of overrides. |
| `capy.coverage-source/1` | Required pixel `domain`; `initial`, `default_coverage`, sparse `tiles`. Initial contour/pixel selection remains authoritative where supplied. |
| `capy.effect/1` | Required `definition` reference and pixel `domain`; `values` keyed by parameter keys, `bindings` keyed by resource-local slots, `inputs` keyed by typed input ports. |
| `capy.effect-definition/1` | Required stable program `key`, evaluation `contract`, shader `abi`, `kind`, `code` resource references, `entry` and keyed `parameters`; ordered ABI `slots`, passes, lookups, constraints and presentation declarations. |
| `capy.selection/1` | Required `shape`; placement/inversion and saved display `color`/`opacity`. Pixels use coverage resources; contours keep their geometry. |
| `capy.guides/1` | Authored ruler geometry and reference markings. |
| `capy.output/1` | Required `source` composition endpoint; `name`, `context`, framing, SDR rendition, proof intent and optional `representation`. |

`content` is one of `{"paint":{"ref":"…"}}`,
`{"stack":{"ref":"…"}}`, `{"effect":{"ref":"…"}}`,
`{"selection":{"ref":"…"}}`, or `{"paper":{"color":…}}`.
Exactly one alternative appears. Paper is an authored stack operation; it has no
fake editable raster source. A mask is inline
`{"source":{"ref":"…"},…}` with slot key `mask` fixed by the occurrence
schema, and optional `enabled`, `linked`, `inverted` and `placement` values.
The coverage source has its own paint-target identity.

Writers omit optional fields equal to their frozen wire defaults. Required
fields have no omission default. A changed UI default does not change the wire
meaning. Empty optional lists/maps are omitted; required tables remain present.
The common frozen defaults are:

| Field | Absent meaning |
| --- | --- |
| `name` | Empty string. |
| `visible`, mask `enabled`, mask `linked` | True. |
| `opacity` | One. |
| Stack `entries` | Empty array. |
| `locked`, `alpha_locked`, `reference`, `clipped`, `inverted` | False. |
| Occurrence `blend` | `normal`; group pass-through is explicitly `pass_through`. |
| Composition `blend` | `linear`. |
| Composition `color` | Built-in `srgb`, unsigned `u8`. |
| `origin`, placement translation | `[0,0]` in the owner's declared pixel domain. |
| Retained placement | Identity projective transform, no mesh, bilinear interpolation. |
| Physical `resolution` | Unspecified; it is not silently 72 or 300 pixels/inch. |
| Original role | `original`; the `rasterized` role is explicit. |
| Original `profile_assumed` | False. |
| Paint overrides/material tiles | No override at absent coordinates; an imported base is revealed. |
| Coverage `default_coverage` | One; supplied `initial` coverage takes precedence before painted overrides. |
| Effect parameter value | The definition's frozen keyed default, not the current catalog default. |
| Effect alpha/space/resolution/time | `preserve`, `linear`, `native`, false. |
| Output context | Elapsed coordinate zero; animated effect opening phase zero. |
| Output representation | Unavailable. |

Unknown fields, variants, parameter keys, choice values, ports, resource encodings
or evaluation contracts in a required record make the entire artwork unsupported.
Do not deserialize them away. Malformed data for understood fields is invalid.
Validate every retained object, including unplaced work. A valid graph outside the
editable subset remains preserved: shared editable sources, shared effect
applications or two group occurrences using one stack cannot be normalized by
duplicating or dropping their content. An occurrence in two containing stacks
violates occurrence identity and is invalid. Cycles in
instantaneous evaluation or recursive expansion and dangling references are
invalid; the cycle rule does not apply indiscriminately to ancillary or resource
references. Relationship semantics and field ownership are specified with the
[authored model](authored-model.md).

Parameter keys and choice option values are strings; array positions are never
saved choices. The definition's `slots` fixes positional shader ABI layout
independently of parameter display order. Parameter dimensions use `scalar`,
`angle`, `time`, or `length`; a length additionally names `source_pixels`,
`composition_pixels` or `normalized` reference space. A displayed unit label does
not control resizing. Evaluation contract `capy.filter/1` is distinct from shader
ABI `4`. Unknown future contracts/ABIs remain preserved without compilation.

### Nested values

The following spelling is independent of Rust enum/field serialization. All
objects have closed known field sets; unknown additions follow preservation policy.

| Value | Grammar |
| --- | --- |
| Pixel point/size | Two-element `[x,y]`; point components finite F32, sizes positive U32. |
| Source `domain` | `{"size":[width,height]}`; local origin is zero. Composition frame additionally permits `origin:[x,y]`. |
| Working `color` | `{"space":"srgb","depth":"u8"}` with either default-valued field omitted. Spaces are `srgb`, `display_p3`, `adobe_rgb`, `pro_photo`. |
| Portable color | `{"rgba":[r,g,b,a],"space":"srgb","linear_rgb":[r,g,b]}`; `space` defaults to `srgb`, `linear_rgb` is optional exact authored linear RGB. Alpha belongs only to `rgba`. |
| Physical resolution | `{"unit":"inch","density":[[x_n,x_d],[y_n,y_d]]}`; all values required and positive U32; unit also permits `centimetre`, `metre`. |
| Profile | `{"builtin":"srgb"}` or `{"resource":{"ref":"…"}}`; exactly one alternative. |
| Placement | Optional `translation:[x,y]`, row-major `projective:[m00,m01,m02,m10,m11,m12,m20,m21,m22]`, `mesh`, `interpolation`; maps local geometry, then translates. Interpolation is `nearest`, `linear`, `bicubic`, `lanczos`, default `linear`. |
| Cubic mesh | Required `frame:[a,b,c,d,tx,ty]`, `breakpoints:[[x…],[y…]]`, `net:[[x,y]…]`; affine frame maps the unit square to source pixels, breakpoints retain the current cubic patch partition, net is row-major destination control points. GPU tessellation is absent. |
| Sparse tile entry | `{"coordinate":[x,y],"plane":"color","resource":{"ref":"…"}}`; all fields required, coordinate U32 in source-local 256-pixel tiles, no repeated coordinate/plane pair. |
| Imported `original` | Required `extent`, `interpretation`, `tiles`; optional `role` (`original` or `rasterized`) and `resolution`. Interpretation has required `channels`, `depth`, `profile` and optional `profile_assumed`. Original tile entries have coordinate/resource only. |
| Material | `{"watercolor":{"wet_edge":number,"burnt_edge":number,"edge_width":number}}`; all three settings required when watercolor exists; finite wet/burnt edges in `[0,1]`, edge width in `[1,16]` source pixels. |
| Selection shape | `{"contours":[[[x,y]…]…]}` using even/odd interiors, or `{"pixels":{"extent":[w,h],"bounds":[x0,y0,x1,y1],"depth":"u4","chunks":[{"ref":"…"}…]}}`; exactly one alternative. U8 coverage uses depth `u8`. |
| Selection placement | Optional `affine:[a,b,c,d,tx,ty]` and `inverted`; identity/false when absent. Coverage-source `initial` uses the same shape/placement values inline. |
| Guides | `rulers` array with required stable portable `id` and `geometry`; geometry is `{kind:"straight"|"parallel",start:[x,y],end:[x,y]}` or `{kind:"radial",center:[x,y]}`. Ruler IDs are type-owned local subelement IDs. |
| Output framing | Optional `frame:{origin:[x,y],size:[w,h]}` and `scale:[x,y]`; absence inherits composition frame and scale `[1,1]`. |
| SDR rendition | `sdr` with optional `exposure`, `contrast`, `headroom`, `highlight_color`, `balance`; defaults respectively `0`, `1`, `2.3004484`, `0.3`, `0`. |
| Proof intent | `proof` with required `name`, `profile`, optional `intent`, `black_point_compensation`, `simulate_paper`, `simulate_black_ink`; defaults relative colorimetric, true, false, true. |

Affine `[a,b,c,d,tx,ty]` means `x'=a*x+c*y+tx`, `y'=b*x+d*y+ty`.
Matrices/meshes preserve current invertibility and bounded mapping validation.
A mask placement permits projective/translation only; linked owner mesh mapping
is evaluated by its existing pre-map contract, not duplicated onto the mask.
Saved-selection display color defaults to straight sRGB red `[1,0,0,1]` and
opacity `0.5`, independently of occurrence contribution opacity. Blend and
rendering-intent names use the existing semantic names in lower snake case,
never Rust discriminants or GPU blend codes. Blend names are `normal`,
`multiply`, `screen`, `add`, `overlay`, `soft_light`, `color`, `darken`, `lighten`,
`color_burn`, `linear_burn`, `color_dodge`, `hard_light`, `vivid_light`,
`linear_light`, `pin_light`, `hard_mix`, `difference`, `exclusion`, `subtract`,
`divide`, `hue`, `saturation`, `luminosity`, `pass_through`. Only stack occurrences
admit pass-through. Rendering intents are `perceptual`, `relative_colorimetric`,
`saturation`, `absolute_colorimetric`. Proof paper simulation requires black-ink
simulation; absolute colorimetric intent forbids black-point compensation.

Effect `values` maps stable parameter keys to `{kind,value}` values: `number`
(F32), `toggle` (boolean), `choice` (stable option string), `color` (portable
color), `curve` (ordered `[x,y]` pairs), `gradient` (ordered
`{position,color}` stops), or `lut3d` (visible resource reference or explicit
null). Omission means the definition default; null LUT means intentionally empty.
Definition parameters are a map keyed by stable keys, with required `kind`,
`default`, `label`, and optional `section`, `page`, `visible_when`, `soft_bounds`,
`mapping` and dimensional declarations. Parameter `kind` is an object tagged by `kind`. Number adds
required finite `min`,`max`,`step`, U8 `decimals`, and optional presentation `unit`
(default empty); `min<=max`, `step>0`, decimals at most 6. Choice adds required
`options`, a nonempty array of unique stable literal strings or `{value,label}`
objects (at most 256). Other kinds add no kind fields. Values satisfy their kind:
number within bounds, choice one declared option, curves/gradients 2–32 strictly
increasing points/stops in `[0,1]` with endpoints zero and one. Curve ordinates
are within `[0,1]`. LUT resource type and declared working-color binding are
validated together. Optional soft bounds lie within hard bounds; mapping is
permitted only for number, logarithmic mapping requires positive minimum and
power exponent is within `[0.125,8]`. Labels are literal strings or `{"message":"catalog-key"}`.
`visible_when` addresses a local parameter `key` and typed `value`. Mapping is
`{"type":"linear"}`, `{"type":"log"}` or
`{"type":"power","exponent":number}`, with linear omitted.

Definition `passes` retain `entry` and `sampling` (`neighborhood` with `radius`,
`parameter` with `key`,`scale`,`padding`, or `document`). `lookups` retain code
resource refs, `entry`, ordered parameter `dependencies`, `values`,
`workgroup_size`, `workgroups`. `auxiliary` is `lut3d` with local `resource` and
`color_space` keys, or `analysis` with kind `local_illumination`. `pages` retain
local IDs/labels; `constraints` retain kind `ordered_numbers`, `lower`, `upper`,
`gap`. These local strings are not cross-object references. Missing lists are
empty, missing auxiliary is absent. Required scalar fields are not silently
replaced from a newer catalog. Type and dimensional additions are unsupported
until explicitly interpreted by the reader.

## Resources and packs

A resource has required `id`, `type`, `data`, `encoding`, `location`, `bytes` and
`crc32`. `data` contains its typed interpretation. `bytes` is the stored length
as a decimal `u64` string; `crc32` is exactly eight lowercase hexadecimal digits.
It is CRC-32/ISO-HDLC of the stored payload bytes, using ZIP's polynomial and
initial/final convention, not CRC-32C. It is integrity evidence, not identity,
authentication or sufficient evidence for deduplication.

A location is either `{"member":"data/<resource-id>"}` or
`{"pack":"data/tiles-1.bin","offset":"0"}`. Offsets are relative to the
uncompressed STORED member data, never absolute ZIP offsets. The resource table
is the authoritative pack index; no second binary header or offset table exists.
`offset + bytes` must not overflow or leave the member. Resource lengths and
checksums agree with the complete member for standalone payloads.

Distinct range locations cannot partially overlap. Exact aliases are permitted
only when the complete resource type, interpretation, encoding, length and CRC
agree. Readers allow unreferenced bytes inside a pack; writers compact packs and
emit no unused bytes. The initial writer emits one pack, but readers accept
multiple `data/tiles-<n>.bin` packs, with positive decimal `n` and no leading zero.
Pack partition and range changes never change source or resource identity.

| Resource type | Encoding and descriptor |
| --- | --- |
| `capy.raster-tile/1` | `capy.lz4-tile/1`; exactly 256 × 256 samples, with `channels`, `depth`, `transfer`, `alpha` and an optional visible profile reference. |
| `capy.selection-coverage/1` | `capy.lz4-coverage/1`; coverage depth `u4` or `u8`, required extent/bounds and chunk index. |
| `capy.icc/1` | `raw`; exact profile bytes, profile interpretation validated by the color subsystem. |
| `capy.photo-metadata/1` | `raw`; `kind` is `exif`, `xmp` or `iptc`; retain exact supplied bytes. |
| `capy.wgsl/1` | `utf8`; resolved shader text, local dependency slots only. |
| `capy.lut3d/1` | `capy.lut3d-block/1`; current immutable little-endian F32 cube block, red coordinate fastest, declared size/domain/title. |

Raster channels are `coverage`, `gray`, `gray_alpha`, `rgb`, `rgba` or `cmyk`;
color depths are `u8`, `u16`, `f16`, `f32`. Coverage/material samples are unsigned
`u8` or `u16`; F16/F32 composition coverage is U16, as fixed by
`SampleDepth::coverage`, never floating-point coverage. Multibyte samples are little-endian. Transfers are `linear`, `srgb`
or `profile`; alpha is `none`, `straight` or `premultiplied_linear`. Ordinary
paint is straight RGBA, integer paint uses its working profile and float paint
is linear. Float color is RGB/RGBA only and finite; float alpha is straight and
within `[0,1]`. Preserve hidden straight RGB, including alpha-zero pixels, and
all finite float bit patterns. Imported originals retain their independent
interpretation; paint admission can require matching composition format.

Tiles store one raw LZ4 block with no frame or prefixed decoded length. With `P`
pixels and `B` bytes/pixel, bytes of a multibyte-depth pixel are shuffled by
`stored_uncompressed[b*P+p] = interleaved[p*B+b]`. U8-depth tiles are unshuffled.
The decoded length is exactly `256*256*B`; decoding is bounded to that size and
rejects trailing input. Tile plane membership (`color`, `mask`, `wetness`,
`watercolor_wetness`) belongs to the source's tile index, not the byte identity.
Material settings and missing-tile behavior stay on their authored source.

Selection coverage preserves the current defined packing: rows start at whole
little-endian U32 words, low bits hold the first pixel, with eight U4 or four U8
pixels/word. U4 codes are exactly `0..4` and mean coverage divided by four;
values `5..15` are invalid. U8 codes mean coverage divided by 255. Row padding is zero. Linear word bytes are split into 65,536-byte
chunks, with zero padding in the final chunk, compressed as raw LZ4 without byte
shuffle. Bounds cannot omit nonzero coverage; unused padding is validated.
Contours are not converted to pixel coverage for convenience.

A LUT block retains the existing 96-byte header followed by `size^3` 16-byte
records `[red,green,blue,0]`. Its six header vectors contain `[size,0,0,0]`,
`[low_rgb,0]`, `[high_rgb,0]`, `[exponent_rgb,0]`, `[scaled_low_rgb,0]`, and
`[inverse_scaled_width_rgb,0]`. Per axis, the integer exponent is
`clamp(-floor(log2(max(abs(low),abs(high)))),-126,126)`; scaled low is
`low*2^exponent` and inverse width is `1/(high*2^exponent-low*2^exponent)`, evaluated in
F64 then rounded to F32. Domain, padding and header agreement are checked using
the LUT validator; headers and samples are preserved byte-for-byte, not
regenerated on each save. This freezes a resource encoding, not a requirement
that future GPU implementations consume that header directly.

Verify the manifest member CRC before interpreting the index. Verify resource
CRC before decoding or copying bytes; decoding additionally validates sample and
size rules. A random block read does not require reading its entire pack. A full
pack read/write checks the member CRC. A detected failure is retained on the
backing owner and fails every dependent operation; absent residency or corruption
never becomes empty paint. Save verifies every retained required payload,
including hidden and unplaced content. Unknown ancillary-only payloads require
bounded transport integrity, not knowledge of their decoder.

## Restricted ZIP64 transport

Members use method 0 (STORED), zero flags, no encryption or descriptors, and
matching size/CRC/name/version fields in local and central headers. Names are
relative ASCII paths without empty, `.` or `..` components, backslashes, drive
prefixes, NULs or case-insensitive aliases. No directory entries, symlinks,
archive/member comments, leading executable bytes or trailing bytes are allowed.
Writers use zero DOS timestamps and no platform ownership metadata. Readers do
not use timestamps or external attributes as artwork semantics.

The first local member is `mimetype`, containing exactly the identity bytes above
without a newline. `manifest.json` is required; `preview.png` is optional. Data
members use the resource/pack names above. Bounded optional `META-INF/` members
are reserved for signatures and do not establish authenticity. Only the exact
member `META-INF/content_credential.c2pa` may have zero CRC fields without the
usual CRC comparison; its bounds and header agreement are still checked. Edited
saves drop signatures. Signing and verification are not baseline features.

Local members occupy consecutive ranges beginning at byte zero. Their central
directory follows immediately, with one entry per member in physical order and
no duplicates. The end records follow the directory with no gaps. A reader checks
both views of each member and rejects competing directories or overlapping data.

For sizes/offsets/counts below their classic maximum, use the classic value and
omit the corresponding ZIP64 value. At or above `0xffffffff` for U32 fields or
`0xffff` for U16 counts, use that sentinel and the actual U64 value. ZIP64 extra
field `0x0001` contains exactly the saturated fields in specification order:
uncompressed size, compressed size, local-header offset, disk number. Local
headers include only sizes; STORED sizes are equal and saturate together. No
other extra fields are accepted. Required extraction version is 20 for classic
members and 45 when that member requires ZIP64.

There is one disk, numbered zero. Emit a ZIP64 end record and locator whenever
any member or directory field requires ZIP64; otherwise omit both. The ZIP64
end record has its fixed 44-byte body, the locator immediately follows it, and
the classic end record terminates the file. Classic end fields independently use
actual values or sentinels by the same threshold rule. Disk numbers are zero and
the locator declares one disk. Sizes and offsets are checked with U64 arithmetic
before conversion to a host address size. Test field thresholds without requiring
a multi-gigabyte in-memory allocation.

Local sizes and CRCs are known before writing the member header. Reused block
CRCs can be combined in physical order with their lengths to calculate a pack
CRC; copying still verifies the actual bytes. Newly encoded data/previews use
bounded private spooling when length or CRC is not yet known. A non-seekable
writer never patches headers or emits descriptors.

## Preview and file outcomes

The fixed `preview.png` is at most 1,024 pixels on each axis and 8 MiB encoded.
It is non-interlaced 8-bit RGBA PNG with standard sRGB interpretation and straight
alpha, containing the complete default output framing at a uniform scale. PNG
chunk CRCs, dimensions and decoded allocation bounds are checked before display.
HDR uses the output's authored SDR rendition. No checkerboard, current selection,
mask-inspection tint, guides or editor chrome appears in it. Reduced size and SDR
status remain explicit when it is displayed as the only available representation.

The default output's `representation` is
`{"member":"preview.png","size":[width,height],"color":"srgb"}`. It
exists if and only if the member exists in a supported envelope. Its pixels
represent that output's captured source roots and context exactly. A source save
may omit both the representation and member when preview evaluation/encoding
fails or exceeds its budget; never retain an older preview as current. Missing
preview does not excuse failed source capture, resource integrity or publication.

Shared opening yields one of these outcomes before constructing an editor:

| Outcome | Permitted operations |
| --- | --- |
| Editable | All retained required semantics and resources are valid, supported and admitted; construct the authored model and normal editor. |
| Preserved | Structurally valid but unsupported semantics, envelope, device admission or evaluator support; retain immutable source, list understood outputs, show independently verified representations, copy original bytes and export a representation to a new destination. |
| Recovered view | Native content is invalid, but the separately verified bounded preview is readable; retain original bytes and allow explicit image recovery/export. It is never a successful native editable open. |
| Failure | The container cannot be safely parsed or native content is invalid and no verified representation is available. Retain the original instead of adopting partial artwork. |

Unsupported packages without previews remain Preserved and permit copying the
source and reporting any understood output inventory. A preserved/recovered view
is not an editable preview `Project`. It cannot
replace the original with flattened pixels, acknowledge an editable save, or
enter paint/history code. Converting a representation into new artwork requires
an explicit operation with a new document identity and destination.

## Immutable backing, capture and publication

Shared byte ownership consists of an immutable owner, U64 range, interpretation
and integrity state. Owner identity is unique per opened backing generation;
identical declared resource IDs in separately opened packages do not establish
byte equality. Reads return an owned shared byte range that keeps its owner alive.
An already resident slice requires no new payload allocation. Reads can be
pending, ready or failed; decoded readiness is distinct from byte readiness.
Cancellation drops only the request's ownership and does not poison a backing
still needed by another accepted job, document or undo entry.

Native workers may use retained immutable file ranges. A mutable destination or
revocable provider is spooled into private owned storage before adoption. Streams
are copied in bounded chunks before random-access ZIP parsing. Browser workers
retain package/Blob or private-store owners and service bounded range requests;
U64 values cross bridges as strings or explicit integer halves, never JavaScript
numbers. No input callback reads files, decompresses samples or copies pixels.
M3 may eagerly prepare admitted resources through this interface. It need not
implement M4 demand loading, but cannot require every host to build another full
archive or recreate an artwork schema beside shared Rust.

A shared capture from the ordered editor owner contains immutable artwork roots,
document/session generation, edit checkpoint, working-state generation and output
context. Resource enumeration is independent of ZIP assembly and does not mutate
a cloned document to detach payloads. Unchanged resources retain owner/block
identity across captures. Metadata capture may share immutable roots while raster
publication remains pending; workers await those publications cancellably.
Active contact/predicted pixels and uncommitted operations are excluded.

Output `context` contains finite seconds `elapsed` and an `effect_phases` array
of `{"effect":{"ref":"…"},"phase":seconds}` records. Entries address effect
application identity, never source content hashes or shader catalog names, and
cannot repeat an effect. Captures include every time-dependent effect needed by
the output, including its static phase; a writer omits zero phase overrides only
when their frozen default reproduces that evaluation. Source roots, keyed values, actual integrated phases
and source-analysis inputs are frozen in the same capture. Changing playback
rate integrates the previous rate up to the edit, then changes future motion;
phase is not reconstructed as elapsed times the new rate. Opening seeds animated
clocks at the saved phase with the new host elapsed origin. Static effects use
their authored `time` and rate; zero opening phase is the frozen default for an
animated effect with no saved override. Renderer recreation and worker exports
consume that context rather than restarting the phase. Persisting a portable
output context does not add an animation timeline or session clock to the file.

Capture/write tokens identify their document/session generation and exact captured
checkpoint. A successful durable manual save acknowledges that checkpoint only;
newer edits remain dirty, and Undo/Redo compare against the acknowledged checkpoint.
Recovery publication uses a separate token/state and never marks manual work
saved. Failed, cancelled, duplicate or stale completions cannot acknowledge newer
work, adopt an obsolete document or release another owner's backing. A source
save with an unavailable preview reports source success and preview absence
separately. Hosts retain their actual atomic-replacement/provider guarantees.

The retention visitor accepts authored-document, history, parked-tab, snapshot
and accepted-job roots and accounts shared bytes once per immutable owner. The
portable writer selects artwork roots only. Pending and failed publication owners
survive until their last dependent owner is released, including after Save As,
atomic replacement, tab close and renderer loss. Current recovery can write an
ordinary package, but future private recovery can enumerate and reuse resources
without constructing, reopening or unpacking an archive. Session selection,
targets, camera, tab order and bounded history remain outside the portable
manifest; their shared owners/generations remain available for later coherent
capture under the [recovery boundary](../development/autorecovery.md).

## Fixtures and acceptance cases

A minimal supported document has one composition, one root stack and one canvas
output. This abbreviated fixture has no pixels, paper or optional preview:

```json
{
  "format": "capy.canvas",
  "version": 1,
  "document": "00000000000000000000000000000001",
  "root": {"ref": "00000000000000000000000000000002"},
  "objects": [
    {"id": "00000000000000000000000000000002", "type": "capy.composition/1", "data": {
      "frame": {"size": [64, 64]},
      "result": {"object": {"ref": "00000000000000000000000000000003"}, "port": "color"}
    }},
    {"id": "00000000000000000000000000000003", "type": "capy.stack/1", "data": {}},
    {"id": "00000000000000000000000000000004", "type": "capy.output/1", "data": {
      "source": {"object": {"ref": "00000000000000000000000000000002"}, "port": "color"}
    }}
  ],
  "resources": [],
  "outputs": [{"ref": "00000000000000000000000000000004"}],
  "default_output": {"ref": "00000000000000000000000000000004"}
}
```

Fixtures distinguish malformed content from unsupported content and preservation:

| Fixture | Required result |
| --- | --- |
| Duplicate JSON key/ID, dangling ref, malformed reserved ref, occurrence in two stacks, cyclic stack expansion, overflowing range, noncanonical U64 | Invalid; no editable adoption. |
| Known object with unknown `future_mode`, enum/port, definition ABI or resource encoding | Preserved even when hidden or unplaced. |
| Two groups reference the same stack, or an unplaced second occurrence references an already used paint source | Preserved; direct paint-reference counts alone do not establish support. |
| Unknown non-ancillary record with no path from the root/output | Preserved; it is retained authored work. |
| Unknown ancillary copy-safe note references an existing occurrence and opaque resource | Editable; preserve note/resource unchanged after unrelated edits. |
| Same note without `copy_safe`, or after deletion of its referenced occurrence | Editable; drop note on edited save, retain byte-for-byte on no-edit copy. |
| Ancillary record refers to another ancillary record, or required artwork depends on ancillary data | Invalid. |
| Known future root with empty outputs and no default | Structurally valid; preserved if outside the editable subset. |
| Supported source, missing or corrupt preview | Editable source; no stale preview is adopted. |
| Corrupt required source block with valid preview | Recovered view; never substitute empty source. |
| Source capture succeeds and preview rendering/encoding fails | Source-only save succeeds; no representation/member survives. |
| Interrupted/stale write, provider pipe, browser bounded ranges, ZIP64 size/offset/count threshold | Preserve original publication and newer work; no imprecise bridge integers or unbounded whole-package copy. |
| Save after rate change; renderer recreation; retained analysis/bake during later painting | Saved/reopened output uses captured roots and integrated phase; old resources live through accepted jobs. |

Retain exact-byte/source/material/profile/LUT/selection assertions from the existing
codec tests when replacing their envelope fixtures. Retain the integrated-phase
oracle in `snapshot/tests.rs`, publication and ownership assertions in
`raster_storage.rs`, Save/Undo/Redo policy tests and source-analysis lease tests.
Round trips alone do not establish semantic fidelity, compressed-byte reuse,
allocation bounds or host qualification.
