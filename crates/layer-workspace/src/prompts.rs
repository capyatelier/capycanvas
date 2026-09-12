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
            .filter(|i| i.metadata.kind == kind && i.metadata.deleted_at_ms.is_none())
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
        let source = match action {
            A::EditAsWorkspace(id)
            | A::Rename(id)
            | A::Duplicate(id)
            | A::SaveAsTemplate(id)
            | A::Reset(id)
            | A::UpdateFromCurrent(id)
            | A::Delete(id)
            | A::UpdateToolbar(id) => Some(self.presentation_entity(id).await?),
            _ => None,
        };
        let mut p = match action {
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
            A::EditAsWorkspace(_) => {
                let mut choices = vec![ManagerChoice {
                    id: String::new(),
                    label: "Current layout".into(),
                }];
                choices.extend(self.choices_for(ItemKind::Template));
                let selected = source.as_ref().map(|s| s.id.clone());
                let mut p=ManagerPrompt::confirm("New Workspace","Choose a name and starting layout. Starting from a saved layout uses default tool settings.","Create and Switch")
                    .choices("Start with",choices,selected);
                p.name = Some("New Workspace".into());
                p
            }
            A::Rename(_) => {
                let s = source.as_ref().unwrap();
                let mut p = ManagerPrompt::confirm(
                    "Rename",
                    "Choose a name and optional description.",
                    "Rename",
                );
                p.name = Some(s.metadata.name.clone());
                p.description = Some(s.metadata.description.clone());
                p
            }
            A::Duplicate(_) => {
                let s = source.as_ref().unwrap();
                let mut p = if s.metadata.kind == ItemKind::Workspace {
                    ManagerPrompt::confirm(
                        "Duplicate Workspace",
                        "The copy keeps its own changes, history, working values, and original reset target.",
                        "Duplicate and Switch",
                    )
                } else {
                    ManagerPrompt::confirm(
                        "Duplicate",
                        "Create an independent, editable copy in your library.",
                        "Duplicate",
                    )
                };
                p.name = Some(format!("{} Copy", s.metadata.name));
                p
            }
            A::SaveAsTemplate(_) => {
                let mut p = ManagerPrompt::confirm(
                    "Save Layout",
                    "Capture panels, toolbars, and their customizations. Brush settings and colors are excluded. This does not change the current workspace’s original reset target.",
                    "Save Layout",
                );
                p.name = Some(format!("{} Layout", source.as_ref().unwrap().metadata.name));
                p.description = Some(String::new());
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
            A::Reset(_) => {
                let p = reset_prompt(source.as_ref().unwrap())?;
                ManagerPrompt::confirm(p.title, p.message, p.confirm)
            }
            A::UpdateFromCurrent(_) => {
                let current = self
                    .current()
                    .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
                let p = update_prompt(source.as_ref().unwrap(), &current);
                ManagerPrompt::confirm(p.title, p.message, p.confirm)
            }
            A::Delete(id) => {
                let s = source.as_ref().unwrap();
                let mut p = ManagerPrompt::confirm(
                    "Delete",
                    format!("Delete “{}”?", s.metadata.name),
                    "Delete",
                );
                if self.active_id().as_deref() == Some(id) {
                    p.message
                        .push_str(" This window will switch to the replacement workspace.");
                    let mut choices = vec![ManagerChoice {
                        id: String::new(),
                        label: "New workspace from Default".into(),
                    }];
                    choices.extend(
                        self.items()
                            .into_iter()
                            .filter(|i| {
                                i.id != *id
                                    && i.metadata.kind == ItemKind::Workspace
                                    && i.metadata.deleted_at_ms.is_none()
                                    && !i.claim.as_ref().is_some_and(|c| {
                                        c.owner != self.owner && c.expires_at_ms > now
                                    })
                            })
                            .map(|i| ManagerChoice {
                                id: i.id,
                                label: i.metadata.name,
                            }),
                    );
                    p = p.choices("Replacement workspace", choices, None);
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
                ManagerPrompt::confirm("Update Saved Toolbar",format!("Choose a toolbar from {} to update {}. Previous versions remain available; existing workspace copies stay as they are.",current.metadata.name,source.as_ref().unwrap().metadata.name),"Update")
                    .choices("Toolbar",self.current_toolbar_choices()?,None)
            }
            A::SaveAsNew => {
                let mut p = ManagerPrompt::confirm(
                    "Save as New Workspace",
                    "Preserve the current in-memory layout, history, original reset target and latest tool values in an independent workspace.",
                    "Save and Switch",
                );
                p.name = Some("Recovered Workspace".into());
                p
            }
            A::RecoverInterrupted => {
                let choices = self
                    .interrupted_changes(now)
                    .await?
                    .into_iter()
                    .map(|(id, label)| ManagerChoice { id, label })
                    .collect::<Vec<_>>();
                if choices.is_empty() {
                    return Err(StoreError::invalid(
                        "There are no interrupted changes to recover.",
                    ));
                }
                ManagerPrompt::confirm("Recover Interrupted Changes","Recover the selected changes into independent copies with unique names. Existing items stay as they are.","Recover Copies")
                    .choices("Interrupted changes",choices,None)
            }
            _ => {
                return Err(StoreError::invalid(
                    "This action does not use a workspace form.",
                ));
            }
        };
        if p.selected.is_none() {
            p.selected = p.choices.first().map(|c| c.id.clone());
        }
        Ok(p)
    }
}
