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
        label: &str,
        choices: Vec<ManagerChoice>,
        selected: Option<String>,
    ) -> Self {
        self.selected = selected.or_else(|| choices.first().map(|c| c.id.clone()));
        self.choice_label = Some(label.into());
        self.choices = choices;
        self
    }
}
pub fn recover_prompt(changes: Vec<(String, String)>) -> Result<ManagerPrompt, StoreError> {
    let choices = changes
        .into_iter()
        .map(|(id, label)| ManagerChoice { id, label })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return Err(StoreError::invalid(
            "There are no interrupted changes to recover.",
        ));
    }
    Ok(ManagerPrompt::confirm(
        "Recover Interrupted Changes",
        "Recover the selected changes into independent copies with unique names. Existing items stay as they are.",
        "Recover Copies",
    )
    .choices("Interrupted changes", choices, None))
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn presentation_entity(&self, id: &str) -> Result<Entity, StoreError> {
        if self.active_id().as_deref() == Some(id) {
            self.current()
                .ok_or_else(|| StoreError::invalid("No workspace is active."))
        } else {
            self.load(id).await.map(|s| s.entity)
        }
    }
    fn choices_for(&self, kind: ItemKind) -> Vec<ManagerChoice> {
        self.items()
            .into_iter()
            .filter(|i| i.metadata.kind == kind)
            .map(|i| ManagerChoice {
                id: i.id,
                label: i.metadata.name,
            })
            .collect()
    }
    fn current_toolbar_choices(&self) -> Result<Vec<ManagerChoice>, StoreError> {
        let current = self
            .current()
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        Ok(current
            .capture()?
            .history
            .layout()
            .panels
            .iter()
            .filter(|p| p.id.kind() == layer_ui::PanelKind::Tiles)
            .map(|p| ManagerChoice {
                id: serde_json::to_string(&p.id).unwrap(),
                label: p.title().into(),
            })
            .collect())
    }
    pub fn new_toolbar_prompt(&self) -> ManagerPrompt {
        let mut choices = vec![ManagerChoice {
            id: String::new(),
            label: "Empty toolbar".into(),
        }];
        choices.extend(self.choices_for(ItemKind::Toolbar));
        let mut p = ManagerPrompt::confirm(
            "New Toolbar",
            "Start with an empty toolbar or an independent copy from the Toolbar Library.",
            "Add to Workspace",
        )
        .choices("Start with", choices, None);
        p.name = Some("New Toolbar".into());
        p
    }
    pub async fn prompt(
        &self,
        action: &ManagerAction,
        now: u64,
    ) -> Result<ManagerPrompt, StoreError> {
        use ManagerAction as A;
        if matches!(action, A::RecoverInterrupted) {
            return recover_prompt(self.interrupted_changes(now).await?);
        }
        let source = match action {
            A::Rename(id) | A::Reset(id) | A::Delete(id) | A::UpdateToolbar(id) => {
                Some(self.presentation_entity(id).await?.metadata)
            }
            _ => None,
        };
        self.form_prompt(action, source.as_ref())
    }
    pub fn form_prompt(
        &self,
        action: &ManagerAction,
        source: Option<&Metadata>,
    ) -> Result<ManagerPrompt, StoreError> {
        use ManagerAction as A;
        let source = || source.ok_or_else(|| StoreError::invalid("Choose an item."));
        Ok(match action {
            A::New => {
                let mut p = ManagerPrompt::confirm(
                    "New Workspace",
                    "Copy your current tool settings and layout into a new workspace.",
                    "Create and Switch",
                );
                p.name = Some("New Workspace".into());
                p
            }
            A::ResetBrushes => {
                let mut p = ManagerPrompt::confirm(
                    "Reset All Brushes?",
                    "Restore every brush’s settings in this workspace to their defaults.",
                    "Reset Brushes",
                );
                p.destructive = true;
                p
            }
            A::Rename(_) => {
                let s = source()?;
                let reusable = s.kind == ItemKind::Toolbar;
                let mut p = ManagerPrompt::confirm(
                    "Rename",
                    if reusable {
                        "Choose a name and optional description."
                    } else {
                        ""
                    },
                    "Rename",
                );
                p.name = Some(s.name.clone());
                p.description = reusable.then(|| s.description.clone());
                p
            }
            A::SaveToolbar(panel) => {
                let current = self
                    .current()
                    .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
                let capture = current.capture()?;
                let toolbar = capture
                    .history
                    .layout()
                    .panel(*panel)
                    .map_err(StoreError::invalid)?;
                ToolbarDefinition::capture(toolbar)?;
                let mut p = ManagerPrompt::confirm(
                    "Save to Toolbar Library",
                    "Create a reusable copy of this toolbar. Its placement belongs to each receiving workspace.",
                    "Save to Library",
                );
                p.name = Some(toolbar.title().into());
                p
            }
            A::Reset(id) => {
                let s = source()?;
                if s.kind != ItemKind::Workspace {
                    return Err(StoreError::invalid("Choose a workspace."));
                }
                ManagerPrompt::confirm(
                    "Restore Starting Layout",
                    format!(
                        "{} The arrangement shown behind this dialog is a preview. You can undo restoring it with {} → Undo Workspace.",
                        if s.builtin && is_default_item(id) {
                            "Restore the latest default layout for this workspace."
                        } else {
                            "Restore this workspace’s saved starting layout."
                        },
                        layer_ui::WORKSPACE_MENU_LABEL
                    ),
                    "Restore",
                )
            }
            A::Delete(id) => {
                let s = source()?;
                let mut p = ManagerPrompt::confirm(
                    "Delete",
                    format!("Delete “{}”? This is permanent.", s.name),
                    "Delete",
                );
                if self.active_id().as_deref() == Some(id) {
                    p.message
                        .push_str(" This window will switch to an available default workspace.");
                }
                p.destructive = true;
                p
            }
            A::ReplaceToolbar(_) => {
                let choices = self.choices_for(ItemKind::Toolbar);
                if choices.is_empty() {
                    return Err(StoreError::invalid("Save a toolbar to the Library first."));
                }
                ManagerPrompt::confirm("Replace from Library","Replace this toolbar’s contents and display options while preserving its placement. Undo Layout Change can restore it.","Replace Toolbar")
                    .choices("Saved toolbar",choices,None)
            }
            A::UpdateToolbar(_) => {
                let current = self
                    .current()
                    .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
                ManagerPrompt::confirm("Update Saved Toolbar",format!("Choose a toolbar from {} to replace the saved {}. Existing workspace copies stay as they are.",current.metadata.name,source()?.name),"Update")
                    .choices("Toolbar",self.current_toolbar_choices()?,None)
            }
            A::NewToolbar(_) => self.new_toolbar_prompt(),
            A::SaveAsNew => {
                let mut p = ManagerPrompt::confirm(
                    "Save as New Workspace",
                    "Preserve the current in-memory layout, history, original reset target and latest tool values in an independent workspace.",
                    "Save and Switch",
                );
                p.name = Some("Recovered Workspace".into());
                p
            }
            _ => {
                return Err(StoreError::invalid(
                    "This action does not use a workspace form.",
                ));
            }
        })
    }
}
