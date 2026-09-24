# Toolbar components (GTK)

[Workspace and UI](README.md) · [Panel contract](panel-customization.md) ·
[Numeric controls](numeric-controls.md) · [Drag convention](drag-and-reorder.md)

GTK toolbars support **Brush size slider**, **Brush opacity slider**, and
**Tool Options**. Add them through Add Tools like ordinary tiles. Each has a
stable tile identity; the entire component moves, copies, removes, docks, and
participates in workspace undo/redo as a single item.
Toolbars can also use [compact edge regions](compact-toolbar-edges.md).
They are not title-bar items. Other hosts do not yet offer these components.

## Included workspaces

Sketch centers size and opacity in a compact toolbar on the left edge. Photo appends
Tool Options to its top commands toolbar, retaining New/Open/Save, Undo/Redo,
Scale/Rotate, and removing Flip Horizontal, Clear Layer and Fill Selection.
Tool Options takes the remaining lane width. Other hosts and Paint retain their
previous defaults. Only untouched built-in Sketch/Photo layouts migrate; custom
baselines, copied workspaces and edited histories are preserved.

## Numeric controls

Sliders occupy one lane and prefer four tiles of length, including tile gaps.
They work horizontally or vertically; vertical values increase upward. They
follow the active tool’s size and opacity settings and disable when unavailable.
Multiple instances and existing panels share the same state and preset memory.
Size tracks widen toward larger values; opacity tracks show transparency over
a checkerboard. Values remain horizontal in both orientations.

Click the value to edit an expression inline. Drag a number up/down with touch
or pen along its slider's mapping, or scroll over it with a mouse. Size is
logarithmic, 0.5–2048 px, and
snaps to whole pixels above 32. Compact readouts omit decimals at three digits
and above; exact entry retains the shared numeric precision. Horizontal
readouts show units. Standalone vertical sliders omit units at every size.
Opacity sliders use whole percentages. Short allocations reduce the track before the value editor.
Tool, document and workspace changes cancel unfinished text. Numeric edits do not create workspace-layout history entries.

## Contextual options

Rust derives the ordered form from existing tool settings, subtools and actions:
completion actions first, tool/variant choices, independent eyedropper sample
size, selection combination and sampling-source choices, numeric fields, then
remaining actions/toggles. A tool switch changes the form,
not the toolbar allocation or canvas size. Value changes retain native editors.

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
Floating toolboxes and side toolbars wider than one tile fill rows left-to-right.
Dividers span the full width; the options component occupies its own full-width
row and packs its compact controls in the same order. Drawer height measurement
uses these same rows. Short allocations clip trailing content and keep
visible insertion targets attached to their original tile IDs.

Actions use the surrounding toolbar’s tile dimensions, centered beside the
shorter form fields. GTK supplies natural sizes and theme spacing; Rust fits
complete fields in order, reserving **More tool options** at the trailing end. That button always opens the complete tool/variant and settings drawer,
including actions that did not fit. The drawer aligns to its right edge with a
standard gap, connector and corner treatment of other tool drawers. Multiple
horizontal Tool Options components share remaining space in their lane. Vertical components shrink before moving to another column. Components stay
atomic; child fields are never independent drop destinations.

Segmented choices retain connected icon buttons (for example New/Add/Subtract/
Intersect selection); list choices such as selection source remain dropdowns.
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
- Numeric values, native measurements and overflow visibility are not saved in
  toolbar configuration. Typed component kinds, horizontal text/icon mode, slider visibility, and ordinary
  tile IDs persist. Preferences participate in workspace undo/redo.

Slider tracks and option controls respond immediately to mouse, touch and pen.
Press, hold, then drag the slider value cap or the options More button to
reorder the component. A quick cap drag never reorders. Holding a track remains
a numeric interaction. Disabled controls retain a draggable cap wrapper. Holding
empty Tool Options space with touch or pen opens its display menu; mouse uses
secondary click. Vertical options always use icons.

## Validation

Core regressions cover numeric bindings and invalid/stale edits, contextual
settings and completion actions, all five tile styles and both axes, flexible
allocation/overflow, GTK-only defaults, serialization, and conservative migration.
GTK native-input regressions run through
`tools/performance/workspace-motion.sh gtk` on a private Mutter display:

- `--native-test=native_toolbar_components_input`: mouse/touch editing and
  hold-to-reorder caps, exact expressions and errors, context changes, full-width
  values, inline sliders, vertical/floating placement, independent eyedropper
  choices, Apply/Cancel, and light/dark screenshots.
- `--native-test=native_toolbar_components_pen_input --tablet`: the same slider
  and reorder gestures with GDK tablet events in both themes.
- `--native-test=native_toolbar_components_narrow_input` with
  `LAYER_MOTION_VIEWPORT=680x500`: overflow and full options at small widths.
- `--native-test=native_toolbar_components_drawer_input`: retained toolbar
  drawer sliders, inline exact values, and cleanup on close.
- `--native-test=native_toolbar_options_presentation_input`: display preferences,
  tile action dimensions, dropdown alignment, and four-digit values in every style.
- `--native-test=native_toolbar_visual_audit_input`: both themes, all tile sizes,
  both orientations, label/value alignment and clipping, separate horizontal
  icons, inline editing and outside-tap dismissal, and drawer appearance.
- `--native-test=native_toolbar_value_controls_input`: mapped touch number
  drags, label resets, and vertical slider popovers.
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
