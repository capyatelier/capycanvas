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
Other drawings can reopen and continue checkpointing. If none can reopen, the
new drawing uses a different session identity; its checkpoints and explicit close
preserve the failed copies. Recovery never substitutes an empty drawing for a
failed restore. Retrying a failed drawing appends it without replacing work begun
since launch.

On Apple, an invalid drawing list preserves its entire directory before a fresh
session is created. Missing membership does not authorize deletion of drawing
heads. A temporary read or permission failure stays retryable in place. Known
per-drawing failures are recorded separately from authored data, so the next
restart can show the original cause. Dismissing a warning changes its visibility,
not its stored drawing or recovery status.

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

Checked layer rows, their range-selection anchor and Solo's previous visibility
state live in `WorkingState`. Private checkpoints retain them on both sides of
structural edits and visibility changes, so restart and undo/redo restore the
same checked rows and Solo toggle. Row navigation does not add an undo step.
The image selection and the canvas view origin live there too. The view origin
is the signed integer frame shift accumulated by crops and grows; it replaces a
saved composition origin. History records its change with each canvas edit, so
Undo and Redo after recovery move the camera with the frame's top-left while
artwork coordinates stay unchanged. Renderer rebuilds compare the canvas extent
and source domains as well as the origin, so a right- or bottom-only crop also
rebuilds.

The [`private codec`](../../crates/layer-core/src/package/session.rs) shares the
portable artwork record and resource adapters. It retains typed record identities,
tombstones and checkpoint identities across history. Object versions, raster index
chunks and immutable resource payloads are shared across historical states.
Immutable images are interned by portable ID across all states. Changed-record
transfers carry each value’s image dependency closure, including images retained
only by undo. Conflicting descriptors for one image ID reject the session;
foreign imports remap identities. Portable saves retain authored image uses,
while private history retains deleted uses until their undo owners are released. The
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
That history boundary is prepared even when the renderer supplies its final
evaluation context after the editor snapshot has been captured.
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
history fail before adoption. Incremental worker decoding validates each restored
artwork graph and editing target before accepting its history entry. The editor's
existing history limits apply to
restored history; a reader does not silently trim history to make it fit. These
checks enforce known invariants; they are not a proof against every possible
hardware or implementation failure.

## Private custom programs

Private `capy.effect/2` has two exclusive alternatives: the portable inline
built-in descriptor and all `values`, or `{program,values}` with an embedded
validated custom program. Each application owns its optional consumed spatial
reference; neither alternative creates a definition identity. Built-ins resolve
through the catalog; decoded custom descriptors and their code/table resources
are interned across current artwork, history and worker transfers.

Custom filters are not yet part of the portable format. A writer refuses to save
artwork that uses one, and a reader opens a package containing one as preserved.
The private session and worker formats keep them with the grammar below until a
portable custom filter contract is designed. Custom programs have a stable
`key`, evaluation `contract`, `kind`, `code`, `entry`, keyed `parameters`,
ordered `slots` and literal labels, and execute independently of built-in shader
fusion. Their `slots` fixes the shader layout.
Their parameter dimensions use `scalar`, `count`, `angle`, `time`, or `length` with a
`source_pixels`, `composition_pixels` or `normalized` reference. Built-in dimensions
come from the current catalog; a displayed unit never controls resizing.
Counts require whole bounds and values. A pixel length accepts any value from zero
to 65,536 pixels (or the declared bounds when wider, keeping a negative minimum's
sign), independently of its declared `min`/`max`. Canvas resizing composes the application’s authored spatial mapping without
rewriting its values or reference extent. Values remain finite and within the
accepted data range. Positive periods
used as divisors have a numerical floor of 1/256 pixel; this prevents undefined
zero-period patterns and overflowing integer noise coordinates. The floor affects
evaluation only, and never rewrites the authored value.
The custom evaluation contract `capy.filter/1` fixes shader ABI `5`; artwork has
no separate `abi` field. This contract is independent of built-in parameter
versions. Unknown custom contracts remain preserved.

Custom program parameters are a map keyed by stable keys, with required `kind`,
`default`, `label`, optional `opaque` (default false, color only) and dimensional
declarations. Parameter `kind` is an object tagged by `kind`. Number adds required
finite `min`,`max` and optional semantic `unit` (default empty); `min<=max`.
Count parameters require whole bounds and values. Choice adds required
`options`, a nonempty array of unique stable literal strings or `{value,label}`
objects (at most 256). Other kinds add no kind fields. Values satisfy their kind:
number within bounds, choice one declared option, curves/gradients 2–32 strictly
increasing points/stops in `[0,1]` with endpoints zero and one. Curve ordinates
are within `[0,1]`. LUT resource type and declared working-color binding are
validated together. Custom labels are literal strings. Built-in translation keys
are never saved. Pages, sections, conditional visibility, slider bounds/mapping,
steps and decimal places belong to runtime editor presentation. Custom artwork
opens with plain controls; count controls use whole-number steps.

Custom program `passes` retain `entry` and `sampling` (`neighborhood` with `radius`,
`parameter` with `key`,`scale`,`padding`, or `document`). `lookups` retain code
resource refs, `entry`, ordered parameter `dependencies`, `values`,
`workgroup_size`, `workgroups`. `auxiliary` is `lut3d` with local `resource` and
`color_space` keys, or `analysis` with kind `local_illumination` or `dehaze`.
`constraints` retain kind `ordered_numbers`, `lower`, `upper`,
`gap`. These local strings are not cross-object references. Missing lists are
empty, missing auxiliary is absent. Required scalar fields are not silently
replaced from a newer catalog. Type and dimensional additions are unsupported
until explicitly interpreted by the reader. Runtime `resolution` and
`constant_color` optimization declarations are omitted; reopened custom programs
evaluate at native resolution through their retained code.

## Durable publication and ownership

Sessions are device state: they live in each platform's state folder and are
not backed up ([where the app keeps its files](storage.md)).

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

Apple retains the expected prior membership across live adoption until the
restore acknowledgement is durable. The next restore or checkpoint finishes
that publication before reading membership again. If editing began during
restoration, incoming drawings append with noncolliding runtime identities;
already live drawings keep their durable keys.

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

Apple performs this sweep after a committed checkpoint, outside the startup
path. Cleanup failure is a warning about retired files; the acknowledged drawing
remains saved and later edits can checkpoint again.

## Scheduling, restart and close

Hosts coalesce changes on a two-second schedule and skip unchanged session stamps.
Stamps include artwork identity, editor and working-state generations, the save
checkpoint and persisted camera/file state. Clean drawings, selection changes and
camera-only changes are included. Unchanged resource payloads are reused. Each
drawing has one accepted checkpoint operation; a completion acknowledges its
captured state, never later edits. Slow storage or a large operation can extend
the unprotected interval beyond the scheduling delay.
Committed cleanup warnings do not acknowledge edits that arrived during the
write. A waiting flush repeats until its current stamp is durable, or reports
failure at its deadline; autosave schedules the remaining edits independently.

Orderly exit waits asynchronously for accepted edits to become durable before
marking a clean exit. Storage failure keeps the window open. Backgrounding requests
a flush but abrupt termination always depends on the last completed publication.
Browser unload cannot guarantee an asynchronous flush; pending work keeps the
browser's leave warning enabled. On the web, so does a changed drawing never saved to a
file while the browser has not agreed to keep the site's storage.

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
in each history direction, including renderer contexts supplied after capture.
Both archives and direct worker transfers preserve
the current view, history length and artwork checkpoints across these transitions.

Keep these tests when adding features. A new edit needs meaningful semantic
assertions for both sides of its undo/redo boundary; a shutdown/restart smoke test
alone does not establish that its private representation is complete. Platform
fixtures must also cover new drawing first publication, close cancellation and
failure, dirty and clean restart, and work begun during restoration. Run UI journeys
in both themes and measure painting and navigation while writes are active using
the reference devices in the [performance targets](../PERFORMANCE_TARGETS.md).
