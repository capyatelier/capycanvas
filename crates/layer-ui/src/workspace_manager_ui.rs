//! Shared workspace command/menu vocabulary. The store coordinator owns records;
//! UiSession owns availability and routes commands to the connected host service.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceCommand {
    Manage,
    ManageTemplates,
    New,
    SaveAsTemplate,
    ResetLayout,
    LayoutHistory,
    Switch { id: String },
    ManageToolbars,
    SaveToolbar { panel: Panel },
    NewToolbar { group: Option<u32> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceChoice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug)]
pub struct ManagedWorkspace {
    pub id: String,
    pub name: String,
    pub baseline: DockLayout,
    pub choices: Vec<WorkspaceChoice>,
}
impl ManagedWorkspace {
    pub(crate) fn menu(&self, idle: bool, can_reset: bool) -> ContextMenuItem {
        let command = |label: &str, command: WorkspaceCommand, enabled: bool| {
            let mut item = ContextMenuItem::command(label, UiAction::WorkspaceManager { command });
            item.enabled = enabled;
            item
        };
        let mut choices = vec![WorkspaceChoice {
            id: self.id.clone(),
            name: self.name.clone(),
        }];
        choices.extend(
            self.choices
                .iter()
                .filter(|w| w.id != self.id)
                .take(5)
                .cloned(),
        );
        let choices = choices
            .into_iter()
            .map(|choice| {
                let selected = choice.id == self.id;
                let mut item = command(
                    &choice.name,
                    WorkspaceCommand::Switch { id: choice.id },
                    idle || selected,
                );
                item.selected = Some(selected);
                item
            })
            .collect();
        ContextMenuItem::submenu(
            "Workspaces",
            vec![
                choices,
                vec![
                    command("New Workspace…", WorkspaceCommand::New, idle),
                    command(
                        "Save Layout as Workspace Template…",
                        WorkspaceCommand::SaveAsTemplate,
                        idle,
                    ),
                    command(
                        "Reset Layout…",
                        WorkspaceCommand::ResetLayout,
                        idle && can_reset,
                    ),
                ],
                vec![
                    command("Layout History…", WorkspaceCommand::LayoutHistory, idle),
                    command("Manage Workspaces…", WorkspaceCommand::Manage, true),
                    command(
                        "Manage workspace templates…",
                        WorkspaceCommand::ManageTemplates,
                        true,
                    ),
                ],
            ],
        )
    }
}
