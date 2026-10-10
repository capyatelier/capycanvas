# Color palettes

[Workspace and UI](README.md) · [Research](../history/color-palettes-research.md)

Paint places the Palettes tab immediately after Color; Sketch shows
the same component below the wheel in its color drawer. Customized layouts can
show Palettes through the Window menu (GTK, Android and Windows also through the
color swatch menu's Palettes action). GTK
([`color_library.rs`](../../apps/layer-linux/src/color_library.rs)),
Web ([`palettes.js`](../../apps/layer-web/palettes.js)), Android
([`Palettes.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Palettes.kt)),
Apple ([`PalettePanel.swift`](../../apps/layer-apple/Shared/Editor/PalettePanel.swift))
and Windows ([`PalettesView.cpp`](../../apps/layer-windows/PalettesView.cpp))
render the shared [`PalettePanelView`](../../crates/layer-ui/src/color/palette_view.rs),
its menus and reorder previews; the descriptions below apply to every host
unless one is named.

## Layout and interaction

The component has no header controls. Its body contains one recent-color row,
a divider, and the saved palette. A second divider separates the footer, with the palette selector on
the left and the current color’s name, hex preview, and conditional EV on the
right.

- The minimum width is 242 logical pixels, fitting six 34-pixel swatch targets.
  They grow to 40 pixels at 280. Wider panels add columns; after four rows, the grid scrolls;
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
the palette exception in [the drag convention](drag-and-reorder.md).
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
presentation as Layers. Swatches grow from 34 to 40 pixels, with gaps of 4. Radius, text, and
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


Paint and Photo also place Proof after Navigator and Diagnostics after Tool Set.
Automatic tab labels follow the [panel contract](panel-customization.md).

## Starter palettes

Each host installs ten original editable starter palettes with 11–17 named sRGB
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

The [starter research](../history/color-palettes-research.md#starter-palette-curation)
compares popular artist palettes and print references. These values are original
curations, not copies of those artists' sets or simulations of physical inks.
Each set has a distinct value range, saturation and hue balance. Color families
stay adjacent in the saved sequence; wider grids wrap that sequence without
sorting it. There is no requirement that every palette contain three evenly shaded ramps.

Untouched palettes from older versions migrate using exact name/color/order
fingerprints. Edited or reordered copies are retained. Palette IDs and surviving
swatch IDs stay stable; shorter replacements remove only surplus untouched
starter swatches. Deleted palettes are not recreated.

## Color data and history

Palettes belong to the shared editing state. Names, active palette, selected
swatch identity, imports and bounded recent colors survive workspace switches
and are saved separately from layouts. Artwork and mask paints use the same
library while keeping separate color targets. Native hosts own rendering, focus,
menu presentation and file pickers; the shared palette view identifies the
selected swatch, including when several swatches have the same color.

History contains at most 64 exact definitions, newest first, deduplicated by
exact definition. Brush history is recorded at successful stroke commit using
the source definition captured with the brush. Selection, wheel/eyedropper
preview, cancellation, erasing, selection-mask painting, smudging, and liquify
are not color-use events. Successful fills, figures, and gradients record the
source definitions they use. Undo does not remove usage history.

Saved colors and history retain tagged RGB space, alpha, and HDR values. GTK
renders managed previews through the existing ColorPatch path; Web, Android and
Windows paint the shared view's swatch preview, tone-mapped for HDR documents exactly
as their Color panel swatches are. Hex is an sRGB
preview, not the storage format; HDR uses the picker’s base color and EV readout.
The full color name and space remain available in tooltips/accessibility text.

## Import and export

Import Palette… accepts every supported file through one chooser. Shared Rust
selects the reader from the file's contents, not its extension. Export Palette
is a submenu of formats, ordered by the target applications' priority; choosing
one opens the platform's ordinary save picker with that extension.

| Format | Import | Export | Applications |
| --- | --- | --- | --- |
| Capycolor `.capycolor` (JSON; legacy `.json`) | Exact | Exact | Capy Canvas |
| Adobe Color Swatch `.aco` v1/v2 | RGB, HSB, CMYK, Lab, gray; names | 16-bit sRGB with names | Clip Studio Paint (documented both ways), Photoshop, Procreate, Krita |
| Clip Studio color set `.cls` | 8-bit sRGB and names | — | Clip Studio Paint |
| Procreate `.swatches` | Procreate 4 and 5 files; sRGB and Display P3 | 30 sRGB slots | Procreate |
| Adobe Swatch Exchange `.ase` | RGB, CMYK, Lab, gray; groups | sRGB in one named group | Affinity (documented import), Adobe, Procreate, Krita |
| Affinity `.afpalette` | Solid RGB, CMYK and Lab fills | — | Affinity |
| GIMP `.gpl` | 8-bit sRGB | 8-bit sRGB | Krita, GIMP |
| Krita `.kpl` | RGB profiles, CMYK, gray, XYZ, Lab; grid order | — | Krita |

Every external format stores fewer properties than a Capycolor palette. Exports
other than Capycolor convert each color to opaque, clipped sRGB (Procreate keeps
its first 30), and the panel's message line states how many colors were clipped,
became opaque or were left out. External RGB without a profile is sRGB. Krita
profiles named Display P3, Adobe/Clay RGB or ProPhoto/Large RGB, and linear
(`g10`) profiles, keep their meaning; other profiles are read as sRGB. Lab is
D50 and XYZ is D50; each import keeps the smallest of sRGB, Display P3 and
ProPhoto that contains the color, so wide colors are not clipped. CMYK has no
embedded profile in these formats and uses the device formula; applications
that apply a press profile show different RGB for the same inks. Pantone and
other color-book references are rejected rather than guessed. Affinity's 16-bit
Lab scaling is inferred from sample files. Zero-alpha Clip Studio entries are
empty cells; Affinity fills keep their alpha. Gradients are skipped. Imported
names are clipped to 64 characters, control characters are removed, and GIMP's
“Untitled” placeholder receives a suggested name.

Files are limited to 1 MB. ZIP-based formats read only their palette member,
with expansion bounded to 8 MB and CRC-checked; ZIP64 size records from macOS
are accepted. Libraries retain the limits of 64 palettes and 4096 total swatches.
Hosts read and parse files off their UI thread (Web in its worker, Android on
an I/O dispatcher, GTK through GIO's blocking pool, macOS and iPadOS on the
project I/O queue through `capy_palette_file`, Windows on a palette file thread
that writes exports through a replaced partial file), and failed imports leave
the library unchanged. Library mutations retain existing tile widgets; brush frames
do not rebuild the palette chooser.

Procreate, Clip Studio and Affinity imports of Capy exports have not been
checked in those applications.

## Host notes

Web and Android consume the shared view, menus, reorder previews and codecs;
their files are read and encoded off the UI thread. Web tiles use Pointer
Events with pointer capture, CSS transforms with a 140 ms ease-out transition
(none under `prefers-reduced-motion`) and a fixed-position ghost; its file
chooser and download or `showSaveFilePicker` handle files. Android uses Compose
pointer input with the actual tool type, animates neighbors with the system
animator scale, draws the lifted tile in the workspace overlay, and uses the
Storage Access Framework. Both hosts measure automatic tab names at the current
font and call the shared `TabStyle::automatic_names`. A touch or pen opener
focuses the active chooser row, so the soft keyboard stays closed until search
is tapped; a keyboard (or, on Web, a mouse) opener focuses search as GTK does.
Web chooser buttons keep search focused during a mouse press: hiding the soft
keyboard would move the visual viewport between press and release.

Apple hosts render the shared view in SwiftUI and reach the menus, reorder
previews and dry-run validation through the bridge's query request; library
edits return their validation error to the control. Saved tiles use the native
reorder adapters with an immediate `.swatch` surface: AppKit and UIKit supply
movement slop, a touch or Pencil hold opens the swatch menu through the same
recognizer, and mouse holds open nothing, so a stationary mouse release still
selects as on Web. Chooser rows are menu-only sources that let the list scroll
before a touch or Pencil hold. Neighbors slide for 140 ms unless Reduce Motion is
on, and the lifted swatch is a workspace overlay. Long palettes scroll in the
20-point inner and 12-point outer edge bands at 240 points per second. Import
and export use the system open and save panels (the document picker on iPad),
mounted at the editor root so a closing drawer cannot dismiss them; the codec
runs on the project I/O queue and writes export bytes to a descriptor. Escape
closes the innermost palette overlay before a drawer, and ⌘Z, ⇧⌘Z and ⌘Y undo
and redo reorders after a palette interaction. Automatic tab names measure the
bold title at the current text size and call the shared stateless
`automatic_tab_names` toolbar query; tab slides reuse the fitted labels. A group
holding both Color and Palettes measures the hidden page so switching tabs keeps
its fitted height.

Windows renders the shared view in WinUI and asks the workspace for menus,
reorder previews and dry-run name checks. Tiles capture their pointer and start
dragging after the system drag slop. Touch and pen holds use a WinUI gesture
recognizer to open the shared menu as a native `MenuFlyout`. The lifted tile is
a shadowed popup; neighbors move with 140 ms Composition animations, disabled
when Windows animations are off. A 16 ms timer scrolls long palettes at the grid
edges. Pointer-capture loss after the contact lifts commits like a release;
loss during contact cancels. New, rename and remove prompts use `ContentDialog`.
Import and export use the Windows App SDK file pickers owned by the editor
window. A palette file thread reads imports and writes exports through a
`.partial` file. Chooser focus follows the Web and Android opener rules.

## Checks

```sh
cargo test --locked -p layer-engine -p layer-ui -p layer-workspace
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_panel_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_reorder_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_reorder_pen_input --tablet
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_context_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_palette_context_pen_input --tablet
node apps/layer-web/test.mjs --headless --palettes   # LAYER_PALETTE_SAMPLES=<dir> adds real files
node apps/layer-web/device.test.mjs --palettes
cargo test --locked -p layer-apple palette
./apps/layer-windows/scripts/exercise-palettes.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe
```

Android runs `art.capycanvas.AndroidPaletteTest`; `-e paletteBenchmark true`
records frames during a steady stylus drag and `-e paletteDirectory <device dir>`
imports every pushed sample file. Mac runs `EditorLaunchTests/testPalettes`.
Injected tablet input does not qualify physical stylus popups or holds.

Generated palette and color names use the active interface language when they are created. Saved names, imported names and names you edit stay as written. Starter palettes replace only the untouched fresh palette; restoring a workspace preserves every saved palette, even one named “My colors”.
