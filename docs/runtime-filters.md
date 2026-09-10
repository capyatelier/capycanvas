# Runtime-defined filters

Follow-up to the completed forty-filter milestone (`7719e6b`). This document
records the implementation contract, not a claim that the migration is complete.

## Required outcome

All built-ins and custom filters load through one definition format: WGSL plus
declarative parameters, constraints, sampling bounds, ordered image passes,
animation, category/icon metadata and preview presets. Adding a filter or changing
its generic preparation math must require neither Rust changes nor an application
rebuild. Curves and Gradient Map are explicit exceptions: their custom controls,
interpolation and LUT generation may remain in Rust. No shader-editor UI or general
node graph is part of this task.

Remove `BuiltinEffect`/category switches and filter constructors from registration,
preview generation and host views. Replace Gaussian coefficient generation with
definition-owned WGSL preparation; do not migrate the two custom-editor exceptions.
Retain generic Rust validation, packing, dependency scheduling and native controls.

## Data and loading

- Runtime identifiers, not enum discriminants, identify programs and categories.
  The catalog supplies ordering, labels, icons and preview overrides.
- Ship editable WGSL and declarative metadata as resources. A portable package
  contains these same resources for native assets, static web hosting and custom
  loading. Resource packaging is not recompilation of the application.
- The shared core accepts package data; platform hosts only obtain bytes from
  files, web assets or Android assets. Provide a programmatic load/reload boundary
  that tests can exercise after startup. No custom-only registration route.
- Explicit add versus replace semantics. Reject duplicate IDs, incompatible ABI,
  malformed metadata, conflicting shader declarations and over-limit resources.
  Shared identical shader modules may be reused; conflicting definitions must
  never silently override one another.
- Stage replacements and validate their preparation/render interfaces before
  publishing them. Retain the last working catalog/program and instances on
  failure. Successful replacements preserve compatible values by parameter key
  and invalidate affected renderer dependencies and preview rows.

## GPU preparation

Each optional preparation declaration specifies its WGSL entry, bounded output
storage, workgroup dimensions and parameter dependencies. Generic parameters have
a bounded GPU layout. Preparation code owns normalization, kernel choice and other
mathematical policy. Curve/gradient LUTs retain their existing Rust packing.

Keep prepared output in persistent renderer-owned storage and share it across
image passes and fused consumers. Render shaders retain constant-time table
lookup/interpolation; do not replace LUT access with per-pixel curve/kernel math.
Packing raw parameters and offsets is Rust work, not preprocessing an algorithm.

Preparation dirtiness is separate from image dirtiness:

| Change | Preparation | Images |
| --- | --- | --- |
| Paint or canvas navigation | Reuse | Existing tiled/halo rules |
| Unrelated parameter | Reuse | Only affected filter output/dependents |
| Declared parameter dependency | Once | Filter output/dependents |
| Render-only code | Reuse if preparation/layout unchanged | Affected output |
| Preparation code/layout | Once | Affected output |
| Ordinary animation time | Reuse static tables | Existing time-aware outputs |

Compute writes precede consuming render passes in GPU command order. There must
be no rendering dependency on CPU readback of prepared tables, blocking GPU waits,
or allocations of fresh preparation storage on ordinary frames/parameter edits.
Fixed-capacity outputs prevent parameter changes from reallocating buffers.
Storage limits, bindings, dispatch sizes and checked offset arithmetic are generic
validation responsibilities. A bounded buffer is not proof arbitrary shader code
is cheap or terminating; do not promise sandboxed timing for arbitrary programs.

GTK curve plots, shared curve plot data and gradient-stop insertion may continue
using Rust interpolation. This is a deliberate scope exception, not an alternate
implementation of generic preparation. Adding a generic kernel must never require
a new Rust algorithm variant or a filter-specific renderer branch.

## Validation and milestones

1. Preserve pre-migration pixel references and CPU/GPU median/p95/p99 baselines,
   including continuous relevant/unrelated parameter editing and expensive stacks.
2. Load the forty built-ins from data through the runtime registry. Exercise
   adding a definition and replacing one without rebuilding, including invalid
   replacement rollback and collisions.
3. Introduce bounded WGSL preparation and migrate Gaussian; remove its superseded
   Rust algorithm. Demonstrate a new non-Gaussian kernel using only data/WGSL.
4. Check preparation counts: paint, pan, time and unrelated edits reuse; relevant
   edits execute once; all passes consume the shared result. Preserve masks,
   clipping chains, transparency, image halos and dirty-tile backdrop caching.
5. Re-run before/after rendering and parameter-edit benchmarks. Report any
   regression, including dispatch cost, not just per-pixel shader throughput.
6. Validate controls and rendering in GTK, web and the available Android emulator;
   explicitly identify untested physical devices. Update packaging, docs and
   notices, remove migration helpers/old paths, commit and push tested milestones.

GPU/host validation builds on the existing Naga parser and validator. Device error scopes
return an asynchronous result; popping the scope does not require waiting before
unrelated work continues. See [wgpu error scopes](https://wgpu.rs/doc/wgpu/struct.ErrorScopeGuard.html).

## GPU preparation milestone (2026-09-10)

Implemented the generic preparation ABI and removed Rust Gaussian coefficient
generation. The six Gaussian consumers (Blur, Unsharp Mask, High Pass, Bloom,
Soft Focus and Pencil) use the same WGSL preparation definition. Curves and
Gradient Map retain their Rust interpolation and controls, as requested.

`EffectLookup` declares WGSL, entry function, named dependencies, a fixed vec4
capacity and dispatch dimensions. The wrapper exposes `prep_parameter(index,
element)` for declared inputs and bounded `prep_store(index, value)`. Libraries
may use private/workgroup scratch; additional bindings and entry points are
rejected. Metadata bounds tables to 4,096 records, eight tables per effect and
at most 256 invocations per group / 256 groups. Device limits are checked too.
These are resource bounds, not a guarantee arbitrary authored code terminates.

All image passes share one persistent parameter/table buffer. Edits upload only
the parameter prefix, not GPU-owned output. Queued compute runs in the existing
scene command encoder before consuming draws, with no extra submit, readback,
wait or intermediate table copy. Pointwise filters with preparation still fuse.
A two-pass Unsharp Mask uses 624 parameter/table bytes rather than 1,248; its
large image caches are unchanged. Lookup storage does not grow on slider edits.

Checks cover the pre-migration forty-filter pixel reference (four scope variants
each, maximum one-byte difference), normalization at sigma 0 through 21,
pointwise fusion, and live replacement with an original triangular kernel read
from a WGSL file. Preparation counts prove that paint, pan/zoom, animation,
opacity and unrelated parameter edits reuse tables; relevant edits and
preparation-code changes each dispatch once. Render-only code preserves tables.
Invalid WGSL, extra bindings and attempts to access raw shared storage are
rejected without replacing the working GPU state. Native GPU tests pass, as do
the shared core/engine/UI/Android bridge tests and Clippy. GTK's private-Wayland
forty-filter review, the packaged Chrome/WebGPU forty-filter test and Android's
API-35 emulator forty-filter schema/render test all pass. The static web package
was rebuilt successfully. Physical Android/iPad hardware and other browsers have
not been tested for this migration.

### Workstation comparison

Release mode, 2,048 × 1,536, 320 updates per case; discard 64 CPU warm-up samples.
GPU values are the renderer's bounded telemetry window. Each triple below is
median / p95 / p99 in milliseconds. Parameter edits send the same composition
invalidation as the application. Completion includes the benchmark's GPU wait;
the application does not wait this way. These are individual runs, not confidence
intervals; CPU tail differences should not be interpreted as proven speedups.

| Case | CPU before | CPU GPU-prepared | GPU before | GPU GPU-prepared |
| --- | --- | --- | --- | --- |
| Unsharp, paint | .066 / .146 / .306 | .058 / .075 / .124 | .034 / .035 / .036 | .034 / .035 / .036 |
| Unsharp, kernel edit | .041 / .057 / .456 | .039 / .051 / .278 | .137 / .190 / .193 | .147 / .200 / .206 |
| Unsharp, other edit | .043 / .053 / .107 | .033 / .058 / .164 | .068 / .069 / .069 | .069 / .070 / .070 |
| Five prepared, paint | .167 / .294 / .510 | .116 / .146 / .215 | .087 / .090 / .091 | .087 / .090 / .090 |
| Five prepared, kernel edit | .125 / .267 / .519 | .124 / .214 / .423 | .567 / .623 / .639 | .578 / .640 / .653 |
| Five prepared, other edit | .130 / .258 / .386 | .131 / .251 / .530 | .509 / .521 / .523 | .513 / .525 / .527 |

Preparation is not free: relevant edits add approximately .010–.016 ms GPU time
in these runs. Painting and unrelated edits have no preparation dispatch.
The five-filter completion p99 is .457 ms painting, 1.114 ms on kernel edits,
and 1.114 ms on unrelated edits, below the 8.33 ms render budget. The latter CPU
p99 increased in this run; it is reported rather than hidden. Raw results:
`artifacts/benchmarks/filter-parameters-{before,gpu}.csv` (generated, ignored).

## Manifest and runtime-ID milestone (2026-09-10)

All forty definitions, defaults, constraints, passes, sampling bounds, animation
controls, categories and preview overrides now live in
`assets/filters/manifest.json`. The WGSL libraries are beside it. `BuiltinEffect`,
the category enum and all filter-specific Rust constructors have been removed.
Both the bundled catalog and external packages use `EffectPackage::parse` and
`resolve`; the host supplies module contents. `EffectCatalog::stage` has explicit
add/replace semantics and leaves its input catalog untouched on failure.

The manifest's `program.wgsl` and lookup `wgsl` accept an inline code string or
an ordered array of manifest-local WGSL filenames. Resolved modules share `Arc`
storage and remain separate for fusion: common helper modules are included once,
not concatenated into every filter's private source. Serialized document programs
retain resolved code, so opening a project does not require its original package.
Module names cannot escape the package directory or select another web origin.

The picker, insertion actions, category choices and GTK caches use runtime string
IDs. Preview requests contain resolved programs/presets; the renderer no longer
constructs a forty-filter catalog or indexes programs by enum discriminants.
Changing a requested definition invalidates that preview row, without rebuilding
the document source capture just because the filter program changed.

`examples/filters/tent-blur` is a standalone manifest and two original WGSL files,
including its non-Gaussian preparation. The GPU test loads those files at runtime,
renders the new filter, and verifies radius-zero identity. Tests also load all
forty definitions from disk, compare them with the bundled resource catalog,
check shared module storage and document round trips, and reject ID/category
collisions, invalid ABI/metadata, missing modules and path traversal. The original
forty-filter pixel reference still matches; the full GPU suite passes (57 tests,
six opt-in benchmarks excluded).
GTK's forty-filter private-Wayland review, packaged Chrome/WebGPU test and
Android API-35 emulator test pass with the runtime-ID picker and preview path.
The shared core (19), UI (142), engine (23) and Android bridge (6) tests pass.

Repeating the same workstation benchmark after this catalog migration gives:

| Five prepared filters | CPU median / p95 / p99 | GPU median / p95 / p99 | Completion p99 |
| --- | --- | --- | --- |
| Painting | .114 / .142 / .156 | .088 / .092 / .092 | .382 |
| Kernel edits | .110 / .178 / .415 | .574 / .637 / .650 | 1.044 |
| Unrelated edits | .088 / .149 / .348 | .524 / .528 / .528 | .912 |

Milliseconds, same 2,048 × 1,536 cases and sampling as above. No budget regression
appears in this run, but small GPU/CPU differences remain visible in the raw data;
this is not a claim of statistically identical performance. Results are in
`artifacts/benchmarks/filter-parameters-catalog.csv` (generated, ignored).

## Validated host loading milestone (2026-09-10)

`UiSession::load_effect_package` stages a catalog, requests GPU validation and
keeps the working catalog/document active. The renderer checks the combined WGSL
namespace, metadata, preparation and render interfaces; device error scopes are
popped immediately and polled without waiting. Compilation is cold work, not a
120 Hz operation. Validation neither dispatches preparation nor allocates canvas
image intermediates. Accepted pipelines are reused by subsequent rendering;
superseded compilation versions are pruned at this cold boundary.

Publication waits for pending input, the active stroke and pending document edits
to finish. Compatible current values are matched by parameter key, including
edits made while validation was pending. New/incompatible fields use defaults;
conflicting joint constraints reject the replacement rather than silently
clamping otherwise valid user values. One document edit replaces affected live
programs. Unrelated layers are untouched, paint is not replayed, and the existing
tiled composition dependency rules still apply.

`UiState.filter_load` reports the request ID, pending flag and error.
`filter_catalog_revision` refreshes category/row metadata and is part of the
preview revision. Controls continue to come from the shared schema. GTK's idle
scheduler now follows the session's background-work needs, not just animation
and drawing; validation completes on an otherwise idle canvas.

### Loading without rebuilding

- GTK loads `CAPY_FILTERS_DIR`, an installed `filters` directory beside the
  executable, or development `assets/filters`, with the embedded same-format
  catalog as fallback. `CAPY_FILTERS_MODE` is `replace` by default; use `add` for
  a new package. For example, from the repository root, run an already-built
  executable with `CAPY_FILTERS_DIR=examples/filters/tent-blur
  CAPY_FILTERS_MODE=add ./target/debug/layer-linux`. The native transport function
  also accepts directories for live add/replace; there is no shader-editor UI.
- Web ships separately fingerprinted JSON/WGSL, included in the PWA precache.
  Startup fetches these resources; failed loading retains the working fallback.
  `await layerApp.loadFilters('/my-filter/manifest.json', 'add')` imports a
  package, and `'replace'` reloads existing IDs. This returns a request ID;
  inspect `layerApp.state().filter_load` for completion. Serve edited external
  manifest/WGSL files and call again—no Wasm rebuild is involved. Resource
  requests revalidate HTTP caches. Cross-origin packages require normal CORS.
- Android ships the same resource directory as APK assets, loaded through the
  Rust API after the GPU attaches. `CanvasHost.loadFilters(manifest, modules,
  mode)` accepts acquired package bytes for live import/replacement, with no
  native-library rebuild. The host only transports bytes; Rust owns validation,
  publication, constraints, selection and control metadata. A file-picker or
  download UI is not part of this task.

The generic resource reader validates manifest-local module filenames. Treat
authored WGSL as trusted executable content: resource/interface checks are not a
proof a shader terminates or meets a frame budget. Device watchdogs still apply.

### Current validation

The full GPU suite passes: 59 tests and six opt-in benchmarks excluded, including
the original forty-filter pixel reference, transparency, clipping, masks, dirty
tiles, fusion and preparation dependencies. Shared core (20), UI (144), engine
(23) and Android bridge (6) tests pass. Packaging/launcher tests pass (19).

Live loading tests pass on GTK/private Wayland, packaged Chrome/WebGPU and the
Android API-35 tablet emulator. They load the forty definitions and add Tent
Blur from its standalone package, expose its shared controls, and preserve
values across replacement. The web and Android tests also replace its triangular
kernel with a box kernel and reject invalid preparation WGSL without publishing
it. Renderer tests verify unchanged pixels/preparation after rejection, pipeline
reuse after acceptance and render-only edits retaining existing lookup storage.

Still required before closing the overall goal: final all-filter platform
regression runs, before/after platform timing checks and the concluding audit.
Curves and Gradient Map remain the agreed Rust exceptions. Physical mobile
hardware and browsers other than the available Chrome have not been tested.

The same workstation parameter benchmark after transactional loading produces:

| Case | CPU median / p95 / p99 | GPU median / p95 / p99 | Completion p99 |
| --- | --- | --- | --- |
| Unsharp, painting | .056 / .073 / .164 | .034 / .035 / .039 | .306 |
| Unsharp, kernel edits | .033 / .047 / .153 | .146 / .198 / .200 | .499 |
| Unsharp, unrelated edits | .030 / .031 / .033 | .068 / .069 / .069 | .313 |
| Five prepared, painting | .113 / .195 / .348 | .087 / .095 / .095 | .501 |
| Five prepared, kernel edits | .112 / .201 / .407 | .571 / .627 / .641 | 1.033 |
| Five prepared, unrelated edits | .088 / .111 / .168 | .525 / .528 / .530 | .845 |

Milliseconds; same cases/sampling as the preserved baseline above. Small GPU
increases remain measurable: Unsharp kernel-edit median is about .009 ms higher
than before GPU preparation, and the five-filter unrelated-edit median is about
.016 ms higher. No preparation dispatch occurs for unrelated edits, so that
difference cannot be attributed to dispatch cost. These individual runs do not
isolate driver/clock variance from storage-layout effects. All measured cases
remain below the render budget. Raw output is
`artifacts/benchmarks/filter-parameters-runtime.csv` (generated, ignored).
