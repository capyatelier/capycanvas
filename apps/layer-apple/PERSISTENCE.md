# Apple persistence

Settings and workspace persistence use the existing versioned Rust models and
restore actions. Both native apps share storage and owner coordination.
Both apps expose New, Open, Save and Save As using the shared editable
[`Project` format](../../docs/reference/project-format.md), plus PNG export. Both protect
window close with the shared unsaved-change decision; macOS also protects app
termination. Both also maintain private recovery copies of unsaved artwork.

**File → New Window** opens another editor on both platforms. Each scene owns
its document, camera and Undo history. Closing one scene leaves the other scenes
open. The command is also available through shortcut and toolbar customization;
an iPad environment without multiple-window support reports that limitation.

## Workspace library

Both Apple apps use the shared SQLite workspace library by default. Shared
Swift manager pages follow the compact workspace design: New, Switch and
Rename/Delete. New Workspace copies the current layout and tool settings, with
independent history and no saved-layout selector. Toolbars expose the
shared current-workspace and saved-toolbar actions. Rust owns the rows, action
availability, forms, validation and history policy. Row actions use metadata
summaries without loading every item's retained layout history.

Initialization seeds the shared Painter, Illustrator and Photographer workspaces
idempotently. Stable IDs drive the header switcher, including current edited names.
The defaults are editable and undeletable; existing user data and the last active
workspace survive upgrades. Reset All Brushes locks the editor at an idle boundary,
calls Rust's reset operation, then captures and flushes the current working values.
It creates no layout-history event and leaves other workspaces unchanged.

Selecting a workspace row previews its arrangement in the actual editor
before an explicit Switch. Layout History uses the same preview
mechanism. Every durable capture still sees the layout from before preview;
Cancel restores that layout and Restore commits one undoable change. Closing
or suspending the scene cancels a pending preview.
The workspace browser initially selects the current workspace; filtering clears
selection and preview. Late or rapid selection replies cannot revive a dismissed
preview. Separate Save/Load Layout UI and Apple bridge operations have been
removed. Existing shared template records remain preserved for core migration.

The native package transport supports coordinated `.capyworkspace` and
`.capytoolbar` delivery and consistent database backup. Opening a legacy
`.capytemplate` reports that loading saved layouts is no longer available.
These storage capabilities are covered by direct integration checks; the compact
workspace screens follow the revised shared design without storage-administration,
import/export, trash or metadata/version-management controls.

`NativeWorkspaceLibrary` has a separate serial Dispatch queue; SQLite runs on
the shared Rust storage worker. The drawing owner only captures or adopts
validated workspace state. `WorkspaceLibrary` serializes explicit transitions,
coalesces autosave, renews leases, revalidates suspended owners, and includes its
writes in the editor persistence barrier. Startup blocks new editor input until
restoration completes. Ownership recovery blocks new pen contacts and shortcuts
while allowing existing contacts and property corrections to finish. It retains the outgoing workspace
until storage and adoption acknowledge the transition. Failed ownership retains
in-memory changes for Save as New Workspace or a current-state export.

Migration reads unacknowledged `workspaces/<scene>.json` and `workspace.json`
sources on the storage queue. It preserves distinct scenes, aliases an identical
fallback, commits mappings with the imported records, and retains the original
files. Acknowledged files are no longer read or written. Unknown/corrupt inputs
fail before creating default workspaces. Concurrent imports reconcile both
identical and partially overlapping source sets through the shared manager.

The direct checks use temporary storage and both Apple platform configurations:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-library.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-coordinator.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
cargo test -p layer-apple -p layer-workspace -p layer-ui -p layer-host --features layer-workspace/native
```

They exercise latest-edit switching, failed-transition unlock, migration and
restart beside another owner, toolbar metadata/versions, toolbars, layout
history, import/export packages, trash, consistent SQLite backup and recovery as
a new workspace after a competing owner claims an expired lease. A deliberately
locked temporary database verifies that the drawing owner still serves edits
and queries, and that later edits survive the earlier save acknowledgement.
This is integration evidence, not physical lifecycle or sustained frame-rate
acceptance. Raw logs remain in ignored local artifacts.

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
contains shared app settings. `workspaces.sqlite3` holds named workspaces,
layout/history, latest tool/color/Zen values, reusable templates/toolbars,
metadata history and scene bindings. Legacy scene/default JSON files are retained
as migration inputs and are no longer written by normal app launches. Tests can
explicitly disable the library to exercise the legacy adapter.

Settings commits propagate to the process's other owners. Pending local writes
defer incoming notifications; owners converge to the newest successful commit.
A failed local save retains the accepted in-memory edit and offers Retry Save.
Workspaces remain independent after their initial copy. A live workspace's
Duplicate operation captures its owning window's latest accepted
state instead of copying a stale on-disk version. Switch to Window focuses that
owner through a small AppKit/UIKit adapter.

Layout persistence excludes in-flight gestures, measurements and scroll
allocations. Committed layout history and latest working tool values have
independent generations. Motion/camera publications do not schedule storage.
Autosave coalesces full editor publications, queries unchanged layouts without
copying their history, and performs database work off the input owner. Startup
does not overwrite invalid or unsupported saved data with defaults; the storage
service retains current-state and consistent database exports for recovery.

JSON preferences and legacy migration inputs are limited to 1 MiB per file.
Preference writes sync a private temporary file, atomically rename it, then sync
the directory before acknowledgement. Settings requests remain pending in Rust
until the write completes. Packages use coordinated, bounded file I/O outside
the input owner; destination cancellation or failure does not acknowledge an
export or replace existing bytes.

Close flushes accepted edits before releasing the fenced workspace claim.
A failed release keeps close pending; canceling close revalidates ownership
before editing resumes. App sleep and iPad backgrounding suspend input and flush
within the platform's available lifetime; activation revalidates before editing.
Discarded iPad scenes attempt a final workspace close and preserve artwork
recovery. Teardown and an explicit Quit Anyway release their own claims without
overwriting saved copies. An abrupt process kill can still retain only the last
completed save, with ownership recovered after the shared lease expires. Full
physical lifecycle/expiration coverage remains an acceptance requirement.

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
