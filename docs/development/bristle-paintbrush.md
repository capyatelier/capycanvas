# Bristle Paintbrush

Preset 35 appears immediately after the existing Paintbrush (4), which is
unchanged. It models a flat brush: one fan of hairs swept across the paper,
with the paint each hair carries looked up per pixel.

## Contact

Each input pose becomes one fan contact (`layer-engine/src/brush.rs`,
`fan_pose` and `emit_fan`). The fan lies perpendicular to the stylus axis and is
projected onto the paper:

- Pressure spreads the fan (`splay`, at half strength) and engages more hairs.
  Outer hairs are shorter, so light contact touches fewer of them, but even a
  light touch reaches about half the width. Light pressure mostly presses out
  less paint instead (see **Paint**), so a stroke fades in through a few dry
  streaks rather than a thin point.
- Pressure and leaning deepen the contact, so a stroke pulled sideways along
  the fan's width is a narrow band. Rolling a leaning fan foreshortens it and
  lifts one edge; the dab carries that edge height in place of tilt.
- A pen that measures barrel rotation sets the fan's orientation directly,
  including the hover outline before pen-down. Without one, the fan faces the
  lean direction and keeps its last facing while the pen is nearly upright.
  Moving sideways never rotates the fan.
- A roll is split into arcs short enough that the outer hairs advance no
  further than the contact depth. Path simplification bounds the turn between
  spans to about 3°, because the outer hairs draw the stroke's edges.
- A stroke is a tap until the fan has dragged further than its deepest
  contact, by the lesser of its displacement from touchdown and its coherent
  travel (smoothed over the fan's half width, so sensor jitter does not
  count), plus 4 px. Until then its spans wait in the generator and only the
  preview shows them. A drag releases them streaked along its travel
  direction, so streaks run from the trailing edge back to where the brush
  touched down and a sideways start streaks sideways. A pen-up releases them
  as a pressed imprint, with texture on the hair axis. Each span carries
  whether it streaks.
- `finish` ends every fan stroke with a lift: the last pose, left where it
  is. The live preview ends with the same lift, so what is under the brush
  shows while drawing exactly as it would if the pen lifted there.
- Each span also carries the travel direction relative to the fan. The axis of
  travel is smoothed over about half the fan's width, ignoring its sense, so a
  quick reversal keeps its streaks on the same line instead of swinging them
  through a half turn.

Measured barrel rotation reaches the engine as `SampleFlags::BARREL_TWIST` on
pen-down. Android sends `AXIS_RZ` once the stylus has reported a nonzero value
since entering proximity, so pens without a rotation sensor keep the
lean-facing behavior. Android-predicted samples reuse the last real angle.
On the Wacom DTHA140 the Art Pen (tool 0x804) reports `ABS_Z` from −900 to 899,
which the system exposes as `AXIS_RZ` in radians for the combined
touchscreen/stylus source; other pens report zero.

## Deposit

`layer-render-wgpu/src/bristle.wgsl` evaluates each contact per pixel:

- **Lens.** The hairs reaching the paper form an ellipse in fan coordinates.
  Outer hairs are shorter, so light pressure engages only the central tips and
  leaves a small imprint; pressure lengthens the reach toward the full width.
  Rolling shifts the ellipse toward the lowered edge, where the fan's edge
  clips it.
- **Ragged edge.** Hairs fall short of the smooth ellipse by their tip lengths
  in the hair table (groups of hairs plus a smooth variation), offset per
  stroke and drifting slowly across the fan as the brush travels. The edge
  moves inward by up to 22% of the half width at the fan's ends and 55% of
  the depth at its tips, so even the thin first contact of a stroke has an
  uneven outline and no hair line goes missing. A light touch is fully
  ragged; firm pressure evens the hairs to 65%. After the first half diameter
  of travel the tips even out: they then only decide when the trailing edge
  releases a pixel, and there they opened gaps at span joins. The table is looked up along
  each pixel's track at both ends of a span and blended between them, so
  consecutive spans agree exactly at their join. Pixels further inside than
  the largest shortfall skip the lookup.
- **Trailing edge.** Paint on the paper is what the last hairs to leave it
  laid, so a span commits only the pixels its lens leaves during the span,
  and replaces what an earlier pass left there. Pixels still under the lens at
  its end belong to later spans. Each span checks the exact lens at its own
  start, so pixels at a join are never lost. The pen-down imprint is left by
  the trailing edge like any other paint: dragging streaks it from the back of
  the first contact, and a tap keeps it.
- **Held coverage.** While a pixel is under the hairs, spans only record its
  coverage in the stroke's coverage state. When the hairs leave, it takes the
  most it was covered, so a contact that shrinks as the pen lifts leaves full
  paint wherever it once covered, not an antialiased ring at each size it
  paused at.
- **Lift.** The lift paints everything still under the final lens. As pressure
  falls, the lens shrinks and releases pixels through its receding edge; what
  remains when the pen leaves is committed at once, with streaks continuing
  through it.
- **Streaks.** A pixel's hair track is its offset across the travel direction
  where its path leaves the lens, with the heading, paint load and distance at
  that moment. For steady motion that offset is the same wherever along the
  streak the pixel leaves, so streaks stay continuous across spans, curves and
  reversals, and the pen-down imprint streaks straight into the stroke.
  Imprint texture is fixed to the paper, so pixels a shrinking contact
  releases at different moments still match.
- **Hairs.** The hair track indexes a 2048-texel periodic hair table
  (`bristle_table.rs`): visible streaks, broader hair groups, fine hairs and
  grooves, prefiltered over widths of 1–512 texels to antialias small brushes.
  Each stroke starts at a random table offset. The table and the variation
  field bind in the brush texture set's primary and transport slots, which a
  contact brush otherwise leaves unused.
- **Variation.** A periodic field varies broad bands, individual strands and
  dry-brush dashes along the stroke distance, so a straight pull is not a set
  of uniform stripes.
- **Paint.** Each hair carries a reservoir that empties with distance; then the
  film breaks into dashes along its track and catches paper tooth. A little
  residue continues as dry-brush. Below about half pressure, a fraction of the
  hairs skips along its track for a stretch, fewer than a brush running dry,
  so light strokes and stroke entries show a few long gaps. A tap takes the
  firmest pressure it reached, carried in the spans' deposit strength, and
  misses hair tips the same way, so a light tap has about as many gaps as a
  light drag and a firm one is solid. Thin paint shows the
  painting color; thick ridges blend toward a second paint on the hairs. The
  painting tools load the color swatch that is not painting as that second
  paint (`BrushBristles::streak_rgba_linear`).

All material coordinates are fan-relative or stroke-distance-relative, so
texture scales with the configured size. **Bristle scale** changes hair spacing
and **Paint load** the reservoir. Paper uses the canvas grain at a fixed
document scale; its strength is the tool's texture setting.

The stroke keeps the existing per-stroke coverage state. Within one pass of the
hairs over a pixel, the first opaque paint keeps its color. When a later part
of the stroke reaches the pixel again from outside its hairs, it starts a new
pass and paints on top, so a stroke crossing itself shows the later paint. The
decision depends only on the span and the stored coverage, so replay, frame
grouping and prediction produce the same committed result. The hover outline is
the fan's full contact. Tile planning (`brush_tiles.rs`, `FanRegion`)
clips each tile to the convex hull of a span's start and end contact
rectangles, widened for its roll, so it evaluates only pixels the span can
paint or hold.

## Performance

Wacom DTHA140 / Adreno 735, landscape 2880 × 1800, 9504 × 6336 photo at fit
zoom, 200 Hz injected input, 16 ms prediction, optimized arm64 benchmark
build. Fresh one-second strokes, undone between runs; completed nonempty
canvas updates per input second (not display-latched FPS):

| Stroke | Completed updates/s |
| --- | --- |
| Size 1000, upright, load 80% | 109, 112, 108 |
| Size 1000, strong lean, load 80% | 99, 99, 97 |
| Size 1000, varying pressure | 197, 129, 189 |
| Size 460 (default), strong lean | 264, 277, 266 |

On the Wacom DTHA116 (MovinkPad 11, Mali-G57, landscape 2200 × 1440) with the
same photo and input:

| Stroke | Completed updates/s |
| --- | --- |
| Size 1000, upright, fresh | 21, 22, 21 |
| Size 1000, strong lean, fresh | 9, 10, 9 |
| Size 460, strong lean, fresh | 34, 36, 33 |
| Size 460, upright, 10-second loop | 22, 22, 22 |
| Size 460, strong lean, 10-second loop | 25, 26, 25 |
| Size 1000, upright, 10-second loop | 2.3, 2.7, 2.8 |

The original Paintbrush measured 52 at size 460 and 11–12 at size 1000 in
10-second loops. At size 1000 the bristle stroke falls behind its input:
every span evaluates the whole lens to hold coverage, so a late frame carries
more spans and takes longer again. Frame gaps grow past 2 seconds, and the
one-second rows already lie on that curve.

At a given pressure below full, the fan is wider than when pressure also set
most of its size, so each update paints more. The same build with the earlier
pressure-to-size curves measured 121–131, 113–118 and 214–239 in the first
three rows. The ragged edge costs about 5% more at size 1000: pixels near the
contact's edge look up the hair table twice per span. Generating the edge
from value noise and a hash instead, up to three times per pixel, cost 30%.

Painted pixels are not culled before the contact loop, because a later pass
may repaint them. Instead each span rejects most pixels early: those still
inside its lens at its end, which it only holds, and those whose path through
the span stays more than half a pixel outside the contact band. Only pixels
the hairs leave sample the material. The preview's lift repaints the lens
every frame, which costs about a tenth at size 1000.

The shader's cost is dominated by register pressure on Adreno, and it sits at
a cliff: a per-pixel search of the path since touchdown halved these rates
even when unused, and adding any one of a second dryness lookup, a branch for
the lift's leading edge or a contact fitted to the ragged edge cost about 20%.
Per-stroke work belongs in the engine as ordinary spans; a ridge gloss and a
slow wander of hair groups, neither visible in review, were removed to make
room for tap dryness. The previous
three-group model measured 79–122 at size 1000.

## Reproduce

```sh
cargo test --release -p layer-core -p layer-engine -p layer-ui --lib
cargo test --release -p layer-render-wgpu --lib -- brush_tiles bristle_table
cargo test --release -p layer-render-wgpu --test bristle --test project -- --test-threads=1
cargo build --release -p layer-bench --bin gpu-bench
CAPY_BRUSH_PREVIEW_IDS=35 target/release/gpu-bench --brush-previews
```

`tests/bristle.rs` paints through the engine and GPU: frame grouping and
prediction leave the committed paint unchanged, texture scales with size, the
band stays solid and seamless at every angle, lean and roll, a later pass
paints over an earlier one, taps and pen-down streak as described above.

For device measurements, build and install the isolated
`art.capycanvas.brushbench` package and run:

```sh
python3 tools/performance/android-brush-benchmark.py artifacts/bristle-wacom \
  --adb /path/to/adb --serial 5ll21u1002931 --presets 35 --size 1000 \
  --duration 1000 --repeats 3 --prediction true
```

Add `--mode tilt`, `--mode pressure` or `--mode visual`, and `--paint-load 1`
for a fully loaded brush. Use a new prefix per build; completed labels are
skipped.
