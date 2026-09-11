# layer-engine

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-engine` turns ordered input samples into document edits and drawing work.
It uses the document types from [`layer-core`](../layer-core/README.md) and submits
work through the [`layer-render`](../layer-render/README.md) contract. It does not
create windows or implement GPU pixel operations.

## Drawing a stroke

`CanvasEngine` owns the document editor and a renderer supplied by its caller.
It drains pen input, applies the appropriate camera transform and pressure response,
and passes stroke samples through `DabGenerator`. A *dab* is one resolved brush
contact with a position, shape, color and material parameters.

The generator places contacts by distance traveled and evaluates brush dynamics
on the CPU. Seeded variation makes stroke replay deterministic. Each frame collects
new contacts and document changes into a borrowed `FramePacket` for the renderer.

Prediction uses separate preview work that real input replaces. Predicted samples
do not enter saved strokes or undo history. Corrections to earlier estimated
samples can revise the real stroke and trigger replay of the affected work.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) | The public engine API. |
| [input.rs](src/input.rs) | Pen records, the bounded input queue, pressure curves and view transforms. |
| [canvas.rs](src/canvas.rs) | `CanvasEngine`, document edits, frame scheduling and packet construction. |
| [brush.rs](src/brush.rs) | `DabGenerator`, sensor mappings, spacing and deterministic variation. |
| [feedback.rs](src/feedback.rs) and [corrections.rs](src/corrections.rs) | Temporary stroke feedback and updates to estimated input. |

The engine has one mutable owner. Input collection may run separately through the
queue, or on the same event loop in the browser. Shared behavior must work in both
arrangements.

Read [brushes](../../docs/internals/brushes.md) and
[input and stroke feedback](../../docs/internals/input.md) for the concepts behind
these modules. The [performance guide](../../docs/development/testing.md#performance)
includes the engine's input and dab-generation benchmark.
