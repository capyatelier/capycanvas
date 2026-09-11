# Familiar drawing workspace (approved GTK design)

The GTK design was approved on 2026-09-11, with Navigator made slightly taller
at Layers' expense. The requirements below remain the shared behavior contract.
Web/Android-specific presentation is not part of this GTK approval milestone;
shared models and behavior remain portable.

## Approved GTK checkpoint

The integrated feature audit on 2026-09-11 passes the following native scenarios
from `apps/layer-linux/src/tests.rs`. Older progress/remaining-work entries below
are historical; this checkpoint and the current requirements take precedence.

| Requirement | Current verification |
| --- | --- |
| Default columns, both ribbons, all 17 tool tiles, compact layout | `native_default_workspace` |
| Tool families, subgroup memory, shortcuts, dynamic settings, three-tile minimum | `native_tool_families`, `native_tool_and_color_panels`, shared UI tests |
| HSV/HLS wheel, components, foreground/background/transparent, swap | `native_tool_and_color_panels`, `native_default_workspace`, shared color tests |
| Navigator, camera drag/rotation/flips, live Diagnostics | `native_navigator`, `native_navigation_tools`, `native_tool_drawers` |
| Connected selection/fill, reference layers, selection-constrained brushes, gradients | `native_connected_tools`, `native_selected_brushes`, `native_gradient_tool`, GPU oracle tests |
| Operation/linked masks, figures, editable/snapping rulers | `native_operation_tool`, `native_figure_tools`, `native_ruler_tools`, engine/GPU tests |
| Connected tool drawers, nested drawers, collapsed-column movement/resize | `native_tool_drawers`, `native_nested_tool_drawers`, `native_collapsed_columns`, `native_collapsed_drop_and_resize` |
| Partial/total Zen, eight menus, document files, workspace restore, native fullscreen | `native_zen_behaviors`, `native_menu_sections`, `native_document_files`, `native_workspace_restore`, `native_fullscreen_header_clock_and_battery` |

All 20 integrated GTK scenarios pass. The approval follow-up passes 300
core/engine/UI unit tests; the renderer suite passes 118 hardware tests
(17 benchmarks separately ignored). The Navigator
contrast correction found during visual inspection has its own failing-before,
passing-after GPU regression. Strict GTK/renderer Clippy, the WebAssembly build
check and the normal GTK release build pass. Updated dark/light review captures
are in ignored `artifacts/familiar-workspace/`; default-layout captures are in
its `default/` subdirectory. Targeted tests also render representative custom
workspaces, which should not be mistaken for the shipped default.

The approved height adjustment increases Navigator's default share of the right
column from approximately 20% to 25%, reduces Layers from 50% to 45%, and retains
Properties at 30%. This gives Navigator about 40px more space in a 900px-tall
window. Shared layout tests cover four window sizes, unchanged left panels and
command ribbon, and the matching reduction in Layers. Existing customized
workspaces are preserved; Window → Reset layout applies the updated defaults.

The follow-up layout places Brush size as the second tab beside Tool (renamed
from Tool Settings).
Tool Set receives the former Brush size panel's height and the removed divider;
Tool and Color retain their heights apart from subpixel split rounding.

Diagnostics places its live graph below GPU timings and Canvas storage below
Frames. Rust owns both row order and chart placement; native hosts project that
ordering rather than assigning a fixed footer position to the chart.

**Separate renderer follow-ups remain open:** residual transform/presentation
stalls and strict Metal/D3D12 filter-pixel parity. GTK design approval does not
resolve these or establish physical stylus latency or new device validation on
other platforms. The corrected native event-wait measurements are below.

## Required layout

- Left edge: Pen, Pencil, Brush, Eraser, Airbrush, Decoration, Blend, Liquify;
  divider; Lasso, Auto select, Fill, Gradient; divider; Operation, Figure,
  Ruler, Hand, Eyedropper, foreground/background color selector.
- Second left column: Tool Set, Tool (Brush size as its second tab),
  then Color, vertically stacked. Tool Set uses the freed Brush size space.
  **Tool Set and Tool must fit three standard tiles inside their minimum
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
- Every actual tool/preset gets two columns: Tool Set, then Tool.
  Color gets one Color panel. Opacity gets Tool, exposing the editable
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
  width. Docked Tool Set/Tool still retain their three-tile minimum. Content
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
  column. Drag-resizing collapses it when the requested width falls below 75%
  of the column's minimum width, or reaches the collapsed strip width (36
  logical pixels), whichever happens first. Exactly 25% into the minimum does
  not trigger the percentage rule. While the pointer stays down, moving back
  outward across that same threshold reverses the collapse and immediately
  resumes normal resizing, clamped to the minimum width. It can collapse again
  on the next inward crossing without first reaching the expanded edge.
  Expansion using the button restores the remembered ordinary width.
- Dragging a collapsed column's edge holds the entire layout fixed until the
  pointer moves 36 logical pixels outward. It then opens at the column's minimum
  width and holds that width until the pointer reaches the expanded edge. The
  same drag can then resize normally or collapse the column again. Releasing
  while waiting keeps the column expanded; cancellation restores the original
  collapsed layout. Expansion and subsequent resizing form one undoable action.
  GTK retains the resize cursor throughout the held gesture, including while
  the pointer is inside the expanded panel.
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
G for Gradient, F for Fill, O for Operation, U for Figure, Shift+U for Ruler, H for Hand and I for
Eyedropper. Repeated family keys cycle its tools in toolbar order; direct custom
bindings remain possible. Keep Space temporary pan and Tab zen. New/Open/Save
use the platform command modifier with N/O/S; standard undo/redo stay unchanged.
This follows [CSP's tool shortcut table](https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Tool_Shortcuts.htm),
not a claim that all drawing applications use identical keys. Fill uses F as in
[Krita](https://scripting.krita.org/action-dictionary); Fit canvas moves to
Ctrl/Cmd+0, preserving explicit custom bindings rather than stealing them.

Tool Set shows only groups belonging to the active tool. Group buttons have an
icon and name, three tiles wide and one tall, with exactly one selected. The list
below shows that group's subtools and retains useful brush stroke previews.
Switching tools remembers their last subtool. The Tool panel comes from shared
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

### Selection and watercolor boundary policy

Selections constrain stored paint and wetness, including conductance-driven
transport. Watercolor's live edge is computed from the combined wet layer,
preserving the earlier requirement that overlaps lose internal edges and the
effect is never baked into pigment. Its outside band is therefore a layer effect,
not a second deposit constrained by each old stroke's selection. Changing or
removing a selection does not change the appearance of existing strokes.

For a strict visible boundary, **Mask: reveal selection** clips the combined
pigment and live edge. The existing layer mask is sufficient; do not add another
persistent per-stroke clipping channel or silently create masks while painting.
This separates temporary painting constraints from final appearance, consistent
with [CSP's separate brush and whole-layer watercolor effects](https://support.clip-studio.com/en-us/faq/articles/20200061)
and [selection-created masks](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm).
These sources support the distinction, not a claim of identical CSP selection
behavior. This resolves the earlier implementation notes' open policy question;
the resulting GTK appearance remains subject to human review.

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
- Hand (H) and Eyedropper (I) are implemented in shared Rust, with GTK Tool Set
  projections and shared SVG icons. Hand uses the existing camera path for
  mouse, pen and single-finger touch, without modifying document pixels.
  Eyedropper remembers its Visible color / Layer color subtool. Visible samples
  include composition opacity, masks and effects; Layer color reads the editing
  layer's raw paint. This follows the useful distinction in
  [CSP's Eyedropper tools](https://help.clip-studio.com/en-us/manual_en/300_color/Eyedropper_Tool.htm).
  The initial tool samples one pixel, not an averaged area. Empty pixels do not
  change the paint color; non-empty samples choose opaque color in the active
  foreground/background slot, returning from transparent paint to its last slot.
- Point sampling copies four bytes from an existing GPU texture to one reusable
  staging buffer. It does not recompose, run a shader or wait on the UI thread.
  One in-flight request and latest-point coalescing bound work during pointer
  movement; generations prevent late samples overriding manual choices or tool
  changes. The renderer handles sparse page coordinates and linear premultiplied
  pixels; UI input resolves camera reflection/rotation and layer offsets.
- Hand/Eyedropper validation: 179 shared UI tests, nine GPU layer correctness
  tests, strict UI/renderer/GTK Clippy, workspace and Wasm checks. Native GTK tests
  exercise both sample sources, shared shortcuts, actual subtool buttons, Hand
  motion and return to an idle frame timer. Dark/light screenshots were inspected
  at `artifacts/familiar-workspace/eyedropper-{Dark,Light}.png` (ignored). Shared
  input tests cover GTK/web/Android profiles; browser/device interaction testing
  of these new tools remains pending GTK review.
- Serial six-second release panning measurements on the private 120Hz Wayland
  display (synthetic input, actual child-surface presentation feedback):

  | Input | CPU worker median/p95/p99 ms | Canvas GPU median/p95/p99 ms | GTK handler median/p95/p99 ms | Displayed Hz |
  | --- | --- | --- | --- | --- |
  | Existing pan | 0.277 / 0.487 / 0.574 | 0.075 / 0.198 / 0.382 | 0.014 / 0.032 / 0.040 | 120.01 |
  | Hand tool | 0.182 / 0.380 / 0.517 | 0.071 / 0.131 / 1.426 | 0.008 / 0.020 / 0.028 | 119.80 |

  Hand discarded one presentation; existing pan discarded none. GPU tails vary,
  so this is approximately 120Hz, not a perfect-frame or physical-input guarantee.
  Reports are `/tmp/capy-hand-{baseline-pacing,pacing}.json`, not repository assets.
  A subsequent 384px G-Pen drawing run sustained 119.57Hz (one discarded frame),
  CPU median/p95/p99 0.277/0.601/0.758ms, canvas GPU 0.137/0.284/0.562ms,
  GTK handler 0.011/0.028/0.041ms. This is within the prior drawing measurements,
  not evidence of a speedup. Report: `/tmp/capy-navigation-tools-drawing.json`.
- Gradient (G) now has four shared subtools: linear/radial, foreground to
  background/transparent. Tool Settings exposes opacity. A two-point drag guide
  appears during input; pigment commits on release, not as a live color preview.
  Escape, cancelled contact and focus loss discard the guide without editing.
  The operation respects selection inversion, target offset, alpha lock and
  ordinary layer clipping/masks. Locked, Paper and mask targets are not painted.
- Gradient and constant Fill share one GPU paint/composite pass per affected
  tile, followed by the existing copy into persistent paint. The engine appends
  an ordered operation without replaying old strokes. Undo/device recovery still
  reconstruct the same stored operation. Allocation/dirty bounds account for
  translated/inverted coverage; small selections do not redraw every tile.
- Gradient validation: 180 shared UI, 24 engine, 21 core and 12 GPU layer tests;
  strict Clippy, workspace/Wasm checks, and real GTK subtool/opacity/input/undo
  tests. GPU checks cover 16 color/transparency/alpha-lock/selection combinations,
  tile seams, translated/inverted masks, queued versus separate submissions and
  full replay. Eight dark/light captures were visually inspected at
  `artifacts/familiar-workspace/gradient-*.png` (ignored); this also caught and
  fixed symbolic CSS on the new shared icon.
- Release renderer measurements on the workstation, 2048×1536, warm 120-sample
  windows; each triplet is median/p95/p99 milliseconds. Completion includes an
  explicit **test-only** GPU wait, not compositor/display latency:

  | Operation | CPU | GPU | Completed |
  | --- | --- | --- | --- |
  | Fill, full canvas | 0.714 / 1.329 / 1.728 | 0.682 / 0.683 / 0.698 | 1.482 / 2.199 / 2.538 |
  | Linear, full canvas | 0.885 / 1.646 / 1.942 | 0.682 / 0.685 / 0.721 | 1.703 / 2.587 / 2.820 |
  | Radial, full canvas | 0.763 / 1.421 / 1.798 | 0.682 / 0.683 / 0.720 | 1.541 / 2.178 / 2.377 |
  | Fill, 64×128 selection | 0.032 / 0.050 / 0.116 | 0.024 / 0.024 / 0.024 | 0.091 / 0.114 / 0.176 |
  | Linear, 64×128 selection | 0.035 / 0.042 / 0.079 | 0.024 / 0.024 / 0.024 | 0.095 / 0.117 / 0.157 |
  | Radial, 64×128 selection | 0.031 / 0.061 / 0.211 | 0.024 / 0.024 / 0.024 | 0.089 / 0.165 / 0.270 |

  First-use full-canvas Fill completion was 21.5ms (46.0ms in the preceding run),
  including cold resource/driver work. Subsequent cold Gradient cases were
  2.4–2.6ms. Warm results meet the frame budget; they do not establish a cold
  120Hz guarantee. GPU arithmetic costs are indistinguishable here; CPU tails
  varied between repetitions, so no CPU speedup is claimed.
- Normal 384px G-Pen drawing after these changes delivered 119.75Hz (one
  discarded presentation) and 119.97Hz (none) in two six-second Wayland runs.
  Worker CPU median/p95/p99 was 0.274/0.614/0.783ms, then
  0.282/0.535/0.734ms. GPU was 0.135/0.316/2.183ms, then
  0.127/0.238/0.369ms. The first GPU tail was higher than the earlier baseline;
  it did not repeat. GTK input p99 was 0.040/0.039ms. Reports are
  `/tmp/capy-gradient-drawing{,-repeat}.json`; synthetic input and compositor
  feedback establish approximate 120Hz delivery, not physical pen latency.
- Selection-constrained brush painting now uses immutable, layer-local geometry
  captured at contact start. The same coverage reaches persistent paint, mask
  painting, predicted/private previews and replay; later selection changes do
  not rewrite old strokes. Zero-offset snapshots share the existing contours.
  GPU tests verify all 13 brush families, including unchanged pigment and wetness
  outside the selection during wet-on-wet transport. Selection edits do not
  clip non-destructive layer effects: watercolor's outside edge band still may
  appear beyond the painted boundary, just as image filters can. That is not
  deposited pigment and is not baked into strokes. See the boundary policy above
  for strict final-appearance masking and its validation.
- Selection rasterization is GPU-only: mark four-sample scanline crossings,
  then prefix-XOR/popcount the interiors into a reusable packed buffer. Holes,
  self-crossings, off-canvas geometry, inversion, fractional coordinates and
  multiple scan blocks/tile boundaries match the mask renderer. Both renderers
  share canonical edge intersection math, fixing rounding differences at exact
  sampled vertices. Encoder-ordered uploads preserve multiple selections queued
  in one replay. No pixel readback, wait, or per-frame selection allocation.
- Storage is half a byte/pixel of the selection bounding rectangle, plus a
  32-byte header, row padding and allocation alignment. Full 2048×1536 coverage
  needs about 1.5 MiB (12.5% of one RGBA8 image), not another texture binding:
  material brushes remain within the portable 16 sampled-texture limit. The
  buffer grows when needed and is reused across strokes, camera changes and
  replay with unchanged geometry; changing document extent discards it.
- Selection initialization benchmark, 2048×1536, near-full-canvas polygons:
  prepare plus GPU completion median/max was 0.063/0.079ms at four vertices,
  0.057/0.072ms at 256, and 0.083/0.750ms at 4096. These are ten warm samples,
  not percentile/first-use claims. The initial per-pixel edge-search prototype
  took 1.759ms median at 256 vertices; it was replaced, not retained as another
  runtime path. Benchmark: `selection_raster_latency` (release, ignored test).
- Selected-brush benchmark, 384px diameter, eight incremental dabs per submit,
  2048×1536, warm 120-sample windows. Triplets are median/p95/p99 milliseconds;
  completion includes the benchmark's explicit GPU wait, not display latency:

  | Brush | Selection | CPU | GPU | Completed |
  | --- | --- | --- | --- | --- |
  | G-Pen | None | 0.034 / 0.041 / 0.067 | 0.020 / 0.020 / 0.020 | 0.087 / 0.094 / 0.119 |
  | G-Pen | Active | 0.034 / 0.035 / 0.040 | 0.020 / 0.020 / 0.021 | 0.087 / 0.090 / 0.094 |
  | Natural Blender | None | 0.068 / 0.124 / 0.244 | 0.078 / 0.081 / 0.082 | 0.186 / 0.275 / 0.365 |
  | Natural Blender | Active | 0.063 / 0.144 / 0.216 | 0.085 / 0.085 / 0.086 | 0.184 / 0.266 / 0.345 |
  | Watercolor Wash | None | 0.362 / 0.431 / 0.666 | 0.415 / 0.416 / 0.417 | 0.842 / 0.936 / 1.145 |
  | Watercolor Wash | Active | 0.365 / 0.456 / 0.711 | 0.462 / 0.463 / 0.463 | 0.903 / 1.031 / 1.236 |

  Watercolor has a measurable ~0.047ms median GPU increase from restricting
  deposition, backtraces and neighboring transport samples. This is not free,
  but remains comfortably within 8.33ms. CPU tails vary between runs; do not
  interpret their decreases as a selection speedup. `selected_brush_latency`
  asserts that unchanged coverage is not regenerated between strokes.
- Six-second GTK/Wayland G-Pen runs delivered 119.93Hz without selection and
  119.95Hz with a 256-point selection; the latter discarded one presentation.
  Selected worker CPU median/p95/p99 was 0.281/0.525/0.717ms; GPU
  0.152/0.192/0.346ms; input processing p99 0.041ms. Reports:
  `/tmp/capy-selection-{0,1}.json`. Use `LAYER_PACING_SELECTION=1` with
  `native_frame_pacing` to repeat. These synthetic events plus compositor
  feedback establish approximate 120Hz delivery, not physical tablet latency.
- Validation: 21 core, 25 engine, 180 shared UI and 69 GPU tests passed; strict
  Clippy and workspace/Wasm compilation passed. GTK selected/inverted brush
  input, deselect and undo/redo passed; all four dark/light captures in
  `artifacts/familiar-workspace/{selected-brushes,selection-inverted-replayed}-*.png`
  were visually inspected. The rebuilt static web package passed `--selection`
  (actual WebGPU drawing and replay) and `--gpu-compatibility` (strict non-null
  layouts plus startup-failure recovery). No physical Android/iPad selection
  validation is claimed; new tool UI rollout still awaits GTK review.
- Auto Select / bucket Fill now use the GPU connected-region primitive
  (`flood.rs` / `flood.wgsl`) through shared input/settings and GTK. Its pipelines
  and scratch allocate on the first request, not at application startup. Three
  dispatches classify and join pixels within 16×16 blocks, merge connections
  across block boundaries, then pack the seed component and reduce its bounds.
  This is original WGSL, informed by [GPU connected-component labeling research](https://federicobolelli.it/media/publications/pdfs/2024tpds.pdf).
  No external shader implementation was imported. Four-connected neighbors
  avoid leaks through diagonal corners; tolerance compares every pixel to the
  fixed seed in premultiplied sRGB including alpha, ignoring hidden transparent
  RGB. It does not drift gradually across a color ramp.
- Region validation compares every output pixel and bounds against an independent
  test-only CPU flood oracle for 108 size/pattern/seed combinations: solid,
  checkerboard, isolated cells, one-pixel serpentine corridors, noise and a wall
  with a gap. Additional tests cover tolerance, transparency, invalid requests,
  and several differently sized requests queued together while reusing scratch.
  Runtime rasterization remains GPU-only; the oracle is not a CPU fallback.
- Region storage: reusable parents cost four bytes per source pixel; immutable
  output costs half a byte per pixel plus a 32-byte header and row/alignment
  padding, with a separate 32-byte bounds/count summary. At 2048×1536 this is
  12 MiB scratch plus approximately 1.5 MiB per retained result. Encoding has no
  pixel readback or wait. The benchmark allocates a result and bindings per
  **request**, not per drawing frame; the parent buffer is reused. This does not
  account for source composition, history persistence or selection display;
  complete-request costs are measured separately below.
- Isolated release benchmark, 2048×1536, last 120 of 150 samples on the test
  workstation; each triplet is median/p95/p99 milliseconds. Completion includes
  an explicit **test-only** queue wait. Pipeline creation and source preparation
  occur before timing; these are primitive costs, not end-to-end tool latency:

  | Region source | CPU | GPU | Completed |
  | --- | --- | --- | --- |
  | Solid | 0.017 / 0.022 / 0.187 | 0.265 / 0.290 / 0.292 | 0.339 / 0.368 / 0.490 |
  | Line-art cells | 0.017 / 0.018 / 0.023 | 0.149 / 0.151 / 0.153 | 0.204 / 0.209 / 0.328 |
  | One-pixel maze | 0.017 / 0.021 / 0.067 | 0.268 / 0.335 / 0.421 | 0.328 / 0.418 / 0.490 |
  | Noise | 0.016 / 0.051 / 0.150 | 0.081 / 0.083 / 0.084 | 0.137 / 0.177 / 0.275 |

  First Solid completion was 5.139ms, versus 0.35–0.45ms for the subsequent
  fixtures. No claim of cold-start or tablet latency is implied. Repeat with
  `cargo test -p layer-render-wgpu --release connected_region_latency --lib -- --ignored --nocapture --test-threads=1`.
- Region integration uses immutable packed coverage shared with brush clipping,
  mask initialization, fill/gradient operations and a GPU selection outline in
  the existing presentation pass. Translation shares the coverage allocation;
  no CPU polygon tracing or flood rasterizer is involved. Explicit region clicks
  asynchronously retain one packed CPU history copy; normal drawing/panning
  never downloads it. Undo/replay use that fixed result, not changed source art.
  Live GPU buffers are reused across consumers; device recreation uploads the
  saved coverage once. Masks no longer have a separate per-pixel contour search.
- Auto Select and Fill expose Visible artwork, Editing layer and Reference
  layers, plus shared tolerance (Fill also exposes opacity). References use a
  separate instance of the ordinary GPU scene compositor, not a different blend
  algorithm or the visible scene's cached checkpoints. Original indices and
  ancestor transforms/masks are preserved; marked groups include descendants.
  A clipping stack is treated as one reference object; referenced adjustments
  include the underlying siblings they adjust. Unrelated artwork is excluded.
  Marking only a child does not include its unrelated siblings. Hidden objects
  remain hidden. With no marked references, a concise message explains how to
  mark one instead of silently filling the whole canvas.
  [CSP's reference-layer workflow](https://help.clip-studio.com/en-us/manual_en/180_layers/Reference_layers.htm)
  motivates separating the line-art source from the layer receiving color.
  Source-layer offsets are converted exactly once; pending operations snapshot
  paint parameters. Generation/document checks discard stale replies after tool
  changes, edits or cancellation. Replies schedule the frame displaying the edit.
  Empty fills do not add undo entries. Fill on locked, Paper or mask targets is
  rejected; mask painting remains supported by the ordinary brushes.
- Complete request benchmark, 2048×1536 line-art cells, last 120 of 150 samples:

  | Source | CPU median/p95/p99 ms | GPU median/p95/p99 ms | Complete, including history ms |
  | --- | --- | --- | --- |
  | Visible artwork | 0.022 / 0.043 / 0.179 | 0.200 / 0.203 / 0.206 | 0.558 / 0.661 / 0.794 |
  | Raw editing layer | 0.076 / 0.118 / 0.123 | 0.352 / 0.356 / 0.356 | 0.762 / 0.981 / 1.022 |
  | References | 0.323 / 0.451 / 0.882 | 0.518 / 0.522 / 0.523 | 1.235 / 1.445 / 1.796 |

  First requests were 6.350/1.044/2.969ms respectively; only the first includes
  detector pipeline creation, so these are not three independent cold starts.
  GPU timing includes source capture and copies; completion adds asynchronous
  mapping and history validation. These are workstation results, not tablet
  measurements. Repeat with the ignored release `region_request_latency` test.
  Reusable query storage is 13.5MiB for visible sampling and approximately
  25.8MiB after allocating the reference source/composition scratch. Each retained
  2048×1536 region costs 1.5MiB GPU plus 1.5MiB CPU history, with up to another
  1.5MiB reusable brush-coverage buffer. Diagnostics accounts for these resources.
- Real GTK input/reference/fill/undo/redo tests pass in both themes. Captures
  `region-{selection,fill}-{Dark,Light}.png` in the ignored
  `artifacts/familiar-workspace/` directory were visually inspected. GPU tests
  verify shared coverage across painting/masks/fill, inversion/translation,
  frozen history, selection limits, reference group masks and clipped multipass
  filters, and unchanged visible composition after sampling. Shared UI tests
  cover all three host profiles; native browser/Android interaction awaits rollout.
- Paired six-second 384px G-Pen runs on private Wayland: no selection 119.97Hz,
  raster selection 119.90Hz (one discarded presentation). Selected worker CPU
  median/p95/p99 was 0.248/0.553/0.699ms; GPU 0.161/0.277/1.000ms; GTK handler
  0.007/0.035/0.048ms. No-selection GPU was 0.128/0.265/0.413ms: the outline is
  not free, but the measured drawing path remains within the 8.33ms target.
  Reports: `/tmp/capy-region-{baseline-pacing,pacing}.json`.
  Use `LAYER_PACING_SELECTION=pixels` with `native_frame_pacing` to repeat.
  This is synthetic input with actual presentation feedback, not physical input
  latency. Workspace and Wasm compilation also pass.
- The exact staged milestone, isolated from concurrent startup changes, passes
  24 core, 25 engine, 181 shared UI and 75 GPU correctness tests (11 separate
  hardware benchmarks ignored). Strict all-target Clippy passes for the core,
  engine, UI, renderer and GTK. No native Android/browser region-input validation
  is claimed from shared-schema compilation alone.
- Region follow-ups before the final complete tool review:
  Gap closing, edge expansion and antialiasing need explicit follow-up rather
  than being silently conflated with color tolerance. Affine raster-selection
  resampling is now implemented, as recorded below. Watercolor's display-only outer edge still needs
  its final selection-boundary policy, as noted above.
- Still to implement: the new default layout, missing canvas tools/commands,
  collapsible columns, the full application menus, remaining shortcuts
  and full functional/performance validation.
- The complete eight-tool ribbon is currently exercised by the GTK review test;
  the shipped default toolbar/layout will change when its remaining tools and
  commands are implemented. Tool-family presentation on other hosts awaits GTK
  review, while core models and Wasm remain compatible.
- Implementation in progress; the requested default workspace is not yet shipped.

### Figure tools

- Figure (`U`) now has Line, Rectangle and Ellipse groups. Line offers Outline;
  the other groups offer Outline, Fill and Outline + fill. Shared Tool Settings
  exposes Line width and Opacity, hiding width for Fill. Single-color modes use
  the selected paint color; Outline + fill uses foreground for the outline and
  background for the interior. The transparent color slot erases instead.
- Shift constrains lines to 45° increments and rectangles/ellipses to equal
  sides. Releasing Shift restores the unconstrained endpoint; Escape, canceled
  contacts and focus loss discard the guide without adding history. The group
  and mode survive switching to another tool and back. The shared toolbar picker
  includes Figure, including the normal Tool Set/Tool Settings drawer behavior.
- These initial modes follow [CSP's Figure controls](https://help.clip-studio.com/en-us/manual_en/810_subtools/F.htm)
  and the Shift convention described for [Krita's ellipse tool](https://docs.krita.org/en/reference_manual/tools/ellipse.html).
  They are raster operations, not editable vector objects. During a drag the
  artist sees a rubber-band outline; release commits one operation, not repeated
  copies of a filled shape. Filled live preview is not claimed here.
- `layer-core::Figure` owns immutable geometry/colors and conservative bounds.
  The shared UI owns tool choice, constraints, local coordinates and settings.
  GTK uses existing generic group buttons and numerical controls; new original
  SVGs live in the shared bank. No GTK-only figure behavior or CPU pixel rasterizer.
- The existing GPU paint-operation pass handles figures alongside fills and
  gradients, including coverage, alpha lock, clipping and masks. No new pipeline,
  intermediate texture type or CPU readback is introduced. Only intersecting
  tiles are allocated/updated. Ellipse outlines use a bounded closest-point
  solve so stroke width is measured in image pixels, even on elongated ellipses.
  Inner and outer coverage are partitioned, avoiding double opacity and excessive
  subpixel outline weight. History replay uses exactly the same GPU operation.
- Immutable scene layouts/pipelines are now initialized once per device and
  shared by live composition, explicit captures and recreated scenes. Their
  mutable uniforms, scratch and image caches remain separate. A GPU handle test
  verifies reuse without retaining canvas pixels. Previously, a first figure,
  fill or source capture could compile the same scene pipelines again.
- Scene composition eligibility is computed once per frame from live layer
  properties and **current** paint operations, not all retained undo history.
  Applied fills, gradients, figures and masks already reside in paint pages;
  subsequent ordinary strokes use the ordinary direct composition/preview path.
  Active operations still use scene jobs, including watercolor appearance baking
  before Apply Mask. Live masks, groups, blends, offsets and filters retain their
  required scene semantics. Regression tests compare imported/baked pigment with
  and without retained history, at full/partial layer opacity, using G-Pen,
  Natural Blender and Watercolor previews, cancellation and committed strokes.
- Validation includes an independent dense-geometry pixel oracle, all seven
  modes, 0.25–60px outlines, highly elongated ellipses, clipping/masks/alpha lock,
  erasing, transformed layers, inverted coverage crossing tile boundaries,
  incremental versus replay equality, sparse allocation and idle stability.
  Shared UI tests exercise settings, modifiers, cancellation and undo/redo.
  GTK native buttons and expression inputs render all modes in both themes;
  the GPU eyedropper verifies actual pigment. Visually reviewed captures are
  `artifacts/familiar-workspace/figures-{Dark,Light}.png` and `figure-guide.png`.
  These are ignored review artifacts, not production assets.
- Figure's 2048×1536 release benchmark measures one committed operation
  per frame, growing history to 160 operations; last 120 samples reported:

  | Case | CPU median/p95/p99 ms | GPU median/p95/p99 ms | Completed median/p95/p99 ms |
  | --- | --- | --- | --- |
  | 96×64 rectangle | 0.030 / 0.037 / 0.237 | 0.023 / 0.023 / 0.023 | 0.085 / 0.094 / 0.300 |
  | 1920×1400 rectangle | 0.721 / 1.030 / 1.460 | 0.696 / 0.697 / 0.708 | 1.505 / 1.820 / 2.344 |
  | 1920×1400 ellipse | 0.829 / 1.179 / 2.164 | 0.757 / 0.758 / 0.763 | 1.689 / 2.055 / 2.449 |
  | 1920×10 ellipse | 0.152 / 0.209 / 0.334 | 0.135 / 0.135 / 0.142 | 0.333 / 0.423 / 0.521 |
  | Diagonal line across 1920×1400 | 0.754 / 1.040 / 1.521 | 0.690 / 0.691 / 0.714 | 1.558 / 1.853 / 2.315 |

  This includes the GPU operation and canvas composition, with a test-only wait
  to measure completed work. Production has no such wait. It is not physical
  pen-to-display latency. The two-color rectangle/ellipse and line use 24px
  width. Repeat with the ignored `figure_latency` renderer test in release mode.
  The blank canvas is presented before timing, matching the app lifecycle;
  operation-specific pages/scratch are not preallocated. First small rectangle
  completed in 3.076ms, and the first large rectangle in 4.420ms. These share a
  device and are not independent cold starts. The existing full-canvas Fill
  benchmark still has an 11.564ms first-operation spike (7.425ms in the paired
  pre-Figure baseline run); its final steady p99 was 3.123ms. This cold resource
  cost remains a follow-up, not evidence that all first interactions fit 8.33ms.
  Full-canvas paint-operation GPU medians changed from approximately 0.682ms
  before Figure to 0.693ms; CPU tails varied between runs, so no zero-regression
  claim is made. Eight-dab 384px brush GPU medians remain 0.020ms G-Pen,
  0.078ms Natural Blender and 0.415ms Watercolor without selection (0.020,
  0.085 and 0.464ms with selection). These are operation timings, not display Hz.
- Actual six-second Wayland drawing over a committed large figure, 384px G-Pen:
  before the history eligibility fix, worker CPU median/p95/p99 was
  0.612/1.054/1.230ms and GPU 0.475/1.015/1.997ms. Afterward they are
  0.311/0.599/0.762ms and 0.134/0.266/0.539ms, at 119.96Hz with no discarded
  presentations. The matched blank-layer run was 119.92Hz, CPU
  0.258/0.543/0.732ms, GPU 0.131/0.595/2.155ms; short-run tails are noisy.
  GTK frame-handler p99 over the figure is 0.056ms and input-processing p99
  0.047ms. Reports are `/tmp/capy-figure-fixed-{plain,painted}.json`; use
  `LAYER_PACING_FIGURE=1` with `native_frame_pacing`. Input is synthetic; this
  measures real GPU/presentation feedback, not physical pen-to-display latency.
- The same figure-backed Wayland test with 384px Natural Blender presents at
  119.54Hz (one discarded presentation); CPU median/p95/p99 is
  0.642/1.120/1.448ms, GPU 0.643/1.332/1.694ms. Watercolor presents at 119.27Hz
  (two discarded), CPU 1.313/2.121/2.618ms and GPU 1.533/2.672/4.231ms. These
  sustain approximately 120Hz but do not prove an absence of occasional missed
  frames. Reports: `/tmp/capy-figure-fixed-{blender,watercolor}.json`.
- The isolated staged source passes 27 core, 25 engine, 182 shared UI and 80 GPU
  correctness tests (12 hardware benchmarks are separate). Strict all-target
  Clippy, workspace compilation and Wasm compilation pass. Native Figure buttons,
  settings, Shift constraints, undo/redo and pixel sampling pass on GTK in both
  themes. This does not claim native web/Android Figure input validation.

### Ruler tools

- Ruler (`Shift+U`, alongside Figure's `U`) offers Straight, Parallel and Radial.
  Drag to create straight/parallel guides; tap or drag to place a radial center.
  The same tool moves guide bodies and edits their endpoint handles. Shift snaps
  endpoints to 45° increments immediately, including without pointer movement.
  Escape or canceled contact discards the provisional guide. Release records one
  document edit; guide deletion, movement and creation share document undo/redo.
  Operation-tool integration is now implemented in the controller milestone below.
  Project serialization remains in the file milestone; guides currently persist
  within the open document.
- Show rulers and Snap to rulers appear in View and in shared Tool Settings.
  Hiding guides disables snapping without forgetting the snap preference. Delete
  ruler is enabled only for a selected guide. GTK projects generic command rows
  as native checkboxes/buttons; command labels, state, availability and shortcuts
  stay in Rust. Rulers are available in the toolbar picker and tool drawers.
- Choose a guide once at stroke Down: nearby straight guides take precedence
  within 12 logical pixels; otherwise the closest parallel/radial anchor wins.
  Parallel strokes keep their own starting offset. Radial strokes use the ray
  through their initial point; a stroke starting exactly at the center waits for
  its first real movement to choose the ray. Predicted input cannot change this
  durable direction. Moving a guide cannot alter already committed ink.
- Projection composes with the existing input affine transform before pressure
  processing, prediction and dab generation. Real and predicted samples use the
  same constraint; replay uses stored points, never current rulers. The brush
  outline follows the snapped contact while its optional crosshair remains at
  the physical pointer. Layer offsets are removed during cursor dynamics and
  restored for display, fixing moved-layer outline calculations as well.
- Guides use the existing GPU presentation overlay, not document pixels. No new
  texture, paint pass or readback is needed. Guide-only edits and undo/redo do
  not trigger paint replay or recomposition. Pointer movement changes only the
  guide preview, not panel models/history; the selected guide is a small immutable
  stroke snapshot. The document accepts up to 1,024 validated guides.
- Validation on the isolated staged source: 29 core, 27 engine, 183 shared UI
  and 80 GPU correctness tests pass (12 hardware benchmarks run separately),
  including all three host profiles for guide interactions and tests
  of rotated/high-DPI views, moved layers, predictions, cancellation and history.
  Strict all-target Clippy, workspace and Wasm compilation pass. GTK's actual controls,
  snapped GPU ink sampling, hidden guides, deletion and undo pass. Dark/light
  captures were visually inspected; GTK-specific SVG stroke classes corrected
  missing/filled guide icons. Review images:
  `artifacts/familiar-workspace/rulers-{Dark,Light}.png` and `rulers-hidden.png`.
  This is GTK review coverage, not physical tablet or native web/Android testing.
- Six-second 384px G-Pen tests on the private 120Hz Wayland compositor:

  | Guides | Worker CPU median/p95/p99 ms | GPU median/p95/p99 ms | Displayed Hz | Discarded |
  | --- | --- | --- | --- | --- |
  | None | 0.292 / 0.559 / 0.729 | 0.122 / 0.302 / 1.082 | 119.91 | 0 |
  | Visible, snapping off | 0.288 / 0.584 / 0.761 | 0.124 / 0.286 / 0.448 | 119.72 | 1 |
  | Visible, snapping on | 0.294 / 0.614 / 0.757 | 0.129 / 0.336 / 0.462 | 119.91 | 0 |

  Guide display adds about 0.002ms to GPU median in these runs. Snapping changes
  the stroke's geometry and dab count, so its GPU times are not an identical-image
  comparison. Input processing p99 is 0.040/0.045/0.046ms respectively; GTK frame
  handlers are 0.047/0.053/0.054ms. CPU tails differ by tens of microseconds, not
  milliseconds. This supports approximately 120Hz delivery but not zero missed
  frames or physical pen-to-display latency. Reproduce with `native_frame_pacing`,
  `LAYER_PACING_BRUSH=GPen` and optional `LAYER_PACING_RULER=visible` or `snap`.
  Reports: `/tmp/capy-ruler-{none,visible,snap}.json`.

### Operation / transform foundation (not yet exposed)

- Shared affine geometry, a GPU cut-and-place primitive and ordered paint-layer
  history integration are implemented.
  The intended interaction follows [CSP's transform controls](https://help.clip-studio.com/en-us/manual_en/360_transform/Transform_using_the_Tool_Settings_palette.htm)
  and [Krita's transform handles](https://docs.krita.org/en/reference_manual/tools/transform.html):
  a persistent transform box, move/scale/rotate, numeric values, and explicit
  apply/cancel. These editor controls are **not implemented yet**. No inert
  Operation button has been added.
- `ImageTransform` holds a document-space affine matrix and nearest/linear
  interpolation. CPU work is geometry, validation and parameter packing only.
  `PixelTransform` consumes an immutable premultiplied source texture, optional
  existing packed selection coverage, and a batch of output regions. Each
  preview resamples the original, never the previous preview. Source and output
  must be separate resources; a cropped source must include all original content
  needed as the unselected backdrop, not just the selected pixels.
- Selection coverage weights each source texel before interpolation. This
  prevents unselected colors bleeding into transformed edges. Out-of-source
  samples are transparent, including extreme translations. Exact identity
  returns the original unchanged. Other transforms use ordinary cut then
  source-over: partially selected overlap can change fractional alpha; the
  identity fast path does not claim to solve that general compositing tradeoff.
- A single uniform upload assigns distinct dynamic offsets to all output
  regions; source bindings and parameter storage are retained. Only supplied
  scissor regions are written. Full-image and tiled output match byte-for-byte.
  Multiple encodes in one submission use distinct uniform ranges. Begin a new
  arena frame only after submitting the previous frame.
  There are no production waits/readbacks, per-frame textures or per-tile
  bind-group creations in this primitive.
- At 2048×1536 an uncropped immutable RGBA8 source costs 12MiB. A separate full
  preview image would cost another 12MiB unless existing output pages are reused;
  optional full-image packed selection coverage costs about 1.5MiB. The primitive
  itself owns neither image. Measured retained GPU parameter storage is 304B for
  one target and 16,432B for 48 targets, with 12KiB CPU packing for the latter.
  Bind-group/pipeline driver overhead is not included in these byte counts.
- Workstation release microbenchmark, 120 measured updates after 40 warmups,
  2048×1536 RGBA8. Matrices change every update except the copy baseline:

  | Work | CPU encode/submit median/p95/p99 ms | GPU median/p95/p99 ms | Completed median/p95/p99 ms |
  | --- | --- | --- | --- |
  | Identity copy baseline | 0.013 / 0.017 / 0.859 | 0.011 / 0.012 / 0.013 | 0.055 / 0.066 / 0.916 |
  | Whole-layer transform | 0.018 / 0.020 / 0.042 | 0.030 / 0.030 / 0.031 | 0.080 / 0.085 / 0.111 |
  | Selected transform | 0.018 / 0.025 / 0.125 | 0.042 / 0.043 / 0.043 | 0.093 / 0.120 / 0.291 |
  | Selected, 48 output tiles | 0.172 / 0.276 / 0.545 | 0.100 / 0.102 / 0.103 | 0.318 / 0.436 / 0.720 |
  | Selected, one 256px region | 0.013 / 0.014 / 0.019 | 0.005 / 0.006 / 0.006 | 0.049 / 0.052 / 0.059 |

  These measure the primitive, not source capture, composition, GTK input or
  presentation. Completed latency includes a test-only GPU wait. First pipeline
  creation took 33.1ms: compile on the renderer worker before live interaction,
  never inside pointer processing. A preliminary build including unrelated
  concurrent startup changes had a 15.464ms copy-baseline completion p99; the
  table is from an isolated source snapshot. That difference is not attributed
  to a transform optimization. Reproduce with the ignored release renderer test
  `pixel_transform::tests::transform_latency` on an otherwise idle GPU.
  A repeat gave selected full-layer GPU p99 0.045ms and 48-tile GPU p99 0.102ms,
  with completion p99 0.258/0.647ms respectively; cached pipeline creation was
  1.008ms. Both runs fit the primitive's warm budget, not a full editor frame.
- Correctness tests compare 48 combinations of interpolation, transforms and
  coverage against an independent double-precision pixel oracle, including
  fractional/inverted selection, alpha, cropped/offset source, flips, rotation,
  enlargement, reduction and extreme translation. Additional tests verify tile
  seams, untouched scissor pixels, immutable input, invalid-input rejection and
  retained allocation reuse.
  The isolated build passes 31 core, 27 engine, 183 shared-UI and 86 GPU
  correctness tests, plus strict all-target Clippy for core/renderer and
  workspace/Wasm compilation. Hardware benchmarks are separate from these totals.
- `LayerOperationKind::Transform` now bakes through the ordinary ordered paint
  history. Appending a transform does not replay earlier paint; undo/redo and
  device-loss replay preserve operation order. Later strokes return to the
  ordinary brush path, without retaining scene jobs merely because history
  contains a transform. Multiple transforms can execute in one GPU submission.
- The renderer captures unmodified pigment and any persistent material/watercolor
  wetness into reusable GPU textures, then transforms them with the same WGSL
  resampling kernel. R8 wetness uses maximum on overlap, not color source-over,
  so placement does not introduce extra water. Stroke-local coverage is reset;
  watercolor edge styling remains live and is not baked into pigment. Layer
  masks continue to compose normally; transforming a linked mask itself remains
  pending. Tests cover nonzero wetness, round trips, later wet strokes, fractional
  coverage and interpolation, and multiple-operation replay.
- Cut and placement bounds remain separate when allocating sparse destination
  tiles. Moving across a large gap does not allocate the intervening tiles.
  Linear interpolation expands support in source coordinates before mapping,
  including scaled edges. Composition still receives a conservative union.
  Packed selection coverage is reused directly, without duplicate R8 mask pages.
  Tests compare incremental output against full rebuilding through a group,
  mask, unclipped Gaussian filter and clipped Gaussian filter, including layer
  translation and tile boundaries.
- Committed-operation benchmark at 2048×1536, 120 measured frames after 40
  warmups. This includes source capture, transform and composition, **not** GTK
  input/presentation or the pending interactive preview lifecycle:

  | Artwork / selection | CPU submit median/p95/p99 ms | GPU median/p95/p99 ms | Completed median/p95/p99 ms |
  | --- | --- | --- | --- |
  | Ordinary paint / whole layer | 0.267 / 0.301 / 1.288 | 0.308 / 0.309 / 0.310 | 0.645 / 0.696 / 1.683 |
  | Ordinary paint / selected | 0.318 / 0.529 / 0.745 | 0.336 / 0.337 / 0.337 | 0.716 / 0.939 / 1.183 |
  | Wet round / selected | 0.741 / 0.975 / 1.389 | 0.627 / 0.629 / 0.637 | 1.457 / 1.799 / 2.134 |
  | Watercolor / selected | 0.888 / 1.034 / 1.337 | 0.649 / 0.653 / 0.663 | 1.645 / 1.819 / 2.117 |

  Capture and uniform storage stabilize at approximately 12.02/15.03/18.03MiB
  for ordinary/wet/watercolor paint respectively, excluding ordinary paint
  pages, composition, selection storage and driver overhead. Only channels
  present in the layer are captured. Source captures are cropped to allocated
  page bounds and reject extents beyond the device's texture limit before
  allocation. Captures are refreshed per committed operation; live previews
  must retain an immutable transaction source instead. First-operation times
  in this cached-driver run were 0.85–3.56ms; this does not replace the cold
  compilation warning above. Reproduce with ignored release test
  `layer_tests::transforms::ordered_transform_latency`, serially on an idle GPU.
- Still required before exposing Operation: linked-mask transforms,
  shared handles and numeric settings, GTK rendering/input,
  and end-to-end latency/visual tests. Existing painting is unchanged; this is a
  tested rendering foundation, **not completion of the Operation milestone**.

### Live transform renderer milestone

- `CanvasEngine` now accepts a disposable, absolute transform request with a
  transaction identity, paint-layer target and immutable selection. It does not
  change history. Document edits, undo/redo and new paint contacts cancel it.
  GTK forwards changed requests to its GPU owner; browser and native-host
  adapters forward the same contract. These are renderer/engine APIs, not yet
  an exposed Operation tool or completed GTK interaction.
- The existing ordered-transform implementation now separates source capture
  from rendering. It captures pigment, present wetness channels and packed
  selection once, retaining source bindings. Every update samples that original,
  not the previous preview. The old and new footprints are redrawn through the
  same shader; unrelated tiles remain unchanged. Unchanged previews and camera
  updates do not recapture, rasterize or recompose the document.
- Cancel uses the shader's exact identity path and removes pages created only
  for the preview. Obsolete preview-only pages are also pruned while dragging.
  A matching committed operation keeps the already-rendered result without an
  extra capture/resample. Other edits restore the original before executing.
  Deleting the target safely discards the preview. Layer IDs and damage feed
  the existing filter dependency cache, including clipped filters and groups.
- Pixel tests compare live output with independently restarted committed
  operations across translation, scale/rotation, fractional selection, identity,
  off-canvas moves and tile crossings. They verify exact cancel of pigment and
  wetness, Apply without a visual jump or recapture, unchanged-frame reuse,
  filtered/masked composition, and survival when another operation reuses the
  general selection buffer. No extra CPU canvas raster path exists.
- Warm 2048×1536 live benchmark, 40 warmups and 120 measured absolute updates;
  source capture occurs once and its count stays constant. Includes transform
  and composition, not GTK event/presentation latency:

  | Source | CPU submit median/p95/p99 ms | GPU median/p95/p99 ms | Completed median/p95/p99 ms |
  | --- | --- | --- | --- |
  | Ordinary / whole layer | 0.184 / 0.189 / 1.056 | 0.120 / 0.121 / 0.121 | 0.351 / 0.357 / 1.233 |
  | Ordinary / selected | 0.186 / 0.260 / 0.475 | 0.109 / 0.110 / 0.110 | 0.344 / 0.424 / 0.662 |
  | Wet round / selected | 0.336 / 0.587 / 0.797 | 0.205 / 0.209 / 0.210 | 0.604 / 0.857 / 1.075 |
  | Watercolor / selected | 0.467 / 0.552 / 0.645 | 0.230 / 0.231 / 0.232 | 0.773 / 0.863 / 0.961 |

  Retained capture/parameter storage is 12.02/12.77/15.79/18.79MiB respectively,
  excluding ordinary paint pages and driver overhead. Selected transactions
  retain an additional packed snapshot (about 0.76MiB here), so mask/selection
  work cannot mutate their input. This also adds a small capture-time copy to
  selected committed transforms. Their repeat GPU p99 is 0.342/0.638/0.664ms
  for ordinary/wet/watercolor, versus 0.337/0.637/0.663ms in the previous run;
  completed p99 remains below 2.02ms. These small differences include run-to-run
  variation, not a claim of zero cost. First live updates in this cached-driver
  run take 1.49–3.05ms; the earlier cold-pipeline warning still applies.
- Validation: 28 engine and 183 shared UI tests; 94 GPU suite tests plus the
  target-deletion regression; workspace/Wasm checks and strict renderer/engine/
  GTK/native-host Clippy. This does not establish platform UI readiness. Still
  pending: controller/handles/numeric controls, linked-mask transforms,
  pipeline readiness before interaction, GTK visual/end-to-end checks,
  remaining menus/file workflows, collapsible columns, final default layout and
  human approval. The full workspace goal remains active.

### Affine selections and atomic transform application

- Selection placement is now one affine transform, replacing translation-only
  metadata. Contours and packed raster coverage remain immutable and shared
  across previews and history. Layer offsets compose in that same coordinate
  model; invalid/singular transforms are rejected.
- Contours reuse the existing GPU scanline coverage initializer with transformed
  geometry. Raster selections retain the translation-only copy path; rotation,
  scale and reflection use one bounded WGSL bilinear resampling pass. Preparation
  writes the existing packed four-sample coverage format, so every brush/mask/fill
  consumer keeps its original constant-cost lookup. Changing consumers or drawing
  with an unchanged selection does not regenerate coverage. The new shader uses
  the shared four-stage compilation scheduler.
- Raster-selection outlines sample the original GPU coverage through its inverse
  placement at presentation time. Moving the outline needs only camera uniforms,
  not a coverage image, readback, tracing or another GPU preparation. Contour
  overlays consume the same displayed selection from the engine. An active pixel
  transform moves that displayed selection provisionally; Cancel leaves the
  document unchanged. Apply records pixels and selection together as one undo
  entry, with no extra rasterization of a matching live result. This includes
  inverted selections; identity Apply does not add history.
- Serial 2048×1536 preparation benchmark, 40 warmups and 120 samples:

  | Placement | CPU median/p95/p99 ms | GPU median/p95/p99 ms | Completed median/p95/p99 ms |
  | --- | --- | --- | --- |
  | Changing translation | 0.011 / 0.014 / 1.021 | 0.011 / 0.012 / 0.012 | 0.055 / 0.064 / 1.148 |
  | Changing rotation/scale | 0.014 / 0.015 / 0.022 | 0.051 / 0.054 / 0.054 | 0.099 / 0.103 / 0.106 |
  | Unchanged selection | 0.008 / 0.009 / 0.023 | 0.002 / 0.002 / 0.003 | 0.039 / 0.042 / 0.065 |

  The new resampling costs about 0.04ms more GPU time than the existing copy
  path, not zero. Unchanged timings are the benchmark's empty submission/timestamp
  overhead: the preparation itself encodes no work. All three modes retain the
  same 3MiB source/output storage at this extent, with no additional selection
  channel. Presentation uniforms grow by 16 bytes. These are preparation timings,
  not application presentation or physical-input latency. Repeat with ignored
  release test `affine_selection_preparation_latency`.
- Tests cover fractional coverage, holes, scaling/rotation/reflection, inversion,
  off-canvas placement, exact agreement across fills/brushes/masks, unchanged
  preparation reuse, retained GPU outline sources, and atomic Apply/undo/redo.
  Validation passes 32 core, 29 engine, 183 shared UI and 97 GPU correctness
  tests (16 separate GPU benchmarks ignored), strict all-target Clippy, workspace
  and Wasm compilation. GTK connected-selection/fill input and dark/light captures
  pass on the private Wayland display. No new native web/Android interaction run
  is claimed.
- Paired six-second GTK G-Pen runs (384px, synthetic input, actual Wayland
  presentation feedback) remain at 119.96Hz both with and without a raster
  selection. With selection, worker CPU median/p95/p99 is
  0.248/0.544/0.696ms; GPU 0.152/0.229/0.377ms; GTK frame-handler p99 0.040ms.
  Without selection, CPU is 0.318/0.608/0.736ms; GPU 0.133/0.299/0.455ms;
  GTK handler p99 0.055ms. Selected/unselected runs have one/zero discarded
  presentations, respectively; short-run tail variation is not evidence that
  selection is faster. Raw reports remain local. The pacing test now rejects an
  unknown workload name instead of succeeding with no measurements.
  The remaining Operation UI, linked-mask transforms and cold-pipeline readiness
  requirements still apply; this is not the GTK tool review milestone.

### GTK staged startup milestone

- GTK now uses the same four-stage shader dependency scheduler as Android and
  web. GPU worker construction no longer blocks the GTK initialization call.
  Paper is submitted before document and active-brush compilation, followed by
  unused shaders. Procedural brush textures use that same priority queue on all
  platforms, and readiness waits for their upload. Pixel queries wait for the
  real document frame; a contact begun before brush readiness stays suppressed
  until release. Existing two-frame queuing and direct Wayland presentation are
  preserved.
- Native pipeline data uses a bounded, build/adapter/driver-keyed private cache.
  The old eager constructors are deprecated; tests and diagnostic bindings use
  explicitly named headless constructors. Incoming Apple-port work now also
  uses the staged cached constructor and shares the native-host readiness gate
  with Android. This source integration passes workspace/Wasm checks and eight
  native-host unit tests; no physical Apple runtime validation is claimed here.
- Representative startup measurements (milliseconds from workspace creation):

  | Measurement | Previous eager GTK | Staged, empty app cache | Staged, warm app cache |
  | --- | ---: | ---: | ---: |
  | `window.present()` returns | 2752 | 863 | 779 |
  | First canvas presentation feedback observed | 2825 | 1836 | 1017 |
  | Document / active brush ready observed | Not separately available | 1864 / 1872 | 1017 / 1017 |
  | Entire startup catalog ready observed | Not separately available | 5381 | 1799 |

  These are individual local runs, not statistical startup percentiles or a
  cold-driver guarantee. The readiness timestamps include event-loop polling.
  GTK's initial widget/layout work still pauses the main loop (maximum measured
  post-present pump slice: 928ms cold, 197ms warm); moving shader compilation
  does not remove that separate UI startup cost. Constructor-only diagnostic
  timers were removed after confirming session/host setup itself takes under
  1ms. The retained ignored `native_startup_latency` test checks startup order,
  contact gating, native control changes and painting before optional completion.
- Six-second native pacing runs, 384px brushes, actual Wayland presentation
  feedback, no simultaneous GPU benchmark:

  | Workload | Worker render/present CPU median/p95/p99 ms | GPU median/p95/p99 ms | Presented Hz |
  | --- | --- | --- | ---: |
  | G pen | 0.273 / 0.517 / 0.661 | 0.135 / 0.283 / 0.488 | 119.95 |
  | Natural blender | 0.617 / 1.213 / 1.492 | 0.621 / 1.593 / 3.292 | 120.01 |
  | Wet round | 0.527 / 0.991 / 1.145 | 0.325 / 1.082 / 2.276 | 119.95 |
  | Watercolor | 1.176 / 1.978 / 2.403 | 1.507 / 3.711 / 4.660 | 119.91 |
  | Pan | 0.189 / 0.413 / 0.503 | 0.071 / 0.151 / 0.330 | 119.96 |
  | Hand tool | 0.183 / 0.405 / 0.499 | 0.067 / 0.134 / 0.181 | 120.00 |

  GTK frame-handler CPU p99 stays below 0.052ms in these runs. These are
  steady-state results, not a claim of 120Hz throughout initial UI construction
  or physical tablet input validation. Raw reports/cache files remain local.
- Validation: renderer regression suite including cache reload and reference
  pixels; GTK cold/warm startup and pacing; workspace/Wasm checks and strict
  renderer/GTK/FFI Clippy. Real Chrome + rebuilt Wasm verifies visible paper,
  delayed GPU-validation readiness, native web settings and drawing/panning
  during optional compilation, and startup with a loaded domain-warp filter.
  No new physical Android or Apple run is claimed by this GTK milestone.

### Operation controller and GTK interaction

- Operation now offers Move and Scale / rotate. The shared Rust controller owns
  eight resize handles, rotation, dragging inside the box, X/Y position,
  percentage scale, angle, Keep proportions, and Apply/Cancel. Ctrl/Cmd+T starts
  a transform; Enter applies and Escape cancels. Shift constrains movement,
  preserves resize proportions or snaps rotation to 15 degrees; Alt resizes
  around the center. Modifier changes apply without another pointer movement.
  Reflection and rotation retain the opposite resize anchor without an initial
  grab-offset jump. Move also edits existing rulers, never creates new ones.
- All updates use the immutable GPU preview transaction described above. Apply
  commits pixels and transformed selection as one undo entry; Cancel, tool
  changes and conflicting document edits restore the original. Numeric edits
  update only Tool Settings, not the Layers panel. Position sliders have useful
  content-relative soft ranges while typed expressions retain wider hard bounds.
  Scale display uses whole percentages but retains fractional stored values.
- GTK renders the ordinary Tool Set and numeric/action schemas. The GPU overlay
  adds one reusable filled-rectangle primitive for clean handles, replacing
  overlapping thick line corners. It allocates no additional image or render
  pass. Tool Settings action/checkbox labels now ellipsize with full tooltips,
  keeping the three-tile 128px minimum instead of forcing 165px.
- Color and scalar transform pipelines now use the shared deferred compiler and
  native cache. Unused transforms compile in stage four; a saved document with
  transforms promotes them into document dependencies before replay. Both
  recipes exist from construction, removing lazy scalar-renderer creation during
  the first wet transform. Only tiny empty bindings are added before use; image
  captures and parameter buffers remain demand-allocated and reused.
- Validation: 32 core, 29 engine, 186 shared UI tests; 97 GPU correctness tests
  plus the new saved-transform startup regression (16 hardware benchmarks
  excluded from the suite). Strict UI/renderer/GTK Clippy and workspace/Wasm
  checks pass. GTK input tests cover dragging, resizing, expression input,
  proportional scale, Apply/Cancel, undo and actual GPU color sampling at moved
  and restored locations. Dark/light and 128px screenshots are inspected under
  ignored `artifacts/familiar-workspace/operation-*.png`. No physical input or
  native web/Android transform UI validation is claimed.
- Six-second release GTK transform drag, with live Tool Settings, on the
  private 120Hz Wayland display:

  | Measurement | Median | p95 | p99 |
  | --- | ---: | ---: | ---: |
  | Worker render/present CPU (ms) | 0.572 | 1.034 | 1.216 |
  | Canvas GPU (ms) | 0.692 | 1.251 | 1.678 |
  | GTK frame handler (ms) | 0.096 | 0.262 | 0.315 |
  | Shared frame processing (ms) | 0.009 | 0.027 | 0.034 |

  720 submitted frames, 718 successful presentations, approximately 119.38Hz.
  Input is synthetic; presentation feedback is real. This covers a large
  ordinary-paint layer, not linked masks or continuous numeric editing. Source
  capture and first-use timing are separate from these steady-state numbers.
  A subsequent ordinary 384px G-Pen run delivers 119.96Hz with no discarded
  presentations. Worker CPU median/p95/p99 is 0.304/0.585/0.715ms; GPU is
  0.151/0.289/0.430ms; GTK handler is 0.015/0.040/0.056ms. Medians are slightly
  higher than the previous startup milestone's run; these separate short runs
  do not isolate driver/timing variation from code cost. The frame budget holds.
- Outstanding at this milestone: active/linked-mask transforms and a readiness
  gate if interaction begins before optional transform compilation finishes.
  Until mask support is implemented, Scale / rotate is unavailable for an active
  mask or paint with a linked mask; unlinked masks remain stationary. Bounds are
  conservative document-history bounds rather than a pixel-tight GPU reduction.
  Collapsible columns, region refinements, complete menus/file workflows, the
  final default layout, final validation and human approval remain required.

### Active and linked mask transforms

- Operation now accepts the active mask as well as paint with a linked mask.
  The shared controller uses each target's own origin and conservative content
  bounds; the renderer captures each target once. A linked pair shares the same
  canvas-space transform and selection even with different layer/mask offsets.
  Unlinked targets remain stationary. Apply commits both histories and the moved
  selection as one undo entry, retaining the displayed GPU result without another
  capture or resample. Masks on non-paint owners transform their mask only.
- Mask edits reuse ordered layer operations. Replay interleaves transforms with
  mask strokes, and Apply Mask retains that history after removing the live mask.
  Validation rejects invalid defaults, unordered edits and recursive transform
  coverage. R8 visibility uses replacement with selection coverage and its implicit
  background value, not pigment accumulation or wetness's maximum operation.
  Inversion remains a composition property. No CPU pixel rasterization/readback
  enters the interactive transform path.
- Cancellation now restores both targets before allocating pages for a new
  contact. This fixes newly allocated paint/mask tiles being discarded as preview
  tiles. Damage from a mask preview invalidates its owning layer and dependent
  cached/clipped filters. Saved mask transforms promote their shader dependencies
  into the document startup stage.
- The large linked-watercolor benchmark exposed avoidable scene work. Composition
  now queries only the at-most-four native tiles intersecting an output tile,
  instead of expanding/scanning the entire layer page set for every tile. Pigment
  and scalar fields capture their own bounds, and transformed wetness stays sparse.
  Guaranteed-dry neighborhoods bypass watercolor edge resolution. Watercolor
  scratch clears are folded into their render pass, like ordinary scene draws.
- Validation: 33 core, 30 engine, 3 render-contract and 187 shared UI tests;
  101 GPU correctness tests (16 hardware benchmarks excluded). Independent pixel
  oracles cover visibility defaults, fractional/inverted selections, crop,
  reflection, interpolation and distant transforms. Twelve linked/unlinked mask
  scenarios cover preview/replay equivalence, exact cancellation, atomic commit,
  subsequent ink and Apply Mask. Cached filter tests include mask-only transforms.
  GTK native controls, Apply/Cancel/undo and linked/unlinked mask screenshots pass.
  Workspace/Wasm checks and strict core/engine/render/UI/GTK Clippy pass. Shared
  UI tests cover host profiles; no new native web/Android/Apple UI run is claimed.
- Final renderer-only release timings, 2048×1536 imported artwork, last 120 of
  160 live updates, median/p95/p99 milliseconds:

  | Artwork | CPU submit | GPU | GPU completion included |
  | --- | --- | --- | --- |
  | Selected G pen | 0.188 / 0.205 / 0.241 | 0.109 / 0.112 / 0.116 | 0.345 / 0.388 / 0.439 |
  | Selected wet round | 0.305 / 0.461 / 1.269 | 0.172 / 0.185 / 0.204 | 0.549 / 0.744 / 1.473 |
  | Selected watercolor | 0.456 / 0.622 / 0.945 | 0.193 / 0.210 / 0.258 | 0.730 / 0.952 / 1.335 |
  | Selected G pen + linked mask | 0.645 / 0.737 / 0.865 | 0.525 / 0.530 / 0.553 | 1.263 / 1.393 / 1.579 |
  | Selected watercolor + linked mask | 2.459 / 4.084 / 4.904 | 1.385 / 1.455 / 1.643 | 4.077 / 5.835 / 6.731 |

  Before these scene/sparsity fixes, the isolated linked-watercolor run measured
  CPU 3.165/5.180/5.734, GPU 1.509/1.641/1.788 and completed
  4.938/7.345/7.867ms. An earlier run overlapped compilation and exceeded 8.33ms;
  it is not used as the isolated baseline. These short runs have timing variance,
  but deterministic work/memory reductions and pixel equivalence are verified.
  Linked-watercolor capture/uniform storage fell from 21,231,344 to 19,134,192
  bytes (about 2MiB); without the mask it is 14,456,512 bytes. This excludes scene
  caches and persistent layer pages. First-use linked-watercolor completion was
  8.608ms in the final run; steady-state percentiles do not cover that first use.
- Six-second GTK linked-watercolor transform drag: 721 submitted, 717 presented,
  four discarded; 119.51Hz real Wayland presentation. Worker CPU
  3.745/5.111/6.025ms, GPU 5.092/6.455/7.584ms, GTK frame handler
  0.117/0.248/0.308ms, shared frame processing 0.010/0.025/0.029ms. The first two
  frames exceeded the budget; this is not uninterrupted 120Hz. Ordinary 384px
  G-Pen regression: 119.96Hz, zero discarded, CPU 0.273/0.634/0.719ms,
  GPU 0.141/0.321/0.479ms and GTK frame handler 0.014/0.038/0.049ms. Synthetic
  input, real presentation; not physical pen-to-photon latency. Use
  `LAYER_PACING_BRUSH=Transform LAYER_PACING_TRANSFORM_MASK=watercolor` with
  `native_frame_pacing` to reproduce the linked scenario.
- Local review images: `artifacts/familiar-workspace/operation-linked-mask.png`
  and `operation-unlinked-mask.png`; screenshots, traces and caches remain ignored.
  Next: early optional-pipeline readiness/first-use responsiveness, collapsible
  columns, region refinements, complete menus/file workflows, final default layout
  and the complete GTK validation/human approval gate. The goal remains active.

### Live transform startup readiness

- Live transform presence now participates in the existing shader dependency
  key alongside document revision and brush settings. GTK, web and the shared
  native host check it before draining a frame. Starting a transform promotes
  its color/scalar and selection shaders to interactive priority; the caller
  polls readiness instead of synchronously compiling them during submission.
  Cancelling removes that readiness requirement. Once stage four completes,
  transform changes require no further startup scheduling.
- A deliberately blocked compiler regression verifies immediate not-ready
  status, cancellation, repeated-request deduplication and readiness after
  release without document or brush changes. All five startup regressions,
  eight native-host tests, 30 engine tests, workspace/Wasm compilation and
  strict renderer/engine/host/GTK Clippy pass. GTK's private-Wayland startup
  test also passes, including contact gating and painting before the optional
  catalog completes. Browser/physical-device interaction was not rerun here.
- This removes a shader-readiness hole, not the separate first-use texture
  capture/driver cost recorded above. No new steady-state speedup or cold
  120Hz guarantee is claimed. Collapsible columns are next; the remaining
  region, menu/file, default-layout and final-review requirements are unchanged.

### Collapsible-column shared layout foundation

- The dock tree now retains collapsed-column identity and expanded width,
  without replacing its groups with floating toolbars. Vertical splits remain
  stacked groups; the nearest horizontal split identifies a nested column.
  Resolution emits a one-tile-wide strip, fixed expand/grip geometry, grouped
  icons, a bounded content area and a trailing insertion area. Actual GTK
  widgets and collapse actions are not exposed yet.
- Collapse/expand preserves internal split ratios and tab state. Neighboring
  columns keep their width; full-width rows follow the changed column, while
  independent split rows retain their widths. Allocation uses actual constrained
  child sizes, avoiding width drift when expanding after minimum-width clamping.
  Removing groups transfers collapsed identity to the surviving subtree.
- Shared drop hints distinguish tab insertion, inter-group insertion and trailing
  insertion. New vertically stacked groups inherit collapse state; hidden Zen
  docks and clipped icons do not create targets. Existing removal/reweighting
  treats a collapsed subtree as a single fixed-width item.
- Nine new layout regressions cover nested columns, restore, save/load,
  undo/redo, insertion, pruning, sibling widths and small-view overflow. The
  complete shared UI suite passes 196 tests, with strict UI Clippy and
  workspace/Wasm compilation. These are core tests, not native UI validation.
- This was a layout-only milestone. GTK integration follows below; the complete
  workspace still requires its final validation and human approval.

### Collapsed-column GTK interaction milestone

- GTK now renders one-tile-wide collapsed strips, grouped panel icons, the
  expand button and a bottom grip. Collapse from a group context menu, the
  non-tab header double-click or a horizontal resize gesture. Resize collapse
  latches until release and remembers the pre-gesture width; cancellation and
  workspace undo restore the original layout. Standalone toolbar and floating
  handle behavior is unchanged. New presentation remains GTK-first.
- A grip moves the entire collapsed subtree through the ordinary dock tree.
  Shared target validation permits side edges and positions beside columns,
  never floats or tab merging of a whole column. Canvas release leaves the
  column in place. Groups, tab choices, remembered widths and nested proportions
  survive; source-width reclamation uses the existing recursive rules. Panel,
  group and toolbar incoming drops use shared tab/gap/trailing insertion targets.
- Column drawers reuse the tool drawer renderer and animation lifecycle, with
  a tab header and the maximum declared width of their tabs. One drawer per
  column can remain open independently. Outside canvas input does not dismiss
  them; the originating icon closes its drawer even after a tab switch, and a
  different group replaces it. Tool shortcuts remain available. An open drawer
  pins revealed Total Zen controls, while explicitly entering Zen closes drawers.
  Configure expands the ordinary column first and opens its existing configuration
  view, instead of leaving an invisible expansion attached to a collapsed group.
- Layer/filter preview producers serve dock, tool drawer and column drawer views
  together; no separate GPU preview generator is added. Diagnostics in a closed
  collapsed tab no longer enables telemetry. Native toolbar bodies reuse shared
  tile layout and ordinary tile actions.
- Native tests caught and fixed generic button padding shifting strip icons
  nine pixels down, and fresh GTK scroll adjustments clamping restored positions
  to zero. Native scroll offsets feed shared icon/drop geometry and survive
  widget rebuilds. Transient measurements do not create workspace undo entries.
- Validation: 206 shared UI tests; native GTK collapsed columns, existing tool
  drawers, panel expansion, Zen and stacked-divider tests on the private Wayland
  display, with GTK critical warnings treated as failures. The column test checks
  dark/light pixel bounds, persistent drawers, tab switching, real grip routing,
  column movement and a 31-tab overflowing strip before/after membership changes.
  Dark/light captures were inspected under ignored
  `artifacts/familiar-workspace/columns-*.png`. Workspace/Wasm compilation and
  strict UI/GTK Clippy are checked separately. No new physical-device or frame-rate
  claim is made by these UI tests.
- Nested collapsed subcolumns retain their state when a containing column also
  collapses. Drawer/context anchors resolve to the visible outer strip until it
  expands, not to a hidden child strip.
- Still required for columns: the final presentation audit in the completed
  default workspace. Native incoming-drop, resize and constrained-viewport coverage
  is recorded below. This is not the
  full GTK approval milestone. Region refinements, complete menus/file workflows,
  final default layout and full workspace validation remain outstanding.

### Nested toolbar content drawers

- Tiles inside a collapsed column's toolbar drawer now use the ordinary shared
  tool-selection and drawer lifecycle. First press selects a tool, the next opens
  its controls; Color opens directly. Dismissing the child does not dismiss its
  persistent parent. Switching the parent tab removes an obsolete child.
- GTK reports visible, clipped tile bounds after allocating parent drawers, then
  allocates the tool drawer in the same frame. Shared Rust validates ownership,
  chooses placement and handles outside/origin contacts. Scroll and animation
  update the measurements; they are neither persisted nor workspace undo edits.
  The child stays above its parent and reuses the existing connected-tile styling
  and panel preview producers. No additional canvas/GPU work is introduced.
- A native overflow test exposed a missing height-for-width request on toolbar
  bodies. GTK now advertises it and preserves natural content height inside
  drawer scroll viewports. The test scrolls a 180-tile toolbar on both sides,
  checking its still-visible source and child anchor move together.
- The shared UI suite passes 207 tests and strict UI/GTK Clippy. Workspace and
  Wasm checks pass (the workspace retains existing non-Metal Apple warnings).
  Native nested-drawer tests pass on the private Wayland GPU display, with
  critical warnings fatal, as do collapsed columns, tool drawers, panel expansion,
  Zen and stacked-divider regressions. Dark/light images were visually inspected at
  `artifacts/familiar-workspace/columns-nested-{Dark,Light}.png` (ignored).
  These are GTK interaction checks, not physical-device or frame-rate claims.
- Further native validation covers nine incoming drag combinations: a panel,
  complete tab group and standalone toolbar into a collapsed group's tab list,
  inter-group gap and trailing space. The stable GTK input handler performs the
  tear-off, hint and release; all resulting panels remain in the collapsed tree.
  Native divider drags collapse each side, stay latched when moved back, and
  restore the original width on expansion.
- Nested drawers were also checked at an actually allocated 640×480 window,
  with bounds assertions and inspected dark/light captures at
  `artifacts/familiar-workspace/columns-nested-small-{Dark,Light}.png`.
  The test unmaximizes and waits for the restore configure before resizing;
  requesting a smaller default alone did not resize the maximized test window.
- Scrolling a toolbar origin fully out of view now clears its presented drawer
  rectangle, so invisible UI cannot intercept canvas contacts. Scrolling it back
  restores its current geometry; the persistent column drawer remains open.
  Native tests verify both directions. No test artifacts or local paths are
  included in the repository.

### Editable-project foundation and faithful watercolor replay

- The shared [project codec](reference/project-format.md) now preserves editable document
  history, source assets, masks, selections, transforms, rulers and exact runtime
  filter definitions. It prunes removed artwork and unused assets, rejects
  malformed/oversized input, and checks the compressed stream's checksum and
  completion. This is a codec milestone, not a claim that Save/Open dialogs exist.
- Live-versus-reopened GPU tests found that watercolor replay combined a whole
  stroke into one bleed update. Stored material-update boundaries now preserve
  the live transport sequence during undo/recovery/opening without adding GPU
  work while drawing. Contiguous update lookup avoids scanning unrelated
  strokes. No pixel channel or canvas readback was added for project saving.
- Validation passes 39 core, 31 engine and 207 shared UI tests, plus the new
  fresh-GPU save/reopen test. The latter compares exact output for imported
  transparency, wet brushes, live/applied masks, groups, gradients, figures,
  transforms, clipped multipass filters and animation, then checks continued
  wet painting. Generated PNGs were inspected under ignored
  `artifacts/familiar-workspace/project/`.
- The Vulkan renderer suite passes 103 tests, with 16 hardware benchmarks ignored
  and the previously documented historical filter-reference test explicitly
  excluded. That failure remains unresolved; this is not a fully green renderer
  suite claim. Strict core/engine/render/UI/GTK Clippy, workspace and Wasm checks
  pass, apart from the existing non-Metal Apple workspace warnings.
- The native pacing test now waits for actual brush readiness and verifies a
  committed stroke with real samples/material updates. A fixed warm-up delay
  could otherwise benchmark a contact suppressed during shader initialization.
  Six-second release runs, 384px brushes, on the private Wayland display:

  | Brush | Worker CPU median/p95/p99 ms | GPU median/p95/p99 ms | GTK handler median/p95/p99 ms | Displayed Hz |
  | --- | --- | --- | --- | --- |
  | G-Pen | 0.243 / 0.590 / 0.818 | 0.121 / 0.293 / 1.575 | 0.007 / 0.050 / 0.060 | 119.40 |
  | Watercolor Wash | 1.155 / 1.836 / 2.359 | 1.359 / 2.379 / 2.887 | 0.017 / 0.043 / 0.060 | 119.73 |

  Input is synthetic and child-surface presentation feedback is real. These
  validate approximately 120Hz steady-state drawing, not physical tablet latency,
  cold-start latency, or an isolated before/after overhead measurement. Earlier
  unguarded warm-up runs from this milestone are not used as drawing evidence.
- No new third-party package versions were introduced. The direct compression
  dependency reuses the existing locked MIT/Apache-2.0 `flate2` with its Rust
  backend. `cargo-deny` is not installed in this environment, so its automated
  licensing gate was not rerun; dependency metadata and lockfile changes were
  reviewed directly. Captures, raw timings and local paths remain untracked.
- Next: document asset ownership, atomic asynchronous native file workflows,
  unsaved-work/cancellation handling, the eight menus and command ribbon, final
  default layout, region refinements, historical filter-reference reconciliation,
  and the integrated GTK approval gate. The full goal remains active.

- Post-merge check: incoming native Apple color controls are integrated. Shared
  UI now passes 208 tests; core/engine pass 39/31, native host passes eight, and
  the Apple Rust bridge passes eleven tests serially with Vulkan access on Linux.
  The sandbox run cannot supply the hardware adapter required by its GPU cases;
  this is not macOS/iOS device validation. Workspace/Wasm checks and strict
  UI/GTK Clippy pass. Concurrent Android workspace changes are separate work.

### GTK document workflows

- File now exposes New, Open, Save, Save As, Export PNG and Close with shared
  command metadata/shortcuts. New/Open create separate document windows; the
  existing drawing is never replaced by an invalid incoming file. The main
  window title shows the filename and an unsaved indicator. The other seven
  menus and final command ribbon still need their complete integration.
- Shared Rust owns source-asset retention, undo-state save checkpoints,
  single-flight requests, cancellation and close-after-save authorization.
  Navigation/selection does not dirty artwork; undoing to the saved checkpoint
  clears the indicator. Edits during a write remain unsaved, including a stroke
  that finishes after a pending save-and-close. Discard/Cancel/Save use the same
  policy from the title-bar close button and the File menu.
- GTK uses asynchronous FileDialog and AdwAlertDialog. Source snapshots share
  immutable buffers with uploads; validation, compression, PNG encoding and
  atomic file writes run off the input thread. Export follows pending document
  frames and uses the existing explicit GPU readback, never the viewport image.
  Local files are supported; remote GIO destinations are not implemented.
- Startup filter-library refresh no longer migrates an opened project's embedded
  programs. Explicit replacement still supports live migration, with namespace
  validation and last-working-program behavior unchanged.
- Validation: 40 core, 31 engine and 213 shared UI tests pass. The native GTK
  document test exercises actual fallback file choosers, cancellation, corrupt
  input, Save As, PNG export, New controls, fresh-window pixel-exact reopening,
  and save/cancel during close. Atomic-write failure leaves the original intact;
  PNG round trips preserve RGBA and its sRGB declaration. GTK critical warnings
  remain fatal. The scripted chooser waits for its asynchronous initial folder
  model before responding; production adds no delay. The portal provider itself
  is not automated by this test.
- New/unsaved dialogs were inspected in both themes under ignored
  `artifacts/familiar-workspace/files/`. Workspace/Wasm checks and strict
  UI/GTK Clippy pass; existing non-Metal Apple warnings remain. No new physical
  input, export-during-painting latency, or 120Hz benchmark claim is made here.
  The PNG dependency reuses the already-locked MIT/Apache-2.0 version. No captures,
  project files, raw logs, machine identifiers or local settings are committed.
- Remaining: final eight-menu/command-ribbon assembly, requested default layout,
  region refinements, historical filter-reference reconciliation, and the full
  integrated GTK validation/user-approval gate. The goal remains active.

- Project-source integration: owned RGBA images and R8 brush masks now share
  their immutable pixel allocation across the session, GTK worker queue and
  renderer source cache. Borrowed imports use one common row-packing helper.
  GPU tests assert allocation identity, reject malformed replacements without
  losing the previous source, check padded rows, and still reproduce reopened
  artwork exactly. No canvas readback or drawing-time work was added.
- Post-merge validation passes 41 core, 31 engine, 213 shared UI and 10 host
  tests, both GPU project tests, the native GTK document workflow, workspace
  and WebAssembly compilation, and strict core/engine/render/UI/GTK Clippy.
  Existing non-Metal Apple warnings remain. Panel-availability tests now check
  the shared host policy rather than repeating a list that drifts as ports
  implement their native controls. Concurrent Android presentation work remains
  separately owned; no new device or frame-rate claim is made here.

### Shared application menus and GTK projection

- GTK's header now exposes File, Edit, Layer, Select, Filter, View, Window and
  Help. The GNOME primary menu remains at the right. `ApplicationMenu` owns the
  identities, grouped contents and current command state. Layer directly uses
  the selected layer/mask's existing context model; Window reuses workspace
  management; Filter projects the entire runtime catalog by category regardless
  of the picker's search. Adding a runtime filter updates the menu too.
- Edit exposes working Clear layer, Fill selection and Scale/rotate commands.
  Select exposes Select all pixels, Deselect and Invert selection plus selection
  tools. Commands are independently bindable and usable by toolbar tiles. Clear
  layer explicitly erases the whole editing paint layer (not an implicit selected
  region), and is disabled on locked, paper and mask targets. Fill requires a
  selection and uses the existing GPU operation. Selection changes do not mark
  artwork unsaved; paint edits retain ordinary undo/checkpoint behavior.
- Grouping follows [Krita's Edit menu](https://docs.krita.org/en/reference_manual/main_menu/edit_menu.html)
  and [Select menu](https://docs.krita.org/en/reference_manual/main_menu/select_menu.html),
  using [GNOME's menu sections and native controls](https://developer.gnome.org/hig/patterns/controls/menus.html).
  Unimplemented clipboard/selection-refinement operations are not inert menu
  placeholders. Help provides shortcuts, Website, Source code and About. Link
  metadata is shared with About; GTK launches only those core-defined URLs via
  asynchronous `UriLauncher`. Tests do not launch an external browser.
- GTK now uses the existing context-menu projector for application menus too.
  This removes its separate command-action/accelerator cache and per-update
  menu traversal. Models are resolved on opening with current state and shortcut
  hints, not each drawing frame; Filter menu generation requests no previews.
- The GTK menu test traverses every section/submenu in both themes and activates
  theme toggles, selection, fill/clear/undo and filter insertion through native
  actions. It also checks updated/removed shortcut hints on reopening. Inspected
  captures are under ignored `artifacts/ui/menus/`. Shared tests verify layer and
  Window model equivalence, runtime registration, selection undo/save semantics,
  busy/locked-target restrictions and typed website requests.
- Validation passes 214 shared UI tests, strict UI/GTK Clippy, workspace and
  WebAssembly compilation, and native menu, preferences/shortcut and workspace
  management tests.
  The preferences fixture now edits a tool with a direct default binding rather
  than assuming Brush still owns its family's B key. Existing non-Metal Apple
  warnings remain; no new frame-rate or physical-input claim is made.
- The workspace regression uses the core's Tool Set label and releases its
  free-movement fixture in the canvas center. Its old fixed displacement landed
  inside the neighboring sidebar's current snap zone; production docking
  semantics are unchanged. Native context commands, floating movement/resizing,
  tab grouping, visibility and workspace undo/redo pass in both themes.
- The command ribbon and requested default dock layout are next. Region
  refinements, historical filter-reference reconciliation and the integrated
  visual/performance/user-approval gate still remain. The new eight-menu GTK
  presentation is not yet rolled out to the other hosts; shared Rust/Wasm builds
  remain compatible with their existing menu presentation.
- Incoming Apple document-workflow changes are merged. Post-merge checks pass
  41 core, 32 engine, 215 shared UI and 10 host tests, workspace/Wasm compilation,
  strict UI/GTK Clippy and the native GTK document workflow. The two Apple Rust
  bridge project-job tests also pass on Linux/Vulkan; this is not macOS/iPad UI
  validation. Prepared/retired sessions are boxed on the file worker so adoption
  returns a small ownership handle, including on failure, without moving a large
  session through the error value or destroying GPU resources on the input queue.

### Default editor workspace and command ribbon

- GTK now starts with the full shared editor preset: the left-edge Tools ribbon;
  Tool Set, Tool Settings, Brush size and Color in the next column; the Commands
  ribbon between the sidebars; and Navigator/Diagnostics, Properties/Filters and
  Layers stacked on the right. All 17 selectable tools, the Color drawer tile,
  nine command tiles and their compact dividers use existing toolbar behavior.
  Text, Comic and Correct line remain excluded. Commands is a stable named toolbar,
  not a special renderer or a reserved user-toolbar number.
- The preset and platform rollout policy live in Rust. Other hosts retain their
  existing initial layout until their presentation is ready. Restoring a saved
  workspace never replaces its topology or toolbar contents. Reset layout uses
  the host's preset positions but retains configured/renamed/custom toolbars and
  does not resurrect deleted ones. New topology IDs cannot collide with custom
  toolbar IDs from older workspaces.
- Native visual review caught and fixed allocation issues not detectable from
  control-state checks alone: Tool Set groups now fit their fixed three-tile
  widths instead of long natural label widths forcing an extra row; Color's
  centered wheel compresses before its controls scroll; Navigator's preview
  shrinks before its buttons clip. Centered color hit testing uses the same
  origin as rendering. Long lists and settings remain scrollable.
- Header menu buttons now receive the same six-pixel horizontal padding as
  other header controls; the document title ellipsizes before overlapping them.
  The Color tile displays both foreground/background swatches through one new,
  locally authored shared SVG. Normal, Zen and drawer projections share its
  palette update function. No new canvas passes or readbacks are introduced.
- A narrow work area retains at least 128 logical pixels when panel minimums
  permit, so a wrapping command ribbon does not occupy the entire canvas height.
  Geometry tests cover 1600×1200 through 640×480, both ribbons, group order,
  minimum widths, tile/grip bounds, saved-state round trips and customized resets.
- Validation: 217 shared UI tests, strict UI/GTK Clippy, workspace/all-targets and
  WebAssembly checks pass. The merged Android column implementation required
  removing Android from an outdated test of pre-collapse double-click behavior;
  production behavior is unchanged. Native tests pass for the actual default
  workspace (all tools and their connected drawers, actual GPU strokes, color
  input and compact Navigator hit targets), existing color controls, workspace
  restore, stacked dividers, customization and all application menus.
  Existing gesture fixtures explicitly restore a representative customized layout
  rather than implicitly depending on the shipped preset. Startup/pacing tests
  continue to use the real default.
- Dark/light captures at 1200×900 and 900×640 are under ignored
  `artifacts/familiar-workspace/default/`. Generated captures, projects, raw
  timing reports and machine identifiers are not committed. No new dependency
  or third-party artwork was added.
- Remaining for the goal: region gap closing/expansion/antialiasing, watercolor's
  visible selection boundary, the historical filter-reference mismatch, final
  integrated tool/render validation and human GTK approval. This milestone does
  not claim the full goal or cross-platform presentation rollout is complete.

### Full-workspace pacing investigation (gate remains open)

- An old/new workspace comparison on the same release binary reproduced a
  regression: the old fixture delivered 119.96–120.01Hz, while the complete
  default delivered 116.62–117.77Hz across the paired drawing runs. Both used
  real GPU strokes and actual child-surface presentation feedback. This is
  synthetic input on a private Wayland compositor, not physical tablet latency.
- The retained benchmark now records native GTK paint and whole-main-context
  dispatch intervals, in addition to canvas-worker/input timings. These are
  elapsed intervals (including driver waits), not CPU-utilization measurements.
  It records the actual GTK renderer and number of live overview updates, waits
  for startup completion, and can compare `LAYER_PACING_WORKSPACE=fixture` with
  `default`. No new timers run in production.
- GTK painting produced a 34ms main-loop stall while G-Pen GPU p99 remained
  0.38ms. A corrected Navigator exclusion restored 119.96Hz with no discarded
  frames. The exclusion uses zero opacity and asserts that preview updates stay
  inactive: simply hiding the widget was invalid because control refresh later
  restored its configured visibility. Waiting for shader compilation, disabling
  the canvas cursor, and explicitly requesting GTK Vulkan did not fix the issue.
- A temporary, local-only Vulkan trace located repeated slow memory allocations
  and frees inside GTK on its main thread, including 1MiB backing allocations.
  The default Navigator scale also made GTK create an extra mipmapped image for
  each incoming 256×192 overview. The trace is not an application dependency or
  a library override shipped with the app.
- Navigator now uses GTK's explicit
  [linear texture sampling](https://docs.gtk.org/gtk4/method.Snapshot.append_scaled_texture.html).
  The source still uses the existing GPU downsampling and live update rate;
  only GTK's additional mipmap generation is removed. No CPU canvas raster,
  mutable-texture trick, new graphics backend or hidden Navigator is used.
  The native Navigator test passes; its dark/light output was visually checked.

  Latest complete six-second runs with the full workspace and live overview
  (times in milliseconds, median / p95 / p99):

  | Workload | Worker CPU | Canvas GPU | GTK paint elapsed | Presented Hz | Overview updates |
  | --- | --- | --- | --- | ---: | ---: |
  | G-Pen | .297 / .578 / .730 | .125 / .297 / .432 | .005 / .615 / 2.095 | 118.79 | 87 |
  | Watercolor | 1.088 / 1.986 / 2.404 | 1.244 / 2.289 / 2.842 | .005 / .665 / 2.391 | 116.04 | 88 |
  | Pan | .223 / .400 / .485 | .064 / .126 / .199 | .270 / .509 / .587 | 119.34 | 0 |

- Earlier linear-sampling runs reached 119.45/118.73Hz for G-Pen/watercolor.
  GTK paint p99 improved from roughly 7–9ms to 2–4ms, but allocation stalls and
  missed presentations remain: the latest G-Pen GTK maximum was 23.14ms, and an
  earlier watercolor maximum was 32.92ms. The smaller typical cost is useful,
  but **does not close the consistent-120Hz drawing gate**. Further work must
  address the live-preview texture lifecycle/presentation scheduling without
  suppressing the overview or changing brush algorithms to hide the issue.
- The updated benchmark asserts at least twenty live overview updates during
  drawing; panning correctly reuses the unchanged overview. Raw reports and
  traces remain local and untracked. Region refinements, the historical filter
  comparison, final integrated validation and user approval remain outstanding.
- The full native default-workspace test was rerun successfully after the
  sampling change. Incoming Apple menu/shortcut work is merged; post-merge
  validation passes 217 shared UI and 12 host tests, strict UI/GTK Clippy,
  workspace/Wasm checks and the native GTK menu/document workflows. Existing
  non-Metal Apple warnings remain. This is shared-source integration, not new
  Apple-device validation. The temporary Vulkan tracing source and binary were
  removed; only test-only timing/preview assertions remain in the application.

### GPU overview presentation foundation

- Tested a bounded four-buffer GBM/DMA-buffer preview transport locally. Buffers
  were reused only after GDK's texture-release callback; steady mapped writes
  were inexpensive. Nevertheless the full workspace actually using
  `GdkDmabufTexture` regressed to 115.39/111.34Hz in paired G-Pen runs, with GTK
  paint p99 of 16.56/23.68ms. Reusing backing storage alone does not remove GTK's
  import/presentation stalls. The temporary integration, dependency declarations
  and diagnostic output field were removed, not left as another rendering path.
  Separate rectangular GraphicsOffload probes also fell back (verified through
  Wayland attachment requests); they are not evidence of successful offload.
- Added opt-in overview rendering to the existing `ViewportPresenter`. Native
  hosts can supply image bounds, camera work-area corners, colors, scale and
  opacity. The presenter samples its existing document composition and draws all
  overviews with one instanced draw inside the existing viewport pass. There is
  no preview image, image upload, pixel readback, extra submission or additional
  compositor surface. Work-area outlines are drawn in that same shader.
- The overview pipeline is prepared only for hosts that request it, using the
  renderer's pipeline cache. Each placement uses an 80-byte record; a single
  overview retains a 128-byte vertex buffer. Geometry/color changes reuse this
  buffer; identical placements upload nothing. Hidden overviews draw nothing.
  Native integration must prewarm the optional pipeline during startup, before
  accepting drawing input, and present placements through the existing worker.
- Exported previews and in-surface overviews share the same sixteen-tap linear,
  premultiplied-color sampling function. Direct overviews sample at their final
  physical size, whereas exported previews retain their existing 256px limit.
  This is not a claim of pixel identity to the two-stage GTK-scaled export.
  Window coverage is shared with canvas/cursor presentation; overview blending
  retains destination alpha so it cannot thicken antialiased window corners.
- Release renderer-only benchmark: 2048×1536 output, 40 warmups, 120 measured
  frames per case. CPU covers presenter setup/encoding/submission; GPU is the
  queue span between timestamp markers, including submission gaps. Values are
  median/p95/p99 milliseconds, not physical input-to-display latency:

  | Overview workload | CPU | GPU |
  | --- | --- | --- |
  | None | .007 / .017 / .031 | .028 / .034 / .049 |
  | One, 100×75 | .008 / .027 / .029 | .036 / .051 / .055 |
  | One, 256×192 | .008 / .015 / .027 | .039 / .043 / .056 |
  | Two, 256×192 each | .007 / .014 / .026 | .046 / .048 / .061 |
  | One, with camera/outline updates | .010 / .010 / .016 | .043 / .045 / .046 |
  | None, repeated control | .007 / .015 / .028 | .028 / .033 / .045 |

- GPU tests cover transparent color composition, linear/sRGB targets, overview
  bounds, rotated/flipped camera outlines, subsequent painting, multiple views,
  hiding/reopening, resource reuse and rounded-window alpha. Existing exported
  Navigator preview and transformed selection-outline regressions also pass.
  Strict renderer Clippy, workspace/all-targets and WebAssembly checks pass;
  the native GTK Navigator interaction/capture test passes. Existing non-Metal
  Apple build warnings remain; no new device validation on other hosts is claimed.
- This foundation's renderer-only timing did not prove complete GTK delivery.
  The following milestone connects it to the native workspace and measures that
  integration. Selection refinements, watercolor boundaries, the historical
  filter-reference mismatch and final GTK approval remain separate open work.

### GTK in-surface Navigator integration

- GTK now supplies native image geometry, clipping and opacity with the existing
  canvas frame. The GPU worker prewarms the optional overview pipeline during
  startup, then draws the image and camera outline from the current composition
  in its existing presentation pass. Painting and camera changes no longer
  redraw GTK's Navigator. The GTK exported-preview transport, polling timer and
  preview-specific frame-clock lifecycle are removed; other hosts' exported
  preview API is unchanged.
- A rectangular clip preserves image coordinates when native ancestors scroll
  or crop content. Each placement is now 96 bytes; the one-overview GPU buffer
  remains 128 bytes. No image allocation, pixel upload/readback or additional
  surface/submission is required. This does not mean all per-frame CPU metadata
  is allocation-free: bounded placement records accompany existing frame data.
- Native snapshots retain panels, controls and shadows, but leave transparent
  rectangles for their GPU images. Image holes also cut native panels lower in
  stacking order, so a floating Navigator can cover another panel; later native
  panels still cover the image. The canvas itself is never cut. Native CSS fade
  opacity is read from the snapshot rather than inferred from widget opacity,
  which does not include CSS transitions. Hidden/unmapped views submit nothing.
- GPU tests verify partial clipping against the uncropped image, including its
  camera outline and both output transfer formats. Native regressions cover
  camera-only composition reuse, docked and drawer overviews together, drawer
  closing, Zen hiding/reopening, both overlapping panel orders, and stopping
  presentation while idle. Rectangular subtraction has exact partition/no-
  overdraw coverage. Review images stay in ignored
  `artifacts/familiar-workspace/`; no machine-specific reports are committed.
- Final full-default-workspace release runs on the same private 120Hz Wayland
  compositor (six seconds of synthetic input each, real GPU painting and actual
  canvas-surface presentation feedback; not physical stylus-to-display latency):

  | Workload | Worker CPU median/p95/p99 ms | GPU median/p95/p99 ms | Presented Hz | Discarded |
  | --- | --- | --- | ---: | ---: |
  | G-Pen | .227 / .547 / .702 | .113 / .252 / .384 | 119.958 | 0 |
  | Watercolor | 1.040 / 1.743 / 2.347 | 1.200 / 2.146 / 2.575 | 119.958 | 1 |
  | Pan | .220 / .433 / .529 | .067 / .591 / 1.700 | 119.568 | 1 |

- The live Navigator was drawn in all 723/722/720 canvas frames respectively.
  Its input composition changed 721/720/1 times: panning correctly reused the
  image. GTK painted only twice per drawing run and zero times during panning.
  Main-context dispatch p99/max was .016/1.141ms, .061/1.657ms and .049/.103ms.
  The previous 23–34ms GTK preview-allocation stalls did not recur.
- Do not interpret these averages as a no-spike guarantee: that pan run had one
  33.10ms worker outlier and a 30.21ms presentation interval despite the fast
  main loop. Test-only timings now separate composition, encoding, queue submit,
  feedback and present. Three subsequent pan runs delivered
  120.016/120.005/120.015Hz with zero discarded frames; combined worker CPU p99
  .569ms, GPU p99 .231ms and main-dispatch max .085ms. Their slowest frame was
  2.06ms, principally in queue presentation. The original 33ms event predates
  phase timing, so its exact cause is not established. The additional measurements
  remain test-only; no production timers or alternate preview path were added.
- Validation passes: native Navigator and complete default workspace, GPU
  overview/clipping/resource reuse, existing exported overview and transformed
  selection-outline tests, 217 shared UI and 12 host tests, strict GTK/UI/renderer
  Clippy, workspace/all-targets and WebAssembly checks. The only workspace
  warnings are existing non-Metal Apple unused items. Dark/light, compact,
  drawer, Zen and overlap captures were visually inspected. Remaining overall
  work is region gap closing/expansion/antialiasing, watercolor selection edges,
  the historical filter-reference mismatch, final integrated validation and
  human GTK review; low-frequency worker presentation outliers still warrant
  investigation rather than an unconditional latency claim.
- Merged the independently published Apple Navigator/Diagnostics integration.
  Both hosts reuse its composition-revision query instead of retaining duplicate
  getters. Post-merge shared tests, strict GTK/UI/renderer Clippy, workspace/Wasm
  checks and native Navigator/startup readiness tests pass. This is source and
  Linux-host validation, not additional physical Apple-device testing.

### Fill and Auto Select edge refinement

- Tool Settings now exposes Close gaps (0–32 document pixels), Expansion
  (−32–32 pixels), and Edge smoothing (0–100%), separately from color tolerance.
  Gap closing and expansion default to zero; smoothing defaults to 100% in the
  shared UI. Numeric kinds, ranges, units, validation and request snapshots live
  in Rust; GTK uses its existing spin/slider controls without a custom form.
  The separation follows [CSP's Fill tool controls](https://help.clip-studio.com/en-us/manual_en/420_fill/Fill_Tool.htm),
  not an attempt to copy its proprietary algorithm or claim identical pixels.
- Gap closing opens the color-eligible bitmap (closing its complementary line
  barriers), then the existing connected-component algorithm selects the seed's
  region. Mirrored footprints prevent odd gap widths shifting the result.
  Signed square-neighborhood expansion/shrink follows connectivity. Four corner
  samples soften diagonal stair steps while preserving thin features; the
  existing quarter-coverage representation is retained. Fractional smoothing
  tests use [WGSL's ties-to-even rounding](https://www.w3.org/TR/WGSL/#round-builtin).
  The final selection limit applies after expansion and smoothing as well as
  during detection, so Fill cannot grow outside the artist's selection.
- This bounded geometric method is not semantic gap recognition: an aggressive
  Close gaps value can remove a narrow region, including the seed location.
  Expansion uses square, not circular, neighborhoods. Smoothing softens corners,
  not a Gaussian blur, and straight pixel-aligned boundaries stay sharp.
  These limitations are independent of color tolerance; zero refinements retain
  the original three GPU dispatches.
- Morphology processes 32 pixels per word and reuses the component-parent
  allocation as its other scratch plane outside labeling. Refinements add one
  retained bit per document pixel (row padded): 0.375 MiB at 2048×1536, versus
  12 MiB for the existing parent buffer, a 3.125% increase over that buffer.
  Query-owned resident buffers grow from about 13.5 to 13.875 MiB before source
  captures, about 2.78%. No scratch image is allocated until a request needs it.
  Bounds/count now share the coverage buffer footer, preserving the portable
  four-storage-buffer limit and removing the separate bounds buffer/copy.
- All eleven pipeline recipes participate in the existing four-stage startup
  queue as optional work. An early request promotes only its required stages,
  remains single-flight, and resumes through normal asynchronous polling after
  compilation. It neither joins the compiler nor allocates document-sized
  buffers while waiting. Other tools remain usable. Headless callers retain
  lazy compilation; there is no second raster path or platform-specific policy.
- Serial release benchmark, 2048×1536 linework, 30 warmups and 120 measured
  requests; median/p95/p99 milliseconds. CPU is request encoding/submission,
  GPU is the measured device span, and completion explicitly waits for that
  submission (not physical input-to-display latency):

  | Refinement | CPU | GPU | Completion |
  | --- | --- | --- | --- |
  | None | .016/.028/.036 | .152/.155/.157 | .209/.222/.226 |
  | Close gaps 4px | .021/.024/.027 | .181/.183/.183 | .241/.258/.275 |
  | Expand 4px | .023/.025/.026 | .265/.271/.273 | .329/.336/.339 |
  | Smoothing 100% | .021/.021/.024 | .271/.278/.282 | .333/.341/.349 |
  | Gap 4px, expand 2px, smoothing | .027/.029/.032 | .305/.310/.312 | .373/.380/.383 |
  | Gap 32px, expand 32px, smoothing | .027/.077/.211 | .356/.361/.365 | .428/.475/.614 |
  | Shrink 32px, smoothing | .020/.020/.022 | .306/.312/.314 | .380/.386/.390 |

- A separate complete-request benchmark includes source composition and the
  asynchronous packed history snapshot. With gap 4px, expansion 2px and smoothing,
  visible-source CPU/GPU/completion p99 were .229/.379/1.038ms; raw-layer
  .182/.530/1.268ms; reference-scene .767/.700/1.958ms. Reference-scene median
  completion rose from 1.231ms without refinement to 1.398ms with it. This is
  measured additional work, not a zero-overhead claim. First-use measurements
  are separate: unprepared compilation previously took tens of milliseconds;
  after pipeline preparation one first complete request still took 16.311ms,
  including initial resources/history transfer. No cold-request 120Hz guarantee.
- Native startup passes with document/brush ready at 952ms and all optional
  compilation done at 1047ms in one warm-cache run; not statistical startup
  percentiles. A six-second full-workspace G-Pen run with raster selection
  delivered 119.779Hz with one discarded presentation. Worker CPU p99 .755ms,
  GPU p99 .315ms and main-context maximum .779ms; maximum worker 1.862ms. This
  confirms a fast warm drawing path, not elimination of every presentation gap.
- GPU validation covers independent pixel morphology at odd row widths, gaps,
  large positive/negative distances, fractional smoothing, invalid inputs,
  queued scratch reuse, final selection limits, antialiased Fill and history
  replay, and asynchronous startup ordering. Native testing draws a genuinely
  open outline, proves the unrefined selection leaks, closes it, fills below
  reference line art and checks undo/redo. Dark/light selection and Fill captures
  were inspected under ignored `artifacts/familiar-workspace/`. No external
  textures, logs or machine-identifying reports are committed.
- Validation also passes 217 shared UI tests, strict UI/GTK/renderer Clippy,
  workspace/all-targets and WebAssembly checks. The broad GPU run passed 108
  tests; a further runtime-kernel test initially failed because its fixture was
  resolved against the process working directory. It now uses the package path
  and passes independently from the repository root. Seventeen benchmarks were
  ignored and the known historical filter-reference failure was explicitly
  excluded, not waived. Existing non-Metal Apple build warnings remain.
- Remaining overall gates are watercolor's display-only selection edge policy,
  historical filter-reference reconciliation, rare presentation outliers,
  final integrated validation and GTK human approval. Other hosts compile the
  shared controls and renderer; no new physical-device validation is claimed.

### Filter-reference reconciliation

- The historical reference mismatch is resolved on the Vulkan validation host.
  The unmodified pre-migration renderer (`7719e6b`) reproduces the old fixture
  exactly. Applying only the corrected sRGB import to that old renderer makes
  its unfiltered image and all 160 filter/scope cases exactly match the current
  renderer. The discrepancy came from the earlier hardware-decoding importer;
  no filter algorithm change was needed.
- The replacement reference is generated by that independent old renderer,
  not today's code under test. Its one-byte tolerance is unchanged, and the
  test now validates the input against the independent transfer oracle first.
  See [reference provenance](../crates/layer-render-wgpu/tests/fixtures/README.md)
  and [the comparison results](reference/runtime-filters.md#import-contract-and-reference-reconciliation).
  The obsolete PNG is recoverable in Git history. Temporary baseline sources,
  generated diagnostics and machine-specific output remain outside the commit.
- No production renderer code changes or additional runtime work. Metal and
  browser WebGPU still need to run the new reference; GTK/Vulkan evidence does
  not establish complete cross-backend parity. Remaining GTK goal gates are
  watercolor's display-only selection edges, rare presentation outliers,
  final integrated validation and human review.
- The full Vulkan renderer suite now passes 110 tests with zero failures and
  zero exclusions; 17 hardware benchmarks remain separately ignored. Strict
  renderer Clippy passes. The replacement sheet was visually inspected.

### Active presentation phase and driver-stall investigation

The historical synthetic sweeps in this section used a polling/sleeping test
loop. See **Native event-wait correction** below for the corrected harness;
do not pool their timer-lateness statistics or infer production event-loop
delay from the old polling results. Separately captured native input and driver
traces are not changed by this correction.

- The test-only timing report now separates elapsed stage times from actual
  worker-thread CPU time (`CLOCK_THREAD_CPUTIME_ID`). Acquire, composition,
  encoding, submission, feedback and presentation can be correlated by frame
  ID. These counters, their storage and GPU queries are absent from application
  builds. `LAYER_PACING_GPU_TIMESTAMPS=0` additionally removes query submissions
  from the native pacing test; the summarizer keeps these runs separate.
- The recurring early stall is reproducible without GPU timestamp queries:
  one unprofiled panning run spent 22.844ms in submission (4.671ms thread CPU),
  with a 28.504ms presentation gap. Earlier queried runs placed the corresponding
  event in presentation instead. Native syscall stacks identify memory cleanup
  inside the NVIDIA Vulkan queue submit/present calls, not GTK rendering or an
  application destructor. The observed calls are unmap-DMA, unmap-memory and
  free in NVIDIA's [published ioctl definitions](https://github.com/NVIDIA/open-gpu-kernel-modules/blob/main/src/nvidia/arch/nvalloc/unix/include/nv_escape.h).
  Context-switch evidence also records blocking, not just preemption. This
  identifies the executing path, not which allocations originally triggered
  driver retirement or a proven driver workaround. The stall is **not fixed**.
  Profiler/tracer overhead is excluded from the benchmark numbers. Tools were
  extracted only into temporary storage; raw traces and machine details are
  not repository artifacts.
- The broader sweep exposed a separate, actionable pacing problem. Transforming
  accumulated wet artwork with the ordinary numeric controls updating delivered
  only 104.35Hz, despite CPU/GPU p99 spans of 2.567/5.368ms. Even the simple
  rectangle case delivered 114.84Hz with controls, versus 119.96Hz with the side
  controls hidden. Passing an isolated shader budget did not prove a smooth
  composited application.
- A running timer previously noticed changes in refresh **rate**, but not a
  newly available/corrected presentation **phase**. It now realigns against
  updated compositor feedback while input remains active. Circular phase error
  avoids a wraparound mismatch; changes below 1/16 refresh are ignored to avoid
  rearming for small feedback jitter. Tests cover idle, initial unknown phase,
  rate changes, phase shifts during activity and both directions of jitter.
- Phase correction alone did not leave enough room for the wet transform plus
  native controls. The Wayland deadline now reserves three quarters of a
  refresh instead of half for canvas, GTK and compositor work together. At
  120Hz this adds 2.083ms of lead time, not a frame of queued history. No view
  update throttling, input dropping, extra canvas allocation, GPU readback or
  additional renderer thread was introduced. Experimental deadline overrides
  were removed. The clock arithmetic adds only atomic reads on the main thread.
- Serial six-second full-workspace runs, 384px brushes; later workloads retain
  earlier artwork. Before/after refer to the same seven-workload sweep, with
  real child-surface presentation feedback and no simultaneous build/profiler:

  | Workload | Before Hz | After Hz | After CPU median/p95/p99 ms | After GPU span median/p95/p99 ms |
  | --- | ---: | ---: | --- | --- |
  | G-Pen | 119.956 | 120.008 | .361/.630/.873 | .163/.410/2.249 |
  | Natural Blender | 119.789 | 119.797 | .733/1.291/1.608 | .718/1.535/3.310 |
  | Wet Round | 119.790 | 120.003 | .629/1.218/1.721 | .518/1.220/1.907 |
  | Watercolor | 119.904 | 120.006 | 1.300/2.534/3.172 | 1.536/4.135/4.985 |
  | Pan | 120.012 | 119.864 | .209/.491/.710 | .104/1.275/2.281 |
  | Hand | 120.009 | 119.824 | .284/.540/.762 | .093/.312/1.935 |
  | Transform accumulated artwork | 104.347 | 119.507 | 1.664/2.357/2.773 | 1.937/2.709/3.304 |

  Transform discards fell from 93 to four. Other workloads discarded zero or
  one presentation each. Median enqueue-to-presentation after the change was
  5.375–5.656ms, with p99 6.173–8.877ms; the transform maximum was 11.263ms.
  This is a measured latency/headroom tradeoff, not a free speedup or a guarantee
  for every frame. Synthetic input is not a physical stylus measurement, and
  device timestamp spans include queue bubbles, not only shader execution.
  CPU/GPU tails were higher for several brushes in this repeat; they are
  reported, not dismissed as free. No shader or raster algorithm changed.
- A timestamp-free repeat delivered 119.73–120.01Hz for brushes/navigation,
  and 118.67Hz for transforms (10 discarded presentations). A two-thirds lead
  candidate had reached 119.84Hz with queries, but only 117.67Hz without them.
  The retained change improves composited transforms substantially without
  throttling controls, but does **not** establish a no-missed-frame guarantee.
  Remaining performance work includes the driver-retirement trigger and the
  residual transform presentation misses; watercolor selection-edge behavior
  and final human GTK review remain separate goal gates.
- Post-merge validation with the shared estimated-input correction work passes
  all 306 core/engine/UI/host tests and strict GTK/UI/host Clippy. The release
  application and test executable build. The native default-workspace workflow
  passes again, including fresh dark/light and compact captures; the default
  dark capture was visually inspected. The timer arithmetic regression passes.
  A new timestamp-free seven-workload sweep delivers 119.80–120.01Hz for brushes
  and navigation, and 118.50Hz for transforms (10 discarded presentations).
  Transform worker elapsed median/p95/p99 is 1.649/2.286/2.618ms; GTK dispatch
  p99 is 1.203ms. Pan has zero discarded presentations, 0.570ms worker p99,
  0.078ms input-handler p99 and 0.070ms GTK-dispatch p99. These are synthetic
  input runs with actual presentation feedback, not physical-input acceptance.
  The incoming Apple report separately confirms that the sRGB import oracle
  passes on Metal but the v3 filter-output reference fails (maximum channel
  error 255); see [Apple acceptance](history/apple-acceptance.md). That cross-backend
  failure is not waived by the passing Vulkan or GTK checks.

### Filter storage correction after integration

- The collaborator's independent Curves/Exposure and Halftone color tests fail
  on the hardware Vulkan host, isolating a real reliance on implementation-
  defined UNORM conversion. Explicit nearest rounding at each physical filter
  output fixes them, including six alpha levels. The shared wrapper covers
  previews, multipass and fused output without quantizing between fused filters
  or adding passes/resources/preparation work. No ordinary brush shader changes.
- The pre-migration renderer with the same import and rounding contracts supplies
  a new v4 reference. Only four of 1,966,080 current/reference channels differ,
  each by one byte; no tolerance is relaxed. The old v3 PNG is recoverable in Git.
  See [provenance](../crates/layer-render-wgpu/tests/fixtures/README.md).
- Three paired benchmark repeats show approximately 0.014ms (2.6%) extra GPU
  time for a full-image five-filter edit, with effectively unchanged small-region
  painting. GPU p99 stays under 0.681ms; CPU tails vary in both directions and
  are reported in [the full comparison](reference/runtime-filters.md#explicit-filter-storage-conversion).
  This is not a zero-cost or complete presentation-latency claim.
- The scalar tests also pass on software Vulkan. Full-image differences across
  backends remain after rounding and require further investigation; Metal and
  browser WebGPU have not run v4. This milestone does not declare that gate done.
- The final complete hardware Vulkan suite passes 112 tests with no failures
  or exclusions (17 benchmarks separately ignored), including the v4 reference.
  The reference sheet was visually inspected; the expanded software alpha
  oracles also pass. No raw machine profiles or third-party images are included.
- Strict renderer/GTK Clippy, WebAssembly checking and the GTK release rebuild
  pass. Native runtime package loading (all forty plus Tent Blur) and the full
  forty-filter picker/property workflow pass; the fresh Curves control capture
  was visually inspected. These checks preserve the separate remaining gates:
  watercolor selection-edge appearance, residual presentation stalls/misses,
  broader backend numerical parity and final human GTK review.

### Watercolor halo isolation

- A new mixed-media regression reproduced a concrete edge error: putting an
  opaque dry red mark beside blue watercolor changed a blue halo pixel from
  `[0, 0, 255, 12]` to `[255, 0, 0, 33]`. The morphology used wetness correctly,
  but its strongest-color search included unwetted pigment. Color candidates
  now come only from occupied wet samples. The unused sampling argument is
  removed; there are no new passes, channels, allocations or CPU pixel work.
- Eight synthetic cases cover all four stencil directions inside a tile and
  across tile boundaries. Every pixel outside the dry mark stays unchanged;
  tight dirty-region updates equal full recomposition. The diagnostic sheet
  was visually inspected: the first row contains blue watercolor alone, the
  second adds the separate red marks. It deliberately uses a strong, wide edge
  on tiny dabs to expose sampling errors, not as a representative brush preset.
  Generated images remain ignored under `artifacts/brush-validation/wet-edge/`.
- A separate GPU test verifies that a live layer mask clips both watercolor
  pigment and the outside band without modifying the stored pigment or wetness.
  Disabling or deleting that mask restores the exact original image. This
  validates an explicit final-appearance mask, not automatic per-stroke halo
  clipping to a past drawing selection.
- [CSP distinguishes brush and whole-layer watercolor edges](https://support.clip-studio.com/en-us/faq/articles/20200061),
  [describes an outside-stroke band](https://help.clip-studio.com/en-us/manual_en/810_subtools/W.htm),
  and [supports masks created from selections](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm).
  These support the separate painting/effect/mask concepts; they do not prove
  CSP's exact selection-clipping policy. No claim of that behavior is inferred.
- Three serial, alternating before/after benchmark pairs use the existing
  2048×1536 selected-brush workload: eight 384px dabs per update, forty warmup
  updates and 120 measured updates. Each table cell is the median of three
  per-run median/p95/p99 values, not a pooled distribution; units are ms.

  | Watercolor workload | CPU before | CPU after | GPU before | GPU after | Completion before | Completion after |
  | --- | --- | --- | --- | --- | --- | --- |
  | No selection | .366/.493/.752 | .362/.622/1.012 | .413/.416/.417 | .412/.414/.424 | .855/1.091/1.247 | .849/1.111/1.534 |
  | Selection | .353/.365/.384 | .358/.538/.702 | .462/.475/.479 | .461/.462/.472 | .874/.959/.983 | .877/1.138/1.220 |

  GPU medians are effectively unchanged. CPU and completion tails are higher
  in these samples, despite no CPU scheduling change; the cause is not proven
  and this is not a zero-cost claim. The largest post-change per-run completion
  p99 is 1.823ms, below the 8.33ms budget. Explicit GPU-completion benchmarks do
  not establish compositor or physical input latency.
- The complete GPU suite passes 113 tests without exclusions (17 benchmark
  tests separately ignored), followed by the additional live-mask test passing
  independently. Remaining gates are still strict selection-edge appearance,
  rare presentation stalls/residual transform misses, cross-backend filter
  numerical parity and final GTK human review.
- After merging the Apple UI/validation milestone, the release GTK application
  rebuild and native selected-brush workflow pass (G-Pen, Wet Round, Watercolor,
  selection inversion and undo/redo). Fresh dark/light captures were visually
  inspected. Strict renderer/GTK Clippy and the WebAssembly check pass. The
  incoming Metal report now tests v4: 111 tests pass and the strict reference
  still fails, with maximum channel error 47; it is no longer an untested v4
  backend. The detailed counts remain in [runtime filter validation](reference/runtime-filters.md).

### Transform resource reuse

- Live transforms now share the renderer's existing upload belt for color,
  material wetness, watercolor wetness and mask parameters. They previously
  bypassed it through `Queue::write_buffer`, which
  [allocates fresh native staging memory](https://docs.rs/wgpu/30.0.0/wgpu/struct.Queue.html#method.write_buffer).
  Uniform offsets, shader math, damage and submission ordering are unchanged.
  There is no additional pool, image channel, wait or per-transform buffer ring;
  the existing belt grows only as needed. Web retains its direct byte-transfer
  implementation through the same upload abstraction.
- The low-level transform module is internal. Its eager constructors are now
  test-only; native and web hosts use the existing staged compiler rather than
  being offered a second public transform startup path.
- All 114 hardware Vulkan renderer checks pass, including the strict v4 filter
  reference, ordered transforms, selections, masks and wetness; 17 performance
  tests remain separately ignored. Strict renderer/GTK Clippy and the WebAssembly
  build check pass. The current default-workspace workflow also passed before
  the upload-only edit; fresh default dark and compact light captures were
  visually inspected.
- After the upload change, the native GTK Operation-tool workflow and release
  application rebuild pass, including linked/unlinked mask transforms and
  compact controls. No new physical-tablet or non-Vulkan device run is claimed.
- A first serial isolated before/after comparison measures an additional
  0.002–0.003ms GPU span because the upload copy is now inside the measured
  encoder. Selected 48-tile CPU median/p95/p99 is .160/.198/.413ms before and
  .155/.185/.320ms after; completion is .304/.374/.558ms and .299/.372/.483ms.
  These are single runs, not a statistical speedup claim. Shader work is unchanged.
- A first timestamp-free full-workspace pair measures transform presentation
  at 116.01Hz before and 117.84Hz after (25 versus 14 discarded presentations).
  Worker median/p95/p99 is 1.408/2.082/2.348ms versus 1.451/2.196/2.553ms;
  the CPU tails did not improve. Pan remains 120.01Hz with no discards in both.
  This removes a known allocation source, **not** the remaining driver-stall or
  consistent-120Hz gate. Further paired measurements are needed. Raw captures
  and timing reports remain outside the tracked tree.

- A separate GPU identity regression confirmed that repeating an already-warmed
  transform path recreated paint pages on its third traversal. Retired preview
  pages now stay in transaction-local typed spare lists. The next changed region
  reuses and clears them; apply/cancel/reset releases the spare GPU resources.
  Original sparse pages are never recycled. Pigment, material water, watercolor
  water and visibility masks retain their distinct existing formats/semantics.
  Capacity follows peak live footprints, not the accumulated area visited.
- Repeated-path tests cover all three painting modes and linked/unlinked masks,
  including inverted/default-visible masks. Their third traversal creates no new
  page identities, output matches independent replay, cancellation restores exact
  sparse pigment/wetness, and apply/cancel leave zero spare-page bytes.
- Diagnostics now includes spare pages. The latency benchmark's previous stable-
  storage assertion covered only captures/uniforms, not temporary destination
  pages; that assertion is retained with its precise name, and page memory is
  measured separately. In the 2048×1536 test, spare peaks are zero for G-Pen,
  1.25MiB for Wet Round and 2.5MiB for Watercolor, with or without a linked mask.
  Peak paint/mask/transform storage is respectively 32.82MiB, 37.07MiB and
  44.88MiB for Wet Round, Watercolor and linked Watercolor. These totals include
  spares and exclude the compositor and driver allocations. No full-document
  spare image or additional persistent channel is allocated.
- Three alternating before/recycled benchmark pairs were attempted using the
  same six cases, forty warmups and 120 measured updates. Two completed fully:
  the table reports the median of those two runs' median/p95/p99 values in ms,
  not pooled percentiles. "Before" already includes the shared upload fix.

| Transform | CPU before | CPU recycled | GPU before | GPU recycled | Completion before | Completion recycled |
| --- | --- | --- | --- | --- | --- | --- |
| G-Pen, whole | 0.179/0.198/3.139 | 0.185/0.256/1.607 | 0.118/0.119/0.120 | 0.117/0.119/0.121 | 0.342/0.415/3.356 | 0.356/0.448/1.830 |
| G-Pen, selected | 0.185/0.222/0.365 | 0.182/0.216/0.329 | 0.114/0.115/0.127 | 0.111/0.112/0.112 | 0.352/0.421/0.576 | 0.337/0.422/0.558 |
| Wet Round, selected | 0.304/0.421/0.687 | 0.294/0.369/0.709 | 0.179/0.193/0.211 | 0.175/0.192/0.211 | 0.554/0.704/0.935 | 0.534/0.643/0.936 |
| Watercolor, selected | 0.443/0.696/1.153 | 0.421/0.582/0.750 | 0.198/0.216/0.261 | 0.196/0.209/0.261 | 0.732/1.027/1.453 | 0.693/0.877/1.091 |
| G-Pen, linked mask | 0.630/0.894/1.377 | 0.694/1.071/1.464 | 0.526/0.550/0.567 | 0.523/0.531/0.539 | 1.254/1.550/2.079 | 1.312/1.707/2.260 |
| Watercolor, linked mask | 2.367/4.096/4.992 | 2.318/3.965/4.627 | 1.351/1.392/1.520 | 1.373/1.480/1.525 | 3.885/5.852/6.479 | 3.877/5.741/6.552 |

  The third before run failed the unchanged 8.333ms assertion on its first
  G-Pen case (10.623ms completion p99), so its other five cases did not run.
  The third recycled run completed all six; linked Watercolor reached 8.114ms
  completion p99. Neither run is hidden or pooled into the two complete pairs.
  CPU/GPU tails vary and some increase; these runs prove neither a universal
  speedup nor elimination of rare driver stalls.
- The next timestamp-free full-workspace sweep with both changes delivers
  119.33Hz for accumulated-artwork transforms (six discarded presentations),
  versus 117.84Hz with upload reuse alone. Transform worker median/p95/p99 is
  1.625/2.349/2.594ms; GTK dispatch p99 is 1.168ms. Pan and Hand remain 120.01Hz
  with no discarded presentations. This is actual child-surface feedback with
  synthetic input, not a physical-tablet or no-missed-frame guarantee. The
  remaining presentation, watercolor-edge, backend-parity and human-review
  gates stay open.
- Final combined validation passes all 114 hardware renderer tests, strict
  renderer/GTK Clippy, the WebAssembly check and native GTK Operation workflow.
  The matching mask/pigment review capture was visually inspected. No raw
  hardware logs or generated screenshots are part of this milestone.

### Presentation dependency investigation (not a shipped change)

- API tracing found three Vulkan submissions per ordinary frame: document,
  viewport, then another presentation transition. A separate CPU-only call
  counter, without Nsight or timestamp queries, confirms 300 submissions for
  every 100 warmed presentations. It is not profiler-injected work.
- In wgpu 30.0.1, queue submission already transitions the surface to `PRESENT`.
  [Presentation preparation](https://docs.rs/wgpu-core/30.0.1/src/wgpu_core/device/queue.rs.html)
  asks for that same state again, but Vulkan's ordered-usage mask excludes it;
  the tracker therefore produces another barrier and submission. Adding an
  explicit transition in the app did not remove the duplicate and was reverted.
- A temporary dependency experiment adds only `PRESENT` to Vulkan's ordered
  texture usages. It reduces the independent counter to 200 submissions per
  100 presentations. This does **not** relax write-after-write barriers such as
  color attachments, which [require explicit ordering on Vulkan](https://github.com/gfx-rs/wgpu/issues/8853).
  No dependency fork, registry-source edit, unsafe presenter bypass, or app-side
  workaround is retained in this repository; the released lockfile is restored.
- A serial, unprofiled full-workspace pair uses the same seven six-second
  workloads and accumulated artwork. CPU columns are median/p95/p99 in ms;
  these are single runs, not pooled statistics or a universal speedup claim.

| Workload | Released CPU | Experimental CPU | Released Hz / discarded | Experimental Hz / discarded |
| --- | --- | --- | --- | --- |
| G-Pen | .253/.509/.670 | .234/.493/.638 | 120.01 / 0 | 120.01 / 0 |
| Natural Blender | .634/1.099/1.284 | .556/1.241/1.440 | 119.95 / 1 | 119.96 / 1 |
| Wet Round | .541/1.012/1.185 | .494/1.007/1.292 | 120.00 / 1 | 120.01 / 0 |
| Watercolor | 1.105/1.910/2.385 | 1.033/1.828/2.274 | 120.00 / 1 | 120.01 / 1 |
| Pan | .202/.390/.503 | .187/.381/.466 | 120.01 / 0 | 120.01 / 0 |
| Hand | .202/.412/.496 | .185/.380/.506 | 119.92 / 0 | 119.92 / 0 |
| Transform | 1.403/2.088/2.404 | 1.388/2.161/2.539 | 115.51 / 28 | 119.00 / 7 |

- Median presentation-call time falls by roughly .01–.02ms. Some CPU tails
  increase, and a 1.963ms presentation outlier remains. The test proves the
  redundant submission, not elimination of rare driver stalls or reliable
  120Hz transforms. No shader-only GPU timings are inferred from these runs.
- Khronos validation of the native Operation workflow reports SPIR-V
  `ArrayStride` storage-class errors (`VUID-StandaloneSpirv-None-10684`) on the
  unchanged baseline as well as the discarded app-transition candidate. Both
  workflows finish, but that is **not** a clean Vulkan-validation result.
  A separate temporary executable using only released wgpu, with no Capy
  renderer, reproduces both errors: a scalar compute shader is clean, while
  four-element function and workgroup arrays each trigger that VUID. Naga
  decorates array types with `ArrayStride` without separating these storage
  classes; wgpu selects SPIR-V 1.6 on this Vulkan 1.3 device. This matches the
  open [upstream Naga issue](https://github.com/gfx-rs/wgpu/issues/7696).
  No app shader rewrite, validator suppression or dependency fork is added.
  Successful pipeline creation is not evidence of valid generated SPIR-V;
  this dependency limitation remains explicit in validation results.

### Watercolor selection transport validation

- An independent donor-color regression found that wetness transport honored
  source selection coverage, but the equal-wetness color-mixing term did not.
  In its initial 128×128 case, an excluded red neighbor changed 3,267 selected
  blue pixels, with a maximum channel difference of 100/255, without brush
  motion. The mixing term now uses the already-sampled donor coverage. No new
  pass, channel, texture sample, allocation or CPU pixel work is introduced.
- Eight boundary cases cover horizontal/vertical selections, both inversions,
  and inside-page/cross-page boundaries. Selected pigment matches the independent
  blue-neighbor control exactly, unselected pigment/water remains unchanged,
  and an unrestricted control still mixes. The generated three-column diagnostic
  sheet was visually inspected; it remains ignored under
  `artifacts/brush-validation/wet-selection/`.
- Three serial alternating before/after pairs use the existing 2048×1536,
  eight-dab, 384px selected-brush benchmark, with forty warmup and 120 measured
  updates. Cells are the median of three per-run median/p95/p99 values, in ms,
  not pooled percentiles. Completion includes the explicit benchmark GPU wait.

| Watercolor | CPU before | CPU after | GPU before | GPU after | Completion before | Completion after |
| --- | --- | --- | --- | --- | --- | --- |
| No selection | .359/.457/.527 | .379/.720/.960 | .412/.414/.414 | .412/.414/.425 | .851/.950/1.020 | .859/1.211/1.439 |
| Selection | .364/.565/1.016 | .480/.688/.868 | .461/.462/.463 | .468/.470/.478 | .902/1.092/1.547 | 1.020/1.235/1.392 |

  Selected GPU median increases 0.007ms (about 1.5%); the kernel performs one
  additional coverage multiplication per neighbor. CPU/completion timings also
  vary, including increases; no zero-cost or universal speedup claim is made.
  The slowest individual post-change completion p99 is 2.153ms, below 8.33ms.
  These measurements do not prove presentation rate or physical input latency.
- All 115 hardware renderer tests pass, with seventeen benchmarks separately
  ignored. GTK selected-paint/mask and full default-workspace workflows pass;
  mask creation consumes the selection, preserves stroke history, and restores
  the prior checkpoint on undo. Masked dark/light and representative full/compact
  default captures were visually inspected. The WebAssembly check passes.
  Rare presentation stalls, residual transform frame misses, strict cross-backend
  filter parity and final human GTK approval remain separate open gates.

### Combined-submission experiment (rejected)

- A temporary append-to-frame API encoded document and viewport commands into
  one command buffer. The independent Vulkan counter fell from three to two
  submissions per frame without modifying wgpu. Pixel checks passed for both
  surface formats, including new wet pigment, camera-only changes, overviews
  and rejection of invalid frames before presentation.
- Two unprofiled full-workspace pairs were run in opposite orders, with GPU
  timestamp queries disabled. Each uses the same seven six-second workloads;
  transform runs retain artwork from preceding brush cases. Selected results
  below are worker median/p95/p99 ms and actual child-surface feedback.

| Pair / workload | Separate CPU | Combined CPU | Separate Hz / discarded | Combined Hz / discarded |
| --- | --- | --- | --- | --- |
| 1 / Pan | .222/.421/.521 | .149/.348/.477 | 119.86 / 1 | 120.01 / 0 |
| 1 / Transform | 1.463/2.208/2.534 | 1.424/2.154/2.363 | 119.34 / 6 | 117.67 / 14 |
| 2 / Pan | .185/.388/.497 | .238/.471/.640 | 119.90 / 1 | 119.81 / 2 |
| 2 / Transform | 1.416/2.285/2.638 | 1.521/2.153/2.415 | 114.94 / 31 | 117.52 / 15 |

  The first pair reduces CPU time for every workload, but the reverse-order
  pair does not confirm that benefit. Combined presentation still has a 15.328ms
  outlier. These results prove neither consistent improvement nor a consistent
  transform regression. Vulkan's wgpu backend also attaches the acquired-image
  semaphore wait at `TOP_OF_PIPE` to the entire surface-using submission; combining
  commands puts offscreen document work behind that wait. This is a concrete
  synchronization tradeoff, not proof that it caused the measured variation.
- The experimental API, host changes and its now-unused test are removed.
  There is no runtime switch or additional supported presentation path. Private
  reports and the rejected patch are outside the tracked tree. Fewer submissions
  alone do not justify retaining an API that has not improved end-to-end latency.

### GTK paint-color refresh

- Temporary main-thread profiling separates input, model refresh and GTK paint.
  Transform input p99 is 0.029ms; cloning the UI state is 0.044ms. Neither explains
  the 1.436ms GTK paint p99. Unchanged toolbar colors nevertheless reloaded CSS
  on every transform update, and unchanged color swatches requested redraws.
- Docked, Zen and drawer toolbar projections now share one palette per window.
  Only foreground/background changes reload its CSS. The Color panel similarly
  updates only when its color model changes, with explicit first initialization.
  This removes redundant native rendering work without changing core behavior,
  throttling visible values, or adding another GPU/presentation path.
- A serial baseline/change/baseline sweep uses the same seven six-second native
  workloads, no concurrent compilation and no GPU timestamp queries. Transform
  results below are median/p95/p99 milliseconds, not pooled statistics.

| Transform stage | Baseline | Cached colors | Baseline rerun |
| --- | --- | --- | --- |
| GTK paint | .788/1.229/1.438 | .406/.852/1.039 | .623/1.137/1.266 |
| Main-context dispatch | .017/.988/1.226 | .011/.505/.848 | .014/.748/1.135 |
| App frame handler | .175/.330/.394 | .111/.297/.342 | .137/.271/.314 |

  This confirms less GTK work, **not** reliable 120Hz transforms: actual transform
  presentation rates are 117.68, 111.84 and 113.17Hz respectively. Pan/Hand remain
  about 120Hz; individual brush CPU tails fluctuate, and the changed run still
  contains a 15.164ms watercolor worker outlier. No GPU-kernel improvement or
  elimination of presentation stalls is claimed. Temporary refresh-stage
  instrumentation has been removed; only the established benchmark remains.
- Palette initialization/change suppression, native color controls, nested tool
  drawers, selected watercolor/masks and the complete default-workspace workflow
  pass. Dark/light captures were inspected. The final hardware renderer suite
  passes 115 tests (17 separately ignored benchmarks); strict GTK/renderer Clippy,
  the WebAssembly check and the normal GTK release build also pass. This validates
  the shared WGSL change on Vulkan, not a new Metal/D3D12/device-parity claim.

### Native event-wait correction

- The synthetic pacing harness previously slept GTK's main thread for 1ms
  between event polls. That delays otherwise ready frame timers; the application
  itself uses GLib's native wait. The harness now uses one recurring 2ms input
  source and blocking main-context iteration, allowing frame, GDK and worker
  events to wake it immediately. This changes the test, not production input or
  rendering. Input remains synthetic and does not measure physical pen latency.
- Whole-dispatch measurements now use thread CPU time, excluding the native
  wait. Reports name this separately from the old elapsed dispatch measurement,
  record actual synthetic input frequency, and the summarizer separates the
  two harness versions. GPU queries remain independently optional.
- Serial query-free full-workspace sweeps compare the existing 3/4-refresh lead
  with a temporary 7/8 lead and then repeat the baseline. Each runs all seven
  six-second workloads; transforms retain the preceding wet artwork. Native
  event waiting reduces timer-lateness p99 from about 1ms to .019–.030ms in the
  baseline runs. Input delivery is 480–487Hz, with input CPU p99 .026–.054ms.

| Measurement | Baseline | Earlier scheduling | Baseline repeat |
| --- | --- | --- | --- |
| Pan presentation Hz | 120.009 | 120.003 | 120.008 |
| Transform presentation Hz / discarded | 118.673 / 9 | 118.003 / 13 | 117.510 / 15 |
| Transform CPU median/p95/p99 ms | 1.445/2.193/2.450 | 1.373/2.225/2.698 | 1.406/2.248/2.532 |
| Transform enqueue-to-presentation median/p95/p99 ms | 5.892/6.146/6.223 | 7.224/7.425/7.858 | 6.101/6.335/9.619 |

- Earlier scheduling does not demonstrate a consistent improvement and adds
  roughly 1ms of nominal latency. The override is removed; the existing clock
  policy and its phase/refresh tests remain unchanged. Correcting the harness
  improves the evidence, **not** the application's performance. Residual transform
  misses and occasional driver stalls remain open.
- A separate private Vulkan trace distinguishes submission-to-GPU waiting from
  execution: missed transform frames commonly wait longer to start, rather than
  simply executing an expensive shader. Profiling overhead prevents treating
  those spans as the unprofiled baseline or proving the cause of the wait.
  Batch-mode GPU timestamps were inconsistent with presentation and rejected;
  individual-workload tracing gave plausible spans. GTK and canvas used the same
  GPU. No GPU-affinity change, clock override, alternate backend or dependency
  patch is retained. Machine-identifying traces stay outside the repository.
- The rebuilt, override-free harness also passes a GPU-timed sweep. Pan delivers
  119.999Hz with CPU/GPU p99 .588/.466ms. Transforms deliver 116.843Hz with
  CPU/GPU-span p99 2.501/5.755ms and 21 discarded frames. The G-Pen and watercolor
  runs retain 17.785ms and 15.525ms worker outliers. GPU query submissions affect
  scheduling; these are separate results, not pooled with the query-free table.
  Four corrected native sweeps, the unchanged clock regression, report-generation
  separation checks and strict GTK Clippy pass. No production latency fix is
  claimed from this test-only correction.

### Integrated GTK visual audit: Navigator contrast

- The previous shader put a white surround behind the theme's outline color.
  In dark UI the outline is also light, making the work-area rectangle disappear
  against white artwork. A new hardware pixel test reproduces the all-white
  result before the fix; it now passes black/white artwork and outline colors,
  1×/2× display scales, and both explicit/hardware sRGB output paths.
- The shared WGSL chooses the more contrasting black or white surround from
  outline luminance in the vertex stage. No host-specific behavior, extra render
  pass, texture, readback or change to the image interior is introduced. The
  existing tests also preserve scrolling clips, window coverage, opacity,
  rotated/flipped camera geometry and live overview updates. Updated GTK
  Navigator captures show the complete rectangle over both paper and paint.
- Serial 2048×1536 viewport measurements below are total presentation GPU
  median/p95/p99 milliseconds, not isolated outline costs. The two changed runs
  retain the same medians; tails fluctuate rather than showing a consistent
  regression. CPU medians remain .008–.010ms for these cases.

| Overview case | Before | After | After repeat |
| --- | --- | --- | --- |
| Default | .036/.038/.038 | .036/.088/.109 | .036/.037/.037 |
| Two larger overviews | .045/.047/.048 | .045/.047/.048 | .045/.047/.060 |
| Panning | .043/.051/.068 | .043/.044/.046 | .043/.080/.087 |

The complete GPU suite and integrated GTK workflow audit pass. Navigator and
the complete default-workspace tests were rerun after the shader change.
Representative default, compact, three-tile, color, gradient, figure, mask-transform,
Navigator, collapsed-column and Zen captures were inspected across both themes.
The subsequent GTK design approval and final height adjustment are recorded at
the top of this document; the separate renderer investigations remain open.
