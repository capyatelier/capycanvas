# Compact toolbar edges (GTK)

[Workspace UI](README.md) · [Toolbar components](toolbar-components.md) ·
[Drag convention](drag-and-reorder.md)

Drag a toolbar's handle close to the start, center, or end of an edge to dock
it at its natural length. The target occupies the nearest 24 logical pixels
and a short section around each anchor. The top edge begins below the title
bar. Farther from the edge, the existing full-width/full-height targets remain
available. The short target is shaded while dragging. Native footer insets do
not block the bottom target. Existing sidebar/tab insertion surfaces retain
priority; content panels and tab groups keep their existing docking rules.

Dropping at a compact toolbar's leading or trailing end adds an independent
toolbar to that region. Each toolbar keeps its own handle and identity. The
whole stack aligns together, with the standard workspace gap between bars.
Three regions on the same edge share one strip of workspace space. They keep
their preferred lengths until they would overlap; small windows compress and
wrap them through the ordinary toolbar allocator. Compact regions do not have
stretch/resize dividers.

Sketch's GTK default places size and opacity in the centered left region.
Dragging the handle to the centered right target moves the same toolbar there.
Only untouched included defaults migrate from the previous bottom toolbar;
custom layouts, edited histories, working tool state, and other hosts are kept.

`DockBand.alignment` is optional in saved layouts. A compact region contains
standalone toolbar nodes joined along the edge axis. They use the normal dock
identity, detach, validation, and workspace history paths. Rust owns target
selection, stacking order, alignment, sizing and layout publication; GTK uses
its existing native handles and drag capture. The compact targets are currently
offered on GTK only. Other hosts can resolve saved geometry in the shared core.

Validation includes all edges and tile styles, collisions in small windows,
round trips, conservative migration, rejection of content/tab groups, stacking
and returning to full edge/floating layouts. Native checks use
`tools/performance/workspace-motion.sh gtk` with:

- `--native-test=native_compact_toolbar_edges_input` for mouse/touch docking,
  gradual approaches and live previews, stacking, full-height targets and
  one-step undo/redo.
- `--native-test=native_compact_toolbar_edges_pen_input --tablet` for the same
  handle workflow with GDK pen contacts.
