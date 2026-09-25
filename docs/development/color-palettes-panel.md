# GTK color palettes

GTK places the Palettes tab immediately after Color in the Paint and Photo defaults,
and the same component below the wheel in Sketch’s color drawer. Untouched
included layouts migrate; customized layouts keep their placement and can show
Palettes through the Window menu or the wheel’s Palettes action. Other hosts do
not expose the new panel yet.

## Layout and interaction

The component has no header controls. Its body contains one recent-color row,
a divider, and the saved palette. A second divider separates the footer, with the palette selector on
the left and the current color’s name, hex preview, and conditional EV on the
right.

- The minimum width is 280 logical pixels, fitting six 40-pixel tile targets.
  Wider panels add columns. The saved grid grows to four rows, then scrolls;
  two rows of minimum space keep the in-place chooser usable with empty palettes.
- The last history cell expands recent colors over the saved grid. The final
  visible cell collapses it. Expansion fits at most four rows within the existing
  body; neither overlay changes the footer position. Covered controls cannot
  receive pointer or keyboard input.
- The final saved-palette tile is **+**, which adds the current color. Saving or
  choosing a swatch does not update history. The added swatch becomes selected.
- Clicking the footer selector opens a chooser over the body. Search and a **+**
  menu sit at the top; that menu contains **New Palette…** and **Import Palette…**.
  Single-line palette rows show names, contiguous preview strips and selected-row
  highlighting. Secondary click, touch/pen hold, or the keyboard menu action
  opens rename, export and removal; rows have no ellipsis button. There is no
  Generate action.
- Choosing a palette closes the chooser. Escape dismisses the innermost editor
  or overlay before the enclosing drawer. Space/Enter retain native button
  activation rather than invoking canvas shortcuts.
- Sketch's color drawer follows the shared outside-contact dismissal policy.
  A canvas contact closes it without painting; reopen it to view updated history.

Clicking the color name opens an inline editor. Enter or valid focus loss commits;
Escape cancels. Saved color names are unique within a palette after trimming,
collapsing whitespace, and case-insensitive comparison. Duplicate edits keep the
previous name and show an error. Blank names receive a color-family name such as
Coral or Teal, with a numeric suffix if necessary. Imported duplicates and legacy
saved duplicates receive suffixes without changing IDs or definitions.

Naming an unsaved color prepares its name; only the + tile saves it. Choosing a
color independently never overwrites a saved definition. Swatches have a context
menu for rename and removal, available through secondary click, touch/pen hold,
and the keyboard menu action. Mouse holds never open menus. Saved color tiles
reorder immediately after native movement slop with mouse, touch, and pen,
the palette exception in [the drag convention](../ui/drag-and-reorder.md).
A tap still selects a color. Gaps, the scrollbar, wheel and trackpad scroll long
palettes; the tile body owns reordering. A stationary touch/pen hold opens a menu,
and the same contact can drag out of it; release without dragging keeps the menu.
GTK groups the tile's native hold and drag gestures and excludes unrelated
pointer-device events while a contact is active. This prevents synthetic mouse
hover events from cancelling a pen gesture when its menu appears.
The lifted swatch follows the original grab point over the whole workspace.
Neighbors slide for 140 ms into the shared core's proposed order, leaving a gap
at the destination. Native grid cells stay fixed for hit testing, so animated
neighbors cannot change the target under a stationary pointer. Retargeting starts
from each neighbor's current visual position; the native animation follows the
system's reduced-motion setting. Edge hovering scrolls long palettes on the
frame clock while the lifted swatch stays under the pointer. Moving outside the
grid previews the original order, and releasing outside cancels. Escape, focus loss, source changes, and cancelled contacts
retire the drag without changing the palette. Only a valid release commits.
Reorder undo/redo is scoped to each palette and available from the swatch menu
or Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y while a palette control has focus. Reordering
keeps color IDs, definitions, selection, and usage history unchanged. Adding or
removing swatches clears that palette’s reorder undo/redo; renaming does not.

The footer name and selector use the tool panels’ compact 24-pixel controls.
Search and inline editing share their input background and padding; the + menu
button is a 24-pixel square. Palette rows, color tiles and the + menu use native
GTK menu rows, separators and disabled states, with the same arrow-free
presentation as Layers. Swatches keep their 40-pixel targets. Radius, text, and
selection colors use existing panel theme values in both dock and drawer views.

Fitted dock groups reserve the largest measured minimum body height among their
tabs. Compact pages stay whole; scrollers use the existing four-row minimum from
floating drop sizing. Long hidden lists therefore do not inflate the group.
The color wheel therefore fits on workspace load, and switching to a shorter
palette page does not move the dividers. This shared group rule uses no tab-switch
size cache or panel-specific exception. Manual divider resizing still opts the
group out of content fitting. Floating groups also fit all their pages and retain
manual heights across tab selection. The palette body takes spare space with its footer
at the bottom.

The GTK defaults also place Proof after Navigator and Diagnostics after Tool Set.
Automatic tab labels use native measurements at the current font and width.
Every icon is reserved first, then complete names are restored left to right as
space permits. Selection does not change this priority. Explicit tab styles keep
their existing behavior, and the same fitting component is used in retained drawers.

## Starter palettes

GTK installs ten original editable starter palettes with 11–17 named sRGB
colors (two or three rows including the + tile at minimum width). Existing user
palettes and IDs are retained. A persisted installation marker prevents deleted
starters from reappearing. A pristine empty “My colors” palette is replaced;
other libraries receive the starters alongside existing work, within the limits.

| Palette | Colors | Intended style and organization |
| --- | --- | --- |
| Ocean Study | 17 | Muted coastal painting; water, clay and green value ramps |
| Pixel Arcade | 16 | Bright game sprites; short warm/cool hue groups and shared neutral anchors |
| Dark Fantasy | 15 | Gothic environments; cool stone, oxblood, moss and old metal, mostly dark values |
| Pop Art | 11 | Comic posters; bold red/yellow/blue families with ink and paper |
| Candy Pastels | 17 | Kawaii stickers and soft character art; warm/cool pastels with a small plum outline group |
| Riso Print | 11 | Zines and print illustration; pink/blue ink families, violets and warm paper |
| Synthwave | 17 | Neon night scenes; violet/pink, electric blue/cyan, hot sunset accents |
| Seventies Print | 11 | Retro illustration; brown/orange/mustard, avocado and dusty rose |
| Woodblock | 11 | Japanese print studies; a long indigo/blue group, vermilion, ochre and paper |
| Ink | 11 | Monochrome comics and value studies; a single neutral black-to-white scale |

The [starter research](color-palettes-research.md#starter-palette-curation)
compares popular artist palettes and print references. These values are original
curations, not copies of those artists' sets or simulations of physical inks.
Each set has a distinct value range, saturation and hue balance. Color families
stay adjacent in the saved sequence; wider grids wrap that sequence without
sorting it. Ocean Study retains the earlier demo's muted water, clay and greens.
There is no requirement that every palette contain three evenly shaded ramps.

Untouched palettes from earlier GTK reviews migrate using exact name/color/order
fingerprints. Edited or reordered copies are retained. Palette IDs and surviving
swatch IDs stay stable; shorter replacements remove only surplus untouched
starter swatches. Deleted palettes are not recreated.

## Color data and history

Palettes continue to belong to the workspace’s working state. Names, palette
selection, imports, and bounded recent colors are shared Rust state; native GTK
owns rendering, focus, menus, and file pickers. Workspace persistence retains
these values. This increment does not introduce an application-wide library.

History contains at most 64 exact definitions, newest first, deduplicated by
exact definition. Brush history is recorded at successful stroke commit using
the source definition captured with the brush. Selection, wheel/eyedropper
preview, cancellation, erasing, selection-mask painting, smudging, and liquify
are not color-use events. Successful fills, figures, and gradients record the
source definitions they use. Undo does not remove usage history.

Saved colors and history retain tagged RGB space, alpha, and HDR values. GTK
renders managed previews through the existing ColorPatch path. Hex is an sRGB
preview, not the storage format; HDR uses the picker’s base color and EV readout.
The full color name and space remain available in tooltips/accessibility text.

Imports accept GIMP GPL (sRGB) and native Capycolor (`.capycolor`) palettes.
Capycolor uses JSON internally and preserves color definitions exactly. Existing
native `.json` exports remain importable; new exports use `.capycolor`.
Files are limited to 1 MB; libraries retain the existing limits of 64 palettes
and 4096 total swatches. File reading is asynchronous and bounded, and failed
imports leave the library unchanged. Library mutations
retain existing tile widgets; brush frames do not rebuild the palette chooser.

## Validation and review

Focused shared checks include history event boundaries, naming, import rollback,
legacy loading, exact color round trips, default migration, and host availability.
The GTK input test exercises Sketch, Paint, and Photo, inline editing, overlay
geometry, search, menu placement, and managed color selection.

```sh
cargo test --locked -p layer-engine -p layer-ui -p layer-workspace
cargo build --locked --release -p layer-linux
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_panel_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_adaptive_panel_tabs
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_reorder_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_reorder_pen_input --tablet
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_context_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_context_pen_input --tablet
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_pen_input --tablet
bash tools/performance/workspace-motion.sh gtk --native-test=native_numeric_colors_and_saved_palettes
bash tools/performance/workspace-motion.sh gtk --native-test=native_paint_fitted_columns
bash tools/performance/workspace-motion.sh gtk --native-test=native_nested_tool_drawers
```

The native harness writes screenshots and logs to its reported temporary directory.
Set `LAYER_NATIVE_CAPTURE_DIR` to an existing absolute directory to capture the
composited monitor as well as GTK snapshots. Mouse and touch tests verify native
menu mapping and keyboard dismissal. The virtual tablet test covers selection,
immediate reordering, held-menu dragging, cancellation, undo/redo, selection,
hold recognition, click suppression, and gutter scrolling; its synthetic serials cannot
authorize compositor popup grabs, so physical stylus popup behavior still needs
hardware review.
GTK is the reference implementation for subsequent host ports. The earlier
[research](color-palettes-research.md) and [HTML study](prototypes/color-palettes.html)
provide design context; this document describes the current implementation.
