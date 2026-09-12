use crate::*;
use layer_ui::DockLayout;
use serde::{Deserialize, Serialize};

/// Display legacy history using current captions without rewriting saved revisions.
pub fn layout_history_versions(history: &layer_ui::LayoutHistory) -> Vec<layer_ui::LayoutRevision> {
    let mut versions = history.revisions.values().cloned().collect::<Vec<_>>();
    // Older releases saved generic labels. Recover names where the retained
    // undo/redo chain establishes the predecessor; never guess a branch's parent.
    let chain: Vec<_> = history
        .undo
        .iter()
        .chain(std::iter::once(&history.current))
        .chain(history.redo.iter().rev())
        .collect();
    for version in &mut versions {
        // Keep old saved descriptions readable in the current vocabulary.
        if let Some(name) = version
            .description
            .strip_prefix("Applied ")
            .and_then(|name| name.strip_suffix(" Workspace Template"))
        {
            version.description = format!("Loaded “{name}” layout");
        } else if version.description == "Reset to starting layout" {
            version.description = "Restored starting layout".into();
        }
        if version.description == "Starting configuration" {
            version.description = "Starting layout".into();
        }
        if version.description == "Arrange panels and toolbars" {
            version.description = chain
                .windows(2)
                .find(|pair| pair[1] == &version.id)
                .map(|pair| {
                    layer_ui::layout_change_description(
                        &history.revisions[pair[0]].layout,
                        &version.layout,
                    )
                })
                .unwrap_or_else(|| "Earlier layout".into());
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerHistoryMode {
    Layout,
    Versions,
    Metadata,
}
#[derive(Clone, Debug, Serialize)]
pub struct ManagerHistoryView {
    pub title: String,
    pub rows: Vec<ManagerRow>,
    pub selected: Option<String>,
    pub description: String,
    pub preview: Option<DockLayout>,
    pub restore: Option<ManagerPrompt>,
    pub open: Option<ManagerPrompt>,
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn history_view(
        &self,
        id: &str,
        mode: ManagerHistoryMode,
        selected: Option<&str>,
        idle: bool,
        now: u64,
    ) -> Result<ManagerHistoryView, StoreError> {
        let entity = self.presentation_entity(id).await?;
        let stored = self.load(id).await?;
        let editable = !entity.metadata.builtin
            && entity.metadata.deleted_at_ms.is_none()
            && !stored
                .claim
                .as_ref()
                .is_some_and(|c| c.owner != self.owner && c.expires_at_ms > now);
        let mut view = ManagerHistoryView {
            title: format!(
                "{} — {}",
                match mode {
                    ManagerHistoryMode::Layout => "Layout History",
                    ManagerHistoryMode::Versions => "Previous Versions",
                    ManagerHistoryMode::Metadata => "Name and Description History",
                },
                entity.metadata.name
            ),
            rows: Vec::new(),
            selected: None,
            description: String::new(),
            preview: None,
            restore: None,
            open: None,
        };
        let mut entries = Vec::new();
        match mode {
            ManagerHistoryMode::Layout => {
                let ItemContent::Workspace { history, .. } = &entity.content else {
                    return Err(StoreError::invalid("Choose a workspace."));
                };
                for (id, revision) in &history.revisions {
                    entries.push((
                        revision.timestamp_ms,
                        ManagerRow {
                            id: id.clone(),
                            title: revision.description.clone(),
                            subtitle: format!(
                                "{}{}",
                                date(revision.timestamp_ms),
                                if id == &history.current {
                                    " · Current"
                                } else {
                                    ""
                                }
                            ),
                            builtin: false,
                        },
                    ));
                }
            }
            ManagerHistoryMode::Versions => {
                let ItemContent::Reusable { current, previous } = &entity.content else {
                    return Err(StoreError::invalid("Choose a template or saved toolbar."));
                };
                for revision in std::iter::once(current).chain(previous) {
                    entries.push((
                        revision.timestamp_ms,
                        ManagerRow {
                            id: revision.id.clone(),
                            title: revision.name.clone(),
                            subtitle: format!(
                                "{}{}",
                                date(revision.timestamp_ms),
                                if revision.id == current.id {
                                    " · Current"
                                } else {
                                    ""
                                }
                            ),
                            builtin: false,
                        },
                    ));
                }
            }
            ManagerHistoryMode::Metadata => {
                for revision in &entity.metadata.previous {
                    entries.push((
                        revision.timestamp_ms,
                        ManagerRow {
                            id: revision.id.clone(),
                            title: revision.name.clone(),
                            subtitle: date(revision.timestamp_ms),
                            builtin: false,
                        },
                    ));
                }
            }
        }
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        view.rows = entries.into_iter().map(|(_, row)| row).collect();
        view.selected = selected
            .filter(|s| view.rows.iter().any(|r| r.id == *s))
            .map(str::to_owned)
            .or_else(|| match (&entity.content, mode) {
                (ItemContent::Workspace { history, .. }, ManagerHistoryMode::Layout) => {
                    Some(history.current.clone())
                }
                _ => view.rows.first().map(|r| r.id.clone()),
            });
        let Some(selected) = view.selected.as_deref() else {
            return Ok(view);
        };
        let mut can_restore = editable;
        let can_open = match mode {
            ManagerHistoryMode::Layout => {
                let ItemContent::Workspace { history, .. } = &entity.content else {
                    unreachable!()
                };
                let revision = &history.revisions[selected];
                view.preview = Some(revision.layout.clone());
                view.description = revision.description.clone();
                can_restore &= idle && selected != history.current;
                idle
            }
            ManagerHistoryMode::Versions => {
                let ItemContent::Reusable { current, previous } = &entity.content else {
                    unreachable!()
                };
                let revision = std::iter::once(current)
                    .chain(previous)
                    .find(|r| r.id == selected)
                    .unwrap();
                view.description = revision.description.clone();
                can_restore &= selected != current.id;
                if let ReusableContent::Layout { layout } = &revision.content {
                    view.preview = Some(layout.clone());
                    idle
                } else {
                    false
                }
            }
            ManagerHistoryMode::Metadata => {
                let revision = entity
                    .metadata
                    .previous
                    .iter()
                    .find(|r| r.id == selected)
                    .unwrap();
                view.description = revision.description.clone();
                false
            }
        };
        if can_restore {
            view.restore = Some(ManagerPrompt::confirm(
                "Restore This Version",
                format!(
                    "Restore the selected {} for {}? A new recovery point will retain the current state.",
                    if mode == ManagerHistoryMode::Metadata {
                        "name and description"
                    } else {
                        "version"
                    },
                    entity.metadata.name
                ),
                "Restore This Version",
            ));
        }
        if can_open {
            let mut prompt = ManagerPrompt::confirm(
                "Open as New Workspace",
                "Create an independent workspace from this retained layout. The source and its newer history remain available.",
                "Create and Switch",
            );
            prompt.name = Some(format!("{} Copy", entity.metadata.name));
            view.open = Some(prompt);
        }
        Ok(view)
    }
}
