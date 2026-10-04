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
Tool action groups declare their command members, label and bar/list presentation together.
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

`UiSession::layout` supplies the standard workspace rectangles through
`DockLayout::resolved`. Shared layout operations use that default-chrome helper;
hosts with measured chrome use `DockLayout::workspace`.
`UiSession::drop_hint` validates a proposed target with the same transactional
move used on drop; hosts do not clone and probe the layout themselves.

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
Resize gestures preserve the pointer-to-divider grab offset in stable window
coordinates, never accumulating offsets from a moving handle. GTK uses the
[surface-relative event position](https://docs.gtk.org/gdk4/method.Event.get_position.html).
Native hosts retain scroll position, focus, hover, and other
ephemeral widget state. Canvas input uses physical viewport pixels separately
from these logical layout units.

`UiSession::blank` owns new-document defaults. `set_viewport(logical, physical)`
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
or theme-colored strips hide any canvas area. DOM elements do the equivalent
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
two surfaces. GTK paints only those corner joins in a non-interactive native
overlay; DOM uses CSS pseudo-elements. Native tab controls and their hit
targets are unchanged. Panels have soft shadows rather than persistent outlines.
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
presentation: it hides the header and docked chrome, keeps floating panels, and
never projects alternate toolbar strips or changes the saved layout. A minimal
persistent UI belongs in a workspace instead. The standalone Capy button or Tab
exits Zen; Tab toggles Zen outside settings and native text editors. Explicit
settings dialogs remain usable.

Preferences → Appearance → Zen mode has **Show Capy in Zen mode** (on by
default) and **Reveal panels near screen edges** (off by default). With reveal
enabled, the shared `near_chrome` rule reveals hidden controls only within
80 logical pixels (`WORKSPACE_PROXIMITY`) of an occupied window edge, not by
approaching a hidden toolbar or panel. The top always reveals the header;
left/right/bottom reveal only when a visible dock band occupies that edge. The
status HUD alone does not enable bottom-edge reveal. Once visible, the same
80px margin around panels, header and HUD keeps controls available; enabled
edge zones also retain visibility to prevent oscillation. Neither distance is a
preference.

Enabling Zen hides the chrome at once. The core suppresses hover reveal inside
a fixed 300 × 300 logical-pixel top-left guard until the pointer leaves it, so
the activating button does not reveal itself again; a fresh deliberate contact
re-enables edge reveal for touch users. Hosts animate opacity over 180 ms and
disable hit-testing while hidden. Web respects reduced-motion preferences; GTK
uses the platform animation setting. Keyboard navigation, open menus, settings
and native title-bar grabs keep controls available. Floating panel movement
reveals hidden docks only at an occupied screen edge, then holds that visibility
for the rest of the drag. Every drop returns to normal cursor proximity. An
initial contact in a hidden control's reveal zone reveals instead of painting,
and cannot also activate a newly exposed button. A captured stroke does not
reveal controls under its moving tip. Moving away or leaving the window fades
the chrome, except during a native title-bar grab. A release or subsequent
unpressed motion clears that grab latch; leave/cancel during a WM drag does not.
On touch, the last contact keeps revealed controls available after finger lift;
another contact away from controls can hide them.

Zen's hidden/visible state, last hover/contact, keyboard pin, and first-contact
consumption live in Rust (`Settings.zen_show_capy`, `zen_reveal_at_edges`, and
`InputReply`'s `chrome_hidden` and `keep_zen_button` flags), not frontend
booleans. Hosts send `UiInput::Chrome` events plus `ChromeFacts` (native grab,
panel drag, popup visibility) and only apply visibility, hit-testing and
styling. Android carries both flags in its change-detected snapshot, including
updates without a document revision. The DOM retains only its suppressed-click
pointer ID to prevent the browser's subsequent click from activating a revealed
control. These decisions belong to `DragWorkspace` and the Rust interaction
state, not frontend callbacks.

The Capy button is a sibling of the header with a same-sized spacer. Its active
background is subtle grey while the full UI is visible and neutral when only
the button remains; hover still works. Its icon is 31px (`ZEN_ICON_SIZE`).
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
Menus requested during a stroke also use live availability.

Each command also publishes `disabled_reason`: the text
`command_disabled_reason` returns, or null while the command is enabled. The key
is always present, so a retained-model diff changes one field rather than
replacing the command object. The reason stays steady with `enabled` during a
contact. Hosts show it as the tooltip, hover text or tap response of a disabled
control.

Canvas gestures that would change nothing say why through `UiState.notice`,
published under `regions::HOST`:
- Move on a locked layer, and Move over a selection on a group,
  an effect layer or a layer with no pixels;
- Fill, Gradient, Figure and Lasso Fill with no paint content to act on;
- brushes with no paint target, and erasing under alpha lock;
- a layer-mask stroke that paints dry coverage instead of the brush's wet or
  blending behavior, once per mask-editing session;
- Wand and Fill sampling reference layers when none is marked. The notice offers
  **Use *layer* as Reference**, which marks the nearest visible paint or photo
  layer below the active layer.

The core writes the text and the action's label, and keeps the action. Every
notice has a new `id`, so a repeated refusal shows again.
`UiAction::Notice { id, accept }` runs the action or dismisses the notice; a
replaced or cleared id is rejected. The core clears the notice at the next
canvas contact that raises nothing new, and when another document becomes
active. Hosts show each id once, in a bubble over the canvas that never takes
focus, and hide it after about 4 s (answering `accept: false`) or at the next
canvas contact. Notices overlay the canvas and never resize its viewport or GPU
surface. `host_error` is kept for file and renderer errors.

Brush previews are cached transparent PNGs from the actual GPU presets, with
destination paint seeded for blender/eraser/liquify examples. GTK bundles the
same dark/light assets served by the web client. `gpu-bench --brush-previews`
regenerates them; no live brush jobs or canvas readbacks run when opening the picker.

Brush picker labels, preset identities, and size choices have one Rust source.
Picker colors are display-encoded sRGB; Rust converts them to linear brush
color. Tool selection and brush parameters apply to subsequent strokes. Layer
selection is navigation: it consumes no undo step and preserves redo history.
Document mutations and camera gestures are rejected while pen input is pending
or a stroke is active, keeping command ordering explicit.

`input(UiInput)` handles normalized keys, canvas contacts, chrome events and
focus loss. Its small `InputReply` reports changed regions, whether an event
is handled, whether to enqueue paint, cursor/visibility state and popup/cancel
requests. Rust owns modifier interpretation, repeat suppression for toggles,
editing/modal/popup guards, momentary-pan ownership, competing pointer exclusion,
and cancellation. Modified keys never silently become unmodified shortcuts.
Releasing the bound pan key or changing input focus clears the held-key state;
releasing it during a pan does not change that contact into paint.

Pen history uses `pen(PenEvent)` only after the pointer reply selects paint.
Records retain native
timestamps, pressure, tilt, twist, prediction flags, and camera revision. The
bounded queue returns a record on overflow; adapters must preserve and retry
it after draining a frame. Never drop an up/cancel event silently. Per-frame
command refresh updates existing entries without allocating new command lists.
Ordinary pen motion does not produce UI snapshots.

The host calls `frame(now_ns, presentation_ns)` at display time and presents the
renderer-owned composite through the GPU viewport presenter. Only explicit
exports and visual tests read pixels back. Camera movement uses the same
forward/inverse transform for drawing and display. Shared `TouchGesture`
interprets two fingers as anchored pan/zoom/rotation; one finger does not paint.
Mouse and stylus event collectors remain native.
Wheel input pans; Shift-wheel pans horizontally; Ctrl-wheel zooms around the
cursor. Hosts normalize native wheel units and Rust applies the camera gesture.
Space + left-drag temporarily pans without pen records. Releasing Space during
the drag does not turn its remaining motion into paint. Middle/right drag also
pans; touch retains two-finger rotation. The momentary pan binding defaults to
Space and is editable in Preferences alongside semantic command bindings.

Zoom stays between 2% and 1600%. **View ▸ Actual Pixels** (Ctrl+1 or
Ctrl+Alt+0; Ctrl+1 in the Photoshop and Affinity keymaps, 1 in GIMP's) shows
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

GTK, Web and Android also show a rotation slider from
`NumericControl::rotation()`. Both fields use one row with the slider and editable
value, without a visible label or step buttons. Zoom controls and percentages
precede Lock zoom; the rotation slider, Reset rotation and Lock rotation follow
together. Both locks show checkmarks. `SetRotation` rotates about the work-area
centre without changing the artwork. The locks belong to the camera and stop
canvas navigation gestures, including touch, wheel and continuous zoom input;
pan remains available. Explicit sliders, percentages, commands and shortcuts
remain usable while locked. The bottom row uses the same shared
`NAVIGATOR_COMMANDS` order as Navigator: zoom out/in, rotate left/right and flip
horizontal/vertical. These buttons keep the menu open and follow live command
availability.

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
  Escape and a second press close it.

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

Hosts use one header button for both operations. Its tooltip and accessible label
say **Clip to {base}** or **Release clipping from {base}** for content, and
**Apply to {owner}** or **Apply to layers below** for effects. The effect action
uses the vertical link symbol. Attaching across a Pass Through boundary requires
the explicit **Isolate group and attach** action. The same actions appear in
Layer settings, with shared availability and checked state.

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
existing hit area; content generators retain their content thumbnails. Hiding
an owner also hides its attached effects without changing their individual
visibility settings. The shared row marks inherited hiding for a dimmed eye.
Showing the owner restores effects that were not individually hidden.
Use Selection is a normal icon button with squircle corners, a transparent idle
background and the usual hover/pressed states, within its existing hit area.

Pass Through groups have a through-arrow badge inside the folder thumbnail;
expansion keeps its separate folder shape. Every group shows its blend mode in
the subtitle, including Normal. Right swipe invokes the shared group-mode action
or paint alpha lock; see the [gesture rules](drag-and-reorder.md#scrolling-menus-and-cancellation).

Layer drop previews and commits use the same shared planner. A thumbnail hit can
attach an effect to an owner; a row hit chooses a gap or a group destination.
The preview reports the actual insertion target after accounting for attached
effects and clipping runs. Moving an owner carries its effects, and moving a
clipping base carries the run. A Pass Through thumbnail cannot silently become
an isolated effect owner.
Row gaps join a chain only between its remaining members and its owner or base.
Drops above the top member, below the owner or base, between independent chains,
or at the beginning of a group stay outside that relationship. This also applies
to imported images. Effect gaps use adjacent rows; saved Selections cannot be
skipped to infer an effect attachment. Moves that would split an existing
clipping run with a standalone adjustment are refused.
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
