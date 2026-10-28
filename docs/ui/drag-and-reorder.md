# Drag and reorder convention

[Workspace and UI](README.md)

This is the required interaction convention for all Capy Canvas frontends.
It applies to existing and new reorderable controls, including their docked,
floating, collapsed-column, and drawer presentations. Older implementation
descriptions do not override it.

## Pickup rules

| Surface under the contact | Mouse | Touch | Pen |
| --- | --- | --- | --- |
| Reorderable button or tile body | Hold, then drag | Hold, then drag | Hold, then drag |
| Saved palette color tiles | Drag without a hold | Drag without a hold | Drag without a hold |
| Customize Title Bar: entire editable item or component-bank chip | Drag without a hold | Drag without a hold | Drag without a hold |
| Grab handle, including a handle inside a tile or list row | Drag without a hold | Drag without a hold | Drag without a hold |
| Title bar, panel/tab strip, individual workspace tab, unused draggable header space | Drag without a hold | Drag without a hold | Drag without a hold |
| Other reorderable list-row bodies | Drag without a hold | Hold, then drag | Hold, then drag |
| Layer-row body | Drag without a hold | Scroll or swipe; use the grab handle to reorder | Scroll or swipe; use the grab handle to reorder |
| Tool bubble face, slider center, color-plate rim or Layers card edge | Drag without a hold | Drag without a hold | Drag without a hold |

“Without a hold” means no time delay is required: ordinary movement slop can
distinguish a click from a drag. “Hold, then drag” requires a stationary hold to
arm reordering before subsequent movement starts the drag. Waiting alone does
not move anything or create an undo entry. Use native long-press timing and slop
where available; a larger distance threshold is not a substitute for a hold.

Windows uses native drag recognition for mouse and pen and native manipulation
recognition for touch. Tab selection remains available until that recognition
admits a drag; mouse drag-distance metrics do not define pen tap tolerance.

Saved palette color tiles are an explicit exception: they reorder immediately
after native movement slop with every device. A short tap selects the color;
a stationary touch/pen hold opens its context menu. Scroll long palettes from
the gaps, scrollbar, wheel or trackpad; dragging near an edge scrolls during
reordering. The lifted color follows the grab point, and neighboring colors slide
around its proposed slot. Hovering previews order without editing the palette;
a valid release commits one move. Resolve the final slot from release coordinates,
independently of earlier asynchronous preview replies. A preview reply belongs to
one request in one contact, including across scrolling. Other interactive tiles
keep the hold rule.

An explicit handle takes precedence over its enclosing row or tile. Other child
controls retain ordinary click/edit behavior until a recognized reorder gesture
claims the contact. Title/tab bars use the immediate rule even when a tab looks
like an icon button. A collapsed-column icon outside a tab bar uses the tile rule
if it supports dragging. Classify by the visible hit surface, not only by the
Rust drag payload: `DockItem::Panel` can originate at a tab, grip, or icon tile.

Keep native controls and their ancestors in place while a contact can still
become a click. Raising, reparenting or replacing a pressed container can cancel
pen and touch capture. Transfer capture after a hold or drag wins, then change
the presentation; a press alone must not change stacking order.
Pointer cleanup must also preserve a menu opened by the native click. Dismiss
menus explicitly when cancellation, a new contact or a drag takes ownership.
Native flyout tracking must retain window cancellation after the contact ends.
Abandoning an unclaimed drag candidate must leave native child clicks available;
native scrolling or an admitted gesture can cancel them. Held menus must keep
the original contact available for dragging. Pending menu replies belong to the
same source and must retire with dismissal, cancellation or a later native press,
including presses on controls outside registered drag sources. Register that
ownership before requesting the menu and retain it through native opening.
Custom hold recognition owns the loaded visual tree, including template and
content children, and refreshes ownership when a contact begins. Native text
editing keeps its own gestures.
Native list-item clicks own row activation; drag completion must not synthesize
a click from distance or elapsed time. Retire an unclaimed candidate when it
leaves the observed surface; captured gestures retain their terminal events.
Native editors and submenus handle Escape before ancestor gesture cancellation.
Windows shortcut owners use `ShortcutKeys`: other keys preview, while unhandled
Escape bubbles through `KeyDown`.

In **Customize Title Bar**, the whole item is a placement surface, not an active
tool button. Its body and decorative grip both drag immediately after slop.
The component bank has no click, tap or keyboard activation: a component must
be dropped into the bar, and Add Tools opens its picker only on a valid drop.
Selecting an existing bar item still supports keyboard move/remove and context
menus. Outside this editor, ordinary tile holds and native window movement keep
their existing rules.

Pen includes stylus contacts such as Apple Pencil. Do not infer mouse behavior
from “not touch,” the absence of a touch sequence, or synthesized mouse events
when the native API provides the original device type.
Layer snapshots publish `drag_from_handle` and `drag_from_body` from Rust.
Every client reads those instead of the row's lock or drop flags, latches
pickup eligibility at the start of a contact, and retires an active drag preview
when its row stops being eligible; closing a swipe cannot turn that same contact
into a reorder. Shared pickup validation rejects locked rows, including inherited
locks: their handle is disabled and their body device list is empty. Destination
drop eligibility stays separate.

GTK and Web mouse/pen feedback on hold-to-reorder tiles (including toolbar/drawer
tiles and collapsed-column panel icons) is the regular pointer before pickup,
`grab` after the hold arms pickup, and `grabbing` during dragging. Release, cancellation,
focus loss, or source invalidation restores the cursor. This feedback does not
change the immediate pickup rule for handles and title/tab bars. Panel tabs and
unused tab-header space use the regular pointer before dragging. Primary contact
selects a tab immediately; retain its native button and ancestors through that
selection so the same contact can become a drag. Capture preview geometry at
pickup from that retained source. Keep its selected appearance current when
selection publication arrives after pickup.

While a tab slides within its pane, the moving tabs indicate the destination;
show no insertion line. Once the tab undocks, show its destination drop hint.

Dragging a title-bar button, a brush or medium row, or a toolbar tool (other
than slider and Tool Options tiles) out of its bar, panel or drawer takes a
[tool bubble](tool-bubbles.md) without a hold. Until the contact leaves that
container it keeps its ordinary scrolling and click behavior, so this does not
add a pickup delay or steal list scrolling. A held tile drag still reorders; when
it is released on the canvas past the 80-pixel line it takes a bubble too.

## Scrolling, menus, and cancellation

- Pen drags scroll lists like touch on every host (GTK's shared pen scroller,
  Web `pen-scroll.js`, Android Compose); mouse drags never pan lists.
- Panel scrollbars appear for mouse interaction. Pen and touch hide them and
  leave the panel edge available to grab handles. Switching devices preserves
  the scroll position and viewport size.
  Hosts classify native input before scrollbar pickup; promoted mouse events
  retain their pen or touch identity. Web and Android use overlay thumbs;
  GTK, AppKit, UIKit and WinUI retain their native scrollers.
- A touch or pen swipe left across a layer row reveals **Lock** (or **Unlock**),
  **Duplicate** and **Delete**. A reverse swipe or an outside click closes it.
  Shared Rust supplies their order, labels and availability. They act on the swiped row and
  retain shared Undo. A locked parent disables changes to its children; Quick
  Mask offers only Delete. The action strip fits the row width and keeps its full
  width throughout the swipe; the moving row reveals clipped buttons without
  resizing them. Full labels remain available through accessibility and tooltips. Swiping right
  on a closed paint-layer row toggles alpha lock. Locked layers cannot toggle alpha
  lock. Release commits one undo step; a short or cancelled swipe changes nothing.
  Swiping right to close Delete does not also toggle alpha lock.
  A displaced layer row cannot start a reorder until it has returned to its
  resting position, including the closing animation. Each host fades the grab handle
  with swipe distance, reaching zero opacity at full reveal and returning to full
  opacity at rest. Keep the handle in the layout and animate opacity with the row's
  existing swipe transform, using native animation or compositor transitions.
  Release and close settle over 150 ms; disabled animations go directly to the
  target without leaving a displaced input surface.
  Web matches its opacity and transform transitions; Compose reads the animated
  shift in `graphicsLayer`; SwiftUI uses the offset's animation transaction.
  GTK and WinUI update native opacity beside their existing swipe transform.
- Group rows use the same right swipe to toggle Pass Through, whether the
  folder is expanded or collapsed. Returning to isolation makes the group Normal.
  Locked groups and groups needed by clipping or attached effects
  cannot enter Pass Through. The shared optional row action controls availability;
  closing Delete, a short swipe and cancellation never change group mode.
- Before a touch/pen list-row hold wins, motion remains available to normal list
  scrolling and cancels the pending reorder hold. Lifting, cancellation, capture
  loss, focus loss, or invalidating the source also retires pending holds.
  Layer-row bodies keep scrolling and swipe gestures; holding opens their context
  menu without arming reordering. Their grab handles reorder immediately.
- A plain drag on a tile must not reorder it before the hold, except on saved
  palette color tiles and the explicit title-bar customization surface. Preserve
  scrolling when its container scrolls; this convention does not add scrolling to toolbars
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
- Workspace-manager grips open Move Up / Move Down on an ordinary click, tap,
  Enter or Space. Movement still starts a drag immediately after native slop;
  a completed or cancelled drag suppresses that menu activation. Row context
  menus use the same reorder commands. Shared Rust supplies Rename and Delete
  for custom workspaces in the row context menu, with inline name editing and
  a swipe Delete action. Pins and grips sit at the right edge.
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
the fixed member grips. The first tile starts at the column top. The workspace
gap immediately above the column prepends a group, with a blue preview on the
column's top edge, while its first group remains unscrolled. This target takes
priority over title-bar stacking for panels and groups; whole columns retain
their stack targets. Only dividers between groups are painted.
GTK and Web regression:
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
Dock and floating edges also reach past their handles; see
[edge reach](panel-customization.md#edge-reach). Sideways edge reach never takes
a contact from a surface in the table above that drags without a hold. Beside an
edge that resizes vertically, a vertical swipe from the top 6 pixels of a tab or
from a row that tears off bubbles resizes instead, and a tab pressed there
selects on release rather than on press. Tool tiles that tear off bubbles keep
every swipe. A hold-to-drag tile yields to edge reach only before its hold arms.
It also does not make every button, picker result, or library row reorderable;
the control must already offer or deliberately gain that capability.

## Floating preview and release size

GTK, Web, Android, macOS and iPadOS keep a dragged panel's visible width and height frozen.
Its grab point follows the contact past workspace edges, and the host clips the
preview at the application surface. A collapsed icon has no visible body to
preserve, so its preview uses the measured/default floating size. Toolbars keep
their compact grid layout.

On a floating release, Rust uses native content measurements at the final width.
A tab group sizes for all of its tabs, so its height does not depend on the
active tab and switching tabs never clips a page:

- Compact content (such as the Color square) and short lists fit their content.
- Long scrollable content uses a budget of 400 logical pixels or half the usable
  workspace height, whichever is smaller. Retain a sidebar's height when it fits
  between the useful minimum and that budget; expand a squashed sidebar.
- Near an edge, scrolling panels shrink down to fixed controls plus four rows
  (or all content when shorter). Continuous scrollers use four 36-pixel units.
  The budget never goes below that useful minimum, within the usable workspace.
  Then move the panel inward as needed. Compact content stays whole.
- Established floating panels retain their size unless edge fitting requires a
  smaller scrolling viewport. Header drags anchor the top; footer drags anchor
  the bottom. Docked releases use the destination's allocator.

The result is stored once in the drag's undo step. Later content changes do not
resize an established float. Hosts report natural content height, fixed control
height, and a native row height; they do not implement their own sizing policy.

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

Layer swipe checks: `workspace-motion.sh gtk --native-test=native_layer_swipe_alpha_lock --tablet`
and `--native-test=native_layer_swipe_mouse` (without `--tablet`), Web
`test.mjs --layer-swipes` or `device.test.mjs --layer-swipes`, and Android
`AndroidTitleBarTest#layerSwipeAlphaLock`. The Android motion measurement is
`AndroidTitleBarTest#layerSwipeFrameTiming` with `-e layerSwipeBenchmark true`
and `-e photo <device-path>` pointing to the mid-tier photo; it writes
`layer-swipe-1.json` through `layer-swipe-3.json` in the test app's external files.
GTK's additional actions run `--native-test=native_sketch_layer_swipe_actions`
and `--native-test=native_sketch_layer_swipe_actions_pen --tablet`, in both themes.
`--native-test=native_sketch_layer_swipe_motion` records three sustained native
touch gestures per theme, counting presentations while the row moves.
The combined drawer runs GTK `--native-test=native_layer_drawer_any_toolbar`, Web
`--layer-properties-drawer`, and Android
`AndroidTitleBarTest#layerPropertiesDrawerUsesNativeSizingAndActions` in both themes.
Android `#layerSwipeFrameTiming` uses `-e layerPropertiesBenchmark true` for drawer transitions;
the existing swipe benchmark accepts `-e layerSwipeDirection left` for the action strip.
Android `#layerInputOwnership` and `#layerScrollbarsFollowNativePointer`, Web `--layer-hold`, Apple
`tests/layer-row-input.swift` and Windows `exercise-layer-pickup.ps1` cover
mouse body pickup, direct-contact handle pickup, stationary body menus,
displaced handles, cancellation and Undo in both themes.
Web `--layer-swipes` also closes and reopens a collapsed panel during a swipe
return and checks that its resting rows can be picked up again.
GTK `--native-test=native_layer_relationship_review` covers group-mode
swipes, clipped owners, effect thumbnail drops, normalized gap feedback and
relationship geometry in both themes at narrow and wide panel widths.
Run this journey with `--native-recovery`. It also
checks outer and inner chain drops and automatic recovery while Motion Blur is
attached, after its removal, and through undo/redo.
Run its pen branch with `LAYER_RELATIONSHIP_PEN=1` and `--tablet`; the tablet
proxy checks pen hold, swipe and cancellation but cannot authorize compositor
drag-and-drop with its synthetic serials.

## Known gaps

- Physical pen and Apple Pencil acceptance is outstanding on most hosts;
  automated suites inject pen events, which do not qualify physical digitizers.
- iPadOS: held collapsed-column icons and the expanded toolbar cases
  (disabled commands, dividers, drawers) lack UIKit coverage.
- macOS: Manage Workspaces rows keep custom vertical row menus, because the
  native menu presenter does not preserve the held contact for dragging.
- Android: check that the layer-row trailing grip's 20 dp hit region matches
  its visible handle.
