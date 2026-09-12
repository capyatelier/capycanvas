# Remaining drag pickup work

Tiles: hold then drag for every device. Row bodies: mouse immediate; touch/pen
hold first and scroll before holding. Grips and title/tab bars: immediate for all.
Hold menus: touch/pen only; mouse uses secondary click. Keep same-contact
menu-to-drag, cancellation, and undo/redo. Mouse tile holds only arm pickup.

- **Apple:** add tile hold gating and whole-row pickup; currently only row grips drag.
- **Windows:** gate tiles on hold; preserve contact when menus open; verify and
  enforce row/grip device rules, including row whitespace.

All platforms: collapsed-sidebar expand buttons use two chevrons toward the
canvas (`>>` on the left, `<<` on the right). GTK/Web use the shared
`chevron-double-left/right` SVGs; Android/Apple/Windows need to adopt them.

Details: [convention](drag-and-reorder.md), [source inventory](drag-inventory.md).
