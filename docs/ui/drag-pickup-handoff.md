# Remaining drag pickup work

Tiles: hold then drag for every device. Row bodies: mouse immediate; touch/pen
hold first and scroll before holding. Grips and title/tab bars: immediate for all.
Keep same-contact menu-to-drag, cancellation, and undo/redo.

- **Android:** gate tile pickup on hold; stop treating pen row drags as mouse.
- **Apple:** add tile hold gating and whole-row pickup; currently only row grips drag.
- **Windows:** gate tiles on hold; preserve contact when menus open; verify and
  enforce row/grip device rules, including row whitespace.

Details: [convention](drag-and-reorder.md), [source inventory](drag-inventory.md).
