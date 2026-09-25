# Panel transparency

[Workspace and UI](README.md) · [Theme colors](theme-colors.md)

**Appearance → Panel transparency** offers Off, Low (the default), Medium and
High. It is a shared setting (`Settings::transparency`) presented by GTK, Web and
Android. The other levels show a blurred copy of the artwork behind panels, tab
strips, drawers, their connectors and title-bar controls. Controls inside
panels, such as inputs, lists and sliders, stay opaque.

Off keeps the opaque theme. Title-bar controls, the zoom readout and the Zen
button are opaque too, rather than the earlier translucent fill with no blur.
At Off every `GlassPalette` color equals its opaque role, so hosts apply the
glass colors in every mode and gate only the blur.

Each option is drawn as a circle. Off is a solid disc. The other levels are
glass discs over a faint checkerboard with a soft highlight. The tint uses the
level's real alpha, so more of the checkerboard shows at higher levels. The
preference row publishes each level's light and dark alpha
(`ChoicePresentation::Circles`).

## Why the presenter draws the blur

No host can blur the canvas with its toolkit:

- On GTK the canvas is an app-owned Vulkan subsurface below the GTK window. GTK
  never has those pixels, and Mutter implements no compositor blur protocol.
- On Android the canvas is a `SurfaceView` beneath Compose; `RenderEffect`
  cannot sample it.
- On Web a CSS `backdrop-filter` over the continuously repainting WebGPU canvas
  makes the browser compositor re-blur every canvas frame.

Instead, hosts draw translucent fills and publish each fill's bounds and corner
radii. The shared `ViewportPresenter` blurs its own artwork beneath them:

- GTK: `DockSurface::snapshot` walks each child's render nodes for backgrounds
  whose color exactly matches one of the palette's glass surface colors
  (`GlassPalette::surfaces`). Each match becomes a region: its bounds clipped
  by enclosing clip nodes, and the corner radii of its enclosing rounded clip.
  Radii use the same superellipse conversion as the
  [squircle](squircle-corners.md) converter. Nodes inside a found surface are
  skipped, so panel content is not visited.
- Web: `glass.js` measures the translucent DOM surfaces and their CSS radii in
  the canvas frame, and `set_glass` scales them to device pixels. Layout,
  theme and Zen changes queue a new measurement for the next canvas frame.
- Android: `Modifier.glass(shape, color)` draws the fill and registers its
  surface-pixel bounds and radii. `CanvasHost` sends all regions once per layout
  pass through `Native.glassRegions`.
- Drawer and column connectors add their rectangle and concave feet from the
  shared `DrawerConnection::glass` geometry.

The presenter renders the artwork again at quarter resolution, with a camera of
four times the pixel footprint, into a dual-Kawase pyramid and caches the
finished half-resolution blur. It draws the glass in its own pass, above the
cursor and color picker and below Navigator overviews, which are neither blurred
nor used as blur input. Each region's fully covered interior (the cross of a
5×5 grid through its corners) takes one texture tap, and the viewport skips
those pixels. Corner blocks and the antialiased outline are drawn with the
region's superellipse or concave coverage.

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

The blur is cached. Its input is the composite repaint plus selection paint;
cursor, picker and overviews do not affect it. When that damage lies within the
blur reach of cached glass, only the glass within reach of the damage is
recomputed, from its own reach of artwork. New regions, regions that move beyond
a 96-pixel slack, and camera changes recompute whole regions. Retained targets,
such as Android's front buffer, repaint only glass that changed. Idle views
present nothing.

GPU time per presented frame on an RTX PRO 6000 (`backdrop_blur_cost`, the
default Paint layout's glass):

| Surface (glass share) | Viewport only | Cached glass | Moving camera, Low / High |
| --- | --- | --- | --- |
| 1600×1000 @1× (35%) | 0.015 ms | 0.013 ms | 0.041 / 0.046 ms |
| 3200×2000 @2× (35%) | 0.037 ms | 0.033 ms | 0.072 / 0.078 ms |
| 3840×2160 @1× (15%) | 0.045 ms | 0.044 ms | 0.077 / 0.082 ms |
| 5120×2880 @2× (23%) | 0.071 ms | 0.070 ms | 0.109 / 0.117 ms |

Cached glass costs no more than the plain viewport, because its interiors
replace viewport pixels. A moving camera recomputes every frame.

The Huion tablet (Mali-G57) sets the tight budget. Its pen measurements compare
the same build at each level with the previous main, drawing an 18 px round
brush on a 1024 px document with replayed OS pen input.

Web, Chrome on the tablet, a stroke across the fitted document (3–6 runs of 5 s):

| Level | Updates/s | Input → submit p50 / p95 / p99 |
| --- | --- | --- |
| Previous main | 85.9 | 27.8 / 31.5 / 32.9 ms |
| Off | 83.7 | 28.3 / 31.7 / 33.2 ms |
| Low | 85.1 | 27.8 / 31.6 / 32.9 ms |
| Medium | 83.5 | 28.4 / 32.1 / 33.5 ms |
| High | 80.9 | 28.3 / 31.8 / 33.7 ms |

Low and Medium reuse the cached blur on every frame of this stroke. High's
139-pixel reach touches the panels, so it recomputes about half of its frames.

Android, front-buffer presentation, submission to GPU completion of each
presentation:

| Stroke | Previous main p50 / p95 | Low | Medium | High |
| --- | --- | --- | --- | --- |
| Across the fitted document | 1.77 / 4.38 ms | 1.64 / 4.34 ms | — | 3.34 / 5.82 ms |
| Zoomed 2×, beside the panels | 2.78 / 4.95 ms | 3.10 / 7.40 ms | 3.34 / 7.67 ms | 5.15 / 9.45 ms |

A stroke beside the panels must refresh the glass it blurs into, so this cost
cannot be cached away. Two things set its size on the tablet: contact brushes
report composite damage in 256-pixel document pages, so each frame refreshes
the glass near whole pages rather than near the dab; and each refresh runs
four render passes at Low and Medium, and six at High, on a tile-based GPU where
every pass carries a fixed cost.

## Known limitations

- The GTK canvas subsurface is desynchronized from GTK, so while a panel moves
  its blur follows about one frame behind. Web and Android place glass in the
  canvas frame that follows the layout.
- Popovers, menus and tooltips are separate surfaces and stay opaque.
- The blur only reaches the canvas. A drawer that opens over another panel,
  such as the Paint tool drawer over the Tool Set column, is translucent over
  that panel too, so its content shows faintly through the drawer on every
  host.
- Near the panels, Android pen completion slows as measured above. Exact
  visual damage from the brush engine, which would also speed up ordinary
  front-buffer repaint, or refreshing glass less often than ink, would reduce it.
- Drawer shadows are cut only beneath connectors. Beneath a translucent source
  toolbar or column they can darken it by about one level; drawer-style tests
  therefore run at Off, as on GTK.
- Web Zen glass follows the chosen visibility, not the fade animation.

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
bash tools/performance/workspace-motion.sh web --preferences
```

The capture test paints bands across the window and captures the Paint, Sketch
(header drawer) and Photo (column drawer) workspaces and Preferences.
`LAYER_GLASS_THEME=light` and `LAYER_GLASS_LEVEL=off|low|medium|high` select
the variant.

For tablet pen timing, `LAYER_PEN_TRANSPARENCY=off|low|medium|high` selects the
level in the [Web pen harness](../development/web-pen-huion-2026-09-20.md), and
each run reports recomputed and reused glass frames. The Android viewport
benchmark takes `-e transparency low`, `-e zoomSteps 2` and `-e strokeOffset 0.25`
(a fraction of the work area toward the right-hand panels) alongside the
[front-buffer benchmark arguments](../development/android-front-buffer-results-2026-09-20.md);
`android-viewport-report.py` reports `completion_ms`.
