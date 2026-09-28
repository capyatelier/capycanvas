# Editor internals

[Technical documentation](../README.md)

Start with [Architecture](../architecture.md), then choose the part of the drawing
path you want to understand:

- [Documents and edits](documents.md) explains artwork, history and persistence.
- [Input and stroke feedback](input.md) explains pen records and prediction.
- [Brushes](brushes.md) follows stroke samples into resolved marks.
- [Rendering and composition](rendering.md) explains how those marks become the
  visible layered image.

Subsystem guides:

- Documents and files: [binary payloads](binary-payloads.md),
  [workspace ownership](workspace-ownership.md),
  [portable photo codecs and color](portable-color.md) and
  [Float32 HDR layers](float32-hdr-scope.md).
- Brushes and rendering: [contact brush engine](contact-brush-engine.md),
  [shader readiness](shared-shader-readiness.md) and
  [tonal selection storage](tonal-performance.md).

The [UI guide](../ui/README.md) covers the controls around the drawing, and
[platform integration](../platforms/README.md) covers native services and surfaces.
