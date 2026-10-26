# Capy package contract

[Technical documentation](../README.md)

The shared `layer-core::package` library implements the application's portable
`.capy` envelope, typed record adapters, immutable resource visitor, bounded
transport and package I/O. The [authored model](authored-model.md) defines runtime
ownership; [project saving](project-format.md) describes capture and publication.
Library tests do not qualify host journeys, device admission or frame performance.

## Envelope and references

The package identity is `application/vnd.capycanvas`. Its UTF-8 `manifest.json`
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
Native document construction, authored edits and ready raster capture enforce
the same object/resource namespace before a package is written. Undo and redo
retain that namespace while they own records or resources.

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
signed 64-bit fields, with a leading minus only for negative values and
the signed range enforced by that field. Counts/dimensions explicitly declared
`u32` and schema versions are JSON integers. Floating values are finite JSON
numbers with type-defined ranges; stored pixel floats remain binary. JSON
objects cannot repeat keys, even if their values agree. Reject repeated keys
before interpreting or discarding any record.

Unknown envelope fields or an unknown envelope version prevent interpretation of
the manifest; only the fixed preview convention and original package remain
available. The format therefore grows through record types, fields and values,
never through new envelope fields. Malformed JSON, IDs, references, lengths and known required fields are
invalid. Metadata size, nesting, total reference edges and reference-expansion
work are bounded before adoption independently of device pixel admission.

## Object records and defaults

An object is `{"id":"…","type":"capy.stack/1","data":{…}}`. Optional
`ancillary` and `copy_safe` flags default to false. A known type's flags must
match its registration, and every current type is required artwork, so known
types reject either flag. An unknown type marked `copy_safe` without `ancillary`
is unknown required artwork. Required artwork, required resources and photo
metadata cannot depend on ancillary records. An ancillary record that names
another ancillary record is accepted but never retained after an edit. Unknown
ancillary data is retained after edits only when copy-safe and every reference
still resolves; otherwise it is dropped on an edited save. Retention checks only
that referenced records still exist, so a copy-safe record may depend on their
identity, never on their content. No-edit copying retains the original package.
Unknown records are not cloned onto independently duplicated objects.

The current registry is shared by portable and private contexts. Manifest
classification, topology, record adapters and history descriptors consume its
type definitions and contextual restrictions.
Superseded pre-release types (`capy.occurrence/2`, `capy.composition/1`,
`capy.coverage-source/1`, `capy.output/1`, `capy.paint-source/1`, `capy.effect/1`
and `capy.effect-definition/1`) are absent from it, so a package using them opens
as unsupported and is preserved; there is no conversion reader.

| Type | Owned data and wire relationship |
| --- | --- |
| `capy.composition/2` | Required pixel `size:[width,height]` and `result` endpoint; optional `resolution`, `color`, `blend`. The frame has no saved origin. |
| `capy.stack/1` | `entries`, an ordered array of occurrence references, front to back. |
| `capy.occurrence/3` | Required `content`; `name`, `visible`, `opacity`, `blend`, `locked`, `alpha_locked`, `reference`, `attachment`, a signed decimal-string integer `offset` for paint, object and stack content, and an optional `mask` with its own integer `offset`, relative to its owner when linked. |
| `capy.paint-source/2` | Required pixel `domain`; authored `color_mode` (`full_color`, `grayscale`, `two_tone`, default `full_color`), optional `base` image binding, sparse `tiles`, `material`. The binding owns its base offset and color policy. |
| `capy.image/1` | Immutable sample `extent`, `interpretation`, complete `tiles`; optional physical `resolution`. |
| `capy.image-object/1` | Required immutable `image` reference; optional F64 `affine`, `interpolation`. |
| `capy.vector-object/1` | Optional `items`, an ordered array of `capy.vector-items/1` resource references whose record streams concatenate front to back; `next_item`, a decimal-string `u64` greater than every item ID in the object, including IDs inside unknown records and Items-class values, required when `items` is present; and optional `slots`, an ordered array of resource or `capy.image/1` references that item records name by index. Absent means none. See [vector records](#vector-records). |
| `capy.coverage-source/2` | Required pixel `domain`; `default_coverage` and sparse `tiles`. A mask made from a selection stores that selection's coverage in tiles. |
| `capy.effect/2` | Required inline `builtin`, parameter-data `version` and every keyed `values` entry. Effects consuming authored coordinates require F64 `spatial`. |
| `capy.selection/1` | Required `shape`; optional `affine` and `inverted`. Pixels use coverage resources; contours keep their geometry. |
| `capy.guides/1` | Authored ruler geometry and reference markings. |
| `capy.output/2` | Required `source` composition endpoint; `name`, `context`, SDR rendition, proof intent and disposable optional `representation`; unfamiliar caches are ignored. Export requests own delivery size. |

`content` is one of `{"paint":{"ref":"…"}}`,
`{"object":{"ref":"…"}}` (one image or vector object), `{"stack":{"ref":"…"}}`, `{"effect":{"ref":"…"}}`,
or `{"selection":{"ref":"…"}}`.
Exactly one alternative appears. Paper is an ordinary Solid Color effect
occurrence, with its color in the effect values. Files using the former inline
Paper alternative fail admission. A mask is inline
`{"source":{"ref":"…"},…}` with slot key `mask` fixed by the occurrence
schema, and optional `enabled`, `linked`, `inverted` and `offset` values.
A linked mask's document origin is the parent offset plus the owner offset plus
the mask offset; an unlinked mask omits the owner offset. Effect and selection
occurrences carry no `offset`, and `alpha_locked` applies only to paint.
The coverage source has its own paint-target identity.

Paint sources write `color_mode` even at its default. Full color paint tiles use
four channels; Grayscale and Two-tone paint tiles use two (gray and straight
alpha), at the composition's depth and transfer. Coverage and watercolor planes
keep their scalar descriptors. Two-tone commits black or white with binary
coverage; its color threshold is encoded gray 0.5 for integer documents and
linear gray 0.5 for floating documents, and its alpha threshold is 0.5.
Layer filters operate on expanded RGB and may introduce color without changing
the paint source's mode. A conversion preserves retained original image bytes;
Discard Paint Edits keeps the layer's color mode.

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
| `locked`, `alpha_locked`, `reference`, `inverted` | False. |
| Occurrence `blend` | `normal`; group pass-through is explicitly `pass_through`. |
| Occurrence `attachment` | `none`; alternatives are `clip` and `effect`. |
| Composition `blend` | `linear`. |
| Composition `color` | Built-in `srgb`, unsigned `u8`. |
| Physical `resolution` | Unspecified; it is not silently 72 or 300 pixels/inch. |
| Paint base `offset` | U32 `[0,0]`. |
| Occurrence/mask `offset` | Canonical signed decimal strings `["0","0"]`. |
| Paint base `policy` | `source_profile`; the alternative is `working_pixels`. |
| Image-object `affine`, `interpolation` | Identity F64 affine; `linear`, with `nearest` as the other implemented kernel. |
| Image `profile_assumed` | False. |
| Paint overrides/material tiles | No override at absent coordinates; an imported base is revealed. |
| Coverage `default_coverage` | One; stored tiles take precedence. |
| Effect parameter value | Required for every parameter, including a value equal to the insertion default. |
| Custom effect alpha/space/time | `preserve`, `linear`, false. |
| Output context | Animated effect opening phase zero. |
| Output representation | Unavailable; unfamiliar or unusable cached representations are ignored. |

Except for the disposable output `representation`, unknown fields, variants, parameter keys, choice values, ports, resource encodings
or evaluation contracts in a required record make the entire artwork unsupported.
Do not deserialize them away. Malformed data for understood fields is invalid.
Invalid is reserved for data that can never become valid: malformed JSON, IDs,
references, lengths, integrity, cycles and values outside a field's mathematical
domain. A well-formed value that this version does not accept, such as a count,
range or combination a later version may allow, is unsupported. That includes a
reference to a known record type in a slot that does not accept it, a field that
does not apply to the occurrence's content and an attachment without an eligible
target. Later versions extend the format with new names, never by giving an
existing field a second meaning.
Reader, editor admission and finalized capture publication share metadata and
record admission. Metadata is bounded to 64 MiB, fewer than 128 nested JSON
containers and 4,194,304 JSON values, counted while parsing so a larger document
stops before its tree is built; evaluation graphs admit 65,536 objects,
262,144 evaluation edges and depth 128.
Layer names are at most 4,096 bytes. Resource bindings, including repeated
tile references, do not consume evaluation edges. There are at most 262,144
resource records. Project admission also bounds layers, dimensions, retained
source bytes and raster bytes using the same units on reopen. Shared source
owners and compressed tile allocations count once; editable raster capacity is
charged per source. Opening charges vector items for both their decoded bytes
and the path records they become. Pending edits must resolve before final publication.

Current evaluator bounds include 1,024 rulers, 32 mesh cells per axis, 32 curve
points or gradient stops and LUT cubes of side 65. Precision bounds on invertible
maps, positive mesh intervals, distinct ruler points and finite LUT domains are unsupported; singular
maps, horizon crossings and unordered domains are invalid. Readers stop at an
admission bound and preserve original bytes without attempting further allocation.
Known outputs are listed when bounded metadata parsing can reach them.

Validate every retained object, including unplaced work. A valid graph outside the
editable subset remains preserved: shared editable sources, shared effect
applications or two group occurrences using one stack cannot be normalized by
duplicating or dropping their content. An occurrence in two containing stacks
violates occurrence identity and is invalid. Cycles in
instantaneous evaluation or recursive expansion and dangling references are
invalid; the cycle rule does not apply indiscriminately to ancillary or resource
references. Relationship semantics and field ownership are specified with the
[authored model](authored-model.md).

Built-in applications inline `{"builtin":"exposure","version":1,"values":{…}}`. The
version describes parameter data, not shader code. Gradient Map, Gradient Fill,
Denoise, Domain Warp, Posterize and Kaleidoscope use data version 2. Gradients
retain explicit interpolation and stops; Posterize and Kaleidoscope require whole
counts. Denoise authors Strength with an internal radius of two pixels; Domain
Warp uses three internal noise octaves. Opening resolves the current bundled
implementation and editor schema once. Labels, ranges used by sliders,
shader ABI, modules, passes, GPU preparation and fusion policy are app code;
they are not saved for built-ins. Slight rendering changes from shader fixes
are allowed. Built-in IDs are reserved; imported packages cannot replace them.

Every application saves all keyed values, including defaults. Parameter keys,
choice IDs, units and accepted data ranges form the lasting contract. Reordering
controls or choices and changing insertion defaults must leave saved values
unchanged. A UI range uses `soft_bounds`; it must not narrow accepted saved data.
Unknown IDs, data versions, parameter keys or choices preserve the package as
unsupported, without guessing, dropping values or substituting defaults.

Pixel lengths retain their complete authored value, including values outside the
catalog's numeric editing bounds. A Gaussian `sigma` of 120 means a standard
deviation of 120 source pixels. UI bounds and GPU table capacity cannot change
that meaning. Sampling footprints and dirty regions follow the authored length.
An evaluator may approximate the mathematical operation, with numerical reference
tests covering its error, while preserving its scale, color domain and endpoints.

Keep fixed-file tests for the supported parameter data. Before release, replace
superseded representations without adding conversion readers. Once a data version
is released it stays readable: changing what a built-in's saved values mean needs
a new data version with a conversion from the previous one and a fixed-file test,
never a reinterpretation in place. Historical shaders,
shader generations, generic schema rebinding and migration machinery are outside
the format. New fields must represent authored
intent; runtime layouts, caches, preview settings and editor organization stay
outside the file.

### Adding controls after GA

Use a built-in **parameter-data version bump with an explicit converter** when
adding an authored control to a released built-in. New readers recognize the
released source version, validate its complete values against that version's
schema, then convert to the supported representation before current-schema
validation. The converter inserts the documented value that preserves the
earlier control intent; it does not consult a later insertion default. Preserve
all other authored values and resources. Writers save every parameter of the
resulting version, including defaults and hidden controls.

A missing required value in a package claiming the current version remains
invalid. Do not add a key to a released version and recover missing values by
guessing. Unknown future built-ins, versions, keys or choices remain unsupported.
An older app preserves the original package and may display an independently
verified cached preview; it cannot promise an editable document or a fresh
read-only render of effects it does not understand.

This is the extension rule for released GA data, not an implementation of a
conversion framework. Add each concrete converter with an unchanged older-file
regression, including save/reopen and resources, when that extension ships.
Pre-release replacements still need no compatibility reader. Future controls
have no reserved field today, and their neutral/default values are not frozen
until their introduction requires a concrete conversion decision.

Algorithm improvements in the explicitly evolving artistic filters follow the
[illustration rendering policy](../development/illustration-filters-proposal.md#release-policy-small-saved-controls-evolving-artistic-rendering).
They do not require parameter-data versions solely to retain an old appearance.
Saved IDs, units, accepted values and resources remain interpretable; changes to
those data meanings still use the extension rule above. The current evaluation
tables and hardware fixtures describe the implemented baseline. Enabling this
exception for a filter requires its implementation and tests to distinguish
stable data/geometry/alpha invariants from reviewed artistic appearance changes;
it does not authorize weakening unrelated color or sampling contracts.

### GA filter data and planned extensions

The [illustration design](../development/illustration-filters-proposal.md#decisions-before-implementation)
coordinates these additions with the
[object-layer ownership](authored-model.md#object-layers-and-image-objects).
Image-input roles, seed controls and asset selections below remain planned
extensions. Spatial references are implemented in `capy.effect/2`:

- Actual image inputs reference the shared `capy.image/1`, with a per-use
  `color` or `data` role. Raw data sampling ignores profile conversion without
  discarding the shared image's interpretation. Do not invent another image
  resource or add empty future graph input maps.
- Coordinate-dependent effects save the F64 reference-to-composition mapping
  and positive extent they evaluate. Crop composes its translation once;
  Image Rotate, Flip and Size compose their geometry while retaining authored
  values and reference extent. Pointwise built-ins omit this reference.
- Seeds use Number with Count dimension and inclusive range `0..16777215`.
  Reject fractional wire values before conversion to f32; save Randomize as an
  ordinary authored edit. This does not promise support for every u32 value.
- Serialized bundled-asset IDs are permanent. Artistic pixels may evolve; a
  retired ID needs an explicit, cycle-free mapping to an available replacement.
  Internal assets with no serialized selection need no portable ID. Imported
  images remain retained authored samples.
- Freeze each new parameter's accepted type, units, bounds, choices and coupled
  constraints before that filter's release. Do not narrow a released accepted
  range; slider ranges and processing admission are separate. Unsupported work
  must not silently clamp authored values.

Ship each data contract with its actual consumer and fixtures. A future filter
can reuse the package envelope while remaining unsupported by an older reader;
no unimplemented filter or parameter is reserved solely to avoid that outcome.

Custom programs are private session content. Portable save refuses them and
portable open preserves their package without executing embedded shaders. See
[private custom programs](../internals/session-recovery.md#private-custom-programs).

### Nested values

The following spelling is independent of Rust enum/field serialization. All
objects have closed known field sets; unknown additions follow preservation policy.

| Value | Grammar |
| --- | --- |
| Pixel point/size | Two-element `[x,y]`; existing point components finite F32, sizes positive U32. |
| Source `domain` | `{"size":[width,height]}`; local origin is zero. |
| Working `color` | `{"space":"srgb","depth":"u8"}` with either default-valued field omitted. Spaces are `srgb`, `display_p3`, `adobe_rgb`, `pro_photo`. |
| Portable color | `{"rgba":[r,g,b,a],"space":"srgb"}` with transfer-encoded RGB, or `{"linear_rgba":[r,g,b,a],"space":"srgb"}` with linear RGB in the space's primaries; exactly one of the two arrays. `space` defaults to `srgb`. Writers use `linear_rgba` only when encoding would lose the authored value. |
| Physical resolution | `{"unit":"inch","density":[[x_n,x_d],[y_n,y_d]]}`; all values required and positive U32; unit also permits `centimetre`, `metre`. |
| Profile | `{"builtin":"srgb"}` or `{"resource":{"ref":"…"}}`; exactly one alternative. |
| Sparse tile entry | `{"coordinate":[x,y],"plane":"color","resource":{"ref":"…"}}`; all fields required, coordinate U32 in source-local 256-pixel tiles, no repeated coordinate/plane pair. |
| Immutable image | Required `extent`, `interpretation`, `tiles`; optional `resolution`. Interpretation has required `channels`, `depth`, `profile` and optional `profile_assumed`. Tiles have coordinate/resource only. |
| Paint `base` | Required `image:{ref:…}`; optional U32 `offset:[x,y]` and `policy:source_profile` or `working_pixels`. The image rectangle must fit the zero-origin paint domain. |
| Image-object affine | Six finite F64 values `[a,b,c,d,tx,ty]`; invertible with finite bounds. |
| Effect spatial reference | Required `mapping:[a,b,c,d,tx,ty]` of finite F64 values and positive `extent:[width,height]`; invertible mapping from authored reference to composition coordinates. |
| Material | `{"watercolor":{"wet_edge":number,"burnt_edge":number,"edge_width":number}}`; all three settings required when watercolor exists; finite wet/burnt edges in `[0,1]`, positive edge width in source pixels, currently admitted in `[1,16]`. |
| Selection shape | `{"contours":[[[x,y]…]…]}` using even/odd interiors, or `{"pixels":{"extent":[w,h],"bounds":[x0,y0,x1,y1],"depth":"u4","chunks":[{"ref":"…"}…]}}`; exactly one alternative. U8 coverage uses depth `u8`. |
| Selection geometry | Optional `affine:[a,b,c,d,tx,ty]` and `inverted`; identity/false when absent. |
| Guides | `rulers` array with required stable portable `id` and `geometry`; geometry is `{kind:"straight"|"parallel",start:[x,y],end:[x,y]}` or `{kind:"radial",center:[x,y]}`. Ruler IDs are type-owned local subelement IDs. |
| SDR rendition | `sdr` with optional `exposure`, `contrast`, `headroom`, `highlight_color`, `balance`; defaults respectively `0`, `1`, `2.3004484`, `0.3`, `0`. |
| Proof intent | `proof` with required `name`, `profile`, optional `intent`, `black_point_compensation`, `simulate_paper`, `simulate_black_ink`; defaults relative colorimetric, true, false, true. |

Affine `[a,b,c,d,tx,ty]` means `x'=a*x+c*y+tx`, `y'=b*x+d*y+ty`.
Affine matrices preserve invertibility and bounded mapping validation.
Occurrence and mask `offset` values are canonical signed 64-bit decimal strings:
no sign on positive values, no `-0` and no leading zeros. Malformed spellings and
64-bit overflow are invalid; well-formed values beyond the editor's integer
admission are unsupported.
Saved-selection overlay visibility, color and opacity belong to working state
and are absent from this grammar. Selection occurrences fix `visible` to true,
`opacity` to one, `blend` to normal, and `reference` and
`alpha_locked` to false, with no mask or attachment; these defaults are omitted.
Names and edit locks remain authored. Blend and
rendering-intent names use the existing semantic names in lower snake case,
never Rust discriminants or GPU blend codes. Blend names are `normal`,
`multiply`, `screen`, `add`, `overlay`, `soft_light`, `color`, `darken`, `lighten`,
`color_burn`, `linear_burn`, `color_dodge`, `hard_light`, `vivid_light`,
`linear_light`, `pin_light`, `hard_mix`, `difference`, `exclusion`, `subtract`,
`divide`, `hue`, `saturation`, `luminosity`, `pass_through`. This version admits
pass-through only on stack occurrences. Attachment relationships resolve from stack order at admission:
`clip` content shares an eligible unclipped base, while `effect` adjustments belong
to the eligible content below their contiguous chain. Direct targets are paint, image
layers or isolated groups. Selection rows do not affect clipping bases and cannot split an
effect chain from its owner. Attachments without an eligible target and
pass-through dependencies are unsupported. Rendering intents are `perceptual`, `relative_colorimetric`,
`saturation`, `absolute_colorimetric`. Proof paper simulation requires black-ink
simulation; absolute colorimetric intent forbids black-point compensation.

Effect `values` maps stable parameter keys to `{kind,value}` values: `number`
(F32), `toggle` (boolean), `choice` (stable option string), `color` (portable
color), `curve` (ordered `[x,y]` pairs), `gradient` (`stops`, an ordered array of
`{position,color}`, and `interpolation`: `classic`, `linear_rgb` or `oklab`), `lut3d` (`{"resource":{"ref":"…"},"title":"…"}` or explicit
null), or `texture`. A texture is a built-in
[texture program](../development/texture-programs.md) with all its values,
`{"builtin":"watercolor","version":1,"values":{…}}`, or an image,
`{"image":{"ref":"…"}}`, naming a `capy.image/1` record with gray U8 samples, a
required resolution and at most 4096 samples a side, read as height data without
profile conversion. Every parameter is required; null LUT means intentionally
empty.
### Evaluation meaning

The table below describes the implemented baseline. Renderer changes may refine
precision, performance and approximations within these meanings; changing stable
data meanings needs a new record or data version. The scoped artistic exception
in [Adding controls after GA](#adding-controls-after-ga) permits reviewed look
improvements when adopted by the relevant filter's implementation and tests.
Spatial effects evaluate their saved authored reference. Current-frame analysis
grids remain separate, and temporary capture edges do not replace source bounds.

| Saved value | Meaning |
| --- | --- |
| Pixel samples | Integer composition samples are display-referred values under the working space's transfer curve. Float RGB is linear, with 1.0 at the 203 cd/m² reference white. Composition `depth` selects that range and the committed sample precision; composition never uses less precision than the declared depth. |
| Blend names | The formulas, ranges and luma weights in [blend modes](../internals/rendering.md#blend-modes), checked against its independent reference. |
| Built-in positions | Authored reference pixels mapped into composition coordinates by the saved spatial affine. New effects start at the local frame's top-left. Centers use the saved reference extent; Vignette radius uses half that extent on each axis, Swirl radius half its shorter side, and Gradient Fill scale its extent along the authored axis, or half its diagonal when radial. Image Size transforms the reference while leaving these values unchanged. |
| Built-in angles | Degrees in the saved authored reference. Gradient Fill turns counterclockwise from +x; every other spatial angle, including Swirl's turn, turns clockwise from +x in y-down reference pixels. |
| Curves | Monotone cubic Hermite through the saved points with Fritsch–Butland interior tangents and secant end tangents, continuing linearly beyond the end points. |
| Filter color and alpha | The keyed built-in contract fixes `space` and `alpha`. Filter input/output is premultiplied in linear document RGB, or in the composition's blending space when declared `blending`. `preserve` retains source coverage; `filter` permits coverage to move with the samples. Tagged colors remain straight and convert to the document's primaries before evaluation. Extended RGB remains extended unless the named operation explicitly clips it. |
| Gaussian family | Normalized separable weights proportional to `exp(-x²/(2*sigma²))`, supported through `ceil(3*sigma)` on each side. Zero sigma is identity. Larger sigma widens the kernel; it does not stop at an editor or lookup-table limit. Gaussian Blur, Unsharp Mask, High Pass, Bloom, Soft Focus and Pencil share this kernel. |
| Spatial sampling | Filter sample positions are continuous pixel coordinates; centers are half-integers. Image filters interpolate linearly unless they explicitly select native pixels, and extend the nearest edge sample outside the finite source support. Dither Spread, Scatter and Crystallize copy native RGBA pixels without interpolation; Density gates whole cells. Motion Blur averages a centered line of the authored `distance` and `angle`. Pixel Mosaic samples the center of each authored-size grid cell. Image-object interpolation uses its saved kernel independently of filter sampling. |
| Image-object kernels | `nearest` takes the source sample containing the inverse-mapped output pixel center, transparent outside the image, also under minification. `linear` is a scale-adapted tent: with `J` the inverse map's 2×2 part per output pixel and `J = U diag(s1,s2) Vᵀ`, `S = U diag(max(1,s1),max(1,s2)) Uᵀ`; each source sample center `p` weighs `max(0,1-|r.x|)*max(0,1-|r.y|)` for `r = S⁻¹(p-q)`, and the premultiplied sum is divided by the weights of the whole lattice, including transparent samples outside the image. Without minification this is bilinear. The binary64 reference is [`object_sampling_reference.rs`](../../crates/layer-render-wgpu/src/object_sampling_reference.rs); moving display may approximate `linear`, while captures, exports, merges and pixel tools use the saved kernel. |
| Tone and color adjustments | The equations and neutral conditions in [photo adjustment mathematics](photo-adjustments.md) and [runtime filter color semantics](runtime-filters.md#filter-spaces). Amount/strength controls retain each operation's existing mapping, including divisions by 100, stop exposure, thresholds and signed continuations; a new slider mapping cannot retune them. |
| Gradients | Interpolate associated color and coverage in the saved mixing space; preserve exact stop colors, extend end colors outside the stop range and contribute no hidden color in a fully transparent span. Gradient Map's stop alpha is mapping strength; Gradient Fill's stop alpha is coverage. |
| Procedural patterns | `fx_random`, `fx_noise`, `fx_noise2` and `fx_fbm` in [`filter_library.wgsl`](../../assets/filters/filter_library.wgsl), with each filter's fixed seeds, belong to that filter's data version. |
| Time and phase | Saved output phases are accumulated seconds, already integrated through speed changes. Without an output phase, frozen time is `time*speed`. Pattern frequency and phase direction belong to each filter's math; playback scheduling does not. |
| Imported originals | Untouched original tiles convert to the working space with relative colorimetric intent and no black-point compensation; painted tiles hold the converted result. |
| Masks | A document pixel reads the stored coverage at its position minus the mask origin, or the default coverage where no tile is stored, then applies inversion. The origin is the parent offset plus the mask offset, plus the owner offset when linked. |
| SDR rendition | Parameters of the tone mapper in [`sdr.rs`](../../crates/layer-core/src/color/hdr/sdr.rs): exposure is stops, headroom is the input white endpoint in stops above RGB 1, and highlight color runs from luminous white to retained chroma. Macro/micro gains are `1.3*contrast*2^(∓0.5*(0.3+balance))`; baseline compression is 0.6 and default headroom is log2(1000/203). |
| Watercolor | Exact saved pigment and wetness samples, with the 2/255 material floor and the wet-edge, burnt-edge and source-pixel edge-width meanings in [painterly paint state](painterly-paint-state.md). Opening never replays strokes or ages the saved water. Future brush interactions may improve while retaining that material interpretation. |

The compact mathematical expressions in the bundled filter sources define each
built-in's remaining parameter mappings. Their names, coordinate conventions,
strength normalization, clipping decisions, seeds and neutral behavior belong to
the data version. Tap packing, pass fusion, workgroup sizes, precision improvements
and numerical approximations may change. Tests compare independent equations and
authored values with operation-specific tolerances; rendered GPU bytes are not the
portable contract. Fixed linear-float hardware renders protect the appearance of
saved values. A change beyond its recorded tolerance is a regression to fix, or
requires a new data version with a concrete converter and a fixed-file regression
test, except for a reviewed artistic appearance change under the scoped policy
above. Procedural synthesis in those designated filters may evolve while its
saved seed, spatial reference, resources and control intent remain interpretable.
Never regenerate a baseline merely to make a failing test pass.

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
| `capy.raster-tile/1` | `capy.lz4-tile/1` or `capy.deflate-tile/1`; exactly 256 × 256 samples, with `channels`, `depth`, `transfer` and `alpha`. A `profile` transfer uses the profile of the owning original or composition. |
| `capy.selection-coverage/1` | `capy.lz4-coverage/1`; coverage depth `u4` or `u8`, required extent/bounds and chunk index. |
| `capy.icc/1` | `raw`; exact profile bytes, profile interpretation validated by the color subsystem. |
| `capy.photo-metadata/1` | `raw`; `kind` is `exif`, `xmp` or `iptc`; retain exact supplied bytes. |
| `capy.wgsl/1` | `utf8`; resolved shader text for custom programs in the private formats, local dependency slots only. |
| `capy.lut3d/1` | `capy.rgb-f32/1`; current immutable little-endian F32 cube block, red coordinate fastest, declared size/domain. Titles belong to individual parameter bindings. |
| `capy.vector-items/1` | `capy.vector-records/1`; a record stream of at most 64 records with the integer `records` count in `data` ([vector records](#vector-records)). |

ICC, photo metadata, WGSL, LUT and vector-item resources may use `capy.lz4-bytes/1`
or `capy.deflate-bytes/1` instead of their raw encoding. In that case
`data.decoded_bytes` is required and gives the exact decoded length as a
canonical decimal U64 string. It is absent for raw encodings. `capy.lz4-bytes/1`
is one raw LZ4 block; `capy.deflate-bytes/1` is one complete zlib stream
(RFC 1950, Adler-32 included) with no trailing input. Either preserves every
decoded byte; the resource's type fixes the interpretation after decompression.
Writers select LZ4 only when it reduces size, cache the encoded result on the
immutable resource, and preserve the original LZ4 encoding and bytes when opening
and saving unchanged resources. Current writers do not produce DEFLATE; readers
decode it and save the resource again with LZ4. Bounds on both encoded and
decoded size apply before allocation.
No quantization, palette reduction or lossy image encoding is used for authored data.

Raster channels are `coverage`, `gray`, `gray_alpha`, `rgb`, `rgba` or `cmyk`;
color depths are `u8`, `u16`, `f16`, `f32`. Coverage/material samples are unsigned
`u8` or `u16`; F16/F32 composition coverage is U16, as fixed by
`SampleDepth::coverage`, never floating-point coverage. Multibyte samples are little-endian. Transfers are `linear` or
`profile`; alpha is `none` or `straight`. Ordinary
paint is straight RGBA, integer paint uses its working profile and float paint
is linear. Float color is RGB/RGBA only and finite; float alpha is straight and
within `[0,1]`. Preserve hidden straight RGB, including alpha-zero pixels, and
all finite float bit patterns. Imported originals retain their independent
interpretation; paint admission can require matching composition format.

`capy.lz4-tile/1` stores one raw LZ4 block with no frame or prefixed decoded length. With `P`
pixels and `B` bytes/pixel, bytes of a multibyte-depth pixel are shuffled by
`stored_uncompressed[b*P+p] = interleaved[p*B+b]`. U8-depth tiles are unshuffled.
`capy.deflate-tile/1` stores one zlib stream of the same planes for every depth,
U8 included, after a Sub filter on each 256-byte plane row: every byte but the
first of a row holds its difference from the byte to its left, modulo 256.
Current writers do not produce it; readers decode it and keep the tile as LZ4.
The decoded length is exactly `256*256*B`; decoding is bounded to that size and
rejects trailing input. Tile plane membership (`color`, `mask`,
`watercolor_wetness`) belongs to the source's tile index, not the byte identity.
Material settings and missing-tile behavior stay on their authored source.
`watercolor_wetness` tiles require the source's watercolor material. A sample of
at least 2/255 (U8 code 2, U16 code 514) marks watercolor paint, whose edges the
material settings shape; the amount above that is water left for later strokes,
which the brush engine may use as it chooses. Smaller values are not watercolor.

Selection coverage preserves the current defined packing: rows start at whole
little-endian U32 words, low bits hold the first pixel, with eight U4 or four U8
pixels/word. U4 codes are exactly `0..4` and mean coverage divided by four;
values `5..15` are invalid. U8 codes mean coverage divided by 255. Row padding is zero. Chunks store only the words of
`bounds`: rows `y0..y1`, and in each row the words from the one holding pixel
`x0` to the one holding pixel `x1 - 1`. Every other word is zero. Those words,
row by row, are split into 65,536-byte chunks, with zero padding in the final
chunk, compressed as raw LZ4 without byte shuffle; empty bounds have no chunks.
Bounds cannot omit nonzero coverage; unused padding is validated.
Contours are not converted to pixel coverage for convenience.

A LUT resource stores `size`, input `domain`, and exactly `size^3` tightly
packed RGB samples: three little-endian F32 numbers per sample, red varying
fastest, then green, then blue. `capy.rgb-f32/1` has no header or padding.
Samples, domain and title survive save/reopen without reinterpretation. The
renderer derives normalization metadata and its GPU buffer layout at runtime.
Finite ordered domains outside GPU precision or range, and cubes exceeding the
current size limit, are unsupported rather than corrupt. Nonfinite samples,
unordered domains and inconsistent payload lengths remain invalid.
The selected color space uses the stable IDs `srgb`, `display_p3`, `adobe_rgb`,
`pro_photo`; option order is unrelated to their explicit GPU codes.

The 256-pixel raster tile and 65,536-byte selection chunk sizes belong to their
wire encodings. Changing runtime storage must not change interpretation of these
encodings. Watercolor coefficients and wetness planes remain authored material
state; brush presets and input dynamics are not stored stroke recipes. SDR
rendition coefficients retain their wire defaults independently of UI defaults.
Current renderer math may evolve while retaining these authored values.

Verify the manifest member CRC before interpreting the index. Verify resource
CRC before decoding or copying bytes; decoding additionally validates sample and
size rules. A random block read does not require reading its entire pack. A full
pack read/write checks the member CRC. A detected failure is retained on the
backing owner and fails every operation that interprets its content; absent
residency or corruption never becomes empty paint. Copy Original may still copy
the captured bytes after an integrity failure. A source-read failure remains
terminal for both interpreted reads and original copying. Save verifies every
retained required payload, including hidden and unplaced content. Unknown
ancillary-only payloads require bounded transport integrity, not knowledge of
their decoder.

### Vector records

A vector object holds one item tree as a stream of binary records. The stream is
split into `capy.vector-items/1` resources of at most 64 records each. A record
never spans resources, and a resource boundary may fall inside an unknown
container. Writers keep resource membership stable across edits, so unchanged
resources keep their IDs and bytes. Items are front to back, as stack entries are.

**Framing.** All integers are little-endian. A varuint is minimal unsigned LEB128.

| Element | Encoding |
| --- | --- |
| Record | Kind word (varuint). Kind 0 is the End marker and nothing follows. Otherwise a varuint item ID (0 is anonymous) and a varuint byte length, then that many bytes of properties. |
| Property | Tag word (varuint), varuint value length, value bytes. Tag numbers strictly ascend within a record. Every value is length-prefixed, so any reader can step over any property. |
| Kind word | `number << 4` with flags ignorable (1), copy-safe (2), parent (4) and reserved (8). A parent's children and its End marker follow it. |
| Tag word | `number << 5` with flags ignorable (1), copy-safe (2), a value class in bits 2–3 and reserved (16). |
| Value class | Data (0) opaque bytes; Items (1) a list of nonzero varuint item IDs; Slots (2) varuint indices into the object's `slots`; Nested (3) a record stream with the same framing. |

Flags belong to a number's identity: a known number with other flags is invalid.
Numbers are listed, with their flags, owners and the vocabulary revision that
introduced them, in
[`vector_numbers.tsv`](../../crates/layer-core/src/package/vector_numbers.tsv),
which only grows; retired numbers stay reserved. Revision 1 is the vocabulary
of the first reader; a layer that uses a later number needs a fallback image
for older readers.

**Slots.** Item records never contain Capy IDs. A record that needs another
record names it through the object's `slots` array, by index, in a Slots-class
value. A slot is a resource of any type or a `capy.image/1` object; any other
object type makes the object unsupported. A slot index at or past the end of
`slots` is invalid. A reader keeps every slot of an object while the object
exists, including slots that no property it understands uses, and never
interprets a slot resource, so a resource that only slots reference may have a
type or encoding the reader does not know. A slot resource must not reference
other records. When paths are copied or merged into another object, unknown
properties of the Items and Nested classes and unknown paint records are dropped,
because their identities belong to the source object, and so are Slots-class
properties unless both objects share their slots.

**Fallback.** A `capy.vector-object/1` record may carry `fallback`: the
`image`, `affine` and `interpolation` of an image object that shows the layer.
A reader that can read every item ignores it and drops it on save. A reader
whose item decoding is unsupported, and only then, opens the object as a
locked image object showing the fallback, keeps each item resource's
descriptor and decoded bytes and every slot, and saves the record back with the
same items, slots and fallback, in packages, sessions and worker transfers alike.
Without a fallback, unsupported items make the artwork unsupported.

**Vocabulary.** Item records: Path (kind 1) with a nonzero layer-local ID. A writer
allocates IDs in sequence from `next_item` and advances it, so deleting items
never lowers it; undo restores an earlier object and its counter. Path tags: `geometry` (required), `fill_rule` (`u8`: 0 nonzero,
1 even-odd; default nonzero), `style` (nested Fill and Stroke records painted
first to last; default empty), `hidden` (`u8` flag), and the ignorable,
copy-safe `name` (UTF-8) and `locked` (`u8` flag), and the ignorable
`live_shape` recipe. Fill (kind 2) and Stroke
(kind 3) records have ID 0 and a required nested `paint` holding one paint
record; the only paint kind is Color (kind 4), whose `color` value is a `u8`
space (0 `srgb`, 1 `display_p3`, 2 `adobe_rgb`, 3 `pro_photo`), a `u8` form (0 encoded
RGBA, 1 linear RGB) and four F32 values, as portable colors define them. Stroke
tags are `width` (varuint in 1/256 layer pixels, below 2³¹, default 256), `cap` (`u8` 0 butt, 1 round, 2 square;
default butt), `join` (`u8` 0 miter, 1 round, 2 bevel; default miter), `miter_limit`
(F64, at least 1, default 4) and `width_profile`.

| Value | Encoding |
| --- | --- |
| Geometry | Varuint subpath count, at least one. Per subpath: varuint anchor count (at least one) and a `u8` flags byte (bit 0 closed; other bits are unsupported). Per anchor a `u8` header, `kind << 2 \| in_present << 1 \| out_present`, with kind 0 corner, 1 smooth or 2 symmetric; then the point as two zigzag varints in 1/256 layer pixels, as a delta from the previous anchor of the path (the first anchor from 0,0); then the incoming and the outgoing handle offset, when present, each as two zigzag varints in 1/256 pixels relative to the point. An absent handle is zero, except that a symmetric anchor's absent incoming handle is its negated outgoing handle. Values of 2³¹ units or more are unsupported, and so is a value that F32 cannot hold exactly. Segment *i* is the cubic from anchor *i* through its outgoing handle and the next anchor's incoming handle; a closed subpath adds the closing segment. A single-anchor subpath is a dot. |
| Width profile | Varuint knot count, at least one. Per knot: a varuint segment delta from the previous knot (the first counts from segment 0), segments counted across subpaths; a `u8` flags byte (bit 0 `t` is 0, bit 1 `t` is 1, bit 2 the right factor equals the left; bits 0 and 1 together are invalid and bits 3–7 unsupported); then F32 `t` in `[0,1]` unless bit 0 or 1 is set, the F32 left factor, and the F32 right factor unless bit 2 is set. Knots strictly ascend by (segment, `t`). Each side's offset is half the stroke width times its factor, interpolated linearly in segment plus `t` and held beyond the ends. |
| Live shape | `u8` kind (1 rectangle, 2 ellipse, 3 polygon, 4 star), then F64 center x and y, width, height and rotation in radians; a rectangle adds four F64 corner radii from the top-left corner clockwise in its own frame, a polygon an F64 corner count and a star an F64 point count and an F64 inner radius as a fraction of the outer one, in (0, 1]. Counts are whole numbers from 3 to 100; the first corner or point sits at the top of the frame. Then 16 bytes: the first half of the SHA-256 of the path's kind word followed by its encoded required properties (`geometry`, `fill_rule`, `style`, `hidden`). |

A reader treats an anchor kind it does not know as a corner when editing and
keeps it otherwise.

**Live shapes.** A rectangle, ellipse, polygon or star keeps its frame and its
corner radii, corner count or points and inner radius as a recipe beside the
geometry it generated. A reader that knows the recipe checks
the fingerprint on open and detaches a mismatched recipe, keeping the geometry;
nothing is regenerated on open. Moving, rotating, uniform scaling and flipping
update the recipe, and so does scaling a square-cornered shape along its own
axes; other transforms and geometry edits detach it. A reader that does not know
the recipe renders the geometry and, because the tag is not copy-safe, drops the
recipe when the path changes. An unaccepted live-shape value is kept as unknown
data under the same rule.

The fixed [`vector-objects.capy`](../../crates/layer-core/src/package/codec/fixtures/vector-objects.capy),
generated by `cargo run -p layer-core --example vector_fixture -- OUTPUT.capy`,
covers every V0 feature and must reopen and save byte for byte. Its hidden Codes
layer holds every color space in both forms, symmetric anchors with implied and
explicit incoming handles, an Items-class unknown property, a chunk stored raw
and a `next_item` past deleted IDs. The fixed
[`vector-future.capy`](../../crates/layer-core/src/package/codec/fixtures/vector-future.capy),
generated by `cargo run -p layer-core --example vector_future_fixture -- OUTPUT.capy`,
holds a readable layer whose path keeps an ignorable Slots value naming an image
and an unknown `example.font/1` resource, and a layer from a newer version whose
required Slots value opens it from its fallback. It must open editable and save
byte for byte.

**Unknown data.** An unknown kind or tag that is not ignorable, or has a reserved
bit set, makes the object unsupported: it opens from its fallback, or makes the
artwork unsupported when it has none. An ignorable one is kept byte for byte
and ignored for rendering. A copy-safe one survives every edit. One that is not
copy-safe is dropped when its record's required properties change. An unknown
parent kind is atomic: its children are never interpreted and an ignorable one
hides its subtree. A known tag on another kind, an unaccepted value of a known
required tag and admission bounds (1,048,576 records per object and anchors per
path, 16 MiB per property, depth 64) are unsupported.

**Invalid** framing: truncated or trailing bytes, non-minimal varuints,
unordered or repeated tags, unbalanced End markers across the object, a record
count that disagrees with the stream, nonfinite F32 or F64 values, duplicate item IDs,
a path without geometry or ID, a paint entry without a paint, negative widths
or factors and width knots outside their path.

## Restricted ZIP64 transport

The reader admits terminal metadata before invoking ZIP parsing with a known
zero archive offset. It checks raw central-directory file-type attributes as
well as local/central agreement; creator-OS permission normalization cannot
admit links or directories. Directory discovery never scans member payloads.

Resource packs, previews, signatures and `mimetype` use method 0 (STORED).
`manifest.json` uses method 8 (raw DEFLATE) when its encoded bytes are smaller
than the UTF-8 JSON, otherwise STORED. There is one bounded DEFLATE stream, with
no trailing bytes or concatenated streams. The declared decoded metadata length
is checked before allocation; decoding must consume exactly the encoded length
and produce exactly the declared length. The manifest CRC covers decoded JSON.
Resource CRCs and byte-range offsets retain their existing stored-byte meaning.

All members have zero flags, no encryption or descriptors, and matching
size/CRC/name/version fields in local and central headers. Names are
relative ASCII paths without empty, `.` or `..` components, backslashes, drive
prefixes, NULs or case-insensitive aliases. Readers currently admit names up to
255 bytes; longer names exceed admission rather than indicate corruption.
No directory entries, symlinks,
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
Unreferenced members outside `data/` are optional: readers ignore their content
while enforcing transport bounds and header rules. Edited saves omit them; Copy
Original preserves every original byte. Every `data/` member must be indexed by
a resource record, including packs; unindexed data is invalid.

Local members occupy consecutive ranges beginning at byte zero. Their central
directory follows immediately, with one entry per member in physical order and
no duplicates. The end records follow the directory with no gaps. A reader checks
both views of each member and rejects competing directories or overlapping data.

For sizes/offsets/counts below their classic maximum, use the classic value and
omit the corresponding ZIP64 value. At or above `0xffffffff` for U32 fields or
`0xffff` for U16 counts, use that sentinel and the actual U64 value. ZIP64 extra
field `0x0001` contains exactly the saturated fields in specification order:
uncompressed size, compressed size, local-header offset, disk number. Local
headers include only sizes; STORED sizes are equal and saturate together. The
bounded compressed manifest has distinct encoded and decoded sizes. No
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

The fixed `preview.png` is at most 512 pixels on each axis and 8 MiB encoded.
It is a non-interlaced 8-bit PNG with an explicit sRGB chunk and standard sRGB
interpretation, RGB when every pixel is opaque and otherwise RGBA with straight
alpha, containing the complete default output framing at a uniform scale. PNG
chunk CRCs, dimensions and decoded allocation bounds are checked before display.
HDR uses the output's authored SDR rendition. No checkerboard, current selection,
mask-inspection tint, guides or editor chrome appears in it. Reduced size and SDR
status remain explicit when it is displayed as the only available representation.

The default output's `representation` is
`{"member":"preview.png","size":[width,height],"color":"srgb"}`. It
is emitted together with the member by current writers. Readers adopt it only
when its fields, member, size and color exactly match an independently verified
preview. Unknown fields, future colors or members, oversized dimensions and
other unusable representations mean no preview; they never change artwork
support or contribute authored references. Its pixels
represent that output's captured source roots and context exactly. A source save
may omit both the representation and member when preview evaluation/encoding
fails or exceeds its budget; never retain an older preview as current. Missing
preview does not excuse failed source capture, resource integrity or publication.
Normal save workers attempt this preview through the shared snapshot renderer,
with a 128 MiB pixel-planning ceiling and bounded row readbacks. Unsupported output
framing or unavailable GPU evaluation yields a source-only save.

The codec returns `OpenOutcome::Candidate` for understood authored records and
verified resources. Shared editor admission must validate color-system support,
shader/evaluator support and device budgets before constructing an editor. A
candidate is not proof that a particular GPU or color backend can execute it.
The complete shared opening operation yields these outcomes:

| Outcome | Permitted operations |
| --- | --- |
| Editable | All retained required semantics and resources are valid, supported and admitted; construct the authored model and normal editor. |
| Preserved | Structurally valid but unsupported semantics, envelope, device admission or evaluator support; retain immutable source, list understood outputs, show independently verified representations, copy original bytes and export a representation to a new destination. |
| Recovered view | Native content is invalid, but the separately verified bounded preview is readable; retain original bytes and allow explicit image recovery/export. It is never a successful native editable open. |
| Failure | The container cannot be safely parsed or native content is invalid and no verified representation is available. Retain the original instead of adopting partial artwork. |

Unsupported packages without previews remain Preserved and permit copying the
source and reporting any understood output inventory. A preserved/recovered view
never constructs an editable `Document`. It cannot replace the original with
flattened pixels, acknowledge an editable save, or
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

`package::transport::spool` copies native readers, including pipes, into a private
owned file using bounded buffers. `BackingReader` adapts a ready immutable owner
to the archive reader's checked U64 seeks. A host providing pending ranges must
finish supplying those ranges on its worker before calling the synchronous codec.
`ChunkedBytes` supports bounded in-memory owners without retaining a whole archive
through a tiny read. `PreparedResources` can enumerate or stream immutable blocks
independently of `PreparedPackage` and its ZIP writer.

Native workers may use retained immutable file ranges. A mutable destination or
revocable provider is spooled into private owned storage before adoption. Streams
are copied in bounded chunks before random-access ZIP parsing. Browser workers
retain package/Blob or private-store owners and service bounded range requests;
U64 values cross bridges as strings or explicit integer halves, never JavaScript
numbers. No input callback reads files, decompresses samples or copies pixels.
The integrated baseline uses bounded eager preparation through this interface.
Optional M4 demand loading remains separate work; hosts do not build another full
archive or recreate an artwork schema beside shared Rust.

A shared capture from the ordered editor owner contains immutable artwork roots,
document/session generation, edit checkpoint, working-state generation and output
context. Resource enumeration is independent of ZIP assembly and does not mutate
a cloned document to detach payloads. Unchanged resources retain owner/block
identity across captures. Metadata capture may share immutable roots while raster
publication remains pending; workers await those publications cancellably.
Active contact/predicted pixels and uncommitted operations are excluded.

Output `context` contains an `effect_phases` array
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
atomic replacement, tab close and renderer loss. The private
[`session` codec](../../crates/layer-core/src/package/session.rs) reuses these
resource identities, encodings and record adapters with complete checkpoint
metadata. Selection, targets, camera, tab order and bounded history remain
outside the portable manifest. Historical record versions and payload owners
are shared across checkpoints. Raster `tiles` and vector `items` lists are split
into metadata groups of at most 64 entries, ending after an entry whose CRC-32
is divisible by 8, so an insertion changes only nearby groups. Immutable image
IDs are interned across paint revisions; the
private wrapper expands back into the ordinary artwork records before validation.
Manual-save identity stays separate from recovery
publication. Private sessions preserve all retained ancillary data, including
records excluded by portable edited-save filtering.

Private program descriptors carry authored code and parameter data. Execution
uses the independent approval boundary in
[session recovery](../internals/session-recovery.md); serialized descriptors
cannot supply approval. Locally captured program owners remain separate from
wire and disk bytes.

`PreparedSession::metadata` and `resources` expose the private checkpoint without
archive assembly. `open_parts` independently checks resource integrity, complete
artwork/working state, history transitions and budgets; it returns an error for
invalid or unsupported state and never omits history to make restoration succeed.
`SessionMetadata` separates lightweight host JSON from immutable color profiles.
Private `metadata_profiles` bindings retain ICC resource identities through disk
checkpoints and trusted worker transfer. Readers validate the same profile-size,
resource-kind and integrity limits as artwork profiles; returned metadata keeps
its profile owners attached. Camera changes reuse the existing ICC payload.
Native storage publishes immutable payloads before their metadata, retains the
previous complete generation and pins resources held by readers. Durable close
membership and host lifecycle behavior belong to shared session policy.
Ownership locks explicitly unlock when their final owner is dropped; duplicated
or inherited file handles cannot extend the editing session's ownership.

## Worker transport

`package::transfer::PreparedTransfer` captures the same typed artwork and final
manifest adapters without assembling an archive. Its private descriptor also
carries runtime store slots, checkpoint, verified resource receipts and optional
working selection. These execution fields are outside the portable manifest.
Workers retain compact handle identity, including vacant slots, so returned
results can be matched to the captured source targets.

Payload chunks are bounded by `MAX_RANGE_BYTES`. `TransferReceiver` adopts
complete verified payloads into immutable owners; incomplete transfers fail.
Coverage word storage is shared across aliases. Worker admission verifies external
bytes before this trusted internal transfer, so main-thread adoption does not
repeat image decoding, color-profile preparation or decoded-content hashing.
Tile receipts retain cached fingerprints of descriptors and encoded bytes.
Worker color results intern the complete immutable image, tile and profile
closure against the captured artwork; unchanged resources reuse their owners and
replacements keep fresh identities. Receiving owners never hash or decode to
recover identity.
Unchanged encoding receipts and copy-safe ancillary payloads retain their exact
bytes and resource identities.

The Web host sends bounded transferable buffers and yields between chunks.
`PreparedSessionTransfer` extends the trusted transport with working targets,
bounded inverse edits, next edit/stroke identities and host session metadata.
History uses the same per-record adapters; the UI owner does not replay complete
historical drawings to send a captured editor to its worker. Resource receipts
remain transient and are admitted only after independently validating storage
bytes in the worker. The private codec's exhaustive field/variant boundaries and
headless restart/Undo/Redo fixtures require new editor features to classify their
persistence behavior.

Its worker streams package writes into private browser storage, returning a file
handle for publication. Preserved packages keep the original browser `Blob`;
showing their summary or preview does not reconstruct an editable document.

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
    {"id": "00000000000000000000000000000002", "type": "capy.composition/2", "data": {
      "size": [64, 64],
      "result": {"object": {"ref": "00000000000000000000000000000003"}, "port": "color"}
    }},
    {"id": "00000000000000000000000000000003", "type": "capy.stack/1", "data": {}},
    {"id": "00000000000000000000000000000004", "type": "capy.output/2", "data": {
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
| Known object with unknown `future_mode`, enum/port, evaluation contract or resource encoding | Preserved even when hidden or unplaced. |
| Two groups reference the same stack, or an unplaced second occurrence references an already used paint source | Preserved; direct paint-reference counts alone do not establish support. |
| Unknown non-ancillary record with no path from the root/output | Preserved; it is retained authored work. |
| Unknown ancillary copy-safe note references an existing occurrence and opaque resource | Editable; preserve note/resource unchanged after unrelated edits. |
| Same note without `copy_safe`, or after deletion of its referenced occurrence | Editable; drop note on edited save, retain byte-for-byte on no-edit copy. |
| Ancillary record refers to another ancillary record | Editable; the referring record is dropped on edited save and kept by no-edit copying. |
| Required artwork depends on ancillary data | Invalid. |
| Known future root with empty outputs and no default | Structurally valid; preserved if outside the editable subset. |
| Supported source, missing or corrupt preview | Editable source; no stale preview is adopted. |
| Corrupt required source block with valid preview | Recovered view; never substitute empty source. |
| Source capture succeeds and preview rendering/encoding fails | Source-only save succeeds; no representation/member survives. |
| Interrupted/stale write, provider pipe, browser bounded ranges, ZIP64 size/offset/count threshold | Preserve original publication and newer work; no imprecise bridge integers or unbounded whole-package copy. |
| Save after rate change; renderer recreation; retained analysis/bake during later painting | Saved/reopened output uses captured roots and integrated phase; old resources live through accepted jobs. |

The checked-in `codec/fixtures/authored-filters.capy` covers all 52 built-ins, exact
raster samples, watercolor state, LUT samples, SDR rendition and proof, every blend
name, clip and effect attachments, a Pass Through group, signed layer and linked
mask offsets, contour selection geometry, every ruler kind, non-default built-in
choices and gradient interpolations, and both color forms, using the current
occurrence and filter data versions. Its fixed values exercise opening, editing,
saving, reopening and compiling current filters. `builtin-contracts.json` fixes
all 62 built-in contracts, 54 filters and 8 paper textures, and their 314 parameter keys: kinds, choice IDs,
dimensions, accepted bounds, constraints, color domain, alpha behavior and time
use. Parameters are keyed; control order, shader parameter order, runtime ABI
and displayed unit labels are excluded. Choice order may change because built-in
shader codes map explicitly from stable values. Reader regressions distinguish future record/resource types,
unknown descriptors and admission limits from inconsistent known descriptors,
malformed data and failed integrity checks.
The separate `codec/fixtures/shared-image-objects.capy` fixes shared immutable
image IDs, both paint-base policies, offsets crossing tile boundaries and both
object interpolation choices. Its tests compare hardcoded F64 coefficient bits
and source U16/F32 sample bits through portable open/save, private checkpoints
and serialized worker transfer. It, `vector-objects.capy` and the Web
`shared-image-f64-*.capy` packages were last converted to the `object` content
key by re-saving them through the current writer, and must reopen and save byte
for byte; `unsupported-pass-through.capy` was converted in place and stays
preserved.
`codec/fixtures/deflate-resources.capy`, generated by
`cargo run -p layer-core --example deflate_fixture -- OUTPUT.capy`, stores a
U16 RGBA paint tile, U8 RGB image tiles, vector items, a LUT and XMP metadata
with the DEFLATE encodings. Its test compares exact decoded samples and values,
saves them back with LZ4 and rejects trailing input and wrong decoded lengths.

Keep these inputs fixed while their data versions remain supported. Adding a
built-in, widening an accepted numeric range or adding a stable choice does not
require updating the baseline. Add separate fixed inputs for new authored data.
An intentional data-version change requires a concrete conversion and tests
opening the unchanged released fixture, editing and resaving it. Do not replace
the baseline with output from the new writer to make a compatibility failure pass.
Superseded pre-release formats have no conversion readers.

`illustration-conversions.capy` fixes Brightness to Opacity 1 and Threshold 2,
including single-color output, binary alpha and the hidden alpha threshold after
switching back to Keep. The pre-release Threshold 1 entry in the original
52-filter fixture is replaced with Threshold 2's complete neutral values;
its raster samples and every other authored value are retained.

Retain exact-byte/source/material/profile/LUT/selection assertions from the existing
codec tests when replacing their envelope fixtures. Retain the integrated-phase
oracle in `snapshot/tests.rs`, publication and ownership assertions in
`raster_storage.rs`, Save/Undo/Redo policy tests and source-analysis lease tests.
Round trips alone do not establish semantic fidelity, compressed-byte reuse,
allocation bounds or host qualification.
