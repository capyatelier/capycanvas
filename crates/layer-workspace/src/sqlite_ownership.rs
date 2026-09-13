//! Kernel locks are native liveness; database owner/epoch/fence remains write
//! authority. Every probe/acquisition happens inside an IMMEDIATE transaction,
//! so observing an unlocked file and retiring its old claim cannot race a claim.
use super::*;
use std::{collections::BTreeMap, fs::File, path::PathBuf};

pub(super) struct Ownership {
    pub(super) directory: PathBuf,
    held: BTreeMap<String, Held>,
}
struct Held {
    owner: Owner,
    fence: u64,
    _file: File,
}
impl Ownership {
    pub(super) fn new(database: &Path) -> Result<Self> {
        // Canonicalize the existing database, not its display spelling: callers
        // using a symlink must coordinate on the same lock files.
        let database = std::fs::canonicalize(database).map_err(unavailable)?;
        let directory = lock_directory(&database);
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory).map_err(unavailable)?;
        Ok(Self {
            directory,
            held: BTreeMap::new(),
        })
    }

    fn try_lock(&self, id: &str) -> Result<Option<File>> {
        // IDs are not paths. Keep the files permanently: unlink/recreate would
        // allow two processes to lock different inodes for the same workspace.
        let path = self.directory.join(content_id(id.as_bytes()));
        let mut options = File::options();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(unavailable)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(unavailable(error)),
        }
    }

    pub(super) fn reconcile(&mut self, tx: &rusqlite::Transaction<'_>, now: u64) -> Result<()> {
        // Do not decode layouts/metadata here: a corrupt item must not prevent
        // the rest of the catalog from loading or stale ownership from clearing.
        let rows = tx
            .prepare("SELECT id,owner,epoch,fence,lease_until FROM items WHERE owner IS NOT NULL")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.held.retain(|id, held| {
            rows.iter().any(|r| {
                &r.0 == id
                    && r.1 == held.owner.id
                    && r.2.as_ref() == Some(&held.owner.epoch)
                    && r.3 == held.fence.to_string()
            })
        });
        for (id, _, _, _, until) in rows {
            let live = self.held.contains_key(&id) || self.try_lock(&id)?.is_none();
            if !live {
                tx.execute(
                    "UPDATE items SET owner=NULL,epoch=NULL,lease_until=NULL WHERE id=?1",
                    [&id],
                )?;
            } else if until
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|until| until <= now)
            {
                // Preserve the shared lease-shaped API without allowing missed
                // native heartbeats (sleep, suspension, busy UI) to steal a lock.
                tx.execute(
                    "UPDATE items SET lease_until=?2 WHERE id=?1",
                    params![id, now.saturating_add(OWNER_LEASE_MS).to_string()],
                )?;
            }
        }
        Ok(())
    }

    /// A new guard stays local until the SQL commit succeeds; rollback drops it.
    pub(super) fn acquire(&self, id: &str, owner: &Owner) -> Result<Option<File>> {
        if let Some(held) = self.held.get(id) {
            return if held.owner == *owner {
                Ok(None)
            } else {
                Err(occupied())
            };
        }
        self.try_lock(id)?.map(Some).ok_or_else(occupied)
    }
    pub(super) fn publish(&mut self, id: &str, owner: &Owner, fence: u64, file: Option<File>) {
        if let Some(file) = file {
            self.held.insert(
                id.into(),
                Held {
                    owner: owner.clone(),
                    fence,
                    _file: file,
                },
            );
        } else if let Some(held) = self.held.get_mut(id) {
            held.fence = fence;
        }
    }
    pub(super) fn release(&mut self, id: &str, owner: &Owner, fence: u64) {
        if self
            .held
            .get(id)
            .is_some_and(|h| h.owner == *owner && h.fence == fence)
        {
            self.held.remove(id);
        }
    }
    pub(super) fn retire(&mut self, owner: &Owner) {
        self.held.retain(|_, held| held.owner != *owner);
    }
}
pub(crate) fn lock_directory(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push("-locks");
    PathBuf::from(path)
}
fn occupied() -> StoreError {
    StoreError::new(
        ErrorKind::OwnedElsewhere,
        "This workspace is open in another window. Switch to that window or duplicate it.",
    )
}
fn unavailable(error: std::io::Error) -> StoreError {
    StoreError::new(
        ErrorKind::Unavailable,
        format!("Workspace ownership lock: {error}"),
    )
}
