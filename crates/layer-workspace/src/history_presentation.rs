use crate::*;
use layer_ui::DockLayout;
use serde::Serialize;

/// Newest first; revision ids break ties.
pub fn layout_history_versions(history: &layer_ui::LayoutHistory) -> Vec<layer_ui::LayoutRevision> {
    let mut versions = history.revisions.values().cloned().collect::<Vec<_>>();
    versions.sort_by(|a, b| {
        b.timestamp_ms.cmp(&a.timestamp_ms).then_with(|| {
            let n = |id: &str| {
                id.strip_prefix('r')
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            n(&b.id).cmp(&n(&a.id))
        })
    });
    versions
}

#[derive(Clone, Debug, Serialize)]
pub struct ManagerHistoryView {
    pub title: String,
    pub rows: Vec<ManagerRow>,
    pub selected: String,
    pub preview: Option<DockLayout>,
    pub restore: Option<ManagerPrompt>,
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn history_view(
        &self,
        id: &str,
        selected: Option<&str>,
        idle: bool,
        now: u64,
    ) -> Result<ManagerHistoryView, StoreError> {
        let entity = self.presentation_entity(id).await?;
        let stored = self.load(id).await?;
        let editable = !stored
            .claim
            .as_ref()
            .is_some_and(|c| c.owner != self.owner && c.expires_at_ms > now);
        let ItemContent::Workspace { history, .. } = &entity.content else {
            return Err(StoreError::invalid("Choose a workspace."));
        };
        let rows: Vec<_> = layout_history_versions(history)
            .into_iter()
            .map(|revision| ManagerRow {
                subtitle: if revision.id == history.current {
                    format!("Current layout · {}", date(revision.timestamp_ms))
                } else {
                    date(revision.timestamp_ms)
                },
                id: revision.id,
                title: revision.description,
                builtin: false,
            })
            .collect();
        let selected = selected
            .filter(|s| rows.iter().any(|r| r.id == *s))
            .map_or_else(|| history.current.clone(), str::to_owned);
        let restore = (editable && idle && selected != history.current).then(|| {
            ManagerPrompt::confirm(
                "Restore This Version",
                format!(
                    "Restore the selected version for {}? A new recovery point will retain the current state.",
                    entity.metadata.name
                ),
                "Restore This Version",
            )
        });
        Ok(ManagerHistoryView {
            title: format!("Layout History — {}", entity.metadata.name),
            rows,
            preview: history.revisions.get(&selected).map(|r| r.layout.clone()),
            selected,
            restore,
        })
    }
}
