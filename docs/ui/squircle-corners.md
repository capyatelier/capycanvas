# Squircle corners

[Workspace and UI](README.md) · [Shared UI](shared-ui.md) ·
[Toolbar components](toolbar-components.md)

The drawing interface rounds corners with superellipses (CSS
`corner-shape: squircle`, |x|⁴ + |y|⁴ = 1 within each corner box) instead of
circular arcs. A squircle corner stays flat for longer and turns more tightly
than a circle of the same radius, so an 18px squircle reads about as soft as
the former 8–10px circular corners.

## Radii

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
  highlights) use 12px, concentric with surfaces at the standard 6px inset.
  Controls 24px tall or shorter become capsules.
- **Concave joins** (tab feet, drawer bridges and expanded-panel joins) keep
  their sizes and use inverted squircle curves.
- Checkboxes, thumbnails, slider thumbs and other small details keep their
  former visual rounding with squircle corners.

A drawer's source tile squares only the corners facing its drawer and keeps
its selected blue; an unselected source turns panel grey. Its container
flattens facing corners within `SURFACE_RADIUS` of the tile
(`DrawerPlacement::source_corners`), so a padded container's larger rounding
never clips the square join. A closing drawer keeps these joins while its body
and connector shrink; they round again only once the drawer is removed.

## Scope

Squircles cover the workspace: the title bar, toolbars, panels, tabs, drawers,
collapsed columns, status notices, the brush-size preview and the controls
inside them. Settings, dialogs, menus, tooltips and other popovers keep
platform styling. True circles stay round: color swatches and wheel buttons,
wheel markers, dials, radio indicators, gradient stops and native window
controls.

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
keep their former visual rounding the same way. Masks are rasterized once per
device-pixel geometry and reused as GPU textures; unchanged render nodes,
including shadow-wrapped subtrees, are reused between frames.
Blurred outset shadows never reach GTK's analytic box-shadow shader. In GTK
4.22 it returns NaN near rounded corners for some radius-to-blur ratios on
NVIDIA, including the 9.72px drawer radius with its 24px blur. HDR windows
render in float, so Mutter shows those pixels as black boxes; SDR loses the
shadow there. Each shadow style (corner radii, blur, offset and scale) is
instead blurred once on the CPU, about 2 ms for the drawer shadow at 2×, and
drawn as nine cached slices. Shapes too small for the slices use a GSK blur
node.
`squircle::Popover` applies the same conversion to the brush-size preview.
Subtrees that must stay round are drawn through `squircle::append_round`.
Rust-drawn rounded rectangles in the workspace multiply design radii by
`squircle::CORNER_FIT`.

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
Core
Animation applies circular clips directly; a squircle path clip is an
offscreen mask on every composited frame, which halved the Mac ink rate
beside a live canvas. Clipping does not narrow SwiftUI hit testing, and
squircled controls use their whole bounds or the same shape as their content
shape. Open drawers publish their source bounds and
direction, and panel groups, collapsed columns and drawer bodies square the
same corners as `DrawerPlacement::source_corners`.

## Validation

- GTK: `tools/performance/workspace-motion.sh gtk` with
  `--native-test=native_squircle_corners` (corner picks reach tiles,
  shadowed subtrees convert and blurred drawer shadows stay finite; add
  `GDK_DEBUG=color-mgmt MUTTER_DEBUG_FORCE_HDR=1` for the HDR path),
  `--native-test=native_toolbar_visual_audit_input`, `--drawer-style` and
  `--drag-pickup`. `--workspace-motion` presentation rate
  is unchanged by the conversion.
- Web: `apps/layer-web/test.mjs` with `--drawer-style`, `--toolbar-components`,
  `--compact-workspaces`, `--tab-styles` and `--title-bar-state`.
- Apple: `bash apps/layer-apple/scripts/test-project-files.sh
  apps/layer-apple/tests/squircle-geometry.swift` checks the corner formula,
  clamping, joined drawer sources and source-corner flattening; the Mac and
  iPad `testToolbarComponents`, `testColumnStacks`, `testTitleBarToolDrawers`
  and `testColorPicker` journeys attach the drawer, column and title-bar
  captures.
- Android: `AndroidInteractionTest` `drawerButtonsAndBridgesKeepTheirColors`,
  `drawerTabsKeepActiveColorsAndPadding`,
  `collapsedIconsKeepTheirSourceWhenDrawerTabsAreVisible` and
  `toolbarComponentsAcrossDevicesAndLayouts`, plus `AndroidTitleBarTest` in
  the landscape orientation recorded by its acceptance.
