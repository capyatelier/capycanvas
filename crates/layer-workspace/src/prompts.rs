//! Host-independent forms for workspace/library operations. Hosts own focus,
//! controls and file pickers; names, descriptions and source choices live here.
use crate::*;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ManagerChoice {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ManagerPrompt {
    pub title: String,
    pub message: String,
    pub confirm: String,
    pub destructive: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub choice_label: Option<String>,
    pub choices: Vec<ManagerChoice>,
    pub selected: Option<String>,
}
impl ManagerPrompt {
    pub fn confirm(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm: confirm.into(),
            destructive: false,
            name: None,
            description: None,
            choice_label: None,
            choices: Vec::new(),
            selected: None,
        }
    }
    fn choices(
        mut self,
        label: impl Into<String>,
        choices: Vec<ManagerChoice>,
        selected: Option<String>,
    ) -> Self {
        self.selected = selected.or_else(|| choices.first().map(|c| c.id.clone()));
        self.choice_label = Some(label.into());
        self.choices = choices;
        self
    }
}
pub fn recover_prompt(localization: &layer_ui::Localizer, changes: Vec<(String, String)>) -> Result<ManagerPrompt, StoreError> {
    let choices = changes
        .into_iter()
        .map(|(id, label)| ManagerChoice { id, label })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThereAreNoInterruptedChangesToRecover));
    }
    Ok(ManagerPrompt::confirm(
        localization.text(layer_ui::MessageId::WORKSPACE_RECOVER_INTERRUPTED_CHANGES).to_string(),
        localization.text(layer_ui::MessageId::WORKSPACE_RECOVER_CONFIRM).to_string(),
        localization.text(layer_ui::MessageId::WORKSPACE_RECOVER_COPIES).to_string(),
    )
    .choices(localization.text(layer_ui::MessageId::WORKSPACE_INTERRUPTED_CHANGES).to_string(), choices, None))
}
impl<S: WorkspaceStore> WorkspaceManager<S> {

    fn choices_for(&self, kind: ItemKind) -> Vec<ManagerChoice> {
        self.items()
            .into_iter()
            .filter(|i| i.metadata.kind == kind)
            .map(|i| ManagerChoice {
                id: i.id.clone(),
                label: self.summary_display_name(&i),
            })
            .collect()
    }
    fn current_toolbar_choices(&self) -> Result<Vec<ManagerChoice>, StoreError> {
        let current = self
            .current()
            .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoWorkspaceIsActive))?;
        Ok(current
            .capture()?
            .history
            .layout()
            .panels
            .iter()
            .filter(|p| p.id.kind() == layer_ui::PanelKind::Tiles)
            .map(|p| ManagerChoice {
                id: serde_json::to_string(&p.id).unwrap(),
                label: p.title_localized(&self.localization()),
            })
            .collect())
    }
    pub fn new_toolbar_prompt(&self) -> ManagerPrompt {
        let mut choices = vec![ManagerChoice {
            id: String::new(),
            label: self.localization().text(layer_ui::MessageId::WORKSPACE_EMPTY_TOOLBAR).to_string(),
        }];
        choices.extend(self.choices_for(ItemKind::Toolbar));
        let mut p = ManagerPrompt::confirm(
            self.localization().text(layer_ui::MessageId::WORKSPACE_NEW_TOOLBAR).to_string(),
            self.localization().text(layer_ui::MessageId::WORKSPACE_NEW_TOOLBAR_CONFIRM).to_string(),
            self.localization().text(layer_ui::MessageId::WORKSPACE_ADD_TO_WORKSPACE).to_string(),
        )
        .choices(self.localization().text(layer_ui::MessageId::WORKSPACE_START_WITH).to_string(), choices, None);
        p.name = Some(self.localization().text(layer_ui::MessageId::WORKSPACE_NEW_TOOLBAR).to_string());
        p
    }

    pub fn form_prompt(
        &self,
        action: &ManagerAction,
        source: Option<&Metadata>,
    ) -> Result<ManagerPrompt, StoreError> {
        use ManagerAction as A;
        let source = || source.ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAnItem));
        Ok(match action {
            A::New => {
                let mut p = ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_NEW_WORKSPACE).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_NEW_CONFIRM).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_CREATE_AND_SWITCH).to_string(),
                );
                p.name = Some(self.localization().text(layer_ui::MessageId::WORKSPACE_NEW_WORKSPACE).to_string());
                p
            }
            A::ResetBrushes => {
                let mut p = ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RESET_ALL_BRUSHES).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RESET_BRUSHES_CONFIRM).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RESET_BRUSHES).to_string(),
                );
                p.destructive = true;
                p
            }
            A::Rename(_) => {
                let s = source()?;
                let reusable = s.kind == ItemKind::Toolbar;
                let mut p = ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RENAME).to_string(),
                    if reusable {
                        self.localization().text(layer_ui::MessageId::WORKSPACE_RENAME_TOOLBAR_MESSAGE).to_string()
                    } else {
                        String::new()
                    },
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RENAME).to_string(),
                );
                p.name = Some(s.name.clone());
                p.description = reusable.then(|| s.description.clone());
                p
            }
            A::SaveToolbar(panel) => {
                let current = self
                    .current()
                    .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoWorkspaceIsActive))?;
                let capture = current.capture()?;
                let toolbar = capture
                    .history
                    .layout()
                    .panel(*panel)
                    .map_err(StoreError::invalid)?;
                ToolbarDefinition::capture(toolbar, &self.localization())?;
                let mut p = ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_SAVE_TO_TOOLBAR_LIBRARY).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_SAVE_TOOLBAR_CONFIRM).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_SAVE_TO_LIBRARY).to_string(),
                );
                p.name = Some(toolbar.title_localized(&self.localization()));
                p
            }
            A::Reset(id) => {
                let s = source()?;
                if s.kind != ItemKind::Workspace {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAWorkspace));
                }
                ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RESTORE_STARTING_LAYOUT).to_string(),
                    message(&self.localization(), if s.builtin && is_default_item(id) { layer_ui::MessageId::WORKSPACE_RESET_DEFAULT_CONFIRM } else { layer_ui::MessageId::WORKSPACE_RESET_SAVED_CONFIRM }, &[("menu", self.localization().text(layer_ui::MessageId::WORKSPACE_MENU).to_string())]),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_RESTORE).to_string(),
                )
            }
            A::Delete(id) => {
                let s = source()?;
                let mut p = ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_DELETE).to_string(),
                    message(&self.localization(), if self.active_id().as_deref() == Some(id) { layer_ui::MessageId::WORKSPACE_DELETE_ACTIVE_CONFIRM } else { layer_ui::MessageId::WORKSPACE_DELETE_CONFIRM }, &[("name", self.display_name(id, s))]),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_DELETE).to_string(),
                );
                p.destructive = true;
                p
            }
            A::ReplaceToolbar(_) => {
                let choices = self.choices_for(ItemKind::Toolbar);
                if choices.is_empty() {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::SaveAToolbarToTheLibraryFirst));
                }
                ManagerPrompt::confirm(self.localization().text(layer_ui::MessageId::WORKSPACE_REPLACE_FROM_LIBRARY).to_string(),self.localization().text(layer_ui::MessageId::WORKSPACE_REPLACE_TOOLBAR_CONFIRM).to_string(),self.localization().text(layer_ui::MessageId::WORKSPACE_REPLACE_TOOLBAR).to_string())
                    .choices(self.localization().text(layer_ui::MessageId::WORKSPACE_SAVED_TOOLBAR).to_string(),choices,None)
            }
            A::UpdateToolbar(_) => {
                let current = self
                    .current()
                    .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoWorkspaceIsActive))?;
                ManagerPrompt::confirm(self.localization().text(layer_ui::MessageId::WORKSPACE_UPDATE_SAVED_TOOLBAR).to_string(),message(&self.localization(), layer_ui::MessageId::WORKSPACE_UPDATE_TOOLBAR_CONFIRM, &[("workspace", self.display_name(&current.id, &current.metadata)), ("toolbar", source()?.name.clone())]),self.localization().text(layer_ui::MessageId::WORKSPACE_UPDATE).to_string())
                    .choices(self.localization().text(layer_ui::MessageId::WORKSPACE_TOOLBAR).to_string(),self.current_toolbar_choices()?,None)
            }
            A::NewToolbar(_) => self.new_toolbar_prompt(),
            A::SaveAsNew => {
                let mut p = ManagerPrompt::confirm(
                    self.localization().text(layer_ui::MessageId::WORKSPACE_SAVE_AS_NEW_WORKSPACE).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_SAVE_AS_NEW_CONFIRM).to_string(),
                    self.localization().text(layer_ui::MessageId::WORKSPACE_SAVE_AND_SWITCH).to_string(),
                );
                p.name = Some(self.localization().text(layer_ui::MessageId::WORKSPACE_RECOVERED_WORKSPACE).to_string());
                p
            }
            _ => {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisActionDoesNotUseAWorkspaceForm));
            }
        })
    }
}
