use crate::*;

/// Shared retention policy. An owner must settle its pending writes before
/// allowing maintenance; other live owners are deferred, never overwritten.
pub(crate) fn retention_plan(
    items: &[StoredEntity],
    owner: Option<&Owner>,
    now: u64,
    budget: usize,
) -> Result<Vec<Entity>, StoreError> {
    let mut entities: Vec<_> = items.iter().map(|i| i.entity.clone()).collect();
    let mut eligible = Vec::new();
    let mut remaining = 0u64;
    for (index, stored) in items.iter().enumerate() {
        let elsewhere = stored
            .claim
            .as_ref()
            .is_some_and(|c| c.expires_at_ms > now && Some(&c.owner) != owner);
        let ItemContent::Workspace { history, .. } = &stored.entity.content else {
            continue;
        };
        for (id, revision) in &history.revisions {
            if id != &history.current && !history.undo.contains(id) && !history.redo.contains(id) {
                let bytes = serde_json::to_vec(revision)?.len() as u64;
                remaining += bytes;
                if !elsewhere {
                    eligible.push((revision.timestamp_ms, index, id.clone(), bytes));
                }
            }
        }
    }
    eligible.sort_by_key(|(date, index, _, _)| (*date, *index));
    for (_, index, id, bytes) in eligible {
        if remaining <= budget as u64 {
            break;
        }
        if let ItemContent::Workspace { history, .. } = &mut entities[index].content {
            history.revisions.remove(&id);
        }
        remaining = remaining.saturating_sub(bytes);
    }
    Ok(entities
        .into_iter()
        .zip(items)
        .filter(|(e, s)| e != &s.entity)
        .map(|(e, _)| e)
        .collect())
}
