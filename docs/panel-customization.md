# Panel and toolbar customization

## Interaction contract

The Rust UI core owns customization and the complete serializable workspace.
GTK, web and Android translate native events and render its menu/dialog/control models.
This feature does not add another canvas/rendering path.

The rollout and per-platform evidence are tracked in
[workspace-management-progress.md](workspace-management-progress.md).

Zen interactions below describe the default **At edges** mode. GTK's **With button**
trial hides floating panels too and disables all hidden docking targets. Button
visibility is a separate setting. See [Zen modes](shared-ui.md#window-chrome-and-zen-mode).

- **Workspace** contains Undo/Redo Workspace Change, checkable built-in-panel
  visibility, a separate toolbar-visibility section, **New Toolbar…**, then
  **Manage Toolbars…**.
  Hiding removes placement, not configuration; checking the item shows it again.
- **Manage Toolbars…** opens a single-selection list of all toolbars, including
  hidden ones. Select a row, then **Delete Toolbar…** to open the existing
  confirmation. Cancel returns to the selected row; successful deletion clears
  selection and keeps the manager open. Delete is disabled without a selection.
  The empty list shows **No toolbars**. Workspace Undo restores deleted toolbars.
  Rust owns this list, copy, selection, eligibility and actions; GTK, web and
  Android only render them. Manager selection is transient, not saved workspace data.
- A built-in-panel tab or body opens **Configure Brushes panel…** and
  **Hide Brushes panel** (using its actual
  name). A toolbar tab/body has **Configure Tools toolbar…** and
  **Hide Tools toolbar**, likewise using its actual name; display and management
  options are in its configuration column. A single tap on the selected tab toggles
  configuration; an inactive tab selects it. Inputs retain native behavior.
- Empty tab-header space and the group grip target the whole tab group:
  **Icons and active tab name** (default), **Names only**, **Icons only**,
  **Add built-in panel** / **Add Toolbar** submenus,
  and **New Toolbar…**. Submenu checks show current group membership; choosing
  another panel moves it here, never duplicates it. Style is stored once on the
  group's `DockNode::Tabs`, never on individual panels. The default shows every
  icon and only the active tab's name. Names-only and icons-only apply to all tabs.
  Rust resolves `PanelView.tab` into `show_icon` / `show_name`; GTK, web and Android
  render and measure those contents with a 6px icon/name gap. Whole-group moves
  retain style; merges adopt the destination style, and splitting out a tab creates
  a new group with the default style. Per-tab overrides and their menu/API are removed;
  old per-panel style saves are not migrated.
- A lone built-in panel also offers **Hide tab**, in a separate section, in
  both its panel and group menus. This flag is independent of the group's display style.
  Its content replaces the header with a 20px bottom-center drag strip using
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
- Mouse/pen secondary click and touch press-and-hold open the same menu.
  Native gesture recognition owns timing/slop; the deepest applicable target
  wins. Recognized hold/drag suppresses the ordinary click, and scrolling cancels
  a pending hold. Existing input/context menus inside text inputs remain native.
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
  ribbons. A blue insertion line previews the exact core-validated destination.
  Stable tile IDs prevent stale drags from moving a different tile after edits.
- Ribbons wrap and grow where space permits, then clip at their panel boundary.
  They do not scroll: dragging remains reserved for tile reordering. Clipping
  preserves every configured tile, so resizing can reveal it again. Insertion
  previews only target visible slots and are clipped to the same boundary.
- Tiles use Small 36×36, Large 72×72, or Labeled 108×72 logical units. Large
  icons are 32px; Small/Labeled use 16px. Labeled tiles reserve 36px for the
  centered icon and 72px for the vertically centered name, wrapping at words or
  characters with an ellipsis after three lines. The first tool supplies the
  toolbar's tab icon. All geometry and insertion slots use the same allocator.
- A standalone vertical toolbar narrower than two tiles keeps a center-third
  tab-merge target. Adding tabs grows the group to fit measured native tab
  widths. Manual horizontal resizing releases that automatic minimum.
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
  Floating groups retain their width and default to the smaller of 75% of viewport height or
  natural active content plus tabs. Standalone floating toolbars default to
  three Small or Large columns, or two Labeled columns, with enough rows for
  every tool. Manual resizing overrides this; changing tabs restores natural
  height. Floating groups accept tab merges anywhere inside, never split drops.
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
  a lone built-in panel toggles Hide tab; both the entire bottom grip strip and
  non-tab title-bar space activate this. Multi-tab groups only reset size.
  Standalone toolbars cycle compact grid → vertical column → horizontal row →
  compact grid; the horizontal grip is on the right. Additional lanes are used
  only when a single column/row cannot fit the viewport. Changing tile size
  refits the current preset without changing orientation. All changes animate
  briefly and preserve the anchor where viewport bounds allow. Tab labels never
  activate this behavior. The cycle, size comparison, tab toggle and persistence
  belong to Rust (`DoubleClickPanelHandle`), not host click handlers. On a
  docked lone built-in panel, the same drag areas toggle Hide tab immediately
  without resizing its dock or changing its group's display style. A docked lone
  toolbar resets to one row (top/bottom) or column (left/right), adding lanes
  only when needed to fit. Nested resets preserve side-by-side panel widths.
  Docked multi-tab groups do not change.
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
  saved settings or undo entries. Hosts do not decide widths, heights, targets,
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

## Implementation and verification

The shared model now implements dynamic panel configuration, stable tile IDs,
transactional creation/insertion, context menu targeting, picker search/selection,
group-owned tab styles, expanded-control view metadata, tile movement and
variable-count ribbon allocation. Workspace JSON stores tab display style on each
group; the retired per-panel display field is not migrated. Tests cover these
policies and native/Wasm type checks pass.

GTK now renders dynamic toolbars, native contextual menus, the searchable picker
and in-place two-column configuration. The original group and preview controls
stay in their parents; closing restores its allocation without changing saved geometry. Native
checks exercise group/panel/tile/empty-ribbon targets, all three tab-group styles,
creation/insertion, control visibility and editing, a GTK drop signal, restore
and reset. Dark/light GTK widget captures are inspected in
`artifacts/ui/customization/` (ignored). Gesture signals test native bindings,
not physical tablet/touch delivery or compositor timing.

Tabbed ribbons reserve one padded lane below the header; further overflow clips.
GTK tool ribbons are explicitly clipped and never gain a scroller.

Web uses `customization.js` for DOM context menus, picker widgets, live controls
and expansion presentation. `app.js` keeps event routing and the existing panel
widgets; toolbars now consume the dynamic Rust tile views instead of a fixed
six-button list. The Wasm adapter exposes the same workspace/context menus,
toolbar prompts, picker, tile layout, expanded geometry and validated drop APIs.
Panel, group and resize gestures capture on the stable workspace, so replacing a
tab or grip during tear-off cannot cancel the gesture. Rust handles every phase,
including sizing cycles, history, snapping and Zen visibility. External resize
strips suppress native selection drags. Touch uses pointer capture for moving
tiles and a cancellable long-press recognizer for context menus; mouse/pen
secondary click uses the browser context event. Disabled command buttons remain
inside an enabled drag/context target, so they can still be removed or moved.

The web expansion animates the existing group's two columns with one CSS
`drop-shadow` on their common ancestor. GTK wraps the whole group's snapshot in
one GSK shadow. Both include the preview, tabs and drawer, without darkening their
internal seam. DOM content heights are measured on layout changes, not every
animation frame; interpolation and placement stay in Rust. Switching tabs uses
the currently displayed bounds as the animation origin. Docking into an expanded
group keeps its configuration synchronized with the group's active tab.

Android renders the same workspace/context menu trees and toolbar prompts in
Compose. `WorkspaceMenus.kt` presents shared items and actions, including nested
menu pages, validation, hints and disabled states. `WorkspaceInput.kt` holds only
native hit geometry, pointer capture and asynchronous reply guards. The stable
workspace captures panel/group/divider/resize gestures; child reparenting cannot
cancel a live tear-off. Tile reordering uses the same validated drop query.
Touch long-press and mouse secondary click open the shared context model.

Floating groups remain composed when Zen hides docked groups. Core bounds drive
their layout, external handles and compact/vertical/horizontal presets; Compose
animates size changes but follows drag coordinates immediately. Native text and
intrinsic content measurements return to Rust as one complete measurement set,
separate from saved workspace JSON. Content is measured before viewport stretching
so resizing cannot redefine its natural height. The shared accepted measurements
also prevent repeated UI measurement dispatches. Both drawer columns share one
outline/shadow, including the flat join when the tab is hidden. Floating panels
draw above the status display. No canvas/GPU integration changes are needed.

Regression coverage:

| Contract | Evidence |
| --- | --- |
| Named toolbars, transactional multi-selection, search/cancel/validation | Core customization tests; GTK and web picker controls |
| Panel/group/tile/empty-ribbon menus and group-owned tab styles | Core context models; GTK gesture/action signals; browser pointer/hold events |
| Live controls, visibility, selected-tab toggle, different-tab switch, outside/Escape dismissal | Core interaction tests; native expansion test; browser customization test |
| Same/cross-toolbar moves, stable IDs and insertion previews | Core slot/move tests; native drop signal; browser native-DND and touch movement |
| Dynamic wrapping, tabbing, clipped overflow and resize | Core allocation tests; GTK ribbon captures; web customization/parity checks |
| Combined shadow, permitted expansion placements, seamless corners, 11pt controls | Dark/light GTK and browser captures; web geometry/shadow comparison |
| Workspace restore, reset retaining custom toolbars, malformed/stale targets | Core validation tests and both host round trips |
| Zen drag/dismiss lifecycle and no accidental ink | Core input tests; native expansion test; browser customization/parity tests |
| Workspace menus, naming/duplicate/delete prompts, visibility and independent history | GTK workspace-management test; browser workspace menu/button/shortcut tests |
| Live tear-off, external resize, first double-click reset, layout/style cycles, floating-only Zen merges | Shared core tests; GTK native input tests; browser mouse/touch workspace regressions |

Android's `AndroidHostTest` covers every row above with native Compose input and
the actual JNI/Vulkan host: workspace menus, nested moves, naming validation,
duplicate/rename/delete/undo, creation, tile reorder, configuration, all eighteen
theme/layout/tile-style combinations, live tear-off continuation, all eight
resize/reset directions, hidden tabs, group collapse, narrow-ribbon merging,
top snap coordinates and Zen hidden-edge/floating-only targets. The complete
suite also retains drawing, settings, numeric-input and lifecycle checks.

The group-tab-style tests on GTK, web and Android check all three modes with
every tab active in turn, in both themes. Shared tests also cover serialization,
undo/redo, whole-group moves, merging into a differently styled group and
splitting out a new default-style group. GTK keeps the existing tab widgets and
changes child visibility; all hosts render the Rust-resolved icon/name flags.
Review captures are in `artifacts/ui/group-tab-styles/` (ignored).

Toolbar-manager coverage uses GTK `native_toolbar_manager`, web
`--toolbar-manager`, and Android `toolbarManagerSelectsConfirmsDeletesAndRestores`.
These exercise selection, hidden entries, cancellation, confirmed deletion,
the empty state, dismissal and undo in both themes. Review captures are in
`artifacts/ui/toolbar-manager/` (ignored).

Run the native tests separately (GTK initialization is thread-affine):

```sh
cargo test -p layer-linux native_group_tab_styles -- --ignored --test-threads=1
cargo test --release -p layer-linux native_panel_customization -- --ignored --test-threads=1
cargo test --release -p layer-linux native_panel_expansion -- --ignored --test-threads=1
cargo test --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1
```

Use isolated `LAYER_SETTINGS_FILE` paths and a Wayland/Vulkan display. After
building/serving the web client, run `node apps/layer-web/test.mjs --customization`,
`--tab-styles`, `--workspace` and `--parity` against `LAYER_WEB_URL`, or add `--package` to test
the static build. `--workspace --gestures` runs only the pointer/touch subset.
Web customization/workspace captures are under
`artifacts/ui/workspace-management/web/`; parity captures remain under
`artifacts/ui/parity/`. These directories are ignored. The browser
harness uses a temporary profile; its software Canvas2D context only inspects
captured PNGs, never renders the application's canvas.

Run Android's emulator suite with `apps/layer-android/run.sh test`. Review PNGs
are in `artifacts/ui/workspace-management/android/` after pulling the run from
`Pictures/CapyCanvasValidation` on the emulator. These are ignored build artifacts.

These tests exercise native bindings and browser-injected input, not physical
tablet delivery or an iPad device. They do not claim a new latency benchmark;
customization leaves the GPU brush/raster path unchanged. Static packaging
includes and fingerprints the new module and its importing app, so service
worker versions follow the changed runtime content. Generated bundles/captures
remain ignored and no third-party code or assets are added.
