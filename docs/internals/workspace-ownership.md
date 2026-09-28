# Native workspace ownership

[Editor internals](README.md)

Native clients share `layer-workspace`'s SQLite worker. Each claimed item has an
exclusive kernel file lock, retained until its ownership is released. SQL's
owner UUID, epoch, fence and generation checks still authorize writes. The files
are under `<database>-locks/`, keyed by a hash of the item ID; existence is not
ownership. Never unlink/recreate a lock file while clients may be running.

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

GTK always attempts the authoritative switch/claim. On a live conflict it focuses
the owning GTK window where available. A stale SwitchToWindow menu action uses
the same path; if the external owner disappeared during the attempt, it retries
the claim rather than retaining the stale error.

Lock failures fail closed; database export refuses paths inside the lock store.

## Startup always adopts a workspace

Every host (GTK, Web, Android, Apple and Windows) starts through
`WorkspaceController`, in up to three stages. Any error before the first
adoption, whether from storage, decoding, validation, claiming or
`adopt_workspace`, moves startup to the next stage:

1. **Stored**: start from the store as it is.
2. **Replaced**: `StoreRequest::Reset` empties the store, then start again.
3. **In memory**: continue on a `BrowserDatabase` owned by the manager. Nothing
   in this window is persisted.

Stages 2 and 3 raise the shared canvas notice to explain what happened (GTK, Web
and Android show it). Stage 3 reads nothing that was stored: it seeds the
built-in presets from code into an empty store and adopts one. Startup
therefore always ends with an adopted workspace, provided the presets adopt on
that platform and every storage request eventually replies or fails. Tests
check the presets for every `Platform`. The transports fail rather than wait:
SQLite has a busy timeout, and the web store rejects on IndexedDB errors,
blocked upgrades and worker failures.

Formats are not migrated while they are unstable. `SCHEMA_VERSION` only
identifies a store: a store of any other version serves nothing but `Reset`.
Startup maintenance decodes every item and pending delivery, so a store this
build cannot fully read fails stage 1 whether or not the version was bumped.
`Reset` refuses a store written by a newer build. A window still running an
older build keeps that store and runs in memory.

## Verification

`cargo test --locked -p layer-workspace --features native` covers real process
kill with another client alive, interrupted publication and pending delivery,
live ownership past heartbeat expiry, same-process window teardown, queued
unadopted claims, failed teardown cleanup, racing claims, failed SQL publication,
stale writes/releases, path aliases, lock I/O errors, recovery/maintenance and
browser lease behavior.

Startup is covered by `startup_on_every_platform_adopts_from_empty_or_unusable_storage`
and `startup_survives_a_failure_at_every_storage_request`, which fails each
startup request in turn, with and without a healing reset. The SQLite cases
`sqlite_startup_replaces_workspaces_of_the_same_version_it_cannot_read` and
`sqlite_startup_keeps_a_newer_store_and_runs_in_memory` cover the rest. The same
journey runs on real storage with
`node apps/layer-web/test.mjs --headless --workspace-startup` and
`bash tools/performance/workspace-motion.sh gtk
--native-test=native_unreadable_workspace_storage_input --native-storage`.

`bash tools/performance/workspace-motion.sh gtk
--native-test=native_workspace_ownership_input --native-storage` runs the GTK
switch/focus/reclaim journey in the private compositor with disposable storage.
No acceptance on Linux implies Windows, Apple or physical-tablet GUI testing.

`node apps/layer-web/test.mjs --headless --workspace-windows` navigates a window
away and back three times within the lease and requires the same workspace
without a copy, then checks two live windows, reload identity and takeover.
