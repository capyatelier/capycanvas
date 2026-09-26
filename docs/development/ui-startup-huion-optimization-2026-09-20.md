# Huion UI startup optimization — 2026-09-20

The bottlenecks were repeated work and broad invalidation, rather than the number
of visible controls or decorative CSS. See the [original device audit](../history/ui-startup-huion-audit-2026-09-20.md)
for baseline measurements and controlled icon/CSS experiments.

The optimization was developed in a clean worktree based on `origin/main`
(`8bd5509e`), then rebased onto `0a1acaf4` before the final merge builds. Pre-existing uncommitted work in the original checkout was preserved.
The historical baseline includes the original checkout’s uncommitted changes;
its before/after deltas also include intervening upstream changes. The controlled
experiments in the audit support the individual bottleneck findings.
The Huion was `G7DL2S300241`, KP1202, Android 16; Web tests used Chrome 143,
1200 × 680 CSS pixels at 2× scale. Dedicated test origins and a separate Android
application ID kept performance fixtures separate from the user's documents.

## Targets and measurement boundaries

Use three unprofiled runs and report medians, with individual samples retained.
Targets apply to medians, not a guarantee for every frame or cold launch.
The practical targets on this device are:

- Warm Web controls built within 700 ms; workspace adopted within 1,000 ms.
- Non-debug Android first UI draw within 700 ms.
- Repeated Settings response within 75 ms; first-use cost reported separately.
- Background canvas preparation must allow UI interaction, and must resume after
  interaction. Canvas/brush/full shader readiness are separate milestones.

Web startup begins at navigation. `capy.startup.ui` means initial controls and
layout have been built, rather than physical display scanout. Settings response
runs from invoking the real control to two subsequent animation callbacks.
Android response runs from the request to a window draw with the new model; it
excludes completion of the 240 ms entrance animation. Android first draw starts
at activity launch inside instrumentation, excluding process/instrumentation
bootstrap. The original audit used ActivityScenario; the optimized runner uses
startActivitySync, so historical launch deltas also include that harness change.

Measurements describe this tablet with local USB delivery. The final Web
fixture is the Paint workspace with approximately 1.06 MB of retained workspace
state and layout history accumulated during testing. This history was retained
for acceptance. These measurements do not establish WAN download time or
arbitrary large-document performance. Raw samples and profiles are in
[`artifacts/ui-startup-optimization-2026-09-20/`](../../artifacts/ui-startup-optimization-2026-09-20/).

## Changes

Web:

- Build one SVG bundle from the 161 canonical icons, preserving each original
  vector and paint definition. Fetch and parse it once, concurrently with Wasm.
- Preload the JavaScript module graph, icon bundle and Wasm; compile Wasm once and share the immutable module
  with the storage worker. Load its JavaScript while Wasm compiles, then
  instantiate its Wasm instance alongside the main instance. A small asynchronous
  entry point starts this work before the editor module graph is ready; the first
  storage request overlaps panel construction.
- Cache one decoded workspace database, keyed by the exact snapshot read in each
  IndexedDB transaction (up to 8 MiB of encoded data). External writes and aborted
  writes invalidate the candidate naturally; reducer errors discard it. Read
  requests omit unused snapshot encoding. A consuming reducer avoids copying all
  retained history for a transaction that already owns its candidate. Compare
  the source strings in JavaScript before copying them into Wasm, return snapshot
  strings directly across the bridge, and reuse validated catalog replies while
  the database bytes remain unchanged.
- Use production thin LTO and one codegen unit; the measured Wasm shrank from
  approximately 21.5 MB to 19.1 MB before HTTP compression.
- Send only changed top-level UI fields across the Wasm/JavaScript boundary,
  retaining the existing BigInt types and independent full-snapshot API. Reuse an
  unchanged read-only Settings view across openings.
- Skip workspace refreshes for Settings-only changes. Avoid repeated theme,
  control, title-bar, and numeric-control writes. Build the title-bar component
  bank only when customization opens.
- Batch geometry reads/writes and scrollbar measurements. Hide the retained
  offscreen measurement tree between measurements; it otherwise adds browser
  modal layout cost despite being invisible. Pause layer thumbnail geometry
  polling behind Settings and avoid unchanged background status writes. Retain observed
  Navigator geometry and the canvas scale instead of forcing layout every frame.
  Guard unchanged command text and accessibility attributes. Batch color-wheel
  raster painting after layout and theme writes, flushing before the UI readiness
  mark so the wheel is drawn at its final size rather than rasterized twice.
- Restore an already-bound window by claiming and validating its workspace,
  without republishing the unchanged selection or waiting for redundant delivery
  bookkeeping. Explicit switches and migrations still publish; resuming one
  window preserves another window's more recent selection. A regression verifies
  ownership renewal, unchanged generations and ordinary switch publication.
- Give saved-workspace adoption a bounded one-second head start over GPU allocation.
- Stop continuous canvas redraws merely to wait for optional shaders. During
  startup, Settings takes priority; the next shader job waits for 500 ms after
  dismissal so a burst of UI interactions can finish. Drawing frames remain
  independent; preparation resumes automatically.

Android:

- Enable R8 code optimization and resource shrinking for release builds, with
  explicit JNI name preservation. The installed baseline had been debuggable.
- Cache a bounded set of immutable SVG recordings by asset manager, icon, tint
  and fill. Each component still owns its painter.
- Retain unchanged panel models and derived lookup/group identities, including
  when a transport baseline is refreshed. Settings navigation does not invalidate
  the panel-content tree.
- Create tooltip popup infrastructure only for a hovered item, keeping the
  interactive node and pointer capture stable. Increase the header text cache.
- Retain the header and Settings composition while hidden. Hidden views release
  focus, popups, Back handlers and accessibility; invalid numeric drafts reset.
  A tracked CompositionLocal limits visibility invalidation to its consumers.

Controlled dialog-layout retention and CSS-containment experiments showed no useful improvement,
so they were not included. The initial CSS-effect ablation likewise did not justify
removing shadows, filters, or interface components.

## Reproduction

Build Web with the documented [packaging prerequisites](web-packaging.md), serve
`dist/capycanvas` on a dedicated origin, and forward Chrome's DevTools socket:

```sh
LAYER_DEVICE_CDP=http://127.0.0.1:9246 \
LAYER_WEB_URL=http://127.0.0.1:4197/ \
LAYER_TEST_ARTIFACTS=artifacts/ui-startup \
node tools/performance/web-ui-startup.mjs --assert-targets
```

A packaged PWA deliberately waits to activate updates. Close its old clients or
unregister only the dedicated test origin before benchmarking a new package;
hard-reloading a development URL does not reliably update a cached worker.
Never clear a user's entire browser cache to refresh this fixture.

Build the optimized Android benchmark with:

```sh
apps/layer-android/gradlew -p apps/layer-android :app:assembleBenchmark \
  -PcapyBenchmark=true -PcapyOptimize=true -PcapyAbi=arm64-v8a \
  -PcapyApplicationId=art.capycanvas.uispeed
adb -s G7DL2S300241 install -r \
  apps/layer-android/app/build/outputs/apk/benchmark/app-benchmark.apk
adb -s G7DL2S300241 shell am instrument -w \
  -e uiStartupAudit true -e auditLabel run \
  art.capycanvas.uispeed/art.capycanvas.UiStartupInstrumentation
```

The runner is in the benchmark source set and absent from release. It records
first draw, early and steady Settings responses, UI frame intervals and native
publication costs. Reports are under the app's external `files/ui-startup-audit/`
directory. Cross-APK white-box regressions use the ordinary unminified benchmark
(omit `capyOptimize`); R8 can remove methods accessed only by a separate test APK,
as described in the [Android benchmarking guidance](https://developer.android.com/topic/performance/benchmarking/microbenchmark-without-gradle).

## Web results

Three warm production-package runs on the Huion, after all code changes:

| Metric | Historical baseline | Optimized median | Target |
| --- | ---: | ---: | ---: |
| Controls and initial layout | 1,363–1,602 ms | **556 ms** | ≤700 ms |
| Saved workspace adopted | 1,904–2,232 ms | **974 ms** | <1,000 ms |
| Repeated Settings response | — | **55 ms** | <75 ms |
| Ordinary panel tab response | — | **45 ms** | <75 ms |

Controls took 569, 556 and 533 ms; workspace adoption took 972, 974 and 1,015 ms.
All median targets pass, with one workspace run just above one second. The 12
steady Settings samples ranged from 44–59 ms. Repeated Settings openings during
canvas preparation had a 45 ms median, with two 124 ms samples. Panel switches
ranged from 32–66 ms. Panel probes run only after the last startup sample, so
persisting their undo history does not enlarge the following startup fixture.

Canvas readiness is outside these UI targets: paper appeared at 1.7–1.8 seconds,
brush readiness at 6.5–6.7 seconds, and the full optional shader set finished at
about 44 seconds, including deliberate interaction pauses. Initial UI construction
still has a roughly 188 ms main-thread task; individual background canvas jobs
can also exceed 100 ms. The changes eliminate repeated UI work and give Settings
priority, but do not establish zero jank or eliminate the separate GPU startup
cost. The staged-startup regression checks that mouse, touch and pen interaction
works while optional compilation is held and that compilation resumes afterward.

Compact individual samples are checked in at
[`measurements/huion-ui-2026-09-20.json`](measurements/huion-ui-2026-09-20.json).
Raw final Web results are in `web-paint-acceptance/web-ui.json` within the artifact
directory, including navigation/resource timing, readiness marks and long tasks.

## Android results and installed build

Three optimized runs on the final Android code (`final-1` through `final-3`):

| Metric | Before, debug | Before, non-debug | Optimized | Target |
| --- | ---: | ---: | ---: | ---: |
| First UI draw, median | 2,352 ms | 1,041 ms | **564 ms** | ≤700 ms |
| Repeated Settings, median | 291 ms | 144 ms | **66 ms** | <75 ms |

First UI draw ranged from 561–682 ms. The 12 repeated Settings samples were
46–75 ms. First-use Settings still builds its controls: 90–124 ms in these runs;
the second opening during startup took 62–70 ms. UI callback intervals had
per-run p95 values of 12.8–13.9 ms, with worst gaps of 78–98 ms, including first
Settings composition. The observer runs after activity launch and is not a
measurement of display scanout or input latency.

The ordinary R8-optimized release APK, without benchmark instrumentation, was
installed over `art.capycanvas` on the Huion using the matching development
certificate. Existing app data was retained; the original APK was backed up.
Package inspection confirmed the installed application is not debuggable.
Its first process-cold `am start -W` reported **838 ms** total launch time.
That includes more of the launch path than the instrumented UI-draw metric above.
Opening and closing Settings in this installed release was also checked.

## Validation

- All 161 bundled Web icon XML trees match their canonical source; repeat bundles
  are deterministic. Static-package, service-worker and workspace-client tests:
  23 passed. Shared Rust host tests: 28 passed, one ignored.
- The broader native workspace suite had 85 passes and three controller failures.
  All three reproduce on unchanged `origin/main` (`0a1acaf4`):
  `invalid_defaults_recover_through_controller_switch_and_preview`,
  `active_deletion_uses_available_defaults_and_persists_the_replacement`, and
  `unpinned_current_workspace_is_temporary_and_previews_do_not_replace_it`.
  Their equality assertions include a stale F16 versus U8 working-color depth.
  All 50 SQLite/browser storage contract cases pass on the Huion, including
  aborted writes, external snapshot changes, default recovery, ownership,
  reopen persistence, newer schema rejection and storage/quota failures.
- Huion Android: canonical icon rendering and theme/size/tint cases; mouse/pen
  tooltip placement and focus; tile hold ownership across input presentations;
  title-bar keyboard/context/focus ownership; Settings animations, choice menus,
  typing, numeric validation, reset behavior and canvas input isolation passed.
  The final retained-Settings checks cover popup dismissal, focus release and
  invalid draft cancellation on close/reopen.
- Huion Web: incremental/full-state equivalence and snapshot ownership; Settings
  values, numeric boundaries, reopening, search and themes; staged startup with
  mouse/touch/pen interaction while shader work is delayed; title-bar feedback;
  retained panel/drawer geometry and resize undo/redo/cancellation passed.
  The complete editor journey also passed: color controls, Navigator, nested
  drawers, real Wasm project save/open/new, PNG export, cancelled unsaved edits
  and workspace restoration after reload. Final component runs are recorded in
  `web-ui-final-regressions.log` (staged startup/state/Settings),
  `web-feedback-final.log`, `web-resize-final.log` and
  `web-editor-final-regressions.log`. The combined UI run exposed fixture
  assumptions about existing layouts and recovery dialogs; the remaining
  components were rerun separately after preparing the dedicated fixture.
- Native test fixtures now wait for asynchronous page navigation and inspect
  the actual text nodes under retained controls. The tooltip edge expectation
  uses available window space; the former unconditional bottom-flip assertion
  also failed against the unmodified tooltip implementation. The Web editor
  fixture explicitly selects Paint and its color-wheel shape, enables drawers by their
  column IDs, confirms layout reset and export options, and uses current drawing-tab
  cancellation behavior. Its in-memory file transport accepts both byte arrays
  and Blob exports. Tests use a dedicated
  browser origin; its default layouts and recovery prompts are prepared before
  journeys that require a clean starting state.

Earlier desktop GPU runs encountered the Linux NVIDIA external-instance warning
and could not provide a clean full-suite result. Physical Huion runs are the
acceptance evidence for GPU-dependent UI behavior; the package/transport tests
also run independently of that desktop GPU environment.
