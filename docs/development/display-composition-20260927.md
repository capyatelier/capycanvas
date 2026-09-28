# TCL display-composition prototype evaluation

The prototype demonstrates substantial gains from composing reduced layers and
using compact temporary brush pages while keeping committed paint exact. It is
worth developing further for eligible painting workloads. The fast 2000 px
G-Pen remains too slow for fluid interaction; this change does not solve
full-resolution brush throughput. Filters, transformed stacks and advanced
preview kernels are intentionally deferred.

See the [design and ownership contract](../rendering/display-composition.md).
All work was performed in `capycanvas2`, on the local
`prototype/display-composition` branch. No changes were pushed to main.

## Builds and method

Device: TCL 9465X, Mali-G52 MC2, serial `BC9424FE8557712`, landscape 1920 × 1200.
The source is the supplied Sony JPEG, 9504 × 6336 (60,217,344 pixels, marketed as
61 MP). The smaller control is a 4752 × 3168 resize of the same photograph.

Three baselines must be distinguished:

- **Pinned main A:** `bad5ecd765a63ade8bc84457b53215e17101e788`, using the preserved
  inside-photo benchmark APK from `artifacts/tcl-main-gpen-20260927/`.
- **Worktree control C:** `d3ec11c2858e1c212b2458882fdb4dc3d770ea9e`, the original
  renderer in this checkout, rebuilt from a source archive with the identical
  benchmark harness. This isolates the new engine from earlier brush changes.
- **Prototype B:** the new implementation. Final APK renderer-library SHA-256:
  `5205f5240c3220143824e7d4ee8c736996a40ca591a70207ddeb209aa6801f7d`.
  The control renderer-library SHA-256 is
  `21f9e5c38e84d38393f5c38b57d1086d3636ee43b35decc78886338621b9f305`.

APKs use separate package IDs and optimized benchmark builds. Other test
packages were force-stopped before final comparisons. Synthetic stylus samples
arrive at 200 Hz. Each painting result below is the median of three untraced
five-second input runs, after warm-up and with undo between runs. Prediction is
16 ms unless noted. Ellipse radii are 240 × 140 surface pixels, keeping the large
brush cases inside the photo. Camera, layer opacity and trajectory metadata were
checked across all eleven pairs. Controls were measured in an alternating B/C
matrix. After the navigation/presentation refinements, the entire prototype
matrix was rerun, with a 1000 px control repeat at the end.

The reported rate counts **nonempty updates completed inside the input window**,
not displayed FPS. Traces are separate diagnostic runs. Neither GPU observations
arriving after input nor terminal stroke work are credited as input-window
completions. Resource observations after the stroke and process high-water RSS
are reported separately. These short repeated tests establish a prototype gain,
not a long-duration thermal or power qualification.

## Untraced throughput

Final build comparisons against the same-worktree control:

| Workload | Control updates/s | Prototype updates/s | Ratio |
|---|---:|---:|---:|
| 61 MP, Fit, G-Pen 1000 px | 8.75 | 25.52 | 2.92× |
| 61 MP, Fit, G-Pen 1000 px, prediction off | 10.95 | 28.65 | 2.62× |
| 61 MP, Fit, G-Pen 2000 px, fast | 1.00 | 2.19 | 2.20× |
| 61 MP, Fit, G-Pen 2000 px, quarter speed | 5.55 | 11.36 | 2.05× |
| 61 MP, Fit, Pencil 460 px | 19.72 | 37.41 | 1.90× |
| 61 MP, Fit, Airbrush 460 px | 30.35 | 79.01 | 2.60× |
| 15 MP, Fit, G-Pen 1000 px | 16.72 | 34.07 | 2.04× |
| 61 MP, 25%, G-Pen 1000 px | 20.31 | 28.88 | 1.42× |
| 61 MP, 100%, G-Pen 1000 px | 22.56 | 22.73 | 1.01× |
| 15 MP, Fit, four photo layers, G-Pen 1000 px | 1.40 | 27.88 | 19.97× |
| 61 MP, Fit, G-Pen 50 px | 105.51 | 127.93 | 1.21× |

The stack contains one opaque photograph and three 35%-opacity duplicates, plus
an empty paint layer. Reusing unchanged reduced layers is particularly valuable
there. At 100% the new reduced path is ineligible; the measured result is neutral.
High update rates in the small-brush control are still not scanout FPS.

Pinned main A measured 9.16 updates/s at 1000 px, 1.00 at fast 2000 px,
5.58 at quarter-speed 2000 px, 13.74 for Pencil 460 px, and 31.84 for Airbrush
460 px. The original checkout already improves Pencil over pinned main; that
improvement is not attributed to this prototype. The final 1000 px gain is
2.79× over pinned main and 2.92× over the worktree control.

A composition-only intermediate measured 17.72 updates/s at 1000 px before
compact G-Pen prediction was admitted. The final repeats were 25.52, 26.87 and
25.10; the paired control was 8.73, 8.77 and 8.75. These end-to-end gains are
substantial, but are not the 64× composition-pixel ratio. The final control
repeat measured 8.78, 8.77 and 8.37 updates/s (median 8.77), corroborating the
8.75 control used in the table.

## CPU, latency and memory

For final 61 MP Fit / G-Pen 1000 px, median per-run 95th-percentile input queue
delay fell from about 148 ms to 44 ms. The corresponding update-start gap fell
from about 189 ms to 61 ms. These are host/renderer observations, not physical
pen-to-photon measurements. Observed terminal completion was about 726 ms after
input in C versus 359 ms in B; committed work is not deferred behind an
unmeasured background queue.

The owner thread used about 0.16 of a CPU core in C and 0.44 in B during input.
More completed updates increase CPU activity per second even while individual
updates become cheaper. The algorithm has not made CPU coordination disappear.

| Final workload | Control renderer MiB after stroke | Prototype renderer MiB after stroke | Control / prototype peak process RSS MiB |
|---|---:|---:|---:|
| 61 MP Fit, 1000 px | 2242 | 968 | 3516 / 2275 |
| 15 MP Fit, 1000 px | 938 | 628 | 1693 / 1469 |
| 61 MP 25%, 1000 px | 1869 | 780 | 2947 / 1822 |
| 61 MP 100%, 1000 px | 1778 | 1793 | 2748 / 2829 |
| 15 MP, four photos | 950 | 672 | 2023 / 1355 |

Renderer figures are medians of post-stroke resident observations, not sampled
peak GPU allocation. Process RSS is the maximum reported high-water value across
the three runs, including loading and warm-up. Exact paint/source caches still
account for substantial memory. The cache-ownership tests additionally verify
that the obsolete composite/pyramid and effect-image pixels are released when
the supported reduced path takes ownership.

## Navigation and preparation

The first implementation exposed a preparation regression: rebuilding at a zoom
level boundary produced a 1272 ms maximum update-start gap. The final cache
retains one neighboring level while artwork is unchanged, derives coarser layer
images from existing reduced pixels, and releases the spare on an artwork edit.
An adjacent composed output mip restores the existing trilinear presenter,
avoiding sixteen texture samples per screen pixel. These are part of the final
implementation and its memory accounting.

Two five-second pinch replays crossed the 6.25% level boundary repeatedly:

| Build / run | Viewport callbacks | Median gap ms | 95th-percentile gap ms | Maximum gap ms |
|---|---:|---:|---:|---:|
| Control, first | 299 | 16.49 | 25.66 | 40.90 |
| Control, repeat | 300 | 15.91 | 29.74 | 36.83 |
| Final prototype, first | 299 | 16.48 | 25.89 | 35.31 |
| Final prototype, repeat | 301 | 16.59 | 26.11 | 39.89 |

The final prototype is neutral on this navigation workload. Tests also assert
that revisiting the two levels adds neither source misses nor composition work,
and that visibility/opacity edits cannot restore a stale spare. A never-seen
finer level still requires preparation; cold import time and long multi-level
zoom sweeps were not qualified as performance wins. No exact painting is delayed
until a later navigation or idle interval.

## Quality and exactness

The tests compare compact and committed display pixels with an area reduction of
exact float composition. A gradient fixture at an odd 517 × 259 extent had
mean/max channel errors of 0.0000062 / 0.0117 for prediction and
0.0000020 / 0.00072 after commit.

A 2048 × 1536 crop of the actual Sony photo, starting at document coordinate
(2600, 2200), was tested at Fit with 50, 460, 1000 and 2000 px G-Pen contacts.
Mean linear-channel error stayed below 0.000021; the 99th percentile was below
0.000001. The largest localized difference was 0.0995 for prediction and 0.0858
after commit, at brush edges, before viewport filtering. Sparse edge errors make
the mean and 99th percentile look better than the worst pixel. The TCL rendered
stroke/photo screenshot was also inspected.

Exact RGBA output matched the reference byte for byte for every photographic
case, including in-progress compact tails replayed on demand. Tests cover tail
correction/retirement, odd-edge area weights, layer opacity/reordering/source
replacement, resolution changes, effect fallback and exact project history.
Downsampling and normal blending do not commute for arbitrary high-frequency
alpha and backgrounds; these measurements are not a universal error bound.

## GPU phase diagnosis

Separate traces join GPU observation IDs to renderer generations, including late
observations after input ends. The terminal ID difference was independently five
in all four captures; every selected nonempty update had elapsed and all three
phase timings. `tools/performance/android-brush-gpu.py` performs this join and
refuses incomplete coverage. It also reproduced the preserved old-main analysis.

Mean GPU milliseconds per update queued during input:

| Workload / build | Paint | Prediction | Composition | Total elapsed |
|---|---:|---:|---:|---:|
| 1000 px, C (131 updates) | 29.90 | 9.78 | 65.36 | 109.68 |
| 1000 px, B (371 updates) | 11.94 | 9.41 | 12.83 | 37.31 |
| 2000 px quarter speed, C (86 updates) | 28.11 | 22.38 | 95.95 | 165.21 |
| 2000 px quarter speed, B (155 updates) | 15.22 | 21.79 | 24.84 | 75.54 |

The streams have the same prescribed input but different batching because the
prototype consumes it faster. These are not equal-work shader microbenchmarks:
the lower paint time partly reflects smaller accumulated batches. Prediction
phase time per update is similar; compact prediction also reduces the work later
needed to read/reduce temporary pages during composition. GPU intervals include
queue scheduling gaps. Total elapsed includes work outside the three named
phases; the rows should not be added as independent hardware occupancy estimates.

CPU command finishing and queue submission remain material. Across the three
1000 px traced input windows they total about 3.71 seconds in B versus 1.48 in C,
while B completes many more updates. Neither build incurred a bounded-source-wait
slice inside those input windows. The fast 2000 px case is separately limited by
exact brush work and is not explained by these slower traced workloads.

## Validation result

- Complete renderer GPU suite: **227 passed, 0 failed, 13 explicitly ignored**.
  The ignored tests have their existing opt-in/environment requirements; the
  optional photographic test was run separately and passed.
- Project integration suite: **2 passed**, covering all brush presets, exact
  save/reopen and undo/redo, plus a source-backed stroke at 12.5% versus 100%.
- Final scale/presentation tests: **5 passed**, including partial output-mip
  updates, odd-edge weighting, neighboring-level reuse and empty/visible stacks.
- Optimized arm64 Android build succeeded. Benchmark accounting unit test and
  Python syntax checks passed; `git diff --check` was clean.
- All eleven painting input/camera/layer pairs matched. The final device matrix
  completed without instrumentation failures; screenshots and raw reports were
  retained. This evaluation covers the TCL, not other GPU families.

## Reproduction and artifacts

The local artifact root is `artifacts/display-composition/`. `provenance.json`
records APK/library hashes and source hashes. `comparison.json` preserves
per-run throughput, latency, CPU and memory distributions;
`paired-input-audit.json` verifies the compared input, camera and layer metadata.
Raw JSON, screenshots, environment snapshots, instrumentation output and traces
remain beside each run. These generated artifacts and the photograph are not
tracked in Git.

Build the prototype with an isolated package:

```sh
ANDROID_HOME=/home/babymastodon/Android/Sdk \
  apps/layer-android/gradlew -p apps/layer-android --offline \
  :app:assembleBenchmark -PcapyAbi=arm64-v8a -PcapyOptimize \
  -PcapyApplicationId=art.capycanvas.composition
```

Use the device reservation wrapper before installation or tests. This checkout
predates that tool; `artifacts/display-composition/devices.py` is the copy from
pinned main `bad5ecd765a63ade8bc84457b53215e17101e788`. The benchmark requires the
approved photo staged at `/data/local/tmp/capy-brush-photo.jpg` on the TCL.

```sh
python3 artifacts/display-composition/devices.py --owner capycanvas2 run tcl -- \
  python3 tools/performance/android-brush-benchmark.py artifacts/recheck \
  --adb /home/babymastodon/Android/Sdk/platform-tools/adb \
  --serial BC9424FE8557712 --package art.capycanvas.composition \
  --presets 1 --size 1000 --radius-x 240 --radius-y 140 \
  --duration 5000 --repeats 3 --prediction true
```

Vary `--size`, `--speed .25`, `--prediction false`, `--presets 2,5`,
`--zoom .25`, `--zoom 1`, or `--photo /data/local/tmp/capy-15mp.jpg`.
The stack case adds `--photo-layers 4` to the smaller-photo case. Navigation uses
`--mode pinch --speed .25 --repeats 1`; its separate measurements file records
camera samples and viewport callbacks. Its rate is not the brush completion
metric. Use `--trace` in a separate output directory, then:

```sh
python3 tools/performance/android-brush-report.py artifacts/recheck
python3 tools/performance/android-brush-gpu.py artifacts/traced-recheck \
  --processor /path/to/trace_processor
```

The worktree control was built from `git archive d3ec11c2` under
`target/display-control-source`, with only the benchmark instrumentation copied
from this worktree, using the same release flags and the package
`art.capycanvas.composition_control`. The preserved control APK makes another
copy of the old renderer inside the product unnecessary.
