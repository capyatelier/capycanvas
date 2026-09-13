# Workspace title bar (GTK)

The title bar is a workspace-owned arrangement of individual controls, not a
dock for toolbar containers. The default workspaces are **Sketch**, **Paint** and
**Photo** (their internal IDs remain unchanged). Sketch starts with Capy, Main
Menu, Filters, Lasso and Transform on the left; workspace choices in the center;
and Brush, Blend, Erase, Layers, Color and Settings on the right.
It uses medium icons and hides canvas zoom/rotation. The canvas extends behind
the transparent bar. Included names refresh to Sketch, Paint and Photo without
resetting saved arrangements, working tools or history. Custom workspace names
are untouched; normal name-collision suffixes remain supported.

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
- Drag a component into the bar, or click it to insert at the marked position.
  Click in the bar to choose a position; focusing an item also chooses a
  position before it. There are no separate Left/Center/Right insertion buttons.
  An empty center reserves up to one third of the usable width (capped at
  320 logical pixels) while editing, with visible gaps separating the regions.
  It stays centered and never covers side items or native window controls.
- **Add Tools…** is a palette component, not a separate top-row action.
  Clicking it or dropping it in the bar opens the same searchable, multi-select
  picker used by toolbars. No Tools placeholder is saved. Filtering preserves
  selection; confirming inserts tools in selection order. Picker Cancel/Escape
  returns to the editor without adding anything.
- Grips drag immediately after native movement slop; item and palette button
  bodies require hold then drag for every device. Touch/pen holds can open the
  existing context menu and continue into a drag; mouse holds only arm pickup.
  See the [application convention](drag-and-reorder.md).
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
- **Show footer** toggles the bottom-right canvas readout (zoom and rotation).
  Its position is not customizable. Menu labels are added or removed as a
  component, with no separate Show Menu Bar toggle.
- **Done** commits the preview; **Cancel** restores the arrangement and canvas
  visibility from when editing began. Closing without Done does not save the
  preview. There is no Reset Bar or editor undo/redo stack. A completed edit
  is one ordinary workspace-history change, separate from drawing history.
  Drag motion itself never mutates the session or saves intermediate layouts.

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
a menu when needed. Clock and battery items occupy space only in fullscreen,
even when included in the saved bar. Both have editable placeholders in the
builder while windowed; battery also has a placeholder on devices without one.
Fullscreen is a Web-only title-bar component. GTK keeps F11 and View → Full screen,
but excludes the tile from its bar, overflow menus and palette, including when
loading a saved arrangement containing it. Other default controls stay aligned
across hosts without replacing each workspace's distinct tools/panels.

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
types, sizes, visibility, empty-bar recovery and defaults),
`native_header_editor_keyboard_input` (menu entry, focus, navigation,
reordering, cross-region moves, contextual actions, removal and cancellation),
`native_header_editor_short_window_input` (640×480 minimum window, full palette,
bounded panel and keyboard access to Done),
`native_header_spacing_visual`
(both themes, all sizes, gaps, corners and grip alignment),
`native_header_hold_context_input`, `native_header_cancel_caption_input`, and
`native_drawer_dismissal_input`. The new `native_header_slide_remove_input`
checks live neighbor shifts, backtracking, grab offsets, detachment, re-entry,
removal and palette return with mouse and touch; run it at 1× and 2×.
`native_header_overflow_drag_input` runs at 640×600 and checks that hidden
neighbors survive preview, removal and Cancel at every size.
`native_header_empty_center_input` removes the center item and restores it by
dropping near either edge of the enlarged target, with mouse and touch at every
size. Run it both at 640×600 and at 2× scale.
`native_header_tools_drop_input` checks the palette-to-modal handoff, and
`native_header_window_actions_input` checks Settings and F11 at all sizes,
including reopening Settings, keyboard focus, fullscreen-only clock/battery
geometry, editable windowed placeholders and exclusion of saved Web-only tiles.
Inspect their captured screenshots as well as
assertions. Physical pen input and non-GTK window managers still require their
own platform/hardware validation.
