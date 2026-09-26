# Shared shader readiness

Android, GTK, Web and Windows now use the same demand-driven dependency tracking
and input admission policy in `layer-render-wgpu`. This extends the
[Web refresh work](web-refresh-responsiveness.md). Apple retains legacy warmup
until its host opts in and completes device validation.

## What changes

Startup prepares paper/presentation, the actual document, then the selected
brush and physical eraser. Unused brushes, procedural masks, region tools and
filter programs retain recipes; adding one no longer adds a startup compile.
Visible previews request their own variants later. Already compiled variants
are reused for subsequent selections and document effects.

The existing native compiler thread and browser task runner share
`shader_admission.rs`: optional work waits for 200 ms without input and an idle
session. Strokes, held gestures, queued input, pending document edits and
settings hold that gate closed. Native workers sleep on their existing
condition variable; browsers use a host timer. Window input observers forward
activity without changing gesture routing or drag conventions. Required
canvas/brush dependencies and explicit package validation retain priority and
can progress while input is arriving. An in-flight driver call cannot be
interrupted; this is admission between jobs, not preemption.

Readiness continues after initial startup. Shared `UiSession` wakes the host
when a UI-only brush change needs preparation. Native readiness snapshots use
the current dependencies, so a previous tool's ready flag cannot authorize the
new tool. Contacts begun before readiness remain suppressed through release.
`ShaderDocument` memoizes the dependency-relevant document state: ordinary
raster/parameter edits do not invalidate readiness or trigger effect preparation.
Effect-chain inspection now borrows layers; only compiler jobs clone their
owned inputs.

Android and GTK no longer reload their already embedded filter package during
startup. Android also stops packaging that redundant asset copy. GTK still
accepts explicit `CAPY_FILTERS_DIR` / `CAPY_FILTERS_MODE` overrides; runtime
package installation and atomic validation remain available. Identical library
refreshes are no-ops on all three hosts. Existing native pipeline caches remain;
there is no additional platform cache or compiler worker. Current native cache
saving still closes the initial cache after required startup work; this change
does not add persistence for later first-use variants.

Net code growth supports lifetime dependency identity, the common admission
policy, host input bridges and regression coverage. Superseded catalog loading
and the separate browser quiet-time policy are removed. Legacy eager warming
remains solely for hosts that have not opted in.

## Measurements, 2026-09-25

Optimized `dev-perf`; Android also uses R8. Baseline is `741a7f60`.
[Compact results and methodology](measurements/shared-shaders-2026-09-25.json)
retain individual runs and artifact hashes. All device work used a separate
benchmark application/origin; production drawings were not used.

| Host and cache condition | Baseline shader completion | Shared framework |
| --- | ---: | ---: |
| Huion KP1202 / Android 16, first launch after cache-generation change | 27.73 s | 3.50 s |
| Same Huion, two launches reusing caches, median | 4.57 s | 1.02 s |
| GTK / NVIDIA RTX PRO 6000, three empty test-cache runs, median | 8.70 s | 2.15 s |
| Same GTK, immediate warm repeats, median | 0.85 s | 0.87 s |

Completion now means all requested work, rather than compiling the unused
catalog. Cold GTK completion coincides with brush readiness. Its saved pipeline
cache falls from approximately 3.91 MB to 0.81 MB. Warm GTK startup is essentially
unchanged. Empty GTK test caches include driver/compositor caches under
`XDG_CACHE_HOME`; they are not an isolated application-cache measurement.
Separate runs with warm driver caches and an invalidated application cache also
complete near one second. Cold GTK event-loop slices can still approach one
second during initial graphics setup; this removes the catalog tail, not every
startup hitch.

Huion workspace readiness improves from 10.34 to 2.34 s on the first measured
launch, and from 4.06 to 1.80 s with reused caches. Initial UI draw stays near
0.6 s. Settings' first observed draw improves from 335 to 215 ms on the first
launch; steady settings remain roughly 60–85 ms. The probes now reach the UI
earlier in GPU startup, so their overlap with compilation differs.

First-use native pencil/watercolor preparation on Huion takes 1.15/1.51 s;
cached selections take 0.17/0.20 s, including UI publication. Those costs are
paid when requested, with readiness gating, instead of during every launch.
These tests verify drawing/history, not physical pen-to-scanout latency.

Huion Web, three warm refreshes: UI 506 ms, workspace 713 ms, steady settings
57 ms, settings during startup 53 ms, panel tabs 44 ms (medians). These meet the
existing 700/1000/75/75 ms UI/workspace/settings/panel targets. Browser/driver
caches were retained; localhost measurements make no production-network claim.

The measurements above precede integration of the unrelated command-search
change `35518698`. After integration, the shared UI/native-host suites and GTK
command-search test pass; a GTK warm startup completes in 0.75 s, Huion Web
again meets all four latency targets, and optimized Android startup/Settings
checks pass. The record includes those separate verification timings and APK hash.

## Validation and reproduction

- Shared renderer: 11 startup/admission/GPU tests, including promotion while
  optional jobs are held, automatic quiet-time resumption, teardown, unused
  recipes, first-use transforms, document effects and saved mask transforms.
- Shared UI: 663 tests after main integration. Native host: 31 passing tests, one pre-existing ignored
  test. Commit guard tests: 10 passing.
- Isolated GTK: `native_startup_latency`, `native_contact_brushes` (every contact
  preset plus spray, pixels and history), and `native_runtime_filter_packages`.
- Huion Android: `AndroidStartupTest` (startup, navigation, previously unused
  pencil/watercolor, reuse and undo), `AndroidPredictionTest#fallingPressureStrokeRendersAndSurvivesUndoRedo`,
  and optimized `UiStartupInstrumentation` timing runs.
- Huion Web: `device.test.mjs --staged-startup` and `--filter-previews`, including
  zero optional pipeline calls during a held contact and resumption afterward.
  `tools/performance/web-ui-startup.mjs` supplies refresh/interaction timings.

Build GTK with `cargo test --locked --profile dev-perf -p layer-linux --no-run`,
then pass its test executable as `LAYER_NATIVE_TEST_EXECUTABLE` to
`bash tools/performance/workspace-motion.sh gtk --native-test=native_startup_latency`.
Use a fresh `XDG_CACHE_HOME` for each cold/warm pair. Keep other builds and GPU
benchmarks stopped while measuring.

Android timing uses `:app:assembleBenchmark -PcapyBenchmark=true -PcapyOptimize=true
-PcapyAbi=arm64-v8a -PcapyRustProfile=dev-perf -PcapyApplicationId=art.capycanvas.shaderbench`.
Run the self-instrumenting `art.capycanvas.UiStartupInstrumentation` with
`-e uiStartupAudit true -e auditLabel LABEL`. Reports live in the application's
external-files `ui-startup-audit` directory. The old runner exceeded Binder's
result budget after saving its full report; the new runner returns the path.
Use an unminified benchmark plus its matching test APK for white-box Android tests.

## Windows integration

Windows enables demand shaders on its live, file-open and resumed-tab renderers;
color candidates inherit the live device setting. Window pointer, chrome, key and
action traffic already reaches the shared `UiSession` on the render owner, which
forwards `shader_input()`, so no separate observer is needed. Startup no longer
reloads the embedded catalog or stages `Assets/filters`; `CAPY_FILTERS_DIR` remains
an explicit override, and identical library refreshes are no-ops.

## Apple integration

Call `enable_demand_shaders()` on every staged renderer, including private
open/resume/color candidates, before preparing it. Keep polling actual
requirements after initial completion; document adoption needs canvas/brush
readiness, not unused-catalog completion. Forward native window activity through
`shader_input()` (or the opaque thread-safe `ShaderActivity` handle). Retain the
shared `CanvasRenderer` admission/wakeup delegation. Browser hosts additionally
schedule the remaining `shader_wait_ms()` delay. Remove duplicate startup
catalog loading and extend the identical-library no-op once those hosts opt in.
Validate first-use effects/tools, failed compilation/restart, held contacts,
rapid edits, document restoration and cache reuse on the target devices before
removing their legacy warmup path.
