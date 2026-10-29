# Toolbar components

[Workspace and UI](README.md) · [Panel contract](panel-customization.md) ·
[Numeric controls](numeric-controls.md) · [Drag convention](drag-and-reorder.md)

Toolbars support **Brush size slider**, **Strength slider** and **Tool Options**. Add them through Add Tools like ordinary tiles. Each has a
stable tile identity; the entire component moves, copies, removes, docks, and
participates in workspace undo/redo as a single item.
Toolbars can also use [compact edge regions](compact-toolbar-edges.md).
They are not title-bar items.

## Included workspaces

Sketch centers Size and Strength in a compact toolbar on the left edge. Photo appends
Tool Options to its top commands toolbar, where it takes the remaining lane width.
Paint registers a separate Brush controls toolbar. Tablets show it below their
two-column tool grid, with values visible and both sliders expanded; desktop
Paint keeps it hidden. See [default workspaces](default-workspaces.md#arrangements).

## Numeric controls

Sliders occupy one lane and prefer four tiles of length, including tile gaps.
They work horizontally or vertically; vertical values increase upward. They
follow the active tool’s Size or Strength setting and disable when unavailable.
Multiple instances and existing panels share the same state and preset memory.
Size tracks widen toward larger values. Both orientations use equal end padding:
6px end insets and rounded ends, with the theme text color at 22% for size.
Strength tracks use a quiet gradient of the theme text color.
A 12×28px squircle thumb uses a 60% text border. The tracks have no
persistent numeric readout by default. Each slider's context menu offers
**Show value** and **Expand to fit**, independently for each copy. Show value adds
a number and its unit above a vertical track, or before a horizontal track.
The readout appears only when the slider is at least four tiles long along its
track axis. Shorter sliders give that space back to the track without changing
Show value; the readout returns when the slider grows.
Small vertical sliders put `px` or `%` below the number. Medium vertical sliders
also stack the unit when the value has three or more digits; shorter values and
other presentations keep them together on one line. The standard font size is
retained whenever it fits.
Numbers and units shrink together only as needed, using native glyph measurements,
with a 6-pixel content inset and the shared compound gap between them. Small and
Medium vertical headers reserve two control rows; horizontal headers reserve enough
width for the value and unit. Both sliders present the value directly, without a tip preview, extra
graphic, color or text shadow. The headers use the toolbar's existing surface
and normal typography, without inset cards, borders or value badges.
Labeled styles put the short setting name above the value.
Expand to fit gives the slider the available space in its toolbar row or lane.
Two vertical sliders in the same row stretch together when both use it. Place
them in a floating toolbar two tiles wide, resize its height, and enable both
options to make a pair of tall Size and Strength controls. The defaults keep
their compact length. Secondary-click the track or cap to open the menu;
touch and pen hold the cap. Holding the track keeps editing its value.
Dragging opens a rounded floating stamp preview
beside the slider, updates it continuously, and
closes it on release or cancellation. A tap keeps the preview open until an
outside tap or context change. The preview has the toolbar's tile radius and
no border. Its header is one row of that toolbar's tiles: the bookmark button
is a full tile with the tile's icon size in the top-end corner, and the value
and units are vertically centered beside it. The header fade scales with the
tile (65px on medium tiles) from 50% panel color to transparent. Size uses
the current tip at its document-pixel diameter, filling the popup to its rounded
edges for oversized tips. A background fade keeps the header legible;
Strength uses a fitted contact patch rendered by the canonical GPU
coverage evaluator at full physical pen pressure and the writing tilt pose. The
configured brush supplies shape, relief, paper texture, Strength and Opacity.
Dry Strength changes the material pressure, so fewer paper contacts appear at
lower values. Eraser, smudge, liquify and retouching tools show their shape and
soft edge filled at the selected Strength. They do not need a destination image.

The shared session owns one asynchronous preview request and coalesces value
changes. Clients retain the latest completed image while the next is pending.
Each 192-square alpha mask is delivered once; subsequent queries carry its
request ID and return metadata. A reopened or missing client image requests a
new mask. Context and renderer changes reject old results. Preview rendering
never changes artwork, stroke history or the document's painting resources.
Mask-to-image conversion runs on host workers. GTK takes ownership of the alpha
buffer directly; Web receives an ImageBitmap from its image-preparation worker.
Windows workers fill an unpublished bitmap buffer; the UI publishes the completed
image.
Idle metadata polling neither redraws the popup nor submits more GPU work.
Unavailable tools clear their persistent numeric readouts.

The preview's plus button bookmarks the value for that brush preset; the minus
button removes an existing mark. Marks are short lines perpendicular to the
track, placed inside the wider handle when selected. Tapping near a mark
recalls its exact value; dragging retains the full range without magnetic
snapping. Bookmarks persist in application settings and appear in every copy
of that slider. Size uses the shared logarithmic mapping and snaps to whole
pixels above 32. Strength uses whole percentages and its own bookmark
collection. Changes to brush values or
bookmarks do not create workspace-layout history entries.

## Contextual options

Rust derives the ordered form from existing tool settings, subtools and actions:
completion actions first, tool/variant choices, independent eyedropper sample
size, tool-specific fields (including tonal presets and intervals), selection
combination, sampling-source, family-ordered brush fields, then
remaining actions/toggles. A tool switch changes the form,
not the toolbar allocation or canvas size. Value changes retain native editors.
Each toolbar choice publishes one selected item. Brush variants select the
concrete preset; the Tool Set retains its selected parent group and preset.
GTK also presents shared list (single or multiple choice), text, and information
fields. Narrow bars use menu faces with native popover contents; their complete
form remains available through overflow. Within one tool context, adding or
removing fields retains compatible editors and open lists. A context change
discards old editors and their action bindings.

Brush forms share Size and Strength as their first numeric controls. Immutable
brush capabilities and execution family choose the remaining primary controls;
changing a value to zero does not change that layout. Dry media place Texture
strength before Material hardness. Bristles promote Load and Scale, wet paint
promotes Paint and Pull, watercolor promotes Water load and Wet edge, and smudge
promotes Pull. Ordinary brushes promote Hardness when applicable. A promoted
field appears once, without a category heading.

Smoothing follows the primary fields as one control without a category heading.
It ranges from 0 to 100, defaults to 6 and uses a logarithmic slider with 30 at
75% travel. Each brush retains its value; selection and coverage-mask tools omit
it. Command search labels the control Smoothing.

Secondary categories follow a shared order: Tip, Material, Texture, Bristles,
Mixing, Watercolor, Bleed, Liquify and Blending. Brush Color mixing joins the
secondary Mixing category in Tool Options. Opacity belongs to Blending, after
the material controls. Hosts use stable `group_id` values for category boundaries
and schema identity, with localized `group` text only for headings. Dry media
therefore show Size, Strength, Texture strength, Material hardness, then
Smoothing, Tip angle under Tip and Opacity under Blending. Contact softness is hidden and fixed in the
engine. Material hardness controls the pressure response of pigment loading.
Stable numeric rules retain editors across language changes; localized endpoint
labels do not form part of schema identity. Tool Settings and Tool Options
consume the same numeric order.
Shared projections that show a field alone, such as figure/fill Opacity or
selection Feather, remove both its category ID and heading text. An empty
heading cannot retain a category boundary.

The [canvas action bar](canvas-action-bar.md) presents the same `ToolOption`
actions and choices beside the object being edited. On the Web it builds them
with the same `actionField` and `choiceField` builders as Tool Options; a
segmented choice keeps its natural width there, and other choices open a
dropdown. Android uses the same `ToolOptionField` and `toolOptionSize` as its
Tool Options, with each item's short caption.
Android measures worded controls by summing the native pixel widths of the text,
icons, gaps and each padding edge, including at fractional display densities.

Horizontal numeric fields use label/icon, slider, then editable value, with
the label/icon outside the value field. Editing stays within the same footprint;
a tap outside accepts valid text and ends editing. Inputs and dropdowns use the
panel controls' typography and colors. Horizontal values reserve a fixed width
from the numeric range, signs, fractional boundaries and units, measured in the
native font and rounded up to a whole pixel; the value and its editor are centred
in it. Changing a value or entering an expression never shifts its neighbors.
Double-click a numeric label (or its
horizontal icon) to reset that setting to its tool/preset default. Icon mode
keeps horizontal dropdown labels readable. Vertical
options use flat icon choices and icon/value buttons; tapping a value opens
the standard numeric slider in a popover. Labeled tile styles
put the icon beside the label and value; the other styles stack icon and value.
Text follows the shared 11 pt typography; form icons stay 16px. Units sit
beside values on the same baseline and hide when space is tight. Only oversized
numbers in small tiles shrink to fit. Popovers close on context changes or teardown.
The tonal interval is one atomic field using the panel's two-ended range
component: compact one-decimal low/high values surround a wide track. Units are
in tooltips. Narrow bars expose the complete interval through existing overflow.
Floating toolboxes and side toolbars wider than one tile fill rows left-to-right.
Dividers take a full row; the options component occupies its own full-width
row and packs its compact controls in the same order. Drawer height measurement
uses these same rows. Short allocations clip trailing content and keep
visible insertion targets attached to their original tile IDs.

A divider is an 8-pixel slot (`TOOLBAR_DIVIDER_SIZE`). On GTK the line fills the
slot across the strip with a 9-pixel inset at each end (`TOOLBAR_DIVIDER_INSET`),
so it grows with wider toolbars and returns to 18 pixels in a Small single-column
toolbar. Horizontal toolbars apply the same inset vertically. Other hosts use a
centred line half a Small tile long (`DIVIDER_LENGTH`) in every tile size. Column
groups and the canvas action bar use the same divider; GTK builds all three with
`tiles::divider_slot` and `divider_line`, Apple and Android with `ToolbarDivider`, Web
draws `.tile-divider` from `divider_metrics`, and Windows uses `dividerLine` in
`apps/layer-windows/WorkspaceGeometry.h`.

Actions use the surrounding toolbar’s tile dimensions, centered beside the
shorter form fields. Hosts supply natural sizes and theme spacing; Rust fits
complete fields in order, reserving **More tool options** at the trailing end.
Fields taller than the remaining height enter overflow with their successors;
earlier fitting fields stay visible.
More always opens the complete tool/variant and settings drawer,
including actions that did not fit. Grouped tools include sibling variations and
the active tool's presets through the shared drawer projection. The drawer aligns to its right edge with a
standard gap, connector and corner treatment of other tool drawers. Multiple
horizontal Tool Options components share remaining space in their lane. Vertical components shrink before moving to another column. Components stay
atomic; child fields are never independent drop destinations.

Segmented choices retain connected icon buttons (for example New/Add/Subtract/
Intersect selection); list choices such as selection source remain dropdowns.
Choices with `labeled` put the shared label beside the bar, including Zoom Click
and Drag. Hosts right-align the joined icon bar in its row; each segment
is one small tile square (36×36 logical pixels) in Tool Options. GTK Tool Settings
panels and their drawers use 26×26-pixel segments with 16-pixel icons.
Joined segments share edges. Separate Tool Settings rows, including Zoom Click
and Drag, use the same 4-pixel gap as independent buttons beside each other;
see [panel spacing roles](README.md#compact-panel-spacing).
Tooltips and accessibility labels retain each option's full name. The
toolbar measures the label beside these segments and enters overflow as a whole.
Panel captions wrap to fit narrow docks.
Iconless checkable actions use a text checkbox, including Center on click.
The checkbox keeps its caption in Tool Options and moves into overflow
as a complete control when the lane is too narrow.
Grid choices can name a setting through `beside`; the settings panel places the
compact chooser next to that setting's numeric group under the choice label.
In horizontal toolbars, icon-only actions stay full tiles while every control with words or
values (dropdowns, radios, number fields, checkboxes) is the 26px panel control height with 5px
clear on every side, like the workspace switcher pills; Tool Options separates fields with the tile
gap. Bars keep their joined segments with 8px padding, 16px icons and the control radius on the
outer corners.
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
  editor supplies parsing and keyboard behavior. Slider input resolves the shared
  value and releases component-state borrows before changing the GTK scale;
  its synchronous value-change signal can dispatch an edit and refresh the component.
  Options retain each schema, editor and row together. Refresh owns these records
  while updating widgets, and previews use owned handles across native callbacks.
- Web `toolbar-components.js`, Android `ToolbarComponents.kt` and Apple
  `ToolbarComponents.swift` render the same owned component projection in ordinary
  toolbars and retained drawers. `toolbar_transport.rs` exposes stateless fitting,
  numeric metadata and formatting queries to Wasm, JNI and `capy_apple_toolbar_ui`;
  pointer timing/capture and font measurement stay native.
  `slider_layout` receives the tile and slider styles and returns `cap`, `track`
  and `show_value`; `slider_readout` publishes
  the formatted value, unit, spacing and unit placement. Every host measures its
  native font, keeps the standard size when it fits, and shrinks the value and
  unit together inside the shared content insets. Windows uses the same queries
  through `ToolbarComponents.cpp`. Toolbar structure caches include each copy's
  display preferences.
  The `choice_style` query resolves labeled choices to the shared Small tile
  metrics, independently of the surrounding toolbar style. GTK calls the same
  `tool_choice_style` helper directly. Captions and checkbox text retain native
  font measurement; Rust fits each complete field into the lane.
  Editors retain their original context token, and measurements are cached across
  value-only updates. The standalone slider uses the same shared cap/track geometry.
- `toolbar_preview.rs` owns bookmark validation, hit policy and completed contact
  plans. `CanvasRenderer` carries asynchronous stamp requests and readbacks; hosts
  retain the resulting image until a newer request completes.
- Numeric values, native measurements and overflow visibility are not saved in
  toolbar configuration. Typed component kinds, horizontal text/icon mode, slider visibility, and ordinary
tile IDs and per-slider display choices persist. Preferences participate in workspace undo/redo.

Slider tracks and option controls respond immediately to mouse, touch and pen.
Press, hold, then drag the slider’s empty leading cap or the options More button to
reorder the component. A quick cap drag never reorders. Holding a track remains
a numeric interaction. Disabled controls retain a draggable cap wrapper. Holding
empty Tool Options space with touch or pen opens its display menu; mouse uses
secondary click. Vertical options use icons; labeled choices retain their left caption.

Compact top/bottom options prefer sixteen tiles of length (side options eight),
then shrink to the available edge.

## Checks

GTK native input runs through `tools/performance/workspace-motion.sh gtk` on a
private Mutter display with `--native-test=` and one of these tests from
`apps/layer-linux/src/toolbar_component_tests.rs`:

- `native_paint_toolbar_dividers_input`: Paint tool order, growing and shrinking
  divider paint, orientation changes, retained widgets, input and drop bounds
  in both themes.
- `native_toolbar_components_input`: editing, hold-to-reorder caps, stamp
  previews and bookmarks, inline transform completion, context changes,
  placement and both themes. The
  `native_toolbar_components_pen_input` variant adds `--tablet`; its synthetic
  serials cannot authorize popup grabs, so popups are covered by mouse and touch.
- `native_toolbar_added_sliders_input`: Add Tools in Paint, then tap and drag
  the added Size and Strength sliders with mouse and touch in both themes.
- `native_toolbar_components_narrow_input` with `LAYER_MOTION_VIEWPORT=680x500`:
  overflow and full options at small widths.
- `native_toolbar_components_drawer_input`, `native_toolbar_options_presentation_input`,
  `native_toolbar_visual_audit_input`, `native_toolbar_value_controls_input`,
  `native_toolbar_rows_input`.
- `native_toolbar_slider_display_input`: secondary track menus, held cap menus,
  saved display choices, equal-height side-by-side sliders, live numeric readouts,
  mouse/touch input and unavailable tools in both themes. The
  `native_toolbar_slider_display_pen_input` variant uses `--tablet`. Set
  `LAYER_SLIDER_DISPLAY_MOTION=1 LAYER_NATIVE_EVENT_MS=4` on the mouse/touch journey for three sustained
  presentation measurements per slider and theme. Reference-tier measurements
  remain required before claiming the frame-rate targets.
- `native_toolbar_slider_design_gallery`: both orientations in Small, Medium,
  Large and both labeled styles, light/dark themes, red/blue/white/black paint,
  and minimum/maximum sizes. It captures the whole workspace and individual
  toolbar details; `LAYER_TEST_ARTIFACTS` selects the output directory.

Set `LAYER_NATIVE_TEST_EXECUTABLE` to the release test binary when iterating.
Web runs `tools/performance/workspace-motion.sh web --toolbar-components`, including
the slider display matrix and inline transform completion in both themes, or
`apps/layer-web/device.test.mjs --toolbar-components` on a tablet. Android runs
`AndroidInteractionTest#toolbarComponentsAcrossDevicesAndLayouts` and
`#toolbarEditorsAndOverflow` and `#toolbarSliderReadoutsAndTabletPaint`. The latter
checks the default tablet Paint stack and value/unit margins at all five tile
styles, both orientations and both themes. Apple runs the `toolbar_component` tests in
`cargo test --locked -p layer-apple --target aarch64-apple-darwin --lib` and the
`testToolbarComponents` journey on macOS and iPad.

Windows exposes the preview frame with the shared tile label, so accessibility
clients can identify its bounds and bookmark button.
Windows answers idle preview queries on the render owner without acquiring or
presenting the canvas. Input, visual changes and GPU recovery keep their normal
frame path. A full optional-query queue retries on the next preview tick.
Idle service deadlines remain independent of preview query arrivals.
