# Workspace manager: approved host implementation handoff

The user approved the GTK design on 2026-09-12. Other-platform implementation can
start. This document and the [GTK visual reference](workspace-manager-gtk-redesign.md)
define the approved scope. The [original proposal](workspace-manager-proposal.md)
and its review are historical background; their larger UI is superseded.

## Product contract

| Term | Meaning |
| --- | --- |
| Workspace | A named layout and its latest tool settings, saved automatically for a task |
| Saved layout | A named, reusable arrangement of tools and panels, including toolbar configuration |
| Layout History | Earlier arrangements of the current workspace; excludes historical tool settings and document edits |
| Starting layout | The workspace's original arrangement, retained even after loading another saved layout |

Use the exact shared captions and introductory copy in
[`workspace_manager_ui.rs`](../../crates/layer-ui/src/workspace_manager_ui.rs) and
the GTK reference. Internal names such as `Template`, `ManageTemplates`,
`SaveAsTemplate`, and `UseTemplate` are compatibility vocabulary, not UI wording.
Do not rename serialized record kinds, command IDs, or package formats just to
match the visible Layout terminology.

## Approved UI and behavior

The Window menu starts with Undo Layout Change / Redo Layout Change, then
Workspaces, then Quick Access Toolbars, followed by the direct panel rows.
Quick Access Toolbars is a sibling directly below Workspaces. Panel and toolbar
visibility names omit the redundant "panel" and "toolbar" suffixes.

Workspaces contains recent workspace choices followed by three sections:

1. New Workspace… / Manage Workspaces…
2. Save Layout… / Load Layout…
3. Layout History… / Restore Starting Layout…

Recent workspace menu choices switch directly. The manager dialogs use preview
selection and explicit confirmation instead:

- **Workspaces:** compact selectable list, top-right square + for New Workspace,
  per-row Rename/Delete options, Cancel and Switch to Workspace below the list.
  Initially select the current workspace and disable its Switch button. An item
  already owned by another window offers Switch to Window through the existing
  ownership policy.
- **Saved Layouts:** opened by Load Layout…, with a top-right + for Save Layout,
  per-row Rename/Delete for user entries, Cancel and Load Layout below the list.
  Initially no row is selected and Load Layout is disabled. The included Default
  layout can be loaded but cannot be renamed/deleted. Save Layout suggests
  `<workspace name> Layout`; the name is editable and ordinary collision handling
  remains in place.
- **Layout History — <workspace name>:** one scrollable list of versions, Cancel,
  and Restore This Version. Select the current version initially and disable its
  Restore button. Entries identify affected panels/toolbars and the action/date.
  Do not add "select a version to preview" instructions.

In all three dialogs, row selection, double-click, and Enter on a row only preview
the arrangement in the actual editor behind the dialog. Confirming with the
button finalizes it. Cancel, Escape, back navigation, and native modal dismissal
restore the original arrangement. Use opaque dialogs and a light enough backdrop
to see the preview. Adapt sizing to the host and viewport without changing these
semantics; retain keyboard focus, accessibility labels, and scrollable lists.

New Workspace supports a name and starting layout. Starting from Current layout
copies the current working settings into a new independent history. Starting from
a saved layout uses default working settings. Loading a saved layout into an
existing workspace preserves its identity, starting layout, and working settings.
It creates one undoable layout change, or no event if the layout is identical.
Restore Starting Layout restores the original baseline with one undoable change.

Keep the existing simple toolbar management flow. Do not add Recently Deleted,
backup/import/export, metadata or saved-layout version screens, duplication/update
controls, network sync, or device-specific layout variants. Some underlying APIs
remain for compatibility and retention; their existence does not expand the UI.
Failed saves still need the existing Retry / Save as New Workspace recovery, and
failed close must preserve the option to keep the window open.

## Shared implementation to reuse

| Responsibility | Reference |
| --- | --- |
| Working state, captures, prepared adoption, temporary previews | [`workspace_session.rs`](../../crates/layer-ui/src/workspace_session.rs) |
| Menu and command dispatch | [`workspace_manager_ui.rs`](../../crates/layer-ui/src/workspace_manager_ui.rs), `UiSession::workspace_menu` |
| Entity validation, storage policy, ownership, mutations | [`layer-workspace`](../../crates/layer-workspace/src/lib.rs), `WorkspaceManager<S>` |
| Lists, action availability, prompts | [`presentation.rs`](../../crates/layer-workspace/src/presentation.rs) |
| Asynchronous store protocol | [`protocol.rs`](../../crates/layer-workspace/src/protocol.rs), `WorkspaceStore::execute` |
| Native SQLite and worker | [`sqlite.rs`](../../crates/layer-workspace/src/sqlite.rs), [`worker.rs`](../../crates/layer-workspace/src/worker.rs) |
| GTK lifecycle and recovery reference | [`workspace_manager.rs`](../../apps/layer-linux/src/workspace_manager.rs), `workspace_manager_actions.rs`, `workspace_manager_storage.rs` |
| GTK selection/confirmation and preview lifetime | [`workspace_manager_dialog.rs`](../../apps/layer-linux/src/workspace_manager_dialog.rs), [`workspace_history_dialog.rs`](../../apps/layer-linux/src/workspace_history_dialog.rs) |

Hosts own toolkit controls, async transport, lifecycle notifications, and native
window focus. Keep workspace decisions in shared Rust. In particular:

- Capture accepted edits through `capture_workspace`/`workspace_working_state`;
  use `WorkspaceManager` for autosave, naming, loading, claims, and publication.
  Do not serialize widget state or implement a second workspace model in a host.
- Switch/adopt only at a document/workspace idle boundary. Prepare and validate the
  incoming capture, retain the outgoing state on failure, and use
  `PreparedWorkspace`/`adopt_workspace`. The legacy `RestoreWorkspace` action
  clears history and is not the named-workspace adoption path.
- During previews, hold the workspace transition, block editor mutations/autosave
  of temporary arrangements, and renew ownership with `OWNER_RENEW_MS`. Durable
  captures must still return the pre-preview state. Cancel the preview before
  beginning Save/New/Rename/Delete or committing a selection. Resume it only if
  the manager remains open after a secondary action.
- Invalidate asynchronous loads when selection, search results, dialog lifetime,
  or workspace changes. Clearing the selection restores the original layout and
  disables confirmation. Late replies must not resurrect a cancelled preview.
- Use fenced writes and immutable operation receipts. Retry the original operation
  without duplicating history/items; never overwrite another window's workspace
  after suspension or ownership loss. Preserve pending edits on save failures.
- Preserve the existing retained-control rendering and resize behavior. Workspace
  integration should not introduce database I/O or widget rebuilding on pointer
  motion paths, or disturb document strokes, transforms, undo, or rendering.

## Platform work and coordination

| Host | Work still required |
| --- | --- |
| Web (`apps/layer-web`) | IndexedDB implementation of the shared store protocol; Wasm async transport, DOM dialogs, tab ownership, visibility/page lifecycle, legacy localStorage migration |
| Android (`apps/layer-android`) | Shared native worker/manager bridge, app-private directory, Compose dialogs, activity/suspend/recreate lifecycle, legacy state migration |
| Apple (`apps/layer-apple`) | Shared native worker/manager bridge, persistent app directory, native macOS/iOS dialogs, independent scene ownership, scene persistence migration and lifecycle |
| Windows (`apps/layer-windows`) | Shared native worker/manager bridge, app-data directory, WinUI dialogs, independent windows, lifecycle and existing-state migration |

Native hosts use `layer-workspace` with its `native` feature and one
`StoreWorker::shared` service per installation directory. Keep disk work off the
UI/render thread; await or poll replies instead of using blocking `StoreReply::wait`
there. Web needs the actual IndexedDB adapter; native SQLite being complete does
not mean web storage already exists. IndexedDB must validate generations/ownership
and atomically publish the complete prepared batch, acknowledging transaction
completion rather than individual request success.

Implement idempotent migration of each host's existing state, with stable source
mapping/import markers committed atomically. Preserve multiple Apple scenes and
unreadable/newer legacy data; do not replace existing user state with defaults.
See the proposal's storage/migration contract and current Rust protocol for detail.

Before parallel host changes touch `crates/layer-host`, shared command transport,
or common store types, agree on one owner for each additive bridge change. Keep
platform UI work in its host tree and integrate shared additions from main rather
than creating incompatible copies of the manager/protocol. No broad shared-code
refactor is a prerequisite to starting the host work.

## Acceptance evidence for each host

1. Real native/browser input opens menus, both + dialogs, row options, and history.
   Capture and inspect screenshots against the approved GTK reference.
2. Selection previews without switching, saving, or adding history. Verify rapid
   selections, filtering away a selection, Cancel/Escape/back, and dismissal while
   a load is pending. Verify default saved-layout naming and explicit button apply.
3. Load Layout preserves current workspace identity/tool settings/baseline and is
   undoable once. Switch to Workspace restores that workspace's own latest tool
   settings. Layout History and Restore Starting Layout preserve tool settings.
4. Restart preserves active workspaces, settings, layout history, and undo/redo.
   Live tool changes do not create layout history; gestures create one event.
5. Existing legacy state migrates once, including interrupted/concurrent startup.
   Multiple windows/tabs/scenes remain independent; takeover blocks stale writes.
6. Save/load failures, unavailable/full storage, lost acknowledgements, and
   lifecycle interruption preserve user state and expose usable recovery. Web
   additionally covers IndexedDB aborts/upgrades/quota and storage availability.
7. Modal close/confirmation and normal editor input still work; no regression to
   retained controls, resize/drag behavior, artwork, or input/render performance.

Run the shared UI/store suite and the host's relevant build and integration checks.
For Web, run the store contract cases against IndexedDB itself. GTK reference
checks are `native_named_workspace_manager_templates_library_and_history` and
`apps/layer-linux/bench/workspace-menus.sh`; their bodies show assertions and
safe isolation requirements. Record actual host test/build/screenshot evidence;
GTK approval alone does not certify another platform implementation.
