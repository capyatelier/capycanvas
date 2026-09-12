use crate::*;
use layer_ui::{DockLayout, Panel};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerPage {
    Workspaces,
    Templates,
    ThisWorkspace,
    ToolbarLibrary,
}
impl ManagerPage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspaces => "Workspaces",
            Self::Templates => "Saved Layouts",
            Self::ThisWorkspace => "This Workspace",
            Self::ToolbarLibrary => "Saved Toolbars",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ManagerAction {
    New,
    SaveAsNew,
    RetryStorage,
    RecoverInterrupted,
    Switch(String),
    SwitchToWindow(String),
    UseTemplate(String),
    EditAsWorkspace(String),
    Rename(String),
    Duplicate(String),
    SaveAsTemplate(String),
    Reset(String),
    ResetBrushes,
    History(String),
    UpdateFromCurrent(String),
    Delete(String),
    AddToolbar(String),
    UpdateToolbar(String),
    ShowToolbar(Panel, bool),
    RenameToolbar(Panel),
    DuplicateToolbar(Panel),
    SaveToolbar(Panel),
    ReplaceToolbar(Panel),
    DeleteToolbar(Panel),
}
impl ManagerAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::New => "New Workspace…",
            Self::SaveAsNew => "Save as New Workspace…",
            Self::RetryStorage => "Retry Storage",
            Self::RecoverInterrupted => "Recover Interrupted Changes…",
            Self::Switch(_) => "Switch",
            Self::SwitchToWindow(_) => "Switch to Window",
            Self::UseTemplate(_) => "Load Layout",
            Self::EditAsWorkspace(_) => "Edit as Workspace…",
            Self::Rename(_) | Self::RenameToolbar(_) => "Rename…",
            Self::Duplicate(_) | Self::DuplicateToolbar(_) => "Duplicate…",
            Self::SaveAsTemplate(_) => "Save Layout…",
            Self::ResetBrushes => "Reset All Brushes…",
            Self::Reset(_) => "Restore Starting Layout…",
            Self::History(_) => "Layout History…",
            Self::UpdateFromCurrent(_) => "Replace with Current Layout…",
            Self::Delete(_) => "Delete…",
            Self::AddToolbar(_) => "Add to Workspace",
            Self::UpdateToolbar(_) => "Update from Workspace…",
            Self::ShowToolbar(_, true) => "Show",
            Self::ShowToolbar(_, false) => "Hide",
            Self::SaveToolbar(_) => "Save to Toolbar Library…",
            Self::ReplaceToolbar(_) => "Replace from Library…",
            Self::DeleteToolbar(_) => "Delete Toolbar…",
        }
    }
    pub fn destructive(&self) -> bool {
        matches!(self, Self::Delete(_) | Self::DeleteToolbar(_))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ManagerButton {
    pub action: ManagerAction,
    pub label: String,
    pub enabled: bool,
    pub primary: bool,
}
impl ManagerButton {
    fn new(action: ManagerAction, enabled: bool, primary: bool) -> Self {
        Self {
            label: action.label().into(),
            action,
            enabled,
            primary,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ManagerRow {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub builtin: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ManagerDetails {
    pub title: String,
    pub description: String,
    pub preview: Option<DockLayout>,
    pub actions: Vec<ManagerButton>,
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub fn toolbar_details(&self, panel: Panel, idle: bool) -> Result<ManagerDetails, StoreError> {
        if panel.kind() != layer_ui::PanelKind::Tiles {
            return Err(StoreError::invalid("Choose a toolbar."));
        }
        let current = self
            .current()
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        let capture = current.capture()?;
        let config = capture
            .history
            .layout()
            .panel(panel)
            .map_err(StoreError::invalid)?;
        let visible = capture.history.layout().panel_group(panel).is_some();
        Ok(ManagerDetails {
            title: config.title().into(),
            description: format!(
                "Toolbar in {}. Changes are saved with this workspace and can be recovered in Layout History.",
                current.metadata.name
            ),
            preview: None,
            actions: [
                ManagerAction::ShowToolbar(panel, !visible),
                ManagerAction::RenameToolbar(panel),
                ManagerAction::DuplicateToolbar(panel),
                ManagerAction::SaveToolbar(panel),
                ManagerAction::ReplaceToolbar(panel),
                ManagerAction::DeleteToolbar(panel),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, action)| ManagerButton::new(action, idle, index == 0))
            .collect(),
        })
    }
    pub async fn inspect_details(
        &self,
        stored: &StoredEntity,
        idle: bool,
        now: u64,
    ) -> ManagerDetails {
        self.details(stored, idle, now)
    }

    pub fn rows(&self, page: ManagerPage, query: &str, now: u64) -> Vec<ManagerRow> {
        if page == ManagerPage::ThisWorkspace {
            let Some(entity) = self.current() else {
                return Vec::new();
            };
            let Ok(capture) = entity.capture() else {
                return Vec::new();
            };
            return capture
                .history
                .layout()
                .panels
                .iter()
                .filter(|p| p.id.kind() == layer_ui::PanelKind::Tiles)
                .filter(|p| name_key(p.title()).contains(&name_key(query)))
                .map(|p| ManagerRow {
                    id: serde_json::to_string(&p.id).unwrap(),
                    title: p.title().into(),
                    subtitle: if capture.history.layout().panel_group(p.id).is_some() {
                        "Visible"
                    } else {
                        "Hidden"
                    }
                    .into(),
                    builtin: false,
                })
                .collect();
        }
        let active = self.active_id();
        let mut items = self.items();
        let order = self.workspace_ids();
        items.sort_by_key(|i| {
            let rank = if page == ManagerPage::Workspaces {
                order
                    .iter()
                    .position(|id| id == &i.id)
                    .unwrap_or(usize::MAX)
            } else if i.metadata.builtin {
                0
            } else {
                1
            };
            (rank, name_key(&i.metadata.name))
        });
        items
            .into_iter()
            .filter(|i| {
                i.metadata.deleted_at_ms.is_none()
                    && i.metadata.kind
                        == match page {
                            ManagerPage::Workspaces => ItemKind::Workspace,
                            ManagerPage::Templates => ItemKind::Template,
                            _ => ItemKind::Toolbar,
                        }
            })
            .filter(|i| {
                format!("{} {}", i.metadata.name, i.metadata.description)
                    .to_lowercase()
                    .contains(&name_key(query))
            })
            .map(|i| {
                let subtitle = if let Some(error) = i.error {
                    format!("Unavailable: {error}")
                } else if active.as_ref() == Some(&i.id) {
                    "Current workspace".into()
                } else if i
                    .claim
                    .is_some_and(|c| c.owner != self.owner && c.expires_at_ms > now)
                {
                    "Open in another window".into()
                } else if i.metadata.builtin {
                    "Included with CapyCanvas".into()
                } else if i.metadata.kind == ItemKind::Workspace {
                    String::new()
                } else {
                    i.metadata.description.clone()
                };
                ManagerRow {
                    id: i.id,
                    title: i.metadata.name,
                    subtitle,
                    builtin: i.metadata.builtin,
                }
            })
            .collect()
    }
    /// Compact managers can show every row's actions without loading retained
    /// history or template contents. Details use the same availability policy.
    pub fn summary_actions(&self, item: &ItemSummary, idle: bool, now: u64) -> Vec<ManagerButton> {
        self.metadata_actions(&item.id, &item.metadata, item.claim.as_ref(), idle, now)
    }
    fn metadata_actions(
        &self,
        id: &String,
        metadata: &Metadata,
        claim: Option<&Claim>,
        idle: bool,
        now: u64,
    ) -> Vec<ManagerButton> {
        let current = self.active_id().as_ref() == Some(id);
        let elsewhere = claim.is_some_and(|c| c.owner != self.owner && c.expires_at_ms > now);
        let available = !elsewhere && !metadata.read_only();
        let mut actions = Vec::new();
        let mut add =
            |action, enabled, primary| actions.push(ManagerButton::new(action, enabled, primary));
        if metadata.deleted_at_ms.is_none() {
            match metadata.kind {
                ItemKind::Workspace => {
                    add(
                        if elsewhere {
                            ManagerAction::SwitchToWindow(id.clone())
                        } else {
                            ManagerAction::Switch(id.clone())
                        },
                        idle && !current,
                        true,
                    );
                    add(ManagerAction::Rename(id.clone()), available, false);
                    add(
                        ManagerAction::Delete(id.clone()),
                        available && idle && !metadata.builtin,
                        false,
                    );
                }
                ItemKind::Template | ItemKind::Toolbar => {
                    if metadata.kind == ItemKind::Template {
                        add(ManagerAction::UseTemplate(id.clone()), idle, true);
                    } else {
                        add(ManagerAction::AddToolbar(id.clone()), idle, true);
                        add(
                            ManagerAction::UpdateToolbar(id.clone()),
                            available && idle,
                            false,
                        );
                    }
                    if available {
                        add(ManagerAction::Rename(id.clone()), true, false);
                        add(ManagerAction::Delete(id.clone()), true, false);
                    }
                }
            }
        }
        if current && let Some(primary) = actions.first_mut() {
            primary.label = "Current workspace".into();
        }
        actions
    }
    pub fn details(&self, stored: &StoredEntity, idle: bool, now: u64) -> ManagerDetails {
        let entity = &stored.entity;
        let preview = match &entity.content {
            ItemContent::Workspace { history, .. } => Some(history.layout().clone()),
            ItemContent::Reusable { current, .. } => match &current.content {
                ReusableContent::Layout { layout } => Some(layout.clone()),
                _ => None,
            },
        };
        let mut description = entity.metadata.description.clone();
        if entity.metadata.builtin {
            description.push_str("\nIncluded with CapyCanvas.");
        }
        ManagerDetails {
            title: entity.metadata.name.clone(),
            description: description.trim().into(),
            preview,
            actions: self.metadata_actions(
                &entity.id,
                &entity.metadata,
                stored.claim.as_ref(),
                idle,
                now,
            ),
        }
    }
}
/// Calendar dates are presentation only, never conflict/ownership ordering.
pub fn date(timestamp_ms: u64) -> String {
    // Civil date from days since Unix epoch; supports native and Wasm without a
    // timezone library or host-dependent date arithmetic in manager policy.
    let z = (timestamp_ms / 86_400_000) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    format!("{:04}-{:02}-{:02}", y + i64::from(m <= 2), m, d)
}

pub struct WorkspacePrompt {
    pub title: String,
    pub message: String,
    pub confirm: &'static str,
}
pub fn reset_prompt(entity: &Entity) -> Result<WorkspacePrompt, StoreError> {
    let ItemContent::Workspace { .. } = &entity.content else {
        return Err(StoreError::invalid("Choose a workspace."));
    };
    Ok(WorkspacePrompt {
        title: "Restore Starting Layout".into(),
        message: format!("Restore “{}” to its starting layout?", entity.metadata.name),
        confirm: "Restore Starting Layout",
    })
}
pub fn update_prompt(target: &Entity, source: &Entity) -> WorkspacePrompt {
    WorkspacePrompt {
        title: "Replace Saved Layout".into(),
        message: format!(
            "Replace the saved layout in “{}” with the layout from “{}”?",
            target.metadata.name, source.metadata.name
        ),
        confirm: "Replace Layout",
    }
}
