use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl SqliteStore {
    pub(super) fn maintenance(
        &mut self,
        owner: Option<&Owner>,
        clear_older: bool,
        apply: bool,
    ) -> Result<StorageReport> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = strings(&tx, "SELECT id FROM items")?;
        let mut items = Vec::new();
        for id in ids {
            if let Ok(item) = load(&tx, &id) {
                items.push(item);
            }
        }
        let mut plan = retention::retention_plan(
            &items,
            owner,
            now,
            if clear_older {
                0
            } else {
                HISTORY_BUDGET_BYTES as usize
            },
        )?;
        let pages: i64 = tx.pragma_query_value(None, "page_count", |r| r.get(0))?;
        let page_size: i64 = tx.pragma_query_value(None, "page_size", |r| r.get(0))?;
        plan.report.database_bytes = pages.saturating_mul(page_size) as u64;
        plan.report.component_bytes = tx.query_row(
            "SELECT coalesce(sum(length(bytes)),0) FROM components",
            [],
            |r| r.get::<_, i64>(0),
        )? as u64;
        if apply {
            for entity in plan.changed {
                let old = items.iter().find(|s| s.entity.id == entity.id).unwrap();
                if old.entity.metadata != entity.metadata {
                    tx.execute(
                        "UPDATE items SET metadata=?2,metadata_generation=?3 WHERE id=?1",
                        params![
                            entity.id,
                            serde_json::to_string(&entity.metadata)?,
                            advance(old.generations.metadata)?.to_string()
                        ],
                    )?;
                }
                if old.entity.content != entity.content {
                    let mut components = BTreeMap::new();
                    let packed =
                        protocol::pack(serde_json::to_value(entity.content)?, &mut components)?;
                    for (id, bytes) in components {
                        tx.execute(
                            "INSERT OR IGNORE INTO components(id,bytes) VALUES(?1,?2)",
                            params![id, bytes],
                        )?;
                    }
                    tx.execute(
                        "UPDATE items SET content=?2,layout_generation=?3 WHERE id=?1",
                        params![
                            entity.id,
                            serde_json::to_string(&packed)?,
                            advance(old.generations.layout)?.to_string()
                        ],
                    )?;
                }
            }
            for id in plan.expired {
                remove(&tx, &id)?;
            }
            // Once delivery is acknowledged and its owner session has expired,
            // retries can no longer be legitimate fenced writes.
            tx.execute("DELETE FROM receipts WHERE acknowledged=1 AND NOT EXISTS(SELECT 1 FROM pending WHERE pending.id=receipts.id) AND NOT EXISTS(SELECT 1 FROM items WHERE items.owner=receipts.owner AND items.epoch=receipts.epoch AND CAST(items.lease_until AS INTEGER)>?1)",[now as i64])?;
            collect_components(&tx)?;
        }
        tx.commit()?;
        if apply {
            self.connection
                .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        }
        Ok(plan.report)
    }
    pub(super) fn delete_permanently(&mut self, id: &str, owner: &Owner, fence: u64) -> Result<()> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = header(&tx, id)?;
        check_owner(&row, owner, fence, now)?;
        if !row.deleted || row.builtin {
            return Err(StoreError::invalid(
                "Only items in Recently Deleted can be permanently removed.",
            ));
        }
        remove(&tx, id)?;
        collect_components(&tx)?;
        tx.commit()?;
        Ok(())
    }
}
fn strings(connection: &Connection, sql: &str) -> Result<Vec<String>> {
    Ok(connection
        .prepare(sql)?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
fn remove(connection: &Connection, id: &str) -> Result<()> {
    let row = header(connection, id)?;
    connection.execute("INSERT INTO tombstones(id,fence) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET fence=excluded.fence",params![id,advance(row.fence)?.to_string()])?;
    connection.execute("DELETE FROM bindings WHERE item_id=?1", [id])?;
    connection.execute("DELETE FROM items WHERE id=?1", [id])?;
    Ok(())
}
fn collect_components(connection: &Connection) -> Result<()> {
    fn visit(
        value: &serde_json::Value,
        connection: &Connection,
        used: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<()> {
        if depth > 96 {
            return Err(StoreError::invalid(
                "Resource references exceed supported depth.",
            ));
        }
        match value {
            serde_json::Value::Object(object) => {
                if let Some(id) = object.get("$workspace_component").and_then(|v| v.as_str()) {
                    if used.insert(id.into()) {
                        let bytes = component(connection, id)?;
                        visit(
                            &serde_json::from_slice(&bytes)?,
                            connection,
                            used,
                            depth + 1,
                        )?;
                    }
                } else {
                    for value in object.values() {
                        visit(value, connection, used, depth + 1)?;
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value, connection, used, depth + 1)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut used = BTreeSet::new();
    // If a preserved newer/corrupt record cannot be decoded, defer collection.
    // Its dependencies must survive even while the item cannot be opened.
    for text in strings(connection, "SELECT content FROM items")? {
        let Ok(value) = serde_json::from_str(&text) else {
            return Ok(());
        };
        if visit(&value, connection, &mut used, 0).is_err() {
            return Ok(());
        }
    }
    for text in strings(connection, "SELECT payload FROM pending")? {
        let Ok(batch) = serde_json::from_str::<CommitBatch>(&text) else {
            return Ok(());
        };
        used.extend(batch.components.keys().cloned());
    }
    for id in strings(connection, "SELECT id FROM components")? {
        if !used.contains(&id) {
            connection.execute("DELETE FROM components WHERE id=?1", [id])?;
        }
    }
    Ok(())
}
