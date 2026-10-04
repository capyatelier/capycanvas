# Restoring the editing session

[Technical documentation](../README.md)

Restart restores open drawings without a recovery prompt. Quitting retains clean,
modified and untitled drawings. Explicitly closing a drawing still uses
Save/Discard/Cancel and removes it from the next session. A successful manual save
changes the saved checkpoint; it does not remove the drawing from the session.

Drawings restored after an interrupted session or from a previous checkpoint have
“(recovered)” in their tabs until a successful manual save, including clean drawings.
Orderly restarts preserve ordinary titles.
Unreadable drawings remain stored and require an explicit retry or discard.
Recovery never substitutes an empty drawing for a failed restore.

## Capture and completeness

[`SessionCapture`](../../crates/layer-ui/src/session_recovery.rs) freezes the
editor, bounded undo/redo, working selection and editing targets, camera, manual
save checkpoint, name, save destination and last successful export recipe and
destination at one committed owner boundary. Camera
position is a document point at the viewport center, so a changed window size
does not move the drawing away. Workspace and preference state use their existing
stores. Incomplete gestures, previews, file requests and GPU handles are transient.
A pending manual save can checkpoint committed edits without acknowledging that
save; active gestures and other file operations still block capture.

The [`private codec`](../../crates/layer-core/src/package/session.rs) shares the
portable artwork record and resource adapters. It retains typed record identities,
tombstones and checkpoint identities across history. Object versions, raster index
chunks and immutable resource payloads are shared across historical states. The
last export's embedded ICC profile uses the same resource inventory and worker
transfer. Session stamps and metadata carry its stable identity; camera-only
checkpoints reuse its payload. Typed metadata keeps those resource owners attached
through preparation, storage and restoration. Private history is not added to
exported `.capy` files. The
[`worker transfer`](../../crates/layer-core/src/package/session_transfer.rs) carries
record changes and resource owners without replaying historical documents on the
input thread. Encoding, validation, decoding, storage and resource preparation run
on workers. `capture_artwork` captures portable artwork; session persistence
requires the distinct `capture_session` type, which includes private history.

The current artwork retains the last submitted frame's evaluation context.
History restores the corresponding authored output context before an artwork
transition, so removing an effect cannot leave a captured phase pointing to it.
Working-only undo and redo retain the captured context and the same artwork
checkpoint. This distinction applies to both archived sessions and worker
transfers without adding an undo step or changing the live editor.

Adding persistent state requires an explicit classification. Capture boundaries
destructure `UiSession`, `CanvasEngine`, `Editor`, `Document`, `WorkingState`, `UiState`, `DocumentFiles`,
`DocumentFileState` and `Camera` without a rest pattern. New fields therefore fail
compilation until they are persisted, reconstructed or deliberately classified as
transient. Edit conversion matches every `Edit` variant; record adapters and
constructors name all fields. Do not weaken these checks with `..`, wildcard match
arms, ignored decode errors or default values for missing private records.

Stored records are strict and bounded. Invalid checkpoint identities, unresolved
references, unsupported records, conflicting immutable resources and invalid
history fail before adoption. The editor's existing history limits apply to
restored history; a reader does not silently trim history to make it fit. These
checks enforce known invariants; they are not a proof against every possible
hardware or implementation failure.

## Durable publication and ownership

[`SessionManifest`](../../crates/layer-ui/src/session_recovery.rs) owns tab order,
selection, membership and unfinished restore attempts. The native window holds an
exclusive lease; web uses browser locks. A live owner's session cannot be claimed
by another window. Session and artwork identities are distinct from runtime tab
numbers. Window generations and runtime identities stay within the exact integer
range shared with JavaScript; exhausted counters fail instead of rounding or wrapping.

Publish `stage` membership before the first checkpoint of a new drawing. If the
process stops before that checkpoint, the indexed row is retained as unavailable;
if its checkpoint completed, startup can find it. A complete checkpoint cannot
become an invisible orphan between the drawing and window publications.
`reconcile` refuses implicit removal or identity rebinding. Only explicit `remove`
closes a row, and explicit validated `remap` changes runtime identities while
preserving durable keys when restoration races with new work.

The native [`SessionStore`](../../crates/layer-core/src/package/session_store.rs)
publishes immutable resource files, then generation metadata, then an atomic head
replacement. Each file and the containing directory are synchronized in order.
The head retains current and previous generation identities and SHA-256 checksums
of their exact metadata bytes. Decode and cleanup verify these checksums before
using metadata references. Interrupted publication leaves
a complete old or new checkpoint; validated fallback to the previous generation
is treated as recovery. Membership has no previous-generation fallback, which
would resurrect deliberately closed drawings.
Publication errors retain whether rename completed. A close cannot be cancelled
after its membership replacement became visible; a subsequent synchronization
error is reported while the accepted close completes. Republishing identical
membership retries directory synchronization.

Web stores resources and checkpoint heads in IndexedDB transactions. Browser
transaction failure leaves the previous committed state. Original project files
are separate from private session storage and are never overwritten by recovery.

Cleanup retains current and previous generations plus resources pinned by readers
and accepted work. Resource identity reuse requires matching descriptors and bytes.
Retirement is durable and terminal: a stale job cannot commit to a retired key.
Before removing membership, publish a retirement intent. The intent leaves an
indexed drawing usable, including after a cancelled or failed close. Once the
authoritative manifest excludes its key, cleanup makes retirement terminal, even
if the process stopped between membership removal and resource cleanup. Explicit
discard can prepare this intent without decoding a damaged checkpoint.
Cleanup only removes unindexed stores with that intent, proven retired, or without
a published head; an unexplained head is preserved. Only a typed file-not-found
error establishes absence; access errors stop destructive cleanup. Lock files remain
to avoid replacing a lock inode under another process. After readers release retired resources, cleanup removes
the empty resource and generation directories. A closed drawing keeps only its
directory, permanent empty lock file and empty terminal marker. These filesystem
objects accumulate with closed drawings; artwork, history and generation files do
not. Completed tombstones are skipped without lock claims, rewrites or directory
synchronization. GTK checks reachability across every window manifest and
fails closed when any manifest is unreadable.

## Scheduling, restart and close

Hosts coalesce changes on a two-second schedule and skip unchanged session stamps.
Stamps include artwork identity, editor and working-state generations, the save
checkpoint and persisted camera/file state. Clean drawings, selection changes and
camera-only changes are included. Unchanged resource payloads are reused. Each
drawing has one accepted checkpoint operation; a completion acknowledges its
captured state, never later edits. Slow storage or a large operation can extend
the unprotected interval beyond the scheduling delay.

Orderly exit waits asynchronously for accepted edits to become durable before
marking a clean exit. Storage failure keeps the window open. Backgrounding requests
a flush but abrupt termination always depends on the last completed publication.
Browser unload cannot guarantee an asynchronous flush; pending work keeps the
browser's leave warning enabled.

Before decoding a drawing, publish its unfinished restore marker. Each attempt
carries its publication generation, so a late completion cannot finish a later
retry of the same drawing. Clear the marker only after the live owner adopts it.
A crash quarantines all unfinished attempts;
explicit retry clears that drawing's blocked state. Failures keep the stored data.
Startup adoption verifies the original blank session stamp. If the user has already
opened or edited a drawing, restored drawings append without replacing that work.
Inactive drawings are parked and admission checks apply before publication.

Explicit close prepares fallible owner changes before durable membership removal.
After removal, completion must not fail while leaving a live drawing without its
session membership. Resource retirement follows the accepted close; any retained
reader keeps its immutable resources alive until released. Native publication
errors distinguish failure before replacement from failure to synchronize a
successfully replaced index. The latter accepts an already published explicit
close and reports the durability error. New drawing checkpoints still require
fully synchronized membership. Retrying an identical index synchronizes its
directory again before acknowledging it.

Save destinations include an expected fingerprint of the bytes opened or last
successfully saved. Restoration checks the original on a worker before adoption.
A clean drawing stays clean only when its original matches; a missing, changed,
inaccessible or unverifiable original requires a save or explicit discard before
closing. The saved checkpoint and history remain intact, and undo cannot clear
this protection. Restart itself does not require a dialog.
Before replacement the host verifies the destination again.
A missing, changed or inaccessible original requires Save As while preserving the
private drawing. Provider permissions remain host-owned; a stored URI alone does
not establish write access. Fingerprints detect changes, but cannot make an
external application's concurrent write atomic with our provider transaction.
Android providers may truncate a destination while copying a completed save.
A manual save therefore makes the private editing checkpoint durable before
opening that destination for writing. A checkpoint failure leaves the original
untouched; interrupted provider writes can restore the private copy. This uses
the same checkpoint writer and keeps the session marked active until orderly exit.

## Regression checks

The [testing guide](../development/testing.md) lists host and device checks.
Private persistence tests exercise every edit family through undo, redo and a
branched history, transferring to a worker, storing, restoring and transferring
back. Storage tests interrupt publication before and after durability boundaries,
kill subprocess writers, corrupt resources and heads, exercise lock contention,
retain live readers during collection and check bounded generations over repeated
edits. UI tests cover manual-save checkpoints, title state, camera, strict records,
membership staging/remapping, stale completion and interrupted restore batches.

Captured-frame context tests retain finite filter phases while removing and
restoring their effects through undo and redo, with a working-only entry first
in each history direction. Both archives and direct worker transfers preserve
the current view, history length and artwork checkpoints across these transitions.

Keep these tests when adding features. A new edit needs meaningful semantic
assertions for both sides of its undo/redo boundary; a shutdown/restart smoke test
alone does not establish that its private representation is complete. Platform
fixtures must also cover new drawing first publication, close cancellation and
failure, dirty and clean restart, and work begun during restoration. Run UI journeys
in both themes and measure painting and navigation while writes are active using
the reference devices in the [performance targets](../PERFORMANCE_TARGETS.md).
