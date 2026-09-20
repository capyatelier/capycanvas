# Unified drawing tabs implementation

Completed: Web, then Android, with one shared Rust drawing-tab model and resource
lifecycle; GTK migrated/tested and both ports deployed on the attached Huion.
Acceptance inventory: [assessment](document-tabs-web-android-assessment.md).
Deployment and verification: [review guide](document-tabs-review.md).

## Milestones

- Starting synchronization: fetched and fast-forwarded `origin/main` to `bc38f041`.
- M1: shared session ownership, tab presentation/policy, safe parking and storage;
  migrate GTK and run shared + native GTK tab regressions.
- M2: Web tab/session integration and browser storage, browser/device acceptance.
- M3: Android integration, native Huion acceptance.
- M4: final GTK regression, refresh from `origin/main`, deploy both review apps
  to the Huion and record reproducible verification instructions.

Fetch and merge the latest `origin/main` at every milestone, resolve conflicts,
and rerun the checks affected by upstream changes before recording the milestone.

## Environment and ownership

- Checkout: branch `gtk-editing-regressions`, linked worktree. Git metadata is
  outside the writable checkout; git fetch/merge/commit require tool escalation.
- ADB: `/home/babymastodon/Android/Sdk/platform-tools/adb` (requires host access).
- Requested Huion: `G7DL2S300241`, Kamvas Pad 12 / KP1202. Another Wacom device is
  attached; always pass the Huion serial explicitly.
- Use an isolated Web origin and Android test application ID for destructive
  test fixtures. Do not clear the user's app/browser storage.
- Existing unrelated user edits at start: `docs/history/color-management-m2-port-handoff.md`
  and untracked `docs/development/color-management-m3-gtk-handoff.md`; preserve them.

## M1 validation

Moved GTK's inactive ownership, admission watermark, LRU spill selection and
labels into `layer-ui::DocumentSessions`; native private chunk storage into
`layer-core::raster_storage::spill_to_directory`; tab header expansion into
`HeaderLayout::resolve_documents`. All hosts can use `UiSession::park_document`
to reject unsafe retirement without discarding input, and `RetainedTiles::try_blobs`
to poll exact current/undo/redo backing without blocking an event loop.

Validation logs are in ignored `artifacts/document-tabs/m1/`:

- Full core/engine/UI suites: 104 + 63 + 495 tests passed before the two added
  parking regressions; those new tests and focused header/lifecycle/storage
  suites subsequently passed.
- GTK release unit suite: 22 passed, 185 native cases ignored by that command.
- Native real-GPU GTK cases passed: document history/storage/close, immediate
  stroke/undo switching, failed-renderer navigation, multiple recovery offers,
  disk failure retaining data, and document tab input.
- The first input run missed mouse pickup while another compositor test was
  running. The isolated rerun passed the complete mouse/touch/key journey.
- The disk test initially caught changed error wording; restored the existing
  actionable wording and reran successfully.
- Committed shared extraction as `f5400869`, then fetched and merged the new
  `origin/main` (`9bba67d9`, LZ4 tile/project storage) in `5696b1e7`.
- Post-merge core/engine/UI: 105 + 63 + 496 passed. Nine photo fixtures needed
  larger compressed-byte budgets for padded LZ4 tiles; production limits and
  budget rejection tests remain unchanged. WASM and GTK release builds passed.
- Post-merge native GTK history/storage, immediate stroke/undo and disk failure
  regressions passed on the real GPU.

## Current work

M2 (Web) is implemented and qualified below. Proceed to M3 (Android), then
perform the final GTK regression, upstream synchronization and device deployments.
Hosts retain native input/capture, GPU surfaces, scheduling and storage transport;
shared tab/session/storage policy stays in Rust.

## M2 Web milestone

Web now retains CPU editors in the shared `DocumentSessions`, has a flexible
strip/compact selector, shared Drawings command, immediate tab/handle input for
mouse/touch/pen, reorder history, multi-file opening, background close and a
fresh final browser drawing. Recovery has a lease/policy per drawing, immutable
capture-before-await, and one serialized window writer. Proof/tone/histogram jobs
retire before switches. The GPU device/surface now belongs to the window;
parked editors have no renderer or textures. A newly selected editor gets a new
renderer on that same device (the first Huion run exposed the cost and close
race from requesting a whole new device on each switch).

Core tile storage now also supports asynchronous immutable host chunks. Browser
OPFS uses async APIs, no blocking main-thread calls or per-tab workers. Core
publishes references only after complete successful writes, verifies compressed
integrity on reads, and releases chunks with their last tile owner. Render
restoration polls backing before consuming a frame; file packing awaits backing
and reports errors instead of assuming all WASM tiles remain resident. Old read
caches evict on parking. Inactive RAM and metadata policies remain shared Rust.

Current validation:

- Shared full suites: 106 core + 63 engine + 496 UI passed.
- Core external-chunk test covers failed/cancelled write retaining bytes, pending
  async reads, exact samples, cache eviction without rewriting, last-owner drop.
- Updated GTK release builds and native history/storage/close passed.
- Desktop Chrome `--headless --offscreen-raster --drawing-tabs` passes independent
  history, mouse/pen/touch immediate reorder, order undo, background close,
  cancel/discard, selector, final fresh drawing, and OPFS redo-only exact data.
  The explicit offscreen mode is needed for the documented pre-existing Chrome
  headless Dawn instance warning; this does not qualify desktop presentation.
- Huion first run reached closing and caught a close request issued before
  replacement startup was idle. Fixed by waiting for the safe boundary, and
  avoiding device/catalog recreation during tab switches. The updated device run
  passed lifecycle, immediate mouse/pen/touch input and exact redo-only OPFS data.
- Desktop final journeys also pass corrupt and duplicate opens, failed Save/Close,
  real OPFS write failure retaining RAM, refusal of additional opens and retry.
- Multiple recovery offers append independent unsaved drawings on both desktop
  and Huion, and inactive unsaved tabs protect browser unload. The Huion reload
  test initially timed out during cold shader-library startup (149 s measured);
  the rerun with a longer cold-start allowance passed. Tab switches reuse the
  window device and do not repeat the catalog load.
- Visual review caught stale disabled controls after renderer attachment; Web
  now publishes the refreshed command state. The final desktop journey asserts
  visible New controls are enabled after reactivation.
- GTK now uses the shared tab drop target calculation. Its native mouse/touch/
  keyboard regression passed again. The Close command and approved-close parking
  also honor the existing shared permission to close during read-only library
  warmup; the extended shared guard test passes.
- Recovery test navigation now waits for the new page context. An intermediate
  desktop run caught a harness race evaluating the page being destroyed; rerun
  passed after fixing the navigation wait.

Live test server: exec session 49285 on 127.0.0.1:8147. Huion reverse ports 8147
8148 and 8150 route to it; CDP forward 9237. Device lifecycle used
http://127.0.0.1:8148/; recovery used http://127.0.0.1:8150/. Earlier test origins
contain only this task's regression records. Final review will use a separate
packaged build/origin. A generic ADB screenshot showed another foreground native
activity; the actual Chrome capture is `web/huion-browser-tabs.png`.

## M2 synchronization and M3 Android work

- Web committed as `a3f4c196`; fetched and merged latest `origin/main` at
  `8789c04c` in `b20fd452`. Upstream adds shared HDR gain-map delivery. The only
  conflict was Web test registration; both suites are retained. Merged WASM
  build and desktop drawing-tabs journey pass (`m2-merged-web-*.log`).
- Android implementation is in progress. `App` now retains shared
  `DocumentSessions<UiSession<Renderer>>`; one bounded activation worker drops
  the outgoing renderer and prepares the incoming renderer on the window device.
  File preparation admits candidates before GPU allocation and appends only after
  exact outgoing captures are ready. Native spill uses the same unlinked immutable
  chunks as GTK. Stable tab IDs bind file transfers and recovery captures.
- Android has a strip/compact selector, shared drop queries/order history,
  Drawings command and keyboard alternatives, sequential multi-file SAF opening,
  selected/background close, final Activity/workspace flush, and per-tab recovery
  leases with one serialized writer. Proof, tone and inspection work drain before
  transitions. Header customization keeps ownership of the whole title item.
- First Android build + instrumentation build + lint passed. SDK/tool invocation:
  `ANDROID_HOME=/home/babymastodon/Android/Sdk`
  `PATH=/tmp/capy-audit-tools/bin:/home/babymastodon/.cargo/bin:$PATH`
  with isolated application ID `art.capycanvas.tabtest`, label `Capy Tabs Test`.
  The temporary tools directory contains the installed cargo-about needed by the
  new upstream Android notice packaging; do not skip that packaging task.
- Huion `AndroidRasterTest#drawingTabsKeepHistorySpillAndLifecycle` passed
  (54.817 s): exact redo-only disk backing, independent history, no parked
  renderers, same GPU device generation, order undo independent of selection,
  Activity recreation, corrupt-open retention, admission rejection, close cancel,
  neighbor choice and empty final ownership (`m3-huion-native-tabs.log`).
- Huion `AndroidRasterTest#drawingTabsRecoverMultipleInactiveDrawings` passed
  (20.162 s): two independently captured active/inactive recovery records, two
  sequential offers appending unsaved drawings with distinct layer contents and
  correct origin retirement (`m3-huion-recovery.log`).
- Rotation audit found parked viewport dimensions could be stale. Shared
  `inherit_window_state` now copies window viewport/scale while retaining drawing
  history/camera state. Its added test passes, as do all five renderer lifecycle
  tests and 28 native-host tests. The initial test incorrectly assumed a blank
  drawing had one layer; changed it to compare the actual starting layer count.
- Current work: compile/run real Android mouse/touch/pen strip/handle input and
  production close flow, finish edge-case audit, commit/synchronize M3, then final
  GTK/Web qualification and packaged Huion deployments. No final deployment yet.

M3 additional qualification:

- Android native strip mouse/pen/touch dragging and cancellation passed. The first
  selector test used its underlying header's target coordinates; fixed the test
  to use the same native view as the selector handle. The row-hold extension
  needed Compose's virtual clock advanced alongside real time. The complete
  input/close journey then passed in 20.536 s (`m3-huion-input-close-clock.log`),
  including pre-hold scrolling, held pen/touch row drag, background close,
  Cancel/Discard, and final Activity closure.
- Android serial file-batch test passed in 11.626 s, including duplicate URIs,
  independent clean owners, a corrupt middle file, and last-success selection.
- Added Android ACTION_VIEW/SEND/SEND_MULTIPLE entry points, title/compact/overflow
  file drops, and shared Rust prefix classification for canvas/layer drops.
  Photos keep captured layer targets; native drawings open as drawings. Mixed
  canvas batches place photos first, then open native drawings only after the
  placement succeeds. Title drops open all inputs in supplied order.
- New/Open and recovery retry native spill before admission, so freeing disk
  space allows a fresh attempt. Normal switching refuses active edits/modals
  before taking input ownership. Surface recreation defers while an activation
  owns the renderer exchange. The native placeholder covers transitions.
- Shared full suites pass: 106 core, 63 engine, 497 UI, 28 native-host (one
  intentionally ignored host test). GTK real-GPU history/storage/close passed
  9.89 s and native input passed 18.11 s after the viewport change.
- Final Android build/instrumentation/lint passed (`m3-android-final-build.log`).
  A combined Huion suite is currently running in exec session 24692; its log is
  `m3-huion-final-suite.log` (four tab cases plus retained place/paste/details).
- Preparing Web distribution exposed missing new module entries in the explicit
  fingerprint graph. Fixed packaging and extended its fixtures; all 14 package
  tests pass. Also hardened Web multi-file continuation after a bad middle file,
  batch input ownership, and installed-app file-handle entry point. Updated Web
  lifecycle passes; the test now also includes a corrupt middle batch item.
- Web production package is building in exec session 83676, log
  `m4-package-build.log`; tools are `/tmp/capy-audit-tools/bin/{resvg,cargo-about}`.
  New package uses the shared viewport fix. Must rerun packaged Web/device
  qualification after final upstream merges. No M3 commit/sync yet.

M3 final reruns:

- The combined five-case Huion run passed three tab cases and exposed two issues.
  The existing photo test called adoption without waiting for parking; its Open
  helper now uses `projectParkReady`. The new selected-tab guard also refused
  final close during read-only library warmup. Closing the already selected tab
  now uses the shared Close permission directly, without requesting a switch.
  Both affected cases then passed together in 75.323 s
  (`m3-huion-close-photo-retry.log`). No failing cases remain from that run.
- Native visual inspection found Compose's zero-width border still paints a
  hairline. Idle tabs now omit that modifier entirely; only the dragged tab gets
  the drag border. Titles are centered with balanced close-control space.
- The Web distribution built successfully with host access for required original
  dependency notices (`m4-package-build-retry.log`, 282 precached files).
  Packaged tab lifecycle passes (`m4-package-tabs.log`). Desktop offline PWA test
  reached its real-ink presentation assertion and failed under the already
  documented headless Dawn issue; final offline presentation must be checked on
  the Huion, not counted as a desktop pass.

## M3 committed; M4 final qualification

- M3 Android/shared work committed as `db92d0da`; Web packaging and ordered
  external opening follow-up as `e8df2d95`. M3 merged latest upstream `a23c627a`
  (AVIF gain-map delivery quality) in `3a710bd6`, without conflicts. Final M4
  fetch/merge again reported already up to date at `a23c627a`.
- Merged builds pass: GTK release test binary (`m4-gtk-build.log`), Android debug
  APK/instrumentation APK/lint (`m4-android-build.log`), production Web package
  including original dependency notices (`m4-merged-package.log`, 282 precached
  files). Upstream affected gain-map tests pass: 11 passed, 2 intentionally
  ignored (`m4-merged-color.log`).
- Final packaged Web lifecycle passed on the Huion ARM Valhall GPU, with strict
  zero unexpected browser errors (`m4-huion-packaged-tabs.log`). It covers real
  mouse/pen/touch reorder, independent sessions/history, close decisions, compact
  selector, final blank, exact redo-only OPFS spill/restore, corrupt/duplicate
  batch opens, failed Save, quota failure and retry.
- An overly broad GTK filter was invalid: the disk-failure fixture needs its
  dedicated environment and GTK initialization cannot move between libtest
  threads, even with `--test-threads=1`. The failed-renderer case passed before
  the process aborted. Reran separate processes successfully: history/storage/
  close 10.13 s (`m4-gtk-history.log`), immediate stroke/undo 4.26 s
  (`m4-gtk-immediate.log`). Earlier dedicated disk/recovery/input runs remain
  recorded above.
- Installed final isolated Android APKs on Huion `G7DL2S300241`. An initial
  final input test lost the foreground when Web testing opened Chrome and timed
  out during row dragging (`m4-huion-native-input.log`); this run is invalid as
  qualification. The retry reserves the tablet foreground for instrumentation.
- That final Android retry passed in 25.365 s (`m4-huion-native-input-retry.log`).
- A final Web input audit found that selector row holds needed non-passive touch
  arbitration and retained contextual actions. Added these, clipped drop hit
  rectangles to the visible list, added edge scrolling during drag, and centered
  strip titles independently of close controls. Extended the lifecycle journey
  with mouse-hold suppression, touch/pen pre-hold movement, held release, continued
  drag, late native context events and independent reorder undo. The updated
  desktop journey passes (`m4-web-row-hold-retry.log`); it retains the documented
  offscreen-presentation limitation. The first desktop attempt omitted this
  task's server URL and did not load the app (`m4-web-row-hold.log`).
- Rebuilt the production package with these input fixes (`m4-final-package.log`).
  Another milestone fetch/merge confirmed upstream remains `a23c627a`.
- Offline harness correction: CDP `Page.reload(ignoreCache: true)` bypasses
  service-worker control in Chrome, leaving an activated registration but an
  uncontrolled page. The tablet offline journey now uses normal navigation,
  independently disables the HTTP cache, then disables networking. Earlier
  interrupted/uncontrolled offline attempts are not counted as passes.
- The final packaged Huion lifecycle, including the new selector row gestures,
  passes with zero unexpected browser errors (`m4-huion-final-tabs.log`).
- Committed the final Web input fixes, harness and review guide in `4be6cf2c`.
  Fetch/merge after that commit again reported already up to date at `a23c627a`.
- The corrected offline navigation reached the ready canvas, but Android Chrome
  resets `navigator.onLine` to true after navigation even while CDP still blocks
  requests. A direct uncached fetch confirmed `TypeError: Failed to fetch` in that
  state. Replaced the unreliable status assertion with an uncached request probe;
  its one expected network error is identified by its unique URL, while all
  other browser errors still fail the run. This only changes the test harness.
- Final offline Huion run passes (`m4-huion-final-offline-probe.log`): cached PWA
  cold navigation with HTTP cache disabled, a failed uncached network probe,
  actual pen ink (50,400 to 48,810 white pixels in the sampled area), new drawing,
  and exact displayed ink after GPU retirement/restoration. No unexpected browser
  errors. Networking and HTTP-cache settings were restored in `finally`.
  Screenshot inspected: `web/huion-packaged-offline-tabs.png`.
- Closed only completed task-owned browser test pages to release their GPU
  resources; preserved the review page and unrelated user pages. The final review
  origin is `http://127.0.0.1:8162/`, backed by the packaged local server. Android
  is installed as **Capy Tabs Test** (`art.capycanvas.tabtest`).
- Final native visual check passed: launched the installed review app, created a
  second drawing through its production New dialog, and inspected
  `huion-native-review-tabs.png`. Both titles are centered; selected fill and close
  controls render without the removed hairline border. Left Android in the
  foreground with two clean drawings, and Chrome's review page with two drawings
  and its sample ink. No production app/browser data was cleared.
- Chrome accepted the optional Web Install action, but no separate WebAPK was
  observed at handoff; the verified Web review entry point is the cached Chrome
  URL documented above. Standalone launcher installation is not claimed.
- All implementation, GTK/shared qualification and Huion review deployment work
  is complete. The two unrelated color-management handoff edits remain untouched.
