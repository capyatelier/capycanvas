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
  labeled tiles become squircle capsules. Title-bar tools, menu labels,
  readouts, document tabs, the workspace switcher and status bubbles follow the
  same rule with the header tile.
- **Standalone toolbars** use their tile radius, so end tiles fill the toolbar
  ends exactly. `TileStyle::corner_radius` is published per panel as
  `tile_corner_radius`.
- **Surfaces** (panels, tab tops, drawers, collapsed columns, the title-bar
  editor, notices and the brush-size preview) use `SURFACE_RADIUS`, 18px or
  half a small tile.
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
never clips the square join.

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
circular radius (design radius × `--corner-fit`). GTK's own hit testing,
overflow picking and blurred shadows therefore cover every painted pixel.
`squircle::Squircles` wraps the workspace window content and redraws each
circular rounded clip, uniform border or outline, and crisp shadow ring as a
squircle with the circle radius divided by the fit. Undesigned toolkit radii
keep their former visual rounding the same way. Masks are rasterized once per
device-pixel geometry and reused as GPU textures; unchanged render nodes,
including shadow-wrapped subtrees, are reused between frames.
`squircle::Popover` applies the same conversion to the brush-size preview.
Subtrees that must stay round are drawn through `squircle::append_round`.
Rust-drawn rounded rectangles in the workspace multiply design radii by
`squircle::CORNER_FIT`.

Android uses `SquircleShape`, a Compose `CornerBasedShape`, through
`TileShape`, `SurfaceShape` and `ControlShape`; custom paths share
`squircleCorner`. Compose pointer input already uses whole bounds.

## Validation

- GTK: `tools/performance/workspace-motion.sh gtk` with
  `--native-test=native_squircle_corners` (corner picks reach tiles and
  shadowed subtrees convert), `--native-test=native_toolbar_visual_audit_input`,
  `--drawer-style` and `--drag-pickup`. `--workspace-motion` presentation rate
  is unchanged by the conversion.
- Web: `apps/layer-web/test.mjs` with `--drawer-style`, `--toolbar-components`,
  `--compact-workspaces`, `--tab-styles` and `--title-bar-state`.
- Android: `AndroidInteractionTest` `drawerButtonsAndBridgesKeepTheirColors`,
  `drawerTabsKeepActiveColorsAndPadding`,
  `collapsedIconsKeepTheirSourceWhenDrawerTabsAreVisible` and
  `toolbarComponentsAcrossDevicesAndLayouts`, plus `AndroidTitleBarTest` in
  the landscape orientation recorded by its acceptance.
