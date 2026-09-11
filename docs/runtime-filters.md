# Runtime-defined filters

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
| GTK, web, Android hosts | Obtain bytes and render the shared schema |

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

See [the Tent Blur package](../examples/filters/tent-blur/README.md) and
[web packaging](web-packaging.md). All shaders added here are original code;
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


## Cross-backend color evidence

The forty-filter PNG reference remains a strict check with a one-byte channel
threshold. Failure now writes the actual/reference contact sheets and a per-filter,
per-mask-scope TSV report under ignored `artifacts/performance/filter-reference`.
These diagnostics do not replace or regenerate the reference.

On Metal, that saved reference also fails against its original `3f6d2d5`
implementation, with a maximum channel error of 255. With identical explicit
sRGB import decoding, the current implementation matches the original output
exactly across all 160 filter/scope cases: zero differing channels. This isolates
the remaining saved-reference discrepancy from subsequent filter migration and
transform work; it does not establish cross-backend pixel parity or authorize a
looser tolerance. The saved-reference test remains failing on the tested Metal
hardware. Some large raw-channel errors occur near transparent blur edges;
opaque color differences are also present and remain visible in the report.

Imported image bytes now use the shared WGSL sRGB transfer curve before being
stored as premultiplied linear paint. Initialization reads exact source texels
and performs the conversion once on the GPU. Existing sampling for transforms
and effects continues to operate on the resulting linear paint. This avoids
backend differences in hardware sRGB decoding moving a value across the
8-bit linear storage boundary. The unmodified Metal path mapped an opaque
encoded channel of 97 to exported 98 (the reference calculation gives 96), and
100 to 101 (reference 99). The new path passes a test of all 256 encoded values
in each RGB channel at six alpha levels, checked against the standard transfer
curve and existing linear storage quantization. No canvas readback or CPU
conversion is added to drawing or import.

The transfer reference is the
[W3C sRGB specification](https://www.w3.org/Graphics/Color/srgb.pdf).
