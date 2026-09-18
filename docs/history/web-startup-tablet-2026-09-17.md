# Web startup on the Android tablet — 2026-09-17

Base: `origin/main` at `e578f476` (Add shared print proofing to macOS and
iPadOS). Device: Wacom MovinkPad 14 / DTHA140, Qualcomm Adreno 7xx, Chrome
152.0.7977.82. Measurements were taken on the attached tablet over a USB
localhost reverse tunnel, using the optimized development Wasm build.

## Findings

There are two independent causes of the apparent startup freeze:

1. **Workspace input is locked until filter validation finishes.** Web startup
   used `load_filter_package`, which calls the shared document-migrating
   `load_effect_package`. The pending operation fails
   `require_document_snapshot_idle`, so `WorkspaceController::tick` cannot adopt
   the saved workspace. `workspace_start` keeps the session read-only until
   adoption. Visible buttons consume contacts without activating, even after
   the current brush has compiled.
2. **Pipeline compilation blocks Chrome's GPU process.** The deferred scheduler
   calls wgpu's immediate `createRenderPipeline` / `createComputePipeline`
   APIs, then awaits error scopes. Returning to JavaScript between jobs does
   not prevent each job from blocking GPU processing and display callbacks.
   The trace contains WebGPU tasks of up to 538.5 ms on `CrGpuMain`; their
   times coincide with pipeline validation waits and display stalls.
   Main-thread JavaScript long-task reporting misses most of these freezes.

Three unprofiled reloads of the original build produced these medians:

| Milestone, relative to navigation | Time |
| --- | ---: |
| DOM controls constructed (`window.layerApp` assigned) | 702 ms |
| First canvas submission observed by the host | 1,343 ms |
| First viewport submission's GPU completion | 1,383 ms |
| Current brush ready | 2,100 ms |
| Full startup catalog ready | 22,989 ms |

The first row is **not** input readiness. Workspace ownership must finish too.
This distinction was confirmed with trusted tablet touch events: early Settings
taps were consumed until workspace adoption.

Wasm streaming download/instantiation took about 425–435 ms for the approximately
16.75 MB uncompressed development module over USB. One cached reload reduced
that interval to 44 ms and DOM construction to 297 ms, but workspace input still
waited until 22.79 seconds. Network transfer and Wasm initialization matter for
first load, but do not explain the long input lock or repeated display freezes.
The host also fetches 161 individual SVG icons before constructing the controls.

The first-canvas staging boundary has regressed since the September 10 record:
**22 pipelines** are now created before paper, versus the earlier four.
`raster/native_edit.rs::initialize_native` prepares transfer state through
`Scene::new` (which compiles both scene pipelines), then constructs native color,
scalar, promotion and publication-validation pipelines. Sixteen native compute
pipelines and the two scene pipelines are additional eager work. The initial
GPU validation scope alone took about 582 ms in the unprofiled baseline.

## Change in this checkout

Web startup now explicitly calls the existing shared **library-only** loader.
Explicit runtime package imports retain document migration, validation and
atomic publication. Startup library refresh also preserves programs embedded
in reopened documents, matching the shared library policy.

Three alternating pairs used the same rebuilt Wasm and resource URLs. The
legacy arm redirected only `load_filter_library` back to `load_filter_package`
at the browser boundary; the corrected arm ran normally. HTTP cache was
bypassed and normal browser GPU caches were retained.
Timing pairs were idle reloads; separate checks exercised input.

| Pair | Legacy workspace ready | Corrected workspace ready |
| --- | ---: | ---: |
| 1 | 23,133 ms | 1,973 ms |
| 2 | 23,338 ms | 2,026 ms |
| 3 | 24,219 ms | 1,992 ms |
| Median | **23,338 ms** | **1,992 ms** |

That removes **21.35 seconds / 91.5%** of the workspace input lock. Brush
readiness remains approximately 2.1 seconds. Full compilation still takes
approximately 23 seconds and GPU display stalls remain: this routing change
does not claim to fix GPU scheduling.

## Next rendering change, tested as an experiment

A temporary CDP preload substituted `createRenderPipelineAsync` and
`createComputePipelineAsync`, retained their promises within the validation
scopes, and substituted resolved handles at pipeline use sites. This is a
diagnostic prototype, **not production code**. It ran against the original
build, separately from the workspace fix, for three unprofiled reloads.

| Median metric, from DOM construction until full startup completion | Immediate pipelines | Async experiment |
| --- | ---: | ---: |
| Display callback interval, 95th percentile | 225 ms | 8.4 ms |
| Worst display callback interval per run | 575 ms | 92 ms |
| Intervals over 50 ms per run | 75 | 8 |
| First viewport GPU completion | 1,383 ms | 1,123 ms |
| Current brush ready | 2,100 ms | 1,945 ms |
| Full catalog ready | 22,989 ms | 19,596 ms |

All 187 pipeline creations remained; this did not suppress optional work.
The remaining long intervals include CPU-side work such as procedural texture
generation. Async compilation alone does not remove the workspace ownership
gate in the original code.

The next substantial improvement should expose true asynchronous WebGPU
pipeline creation through typed wgpu/deferred recipes, while preserving owned
futures, validation errors, required-work priority and the prohibition on using
unready handles. Merely awaiting an error scope around the immediate APIs or
moving JavaScript scheduling to a worker does not address the observed GPU
process stalls. The [WebGPU pipeline creation specification](https://www.w3.org/TR/webgpu/#pipeline-creation)
describes the distinction and recommends async creation to avoid blocking the
queue timeline.

Also restore the first-canvas boundary: defer native writeback/promotion/
validation until their first required document or brush stage, and prepare
transfer tables without constructing/compiling the entire scene. Loaded
document and painting dependencies must still gate their respective readiness.
Wasm delivery, icon bundling and workspace storage polling are secondary
optimization candidates after these GPU issues.

## Validation and reproduction

- Optimized Wasm build passed.
- Tablet staged-startup regression passed: visible paper before brush readiness;
  Settings activation with real mouse/touch/pen while required compilation is
  held; painting and camera input while optional compilation is held; and loaded
  document filters prepared before brush readiness.
- Shared library-warmup input/snapshot guard test passed.
- All 24 repeated Android-native touchscreen taps opened Settings, beginning
  before background compilation finished. This separate check used ADB input
  rather than CDP touch emulation.
- Shared startup-library embedded-program preservation test passed.
- JavaScript syntax checks and `git diff --check` passed.
- Desktop headless Chrome could not establish the pixel check: it returned a
  black presented canvas and logged an external-Instance error. The successful
  physical-tablet run is the rendering acceptance evidence for this change.

The focused regression's required-pipeline fixture now targets the current
Float32 brush path. The loaded-document fixture waits for workspace ownership
before inserting its filter. Run on a dedicated tablet origin:

```sh
LAYER_DEVICE_CDP=http://127.0.0.1:9237 \
  LAYER_WEB_URL=http://127.0.0.1:4187/ \
  node apps/layer-web/device.test.mjs --staged-startup
```

`artifacts/web-startup-2026-09-17/` retains raw probe JSON, a Chrome trace and CPU
profile, screenshots, probe/preload sources, build hashes, summary calculations
and validation logs. These generated artifacts are ignored by Git. The probe
follows command buffers containing `viewport presentation`, rather than
mistaking an upload-only submission for the first canvas.

These are local-development measurements with normal browser/driver caches,
not factory-cold GPU-cache or internet-hosting benchmarks. Display callback
intervals and queue completion do not measure physical scanout or end-to-end
input-to-photon latency. The async sample was sequential, not randomized; use
its large display-stall reduction as a mechanism check, not a precision claim
about total shader compilation speed.

## Implemented follow-ups

The follow-up to `7beb55b7` implements three changes:

- Storage request settlement schedules a coalesced workspace tick after the
  promise settles, including rejection and worker failure. The 100 ms timer
  remains responsible for leases, maintenance and delayed observation.
- The browser compiler uses typed asynchronous render/compute pipeline recipes.
  A narrow vendored wgpu addition shares descriptor conversion with the immediate
  API, starts the browser promise and installs rejection handling immediately,
  and returns a typed pipeline only on success. Futures own their GPU handles;
  renderer/session borrows and error-scope guards never cross an await.
  Up to four same-priority pipeline requests run together. Procedural texture
  generation and filter transactions retain separate task boundaries. Required
  document/brush work still precedes speculative work.
- Native writeback, promotion and publication validation now compile before
  brush readiness, instead of before paper. Preparing native transfer tables
  retains deferred scene pipelines. This restores **four pipelines before the
  first canvas**, down from 22. Native hosts retain their worker/cache path;
  standalone encoding constructors still prepare their pipelines eagerly.

The same owned async path covers loaded document filters and runtime catalog
validation. Failed candidates cannot replace the working catalog. Pipeline
promise failures and all three device error scopes are drained before completion
is published, with labels and browser error details preserved.

Three ordinary tablet reloads produced the following medians. The comparison
column is the earlier library-only fix, so it already excludes the original
23-second input lock.

| Metric | Library-only fix | Follow-up |
| --- | ---: | ---: |
| Workspace ready | 1,992 ms | **1,036 ms** |
| First canvas submission | 1,343 ms | **1,065 ms** |
| First viewport GPU completion | 1,404 ms | **1,177 ms** |
| Current brush ready | 2,095 ms | 2,173 ms |
| Full catalog ready | 23,379 ms | **19,199 ms** |
| Display callback interval, p95 | 242 ms | **8.5 ms** |
| Worst display interval per run | 592 ms | **83 ms** |
| Intervals over 50 ms per run | 75 | **2** |
| Pipelines created before canvas | 22 | **4** |

All 187 observed pipeline creations remain; 178 use the async APIs. The others
are paper/presentation and thumbnail pipelines. Brush readiness is approximately
unchanged (2.14–2.43 seconds in the three follow-up runs). The main gains are
earlier usable controls/paper and continuous display updates during compilation.
Full warmup ranged from 17.78 to 20.89 seconds. These samples were taken later in
the same device session, not as randomized before/after pairs; do not interpret
small timing differences as a precise regression or improvement. The earlier
three-pair storage-only experiment independently measured 2,005 ms to 899 ms.

An intermediate implementation awaited every native pipeline separately and
delayed brush readiness to roughly 3.14 seconds. Bounded async batching removed
that regression without batching CPU-heavy texture recipes. It also reduced
total warmup from that intermediate implementation's roughly 28.76 seconds.

Follow-up validation includes the physical-tablet staged-startup checks, required
pipeline promise and optional validation holds, actual painting/camera input,
loaded-filter ordering, injected compute rejection with canvas restart, and
injected render rejection with atomic catalog preservation and successful retry.
All 24 Android-native Settings taps also succeeded during an ordinary warmup run.
The native hardware checks cover startup ordering/teardown, native publication
readiness, painting/undo/save/reopen/device replacement, and runtime filter
validation/rejection. The storage transport test covers success, rejection,
worker failure, pending operations and explicit reconnect.
The physical-tablet workspace suite passed creation, rename, switching, preview
and cancel, history/undo, reload, layout and brush reset, and deletion. Its full
warmup waits now use the same 55-second allowance as the device harness. Run it
with `--workspace-manager` on a fresh dedicated origin: the suite assumes the
default header pins, and document recovery dialogs from painting fixtures can
intercept its menu contacts.

Raw timing runs, summaries, screenshots, build hashes and validation logs are in
`artifacts/web-startup-2026-09-17/implemented-optimizations/` (ignored by Git).
Run the storage transport test with
`node --test apps/layer-web/workspace-client.test.mjs`; use the tablet command
above for the expanded staged-startup suite.

Further shader optimization has lower priority now that background warmup no
longer repeatedly stalls the display. Full catalog readiness still takes about
19 seconds, but controls become usable around one second and the current brush
around 2.2 seconds. Keep those user-visible milestones as the targets. Before
changing module delivery or bundling the 161 icons, measure a production build
over a representative cold network connection: the USB development measurements
do not establish their production cost. A larger shader/cache redesign is not
justified by these remaining startup measurements alone.
