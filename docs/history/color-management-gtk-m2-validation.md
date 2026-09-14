# GTK SDR color milestone 2 — implementation and validation

Work begins at `e46f271` on 2026-09-13. Milestone 2 is **in progress**;
this report is not a declaration that the new modes are qualified. Scope is
shared implementation and GTK integration. Other host integration requires the
user's approval after GTK qualification.

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
