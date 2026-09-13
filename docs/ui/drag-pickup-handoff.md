# Remaining drag pickup work

Tiles: hold then drag for every device. Row bodies: mouse immediate; touch/pen
hold first and scroll before holding. Grips and title/tab bars: immediate for all.
Hold menus: touch/pen only; mouse uses secondary click. Keep same-contact
menu-to-drag, cancellation, and undo/redo. Mouse tile holds only arm pickup.

- **Apple:** tile/icon hold gating is implemented with native validation in progress;
  finish disabled/divider/drawer and physical input coverage, and add whole-layer-row
  pickup. UIKit toolbar grip/Undo checks pass with shared native-window clearance.
- **Windows:** gate tiles on hold; preserve contact when menus open; verify and
  enforce row/grip device rules, including row whitespace.

All platforms: collapsed-sidebar expand buttons use compact inward-pointing
guillemets (`»` on the left, `«` on the right), matching the current GTK/Web
controls. Apple and Android use the same directional glyphs; Windows still needs
alignment. Keep the shared editor text size and bold weight.

Details: [convention](drag-and-reorder.md), [source inventory](drag-inventory.md).
