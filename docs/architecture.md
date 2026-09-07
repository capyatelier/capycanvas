# Portable core architecture

## Fixed boundaries

1. Each platform owns native widgets, high-rate pen event retrieval, its window,
   and surface presentation.
2. Shared Rust owns the document, input interpretation, brush dynamics,
   commands, undo/redo, AI request semantics, and toolkit-neutral UI state.
3. `layer-render-wgpu` owns every canvas pixel and canvas GPU resource; platform
   hosts own surface/image sharing and presentation synchronization. Painting
   requires a hardware GPU; there is no software rasterizer, host-memory canvas,
   or canvas-composition fallback.
4. Input callbacks never wait for rendering, inference, disk, or the document.
5. Render backends receive one borrowed packet per frame, not calls per contact.
6. Predicted input is visual-only. AI output is a revision-tagged image layer.
7. Shared APIs require neither threads nor a specific event loop and remain
   suitable for `wasm32-unknown-unknown`.

## Packages

```text
native / Web frontend
    ├── layer-ui ─────────────────────────► layer-engine ─────► layer-core
    │          └──────────────────────────► layer-render           │
    │                                                               │
    ├── native input adapter ── PenEvent ingress ──────────────────┘
    │
    └── platform presenter ─────► layer-render-wgpu ──► wgpu
                                      ▲       │
                                      └───────┴── FramePacket

headless validation and bindings
    └── layer-ffi ───────────────► layer-engine + layer-render-wgpu
        layer-bench ─────────────► layer-ffi
```

- `layer-core` owns persistent document, edit, geometry, brush preset, and AI
  request semantics.
- `layer-render` owns only resolved contact/batch records and the small
  renderer-facing contract.
- `layer-engine` owns the input queue, sample transforms, pressure and sensor
  evaluation, deterministic contact placement, stroke construction, document
  orchestration, and frame packet construction.
- `layer-render-wgpu` owns brush raster, paint/erase, preview, color/material
  pages, brush reservoir, composition, explicit readback, and the shared GPU
  viewport presenter. Platform hosts supply its target surface or shared image.
- `layer-ffi` exposes validated batched input and canvas/document commands. Its
  explicit pixel copy is for export and tests, never presentation.
- `layer-bench` exercises the public ABI with 15 legacy and 10 painter-focused
  4096×4096 workloads, at least 32 visible layers, repeated fresh canvases, and
  reports submission, completed GPU work, and resident canvas bytes.
- `layer-ui` owns toolkit-free screen state, docking, camera/touch, typed
  actions, validation, and multi-step flows. Native toolkits draw the controls.

`layer-render-wgpu` is one portable renderer implementation over Vulkan, Metal,
D3D12, and WebGPU. Platform presenters supply surface integration but do not
implement brush or canvas-pixel behavior.

## Runtime flow

```text
PLATFORM UI EVENT LOOP
  native widget event → UiAction → UiSession → CanvasEngine

PLATFORM INPUT CALLBACK
  retrieve complete coalesced/predicted history
  normalize axes, timestamp, flags, and view revision
  push fixed PenEvent records into bounded ingress
                          │
                          ▼
ENGINE / RENDER OWNER (CPU)
  drain input
  transform → dynamics → distance sampling → ordered Dabs
  update document and build one borrowed FramePacket
                          │
                          ▼
WGPU RENDERER
  one contact upload + one style upload
  GPU contact coverage + paint/erase → persistent layer textures
  GPU dirty composition → persistent composite texture
  submit without waiting
                          │
                          ▼
PLATFORM PRESENTER
  sample composite into acquired surface and present

BACKGROUND HOST WORK
  image decode · project/image encode · AI inference
```

There is one ordered engine/render owner because input interpretation, brush
dynamics, document edits, and command submission are sequential. GPU passes
provide the pixel parallelism. Native hosts may keep UI and engine on one event
loop or place the engine on a canvas thread; the shared APIs do not require one
topology. Wasm may run everything on the browser event loop.

## CPU and GPU work

| Operation | Owner |
| --- | --- |
| Retrieve platform event history and normalize axes | Native/Web adapter, CPU |
| Pressure mapping, sensors, smoothing, spacing, and contact placement | `layer-engine`, CPU |
| Document edits, stroke history, undo/redo, and UI business logic | Shared Rust, CPU |
| Damage bounds, layer reconciliation, and GPU command encoding | Engine/renderer owner, CPU |
| Analytic or R8 contact coverage | `layer-render-wgpu`, GPU |
| Paint and erase blending | `layer-render-wgpu`, GPU |
| Preview, layer opacity composition, viewport sampling, and presentation | `layer-render-wgpu`, GPU |
| Pickup, smear, wet transfer, blend, and liquify | `layer-render-wgpu`, GPU |
| Explicit export | GPU straight-alpha/sRGB conversion and readback, then host byte-stream encoding |
| Image decode, persistence, and AI inference | Background host/runtime |

CPU code never loops over canvas pixels for coverage, blending, compositing,
color conversion, or transforms. Explicit export maps the GPU-produced sRGB
byte stream for the host image encoder outside the drawing path. Failure to
create a hardware GPU adapter disables painting instead of selecting another
pixel implementation.

## Shared input contract

`PenEvent` is a fixed-size record containing physical-surface position,
normalized pressure, tilt, twist, distance, tool, monotonic time, sequence,
prediction flags, and a view revision.

The adapter retrieves complete history immediately and pushes into a bounded
`rtrb` ingress. The queue allocates once, never blocks, and reports capacity
failure. The view revision maps delayed samples through the camera transform
visible when they were captured.

## Document contract

The current document stores immutable source strokes with shared point arrays.
Undo and redo move inverse edits without copying points. GPU textures are
rebuildable renderer state.

Dry and destination-aware brushes replay deterministically after load or device
recreation. A future persistent water/pigment simulation would add versioned GPU
material checkpoints; it will not add a CPU simulation.

## Rendering contract

`FramePacket` contains:

- finite document extent, view transform, surface extent, and background;
- the authoritative front-to-back layer slice;
- one contiguous slice of newly resolved fixed-size `Dab` contacts;
- compatible batches with layer, style, contact range, and damage;
- an explicit full-rebuild flag;
- an explicit full-composition flag.

It contains no wgpu handle, texture ID, surface object, tile coordinate, fence,
or synchronous timing result. The packet is borrowed for `submit`; a renderer
copies only its compact GPU records.

The implemented renderer stores only touched 256×256 GPU pages per paint layer
and uses damage scissors. Empty layers contain metadata only. Paging is private
and does not change this contract.

## Frame policy

- Wake from display callbacks and new work; never busy-poll.
- Drain all available coalesced samples within a defensive bound.
- Generate and upload only contacts that are new since the previous frame.
- Keep no more presentation work queued than the platform needs.
- Never wait for GPU completion in a live frame.
- Never read back a pixel for drawing, AI preview, or presentation.
- Measure input-to-submit in the engine and input-to-present with platform
  timestamps.

## UI boundary

`layer-ui` represents concepts such as the active tool, toolbar state, color
selection, layer list, settings editor, errors, and modal flows. Swift,
Kotlin/Compose, GTK, WinUI, AppKit/SwiftUI, and Web frontends render those values
with native controls and send typed actions back. Canvas surface handles and
high-rate pen records bypass generic UI bindings and go to their dedicated
platform adapters.

See [shared-ui.md](shared-ui.md) and [platform-adapters.md](platform-adapters.md).

## Current milestone

Implemented:

- shared document, edits, input queue, brush dynamics, and incremental packets;
- one wgpu canvas renderer with analytic, mask, grain, dual, wet, smudge, blend,
  liquify, prediction, sparse layer composition, and explicit export paths;
- C ABI plus repeated 15-scenario legacy and 10-scenario painter GPU benchmarks
  with sparse-state memory metrics;
- 10 labeled 4096×4096 painter outputs and a contact sheet; every painter path's
  move and pen-up p99 is under the 8.33 ms 120 Hz work budget on the benchmark
  workstation;
- controlled destination-interaction outputs for smudge, wet mixing, natural
  blending, and push/twirl liquify, with corrected ordered feedback and
  cross-page bilinear sampling;
- shared `UiSession` actions, docking allocation, settings and camera/input,
  with GTK4/libadwaita and actual Wasm/WebGPU frontends; both present the same
  GPU viewport shader and have interaction checks and light/dark screenshots.

Next:

1. Human-test native tablet/touch delivery and capture input-to-present traces.
2. Add imported and AI image assets.
3. Evaluate persistent pigment/water simulation only after the implemented wet
   interaction is tested with artists.
