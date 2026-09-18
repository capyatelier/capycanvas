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
