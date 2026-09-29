# Panel transparency

[Workspace and UI](README.md) · [Theme colors](theme-colors.md)

**Appearance → Panel transparency** offers Off, Low (the default), Medium and
High. It is a shared setting (`Settings::transparency`) presented by GTK, Web,
Android, macOS, iPadOS and Windows. The other levels show a blurred copy of the
artwork behind panels, tab strips, drawers, their connectors, title-bar
controls and the [command bar](command-search.md) on GTK, Web, Android and
Windows. The [canvas action bar](canvas-action-bar.md) is a glass surface in
the panel layer on every host that presents it, and it keeps its glass in Zen.
Controls inside panels, such
as inputs, lists and sliders, stay opaque. Menus, popovers and tooltips stay
opaque.

Off keeps the opaque theme. Title-bar controls, the zoom readout and the Zen
button are opaque too.
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
  skipped, so panel content is not visited. The command bar is a native popup
  on its own surface, outside that scan. The workspace adds its body, with
  circular corners, from the popup's position relative to the window. The
  region is refreshed when the popup's layout changes and dropped when it
  unmaps.
- Web: `glass.js` measures the translucent DOM surfaces and their CSS radii in
  the canvas frame, and `set_glass` scales them to device pixels. Each region
  carries its corner shape: workspace surfaces are squircles where the browser
  supports `corner-shape`, while the command bar dialog is circular. Layout,
  theme and Zen changes, and the command bar opening, resizing or closing,
  queue a new measurement for the next canvas frame.
- Android: `Modifier.glass(shape, color)` draws the fill and registers its
  surface-pixel bounds and radii. `CanvasHost` sends all regions once per layout
  pass through `Native.glassRegions`.
- macOS and iPadOS: `glassSurface` and `GlassRegistration` draw each glass
  fill as a mask-free squircle and register its bounds and design radii in the
  editor-workspace space. One `GlassRegistry` per window sends them, once per
  run-loop turn, through `capy_apple_glass_regions`; the render owner scales
  them to surface pixels each frame, so display changes need no republication.
- Windows: `WorkspaceView` measures panel groups, collapsed columns, drawers,
  the zoom readout, the canvas action bar and the open command bar. `HeaderView` walks its tree for backgrounds painted
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
  achievable.
- Drop shadows are cut away beneath drawer and column connectors, so a
  translucent bridge matches the surfaces it joins. Windows Composition shadows
  would also fill beneath their own surface, so Windows clips panel and drawer
  shadows to the outside of the surface with a Direct2D geometry.

The per-level alphas are in `Transparency::alphas` in
[`glass.rs`](../../crates/layer-ui/src/glass.rs).

`LAYER_GLASS_PROBE=1` makes the capture test paint white and dark-grey
backdrops. It prints element rectangles (`ELEM`) for the Paint, Sketch and Photo
scenes, so compositor captures can be sampled per element.

Web and Android apply the same fills, and browsers and Android's compositor
blend them in sRGB as GTK does. Selection fills follow the accent, so a system
accent can change their alpha.

Channels outside 0–255 are clamped. Light themes with a darker base therefore
look slightly darker at High than the opaque theme.

## Cost

The blur is cached. Its input is the composite repaint plus selection paint;
cursor, picker and overviews do not affect it. When that damage lies within the
blur reach of cached glass, only the glass within reach of the damage is
recomputed, from its own reach of artwork. New regions and regions that move
beyond a 96-pixel slack recompute whole regions. While only the camera moves,
the glass samples its cached blur through the camera change, so it moves and
zooms with the canvas, and recomputes it every fourth frame or as soon as a
panel would sample beyond it; blur recomputed mid-gesture keeps a 48-pixel slack
for that. The first still frame recomputes the exact blur. A new document,
proofing, HDR or a camera change that rebinds the display cache recomputes at
once. While a brush stroke or canvas handle drag is in progress
(`UiSession::hold_canvas_backdrop`), artwork damage leaves the cached blur in
place; the first frame after release refreshes the glass it reached. A stroke that
begins just after a gesture keeps the moved blur until it ends. Retained
targets, such as Android's front buffer, repaint only glass that changed. Idle
views present nothing.

Cached and moved glass cost no more than the plain viewport, because their
interiors replace viewport pixels. Stopping the blur at quarter resolution and
moving the cached blur between refreshes keep pan and pinch on tile-based
tablet GPUs at the frame rate of Off. Current measurements are in the tier
tables, such as [low tier](../performance/low-tier.md).

## Known limitations

- The GTK canvas subsurface is desynchronized from GTK, so while a panel moves
  its blur follows about one frame behind. Web and Android place glass in the
  canvas frame that follows the layout.
- Popovers, menus and tooltips are separate surfaces and stay opaque. The
  command bar is the exception. On GTK its popup and the canvas are separate
  surfaces, so when the result count changes its size, the blur can trail the
  new edge by a frame. The blur appears at full strength while the bar's
  entrance fades in.
- The command bar blurs only the canvas. Over a panel, as on narrow windows,
  the panel's content shows faintly through it, as with drawers.
- The blur only reaches the canvas. A drawer that opens over another panel,
  such as the Paint tool drawer over the Tool Set column, is translucent over
  that panel too, so its content shows faintly through the drawer on every
  host.
- Glass over artwork being painted or transformed refreshes when the contact
  ends.
- During a pan or pinch the blur is recomputed every fourth frame and moved
  with the canvas in between, so artwork sliding beneath a panel, and the blur
  size while zooming, trail by up to three frames. At the window edges the
  moved blur repeats its outermost pixels for up to 48 pixels.
- The blur reach is in device pixels, so on a 2× display it covers half the
  distance it does at 1×. Scaling it to logical pixels would roughly double the
  refreshed area and add pen latency on tablets.
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
node apps/layer-linux/bench/summarize.mjs /tmp/layer-wayland-pacing.json
LAYER_MOTION_TRANSPARENCY=medium bash tools/performance/workspace-motion.sh gtk --workspace-motion
bash tools/performance/workspace-motion.sh web --preferences
cargo test --locked -p layer-windows --lib glass
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-transparency.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
```

`native_command_bar_glass` (see [command search](command-search.md)) and the
Web `--command-bar` test check the command bar at each level in both themes.
Inside the bar, stripes painted behind it must be blurred and tinted; after the
bar shrinks or closes, the freed area must show them sharp again.

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
level in the Web pen harness (`tools/performance/web-pen.mjs`), and each run
reports recomputed and reused glass frames. The Android viewport benchmark
(`AndroidViewportBenchmarkTest`) takes `-e transparency low`, `-e zoomSteps 2`
and `-e strokeOffset 0.25` (a fraction of the work area toward the right-hand
panels); `-e motion pan` or `-e motion pinch` replaces the stroke with two
injected fingers. `tools/performance/android-viewport-report.py` reports `completion_ms` and
`input_to_completion_ms`, from each frame's newest input to its GPU completion.
