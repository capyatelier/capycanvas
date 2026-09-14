# GTK SDR color milestone 2 — implementation and validation

Work begins at `e46f271` on 2026-09-13. Milestone 2 is **in progress**;
this report is not a declaration that the new modes are qualified. Scope is
shared implementation and GTK integration. Other host integration requires the
user's approval after GTK qualification.

Current delivery status: retained source/ICC/output and native tile precision
primitives are implemented, but the complete GTK SDR/photo workflows are **not
enabled or qualified**. Native document edit publication is now connected and
qualified in the headless paint/history fixtures below. Native photo-adjustment
and material references pass, and document-to-view color conversion is explicit.
Remaining tool/effect precision, bounded mutable/composite/filter residency and
mips, managed GTK viewing, color/photo controls and interchange are still required.
Large-photo transforms fail the latency gate. Existing drawing, project files,
diagnostics and GPU recovery continue to receive regression checks; these do not
substitute for qualification of the new workflows. The implementation sections
below distinguish each measured primitive from an integrated user journey.

**Current work order (user instruction, 2026-09-14):** finish functional milestone 2
implementation and correctness/recovery validation first. Further benchmarking,
regression investigation and optimization are deferred to the final qualification
phase. The earlier measured failures remain open and must be resolved before GTK
mode enablement; this sequencing does not waive any performance or memory gate.

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
