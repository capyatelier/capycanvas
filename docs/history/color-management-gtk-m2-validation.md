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

Remaining prerequisites before enabling large integer16 documents:

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
