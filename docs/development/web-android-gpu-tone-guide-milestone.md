# Web and Android GPU illumination guide milestone

2026-09-19, following approval of the GTK GPU guide milestone. Implementation
and hardware qualification were performed in `capycanvas3`. Tablet validation
uses the Huion Kamvas Pad 12 (KP1202, serial `G7DL2S300241`), Android 16,
Mali-G57 MC2. Evidence is in `artifacts/gpu-tone-web-android/`.

## Shared implementation

GTK, Android and Web now use the same Rust snapshot orchestration and
`local_tone.wgsl` compute kernels. `snapshot/tone.rs` exposes asynchronous
preparation for browsers and synchronous wrappers for native workers. Source
regions are composed and reduced directly on the GPU. Interactive preparation
reads back only the 16-byte range summary, then publishes an immutable GPU
guide to the presenter. CPU export codecs download the bounded guide when
needed; their output-pixel readback remains separate from guide analysis.

Web no longer reads the whole image into a CPU illumination worker. Its native
WebGPU pipeline compilation, queue completion and mapping are asynchronous.
Concurrent captures await one cached pipeline preparation. Export workers
receive the bounded GPU-produced guide alongside their existing pixel buffers.
Resized output samples it in the original guide's document coordinate system.

Android publishes a GPU buffer directly, without downloading and uploading
the guide. Kotlin owns debounce/lifecycle scheduling, Rust owns snapshot and
publication validity, and the render owner binds the immutable handle.

Both hosts retain compatible illumination across paint edits and Undo. A new
contact immediately cancels pending analysis. No candidate may publish while
the document is busy, after cancellation, against another revision, or on a
different GPU owner. New documents and changes to geometry/color interpretation
discard incompatible guides before rendering. Publication counters count
accepted results, independently of requested generations. Routine cancellation
does not surface as a preparation error.

The existing 180 ms debounce and host polling remain. The source analysis
still visits the immutable whole document after idle, in bounded 512-square
regions; this is not persistent dirty-tile statistics. Drawing uses the tile
renderer and the previous illumination on each frame. The globally dependent
pyramid is rebuilt after drawing, rather than on every refresh.

## Huion Float32 compatibility

The Huion supports Float32 sampling, storage and render attachments, but does
not expose `FLOAT32_BLENDABLE`. Previously the app rejected its working tiles.
The renderer now selects a shared compute blending path on such devices:

- Existing destination compute handles direct dry brushes.
- Source-over, erase and maximum kernels blend Float32 composition, masks,
  wetness and thumbnails with bounded writes.
- Composition retains tile-sized scratch; immutable pipelines are shared per
  device and mutable scratch remains private to each renderer/capture.
- Float32 color and coverage are preserved, including negative and above-white
  samples. No Float16 working-tile substitution is used.
- GPUs with hardware Float32 blending keep their existing path.

The guide geometry header is copied as integer bytes instead of passing integer
bit patterns through floating-point shader values. This avoids subnormal
flushing on mobile GPUs without changing guide sample arithmetic.

The portable blending code and guide shaders live in `layer-render-wgpu` and
therefore serve Vulkan and browser WebGPU from one implementation. Metal and
D3D12 can use the same wgpu code; physical Apple/Windows qualification is not
part of this milestone.

## Validation and measurements

The following completed before or during upstream integration:

- Huion GPU guide numerical suite: all 24 vectors in four RGB spaces, including
  fractional coverage, transparency, wide HDR range, odd sizes and tile edges.
  Largest reduced log-luminance/illumination errors were approximately
  0.000090 / 0.000033 EV; maximum coverage error was 0.000058.
- Huion native HDR editing: negative and above-white paint, exact Undo/Redo,
  SDR controls, thumbnails, master save/reopen, SDR/PQ/EXR delivery, GPU restart,
  recovery and Activity recreation. The integrated build passed again.
- Huion native retained-guide instrumentation: actual Android stylus input,
  GPU/CPU guide comparison, immediate cancellation, rejected late publication,
  refresh after release and Undo. The 512 × 384 fixture had maximum guide errors
  below 0.000001 and identical peak.
- Desktop forced portable blending: 21 snapshot tests, five native material
  selections, 12 native-edit tests, and the independent source-over/erase/max
  kernel oracle. Two explicit large-document stress tests remained ignored.
- All five row-output tests, including reuse of the original document guide
  when resizing the source for an output preview.
- Desktop Web HDR journey, including exports, thumbnails, master preservation,
  GPU recovery and retained illumination. No browser runtime exceptions.
- The same full Web HDR journey passed in ordinary Chrome 143 on the Huion,
  with its real touch/pen dispatch, GPU restart, SDR/PQ/EXR previews and exports.
  The test harness now follows the compact application menu on narrow tablet
  headers. No browser runtime exceptions occurred. The fixture's post-release
  guide refresh took 3.67 seconds; drawing retained its previous guide.
- All 21 forced-portable snapshot tests passed again after upstream integration.
- Integrated GTK guide drawing/cancellation/Undo/tab ownership regression:
  passed, with 368 ms observed pen-up refinement.

Desktop Web also ran a 3840 × 2160 viewport on a 120 Hz virtual display using
the local NVIDIA GPU. The two three-second pen contacts kept their guide
publication unchanged. Host callback intervals were 8.3 ms median / 8.4 ms p99;
host frame work was 0.5–0.6 ms median / 1.0–1.3 ms p99. Refresh after release
took 502–531 ms. These are browser callback/CPU measurements, not independent
scanout or GPU timestamps, and do not establish 4K120 on the Huion.

The final Huion native 3840 × 2160 document test used its 1600 × 2400 surface
at approximately 90 Hz, with SDR proof explicitly selected. Pen-contact frame
intervals were 11.0 ms median and 29.7–33.0 ms p99; host CPU frame time was
7.1–7.3 ms median and 20.1–20.6 ms p99. The old guide remained bound, with
replacement observed after approximately 2.07–2.13 seconds. Peak process PSS
was 822 MiB; tracked canvas storage was 271 MiB. Raw records distinguish
finger contacts that made no edit from pen drawing.
The measurement helper collects diagnostics after release, so its reported
`pen_up_wait_ms` excludes that collection overhead. The Huion is not qualified
for sustained 120 Hz rendering by these results.

Huion Chrome's 4K-document workload also passed retention, late refresh,
histogram cancellation, concurrent save/analysis and output cancellation.
An initial run with full-system PSS sampling observed 1.51 GiB for Chrome's
processes. That sampling was intrusive, so pacing was repeated with
`LAYER_HDR_MEMORY=off`. In this repeat, pen-contact callback intervals were
33 ms median and 66–77 ms p99; host frame work was 6.4–6.6 ms median and
13.7–15.3 ms p99. Post-release guide refresh took 2.37–5.48 seconds. The browser
viewport was 800 × 1080 CSS pixels at scale 2; this is a 4K document, not 4K
scanout. The benchmark now timestamps guide readiness before querying memory
and waits for a new accepted publication after a real stroke.

The shared GPU algorithm and retained preview are qualified on this Huion;
faster mobile drawing and refinement remain performance work. Potential next
steps are persistent dirty-tile source statistics, less transient composition
work, and profiling WebGPU queue/host scheduling on this Mali driver. The
global pyramid still cannot be made strictly local to a changed tile.

## Reproduction and review

The isolated Android package is `art.capycanvas.gputone`, labeled
**Capy Canvas GPU Proof**. It does not replace the regular installed app.
The APK and Huion-only launcher are in `artifacts/gpu-tone-web-android/review/`.
The tested input is also installed as `Downloads/Capy GPU proof HDR.png` on
the Huion. Open it in the review app and select SDR in Proof to exercise the
mapped preview on its HDR-capable display.

```sh
ANDROID_HOME=/home/babymastodon/Android/Sdk \
  apps/layer-android/gradlew -p apps/layer-android \
  :app:assembleDebug :app:assembleDebugAndroidTest \
  -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.gputone \
  '-PcapyAppLabel=Capy Canvas GPU Proof'
```

Install both APKs on the Huion, supply an independently encoded PQ PNG as
`-e hdrFile /data/local/tmp/capy-gpu-tone-pq.png`, and select
`AndroidRasterTest#gpuToneRetainsPreviewAndRejectsLatePublication` or
`AndroidRasterTest#hdrEditingProofDeliveryAndRecovery`. The retention test
explicitly selects SDR proof, including on HDR-capable displays.

Build Web with `bash apps/layer-web/build.sh`. Serve `apps/layer-web`, reverse
that local port to the Huion with ADB, and forward its Chrome DevTools socket.
Run `proof-tablet.test.mjs TEST_TAB_ID hdr OUTPUT_DIRECTORY` with
`LAYER_CDP_URL` and `LAYER_TEST_ARTIFACTS` set. Select `hdr-performance` and set
`LAYER_HDR_WORKLOADS=sparse4k`, `LAYER_DEVICE_SERIAL=G7DL2S300241` and `ADB` for
browser performance and process memory evidence. The test targets one explicit
tab and uses ordinary Chrome WebGPU, without experimental browser flags.

GTK reproduction follows the previous
[GTK milestone](gtk-gpu-tone-guide-milestone.md). Set
`CAPY_GPU_NO_FLOAT32_BLEND=1` for native headless tests to exercise the portable
path even on a desktop GPU with hardware Float32 blending.
