# Panel transparency

[Workspace and UI](README.md) · [Theme colors](theme-colors.md)

**Appearance → Panel transparency** offers Off, Low (the default), Medium and
High. It is a shared setting (`Settings::transparency`) presented by GTK, Web,
Android, macOS, iPadOS and Windows. The other levels show a blurred copy of the
artwork behind panels, tab strips, drawers, their connectors and title-bar
controls. Controls inside panels, such as inputs, lists and sliders, stay opaque.

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
- On macOS and iPadOS a SwiftUI material over the continuously presenting
  `CAMetalLayer` would also re-blur in the compositor every canvas frame, with
  system tints instead of the calibrated palette.
- On Windows the canvas is a D3D12 swap chain beneath the WinUI tree, drawn by
  the same presenter.

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
- macOS and iPadOS: `glassSurface` and `GlassRegistration` draw each glass
  fill as a mask-free squircle and register its bounds and design radii in the
  editor-workspace space. One `GlassRegistry` per window sends them, once per
  run-loop turn, through `capy_apple_glass_regions`; the render owner scales
  them to surface pixels each frame, so display changes need no republication.
- Windows: `WorkspaceView` measures panel groups, collapsed columns, drawers
  and the zoom readout. `HeaderView` walks its tree for backgrounds painted
  with a glass brush and skips their contents. After each workspace or header
  layout pass, `CanvasWindow` sends the regions and drawer connections through
  `capy_glass`, and the host scales them to physical pixels.
- Drawer and column connectors add their rectangle and concave feet from the
  shared `DrawerConnection::glass` geometry.

The presenter renders the artwork again at quarter resolution, with a camera of
four times the pixel footprint, into a dual-Kawase pyramid (Low and Medium go
down to 1/8, High to 1/16). The last upsample writes the finished
quarter-resolution blur straight into a cache, over the glass interiors only.
Stopping at quarter resolution rather than half removes the most expensive
pass and a copy; Low and Medium use Kawase offsets of 2.9 and 3.4 so the blur
keeps its earlier width. It draws the glass in its own pass, above the
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
- Dark themes use about two thirds of the light theme's transparency at each
  level.
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
  achievable. In dark Low the header selection reaches about 46% over white,
  because the chip itself lightens toward the selection color. In light High the
  document tab reaches about 42% over white.
- Drop shadows are cut away beneath drawer and column connectors, so a
  translucent bridge matches the surfaces it joins. Windows Composition shadows
  would also fill beneath their own surface, so Windows clips panel and drawer
  shadows to the outside of the surface with a Direct2D geometry.

Measured alphas:

| Level | Panel | Chips | Document tab | Header / workspace selection | Tool / layer selection |
| --- | --- | --- | --- | --- | --- |
| Dark Low | 0.96 | 0.88 | 0.87 | 0.86 / 0.86 | 0.86 |
| Dark Medium | 0.845 | 0.77 | 0.74 | 1.00 / 1.00 | 0.74 |
| Dark High | 0.72 | 0.65 | 0.55 | 0.68 / 0.77 | 0.55 |
| Light Low | 0.94 | 0.71 | 0.78 | 0.78 / 0.78 | 0.78 |
| Light Medium | 0.76 | 0.51 | 0.75 | 0.59 / 0.59 | 0.96 |
| Light High | 0.56 | 0.43 | 0.75 | 0.41 / 0.40 | 0.40 |

`LAYER_GLASS_PROBE=1` makes the capture test paint white and dark-grey
backdrops. It prints element rectangles (`ELEM`) for the Paint, Sketch and Photo
scenes, so compositor captures can be sampled per element.

Web and Android apply the same fills, and browsers and Android's compositor
blend them in sRGB as GTK does. Probed over the same white and dark-grey
backdrops at every level and theme, the share of the backdrop showing through
each surface matches GTK's captures within 0.02 on Web (Chrome) and 0.01 on
Android. Selection fills follow the accent, so a system accent can change their
alpha: a tan Android accent reaches opaque in dark Low.

Channels outside 0–255 are clamped. Light themes with a darker base therefore
look slightly darker at High than the opaque theme.

## Cost

The blur is cached. Its input is the composite repaint plus selection paint;
cursor, picker and overviews do not affect it. When that damage lies within the
blur reach of cached glass, only the glass within reach of the damage is
recomputed, from its own reach of artwork. New regions, regions that move beyond
a 96-pixel slack, and camera changes recompute whole regions. Glass recomputed
on consecutive camera frames keeps no slack, since the next frame recomputes it
anyway. While a brush
stroke is in progress (`has_active_stroke`), artwork damage leaves the cached
blur in place; the first frame after the stroke refreshes the glass it
reached, so pen latency never includes a glass refresh. Retained targets, such
as Android's front buffer, repaint only glass that changed. Idle views present
nothing.

GPU time per presented frame on an RTX PRO 6000 (`backdrop_blur_cost`, the
default Paint layout's glass):

| Surface (glass share) | Viewport only | Cached glass | Moving camera, Low / High |
| --- | --- | --- | --- |
| 1600×1000 @1× (35%) | 0.015 ms | 0.013 ms | 0.026 / 0.031 ms |
| 3200×2000 @2× (35%) | 0.037 ms | 0.033 ms | 0.048 / 0.054 ms |
| 3840×2160 @1× (15%) | 0.044 ms | 0.044 ms | 0.055 / 0.060 ms |
| 5120×2880 @2× (23%) | 0.070 ms | 0.069 ms | 0.081 / 0.089 ms |

Cached glass costs no more than the plain viewport, because its interiors
replace viewport pixels. A moving camera recomputes every frame.

The Huion tablet (Mali-G57 MC2, 2400×1600) sets the tight budget. For
navigation, two injected fingers pan or pinch a 1024 px document at 205% for
3×5 s (`AndroidViewportBenchmarkTest` with `motion`); the latency runs from each
frame's newest input to its GPU completion (p50):

| Motion | Off | Low | Medium | High |
| --- | --- | --- | --- | --- |
| Pan, half-resolution blur | 25.7 ms | 31.1 ms | 30.7 ms | 32.2 ms |
| Pan, quarter-resolution blur | 25.8 ms | 27.4 ms | 27.1 ms | 28.1 ms |
| Pinch, half-resolution blur | 29.5 ms | 41.5 ms | 40.2 ms | 42.5 ms |
| Pinch, quarter-resolution blur | 29.4 ms | 36.8 ms | 37.5 ms | 37.9 ms |

Panning at Low now presents 88 frames/s against 89 at Off (76 before). A pinch
already keeps this GPU nearly busy at Off while display mips follow the zoom,
so even the smaller recompute makes it GPU-bound and queues a frame.

The tablet's pen measurements compare the same build at each level, drawing an
18 px round brush on a 1024 px document with replayed OS pen input.

Android, front-buffer presentation, submission to GPU completion of each
presentation (p50 / p95):

| Stroke | Off | Low | Medium | High |
| --- | --- | --- | --- | --- |
| Across the fitted document | 2.88 / 5.00 ms | 2.83 / 4.93 ms | — | 2.80 / 4.91 ms |
| Zoomed 2×, beside the panels | 2.93 / 5.83 ms | 2.85 / 5.88 ms | 2.86 / 6.04 ms | 2.95 / 5.96 ms |

Before strokes held the glass, the near-panel stroke cost +0.4 / +1.9 ms at
Low and +2.1 / +4.2 ms at High: contact brushes report damage in 256-pixel
document pages, and each refresh runs four to six render passes, each with a
fixed cost on this tile-based GPU.

Web, Chrome on the tablet, a stroke across the fitted document (3–6 runs of
5 s each; updates/s drift by several between runs as the tablet warms):

| Level | Updates/s | Input → submit p50 / p95 / p99 |
| --- | --- | --- |
| Off | 80.2 | 28.4 / 32.0 / 33.4 ms |
| Low | 77.4 | 28.3 / 32.2 / 33.8 ms |
| Medium | 75.8 | 28.0 / 32.0 / 34.3 ms |
| High | 77.3 | 28.3 / 32.2 / 33.6 ms |

Every level reuses the cached blur for the whole stroke; High, whose 123-pixel
reach touches the panels, refreshes it once when the stroke ends.

Apple Release builds, the synthetic `ink` workload (2048 px document, 24 px
brush, 60 s after warm-up) with the Paint glass registered, measured before
strokes held the glass:

| Host | Before glass | Low | High |
| --- | --- | --- | --- |
| Mac, M2 Pro, 90 Hz | 84.8 fps, GPU p50 1.09 ms | 84.4 fps, 1.47 ms | 83.9 fps, 1.55 ms |
| iPad Pro 13 M4, 120 Hz | 112.6 fps, GPU p50 1.26 ms | 113.2 fps, 1.35 ms | — |

## Known limitations

- The GTK canvas subsurface is desynchronized from GTK, so while a panel moves
  its blur follows about one frame behind. Web and Android place glass in the
  canvas frame that follows the layout.
- Popovers, menus and tooltips are separate surfaces and stay opaque.
- The blur only reaches the canvas. A drawer that opens over another panel,
  such as the Paint tool drawer over the Tool Set column, is translucent over
  that panel too, so its content shows faintly through the drawer on every
  host.
- Glass over artwork that a stroke is still painting shows the new ink only
  when the stroke ends.
- The blur reach is in device pixels, so on a 2× display it covers half the
  distance it does at 1×. Scaling it to logical pixels roughly doubled the
  refreshed area on the tablet and added pen latency, so it was not adopted.
- Drawer shadows are cut only beneath connectors. Beneath a translucent source
  toolbar or column they can darken it by about one level; drawer-style tests
  therefore run at Off, as on GTK.
- Web Zen glass follows the chosen visibility, not the fade animation.
- On macOS and iPadOS SwiftUI commits and Metal presentation are not
  synchronized, so glass can trail a moving panel by a frame. Implicit SwiftUI
  animations, such as the title-bar bars sliding, report only their final
  geometry. Shadows are drawn outside each glass surface only, so a translucent
  surface never shows its own shadow; drawer and column shadows are also cut
  beneath their connectors. Neither platform's Reduce Transparency setting is
  applied yet; Off is the opaque mode, as on the other hosts.
- Windows sends glass after XAML layout, so a moving panel's blur can trail
  the panel by a frame, as on GTK.

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
cargo test --locked -p layer-windows --lib glass
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-transparency.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
```

The capture test paints bands across the window and captures the Paint, Sketch
(header drawer) and Photo (column drawer) workspaces and Preferences.
`LAYER_GLASS_THEME=light` and `LAYER_GLASS_LEVEL=off|low|medium|high` select
the variant.

On macOS and iPadOS, `cargo test --locked -p layer-apple glass` checks region
validation, display scaling and connector feet, and the `testPanelTransparency`
journey checks the Settings circles and that panels over paper brighten from
Off to Low to High. `CAPY_WORKLOAD_TRANSPARENCY=off|low|medium|high` selects the
level for the Apple drawing workloads.

For tablet pen timing, `LAYER_PEN_TRANSPARENCY=off|low|medium|high` selects the
level in the [Web pen harness](../development/web-pen-huion-2026-09-20.md), and
each run reports recomputed and reused glass frames. The Android viewport
benchmark takes `-e transparency low`, `-e zoomSteps 2` and `-e strokeOffset 0.25`
(a fraction of the work area toward the right-hand panels) alongside the
[front-buffer benchmark arguments](../development/android-front-buffer-results-2026-09-20.md).
`-e motion pan` or `-e motion pinch` replaces the stroke with two injected
fingers. `android-viewport-report.py` reports `completion_ms` and
`input_to_completion_ms`, from each frame's newest input to its GPU completion.
