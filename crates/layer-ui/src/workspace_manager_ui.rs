//! Shared workspace command/menu vocabulary. The store coordinator owns records;
//! UiSession owns availability and routes commands to the connected host service.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceCommand {
    Manage,
    New,
    ResetBrushes,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedWorkspace {
    pub id: String,
    pub name: String,
    /// Restore target for this host: latest built-in default or saved custom baseline.
    pub baseline: DockLayout,
    pub choices: Vec<WorkspaceChoice>,
}
impl ManagedWorkspace {
    pub(crate) fn menu(&self, idle: bool, can_reset: bool, localizer: &Localizer) -> ContextMenuItem {
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
            &localizer.text(MessageId::WORKSPACE_WORKSPACES),
            vec![
                choices,
                vec![
                    command(&localizer.text(MessageId::WORKSPACE_ACTION_NEW_WORKSPACE), WorkspaceCommand::New, idle),
                    command(&localizer.text(MessageId::WORKSPACE_WORKSPACES), WorkspaceCommand::Manage, true),
                ],
                vec![
                    command(&localizer.text(MessageId::WORKSPACE_ACTION_LAYOUT_HISTORY), WorkspaceCommand::LayoutHistory, idle),
                    command(
                        &localizer.text(MessageId::WORKSPACE_ACTION_RESTORE_STARTING_LAYOUT),
                        WorkspaceCommand::ResetLayout,
                        idle && can_reset,
                    ),
                ],
                vec![command(
                    &localizer.text(MessageId::WORKSPACE_ACTION_RESET_ALL_BRUSHES),
                    WorkspaceCommand::ResetBrushes,
                    idle,
                )],
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_menu_localizes_actions_and_preserves_literal_choice_names() {
        let en = Localizer::shared(UiLanguage::English);
        let ja = Localizer::shared(UiLanguage::Japanese);
        let workspace = ManagedWorkspace {
            id: "current".into(), name: "Workspaces {$name}".into(), baseline: DockLayout::default(),
            choices: vec![WorkspaceChoice { id: "other".into(), name: "New Workspace… 日本語".into() }],
        };
        let english = workspace.menu(false, true, &en);
        let japanese = workspace.menu(false, true, &ja);
        assert_eq!(japanese.label, ja.text(MessageId::WORKSPACE_WORKSPACES).as_ref());
        assert_ne!(english.label, japanese.label);
        assert_eq!(japanese.sections[0][0].label, workspace.name);
        assert_eq!(japanese.sections[0][1].label, workspace.choices[0].name);
        for (english, japanese) in english.sections.iter().flatten().zip(japanese.sections.iter().flatten()) {
            assert_eq!(english.action, japanese.action);
            assert_eq!(english.selected, japanese.selected);
            assert_eq!(english.enabled, japanese.enabled);
        }
        let actions = [
            (1, 0, MessageId::WORKSPACE_ACTION_NEW_WORKSPACE, WorkspaceCommand::New),
            (1, 1, MessageId::WORKSPACE_WORKSPACES, WorkspaceCommand::Manage),
            (2, 0, MessageId::WORKSPACE_ACTION_LAYOUT_HISTORY, WorkspaceCommand::LayoutHistory),
            (2, 1, MessageId::WORKSPACE_ACTION_RESTORE_STARTING_LAYOUT, WorkspaceCommand::ResetLayout),
            (3, 0, MessageId::WORKSPACE_ACTION_RESET_ALL_BRUSHES, WorkspaceCommand::ResetBrushes),
        ];
        for (section, row, label, command) in actions {
            let item = &japanese.sections[section][row];
            assert_eq!(item.label, ja.text(label).as_ref());
            assert_eq!(item.action, Some(UiAction::WorkspaceManager { command }));
        }
    }
}
