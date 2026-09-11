# Architecture

[Technical documentation](README.md)

Capy Canvas separates what a drawing means from how it is displayed. Shared Rust
code owns the document, tools and editor state. A platform client owns native
controls and operating-system services. One GPU renderer implements painting and
composition for every client.

This separation lets a new client reuse drawing behavior without recreating the
brush engine, layer rules or preference validation. It also lets the shared code
be tested without launching a window.

## The shared editor

`Document`, in `layer-core`, describes the artwork: layers, strokes, source images,
masks and other edits. `Editor` applies reversible edits and keeps undo/redo
history. These types do not depend on a graphics API or a UI toolkit.

`CanvasEngine`, in `layer-engine`, connects that model to drawing input. It
interprets ordered pen samples, evaluates brush dynamics and produces work for a
renderer. Brush dynamics are the rules that map inputs such as pressure to
properties such as size or opacity.

`UiSession`, in `layer-ui`, coordinates the engine with application behavior. It
owns the active tools, camera, workspace, command availability, preferences and
file-operation state. A frontend sends typed actions and renders the resulting
views. It keeps native widget objects, focus and accessibility outside the session.

Android, Apple and Windows use `NativeHost`, in `layer-host`, to share session and
renderer integration. GTK and web integrate `UiSession` directly. The host layer
is not another document model and does not own native surfaces or widgets.

## From a pen event to a frame

```text
Platform input callback
    │ ordered pen records
    ▼
UiSession / CanvasEngine
    ├── interpret the current tool and camera
    ├── update the document
    └── resolve brush contacts
    │ FramePacket
    ▼
CanvasRenderer (implemented by WgpuRasterizer)
    ├── update affected paint textures
    ├── compose changed layer regions
    └── prepare the visible canvas
    │
    ▼
Platform surface and presentation
```

1. The platform collects the samples supplied by its input API, including any
   history delivered with an event. It normalizes timestamps and pen axes and
   preserves whether a sample is real, predicted or a correction.
2. The shared editor decides whether the gesture paints, pans or operates another
   tool. For a stroke, it converts input into document coordinates and places
   brush contacts along the path. A resolved contact is called a *dab*.
3. The engine builds a `FramePacket`, defined by `layer-render`. This packet
   borrows the prepared batch data for submission and includes document changes
   and the affected regions. It is not a bitmap or a complete document replay
   on every frame.
4. `WgpuRasterizer`, in `layer-render-wgpu`, records GPU commands to draw new dabs
   into retained layer textures and update their composition. The shared viewport
   presenter draws the canvas at the current zoom and rotation.
5. The platform presents the result using its surface and frame scheduling rules.
   Native integration must also handle resize, suspension and surface loss.

## CPU and GPU responsibilities

The CPU performs ordered, comparatively small operations: input interpretation,
brush dynamics, document edits, layout, dependency tracking and command encoding.
The GPU evaluates canvas pixels, including brush coverage, blending, masks,
filters and composition. Native widgets are drawn by their own toolkit; the
canvas renderer does not draw the application's controls.

`wgpu` provides access to Vulkan, Metal, D3D12 and browser WebGPU. The renderer
requires a hardware GPU and has no CPU painting fallback. Imported image and
brush source bytes may be retained in CPU memory for upload and project saving;
this is different from maintaining a CPU copy of the rendered canvas.

[Rendering and composition](internals/rendering.md) explains the GPU resources
and the explicit export, thumbnail and color-sampling readbacks.

## Scheduling and responsiveness

Each session has one ordered owner for engine and renderer mutations. Native
hosts use queues and render workers to keep input collection and native UI work
independent of GPU waits. The web host runs synchronously on the browser event
loop, using animation callbacks to schedule updates; shared code does not require
threads or shared memory.

Frame work is incremental. New samples produce new contacts, unchanged layer
results can be reused, and camera movement can redraw the viewport without
rasterizing the artwork again. UI notifications separately identify affected sections of editor state, such as
the layer list or tool settings. These are not pixel rectangles: they tell hosts
which controls need refreshing.

Shader startup is staged. The host can show controls and paper while the renderer
prepares the current document and brush, then the remaining catalog. Painting
waits for the required resources; showing the first frame is not the same as being
ready for a stroke. Native compilation workers and incremental web preparation
implement the same dependency ordering.

Use the [testing and performance guide](development/testing.md) to measure frame
cost and input-to-display latency on the target device.

## Files and other host services

Shared code defines file requests, save checkpoints and close decisions. The host
opens pickers, reads or writes bytes and reports completion. Project decoding and
encoding belong off the input path. An incoming document is validated before it
replaces the live one; a failed or cancelled save must not mark a document clean.

The [document guide](internals/documents.md) explains this contract. Platform
storage APIs differ, so a shared request does not by itself establish that every
client implements the corresponding UI or provides the same storage guarantees.

## Where to read next

- [Workspace and UI](ui/README.md) follows a command from a widget to shared state.
- [Brushes](internals/brushes.md) follows pen samples through stroke generation.
- [Platform integration](platforms/README.md) describes the native boundaries.
- The [package table](../README.md#package-layout) links to each crate's entry point.
