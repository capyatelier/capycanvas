# Apple persistence

[Capy Canvas for macOS and iPadOS](README.md)

Settings use the versioned Rust model and atomic JSON files; workspaces use the
shared SQLite library. Both apps expose New, Open, Save and Save As with the
shared [`Project` format](../../docs/reference/project-format.md), profiled image
export, and private recovery copies of unsaved artwork. Window close uses the
shared unsaved-change decision; macOS also protects app termination.

**File → New Window** opens another editor on both platforms. Each scene owns its
document, camera and Undo history, and closing one leaves the others open. An
iPad without multiple-window support reports that limitation.

## Files and threads

- Everything lives in the app's own Application Support directory:
  `settings.json` for app settings, `workspaces.sqlite3` for the workspace
  library, and `recovery/` for recovery copies. Debug builds can redirect this
  with `CAPY_PERSISTENCE_NAMESPACE` or disable it with
  `CAPY_DISABLE_PERSISTENCE=1`; Release builds ignore both.
- `EditorPersistence` uses one background I/O queue per process. Neither the UI
  thread nor the render owner reads or writes files. The render owner reserves
  its first operation for restoration; queued input, fixtures and surface
  attachment follow it.
- JSON preferences and recovery indexes are limited to 1 MiB. A preference
  write syncs a private temporary file, renames it atomically and syncs the
  directory before acknowledging. Rust keeps the settings request pending until
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
  replaces the window's current document. Undo back to the saved checkpoint marks
  the document clean. New, Open and close decisions wait for an idle canvas, and
  a late result cannot change a replacement document.
- External Open and recovery reserve their pending URL before shared busy state
  arrives, so nothing else can overtake them. Delivery during launch waits for
  the first editor state, native startup, workspace restoration and
  `shaders_ready`.
- macOS attaches `NSOpenPanel` and `NSSavePanel` to the owning window. iPad uses
  `UIDocumentPickerViewController`; Save As prepares the archive in a private
  temporary directory before presenting the export picker. A save is
  acknowledged only after the destination write or picker export succeeds.
- One owner captures immutable document and source metadata. Pruning,
  compression, file coordination and GPU preparation run on a worker that holds
  no live editor pointer. Open prepares a candidate GPU session before adoption,
  keeps the window's settings and workspace, and starts fresh history.
- Security-scoped access and `NSFileCoordinator` surround file operations. The
  owner keeps the picker URL for later saves; the shared URI is an identity, not
  an access grant. Writes stream into a system replacement directory on the
  destination volume, sync, then publish with `FileManager` replacement.
- Image export captures an immutable project and GPU reference and encodes
  through the shared Float32 snapshot worker, never through the display cache.
  It never renames the drawing or marks unsaved edits as saved. Export presets
  and ICC library copies are saved atomically in the private persistence root.
- Image imports use the same coordinated reader through ImageIO decoding, and
  the bridge rejects a result that arrives after the document was replaced.
- Not handled: restoring a provider URL across launches, provider conflicts and
  file presenters. Recovery restores a private copy, not access to the original
  destination.

## Artwork recovery

- Unsaved changes schedule a recovery capture. Each editor keeps one capture in
  flight plus the latest wanted revision. Capture keeps the last committed raster
  during a stroke and prepares queued pen-up work without a drawable. The full
  project format is reused; Undo history is not stored.
- Copies live in `recovery/<runtime UUID>`, separate from the scene ID, so a new
  process's blank canvas cannot overwrite an earlier drawing. Each archive is a
  generation; its contents and directory are synced before an atomic
  `current.json` publishes it, and older generations are reclaimed afterwards. A
  failed write keeps the previous generation. Corrupt records are reported and
  left in place. No provider URL, bookmark, account or hardware identifier is
  stored.
- **File → Recovered Drawings…** and the launch prompt list copies not owned by a
  live editor. Opening one goes through the normal replacement flow; recovered
  content stays unsaved until a manual save. The copy remains until the new owner
  publishes its own copy or the user saves or discards.
- Lifecycle flushing waits for preferences, workspace writes and the recovery
  manifest before acknowledging. On iPad it runs inside a background task whose
  expiration handler ends the task synchronously on the main actor. A kill before
  publication leaves the previous complete copy.
- Shared `layer-ui::recovery::RecoveryState` decides checkpoint freshness,
  replacement order and close resumption; Swift executes its tickets with the
  atomic file helpers.
- GPU loss or an uncaptured validation error suspends the session and retires
  the renderer on a worker. The editor keeps the CPU document, sources, rasters,
  history and settings, cancels any unfinished contact, and offers Restart Canvas
  or Save As. Restart rebuilds the document through the shared
  renderer-replacement API.

## Lifecycle

Close flushes accepted edits before releasing the workspace claim; a failed
release keeps the close pending. Sleep and iPad backgrounding suspend input and
flush; activation revalidates ownership before editing resumes. Discarded iPad
scenes attempt a final workspace close and keep their recovery copy.

## Checks

```sh
bash apps/layer-apple/scripts/test-persistence.sh
bash apps/layer-apple/scripts/test-project-files.sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/recovery.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-coordinator.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
python3 apps/layer-apple/scripts/test-project-access.py
python3 apps/layer-apple/scripts/test-recovery-interruption.py
python3 apps/layer-apple/tests/background-expiration.py
cargo test -p layer-apple tests::recovery -- --test-threads=1
cargo test -p layer-apple renderer_failure_retains -- --test-threads=1
cargo test -p layer-apple -p layer-workspace -p layer-ui -p layer-host --features layer-workspace/native
```

`tests/sdr-recovery.swift` covers P3/U8 and ProPhoto/U16 projects with retained
photos and corrections; run it with `CAPY_TEST_ASSETS_APP` set. The XCTest
journeys `testSettingsAndWorkspaceRestart`, `testArtworkRecoveryAfterRestart`,
`testIndependentEditorWindows`, `testFailedProjectOpenPreservesArtwork` and
`testRendererRecovery` cover the same contracts through the native UI. None of
these checks covers physical background-task expiration or file-provider
delivery.
