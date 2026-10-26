# Mid tier: 90 fps on 24 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: Wacom MovinkPad 11 (DTHA116) with a 1440 × 2200, 90 Hz panel. The
canvas is 6000 × 4000.

- Every row targets **90 fps** unless marked soft.
- **The panel held 60 Hz in the recorded runs.** The default display mode is
  60 Hz, and the firmware pins every app to 60 Hz (display policy render range
  60–60) unless the app is switched on in Settings › Display › Adaptive
  (`com.android.settings/.display.AdaptiveAPPRefreshRateActivity`), whose list
  enables only Clip Studio, ibisPaint X and Wacom's apps. The app's
  `Surface.setFrameRate(90)` reaches SurfaceFlinger but lies outside that range,
  and setting `peak_refresh_rate` or turning off `setting.adaptive_refresh.enabled`
  does not lift it. With the test app switched on, the panel runs at 90 Hz.
  Switch on your own application ID there before measuring a 90 Hz row, and
  switch it off afterwards. Installing the app again or running
  `adb shell am force-stop` switches it off; instrumentation startup also resets
  it. Enable Adaptive after benchmark setup at its ready/go barrier, and check
  the panel mode before measuring.
- **Until it presents at 90 Hz, no display-paced row can pass.** Renderer rates
  above 90 show that the rendering has headroom.
- **Huion Kamvas Pad 12 proxy rows.** Rows marked "Huion" come from the Huion
  Kamvas Pad 12: a MediaTek MT8391 with the same Mali-G57 MC2 GPU and a 90 Hz
  panel. Use them only until the MovinkPad 11 is measured.

## Navigation presentation

Measured 2026-10-25 on the reference Wacom MovinkPad 11, comparing `2d7c4d5d7`
with the presentation-admission and renderer-cache fix, final optimized APK
`55febd3740001e0ca1f89c1761e1995ade72ea5bb8b8aa96da497680c67b623d`.
Each row has three independent eight-second moving contacts, warm required
shaders, actual 90 Hz display mode and thermal status 0. Factory density 240,
font scale 1 and physical resolution are verified; the inherited density 280
and font scale 0.85 are restored afterward. Adaptive is enabled only for the
private test app as required above. Earlier density-280 runs are separate
matched diagnostics. Baseline and fixed builds use an identical test APK per
row; default-camera setup verifies accepted Save/Restore, host publication,
fresh completed GPU frames and the displayed paper pose before every contact.
Rates are median SurfaceFlinger canvas presentations, independently matched to
the latency ring. The photo is the 6000 × 4000 tier reference at Fit; default
paper uses the startup camera, Sketch workspace and Coldpress Naturalwhite at
strength 100. Navigator conditions remain separate.

| Workload and motion | Baseline fps | Fixed fps | Gain | Worst contact gap p99, ms | 90 Hz rate and gap target |
| --- | --- | --- | --- | --- | --- |
| Default 1920 × 1440 paper, rotate | 71.56 | 86.08 | +20.3% | 22.332 | Not met |
| Default 1920 × 1440 paper, pinch | 71.34 | 77.76 | +9.0% | 22.390 | Not met |
| 24 MP, Navigator closed, pan | 55.88 | 66.53 | +19.1% | 22.401 | Not met |
| 24 MP, Navigator closed, rotate | 51.99 | 61.10 | +17.5% | 22.450 | Not met |
| 24 MP, Navigator closed, pinch | 57.84 | 66.69 | +15.3% | 22.414 | Not met |
| 24 MP, Navigator open, pan | 63.86 | 77.95 | +22.1% | 22.345 | Not met |
| 24 MP, Navigator open, rotate | 58.55 | 70.39 | +20.2% | 22.367 | Not met |
| 24 MP, Navigator open, pinch | 66.62 | 73.63 | +10.5% | 22.363 | Not met |

Default rotation clears the 85.5 presentations/s floor, but its gap p99 exceeds
the 22.222 ms limit. No row meets both gates in all three contacts. The
presentation shader and quality settings are unchanged. The default camera is
restored in both model and displayed canvas; excluded startup screenshots also
record a whole-window Android transition, separate from paper motion.

Default rotation input-queue p95 rises from 1.88–2.07 to 10.46–10.60 ms;
default pinch rises from 1.99–2.09 to 15.38–15.60 ms. The driver presentation
call delays the owner despite low CPU work. This is a throughput improvement
with a response tradeoff. Vendor CPU-trace parse warnings remain; presentation
timestamps agree exactly with the independent latency ring. All per-contact
values, geometry, warnings, excluded setup attempts and provenance remain in
`artifacts/nav-fix-qualification-20261024/mid-factory240/comparison.json`.

### Touch gesture pinch diagnostic

Measured 2026-10-25 on Wacom MovinkPad 11 using the private unminified
`benchmark` APK, optimized Rust `release` profile, 6000 × 4000 reference photo
at Fit, light theme and Low glass. Each condition primes once and records
three five-second native two-finger pinches. Density is 240, font scale 1 and
thermal status 0. The active display remains **60 Hz**; these results do not
qualify the 90 Hz tier or establish a matched regression comparison.

| Navigator | Canvas presents during each complete contact | Full-contact presents/s | Full-contact gap p99, ms | First canvas present after contact start, ms | Longest gap including contact edges, ms |
| --- | --- | --- | --- | --- | --- |
| Closed | 212 / 221 / 223 | 42.36 / 44.12 / 44.44 | 66.99 / 66.95 / 50.23 | 21.63 / 76.55 / 68.17 | 117.25 / 100.36 / 100.49 |
| Open | 242 / 249 / 250 | 48.21 / 49.69 / 49.87 | 33.53 / 33.70 / 33.55 | 2.79 / 77.89 / 79.28 | 117.23 / 77.89 / 79.28 |

Actual SurfaceFlinger canvas presents are counted only inside each contact's
boot-clock start/end interval, including any previously queued frame presented
there. The first-present onset is consequently not an input-to-visible latency
measurement. Contact durations are 5.004–5.018 s closed and 5.011–5.019 s open.
The collector's submission-attributed first-to-last rates are 42.42 / 44.63 /
44.88 and 47.92 / 50.43 / 50.63; their gap p99 values are 66.99 / 66.99 / 66.98
and 66.97 / 50.26 / 50.24 ms. These include presentations 24–42 ms after lift;
full-contact accounting above retains the onset and excludes those tails.

Evidence is retained in `artifacts/touch-performance-analysis/`, including raw
run JSON, exact latch/display-present timestamps, extraction SQL, trace hashes
and processor warnings. Vendor systrace parse failures remain; the presentation
join uses FrameTimeline and SurfaceView buffer numbers. The build log records
optimized `release`, but measured APK hashes and a build-time dirty-source
snapshot were not retained before build scratch cleanup. The build uses `d141f10ba` plus the uncommitted gesture candidate; the
separately captured analysis-time source manifest cannot prove the exact measured
tree. Current-revision 90 Hz qualification remains unmeasured.

### Brush and pending-filter controls

G-Pen 1536 px on the 24 MP photo at Fit, Navigator closed, pressure 1 and
physical inclination 40°, uses nominal 200 Hz input. Fresh input-attributed GPU
completions are 31.67 / 31.48 / 31.47 → 30.89 / 31.10 / 31.12 updates/s;
completion-gap p99 is 40.77–43.00 → 43.04–46.58 ms. The median changes by
−1.2%; both builds miss the full 90 updates/s brush requirement.

The earlier matched Gaussian Blur 85 interruption control at density 280 is
explicitly diagnostic: nominal one-second navigation takes 1.358 / 1.019 /
1.014 → 1.738 / 1.321 / 1.020 s, with only one attributable present per probe.
Both builds retain severe response misses. It does not qualify factory-scale
filter performance; raw witnesses remain in
`artifacts/nav-fix-qualification-20261024/mid/gaussian-comparison.json`.

## Operations

### Dither cell sizes

| Cell size, 512 px brush | Completed updates/s on the tier canvas | 90/s target |
| --- | --- | --- |
| 2–50 px | Unmeasured | Open |

Existing Dither measurements use 1 px cells.

### Scatter cell sizes

| Cell size, 512 px brush | Completed updates/s on the tier canvas | 90/s target |
| --- | --- | --- |
| 1–50 px | Unmeasured | Open |

Scatter now uses stable staggered cells and retained stroke-start pickup.
Earlier Scatter measurements do not qualify this algorithm.


### Smoothing

Measured 2026-10-25 on the reference MovinkPad 11 with speed adjustment removed,
using G-Pen 1536 px above the 6000 × 4000 photo at Fit. Each row has three
warmed five-second contacts, factory density 240, font scale 1 and thermal
status 0 before and after. Rust uses the release profile; the benchmark APK is
unminified. Its SHA-256 is
`90f681b01e0518e7525ac892a4b0d44f73a8dd6c2b0eb0f935ac42a814172c72`.

The private app remains at 60 Hz because its Adaptive entry was not enabled.
These are diagnostics, with no 90 Hz qualification or matched baseline.

| Smoothing | Completed canvas updates/s, range | Fresh input updates/s, range | Fresh completion gap p99, range | 90/s and 22.22 ms target |
| --- | --- | --- | --- | --- |
| 6, default | 29.27–29.50 | 29.27–29.50 | 42.42–44.93 ms | Not met |
| 100 | 26.53–26.72 | 26.53–26.72 | 48.00–48.46 ms | Not met |

Raw measurements and device witnesses remain in
`artifacts/remove-smoothing/android-performance-default-report.txt`,
`artifacts/remove-smoothing/android-performance-max-report.txt` and
`artifacts/remove-smoothing/android-performance-after-device.txt`.

### Checked upload comparison

Measured 2026-10-25: `e038e3361` baseline, `c1b215667` checked dab/style upload
growth, then the baseline again. Optimized release/R8 builds use the reference
24 MP photo at Fit, factory density 240 and font scale 1, thermal status 0,
and three warmed five-second gestures per successful row. The private app's
Adaptive setting and actual 90 Hz surface period are verified after each setup.
Dither paints Photo; the other rows paint an empty layer above it.

| Row | Baseline / candidate / baseline, completed updates/s | Gap p99, ms | 90/s and gap target |
| --- | --- | --- | --- |
| G-Pen 1536 | 38.07 / 38.29 / 38.29 | 34.46 / 34.61 / 35.00 | Not met |
| Spray 1024 | 62.29 / 62.40 / 63.25 | 74.19 / 73.85 / 77.06 | Not met |
| Dither Blender 512 | 62.50 / Missing / 62.83 | 29.10 / Missing / 29.25 | Open |
| Wet Round 512 | 97.52 / 97.74 / 96.99 | 15.87 / 16.13 / 15.57 | Met for this fixture |
| G-Pen 1536, changing preview | 37.71 / 37.27 / 37.29 | 36.57 / 34.96 / 35.94 | Not met |

Wet Round's candidate contacts range from 93.34 to 97.85 completed updates/s;
its fresh-input rate agrees with the completed rate. This does not qualify
other footprints or replay. The candidate Dither contact consumes input but
does not advance the document revision; it times out with no renderer error or
pending work. Both baseline phases author successfully. The failure remains
unclassified, with no established cause or candidate throughput result. Matched
camera, brush and input witnesses are retained alongside exact APK/source proof
in `/mnt/localssd/capycanvas9-stability-android-artifacts/ad5-reference/`.
Genuine late sensor correction and whole-stroke replay remain unmeasured.

Required-only shader startup and first-use tool preparation are unmeasured on
this reference device. Earlier idle-catalog measurements describe their named
builds, not the current startup policy. See [startup preparation](responsiveness.md#demand-driven-startup)
for the Apple diagnostic and its limits.

The earlier M3 G-Pen comparisons use clean `192601dac` and the authored-model
candidate built on that revision (BUILD29/32). The cutover landed as `b3f6f8e51`;
the intervening footer/preview UI changes do not change the measured renderer.
Earlier operation rows and 83/BUILD15 effect and transform probes apply only
to their named binaries. The [selected-pixel preview diagnostic](#selected-pixel-previews)
does not qualify current selected-transform presentation.

Overall M3 performance qualification remains pending. Matched release offscreen
runs on the desktop RTX PRO 6000 compare frozen `192601dac` with the authored
candidate. Navigation CPU p95 improves in every comparison, with unchanged
camera/work/cache counters and zero source misses or recomposition. Completed
p95 exceeds 5% in 7/15 cold and 8/15 warmed observations; warmed native-scale
excesses repeat in all three pairs (+0.024–0.178 ms). All p99 shifts stay below
+1 ms. These completion spans include host queue polling, not GPU timestamps.

The 100-repeat sixteen-layer bottom-paint ABBA comparison retains exact PNGs,
frame/work counters and 110,624,776 bytes of capture backing. CPU-submit bounds
pass both temporal pairs. The reverse pair exceeds completed-motion p95 by
7.09% (+0.041 ms), loses 6.23% throughput and adds 1.025 ms to pen-up p99;
the forward pair passes. These excesses remain unresolved and do not qualify
reference-tablet rates or physical input-to-present response. Exact observations
are under `artifacts/format/m3-uninstrumented-34/` and
`artifacts/format/m3-final192-ordinary-fixture/measurements/`. Earlier passing
`4a2cf6aa0` diagnostics remain in their own artifacts and do not override these
current-source results.

The 2026-10-08 navigation rows use three 5 s gestures on the 24 MP photo,
actual SurfaceFlinger presentations, and thermal status 0. The brush/canvas
and matching gesture were warmed; optional full-catalog `shaders_ready` was
false. Source: `067f5cbb7` plus the uncommitted navigation candidate
(`navigation.rs` SHA-256 `2c0e9c5a7eee70c3e881ba9810a286cbe5a49ca1059d23a18a3892c8d1354d79`);
optimized APK SHA-256 `e74cd6b9b8f0dc15e1beb2f5e6ff6348bae20025ebc6e1f6adbd9dca7682d462`.

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Startup and first-use shader preparation | [Latency limits](responsiveness.md) | Unmeasured with required-only startup on this reference device | [Apple diagnostic](responsiveness.md#demand-driven-startup); no tier qualification |
| Tool cursor hover, with and without brush size | 90 | Unmeasured on the reference tablet | [Top-tier rendering measurements](top-tier.md#tool-cursors) do not qualify this tier |
| Pan: Hand tool, one or two fingers | 90 | **Not met.** Two-finger pan 77.95 fps median, worst p99 22.345 ms, 24 MP photo, Navigator open | [Navigation presentation](#navigation-presentation), 2026-10-25 |
| Pinch zoom | 90 | **Touch gesture revision unqualified (2026-10-25).** 60 Hz diagnostic: 42.36–44.44 presents/s closed, 48.21–49.87 open. Earlier: **Not met.** 73.63 fps median, 24 MP photo, Navigator open; default paper 77.76 fps | [Touch gesture pinch diagnostic](#touch-gesture-pinch-diagnostic); [earlier navigation presentation](#navigation-presentation), 2026-10-25 |
| Mouse-wheel pan and Ctrl-wheel zoom, including held navigation buttons | 90 | Unmeasured on the reference tablet | [Wheel input contract](../ui/shared-ui.md); desktop correctness checks do not qualify this tier |
| Two-finger rotate | 90 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** 70.39 fps median, 24 MP photo, Navigator open; default paper 86.08 fps with gap p99 22.332 ms | [Navigation presentation](#navigation-presentation), 2026-10-25 |
| Rotation snapping: two fingers and Rotate View | 90 | **Unqualified diagnostic:** 59.698–59.706 total presents/s at 60 Hz, including stationary snap holds; moving-only rate unmeasured | [Rotation snapping](#rotation-snapping), 2026-10-24 |
| Smooth Zoom tool, left/right | 90 | **Misses:** 50.20–50.33 presented fps, p99 67.00–83.73 ms, 24 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/mid/` |
| Smooth Zoom tool, up/down | 90 | **Misses:** 50.03–50.28 presented fps, p99 66.99–117.22 ms, 24 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/mid/` |
| Footer zoom and rotation sliders | 90 | Unmeasured on the reference tablet with slider snapping enabled | [Shared slider behavior](../ui/shared-ui.md#drawing-and-navigation) |
| Navigator drag | 90 | Current Web candidate unmeasured; functional geometry and camera checks do not qualify motion | [Web Navigator resize diagnostic](#web-navigator-resize-diagnostic), 2026-10-19 |
| Pan with Navigator open, checkerboard shader | 90 | Diagnostic: 60.33–60.43 renderer submissions/s; submission interval p99 19.12–19.25 ms. Presented FPS unmeasured; target unqualified | 2026-10-17, three warmed 5 s gestures, 24 MP tier photo, thermal 0; release APK `e38cf984`, base `5cee14be0` plus shared white/gray checker colors; `artifacts/checker-android/mid/` |
| Brush-cursor hover | 90 | | |
| Placed-photo drag (24 MP photo) | 90 | screen 59.0/s; renderer 99.4 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 90 | Free: screen 59.2/s; renderer 182.1 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform: Distort or Perspective | 90 | Distort: screen 59.2/s; renderer 168.2 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform: Warp | 90 | screen 59.0/s; renderer 72.5 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Selection transform, full canvas | 90 | Diagnostic only: handle 60.03–192.68 renderer submissions/s; Distort 101.67–191.57/s on a filled 24 MP canvas. Presentation and reference-photo qualification remain open | [Selected-pixel previews](#selected-pixel-previews), 2026-10-09 |
| Move tool layer drag | 90 | | |
| Object Layer placement, Move and Scale/Rotate | 90 | Current Object-layer simplification unmeasured on the reference tablet | [Qualification gap](known-gaps.md) |
| Vector layer: G-Pen 24 px on 20,000 paths | 90 | **Met.** 163.9–178.4 completed updates/s | [Vector layers](#vector-layers), 2026-10-21 |
| Vector paths: Move drag, 1 / 100 / 5,000 paths | 90 | **Met.** 89.2–89.6 presented/s at 90 Hz, p99 ≤11.5 ms, start 42–63 ms | [Vector layers](#vector-layers), 2026-10-20 |
| Vector paths: Scale/Rotate, eraser, Outline width scrub | 90 | **Met** except the 5,000-path width scrub (24.7–24.9 updates/s); Edit Points **met** (89.4–89.6/s) | [Vector layers](#vector-layers), 2026-10-21 |
| Navigation over a crowded vector layer | 90 | **Not met.** Pan 57.8–58.2, pinch 56.7–58.4, rotate 54.8–55.1 presented/s at 90 Hz, with or without 5,000 paths selected | [Vector layers](#vector-layers), 2026-10-20 |
| Marquee, Lasso or Polygon drag | 90 | | |
| Enclose and Fill: loop drag and navigation during completion | 90 | Unmeasured on the reference tablet | Shared lasso overlay; GPU region discovery starts on release. Desktop checks do not qualify this tier |
| Selection Brush or Quick Mask, 1536 px | 90 | | |
| Grow, Shrink or Feather drag, full canvas | 90, soft | | |
| Paper Scale drag with relief preview | 90 | Unmeasured on the reference tablet | Canvas-bar `paper-scale-drag`; [Paper rendering](../development/paper.md#rendering-and-performance) |
| Pointwise adjustment slider: Levels, Curves, Exposure, Hue/Saturation, Color Balance, White Balance, Black & White | 90, soft | | |
| Neighbourhood filter slider: Gaussian Blur, Unsharp Mask, Edge-Preserving Smooth | 90, soft | Gaussian at 50%: 7.2/s small, 4.6/s large fresh completed canvas updates; UI 58.7–59.3/s and 40.3–41.8/s. Fit: 38.9/s and 26.1/s fresh canvas updates | Spatial composition comparison below; target not met; other filters unmeasured |
| Saved spatial lengths beyond editor bounds, including Gaussian sigma >85 | 90, soft | Unmeasured | Full authored lengths need moving-frame qualification; earlier capped-value measurements do not apply |
| Animated or warping filter: Domain Warp, Ripple | 90, soft | **Unqualified.** Non-reference Mali-G57 MC2 tablet, 6000 × 4000 at Fit: Ripple 87.6 ms, Domain Warp 113.1 ms per animated frame, from 3.3–3.8 s; reference tablet unmeasured | [Animated filters diagnostic](#animated-filters-diagnostic), 2026-10-18 |
| Fill layer or gradient-fill edit | 90, soft | Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Paper Texture visibility, Light and Shows through paint drag | 90 | **Unqualified.** Non-reference XP-Pen, 24 MP: 55.6–60.9 completed canvas updates/s, UI 83.1–88.9/s; reference tablet unmeasured | [Paper lighting diagnostic](#paper-lighting-diagnostic), 2026-10-18 |
| Navigation with proof or tone guide shown | 90 | | |
| Gradient drag | 90 | | |
| Figure or ruler drag | 90 | | |
| Layer opacity scrub | 90 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Layer reorder drag | 90 | **Not met.** Android mouse-body drag: 58.8–59.6 window frames/s, interval p99 up to 33.49 ms, 24 MP photo, thermal 0, 60 Hz panel | `ded8453bf` + layer input port, release-profile benchmark; `artifacts/layer-input-port/android/motion-summary.json`, 2026-10-10 |
| Layer row swipe | 90 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Current handle fade unmeasured.** Earlier Android left reveal: 57.6–58.8 moving window frames/s, interval p99 up to 33.50 ms, 24 MP photo, thermal 0, 60 Hz panel; 90 Hz not met | `ded8453bf` + layer input port, 2026-10-10; [current measurements on other hardware](top-tier.md#layer-handle-fade), 2026-10-21–22 |
| Workspace-manager row swipe and reorder | 90 | Unmeasured on reference hardware; functional input checks do not qualify moving-frame timing | [Workspace manager](../ui/default-workspaces.md#workspace-manager) |
| Layer thumbnail selection animation | 90 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** 59.705–59.706 window frames/s; p99 gap 16.80–16.91 ms on the device's 60 Hz panel | Isolated outline; measurement below, 2026-10-05 |
| Attached filter drag inside a clipping run | 90 | Unmeasured: both attempts timed out waiting for the 24 MP drawing's shaders before motion | `7f69a9356` + clipping-filter changes, benchmark APK; `artifacts/clipping-filter/mid-performance{,-retry}.log`, 2026-10-04 |
| Layer swipe right: alpha lock (24 MP photo) | 90 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** Android relationship overlay 58.8–59.4 fps, interval p99 16.8–16.9 ms; Web 53.1–54.6 fps, interval p99 33.5–50.2 ms | Android `00b2d6e73` + native port, 2026-10-04; Web `1d251ece`, 2026-09-27; details below |
| Navigation with 16 visible paint layers | 90 | | |
| Drawing between 16 photo layers, G-Pen 1024 px (17 visible layers) | 90 | **Not met.** BUILD32: 74.733–75.449 fresh updates/s; fresh gap p99 18.465–18.871 ms, Linear | [Current middle-layer comparison](#drawing-in-the-middle-of-sixteen-photo-layers); measured comparison bounds pass, 90/s remains missed |
| Panel, tab, column or toolbar drag and docking | 90 | **Not met.** Floating panel-group drag frame p50/p95 13.4/15.5 ms; continuous-tab revisions unqualified on this reference device; GTK GPU joins and revised chrome handoff are unmeasured | `cbfad9e5`, 2026-09-26; [Web collector limits](#web-navigator-resize-diagnostic), 2026-10-19; [panel-tab compositing](top-tier.md#panel-tab-compositing) and [GTK column bridge diagnostic](top-tier.md#gtk-column-bridge-diagnostic), 2026-10-25 |
| Android palette pill opening at the work-area edge | 90 | **FPS unqualified.** Instant clamp/short transition; 3–4 actual presents per opening, all neighbors stationary in the diagnostic | [Native palette opening](#native-palette-opening), 2026-10-23 |
| Android tool bubble drag through four neighbours | 90 | **Not met.** Bubble release fixes: 85.784–89.064 actual app-window presents/s; p99 up to 22.339615 ms exceeds 22.2 ms, default glass, 24 MP photo at Fit | [Native tool bubbles](#native-tool-bubbles), 2026-10-24 |
| Web tool bubble drag through four neighbours | 90 | **Unqualified after the bubble release fixes.** Previous parity build missed the target: 60.01–74.59 actual presents/s, p99 33.497–33.525 ms, default Low glass, 24 MP photo at Fit | [Tool bubbles](#web-tool-bubbles), 2026-10-24 |
| Panel or column resize | 90 | Current Web candidate and GTK GPU column joins unmeasured on reference hardware. Web baseline diagnostic: 18.64–18.72 changing bounds/s, RAF gap p99 67.1–83.8 ms; presented FPS unqualified. Blank-canvas Debug divider diagnostic: 27.22–44.74 changing bounds/s, window-vsync p99 33.50–83.78 ms | [Web Navigator resize](#web-navigator-resize-diagnostic), 2026-10-19; [Divider resize diagnostic](#divider-resize-diagnostic), 2026-10-10; [GTK Color resize](top-tier.md#gtk-color-panel-resize), 2026-10-04; [GTK column bridge diagnostic](top-tier.md#gtk-column-bridge-diagnostic), 2026-10-25 |
| Pen pressure and tilt utilities, curves and live previews | 90 | 87.74–89.25 presented fps; 11.17–22.33 ms gap p99 | [Current utility measurements](#pen-response-utilities); all rate floors pass, two cases narrowly miss the 22.2 ms gap limit |
| Windows tool bubble drag through four neighbours | 90 | Unmeasured on reference hardware; Surface Pro / Adreno X1-85 functional checks do not qualify timing (PresentMon ETW access denied) | [Windows tool bubbles](../development/windows.md#tool-bubbles) |
| Windows HDR color bubble drag and intensity scrub | 90 | Unmeasured on reference hardware; native input and pixel checks do not qualify presentation timing (PresentMon ETW access denied) | [Windows tool bubbles](../development/windows.md#tool-bubbles) |
| Apple tool bubble drag through four neighbours | 90 | **Unqualified.** Reference hardware remains unmeasured; the M2 Pro Mac diagnostic uses the 61 MP canvas and does not measure the whole native overlay | [Apple native measurements](top-tier.md#apple-native-measurements), 2026-10-24 |
| HDR color bubble drag and intensity scrub | 90 | **Not met; diagnostic display settings.** Android drag 58.39–86.64 actual app-window presents/s; intensity 45.57–50.75/s, p99 up to 55.825 ms, 24 MP F16 photo at Fit; Apple reference and native overlay timing unmeasured; 61 MP Mac canvas diagnostic only | [HDR color bubbles](#hdr-color-bubbles); [Apple HDR diagnostic](top-tier.md#apple-hdr-color-bubble-diagnostic), 2026-10-24 |
| Drawer open and close | 90 | **Not met.** Layers/Properties completed native motion: 59.70–59.71 moving window frames/s, interval p99 up to 16.86 ms, 24 MP photo, thermal 0, active display mode 60 Hz | [Combined layer drawer](#combined-layer-drawer), 2026-10-10 |
| Zen fade, expanded configuration and dialog page transitions | 90 | Unmeasured on the reference tablet, including Windows header replacement during live theme changes; controlled-clock and input checks do not qualify presentation timing | [Shared chrome](../ui/shared-ui.md#window-chrome-and-zen-mode); [numeric controls](../ui/numeric-controls.md) |
| Grouped tool menus, drawer switching and tile drag | 90 | Not measured on reference hardware | [Tool variations](../ui/panel-customization.md#tool-variations); desktop functional checks do not qualify this tier |
| Colour wheel or picker drag | 90 | Huion: frame CPU p50 4.6–5.1 ms, p95 under 9.6 ms; XP-Pen swatch comparison below is diagnostic only. Huion wheel drags (FrameMetrics, release benchmark, one 5 s mouse and one touch drag each, 90 Hz panel): Edit Color dialog 79.4 / 87.4 fps presented, panel wheel 77.1 / 79.2 fps; not met | [Colour picker](../ui/color-picker.md); [swatch comparison](#selected-swatch-comparison), 2026-10-03; `AndroidWorkspacePerformanceTest#editColorWheelFrameTiming`, 2026-10-04 |
| Slider and value scrub: size, opacity, flow | 90 | Numeric input and geometry verified on MovinkPad 11; moving presentations unmeasured. Workstation diagnostics do not qualify this tier | [GTK](top-tier.md#gtk-panel-slider-diagnostic) and [Web](top-tier.md#web-numeric-panel-diagnostic), 2026-10-10 |
| Canvas action bar show, hide and move | 90 | **Not met.** UI frame p50: 22.8 ms show and hide, 34.8 ms moving the bar | `cbfad9e5`, 2026-09-26 |
| Tool Options or panel content change | 90 | **Not met.** UI frame p50 21.4 ms | `cbfad9e5`, 2026-09-26 |
| List scrolling: layers, brushes, filters | 90 | Unmeasured on the reference tablet; GTK Sketch with 1,000 layers has a desktop diagnostic only | [GTK layer input diagnostic](top-tier.md#gtk-layer-input-diagnostic) |
| Menu open and close | 90 | Application menu heading switching unmeasured on the tier canvas | [Menu interaction](../ui/window-bar.md); mouse and pen functional checks do not qualify presented frame rate |
| G-Pen 1536 px stroke with a pending language change | 90 | **Not met.** 56.80 fresh updates/s (52.40–56.99), completion-gap p99 32.08–34.97 ms | Language-change diagnostic below; synthetic owner replay, no scanout qualification |

## HDR color bubbles

The 2026-10-24 Wacom MovinkPad 11 diagnostic uses the HDR bubble port published
as `712b4a18a`, the 6000 × 4000 reference photo in an F16 drawing at Fit with an
empty paint layer, and default Low glass. The bubble size, neighboring bubbles,
warmup and three six-second contacts follow the
[top-tier protocol](top-tier.md#web-and-android-hdr-color-bubbles).

The active display reports 90 Hz and thermal status stays at 0. The device's
preexisting density override of 280 is preserved. Adaptive refresh is enabled
only for the private test app, then restored to its previous value. These
settings limit the measurements to a diagnostic; they do not establish the
default-display qualification required by the tier.

| Motion | Actual app-window presents/s, three trials | Worst presentation gap p99 |
| --- | --- | --- |
| Drag through four neighbors | 58.393 / 86.642 / 85.604 | 55.825 ms |
| Intensity | 50.746 / 49.114 / 45.570 | 55.820 ms |

The first drag contact misses the 85.5 fps floor, and all three miss the 22.2 ms
gap limit. All intensity contacts miss both criteria and visit only 154–167
distinct values per contact. No tier target is marked met. Evidence, exact
source/APK hashes and setting-restoration records are in `artifacts/hdr-color-bubble-ports/android/`.

## Native palette opening

The 2026-10-23 shared geometry change includes the open palette pill in desk
bounds. Light/dark native touch journeys passed on MovinkPad 11 and MovinkPad Pro
14. Each moves the wheel toward the bottom-right, reopens the pill after drag
release, checks its full bounds and both buttons, and verifies that Next advances
once and Previous restores the name. On this tablet the pill bottom is 1415.5 px
inside the 1440 px workspace; the top tablet has the same 24.5 px clearance.

The release-Rust, nondebuggable, unminified benchmark APK is SHA-256
`27d0bd478031fe5b5cd521f55f02a2d8357fe7a9aa703ebaa1d211e35f9e9657`.
Test APK `225cd5c700ab57497028eb209db465a5a9826ef5ab7e2e4943f9ab15f3720376`
records three native palette-bead taps on the prescribed 6000 × 4000 photo at Fit,
default glass, 90 Hz display and thermal status 0. The retained production
`BubbleInteraction.active` flag brackets each diagnostic, while cached semantic
bounds record individual drawn poses. No full-model reads occur during a window.

| Opening | Production-active window | Actual app-window presents | Distinct wheel poses | Distinct poses per neighbor |
| --- | --- | --- | --- | --- |
| 1 | 62.319 ms | 4 | 2 | 1 each |
| 2 | 75.452 ms | 3 | 2 | 1 each |
| 3 | 54.882 ms | 3 | 3 | 1 each |

The wheel clamps in one step; the third sample also has a one-pixel adjustment.
This is not sustained animation or a neighbor return. Production-active time
includes request/settling bookkeeping beyond the changed pose, so dividing its
presentation count by that duration would not qualify moving FPS. A window with fewer than two presentations cannot yield an inter-presentation
interval; the collector rejects sustained-FPS reports for these windows. The valid five-bubble drag measurements remain in their separate row.

`tools/performance/android-bubbles.py --workload palette-opening` retains the
short diagnostic without an FPS result. Reset and idle periods are excluded.
Raw presentation timestamps, per-transition drawn positions, build hashes and
both-theme bounds/screenshots are in `artifacts/bubbles/android-palette-growth/`.

## Native tool bubbles

The bubble release fixes measured 2026-10-24 on the MovinkPad 11 use the same
6000 × 4000 photo, Fit view, default glass and three six-second contacts below.
The display runs at 90 Hz and thermal status stays at 0. The optimized arm64
benchmark APK is `3482d5832154d74191cc8c315e9a9f7d16c4d02115f98c544b1b69d67c4a0789`,
with test APK `529d2eb97ca16fdf5406da94b3ab61467ae98912db2bc42d78cc63bd43b43650`.
The measured source is `59722d105` plus the release fixes, including the shared
paper scheduling change. All contacts clear 85.5 presents/s; two exceed the
22.2 ms p99 limit, so the target remains unmet. The app's Adaptive switch is
restored to its original Off setting afterward. Raw presents, contact windows,
photo/source hashes and binary provenance are in
`artifacts/bubble-release/verification/android-mid-motion/` and
`artifacts/bubble-release/verification/benchmark-provenance.json`.

Measured 2026-10-23 on the MovinkPad 11, 6000 × 4000 prescribed photo at Fit,
default glass, 90 Hz app policy and thermal status 0 throughout. Benchmark APK
SHA-256 `5c2e4751ef908941013c5428a4e60fedba484423d515f53ecb5eb62d94dbee64`
uses release Rust and unminified, nondebuggable Android code. The separate white-box
test APK is `d6d185ad72095f5d687653ba6cda1c9eb6296a817b92638b94ec4c8ead785c45`.

`AndroidBubbleBenchmarkTest#fiveBubbleMotion` warms one six-second contact, then
moves one bubble through four settled neighbors in three six-second native touch
contacts. All four neighbors visibly move each time; canonical resets and settling
occur outside measured windows. `tools/performance/android-bubbles.py` polls the
app window's actual SurfaceFlinger presentation timestamps, excluding warmup and
100 ms at each contact edge. The ready/go gate enables the exact app Adaptive row
after instrumentation starts and resumes the same private Activity. The original
OFF setting was restored after capture.

| Build | Actual presents/s, three contacts | p99 interval, ms | Result |
| --- | --- | --- | --- |
| Bubble release fixes, APK `3482d583` | 85.784 / 89.064 / 86.820 | 22.339615 / 11.304000 / 22.329154 | Not met |
| Before primitive glass transport, APK `f589c1fa` | 86.642 / 89.238 / 86.475 | 22.336615 / 11.310384 / 22.333308 | Not met |
| Primitive glass transport, APK `5c2e4751` | 89.063 / 86.296 / 87.166 | 11.289538 / 22.330692 / 22.330384 | Not met |
| Previous release Rust, APK `7014e78c` | 86.475 / 86.988 / 86.647 | 22.331154 / 22.328077 / 22.330769 | Not met |

Both primitive-glass comparison builds meet the 85.5 presents/s floor but exceed
the fixed 22.2 ms p99 limit in two contacts. The comparison does not establish a
performance improvement. In the primitive-glass capture, FrameMetrics animation
p99 is 3.12–3.68 ms, layout ≤0.542 ms and
draw ≤0.560 ms; command issue is 9.12–12.70 ms and GPU duration 12.61–18.54 ms.
These are attribution proxies, not actual presents. Raw evidence is in
`artifacts/bubbles/android-mid-final-reference/` and
`artifacts/bubbles/android-mid-transport-reference/`; each includes a build manifest,
contact windows, thermal/display records, JUnit result and raw presentation samples.

The previous APK SHA-256 is
`7014e78cd0ca4980590511875b92d05df2d8c66de37d7c6a19683bfbdc4df855`,
with test APK
`0844ff434f0f2aaa7743323160db1e7ab909b448e62655d5e0c530b6b7d03622`.
It uses release Rust and nondebuggable, unminified white-box Android code,
not an R8 release. All three contacts pass the rate floor and miss the p99 limit.
FrameMetrics command issue p99 is 12.19–13.07 ms and GPU duration
11.52–19.21 ms; layout ≤0.548 ms and draw ≤0.418 ms. Shared renderer
CPU render/present p99 is 6.16–6.36 ms and owner-thread CPU 5.33–5.47 ms.
These CPU/HWUI proxies do not isolate a shared-renderer GPU pass. Thermal status
remained 0 and the app policy/display were 90 Hz. All four neighbours moved;
JUnit passed, the same Activity was retained, and Adaptive was restored OFF.
Raw evidence is in `artifacts/bubbles/web-current/native-release/`.

Current Android visual parity fixes were measured with APK
`c9ad05fde41bde88d63b5e0a972334af9a2d9dfa7f6cc90dad4f5b4b747329eb`
and test APK
`ab325b4472515ca0690ccd7ff6f1f6a35d03e7f692eb8db6c3ec6f1b24e6937d`.
This uses release Rust and nondebuggable, unminified white-box Android code,
not an R8 release. The same warmed photo/Fit/default-Low-glass workload gives:

| Current contact | Actual presents/s | p99 gap, ms |
| --- | --- | --- |
| 1 | 85.777264 | 22.326154 |
| 2 | 87.165186 | 22.328693 |
| 3 | 86.647279 | 22.338077 |

All contacts pass the 85.5 presents/s floor and **miss** the fixed 22.2 ms p99
limit. Every neighbor moves; resets remain outside motion, thermal status is 0
and contact display refresh is 90 Hz. The SF metadata includes 60 Hz during
settings/teardown outside contacts. The ready/go gate retained the same private
Activity. JUnit passed in 48.228 s. Source/binary hashes are in
`artifacts/bubble-parity-review/performance/source.json`; raw evidence is under
`/mnt/localssd/capycanvas2-bubble-parity-review/performance/android-mid/`.

## Web tool bubbles

Current Web visual parity fixes measured 2026-10-23 on MovinkPad 11 with the
prescribed 6000 × 4000 photo below an empty paint layer at Fit and default Low
glass. The loaded web-release Wasm is
`2392b93b11b6d259d7a9ec905548b2144c495f9ae6246fb896f5e2fc39dd0cea`;
loaded CSS is
`ecd2159249290d3b22b6799fbb313b07720af5942b5a2e4c94648073950b7b47`.
Both were verified against the release package/current source. One six-second
warmup precedes three six-second touch drags through four settled neighbors.
Every neighbor visibly moves. Only moving-contact windows count, trimmed by
100 ms at each edge; direct SurfaceFlinger actual-present histories overlap
without rollover loss. Display/app policy is 90 Hz, SF period 11,111,111 ns,
and thermal status is 0 before and after.

| Current contact | Actual presents/s | p99 gap, ms | rAF/s, proxy |
| --- | --- | --- | --- |
| 1 | 74.593775 | 33.501154 | 86.416464 |
| 2 | 60.007487 | 33.524692 | 89.083898 |
| 3 | 68.090865 | 33.497462 | 88.251425 |

All three contacts **miss** both the 85.5 presents/s floor and 22.2 ms p99 limit.
Browser exceptions are empty. rAF callbacks remain a proxy, not completed or
presented frames. Supplemental Chrome FrameTimeline has 452 negative-timestamp
drops; it supplies only REALTIME/MONOTONIC clock alignment (462 ns offset
spread), never presentation counts. This run does not isolate a GPU pass cost.
Source/binary hashes are in `artifacts/bubble-parity-review/performance/source.json`;
raw evidence is under
`/mnt/localssd/capycanvas2-bubble-parity-review/performance/web-mid/`.

Previous promoted release measured 2026-10-23 on Wacom MovinkPad 11, Chrome, with the
prescribed 6000 × 4000 photo below an empty paint layer at Fit and default Low
glass. Loaded Wasm SHA-256 is
`1cf5c04479606370fa469f4adedaf3205a65ab012a3672f56f5b088b937a3110`.
The production CSS containing one transform layer per bubble is
`17497168dbc97c5d13153915e1da6291babb506b20b154605bb55a80ad0df72d`, verified
against the browser's loaded stylesheet. Chrome's UID has the temporary 90 Hz
Adaptive override; display period is 11,111,111 ns and thermal status is 0.

One six-second warmup precedes three six-second native CDP touch drags through
four actual settled neighbors. Every neighbor visibly moves. Direct overlapping
500 ms SurfaceFlinger content-surface histories are deduplicated inside each
contact with 100 ms trimmed at both edges. There is no ring rollover loss.

| Previous promoted CSS contact | Actual presents/s | p99 gap, ms | rAF callbacks/s (proxy) |
| --- | --- | --- | --- |
| 1 | 72.774 | 33.505846 | 85.918 |
| 2 | 61.447 | 33.539384 | 88.253 |
| 3 | 61.653 | 33.552693 | 88.083 |

**Not met:** each contact misses the 85.5 presents/s floor and 22.2 ms p99 limit.
Near-90 Hz callbacks do not establish presentation. Before promotion, this same
release measured 57.367 / 55.513 / 58.036 actual presents/s with p99 gaps
33.505–33.553 ms. Those are port-internal comparisons, not a pre-port main baseline.

The pre-promotion CPU/browser trace identifies Chrome GPU-service raster work:
GPU-service RunTask wall occupancy is 89.6–92.5% of moving windows, with thread
CPU occupancy 77.7–81.9%; renderer-main wall occupancy is 50.6–57.6%, CPU
44.4–51.1%. Raster work consumes 9.17 s across 18.10 s of motion, including
5.26 s of raster flushes. Main-thread `app.frame` calls have median 2.1 ms and
p99 4.2 ms; no full `bubbles()` queries occur in motion. Nested trace categories
are not additive and these CPU/service timings are not hardware GPU pass times.

An isolated promotion diagnostic reduces raster work to 76.4 ms over 6.05 s.
CDP observes five additional content layers, one per bubble; an attributable GPU
memory cost is not available. A matched instrumented default contact has 425 rAF
callbacks, 425 `app.frame` attempts and 1045 WebGPU queue submissions but only
52.64 actual presents/s. The promoted contact has 536 / 536 / 1316 respectively,
and 66.49 presents/s. Queue submissions are work batches, not completed frames.

A separate promoted, transparency-Off diagnostic reaches 89.582 actual
presents/s with p99 11.269 ms and 89.583 rAF callbacks/s. It does not qualify the
default appearance. It establishes a remaining glass-associated graphics cost
after raster pressure is removed; the retained trace cannot separate hardware
shader duration from compositor/driver waiting. No default 60 Hz rAF cap is
established. Earlier command-probe profiling and its port-internal guard A/B
remain in `artifacts/bubbles/mid/`.

Current raw histories, source/binary hashes, instrumented comparisons and
appearance captures are in `artifacts/bubbles/web-current/`. Large Chrome traces
are in `/mnt/localssd/capycanvas2-bubbles-perf/web-current/`. Supplemental
FrameTimeline still reports malformed Chrome child-slice timestamp drops; its
valid clock snapshots only align the independent SurfaceFlinger timestamps.
Its child-slice counts are not presentation evidence.

## Web Navigator resize diagnostic

The 2026-10-19 Chrome 155 diagnostic on Wacom MovinkPad 11 uses the 6000 × 4000
reference photo beneath one empty paint layer at Fit, default Low panel glass,
an isolated Navigator column, and thermal status 0 before and after motion.
The native viewport is 1257 × 691 CSS pixels at DPR 1.75. Three warmed five-second
mouse gestures produce 18.64–18.72 changing column bounds/s, with RAF interval
p99 67.1–83.8 ms. These are geometry observations, not presented FPS.

The frozen baseline Wasm SHA-256 is
`4dfb5f6e7fe90fb70f4f98d2596fddf841943eb398227e4c9025a5e81c942ade`.
Its compiler profile is not captured in the diagnostic. The dev-perf candidate
loaded during the functional checks has SHA-256
`938121939f4575295d040c7d52c2b81b082fa2c74683e7fa8566a7945c0dd4d2`;
its motion rate remains unmeasured. Raw evidence is in
`artifacts/navigator-layout-fix/movinkpad11/`.

SurfaceFlinger timestamps are retained, but their gesture alignment is invalid:
the collector mixed uptime including suspend with monotonic presentation time.
The panel-drag collector also required zero GPU frames while default glass was
enabled and stopped that series. Navigator camera throughput, aligned presented
FPS and the candidate comparison remain unqualified. The observed 60 Hz display
mode also prevents a 90 fps tier pass.

## Combined layer drawer

Measured 2026-10-10 on Wacom MovinkPad 11 with a release Rust benchmark APK,
the 6000 × 4000 reference photo beneath one empty paint layer at Fit zoom,
default panel glass and thermal status 0 before and after motion. The device's
active display mode remains 60 Hz; these measurements do not meet the 90 Hz tier.
Each theme uses a priming gesture and three five-second runs.

Native OnDraw geometry identifies changing page or row positions, matched to
unique FrameMetrics window-vsync timestamps. Completed Properties transitions
alternate native footer taps every 250 ms. Each run has 19 complete motions;
intervals extend from the first departure through the actual opposite endpoint,
including stalled frames inside the motion. Stationary gaps between completed
motions are excluded. The container and both page offsets share one progress.

| Native motion | Moving window frames/s, three runs | Maximum interval p99 |
| --- | --- | --- |
| Properties, light | 59.70–59.71 | 16.86 ms |
| Properties, dark | 59.71 | 16.86 ms |
| Left swipe, light | 58.90–59.30 | 33.50 ms |
| Left swipe, dark | 59.30–59.50 | 16.82 ms |

A separate rapid-reversal diagnostic taps every 150 ms during the 200 ms slide.
Over those full windows, actual changing geometry increased from
21.5–23.4 to 42.7–43.9 updates/s after replacing per-frame render-owner queries
with shared stateless interpolation and removing an initial idle frame. The
final full-window p99 is 66.98–83.74 ms, including pauses before a reversed
motion starts. This stress diagnostic does not establish steady motion rate or
input-to-presentation latency.

Source is `e9c1c5cdb` plus the drawer port; the benchmark APK SHA-256 is
`49e29f3095f18b24e29d1912e76d93c61a31a21fe5d3beb5644049b92edaf757`.
Exact Kotlin, JNI and shared Rust hashes are in
`artifacts/layer-drawer-ports/android/final-native-interpolation-build.json`.
Raw geometry and endpoint segment timestamps are retained with
`motion-summary-final-interpolation.json`. Earlier window rates and a fixed
200 ms temporal mask remain diagnostics; neither qualifies completed motion.
GTK workstation measurements are [recorded separately](top-tier.md#gtk-sketch-layers-diagnostic).

## Divider resize diagnostic

The Wacom MovinkPad 11 was measured on 2026-10-10 with the 108-pixel collapse
distance applied to `8e69d84fb`, a Debug APK and `dev-perf` Rust. The display
reported 60 Hz. `AndroidWorkspacePerformanceTest#continuousResizeFrameTiming`
uses a blank drawing, a custom panel workspace and default panel glass. After
warming up, one five-second mouse drag and one touch drag resized each of
Brushes, Properties, Navigator and the toolbar. Changing bounds reached
27.22–44.74 updates/s; distinct window-vsync rates were 31.14–57.02/s with p99
intervals of 33.50–83.78 ms. All eight windows published no full snapshots or
panel content changes and lost no frame reports. These diagnostics do not measure
collapse transitions or qualify the 90 Hz, 24 MP tier workload. Raw reports are in
`artifacts/panel-collapse-108/android-resize-logcat.txt`.

## Selected-pixel previews

The 2026-10-09 diagnostic uses Wacom MovinkPad 11 at thermal status 0 before
and after three warmed 5 s gestures per mode, a 6000 × 4000 selected fill,
and the optional standby shader-readiness gate. Source is `43427c897` plus
`paint_transform.rs` SHA-256 `310a381396804f1f058b81cd521e3f02815d0eb0c64bbd0cb53b2db4e7a5ded3`;
benchmark APK SHA-256 `0a345b3e0bf4b591e2e2c4730e8172d011ab18c233c4e921d3a83b7f1bf788e3`.
Handle submissions are 60.03, 192.35 and 192.68/s; Distort submissions are
101.67, 191.57 and 189.87/s. `gpu_completed_hz` also counts retained Navigator
updates. UI FrameMetrics reports only bar recompositions here (4.5–5.3/s), not
presented canvas frames. This scenario does not load the supplied reference
photo, and the panel reports 60 Hz. These observations establish no 90 Hz target;
the reference-photo and presented-frame measurements remain unqualified.
Raw results are under `artifacts/release/v1.0.12/android-checks/selection-run1..3/`.

## Layer thumbnail selection

Layer thumbnail selection uses the Wacom MovinkPad 11 at thermal status 0,
a benchmark APK with release Rust based on `8f73b600d` plus isolated Android
outline drawing, and the 6000 × 4000 reference photo beneath an empty paint layer
with a mask, at Fit zoom and default panel glass. After one second of priming,
three five-second runs switch content/mask every 150 ms in dark theme, without
screen recording. Unique `FrameMetrics` window-vsync rates were 59.705, 59.706
and 59.706/s, with p99 gaps of 16.906, 16.875 and 16.799 ms and zero dropped
reports. These measure window cadence, not actual screen presentation. The panel
ran at 60 Hz, so the 90 Hz tier remains unqualified. Raw reports and APK hashes
are in `artifacts/android-border-performance/final-mid/`.

Layer swipe measurements use the Wacom MovinkPad 11 at thermal status 0, the
6000 × 4000 reference photo beneath one empty paint layer, Fit zoom and default
panel glass. Each host ran a priming gesture and three five-second moving
gestures. Android uses the benchmark APK with release Rust and native touch
input; its frame timestamps come from `FrameMetrics`. Web uses release Rust,
Chrome 137, injected pen input through DevTools and Chrome's
`AnimationFrame::Presentation` timestamps. The display presents at 60 Hz.
Raw frame data, Chrome traces and fixture details are in
`artifacts/swipe-alpha-lock/`; Android's repeatable entry point is
`AndroidTitleBarTest#layerSwipeFrameTiming` ([layer gesture checks](../ui/drag-and-reorder.md#required-validation-when-implementing)).

The Android relationship overlay was measured again on 2026-10-04 at thermal
status 0 before and after motion. The 6000 × 4000 photo had one clipped empty
paint layer and two attached adjustments, Gaussian Blur and Curves, at Fit zoom
with default panel glass. Three five-second native touch gestures after priming
produced 59.0, 58.8 and 59.4 fps. Frame interval p99 was 16.90, 16.80 and 16.80 ms;
`FrameMetrics.TOTAL_DURATION` p99 was 31.37, 36.11 and 29.51 ms. These are native
window frame metrics, not a scanout latency measurement. The active display mode
was 60 Hz, so this result does not meet the 90 fps tier target.

The benchmark APK contains release Rust and unminified Kotlin from `00b2d6e73`
plus the Android relationship port. APK SHA-256 is
`a3a6ebf01e59cc6c7f89b7538fd0eb2d355188cd79657c45c006d18b54db0e22`;
the JNI library SHA-256 is recorded with exact compiled source hashes in
`artifacts/layer-relationship-android/benchmark-build-final.json` and
`build-final-source-end.json`. Raw records and the three-run summary are in
`artifacts/layer-relationship-android/layer-swipe-{1,2,3}.json` and
`motion-summary.json`. This updates the Android result; the Web result above
retains its earlier workload and measurement date.

## Selected swatch comparison

Measured 2026-10-03 on the XP-Pen Magic Note Pad (MNP1095), at its default
90 Hz and thermal status 0. The benchmark APKs share the same Rust library;
the candidate adds Android stacking from the shared front-swatch field. Both
builds start from `62b01a884`. APK and native-library hashes and raw records are
in `artifacts/android-color-overlap/provenance.json` and `motion-summary.json`.

Three warmed five-second gestures per device use the existing color-panel
fixture and its small default document:

| Input | Before, UI frames/s | After, UI frames/s |
| --- | --- | --- |
| Mouse | 88.62–89.01 | 88.97–89.05 |
| Touch | 88.83–89.08 | 88.79–89.05 |

Both builds have FrameMetrics vsync-interval p95 of 11.15–11.19 ms and publish
no retained workspace snapshots or panel contents during the valid runs. The
first cold baseline run failed that retention assertion with two snapshots;
its record is retained separately. This comparison uses a non-reference device,
a small document and p95 rather than p99, so it does not qualify the 24 MP target.

## Language-change diagnostic

Measured on the Wacom MovinkPad 11 on 2026-10-02 with `19a086a37`, release Rust
in the unminified Android benchmark APK. The 6000 × 4000 Perceptual photo has an
empty drawing layer above it and visible Paper below. Navigator is open, panel
glass uses its default and Fit zoom is 15.99%. G-Pen uses 1536 px and pressure 1.
Three warmed five-second strokes per scenario follow an ellipse with screen
semiaxes 310 × 150 px; the whole brush footprint stays at least 46.9 px inside
the canvas. The priming stroke is undone. Thermal status is zero before and
after the runs.

| Same-build workload | Median fresh GPU updates/s (range) | Completion-gap p99 across runs |
| --- | ---: | ---: |
| Steady language | 55.40 (47.00–57.40) | 30.98–51.27 ms |
| Language requested halfway through the stroke | 56.80 (52.40–56.99) | 32.08–34.97 ms |

Preparation takes 8.81–11.23 ms. Publication waits for pen-up; pen-up to observed
published model takes 222.51–303.51 ms. The harness resumes input within
0.014–0.025 ms of observing publication. Consumed input to the first resumed
GPU submission takes 14.06–19.30 ms, and to completion 57.00–65.15 ms.

The 90/s and 22.2 ms criteria are not met. Overlapping run ranges and the slower
first steady run establish no improvement or regression. This compares two
scenarios in the candidate, not a previous revision. Synthetic owner replay
does not qualify OS input, display scanout or a real pen. Visible Paper also
differs from the official brush fixture, which hides it. UI publication frame
cadence remains unmeasured.

APK SHA-256 is
`3f7fe53170f5a8282b2766bd5cc828c6a9c971148956b76561809abf777a7553`.
Raw records are under `artifacts/localization-live-switching/android/viewport-tier/`;
`viewport-tier-language-summary.json` in its parent directory records the
samples, geometry, layers and publication/resume intervals.

## Spatial composition comparison

Measured on 2026-10-02 UTC on the Wacom MovinkPad 11, Mali-G57 MC2, with
release Rust in the benchmark APK. The baseline is `f820bb89c`; the optimized
build is based on `864629ed8`. Its APK SHA-256 is
`bbce2e5cb432c2155cfec07eb85c21ea190696dfd7813b6f9a48897e9e7ffd5f`.
The 6000 × 4000 reference photo has an empty paint layer above it; Navigator
and the filter properties are open, Stats is closed, and panel glass uses its
default. Gaussian slider gestures have one priming drag and three warmed
five-second moving drags per range and camera. Thermal status is zero before
and after every run. The measured sigma ranges are 2.6–4.9 document pixels
(small) and 14.2–16.6 (large). The physical surface is 2200 × 1440, with a
1150 × 1272 work area. The 50% camera is translated by (-341, -207); Fit is 17.25%.

| Gaussian workload | Before fresh canvas updates/s, median (range) | After fresh canvas updates/s, median (range) | GPU composition p50 before → after |
| --- | ---: | ---: | ---: |
| 50%, small radius | 0.40 (0.40–0.40) | 7.17 (7.17–7.18) | 1796.89 → 116.20 ms; 15.46× |
| 50%, large radius | 0.20 (0.20–0.20) | 4.56 (4.56–4.59) | 4209.94 → 195.17 ms; 21.57× |
| Fit, small radius | 37.34 (29.31–38.27) | 38.89 (38.50–43.64) | Not traced |
| Fit, large radius | 25.08 (24.52–25.31) | 26.12 (25.32–26.12) | Not traced |

Canvas rates count changed raster frames queued and GPU-completed within the
motion window. They are not presentation rates. UI rates come from
`FrameMetrics`, which also include controls moving while the canvas waits.
The two 50% ranges show 58.7–59.3/s and 40.3–41.8/s UI frames after optimization;
the baseline shows 57.1–57.7/s and 42.2–42.8/s. The display holds 60 Hz, and
neither range meets the 90 fps target. These results do not establish a soft
target waiver. A separate traced drag supplies the GPU medians; observations
are matched to raster frame IDs queued during motion, including timing counters
published after release. Baseline GPU samples are sparse (two small, one large)
because its full-image fallback takes seconds per update.

At 50%, the output window is 2560 × 1792 texels. The input window includes the
sum of the chain's declared sampling radii; global effects retain whole-image
dependencies. A successful standalone calibration measured both Gaussian
passes at sigma 3, 8 and 21 on a 2048² RGBA32Float image, with 512² scissor
passes, four warmups and 30 retained samples per direction and radius.
Interpolating those per-pixel costs at half the native radius, adding three
copy-equivalent composition operations and 10–20 ms for overview/mip work,
predicts 103–113 ms for the small range and 206–216 ms for the large range.
Measured composition is within about 12% of the corresponding predicted
speedups. The coefficients were measured independently of the slider journey.
This is an approximate cost model, not a bound on GPU scheduling contention.

The retained-photo controls use three warmed five-second native-input gestures:

| Control | Before | After |
| --- | ---: | ---: |
| Pan, completed canvas updates/s, median | 59.94 | 60.05 |
| Pan, completion gap p99, range | 17.46–17.58 ms | 18.79–19.28 ms |
| G-Pen 1536 px, fresh/input updates/s, median | 55.28 | 54.78 |
| G-Pen 1536 px, input completion gap p99, range | 29.84–32.07 ms | 30.98–34.40 ms |

The G-Pen path is a contained 310 × 150 px ellipse at 15.99% Fit, with 200 Hz
stylus input and 16 ms prediction. These controls show no material throughput
regression; the brush remains below its 90 updates/s guarantee. Pan records GPU
completion rather than scanout, so it does not qualify the display-paced target.
The final Fit medians are slightly higher. No separate ROI speedup is expected
when the whole photo is visible.

Raw runs, traces, calibration, cost model and validation logs are retained in
`artifacts/bounded-spatial-effects-results/`. The shared pixel oracle verifies
chained filters, masks, clipping, offscreen damage, both blend spaces and exact
refinement. The actual tablet viewport admits the 24 MP single-filter window
under the 608 MiB composition-cache limit; total renderer storage includes
additional source and paint allocations and is not bounded by that cache limit.

## Authored-model G-Pen comparison

Measured on 2026-10-04 UTC with clean `192601dac` (BUILD29) and the authored-model
candidate on that revision (BUILD32), candidate then baseline. Both benchmark
APKs use release Rust, the 6000 × 4000 tier photo, Linear blending, Navigator
open, Stats closed and default glass. The default solid fill is hidden. A priming
stroke is undone before three warmed ten-second strokes with 200 Hz injected
stylus input, pressure 1 and 16 ms prediction. Thermal status is zero before and
after. Fit is 15.9900%; the 310 × 150 px ellipse keeps the full 1536 px tip at
least 46.90 px inside the photo.

Rates count GPU-completed nonempty updates consuming fresh paint input inside
the contact. Response ends at completion-callback service, not physical pen
latency or presentation. No samples are discarded.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 59.308 | 25.827 | 55.093 | 10.466 |
| Baseline 2 | 59.531 | 25.705 | 55.321 | 10.413 |
| Baseline 3 | 59.779 | 25.275 | 57.357 | 10.453 |
| Candidate 1 | 59.668 | 27.371 | 55.247 | 10.690 |
| Candidate 2 | 59.181 | 27.355 | 56.785 | 10.464 |
| Candidate 3 | 59.116 | 26.389 | 58.013 | 10.559 |

Both builds miss 90 fresh updates/s and the 22.2 ms gap limit. Throughput,
owner CPU p95 and callback wall p95 comparisons remain within 5%. Response p99
changes by +0.154 / +1.464 / +0.656 ms: the second stroke exceeds +1 ms, so
that common bound remains unresolved. Accounted renderer residency is identical
at 1466.523 MiB in every contact; each build retains 384 decoded source slots
before painting. Dynamic admission budgets differ and are recorded separately.

### Drawing in the middle of sixteen photo layers

The matched 1024 px workload has sixteen photos plus paint: seventeen visible
layers, with eight photos above and eight below the paint layer. Photo copies
have 35% opacity over the opaque base. Other settings match the guarantee run;
the nominal full-tip margin is 87.83 px.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 73.771 | 19.981 | 42.142 | 10.568 |
| Baseline 2 | 75.047 | 18.207 | 40.515 | 10.714 |
| Baseline 3 | 75.068 | 20.832 | 47.270 | 10.512 |
| Candidate 1 | 74.733 | 18.784 | 40.246 | 10.273 |
| Candidate 2 | 74.790 | 18.871 | 41.009 | 10.726 |
| Candidate 3 | 75.449 | 18.465 | 39.904 | 10.371 |

Both builds miss 90 updates/s. Every pair meets the measured common throughput,
owner CPU and callback-wall p95 bounds; response p99 changes by
−1.896 / +0.494 / −7.366 ms. Accounted renderer residency is identical at
1709.094 MiB. These results supersede the older BUILD20 middle-layer comparison
for this workload; they do not clear the absolute rate miss or the guarantee
workload's response excess.

Residency boundaries meet the additional max(16 MiB, 5%) ceiling. Settled
allocator/RSS/high-water observations do not establish continuous memory peaks
or edit/undo/output lifetime. GPU execution p95 and presentation remain
unmeasured. Raw samples and analysis are under
`artifacts/format/m3-final-android-performance-38/movinkpad11/`; APK/source
provenance is under `artifacts/format/m3-baseline192-android-build-29/` and
`artifacts/format/m3-candidate192-android-build-32/`.
Baseline app SHA-256 is
`e7ddce0b37f254df4fcdee4c502bc2220c5a2f050e644060bc4a47a06067e429`;
candidate is `acccdc9d9464c2ca2db5fc3291dc252b7affe683ca9cadb7628ffef58863a51c`.
Older BUILD20 samples remain under `artifacts/format/m3-final-android-performance-20/`.

### Composition CPU trace

A separate matched sixteen-photo trace uses the same BUILD29/32 binaries and
three ten-second contacts. Full CPU traces contain no buffer overwrite or
kernel overruns. Matching raw begin events to scheduling threads avoids the
existing report's incorrect zero owner-CPU result. Candidate composition CPU
totals decrease by 383.051 ms across all contacts; per phase they decrease
4.8841 → 4.7554 ms (2.64%). Total callback CPU decreases 1.48%; independent
Android thread CPU decreases 1.69%. Publication adds 53.953 ms, largely inside
nested command-finish work, which must not be added twice.

The bounded GPU timing queue retains only 764/2052 baseline and 763/2024
candidate frame observations. Missing timings are not zero-cost frames. This
trace supports CPU attribution, not full GPU p95 or untraced tier qualification.
Exact joins, missing IDs and per-contact results are under
`artifacts/format/m3-mid-stack-trace-37/trace-diagnosis/`.

### Complete GPU timing diagnostic

A separate BUILD29/32 comparison uses six three-second contacts in the same
sixteen-photo middle-layer workload. Short contacts fit the existing GPU timing
queue: all 1,157 baseline and 1,162 candidate selected submissions have elapsed,
paint, prediction and composition timings. They do not satisfy the sustained
stroke duration required for tier qualification.

| Contact | GPU elapsed p95, ms, baseline → candidate | GPU elapsed p99, ms, baseline → candidate |
| --- | ---: | ---: |
| 1 | 17.903 → 17.743 | 23.696 → 22.720 |
| 2 | 18.677 → 17.905 | 25.848 → 22.846 |
| 3 | 17.955 → 17.847 | 26.066 → 21.994 |
| 4 | 18.418 → 17.581 | 25.683 → 22.169 |
| 5 | 18.038 → 17.934 | 25.839 → 18.950 |
| 6 | 17.744 → 18.401 | 26.269 → 26.895 |

Elapsed p95 improves in five contacts and rises 3.70% in the sixth. Paint p95
rises in five contacts, by at most 0.207 ms (4.04%). Individual phase tails
remain: composition p99 rises 1.346 ms in contact 1, and paint p99 rises
1.439 ms in contact 6. No contacts or tails are pooled away.

Settings and retained source residency match. Dynamic budgets differ; the
candidate starts with the GPU 1.30°C warmer, and per-frame dab counts vary.
Both runs report zero thermal and cooling status. Only baseline-first order
was measured. Contact 6 input-to-completion p99 rises 5.627 ms; different
input ages, GPU spans and completion service all contribute to the selected
observations, without establishing a causal matched-frame regression.
This diagnostic fills the timing-coverage gap for these short contacts; it
does not clear the sustained response failures, presentation or 90/s target.
Raw traces, complete ID joins, per-phase quantiles and limitations are in
`artifacts/format/m3-mid-gpu-trace-49/`.

## Layer color modes

Measured on 2026-10-04 at `de1159228`, release benchmark APK SHA-256
`fc0de7ee32ebbb9947803e461ca2bb436e07915019c2f82543cafcce8e5ea645`.
App sources match the implementation at `c92420837`.
The tier photo sits below one empty paint layer at Fit, with Perceptual blending,
pressure 1, 16 ms prediction, default display settings and panel glass, Stats
closed and thermal status 0 before and after each workload. Each mode uses one
priming gesture undone, followed by three five-second 200 Hz stylus ellipses.
The ellipse radii are 310 × 150 surface pixels at 15.99% Fit.

| Brush | Color mode | Canvas updates/s, median (range) | Fresh input updates/s, median (range) | Fresh completion-gap p99, range | Target |
| --- | --- | --- | --- | --- | --- |
| G-Pen 1536 px | Full color | 53.46 (53.36–53.51) | 53.46 (53.36–53.51) | 30.18–31.25 ms | **Not met** |
| G-Pen 1536 px | Grayscale | 53.39 (53.25–53.82) | 53.39 (53.25–53.82) | 29.86–30.44 ms | **Not met** |
| G-Pen 1536 px | Two-tone | 53.60 (53.15–53.72) | 53.60 (53.15–53.72) | 29.01–30.22 ms | **Not met** |
| Paintbrush 1024 px | Full color | 24.74 (24.73–24.93) | 24.74 (24.73–24.93) | 82.59–91.12 ms | **Not met** |
| Paintbrush 1024 px | Grayscale | 24.54 (24.33–24.73) | 24.54 (24.33–24.73) | 84.41–85.88 ms | **Not met** |
| Paintbrush 1024 px | Two-tone | 24.35 (24.35–24.37) | 24.35 (24.35–24.37) | 83.80–88.76 ms | **Not met** |
| Watercolor Wash 512 px | Full color | 0.60 (0.60–0.60) | 0.60 (0.60–0.60) | 1536.52–1726.83 ms | **Not met** |
| Watercolor Wash 512 px | Grayscale | 0.60 (0.60–0.60) | 0.60 (0.60–0.60) | 1587.10–1645.00 ms | **Not met** |
| Watercolor Wash 512 px | Two-tone | 0.60 (0.60–0.80) | 0.60 (0.60–0.80) | 1552.36–2601.22 ms | **Not met** |

These workloads miss the tier rate. Other brushes, layer-mode conversion latency,
and physical input-to-present latency remain unqualified. Raw runs, screenshots
and environment records are under `artifacts/layer-modes/mid-rebased/`;
the binary and source snapshot are under `artifacts/layer-modes/rebased-build/`.

## Brushes

Target: **90 completed updates/s** at the guaranteed size, on the 24 MP canvas.
Simple brushes are guaranteed through **1536 px**; complex and very complex
brushes retain their 1024 px and 512 px guarantees.

### JNI cleanup comparison

Measured on 2026-09-30 on the MovinkPad 11: 6000 × 4000, G-Pen 1024 px at Fit,
default workspace with Navigator, prediction enabled and OS-injected 240 Hz input.
Each batch contains three warmed five-second strokes; batches alternate builds.
Baseline source is `c993253ca`, APK SHA-256
`a59fa1c30bf260f8edec0bbf0354bfd342d55f9ba2eaab6a55e0ee717342072b`.
The JNI cleanup builds on `829f1e223`, APK SHA-256
`3a05ab028cc7bafb409313ad23741c403b643c277dfc235a4f2fb30194599e24`.

| Metric | Baseline | JNI cleanup | Repeated baseline | Repeated cleanup |
| --- | ---: | ---: | ---: | ---: |
| Median fresh completed canvas updates/s | 166.1 | 164.4 | 166.2 | 165.7 |
| Median per-run p99 fresh completion gap | 14.92 ms | 14.98 ms | 14.74 ms | 14.71 ms |
| Input to GPU completion p99 | 28.01 ms | 27.97 ms | 27.82 ms | 27.48 ms |
| Owner CPU p99 | 6.20 ms | 6.01 ms | 6.08 ms | 5.95 ms |

Individual rates overlap: baseline 163.2–169.6 and cleanup 164.1–170.0 updates/s.
This empty-canvas 1024 px workload meets the 90 updates/s and 22.2 ms gap criteria;
the 1536 px guarantee and display-paced motion need their own measurements.

### Shared geometry comparison on Huion

Measured on 2026-09-30 on Huion KP1202: 4248 × 2832, 512 px watercolor brushes,
Fit, 100 × 70 px ellipse, 16 ms prediction, Stats closed and 200 Hz stylus input.
Each alternating batch contains three warmed ten-second strokes. Baseline
source is `5b514bbd`; benchmark APK SHA-256
`f14abb270b0e3a654de9a22cacb6b8f568def358ecab7b30809cdded21fe2944`.
The shared geometry/comment cleanup APK SHA-256 is
`84a27bef3875899f1eea4ad38e894489e83ba7d20d74d48b39d342fe249e077a`.
Both APKs have identical Java code and startup profiles.

| Metric | Before | After | Repeated before | Repeated after |
| --- | ---: | ---: | ---: | ---: |
| Watercolor Wash, median fresh updates/s | 3.80 | 3.69 | 3.60 | 3.80 |
| Watercolor Wash, median completion gap p99 | 461.3 ms | 471.2 ms | 470.3 ms | 433.5 ms |
| Wet Watercolor, median fresh updates/s | 4.19 | 3.99 | 4.09 | 3.99 |
| Wet Watercolor, median completion gap p99 | 407.0 ms | 482.2 ms | 451.9 ms | 459.3 ms |

Run ranges overlap; the samples do not establish a throughput change. This
12 MP comparison on Huion does not qualify the 24 MP reference-tier targets.
Reports: `artifacts/simplification-cleanup/performance/m28/`.

### Dry-media contact model

Measured on 2026-10-17 UTC with dry kernels specialized to normal blending and
zoomed-out dry relief previews evaluated at display resolution, applied to the
control `c674ee521`. The builds alternate for two passes each with the contained
45° profile described below; the Smoothing 100% row sets Curve fit to its
maximum, which lengthens the predicted tail. Optimized APK SHA-256
`1c8474b097ad06cff2f84aff7eee75be6025f09a739187344eddfc823c097a1f`, control
`4159a2cb7a267538ee3a0ac46d5bc8830686e04e70c07e69629e7b12a755f3ed`. Thermal
status was zero in every record. Ranges cover all six contacts; gaps are the
worst per-contact p99 between fresh completions.

| Brush and Size | Before completed updates/s | After completed updates/s | After fresh input updates/s | After worst fresh p99 gap | After input-to-GPU p99 | 90/s, 22.2 ms fresh profile |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 1536 px | 41.98–42.71 | 123.82–124.82 | 123.49–124.82 | 18.67 ms | 35.88–38.56 ms | Meets |
| Pointy Pencil 1536 px | 48.34–49.42 | 136.17–137.53 | 136.17–137.20 | 15.57 ms | 31.84–33.59 ms | Meets |
| Charcoal 1536 px | 41.26–42.05 | 123.05–124.06 | 122.88–123.90 | 18.58 ms | 36.78–41.75 ms | Meets |
| Pastel Block 1024 px | 62.79–63.35 | 161.82–164.46 | 160.32–162.29 | 13.12 ms | 27.89–38.39 ms | Meets |
| Pencil 1536 px, Smoothing 100% | 35.09–35.93 | 109.49–114.83 | 109.32–114.67 | 18.18 ms | 37.21–48.83 ms | Meets |

First selection after clearing app data (`--probe-presets 2,25,6`, three runs)
takes Pencil 1.22 → 0.89 s, Pointy Pencil 0.94 → 0.62 s and Chalk 0.72 → 0.95 s;
Chalk's background compile order varies between runs. Raw runs are under
`artifacts/dry-preview-speed/movinkpad11/`. The earlier 2026-10-17 rows below
measure the previous build.

Measured on 2026-10-17 UTC with the one-kernel dry evaluator, two fine-tip
nodes from 4 px minor radius, Curve fit vertices bounded by 1% of the contact
radius and five-point Gauss–Legendre for short segments, applied to the control
`1300ba9f2`. The builds alternate for
two passes each with the profile described below, and ranges cover all six
contacts. Optimized APK SHA-256
`c8db6e1f2a909dea4f5d2b1266066d5c59bc6eb99c1ec9785adb4c24685c56c9`, control
`3a7340bdd1fdf0fc9d78f50c5cca810bd5c9226d9d066e6edb1abe5263439b4d`. Thermal
status was zero at every install. Gaps are the worst per-contact p99 between
fresh completions.

| Brush and Size | Before completed updates/s | After completed updates/s | After fresh input updates/s | After worst fresh p99 gap | After input-to-GPU p99 | 90/s, 22.2 ms fresh profile |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 1536 px | 25.59–26.31 | 38.27–39.08 | 38.27–39.08 | 35.13 ms | 88.79–89.83 ms | **Not met** |
| Pointy Pencil 1536 px | 33.74–34.30 | 43.24–43.93 | 43.08–43.93 | 30.66 ms | 77.57–79.35 ms | **Not met** |
| Charcoal 1536 px | 24.60–24.97 | 37.92–38.29 | 37.92–38.29 | 36.18 ms | 89.85–92.88 ms | **Not met** |
| Pastel Block 1024 px | 52.24–53.12 | 59.75–60.51 | 59.75–60.51 | 25.42 ms | 64.40–66.71 ms | **Not met** |

Selecting a brush after clearing app data (`--probe-presets 2,25,6`, three runs)
takes Pencil 4.31 → 1.22 s, Pointy Pencil 3.93 → 0.95 s and Chalk 1.98 → 0.79 s;
a 36 px size change then takes 1.69 s → 82 ms. Raw runs are under
`artifacts/dry-kernel-merge/movinkpad11/`, beside the summary script and the
benchmark wrapper that passes tilt and opacity. The 2026-10-13 rows below
measure the earlier build.

Measured on 2026-10-13 (UTC), with the final dry defaults, unified contact loop
and compile-time aggregate specialization based on `cbc74b304`.
Frozen source SHA-256 `04adb5a414d5fee9148980b56487e9d211e1dcb7b497185ee8faab955f287eda`;
optimized APK `66f517b7c867a83caee591780226ff36591bf246bb3b3fd6ba04ccd7239d6020`,
JNI `7de0e1d01ae832bcb4bd91b0ed7157052f24d3668439edbdc299c50d54c7e49b`.
The production portions of the three renderer files restored for aggregate
specialization match production; later differences are test assertions and trailing
WGSL whitespace. The matched dynamic-aggregate
comparison uses APK `a144b7dc328d9861481901cb650977670bf9f02c7737ca5689298ed06165b36a`;
it has the same contact equations, current defaults and Curve fit input code.

Both use the tier photo at Fit, one empty Full color paint layer above it,
Navigator open, Stats closed and 16 ms prediction. Each warmed profile has three
six-second 200 Hz stylus contacts at physical pressure one, 45° tilt and a
contained 60 × 10 surface-pixel ellipse. All before/after thermal records are zero.
Size is the full-pressure long axis at the writing pose. Strength, Opacity and
Texture strength are 100%; Material hardness is Pencil 90%, Pointy 95%, Charcoal
55% and Pastel 50%. Smoothing retains its default 6%. Pressure balance is absent.
The calibrated Pencil and Pointy pressure gains are 0.7.

| Brush and Size | Dynamic aggregate fresh updates/s | Current completed canvas updates/s | Current fresh input updates/s | Current worst fresh p99 gap | Current input-to-GPU p99 | 90/s, 22.2 ms fresh profile |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 1536 px | 20.79–21.60 | 26.28–26.29 | 26.28–26.29 | 52.90 ms | 133.43–133.95 ms | **Not met** |
| Pointy Pencil 1536 px | 25.59–25.92 | 33.74–34.22 | 33.74–34.22 | 38.41 ms | 102.76–104.51 ms | **Not met** |
| Charcoal 1536 px | 20.12–20.30 | 24.93–25.31 | 24.93–25.31 | 55.03 ms | 139.94–141.02 ms | **Not met** |
| Pastel Block 1024 px | 44.03–44.41 | 51.93–52.17 | 51.93–52.17 | 24.83 ms | 67.19–70.18 ms | **Not met** |

Fresh input throughput counts the first GPU completion consuming a new input;
completed canvas throughput also includes prediction/refinement completions.
Input-to-GPU observations include completion polling and do not measure physical
scanout. Compile-time aggregate specialization improves these matched profiles,
but all four still miss rate and gap.
Earlier `611578d3` and `fcb19ecf` results use different defaults and remain
historical comparisons, not qualification of this build.

A separate same-APK Pencil 1536 px diagnostic changes only native Smoothing
to 0% before three measured contacts. Every post-contact state verifies zero;
the default 6% setup snapshot remains preserved. It completes 50.91–51.63 fresh
updates/s with worst fresh gap 29.53 ms and input-to-GPU p99
73.36–76.32 ms. This isolates a substantial Curve fit cost but
still misses the nominal rate and gap; the qualification rows keep 6%.

Brief visual-only replays on the dynamic-aggregate comparison build cover
Shading Pencil Size 24.615 px, Chalk 19.870 px and Crayon 12.308 px at 45°,
Strength 100% and actual physical pressure 0.1–1.0. Each has one two-second
contact and a retained screenshot; these do not qualify nominal tier sizes or
sustained frame rates. Other tilts and partial Opacity remain unmeasured.
Raw current profiles, matched controls, diagnostic captures and source/binary
proof are under `artifacts/dry-defaults-final/midtop/`.

### Retained-photo brush measurements

Except for G-Pen and the current dry-media rows, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets. The ellipse is 520 × 299 px at 16.0% zoom.

Multiply Glaze was measured on 2026-10-19 with the same profile, using a release benchmark build of `66c1142ee`, which runs every material brush as compute kernels; its gap is the completion-gap p99. The measured very complex rows were recorded on 2026-10-20 with the same profile, using a release benchmark build of the blender suite on `13bec703f`. The Smudge, Natural Blender, Soft, Blur, Bristle, Grainy, Blending Knife, Blending Stump and Tissue rows were measured again on 2026-10-20 at `68012f20a`, after [paper-aware blending](../history/brushes/paper-aware-blenders.md): six runs from two candidate sets interleaved with a control of the previous `main` (three runs for the new presets). The control completed Smudge 8.3, Natural Blender 1.9, Soft 7.5, Blur 9.1, Bristle 9.0, Grainy 4.9 and Blending Knife 1.1 updates/s. Every one of them is past the tier's GPU capacity at this size, so the backlog grows through each stroke. Comb and Sharpen skip most of their work on the empty layer, so painted layers cost them more. The Blending Knife's and Natural Blender's elongated tips are four and 1.6 times a round tip's area at 512 px.

Marker, Paintbrush, Textured Flat, Dry Scumble and Transparent Glaze were measured on 2026-10-21 after Uniform contacts stopped counting the pose they share twice ([paint state](../reference/painterly-paint-state.md)): six strokes from two candidate sets interleaved with a control of the previous `main` (`c12ec63d2`), which completed Marker 18.32, Paintbrush 14.78, Textured Flat 16.13, Dry Scumble 3.80 and Transparent Glaze 5.49 updates/s. The control's strokes sped up after a lap or two, once the doubled coats brought the ellipse to full coverage and its pixels skipped their contacts. Each pass now lays one coat, so a pixel the ellipse crosses again costs as much as on its first pass.

Liquify uses the separate Photo-layer profile below. Its measurements deform
the photo itself; the reference tablet's 90 Hz mode was verified before the runs.

The older 2048 px simple-brush rows are above the 1536 px guarantee and do not
classify performance at the guaranteed size. G-Pen, Pencil, Pointy Pencil and Charcoal have current 1536 px measurements; the remaining
simple brushes retain unqualified rows.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1536 px | 31.10 (30.89–31.12) fresh input-attributed updates/s; completion-gap p99 43.04–46.58 ms | **Not met**; [current controls](#brush-and-pending-filter-controls) |
| G-Pen (1), brush controls | Simple | 1536 px | 75.36–75.54 fresh updates/s; fresh gap p99 ≤20.13 ms, contained 60 × 10 path | **Rate not met**; [earlier controls](#dry-media-contact-model) |
| Rough G-Pen (28) | Simple | 2048 px | 12.2 updates/s (12.0–12.2); gap p99 152.2 ms | 1536 px unmeasured |
| Calligraphy Pen (29) | Simple | 2048 px | 35.2 updates/s (35.1–35.3); gap p99 73.2 ms | 1536 px unmeasured |
| Antique Pen (30) | Simple | 2048 px | 16.2 updates/s (16.0–16.3); gap p99 171.2 ms | 1536 px unmeasured |
| Realistic Pen (31) | Simple | 2048 px | 13.4 updates/s (13.4–13.5); gap p99 131.6 ms | 1536 px unmeasured |
| Wet Ink (32) | Simple | 2048 px | 11.0 updates/s (10.9–11.1); gap p99 168.6 ms | 1536 px unmeasured |
| Pencil (2) | Simple | 1536 px | 26.28–26.29 fresh updates/s; fresh gap p99 ≤52.90 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Pointy Pencil (25) | Simple | 1536 px | 33.74–34.22 fresh updates/s; fresh gap p99 ≤38.41 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Shading Pencil (26) | Simple | 1536 px | Current contact model unmeasured | Unqualified; [dry media](#dry-media-contact-model) |
| Charcoal (27) | Simple | 1536 px | 24.93–25.31 fresh updates/s; fresh gap p99 ≤55.03 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Chalk (6) | Simple | 1536 px | Current contact model unmeasured | Unqualified; [dry media](#dry-media-contact-model) |
| Crayon (43) | Simple | 1536 px | Current contact model unmeasured | Unqualified; [dry media](#dry-media-contact-model) |
| Eraser (3) | Simple | 2048 px | 7.1 updates/s (7.0–7.2); gap p99 174.5 ms | 1536 px unmeasured |
| Airbrush (5) | Simple | 2048 px | 9.7 updates/s (9.7–9.9); gap p99 135.9 ms | 1536 px unmeasured |
| Marker (7) | Complex | 1024 px | 8.74 updates/s (8.58–9.09); gap p99 297.8 ms | **Not met** |
| Blotty Ink (33) | Complex | 1024 px | 20.1 updates/s (19.9–20.2); gap p99 90.2 ms | **Not met** |
| Realistic Brushed Ink (34) | Complex | 1024 px | 19.6 updates/s (19.5–19.6); gap p99 136.3 ms | **Not met** |
| Pastel Block (17) | Complex | 1024 px | 51.93–52.17 fresh updates/s; fresh gap p99 ≤24.83 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Paintbrush (4) | Complex | 1024 px | 5.19 updates/s (5.09–5.29); completion-gap p99 380.7 ms | **Not met**; [layer color modes](#layer-color-modes) |
| Textured Flat (15) | Complex | 1024 px | 5.99 updates/s (5.60–6.19); gap p99 524.3 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 3.44 updates/s (3.39–3.59); gap p99 537.1 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 4.94 updates/s (4.79–5.09); gap p99 523.8 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 12.1 updates/s (11.9–12.3); input 12.1/s; completion-gap p99 112.0 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 6.2 updates/s (6.2–6.2); gap p99 201.8 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 5.9 updates/s (5.5–6.5); gap p99 266.9 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | 1.9 updates/s (1.9–1.9); input 2.1/s; completion-gap p99 978.2 ms | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | 1.7 updates/s (1.7–1.7); input 1.8/s; completion-gap p99 736.2 ms | **Not met** |
| Wet Watercolor (21) | Very complex | 512 px | 1.6 updates/s (1.6–1.6); input 1.7/s; completion-gap p99 768.4 ms | **Not met** |
| Loaded Oil (22) | Very complex | 512 px | 1.8 updates/s (1.8–1.8); input 2.0/s; completion-gap p99 1050.0 ms | **Not met** |
| Palette Knife (23) | Very complex | 512 px | 1.6 updates/s (1.6–1.6); input 1.7/s; completion-gap p99 1262.1 ms | **Not met** |
| Wet Round (11) | Very complex | 512 px | 2.6 updates/s (2.5–2.6); input 2.7/s; completion-gap p99 466.7 ms | **Not met** |
| Natural Blender (24) | Very complex | 512 px | 1.9 updates/s (1.9–2.0); input 1.9/s; completion-gap p99 618.8 ms | **Not met** |
| Smudge (10) | Very complex | 512 px | 8.5 updates/s (8.3–8.7); input 8.5/s; completion-gap p99 139.3 ms | **Not met** |
| Soft Blender (45) | Very complex | 512 px | 7.6 updates/s (7.5–8.2); input 7.6/s; completion-gap p99 167.2 ms | **Not met** |
| Blur (46) | Very complex | 512 px | 10.3 updates/s (10.3–10.4); input 10.3/s; completion-gap p99 120.6 ms | **Not met** |
| Sharpen (60) | Very complex | 512 px | 31.5 updates/s (31.2–31.6); input 31.6/s; completion-gap p99 45.2 ms | **Not met** |
| Blending Knife (56) | Very complex | 512 px | 1.2 updates/s (1.1–1.2); input 1.2/s; completion-gap p99 1461.9 ms | **Not met** |
| Blending Stump (65) | Very complex | 512 px | 8.9 updates/s (8.9–9.0); input 8.9/s; completion-gap p99 143.5 ms | **Not met** |
| Tissue (66) | Very complex | 512 px | 7.3 updates/s (7.2–7.3); input 7.3/s; completion-gap p99 171.9 ms | **Not met** |
| Grainy Blender (48) | Very complex | 512 px | 5.6 updates/s (5.5–5.8); input 5.6/s; completion-gap p99 222.5 ms | **Not met** |
| Texture-Keeping Blender (61) | Very complex | 512 px | 2.6 updates/s (2.5–2.6); input 2.8/s; completion-gap p99 596.4 ms | **Not met** |
| Hue Blender (62) | Very complex | 512 px | 9.5 updates/s (9.3–10.2); input 9.7/s; completion-gap p99 129.8 ms | **Not met** |
| Wet Blender (57) | Very complex | 512 px | 3.1 updates/s (3.1–3.1); input 3.3/s; completion-gap p99 379.0 ms | **Not met** |
| Color Mixer (58) | Very complex | 512 px | 2.9 updates/s (2.9–2.9); input 3.1/s; completion-gap p99 396.4 ms | **Not met** |
| Mixing Brush (59) | Very complex | 512 px | 2.4 updates/s (2.4–2.5); input 2.6/s; completion-gap p99 459.5 ms | **Not met** |
| Bristle Blender (47) | Very complex | 512 px | 12.1 updates/s (11.8–12.4); input 12.1/s; completion-gap p99 106.7 ms | **Not met** |
| Painterly Blender (50) | Very complex | 512 px | 1.7 updates/s (1.6–1.7); input 1.9/s; completion-gap p99 854.7 ms | **Not met** |
| Crystal Blender (51) | Very complex | 512 px | 8.4 updates/s (8.3–8.5); input 8.6/s; completion-gap p99 146.9 ms | **Not met** |
| Marbling (52) | Very complex | 512 px | 3.3 updates/s (3.2–3.4); input 3.5/s; completion-gap p99 405.4 ms | **Not met** |
| Comb Blender (53) | Very complex | 512 px | 13.8 updates/s (13.5–14.2); input 14.0/s; completion-gap p99 91.8 ms | **Not met** |
| Scatter Blender (49) | Very complex | 512 px | 25.5 updates/s (25.3–25.7); input 25.7/s; completion-gap p99 54.7 ms | **Not met** |
| Dither Blender (55) | Very complex | 512 px | 81.33 updates/s (79.98–82.14); fresh input 81.33/s; worst completion-gap p99 30.76 ms | **Not met**; [brush contact comparison](#dither-pickup-within-the-brush) |
| Liquify Push (12) | Very complex | 512 px | 45.89 fresh updates/s (45.47–46.07); fresh completion-gap p99 ≤44.84 ms | **Not met** |
| Liquify Twirl Clockwise (36) | Very complex | 512 px | 31.52 fresh updates/s (31.34–31.96); fresh completion-gap p99 ≤83.66 ms | **Not met** |
| Liquify Twirl Counterclockwise (13) | Very complex | 512 px | 31.12 fresh updates/s (30.55–31.15); fresh completion-gap p99 ≤87.86 ms | **Not met** |
| Liquify Pinch (37) | Very complex | 512 px | 53.85 fresh updates/s (45.30–54.14); fresh completion-gap p99 ≤67.69 ms | **Not met** |
| Liquify Expand (38) | Very complex | 512 px | 55.95 fresh updates/s (55.69–56.71); fresh completion-gap p99 ≤47.49 ms | **Not met** |
| Liquify Crystals (39) | Very complex | 512 px | 33.94 fresh updates/s (33.92–34.10); fresh completion-gap p99 ≤56.98 ms | **Not met** |
| Liquify Edge (63) | Very complex | 512 px | 45.08 fresh updates/s (43.46–46.76); fresh completion-gap p99 ≤71.68 ms | **Not met** |
| Liquify Reconstruct (64) | Very complex | 512 px | 55.70 fresh updates/s (53.27–56.83); fresh completion-gap p99 ≤57.81 ms | **Not met** |
| Clone Stamp (40) | Very complex | 512 px | 115.7 fresh updates/s; completion gap p99 15.2 ms | **Met for drawing** |
| Healing Brush (41) | Very complex | 512 px | 113.7 fresh updates/s; completion gap p99 15.2 ms | **Met for drawing** |
| Spot Healing Brush (42) | Very complex | 512 px | 119.7 fresh updates/s; completion gap p99 13.6 ms | **Met for drawing** |

Retouching rows use the integrated-compositor measurements below, copying from the photo marked as a reference layer.

### Dither pickup within the brush

Measured on 2026-10-22 on the reference Wacom MovinkPad 11, Mali-G57 MC2,
thermal status 0, using optimized release benchmark APKs. The baseline is
`bc25c671c`, which preserves source colors but permits pickup outside the tip.
The new implementation accumulates each donor's actual contact exposure and
caches its immutable palette order. Dither is 512 px at pressure 1, Fit zoom
0.2964375, default workspace and panel glass, Stats off and 16 ms prediction.
The 310 × 150 screen-pixel ellipse differs from the older blender profile above.
Each workload has three warmed 10 s strokes; the empty layer has three 5 s
strokes. Fresh-input and completed-update rates agree. Gaps below are the worst
p99 across the three strokes.

| Paint being blended, 6000 × 4000 canvas | Baseline → brush contact, completed updates/s | Completion-gap p99, baseline → brush contact |
| --- | --- | --- |
| Tier photo itself | 13.59 / 14.18 → 21.17 | 114.66 / 99.73 → 78.26 ms |
| Empty paint above the tier photo | 30.55 → 81.33 | 53.53 → 30.76 ms |
| Black 512 px G-Pen stroke against transparency | 27.96 → 63.42 | 52.96 → 30.64 ms |
| Three solid G-Pen color bands and a thin fourth-color mark | 29.96 → 63.14 | 49.50 → 30.41 ms |

Photo uses a baseline/candidate/baseline bracket: baseline stroke rates range
13.57–13.68 and 13.48–14.33 updates/s; candidate rates range 21.08–21.18.
The candidate median improves by 52% against the mean of the two baseline
medians. The other fixtures improve by 111–166%. The authored fixtures contain
one Ink layer and hidden Paper; Photo uses the normal two-layer document.
All four workloads still miss the 90 updates/s and 22.2 ms p99 targets.
These are completed canvas updates, not scanout or physical pen latency.

The first contact on the empty layer has a next-GPU-completion diagnostic of
143.64–146.47 ms before and 139.89–149.43 ms after. A fresh-process diagnostic
of three 500 ms contacts separated by 2.5 s gives a median next-GPU completion
of 27.81 / 36.05 ms in the baseline brackets and 32.57 ms in the candidate;
the candidate median lies within observed baseline drift. Individual candidate
contacts range 24.95–40.86 ms. These diagnostics do not measure physical pen
latency.

Each touched 256² page needs 4 MiB for its two exposure/palette surfaces,
or 6 MiB with explicit post-stroke edges; this scratch retires after correction
and replay end. Mali-G57 offline compilation reports zero register spills,
zero stack use and zero local allocation.

A matched 30 s Photo stroke and 10 s idle capture changes peak tracked GPU
allocation from 1470.66 to 1686.96 MiB (+216.30 MiB), reserved GPU memory from
1494.40 to 1702.40 MiB and process PSS from 1965.83 to 2160.53 MiB. After idle,
allocated GPU memory is 1167.49 → 1166.96 MiB and PSS is 1670.94 → 1612.01 MiB;
the added state retires. Minimum system memory available is
3210.62 → 3226.48 MiB during these captures.

Baseline APK SHA-256: `e03abb7d3a5292ed901598fd6e8baa5f20f766a396999986560f6bb3bba723a7`;
brush-contact APK: `83846a2127c589d05ed5a09cbb740b3ad9cb236ce0a4abb7abd9ab61b889cc32`.
Raw runs, source identities, fixture recipes, memory and restart diagnostics:
`artifacts/dither-footprint.local/android/`.

The matched desktop diagnostic uses the RTX PRO 6000 Blackwell Max-Q, Vulkan,
driver 615.71.09, a 4096² canvas and 32 visible layers. Both binaries warm and
undo the complete scenario before measurement. Interleaved ABBA sets, each with
three repetitions, change moving-frame CPU p50/p95/p99 from
0.628/1.402/2.237 ms to 0.652/1.243/1.884 ms and completed-frame spans from
0.837/2.051/2.731 ms to 0.851/1.667/2.444 ms. Maximum moving completion is
3.272 → 3.075 ms; pen-up p99 is 7.166 → 7.964 ms. Neither version has a measured
frame over 8.33 ms in these continuous-motion runs. A separate matched diagnostic
with 2.1 s idle changes nine first-motion completions from 1.983–3.591 to
2.186–3.639 ms and moving p99 from 5.766 to 5.109 ms. Idle pen-up p99 grows
from 15.138 to 17.575 ms; six baseline and seven candidate pen-ups exceed
8.33 ms. Resident canvas memory grows from 758.4 to 1123.4 MiB; capture backing
peaks at 89.3 → 94.5 MiB. Work counters and rendered images match
exactly. These serialized offscreen spans do not qualify GTK presentation or
any tablet tier. Raw frames: `artifacts/dither-footprint.local/local/selector-final-*`.

### Retained-map Liquify

Measured on 2026-10-20 on the Wacom MovinkPad 11, Mali-G57 MC2, with the
retained-map implementation based on `b7fb3654a`. Benchmark APK SHA-256:
`7a5a27c61b93890c744413a0f31a4d6c54c074377884af3ac8e7c3bb2f115c05`.
The eight rows above use the 6000 × 4000 photo, Size 512, full pressure,
16 ms prediction and three five-second contacts after shader warm-up.
`--paint-layer-name Photo` selects the photo itself. Reconstruct first seeds
its map with a five-second Push stroke; each measured contact is undone before
the next.

The 300 × 180 screen-pixel semiaxes run at Fit zoom 0.296438 in the
2200 × 1440 viewport. Sketch is collapsed and Stats is closed; thermal status
is zero. Adaptive refresh is enabled for the benchmark application. A display
dump before the matrix verifies the active 90 Hz mode; all 24 trials record
the host's nominal 11.111111 ms interval, not physical display timing. Hardware
refresh was not captured per trial. Values are median completed
updates consuming fresh input per second with the three-run range, and the
worst run's fresh completion-gap p99. Completed nonempty and fresh-input rates
are equal except for two additional completed updates in one Expand trial.
They do not measure physical scanout or pen-to-display latency. Every mode
misses both 90/s and the 22.2 ms gap limit. Ordinary-stroke
pen-up settlement reaches 846 ms for Push, including momentum and publication.

## Layer attachment comparison

Measured on 2026-10-04 on the Wacom MovinkPad 11. Current M1 source is
`bc295637a65387dc8a712d3841fb553ec85057fa`; the matched control is
`10811a2ee47165cdfeb05f9f0cc23df2b73263d6` with identical test-only brush
instrumentation. Both use release Rust in the unminified Android benchmark APK.
Frozen sources, harness hashes, APKs and raw records are under
`artifacts/layer-attachment-{control,m1}-rebased/`;
`matched-final-comparison.json` records the paired geometry and results.

The same 6000 × 4000 Sony photo has one empty paint layer above it, Perceptual
blending, Navigator open, Stats and tracing closed, default glass and 16 ms
prediction. Thermal status is zero before and after every fixture. Each fixture
has a priming stroke undone and three warmed five-second runs. Fit is 15.99%;
the 210 × 110 surface-pixel ellipse keeps the entire 1536 px footprint inside
the photo. Camera and geometry match exactly. The owner-blur fixture attaches
Gaussian Blur at sigma 8 to the photo and clips the paint above it; metadata
validates the attachment, row order and actual radius.

| Workload | Control fresh updates/s, median (range) | Current fresh updates/s, median (range) | Current timing | Qualification |
| --- | ---: | ---: | --- | --- |
| Continuous G-Pen 1536 px | 62.17 (61.54–62.23) | 62.03 (61.74–62.13) | Fresh completion-gap p99 22.91–23.55 ms | Paired ranges overlap; 90/s and 22.2 ms criteria not met |
| Continuous G-Pen 32 px | 197.97 (197.24–198.12) | 197.84 (197.47–197.94) | Fresh completion-gap p99 8.05–8.50 ms | Measured stroke criteria met; fresh ranges overlap |
| Clipped 1536 px, repeated contacts | Active 19.23 (18.01–20.41) | Active 20.01 (19.21–20.80) | Renewed-completion p99 63.84–71.26 ms; pending input-queue p99 3.72–4.25 ms | Refinement diagnostic; paired rates and renewed completion overlap |
| Paint clipped to an attached Gaussian, repeated contacts | No equivalent owner fixture | Active 20.80 (20.80–21.22) | Renewed-completion p99 82.54–92.65 ms; pending input-queue p99 3.57–4.36 ms | Refinement diagnostic; no prior owner equivalent claimed |

Repeated-contact rows use 100 ms contacts and 100 ms pauses. Their denominator
is the summed active contact windows, approximately 2.5 seconds, excluding idle
pauses. They do not qualify sustained 90/s motion. Clipping observes 24 resumed
contacts while refinement is pending per run; owner blur observes 25. Renewed
completion means are 54.18–57.01 ms for clipping, versus 56.28–57.61 ms on the
control, and 55.67–57.69 ms for owner blur. The owner-blur tail remains higher.
Clipped settling is 864–907 ms, versus control 827–924 ms; owner-blur settling
is 5.49–6.34 seconds. New contacts interrupt that work. These timing counters
do not establish unchanged-effect reuse or GPU execution time.

At 32 px, total completed updates including prediction are 308.18–312.75/s
(median 312.31), versus control 314.59–317.95/s (median 315.04). The reverse-order
control reaches 313.11–316.72/s (median 315.98), retaining a 1.16% median
difference. Fresh-input ranges overlap both control batches, but the smaller
prediction-inclusive rate remains recorded; it is not fresh-input throughput.

Rates count completed raster work, not display scanout or physical pen latency.
The panel holds 60 Hz, so no 90 Hz presentation or complete simple-brush-class
qualification is claimed. Current Gaussian-slider, low-tier and top-tier
attachment measurements remain unverified. Earlier Gaussian measurements and
rejected intermediate binaries remain in their own artifacts and do not qualify
this final source.

## G-Pen at the 1536 px guarantee

Measured on 2026-10-01 UTC on the Wacom MovinkPad 11 at `2e7dd29f4`:
6000 × 4000 reference photo beneath one empty paint layer, Perceptual blending,
Fit, 310 × 150 px ellipse, 200 Hz injected stylus samples and 16 ms prediction.
The optimized release benchmark uses the default workspace with Navigator and
Stats closed. Each size has a priming stroke undone followed by three warmed
five-second strokes. Tracing and memory sampling are disabled; all before/after
thermal-status readings are zero. Both sizes use the same APK and setup.
The observed zoom is 15.99%. The path leaves at least 5.96 surface pixels
between the full 2048 px brush footprint and the photo edge; the 1536 px
footprint leaves at least 46.90 pixels.

| G-Pen diameter | Fresh completed updates/s, median (range) | Completion-gap p99, range | Settling after pen-up, median | Qualification |
| --- | ---: | ---: | ---: | --- |
| 1536 px, guaranteed size | 55.64 (54.86–56.06) | 31.87–33.45 ms | 540 ms | Below 90/s; gap exceeds 22.2 ms |
| 2048 px, above guarantee | 33.36 (33.11–33.47) | 45.61–56.32 ms | 708 ms | Above-guarantee comparison |

Reducing diameter raises this stroke's fresh throughput by 66.8%. The 1536 px
row still needs 61.7% more throughput to reach 90/s, along with shorter tail
gaps. This size comparison is not a renderer-code speedup or a hardware
impossibility proof. It does not qualify other trajectories, the rest of the
simple-brush class or 90 Hz screen presentation. The previously measured
2048 px refinement and contact latencies below retain their original conditions.

The wider 479.7 × 299 px path clips the brush at the photo edges. The same APK
reaches 71.59 fresh updates/s at 1536 px and 48.03/s at 2048 px on that path,
with completion-gap p99 24.94–28.21 and 35.65–37.79 ms. Those are diagnostic
comparisons and do not qualify the contained-footprint workload required by
[the measuring guide](measuring.md#rules). The older wide-path 2048 px records
below also retain that distinction.

Optimized benchmark APK SHA-256:
`eb91ff69cafd3560603e242fda1e46af9f372d97b4004442ed95927cc1ffd2cc`.
Raw records, immutable APK and build provenance:
`artifacts/mid-tier-1536/contained-gpen-{1536,2048}`, `contained-results.json`
and `provenance.json`. The clipped diagnostics remain in `gpen-{1536,2048}`.

## G-Pen above the 1536 px guarantee

Measured on 2026-09-30 against `ba8835fec`: the tier photo beneath one paint
layer, Perceptual blending, 2048 px G-Pen, Fit, 16 ms prediction, default
workspace with Stats closed, warm-up and three five-second strokes. Thermal
status is zero. This historical front-stack and covered-pixel candidate raises fresh input
throughput from 37.72 to 49.02 updates/s. Completion-gap p99 is
32.55–34.35 ms. The 90/s rate target and 22.2 ms gap target remain open.
This does not qualify the class or its other brushes.
Settling increases from 671–679 ms to 787–811 ms; smaller idle-refinement
batches trade completion time for admission of fresh input.

Candidate: `c452a0642` (production source matches `fc5d00fd5` after the
test-fixture rebase), optimized benchmark APK SHA-256
`865b0dd05b8253b3b22eac806136023a5befeac5b642e5fddb2f3cd24194d047`.
Raw records: `artifacts/optimization-roi/{current-main,final}-mid-fit`.

### Layer-neutral composition and refinement

Measured on 2026-09-30 on the MovinkPad 11: 24 MP Perceptual photo, G-Pen
2048 px at Fit, 479.7 × 299 px path, 16 ms prediction, default workspace,
Stats closed and thermal status zero. Three warmed five-second strokes in the
selected balanced-root, four-page build reach 48.04 fresh updates/s
(47.90–48.08), with completion-gap p99 36.78–38.06 ms. The 90/s and 22.2 ms
targets remain open.

The controlled two-page build reaches 47.96/s. Restoring four-page batches
shortens median settling from 903 to 687 ms, while median resumed-contact
GPU completion changes from 89.1 to 92.4 ms. Allowing fresh-input submission
behind one unfinished batch reduces median submission from 48.5 to 27.8 ms
and GPU completion from 112.6 to 92.4 ms against the four-page gated control.
Continuous-stroke rates are unchanged. The cap is restored to four; queueing
remains. See [responsiveness](responsiveness.md#refinement-batch-tradeoff) for
the complete controls and limits.

Source base: `1557688aa` plus the layer-neutral root and four-page cap changes.
Optimized release benchmark APK SHA-256:
`05d747e5f3e6417e9ea3a96a0d1b5bfe38a3aca00744891bbdd01c9a37334a8b`.
Raw records: `artifacts/refinement-tradeoff/four-overlap-mid-constant`;
the source patches and other controls are in the same artifact directory.

## Retouching with the integrated compositor

Measured on 2026-09-29 on the reference tablet: 24 MP Perceptual photo,
512 px brushes, Fit zoom, 479.7 × 240 px trajectory, 16 ms prediction, three
five-second strokes per brush. Stats is closed. Fresh
updates count completed frames that consumed new real pen samples.

| Brush | Fresh updates/s, median | Completion gap p99, median | Moving-stroke rate |
| --- | ---: | ---: | --- |
| Clone Stamp | 115.7 | 15.2 ms | Meets 90/s |
| Healing Brush | 113.7 | 15.2 ms | Meets 90/s |
| Spot Healing Brush | 119.7 | 13.6 ms | Meets 90/s |

These measurements cover drawing. Clone settles in 470–566 ms; Healing in
1633–1830 ms and Spot Healing in 2702–2821 ms. Completion after pen-up remains an
open gate. They do not qualify presentation-paced navigation or the other
brushes. The measured build integrates `64036b643` with `13720d303`, bounded
bakes, the healing solver optimization and sparse stroke replay. Its APK SHA-256
is `be9a9ad367b9a0215ea5f57ac9d14028818d1f029fe8787324b66d3d4ac71e48`.
Raw reports are in `artifacts/integration/source-record-batch/mid`.

## Transform presentation

Measured 2026-09-29 on the 24 MP Perceptual reference photo at Fit, release Rust,
default glass and Stats closed. Each row contains three warmed five-second
gestures, with thermal status zero. Values are medians across runs. The harness
sets Navigator visibility explicitly and asserts the renderer's overview count.

| Journey | Navigator closed, completed updates/s | Navigator open, completed updates/s |
| --- | ---: | ---: |
| Placed-photo translation | 99.42 | 82.08 |
| Placed-photo resize, bar hidden | 90.87 | 79.23 |
| Placed-photo resize, bar visible | 90.64 | 78.90 |
| Pixel transform translation | 186.54 | 79.90 |
| Pixel transform resize: Free | 182.14 | 77.72 |
| Pixel transform: Distort | 168.17 | 73.10 |
| Pixel transform: Warp | 72.48 | 69.36 |

Median screen rates are 58.88–59.19 presents/s with Navigator closed and
58.87–59.20 with it open; median screen interval p99 is 16.78–16.81 ms.
These rates do not qualify the 90 Hz tier: the panel presents at 60 Hz.
SurfaceFlinger screen timestamps do not independently establish canvas scanout.

Single-input affine and perspective previews sample their retained source
directly into presentation when Navigator is closed. In these measured builds, a
visible Navigator shares a materialized composition with the main canvas to
preserve area-filtered preview quality. Opening its panel also changes the Fit camera and work area;
open/closed rates are different workloads, not an isolated panel-cost estimate.

Closed-panel build: `e41ee80fe` plus direct transform presentation, Navigator
materialization and placement refinement scheduling, benchmark APK SHA-256
`ede35aa5b23bd8770d1207929e1b633a25b481f7987eb7201645125a11e013e3`.
Raw runs: `artifacts/latency-investigation/deferred-9-mid-closed-fixed`.
Open-Navigator rows also include fragment materialization and source-specific
main-view shader entries, APK SHA-256
`1dbac0ef1dcf9208709ba3e7df71f0a028e99d6d25e4c60356e7413a270f1208`.
Those runs are in `present-18-mid-navigator`, including its `extra` directory.
Earlier default-panel runs retained Navigator;
they are not evidence for the closed-panel workload.

## Input during Healing finalization

MovinkPad 11 navigation during large Healing/Spot Healing finalization has
input-queue p95 of 15.2–18.4 ms and a maximum of 28.8 ms. The largest settle
callback is 51.7 ms. The one-second pinch probes present at 56.5–58.6/s on the
60 Hz panel; the 90 Hz tier target remains unqualified. System available memory
stays above 1,358 MiB.

Measured on 2026-09-29, three runs per brush on the tier photo, 512 px,
Perceptual, Fit and Stats closed. Dependent painting queues until the healed
raster publishes. The [responsiveness record](responsiveness.md#healing-finalization)
contains the build, workload, tool-action limits and raw records.

## Pinned localization comparison

Measured on 2026-10-01 PDT / 2026-10-02 UTC on the Wacom reference tablet,
comparing `271918681` with source tree `e56742a5`. The release APK hashes and
attribution limits are in the [low-tier comparison](low-tier.md#pinned-localization-comparison).
These observations do not qualify the later GPU-bounds successor.

The 6000 × 4000 Sony photo has one empty paint layer, Perceptual blending,
Fit zoom 15.99%, default Navigator and glass, Stats closed and 16 ms prediction.
Three warmed ten-second OS stylus strokes use G-Pen 1536 px and a contained
310 × 150 px trajectory. Settings, camera and visible layers match; painting
thermal status is zero before and after each run.

| Source sequence | Fresh completed updates/s | Maximum fresh gap p99 | 90 fps / 22.2 ms criteria |
| --- | --- | --- | --- |
| Baseline | 55.54–55.90 | 30.61 ms | Not met |
| Candidate | 55.81–56.45 | 29.55 ms | Not met |
| Repeated baseline | 55.49–56.17 | 28.86 ms | Not met |

The ranges overlap; no regression is demonstrated for this stroke. Both
versions miss the target. This does not qualify other brushes or scanout.

| Numeric motion, three warmed ten-second drags | Baseline UI Hz / maximum p99 | Candidate UI Hz / maximum p99 | Raw canvas completions/s, baseline → candidate |
| --- | --- | --- | --- |
| Exposure | 59.20–59.50 / 16.77 ms | 59.20–59.40 / 16.77 ms | 32.05–32.95 → 31.08–32.42 |
| Chain Exposure | 59.00–59.30 / 33.50 ms | 59.20–59.30 / 16.79 ms | 24.46–24.77 → 24.05–24.67 |

The display holds 60 Hz and neither numeric row meets 90 fps. UI frames and
raw completions do not establish fresh photo previews: completion records
include Navigator and lack effect input/revision pairing. The low-tier numeric
decrease and its attribution limits remain explicit. Numeric thermal status
was zero before every gesture; the final after-snapshot was not captured.

## Brightness to Opacity and Threshold

Measured on the reference tablet on 2026-10-05. Fresh merged first-pair
measurements use `merged-pair` benchmark APK
`5cc546ae3f9bb1a0372f660660757c970949becfca8f90440f3c85edeb75cf97`, built from
`09e2193f86f155977022fadd4248c768b7fd5acc`. Each completed case verifies its
installed APK; raw completion and contact timing are independently recomputed.
Recorded thermal boundaries are zero.

The optimized Android benchmark uses 6000 × 4000, G-Pen 1536 px, default
workspace with Navigator, Stats closed, prediction enabled and workspace ink
color. Motion cases use three warmed five-second strokes. Threshold uses Black,
Threshold Transparency and retained Alpha Threshold 37%; Brightness to Opacity
has no parameters. Enabled and disabled cases use matching authored graphs and
workloads.

**Incremental efficiency accepted; absolute tier targets are not met.** The
revised criterion accepts efficient incremental work over an already failing
disabled baseline. Disabled-baseline failures remain failures; paired throughput
does not isolate GPU filter cost or prove optimality.

| Filter | Blending | Zoom | 64 px canvas on / off | Large canvas on / off | 64 px fresh on / off | Large fresh on / off | Added large canvas time (ms) |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| Brightness to Opacity | Perceptual | Fit | 281.35 / 293.35 | 90.19 / 65.10 | 198.23 / 197.36 | 90.19 / 65.10 | -4.273 |
| Brightness to Opacity | Perceptual | 100% | 285.45 / 307.11 | 18.76 / 22.75 | 198.28 / 197.97 | 18.76 / 22.75 | +9.351 |
| Brightness to Opacity | Linear | Fit | 286.73 / 301.95 | 91.14 / 66.86 | 198.05 / 197.35 | 91.14 / 66.66 | -3.984 |
| Brightness to Opacity | Linear | 100% | 284.45 / 308.81 | 19.35 / 23.97 | 197.94 / 197.82 | 19.35 / 23.97 | +9.945 |
| Threshold | Perceptual | Fit | 280.18 / 294.32 | 90.88 / 64.20 | 198.50 / 197.59 | 90.88 / 64.20 | -4.572 |
| Threshold | Perceptual | 100% | 282.58 / 307.64 | 18.94 / 22.93 | 198.30 / 197.88 | 18.94 / 22.93 | +9.178 |
| Threshold | Linear | Fit | 285.60 / 303.67 | 92.67 / 67.02 | 197.91 / 197.32 | 92.67 / 67.02 | -4.129 |
| Threshold | Linear | 100% | 288.09 / 306.27 | 19.54 / 24.14 | 198.23 / 197.98 | 19.54 / 24.14 | +9.746 |

The target is 90 completed canvas updates/s with p99 gaps at most 22.222 ms.
Fresh input-consuming updates and their gaps are reported separately. Medians
above summarize three runs; the gap and age columns show ranges across the three
per-run percentiles. Added time is `1000/enabled rate − 1000/disabled rate`, a
throughput comparison rather than isolated GPU filter timing.

Response measurements for the large brush (ranges across per-run percentiles):

| Filter / blending / zoom / state | Canvas gap p99 (ms) | Fresh gap p99 (ms) | Input age p95 (ms) | Input→GPU p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| Threshold / perceptual / Fit / Enabled | 19.93–21.38 | 19.93–21.38 | 40.00–41.00 | 42.37–43.67 |
| Threshold / perceptual / Fit / Disabled | 22.33–22.57 | 22.33–22.57 | 50.00–50.00 | 50.68–53.11 |
| Brightness to Opacity / perceptual / Fit / Enabled | 19.85–21.43 | 19.85–21.43 | 40.00–40.00 | 43.01–44.05 |
| Brightness to Opacity / perceptual / Fit / Disabled | 21.77–23.39 | 21.77–23.39 | 50.00–50.00 | 50.55–51.15 |
| Threshold / perceptual / 100% / Enabled | 58.43–62.94 | 58.43–62.94 | 155.00–155.00 | 157.46–158.74 |
| Threshold / perceptual / 100% / Disabled | 47.88–49.14 | 47.88–49.14 | 130.00–130.00 | 129.19–130.39 |
| Brightness to Opacity / perceptual / 100% / Enabled | 59.86–61.39 | 59.86–61.39 | 155.00–155.00 | 158.28–159.16 |
| Brightness to Opacity / perceptual / 100% / Disabled | 49.02–56.73 | 49.02–56.73 | 130.00–130.00 | 131.13–133.24 |
| Threshold / linear / Fit / Enabled | 19.73–20.21 | 19.73–20.21 | 40.00–40.00 | 40.56–42.28 |
| Threshold / linear / 100% / Enabled | 57.94–59.42 | 57.94–59.42 | 150.00–150.00 | 151.60–153.64 |
| Threshold / linear / Fit / Disabled | 20.66–22.79 | 20.66–22.79 | 45.00–50.00 | 48.39–53.13 |
| Threshold / linear / 100% / Disabled | 45.67–46.92 | 45.67–46.92 | 120.00–120.00 | 123.40–124.03 |
| Brightness to Opacity / linear / Fit / Enabled | 19.93–20.25 | 19.93–20.25 | 40.00–40.00 | 41.91–42.22 |
| Brightness to Opacity / linear / 100% / Enabled | 58.49–59.27 | 58.49–59.27 | 150.00–150.00 | 154.12–154.75 |
| Brightness to Opacity / linear / Fit / Disabled | 21.04–23.84 | 20.47–23.84 | 45.00–50.00 | 48.66–52.84 |
| Brightness to Opacity / linear / 100% / Disabled | 46.32–52.30 | 46.32–52.30 | 120.00–125.00 | 123.06–125.34 |

Among 36 completed motion cases, 18 pass all three runs of the canvas
rate-and-gap gate, 18 pass the fresh rate-and-gap gate, and 16 meet the age
limit. These separate counts do not establish a combined pass.

[Full current rows, individual
gates](../../artifacts/illustration-filters/pair1/merged-pair-performance-doc-draft-mid.txt).
Absolute disabled-baseline misses remain failures; the revised criterion
assesses efficient incremental cost separately.

 | Fit Perceptual large brush | State | Long-stroke settle range (ms) | | --- |
--- | ---: | | threshold | Enabled | 371.31–472.61 | | threshold | Disabled |
458.08–515.41 | | brightness_to_opacity | Enabled | 361.98–368.73 | |
brightness_to_opacity | Disabled | 449.90–503.83 |

Three long strokes give a range, not a settling percentile. Short local-stroke
settling remains separate.

| Three-photo translucent stack, varying pressure | Canvas on / off | Fresh on / off |
| --- | ---: | ---: |
| threshold | 143.85 / 144.90 | 136.19 / 128.87 |
| brightness_to_opacity | 144.03 / 143.29 | 135.74 / 128.79 |

Native pointwise output is evaluated into retained filter images before
reduction. Invalid required native pages are captured in contiguous row batches
of at most 16, skipping valid pages and gaps. Historical native-direct
diagnostic traces and reviews describe this implementation; their older APK
measurements do not qualify this merged build. Current paired rates preserve the
material native-resolution increments: approximately 9–10 ms per effective
canvas update on Mid, 2.5–3.0 ms for Threshold and 4.9–5.4 ms for Brightness to
Opacity on Top. The pointwise algorithm evaluates required changed native pixels
once before reduction. Current 73 GPU tests and 16 refinement tests cover no-op
reuse, bounded passes, native ordering, source leases and the older-path
counterfactual. Source and output bindings cache actual view tuples for the
current and previous frame; different resources require distinct groups. These
code and physical-work checks support scoped algorithmic efficiency acceptance
without proving every measured tail unavoidable or global optimality.

On the 6000 × 4000 canvas, matched Threshold enabled/disabled runs each
completed 20 five-second 1536 px GPen contacts with Undo between contacts, then
retained the final stroke for 120 seconds. Threshold uses Colors=1,
Transparency=1 and alpha threshold=37 in Perceptual blending. Thermal status was
0 at both boundaries of every run. Settling uses nearest-rank p95 across 20
contacts.

| Threshold | Five-second settle p95 (ms) | Max sampled GPU allocated / reserved (MiB) | Max sampled PSS / RSS (MiB) | Minimum sampled system MemAvailable (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Enabled | 485.344 | 1425.77 / 1451.20 | 2123.18 / 2219.66 | 2391.81 |
| Disabled | 521.043 | 1422.86 / 1443.20 | 2149.73 / 2247.32 | 2319.88 |

The maxima and minimum span the whole captured sampler, including setup and Undo
after idle where recorded; maxima are separate samples, not simultaneous or
continuous peaks. Idle comparisons exclude Undo after idle. These long-stroke
settling results do not establish the short local-stroke settling target.

Enabled contact 5→20 endpoints: PSS -62.38 MiB, RSS -26.79 MiB, mappings -463;
tracked GPU allocated +0.05 MiB and reserved +0.00 MiB. During 120.051s idle:
PSS +0.91 MiB, RSS +0.02 MiB, mappings -120, tracked GPU allocated -0.06 MiB and
reserved +0.00 MiB. Disabled contact 5→20 endpoints: PSS -13.61 MiB, RSS +20.48
MiB, mappings -36; tracked GPU allocated +0.06 MiB and reserved +0.00 MiB.
During 120.051s idle: PSS -17.28 MiB, RSS -52.16 MiB, mappings -160, tracked GPU
allocated -0.06 MiB and reserved +0.00 MiB.

After idle, enabled minus disabled was PSS +40.66 MiB, RSS +40.02 MiB, tracked
GPU allocated +2.85 MiB and reserved +8.00 MiB. These are separate
processes/runs with matching requested setup; the differences do not isolate
allocator or driver history.

The private-photo large accepted-affine journey passed in 189.541s: 20 nonzero
drags, accepted Apply, 120.155s idle, resumed nonzero drag and Apply. All 30
snapshots and 21 input probes share one PID and ordered boot times; map counts
and live canvas input paths match. This exercises accepted resource high-water,
not an exceptionally large individual command buffer.

| Large affine stage samples | GPU allocated / reserved (MiB) | PSS / RSS (MiB) | Minimum system MemAvailable (MiB) |
| --- | ---: | ---: | ---: |
| Separate maxima over 30 stages | 1411.47 / 1431.00 | 2287.96 / 2385.83 | 2437.45 |

Accepted idle: PSS -126.55 MiB, RSS -147.60 MiB, mappings -200, tracked GPU
allocated +0.00 MiB and reserved +0.00 MiB. Idle end→resumed Apply: PSS +21.88
MiB, RSS +18.62 MiB, mappings +143, tracked GPU allocated +0.00 MiB and reserved
+0.00 MiB.

[Large-affine resource
ledger](../../artifacts/illustration-filters/pair1/mid-merged-pair-large-memory-summary.json).

PSS is process-level evidence, not isolated driver command-memory accounting.
Snapshot fields are read sequentially; nearest same-PID contact PSS samples are
strictly inside idle with explicit time offsets. Mapping counts do not count
command pools. These sampled runs establish neither a transient peak bound, a
per-command byte bound, nor a leak or its absence.

[Current resource ledger and raw
audit](../../artifacts/illustration-filters/pair1/merged-pair-current-memory-summary.json).

Resumed 100 ms contacts use three runs of 21 contacts per state. Cold is the
first contact; warm groups use actual pending-composition flags. Requested 5
ms/1,000 ms gaps alone do not determine actual state. The table reports actual
warm pending/settled next GPU completion, including completions after release;
GPU completion is not scanout.

| Filter | Requested gap (ms) | Actual warm group | Count on / off | Next GPU p95 on / off (ms) | Added p95 (ms) |
| --- | ---: | --- | ---: | ---: | ---: |
| threshold | 5 | warm_pending | 60 / 60 | 287.76 / 242.92 | +44.839 |
| threshold | 1000 | warm_settled | 60 / 60 | 58.72 / 52.22 | +6.502 |
| brightness_to_opacity | 5 | warm_pending | 60 / 60 | 290.34 / 236.67 | +53.675 |
| brightness_to_opacity | 1000 | warm_settled | 60 / 60 | 59.16 / 51.67 | +7.492 |

All completed matched resume rows fail absolute throughput and input-age
criteria. Contacts without an active-window fresh completion remain failures;
post-release completions count only as latency evidence. Final-input settling is
not per-contact settling p95. Positive paired contact penalties remain material
unresolved observations; historical traces do not establish their current cause
or inevitability. They remain explicit limitations of the scoped algorithmic
efficiency acceptance.

| Contacts without fresh completion in the active window | 5 ms enabled / disabled | 1,000 ms enabled / disabled |
| --- | ---: | ---: |
| threshold | 60/63 / 63/63 | 1/63 / 3/63 |
| brightness_to_opacity | 60/63 / 63/63 | 1/63 / 3/63 |

[Current raw resume
recomputation](../../artifacts/illustration-filters/pair1/merged-pair-matched-resume-summary.json).
Measurements remain attributed to benchmark APK
`5cc546ae3f9bb1a0372f660660757c970949becfca8f90440f3c85edeb75cf97`; final
fixture-correction APK confirmation is separate and does not relabel these
samples.

Separate final APK
`9de4984f656bbbd12a17e3023f7fc85e959d10d9885a3cc6d6150e3d25026c76` confirmation
uses three warmed Fit, Perceptual large-brush strokes per state. The
fixture-only correction has a separate runtime-equivalence audit; the full
matrix and resource evidence above remain attributed to the measured APK.

| Final APK Fit confirmation | Canvas on / off | Fresh on / off |
| --- | ---: | ---: |
| threshold | 91.46 / 64.49 | 91.46 / 64.49 |
| brightness_to_opacity | 91.46 / 64.54 | 91.46 / 64.54 |

[Final APK primary raw
audit](../../artifacts/illustration-filters/pair1/merged-pair-final-matched-evidence-audit-primary.json).
Native paint/mask/history/reopen and parameter/source-persistence checks pass
separately on both tiers. The Top supplemental trace and identical rerun fail
strict GPU coverage and remain preserved.

## Zoom settings navigation comparison

The 2026-10-09 1.0.12 run uses the tier photo, an empty paint layer and the
Navigator, warm shaders, thermal status 0 and three five-second gestures.
SurfaceFlinger actual-present timestamps establish the displayed rates. The
`smooth_zoom` benchmark uses native stylus input and verifies changing zoom
percentages without changing the drawing revision.

Baseline `f02d74518` horizontal Zoom measured 50.77–50.90 fps, p99 50.26–50.33 ms. Both
builds miss this tier. The measured build includes upstream rendering changes
as well as Zoom settings, so this comparison does not isolate their cost or
establish no regression.
Current per-gesture evidence: `artifacts/navigation-controls/zoom-audit-20261009/post-rebase-results.json`.

## Pen pressure response diagnostic

The 2026-10-10 build based on `12779d197` uses the authored soft pressure
response on the MovinkPad 11, with release Rust, an optimized benchmark APK,
the 6000 × 4000 reference photo and an empty paint layer at Fit. A pressure-varying
G-Pen at 1536 px follows a 310 × 150 surface-pixel ellipse. After one primer,
three five-second strokes measured 80.18, 81.40 and 81.27 GPU-completed updates/s.
Fresh-input completion-gap p99 was 24.98, 25.72 and 24.98 ms. These miss the
90/s and 22.2 ms completion criteria; they do not establish a regression against
a matched baseline. Thermal status was zero before and after.

The physical panel was running at 60 Hz and would not change refresh mode.
These are GPU completion measurements, not scanout timestamps. The pressure
utility's 90 Hz presentation target remains unmeasured. Raw reports, APK/JNI
hashes and provenance are in `artifacts/pen-pressure/android-mid-20261010/`.

## Pen response utilities

Measured on 2026-10-24 on the reference MovinkPad 11 at `d44a2bb89`, using
an unminified, non-debuggable Android benchmark APK and Rust release profile.
The 6000 × 4000 reference photo is below one empty paint layer at Fit, with
the default Sketch workspace, Navigator visible, Stats hidden and Low panel
glass. Thermal status is zero before and after. The private test application's
Adaptive entry is enabled after installation; supported rates are 60/90 Hz,
and utility interaction requests and observes 90 Hz during all 18 runs.

The existing canvas-bar harness measures three six-second gestures per case
after priming. Window dragging follows a 200 × 100 logical-pixel ellipse;
curve dragging follows a 12 × 6 surface-pixel sinusoid. These use OS-injected
stylus contacts. Live previews use Activity-dispatched synthetic pen hover
with varying pressure, inclination and direction. Rates and intervals are
Android window `FrameMetrics` during motion, not renderer submissions or
SurfaceFlinger scanout correlations.

| Utility motion | Presented fps, three runs | Worst gap p99 ms | 90 Hz target |
| --- | --- | ---: | --- |
| Pressure window drag | 88.58 / 89.25 / 88.91 | 11.193 | Met |
| Pressure curve drag | 88.91 / 89.25 / 89.25 | 11.187 | Met |
| Pressure live preview | 87.74 / 88.75 / 88.24 | 22.325 | Gap limit missed |
| Tilt window drag | 89.08 / 87.75 / 89.08 | 22.326 | Gap limit missed |
| Tilt curve drag | 88.26 / 88.75 / 88.75 | 11.226 | Met |
| Tilt live preview | 88.58 / 88.58 / 88.40 | 11.321 | Met |

Every run passes the 85.5 fps display-paced floor. The worst gaps for pressure live preview and
tilt-window dragging exceed the nominal 22.2 ms interval limit by about 0.13 ms;
the complete six-case target is not met. Sensor Activity-dispatch CPU p99 is
0.493–0.565 ms for pressure and 0.456–0.550 ms for tilt. This includes capture,
Compose routing and queue enqueue, excluding native-owner execution; it does
not isolate metadata-registration cost.

The earlier frozen APK, `f84a3d871ea8c442005e83d459079a3dbdcc91bc9e808bd768011089a98d9b0d`,
ran at 60 Hz with the private app's Adaptive entry disabled. Dragging measured
59.37–59.71 fps and sensor previews 29.85–30.02 fps by median; the latter used
the old 33 ms moving-model publication limiter. The current host coalesces
sensor publication on the owner's display callback and includes utility motion
in the workspace's refresh request. These builds also differ in upstream native
presentation code, so this is diagnostic history rather than a controlled speed
comparison.

Current APK SHA-256 is
`a3f6036b334fd3497bf5cfe972a6222f19c80e5ebc2ad30ab3c06b0a648a0c2c`;
JNI SHA-256 is
`3f42bce45e7dbcd691367d9fa3b930f65422f9b9ad84571df17207e6174d574c`.
Source/APK hashes, per-run summaries and the earlier baseline are recorded under
`artifacts/pen-tilt-editor/`; raw reports, refresh setup XML and thermal evidence
are retained in `/mnt/localssd/capycanvas1-tilt-editor-android/`. These measurements
apply only to this reference device and these utility motions.

## Animated filters diagnostic

Measured on 2026-10-18 on an XP-Pen Magic Note Pad, whose Mali-G57 MC2 matches
this tier's GPU; it is not the reference tablet. The ignored renderer test
`photo_filter_frame_time`, cross-built with `cargo ndk` in release, renders the
6000 × 4000 reference photo under each filter at Fit in a 1920 × 1200 view and
advances its animation one frame at a time. The control is `9a2dba37f`; the
optimized build adds display-grid evaluation and the noise changes. Values are
the median time to GPU completion of ten animated frames from two ABBA-ordered
runs, with thermal status zero before each run. A full recomposite without a
filter takes 39 ms in both builds. Each run sets `CAPY_FILTER_NAME`,
`CAPY_FILTER_EXTENT=6000x4000`, `CAPY_FILTER_VIEW=1920x1200` and
`CAPY_FILTER_PHOTO_RGBA` (raw RGBA of the photo at that extent), then runs the test
binary with `--ignored --exact tests::filter_library::investigation::photo_filter_frame_time`.

| Filter | Before ms | After ms | Speedup |
| --- | ---: | ---: | ---: |
| Film Grain | 3360 | 43.0 | 78× |
| Ripple | 3326 | 87.6 | 38× |
| Rainy Glass | 3321 | 85.1 | 39× |
| VHS | 3366 | 97.6 | 34× |
| CRT | 3392 | 99.6 | 34× |
| Heat Haze | 3416 | 96.4 | 35× |
| Iridescence | 3388 | 43.9 | 77× |
| Domain Warp | 3762 | 113.1 | 33× |

Before, every animated frame evaluated the filter at document resolution.
The filters now run on the reduced display grid, 3000 × 2000 here; Film Grain
and Iridescence share the fused pointwise pass. The remaining image-filter
cost is mostly that grid's Float32 input and output traffic: the same frame
with a pass-through Ripple takes about 67 ms. The 512-texel window split that
keeps native controls responsive costs about 8 ms of it.

The 9504 × 6336 top-tier photo in the same view fits on the 2376 × 1584 grid:
Ripple 7461 → 58.8 ms (127×) and Domain Warp 8461 → 74.4 ms (114×). When Fit
falls between 25% and 50%, that photo's reduced grid exceeds the display cache
budget and evaluation stays at document resolution; a Samsung Tab S7 at
2560 × 1600 took 5.7–9.0 s per frame in both builds. Raw logs and binary
hashes are in `artifacts/animated-filters-20261018/`.

## Paper lighting diagnostic

Measured on 2026-10-18 on an XP-Pen Magic Note Pad (MNP1095), whose Helio G99
class SoC and Mali-G57 MC2 match this tier's hardware; it is not the reference
tablet, and its panel ran at 90 Hz. `AndroidCanvasBarBenchmarkTest` with
`-e scenarios paper` opens the synthetic tier photo, shows a rough Paper with
Texture visibility raised, and drags each Paper control for 5 s in Properties
with OS injected touch: Texture visibility, a circle around the Light dial, and
Shows through paint between 20% and 80% (the first two at 100% Shows through
paint). The
benchmark APK was built from `fa32b31db` with the paper lighting changes;
thermal status stayed 0. Rates are completed canvas updates per second and
UI frames per second.

| Drag | 12 MP canvas | 24 MP canvas | UI |
| --- | ---: | ---: | ---: |
| Texture visibility | 59.8 | 55.6 | 83.1–88.3 |
| Light dial | 69.2 | 58.8 | 88.2–88.8 |
| Shows through paint | 68.3 | 60.9 | 88.9–89.1 |

Before, the same 12 MP drags updated the canvas 3.8 and 2.8 times per second.
Every Properties move applied the edit and then published the whole UI model,
about 2.5 ms and 8.9 ms on the render owner, so moves queued ahead of canvas
frames. Android now publishes a moving Properties gesture at most every 33 ms,
and always after its last move, while the canvas renders every frame. The same
change raises a 12 MP Exposure drag from 3.2 to 39.9 updates/s. The Layers
opacity field sends each value as its own edit and stays at 2.8/s. Raw reports
are in `artifacts/paper-android/final-*`.


## Vector layers

Measured 2026-10-20 on the MovinkPad 11 at `2567874a7`, built from `git archive`
(benchmark APK SHA-256 `5ca3906e…`, native library `b177594c…`), with
`android-vector-benchmark.py` and, for inking, `android-brush-benchmark.py`
([measuring](measuring.md#vector-layers-android)). The document is a
`crowded_vector_fixture` generated at that commit, 6000 × 4000, with 20,000
hatching paths on the edited Ink vector layer below a translucent Tones vector
layer, an empty Base paint layer and Paper hidden, at Fit (zoom 0.296) in the
default Sketch workspace with Stats and Navigator closed. **The panel ran at
90 Hz:** Adaptive refresh was switched on for the benchmark application after
every install.
Rows marked *round 3* were re-measured on 2026-10-20 on the round 2 and 3 tree
(`git archive e7cdd23412d7`, the same fixture generator) in alternating runs
against `2567874a7` where noted, and rows marked *round 4* on 2026-10-21 on the
round 2–4 tree (`git archive e2667d46`; the Outline width scrub on
`git archive a4027fb6`), each with a fixture generated from its tree; the
other rows are from `2567874a7`.
Each row has one 1.5 s priming gesture and three 5 s gestures (10 s for
inking), each undone; thermal status stayed 0. Presented rates join the canvas
SurfaceView's buffers to SurfaceFlinger presents; drag start is pen-down to the
present of the first frame submitted after the first move. Responses give the
first changed present and, where later, the present that completes the work.
Causes and proposed fixes for the misses are in [known gaps](known-gaps.md);
summaries and symbolized profiles are in `artifacts/vector-tablets-20261020/`.

| Row | Target | Measured | Result |
| --- | --- | --- | --- |
| G-Pen 24 px on the crowded layer, *round 4* | 90 | 163.9–178.4 completed updates/s, gap p99 10.1 ms; 168.5–176.5 on `e7cdd234` | **Met** |
| Pen-down 5 ms after committing a 195 ms line | 22 ms | First GPU completion p50 23.9–24.6, p95 32.8–35.5, max 38.6–41.3 ms | **Not met** |
| Move drag, 1 / 100 / 5,000 paths | 90, 100 ms start | 89.2–89.4 / 89.6 / 89.4 presented/s, p99 ≤11.5 ms; start 58–63 / 42–56 / 55–62 ms | **Met** |
| Scale with the Move frame, 100 / 5,000 paths | 90 | 89.6 / 89.4/s, p99 ≤11.4 ms; 5,000 settles 0.73–0.77 s after release | **Met** |
| Rotate with the Move frame, 100 / 5,000 paths | 90 | 89.6 / 88.6–89.0/s, p99 ≤11.5 ms | **Met** |
| Edit Points anchor drag, 1 / 100 paths selected, *round 4* | 90 | 89.4–89.6 / 89.6/s, p99 ≤11.4 ms; start 52–56 ms | **Met**; 62.2–62.4 / 59.8–60.0/s at `2567874a7` |
| Eraser: touched part / whole paths / up to intersection (64 px) | 90 | 89.4–89.6/s each, p99 ≤11.4 ms | **Met** |
| Properties Outline width scrub, 100 / 5,000 paths, *round 4* | 90 | 89.6 / 24.7–24.9 updates/s, p99 11.4–11.6 / 44.8–44.9 ms; 5,000 at 17.8–18.2 on `e7cdd234` | 100 **met**; 5,000 **not met** |
| Pan / pinch / rotate, no paths selected | 90 | 57.8–58.0 / 58.2–58.4 / 54.8–55.1/s, p99 22.4 / 33.5 / 22.5 ms | **Not met** |
| Same, 5,000 paths selected | 90 | 57.9–58.2 / 56.7–57.1 / 54.9–55.0/s | **Not met** |
| Fill inside a closed path | 100 ms | First change 32–64 ms, complete 100–111 ms | **Met** |
| Fill outside the paths (new layer below) | 250 ms | First change 0.80–0.81 s, complete 2.68–2.73 s | **Not met** |
| Commit after a 3 s line | No separate limit | Committed path presented 216–247 ms after pen-up | See the pen-down row |
| Undo of that line | 250 ms | 169–204 ms | **Met** |
| Merge Down onto a vector layer | 100 ms response | 316–331 ms | **Not met** |
| Merge Down onto a paint layer, from the notice | 100 ms response | 1.21–1.34 s; nothing changes on screen until it completes | **Not met** |
| Rasterize Layer | 100 ms response | 1.17–1.19 s; nothing changes until it completes | **Not met** |
| Frame that shows the finished Rasterize | Motion only | 231–251 ms CPU and 222–238 ms GPU | Not a motion row |
| Open the document | 4 s | Parsed 476–487 ms, first frame 746–849 ms, settled 1.80–1.91 s | **Met** |
| Paint-layer thumbnail after filling the whole Base layer | No limit | 3.78–3.83 s, polling every 120 ms like the Layers panel | Unqualified |
| Pan / G-Pen stroke while that thumbnail is pending | 90 | 55.7–55.8 / 89.6/s; 55.7–56.1 / 89.4–89.6/s without polling | No extra drops |

## Rotation snapping

Measured 2026-10-24 on Wacom MovinkPad 11, 6000 × 4000 reference photo,
thermal status 0 before, during and after three warmed five-second rotation
sweeps. Snapping holds within 3° and returns to free rotation by 10°.
The optimized candidate is based on `4a6abf44b`, benchmark APK SHA-256
`c3f340f3caa451588f7c664a1d667deeb1457ae8221740b7560f2cb6843f9a0a`.

The private app ran at 60 Hz although the panel supports 90 Hz. The existing
collector includes stationary snap holds and lacks per-sample camera transforms,
so its 59.698–59.706 total presents/s and 16.779–16.782 ms p99 gaps do not
qualify moving-frame rate or gaps. Viewport GPU p50 is 13.311–13.318 ms and
p95 is 14.073–14.098 ms. Rotate View drags remain unmeasured.
Evidence is under `artifacts/rotation-snap/android/performance/`.

## Native source preparation qualification

| Change | Affected work | Reference-device qualification |
| --- | --- | --- |
| Shared bounded preparation workers | Cold source/mip preparation, raster restoration and simultaneous windows | Not measured; existing motion results do not qualify this pool |

The [architecture](../architecture.md) defines the worker and queue bounds.
Warm canvas motion keeps its existing preparation and GPU paths.
