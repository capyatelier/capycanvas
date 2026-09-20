# Swept-brush research track

This branch explores a **new** brush class. It is not a compatibility rewrite
of the existing dab renderer: its strokes may intentionally look different.
The existing dab pipeline remains the fallback for saved brushes and for media
whose appearance depends on ordered simulation.

## Question

The present material shader runs one 256x256-page invocation per output pixel,
then iterates that pixel's ordered dab range. Large contacts make a single
pixel evaluate many highly overlapping footprints and repeatedly write the
same destination. A swept brush instead represents input as connected,
tapered centerline segments and evaluates a continuous field once per pixel.

That does not eliminate the cost of filling a wide stroke's area. It removes
the repeated-dab component and makes work approximately proportional to swept
area plus a bounded local segment list, rather than area times overlapping
stamp count.

## Wacom baseline

All runs use Wacom `5ll21u1002931` (DTHA140), the 61 MP test JPEG, a 15-second
injected stylus stroke, 1 Hz delivery, 16 ms prediction, one run, and the
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
G-Pen is also relevant: its 2048 px 2 Hz version loses the Adreno device due
to growing GPU backlog, despite completing at 1 Hz.

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
2. Bin segments into the existing sparse 256x256 pages. Split at page
   boundaries to keep each page list bounded.
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
