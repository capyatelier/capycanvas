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
  editor and notices) use `SURFACE_RADIUS`, 18px or half a small tile.
- **Panel controls** (buttons, entries, dropdowns, segmented choices and list
  highlights) use 12px, concentric with surfaces at the standard 6px inset.
  Controls 24px tall or shorter become capsules.
- **Concave joins** (tab feet, drawer bridges and expanded-panel joins) keep
  their sizes and use inverted squircle curves.
- Checkboxes, thumbnails, slider thumbs and other small details keep their
  radii with squircle corners.

A drawer's source container flattens facing corners within `SURFACE_RADIUS` of
the opening tile (`DrawerPlacement::source_corners`), so a padded container's
larger rounding never clips the square join.

## Scope

Squircles cover the workspace: the title bar, toolbars, panels, tabs, drawers,
collapsed columns, status notices and the controls inside them. Settings,
dialogs, menus, tooltips and popovers, including the brush-size preview, keep
platform styling. True circles stay round: color swatches and wheel buttons,
wheel markers, dials, radio indicators, gradient stops and native window
controls.

## Hosts

- **GTK** CSS has no `corner-shape`. `squircle::Squircles` wraps the workspace
  window content and converts circular rounded clips, uniform borders and
  outlines, and crisp inset rings into alpha masks. Each mask is rasterized
  once per device-pixel geometry and reused as a GPU texture; unchanged render
  nodes are reused between frames. Blurred shadows keep GTK's circular
  approximation. Subtrees that must stay round are drawn through
  `squircle::append_round`, and libadwaita dialogs and popovers sit outside the
  wrapper. GTK CSS radii therefore mean the same as the Web values.
- **Web** scopes `corner-shape: squircle` to `#workspace`, `#status` and
  `.image-placement-controls`; popups and circles opt back to `round`. Browsers
  without `corner-shape` scale radii by `--corner-fit` (0.55), keeping tiles as
  rounded squares rather than circles.
- **Android** uses `SquircleShape`, a Compose `CornerBasedShape`, through
  `TileShape`, `SurfaceShape` and `ControlShape`; custom paths share
  `squircleCorner`.

## Validation

- GTK: `tools/performance/workspace-motion.sh gtk` with
  `--native-test=native_toolbar_visual_audit_input` and `--drawer-style`.
  `--workspace-motion` presentation rate is unchanged by the conversion.
- Web: `apps/layer-web/test.mjs` with `--drawer-style`, `--toolbar-components`,
  `--compact-workspaces` and `--title-bar-state`.
- Android: `AndroidInteractionTest` `drawerButtonsAndBridgesKeepTheirColors`,
  `drawerTabsKeepActiveColorsAndPadding`,
  `collapsedIconsKeepTheirSourceWhenDrawerTabsAreVisible` and
  `toolbarComponentsAcrossDevicesAndLayouts`.
