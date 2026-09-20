# Huion UI startup audit — 2026-09-20

There is avoidable startup and UI-update work in both clients. The strongest
measured Web opportunity is the serial startup dependency on loading and parsing
161 separate icons, even when their network responses are cached. On Android,
the installed app is a debug build; the non-debug benchmark build substantially
reduces both startup and Settings latency. Remaining Android work is dominated by
Compose composition/measurement, including rebuilding or updating the surrounding
workspace when opening and closing Settings. SVG reuse is also incomplete.

The results do not justify removing UI features or broadly simplifying the CSS.
A controlled Web experiment removing shadows, filters and backdrop blur did not
improve startup consistently. This is an audit, not a production optimization patch.

Measurements used the attached Huion KP1202 / Kamvas Pad 12, MT8391, Android 16,
physical 1600 × 2400 display at density 320. Chrome reported version 143 in desktop
site mode, with a 1200 × 680 CSS viewport at device scale 2 and the light theme.
Android testing used the separate application ID `art.capycanvas.uiaudit`.
The main application's documents and preferences were not used as fixtures.
Source was working-tree revision `5004f1bead31f9b3359b34b5713919fdedcd73ff`, including
pre-existing uncommitted changes. Both Rust targets were compiled in release mode.
The device reported thermal status 0 near the end of measurement.

All raw logs, timing JSON, trace files, analysis scripts and build hashes are under
[`artifacts/ui-startup-huion-2026-09-20/`](../../artifacts/ui-startup-huion-2026-09-20/).
These generated artifacts are ignored by Git.

## Web findings

Four ordinary startup runs were measured over a local USB-forwarded HTTP server:
one with Chrome's HTTP cache bypassed and three with normal caching. The third
warm run also collected a CPU profile and a Chrome timeline, so its timings
include profiling overhead.

| Phase | Cache bypassed | Three normal-cache runs |
| --- | ---: | ---: |
| Initial JS module begins | 612 ms | 359–492 ms |
| Wasm initialization duration | 707 ms | 241–361 ms |
| Icon loading/parsing duration | 739 ms | 373–506 ms |
| Initial `update(255)` duration | 178 ms | 131–193 ms |
| Controls built, before initial GPU await | 2,375 ms | 1,363–1,602 ms |
| Workspace reports ready | about 2,964 ms | about 1,904–2,232 ms |

Times start at navigation. “Controls built” is the completion of the initial DOM
update, not proof of physical display scanout. The first profiling harness's
`audit.ui` field used the animation callback's frame timestamp, which can precede
a long task; use the explicit phase marks above instead. Workspace timestamps
from that harness are also frame-clock observations, not precise input latency.

The 161 icons contain only **56,366 bytes** of decoded SVG. All three warm runs
recorded **zero transferred bytes for these icons**. Browser network caching is
working; each reload still performs 161 fetch operations, response conversions and
XML parses before allowing any control construction. The CPU profile attributed
about 68 ms directly to `DOMParser.parseFromString` and 87 ms to `fetch` across the
startup capture. Those self-times do not account for the entire asynchronous wait.

The critical dependency is explicit in
[`app.js`](../../apps/layer-web/app.js): `await init()` → session/catalog →
`await loadIcons()` → `buildHeader()` / `buildPanels()` → `update(255)`.
The observed Wasm resource is approximately 21.5 MB uncompressed. This local
server experiment does not establish production compressed-download cost or PWA
service-worker behavior. The packaged service worker already caches app assets.

A second experiment isolated UI startup by omitting GPU attachment. It used a
single retained Paint workspace, normal HTTP caching, alternating treatments,
three samples per treatment, and asserted identical workspace layout in each
sample. The bundle treatment fetched the same source SVGs in one JSON response;
it still parsed each SVG and built the same controls. The CSS treatment removed
box shadows, filters and backdrop filters. These were test-server transformations,
not source changes to the app.

| UI-only treatment | UI-ready callback, median | Icon phase, median | Workspace-ready callback, median |
| --- | ---: | ---: | ---: |
| Current code | 621 ms | 247 ms | 1,007 ms |
| Single icon bundle | **379 ms** | **34 ms** | **743 ms** |
| Shadows/filters/blur disabled | 674 ms | 261 ms | 1,126 ms |

This establishes a roughly **242 ms reduction in the UI-ready callback** for the
bundle in this small controlled sample. Do not subtract it mechanically from the
earlier full startup runs: cache warming, GPU activity and profiling differ.
The CSS result is consistent with no benefit, not evidence that shadows themselves
make startup faster. Aggregate style recalculation remained about 50–52 ms and
layout about 23–25 ms in these UI-only samples.

There is also a main-thread startup burst. In the full startup runs, the task
following icon completion lasted **235–354 ms**. The profiled initial update
contained **11 style recalculations and 9 layouts**, taking approximately 46 ms
and 16 ms respectively within its 131 ms total. Control construction also builds
hidden panels and customization infrastructure. `measurePanels()` maintains
retained offscreen copies and queries intrinsic dimensions; `arrange()` writes
placement and then calls `resizeCanvas()`, which reads geometry. This identifies
repeated synchronous layout and eager construction as worthwhile targets.

The default workspace had 1,268 document elements, about 790 with layout boxes,
and additional measurement content inside a closed shadow root. This is enough
UI to make unnecessary passes costly, but the CSS ablation does not support
blaming decorative effects as the main startup bottleneck.

## Android findings

`dumpsys package art.capycanvas` confirmed that the installed application was
`DEBUGGABLE`. The existing `benchmark` build type is non-debug, inherits release,
and retains UI publication counters. It is not a fully minified production build.

The opt-in
[`AndroidUiStartupAuditTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidUiStartupAuditTest.kt)
measures first workspace draw, window frame metrics, native model publications,
and the first draw after a Settings-open request. It uses the actual frame clock,
not Compose test-clock advancement. Three unprofiled runs of each build produced:

| Metric | Debug | Non-debug benchmark |
| --- | ---: | ---: |
| First workspace draw, median | **2,352 ms** | **1,041 ms** |
| First workspace draw, range | 2,190–2,542 ms | 992–1,135 ms |
| First Settings opening in process, median | 431 ms | 212 ms |
| Subsequent Settings openings, median of nine | 291 ms | 144 ms |

An earlier first-install debug sample recorded a 2,802 ms first workspace draw.
Its original Settings probe waited for instrumentation idle and included extra
animation time, so its Settings numbers are excluded. The final comparisons use
`debug2`, `debug3`, `debug4` and `benchmark1`–`benchmark3` in `android-final/`.

Startup is measured from `ActivityScenario.launch` inside an already initialized
instrumentation process to the workspace's actual draw callback. It excludes
Android's process-launch and instrumentation bootstrap time. Settings timing ends
at the first draw with its model present, not the end of its 240 ms entrance
animation or physical scanout. Samples are sequential and not randomized between
build types. Treat the large build-type difference as strong directional evidence;
small differences between individual launches are not precision benchmarks.

A separate sampled method trace over four Settings open/close cycles confirmed:

- Substantial main-thread work in Compose recomposition, initial composition,
  intrinsic measurement and `measureAndLayout`.
- Repeated work in the surrounding `Workspace`, `PanelGroup`, `ToolRibbon` and
  `WorkspaceHeader`, alongside the Settings screen itself. `PanelGroup` accounted
  for approximately 388 ms of inclusive sampled CPU and header measurement about
  120 ms. These nested, instrumented times must not be added together or treated
  as unprofiled interaction latency.
- Approximately 71 ms of inclusive sampled CPU in AndroidSVG parsing and 112 ms
  in `SharedIcon`, which includes other work. Icons contribute, but are not the
  whole explanation.

Specific code paths explain these findings:

1. [`CanvasHost.publish`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt)
   considers nearly every change below `state` to be panel-content change, then
   publishes a fresh `panelContent` state object. This includes UI state unrelated
   to many panels. Field-level JSON transport already preserves many nested
   objects, but the dependency boundary remains broad.
2. [`Workspace`](../../apps/layer-android/app/src/main/java/art/capycanvas/Workspace.kt)
   reconstructs its panel lookup map with `objects().associateBy` on composition
   and passes that map to every `PanelGroup`. It removes `WorkspaceHeader` from
   composition while Settings is open, then recreates it on close. These are
   concrete sources of avoidable invalidation/reconstruction; a targeted fix still
   needs before/after testing and interaction validation.
3. [`SharedIcon`](../../apps/layer-android/app/src/main/java/art/capycanvas/SharedIcon.kt)
   synchronously opens the asset and parses SVG inside `remember(context, name,
   tint, fill)`. This cache belongs to one composition instance. New instances,
   reopening disposed UI, and tint changes can repeat parsing. There is no shared
   parsed-icon cache. `renderToCanvas` also traverses SVG during drawing.
4. [`Preferences`](../../apps/layer-android/app/src/main/java/art/capycanvas/Preferences.kt)
   composes its current page and shortcut rows in scrolling `Column`s, not lazy
   lists. This is a potential scaling issue for large lists; this audit does not
   establish it as the main default-workspace startup cost.

The native session, storage work and JSON publication run on the dedicated
`capy-canvas` owner, rather than the UI thread. In the non-debug runs, the largest
individual Rust model-publication samples were 22–24 ms; JSON parsing maxima were
10–15 ms. There is still latency to recover here, especially because field-diff
publication first generates a full serialized model, but it does not explain the
much larger Compose stalls by itself. Debug JSON parsing reached 115–159 ms,
another reason to use the non-debug build for performance decisions.

## Validation and next work

Completed checks:

- Web: four ordinary startup captures; nine controlled UI-only reloads, each
  opening/closing Settings three times and asserting a retained identical layout;
  no page exceptions in the successful runs.
- `node --test apps/layer-web/workspace-client.test.mjs`: passed. Storage settlement
  already schedules an immediate coalesced wakeup; the 100 ms maintenance timer is
  not the sole completion mechanism.
- `cargo test -p layer-host`: **28 passed, 1 ignored**. This includes retained-model,
  field-patch and unchanged-publication behavior.
- Android: three comparable unprofiled runs per build, a successful focused method
  trace, and a successful run of the finalized audit test.
- Existing `AndroidFirstUiTest`: **passed in debug**, demonstrating controls and a
  menu while GPU initialization is deliberately held, then surface recovery.
  An initial run against benchmark timed out because its test hook is explicitly
  guarded by `BuildConfig.DEBUG`; it is not evidence of a startup regression.
- An initial startup-wide method-tracing experiment did not complete and was
  aborted. It produced no usable trace and is excluded. The successful trace starts
  after warmup. An exploratory Web ablation that navigated into different workspace
  owners was likewise excluded and replaced by the reload-based controlled run.

Example audit invocation after building/installing the chosen variant with
`-PcapyApplicationId=art.capycanvas.uiaudit -PcapyAbi=arm64-v8a`:

```sh
adb -s G7DL2S300241 shell am instrument -w \
  -e class art.capycanvas.AndroidUiStartupAuditTest \
  -e uiStartupAudit true -e auditLabel benchmark1 \
  art.capycanvas.uiaudit.test/androidx.test.runner.AndroidJUnitRunner
```

Add `-e auditSampling true` only for attribution, not baseline timing. Use
`-PcapyBenchmark=true` when building the benchmark instrumentation APK. Results go
to the isolated app's external `files/ui-startup-audit/` directory.

Recommended order:

1. Bundle Web icons at build time from the existing canonical SVG files; preserve
   icon/tint semantics. The experiment establishes a benefit even with warm HTTP
   caches. Consider parsing one sprite or loading only initially needed icons next.
2. Batch Web startup layout reads/writes and defer inactive panel/editor creation.
   Preserve the retained measurement and drag/reorder contracts.
3. Evaluate Android performance with the non-debug build, then narrow panel model
   dependencies, retain derived lookup identity, and avoid rebuilding the header
   merely because a modal opens/closes. Verify visible changes and input ownership.
4. Add bounded, correctly keyed Android icon/preview reuse. Preserve tint, fill,
   density, resource configuration and drawing correctness; do not cache arbitrary
   user-dependent UI snapshots without invalidation.
5. Measure production Web delivery separately before changing Wasm packaging or
   network cache policy. Neither this audit nor the old GPU-startup timings proves
   production network cost.

Both clients already cache important resources and retain geometry/model state.
The opportunity is to remove specific repeated work and unnecessary critical-path
waits, while keeping the same interface.
