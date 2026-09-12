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
