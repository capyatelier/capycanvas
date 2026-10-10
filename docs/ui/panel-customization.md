# Panel and toolbar customization

[Workspace and UI](README.md) · [Drag convention](drag-and-reorder.md)

## Tool variations

Toolbar and title-bar controls derive their tool groups in shared Rust. A
predefined `ToolSlotId` remembers its variation across workspaces; a brush command
exposes its media or presets, and Sketch's Brush, Sculpt and Select commands
expose their existing sets. Figure, Ruler, Gradient, Move and Fill commands also
expose their variations. Existing command controls gain this behavior without
changing saved layouts. Brush media (`ToolGroup`) and shortcut families
(`ToolFamily`) keep their own meanings.

Clicking an inactive group selects its remembered variation immediately. Clicking
an active group opens its full drawer, and clicking again closes it; this does
not wait for a double-click timer. With a tool drawer already open, selecting
another group switches the drawer. Eyedropper and Color Picker keep their existing
return-to-paint behavior.

The triangle is a decorative group indicator. Clicking anywhere on the button,
including the triangle, performs the same tool selection or drawer action.
Secondary click opens variations above the existing customization actions.
Keyboard context-menu actions and touch or pen holds use the same shared choices.
Mouse holds only arm reordering; a drag closes a held menu and suppresses the
following click. Menus show icons, checked choices, availability and shortcut
hints. An unavailable remembered tool keeps its variations reachable.

Every tool group uses the same softly rounded bottom-right triangle in Paint,
Photo and Sketch, in both toolbars and the title bar. GTK, Web, Android and
Windows render the shared `tool-group` icon. Brush categories remain groups even
when they contain only one preset; pinned brush presets and individual leaf
tools have no group marker. The triangle's painted edges have at least 6px of
right and bottom clearance, including Small, Medium and Large title-bar tools;
the corner stays part of the tool button's primary target, without a separate
focus stop or menu action. Title-bar overflow preserves these interactions in
scrolling lists; see [the window bar](window-bar.md).

The tile icon follows the remembered variation, including brush media such as
Marker, Pastel, Watercolor, Oil and Spray. Brush category tooltips also name the
remembered medium. Brush size and opacity do not change that identity. The full
grouped drawer presents sibling variations, the active tool's presets or modes,
and its settings.
`ContentDrawer.tool_set` reuses `ToolSetView`. Menus, drawers, Tool Options and
Tool Set panels project sibling choices from the same shared group definition,
including their order, icons and availability. Presets and modes follow the
active sibling. Selecting a group retains its origin, so overlapping groups in
a custom layout stay distinct; moving a tile preserves that origin. Shortcuts
keep a matching origin or resolve the first matching slot in layout order.
Temporary held tools preserve the permanent origin. Paint's manual and automatic
selection groups stay separate, Photo keeps its Marquee and Lasso pairs, and
Sketch's single Select group contains all eight selection tools. Choosing a
sibling updates the retained drawer at the same origin.
Sketch's Brush and Sculpt retain their three-column drawers. Tool Options uses
the same complete projection through More tool options.

Tool Set category buttons fill their grid cells, so hover, selection and input
cover the whole category. A thin horizontal divider separates categories from
tools when both are shown, using the toolbar's inset separator style.
GTK tool and filter rows keep their full preview and caption height when the
panel has room. Shorter panels bring the caption into the preview area before
scrolling, reducing row height by about a quarter. Every part of a tool row
shows the same action tooltip.

`EditingState.tool_slots` remembers one choice per tool slot across layouts,
outside document and layout history. Brush groups use `ToolMemory` for presets
and parameter overrides. Permanent selections through shortcuts, drawers and
panels update matching slots. Temporary held tools preserve these choices and
the saved permanent tool. Moving, duplicating or restoring controls keeps the
shared choice.

Add Tools offers both predefined groups and individual tools, so a variation can
also have a dedicated command button. The initial groups are fixed definitions;
there is no separate group editor. Opening an existing workspace preserves its
layout. Fresh workspaces and Restore Starting Layout use the current defaults.

GTK coverage is `workspace-motion.sh gtk --native-test=native_toolbar_variations_input`;
`--native-test=native_toolbar_variations_overflow_input` focuses on title-bar
sizes and overflow. `--native-test=native_panel_preview_input` checks tool and
filter row compression and expansion; `--tooltips` checks captions across the whole row.
Web uses `workspace-motion.sh web --tool-variations`, and Apple runs
`EditorLaunchTests/testToolGroups` and `testToolGroupsDark`. Run
native popup journeys without `--tablet`, whose synthetic pen serials cannot
authorize popup grabs. Windows uses `exercise-tool-variations.ps1`, including the
title-bar overflow, `exercise-tools.ps1` for the Tool Set divider and
`exercise-expansion.ps1` for the Brush Sizes grid.

## Interaction contract

The Rust UI core owns customization and the complete serializable workspace.
Hosts translate native events and render its menu, dialog and control models.
Customization adds no canvas or rendering path. Zen behavior is described in
[shared UI](shared-ui.md#window-chrome-and-zen-mode).
Saved configurations retain every built-in panel. Shared panel views publish
controls only when the current host supports that panel, matching the panel
picker's availability rules.

- The **Window** menu (see the [workspace manager](default-workspaces.md#workspace-manager))
  has checkable built-in-panel rows and a **Quick Access Toolbars** submenu with
  toolbar visibility, **New Toolbar…** and **Manage Toolbars…**. Hiding removes
  placement, not configuration; checking the item shows it again. A content panel
  opens floating when the dock cannot provide its minimum width.
- **Manage Toolbars…** opens a single-selection list of all toolbars, including
  hidden ones. Select a row, then **Delete Toolbar…** to open the existing
  confirmation. Cancel returns to the selected row; successful deletion clears
  selection and keeps the manager open. Delete is disabled without a selection.
  The empty list shows **No toolbars**. Workspace Undo restores deleted toolbars.
  Rust owns this list, copy, selection, eligibility and actions; hosts only
  render them. Manager selection is transient, not saved workspace data.
- A built-in-panel tab or body opens **Configure Tool Set panel…** and
  **Hide Tool Set panel** (using its actual
  name). A toolbar tab/body has **Configure Tools toolbar…** and
  **Hide Tools toolbar**, likewise using its actual name; display and management
  options are in its configuration column. A single tap on the selected tab toggles
  configuration; an inactive tab selects it. Inputs retain native behavior.
- Empty tab-header space and the group grip target the whole tab group:
  **Automatic** (default), **Icons and active tab name**, **Icons and names**, **Names only**, **Icons only**,
  **Add built-in panel** / **Add Toolbar** submenus,
  and **New Toolbar…**. Submenu checks show current group membership; choosing
  another panel moves it here, never duplicates it. Style is stored once on the
  group's `DockNode::Tabs`, never on individual panels. On GTK, Web, Android,
  macOS, iPadOS and Windows, Automatic fits complete names to the measured
  tab-strip width, reserving icons for every tab and adding names left to right
  while space remains; the shared `TabStyle::automatic_names` chooses them from
  native measurements. Selection does not change name priority. Windows reads
  each group's style from the snapshot's `windows_tab_styles` and fits docked
  and drawer strips. Explicit styles do not adapt:
  icons-and-active-name shows every icon and only the active name regardless of
  count; icons-and-names shows both on every tab;
  names-only and icons-only likewise apply to all tabs.
  Rust resolves `PanelView.tab` into `show_icon` / `show_name`; hosts
  render and measure those contents with a 6px icon/name gap. Whole-group moves
  retain style; merges adopt the destination style, and splitting out a tab creates
  a new group with the default style. Per-tab overrides and their menu/API are removed;
  old per-panel style saves are not migrated.
- A lone built-in panel also offers **Show tab bar**, checked when visible, in
  a separate section in both its panel and group menus. This is independent of
  the group's display style. Unchecking it replaces the header with a 20px bottom-center drag strip using
  the horizontal grip glyph. The strip's menu includes that panel's Configure
  and Hide actions; configuration opens the same live two-column drawer.
  Tearing off a single panel hides its tab immediately. Docking a lone floating
  built-in panel shows its tab again, including when it was floated in an earlier
  drag. There is no saved pre-floating hide flag. Merging into a group clears the
  flag and adopts that group's display style. Dropping on the canvas
  keeps the tab hidden; the menu can show it again. Moving an already-floating
  panel preserves its current choice. Multi-tab groups always show their tabs.
- A ribbon tile targets that tile: **Remove Tool**, **Insert Tools…**.
  Empty standalone ribbon space and its grip target the toolbar: configuration,
  append tools, tile style, rename, duplicate, and hide.
  Configure, Rename, Duplicate and Hide include the actual toolbar
  name; generic creation/addition entries use “toolbar.” Duplicate suggests a
  unique editable name. Names are case-insensitively unique across built-in panels and
  toolbars. Delete is absent from toolbar menus and configuration columns;
  management lives under Workspace. Its confirmation explains Workspace Undo
  with the current shortcut. A tabbed
  toolbar keeps the group grip; its configuration column contains these options.
- Drag pickup, including inside drawers and on collapsed-column icons, follows
  the [drag and reorder convention](drag-and-reorder.md). Secondary click and
  touch/pen holds open the same menu, and the deepest applicable target wins.
  Menus inside text inputs remain native.
- **Configure <name> panel…** / **Configure <name> toolbar…** raises the existing tab group and animates its bounds
  into a two-column layout. The original column remains a live preview of the
  compact panel; a wider configuration column opens on the canvas-facing side.
  Its controls edit the same Rust state, and visibility checkboxes immediately
  show/hide controls in the preview. Existing preview widgets stay parented;
  there is no popover, duplicate tab bar, scaled text or second settings model.
  Outside tap, a tap on the active tab or empty header space, or Escape reverses
  the animation without changing saved docking. Tapping another tab switches
  the preview and configuration without closing the drawer, animating any size
  change from the currently displayed bounds. Dismissal consumes
  the contact rather than activating another control. There is no close button.
  In Zen mode, dismissing the drawer keeps the other panels visible through
  motion/leave; a subsequent canvas-center tap can hide chrome. Both dismissal
  taps are consumed rather than depositing ink.
- The columns share the height needed by the taller content, never reducing the
  original panel height. The configuration column starts below the original
  tabs, aligned with the preview's content area; their bottoms align. Expansion
  uses squared internal seams. A concave tab-to-column transition appears only
  when opening left with the first tab active, where the content colors match;
  right-opening drawers and later tabs have a flat join against the tab strip.
  Exposed outer corners stay rounded, so the surface reads as one panel.
  It is constrained to the window; oversized content scrolls. Left/right panels
  preserve their preview width. Top/bottom
  panels form a compact two-column arrangement growing down/up, anchored to the
  nearest side. Neighboring panels, the reserved dock slot and canvas fit stay
  unchanged. Controls remain 11pt throughout the animation. An open drawer has
  a stronger shadow around the combined surface, not between its columns.
- Toolbar creation and insertion use one searchable multi-select picker with
  icons, descriptions and explicit confirmation/cancel. Creation also asks for
  a trimmed, case-insensitively unique name. Invalid names leave the draft open.
- Tiles move within and between ribbons, including wrapped/vertical/tabbed
  ribbons, after a hold followed by movement with any device. A blue insertion
  line previews the exact core-validated destination.
  Stable tile IDs prevent stale drags from moving a different tile after edits.
- Ribbons wrap and grow where space permits, then clip at their panel boundary.
  They do not scroll: hold-then-drag is reserved for tile reordering; their grips
  move the toolbar without a hold. Clipping
  preserves every configured tile, so resizing can reveal it again. Insertion
  previews only target visible slots and are clipped to the same boundary.
- Tiles use Small 36×36, Large 72×72, or Labeled 108×72 logical units. Large
  icons are 32px; Small/Labeled use 16px. Labeled tiles reserve 36px for the
  centered icon and 72px for the vertically centered name, wrapping at words or
  characters with an ellipsis after three lines. The first tool supplies the
  toolbar's tab icon. All geometry and insertion slots use the same allocator.
- A standalone vertical toolbar narrower than two tiles keeps a center-third
  tab-merge target. Adding tabs grows the group to fit measured native tab
  widths within the available dock budget. Full captions and additional vertical
  ribbon lanes are preferred widths; control minima, including open collapsed
  columns, reserve space for later sidebars and the canvas before they grow.
  Narrow windows shorten captions and clip ribbons while keeping opposing dock
  controls reachable. Automatic growth leaves saved band extents unchanged and
  returns when space becomes available.
  Manual horizontal resizing releases the caption fit.
- A stacked group can instead take its active panel's measured content height,
  leaving the rest of the column to its sibling. Siblings keep a tab bar and one
  tile row before the fitted group shrinks. Dragging the adjacent divider
  releases the fit; moving the whole group keeps it.
- Docking a standalone toolbar resets its cross-axis size to one column on a
  vertical dock or one row on a horizontal dock. Add only the lanes required
  to fit its tiles and trailing grip in the available length; floating widths
  never carry over into a dock. Changing tile size refits single-lane docked
  toolbars and all standalone floating toolbars. Wider manually resized docks
  and tabbed panel groups retain their allocation. Style plus refit is one undo.
- Top/bottom dock bands only accept standalone toolbars. Built-in panels and
  tabbed groups cannot dock at those window edges or stack/merge onto an existing
  horizontal toolbar there. Vertical sidebar stacking remains supported. The
  core rejects these destinations before showing a snap indicator or editing
  the layout. Duplicating a horizontal toolbar creates a separate ribbon;
  resetting docking also keeps each toolbar in its own ribbon.
- Removing a subcolumn reclaims its width when it belonged to the column's
  only split row; full-width rows shrink with that row. Multiple independent
  split rows retain the overall column width and expand surviving branches.
  Apply this rule recursively within nested columns. Removing the edge-most
  neighbor also preserves the survivor's width and shifts it to the edge.
  Hiding, moving, and merging use the same rule and remain undoable.
  Width constraints add sibling minima instead of preserving an impossible
  original ratio: a branch at minimum width stays clamped while the others
  continue to shrink, stopping only at their combined minimum.
- Pulling a panel, group or toolbar more than **80px outside its original
  group** turns it into a live float that follows the pointer. Free canvas has
  no drop indicator; release leaves the float at its current position. Within
  80px of a sidebar or screen edge, a blue line previews snapping. Individual
  panel side targets reach only 40px, leaving a distinct outer zone for docking
  beside the complete sidebar. Top/bottom screen snaps use the nearest 40px for
  outside the sidebars and the next 40px for between them. The bare top edge's
  distances and line both start below the app header, not at the window's top.
  Tear-off, snap reach and Zen proximity share one
  80px Rust constant; the smaller snap zone is half that distance.
  Floating groups retain their width. Without a stored height they default to
  the smaller of 75% of viewport height or the group's fitted content height
  (see content fitting below: scrollers show their controls and four rows)
  plus tabs; drag releases store the height described in the drag convention.
  Standalone floating toolbars default to
  three Small or Large columns, or two Labeled columns, with enough rows for
  every tool. Manual resizing overrides this and survives tab changes.
  Floating groups accept tab merges anywhere inside, never split drops.
  When removal leaves a floating group with only a toolbar, clear the group's
  manual size and tab-fit constraint and restore that toolbar's default grid,
  without moving its anchor. This applies to hiding and moving panels alike.
- Floating panels remain visible in Zen. Empty tab-bar space and the group
  grip move the whole float; standalone toolbars use the entire trailing grip
  strip. Movement is live, preserving the grab offset and native widget/grab.
  Dragged floats rise above other floats. Releasing over a dock or another
  float docks/merges; releasing on free canvas keeps the current live position.
  A click without motion never merges a float with a panel underneath.
  A singleton tab moves its whole group; a tab in a multi-tab group tears off
  only that tab. All eight resize hit regions sit 6px outside the border;
  inside the title bar is for moving, not resizing. Double-clicking the drag
  area of a custom-sized float first restores its default size. At default size,
  a lone built-in panel toggles its tab bar; both the entire bottom grip strip and
  non-tab title-bar space activate this. Multi-tab groups only reset size.
  Standalone toolbars cycle compact grid → vertical column → horizontal row →
  compact grid; the horizontal grip is on the right. Additional lanes are used
  only when a single column/row cannot fit the viewport. Changing tile size
  refits the current preset without changing orientation. All changes animate
  briefly and preserve the anchor where viewport bounds allow. Tab labels never
  activate this behavior. The cycle, size comparison, tab toggle and persistence
  belong to Rust (`DoubleClickPanelHandle`), not host click handlers. On any
  group in a top-level side column, the same drag areas collapse that column.
  Elsewhere, a docked lone built-in panel toggles its tab bar immediately
  without resizing its dock or changing its group's display style. A docked lone
  toolbar resets to one row (top/bottom) or column (left/right), adding lanes
  only when needed to fit. Nested resets preserve side-by-side panel widths.
  Other docked multi-tab groups do not change.
- All durable workspace edits have independent undo/redo, including panel/tab
  moves, tile ordering, names, visibility, style, and floating geometry. Resize
  and live-move gestures coalesce into one entry; cancel restores the start.
  Default shortcuts are Ctrl+Alt+Z and Ctrl+Alt+Shift+Z (Command on Apple).
  Workspace history never changes drawing undo or stores document pixels.
- Dragging a floating panel in Zen does not reveal hidden docks. Reaching an
  occupied screen edge reveals them normally and latches that visibility only
  until the drag ends. While docks are hidden, neither screen edges nor docked
  panels are targets; only tab merging into other visible floats is allowed,
  never side-by-side floating splits. After any drop or cancellation, visibility uses normal
  cursor proximity; neither floating nor docked drops force docks to stay open.
  Floating panels remain visible independently. Native tool-tile DND keeps
  chrome visible for its active grab. Loss of focus cancels ordinary captured
  move/resize gestures and restores their original geometry.

Expansion is transient presentation, not a change to the saved dock. Shared Rust
geometry accepts the host's measured content heights and animation fraction;
GTK drives that fraction with a 200ms
[AdwTimedAnimation](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.TimedAnimation.html),
respecting the system animation setting. The same existing group is reordered
within its parent for both drawing and hit testing, using
[GTK's same-parent reordering](https://docs.gtk.org/gtk4/method.Widget.insert_after.html).
Complex creation/selection uses a dialog, following GNOME's
[popover guidance](https://developer.gnome.org/hig/patterns/containers/popovers.html).
GTK uses [native long-press recognition](https://docs.gtk.org/gtk4/class.GestureLongPress.html).
The compact/all-controls distinction follows the interaction described in
[Clip Studio Paint's brush customization guide](https://help.clip-studio.com/en-us/manual_en/240_brushes/Customizing_brush_tools.htm);
no third-party code or assets are imported.

## Model

Brush Sizes uses 36 × 44 tiles and the Small toolbar's 2 px spacing, six across
in the standard column, with no slider by default. Its 40 presets span
0.7–2000 px. The preview is centred in the tile's upper square and fades behind
the regular-weight, full-size number below. Wider columns fit more tiles; narrower columns wrap them.
Enable Brush Size in the panel configuration to show its slider; the drawer uses
the same controls.

- Built-in panel identities remain stable. Custom toolbar identities are allocated
  independently of their editable display names and can be docked/tabbed exactly
  like built-in panels. There is no fixed toolbar count.
- `DockLayout` contains one panel registry alongside dock bands and floating
  tab groups. Hidden panels have no placement; only toolbars can be deleted.
  Therefore
  geometry, tab styles, visible controls, toolbar names and ordered tiles restore
  atomically. The existing `WorkspaceState` wraps it and Zen mode.
- Floating groups share IDs, tab selection, content, and move operations with
  docks. Native text/content measurements are transient geometry input, not
  saved settings or undo entries. Queued header measurements containing removed
  items are ignored after a layout or workspace change. Hosts do not decide widths, heights, targets,
  naming rules or menu availability. `DragWorkspace` owns tear-off, live movement,
  snapping, singleton/group semantics and Zen reveal state. `ResizeFloating`
  owns eight-edge resizing; `DoubleClickPanelHandle` toggles a docked lone
  panel's tab or refits a docked toolbar, or restores floating dimensions and cycles the applicable
  default layout or tab visibility. `panel_handle_target` determines handle
  eligibility; hosts do not replicate the singleton/content/floating rules.
  Both continuous gestures use Down/Move/Up/Cancel and coalesced history. Native
  hosts retain their gesture on the workspace container, not a replaceable tab
  widget. `ChromeFacts.dragging` is reserved for native tool-tile DND, not shared
  workspace gestures. Session-only workspace history
  starts empty on restore; picker and confirmation drafts remain transient.
- Toolbar tiles have stable IDs and typed controls. The available-button catalog
  includes application commands, brush presets, size presets and color/opacity
  buttons; control values and execution remain in the existing Rust session.
  Saved toolbars validate their panel configuration directly, with unique nonzero
  tile IDs that leave room for the allocator.
- Built-in-panel control catalogs define allowed controls and compact defaults.
  The configuration column shows that catalog, including hidden controls. This is
  metadata for native controls, not a generic widget-tree abstraction.
- Missing configuration in older workspaces receives the original defaults.
  Restore validates references, IDs, names, controls, active tabs and allocator
  state before changing the live workspace. Reset Layout must preserve custom
  toolbars and their contents rather than orphaning them.
- Picker drafts and expanded/context targets are transient UI state, never part
  of saved layouts. Closing/canceling does not partially insert/create anything.
- Context-menu contents, picker validation/search/selection, control visibility,
  and tile drop targets are generated/validated by Rust, not duplicated in hosts.

## Tool drawers

Rust owns tile semantics, drawer open/closed state, contents, placement and
animation geometry ([`drawers.rs`](../../crates/layer-ui/src/drawers.rs)). Hosts
report tile bounds, content measurements and input.

- A tool tile first selects its tool. Pressing it again opens its drawer, and
  pressing the originating tile again closes it; an already selected tool opens
  on the first press. Selecting another tool closes the old drawer.
- Color and built-in-panel tiles toggle their drawer directly without changing
  the tool. Command tiles (New, Open, Save, Undo, transforms, navigation) stay
  immediate actions. Size preset tiles apply a value; the Brush size tile opens
  its panel. Every built-in panel has a drawer tile in the toolbar picker.
- A tool drawer holds the Tool Set and Tool columns; Color opens the Color panel
  and Opacity opens Tool. Columns stack ordinary panel bodies, without tabs or
  grips, separated by vertical rules. Drawers use the same responsive preferred width
  as docked panels. Opening a drawer does not detach a docked copy
  of the panel or duplicate preview generation.
- The drawer joins its tile across the panel gap: the tile extends toward the
  drawer with concave, tab-like joins, and only exposed corners are rounded.
- Top toolbars open downward, left-aligned with the tile; bottom toolbars open
  upward, up to 800 px and never over the title bar; side toolbars open toward
  the canvas, starting 500 px above the tile. Floating toolbars choose by
  orientation and room. Drawers clamp to the usable viewport and scroll.
- An outside click closes a tool drawer without painting or hiding Zen chrome.
  Collapsed-column drawers stay open on outside clicks and close from their
  opening icon; see [stacked columns](stacked-columns.md).

## Host notes

Tabbed ribbons reserve one padded lane below the header; further overflow clips.
Tool ribbons never gain a scroller. Panel, group and resize gestures capture on
the stable workspace, so replacing a tab or grip during tear-off cannot cancel
the gesture; Rust handles every phase, including sizing cycles, history,
snapping and Zen visibility. Disabled command buttons stay inside an enabled
drag and context target, so they can still be removed or moved.

The expanded configuration drawer draws one shadow around both columns: Web
uses one CSS `drop-shadow` on their common ancestor, and GTK wraps the group's
snapshot in one GSK shadow, without darkening the internal seam. Content heights
are measured on layout changes, not every animation frame; interpolation and
placement stay in Rust. Switching tabs animates from the displayed bounds, and
docking into an expanded group keeps its configuration on the active tab.

Content fitting belongs to a tab group: its preferred height is the largest
measured minimum height among its pages, so selecting a shorter page does not
move dock dividers. Compact pages stay whole; scrollers reserve controls and four
rows using the same rule as floating drops. Manual dock resizing opts out of
fitting. Floating groups keep a manually set height across tab selection.
Native content is measured before viewport stretching and returns to Rust as one
measurement set, separate from saved workspace JSON. Floating panels draw above
the status display.

## Checks

```sh
cargo test -p layer-linux native_group_tab_styles -- --ignored --test-threads=1
cargo test --release -p layer-linux native_panel_customization -- --ignored --test-threads=1
cargo test --release -p layer-linux native_panel_expansion -- --ignored --test-threads=1
cargo test --release -p layer-linux native_toolbar_manager -- --ignored --test-threads=1
cargo test --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1
```

GTK tests are thread-affine and need an isolated `CAPY_STORAGE_DIR` and a
Wayland/Vulkan display. Against a served web client (`LAYER_WEB_URL`, or
`--package` for the static build), run `node apps/layer-web/test.mjs` with
`--customization`, `--tab-styles`, `--workspace`, `--toolbar-manager` and
`--parity`. `--parity` compares the `WorkspaceState::default()` fixture at
1200×900 with the `native_web_parity_reference` JSON and PNGs; run it headed,
because its pixel checks sample the presented canvas and glass. Android runs
`apps/layer-android/run.sh test`; `AndroidHostTest` covers the same contracts
with native Compose input, including `toolbarManagerSelectsConfirmsDeletesAndRestores`.

## Collapsed columns

[Stacked columns](stacked-columns.md) replace the former Group panel mode. Each
stack contains complete collapsed columns and owns “Open individual panels” and
Auto-hide. Member footer handles stack and unstack columns with immediate
pickup; sidebar tiles hold before dragging.
