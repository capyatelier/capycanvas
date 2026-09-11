# Apple persistence

Settings and workspace persistence use the existing versioned Rust models and
restore actions. Both native apps share storage and owner coordination.
The shared editable `Project` codec and fresh Metal replay checks now exist.
File menus and artwork recovery are still required; the running apps do **not**
yet save artwork. See [the shared format](../../docs/project-format.md).

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
cargo test -p layer-host workspace_persistence --lib
```

The standalone tests use temporary directories. They verify complete old/new file
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
bounded/coalesced storage work under sustained edits, document save/open/recovery,
and storage overhead in the hardware performance workloads.
