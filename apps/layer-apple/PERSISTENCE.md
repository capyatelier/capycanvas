# Apple persistence

Settings and workspace persistence use the existing versioned Rust models and
restore actions. Both native apps share storage and owner coordination.
Both apps expose New, Open, Save and Save As using the shared editable
[`Project` format](../../docs/project-format.md), plus PNG export. Both protect
window close with the shared unsaved-change decision; macOS also protects app
termination. Both also maintain private recovery copies of unsaved artwork.

**File → New Window** opens another editor on both platforms. Each scene owns
its document, camera and Undo history. Closing one scene leaves the other scenes
open. The command is also available through shortcut and toolbar customization;
an iPad environment without multiple-window support reports that limitation.

## Artwork files

The shared Rust document request flow owns busy state, save checkpoints and
Save/Discard/Cancel decisions. Apple opts into replacing the current document;
GTK can create another window. Undo returning to the saved checkpoint marks the
document clean again, while later edits remain unsaved after an older snapshot
finishes writing. New/Open and close decisions require an idle canvas. A late
open result or input from the previous document cannot change a replacement.

macOS uses NSOpenPanel/NSSavePanel. iPad uses UIDocumentPickerViewController:
Save As prepares an archive in a private temporary directory before presenting
the export picker. Save acknowledgments follow the completed destination write
or successful picker export, never location selection alone. Cancelled or failed
writes preserve the current document and its saved checkpoint. Open validates
and prepares a candidate GPU session before adoption; it retains the window's
settings/workspace and starts fresh undo history. New presents a native size form
using shared labels, defaults (2048×1536) and bounds (1…8192 pixels per dimension).
Rust validates the dimensions again before preparing the replacement canvas.
Bundled filter loading populates the library without migrating embedded document
definitions. File capture waits asynchronously for that preparation to finish.

One owner captures immutable document/source metadata. Pruning, validation,
compression, file coordination and GPU preparation run off the drawing queue.
The worker borrows file descriptors, holds no live editor pointer, and uses a
reserved stack for recursive shader translation. Retired document/GPU resources
are also released off the owner queue. Input revision and animation clocks reset
when a prepared document enters the existing window.

PNG export submits a document-sized sRGB conversion and GPU buffer copy in the
owner queue, then transfers the readback ticket to the file worker. Shader
compilation, GPU completion waits, pixel packing and PNG encoding run outside
the input owner. The ticket retains its captured pixels after later drawing or
renderer destruction. GTK and Apple use the same RGBA8/sRGB PNG encoder. Export
excludes viewport inspection aids and does not rename the editable document or
mark unsaved edits as saved. Mac chooses a destination first; iPad stages the
PNG before presenting its export picker.

Security-scoped access and NSFileCoordinator surround file operations. Regular
writes stream into a private sibling temporary file, sync, rename and sync the
directory. Cancellation wins before the atomic publication boundary. File-provider
export is completed by the native picker. Source/sample allocations are shared
with the snapshot, but large-document capture cost, GPU preparation, memory peaks
and storage latency still require measurement.

Restoring a provider URL across launches, provider conflicts/file presenters,
interruption during provider access, iPad multi-window lifecycle, and physical
background-task expiration remain open. Recovery uses a private copy and does
not restore access to the original provider destination.

## Artwork recovery

Unsaved changes schedule a recovery attempt after two seconds. Each editor keeps
one capture/write in flight and the latest desired document revision; edits during
a write schedule a subsequent snapshot instead of queuing full document copies.
Capture requires an idle committed document and waits while a stroke, transform
or other canvas operation is active. Pruning, compression and disk operations use
the existing project worker. Recovery neither creates a manual Save request nor
acknowledges its checkpoint. The full editable project format is reused, including
embedded source assets, masks and effects; undo history is not stored.

Files live in private `recovery/<runtime UUID>` directories under Application
Support. The runtime identity is separate from the restored scene ID, so a new
process's initial blank canvas cannot overwrite a previous drawing. Each archive
has a generation UUID. Its contents and directory are synced before a small
atomic `current.json` manifest publishes that generation; obsolete generations
are reclaimed afterward. A failed or cancelled write preserves the previously
published generation. Stale removals check the manifest generation, and a durable
tombstone precedes archive deletion. Corrupt records remain untouched and are
reported without hiding other valid records. No provider URL, bookmark, account
or hardware identifier is stored in the recovery record.

**File → Recovered Drawings…** is shared by the iPad menu and Mac OS File menu.
Available copies are also offered after launch. Archives belonging to other live
owners are excluded. Open dismisses the picker before entering the existing
Save/Discard/Cancel replacement flow. A prepared candidate is adopted only if the
current document still matches its approved epoch and revision. Recovered content
is unsaved and has no user destination; even Undo back to its initial state cannot
mark it clean. A successful manual save restores ordinary checkpoint behavior.
The source recovery remains until the new owner publishes its own durable copy
or the user saves/discards. A cancelled replacement leaves both drawings intact.

Lifecycle flushing drains accepted input without acquiring a drawable, then waits
for preferences and the recovery barrier. This includes pen-up queued immediately
before a surface stops. Mac cleanup follows actual window close or final accepted
application termination; iPad cleanup follows an authorized scene's view detachment.
Cancelled termination does not consume recovery copies. Barriers retain the
coordinator if the UI owner disappears while its write completes. The iPad uses
the OS background-task allowance; a kill or allowance expiration before durable
publication can still leave only the previous completed copy. Full physical
expiration/interruption and sustained storage overhead remain acceptance work.

## Ownership and files

`EditorPersistence` uses one background I/O queue for the process. The render
owner reserves its first operation for restore, while disk reads run separately.
Queued input, fixtures and surface attachment follow validated restoration.
Neither the UI thread nor the active render owner performs file reads/writes.

Files live under the app's own Application Support directory. `settings.json`
contains the shared settings model. `workspaces/<scene UUID>.json` belongs to one
system-restored scene. `workspace.json` is the last committed layout used to seed
a new scene. A new scene saves its own initial snapshot without replacing that
default, so another window's changes cannot alter its future restore.

Settings commits propagate to the process's other owners. Pending local writes
defer incoming notifications; owners converge to the newest successful commit.
A failed local save retains the accepted in-memory edit and offers Retry Save.
Workspaces remain independent after their initial copy. Full native multi-scene
lifecycle acceptance, including physical iPad window management, remains open.

Rust's durable workspace view excludes in-flight layout gestures, measurements
and scroll allocations. The native host emits it only when committed topology
changes; camera patches and ordinary brush edits do not trigger workspace writes.
Startup does not overwrite an invalid saved model with defaults. Read failures
are reported, and the original file remains available for repair.

Each JSON file is limited to 1 MiB, matching the existing settings reader's limit.
Writes create a private temporary file in the destination directory, sync its
contents, rename it atomically, then sync the directory before acknowledgment.
Files use mode 0600 and newly created storage directories use mode 0700. Settings
requests remain pending in Rust until their write completes. Scene and default
workspace files are individually atomic; failure updating the default is reported
even if the scene file was saved successfully.

Lifecycle flushing places barriers across the owner and I/O queues. iPad uses a
background-task allowance; Mac waits before responding to application termination.
Failed saves are visible in the editor or active settings sheet and can be retried.
These adapters do not guarantee completion after a forced kill before a write
finishes; recovery preserves the last completed generation in that case.

## Checks

On an Apple Silicon development Mac, run from the repository root:

```sh
bash apps/layer-apple/scripts/test-persistence.sh
bash apps/layer-apple/scripts/test-project-files.sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/recovery.swift
cargo test -p layer-apple project_ --lib
cargo test -p layer-apple ui_actions_change_only_the_addressed_apple_session
cargo test -p layer-host workspace_persistence --lib
```

The focused `testIndependentEditorWindows` UI check is shared by both Xcode test
targets. It activates ordinary in-app New Window and Close toolbar commands,
checks independent layers and Undo in two scenes, closes the second scene and
continues editing the first. Test applications use isolated persistence and
register termination at teardown, including when an assertion fails.

The project-file checks use the actual Swift owner, coordinator and Metal C ABI
with injected location choices. They cover both Mac destination-first saves and
iPad staged exports, Save/Open/New, private permissions, cancellation, failed
writes/reads, changes queued before capture/close, and unsaved Save/Discard/Cancel.
The checks also cover custom canvas dimensions, cancelled creation, PNG decode
and export checkpoint preservation. They exercise editor effects without
system-menu automation. The focused `testNewDrawingAndExportCancellation` UI
check exercises the size form and native export cancellation separately.
Physical file-provider delivery remains unverified.

The recovery checks use actual Swift owners and the Metal bridge for both platform
policies. They verify private file modes, cancelled capture, newest-revision flush
under queued edits, stale removal, malformed-record isolation, failed-write retry,
owner loss/restart, migration and Save/Discard/Cancel protection. The Rust recovery
check drains pen-up without a drawable, compares exact recovered GPU pixels and
checks that only a durable manual save clears the recovered document's dirty state.
Save-before-recovery checks preserve the selected archive through both Mac saves
and iPad staged exports, without requesting another Open location.

The focused `testArtworkRecoveryAfterRestart` UI check passes on Mac and iPad
Simulator. It waits for a completed private copy, terminates and relaunches the
app, opens the offered drawing after document readiness, and verifies the restored
layer count and a new recovery copy. It uses an isolated persistence namespace
and actual in-app controls. This checks completed-copy restart; it does not model
a physical background-task expiration or a kill during publication.

The standalone settings tests use temporary directories. They verify complete old/new file
generations under concurrent reads, private permissions, size limits, failed-write
preservation, malformed-file handling, per-scene isolation, settings notifications
and flush ordering. The real-owner checks use the actual Swift owner and Rust C
ABI for both platform configurations, including immediately queued edits, scene
restart, rapid edits across owners and forced write failure followed by retry.
They require no UI automation and do not establish platform lifecycle delivery.

The focused `testSettingsAndWorkspaceRestart` UI test runs on Mac and iPad
Simulator. It saves a dark theme and visible Color panel, waits for write
acknowledgment, terminates the app, and verifies both after relaunch without
fixture actions. Debug test namespaces are private and isolated from user state.
Other UI fixtures disable persistence explicitly. Release builds ignore all
persistence test environment variables.

Remaining acceptance includes interrupted/background/termination delivery on
physical devices, workspace retention across the complete window/surface matrix,
bounded storage work under sustained workloads, physical recovery and provider delivery,
and storage overhead in the hardware performance workloads.
