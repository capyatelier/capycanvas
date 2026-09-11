# Brushes

[Technical documentation](../README.md) · [Architecture](../architecture.md)

A brush has two jobs: decide where and how to place marks along a stroke, then
evaluate those marks against the canvas. Capy Canvas keeps placement in the shared
CPU engine and pixel work in the GPU renderer. Every platform uses both parts.

## Brush definitions and strokes

[`BrushSnapshot`](../../crates/layer-core/src/lib.rs) describes the brush settings
captured for a stroke. It includes the tip, spacing, sensor mappings, color and
rendering behavior. Built-in definitions live in
[`presets.rs`](../../crates/layer-core/src/presets.rs); shared tool controls expose
the editable settings.

*Dynamics* map a changing input to a brush property. Pressure might change size or
opacity; tilt can change the shape or rotation of a mark. Input availability varies
by device, but the mapping rules are shared. A stroke stores the settings it used,
so editing a brush later does not change old strokes during replay.

## From samples to dabs

[`DabGenerator`](../../crates/layer-engine/src/brush.rs) processes the ordered path
and places contacts by distance traveled. A *dab* is one resolved brush contact:
its location, shape, color and other parameters are ready for the renderer.

Distance-based placement avoids making the brush density depend directly on how
many events a platform sends. Spacing, stabilization, taper and deterministic
variation shape the resulting sequence. The random seed is tied to the brush and
stroke so replay can reproduce its variation.

Dabs and shared style data are collected into batches. The renderer receives those
batches through the [render contract](../../crates/layer-render/src/lib.rs), rather
than receiving a separate platform callback for each mark.

## GPU execution

Ordinary ink and erase contacts use coverage and GPU blending. Textured brushes
also sample tip and grain resources. These paths can avoid the extra state needed
by brushes that interact with existing paint.

A destination-aware brush reads what is already on the layer. Smudge, wet mixing,
watercolor and deformation therefore require ordered operations and additional
GPU resources. Depending on the brush, those resources track coverage, wetness or
paint carried by the brush. Reading and writing the same image requires controlled
staging; it cannot be treated as ordinary independent source-over blending.

Watercolor uses its own pigment and wetness behavior. It is not a general physical
fluid simulation, and not every brush uses the same state or passes. The
[painterly paint-state reference](../reference/painterly-paint-state.md) explains
those distinctions in detail.

## Feedback and replay

The engine can draw temporary predicted contacts to reduce the apparent gap
between the pen and the stroke. Prediction uses replaceable preview state, leaving
committed paint, material state and undo history unchanged. Real input replaces
that preview. Predictions are not saved into the document.

Replay uses the stored real samples and brush snapshot. Any brush change should
therefore be tested both while drawing and after undo/redo or reopening a project.

## Performance and mobile devices

The main reason to put pixel work on the GPU is the cost of brushes that interact
with existing paint. Simple stamps can be efficient on a CPU. Smearing, blending,
liquify and painterly models require more sampling and state updates per contact,
and larger tips multiply that work. More detailed fluid and paint simulations
would increase the cost further; the current watercolor model uses bounded,
localized transport.

The engine keeps sequential input and placement on the CPU, batches the resolved
contacts and leaves layer pixels and paint state in GPU resources. The renderer
runs pixel work in parallel while retaining the ordering that each brush requires.
Sparse pages and changed-region bounds limit the affected area. Specialized paths
avoid allocating or updating material state that a brush does not use.

These choices target both performance and energy use. On mobile hardware, memory
traffic and CPU/driver overhead contribute to power consumption alongside GPU
computation. Reducing those costs can improve efficiency even when frame rate is
unchanged, as explained in [Arm's GPU energy-efficiency guide](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/energy-efficiency-in-gpu-applications-part-1).

This is the rationale for the design, rather than a measured advantage over other
painting apps. Compare equivalent brushes and image quality on the same hardware,
and measure sustained frame time and energy per stroke. The existing
[GPU workloads](../development/gpu-raster-benchmarks.md) measure our renderer's
cost; they do not establish a CPU comparison or battery-life improvement.

## Changing a brush

Start with the preset and shared settings when changing a brush's behavior. Change
CPU dynamics for placement or sensor rules, and the renderer for pixel interactions.
Avoid implementing a separate brush rule in a platform adapter.

The [raster reference](../brush-renderer.md) documents dab fields and
correctness rules. [GPU brush stages](../reference/gpu-brush-engine.md) explains
destination interactions. The [developer testing guide](../development/testing.md)
links to replay tests, benchmark workloads and preview generation.
