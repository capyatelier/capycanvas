use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl SqliteStore {
    pub(super) fn maintenance(&mut self, owner: Option<&Owner>, clear_older: bool) -> Result<()> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, now)?;
        let items = strings(&tx, "SELECT id FROM items")?
            .iter()
            .map(|id| load(&tx, id))
            .collect::<Result<Vec<_>>>()?;
        let changed = retention::retention_plan(
            &items,
            owner,
            now,
            if clear_older {
                0
            } else {
                HISTORY_BUDGET_BYTES as usize
            },
        )?;
        for entity in changed {
            let old = items.iter().find(|s| s.entity.id == entity.id).unwrap();
            if old.entity.content != entity.content {
                let mut components = BTreeMap::new();
                let packed =
                    protocol::pack(serde_json::to_value(entity.content)?, &mut components)?;
                for (id, json) in components {
                    tx.execute(
                        "INSERT OR IGNORE INTO components(id,json) VALUES(?1,?2)",
                        params![id, json],
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
        // Once delivery is acknowledged and its owner session has expired,
        // retries can no longer be legitimate fenced writes.
        tx.execute("DELETE FROM receipts WHERE acknowledged=1 AND NOT EXISTS(SELECT 1 FROM pending WHERE pending.id=receipts.id) AND NOT EXISTS(SELECT 1 FROM items WHERE items.owner=receipts.owner AND items.epoch=receipts.epoch AND CAST(items.lease_until AS INTEGER)>?1)",[now as i64])?;
        collect_components(&tx)?;
        tx.commit()?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        Ok(())
    }
}
fn strings(connection: &Connection, sql: &str) -> Result<Vec<String>> {
    Ok(connection
        .prepare(sql)?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
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
                        let json = component(connection, id)?;
                        visit(&serde_json::from_str(&json)?, connection, used, depth + 1)?;
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
    for text in strings(connection, "SELECT content FROM items")? {
        visit(&serde_json::from_str(&text)?, connection, &mut used, 0)?;
    }
    for text in strings(connection, "SELECT payload FROM pending")? {
        let batch: CommitBatch = serde_json::from_str(&text)?;
        used.extend(batch.components.keys().cloned());
    }
    for id in strings(connection, "SELECT id FROM components")? {
        if !used.contains(&id) {
            connection.execute("DELETE FROM components WHERE id=?1", [id])?;
        }
    }
    Ok(())
}
