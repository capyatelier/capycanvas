# Workspace manager plan review

Historical review of the original proposal. For the user-approved UI and current
host rollout scope, use the [implementation handoff](workspace-manager-host-handoff.md).
The removed maintenance/export screens discussed below are not rollout requirements.

Reviewed 2026-09-11. A separate reviewer received the plan and repository path
with no conversation history. It examined the design, relevant implementation
and primary browser documentation. This record describes design corrections;
the acceptance scenarios are planned checks, not executed implementation tests.

Scope retained: layout-only templates, latest-only live working values, sparse
brush overrides against unversioned built-in defaults, and one storage interface
with shared native SQLite plus a web IndexedDB adapter. Raster artwork work is
deferred. See [the revised plan](workspace-manager-proposal.md).

## Findings and disposition

All eight findings were accepted. No finding required changing the settled scope.
The reviewer's second pass confirmed all eight were resolved and identified one
follow-up gap in the new recovery-export flow; its import counterpart is now
specified as well.

| Priority | Finding | Correction in the plan | Required verification |
| --- | --- | --- | --- |
| P1 | Flushing on switch did not establish that outgoing dirty data survives failed saves or incoming loads. Captures could use stale disk state. | Keep the old workspace and dirty data until acknowledged; validate/claim the incoming state before adoption; preserve state on error; capture accepted edits at a completed-gesture boundary. Track status by workspace/generation. | Failed outgoing save, failed incoming load, capture while a write is pending, rapid A→B→A, stale completion after a newer edit. |
| P1 | Restoring ordinary tool-selection commands can cancel an unfinished transform or region operation. | Gate switching on document/workspace idle; restore through a dedicated prepared-state operation; never implicitly apply/cancel artwork. | Pending transform, live stroke and asynchronous region work prevent switching without changing artwork or document undo. |
| P1 | Immutable revisions/current pointers did not specify persistent undo/redo; current snapshots also include Zen. | Persist independent undo/redo references and a monotonic write generation; retain abandoned redo content under normal retention; exclude all latest-only values from history; define independent duplication. | A→B→C→Undo→restart→Redo; Undo→new edit leaves C recoverable; layout undo/reset/history leave Zen and live values untouched. |
| P1 | One editable window had no enforceable ownership rule or dirty-conflict recovery. | Atomic leased ownership and fencing within each local store; writes validate owner/generation; route manager mutations through the owner; preserve a losing owner's state with Save as New Workspace. | Concurrent opens, crash/lease expiry, suspended owner returning, cross-window manager reset/delete, browser focus failure. |
| P1 | Migration omitted web localStorage and could duplicate/collapse Apple's scene records after retry. | Include layer.workspace.v1; source-to-destination mappings and import markers commit together; preserve scenes, define fallback deduplication and retained original baselines; preserve unreadable legacy data. | Interrupted migration, concurrent first launch, multiple equal-layout Apple scenes, fallback-only storage, newer schema. |
| P2 | An asynchronous generic API could accidentally split IndexedDB compare/write operations or acknowledge requests before transaction completion; upgrades can block. | Prebuild commit batches; perform checks and writes within one transaction; acknowledge completion; handle versionchange/blocked without deleting the store. | Abort after successful individual requests; delayed payload preparation; an older browser tab blocking upgrade. |
| P2 | Record transactions did not protect transitive resources or external asset publication. | Pin custom-resource dependencies from every recovery root; initially prefer transactional blobs; otherwise stage immutable assets before references and collect only unreachable data. | Delete the original resource then reset/export/restore; interrupt import publication or collection. |
| P2 | Unbounded recovery and browser durability promises lacked quota/eviction behavior. | Define a 100 MiB eligible-history target, bounded navigation, protected roots and 30-day trash; add storage management, permanent-delete confirmation, recoverable save failures and full recovery export; qualify browser persistence. | Quota/disk-full, protected data above target, expiry with shared dependencies, private/unavailable browser storage, export of unsaved state. |
| Follow-up | Full recovery export needed a matching recovery import; template import cannot restore working state/history. | Import Workspace Backup validates and restores a new independent workspace with working values, baseline, retained history/navigation and resources, fresh local IDs and no overwrite. Runtime ownership/receipts are excluded. | Full backup round-trip, naming/reference conflicts, corrupt resources, quota failure and interrupted publication. |

The tool-restoration issue was checked directly against `UiSession::select_brush`
in `crates/layer-ui/src/session.rs`, `cancel_layer_gesture` in `art_layers.rs`, and
`require_document_idle` in `document_files.rs`. The existing history's whole-state
snapshots and redo clearing were checked in `crates/layer-ui/src/workspace.rs`.
Legacy source locations were checked in Android `CanvasHost.kt`, Apple
`EditorPersistence.swift`, and web `app.js`.

## Additional clarifications applied

- Historical-layout copies use that selected layout as their reset baseline and
  copy only the source's latest working values; they start new layout history.
- Layout History restores layout only. Names/metadata have separate revision
  recovery and do not unexpectedly change when choosing an old arrangement.
- Entity identity is independent of names, with lossless IDs/generations across
  Rust/JSON/JavaScript. Naming collisions follow consistent create/rename versus
  duplicate/import/restore rules.
- Explicitly setting a live value equal to the current default removes its
  override. A load or unrelated save does not erase a previously saved override.
- Commit receipts, ownership fences and write generations distinguish retry,
  content history and active ownership. Revisited layout IDs alone cannot reject
  stale writes after undo.
- Native and web share atomic application semantics, while disk durability and
  browser storage eviction remain separately stated capabilities.
- The first release uses one logical layout per workspace with transient viewport
  fitting. Independent per-device layouts and network synchronization require
  separate contracts; local persistence alone is not advertised as device handoff.

The 100 MiB/100-step/30-day values are initial product defaults, not benchmark
results or guarantees that all protected data fits in that amount of storage.
Automatic collection must preserve the defined roots regardless of these values.
