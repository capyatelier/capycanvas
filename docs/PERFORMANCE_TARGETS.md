# Performance targets

[Technical documentation](README.md)

Capy Canvas guarantees a minimum sustained frame rate for every brush, tool and
movement on three hardware tiers. This page covers the goals, how to test them
and where we stand. The [detailed references](#details) hold the per-operation
tables and hardware specifications.

## Goals

| | Low | Mid | Top |
| --- | --- | --- | --- |
| Reference device | TCL TAB 11 Gen 2 (9465X) | Wacom MovinkPad 11 (DTHA116) | Wacom MovinkPad Pro 14 (DTHA140) |
| GPU, memory bandwidth | Mali-G52 MC2, 14.4 GB/s | Mali-G57 MC2, 17.1 GB/s | Adreno 735, 67.2 GB/s |
| Target canvas | 12 MP, 4248 × 2832 | 24 MP, 6000 × 4000 | 61 MP, 9504 × 6336 |
| Minimum sustained rate | **60 fps** | **90 fps** | **120 fps** |
| Simple brushes guaranteed to | 1024 px (goal 2048 px) | 1536 px | 2048 px |
| Complex brushes guaranteed to | 1024 px | 1024 px | 1024 px |
| Very complex brushes guaranteed to | 512 px | 512 px | 512 px |

- **Every motion holds the tier rate on the tier canvas.** That includes strokes,
  pan, zoom and rotate, transform and placement drags, selections, slider
  scrubs, panel drags and animations.
- **Only moving frames count.** A still frame, such as the one after a drag is
  released, may be slow unless it delays the next motion.
- **Brushes are guaranteed by class.**
  - Simple: dry deposit from one tip, such as the G-Pen, pencils, Airbrush and Eraser.
  - Complex: textured or bristle work, such as the Paintbrush, Marker and Spray.
  - Very complex: brushes that sample the canvas or keep paint state, such as wet
    paint, watercolour, Smudge and Liquify.
  - Every brush must reach its tier's rate at up to 512 px or more.
- **Full-screen filters are soft targets.**
  - They may run slower only when arithmetic shows the hardware cannot deliver
    them, and no valid approximation exists.
  - Previewing at display resolution while a control moves counts as a valid
    approximation.
- **Painting through a live filter has its own gate.** Cheap filters must keep
  the tier rate using incremental evaluation. More expensive filters may qualify
  for a lower rate of fresh filtered results only under the
  [live-filter gate](performance/measuring.md#live-filter-performance-gates):
  measured hardware costs, no adequate faster approach, bounded result age and
  refinement, and responsive input/navigation. Fast/Medium/Slow are engineering
  expectations, not automatic exemptions or user-selectable quality settings.
  An accepted slower result is recorded as an exception, never as meeting the
  ordinary tier target. Wasteful algorithms do not qualify for an exception.
- **Hardware at or above a tier must meet that tier's targets.** To place a
  device, compare its memory bandwidth and GPU throughput with the
  [tier hardware](performance/hardware.md).

## How to test

### Engineering budgets

These budgets leave time for presentation and scheduling within the tier's frame
period. They guide optimization; passing them does not replace the moving-frame
measurements below.

| Work per moving frame | Low | Mid | Top |
| --- | --- | --- | --- |
| CPU input, update and submission | 4 ms | 3 ms | 2 ms |
| GPU painting, composition and presentation | 12 ms | 8 ms | 6 ms |

Background refinement must yield to fresh input. Aim for at most one frame of
additional delay when drawing resumes, and require new operations to meet their
normal motion and response targets while refinement is pending. Refinement may
take longer to finish if it preserves that responsiveness. On the low tier,
100–250 ms for ordinary stroke refinement and 250–500 ms for broad composition
damage are efficiency aims, not independent pass conditions. Measure both
completion and interrupted refinement; a fast completion time cannot justify a
long input stall.

Model each workload from its changed pixels, texture reads and writes, filter
taps, and submissions. Compare it with the same kernels and formats measured on
the reference device. Investigate costs above 1.5 times that calibrated estimate.
Peak memory bandwidth alone is not an adequate model for conversion, filtering,
small dispatches, or CPU submission overhead.

### Qualification

**When a target is met:**

- Brushes: at least the target in completed canvas updates per second, with a
  p99 gap between updates of at most two frame budgets.
- Display-paced motion: presented frames per second of at least 95% of the
  target, with p99 intervals of at most two frame budgets (33.3, 22.2 and
  16.7 ms).

**How to run:**

- Measure release or benchmark builds, warmed up, with at least three gestures
  of 5–10 s each, on the reference device.
- Brushes: `tools/performance/android-brush-benchmark.py`, with `--photo` set to
  the tier canvas and `--size` set to the guaranteed size.
  `android-brush-report.py` then summarizes the results.
- Navigation: `AndroidViewportBenchmarkTest`.
- Transforms, placement, selections and the canvas bar:
  `AndroidCanvasBarBenchmarkTest`, with `-e width`/`-e height` set to the tier
  canvas.
- Desktop, Web and Apple use their own harnesses.

[Measuring performance](performance/measuring.md) has the full rules and commands.
Record each result in the tier's table with its value, date and commit.

## Where we stand

The integrated compositor has moving-stroke measurements on all three
reference tablets (2026-09-29–2026-10-01). Balanced-root, four-page builds are
measured on low and mid tiers:

- TCL G-Pen 1024 px on 12 MP reaches 63.7 fresh updates/s with completion-gap
  p99 below 28.4 ms; this measured stroke meets 60/s. Mid-tier G-Pen at the
  revised 1536 px guarantee reaches 55.6/s on a path that keeps the whole brush
  inside the photo, with completion-gap p99 31.9–33.5 ms; the 90/s and
  22.2 ms criteria remain open. The current
  top-tier G-Pen comparison reaches 87.6/s with completion-gap p99
  31.6–38.8 ms; both targets remain open.
  Other mid-tier simple brushes need measurements at 1536 px; their older
  2048 px results are above the guarantee. Stacked-photo strokes remain below
  their targets.
- Clone, Healing and Spot Healing at 512 px reach 114–120 fresh updates/s on
  the mid tier and 185–192 on the top tier. Their completion gaps meet the
  moving-stroke criterion. Bounded Healing finalization keeps navigation input
  queue p95 at 14–21 ms on the low and mid tablets; dependent painting waits
  for publication. See [responsiveness](performance/responsiveness.md) for
  maximum delays, tool actions and the remaining ordinary-stroke latency misses.
- With Navigator open, retained previews raise low-tier Free resize from 73.28
  to 150.69 fresh completed updates/s and Distort from 64.52 to 133.97/s.
  Both retain approximately 59.4 screen presents/s on the 60 Hz panel.
  Two-photo composed transforms and eight-layer strokes remain below 60 Hz. The
  [matched comparison](performance/low-tier.md#current-drag-comparison) records
  completion gaps, presentation-accounting limits and the composed-case outlier.
- The XP-Pen 6 GB memory journey completes 24 MP Frequency Separation around
  2.0 GiB peak PSS, with more than 1 GiB available. Its roughly 10-second
  completion time remains above the 2–5-second engineering aim.

The tier tables identify the exact frozen builds and measurement conditions.
Exact GPU bounds queries on the top-tier 61 MP canvas take 95–135 ms for a
transform target and 120–215 ms for visible content. They run asynchronously and
cache results. These [query measurements](performance/top-tier.md#exact-content-bounds)
do not qualify command response or moving-frame rates, and do not establish a
hardware-limit waiver.
Other brush, navigation, transform and UI rows retain their previous measurements
or remain unmeasured; improvements above do not qualify them. Exact refinement,
startup and host presentation still need their complete qualification matrices.

[Known gaps](performance/known-gaps.md) lists the details and open measurements.

## Details

| Reference | Contents |
| --- | --- |
| [Measuring performance](performance/measuring.md) | Frame definitions, pass criteria, the soft-target bandwidth rule and benchmark commands |
| [Tier hardware](performance/hardware.md) | SoC, CPU, GPU, registers, tile memory, bandwidth, RAM, display and public benchmark scores, with comparable hardware |
| [Brush classes](performance/brush-classes.md) | Class of each of the 39 presets and the reason for it |
| [Low tier](performance/low-tier.md), [Mid tier](performance/mid-tier.md), [Top tier](performance/top-tier.md) | Every operation and brush with its target and newest measurement |
| [Responsiveness](performance/responsiveness.md) | Latency limits for taps, pen-down, undo, selections, launch and open |
| [Known gaps](performance/known-gaps.md) | Failures, unmeasured areas and missing harnesses |
