# Unified drawing tabs implementation

Active goal: implement Web, then Android, with one shared Rust drawing-tab model
and resource lifecycle; preserve/test GTK and deploy both ports on the attached
Huion. Acceptance inventory: [assessment](document-tabs-web-android-assessment.md).

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
