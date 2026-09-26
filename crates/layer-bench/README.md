# layer-bench

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-bench` provides the `gpu-bench` executable for repeatable drawing workloads
and runtime brush previews. It drives `CanvasEngine` with a headless
`WgpuRasterizer`, including input submission, frame drawing, GPU completion and
explicit image export.

## Workloads and outputs

The harness includes ink, textured and painterly brush scenarios, destination
interactions such as smudge, and layered composition. Repeated runs use fresh
canvases. Reports distinguish CPU submission from completed GPU work and include
resource measurements.

Each run also writes a per-scenario PNG gallery for visual comparison. Preview
generation produces the light/dark brush samples bundled by the clients. These are
different outputs: benchmark galleries belong in ignored artifact directories,
while the application intentionally tracks its runtime previews.

Offscreen measurements do not include native input delivery, widget behavior or
display presentation. Those require host and physical-device checks.

## Where to start

- [main.rs](src/main.rs) defines scenarios, command-line options, repeated runs
  and reports.
- [previews.rs](src/previews.rs) generates the application's brush-preview assets.
- The [GPU benchmark guide](../../docs/development/gpu-raster-benchmarks.md) lists
  workloads and run commands.
- [Testing and performance](../../docs/development/testing.md#performance) explains
  how these measurements relate to the engine benchmark and native input latency.

The harness requires a hardware GPU. The platform build prerequisites are listed
in the [developer guide](../../docs/development/README.md).
