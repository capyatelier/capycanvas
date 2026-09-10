# Familiar drawing workspace (GTK review)

This is the active implementation checklist, not a completion claim. The complete
tool set, panel layout and interactions below must work before the GTK review.
Web/Android-specific presentation is not part of this approval milestone; shared
models and behavior must remain portable.

## Required layout

- Left edge: Pen, Pencil, Brush, Eraser, Airbrush, Decoration, Blend, Liquify;
  divider; Lasso, Auto select, Fill, Gradient; divider; Operation, Figure,
  Ruler, Hand, Eyedropper, foreground/background color selector.
- Second left column: Tool Set, Tool Settings, Brush size, Color, vertically stacked.
  **Tool Set and Tool Settings must fit three standard tiles inside their minimum
  width**, with normal two-pixel tile gaps and content padding. Group selectors
  wrap; labels truncate instead of forcing wider panels. Layers keeps its
  separate six-tile minimum. Settings controls must remain usable at minimum width.
- Top, between sidebars: New, Open, Save; divider; Undo, Redo; divider; Clear,
  Fill selection, Scale/rotate selection; divider; Flip horizontal.
- Right: Navigator (Diagnostics as its next tab), Properties (Filters as its
  next tab), then Layers in its own group, vertically stacked. This supersedes
  the earlier request to group Filters with Layers. Properties remains layer properties today;
  its name also suits future selected-object properties.
- Text, Comic and Correct line are explicitly excluded. Do not add inert buttons.

Dividers are actual toolbar items with stable identities, compact axis-aware
geometry, drag/reorder support and context customization, not disabled commands
or full-sized blank tiles. Preserve existing floating/docking/zen interactions.

## Application menus (added to this milestone)

The title bar contains **File, Edit, Layer, Select, Filter, View, Window, Help**
in that order. Rename Workspace to Window. Research customary actions in drawing
applications and add the missing working actions appropriate to this milestone;
do not fill menus with inert placeholders. Group related commands with separators
and show current core-resolved shortcuts alongside their labels.

- Layer projects the same actions, enabled states and selection semantics as
  the layer context menu, rather than maintaining a second command list.
- Filter projects the filter catalog with one submenu per category, including
  runtime-loaded filters. Insertion follows the same clipping-stack and Properties
  behavior as the Filter panel.
- File owns document/window creation, open/save/export and their native flows.
- Edit owns document undo/redo and other supported editing operations.
- Select owns selection creation/modification, deselection and inversion.
- View owns canvas/navigation and Zen display actions.
- Window owns the existing workspace/panel/toolbar management operations.
- Help owns shortcuts/help/about and relevant application links.

All menu copy, sections, actions, availability and shortcut resolution belong to
Rust; GTK renders the model. Validate against existing context/panel actions so
enabling, selection and execution cannot diverge.

## Tool-tile drawers (added to this milestone)

Drawers are part of the same GTK approval goal, not deferred follow-up work.
Rust owns tile semantics, open/closed state, contents, positioning and animation
geometry. Hosts report tile bounds, content measurements and input; they do not
independently decide when a tool is selected or a drawer opens.

- A selectable tool tile first selects its tool/subtool. Pressing it again opens
  its drawer; pressing the originating tile again closes it. Selecting another
  tool closes the old drawer. An already selected tool opens on its first press.
- Dedicated drawer tiles are not selectable tools. Color and built-in-panel
  tiles toggle their drawer directly. Opening these does not change the tool.
- Clicking outside closes the open drawer. Preserve existing zen semantics:
  dismissal is not also a drawing stroke or a second request to hide all UI.
  Reuse the existing expansion lifecycle and animation duration; do not create
  competing open drawers or separate per-host visibility logic.
- The drawer connects to its originating tile across the normal panel gap,
  instead of appearing as an unrelated floating box. Extend that tile toward
  the drawer and use concave, tab-like joins into the body. Rust determines
  the connector geometry and suppresses concave joins near an aligned body
  edge; round only the exposed corners. Apply this to normal and Zen tiles,
  every opening direction, viewport clamping, resizing and animation.
- Every actual tool/preset gets two columns: Tool Set, then Tool Settings.
  Color gets one Color panel. Opacity gets Tool Settings, exposing the editable
  value and its related controls. Size preset tiles remain immediate value
  choices; add a Brush size panel tile for its full controls.
- New/Open/Save, undo/redo, clear/fill selection, transforms and navigation
  commands remain immediate actions. They do not acquire drawers just because
  they are toolbar buttons. Add dedicated drawer tiles for every built-in panel
  to the shared toolbar picker; they need not appear in default toolbars.
- Drawer composition is declarative columns of vertically stacked panel bodies,
  without tabs, grips or panel decoration. Columns have vertical separators.
  Each built-in panel declares a separate, more relaxed `drawer_width` in Rust,
  wider than its ordinary dock default and respecting its intrinsic minimum.
  Use this width in every drawer type, superseding the earlier three-tile drawer
  width. Docked Tool Set/Tool Settings still retain their three-tile minimum. Content
  uses ordinary panel models and shared preview caches; opening a drawer must not
  detach an existing docked widget or duplicate GPU preview generation.
- Top toolbar: open downward, left-aligned with the tile; shift left only as
  necessary to fit the viewport. Height follows changing content up to the
  bottom of the usable viewport, with scrolling beyond it.
- Bottom toolbar: open upward with the same horizontal alignment. Requested
  height is min(800px, viewport height excluding the title bar). On short windows
  cap it to the actual room above the tile so it does not cover the title bar.
- Side toolbar: open toward the center. Start 500px above the tile, clamped to
  the usable viewport top. Height reaches at least the bottom of that tile and
  otherwise follows content, capped at the viewport bottom with scrolling.
- For floating toolbars, use their orientation and the side with available room;
  this decision also lives in Rust. Clamp drawers on small windows and after
  viewport resizing rather than leaving controls off-screen.

Validation must cover selectable/direct-open tiles, repeated presses, outside
dismissal, switching tools, zen, all dock edges, floating toolbars, live content
height changes, overflow scrolling, built-in-panel tiles and simultaneous docked
and drawer projections of the same panel.

## Zen modes (replacement requirement)

Replace the independent Show controls / Keep Zen button visible settings with
one **Total zen** toggle, off by default, in Appearance's Zen section and the
Zen button context menu. Retain the icon chooser. Remove superseded settings,
options and behavioral branches; the two modes below are the only choices.

- **Partial Zen** (Total zen off): hide the regular editor chrome, but keep the
  Zen button at the top left and standalone edge-docked toolbars visible.
  Toolbars in tab groups and ordinary floating panels are not additional retained
  chrome. Edge reveal is disabled; clicking the Zen button or its shortcut exits
  Zen to show the editor again.
- Project each retained toolbar as one row/column at its configured tile size.
  Split at actual toolbar dividers, showing each non-empty section as a separate
  floating-style bar. These are transient projections, not new floating workspace
  panels. Preserve tile identities, order, actions and the normal saved layout.
- On top, anchor two clusters to the left/right ends, split near half the tile
  count. Never split a section: one crossing the midpoint belongs to the right
  cluster. Within each cluster use one tile-width gap. On bottom, distribute
  sections evenly with the first/last at the ends; a single section is centered. On left/right,
  center the sections together as one stack with one tile-height gap between
  sections (largest configured height on that edge). Reduce gaps before clipping
  overflow on short viewports. Move the partial-Zen top
  toolbar to the physical window top edge, with no menu/title-bar inset.
  Reserve the top-left Zen button and other occupied corners so bars cannot
  cover them. Core geometry owns reservations, section sizing and placement.
- **Total Zen** (toggle on): hide the Zen button and toolbars too. Reveal the UI
  by approaching occupied dock edges, using the existing fixed distances and
  entry guard. The Tab shortcut continues toggling Zen in either mode.

This supersedes earlier independent reveal-mode and button-visibility requests.
Implement with the toolbar-divider work and shared layout/input model, GTK first;
no per-host implementations of reveal policy or section distribution. Validate
all edges and tile sizes, single/multiple sections, empty/adjacent dividers,
multiple toolbars, corner collisions, entry/exit, tool drawers, and unchanged
normal workspace persistence. The shared toggle and GTK split-bar presentation
are implemented and validated. Connected tile drawers share their geometry and
  corner decisions in Rust, including during animation; GTK paints the same
  panel-colored stem and concave joins as the existing panel configuration design.

## Collapsible panel columns (added to this milestone)

Collapsing is a state of a docked column, not conversion to a floating toolbar
or destruction of its panel groups. Keep group identity, order, active tabs,
contents and expanded column width. Rust owns state, geometry, thresholds,
drop eligibility and drawer selection; GTK only renders and forwards input.

- **Collapse column** is available from any panel-group context menu in the
  column. Drag-resizing the column closed also collapses it; start with the
  collapsed strip width as the threshold. Avoid threshold oscillation during
  a resize gesture. Expansion restores the remembered ordinary width.
- Double-clicking the **non-tab area of any docked panel group's tab bar** also
  collapses its containing column, whether the group has one tab or several.
  This replaces the existing docked single-panel header double-click toggle
  for tab-bar visibility (now labeled **Show tab bar**). Actual tab buttons do not trigger collapse. Keep the distinct
  floating-panel reset/toggle and standalone-toolbar double-click behaviors.
- The collapsed column is a full-height docked strip, styled like a toolbar,
  occupying its allocated dock area. It cannot become floating. A small, flat
  expand button with a `<>`-style arrow sits at the top, with a gap below.
- Each panel group is represented by a one-icon-wide vertical mini-toolbar,
  with one icon per panel/toolbar tab and the active item indicated. Keep a gap
  between groups; do not flatten the groups into a single list.
- A flexible empty group region fills the remaining space after the final
  group. A bottom grip moves the **whole collapsed column** to an eligible
  viewport edge or between other columns. Preserve its groups and collapsed
  state while moving; never leave it as a floating window on a canvas drop.
- Each mini-toolbar is a drop target for panels, toolbars and whole panel
  groups. Insert their tabs at the corresponding position in that collapsed
  group, following ordinary group-merging semantics. Gaps insert a new collapsed
  group at that location. The trailing empty region permits appending groups.
- Clicking an icon opens the group's drawer with that item selected. This is
  a content drawer, **not** the panel-configuration drawer. It displays all tabs
  in that group. Switching tabs updates the selected icon in the collapsed
  strip. **The drawer remains open on outside clicks**, including canvas input.
  Close it by clicking its original opening icon, or replace it by opening
  another drawer in that column. This supersedes the earlier outside-dismiss
  requirement for collapsed-column drawers only; tool-tile drawers still close
  on outside clicks. Model dismissal policy in Rust and reuse the shared drawer
  lifecycle and animation, rather than introducing host-specific exceptions.
- The group drawer uses the **maximum declared drawer width of its tabs**, so
  changing the active tab does not change its width. All built-in panels use
  their shared `drawer_width` here and in tool-tile drawers. Keep drawers within
  the usable viewport, with content scrolling where needed.
- Fit this into the existing recursive column layout, rather than creating a
  second docking system. Preserve workspace save/restore and undo/redo behavior;
  ordinary floating-panel tear-off rules must not turn collapsed columns into
  floats. Derive valid locations through shared docking eligibility rules.

Validation: context-menu, resize and empty-header double-click collapse;
single/multi-tab groups and exclusion of actual tabs; expand/restore width,
threshold stability, multiple/nested columns, whole-column movement, each kind of incoming
drop, insertion between groups and at the end, tab/icon synchronization, sticky
outside-click behavior, opening-icon dismissal, replacement within the column,
Zen, constrained viewport/overflow, workspace persistence and undo.
Verify a collapsed group drawer uses its widest tab's drawer width, and that
all drawer variants consume the same per-panel width metadata.

## Interaction decisions

Use drawing-oriented CSP key families: P for Pen/Pencil, B for Brush/Airbrush/
Decoration, E for Eraser, J for Blend/Liquify, M for Selection, W for Auto select,
G for Fill/Gradient, O for Operation, U for Figure/Ruler, H for Hand and I for
Eyedropper. Repeated family keys cycle its tools in toolbar order; direct custom
bindings remain possible. Keep Space temporary pan and Tab zen. New/Open/Save
use the platform command modifier with N/O/S; standard undo/redo stay unchanged.
This follows [CSP's tool shortcut table](https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Tool_Shortcuts.htm),
not a claim that all drawing applications use identical keys.

Tool Set shows only groups belonging to the active tool. Group buttons have an
icon and name, three tiles wide and one tall, with exactly one selected. The list
below shows that group's subtools and retains useful brush stroke previews.
Switching tools remembers their last subtool. Tool Settings comes from shared
metadata and follows the current subtool; no GTK-owned brush or selection policy.

Existing GPU brush presets supply Pen/Marker, Pencil/Pastel, paint/watercolor/oil,
eraser, airbrush/spray, decorative textured stamps, blend/smudge and liquify.
The grouping follows the responsibilities in [CSP's drawing-tool overview](https://help.clip-studio.com/en-us/manual_en/060_pc/Drawing_on_the_canvas.htm).
Missing tools must be real operations, not aliases that merely change an icon:

| Tool | Initial working behavior |
| --- | --- |
| Lasso | Freehand/polygon selection; selection constrains painting and operations |
| Auto select | Contiguous color selection, tolerance and reference-source choice |
| Fill | Contiguous fill using current/reference layers; fill into the editing target |
| Gradient | Drag linear/radial foreground-to-background or foreground-to-transparent fill |
| Operation | Select/move layer content and edit/transform the selected object/selection |
| Figure | Line, rectangle and ellipse, outline/fill modes and constrained proportions |
| Ruler | Editable straight, parallel and radial guides with stroke snapping |
| Hand | Direct drag navigation without altering document pixels |
| Eyedropper | GPU sample of visible artwork or editing layer, updating the active paint color |

Rulers are durable, undoable guide geometry, not paint pixels. Ruler draws/edits
guides; drawing tools snap when enabled. Choose the guide at stroke start to
avoid jumping between guides during a stroke. Parallel guides preserve the
stroke's initial offset; radial guides converge at their center. Operation can
select/move guides. Snap visibility/enabling belongs in shared tool state.
These initial types follow [CSP's ruler behaviors](https://help.clip-studio.com/en-us/manual_en/510_ruler/Basics_of_Creating_Rulers.htm).
Curved/perspective/symmetry rulers are later extensions, not placeholder tools.

## Color and navigation

Color uses a custom hue ring, HSV square or HLS triangle, with three editable
components below. Foreground, background and transparent-paint swatches sit at
bottom left, swap beside them, HSV/HLS toggle at bottom right. Transparent paint
uses the current brush as an eraser, not a different preset. Picking a color
returns to the last foreground/background slot. Preserve hue when saturation is
zero. Hit testing, conversions, component values and selected swatch live in Rust;
GTK only renders geometry and reports input. Reference: [CSP Color Wheel](https://help.clip-studio.com/en-us/manual_en/300_color/Color_Wheel_palette.htm).
The supplied screenshot is inspected locally, not imported as a repository asset.

Navigator displays a full-document, aspect-correct GPU thumbnail and the visible
work-area rectangle, excluding docked bars/panels. Rotation/flips transform that
rectangle correctly. Dragging it updates the shared camera. Bottom controls zoom,
rotate 90° both ways, and flip horizontally/vertically. Flips are view operations,
not destructive document edits. Camera changes update only the rectangle;
thumbnail generation is small, asynchronous, revision-driven and capped in rate.

## Engineering and validation gates

- Keep canvas raster, selection masks, flood operations, gradients and transforms
  on the GPU. CPU may own geometry, input, metadata and undo, never a fallback
  canvas raster. Reuse the existing ordered layer-operation/selection machinery.
- GPU fills/selections must respect reference choices, alpha lock, masks, layer
  offsets and document boundaries; do not substitute a full-image color match for
  contiguous flood selection. Final algorithm needs correctness and latency tests.
- New/Open/Save must persist actual document state/assets, handle cancellation and
  unsaved work; no misleading success for unsupported data.
- Test color round trips and achromatic hue, HSV/HLS dragging, fg/bg/transparent;
  camera round trips with flips/rotation, navigator work-area bounds and panning.
- Test all tool actions, shortcut family cycling/rebinding, dynamic groups/settings,
  GPU image results, references, selection boundaries, undo/redo and save/reload.
- Audit GTK dark/light screenshots at default and three-tile widths, divider
  wrapping, touch/stylus input and existing panel/zen behavior. Exercise every
  visible tool and command. Measure warm drawing and new operation costs separately.
- Obtain human approval only after the full GTK implementation and validation.

## Progress

- Repository inventory and primary-source tool/color/ruler research completed.
- Tab groups now default to Automatic: icons and names at one/two tabs, icons
  and only the active name at three or more. Fixed Icons and names is also
  available; explicit saved styles are retained. All five styles and the live
  two/three-tab transition pass GTK, Wayland Chrome and Android emulator tests
  in both themes; captures are in `artifacts/ui/group-tab-styles/` (ignored).
- Shared three-tile minimum implemented for Tool Set and Tool Settings: 112px
  content + 8px each side = 128 logical pixels. Layers retains its own minimum.
  Fixed ribbon height measurement to use the same constrained split widths as
  allocation; a narrower neighboring panel no longer miscalculates wrapping.
- Custom HSV/HLS Color panel and schema-driven brush Tool Settings implemented
  on GTK, available through Workspace. Color state, hit testing, conversions,
  numeric constraints and brush edits are shared Rust. Stroke snapshots remain
  immutable while next-stroke settings are edited.
  Rust marks these new panel bodies GTK-only pending review, excluding their
  controls and add-panel actions on web/Android so hidden registrations cannot
  break hosts whose view implementations have not yet been built.
- GTK color ring uses a continuous [conic gradient](https://docs.gtk.org/gtk4/method.Snapshot.append_conic_gradient.html)
  clipped to a stroked circle, avoiding antialiased seams between color wedges.
- Painting-tool families now own their groups and subtools in shared Rust.
  Pen, Pencil, Brush, Eraser, Airbrush, Decoration, Blend and Liquify select actual
  existing GPU presets. Switching tools/groups remembers the last subtool and
  its edited parameters; color remains shared and active strokes retain their
  original immutable snapshot. GTK projects only the active group's subtools.
- P/B/J cycle their tool families in toolbar order; E/M/O select Eraser/Lasso/
  Move. Separate direct command bindings remain programmable. Explicit saved
  shortcuts take priority over new default keys when loading settings.
- Tool Set's group buttons measure 112×36px (three tiles by one), with two-pixel
  gaps. They and subtool buttons use the existing selected-tool tint. GTK uses
  the shared symbolic SVG bank and existing cached brush previews.
- Tool/panel drawers now have shared selection, direct-open, dismissal and
  geometry rules with 272–320px per-panel drawer widths. GTK projects independent
  panel bodies using the existing native controls, without reparenting docked
  widgets. Dynamic contents animate; each column scrolls overflow. Every current
  built-in panel is available as a dedicated drawer tile on GTK.
- Layer/filter previews have one producer across dock and drawer views. Native
  tests verify identical filter texture objects in both views, no new preview
  requests when unchanged drawers reopen, and native context-menu ownership.
  Opening Diagnostics in a drawer enables shared renderer telemetry too.
  Shortcut handling stays available outside text editors; entering Zen closes
  the drawer without accidentally revealing chrome again.
- Validation so far: 165 shared UI tests, 23 engine tests, workspace and Wasm compile,
  strict UI/GTK Clippy, and isolated Wayland/GPU checks of all 24 brush control
  schemas at 128px. Native expression editors and color buttons exercised;
  GTK family tests activate all eight tools and every group/subtool using native
  buttons in both themes, checking selection, dimensions and applied brush state.
  Dark/light tool-family, HSV/HLS and narrow-panel captures are in the ignored
  `artifacts/familiar-workspace/` directory.
- Native drawer checks cover eleven tool/panel tiles in both themes, all dock
  edges, live numeric editing, diagnostics and shared preview reuse. Captures
  use the `drawer-` prefix in the same directory. Existing side-panel expansion
  regression also passes. Floating/constrained placement and stacked-column
  measurements have shared geometry tests; collapsed-column UI is not built yet.
- Release Wayland G-Pen smoke benchmark (384px, 6 seconds): 722 submitted frames,
  721 displayed, approximately 119.7Hz. CPU worker median/p95/p99:
  0.263/0.575/0.693ms; GPU: 0.138/0.522/3.037ms; GTK frame handler:
  0.012/0.032/0.045ms. This synthetic-input run checks the normal drawing path,
  not physical tablet delivery or a before/after drawer-open comparison. Raw
  output is `/tmp/capy-tool-drawer-pacing.json`, not a checked-in artifact.
- Toolbar dividers now take eight logical pixels, retaining stable tile IDs and
  insertion slots. Ribbon measurement, wrapping and drop markers share compact
  extents. Partial Zen reuses the same tile actions in separate native sections;
  side sections stay centered with one tile-height gap; top sections form
  left/right clusters split on section boundaries, bottom sections spread out.
  Geometry and hit tests are core-owned; no saved panels are moved or cloned.
- Tool drawers connect only to their originating tile across the 6px panel gap.
  The source tile's facing corners flatten; concave joins shrink away near a
  drawer corner, and aligned body corners flatten to maintain a continuous edge.
  Opening/closing grows from the attached side. The connection is part of the
  drawer's hit region, never a canvas drawing target or outside-dismiss contact.
  Opening also darkens the source toolbar background slightly (8% black mix),
  while the originating tile retains the panel background. Partial-Zen section
  corners reached by that tile flatten too, so the ancestor clip cannot cut off
  the connection. GTK reuses the native tile node above the drawer shadow rather
  than darkening that tile or creating another input widget. All styles restore
  after dismissal. Pixel tests cover the joined corners and contrast in both
  themes, not just the presence of CSS classes.
- Current milestone validation: 171 shared UI tests; workspace/Wasm checks;
  strict UI/GTK Clippy; GTK `native_tool_drawers`, `native_zen_behaviors` and
  `native_panel_expansion` on the private Wayland GPU display. Zen tests cover
  four dock edges in both themes, exact tile bounds and picking, connection
  contacts, outside dismissal and unchanged normal layout. Reviewed images use
  `zen-` and `drawer-` prefixes in `artifacts/familiar-workspace/` (ignored).
  Web/Android test fixtures use the replacement setting ID, but their native
  section/drawer presentation is not rolled out or device-tested this milestone.
- Paired release Wayland G-Pen benchmark after the final changes (384px brush,
  six seconds of synthetic input; real child-surface presentation feedback):

  | UI | CPU worker median/p95/p99 ms | GPU median/p95/p99 ms | GTK handler median/p95/p99 ms | Displayed Hz |
  | --- | --- | --- | --- | --- |
  | Normal | 0.258 / 0.541 / 0.743 | 0.103 / 0.266 / 0.415 | 0.014 / 0.036 / 0.044 | 119.75 |
  | Partial Zen | 0.289 / 0.538 / 0.699 | 0.122 / 0.331 / 2.084 | 0.013 / 0.033 / 0.042 | 119.98 |

  Both sustain the 120Hz display target. GPU tails vary between runs: the earlier
  pair had normal/partial p99 of 1.936/0.737ms, respectively. The final partial
  p99 is higher, still below 8.33ms; these short runs do not establish a fixed
  GPU overhead or measure physical tablet latency. Reports are the temporary
  `capy-zen-normal-pacing.json` and `capy-zen-partial-pacing.json` files.
- Navigator is implemented on GTK, with shared camera reflection/rotation, work-area
  geometry, drag/recenter behavior and six navigation commands. Diagnostics is its
  intended companion tab. The new default workspace is still pending; Navigator is
  available through the panel picker and as a dedicated tool drawer tile.
- Navigator samples the existing GPU composition, including provisional strokes,
  clipping/masks and filters. One persistent 256px-long-side output/staging pair
  serves all its views; one request may be in flight, capped at 15Hz when visible.
  Unchanged composition returns only its revision, with no GPU pass/readback;
  camera-only motion updates eight small native outline rectangles.
  At 256×192 the GPU target/staging storage is 384KiB, plus a 192KiB returned CPU
  image and GTK's small texture upload. Maximum square target/staging storage is
  512KiB. Canvas pixels are not copied to GTK for normal drawing/panning.
- Intermittent native thumbnail updates exposed a presentation-scheduling issue:
  an arbitrary-phase canvas timer could repeatedly miss the compositor deadline
  when GTK restarted its idle clock. Use actual Wayland presentation phase/period
  (one sample per second, 120Hz fallback) and retain GTK's update clock only during
  visible live-thumbnail changes. Do not force GTK redraws. Panning remains driven
  independently of GTK. Regression assertions verify live/idle clock ownership.
- Serial release measurements on the private 120Hz Wayland display, six seconds
  each, 384px G-Pen, synthetic input with real child-surface presentation feedback:

  | Navigator | Operation | CPU worker median/p95/p99 ms | Canvas GPU median/p95/p99 ms | GTK handler median/p95/p99 ms | Displayed Hz |
  | --- | --- | --- | --- | --- | --- |
  | Hidden | Draw | 0.311 / 0.596 / 0.773 | 0.150 / 0.326 / 1.483 | 0.014 / 0.036 / 0.048 | 119.97 |
  | Hidden | Pan | 0.168 / 0.376 / 0.472 | 0.067 / 0.147 / 0.293 | 0.005 / 0.021 / 0.029 | 119.65 |
  | Visible | Draw | 0.285 / 0.560 / 0.705 | 0.138 / 0.330 / 0.602 | 0.013 / 0.035 / 0.046 | 119.14 |
  | Visible | Pan | 0.167 / 0.367 / 0.457 | 0.101 / 0.180 / 0.783 | 0.009 / 0.021 / 0.026 | 119.69 |

  Navigator-on runs discarded three/two presentations, respectively; this is
  approximately 120Hz, not a guarantee that every refresh displays a new frame.
  GPU timestamps cover canvas submissions, not the separate thumbnail or GTK
  passes; end presentation includes their scheduling impact. No physical input
  latency or mobile performance is claimed. Reports are temporary
  `capy-navigator-verified-{0,1}-{GPen,Pan}.json` files, not checked-in artifacts.
- Navigator validation: shared camera/geometry/cache tests, native dark/light
  dock/drawer captures, live/idle timing assertions, and GPU checks of provisional
  strokes, masks, transparency and unchanged camera revisions. The layer GPU suite
  passes eight correctness tests (its separate latency benchmark remains ignored).
  Native `native_navigator` and `native_tool_drawers` pass; the latter now covers
  twelve tile types. Captures remain under `artifacts/familiar-workspace/` (ignored).
- GTK Filters now insets its header/list contents, not the scroll container;
  the scrollbar reaches the panel edge like Layers. Native dock/drawer geometry
  checks preserve six-pixel content insets. Total zen has no description, from
  the shared preferences schema.
- Same-slot drops preserve sizes, IDs, active tabs and layout history. The shared
  move path compares dock placement independently of split fractions and equivalent
  same-axis binary nesting. Dropping back after temporary tear-off restores the
  original layout too. Resize gestures and actual reorders keep their normal
  behavior. Native handle tests verify exact before/after bounds; shared tests
  exercise nested rows/columns, individual/group moves and all three host profiles.
  Current shared UI suite: 177 passing tests; strict UI/renderer/GTK Clippy passes.
- Still to implement: the new default layout, missing canvas tools/commands,
  collapsible columns, the full application menus, rulers, remaining shortcuts
  and full functional/performance validation.
- The complete eight-tool ribbon is currently exercised by the GTK review test;
  the shipped default toolbar/layout will change when its remaining tools and
  commands are implemented. Tool-family presentation on other hosts awaits GTK
  review, while core models and Wasm remain compatible.
- Implementation in progress; the requested default workspace is not yet shipped.
