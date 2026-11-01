# Native workspace ownership

[Editor internals](README.md)

Native clients share `layer-workspace`'s SQLite worker. Each claimed item has an
exclusive kernel file lock, retained until its ownership is released. SQL's
owner UUID, epoch, fence and generation checks still authorize writes. The files
are under `<database>-locks/`, keyed by a hash of the item ID; existence is not
ownership. Never unlink/recreate a lock file while clients may be running.

`WorkspaceController` presents workspace and layout-history rows through
`WorkspaceView`. Restoring history uses `WorkspaceManager::change_layout`, which
checks storage ownership before changing the layout.

Lock acquisition and liveness probes run under an IMMEDIATE SQLite transaction.
Claim/load/list, writes, renewal and maintenance reconcile stale claims before
making ownership decisions. An unlocked stale claim is cleared immediately,
even if other clients remain open. A live lock protects its owner through sleep
or missed heartbeats; the lease-shaped shared API is refreshed as needed, not
used to steal native ownership.

Browser leases remain timer-based while their owner is alive. Each document also
holds a Web Lock named for its owner ID until it closes. Before each transaction
the storage worker lists held and requested owner locks; a claim made before that
query whose owner has no lock is cleared. Closing, reloading into a new identity
or navigating away therefore releases ownership immediately, although `pagehide`
cannot finish an asynchronous release. Without Web Locks, claims wait for expiry.

New locks are held locally through SQL publication, then installed in the
worker. Failed claims/creations drop unpublished guards. Normal close flushes
and releases; final manager teardown queues retirement after accepted writes,
including unadopted creations, without affecting other owners. Retirement drops
kernel locks even when SQL cleanup fails. A process kill closes the handles in
the kernel; a subsequent transaction reclaims the row. Pending deliveries and
receipts are preserved, and a new fence rejects stale saves/releases.

Failed metadata writes retain their original claims until retry or teardown.
Renewal includes those claims, and retry replays the same operation and fence.
A metadata-only retry updates the saved metadata without adopting a workspace;
temporary claims are released after publication. Renaming or deleting an
unrelated workspace therefore keeps the manager's selected preview in place.
Inline workspace names require an accepted submission or explicit cancellation
before navigation. A rejected submission keeps its prompt and selected preview;
pin edits remain available while a name is being edited or saved.

Editing state is shared across windows. Each save includes the complete editing
snapshot and the top-level fields changed since this window's last submitted
snapshot. Inside the write transaction, the store merges those fields into the
stored document and validates it; an invalid merge takes the complete submitted
snapshot. A later save of the same field wins. Receipts carry no editing state,
and other windows' values are not propagated into a live session.

GTK always attempts the authoritative switch/claim. On a live conflict it focuses
the owning GTK window where available. A stale SwitchToWindow menu action uses
the same path; if the external owner disappeared during the attempt, it retries
the claim rather than retaining the stale error.

Lock failures fail closed; database export refuses paths inside the lock store.

## Startup always adopts a workspace

Every host (GTK, Web, Android, Apple and Windows) starts through
`WorkspaceController`, in up to three stages. An error before the first
adoption moves startup to a later stage:

1. **Stored**: start from the store as it is.
2. **Replaced**: only for an unsupported table schema, `StoreRequest::Reset`
   keeps a copy of it and empties it, then startup begins again. SQLite copies
   the database and its log to
   `workspaces.unreadable.sqlite3`; the browser keeps the previous record under
   the id `unreadable`. Each reset replaces the previous copy.
3. **In memory**: after any other error, or a failed replacement, continue on a
   `BrowserDatabase` owned by the manager. Nothing in this window is persisted,
   and the next launch tries the store again.

Stages 2 and 3 raise the shared canvas notice to explain what happened (GTK, Web,
Android and Windows show it). Stage 3 reads nothing that was stored: it seeds the
built-in presets from code into an empty store and adopts one. Startup
therefore always ends with an adopted workspace, provided the presets adopt on
that platform and every storage request eventually replies or fails. Tests
check the presets for every `Platform`. The transports fail rather than wait:
SQLite has a busy timeout, and the web store rejects on IndexedDB errors,
blocked upgrades and worker failures.

Editing state written by 1.0.15 is converted on read; unreadable editing and
working-state fields use their defaults. App-state documents accept unknown
option fields and do not reject a readable document for its version number.
`SCHEMA_VERSION` identifies the tables: a store of any other schema version
serves nothing but `Reset`.
Unreadable workspace and toolbar rows remain listed and are skipped during
startup and retention. Pending deliveries and switcher preferences remain
opaque until read; unreadable ones are skipped and kept. Component collection
scans raw item and delivery JSON and keeps referenced components. If it cannot
follow a malformed graph, it skips collection. When no workspace can be read,
startup creates a new workspace from the included layout and leaves the
unreadable rows untouched. Copy protection precedes schema setup, maintenance
and ownership writes; copy retention runs only after adoption. See
[version copies](storage.md#workspace-version-copies).
`Reset` refuses a newer table schema. A build that cannot read that schema
keeps the store and runs in memory.

## Verification

`cargo test --locked -p layer-workspace --features native` covers real process
kill with another client alive, interrupted publication and pending delivery,
live ownership past heartbeat expiry, same-process window teardown, queued
unadopted claims, failed teardown cleanup, racing claims, failed SQL publication,
stale writes/releases, path aliases, lock I/O errors, recovery/maintenance and
browser lease behavior.

Startup is covered by `startup_on_every_platform_adopts_from_empty_or_unusable_storage`
and `startup_survives_a_failure_at_every_storage_request`, which fails each
startup request in turn, with and without a healing reset.
`startup_replaces_only_storage_it_cannot_read` checks that invalid records,
unavailable, busy, owned, failed and full storage never reach `Reset`, and
`stores_of_other_versions_serve_only_reset` checks the kept copy. The SQLite cases
`sqlite_startup_creates_one_workspace_and_keeps_unreadable_rows` and
`sqlite_startup_keeps_a_newer_store_and_runs_in_memory` cover the rest. The same
journey runs on real storage with
`node apps/layer-web/test.mjs --headless --stale-storage` and
`bash tools/performance/workspace-motion.sh gtk
--native-test=native_unreadable_workspace_storage_input --native-storage`.

`editing_saves_from_two_windows_keep_both_changes` covers independent editing
fields, repeated saves and later writes to the same field; the shared store
contract also checks duplicate delivery through SQLite and IndexedDB.

`bash tools/performance/workspace-motion.sh gtk
--native-test=native_workspace_ownership_input --native-storage` runs the GTK
switch/focus/reclaim journey in the private compositor with disposable storage.
No acceptance on Linux implies Windows, Apple or physical-tablet GUI testing.

`node apps/layer-web/test.mjs --headless --workspace-windows` navigates a window
away and back three times within the lease and requires the same workspace
without a copy, then checks two live windows, independent saved Zen state,
reload identity and takeover. Drawing checkpoints finish before navigation so
this journey tests workspace ownership independently of interrupted recovery.
