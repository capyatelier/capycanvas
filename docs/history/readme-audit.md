# README and documentation audit

Audit date: 2026-09-11. This records the documentation changes made after comparing
the early README with the workspace manifests, shared crates, platform sources
and build scripts. The [current guides](../README.md) are the reading entry point.

| Earlier coverage | Finding | Documentation change |
| --- | --- | --- |
| Core and frame flow | The CPU/GPU boundary was useful, but introduced queue and ABI details before explaining the editor. | Explain document, session, renderer and host first; link to detailed input and renderer contracts. |
| Package list | `layer-host` was missing, and early suggestion-request types were presented alongside actual editor capabilities. | Include the native host layer; remove implications of an integrated AI drawing feature. Legacy model types are not evidence of a user-facing service. |
| Platforms | The README described GTK, web and Android. The adapter guide still treated Apple and Windows as future mappings and named SwiftUI instead of the native AppKit/UIKit implementation. | Document all implemented hosts and distinguish shared capabilities from incomplete UI and hardware acceptance. |
| Documents and tools | Editable projects, masks, selections, figures, transforms, rulers and expanded tool models were mostly absent. Android notes still described drawings as in-memory only. | Add document and UI guides and point to the actual project transports and platform differences. |
| Rendering | GPU residency and incremental work were covered, but newer scene caching, effect dependencies and explicit UI readbacks needed context. | Explain retained layer pages, composition and filter dependencies without claiming every operation is cheap or that export is the only readback. |
| Workspace and settings | Detailed controls and older Zen options obscured the architectural role of the shared UI. | Explain actions, views, configurable layouts and separate persistence; keep detailed contracts linked below them. |
| Build and test commands | Common README instructions mixed platforms and assumed one environment could validate the workspace. | Create a developer entry point, platform setup pages and a testing guide. Consolidate duplicated setup sections. |
| Design records | Proposals, active implementation notes and measured checkpoints were mixed with explanatory documentation. | Group records under `history/`, label their scope and provide current concept guides separately. Remove the redundant iPad acceptance pointer in favor of the Apple record. |
| License | The dual software license and separate branding terms were already documented. | Keep a brief README note and link to the existing terms and publication guidance. |

The source audit included `layer-core` document and project types, `layer-engine`
input and dab generation, `layer-ui` tools/workspace/settings, `layer-host`, GPU
scene and image-stage caching, and each app's native bridge and build entry points.
It establishes documentation coverage, not a new round of platform acceptance or
performance measurements.
