# Panel transparency

[Workspace and UI](README.md) · [Theme colors](theme-colors.md)

**Appearance → Panel transparency** offers Off, Low (the default), Medium and
High. It is a shared setting (`Settings::transparency`); only GTK presents it so
far. The other levels show a blurred copy of the artwork behind panels, tab
strips, drawers, their connectors and title-bar controls. Controls inside
panels, such as inputs, lists and sliders, stay opaque.

Off keeps the opaque theme. Title-bar controls, the zoom readout and the Zen
button are opaque too, rather than the earlier translucent fill with no blur.

Each option is drawn as a circle. Off is a solid disc. The other levels are
glass discs over a faint checkerboard with a soft highlight. The tint uses the
level's real alpha, so more of the checkerboard shows at higher levels.

## Why the canvas worker draws the blur

On GTK the canvas is an app-owned Vulkan subsurface below the GTK window. GTK
never has those pixels, so CSS or GSK blur cannot sample them. Mutter implements
no compositor blur protocol, and importing the canvas into GTK every frame would
make GTK redraw its panels on every canvas frame.

Instead, GTK draws translucent fills and the canvas worker blurs its own
presented image beneath them:

1. `DockSurface::snapshot` walks each child's render nodes for backgrounds whose
   color exactly matches one of the palette's glass surface colors
   (`GlassPalette::surfaces`). Each match becomes a region: its bounds clipped by
   enclosing clip nodes, and the corner radii of its enclosing rounded clip.
   Radii use the same superellipse conversion as the
   [squircle](squircle-corners.md) converter. Once a surface is found, nodes
   inside it are skipped, so panel content is not visited.
2. Drawer and column connectors are Cairo drawings, so their rectangle and
   concave feet come from the shared `DrawerConnection` geometry.
3. The worker presents the viewport, runs a dual-Kawase blur restricted to the
   regions' neighborhoods, and writes the result inside each region's
   superellipse or concave shape. Navigator overviews are drawn afterwards, so
   they are neither blurred nor used as blur input.

## Colors

`GlassPalette` (in `layer-ui`) solves every translucent fill so that, composited
over the base color, it reproduces today's opaque color. The glass therefore
looks unchanged over an empty canvas, and only artwork behind it shows through.

    fill = (target − (1 − α) · under) / α

- Panel bodies, drawers, connectors and open tiles use the level's surface alpha.
- Inactive tab strips and toolbars with an open drawer use a lower alpha, so they
  are fainter than the active panel over artwork. They stay darker over the base.
- A selected tab, and an open tile on such a toolbar, uses
  `α = 1 − (1 − α_panel) / (1 − α_strip)`. Over any backdrop it composites to
  exactly the panel body, so the active tab and its content read as one surface.
- Selected tools, layers and workspaces are translucent fills over their parent
  surface.
- Dark themes use the same levels at about two thirds of the transparency: dark
  High uses the light Medium alphas.
- Title-bar chips, the zoom readout and the workspace switcher are filled with
  the base color, so they stay invisible over the canvas surround in every
  level. Their per-level alpha is tuned separately from the panels':
  - Dark alphas match the panel's perceived visibility. That is the mean
    OKLab-lightness deviation from what lies behind, over one third white paper
    and two thirds grey artwork tones.
  - Light alphas are the best compromise between two targets: look like the
    active panel over white paper, and like an inactive tab strip over dark
    grey. The compromise minimizes the summed squared OKLab difference. Low
    and Medium weight the white-paper target twice as much, so those chips stay
    as close to the panels on white as High's.
  - Tests re-derive both calibrations from the table.
- Selected states (document tab, header and workspace selection, selected tools
  and layers) keep at least 60% of their opaque OKLab color difference from
  their parent over white paper and over dark grey. Their alpha rises where
  needed. Some cases cannot reach 60% at any alpha, so they use the best
  achievable. In dark Low the header selection reaches about 40% over white,
  because the chip itself lightens toward the selection color. In light High the
  document tab reaches about 42% over white.
- Drop shadows are cut away beneath drawer and column connectors, so a
  translucent bridge matches the surfaces it joins.

Measured alphas:

| Level | Panel | Chips | Document tab | Header / workspace selection | Tool / layer selection |
| --- | --- | --- | --- | --- | --- |
| Dark Low | 0.91 | 0.83 | 0.81 | 0.81 / 0.81 | 0.81 |
| Dark Medium | 0.82 | 0.75 | 0.71 | 0.93 / 1.00 | 0.71 |
| Dark High | 0.72 | 0.65 | 0.55 | 0.68 / 0.77 | 0.55 |
| Light Low | 0.86 | 0.64 | 0.71 | 0.70 / 0.70 | 0.70 |
| Light Medium | 0.72 | 0.48 | 0.75 | 0.55 / 0.55 | 0.77 |
| Light High | 0.56 | 0.43 | 0.75 | 0.41 / 0.40 | 0.40 |

`LAYER_GLASS_PROBE=1` makes the capture test paint white and dark-grey
backdrops. It prints element rectangles (`ELEM`) for the Paint, Sketch and Photo
scenes, so compositor captures can be sampled per element.

Channels outside 0–255 are clamped. Light themes with a darker base therefore
look slightly darker at High than the opaque theme.

## Cost

The blur is cached. The presenter reports the damage that affects the blur input:
composite, selection paint, picker and per-segment cursor bounds, excluding
Navigator overviews. The expensive passes rerun only when that damage is within
the blur reach of a glass region, when the camera changes, or when a region
moves beyond a 96-pixel slack. Otherwise only the final masked pass runs. Idle
windows present nothing.

These costs were measured on an RTX PRO 6000 (Mutter headless, 120 Hz) for the
default Paint layout. Cost is per presented frame.

| Surface | Full recompute | Cached frame | Viewport present |
| --- | --- | --- | --- |
| 1600×1000 @1× | 0.025 ms | 0.009 ms | 0.011 ms |
| 3200×2000 @2× | 0.042 ms | 0.019 ms | 0.034 ms |
| 5120×2880 @2× | 0.051 ms | 0.022 ms | 0.067 ms |

`native_frame_pacing` at Medium, compared with Off:

- Pan and Hand stay at 120 fps. The camera changes every frame, so the blur
  recomputes each frame: about +0.1 ms GPU and +0.1 ms worker CPU.
- Brush strokes keep their frame rate and reuse 35–65% of blurs in this
  near-panel stress stroke.
- G-Pen shows a small tail: about 0.2% more late frames, concentrated in
  occasional bursts.
- During floating-panel drags, 95–99% of canvas frames reuse the blur. GTK's
  drag frame rate is unchanged.

## Known limitations

- The canvas subsurface is desynchronized from GTK, so while a panel moves its
  blur follows about one frame behind.
- Popovers, menus and tooltips are separate surfaces and stay opaque.

## Validation

```bash
cargo test --locked -p layer-ui glass
cargo test --locked --release -p layer-render-wgpu backdrop_blur -- --test-threads=1
cargo test --locked --release -p layer-render-wgpu backdrop_blur_cost -- --ignored --nocapture
LAYER_NATIVE_CAPTURE_DIR="$PWD/artifacts/glass" LAYER_GLASS_LEVEL=medium \
  bash tools/performance/workspace-motion.sh gtk --native-test=native_backdrop_blur_capture
LAYER_PACING_TRANSPARENCY=medium \
  bash tools/performance/workspace-motion.sh gtk --native-test=native_frame_pacing
LAYER_MOTION_TRANSPARENCY=medium bash tools/performance/workspace-motion.sh gtk --workspace-motion
```

The capture test paints bands across the window and captures the Paint, Sketch
(header drawer) and Photo (column drawer) workspaces and Preferences.
`LAYER_GLASS_THEME=light` and `LAYER_GLASS_LEVEL=off|low|medium|high` select
the variant.
