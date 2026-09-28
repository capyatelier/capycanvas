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
| Simple brushes guaranteed to | 1024 px (goal 2048 px) | 2048 px | 2048 px |
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
- **Hardware at or above a tier must meet that tier's targets.** To place a
  device, compare its memory bandwidth and GPU throughput with the
  [tier hardware](performance/hardware.md).

## How to test

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

Measured on 2026-09-27 at `be5a7c38`:

- **Brushes.** One of the 114 brush-and-tier combinations meets its target: the
  Calligraphy Pen on the top tier.
  - The G-Pen at its guaranteed size completes 88 updates/s on the top tier
    (target 120), 17 on the mid tier (target 90) and 37 on the low tier (target
    60).
  - On the top tier, complex brushes are close at 76–118 updates/s.
  - On the mid and low tiers, complex brushes reach only 3–25.
- **Wet, smudge and blending brushes run out of memory on every tier at 512 px.**
  They are killed, abort in the allocator, or crash in the GPU driver.
- **Transforms and placement.**
  - At 24 MP the renderer has headroom on the mid tier (188–217 submissions/s)
    and the low tier (130–141).
  - One exception: pixel-transform handle drags on the TCL manage only 24/s.
  - The top tier has no on-device transform measurements.
- **Navigation.**
  - The top tier meets pinch zoom on 61 MP (119 fps).
  - The low and mid tiers are unmeasured on their canvases.
  - The MovinkPad 11 presents at 60 Hz, not 90 Hz.
- **UI.** Moving the canvas bar and changing panel content take 21–63 ms per UI
  frame at p50, which misses the target on every tier.

[Known gaps](performance/known-gaps.md) lists the details and open measurements.

## Details

| Reference | Contents |
| --- | --- |
| [Measuring performance](performance/measuring.md) | Frame definitions, pass criteria, the soft-target bandwidth rule and benchmark commands |
| [Tier hardware](performance/hardware.md) | SoC, CPU, GPU, registers, tile memory, bandwidth, RAM, display and public benchmark scores, with comparable hardware |
| [Brush classes](performance/brush-classes.md) | Class of each of the 38 presets and the reason for it |
| [Low tier](performance/low-tier.md), [Mid tier](performance/mid-tier.md), [Top tier](performance/top-tier.md) | Every operation and brush with its target and newest measurement |
| [Responsiveness](performance/responsiveness.md) | Latency limits for taps, pen-down, undo, selections, launch and open |
| [Known gaps](performance/known-gaps.md) | Failures, unmeasured areas and missing harnesses |
