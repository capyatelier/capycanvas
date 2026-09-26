# Web refresh responsiveness investigation

Investigated 2026-09-25 on `perf/web-refresh-responsiveness`, from `367cff57`.
The reported case is refreshing <https://editor.capycanvas.art/> with a moderate
image and eventually receiving “Recover drawing?”. This branch adds observation
tools, not a production startup change.

The main finding is a dependency problem as well as a scheduling problem.
Document commands and recovery still wait for the full startup filter library.
The DOM and current brush can already be ready. The compiler also admits unused
work during drawing. More filters and brush variants will increase both costs.

Wacom measurements confirm this: the workspace responds in **0.41–0.44 s** and
the brush is ready in **2.60–2.75 s**, while several document menu actions remain
disabled until **19.70–20.28 s**. An experimental early-recovery/catalog bypass
moved the prompt from **20.06 s to 1.27 s**. Pausing optional compilation during
pen contact reduced the 95th-percentile sample age at CPU submission from
**21.1–24.5 ms to 6.9–7.1 ms**. These are local optimized-build measurements on
the physical tablet, not a deployed fix or physical pen-to-photon measurements.

## What the current code does

| Order | Work and readiness boundary | What can delay it |
| --- | --- | --- |
| 1 | HTML/CSS, JS imports, icon sprite, streaming Wasm compilation. `workspace-preload.js` starts storage and Wasm early. | Download/cache, parsing, Wasm compilation. Service-worker caching avoids fetching immutable bytes again; it does not preserve live renderer objects across navigation. |
| 2 | Instantiate Wasm, create a **blank** session, restore preferences/layout, build controls, first layout. `capy.startup.ui` marks this boundary. | Synchronous model/DOM/layout work. The image awaiting recovery is not loaded here. |
| 3 | Workspace worker/IndexedDB ownership and saved workspace adoption. `capy.startup.workspace`. | Storage and main-thread reply/publication. Workspace ownership temporarily gates input. GPU setup waits for workspace readiness, capped at one second. |
| 4 | Acquire adapter/device; allocate renderer/presenter and initial pipelines; submit paper. | Initial GPU and driver work. `capy.startup.canvas` records submission observed by the host, **not GPU completion or display scanout**. The compiler waits for `wait_for_canvas()` before admitting later work. |
| 5 | Prepare document dependencies (priority 2), then current brush and physical eraser (priority 3). | Required pipelines, native writeback/publication, selected textures. Input begun before brush readiness remains suppressed through release. |
| 6 | Generate all remaining brush textures; compile unused brushes, attachment variants, transforms, selection/region/export kernels (priority 4). | CPU recipes and GPU compilation. The Web compiler batches up to four same-priority async pipelines; CPU recipes retain individual task boundaries. |
| 7 | Fetch/resolve the filter package, validate the linked namespace, and warm every catalog filter, including unchanged bundled programs. | Namespace and per-program Naga validation run in the main Wasm instance. Each program queues preview and fused/image variants. Library validation finishes after earlier priority-4 work, then publishes on a session frame. Fetching begins alongside the earlier stages, but readiness waits for compilation. |
| 8 | Recovery initialization begins at the first **15-second timer tick** after GPU/brush readiness. Acquire an owner, autosave, list old keys, then wait for `canOffer()`. | Timer rounding, worker startup, storage, current interaction/dialog, and `document_park_ready()`, which rejects a pending library transaction. |
| 9 | Only after the user chooses Recover: read recovery bytes, decode the project, prepare another renderer on the existing device, validate document effects, prepare source tiles, adopt the drawing, and checkpoint it. | Archive/image work, required shaders, tile validation/upload, publication. Float source preparation yields every four tiles. More optional warming follows adoption. |

The pre-prompt delay is therefore not necessarily decoding the moderate image:
the old drawing's bytes are requested by `recover-get` only **after** Recover is
chosen. Before that, this startup initializes a blank drawing and its catalog.
Outstanding storage from the previous page and saved workspace size remain
possible additional factors to measure.

Relevant implementation:
[host startup/scheduler](../../apps/layer-web/app.js),
[recovery](../../apps/layer-web/document-recovery.js),
[document transitions](../../apps/layer-web/documents.js),
[project preparation](../../apps/layer-web/src/documents.rs),
[shared priorities](../../crates/layer-render-wgpu/src/startup.rs),
[browser compiler](../../crates/layer-render-wgpu/src/startup_web.rs).

## Why visible menu items remain unavailable

`UiSession::command_flags` gates New, Open, Import/Paste Image, Export,
Document Properties and color conversions on `require_document_idle()`.
That method rejects **any** `pending_filters`, including the library-only startup
refresh. Settings and ordinary brush input have different guards. This explains
how some controls can respond while other menu actions remain unavailable for
the whole catalog warmup; delayed dispatch is a separate phenomenon.

The same guard is used by ordinary document parking. Recovery's `canOffer()`
requires parking readiness. If the brush is ready at 2 s, the first recovery scan
is still around 15 s after controls were constructed; if the catalog is ready at
25 s, the offer waits beyond that. If the brush misses the first timer tick,
initialization waits for the next tick. These are illustrations of the gates,
not new device measurements.

[Command availability](../../crates/layer-ui/src/session.rs),
[idle guards](../../crates/layer-ui/src/document_files.rs),
[parking](../../crates/layer-ui/src/renderer_lifecycle.rs), and
[library publication](../../crates/layer-ui/src/filter_loading.rs) establish
these dependencies. Closing/saving already distinguish a library-only refresh
from an import that migrates document effects; ordinary parking still waits.

## Where computation still competes with input

- The host pauses compilation for Settings and for 500 ms after closing it.
  It has no equivalent admission check for strokes, pen movement, panning,
  other dialogs, or open menus. `setTimeout(0)` after a frame provides a task
  boundary, not a CPU/GPU priority or a frame-time budget.
- Async pipeline APIs are already used. Replacing synchronous pipeline creation
  was an earlier fix, not a missing new feature. The WebGPU specification
  [prefers async pipeline creation](https://www.w3.org/TR/webgpu/#dom-gpudevice-createrenderpipelineasync)
  to avoid blocking queue work on compilation. It does not give this host a
  cancellable or lower-priority GPU compiler queue.
- CPU shader assembly/Naga validation and procedural texture generation remain
  on the UI thread. A single “job” is not time-bounded. The contact-paper recipe
  alone fills a 1024×1024 texture with three noise evaluations per pixel;
  watercolor transport fields also contain nested procedural computation.
- The current catalog contains 40 effects. Cold validation warms the entire
  namespace even when the downloaded package changes no programs. Brush warming
  also enumerates all 30 destination operation/attachment combinations, plus
  direct, gather, transport, export, mask and other kernels.
- Catalog preparation visits the accumulated pipeline cache for each program.
  A job-per-effect abstraction still needs bounds on source size, pass count,
  preparation work and allocations as the catalog grows.
- Filter preview scheduling already checks active strokes, touch, pending input
  and document edits in shared Rust. Its existing admission rules are a useful
  basis for compiler admission. Merely pausing thumbnails is insufficient while
  compiler jobs continue independently.
- Pending library validation also requests continuous session frames. A future
  paused compiler should wake on meaningful completions/required work rather
  than polling an otherwise unchanged document indefinitely.
- Recovering a project with effects has another ordering issue: its private
  renderer's effect validation is queued at `OTHER`, behind unused brush work,
  but the preparation loop waits for that validation. Required project effects
  need required priority as well as early preparation.

## Recommended changes, in order

1. **Separate library acceptance from speculative warming.** An identical
   library-only refresh should not create a document-blocking validation
   transaction. Use the already published catalog; warm private pipeline caches
   independently. Preserve atomic validation/publication for genuinely new or
   changed runtime packages, and preserve embedded project programs. Do not
   broadly weaken `require_document_idle()` for operations that really mutate
   the document.
2. **Discover recovery at storage readiness.** List records early, without
   waiting for the autosave interval, brush or full catalog. Listing keys in
   `raster-worker.js` currently even waits for Wasm initialization although it
   only needs IndexedDB. Present recovery when no contact/modal operation owns
   interaction. Keep/discard can work before canvas preparation; accepting a
   restore should prepare that actual document once the device is available.
   Retain Web Locks, per-drawing ownership, cancellation, and the rule that the
   origin is only retired after a durable replacement checkpoint.
3. **Give required work a separate admission lane.** Shared Rust should select
   required document/current-tool jobs and track readiness. The Web host should
   admit speculative jobs only during inactivity, with a short cooldown after
   input. Pause admission during active strokes, pending edits, canvas gestures,
   menus, dialogs and hidden-page periods. Required work must still progress
   when the selected tool changes; a blanket pause risks preventing readiness.
   Keep at most one speculative job in flight on weak devices. Already-started
   jobs cannot be preempted by this scheduling policy.
4. **Bound CPU work too.** Prebuild immutable procedural masks, or generate them
   in a worker with bounded uploads. Move package parsing/link validation and
   shader source preparation to an independent worker where practical. Yielding
   between multi-hundred-millisecond recipes cannot make those recipes smooth.
5. **Make effect/brush compilation demand-driven.** Order actual document
   effects, selected brush, visible previews, then optional predictions of next
   use. Track readiness by program/variant/device, with bounded cache memory and
   invalidation. Gate the affected tool's use until its async preparation has
   completed, and prioritize that preparation. Do not simply remove warmup:
   `Deferred::compile()` currently has synchronous first-use paths, so doing so
   can move the freeze to the first stroke or filter selection. Layer thumbnails
   should depend on their own pipelines, not global startup completion.

For long-term catalog growth, separate lightweight UI metadata from lazily loaded
shader sources too. The embedded fallback and the startup fetch currently both
carry the catalog. Avoiding GPU warmup alone does not eliminate growth in Wasm
download/instantiation, catalog parsing, validation or retained source memory.

A renderer worker with `OffscreenCanvas` could isolate more work later, but is
a larger ownership/input/presentation change. GPU objects are device-bound and
the WebGPU interfaces do not provide a transferable pipeline cache for compiling
on an unrelated worker device. Keep real async compilation and first-use
readiness even if renderer ownership moves. Browser caching is useful but cannot
be the correctness or latency strategy for an ever-growing shader catalog.

## Evidence and measurement status

The deployed page was fetched on 2026-09-25. Its service-worker version was
`c2ef37df1bb687fd1e96c2765ae726ca79f92bbd00d40b473b4100b41aab8ac4`, with
`assets/app.05f384e3bec15ebd2788.js` and
`assets/document-recovery.dd5da27bcbeff84f5860.js`.
Recovery is byte-identical to this checkout; startup and compiler scheduling
match after asset fingerprint normalization. There are unrelated UI differences;
this comparison does not establish identical Wasm binaries or the user's cached
service-worker version.

Physical tests used Wacom MovinkPad 14 / DTHA140, Android 15, Chrome
153.0.8010.52, the dedicated `http://127.0.0.1:4197/` origin over USB forwarding,
and an optimized `dev-perf` build. HTTP and browser/driver caches were retained
after an untimed warmup. Thermal status was 0 before and during the comparisons.
This does not measure a cold driver cache, production network delivery, or the
user's exact drawing. No production-origin storage was used.

### Passive refresh and menus

Two ordinary refreshes, with no synthetic interaction:

| Milestone from navigation | Run 1 | Run 2 |
| --- | ---: | ---: |
| Wasm ready | 240 ms | 250 ms |
| Controls constructed | 299 ms | 308 ms |
| Workspace ready | 407 ms | 441 ms |
| Paper submission | 849 ms | 922 ms |
| Document dependencies ready | 1,224 ms | 1,339 ms |
| Current brush ready | 2,599 ms | 2,751 ms |
| Full catalog ready | 19,705 ms | 20,277 ms |

Both runs made **201 pipeline-creation calls**. The 86 pointwise-effect pipeline
calls began at 12.94/13.49 s and finished at 19.70/20.27 s. Substantial brush and
other warmup precedes these effect jobs. Several destination-brush pipeline
promises individually took 1.1–1.5 s; four promises can overlap within a batch,
so their durations must not be added as wall time.

New/Open/Export/Properties were observed disabled after library staging and
remained disabled until publication near full completion. A separate menu-probe
run opened and populated **59/59 menus**: median 12.2 ms, p95 16.8 ms, maximum
25.7 ms for the click plus two animation callbacks. This distinction matters:
the menus open promptly while particular actions are unavailable.

### Recovery and ordering experiments

The fixture was a 2048×1536 patterned gradient PNG, 2,818,792 bytes, without
effects. It was opened normally and autosaved; there was one recovery record.

| Treatment | Recovery prompt | Document commands |
| --- | ---: | --- |
| Normal refresh | **20.057 s** | Disabled during catalog validation |
| Skip startup catalog transaction | **15.415 s** | No catalog-induced disabled period observed |
| Also start recovery discovery early | **1.271 s** | No catalog-induced disabled period observed |

In the baseline, recovery key listing started at 15.419 s and took only 5 ms;
the offer then waited until the library released the parking guard. Skipping
the library still warmed other shaders until 11.504 s, but the independent
15-second recovery timer dominated the offer. Early discovery listed keys by
0.445 s and offered recovery after initial document readiness.

Actual recovery was verified in both the normal and early paths, including the
2048×1536 dimensions, unsaved-document state and a screenshot of the patterned
image. After clicking Recover, adoption plus a queue-completion wait took
**0.476 s normally** and **0.668 s in the early experiment**. The early test's
click occurred at 3.988 s, well after its prompt appeared; that human/automation
delay is not preparation time. `recover-get` took 16–20 ms and the archive worker
read took 167–209 ms. New recovery writes completed before the origin record was
deleted. A filtered, larger or more complex photo can have different restore cost.

These are ablations, not shippable changes. In particular, skipping the catalog
can expose synchronous first-use effects. The production design must retain
validation and explicit readiness for the selected effect/tool.

### Drawing during background compilation

Two baseline/deferred comparisons replayed a seven-second pen contact during
startup and a five-second contact after startup. They used the same G Pen
preset, 18 px, pressure 0.65 and a nominal 120 Hz CDP event schedule. Each stroke
was accepted and undone; the blank document returned to unmodified state.

| During-startup metric | Normal scheduler, two runs | Pause optional compiler, two runs |
| --- | ---: | ---: |
| Delivered replay moves/s | 82.3, 86.4 | 119.2, 118.6 |
| CPU frame duration, p95 | 3.6, 3.0 ms | 1.2, 0.7 ms |
| Latest consumed sample age at CPU submission, p95 | 24.5, 21.1 ms | 7.1, 6.9 ms |
| New pipeline calls during contact | 20, 24 | **0, 0** |

After startup, the baseline samples delivered about 117 moves/s with 7.1 ms
p95 sample age. The deferred runs were close to that behavior while still
starting up. They completed full warmup later, around 28.1–28.5 s, as expected
after withholding roughly seven seconds of optional work. Making the entire
catalog complete sooner is the wrong optimization target for this workload.

CDP delivery can be limited by browser/protocol acknowledgement; these results
demonstrate contention and its removal, not physical stylus latency or display
scanout. Statistics stop at pointer-up and exclude samples from before the
contact. Start times and individual samples are retained. The second deferred
run starts its replay directly from the integrated `--strokes` option, earlier
than the separate replay process used in the first pair. No weak-device or
thermal-throttling speedup is claimed from these Wacom samples.

### Profiling and validation

A separate profiled run completed warmup at 20.418 s. Its main-thread long-task
observer recorded one 58 ms task, yet the GPU-process timeline contained a
306 ms WebGPU task during initial GPU setup, a 141 ms task around document
preparation, and 51–55 ms tasks during later filter warming. These are nested
GPU-process task durations, not additive shader costs. Main-thread Long Tasks
alone therefore miss relevant stalls. The sampled CPU profile also identifies
procedural contact-paper generation (about 33 ms sampled self time); job
boundaries do not impose a time limit on those recipes.

The optimized Wasm build passed. GPU-disabled desktop smoke runs verified the
observer, worker timings, menu probes, CPU profile, browser timeline and cleanup.
The reusable `--strokes` path passed on the Wacom, as did both actual recovery
paths. Browser error collections were empty for the physical captures. Software
WebGPU could not initialize this renderer and provides no GPU timing evidence.

Compact results, build hash and conditions are checked in at
[Wacom measurements](measurements/wacom-refresh-2026-09-25.json). Raw JSON,
profiles, Chrome timeline and restored-image captures are in the ignored
`artifacts/web-refresh/wacom/` directory. Prior context remains in the
[September 17 record](../history/web-startup-tablet-2026-09-17.md); the
[drawing recovery test](../../apps/layer-web/drawing-tabs.test.mjs) also records
a 149-second Huion cold catalog load. Its manual `startRecovery()` call does not
exercise the natural initialization timer.

## Reproducing and extending the trace

Use a dedicated origin and the forwarding workflow in [Web development](web.md).
Never point fixture setup at an artist's working tab. The runner only reloads
the matching URL and records observations; it does not delete storage, accept
recovery, or discard drawings. For recovery, open an image in the dedicated
origin and let autosave finish first. Choose Recover manually during capture
to include the restore path; `markEvents` retains both startup and replacement
renderer milestones.

```sh
LAYER_DEVICE_CDP=http://127.0.0.1:9247 \
LAYER_WEB_URL=http://127.0.0.1:4197/ \
LAYER_REFRESH_MS=180000 \
LAYER_TEST_ARTIFACTS=artifacts/web-refresh/passive \
node tools/performance/web-refresh.mjs
```

Repeat separately with `--probes` to open/populate/close a title-bar menu every
500 ms. These are scripted DOM clicks, not physical input-to-photon latency.
Use `--timeline --profile` in a separate diagnostic run to attribute long tasks
and GPU-process stalls; profiling adds overhead. `--bypass-http-cache` does not
clear browser shader caches or service-worker Cache Storage, and must not be
called a cold-driver run. `LAYER_REFRESH_RUNS` repeats completed captures without
overlapping an unfinished shader queue.

Three opt-in **experimental ablations** support causal comparison:
`--skip-catalog` suppresses the startup library refresh while retaining bundled
definitions; `--quiet-compiler` waits for contact release, closed menus/dialogs
and 500 ms of quiet before admitting a job after brush readiness;
`--early-recovery` starts discovery when controls are built. None is a production
fix: the first can expose synchronous first-use effects, and the second only
simulates host admission without a shared work budget. Hooks are restored after
capture; required jobs are allowed when brush readiness changes.

On an empty, dedicated test document, `--strokes` replays and undoes the startup
and steady-state pen contacts and writes `stroke-N.json`. Allow at least 45 s
of capture for this fixture. Compare it with `--strokes --quiet-compiler`, keeping
the selected brush and workspace unchanged. This option modifies its test drawing
and checks that undo returns it to unmodified state; omit it for passive captures.

Compare passive/probed warm reloads, first-device cache state, blank versus a
known moderate-image recovery fixture, and startup drawing versus steady-state
drawing. Record UI/workspace/brush/catalog readiness, command-enabled times,
recovery-list/offer/accept/first-restored-frame, long tasks, frame gaps, pipeline
count, and first use of previously unused effects. Adding unused shaders should
not move UI, recovery-offer or selected-brush readiness. Recovery pixel/history
round trips and drawing latency during optional work are acceptance checks,
not just shorter “complete” timestamps.
