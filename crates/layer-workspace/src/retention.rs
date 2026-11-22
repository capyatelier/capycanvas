use crate::stored_workspace::StoredWorkspace;
use crate::*;
use serde_json::value::RawValue;
use std::collections::BTreeSet;

pub(crate) struct Retained {
    pub id: String,
    pub claim: Option<Claim>,
    pub content: StoredWorkspace,
}

/// Shared retention policy. An owner must settle its pending writes before
/// allowing maintenance; other live owners are deferred, never overwritten.
pub(crate) fn retention_plan(
    items: &mut [Retained],
    owner: Option<&Owner>,
    now: u64,
    budget: u64,
    mut size: impl FnMut(&RawValue) -> u64,
) -> BTreeSet<usize> {
    #[derive(serde::Deserialize)]
    struct Dated {
        #[serde(with = "counter")]
        timestamp_ms: u64,
    }
    let mut eligible = Vec::new();
    let mut remaining = 0u64;
    for (index, item) in items.iter().enumerate() {
        let elsewhere = item
            .claim
            .as_ref()
            .is_some_and(|c| c.expires_at_ms > now && Some(&c.owner) != owner);
        for (id, revision) in item.content.unreachable() {
            let bytes = size(revision);
            remaining += bytes;
            if !elsewhere {
                let date = serde_json::from_str::<Dated>(revision.get()).map_or(0, |dated| dated.timestamp_ms);
                eligible.push((date, index, id.clone(), bytes));
            }
        }
    }
    eligible.sort_by_key(|(date, index, _, _)| (*date, *index));
    let mut changed = BTreeSet::new();
    for (_, index, id, bytes) in eligible {
        if remaining <= budget {
            break;
        }
        items[index].content.remove(&id);
        changed.insert(index);
        remaining = remaining.saturating_sub(bytes);
    }
    changed
}
