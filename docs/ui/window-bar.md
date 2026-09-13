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

- The customization panel reuses the toolbar Add Tools search/list component,
  with Capy, menus, workspace choices, document title, clock, battery and spaces
  added to the catalog. Drag a row's grip directly into the bar. Row bodies drag
  immediately with a mouse; touch/pen require a hold and otherwise keep scrolling.
  Alternatively choose Left, Center or Right and click the row's **+** button.
  Tools and spaces may repeat; singleton application controls cannot be added twice.
- Drag a grip immediately, or hold then drag an item body. The three regions
  and insertion line show the destination. Release outside the bar or press
  Escape to cancel. A touch/pen hold also opens the item's menu; a mouse hold
  only arms dragging. See the [application convention](drag-and-reorder.md).
- Click/tap an item in the editor, secondary-click it, or use Menu/Shift+F10
  on its keyboard focus to move earlier/later, move between regions or remove.
  Delete removes a focused item. Tab navigates while editing.
- **Small / Medium / Large** resizes the bar and its icons together. Native
  window controls remain toolkit-owned and cannot be removed or rearranged.
- **Options** controls canvas zoom/rotation visibility (always bottom right), and restores
  the current workspace's baseline window bar. Window → Show Menu Bar controls
  menu labels independently of the other items.
- **Done** applies the preview; **Cancel** restores the bar and visibility from
  when editing started. Escape cancels the active drag/menu first, then the editor.
  Closing the window without Done does not save the preview. There is no editor
  undo/redo stack. A completed customization is one ordinary workspace-history
  change, separate from drawing history.

Tool tiles have 6px gaps; drawer origins have square bottom corners while open.
Text menus retain their original 36px outer button height and 6px padding,
centered in larger bars. The workspace selector keeps its pill background.
Native window-control targets grow equally in both axes, with 6px outer clearance.

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
