# Apple persistence

Settings and workspace persistence use the existing versioned Rust models and
restore actions. Both native apps share storage and owner coordination.
Both apps expose New, Open, Save and Save As using the shared editable
[`Project` format](../../docs/project-format.md), plus PNG export. Both protect
window close with the shared unsaved-change decision; macOS also protects app
termination. Automatic artwork recovery remains required.

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

These manual operations do not implement artwork autosave or recovery. Restoring
an artwork URL across launches, provider conflicts/file presenters, interruption
during provider access, iPad multi-window lifecycle, and physical background-task
expiration remain open. Enabling multiple iPad scenes is not lifecycle acceptance.

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
Workspaces remain independent after their initial copy. Native multi-scene
lifecycle acceptance, including iPad window-management support, remains open.

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
finishes; document journaling/recovery remains part of the broader goal.

## Checks

On an Apple Silicon development Mac, run from the repository root:

```sh
bash apps/layer-apple/scripts/test-persistence.sh
bash apps/layer-apple/scripts/test-project-files.sh
cargo test -p layer-apple project_ --lib
cargo test -p layer-host workspace_persistence --lib
```

The project-file checks use the actual Swift owner, coordinator and Metal C ABI
with injected location choices. They cover both Mac destination-first saves and
iPad staged exports, Save/Open/New, private permissions, cancellation, failed
writes/reads, changes queued before capture/close, and unsaved Save/Discard/Cancel.
The checks also cover custom canvas dimensions, cancelled creation, PNG decode
and export checkpoint preservation. They exercise editor effects without
system-menu automation. The focused `testNewDrawingAndExportCancellation` UI
check exercises the size form and native export cancellation separately.
Physical file-provider delivery remains unverified.

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
bounded/coalesced storage work under sustained edits, document recovery and provider delivery,
and storage overhead in the hardware performance workloads.
