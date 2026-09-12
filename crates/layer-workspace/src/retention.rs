use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StorageReport {
    pub database_bytes: u64,
    pub component_bytes: u64,
    pub eligible_history_bytes: u64,
    pub versions_to_remove: usize,
    pub expired_items: usize,
    pub deferred_open_items: usize,
    pub oldest_history_ms: Option<u64>,
}
pub(crate) struct RetentionPlan {
    pub report: StorageReport,
    pub changed: Vec<Entity>,
    pub expired: Vec<String>,
}
#[derive(Clone)]
enum Version {
    Metadata(String),
    Layout(String),
    Reusable(String),
}
/// Shared retention policy. An owner must settle its pending writes before
/// allowing maintenance; other live owners are deferred, never overwritten.
pub(crate) fn retention_plan(
    items: &[StoredEntity],
    owner: Option<&Owner>,
    now: u64,
    budget: usize,
) -> Result<RetentionPlan, StoreError> {
    let mut report = StorageReport::default();
    let mut entities: Vec<_> = items.iter().map(|i| i.entity.clone()).collect();
    let mut eligible = Vec::new();
    let mut expired = Vec::new();
    for (index, stored) in items.iter().enumerate() {
        let entity = &stored.entity;
        let elsewhere = stored
            .claim
            .as_ref()
            .is_some_and(|c| c.expires_at_ms > now && Some(&c.owner) != owner);
        if elsewhere {
            report.deferred_open_items += 1;
        }
        if entity
            .metadata
            .deleted_at_ms
            .is_some_and(|t| now.saturating_sub(t) >= TRASH_LIFETIME_MS)
        {
            if !elsewhere {
                expired.push(entity.id.clone());
            }
            continue;
        }
        let oldest = match &entity.content {
            ItemContent::Workspace { history, .. } => {
                history.revisions.values().map(|r| r.timestamp_ms).min()
            }
            ItemContent::Reusable { current, previous } => std::iter::once(current)
                .chain(previous)
                .map(|r| r.timestamp_ms)
                .min(),
        };
        if let Some(oldest) = oldest {
            report.oldest_history_ms = Some(
                report
                    .oldest_history_ms
                    .map_or(oldest, |old| old.min(oldest)),
            );
        }
        let mut add = |version: Version, date: u64, bytes: usize| {
            report.oldest_history_ms =
                Some(report.oldest_history_ms.map_or(date, |old| old.min(date)));
            report.eligible_history_bytes += bytes as u64;
            if !elsewhere {
                eligible.push((date, index, version, bytes));
            }
        };
        for version in &entity.metadata.previous {
            add(
                Version::Metadata(version.id.clone()),
                version.timestamp_ms,
                serde_json::to_vec(version)?.len(),
            );
        }
        match &entity.content {
            ItemContent::Workspace { history, .. } => {
                for (id, revision) in &history.revisions {
                    if id != &history.current
                        && !history.undo.contains(id)
                        && !history.redo.contains(id)
                    {
                        add(
                            Version::Layout(id.clone()),
                            revision.timestamp_ms,
                            serde_json::to_vec(revision)?.len(),
                        );
                    }
                }
            }
            ItemContent::Reusable { previous, .. } => {
                for version in previous {
                    add(
                        Version::Reusable(version.id.clone()),
                        version.timestamp_ms,
                        serde_json::to_vec(version)?.len(),
                    );
                }
            }
        }
    }
    eligible.sort_by_key(|(date, index, _, _)| (*date, *index));
    let mut remaining = report.eligible_history_bytes;
    for (_, index, version, bytes) in eligible {
        if remaining <= budget as u64 {
            break;
        }
        match version {
            Version::Metadata(id) => entities[index].metadata.previous.retain(|v| v.id != id),
            Version::Layout(id) => {
                if let ItemContent::Workspace { history, .. } = &mut entities[index].content {
                    history.revisions.remove(&id);
                }
            }
            Version::Reusable(id) => {
                if let ItemContent::Reusable { previous, .. } = &mut entities[index].content {
                    previous.retain(|v| v.id != id);
                }
            }
        }
        remaining = remaining.saturating_sub(bytes as u64);
        report.versions_to_remove += 1;
    }
    report.expired_items = expired.len();
    let changed = entities
        .into_iter()
        .zip(items)
        .filter(|(e, s)| e != &s.entity && !expired.contains(&e.id))
        .map(|(e, _)| e)
        .collect();
    Ok(RetentionPlan {
        report,
        changed,
        expired,
    })
}

impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn storage_report(&self, clear_older: bool) -> Result<StorageReport, StoreError> {
        match self
            .store
            .execute(StoreRequest::Maintenance {
                owner: Some(self.owner.clone()),
                clear_older,
                apply: false,
            })
            .await?
        {
            StoreResponse::Storage(report) => Ok(report),
            _ => Err(StoreError::invalid("Unexpected storage usage reply.")),
        }
    }
    pub async fn maintain_storage(
        &self,
        clear_older: bool,
    ) -> Result<Option<StoredEntity>, StoreError> {
        self.flush().await?;
        self.store
            .execute(StoreRequest::Maintenance {
                owner: Some(self.owner.clone()),
                clear_older,
                apply: true,
            })
            .await?;
        self.refresh().await?;
        match self.active_id() {
            Some(id) => self.load(&id).await.map(Some),
            None => Ok(None),
        }
    }
    pub async fn delete_permanently(&self, id: &str) -> Result<(), StoreError> {
        let stored = self.claim(id).await?;
        let result = self
            .store
            .execute(StoreRequest::DeletePermanently {
                id: id.into(),
                owner: self.owner.clone(),
                fence: stored.claim.as_ref().unwrap().fence.to_string(),
            })
            .await;
        self.release(&stored).await;
        result?;
        self.refresh().await
    }
}
