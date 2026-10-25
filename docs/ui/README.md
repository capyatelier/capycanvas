# Workspace and UI

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The UI is shared at the level of behavior and layout, rather than as a collection
of rendered widgets. Rust describes the editor state and the controls it needs;
each frontend presents them using its native toolkit or the browser DOM.

This lets tool commands, numeric rules and workspace configuration stay consistent
while file dialogs, focus and accessibility follow the platform.

## Rules for UI changes

- Build and validate a UI change on GTK first, then get the user's approval
  before porting it. Web follows GTK, and the native ports follow Web; see the
  [platform guide](../platforms/README.md).
- Follow the layouts and conventions artists already know, and ship no inert
  buttons or placeholder menu items; see the
  [design criteria](default-workspaces.md#design-criteria).
- Take colors only from `theme.rs` through the published palette; see
  [theme colors](theme-colors.md).
- Take icons only from the shared bank in `apps/layer-web/icons`; see its
  [README](../../apps/layer-web/icons/README.md) and [icons](#icons).
- Check every change in both the light and the dark theme.
- Write UI text by the [UI text rules](../development/writing.md#ui-text).
- Draggable controls follow the [drag and reorder convention](drag-and-reorder.md).
- Hosts never reproduce shared logic. Menu copy, command availability, numeric
  formatting and layout decisions belong to Rust; see [shared UI](shared-ui.md).

## Guides

- [Localization](localization.md): shared Fluent catalogs, message identities and
  translation checks.
- [Shared UI](shared-ui.md): the Rust/host boundary, docking, window chrome,
  Zen mode, actions, input and settings flows.
- [Default workspaces](default-workspaces.md): Sketch, Paint and Photo, the
  workspace manager and switcher, and the design criteria.
- [Panel customization](panel-customization.md): menus, configuration drawers,
  tool drawers, floating panels and workspace history.
- [Title bar](window-bar.md), [toolbar components](toolbar-components.md),
  [compact toolbar edges](compact-toolbar-edges.md) and
  [stacked columns](stacked-columns.md).
- [Drag and reorder](drag-and-reorder.md): the required pickup rules.
- [Tool bubbles](tool-bubbles.md): tools, brushes, sliders, the color ring and
  Layers kept on the canvas.
- [Canvas action bar](canvas-action-bar.md), [image commands](image-commands.md),
  [command search](command-search.md),
  [selections](selections.md) with the [selection tools](selection-tools.md) and
  [tonal range](tonal-selection.md), [open and import](open-and-import.md),
  [object layers](image-objects.md), [vector layers](vector-layers.md),
  [copy and paste](clipboard.md), [New drawing](new-drawing.md) and
  [GTK drawing tabs](gtk-document-tabs.md).
- [Settings](settings.md), [numeric controls](numeric-controls.md),
  [theme colors](theme-colors.md), [panel transparency](panel-transparency.md)
  and [squircle corners](squircle-corners.md).
- [Color picking](color-picker.md), [color palettes](color-palettes.md) and the
  [color-management journeys](color-management.md).

## Histogram and Waveform

Photo opens Histogram above Properties and Layers, with Waveform in the
adjacent tab. Both are ordinary dockable panels, shown or hidden through Window.
Their command-search actions reveal the existing panel wherever it was placed. Both panels share the inspected source:
Visible, Selected layer, Reference or Selection. Their channel menus independently
choose RGB, individual channels or luminance. Waveform retains the image's
left-to-right position, with brighter values toward the top; it initially shows
RGB. Histogram shows how often each value occurs. Counts start on a linear scale,
so graph heights remain proportional to pixel populations. Log counts is an
optional view for revealing small populations beneath large peaks, especially
in artwork with broad areas of one color.

Source and channel selectors occupy one toolbar above the graph. Log counts
changes Histogram's height or Waveform's trace brightness. Its toggle has its
own row below the graph, so a translated caption wraps instead of crowding the
status and clipping buttons beneath it. The toggle and its caption share a
26-pixel target. Status and axis labels use secondary text at 55% opacity.
The graph's tooltip contains detailed counts. In floating documents, Waveform includes nonpositive
values on its bottom rail. Its scale labels overlay the graph so the trace uses
the full panel width.

Preview uses estimated counts while input changes; Exact scans the frozen source
after it settles. Updating keeps that distinction visible. Both panels use the
same frozen query; Waveform's spatial counters are allocated only while it is
presented. Hiding every view retires its work and data. Levels and Curves have
embedded input statistics in Properties, sharing the same query and display rules.
Sampling, Auto and geometry queries pause scans without discarding valid results.
Source changes still invalidate those results while scanning is paused.

Shadows and Highlights mark pixels at or beyond zero and one. Floating documents
label these SDR thresholds. The overlays apply to the canvas after proof and
screen transforms; they never change artwork, exported pixels or histogram
counts. While either is active it takes precedence over mask tint and gamut
warning. Their settings remain intact and their display returns afterward.

## Gradients

The Gradient tool uses Tool Settings and the Tool Options bar. Tool Settings
places Linear, Radial and Reflected shapes above the gradient editor. The toolbar
keeps shape outside its gradient popup. The tool shares one stop editor with
Gradient Fill and Gradient Map. Gradient Fill places Shape first in Properties,
then the stop editor and its angle, scale and position. Click the strip to
add a stop, click a handle to select it, and drag to move it. The selected stop's
position and color sit together below the strip; the color picker includes
opacity. Interior stops can be deleted. Endpoints stay at 0% and 100%.

Oklab is the default interpolation. Linear light and Classic (encoded document
RGB) are available in the same selector. Integer gradients always use dithering;
floating-point gradients are unchanged. The preview shows the same
quantization and dithering. Reverse mirrors the stops; the bucket applies the
current drawing color to the selected stop. Tool settings are retained separately from artwork;
one canvas drag creates one undo step. Effect stop drags also create one step,
and Escape cancels the current drag. Reset returns tool stops to the current
foreground/background pair or restores an effect's default gradient.

## Different workflows, shared tools

The interface serves digital painters, photographers and comic artists. Their
priorities differ even when they use the same layers and masks. A comic artist
may keep reference-layer and lasso-fill controls close at hand. A painter may
devote that space to brush settings. A photographer may need effect chains,
which apply filters in order, and properties for the selected layer or filter.

Existing habits matter as much as the choice of tools. Artists build muscle
memory around panel positions, shortcuts and repeated gestures, and that is hard
to relearn. Configurable layouts, controls and shortcuts let the app adapt to
those habits while keeping the underlying commands consistent. These workflows
are arrangements of a common editor rather than separate applications. Tool
Settings follows the active tool, and Properties exposes the relevant effect
parameters. Shared Properties copy identifies the layer name and localized type.
Workspace panels on GTK, Web, Android, Windows, macOS and iPadOS use 26-unit
dropdowns, checkboxes, action buttons and joined choices, with 16-unit icons.
These measurements are logical pixels, dp on Android. Numeric sliders keep their
36-unit base rows; tonal
intervals, transform anchors and gradient stop strips keep their specialized
layouts. Independent controls have 4-pixel gaps horizontally and vertically,
with 6-pixel gaps between logical groups and one 8-pixel panel inset.
Mutually exclusive action groups use labeled
dropdowns. Flip and quarter-turn actions sit in icon rows; Apply and Cancel sit
together. Eyedropper labels sit beside their Source and Sample size dropdowns.

### Compact panel spacing

The shared panel spacing roles apply on both axes. Tool Settings, Properties,
gradient and curve editors, graph toolbars, Proof, palette controls, Navigator controls and Layers action
rows use these roles in docks, floating panels and drawers. `UiCatalog.panel_metrics`
publishes the measurements from `PANEL_CONTROL_METRICS`; native hosts present
them without changing the shared roles:

| Role | Gap | Use |
| --- | --- | --- |
| Joined parts | 0 px | Segmented choices and anchor grid cells share edges. |
| Compound parts | 2 px | The anchor chooser and X/Y fields; the two X/Y fields. |
| Independent controls | 4 px | Consecutive rows, buttons beside each other, label/control pairs and gradient editor parts. |
| Logical groups | 6 px | Tool mode/actions section to settings; space before a group heading. |
| Content inset | 8 px | Each edge of the panel body, applied once. |

Plain labels and headings align with the content's leading edge, without another
label inset. Full-width controls and right-aligned fields end at its trailing
edge. Related dropdown rows share a label column sized from their captions.
Captions stay beside their controls at every width and wrap to two lines, on
every host. Labels wrap between words, and dialog buttons keep their captions
together.
Numeric readouts and their text editors use 6-pixel horizontal padding, including
inline fields. Checkboxes and button captions retain the space for their
indicators and icons; slider tracks and gradient handles retain their internal
insets.

A row of independent buttons uses the same 4-pixel horizontal gap as the vertical
gap to the next row in its group. Joined segments form one control; their shared
edges do not set the gap to a separate control. Headings have 6 pixels before and
4 pixels after, with no leading gap at the start of the settings section. Parent
spacing and child margins together must equal the intended gap. Container
boundaries do not create additional group spacing: Zoom Click, Drag and Center
belong to one group, as do Gradient shape, stop editing and opacity.

Toolbar tiles keep their shared 2-pixel or 4-pixel gaps and 36-pixel base size.
Preferences and dialogs keep their larger section spacing. These contexts do not
set the density of compact workspace forms. Single-line controls have a 26-unit
base height and may grow for system text scaling; icon-only actions stay square.
Ordinary numeric readouts and editors use the same 26-unit height. Control
surfaces follow the [13-unit squircle rule](squircle-corners.md#radii).

### Properties

Shared Properties pages choose which
parameter controls are shown; hidden values remain active. Channel selection,
automatic adjustment and sampling share one toolbar. Levels and Curves offer
black, neutral and white points through one sampling menu; White Balance uses the
same sampling icon for its neutral point.
Properties offers **Color mode** after opacity and blending for paint content: Full color,
Grayscale, and Two-tone (black & white). Changes convert existing paint and
constrain later painting. Undo restores the previous pixels and mode; returning
to Full color keeps the converted pixels. Reduced modes appear in the layer
subtitle. Image objects, masks, selections and effect layers do not offer this control.

Properties gives the layer name its own heading, with the smaller, muted
layer type below it. On raster and image object layers, **Add Filter** sits at the
right of that second row with an **fx+** icon, its text label and a menu arrow.
The heading has a 4-unit gap below it; the layer-type row has a 6-unit gap
before the parameter controls.
Filter, mask, selection and group properties omit this button. The Layers footer
uses the same icon.
Its Add Filter button shares the other footer buttons' theme color and flat style,
with a transparent resting background.
A footer too narrow for every button hides the lowest-priority ones and lists them
at the top of its actions menu: New selection layer goes first, then Import image,
New group, Add Filter, Add mask and Delete; New layer and New vector layer stay
longest. `LayersView::footer` publishes each button with the narrowest footer that
shows it, from the shared 26-unit buttons, 2-unit spacing and 6-unit insets.
Long layer types, property labels and Tools list rows wrap to two lines before
they ellipsize.
Properties dropdowns and their adjacent action buttons share the compact
26-pixel panel height and 6-pixel horizontal padding, in docks and drawers.
**Add Filter** is also available in each eligible layer's context menu. It opens
the menu bar's filter categories and
adds a local filter above the owner's existing chain, so it runs last. Selecting
a local filter keeps its owner as the destination. Paint layers, image object
layers and isolated groups accept local filters; locked owners and Pass Through
groups do not.
Generators and Frequency Separation stay in the menu bar. A current selection
becomes the new filter's mask. Insertion is one undo step and selects Properties.
Every entry point captures its layer, so changing selection while a menu is open
does not redirect the filter to another layer.

Number fields sit beside their labels; Curves coordinates use two labeled
columns below the graph. Color Lookup (LUT) has a single selector
for Original, Warm, Cool, Monochrome and the current imported LUT, with a separate
Import LUT button. Long names truncate in the selector and remain available on
hover and to accessibility. Presets use sRGB; imported LUTs expose their color
space. Intensity blends either kind with the original. Presets and imported
tables share the same saved resource, renderer and undo path.
Curves uses these pages for RGB and channel
selection, with [precise point controls](numeric-controls.md#properties-and-curves).
[Pen pressure](pen-pressure.md) and [Pen tilt](pen-tilt.md) share the curve
widget and modeless utility. Their calibration state and input mapping live in Rust.
The [Sketch, Paint and Photo defaults](default-workspaces.md)
provide initial arrangements and remain editable workspaces.

## Session, actions and views

[`UiSession`](../../crates/layer-ui/src/session.rs) coordinates the engine and
editor state. A widget sends a typed `UiAction` instead of directly modifying a
layer or a renderer resource. The session validates and executes the action, then
reports which parts of the UI changed.

For example, selecting a different layer changes the active edit target and the
relevant controls. Moving the camera changes viewport state, without requiring
the host to rebuild the layer list. A *view* here is a description of UI state,
such as layer rows and available actions, rather than rendered pixels. Hosts
cache these descriptions and update affected controls; this also avoids
replacing a widget while a user is dragging or editing it.
Panel customization refreshes only the captions it creates. Properties and
other state-driven panel contents keep their own headings and status text.

Commands have stable identities and shared availability rules. A button, menu item
and shortcut invoke the same action and agree on whether it is enabled.
Native text fields still handle their own editing keys.

`ApplicationMenu::ALL` defines the menu bar's order: File, Edit, Image, Layer,
Select, Filter, View and Window. `UiSession::application_menu` supplies their
localized titles, sections, submenus, actions, availability and shortcuts to every
host. Native macOS registers its system menu slots and presents shared titles and
contents for the application-specific menus.
Edit starts with Undo and Redo, keeps everyday clipboard, fill, clear and transform
commands direct, and groups specialized pastes under Paste Special. Search
Commands and Preferences come last; macOS puts settings in its application menu.
The compact Main Menu ends with a separate Preferences section on every platform,
and keeps Preferences in its Edit submenu too.
Whole-image operations live in [Image](image-commands.md). Rasterize Source,
Discard Paint Edits and Clear Entire Layer live in Layer and its context menus.
Select holds [selection controls](selections.md#select-menu); pixel and layer
edits remain available through Edit, Layer and the contextual selection bar.

## Layout and customization

The [layout model](../../crates/layer-ui/src/layout.rs) describes docked bands,
splits, tab groups, floating panels and collapsed columns. These are semantic
relationships: Rust knows where a panel belongs, while the frontend creates and
sizes the actual widgets. The [customization model](../../crates/layer-ui/src/customization.rs)
describes panel contents, including tool and command tiles and configurable
controls.

[`WorkspaceState`](../../crates/layer-ui/src/workspace.rs) is the durable layout
value. Transient menus, native widgets and unfinished gestures are not serialized.
Every host stores named workspaces through `layer-workspace`, which saves tool
settings and the arrangement of tools and panels automatically and keeps a
layout history per workspace. Layout undo is separate from document undo. A drag
is one layout change, and cancelling it restores the original arrangement.

## Zen mode and the camera

Zen mode hides the header and docked chrome and keeps floating panels.
Preferences choose Capy’s icon and whether it stays visible. Shared logic manages
visibility and keeps controls available during a menu or interaction; see
[shared UI](shared-ui.md#window-chrome-and-zen-mode).

Hiding controls does not resize the document viewport or move the camera. The
usable workspace area can inform an explicit Fit Canvas command, but ordinary
visibility changes do not move artwork under the pen.

## Icons

Every host draws the same SVGs from `apps/layer-web/icons`; Rust supplies the
icon identity for each command, preset, panel and tool setting.

Filters panels and generic effects use **fx**. Actions that add a filter use
**fx+**, including Properties, Layers, layer context menus and the selection bar.
Individual filters retain their own symbols, including in nested menus. Categories use
the same symbols in the picker and menus: a half-lit circle for Tone, color swatches
for Color, a sharp triangle for Detail, and a warped grid for Distort.
Submenu arrows align at the trailing edge, including rows with icons.

- Use solid silhouettes for painting tools and concrete objects. Keep contour
  geometry where the outline carries the meaning: selection boundaries, shapes,
  links, guides and cursor previews.
- Keep consistent negative space, optical centering and the 1.5 px contour
  weight. Weight, fill and size follow the
  [Material Symbols guide](https://developers.google.com/fonts/docs/material_symbols).
- Foreground paint follows the theme through `currentColor`. Explicit black and
  white paints, such as color swatches, keep their colors, including under
  disabled opacity.
- Give each meaning its own symbol. Parameter icons describe the setting (size,
  flow, hardness), not the tool that exposes it.
- Check new icons at 16, 24 and 32 pixels in both themes, in normal, accent and
  disabled states.

## Adding UI behavior

Put a behavior in shared Rust when it determines command meaning, validation,
layout topology or document state. Keep widget construction, native focus,
accessibility and OS service calls in the frontend. `layer-host` shares transport
and session integration for Android, Apple and Windows; it does not replace their
native UI implementation. Host coverage is listed in the
[platform guide](../platforms/README.md).
