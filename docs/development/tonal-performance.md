# Photographic tonal selections

## Storage and memory

A 9504 × 6336 selection contains 60,217,344 coverage bytes. Previously these
packed words were serialized as JSON numbers, exceeding the 64 MiB project
metadata limit. Version 7 stores them in indexed, compressed binary chunks;
version 6 remains readable. Shared current/saved/initial masks retain one pixel
allocation. Workspace admission and undo accounting charge that allocation once,
without serializing its pixels or multiplying JSON size to estimate memory.

The GPU caches full-precision linear luminance and alpha for unchanged composite
artwork. An unedited opaque RGB photograph needs only luminance (4 bytes/pixel);
other composites use 8 bytes/pixel. Navigation and selection overlays preserve
the cache; artwork edits invalidate it. The cache is bounded at 512 MiB, with
larger inputs using the existing tiled source path. Texture arrays and bindings
fit portable WebGPU limits without optional device features.

Default tonal selections publish their byte mask directly. Bounds are reduced
per row, and immutable CPU history is captured with one bulk copy. The GPU copy
cache retains at most 128 MiB (or one larger mask); older undo entries upload from
their immutable CPU copy when needed. At 61 MP, the RGB scalar cache, readback and
query parameters retain about 303 MB, plus at most two recent 60 MB GPU masks.
This excludes the document's existing display/source caches and CPU history.

Spatial feathering uses separable Gaussian convolution in 256-row bands with
halos, avoiding a 241 MB float intermediate that exceeded the portable storage
binding limit. Gaussian weights are calculated once per request, and neighboring
pixels share vectorized tap calculations. Sampling uses
64 histogram shards to avoid contention when a large uniform region occupies
one luminance bin. All these paths remain shared by native and Web hosts.

## Huion measurement and targets

Measured on Huion KP1202 / MT8391, Mali-G57 MC2, Android 16, using a generated
9504 × 6336 RGB fixture, release Rust code and Vulkan. No artist image or recovery
file is needed. The source is deliberately compressible; recovery archive size
and compression time are not representative of every photograph.

The acceptance targets for ordinary tonal selection (zero spatial feather) are
a warm mask median below 350 ms and below twice a
same-device streaming control, with full native application warm adjustments
below one second. Both timing paths include immutable CPU coverage capture.
The control performs the same scalar reads, coverage writes, bounds and readback
with trivial arithmetic. It is compiled only into tests.

| Operation | Measured time |
| --- | ---: |
| Previous 61 MP mask path, warm | 3.36–3.65 s |
| Cached 61 MP mask, warm median across final runs | 244–266 ms |
| Same-device streaming control median | 174–232 ms |
| Cold source/cache preparation and first mask | 2.32–2.45 s |
| Native Android application, first mask | 3.50–3.56 s |
| Native Android application, warm adjustments | 878–937 ms |
| Native Android Quick Mask adjustment | 1,010–1,054 ms |
| Native Android recovery write | 153–156 ms |
| 61 MP with 12 px spatial feather, including first shader compilation | 2.97 s |
| 24 MP, seven bands, no histogram / full-image histogram | 155 / 163 ms |

The original baseline used the same RGB pixels with an opaque alpha channel;
final opaque-photo runs use RGB input. Spatial feathering remains substantially
more expensive than tonal softness: exact Gaussian convolution costs `O(Nr)`
taps, while ordinary tonal classification costs `O(N)`. The 12 px feather case
improved from 8.91 s in the first bounded implementation to 2.97 s; it is not an
interactive-rate operation at this image size.

For a theoretical bandwidth floor, an opaque cached selection moves at least
approximately `10N` bytes: `4N` scalar reads, `N` mask writes, `N` bounds reads,
`2N` GPU readback copy traffic and `2N` CPU immutable-copy traffic. For this
fixture that is 602 MB. MediaTek's [Genio 720 factsheet](https://www.mediatek.com/hubfs/MediaTek%20Assets/Pdfs/FactSheet%20Assets/Pdf/Genio%20720%20-%20Factsheet%20(30th%20Jan).pdf)
lists x32 LPDDR4X-4266 or LPDDR5(X)-6400: nominal peaks of 17.1 or 25.6 GB/s give
35 or 24 ms floors. The tablet's actual memory variant/frequency was not verified.
Those ideal floors exclude shader arithmetic, allocation/zeroing, texture access,
driver overhead and synchronization; the measured control is the practical
comparison. Final classification medians were 1.11–1.43× that control.

## Reproduction

- Core: `cargo test --locked -p layer-core -p layer-ui --lib`.
- GPU correctness: `cargo test --locked -p layer-render-wgpu --lib tonal_`, and
  the `selection_options` filter. Use a hardware adapter. Feather tests compare
  fractional and maximum radii with an independent Gaussian reference.
- Native device benchmark: build the renderer test executable using
  `cargo ndk -t arm64-v8a --platform 29 test --locked --release -p layer-render-wgpu --lib --no-run`,
  push it to `/data/local/tmp`, and run `tonal_61mp_performance --ignored
  --nocapture --test-threads=1`. `tonal_preview_latency` measures broad sampling.
- Native application: build/install a separate `capyApplicationId`, then run
  `AndroidRasterTest#tonal61MpRecoveryAndInteraction` and
  `AndroidRasterTest#tonalHdrCoverageAndSamplingOnDevice`. The 61 MP test exercises
  recovery publication/reopening, Quick Mask and undo/redo. Its report stays in
  that isolated app's external files directory.
- [Tonal selection checks](tonal-selection.md#checks) cover GTK, browser and
  Android panel/toolbar interaction. Use Huion Chrome for hardware browser
  presentation; desktop headless Chrome/NVIDIA did not render the screenshot
  fixture correctly in this environment.
