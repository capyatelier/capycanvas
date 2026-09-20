# Contact brush engine

Implemented and reviewed on GTK/Wayland/Vulkan, 2026-09-13. This is the follow-up
to the [research proposal](../history/dry-media-brush-design.md).

## Model

All twelve presets use one [contact evaluator](../../crates/layer-render-wgpu/src/contact.wgsl)
and the existing sparse GPU rasterizer. The CPU resolves pressure curves,
modeled input, tilt and direction into pairs of contact poses. The GPU
interpolates the footprint between those poses, evaluates material contact,
and deposits pigment. There is no per-frame bitmap generation or texture upload.
The 128-byte contact record is an internal Rust/GPU contract, not the foreign ABI.

Swept brushes no longer insert distance-spaced stamps between input samples.
An online simplifier retains a modeled vertex when the path bends or pressure,
tilt, or twist changes enough. The traveled-length/chord ellipse bounds geometric
error without buffering a point list: tolerance is 0.25 document pixels or 1% of
the previous minor radius, whichever is larger. Pressure uses the nominal-radius
change against that tolerance; tilt and twist have separate pose thresholds.
Straight steady runs advance at half the nominal brush diameter (at least 4 px),
so large brushes keep coarse sampling. Pen-up flushes the pending endpoint.
Decisions depend on input samples, not render frames, and prediction clones the
same generator. Ordinary stamp brushes retain their distance-spacing loop.

This changes contact density and procedural variation; it does not promise pixel
identity with earlier contact rendering. See the
[Huion optimization measurements](gpen-huion-sparse-strokes-2026-09-20.md).

The nib sweep projects in the ellipse's metric. Projecting only by canvas
distance produced scalloped edges on angled calligraphy nibs. A shared bound
includes both poses and bounded edge expansion, keeping page allocation and
scissoring consistent with the shader.

Paper uses an original, cached 1024² R8 height field. Its coordinates exclude
contact center, random seed, size and stylus orientation. Pressure determines
which heights receive graphite; tilt broadens the footprint and concentrates
pressure toward the point. Repeated passes deepen the same contacted tooth.
Flow is normalized for travel distance and the additional area of the sweep,
so changing contact spacing does not simply add more graphite. The paper recipe
is repository code under MIT OR Apache-2.0; no third-party image assets were added.

Ink keeps the maximum requested coverage for the current stroke and deposits
only the increment. Consecutive contacts therefore extend the same deposit.
R8 coverage is quantized before applying its color delta, avoiding differences
caused by grouping contacts into different frames. Roughness comes from bounded,
stationary noise in the contact boundary. Pooling uses the transverse stroke
profile; darkening every circular front cap left a repeated ring pattern and
was removed. Brushed ink uses coherent strand identities with pressure-dependent
separation and declining supply.

This is a procedural contact/deposition model, without canvas diffusion,
pigment mixing or a mechanical bristle solver. A later GPU compute stage can
write resolved contact poses or a footprint field before deposition. Its state
must follow recorded input steps and participate in prediction rollback.
The current shape evaluator need not be replaced to add that producer.

## Presets and controls

Definitions are in [contact_presets.rs](../../crates/layer-core/src/contact_presets.rs).
The shared catalog registers the same stable IDs through the UI and FFI.
GTK and Web ship PNG previews produced by the actual engine.

| Preset | Group | Reviewed behavior |
| --- | --- | --- |
| Pencil | Pencil | Fine stationary grain, pressure buildup, shaded side when tilted |
| Pointy Pencil | Pencil | Narrow contact with a stronger dark edge and reduced opposite side |
| Shading Pencil | Pencil | Broad, soft tilted shading that retains paper tooth |
| Charcoal | Pastel | Coarser tooth, broad contact, dark dense buildup |
| G-Pen | Pen | Solid continuous line, fine pressure-controlled lift |
| Rough G-Pen | Pen | Continuous solid core with fine irregular edges |
| Calligraphy Pen | Pen | Angled flat nib; stroke width changes with drawing direction |
| Antique Pen | Pen | Uneven nib edge and mildly declining ink supply |
| Realistic Pen | Pen | Fine nib with restrained edge texture and pooling |
| Wet Ink | Pen | Broad dark deposit with a subtle continuous edge |
| Blotty Ink | Pen | Larger irregular lobes joined into a continuous deposit |
| Realistic Brushed Ink | Pen | Loaded center, coherent strand gaps, pressure-dependent separation |

Existing size, opacity, color and stabilization controls remain the UI surface.
Pressure curves, minimum diameter, taper distances, endpoint size/opacity and
the new `taper.tip_sharpness` remain shared brush parameters. Pen presets retain
1% diameter at zero pressure and use a slightly sharpened pressure curve. The
generator explicitly closes the final sub-spacing segment with the final pose
instead of copying the preceding large contact. Configured contact taper lengths
use nominal diameter. Assisted end taper still requires a final-length replay;
the default presets rely on live pressure and do not enable that replay.

Stylus tilt and twist directions now rotate/reflect with the document view;
zoom does not change tilt magnitude. Calligraphy uses a held nib angle, whose
projected width naturally varies with the stroke direction. Brushed ink follows
stroke direction. A device providing twist can use the existing twist mapping.

Contact snapshots use schema 5. All contact strokes use the current swept
generator, including replay; there is no version-selected contact algorithm.
Saved raster revisions preserve existing artwork. Ordinary stamp brushes and
the visibility-mask painter remain separate current tools.

## Research used

Procreate's documented stationary grain, tilt gradation and taper character
guided the expected artist controls. This is our implementation, not a claim
about Procreate's proprietary algorithms.
[Brush Studio settings](https://help.procreate.com/procreate/handbook/brushes/brush-studio-settings).

The separation of contact shape from deposition is consistent with
[A Brush Stroke Synthesis Toolbox](https://research.adobe.com/publication/a-brush-stroke-synthesis-toolbox/).
[Detail-Preserving Paint Modeling for 3D Brushes](https://www.microsoft.com/en-us/research/wp-content/uploads/2010/06/PaintModel_NPAR_2010.pdf)
also motivates keeping paint detail independent of repeatedly resampled moving
footprints. Its pickup/mixing algorithm is outside this change.

[Real-time Dynamic and Pressure-sensitive Brush Rendering](https://shizhezhou.github.io/projects/dynamicBrush/dynamic_brush.pdf)
models a growing stroke with iterative GPU diffusion. We adopt the continuity
goal, using a swept footprint and persistent maximum coverage instead of that
iterative solver. Darkly was examined as an inexpensive procedural reference;
its source was not copied and it is not the quality target.

## Validation and performance

Every preset has a 1000×760 swatch covering pressure ramps, light/medium/heavy
lines, opposite tilt, broad curves and repeated hatching. The review found and
corrected coarse repeating paper, angled-nib scallops, circular pooling marks,
excessive full-width strand separation and spacing-dependent graphite buildup.

The GTK test activates actual group/preset buttons, routes pressure/tilt pen
records through the host input path, verifies twelve committed strokes, captures
the canvas and exercises undo/redo. This tests GTK scheduling and presentation;
physical tablet hardware delivery remains a separate manual check.

GPU tests verify stationary paper across seeds, pressure thresholds, tilt-side
reversal, repeat deposition, spacing, frame cadence, prediction and continuous
ink. Every preset also passes archive save/reopen and exact raster undo/redo.
Renderer regressions cover sparse pages, selections, alpha lock, startup,
material specialization, existing wet tools and renderer replacement.

Measured with a release build on NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan,
driver 610.57.04. Two 240 Hz samples are processed per 120 Hz frame. GPU timestamps
cover raster and compositing commands. CPU timings cover `render_frame_at`;
GPU waits, image readback, startup and capture-backpressure draining are excluded.
These are work times on this workstation, not pen-to-display latency or mobile
performance guarantees.

| Workload, worst p95 across the twelve presets | CPU | GPU |
| --- | ---: | ---: |
| Default diameter, ordinary stroke, prediction disabled | 0.078 ms | 0.027 ms |
| 128 px, fast stroke, prediction enabled | 0.200 ms | 0.100 ms |
| 512 px, fast stroke, prediction enabled | 0.316 ms | 0.239 ms |

The largest GPU sample in these runs was 0.249 ms. The broad tests use the same
1000×760 surface and include self-crossings and page changes. Compilation and
one-time paper generation happen during brush preparation, before pen-down.

Reproduce the gallery and timings:

```sh
cargo run --locked --release -p layer-render-wgpu --example contact_gallery -- artifacts/contact-brushes/gallery
cargo test --locked --release -p layer-render-wgpu --test contact --test project -- --test-threads=1
bash tools/performance/workspace-motion.sh gtk --native-test=native_contact_brushes
CAPY_CONTACT_PREVIEWS_ONLY=1 cargo run --locked --release -p layer-bench -- --brush-previews
```

Generated review files are in `artifacts/contact-brushes/gallery/index.html`,
`timings.csv`, `stress-timings.csv` and `artifacts/contact-brushes/gtk/`.
