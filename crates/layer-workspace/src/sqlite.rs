use crate::protocol::{PreparedWrite, unpack};
use crate::store_rules::*;
use crate::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, StoreError>;
#[path = "sqlite_ownership.rs"]
pub(crate) mod ownership;
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}
struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        let kind = match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::CannotOpen | rusqlite::ErrorCode::PermissionDenied) => {
                ErrorKind::Unavailable
            }
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                ErrorKind::Conflict
            }
            Some(rusqlite::ErrorCode::ConstraintViolation) => {
                if matches!(&error,rusqlite::Error::SqliteFailure(code,_) if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE)
                {
                    ErrorKind::NameCollision
                } else {
                    ErrorKind::FailedWrite
                }
            }
            Some(rusqlite::ErrorCode::DiskFull) => ErrorKind::StorageFull,
            _ => ErrorKind::FailedWrite,
        };
        Self::new(kind, format!("Workspace storage: {error}"))
    }
}

/// Use on a storage worker only. One transaction publishes each prepared batch.
pub struct SqliteStore {
    connection: Connection,
    clock: Arc<dyn Clock>,
    ownership: ownership::Ownership,
}
impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self> {
        Self::with_clock(path, Arc::new(SystemClock))
    }
    pub fn with_clock(path: &Path, clock: Arc<dyn Clock>) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(parent)
                .map_err(|e| StoreError::new(ErrorKind::Unavailable, e.to_string()))?;
        }
        let unavailable =
            |e: std::io::Error| StoreError::new(ErrorKind::Unavailable, e.to_string());
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map_err(unavailable)?;
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "wal_autocheckpoint", 1000)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(newer_schema());
        }
        if version < SCHEMA_VERSION {
            // A new, unversioned or older store becomes an empty current one;
            // older workspaces are discarded, not migrated (see newer_schema).
            let tables = tx
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for table in tables {
                tx.execute(
                    &format!("DROP TABLE \"{}\"", table.replace('"', "\"\"")),
                    [],
                )?;
            }
            tx.execute_batch("CREATE TABLE items (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, name_key TEXT NOT NULL,
                metadata TEXT NOT NULL, content TEXT NOT NULL, working TEXT,
                metadata_generation TEXT NOT NULL, layout_generation TEXT NOT NULL, working_generation TEXT NOT NULL,
                builtin INTEGER NOT NULL,
                fence TEXT NOT NULL DEFAULT '0', owner TEXT, epoch TEXT, lease_until TEXT);
                CREATE UNIQUE INDEX item_names ON items(kind,name_key);
                CREATE TABLE components (id TEXT PRIMARY KEY, json TEXT NOT NULL);
                CREATE TABLE receipts (id TEXT PRIMARY KEY, hash TEXT NOT NULL, receipt TEXT NOT NULL,
                    owner TEXT NOT NULL, epoch TEXT NOT NULL, acknowledged INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE pending (id TEXT PRIMARY KEY, hash TEXT NOT NULL, payload TEXT NOT NULL);
                CREATE TABLE bindings (key TEXT PRIMARY KEY, item_id TEXT NOT NULL);
                CREATE TABLE tombstones (id TEXT PRIMARY KEY, fence TEXT NOT NULL);
                CREATE TABLE cancelled_operations (id TEXT PRIMARY KEY);
                CREATE TABLE workspace_switcher (id INTEGER PRIMARY KEY CHECK(id=1), workspace_ids TEXT NOT NULL);
                CREATE TABLE workspace_order (id INTEGER PRIMARY KEY CHECK(id=1), workspace_ids TEXT NOT NULL);")?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        tx.commit()?;
        Ok(Self {
            connection,
            clock,
            ownership: ownership::Ownership::new(path)?,
        })
    }
    pub fn handle(&mut self, request: StoreRequest) -> Result<StoreResponse> {
        match request {
            StoreRequest::Switcher => self
                .preference_ids("workspace_switcher")
                .map(StoreResponse::Switcher),
            StoreRequest::UpdateSwitcher { expected, ids } => self
                .update_preference_ids("workspace_switcher", expected, ids)
                .map(StoreResponse::Switcher),
            StoreRequest::WorkspaceOrder => self
                .preference_ids("workspace_order")
                .map(StoreResponse::WorkspaceOrder),
            StoreRequest::UpdateWorkspaceOrder { expected, ids } => self
                .update_preference_ids("workspace_order", expected, ids)
                .map(StoreResponse::WorkspaceOrder),
            StoreRequest::List => self.list().map(StoreResponse::List),
            StoreRequest::Maintenance { owner, clear_older } => {
                self.maintenance(owner.as_ref(), clear_older)?;
                Ok(StoreResponse::Done)
            }
            StoreRequest::Load { id } => self.load(&id).map(Box::new).map(StoreResponse::Entity),
            StoreRequest::Claim { id, owner } => self
                .claim(&id, owner)
                .map(Box::new)
                .map(StoreResponse::Entity),
            StoreRequest::Renew { id, owner, fence } => self
                .renew(&id, &owner, parse_counter(&fence)?)
                .map(StoreResponse::Claim),
            StoreRequest::Release { id, owner, fence } => {
                self.release(&id, &owner, parse_counter(&fence)?)?;
                Ok(StoreResponse::Done)
            }
            StoreRequest::Commit { batch } => {
                let result = self.commit(batch.clone());
                if result
                    .as_ref()
                    .is_err_and(|e| e.kind == ErrorKind::StorageFull)
                {
                    // The immutable delivery retains its ID and bytes. Defer
                    // every live owner, including the target of this write, so
                    // cleanup cannot invalidate its expected generations.
                    let _ = self.maintenance(None, true);
                    self.commit(batch).map(StoreResponse::Committed)
                } else {
                    result.map(StoreResponse::Committed)
                }
            }
            StoreRequest::Acknowledge { operation_id } => {
                let tx = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                tx.execute(
                    "UPDATE receipts SET acknowledged=1 WHERE id=?1",
                    [&operation_id],
                )?;
                tx.execute("DELETE FROM pending WHERE id=?1 AND EXISTS (SELECT 1 FROM receipts WHERE id=?1)", [&operation_id])?;
                tx.execute("DELETE FROM receipts WHERE id IN (SELECT id FROM receipts WHERE acknowledged=1 AND NOT EXISTS(SELECT 1 FROM pending WHERE pending.id=receipts.id) ORDER BY rowid DESC LIMIT -1 OFFSET 256)",[])?;
                tx.commit()?;
                Ok(StoreResponse::Done)
            }
            StoreRequest::Receipt { operation_id } => Ok(StoreResponse::Receipt(
                receipt(&self.connection, &operation_id)?.map(|(_, receipt)| receipt),
            )),
            StoreRequest::Binding { key } => Ok(StoreResponse::Binding(
                self.connection
                    .query_row("SELECT item_id FROM bindings WHERE key=?1", [key], |r| {
                        r.get(0)
                    })
                    .optional()?,
            )),
            StoreRequest::Pending => {
                let mut statement = self
                    .connection
                    .prepare("SELECT payload FROM pending ORDER BY rowid")?;
                let texts = statement
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(StoreResponse::Pending(
                    texts
                        .iter()
                        .map(|v| serde_json::from_str(v).map_err(StoreError::from))
                        .collect::<Result<_>>()?,
                ))
            }
            StoreRequest::Reopen => {
                self.connection
                    .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
                Ok(StoreResponse::Done)
            }
        }
    }
    pub fn load(&mut self, id: &str) -> Result<StoredEntity> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, self.clock.now_ms())?;
        let result = load(&tx, id)?;
        tx.commit()?;
        Ok(result)
    }
    fn update_preference_ids(
        &mut self,
        table: &str,
        expected: Option<Vec<String>>,
        ids: Vec<String>,
    ) -> Result<Option<Vec<String>>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<String> = tx
            .query_row(
                &format!("SELECT workspace_ids FROM {table} WHERE id=1"),
                [],
                |r| r.get(0),
            )
            .optional()?;
        let current: Option<Vec<String>> = current.map(|s| serde_json::from_str(&s)).transpose()?;
        if update_preference(current.as_ref(), expected, &ids, |id| {
            Ok(header(&tx, id)?.kind == "workspace")
        })? {
            tx.execute(&format!("INSERT INTO {table}(id,workspace_ids) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET workspace_ids=excluded.workspace_ids"), [serde_json::to_string(&ids)?])?;
        }
        tx.commit()?;
        Ok(Some(ids))
    }
    fn preference_ids(&self, table: &str) -> Result<Option<Vec<String>>> {
        let json: Option<String> = self
            .connection
            .query_row(
                &format!("SELECT workspace_ids FROM {table} WHERE id=1"),
                [],
                |r| r.get(0),
            )
            .optional()?;
        let ids: Option<Vec<String>> = json.map(|s| serde_json::from_str(&s)).transpose()?;
        if let Some(ids) = &ids {
            validate_switcher_ids(ids)?;
        }
        Ok(ids)
    }
    pub fn list(&mut self) -> Result<Vec<ItemSummary>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, self.clock.now_ms())?;
        let ids = {
            let mut statement = tx.prepare("SELECT id FROM items ORDER BY name_key,id")?;
            statement
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let values = ids
            .into_iter()
            .map(|id| {
                let result = (|| -> Result<ItemSummary> {
                    let row = header(&tx, &id)?;
                    let metadata: Metadata = serde_json::from_str(&row.metadata)?;
                    metadata.validate()?;
                    Ok(ItemSummary {
                        id: id.clone(),
                        metadata,
                        generations: row.generations,
                        claim: row.claim,
                        error: None,
                    })
                })();
                match result {
                    Ok(summary) => Ok(summary),
                    Err(error) => {
                        let (name, kind): (String, String) =
                            tx.query_row("SELECT name,kind FROM items WHERE id=?1", [&id], |r| {
                                Ok((r.get(0)?, r.get(1)?))
                            })?;
                        let row = header(&tx, &id).ok();
                        Ok(summary_fallback(
                            id,
                            Some(&kind),
                            &name,
                            row.as_ref().is_some_and(|r| r.builtin),
                            row.as_ref().map(|r| r.generations).unwrap_or_default(),
                            row.and_then(|r| r.claim),
                            error.to_string(),
                        ))
                    }
                }
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(values)
    }
    pub fn claim(&mut self, id: &str, owner: Owner) -> Result<StoredEntity> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, now)?;
        let row = header(&tx, id)?;
        let fence = claim_fence(row.claim.as_ref(), &owner, row.fence, now)?;
        load(&tx, id)?;
        let guard = self.ownership.acquire(id, &owner)?;
        tx.execute(
            "UPDATE items SET owner=?2,epoch=?3,fence=?4,lease_until=?5 WHERE id=?1",
            params![
                id,
                owner.id,
                owner.epoch,
                fence.to_string(),
                now.saturating_add(OWNER_LEASE_MS).to_string()
            ],
        )?;
        // Ownership acquisition and the returned snapshot are one transaction.
        let result = load(&tx, id)?;
        tx.commit()?;
        self.ownership.publish(id, &owner, fence, guard);
        Ok(result)
    }
    pub fn renew(&mut self, id: &str, owner: &Owner, fence: u64) -> Result<Claim> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, now)?;
        check_claim(header(&tx, id)?.claim.as_ref(), owner, fence, now)?;
        let claim = Claim {
            owner: owner.clone(),
            fence,
            expires_at_ms: now.saturating_add(OWNER_LEASE_MS),
        };
        tx.execute(
            "UPDATE items SET lease_until=?2 WHERE id=?1",
            params![id, claim.expires_at_ms.to_string()],
        )?;
        tx.commit()?;
        Ok(claim)
    }
    pub fn release(&mut self, id: &str, owner: &Owner, fence: u64) -> Result<()> {
        // A delayed release must never clear a successor's claim.
        self.connection.execute("UPDATE items SET owner=NULL,epoch=NULL,lease_until=NULL WHERE id=?1 AND owner=?2 AND epoch=?3 AND fence=?4", params![id, owner.id, owner.epoch, fence.to_string()])?;
        self.ownership.release(id, owner, fence);
        Ok(())
    }
    pub(crate) fn retire_owner(&mut self, owner: &Owner) {
        // The window no longer exists. Even a failed database cleanup must not
        // pin its kernel locks in a worker shared by the surviving windows.
        // A subsequent transaction reclaims any leftover row; pending deliveries
        // and receipts remain available for recovery.
        let _ = self.connection.execute(
            "UPDATE items SET owner=NULL,epoch=NULL,lease_until=NULL WHERE owner=?1 AND epoch=?2",
            params![owner.id, owner.epoch],
        );
        self.ownership.retire(owner);
    }
    pub fn commit(&mut self, batch: CommitBatch) -> Result<CommitReceipt> {
        let encoded = batch.encoded()?;
        let hash = content_id(encoded.as_bytes());
        let cancelled: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cancelled_operations WHERE id=?1)",
            [&batch.operation_id],
            |r| r.get(0),
        )?;
        if cancelled {
            return Err(recovered_elsewhere());
        }
        if let Some((original_hash, original)) = receipt(&self.connection, &batch.operation_id)? {
            return if original_hash == hash {
                Ok(original)
            } else {
                Err(StoreError::invalid(
                    "An operation ID cannot be reused for a different change.",
                ))
            };
        }
        validate_payload(&batch, |id| component(&self.connection, id))?;
        // A pending delivery survives interruption until its receipt is acknowledged.
        self.connection.execute(
            "INSERT INTO pending(id,hash,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO NOTHING",
            params![batch.operation_id, hash, encoded],
        )?;
        let saved_hash: String = self.connection.query_row(
            "SELECT hash FROM pending WHERE id=?1",
            [&batch.operation_id],
            |r| r.get(0),
        )?;
        if saved_hash != hash {
            return Err(StoreError::invalid(
                "An operation ID is already bound to another payload.",
            ));
        }
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, now)?;
        // Another process may have delivered the operation while this writer waited.
        let cancelled: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cancelled_operations WHERE id=?1)",
            [&batch.operation_id],
            |r| r.get(0),
        )?;
        if cancelled {
            tx.execute("DELETE FROM pending WHERE id=?1", [&batch.operation_id])?;
            tx.commit()?;
            return Err(recovered_elsewhere());
        }
        if let Some((original_hash, original)) = receipt(&tx, &batch.operation_id)? {
            return if original_hash == hash {
                Ok(original)
            } else {
                Err(StoreError::invalid("Conflicting operation receipt."))
            };
        }
        for (id, json) in &batch.components {
            tx.execute(
                "INSERT INTO components(id,json) VALUES(?1,?2) ON CONFLICT(id) DO NOTHING",
                params![id, json],
            )?;
        }
        let mut result = CommitReceipt {
            operation_id: batch.operation_id.clone(),
            items: Vec::new(),
        };
        for id in &batch.abandon_operations {
            if id == &batch.operation_id || id.len() > 128 {
                return Err(StoreError::invalid("Invalid recovery delivery."));
            }
            if receipt(&tx, id)?.is_some() {
                return Err(already_saved());
            }
            let original: Option<String> = tx
                .query_row("SELECT payload FROM pending WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(original) = original {
                let original: CommitBatch = serde_json::from_str(&original)?;
                let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE owner=?1 AND epoch=?2 AND CAST(lease_until AS INTEGER)>?3)",params![original.owner.id,original.owner.epoch,now as i64],|r|r.get(0))?;
                if original.owner != batch.owner && live {
                    return Err(StoreError::new(
                        ErrorKind::OwnedElsewhere,
                        "The source window is still saving these changes.",
                    ));
                }
            }
            tx.execute("INSERT INTO cancelled_operations(id) VALUES(?1)", [id])?;
            tx.execute("DELETE FROM pending WHERE id=?1", [id])?;
        }
        let mut guards = Vec::new();
        let mut released = Vec::new();
        for write in &batch.writes {
            if write.create && write.claim {
                guards.push((
                    write.id.clone(),
                    self.ownership.acquire(&write.id, &batch.owner)?,
                ));
            }
            let generations = apply_write(&tx, write, &batch.owner, now)?;
            if write.delete || (!write.create && header(&tx, &write.id)?.claim.is_none()) {
                released.push((write.id.clone(), write.fence));
            }
            result.items.push((write.id.clone(), generations));
        }
        if !batch.pin_workspaces.is_empty() {
            let pins: Option<String> = tx
                .query_row(
                    "SELECT workspace_ids FROM workspace_switcher WHERE id=1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let pins = pins.map(|s| serde_json::from_str(&s)).transpose()?;
            let pins = with_created_pins(pins, &batch.pin_workspaces)?;
            tx.execute("INSERT INTO workspace_switcher(id,workspace_ids) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET workspace_ids=excluded.workspace_ids", [serde_json::to_string(&pins)?])?;
        }
        for (key, id) in &batch.bindings {
            if let Some(id) = id {
                let row = header(&tx, id)?;
                check_binding(
                    row.kind == "workspace",
                    row.claim.as_ref(),
                    &batch.owner,
                    now,
                )?;
                tx.execute("INSERT INTO bindings(key,item_id) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET item_id=excluded.item_id", params![key, id])?;
            } else {
                tx.execute("DELETE FROM bindings WHERE key=?1", [key])?;
            }
        }
        tx.execute(
            "INSERT INTO receipts(id,hash,receipt,owner,epoch) VALUES(?1,?2,?3,?4,?5)",
            params![
                batch.operation_id,
                hash,
                serde_json::to_string(&result)?,
                batch.owner.id,
                batch.owner.epoch
            ],
        )?;
        tx.commit()?;
        for (id, guard) in guards {
            self.ownership.publish(&id, &batch.owner, 1, guard);
        }
        for (id, fence) in released {
            self.ownership.release(&id, &batch.owner, fence);
        }
        Ok(result)
    }
}
struct Header {
    metadata: String,
    kind: String,
    builtin: bool,
    generations: Generations,
    fence: u64,
    claim: Option<Claim>,
}
fn header(connection: &Connection, id: &str) -> Result<Header> {
    let row = connection.query_row("SELECT metadata,kind,builtin,metadata_generation,layout_generation,working_generation,fence,owner,epoch,lease_until FROM items WHERE id=?1", [id], |r| Ok((
        r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, bool>(2)?,
        r.get::<_, String>(3)?,r.get::<_, String>(4)?,r.get::<_, String>(5)?,r.get::<_, String>(6)?,
        r.get::<_, Option<String>>(7)?,r.get::<_, Option<String>>(8)?,r.get::<_, Option<String>>(9)?,
    ))).optional()?.ok_or_else(not_found)?;
    let fence = parse_counter(&row.6)?;
    let claim = match (row.7, row.8, row.9) {
        (Some(id), Some(epoch), Some(until)) => Some(Claim {
            owner: Owner { id, epoch },
            fence,
            expires_at_ms: parse_counter(&until)?,
        }),
        (None, None, None) => None,
        _ => return Err(StoreError::invalid("Invalid workspace ownership record.")),
    };
    Ok(Header {
        metadata: row.0,
        kind: row.1,
        builtin: row.2,
        generations: Generations {
            metadata: parse_counter(&row.3)?,
            layout: parse_counter(&row.4)?,
            working: parse_counter(&row.5)?,
        },
        fence,
        claim,
    })
}
fn component(connection: &Connection, id: &str) -> Result<String> {
    connection
        .query_row("SELECT json FROM components WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(missing_resource)
}
fn load(connection: &Connection, id: &str) -> Result<StoredEntity> {
    let row = header(connection, id)?;
    let (content, working): (String, Option<String>) =
        connection.query_row("SELECT content,working FROM items WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let entity = Entity {
        id: id.into(),
        metadata: serde_json::from_str(&row.metadata)?,
        content: unpack(&content, |id| component(connection, id))?,
        working: working.map(|v| serde_json::from_str(&v)).transpose()?,
    };
    entity.validate()?;
    Ok(StoredEntity {
        entity,
        generations: row.generations,
        claim: row.claim,
    })
}
fn receipt(connection: &Connection, id: &str) -> Result<Option<(String, CommitReceipt)>> {
    connection
        .query_row("SELECT hash,receipt FROM receipts WHERE id=?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .optional()?
        .map(|(hash, text)| Ok((hash, serde_json::from_str(&text)?)))
        .transpose()
}
fn apply_write(
    connection: &Connection,
    write: &PreparedWrite,
    owner: &Owner,
    now: u64,
) -> Result<Generations> {
    if write.create {
        let metadata = write
            .metadata
            .as_ref()
            .ok_or_else(|| StoreError::invalid("A new item needs metadata."))?;
        let content = write
            .content_json
            .as_ref()
            .ok_or_else(|| StoreError::invalid("A new item needs content."))?;
        let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=?1 UNION SELECT 1 FROM tombstones WHERE id=?1)", [&write.id], |r| r.get(0))?;
        if exists {
            return Err(StoreError::conflict());
        }
        let metadata = resolve_name(connection, metadata.clone(), &write.id, write.name_policy)?;
        let generations = Generations {
            metadata: 1,
            layout: 1,
            working: u64::from(write.working_json.is_some()),
        };
        connection.execute("INSERT INTO items(id,kind,name,name_key,metadata,content,working,metadata_generation,layout_generation,working_generation,builtin,fence,owner,epoch,lease_until) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)", params![
            write.id, metadata.kind.key(), metadata.name, name_key(&metadata.name), serde_json::to_string(&metadata)?, content, write.working_json,
            generations.metadata.to_string(), generations.layout.to_string(), generations.working.to_string(), metadata.builtin,
            if write.claim { "1" } else { "0" }, write.claim.then_some(&owner.id), write.claim.then_some(&owner.epoch), write.claim.then(|| now.saturating_add(OWNER_LEASE_MS).to_string()),
        ])?;
        return Ok(generations);
    }
    let row = header(connection, &write.id)?;
    check_claim(row.claim.as_ref(), owner, write.fence, now)?;
    check_update(
        &serde_json::from_str(&row.metadata)?,
        row.generations,
        write,
    )?;
    if write.delete {
        remove(connection, &write.id, row.fence)?;
        return Ok(row.generations);
    }
    let mut generations = row.generations;
    if let Some(metadata) = &write.metadata {
        let metadata = resolve_name(connection, metadata.clone(), &write.id, write.name_policy)?;
        generations.metadata = advance(generations.metadata)?;
        connection.execute(
            "UPDATE items SET name=?2,name_key=?3,metadata=?4,metadata_generation=?5 WHERE id=?1",
            params![
                write.id,
                metadata.name,
                name_key(&metadata.name),
                serde_json::to_string(&metadata)?,
                generations.metadata.to_string()
            ],
        )?;
    }
    if let Some(content) = &write.content_json {
        generations.layout = advance(generations.layout)?;
        connection.execute(
            "UPDATE items SET content=?2,layout_generation=?3 WHERE id=?1",
            params![write.id, content, generations.layout.to_string()],
        )?;
    }
    if let Some(working) = &write.working_json {
        generations.working = advance(generations.working)?;
        connection.execute(
            "UPDATE items SET working=?2,working_generation=?3 WHERE id=?1",
            params![write.id, working, generations.working.to_string()],
        )?;
    }
    Ok(generations)
}
fn remove(connection: &Connection, id: &str, fence: u64) -> Result<()> {
    connection.execute("INSERT INTO tombstones(id,fence) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET fence=excluded.fence",params![id,advance(fence)?.to_string()])?;
    connection.execute("DELETE FROM bindings WHERE item_id=?1", [id])?;
    connection.execute("DELETE FROM items WHERE id=?1", [id])?;
    Ok(())
}
fn resolve_name(
    connection: &Connection,
    metadata: Metadata,
    id: &str,
    policy: NamePolicy,
) -> Result<Metadata> {
    let kind = metadata.kind.key();
    apply_name_policy(metadata, policy, |key| {
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE kind=?1 AND name_key=?2 AND id!=?3)",
            params![kind, key, id],
            |r| r.get(0),
        )?)
    })
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[path = "sqlite_maintenance.rs"]
mod maintenance;
