# Workspace manager: GTK implementation

Target: implement [the proposal](../ui/workspace-manager-proposal.md) on GTK,
validate it in the native application, and obtain user approval before adapting
other hosts. Publish notable, tested milestones to main and integrate concurrent
main changes. GTK implementation and automated/native acceptance checks are now
complete through milestone 5; user review and approval remain pending. This record
does not claim approval or acceptance of the other hosts.

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

## Remaining implementation and acceptance gates at milestone 1

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

## Milestone 4: GTK manager, reuse, and portable recovery

Connected the shared Workspace menu to a searchable GTK manager with workspace,
template, local-toolbar, library, and Recently Deleted views. Implemented named
creation, duplication, rename, template publication and updates, original-baseline
reset, layout history, metadata recovery, reusable versions, toolbar copies and
replacement, deletion with a selected replacement, restoration, and permanent
deletion. Copies from another window in the same process settle that window's
accepted edits before capture. Other processes remain fenced and require their
source window to close before a current snapshot can be duplicated.

Added separate hash-verified template, toolbar, and full workspace backup packages.
Backup import retains working values, baseline and history/navigation under fresh
local IDs; reusable imports contain layout/configuration only. Imports use unique
names and publish atomically. GTK supplies native file pickers and background
atomic file writes. Storage details offer backup export/import, in-memory recovery
as a new workspace, retry, and a preview before clearing eligible older history.

Shared retention protects current state, baselines, active navigation, and data
referenced by other items. Native maintenance runs at startup and periodically
while idle, defers other live owners, expires 30-day trash, collects unreachable
components, and bounds acknowledged delivery receipts. History usage is currently
reported conservatively from serialized revision sizes; shared component usage is
reported separately. Current supported toolbar controls reference built-in tools
and commands; portable packages carry their exact layout/toolbar definitions.

Native testing caught and fixed closing an unopened manager dialog, delayed list
replies overwriting Storage details, wrong sidebar selection for history, and
startup racing asynchronous filter preparation. Routine GTK measurements no longer
clone complete retained layout history on every presentation change.

Validation:

- 260 shared UI and 20 native store/coordinator tests pass, plus doc tests.
  New cases cover library copy/reset/undo, pinned resets after template update and
  deletion, backup navigation and fresh IDs, corrupt/missing/newer packages,
  failed publication, retention, shared resources, and deletion fences.
- `native_named_workspace_manager_templates_library_and_history` passed in 4.50s
  on real GTK/Wayland/Vulkan with fatal criticals enabled. It exercises naming and
  confirmation dialogs, independent working values, template creation/use, reset
  undo, toolbar-library copies/deletion/recovery, and the history/storage screens.
  The document remains unchanged. Reviewed manager and history captures in `/tmp`.
- `native_workspace_database_resume_and_independent_windows` passed again in 6.99s,
  including startup from the repository root with installed filter preparation.
- `cargo check --locked -p layer-linux` passed. Integrated incoming Apple, Windows,
  and Web changes through `56a897e` while retaining the GTK work.

### Remaining GTK acceptance audit at milestone 4

This milestone is not final GTK approval. Finish and validate interruption recovery
for non-autosave operations, storage-full cleanup/retry, recovery of unsupported or
corrupt database bytes, suspend/resume ownership revalidation, session-only close
recovery, and resource-publication failure boundaries. Check native file-picker
backup round trips and concurrent-owner failure presentation. Audit the proposal's
remaining details and action availability, including newer-template information,
before supplying the trial command and requesting approval for other hosts.

## Milestone 5: failure recovery and GTK acceptance

Completed ownership revalidation before resumed input, stale-owner input fencing,
and Save as New Workspace recovery without replacing a successor's state. Failed
close offers Keep Open, Discard Unsaved Changes, and Export Backup and Close;
cancellation or export failure keeps the window and restores document close guards.

Failed named operations retain their immutable deliveries for retry, including
failures before SQLite receives a write. Lost acknowledgements resolve receipts
before reporting failure. Interrupted publications can be recovered into uniquely
named independent copies; publication and cancellation of delayed original writes
commit together. The additive schema-2 migration preserves existing data and
operation hashes. Storage-full errors clean eligible history and retry the same
delivery, preserving protected data and pending edits if space remains insufficient.

Export Original Database uses SQLite's backup API on the worker, including WAL
and unsupported JSON/model records even when normal opening fails. Failed exports
preserve the source and existing destination. This is a repair backup, separate
from the portable Workspace Backup importer. A physically unreadable SQLite file
is preserved and reported; this export does not repair damaged SQLite bytes.

Added newer-source-template information and the action to create from its latest
version, plus the oldest retained history date in Storage details. Scoped GTK's
transparent window background and editor palette to editor windows, so native
dialogs retain opaque themed surfaces. Reviewed fresh manager, Layout History,
and native file-picker captures. The picker test now waits for GTK's initial
folder load and verifies its selected path before accepting.

Final validation, after integrating concurrent main through `9a1235e`:

- 260 shared UI and 30 native store/coordinator tests pass, plus doc tests.
  Recovery cases cover receipt loss, expired ownership, interrupted publication
  and recovery rollback, schema migration, database backup, unavailable storage,
  and SQLite disk-full retry using a constrained database page limit.
- All five native GTK tests pass in separate processes with isolated databases,
  Wayland, NVIDIA Vulkan, and fatal GTK criticals enabled: manager/templates/
  toolbar library/history (4.62s), restart and independent windows (6.47s), owner
  takeover and Save as New (3.07s), unavailable-storage close recovery (2.27s),
  and backup file-picker export/import with fresh identity (4.37s).
- `cargo build --locked --release -p layer-linux` passes, and `git diff --check`
  is clean. Native logs are in `/tmp/capy-gtk-final-kpd_q_sh`; shared test output
  is `/tmp/workspace-manager-tests.log`.

The proposal audit is complete for GTK and its shared/native storage scope.
GTK has no legacy on-disk workspace source to migrate. Apple/Android legacy
imports, Web IndexedDB, and host lifecycle adaptation follow GTK approval.
Current toolbar resources are built-in commands/tools; introducing a custom brush
library is separate from packaging the definitions supported by this editor.
Cross-process ownership is fenced, but focusing or duplicating another process's
live snapshot requires that source window to close; same-process windows route
snapshot capture through their owner. Retained-history usage is an estimate.
Fake-clock and native takeover tests do not establish physical suspend/power-loss
guarantees or cross-device synchronization.

To review, close an older running GTK instance and run from the repository root:

```sh
cargo run --locked --release -p layer-linux
```

Open Window → Workspace → Manage Workspaces. Review switching away and back,
template creation, reset/undo, toolbar reuse, and Storage and Backups. User approval
is the remaining GTK gate before adapting other hosts.

## User review: menu activation regression

User review found that Window and context menus would not open. The earlier
acceptance tests invoked manager actions directly and missed native pointer
activation. A new test reproduced the failure through Mutter → Wayland → GTK:
window activation began ownership revalidation and temporarily disabled the
editor, cancelling the same click that should open the menu. Popup focus changes
could repeat this interruption.

Ordinary activation now retains input when the ownership lease remains valid.
Expired or lost ownership still requires revalidation before editing. The pointer
test now opens Window → Workspace → Manage Workspaces, File, and the Layers tab's
right-click context menu. It passes in 5.12s; inspected the actual popup captures.
Native stale-owner takeover/Save as New still passes in 3.15s. The production
release build and script syntax checks pass. Concurrent Android main changes
through `bcfc885` were integrated before validation.
The concurrent GTK motion milestone through `5619a3a` was then integrated;
the complete pointer menu test passed again in 5.13s on that combined tree.

Reproduce the pointer regression check with:

```sh
bash apps/layer-linux/bench/workspace-menus.sh
```

GTK remains subject to user approval. Close the older running instance before
using the normal `cargo run --locked --release -p layer-linux` trial command.

## GTK redesign after user feedback

Implemented the latest requested menu order: Undo/Redo first, Workspaces,
flat panel rows, then Quick Access Toolbars. Generated panel/toolbar suffixes
are omitted from these rows. User-facing Template terminology is now Workspace
Template, including its introductory explanation.

Manage Workspaces is a single list with New, Switch, and Rename/Delete options.
Secondary library, deletion recovery, and storage pages are behind More options;
maintenance controls are collapsed. Workspace, toolbar, backup, and recovery
copy explains the user's task without storage implementation details.

Layout History is now one modal with real editor previews, Cancel, and Restore
This Version. Shared captures preserve the pre-preview layout and working state;
GTK blocks editor input/autosave and renews ownership while browsing. Restore
rolls back the temporary preview before committing one recoverable layout edit.
New history descriptions name the affected panel/toolbar. Older generic labels
can be recovered along retained navigation ancestry; unknown abandoned branches
remain honestly labeled Earlier layout.

Validation on concurrent main through `1b4dc00`: 264 shared UI and 30 native
store/coordinator tests pass. Five isolated GTK acceptance tests pass, including
15.12s manager/history/lease renewal, restart/independent windows, takeover,
unavailable-storage recovery, and backup picker round-trip. Native pointer tests
open Window → Workspaces and Quick Access Toolbars, File, and panel context
menus (5.80s). Screenshot inspection confirms opaque dialogs and a lighter
history backdrop. Logs: `/tmp/capy-workspace-redesign.wnLSpJ`,
`/tmp/capy-workspace-menus.bmCiGt`, and `/tmp/workspace-redesign-shared-tests.log`.

See [the review guide and screenshots](../ui/workspace-manager-gtk-redesign.md).
The release build and diff checks pass. This milestone is ready for user review;
approval before adapting other hosts remains outstanding.

Integrated the subsequent GTK/Web layer-hold milestone `ce3751b` before
publication. The combined release build passes, and the native pointer-menu
check passes again in 5.69s (`/tmp/capy-workspace-menus.Qa6ihE`).
