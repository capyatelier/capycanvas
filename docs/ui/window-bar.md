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

- A compact palette offers Capy, menus, workspace choices, document title,
  clock, battery and spaces. Already-present singleton components are omitted;
  removing one makes it available again. Click a component to add at the marked
  position, or drag its grip directly into the bar. Component button bodies
  require hold then drag with every device; grips need no hold.
- Click in the bar to choose an insertion position, or click its **Left / Center /
  Right** region label to append there. The marker and destination description
  agree. Keyboard users can focus an item and press Enter/Space to insert before
  it. Selecting a hidden item from More also chooses a position before that item.
- **Add Tools…** opens the same searchable, multi-select picker used by toolbars,
  separately from the editor. Filtering preserves selection. **Add Tools** inserts
  the selected tools in selection order; picker **Cancel**/Escape returns to the
  editor without changing its preview. Tools and spaces may repeat. Destination
  validity and the 128-item limit are checked atomically in shared Rust.
- Drag a grip immediately, or hold then drag an item body. The three regions
  and insertion line show the destination. Release outside the bar or press
  Escape to cancel. A touch/pen hold also opens the item's menu; a mouse hold
  only arms dragging. See the [application convention](drag-and-reorder.md).
- Click/tap an item and use **Remove**, or secondary-click/use Menu/Shift+F10
  for move earlier/later, move between regions and remove. Delete removes a
  focused item. Tab navigates while editing. The palette and editor controls wrap
  at narrow widths; their height is measured, not a fixed catalog-sized panel.
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
An open action drawer (for example Color or Layers) gives its tile neutral grey
feedback, not selection blue. Selected drawing tools retain their blue fill;
native keyboard-focus indication remains independent of both states. The same
distinction applies to toolbar tiles, including those inside another drawer.
Text menus retain their original 36px outer button height and 6px padding,
centered in larger bars. The workspace selector keeps its pill background.
Native window-control targets grow equally in both axes, with 6px outer clearance.

At narrow widths, each region overflows whole items into a More menu. Tools
still open their normal drawers, anchored to the visible overflow control;
resizing does not change the stored arrangement. Workspace choices compact to
a menu when needed. Battery is absent on devices without one, but has an
editable placeholder in the builder.

Removing all navigation items exposes a recovery menu; native close is always
outside customization. Outside editing, unused caption space and informational
items move the desktop window; while editing they choose insertion positions.
Actual controls own their clicks, holds and context menus.
An outside contact on the bar or a toolbar dismisses a tool drawer without
consuming the target's normal click or drag. Another eligible tool can switch
the drawer in one click; clicking its current opener toggles it closed. Contacts
inside the drawer or its connector stay inside. Canvas dismissal consumes that
first contact so it cannot leave a mark.

## Zen and compatibility

Zen has its original single mode: hide normal chrome, with edge/corner reveal
and the Tab shortcut to return. Explicitly floating palettes retain the
original behavior. There is no partial-Zen toolbar projection or Total Zen
preference. A minimal persistent UI belongs in a workspace instead.

The builder's model, validation, overflow policy, tool activation, drawer
origins and history are shared Rust. GTK owns native widgets, measurements,
caption behavior and device timing. Other hosts retain their current header
projection until ported; this is not a claim of macOS/Windows tablet testing.

## Regression checks

Run native input in a private compositor, never on the user's desktop:

```bash
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_picker_journey
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_catalog_preview_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_header_managed_input --native-storage
LAYER_MOTION_VIEWPORT=640x600 bash tools/performance/workspace-motion.sh gtk --native-test=native_header_overflow_input
LAYER_MOTION_VIEWPORT=3200x2000 LAYER_MOTION_SCALE=2 bash tools/performance/workspace-motion.sh gtk --native-test=native_header_catalog_preview_input
bash tools/performance/workspace-motion.sh web --tool-picker
```

The GTK picker journey checks native insertion selection, multi-selection across
queries, empty results, nested Cancel/Escape, exact insertion order, singleton
availability, removal, keyboard insertion and parent cancellation. The catalog
case checks mouse/touch early-drag rejection, held bodies, immediate grips and
cancellation on outside drop, Escape, blur and source replacement. Managed input
checks Done, workspace switching, restart and unsaved-preview cancellation.
The narrow case includes hidden-item selection and modal Escape with an empty
search field. The focused Web check protects the shared picker's existing toolbar
destinations; it does not imply that Web implements this GTK editor.

Additional native cases are `native_header_editor_controls_input` (all component
types, options, empty-bar recovery and defaults), `native_header_spacing_visual`
(both themes, all sizes, gaps, corners and grip alignment),
`native_header_hold_context_input`, `native_header_cancel_caption_input`, and
`native_drawer_dismissal_input`. Inspect their captured screenshots as well as
assertions. Physical pen input and non-GTK window managers still require their
own platform/hardware validation.
