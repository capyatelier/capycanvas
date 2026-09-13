# Workspace window bar (GTK)

The window bar is a workspace-owned arrangement of individual controls, not a
dock for toolbar containers. Painter starts with Capy, Menu, Filters, Lasso and
Transform on the left; workspace choices in the center; and Brush, Blend,
Erase, Layers and Color on the right. It uses medium icons and hides canvas
zoom/rotation. The canvas extends behind the transparent bar.

## Customize inline

Choose **Window → Customize Workspace UI…**, press **Ctrl+Shift+U**, or use an
item's secondary-click/long-press menu. The editor appears immediately below
the bar, without opening a separate designer.

- **Add Item…** searches the same tool catalog as ordinary toolbars, plus
  Capy, menus, workspace choices, document title, clock, battery and spaces.
  Choose Left, Center or Right before adding. Tools and spaces may repeat;
  singleton application controls cannot be added twice.
- Drag a grip immediately, or hold then drag an item body. The three regions
  and insertion line show the destination. Release outside the bar or press
  Escape to cancel. A touch/pen hold also opens the item's menu; a mouse hold
  only arms dragging. See the [application convention](drag-and-reorder.md).
- Click/tap an item in the editor, secondary-click it, or use Menu/Shift+F10
  on its keyboard focus to move earlier/later, move between regions or remove.
  Delete removes a focused item. Tab navigates while editing.
- **Small / Medium / Large** resizes the bar and its icons together. Native
  window controls remain toolkit-owned and cannot be removed or rearranged.
- **Options** controls canvas zoom/rotation visibility and corner, and restores
  the current workspace's baseline window bar. Window → Show Menu Bar controls
  menu labels independently of the other items.
- **Undo / Redo** use ordinary workspace history. Done or Escape exits editing;
  changes are saved continuously with the workspace, not with the drawing.

At narrow widths, each region overflows whole items into a More menu. Tools
still open their normal drawers, anchored to the visible overflow control;
resizing does not change the stored arrangement. Workspace choices compact to
a menu when needed. Battery is absent on devices without one, but has an
editable placeholder in the builder.

Removing all navigation items exposes a recovery menu; native close is always
outside customization. Unused caption space and informational items still move
the desktop window. Actual controls own their clicks, holds and context menus.

## Zen and compatibility

Zen has its original single mode: hide normal chrome, with edge/corner reveal
and the Tab shortcut to return. Explicitly floating palettes retain the
original behavior. There is no partial-Zen toolbar projection or Total Zen
preference. A minimal persistent UI belongs in a workspace instead.

The builder's model, validation, overflow policy, tool activation, drawer
origins and history are shared Rust. GTK owns native widgets, measurements,
caption behavior and device timing. Other hosts retain their current header
projection until ported; this is not a claim of macOS/Windows tablet testing.
