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
sidebar/tab insertion surfaces retain priority; content panels and tab groups
keep their existing docking rules.

Dropping at a compact toolbar's leading or trailing end adds an independent
toolbar to that region. Each toolbar keeps its own handle and identity. The
whole stack aligns together, with the standard workspace gap between bars.
Three regions on the same edge share one strip of workspace space. They keep
their preferred lengths until they would overlap; small windows compress and
wrap them through the ordinary toolbar allocator. Compact regions do not have
stretch/resize dividers.

Sketch's GTK, Web and Android default places size and opacity in the centered left region.
Dragging the handle to the centered right target moves the same toolbar there.
Only untouched included defaults migrate from the previous bottom toolbar;
custom layouts, edited histories, working tool state, and unsupported hosts are kept.

`DockBand.alignment` is optional in saved layouts. A compact region contains
standalone toolbar nodes joined along the edge axis. They use the normal dock
identity, detach, validation, and workspace history paths. Rust owns target
selection, stacking order, alignment, sizing and layout publication. GTK, Web
and Android use their existing native handles and drag capture. Other hosts can
resolve saved geometry in the shared core.

Validation includes all edges and tile styles, collisions in small windows,
round trips, conservative migration, rejection of content/tab groups, stacking
and returning to full edge/floating layouts. Native checks use
`tools/performance/workspace-motion.sh gtk` with:

- `--native-test=native_compact_toolbar_edges_input` for mouse/touch docking,
  gradual approaches and live previews, stacking, full-height targets and
  one-step undo/redo.
- `--native-test=native_compact_toolbar_edges_pen_input --tablet` for the same
  handle workflow with GDK pen contacts.

The Web `--toolbar-components` journey and Android
`toolbarComponentsAcrossDevicesAndLayouts` test drag actual handles into every
edge/start/center/end target with mouse, touch and pen, then verify one-step
undo/redo. See [toolbar validation](toolbar-components.md#validation) for the
browser-on-tablet harness and native test setup.
