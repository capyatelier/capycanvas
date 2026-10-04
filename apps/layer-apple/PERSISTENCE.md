# Apple persistence

[Capy Canvas for macOS and iPadOS](README.md)

Settings use the versioned Rust model and atomic JSON files; workspaces use the
shared SQLite library. Both apps expose New, Open, Save and Save As with the
shared [Capy package](../../docs/reference/capy-package.md), profiled image
export, and private checkpoints for open drawing sessions. Explicit window close uses the
shared unsaved-change decision. App termination preserves the open session.

**File → New Window** opens another editor on both platforms. Each scene owns its
document, camera and Undo history, and closing one leaves the others open. An
iPad without multiple-window support reports that limitation.

## Files and threads

- Shared Rust names every store ([where the app keeps its files](../../docs/internals/storage.md)).
  `StorageLocations` resolves them once per process:

  | Folder | Contents |
  | --- | --- |
  | `Application Support/<bundle id>` | `settings.json`, `export-presets`, `color-profiles/`, `workspaces/workspaces.sqlite3` |
  | `Application Support/<bundle id>/State` | `sessions/<scene UUID>/` drawing checkpoints; excluded from backups |
  | `Caches/<bundle id>` | `shaders/` pipeline cache |
  | The app's temporary folder | Anonymous copies of opened `.capy` files and parked drawing tabs |

  On macOS these folders are inside the app's sandbox container.
  `CAPY_STORAGE_DIR` replaces all of them with one private folder; a relative
  name is a folder inside the app's temporary folder.
- `EditorPersistence` uses one background I/O queue per process. Neither the UI
  thread nor the render owner reads or writes files. The render owner reserves
  its first operation for restoration; queued input, fixtures and surface
  attachment follow it.
- JSON preferences and session indexes are limited to 1 MiB. A preference
  write flushes a private temporary file through the drive cache
  (`F_FULLFSYNC`), renames it atomically and flushes the directory before
  acknowledging. Rust keeps the settings request pending until
  then. A failed save keeps the accepted in-memory edit and offers Retry Save.
- Settings commits propagate to the process's other owners; pending local writes
  defer incoming notifications, and owners converge on the newest successful
  commit. Startup never overwrites invalid settings or SQLite data with defaults.

## Workspace library

- Rust's `WorkspaceController` owns rows, action availability, forms,
  validation and history; SQLite runs on the shared Rust storage worker, and the
  owner only polls its replies. `WorkspaceController.swift` sends inputs, ticks
  the controller and renders its view.
- The controller serializes transitions, autosaves after a quiet period, renews
  leases, revalidates suspended owners and saves before releasing a closing or
  suspended window. Startup blocks editor input until restoration completes.
  When another owner claims the workspace, the editor turns read-only and keeps
  its in-memory changes for Save as New Workspace.
- Startup prefers the scene's `apple:scene:<uuid>` binding, then the last used
  workspace, then the shared default. Editor construction waits for the resolved
  `SceneStorage` identifier so a restored Mac scene opens its own binding.
- The included Sketch, Paint and Photo workspaces are seeded idempotently, keep
  their names and cannot be deleted. Reset All Brushes runs at an idle boundary
  and adds no layout history.
- Selecting a workspace row or a Layout History entry previews it in the editor.
  Durable captures still see the layout from before the preview; Cancel restores
  it and Restore commits one undoable change. Closing or suspending the scene
  cancels a pending preview.
- Layout persistence excludes in-flight gestures, measurements and scroll
  allocations. Motion and camera publications never schedule storage.
- The store holds an OS file lock while any client is open. The first opener
  after all clients exit clears abandoned claims, so a quick restart does not
  fall back to another preset while the old lease expires.

## Artwork files

- Shared Rust owns busy state, save checkpoints and Save/Discard/Cancel. Apple
  opens drawings in the window's tab collection. Undo back to the saved checkpoint marks
  the document clean. New, Open and close decisions wait for an idle canvas, and
  a late result cannot change a replacement document.
- External Open reserves their pending URL before shared busy state
  arrives, so nothing else can overtake them. Delivery during launch waits for
  the first editor state, native startup, workspace and drawing-session restoration, and
  `shaders_ready`.
- macOS attaches `NSOpenPanel` and `NSSavePanel` to the owning window. iPad uses
  `UIDocumentPickerViewController`; Save As prepares the archive in a private
  temporary directory before presenting the export picker. A save is
  acknowledged only after the destination write or picker export succeeds.
- The serial owner captures `ArtworkCapture` with shared typed artwork roots,
  source metadata, the committed output context and its exact checkpoint.
  `PreparedPackage` enumerates resources, compresses and streams the final codec
  on the file worker, which holds no live editor pointer. Saves are source-only
  and do not require a rendered preview. Open uses the shared package reader and
  prepares a candidate GPU session before adoption, retaining the window's
  settings and workspace while starting fresh history.
- Unsupported packages and recovered previews use the shared read-only
  `PackageView`. The bridge exposes its summary and bounded preview without
  replacing editable artwork. Copy original streams the retained package bytes.
- Security-scoped access and `NSFileCoordinator` surround file operations. The
  owner keeps the picker URL for later saves; the shared URI is an identity, not
  an access grant. Writes stream into a system replacement directory on the
  destination volume, sync, then publish with `FileManager` replacement.
- Image export captures immutable artwork and a GPU reference and encodes
  through the shared Float32 snapshot worker, never through the display cache.
  It never renames the drawing or marks unsaved edits as saved. Export presets
  and ICC library copies are saved atomically in Application Support.
- Image imports use the same coordinated file worker and shared Rust decoder;
  the bridge rejects a result that arrives after the document was replaced.
- Not handled: restoring a provider URL across launches, provider conflicts and
  file presenters. Recovery restores a private copy, not access to the original
  destination.

## Drawing session restart

- Each system-restored scene uses its `SceneStorage` UUID for a private session
  directory. A permanent shared `SessionLease` excludes a second owner. The
  serial render owner captures the window's ordered drawing list and shared
  `SessionCapture` values; one file worker encodes and publishes them.
- A window whose own session has no drawings adopts the newest unlocked session
  that has drawings and renames it to its own. Artwork from windows the system
  did not restore (Close windows when quitting, Option-Quit, cleared saved
  state, iPad windows swiped away) therefore reopens in the next new window. On
  iPad a window adopts only while every open scene session is connected, so a
  background window can still reconnect to its own session.
- The private shared session codec preserves authored artwork, working selection,
  editing target, bounded Undo/Redo, camera, saved checkpoint, modified state and
  drawing names. Portable `.capy` exports remain artwork files. The old full
  package recovery folders, picker and opaque recovery-policy bridge are removed.
- `SessionStore` publishes immutable resources and checkpoint metadata before its
  atomic current/previous head. File and directory syncs precede acknowledgement.
  Unchanged resources are reused, and unchanged prepared metadata skips a drawing
  commit. New membership is registered before the first drawing checkpoint, so
  a crash cannot strand a durable drawing outside the index. The final index
  acknowledgement follows all drawing checkpoints. Explicit removal publishes
  first; drawing storage is retired after accepted close. A window manifest is never replaced by an
  older manifest when it is unreadable.
- The native two-second timer coalesces changes without restarting the timer for
  every edit. One window write runs at a time. Lifecycle barriers wait for the
  latest observed checkpoint and report failure after a bounded wait. Encoding,
  decoding, GPU preparation, storage and durability waits stay on workers.
- Startup restores drawing candidates through the shared GPU admission path,
  prepares the active drawing first, parks each inactive candidate and releases
  its GPU before preparing the next, then installs validated membership atomically.
  Apple currently waits for all tab candidates before showing the first restored
  canvas; it keeps at most the active GPU and one scratch GPU during preparation.
  Ordinary restart has no chooser. Abnormal restart uses the shared recovered tab
  title until a manual save. Input arriving during preparation cannot overwrite
  the live drawing through stale adoption.
- Restore attempts are recorded before decoding each drawing. An interrupted or
  failed attempt preserves its data and exposes Retry. Markers remain pending
  until actual owner adoption, including the worker-to-owner transfer. Retry uses the retained
  original session; a failed restore never publishes a blank session over it.
- Save destinations remain display identities after restart. A restored Apple
  drawing asks for a destination on Save because provider access/bookmarks have
  not been restored. Before adoption, the file worker attempts a security-scoped,
  coordinated read of each original and passes its fingerprint to shared Rust.
  A matching original preserves clean state; changed, missing or inaccessible
  originals require Save or Discard before close. Unavailable access does not
  prompt during restart. Private checkpoints never overwrite an original project.
- GPU loss retains the CPU document, sources, rasters, history and settings. The
  shared renderer replacement supports Restart Canvas or Save As.

## Lifecycle

Close flushes accepted edits before releasing the workspace claim; a failed
release keeps the close pending. Sleep and iPad backgrounding suspend input and
flush; activation revalidates ownership before editing resumes. Discarded iPad
scenes attempt a final workspace close and preserve their drawing session for
the next new window. macOS
Quit flushes open drawings and releases workspace owners without prompting to
save or retiring drawing membership. Explicit drawing/window close retains
Save/Discard/Cancel and publishes removal before native destruction.

## Checks

```sh
bash apps/layer-apple/scripts/test-persistence.sh
bash apps/layer-apple/scripts/test-project-files.sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-coordinator.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
python3 apps/layer-apple/scripts/test-project-access.py
python3 apps/layer-apple/tests/background-expiration.py
cargo test --locked -p layer-apple tests::session -- --test-threads=1
cargo test --locked -p layer-apple tests::document_tabs -- --test-threads=1
cargo test --locked -p layer-core package::session_store -- --test-threads=1
cargo test -p layer-apple renderer_failure_retains -- --test-threads=1
cargo test -p layer-apple -p layer-workspace -p layer-ui -p layer-host --features layer-workspace/native
```

The portable `tests::session` bridge journeys cover both Apple policies, exact
pixels, selection, Undo/Redo after restart, orderly restart, explicit removal,
failed peer checkpoint publication, stale adoption and new windows adopting
unrestored sessions. Tab checks cover cancelling
a prepared close before membership changes. The XCTest `testArtworkRecoveryAfterRestart` journey expects
immediate restored editing without a chooser. macOS/iPadOS builds, light/dark UI,
system scene restoration, background-task expiration and provider delivery still
require Apple hardware validation; Linux bridge checks do not qualify them.
