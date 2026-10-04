# layer-core

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-core` owns authored artwork, working state, reversible edits, brushes and
the `.capy` package codec. The engine edits these records; renderers borrow typed
scene views to reconstruct the image.

## Document and history

`Document` owns `Artwork` and `WorkingState`. Typed stores separate ordered
stacks and placed occurrences from paint, coverage, effects, saved selections,
guides and outputs. Working selection and editing targets stay outside the
portable artwork. `SceneView` borrows records; `SceneSnapshot` retains immutable
roots and evaluation context for queries, previews and pixel operations.

`Editor` applies typed `Edit` values and retains inverse changes for undo/redo.
Strokes retain pen samples and their `BrushSnapshot`. Immutable raster revisions
and resources share storage across artwork, history and accepted captures.

`Editor::capture` produces an `ArtworkCapture` with an owner-bound checkpoint.
`PreparedPackage` enumerates authored records and resources, then writes the
package without exporting working state or history. Shared open outcomes separate
editable artwork from preserved or recovered package views. Hosts execute file
transport; shared UI policy handles admission, replacement and save acknowledgement.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) | `Document`, `Editor`, typed `Edit`, strokes and brushes. |
| [authored/](src/authored/mod.rs) | Artwork, working state, typed stores, identities and scene access. |
| [layers.rs](src/layers.rs) | Blending, command coverage, raster operations and geometry planners. |
| [effects.rs](src/effects.rs) and [effect_catalog.rs](src/effect_catalog.rs) | Filter programs, values and catalog validation. |
| [presets.rs](src/presets.rs) | Built-in brush definitions. |
| [package/](src/package/mod.rs) | Record adapters, resources, capture transport and package codec. |
| [project.rs](src/project.rs) | Brush assets and artwork admission limits. |

Read [documents and edits](../../docs/internals/documents.md), the
[authored model](../../docs/reference/authored-model.md) and the
[package format](../../docs/reference/capy-package.md) before changing ownership
or persistence. The [testing guide](../../docs/development/testing.md#checks-by-change-type)
covers shared model checks.
