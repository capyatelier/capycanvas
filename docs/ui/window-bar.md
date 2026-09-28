# Workspace title bar

[Workspace and UI](README.md) · [Default workspaces](default-workspaces.md) · [Drag convention](drag-and-reorder.md)

The title bar is a workspace-owned arrangement of individual controls, not a
dock for toolbar containers. Its left, center and right regions hold tools,
menus and informational items; each workspace also sets the bar size and
whether the canvas readout shows. Sketch's arrangement is in
[default workspaces](default-workspaces.md#arrangements). The canvas extends
behind the transparent bar.

## Customize inline

Choose **Window → Customize Title Bar…** or use an item's
secondary-click/long-press menu. The editor is a full-width horizontal strip
below the bar, with no repeated heading or size label. Components sit on the
left; size, Show footer and Cancel/Done stay together at the trailing edge.
The strip uses one row when it fits and wraps at narrower widths, keeping
every component and confirmation control reachable. There is no separate
designer or opening shortcut.

- The palette offers **Add Tools…**, Capy, Main Menu, Menu Labels, Settings,
  Workspaces, Document Title, Clock, Battery and Space.
  Already-present singleton components are omitted. Removing one returns it
  to the palette; tools disappear and can be added again using Add Tools.
- Drag a component from anywhere on its chip into the bar; there is no
  click/tap-to-add action or persistent insertion cursor. The live drag preview
  determines its position. There are no Left/Center/Right insertion buttons.
  An empty center reserves up to one third of the usable width (capped at
  320 logical pixels) while editing, with visible gaps separating the regions.
  It stays centered and never covers side items or native window controls.
- **Add Tools…** is a palette component, not a separate top-row action.
  Dropping it in the bar opens the same searchable, multi-select
  picker used by toolbars. No Tools placeholder is saved. Filtering preserves
  selection; confirming inserts tools in selection order. Picker Cancel/Escape
  returns to the editor without adding anything.
- Whole items and bank chips drag immediately after native movement slop, with
  mouse, touch and pen. Grips are visual hints, not separate buttons or hit
  targets; this is the editor exception in the [drag convention](drag-and-reorder.md).
- While held in the bar, the item tracks horizontally and its neighbors slide
  to make room. Reorder thresholds use the same frozen-geometry algorithm as
  tab groups, so animated neighbors do not cause oscillation. Moving more than
  half a tile beyond the bar detaches the item. It then follows the pointer in
  both axes with the original grab offset; a red outline indicates removal.
  Returning inside reattaches it. Releasing detached outside removes an
  existing item, but discards a new component. Escape, focus loss, source
  replacement or window resizing cancels the active drag.
- Click/tap a bar item to select it. **Left/Right** moves it one position,
  crossing into the adjacent section at a boundary. **Delete/Backspace**
  removes it. Native Tab, button activation and Menu/Shift+F10 continue to work;
  there are no other custom editor shortcuts, arrangement buttons or Help.
  The overflow menu also lets you select a hidden item for keyboard editing.
- **Small / Medium / Large** resizes the bar and its icons together. Items
  have consistent 6px gaps; **Space** adds exactly one tile, never flexible
  space. Native window controls remain toolkit-owned and fixed.
- **Show footer** toggles the bottom-right canvas readout (zoom and rotation),
  which opens the zoom menu and field.
  Its position is not customizable. Menu labels are added or removed as a
  component, with no separate Show Menu Bar toggle.
- **Done** commits the preview; **Cancel** restores the arrangement and canvas
  visibility from when editing began. Closing without Done does not save the
  preview. There is no Reset Bar or editor undo/redo stack. A completed edit
  is one ordinary workspace-history change, separate from drawing history.
  Drag motion itself never mutates the session or saves intermediate layouts.

Adjacent icon controls (tools, Main Menu, Settings, Full Screen and a region's
More button) join one flush bar that is exactly one tile tall, so a Small bar
matches a Small toolbar tile (36px). Tiles inside a bar are square, fill the
bar's height with toolbar-style feedback and use the toolbar tile gap:
`HeaderSize::gap()` is 2px at Small and Medium and 4px at Large, like
`TileStyle::gap()`. Capy gets its own single-tile bar. Multiple drawing tabs
form one full-height strip with the same gap; each tab's 24px close button
keeps equal end and vertical insets so it stays concentric with the tab. Every
title-bar surface (bars, menu labels, the tab strip, the document title, clock,
battery, compact menus, the native close button and the Zen Capy) is a glass
chip filled with the canvas surround color, so it vanishes over the surround and
lets artwork show through at the [panel transparency](panel-transparency.md)
level; at Off it is opaque. Hover and press add overlays to that surface. The
selected workspace and tool use the header selection tint, and the selected
drawing tab the panel color; see [theme colors](theme-colors.md). Capy, menu labels, the switcher, the document title or
tabs, clock, battery and Space stay separate with 6px gaps, and customization
shows every item separately. Drawer origins
fill their whole tile with square bottom corners until their drawer has closed.
An open action drawer (for example Color or Layers) gives its tile neutral grey
feedback, not selection blue. Selected drawing tools retain their blue fill;
native keyboard-focus indication remains independent of both states. The same
distinction applies to toolbar tiles, including those inside another drawer.
Menu labels use the workspace selector's track: one continuous 36px bar with
a 5px inset and 26px capsule items, 2px apart. Text keeps its size, so these
tracks and the clock stay 36px tall and centered in Medium and Large bars.
Text items use 8px side padding, so the full menu still fits beside a centered
title in a 1200px-wide window.
Native window-control targets grow equally in both axes, with 6px outer clearance.

Menu Labels compacts to an icon-sized menu inside its own item when space is
short. At narrow widths, each region overflows whole items into a More menu. Tools
still open their normal drawers, anchored to the visible overflow control;
resizing does not change the stored arrangement. Workspace choices compact to
a menu when needed. Clock and battery items occupy space only in fullscreen,
even when included in the saved bar. Both have editable placeholders in the
builder while windowed; battery also has a placeholder on devices without one.
Fullscreen is a Web-only title-bar component. GTK keeps F11 and View → Full screen,
but excludes the tile from its bar, overflow menus and palette, including when
loading a saved arrangement containing it. Other default controls stay aligned
across hosts without replacing each workspace's distinct tools/panels.

Removing all navigation items exposes a recovery menu; native close is always
outside customization. Outside editing, unused caption space and informational
items move the desktop window; while editing, item bodies move the item and
empty space clears the keyboard selection without setting an insertion point.
Actual controls own their clicks, holds and context menus.
An outside contact on the bar or a toolbar dismisses a tool drawer without
consuming the target's normal click or drag. Another eligible tool can switch
the drawer in one click; clicking its current opener toggles it closed. Contacts
inside the drawer or its connector stay inside. Canvas dismissal consumes that
first contact so it cannot leave a mark.

## Ownership

The builder's model, validation, overflow policy, tool activation, drawer
origins, bar membership and history are shared Rust. Web implements the same bar
and inline editor in `header.js` with the shared `HeaderDrag`; its `#header`
uses `touch-action: manipulation` and tracks the pressed contact itself. Hosts own native widgets,
measurements, caption behavior and device timing, and paint the same bars,
surfaces and selection roles. macOS has no Main Menu or menu labels because the
system menu bar owns them. Zen hides the title bar like other chrome; see
[shared UI](shared-ui.md#window-chrome-and-zen-mode).

## Checks

Run native input in a private compositor, never on the user's desktop:

```bash
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_picker_journey
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_drag_only_bank_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_catalog_preview_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_managed_input --native-storage
LAYER_MOTION_VIEWPORT=640x600 bash tools/performance/workspace-motion.sh gtk --native-test=native_header_overflow_input
bash tools/performance/workspace-motion.sh web --tool-picker
```

Web runs `tools/performance/workspace-motion.sh web` with `--title-bar`,
`--title-bar-state`, `--title-bar-feedback`, `--title-bar-overflow`,
`--menu-labels`, `--compact-workspaces` and `--header-controls`.

Other cases in `apps/layer-linux/src/workspace_header_tests.rs` cover editor
controls, keyboard editing, the 640×480 minimum window, spacing in both themes,
hold menus, caption cancellation, drawer dismissal, slide-to-remove (run at 1×
and 2×), overflow drags, the empty center target and window actions. Inspect
their screenshots as well as their assertions.
