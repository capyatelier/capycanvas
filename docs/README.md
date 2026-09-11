# Technical documentation

These guides explain how Capy Canvas works and how to develop it. Start with the
[root README](../README.md) for an introduction to the project. Instructions for
artists belong in the [website documentation](https://capycanvas.art/docs/).

## Understand the code

Read [Architecture](architecture.md) first. It introduces the shared editor and
follows an input event through to the GPU. The guides below cover one system at a
time and link to implementation details when they become relevant.

| Topic | What it explains |
| --- | --- |
| [Documents and edits](internals/documents.md) | Layers, strokes, undo, editable projects and file-operation ownership. |
| [Workspace and UI](ui/README.md) | The editor session, commands, configurable panels, docking and Zen mode. |
| [Settings and persistence](ui/settings.md) | Shared defaults and validation, shortcuts, saved preferences and workspace state. |
| [Rendering and composition](internals/rendering.md) | GPU storage, incremental updates, masks, filters and readback. |
| [Brushes](internals/brushes.md) | Brush definitions, input dynamics, dab placement and different GPU execution paths. |
| [Input and stroke feedback](internals/input.md) | Pen history, coordinate transforms, prediction and estimated-sample corrections. |
| [Platform integration](platforms/README.md) | Native toolkits, graphics backends, host responsibilities and port differences. |

## Build and contribute

The [developer guide](development/README.md) lists the build entry points and
explains where to make changes. Setup is documented separately for
[Linux](development/linux.md), [web](development/web.md),
[Android](development/android.md), [macOS and iPadOS](development/apple.md), and
[Windows](development/windows.md).

[Testing](development/testing.md) covers shared tests, host checks and GPU
measurements. [Publication](development/publication.md) covers source and binary
distribution requirements. Contribution priorities are in the
[root README](../README.md#contributing).

## Detailed references

These documents assume familiarity with the concept guides above.

| Area | References |
| --- | --- |
| Document files | [Project format](reference/project-format.md). |
| Brushes | [Dab layout and raster rules](brush-renderer.md), [GPU brush stages](reference/gpu-brush-engine.md), [painterly paint state](reference/painterly-paint-state.md). |
| Input | [Stroke feedback and platform mapping](reference/instant-stroke-feedback.md). |
| UI | [Shared UI contract](ui/shared-ui.md), [panel customization](ui/panel-customization.md), [numeric controls](ui/numeric-controls.md), [theme colors](ui/theme-colors.md). |
| Extensions | [Runtime filters](reference/runtime-filters.md), [C interface](reference/canvas-ffi.md). |
| Distribution and performance | [Web/PWA packaging](development/web-packaging.md), [GPU benchmark workloads](development/gpu-raster-benchmarks.md). |

## Design and validation history

[History](history/README.md) contains proposals, implementation checkpoints,
research and recorded measurements. Those records explain earlier decisions;
their completion claims and open-task lists apply to the checkpoint they describe.
The current guides above distinguish shared capabilities from unfinished host
integration. The [README audit](history/readme-audit.md) records the scope of this
reorganization.
