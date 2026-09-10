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
- Right: Navigator (Diagnostics as its next tab), Properties, Layers (Filters as
  its next tab), vertically stacked. Properties remains layer properties today;
  its name also suits future selected-object properties.
- Text, Comic and Correct line are explicitly excluded. Do not add inert buttons.

Dividers are actual toolbar items with stable identities, compact axis-aware
geometry, drag/reorder support and context customization, not disabled commands
or full-sized blank tiles. Preserve existing floating/docking/zen interactions.

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
  Each panel keeps its default width (three tiles plus normal insets for Tool
  Set/Tool Settings), respecting larger intrinsic minima such as Layers. Content
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
- Validation so far: 156 shared UI tests, 23 engine tests, workspace compile,
  strict UI/GTK Clippy, and isolated Wayland/GPU checks of all 24 brush control
  schemas at 128px. Native expression editors and color buttons exercised;
  dark/light HSV/HLS and narrow-panel captures in the ignored
  `artifacts/familiar-workspace/` directory.
- Still to implement: dynamic tool families/groups and the new default layout,
  toolbar dividers and tool/panel drawers above, missing canvas tools/commands,
  Navigator, rulers, shortcuts and full functional/performance validation.
- Implementation in progress; the requested default workspace is not yet shipped.
