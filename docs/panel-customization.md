# Panel and toolbar customization

## Interaction contract

The Rust UI core owns customization and the complete serializable workspace.
GTK and web translate native events and render its menu/dialog/control models.
This feature does not add another canvas/rendering path.

- A panel tab or panel body opens that panel's context menu: **Tab Name** /
  **Tab Icon** and **Configure Panel…**. A single tap on the selected tab toggles
  configuration; an inactive tab selects it. Inputs retain native behavior.
- Empty tab-header space and the group grip target the whole tab group:
  **Tab Names** / **Tab Icons** and **New Toolbar…**. Group changes apply to
  every current tab; an individual tab can subsequently override its style.
- A ribbon tile targets that tile: **Remove Tool**, **Insert Tools…**.
  Empty ribbon space targets the ribbon: **Add Tools…** (append).
  A standalone ribbon's grip still drags the entire ribbon; its context menu
  uses the empty-ribbon actions. A tabbed ribbon keeps the group grip.
- Mouse/pen secondary click and touch press-and-hold open the same menu.
  Native gesture recognition owns timing/slop; the deepest applicable target
  wins. Recognized hold/drag suppresses the ordinary click, and scrolling cancels
  a pending hold. Existing input/context menus inside text inputs remain native.
- **Configure Panel…** raises the existing tab group and animates its bounds
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
- Zen keeps chrome visible for the complete drag lifecycle, including pointer
  leave and focus loss while the native DND grab owns input. Drop or cancellation
  changes that pin to the same wait-for-another-contact state as drawer dismissal,
  so the resulting layout stays visible. Blur still cancels canvas input but
  does not end UI dragging.

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

- System panel identities remain stable. Custom toolbar identities are allocated
  independently of their editable display names and can be docked/tabbed exactly
  like system panels. There is no fixed toolbar count.
- `DockLayout` contains panel configurations alongside its docking tree, so
  geometry, tab styles, visible controls, toolbar names and ordered tiles restore
  atomically. The existing `WorkspaceState` wraps it and Zen mode.
- Toolbar tiles have stable IDs and typed controls. The available-button catalog
  includes application commands, brush presets, size presets and color/opacity
  buttons; control values and execution remain in the existing Rust session.
- System-panel control catalogs define allowed controls and compact defaults.
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
per-panel and group tab styles, expanded-control view metadata, tile movement and
variable-count ribbon allocation. Old workspace JSON receives the original panel
configuration. Tests cover these policies and native/Wasm type checks pass.

GTK now renders dynamic toolbars, native contextual menus, the searchable picker
and in-place two-column configuration. The original group and preview controls
stay in their parents; closing restores its allocation without changing saved geometry. Native
checks exercise group/panel/tile/empty-ribbon targets, name/icon choices,
creation/insertion, control visibility and editing, a GTK drop signal, restore
and reset. Dark/light GTK widget captures are inspected in
`artifacts/ui/customization/` (ignored). Gesture signals test native bindings,
not physical tablet/touch delivery or compositor timing.

Tabbed ribbons reserve one padded lane below the header; further overflow clips.
GTK tool ribbons are explicitly clipped and never gain a scroller.

Web uses `customization.js` for DOM context menus, picker widgets, live controls
and expansion presentation. `app.js` keeps event routing and the existing panel
widgets; toolbars now consume the dynamic Rust tile views instead of a fixed
six-button list. The Wasm adapter exposes the same context, picker, tile layout,
expanded geometry and validated drop APIs. Touch uses pointer capture for moving
tiles/tabs and a cancellable long-press recognizer for context menus; mouse/pen
secondary click uses the browser context event. Disabled command buttons remain
inside an enabled drag/context target, so they can still be removed or moved.

The web expansion animates the existing group's two columns with one CSS
`drop-shadow` on their common ancestor. GTK wraps the whole group's snapshot in
one GSK shadow. Both include the preview, tabs and drawer, without darkening their
internal seam. DOM content heights are measured on layout changes, not every
animation frame; interpolation and placement stay in Rust. Switching tabs uses
the currently displayed bounds as the animation origin. Docking into an expanded
group keeps its configuration synchronized with the group's active tab.

Regression coverage:

| Contract | Evidence |
| --- | --- |
| Named toolbars, transactional multi-selection, search/cancel/validation | Core customization tests; GTK and web picker controls |
| Panel/group/tile/empty-ribbon menus and individual/group tab styles | Core context models; GTK gesture/action signals; browser pointer/hold events |
| Live controls, visibility, selected-tab toggle, different-tab switch, outside/Escape dismissal | Core interaction tests; native expansion test; browser customization test |
| Same/cross-toolbar moves, stable IDs and insertion previews | Core slot/move tests; native drop signal; browser native-DND and touch movement |
| Dynamic wrapping, tabbing, clipped overflow and resize | Core allocation tests; GTK ribbon captures; web customization/parity checks |
| Combined shadow, all four expansion edges, seamless corners, 11pt controls | Dark/light GTK and browser captures; web geometry/shadow comparison |
| Workspace restore, reset retaining custom toolbars, malformed/stale targets | Core validation tests and both host round trips |
| Zen drag/dismiss lifecycle and no accidental ink | Core input tests; native expansion test; browser customization/parity tests |

Run the native tests separately (GTK initialization is thread-affine):

```sh
cargo test --release -p layer-linux native_panel_customization -- --ignored --test-threads=1
cargo test --release -p layer-linux native_panel_expansion -- --ignored --test-threads=1
cargo test --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1
```

Use isolated `LAYER_SETTINGS_FILE` paths and a Wayland/Vulkan display. After
building/serving the web client, run `node apps/layer-web/test.mjs --customization`
and `--parity` against `LAYER_WEB_URL`. Captures are under the ignored
`artifacts/ui/customization/` and `artifacts/ui/parity/` directories. The browser
harness uses a temporary profile; its software Canvas2D context only inspects
captured PNGs, never renders the application's canvas.

These tests exercise native bindings and browser-injected input, not physical
tablet delivery or an iPad device. They do not claim a new latency benchmark;
customization leaves the GPU brush/raster path unchanged. Static packaging
includes and fingerprints the new module and its importing app, so service
worker versions follow the changed runtime content. Generated bundles/captures
remain ignored and no third-party code or assets are added.
