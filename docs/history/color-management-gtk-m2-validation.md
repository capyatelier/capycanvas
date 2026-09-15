# GTK SDR color milestone 2 — implementation and validation

Work begins at `e46f271` on 2026-09-13. Milestone 2 is **in progress**;
this report is not a declaration that the new modes are qualified. Scope is
shared implementation and GTK integration. Other host integration requires the
user's approval after GTK qualification.

Current delivery status (2026-09-15): the GTK development build implements
integer8/integer16 SDR editing in sRGB, Display P3, Adobe RGB and ProPhoto;
retained sources and native masters; revisable photo corrections and masks;
exact artwork sampling and histograms; tagged color entry, palettes and managed
viewing; Assign/Convert/precision changes; and profiled PNG/JPEG/TIFF delivery
with preview, resizing, reusable recipes and physical resolution. Display mips,
bounded composite/filter windows and losslessly backed paint caches are present.
The later checkpoint sections record implementation and reproducible evidence.

**GTK is not yet qualified as complete.** The closing work is the affected
correctness/recovery matrix and any failures it exposes, managed-display and
supported-renderer coverage, followed by fresh fixed/parent/current frame-creation
comparisons, combined peak/steady CPU/GPU budgets and large-document interaction
latency. Active edit pins and simultaneous workers are not covered by the
individual cache ceilings alone. Earlier measured failures remain open until
new measurements resolve them. Coarse-first work or additional scheduling is
required where those measured gates fail, rather than as an independent feature
checklist. The recorded toolbar/workspace native harness failures also remain
unqualified. Other platform hosts still require approval after GTK qualification.

**Current work order (user instruction, 2026-09-14):** finish functional milestone 2
implementation and correctness/recovery validation first. Further benchmarking,
regression investigation and optimization are deferred to the final qualification
phase. The earlier measured failures remain open and must be resolved before GTK
release qualification; this sequencing does not waive any performance or memory gate.

**Filter accuracy (user instruction, 2026-09-14):** edited filters need perceptually
equivalent results, not exact historical pixel parity. Use practical numerical
tolerances and visual review. Lossless persistence, exact undo and unchanged
integer16 identity remain separate guarantees.

## Independently checked prerequisites

The milestone 1 archive/replay replacement is present: immutable compressed
256×256 raster revisions, affected-tile history, asynchronous capture, indexed
project storage, GTK atomic publication, and failed-worker recovery. The
historical research document's descriptions of linear8 painting and stroke
archives do not describe this checkout. Current painting and physical effect
boundaries use `Rgba8UnormSrgb`, with linear-premultiplied Float32 shader values.

Initial prerequisite audit (completed work and remaining limits are recorded
in the implementation sections below):

- Pixel descriptors, plane validation, source stride calculations, GPU formats,
  sampling and export accept only sRGB8/R8. Extend the complete precision path;
  a 16-bit container or retained original alone is insufficient.
- Raster restoration decodes every replacement into a simultaneous CPU vector.
  Bound scratch while retaining atomic failure behavior and exact undo.
- Source CPU pixels, a full immutable GPU source, materialized paint pages and a
  full composite coexist. Image-boundary filters retain further full-resolution
  textures. Bound and account for source, composite, effect and output work
  before qualifying dense photos.
- The working-format example establishes equal-kernel format costs, not the
  accuracy/cost of a complete integer16 edit pipeline. Repeat it and add
  operation-specific precision checks before selecting working buffers.
- Prior native pacing measures software input-to-presentation feedback. It is
  not physical pen-to-photon measurement or calibrated-monitor qualification.
- Apple and Windows milestone 1 host lifecycle qualification remains incomplete
  in the historical evidence. It is outside this GTK implementation phase.

## Reference and acceptance policy

Use the [common gates and milestone 2 scope](color-management-milestones.md),
with exact sample/profile persistence, one-step conversion undo, declared
operation tolerances and separately measured CPU/GPU/presentation boundaries.
Investigate unchanged-path p95/p99 increases greater than
`max(5% of baseline, 0.20 ms)`, against both the fresh fixed baseline and the
parent of each significant implementation stage. Repeated baseline runs retain
noise; missing budgets/measurements do not pass a new mode.

Primary references checked during implementation:

- [W3C color conversion definitions](https://www.w3.org/TR/css-color-4/#color-conversion-code)
  for standard-space transfer, primary matrices and D50/D65 adaptation.
- [PNG third edition](https://www.w3.org/TR/png-3/#11cICP) for metadata precedence
  and matching encoded samples and color tags.
- [Pinned wgpu texture contracts](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html)
  for format capabilities; normalized16 is not an automatic transfer decoder.
- [Little CMS](https://www.littlecms.com/color-engine/) as an independent ICC
  reference. Installed native reference library: 2.16.

## Fresh baseline

Fedora 44, kernel `7.1.10-200.fc44.x86_64`; NVIDIA RTX PRO 6000 Blackwell Max-Q,
Vulkan driver 610.57.04, 97,887 MiB GPU memory, configured power limit 250 W.
Use `LAYER_GPU_INDEX=0`; record adapter PCI identity in the kernel/native runs.
Hardware access requires running outside this session's device sandbox.

Rebuild the unmodified checkout with `cargo build --release -p layer-bench
-p layer-linux --examples --bins`. Run without concurrent builds or other test
processes:

```sh
LAYER_GPU_INDEX=0 target/release/gpu-bench --scenario all --repeats 3 \
  --output-dir artifacts/color-m2/baseline-images \
  --report artifacts/color-m2/baseline.md
```

Repeat with `baseline-repeat` paths. The first run covers 10,920 frames, with no
Move/Pen-up completed sample over 8.33 ms. Per-scenario CPU frame p95 ranges
0.080–2.082 ms; p99 ranges 0.144–4.748 ms. The 4.748 ms tail is the small
dual-texture scenario and requires the repeat/noise comparison. Capture
allocated/reserved peak reaches 184.2 MiB. Setup, compilation, export and
presentation are excluded from these timing distributions. Generated artifacts
remain local; subsequent sections retain qualification measurements here.

GTK workflows, dense-photo budgets, integer16 accuracy, profiled interchange,
managed-view tests and final regression qualification are still outstanding.

## First implementation stage: restoration and averaged sampling

Raster restoration now prepares GPU replacements before publication while
decoding one tile at a time. A bad later tile leaves the entire live revision
intact. This removes the additional full-document decoded CPU vector; it does
**not** yet bound driver upload staging or eliminate the old/new GPU pages needed
for atomic replacement. The dense workload's process peak does not improve
materially because its source/export allocations dominate. Do not infer a total
memory saving from the scratch change alone.

GTK's eyedropper exposes Point, 3×3 average and 5×5 average alongside its existing
visible/raw-layer selection. Samples use artwork before view overlays, clip the
square to canvas bounds, average linear-premultiplied color and coverage, then
unassociate. Alpha-zero RGB contributes nothing; a fully transparent sample
retains the paint color. Sampling keeps brush opacity independent and does not
dirty artwork. One reusable 128-byte buffer and one asynchronous request bound
the GPU readback. Changing size cancels stale results. Other host controls are
not enabled by this stage.

Validation passes: 128 GPU tests (18 separate hardware workloads ignored), three
GPU project integration tests, and 369 shared UI tests. Numerical area sampling
matches an independent Float64 reference within `1e-6` in linear channels and
coverage, including four-page boundaries, sparse missing pages, clipped canvas
edges, hidden transparent RGB and buffer reuse. The new native GTK test activates
each size button and samples a transparent/red source through the production
worker, checking color, opacity and unchanged document revision. Existing native
file and diagnostics/fault/restart workflows pass. Native RGB presentation of
the sampled primary uses the same `1e-6` tolerance for Float32 transfer arithmetic.

The fresh baseline repeat also passes 10,920 frames; maximum per-scenario CPU
p95/p99 is 2.536/3.034 ms. The first implementation run passes another 10,920
frames, with maximum CPU p95/p99 2.004/2.663 ms and no Move/Pen-up deadline misses.
No per-scenario CPU p95/p99 exceeds the larger fresh baseline plus the declared
investigation threshold. Seven native pacing workloads pass 5,055 frames. Worker
render-plus-present wall elapsed p95/p99 (ms), measured separately from
main-thread frame creation and from worker thread CPU time:

| Workload | Baseline | First stage |
| --- | --- | --- |
| G-Pen | 0.396 / 0.536 | 0.394 / 0.508 |
| Natural Blender | 1.055 / 1.196 | 1.075 / 1.286 |
| Wet Round | 0.830 / 0.936 | 0.827 / 0.968 |
| Watercolor | 1.775 / 2.007 | 1.723 / 2.034 |
| Pan | 0.283 / 0.309 | 0.278 / 0.300 |
| Hand | 0.275 / 0.287 | 0.262 / 0.290 |
| Transform | 1.723 / 2.285 | 1.678 / 1.800 |

The 24/45/60 MP and two-document workloads pass exact tile-digest/archive and
undo/export checksum comparisons. Baseline versus first-stage undo/redo times
are 14.80/13.61 versus 14.76/13.58 ms (24 MP), 21.39/19.75 versus 21.22/19.66 ms
(45 MP), and 20.80/18.98 versus 20.77/19.05 ms (60 MP). These are individual
bulk-operation observations, not p99 distributions. Cumulative process high-water
is 1,672,664 KiB before and 1,676,200 KiB after; large-image memory work remains.

The clean repeated working-format experiment uses 50 warmups and 300 samples,
16 passes/sample. At 4096², matched sample/source-over completed p95/p99 is
1.829/1.898 ms for UNORM16, 1.838/1.987 ms for FP16, and 10.441/10.514 ms for
Float32. This rules out assuming that full-image Float32 blending is inexpensive;
it does not establish complete integer16 editing precision. An earlier kernel run
overlapped compilation and is excluded from qualification.

Reproduction uses `stage1-*` local artifacts and `working-formats.md` under
`artifacts/color-m2`. Build native tests separately, then use
`bash tools/performance/gtk-raster.sh TEST_BINARY TEST_FILTER REPORT_PREFIX` for
`native_sdr_sampling_controls`, `native_document_files`,
`native_diagnostics_and_gpu_failure_recovery` and `native_frame_pacing`.
The runner sets `GDK_DEBUG=no-portals` because the file test drives GTK's in-process
fallback chooser; its initial missing-chooser failure was a runner configuration
error. Production retains its normal portal selection.

## Second implementation stage: bounded source upload

Image initialization no longer retains a full-size immutable GPU source. It
packs one 256² encoded tile, uploads through at most 64 mapped COPY_SRC buffers,
and performs the existing GPU decode/premultiply operation into paint pages.
Native upload chunks wait on the render owner for their exact queue submission;
GTK input does not wait. Upload buffers plus CPU/GPU source scratch peak at
16.50 MiB. A 2049² test crosses the 64-tile boundary and partial image edges,
checking every exported code and source Arc sharing. All 129 GPU library tests,
three project tests, native GTK files and fault/recovery workflows pass.

The 24/45/60 MP and two-document workloads retain exact archive, undo/redo and
export checksums (`8356e7bd`, `86475ef0`, `50c21c68`). Repeated current cold
source-generation/device/initial-submission observations are 761.54/617.44 ms,
902.55/807.90 ms and 1067.20/1095.61 ms, respectively. Initial host backing is
845.24/700.93, 1028.23/935.27 and 1237.19/1274.03 ms. These single observations
include device setup and cannot isolate decoder or upload latency.

The old renderer residency counter omitted the immutable source. The workload
now also reports wgpu's allocator live/reserved totals, including tracked
staging but excluding driver-private allocations. Rebuild `e46f271` in an
isolated archive with only this reporting addition to reproduce the comparison:

| Workload | Baseline live / reserved MiB | Tile upload live / reserved MiB |
| --- | --- | --- |
| 24 MP | 352.85 / 704 | 259.01 / 640 |
| 45 MP | 589.56 / 896 | 417.38 / 640 |
| 60 MP | 765.61 / 1280 | 533.38 / 1024 |
| Each of two 24 MP documents | 352.85 / 704 | 259.01 / 640 |

The instrumented baseline's cumulative process high-water is 1,654,028 KiB;
two current runs reach 1,615,064 and 1,606,540 KiB. Final two-document RSS falls
from 1,305,656 to 1,248,916/1,249,556 KiB. CPU source bytes, eager paint pages,
full composites and image-boundary filters remain; this stage does not qualify
the complete photo residency requirement. Browser queue draining is also still
unqualified; no other host controls are enabled.

Both complete 25-scenario current runs pass 10,920 frames each with no completed
Move/Pen-up sample over 8.33 ms. Initial p95/p99 investigation triggers required
a paired rebuild of parent `5114481` and alternating isolated repeats. Parent
and current full-suite maxima are 2.229/2.999 and 2.387/2.958 ms CPU p95/p99.
The remaining three Move CPU triggers do not reproduce consistently in
current/parent/parent/current order, five repetitions per invocation:

| Workload | Parent p95 range / p99 range ms | Current p95 range / p99 range ms |
| --- | --- | --- |
| Wet Round Oklab | 1.534–1.573 / 2.019–2.110 | 1.522–1.534 / 2.007–2.140 |
| Opaque Gouache | 2.045–2.100 / 2.841–3.008 | 2.086–2.133 / 2.596–2.727 |
| Wet Watercolor | 2.147–2.198 / 2.884–3.144 | 2.109–2.257 / 2.856–3.031 |

Initialization runs only on reset, and these brush workloads have no imported
source. Both revisions show tail variation; the isolated Move distributions
overlap and no consistent added cost was found. Pen-up tails have fewer samples
and vary too (Wet Round parent 1.734–1.882 versus current 1.637–2.243 ms); do not
treat a short-run p99 as a stable estimate. Final native input/presentation and
sustained pen-up qualification must still be repeated after later pipeline work.
Artifacts: `source-tiles*`, `baseline-allocator*`, `paired-*`, `isolated-*`.

## Third implementation stage: color and source services

The new native `layer-color` library provides reusable Float32 Little CMS
transforms, exact embedded profile ownership and depth-preserving source codecs.
It is not yet wired to GTK Open, the document renderer or other hosts. Existing
document mode remains sRGB8. Profile, integer depth and conversion intent/BPC
have independent shared types. sRGB, Display P3, Adobe RGB (1998) and ProPhoto
definitions use Float64 reference mathematics with unclipped matrix/Bradford
conversion; every 16-bit code survives transfer decoding through a Float32
storage boundary and re-encoding in the reference test. This is not yet a
complete GPU integer16 precision qualification.

Native transforms use `lcms2` 6.2.0 / `lcms2-sys` 4.0.7, dynamically linked here
to Little CMS 2.16. The sys crate's static fallback is a different library version
(2.19) and is unqualified. Transforms own their thread context, use Float32
formatters with optimization/cache disabled, preserve linear alpha separately,
and expose intent/BPC explicitly. RGB identity is a copy after validating a
usable transform, preserving hidden RGB and every sample. Standard-space CMM
results agree with the independent Float64 reference within 0.0003 encoded
channel units for tested in-gamut interiors; ICC fixed-point matrix/TRC tags
differ from analytic definitions. This tolerance is for profile conversion,
not identity storage or future editing. Arbitrary device profiles, BPC numerical
corpora, managed display and GPU conversion still need full qualification.

The shared source representation holds immutable compressed 256² tiles. Reading
or writing rows decodes one 256-row band. Source compression uses zstd level 1;
interactive 8-bit raster capture retains its existing -20 setting. Integer16
tile blobs reversibly separate component-byte planes before compression and
restore the original little-endian bytes before digest validation. There is no
sample quantization in this permutation. Snapshots share source tiles with Arc.
Eight EXIF orientations normalize sample positions using at most four decoded
input tiles and one output tile, while retaining the same profile/depth. During
normalization both compressed source and output can coexist.

PNG output streams rows with matching profile/depth and flush errors propagated.
TIFF reads strips/tiles directly, preserving RGB/gray/CMYK channels and 8/16-bit
codes before any CMM conversion; output streams strips with an embedded ICC and
explicit straight alpha. JPEG RGB/gray decoding retains its 8-bit samples and
profile. Untagged ordinary RGB/gray sources record the sRGB assumption. Profile
channel mismatches and ambiguous untagged CMYK fail explicitly.

Inspection of pinned codec source found two silent-fallback hazards. zune-jpeg's
ICC accessor returns None for malformed chunk sequences; a bounded marker scan
now distinguishes corruption from absence and supports all 255 ICC chunks.
png 0.18.1 discards iCCP decompression errors; chunk preflight detects an unreadable
declared profile. PNG follows cICP → iCCP → sRGB → cHRM/gAMA precedence, handles
supported sRGB/P3 cICP values, and rejects unsupported/HDR transfer encodings.
The PNG test writer must explicitly write cICP because that crate's Info encoder
does not emit the field. References: [PNG3 color chunk precedence](https://www.w3.org/TR/png-3/#4Concepts.ColourSpaces),
[ICC embedding rules](https://www.color.org/technotes/ICC-Technote-ProfileEmbedding.pdf),
[zune-jpeg API](https://docs.rs/zune-jpeg/0.5.15/zune_jpeg/struct.JpegDecoder.html).

The source tests cover exact 8/16-bit PNG/TIFF pixels and ICC bytes in all four
working spaces, hidden RGB, partial tiles, row cache bounds, orientation,
corrupt/conflicting profiles, metadata precedence, gamma-only PNGs, allocation
limits and failed output. Existing 49 core, 49 engine, 129 GPU library and three
GPU project tests pass; GTK checks successfully. ImageMagick-produced RGB16,
gray16 and CMYK16 TIFFs, tiled ZIP and big-endian LZW TIFFs, and an RGB8 tagged
JPEG round-trip through the source runner with exact decoded tile digests and
embedded ICC bytes. ImageMagick reads the emitted depth and profile descriptions.
This is codec interoperability evidence, not the final external-editor workflow.

Reproduce source measurements with `cargo build --release -p layer-color
--example photo_sources --offline`, then, with no concurrent build/test work:

```sh
target/release/examples/photo_sources generate 6000 4000 /tmp/source24.tiff
target/release/examples/photo_sources generate 8192 5504 /tmp/source45.tiff
target/release/examples/photo_sources generate 8192 7324 /tmp/source60.tiff
```

Repeat with `generate_noise` for independent random RGB16 samples (alpha includes
0, 1, 257 and 65535). These are deterministic source stress fixtures, not a corpus
of camera photographs. The smooth/ramp fixture is very compressible after byte
shuffling; the random case guards against inferring a universal compression ratio.
All numbers below use the final shuffle/level-1 representation:

| Size | Ramp / random retained MiB | Decoded band MiB | Random generate / output / reopen+compare ms | Random process high-water KiB |
| --- | --- | --- | --- | --- |
| 24 MP | 3.41 / 137.58 | 12 | 394.25 / 294.13 / 285.82 | 299936 |
| 45 MP | 6.67 / 258.36 | 16 | 704.47 / 535.50 / 503.63 | 552084 |
| 60 MP | 8.87 / 343.78 | 16 | 951.47 / 723.10 / 672.87 | 727012 |

Process high-water includes both retained source and reopened source for exact
comparison. At 60 MP, RSS immediately after random source generation is 357820
KiB. With the initial -20/interleaved representation, the ramp source alone was
457.78 MiB; level 1 without shuffling reduced it to 321.93 MiB. Final source
compression improves residency at a measured cold CPU cost; no interactive paint
compression policy was changed. Artifacts: `source*-prophoto16*`, `source-codec*`,
`color-foundation-*`, `external-*`, `handoff-*`. Final editing budgets still need
to include GPU working/composite/filter residency, history, other documents and
concurrent save/export rather than only these source components.

Remaining codec work before GTK workflow qualification: JPEG delivery and CMYK
JPEG/YCCK conversion; full metadata/DPI policy; grayscale-alpha and associated-alpha
TIFF handling; multi-page selection; planar TIFF (the pinned chunk API documents
an incomplete-plane bug); interlaced PNG above its explicit full-decoded-image
limit; further HDR/ambiguous-profile recognition; cancellation/progress integration.
Current default codec allocation limit is 128 MiB, retained compressed source
limit 512 MiB and dimension limit 32768. These are implementation ceilings, not
qualified whole-document memory budgets. Source-backed renderer composition,
copy-on-write painting and all exposed color journeys remain outstanding. Native
source persistence is implemented in the next stage below.

## Fourth stage: physical precision and native source ownership

The `sdr_precision` example compares identical Float32 arithmetic through physical
`Rgba16Unorm`, `Rgba16Float` and `Rgba32Float` intermediates. It uploads every
16-bit gray code, decodes to linear premultiplied working values, executes zero,
one or 64 physical passes, and encodes to integer16. The independent Float64
reference covers identity, 64 exposure multiplications, 64 low-alpha source-over
steps and two-tap linear resampling in all four standard spaces. The Float32
acceptance tolerance was declared as at most two RGB codes and one alpha code
before the successful run; measured results are stricter:

| Physical working format | Worst identity RGB error | Worst edit RGB error | Worst alpha error |
| --- | --- | --- | --- |
| Linear UNORM16 | 325 | 23156 | 0 |
| FP16 | 33 | 671 | 16 |
| Float32 | 0 | 1 | 0 |

Values are integer16 code differences, not percentages. Float32 identity changes
zero codes across all 65,536 opaque gray values in each space. Its edited RMS
error is 0.113–0.224 code. This supports bounded Float32 working tiles with
integer backing. It does not qualify every brush, effect or ICC transform, and
does not establish hidden-RGB identity through a premultiplied edit surface.
Untouched source samples retain their exact straight representation.

Reproduce with `cargo build --release -p layer-render-wgpu --example sdr_precision
--offline`, then `LAYER_GPU_INDEX=0 target/release/examples/sdr_precision`. The
recorded adapter is PCI `0000:f1:00.0`, Vulkan 610.57.04. Normalized16 render targets
require both `TEXTURE_FORMAT_16BIT_NORM` and adapter-specific format capabilities;
the initial missing-feature validation error was corrected before measuring.
Float32 uses unfilterable texture bindings and explicit loads, without requiring
Float32 filtering. Ten warmups and 100 measured submissions per case include CPU
view/bind-group construction and queue completion. Typical Float32 64-pass p95
is 0.87–0.96 ms per 256² tile, with noisy p99 outliers. These are cold submission
costs, not production hot-frame or pure GPU timings. The earlier 4096² equal-format
experiment still demonstrates why full-resolution Float32 copies are unsuitable
as an automatic replacement. Raw results: `sdr-precision.log`.

Layers now own immutable tiled source handles. Duplication, composition snapshots,
undo and file snapshots share them; cleared history releases sources absent from
the document. History accounting includes distinct source indices, compressed
tile allocations and embedded profiles while excluding allocations already owned
by the current document. Equal samples in separate allocations are still charged.

Archive version 2 has explicit image/layer indices, a shared content-addressed
tile pool and binary ICC payloads with independent SHA-256 checks. Reader
preflight validates dimensions, roles, coordinate coverage, offsets, references,
tile counts and retained-source budgets before reading payloads. Tiles are
decoded one at a time for integrity checking. Duplicated layers share the same
source after reopening. Saving reuses compressed tiles and exact ICC bytes;
there is no old-version reader. Existing packed8 host resources remain a separate
valid input contract until their respective host integrations are replaced.

Source-only native archive measurements, using the random ProPhoto16 fixtures
from the preceding stage and no concurrent compilation or test work:

| Size | Retained source MiB | Archive write ms | Reopen + exact comparison ms | Process high-water KiB |
| --- | --- | --- | --- | --- |
| 24 MP | 137.58 | 49.27 | 255.56 | 289208 |
| 45 MP | 258.36 | 82.09 | 450.87 | 538452 |
| 60 MP | 343.78 | 113.87 | 605.68 | 712428 |

High-water includes both original and reopened sources, without GPU composition.
Writes include buffered output flush but exclude file synchronization and atomic
publication; GTK retains its existing durable publication path. Reproduce by
building `photo_sources`, then running `photo_sources roundtrip INPUT.tiff
OUTPUT.capy`. Artifacts: `native-source{24,45,60}.log` and corresponding archives.
This helper stores a photo source in a document; it does not claim the document's
working precision or renderer has been upgraded.

Session creation and GPU replacement reject tiled-source documents until the
renderer advertises actual support. No new photo/color mode is exposed in GTK
yet; other platform integration and qualification remain pending user approval.

Validation: 52 core tests, 13 color-service tests, 49 engine tests and the existing
369 UI tests pass, plus the new unsupported-source adoption test. All 129 GPU
library tests and three GPU project tests pass. The rebuilt GTK host passes
`native_document_files` and `native_diagnostics_and_gpu_failure_recovery` on the
private 120 Hz Mutter display with fatal GTK criticals. The recovery test's
deliberately invalid scissor produces the expected worker failure and restores
the retained checkpoint. Artifacts: `source-persistence-tests.log`,
`native-source-core-tests.log`, `source-capability-test.log`, `persistence-gpu.log`,
`persistence-project.log`, `persistence-gtk-{files,recovery}.log`. No hot-rendering
format changed in this stage; final frame and whole-document memory qualification
remains required after source composition and editing are integrated.

## Fifth implementation stage: source composition and copy-on-write paint

The native renderer can now compose retained tiled originals without first
allocating paint pages for the whole image. A first edit initializes only the
affected paint pages from the original. Restoring an empty raster root reveals
the original again. This is an internal integration stage: the renderer's public
`supports_tiled_sources` capability remains false. GTK Open/Place and integer16
editing must not adopt it until the remaining operation and precision paths are
complete. Existing documents still use sRGB8 paint/composite storage.

Source working tiles are linear premultiplied RGBA Float32. Built-in sRGB, P3,
Adobe RGB and ProPhoto sources upload their original integer samples to a
reusable unsigned-integer texture and decode directly on the GPU. This avoids
both a bounded sRGB intermediate and CPU expansion of every integer pixel into
four floats. Embedded ICC profiles use a worker-owned Little CMS transform
directly into linear destination RGB; alpha is restored independently. Profile
labels never select the analytic path. Current document primaries are sRGB;
the decoder also has native-primary precision fixtures for the future document
contract. Original hidden RGB remains in the immutable integer source, whereas
alpha-zero working pixels are transparent black.

Declared per-scene source limits, enforced before enabling the public path:

- 16 reusable 256² Float32 GPU slots: at most 16 MiB. Eviction reuses the same
  texture in queue order; jobs retain source/coordinate metadata, not another
  converted GPU tile. Cache keys use weak source references so deleting a photo
  does not retain its full original through the cache.
- One RGBA8Uint and one RGBA16Uint input texture, allocated on demand: at most
  0.75 MiB combined. Built-in conversion packs at most one integer row beyond
  the decoded tile; ICC conversion uses one 1 MiB output scratch tile.
- At most 16 source upload buffers in flight across frames, charged until queue
  completion or cancellation of the unsubmitted encoder. Thus staging is at
  most 4 MiB for RGBA8, 8 MiB for RGBA16 or 16 MiB for ICC Float32 output. A full
  queue forces an exact submission/wait on the native render owner. An ordinary
  cold tile can join its frame's submission without an unconditional wait.
- Four worker decoders, using fixed 256-pixel conversion scratch. Built-in CPU
  fallback tone tables contain every possible code (at most 256 KiB each), not
  interpolated curves. Little CMS internal transform allocations still need
  separate accounting in the complete profile/workload budget.

These are component limits, not whole-document qualification. Full composites,
image-boundary filters, export copies and edited paint residency still require
the planned bounding work. Multiple concurrent scenes/documents each have their
own cache and must be included in the total.

Precision validation reads the actual Float32 GPU cache. Both integer depths,
all possible channel codes, four built-in spaces, RGB/RGBA/gray/gray-alpha,
native-primary decoding and conversion into extended sRGB are exercised.
Before acceptance the limits are 2 RGB codes in destination coordinates,
1 alpha code, and 3e-6 absolute premultiplied linear component error. Native
primary cases have zero code errors; conversion to sRGB has at most one integer16
code error and 5.97e-7 linear error. Opaque coverage is explicitly exactly one:
the first shader test exposed approximate GPU reciprocal division at the
integer maximum. The shader preserves that endpoint explicitly.

An initial test additionally converted Adobe RGB through linear sRGB and back;
dark-sample cancellation produced three codes of error. That extra inverse
conversion is not the decoder's one-way destination contract. The retained test
now measures destination coordinates and separately checks native-primary
identity. This observation supports keeping document-native primaries and
retained original samples; it does not qualify arbitrary repeated profile
conversion as lossless. Identity source import/export continues to compare the
original integer bytes and profile, independently of this working-cache test.

The first copy-on-write experiment exposed a separate undo regression: restoring
a raster root requested full-image composition and the changed root also
invalidated every cached filter pixel. Pure-raster undo/redo now schedules a
frame without requesting full composition on a backend that supports raster
damage. The renderer compares tile capture identities and includes uncommitted
GPU changes and watercolor neighbor footprints. Filter metadata excludes raster
root identity; batches/restoration damage identify the pixels to refresh. Other
backends retain their existing full-refresh capability default.

New correctness fixtures compare the displayed composite directly, including a
Gaussian blur after paint/undo with `composite_all=false`; a full export cannot
hide a stale display in these assertions. A 2049×513 source crosses the 16-slot
cache capacity and partial tile boundaries, starts with zero paint pages, edits
one page and restores the exact original. A GPU native-save/reopen fixture
retains integer16 original samples while preserving its sRGB8 edited tiles and
undo/redo. This last fixture is not an integer16 editing qualification.

Reproduce source measurements by building `raster_workloads` in release, then
running `LAYER_GPU_INDEX=0 /usr/bin/time -v target/release/examples/raster_workloads
all --tiled-sources`. It uses the same deterministic opaque sRGB8 samples,
document dimensions, strokes and 31 empty layers as the original dense baseline.
Source generation, compression, device startup and first submission are timed
together; those numbers must not be described as isolated rendering time.
Drawing is also classified by whether the frame caused a source-cache miss.
The native save comparison checks source identity, exact original tile/profile
content and edited raster digests, while the undo/redo comparison checks the
three established export hashes.

The pre-optimization 24/45/60 MP source prototype reached 204/393/584 ms undo and
194/368/554 ms redo. Damage-based restoration reduced the 24 MP case to 36/14 ms.
GPU decoding then reached 25/14, 45/35 and 56/48 ms, respectively, before removing
the unconditional final source-upload wait. Those exploratory measurements are
retained in `source-cow-{24mp,45mp,60mp}.log`, `source-cow-damage-24mp.log` and
`source-gpu-decode-dense.log`; they are not the final qualification result.

Separate warm/cold reporting exposed two-document stalls that aggregate p99
hid. Six isolated repeats reproduced 8–14 ms frames around the first stroke,
with only 0.3–2.7 ms of render-thread CPU and no source-capacity wait. Unlike the
old eagerly materialized photo, the new source had no initial raster capture;
its worker therefore allocated four 16 MiB pinned spare buffers during the
first stroke. Preparation now starts during source loading, and backing readiness
includes preparation completion. No staging budget was raised. Six repeats after
the fix pass all 3,072 frames: per-document cold completed p99 is 4.296–5.898 ms,
with no CPU/completed sample above 8.33 ms. Logs: `source-outlier-multiple-*`
before and `source-prepared-multiple-*` after. The example records current-thread
CPU separately from submission wall time and prints each over-budget frame.

The final stage run (`source-prepared-dense.log`) measures:

| Workload | 24 MP | 45 MP | 60 MP |
| --- | ---: | ---: | ---: |
| Warm/cold drawing frames | 163 / 93 | 126 / 130 | 126 / 130 |
| Warm CPU p95 / p99 ms | 0.318 / 1.926 | 0.337 / 0.474 | 0.329 / 0.537 |
| Cold CPU p95 / p99 ms | 2.178 / 3.346 | 1.824 / 2.318 | 2.201 / 3.555 |
| Cold completed p95 / p99 ms | 2.378 / 4.668 | 2.039 / 2.514 | 2.483 / 3.560 |
| Concurrent save ms / archive MiB | 27.14 / 32.56 | 43.80 / 56.00 | 55.68 / 70.57 |
| Exact archive reopen ms | 185.71 | 330.71 | 435.90 |
| Undo / redo ms | 25.10 / 14.71 | 45.97 / 35.21 | 56.45 / 48.58 |
| Export and checksum ms | 58.79 | 124.84 | 169.30 |
| Actual GPU live / reserved MiB | 221.88 / 640 | 329.07 / 640 | 387.04 / 640 |
| Cumulative process high-water KiB | 629,304 | 861,432 | 1,006,336 |

All 768 measured single-document drawing frames and 512 additional alternating
two-document frames pass the 8.33 ms CPU/completed gate. The established hashes
remain `8356e7bd`, `86475ef0`, `50c21c68`. Two-document final RSS is 782,492 KiB;
the original instrumented baseline was 1,305,656 KiB. The comparable first-24 MP
high-water decreases from 937,100 to 629,304 KiB (or 908,120 KiB for the bounded
upload parent). Do not compare an individual document's peak with the old
whole-run 60 MP peak. GPU counters include application allocations and staging,
but exclude driver-private memory.

This source path adds cold decode work that the fully materialized baseline had
already paid during loading. It is not an unchanged hot-path comparison. Bulk
undo at 45/60 MP still exceeds the earlier 21/20 ms observations: the current
damage representation merges changed pages into a rectangle and reloads
unaffected source tiles inside that rectangle. Disjoint composition damage and
the remaining source-aware operations must be completed before enabling it.
Startup/source generation also includes compression and is slower than the
packed fixture. None of these measurements qualifies the unfinished integer16
document path or replaces native input-to-present qualification.

The unchanged 25-scenario suite passes 10,920 frames (`source-cow-frames.md`),
with maximum CPU p95/p99 2.357/2.924 ms. Investigation compared both fixed
baseline runs and a freshly rebuilt parent `8a4dc00`. Ten initially triggered
scenarios ran current/parent/parent/current with five repetitions per invocation
(`source-cow-abba-*`). No consistent Move increase above the declared trigger
was reproduced. Two pen-up tails received longer, twenty-
repeat alternating runs (`source-prepared-penup-*`):

| Scenario | Parent CPU p95 / p99 / pen-up p99 range ms | Current range ms |
| --- | --- | --- |
| Large Paintbrush | 0.245–0.253 / 0.627–0.632 / 0.600–0.774 | 0.234–0.241 / 0.618–0.649 / 0.559–0.715 |
| Loaded Oil Mixer | 2.009–2.026 / 2.629–2.670 / 2.298–2.309 | 2.024–2.082 / 2.617–2.625 / 2.383–2.588 |

Loaded Oil's larger current pen-up tail exceeds the 0.2 ms trigger in one of
these two runs, but not both; Move distributions do not show the same increase.
This is retained as a tail to monitor during final sustained GTK qualification,
not an allowance to accumulate regressions. Reproduce with `gpu-bench --scenario
NAME --repeats 20 --report PATH --output-dir PATH`, running serially without
compilation or other GPU tests. The parent was built from a `git archive 8a4dc00`
checkout in `/tmp/capy-m2-stage5-parent`; saved parent/current binaries and all
reports remain in the local artifact directory.

Stage validation passes 16 color-service, 52 core, 49 engine and 370 UI tests;
132 GPU library tests (18 hardware-specific benchmarks intentionally ignored in
that invocation) and four GPU project tests. The rebuilt GTK host passes native
file operations and the injected-failure/diagnostics/recovery workflow on the
isolated 120 Hz Mutter display. Source tests additionally verify completion and
cancellation release upload reservations and compare embedded RGB ICC Float32
uploads against the native transform. Logs: `raster-damage-shared-tests.log`,
`source-prepared-{gpu-tests,project-tests}.log`,
`source-prepared-gtk-{files,recovery}.log`. Build tests with `cargo test --offline
--release -p layer-linux -p layer-render-wgpu --no-run`, then run the resulting
executables serially with `LAYER_GPU_INDEX=0`; use the documented
`tools/performance/gtk-raster.sh` wrapper for native GTK tests.

Outstanding integration includes transform capture, raw-layer sampling and
regions, material/smudge neighbor inputs, previews/thumbnails, document working
space/depth, integer16 raster capture/restoration/export, bounded composition and
filters, all GTK color journeys and managed display. No other platform host
integration is authorized or performed in this stage.

## Sixth implementation stage: disjoint restoration damage

Raster restoration now retains the individual changed tile footprints, expanding
each watercolor footprint for its neighbors. Pointwise scene composition visits
only the translated tiles in those footprints. Full rebuilds, painting/preview
cleanup, animated programs and image-boundary effects keep their existing full
damage propagation. Source/composite precision and cache limits are unchanged.

The extended source fixture paints two disconnected tiles in separate frames,
restores the original raster root and verifies both the exact displayed image
and two tiles' worth of composition work. The existing displayed Gaussian-blur
test verifies the image-boundary fallback. All 132 GPU library tests, four GPU
project tests and native GTK file and diagnostics/recovery workflows pass:
`disjoint-{gpu-tests,project-tests,gtk-files,gtk-recovery}.log`.

The same dense source workload now measures undo/redo at **15.23/11.50 ms**
(24 MP), **22.57/14.33 ms** (45 MP) and **18.30/13.46 ms** (60 MP), compared with
25.10/14.71, 45.97/35.21 and 56.45/48.58 ms in the previous stage. These single
bulk-operation observations return close to the original baseline; they are not
p99 distributions. Exact export hashes remain unchanged. All 1,280 measured
single/two-document drawing frames pass the 8.33 ms CPU/completed gate. Actual
GPU live/reserved allocations remain 221.88/640, 329.07/640 and 387.04/640 MiB;
cumulative process high-water is 999,740 KiB (`disjoint-dense.log`). The source
mode remains disabled pending the remaining operation and color integration.

## Seventh implementation stage: original-aware raw sampling

Current-layer point and area queries now read untouched source tiles directly
through the bounded Float32 cache; edited paint pages override those originals.
The mixed-format query uses at most 512 bytes of asynchronous readback (128 bytes
for existing integer8-only queries). It decodes integer paint, averages linear
premultiplied artwork, then unassociates the result. Transparent source padding
and absent pages contribute transparent black. Sampling neither creates paint
pages nor changes the composite revision. The renderer shares source ownership
for queries only while the last submitted layer references that source.

The new 513×257 integer16 fixture crosses four tiles, partial source boundaries
and integer8 paint overrides, including alpha-zero hidden RGB and partial alpha.
Point/5×5 results match an independent Float64 reference within 1e-6; deletion
releases the query's source reference. This qualifies source-aware queries in the
existing sRGB8 document, not integer16 painting or arbitrary working spaces.

All 133 GPU library tests and four GPU project tests pass, followed by native GTK
file and diagnostics/injected-failure/recovery checks. Reproduce using the same
release build and GTK wrapper described above. Logs are
`source-inspection-{build,gpu,project,gtk-files,gtk-recovery}.log`. The new source
map synchronization follows the existing submitted-layer scan; this stage adds
no work to individual brush dabs. Final frame-creation and sustained native
latency comparisons remain required after the rest of the integration.

## Fractional resampling qualification

Float32 texture storage alone does not establish integer16 resampling precision.
The [Vulkan limits specification](https://docs.vulkan.org/spec/latest/chapters/limits.html)
defines `subTexelPrecisionBits`: filtered sample coordinates snap to the device's
subtexel grid. The earlier `sdr_precision` experiment explicitly mixed neighbors
at 0.375, so it did not exercise this hardware limit.

`sdr_sampling` now compares physical hardware bilinear sampling with four
`textureLoad` calls and Float32 interpolation, against a separate Float64 oracle.
Each method evaluates 65,536 fractional positions on opaque edges, four colors,
alpha 1/13/17/257 (integer16 codes), and a transparent/opaque edge, in all four
built-in spaces. Both methods read identical linear premultiplied RGBA32Float
textures; neither includes a narrowing intermediate. The acceptance threshold
remains 2 straight RGB codes / 1 alpha code, including low alpha.

On the reference Vulkan GPU, hardware sampling reaches 1,641 RGB codes of error
on the opaque sRGB/P3 edge, 3,828 in Adobe RGB and 2,032 in ProPhoto. Four-color
errors reach 487–828 codes; low-alpha errors reach 11,116–12,667 RGB codes and
2 alpha codes. Snapping a small positive coverage to zero produces up to 128
alpha-code error on the transparent edge; its lost straight color is also
reported, rather than hidden by an opaque-only test. These are measured output
errors, not an inference from the Vulkan minimum limit.

Explicit interpolation passes every case with at most **1 RGB code and 0 alpha
codes** of error, maximum premultiplied linear error 1.14e-7, and RGB RMS at most
0.0359 codes. This selects explicit Float32 interpolation for precise artwork
resampling. Hardware-filtered display caches must not feed edits, exact queries
or export. Production transform/neighborhood/filter integration and their large
image cost remain outstanding. The tiny 2×2-source/65,536-output kernel measures
roughly 0.031–0.046 ms completed p95, with noisy p99 up to 2.07 ms; those timings
do not qualify a document operation or establish a speed advantage.

Reproduce: `cargo build --offline --release -p layer-render-wgpu --example
sdr_sampling`, then run `LAYER_GPU_INDEX=0 target/release/examples/sdr_sampling`.
The harness uses 10 warmups and 100 timed submissions per case, without readback
inside the timed interval. Logs: `sdr-sampling{,-build}.log`. No production shader
or enabled GTK workflow changes in this experiment.

## Eighth implementation stage: cache ownership and mask damage

Composition metadata now holds weak source identities and omits raster backing
and submitted mask commands. Filter-preview keys additionally retain numeric
raster identities, so a new request detects a changed revision without owning
old tile maps. An active preview request still owns its coherent snapshot until
completion. This closes a source-retention gap in GTK's retained compiled scene
when the last photo layer is removed without another scene composition.

Image-boundary effects compare normalized mask metadata, refresh only dirty mask
tiles on raster restoration/painting, and detect both mask and linked world
offset changes. Previously comparing a normalized cached mask against its live
raster root could refresh the entire mask during unrelated input painting.

The extended 1024×768 source/blur fixture has a backed partial-coverage mask. It
verifies zero mask-copy work during input painting and undo, exactly one tile
on mask restoration, and equality with forced recomposition after restoration
and translation. Keeping the scene while removing the source releases the
original. A separate cache-key test verifies source replacement invalidation and
release of historical raster maps. All 134 GPU library tests, four GPU project
tests and native GTK file and injected GPU-failure/recovery tests pass. Logs:
`source-cache-{build,host-build,keys,mask,gpu,project,gtk-files,gtk-recovery}.log`.

## Ninth implementation stage: source-aware connected regions

Raw-layer connected-region queries classify paint overrides and untouched source
tiles directly into packed eligibility bits. The previous full-document integer8
raw-query texture and copy path are deleted. Existing GPU morphology, connected
components, selection limits and immutable history coverage consume that mask.
The comparison domain remains the current sRGB8 document's alpha-weighted encoded
color; document working-space integration is still outstanding.

Classification uses up to sixteen existing texture views per dispatch, fitting
both the source-cache capacity and portable texture-binding limit. Each batch is
consumed before source slots can be reused. Query parameters are reused, with a
host copy of 64 bytes per tile plus an aligned seed block; at 60 MP this is about
59 KiB each on host and GPU. Four cached bind groups are cleared on the next
document submission so they cannot retain retired paint pages. The packed mask
uses one bit per document pixel; connected-component labels remain four bytes
per pixel, preflighted against the existing device allocation limits. This stage
does not yet replace full composition/filter captures for reference-layer queries.

The 2305×513 integer16 source in a 2560×768 document distinguishes adjacent codes
32768/32769 that would collapse in integer8. Tests cross more than sixteen source
tiles, combine an integer8 paint override, reuse/change query parameters and
selection limits, and check every coverage pixel including connected transparent
padding. No paint pages or composite revisions are created by queries. Clearing
the document releases source ownership and query bindings. All 135 GPU library
tests, four GPU project tests, and native GTK file and injected-failure/recovery
checks pass: `region-accepted-{build,gpu,project,gtk-files,gtk-recovery}.log`.

The initial one-tile-per-dispatch implementation saved memory but increased the
3 MP raw-query GPU p95 from 0.373 to 0.872 ms. Batching reduced it to 0.269 ms.
Fresh-parent comparisons then exposed repeatable CPU tails. Reusing tile views,
parameter buffers and bounded bindings reduced normal CPU work; the final matched
runs do not reproduce the earlier warm p99 regression. Query-owned GPU buffers
fall from **27,132,016 to 14,946,480 bytes** (25.88 to 14.25 MiB). This count excludes
the artwork textures already owned by the renderer and the small host parameter
copy; it is not a whole-process memory measurement.

Two final serial current/parent/parent/current cycles, with a fresh `a93b799`
parent build, produce these ranges across four runs of each binary:

| Raw-query refinement | Parent CPU p95 / p99 ms | Current CPU p95 / p99 ms | Parent completed p95 / p99 ms | Current completed p95 / p99 ms |
| --- | --- | --- | --- | --- |
| Plain | 0.077–0.137 / 0.091–0.184 | 0.060–0.089 / 0.089–0.226 | 0.760–0.981 / 0.895–1.049 | 0.695–0.910 / 0.784–1.026 |
| Antialias | 0.098–0.169 / 0.116–0.199 | 0.057–0.088 / 0.074–0.159 | 0.913–1.121 / 1.026–1.155 | 0.882–0.952 / 0.951–1.033 |
| All refinements | 0.126–0.172 / 0.137–0.242 | 0.065–0.099 / 0.074–0.317 | 0.969–1.174 / 1.030–1.297 | 0.919–0.980 / 0.979–1.160 |

The established harness issues 150 requests per case and reports its last 120
observations, plus the first request separately. Logs now additionally identify
all CPU submissions over 0.6 ms. Occasional roughly 2.1 ms raw-query spikes remain;
phase instrumentation locates them in `wgpu::Queue::submit` (about 2.08 ms), with
source preparation about 0.008 ms and command finalization about 0.03 ms. The
underlying wgpu/driver cause is not established. Do not claim these spikes were
eliminated or infer native input latency from this microbenchmark. Final sustained
GTK qualification still needs them included alongside drawing and other queries.

Reproduce with the release GPU library test executable, `LAYER_GPU_INDEX=0`, and
`region_request_latency --ignored --nocapture --test-threads=1`. The parent was
compiled from `git archive a93b799` in `/tmp/capy-region-parent`; saved binaries
are `region-parent-gpu-tests` and `region-reused-gpu-tests` under the artifact
directory. Final logs: `region-reused-abba-*`. Intermediate investigation logs
are `region-{source,batch,tail,phase,views,reuse,submit}*`. The source-backed mode
remains disabled pending the complete editing, color and GTK integration.

## Float32 blend and pass-boundary qualification

The reference adapter also supports the optional
[`FLOAT32_BLENDABLE` feature](https://docs.rs/wgpu/30.0.1/wgpu/struct.Features.html#associatedconstant.FLOAT32_BLENDABLE).
The extended `sdr_precision` harness requests that feature and compares 64
source-over blends in one instanced draw against 64 separate render passes and
the existing manual Float32 blend kernel. Each case spans 65,536 RGB combinations,
initial alpha 257/65535 and added coverage 1/65535 in all four built-in spaces.
Readback and shader preparation remain outside the 100 timed submissions after
10 warmups. The precision threshold remains 2 RGB / 1 alpha integer16 codes;
the additional partition comparison allows at most one code difference.

RGBA32Float passes both physical blend paths with maximum **1 RGB code / 0 alpha
codes** of error and RGB RMS 0.2149–0.2244 codes against Float64. Output codes are
**identical** to the manual physical-pass kernel in every space and both
partitions. FP16 fixed-function blending is more accurate in these cases than
the earlier manual FP16 kernel, but still reaches 25–31 RGB codes of error;
linear UNORM16 reaches 18,385–23,156. Neither narrower working format meets the
declared integer16 contract.

For this deliberately dense 256² tile, the 64 blended full-tile instances complete
at p95 **0.246–0.248 ms** with Float32 versus 0.094–0.098 ms with FP16; Float32
separate passes take 0.928–0.969 ms. The batched Float32 p99 is noisy at
2.228–2.256 ms. These are isolated format/partition measurements, not brush-frame
or input-to-present budgets. They establish a viable batched Float32 blend path
on the reference adapter; they do not qualify whole-document residency or other
GPUs. Hosts without this feature still need an explicitly measured full-precision
fallback or a declared unsupported workload, never an implicit FP16 substitution.

Reproduce: build and run `sdr_precision` as above, with `LAYER_GPU_INDEX=0`.
Final logs are `sdr-blend-batch{,-build}.log`; the preceding split-pass-only run
is `sdr-blend{,-build}.log`. Production document/paint formats remain unchanged
until their complete commit, restore, editing and memory integration is ready.

## Streaming profiled output foundation (2026-09-14)

`WorkingEncoder` converts straight or premultiplied linear Float32 working rows
to encoded integer8/integer16 RGB, gray or CMYK. Built-in RGB uses the independently
checked Float64 matrix/transfer definitions at the final encoding boundary;
ICC RGB/gray/CMYK uses worker-owned LCMS Float32 transforms with explicit intent
and BPC. CMYK percentages are scaled independently of alpha. Opaque delivery
requires an explicit linear matte when coverage is incomplete. Straight input
retains hidden RGB; premultiplied zero coverage becomes transparent black.
Non-finite data and channel/profile mismatches fail. Quantization/clamping happens
at the output boundary, with counts of channels clipped by more than half a code.

PNG/TIFF writers now consume a fallible sequential row producer. Original source
output uses the same path without a color round trip. Converted output requires
one working row and one encoded row, plus fixed 256-pixel CMM stack scratch;
TIFF16 also reuses one native-integer row. No second full image is constructed.
Cancellation stops further row requests; callers must publish temporary files
only after successful completion. Matching stable gray ICC profiles replace
invalid RGB-profile attachment to gray. TIFF gray-alpha now writes and recognizes
the standard black-is-zero, straight-alpha layout; the pinned decoder reports
this as `Multiband`, so acceptance checks its actual tags explicitly.

The color suite passes **26 tests**, including the optional external CMYK case.
Every native integer code round-trips exactly through straight working decode /
encode in all four spaces and both depths. Premultiplied integer16 tests cover
all 65,536 RGB codes at alpha 0, 1, 2, 17, 257, 32768 and 65535: maximum one RGB
code error, exact alpha. Separate encoded-RGB CMM transforms agree within two
integer16 codes for all 16 space pairs and four intents. External CMYK checks
cover 729 colors, all four intents and both BPC settings, including K as color
data, with the same two-code limit. The system LCMS 2.16 engine is common to
these CMM comparisons; they are independent transform paths, not independent
color engines. The licensed external fixture is not copied into the repository:
`/usr/share/color/icc/krita/cmyk.icm`, SHA-256
`156e7c14f244cfc4ed83a755ca4803d80e15dd249b40fae82cb127d3902e15c7`.

ImageMagick reads the generated RGB, gray/alpha and CMYK TIFFs and RGB/gray-alpha
PNGs with matching dimensions, depth, channels and ICC presence. Its raw
ProPhoto TIFF16 output matches every straight RGBA sample, including hidden RGB
and alpha 0/1/257. Our reopen/identity-output checks preserve tile digests and
original ICC payloads. Logs: `output-complete-tests.log`, `output-handoff.log`.

The independent `sdr_output` harness synthesizes one linear ProPhoto row at a
time, converts and writes it. These single-run measurements include row creation,
conversion, codec and buffered file writes, but exclude fsync, GPU capture, GTK,
retained document/history memory and concurrent export. Same reference machine
as the baseline; process high-water is **4,144–4,400 KiB** across these runs.
Preparation takes 0.6–1.1 ms. At width 8192, the working row is 128 KiB, encoded
RGBA16 row 64 KiB and test-only decode LUT 256 KiB.

| Random RGB16 + varied alpha | 24 MP | 45 MP | 60 MP |
| --- | ---: | ---: | ---: |
| TIFF write ms | 1640.3 | 2801.4 | 3048.8 |
| PNG default balanced write ms | 11931.2 | 23723.4 | 28846.4 |
| PNG selected level 1 write ms | 2701.7 | 4507.0 | 4785.6 |
| PNG balanced bytes | 171545324 | 322060642 | 428578210 |
| PNG level 1 bytes | 172209129 | 323350592 | 430288295 |

The codec's `Fast` setting was faster at 2193–3846 ms but produced 22% larger
files on this data. Level 1 preserves almost the balanced size (0.4% increase)
and reduces the long compression work substantially. The 24 MP ramp changes
from 1971.3 ms / 1,673,957 bytes to 1439.8 ms / 3,547,526 bytes. The selected
default uses level 1 with adaptive filtering; all choices are lossless. This
tradeoff follows measurements of the pinned implementation, not its setting
names. The [codec documentation](https://docs.rs/png/0.18.1/png/enum.DeflateCompression.html)
also identifies the streaming size limitation of its fastest implementation.

Reproduce with `cargo build --offline --release -p layer-color --examples`, then
`target/release/examples/sdr_output 6000 4000 prophoto OUTPUT.png noise`.
Use 8192×5504 and 8192×7324 for the other sizes, `ramp` for the smooth fixture,
and `.tif` for TIFF. Use `gray` or `cmyk` instead of `prophoto` for handoff
fixtures; CMYK requires `LAYER_TEST_CMYK_PROFILE` above. The optional test runs
with that variable and `cargo test --offline --release -p layer-color --
--include-ignored`. Logs are `output-{noise,fast,level1,ramp}-*`; saved comparison
executables are `output-balanced-runner` and `output-fast-runner`.

GPU correctness remains **135 passed / 18 benchmark tests ignored** after the
shared profile-helper refactor (`output-gpu-tests.log`). This change does not
alter frame construction or activate photo editing. JPEG delivery, dithering,
metadata policy, managed GTK export/preview integration and the full concurrent
workload gates remain outstanding.

The final level-1 build also passes all 26 color tests, four GPU project tests,
and the isolated native GTK file and diagnostics/GPU-recovery workflows. Logs:
`output-final-{build,color-tests}.log`, `output-project-tests.log`, and
`output-gtk-{files,recovery}.log`. No other host integration was changed.

## Source-aware material brushes and prediction (2026-09-14)

Material brush neighborhoods, watercolor transport/composition and disposable
prediction pages now include immutable photo tiles beneath paint overrides.
Preparation warms at most nine of the 16 source-cache slots; each binding is
consumed before another neighborhood can evict its inputs. Bindings borrow
prepared views. Persistent page generations flip after all jobs have sampled
the pre-batch state; reservoir exchange retains that same generation. Superseded
paint-only binding helpers and eagerly retained job bindings were removed.

Regression tests exposed two additional prerequisites: untouched photo prediction
pages were initialized as transparent, and default-accumulation watercolor could
omit the coverage attachment required by its physical shader. Prediction now
seeds complete disposable pages, limits retained preview inputs to the current
tail footprint, and initializes source originals without creating permanent paint
pages. Watercolor explicitly requests its required coverage state.

The new physical-GPU comparison covers Smudge, Wet, Liquify and Watercolor on a
2305×769 source with 40 original tiles. It crosses cache capacity and tile edges,
then compares initial rendering, one prediction batch, a shortened two-batch
prediction tail, persistent painting and zero-dab PenUp against materialized
reference pixels. All 20 complete-image comparisons agree within one integer8
code. Each brush changes pixels; untouched neighbors remain unmaterialized,
source digests remain exact and source upload staging stays within 16 MiB.
The source uses integer16 endpoints representable in integer8: this qualifies
source access and existing paint behavior, **not integer16 editing precision**.

Final correctness: **136 GPU tests passed / 18 benchmark tests ignored**, four
GPU project tests passed, and isolated native GTK file and diagnostics/GPU
recovery workflows passed. Logs: `source-brush-final-{build,gpu-tests,project-tests}.log`,
`source-brush-final-gtk-{files,recovery}{,-run}.log`.

A fresh parent executable from `196e456` and the first implementation each pass
all 10,920 measured frames in the 25-scenario suite; every final PNG is byte
identical. The first implementation cloned owned texture/view handles for each
neighbor and triggered relative CPU gates in several material brushes. Replacing
that with borrowed prepared views removes the repeatable Move regressions.
Five affected scenarios were then measured in current/parent/parent/current order,
10 repetitions per case (`source-brush-abba-*`). Final PNGs remain identical.

Oil PenUp still triggered the relative gate in those short runs (40 measured
PenUp samples, whose reported p99 is the maximum). Paired phase probes measured
frame encoding, primary submission, capture metadata/allocation/copy encoding,
command finishing and capture submission. Excluding each repetition's three
setup captures and one warm-up capture, parent/current capture p99 was
552/506 microseconds; frame encode 536/642, primary submit 1044/955. Capture
allocation p99 was 4/6 microseconds. No capture-growth mechanism was established.
The cold setup allocation/submit spikes are outside the drawing sample window.

Uninstrumented 20-repetition Oil runs in parent/current/current/parent order
(80 measured PenUp samples per run) clear the trigger:

| Run | CPU Move p95 / p99 ms | CPU PenUp p99 ms |
| --- | ---: | ---: |
| parent 0 | 1.960 / 2.441 | 2.112 |
| current 1 | 1.882 / 2.286 | 2.111 |
| current 2 | 1.917 / 2.357 | 2.165 |
| parent 3 | 1.902 / 2.366 | 2.294 |

All 11,680 measured frames in these four runs pass 8.33 ms. Artifacts are
`source-brush-oil20-*`; diagnostic probes are `source-brush-trace-*`.
`tools/performance/trace-raster-commit.py --parent 196e456 --current REV
--output-dir OUTPUT` reproduces isolated instrumented builds from committed
revisions; run the printed executables with `CAPY_TRACE_COMMIT_PHASES=1` and
`LAYER_GPU_INDEX=0`. Instrumented timings are diagnostic, not qualification.

Prediction comparison uses an 8 ms simulated input tail with +4/+8 ms predictions.
Initial Smudge/Liquify CPU and completion timings did not regress. Initial Oil
and Watercolor CPU p95 triggers cleared in current/parent/parent/current repeats:
Oil parent 3.527–4.015 ms versus current 3.353–3.425; Watercolor parent
3.296–3.380 versus current 3.382–3.511. Current prediction-enabled frames all
remain within 8.33 ms; one parent Oil frame exceeds it. Tip gap, correction,
preview dab counts and resident memory agree across revisions. Watercolor's
completed p95 remains higher at 5.834–5.854 versus 5.502–5.596 ms (0.238–0.352 ms),
while p99 is 6.275–6.378 versus 6.062–6.138 ms. Complete prediction-page seeding
is required for neighborhood correctness and adds copy area. This remaining
completion-time signal must be rechecked in final sustained native qualification;
it is not waived by the CPU result or attributed solely to noise. Logs/reports:
`source-brush-feedback-*`, `source-brush-feedback-repeat-*`. These offscreen
fixtures do not establish GTK input-to-present latency.

Reproduce ordinary runs using saved binaries `source-brush-parent-gpu-bench`
and `source-brush-borrow-gpu-bench`, `--scenario NAME --repeats 10 --report FILE`
(or 20 for Oil), with GPU index 0. Use `--feedback-comparison` for prediction
(the mode runs its own single fixture and does not use `--repeats`). Final
integer16 paint, source-aware transforms/thumbnails and bounded whole-document
working/composite residency remain prerequisites to enabling GTK photo editing.

## Bounded original-photo thumbnails and abandoned uploads (2026-09-14)

Photo-layer thumbnails now frame the document and integrate linear premultiplied
pixels over exact box footprints, including partial source tiles, transparency
and empty canvas beyond the photo. This preserves photo composition. Existing
paint-only thumbnails still frame visible marks; paper and mask journeys keep
their existing behavior. The finished 32px UI image uses the existing sRGB UI
handoff. These overviews never feed edits, numerical sampling or export; managed
viewing will replace that UI handoff with the rest of the GTK display integration.

The original overview is cached by weak source identity and document extent.
Each original tile also retains only its contribution to the few thumbnail
pixels it intersects. A paint override replaces that contribution, without
redecoding the original. Eight cached originals are allowed; each contribution
buffer has a checked 512 KiB ceiling, plus 16 KiB for its overview. Shared row
scratch is 128 KiB, working sums 16 KiB and parameters 48 bytes. Tile-footprint
metadata is bounded by the validated source tile count. GPU allocations appear
in renderer telemetry; weak keys do not retain original compressed data or old
paint generations. This cache is specific to the currently exposed sRGB working
representation and must be invalidated when document working-space selection is
integrated.

A physical-GPU test compares all 1024 linear sums against a Float64 area oracle
on a 2305×769 ProPhoto16 source inside a 2401×901 document. It covers 40 source
tiles, high-frequency data, alpha 0/1/257/32768/65535, wide-gamut components,
opaque paint replacement, complete erasure and removing overrides. Maximum
allowed absolute linear error is 0.00002. Repeat queries are byte-exact and
perform no unchanged-source decoding. Separate tests cover eviction, extent
changes, weak ownership, discarded command encoders and a late malformed tile
followed by retry. All three tests pass (`source-thumbnail-final-tests.log`).

The discarded-encoder test exposed a shared source-cache prerequisite: a planned
upload could leave its CPU cache key valid after its producing GPU commands were
abandoned. Source uploads and overviews now carry cancellation-aware validity;
dropping unencoded jobs or unsubmitted commands invalidates those keys. Failed
scene encoding also drops its outstanding jobs. Retrying performs fresh uploads
and reproduces the Float64 reference instead of accepting partial pixels.

The first bounded prototype redecoded originals beneath each paint override.
At 24 MP, 64 overrides overflowed the 16-slot source cache on every request:
CPU p95/p99 **46.000/46.633 ms**, completed **47.654/48.274 ms**. Compact cached
contributions remove that failure: CPU **0.844/1.021 ms**, completed
**4.683/4.839 ms**, with 196,448 bytes of retained thumbnail GPU storage. Across
all 480 requests, original decoding now occurs only during the initial overview
(384 tiles), versus 8,064 decodes in the prototype. Saved comparison binaries
are `source-thumbnail-{first,compact}-gpu-tests`.

The harness uses deterministic ProPhoto16 color ramps/stripes with varied alpha,
a fresh renderer and 20 warm-up plus 100 measured requests per override count.
It measures request CPU construction and GPU completion/readback separately;
these are isolated thumbnail requests, not drawing-frame or native presentation
latency. Source creation and renderer initialization precede the timed cold
request. The first cold request includes overview pipeline creation, tile decode,
upload, integration and UI readback. The process high-water includes fixture
construction and renderer state but not GTK or a fully composited photograph.
All runs use reference GPU 0. The final 45/60 MP runs are uncontended; earlier
runs overlapping compilation are retained with `-during-build` and excluded.

| Measurement | 24 MP (6000×4000) | 45 MP (8192×5504) | 60 MP (8192×7324) |
| --- | ---: | ---: | ---: |
| Cold CPU / completed ms | 260.665 / 261.982 | 559.755 / 561.622 | 725.116 / 726.844 |
| Warm unedited completed p95 / p99 ms | 0.059 / 0.085 | 0.076 / 0.082 | 0.070 / 0.088 |
| One override completed p95 / p99 ms | 0.130 / 0.148 | 0.171 / 0.217 | 0.140 / 0.151 |
| 16 overrides completed p95 / p99 ms | 1.110 / 1.125 | 1.394 / 1.726 | 1.274 / 1.423 |
| 64 overrides CPU p95 / p99 ms | 0.844 / 1.021 | 0.966 / 0.982 | 0.858 / 0.883 |
| 64 overrides completed p95 / p99 ms | 4.683 / 4.839 | 5.455 / 5.609 | 5.239 / 5.499 |
| Retained thumbnail GPU bytes | 196448 | 185904 | 193584 |
| Source upload peak bytes | 8388608 | 8388608 | 8388608 |
| Process high-water KiB | 271492 | 212780 | 214936 |

Cold overview construction **does not meet an interactive frame budget**. Before
GTK enables photo editing, prepare it during cancellable photo loading or split
it into scheduled work that yields to drawing. More than 64 overrides and
concurrent drawing still require qualification; these results are not a blanket
latency guarantee. The warm results establish a useful bounded implementation
without excusing that remaining scheduling work.

Reproduce after `cargo test --offline --release -p layer-render-wgpu --lib
--no-run`, using its printed test executable with
`LAYER_GPU_INDEX=0 LAYER_PHOTO_BENCH_EXTENT=6000x4000 TEST_BINARY
source_thumbnails::tests::photo_thumbnail_workloads --ignored --nocapture`.
Use the other extents above; wrap with `/usr/bin/time -v` for process high-water.
Reports: `source-thumbnail-first-24.log`, `source-thumbnail-compact-{24,45,60}.log`.

The final build passes **139 GPU tests / 19 benchmark tests ignored**, all four
GPU project round trips, and isolated native GTK file and diagnostics/GPU-recovery
workflows. The recovery test's injected validation failure is expected and the
workflow passes. Logs: `source-thumbnail-native-build.log`,
`source-thumbnail-{gpu-tests,project-tests}.log`,
`source-thumbnail-gtk-{files,recovery}{,-run}.log`. Ordinary drawing shaders and
paint-only thumbnail behavior are unchanged; final sustained frame and native
input-to-present qualification remains required with the complete GTK workflow.

## Tiled photo transforms and capture reuse

Ordered and live transforms now read immutable paint/source tiles directly.
The production rectangular-capture allocator and its full-image copy path are
removed. Each output region binds at most sixteen source tiles and evaluates
four-tap interpolation explicitly in Float32. A conservative inverse footprint
includes Float32 coordinate rounding; larger footprints split into smaller output
regions before any destination mutation. Planning rejects noninvertible input,
unrepresentable coordinates or more than 65,536 regions per channel.

Original photo data stays in the existing bounded decoded-source cache. A new
paint override first receives the untouched original tile, then the transformed
scissor, so a partial selection does not erase pixels elsewhere in the page.
Existing paint surfaces share the snapshot until their first overwrite; that
first overwrite copies them into reusable per-coordinate snapshot tiles. All
copies for a channel precede its draws. This keeps live target identities stable
and avoids allocating replacement textures on each committed transform. Reuse
also applies to existing texture views. Active copies follow the overwritten
paint footprint; after a transaction, each channel retains at most 64 snapshot
tiles. This is **not yet a bounded working-residency implementation for dense
edited integer16 photographs**. Disposable preview pages follow peak visited
footprints and are released on cancel or commit.

Source metadata and transform parameters upload in batches, with independent
aligned dynamic offsets. A 256-entry binding cache uses resource identities and
retains only views owned by the reusable snapshot pool between transactions.
Pool pruning releases coordinates absent from the next paint snapshot. Buffer
growth clears bindings, and allocation accounting avoids counting shared buffers
or unchanged paint surfaces twice. There is no retained full-resolution Float32
capture of the original photograph.

The new physical-GPU test uses a 1537×1025 integer16 source (35 tiles) inside a
1792×1280 document, versus an equivalent materialized endpoint-color reference.
Eight full-image comparisons cover full/fractional selections, fractional
translation, strong rotated downscale, flipped rotation and identity. Maximum
per-channel integer8 difference is one code. Cancel restores exact displayed
pixels and releases all photo paint overrides; a matching preview commit has
no jump, and undo/redo restore exact captured raster states. Source tile hashes
remain unchanged. This endpoint test qualifies source integration into the
currently exposed sRGB8 renderer; it **does not qualify integer16 edit precision**.
Existing primitive, masks, linked-mask, wetness, preview-page reuse and ordered
transform tests also pass.

### Existing transform performance

The fresh parent is `317acfd`; the fresh fixed reference is `e46f271`. The
unchanged 2048×1536 harness runs 160 frames per case, discarding 40 warm-up frames.
Each comparison arm runs five repetitions; the sequence is current, fixed,
parent, parent, fixed, current. Values below are ranges of the two arms' median
per-run p95 values, in milliseconds. Each version contributes 12,000 measured
frames across six live and four ordered cases. These measure CPU construction,
GPU timestamps and GPU completion, not native input-to-present latency.

| Case | Fixed CPU p95 | Parent CPU p95 | Current CPU p95 | Parent completed p95 | Current completed p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Ordered GPen, full | 0.365–0.424 | 0.370–0.443 | 0.443–0.444 | 0.736–0.809 | 0.791–0.795 |
| Ordered GPen, selected | 0.393–0.410 | 0.394–0.464 | 0.501–0.505 | 0.800–0.874 | 0.816–0.833 |
| Ordered WetRound, selected | 0.725–0.791 | 0.717–0.755 | 0.899–0.929 | 1.456–1.481 | 1.511–1.557 |
| Ordered watercolor, selected | 0.889–0.937 | 0.935–0.984 | 1.068–1.125 | 1.694–1.755 | 1.715–1.771 |
| Live GPen, full | 0.275–0.283 | 0.270–0.280 | 0.344–0.363 | 0.440–0.459 | 0.519–0.544 |
| Live GPen, selected | 0.266–0.316 | 0.270–0.275 | 0.347–0.360 | 0.435–0.436 | 0.533–0.540 |
| Live WetRound, selected | 0.429–0.444 | 0.438–0.443 | 0.558–0.585 | 0.696–0.722 | 0.812–0.851 |
| Live watercolor, selected | 0.624 | 0.598–0.643 | 0.720–0.754 | 0.882–0.963 | 1.020–1.071 |
| Live GPen, linked mask | 0.898–0.941 | 0.935–0.960 | 1.057–1.066 | 1.562–1.605 | 1.702–1.717 |
| Live watercolor, linked mask | 3.165–3.207 | 3.110–3.181 | 3.491–3.653 | 4.712–4.779 | 5.167–5.193 |

All runs pass the existing completed-p99 <8.333 ms assertion. **The linked-mask
watercolor regression remains above the relative investigation trigger**:
current CPU p99 arm medians 4.395–4.722 ms versus parent 3.920–3.931 and fixed
3.867–3.888; completed p99 5.472–5.511 versus parent 4.942–5.063 and fixed
5.069–5.317. Some WetRound tail measurements also trigger, with overlapping
parent noise. These are retained concerns for the final sustained native
qualification, not a declaration that the unchanged-path gate has passed.

The initial correct prototype interleaved per-region metadata copies with draws
and allocated replacement target textures repeatedly. For example, selected
ordered WetRound used CPU p95 2.179 ms and completed p95 3.726 ms, versus final
0.899–0.929 and 1.511–1.557. Batched metadata restored GPU p95 to the parent's
level or better; reusable snapshots and views removed most allocation overhead.
Final ordered GPU p95 is approximately 0.307/0.245/0.542/0.558 ms for the four
cases, versus parent 0.317/0.351/0.647/0.664. Live GPU p95 remains approximately
0.126/0.123/0.203/0.218/0.544/1.400 ms.

Temporary phase probes locate planning at 7–18 μs and typical complete transform
encoding at 54–123 μs. Much of the remaining CPU cost is outside that encoder,
consistent with validation/submission of the expanded source bindings. That is
an inference, not a driver profile. A reusable render-bundle experiment passed
correctness but did not improve the measured workloads enough to retain; it was
removed. Diagnostic runs are excluded from the paired qualification measurements.

Capture/uniform storage grows by only 49,968–148,272 bytes in these cases for
batched metadata; snapshot pool coordinates do not accumulate across unrelated
workloads. For example full GPen uses 12,649,408 bytes versus parent's 12,599,440;
live linked-mask watercolor uses 19,282,464 versus 19,134,192, excluding its
2,621,440-byte preview spare footprint. Transform snapshot storage follows
modified paint, not source-photo dimensions.

Saved executables and logs use `tiled-transform-{parent,fixed,view-reuse}-gpu-tests`
and `tiled-transform-final-paired-*`; the machine-readable arm summary is
`tiled-transform-final-paired-summary.json`. Reproduce with the printed release
GPU test executable, `LAYER_GPU_INDEX=0`, and the ignored filters
`layer_tests::transforms::{live_transform_latency,ordered_transform_latency}`
(one filter per invocation, `--nocapture --test-threads=1`). Intermediate,
instrumented and discarded-experiment logs remain under `tiled-transform-*`.

### Photo transform limits before viewport residency

`photo_transform_workloads` uses the same deterministic ProPhoto16 ramps,
stripes and low-alpha codes as the thumbnail workload. It constructs a fresh
renderer and source, renders into the current sRGB8 document path, and sets a
1920×1080 fit view. Source construction and renderer initialization precede the
startup timer. It then measures a moving 1024×1024 polygon selection and a
full-photo transform, each with 20 warm-up and 100 measured updates. Cancel
releases every preview paint override and source tile hashes remain unchanged.
The memory figures include the full current compositor, paint overrides,
transform state, source cache and explicit staging; they exclude driver-private
allocations. Process high-water includes fixture construction and initialization.

| Measurement | 24 MP (6000×4000) | 45 MP (8192×5504) | 60 MP (8192×7324) |
| --- | ---: | ---: | ---: |
| Initial source composition completed ms | 273.978 | 472.444 | 618.907 |
| First 1 MP selection CPU / completed ms | 22.760 / 24.477 | 21.556 / 22.210 | 21.648 / 22.373 |
| Warm 1 MP selection CPU p95 / p99 ms | 16.426 / 16.583 | 19.688 / 20.049 | 19.708 / 20.480 |
| Warm 1 MP selection completed p95 / p99 ms | 17.898 / 18.085 | 20.336 / 20.540 | 20.291 / 20.540 |
| First full-photo CPU / completed ms | 397.752 / 401.343 | 979.886 / 980.977 | 978.672 / 981.613 |
| Warm full-photo CPU p95 / p99 ms | 509.474 / 522.939 | 941.705 / 948.919 | 1227.490 / 1232.763 |
| Warm full-photo completed p95 / p99 ms | 513.082 / 526.307 | 945.671 / 952.811 | 1230.485 / 1235.784 |
| Initial renderer residency bytes | 181230528 | 266109888 | 325747648 |
| Peak full-preview renderer residency bytes | 283477024 | 452766752 | 571124768 |
| Full-preview paint bytes | 100663296 | 184549376 | 243269632 |
| Full-preview transform storage bytes | 1054704 | 1578992 | 1578992 |
| Peak source upload bytes | 8388608 | 8388608 | 8388608 |
| Process high-water KiB | 413148 | 356544 | 354516 |

These measurements **fail the interactive frame budget**. Original-photo
sampling is bounded in memory, but a moving 1 MP selection decodes 2,500/3,000/
3,000 source tiles over 100 updates; full-photo updates decode 77,380/141,760/
186,386. The source cache is smaller than even that selected footprint, and
full-resolution preview/composite processing remains proportional to document
area despite the fit view. Increasing the number of full-resolution working
copies is not an accepted fix. Before enabling the workflow, viewport/mip
residency must serve previews, and full-resolution committed work must be
scheduled independently with explicit cancellation/publication boundaries.
These runs establish the blocker and its current cost, not a supported device
or workload envelope. No GTK photo mode was enabled by this checkpoint.

Reproduce with `cargo test --offline --release -p layer-render-wgpu --lib
--no-run`, then `/usr/bin/time -v env LAYER_GPU_INDEX=0
LAYER_PHOTO_BENCH_EXTENT=6000x4000 TEST_BINARY
layer_tests::transforms::photo_transform_workloads --ignored --nocapture
--test-threads=1`. Repeat for the other extents. The default is 100 measured
samples; `LAYER_PHOTO_BENCH_SAMPLES` permits explicitly labeled exploratory runs
of at least 20. The reported runs all use the default. Saved executable:
`tiled-transform-final-gpu-tests`; logs: `tiled-transform-photo-{24,45,60}.log`.
Source and executable hashes are in `tiled-transform-artifact-sha256.json`.

The final GTK build passes **140 GPU tests / 20 benchmark tests ignored**, all
four project round trips, and isolated native GTK file and diagnostics/GPU
recovery workflows. The recovery test's injected GPU validation failure is
expected; recovery completes. Logs: `tiled-transform-native-build.log`,
`tiled-transform-{gpu-tests,project-tests}.log`, and
`tiled-transform-gtk-{files,recovery}{,-run}.log`. Formatting checks for the
changed Rust modules and `git diff --check` pass. Other platform hosts were not
integrated or qualified in this checkpoint.

## Native integer paint boundary qualification

The `sdr_precision tiles` experiment checks native straight `Rgba8Uint` and
`Rgba16Uint` samples through physical `Rgba32Float` premultiplied working tiles.
It requests **no optional GPU features**: neither normalized16 storage,
Float32 filtering nor Float32 fixed-function blending is needed for these two
conversion passes. The existing blend/pass experiments still establish their
separate feature and precision requirements. This experiment does not change
current document paint storage or enable integer16 editing.

Three methods are compared: analytic transfer functions; Float64-generated
Float32 decode values with binary-search quantization; and the same tables with
an analytic estimate verified against exact decision boundaries. The last
method adjusts at most twice, then falls back to a bounded 16-step binary search
if the estimate still lacks a valid bracket. Correctness does not rely on a
particular driver's `pow` approximation. Decision boundaries are the smallest
Float32 values at or above the Float64 decoded encoded-half-code boundary.

The 65,536-entry table contains one decode value and one quantization boundary
per entry (524,288 bytes plus a 16-byte experimental header). Its body serves
both depths: integer8 decode indices are `257 × code`, and integer8 boundary
indices are `257 × code + 128`. It can therefore be shared independently of
native depth in the production codec. sRGB and P3 also have identical transfer
tables; their primaries remain separate. All pipeline compilation, table creation
and fixture construction precede timed work.

Across all four working spaces, every integer code, low-alpha values
0/1/2/17/128/255 for integer8 and 0/1/2/17/257/32768/65535 for integer16, all
three methods return **zero RGB and alpha code error** after both one and 64
physical decode/encode cycles (312 cases). Transparent working pixels become
canonical black. This is the edit-boundary convention; untouched source samples,
including hidden RGB, must continue to use the exact source-preserving route.

A separate adversarial corpus feeds the Float32 values immediately below, at
and above every rounding boundary, plus deterministic extended-range samples,
into the encoder. Across 6,291,456 RGB comparisons per method, both table methods
match the Float64 transfer/quantization reference exactly. The analytic method
has maximum error one code, affecting 1,307,506 comparisons in this deliberately
boundary-heavy corpus. That fraction does not describe ordinary photographs.
These cases distinguish deterministic final quantization from integer identity
round trips, which alone did not reveal the difference.

The final no-optional-feature run uses 20 warm-up and 100 measured single-cycle
iterations per case. CPU timing covers command construction/submission; completed
timing includes GPU completion and the already queued one-tile input upload.
Input upload CPU work and final numerical readback are outside those timers.
The 64-cycle cases verify accuracy once and have no percentile timing; their
output records `timed_samples=0`. The following are medians of the per-case p95
values for nonzero alpha, not pooled latency percentiles:

| Native depth / method | CPU p95 ms | Completed p95 ms |
| --- | ---: | ---: |
| Integer8 analytic | 0.0151 | 0.0519 |
| Integer8 binary table | 0.0149 | 0.0565 |
| Integer8 verified estimate | 0.0154 | 0.0544 |
| Integer16 analytic | 0.0156 | 0.0539 |
| Integer16 binary table | 0.0159 | 0.0706 |
| Integer16 verified estimate | 0.0157 | 0.0587 |

Maximum individual completed p99 is 0.2292 ms across these runs; process
high-water is 218,000 KiB including the GPU device/compiler and fixtures.
The verified-estimate method provides exact reference quantization with a
smaller measured cost than full binary search. This supports using it at the
native paint boundary. It does not justify per-dab conversion passes: active
stroke work must remain Float32 and native publication must be batched.
Partial-tile preservation, invalid nonfinite results, asynchronous native capture,
bounded edited/composite residency and complete-tool precision still need
integration and qualification before the document mode is enabled.

Reproduce with `cargo build --offline --release -p layer-render-wgpu --example
sdr_precision`, then `LAYER_GPU_INDEX=0 /usr/bin/time -v
target/release/examples/sdr_precision tiles`. Reports:
`native-tile-boundary-{first,final,portable}.log`; `portable` is the final run
without optional features. `final` contains the same full-table/adversarial
corpus with the example's older feature request. Both pass. The initial `first`
run has depth-sized tables and no adversarial boundary corpus. Only example and
reporting code changed in this qualification; the GTK renderer is unchanged.

## Shared transfer tables in the source decoder

Built-in RGB/gray source decoding now uses the qualified native transfer values
instead of evaluating the transfer function in the source shader. Native-depth
samples address the shared integer16 table directly (integer8 uses stride 257).
The existing Float32 primary/white conversion and independent coverage remain.
Embedded profiles continue through the CMM. The unused shader transfer-function
include and source-space selector were removed from this path.

Each renderer lazily retains at most three 512 KiB table bodies; sRGB and P3
share one buffer across depths. The integer input bindings reuse the same two
input textures and those three table identities. The persistent maximum is
**1.5 MiB**, included in source-cache GPU telemetry. Table construction uses at
most **512 KiB** of temporary CPU bytes. Also budget up to **1.5 MiB** for the
three tables' initial mapped-buffer uploads; these are separate from ordinary
source-tile upload telemetry and are not per-frame allocations. Original source
objects and history generations are not retained by the tables.

All 96 source-decoder cases pass. Native built-in space/depth combinations now
require **zero integer-code error**; cross-space and embedded-profile cases have
observed maximum error **one code**, with maximum absolute linear error
0.00000023841858 (existing limits remain two codes and 0.000003). Coverage is
independent, including exact opaque coverage. A shared-buffer check verifies
sRGB/P3 identity and the three-table allocation ceiling. Log:
`native-transfer-current-accuracy.log`.

The fresh parent is `8e48430`. An identical added benchmark in the parent archive
and current checkout measures 16 forced source-cache misses per batch, cycling
through twenty aligned tiles of a 1280×1024 deterministic image. It includes
decompression/integrity checks, native channel upload, decode passes and explicit
GPU completion. Each space/depth case has one cold batch, 20 warm-up batches
(including that cold batch), and 100 measured batches. This is a source loading
workload, not a complete drawing or presentation frame. The new integer source
path has no equivalent at fixed baseline `e46f271`; the full-program baseline
comparisons remain the earlier drawing/photo reports and final GTK qualification.

Current/parent/parent/current runs with normal scheduling show two repeatable
CPU-speed bands, approximately 2.9/4.0 ms for integer8 and 7.0/9.8 ms for integer16
batches. The bands occur in both versions. A second sequence pins only the
benchmark thread to CPU 4 after GPU workers start, then restores its affinity;
the band transition still occurs. The CPU is AMD Ryzen Threadripper PRO 9995WX,
96 cores/192 threads, `amd-pstate-epp` with the existing `powersave` governor.
No system power settings were changed. The cause of the band transition is not
established, so differently timed transitions are not attributed to the lookup.

Representative pinned completed p95 ranges in matching stable bands:

| Source case | Parent ms | Current ms |
| --- | ---: | ---: |
| sRGB integer8, earlier slow band | 4.205–4.325 | 4.278–4.346 |
| sRGB integer16, earlier slow band | 9.952–10.150 | 10.067–10.105 |
| Adobe RGB integer8, later fast band | 3.115–3.126 | 3.180–3.254 |
| Adobe RGB integer16, later fast band | 7.297–7.342 | 7.300–7.358 |
| ProPhoto integer8, later fast band | 3.127–3.142 | 3.141–3.147 |
| ProPhoto integer16, later fast band | 7.299–7.345 | 7.262–7.297 |

Warm p95 changes in these matching bands remain below the relative trigger.
Some individual p99 tails still exceed it, without a stable increase across
both arms; complete distributions and the P3 transition cases are retained in
the logs. Cold batches add roughly 1.4–3.5 ms in the pinned comparisons, including
table preparation and other cold work. Table preparation belongs in cancellable
photo loading. A sixteen-tile miss batch itself can exceed 8.33 ms in both
versions: source decoding must be scheduled independently of input frames.
This is further evidence for the outstanding scheduling/residency work, not
qualification of cache-miss interaction latency.

A single active curve raises the benchmark's retained source GPU bytes by
524,288: integer8 uses 17,563,648 bytes; integer16 uses 17,825,792. Ordinary tile
upload peaks remain 4/8 MiB respectively (plus the separately budgeted table
initialization). Across all curves and both input depths, retained source
storage has a fixed 18.25 MiB ceiling. Pinned process high-water is
138,876–140,256 KiB current and 141,408–141,608 KiB parent; this small difference
is not claimed as a memory saving.

Reproduce after a release GPU-test build with `LAYER_GPU_INDEX=0 TEST_BINARY
scene::sources::tests::native_source_decode_workloads --ignored --nocapture
--test-threads=1`; add `LAYER_BENCH_CPU=4` for the pinned-thread diagnostic.
Reports: `native-transfer-abba-*`, `native-transfer-pinned-*`, and
`native-transfer-source-measurements.json`. The earlier
`native-transfer-unwaited-abba-*` runs are **excluded**: the initial harness used
`wait_idle` without a renderer-owned submission, so its completion timer did not
wait for the directly submitted GPU commands. The corrected harness explicitly
polls the device to completion; numerical readback tests were unaffected.

The production decoder build passes **140 GPU tests / 21 benchmarks ignored**,
all four project round trips, and isolated GTK file and diagnostics/GPU-recovery
workflows. Logs: `native-transfer-native-build.log`,
`native-transfer-{gpu-tests,project-tests}.log`, and
`native-transfer-gtk-{files,recovery}{,-run}.log`. The injected recovery validation
error is expected. Changed Rust formatting and `git diff --check` pass. Native
integer paint writeback and viewport residency remain unfinished; this change
does not enable a new document mode or integrate another platform host.

## Batched native integer writeback and canonical working pixels

Parent: `cfc48ce`. This stage adds a shared GPU publication primitive; the
application still exposes its existing sRGB8 document mode. It does not yet
connect integer16 paint to the raster worker, history, cache eviction or GTK UI.

`native_tiles` writes at most sixteen 256×256 tiles per prepared batch. It takes
linear premultiplied RGBA32Float working pixels, emits RGBA8Uint or RGBA16Uint
native bytes, and emits a separate RGBA32Float working candidate decoded from
those final integer codes in the same compute pass. Publishing the canonical
candidate prevents live committed pixels from retaining precision absent from
save/reopen. Caller-owned source and destination tiles, queues and publication
lifetimes remain explicit. The ordinary submission owner opens the compute pass,
so its command chunking and cancellation also apply to this work.

RGB uses the previously qualified table-verified estimate with a bounded binary
fallback. Source decode and writeback now share `NativeTransfer` and its table
pool; the old source-private transfer module was moved and replaced. Both native
depths and straight/premultiplied-linear storage are supported. SDR range clipping
occurs only at this native publication boundary, is counted, and happens before
unassociation to prevent finite extended RGB overflowing at low alpha. Coverage
is independent. A final alpha code of zero produces canonical transparent black
inside the changed region. Original native bytes outside that region, including
hidden RGB, are left intact; callers initialize both destination candidates.

The first floating-point alpha-boundary correction failed on the GPU: coverage
`0.50392157` selected integer8 alpha 129 when the Float64 reference selected 128.
It was removed. Alpha now uses the input Float32 significand and exponent to
form an exact 40-bit integer product in two uint32 words and round it to native
coverage. This avoids relying on floating-point reassociation or multiplication
at half-code thresholds. WGSL explicitly permits floating-point reassociation
and does not require universal IEEE-754 nonfinite behavior; these are relevant
constraints, not a promise that a CPU compensation expression survives a GPU
compiler. [WGSL floating-point rules](https://www.w3.org/TR/WGSL/#floating-point-evaluation),
[reassociation and fusion](https://www.w3.org/TR/WGSL/#reassociation-and-fusion).

A shared eight-byte GPU status records invalid values/coverage and clipped pixel
count. Each publication resets it once, all its batches contribute, and the
capture owner must read and accept it before publishing any candidate. Integer
exponent/sign inspection detects nonfinite values and invalid coverage on the
qualified Vulkan device, including a negative subnormal alpha. This hardware
check is not evidence for an untested backend's handling of nonfinite WGSL input.
Whole-batch validation rejects invalid dimensions, mip/array counts, texture
formats/usages, working/output aliasing, alpha representation and overflowing
regions before creating bindings or recording writes. Empty batches allocate no
parameters; zero regions dispatch no work. Dropping recorded commands changes
neither native output nor a live source.

Physical GPU validation:

- 160 space/depth/association/input cases: 10,485,760 pixels with **zero native
  RGB or alpha code error** against Float64 transfer/rounding after the declared
  Float32 unassociation. Inputs include all native codes, nearest Float32 values
  around RGB and alpha half-code thresholds, zero/sub-half-code/low alpha, and
  finite extended RGB. Canonical working output matches a separately computed
  decode within a relative 0.00000024 tolerance (with a 0.0000001 scale floor).
- Eight space/depth cases, all native codes with seven coverage values, retain
  **zero code drift after 64 successive physical publications**, alternating
  working textures. This tests the fused canonical output feeding later edits.
- Mixed sixteen-tile batches preserve sentinel bytes outside partial regions;
  source working pixels remain byte-identical. A later batch containing NaN,
  either infinity or invalid coverage makes the shared status fail after an
  earlier valid batch. Reset/retry succeeds. These tests validate the primitive's
  status contract; transactional raster-worker integration is still pending.

Reproduce with a release GPU-test build and `LAYER_GPU_INDEX=0 TEST_BINARY
native_tiles::tests:: --test-threads=1 --nocapture`. Numerical log:
`native-writeback-tests.log`.

The ignored `native_tiles::tests::native_writeback_workloads` benchmark covers
sRGB, Adobe RGB and ProPhoto (P3 shares the sRGB curve), both depths, in-range and
clipped inputs, one/sixteen tiles, and 32×32/full-tile regions. Each of 96 cases
has 20 warm-up and 100 measured batches; two final runs give 19,200 measured
batches. CPU time includes command creation/submission, optionally rebuilding
all texture views/bindings/parameters. Completion uses an explicit device wait
for that submitted work. Initial texture preparation/upload, preservation copies,
readback, compression, history publication and display are **not timed**. This
is not input-to-present or the complete pen-up path. No builds ran concurrently.

Across curves and clipping cases, full sixteen-tile results on the same
reference Vulkan GPU and unchanged power settings:

| Preparation | Depth | Completed p95 range, ms | Completed p99 range, ms |
| --- | --- | ---: | ---: |
| Recreate bindings | integer8 | 0.3032–0.3588 | 0.3082–0.6135 |
| Recreate bindings | integer16 | 0.3114–0.3655 | 0.3166–0.4770 |
| Reuse bindings | integer8 | 0.1248–0.1401 | 0.1273–0.2634 |
| Reuse bindings | integer16 | 0.1315–0.1479 | 0.1347–0.1769 |

Maximum full-batch CPU p95 is 0.1701 ms with reconstruction and 0.0423 ms with
reuse. Some other reconstructed-binding cases have CPU/completed p99 around
2.0–2.18 ms; the same tail occurred in the initial implementation. Its cause is
unestablished. Stable pool bindings should be reused during cache integration,
and the complete commit path still needs frame-level measurement. Pipeline
creation is 1.69–1.74 ms with the existing driver cache; each curve's CPU table
creation/upload preparation is 2.02–2.46 ms. These belong to mode preparation,
not a warm input callback. First-use times are retained separately in the logs.

The initial prototype emitted native bytes alone. Adding the canonical output
raises full-batch reconstructed completed p95 from roughly 0.24–0.30 ms to
0.30–0.37 ms; reused bindings go from roughly 0.11–0.13 to 0.12–0.15 ms. The
prototype is not an equal-work baseline and was superseded. Parent `cfc48ce`
and fixed baseline `e46f271` have no native writeback operation; unchanged-path
frame comparisons remain the earlier reports and outstanding final GTK suite.

The benchmark preallocates sixteen working and sixteen canonical Float32 tiles
(16 MiB each), plus sixteen native tiles (4 MiB integer8 / 8 MiB integer16).
One batch has at most 4 KiB of uniform parameters at this device's 256-byte
alignment, plus eight-byte status and up to 1.5 MiB shared transfer tables across
curves. Table initialization retains its separately budgeted mapped-upload
cost. This is a bounded 36/40 MiB texture pool, not per-document duplication.
Peak process RSS is 221,112–221,220 KiB in the final benchmark runs; compiler and
driver allocations are included in that process measurement. Capture staging
and existing source/composite residency are additional budgets for integration.

Artifacts: `native-writeback-{first,final-bench-1,final-bench-2}*.log`, saved first
and final GPU test executables, and `native-writeback-measurements.json` with
per-case results and binary hashes. The first executable omits canonical output;
only the final executable represents the retained implementation.

The retained implementation passes **143 GPU tests / 22 benchmarks ignored**,
four project round trips, and isolated GTK file and diagnostics/recovery checks.
The recovery test's injected wgpu validation panic is expected and recovered.
Logs: `native-writeback-native-build.log`, `native-writeback-full-gpu-tests.log`,
`native-writeback-project-tests.log`, and `native-writeback-gtk-{files,recovery}`
logs. Changed Rust formatting and `git diff --check` pass. No other platform
host was integrated. The existing raster worker still needs explicit native
entry descriptors and status-before-backing publication; viewport residency,
complete precision editing and the GTK SDR journeys remain outstanding.

## Native raster capture through the existing backing worker (in progress)

Parent: `7fc143d`. The common capture path now carries each tile's explicit
pixel descriptor into compression. Native RGBA8Uint/RGBA16Uint and the existing
sRGB8/coverage8 captures share staging, queue ordering, worker backpressure,
compression and failure tickets. The browser side of this shared Rust transport
receives the same descriptor field; no browser or other platform host mode was
activated or qualified. Document-mode, raster-index and cache integration remain
pending.

A capture can include a copied eight-byte native encoding status. The worker
accepts this status before publishing any of that capture's tile backing. Its
copy is queue-ordered with the pixels, so resetting the live GPU status for the
next publication cannot hide an earlier failure. Descriptor/texture consistency,
unique unpublished tickets, counts and the actual rounded staging allocation
are validated before recording copies. The staging limit remains 256 MiB per
capture; it is not raised for integer16.

New physical tests cover 37 mixed native/coverage tiles crossing two chunks,
exact bytes after compression despite later GPU overwrites, ProPhoto16 GPU
writeback into 33 captured tiles, a nonfinite last pixel rejecting all backing,
status reset before worker completion, retry and abandonment. A 252.5 MiB mixed
payload requiring 256.5 MiB of rounded staging is rejected before allocation.
Four targeted tests pass after the cache correction below; log
`native-capture-pooled-tests.log`. Current capture metadata still uses the
existing sRGB8 raster index for exposed documents; these tests do not establish
complete native integer16 document editing or saving.

The initial worker benchmark exposed a pre-existing staging-cache defect. Four
16 MiB startup spares filled its 64 MiB cache. Its eviction rule only removed
buffers smaller than a newly returned buffer, so a small capture (and the new
status buffer) could never enter that full cache. Repeated captures therefore
allocated pinned buffers repeatedly. The cache now preserves recent size usage,
evicts the oldest unused buffer when necessary, and caps cached status buffers
at sixteen. The total cache ceiling stays 64 MiB. A resource-identity test
verifies that small tile/status buffers really are reused after large prefill.

The ignored `raster::native_tests::native_capture_workloads` test runs actual
queue copies and submits the ticket to the existing backing worker. It covers
integer8/integer16, structured/noise bytes and one/sixteen/64 tiles, with 20 warm
and 50 measured captures per case. Submission includes capture metadata,
allocation/copy recording, queue submit/map setup and worker enqueue. The
host-backed timer also includes mapping, compression and worker completion;
no input owner waits in the application path. GPU quantization and display are
separate work, excluded here. Each case uses repeated tile contents to isolate
transport/compression, not a full photographic editing workload.

Before the cache correction, warm submission p95 was approximately 7–13 ms.
With reuse, the first corrected run's maximum is 0.197 ms; integer16's maximum
is 0.170 ms. Host-backed p95 for 64 integer16 tiles is 12.90–13.56 ms, which must
remain background work. A first small capture still takes 9.52 ms to submit in
that run because its sizes were not prepared: mode preparation must establish
needed size classes before exposing interactive native editing. These results
are not a pass for cold interaction. Logs: `native-capture-first-bench.log` and
`native-capture-pooled-bench.log`.

The corrected run's process high-water is 514,968 KiB versus 437,120 KiB in the
first run, despite the same 64 MiB application staging-cache ceiling. This
increase is retained as an unresolved process-memory observation; the faster
allocation/reuse cadence and driver/allocator behavior have not been separated.
Repeat process measurements and complete frame comparisons are pending. The
64-tile integer16 case uses 32 MiB plus eight bytes of pending capture staging;
parallel CPU mapping/compression scratch retains its existing bounded worker
contract. Peak process memory cannot be inferred from native pixel payload alone.

The repeated old/pooled/pooled/old worker sequence retains the submission
improvement. Process high-water is 452,144 / 459,788 / 461,204 / 443,076 KiB;
end-of-run RSS is 345,672 / 459,788 / 461,204 / 331,256 KiB. The earlier corrected
514,968 KiB high-water remains the observed maximum. Reuse changes resident
memory behavior as well as timing; the driver/allocator contribution is still
unattributed, and these results are not a complete photo-memory qualification.
All per-case results and binary hashes are in
`native-capture-worker-measurements.json`; raw repeats are
`native-capture-memory-{0-old,1-pooled,2-pooled,3-old}.log`.

Fresh fixed (`e46f271`), parent (`7fc143d`), current, current, parent drawing runs
cover all 25 scenarios, three repetitions each: **54,600 measured frames**.
No Move or Pen-up sample exceeds 8.33 ms. All 25 PNGs are byte-identical across
all five arms. Maximum CPU Move p95/p99 across scenarios is 2.168/2.864 ms fixed,
2.055/2.869 and 2.011/2.890 ms parent, and 2.236/2.822 and 2.114/2.892 ms current.
Reported peak capture allocated/reserved storage is 184.2 MiB fixed,
192.2/184.2 MiB parent and 168.5/176.5 MiB current. This counter includes pending
reservations and CPU scratch, so it is not a distinct-buffer VRAM measurement.

The short runs trigger wet-watercolor completed Pen-up p99: parent
3.140/3.196 ms, current 3.434/3.475 ms, fixed 3.180 ms. A longer
parent/current/current/parent comparison uses twenty repetitions per arm,
**11,680 additional frames**. Current completed Pen-up p99 is 3.575/3.550 ms,
within the parent range 3.451/3.750 ms; CPU Pen-up p99 is 2.388/2.251 ms current
versus 2.186/2.520 ms parent. Move CPU p95 is 2.039/2.033 ms current versus
1.951/1.927 ms parent, and completed p95 is 3.282/3.261 versus 3.170/3.142 ms.
The longer comparison clears the relative trigger. Every measured frame meets
the absolute limit. Other short-run triggers do not reproduce across the two
parent/current arms. Earlier outstanding transform/prediction limits remain
unchanged; these drawing runs do not substitute for their final qualification.

Reproduction: build `layer-bench --bin gpu-bench` in fresh archives of the named
commits, copy each successful executable, then run with `LAYER_GPU_INDEX=0`,
`--scenario all --repeats 3` in fixed/parent/current/current/parent order. Use
`--scenario wet_watercolor --repeats 20` for the longer paired check. Reports
are `native-capture-frame-{0-fixed,1-parent,2-current,3-current,4-parent}.md` and
`native-capture-watercolor-{0-parent,1-current,2-current,3-parent}.md`; image and
binary hashes and full-suite values are in `native-capture-frame-summary.json`.
Builds and other GPU workloads do not overlap these measurements. The public
frame benchmark waits for GPU completion and accounts for deferred capture
capacity, but does not measure native input-to-present latency.

The phase-profiling script now instruments the shared copy helper as `CM2_COPY`
while retaining full-frame `capture_us` around index/capture publication. Its
old-function instrumentation remains usable for parent comparisons; stale
whitespace anchors were corrected. Fresh parent/current probe builds and a
one-repetition ink smoke test produce the expected frame/capture/copy records.
These instrumented executables are diagnostic only, excluded from qualification.

Final stage checks: **147 GPU tests / 23 hardware benchmarks ignored**, four
project round trips, isolated GTK file workflows and injected GPU failure/recovery
all pass. Logs: `native-capture-native-build.log`,
`native-capture-full-gpu-tests.log`, `native-capture-project-run.log`, and
`native-capture-gtk-{files,recovery}` logs. The recovery test's intentional wgpu
panic is handled successfully. Changed Rust formatting, Python parsing and
`git diff --check` pass. Native document/raster format adoption, source/composite
residency, cold preparation and the complete GTK SDR journeys remain pending.

## Shared native raster decoding and restoration (parent `d56520f`)

The fixed sixteen-slot original-image decode cache also accepts committed integer
raster blobs. It keeps weak blob identity, source working-space identity and
requested destination space in its key, so a different interpretation cannot
reuse pixels with the previous meaning and history backing is not pinned by GPU
cache entries. Both consumers share the integer upload textures, three transfer
curves, sixteen Float32 textures and sixteen-upload ceiling. There is no second
retained decoded-paint cache. Stored straight RGB is associated after decoding;
stored linear-premultiplied RGB is not multiplied by coverage again. Zero coverage
produces canonical transparent black without changing retained native bytes.

`restore_native_tiles` queues up to sixteen full tiles into caller-owned private
RGBA32Float candidates, consuming each cache view before a later request can reuse
its slot. It validates destination shape/format/usage, duplicate destinations,
integer descriptors and declared profile meaning before recording restoration.
Corrupt compressed contents still fail on bounded decode. Callers must discard
all candidates on error; this primitive publishes no document revision. Like
source preparation it may drain preceding upload work at the staging ceiling and
belongs in scheduled cold work, outside input handling. `prepare_native_transfer`
lets writeback use the very same curve buffer as source/raster decoding and moves
curve preparation outside interaction.

Physical GPU correctness covers 112 space/depth/alpha combinations, each checked
in its native space and sRGB. The fixtures exercise every integer16 code, all
integer8 codes, zero/one/two/seventeen/half/full coverage and varying coverage.
Native code recovery has zero error; cross-space Float32 premultiplied output is
within the predeclared absolute linear tolerance 3e-6 against f64 transfer/matrix
reference calculations. Sixty-four mixed original/native misses reuse exactly
sixteen physical textures. Unencoded and encoded-but-discarded reservations are
invalidated; successful retry, weak backing release, both profile components of
the key, and descriptor rejection pass. Sixteen space/depth/alpha combinations
also complete four physical writeback → compressed capture → restore cycles,
with exact native bytes across cycles and restored/canonical component agreement
within 2.4e-7. These are native tile primitives, not a qualification of the still
pending document tool pipeline.

The shared decoded cache maximum remains **18.25 MiB**: sixteen MiB of Float32
slots, 0.75 MiB of integer input textures and 1.5 MiB of transfer tables. At most
four/eight MiB of integer8/integer16 upload payload is in flight for a sixteen-tile
batch (ICC Float32 uploads retain the existing sixteen-MiB limit). Curve creation
uses its separately recorded mapped initialization. A caller retaining sixteen
Float32 working candidates adds sixteen MiB; that allocation is not hidden in the
decode-cache counter. Mutable working-tile eviction and full photo residency are
still separate, unfinished integration work.

The complete GPU suite passes **151 tests / 24 hardware benchmarks ignored**
(`native-raster-full-gpu-tests.log`). The new restore workload measures one or
sixteen caller-owned destinations, all cache hits or cyclic misses over twenty
compressed native tiles, both depths, and the three distinct transfer curves.
Twenty warm-up batches precede one hundred measured batches in each case; two
runs cover **4,800 measured batches**. CPU time includes restore validation,
cache lookup, bounded decode/hash/upload on misses, uniforms, copies and queue
submission. Completed time explicitly waits for the last queue work. Source
fixture creation, destination allocation, writeback, capture/compression,
document publication and presentation are outside this timing window.

Across both runs, sixteen cache hits have completed p95 **0.0847–0.1062 ms** and
CPU p95 at most **0.0273 ms**. Sixteen integer8 misses have completed p95
**2.6518–3.7434 ms**; sixteen integer16 misses are **10.3709–10.5893 ms**. This
uncached batch fails an 8.33 ms interactive budget and must be scheduled before
input consumes it. One integer16 miss has completed p95 **0.5142–0.7015 ms**.
The ProPhoto integer8 single-miss case has repeatable completed p99
**2.1106–2.1234 ms** despite p95 below 0.20 ms; the outlier source is not yet
attributed. It is retained as a scheduling risk, consistent with the earlier
isolated upload/submission stalls. Process high-water is **144,348 / 140,168 KiB**;
this is the small tile fixture, not a whole-photo memory qualification.

Fresh parent/current/current/parent runs of the unchanged original-image
benchmark cover **3,200 measured sixteen-miss batches**. The previously observed
roughly seven/ten-ms integer16 CPU bands remain present in both implementations;
the change does not establish their cause. Matching slower-band completed p95
ranges include sRGB16 parent 10.2127 ms versus current 10.1938–10.5131 ms,
P3 integer16 parent 10.0878–10.2014 versus current 10.0217–10.1629 ms, and Adobe
integer16 parent 9.9668–10.0221 versus current 10.1601 ms (the other current run
is in the faster band). The relative regression trigger does not reproduce
across the paired arms. Source and native restore still require cold scheduling.
Source process high-water varies 142,160–180,012 KiB across the four runs; no
process-memory reduction is claimed from that variation.

Reproduction: release-build the GPU test executable at parent `d56520f` and this
change; select `LAYER_GPU_INDEX=0`. Run `native_source_decode_workloads --ignored
--nocapture --test-threads=1` in parent/current/current/parent order, then run
`native_restore_workloads` with the same flags twice. Each executable is copied
only after a successful build. `native-raster-benchmark-runs.json` records binary
hashes and elapsed times; `native-raster-decode-measurements.json` contains all
case values, with raw `native-raster-source-*` and `native-raster-restore-bench-*`
logs. Compilation and other GPU workloads do not overlap the measurements.

The fresh fixed/parent/current drawing comparison covers **32,760 measured
frames**, all twenty-five scenarios with three repetitions per arm. Fixed
`e46f271` and parent `d56520f` use the previously verified, saved production
executables; the current executable is rebuilt successfully from this change.
These are new runs, not reused timings. Maximum CPU Move p95/p99 is
**2.089/2.814 ms fixed**, **2.082/2.849 ms parent**, and **2.128/2.762 ms current**.
Every Move/Pen-up sample stays within 8.33 ms. All twenty-five PNGs are
byte-identical across all three arms. Reports and hashes are
`native-raster-frame-{0-fixed,1-parent,2-current}.md`,
`native-raster-frame-runs.json` and `native-raster-frame-measurements.json`.
The short-run relative triggers include palette knife versus fixed, natural
blender versus both baselines, and watercolor wash/wet watercolor versus parent;
longer paired checks follow below. This harness measures CPU frame creation and
completed work, not native input-to-present.

Ten-repetition fixed/parent/current/current/parent runs of palette knife, natural
blender, watercolor wash and wet watercolor add **29,200 measured frames**. All
stay below 8.33 ms. Palette Move CPU p99 is 1.738/1.726 ms current versus 1.682 ms
fixed and 1.820/1.939 ms parent; completed p99 is 2.819/2.761 versus 2.768 fixed
and 2.744/2.976 parent. Watercolor wash completed p99 is 4.350/4.381 current versus
4.328 fixed and 4.371/4.345 parent; wet watercolor CPU p99 is 2.795/2.808 versus
2.813 fixed and 2.860/2.804 parent. Those initial Move triggers clear. Natural
blender Pen-up CPU p99 varies 1.792/1.602 current and 1.530/2.169 parent;
completed p99 is 2.989/2.833 current versus 3.161 fixed and 2.664/3.403 parent,
so the initial Pen-up trigger does not reproduce across both paired arms.
Palette Pen-up CPU p99 remains above the single fixed arm (2.433/2.220 versus
1.984 ms), while parent is slower at 3.271/3.428 ms; completed Pen-up is within
the fixed comparison. A longer fixed/current check follows. Raw
`native-raster-repeat-*` reports and `native-raster-repeat-measurements.json`
retain every arm, including slower parent values.

An isolated phase diagnostic narrows the single-tile stall to **`Queue::submit`**.
At ProPhoto integer8 miss frames 25/57, command finalization is about
0.009–0.010 ms and the queue call is **2.04–2.21 ms**, while decode/job preparation
is about 0.18 ms. The same diagnostic sees a roughly 2 ms queue call on a fully
cached integer16 restore at frame 68. This distinguishes the stall from native
transfer calculation, decompression and command-buffer finalization; it does not
establish the underlying wgpu/driver cause. First use of each transfer curve
still has the expected separate 2.2–3.0 ms preparation cost. The probe is generated
in a fresh temporary archive by `artifacts/color-m2/trace-native-restore.py`;
`native-raster-restore-profile-{0,1}.log` contains phase records,
`native-raster-restore-coarse-{0,1}.log` preserves the first diagnostic, and the
successful build/location are recorded alongside them. Instrumented executables
are excluded from qualification and do not modify production code. Cold work and
submission scheduling remain required before exposing native photo editing.

The final thirty-repetition fixed/current/fixed palette check adds **13,140
frames** and clears the remaining short-run CPU trigger. Pen-up CPU p99 is
**2.137 ms current**, between fixed **2.167/1.979 ms** and within the threshold
of both. Completed Pen-up p99 is **3.493 ms current** versus fixed
**3.805/3.777 ms**; Move CPU p95/p99 is **1.383/1.696 ms current** versus fixed
**1.386/1.706** and **1.443/1.903 ms**. No frame misses the absolute limit.
Reports are `native-raster-palette-{0-fixed,1-current,2-fixed}.md` and the run
manifest is `native-raster-palette-runs.json`. Across the initial and focused
comparisons this stage measures **75,100 drawing frames**. These checks do not
clear the earlier outstanding transform/prediction or full-photo limits.

Changed Rust formatting and `git diff --check` pass. The existing drawing path
remains active; native document/raster adoption, the common Float32 tool pipeline,
bounded edited/composite/filter residency, managed GTK viewing and the complete
SDR journeys still require implementation and qualification. No additional
platform host integration is enabled by this stage.

## Float32 working-target integration (parent `6f792db`)

Working attachment selection now travels with the renderer's pipeline recipes,
covering color pages, companions, brush reservoirs, scene tiles/images, effects,
transforms and query intermediates. The new internal qualification constructor
requests `FLOAT32_FILTERABLE` and `FLOAT32_BLENDABLE`, rejecting unsupported
capabilities instead of selecting a narrower format. These requirements match
[wgpu's Float32 feature definitions](https://docs.rs/wgpu/30.0.0/wgpu/struct.Features.html#associatedconstant.FLOAT32_BLENDABLE).
The exposed constructors still select the current sRGB8 path. Native publication,
document adoption and bounded photo residency must be integrated before enabling
the Float32 path in GTK. UI/output textures keep their declared output formats.

A measured prerequisite is **scalar working precision**, not only RGBA precision.
The first Float32 color prototype retained R8 stroke coverage. Repeating 128
uniform-accumulation dabs with requested alpha 1/65535 produced alpha
**0.0019512624**, instead of **0.000015259022**: rounding the saved coverage to zero
caused each frame to apply the same tiny coverage again. The failing physical test
is retained in `float32-working-coverage-probe.log` and its executable. Mutable
coverage, layer masks and wet state now use R32Float alongside RGBA32Float in the
new working path; their blend and transform recipes use the same selection.
Immutable byte brush-tip assets retain their explicitly eight-bit source samples.

The corrected test preserves both flow accumulation (128 separate physical
submissions, compared with the f64 source-over formula) and uniform accumulation
with component error below 3e-8. Native integer16 source → scene composition →
integer16 publication preserves every RGB code exactly at alpha codes
1/2/17/32768/65535. No-op Dry/Smudge/Wet/Liquify/Watercolor material paths also
preserve every opaque native RGB code, with checks on actual color/coverage/wet
texture formats. These are preparation and identity checks; active-edit precision,
resampling, extended effect behavior, capture and whole-workflow budgets are not
yet qualified. In particular, existing filter wrappers/built-ins still contain
SDR clipping that must be removed from the extended working path.

Residency accounting now derives color, scalar, transform, mask and image-cache
payloads from the actual formats instead of multiplying all color allocations by
four and all scalar allocations by one. The two initial corrected tests pass in
`float32-working-scalar-run.log`; all three working-target tests pass in
`float32-working-material-run.log`. These logs are correctness evidence only.


The fourth physical test applies 128 low-flow mask dabs and compares actual R32
mask coverage with the f64 accumulation reference (absolute error below 3e-8).
The successfully rebuilt production suite passes **155 tests, 24 ignored**, in
96.51 seconds (`float32-working-full-gpu-tests.log`). The four focused tests and
successful build provenance are retained in `float32-working-final-tests*`;
these checks still do not establish active material-edit integer16 tolerances.

Fresh parent/current/fixed all-scenario runs measure **32,760 frames**. Maximum
CPU Move p95/p99 is **2.268/2.849 ms parent**, **2.190/2.843 ms current**, and
**2.101/2.746 ms fixed**. All twenty-five scenario PNGs are byte-identical across
those arms; no Move or Pen-up sample exceeds 8.33 ms. Production executable,
run provenance and detailed metrics/hashes are `float32-working-frame`,
`float32-working-frame-runs.json`, `float32-working-frame-measurements.json`
and `float32-working-frame-{0-parent,1-current,2-fixed}.md`.

Short-run relative triggers prompted twenty-repetition parent/current/parent
runs of wet round Oklab, opaque gouache, loaded oil mixer, palette knife, wet
watercolor and anchored grain chalk: **44,520 additional frames**, none above
8.33 ms. Wet-round Move CPU p95/p99 becomes 1.416/1.731 ms current versus
1.468/1.835 and 1.438/1.852 parent; completed p95/p99 is 2.300/2.666 versus
2.415/2.820 and 2.494/2.807 ms. Loaded-oil Pen-up CPU p99 is 2.189 versus
2.114/2.014 ms; gouache is 2.155 versus 1.931/2.068 ms, which does not reproduce
the trigger against both parent arms. Wet-watercolor is 2.130 versus
2.263/2.003 ms. Chalk CPU Move p99 is 0.366 versus 0.521/0.378 ms. Palette
Pen-up CPU p99 remains 2.506 versus 2.271/2.172 ms, so it received another
focused check. All raw arms are retained as `float32-working-repeat-*.md`,
with run and measurement JSON files; the variable Pen-up values are not omitted.

Sixty-repetition parent/current/fixed palette runs add **26,280 frames** and
clear that trigger. Current Pen-up CPU p99 is **2.219 ms**, versus **2.562 ms
parent** and **2.295 ms fixed**; completed Pen-up p99 is 3.512 versus 3.612/4.000
ms. Move CPU p95/p99 is 1.448/1.866 current, 1.449/1.834 parent and 1.451/1.863
fixed. Reports are `float32-working-palette-{0-parent,1-current,2-fixed}.md` and
`float32-working-palette-runs.json`. No compilation ran during measurements.
The stage totals **103,560 measured drawing frames**, all below the absolute
limit. Longer samples distinguish the initial short-run tail variation from a
repeatable change; earlier transform/prediction and photo limits remain open.

This commit selects formats consistently through existing rendering recipes and
corrects scalar precision/accounting. It does not enable Float32 document editing
in exposed constructors. Native scalar backing, document/native publication,
bounded photo residency, extended effects, GTK viewing and user journeys remain
required before enabling the new mode. No other platform host is integrated.


## Native scalar writeback, capture and restore (parent `d2195b8`)

Mutable mask/wet-state working tiles need the same native precision boundary as
color tiles. `native_tiles::scalar` now quantizes R32Float into packed integer8 or
integer16 buffers and writes the corresponding canonical R32Float candidate.
The exact integer-significand coverage quantizer is shared with RGBA writeback;
there is no second approximate scalar rounding algorithm. One invocation owns
each packed u32 word, preserving components outside the requested region even
at odd pixel boundaries. The path does not require single-channel integer
storage-texture extensions. Nonfinite or out-of-range coverage rejects the
publication through the existing shared status; it is not clipped into validity.

`capture_tiles` now accepts encoded textures and packed scalar buffers through
one descriptor preflight, staging budget and worker. Each packed tile is exactly
64 KiB or 128 KiB. The captured status is checked before any backing ticket is
published, including mixed-depth captures; resetting the GPU status afterward
cannot erase an earlier capture failure. Cancellation retains the existing failed
ticket behavior. These validated capture types are exposed alongside the native
encoder primitives for a GPU owner to integrate, without changing host adoption.

Scalar restore decodes one backing tile at a time into a mapped 256 KiB R32Float
upload. It shares the source/raster sixteen-upload ceiling and drains that queue
when full. It creates no second decoded cache. Source texture shape, descriptor
and duplicate destinations are preflighted; late corruption discards the current
encoder. All outputs are private candidates, and a caller must discard the whole
set on failure, including candidates written by an earlier drained chunk.

Four physical tests pass. Every native code and neighboring Float32 values at
half-code boundaries match a separate f64 rounding reference at both depths;
five regions cover full, partial, last-row/column and empty writes, with exact
preservation outside the region. Canonical scalar error is below 6e-8. Six invalid
coverage cases reject every backing ticket in mixed-depth publication; negative
zero succeeds. Sixteen mixed-depth tiles survive four encode/capture/compress/
restore cycles byte-exactly (64 tile publications). Twenty sixteen-tile restore
batches total 320 tile restores and exercise the shared upload ceiling, with
peak charged staging **4 MiB**. Shape/depth/alias rejection and late corruption
followed by retry also pass. The existing RGBA reference corpus remains
**10,485,760 pixels with zero native-code error** after quantizer extraction.

The full production GPU suite passes **159 tests, 25 ignored**, in 98.31 seconds
(`native-scalar-full-gpu-tests.log`). That executable precedes only the visibility
change exposing the shared capture API; `native-scalar-public-{tests,frame}` are
successfully rebuilt from the same implementation with that API exposed. Build
JSON/logs and `native-scalar-restore-run.log` retain provenance and focused output.
No document/native edit mode or additional platform host is enabled here.

`native_tiles::scalar::tests::bench::scalar_native_workloads` measures separate
restore and writeback phases with 20 warm-up/100 measured batches per case, twice.
There are four restore cases (two depths × one/sixteen tiles) and sixteen encode
cases (add full/63×65 region and rebuilt/reused bindings): **4,000 measured
batches**. Fixtures and pipeline preparation are outside warm timing. CPU timing
includes validation and queue submission; completed timing explicitly polls all
submitted work. Encoding excludes preservation copies, capture, history and
presentation. Restore includes backing decode/digest validation, scalar conversion,
upload and submission. Cold creation and first restore are reported separately.

| Workload | CPU p95 ms | Completed p95 ms |
| --- | ---: | ---: |
| Restore one integer8 tile | 0.0825–0.0847 | 0.1129–0.1153 |
| Restore sixteen integer8 tiles | 1.1226–1.1272 | 1.2117–1.2190 |
| Restore one integer16 tile | 0.1808–0.1813 | 0.2137–0.2160 |
| Restore sixteen integer16 tiles | 2.7300–2.7330 | 2.8554–2.8593 |
| Write sixteen full tiles, rebuilt bindings, both depths | 0.0902–0.0931 | 0.2121–0.2207 |
| Write sixteen full tiles, reused bindings, both depths | 0.0297–0.0345 | 0.0918–0.0967 |

Cold pipeline preparation is **19.7915/0.5560 ms**, demonstrating why preparation
must precede interaction even for this small shader. First integer8 single-tile
restore completes in 2.8657/0.6848 ms. The measured sixteen-tile working/canonical
payload is 8 MiB plus 1/2 MiB of packed outputs, at most 4 KiB batch parameters
and 8 status bytes. Restore adds up to 4 MiB of charged uploads, one decoded
native tile (up to 128 KiB), its bounded unshuffle temporary and a 1 KiB row.
The existing 64 MiB capture pool and driver allocations are separate. Process
high-water marks are **194,372/141,752 KiB** for these small fixtures; they do not
qualify dense-photo residency. Raw timings and process statistics are
`native-scalar-workloads-{0,1}.log`.


Fresh ten-repetition parent/current/fixed runs measure **109,200 drawing frames**,
with all twenty-five scenario PNGs byte-identical and no Move/Pen-up sample over
8.33 ms. Maximum CPU Move p95/p99 is **2.169/2.863 ms parent**, **2.190/2.815 ms
current**, and **2.035/2.895 ms fixed**. Exact executable hashes and raw reports
are `native-scalar-frame-runs.json`, `native-scalar-frame-measurements.json` and
`native-scalar-frame-{0-parent,1-current,2-fixed}.md`. No compilation ran during
these measurements or the scalar workload runs.

Relative tail triggers remain under investigation. Against both fresh baselines,
wet-round Oklab Pen-up CPU p99 is 2.301 ms versus 1.692/1.573; dry scumble is
0.961 versus 0.696/0.663; natural blender is 2.432 versus 2.220/1.841; loaded-oil
Move CPU p99 is 2.635 versus 2.279/2.376. Liquify-twirl Move and wet-watercolor
Pen-up trigger against parent; pastel-block Pen-up and watercolor-wash Move
trigger against fixed. The full data retains all those comparisons. The capture
phase diagnostic now recognizes the public `capture_tiles` entry point; subsequent
focused runs will distinguish copy/capture costs from the prior variable tails.
The absolute pass and matching images do not clear these relative investigations,
or the earlier transform/prediction and full-photo failures. Native document
publication and the complete GTK color/photo journeys remain unimplemented.


## Document interpretation and typed pending backing (parent `ed0faad`)

Document interpretation now records independent built-in RGB space and committed
integer depth. Version 3 of the archive fixes the built-in definitions and checks
color, mask, wetness and watercolor-wetness layouts against that interpretation.
The old version 2 reader is removed. Nondefault native modes use straight RGB
codes so very low coverage does not destroy unassociated integer16 precision.
The currently exposed sRGB8 renderer retains its existing encoded linear
premultiplication until common-pipeline adoption; that transitional special case
is not a final precision qualification for native sRGB8.

Each pending raster tile now owns its declared pixel descriptor in the shared
publication allocation. Cheap revision clones still copy an Arc-sized handle.
Readback requests derive their descriptor from that ticket; the redundant capture
field and untyped default constructor are deleted. Publishing a different layout
fails and wakes waiters with an error. Index validation checks pending layouts
without waiting for CPU backing. History charges each retained ticket's own
precision, including old revisions whose layout differs from the current
document. Invalid pending layouts receive the conservative maximum tile charge
until rejected by adoption/storage, rather than panicking during history trim.

The renderer declares its prepared document interpretation. Engine construction
and device replacement reject a mismatch before resizing, consuming input or
changing history. The exposed GPU/GTK constructors still declare sRGB8; this
change does not enable a native editing mode. Tests cover all eight built-in
space/depth combinations, failed replacement retaining queued input/checkpoint,
and successful replacement with matching interpretation.

Archive tests round-trip every code at both depths in four spaces, all four
persisted raster planes, exact descriptor/digest/bytes, shared color-tile identity
and byte-identical resave. Wrong color/scalar depth and the previous archive
version fail. A history test retains three 500-tile pending revisions: the 512 MiB
limit keeps all three integer8 revisions but only two integer16 revisions, even
with a current sRGB8 document. It also exercises malformed-layout accounting
without allocating any pixel payload.

Validation: **56 core, 50 engine and 370 UI tests pass**. The final additional
history test is in `document-color-history-tests.log`; earlier combined output is
`document-color-shared-tests.log`. GTK test compilation passes. The saved
`document-color-ticket-gpu-tests` executable passes **159 GPU tests, 25 ignored**,
in 102.52 seconds (`document-color-full-gpu-tests.log`). Production GTK native
files and diagnostics/device-failure/recovery tests both pass using
`document-color-ticket-gtk-tests` under private Mutter at 120 Hz/GSK Vulkan.
Reproduce with `bash tools/performance/gtk-raster.sh BINARY FILTER PREFIX` and
filters `native_document_files` and `native_diagnostics_and_gpu_failure_recovery`.
Build JSON/logs and `document-color-gtk-*-run.log` retain provenance. The GPU/GTK
executables precede only the final conservative invalid-layout history charge
and its core test; `document-color-production-frame` includes them.

The scalar-stage phase investigation uses isolated `d2195b8`/`ed0faad` probes,
created by `tools/performance/trace-raster-commit.py`. Each of wet-round Oklab and
natural blender runs 30 repetitions in parent/current/parent order. The last one
or four traced stroke ends per repetition, respectively, are the measured
window; setup and the first warm-up stroke are excluded. Raw phase records and
summaries are `native-scalar-trace/phase-measurements.json`, with all original
logs/reports retained. These instrumented runs diagnose costs; production
comparisons determine qualification.

Warm capture allocation time is zero at p99 in all six arms. Natural-blender
capture p99 is 0.320 ms current versus 0.323/0.383 parent, with similar copy
finishing/submission costs. Wet-round capture p99 is 0.704 ms versus 0.543/0.437;
its copy-command finish p99 is 0.326 versus 0.235/0.213, and copy submission p99 is
0.126 versus 0.051/0.037. Frame encoding p99 is 0.594 versus 0.622/0.604. This
localizes that observed increase to capture command finishing/submission and
related overhead, without identifying a reproducible driver or code cause.
These observations do not clear every relative trigger from the preceding stage.
No compilation ran during any phase or production measurement.

Fresh production ten-repetition parent/current/fixed runs measure **109,200
frames**. All 36,400 current frames pass the 8.33 ms Move/Pen-up gate and all
25 PNGs are byte-identical across all three arms. Maximum CPU Move p95/p99 is
**2.193/2.940 ms parent**, **2.168/2.845 current**, and **2.403/3.123 fixed**.
The parent has one loaded-oil completed Move deadline miss; fixed has one
watercolor-wash completed Move miss. These samples remain in their reports.
The entire baseline runs therefore do not receive an absolute pass.

Remaining relative triggers in this comparison are opaque-gouache Pen-up
completion and palette-knife Move CPU/completion against parent; wet-round
Pen-up CPU/completion and palette-knife Pen-up CPU/Move completion against
fixed. Raw values, sample counts, PNG hashes and all triggers are in
`document-color-frame-measurements.json`; executable hashes and elapsed run
times are in `document-color-frame-runs.json`. Reproduce each saved executable
with `--scenario all --repeats 10 --output-dir PREFIX --report PREFIX.md` and
`LAYER_GPU_INDEX=0`. Focused paired runs remain necessary to assess these tails;
the preceding phase diagnosis alone does not clear them. Native color/depth
adoption stays disabled, and dense-photo/transform memory and latency failures
remain outstanding.

## GTK display integration audit (implementation still outstanding)

The GTK canvas is a Vulkan WSI surface on an application-owned Wayland child
subsurface (`wayland.rs` and `render_thread.rs`), not a GTK-managed image texture.
Applying a GTK widget color state alone therefore does not establish the canvas's
image description. Current creation selects a non-sRGB swap-chain format and
leaves the surface color space at its automatic default. Native drawing/recovery
checks do not verify wide-gamut presentation or monitor transitions.

The pinned wgpu 30.0.1 source already exposes per-format color-space capabilities
and explicit `SurfaceConfiguration.color_space`; Auto does not select a wide
space for ordinary integer formats. The pinned Wayland color-management v3 XML
specifies commit-buffered surface image descriptions and implementation-defined
behavior for an untagged surface. The compositor performs output conversion for
tagged surfaces, including surfaces spanning outputs, as described in the
[official Wayland color-management overview](https://wayland.freedesktop.org/docs/book/Color.html).
A future integration must first inspect actual WSI/compositor capabilities and
protocol ownership: independently installing a second surface-color object may
conflict with Vulkan WSI's own object. No profile/fallback/monitor behavior is
claimed from this source audit alone, and no display code is changed here.


## Canonical working-value adoption (parent `0ec14e8`)

`native_tiles::promote` adds the GPU step that copies canonical native-decoded
values back into existing RGBA32Float/R32Float working attachments. It reads the
shared validation result after all color/scalar compute batches. If any batch
failed, every promotion discards its fragment output and leaves its destination
unchanged. CPU backing capture checks the same result independently. This
primitive does not itself restore a failed provisional stroke or publish a
document revision; the existing edit/recovery owner must perform that integration.

Color writeback preflight now also rejects duplicate encoded/canonical outputs
and input/output aliases between requests, matching scalar validation. Its
sixteen-tile mixed-depth fixture uses private canonical destinations and adds
four cross-request alias failures. Repeated read-only working inputs remain
valid. This closes a missing check before the primitives are used by a live
publication owner.

The shader uses a physical render-pass boundary and WGSL's defined
[discard semantics](https://www.w3.org/TR/WGSL/#discard-statement), which suppress
fragment output. It requires no extra storage usage on working attachments, no
new pixel payload allocation, no filtering/conversion and no input-owner readback.
Each prepared batch validates at most sixteen distinct input/output pairs, shape,
format, usage and region before recording any writes. Callers must retain unique
candidate ownership across the entire publication and delay every promotion
until all encoding batches have completed in queue order. Empty regions produce
no render pass. Existing live document/host modes are unchanged.

Three physical tests pass: 1,310,720 fixture texels verify exact Float32 bytes
through full/odd/last-row/last-column/empty regions at both attachment formats,
with invalid status preserving every pixel. The corpus includes fractional
values that would be narrowed by Float16 and finite extended-range values; it is
not a claim to test every IEEE bit pattern. A mixed native integer16 test encodes
either bad color or bad scalar last in separate compute passes, then attempts
both promotions and one shared native capture. Every destination remains exact
and every backing ticket fails; resetting status permits a subsequent successful
publication. Count/layout/region/cross-request alias failures are preflighted.

The complete production GPU suite passes **162 tests, 26 ignored**, in 96.83
seconds (`native-promotion-full-gpu-tests.log`). After the additional color
ownership preflight, it passes again in 102.07 seconds
(`native-promotion-owned-gpu-tests.log`, saved `native-promotion-owned-tests` and
matching build JSON/log). The latter is a correctness run concurrent with
compilation of isolated diagnostic probes; its elapsed time is not a latency
comparison. Focused output is
`native-promotion-focused-tests.log`; `native-promotion-final-tests` and its build
JSON/log retain the successfully built executable. No new GTK host adoption is
introduced by these primitives, so the document/storage GTK checks above remain
the applicable host evidence, not native color-mode qualification.

An ignored physical workload covers two formats × one/sixteen tiles × full/63×65
regions × rebuilt/reused bindings, with twenty warm-up and one hundred measured
batches per case, twice: **3,200 measured batches**. Timing includes preparation
when rebuilding bindings, command encoding and queue submission; completed timing
polls all submitted work. Pixel initialization is outside timing. These costs
exclude native quantization, capture, history, composition and presentation.

| Full-tile workload | CPU p95 ms | Completed p95 ms |
| --- | ---: | ---: |
| One RGBA32 tile, rebuilt bindings | 0.0191–0.0281 | 0.0478–0.0584 |
| One RGBA32 tile, reused bindings | 0.0096–0.0097 | 0.0331–0.0334 |
| Sixteen RGBA32 tiles, rebuilt bindings | 0.1220–0.1280 | 0.2402–0.2717 |
| Sixteen RGBA32 tiles, reused bindings | 0.0682–0.0684 | 0.1388–0.1402 |
| Sixteen R32 tiles, rebuilt bindings | 0.1170–0.1658 | 0.2337–0.2824 |
| Sixteen R32 tiles, reused bindings | 0.0654–0.0734 | 0.1265–0.1311 |

One-tile rebuilt RGBA32 CPU p99 reaches 0.6089–0.6103 ms; reused bindings reduce
that tail to 0.0106–0.0118. The data therefore favors retaining bounded bindings
when resource identities allow reuse. Sixteen-tile partial-region complete p95
is 0.1252–0.1563 ms RGBA32 and 0.1234–0.1276 R32 with reused bindings. Pipeline
construction takes 0.3992–0.4068 ms after the correctness run has warmed the driver
cache; this is not a cold-cache qualification. First full sixteen-tile rebuilt
RGBA32 batches complete in 0.5012–0.5122 ms.

Fixture canonical plus working payload is 32 MiB for sixteen RGBA32 tiles or
8 MiB for sixteen R32 tiles. Promotion adds the existing shared eight-byte status,
at most sixteen bind groups/views per batch, and no parameter slab or pixel
scratch. Process high-water marks are 138,908/135,880 KiB; these small fixtures
do not qualify dense-photo memory. Reproduce with the saved test binary and
`native_tiles::promote::tests::canonical_promotion_workloads --ignored
--test-threads=1 --nocapture`; raw timing/process output is
`native-promotion-workloads-{0,1}.log`. No compilation or other GPU test ran during
these measurements. Full native edit publication, bounded large operations,
precision across all active tools, managed viewing and GTK color/photo journeys
remain required.


The metadata-stage focused parent/current/fixed comparisons use sixty repetitions
each for wet-round Oklab, palette knife and opaque gouache: **60,660 frames**,
with matching output PNGs in every arm. Palette-knife and opaque-gouache relative
triggers clear in these larger samples; all their frames meet 8.33 ms. Wet-round
still fails: current Pen-up CPU/completed p99 is **2.766/3.806 ms**, versus
**2.219/3.358 parent** and **2.512/3.592 fixed**. Current also has three completed
Move misses, maximum **34.086 ms**, while those two baseline arms have none.
Capture allocated/reserved peak is 120 MiB current versus 56/64 MiB. These samples
remain in the results, and existing drawing performance is not declared fully
qualified. Raw reports, executable hashes, counts and comparisons are
`document-color-focused-*`, `document-color-focused-runs.json` and
`document-color-focused-measurements.json`. The diagnostic helper now supports
all-frame tracing and capture-worker poll/total backing time to investigate this
specific sustained failure, rather than repeating the complete drawing suite.


Native color ownership preflight is also measured in parent/current/parent
writeback runs: 96 cases × 100 measured batches per arm, **28,800 batches**.
Maximum sixteen-full-tile rebuilt-binding CPU p95/p99 is 0.1768/0.2654 ms
current versus 0.1432/0.2923 and 0.1709/0.2911 parent; completed p95/p99 is
0.3744/0.5421 versus 0.3476/0.5013 and 0.3690/0.4842. Reused-binding current
maxima are 0.0471/0.0830 CPU and 0.1590/0.1852 completed. This comparison
retains the ownership checks without a meaningful cost increase at this batch
size. Logs are `native-promotion-ownership-{0-parent,1-current,2-parent}.log`,
with parsed cases in `native-promotion-ownership-measurements.json`.

The `ed0faad`/`0ec14e8` all-frame diagnostic probes run wet-round sixty times in
parent/current/parent order (**8,100 measured frames**). All three diagnostic
arms remain below 8.33 ms, so they do not reproduce the earlier 34.086 ms miss.
Current Pen-up CPU p99 remains 2.394 ms versus 1.922/2.058 parent. Capture p99
is 0.482 ms versus 0.463/0.540; copy finishing is 0.246 versus 0.241/0.253,
with essentially no warm allocation. Current frame encoding p99 is 0.617 ms
versus 0.477/0.467, and frame submission 1.091 versus 0.999/1.009. The shared
capture copy is therefore not a sufficient explanation of the remaining tail.
No code cause or fix is claimed from these traces. Worker poll/total timings,
all frame records and slow-frame diagnostics are retained under
`document-color-trace/`, including build provenance and
`all-frames-measurements.json`. Traced timing is diagnostic evidence only;
the production sustained failure remains open.


## Connected native edit publication and history (2026-09-14)

The Float32 renderer now has a native document constructor for headless workflow
qualification, `WgpuRasterizer::new_native_headless(DocumentColor)`. It declares
the same profile/depth to the engine, decodes retained sources and native tiles
into the document's linear RGB coordinates, and prepares native pipelines and
scratch before editing. This constructor does not enable a GTK mode or claim
managed viewing. Existing interactive factories still select the qualified
sRGB8 path. The default sRGB8 archive descriptor still has its previously
recorded premultiplied-linear association; the other native modes use straight
encoded RGB. Final common-mode cleanup and low-alpha source policy remain part
of adoption, rather than a compatibility promise.

Pending raster roots now trigger native commit encoding after persistent dabs,
mask edits and stroke-edge work, before preview copies and composition. Every
changed color and scalar page is validated before any canonical value is adopted.
Validation and the encoders share the same bit-based non-finite/coverage checks.
After that scan, batches of at most sixteen pages quantize into reusable scratch,
promote canonical Float32 samples, and record exact native copies before reusing
the scratch. A later bad color or mask therefore cannot leave an earlier batch
quantized while the publication fails. Finite extended RGB clips only at this
native write boundary; invalid coverage/non-finite data rejects the publication.
Failure still requires the existing checkpoint/recovery owner to discard the
provisional edit; promotion does not roll back preceding dabs.

Capture preparation now records into the owning frame's command stream. Mapping
starts only after submission, then the existing bounded worker compresses and
publishes the typed tickets. Dropped prepared captures fail their tile tickets;
dropped native frames also fail their pending roots. Undo/open/replacement decode
native color and scalar backing into private candidates before replacing live
pages. Unchanged tile tickets are reused. The command wrapper accounts for native
promotion passes in its existing submission ceiling.

The owner reserves sixteen RGBA32 canonical/color-encoded slots and sixteen R32
canonical/scalar-encoded slots. Their payload plus the shared status is
25 MiB + 8 bytes at U8 and 30 MiB + 8 bytes at U16; transfer curves remain owned
by the shared decoder cache. This scratch is included in renderer telemetry.
Readback checks aggregate rounded chunks plus each validation copy against the
existing 256 MiB frame ceiling; worker and spare-pool budgets are unchanged.
These are allocation contracts, **not measured photo-memory qualification**.
Full-photo operations that exceed the capture ceiling still need scheduled,
streamed publication; mutable/composite/filter residency remains unfinished.

Connected correctness fixtures cover:

- All four built-in spaces at both integer depths, through actual `CanvasEngine`
  pen input, two committed strokes, undo/redo, native save/reopen, matching
  continued painting, renderer replacement and undo after replacement. Stored
  native bytes and canonical Float32 working bytes match exactly between these
  states. Replacement retains the checkpoint and history; this test destroys
  the retired device after replacement and does not simulate every device-loss
  timing during active input.
- Seventeen tiles each of color, wetness, watercolor wetness and a layer mask
  (**68 pages**) crossing five mixed capture batches. Color uses alpha 17/65535;
  scalar ramps cover all 65,536 codes. Three recommits preserve exact native
  bytes, scratch allocation stays fixed, and an unchanged revision reuses tickets.
- A deliberately invalid seventeenth color page, or the final mask page, rejects
  all captures and leaves every provisional working byte unchanged. An earlier
  valid noncanonical pixel detects accidental partial promotion. The last backed
  checkpoint remains independently restorable on a replacement renderer.
- Abandoning an encoded but unsubmitted frame fails both roots and all tile
  waiters, and leaves live working bytes unchanged.

The first two connected tests passed, followed by all four expanded fixtures.
Reproducible final build/test artifacts and broader validation are recorded below.
No benchmarks or optimization experiments were run for this intermediate step;
the user's requested final-phase ordering applies.

Final validation of this change: **166 GPU tests pass, 26 benchmark/large-workload
tests remain ignored**. `cargo check --offline -p layer-linux --tests` passes.
The existing GTK `native_document_files` and
`native_diagnostics_and_gpu_failure_recovery` tests both pass under the isolated
120 Hz Mutter/GSK Vulkan harness on the previously recorded NVIDIA device.
This verifies the existing host route; native color mode is not yet exposed in
that host. The GPU correctness suite overlapped GTK compilation; test elapsed
time is not used as latency evidence.

Reproduce from this implementation using:

```sh
cargo test --offline --release -p layer-render-wgpu --lib -- --test-threads=1
cargo test --offline --release -p layer-linux --bin layer-linux --no-run --message-format=json
# Select the executable from the successful Cargo compiler-artifact message:
bash tools/performance/gtk-raster.sh "$GTK_TEST_BINARY" native_document_files artifacts/color-m2/native-edit-files
bash tools/performance/gtk-raster.sh "$GTK_TEST_BINARY" native_diagnostics_and_gpu_failure_recovery artifacts/color-m2/native-edit-recovery
```

Saved executables are `artifacts/color-m2/native-edit-final-tests` and
`native-edit-gtk-tests`; SHA-256 hashes and parent revision are in
`native-edit-provenance.json`. Cargo JSON/build logs use `native-edit-final-build`
and `native-edit-gtk-build`; results are `native-edit-full-gpu-tests.log`,
`native-edit-gtk-check.log`, `native-edit-files.log` and `native-edit-recovery.log`.
Earlier focused runs used the intermediate `native-edit-tests` executable and
are recorded in `native-edit-focused.log`; final GPU results include all four
fixtures plus the command-wrapper accounting change.

Next functional work is the extended Float32 tool/effect contract, bounded large
operations/residency and the remaining connected GTK color/photo journeys. The
68-page fixture is not a dense-photo throughput or memory pass. Managed viewing,
profile/depth actions, inspection/UI, remaining interchange and final hardware
qualification are still required. Other platform hosts remain untouched and
require the user's later approval.


## Native Float32 photo adjustments and editable-master round trips (2026-09-14)

Native effect compilation now retains the document's RGB space along with its
working attachment format, including compiler-thread contexts. Native helpers
use the selected space's signed transfer curve and primary-derived luminance
weights. The definitions continue to match the independently checked
[CSS Color 4 conversion reference](https://www.w3.org/TR/css-color-4/#color-conversion-code).
This applies to both native integer depths; profile selection does not follow
bit depth. Existing interactive sRGB8 factories retain their current effect
contract until common-mode adoption.

The common Float32 wrapper no longer clamps every adjustment result to bounded
RGB or divides by an alpha epsilon. Zero coverage returns zero RGB; positive
coverage retains its color. Native Exposure keeps EV/offset in linear RGB,
uses a signed extension for non-unit gamma, and retains values outside native
SDR bounds between live nodes. Unit-gamma EV/offset operates directly on
premultiplied RGB; integral EV stops use exact binary exponent scaling. Normal
alpha-preserving adjustments avoid redundant association round trips. Full
strength and bypass return their selected values directly. White Balance is
explicitly a rendered-image RGB gain/tint correction, with optional preservation
of document-space luminance; it is not a RAW/CCT reconstruction. Native image
passes use four explicit Float32 texel loads for interpolation.

Correctness tests fixed their acceptance threshold at **2e-6 absolute error in
straight linear RGB with exact coverage** before acceptance. The initial
24-node inverse-exposure fixture failed at alpha 8e-8: the slider result was
3.0000024 rather than 3. Eliminating association round trips and enforcing exact
integral-stop gain did not eliminate this failure. Returning the selected operand
at interpolation weights zero/one did, with the original tolerance unchanged.
This is consistent with cancellation in interpolation lowering; no driver ISA
analysis is claimed. WGSL defines interpolation and its arithmetic accuracy
separately from exact operand selection; see
[WGSL mix](https://www.w3.org/TR/WGSL/#mix-builtin) and
[floating-point accuracy](https://www.w3.org/TR/WGSL/#floating-point-accuracy).
These changes address numerical correctness, not performance tuning.

Three connected fixtures now pass:

- Twenty-four alternating +5/-5 EV nodes, with fused and explicit image-pass
  execution, all four spaces and both native depths, and coverage 0, 8e-8,
  1/65535, 0.5 and 1: **80 cases**, each evaluated initially, after a slider
  change, and after reset. The constant-color fixture includes negative and
  greater-than-one RGB. Its inspected Float32 sample meets the threshold;
  resetting the slider restores the exact prior sample.
- Rendered white-balance gains, luminance preservation and encoded-domain
  brightness evaluated against Float64 reference arithmetic in all four spaces,
  through fused and physical passes, at coverage 8e-8. These meet the same
  threshold and preserve coverage exactly.
- Eight profiled retained-photo cases (four spaces × two depths), with Exposure,
  White Balance, Levels, Curves, Hue/Saturation and Color Balance plus masks,
  native save and reopen on a fresh renderer. Entire 256×256 Float32 composites
  match byte-for-byte. Editing the reopened exposure changes the result;
  restoring its saved value restores the exact composite. Source profile, depth
  and source-backed ownership survive; no source rasterization is introduced.

This is not a full precision qualification for every brush, blend mode, nonlinear
control, spatial filter or resampling coordinate. Artistic HSL/Levels/Curves
clamps and their extended/default behavior still need explicit acceptance;
non-Normal blend low-alpha handling and other sampling paths remain to be
completed. The image sampler change is not a large-photo memory/latency pass.
Photo/file/dialog UI, managed viewing, conversion/depth actions, inspection,
remaining interchange and bounded global operations are still required.

Final validation: **169 GPU tests pass, 26 remain ignored**; shared suites pass
**56 core, 50 engine and 370 UI tests**, and the GTK test build check passes.
The GPU correctness suite overlapped shared compilation/testing; elapsed test
time is not performance evidence. The prior connected-publication commit's GTK
file/recovery runtime evidence remains recorded separately above.

Reproduce with `cargo test --offline --release -p layer-render-wgpu --lib --
--test-threads=1`, `cargo test --offline --release -p layer-core -p layer-engine
-p layer-ui`, and `cargo check --offline -p layer-linux --tests`. The saved GPU
binary is `artifacts/color-m2/native-effects-tests`; hash and parent are in
`native-effects-provenance.json`. Successful Cargo JSON/build logs are
`native-effects-build.{json,log}`. Results are `native-effects-focused.log`,
`native-effects-full-gpu.log`, `native-effects-shared-tests.log` and
`native-effects-gtk-check.log`. Failed strict-tolerance runs are retained as
`native-effects-rounding-before-{gain,mix}.log`; the final run passes that same
threshold. Further performance measurement and optimization remain deferred.


## Document-to-view color boundaries and explicit GTK sRGB (2026-09-14)

Viewport, sRGB export, Navigator and both paint/source thumbnails now transform
native document primaries at the output boundary. Native thumbnail intermediates
retain Float32 until that conversion, avoiding a gamut clamp in working RGB.
Raw point/average samples remain straight linear document RGB. ReadbackImage
remains explicitly sRGB8; this does not yet connect profiled native-depth delivery
to whole-composition export. Viewing leaves document and retained-source bytes
unchanged.

The presenter accepts an explicit sRGB, Display P3 or extended linear sRGB surface
contract. Artwork converts from the renderer's space; colored application surround
and overview outlines convert from their sRGB definitions. Encoded targets and
linear floating targets select different output transfer behavior. Native
Float16 display output is permitted, while Float32 document editing is retained.
This is SDR viewing support, not an HDR editing claim. Primary conversions use the
previously verified [CSS Color 4 matrices and adaptation](https://www.w3.org/TR/css-color-4/#color-conversion-code).

The pinned wgpu-types 30.0.1 `src/surface.rs` independently establishes the surface
contract: Auto may select extended linear sRGB for floating targets; sRGB/P3
transfer encoding still depends on the texture format. GTK now requests explicit
sRGB and an advertised compatible format. The isolated native session reports
Rgba8Unorm + sRGB; NVIDIA Vulkan/Mutter also advertises Display P3 for several
formats, including Rgba16Float, but does **not** advertise extended linear sRGB.
Those capability bits are not calibrated-monitor or compositor conversion proof.
GTK wide-surface selection, monitor changes and picker agreement remain work;
no duplicate Wayland color-management surface is installed over WSI ownership.

Four focused GPU fixtures cover:

- Four working spaces × two native depths, retained ProPhoto16 sources (including
  out-of-working-gamut colors and coverage 1/65535), sRGB export, Navigator, retained
  source thumbnails, raw point/Average5 samples and unchanged composite ownership.
  Output error is at most one 8-bit code; raw RGB uses an absolute 3e-6 threshold.
- Four working spaces on explicit sRGB/P3 UNORM and sRGB-format attachments, plus
  extended linear Float32 and Float16 output, with colored UI surround. Float32
  output uses absolute 3e-6; display-only Float16 permits its independently declared
  0.05% relative rounding plus 3e-6 arithmetic error. Invalid extended-linear UNORM
  configuration fails before presenter creation.
- Native paint thumbnails through the ordinary cropped paint route, checked against
  the same Float64 profile-conversion reference within one output code.
- Hidden RGB injected at exactly zero coverage in both attachment modes: export and
  Navigator must return transparent black, with source composite bytes untouched.

The first full suite failed the independent filter PNG comparison at four pixels:
all had alpha zero and hidden nonzero RGB from the former epsilon division. The
PNG/checksum remain unchanged. The comparison now applies the declared canonical
zero-coverage output contract to the reference and still compares every channel;
its one-byte tolerance is unchanged. The separate injected-RGB test directly
checks the contract. This is an explicit correctness correction, not a tolerance
relaxation. The first full run (171 pass/one failure) and focused failure are kept
as `view-color-full-gpu.log` and `view-color-filter-recheck.log`.

GTK development test check passes. Real private-Mutter checks pass for sampling
controls (4.71 s), document files/save/reopen (9.96 s), and diagnostics/GPU recovery
(1.96 s), using the existing production sRGB8 document factory. These elapsed test
times are correctness evidence only; the GPU suite overlapped some GTK checks.
The current four-fixture GPU executable is `view-color-final-tests`; the GTK
executable is `view-color-gtk-tests`. Successful Cargo JSON/build logs use
`view-color-final-build` and `view-color-gtk-build`; runtime artifacts use
`view-color-{sampling,files,recovery}`. Reproduce with the serial release GPU suite
and the corresponding filters via `bash tools/performance/gtk-raster.sh` as above.
Final GPU result and executable hashes are recorded in `view-color-provenance.json`.
Further benchmarking and optimization remain deferred until functional completion.

The added Float16 display check initially failed its fixed tolerance: green was
1.0048828125 instead of the Float64 reference 1.0057629312917291. The preceding
Float32 path passed. Vulkan's [floating-point format conversion rule](https://docs.vulkan.org/spec/latest/chapters/fundamentals.html#fundamentals-fp16)
does not require nearest rounding; the observed value is the lower adjacent half.
The main viewport now explicitly selects a nearest-even Float16 value before the
attachment conversion, including half subnormals and a finite half-range display
cap. This is confined to Float16 presentation and does not feed artwork, sampling
or export. The test's original 0.05% relative + 3e-6 bound remains unchanged.
`view-color-final-full-gpu.log` retains that failed check (172 pass/one failure).
The final implementation/build uses `view-color-rounded-{tests,build.json,build.log}`;
`view-color-rounded-focused.log` and `view-color-rounded-full-gpu.log` record its
checks. GTK runtime evidence above predates only this Float16-specific rounding
and the final test additions; the exercised GTK surface remains Rgba8Unorm.

Final view-color validation: **173 GPU tests pass, 26 remain ignored** (138.36 s);
all four focused fixtures pass (15.86 s). These are correctness run durations,
not frame-creation performance measurements.


## Shared native material color and faint-pigment transport (2026-09-14)

A shared working-color shader now supplies unassociation, Float32 interpolation
and Oklab conversion to material, scene and effect shaders. This deletes the
separate material Oklab implementation and effect-only interpolation helpers.
Document RGB converts to linear sRGB/D65 before the published Oklab equations and
back to document primaries afterward, including D50 adaptation for ProPhoto.
Signed cube roots remain; native output no longer clamps negative RGB. Perceptual
mix endpoints retain the selected value. Primary reference:
[Ottosson's Oklab definition and updated matrices](https://bottosson.github.io/posts/oklab/).

Normal composition and existing channel blend formulas keep their linear-document
RGB domain across both depths. Explicit Add/Subtract bounds remain artistic
operations; no global clamp is added. Native scene sampling now uses the same
four-load Float32 interpolation as image effects. Unassociation preserves all
positive coverage, including locked-alpha paint and wet reservoirs. Scalar uniform
coverage ratios no longer apply an epsilon floor in the native path. Region
comparison uses encoded document RGB weighted by alpha. Existing exposed sRGB8
factories retain their current arithmetic while common native adoption is pending.
The [rendering contract](../internals/rendering.md#native-sdr-working-color) records
these choices separately from this implementation history.

Native watercolor edge/transport work preserves extended RGB while constraining
coverage. A connected fixed-water-field test exposed an additional defect: the
kernel used `MIN_WETNESS` (2/255) to reject faint **pigment**, although water and
pigment have independent coverage. Native pigment presence now tests positive
coverage separately. The water activation threshold remains a material parameter.
The recorded pre-fix test fails faint-front scale consistency; the corrected
kernel passes that same 3e-6 relative scale bound. This is a correctness fix,
not a change to the water model or performance tuning.

Four connected native fixtures cover:

- **384 brush cases:** four spaces × two depths × eight brush blend modes ×
  coverage 0, 8e-8 and 1/65535 × unlocked/locked alpha. Real uniform-material brush
  dispatch is compared to Float64 source-over/channel-formula arithmetic, including
  negative and greater-than-one working RGB. Straight RGB tolerance is 2e-6.
- **24 wet/Oklab cases:** four spaces × two depths × mixing weights 0, 0.37 and 1,
  evaluated against Float64 primary conversion and Oklab equations. Straight RGB
  tolerance is 5e-6; the fixture includes negative/out-of-range working values.
- Sub-epsilon wet/watercolor deposition in all four spaces, plus actual transport
  on a constant extended-color interior. Nonzero pigment must survive; straight
  RGB meets 2e-6, with negative RGB and RGB greater than alpha retained.
- A real capillary front in all four spaces with identical water coverage but
  pigment coverage 0.5 versus 1/65535. Faint transported coverage scales within
  3e-6 relative error; its straight RGB meets 2e-6. The pre-fix failure is retained
  in `working-color-front-before.log` and its saved executable/build records.

Final material acceptance strengthens alpha comparison from the initial absolute
2e-7 check to **3e-7 relative**, requiring exact zero at zero alpha. The stricter
focused build changes only that assertion; implementation code is identical to
the full-suite build. The RGB and front-scaling thresholds remain unchanged.
These are Float32 in-progress brush fixtures, not a claim that sub-code pigment
survives an intentional native integer commit or every sensor/texture/selection
combination is qualified. Native backing/save/recovery tests run in the full suite.

A broader `cargo check --offline -p layer-render-wgpu --tests` also found two stale
integration-test calls to the raster validator left by document-color metadata
adoption. They now pass the fixture's declared sRGB8 document color. This closes a
previously missed test-build prerequisite; the project integration suite is run
explicitly below. GTK development checks and the release GTK test build pass.

Reproduce the GPU library suite with `cargo test --offline --release -p
layer-render-wgpu --lib -- --test-threads=1`, project integration with `--test
project -- --test-threads=1`, and the GTK file/recovery filters using the isolated
Mutter harness. `working-color-final-tests` is the full-suite executable;
`working-color-strict-tests` strengthens only the material alpha assertion.
Project and GTK executables are `working-color-project-tests` and
`working-color-gtk-tests`. Cargo JSON/build logs use `working-color-final-build`,
`working-color-strict-build` and `working-color-final-gtk-build`; final hashes and
results are in `working-color-provenance.json`.

Remaining functional work includes document-aware CPU brush dynamics, explicit
extended/default behavior for nonlinear tone controls, complete source/rasterize/
conversion/depth actions and GTK color/photo UI, bounded dense-photo operations,
managed wide-gamut/monitor/picker behavior and connected profiled interchange.
The transitional exposed sRGB8 path still needs coordinated replacement/cleanup.
All previously reported memory/latency failures remain open for the final phase;
no new benchmarks or performance optimization were performed here.

The final implementation passes **177 GPU library tests (26 ignored)**, all
**four project integration tests**, and the four material fixtures with the
stricter relative-alpha assertion. Saved logs are
`working-color-final-full-gpu.log` (156.57 s), `working-color-project.log`
(7.94 s), and `working-color-strict-focused.log` (24.21 s). GTK/source test build
checks pass (`working-color-gtk-check.log`, `working-color-final-check.log`).
Elapsed test durations are not performance evidence; some compilation overlapped
the full GPU correctness run. The stricter focused and project suites ran serially.

Real private-Mutter GTK document file checks pass (12.84 s), as do diagnostics
and forced GPU-failure recovery (1.96 s), on the current explicit sRGB surface.
Runtime logs/session records use `working-color-files` and `working-color-recovery`.


## Document-coordinate windows and bounded preview capture (2026-09-14)

The image-stage compositor now accepts a document-coordinate window. Input,
output, mask, clipping backdrop/composition and intermediate textures allocate
that window's dimensions. Capture conservatively expands the requested crop by
the sum of the visible spatial passes' declared support; document samplers retain
their full dependency. Groups, masks, clipping and pass execution still use the
ordinary compositor. Dirty regions and effect positions remain in document
coordinates, while copies/scissors and sampling translate into texture-local
coordinates. Current-pass and original-input sampling have separate origins.
Changing a window invalidates its image caches. Ordinary live composition resets
the window to the full document.

The first connected consumer is the filter picker. It scans four tiles per
asynchronous completion, with the corner-probe halo, then captures one shared
source crop for the requested rows. Each tile submission precedes reuse of its
scene uniforms, and only one four-tile chunk is in flight. Source metadata,
extent, paper or paint changes cancel an unfinished request after its callback;
a fresh request can then restart without consuming an old callback. The capture
projection excludes layers above the insertion scope, so an excluded global
filter cannot force a full-document input. Oversized declared neighborhood
support stops at the document extent instead of allocating a padded texture
larger than that input. Global input/intermediate allocation remains explicit
and still needs a scheduled, budgeted route.

The first Float32 crop oracle failed at 0.07194069 versus 0.07194312, beyond the
unchanged absolute 2e-6 tolerance. Reconstructing one-to-one pixel coordinates
from interpolated UVs changed the bilinear footprint with texture size. Native
image reads now use fragment position minus the draw origin directly when image
and draw dimensions agree. This fixes the discrepancy at the original tolerance;
no precision downgrade or relaxed acceptance bound was used. WGSL's fragment
position and interpolation definitions are the relevant primary contract:
[WGSL position builtin](https://www.w3.org/TR/WGSL/#position-builtin-value).

New connected correctness fixtures cover:

- Forty crop comparisons: four crops × unclipped/clipped nested groups × the
  exposed sRGB8 path and native integer16 sRGB/P3/Adobe RGB/ProPhoto. Two ordered
  spatial adjustments, translated polygon masks, non-unit opacity, non-page
  origins and partial document edges are exercised. Native premultiplied
  channels meet absolute 2e-6; exposed sRGB8 differs by at most one code. Returning
  the same scene to a full capture reproduces its prior pixels exactly. Cropped
  image-cache allocations are less than one twentieth of the corresponding full
  capture in these fixtures.
- A document-remapping stage after a neighborhood pass retains the complete
  input; its cropped output equals the corresponding full pixels exactly.
- A native ProPhoto 2049×513 source scans all 27 tiles across completions, finds
  the expected center, bounds retained source/image dimensions, cancels an edit
  after the first four tiles, and completes a fresh request. The excluded upper
  correction declares a global dependency. A separate three-pass 4096-pixel
  support declaration verifies that preview scratch stops at document dimensions.
- Existing preview insertion, empty-document sample and complete bundled-filter
  crop-versus-canvas comparisons remain in the focused preview suite.

These are correctness and GPU resource-size assertions, not peak-RSS/VRAM or
frame-latency qualification. The live full composite, live full image stages,
mutable raster residency, dense history capture, global scheduling, standalone
source-backed region preparation and mip selection are still incomplete. Region
capture currently uses the live renderer's prepared paint and mask pages. It does
not yet replace full-document preparation, export or exact inspection. GTK native
document activation and the remaining color/photo UI, managed display and profiled
interchange journeys also remain open. Benchmarking and optimization are deferred
to the final phase as requested; previously recorded breaches are not closed here.

Reproduction uses the serial release GPU library suite and the isolated GTK
Mutter harness. Initial precision failure artifacts are `image-windows-tests`,
`image-windows-build.{json,log}` and `image-windows-focused.log`; the corrected
two-test oracle passes in `image-windows-exact-focused.log` (15.55 s).
`bounded-previews-tests` passes all four non-benchmark preview tests (9.92 s).
`bounded-previews-verified-tests` passes the full library suite: **180 passed,
26 ignored**, 165.07 s, in `bounded-previews-full-gpu.log`. That full-suite build
precedes only the oversized-support clamp and insertion-scope pruning; those
final changes are checked with the focused preview/window suite and GTK runtime
below. Compilation overlapped parts of the GPU correctness run; these elapsed
test durations are not performance evidence.

The final executable `bounded-previews-scope-tests` passes the four preview tests
(8.76 s; one benchmark ignored) and both window oracles (11.71 s). Logs are
`bounded-previews-scope-focused.log` and `bounded-previews-scope-windows.log`.
Run them with `scene::previews::tests --test-threads=1` and
`image_windows --test-threads=1`, respectively. The final GTK executable is
`bounded-previews-scope-gtk-tests`; release Cargo JSON/build logs use
`bounded-previews-scope-build` and `bounded-previews-scope-gtk-build`.

Actual GTK `native_runtime_filter_packages` (6.77 s), `native_document_files`
(9.93 s), and `native_diagnostics_and_gpu_failure_recovery` (1.93 s) pass with
fatal GTK criticals under the isolated 1600×1000@120 Hz Mutter harness. Saved
logs/session records use `bounded-previews-filters`, `bounded-previews-files`, and
`bounded-previews-recovery`. Reproduce with `bash tools/performance/gtk-raster.sh
ABSOLUTE_TEST_BINARY TEST_FILTER REPORT_PREFIX`. The final runs were serial;
release compilation overlapped the focused preview run. Hashes, parent revision,
source hashes and build/result mappings are in `bounded-previews-provenance.json`.


## Standalone native snapshot capture and profiled row output (2026-09-14)

Document metadata preparation is now separate from allocating the live full
composite. The new `snapshot::SnapshotRenderer` owns an immutable project on a
worker, resolves its backing and creates native Float32 processing in the declared
document space/depth. Each request restores only the translated native paint,
mask and material pages required by the image window, including watercolor's
neighbor pages. Restoration shares the existing native color/scalar decoder and
updates its private resident index per successfully restored target, so a later
failure/cancellation cannot leave that index describing discarded pages. Packed
legacy RGBA8 project images become retained tiled sources for this consumer;
there is no second full immutable GPU upload. Native edit/history roots remain
unchanged.

Initial selection masks no longer require full selection output for this route.
The shared GPU crossing/fill shaders rasterize the requested rectangle. Packed
source selections copy only required source words/rows; coverage is neither
rasterized nor interpolated on the CPU. Fractional translation preserves the
existing zero-resampling coverage rule, and affine placements use the existing
GPU resampler with a conservative source footprint. Cropped buffers include the
source origin, and polygon edges outside the crop still establish its interior
parity. Old selection staging is released before the next completed snapshot
capture. Native mask textures remain R32Float.

Completed captures evict obsolete private paint/material/mask pages and image
windows before restoring replacements. This avoids retaining both completed
windows during the next allocation. The final focused suite includes repeated
and disjoint captures after this change; it does not establish measured peak
memory for a complete GTK export job.

PNG and TIFF output now have a connected headless document route: sixteen-row
Float32 composite strips feed `WorkingEncoder` and the profiled row writers.
Output transforms a copy; the surface/presentation format never participates.
The requested profile, channel model, precision and intent/BPC remain independent.
An explicit matte flattens in linear document RGB before output conversion.
A matching untouched source, with default conversion and no matte, bypasses
compositing and preserves every integer sample, including hidden RGB at zero
alpha. Gray builtin interpretations write a real gray ICC profile. Writer or
cancellation errors leave publication to the caller's existing temporary-file
protocol; this API does not publish files itself.

Each capture checks an explicit conservative pixel-dependency plan before
restoring pages. The default planning ceiling is 512 MiB, configurable by the
worker owner. It is **not** a measured peak-RSS/VRAM budget: retained compressed
sources, codec/output buffers, pipeline/driver resources and concurrent jobs need
separate host accounting. A global dependency can exceed the plan and fail before
restoration. Global scheduling, tighter dependency planning, named large-photo
limits and live composite/mutable residency remain outstanding. No resource-size
assertion here closes the final memory or latency gates.

Connected fixtures cover:

- Integer8/integer16 × all four builtin RGB spaces × PNG/TIFF identity delivery.
  The 257×256 source includes every integer16 code, repeat codes, zero alpha,
  one-code alpha and hidden RGB. Decoded output samples match exactly. Neither
  full composites nor materialized paint pages are allocated by identity output.
- GrayAlpha16 identity through both codecs with gray-profile validation, and an
  explicit same-profile matte that makes all output alpha opaque and matches
  the encoded matte at originally transparent pixels.
- Fifty-four cropped/full comparisons over sRGB8, Display P3 integer16 and
  ProPhoto integer16: nested groups, a spatial pass, non-unit opacity, translated
  source/paint/mask origins, committed mask edits, watercolor pigment/wetness and
  polygon/fractionally translated/affine-inverted packed selections. All channels
  meet absolute 2e-6. The affine cases also pass native project save/reopen before
  snapshot rendering. The final focused version enables mask-area viewing only
  in the snapshot input and still compares against untinted artwork.
- Eight edited composite deliveries: sRGB/P3 × integer8/integer16 × PNG/TIFF,
  compared with encoded full native-render pixels at at most one code per channel.
  Source raster publication identities remain unchanged; budget rejection occurs
  without a full composite, and a cancelled write emits no bytes.
- Legacy packed sRGB8 source conversion into a ProPhoto integer16 snapshot agrees
  with `WorkingDecoder` within 2e-6, with no full composite, legacy image GPU
  source or materialized paint pages.

The concurrent preview audit found that a view-only frame could cancel a scan
when paper opacity was below one: it compared the caller's paper alpha with the
already multiplied compositor alpha. Cancellation now compares the original
requested view. The chunked preview test includes partially transparent paper
and a view-only frame before continuing all 27 tiles; real source edits still
cancel after the in-flight chunk.

This is a **headless worker API**, not completed GTK export integration. GTK file
jobs/recipes/progress/cancellation still need wiring, together with JPEG output,
the remaining RGB/gray/CMYK metadata/interchange matrix and external-editor
handoff. Live native GTK activation, source/color/photo UI, histogram/numeric
color journeys, managed display/picker/monitor behavior, brush dynamics and
remaining nonlinear precision also remain open. Benchmarking and optimization
remain last, as requested; no new frame-performance measurements were run.

Validation: the release GPU library suite passes **185 tests, with 26 benchmarks
ignored**, in 184.32 s (`snapshot-full-gpu.log`, `snapshot-verified-tests`). That
build precedes only private snapshot cache eviction and the mask-area exclusion
assertion; the final `snapshot-residency-tests` passes all **five snapshot tests**
in 26.89 s (`snapshot-residency-focused.log`). Their build records are
`snapshot-verified-build.{json,log}` and `snapshot-residency-build.{json,log}`.
Reproduce with `cargo test --offline --release -p layer-render-wgpu --lib --no-run
--message-format=json`, retain the executable from its successful compiler
artifact, and run it serially with `--test-threads=1` or
`snapshot::tests --test-threads=1`. These test durations are correctness-run
elapsed times, not frame-performance measurements.

The release GTK executable `snapshot-gtk-tests` passes actual
`native_runtime_filter_packages` (6.79 s), `native_document_files` (9.94 s), and
`native_diagnostics_and_gpu_failure_recovery` (1.93 s). Runs use fatal GTK
criticals and the isolated 1600×1000@120 Hz Mutter harness, serially, with the
existing drawing path. Its build includes all shared changes but predates the
private snapshot cache eviction; GTK does not yet invoke that API. Build records
are `snapshot-gtk-build.{json,log}`; logs and session records use
`snapshot-filters`, `snapshot-files`, and `snapshot-recovery`. Reproduce with
`bash tools/performance/gtk-raster.sh ABSOLUTE_TEST_BINARY TEST_FILTER
REPORT_PREFIX`. The final `cargo check --offline -p layer-linux` and
`git diff --check` pass. `snapshot-provenance.json` records the parent revision,
source/binary/result hashes and successful Cargo build mappings under
`artifacts/color-m2/`.

## Document-aware CPU brush color dynamics (2026-09-14)

The shared engine now configures live and transient reconstruction generators
with the document RGB space. Cursor state reused by a host across documents is
reset on an interpretation change. Generator reset, cloning, early/late sensor
corrections and renderer replacement retain the space; integer depth never changes
resolved dab colors. Standalone generation takes an explicit `RgbSpace`.

Primary/secondary interpolation stays in straight linear document coordinates,
with exact endpoints and independent coverage alpha. Both RGB colors accept
finite extended values; alpha validation remains [0,1]. Hue/saturation/lightness
uses the selected space's transfer-encoded coordinates and returns linear
document RGB. The superseded local sRGB-only transfer helpers are deleted.
Disabled dynamics bypass the nonlinear round trip and preserve the input bits.
Color-coordinate evaluation uses Float64 intermediates and emits Float32 dabs.

The [CSS Color 4 HSL algorithms](https://www.w3.org/TR/2026/CRD-css-color-4-20260913/#hsl-to-rgb)
and [standard transfer definitions](https://www.w3.org/TR/2026/CRD-css-color-4-20260913/#color-conversion-code)
were checked against current code. CSS HSL is defined in sRGB; using cylindrical
coordinates in each encoded document space is an explicit application policy,
not a claim that CSS defines wide-gamut HSL. Extended RGB uses the smallest common
channel interval containing the input and [0,1], evaluates HSL in that interval,
then restores its range. Hue rotation preserves negative/above-one components;
saturation/lightness adjustments remain bounded within that interval. This
avoids an implicit sRGB conversion or a hidden clamp to document [0,1].

Numerical acceptance includes five independently evaluated Python `colorsys`
vectors over all four spaces, both stamp and cursor emission, within absolute
2e-6 linear RGB and one integer16 code. Every integer16 code over all four spaces
passes bit-exact disabled dynamics, with zero/tiny alpha and hidden negative RGB.
Half-turn hue rotation preserves extended and very dark chroma within relative
2e-6 (with a 1e-20 floor on reference magnitude). The first dark test exposed
cancellation in `1 - abs(2*L - 1)`; the equivalent `2 * min(L, 1-L)` removes it.
The initial failure remains in `brush-space-core-engine.log`.

Connected engine fixtures cover four spaces × two depths × renderer replacement
on/off × active/completed correction, plus reused cursor state and the next
contact after undo. Corrected dabs match explicit generation exactly; both depths
emit identical dabs. The first fixture revision mistakenly compared the mock
renderer’s accumulated submission log across an undo with one new stroke; its
log is retained in `brush-space-corrected-core-engine.log`. Resetting only the
test recorder before the new contact corrects that oracle, without changing
production undo. The native GPU paint/undo/save/reopen/replacement fixture now
uses primary/secondary mixing and per-stamp/per-stroke color jitter.

This completes the document-aware CPU emitter prerequisite. Profiled preset and
swatch ownership in the GTK controls, remaining nonlinear effect precision,
live residency/mips/global scheduling, managed viewing, source/photo journeys
and full interchange remain open. This work does not activate native GTK modes
or qualify performance; benchmarking and optimization remain last.

Final correctness checks pass: **57 core tests and 55 engine tests**
(`brush-space-clean-core-engine.log`); **185 GPU library tests, 26 benchmarks
ignored**, 182.47 s (`brush-space-full-gpu.log`); and **four project integration
tests**, 8.47 s (`brush-space-project-hardware.log`). The initial project attempt
ran inside the device sandbox and failed adapter creation; its separate
`brush-space-project.log` is retained. The accepted run uses the physical GPU.
The GTK development check also passes (`brush-space-gtk-check.log`).

Rebuild with `cargo test --offline --release -p layer-render-wgpu -p layer-linux
--no-run --message-format=json`; the successful Cargo artifact record is
`brush-space-verified-build.{json,log}`. Retained executables are
`brush-space-gpu-tests`, `brush-space-project-tests`, and `brush-space-gtk-tests`.
Run the first two serially with `--test-threads=1` and physical GPU access.
The broader build found one stale `RasterData::validate` call in the
`raster_workloads` example; it now passes the actual document interpretation.
No example workload or benchmark was executed.

Actual GTK `native_runtime_filter_packages` (6.91 s), `native_document_files`
(9.93 s), and `native_diagnostics_and_gpu_failure_recovery` (1.97 s) pass
serially with fatal criticals under the private 1600×1000@120 Hz Mutter harness.
Reproduce using `bash tools/performance/gtk-raster.sh ABSOLUTE_TEST_BINARY
TEST_FILTER REPORT_PREFIX`; log/session prefixes are `brush-space-filters`,
`brush-space-files`, and `brush-space-recovery`. These elapsed test durations
are correctness records, not latency qualification.

`native_color_panel_input` also passes (30.02 s) with its required native input
driver. It exercises the existing compact panel at five sizes, three shapes and
both themes on the private display; this does not qualify a wide-gamut picker.
Reproduce with `CARGO_NET_OFFLINE=true LAYER_TEST_ARTIFACTS=ABSOLUTE_OUTPUT_DIR
bash tools/performance/workspace-motion.sh gtk --color-panel`. The accepted
session is `brush-space-color-input-session.log`, with generated artifacts in
`brush-space-color-input/`. The initial `brush-space-color-panel.log` records a
missing input-protocol environment variable because the generic raster harness
was used; it did not enter the test workflow. The proper driver’s Cargo executable
matches the retained GTK test executable. `brush-space-provenance.json` records
source, binary and log hashes, the parent/committed revisions and successful
build mapping. Final `git diff --check` passes.


## Analytic filter controls and native SDR tone semantics (2026-09-14)

User clarification: edited filters require perceptually equivalent results, not
bit-for-bit parity with earlier implementations. Small arithmetic differences
alone do not justify further accuracy work. Exact committed samples, unchanged
integer16 identity, save/undo and profile persistence remain separate guarantees.
This supersedes strict historical pixel-parity expectations for edited filters.

The old curve/gradient parameter representation resampled up to 32 authored
controls into 256 values. Current code now prepares Hermite segment coefficients
or exact gradient stops; the GPU evaluates them directly after a bounded binary
search. Curve tangents retain the existing secant endpoint/weighted harmonic
interior policy, using interval-scaled coefficients to avoid storing enormous
slopes. Gradient RGBA interpolation and narrowly separated stops no longer pass
through an intermediate sampled table. Parameter records are 65 vec4 values per
curve/gradient. Shader helper offsets, generic preparation dependencies, bundled
programs and the Tent Blur example all use **ABI 3**. The sampled ABI is deleted;
ABI 2 packages and embedded programs are rejected, without an adapter.

Native identity curves and neutral Levels, hue/saturation, Color Balance,
Brightness/Contrast and Vibrance bypass nonlinear round trips. A zero-strength
Gradient Map is also neutral. Curves continue linearly beyond their endpoint
controls. Gradient Map retains its explicit endpoint-color mapping. Levels now
has separate **Clamp input** and **Clamp output** controls, both initially off.
Unclamped levels apply signed gamma between declared input/output anchors;
input clipping precedes gamma and output clipping follows the output mapping.
These controls are supported by the existing shared schema and GTK properties.
The distinction is consistent with the independently checked
[GIMP Levels API](https://developer.gimp.org/api/3.0/libgimp/method.Drawable.levels.html);
our defaults and signed continuation are explicit application policy.

Hue/saturation and vibrance use encoded document coordinates, with the same
extended HSL interval policy as brush dynamics. Color Balance preserves weighted
encoded-domain luminosity without a hidden native RGB clamp. These are artistic
tone operations, distinct from scene-linear exposure and linear luminance.

Validation exercises twenty-four neutral controls over all four spaces and both
depths, fused and physical passes, at zero, tiny, one-code, partial and opaque
alpha; neutral output matches the unfiltered Float32 result exactly. Levels
covers all input/output-clipping combinations, gamma 1/1.7 and all four transfer
curves against the scalar reference. Extended hue rotation, desaturation and
color balance have independent expected values. Native project round trips now
include non-default Levels, Curves and Color Balance together with the existing
exposure, white balance, hue correction and masks.

The direct table tests evaluate every integer16 input code against Float64
references, including narrow knots and a 32-control alternating curve. Acceptance
allows one integer16 code and absolute 1/65535 in the normalized result. The
initial test's 2e-6 threshold failed at 2.17e-6 on a steep curve because of shader
input arithmetic; that is less than one code and does not warrant a precision
fix under the user's clarified criterion. The original failure remains in
`tone-controls-focused.log`. Profiled Curves additionally exercises the actual
encoded/linear boundaries at one-code alpha in every working space and both
pass types. These checks establish practical numerical bounds; they do not
claim exact historical edited pixels or calibrated-monitor qualification.

Native GTK activation, photo/import/export jobs, numeric colors and profiled
swatches, histogram/clipping inspection, managed display/picker behavior, live
residency/mips/global scheduling and the remaining interchange matrix are still
open. Benchmarking and optimization remain deferred to final qualification.

Validation results and reproduction:

- `cargo test --offline --release -p layer-core -p layer-ui` passes **58 core
  and 370 UI tests** (`tone-controls-final-core-ui.log`). The production GPU/GTK
  build is `cargo test --offline --release -p layer-render-wgpu -p layer-linux
  --no-run --message-format=json`, recorded in `tone-controls-verified-build`.
- The saved `tone-controls-verified-tests --test-threads=1` run passes **189 GPU
  tests**, including all five new native tone tests, and leaves 26 performance
  tests ignored. Its one failure was an obsolete eight-bit CPU oracle reading
  analytic parameter records as the removed 256-sample table. The shader's
  identity output was correct. The oracle now uses the same independent Float64
  reference as the native test, reading authored controls instead of GPU records.
  The final build and focused `pointwise_tone_filters_match_scalar_color_oracles
  --test-threads=1` pass are `tone-controls-oracle-build.{json,log}` and
  `tone-controls-oracle.log` (3.29 s). Production code did not change between
  these runs. Together these cover all **190 nonignored GPU tests**; this is
  not represented as a second full-suite run.
- The saved project integration executable passes **4 tests**
  (`tone-controls-project.log`, 9.31 s). GTK `native_runtime_filter_packages`,
  `native_adjustment_panels_review`, `native_document_files`, and
  `native_diagnostics_and_gpu_failure_recovery` pass, using
  `bash tools/performance/gtk-raster.sh ABSOLUTE_TEST_BINARY TEST_FILTER
  ABSOLUTE_REPORT_PREFIX` with physical GPU access. The log prefixes are
  `tone-controls-filters`, `tone-controls-panels`, `tone-controls-files` and
  `tone-controls-recovery`. This preserves saving and recovery coverage.
- The final GTK test additionally checks both Levels clipping switches, their
  off defaults, on/off publication to shared parameters, and the scrolled view.
  It passes in 32.24 s (`tone-controls-clipping-panels-session.log`, binary
  `tone-controls-clipping-gtk-tests`). All forty filters are exercised by the
  native panel fixture. Curves, Levels, hue/saturation, Color Balance and the
  clipping controls were visually inspected; no obvious artifacts were seen
  in these sRGB fixture captures. This is scoped UI review, not a calibrated
  wide-gamut or exhaustive perceptual comparison.

Retained captures are in `artifacts/color-m2/tone-controls-ui/`.
`tone-controls-provenance.json` records source, binary, log and capture hashes
and the parent/committed revisions. Some correctness jobs overlapped; none of
these elapsed times are latency measurements. `git diff --check` passes.


## GTK profiled snapshot export (2026-09-14)

GTK **Export…** now opens color/output choices before the file picker. The
shared recipe model provides Web / Share (sRGB8 PNG), Wide-color image (P3 8-bit
PNG), Further editing (document-space integer16 TIFF), and custom choices.
PNG/TIFF, all four built-in RGB profiles, both integer depths, and preserved
transparency or white/black mattes are connected. Choosing a different space
converts the rendered copy and embeds matching metadata. The sheet identifies
8-bit reduction and recommends integer16 for ProPhoto. A file extension that
disagrees with the chosen format is rejected, as is the editable master's own
location. The complete export journey remains unfinished: JPEG, dimensions,
output previews/comparison, dithering, custom ICC/intent/BPC UI and remembered
named recipes are still required.

The GTK file path no longer requests a full live RGBA8 readback. Shared export
capture freezes the immutable master, paper/view background and last successfully
submitted animation time without reserving a save checkpoint. A GTK file worker
owns `SnapshotRenderer`, prepares bounded dependencies and streams the profiled
rows. The old readback helper now belongs to the native lifecycle tests; those
checks deliberately still compare the actual interactive renderer's pixels.
This does not activate the native GTK painting/source-document mode.

Cancellation control is shared before worker initialization and checked during
source preparation, capture and row output. Completed-row progress distinguishes
preparation, writing and file finalization. The seekable atomic writer supports
TIFF without staging a second complete output in memory. It calls the job's
publication decision after flushing/syncing the temporary file and before rename.
Accepted cancellation and publication use one lock: an accepted cancellation
preserves the original destination; once publication begins, completion wins.
Failed or cancelled output does not acknowledge the master as saved.

Correctness validation:

- **59 core and 55 engine tests** pass (`export-worker-core-ui.log`), including
  seekable output, rejection at the publication boundary, original-file
  preservation and the frozen/reset animation clock. **371 UI tests** pass in
  `export-worker-final-ui.log`; the new export test confirms unchanged master,
  checkpoint, dirty state and destination through completion and cancellation.
  The initial new test omitted its GTK platform selection; the corrected focused
  run is also retained in `export-worker-ui-snapshot.log`.
- **6 snapshot GPU tests** pass (`export-worker-snapshot.log`, 25.95 s), covering
  exact untouched source codes/hidden RGB, profiled composite output, masks,
  selections, legacy image conversion, explicit mattes, planning limits,
  cancellation before device initialization and completed-row progress.
- GTK `native_document_files` passes (`export-worker-files-session.log`, 18.25 s).
  It drives the real sheet and chooser, exports PNG matching the existing
  interactive fixture exactly, and reopens a **16-bit ProPhoto TIFF** with the
  expected ICC bytes and dimensions. Save/reopen and renderer replacement remain
  covered. The two file-policy tests pass in `export-worker-file-policy.log`.
- GTK `native_diagnostics_and_gpu_failure_recovery` passes
  (`export-worker-recovery-session.log`, 1.98 s), preserving drawing and manual
  save/recovery through device failure and replacement.

The primary release artifact mapping is `export-worker-final-build.{json,log}`;
retained executables are `export-worker-gpu-tests` and `export-worker-gtk-tests`.
Reproduce GPU tests with `snapshot::tests:: --test-threads=1`, file policy with
`files:: --test-threads=1`, and GTK tests with the existing
`bash tools/performance/gtk-raster.sh ABSOLUTE_BINARY TEST_FILTER REPORT_PREFIX`
harness. Physical GPU access is required for the native/snapshot checks. Some
correctness jobs overlapped; their durations are not performance evidence.

The initial sheet review found truncated selected values. The alert dialog
controls its own width; the final sheet uses native ComboRow subtitle values
and the alert's wide button-layout preference so selections remain readable.
This follows the installed libadwaita 1.9 introspection/API definitions. Final
layout validation, capture and binary hashes are recorded in
`export-worker-provenance.json`.
The existing 512 MiB snapshot planning ceiling still excludes retained source,
codec, pipeline and driver memory. Live residency, complete photo workflows,
managed viewing, and measured memory/latency qualification remain open; no
benchmarking or optimization was performed for this stage.

The final snapshot implementation also checks cancellation between legacy asset
rows during setup. Rebuilt `export-worker-reviewed-gpu-tests` passes all six
snapshot tests again (24.90 s, `export-worker-reviewed-snapshot.log`), and the
matching GTK file workflow passes (18.14 s). These executables are mapped by
`export-worker-reviewed-build.{json,log}`. The final subtitle-layout-only GTK
build is `export-worker-sheet-build.{json,log}` and its saved binary is
`export-worker-sheet-gtk-tests`; no additional snapshot/GPU changes followed.

Final GTK acceptance uses `export-worker-values-build.{json,log}` and retained
`export-worker-values-gtk-tests`. `native_document_files` passes in **15.34 s**
(`export-worker-values-files-session.log`), now asserting all five initial
selected labels as well as the format/depth preset transitions and PNG/TIFF
outputs. The ComboRows explicitly define their string expression and subtitle
mode before installing the model; the earlier initial-label failure remains in
`export-worker-visible-files-session.log`. Final sRGB and ProPhoto choices were
visually reviewed and retained in `export-worker-ui/`, together with the produced
sRGB8 PNG and ProPhoto16 TIFF. Source/binary/log hashes and revision mapping are
in `export-worker-provenance.json`. Final `git diff --check` passes.


## Streaming JPEG and GTK delivery (2026-09-14)

JPEG import now decodes scanlines directly into retained source tiles. The old
full compressed-file/full decoded-image `zune-jpeg` path and its direct
dependency are deleted. A small C shim uses **libjpeg-turbo 3.1.3**, the installed
Fedora library. Error recovery stays inside C; Rust I/O callbacks return errors
or caught panics before C raises a codec error. Failed contexts cannot be reused.
Two fixed 64 KiB I/O buffers (one per encoder/decoder), a scanline and bounded
codec state replace full decoded staging. The existing TIFF dependency still
uses its own `zune-jpeg` version; it was not removed.

Metadata preflight reads through all JPEG scans without retaining the compressed
file. ICC chunk sequence/count/size, EXIF orientation, end-of-image and a maximum
256 scans are checked before choosing the source interpretation. Untagged
RGB/gray records its sRGB assumption; invalid ICC does not become untagged.
Adobe CMYK and YCCK decode to conventional ink channels before the CMM; ambiguous
CMYK polarity requires an explicit interpretation. CMYK without a profile is
rejected. HDR gain-map namespaces and multiple-picture JPEG require an explicit
rendition/image choice, rather than silently becoming plain SDR. All eight EXIF
orientations retain their existing exact sample permutation.

The implementation follows the installed [libjpeg-turbo API manual](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/main/doc/libjpeg.txt)
for scanline I/O, source/destination managers, fatal-error destruction and
coefficient-buffer behavior. [ICC embedding guidance](https://www.color.org/technotes/ICC-Technote-ProfileEmbedding.pdf)
defines the APP2 chunk protocol. Independent handoff uses
[Pillow's JPEG encoder](https://github.com/python-pillow/Pillow/blob/main/src/libImaging/JpegEncode.c)
for direct Adobe CMYK and [ImageMagick's JPEG codec](https://github.com/ImageMagick/ImageMagick/blob/main/coders/jpeg.c)
for YCCK and reference ink decoding. Both use libjpeg-turbo internally, so this
is independent wrapper/profile/polarity validation, not an independent DCT
algorithm comparison. Pillow 12.3.0 bundles libjpeg-turbo 3.1.4.1; ImageMagick is
7.1.2-27 Q16-HDRI on this machine. Fixture/profile hashes are retained.

JPEG output streams opaque 8-bit RGB/gray/CMYK, embeds the actual profile, uses
full chroma resolution at all qualities, and never requests further rows after
provider failure. It performs no progressive/optimized encoding that would need
a complete coefficient image. Source/native master data is unchanged by lossy
export. GTK adds JPEG to the export recipe, quality 1–100 (default 90), explicit
white/black backgrounds and an 8-bit explanation. Selecting JPEG makes its depth
constraint visible; keeping transparency disables continuation. The recipe is
also validated before file selection and worker setup. `.jpg` and `.jpeg` are
accepted. Existing snapshot cancellation and atomic publication are reused.

Correctness evidence:

- The complete color suite covers **33 tests including the supplied external
  fixtures**. `jpeg-external-core.log` has 32 passes and one setup failure caused
  by the missing `LAYER_TEST_CMYK_PROFILE` variable. With that variable supplied,
  the remaining CMM test passes in `jpeg-cmyk-cmm.log`. RGB/gray JPEG at both
  qualities preserves profile bytes, stays within declared lossy tolerances, and
  saves/reopens the decoded source exactly. Error, panic, malformed/truncated
  stream and cancelled provider cases return failures without false success.
- `jpeg-fixtures/` contains direct CMYK, YCCK, progressive RGB 4:2:0, progressive
  gray and EXIF-rotated fixtures. Decoded ink/RGB/gray samples match the reference
  within one 8-bit code. Independent Pillow reopening of our CMYK exports
  preserves ICC and polarity with maximum ink-code differences **0 and 1**
  (`jpeg-handoff.log`, `jpeg-fixtures/handoff.json`). This validates code/profile
  handoff; it is not a calibrated print or external-editor UI comparison.
- An **8192×7324 baseline JPEG** decodes under an **8 MiB codec planning limit**.
  A 60 MP progressive 4:4:4 fixture is rejected under the default 128 MiB limit
  before coefficient allocation and succeeds with an explicit **384 MiB** limit.
  This is structural/correctness evidence, not measured peak RSS or a qualified
  shipping budget. Progressive coefficients need document-scale storage; the
  default policy for large progressive photos remains open for final measured
  qualification. The source tile budget (default 512 MiB), orientation work,
  CMM, process and driver allocations are additional.
- The GPU JPEG snapshot test passes (`jpeg-snapshot.log`, 3.89 s). Explicit
  linear matte and conversion into each of the four RGB spaces match PNG output
  within four 8-bit codes at JPEG quality 100; clipping statistics agree.
- Real GTK `native_document_files` passes (`jpeg-files-session.log`, 20.17 s).
  It drives quality/background/depth constraints, writes and reopens P3 JPEG,
  and continues checking PNG, 16-bit ProPhoto TIFF, save/reopen and renderer
  replacement. The JPEG sheet was visually reviewed. Diagnostics and device-loss
  recovery pass (`jpeg-recovery-session.log`, 1.97 s), including the intentional
  GPU validation failure. Portal/GVFS teardown warnings are harness noise.

Reproduce the external fixtures with locally supplied profiles:

```sh
# Install Pillow in a local venv or an ignored target directory first.
python3 tools/validation/jpeg_interchange.py prepare artifacts/color-m2/jpeg-fixtures \
  --cmyk-profile /usr/share/color/icc/krita/cmyk.icm \
  --rgb-profile /usr/share/color/icc/krita/sRGB-elle-V2-srgbtrc.icc
LAYER_TEST_JPEG_FIXTURES="$PWD/artifacts/color-m2/jpeg-fixtures" \
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  cargo test --offline --release -p layer-color -- --include-ignored --test-threads=1
python3 tools/validation/jpeg_interchange.py verify artifacts/color-m2/jpeg-fixtures
```

This session used `PYTHONPATH="$PWD/artifacts/deps/jpeg-python"` for Pillow.
The system development package was unavailable to install without interactive
administrator credentials. Its matching **3.1.3-1.fc44.x86_64** RPM was downloaded
with `dnf download`, extracted locally using `rpm2cpio`/`cpio`, and its ignored
`libjpeg.pc` prefix made relative to the extracted directory. The local linker
symlink resolves to the already installed `/lib64/libjpeg.so.62`. Cargo used
`PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig"`. Standard builds
use installed development headers via pkg-config; no local path is compiled into
the build script. The [Linux setup guide](../development/linux.md) records this
new system dependency. Non-GTK native/Web packaging and codec dependency
integration remain unqualified and require the user's later platform approval.

JPEG completes another functional export format, not milestone 2. Remaining
interchange work includes metadata/DPI retention, resize/dither/preview, custom
ICC/intents/BPC controls, remembered recipes and richer clipboard/source repair.
Native GTK photo activation, bounded live residency/mips, complete color tools,
managed viewing and final memory/latency gates remain outstanding. No performance
workload or optimization was run in this stage; correctness job durations above
are not latency evidence.


Final source builds are mapped by `jpeg-reviewed-build.{json,log}` and saved as
`jpeg-reviewed-{color,ui,gpu,gtk}-tests`. The final color executable passes **all
33 tests**, including external fixtures, in 1.58 s (`jpeg-reviewed-color.log`).
The shared UI suite passes **371 tests** in 0.71 s (`jpeg-reviewed-ui.log`), and
**all 7 snapshot GPU tests** pass in 27.47 s (`jpeg-reviewed-snapshot.log`).
The two file publication/cancellation policy tests pass in `jpeg-file-policy.log`.
Source, binary, dependency, fixture and capture hashes are retained in
`jpeg-provenance.json`; JPEG/PNG/TIFF sheet captures are in `jpeg-ui/`.
The final GTK file workflow also passes in 17.34 s
(`jpeg-reviewed-files-session.log`). Final `git diff --check` passes.


## Export conversion controls and 8-bit dithering (2026-09-14)

Shared output encoding now keeps ICC conversion policy and precision reduction
separate. The snapshot PNG/TIFF/JPEG paths consume the same encoding choices;
GTK no longer supplies hardcoded conversion defaults to its export worker.
**Advanced color** exposes the four ICC intents, black point compensation and
optional **Reduce banding**. Relative colorimetric plus BPC remains the default.
BPC is unavailable under absolute colorimetric intent; dithering is available for
8-bit delivery only. Preset selection resets these choices, while manual changes
select Custom. The scrollable sheet keeps advanced options and action buttons
reachable without increasing the initial set of color decisions.

Dithering performs deterministic stochastic rounding **after destination color
conversion**, in encoded integer8 code units. Each channel chooses one of its
two neighboring codes according to its fractional part. RGB channels share a
pixel threshold so neutral inputs remain neutral; alpha uses ordinary rounding.
The threshold derives from output coordinates, with no frame, chunk, RNG-thread
or export-call state. Black/white endpoints remain exact, clipping statistics
are computed before dithering, and 16-bit dither requests are rejected before
output. The existing direct source route still preserves exact unchanged codes,
including hidden RGB, even when 8-bit dithering is selected. Quantization is an
output-copy choice; the editable master and its depth are unchanged.

The coordinate mixer adapts the [public-domain SplitMix64 reference](https://prng.di.unimi.it/splitmix64.c)
with attribution in source and the repository notices. Correctness checks define
fractional-code mean error below **0.004 code** over 65,536 samples, less than one
code error in the mathematical quantizer, and at most **1.0001 code** through the
Float32 working/transfer round trip. These are quantization-specific tolerances,
not new exact-parity requirements for edited filters.

Validation:

- **35 color, 59 core and 371 shared UI tests** pass in
  `output-encoding-core-ui.log`, including supplied JPEG/CMYK fixtures. Existing
  exact integer16 identity/hidden-color and low-alpha tests remain intact. New
  checks cover fractional means, endpoints, all four RGB spaces, neutral RGB,
  unchanged alpha, repeated rows, chunk boundaries and invalid 16-bit requests.
- **8 snapshot GPU tests** pass in 27.41 s (`output-encoding-snapshot.log`). The
  new ProPhoto16-to-8 case spans source tiles and output strips, compares PNG
  and TIFF, repeats PNG byte-for-byte, checks alpha against undithered output,
  and retains exact same-depth source delivery. Other snapshot tests still cover
  mattes, profiles, legacy images, masked materials, cancellation and limits.
- GTK `native_document_files` passes in 20.63 s
  (`output-encoding-files-session.log`). It changes intent/BPC/dither in the
  real export sheet, verifies applicability, exports JPEG/PNG/TIFF, saves/reopens
  the native project and replaces the renderer. Initial visual inspection found
  unnecessary clipping of choices in the constrained scroll area; the reviewed
  sheet allows more natural height and uses a shorter dither explanation.

The primary release mapping is `output-encoding-build.{json,log}` and the final
layout/test cleanup mapping is `output-encoding-reviewed-build.{json,log}`.
Source, binary, log and capture hashes are in `output-encoding-provenance.json`.
Use the same libjpeg pkg-config setup and GTK harness recorded in the JPEG stage.
No performance measurements or optimizations were performed; correctness jobs
may overlap and their durations are not latency evidence.

Export still needs resized output, output previews/comparison, custom ICC/profile
channel choices, remembered named recipes and photo metadata/DPI retention.
Complete native GTK photo editing, managed viewing, live residency/mips and the
final measured budgets remain required before milestone 2 is qualified.

The reviewed GPU dither test passes in 3.10 s
(`output-encoding-reviewed-dither.log`), and the revised GTK workflow passes in
20.70 s. Final screenshot timing waits for the native expander animation to
settle; `output-encoding-layout-build.{json,log}` maps that test-only build.
Its GTK workflow passes in **18.23 s** (`output-encoding-layout-files-session.log`).
The final expanded and collapsed sheets were visually reviewed and retained in
`output-encoding-ui/`. Final `git diff --check` passes.


## GTK custom ICC and scoped grayscale/CMYK delivery (2026-09-14)

The export recipe now carries an explicit profile definition, profile channel
model, descriptive name and independent integer depth. The RGB-only
`DocumentColor` output assumption is removed. ICC channels have one shared core
type, reused by the CMM and recipe model. Original profile bytes, not filenames
or display labels, define the output and survive recipe serialization.

GTK **Color space → Custom ICC → Choose…** accepts local RGB, grayscale and CMYK
profiles. Reading is bounded at 16 MiB and runs on a file worker. Header/source
validation and an actual Float32 output transform with a finite sample probe
must succeed before adoption; an input profile is not automatically treated as
a usable delivery profile. The selected bytes are retained independently of
subsequent file changes. Cancelling or failing another selection retains the
previous one. With no valid selection, export remains unavailable; malformed
metadata is not silently replaced by sRGB.

The file's color model follows the selected profile. RGB/gray can preserve alpha;
CMYK requires an explicit matte and TIFF or JPEG. Choosing a CMYK profile from
PNG selects TIFF and a white background visibly. Subsequent incompatible manual
choices disable continuation with a short explanation, and the shared recipe
validates them again before worker setup. Bit depth and the existing advanced
conversion options remain independent. No proof profile is selected implicitly,
and this does not introduce native CMYK painting or print-proof simulation.

Validation:

- **59 core, 372 UI and 32 non-external color tests** pass in
  `custom-profile-core-ui.log`; the three external color fixtures remain covered
  by the preceding stages. The final explicit recipe cases pass in
  `custom-profile-recipe.log`, covering RGB/gray/CMYK channel mapping,
  transparency/depth/format constraints and retained payload serialization.
- The worker loader test passes (`custom-profile-loader.log`), preserving actual
  RGB and gray ICC bytes and rejecting corrupt and oversized files.
  Production GTK `cargo check --offline --release -p layer-linux` passes in
  `custom-profile-production-check.log`. `layer-color` is now a regular GTK
  dependency instead of a test-only dependency; no new native library is added
  beyond the existing color/snapshot dependencies.
- Real GTK `native_document_files` passes in **34.20 s**, then the final
  frozen-profile/cancellation/handoff fixture passes in **33.31 s**
  (`custom-profile-reviewed-files-session.log`). It tests cancellation before
  and after a valid selection, malformed ICC, disabled incompatible choices,
  and changing the selected RGB profile file before exporting. The resulting
  file still embeds the previously selected bytes. The workflow exports
  **16-bit Adobe RGB ICC TIFF with alpha, 16-bit profiled grayscale PNG with
  alpha, and 16-bit CMYK TIFF with an explicit white matte**, in addition to the
  existing PNG, ProPhoto TIFF and P3 JPEG cases. Native save/reopen and renderer
  replacement remain covered. The RGB/gray/CMYK option sheets were reviewed.
- Independent ImageMagick 7.1.2-27 decoding exactly matches **983,040 integer16
  samples** across the three custom outputs; ICC payloads match exactly
  (`custom-profile-handoff.log`). It uses ImageMagick/libtiff and PNG decoding
  without a requested color conversion and compares little-endian raw channels
  plus the extracted ICC bytes. This is numerical file handoff, not an
  external-editor UI session or calibrated print comparison. The GTK master in
  this fixture still uses the existing sRGB8 interactive renderer; these outputs
  do not qualify native 16-bit GTK editing.

Build mappings are `custom-profile-build.{json,log}` and the final
`custom-profile-reviewed-build.{json,log}`. Saved executables are
`custom-profile-{ui,gtk}-tests` and `custom-profile-reviewed-gtk-tests`.
`custom-profile-provenance.json` records source/binary/log/fixture/capture hashes.
Reproduce with the prior libjpeg pkg-config environment and:

```sh
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_document_files ABSOLUTE_REPORT_PREFIX
python3 tools/validation/icc_export_handoff.py \
  artifacts/familiar-workspace/files GTK_TEST_PROCESS_ID
```

The CMYK native branch requires that environment variable; omitting it exercises
RGB/gray and the existing formats only. The native fixture retains `.raw` and
`.icc` siblings of its custom outputs for independent checking. This run used
CMYK profile SHA-256
`156e7c14f244cfc4ed83a755ca4803d80e15dd249b40fae82cb127d3902e15c7`.
Final captures and outputs are retained in `custom-profile-ui/`.

Output resizing, previews/comparison, reusable named recipes and photo metadata/
DPI retention remain unfinished. Native GTK photo editing, managed viewing,
bounded live residency/mips and final measured memory/latency gates also remain
open. Other hosts have not been integrated or qualified. No benchmarking or
performance optimization was performed during this stage.

## Native live physical-filter windows (2026-09-14)

Parent: `4e14a2e5`. Current-code inspection confirmed that snapshot captures had
bounded dependency windows while live physical filters still allocated their
inputs, outputs, masks and clipping backdrops at document dimensions. This stage
connects the same document-coordinate compositor to bounded live filter windows.
It does **not** enable native photo documents in GTK.

The native renderer now preflights physical-filter image pixels before restoring
paint, resizing the composite or submitting a frame. Below a provisional **256
MiB image-pixel ceiling**, it retains the ordinary incremental image cache. Above
that ceiling, neighborhood chains use 1024-, 512- or 256-pixel output windows,
including every visible pass's cumulative halo. Output pages do not overlap;
input halos do. Groups, clipping, translated masks, mask-area overlays and
document-coordinate sampling use the existing compositor and Float32 formats.
Adjustment/topology/background changes invalidate the complete result; paint
damage expands through the chain; animation refreshes even without paint damage.

Each window finishes its queued work before releasing its image set and staging.
An initial drain retires the previous cache before replacing it. Source uploads
and window drains share one submission helper and retain separate counters.
This is a correctness-first execution path on the render owner, with bounded
image lifetimes; chunk scheduling, cancellation and latency optimization remain
unfinished. The queue/staging ownership was checked against the pinned
[wgpu 30.0.1 queue contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_buffer)
and [polling contract](https://docs.rs/wgpu/30.0.1/wgpu/type.PollType.html).

A document-wide sampler still receives the entire document. If its conservative
allocation plan exceeds the ceiling, or a neighborhood halo cannot fit around
one output page, the renderer returns an explicit error before changing the
current document or canvas. It does not crop the dependency or narrow precision.
Small supported global effects continue to use their full inputs. An asynchronous
global-job workflow and the corresponding GTK limits/error handling remain work.

Correctness evidence on the existing Linux/Vulkan RTX PRO 6000 reference system
(driver 610.57.04, kernel 7.1.10-200.fc44.x86_64):

| Check | Result and evidence under `artifacts/color-m2/` |
| --- | --- |
| Window planning | Two tests pass for 24/45/60 MP and a 32768×257 strip, including full output coverage, complete halos, per-window bounds and explicit global/halo rejection. `live-filter-plan.log`. These are dimension/allocation-plan tests, not rendered large-photo measurements. |
| Live GPU windows | Four tests pass, 42.66 s total. Four spaces × both depths × clipping on/off × isolated group on/off, with translated masks and mask-area overlays; full/window transitions, partial damage, animation/frozen time, paint crossing seams, undo/redo, renderer recreation and metadata invalidation. `live-filter-final-gpu.log`. |
| Image allocation ownership | The 777×533 comparison fixtures force a 16 MiB ceiling. Full image caches occupy 33,131,280–46,383,888 bytes; peak window caches occupy 6,094,080–8,531,808 bytes, including clipping uniforms. These counters describe owned resources, not driver residency or process RSS. |
| Edited-image comparison | The numerical ceiling is absolute 3e-6 per Float32 premultiplied channel. Observed maximum difference is zero in these fixtures; exact equality is not the filter acceptance requirement. Rejected frames preserve the exact existing composite; undo and raster ownership remain exact. |
| Existing image windows | Two tests pass, 16.63 s; `live-filter-windows.log`. |
| Native publication/history | Four tests pass, 40.48 s, including invalid/abandoned frames and paint → undo → save/reopen → continue → device replacement; `live-filter-native-edit.log`. |
| Snapshot/interchange | Eight tests pass, 44.35 s, including exact source identity, profiles, matte, dithering, dependency budget and cancellation; `live-filter-snapshot.log`. |
| Existing filters | Twenty correctness tests pass, 30.45 s, including the runtime pixel reference, physical/fused paths, masks and incremental invalidation. Latency tests explicitly excluded; `live-filter-filters.log`. |
| GTK | Production check and test build pass. Native diagnostics/fault/recovery check passes, 6.01 s; its invalid-scissor GPU failure is intentional. Native save/reopen and profiled file workflows pass, 38.41 s, including the optional custom CMYK branch. `live-filter-gtk-check.log`, `live-filter-final-gtk-build.{json,log}`, `live-filter-recovery.log`, `live-filter-files.log`. |
| External output handoff | ImageMagick reproduces all 983,040 integer16 RGB/gray/CMYK output samples and embedded ICC bytes exactly. `live-filter-handoff.log`; files from GTK process 1306851 are retained in `live-filter-ui/`. |

The final saved executables are `live-filter-final-gpu-tests` and
`live-filter-final-gtk-tests`; Cargo build mappings use `live-filter-final-build`
and `live-filter-final-gtk-build`. The earlier `live-filter-reviewed-gpu-tests`
ran the unchanged full-image, snapshot, native-publication and filter regressions;
the final binary adds the window-specific metadata invalidation and its tests.
`live-filter-provenance.json` records source, binary and log hashes.

Reproduce with the previously documented local libjpeg pkg-config setup:

```sh
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo test -p layer-render-wgpu --offline --lib --no-run --message-format=json
ABSOLUTE_GPU_TEST_BINARY scene::windows::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY tests::live_windows --test-threads=1 --nocapture
ABSOLUTE_GPU_TEST_BINARY tests::image_windows --test-threads=1
ABSOLUTE_GPU_TEST_BINARY raster::native_edit::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY snapshot::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY tests::filter_library --test-threads=1 --skip latency
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_diagnostics_and_gpu_failure_recovery ABSOLUTE_REPORT_PREFIX
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_document_files ABSOLUTE_REPORT_PREFIX
python3 tools/validation/icc_export_handoff.py \
  artifacts/familiar-workspace/files GTK_TEST_PROCESS_ID
```

The ceiling excludes source decoding, paint/material/mask pages, the full live
composite, reusable scene tiles, effect preparation tables, history, other
documents, surfaces and driver allocations. Those need their own bounds and a
combined measured budget. Live mutable/composite residency and source mips remain
prerequisites, as do native GTK activation, complete color/photo controls,
managed viewing and the remaining interchange journeys. The pre-existing latency
failures remain open. No frame-creation benchmark or performance optimization was
run in this stage; all elapsed times above are test durations. Other hosts remain
unintegrated and require user approval after GTK qualification.

## Losslessly backed native color residency (2026-09-14)

Parent: `c325268f`. Current live reconciliation still restored every committed
color tile into a mutable Float32 page, even though snapshots and retained photos
already decoded through fixed slots. Native live paint now retains the complete
immutable raster index independently of its mutable GPU cache. A provisional
**256 MiB color-cache target** controls eager restoration and eviction. Eager
restoration reserves room for both color surfaces; otherwise only already
resident color pages and the existing scalar planes are restored.

Only tiles with completed, successful lossless backing can be evicted. Dirty
coordinates, pending captures and active transform previews remain resident.
Eviction performs no readback, wait or new quantization. It drops cache ownership;
it does not destroy textures that queued commands may still reference. Read-only
composition, raw sampling/regions, thumbnails and transform snapshots obtain
cold color through the existing 16-slot decoded source cache. Original photo
pixels still show through wherever no paint override exists. A later brush dab
or image operation initializes its affected mutable pages from that same backing.
New revisions retain every cold tile and replace changed resident coordinates.
Undo and device recreation consume the complete immutable revision.

Native composition always uses the scene compositor, including plain paint
layers. During validation, the watercolor layer disappeared after its last
mutable page was evicted: the general layer-stack eligibility check considered
only original photos and resident paint. Eligibility now includes backed color.
The failure and its phase-by-phase reproduction remain in
`cold-paint-reviewed-gpu.log` and `cold-paint-brush.log`. Smudge, wet, liquify and
watercolor preview/terminal/eviction comparisons pass after this correction.
Review also found that Apply Mask must initialize affected cold or original-photo
pages before baking coverage; that route now does so. Transform captures keep
the original immutable backing, independently of changing preview output.

Thumbnail bounds and image draws consume decoded inputs in sets of at most 16.
The per-page uniform records remain immutable across those sets. This preserves
command order and avoids treating queued writes as immediately executed updates;
the pinned [wgpu 30.0.1 write-buffer contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_buffer)
and [completion callback contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.on_submitted_work_done)
were checked against the shared source uploader. This is shared Rust behavior;
no additional platform host was integrated or qualified.

Correctness fixtures use a 4352×512 image with 33 backed color tiles and one hole,
so queries exceed the 16-slot source cache. A zero-byte mutable color-cache target
is compared with an unlimited target. The fixtures cover:

- sRGB, P3, Adobe RGB and ProPhoto at both integer depths; complete composition,
  cropped thumbnails, point/Average5 sampling and connected raw-region masks.
- A localized native edit that materializes one tile, retains all unchanged tile
  bytes, saves/reopens exactly, evicts completed color, and undoes/redoes exactly.
- Repeated immutable transform previews and cancellation; terminal transform,
  alpha-locked fill and mask application, followed by eviction, renderer
  recreation and exact raster history comparison.
- Retained Adobe RGB photo pixels underneath ProPhoto paint overrides, including
  the hole, samples and thumbnail contributions.
- Smudge, wet, liquify and watercolor prediction, continued paint and terminal
  publication; material state survives color eviction.
- Two live neighborhood filters under a forced 8 MiB image-pixel ceiling, reading
  cold color through multiple dependency windows.

Complete working images use an absolute Float32 comparison ceiling of 3e-6 per
premultiplied channel; thumbnail bytes allow one output code. This compares two
residency strategies with the same editing math. It does not impose exact
historical filter parity. Stored tile data, untouched pixels and history remain
exact comparisons.

The six initial corrected tests pass in `cold-paint-fixed-gpu.log` (114.43 s).
The added operation/commit/history test passes in `cold-paint-operations.log`
(36.96 s). Build mappings are `cold-paint-fixed-build.{json,log}` and
`cold-paint-complete-build.{json,log}`; saved executables use the corresponding
prefix and `-gpu-tests`/`-gtk-tests` suffixes. Test duration is not frame latency.

Affected GPU regressions also pass on the same Linux/Vulkan RTX PRO 6000 system
and driver recorded in the preceding stage:

| Filter | Passed | Duration | Log under `artifacts/color-m2/` |
| --- | ---: | ---: | --- |
| Native publication/history | 4 | 47.16 s | `cold-paint-native-edit.log` |
| Native material/brush precision | 4 | 28.75 s | `cold-paint-material.log` |
| Retained-source neighborhood brushes | 1 | 19.05 s | `cold-paint-source-brushes.log` |
| Source thumbnails, cancellation and failed commands | 3 | 10.87 s | `cold-paint-thumbnails.log` |
| Document/view color, sampling and previews | 4 | 22.97 s | `cold-paint-view-color.log` |
| Snapshot and profiled interchange | 8 | 45.36 s | `cold-paint-snapshot.log` |
| Transform, selection, mask and source history | 7 | 21.21 s | `cold-paint-transforms.log` |
| Raster persistence and GPU recovery | 6 | 8.87 s | `cold-paint-raster.log` |

These 37 checks use `cold-paint-complete-gpu-tests`. Latency tests are explicitly
excluded and ignored performance workloads remain ignored. The final source
build (`cold-paint-final-source-build.{json,log}`) includes only formatting changes
after that build; its saved GPU/GTK executables use `cold-paint-final-source-`.
The production GTK check also passes (`cold-paint-final-gtk-check.log`).

Private-Mutter GTK checks with fatal GTK criticals pass: tool/color panels
(4.58 s), intentional diagnostics/GPU-failure recovery (3.25 s), and native
save/reopen plus profiled export (38.18 s), including custom CMYK. Logs use
`cold-paint-gtk-{panels,recovery,files}`; the invalid-scissor GPU failure in the
recovery test is deliberate. ImageMagick independently reproduces all 983,040
integer16 RGB/gray/CMYK samples and embedded ICC bytes exactly
(`cold-paint-handoff.log`). Output files from GTK process 1393016 are retained
in `cold-paint-ui/`. Source, executable and evidence hashes are recorded in
`cold-paint-provenance.json`.

Reproduce using the local libjpeg pkg-config setup described earlier:

```sh
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo test -p layer-render-wgpu -p layer-linux --offline --no-run --message-format=json
ABSOLUTE_GPU_TEST_BINARY tests::cold_paint --test-threads=1 --nocapture
ABSOLUTE_GPU_TEST_BINARY raster::native_edit::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY tests::native_material --test-threads=1
ABSOLUTE_GPU_TEST_BINARY tests::source_brushes --test-threads=1
ABSOLUTE_GPU_TEST_BINARY source_thumbnails::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY tests::view_color --test-threads=1
ABSOLUTE_GPU_TEST_BINARY snapshot::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY layer_tests::transforms --test-threads=1 --skip latency
ABSOLUTE_GPU_TEST_BINARY layer_tests::raster --test-threads=1
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_tool_and_color_panels ABSOLUTE_REPORT_PREFIX
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_diagnostics_and_gpu_failure_recovery ABSOLUTE_REPORT_PREFIX
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_document_files ABSOLUTE_REPORT_PREFIX
```

The color target is **not a hard active-edit memory limit**. Large strokes,
operations and transforms can pin more than the target; scalar/material pages
remain eager. Composite storage, source display mips, job scheduling/cancellation
and combined device/process budgets remain work before native GTK activation.
The full GTK color/photo controls, managed viewing and interchange journeys also
remain incomplete. No benchmarking or performance optimization was performed in
this stage. Previously recorded latency failures remain open for the final
qualification phase. Other hosts still require approval after GTK qualification.

## Exact artwork queries independent of display storage (2026-09-14)

Parent: `9216852b`. Inspection found three dependencies on the displayed
composite that would make reduced display caches unsafe: composite color picking,
connected selections and an opportunistic filter-preview copy. Composite picking
also included the mask-area inspection tint. This stage removes those dependencies
before changing the live composite's storage. The full live composite itself is
still allocated; this is not completion of that residency prerequisite.

Successful submissions retain composition metadata, effective background, time
and current preview styles. They retain no additional document-sized pixel image.
Composite point/Average5 queries capture at most 5×5 working pixels. Composite and
projected-layer selections capture one 256×256 tile at a time into a reusable
target, then classify it into the existing packed eligibility mask. Raw-layer
selections continue using the 16-slot source path. The old full-document selection
color target and its capture entry point are deleted. Filter previews always
capture their insertion scope; the displayed-composite copy shortcut is deleted.

Queries preserve masks, opacity, groups, clipping, complete physical-filter halos
and current watercolor prediction while excluding mask-area, checkerboard and
presentation overlays. Watercolor style records now use layer IDs, so a projected
layer list does not accidentally address another layer's style record. Obsolete
positional style-base fields and assignments are removed. Background opacity in
projected selections is applied once from the unattenuated document background.

Each artwork-query capture preflights physical-filter image pixels against a
provisional 256 MiB ceiling. A document-wide dependency retains its full support
or returns an explicit limit error; it never substitutes a cropped or lower-
precision input. Native queue completion retires a previous filter window before
allocating its replacement. The source decoder retains its fixed slots and
ordered staging; the pinned [wgpu 30.0.1 staging-belt ownership contract](https://docs.rs/wgpu/30.0.1/wgpu/util/struct.StagingBelt.html)
was checked against the current upload implementation. These synchronous chunk
drains still require job scheduling/cancellation and latency qualification.

A new partial-alpha fixture exposed an empty zero-tolerance selection, including
at its seed. The seed shader previously stored encoded comparison color, while
the classifier separately encoded candidates. Moving both comparisons into the
classifier and accepting identical working samples fixes the failure without
adding an arbitrary tolerance. This avoids reliance on cross-pipeline Float32
rounding identity, which is not a general promise of [WGSL floating-point
accuracy](https://www.w3.org/TR/WGSL/#floating-point-accuracy). The failing output
is retained in `artwork-query-gpu.log`; the corrected two-test run is in
`artwork-query-seed-gpu.log` (9.17 s).

Three final artwork tests cover:

- P3 integer16 and ProPhoto integer8 with partial alpha, layer opacity, mask-area
  inspection, tile seams and partial document edges. The display texture/view/
  binding are removed. Point/Average5 values match a Float64 transfer/coverage
  reference within 3e-6, connected selection has the independently specified
  rectangle, cold paint remains unmaterialized, and the retained display texture
  is byte-for-byte unchanged.
- Two neighborhood filters, edge/seam sampling against full-resolution working
  output, and pre-allocation rejection of an insufficient image budget. A
  deliberately overwritten display texture remains untouched by queries.
- Watercolor paint and prediction with an unrelated style slot ahead of the
  queried layer. A projected-layer crop matches the complete working image within
  3e-6 per channel across page boundaries.

The filter-preview regression additionally clears the displayed composite while
retaining it, discards preview caches and repeats a top-level insertion query.
Its output must remain exactly equal to the earlier preview. These are residency
and source-ownership comparisons; perceptual equivalence remains the acceptance
criterion for edited filters, separate from exact saved/undo state.

GPU results on the Linux/Vulkan RTX PRO 6000 reference system (driver 610.57.04):

| Check | Result | Log under `artifacts/color-m2/` |
| --- | --- | --- |
| New artwork queries | 3 pass, 10.18 s | `artwork-query-final.log` |
| Existing region/flood/selection and related region primitives | 12 pass, 19.15 s | `artwork-query-regions.log` |
| Document/view color, raw samples and previews | 4 pass, 26.05 s | `artwork-query-view.log` |
| Snapshot and profiled interchange | 8 pass, 42.95 s | `artwork-query-snapshot.log` |
| Cold color, save/reopen, history, transforms and filter windows | 7 pass, 138.11 s | `artwork-query-cold.log` |
| Watercolor and its material/selection/prediction paths | 13 pass, 21.56 s; includes the new projected query test above | `artwork-query-watercolor.log` |
| Filter previews, probe cancellation, insertion scopes and physical crops | 4 pass, 11.73 s | `artwork-query-previews.log` |

All performance workloads are excluded. These durations are correctness-suite
elapsed times, not latency measurements. The first six rows use the saved
`artwork-query-final-gpu-tests` executable and its matching Cargo JSON/log build
mapping. The final preview change and its added check use
`artwork-query-reviewed-gpu-tests`; that build also produces
`artwork-query-reviewed-gtk-tests`. The production GTK check passes in 2.52 s
(`artwork-query-gtk-check.log`).

GTK's connected-tool fixture initially failed its gap-closing check. The saved
parent executable (`cold-paint-final-source-gtk-tests`) reproduces that failure
in `artwork-query-parent-tools.log`; an extra two seconds of event processing
does not fix it. A diagnostic screenshot and layer inspection show no ink:
the fixture started its pen contact before the brush was ready, and the native
input gate correctly suppressed the whole contact. The fixture now waits for
that actual readiness gate and asserts a committed boundary stroke. Gap closing
then passes. Its later fill assertions also still expected operations to remain
queued, predating committed raster ownership. They now verify nonempty raster
publication and exact revision restoration on undo/redo. Clearing the foreground
before picking additionally prevents the existing fill color from hiding a
failed picker. No application behavior or gap-closing tolerance was relaxed.
The corrected fixture passes in 5.65 s (`artwork-query-gtk-raster-tools.log`), using
`artwork-query-gtk-raster-tests` and its matching build mapping. Intermediate
failures and the blank-canvas diagnostic remain in the evidence directory.

Private-Mutter GTK diagnostics/GPU recovery passes in 4.60 s and native
save/reopen/profiled export passes in 38.86 s
(`artwork-query-gtk-{recovery,files}.log`, reviewed GTK executable). The invalid
scissor in the recovery test is intentional. Independent ImageMagick decoding
of process 1420539's integer16 RGB, gray and CMYK outputs matches all 983,040
samples and embedded ICC bytes exactly (`artwork-query-handoff.log`). These
checks use the same local GPU and private compositor as the preceding stage;
they do not establish calibrated display agreement or physical tablet delivery.
Source, executable, output and evidence hashes are recorded in
`artwork-query-provenance.json`.

Reproduce with the existing local libjpeg pkg-config setup:

```sh
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo test -p layer-render-wgpu -p layer-linux --offline --no-run --message-format=json
ABSOLUTE_GPU_TEST_BINARY artwork::tests --test-threads=1 --nocapture
ABSOLUTE_GPU_TEST_BINARY region --test-threads=1 --skip latency --skip workloads
ABSOLUTE_GPU_TEST_BINARY tests::view_color --test-threads=1
ABSOLUTE_GPU_TEST_BINARY snapshot::tests --test-threads=1
ABSOLUTE_GPU_TEST_BINARY tests::cold_paint --test-threads=1
ABSOLUTE_GPU_TEST_BINARY watercolor --test-threads=1 --skip latency --skip workloads
ABSOLUTE_GPU_TEST_BINARY scene::previews::tests --test-threads=1 --skip latency
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_connected_tools ABSOLUTE_REPORT_PREFIX
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_diagnostics_and_gpu_failure_recovery ABSOLUTE_REPORT_PREFIX
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_document_files ABSOLUTE_REPORT_PREFIX
```

The capture ceiling excludes source slots, reusable scene scratch, shader tables,
packed classification/connected-component/history buffers and driver allocations.
Existing connected-component device limits still apply. Large-document query
cancellation, scheduling and total memory/latency budgets remain unfinished. The
legacy explicit readback API still consumes full composition; native GTK file
output uses the independent streaming snapshot worker. Live display/composite
residency and source mips remain next, alongside active-edit bounds and complete
GTK color/photo controls and managed viewing. No benchmarking or performance
optimization ran in this stage; other platform hosts remain unintegrated and
require approval after GTK qualification.


## Bounded display mip reduction and native preview (2026-09-14)

Parent: `8c07eef2`. A completed Float32 composition tile can now be reduced into
an independent coarse display image through a reusable 256×256 mip chain. The
coarse image has at most 512 pixels on either side. The planner retains document
dimensions separately and selects a power-of-two footprint; coarse pixels plus
all scratch mip levels total less than 6 MiB for the supported plans. This is an
allocation bound for this component, not a measured total-process memory result.

Each reduction pass loads four premultiplied working-color samples and weights
them by their actual original-pixel coverage. Partial right/bottom footprints
exclude stale padding. Float32 storage preserves negative and out-of-gamut working
coordinates; this stage introduces no Float16 narrowing. The single-level views
read the previous mip and write the next, consistent with the pinned wgpu 30.0.1
subresource definitions and the [WGSL unfiltered texel-load contract](https://www.w3.org/TR/WGSL/#textureLoad).
Queued tiles reuse immutable per-edge-size uniform records. A tile update changes
only its derived footprint, leaving neighboring coarse pixels untouched.

The native Float32 `CanvasPreview` readback API is the first consumer. It weights
coarse-cell overlap using the original document geometry before color conversion,
so an incomplete edge does not stretch the image. A one-bright-column-per-four
stripe pattern retains its quarter coverage; fixed sparse sampling can alias
that pattern away. Alpha is averaged with premultiplied color, then the existing
straight-alpha/sRGB output boundary is applied. Reusing a known revision returns
no new image. The mip pipeline uses the background compiler; a request arriving
while it is unavailable remains retryable and allocates no preview pixels.

The reducer is ready to consume individual completed scene tiles. **The live
full-document composite has not yet been replaced.** This first API integration
currently copies tiles from that composite and rebuilds the coarse image when a
new preview is requested. GTK's in-surface Navigator/viewport still use their
existing presenter; they have not yet been connected to this cache. Direct
incremental scene output, detailed visible-tile residency, presentation and source
mips remain the next work. No native color mode is enabled by this change.

Four new correctness checks cover:

- Allocation-only plans for 24/45/60 MP, a 32768-pixel strip, partial edges and a
  one-pixel document; invalid/oversized plans fail before allocation.
- Complete queued tile reductions against independent Float64 area sums on
  257×3, 513×273, 2051×1027 and 4097×1 inputs, including alpha, negative color,
  seams, partial footprints, stale padding and local updates. The absolute
  Float32 error ceiling is 3e-7; unchanged source pixels and neighboring cached
  pixels compare exactly. At most four sets of edge-size records are retained.
- Native preview stripes, partial-alpha output, unchanged artwork and known-
  revision reuse. A second fixture compares every output channel with an
  original-coordinate Float64 box integral, allowing one 8-bit output code.
  The first test draft used eight-pixel cells although its 4101-pixel width
  selects sixteen-pixel cells. The retained failure (`display-mips-gpu.log`)
  exposed that incorrect fixture assumption; the corrected independent fixture
  uses sixteen-pixel cells without changing the rendering tolerance.
- A deliberately blocked compiler, no preview allocation while blocked, and
  successful request retry/readback after release. This is lifecycle correctness,
  not a startup latency measurement.

On the same Linux/Vulkan RTX PRO 6000 reference system and 610.57.04 driver:

| Check | Result | Log under `artifacts/color-m2/` |
| --- | --- | --- |
| Four new mip/preview/planning checks | 4 pass, 13.22 s | `display-mips-final.log` |
| Document/view color, export, samples and preview alpha | 4 pass, 53.00 s | `display-mips-view.log` |
| In-surface overview, retained-source overview and presenter resources | 7 pass, 26.84 s | `display-mips-overview-checked.log` |
| GPU startup, filters, regions and transforms | 5 pass, 9.46 s | `display-mips-startup-checked.log` |

The final tests use `display-mips-final-gpu-tests`, mapped by
`display-mips-final-build.{json,log}`. The view-color row uses
`display-mips-corrected-gpu-tests`; the only subsequent production change starts
the already-enqueued background compiler from the preview request, covered by
the final blocked-compiler test. Early filters named `overview_tests` and
`startup::tests` selected zero and one CPU test respectively; their logs are
retained but are not counted as GPU qualification. The corrected filters and
counts above are verified against the executable's test list.

Reproduce with the existing local JPEG dependency setup:

```sh
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo test -p layer-render-wgpu -p layer-linux --offline --no-run --message-format=json
ABSOLUTE_GPU_TEST_BINARY display_mips::tests --test-threads=1 --nocapture
ABSOLUTE_GPU_TEST_BINARY tests::view_color --test-threads=1
ABSOLUTE_GPU_TEST_BINARY overview --test-threads=1 --skip latency --skip workloads
ABSOLUTE_GPU_TEST_BINARY startup::gpu_tests --test-threads=1
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo check -p layer-linux --offline
```

Production GTK builds successfully (`display-mips-gtk-check.log`). Source,
executable and evidence hashes are recorded in `display-mips-provenance.json`.
No performance workloads, benchmark comparisons or optimizations ran here.
Existing frame, total residency and latency gates remain open; correctness-suite
durations above are not performance evidence. The full GTK color/photo controls,
managed viewing and other platform gaps recorded earlier remain outstanding.
Other platform host integration still requires approval after GTK qualification.


## Live bounded composition and exact readback (2026-09-14)

Parent: `954b6504`. Native Float32 documents whose full composite would exceed
64 MiB now compose directly into a bounded display cache. It retains the complete
coarse image from the preceding stage and a toroidal texture of visible detail
tiles. There is no full-resolution Float32 composite for these documents. Small
native documents keep their existing dense composite. The provisional 256 MiB
display ceiling includes coarse pixels, mip scratch, detail pixels and immutable
geometry records; it is not a qualified combined GPU/process budget.

The inverse camera bounds determine required detail. Power-of-two reduction uses
the largest affine scale so a chosen mip texel is no larger than a surface pixel.
Cached world-tile identities survive pans; a resident one-pixel pan performs no
composition and does not advance the artwork revision. A change of mip or newly
visible tile regenerates its required source pages. Fine sampling wraps across
the atlas without seams; original document dimensions determine partial-edge
sample positions. The viewport and in-surface Navigator consume the cache, and
the readback preview reuses its coarse image. Neither display consumer supplies
pixels to edits, picking or delivery.

The cache preflights required storage before painting. When a tall view follows
a wide view, it can replace the old allocation with the required shape instead
of retaining an over-budget bounding rectangle. Queued users retire before the
old detail texture is destroyed; presenters rebind when texture/buffer identities
change. A rejected view leaves the preceding presentation intact. Discarded tile
writes invalidate their slots; only successful frame completion publishes them.
Large-document startup now includes mip reduction in canvas readiness, before
any display pixels are allocated.

Explicit RGBA8 readback now composes exact 256-pixel artwork crops with physical
filter halos and converts/copies them into the requested result. Its API still
owns a complete RGBA8 result texture and staging buffer, with a device-buffer
size check. It no longer requires or temporarily rebuilds a full-resolution
working composite. Interactive GTK file output continues to use the streaming
snapshot worker. The obsolete inspection export/recompose/restore path and its
separate retained layer snapshot are deleted. Removing source topology or changing
document dimensions invalidates obsolete exact-query metadata and releases its
retained originals.

Correctness checks exposed and fixed these issues before adoption:

- Reconstructing surface pixels from interpolated UVs differed near sharp color
  transitions. The viewport now uses fragment `position.xy`, whose pixel-center
  semantics are specified by [WGSL](https://www.w3.org/TR/WGSL/#position-builtin-value).
  Both original comparisons pass at their unchanged 5e-6 Float32 ceiling. The
  initial failures are retained in `live-display-gpu.log`.
- Tiled export initially lacked `COPY_DST` on its RGBA8 destination. GPU
  validation caught it; the destination now declares its actual copy uses.
- Exact readback exposed the direct brush-preview path, which owns no paint
  page. Exact captures replay the retained GPU drawing commands into the crop.
  Watercolor captures also require tile coordinates and raw layer opacity;
  separate scene records prevent applying opacity twice or losing an edge on a
  nonzero tile. Existing drawing tolerances were not relaxed.
- Native redo initially recycled staging memory for display geometry through a
  separate raster-restoration submission. Diagnostic readbacks showed finite,
  correct paint/coarse/detail pixels but invalid presentation. Display geometry
  is now staged after independent restoration, alongside the frame's style
  records. The same native stroke/undo/redo/device-replacement test then passes.
  The pinned wgpu 30.0.1 implementation of
  [StagingBelt](https://docs.rs/wgpu/30.0.1/wgpu/util/struct.StagingBelt.html)
  attaches recall of all closed chunks to the supplied submission; it does not
  infer that another unsubmitted encoder still needs their contents.
  `live-display-{stroke-backtrace,redo,order}.log` retain the diagnosis and fix.
- The new property-edit fixture first omitted `FramePacket::composite_all`,
  although real layer-property actions request recomposition. The fixture now
  follows that contract. Camera-only tests continue to require cache reuse.
  The prior zero-alpha output test now exercises the conversion boundary
  directly: deliberately changing display pixels must not change exact export.

Eight new checks cover visible detail across pans, toroidal wrapping, rotation,
reflection, enlargement and minification; independent Float64 area averaging;
budget failure and discarded writes; allocation shape changes with a retained
presenter; masked physical filters, translated source, inspection and restoration;
in-surface Navigator/readback agreement and clipping; deferred startup; and
native U16 ProPhoto painting across a tile boundary, exact undo/redo and device
replacement. Dense/cache Float32 presentation uses an absolute 5e-6 comparison;
the Navigator comparison allows one output code. Source and committed backing
identity/restoration checks remain exact.

The first complete renderer run found four failures (213 pass, 4 fail, one
explicitly ignored 4K replay, 780.12 s). Direct preview and both watercolor
failures are fixed by the capture changes above. The fourth found a retained
source after document topology removal and is fixed by query-metadata retirement.
All original failure logs remain under `artifacts/color-m2/`; they are evidence
of issues found, not successful qualification. The reviewed full renderer suite
passes **221 tests, zero failures**, with one explicitly ignored 4K replay and
25 performance workloads filtered out, in 899.29 s (`live-display-reviewed.log`).
This includes all eight new cases and every previously failing case. The exact
executables are `live-display-reviewed-{gpu,gtk}-tests`, mapped by
`live-display-reviewed-build.{json,log}`. Code and artifact hashes, including
the retained intermediate binaries and logs, are in `live-display-provenance.json`.

Private-Mutter GTK checks pass on the exact reviewed source using
`live-display-reviewed-gtk-tests`: connected tools 5.60 s, diagnostics/GPU recovery
3.24 s and native files/profiled delivery 38.72 s
(`live-display-reviewed-gtk-{tools,recovery,files}.log`). Independent ImageMagick
decoding matches all 983,040 U16 RGB/gray/CMYK samples and embedded ICC bytes
exactly (`live-display-reviewed-handoff.log`, process 1582742). Output artifacts
are copied into `live-display-ui/`. An earlier build also passed all three GTK
checks and the same external handoff; its logs and process 1563743's outputs are
retained separately. Production GTK checks successfully on the reviewed source
(`live-display-reviewed-gtk-check.log`). Correctness-test durations here do not
establish interaction, frame-creation or end-to-end latency budgets.

Reproduce with the same local JPEG dependency setup and physical reference GPU:

```sh
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo test -p layer-render-wgpu -p layer-linux --offline --no-run --message-format=json
ABSOLUTE_GPU_TEST_BINARY --test-threads=1 --skip latency --skip workloads
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_connected_tools ABSOLUTE_REPORT_PREFIX
bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_diagnostics_and_gpu_failure_recovery ABSOLUTE_REPORT_PREFIX
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh ABSOLUTE_GTK_TEST_BINARY \
  native_document_files ABSOLUTE_REPORT_PREFIX
PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig" \
  cargo check -p layer-linux --offline
```

This is correctness qualification of the shared renderer on the existing
Linux/Vulkan RTX PRO 6000 reference system, not completion of the native GTK
photo workflow. GTK's production factory still selects the existing SDR8 mode.
The private 1600×1000@120 compositor uses software test input and establishes
neither calibrated monitor agreement nor physical tablet delivery.

Remaining work includes full GTK color/photo controls and managed display
activation; source/display scheduling and cancellation; active-edit, scalar-plane
and combined source/filter/display/history/staging budgets; complete zoom and
anisotropic-minification quality qualification; and the remaining profiled
interchange/inspection workflows. The full RGBA8 readback allocation belongs to
its explicitly requested API, not the interactive display ceiling. No performance
workload or benchmark comparison ran in this stage. Fresh frame baselines,
regression investigation, optimization and final memory/latency qualification
remain last, as requested. Other platform host integration remains unapproved.

## GTK native SDR factory and recovery (2026-09-14)

Parent: `d669cefd`. GTK now constructs its GPU owner for the document's explicit
RGB space and integer depth. The surface-compatible staged constructor selects
Float32 color and scalar working attachments before creating any immediate or
deferred pipeline recipes. It shares native transfer, integer publication and
backing initialization with the headless renderer. GTK requests and checks both
`FLOAT32_FILTERABLE` and `FLOAT32_BLENDABLE`; a device lacking either reports an
unsupported editing-format error rather than selecting a narrower working format.
Pipeline construction and native scratch preparation stay on `canvas-gpu`.

The main-thread transport reports the same document interpretation and retained
source capability before session adoption. New windows derive these from their
project; blank windows use sRGB8. Restart derives them from the surviving document,
so replacing a failed GPU owner cannot reinterpret P3 or ProPhoto paint as sRGB8.
The GTK factory's previous encoded8 working-renderer selection is removed. Other
platform factories are unchanged. Default sRGB8 still uses its existing declared
premultiplied-linear encoded integer backing descriptor; unifying that descriptor
with the other native modes remains separate cleanup, not an archive migration.

The presentation route remains explicitly tagged sRGB. This stage connects native
document rendering, including wide-gamut source composition through that fallback;
it does not complete wide-color numeric/picker controls or managed wide-gamut GTK
widgets/monitor transitions. The current compact picker still assumes sRGB and
must be connected to portable color definitions before wide-document paint-color
selection and sampling can be qualified. New/Open/Place/profile/depth controls,
source policy and the other outstanding photo journeys remain unfinished.

New native-window checks cover every combination of sRGB, Display P3, Adobe RGB
and ProPhoto with integer8/integer16. Each retains an independently tiled photo
with matching embedded ICC bytes, paints across two tile boundaries, checks
committed descriptors, restores undo/redo pixels, writes/reads an editable project,
and opens the result in another real GTK window. Original source samples/profile
and committed tile bytes survive exactly. A 4097×1025 ProPhoto16 window also checks
the host's initial paper presentation, bounded-display startup, painting and an
exact composite sample. P3 8-bit and ProPhoto16 cases extend the existing deliberate
GPU validation failure test: failed producers resolve, the last backed checkpoint
remains saveable, Restart restores exact pixels and interpretation, and earlier
undo/redo plus subsequent drawing remain usable.

Validation exposed stale native fixtures as well as a new fixture setup error:

- The first wide-recovery fixture registered the same GTK application twice in
  one test. It now owns one application for both cases; the first case's actual
  GPU recovery had already passed before registration of the second failed.
- Selected brushes, operation controls and Navigator overlap fail identically
  with the saved parent executable (`gtk-native-parent-*.log`). The first two
  begin contacts before the native brush-readiness gate; the third places its
  sample using obsolete panel coordinates. Native pen-path fixtures now wait for
  actual readiness, sharing the connected-tools gate. Navigator overlap uses
  allocated image/panel positions and still checks both pixel occlusion orders.
- Operation controls now check published raster identities instead of a pending
  operation count from the removed reconstruction model. Their real pixel,
  linked/unlinked mask, cancellation and exact undo checks remain. Narrow-control
  checks select the actual column divider and distinguish the control's unchanged
  three-tile minimum from larger minima imposed by neighboring panels.
- Navigator resizing selects current panel groups instead of historical IDs,
  specifies explicit column sizes instead of automatic tab-label fitting, and
  explicitly expands the column before testing collapse. A visible open member
  of a collapsed stack is a different legitimate starting state. Each GTK case
  runs in its own test process; serial Rust test threads still cannot initialize
  GTK successively on different threads in one process.

No production drag, layout, contact suppression or readiness policy changed.
The required drag convention was read while checking the resize fixture; it uses
an existing divider handle with immediate movement after slop. The parent and
intermediate failures, diagnosis logs, snapshots and exact executables remain in
`artifacts/color-m2/gtk-native-*`.

All **13 final targeted GTK checks pass**, one per process, on the reviewed
executable `gtk-native-expanded-tests` from
`gtk-native-expanded-build.{json,log}`. `gtk-native-verified-results.json` records
the exact test names/results; `gtk-native-expanded-sources.json` records code
hashes. Final correctness durations are:

| Native GTK check | Result / duration |
| --- | --- |
| All eight SDR source/paint/save/reopen modes | pass, 35.76 s |
| Bounded-canvas startup and exact sampling after paint | pass, 2.02 s |
| P3 8-bit and ProPhoto16 GPU failure/restart | pass, 5.98 s |
| Ordinary diagnostics/GPU failure/restart | pass, 3.28 s |
| Point/average sampling controls | pass, 2.42 s |
| Selected G-Pen, wet round and watercolor brushes | pass, 6.50 s |
| Transform controls, linked/unlinked masks and undo | pass, 7.19 s |
| Gradient tools | pass, 7.08 s |
| Navigator layering, clipping and idle behavior | pass, 8.99 s |
| Navigator held column collapse/reopening on both sides | pass, 8.16 s |
| Runtime filter packages | pass, 2.97 s |
| Connected drawing tools | pass, 5.59 s |
| Native files and profiled delivery | pass, 35.81 s |

These durations do not establish frame-creation or interaction latency budgets.
Production GTK checks in 2.24 s (`gtk-native-check.log`). Earlier factory,
fixture and layout checks, including failures, remain recorded separately.
Code/artifact hashes and retained image hashes are in `gtk-native-provenance.json`.
The selected-brush and Navigator overlap snapshots were also visually inspected;
that inspection does not qualify calibrated physical monitor agreement.

Independent ImageMagick decoding of process 1630194's reviewed GTK exports
matches all 983,040 U16 RGB/gray/CMYK samples and ICC bytes exactly
(`gtk-native-verified-handoff.log`, copies in `gtk-native-ui/`). The previous
factory build's process 1607462 also matches exactly (`gtk-native-handoff.log`).
Reproduce the independent check with:

```sh
python3 tools/validation/icc_export_handoff.py \
  artifacts/familiar-workspace/files PROCESS_ID
```

All three reported maximum code differences are zero in these runs. This tests
delivery through the native GTK factory; it does not establish arbitrary-profile
paint-color UI support.

Reproduce using the local JPEG dependency setup above and the same Linux/Vulkan
reference GPU. Build with `cargo test -p layer-linux --offline --no-run
--message-format=json`; run each fully qualified native test through
`tools/performance/gtk-raster.sh` in a separate process. The retained
`gtk-native-expanded-tests-exact` wrapper adds `--exact`, avoiding accidental
substring matches such as Navigator plus Navigator column resizing. Pass
`LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm` for document files.
Use `cargo check -p layer-linux --offline` for the production configuration.

This is an intermediate integration checkpoint. Combined memory accounting,
scheduling/cancellation, all color/photo UI, wide display agreement and final
24/45/60 MP/multiple-document workload limits remain open. The reference GPU has
both required Float32 features; constrained and unsupported GPU policies are not
qualified by these checks. No performance workloads ran. Fresh baseline and parent
comparisons, regression investigation, optimization and memory/latency acceptance
remain last. Other platform host integration still requires the user's approval.

## Portable paint definitions and document-gamut GTK picking

This checkpoint builds on `2a985772`. It removes the remaining sRGB reinterpretation
from the shared foreground/background paint path and GTK picker. It is an
intermediate correctness checkpoint, not milestone 2 completion or performance
qualification. No other host integration was changed.

`RgbColor` retains straight encoded RGB, its defining built-in space, and independent
alpha. Finite extended RGB survives serialization and conversion. Changing documents,
picker shape, readout or preview never replaces the definition with clipped picker
coordinates. Alpha edits retain the original space/RGB. The workspace stores these
definitions; adopting a workspace resolves its picker and brush in the receiving
document. The old untagged color-array payload is replaced rather than migrated.

Brush paint, preset secondary pigment, figures and both gradient endpoints resolve
into document-linear RGB. Document adoption also converts retained secondary pigment;
resetting presets resolves their sRGB definitions into the current document. Point,
3×3 and 5×5 sampling encode exact document-linear artwork in its own space, keep
extended RGB, ignore fully transparent samples and leave brush opacity independent.
The existing `BrushState.color` is an sRGB widget preview, never a source for pigment.

The numerical design was checked against the [CSS Color 4 conversion code](https://www.w3.org/TR/2026/CRD-css-color-4-20260913/#color-conversion-code)
and [Ottosson's picker reference](https://bottosson.github.io/posts/colorpicker/).
The latter's fitted cusp model is specifically sRGB. Our four-space extension derives
channel-boundary cubics from the working RGB matrix and white adaptation. A hue ray
near blue can leave and reenter the gamut: selecting its first root failed the existing
sRGB round-trip checks. Selecting the outermost feasible root fixes that failure
without loosening those existing checks. The smooth perceptual hue ring remains the
absolute sRGB reference guide; the disc itself covers the document gamut.

Independent checks cover 14,400 hue boundaries and a 21³ RGB grid in each space.
Maximum grid round-trip errors (linear / encoded RGB) were:

| Space | Maximum linear error | Maximum encoded error |
| --- | --- | --- |
| sRGB | 6.780e-7 | 4.195e-6 |
| Display P3 | 7.077e-7 | 4.979e-6 |
| Adobe RGB | 7.396e-7 | 5.229e-4 |
| ProPhoto | 2.345e-6 | 3.949e-6 |

Adobe RGB's transfer has no linear toe, magnifying minute near-zero residues from
f32 hue coordinates. Picker projection validation checks linear error below 3e-6
and encoded error below one quarter of an 8-bit code; the retained definition never
round-trips through these coordinates. These are picker tolerances, not an exception
to exact stored-U16/undo requirements. All original sRGB author-reference and grid
checks retain their 2e-5 encoded tolerance. The initially uniform Adobe preview
transfer table missed visible near-black codes; exact power-law evaluation fixes it.
All 16 working/display pairs now satisfy the one-byte field-preview test.

GTK evaluates all three picker fields in working RGB before converting their pixels
to its explicit sRGB widget fallback. Its old Cairo square/triangle endpoint gradients
are deleted. Field cache keys include hue, size, shape and working space. Paint chips,
markers and retained toolbar/drawer icon palettes use derived previews. A compact `!`
and accessible tooltip identify document/sRGB-preview gamut limits without changing
the stored color. Monitor-aware wide widget presentation remains outstanding.

Validation artifacts use the `artifacts/color-m2/portable-color-` prefix:

- `shared-validation.log`: all 380 shared UI tests and the workspace migration
  check pass. The new protocol checks cover document/workspace adoption, multiple
  brush presets, both figure/gradient colors and extended samples in all four spaces.
- `workspace-native.log`: all 86 SQLite/native workspace tests pass, including
  persistence, failure, recovery and ownership checks.
- `final-values.log`: all three portable-core-color checks pass, including an
  independent P3-red conversion, extended round trips and rejected overflow.
- `production-check.log`: production GTK compiles without warnings.
- `native-ready-build.{json,log}`, `native-sources.json`: captured GTK test build,
  code hashes and executable SHA-256
  `370a32a8eeae5532e3e8a3d9816641369b25fe4ef59a9604db006f27db1595c6`.

All six final native checks pass on that executable:

| Check | Correctness-run duration |
| --- | --- |
| Portable paint and point/3×3/5×5 samples, four U16 document spaces | 21.92 s |
| Native tool/color controls | 4.62 s |
| Wide-color GPU failure/restart | 6.01 s |
| Gradient tools | 7.09 s |
| Native files/profiled delivery | 35.93 s |
| Real Mutter mouse/touch color-panel input | 30.57 s |

The four-space paint test checks native sampled coordinates within two U16 codes
and exact unchanged pixels across sampling. File/restore tests keep their existing
exact assertions. These durations do not establish latency or throughput budgets.
The mouse/touch case retains 88 screenshots; light 280px circle/triangle and dark
144px square captures were visually inspected. The panel screenshots exercise layout
and sRGB fallback appearance, not calibrated physical monitor agreement. The driver
now accepts `LAYER_NATIVE_TEST_EXECUTABLE`, avoiding an unrelated release rebuild.
Nonfatal portal/secret-service warnings are preserved in the native-input log.

ImageMagick independently decodes GTK process 1665254's RGB TIFF, gray PNG and CMYK
TIFF with all 983,040 U16 samples and embedded ICC bytes exact: each maximum code
difference is zero (`external-handoff.log`). The CMYK input profile was the retained
`artifacts/familiar-workspace/files/custom-cmyk-1630194.icc` from the preceding
qualification; the delivered ICC hash is recorded. Copies are in `portable-color-ui/`.

Reproduce with the JPEG pkg-config setup described above:

```sh
cargo test -p layer-ui --offline -- --test-threads=1
cargo test -p layer-workspace --features native --offline -- --test-threads=1
cargo test -p layer-core --offline color::value:: -- --test-threads=1
cargo test -p layer-linux --offline --no-run --message-format=json
cargo check -p layer-linux --offline
```

Use `gtk-raster.sh` with the captured executable and an `--exact` wrapper, one
fully-qualified test per process. `native-checks.json` records four existing test
names; the additional paint test is
`workspace::tests::color_management::native_sdr_portable_paint_and_sampling`.
For the real-input case:

```sh
LAYER_NATIVE_TEST_EXECUTABLE="$PWD/artifacts/color-m2/portable-color-gtk-tests" \
LAYER_TEST_ARTIFACTS="$PWD/artifacts/color-m2/portable-color-panel" \
  bash tools/performance/workspace-motion.sh gtk --color-panel
python3 tools/validation/icc_export_handoff.py \
  artifacts/familiar-workspace/files 1665254
```

The next functional work is numeric Edit Color and reusable swatches, followed by
the remaining New/Open/Place, document conversion/precision, photo inspection,
preferences and delivery UI. GTK managed wide viewing/monitor transitions, combined
memory/scheduling gates and the final hardware workload matrix remain open. Fresh
frame-creation baselines, regression investigation, optimization and performance
acceptance remain last. The milestone 1 cleanup/prerequisite gaps recorded above
still apply. Other platform hosts require the user's approval before integration.


## Numeric Edit Color and named palettes (GTK, 2026-09-15)

The color-chip context menu now opens Edit Color and Color Swatches. Numeric
entry supports normalized document RGB, explicitly sRGB hex, document HSV/HLS,
and absolute OKLCH. Switching models and accepting an untouched dialog preserves
the original portable definition and alpha exactly, including colors outside the
preview gamut. Alpha-only edits retain RGB; invalid/nonfinite input disables
acceptance, and cancellation never changes paint. Brush opacity stays independent.
The dialog previews the complete color through the current sRGB widget fallback
and names its definition and document spaces. This does not qualify managed wide
monitor presentation.

Named workspace palettes retain portable color definitions, survive workspace
serialization/database resume, and can be reused in foreground or background in
a different document space. Creation, renaming, deletion and selection validate
before mutation. At least one palette remains; duplicate palette names are rejected
without changing state. Palette rows use ordinary activation and explicit rename/
remove buttons; they do not reorder. Limits of 64 palettes and 4,096 total swatches
are input guards, not measured memory or latency acceptance.

Validation artifacts use `artifacts/color-m2/numeric-palette-` unless stated:

- `shared-suite.log`: 383 shared UI and 86 native workspace checks pass. After the
  final alpha-percent formatting polish, `editor-final.log` reruns both exact
  numeric-model/extended-color checks successfully.
- `reviewed-production.log`: production GTK checks cleanly, without warnings.
- `final-checks.json`: the expanded numeric/palette native journey (7.73 s),
  workspace database resume and independent windows (3.46 s), and real Mutter
  mouse/touch color controls (30.59 s) all pass. The input run retains 88 screenshots
  in `numeric-palette-panel/`.
- `reviewed-build.{json,log}`, `reviewed-sources.json`: final reviewed executable
  `numeric-palette-reviewed-gtk-tests`, SHA-256
  `c09e46fcc3440047cd98c4808fd04ec25e2d986ea79edbc68b92a6b68bf87c8f`.
  `reviewed-dialogs.log` repeats the full dialog journey after the final presentation
  polish: 1 passed in 7.75 s. Durations here describe correctness runs, not latency
  or performance budgets.

The native journey is
`workspace::tests::color_management::native_numeric_colors_and_saved_palettes`.
It uses a ProPhoto U16 document and an exact P3 definition with alpha 123/65535,
cycles all five entry models, checks invalid/cancelled edits and explicit sRGB
hex, creates/renames/deletes palettes and swatches, rejects duplicate names, and
reuses the serialized P3 swatch in another sRGB8 window. It checks document-linear
pigment, exact retained definition, independent brush opacity and unchanged document
revision. The current numeric RGB and saved-swatch screenshots in
`numeric-palette-ui/` were visually reviewed for readable fields, gamut labels and
row actions. The earlier cramped row capture is retained for comparison.

Initial native testing found a real GTK lifetime defect: synchronously rebuilding
a ComboRow model inside its selection notification first caused repeated rebuilds,
then a segmentation fault after adding only a same-selection guard. The final code
replaces models only when palette names/IDs change and coalesces widget refreshes
onto the next owner-loop idle turn. Activating a swatch closes without rebuilding
its active row. The stalled stacks, owner stack, crash information and failed logs
are retained (`stalled-stacks.log`, `stalled-owner.log`, `crash-info.log`,
`native-reviewed-dialogs-harness.log`). Both the expanded and final native journeys
pass after the deferred-refresh fix. Nonfatal portal/secret-service messages in the
private compositor harness do not establish a product defect or a display match.

Reproduce the shared suites and production build using the JPEG pkg-config setup
above, then build/capture the GTK executable and run the fully qualified dialog
test with `gtk-raster.sh` and an exact-test wrapper. Run the existing database-resume
test similarly. The real-input command is:

```sh
LAYER_NATIVE_TEST_EXECUTABLE="$PWD/artifacts/color-m2/numeric-palette-final-gtk-tests" \
LAYER_TEST_ARTIFACTS="$PWD/artifacts/color-m2/numeric-palette-panel" \
  bash tools/performance/workspace-motion.sh gtk --color-panel
```

`numeric-palette-provenance.json` records source and artifact hashes. No renderer,
codec, stored sample or export transform changed in this step; the preceding exact
external-editor handoff evidence still applies. New/Open Photo is the next functional
stage. Managed viewing, photo workflows, combined residency/scheduling, delivery
controls and the full correctness/memory/latency matrix remain incomplete.
Benchmarking and optimization remain last. Other platform host integration still
requires user approval after GTK qualification.


## GTK New, Open Photo and document color details (2026-09-15)

New now offers Standard drawing (sRGB8), Wide color (P3 8-bit), and Photo editing
(ProPhoto integer16). Dimensions, white/transparent background, working space and
integer depth are independent. Color details expand below the compact preset/size
form. Named presets and explicit remembered defaults persist in shared settings;
new GTK windows allocate their native renderer in those same defaults. Cancelling
creation preserves the current document and defaults. Saving/removing a preset is
an explicit settings operation independent of whether a drawing is later created.
The 8,192-pixel creation dimension and 64 saved-preset limits are input guards,
not measured memory or latency qualification.

Open recognizes native projects, PNG, JPEG and TIFF by signature. It keeps decoded
source samples, actual channel layout, depth, exact ICC bytes and transparency.
The source stays attached to a paint layer; local brush edits create native raster
overrides without replacing the retained original. Untagged SDR RGB assumes sRGB.
Known matrix gamuts suggest their corresponding built-in editing space. Other
supported ICC gamuts use ProPhoto working RGB, retaining the original profile and
samples; no arbitrary device ICC profile becomes a painting space. The full source
CMM transform remains authoritative, including when its gamut resembles a built-in
space. No profile-name match, tone-curve substitution or source relabelling occurs.

The profile suggestion was checked against the [ICC PCS/adaptation guidance](https://registry.color.org/rgb-registry/icctransform)
and actual colord 1.4.8-4.fc44 profiles. Direct comparisons of D50 matrix columns
missed colord's sRGB/Adobe variants: their recovered native white differs from the
built-in rounded D65 white by about 1.2e-4 in y. The final suggestion recovers native
primaries/white using the profile's own inverse chromatic-adaptation matrix and
compares xy within 2e-4. Without that tag, it conservatively compares D50 columns.
This threshold chooses a supported working gamut only; it never declares profiles
identical, bypasses conversion, or relaxes sample precision. The independent sRGB,
Adobe RGB (1998) and ProPhoto profiles now choose their expected working spaces.
Tests also give all four built-in profiles misleading names and verify that names
do not affect suggestions; a linear P3 profile suggests P3 while retaining its
actual source transfer. Original profile data stays unchanged.

File → Document Properties displays canvas dimensions, working profile, integer
precision and retained-source details, including an assumed profile. Profile metadata
is parsed on a worker. Opening a photo supplies a suggested master name with no
native save location and an unpublished-content flag. Save prompts for a separate
`.capy` master, and cancellation/close protection remains active until publication.
The old recovery-only flag is now named for this shared unpublished-content policy.
Native project open still retains its explicit native location. No source path is
stored as native save authority.

Artifacts use `artifacts/color-m2/new-photo-`:

- `shared-suite.log`: 385 shared UI tests pass, including all eight creation
  space/depth combinations with white/transparent backgrounds and exact native
  archive round trips. `shared-document-final.log` reruns 14 document-policy
  checks after the unpublished-state refactor; all pass.
- `workspace-suite.log`: all 86 native workspace database checks pass.
- `colord-chromaticities.log`: both profile-suggestion tests pass, including the
  independent installed profiles, retained in `colord-profiles/` for reproduction.
  Earlier failing direct/adapted-column comparisons remain in their logs.
- `reviewed-reader.log`: PNG sources at both depths in all four working gamuts
  retain exact samples/profile data through Open and master serialization despite
  a misleading `.capy` filename; an untagged JPEG stays sRGB8 and records its
  assumption. The source file is unchanged. This check passes in 0.36 s.
- `shared-ffi.log` and `footer-production.log`: shared FFI and production GTK
  check without warnings. No other platform host was integrated or built.

The complete native journey
`workspace::tests::new_photo::native_new_presets_and_profiled_photo_master` passes
on the final `footer-gtk-tests` executable in 13.10 s (`footer-journey.log`). Its
SHA-256 is `506928ce8a81c8b0bbce5b067316fa2e074996624d186dc43a0bbc75f238f953`;
`footer-build.{json,log}` and `footer-sources.json` identify the captured code/build.
It creates/saves/reuses/removes a P3 preset, exercises independent Adobe RGB8
controls, cancels creation, and paints in a fresh P3 window using remembered defaults.
It opens a 513×257 ProPhoto U16 TIFF through the actual file dialog, inspects color
properties, paints across a tile boundary, saves a separate master and reopens it
in another real window. The canonical project bytes and displayed composite are
exact across reopening. It exports a ProPhoto U16 TIFF while retaining the source
file byte-for-byte and keeping the master clean.

The reviewed production implementation also passes the existing native file
workflow (36.52 s) and wide-color diagnostics/GPU-failure recovery (6.21 s), recorded
in `existing-checks.json`. Those ran on `gated-gtk-tests`, SHA-256
`2c103cc80c8e42da1cc518e35814f136335919f2fdab5461cb2db9a2f650b57c`.
The later build changes only the New form's footer placement, tests and formatting.
The full New/Open journey passes again after that final presentation change.
All durations in this section describe correctness runs, not performance acceptance.

Initial native checks caught fixture and presentation issues. RasterRevision equality
represents publication identity, so the first reopened-project assertion was replaced
with complete canonical archive-byte comparison. A later synthetic contact produced
no committed stroke with the fixture's renderer-only readiness check; it now waits
for workspace/input ownership and dialog dismissal and explicitly selects Pen before
painting. Both subsequent complete runs pass. Expanded Color initially clipped the
remembered-default checkbox at the scroll boundary; preset/default controls now sit
outside the scrolling fields. The final compact P3/ProPhoto, expanded Adobe RGB8 and
source-properties screenshots in `ui/` were visually inspected. These are layout and
sRGB-fallback checks, not calibrated monitor-color qualification.

ImageMagick independently verifies all 983,040 U16 samples and ICC bytes from the
existing file test's RGB TIFF, grayscale PNG and CMYK TIFF: each maximum code
difference is zero (`external-handoff.log`, GTK process 1736637). The CMYK input is
`/usr/share/color/icc/krita/cmyk.icm`; output/source profile hashes are recorded.
Nonfatal private-session GVFS warnings are retained in the harness logs.

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-ui --offline -- --test-threads=1
cargo test -p layer-workspace --features native --offline -- --test-threads=1
LAYER_TEST_WORKING_PROFILES="$PWD/artifacts/color-m2/new-photo-colord-profiles" \
  cargo test -p layer-color --offline icc::description:: -- --include-ignored --nocapture
cargo test -p layer-linux --offline files::open::tests:: -- --test-threads=1
cargo test -p layer-linux --offline --no-run --message-format=json
cargo check -p layer-linux -p layer-ffi --offline
```

Capture the resulting GTK binary, then run one fully qualified native test per
process with `gtk-raster.sh` and an exact wrapper. Set `LAYER_TEST_CMYK_PROFILE`
for the existing file workflow. Reproduce the external handoff with:

```sh
python3 tools/validation/icc_export_handoff.py artifacts/familiar-workspace/files 1736637
```

`new-photo-provenance.json` records source, build, profile, screenshot and output
hashes. Profile-preserving Place/Paste, source repair/rasterization, document
assignment/conversion/depth, photo inspection, Color preferences and remaining
output controls are still open. So are managed wide viewing, global cancellation
and combined source/composite/history/staging budgets. Final fresh baselines,
frame-creation regressions/optimization and the hardware memory/latency matrix
remain last. This local functional milestone does not claim those gates pass.
Other platform hosts remain approval-gated after GTK completion.

## GTK retained Place/Paste and source history (2026-09-15)

GTK's layer import button, File → Import Image as Layer (`Ctrl+Shift+O`) and
Edit → Paste Image as Layer (`Ctrl+V`) now use the shared retained-source route.
The superseded GTK texture download through untagged RGBA8 was deleted. PNG,
JPEG and TIFF keep original samples, profile interpretation, depth and alpha;
placing into an sRGB8 document does not first quantize a P3 U16 source to sRGB8.
Source conversion remains in the document-native working tile decoder. The shared
command policy reserves the file operation, validates the layer name/target and
renderer capability before publication, and creates one undoable layer edit.

Clipboard reading requests TIFF, PNG, then JPEG. This follows GDK's documented
[ordered MIME preference](https://docs.gtk.org/gdk4/method.Clipboard.read_async.html),
checked against the native clipboard test below. Transfer uses 64 KiB asynchronous
buffers and a private temporary file capped at 512 MiB. The file is removed on
success, error or cancellation. Decode runs on a worker, checks cancellation at
reader/seek boundaries and keeps the request reserved until the worker exits.
This is a transfer limit, not a combined process/decode memory qualification.
Decoder-internal cancellation granularity and cross-window scheduling remain open.
Publication rejects a changed document epoch, revision or editing target.

Retained sources exposed two missing existing layer behaviors: Clear now removes
the source as well as raster/asset content, and transform eligibility/bounds now
include retained image content. Undo restores the exact original source. This
stage validates transform begin/cancel; applied resampling and large off-canvas
sources still belong to the complete tool qualification matrix.

Artifacts use `artifacts/color-m2/place-source-*` on the Linux/NVIDIA/private-Mutter
hardware setup recorded above:

- `shared-initial.log`: the source policy test passes, covering unsupported-renderer
  and invalid-name atomicity, P3 U16 hidden/low-alpha sample retention into sRGB8,
  transform cancellation, Clear/Undo/Redo, single-step import history and exact
  archive restoration.
- `shared-suite.log`: all 386 shared UI checks pass (30.65 s), including shortcut
  editing guards and command publication. `final-check.log` checks production GTK
  and shared FFI without warnings; no other platform host was integrated.
- `reviewed-journey.log`: the real native
  `workspace::tests::place_source::native_profiled_place_paste_and_source_history`
  passes in 5.57 s. It imports a P3 U16 PNG into sRGB8, checks exact retained source
  data, cancels a transform, clears/restores the displayed content and source,
  undoes/redoes import, and pastes a clipboard offering a black RGBA8 PNG before
  the richer TIFF. TIFF wins and retains exact U16/profile data. Saving to native
  archive and reopening in a second window preserves both sources and exact
  displayed pixels. Cancelling a pending 32 MiB transfer and rejecting ordinary
  text leave canonical document bytes unchanged.

The first native journey caught a real cancellation lifecycle bug: Cancel already
closed the Adwaita dialog, and completion closed it again, triggering a fatal
critical. Completion now returns directly after the cancellation response. The
failing `native-journey.log` is retained; the corrected full journey passes with
fatal GTK criticals enabled. Nonfatal private-session GVFS warnings are retained.
The initial production check also caught an unavailable optional GIO async-temp
API; private temp creation now establishes its unlink guard synchronously before
asynchronous payload I/O begins.

The passing captured binary is `reviewed-gtk-tests`, SHA-256
`27834baac13e0107709f0e11ccd79de0a7a917bfe1c49ce254d62be1d7c7420c`.
`reviewed-build.{json,log}`, `reviewed-sources.json` and `provenance.json` identify
the code and outputs. Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-ui --offline -- --test-threads=1
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/place-source-reviewed-gtk-exact" \
  workspace::tests::place_source::native_profiled_place_paste_and_source_history \
  "$PWD/artifacts/color-m2/place-source-reviewed-journey"
```

The exact wrapper runs the captured test binary with `--exact`; each native test
uses a separate process. Durations here are correctness runs, not performance
acceptance. Source repair/rasterization, document assignment/conversion/depth,
inspection, Color preferences, remaining output controls, managed wide viewing
and combined memory/job bounds remain open. Fresh performance baselines,
regression investigation and optimization remain last. Other host integration
still requires approval after GTK completion.

## GTK source-profile repair and shared ICC chooser (2026-09-15)

File → Repair Source Profile and the retained layer's context menu now correct
its original color interpretation. Shared Rust enforces idle/lock/source identity,
unchanged extent/channels/depth and reuse of every immutable source sample tile.
Untouched sources change interpretation in one undoable edit. A source with baked
raster/transform/mask-application content instead offers **Add Corrected Source**:
a plain corrected original at the same placement and parent, leaving the existing
layer's pixels, masks and adjustments intact. The new layer is selected and has
its own normal paint-layer properties. This does not reconstruct old strokes.

The GTK ICC chooser moved from the export-only module to `files/profile.rs` and
now validates the role: delivery compiles/probes an actual output transform;
source repair checks actual RGB/gray/CMYK channels and compiles the input transform
into working RGB. File parsing/CMM setup remain on a worker. Original selected
ICC bytes are retained. Builtin source choices also receive worker validation
before publication. Profile selection and cancellation do not mutate the project;
publication rejects a changed document epoch/revision/source. Assumed metadata is
cleared only by an explicit successful interpretation choice.

Artifacts use `artifacts/color-m2/source-repair-*` with the hardware/software
setup recorded above:

- `shared-suite.log`: all 387 shared UI checks pass (30.35 s). The new policy
  test checks unchanged U16/low-alpha sample ownership, invalid/stale source
  rejection, exact undo, source insertion beside an unchanged baked layer/mask,
  single-step undo/redo and exact native archive restoration. `host-policy.log`
  passes again after adding the layer-menu host-notification regression assertion.
- `profile-roles-final.log`: both profile-reader checks pass. They retain RGB and
  gray ICC bytes, reject corrupt/oversized profiles, and validate/reject source
  RGB, gray-alpha and CMYK channel combinations using the actual source role.
- `production-final.log`: production GTK and shared FFI check without warnings.
  No other platform host was built or integrated.
- `final-journey.log`: the real native
  `workspace::tests::source_repair::native_source_profile_repair_preserves_originals_and_baked_edits`
  passes in 7.85 s. It exercises both command routes, Cancel, rejection of a gray
  ICC for an RGB photo, applying an Adobe RGB ICC, exact original tile sharing,
  changed display appearance and exact Undo display restoration. It paints on
  that source, cancels a second repair, adds a ProPhoto corrected original beside
  the unchanged baked layer, saves/reopens in a fresh window with identical native
  archive/display bytes, then undoes/redoes insertion without changing baked data.

The final captured binary is `final-gtk-tests`, SHA-256
`646f9a37f31299497391e2c86ab7c9195a431e4b55e4ea3db9e2009107538688`;
`final-build.{json,log}` and `final-sources.json` identify its inputs. The earlier
complete `serviced-journey.log` also passes (7.22 s). The first native run failed
because the layer action queued a host request without publishing the HOST region;
that integration bug is fixed and covered by the shared regression assertion.
Initial compile logs and an invalid test-only ID allocator fixture remain recorded;
the fixture now allocates its mask ID normally.

Visual review of the original, mismatch and baked-source sheets prompted a wider
layout and an explicit RGB-profile mismatch message. Final screenshots in `ui/`
were inspected. They verify presentation in the declared sRGB fallback, not
calibrated physical color agreement. A Before/After source-repair preview is still
outstanding; this section qualifies source ownership, history and the repair action,
not the complete consequential-color-change preview journey.

`existing-files.log` passes the existing native Open/Save/Export/failure/cancellation
and recovery workflow (36.04 s) on `serviced-gtk-tests`, SHA-256
`6e309ce7bbf35aaac7234150b56ce7516e4a5e47c0d3458f976aa4169159db35`.
The later changes only widen the source sheet and clarify source-only validation
messages. Export uses the same validated output-role behavior. ImageMagick
independently checks all 983,040 U16 RGB/gray/CMYK samples and matching ICC bytes
from GTK process 1772689; every maximum code difference is zero
(`external-handoff.log`). CMYK uses `/usr/share/color/icc/krita/cmyk.icm`.
Nonfatal private-session GVFS warnings remain in the logs.

Reproduce using the JPEG pkg-config setup above:

```sh
cargo test -p layer-ui --offline -- --test-threads=1
cargo test -p layer-linux --offline files::profile:: -- --test-threads=1
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/source-repair-final-gtk-exact" \
  workspace::tests::source_repair::native_source_profile_repair_preserves_originals_and_baked_edits \
  "$PWD/artifacts/color-m2/source-repair-final-journey"
python3 tools/validation/icc_export_handoff.py artifacts/familiar-workspace/files 1772689
```

`source-repair-provenance.json` records code/build/profile/output/screenshot hashes.
Source-repair Before/After, explicit rasterization, document assignment/conversion/
depth, inspection, Color preferences and remaining output controls are still open.
Managed wide viewing, complete tool precision and combined memory/job budgets also
remain open. These correctness durations are not benchmark acceptance. Fresh
baselines, frame-creation regressions, optimization and the memory/latency matrix
remain last; other host integration remains approval-gated after GTK completion.

## Complete-stack Before/After for source repair (2026-09-15)

Source repair now previews the complete original and candidate canvases before
Apply. The shared preview builder uses the same layer edit as publication,
including insertion of a corrected original above a baked layer. Preview owns a
cloned document and provisional IDs; it changes neither live history nor allocator
state. This closes the source-repair Before/After gap recorded immediately above.
The complete conversion/depth workflows remain separate unfinished work.

The GTK comparison component renders immutable snapshots on a worker through the
existing exact Float32 capture path. It includes every composed source pixel,
then performs area reduction in linear premultiplied working RGB. Fractional
edges use integral area weights; transparency is averaged with coverage rather
than darkening straight colors. The final at-most-220×160 images use an explicit
sRGB display encoding matching the current GTK fallback. They never feed document
edits, exact sampling or export. Managed wide preview encoding remains outstanding.

Capture consumes 16-row strips and the existing explicit snapshot dependency
ceiling. The thumbnail itself needs at most 35,200 accumulation pixels; there is
no full-resolution host preview bitmap. The component holds one active worker and
one replaceable request. A changed choice cancels stale work; its successor starts
only after the worker returns. Cancelling the dialog waits for that acknowledgement
before releasing the document request. Apply is enabled only for a successful
preview of the current choice. This bounds this component's queue; cross-window
job scheduling, capture cancellation while awaiting other raster publications,
combined CPU/GPU budgets and large-photo preview latency still need qualification.

Artifacts use `artifacts/color-m2/color-preview-*` on the setup recorded above:

- `shared-plan.log` passes the source-policy check with new assertions that preview
  leaves the live document/counters untouched and exactly matches Apply for both
  untouched and baked sources, while preserving prior undo/archive checks.
- `area.log` passes analytical checks for odd-size fractional reduction, strip
  boundaries, identity and premultiplied transparency. Extended positive/negative
  working values are retained until display encoding.
- `production.log` checks production GTK and shared FFI without warnings.
- `initial-journey.log` passes the full native repair journey with preview (14.01 s).
  `reviewed-journey.log` additionally exercises rapid queued profile changes and
  cancellation of an active preview (14.87 s). `final-journey.log` passes again
  with per-process artifact directories (14.96 s, GTK process 1788906).
  All preserve exact source/baked data, Undo/Redo, Cancel and native reopen/display
  checks from the previous repair milestone.

The final binary `final-gtk-tests` has SHA-256
`4317a40808a7517bace01f7ea7e52d62570c5d0555ddd9889c602f07e374fab6`.
`final-build.{json,log}` and `final-sources.json` identify its inputs. Final UI and
ICC fixtures are in `ui/1788906/`; Before/After for a baked stroke visibly shows
that the new corrected source covers it while the old edited layer remains
intact. The layout and comparison captures were visually reviewed. These are
sRGB fallback/layout checks, not calibrated physical display qualification.

Early preview runs reused the preceding test's screenshot directory. Those new
captures were copied to `color-preview-reviewed-ui/`, and the previous captured
`source-repair-final-gtk-tests` binary regenerated its own sheets in a passing
7.61 s run (`source-repair-restored-ui-journey.log`). Regenerated image hashes are
recorded separately in `source-repair-restored-ui-provenance.json`; prior screenshot
hashes describe the superseded original captures. Profile fixtures match their
original hashes. New preview captures use a process-specific directory to avoid
replacing earlier validation evidence.

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-ui --offline source_profile_repair_preserves_samples_and_baked_edits
cargo test -p layer-linux --offline files::preview::tests::
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/color-preview-final-gtk-exact" \
  workspace::tests::source_repair::native_source_profile_repair_preserves_originals_and_baked_edits \
  "$PWD/artifacts/color-m2/color-preview-final-journey"
```

`color-preview-provenance.json` records code/build/output hashes. No performance
workloads were run: durations above only identify correctness runs. Explicit
rasterization, document assignment/conversion/depth, histogram/clipping inspection,
Color preferences, remaining output controls, managed wide viewing and the full
precision/memory/job gates remain open. Fresh baselines, regression investigation
and optimization remain last; other host integration remains approval-gated.

## Explicit GTK source rasterization (2026-09-15)

Edit → Rasterize Source and the layer context action convert an original image
into RGBA samples in the document's built-in color space and integer depth. This
is a worker conversion of the full tiled image, including content outside the
canvas. Existing paint overrides, scalar planes, masks, layer properties and
placement remain unchanged. The complete Before/After canvas uses the same
candidate as Apply. Cancel waits for conversion/preview acknowledgement and
leaves the document unchanged; Apply is one layer replacement in history.

Current code required an explicit distinction between an original and a committed
image base. Both use the existing immutable tiled-image storage; `SourceKind`
records Original or Rasterized. A rasterized base must have RGBA channels, a
non-assumed built-in profile and the document's depth/space. Source profile repair
is offered only for originals. Rasterized bases will participate in document
Assign/Convert/depth operations; those operations are still outstanding. The
native container is version 4 with a required image role, validated before payload
reading. Earlier versions are deliberately rejected; no migration reader was
added. Other host integration and their old archive assertions remain an approval
gap. The project-format reference now describes current code rather than its
former version-1 design.

The conversion reuses exact samples/Arc tile ownership when profile, RGBA channels
and depth already match. Other conversions stream through the existing CMM and
integer encoder in row/tile bands, checking cancellation each row. Conversion
reports clipped channels before Apply. Its 512 MiB compressed-output ceiling is
an individual component limit, not a qualified combined process budget. No source
is downsampled or precision-reduced to relieve memory pressure.

Review of the first passing native screenshot exposed a pre-existing composition
bug: translated source coordinates were clamped to canvas dimensions, hiding the
part of a larger image brought into view. Composition now considers the full
source extent. The native check explicitly samples that translated photo region;
reviewed Before/After now visibly includes the photo and independent paint layer.
This corrects source presentation for Move; it does not qualify every applied
resampling/transform or off-canvas painting path.

Artifacts use `artifacts/color-m2/rasterize-*` on the setup recorded above:

- `conversion.log` passes both conversion tests (1.51 s): all 65,536 U16 codes,
  U8 identity, exact shared ownership, U8→U16 ×257 and nearest U16→U8 in all four
  spaces, alpha/hidden RGB, gamut clipping, cancellation and allocation rejection.
- `shared-policy.log` passes the candidate/Apply/history check (0.06 s), including
  a 1500-pixel original wider than its canvas, fractional offset, existing paint,
  mask ownership and command/host notification. `native-storage-reviewed.log`
  passes seven archive checks (1.98 s), including role roundtrip, rejection of
  mismatched interpretation and deliberate version-3 rejection.
- `core-color-suite.log` passes 63 core and 35 color tests (2.79/12.58 s); four
  separately invoked/fixture-dependent color tests are ignored by that suite.
  `workspace-suite.log` passes 86 tests (9.38 s). `shared-suite.log` passes 387
  tests with one command-copy length failure: shortening the command to
  “Rasterize Source…” fixes it; `command-copy.log` passes that check (0.29 s).
- `reviewed-journey.log` and `final-journey.log` precede the visibility correction.
  `placement-journey.log` passes the corrected native journey (11.93 s): retained
  P3 ICC U16 → sRGB8, real pen paint, moving the larger source, independent overlay,
  Cancel, clipping preview, Apply, exact archive/display reopen, Undo/Redo and
  continued source-layer painting. UI capture `ui/1816502/source-before-after.png`
  was visually reviewed; earlier per-process captures remain as evidence.
- `existing-files.log` passes native file/ICC export/failure/cancellation/recovery
  checks (35.84 s) using `final-gtk-tests`, SHA-256
  `855182c5bb8ad43a04f86f2b061eff2f4fc0dfa4a5498ee2ac1e7d4d865b32ac`.
  ImageMagick independently decodes all 983,040 U16 RGB/gray/CMYK output samples
  from process 1814212 with zero differences and matching ICC bytes
  (`external-handoff.log`). The later composition fix changes only moved-source
  gathering, not the archive or output encoders.
- `gpu-recovery.log` passes the wide-color native diagnostics/failure/recovery
  journey (11.90 s) on the corrected build. `production.log` checks GTK and shared
  FFI without warnings (4.07 s).
- `eight-modes.log` passes real painting, exact Undo/Redo and native archive/window
  reopen in all four spaces at both depths (31.02 s) on the corrected build.

The corrected native binary is `placement-gtk-tests`, SHA-256
`0334b381156db0c58490026040aa18bf9d538948ed12f12e5ece372c60b9cc2a`.
`placement-build.{json,log}` and `placement-sources.json` identify its inputs.
The captured comparison is an sRGB fallback/layout check, not physical wide-color
calibration. Private-session nonfatal GVFS warnings remain in the logs.

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-core -p layer-color --offline -- --test-threads=1
cargo test -p layer-ui --offline rasterizing_an_image_preserves_full_extent_edits_masks_and_history
cargo test -p layer-ui --offline settings_copy_is_short_on_every_platform
cargo test -p layer-workspace --features native --offline -- --test-threads=1
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/rasterize-placement-gtk-exact" \
  workspace::tests::source_rasterize::native_rasterization_keeps_off_canvas_source_paint_mask_and_reopen \
  "$PWD/artifacts/color-m2/rasterize-placement-journey"
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/rasterize-final-gtk-exact" \
  workspace::tests::native_document_files \
  "$PWD/artifacts/color-m2/rasterize-existing-files"
python3 tools/validation/icc_export_handoff.py artifacts/familiar-workspace/files 1814212
```

`rasterize-provenance.json` records source/build/artifact hashes. Combined source,
paint, history, staging and worker budgets still require admission and measurement:
current history trimming can drop an oversized newest entry, and aggregate placed
sources can exceed native save limits. These large-operation boundaries must be
resolved before claiming guaranteed Undo or complete large-photo acceptance.
Document Assign/Convert/depth, inspection, Color preferences, managed wide viewing,
remaining export controls and full precision/job qualification also remain open.
These correctness durations are not benchmark acceptance. Fresh baselines,
frame-creation regressions, optimization and the memory/latency matrix remain last;
other host integration requires approval after GTK completion and qualification.

## Admission for retained-source edits (2026-09-15)

The source/history boundary identified above now rejects source edits that cannot
retain their newest Undo and Redo states within the existing 512 MiB history
allowance. Admission builds an immutable candidate, its inverse and the canonical
Redo before changing the live document, checkpoint or history. Older history may
still be evicted normally. The same ownership accounting serves admission and
trimming; current-document mask ownership now includes pending operation masks.
Equal bytes in independent allocations are charged independently; shared source
images, tile backing and ICC profiles are counted by ownership.

Import/Place/Paste, source-profile repair and rasterization validate candidate
projects against aggregate retained-source limits before publication. Profile
repair and rasterization use this same admitted candidate for complete previews.
New layer IDs are provisional until admission succeeds, so rejected imports or
corrected-source insertions do not consume allocator state. These checks cover
source ownership and the existing project validator. They do not replace the
remaining combined raster/index/capture/process budget work.

The first attempt applied strict admission to all edits. The existing linked-mask
transform test correctly rejected that change: two pending roots were each
reserved at a full 256 MiB before their producer supplied shared tile identities.
Strict publication admission now applies to retained-source ownership changes;
ordinary drawing and linked-mask transforms retain their existing capture path.
General pending-capture admission, history residency across all navigation states,
active-operation pins and combined CPU/GPU accounting remain open. This is a
source-workflow correctness fix, not a claim that every memory gate is closed.

Artifacts use `artifacts/color-m2/source-admission-*`:

- `core-suite.log` passes the initial 65 core tests (2.66 s).
  `shared-suite.log` preserves the linked-mask failure described above.
  `reviewed-shared-suite.log` passes 65 core, 55 engine and 389 shared UI tests
  (2.43/0.85/30.30 s), including the existing linked-mask transform journey.
- New core tests use small explicit budgets to reject adding/removing an
  independently owned source or ICC profile while preserving exact document,
  allocator/checkpoint state, Undo/Redo availability and original tile ownership.
  A shared test rejects a second independently allocated image before mutation,
  accepts a second layer sharing its tile backing, and rejects a profile change
  that exceeds the aggregate source allowance. These are admission correctness
  tests; they do not measure large-photo peak memory.
- `native-workspace.log` passes 86 tests (9.00 s) for the GTK-used native workspace feature. The initial
  `workspace.log` without that feature runs only the portable migration test;
  reproduction commands now explicitly enable `--features native`.
- `repair.log` passes complete native source repair (16.81 s), and `rasterize.log`
  passes full-extent native rasterization (8.52 s). Both retain complete previews,
  exact cancellation/history, original/baked ownership and native reopening.
  `place.log` passes profiled file import and rich clipboard paste with admission
  (5.33 s).
  Captures and file fixtures remain process-specific, preserving prior evidence.
- `production.log` checks production GTK and shared FFI without warnings (4.52 s).

The captured native binary is `native-gtk-tests`, SHA-256
`4eecbee8ddb0e436bbcece8999a2b7f3d875dbede84d14abc253e8d8baaea57d`;
`native-build.{json,log}` and `native-sources.json` record its inputs. A subsequent
core comment clarification does not change the binary's behavior.

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-core -p layer-engine -p layer-ui --offline -- --test-threads=1
cargo test -p layer-workspace --features native --offline -- --test-threads=1
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/source-admission-native-gtk-exact" \
  workspace::tests::source_repair::native_source_profile_repair_preserves_originals_and_baked_edits \
  "$PWD/artifacts/color-m2/source-admission-repair"
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/source-admission-native-gtk-exact" \
  workspace::tests::source_rasterize::native_rasterization_keeps_off_canvas_source_paint_mask_and_reopen \
  "$PWD/artifacts/color-m2/source-admission-rasterize"
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/source-admission-native-gtk-exact" \
  workspace::tests::place_source::native_profiled_place_paste_and_source_history \
  "$PWD/artifacts/color-m2/source-admission-place"
```

`source-admission-provenance.json` records code/build/output hashes. No benchmark
workloads were run. Document Assign/Convert/depth, remaining inspection/settings/
output UI, managed wide viewing and the remaining precision/memory/job gates are
still unfinished. Fresh baselines, frame-creation regression investigation,
optimization and memory/latency qualification remain last. Other platform hosts
remain approval-gated after GTK completion.

## Canonical straight sRGB8 paint (2026-09-15)

GTK sRGB8 paint now uses the same straight, profile-encoded integer backing as
all other native SDR modes. This closes the default-paint descriptor prerequisite
recorded above: `DocumentColor::paint_descriptor()` has no default-mode alpha or
encoding exception. Working math remains Float32 linear premultiplied RGB; output,
source preservation and artistic processing domains are unchanged.

The shared normalized-attachment capture path explicitly labels its own sRGB8
premultiplied representation. Shared archive validation accepts that layout only
for sRGB8 color planes, while all native modes use the canonical descriptor.
Restoration checks the actual representation: an unintegrated normalized renderer
rejects native straight tiles before modifying its live pages. No other platform
host was integrated or built. The native v4 container already stores each tile's
full descriptor; this change requires no new container structure or migration.

The analytical transfer reference was independently checked against W3C's
[CSS Color 4 sRGB conversion example](https://www.w3.org/TR/2026/CRD-css-color-4-20260913/#color-conversion-code).
A new native publication check covers every 8-bit RGB code at every nonzero
coverage code, verifies premultiplied working values against an independent f64
calculation within 2e-7, and preserves the stored codes exactly over 16 repeated
publications. Alpha-zero paint uses canonical black; immutable image sources
retain their independent hidden RGB samples.

Evidence under `artifacts/color-m2/straight-srgb-*`:

- `shared-suite.log`: 35 color, 65 core, 55 engine and 389 UI tests pass
  (14.14/2.70/0.84/32.50 s); four fixture-dependent/explicit color tests remain
  ignored. Core archive checks include every code and scalar plane in all modes.
- `native-publication.log`: five GPU tests pass (51.84 s), including the new
  code/coverage grid, all eight SDR paint/Undo/save/reopen/continued-paint/device
  replacement workflows, multi-chunk color/mask canonicalization and invalid or
  abandoned publication recovery.
- `gtk-modes.log`: the real GTK eight-mode pen/edit/Undo/native-window reopen
  journey passes (39.08 s). `files.log`: GTK save, profile output, file failure,
  cancellation and recovery pass (36.08 s), with the installed CMYK fixture.
- `external-handoff.log`: ImageMagick independently reproduces all 983,040 U16
  RGB/gray/CMYK output samples and embedded profiles from process 1848963 exactly.
- `production.log`: GTK and shared FFI check without warnings (3.19 s).
- `gtk-recovery.log`: default-mode GTK diagnostics and intentional GPU failure
  recovery pass (3.32 s) with the canonical sRGB8 backing.
- `attachment-regression.log` selected zero tests because of an incorrect module
  filter; it is not passing coverage. `attachment-reviewed.log` uses the actual
  `layer_tests::raster::` module: all six tests pass (13.08 s), including the new
  explicit native-layout rejection assertion in the existing failed-restore test.

The captured `native-gpu-tests` hash is
`5b69bbfa45395cbaf1e5fa2e596cefabb1e6d7af33c0254e6a062e82b8a7804c`;
`native-gtk-tests` is
`760ebb22f3f538b623e1752d45af3efac4b830ce6f81b42dde3a939b62b12d45`.
`native-build.{json,log}` and `native-sources.json` identify their inputs. The
reviewed GPU binary adds only the attachment rejection assertion and formats the
new code/coverage test; its hash is
`a468e1c8b2e8f587e76c953949781a6ad9f083aa6f6a06c576a7bbe12d7238fa`,
with `reviewed-gpu-build.{json,log}` and `reviewed-sources.json` provenance.

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-core -p layer-color -p layer-engine -p layer-ui --offline -- --test-threads=1
cargo test -p layer-render-wgpu -p layer-linux --offline --no-run --message-format=json
artifacts/color-m2/straight-srgb-native-gpu-tests raster::native_edit::tests --test-threads=1
artifacts/color-m2/straight-srgb-reviewed-gpu-tests layer_tests::raster:: --test-threads=1
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/straight-srgb-native-gtk-exact" \
  workspace::tests::color_management::native_sdr_document_modes \
  "$PWD/artifacts/color-m2/straight-srgb-gtk-modes"
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/straight-srgb-native-gtk-exact" \
  workspace::tests::native_document_files \
  "$PWD/artifacts/color-m2/straight-srgb-files"
python3 tools/validation/icc_export_handoff.py artifacts/familiar-workspace/files 1848963
```

`straight-srgb-provenance.json` records source/build/output hashes. No performance
workloads were run; these durations identify correctness checks only. Document
assignment/conversion/depth, managed wide viewing, remaining inspection/settings/
output controls and full precision/memory/job gates remain open. Fresh benchmarks,
regression investigation and optimization remain last; other hosts require approval.

## Document color preparation and atomic model history (2026-09-15)

The shared document worker now prepares Assign, Convert and Bit Depth candidates
without mutating the live project. Assign shares canonical RGB backing exactly;
Convert uses the existing native Float32/CMM output path with intent/BPC; depth
changes preserve scalar meaning and optionally dither 8-bit RGB, never coverage.
Rasterized image bases keep their full extent, including outside the canvas.
Retained originals keep their independent samples, ICC interpretation and Arc
ownership; source-profile repair remains a separate operation. Existing effect
color parameters are explicitly sRGB in current code and are not reinterpreted.

These semantics were checked against current code and Adobe's
[Assign/Convert documentation](https://helpx.adobe.com/photoshop/desktop/adjust-color/color-profiles/change-color-profile-for-documents.html),
which distinguishes retaining numeric values from transforming them. The
[ICC explanation of profile connections](https://www.color.org/iccmax/connection2/)
describes absolute intent's white-point adjustment. This worker reuses the
previously qualified CMM implementation; its absolute-intent routing check proves
that the selection affects the prepared data, not independent CMM accuracy.

`Edit::SetColor` publishes mode and completed layer backing in one model edit.
It preserves layer properties, masks, sparse tile coverage, watercolor state,
source roles and full extents, and rejects replacement of an original. Both
history directions undergo existing admission before mutation. Preparation polls
pending backing with cancellation and charges new compressed backing/index/cache
ownership against a caller-supplied component limit. This is not a claim that
combined source/history/worker/GPU budgets have been qualified.

The engine now rejects color edits, nested batches and color Undo/Redo when its
renderer is configured for another mode, before consuming input. **GTK renderer
transition, dialogs, complete-stack comparison and flattened-copy integration
are still pending. These operations are not exposed through GTK commands yet.**

Evidence in `artifacts/color-m2/document-color-*`:

- `worker-tests.log`: initial six tests pass (15.45 s).
- `shared-suite.log`: 41 color, 65 core, 56 engine and 389 UI tests pass
  (11.59/1.44/0.42/18.42 s); four explicit/fixture color tests remain ignored.
  Includes mismatch rejection without input consumption or history mutation.
- `worker-reviewed-tests.log`: all eight final worker tests pass (15.11 s).
  Assignment covers every code at both depths in all four spaces. Depth changes
  check all 65,536 U16 codes, RGBA, masks and both wetness planes, exact U8×257
  promotion and nearest U16→U8 reduction. Conversion compares each RGB sample
  against the f64 coordinate path within one code, with exact alpha and clipping
  counts. The f64 path shares the standard space definitions; it is not an
  independent set of colorimetric coefficients. Further checks cover stable
  coordinate dithering, backing deduplication, full off-canvas sources, exact
  Undo/Redo/checkpoints, cancellation during work or pending publication,
  memory-limit rejection and malformed candidates. The two added tests check
  absolute intent and canonicalization of explicitly tagged attachment input.
- `production-check.log`: GTK and shared FFI compile without warnings (4.37 s).

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-core -p layer-engine -p layer-color -p layer-ui --offline
cargo test -p layer-color --offline document::tests -- --test-threads=1
cargo check -p layer-linux -p layer-ffi --offline
```

`document-color-provenance.json` records parent/source/output hashes. This stage
runs correctness tests only. Fresh performance baselines, frame-creation
regression investigation, optimization and final memory/latency qualification
remain last; other platform integration still requires approval after GTK.

## Prepared renderer/document color transitions (2026-09-15)

The model can now prepare Apply, Undo or Redo as an immutable color transition.
Its candidate is available before publication for GPU preparation and comparison.
Commit rechecks the exact document and history state, calls the host's final
fallible resource-adoption step, then publishes the matching model/history state.
Failure or stale preparation leaves the document, checkpoint and history intact.
Ordinary drawing and ordinary history navigation retain their existing paths.

The engine requires idle input and a prepared renderer configuration, clears
obsolete stroke-correction/display restoration state after success, and updates
the dab generator's space. Tool/background coordinates retain their appearance;
their conversion is validated before resource adoption, including finite-range
failure. The generic preview path now has the same color guard as Apply/history.
`CanvasRenderer::adopt_prepared_color` defaults to refusing a different mode;
no other host acquires color-transition support implicitly.

This completes the shared transition mechanism, **not the GTK journey**. GTK's
asynchronous configuration preparation, cancellation acknowledgement, color
dialogs and history request routing remain to be connected. No new color-edit
command is exposed in this stage.

Evidence under `artifacts/color-m2/color-transition-*`:

- `shared-suite.log`: 43 color, 65 core, 57 engine and 389 UI tests pass
  (12.23/0.90/0.39/18.69 s); four explicit/fixture color tests remain ignored.
  The sample-bearing worker history test now uses prepared transitions for
  Assign, Convert, reduction and repeated exact Undo/Redo.
- `reviewed-engine-tests.log`: all eight selected color tests pass (0.49 s),
  including the final overflow and queued-input test. The engine refuses absent
  resources, backend failure and stale preparation without changing the live
  mode; a stale candidate does not invoke the resource-adoption callback. Apply
  and three Undo/Redo cycles pair renderer/document modes and checkpoints.
  Unconsumed input and active contacts prevent transition publication.
- `production-check.log`: GTK and shared FFI compile cleanly (8.21 s).

Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-core -p layer-engine -p layer-color -p layer-ui --offline
cargo test -p layer-engine --offline color -- --test-threads=1
cargo check -p layer-linux -p layer-ffi --offline
```

`color-transition-provenance.json` identifies the parent and source/log hashes.
The reviewed engine check follows the full suite's final tool-coordinate
validation and extra failure test. No performance workloads were run. GTK
functional completion still precedes fresh benchmarks and optimization, and
other platform hosts remain approval-gated.

## GTK document profile, conversion and precision workflows (2026-09-15)

GTK now exposes Edit → Assign Profile, Convert Color Space and Change Bit Depth.
Each prepares the actual candidate and a complete-stack Before/After comparison.
Assignment preserves canonical RGB numbers; conversion transforms editable backing
with intent/BPC; precision changes optionally dither 8-bit RGB. Retained originals
keep their own profiles and samples. Conversion can instead create a separate
flattened raster drawing, leaving the original layered document untouched.
Editable-layer conversion can change blending and adjustments; the dialog explains
that distinction before Apply. The preview remains explicitly sRGB while managed
wide viewing is unfinished.

The GTK file-operation reservation covers CPU conversion, comparison, GPU
preparation, publication and cancellation acknowledgement. Settings changes retain
one active conversion plus one replaceable pending choice. The GPU owner prepares
one complete destination renderer on its existing device, keeping the old renderer
until adoption. A reply marker excludes interpretation-dependent responses from
the previous renderer. Undo/Redo follows the same prepared transition when the
working mode changes. Publication also updates picker coordinates while retaining
portable color definitions. A cancelled candidate is destroyed before releasing
the document reservation.

The flattened path streams complete Float32 composition in 16-row strips into a
new document-space raster source. This avoids a full CPU Float32 canvas. Its source
and snapshot limits, and the conversion worker's 512 MiB limit, remain component
limits: combined old/new renderer, source/history/cache/staging and concurrent-job
peak memory has not been qualified. Renderer setup currently prepares complete
resources; its cost and any redundant work belong in the final benchmark phase.

Evidence under `artifacts/color-m2/gtk-color-*`, using the reference NVIDIA/GTK
configuration recorded above and private Mutter at 1600×1000, 120 Hz:

- `final-shared-tests.log`: 390 shared UI and 86 native-workspace tests pass
  (18.53/2.87 s). The added shared test verifies failed-adoption isolation,
  portable picker/brush coordinates, host history routing and exact restoration.
- `visible-production-check.log`: GTK and shared FFI check cleanly (0.86 s).
- `visible-workflow.log`: the complete native document-color journey passes
  (31.19 s), process 1906302. A P3 16-bit fixture includes a retained profiled
  original, real paint, a painted mask and revisable Exposure. It exercises
  Assign → Adobe RGB16, Convert → ProPhoto16, reduction → ProPhoto8, exact
  backing/history and checkpoint restoration, cancellation, save/reopen, a
  separately dirty sRGB flattened copy, and continued painting. Flattened copy
  display agrees within one sRGB code on this fixture. A deliberately failed GPU
  stroke suspends the canvas; restart restores the last recoverable image, and
  prior color Undo/Redo still works. The intentional validation panic in the log
  is part of that assertion.
- `document-color-ui/1906302/` contains the four final dialog captures. Visual
  inspection confirms complete Before/After images, visible profile/result/intent
  choices, separate depth control and the Create Copy action.
- `visible-files.log`: the existing native file journey passes (36.04 s), process
  1907660, covering save/open/export, cancellation/failure and profiled delivery.
  `visible-external-handoff.log` verifies exported 16-bit PNG/TIFF independently
  with ImageMagick/LittleCMS: all 983,040 checked channel codes match exactly.

The initial native workflow passed before adding recovery/picker/visible-label
checks (`workflow.log`, 31.20 s). Review found and fixed picker coordinates left
in the previous working space. The first recovery assertion expected the failed
contact's allocator ID to be reused; it now correctly accounts for that consumed
ID while requiring exact surviving document/history and pixels
(`recovery-reviewed-workflow.log`, 31.27 s). The final control review found blank
selected subtitles when enabling subtitle mode after setting the model. Configure
subtitle mode and the string expression before the model. The failed
`controls-workflow.log` also used the ActionRow subtitle getter; the final test
asserts mapped label text instead, consistent with libadwaita's
[ComboRow subtitle contract](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/property.ComboRow.use-subtitle.html).

The final captured test binary `gtk-color-visible-tests` has SHA-256
`9832bb75ead6b5ed7ac0df5e3f110f358502fbe1902bcf52648e4f959270a3e1`.
`visible-build.{json,log}` and `visible-sources.json` identify its inputs.
Reproduce with the JPEG pkg-config setup above:

```sh
cargo test -p layer-ui -p layer-workspace --features native --offline
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/gtk-color-visible-exact" \
  workspace::tests::document_color::native_document_color_assignment_conversion_depth_history_and_copy \
  "$PWD/artifacts/color-m2/gtk-color-visible-workflow"
LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm \
  bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/gtk-color-visible-exact" \
  workspace::tests::native_document_files \
  "$PWD/artifacts/color-m2/gtk-color-visible-files"
python3 tools/validation/icc_export_handoff.py artifacts/familiar-workspace/files 1907660
```

`gtk-color-provenance.json` records the parent and source/build/output hashes.
These are correctness checks, not performance measurements. Whole-stack advanced
intent coverage, managed-wide canvas/widget/monitor agreement, inspection/Color
preferences, remaining output controls, and combined resource/job limits remain
open. Fresh frame-creation baselines, regression investigation, optimization and
final memory/input-latency qualification remain last. Other platform hosts remain
unintegrated and require approval after GTK completion.

## GTK full-resolution histogram and SDR clipping inspection (2026-09-15)

View → Histogram now opens a nonmodal inspector for the complete committed
composite. It offers combined/individual RGB channels, linear relative luminance
Y and logarithmic count scaling. RGB bins use the document's transfer function;
Y uses its primaries and reference white. Neither changes with the display or an
export profile. All source pixels contribute at full resolution; no thumbnail
averaging feeds the distribution. Fully transparent pixels are excluded. Partial
coverage is unassociated and each remaining pixel counts once. Visible paper and
masks participate; checkerboards, mask-area tint and other display overlays do not.

The inspector distinguishes endpoint counts (≤0 / ≥1) from values outside the
SDR range (<0 / >1). Endpoint occupancy does not establish that detail was lost.
Out-of-range values remain in the endpoint bins and in separately reported counts;
inspection never clamps the artwork. This channel/distribution presentation was
checked against Adobe's [histogram documentation](https://helpx.adobe.com/photoshop/using/viewing-histograms-pixel-values.html);
the alpha policy, linear Y coordinates and full-resolution choice above are
explicit application contracts, not claims of matching another editor's bins.

The shared accumulator uses four fixed 256-bin distributions with u64 counts.
The existing snapshot worker supplies 16-row Float32 strips. One inspector and
one cancellable worker are retained per drawing, including across close/reopen.
Automatic updates wait 300 ms for the committed revision to settle; a 150 ms
native timer coalesces further changes. Stale/cancelled results are not published.
The graph can be paused while editing continues. Animated effects are sampled at
a stated time; pause/resume requests another sample. Inspection does not reserve
file operations or change dirty state, history, samples or profiles.

Evidence under `artifacts/color-m2/histogram-*`:

- `core-reviewed-tests.log`: both accumulator tests pass (0.13 s), covering every
  U8/U16 code in all four spaces, strip-partition identity, exact endpoint counts,
  partial/zero alpha, extended range, invalid data and space-specific luminance.
- `shared-tests.log`: all 67 core tests pass (1.51 s); 389 UI tests pass and the
  expected serialized View-menu fixture initially fails because it predates the
  new command (18.47 s). After updating that expectation,
  `menu-reviewed-test.log` passes the affected test. No production shared code
  changed after this suite. `workspace-tests.log` passes all 86 native-workspace
  tests (2.80 s).
- `native-workflow.log`: the first native histogram journey passes (10.22 s).
  Visual review found that the generic screenshot helper omitted a toplevel's
  own background; capture now includes it. The inspector uses a scrollable native
  window with enough initial height for the channel counts and interpretation.
- `reviewed-workflow.log`: the final native journey passes (8.17 s), process
  1927296. A P3 U16 source has known opaque, partially covered and transparent
  regions under a half-coverage mask with its inspection tint enabled. All RGB
  bins and endpoint/out-of-range counts match the expected full-resolution data.
  Exposure edits update the counts without baking source pixels. Checks cover
  pause/resume, rapid edits, closing during a real capture, reopening the same
  inspector, RGB/luminance controls and unchanged drawing data during inspection.
- `histogram-ui/1927296/{rgb,luminance}.png` are visually reviewed native captures.
  `reviewed-production-check.log` checks GTK/shared FFI cleanly (1.02 s).

The final captured `histogram-reviewed-tests` binary has SHA-256
`5fa2e8cc0f778e58d050e1dd21b091cd72ff1640088f6f796a32c680b1be7468`;
`reviewed-build.{json,log}` and `reviewed-sources.json` identify its inputs.
Reproduce using the JPEG pkg-config setup above:

```sh
cargo test -p layer-core --offline color::histogram
cargo test -p layer-core -p layer-ui -p layer-workspace --features native --offline
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/histogram-reviewed-exact" \
  workspace::tests::histogram::native_composite_histogram_updates_without_changing_the_drawing \
  "$PWD/artifacts/color-m2/histogram-reviewed-workflow"
```

`histogram-provenance.json` records source/build/output hashes. This closes the
basic GTK composite-inspection journey, not large-photo/job qualification. The
snapshot component ceiling remains 512 MiB, separate from retained source, driver
and other jobs. Full-frame histogram cost, cancellation latency, simultaneous
inspector/export/conversion memory, and revision-based over-invalidation still
need the final workload matrix and optimization. Color preferences, managed wide
viewing, remaining output controls and the other documented gaps stay open.
No performance workloads or other host integration were performed in this stage.

## GTK Color preferences and reusable ICC profiles (2026-09-15)

Preferences → Color now owns defaults for future drawings (space, depth and
background), dimensions/preset management, optional promotion of opened photos to
16-bit editing, and the choice to assume sRGB or ask about untagged RGB/grayscale
images. These preferences persist without changing an existing drawing. Promotion
changes document editing precision; retained original samples keep their source
depth and profile. Native masters ignore photo-opening policy. Tagged photos open
without a prompt. Open, Place and Paste share the optional interpretation sheet;
cancellation publishes nothing, and Place/Paste keep the destination's mode.
Malformed profiles and ambiguous CMYK remain explicit decoder errors.

The native ICC library imports exact bytes into application-owned, SHA-256-named
files, with limits of 128 profiles, 16 MiB per profile and 64 MiB total. Import is
atomic and deduplicates exact profiles. Missing/damaged files cannot be selected;
changed files remain visible for removal or repair by reimport. Source and export
choosers validate the selected profile for their actual CMM role. Removing an entry
removes only its library copy; originals and profiles embedded in drawings remain
intact. File reading, validation and writes use worker jobs. These library limits
are component limits, not evidence of a combined application-memory bound.

Evidence under `artifacts/color-m2/color-preferences-*`:

- `shared-test.log` passes the shared policy/serialization test. `shared-suite.log`
  initially reports 389 UI passes and two stale metadata/copy expectations. After
  correcting the six-page count and shortening descriptions, the 11 affected
  settings tests and metadata test pass in `reviewed-settings-tests.log` and
  `reviewed-metadata-test.log`. All 86 native-workspace tests pass in
  `workspace-tests.log`.
- `file-tests.log` passes all seven selected file tests, including source-depth
  promotion, unchanged native masters, exact library bytes, deduplication,
  corrupt/oversized rejection, checksum repair and preservation after removal.
- `visible-workflow.log` passes the original native preference/Open/Paste journey
  (7.88 s). Its Color page and untagged-interpretation captures were inspected;
  shortened choice labels fit. `reviewed-workflow.log` extends that journey with
  a real library-profile PNG export and verifies the exact embedded ICC bytes
  (9.92 s, process 1953324). The test also checks saved preferences, unchanged
  existing drawing, cancelled Open/Paste, interpretation without altering source
  pixels, library removal and native save/reopen.

The existing preferences regression exposed two real host routing gaps: settings
broadcasts were discarded by the workspace input gate during loading/ownership
changes, and the main Preferences dialog lost shared type-to-search routing.
Host RestoreSettings now crosses that gate, and the main dialog retains its
search behavior while nested sheets own their keys. The regression explicitly
selects the application keyboard controller instead of assuming GTK controller
order. Its old prediction-control assertion now checks the current disabled
control and unavailable-system explanation. Earlier failing attempts are retained
in `regression.log`, `reviewed-regression.log`, `complete-regression.log` and
`named-regression.log`; they are not passing evidence. The last stale assertion
looked for Preferences at the top level of the GTK main menu; it now checks its
current Edit-menu location (`accepted-regression.log` records that failure).

The final captured binary `color-preferences-verified-tests` has SHA-256
`95e0ed081841e53bdf623a122b654afcfd31dc8cefd7e3a0e22a77f903d076b9`.
`verified-build.{json,log}` and `verified-sources.json` identify its inputs.
`verified-regression.log` passes the complete existing preferences test (16.79 s),
including dark/light pages, search, native shortcut recording, persistence and a
new window. `verified-workflow.log` passes the complete Color/ICC journey (9.90 s,
process 1963836). The light Color page and final interpretation dialog were
visually inspected. `accepted-production-check.log` passes GTK/shared FFI checks
(1.03 s); only test expectations changed after that check.

Reproduce using the JPEG pkg-config setup above:

```sh
cargo test -p layer-ui -p layer-workspace --features native --offline
cargo check -p layer-linux -p layer-ffi --offline
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/color-preferences-verified-exact" \
  workspace::tests::native_preferences_and_shortcuts \
  "$PWD/artifacts/color-m2/color-preferences-verified-regression"
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/color-preferences-verified-exact" \
  workspace::tests::color_preferences::native_color_preferences_profiles_and_untagged_photo_policy \
  "$PWD/artifacts/color-m2/color-preferences-verified-workflow"
```

`color-preferences-provenance.json` records source/build/output hashes. Final
Color fixtures and captures are under `color-preferences-ui/1963836/`; the
existing preference captures are retained under `color-preferences-regression-ui/`.

No performance workload ran in this stage. The Color page accurately reports the
current sRGB canvas fallback. Managed-wide canvas/widget/monitor agreement,
remaining export controls, combined resource/job limits and final precision and
workload qualification remain open. Fresh frame-creation baselines, regression
investigation and optimization remain last. Other platform hosts remain
unintegrated pending approval after GTK completion.

## GTK managed artwork and explicit Wayland SDR descriptions — 2026-09-15

This checkpoint follows `96c41bd0`. It connects managed viewing; it does not
complete milestone 2 or qualify a physical monitor. The GTK canvas, picker
fields/ring/markers, paint samples, saved palettes, numeric-color sheet,
Before/After comparisons, layer thumbnails and filter thumbnails now carry a
consistent view definition. View conversion never feeds document edits, exact
sampling, native persistence or profiled delivery. GTK follows the compositor's
output state; monitor enter/leave notifications refresh the informational Color
page without changing document/history state. Generic effect and gradient color
editors remain outstanding.

The renderer chooses an advertised 8-bit Vulkan pass-through format, then the
host negotiates an exact SDR image description: Display P3 first, sRGB second.
The preferred description uses Wayland color-management v2's
`compound_power_2_4` transfer (14). A generated matrix/TRC ICC profile provides
an alternative, including protocol v1. If the compositor exposes no color manager,
both GTK artwork and the canvas use the untagged sRGB fallback. If an available
manager rejects both precise SDR descriptions, initialization reports the error.
No document values are clipped to the view gamut; clipping occurs only in the
view outputs. HDR surface modes are not selected.

Independent checks changed the initially proposed integration:

- GTK 4.22.4 ignores the color-manager global unless `GDK_DEBUG=color-mgmt` is
  enabled. The first protocol trace showed only the canvas publishing a color
  description. The production entry point now enables that flag before GTK or
  worker initialization and preserves other debug flags. Native tests set it in
  their launch environment. This is a scoped workaround for an upstream runtime
  gate without a public API, confirmed against the
  [GTK 4.22.4 Wayland implementation](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdkdisplay-wayland.c).
- NVIDIA's WSI described its initial P3 swapchain using the legacy sRGB transfer
  value 9. [Mutter 50.4](https://github.com/GNOME/mutter/blob/50.4/src/wayland/meta-wayland-color-management.c)
  interprets that value as gamma 2.2, unlike the piecewise curve encoded by our
  presenter. Matching offscreen RGB values did not establish correct display
  interpretation. The raw Vulkan capability probe confirmed pass-through support.
  The [Vulkan specification](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html)
  defines that mode as the way to prevent WSI from owning the Wayland color
  object. A small wgpu patch exposes it; provenance, exact source patch and
  licenses are in [vendor/README.md](../../vendor/README.md). wgpu retains
  swapchain acquisition, synchronization and presentation.
- The ICC alternative initially failed with `Couldn't parse ICC profile`.
  Mutter's [file reader](https://github.com/GNOME/mutter/blob/50.4/src/core/util.c)
  reads from the received descriptor's current position. Rewinding the anonymous
  profile file before transferring it fixes the failure; the profile bytes and
  declared offset remain unchanged. A subsequent native v1 ICC run passes.
- A GTK snapshot caught fractional checker-edge alpha seams: an opaque swatch
  produced alpha about 0.755. Waiting for animations did not change the result.
  Painting one opaque base beneath alternating checker tiles fixes the seam.
  The regression compares the actual GTK widget's rendered RGBA with the canvas.

GDK textures carry explicit ColorState metadata, using P3/D65 and the piecewise
sRGB curve for P3. Color samples composite transparency in linear light before
view clipping. The GPU's small UI-image encoder has a separate configured output
space, including Navigator readback and filter previews. It does not change the
sRGB diagnostic/export readback contract or exact document-space samples. Color
conversion, depth changes, Undo/Redo and GPU recovery preserve the selected view
route when preparing a replacement renderer. The former untagged Cairo artwork
paths for picker fields, paint samples, palette samples and numeric previews are
removed; neutral outlines remain native GTK/Cairo.

All evidence below is in `artifacts/color-m2/managed-view-*`. The environment is
GTK 4.22.4, Mutter 50.4, the recorded Fedora 44/NVIDIA 610.57.04 workstation and a
private 1600×1000@120 Hz Wayland monitor. These are correctness checks, not
performance workloads; their elapsed times do not qualify frame creation or
input latency.

- `shared-color-tests.log`: 37 existing shared color tests pass. The separate
  `field-tests.log` checks all four working spaces, three picker shapes and both
  output gamuts against exact picker definitions at real field pixel centers;
  rendering leaves color state unchanged.
- `gpu-final-tests.log`: four GPU view tests pass (47.23 s). The expanded
  export/Navigator/thumbnail/sample case covers both UI output gamuts, all four
  working spaces and both integer depths. It includes zero-exposure filter
  previews and verifies that sRGB export coordinates and native samples stay
  independent of the selected UI output. Source and composite bytes stay intact.
- `explicit-wayland.log`: native P3 canvas/widget comparison passes (6.50 s).
  The trace identifies GTK parent `wl_surface#48` and canvas child `#73`.
  The app's color manager `#90` creates a P3 description with transfer 14,
  receives `ready2`, and attaches it through color surface `#160`. GTK independently
  attaches its output description through `#51`. WSI does not create a second
  color object for the child.
- `explicit-srgb.log`: the managed sRGB fallback passes (2.91 s) with the same
  wide-gamut source and exact retained data.
- `qualified-icc.log`: protocol-v1 P3 through a 580-byte ICC profile passes
  (3.19 s), including descriptor acceptance and canvas/widget comparison.
- `qualified-unmanaged.log`: disabling color management for both native clients
  passes the shared untagged sRGB fallback (2.77 s).
- `qualified-recovery.log`: Assign/Convert/Depth, Before/After, exact Undo/Redo,
  save/reopen, a flat copy, continued drawing and deliberate GPU failure/restart
  pass with the explicit managed route (33.43 s).
- `qualified-selection.log`: unsupported and mismatched pass-through format
  choices are rejected; selection uses the actual format/capability pairs.
- `vulkan-mapping-online-tests.log`: all three upstream Vulkan mapping tests
  pass, including the newly added pass-through round trip. Initial workspace
  selection attempts could not run an excluded dependency's dev tests; the
  standalone offline attempt lacked `glam`. The successful standalone run fetched
  test dependencies and retained its lockfile as `wgpu-hal-test.lock`.
- `qualified-production-check.log`: GTK and shared FFI checks pass (3.90 s).
- `patched-gpu-tests.log`: all four GPU view tests also pass against the patched
  dependencies (49.73 s), with executable SHA-256
  `1f4679a1ef48df5d937de793daae763c0210cbdd7d550857c60de1a1343430ff`.
- `qualified-numeric.log`: numeric editing and saved palettes pass (7.55 s).
- `explicit-production-wayland.log`: the real application exits successfully
  after capturing its window. With the GTK flag initially absent, both parent
  and child publish descriptions; the child uses P3 and transfer 14. This proves
  the production startup path rather than relying on the test harness's flag.

The final GTK test executable `managed-view-qualified-tests` has SHA-256
`a106b91ee0379702cb28e03cb270a181bd07c0475542e5fd2d1bd5d362237dd0`.
The production executable `managed-view-explicit-production-app` has SHA-256
`7ddd33995156838f502066e6b263238ff0d3a4b2a34f8d707125da72bd649499`.
Build JSON/logs and captured source hashes accompany them. The ICC run's screenshot
`managed-view-ui/2011864/p3-canvas-and-picker.png` was visually inspected: canvas,
Navigator, layer preview and selected paint sample agree, with intact picker
geometry. The raw capability probe's C source is retained as `wsi-formats.c`.
`managed-view-provenance.json` records the final inputs, executable hashes,
validation outputs and remaining qualification gaps. All 308 captured Rust
source hashes still matched when the checkpoint was recorded.

Reproduce the main native routes with the JPEG pkg-config setup above:

```sh
cargo test -p layer-linux --offline --no-run --message-format=json
bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/managed-view-qualified-exact" \
  workspace::tests::managed_view::native_managed_canvas_and_gtk_artwork_agree \
  "$PWD/artifacts/color-m2/managed-view-repeat-p3"
LAYER_TEST_VIEW_ICC=1 bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/managed-view-qualified-exact" \
  workspace::tests::managed_view::native_managed_canvas_and_gtk_artwork_agree \
  "$PWD/artifacts/color-m2/managed-view-repeat-icc"
LAYER_TEST_VIEW_SRGB=1 bash tools/performance/gtk-raster.sh \
  "$PWD/artifacts/color-m2/managed-view-qualified-exact" \
  workspace::tests::managed_view::native_managed_canvas_and_gtk_artwork_agree \
  "$PWD/artifacts/color-m2/managed-view-repeat-srgb"
```

Add `WAYLAND_DEBUG=client` to capture the actual surface descriptions. The
unmanaged fixture additionally sets `LAYER_TEST_VIEW_UNMANAGED=1` and
`GDK_WAYLAND_DISABLE=wp_color_manager_v1`. Those route overrides exist only in test
builds. Production enables GTK's flag itself; its smoke wrapper deliberately
starts without `color-mgmt` so the check exercises the application entry point.

The upstream Vulkan mapping tests use the vendored package's standalone manifest:

```sh
cp artifacts/color-m2/managed-view-wgpu-hal-test.lock vendor/wgpu-hal/Cargo.lock
CARGO_TARGET_DIR="$PWD/target" cargo test \
  --manifest-path vendor/wgpu-hal/Cargo.toml --locked --offline --features vulkan --lib \
  --config "patch.crates-io.wgpu-types.path=\"$PWD/vendor/wgpu-types\"" \
  vulkan::conv::tests:: -- --test-threads=1
```

Earlier `probe-workflow.log` and `previews-workflow.log` are failing checker-seam
attempts, and `explicit-icc.log` is the failing file-position attempt. Earlier
`enabled-wayland.log` and production traces still used WSI's ambiguous transfer
value and are not accepted managed-display evidence. `gpu-selection-error.log`
ran zero tests after selecting an integration-test binary; it is not passing
matrix evidence. `gdk-flags.log` intentionally failed display initialization while
listing GTK debug flags. These records are retained to explain the findings.

Physical calibrated-monitor agreement, monitor/profile changes, window spanning,
alternate GTK renderers and final zoom/anisotropic quality remain unqualified.
A fixed assumed SDR surface description is valid across monitors because
[Wayland assigns monitor transforms to the compositor](https://wayland.freedesktop.org/docs/book/Color.html);
that protocol contract is not physical measurement. Effect/gradient color
controls, remaining delivery controls, combined job/resource limits and the final
precision/workload matrix remain open. Benchmarking, fresh frame-creation
baselines, regression investigation and optimization remain last. Other platform
hosts have not been integrated or built for this checkpoint and require approval
after GTK completion.

## GTK effect colors, gradients and retained paint controls — 2026-09-15

This checkpoint follows `bb8a0300`. It completes the remaining GTK effect-color,
gradient-stop and retained paint-control integration, not milestone 2 as a whole.
Benchmarking and optimization remain deferred to the final phase. Correctness
run durations below are not frame-creation or interaction-latency measurements.

Current code contradicted its old comment: `EffectValue::Color` and gradient
stops were documented as sRGB arrays, but the built-in shaders consumed encoded
document RGB. In particular, `fx_rgba` decodes using `FX_SPACE`; Black & White,
Split Tone, Halftone, Crosshatch and Pencil pass their color parameters through
that domain. Parameter preparation now converts portable `RgbColor` definitions
to encoded document RGB. Stored defining spaces, extended RGB and alpha remain
unchanged. Assign/Convert/depth changes preserve these definitions, and derived
GPU coordinates follow the new document space. GPU record layout stays ABI 3;
untagged serialized color arrays are rejected, without a compatibility reader.
The current contract and examples are in [Runtime filters](../reference/runtime-filters.md).

The existing RGB conversion implementation was checked against the
[CSS Color 4 conversion reference](https://www.w3.org/TR/2026/CRD-css-color-4-20260913/#color-conversion-code).
A new upload oracle checks P3 red against independent linear-sRGB coordinates
`[1.2249402, -0.0420570, -0.0196376]`, including the negative components and exact
low alpha. Persistent colors must remain finite in every supported working-space
conversion; corrupt extreme values are rejected before publication.

GTK uses one managed numeric draft for paint and effect colors. Input-model
switches preserve untouched definitions; alpha-only edits preserve defining RGB.
Cancel/invalid input leave the document untouched. Acceptance checks document
identity and retained-control lifetime; gradient acceptance also checks the
selected stop and complete stop list. Accepting an unchanged effect value now
skips the shared edit, preserving history and dirty state. This fixes an existing
no-op history defect exposed by the native test.

The gradient ramp evaluates encoded document RGB and straight alpha, then
converts and composites over its checker in linear light before view clamping.
Only these preview textures are display data. Stop insertion uses the same
shared interpolation domain and keeps one undo step. The original 65-vector
analytic tables retain all knots, including intervals narrower than one U16 code.
Existing effect alpha semantics remain: Gradient Map uses alpha as mapping
strength; tint/ink/paper effects use their RGB components only.

The generic GTK sRGB ColorDialogButton paths, raw Cairo gradient fill and live
sRGB SVG-palette substitution are deleted. Toolbar and title-bar swatches retain
the original shared icon coordinates and neutral outline, with tagged fills.
Their weak view registry discards destroyed widgets when creating or updating
views. Repeatedly rebuilding a toolbar with unchanged colors cannot grow that
registry indefinitely. Gradient preview storage is two float RGBA rows at widget
resolution, cached independently of the selected handle. These are implementation
bounds, not measured peak-process/GPU-budget qualification.

### Correctness evidence

Artifacts use `artifacts/color-m2/effect-colors-` prefixes. The environment is the
same Fedora 44 / GTK 4.22.4 / Mutter 50.4 / NVIDIA 610.57.04 RTX PRO 6000 Blackwell
Max-Q reference setup as the preceding managed-view checkpoint. Native runs use
a private 1600×1000@120 Wayland compositor with the GTK color-management flag.
No physical calibrated-monitor or stylus qualification is claimed.

| Check | Result and evidence |
| --- | --- |
| Core effect definitions | 10 passed; `core-tests.log`: tagged persistence, conversion coordinates, invalid-value atomicity, mixed-gamut insertion and existing catalog/table checks. |
| Portable values | 3 passed; `values-tests.log`: extended coordinates, low alpha, conversion overflow and serialization. |
| Shared effect actions | 1 passed; `session-qualified.log`: insertion, property/stop edits and undo through shared policy. |
| Shared color/picker/editor | 38 passed; `color-tests.log`. |
| Assign/Convert/depth/history | 8 passed, 12.06 s; `document-tests-2.log`. Fixtures now include P3 and ProPhoto live colors and gradient stops; original sources, samples, masks, wet planes and exact history remain covered. |
| Native GPU color parameters | 1 passed, 28.32 s; `gpu-tagged.log`: all four working spaces × both depths × fused/physical gradient execution × four input-alpha cases; Halftone and Gradient Map scalar expectations. |
| Full U16 analytic table precision | 1 passed, 5.01 s; `final-gpu-native_analytic_curve_and_gradient_tables_resolve_every_code_and_narrow_knots.log`: every code, four gradient components, close knots and both execution paths. |
| Existing Halftone oracle | 1 passed, 3.37 s; `final-gpu-halftone_endpoints_match_scalar_color_oracles.log`. |
| Existing adjustment color oracles | 1 passed, 1.56 s; `final-gpu-builtin_adjustments_have_known_color_results.log`. |
| GTK P3 workflow | 1 passed, 5.70 s; `complete-native_effect_colors_gradients_and_retained_controls.log`: exact model switching, invalid input/cancel, numeric changes, alpha-only edits, undo/redo, stop insertion, GTK ramp pixel comparison, native save/reopen and retained brush controls with independent brush opacity. |
| GTK sRGB fallback workflow | 1 passed, 5.71 s; `complete-srgb.log`: the same wide-gamut document and portable definitions through fallback viewing. |
| Canvas / GTK artwork agreement | 1 passed, 2.91 s; `complete-native_managed_canvas_and_gtk_artwork_agree.log`, after extracting the shared opaque-checker drawing helper. |
| Native icons and retained swatches | 1 passed, 43.91 s; `complete-icons.log`: original vector geometry, both themes, three toolbar sizes, both paint slots and all forty filter controls/previews. |
| Existing numeric / palette workflow | 1 passed, 10.95 s; `native_numeric_colors_and_saved_palettes.log`, before the final icon-only correction. |
| Drawing and GPU recovery | 1 passed, 30.83 s; `recovery.log`: Assign/Convert/depth, complete comparisons, undo/redo, save/reopen, flat copy, continued drawing and deliberate GPU failure/restart. The logged GPU panic is the injected recovery condition. This run preceded the icon-only correction. |
| Production GTK / shared FFI | Passed; `production-check.log`, 4.43 s. |
| Real native pickup | 1 passed, 80.20 s; `native-pickup-run.log`: mouse/touch tile, drawer, column, tab and grip pickup, hold-release/context, cancellation and undo. Physical pen remains unqualified. |

The final GTK executable `effect-colors-complete-tests` has SHA-256
`ee2cca119b9154e48dcb0741cd50f7b314957bec7f33a9adaf8cb0a05874ec4f`.
The final GPU executable `effect-colors-final-gpu-tests` has SHA-256
`bc8e3c2e9505372f0b8722c29d43dbbe63a205973c671657bc6f48ea13c6da75`.
The tagged GPU matrix used `effect-colors-gpu-tests`, SHA-256
`f2d59a5c405786709bd333475b1998f5474db3fae2a0ac7b3a8e0555b2cbc3c1`;
the subsequent renderer-test change only retagged the independent narrow-knot fixture to its
actual ProPhoto space. Build JSON/logs and per-build source hashes accompany each.
`effect-colors-provenance.json` records 34 inputs and 662 outputs; every captured
final GTK Rust source hash matches the committed candidate.

Reproduce from the repository root, with the local JPEG headers visible:

```sh
export PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo test -p layer-core --offline effects:: --lib
cargo test -p layer-color --offline document::tests:: --lib
cargo test -p layer-ui --offline effect_creation_properties_and_navigation_are_shared --lib
cargo test -p layer-ui --offline color:: --lib
cargo test -p layer-linux --offline --no-run
cargo test -p layer-render-wgpu --offline --no-run
cargo check -p layer-linux -p layer-ffi --offline
```

Use Cargo's reported `layer-linux` executable with `tools/performance/gtk-raster.sh`
and filter `workspace::tests::effect_color::native_effect_colors_gradients_and_retained_controls`.
Repeat with `LAYER_TEST_VIEW_SRGB=1`. The captured `*-exact` wrappers add `--exact`;
GPU executables use the full `tests::native_effects::tone::...` names recorded in
the table, `--exact --test-threads=1`. Real mouse/touch pickup uses
`GDK_DEBUG=color-mgmt LAYER_NATIVE_TEST_EXECUTABLE="$PWD/artifacts/color-m2/effect-colors-complete-tests" bash tools/performance/workspace-motion.sh gtk --drag-pickup`.
This executes correctness tests despite the
harness directory being named `performance`.

### Findings and remaining qualification

Earlier failing attempts are retained. `shared-tests.log` used a lowercase
manifest enum before correcting it to `Srgb`; `document-tests.log` initially put
the added effect fixtures below Paper. `final-build.json` records the test's
missing `Project` qualification. `native-p3.log` exposed the no-op history defect.
`qualified-p3.log` / `final-p3.log` reached the reopened document but failed to
show its initially hidden Properties panel. `gpu-native_analytic_...log` tagged
a numeric ProPhoto precision fixture as sRGB; the new upload correctly converted
it, exposing the test setup error. These are not passing evidence.

The first toolbar swatch implementation used GTK Frame corners. The icon oracle
found red 58 instead of 51 at the background sample: theme rounding changed the
square into a disc. `icon-probe.log` and its small-patch capture isolate this.
The final custom snapshot uses the original SVG geometry; the same two-byte
oracle passes without loosening its tolerance. P3 gradient/numeric screenshots
under `effect-color-ui` and both themes/sizes under
`effect-colors-complete-icon-ui/controls` were visually inspected.

`complete-native_toolbar_sizing.log` fails its synthetic grip double-click reset.
The saved parent executable fails identically in `parent-toolbar-sizing.log`
(262×346 retained versus expected 112×286). Current input handling requires an
actual GDK event and ignores that test's emitted signal without one. This is a
pre-existing native harness gap, not accepted sizing evidence; it must be repaired
or replaced with real-contact coverage before final broad qualification.

Remaining GTK work still includes delivery resize/output-preview/recipes/DPI and
format-policy gaps, combined job cancellation/scheduling and resource budgets,
coarse-first source/display work and final zoom quality, complete tool/filter
precision and large-document qualification, and monitor/profile/alternate-renderer
validation. Fresh frame-creation baselines, regression investigation and all
benchmarking/optimization remain last. Other host color controls and their JSON
color projections have not been integrated or built for this checkpoint; shared
Rust/FFI checks do not qualify them. They require approval after GTK completion.

## 2026-09-15 — Resized SDR delivery and GTK alert ownership

Implementation starts from `6f24d143`. This is a GTK functionality/correctness
checkpoint, not completion of milestone 2. Benchmarking and optimization remain
last, as requested; elapsed test durations below are correctness-run durations.
Other platform hosts have not been integrated or built.

### Output contract and independent checks

The renderer now distinguishes native snapshot geometry from delivery dimensions.
GTK Export offers Original size or Fit within a maximum width/height, preserving
proportions with explicit enlargement. The resolved dimensions remain outside the
scrolling options, above the action buttons. The master keeps its dimensions,
backing, color mode, location and dirty state. These choices are shared serialized
`ExportSize` data; they do not yet constitute a named-preset library.

Resizing takes the full native linear premultiplied composition, then applies the
selected output conversion, matte, depth and output-coordinate dithering. Axis
reductions integrate exact pixel-area overlaps; enlargement evaluates the
Catmull–Rom cubic with replicated edge pixels. The cubic's parameters are B=0,
C=1/2 in the [Mitchell–Netravali family described by PBRT](https://www.pbr-book.org/3ed-2018/Sampling_and_Reconstruction/Image_Reconstruction#MitchellFilter).
This choice is a reconstruction policy, not a claim of ideal reconstruction or
photographic quality qualification. The mathematical checks use independently
written rectangle integration and cubic Hermite interpolation, including mixed
axis scaling; they do not use production taps as their oracle.

Float32 associated samples use Float64 filter weights and accumulation. Cubic
coverage is clamped to [0,1] with associated RGB renormalized by the same factor;
extended RGB survives until output conversion. Transparent samples cannot add
hidden RGB or dark fringes during resizing. Unchanged-size identity exports still
bypass composition/resampling and preserve native integer samples, including
hidden straight RGB, exactly. Returning a renderer to original delivery size
restores that route. PNG/TIFF/JPEG share one profiled row pipeline; the old inline
snapshot output implementation was moved and replaced, without a second codec
path or compatibility adapter.

The resampler retains at most four filtered rows, one raw source row, one output
accumulator and fixed-size horizontal tap descriptions. Area support is computed
without allocating a vector proportional to image height. Snapshot capture uses
16-row source bands and the existing dependency planner. A 32,768-row scalar
fixture checks cache bounds and reads every source row exactly once. These are
structural bounds, not a measured process/GPU memory qualification: codec buffers,
ICC state, retained sources, history, active jobs and driver allocations still
need the final aggregate budgets and measurements.

Cancellation reaches source-row capture and each output row. Output progress uses
delivery height. Existing atomic publication arbitration is preserved; failures
cannot publish partial output. The resized GPU test also exercises capture-budget
failure and cancellation, while the existing export regression tests cover exact
identity, profile conversion, matte and deterministic dithering.

### Native dialog finding

A repeated-cancel test found that the installed libadwaita-rs 0.9.2 async alert
wrapper leaves an owned dialog reference alive after dismissal. Its local
`src/alert_dialog.rs` passes `self.upcast().into_glib_ptr()` to a borrowed C
parameter. [libadwaita 1.9.3's implementation](https://raw.githubusercontent.com/GNOME/libadwaita/1.9.3/src/adw-alert-dialog.c)
creates and releases its own GTask reference; it does not consume this caller
reference. The diagnostic retained-object walk reaches an unrooted AlertDialog.
This is separate from the initially introduced sizing callback cycle, which was
removed by making widget captures weak.

`alert::choose` now owns a native response connection and its Rust dialog handle
explicitly. Completion disconnects the handler; dropping an unfinished future
closes the dialog. All GTK async AlertDialog callers use this helper, including
color/photo dialogs and existing file/recovery/workspace confirmations. No unsafe
manual unref, dependency fork or timer-polling production wait was introduced.
The precision-note callback also uses weak widget references.

### Validation record

All paths below are under `artifacts/color-m2/`. Production GTK/FFI checks are
read-only shared integration checks, not qualification of another host. Hardware
runs use the local RTX PRO 6000 Blackwell Max-Q, NVIDIA 610.57.04, GTK 4.22.4,
libadwaita 1.9.3, and the private Mutter 1600×1000@120 harness with GSK Vulkan.

- `export-resize-color-tests.log`: 5 scalar tests passed (0.15 s), reference
  tolerance 2e-7 in linear associated channels. Includes fractional edges, thin
  features, constants, low alpha, extended RGB, anisotropy, bounded cache,
  invalid geometry/sequence/nonfinite input and provider cancellation.
- `export-resize-ui-tests.log`: 2 shared recipe tests passed, covering fit,
  orientation, enlargement, dimensional limits, serialization and channel/depth
  policy. Profile and depth are independent of size.
- `export-resize-gpu-final-tests.log`: 2 resized GPU tests passed (12.01 s),
  including a full independent render with masks, transformed selections,
  compositing and effects. PNG/TIFF output matches an independent area reference
  within one code at both depths for P3 RGBA and grayscale with matte. Enlarged
  quality-100 JPEG differs from the matching PNG by at most four 8-bit codes.
- `export-resize-regression-snapshot_*.log`: existing exact identity (18.51 s),
  profiled composition/budget/cancel (8.95 s), JPEG (1.06 s) and deterministic
  dithering/identity (2.21 s) tests each passed without changing their tolerance.
  GPU executable `export-resize-gpu-final-tests` SHA-256:
  `871c82cbb08aec15575003fd85141836cee1655d40c91c183a99ef559c57cdc9`.
  Subsequent renderer-source change only removed blank lines at the module split.

The final GTK executable is `export-resize-gtk-accepted-tests`, SHA-256
`800ae4f85ebeab9fc55ef486a4e5513eb17d6b339b9a0d4f4ef533643690364e`.
Its `-sources.json` records 28 changed/new Rust source hashes; all match the final
implementation. Each native log below has prefix `export-resize-accepted-` and
suffix `.log`, with separate `-session.log` / `-mutter.log` diagnostics:

| Native check (test-name suffix) | Result |
| --- | --- |
| `native_alert_wait_releases_responses_and_abandoned_futures` | 1 passed, 1.86 s |
| `native_export_sizes_preserve_master_and_release_cancelled_dialogs` | 1 passed, 13.20 s |
| `native_document_color_assignment_conversion_depth_history_and_copy` | 1 passed, 32.46 s; includes intentional GPU panic and successful recovery |
| `native_numeric_colors_and_saved_palettes` | 1 passed, 7.69 s |
| `native_new_presets_and_profiled_photo_master` | 1 passed, 12.68 s |
| `native_document_files` | 1 passed, 36.62 s; custom RGB, gray and CMYK output |
| `native_color_preferences_profiles_and_untagged_photo_policy` | 1 passed, 10.04 s |
| `native_source_profile_repair_preserves_originals_and_baked_edits` | 1 passed, 15.04 s |
| `native_rasterization_keeps_off_canvas_source_paint_mask_and_reopen` | 1 passed, 8.45 s |
| `native_named_workspace_manager_library_and_history` | Fails before completion, 8.19 s; parent fails identically, see below |

`export-resize-accepted-production-check.log`: GTK and shared FFI check passed.
The native sizing test exports a ProPhoto16 master as 75×50 PNG16, 192×128 TIFF16
(no enlargement requested), and 300×200 JPEG8 (explicit enlargement and matte).
It checks decoded extent/depth/profile and exact serialized master equality with
unsaved state retained, and repeats three cancelled sheets with weak-reference
retirement assertions. Screenshots `export-resize-native/2113622/{png,jpg}-sizing.png`
were visually inspected; output dimensions remain readable above the buttons.

ImageMagick 7.1.2-27 Q16-HDRI independently reports those dimensions/depths and
embedded ProPhoto RGB ICC descriptions in `export-resize-external-identify.log`.
Its generic `colorspace=sRGB` classification describes nonlinear RGB here; the
embedded profile description and the native byte-equality checks identify the
actual ProPhoto interpretation. This is external decoder inspection, not a new
manual external-editor handoff or calibrated-monitor qualification.

Reproduction (from the repository root, with local JPEG development headers):

```sh
export PKG_CONFIG_PATH="$PWD/artifacts/deps/jpeg/usr/lib64/pkgconfig"
cargo test -p layer-color --offline resize:: -- --test-threads=1
cargo test -p layer-ui --offline export:: -- --test-threads=1
cargo test -p layer-render-wgpu --offline --no-run
cargo test -p layer-linux --offline --no-run
cargo check -p layer-linux -p layer-ffi --offline
```

Run Cargo's reported GPU executable with `snapshot::tests::resized::
--test-threads=1`; the four existing regression names are recorded in the log
filenames above and take `--exact snapshot::tests::<name> --test-threads=1`.
Use the GTK executable with `tools/performance/gtk-raster.sh`, setting
`LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm`. The new full names are
`workspace::tests::export_resize::native_alert_wait_releases_responses_and_abandoned_futures`
and `workspace::tests::export_resize::native_export_sizes_preserve_master_and_release_cancelled_dialogs`.
Other full names and exact command arguments are captured in the validation
session records. The `*-exact` wrappers add `--exact` to prevent accidental
substring selection. `export-resize-provenance.json` records inputs and artifacts.

### Failed attempts and remaining work

The first GTK test build had test-only type/name errors. The initial GPU resized
reference passed its numerical comparisons but its failure-path fixture looked
for a source on the group's first layer; the corrected test constructs its target
interpretation explicitly. An initial native invocation used a missing
`workspace::` prefix and ran zero tests; it is not evidence. These logs remain.

The `native-qualified`, `native-lifetime` and `native-release` attempts exposed
the alert reference leak. `native-owned` first passed repeated export-sheet
retirement and all three formats. The minimal alert fixture initially used an
empty window: GTK released the dialog but retained an unrooted content subtree.
Waiting for unmapping did not fix that variant and was removed. Giving the parent
its normal focusable editor content makes acceptance, close/cancel and abandoned
future checks pass. The blank, unfocusable parent variant is not qualified; it is
not used as evidence that every GTK internal object retires in every host setup.
The `delivery`, `checked`, `content` and `unmap` lifetime failures are retained.

The workspace-manager library/history test stalls at “Workspace selection did
not finish” before reaching its later confirmations. The saved parent executable
from `6f24d143` fails at the same assertion in 8.22 s
(`export-resize-parent-workspace.log`; SHA-256
`ee2cca119b9154e48dcb0741cd50f7b314957bec7f33a9adaf8cb0a05874ec4f`). This is an
existing native qualification gap, not accepted workspace coverage. Investigate
it alongside the previously recorded toolbar-sizing harness gap before final
broad qualification. The focused alert and affected color/photo workflows above
pass; the failed workspace test is not silently counted as passing.

Remaining delivery work is output preview/comparison, reusable named and remembered
recipes, resolution metadata and the recorded TIFF/HDR-input policies. Remaining
milestone-wide work includes aggregate job cancellation/scheduling and resource
budgets, coarse-first source/display work and final zoom quality, complete
brush/filter/precision/large-document qualification, monitor/profile and alternate
GTK-renderer checks. Fresh frame-creation baselines, regression investigation,
peak/steady memory, p95/p99 latency and optimization stay last. Other platform host
integration still requires user approval after GTK completion and qualification.

## 2026-09-15 — Preview the delivered SDR samples

Implementation starts from `5e1cdc62`. This completes the current GTK output
comparison functionality, not milestone 2 qualification. No benchmarking or
optimization was performed. Other platform hosts were not integrated or built.

Export captures an immutable project snapshot before opening its options. The
Master/Output comparison and final file use that same snapshot, background and
time. Output preview uses the encoder's shared row pipeline: resizing, actual
output ICC interpretation, depth, matte and dithering precede thumbnail reduction.
Grayscale uses the actual generated grayscale profile. CMYK samples are decoded
through their output ICC profile for viewing. The returned clipping statistics
cover the whole output. JPEG explicitly discloses that compression artifacts are
excluded; changing JPEG quality therefore does not invalidate this preview.

The shared renderer now provides bounded linear associated previews, replacing
the GTK-only area accumulator. Master preview reduces the native composition and
maps linear RGB to the viewing space. Output preview reduces interpreted delivery
samples. GTK applies its checker in linear light and creates an opaque texture
tagged with the selected sRGB/P3 view, so GTK theme/alpha composition cannot alter
translucent artwork edges. Neither preview enters editing, sampling or export.
Bounds are limited to 1024 per axis; GTK requests 220×160. The row resampler and
16-row capture bands avoid a whole-document CPU image. This is a structural bound,
not aggregate CPU/GPU memory or large-photo latency qualification.

The existing comparison controller retains one active worker and one replaceable
pending request. Changes cancel the active request, stale results cannot publish,
and closing the sheet cancels and waits for worker acknowledgement. The same
shared preview implementation now serves Assign/Convert, source repair and
rasterization comparisons. Each comparison currently captures its viewing route
when opened; live route changes inside an already-open modal and physical monitor
movement remain part of the outstanding viewing qualification.

Validation paths are under `artifacts/color-m2/`. GPU tests used the local RTX PRO
6000 Blackwell Max-Q / NVIDIA 610.57.04. Native tests used the private Mutter
1600×1000@120 harness, GTK 4.22.4, libadwaita 1.9.3 and GSK Vulkan.

- `export-preview-gpu-qualified-tests.log`: 1 passed (5.06 s). Both sRGB and P3
  viewing spaces cover native ProPhoto RGBA16, resized/dithered sRGB RGBA8,
  resized P3 RGB8 with matte, enlarged grayscale-alpha16, and CMYK8/16 TIFF with
  different mattes. Preview is compared to decoded actual exported files with an
  independent area integral, within 5e-4 linear associated channels. Master
  preview matches the independent native render/linear matrix within 2e-7.
  Whole-output statistics and row progress match exactly; cancellation, invalid
  bounds and unchanged project data are checked.
- `export-preview-native.log`: 1 passed (21.08 s), P3; the same test in
  `export-preview-native-srgb.log` passed (17.97 s) using `LAYER_TEST_VIEW_SRGB=1`.
  Actual PNG/TIFF files match downloaded GTK preview textures within two viewing
  codes, including the linear checker. PNG/TIFF/JPEG preview dimensions, JPEG
  disclosure and unchanged texture after quality changes are checked. Repeated
  cancellation releases sizing widgets; project bytes, revision, dirty state
  and location remain unchanged.
- Affected native regression logs use prefix `export-preview-`: document
  Assign/Convert/depth/history/copy passed (34.49 s), source repair passed
  (14.97 s), source rasterization passed (8.48 s), and `native_document_files`
  passed (37.39 s), including custom RGB/gray/CMYK output. The document-color test
  deliberately induces a GPU validation panic and verifies recovery.
- `export-preview-production-check.log`: GTK and shared FFI check passed (1.48 s).
  This is not qualification of another platform host.

Exact GPU executable `export-preview-gpu-qualified-tests` SHA-256:
`4a4bb8a1d96635f15fdf25bfcbd2a034107ba53e60684b776195422952a88d11`.
Exact GTK executable `export-preview-gtk-checked-tests` SHA-256:
`2472e349622e73e250ec4e24e176afbf70d8099c635c4b3cffb65054f51028ce`.
Both `-sources.json` files match all final changed/new Rust sources. The CMYK
fixture is `/usr/share/color/icc/krita/cmyk.icm` (961644 bytes), SHA-256
`156e7c14f244cfc4ed83a755ca4803d80e15dd249b40fae82cb127d3902e15c7`.
`export-preview-provenance.json` hashes the build/run logs and source manifests.
PNG/JPEG screenshots under `export-resize-native/2136767/` were visually inspected:
Master/Output and resolved dimensions fit the dialog, and the JPEG disclosure is
visible. This is not calibrated-display or external-editor qualification.

Reproduce with the local JPEG headers in `PKG_CONFIG_PATH`, `cargo test --offline
-p layer-render-wgpu --no-run` and `cargo test --offline -p layer-linux --no-run`.
Run the GPU executable with `--exact snapshot::tests::resized::output_preview_matches_the_delivered_samples_after_profile_depth_resize_and_matte
--test-threads=1 --nocapture`. Set `LAYER_TEST_CMYK_PROFILE` to the fixture above.
Run the GTK executable through `tools/performance/gtk-raster.sh` with full name
`workspace::tests::export_resize::native_export_sizes_preserve_master_and_release_cancelled_dialogs`,
then repeat with `LAYER_TEST_VIEW_SRGB=1`. The saved `*-exact` wrappers add
`--exact`. Regression suffixes/full names are captured in their log filenames.

The first GPU attempt failed in the independent reference: floating-point ceil
produced a row beyond the image boundary. Clamping its loop to source dimensions
fixed the oracle; production integer-grid area taps were unchanged. The first
native test build had test-only signed/unsigned dimension mismatches, corrected
before execution. These failed logs remain; neither is counted as passing.

Remaining delivery work is reusable named/remembered recipes, resolution metadata
and TIFF/HDR-input policies. Aggregate scheduling/cancellation/resource budgets,
coarse-first source/display and zoom quality, the full brush/filter/precision
matrix and large documents, monitor/profile/alternate GTK renderer qualification,
and the two previously recorded native workspace/toolbar harness gaps remain.
Fresh frame-creation baseline comparisons, memory/latency measurements and any
necessary optimization remain last. Other platform host work requires approval
only after GTK completion and qualification.

## 2026-09-15 — Reusable export presets and remembered destinations

Implementation starts from `782ad2cc`. GTK Export now restores named presets with
embedded ICC bytes and all current delivery choices. Save as creates a name;
Update replaces its choices; Remove deletes it. Reset restores a built-in
destination's original recipe. Successful file delivery remembers the chosen
Web / Share, Wide-color, Further editing or Custom destination. Temporary edits
and cancelled/failed exports do not update that memory. Updating a named preset
requires its explicit Update action. Named preset saves are application preference
actions and survive cancelling the export sheet. New/Open settings and editable
project state are independent.

The portable `ExportPresets` model interns profiles separately from recipe data;
matching profile bytes/channels reuse the retained profile, and unused profiles
retire on changes. Limits are 64 names, 16 MiB of retained profile data and 64 MiB
of serialized JSON. These are storage/admission limits, not measured process
memory budgets. GTK reads, validates actual ICC channel/CMM support and writes on
workers. Its separate `export-presets.json` is atomically replaced only if the
expected prior library still matches disk; a stale window reports a conflict
instead of overwriting newer choices. Missing/corrupt profile references cannot
silently become sRGB. Source profile-library files are not dependencies of saved
presets. Restoring a builtin clears the custom-ICC slot; choosing Custom ICC then
requires an explicit profile selection.

The shared export recipe now permits an interned profile reference for storage,
while normal rendering/host recipes retain their typed `ExportProfile`. No
alternative output path or settings migration was introduced. GTK controls reuse
one recipe reader and application path, including size, quality, profile/depth,
matte, intent/BPC and dithering. Preset actions sit directly below the selector.

Validation artifacts are under `artifacts/color-m2/`. No benchmarking or
optimization was performed; test durations are correctness-run durations. Other
platform hosts were not integrated or built.

- `export-presets-shared-qualified-tests.log`: 5 tests passed, including recipe
  validation/resizing, immutable snapshot capture, named/remembered round trips,
  profile reuse/retirement, duplicate names, storage limits and atomic rejection.
- `export-presets-store-tests.log`: 1 native storage unit test passed. Exact ICC
  bytes survive reopening; stale or corrupt files cannot be overwritten through
  the normal update operation.
- `export-presets-native_export_sizes_preserve_master_and_release_cancelled_dialogs.log`:
  1 passed (20.43 s), with resizing, actual-file preview comparison, JPEG
  disclosure, unchanged master and repeated cancelled-sheet retirement.
  Executable `export-presets-gtk-checked-tests` SHA-256:
  `675a85c93d2091e3fec03beb416d874d12f4370a614b02bc659c9e386d12513a`.
  Subsequent changes place preset actions beside the selector and fix the
  builtin/custom profile restoration described above; final native runs below
  exercise those changes.

Final executable `export-presets-gtk-accepted-tests` SHA-256:
`f8bcedda51bbcef322d3ce312df2fc88d1f57dd2f0695a3c84d2ed6962445b46`.
Its eight changed/new Rust source hashes match the final implementation. Native
logs with prefix `export-presets-accepted-` report the preset journey passing in
8.79 s and `native_document_files` passing in 37.96 s. The corresponding GTK/FFI
production check passed in 1.00 s. `export-presets-provenance.json` records the
source hashes and 42 build/run artifacts. The layout screenshot
`export-presets-native/2157715/saved-presets.png` was visually inspected; all four
preset buttons fit beside the selector and output dimensions remain pinned.
The final run also saves its screenshot under `export-presets-native/2161881/`.

The first native file regression assumed every sheet discarded previous choices;
it now explicitly invokes Reset before its codec matrix. Its next run exposed a
real restoration defect: a builtin could occupy the custom-ICC slot. That defect
was fixed, preserving disabled export after choosing Custom ICC and after
cancelling or failing profile selection. Both failed logs remain. The first test
build also had a test-only widget/button type mismatch, fixed before execution.

The focused preset journey covers restoring a retained ICC, Save as/Update,
reopening, cancelled file selection, successful resized PNG delivery, remembered
destination, Reset/Remove, and unchanged serialized master and New/Open settings.
The file regression additionally covers native saving/reopening, autosave, surface
recovery, all three delivery formats and custom RGB/gray/CMYK profiles. Tests run
on the isolated Mutter 1600×1000@120 harness with GTK 4.22.4/libadwaita 1.9.3,
GSK Vulkan and the local RTX PRO 6000 Blackwell Max-Q / NVIDIA 610.57.04. The CMYK
fixture remains `/usr/share/color/icc/krita/cmyk.icm` with the hash recorded above.

Reproduction: build with local JPEG headers in `PKG_CONFIG_PATH`; run
`cargo test -p layer-ui --offline export -- --test-threads=1`,
`cargo test -p layer-linux --offline --no-run` and
`cargo check -p layer-linux -p layer-ffi --offline`. Use the GTK test executable
through `tools/performance/gtk-raster.sh` with full test names
`workspace::tests::export_resize::native_export_presets_save_update_remove_reset_and_remember_after_delivery`
and `workspace::tests::native_document_files`; set `LAYER_TEST_CMYK_PROFILE` as
above. Saved `*-exact` wrappers prevent accidental substring selection.

Remaining functionality and qualification are resolution metadata, TIFF/HDR-input
policies, aggregate job scheduling/cancellation/resource budgets, coarse-first
source/display and zoom quality, full tool/filter precision and large-document
qualification, monitor/profile/alternate-renderer checks, and the existing
workspace/toolbar native harness gaps. Fresh baseline comparisons and all
performance/memory qualification and optimization remain last. Other platform
hosts still require approval after GTK completion and qualification.

## 2026-09-15 — Preserve and select physical resolution metadata

Implementation starts from `30a7423a`. Native documents and retained sources now
carry optional physical density as two positive rational values and an explicit
inch/centimetre/metre unit. Native save/reopen preserves these values exactly;
photo Open adopts the source density, while Place retains it with the source.
Source rasterization and document color conversion preserve the metadata.
Orientation normalization swaps density axes whenever it swaps pixel axes.
Resolution is independent of color, precision and pixel dimensions.

GTK Document Properties reports the master density. Export offers From master,
Custom (1–65535 pixels per inch) or Omit, with physical dimensions and density
shown beside the resolved pixel size. Presets retain the choice. From master
keeps density when resizing, so the physical size changes with the pixel count;
choosing Custom changes metadata without resampling. A single complete-recipe
validator now controls export availability, replacing the partial profile/matte
validator. Metadata-only changes do not restart pixel preview work.

The shared row writers accept resolution explicitly. PNG writes integer pixels
per metre in [pHYs](https://www.w3.org/TR/png-3/#11pHYs); the maximum rounding error
is half a pixel per metre (0.0127 ppi). Explicit PNG physical metadata takes
precedence over duplicate Exif density. TIFF uses its rational X/YResolution and
ResolutionUnit tags ([LibTIFF tag documentation](https://libtiff.gitlab.io/libtiff/functions/TIFFSetField.html)).
JPEG writes rounded whole-unit JFIF density where JFIF applies and a minimal Exif
APP1 IFD with rational density and normalized orientation. JFIF's integer fields
and inch/cm units follow the [JFIF 1.02 specification](https://www.w3.org/Graphics/JPEG/jfif.pdf).
On JPEG import, supported rational Exif density takes precedence over rounded
JFIF density. The C wrapper's existing bounded marker writer now handles APP1
and ICC APP2; its setjmp boundary remains wholly in C.

No arbitrary input Exif block is copied to exports: stale dimensions, thumbnails,
camera settings and orientation tags are not reattached after edits. Unknown
physical units and zero/invalid print density do not invent a physical size;
otherwise supported pixels still open. This work does not add non-square-pixel
editing or complete Exif/XMP preservation, nor claim full Exif-file conformance.
Density values outside a selected container's representation produce an explicit
export validation error rather than being silently clamped.

Artifacts are under `artifacts/color-m2/`. No benchmarks or optimization ran.
Correctness results:

- `resolution-core-tests.log`: rational/unit/rounding/boundary test passed.
- `resolution-ui-tests.log`: 5 shared export/preset/snapshot tests passed.
- `resolution-photo-checked-tests.log`: 18 photo tests passed, 2 existing ignored
  tests not run (11.64 s). Covers both depths, exact PNG/TIFF samples, fractional
  TIFF/JPEG density, unchanged JPEG decoded samples, raw JFIF bytes, PNG/Exif
  precedence and axis normalization, grayscale and CMYK JPEG metadata. Existing
  codec I/O panic tests deliberately panic inside callbacks and verify errors
  return safely. The first build missed the new metadata argument at one such
  test-only codec constructor; it was fixed before execution.
- `resolution-native_new_presets_and_profiled_photo_master.log`: passed (21.89 s).
  A ProPhoto16 TIFF with 300.5×300 ppi completes Open, brush edit, native Save,
  reopen and TIFF delivery. Original file bytes, retained source and exact
  canonical editable archive remain preserved, including resolution.
- `resolution-native_export_sizes_preserve_master_and_release_cancelled_dialogs.log`:
  passed (17.60 s). The 600 ppi master exports a fitted PNG preserving density,
  TIFF at explicit 300 ppi, and enlarged JPEG with density omitted. Existing
  preview/sample/profile/lifetime and master-state assertions still pass.
- `resolution-native_export_presets_save_update_remove_reset_and_remember_after_delivery.log`:
  passed (8.64 s), including restoration and persistence of a custom 240 ppi
  recipe, successful delivery, cancellation and independent New/Open settings.
- `resolution-production-check.log`: GTK/shared FFI passed (1.69 s). Other
  platform hosts were not integrated or built.

Native runs use the private Mutter 1600×1000@120 harness, GTK 4.22.4,
libadwaita 1.9.3, GSK Vulkan and RTX PRO 6000 Blackwell Max-Q / NVIDIA 610.57.04.
Exact executable `resolution-gtk-tests` SHA-256:
`d11b9834c53bcd7ea9b63f7ef5440ce196a4d02dae5786142431238e73f2f4b6`.
Its source manifest includes the C codec and all 28 changed/new Rust sources;
all hashes match. `resolution-provenance.json` records those and 20 artifacts.

`resolution-external-identify.log` records independent ImageMagick inspection:
513×257 TIFF16 at 300.5×300 ppi, 75×50 PNG16 at 236.22 pixels/cm (approximately
600 ppi), 192×128 TIFF16 at 300 ppi, and 300×200 JPEG8 with undefined physical
units. ImageMagick displays a 72×72 fallback for the last file; it is not a stored
72 ppi tag, and the production decoder reports no physical resolution. All retain
ICC profiles. This is decoder inspection, not manual external-editor or print
qualification. `export-resize-native/2175896/tif-sizing.png` was visually inspected;
its physical dimensions and resolution remain visible above the action buttons.

Reproduce using local JPEG headers in `PKG_CONFIG_PATH`: run `cargo test -p
layer-core --offline image_metadata::`, `cargo test -p layer-ui --offline export`,
and `cargo test -p layer-color --offline photo:: -- --test-threads=1 --nocapture`
with `LAYER_TEST_CMYK_PROFILE=/usr/share/color/icc/krita/cmyk.icm`. Build GTK tests
with `cargo test -p layer-linux --offline --no-run`, then use
`tools/performance/gtk-raster.sh` with the three full names under
`workspace::tests::new_photo::` or `workspace::tests::export_resize::` above.
The saved exact wrapper prevents substring selection.

Remaining GTK work is TIFF/HDR-input policy completion, combined job scheduling,
cancellation and resource budgets, coarse-first source/display and final zoom
quality, full tool/filter/large-document precision qualification, managed-display
and alternate-renderer qualification, and the previously recorded workspace and
toolbar test gaps. Fresh baseline/frame-creation comparisons, memory and latency
measurements and necessary optimization remain last. Other platform host work
requires approval after GTK completion and qualification.

## 2026-09-15 — Explicit SDR input and TIFF variant policy

Implementation starts from `4ee46c30`. The agreed SDR journey permits a specific
interpretation choice **or error** for unidentified HDR; HDR editing/rendition
import is journey 8 and milestone 4. Therefore this checkpoint closes the current
SDR policy with actionable rejection, rather than adding a gain-map rendition
selector. Recognized HDR gain-map JPEGs and MPF containers no longer refer users
to a choice the application does not provide. Open/Place/Paste keep the current
project unchanged and advise creating a separate SDR image in another editor.
These checks do not qualify every possible HDR container or reconstruct gain maps.

TIFF's declared initial subset is one image, interleaved unsigned 8/16-bit
RGB/gray (optional explicitly unassociated alpha) or profiled CMYK without alpha.
The reader accepts supported strips/tiles, byte orders and classic/BigTIFF
containers. Planar, associated-alpha, floating-point and multi-page TIFF remain
explicitly unsupported. The installed tiff 0.10.3 chunk-reader documentation
still describes incomplete planar reads; the application rejects that layout
before requesting chunks. Multi-page and planar errors now give a concrete
separate-page/interleaved export route. These are supported-subset limits, not
pending promises of a page selector or native CMYK/HDR editing.

Output remains uncompressed, interleaved, single-image classic TIFF with one-row
strips. Before writing metadata or requesting pixels it conservatively accounts
for payload, strip tables/alignment, ICC and fixed tags, rejecting output beyond
the 32-bit file-offset limit. [LibTIFF's BigTIFF design](https://libtiff.gitlab.io/libtiff/specification/bigtiff.html)
confirms the classic/64-bit distinction. PNG or smaller dimensions remain
available. This avoids spending time creating a partial multi-gigabyte output;
it does not add BigTIFF delivery. Existing row cancellation/atomic publication
remains in use.

All artifacts below are under `artifacts/color-m2/`; no performance qualification
or optimization was performed.

- `tiff-policy-checked-tests.log`: 2 tests passed (0.21 s). Eight RGB16 cases cover
  classic/BigTIFF × uncompressed/LZW/Deflate/PackBits strips, exact samples and
  ICC. Independent valid planar and multi-page/associated-alpha/float fixtures
  exercise explicit rejection. A 32768² RGBA16 output is rejected before the
  provider runs or any output bytes are written.
- `tiff-policy-external.log`: ImageMagick independently recovers all 1,683 U16
  samples exactly from each of the eight files. It also generated big-endian LZW
  and tiled ZIP fixtures; current `photo_sources roundtrip` reads and exports
  those to PNG with exact source/profile assertions, then ImageMagick verifies
  every output sample. Fixtures and hashes are retained in `tiff-policy-fixtures/`.
  The existing runner was used solely for interchange correctness; its incidental
  timings/RSS are not benchmark evidence.
- `input-policy-jpeg-tests.log`: both strict marker tests passed, including
  recognized ISO/Adobe/Google gain-map declarations, MPF, late scans and ICC
  sequence errors.
- `input-policy-native.log`: 1 passed (3.63 s). Actual GTK Open, Import Image as
  Layer and Paste Image as Layer reject declared HDR/MPF JPEG inputs, report the
  specific error and preserve exact serialized document state. Fixtures are
  ordinary valid JPEGs with injected recognized declarations, not a full HDR
  conformance corpus.
- `input-policy-production-check.log`: GTK and shared FFI passed (1.62 s).
  Other platform hosts were not integrated or built.

Native executable `input-policy-gtk-tests` SHA-256:
`031cd2509189046f48535abb79c43a4d5094bf0c81561d4d99481c81db4553f3`.
It runs on the same isolated Mutter/GTK/GSK Vulkan reference setup recorded in
the preceding checkpoint. Its source manifest records the exact implementation.
`input-policy-provenance.json` records build/run and fixture hashes.

The first TIFF fixtures were invalid: the pinned encoder enables compression
inside `write_data`, so direct `write_strip` produced raw bytes with an LZW tag;
and simply retagging interleaved offsets as planar did not produce valid plane
counts. The corrected fixtures use the documented compressed writer and an
independently constructed three-plane TIFF. Failed and diagnostic logs remain;
there was no production decompression change or acceptance-tolerance relaxation.

Reproduce with local JPEG headers in `PKG_CONFIG_PATH`: run `cargo test -p
layer-color --offline photo::tiff_policy_tests:: -- --test-threads=1 --nocapture`
and `cargo test -p layer-color --offline photo::jpeg_markers::`. Build GTK tests,
then run the exact executable through `tools/performance/gtk-raster.sh` with
`workspace::tests::place_source::native_unsupported_hdr_and_multiple_picture_inputs_preserve_the_document`.
External fixture commands and the current example's identity results are retained
in the corresponding session/log records.

Remaining GTK qualification concerns cross-workflow job cancellation and resource
accounting, the complete tool/filter/precision and recovery matrix, the existing
workspace/toolbar test gaps, and managed-display/alternate-renderer coverage.
Fresh frame-creation baselines and large-document/multiple-document memory and
latency measurements remain last. Coarse-first source/display work and further
zoom-performance optimization belong to that final measured phase where needed;
missing measurements cannot be counted as passing budgets. Other platform work
still requires approval after GTK completion and qualification.

## 2026-09-15 — Open cancellation and closing correctness audit

After `5018090c`, Open now offers Cancel during photo/native-project decoding.
It shares Place/Paste's cancellable file reader and awaits worker acknowledgement
before releasing the document request. Cancellation discards the candidate before
window publication. This does not promise instantaneous interruption of an
already-running codec/CMM call; large-file cancellation latency remains measured
qualification work.

`open-cancel-checked-gtk-tests` SHA-256
`ac45564bfe28e5cc09a97c45c796ca1f6c114120da4d73fac2039587ba478086` passes all
14 default tests and four isolated native cases: repeated Open cancellation and
successful reopening (4.78 s), New/photo/master/export (12.16 s), retained
Place/Paste/history/cancellation (4.89 s), and wide-color GPU failure/recovery
(5.24 s). Cancellation preserves exact serialized current-document bytes and
publishes no new window. The GTK/shared-FFI production check passes (0.95 s).
The first native-test build had three test-only API/name errors, corrected before
capture. Logs, exact wrapper and five matching source hashes are recorded in
`artifacts/color-m2/open-cancel-provenance.json`.

The closing audit also runs the existing affected suites from exact binaries at
`5018090c`, not newly invented operation tolerances. Core (70), color (53), UI
(395), engine (58), and renderer contracts (3) pass; four color performance tests
remain ignored. The shared diagnostic ABI run passes ten cases but fails exact
stroke-cancellation restoration for destination-brush preset 11. Its explicit
readback can precede a deferred raster-restoration frame; this failure is being
investigated and is not accepted as passing or relaxed to a visual tolerance.
The corresponding `sdr-qualification-*` build, executable and run records remain.

These are correctness runs. Some independent suites overlapped on the reference
GPU; their wall times establish no latency or memory budget. The resource audit
confirms one file request and replaceable preview/histogram workers per owner,
512 MiB additional-history admission, bounded capture staging and component
source/paint/display/filter limits. Active edits pin pages beyond the paint-cache
target, and independent windows/workers add allocations. Combined pressure,
scalar-plane residency and workload admission still require final measurement;
component limits alone do not pass the common memory gate.
