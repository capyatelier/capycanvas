use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl SqliteStore {
    pub(super) fn maintenance(&mut self, owner: Option<&Owner>, clear_older: bool) -> Result<()> {
        let now = self.clock.now_ms();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.ownership.reconcile(&tx, now)?;
        let mut items = Vec::new();
        for id in strings(&tx, "SELECT id FROM items WHERE kind='workspace'")? {
            match retained(&tx, id) {
                Err(error) if error.kind == ErrorKind::InvalidData => {}
                item => items.extend(item?),
            }
        }
        let mut lengths = BTreeMap::new();
        let changed = retention::retention_plan(
            &mut items,
            owner,
            now,
            if clear_older { 0 } else { HISTORY_BUDGET_BYTES },
            |revision| stored_workspace::resolved_len(revision.get(), &mut |id| component(&tx, id).ok(), &mut lengths),
        );
        for item in changed.into_iter().map(|index| &items[index]) {
            tx.execute(
                "UPDATE items SET content=?2,layout_generation=?3 WHERE id=?1",
                params![
                    item.id,
                    item.content.encode()?,
                    advance(header(&tx, &item.id)?.generations.layout)?.to_string()
                ],
            )?;
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
fn retained(connection: &Connection, id: String) -> Result<Option<retention::Retained>> {
    let row = header(connection, &id)?;
    serde_json::from_str::<Metadata>(&row.metadata)?.validate()?;
    let content: String = connection.query_row("SELECT content FROM items WHERE id=?1", [&id], |r| r.get(0))?;
    Ok(StoredWorkspace::parse(&content).ok().map(|content| retention::Retained { id, claim: row.claim, content }))
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
    for (sql, pending) in [("SELECT content FROM items", false), ("SELECT payload FROM pending", true)] {
        for text in strings(connection, sql)? {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return Ok(()); };
            if pending && let Some(components) = value["components"].as_object() {
                used.extend(components.keys().cloned());
            }
            match visit(&value, connection, &mut used, 0) {
                Err(error) if error.kind == ErrorKind::InvalidData => return Ok(()),
                result => result?,
            }
        }
    }
    for id in strings(connection, "SELECT id FROM components")? {
        if !used.contains(&id) {
            connection.execute("DELETE FROM components WHERE id=?1", [id])?;
        }
    }
    Ok(())
}
