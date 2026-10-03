use crate::*;
use layer_ui::{DockLayout, Panel};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerPage {
    Workspaces,
    History,
    ThisWorkspace,
    ToolbarLibrary,
}
impl ManagerPage {
    pub fn label(self, localization: &layer_ui::Localizer) -> String {
        match self {
            Self::Workspaces => localization.text(layer_ui::MessageId::WORKSPACE_WORKSPACES).to_string(),
            Self::History => localization.text(layer_ui::MessageId::WORKSPACE_LAYOUT_HISTORY).to_string(),
            Self::ThisWorkspace => localization.text(layer_ui::MessageId::WORKSPACE_THIS_WORKSPACE).to_string(),
            Self::ToolbarLibrary => localization.text(layer_ui::MessageId::WORKSPACE_SAVED_TOOLBARS).to_string(),
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
    Rename(String),
    Reset(String),
    ResetBrushes,
    History(String),
    Delete(String),
    AddToolbar(String),
    UpdateToolbar(String),
    NewToolbar(Option<u32>),
    ShowToolbar(Panel, bool),
    RenameToolbar(Panel),
    DuplicateToolbar(Panel),
    SaveToolbar(Panel),
    ReplaceToolbar(Panel),
    DeleteToolbar(Panel),
}
impl ManagerAction {
    pub fn label(&self, localization: &layer_ui::Localizer) -> String {
        match self {
            Self::New => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_NEW_WORKSPACE).to_string(),
            Self::SaveAsNew => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_SAVE_AS_NEW_WORKSPACE).to_string(),
            Self::RetryStorage => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_RETRY_STORAGE).to_string(),
            Self::RecoverInterrupted => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_RECOVER_INTERRUPTED_CHANGES).to_string(),
            Self::Switch(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_SWITCH).to_string(),
            Self::SwitchToWindow(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_SWITCH_TO_WINDOW).to_string(),
            Self::Rename(_) | Self::RenameToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_RENAME).to_string(),
            Self::DuplicateToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_DUPLICATE).to_string(),
            Self::ResetBrushes => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_RESET_ALL_BRUSHES).to_string(),
            Self::Reset(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_RESTORE_STARTING_LAYOUT).to_string(),
            Self::History(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_LAYOUT_HISTORY).to_string(),
            Self::Delete(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_DELETE).to_string(),
            Self::AddToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_ADD_TO_WORKSPACE).to_string(),
            Self::UpdateToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_UPDATE_FROM_WORKSPACE).to_string(),
            Self::NewToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_NEW_TOOLBAR).to_string(),
            Self::ShowToolbar(_, true) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_SHOW).to_string(),
            Self::ShowToolbar(_, false) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_HIDE).to_string(),
            Self::SaveToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_SAVE_TO_TOOLBAR_LIBRARY).to_string(),
            Self::ReplaceToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_REPLACE_FROM_LIBRARY).to_string(),
            Self::DeleteToolbar(_) => localization.text(layer_ui::MessageId::WORKSPACE_ACTION_DELETE_TOOLBAR).to_string(),
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
    fn new(localization: &layer_ui::Localizer, action: ManagerAction, enabled: bool, primary: bool) -> Self {
        Self {
            label: action.label(localization),
            action,
            enabled,
            primary,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceRow {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub current: bool,
    pub actions: Vec<ManagerButton>,
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
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAToolbar));
        }
        let current = self
            .current()
            .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoWorkspaceIsActive))?;
        let capture = current.capture()?;
        let config = capture
            .history
            .layout()
            .panel(panel)
            .map_err(StoreError::invalid)?;
        let visible = capture.history.layout().panel_group(panel).is_some();
        Ok(ManagerDetails {
            title: config.title_localized(&self.localization()),
            description: message(&self.localization(), layer_ui::MessageId::WORKSPACE_TOOLBAR_IN, &[("name", self.display_name(&current.id, &current.metadata))]),
            preview: None,
            actions: toolbar_actions(&self.localization(), panel, visible, idle),
        })
    }
    pub fn rows(&self, page: ManagerPage, query: &str, now: u64) -> Vec<WorkspaceRow> {
        if page == ManagerPage::History {
            return Vec::new();
        }
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
                .filter(|p| layer_ui::normalize_search(&p.title_localized(&self.localization())).contains(&layer_ui::normalize_search(query)))
                .map(|p| WorkspaceRow {
                    id: serde_json::to_string(&p.id).unwrap(),
                    title: p.title_localized(&self.localization()),
                    subtitle: if capture.history.layout().panel_group(p.id).is_some() {
                        self.localization().text(layer_ui::MessageId::WORKSPACE_VISIBLE).to_string()
                    } else {
                        self.localization().text(layer_ui::MessageId::WORKSPACE_HIDDEN).to_string()
                    }
                    .into(),
                    current: false,
                    actions: Vec::new(),
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
                i.metadata.kind
                    == match page {
                        ManagerPage::Workspaces => ItemKind::Workspace,
                        _ => ItemKind::Toolbar,
                    }
            })
            .filter(|i| {
                layer_ui::normalize_search(&format!("{} {} {}", self.summary_display_name(&i), i.metadata.name, i.metadata.description))
                    .contains(&layer_ui::normalize_search(query))
            })
            .map(|i| {
                let subtitle = if let Some(error) = &i.error {
                    message(&self.localization(), layer_ui::MessageId::WORKSPACE_UNAVAILABLE, &[("error", error.localized_message(&self.localization()))])
                } else if active.as_ref() == Some(&i.id) {
                    self.localization().text(layer_ui::MessageId::WORKSPACE_CURRENT_WORKSPACE).to_string()
                } else if i
                    .claim
                    .as_ref().is_some_and(|c| c.owner != self.owner && c.expires_at_ms > now)
                {
                    self.localization().text(layer_ui::MessageId::WORKSPACE_OPEN_IN_ANOTHER_WINDOW).to_string()
                } else if i.metadata.builtin {
                    self.localization().text(layer_ui::MessageId::WORKSPACE_INCLUDED_WITH_CAPYCANVAS).to_string()
                } else if i.metadata.kind == ItemKind::Workspace {
                    String::new()
                } else {
                    i.metadata.description.clone()
                };
                WorkspaceRow {
                    id: i.id.clone(),
                    title: self.summary_display_name(&i),
                    subtitle,
                    current: false,
                    actions: Vec::new(),
                }
            })
            .collect()
    }
    /// Compact managers can show every row's actions without loading retained
    /// history or toolbar contents. Details use the same availability policy.
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
        let available = !elsewhere;
        let mut actions = Vec::new();
        let mut add =
            |action, enabled, primary| actions.push(ManagerButton::new(&self.localization(), action, enabled, primary));
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
                if !metadata.builtin {
                    add(ManagerAction::Rename(id.clone()), available, false);
                }
                add(
                    ManagerAction::Delete(id.clone()),
                    available && idle && !metadata.builtin,
                    false,
                );
            }
            ItemKind::Toolbar => {
                add(ManagerAction::AddToolbar(id.clone()), idle, true);
                add(
                    ManagerAction::UpdateToolbar(id.clone()),
                    available && idle,
                    false,
                );
                if available {
                    add(ManagerAction::Rename(id.clone()), true, false);
                    add(ManagerAction::Delete(id.clone()), true, false);
                }
            }
        }
        if current && let Some(primary) = actions.first_mut() {
            primary.label = self.localization().text(layer_ui::MessageId::WORKSPACE_CURRENT_WORKSPACE).to_string();
        }
        actions
    }
    pub fn details(&self, stored: &StoredEntity, idle: bool, now: u64) -> ManagerDetails {
        let entity = &stored.entity;
        let preview = match &entity.content {
            ItemContent::Workspace { history, .. } => Some(history.layout().clone()),
            ItemContent::Toolbar { .. } => None,
        };
        let mut description = entity.metadata.description.clone();
        if entity.metadata.builtin {
            description = message(&self.localization(), layer_ui::MessageId::WORKSPACE_INCLUDED_DESCRIPTION, &[("description", description)]);
        }
        ManagerDetails {
            title: self.display_name(&entity.id, &entity.metadata),
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
pub fn toolbar_actions(localization: &layer_ui::Localizer, panel: Panel, visible: bool, idle: bool) -> Vec<ManagerButton> {
    [
        ManagerAction::ShowToolbar(panel, !visible),
        ManagerAction::RenameToolbar(panel),
        ManagerAction::DuplicateToolbar(panel),
        ManagerAction::SaveToolbar(panel),
        ManagerAction::ReplaceToolbar(panel),
        ManagerAction::DeleteToolbar(panel),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, action)| ManagerButton::new(localization, action, idle, index == 0))
    .collect()
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

pub(crate) fn message(localization: &layer_ui::Localizer, id: layer_ui::MessageId, values: &[(&str, String)]) -> String {
    let mut args = layer_ui::FluentArgs::new();
    for (name, value) in values { args.set(*name, value.as_str()); }
    localization.format(id, &args)
}

pub(crate) fn name_list(localization: &layer_ui::Localizer, names: &[String]) -> String {
    let mut args = layer_ui::FluentArgs::new();
    for (key, name) in ["first", "second", "third"].into_iter().zip(names) { args.set(key, name.as_str()); }
    args.set("others", names.len().saturating_sub(3));
    localization.format(match names.len() {
        1 => layer_ui::MessageId::WORKSPACE_HISTORY_LIST_ONE,
        2 => layer_ui::MessageId::WORKSPACE_HISTORY_LIST_TWO,
        3 => layer_ui::MessageId::WORKSPACE_HISTORY_LIST_THREE,
        _ => layer_ui::MessageId::WORKSPACE_HISTORY_LIST_MORE,
    }, &args)
}
