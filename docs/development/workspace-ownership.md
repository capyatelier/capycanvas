# Native workspace ownership

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

SQLite's native protocol version is now 5; browser/package schema remains 4.
Saved layouts/working state are preserved. Older native builds reject the newer
store. Close all older app instances before first launching this development
build: already-running pre-lock binaries cannot participate in the new protocol.
Lock failures fail closed; database export refuses paths inside the lock store.

## Verification

`cargo test --locked -p layer-workspace --features native` covers real process
kill with another client alive, interrupted publication and pending delivery,
live ownership past heartbeat expiry, same-process window teardown, queued
unadopted claims, failed teardown cleanup, racing claims, failed SQL publication,
stale writes/releases, path aliases, lock I/O errors, recovery/maintenance and
browser lease behavior.

`bash tools/performance/workspace-motion.sh gtk
--native-test=native_workspace_ownership_input --native-storage` runs the GTK
switch/focus/reclaim journey in the private compositor with disposable storage.
No acceptance on Linux implies Windows, Apple or physical-tablet GUI testing.

`node apps/layer-web/test.mjs --headless --workspace-windows` navigates a window
away and back three times within the lease and requires the same workspace
without a copy, then checks two live windows, reload identity and takeover.
