# M5–M6 artwork inspection, presets and export

[Photo editing M5–M6](photo-editing-m5-m6.md)

Status: **PROPOSED for review**. This specification owns ADJ-1 sampling transport,
ADJ-3 interaction, ADJ-10, VIEW-2/3, BAR-5/6 and IO-3. It describes required work,
not implemented behavior or measured performance. GTK, Web and Android deliver
first; Apple/macOS/iPadOS and Windows follow the same shared contracts.
[Color contracts](photo-editing-m5-m6-color.md) own adjustment mathematics,
calibration feasibility, page domains and precise curve-point editing.

## Ownership and existing integration

Shared Rust owns source selection, validation, state, readouts, geometry, undo, labels and
availability. Hosts present small views, forward native input and own storage, focus, capture, hold
timing and scheduling timestamps. Extend the existing query/capture, property, panel, bar,
preset-store and export paths; do not add a second compositor, histogram window manager or general
task framework.

| Contract | Existing code to extend |
| --- | --- |
| Capture/source identity | `crates/layer-render/src/lib.rs`; `crates/layer-render-wgpu/src/{artwork.rs,color_sample.rs,scene/stack.rs,scene_images.rs,scene/sources.rs}` |
| Statistics/reduction | `crates/layer-core/src/color/histogram.rs`; `crates/layer-render-wgpu/src/{tonal.rs,region_sources.rs,region_requests.rs}` and existing `tonal.wgsl` |
| Picker/property gestures | `crates/layer-ui/src/{color_picker_session.rs,eyedropper.rs,effects.rs,numeric.rs}` |
| Panels/bars/holds | `crates/layer-ui/src/{session.rs,layout.rs,layout_presets.rs,customization.rs,canvas_bar.rs,toolbar_components.rs,shortcuts.rs}` |
| Proof/presentation | `crates/layer-ui/src/{proof_workflow.rs,screen_status.rs}`; `crates/layer-render-wgpu/src/{present.rs,proof_view.wgsl}`; `crates/layer-host/src/scene.rs` |
| Presets/admission | `crates/layer-core/src/effects.rs`; `crates/layer-ui/src/{effects.rs,filter_loading.rs,export_presets.rs}`; renderer `effects.rs::validate_namespace` |
| Export | `crates/layer-ui/src/{export.rs,export_presets.rs}`; `crates/layer-host/src/export.rs`; `crates/layer-color/src/{resize.rs,output_rows.rs}`; renderer `snapshot/output.rs` |

New type/action names below are **proposed API sketches**, not existing symbols. Use neighboring
request/take/cancel methods and notification plumbing rather than an unrelated service. Small readout
publications must not rebuild the layer list or replace focused Properties controls at pointer
frequency.

## One bounded artwork-query path

| Proposed source | Exact meaning |
| --- | --- |
| `Visible` | Current document artwork including visible paper; excludes checkerboard, selection/Quick Mask tint, clipping marks, cursor, comparison, proof and monitor conversion. |
| `LayerContent(id)` | Selected paint/source layer content placed in document coordinates, **before its masks, opacity, blend and effects**. Use the retained placement evaluator, including nonlinear placement and renderer triangle order for folds. A group/adjustment without directly sampleable content is unavailable. |
| `Reference` | Existing `Document::reference_snapshot` membership/composition; preserve original renderer layer indices and existing clipping/group policy. |
| `EffectInput(id)` | Actual input to that adjustment before its own parameters/opacity/mask; excludes upper layers and uses existing `stop_before`/`input_scope` clipping and nearest-isolated-ancestor rules. |
| `EffectBaseline(original)` | Current composition substituting the active gesture's original effect layer; gesture-local metadata, never a second full image. |

Existing raw-local `ColorSampleSource::Layer` is not the Selected layer contract. Retain it only for
explicit raw-local consumers while migrating adapters; do not silently reinterpret local coordinates
or leave competing queues permanently. Carry the requested source through `Scene::capture_region`; a
mutable stop flag cleared by preparation cannot identify effect input correctly. Public values are
straight linear document RGB after conversion from the shader's working input. Embedded Curves/Levels
statistics use the selected page's mathematical input: channel pages before their channel operation;
RGB master after channel operations.

Each request/result carries epoch, renderer generation, request serial, source identity/generation,
document color/geometry, frozen artwork identity and `Frame.time`. Explicit destinations additionally
carry stable layer ID, program identity, parameter/page identity and session generation. Source
identity includes all pixel-bearing dependencies, live drafts, placement/masks and raster publication;
committed document revision alone is insufficient. Ignore camera/overlays/labels. A batch captures
metadata and animation time once; all chunks use that frame.

Explicit correction/targeted/Auto completion may accept **time-only advancement** when its underlying
source, destination and all other identities still match: it applies the sample at frozen
`Frame.time`. A source edit, changed parameter draft, color/geometry change or raster publication
invalidates it. Separate time from source generation so animation does not make every released
correction impossible. Background results identify their captured time; never combine chunks from
changed pixel backing. Cancel a scan before admitting more chunks if backing changes.

One measurement batch runs, with one replaceable newest request per visible consumer. Priority is
explicit correction/targeted contact, active sampler/pointer, embedded editor statistics, independent
panel statistics. A histogram yields after one bounded chunk. Allocate/reuse scratch and summary
buffers lazily. At most ten persistent points plus the pointer are admitted together; hover coalesces,
whereas a released correction retains its release point until matching completion.

Encoding cannot decode cold sources, drain uploads/resources, wait for GPU work or capture an entire
image. Replace blocking capture admission (`Capture::would_block`, `region_into`, source
preparation/retirement) before relying on asynchronous mapping. Use existing native workers/render
jobs and Web asynchronous source preparation. Pending admission returns pending and wakes on
readiness; it is not an empty sample. GPU work reduces colors/bins; CPU combines reduced chunks and
formats small results. It does not read/classify complete images or footprint texels. Retained scratch
obeys existing artwork-query image admission, including filter-image windows.

Cancellation stops new chunks, safely drains accepted callbacks and drops results from old owners.
Errors retain request identity. A failed optional query preserves valid artwork and exposes shared
retryable status without per-frame notices; an explicit correction/Auto reports a reason and performs
no edit. Closing the last consumer releases retained references and scratch; no panel retains a
canvas-sized float image. Renderer loss clears readouts and requires a new generation.

### Samples and Histogram

Calibration and Info use point or circular linear averages of width **1/5/15/51/101**, default **5**
for calibration. Floor finite document coordinates to the pixel; width 1 selects that pixel, other
widths include integer offsets inside the circle `dx²+dy² <= (width/2)²`. Clip to document edges
without rescaling. For N admitted pixels with straight RGB c and alpha a, return `sum(a*c)/sum(a)` and
`sum(a)/N`; alpha-zero RGB has no weight. Empty inside and Outside canvas are distinct. Reject
nonfinite values/invalid alpha; never clamp extended RGB before reduction. The ordinary paint picker's
established Oklab averaging stays unchanged.

Independent Histogram defaults to Visible/RGB, with RGB and Y channels and Source choices Visible,
Selected layer, Reference, Selection. Selection means Visible restricted to coverage **>0**;
absent/empty selection is unavailable. A partially selected pixel counts once, not by fractional
coverage. Every included alpha>0 pixel counts once after unassociation; alpha0 contributes only
transparent count. Histogram bin totals therefore differ deliberately from alpha-weighted samples.

Preserve `Histogram`'s 256-bin semantics: U8/U16 RGB uses the document profile's encoded bins, Y its
existing linear bins; float RGB/Y uses log-linear bins with nonpositive bin zero, F16 positive
-12..+16 stops and F32 -149..+128 stops. Use document-primary luminance `Y=g+yr*(r-g)+yb*(b-g)`.
Counters are `below <0`, `above >1`, `black <=0`, `white >=1`; exact zero/one remain inclusive
endpoints. Do not clamp before classifying. Reject extents over 32768² before dispatch: maximum
included count 1,073,741,824 fits a u32 reduction counter. Keep the CPU implementation as a small
independent test oracle, remove production CPU scans.

Live Preview uses a deterministic grid `gx=min(W,256), gy=min(H,256)` of at most 65,536 nearest
original pixels at `floor((2*i+1)*W/(2*gx))` and corresponding y. Transparency/selection filtering
does not replace rejected points; never use averaged mip texels. At most one Preview starts per 100
ms. Complete an admitted Preview despite newer effect-value drafts while retained pixel backing
remains valid; publish captured identity as Updating, then take the newest request. Changed source
descriptor/document/color/geometry or pixel backing invalidates it. Exact starts after 200 ms settled
and reduces full-resolution originals. Capture outputs/classification are at most 256² (65,536 texels)
per owner poll/submission; next chunk only after prior completion. Dependency halos use existing
image-budget planning; split down to edge 16, then return a visible error if still inadmissible.
Sparse Preview evaluates only original grid coordinates with shared prepared source tiles, never
builds a full composite first. These proposed bounds require performance validation. Publish
Preview/Updating/Exact and captured identity; Preview numeric totals are estimates. Same-source prior
data may remain visible as Updating; changed source/document/color/geometry clears immediately. Never
call stale data Exact/current. Share results between consumers when source and bin domain match.
Use 64 atomic shards/64-lane compute as in tonal reductions: four-channel display bins use 256 KiB, three-channel Auto 4096 bins use 3 MiB. Reserve at most 4 MiB reduction buffers plus one aggregated readback and separately admitted capture scratch; fold shards on GPU and do not allocate display and Auto families simultaneously.

Levels Auto requires an exact matching source/page/percentile domain and consumes the dedicated
higher-resolution summary defined by the color contracts, not the 256 display bins or tonal luminance
bins. Selecting channel/log-count display is view state. Admission constants land with query schemas.

Histogram and embedded tonal controls offer Shadows/Highlights clipping preview. Straight artwork RGB
tests any channel <=0 or >=1; alpha0 is unmarked, both tests use a distinct combined pattern. Float
labels name **SDR** bounds. Presentation-only marks stay below selection/handles and never enter
readings/export/proof. Active clipping preview temporarily takes precedence over gamut warning and layer-mask area tint,
preserving their prior toggles. Mask tint is suppressed during clipping inspection so it cannot alter
the artwork classification; selection outlines and handles remain visible. No selection or history edit is created.

### Finite-range reduction arithmetic

The sums are mathematical expressions, not permission to overflow GPU Float32.
Implement sample RGB reduction in two GPU passes: first maximum absolute covered
premultiplied RGB component m (plus validation), then normalized RGB sums using
`rgb/m` when m>0 and alpha sums. Normalize only covered RGB; alpha0 RGB never
contributes. Read back m, normalized sums, alpha sum, included count and flags;
combine/unscale these few scalars in f64 on CPU, validate finite representable
f32 final sample and publish. m=0 supplies RGB zero without division. A maximum
width101 footprint has at most 10,201 terms, so normalized sums are bounded.
Do not first unassociate each sample texel; that intermediate can overflow even
when an alpha-weighted final average is representable. If a point's required
straight RGB cannot be represented as f32, return explicit unrepresentable
sample rather than infinity. This is new-query arithmetic, not an unrelated
rewrite of paint/composition arithmetic.

Histogram must accept finite premultiplied input with tiny alpha even when its
straight value exceeds f32: existing `Histogram::add` unassociates in f64 and
classifies this valid input. RGB clipping counters compare premultiplied channels
directly against zero/alpha; HDR positive bin log is `log2(p)-log2(alpha)`, with
no overflowing division. For Y, normalize premultiplied RGB by maximum absolute
component k, use neutral-preserving `q=g/k+yr*(r/k-g/k)+yb*(b/k-g/k)`, and classify
the sign/exponent of `k*q/alpha` without constructing an overflowing result.
Use mantissa/exponent decomposition for products/comparisons and bitcast-based
subnormal normalization where native GPU arithmetic would flush a positive
subnormal to zero; do not assume `log2(subnormal)` is portable. RGB SDR values
outside `[0,alpha]` take endpoint bins before any division; bounded ratios use
the existing transfer/bin helper. This is a small shared measurement helper
with extreme-value oracles, not a color engine rewrite. Reject invalid source
premultiplied values/alpha, not an otherwise classifiable extended ratio.
Auto/calibration may separately reject unrepresentable editable anchors or
returned f32 samples. Add near-F32-maximum positive/negative, opposing-channel,
tiny/subnormal-alpha and representable averaging fixtures; no silent clamp,
drop, CPU pixel fallback or host-specific exception.


## Picker and targeted-adjustment interaction

Add optional picker metadata/actions to existing property descriptors; Color parameters retain
UseCurrentColor. Canvas color destination stores a sampled tagged `RgbColor`; calibration is one
effect/group action applying its complete validated solution. Arming preserves paint/mask colors and
records original effect/tool. The bottom bar says Click a black/white/neutral point, with Sample size
and Cancel. Mouse/pen tap selects; finger uses existing native hold/lifted loupe. Calibration glass
shows the sampled source color at that coordinate, with a checker while pending. This bounded
swatch avoids showing the corrected canvas as though it were the pre-effect input. Hover previews location/readout
only. Successful solve is one ReplaceLayer undo item; invalid/empty/unrepresentable input keeps the
picker armed with a reason.

Targeted Curves uses Drag on the image and Done. Request one pre-effect sample per contact, map x by
the selected page contract, reuse a nearby interior point or add within the 32-point limit, then fix x
and drag output vertically in logical surface distance. Up raises/down lowers the selected curve output independently of
zoom/rotation. Shared `EffectAction::Gesture` owns one undo step. Pending sampling retains latest
delta and release state; no point exists until admission/result succeeds. Empty/domain failure or full
table changes nothing. Done commits completed drags and restores the prior tool; Escape/blur restores
unfinished gesture then exits. Graph/numeric point selection, exact axis conversion and keyboard rules
are owned by color docs.

| Event | Required shared transition |
| --- | --- |
| Effect/layer/program replaced, another property edit, document/workspace change | Cancel pending correction/active draft before new action; invalidate destination. No late reply edits a new target. |
| Page change | Cancel correction; targeted mode remains armed on new page after cancelling contact. |
| Another tool requested | Restore/cancel old preview, activate the **requested** tool; remove the existing prior-tool restoration failure in shared lifecycle. |
| Pointer Cancel | Restore current contact; correction can remain armed for retry. |
| Escape/blur/surface teardown | Cancel unfinished action and retire replies; completed earlier edits remain. In-window dropdown preserves ownership. |
| Undo/Redo | Cancel preview first, then apply history; never accept a partial correction because history was invoked. |
| Save/close | Follow existing `require_document_snapshot_idle`; never serialize a draft as accepted artwork. |

BAR-5/6 extend `CanvasBarKind`/context generation using existing ToolOption choices.
Picker/targeted/comparison use bottom placement; selected sampler uses near-object placement.
Essential Cancel/Done/Exit stays reachable when optional bars are hidden. Shared labels, enabled
reasons and accessible names follow visible state.

## Info and persistent samplers

New sampler width defaults to 5 and readout to Document RGB; pointer Info uses width 1. These are shared session defaults, not additional preferences.

Document stores up to ten stable-ID points: document-pixel position, sample width, readout choice.
Proposed sampler edit has **changes_project=true, changes_image=false**; definitions dirty/save the
project and undo without forcing artwork rerasterization. Reject duplicate IDs/nonfinite
positions/unsupported widths. Never serialize readouts, hover, generations or GPU state.
Add/move/delete/size/readout are reversible edits. The disabled Add reason is You can keep up to 10
color samples.

Color sampler joins the existing picker family; Info offers Add sample. Tap selects, drag starts
immediately for mouse/pen/finger, release accepts one undo item, cancel restores original position.
Bar offers Delete/Sample size/Readout. Markers use stable logical-size numbered crosshairs. Selection
alone is transient. Whole-document resize/crop/turn/flip maps points alongside guides in the same
transaction; non-destructive crop retains outside points. Individual layer transforms leave points
fixed. Save/open restores definitions and recomputes values.

Info shows pointer location/value over the canvas and one numbered row per point. Readouts are
Document RGB (named profile, encoded 0..255 in SDR), linear RGB/EV and OKLCH using document
primaries/shared conversion; alpha is separate. Linear HDR retains negative/above-white data; zero has
Black rather than infinite EV; negative channels show their signed linear value and an unavailable EV. Shared formatting uses three decimals for encoded 0..255, six significant digits for linear values and two decimals for EV/OKLCH, with scientific notation where fixed decimals would erase a nonzero value. Optional HSB/HLS reuse existing routines independently of paint-wheel
shape. Empty inside and Outside canvas are distinct; pending data never fabricates zero/new colors.
Active effect gesture can show Before/After from its original layer/current values; after
commit/cancel return to Current. Explicit comparison labels its own sources. Values are artwork values
regardless of visible proof/SDR/split branch.

Query only while Info is actually visible or a sampling gesture requires it; persistent markers alone
do not cause continuous readback. Stationary points update on live artwork changes. Keys include
sampler-definition generation to prevent late results being relabeled after deletion. Theme/monitor
changes do not reinterpret points. Add Histogram/Info through Panel/PANEL_NAMES/ALL, customization and
duplicate host view factories. Photo opens Histogram with an adjacent RGB Waveform tab in the
expanded column above Properties and Layers, replacing Color/Palettes. Window owns panel access;
Color and Palettes remain available there. Info remains a secondary inactive tab beside
Navigator/Proof. Preserve customized layouts. Reuse actual tab/column/drawer/floating/app visibility
gating from renderer telemetry. Histogram command becomes
show/focus panel, without ellipsis or second command.

## Before/After

Proposed session `ComparisonState` owns source/layout/divider/contact/momentary ownership. It is
transient: no visibility edit, history, project metadata or export change. Default is vertical split
at 50%; Before left/top, After right/bottom. Layouts are Before, After, vertical/horizontal split; bar
identifies source and Exit. Divider hit/drag is immediate in logical work-area coordinates, clamped,
stable under camera changes and keyboard accessible. Cancellation restores contact-start position;
source/layout changes are view edits. Exit restores exact prior view.

| Source | Before / After | Availability |
| --- | --- | --- |
| Effects | Adjustment-kind effects bypassed / normal composition; generators/fills, placement/crop, masks, opacity, blend and visibility remain | Visible adjustment exists |
| Print proof | Ordinary unproofed view / saved print recipe | Recipe exists and proof LUT ready; reuse ProofView status |
| SDR appearance | HDR master under normal HDR route / saved/draft SDR appearance | HDR document and supported route; SDR-only display labels both as mapped previews |

Momentary Effects Before is **unbound by default**. Reuse existing `ShortcutAction::Momentary` and
held restoration/custom bindings/pen buttons; suppress in text input. Release on key-up, blur, modal
opening and canvas cancel; never add a host key handler or steal a binding. Persistent touch mode has
Exit. Starting an editing tool exits comparison before activating it. Renderer loss exits and retires
pair resources. Both sides evaluate the same animation timestamp.

Effects uses retained alternate composition at display window/resolution sharing source/paint backing;
key includes artwork identity, scale/window and override. Divider-only movement presents the retained
pair without rerasterization. Bound memory by existing display working-set policy. Proof/SDR branches
after artwork acquisition in ViewportPresenter, with one final common monitor/output conversion. Cover
normal/overview/minified/loupe routes; overlays stay main-view and Navigator remains ordinary. Keep
common HDR-capable surface when either side requires HDR; whole-view proof/SDR booleans cannot force
both sides to SDR. Share artwork tone-guide analysis. No full-resolution comparison bitmap or second
document renderer.

## One-filter settings and saved presets

Copy Filter Settings/Paste Filter Settings/Save Filter Preset… are Properties actions for one
effect/generator. Copy works on locked source; Paste needs unlocked
same-kind/original-program-compatible target and replaces only effect instance, preserving
name/mask/opacity/blend/visibility/clipping/parent. A different filter is chosen through existing
Filter Types replacement. Application/window-owned internal settings slot survives source close/tab
switch; it does not replace pixel/system clipboard. Proposed
`EffectSettings={instance:Arc<EffectInstance>,source_color}` retains complete program/immutable LUT
payloads, no live layer or baked pixels.

Preserve numeric/curve values verbatim and tagged color/gradient defining spaces. Numbers remain
relative to destination working space, with no appearance promise or conversion wrapper. Equal program
can be shared; otherwise retain embedded program. Never use `rebind` to silently default incompatible
values. Apply the destination admissibility contract, including bundled F32 widened ranges; refuse the
whole application with parameter/reason if unsupported. Custom declared contracts and
shader/resource/finite-output admission remain effective.

Saved category reuses Filter Types search/eight-item preview cache. Shared Save as new/Rename/Remove
uses stable IDs. Choosing a preset factors `EffectAction::Insert`: existing drawer
replacement/insertion/clipping/selection-to-mask policy, one undo. Library CRUD never dirties artwork.
Proposed separate `EffectPresets` store bounds 64 entries, trimmed 1–80 Unicode-character control-free
names unique by shared Rust lowercase comparison, 64 MiB serialized bytes including self-contained
programs/payloads and existing per-program limits. Do not put saved programs in EffectCatalog. Preview
key includes library generation/stable ID/full settings; no previews during an effect gesture.

Validate structure/shader/resources before library publication, and combined **live WGSL namespace**
before application. Same ID and equal program shares it; same ID unequal source or different IDs with
conflicting declarations refuses atomically with This filter uses a different version of a filter in
this drawing. Hash IDs cannot solve declarations. Source/library and destination remain untouched. A
valid stored preset may be inapplicable to this drawing without being deleted. No text namespacer or
catalog substitution.

One pending validation carries request/epoch/target/effect identity/candidate/intent.
Deleted/locked/changed target, cancellation or stale document drops the result; explicit reinvocation
is required after target change. Commit existing Edit::Batch only after validation/idle boundary;
failure preserves redo/selection/history. Use separate effect-presets file/key and existing worker
store locks/atomic writes/ IndexedDB transactions; validate fresh state before applying mutation.
Failed writes preserve old library; validate/apply against freshly read state under the existing host
store lock (GTK expected-version atomic write, Web navigator.locks plus IndexedDB transaction, Android
store lock). Invalid startup stores report error and are not silently replaced; explicit save may
replace unreadable pre-release state while preserving its error indication. No Settings expansion,
migrations or cloud/library framework.

## Export draft and processed rows

ExportDraft/ExportForm owns size/resolution/quality/sharpening actions, NumericControl constraints,
final extent and errors. Hosts own incomplete text only; remove local recipe assembly. Preserve
profile/depth/metadata normalization. PPI is Master or 1..65535 and changes metadata only; JPEG
quality is 1..100. Recipes save requested mode, not resulting dimensions. Final extents must be
1..32768 per axis and meet codec caps (including WebP). Use checked u64 arithmetic.

| Size mode | Shared contract |
| --- | --- |
| Original / Fit | Original exact; existing aspect-preserving bounds with enlargement off by default |
| LongEdge / ShortEdge | Requested max/min axis; half-up rational rounding of other axis, min 1; enlargement off by default; reject excess other-axis limit |
| Percent | 0.01..3200.00%; basis_points 10000=100%; >100% authorizes enlargement without second toggle; half-up per axis |
| Megapixels | 0.01..1073.74 MP in hundredths; f64 square-root scale, floor axes/min 1, never exceed integer pixel budget; enlargement off by default; reject unrepresentable aspect/limit request |

Sharpening choice Off/Low/Standard/High defaults **Off** for every destination. After existing
linear-premultiplied RowResampler, before tone/profile/quantization and gain-map split, use Gaussian
unsharp sigma .75 final pixels, radius 3, amount 0/.25/.5/1. Extend edge pixels; normalize seven-tap
Gaussian kernel. Blur premultiplied RGB and alpha together; derive blurred straight RGB by coverage
quotient, sharpen straight center `c+A*(c-blur)`, then multiply original center alpha. Preserve alpha;
transparent centers exact zero; reject nonfinite results; retain extended RGB. Off is exact
pass-through without sharpening ring. Gate exact-source bypass on Off so sharpening cannot be skipped;
with Off preserve its hidden straight RGB behavior.

Shared sequential row wrapper admits at most seven input rows plus one output row; include
ring/resampler in checked encode memory. Restart with resampler for codec passes. Native HDR/shared
SDR/Web workers consume the same processed working rows; remove duplicate resize setup. Both gain-map
renditions use sharpened common rows, with existing document-space tone guide sampled at output
positions. OpenEXR allows explicit sharpening and finite extended floats. This CPU stage remains on
explicit file-delivery worker, not canvas/input; whole-frame codec memory remains separately admitted.
Provider error/cancel uses existing CaptureControl and publishes nothing.

### Frozen dialog and actual file size

Freeze one DocumentExport/artwork time per dialog, shared by preview, size and final; Web/Android must
stop separately capturing preview/final. Calculate File Size runs one full metadata-complete encode
into private artifact **on demand**, reports actual File size, and Export reuses it if snapshot/full
recipe match. Any recipe change clears size and invalidates prepared output; no full encode on slider
motion. Export without calculation prepares once then publishes. Preview retains its purpose; if it
does not decode actual lossy bytes, retain its truthful compression-exclusion label. Never infer bytes
from thumbnail or open/truncate target during preparation.

Proposed prepared descriptor contains owner document ID/epoch/frozen revision/time, recipe
generation/full recipe/extent/byte count/statistics; host artifact handles stay private. One active
preparation and one replaceable pending recipe. Stale completion releases artifact and cannot restore
preview/size/HDR status. Dismissal cancels/drains and removes it. Use GTK worker temp, Android
app-cache temp and Web worker OPFS token/Blob. Web requires existing isolated-worker OPFS synchronous
handle: unavailable storage returns existing error, never an unbounded Wasm/in-memory file. Retain Web
50-ms cancellation polling/worker termination and per-job Web Lock discard; startup cleanup removes
only unlocked abandoned jobs. Enforce existing file cap, PhotoMemoryBudget (Web fallback assumes 512
MiB available, half admitted for encode) and codec admissions; row streaming alone proves no total
limit.

### Export Again and publication

Export Again has no default shortcut; available after selected drawing's successful export in this app
session. It freezes a **fresh** snapshot with last successful recipe/target. Regular dialog opens with
that drawing's last recipe/destination; new drawing starts current Web/Share recipe. Session's
ephemeral stable-ID/epoch file state holds portable recipe/opaque host token; host maps token to
target/name. Keep across tab parking/GPU recreation, clear on close/replace. Never serialize in
project/recovery/settings/library. Deleting preset cannot lose concrete recipe.

Only successful publication updates it and ExportPresets::remember. Cancel/error/ stale owner/access
failure preserves prior success; preference-write failure reports unsaved preferences while retaining
successful target in memory. Never clear project dirty/checkpoint. Revalidate editable-master
protection and target before publish. GTK retains atomic write/publication boundary. Web retained
handle uses permission from gesture/createWritable/abort; download fallback starts another named
download with existing acknowledgement, never promises overwrite. Android encodes privately before SAF
publication and disables cancellation during copy; provider failure cannot promise atomic replacement.
Revoked/missing/read-only target opens existing picker with recipe/name; cancel leaves old target for
retry, no silent alternate delivery.

## Integration and acceptance

Use the [execution packets](photo-editing-m5-m6-execution.md) for file ownership, dependencies, host replacement paths and checks. The following acceptance cases supplement those packets.

Required no-window regressions cover every stale identity, time-only versus source change,
release-before-result, contact cancel/blur/menu retention, redo preservation, sampler
save/open/geometry/dirty-without-image, visibility and momentary restoration. Hardware fixtures
compare known bins/readouts/pre-effect inputs; comparison sides match single-view proof/SDR and
original/export hashes, share time, have no split seam; divider moves no composition revision.
Delivery tests use independent sharpening oracle, exact source identity,
extended/high-depth/transparent values and both gain-map renditions, file/provider/storage failures
and no published partial artifact.

GTK/Web/Android journeys run in light/dark, docked/floating/Filters drawer, normal/narrow layouts,
mouse/pen/finger and hardware keyboard where supported: correction/targeted Curves undo and pending
target switch; Histogram/Info scopes/live sliders/hide/reopen, ten sampler drag/delete/crop/save;
three comparison sources/divider/hold/Exit; copy across drawings/save/restart/preset conflicts;
2048-long-edge/Standard sharpening, preview/Calculate File Size/export/edit/Again/revoked
target/cancel/retry. Accessible labels/readout equivalents and focus must survive updates. Use
existing Recorder, GPU snapshot/reduction, GTK native, headed WebGPU and private-ID Android harnesses;
runner additions become executable claims only once implemented.

Measure affected reference-tier rows by the existing performance guide: live tonal sliders with
embedded statistics, Histogram+Info+10 points, width101 resident/cold picker, comparison
divider/zoom/effects cache and HDR/proof route; export 24-MP/deep/HDR wall time/peak CPU+GPU+temp
bytes/cancel latency. Record moving-frame cadence/query latency/current measurements in tier tables.
This document establishes no runtime success or target met; implementation completion requires these
checks and explicit reporting of failures/unverified hosts.
