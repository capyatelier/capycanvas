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

GPU/host validation builds on the existing Naga parser/validator and
[WGSL writer](https://wgpu.rs/doc/naga/back/wgsl/index.html). Device error scopes
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

### Remaining migration

This milestone is **not** full runtime catalog loading. Built-in registration,
metadata constructors and packaging still need conversion to editable resources
and runtime IDs. Candidate program publication/rollback, naming collisions,
runtime UI/catalog updates and final cross-platform performance validation remain
part of the active task. The triangular-kernel test demonstrates the renderer
boundary, not a finished user-facing file loader.
