# Toolbar components

[Workspace and UI](README.md) · [Panel contract](panel-customization.md) ·
[Numeric controls](numeric-controls.md) · [Drag convention](drag-and-reorder.md)

GTK, Web and Android toolbars support **Brush size slider**, **Brush opacity slider**, and
**Tool Options**. Add them through Add Tools like ordinary tiles. Each has a
stable tile identity; the entire component moves, copies, removes, docks, and
participates in workspace undo/redo as a single item.
Toolbars can also use [compact edge regions](compact-toolbar-edges.md).
They are not title-bar items. Other hosts do not yet offer these components.

## Included workspaces

Sketch centers size and opacity in a compact toolbar on the left edge. Photo appends
Tool Options to its top commands toolbar, retaining New/Open/Save, Undo/Redo,
Scale/Rotate, and removing Flip Horizontal, Clear Layer and Fill Selection.
Tool Options takes the remaining lane width. Paint and hosts without component views retain their previous defaults. Only untouched built-in Sketch/Photo layouts migrate; custom
baselines, copied workspaces and edited histories are preserved.

## Numeric controls

Sliders occupy one lane and prefer four tiles of length, including tile gaps.
They work horizontally or vertically; vertical values increase upward. They
follow the active tool’s size and opacity settings and disable when unavailable.
Multiple instances and existing panels share the same state and preset memory.
Size tracks widen toward larger values; opacity tracks show transparency over
a checkerboard. Both orientations use equal end padding. Every host draws
GTK's track: 6px end insets, rounded tapered ends, fills from the theme text
color (22% for size; 8% base, 20% checker cells and a 0–65% gradient for
opacity) and a 12×28px squircle thumb with a 60% text border. The tracks have no
persistent numeric readout. Dragging opens a rounded floating stamp preview
beside the slider, updates it continuously, and
closes it on release or cancellation. A tap keeps the preview open until an
outside tap or context change. The preview has the toolbar's tile radius and
no border. Its header is one row of that toolbar's tiles: the bookmark button
is a full tile with the tile's icon size in the top-end corner, and the value
and units are vertically centered beside it. The header fade scales with the
tile (65px on medium tiles) from 50% panel color to transparent. Size uses
the current tip at its document-pixel diameter, filling the popup to its rounded
edges for oversized tips. A background fade keeps the header legible;
opacity uses a fixed fitted stamp. The tip mask, aspect, rotation, hardness,
grain and dual-tip texture come from the configured brush.

The preview's plus button bookmarks the value for that brush preset; the minus
button removes an existing mark. Marks are short lines perpendicular to the
track, placed inside the wider handle when selected. Tapping near a mark
recalls its exact value; dragging retains the full range without magnetic
snapping. Bookmarks persist in application settings and appear in every copy
of that slider. Size uses the shared logarithmic mapping and snaps to whole
pixels above 32. Opacity uses whole percentages. Changes to brush values or
bookmarks do not create workspace-layout history entries.

## Contextual options

Rust derives the ordered form from existing tool settings, subtools and actions:
completion actions first, tool/variant choices, independent eyedropper sample
size, tool-specific fields (including tonal presets and intervals), selection
combination and sampling-source choices, numeric fields, then
remaining actions/toggles. A tool switch changes the form,
not the toolbar allocation or canvas size. Value changes retain native editors.
GTK also presents shared list (single or multiple choice), text, and information
fields. Narrow bars use menu faces with native popover contents; their complete
form remains available through overflow. Within one tool context, adding or
removing fields retains compatible editors and open lists. A context change
discards old editors and their action bindings.

Horizontal numeric fields use label/icon, slider, then editable value, with
the label/icon outside the value field. Editing stays within the same footprint;
a tap outside accepts valid text and ends editing. Inputs and dropdowns use the
panel controls' typography and colors. Horizontal values reserve a fixed width
from the numeric range, signs, fractional boundaries and units, measured in the
native font; changing a value or entering an expression never shifts its neighbors.
Double-click a numeric label (or its
horizontal icon) to reset that setting to its tool/preset default. Icon mode
keeps horizontal dropdown labels readable. Vertical
options use flat icon choices and icon/value buttons; tapping a value opens
the standard numeric slider in a popover. Labeled tile styles
put the icon beside the label and value; the other styles stack icon and value.
Text follows the shared 11 pt typography; form icons stay 16px. Units sit
beside values on the same baseline and hide when space is tight. Only oversized
numbers in small tiles shrink to fit. Popovers close on context changes or teardown.
GTK's tonal interval is one atomic field using the panel's two-ended range
component: compact one-decimal low/high values surround a wide track. Units are
in tooltips. Narrow bars expose the complete interval through existing overflow.
Floating toolboxes and side toolbars wider than one tile fill rows left-to-right.
Dividers span the full width; the options component occupies its own full-width
row and packs its compact controls in the same order. Drawer height measurement
uses these same rows. Short allocations clip trailing content and keep
visible insertion targets attached to their original tile IDs.

Actions use the surrounding toolbar’s tile dimensions, centered beside the
shorter form fields. Hosts supply natural sizes and theme spacing; Rust fits
complete fields in order, reserving **More tool options** at the trailing end. That button always opens the complete tool/variant and settings drawer,
including actions that did not fit. The drawer aligns to its right edge with a
standard gap, connector and corner treatment of other tool drawers. Multiple
horizontal Tool Options components share remaining space in their lane. Vertical components shrink before moving to another column. Components stay
atomic; child fields are never independent drop destinations.

Segmented choices retain connected icon buttons (for example New/Add/Subtract/
Intersect selection); list choices such as selection source remain dropdowns.
Horizontal bars keep one tile of width per choice but match the dropdown's 24px
height, control corner radius (a capsule at that height) and 16px icons.
The bar stacks on narrow side toolbars and moves into overflow as a whole.

## Ownership and implementation

- `layer-ui/toolbar_components.rs` owns bindings, contextual form metadata,
  stale-target validation and inner fitting. `layout.rs` owns component extents,
  wrapping, floating sizing, insertion markers and shared drop geometry. One
  min/preferred/fill allocator handles dividers and extended items, including
  natural-height measurement for tabbed and drawer toolbars.
- `ToolbarEdit` carries the original tool, brush, layer/mask, operation and
  document generation. Switching away and back cannot revive an obsolete edit.
  Existing action validation and document/workspace history remain authoritative.
- GTK `toolbar_components.rs` owns retained native widgets, measurement, focus,
  pointer capture and native dropdowns. Both normal toolbars and nested drawer toolbars
  use the same typed `TileWidget` builder and refresh path. The existing numeric
  editor supplies parsing and keyboard behavior.
- Web `toolbar-components.js` and Android `ToolbarComponents.kt` render the same
  owned component projection in ordinary toolbars and retained drawers.
  `toolbar_transport.rs` exposes stateless fitting, numeric metadata and formatting
  queries to Wasm/JNI; pointer timing/capture and font measurement stay native.
  Editors retain their original context token, and measurements are cached across
  value-only updates. The standalone slider uses the same shared cap/track geometry.
- `toolbar_preview.rs` owns bookmark validation, hit policy, and stamp geometry.
  A small raster is requested only when opening the popup, from immutable CPU
  tip assets through `CanvasRenderer::tip_mask`; dragging only scales and fades it.
  GTK hands reference-counted brush sources across its render-worker boundary
  alongside cursor outlines, including startup and document-color adoption.
- Numeric values, native measurements and overflow visibility are not saved in
  toolbar configuration. Typed component kinds, horizontal text/icon mode, slider visibility, and ordinary
  tile IDs persist. Preferences participate in workspace undo/redo.

Slider tracks and option controls respond immediately to mouse, touch and pen.
Press, hold, then drag the slider’s empty leading cap or the options More button to
reorder the component. A quick cap drag never reorders. Holding a track remains
a numeric interaction. Disabled controls retain a draggable cap wrapper. Holding
empty Tool Options space with touch or pen opens its display menu; mouse uses
secondary click. Vertical options always use icons.

## Validation

Core regressions cover numeric bindings and invalid/stale edits, contextual
settings and completion actions, all five tile styles and both axes, flexible
allocation/overflow, supported-host defaults, serialization, and conservative migration.
GTK native-input regressions run through
`tools/performance/workspace-motion.sh gtk` on a private Mutter display:

- `--native-test=native_toolbar_components_input`: mouse/touch editing and
  hold-to-reorder caps, stamp preview lifetime and bookmarks, context changes,
  inline sliders, vertical/floating placement, independent eyedropper
  choices, Apply/Cancel, and light/dark screenshots.
- `--native-test=native_toolbar_components_pen_input --tablet`: the same slider
  and reorder gestures with GDK tablet events in both themes. The proxy targets
  the main surface only, so popup buttons are activated through GTK in that
  journey; native popup hits are covered by mouse/touch and the Wacom hosts.
- `--native-test=native_toolbar_components_narrow_input` with
  `LAYER_MOTION_VIEWPORT=680x500`: overflow and full options at small widths.
- `--native-test=native_toolbar_components_drawer_input`: retained toolbar
  drawer sliders and cleanup on close.
- `--native-test=native_toolbar_options_presentation_input`: display preferences,
  tile action dimensions, dropdown alignment, and four-digit values in every style.
- `--native-test=native_toolbar_visual_audit_input`: both themes, all tile sizes,
  both orientations, label/value alignment and clipping, separate horizontal
  icons, inline editing and outside-tap dismissal, and drawer appearance.
- `--native-test=native_toolbar_value_controls_input`: slider previews and bookmarks,
  label resets, and vertical option popovers.
- `--native-test=native_toolbar_visible_edges_input`: compact docking at all
  twelve anchors when the visible toolbar reaches an edge before its handle.

Use the release test executable through `LAYER_NATIVE_TEST_EXECUTABLE` when
iterating. The tablet proxy exercises GDK pen input, not physical tablet hardware;
its synthetic serials cannot authorize native popup grabs, so popup journeys
run separately without that proxy.

The Photo default's command/options band is outermost at the top, above both
side columns, without a Flip Image tile. Only untouched included layouts migrate; custom layouts and their
history remain intact. Compact top/bottom options use a preferred length of
sixteen tiles (side options retain eight), then shrink to the available edge.
`native_toolbar_rows_input` verifies stable readout bounds across range changes,
all six horizontal compact anchors, and editing in floating/left/right toolboxes.

Web regression: `tools/performance/workspace-motion.sh web --toolbar-components`.
The same journey runs with `apps/layer-web/device.test.mjs --toolbar-components`
against a dedicated tablet test origin and forwarded Chrome debugger. It covers
mouse/touch/pen sliders, fixed value widths, segmented/list choices, all twelve
compact handle targets and history, all tile sizes, editor dismissal, and drawers.

Android regressions: `AndroidInteractionTest#toolbarComponentsAcrossDevicesAndLayouts`
and `#toolbarEditorsAndOverflow`, built with a separate application ID, on an
attached tablet. They use native mouse/finger/stylus MotionEvents and isolated
workspace stores. Screenshots cover both themes, standalone tracks, horizontal
options, vertical sizes, numeric popovers and the connected drawer. These are
injected native input journeys, not a hands-on physical stylus test.
