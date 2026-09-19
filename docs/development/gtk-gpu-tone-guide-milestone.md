# GTK GPU illumination guide milestone

2026-09-19. Built on `origin/main` at `7c426431` in `capycanvas3`, with the
subsequent upstream `b79d76d8` HDR-view fix and `ad8d2f57` portable JPEG gain-map
codec incorporated before publication.
GTK is the review boundary: Web and Android host migration starts only after
user approval. The attached Huion/Android device has not been changed.

## Implemented behavior

GTK retains the last compatible illumination guide while drawing and while a
replacement is being prepared. Fresh artwork still goes through the shared
SDR/print shaders on every frame. After the document becomes idle, a background
worker captures an immutable revision and builds its replacement on the GPU.
The existing 180 ms debounce and 100 ms polling cadence remain.

Starting another contact cancels outstanding work. Publication checks both
the exact document key and document idleness. A replacement document, changed
geometry/color interpretation or changed GPU owner cannot reuse the old guide.
Tab switching and window closing drain the cancelled worker. Errors retain any
previous compatible guide; a failed candidate is not counted as published.

The implementation is shared Rust/WGSL:

- `layer-render-wgpu/src/local_tone.rs` owns compute pipelines, scratch planes
  and immutable `GpuToneGuide` buffers. Pipelines are cached per device and
  shared with snapshot workers.
- `local_tone.wgsl` performs coverage-aware log-luminance area reduction,
  range reduction, Gaussian pyramid construction, sampled range remapping,
  Laplacian accumulation and reconstruction. It retains the CPU algorithm's
  768-pixel guide edge, half-EV anchor spacing and Float32 arithmetic.
- Snapshot region capture now accepts a GPU consumer before submission. The
  guide reducer consumes the composed texture directly, including masks,
  placement, blends and filter halos. No full-resolution artwork readback is
  used for guide analysis.
- GTK sends the finished GPU handle to the presenter. This replaces the CPU
  guide upload; explicit viewport captures share the same immutable buffer.
- Native snapshot previews, SDR export and gain-map preparation use the same
  GPU guide builder. CPU codecs download the bounded guide when needed.
  Their separate output-pixel readbacks and CPU output mapping remain.
- `layer-ui::proof_workflow::ToneKey` supplies shared analysis identity and
  compatibility policy. Hosts still own their event loops and device lifetime.

The only analysis readback for interactive presentation is a 16-byte summary
(minimum/maximum log luminance, peak and validation status), used to schedule
the adaptive anchor count. There are no floating-point atomics, subgroup
requirements, Float16 arithmetic or filterable-texture requirements in these
compute kernels. They use four storage bindings, fitting the existing
downlevel device limit. Browser compilation is checked, but its asynchronous
owner/publication migration is intentionally deferred for review.

## Bounds and remaining optimization

Source capture uses at most 512 × 512 regions and shrinks them when the existing
capture budget requires it. Guide scratch/output buffers are included in that
budget. Native workers wait between regions and range anchors, and check
cancellation before submitting more work. This bounds queued analysis work
ahead of a new stroke; it does not create a separate GPU priority queue.

The source pass still recomposes an immutable full-document snapshot after
idle. It does not yet reuse persistent statistics from the live compositor's
dirty tiles. The small pyramid is rebuilt globally because its adaptive range
and coarse levels have image-wide dependencies. This milestone supplies
immediate drawing with retained illumination and asynchronous GPU refinement;
it does not calculate a new, globally current guide every 8.33 ms.

CPU guide construction remains as an independent numerical oracle and for
unmigrated host paths. The existing CPU presenter setter is retained for those
hosts. No Web/Android host code changes are included in this milestone.

## Qualification

Release builds use the local NVIDIA RTX PRO 6000 Blackwell Max-Q GPU, Vulkan,
driver 610.57.04. GTK tests run one per process on an isolated Mutter Wayland
display with the packaged GTK 4.22 runtime and pinned photo codecs.
Evidence is retained locally in `artifacts/gpu-tone-gtk/`.

The first 12-stroke 4K run used a 3840 × 2160 viewport at 120 Hz, a 4096-square
ProPhoto Float32 document with 32 paint layers, a 720-pixel palette knife,
and SDR proof enabled. All contacts retained the same guide. Worker GPU time
was 0.635 ms median / 1.170 ms p99; worker CPU time was 0.648 / 1.162 ms.
Presented-frame intervals were 8.329 ms median / 8.751 ms p99, maximum 10.642 ms.
These are hardware/compositor measurements for this workload, not a guarantee
for every brush, document or display.

The functional drawing test observed approximately 348 ms from pen-up to the
refreshed guide, including debounce, in its first run. It captures actual
presenter pixels during a contact, interrupts an in-flight refresh with a new
stroke, checks unchanged illumination throughout that contact, and verifies
refresh after release and Undo.

The sustained run completed 100 strokes (80 seconds of contact time):

| Measurement | Median | p99 | Maximum |
| --- | ---: | ---: | ---: |
| Worker GPU | 0.611 ms | 1.174 ms | 2.284 ms |
| Worker CPU | 0.619 ms | 1.206 ms | 2.259 ms |
| Presented-frame interval | 8.331 ms | 8.598 ms | 10.755 ms |

There were 9,631 presented frames, averaging 120.005 Hz, with zero missed
refresh slots using rounded interval/refresh-period accounting. Mailbox
presentation superseded 107 submitted frames; these are retained in the raw
records, not counted as presented. Every contact retained guide generation 1.
The functional drawing/tab test settled 353 ms after pen-up, and 369 ms after
the last upstream integration. Both passed in-flight cancellation, Undo,
new-document and reattached-tab checks.

Validation passed:

- GPU guide oracle: all four working RGB spaces; transparent/constant/negative
  luminance inputs; tiny/fractional alpha; hidden RGB; broad HDR ranges; odd,
  thin and tiled dimensions; rejected NaN/invalid alpha; immutable old handles.
  Maximum observed errors were approximately 0.000091 EV in reduced log
  luminance, 0.000033 EV in illumination, and 0.000058 in coverage. Tolerances
  are 0.0003 EV and 0.0001 coverage, accounting for Float32 area arithmetic.
- Masked/filtered composed guide versus the independent CPU oracle, cached
  cancellation and guide scratch budget rejection.
- All 21 native snapshot/export tests, including Float32 master preservation,
  SDR/PQ/EXR output, resize/profile/matte behavior and bounded capture.
- An 18-test GPU regression selection, including the initial numerical guide
  test, startup/cache behavior, GPU filters and native raster paths.
- All 493 shared UI tests after the last upstream integration, including
  document-replacement/retention policy.
- GTK drawing/refresh/tab-switch test; Float16/Float32 close cancellation;
  all five HDR photo/proof-control fixtures including export preview.
- After incorporating the portable-codec milestone, the native snapshot suite
  passed again, as did GTK JPEG/transparent AVIF gain-map preview, flatten,
  export and reopen. Six non-ignored shared gain-map core tests also passed
  (seven separately gated codec tests were not selected in that unit run).
- 60 MP HDR proof controls: 60 updates in 1.13 seconds, maximum observed
  10 ms heartbeat gap 11.93 ms, input-handler maximum 1.30 ms; guide reused
  throughout, Undo restored the recipe and the master remained unchanged.
- Release GTK build and `layer-render-wgpu` wasm32 compile check. The latter
  retains the pre-existing unused native CPU-guide-cache warning; no browser
  integration or physical Metal/D3D12/mobile qualification is claimed.

The 60 MP run uses
`artifacts/color-m4/qualification/timing/local-concurrent-60mp.capy`. The older
`performance/hdr-stress60.capy` fixture was rejected before renderer startup
because it predates the required `headroom` field; it was not modified.

The local review launcher is `artifacts/gpu-tone-gtk/review/launch.sh`. It uses
an isolated settings/recovery directory and the tested GTK/photo libraries.
Enable SDR in Proof to review HDR drawing on an HDR-capable monitor.
The final upstream integration also passed a fresh 12-stroke 4K pacing run;
its raw records and summary are `integrated-4k120.json` and
`integrated-4k120-summary.json` in the evidence directory.

## Reproduction

Build with `cargo test -p layer-linux -p layer-render-wgpu --release --no-run`.
Use the test executable paths reported by Cargo. Hardware GPU tests require
access to the local graphics device outside a restricted sandbox.

The GTK harness is `tools/performance/gtk-raster.sh BINARY FILTER REPORT_PREFIX`.
Set `LD_LIBRARY_PATH` to the packaged GTK runtime and `CAPY_PHOTO_CODEC_DIR` to
`target/photo-codecs/prefix/lib`. The pacing run additionally sets:

```sh
LAYER_DRAWING_HDR=32
LAYER_DRAWING_SDR=1
LAYER_PENUP_STROKES=100
LAYER_TEST_MONITOR=3840x2160@120
```

Select `native_penup_and_following_strokes` for pacing and
`native_gpu_tone_retains_preview_cancels_and_refreshes_after_drawing` for
publication/lifecycle behavior. Raw frame, CPU, GPU, backing and pen-up records
are preserved in the pacing JSON; no frame-rate assertion substitutes for
those records.
