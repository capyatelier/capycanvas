# Automatic recovery after the file format redesign

[Developer guide](README.md)

**Status: deferred until the qualified `.capy` cutover lands.**
The qualified application cutover is the prerequisite. The
[authored capture and recovery boundary](../reference/authored-model.md#capture-phases-and-recovery-extension)
and [package backing/publication contract](../reference/capy-package.md#immutable-backing-capture-and-publication)
define the shared interfaces to recheck in code. Optional lazy-loading or
execution-plan work is not a prerequisite. This plan does not start recovery
implementation alongside the format cutover.

## Intended behavior

Ordinary restarts restore the open editing session without a recovery prompt.
Quitting preserves open drawings, including untitled drawings and drawings with
no unsaved changes. Explicitly closing a drawing keeps Save/Discard/Cancel
semantics and removes it from the next restored session once close succeeds.
Saving a drawing does not remove it from the session.

An orderly exit publishes the latest committed state before completing. A crash
during or immediately after an operation may lose recent work that has not become
durable. Recovery restores a complete committed state; it never combines partial
operations or silently substitutes an empty drawing for unreadable artwork.

Preserve tab order, the active drawing, names, save destinations where access can
be restored, modified state, editing targets, selection, camera and bounded
undo/redo history. Reuse existing workspace and preference persistence for the
state they already own. Keep temporary gestures, previews and GPU handles out of
persisted session state. These are acceptance requirements for the later work,
not claims about current behavior.

## Recheck after the redesign

Read the landed format implementation and its updated
[project format reference](../reference/project-format.md). Determine which
snapshot, resource, validation, lazy-loading and publication APIs recovery can
reuse. The earlier full-archive recovery writer is not a constraint on the new
implementation, and the new package must not be assumed to provide incremental
writes merely because it can reuse compressed resources.

Trace shared policy in [recovery.rs](../../crates/layer-ui/src/recovery.rs), file
checkpoint handling in [document_files.rs](../../crates/layer-ui/src/document_files.rs)
and tab ownership in [document_sessions.rs](../../crates/layer-ui/src/document_sessions.rs).
Recheck each host's recovery, close and background flows against those contracts.
Update these pointers if the format work moves their responsibilities.

The integrated format also exposes [`Editor::capture`](../../crates/layer-core/src/lib.rs),
[`CanvasEngine::capture_artwork`](../../crates/layer-engine/src/canvas.rs) and
[`ArtworkCapture`/`CaptureCheckpoint`](../../crates/layer-core/src/authored/artwork.rs).
[`ResourceInventory`/`PreparedResources`](../../crates/layer-core/src/package/resources.rs)
and [`PreparedTransfer`](../../crates/layer-core/src/package/transfer.rs) enumerate
portable metadata and resource payloads independently of ZIP assembly.
[`Editor::retained_tiles`/`RetainedTiles`](../../crates/layer-core/src/raster_storage.rs)
and [`history_budget`](../../crates/layer-core/src/history_budget.rs) cover retained
owners and accounting. Recheck these interfaces after M3 qualification; they do
not yet persist private session/history records or establish incremental writes.

Measure the landed implementation before selecting storage changes. Include a
metadata-only edit, a small stroke, an operation that changes most pixels, several
open drawings and a document with substantial undo history. Record snapshot cost,
bytes read and written, peak retained memory and time until the checkpoint is
durable. Do not carry forward current tile sizes, hash encodings, archive layout
or timing assumptions without checking the replacement.

## Session and storage contract

**Shared ownership.** Shared Rust owns session membership, checkpoint freshness,
history, restore decisions, validation and failure state. Hosts own timers,
lifecycle notifications, file permissions and storage execution. Replace the
superseded recovery paths as the shared contract reaches each host.

**Artwork and session state.** Reuse the redesigned artwork model and codecs.
Define a private session record for state that the portable artwork format does
not own. Undo/redo needs an explicit persisted representation with the existing
history budgets; it must not become an unlimited editing log or a mandatory
addition to every exported `.capy` file. Preserve the manual-save checkpoint
separately from the latest recovery checkpoint so Undo and Save keep the modified
indicator correct.

**Consistent publication.** Freeze artwork, history and related file state at one
committed boundary. Publish referenced resources before the checkpoint that makes
them reachable. A failed or interrupted write preserves the previous complete
checkpoint. Completion for an older generation must not acknowledge newer edits,
retire their data or resurrect an explicitly closed drawing.

**Incremental storage.** Prefer reusing unchanged immutable resources between
checkpoints when measurements justify it. Reuse the new format's resource
identity and encoding contracts rather than creating a competing artwork schema.
Choose the private storage layout after the redesign; this plan does not choose
a database, directory layout or append protocol. A complete metadata snapshot
with reusable payloads is a candidate, not a requirement to append to a live
`.capy` file. Start with ownership scoped to each drawing rather than requiring
deduplication across unrelated documents.

**Retention and cleanup.** Preserve resources reachable from published
checkpoints, retained undo/redo, active readers and accepted writes. Coordinate
cleanup with live owners across windows and processes. Bound retained generations
and history storage. Disk-full or quota failures retain the last good copy and
report that newer work is not protected; cleanup never deletes the only durable
copy of an open drawing to satisfy a budget.

**Save destinations.** A saved path or URI is not sufficient proof of write
access. Restore native permissions or bookmarks where supported. Restore the
private artwork even when access to the original destination is unavailable, then
request a destination when saving. Detect an externally changed original before
overwriting it. Publishing recovery does not itself write to the user's project
file or mark it saved.

## Scheduling and restoration

Request a checkpoint after a committed operation. Coalesce changes over a short,
bounded interval without postponing publication indefinitely during continuous
editing. One to two seconds is an initial scheduling candidate, not a guaranteed
durability deadline. Measure the age of the oldest unpersisted edit as well as
timer delay; a large operation or slow storage can extend the loss window.

Allow one storage write per drawing with only the latest pending snapshot. Bound
aggregate work across drawings so writes and retained snapshots cannot accumulate
without limit. Keep encoding, decoding, pixel copies, I/O and durability waits off
UI and input threads, and keep GPU waits off the render submission path.
Incremental storage still writes substantial data after an operation that changes
most pixels; background execution alone does not establish acceptable performance.

Flush accepted edits on backgrounding and orderly exit using the host's lifecycle
facilities. An exit flush waits asynchronously; failure does not silently confirm
that the latest work is safe. Abrupt termination relies on the last completed
publication, not on a shutdown callback running. Preserve existing save/close
ordering when the user cancels exit.

At startup, claim abandoned session records without taking a live owner's data.
Restore the active drawing first and admit inactive drawings within the existing
memory budgets. Validate artwork and prepare required GPU resources before
adoption. Preserve any drawing opened or edited while restoration is pending.

Record unfinished restore attempts so an offending drawing cannot repeatedly
crash startup. Preserve failed copies and allow opening the app without the
offending drawing. Surface recovery choices for failures rather than ordinary
restarts. Keep unreadable or unsupported records intact; this work does not add
old-format readers or migrations unless separately requested.

## Milestones after resumption

1. **Rebaseline storage.** Verify the landed format and host transports, measure
   existing checkpoint costs, and select the smallest storage change that meets
   the restart and performance requirements. Specify session state ownership,
   resource reachability, publication ordering and failure behavior before coding.
2. **Implement shared persistence.** Add coherent session snapshots, bounded
   persisted history, durable membership changes and storage ordering using the
   redesigned format's interfaces. Cover success, cancellation, stale completions,
   failure and retry in tests that run without a window.
3. **Integrate hosts.** Implement automatic startup restoration and lifecycle
   flushing on GTK, Web, Android, Apple and Windows through the shared policy.
   Remove obsolete normal-startup prompts and writers as their replacements land.
4. **Qualify and document.** Complete the journeys and measurements below. Update
   the document, format, persistence and platform guides with the implemented
   contracts; remove this plan when the work is complete.

## Acceptance and evidence

Follow the [testing guide](testing.md) for every changed crate and host. Required
journeys include multiple saved and unsaved tabs, restart without a prompt,
Save/Undo/Redo/restart, Save As, close with Save or Discard, cancelled close,
background termination and continued painting while a checkpoint is written.
Verify restored pixels, editable state, tab order, history and modified status.
Exercise recovery failures and affected UI in both light and dark themes.

Inject termination and write failures around resource writes, checkpoint
publication, save acknowledgement, document removal and cleanup. Cover disk-full
and quota failures, corrupt or missing resources, unsupported records, repeated
restore failure, inaccessible destinations, external file changes and concurrent
owners. Assert that restoration selects a complete durable state, acknowledged
closes stay closed and failed writes leave the previous checkpoint usable.

Measure startup readiness, durable checkpoint age, bytes written, peak memory,
disk growth and cleanup cost. Measure drawing, navigation and resumed input while
recovery writes and cleanup are active at each affected tier's target canvas.
Use the [performance targets](../PERFORMANCE_TARGETS.md) and
[measurement rules](../performance/measuring.md); record qualified results in
their tier tables and raw evidence under `artifacts/`. A short storage task or
passing model test does not establish the absence of frame or input regressions.
