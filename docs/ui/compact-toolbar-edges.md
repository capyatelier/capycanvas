# Compact toolbar edges

[Workspace UI](README.md) · [Toolbar components](toolbar-components.md) ·
[Drag convention](drag-and-reorder.md)

Drag a toolbar's handle close to the start, center, or end of an edge to dock
it at its natural length. The visible toolbar reaching an edge activates a
compact target; the grip need not reach the screen edge. The target has a
minimum pointer reach of 24 logical pixels and a short section around each
anchor. Each target is 192 logical pixels long (clamped to one third of a short
edge so regions never overlap). Its preview remains a 24px strip. The top edge begins below the title
bar. Farther from the edge, the existing full-width/full-height targets remain
available farther inward from the visible toolbar. The short target is shaded
while dragging. Native footer insets do not block the bottom target. Existing
sidebar/tab insertion surfaces retain priority: while the pointer is over a
panel group other than a standalone toolbar, that group's tab and body targets
apply even when the visible toolbar reaches an edge. Content panels and tab
groups keep their existing docking rules.

Dropping at a compact toolbar's leading or trailing end adds an independent
toolbar to that region. Each toolbar keeps its own handle and identity. The
whole stack aligns together, with the standard workspace gap between bars.
Three regions on the same edge share one strip of workspace space. They keep
their preferred lengths until they would overlap; small windows compress and
wrap them through the ordinary toolbar allocator. Compact regions do not have
stretch/resize dividers.

Before wrapping a Medium or Large toolbar, a compact region presents it one or
two sizes smaller (Large, Medium, Small) until it fits one lane. The context
menu, saved layout, floating and full-edge placements keep the configured size.
While resolving an edge, Rust steps every wrapping toolbar down together and
repeats until each fits or is Small, so the result depends only on the layout
and viewport and never alternates between sizes. Small and labeled styles are
unchanged; a Small toolbar still wraps on a very short edge. `TileLayout`
publishes the presented `tile_style`, icon size, radius and label metrics with
the tile bounds. Hosts draw docked, expanded and drawer toolbars from that
geometry, so resizing, rotation and divider drags restyle tiles during
layout-only updates. GTK rebuilds an affected strip once, on idle after the
allocation that crossed a threshold.

Sketch's default places size and opacity in the centered left region.
Dragging the handle to the centered right target moves the same toolbar there.
Saved layouts keep their toolbar placement until Restore Starting Layout.

`DockBand.alignment` is optional in saved layouts. A compact region contains
standalone toolbar nodes joined along the edge axis. They use the normal dock
identity, detach, validation, and workspace history paths. Rust owns target
selection, stacking order, alignment, sizing and layout publication. GTK, Web,
Android, macOS, iPadOS and Windows use their existing native handles and drag capture.

## Checks

GTK native tests through `tools/performance/workspace-motion.sh gtk --native-test=`:
`native_compact_toolbar_edges_input` (mouse and touch docking, previews,
stacking and one-step undo), `native_compact_toolbar_edges_pen_input --tablet`,
and `native_compact_toolbar_presentation_input` (Sketch's Medium toolbar
shrinking to one Small column at 960×600 and returning). The Web
`--toolbar-components` journey and Android
`AndroidInteractionTest#toolbarComponentsAcrossDevicesAndLayouts` drag real
handles into every edge target with mouse, touch and pen; see
[toolbar checks](toolbar-components.md#checks).
