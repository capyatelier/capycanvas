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

Proceeding to M2 (Web). Browser tile storage still needs implementation; native
spill support compiling for WASM does not provide browser disk backing. Hosts
retain native input/capture, GPU surface creation, scheduling and storage
transport. Shared tab/session/storage policy stays in Rust.
