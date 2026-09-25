# Panel transparency: Web and Android handoff

[Developer guide](README.md) · [Design and calibration](../ui/panel-transparency.md)

GTK ships **Appearance → Panel transparency** (Off, Low, Medium, High; Low is
the default). Panels, tab strips, drawers, connectors and floating title-bar
controls become frosted glass over the artwork. The setting, every glass color
and the GPU blur pass are shared. This handoff covers the host work that Web and
Android still need. Read the [design and calibration record](../ui/panel-transparency.md)
first. It explains the color formula, the reviewed alpha table and the
constraints that the product review settled.

## What is already shared

Use these as they are; do not re-derive colors in a host.

| Piece | Location | Notes |
| --- | --- | --- |
| `Settings::transparency: Transparency` | `crates/layer-ui/src/glass.rs`, `settings.rs` | `Off`, `Low` (default), `Medium`, `High`, serialized as snake_case. |
| Preference row `PreferenceId::Transparency` ("Panel transparency") | `settings.rs` | Placed directly below Color theme. Uses `ChoicePresentation::Circles` (new JSON type `"circles"`). Hidden unless `Platform::transparency_preference()` is true; add `Web`/`Android` there when a host presents it. |
| `ThemePalette::glass: GlassPalette` | `glass.rs`, `theme.rs` | Published in `UiState.palette`. |
| `GlassColor` | `glass.rs` | Straight sRGB 0–1 plus alpha; `Display` writes `rgba(r, g, b, a)` with 0–255 channels. |
| `GlassPalette::surfaces()` | `glass.rs` | Surfaces that sit directly over the canvas and therefore need blur behind them. |
| `GlassPalette.blur` | `glass.rs` | `BlurStyle { levels, offset }` for the GPU pass. |
| `layer_render_wgpu::BackdropBlur` | `crates/layer-render-wgpu/src/backdrop_blur.rs` | Cached dual-Kawase blur plus a masked region pass. |
| `BackdropRegion`, `BackdropBlurStyle` | `backdrop_blur.rs` | Region bounds are physical pixels. A negative radius is a concave corner. `shape[0]` is the superellipse exponent: `SQUIRCLE` is 4, `CIRCULAR` is 2. |
| `ViewportPresenter::set_deferred_overviews`, `content_damage`, `encode_overviews` | `crates/layer-render-wgpu/src/present.rs` | Draw Navigator overviews after the blur, and feed the blur only the damage that changes its input. |

`GlassPalette` fields and where GTK uses them (`apps/layer-linux/src/style.css`,
the `window.capy-workspace.glass …` rules):

| Field | GTK use | Over what |
| --- | --- | --- |
| `panel` | Untabbed panel roots, tabbed panel bodies (`.panel-body`), footers, drawers, collapsed columns, connectors, header tiles with an open drawer | Canvas |
| `strip` | Inactive tab strip (`.dock-tabs`) | Canvas |
| `tab` | Selected panel tab and its concave joins | `strip` |
| `source` | Toolbar with an open drawer (`.drawer-source`) | Canvas |
| `open_tile` | Tile whose drawer is open | `source` |
| `chip` | Title-bar controls, menu labels, document tabs, zoom readout, Zen button, window-control circles | Canvas |
| `switcher` | Workspace switcher well | Canvas |
| `selection` | Selected tools/subtools, selected layer, selected tiles, checked mode buttons | `panel` |
| `header_selection` | Selected title-bar tools | `chip` |
| `switcher_selection` | Checked workspace | `switcher` |
| `document_tab` | Selected document tab | `chip` |

Behavior that every host must keep:

- At **Off** the palette is fully opaque, and so is *all floating chrome*: title
  bar, zoom readout, workspace switcher. GTK applies `chip` and `switcher` in
  every mode for exactly this reason; only the panel rules are glass-gated.
- `chip` and `switcher` keep the base color as fill, so over the canvas surround
  they are invisible at every level. This was a hard product constraint.
- `strip` is more transparent than `panel`. `tab` composites to exactly the
  panel body over any backdrop. Nest them as GTK does: the panel root is
  transparent, the body carries `panel`, the strip carries `strip`, and the
  selected tab carries `tab`. Never put a panel tint under the strip.
- Do not add borders, outlines or special-case colors to fix legibility. The
  review rejected them. Contrast floors are already in the palette alphas.
- Drop shadows must not show through translucent connectors. GTK cuts shadow
  nodes beneath connector shapes.

## Web

The canvas is a WebGPU `<canvas>` under DOM chrome, so the browser can blur it
directly. Web already uses a 3px `backdrop-filter` on title-bar controls.

1. **Colors.** Publish the `GlassPalette` fields as CSS custom properties next to
   the existing palette variables (`--panel`, `--tabbar`, … in
   `apps/layer-web/style.css`), for example `--glass-panel`. Add a
   `data-transparency` attribute or a `glass` class on `body` when
   `transparency != off`.
2. **Blur.** For each surface in `surfaces()` (panel roots or bodies, strips,
   drawers, connectors, chips, switcher), use
   `backdrop-filter: blur(Npx) saturate(…)`. Match the GTK strength: dual-Kawase
   with `levels`/`offset` from `palette.glass.blur` reaches about 65, 74 and 139
   device pixels for Low, Medium and High (see `BackdropBlurStyle::reach`).
   Start near `blur(18px)` for Low/Medium and `blur(24px)` for High, then compare
   against the GTK captures. Nested translucent elements (tab over strip,
   selection over panel) must **not** have their own `backdrop-filter`; they are
   plain translucent backgrounds, as on GTK.
3. **Structure.** Mirror the GTK layering: transparent tabbed panel root, `panel`
   on the body, `strip` on `.dock-tabs`, and `tab` on the selected tab and its
   joins. Connectors between tiles and drawers use `panel` with blur.
4. **Performance.** `backdrop-filter` over a continuously repainting WebGPU
   canvas makes the compositor re-blur every canvas frame. Measure with
   `bash tools/performance/workspace-motion.sh web --workspace-motion` and the
   web pen timing harness (`tools/performance/web-pen.mjs`) at each level. If Chrome regresses, the fallback is the shared
   `BackdropBlur` in the wasm renderer: publish DOM rectangles as regions, as
   GTK does, and draw overviews after it.
5. **Preferences.** Render `ChoicePresentation::Circles` for this row as four
   28px inline circles like the base-color swatches. Draw each with canvas or
   inline SVG:
   - Off is a solid disc.
   - Other levels paint a 4×4 checkerboard (greys 0.94 and 0.28), then a tint of
     grey 0.55 (dark theme) or 0.80 (light theme) at
     `Transparency::surface_alpha(dark)`.
   - A radial white sheen centered at (0.32, 0.26) with radius 0.5, opacity
     `0.25 + 1.2·(1−α)`.
   - A 1px outline at 45% mid-grey.
   - The selected disc gets a check: white with a dark halo in dark themes, dark
     with a light halo in light themes.

   Then return `true` from `transparency_preference()` for `Platform::Web`.
6. **Tests.** Extend the web workspace tests to assert glass classes, opaque
   floating chrome at Off, and capture light/dark screenshots at each level.

## Android

The canvas is a `SurfaceView` beneath Compose, which is the same situation as
GTK. Compose `RenderEffect`/blur cannot sample the SurfaceView, so the blur must
run in the native renderer.

1. **Regions.** Add a JNI entry modeled on
   `Java_art_capycanvas_Native_navigatorPlacements` in
   `apps/layer-android/native/src/android.rs`. `CanvasHost.kt` already batches
   overview slots as JSON on the UI thread. Send glass regions as `{bounds,
   radii, shape}` in surface pixels, and set `host.dirty` when they change.
   - Compute regions in Kotlin from the composables that draw glass fills:
     panel bodies, strips, drawers, collapsed columns, chips and the switcher.
   - Use each composable's bounds in the canvas surface and its corner radii.
     Android draws squircles; mirror `squircle.rs` (radius ÷ `CORNER_FIT`,
     normalized) or pass circular radii with `shape = CIRCULAR`.
   - Add connector rectangles and concave feet from the shared
     `DrawerConnection` geometry, as in `glass::connector` on GTK.
2. **Renderer.** Hold a `BackdropBlur` beside the presenter. Call
   `presenter.set_deferred_overviews(true)`. After `presenter.encode`, run
   `backdrop.encode(…, presenter.content_damage())` and then
   `presenter.encode_overviews`. Two differences from GTK need care:
   - Android uses `SharedDemandRefresh` with **target retention**: the presenter
     redraws only damaged regions. The final region pass must also redraw every
     glass region whenever the blur recomputed, the regions changed, or damage
     intersects a region. Otherwise stale glass remains in the retained image.
   - The blur samples the presented image. Check `SurfaceCapabilities::usages`
     for `TEXTURE_BINDING` on the shared-present swapchain. If it is missing,
     render the viewport for the region neighborhoods into an offscreen texture
     and blur that.
3. **Colors.** Apply the `GlassPalette` fields in Compose where `UiStyle.kt`
   currently uses the opaque panel/tabbar/selection colors, with the same
   nesting rules. Floating title-bar controls use `chip`/`switcher` in every
   mode.
4. **Preferences.** `Preferences.kt` handles `image_tiles`. Add `circles` with
   the same disc drawing as Web, then enable Android in
   `transparency_preference()`.
5. **Performance.** Tablets are the tight case. Measure pen latency and frame
   pacing with the Android pen and presentation harnesses
   (`tools/performance/android-*.py`) at each level before shipping. The cache
   keeps strokes away from panels to one masked pass. Panning recomputes every
   frame, which cost about one extra viewport pass on GTK.

## Reference numbers (GTK)

These were measured on an RTX PRO 6000 with Mutter headless at 120 Hz; see the
design record.

- The blur costs 0.025–0.05 ms GPU when it recomputes and 0.009–0.022 ms when
  cached. A viewport present pass costs 0.011–0.067 ms.
- Pan, Hand and brush workloads keep their frame rate. Camera motion adds about
  0.1 ms GPU and CPU per frame.
- During panel drags, 95–99% of canvas frames reuse the cached blur.

Calibration probes run `LAYER_GLASS_PROBE=1` with
`native_backdrop_blur_capture` (white and dark-grey backdrops, per-element
`ELEM` rectangles). Compare a port's colors against those captures in OKLab:
chips should match GTK's measured values within about 0.01 ΔE on both backdrops.

## Acceptance

- The setting and circles appear below Color theme. Low is the default for new
  settings, and saved values round-trip.
- At every level, in both themes, glass colors come from `palette.glass` and
  chips are invisible over the surround. At Off, everything is opaque.
- The selected tab matches its panel body. Connectors match the panels they join
  and show no shadow.
- Pan/brush frame pacing and pen latency show no measurable regression at Low.
