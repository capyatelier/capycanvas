# Shared state, platform UI

[Technical documentation](../README.md)

## Boundary

`layer-ui` owns application behavior and semantic workspace layout. Native
frontends own widgets, focus, accessibility, styling, event collection, and
surface lifecycle. Web controls use the DOM. Rust does not describe a generic
widget tree or render toolbars, dialogs, or text.

One synchronous `UiSession<R: CanvasRenderer>` owns the canvas engine and UI
state. It calls the existing engine directly; there is no second document
model or command/feedback relay. GTK uses it directly. The web app wraps the
same session with `wasm-bindgen`, alongside its WebGPU surface.

```text
native widgets / DOM ── UiAction / UiInput ──► UiSession
        ▲                                  │
        └── changed regions + UiState ──────┤
                                           ▼
native pen history ── PenEvent ──► CanvasEngine ──► GPU renderer
```

The session has one owner and creates no threads, runtime, blocking waits, or
UI callbacks. A host may run it on its event loop or move the whole session to
a worker and transport actions/snapshots at that boundary. Threading does not
change application semantics. No UI binding serializes canvas pixels.

Canvas idle guards inspect queued pen samples, active strokes and tool contacts.
A completed synchronous tool contact permits its next command before another
frame; requesting a repaint does not keep that contact active. Queued painting
still waits for its samples to be consumed, and deferred edits retain their order.

## Module layout

```text
crates/layer-ui/src/
    lib.rs       public actions, semantic state, commands and control catalog
    session.rs   action handling, engine integration, validated settings updates
    settings.rs  portable preferences definitions, editor and host requests
    shortcuts.rs configurable chords routed to typed actions
    cursor.rs    display-only stamp outlines, pointer modes and camera mapping
    interaction.rs normalized input/reply types and transient interaction state
    layout.rs    ordered dock bands, splits, tabs, moves and allocation
    workspace.rs versioned durable workspace value and restore validation
    camera.rs    affine camera and shared two-touch interpretation
```

`layer-ui` depends only on `layer-core`, `layer-engine`, `layer-render`, and
portable data serialization. It compiles for `wasm32-unknown-unknown` without a
toolkit, OS handles, filesystem, async runtime, or thread requirement. Target
bindings live in the app that uses them; no separate binding framework/crate is
needed for the Rust GTK and Wasm clients.

Picker enums can use `variants!` to keep declaration-order `ALL` arrays with
their definitions, preserving fixed-size arrays and variant attributes.
Tool action groups declare their command members, localized label and segmented or
dropdown presentation together. The catalog publishes `tool_action_groups`,
`tool_action_rows` and `panel_metrics`; resolved tool actions carry their group,
row and icon-only roles. Hosts retain native controls, while Rust owns which
actions belong together. Ungrouped checkable actions use compact checkboxes.
Tool groups declare their engine, label and medium icon together; their getters
expand to constant matches.

`ui_catalog()` describes built-in panels, tool and layer commands,
brush categories/presets, and [numeric input kind, limits, mapping, units and precision](numeric-controls.md). GTK consumes
the same typed constants exposed to DOM through the Wasm catalog; hosts supply
widgets/icons, not separate command lists or numeric rules. The ribbon allocator
derives its item count from each toolbar's saved panel configuration.
`TOOLBAR_CONTROLS` supplies the original defaults. Context menus, pickers and
expanded panels are described in [panel customization](panel-customization.md). `SetLayerOpacity` may omit `id` to
target the live active layer instead of resolving it from a frontend snapshot.

`ContentDrawer` uses one control factory for header and toolbar openers. Its
columns compose Color with Palettes; Layers carries the Properties page state.
Retained layout updates publish transient measured header positions as
`header_presentation`, beside the authored `workspace_layout` patch. Full native
snapshots publish the same field beside `state`; measurements never enter
`state.workspace.layout`. Shared layout uses these positions to keep header
drawers at their current opener. GTK retains the same measurements in its typed
layout when applying panel geometry.
Opt-in native layout updates retain unchanged controls across viewport and density
changes while publishing geometry and panel vectors at the current scale.
Reusable panel views contain controls and have no panel background or shadow.
Dock groups, drawers and standalone workspace chrome own their surfaces. GTK
assigns `dock-panel` only to those containers. Configuration expansion makes
the root transparent; its preview and configuration columns own opaque fills
with one combined shadow.
Histogram views receive the panel inset where monitors are composed; the same
view embedded in Properties adds no second inset.
`LayerDrawerSizing::pages` sets both page heights from native measurements.
The native sizing query includes both placements. Android, Apple and Windows
cache those endpoints and call stateless `drawer_frame` on their UI clocks;
container, column and bridge geometry advance without a render-owner query
per frame. Opener, workspace, viewport and content measurements invalidate the cache.
Windows resolves drawer widths, batched toolbar geometry and page endpoints
from `LayoutResolveInput::drawer_geometry` before presenting the measured bodies.
Layout callbacks invalidate measurements; Rendering callbacks restore list anchors
and advance the open animation after layout.
`LayerState::swipe_actions` publishes ordered, localized, enabled actions using
document validation. Hosts forward those actions and retain native measurement,
scroll positions, contact capture and animation timing.

## Docking

`UiState.workspace` is the authoritative `WorkspaceState`: format version,
complete dock layout, and Zen mode. Save that value using Serde; restore it via
`UiAction::RestoreWorkspace { workspace }`. Validation rejects unsupported
versions, missing/duplicate panels, invalid selections/ratios/extents, duplicate
node IDs, and invalid ID allocation state before any live state changes. A
restore does not edit the document or move the current camera. Named workspaces
use `WorkspaceCapture`, `PreparedWorkspace` and `layer-workspace` on every host
to keep separate tool settings, layout history and the starting layout; see the
[workspace manager](default-workspaces.md#workspace-manager). Do not implement
named switching with `RestoreWorkspace`, which clears history. Hosts provide
asynchronous storage transport rather than reconstructing layout decisions from
widgets. Routine workspace operations pause input without disabling or
restyling the editor.

All hosts present the shared reusable-toolbar library, including its pages,
choices, descriptions and name prompts. Required-name prompts use shared
`workspace_name_validation` before enabling submission; the storage operation
validates the accepted name again. Native asynchronous queries retain their
prompt identity so an old validation reply cannot enable a newer draft.

`UiSession::layout` supplies the standard workspace rectangles through
`DockLayout::resolved`. Shared layout operations use that default-chrome helper;
hosts with measured chrome use `DockLayout::workspace`.
`UiSession::drop_hint` validates a proposed target with the same transactional
move used on drop; hosts do not clone and probe the layout themselves.

Panel sizing uses transient keyed measurements in `UiSession` and
`LayoutResolveInput`. Resolved layouts publish `measurement_requests`: separate
Rust-issued identities for a panel's tab and body, the requested logical width,
and whether matching measurements exist. Tab widths are independent of body
widths. Body facts are selected by content identity, requested width
and host metric context; the report also records the width actually measured.
Changing workspaces preserves this bounded cache without copying outgoing
unkeyed heights into the destination. Neither keys nor measurements are saved
in artwork or authored workspace JSON.

Hosts send `MeasurePanels { measurements: PanelMeasurementBatch }`. Each batch
contains the current `PanelMetricContext` and reports that retain the request
and context from the measurement itself. Capture these before storing an
inactive body's size or queuing a callback. A missing tab or body fact is `None`;
it does not erase another valid fact. A composed drawer or tool browser must not
submit its alternate content under a canonical workspace body's identity.
Advance the host's context generation when
raster scale, text metrics, native style or loaded fonts change, and keep that
generation monotonic across view recreation within a session. Old-context and
old-content reports cannot replace current geometry. Native premeasurement uses
`LayoutResolveInput::with_panel_measurements` with the same validation; set the
current context before resolving the first destination layout.

When a panel gains a control, label, row or other size-dependent presentation,
update its shared identity projection in
[`measurement_shape.rs`](../../crates/layer-ui/src/measurement_shape.rs) and test
invalidation. Keep values that only repaint fixed-size controls out of that
projection, such as scope samples and capture timestamps. Content supplied only
by a native provider requires fresh verification before reveal; retained sizes
are hints until that particular width and split mode have been measured in the
current publication. An atomic workspace reveal requires measuring missing
destination bodies, including inactive tabs, at their requested widths first.

A `DockLayout` is an ordered list of `DockBand`s, outermost first. Each band
consumes a strip at an `Edge`: top, bottom, left, or right. Earlier bands own
shared corners. Multiple bands on the same edge form adjacent rows or columns.
The canvas covers the entire window, underneath native chrome. The unoccupied
rectangle is `work_area`, used for initial/explicit Fit Canvas; it is not the GPU
viewport. Open drawings appear as drawing tabs in the [title bar](window-bar.md).

A band contains `DockNode::Split` or `DockNode::Tabs`. Splits specify an axis
and fraction. Tab groups contain semantic `Panel` IDs and an active panel.
Stable node IDs target resize, tab selection, and moves. Moving the last panel
out of a group prunes empty groups and collapses redundant splits.

The canvas-status HUD stays above any bottom dock. The shipped arrangements are
in [`layout_presets.rs`](../../crates/layer-ui/src/layout_presets.rs); see
[default workspaces](default-workspaces.md). `DockTarget` supports a new edge band,
a tab insertion slot (including same-group reorder), or a split beside a group.
Moves validate before replacing the layout.

Frontends can translate the topology to native containers or use
`layout.workspace(width, height, top, status_height)` to obtain logical-unit panel,
HUD and divider rectangles. `resolve` is the inner
dock allocator. This shares docking rules without prescribing widget types.
Both frontends dispatch `DragDivider` phases in full-window logical coordinates
and `NudgeDivider` directions for keyboard resizing. Rust owns drag lifecycle,
the 12px nudge step, inset conversion, direction, handle thickness, ratios and limits.
Allocation and resizing use the same measured header origin and full-window
width for responsive sizing. An accepted workspace edit finishes any active
resize as its own history entry before recording the edit; late resize events
are inert. GTK retires detached resize sources before admitting another contact.
Resize gestures preserve the pointer-to-divider grab offset in stable window
coordinates, never accumulating offsets from a moving handle. GTK uses the
[surface-relative event position](https://docs.gtk.org/gdk4/method.Event.get_position.html).
Native hosts retain scroll position, focus, hover, and other
ephemeral widget state. Canvas input uses physical viewport pixels separately
from these logical layout units.

`UiSession::blank` opens on the startup sheet ([New drawing](new-drawing.md)). `set_viewport(logical, physical)`
accepts host measurements and derives physical fitting bounds from the shared
layout. It fits the first measured viewport once; later dock changes update
fitting bounds without moving the camera or issuing GPU work. Explicit Fit Canvas
uses the latest bounds. Hosts do not set work areas or decide initial fitting.
Resolved groups include tab visibility and tool-tile rectangles; DOM does not
recalculate content height or tool count. Native tiles use the same allocator
with their measured widget size.

`PanelKind::Tiles` uses the shared `tile_layout`: 36-unit square controls,
`TileStyle::gap()` between tiles and lanes (two units, four for Large and
Large Labeled tiles) and no outer padding for standalone ribbons, one row on top/bottom
or one column on a side by default. Inside a tabbed panel, Tools retains a 4px
content inset around the tile grid; it still has no duplicate inner grip.
Brush-list entries use the same 2px gap as ribbon tiles.
Cross-axis resizing adds rows/columns. A standalone ribbon also wraps and grows
automatically when its available length is too small, preserving tool order and
space for the grip. Growth is derived during layout, so more space lets it unwrap
back to the user's saved thickness without resize feedback. Tabbed Tools do not
auto-expand their group. Standalone ribbons have a centered grip
at the trailing edge (right horizontally, bottom vertically), with the same
20×24px footprint and dot inset as the tab-bar grip (transposed vertically).
Tabbed ribbons have no additional grip. Color and opacity
tiles open native/DOM popovers rather than embedding wide controls in the ribbon.

Both hosts render one `DropHint` from shared hit testing, supplying measured tab
rectangles. Headers prioritize tab insertion slots. The upper 20% of a body with
visible tabs extends the tab-strip target, the lower 20% splits below, and 18px
side strips split left/right; the rest of the body adds the incoming tabs to the
group (see [stacked columns](stacked-columns.md)). Tab insertions show a vertical
line in the tab row, clamped before the fixed trailing grip if tabs overflow.
Tab labels scroll horizontally without moving the grip.
Tabs have the same 36px button height as tool tiles, in a shared 36px tab bar
with no outer padding. The active surface joins directly into its panel.
Tab display is a group-owned choice in the group context menu: icons with only
the active name (default), names only, or icons only. Rust supplies resolved
icon/name visibility to each host; individual tabs have no style override.
Whole-group moves preserve style, merges use the destination's style, and a
new group split off from an existing group starts with the default.
UI text uses the Rust `UI_TEXT_PT` base size (11 pt), including panel text,
tabs, headings, menus and zoom/rotation status labels. GTK and web settings
row descriptions follow Adwaita's smaller subtitle role (5/6 of the base size,
about 9.17 pt); Android settings retain their native 16 sp/14 sp hierarchy.
Text-bearing inputs/buttons and inline +/− symbols use font-relative sizes.
There is no font-size setting. Checkboxes stay 16px, sliders retain
their dimensions, and toolbar icons remain 16px in 36px tiles; brush-size sample
cells keep their preview space. Both hosts keep resize handles transparent during hover/drag.
Native divider widgets are reconciled by split identity, not only panel identity,
and drag start uses the gesture's widget coordinates mapped into the dock surface.
Horizontal splits preserve the source and destination widths, growing the dock
into available canvas space; vertical splits share the existing height equally.
Moves carry the logical viewport just like resize actions, so preview validation
and the final move use identical geometry. A horizontal move is rejected atomically
if it would squeeze the panels to fit. No extra drop-time layout model is stored.
All tab bars retain an upper-right grip that moves the entire tab group as one
unit, preserving order and the active tab. Individual tabs remain separately
draggable/reorderable. A tabbed tool ribbon has no duplicate internal grip. There
are no panel move menus.

## Window chrome and Zen mode

GTK uses an `AdwHeaderBar` with native window controls and drag region; the dock
widget allocates the full-window GPU picture, transparent header, HUD and panels.
On Wayland, an app-owned Vulkan subsurface presents the full viewport below
transparent GTK chrome. The viewport shader rounds the outer corners; no crop
or theme-colored strips hide any canvas area. GTK's window content always spans
the whole surface, with a transparent node where nothing is drawn. With
color management on, GTK 4.22's GPU renderers convert each damaged rectangle
separately and skip any that lie outside the root node, so a panel that moved
away over bare canvas stayed on screen. DOM elements do the equivalent
in the browser, without fake operating-system window buttons. No custom chrome
compositor or full-window drag gesture is required. The optional
[panel transparency](panel-transparency.md) blurs behind panels inside the
canvas renderer rather than in a compositor.

The title bar's contents are a workspace arrangement; see [title bar](window-bar.md).
Header controls are [squircle](squircle-corners.md) tiles; only the native close
control is circular, using libadwaita's 16px icon plus 4px padding within the
larger native click target. Workspace gaps are 6px at the window sides and
bottom, between panels and between header controls. Panel visibility and reset
live in the Window menu.

Colors come from the [theme palette](theme-colors.md). The active tab matches
its content, with rounded top corners and concave lower shoulders joining the
two surfaces. The tab strip leaves a hole under the active tab and its
shoulders, which take the panel's fill, so the tab is the same surface as its
panel at every [transparency](panel-transparency.md) level. An inactive tab has a
subtle hover fill with rounded lower corners behind the active tab's shoulders.
Headers combine their strip and selected-tab fills in one isolated background.
Antialiased coverage at their shared contour adds once before that background
is composited over the canvas, preserving both opaque and translucent fills.
Theme and transparency changes refresh cached tab fills in docks and drawers.
Primary contact selects the tab immediately, before release or drag pickup.
Tab labels and unused header space use the regular pointer; grips retain their
drag cursor. Selecting the current tab leaves its configuration state unchanged.
Keyboard focus retains its native indication. Panels have soft
shadows rather than persistent outlines.
The header and HUD containers are transparent. Their controls sit on chips
filled with the canvas surround color, translucent or opaque according to
[panel transparency](panel-transparency.md), so they disappear over the
surround but stay readable over artwork. Web hides keyboard focus rings on the
pen-first workspace chrome, and GTK hides them in Preferences; where they show
(native GTK controls, the Web workspace manager and title-bar editor, drawing
tabs) they use the accent.

Appearance defaults to System. `Settings.theme = None` (`null` in JSON) follows
the host; Light/Dark are explicit overrides. `SystemThemeChanged` updates the
shared resolved `UiState.theme`, used for widgets, previews and GPU surround.
GTK observes the default `AdwStyleManager` and applies overrides to the display
manager; web observes `prefers-color-scheme`. OS changes never overwrite a user
override. Returning to System uses the latest OS value.

`ZenMode` toggles shared `UiState.workspace.zen_mode`. Zen has a single
presentation: it hides the header and docked chrome, keeps floating panels,
[tool bubbles](tool-bubbles.md) and toolbars marked **Show in Zen mode**, and
never projects alternate toolbar strips or changes the saved layout. The standalone Capy button or Tab
exits Zen; Tab toggles Zen outside settings and native text editors. Escape exits
Zen after active edits, menus and dialogs have been cancelled or closed. After
cancelling something, release Escape before pressing it again to leave Zen.
Explicit settings dialogs remain usable.

Preferences → Appearance → Zen mode has **Show Capy in Zen mode** (on by
default) and **Button icon**. Moving or touching a screen edge keeps controls
hidden, but a one-finger swipe that starts within 24 px of an edge and moves
40 px inward peeks the toolbars docked on that edge until the next contact
elsewhere. Pen and mouse never peek. `UiSession::zen_keeps(group)` tells hosts
which hidden groups to keep. Floating panels remain available, and their drags
can merge with other floating panels without exposing hidden docks.

Zen immediately excludes hidden chrome from pointer hit testing.
Hosts retain outgoing chrome for its 180 ms fade and honor their platform's
reduced-motion setting; the standalone Capy button switches immediately.
Expanded configuration and drawers also skip their native
frame loops when animation is disabled. Open menus, settings and native
title-bar grabs keep controls available. The next canvas contact releases an
interaction pin without also painting.
Escape closes a visible interaction owner before leaving Zen. An open column
hidden by Zen keeps its arrangement and cannot consume Escape; peeked columns
and columns containing a toolbar kept in Zen retain their normal close priority.

Zen’s visibility and interaction pins live in Rust (`Settings.zen_show_capy`
and `InputReply`’s `chrome_hidden` and `keep_zen_button` flags). Hosts send
`UiInput::Chrome`: `Refresh` and `Motion` update native facts; `Contact` carries
the position for drawer dismissal. `ChromeFacts` reports native grabs, panel drags
and popups. Rust finalizes both visibility flags
after handling every input. Hosts apply visibility and input ownership; Android
publishes flag changes even without a document revision.

The standalone Capy button uses the title bar’s current Small, Medium or Large
size even when Capy was removed from the title bar. Its button dimensions and
icon match the normal header button: shared `HeaderSize::tile` and
`HeaderSize::capy_icon` supply 36/31, 48/41 and 60/52 logical pixels. Hosts read
these metrics from `HeaderSizeView`; Zen never substitutes a fixed button or
icon size. `HeaderGeometry::size` keeps accepted bounds and size metrics together.

`HeaderGeometry::zen_button` keeps Capy's complete header rectangle, including
customized placement, document-tab expansion and native-control insets. Absent
or overflowed Capy uses the leading inset and 6/8/10px padding. Hosts retain the
measurement owner and one interactive control through normal and Zen mode,
preserving hover, press, focus and glass identity. `HeaderGeometry::capy_item`
identifies its normal slot; absent, overflowed or editable slots have none. Windows excludes the Zen rectangle from native
caption dragging so mouse, touch and pen contacts reach the button.
Android and Apple keep their measurement owners alive while removing hidden
controls and their input and glass registrations. Native geometry queries resolve
the model, editing state and document count supplied with their measurements;
drag pickup rejects a measured model that differs from the current shared model.
Normal-header holds begin before Contact and retire after native activation;
queued retirement must match the original press, including reused pointer IDs.
Capy uses neutral hover/press feedback, while menus retain
the command's checked Zen state. Capy paints its own surface and stays outside
joined header bars, so its background never fades with neighboring chrome.

**Button icon** offers Looking up (default), Facing forward, Bathing and
Sleeping. Rust stores `Settings.zen_icon` and supplies each command's icon. The
generic `ChoicePresentation::ImageTiles { columns: 4 }` uses the same choice
validation, persistence and Reset to Default as dropdowns. Hosts render four
selectable 64px tiles with centered 48px previews and accessible labels. There
is no import control. Right-clicking or touch-holding the Capy button opens
**Change icon…**, generated from the same preference row, which reveals the
row through `PreferenceAction::Reveal { id }`. All four theme-tinted SVGs live
in the shared icon bank; app, launcher and PWA icons derive from Looking up at
build time.

Fading does not change allocation, camera or viewport. Zen is the sole global
controls-visibility toggle; the Window menu still manages individual panels.
Dock moves, resizes, hiding and Zen do not request brush/render frames. Window
resize still resizes the full GPU viewport; Fit Canvas explicitly uses updated
work-area bounds.

## Actions and observation

Drag pickup follows the [drag and reorder convention](drag-and-reorder.md).

`dispatch(UiAction)` returns `Result<UiChange, String>`. `UiChange` contains a
revision, changed-region bits, and `canvas_wake`. The host updates only affected
views and schedules a display callback when needed. `state()` is read-only;
it exposes layout, brush controls, layers, document tabs, command availability,
settings/view state, Zen mode, and camera. Foreign bridges return owned snapshots only when
UI state changes; the host redraws the affected regions.

`CommandId` is the common identity for buttons, menus, keyboard shortcuts, and
accessibility actions. Shared command state supplies labels, availability, and
selection and formatted shortcut hints. Typed actions carry values for brush size/opacity/color, layer
identity/properties/order, panel moves, split sizes, themes, and settings.
There is no stringly typed event bus or generic patch language.

Retained controls use `UiState.commands`. Its enabled states stay at their
pre-contact values while canvas input is pending or active, so drawing does not
dim the toolbar on every stroke. Previously unavailable commands remain dim;
release or cancellation refreshes availability. Selection, icons, labels and
brush/color feedback continue updating during the contact. Unsupported platform
commands and a closed document still disable immediately. `UiSession::command`
and dispatch use live availability, including the temporary canvas lock, so a
shortcut or second contact cannot execute an unsafe action during a stroke.
Menus requested during a stroke also use live availability. The retained Layers
footer follows the published commands and refreshes with them, including when a
temporary workspace lock ends without a document edit.

Each command also publishes `disabled_reason`: the text
`command_disabled_reason` returns, or null while the command is enabled. The key
is always present, so a retained-model diff changes one field rather than
replacing the command object. The reason stays steady with `enabled` during a
contact. Hosts show it as the tooltip, hover text or tap response of a disabled
control. Tool rows (`ToolSetItem`), toolbar tiles (`TileView`) and title-bar
items (`HeaderItemView`) carry the same `disabled_reason` while they are
disabled, so a dimmed tool says why in a few words. A tool row can also be
`enabled` with a `disabled_reason`: it cannot act on the edited layer but stays
choosable, and hosts draw it dimmed. On a vector layer a brush that cannot draw
paths says "Doesn’t work on vector layers", and a pen that one of its
settings stops names that setting; choosing either shows the refusal notice at
once. Every host shows the reason as the tooltip or hover text and dims a
choosable row's contents as it dims a disabled row, keeping its selection
highlight; hosts that explain disabled controls on tap or hold do so here too.

Canvas gestures that would change nothing say why through `UiState.notice`,
published under `regions::HOST`:
- Move on a locked layer, and Move over a selection on a group,
  an effect layer or a layer with no pixels;
- Fill, Gradient, Figure, Lasso Fill and Enclose and Fill with no paint content to act on;
- brushes with no paint target, and erasing under alpha lock;
- a layer-mask stroke that paints dry coverage instead of the brush's wet or
  blending behavior, once per mask-editing session;
- Wand and Fill sampling reference layers when none is marked. The notice offers
  **Use *layer* as Reference**, which marks the nearest visible paint or photo
  layer below the active layer;
- choosing a brush or tool that cannot act on the edited vector or image layer,
  before any stroke ([vector layers](vector-layers.md#drawing));
- Fill outside a vector layer's paths, once the fill has landed on the new layer
  below; the notice offers **Back to *layer***.

The core writes the text and the action's label, and keeps the action. Every
notice has a new `id`, so a repeated refusal shows again.
`UiAction::Notice { id, accept }` runs the action or dismisses the notice; a
replaced or cleared id is rejected. An action that no longer applies, because
its layer changed or was locked, raises a new notice that says why. The core
clears the notice at the next canvas contact that raises nothing new, and when
another document becomes active. Hosts show each id once, in a bubble named by
`Notice::name` that does not take focus when it appears; Tab reaches its actions.
Publishing the same id refreshes its localized text and actions without
restarting its original deadline or showing a notice that already expired.
`ResolvedLayout::notice` places it centred at the bottom of the canvas area and no
wider than that area, or at the window's bottom when the canvas area is too small
to hold it; a canvas action bar reaching below `bar_floor` lifts it above the bar.
The anchor follows the bar's current bounds when it returns after a contact.
It is at most 960 pixels wide. Its actions stay beside the text while the text
keeps `min_text_width`, which equals `max_width`, so they share a row only with
text on one line; otherwise they take their own row under it and the text never
wraps beside them. Every host draws the bubble like its canvas action bar: the
same surface, a Small tile tall in one row, a caption 12 pixels in, and its
actions as the bar's worded controls, right-aligned under the text when stacked.
Hosts measure the actions before the bubble first shows. The
first action is the core's suggested way forward (`accent`), drawn as the bar's
accent button, like a mode bar's exit; the others are flat. Hosts hide it after about 4 s (answering
`accept: false`) or at the next canvas contact. Notices overlay the canvas and
never resize its viewport or GPU surface. `host_error` is kept for file and
renderer errors.

Brush previews are transparent lossy WebP files from the actual GPU presets, with
destination paint seeded for blender/eraser/liquify examples. Every host ships
the same dark/light files and decodes them off the UI thread.
`gpu-bench --brush-previews` regenerates them; no live brush jobs or canvas
readbacks run when opening the picker. Built-in filter rows likewise show bundled
WebP files; see [filter previews](../internals/rendering.md).

Brush picker labels, preset identities, and size choices have one Rust source.
Picker colors are display-encoded sRGB; Rust converts them to linear brush
color. Tool selection and brush parameters apply to subsequent strokes. Layer
selection is navigation: it consumes no undo step and preserves redo history.
Document mutations and camera gestures are rejected while pen input is pending
or a stroke is active, keeping command ordering explicit.

### Drawing and navigation

`input(UiInput)` handles normalized keys, canvas contacts, chrome events and
focus loss. Its small `InputReply` reports changed regions, whether an event
is handled, whether to enqueue paint, cursor/visibility state and popup/cancel
requests. Rust owns modifier interpretation, repeat suppression for toggles,
editing/modal/popup guards, temporary navigation ownership, competing pointer exclusion,
and cancellation. Modified keys never silently become unmodified shortcuts.
Selection modifier presses, releases and focus loss publish tool controls and
command selections together, including the configured mode restored on release.
Input replies retain these changes when the same key also records a shortcut,
updates navigation or dispatches an action.
Releasing the bound pan key or changing input focus clears the held-key state;
releasing it during a pan does not change that contact into paint.

Pen history uses `pen(PenEvent)` only after the pointer reply selects paint.
Records retain native
timestamps, pressure, tilt, twist, prediction flags, and camera revision. The
bounded queue returns a record on overflow; adapters must preserve and retry
it after draining a frame. Never drop an up/cancel event silently. Per-frame
command refresh updates existing entries without allocating new command lists.
It runs when editor state, pending edits or command availability change; idle
canvas frames and bubble motion retain it. An interrupted frame leaves a refresh
pending so failed asynchronous edits restore their available commands.
Ordinary pen motion does not produce UI snapshots.

The host calls `frame(now_ns, presentation_ns)` at display time and presents the
renderer-owned composite through the GPU viewport presenter. Only explicit
exports and visual tests read pixels back. Camera movement uses the same
forward/inverse transform for drawing and display. Shared `TouchGesture`
interprets two fingers as anchored pan/zoom/rotation; one finger does not paint,
draw lines or erase. One finger does drag what Move or Edit Points would take
under a pen: images, paths, transform handles and anchors, and a tap on a
segment adds an anchor.
Mouse and stylus event collectors remain native.
Wheel input follows Canvas → Mouse wheel (Pan by default, or Zoom).
Shift-wheel pans horizontally; Ctrl-wheel always zooms around the cursor. Hosts normalize native wheel units and Rust applies the camera gesture.
These gestures also work while a mouse navigation button is held, including
middle-button TrackPoint scrolling. A wheel gesture keeps that contact alive
until release or cancellation. Painting, direct touch gestures, source-disc
drags and queued paint input keep the camera fixed. Ctrl takes precedence over
Shift, and wheel zoom respects the view's zoom lock. Web captures wheel input
over the canvas before browser zoom; wheel input over panels stays with them.
Hand and Rotate View share a flyout; Zoom has its own button beside it. Tap
H, Z or R to select one; hold the key while using it to return to the previous
tool on release.
Temporary navigation does not switch the painting tool or commit an unfinished
polygon selection. Space pans, Ctrl+Space zooms, Alt+Space or Ctrl+Alt+Space zooms
out, and Shift+Space rotates. Middle/right drag also pans. Once captured, a
contact retains its navigation mode through key release until up or cancellation.
Space with arrow keys pans in keyboard-sized steps; Page Up/Down while panning
moves almost one visible page. Ordinary arrow keys retain their editing behavior.
A focused button, switch, check box or list row takes Enter, and takes Space only
when the painter reached it with the keyboard; after a click, Space still pans.
`control_takes_key` decides this for every host. Hosts report whether the focus
came from the keyboard: Web from the last input before `focusin`, GTK from the
last input before the focus widget changed, and Windows from `FocusState`. A
pointer press makes the focused control count as pointer-focused again. Android
and Apple controls do not take focus from a tap or click.

Zoom Tool Settings expose Click (In/Out), Drag (Smooth/Area/Click only),
Direction (Left/right or Up/down), and Center clicked point. Defaults are In,
Smooth, Left/right, and centering off. These tool choices are application
preferences shared across drawings and workspaces, stored in `Settings.zoom_tool`.
Clicks step on release; unbound Alt reverses the chosen click direction.
Smooth drag right or up zooms in; left or down zooms out, around the press
location. A small movement threshold separates clicks from drags. Shift-drag with Zoom draws a
rectangle and fits that area on release. Rotate View turns around the work-area
centre. Area mode uses the same rectangle without Shift. Press Space during
the drag to move the rectangle; Escape cancels it. Space held for temporary
Zoom remains part of that shortcut. A line or rectangle smaller than the movement
threshold does not zoom. Click only suppresses zoom for moved contacts.
Center clicked point moves the clicked image point to the work-area centre only
on clicks; smooth drag, wheel and pinch retain their anchors. The contact captures
the tool choices at press, including temporary navigation. Canvas → Rotate with
two fingers controls touch and trackpad rotation while retaining pan and pinch.
Double-click the Hand tool button to fit, Zoom for actual pixels, or
Rotate View to reset rotation. These operations change the view, never the artwork or its Undo history.

Fit preserves rotation and mirrors and measures the rotated canvas bounds.
Reset rotation keeps the centre and mirrors; Reset view clears rotation and
mirrors and fits the drawing. View also offers Fit width, Fill view, Zoom to
selection and Previous view. Previous view swaps with the last saved camera
position. Save view and Restore saved view are searchable, bindable commands for
one position per open drawing; they do not add toolbar controls or enter artwork.

The Capy navigation defaults use [Clip Studio conventions](https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Menu_Shortcuts.htm) first,
[Photoshop alternatives](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/zoom-images.html) next and
[Krita](https://docs.krita.org/en/user_manual/getting_started/navigation.html) where neither defines a binding. Rotation buttons
step 5°; the Krita preset steps 15° and supplies 1/2/3 for actual pixels/fit/fit
width and 4/5/6 for rotate left/reset/right. Ctrl+; zooms in, with Ctrl+=, Ctrl++
and keypad Ctrl++ as alternatives. Linux window managers can own Alt+Space; Ctrl+Alt+Space and Alt-click
remain alternatives. OS-owned shortcuts cannot be captured by the app.

GTK forwards native touchpad pinch scale, rotation and translation into the
shared camera. Web handles Ctrl-wheel pinch and browser gesture events where
available. Android keeps multi-contact touchpad input as touch so it shares
finger pan/pinch/rotation; synthesized wheel input retains normal wheel behavior.
Selecting Zoom or Rotate View also allows one-finger navigation; adding a second
finger rebases to the shared two-finger gesture. Hosts expose navigation cursor
identities from `InputReply.navigation_cursor`.

Next drawing and Previous drawing use shared commands and custom bindings, with
Ctrl+Tab/Ctrl+Shift+Tab and Ctrl+Page Down/Up on native clients. Alt+Page Down/Up
works on Web, where browser tab switching keeps the Ctrl chords. The host handles
the resulting `AdjacentDrawing` request through its existing tab activation path.
Parking an idle drawing releases held keys and temporary tools, so returning to
it cannot retain a shortcut as a repeated press. Drawing-switch commands remain
available when its renderer is unavailable.

Zoom stays between 2% and 1600%. **View ▸ Actual Pixels** (Ctrl+Alt+0 or
Ctrl+1; Ctrl+1 in the Photoshop and Affinity keymaps, 1 in GIMP's) shows
one image pixel per device pixel, zooming about the work-area centre. When the
view rotation is a quarter turn, `Camera::zoom_to` also rounds the translation
to whole device pixels, so the bilinear presenter samples pixel centres and the
1:1 view is not blurred. On Web, Ctrl+1 and Ctrl+Alt+0 call `preventDefault` so
Chrome does not switch tabs. It differs from the placement bar's **Original Size
(100%)**, which returns a placed image to its own pixel size.
The footer's "N% · D°" readout is a button. It opens `UiSession::zoom_menu()`:
Zoom In, Zoom Out, Fit, Actual Pixels, then 25% to 400%, with a typed zoom field
from `NumericControl::zoom()` (percent on a logarithmic track). `SetZoom` clamps
to the camera limits, zooms about the work-area centre and rounds like Actual
Pixels. The field takes its value from the camera and refreshes only while the
menu is open.

The menu also shows a rotation slider from
`NumericControl::rotation()`. Both fields use one row with the slider and editable
value, without a visible label or step buttons. Zoom controls and percentages
precede Lock zoom; the rotation slider, Reset rotation, Snap rotation and Lock
rotation follow together. The locks and Snap rotation show checkmarks.
`SetRotation` rotates about the work-area centre without changing the artwork.
Slider and inline value drags send `SetRotationFromSlider`, which applies the
current Snap rotation preference in shared Rust. The locks belong to the camera and stop
canvas navigation gestures, including touch, wheel and continuous zoom input;
pan remains available. Explicit sliders, percentages, commands and shortcuts
remain usable while locked. The bottom row uses the same shared
`NAVIGATOR_COMMANDS` order as Navigator: zoom out/in, rotate left/right and flip
horizontal/vertical. These buttons keep the menu open and follow live command
availability.

Snap rotation is on by default and saved in application settings across drawings.
It applies to two-finger and trackpad rotation and Rotate View drags with mouse,
pen or one finger, including temporary rotation with held R or Shift+Space.
The rotation slider uses the same attraction curve for its absolute angle;
its thumb and readout follow the displayed canvas angle, including stationary holds.
The slider keeps −180° at its left end and +180° at its right end when snapping.
Turning snapping on or off leaves the current view still. Typed angles, keyboard
steps and rotation commands keep their requested values.
A shared `layer-core::RotationSnap` retains the raw
angle while holding exactly at quarter turns within 3°. Between 3° and 10°,
a continuous cubic response eases back to free rotation with matching endpoint
slopes. Pan and pinch retain their midpoint anchor. Gesture starts rebase to the
current displayed angle, finger changes discard residual movement, and release
adds no motion. The transient snapping state never enters artwork or recovery.

Hosts read numeric specifications from `UiCatalog` and menu sections and buttons
from the Web `zoom_menu` export or native `zoom_menu` query. Its `rotation_section`
marks where the rotation slider belongs, after the zoom items. Opening the menu or
choosing an item preserves canvas keyboard focus; typing borrows it:
- GTK's readout button cannot take focus, and closing its popover returns
  focus to the canvas.
- Web's `#view-info` button is out of the tab order and its menu cancels the
  presses that would move focus. Escape closes it and returns focus.
- Android opens a windowless dropdown that becomes focusable only while the
  field is being typed in.
- Windows opens a transient flyout whose readout and rows cannot take focus;
  Escape and a second press close it. `exercise-zoom-readout.ps1` covers the menu,
  both fields and locks in either theme.

The camera works in physical pixels. Web uses the fractional
`devicePixelRatio` and Android physical pixels, so 100% is 1:1 there. GTK
renders at the widget's integer `scale_factor()`, so under fractional Wayland
scaling the compositor rescales the canvas and 100% is not exactly 1:1.

### Layer relationships

The shared layer view publishes typed clipping and effect relationships, resolved
targets, connector endpoints, the contextual attachment control and each row's
right-swipe action. Hosts render these values without scanning sibling rows to
infer ownership. The [authored model](../reference/authored-model.md) owns the
composition and editing rules.

Checked rows are the targets for grouping, deletion, movement and duplication.
Clicking a checked row preserves the checked set; clicking the active row also
preserves its mask target. Clicking a thumbnail explicitly changes the painting
target. Shift selects a range of visible rows. Collapsing a folder replaces
concealed checks with the folder row. Undo and
redo restore the checked set and reveal its rows.

Layer lists use the same touch rules in full panels and simple tool bubbles.
One-finger right swipes keep the row's published action: Alpha Lock for paint
layers and Pass Through for groups. Two-finger row swipes have no action in
either panel style.

A two-finger tap on a row opens that layer's opacity editor. Holding both fingers
loads its painted content into the selection through the existing thumbnail
coverage command. Pinching two rows together merges the inclusive range, including
contained groups, in one Undo step. Locked layers, hidden range roots, saved
selections and incomplete clipping or effect dependencies retain the shared merge
refusals; a gesture cannot bypass them.

Hosts forward list identity, stable contact IDs, row hits, physical positions and
native timestamps through `UiInput::LayerTouch`. A second contact retires native
single-contact pickup, menus and swipes. Shared Rust recognizes the gesture and
uses the frame clock for holds. Cancellation, focus loss, document changes and
list removal retire contacts. `LayersView::opacity_popup` and `opacity_surface`
identify the editor's layer and originating list, so retained copies do not open
duplicate editors. Bubble movement leaves contacts over native layer controls
with their list.

The sole selected editing reference uses the lighthouse icon for paint layers
and groups, with or without a mask. Reference rows keep the lighthouse when
unselected. Multiple checked rows and checked rows outside the editing target
use checkmarks.

New Group wraps multiple checked rows from the same parent in their displayed
order, including separated rows. Deleting an expanded folder removes its wrapper
and keeps unchecked children when their appearance can be preserved. A collapsed
folder represents its contents for deletion. Deleting a clipping base or effect
owner keeps unchecked dependents and releases their attachment. Locks protect
descendants during structural edits, and disabled actions use the execution rules.

Hosts use one header button for both operations. Content uses the stable label
**Clip to Layer Below** and a checked state. Its description and tooltip name the
base as **Clip to {base}** or **Clipped to {base}**. Effects use
**Apply to {owner}** or **Apply to layers below**. The effect action
uses the vertical link symbol. Clipping or attaching an effect to a Pass Through
group is disabled until the group's blend mode changes. The description names
the group whose Pass Through setting must be turned off. The same actions appear
in Layer settings, with shared availability and checked state. Menu icons use
the shared bank's short identities.

A straight clipping rail runs through the common-base stack, ending without a
notch at the bottom of the base thumbnail. It continues past an expanded clipped group's children, but
stops at a base group's header. Effect links occupy the existing gaps between
effect thumbnails and their owner, independently of the left rail. Saved
Selection rows stay outside the contiguous effect chain. Neither
indicator adds a column, indentation or row height. The clipping rail uses the
shared `relationship` palette color, derived from a darker shade of the accent.
Effect links match the existing
content-to-mask link's neutral color and shape, oriented vertically.

Adjustment effects show their icon without a thumbnail background in the
existing hit area; content generators retain their content thumbnails. Fill
thumbnails fill their squircle, cropping the document's aspect, so they
read differently from paint drawn on a checker. Tapping a solid color fill's
thumbnail, the Paper included, selects it and opens Edit Color on its color
(`LayerState::fill_color`); Use Color sets it like the Properties color. Hiding
an owner also hides its attached effects without changing their individual
visibility settings. The shared row marks inherited hiding for a dimmed eye.
Showing the owner restores effects that were not individually hidden.
Thumbnail editing marks identify editable paint, saved selection or mask pixels;
filter, fill and folder icons do not show them. The narrow content-to-mask button
uses distinct vertical linked and broken-chain symbols. Every host uses a 10px icon and
minimum width for this control. Solo keeps individually hidden
children hidden and restores visibility through undo and redo.
Use Selection is a normal icon button with squircle corners, a transparent idle
background and the usual hover/pressed states, within its existing hit area.

Pass Through groups have a through-arrow badge inside the folder thumbnail;
expansion keeps its separate folder shape. Layer subtitles show non-default blend
modes and reduced opacity; Normal is omitted. The Layer settings menu keeps
**Pass Through** as a stable checked label and icon. Right swipe invokes the shared group-mode action
or paint alpha lock; see the [gesture rules](drag-and-reorder.md#scrolling-menus-and-cancellation).

Layer drop previews and commits use the same shared planner. A thumbnail hit can
attach an effect to an owner; a row hit chooses a gap or a group destination.
The preview reports the actual insertion target after accounting for attached
effects and clipping runs. Moving an owner carries its effects, and moving a
clipping base carries the run. A Pass Through thumbnail cannot silently become
an isolated effect owner.
Inside a clipping run, an adjustment dropped immediately above eligible content
or its attached effects joins that owner's filter chain, including the first
filter on a clipped member or its base. The preview highlights the owner's
thumbnail alongside the insertion line. Saved Selections and generators cannot
be skipped to infer an owner; hidden owners remain eligible.
Outside clipping runs, effect gaps join only between remaining effects and their
owner. Drops above the top member, below the owner or base, between independent
chains, or at the beginning of a group stay outside that relationship. This also
applies to imported images. Dropping back at the same parent and position preserves
attachments and creates no undo step. Moves that would split an existing clipping
run with a standalone adjustment are refused. Reordering, transferring or releasing
attached filters protects both their old and new owners' locks, including locked
ancestor groups; alpha lock does not prevent filter attachment.
Dragging a checked row moves the checked roots together in display order, with
the insertion line anchored outside the entire moving set. Raise
and Lower use sibling positions for every layer kind. Web edge scrolling continues
the same drag without changing its target semantics.
Dropping a saved Selection inside an effect chain previews and inserts it above
the top effect. Attaching across saved Selections moves them above the resulting
chain atomically. A drop below the owner remains a separate valid position.
Creating a standalone adjustment also places it above the complete clipping and
effect stack. In the filter drawer, adding an adjustment to a clipped paint layer
or isolated group instead places it nearest that owner, before its existing
effects. Its preview uses the owner's masked content at that insertion point.

### Layer blend menu

The Layers header's blend control shows the active layer's `blend_label` and
opens `UiSession::layer_blend_menu(id)`: the modes of `LayerBlend::MENU` in its
six groups, as check items that dispatch `LayerAction::Blend { id, value }` with
the mode's code. The layer menu's **Blend Mode** submenu, and so **Layer ▸ Blend
Mode**, holds the same items; command search finds each mode there. Float
documents leave out the modes defined only on [0, 1] (see
[blend modes](../internals/rendering.md#blend-modes)), except the layer's current
mode. Only groups offer **Pass Through**, first in the group with Normal, as in
Photoshop; another layer refuses it with a reason. Choosing the current mode adds
no undo step. GTK fills a `MenuButton` popover when it opens, Web reads the
`layer_blend_menu` export, Android, macOS and iPadOS the `layer_blend_menu`
query and Windows the layer menu query with `blend`.
`UiCatalog.layer_blends` stays a flat list in code order, so a host that shows a
plain list sends its index as the code; Pass Through has the last code, and the
Properties panel's list leaves it out for layers other than groups.

## Settings and native flows

`OpenSettings { page }` (or the Preferences/Shortcuts/About commands) opens the
core-owned settings view. `preferences()` describes
pages, groups and typed choice/number/switch/information rows, current values,
availability, dependencies, search results, errors and shortcut recording state.
Each accepted `PreferenceAction` edit validates, applies and requests persistence
immediately.
`CloseSettings` only closes the view; Done, Back and dismissal never revert values.
Invalid edits preserve the last accepted value and report an inline error.
Hosts render this small settings-specific model,
not an arbitrary widget tree or a second business-logic layer.

Five pages cover Appearance/Zen, Canvas/navigation, Pen & Input, Keyboard
Shortcuts and About. `Platform` selects native-window commands, predicted-sample
controls and renderer information. Core validation checks field availability,
dependencies, value ranges, shortcut collisions and persisted versions.
GTK uses libadwaita 1.9's `AdwViewSwitcherSidebar`/`AdwNavigationSplitView` in
`AdwDialog`; web uses native DOM controls in a matching adaptive modal.
Android uses a full-screen, top-sliding Settings overlay without a global header.
A full-height sidebar starts with a persistent search field; the main pane has
its own title and filled Done button. Setting rows render the core name and
description on the left and the value/control on the right. Numeric rows share
one slider/text renderer; `Slide` delegates step snapping to Rust. These panes
adapt to list/page navigation on narrow screens.
Printable keys outside editable controls start preferences search through the
shared input router; the view's transient `search_focus` revision asks each host
to reveal and focus its search field. Native text editing and shortcut recording
keep ownership of their input. GTK/web dialogs target 1000 × 744 logical pixels
and shrink to fit smaller windows. They have no footer or Done button: × or
Escape closes them, following libadwaita, without custom outside-click
dismissal. Errors appear below the header only while present.
Android renders simple choices as native anchored dropdowns, with options,
icons and the selected value supplied by Rust. Selection submits `Edit` without
navigating. `ImageTiles` renders centered selectable previews instead; both
presentations share the same choice editing/reset actions. Detailed editors that use a modal on GTK/web, such as the shared
shortcut editor, slide in from the right with a pane-local Back arrow. Numeric
input, shortcut recording, conflicts and validation errors remain inline;
settings never open another Android dialog.

The single keymap routes commands, brush/size presets and registered parameterized
`UiAction`s. Hosts do not resolve shortcuts. Explicit conflict replacement only
removes the colliding alternative; Clear, Reset and Reset All apply atomically.
Recording is provisional until confirmed; navigation and recording alone never save.
Escape cancels recording; Tab can be reassigned like other shortcuts, but retains
native focus navigation inside settings and text editors. Platform-global
shortcuts are not inhibited. Browser-reserved bindings are rejected by the core.

Shortcut presentation also belongs to Rust. `Settings::action_shortcut_localized` resolves
typed action identity (including registered parameterized actions), never a
translated label. `action_tooltip_localized` formats the label and shortcut as a complete Fluent
message, or returns the label
when unbound. Command and toolbar views carry their complete tooltip; other
action buttons request it on hover, so remapping does not leave stale hints.
GTK/Web/Android/Windows display these strings without joining keys themselves.

All application context-menu families—workspace, groups/panels, ribbons/tiles,
Zen and layer/mask menus—pass through the same recursive shortcut annotation.
Toolbar configuration options use it too. The core owns their labels, selected
and enabled state, actions, sections and hints. Preference-reset hints retain
the default value alongside any assigned shortcut. Entries without bindings do
not invent defaults. GTK retains its native check indicator when a hint is
present. Web and Android render the same menu models. Native text editing is
separate from canvas commands: Cut/Copy/Paste/Select All copy and conventional
hints come from `text_edit_menu`, while the host text editor owns execution and
selection. OS-provided text-selection menus remain platform-owned. There is no translation
infrastructure yet.

Android tooltips use Material's [TooltipBox](https://developer.android.com/develop/ui/compose/components/tooltip)
with explicit hover activation. They do not consume touch holds reserved for
dragging or context menus. Their layout wrapper preserves parent sizing and
grid weights. The bubble follows Adwaita styling: white editor-sized text on a
dark background, a subtle border and compact rounded padding. It centers 4dp
below its tile, flips above when needed and slides horizontally to stay visible.
No shortcut lookup runs on the stroke-input path.

GTK keeps native mouse tooltips and picks the pen's hovered widget for its
Adwaita-styled fallback; GTK 4.22's native timeout queries the seat mouse instead.
Web uses one inert tooltip for mouse/pen hover, with the same compact styling,
editor text size and 4px below-center placement (flip/slide at screen edges).
Both wait 500ms, dismiss on contact/exit/cancellation, and preserve shared shortcut
text. Windows keeps native WinUI tooltips for mouse and pen hover and closes any
tooltip that opens while the latest contact is touch
(`apps/layer-windows/scripts/exercise-tooltips.ps1`). Tooltip controllers never
consume touch holds or request model refreshes.
`tools/performance/workspace-motion.sh gtk --tooltips` (or `web`) checks them.

Applied settings emit a durable `HostRequest::SaveSettings`; GTK writes atomically
on GIO's I/O pool and web uses localStorage. Completion/error returns through
`CompleteRequest`. `RestoreSettings` validates without emitting another save.
New Window uses the same request/acknowledgement boundary. Documents carry only
an opaque host URI and name (`DocumentLocation`); native handles, browser Files
and permissions stay in the host. The host relays applied settings to other open
windows using the same validated restore action; GTK serializes file writes so
an older pending snapshot cannot overwrite a newer one.

`cursor_input` accepts the latest optional pen/hover
record independently of the paint queue; `canvas_cursor` returns vector paths
in logical display coordinates. Hosts display those paths without brush math or
canvas readback. Native GPU segments and web SVG use the same thin contrasting dashes.

## Binding portability

The Wasm wrapper uses typed scalar input and owned JavaScript records, not JSON
per pointer sample. `u64` identities/revisions cross as JavaScript `bigint`.
Actions use explicit tagged variants; native GTK calls those Rust variants
directly. Android, Apple and Windows reach the same finite, synchronous facade
through [`layer-host`](../../crates/layer-host/src/lib.rs) and their native
bridges. No frontend duplicates validation, command availability, or document
behavior. Shared tests cover state and input semantics; frontend tests and
screenshots must additionally prove widget wiring, docking, presentation, and
drawing in both themes.

In the GTK host, `preferences.rs` renders the preferences view and services its
storage/window requests; `workspace.rs` maps other state to
native controls and allocates them from the shared layout; `input.rs` collects
GTK stylus/history/touch records; `canvas.rs` advances the shared session and
paces frame preparation. `render_thread.rs` owns the bounded frame handoff,
Vulkan brush engine, viewport/cursor presenter and swapchain. `wayland.rs` owns
the child surface/protocol lifetime, not GTK's connection or parent. No host
module duplicates brush rules or manipulates canvas pixels.
`previews.rs` embeds the shared swatches. `lib.rs` owns application lifetime. The canvas stays parented while dock
wrappers change. Controls and divider handles are reused. A timer handles
input/frame preparation and native control refresh, then stops when idle.
Its period and phase come from the child surface's Wayland presentation feedback
(sampled once per second), with a 120Hz fallback when unavailable. Prepare half a
refresh interval before presentation, independently of GTK's scene-update rate.
At most two paint frames are in flight; when busy, pen records remain queued.
The GPU worker sleeps when idle. No GPU wait, image import or canvas pixel copy
runs on GTK's drawing/input hot path. The shared cursor geometry is drawn in the
same GPU viewport pass; the browser uses its equivalent SVG paths.
The web has the equivalent native DOM host in `app.js` and a small Wasm bridge
in `src/lib.rs`. Its GPU help (`gpu.js`) picks guidance by platform hint only
and never blocks a browser. There is no cross-toolkit widget framework or second action
dispatcher.

Navigator camera/drag geometry and preview scheduling live in the shared UI core.
The GPU renderer downsamples the existing document composition into a persistent
256px-long-side target and asynchronous staging buffer. One producer supplies all
docked/drawer views, at most 15 times per second while visible and changed;
view-only pan/zoom/rotation/reflection reuses the image. GTK draws the work-area
outline with small scene rectangles, not a newly rasterized bitmap per frame.
While live painting/animation updates thumbnails, GTK retains its update clock
without forcing redraws. This avoids restarting its idle clock for each 15Hz image;
the clock is released after painting stops or Navigator becomes hidden.

Hand and Eyedropper also share their input policy in Rust. Hand routes primary
mouse/pen contact and one-finger touch through the existing camera gesture path;
it never submits a paint stroke. Eyedropper offers visible-composition and raw
editing-layer samples, transforming coordinates through the camera and layer
offsets. It keeps one asynchronous request in flight and coalesces subsequent
points. Tool/document/manual-color changes invalidate late replies. The renderer
copies one existing texture pixel to a reusable four-byte staging buffer, with
no shader, composition rebuild, GPU wait or full-image readback. The UI converts
straight linear RGB to the active paint-color slot; transparent samples leave it
unchanged. Ordinary drawing does not request samples. GTK forwards the query to
its GPU worker; web and Android use the same renderer interface.

## Primary references

- [Android UI-layer guidance](https://developer.android.com/topic/architecture/ui-layer)
  and [UI events](https://developer.android.com/topic/architecture/views/ui-layer/events-views)
  describe shared state holders and durable presentation state.
- [GTK actions](https://developer.gnome.org/documentation/tutorials/actions.html)
  provide the native command mapping.
- [Pointer Events](https://www.w3.org/TR/pointerevents3/) specifies browser
  pointer identity, pressure, history, capture, and cancellation.
- [wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/) provides the web
  bridge without imposing OS threads or a native ABI.

## Localization context

`UiSession` and `WorkspaceController` receive an immutable, shared
`Arc<Localizer>` for the current presentation. Prepared and replacement drawings
adopt the window's current context before publication. The session stores its context privately in
`UiState`; clones share the same allocation and serialized views omit it.
Language edits prepare a new immutable context and refresh retained copy at an
input-safe boundary, preserving artwork and native editing state. Embedded Fluent catalogs and cached parameterless labels
live in `layer-ui`; see [localization](localization.md) for the message and
argument contract. Hosts receive resolved text through feature views.
