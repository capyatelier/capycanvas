# Runtime-defined filters

[Technical documentation](../README.md)

Implemented and validated on 2026-09-10. All forty built-ins and custom filters use
one runtime JSON/WGSL format and shared renderer. **Curves and Gradient Map are
the agreed exceptions:** their custom controls, interpolation and LUT generation
remain in Rust. Generic preparation, including Gaussian coefficients, is WGSL.
No shader-editor UI or general node graph was added.

## Definitions and ownership

`assets/filters/manifest.json` supplies runtime IDs, category/ordering, labels,
icons, parameters, constraints, ordered passes, sampling bounds, animation and
preview presets. WGSL modules live beside it. `BuiltinEffect`, built-in category
switches, Rust filter constructors and the Rust Gaussian algorithm are removed.
The same parser resolves external packages and the embedded startup fallback.

| Owner | Responsibility |
| --- | --- |
| `layer-core/effect_catalog.rs` | Parse/resolve packages, validate metadata and stage catalogs |
| `layer-core/effects.rs` | Generic parameter/layout validation; the two custom-editor exceptions |
| `layer-ui/filter_loading.rs` | Transactional publication, compatible values, catalog and preview revisions |
| `layer-render-wgpu/effect_validation.rs` | Namespace/interface checks and device compilation |
| `layer-render-wgpu/effects.rs`, `effect_preparation.rs` | Shared storage, compilation reuse, ordered preparation/render work |
| GTK, web, Android, Windows hosts | Obtain bytes and render the shared schema |

A package has `format: 1`, `categories` and `filters`. Each filter contains a
`program`, category, icon and optional preview overrides. A shader accepts inline
WGSL or an ordered array of manifest-local WGSL filenames. Modules resolve to
shared source chunks, so fused filters include common helpers once. Serialized
document programs contain resolved code and do not need their original package.

Installation modes are explicit:

- `add`: reject existing catalog IDs, including conflicting document-only IDs.
- `replace`: update existing IDs; reject missing IDs.
- `merge`: update existing IDs and admit new IDs. Startup resources use this,
  so editing their manifest can add a filter without rebuilding the executable.
  Omitted IDs remain available; this is not a catalog-deletion operation.

Duplicate IDs inside a package, conflicting WGSL declarations, invalid metadata,
ABI/interfaces and excessive resource requests are rejected. Module names cannot
escape their directory or select another origin. Limits include 1,024 filters,
64 categories, 256 modules, a 16 MiB manifest and 16 MiB aggregate module text.

## Transactional loading

`UiSession::load_effect_package` stages the candidate while keeping the current
catalog and document live. The renderer checks the combined namespace and all
changed preparation/render interfaces. Device error scopes are popped immediately
and polled without a blocking wait. Validation does not dispatch preparation or
allocate canvas image intermediates. Accepted compilation results are reused.

Publication waits until input, the active stroke and pending document edits are
settled. Live values are matched by parameter key, including edits made while
validation was pending. New/incompatible fields use defaults; conflicting joint
constraints reject the replacement rather than silently altering valid values.
A single document edit replaces affected live programs. Unrelated layers and
paint history remain unchanged. Invalid replacement retains the working state.

`UiState.filter_load` reports request ID, pending and error. Catalog revision
refreshes picker categories, rows and controls, and participates in preview
invalidation. GTK's idle scheduler follows the session's pending work, so shader
validation completes even when nothing is being drawn. Superseded compilation
versions are pruned at this cold publication boundary.

Compilation is cold work, not a 120 Hz operation. Authored WGSL is executable
content: bounded storage and interface validation do not prove termination,
safety from GPU watchdog resets, or a particular execution time.

## Windows file transport

Windows builds stage editable resources in `Assets/filters` beside the executable.
`CAPY_FILTERS_DIR` and `CAPY_FILTERS_MODE` select a startup library override. The
render-owner API `capy_load_filter_directory` accepts an optional directory,
installation mode and `library` flag; the default explicit load can migrate live
instances, while startup/library refresh preserves embedded document programs.
An owned background worker reads only manifest-approved flat module names before
passing bytes to the shared loader. `windows_filter_load` adds transport progress
and errors to the native snapshot. There is no filter-import or shader-editor UI.

The native UI fixture validates edited WGSL and metadata without rebuilding, live
values, picker previews and recovery from missing/invalid files. A separate
hardware D3D12 full-image test verifies atomic replacement/rejection and compatible
library refresh without altering the current document. These scoped checks do not
resolve the strict v4 reference discrepancy recorded below or establish performance.
See [Windows host commands](../../apps/layer-windows/README.md#runtime-filter-packages).

## Persistent GPU preparation

`EffectLookup` declares WGSL, entry, named parameter dependencies, fixed output
capacity and dispatch dimensions. Authored functions read declared inputs through
`prep_parameter(index, element)` and write through bounded
`prep_store(index, value)`. They may declare private/workgroup scratch, but not
extra resources, entry points, override constants or direct shared-buffer access.

Limits are eight tables per effect, 4,096 vec4 records per table, 256 invocations
per group and 256 groups. Device workgroup dimensions/storage limits are checked.
Generic Rust packs values and offsets; preparation WGSL owns the mathematics.

Gaussian Blur, Unsharp Mask, High Pass, Bloom, Soft Focus and Pencil share the
Gaussian preparation definition. It generates normalized, bilinear-paired taps;
consuming pixels perform table lookups, not coefficient calculations. The
standalone Tent Blur example demonstrates a different kernel using the same ABI.

Every pass of an effect chain shares one persistent parameter/table buffer.
Edits upload parameter prefixes, never GPU-owned tables. Preparation runs in the
existing scene encoder before consumers, with no extra submit, readback, blocking
wait or intermediate table copy. Pointwise prepared filters still fuse. Ordinary
warm cache lookup reuses its CPU key storage and updates cached time scalars in
place rather than constructing three temporary vectors per frame. Parameter
repacking uses small CPU temporaries; it does not allocate fresh GPU lookup storage.

A two-pass Unsharp Mask uses 624 parameter/table bytes versus 1,248 previously.
Large canvas/image caches are unchanged; slider edits do not grow lookup storage.

| Change | Preparation | Image work |
| --- | --- | --- |
| Painting or navigation | Reuse | Existing dirty-tile/halo rules; navigation reuses pixels |
| Unrelated parameter | Reuse | Affected filter output and dependents |
| Declared dependency | Once | Affected output and dependents |
| Render-only code | Reuse if preparation/layout is unchanged | Affected output |
| Preparation code/layout | Once | Affected output |
| Ordinary animation time | Reuse static tables | Time-dependent output |

Clipping input and static backdrop caches remain separate. Painting on a backdrop
updates its dirty tiles, not the entire cache or an unrelated frozen filter.
Neighborhood footprints propagate through multipass chains; global remapping
conservatively invalidates the required image. Masks, layer properties and blend
apply to final output, not independently to every intermediate pass.

## Use without rebuilding

- GTK checks `CAPY_FILTERS_DIR`, then `filters` beside the executable, then
  development `assets/filters`, with the embedded catalog as fallback.
  `CAPY_FILTERS_MODE` defaults to `merge`. For an isolated custom package:
  `CAPY_FILTERS_DIR=examples/filters/tent-blur CAPY_FILTERS_MODE=add
  ./target/debug/layer-linux`. The native transport also accepts directories
  for live add/replace/merge. There is no automatic file watcher.
- The native packager ships editable resources in
  `dist/capycanvas-linux/bin/filters`; editing those resources does not require
  rebuilding `bin/capycanvas`.
- Web ships fingerprinted JSON/WGSL in the PWA precache. Startup merges them;
  loading failure retains the fallback. Call
  `await layerApp.loadFilters('/my-filter/manifest.json', 'add')`,
  `'replace'` or `'merge'`, then inspect
  `layerApp.state().filter_load` for completion. Serve edited resources and call
  again—no Wasm rebuild. Requests revalidate HTTP caches; normal CORS applies.
- Android merges the same resources from APK assets after GPU attachment.
  `CanvasHost.loadFilters(manifest, modules, mode)` accepts acquired package
  bytes for live loading without rebuilding the native library. A file picker or
  download UI is outside this task.

See [the Tent Blur package](../../examples/filters/tent-blur/README.md) and
[web packaging](../development/web-packaging.md). All shaders added here are original code;
no third-party shader implementation was imported.

## Validation

Final checks pass: 59 GPU tests (six opt-in benchmarks excluded), 21 core tests,
23 engine tests, 145 UI tests, six Android bridge tests, 19 packaging/launcher
tests, Clippy and the Wasm build. Native and static web staging bundles build;
the native bundle's forty definitions and WGSL files match the source resources.

| Requirement | Evidence |
| --- | --- |
| Same format for all forty filters | Disk/bundled catalogs match; shared modules and self-contained document round trips |
| Runtime non-Gaussian algorithm | Tent Blur loaded on GTK, Chrome/WebGPU and Android; box-kernel replacement on web/Android |
| New IDs without rebuilding | Mixed existing/new catalog merge test; browser edits served JSON/WGSL with unchanged Wasm |
| Last working program | Invalid WGSL/resource access/namespace or ID collision rejected; image, values and catalog retained |
| Efficient preparation | Dependency-count tests: paint, pan, time, opacity and unrelated edits reuse; relevant/code edits run once |
| Reuse across passes/fusion | Stable storage size, shared multipass tables, prepared pointwise fusion, accepted pipeline reuse |
| Correct pixels | Original forty-filter reference × four scopes, at most one byte difference; Gaussian sigma 0–21 |
| Incremental correctness | Every filter and expensive chains match forced rebuild at tile/document boundaries; clipped backdrop edits remain local |
| Shared controls and rendering | All-filter UI suites on GTK/private Wayland, packaged Chrome and API-35 tablet emulator; runtime load/replacement tests on each |

Runtime screenshots are under `artifacts/ui/runtime-filters-{gtk,web,android}`;
the GTK, web and Android custom-filter results were visually inspected.
Forty-filter review captures are under `artifacts/ui/adjustments-{gtk,web}` and
the emulator's generated `Pictures/CapyCanvasValidation` directories.

## Performance

All triples below are **median / p95 / p99, milliseconds**. Timings measure
render preparation/encoding/submission and GPU execution, not end-to-end input
latency or final compositor presentation. GPU timing includes scheduling gaps.
Runs were sequential, not simultaneous GPU stress tests; individual runs do not
provide statistical confidence intervals or isolate clock/driver variation.

### Native workstation

Release, 2,048 × 1,536, 320 updates per case, 64 CPU warm-up samples discarded;
GPU values use the bounded telemetry window. The existing benchmark measures
relevant/unrelated edits and painting with the same operations before and after.

| Case | CPU before | CPU final | GPU before | GPU final |
| --- | --- | --- | --- | --- |
| Unsharp, paint | .066 / .146 / .306 | .057 / .086 / .177 | .034 / .035 / .036 | .033 / .035 / .035 |
| Unsharp, kernel edit | .041 / .057 / .456 | .033 / .064 / .257 | .137 / .190 / .193 | .146 / .199 / .201 |
| Unsharp, other edit | .043 / .053 / .107 | .031 / .080 / .112 | .068 / .069 / .069 | .068 / .069 / .070 |
| Five prepared, paint | .167 / .294 / .510 | .120 / .194 / .368 | .087 / .090 / .091 | .087 / .090 / .090 |
| Five prepared, kernel edit | .125 / .267 / .519 | .094 / .122 / .225 | .567 / .623 / .639 | .579 / .645 / .656 |
| Five prepared, other edit | .130 / .258 / .386 | .102 / .184 / .248 | .509 / .521 / .523 | .525 / .531 / .532 |

Five prepared filters are Pencil, Soft Focus, Bloom, Gaussian Blur and Unsharp.
Final completion p99 (including the benchmark-only GPU wait) is .571 ms painting,
.961 ms on kernel edits and .865 ms on unrelated edits: below the 8.33 ms budget.
Preparation is not free: relevant-edit GPU medians increased about .009–.012 ms.
The unrelated five-filter GPU median increased .016 ms despite no preparation
dispatch; these runs do not isolate driver/clock variance from storage effects.
The small Unsharp unrelated CPU tail increase is also retained above.

### Chrome/WebGPU

Same 2,048 × 1,536 canvas, 180 updates per case, last 120 render samples. The
forty single-filter tests plus baseline, three expensive-stack modes and four
preparation-edit cases all pass (48 cases). Five expensive filters are Motion
Blur, Gaussian Blur, Domain Warp, Painterly and Denoise.

| Case | CPU before | CPU final | GPU before | GPU final |
| --- | --- | --- | --- | --- |
| Baseline | .10 / .30 / .40 | .20 / .40 / .50 | .01 / .01 / .02 | .01 / .01 / .01 |
| Five, local paint | .20 / .50 / .50 | .40 / .60 / .70 | .10 / .23 / .36 | .07 / .11 / .15 |
| Five, full update | .30 / .50 / .50 | .30 / .50 / .70 | .49 / .52 / .59 | .48 / .50 / .51 |
| Five, animated | .20 / .40 / .50 | .20 / .50 / .50 | .35 / .37 / .39 | .34 / .34 / .35 |

Individual-filter maximum CPU p99 is .60 ms and GPU p99 .12 ms. CPU values are
modestly higher, including the no-filter baseline; the run does not prove the
cause. All cases remain within the render budget. Browser timer quantization
limits interpretation of these small CPU differences.

### Android emulator

API-35 tablet, x86_64 native library, host-backed GPU, 180+ submitted updates per
case, last 120 samples. Two final 48-case sweeps were retained, not cherry-picked.

| Case | CPU before | CPU final A | CPU final B | GPU before | GPU final A | GPU final B |
| --- | --- | --- | --- | --- | --- | --- |
| Baseline | .73 / .93 / 1.53 | .61 / 1.15 / 4.90 | .65 / .86 / 1.01 | .01 / .02 / .02 | .01 / .02 / .02 | .01 / .02 / .02 |
| Five, local paint | 2.97 / 4.24 / 8.62 | 2.92 / 4.75 / 6.19 | 2.62 / 3.43 / 5.51 | .17 / .27 / .30 | .15 / .21 / .22 | .14 / .19 / .20 |
| Five, full update | 3.10 / 3.59 / 3.82 | 3.38 / 6.33 / 13.03 | 2.67 / 3.63 / 5.72 | .55 / .65 / .66 | .59 / .61 / .62 | .59 / .64 / .66 |
| Five, animated | 1.24 / 1.59 / 1.73 | 1.32 / 3.05 / 3.95 | 1.29 / 1.69 / 2.17 | .39 / .39 / .40 | .40 / .41 / .42 | .41 / .41 / .42 |

The full-update CPU tail regression is real in these measurements, but varies
substantially on repeat; the baseline tail also varies without any filters.
No preparation runs for full-opacity edits or ordinary animation. These tests
do not establish whether the remaining CPU tails arise in the guest, host
scheduling or driver calls. **Reliable 120 Hz on Android is not established.**
GPU work remains small; physical-device profiling is still required.

### Additional parameter-edit coverage

These host cases are new, so no host-specific pre-migration baseline exists;
the native before/after comparison above covers continuous parameter editing.

| Host | Case | CPU | GPU |
| --- | --- | --- | --- |
| Web | Unsharp, kernel | .20 / .40 / .40 | .16 / .24 / .24 |
| Web | Unsharp, other | .20 / .40 / .40 | .07 / .08 / .09 |
| Web | Five prepared, kernel | .20 / .40 / .50 | .19 / .35 / .41 |
| Web | Five prepared, other | .20 / .40 / .40 | .09 / .17 / .21 |
| Android A | Unsharp, kernel | 1.05 / 1.87 / 2.84 | .17 / .25 / .27 |
| Android A | Unsharp, other | 1.26 / 1.71 / 7.84 | .16 / .17 / .18 |
| Android A | Five prepared, kernel | 1.08 / 2.46 / 7.07 | .27 / .38 / .49 |
| Android A | Five prepared, other | 1.01 / 3.13 / 4.86 | .37 / .55 / .57 |
| Android B | Unsharp, kernel | .97 / 1.29 / 1.86 | .18 / .24 / .25 |
| Android B | Unsharp, other | 1.31 / 1.77 / 2.46 | .25 / .26 / .27 |
| Android B | Five prepared, kernel | 1.07 / 1.72 / 7.05 | .29 / .41 / .56 |
| Android B | Five prepared, other | 1.10 / 7.61 / 12.41 | .38 / .44 / .45 |

Android B's unrelated-edit p99 also exceeds budget. It cannot be attributed to
GPU preparation, which is not dispatched in that case. No mobile 120 Hz claim
is made from either run.

Raw reports are generated and ignored under `artifacts/benchmarks`:
`filter-parameters-{before,final}.csv`, `filter-web{,-runtime}.json`, and
`filter-android{,-runtime,-runtime-repeat}.json`. Intermediate milestone
reports remain there too. Compare the unchanged renderer Stats fields across
web runs: the newer auxiliary `frame_cpu` timer additionally includes dispatch,
so that field is not directly comparable with its earlier definition.

Physical Android/iPad devices, Safari, other browsers, Windows and macOS were
not tested for this migration. Resource compilation and user-authored shaders
have no guaranteed latency. There is no CPU canvas fallback and no new rendering
simulation or alternate brush path.


## Import contract and reference reconciliation

Imported image bytes are decoded with the shared sRGB transfer curve, premultiplied,
and explicitly rounded before eight-bit linear paint storage. This is one GPU
initialization pass, not CPU conversion or an extra per-frame operation. Hardware
sRGB decode approximations and normalized-storage rounding otherwise move some
values across paint-byte boundaries. The independent 1,536-pixel ramp test checks
all encoded channel values at six alpha levels against double-precision
[standard sRGB conversion](https://www.w3.org/Graphics/Color/srgb.pdf).
Explicit storage rounding avoids relying on the implementation's preferred
[UNORM rounding](https://docs.vulkan.org/spec/latest/chapters/fundamentals.html#fundamentals-fixedfpconv).

The old v2 filter reference used the earlier hardware-decoding importer. Keeping
that reference after correcting import produced a maximum channel error of 99 on
the Vulkan validation host. Reverting import made v2 pass but failed the transfer
oracle. That is an input-contract mismatch, not a reason to restore the old import.

This has now been isolated using an independent pre-migration renderer:

| Comparison on Vulkan | Maximum channel error | Differing channels |
| --- | ---: | ---: |
| Original renderer vs original v2 reference | 0 | 0 of 1,966,080 |
| Original vs corrected unfiltered output | 13 | 6,129 of 393,216 |
| Old/current unfiltered output, both using corrected import | 0 | 0 of 393,216 |
| Old/current filters, both using corrected import | 0 | 0 of 1,966,080 |

The baseline is commit `7719e6b`, before GPU preparation and runtime-definition
migration. Only its asset texture format and import operation were changed.
Filter algorithms, Rust-generated Gaussian coefficients, previews, masks,
clipping, time and composition stayed unchanged. Its corrected output supplied
the then-current `runtime-filters-v3.png`; the current implementation did not
generate that expected image. See the [fixture provenance](../../crates/layer-render-wgpu/tests/fixtures/README.md)
for reproduction details. V2 and v3 are now retained in Git history, not as
additional active references; the later v4 correction is described below.

The test still requires every compared channel to differ by at most one byte.
It now independently checks the unfiltered input against the transfer curve
before comparing the forty filters in four scopes. Failure diagnostics include
the input, actual/reference sheets and a per-filter/scope report under ignored
`artifacts/performance/filter-reference/`. They do not overwrite the fixture.
The reference covers samples from full-resolution renders, not all source pixels;
separate analytic, incremental, tile-edge, mask, clipping and preparation tests
cover those behaviors. No production shader or execution path changed in this
reconciliation, so there is no new renderer performance cost.

The v3 reference passed on its original Vulkan validation host. Metal passes
the independent import oracle but fails v3: 22,616 sampled pixels exceed one
byte, across 148 of 160 cases, with maximum channel error 255. The pre-migration
Metal renderer with identical corrected imports produces exactly the same sheet
as the current Metal renderer. A separate Vulkan SwiftShader numerical run also
fails v3 (23,594 pixels above one byte, maximum 255); it differs from Metal at
6,531 pixels above one byte. This is not isolated to the Metal backend.

Two independent scalar checks now isolate opaque Curves/Exposure ramps and
Halftone's full ink/paper endpoints. They calculate expected pixels from input
bytes and public filter parameters using double-precision transfer functions,
curve interpolation and nearest eight-bit linear storage. Both checks pass on
Metal and SwiftShader. They cover neither the full spatial filter algorithms nor
partial-alpha composition. For example, the full-sheet Curves case at source
pixel (15, 157) has a stored input red of 1/255. The curve yields approximately
0.5581 linear byte units, rounding to 1 and exporting as encoded red 13 on both
backends; the v3 reference has red 0 at that sample. For full Halftone ink, the
declared green 0.07 similarly exports as 22 in the scalar and endpoint checks.
These isolate color/storage discrepancies; they do not explain every difference.
Explicitly rounding every filter output reduced but did not eliminate the full
Metal mismatch. Flooring made it worse. Neither experiment is a production change.

Run the independent checks with:

```sh
cargo test -p layer-render-wgpu scalar_color_oracles -- --test-threads=1
```

For numerical diagnostics only, renderer unit-test binaries accept CPU adapters
when `LAYER_TEST_SOFTWARE_GPU=numerical` is explicitly set. Select the intended
adapter with `WGPU_ADAPTER_NAME`; an unmatched name fails selection. On Apple,
`--features wgpu/vulkan-portability` enables Vulkan in that test build. A local
Vulkan loader and ICD must also be configured for the test process. This opt-in
prints a warning, is compiled out of production hosts, and must never supply
hardware performance evidence. Keep local loader paths and machine logs in
ignored artifacts. No runtime dependency or default backend selection changes.

The full strict v3 comparison remained a failing cross-backend gate. Its reference,
one-byte tolerance and all channels remain intact. Browser WebGPU has not yet
been checked against v3; no cross-backend pixel-parity claim is made.

### Windows D3D12 integration check

A serial hardware D3D12 run at the Windows integration of 007284c reports 110
passed, 2 failed and 17 ignored benchmark tests. The strict v3 sheet still fails
with maximum channel error 255. Its reference and one-byte tolerance are
unchanged.

The new pointwise scalar oracle also fails in Curves: for source
[113, 142, 121, 255], the output is [102, 152, 117, 255], while the independent
red-channel expectation is 104 (pre-storage linear byte 34.500680587002336).
The test stops at that first mismatch, so its Exposure loop was not evaluated
in this run. This is evidence of a D3D12 mismatch; the cause has not been
established. The independent imported ramp and Halftone endpoint tests pass.
Raw failure images and GPU logs remain under ignored local artifacts.
At that milestone the full strict v3 comparison remained a failing cross-backend
gate. No tolerance or channels were relaxed. Browser WebGPU was not checked.

### Explicit filter storage conversion

Running the new independent scalar tests on the original hardware Vulkan host
reproduces both failures: Curves loses a channel whose output is 0.5581 linear
byte units; Halftone's green endpoint exports as 13 instead of 22. Vulkan's
[fixed-point conversion rules](https://docs.vulkan.org/spec/latest/chapters/fundamentals.html#fundamentals-fixedfpconv)
permit either neighboring integer and only recommend nearest rounding. A
reference captured on one device cannot define that choice for every device.

The shared generated filter fragment now explicitly clamps and rounds its final
RGBA value to the nearest eight-bit linear value. Preview, image stages and
fused chains use this same wrapper. It adds no passes, resources, CPU readback,
preparation work or per-filter branches. Intermediate fused values and lookup
storage remain floating point. The existing render targets already impose this
bit depth; the change selects the intended conversion instead of truncating a
faint channel on some implementations.

Both scalar tests now pass on the hardware Vulkan host and Mesa's software
Vulkan backend. The tone test also covers six alpha levels (255, 192, 127, 64,
1, 0), calculating premultiplication and storage independently from input bytes.
Software results are numerical checks only, never performance evidence.

The pre-migration renderer was independently rerun with just the corrected import
and explicit filter-storage boundaries. Its original wrapper and Rust filter
math were hash-checked against commit `7719e6b0ffa69e9aca1bf19acfedef9584d2fdad`
before the boundary edit. This produces v4: current/old output differs in only
four of 1,966,080 channels, each by one byte. The reference is still generated
by the old renderer, never today's comparison test; its unchanged tolerance
still tests all forty filters and four scopes. V3 is retained in Git history.
See [reference provenance](../../crates/layer-render-wgpu/tests/fixtures/README.md).

This does **not** solve complete cross-backend parity. With explicit rounding,
hardware Vulkan and Mesa software Vulkan still differ above one byte in 23,114
sampled channels (maximum 188); their unfiltered exports differ by at most one.
Spatial sampling, intermediate composition and numerical filter math need
further isolation. Browser WebGPU has not run v4. Do not treat the passing
scalar or same-backend migration checks as complete backend acceptance.

The integrated Metal suite executes 112 checks: 111 pass and the strict v4
reference fails; 17 hardware benchmarks remain separately ignored. Both scalar
color checks pass, including all six tone-ramp alpha levels. The full sheet has
3,461 channels above one byte across 3,274 sampled pixels and 70 of 160 cases,
with maximum channel error 47. This is substantially narrower than the v3
discrepancy, but remains a failing gate with every channel and the one-byte
tolerance intact. Pixel Mosaic accounts for 838 failing pixels across its two
unmasked scopes; Ripple supplies the maximum error. These counts identify
useful isolation cases, not a conclusion about their cause. The complete Metal
sheet was visually inspected and the full per-case report retained locally.

The hardware performance comparison uses the same release test and 2048×1536
artwork, 64 warmup plus 256 measured edits, in three alternating implicit/rounded
pairs. Values below are the median of the three runs' median/p95/p99 values,
not percentiles pooled from raw samples. The five-filter case is Pencil, Soft
Focus, Bloom, Gaussian Blur and Unsharp Mask. Relevant edits change preparation
inputs; unrelated edits change only a consuming-pass parameter.

| Workload | CPU before ms | CPU rounded ms | GPU before ms | GPU rounded ms |
| --- | --- | --- | --- | --- |
| Unsharp, paint | .058/.077/.175 | .057/.072/.135 | .034/.034/.034 | .034/.034/.034 |
| Unsharp, relevant edit | .035/.052/.262 | .034/.058/.173 | .145/.202/.204 | .145/.202/.204 |
| Unsharp, unrelated edit | .031/.036/.110 | .031/.053/.119 | .069/.069/.069 | .070/.070/.070 |
| Five, paint | .116/.162/.217 | .115/.160/.257 | .087/.088/.088 | .087/.087/.087 |
| Five, relevant edit | .094/.179/.303 | .106/.186/.209 | .576/.642/.652 | .581/.643/.667 |
| Five, unrelated edit | .098/.185/.335 | .092/.171/.231 | .531/.540/.542 | .544/.553/.556 |

The measurable full-image GPU increase is about 0.014ms median for the last
case, approximately 2.6%; small dirty-region painting does not show that cost.
CPU tails vary in both directions: five-filter paint p99 increased by 0.041ms
in the paired summary, whereas the editing tails decreased. No CPU speedup or
zero-cost claim follows. Rounded GPU p99 remains below 0.681ms in every paired
run and explicit-wait completion p99 below 0.953ms. This is filter throughput,
not compositor/physical-input latency. The earlier exploratory runs had wider
CPU tails and are not silently pooled into these controlled pairs.

With v4, the complete hardware Vulkan suite passes 112 tests with zero failures
or exclusions; 17 hardware benchmarks remain separately ignored. This includes
incremental/full equivalence, clipping, masks, preview crops, preparation
invalidation/reuse, fusion, animation and the independent color checks. The v4
sheet was visually inspected. The software alpha-oracle repeat also passes.

### Windows D3D12 storage-conversion check

The Windows integration through upstream 75c72f8 runs the complete renderer
suite serially on hardware D3D12: 113 pass, one fails and 17 performance tests
remain separately ignored. Curves and Exposure now pass the independent scalar
oracle across all six alpha levels. The imported ramp across every alpha value,
Halftone endpoints and both new watercolor regressions also pass.

The strict v4 sheet still fails, with maximum channel error 30. The one-byte
tolerance and all channels remain unchanged. This run does not establish the
cause or full cross-backend parity; the remaining discrepancy needs independent
isolation. Windows did not regenerate the reference or adjust expectations.
Raw output and diagnostic images stay in ignored local artifacts.

### Spatial isolation and long-running Ripple

An independent full-resolution oracle now checks Pixel Mosaic and Ripple in
premultiplied linear storage, before export/unpremultiplication. It derives
imported bytes, sampling positions and bilinear interpolation in double precision
from the original synthetic artwork. Ten cases include odd/even mosaic cells,
transparent edges, multiple Ripple phases, maximum amplitude, minimum wavelength
and the maximum frozen time. This supplements the unchanged strict PNG gate;
it does not discard transparent channels or relax that gate's tolerance.

The oracle found a separate Ripple bug: subtracting a large time phase before
range reduction erased spatial precision. At 3600 seconds, 48px amplitude and
8px wavelength, 1,618 pixels differed from the linear oracle by more than one
byte, with maximum error 14 on hardware Vulkan. The WGSL now reduces the
spatial and temporal phases separately before subtraction. Its sine argument
also stays within the interval with a specified
[WGSL accuracy bound](https://www.w3.org/TR/WGSL/#floating-point-accuracy).
No Rust filter mathematics, preparation, texture, pass or readback was added to
production rendering; the existing test-only texture reader inspects the result.

All ten cases now pass on hardware Vulkan and Mesa software Vulkan within one
linear byte, and equal phases separated by 1,799 periods produce identical
stored pixels. The unchanged v4 reference also passes on hardware Vulkan. This
does not explain or resolve every Metal/D3D12 sheet difference; those backends
have not yet run this new oracle or correction. Software checks are numerical
evidence only, never hardware performance measurements.

The release benchmark can select a single filter by its catalog label with
`CAPY_FILTER_BENCHMARK_FILTER=Ripple`. Three alternating before/after pairs use
the existing 4096×4096 benchmark: 24 warmup and 96 measured CPU/completion
samples per workload; GPU telemetry retains the whole 120-frame run. Table
values are the median of each run's median/p95/p99, not pooled percentiles.

| Workload | CPU before ms | CPU after ms | GPU before ms | GPU after ms |
| --- | --- | --- | --- | --- |
| Full recomposition | .629/.972/3.240 | .587/.924/1.322 | .255/.256/.275 | .260/.262/.281 |
| Small dirty-region paint | .059/.072/.178 | .058/.067/.071 | .029/.030/.034 | .029/.030/.034 |
| Broad paint | 1.586/2.934/5.877 | 1.651/3.053/5.881 | .814/.834/.848 | .817/.830/.839 |
| Animation | .029/.040/2.097 | .030/.044/2.116 | .120/.121/.122 | .125/.126/.126 |
| Cached | .013/.016/.022 | .011/.014/.017 | .005/.005/.005 | .005/.005/.005 |

Animation's GPU median increases by about 0.005ms (4%); full recomposition by
about 0.006ms (2%). There is no host-path change and CPU tails vary both ways,
so these measurements do not establish a CPU speedup or zero overhead. Broad
paint's explicit-wait completion p99 is 5.890ms after versus 5.889ms before.
These are isolated renderer timings, not physical input-to-display latency.
Raw logs and paired CSVs remain in ignored local storage.

The complete hardware Vulkan renderer suite passes 118 tests, with 17 benchmarks
separately ignored. Strict renderer Clippy, the WebAssembly compilation check
and the GTK release build pass. No new native Metal, D3D12, Android or browser
device validation is claimed for this correction.
