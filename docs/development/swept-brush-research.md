# Swept-brush research track

This branch explores a **new** brush class. It is not a compatibility rewrite
of the existing dab renderer: its strokes may intentionally look different.
The existing dab pipeline remains the fallback for saved brushes and for media
whose appearance depends on ordered simulation.

Follow-up: [live 61 MP / 2048 px Wacom comparison](swept-brush-live-wacom.md)
integrates the experiment into the primary app and measures actual drawing
throughput, remaining stalls, and memory/appearance tradeoffs.

## Question

The current G-Pen and Pencil already use short swept contacts:
`contact.wgsl::evolving_contact` projects onto the segment between consecutive
poses. The material compute path loops over a page's contact range, keeps the
result in registers, and stores once per pixel. Our earlier description of
one framebuffer write per dab was incorrect for this path. Pencil's flow
brush can instead use direct instanced rasterization with hardware blending;
its traffic differs from G-Pen's uniform-coverage material path.

The proposed improvement is to fit fewer, longer spans, use a simpler nib
distance evaluator, and evaluate paper/material after geometry selection.
This can reduce repeated ALU and texture work. Its cost is still proportional
to shaded pixels times local segment candidates. A nearest-segment loop does
not automatically make the computation constant-time, and page splitting
alone does not bound lists at self-intersections or repeated passes.

## Wacom baseline

All runs use Wacom `5ll21u1002931` (DTHA140), the 61 MP test JPEG, a 15-second
injected stylus stroke, one path revolution per second, 16 ms prediction, one run, and the
isolated `art.capycanvas.penverify` package. CPU and GPU figures are renderer
rolling samples in milliseconds. “Dabs” is cumulative submitted contacts, so
it is useful for workload shape but includes setup work.

| Brush | Diameter | CPU p50 / p95 / p99 | GPU p50 / p95 / p99 | Dabs | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| G-Pen | 32 px | 5.96 / 9.60 / 11.65 | 11.09 / 14.18 / 15.30 | 141,645 | completed |
| G-Pen | 2048 px | 18.88 / 25.73 / 27.00 | 48.61 / 57.64 / 58.63 | 1,025 | completed |
| Pencil | 32 px | 6.77 / 12.88 / 16.07 | 1.95 / 3.49 / 3.81 | 216,160 | completed |
| Pencil | 2048 px | 236.20 / 465.11 / 652.80 | 197.16 / 329.34 / 367.17 | 977 | completed |

The 2048 px Pencil run emitted only 54 CPU and 51 GPU samples because each
update became very slow. This is a strong first candidate for a swept path.
G-Pen is also relevant: its 2048 px, two-revolutions-per-second version loses
the Adreno device after progressively longer callbacks, despite completing at
one revolution per second. This supports a GPU/driver wait diagnosis, but
neither establishes the driver's fault reason nor isolates an individual
shader as the cause. The frequency is path motion, not input delivery rate.

To repeat a row, build the isolated Android app/test APK and run:

```sh
adb -s 5ll21u1002931 shell am instrument -w \
  -e class art.capycanvas.AndroidRasterTest#largePhotoWideBrushAttribution \
  -e wideBrush true -e wideBrushPreset 1 -e wideBrushSize 2048 \
  -e wideBrushTurns 1 -e wideBrushPrediction true \
  -e wideBrushPredictionMs 16 -e wideBrushRuns 1 -e motionDurationMs 15000 \
  art.capycanvas.penverify.test/androidx.test.runner.AndroidJUnitRunner
```

Use `wideBrushPreset 2` for Pencil and change `wideBrushSize` for the small
case. Pull the JSON from
`/sdcard/Android/data/art.capycanvas.penverify/files/wide-brush-attribution.json`.

## First architecture

1. Preserve raw input and existing dabs for current brushes, history, and
   fallback. Add an independent `SweptStroke` record with endpoint position,
   radius, tangent, pressure, color, and cumulative arc length.
2. Bin segment footprints into the existing sparse 256x256 pages. Subdivide
   crowded bins spatially or use an outline renderer where possible. Retain
   explicit overflow handling: splitting at page boundaries cannot bound the
   number of overlapping segments.
3. A tile-local compute shader calculates tapered distance to nearby segments,
   selects a stable closest segment, evaluates coverage and texture once per
   pixel, and writes each target pixel once.
4. Start with normal source-over. Use path-space `(arc length, lateral
   distance)` texture coordinates so ink and pencil grain do not repeat at dab
   centers. The new class may intentionally choose this appearance.
5. Render prediction into the existing disposable preview target with the same
   segment field. Commit real segments independently.

The first prototype should support caps, joins, pressure-varying radius, and
canvas-anchored paper texture. Its acceptance metric is visual quality and
bounded work, not image equivalence with dabs.

## Scope classification

G-Pen and Pencil are current `contact` brushes, not trivial circles. G-Pen
uses uniform accumulation and evolving contact. Pencil uses flow, canvas paper
grain, pressure gain, tilt spread, and tilt shading. The first new class should
not claim to emulate either exactly, but their normal source-over destination
behavior makes them suitable continuous ink and textured-pencil candidates.

Keep the dab path initially for non-normal blending, smudge, liquify, wet
paint, watercolor, reservoir/depletion media, and brushes that need ordered
canvas reads. They can later adopt tile-local simulation or hybrid segment
deposition, but are not prerequisites for the architecture.

## Measurements required before promotion

- segment count and segments/page distribution;
- covered pixels and shader workgroups per committed/preview update;
- CPU encoding time, GPU execution time, and device-loss outcome at 2048 px,
  2 Hz;
- visual review of slow turns, pressure ramps, tilt, joins, and pen-up caps;
- no regression in the retained dab path or stroke persistence.

## Implemented kernel experiment (2026-09-20)

`crates/layer-render-wgpu/examples/swept_brush_bench.rs` is an executable GPU
prototype. It uses the real `DabGenerator` and includes the production
`contact.wgsl` unchanged. It compares three variants:

- **contact**: the production contact evaluator, with normal dry uniform/flow
  accumulation expressed in a compute loop;
- **simple**: the same contacts/page ranges, but round-segment distance,
  nearest-segment material selection and one paper sample per covered pixel;
- **merged**: the simple evaluator after CPU simplification of the resolved
  centerline, with a 0.5-document-pixel center/radius error threshold.

All three use Float32 RGBA source/output, a fixed white source, the same
synthetic 1024x1024 paper texture, 8x8 workgroups and 256x256 page binning.
The baseline keeps a first-to-last contiguous contact range per page, like the
production contact material path. Merging can change page counts and bounds;
both versions use the same conservative bounding-box rule. Counts reported
as `pixel_contact_tests` are static loop/work estimates, not hardware counters.

This is a kernel isolation benchmark, **not a comparison with the complete
production renderer**. In particular, it does not benchmark Pencil's instanced
fragment route, nor photo composition, native storage updates, prediction,
selection or presentation. It removes the production source/coverage binding
machinery. The white backing and repeated static workload may cache/compress
better than artwork. These limitations prohibit multiplying the earlier app
frame numbers directly by these speedup ratios.

The input is a 512 px straight or sine-shaped centerline with constant full
pressure and zero tilt. A separate 4096 px straight fixture tests a larger
batch; "backlog" is the fixture name and does not mean the live app's backlog
was reproduced. At 2048 px the resolved curve contains just three or four
contacts: the simplifier fits those contacts, not the original dense input
curve. Preserving the new brush's intended input shape will require fitting
raw/resolved poses before diameter-based dab spacing.

The prototype deliberately has different appearance: hard round ink with an
antialiasing edge and softer pencil with stationary paper. It does not yet
support tilt, general nib shapes, accumulated stroke coverage across multiple
submissions, or correct repeated-pass graphite buildup. Uniform pressure is
the tested subset; this is not a finished new brush preset.

### Measured results

Physical Wacom `5ll21u1002931`, Vulkan, **Adreno 735**. Both runs rotate variant
order within each repetition and discard five warmups per variant. Run 1 has
30 measured observations/variant; run 2 has 50 and reverses fixture order.
Compute-pass timestamps measure GPU execution. A wait/readback between
observations deliberately prevents queue growth. CPU preparation is a single
setup observation per fixture, and is repeated in CSV for provenance rather
than representing a latency distribution. Timestamp resolution, PNG readback
and PNG encoding are outside the measured GPU pass. Images are captured after
the first measured repetition; absolute timings remain sensitive to clocks.

| Brush / diameter / path | Contacts → segments | Contact GPU median, run 2 | Simple median | Merged median | Speedup across runs |
| --- | ---: | ---: | ---: | ---: | ---: |
| G-Pen / 32 / curve | 103 → 27 | 6.018 ms | 3.092 ms | 0.882 ms | 6.82–7.09× |
| Pencil / 32 / curve | 142 → 27 | 6.213 ms | 3.061 ms | 0.642 ms | 9.67–9.70× |
| G-Pen / 256 / curve | 14 → 13 | 2.773 ms | 1.352 ms | 1.275 ms | 2.17–2.32× |
| Pencil / 256 / curve | 19 → 16 | 2.289 ms | 1.037 ms | 0.902 ms | 2.54–2.57× |
| G-Pen / 2048 / curve | 3 → 2 | 16.975 ms | 7.903 ms | 6.435 ms | 2.64–2.65× |
| Pencil / 2048 / curve | 4 → 3 | 10.969 ms | 4.968 ms | 4.344 ms | 2.46–2.53× |
| G-Pen / 2048 / straight | 3 → 1 | 16.982 ms | 7.902 ms | 5.268 ms | 3.22–3.29× |
| Pencil / 2048 / straight | 3 → 1 | 8.803 ms | 4.331 ms | 3.437 ms | 2.56–2.72× |
| G-Pen / 2048 / 4096 px batch | 13 → 1 | 33.411 ms | 14.937 ms | 8.136 ms | 4.04–4.11× |
| Pencil / 2048 / 4096 px batch | 17 → 1 | 45.948 ms | 18.285 ms | 9.678 ms | 4.71–4.75× |

Small straight paths are favorable special cases: 24.4–25.1× for G-Pen and
31.2–31.4× for Pencil. They should not be advertised as general brush speedups.
Absolute times change substantially with run order, while the paired ratios
are considerably more stable. Reported p95 values in the analyzer can also
be affected by frequency transitions; the sample sizes do not qualify p99.

The simple variant supplies roughly 2× of the improvement without reducing
the contact count. Merging supplies the larger additional gain on small
brushes and long straight batches. On short ultra-large strokes, there are
already few contacts, so replacing a repeated generic contact evaluation with
a simpler material model matters more than geometry reduction.

For planning only, if the changed work is 80% of a frame and improves by
2.5–5×, Amdahl's law predicts 1.92–2.78× overall. At 50% of a frame, it predicts
1.43–1.67×. **Those fractions are hypothetical**; per-pass production timing
and live integration must determine them. This experiment establishes neither
app latency gains nor resolution of the original device loss.

### Research directions and sources

[GPU-friendly Stroke Expansion](https://arxiv.org/html/2405.00127v2), Levien
and Uguray, describes parallel stroke-outline construction with error-bounded
approximations. It also explains why tight curves need careful inner joins
and evolutes. An outline/fill route is worth prototyping for wide ink: it could
move geometric work away from every interior pixel. This is a design inference,
not a result measured in our prototype.

[Polar Stroking](https://arxiv.org/abs/2007.00308), Kilgard, offers an alternative
based on tangent-angle steps and supports arc-length accumulation for path
texturing. Compare its angle-based approximation against a document-space
error bound before using it for a pressure-varying nib.

The next useful experiment is a raw-pose, adaptive curve/outline ink path and
an independently controlled pencil deposition field. A nearest-segment mask
can look good for ink, but pencil must explicitly choose how repeat passes,
pressure, tilt and elapsed exposure add pigment. That choice should be stable
across frame grouping and prediction rollback even though it differs from dabs.

### Reproduction and evidence

```sh
ANDROID_HOME=/path/to/Android/Sdk cargo ndk -t arm64-v8a -P 29 build \
  -p layer-render-wgpu --release --example swept_brush_bench
adb -s 5ll21u1002931 push target/aarch64-linux-android/release/examples/swept_brush_bench \
  /data/local/tmp/capy-swept-brush-bench
adb -s 5ll21u1002931 shell chmod 755 /data/local/tmp/capy-swept-brush-bench
adb -s 5ll21u1002931 shell /data/local/tmp/capy-swept-brush-bench \
  /data/local/tmp/capy-swept-run1 30 all
adb -s 5ll21u1002931 shell /data/local/tmp/capy-swept-brush-bench \
  /data/local/tmp/capy-swept-run2 50 all reverse
adb -s 5ll21u1002931 pull /data/local/tmp/capy-swept-run1/. artifacts/swept-brush/run1/
adb -s 5ll21u1002931 pull /data/local/tmp/capy-swept-run2/. artifacts/swept-brush/run2/
node crates/layer-render-wgpu/examples/swept_brush/analyze.mjs \
  docs/development/measurements/swept-brush/wacom-run2.csv
```

Raw timestamps and counters are committed under `measurements/swept-brush/`.
Representative 32 px curved kernel captures are also committed:
[G-Pen contact](measurements/swept-brush/GPen-32-curve-contact.png),
[G-Pen merged](measurements/swept-brush/GPen-32-curve-merged.png),
[Pencil contact](measurements/swept-brush/Pencil-32-curve-contact.png), and
[Pencil merged](measurements/swept-brush/Pencil-32-curve-merged.png).
Full captures, thermal reports and the previous app baseline JSON files are
in the local `artifacts/swept-brush/` directory. Every captured kernel output
was checked for finite red-channel values and nonempty paint. Representative
G-Pen and Pencil images were visually inspected. Physical Apple, Web and GTK
performance remains untested.
