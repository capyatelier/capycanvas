use crate::*;
use layer_ui::{DockLayout, Panel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagerPage {
    Workspaces,
    Templates,
    ThisWorkspace,
    ToolbarLibrary,
    RecentlyDeleted,
}
impl ManagerPage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspaces => "Workspaces",
            Self::Templates => "Workspace Templates",
            Self::ThisWorkspace => "This Workspace",
            Self::ToolbarLibrary => "Saved Toolbars",
            Self::RecentlyDeleted => "Recently Deleted",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum ManagerAction {
    New,
    ImportTemplate,
    ImportToolbar,
    Storage,
    ExportCurrent,
    ExportDatabase,
    ImportBackup,
    ClearOlderHistory,
    SaveAsNew,
    RetryStorage,
    RecoverInterrupted,
    Switch(String),
    SwitchToWindow(String),
    NewFromTemplate(String),
    EditAsWorkspace(String),
    Rename(String),
    Duplicate(String),
    SaveAsTemplate(String),
    Reset(String),
    History(String),
    Metadata(String),
    UpdateFromCurrent(String),
    Export(String),
    Versions(String),
    Delete(String),
    RestoreDeleted(String),
    DeletePermanently(String),
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
            Self::ImportTemplate => "Import Workspace Template…",
            Self::ImportToolbar => "Import Toolbar…",
            Self::Storage => "Backups and Storage…",
            Self::ExportCurrent => "Save Backup…",
            Self::ExportDatabase => "Export All Stored Data…",
            Self::ImportBackup => "Restore from Backup…",
            Self::ClearOlderHistory => "Clear Older History…",
            Self::SaveAsNew => "Save as New Workspace…",
            Self::RetryStorage => "Retry Storage",
            Self::RecoverInterrupted => "Recover Interrupted Changes…",
            Self::Switch(_) => "Switch",
            Self::SwitchToWindow(_) => "Switch to Window",
            Self::NewFromTemplate(_) => "Use Layout…",
            Self::EditAsWorkspace(_) => "Edit as Workspace…",
            Self::Rename(_) | Self::RenameToolbar(_) => "Rename…",
            Self::Duplicate(_) | Self::DuplicateToolbar(_) => "Duplicate…",
            Self::SaveAsTemplate(_) => "Save Layout as Workspace Template…",
            Self::Reset(_) => "Reset Layout…",
            Self::History(_) => "Layout History…",
            Self::Metadata(_) => "Name and Description History…",
            Self::UpdateFromCurrent(_) => "Replace with Current Layout…",
            Self::Export(_) => "Export…",
            Self::Versions(_) => "Previous Versions…",
            Self::Delete(_) => "Delete…",
            Self::RestoreDeleted(_) => "Restore",
            Self::DeletePermanently(_) => "Delete Permanently…",
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
        matches!(
            self,
            Self::Delete(_) | Self::DeletePermanently(_) | Self::DeleteToolbar(_)
        )
    }
}
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
pub struct ManagerRow {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub builtin: bool,
}
#[derive(Clone, Debug)]
pub struct ManagerDetails {
    pub title: String,
    pub description: String,
    pub preview: Option<DockLayout>,
    pub actions: Vec<ManagerButton>,
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn inspect_details(
        &self,
        stored: &StoredEntity,
        idle: bool,
        now: u64,
    ) -> ManagerDetails {
        let mut details = self.details(stored, idle, now);
        if let ItemContent::Workspace {
            origin: Some(origin),
            ..
        } = &stored.entity.content
            && let Ok(template) = self.load(&origin.id).await
            && template.entity.metadata.deleted_at_ms.is_none()
            && let ItemContent::Reusable { current, .. } = &template.entity.content
            && current.id != origin.version
        {
            details.description.push_str("\nThis Workspace Template has been updated. Create a new workspace to try its latest layout.");
            let mut button = ManagerButton::new(
                ManagerAction::NewFromTemplate(origin.id.clone()),
                idle,
                false,
            );
            button.label = "New Workspace from Latest Workspace Template…".into();
            details.actions.push(button);
        }
        details
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
        items.sort_by_key(|i| (!i.metadata.builtin, name_key(&i.metadata.name)));
        items
            .into_iter()
            .filter(|i| {
                if page == ManagerPage::RecentlyDeleted {
                    i.metadata.deleted_at_ms.is_some()
                } else {
                    i.metadata.deleted_at_ms.is_none()
                        && i.metadata.kind
                            == match page {
                                ManagerPage::Workspaces => ItemKind::Workspace,
                                ManagerPage::Templates => ItemKind::Template,
                                _ => ItemKind::Toolbar,
                            }
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
                } else if let Some(deleted) = i.metadata.deleted_at_ms {
                    format!(
                        "{} · Deleted {} · Expires {}",
                        i.metadata.kind.label(),
                        date(deleted),
                        date(deleted.saturating_add(TRASH_LIFETIME_MS))
                    )
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
    pub fn details(&self, stored: &StoredEntity, idle: bool, now: u64) -> ManagerDetails {
        let entity = &stored.entity;
        let id = &entity.id;
        let current = self.active_id().as_ref() == Some(id);
        let elsewhere = stored
            .claim
            .as_ref()
            .is_some_and(|c| c.owner != self.owner && c.expires_at_ms > now);
        let available = !elsewhere && !entity.metadata.builtin;
        let preview = match &entity.content {
            ItemContent::Workspace { history, .. } => Some(history.layout().clone()),
            ItemContent::Reusable { current, .. } => match &current.content {
                ReusableContent::Layout { layout } => Some(layout.clone()),
                _ => None,
            },
        };
        let mut description = entity.metadata.description.clone();
        let mut actions = Vec::new();
        let mut add =
            |action, enabled, primary| actions.push(ManagerButton::new(action, enabled, primary));
        if entity.metadata.deleted_at_ms.is_some() {
            description.push_str("\nRestore this item to use it again.");
            add(ManagerAction::RestoreDeleted(id.clone()), !elsewhere, true);
            add(
                ManagerAction::DeletePermanently(id.clone()),
                !elsewhere,
                false,
            );
        } else {
            match &entity.content {
                ItemContent::Workspace { .. } => {
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
                    add(ManagerAction::Delete(id.clone()), available && idle, false);
                }
                ItemContent::Reusable {
                    current: revision, ..
                } => {
                    if entity.metadata.builtin {
                        description.push_str("\nIncluded with CapyCanvas.");
                    }
                    if revision.content.kind() == ItemKind::Template {
                        add(ManagerAction::NewFromTemplate(id.clone()), idle, true);
                        if available {
                            add(ManagerAction::UpdateFromCurrent(id.clone()), idle, false);
                        }
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
                    }
                    add(ManagerAction::Export(id.clone()), true, false);
                    if available {
                        add(ManagerAction::Versions(id.clone()), true, false);
                        add(ManagerAction::Delete(id.clone()), true, false);
                    }
                }
            }
        }
        if current && let Some(primary) = actions.first_mut() {
            primary.label = "Current workspace".into();
        }
        ManagerDetails {
            title: entity.metadata.name.clone(),
            description: description.trim().into(),
            preview,
            actions,
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
    let ItemContent::Workspace { origin, .. } = &entity.content else {
        return Err(StoreError::invalid("Choose a workspace."));
    };
    let target = origin.as_ref().map_or_else(
        || "starting layout".into(),
        |o| format!("original {} layout", o.name),
    );
    Ok(WorkspacePrompt {
        title: "Reset Layout".into(),
        message: format!(
            "Return {} to its {target}? You can undo this if you change your mind.",
            entity.metadata.name
        ),
        confirm: "Reset Layout",
    })
}
pub fn update_prompt(target: &Entity, source: &Entity) -> WorkspacePrompt {
    WorkspacePrompt {
        title: "Replace Saved Layout".into(),
        message: format!(
            "Replace the layout saved in “{}” with the layout you’re using in “{}”? Choose this Workspace Template next time to start with the updated layout.",
            target.metadata.name, source.metadata.name
        ),
        confirm: "Replace Layout",
    }
}
