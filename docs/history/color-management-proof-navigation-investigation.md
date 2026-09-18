# Web / Android proof navigation investigation

2026-09-17, follow-up to the [port qualification](color-management-web-android-m3-validation.md).
Same Wacom MovinkPad 14 (Adreno 735), Chrome 152, 2880×1800 viewport, 61 MP
sRGB/U8 photo, Chemical proof CMYK target, relative+BPC+black ink. Baseline
source: `683a9d7d`. No browser flags, device power settings or user tabs changed.
Raw reports and the exact experimental scripts are retained locally under
`artifacts/color-m3-web-android/investigation/`.

## Finding

A substantial part of the warmed Web slowdown is avoidable in the shared proof
shader. It is not evidence of an unavoidable WebAssembly speed limit or a missing
WebGPU feature. Replacing dynamic vector indexing and the three-step interpolation
loop with the six explicit tetrahedra improves Web navigation about 27% and lowers
median main-pass GPU time about 30%, with the same Wasm binary, image cache, LUT
bytes, buffer bindings, viewport and document. Returning to the original shader
restores the slower timings.

This identifies a shader-expression / compilation / GPU-execution interaction.
It does **not** establish a particular Tint or Adreno driver bug: generated machine
code, register spills and hardware cache counters were not captured. The rewrite
can change instruction count, bounds checks, register pressure, memory access
scheduling or several of these. It is an actionable performance issue in our
shared shader on this browser/device combination.

Both hosts use the shared wgpu renderer. Native Android uses its Vulkan backend;
Web sends WGSL through the browser's WebGPU implementation. Chromium uses
[Dawn and the Tint WGSL compiler](https://dawn.googlesource.com/dawn), while wgpu's
native Vulkan path uses Naga. Their compilation paths differ even with identical
WGSL. This experiment uses ordinary portable WGSL and the existing storage
buffer; it needs no native-only feature or lower precision.

## Controlled Web experiments

A CDP script wraps GPU object creation, writes, texture copies and submissions in
one explicitly selected test tab. Optional pass timestamps use at most twelve
nonblocking query/readback slots and skip when busy. Queries bracket the actual
`viewport` render pass, with separate records for the main 2880×1800 canvas and
448×320 Navigator. Browser timestamp quantization remains enabled; measured
values here fall on 65.536 µs increments. Chrome documents its
[timestamp-quantization policy](https://developer.chrome.com/blog/new-in-webgpu-120#timestamp_queries_quantization).

Each case has three 361-input pan/zoom/rotation runs. Rates are input/update and
CPU submission cadence, **not displayed FPS or input-to-photon latency**. The
main-pass GPU duration excludes browser compositing and display presentation.
Table ranges describe the three run summaries, not confidence intervals.

| Same Web document and runtime | Updates/s | Main GPU median | Main GPU p95 |
| --- | ---: | ---: | ---: |
| Proof off | 118–119 | 4.65–4.85 ms | 6.62–6.88 ms |
| Original proof shader | 77–80 | 9.04–9.24 ms | 14.35–15.01 ms |
| Proof state retained, shader returns original paint | 118–119 | 4.06–4.13 ms | 6.88–7.01 ms |
| Explicit tetrahedra, identical LUT/data layout | 98–100 | 6.49 ms | 10.68–11.21 ms |
| Original shader, Navigator rendering suspended | 79–81 | 9.18–9.31 ms | 14.55–15.20 ms |
| Original shader, five-float struct loads | 77–78 | 9.18–9.24 ms | 14.48–15.01 ms |
| Explicit tetrahedra plus struct loads | 98–100 | 6.49 ms | 10.75–11.34 ms |
| Original shader repeated after the variants | 77–78 | 9.24–9.37 ms | 15.07–15.27 ms |

The identity-shader case deliberately removes the visible proof effect only for
measurement. It retains the proof state and resources, isolating shader work
from host/Wasm proof bookkeeping. It is not a product mode. The struct-load
experiment keeps the same five scalar floats and 20-byte stride; it does not test
other packing, texture lookup or cache designs. Only the explicit-tetrahedra
optimization is adopted.

Navigator's original proof pass is about 0.131 ms median / 0.197 ms p95. Disabling
it removes one submission per frame but does not close the gap. The regular
Web path submits three command buffers per navigation update, or two without
Navigator. Changing LUT interpolation, rather than combining surfaces, produces
the material improvement.

## Memory and caching

Warm original, identity, optimized and repeated-original runs record **zero new
GPU textures, zero buffer allocations, zero texture uploads/copies and zero
queue buffer writes**. Their only recorded resource creation is eight viewport
bind groups over 361 updates. Small camera/overlay updates still use existing
upload buffers; this is not a claim of zero uniform traffic. The first instrumented
run creates the bounded profiler/readback resources and a few upload chunks;
subsequent runs reuse them. The 5,492,500-byte GPU proof table is retained and
shared with Navigator. Code inspection confirms identity-based LUT reuse.

There are no >50 ms main-thread long tasks in these warm runs. The reported JS
heap estimate stays at 23.1 MB, but it is quantized and excludes substantial Wasm
and GPU memory. This does not prove zero garbage collection. Together with the
shader-only interventions, the measurements argue against app cache eviction,
repeated LUT generation, repeated uploads or JS GC as the primary warm slowdown.
They do **not** rule out GPU hardware-cache behavior or driver memory management.

The earlier whole-browser memory peak (2231 MiB PSS) cannot be compared directly
with the native process peak (765 MiB): browser totals include multiple processes
and other tabs, and neither total fully measures GPU residency. Cold import,
recovery/autosave, low-memory and multi-document behavior remain separate concerns.

One native diagnostic attempt was rejected before rendering: JPEG estimated
memory 361,304,064 bytes exceeded the then-current 229,666,816-byte codec budget.
After closing two additional task-created source-test tabs, system available
memory was 2,440,212 KiB and the same import/benchmark succeeded. Unrelated user
tabs were preserved. This is evidence that test-tab memory pressure matters for
admission; it does not explain the repeated original/optimized GPU timing change
within a single loaded document. The failed run remains in
`android-gpu-normal.log`; successful runs use `*-retry.log`.

## Native GPU comparison

Opt-in `proofTiming` in `AndroidPhotoNavigationBenchmarkTest` records timestamps
on the actual shared presenter render pass. It reuses the three-slot bounded
native query helper and never waits for a query. Native timestamps include the
in-pass Navigator; Web measures that surface separately. Native collection retains
at most 256 samples per run, so these are sampled distributions rather than every
presented frame. The native timing helper resolves completed queries in separate
submissions; Web resolves after the pass in its existing submission. Profiling
therefore has different small overheads, and host work areas/scheduling also differ.
The controlled within-Web comparisons above are the stronger causal evidence.

| Native run | Submission cadence | GPU median | GPU p95 |
| --- | ---: | ---: | ---: |
| Normal | 109–111/s | 6.00–6.04 ms | 9.60–9.81 ms |
| Original proof | 105–106/s | 6.36–6.40 ms | 10.49–10.92 ms |
| Explicit tetrahedra | 103–107/s | 6.53–6.57 ms | 10.44–10.95 ms |

All 256 timestamp pairs in every recorded native run are valid. Native performance
is approximately unchanged at this test's precision; the optimization is useful
on Web. No measured GPU-temperature throttling was reported (thermal status 0),
but clock frequencies were not locked. Three brief repetitions do not establish
small regression bounds or generalize to other GPUs.

## Change and qualification

`proof_view.wgsl` spells out the same four corner samples and stable x/y/z tie
ordering without dynamic vector indices. It retains f32 arithmetic, accumulation
order, table layout, gamut distances, alpha handling and all viewing-only policy.
The GPU/CPU parity suite now covers all six tetrahedral orders, tied fractions,
endpoints and the existing zero/tiny/fractional/full alpha cases, in every working
space, both depths and both managed output surfaces. Artwork and export invariance
remain exact; the established one-byte presentation tolerance is unchanged.

The rebuilt Web app confirms the experimental result: 99.5–100.2 updates/s with
GPU timing, 6.49 ms GPU median / 10.75–10.94 ms p95; 100.5–100.9 updates/s with
timestamp collection disabled. These final runs follow removal of the extra test
tabs, so the earlier within-document A/B/A remains the isolation experiment.
The fresh instrumented run allocates profiler/staging resources; its second run
grows staging by 16,656 bytes, and the third allocates nothing. Untimed runs also
allocate nothing and have no recorded long tasks or GPU errors. Their JS heap
estimate is 26 MB, still not a total-memory measurement.

Expanded uniform and shadow-grid hardware GPU parity tests pass (70.87 s and
194.35 s). The two existing bounded GPU timer tests also pass. The rebuilt tablet
Web setup → compare → edit → save/reopen → export and GPU recovery journey passes;
native Android's full journey including the Web-created portable P3/U16 file
passes in 33.917 s. Raw logs retain a missing-CDP-hook measurement attempt and a
packaged test's missing-fixture 404, neither of which produced performance data.
The final APK's stricter timestamp-validity benchmark also passes (23.735 s),
with 104–107 submissions/s and 6.52–6.70 ms GPU medians. The final static package
passes both visible desktop Chrome and connected-tablet Chrome proof journeys,
including recovery, without the diagnostic hook. Headless Chrome returned flat,
identical canvas screenshots and failed its visual-change assertion; headless
presentation remains unqualified rather than weakening that assertion.

Reproduction uses the build/fixture setup in the
[review guide](../development/color-management-m3-web-android-review.md). Add
`-e proofTiming true` to the native photo benchmark to collect pass timings.
`investigation/tooling/` preserves the exact CDP hook, navigation loop and shader
variant scripts used here. They require the selected test tab and its pre-startup
hook, and are diagnostic artifacts rather than shipped application code.

The remaining Web cadence gap, cold navigation stalls, proof preparation latency,
129³/61 MP combinations, low-RAM devices, other browser/GPU combinations and
physical presentation latency are not resolved by this investigation. A browser
runtime or driver cost may remain; these results do not justify declaring that
remaining difference fundamental or blaming Wasm arithmetic.
