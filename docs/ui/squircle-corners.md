# Squircle corners

[Workspace and UI](README.md) · [Shared UI](shared-ui.md) ·
[Toolbar components](toolbar-components.md)

The drawing interface rounds corners with superellipses (CSS
`corner-shape: squircle`, |x|⁴ + |y|⁴ = 1 within each corner box) instead of
circular arcs. A squircle corner stays flat for longer and turns more tightly
than a circle of the same radius, so an 18px squircle reads about as soft as
the former 8–10px circular corners.

## Radii

Apple reads the shared surface and control radii through the stateless toolbar
metrics query. The same query supplies layer edge insets and divider geometry;
divider lines grow across their allocated slot with the shared end insets.

- **Tiles** use half their shorter side: square tiles become full squircles and
  labeled tiles become squircle capsules. Title-bar tools, readouts and
  document tabs follow the same rule with the header tile; menu labels, the
  workspace switcher, the clock and status bubbles use half their own height.
- **Standalone toolbars** use their tile radius, so end tiles fill the toolbar
  ends exactly. `TileStyle::corner_radius` is published per panel as
  `tile_corner_radius`. A slider's brush preview uses its toolbar's tile
  radius; its tile-sized bookmark button fills the top-end corner.
- **Surfaces** (panels, tab tops, drawers, collapsed columns, the title-bar
  editor and notices) use `SURFACE_RADIUS`, 18px or half a small tile.
- **Panel controls** (buttons, entries, dropdowns, segmented choices and list
  highlights) use the shared 13px control radius. Compact panel content has one
  8px inset; the control radius is independent of that inset. Controls 26px tall
  or shorter become capsules.
- **Concave joins** (tab feet, drawer bridges and expanded-panel joins) keep
  their sizes and use inverted squircle curves.
- Layer and mask thumbnails use full squircles on every host. Their editing target
  has a 3px border in the resolved accent color, replacing the faint idle edge.
  It sits 2px inside and 1px outside the 30px tile, giving 32px outer bounds
  and covering 1px of the preview. The hit area stays fixed. Type badges sit
  inside the preview corners. Selection changes fade and shrink the outline
  over 200ms with cubic-bezier(.25, .46, .45, .94), following GTK's native
  transition. GTK, Web, Android and Apple interpolate color, width and offset
  from a transparent, 0px outline 4px outside the tile. Windows scales and
  fades its fixed ring through a visual transform, so its stroke scales with
  the outline rather than growing from zero. Its outer extent follows the
  same 38px-to-32px transition. The 1px base edge remains visible. Hosts respect
  the system's reduced-motion or animation-duration setting. Timing belongs
  to GTK/CSS transitions, Compose, SwiftUI and Windows Storyboards. Android,
  Apple and Windows retain the squircle contours during the transition;
  Android isolates the changing outline in its own graphics layer so animation
  frames do not rerecord the unchanged thumbnail and surrounding drawing.
  Windows animates only compositor-supported visual scale and opacity.
- Checkboxes, slider thumbs and other small details
  keep their former visual rounding with squircle corners.

A drawer's source tile squares only the corners facing its drawer and keeps
its selected blue; an unselected source turns panel grey. Its container
flattens facing corners within `SURFACE_RADIUS` of the tile
(`DrawerPlacement::source_corners`), so a padded container's larger rounding
never clips the square join. A closing drawer keeps these joins while its body
and connector shrink; they round again only once the drawer is removed.
Open collapsed columns publish corner geometry for their sidebar and ordinary
panel groups. GTK uses the per-corner radii and receiving-panel geometry
described in [stacked columns](stacked-columns.md), including partial curves and
continuous connections with inverted curves at panel gaps. GTK's connected tab bar
keeps its own upper outline, with panel-colored fill behind its rounded corners.

Panel-tab hover keeps the existing 18-unit upper corners. Shared
`TabPresentation.hover_bottom` publishes the left and right lower radii:

| Neighbour on that side | Lower hover corner |
| --- | --- |
| Active tab | Convex 6-unit corner, following the active tab's concave foot. |
| Inactive tab | Concave 6-unit foot extending into the neighbouring slot. |
| End of the tab strip | Straight edge. |

Negative radii denote concave feet, positive radii denote convex corners, and
zero denotes a straight edge. Selected tabs publish zero hover radii and keep
their active silhouette. Strip viewports clip hover feet at scrolled edges.
These rules apply to docked, floating and drawer tabs and update with selection,
reordering and membership. Hover remains below the active silhouette and uses
one 8% text-color fill, including its feet.
Browsers without `corner-shape` keep the 6-unit lower inset radius to match
the active tab's circular foot; the fitted upper radius stays unchanged.

## Scope

Squircles cover the workspace: the title bar, toolbars, panels, tabs, drawers,
collapsed columns, status notices, the brush-size preview and the controls
inside them. Native hosts keep platform styling for settings, dialogs, menus,
tooltips and other popovers.

GTK's custom workspace-manager list and footer use the shared control contours,
as do the custom drawing and picker sheets. The workspace list's viewport owns
its rounded clip; the inner list and rows have square backgrounds, so selected
rows and swipe actions do not stack corner masks. Scrolling keeps the viewport's
contour. The native dialog shell and menus retain platform styling.

Web squircles those as well. Menus and popovers use `SURFACE_RADIUS` with
12px items, concentric at their own 6px inset. Other dialogs, cards, list highlights and tooltips
keep their former visual rounding: `--squircle-scale` divides the former
circular radius by the corner fit. Circular icon buttons, such as dialog close
buttons, and switch tracks and knobs become squircles.

The Capy button icon choices in settings are tiles on every host: full
squircles like the Capy button itself. GTK wraps the choice grid in
`squircle::Squircles`, and Windows uses the fitted circular radius.

True circles stay round: color swatches and wheel buttons, wheel markers,
dials, radio indicators, gradient stops, circular range knobs and native
window controls.

## Corner fit

A circle with 0.54 of a squircle's radius contains that squircle; the two meet
at 45°. Web uses this `--corner-fit` only when `corner-shape` is unsupported,
keeping tiles as rounded squares rather than circles.

GTK CSS has no `corner-shape`, so its stylesheet always declares the fitted
circular radius (design radius × `--corner-fit`). GTK's own hit testing and
overflow picking therefore cover every painted pixel.
`squircle::Squircles` wraps the workspace window content and redraws each
circular rounded clip, uniform border or outline, and outset shadow as a
squircle with the circle radius divided by the fit. Undesigned toolkit radii
keep their former visual rounding the same way. Solid backgrounds and borders
use cached corner fills and straight bands. Panel contents have one native
rounded clip at the fitted radius, without repeating their subtree through the
corner pieces. Content clips near a window corner use radii that merge with the
window outline, with cache entries scoped to that corner geometry. The shell
keeps its exact squircle contour; content clipping can
change edge antialiasing. Corner paths retain their identity as panel height
changes; internal boundaries meet on the device-pixel grid. Small image clips
and SVG details retain cached masks. Native color nodes
keep their original color states. Conversion descends
through bridge masks, color matrices, crossfades, blends and blur nodes. Both
children of a mask or transition retain the same corner and shadow conversion
as an unwrapped panel, including when a connected corner reaches zero radius.
Blurred outset shadows never reach GTK's analytic box-shadow shader. In GTK
4.22 it returns NaN near rounded corners for some radius-to-blur ratios on
NVIDIA, including the 9.72px drawer radius with its 24px blur. HDR windows
render in float, so Mutter shows those pixels as black boxes; SDR loses the
shadow there. Each large shadow style (corner radii, blur, offset and scale) is
blurred once on the CPU and drawn as nine cached slices. Long narrow toolbars
use cached alpha textures for two end caps and a one-pixel middle strip, stretched
over the changing long dimension. Their joins follow the device-pixel grid.
Compact shapes use the same CPU coverage and blur as the large shadow slices;
they do not add a GSK blur or repeat offscreen.

Native tab surfaces paint the selected region and its even-odd background
complement with color fills. Four bounded contour pieces cover the upper corners
and concave feet; adjacent upper corners share one piece when their device-pixel
bounds meet. Hover stays within the background regions. Color and HDR values
remain native GTK inputs; antialiased contour pixels may differ from a masked
color interpolation.
`squircle::Popover` applies the same conversion to the brush-size preview.
Subtrees that must stay round are drawn through `squircle::append_round`.
Rust-drawn rounded rectangles in the workspace multiply design radii by
`squircle::CORNER_FIT`.

GTK's connected collapsed-column backgrounds use the workspace GPU renderer instead
of GTK masks. A cached normalized profile supplies smooth outer joins; analytic
gap caps stay within the panel gap. One coverage field supplies the sidebar,
receiving panels, bridges, glass and shadow. Native controls retain their clips,
and the tab bar retains its own rounded upper outline over the joined backing.
See [stacked columns](stacked-columns.md).

Android uses `SquircleShape`, a Compose `CornerBasedShape`, through
`TileShape`, `SurfaceShape` and `ControlShape`; custom paths share
`squircleCorner`. Compose pointer input already uses whole bounds.

macOS and iPadOS use `SquircleShape`, an `InsettableShape` with `.tile`,
`.surface` and `.control` tokens and per-corner radii; tab, expanded-panel and
drawer-bridge paths share `Path.squircle`. SwiftUI's continuous rounded
rectangle is a different curve and is not used. Panel groups, collapsed
columns, drawers and segmented controls draw the exact squircle as their fill,
with a cached shadow outside it, and clip content only with the fitted circular
radius. Tabbed groups draw their strip and body as separate squircle segments,
and glass regions publish the same design radii with `BackdropRegion::SQUIRCLE`.
Layer and mask previews use the exact `.tile` clip within their 28px images.
Core Animation applies circular clips directly; a squircle path clip is an
offscreen mask on every composited frame, which is too slow beside a live
canvas. Clipping does not narrow SwiftUI hit testing, and
squircled controls use their whole bounds or the same shape as their content
shape. Open drawers publish their source bounds and
direction, and panel groups, collapsed columns and drawer bodies square the
same corners as `DrawerPlacement::source_corners`.

Windows draws panel groups, tab strips and tab tops, collapsed columns, drawers
and drawer bridges as exact squircle paths (`squircleRectangle`,
`panelTabShape` and `drawerBridge` sample the same corner curve as Web).
Composition shadow masks and their Direct2D cut use the same outline, and glass
regions publish design radii with `BackdropRegion::SQUIRCLE`. Title-bar tools,
bars, chips, document tabs and toolbar tiles keep WinUI's circular
`CornerRadius` at the fitted radius (design radius × 0.54), as on Web without
`corner-shape`. Panel content clips with the fitted circle. Layer and mask
previews fill exact squircle paths with an `ImageBrush` backed by the existing
thumbnail source. Their perimeter uses concentric squircle paths, which also
appear in native drag snapshots.

## Checks

- Panel tabs: GTK `native_panel_tab_lower_contours` at 1x/2x, Web `--tab-styles`,
  and Android `panelHoverCornersFollowEveryActiveTab` in both themes cover
  inactive neighbours, active neighbours and strip ends. GTK and Web also
  compare clipped viewports; Web checks the circular browser fallback.
- GTK layer thumbnails: `tools/performance/workspace-motion.sh gtk --native-test=native_layer_thumbnail_squircles`
  checks preview clipping, outer borders, corner clicks and drag previews in both themes.
- Web `--layer-hold` also checks intermediate selection outlines, rapid retargeting
  and reduced motion. Android's selection timing fixture is described in
  [Android benchmarks](../development/android.md#benchmarks).
- GTK: `tools/performance/workspace-motion.sh gtk --native-test=native_squircle_corners`
  (corner picks reach tiles, masked and unmasked shadows with changing corner
  radii stay finite; add `GDK_DEBUG=color-mgmt MUTTER_DEBUG_FORCE_HDR=1` for the HDR
  path), plus `--drawer-style` and `--drag-pickup`.
  Window-free `squircle::tests` covers wrapper parameters, both mask/transition
  children, changing corner radii, scale changes and retained converted nodes.
- Web: `apps/layer-web/test.mjs` with `--drawer-style`, `--toolbar-components`,
  `--compact-workspaces`, `--tab-styles` and `--title-bar-state`.
- Apple: `bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/squircle-geometry.swift`
  checks the corner formula, clamping and source-corner flattening; the
  `testToolbarComponents`, `testColumnStacks`, `testTitleBarToolDrawers` and
  `testColorPicker` journeys attach captures.
- Android: `AndroidInteractionTest` `drawerButtonsAndBridgesKeepTheirColors`,
  `drawerTabsKeepActiveColorsAndPadding`,
  `collapsedIconsKeepTheirSourceWhenDrawerTabsAreVisible` and
  `toolbarComponentsAcrossDevicesAndLayouts`, plus `AndroidTitleBarTest`.
