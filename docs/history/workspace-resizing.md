# GTK and Web column resizing

Measured on September 11–12, 2026 with optimized builds. Baselines use `2bea478`,
including Android's resize optimization and the benchmark foundation from
`6d3edc3`. GTK/shared publication landed in `b4a1559`; the Web implementation
also incorporates main through `414b346`.

## Implementation

Both frontends use the shared distinction between `model_revision` and
`content_revision`. Every delivered input still enters Rust. A changing model
with unchanged content retains controls and resources; only the newest absolute
layout is published on the display clock. Geometry-only dragging keeps its
existing, cheaper placement path. Content changes, collapse transitions,
completion and cancellation can still require full publication.

`UiSession::workspace_layout_update()` produces the common resolved layout,
workspace layout state, camera and panel measurements. The native host and Wasm
bridge use the same packet. The native host's existing serialized schema and
Android's revision semantics remain compatible.

GTK copies layout fields into its retained surface on the GTK frame clock and
queues native allocation. Its existing widgets measure, wrap and render live;
controls and Navigator resources survive resizing. It avoids full panel models
and widget refreshes on each input.

Web fetches one layout packet per animation frame, retains DOM controls and
skips content/editor/customization refreshes and main-canvas resizing. Intrinsic
measurement retains offscreen controls in a separate shadow tree using the app's
stylesheet. Width writes precede measurement reads. Only changed measurements
are sent back to Rust, using published measurements as the comparison source.
This removes repeated cloning and measurement feedback from steady resizing.

The Web Navigator resizes and redraws its backing canvas in the same display
callback as DOM reflow, using the existing GPU document image. Its backing
allocation grows in 64-physical-pixel blocks and survives shrinking. CSS clips
the spare capacity while keeping each rendered pixel at native resolution.
This avoids both per-pixel surface allocation and a frame of stretched content.
Capacity remains at the largest observed dimensions until the Navigator is
disposed; this trades some retained GPU memory for fewer allocations.

## Measurements

The tablet is a Wacom MovinkPad 14, Android 15, Chrome 152, Qualcomm Adreno 7xx;
the tested viewport was 1646×908 CSS pixels at device scale 1.75. The desktop
uses GTK 4.22.4, Mutter 50.4 and Chrome 152, with an NVIDIA RTX PRO 6000
Blackwell Max-Q Workstation Edition and Vulkan. Desktop tests run in a private
1600×1000, 120 Hz Mutter session.

Each case changes actual panel width over approximately 2.2 seconds, with
requested input intervals of 4 ms and a triangular ±65 px movement after
activation. The left panel spans approximately 201–331 px and the right panel
259–389 px. Startup, the initial press and gesture activation are outside the
timed interval. Platform input coalescing is allowed; all delivered inputs are
dispatched. Tests were run sequentially, without another performance suite.

Tablet Chrome results:

| Column | Input | Baseline geometry Hz | Optimized geometry Hz | Rate gain | Full state reads | DOM clones |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Left | Mouse | 114.5 | 119.1 | 4% | 384 → 0 | 1518 → 0 |
| Right | Mouse | 89.4 | 116.8 | 31% | 329 → 0 | 1576 → 0 |
| Navigator | Mouse | 76.7 | 116.8 | 52% | 317 → 0 | 1690 → 0 |
| Left | Touch | 93.4 | 115.5 | 24% | 384 → 0 | 1242 → 0 |
| Right | Touch | 71.1 | 114.6 | 61% | 305 → 0 | 1256 → 0 |
| Navigator | Touch | 71.1 | 114.6 | 61% | 292 → 0 | 1570 → 0 |

These are the final integrated-main results. The preceding optimized run reached
118.6/119.5/116.4 Hz with mouse and 119.1/115.5/115.0 Hz with touch. Across those
two runs, the slower right/Navigator cases improved by approximately 31–62%.
The final 114.6 Hz touch cases do not pass a strict 115 Hz floor; short-run
variation should be considered when interpreting proximity to 120 Hz.

All optimized tablet cases recorded zero visible DOM insertions/removals and
zero scaled Navigator frames. This fixture also needed zero measurement feedback
dispatches during steady resizing. The old path generated 3,212–4,224 panel views
per case; the optimized path generated none. Tablet full-state bridge calls had
p95 around 0.7 ms before; the replacement layout packet was around 0.2 ms, once
per display callback. Navigator redraw submission was around 0.3 ms p95. These
are CPU call durations, not GPU execution times.

The first Navigator case recorded eight canvas width/height attribute changes
while capacity grew, and the subsequent touch case recorded none. The counter
counts attributes, not unique GPU allocations. Every observed frame kept backing
pixels matched to the canvas's CSS size and device scale.

GTK results:

| Column | Input | Geometry Hz, before → after | p95 dispatch ms, before → after | Dispatch reduction | Full model refreshes |
| --- | --- | ---: | ---: | ---: | ---: |
| Left | Mouse | 118.7 → 118.7 | 0.234 → 0.030 | 87% | 397 → 0 |
| Right | Mouse | 117.5 → 118.7 | 0.261 → 0.031 | 88% | 273 → 0 |
| Navigator | Mouse | 119.1 → 118.7 | 0.243 → 0.033 | 87% | 275 → 0 |
| Left | Touch | 118.7 → 119.1 | 0.133 → 0.014 | 89% | 673 → 0 |
| Right | Touch | 117.3 → 118.6 | 0.141 → 0.015 | 90% | 550 → 0 |
| Navigator | Touch | 118.7 → 118.7 | 0.131 → 0.017 | 87% | 550 → 0 |

GTK was already display-paced on this workstation. The gain is reduced CPU
work and eliminated model refreshes, rather than a substantial frame-rate gain.
Dispatch timings include synchronous publication work but exclude later native
allocation, rendering and GPU work.

Desktop Chrome reached 117.3/116.9 Hz for left/right mouse resizing and
118.3/118.0 Hz for touch. Navigator resizing reached only 100.5 Hz with mouse
and 97.9 Hz with touch, despite zero full models, cloning, visible DOM changes
or scaled frames. Removing repeated DOM attachment and surface allocation did
not bring that configuration to 120 Hz. The remaining bottleneck is not isolated;
these measurements do not establish a driver or compositor cause. There is no
desktop Web baseline comparison here: the tablet is the representative Web
performance device specified for this task.

## Correctness and regression checks

- Real native mouse/touch through Mutter on GTK and desktop Chrome; browser
  mouse/touch input through Chrome DevTools on the physical tablet. All six
  resize cases check native geometry against Rust, retained controls/resources,
  drop and single-step undo/redo.
- Cancellation while a contact is held restores the previous layout. Desktop
  checks inject the host focus-loss signal, followed by native release; tablet
  touch uses Chrome's actual touch-cancel input path. This does not claim testing
  physical focus loss or a human finger's cancellation gesture.
- Browser mouse/touch tests narrow a Sizes panel from 310 to 170 px and verify
  that controls wrap before release while preserving control identity. Live and
  post-release panel screenshots had zero differing channels at both 1× and 2×
  (296,000 and 1,184,000 sampled channels respectively).
- The same-contact collapse/expand reversal, subsequent resizing and undo/redo
  pass on Web. Existing GTK native collapsed-column and Web double-click column
  sizing tests pass, including recursive groups.
- Existing Web Navigator pixel/clip/translation tests pass at 1× and 2×,
  including a concurrent content refresh and queued cancellation. Tablet editor
  checks pass for Navigator controls, nested drawers, columns, tools and project
  persistence.
- Existing GTK and Web native dragging benchmarks pass their 115 Hz floor with
  zero full model refreshes. Shared host snapshot regressions verify layout
  packets against full models, content/collapse transitions, measurements,
  history, legacy publication and failed-serialization handling.

The timing probes count changing geometry, rather than idle display callbacks.
GTK uses completed frame presentation timestamps associated with changed native
allocations. Web observes changed DOM geometry on `requestAnimationFrame`;
it does not measure physical scanout or input-to-photon latency. The short runs
are not a statistical estimate across thermal states, battery states, browsers
or hardware. GPU caches were warm and browser profiles were not cleared. The
tablet results support resizing near 120 Hz on that configuration, not a
universal 120 Hz guarantee.

## Reproduction

With GTK, Mutter, GJS, PipeWire, Chrome, the Wasm target and matching wasm-bindgen
installed, run from the repository root:

```sh
LAYER_RESIZE_RETAINED=1 LAYER_RESIZE_MIN_HZ=115 \
  tools/performance/workspace-motion.sh gtk --workspace-resize
LAYER_RESIZE_RETAINED=1 \
  tools/performance/workspace-motion.sh web --workspace-resize
tools/performance/workspace-motion.sh web --resize-rendering
tools/performance/workspace-motion.sh web --workspace-rendering
LAYER_MOTION_MIN_HZ=115 tools/performance/workspace-motion.sh gtk
LAYER_MOTION_MIN_HZ=115 tools/performance/workspace-motion.sh web
cargo test --locked --release -p layer-host snapshot::tests -- --test-threads=1
```

The runner builds optimized binaries, creates a private compositor/settings
directory and feeds virtual native input without moving the user's pointer.
`LAYER_WEB_PORT` chooses an unused local server port. `LAYER_MOTION_REFRESH`
selects the virtual monitor rate. The optional rate floor should match the
machine; desktop Navigator does not pass a 115 Hz floor in the configuration
reported above.

For a tablet, build with `bash apps/layer-web/build.sh`, serve `apps/layer-web`
on a private origin forwarded over USB, and connect the already-open test tab
using the device handoff instructions. Preserve user work and use a separate
origin. Then run:

```sh
LAYER_DEVICE_CDP=http://127.0.0.1:9228 \
LAYER_WEB_URL=http://127.0.0.1:8127/ \
LAYER_RESIZE_RETAINED=1 \
node apps/layer-web/device.test.mjs --workspace-resize
```

`LAYER_TEST_ARTIFACTS` selects a results directory; the default is
`artifacts/workspace-resize/`. The benchmark emits JSON with input counts,
changing-frame rates, width ranges, bridge timings, full refreshes, cloning,
DOM mutation and native-resolution checks. The original local measurements are
preserved under `artifacts/workspace-resize-sep2026/`; generated captures and
machine logs are not committed.
