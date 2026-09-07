# Canvas FFI

`layer-ffi` is the validated command/input boundary used by the headless
benchmark and available to native hosts. Declarations are in
[`include/layer.h`](../include/layer.h).

## Ownership and frame flow

`LayerCanvas` has one mutable owner; mutable calls must not overlap. A native
host may keep it on its UI event loop or a dedicated canvas thread. A Wasm host
may use the same logical API on the browser event loop.

The live sequence is:

1. Submit complete coalesced sample batches with
   `layer_canvas_submit_pen_events`.
2. Retain an unaccepted suffix if the bounded queue returns `QUEUE_FULL`.
3. Call `layer_canvas_draw_frame_for` once from the platform display callback,
   passing the current and expected-presentation monotonic timestamps.
4. Let the platform presenter sample the renderer-owned GPU composite and
   present it without a readback.

The first timestamp advances continuous brushes such as an airbrush even while
the pen is stationary. The second drives predictive feedback and never commits
future paint. `layer_canvas_draw_frame_at` derives presentation time from the
configured horizon. Untimed `layer_canvas_draw_frame` remains a compatibility
entry point.

`layer_canvas_set_instant_feedback` configures the provisional-tail interval,
prediction sources, horizon/distance clamps, tip-lock strength, and correction
shape. Settings are validated and take effect at the next pen-down.

The current C ABI constructs a headless wgpu renderer for tests and benchmarks.
Creation fails when no hardware GPU adapter is available. Surface creation is a
platform adapter concern; it will receive a dedicated native handle API rather
than a host-memory presentation view.

## Canvas pixels

There is no borrowed pixel-view function. Brush, erase, preview, composition,
camera sampling, and presentation remain GPU-only.

`layer_canvas_copy_rgba8_srgb` is an explicit export/test operation. A GPU pass
performs linear-to-sRGB conversion, then the function waits, maps the staging
buffer, and copies the finished bytes to caller storage. It must never be called
from input or display callbacks.

`layer_canvas_wait_idle` is exposed only for completed-work benchmarks and
tests. Production clients do not call it.

## Safety

Foreign-facing phases, tools, flags, and presets are integers validated before
Rust enum construction. Slices are pointer/length pairs, strings are borrowed
only for one call, and no Rust collection, `Arc`, trait object, wgpu object, or
platform handle crosses this command ABI.

Events carry physical surface pixels and a view revision. Both affine transform
directions arrive together so delayed history uses the transform visible when
the event was captured. Predicted samples are flagged and never enter document
history.

Adapters append platform/browser predictions after the real/coalesced history
and set `LAYER_SAMPLE_PREDICTED`. They are replaceable: the next real sample
clears the previous predicted suffix. Platforms without predicted input submit
only real history and use the shared fallback.

## Capacity and allocation

Canvas creation reserves input, stroke-point, contact, and batch capacities.
Renderer upload buffers grow only past a high-water mark. Layer or document
creation may allocate GPU textures; completed strokes may allocate persistent
point storage. The live path uploads one contiguous contact slice and one small
style table per frame. Allocation counting inside wgpu is not an ABI guarantee;
latency and transferred bytes are the relevant renderer measurements.

`LayerCanvasMetrics` reports color, prediction, destination-companion,
stroke-coverage, and canvas-material page counts plus paint-state bytes. These
are diagnostics, not public paging controls. Brush preset IDs 15–24 expose the
ten painter-focused examples without exposing their internal state layout.
