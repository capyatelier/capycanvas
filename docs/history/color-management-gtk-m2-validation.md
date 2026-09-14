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
render p95/p99 (ms), measured separately from main-thread frame creation:

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
