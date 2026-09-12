# Workspace manager proposal

**Historical proposal.** GTK is implemented and its design was approved on
2026-09-12. Start other-host work from the
[approved implementation handoff](workspace-manager-host-handoff.md) and
[GTK visual reference](workspace-manager-gtk-redesign.md). Those documents
supersede this proposal's UI, terminology, feature list, and implementation status.
In particular, do not reintroduce the removed backup/export, Recently Deleted,
metadata/version-management, or template-management screens described below.
The storage/ownership rationale remains useful background alongside current code.

Original design proposal, 2026-09-11, updated with the workspace-storage discussion.
Based on the reviewed working tree and the official product documentation linked
below. The remaining text records the original intended behavior and pre-implementation
code assessment. A future raster document format was assumed for
planning and is outside this workspace task.

## Recommendation

A workspace remembers where you left off. A template supplies a layout.
Workspace changes save automatically; templates change only through an explicit
action. Most users should only need to choose a workspace and occasionally reset
it. Creating and maintaining reusable templates belongs one level deeper.

Workspace history covers layout/customization and metadata. Live control values,
selected tools and colors retain only their latest state per workspace for resume;
they do not create history entries and are not restored by layout undo/history.

Templates currently capture layout and toolbar configuration only, excluding
brush settings, selected tools and current colors. Resetting to a template
restores that layout while preserving the workspace's working tool settings.

Toolbars belong to the workspace, including their customized contents. A separate
Toolbar Library lets users reuse saved toolbar configurations. Adding one creates
a workspace-owned copy; changing it does not change other workspaces or the library.

Use **Workspace**, **Template**, **History**, and **Previous Versions**. Avoid
profile, session, preset, inheritance, branch, commit, and copy-on-write in the
interface. Reserve **Export** for writing a portable file; creating a reusable
local template is **Save as Template…**.

## Current code

- `crates/layer-ui/src/workspace.rs`: `WorkspaceState` contains the layout and
  Zen state. The layout includes panel configurations and toolbar contents.
  `WorkspaceHistory` has separate in-memory undo/redo stacks and gesture grouping.
  It is not included in serialized workspace state.
- `crates/layer-ui/src/session.rs`: `durable_workspace()` excludes transient
  measurements and incomplete drags. `RestoreWorkspace` clears workspace history.
  The current API therefore cannot implement switching between named workspaces
  while retaining their individual histories without additional session support.
- `workspace_menu()` currently contains workspace undo/redo, panel and toolbar
  visibility entries, New Toolbar, and Manage Toolbars. The application Window
  menu consumes this same model.
- `crates/layer-ui/src/customization.rs`: the existing toolbar manager is a
  selection-and-delete list with visible/hidden descriptions. It provides shared
  presentation infrastructure, but is too limited to serve as the workspace
  manager unchanged.
- `DockLayout::reset_docking()` in `layout.rs` restores docking defaults while
  retaining customized panel/tool definitions. `ResetLayout` currently appears
  in the View menu through `VIEW_MENU`. This existing behavior must change:
  the requested reset includes toolbar customization as part of the workspace.
- GTK startup in `apps/layer-linux/src/canvas.rs` restores platform defaults.
  Web `app.js` saves/restores a current workspace through local storage. Named
  workspaces, template lineage, and a durable history browser are absent from
  the reviewed shared implementation.

The proposed workspace persistence expands beyond the current layout-only value
to include working tool settings. Artwork, document undo, and application/device
preferences remain separate. Templates deliberately retain the narrower layout
scope described below.

## Storage contract

| Stored entity | Contents |
| --- | --- |
| Workspace | Stable ID, name and metadata; versioned layout/customized toolbar configuration and its original baseline; a separate latest-only working-state record containing selected tool/preset, foreground/background colors, per-tool remembered values and active Zen state |
| Workspace history | Recoverable committed layout/customization revisions and meaningful action descriptions; shared references to unchanged layout and toolbar records; retained metadata changes and deletion recovery; no historical brush overrides, live control values, selected tools or colors |
| Workspace template | Name/description and retained revisions; panel membership, configuration, docking, sizes, tabs, visibility and collapsed state; toolbar membership, names, contents/order, display settings and placement; referenced resources needed by its configured controls |
| Toolbar Library entry | Reusable toolbar definition and metadata/history; tiles, dividers, order, display options and configured actions; placement is chosen by the receiving workspace |
| Custom brush/resource library | Custom/imported brush definitions and referenced textures/resources stored once; workspaces refer to those definitions and save their local overrides |

Workspace storage excludes drawing pixels, drawing undo, layer/filter property
values, GPU state, animation, hover, measured widget geometry and in-flight drags.
Layer opacity and filter strength are document values even when edited by sliders.
Tool settings are saved as semantic values, not serialized slider widgets or
derived UI view models. Changing a control updates its single owning setting;
multiple toolbars can expose the same value.

For built-in brushes, persist a sparse map keyed by stable brush and setting IDs.
Omit untouched brushes and settings. Resolve absent values from the installed
app's defaults. Keep a storage schema version for decoding/migration, but do not
version or retain historical built-in brush defaults: the user accepts that app
updates can change inherited values when resuming work.
This policy is separate from retaining revisions of user-created templates,
toolbars and brush definitions.

Entering a setting equal to today's default removes that setting's override;
an explicit reset does the same. Loading or saving another setting must not
silently remove existing overrides merely because a newer default matches them.
Stable setting IDs retain their meaning and units. Renamed/removed settings need
an explicit schema migration; unknown/newer payloads are preserved and reported,
never replaced by defaults and saved back over the original.

Templates exclude live brush values, selected brush/tool, foreground/background
colors, workspace history, device identities and active Zen mode. A toolbar tile
explicitly configured as a 20px size shortcut retains that parameter in a template;
the current value of a brush-size slider is workspace working state instead.
Template export carries its layout resources, not the entire brush library.

Proposed creation behavior: New Workspace from Template uses its layout, starts
with default tool state and empty brush overrides, and begins an independent
history. Duplicate Workspace copies current layout and working values, retaining
recoverable history. Save as Template extracts only layout/toolbar configuration.
Reset Layout restores only the original layout baseline, including all toolbar
customizations; working brush values, colors and the independent libraries remain
unchanged. Restoring a workspace-history revision also preserves latest working
values. The history browser is labeled Layout History to make this scope clear.
Opening an old layout as a new workspace uses a copy of the source workspace's
latest working values; it cannot reconstruct historical brush settings.
Its initial/reset baseline is the selected historical layout, and it starts a
new history. Names and other metadata are restored through explicit metadata
actions in item details; choosing Restore This Version in Layout History changes
layout only. Keep metadata revision recovery separate from layout undo traversal.

For eventual Android/desktop resume, transfer the workspace record, changed
component records and missing referenced resources. Initially each workspace has
one portable logical layout, fitted transiently to the current viewport. Separate
device-layout variants are a later feature with a separate ownership/history
contract. Device calibration and input prediction stay in device settings.
Transferring the drawing is a separate part of resume, and on-disk persistence
alone does not provide synchronization.

Persist each completed layout/customization gesture as one history change,
sharing unchanged records and writing component records plus the new current
layout revision atomically. Live adjustments update the in-memory working state
immediately and save its latest sparse map asynchronously with debouncing. Flush
at gesture completion and workspace switch, and on orderly suspend/close where
supported; save periodically during long interactions rather than relying on a
shutdown callback. Each durable write replaces the latest working record atomically.
There is no per-brush version chain or live-setting undo log. A save generation
can order writes or detect sync conflicts without retaining historical values.
The retention policy below protects current data and original layout baselines
while bounding eligible history. Native storage and web storage can differ
behind this contract.

Use one private SQLite workspace database per app installation
on Android, iOS/iPadOS and desktop, with JSON payloads and indexed ID/name/revision
columns. Shared Rust owns the workspace model, validation, history policy and
JSON payload migrations. A shared native storage implementation owns the SQLite
schema and transactions; native hosts supply the persistent sandbox directory
and lifecycle notifications.
Keep database I/O on a storage worker shared across windows. Android uses private
internal app storage; Apple uses Application Support; desktop uses its normal
application data directory. Web uses IndexedDB behind the same record contract.
This recommendation does not introduce another database server.

### Shared storage interface and two backends

Provide one narrow asynchronous workspace-store interface, with two implementations:

| Implementation | Platforms | Host-specific responsibility |
| --- | --- | --- |
| Native SQLite store in Rust | Android, iOS/iPadOS, macOS, Windows, Linux | Supply the private persistent directory and lifecycle events; present file pickers |
| IndexedDB adapter | Web | Execute storage transactions and return results through the browser/Wasm bridge; present browser import/export |

There is no separate database implementation for each native OS. Both backends
store the same versioned JSON payloads and return the same typed records/errors.
Rust owns application decisions on web too; the browser adapter contains storage
mechanics rather than a second workspace model or history implementation.
Native SQL migrations and IndexedDB object-store upgrades are backend-specific;
JSON model migrations and validation remain shared.

The interface supports listing workspace/template metadata, loading a workspace,
saving its latest working state, committing layout/metadata revisions, creating
or updating reusable templates/toolbars, and moving/restoring items in Recently
Deleted. Reads return the revision/generation needed to identify their result.
Writes carry the expected monotonic layout or working-state generation, allowing
the store to reject stale updates rather than overwrite newer data. Check these
preconditions inside the same transaction as the writes. A workspace load returns
its layout, baseline, history navigation, metadata and working state from one
consistent read transaction. An old content ID alone is not a concurrency token:
undo can revisit that ID while the write generation continues increasing.

A layout commit stores its immutable content, history entry and new current
pointer in one transaction. A working-state commit updates only the latest-value
record. Creating/duplicating a workspace publishes its identity, initial layout,
baseline and working state together. The same transactional guarantees are
required of SQLite and IndexedDB. Callers must be able to distinguish successful
commit, conflict, unsupported schema, unavailable storage and failed writes.

Use a dedicated storage worker for native I/O and an asynchronous browser
adapter. Neither interface blocks the input/render owner. Serialize pending
writes for each workspace and discard late read replies after switching away.
Native windows share the storage service; browser tabs share the origin's store.
App logic must not depend on platform-specific SQL queries or IndexedDB handles.

Validate the implementations with one shared contract suite, adapted to run
against both stores: initial create/load, independent working/layout writes,
atomic history publication, stale-write rejection, deletion/restore, migration,
and storage-error recovery. Platform lifecycle and browser storage behavior also
need integration coverage.

### Commit, switching and ownership rules

Rust prepares an immutable, validated commit batch before opening a transaction.
The backend performs its generation/ownership checks and writes without awaiting
network requests, asset fetches or a Rust-worker round trip inside the transaction.
IndexedDB success is reported on transaction completion, not individual request
success. A request carries workspace ID, owner fence, session epoch, expected
generation and operation ID. Retries reuse operation IDs and return the original
receipt if already committed, so losing an acknowledgement cannot duplicate a
history entry or workspace. Pending receipts/operations survive interruption until
acknowledged; they are delivery bookkeeping, not a history of working values.
An operation ID is bound to one immutable payload. Coalesce unsent live updates
to the newest map; bound receipt retention by acknowledged operations and expired
owner sessions, keeping receipts needed to resolve pending delivery.

Layout history persists current, undo and redo references plus a monotonically
increasing write generation. Undo/redo updates all of these atomically. Editing
after undo clears the redo navigation but leaves abandoned revisions available
in Layout History subject to retention. Duplicate shares immutable retained
layout content and copies navigation into independent workspace ownership; its
later edits cannot change the source's navigation. History payloads, comparisons
and gesture grouping exclude working values and Zen. The existing WorkspaceState
snapshot history must be split accordingly; persisting its current structs as-is
would violate this contract.

Switch/Create-and-Switch/Duplicate-and-Switch require a document-idle session
and no active workspace gesture. Use the existing shared idle checks, including
strokes, transforms, pending region operations and filter preparation; never
implicitly apply or cancel artwork operations. Prepare and validate the incoming
configuration/working state and acquire its ownership before adoption. While
adoption is pending, prevent new conflicting interactions without blocking the
UI thread. A dedicated shared session restore applies the prepared state without
replaying ordinary tool-selection commands, which currently cancel transforms.
Acquire/load atomically or recheck the loaded generation after acquiring ownership
so another owner's final write cannot be missed.

Keep the outgoing workspace and every dirty record/history entry until their
writes are acknowledged. On failed outgoing save or incoming load, keep the old
workspace active and release the unused incoming claim. Only publish the new
active identity after these steps succeed. Capture duplicate/template snapshots
from a committed gesture boundary including pending accepted edits; operation
success must not depend on a stale disk-only snapshot. Deletion first settles
pending writes, then publishes the replacement binding and tombstone atomically.
Save status and Retry belong to the workspace and generation; a late completion
must not clear a newer dirty flag or error. On failure offer Retry and export of
the recoverable in-memory state; routine switching still has no save prompt.

One editable owner per workspace is enforced within a local store, across native
windows/processes and browser tabs, through an atomic claim with a renewable lease
and monotonically increasing fence. Every write checks that fence and the live
entity's generation. Lease expiration allows recovery after a crash; resuming a
suspended owner requires revalidation before editing/writing. Ownership transfer
normally waits for pending writes. A losing owner preserves dirty data and offers
Save as New Workspace, rather than reloading it away. Lease duration/renewal are
implementation constants tested with a fake clock, not wall-clock conflict order.
Deleted items retain a tombstone/fence so delayed saves cannot resurrect them.

A manager must acquire an inactive workspace or route mutations through its live
owner. Reset/delete/rename must not modify another window's stale copy directly.
Switch to Window is best effort on web; if focus is blocked, identify the other
tab and offer Duplicate. This is local ownership only; a future network sync
protocol must handle separate-device conflicts before automatic handoff ships.

### Backend durability, upgrades and migration

Atomicity is shared; physical durability is qualified by backend capabilities.
Native commits use SQLite's durable settings (initially WAL with synchronous=FULL)
on the storage worker; checkpoints are scheduled there too. Consistent backups
use the database backup API, never a copy of the live main file without its WAL.
Web requests strict transaction durability where supported and reports completion
under the browser's guarantees. It cannot promise protection from storage eviction
or user-cleared site data. See [SQLite WAL durability](https://www.sqlite.org/wal.html)
and [IndexedDB transaction durability](https://developer.mozilla.org/en-US/docs/Web/API/IDBDatabase/transaction).

The browser adapter handles versionchange by stopping new writes, retaining dirty
state, closing its connection and offering reload. A blocked upgrade reports the
other open tab and retries after it closes; it never deletes/recreates the store.
After reopening, validate/migrate pending payloads before retrying. Unsupported
newer schemas are preserved, with an update-required error and recovery export.
Shared JSON validation must bound input size, nesting and collection counts before
adoption; corrupt items do not prevent listing/exporting other valid items.
See [IndexedDB upgrade blocking](https://developer.mozilla.org/en-US/docs/Web/API/IDBOpenDBRequest/blocked_event).

Migration is idempotent per legacy source: Android's capy-canvas/workspace value,
web localStorage layer.workspace.v1, and Apple's workspaces/<scene UUID>.json and
workspace.json fallback. Commit each source-to-workspace ID mapping and completed
import together, using a uniqueness constraint to handle concurrent first starts.
Preserve each distinct Apple scene, even if layouts match. Map the fallback to
an identical imported scene when possible, otherwise create one separate seed
workspace. Retain local scene-to-workspace bindings and last-used identity outside
templates. Use the actual imported layout as its reset baseline unless provenance
exists. Import legacy Zen as latest working state; never invent historical brush
values. Preserve legacy inputs; a failed or unknown-schema read is not an empty
store and must not trigger overwriting it with fresh defaults. Later launches use
the new store once migration is acknowledged, without dual-writing legacy paths.

Entity/revision/resource IDs are opaque stable strings, independent of names or
catalog positions; encode wide generation counters losslessly too. JSON/JS must
not round large IDs or generations through floating-point numbers.
Local layout IDs remain scoped to their layout and are remapped when importing
toolbar instances. Names are trimmed and compared case-insensitively within each
library. Create/rename show an inline collision error; duplicate/import/restore
allocate a unique suffix atomically. All references use IDs, so renaming is safe.

### Resource lifetime and bounded recovery

Pin exact custom-resource definitions used by saved captures and retain their
transitive dependencies. Reachability roots include current layouts and working
values, original baselines, retained history/undo/redo, reusable entries/versions,
Recently Deleted items, and in-flight imports/exports. This does not version
built-in brush defaults. Imported payloads/resources are validated before adoption;
missing resources remain represented and reported rather than silently dropped.

Prefer transactional blob records for workspace/template resources initially.
If large files use an external asset directory, stage and verify immutable bytes
before publishing database references; successful publication must only reference
available data. Reclaim abandoned staging files later. Garbage collection first
updates references transactionally and deletes only unreferenced assets; a failure
may leave orphan bytes but must not break a committed layout or recoverable item.
Built-in IDs unresolved by an older app produce an unavailable item or a clear
compatibility error, preserving the original package for a supported app.

Initial retention policy: a 100 MiB target per installation/origin for eligible
layout and metadata/library revision history, pruning oldest unpinned entries.
Keep up to 100 undo and 100 redo references per workspace; their referenced content
is protected. Current working values, live reusable definitions, original reset
baselines and their dependencies are also protected. The budget is a target,
not a guarantee that total protected data fits in 100 MiB. Retain Recently Deleted
items for 30 days, show their expiry date, and expose Delete Permanently with
confirmation. Removal never collects content still referenced by another item.
Retention is not part of undoable editing; show the oldest available history
date and apply the same policy across both backends.

Show storage usage and an explicit Clear Older History action with a preview of
what will be removed, preserving current state, baselines and active navigation.
Under disk/quota pressure, clean only eligible records and retry; if insufficient,
keep dirty state, show the failed save and offer space management plus export.
Do not report Saved or prune protected data merely to make the write succeed.
Provide full workspace recovery export from memory/storage as an overflow/error
action, containing current layout, latest values, baseline, retained history and
required resources. This is a separate package from a layout-only template export.

Pair this with Import Workspace Backup in storage details and the storage-error
recovery flow. Validate schema, bounds, references and resources before publishing
a new independent workspace. Restore latest working values, the original layout
baseline, retained layout history/navigation and recoverable metadata. Assign a
fresh workspace identity and remap package-local revision/resource references;
reuse verified identical assets where possible. Resolve name collisions using a
unique suffix, never by overwriting a live workspace, template or resource.
Ownership, device/scene bindings, write generations and delivery receipts are
local runtime bookkeeping and are excluded from the package. Initialize fresh
local bookkeeping on import. Publish the restored aggregate atomically after
resource staging succeeds. On quota/validation failure retain the package and
existing workspace, with no partial import visible. Normal template import still
creates only a reusable layout; backup import is a separate action and format.

On web check/request persistent storage where supported, handle denial, and expose
its status in storage details. If the store is unavailable, show session-only mode
and export/retry actions; do not silently fall back to localStorage as a second
authoritative store. Private browsing and cleared/evicted site storage can remove
local recovery data. Template export alone does not back up working values/history.
See [browser storage persistence and quotas](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria).

Layout edits append a revision and advance the current pointer in one transaction.
Working values update a separate latest-only JSON record. Exports remain portable
JSON files, packaged with required resources when needed; larger reusable assets
can live in a separate managed asset directory. Cross-device sync exchanges
records/resources rather than copying an open SQLite file. Migrate the legacy
native and web sources under the idempotent migration rules above. Persistence
of unrelated application preferences and artwork remains outside this migration.

SQLite supports local embedded application storage, atomic transactions and
incremental updates; see its [application-format guide](https://www.sqlite.org/appfileformat.html).
Android documents [private internal storage](https://developer.android.com/training/data-storage/app-specific),
and Apple's current host already uses Application Support in
`apps/layer-apple/Shared/Bridge/EditorPersistence.swift`.

Changing the configured parameter of a toolbar shortcut (for example changing a
20px button to 30px) is toolbar customization and remains versioned. Activating
that shortcut or dragging a live brush-size slider changes latest working state
only. Deleting a workspace retains its latest working record alongside its
layout history for Recently Deleted recovery. Library template/toolbar edits
still retain previous versions independently of live working state.

## Research and its implications

| Product | Documented convention | Implication for this app |
| --- | --- | --- |
| Photoshop | Workspaces remember the last panel arrangement; Reset restores the original setup. | Closest precedent for separating current state from a saved starting point. |
| Clip Studio Paint | Window → Workspace; named choices with an active checkmark; Register, Manage, and Reload. Management supports rename/delete. | Keep selection under Window and show the active workspace. Prefer familiar Save/Reset wording over Register/Reload. |
| Krita | Workspaces save docker configuration; Window → Workspaces and a toolbar selector; import and deletion are available. Sessions separately retain open images/windows. | Workspace is appropriate drawing-app terminology. Avoid Session because it suggests open documents. |
| VS Code | Profiles can be created from profile templates. | Supports presenting templates within creation, rather than mixing starting points with active workspaces. Retain Workspace as the drawing-app noun. |

Sources: [Photoshop restore workspaces](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/restore-workspaces.html),
[Clip Studio workspace management](https://help.clip-studio.com/en-us/manual_en/690_interface/Register_and_manage_your_workspace.htm),
[Krita workspaces](https://docs.krita.org/en/reference_manual/resource_management/resource_workspace.html),
[VS Code profiles](https://code.visualstudio.com/docs/configure/profiles).

The two-object distinction, persistent history, and immutable reset target below
are recommendations for this app, not claims that all these products implement
the same model.

## Window menu and window context menu

Use the same Workspace submenu in the Window application menu and the generic
window-background context menu. Panel/tab context menus should retain their local
actions rather than repeat the entire manager.

```text
Window
  Workspace                         >
  ------------------------------------
  Undo Workspace Change
  Redo Workspace Change
  ------------------------------------
  [existing panel visibility entries]
  [existing toolbar visibility entries]
  New Toolbar…
  Manage Toolbars…

Workspace >
  ✓ Painting
    Inking
    Small Screen
  ------------------------------------
  New Workspace…
  Save as Template…
  Reset Layout…
  ------------------------------------
  Layout History…
  Manage Workspaces…
```

The new submenu has six command entries plus workspace choices. Show the current
workspace and up to five other recently used workspaces. More are available in
Manage Workspaces; templates never appear as switch targets. Selection resumes
the workspace's latest state without a save prompt. The active item is checked,
and choosing it again is a no-op.

Reset Layout opens a small confirmation identifying the actual target, e.g.
“Reset Painting to its original Illustration layout? This restores panels,
toolbars, and their customizations. Brush settings and colors stay as they are.
Your current setup will remain available in Layout History.”
Buttons: Cancel / Reset Layout.
For a workspace created without a template, say “starting configuration.” Disable
reset if it would do nothing. No separate confirmation is needed for ordinary
switching, creation, or duplication.

Replace the existing Reset layout action with **Reset Layout…**. Reset restores
the complete layout baseline, including toolbar membership, names, tile contents/order,
display settings, placement and visibility. It restores deleted baseline toolbars
and removes later additions from the active workspace. The previous configuration
remains recoverable. There is no separate position-only reset in this proposal.
Reset preserves working brush settings/colors and the independent Toolbar Library.

Keep Rename, Duplicate, Delete, Import, Export, and template updates in the
manager. Do not add Save Workspace or Save Changes: those labels imply a manual
save obligation. File → Save continues to mean saving the drawing.

## Management modal

One modal, **Manage Workspaces**, with **Workspaces** and **Templates** tabs and
a secondary **Recently Deleted** destination. History is a detail view within
this modal, not a third top-level library or separate dashboard.

Use a searchable list with a selected-item detail pane. Clicking a row selects
it without applying it. A primary button performs the action; keyboard activation
must work without double-clicking. Close/Escape closes the manager; there is no
global Apply/Cancel transaction. A neutral layout thumbnail may help recognition,
but it should show panels and canvas geometry without embedding artwork.

| Surface | Information | Primary action | Secondary actions |
| --- | --- | --- | --- |
| Workspaces list | Name, Current or open-in-another-window state, last used | Switch Workspace; Current Workspace disabled for active row | New Workspace above list |
| Workspace details | Layout preview, “Changes saved automatically,” “Started from Illustration,” history link | Switch Workspace | Rename, Duplicate, Save as Template, Reset Layout, Move to Recently Deleted |
| Templates list | Name, My Templates/Built-in grouping | New Workspace from Template… | Import Template above list |
| Template details | Preview, description, modification date, Built-in badge where applicable | New Workspace from Template… | Rename, Duplicate, Edit as Workspace, Update from Current Workspace, Export Template, Previous Versions, Move to Recently Deleted |
| Layout History | Dated, human-readable actions and recovery points; selected preview | Restore This Version | Open as New Workspace |
| Previous Versions | Retained template content and metadata revisions, dates and names | Restore This Version | Create Workspace from This Version |
| Recently Deleted | Type, name, deleted date and 30-day expiry | Restore | Delete Permanently… with confirmation |

System templates allow creation, duplication, and export. Hide modification and
deletion actions, show the Built-in badge, and offer “Duplicate to customize.”
Built-in templates are never themselves active workspaces: the initial mutable
workspace can be named “My Workspace,” created from “Default.”

For non-current workspace details, actions operate on the selected workspace,
not implicitly on the active window. The template action **Update from Current
Workspace…** explicitly names its source and target before confirmation.

## Toolbar reuse

Use the same ownership rule at both scales: a reusable saved configuration creates
an independent working copy. A workspace template captures the workspace layout;
a saved toolbar captures one reusable component. Users do not need to understand
shared identities or storage deduplication.

| Item | Owns | Effect of editing |
| --- | --- | --- |
| Toolbar in a workspace | Current toolbar definition and its placement/visibility | Changes this workspace, recorded in workspace history |
| Saved toolbar in the Toolbar Library | Reusable definition and retained previous versions | Changes what is added next; existing copies stay unchanged |
| Toolbar inside a workspace template | Definition and placement captured with that template version | Used when creating or resetting the workspace from that version |

The reusable definition includes its name, tiles, order, dividers and toolbar
display options. Placement and visibility belong to the receiving workspace. Add
uses the selected group's normal insertion behavior, or a discoverable floating
position when there is no selected destination. It does not hide the new toolbar
or reproduce another workspace's screen coordinates.

Extend **Manage Toolbars…** with **This Workspace** and **Library** tabs:

- This Workspace lists visible and hidden toolbars. Actions: Show/Hide, Rename,
  Duplicate, **Save to Toolbar Library…**, **Replace from Library…**, and
  **Delete Toolbar…**. The delete description names the current workspace;
  hide remains a separate action that keeps its configuration.
- Library lists saved toolbars. Its primary action is **Add to Workspace**.
  Secondary actions: Rename, Duplicate, **Update from Workspace…**, Import,
  Export, Previous Versions, and Move to Recently Deleted. Updating asks which
  current-workspace toolbar to capture and names the library target explicitly.
- Toolbar context menus expose **Save to Toolbar Library…**. The New Toolbar
  flow offers an empty toolbar or one from the library; avoid another top-level
  Window menu command for every library operation.

Saving to the library creates a saved entry without altering the current toolbar.
A naming collision does not silently overwrite an entry; updating is explicit.
Adding a saved toolbar creates a new toolbar in the current workspace. Replacing
an existing one is a separate explicit action that preserves its placement and
is undoable, so adding never unexpectedly overwrites customization.

For example: save Quick Tools from Painting, add it to Inking, then add an eraser
in Inking. Painting and the saved Quick Tools remain unchanged. Updating the
library from Inking publishes a new version for future additions. Resetting
Painting still restores the exact toolbars captured by Painting's original
workspace baseline, regardless of subsequent library edits or deletion.

Workspace templates must retain their captured toolbar definitions. They must
not resolve a toolbar's latest library version when created, reset or exported.
Immutable shared records are compatible with this rule if references pin exact
versions and remain retained; exported workspace templates must carry enough
content to work without the originating library. Recovery of a library entry
belongs to its Previous Versions/Recently Deleted views, while recovery of a
local toolbar belongs to its workspace history.

## Key flows

| Intent | Flow and result |
| --- | --- |
| Start the app | Resume the last workspace and its history. First launch creates My Workspace from Default without requiring setup. Existing single-workspace data migrates into My Workspace. |
| Rearrange the UI | Edit normally; save committed changes automatically. Workspace undo remains separate from drawing undo. |
| Switch tasks | Window → Workspace → Inking. Preserve Painting's latest state and history; restore Inking's. Keep the document open and unchanged. |
| Create a workspace | New Workspace asks for name and “Start with.” Default starts from the current workspace; alternatives are templates. Create and Switch makes a new workspace with its own history. Starting from current copies working values and captures a new layout baseline; starting from a template pins its layout version and begins with default tool state. |
| Experiment with a copy | Manager → Duplicate. Copy configuration, original reset target, and recoverable history into an independent workspace; ask for name and offer Duplicate and Switch. Subsequent changes diverge. |
| Save a reusable setup | Save as Template asks for a name and optional description. Capture layout and toolbar configuration, excluding working tool settings, colors and workspace history. Create the template without changing the current workspace or its original reset target. Offer Export in template details afterward. |
| Reset an experiment | Reset Layout previews/names the original layout baseline, including its toolbars and customizations, and confirms. Append a recoverable reset action; Undo Workspace Change restores the immediately preceding configuration. Working tool settings, colors and the Toolbar Library are unchanged. |
| Reuse a toolbar | Save to Toolbar Library, switch workspace, then Add to Workspace. Customize independently. Publish an explicit library update or Replace from Library when wanted. |
| Change a custom template | Select template → Edit as Workspace. Create an ordinary editable workspace from it, then customize through the real editor. In template details, Update from Current Workspace publishes a new version after naming source/target. The editing workspace remains recoverable. |
| Update a template from an existing setup | Templates → select custom template → Update from Current Workspace. Confirm “Update Illustration using Painting?”; capture layout/toolbar configuration only and retain the previous version. Existing workspaces do not change. |
| Customize the built-in template | Duplicate it into My Templates, then follow the custom-template flow. |
| Share a setup | Save as Template if necessary, then Export Template to a file. Export layout/toolbar configuration and essential metadata/resources, excluding working brush settings, colors, workspace history and artwork. Import adds a template and offers New Workspace from Template. |
| Remove or recover an item | Move to Recently Deleted; offer immediate Undo. Restore later from the manager, including its retained history/versions. Renaming and template replacement retain previous metadata too. |
| Recover an older arrangement | Layout History → select and preview → Restore This Version. Record a new recovery point; preserve newer states. Open as New Workspace allows comparison without replacing the current state. |

## Rules that keep the model predictable

1. **Original layout means the template version used at creation.** If Illustration changes later,
   Painting still resets to its original Illustration version. Details can show
   the original version date and “A newer template version is available.” Offer
   New Workspace from Latest Template there; do not introduce automatic updates,
   merging, or rebasing in the first release. A workspace without a template has
   a retained creation layout snapshot instead. This does not imply versioning
   the built-in brush defaults, which are outside template scope.
2. **Saving a template does not move the workspace's reset target.** State this
   in the save dialog. Users who want a workspace based on that new template can
   create one from it. Avoid an implicit relationship change or extra checkbox.
3. **Recovery is broader than undo.** Undo/redo handles sequential layout and
   customization edits. History also retains layout states displaced by reset, restoration,
   and editing after undo. Template version restoration creates a new latest
   version; restoration does not erase intervening versions (normal retention
   still applies). Metadata revisions and deletion
   must be recoverable even when no workspace is currently open.
4. **Deletion cannot break a workspace.** Retain its original template snapshot
   even if the template is renamed or deleted. Explain “Original template was
   deleted; its starting configuration is still available” in details. Deleting
   an active workspace requires selecting a replacement; if none exists, offer
   creation from Default. Show affected windows in this confirmation.
5. **Per-window state needs an ownership rule.** Require
   one editable local owner per named workspace under the claim/fence protocol.
   Choosing one open elsewhere offers
   Switch to Window or Duplicate Workspace, preventing silent shared edits.
   New windows get independent workspaces, and closing a window preserves them.
6. **Template payloads should not open into hidden UI.** Retain Zen state when
   resuming workspaces, but recommend creating template-based workspaces with
   normal chrome visible. Treat this as an explicit template-capture rule.
7. **Do not make a recovery promise the storage cannot keep.** Persist committed
   state, history, baseline snapshots, and metadata versions atomically; retain
   previous valid records. If a save fails, show a persistent error and preserve
   the working session. Validate imports before adding them; same-name imports
   receive a unique name rather than overwriting a template. Missing referenced
   resources should be reported before creation, with supported fallback choices.

## Review scenarios before implementation approval

Ask someone unfamiliar with the model to switch away and back, reuse a setup,
recover a moved panel, reset after editing a source template, and customize the
built-in default. They should predict which object each action changes and what
Reset will recover without learning about revisions or copy-on-write.

Implementation checks should specifically cover switching with independent
histories, restart recovery, reset undo, template update/delete with old reset
targets, metadata recovery, rejected/corrupt imports, and separate windows.
Also verify that toolbar library updates/deletion leave existing workspaces and
template resets intact, reset restores toolbar customizations and membership,
reset undo recovers local toolbars, and exported workspace templates retain their
toolbar contents without access to the source library.
Check latest sparse brush restoration against current defaults, debounced writes
and write ordering, zero history entries for live adjustments, template exclusion
of working values, and reset/undo/history preserving working values. Also check
that configured toolbar-action parameter edits still enter layout history.
Cross-device transport and resource availability need their own verification
when synchronization is implemented.
Keep shared command labels, availability, dialog descriptions, and action
semantics in Rust, following the existing host presentation boundary.

## Implementation sequence and acceptance

1. Split persisted layout/metadata history from latest-only working state in the
   shared model. Add typed restore/capture, stable IDs, payload migrations and
   durable undo/redo navigation. Keep existing artwork/document semantics intact.
2. Implement the asynchronous store contract, native SQLite worker and IndexedDB
   adapter. Run the same behavioral fixtures against both real backends, including
   transaction aborts after requests succeed, lost acknowledgements, generation
   races, duplicate operation delivery and coherent reads during concurrent writes.
3. Add ownership, legacy migration and lifecycle integration before enabling
   autosave. Test concurrent first launches, each Apple scene/fallback, the Android
   preference and web localStorage imports, interruption/retry, newer-schema data,
   blocked browser upgrades, stale tabs, suspend/takeover and failed reopen.
4. Wire the menus, manager, template/toolbar reuse, reset and recovery. Verify
   failed outgoing save, failed incoming load, rapid A→B→A switches and queued
   writes after deletion. A pending transform, active stroke or asynchronous
   region operation must prevent switching without changing the document or its
   undo stack. Verify Undo→restart→Redo and Undo→new edit with the old redo branch
   retained in Layout History; all preserve live settings and Zen.
5. Add resource packages, recovery export and retention. Test source-resource
   deletion, reset/export of old captures, interrupted asset publication, expiry
   with shared references, history pruning, native disk-full, web quota denial,
   storage-unavailable mode and export from unsaved memory. Round-trip a recovery
   package through Import Workspace Backup, preserving latest values, baseline
   and history/navigation under fresh local IDs, including conflicts, corrupt
   resources, quota failure and interrupted publication. Exercise supported
   browsers and native mobile lifecycle paths; synthetic storage tests alone do
   not establish physical suspension/power-loss guarantees.

The first persistence/manager release has one portable logical layout per
workspace. Viewport fitting/clamping and measured geometry are transient. Do not
silently save a fitted small-screen layout as a new user edit. Independently
remembered tablet/desktop arrangements require an explicit later variant model;
users can use separate named workspaces meanwhile. Network sync, cross-device
ownership/conflict policy, and automatic artwork handoff are separate follow-ups.
The local store and export formats must not claim to provide automatic resume
across devices until that work is delivered. Raster document changes remain deferred.

Fresh-context review and disposition are recorded in
[workspace-manager-review.md](workspace-manager-review.md).
