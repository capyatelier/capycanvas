# layer-ffi

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-ffi` exposes offscreen drawing through a C interface. It is used by the
benchmark harness and diagnostic tests, and can be called by other native code.
It creates a headless GPU renderer; interactive application clients use their own
platform bridges and surface integration.

The crate builds as a Rust library, a C-compatible dynamic library and a static
library. C declarations live in [`include/layer.h`](../../include/layer.h), with
the implementation in [src/lib.rs](src/lib.rs).

## Handle and call flow

A `LayerCanvas` handle owns a drawing engine and renderer. The caller submits
batches of pen records, invokes frame drawing, and can request metrics or explicit
image export. Foreign enum values are validated. Callers must supply valid handles and
appropriately sized, aligned buffers.

Each handle has one mutable owner; mutable calls must not overlap. Queue saturation
is reported, so callers can retain input that was not accepted. The API exposes
completed-work waits and image copying for tests and benchmarks. These operations
can block and are not a native window presentation path.

## Where to go next

The [C interface reference](../../docs/reference/canvas-ffi.md) describes ownership,
input timing, status codes and readback. [`layer-bench`](../layer-bench/README.md)
provides a complete Rust caller of the exported C functions. For interactive
native integration, start with [`layer-host`](../layer-host/README.md) and the
[platform guide](../../docs/platforms/README.md).
