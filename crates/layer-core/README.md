# layer-core

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-core` describes the drawing independently of its renderer or user interface.
It owns document data, reversible edits, brush definitions and the editable project
format. The engine uses these types while drawing, and the renderer reads them to
reconstruct the image.

## Document and history

`Document` contains layers, strokes and ordered drawing operations. `Editor`
applies `Edit` values and retains the reverse operations for undo/redo. A stroke
stores real pen samples and the `BrushSnapshot` used to draw it, so later brush
changes do not change the meaning of an existing stroke.

Committed samples and source assets use shared storage. History can retain them
without copying all their data, and corrections to estimated pen samples can
replace storage without modifying an earlier snapshot.

`Project` serializes the editable document and its reachable source assets into a
`.capy` file. It validates the data without requiring a GPU. Native file access and
the decision to replace an open document belong to the host and shared UI session.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) | `Document`, `Editor`, `Edit`, `Stroke`, brush settings and shared geometry. |
| [layers.rs](src/layers.rs) | Layer properties, selections, masks and ordered layer operations. |
| [effects.rs](src/effects.rs) and [effect_catalog.rs](src/effect_catalog.rs) | Filter definitions, parameters, instances and catalog validation. |
| [presets.rs](src/presets.rs) | Built-in brush definitions. |
| [project.rs](src/project.rs) | Project encoding, decoding, asset collection and limits. |
| [input_corrections.rs](src/input_corrections.rs) | Updates to previously estimated stroke samples. |

Changes to these types can affect undo, renderer replay and project compatibility.
Start with [documents and edits](../../docs/internals/documents.md), then the
[project format](../../docs/reference/project-format.md) for persistence details.
The [testing guide](../../docs/development/testing.md#shared-tests) covers the
shared model tests.
