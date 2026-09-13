# Remaining drag pickup work

Tiles: hold then drag for every device. Row bodies: mouse immediate; touch/pen
hold first and scroll before holding. Grips and title/tab bars: immediate for all.
Hold menus: touch/pen only; mouse uses secondary click. Keep same-contact
menu-to-drag, cancellation, and undo/redo. Mouse tile holds only arm pickup.

- **Apple:** tile/icon hold gating is implemented with native validation in progress;
  finish disabled/divider/drawer and physical input coverage, and add whole-layer-row
  pickup. UIKit toolbar grip/Undo checks pass with shared native-window clearance.
- **Windows:** toolbar/divider/disabled/drawer tiles and collapsed icons use native
  holds with same-contact menus and stable capture. Native mouse/touch/pen fixtures
  cover docked/floating/drawer tiles, icon tear-off, nested origins and Zen menus.
  Whole layer rows, child controls and grips now use explicit native arbitration;
  mouse/touch/pen fixtures cover docked/floating/drawer rows, scrolling, editing,
  cancellation and history. Pen workspace-tab tear-off exposed capture loss and
  remains open, along with physical input acceptance.

All platforms: collapsed-sidebar expand buttons use compact inward-pointing
guillemets (`»` on the left, `«` on the right), matching the current GTK/Web
controls. Apple and Android use the same directional glyphs. Windows now uses
the shared double-chevron SVGs, centered in the existing expand targets.

Details: [convention](drag-and-reorder.md), [source inventory](drag-inventory.md).
