# Drag and reorder convention

[Workspace and UI](README.md) · [Implementation inventory](drag-inventory.md)

This is the required interaction convention for all Capy Canvas frontends.
It applies to existing and new reorderable controls, including their docked,
floating, collapsed-column, and drawer presentations. The convention was set on
2026-09-12; older implementation descriptions do not override it.

## Pickup rules

| Surface under the contact | Mouse | Touch | Pen |
| --- | --- | --- | --- |
| Reorderable button or tile body | Hold, then drag | Hold, then drag | Hold, then drag |
| Grab handle, including a handle inside a tile or list row | Drag without a hold | Drag without a hold | Drag without a hold |
| Title bar, panel/tab strip, individual workspace tab, unused draggable header space | Drag without a hold | Drag without a hold | Drag without a hold |
| Reorderable list-row body, such as a layer row | Drag without a hold | Hold, then drag | Hold, then drag |

“Without a hold” means no time delay is required: ordinary movement slop can
distinguish a click from a drag. “Hold, then drag” requires a stationary hold to
arm reordering before subsequent movement starts the drag. Waiting alone does
not move anything or create an undo entry. Use native long-press timing and slop
where available; a larger distance threshold is not a substitute for a hold.

An explicit handle takes precedence over its enclosing row or tile. Other child
controls retain ordinary click/edit behavior until a recognized reorder gesture
claims the contact. Title/tab bars use the immediate rule even when a tab looks
like an icon button. A collapsed-column icon outside a tab bar uses the tile rule
if it supports dragging. Classify by the visible hit surface, not only by the
Rust drag payload: `DockItem::Panel` can originate at a tab, grip, or icon tile.

Pen includes stylus contacts such as Apple Pencil. Do not infer mouse behavior
from “not touch,” the absence of a touch sequence, or synthesized mouse events
when the native API provides the original device type.

GTK mouse/pen feedback on reorderable tiles (including toolbar/drawer tiles and
collapsed-column panel icons) is the regular pointer before pickup, `grab` after
the hold arms pickup, and `grabbing` during dragging. Release, cancellation,
focus loss, or source invalidation restores the cursor. This feedback does not
change the immediate pickup rule for handles and title/tab bars.

## Scrolling, menus, and cancellation

- Before a touch/pen list-row hold wins, motion remains available to normal list
  scrolling and cancels the pending reorder hold. Lifting, cancellation, capture
  loss, focus loss, or invalidating the source also retires pending holds.
- A plain drag on a tile must not reorder it before the hold. Preserve scrolling
  when its container scrolls; this convention does not add scrolling to toolbars
  whose existing layout deliberately clips overflow.
- Only touch and pen holds open context menus. Mouse holds never open menus,
  including on tiles, list rows, tabs, and handles; mouse tile holds only arm
  reordering. Preserve secondary-click and existing keyboard menu actions.
  Where a reorderable surface has a context menu, a touch/pen hold can show it
  and arm dragging with the same contact.
  Close the menu when dragging starts; retain it when released without dragging.
  Cancelled holds/drags must not leave a stuck menu, pressed tile, or grab.
- Suppress the ordinary click after a recognized hold or drag, so release cannot
  activate a tool, toggle a layer control, or reopen a dismissed drawer. A short
  click/tap continues to perform its existing action. Name editing and native
  text selection keep their existing ownership.
- Handles and title/tab bars can still have context menus, but opening a menu
  must not become a prerequisite for their drag. Mouse list rows drag immediately.
- Once dragging starts, retain the original contact and grab offset across
  reparenting, scrolling, and view updates. Preserve validated drop indicators,
  cancellation rollback, and one undo/redo transaction per completed reorder.

## Shared behavior and scope

Native frontends recognize the device, hold, movement slop, and capture. Shared
Rust remains responsible for allowed moves, docking/tab decisions, drop queries,
workspace publication, cancellation, and history. Do not add host-specific drop
rules or full model refreshes to implement the pickup delay. The optimized
`workspace_update()` contract remains the workspace motion path.

Collapsed-column dividers create a new group using a shared drop area with total
height one third of a tile (12 logical pixels for a 36px tile), centered on the
visible divider. The thin blue preview stays on that divider; adjacent tile bodies
still join an existing group. Clip targets to the scrolling content and exclude
the fixed expand/grip controls. GTK and Web regression:
`tools/performance/workspace-motion.sh gtk --column-drops` (or `web`).

Toolbar dividers use the same one-third-tile target along the toolbar's flow
axis, scaling with its tile size. Dropping a tool there makes it a separate group,
reusing the existing divider and adding another only when needed. Keep the blue
line centered on the divider, normal insertion outside the target, stable tool
IDs, and one undo/redo step. Moving a divider itself keeps ordinary insertion.
Adjacent toolbar dividers collapse to the first divider when inserting, removing,
or moving tools. Perform this cleanup after the complete move, in its undo step;
never remove a destination divider during temporary source removal. Existing
empty groups are also cleaned up when a workspace is opened.

This convention governs pickup for reordering and moving UI containers. It does
not add holds to drawing, selection/transform handles, canvas/navigation drags,
color wheels, curve/gradient handles, sliders, scrollbars, or resize handles.
It also does not make every button, picker result, or library row reorderable;
the control must already offer or deliberately gain that capability.

## Required validation when implementing

Check each changed source with mouse, touch, and pen; mouse/touch success is not
evidence of pen behavior. Cover docked/floating toolbars, wrapped and vertical
tiles, collapsed columns, nested drawers, title/tab bars, and list-row bodies
versus their handles where those presentations exist.

Verify short click, motion before the hold, hold then drag with the same contact,
hold then release, ordinary list scrolling, secondary context menus, source
removal/reparenting, cancellation/capture loss/blur, and one-step undo/redo. The
negative tests matter: a tile must not reorder early, a pen row must not steal
scrolling early, mouse holds must not open menus, and a handle/tab must not wait
for the hold timeout. Measure
steady motion separately from the intentional pickup delay and preserve retained
controls and display-paced rendering.
