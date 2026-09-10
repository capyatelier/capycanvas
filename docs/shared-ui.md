# Shared state, platform UI

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

`ui_catalog()` describes built-in panels, default toolbar controls and menus,
brush categories/presets, and [numeric input kind, limits, mapping, units and precision](numeric-controls.md). GTK consumes
the same typed constants exposed to DOM through the Wasm catalog; hosts supply
widgets/icons, not separate command lists or numeric rules. The ribbon allocator
derives its item count from each toolbar's saved panel configuration.
`TOOLBAR_CONTROLS` supplies the original defaults. Context menu, picker and
expanded-panel APIs are being integrated as described in
[Panel customization](panel-customization.md). `SetLayerOpacity` may omit `id` to
target the live active layer instead of resolving it from a frontend snapshot.

## Docking

`UiState.workspace` is the authoritative `WorkspaceState`: format version,
complete dock layout, and Zen mode. Save that value using Serde; restore it via
`UiAction::RestoreWorkspace { workspace }`. Validation rejects unsupported
versions, missing/duplicate panels, invalid selections/ratios/extents, duplicate
node IDs, and invalid ID allocation state before any live state changes. A
restore does not edit the document or move the current camera. Automatic disk
storage and a named-workspace picker are not implemented yet; hosts will handle
storage, not reconstruct layout decisions from widgets.

`UiSession::layout` supplies the standard workspace rectangles.
`UiSession::drop_hint` validates a proposed target with the same transactional
move used on drop; GTK and Wasm no longer clone and probe the layout themselves.

A `DockLayout` is an ordered list of `DockBand`s, outermost first. Each band
consumes a strip at an `Edge`: top, bottom, left, or right. Earlier bands own
shared corners. Multiple bands on the same edge form adjacent rows or columns.
The canvas covers the entire window, underneath native chrome. The unoccupied
rectangle is `work_area`, used for initial/explicit Fit Canvas; it is not the GPU
viewport. The single document's title appears in the header, without a tab strip.

A band contains `DockNode::Split` or `DockNode::Tabs`. Splits specify an axis
and fraction. Tab groups contain semantic `Panel` IDs and an active panel.
Stable node IDs target resize, tab selection, and moves. Moving the last panel
out of a group prunes empty groups and collapses redundant splits.

The default has left Brushes/Brush Size (68%/32%) and right Layers bands owning
the corners. The top tool ribbon and bottom canvas-status HUD fit between them;
the HUD also stays above any bottom dock. `DockTarget` supports a new edge band,
a tab insertion slot (including same-group reorder), or a split beside a group.
Moves validate before replacing the layout.

Frontends can translate the topology to native containers or use
`layout.workspace(width, height, top, status_height)` to obtain logical-unit panel,
HUD and divider rectangles. `resolve` is the inner
dock allocator. This shares docking rules without prescribing widget types.
Both frontends dispatch `DragDivider` phases in full-window logical coordinates
and `NudgeDivider` directions for keyboard resizing. Rust owns drag lifecycle,
the 12px nudge step, inset conversion, direction, handle thickness, ratios and limits.
`ResizeDock` remains the direct programmatic center-position action.
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
uses the latest bounds. Hosts no longer set work areas or decide initial fitting.
Resolved groups include tab visibility and tool-tile rectangles; DOM does not
recalculate content height or tool count. Native tiles use the same allocator
with their measured widget size.

`PanelKind::Tiles` uses the shared `tile_layout`: 36-unit square controls,
two-unit gaps and no outer padding for standalone ribbons, one row on top/bottom
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
rectangles. Headers prioritize tab insertion slots; body centers append tabs.
The top and bottom 20% of the body split above/below; narrow side strips split
left/right. Every tab insertion (including a body-center append) shows a vertical
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
cells keep their preview space. Future panel context menus use the same
typography role. Both hosts keep resize handles transparent during hover/drag.
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

GTK uses an `AdwHeaderBar` with native window controls/drag region; the existing
dock widget allocates the full-window GPU picture, transparent header, HUD and panels.
On Wayland, an app-owned Vulkan subsurface presents the full viewport below
transparent GTK chrome. The viewport shader rounds the outer corners; no crop
or theme-colored strips hide any canvas area. DOM elements do the equivalent
in the browser, without fake operating-system window buttons. No custom chrome
compositor, full-window drag gesture, or backdrop blur is required.

An icon-only Zen toggle (a shared, centered capybara SVG, Looking up by default)
occupies the top-left, followed by caret-free Edit and View menus. Its active
state uses the subtle grey hover background while controls are visible. When
the button remains visible alone, its active highlight is suppressed; hover
still works. Header controls use the standard button radius; only the
close control is circular, with unchanged shared header-control colors. Its
circle uses libadwaita's native 16px icon plus 4px padding (24px circle), within
the larger native click target; we do not enlarge the circle to the entire button.
Workspace gaps are 6px at the window sides/bottom, between panels and header
menu buttons, and above/below the 36px header controls. The Zen button is 36×36px,
matching ribbon tiles. The header's bottom padding is normalized from 7px to 6px;
the native close circle keeps its padding and has no additional application inset.
See [libadwaita's window-controls styling](https://github.com/GNOME/libadwaita/blob/main/src/stylesheet/widgets/_header-bar.scss).
GTK's top-right primary menu contains New Window, Preferences, Keyboard Shortcuts
and About Capy Canvas. Web has a gear opening Preferences directly. Panel
visibility/reset live in View. No workspace-grid or
hamburger button remains. The lighter ribbon contains brush, eraser, undo/redo,
color and opacity. Panel contents and ribbons use `#414141` dark / `#EDEDED` light;
tab bars use `#2E2E2E` / `#DEDEDE`. The active tab matches its content, with rounded
top corners and concave lower shoulders joining the two surfaces. GTK paints only
those small corner joins in a non-interactive native overlay; DOM uses CSS
pseudo-elements. Native tab controls and all their hit targets remain unchanged.
The canvas surround is sRGB `#333333` in dark mode and `#B8B8B8` in light mode.
The header and HUD containers are transparent. Their text/control backgrounds
match the surround, disappearing by default but remaining readable over zoomed
artwork. Panels have soft shadows rather than persistent outlines.
Text uses the shared 11 pt typography; keyboard focus halos are disabled
for the requested pen-first presentation, without removing control semantics.
Panel input fields use `#333333` in dark mode and `#FAFAFA` in light mode,
matching the light GTK slider knob interior rather than the canvas surround.

Appearance defaults to System. `Settings.theme = None` (`null` in JSON) follows
the host; Light/Dark are explicit overrides. `SystemThemeChanged` updates the
shared resolved `UiState.theme`, used for widgets, previews and GPU surround.
GTK observes the default `AdwStyleManager` and applies overrides to the display
manager; web observes `prefers-color-scheme`. OS changes never overwrite a user
override. Returning to System uses the latest OS value.

`ZenMode` toggles shared `UiState.workspace.zen_mode`. In the default **Reveal at screen edges**
mode, the shared `near_chrome` rule reveals
hidden controls only within a fixed 80 logical pixels of an occupied window edge,
not by approaching a hidden toolbar/panel. The top always reveals the header;
left/right/bottom reveal only when a visible dock band occupies that edge. The
status HUD alone does not enable bottom-edge reveal. Moving/hiding panels updates
these targets through the shared resolved layout. Once visible, the same
80px margin around panels, header and HUD keeps controls available; enabled
edge zones also retain visibility to prevent oscillation. Hosts animate
opacity over 180 ms and disable hit-testing while hidden. Keyboard navigation,
open menus, settings and native title-bar grabs keep controls available. Floating
panel movement reveals hidden docks only at an occupied screen edge, then holds
that visibility for the rest of the drag. Every drop returns to normal cursor
proximity, without a post-drop pin. These decisions belong to `DragWorkspace`
and the Rust interaction state, not frontend callbacks. An initial contact in a hidden
control's reveal zone reveals instead of painting. A captured stroke does not
reveal controls under its moving tip. Moving away or leaving the window fades
the chrome, except during a native title-bar grab. A release or subsequent
unpressed motion clears that grab latch; leave/cancel during a WM drag does not.
Tab toggles Zen outside settings and native text editors. Web respects reduced-motion preferences; GTK
uses the platform animation setting.
On touch, the last contact keeps revealed controls available after finger lift;
another contact away from controls can hide them. The reveal contact cannot
also activate a newly exposed button.

GTK, web and Android expose Preferences → Appearance → Zen mode:
**Show controls** selects **Screen edges** (default) or **Zen button**. Zen button
hides all editor controls, including floats; edge proximity, contact and
drag/menu pins cannot reveal them. **Keep Zen button visible** defaults on and
keeps the same top-left button in its inactive style while controls are hidden,
in either reveal mode. Clicking it disables Zen. Turning it off hides the button
with the controls; Zen button then requires the Zen shortcut (Tab by default) or
an explicit Preferences shortcut to recover controls. Explicit settings dialogs
remain usable. The button never disappears while Zen is off.

**Button icon** offers Looking up (default), Facing forward, Bathing and Sleeping.
Rust stores `Settings.zen_icon` and supplies each command's icon. The generic
`ChoicePresentation::ImageTiles { columns: 4 }` uses the same choice validation,
persistence and Reset to Default as dropdowns. Hosts render four selectable
64px tiles with centered 48px previews and accessible labels (tooltips on desktop). There is no import control
yet, and no Zen-specific selection logic. The context menu's **Change icon…**
opens the selector via a generic `PreferenceAction::Reveal { id }`: Rust resolves
the page and the host reveals the named row. All four canonical, theme-tinted SVGs live in the shared
icon bank; app/launcher/PWA icons derive from Looking up at build time.
The main Zen icon is 28px inside its unchanged 36px button. SVG artwork retains
roughly 5% padding along its longest dimension without stretching its proportions.
GTK/web Preferences dialogs default to 1000×744 logical pixels, constrained by
the window. Neither has a footer/Done button; × or Escape close them, following
libadwaita behavior without custom outside-click dismissal. Errors appear below
the header only while present. Detailed shortcut dialogs retain their own actions.
Android retains its full-screen settings overlay and pane-level Done button.

Rust owns these decisions through `Settings.zen_reveal_mode`, `zen_show_button`
and `InputReply`'s `chrome_hidden`, `hide_floating_panels`, `keep_zen_button` flags.
Hosts only apply visibility, hit-testing and styling. Android carries all three
flags in its change-detected snapshot, including updates without a document revision.
The button is a sibling of the header with a same-sized spacer, preserving its
36×36px size and 6px inset. Its active background is subtle grey while the full UI
is visible, but neutral when only the button remains.
Right-clicking or touch-holding the Zen button opens the two reveal choices,
a divider, the button visibility toggle, then a separate **Change icon…** entry.
Current reveal/visibility values are checked.
Rust generates the menu from the same preference rows and applies their normal
edit actions without opening Preferences.

Zen's hidden/visible state, last hover/contact, keyboard pin, and first-contact
consumption live in Rust, not frontend booleans. Hosts send `UiInput::Chrome`
events plus `ChromeFacts` (native grab, panel drag, popup visibility). Rust
combines those facts with settings/divider state and returns visibility and
dismiss/consume instructions. The DOM retains only its suppressed-click pointer
ID to prevent the browser's subsequent click from activating a revealed control.

Fading does not change allocation, camera or viewport. Zen is the sole global
controls-visibility toggle; Workspace still manages individual panels. Dock moves,
resizes, hiding and Zen do not request brush/render frames. Window resize still
resizes the full GPU viewport; Fit Canvas explicitly uses updated work-area bounds.

Brush previews are cached transparent PNGs from the actual GPU presets, with
destination paint seeded for blender/eraser/liquify examples. GTK bundles the
same dark/light assets served by the web client. `gpu-bench --brush-previews`
regenerates them; no live brush jobs or canvas readbacks run when opening the picker.

## Actions and observation

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

## Settings and native flows

`OpenSettings { page }` (or the Preferences/Shortcuts/About commands) opens the
core-owned settings view. `preferences()` describes
pages, groups and typed choice/number/switch/information rows, current values,
availability, dependencies, search results, errors and shortcut recording state.
Each accepted `PreferenceAction` edit validates, applies and requests persistence
immediately; the bulk `EditSettings` action does the same for programmatic callers.
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
and shrink to fit smaller windows.
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

Shortcut presentation also belongs to Rust. `Settings::action_shortcut` resolves
typed action identity (including registered parameterized actions), never a
translated label. `action_tooltip` returns `Label (Shortcut)` or just the label
when unbound. Command and toolbar views carry their complete tooltip; other
action buttons request it on hover, so remapping does not leave stale hints.
GTK/Web/Android display these strings without joining keys themselves.

All application context-menu families—workspace, groups/panels, ribbons/tiles,
Zen and layer/mask menus—pass through the same recursive shortcut annotation.
Toolbar configuration options use it too. The core owns their labels, selected
and enabled state, actions, sections and hints. Preference-reset hints retain
the default value alongside any assigned shortcut. Entries without bindings do
not invent defaults. GTK retains its native check indicator when a hint is
present. Web and Android render the same menu models. Native text editing is
separate from canvas commands: Cut/Copy/Paste/Select All copy and conventional
hints come from `text_edit_menu`, while the host text editor owns execution and
selection. OS-provided text-selection menus remain platform-owned. Translation
infrastructure is still deferred; this removes host-owned context-menu copy,
not a claim that the application is already localized.

Android tooltips use Material's [TooltipBox](https://developer.android.com/develop/ui/compose/components/tooltip)
with explicit hover activation. They do not consume touch holds reserved for
dragging or context menus. Their layout wrapper preserves parent sizing and
grid weights. No shortcut lookup runs on the stroke-input path.

Applied settings emit a durable `HostRequest::SaveSettings`; GTK writes atomically
on GIO's I/O pool and web uses localStorage. Completion/error returns through
`CompleteRequest`. `RestoreSettings` validates without emitting another save.
New Window uses the same request/acknowledgement boundary. No file/window handles
or OS paths enter Rust UI state. The host relays applied settings to other open
windows using the same validated restore action; GTK serializes file writes so
an older pending snapshot cannot overwrite a newer one.
See [settings design and validation](settings-implementation-plan.md).

`cursor_input` accepts the latest optional pen/hover
record independently of the paint queue; `canvas_cursor` returns vector paths
in logical display coordinates. Hosts display those paths without brush math or
canvas readback. Native GPU segments and web SVG use the same thin contrasting dashes.

When file pickers or other asynchronous platform services are implemented,
their handles must stay in the frontend. Add a typed request/result carrying an
opaque resource identity at that point; do not put paths, browser Files, or
platform permissions into shared state ahead of a real use case.

## Binding portability

The Wasm wrapper uses typed scalar input and owned JavaScript records, not JSON
per pointer sample. `u64` identities/revisions cross as JavaScript `bigint`.
Actions use explicit tagged variants; native GTK calls those Rust variants
directly. Other frontends can expose this same finite, synchronous facade using
UniFFI for Swift/Kotlin or a C ABI for WinUI when those clients are built.
No frontend duplicates validation, command availability, or document behavior.

GTK and web are developed together; see the current acceptance checklist in
[ui-implementation.md](ui-implementation.md). Shared tests cover state and
input semantics; actual frontend tests and screenshots must additionally prove
widget wiring, docking, presentation, and drawing in both themes.

In the GTK host, `preferences.rs` renders the preferences view and services its
storage/window requests; `workspace.rs` maps other state to
native controls and allocates them from the shared layout; `input.rs` collects
GTK stylus/history/touch records; `canvas.rs` advances the shared session and
paces frame preparation. `render_thread.rs` owns the bounded frame handoff,
Vulkan brush engine, viewport/cursor presenter and swapchain. `wayland.rs` owns
the child surface/protocol lifetime, not GTK's connection or parent. No host
module duplicates brush rules or manipulates canvas pixels.
`previews.rs` embeds the shared swatches. `main.rs` owns application lifetime. The canvas stays parented while dock
wrappers change. Controls and divider handles are reused. A 120 Hz timer handles
input/frame preparation and native control refresh, then stops when idle.
At most two paint frames are in flight; when busy, pen records remain queued.
The GPU worker sleeps when idle. No GPU wait, image import or canvas pixel copy
runs on GTK's drawing/input hot path. The shared cursor geometry is drawn in the
same GPU viewport pass; the browser uses its equivalent SVG paths.
The web has the equivalent native DOM host in `app.js` and a small Wasm bridge
in `src/lib.rs`. There is no cross-toolkit widget framework or second action
dispatcher.

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
