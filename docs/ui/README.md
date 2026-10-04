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
- [Canvas action bar](canvas-action-bar.md), [image commands](image-commands.md),
  [command search](command-search.md),
  [selections](selections.md) with the [selection tools](selection-tools.md) and
  [tonal range](tonal-selection.md), [open and import](open-and-import.md),
  [copy and paste](clipboard.md), and [GTK drawing tabs](gtk-document-tabs.md).
- [Settings](settings.md), [numeric controls](numeric-controls.md),
  [theme colors](theme-colors.md), [panel transparency](panel-transparency.md)
  and [squircle corners](squircle-corners.md).
- [Color picking](color-picker.md), [color palettes](color-palettes.md) and the
  [color-management journeys](color-management.md).

## Histogram and Waveform

GTK and Web Photo open Histogram above Properties and Layers, with Waveform in the
adjacent tab. Both are ordinary dockable panels, shown or hidden through Window.
Their command-search actions reveal the existing panel wherever it was placed. Both panels share the inspected source:
Visible, Selected layer, Reference or Selection. Their channel menus independently
choose RGB, individual channels or luminance. Waveform retains the image's
left-to-right position, with brighter values toward the top; it initially shows
RGB. Histogram shows how often each value occurs.

Source and channel selectors occupy one toolbar above the graph. Log counts
changes Histogram's height or Waveform's trace brightness; its toggle and
clipping buttons sit beside the status below the graph. The graph's tooltip
contains detailed counts. In floating documents, Waveform includes nonpositive
values on its bottom rail. Its scale labels overlay the graph so the trace uses
the full panel width.

Preview uses estimated counts while input changes; Exact scans the frozen source
after it settles. Updating keeps that distinction visible. Both panels use the
same frozen query; Waveform's spatial counters are allocated only while it is
presented. Hiding every view retires its work and data. Levels and Curves have
embedded input statistics in Properties, sharing the same query and display rules.

Shadows and Highlights mark pixels at or beyond zero and one. Floating documents
label these SDR thresholds. The overlays apply to the canvas after proof and
screen transforms; they never change artwork, exported pixels or histogram
counts. While either is active it takes precedence over mask tint and gamut
warning. Their settings remain intact and their display returns afterward.

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
parameters. Its heading shows the layer name followed by the localized filter,
group or saved selection type when the name differs from that type. Ordinary
paint layers keep their name alone. Shared Properties pages choose which
parameter controls are shown; hidden values remain active. Channel selection,
automatic adjustment and sampling share one toolbar. Levels and Curves offer
black, neutral and white points through one sampling menu; White Balance uses the
same sampling icon for its neutral point.
Number fields sit beside their labels; Curves coordinates use two labeled
columns below the graph. Color Lookup (LUT) has a single selector
for Original, Warm, Cool, Monochrome and the current imported LUT, with a separate
Import LUT button. Long names truncate in the selector and remain available on
hover and to accessibility. Presets use sRGB; imported LUTs expose their color
space. Intensity blends either kind with the original. Presets and imported
tables share the same saved resource, renderer and undo path.
Curves uses these pages for RGB and channel
selection, with [precise point controls](numeric-controls.md#properties-and-curves).
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
Preferences choose whether Capy stays visible and whether panels reveal near
occupied screen edges. Shared logic manages visibility, reveal behavior and the
state that keeps controls available during a menu or interaction; see
[shared UI](shared-ui.md#window-chrome-and-zen-mode).

Hiding controls does not resize the document viewport or move the camera. The
usable workspace area can inform an explicit Fit Canvas command, but ordinary
visibility changes do not move artwork under the pen.

## Icons

Every host draws the same SVGs from `apps/layer-web/icons`; Rust supplies the
icon identity for each command, preset, panel and tool setting.

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
