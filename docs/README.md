# Technical documentation

These guides explain how Capy Canvas works and how to develop it. Start with the
[root README](../README.md) for an introduction to the project. Instructions for
artists belong in the [website documentation](https://capycanvas.art/docs/).

[Performance targets](PERFORMANCE_TARGETS.md) sets the minimum frame rate for
every tool, brush and movement. It covers three hardware tiers and records the
newest measurements. Check changes that affect frame generation against it.

## Understand the code

Read [Architecture](architecture.md) first. It introduces the shared editor and
follows an input event through to the GPU. The guides below cover one system at a
time and link to implementation details when they become relevant.

| Topic | What it explains |
| --- | --- |
| [Documents and edits](internals/documents.md) | Layers, strokes, undo, editable projects and file-operation ownership. |
| [Session recovery](internals/session-recovery.md) | Automatic restart, private history, crash-safe publication and bounded cleanup. |
| [Workspace and UI](ui/README.md) | The editor session, commands, configurable panels, docking and Zen mode. |
| [Settings and persistence](ui/settings.md) | Shared defaults and validation, shortcuts, saved preferences and workspace state. |
| [Rendering and composition](internals/rendering.md) | GPU storage, incremental updates, masks, filters and readback. |
| [Brushes](internals/brushes.md) | Brush definitions, input dynamics, dab placement and different GPU execution paths. |
| [Input and stroke feedback](internals/input.md) | Pen history, coordinate transforms, prediction and estimated-sample corrections. |
| [Platform integration](platforms/README.md) | Native toolkits, graphics backends, host responsibilities and port differences. |

## Build and contribute

[`AGENTS.md`](../AGENTS.md) lists the rules every change follows. The
[developer guide](development/README.md) covers building each client, the shared
[environment](development/environment.md) and [test devices](development/devices.md),
[testing](development/testing.md), [writing docs](development/writing.md) and
[publication](development/publication.md). The [commit guide](COMMIT_GUIDE.md)
covers commits and pushing. The [Apple](APPLE_PORTING_GUIDE.md) and
[Windows](WINDOWS_PORTING_GUIDE.md) porting guides describe how features that land
on GTK, Web and Android reach the other clients.

## Detailed references

These documents assume familiarity with the concept guides above.

| Area | References |
| --- | --- |
| Document files | [Project format](reference/project-format.md), [authored model contract](reference/authored-model.md), [package contract](reference/capy-package.md), [binary payload boundaries](internals/binary-payloads.md). |
| Brushes | [Dab layout and raster rules](brush-renderer.md), [GPU brush stages](reference/gpu-brush-engine.md), [painterly paint state](reference/painterly-paint-state.md). |
| Input | [Stroke feedback and platform mapping](reference/instant-stroke-feedback.md). |
| UI | [Shared UI contract](ui/shared-ui.md), [panel customization](ui/panel-customization.md), [numeric controls](ui/numeric-controls.md), [theme colors](ui/theme-colors.md). |
| Extensions | [Runtime filters](reference/runtime-filters.md). |
| Distribution and performance | [Performance targets](PERFORMANCE_TARGETS.md), [Web/PWA packaging](development/web-packaging.md), [GPU benchmark workloads](development/gpu-raster-benchmarks.md). |

## Design history

[History](history/README.md) keeps research and design records that explain why
the current design is the way it is. Their claims apply to when they were
written; the guides above describe the application as it is now.
