# Apple 2000 px brush on a 61 MP photo — 2026-09-20

The fetched optimizations improve this workload, but their initial iPad median
gain is small. Removing a remaining display-tile copy improves both Apple hosts.
Compared with the original local code, final median completed-update time is
approximately **16% lower on Mac and 12% lower on iPad**. The additional correction
alone reduces median time by **7.8% on Mac and 9.8% on iPad** in the final matched
comparison. These percentages use the mean of each pair's run medians.

| Source | M2 Pro Mac median / p99, ms | Physical M4 iPad median / p99, ms |
| --- | --- | --- |
| Original local `599a2ee6`, two runs | 31.17–31.30 / 42.96–52.78 | 37.35–37.88 / 51.70–63.91 |
| Fetched `51e2b005`, four runs | 28.45–28.62 / 38.28–41.12 | 36.74–37.36 / 48.95–50.33 |
| Same main plus display-copy correction, two final runs | **26.17–26.47 / 36.46–36.89** | **33.25–33.32 / 46.19–46.59** |

Ranges show observations, not confidence intervals. Initial run order is local,
main, main, local. The final correction comparison is main, corrected, corrected,
main. The correction also improved two earlier iPad trials to 33.18–33.25 ms
median; those trials are separate from the final table. The original iPad's first
p99 is appreciably higher than its repeat, so the largest tail improvement should
not be treated as a stable percentage.

## Workload and measurement

- Mac: physical Apple M2 Pro with 16 GiB memory. iPad: physical 13-inch iPad Pro
  M4, `iPad16,6`; all retained runs report nominal thermal state before and after.
- Release Rust builds; the same `photo_interaction` replay runs on both hosts.
  The iPad wrapper adds an autorelease pool per update and writes its own results
  in the isolated `art.capycanvas.apple.brushbench.ipad` app.
- 9504 × 6336 sRGB U8 synthetic JPEG, marketed as the 61 MP workload, with an
  empty selected paint layer above the photo and hidden paper. The historical
  recovery project uses unsupported version 4 storage, so a fresh version 6
  project was generated from its original JPEG for **all** compared versions.
  This is not a numerical continuation of the older painted-recovery benchmarks.
- Opaque 2000 px G-Pen, pressure 1, 180 updates of eight samples at 240 Hz,
  deterministic circles, manual 8 ms prediction, 2752 × 2064 output and drawing
  scale 0.2. Each process reads the same input project afresh.
- Explicit 768 MiB retained-display allowance in every run. Native source-cache
  admission follows each implementation's normal policy; the new main admits
  larger source caches. The separate Mac diagnostic records 128 MiB resident
  sources and a 32 MiB upload ceiling. The 768 MiB display override does not
  retroactively replace that source admission snapshot.
- Each update includes input submission, rendering, offscreen managed
  presentation and an explicit GPU-completion wait. Initialization is outside
  the measured window; stroke start/end are included. These are completion times,
  not visible refresh rates or physical Pencil-to-display latency.
- P99 uses sorted index `floor(0.99 * (n - 1))`. Each drawing run has 180 samples;
  360 zoom updates follow. Final zoom medians remain about 1.72 ms on Mac and
  1.86–1.87 ms on iPad.

## Attribution and correction

The first paired main comparison reduces median time by about 8.7% on Mac and
only 1.0% on iPad. The large small-canvas/light-pressure Android gains do not
transfer directly: this full-pressure replay retains five median contact dabs
per update, and the changed generator increases median composition footprint
from 8.32 to 8.98 million pixels. Main reduces median source misses from 92 to 54.
The document does not fit a complete Float32 display pyramid in the fixed
display allowance, so its partial-display path still matters.

A temporary Mac GPU phase probe finds composition dominant: median intervals
are roughly 0.23 ms paint, 2.03 ms prediction and 21.88 ms composition. These are
diagnostic GPU intervals including submission/scheduling gaps, not isolated
shader occupancy or a physical bandwidth bound. Phase probes are absent from
the final binaries. An Instruments capture failed to finish saving; no result
from that unreadable trace is used.

The partial-display compositor previously rendered an eligible tile into a
reusable scene texture, then copied it into the already allocated mip scratch
texture. It now uses the existing direct-composition validation to render into
that scratch texture's first mip. Reduction consumes those completed pixels
without the duplicate copy. Its local viewport remains 256 × 256; partial-edge
reduction, source ordering, masks and fallback cases retain their existing rules.
The scratch texture gains render-attachment usage, with no additional texture
or shader. Unsupported/intermediate compositions retain the copy route.

For every drawing update in the final main/corrected comparison, dab count,
composited pixels, source misses, display submissions and retained display bytes
match on both hosts. Retained display storage is 804,592,448 bytes. This is not
a claim of identical whole-process RSS or driver allocations. Three Mac zoom
updates differ by one source-cache miss; median zoom completion is unchanged.

Two trials were rejected: combining portable layer operations does not address
these Apple devices' hardware Float32 blending path, and scaling partial-display
batches to the admitted upload window reduces submission count without improving
completion time. Neither trial remains in production code.

## Correctness and retained evidence

All runs verify exact native Undo/Redo. On each device, final committed artwork
matches fetched main byte-for-byte. Old versus new main artwork is intentionally
different because `39e9cba3` changes contact generation; this correction does not
change that generator. Cross-device artwork equality is not asserted.

The focused Release Metal checks pass: 23 live-display tests, four mip tests and
six source-cache tests. The new regression also passes with Float32 attachment
blending disabled: 34 passing executions of 33 distinct tests, with one existing
source-decode performance benchmark left ignored. Coverage includes translucent
U16 layers, masks, opacity changes, shifted layers, partial edges, zoom/rotation,
prediction cancellation, discarded commands, history and device replacement.
The new regression compares direct scratch composition against the established
copy route and verifies unchanged retained display bytes. Web compilation and
both final Release replay builds pass. Existing vendored `rav1d` warnings remain.

The [machine-readable results](measurements/apple-main-brush-20260920.json) retain
all main comparison runs, hashes, workload settings and check results. Raw CSVs,
native tile roots, frozen binaries/apps, fixture generator, device scripts,
diagnostic patches, rejected trials and logs are under
`artifacts/apple-main-brush-20260920/`. The ordinary artist apps and their document
containers were not changed during these benchmark runs. Subsequent deployment
and document-preservation evidence is retained separately under `deployment/`.

## Shared-path refactoring and release verification

After the paired comparison, the display cache's separate destination accessors
were replaced by one `composition_target` operation. Complete and bounded
displays now use a single direct-composition call, with document or tile-local
coordinates selected from the admitted display. This replaces the former
complete-only accessor and avoids adding a second composition branch. All runtime
changes are in `layer-render-wgpu`, shared by the native and Web hosts. Intermediate
effects and destination-reading blends still require the existing scratch-copy
fallback; that is a supported operation, not an obsolete implementation.

The refactored code includes the documentation-only main update `24bd0565`.
All 23 live-display tests pass both with normal Float32 blending and with that
capability disabled; all four mip tests and the Web build check pass too. The
normal iPad Release app builds and its signature verifies. No temporary probes,
experimental batching, alternate shaders or Apple-only renderer branches remain.

Final replay checks preserve the improvement: a quiet-host Mac repeat measures
**26.10 ms median / 37.20 ms p99**, and two physical iPad repeats measure
**33.21–33.90 ms median / 45.32–45.37 ms p99**. An earlier refactored Mac run
overlapped iPad compilation and is recorded separately, not used as the quiet-host
confirmation. Native artwork still matches main exactly per device, every replay
passes exact Undo/Redo, and all drawing-frame work/storage counters match main.
Both iPad runs report nominal thermal state. The machine-readable `refactoring`
section records these separate checks, binaries and source hashes; the original
paired measurements and their original source hashes remain unchanged.

Mac reproduction, after generating the retained fixture:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  cargo build --locked --offline --release -p layer-render-wgpu --example photo_interaction
target/release/examples/photo_interaction \
  artifacts/apple-main-brush-20260920/input.capy OUTPUT.csv 8 768 180 2000 circles
```

The retained `ipad-rust/lib.rs`, `ReplayApp.swift`, `build-ipad.py` and
`ipad-run.py` run the same workload on the connected physical device.
