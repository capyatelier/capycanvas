# GPU raster benchmarks

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

## What is measured

`layer-bench` submits 15 legacy and 10 painter-focused 4096×4096 workloads
through the public C ABI. Every scenario has at least 32 visible paint layers
and every frame contains eight simulated coalesced pen samples.

It reports two time boundaries:

- **submit**: event validation, queueing, shared brush dynamics/contact
  generation, wgpu command encoding, uploads, and queue submission; this is the
  non-blocking production call;
- **completed**: the same work plus a benchmark-only wait for that exact GPU
  submission, isolating the full completed GPU workload.

Initialization, shader/pipeline creation, target allocation, brush selection,
layer creation, and PNG export stay outside the measurement. Each repetition
creates a fresh canvas, runs one real stroke and undo to prime the exact
pipeline and GPU clocks, then contributes every measured frame to the aggregate
distribution. The release painter record uses three repetitions per brush.

## Workloads

| Scenario | Workload |
| --- | --- |
| G‑Pen inking | Two long pressure-varying analytic strokes |
| Pencil shading | Twelve textured hatching strokes |
| Large eraser | 360/620 px analytic erase over textured underpaint |
| Large paintbrush | Three 520–880 px rotating textured strokes |
| Soft airbrush | Low-flow analytic coverage |
| Anchored-grain chalk | Mask tip plus canvas-locked grain |
| Flat marker | Directional high-aspect coverage |
| Scatter spray | Seven particles/contact with spatial and count jitter |
| Dual texture | Two transformed tips plus anchored grain |
| Multiply glaze | Destination-read blend mode over underpaint |
| Smudge pickup | Pull, blur, and zero loaded paint over underpaint |
| Wet round Oklab | Pickup, dilution, deposition, jitter, and perceptual mix |
| Liquify push/twirl | Inverse-mapped deformation over underpaint |
| Layered composite | 34 paint layers with opacity, ink, texture, and wash |

The painter suite uses four large, pressure-varying strokes in one coherent
coral/indigo/gold/teal palette over localized color swatches:

| Scenario | State path exercised |
| --- | --- |
| Textured Flat Filbert | advanced dry build-up |
| Dry Scumble | stroke-uniform coverage only, thresholded grain |
| Pastel Block | advanced dry build-up with dense paper grain |
| Transparent Glaze | deposited wetness only |
| Opaque Gouache | spatial reservoir + wetness |
| Watercolor Wash | coverage + R8 wetness + capillary transport + live edge |
| Wet Watercolor | coverage + stronger wet transport + R8 wetness live edge |
| Loaded Oil Mixer | spatial reservoir + wetness |
| Palette Knife | spatial reservoir + wetness |
| Natural Blender | ordered smudge advection |

This gives every new state primitive an independent path: coverage-only,
wetness-only, spatial reservoir, same-layer watercolor advection, and smudge
advection are timed directly. The watercolor cases also time the bounded
conductance-gated transport pass. Watercolor's edge is part of ordinary
composition; the older optional post-stroke edge remains in the pen-up
distribution only for non-watercolor brushes that request it.

## Workstation result

The painter result is
`artifacts/benchmarks/painter-brushes-4k.md`.
The corrected destination-feedback and liquify result is
`artifacts/benchmarks/complex-brush-interactions-4k.md`.
The current layer-wide watercolor and three-stage capillary-relaxation result is
`artifacts/benchmarks/watercolor-relaxation-4k.md`.
The unchanged-path regression result is
`artifacts/benchmarks/gpu-4k-paint-state-regression.md`,
with the pre-state baseline retained at
`artifacts/benchmarks/gpu-4k.md`. Exact
p50/p95/p99, maxima, over-budget wall-clock samples, work counts, sparse page
counts, and resident bytes remain in those generated reports rather than being
duplicated here.

The three-repeat legacy report contains one 24.804 ms serialized wall-clock
Pencil pen-up sample among only 36 pen-ups. The larger
`artifacts/benchmarks/pencil-pen-up-investigation.md`
measures 144 pen-ups at 2.903 ms p99 and 6,480 total frames at 2.880 ms move
p99. Combined with an unchanged 2.36 ms median, this classifies the isolated
sample as host/GPU scheduling noise rather than a repeatable raster regression;
the raw failed small-sample gate remains visible in the original report.

The state-specific implementation is also bounded structurally. A brush that
opts out keeps the original dry pipeline. The material stage has prepared
color-only, coverage-only, scalar-state-only, and combined target layouts, so
it does not attach or write unused state. Coverage, wetness, and watercolor
wetness pages allocate lazily. This is the optimization claim the timings test;
it is not a claim that a finite benchmark proves a globally optimal shader.

The offscreen suite proves brush and composition execution headroom but does not
include surface acquisition, compositor scheduling, or scanout. Product
acceptance remains input-to-present p99 below 8.33 ms on each supported target.

## Run

On a multi-GPU benchmark host, `LAYER_GPU_INDEX=N` selects the enumerated
adapter used by the offscreen harness. Production frontends instead pass their
platform-selected adapter and device to the renderer.

```bash
cargo run --release -p layer-bench -- \
  --scenario painter --repeats 3 \
  --output-dir artifacts/painter-brushes \
  --report artifacts/benchmarks/painter-brushes-4k.md

cargo run --release -p layer-bench -- \
  --scenario legacy --repeats 3 \
  --output-dir /tmp/layer-legacy-regression \
  --report artifacts/benchmarks/gpu-4k-paint-state-regression.md
```

The first command writes ten explicit-export PNGs and a labeled HTML gallery.
The tracked `contact-sheet.png` is their compact release montage. Use
`--scenario all` for all 25 workloads or a scenario name to isolate one brush.

Functional brush review is intentionally separate from the performance
composition. The first command below renders three isolated marks over an
untouched canvas; the second renders two crossings over separated one-contact
opaque color wells for pickup, mixing, and smudge inspection, plus push/twirl
deformation over a fine grid:

```bash
cargo run --release -p layer-bench -- \
  --brush-validation blank \
  --output-dir artifacts/brush-validation/blank

cargo run --release -p layer-bench -- \
  --brush-validation destination \
  --output-dir artifacts/brush-validation/destination

cargo run --release -p layer-bench -- \
  --brush-validation watercolor \
  --output-dir artifacts/brush-validation/watercolor-v4-capillary-relaxation

cargo run --release -p layer-bench -- \
  --brush-validation transport \
  --output-dir artifacts/brush-validation/watercolor-transport-v3-relaxation
```

The watercolor command writes twelve controlled settings and interaction cases.
The transport command writes a 4-field × 3-rate/distance matrix covering long
and short, broad and narrow conductance, 16–88 px effect radii, wet mixing, and
dry bleed. Each visible update uses three bounded coarse-to-fine GPU stages; it
does not jump pigment directly across the configured radius.
The tracked review outputs and their exact pass criteria are documented in
`artifacts/brush-validation/README.md`.
