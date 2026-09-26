# Apple persistence

Settings use the versioned Rust model and atomic JSON storage; workspaces use
the shared SQLite library. Both native apps share storage and owner coordination.
Both apps expose New, Open, Save and Save As using the shared editable
[`Project` format](../../docs/reference/project-format.md), plus profiled image export. Both protect
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

Initialization seeds the shared Sketch, Paint and Photo workspaces idempotently.
Stable IDs drive the header switcher. Included workspaces keep their names and
cannot be deleted; their layouts and tool settings remain editable. Existing SQLite workspace
data and the last active workspace retain their saved scene bindings. Reset All Brushes locks the editor at an idle boundary,
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

Externally opened `.capyworkspace` and `.capytoolbar` files use coordinated reads
on the file queue and shared Rust import validation. Apple no longer registers
the removed `.capytemplate` format. The compact workspace screens follow the
shared design without storage administration, package pickers/exporters, trash
or metadata/version-management controls; their obsolete routing and presentation
state are removed. Layout History keeps the existing preview/Cancel/Restore flow.
The lower-level storage service retains consistent SQLite backup and package
serialization, with direct integration coverage.

`NativeWorkspaceLibrary` has a separate serial Dispatch queue; SQLite runs on
the shared Rust storage worker. The drawing owner only captures or adopts
validated workspace state. `WorkspaceLibrary` serializes explicit transitions,
coalesces autosave, renews leases, revalidates suspended owners, and includes its
writes in the editor persistence barrier. Startup blocks new editor input until
restoration completes. Ownership recovery blocks new pen contacts and shortcuts
while allowing existing contacts and property corrections to finish. It retains the outgoing workspace
until storage and adoption acknowledge the transition. Failed ownership retains
in-memory changes for Save as New Workspace.

Workspace startup uses the SQLite scene binding and shared default catalog.
The Apple legacy JSON writer, migration scan and migration bridge requests are
removed. Old `workspaces/<scene>.json` and `workspace.json` files are ignored and
left untouched; malformed obsolete files cannot block current-library startup.
Current SQLite errors still surface without replacing the stored data. Shared
migration code used by other hosts is unchanged.

The direct checks use temporary storage and both Apple platform configurations:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-coordinator.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
cargo test -p layer-apple -p layer-workspace -p layer-ui -p layer-host --features layer-workspace/native
```

They exercise latest-edit switching, failed-transition unlock, startup and
restart beside another owner, obsolete-file isolation, toolbar metadata/versions, toolbars, layout
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
External Open and recovery reserve their pending URL before shared busy state
arrives; another Open, recovery or window close cannot overtake that reservation.
If a queued command makes Open unavailable, the document service reports the
rejection and releases the URL so the next request remains usable.
File delivery during launch waits for the first editor state, full native startup
and workspace restoration. Metal attachment alone is insufficient: first-frame
bundled-filter validation can still begin afterward and temporarily block Open.
The existing `shaders_ready` flag covers that startup interval.
The same pending URL resumes from existing state
publications and workspace initialization, without a startup timer or another
picker. Local checks cover delivery before the first snapshot and before Metal
attachment with temporary managed workspaces on both Apple configurations,
including the interval before first-frame catalog validation. Actual cold and
warm OS URL delivery also passes on Mac and the physical iPad with synthetic
painted documents and unchanged source bytes. Mac retains the first document in
its own window when warm delivery creates another. These checks do not establish
the full file-provider or lifecycle matrix; evidence and current scope are in
the [Apple handoff](../../docs/development/apple-handoff.md).

macOS attaches NSOpenPanel/NSSavePanel to the owning document window, whose native
title follows the current filename. Returning through the Window menu also
returns to that drawing's pending panel. iPad uses UIDocumentPickerViewController:
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

Image export captures an immutable project and GPU reference, then uses the shared
Float32 snapshot worker for previews and PNG/TIFF/JPEG encoding. It never narrows
through the display cache. Output profile, depth, size, matte, intent, dithering
and resolution use the shared export recipe; untouched matching retained sources
preserve exact integer samples, including hidden RGB. The captured job survives
later drawing or owner destruction. Export excludes viewport inspection aids and
does not rename the editable document or mark unsaved edits as saved. The options
sheet dismisses before native delivery: Mac chooses a destination first; iPad
stages the chosen image format before presenting its export picker.

The same serial file worker atomically saves shared export presets and exact ICC
library copies in the editor's private persistence root. Destination choices are
remembered only after successful delivery; named presets require explicit changes.
ICC copies use content hashes, bounded reads and shared validation. Removing a
library copy leaves original files and profiles embedded in projects/presets
intact. Missing first-run preferences use shared defaults; invalid existing data
reports an error rather than silently substituting a profile.

Security-scoped access and NSFileCoordinator surround file operations. The native
owner retains the actual picker URL for subsequent Save; the shared URI is an
identity, not an access grant. Successful Open replaces that retained destination,
and cancelling Save As preserves the previous one. Regular writes stream into a
system-provided replacement directory on the destination volume, sync the archive,
then publish with FileManager replacement/move. A file-only grant does not permit
creating arbitrary siblings or opening the parent directory for syncing. Private
recovery manifests still use their own directory-synced atomic publication.
Cancellation wins before the replacement boundary. File-provider export is
completed by the native picker. Source/sample allocations are shared
with the snapshot, but large-document capture cost, GPU preparation, memory peaks
and storage latency still require measurement.

This check writes through a real file-only App Sandbox grant:

```sh
python3 apps/layer-apple/scripts/test-project-access.py
```

Image layer imports use that same coordinated reader, including its substituted
URL, throughout ImageIO decoding. This follows Apple's
[external-document access requirements](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller).
The layer name comes from the selected URL. The picker captures the drawing's
epoch, and the Rust bridge rejects a late result after document replacement;
ordinary edits in the same drawing remain allowed. The local owner fixture
holds a coordinated image write, verifies that decoding waits for completed
bytes, and covers read/decode errors, document replacement, retry and history.
That check does not establish cloud-provider delivery or native picker behavior.

Restoring a provider URL across launches, provider conflicts/file presenters,
interruption during provider access, iPad multi-window lifecycle, and physical
background-task expiration remain open. Recovery uses a private copy and does
not restore access to the original provider destination.

## Artwork recovery

Unsaved changes schedule a recovery attempt after two seconds. Each editor keeps
one capture/write in flight and the latest desired document revision; edits during
a write schedule a subsequent snapshot instead of queuing full document copies.
Capture retains the last committed raster boundary while a stroke is active;
transforms and other non-capturable canvas operations still defer it. Queued
pen-up work is prepared without a drawable, including raster reconstruction
after renderer replacement. Pruning, compression and disk operations use
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

Successful publication and discard also reclaim UUID-named private temporary
files left by a terminated writer. Cleanup is confined to that runtime's recovery
folder after the manifest is durable; unrelated names and the current generation
are preserved. It does not scan document destinations or other runtime folders.

**File → Recovered Drawings…** is shared by the iPad menu and Mac OS File menu.
Available copies are also offered after launch. Archives belonging to other live
owners are excluded. Open dismisses the picker before entering the existing
Save/Discard/Cancel replacement flow. A prepared candidate is adopted only if the
current document still matches its approved epoch and revision. Recovered content
is unsaved and has no user destination; even Undo back to its initial state cannot
mark it clean. A successful manual save restores ordinary checkpoint behavior.
The source recovery remains until the new owner publishes its own durable copy
or the user saves/discards. A cancelled replacement leaves both drawings intact.

Lifecycle flushing prepares a capturable committed snapshot without acquiring a
drawable, then waits for preferences, workspace writes and the recovery worker's
atomic manifest publication. Preparation alone never acknowledges durability.
This includes pen-up queued immediately
before a surface stops. Mac cleanup follows actual window close or final accepted
application termination; iPad cleanup follows an authorized scene's view detachment.
Cancelled termination does not consume recovery copies. The barrier retains the
editor and recovery coordinator through the final acknowledgement, even if scene
teardown releases the last UI reference before native preparation returns. Local
checks on both Apple configurations verify that this flush finishes, releases
its owner and leaves an archive that reopens with the accepted edits. The iPad uses
the OS background-task allowance. Its expiration handler ends that allowance
synchronously on MainActor, as required by
[UIKit's callback contract](https://developer.apple.com/documentation/uikit/uiapplication/beginbackgroundtask(withname:expirationhandler:)).
The same idempotent lease handles a later persistence completion without ending
the task twice; expiration does not acknowledge a successful save. The focused
check compiles the exact production helper against a synchronous UIKit-shaped
boundary and covers both callback orders, failed/immediate completion, invalid
identifiers and overlapping scenes. It fails before removal of the queued
`Task` and passes afterward; it does not simulate actual OS expiration:

```sh
python3 apps/layer-apple/tests/background-expiration.py
```

A kill or allowance expiration before durable
publication can still leave only the previous completed copy. The local process-
interruption check now verifies this publication contract and cleanup below.
Physical lifecycle/expiration delivery and sustained storage overhead remain
acceptance work.

GPU loss and uncaptured validation errors suspend the shared session and retire
the failed renderer on a worker. CPU document state, embedded sources, committed
rasters, history and working settings remain owned by the editor. An unfinished
contact is cancelled. The canvas error offers Restart Canvas and Save As; restart
uses the shared renderer-replacement API to reconstruct the retained document.
Callbacks from a retired device cannot stop its replacement. A nonblocking owner
health check detects failures even after the display link becomes idle. Native
thumbnail and filter-preview generations reset when renderer availability changes.
Layer thumbnails also retire pending readbacks on document-epoch changes, even
when the replacement renderer is already ready; this covers ordinary New/Open
and direct document adoption in one publication path.
Healthy native surface replacement retains the existing GPU.

## Ownership and files

`EditorPersistence` uses one background I/O queue for the process. The render
owner reserves its first operation for restore, while disk reads run separately.
Queued input, fixtures and surface attachment follow validated restoration.
Neither the UI thread nor the active render owner performs file reads/writes.

Files live under the app's own Application Support directory. `settings.json`
contains shared app settings. `workspaces.sqlite3` holds named workspaces,
layout/history, latest tool/color/Zen values, reusable templates/toolbars,
metadata history and scene bindings. The drawing owner persists settings only;
workspace capture/adoption and storage use the library coordinator. Tests can
disable the library to isolate document or settings behavior without enabling
a second workspace store.

Settings commits propagate to the process's other owners. Pending local writes
defer incoming notifications; owners converge to the newest successful commit.
A failed local save retains the accepted in-memory edit and offers Retry Save.
The native-owner fixture enumerates all Settings rows on both Apple policies.
Every editable preference passes a value change, restoration in a fresh owner
and exact durable Reset without changing unrelated settings; unavailable Mac
native prediction is separately verified off and disabled. These are storage
and bridge checks, not native widget or physical input acceptance.
Workspaces remain independent after their initial copy. A live workspace's
Duplicate operation captures its owning window's latest accepted
state instead of copying a stale on-disk version. Switch to Window focuses that
owner through a small AppKit/UIKit adapter.

Layout persistence excludes in-flight gestures, measurements and scroll
allocations. Committed layout history and latest working tool values have
independent generations. Motion/camera publications do not schedule storage.
Autosave coalesces full editor publications, queries unchanged layouts without
copying their history, and performs database work off the input owner. Startup
does not overwrite invalid settings or SQLite data with defaults; the library
service retains current-state and consistent database exports for recovery.

JSON preferences and recovery indexes are limited to 1 MiB per file.
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
overwriting saved copies. An abrupt process kill retains only the last completed
save. The shared native store holds an OS file lock while any client is open;
the first opener after all clients exit clears abandoned claims before restoring
the saved workspace. A quick restart therefore does not select another preset
while waiting for the old lease. If another client is still open, normal leases
and fencing continue to protect its work. Full physical lifecycle/expiration
coverage remains an acceptance requirement.

## Checks

Editor construction waits for the resolved `SceneStorage` identifier. Creating
an owner from an eager UUID could leave a restored Mac scene displaying one
identity while opening a different workspace binding. The native Paint check
requires automatic workspace restoration for the same scene; a new system scene
can reopen the saved workspace through the ordinary switcher. The complete OS
window/scene restoration matrix remains acceptance work.

On an Apple Silicon development Mac, run from the repository root:

```sh
bash apps/layer-apple/scripts/test-persistence.sh
bash apps/layer-apple/scripts/test-project-files.sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/recovery.swift
python3 apps/layer-apple/scripts/test-recovery-interruption.py
cargo test -p layer-apple project_ --lib
cargo test -p layer-apple renderer_failure_retains -- --test-threads=1
cargo test -p layer-apple ui_actions_change_only_the_addressed_apple_session
cargo test -p layer-host workspace_persistence --lib
```

The focused `testIndependentEditorWindows` UI check is shared by both Xcode test
targets. It activates ordinary in-app New Window and Close toolbar commands,
checks independent layers and Undo in two scenes, closes the second scene and
continues editing the first. Test applications use isolated persistence and
register termination at teardown, including when an assertion fails.

The native Mac `testSDRWindowSurfaceTransitions` check also passes minimize,
hide, narrow resize and restoration with a ProPhoto U16 drawing. It verifies
the same scene/workspace, profile/depth, layers, live Navigator, sampled artwork,
continued mouse input and Undo/Redo after each transition. Closing Document
Properties must finish ownership revalidation and leave the editor enabled
without a recovery warning. The native full-screen/return check passes on the
same runtime. Evidence is `artifacts/apple-mac-surface-v1/`; sleep/wake and actual
cross-display transitions remain separate acceptance cases.

The project-file checks use the actual Swift owner, coordinator and Metal C ABI
with injected location choices. They cover both Mac destination-first saves and
iPad staged exports, Save/Open/New, private permissions, cancellation, failed
writes/reads, changes queued before capture/close, and unsaved Save/Discard/Cancel.
The checks also cover custom canvas dimensions, cancelled creation, PNG decode
and export checkpoint preservation. They exercise editor effects without
system-menu automation. The focused `testNewDrawingAndExportCancellation` UI
check exercises the size form and native export cancellation separately.
Physical file-provider delivery remains unverified.

`testFailedProjectOpenPreservesArtwork` also passes through the native Mac File
menu, panels and error sheets. After an unsaved Clear and approval to Discard,
an invalid file leaves the current name, layers, sampled artwork and Undo/Redo
intact. The next Open still prompts for unsaved changes; Cancel preserves them,
and retrying the valid file restores its saved artwork. Both source files remain
unchanged. The local Swift/Metal owner fixture verifies the same failed-Open,
dirty-state, history and retry-cancellation contract on both Apple policies.
These checks add local failure/retry coverage, not cloud-provider or physical
UIKit picker acceptance. Evidence is `artifacts/apple-file-failure-v1/`.

The 2026-09-16 iCloud attempt retained scoped image-import data/history and
provider metadata results, but the system access-consent prompt covered the
editor and blocked project workflows. Automatic approval review rejected the
broader OS permission. The user then explicitly deferred iCloud testing because
it does not work in their environment. No grant or full cloud-provider pass is
claimed; the unused provider fixture and permission handler are removed. Local
file acceptance remains unchanged. Evidence is `artifacts/apple-provider-native-v1/`.

Document adoption now evaluates prediction with the receiving window's platform
and live native capability. The file worker's generic preparation session must
not switch an iPad window back to the saved manual lookahead. Shared Open/recovery
regressions and both-policy Apple/Metal preview/history checks cover this path.
Evidence is `artifacts/apple-prediction-adoption-v1/`.

Apple now uses `layer-ui::recovery::RecoveryState` through opaque C-ABI state.
The shared policy owns checkpoint freshness, durable replacement before origin
retirement, clean-copy retirement and native close resumption. Swift executes
the tickets with the existing atomic file helpers and supplies owner observations,
timers and lifecycle barriers. Cancelling app-wide close leaves ordinary captures
in unprepared windows alone. Accepted storage work finishes before an actual
prepared close is resumed. File formats, runtime storage identities and guarded
generation removal are unchanged; no alternate recovery path is introduced.
See the [integration record](../../docs/development/apple-handoff.md#shared-recovery-policy--2026-09-17).

The recovery checks use actual Swift owners and the Metal bridge for both platform
policies. They verify private file modes, cancelled capture, newest-revision flush
under queued edits, stale removal, malformed-record isolation, failed-write retry,
owner loss/restart, migration and Save/Discard/Cancel protection. The Swift fixture
also queues real ink and pen-up without a following drawable, then requires the
complete store barrier to publish that revision. The Rust recovery
check drains pen-up without a drawable, compares exact recovered GPU pixels and
checks that only a durable manual save clears the recovered document's dirty state.
Save-before-recovery checks preserve the selected archive through both Mac saves
and iPad staged exports, without requesting another Open location.

`tests/sdr-recovery.swift` extends the production coordinator checks to P3/U8 and
ProPhoto/U16 projects containing a retained 16-bit photo, integer paint, an
Exposure correction and its mask. Both Apple policies pass a suspended-workspace
flush without another drawable, release and fresh-owner restoration, tagged paint
settings and continued correction editing/Undo. The entire manifest (apart from
session revision) and compressed payload remain exact, as does the original photo.
Run it with `test-project-files.sh` and `CAPY_TEST_ASSETS_APP` pointing to a built
Mac app containing the production filter assets. This is Mac Metal owner evidence;
physical background expiration and provider delivery remain separate checks.

The hardware Rust renderer test covers explicit suspension, device destruction
and an actual uncaptured validation error on both Apple presets. It verifies an
unfinished contact is cancelled, retained sources and raster pixels reconstruct
exactly, saving works while stopped, pending requests and working settings survive,
and Undo/Redo and later painting remain correct. Faults affect only the isolated
editor device. They do not reset the system GPU.

The focused native `testRendererRecovery` exercises both device loss and
validation errors through the visible restart control. It compares artwork
pixels and Undo/Redo, and waits for visible layer thumbnails to regenerate.
The retained catalog must complete each replacement GPU's staged startup;
otherwise idle thumbnail work remains deferred indefinitely.

The focused `testArtworkRecoveryAfterRestart` UI check passes on Mac and the
connected iPad. It waits for a completed private copy, terminates and relaunches the
app, opens the offered drawing after document readiness, and verifies the restored
layer count and a new recovery copy. It uses an isolated persistence namespace
and actual in-app controls. This checks completed-copy restart; it does not model
a physical background-task expiration or a kill during publication.

`test-recovery-interruption.py` supplies the missing local process-kill check.
It compiles the production file helpers and uses real shared recovery tasks for
both Apple policies, with disposable layer edits and no Metal surface or UI.
Each writer is stopped and its actual disk state rechecked before killing only
that child: archive temporary output, a different pending manifest, a published
replacement, and a published discard. Fresh processes must read exactly a
complete old/new archive or the discard; retry, abandoned-file cleanup, preserved
unrelated files and stale-removal protection also pass. The initial run reproduced
temporary files surviving retry; the final run passes after private cleanup was
extended. Evidence is `artifacts/apple-recovery-interruption-v1/`. This qualifies
local publication under process termination, not iPadOS expiration, power loss,
provider delivery, or the full editor lifecycle.

The standalone settings tests use temporary directories. They verify complete old/new file
generations under concurrent reads, private permissions, size limits, failed-write
preservation, malformed-file handling, per-scene isolation, settings notifications
and flush ordering. The real-owner checks use the actual Swift owner and Rust C
ABI for both platform configurations, including immediately queued edits, scene
restart, rapid edits across owners and forced write failure followed by retry.
They require no UI automation and do not establish platform lifecycle delivery.

The focused `testSettingsAndWorkspaceRestart` UI test passes on Mac and the
connected iPad. It saves a dark theme and visible Color panel, waits for write
acknowledgment, terminates the app, and verifies both after relaunch without
fixture actions. Debug test namespaces are private and isolated from user state.
Other UI fixtures disable persistence explicitly. Release builds ignore all
persistence test environment variables.

Remaining acceptance includes interrupted/background/termination delivery on
physical devices, workspace retention across the complete window/surface matrix,
bounded storage work under sustained workloads, interrupted recovery and provider delivery,
and storage overhead in the hardware performance workloads.
