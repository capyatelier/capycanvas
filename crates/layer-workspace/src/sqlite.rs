use crate::protocol::{PreparedWrite, unpack};
use crate::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, StoreError>;
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .map_err(|e| StoreError::new(ErrorKind::Unavailable, e.to_string()))?;
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::new(
                ErrorKind::UnsupportedSchema,
                "This workspace database needs a newer version of Capy Canvas. Its contents have been preserved.",
            ));
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "wal_autocheckpoint", 1000)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            let tables: u32 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get(0))?;
            if tables != 0 {
                return Err(StoreError::new(
                    ErrorKind::UnsupportedSchema,
                    "Unrecognized workspace database. The original data has been preserved.",
                ));
            }
            tx.execute_batch("CREATE TABLE items (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, name_key TEXT NOT NULL,
                metadata TEXT NOT NULL, content TEXT NOT NULL, working TEXT,
                metadata_generation TEXT NOT NULL, layout_generation TEXT NOT NULL, working_generation TEXT NOT NULL,
                deleted_at TEXT, builtin INTEGER NOT NULL,
                fence TEXT NOT NULL DEFAULT '0', owner TEXT, epoch TEXT, lease_until TEXT);
                CREATE UNIQUE INDEX item_names ON items(kind,name_key) WHERE deleted_at IS NULL;
                CREATE TABLE components (id TEXT PRIMARY KEY, bytes BLOB NOT NULL);
                CREATE TABLE receipts (id TEXT PRIMARY KEY, hash TEXT NOT NULL, receipt TEXT NOT NULL,
                    owner TEXT NOT NULL, epoch TEXT NOT NULL, acknowledged INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE pending (id TEXT PRIMARY KEY, hash TEXT NOT NULL, payload TEXT NOT NULL);
                CREATE TABLE bindings (key TEXT PRIMARY KEY, item_id TEXT NOT NULL);
                CREATE TABLE legacy_imports (source TEXT PRIMARY KEY, item_id TEXT NOT NULL);
                CREATE TABLE tombstones (id TEXT PRIMARY KEY, fence TEXT NOT NULL);")?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        if version < 2 {
            tx.execute_batch("CREATE TABLE cancelled_operations(id TEXT PRIMARY KEY)")?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        if version < 3 {
            tx.execute_batch("CREATE TABLE workspace_switcher (id INTEGER PRIMARY KEY CHECK(id=1), workspace_ids TEXT NOT NULL)")?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        if version < 4 {
            tx.execute_batch("CREATE TABLE workspace_order (id INTEGER PRIMARY KEY CHECK(id=1), workspace_ids TEXT NOT NULL)")?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        tx.commit()?;
        Ok(Self { connection, clock })
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
            StoreRequest::Maintenance {
                owner,
                clear_older,
                apply,
            } => self
                .maintenance(owner.as_ref(), clear_older, apply)
                .map(StoreResponse::Storage),
            StoreRequest::DeletePermanently { id, owner, fence } => {
                self.delete_permanently(&id, &owner, parse_counter(&fence)?)?;
                Ok(StoreResponse::Done)
            }
            StoreRequest::Load { id } => self.load(&id).map(StoreResponse::Entity),
            StoreRequest::Claim { id, owner } => self.claim(&id, owner).map(StoreResponse::Entity),
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
                    let _ = self.maintenance(None, true, true);
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
            StoreRequest::LegacyImport { source } => Ok(StoreResponse::Binding(
                self.connection
                    .query_row(
                        "SELECT item_id FROM legacy_imports WHERE source=?1",
                        [source],
                        |r| r.get(0),
                    )
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
            StoreRequest::Raw { id } => {
                let (metadata, content, working): (String, String, Option<String>) =
                    self.connection.query_row(
                        "SELECT metadata,content,working FROM items WHERE id=?1",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )?;
                Ok(StoreResponse::Raw(serde_json::to_string(
                    &serde_json::json!({"id":id,"metadata":metadata,"content":content,"working":working}),
                )?))
            }
            StoreRequest::Reopen => {
                self.connection
                    .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
                Ok(StoreResponse::Done)
            }
        }
    }
    pub fn load(&mut self, id: &str) -> Result<StoredEntity> {
        let tx = self.connection.transaction()?;
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
        validate_switcher_ids(&ids)?;
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
        if current.as_ref() != Some(&ids) {
            if current != expected {
                return Err(StoreError::new(
                    ErrorKind::Conflict,
                    "Workspace preferences changed in another window. Try again.",
                ));
            }
            for id in &ids {
                let row = header(&tx, id)?;
                if row.kind != "workspace" || row.deleted {
                    return Err(StoreError::invalid(
                        "This workspace is no longer available.",
                    ));
                }
            }
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
        let tx = self.connection.transaction()?;
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
                        let kind = match kind.as_str() {
                            "template" => ItemKind::Template,
                            "toolbar" => ItemKind::Toolbar,
                            _ => ItemKind::Workspace,
                        };
                        Ok(ItemSummary {
                            id,
                            metadata: Metadata::new(kind, &name, 0),
                            generations: Generations::default(),
                            claim: None,
                            error: Some(error.to_string()),
                        })
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
        let row = header(&tx, id)?;
        if row
            .claim
            .as_ref()
            .is_some_and(|c| c.owner != owner && c.expires_at_ms > now)
        {
            return Err(StoreError::new(
                ErrorKind::OwnedElsewhere,
                "This workspace is open in another window. Switch to that window or duplicate it.",
            ));
        }
        let fence = if row
            .claim
            .as_ref()
            .is_some_and(|c| c.owner == owner && c.expires_at_ms > now)
        {
            row.fence
        } else {
            advance(row.fence)?
        };
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
        Ok(result)
    }
    pub fn renew(&mut self, id: &str, owner: &Owner, fence: u64) -> Result<Claim> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = header(&tx, id)?;
        check_owner(&row, owner, fence, now)?;
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
        Ok(())
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
            return Err(StoreError::new(
                ErrorKind::Conflict,
                "These interrupted changes were already recovered into an independent item.",
            ));
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
        for (id, bytes) in &batch.components {
            if content_id(bytes) != *id {
                return Err(StoreError::invalid("Invalid workspace component."));
            }
        }
        for write in &batch.writes {
            if let Some(metadata) = &write.metadata {
                metadata.validate()?;
                if write.metadata_json.as_deref() != Some(&serde_json::to_string(metadata)?) {
                    return Err(StoreError::invalid("Inconsistent metadata payload."));
                }
            }
            if let Some(content) = &write.content_json {
                unpack(content, |id| {
                    batch
                        .components
                        .get(id)
                        .cloned()
                        .map(Ok)
                        .unwrap_or_else(|| component(&self.connection, id))
                })?;
            }
        }
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
        // Another process may have delivered the operation while this writer waited.
        let cancelled: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cancelled_operations WHERE id=?1)",
            [&batch.operation_id],
            |r| r.get(0),
        )?;
        if cancelled {
            tx.execute("DELETE FROM pending WHERE id=?1", [&batch.operation_id])?;
            tx.commit()?;
            return Err(StoreError::new(
                ErrorKind::Conflict,
                "These interrupted changes were already recovered into an independent item.",
            ));
        }
        if let Some((original_hash, original)) = receipt(&tx, &batch.operation_id)? {
            return if original_hash == hash {
                Ok(original)
            } else {
                Err(StoreError::invalid("Conflicting operation receipt."))
            };
        }
        for (id, bytes) in &batch.components {
            tx.execute(
                "INSERT INTO components(id,bytes) VALUES(?1,?2) ON CONFLICT(id) DO NOTHING",
                params![id, bytes],
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
                return Err(StoreError::new(
                    ErrorKind::Conflict,
                    "The interrupted changes already finished saving. Refresh the manager to view them.",
                ));
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
        for write in &batch.writes {
            let generations = apply_write(&tx, write, &batch.owner, now)?;
            result.items.push((write.id.clone(), generations));
        }
        for (key, id) in &batch.bindings {
            if let Some(id) = id {
                let row = header(&tx, id)?;
                if row.kind != "workspace" || row.deleted {
                    return Err(StoreError::invalid(
                        "The replacement workspace is unavailable.",
                    ));
                }
                let claim = row.claim.as_ref().ok_or_else(StoreError::conflict)?;
                check_owner(&row, &batch.owner, claim.fence, now)?;
                tx.execute("INSERT INTO bindings(key,item_id) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET item_id=excluded.item_id", params![key, id])?;
            } else {
                tx.execute("DELETE FROM bindings WHERE key=?1", [key])?;
            }
        }
        for (source, id) in &batch.legacy_imports {
            header(&tx, id)?;
            tx.execute(
                "INSERT INTO legacy_imports(source,item_id) VALUES(?1,?2)",
                params![source, id],
            )?;
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
        Ok(result)
    }
}
struct Header {
    metadata: String,
    kind: String,
    builtin: bool,
    deleted: bool,
    generations: Generations,
    fence: u64,
    claim: Option<Claim>,
}
fn parse_counter(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| StoreError::invalid("Invalid workspace generation."))
}
fn advance(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| StoreError::invalid("Workspace generation exhausted."))
}
fn header(connection: &Connection, id: &str) -> Result<Header> {
    let row = connection.query_row("SELECT metadata,kind,builtin,deleted_at,metadata_generation,layout_generation,working_generation,fence,owner,epoch,lease_until FROM items WHERE id=?1", [id], |r| Ok((
        r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, bool>(2)?,r.get::<_, Option<String>>(3)?,
        r.get::<_, String>(4)?,r.get::<_, String>(5)?,r.get::<_, String>(6)?,r.get::<_, String>(7)?,
        r.get::<_, Option<String>>(8)?,r.get::<_, Option<String>>(9)?,r.get::<_, Option<String>>(10)?,
    ))).optional()?.ok_or_else(|| StoreError::new(ErrorKind::NotFound, "This workspace item is no longer available."))?;
    let fence = parse_counter(&row.7)?;
    let claim = match (row.8, row.9, row.10) {
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
        deleted: row.3.is_some(),
        generations: Generations {
            metadata: parse_counter(&row.4)?,
            layout: parse_counter(&row.5)?,
            working: parse_counter(&row.6)?,
        },
        fence,
        claim,
    })
}
fn component(connection: &Connection, id: &str) -> Result<Vec<u8>> {
    connection
        .query_row("SELECT bytes FROM components WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| StoreError::invalid("A referenced workspace resource is missing."))
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
fn check_owner(row: &Header, owner: &Owner, fence: u64, now: u64) -> Result<()> {
    if row
        .claim
        .as_ref()
        .is_none_or(|c| &c.owner != owner || c.fence != fence || c.expires_at_ms <= now)
    {
        return Err(StoreError::conflict());
    }
    Ok(())
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
        connection.execute("INSERT INTO items(id,kind,name,name_key,metadata,content,working,metadata_generation,layout_generation,working_generation,deleted_at,builtin,fence,owner,epoch,lease_until) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)", params![
            write.id, metadata.kind.key(), metadata.name, name_key(&metadata.name), serde_json::to_string(&metadata)?, content, write.working_json,
            generations.metadata.to_string(), generations.layout.to_string(), generations.working.to_string(), metadata.deleted_at_ms.map(|v| v.to_string()), metadata.builtin,
            if write.claim { "1" } else { "0" }, write.claim.then_some(&owner.id), write.claim.then_some(&owner.epoch), write.claim.then(|| now.saturating_add(OWNER_LEASE_MS).to_string()),
        ])?;
        return Ok(generations);
    }
    let row = header(connection, &write.id)?;
    check_owner(&row, owner, write.fence, now)?;
    if row.builtin && row.kind != "workspace" {
        return Err(StoreError::invalid(
            "Load this included layout into a workspace to customize it.",
        ));
    }
    if (write.metadata.is_some() && write.expected.metadata != row.generations.metadata)
        || (write.content_json.is_some() && write.expected.layout != row.generations.layout)
        || (write.working_json.is_some() && write.expected.working != row.generations.working)
    {
        return Err(StoreError::conflict());
    }
    let mut generations = row.generations;
    if let Some(metadata) = &write.metadata {
        if metadata.kind.key() != row.kind || metadata.builtin != row.builtin {
            return Err(StoreError::invalid("Item type cannot change."));
        }
        if row.builtin && metadata.deleted_at_ms.is_some() {
            return Err(StoreError::invalid(
                "Included workspaces cannot be deleted.",
            ));
        }
        let metadata = resolve_name(connection, metadata.clone(), &write.id, write.name_policy)?;
        generations.metadata = advance(generations.metadata)?;
        connection.execute("UPDATE items SET name=?2,name_key=?3,metadata=?4,metadata_generation=?5,deleted_at=?6 WHERE id=?1", params![write.id, metadata.name, name_key(&metadata.name), serde_json::to_string(&metadata)?, generations.metadata.to_string(), metadata.deleted_at_ms.map(|v| v.to_string())])?;
        if metadata.deleted_at_ms.is_some() && !row.deleted {
            connection.execute(
                "UPDATE items SET fence=?2,owner=NULL,epoch=NULL,lease_until=NULL WHERE id=?1",
                params![write.id, advance(row.fence)?.to_string()],
            )?;
        }
    }
    if let Some(content) = &write.content_json {
        generations.layout = advance(generations.layout)?;
        connection.execute(
            "UPDATE items SET content=?2,layout_generation=?3 WHERE id=?1",
            params![write.id, content, generations.layout.to_string()],
        )?;
    }
    if let Some(working) = &write.working_json {
        if row.kind != "workspace" {
            return Err(StoreError::invalid(
                "Reusable items cannot store working values.",
            ));
        }
        generations.working = advance(generations.working)?;
        connection.execute(
            "UPDATE items SET working=?2,working_generation=?3 WHERE id=?1",
            params![write.id, working, generations.working.to_string()],
        )?;
    }
    Ok(generations)
}
fn resolve_name(
    connection: &Connection,
    mut metadata: Metadata,
    id: &str,
    policy: NamePolicy,
) -> Result<Metadata> {
    if metadata.deleted_at_ms.is_some() {
        return Ok(metadata);
    }
    let exists = |key: &str| -> Result<bool> {
        Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE kind=?1 AND name_key=?2 AND id!=?3 AND deleted_at IS NULL)", params![metadata.kind.key(),key,id], |r| r.get(0))?)
    };
    match policy {
        NamePolicy::Unique => metadata.name = available_name(&metadata.name, exists)?,
        NamePolicy::Exact => {
            if exists(&name_key(&metadata.name))? {
                return Err(StoreError::new(
                    ErrorKind::NameCollision,
                    "An item with this name already exists.",
                ));
            }
        }
    }
    Ok(metadata)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[path = "sqlite_maintenance.rs"]
mod maintenance;
