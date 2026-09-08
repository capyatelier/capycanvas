# Shared workspace logic audit

The GTK/web visual checkpoint is `3b3d7f8`. The measured parity suite covers
dark/light controls, layout, tool wrapping, settings, and interaction states.
The subsequent 6px workspace/header gaps and 36×36px Zen button are validated
in both hosts; tool-tile gaps remain 2px. Occupied-edge Zen reveal also passes
core and browser tests (80px reveal, unchanged 40px keep-visible margin).

## Boundary

`layer-ui` is the shared application/workspace core above the drawing engine.
It owns application decisions, durable workspace state, and interaction policy.
Hosts collect native events, report measured widget bounds, present state using
native widgets, and manage GPU surfaces/event scheduling. Local widget animation,
text-edit buffers, scrolling, pointer capture, OS drag payloads, and platform
timestamp/coordinate conversion remain host responsibilities.

## Findings and migration

| Logic/state | Current evidence | Required change |
| --- | --- | --- |
| Dock topology, priority, widths, split fractions, selected tabs, visibility | `layer-ui/layout.rs` already owns these | Implemented: retained allocator, validated restore, checked ID allocation |
| Workspace persistence | Layout was serializable, but had no session restore contract; Zen was separate | Implemented: `UiState.workspace` / `WorkspaceState` and atomic `RestoreWorkspace`; fresh-session core, GTK and Wasm/GPU round-trip tests pass |
| Toolbar/menu/panel definitions | GTK and JS separately enumerated tools/menus; JS repeated panel names and six-tile count | Shared typed `UiCatalog` and `MENUS`; `TOOLBAR_CONTROLS` supplies initial defaults. The allocator now derives each ribbon's tile count from its durable panel configuration; see the in-progress [customization work](panel-customization.md). |
| Brush categories/order | GTK hardcoded category order; JS inferred it from preset IDs | Implemented: both consume Rust `brush_categories` |
| Control constraints | Brush size/opacity/pressure ranges and steps repeated in GTK, HTML, JS and Rust validation | Implemented: shared `NumericControl` specifications consumed by both hosts and validation |
| Selected-layer opacity target | Both views searched selected layers before creating an action | Implemented: omitted opacity target resolves against the core's current active layer |
| Drop validation | Both hosts cloned layout and probed a move | Implemented: `UiSession::drop_hint` performs the probe; both hosts call it |
| Divider drags/nudges | JS duplicated grab offset arithmetic and both hosts implemented keyboard increments | Implemented: core `DragDivider` lifecycle, offsets, validation and `NudgeDivider` step; hosts report coordinates/directions |
| Workspace fitting/startup | Hosts computed fitting bounds and decided when to fit the initial document | Implemented: `set_viewport` derives fitting bounds and fits once; `blank` owns new-document defaults |
| Tool-group presentation | Hosts classified standalone ribbons and DOM subtracted tab height | Implemented: resolved groups expose tab visibility and tool geometry; GTK native allocation uses the same tile function |
| Zen visibility/reveal | Frontends owned hidden state, pin rules, and first-contact consumption | Implemented: `UiInput::Chrome` owns hover/visibility, keyboard pin, edge hysteresis and reveal/dismiss consumption; hosts report native popover/title-grab facts |
| Shortcuts | GTK and JS contained separate mappings with different modifier behavior | Implemented: normalized `UiInput::Key`, modifier/editing/modal/popup guards, repeat handling, shared divider nudges; fixed web treating Ctrl+B/E/F as bare shortcuts |
| Canvas input routing | Space-pan, button-pan, pointer ownership and interruption policy repeated in hosts | Implemented: `UiInput::Pointer` / `Blur` owns routing, pointer exclusion, cancellation and Space lifecycle; raw native histories/backpressure stay in adapters |
| Settings modal/draft, commands, document edits, camera math and two-touch gestures | Already in `UiSession` | Retained; widget focus/native popup facts now feed core interaction guards |
| View-only state | Widget references, CSS classes, scroll offsets, focus, popover placement, animations, numeric edit buffers | Retain in hosts; no second application state or layout policy |

## Final boundary review

GTK `workspace.rs`, `input.rs`, `tiles.rs`, and `canvas.rs`; DOM `app.js`, HTML/CSS,
and the Wasm adapter were reviewed after migration. Remaining host state has a
native/view purpose: cached widget/layout snapshots, drag payload and OS capture,
title-grab observation, popup placement/visibility, text edit buffers, scrollbar
offsets, click suppression, raw input history/clock/sequence mapping and queue
retry, theme observation, and GPU presentation lifecycle. Hosts do not mutate a
second workspace model. Palette-cell wrapping, sample-dot drawing, typography,
focus, and widget padding are local component presentation; dock/ribbon size,
wrapping, placement, drag targeting and fitting are shared Rust behavior.

No generic widget tree, binding framework, new renderer, or pixel-copy path was
introduced. `layer-ffi` remains a lower-level drawing-engine ABI, not another UI
implementation. Future native UIs should bind `UiSession`, including its input
and catalog contracts, rather than build application rules on that low-level ABI.

## Verification

- 104 Rust workspace tests pass, including 40 shared-UI tests; seven launcher
  tests and native/Wasm Clippy pass.
- Native Wayland geometry, fresh-window restore, and full controls/docking/ink
  integration tests pass. No X11 or title-drag automation was used.
- Full hardware Wasm/WebGPU interaction and focused GTK/web parity suites pass.
  The full browser test's obsolete empty-bottom reveal expectation was corrected;
  hover event/assertion pairs run together to avoid unrelated desktop motion.
- Core tests cover catalog consistency, invalid atomic restore, fresh sessions
  at multiple viewport sizes, editing restored IDs, stable divider offsets,
  first-viewport fitting, shortcut guards, pan/ink ownership and consecutive
  strokes queued before a frame. Hardware tests verify widgets consume these APIs.
- Dark/light captures retain ≤1 logical pixel geometry differences; fonts,
  shadows, controls, ribbon/tab behavior and Zen checks remain aligned.

Automatic disk storage/named-workspace selection is deferred, not needed for the
implemented versioned save/restore contract. Physical pen-device delivery, native
WM title dragging and device-loss recovery are not claimed as automated coverage.
