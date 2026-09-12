# Workspace manager: GTK implementation

Target: implement [the proposal](../ui/workspace-manager-proposal.md) on GTK,
validate it in the native application, and obtain user approval before adapting
other hosts. Publish notable, tested milestones to main and integrate concurrent
main changes. This record is a progress log, not a release acceptance claim.

## Milestone 1: shared state and restoration

Implemented layout-only durable history with independent undo/redo references,
retention of abandoned redo content, and string-encoded monotonic generations.
Latest working state captures tool selection, sparse brush overrides, colors,
region-tool settings, remembered figure/gradient choices, and Zen separately.
Prepared restoration validates before adoption and requires an idle document and
workspace. It does not replay tool commands or alter artwork history.

The legacy single-workspace API remains available to hosts awaiting integration.
New capture constructors distinguish template defaults from legacy Zen migration.
Full baseline reset is exposed as a new recoverable layout operation; the old
menu still awaits replacement by the manager milestone.

Validation on 2026-09-11:

- `cargo test --locked -p layer-ui --lib`: 254 passed, including new durable
  redo/restart, abandoned branch, independent working state, sparse override,
  reset, and guarded adoption cases.
- `cargo test --locked -p layer-core -p layer-engine`: 42 core and 35 engine
  tests passed; doc tests passed.
- `cargo check --locked -p layer-linux`: passed.

## Remaining implementation and acceptance gates

- Shared records/store requests and the native SQLite worker: atomic publication,
  independent layout/working generations, coherent reads, fenced ownership,
  idempotent receipts, failure/retry and migration contracts.
- GTK startup, switching, asynchronous autosave, close/suspend/reopen, independent
  windows and persistent save/error status. Validate losing owners, failed saves,
  rapid switches, document-operation guards and queued writes after deletion.
- Shared manager actions/presentation and GTK controls: workspace/template lists,
  creation, duplication, rename, reset, history, metadata recovery, template
  updates, Toolbar Library, Recently Deleted and keyboard operation.
- Resource lifetime, template/toolbar packages, full recovery backup round-trip,
  import validation, retention, quota/disk-full and storage-unavailable recovery.
- Native GTK interaction/restart verification and review of the rendered manager.
  Supply a runnable command and request user approval when GTK acceptance passes.

Web IndexedDB and other native host adaptation follow GTK approval. Their contract
requirements remain in the proposal; they are not claimed as implemented here.

## Milestone 2: native store foundation

Added `layer-workspace` for shared entity records, validation, prepared commit
batches and request/reply transport. The native feature supplies one SQLite worker
per private directory shared by window clients. Immutable layout and panel/toolbar
components are stored once; working values, layout navigation and metadata have
independent write generations. The database uses bundled SQLite, WAL and FULL
synchronization. Creation, binding changes, deletion and migration mappings commit
atomically. Leased ownership, fencing, pending deliveries and immutable operation
receipts protect against stale writers and retries after lost acknowledgements.

Fourteen real SQLite tests pass, including rollback after earlier SQL statements
succeed, repeated delivery, lost acknowledgements, concurrent claims, coherent
reads during writes, suspended-owner takeover, deletion/restore and replacement,
migration mapping, newer-schema preservation, corrupt metadata isolation,
lossless large generations, and reopening unavailable storage through the worker.
GTK autosave and manager integration, resource packages and retention are still
pending; these store tests do not constitute native application acceptance.
After integrating the concurrent GTK/Web tab-drag changes, combined validation
passed 255 shared UI tests and 14 native store tests, plus doc tests:
`cargo test --locked -p layer-ui -p layer-workspace --features layer-workspace/native`.
`cargo check --locked -p layer-linux` also passed on that combined tree.

## Milestone 3: GTK persistence and session coordination

GTK now opens the shared SQLite service, creates My Workspace from Default on
first launch, resumes the last workspace, and gives additional windows independent
workspaces. Accepted layout gestures and latest working values save asynchronously;
working saves debounce and also run periodically during long interactions. Close
waits for acknowledged writes. A failed save retains the pending immutable operation
and accepted newer values for Retry. Save errors remain visible; successful routine
saves do not add persistent chrome in Zen. Tests use isolated CAPY_WORKSPACE_DIR
directories; normal GTK storage is under its application data directory.

Shared coordination includes prepared switching, independent create/duplicate,
layout-only template creation, names, and ownership renewal. A shared session gate
prevents new document/tool input while workspace adoption is pending. The manager
UI, recovery packages, retention, and full suspend/takeover presentation remain
unfinished and are still required before GTK approval.

Validation:

- 256 shared UI and 17 store/coordinator tests pass, including edits arriving while
  an earlier save awaits acknowledgement, errors scoped to the affected operation,
  failed outgoing save/incoming load, template defaults, and independent histories.
- `native_workspace_database_resume_and_independent_windows` passed using actual
  GTK, Wayland and NVIDIA Vulkan with fatal GTK criticals enabled. It changes a
  layout and working settings, closes/reopens, verifies Undo → reopen → Redo, and
  confirms two windows keep independent working values. Artwork stays unchanged.
- Reviewed the native capture at `/tmp/capy-workspace-persistence.png`. Removed
  the routine saved-state row from Zen; persistent storage errors still appear.

After integrating the next Android and GTK/Web changes, 258 shared UI tests and
17 store/coordinator tests passed. The native test passed again in 4.38 seconds.
Its layout assertions compare durable fields, excluding transient GTK widget
measurements, which can arrive at different times after each realization.
