# UI interaction rules

When adding, changing, or reviewing draggable UI, follow the application-wide
[drag and reorder convention](docs/ui/drag-and-reorder.md). It is the required
behavior on GTK, Web, Android, macOS, iPadOS, and Windows:

- Reorderable button/tile bodies require **press, hold, then drag** with mouse,
  touch, and pen.
- Grab handles and title/tab bars support **press, then drag without a hold**
  with every device. Normal movement slop still applies.
- Reorderable list-row bodies require **hold then drag for touch and pen**;
  mouse can drag immediately.
- An explicit list-row grab handle starts dragging immediately for every device,
  including touch and pen. The handle rule takes precedence over the row rule.
- Before a touch/pen row hold completes, preserve normal scrolling. A completed
  touch/pen hold can open the existing context menu; dragging with that contact
  closes it, while release without dragging retains it. Preserve cancellation,
  ordinary clicks, editing, and one-step undo/redo.
- Mouse holds never open context menus. Mouse tile holds only arm reordering;
  use secondary click or the existing keyboard action to open a mouse menu.

Classify the visible hit target and actual pointer device. Pen is not mouse for
list arbitration; an icon tile does not become a tab bar because its payload is
`DockItem::Panel`. Apply the same rules to retained drawer and toolbar views.
Keep native timing/slop and input capture in the host, and shared validation,
drop behavior, layout publication, and history in Rust.

The [drag inventory](docs/ui/drag-inventory.md) records implementation gaps;
current code and historical acceptance notes are not exceptions to this rule.
