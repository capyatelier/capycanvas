use super::*;
use layer_workspace::{ItemContent, ItemKind, ManagerAction as A, ManagerPage, ReusableContent};

type Result<T> = std::result::Result<T, StoreError>;
pub(super) struct OperationGuard {
    workspace: std::rc::Weak<Workspace>,
    generation: u64,
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Some(w) = self.workspace.upgrade()
            && w.workspaces.operation_generation.get() == self.generation
        {
            w.workspaces.finish_operation(&w);
        }
    }
}
impl NativeWorkspaces {
    pub fn sync_binding(&self, w: &Rc<Workspace>) {
        if let Some(manager) = &self.manager
            && let Some(binding) = manager.binding()
        {
            let result = w
                .gpu
                .borrow_mut()
                .as_mut()
                .map(|g| g.session.configure_workspace_manager(binding));
            if let Some(result) = result {
                w.changed(result);
            }
        }
    }
    pub async fn command(
        &self,
        w: &Rc<Workspace>,
        command: WorkspaceCommand,
    ) -> std::result::Result<(), String> {
        let current = self.manager.as_ref().and_then(|m| m.active_id());
        let action = match command {
            WorkspaceCommand::Manage => {
                self.ui.show(w, ManagerPage::Workspaces);
                return Ok(());
            }
            WorkspaceCommand::ManageTemplates | WorkspaceCommand::SaveAsTemplate => {
                return Err("Use workspaces to save and load your setup.".into());
            }
            WorkspaceCommand::ManageToolbars => {
                self.ui.show(w, ManagerPage::ThisWorkspace);
                return Ok(());
            }
            WorkspaceCommand::Switch { id } => A::Switch(id),
            WorkspaceCommand::New => A::New,
            WorkspaceCommand::ResetBrushes => A::ResetBrushes,
            WorkspaceCommand::ResetLayout => A::Reset(current.ok_or("No workspace is active")?),
            WorkspaceCommand::LayoutHistory => A::History(current.ok_or("No workspace is active")?),
            WorkspaceCommand::SaveToolbar { panel } => A::SaveToolbar(panel),
            WorkspaceCommand::NewToolbar { group } => {
                return self.new_toolbar(w, group).await.map_err(|e| e.to_string());
            }
        };
        self.perform(w, action).await.map_err(|e| e.to_string())
    }
    pub(super) async fn begin_operation(&self, w: &Rc<Workspace>) -> Result<OperationGuard> {
        if self.busy.get() {
            return Err(StoreError::invalid(
                "A workspace change is already in progress.",
            ));
        }
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| StoreError::invalid("Workspace storage is unavailable."))?;
        {
            let mut gpu = w.gpu.borrow_mut();
            let gpu = gpu
                .as_mut()
                .ok_or_else(|| StoreError::invalid("Canvas unavailable."))?;
            gpu.session
                .require_workspace_idle()
                .map_err(StoreError::invalid)?;
            let capture = gpu
                .session
                .capture_workspace()
                .map_err(StoreError::invalid)?;
            manager.observe(capture, now_ms());
            gpu.session
                .begin_workspace_transition()
                .map_err(StoreError::invalid)?;
        }
        self.busy.set(true);
        self.update_status();
        self.operation_generation
            .set(self.operation_generation.get().wrapping_add(1));
        w.surface.set_sensitive(false);
        while manager.saving() || self.validating_owner.get() {
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        Ok(OperationGuard {
            workspace: Rc::downgrade(w),
            generation: self.operation_generation.get(),
        })
    }
    pub(super) fn finish_operation(&self, w: &Rc<Workspace>) {
        self.operation_generation
            .set(self.operation_generation.get().wrapping_add(1));
        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
            gpu.session.end_workspace_transition();
        }
        if let Some(manager) = &self.manager {
            manager.finish_transition();
        }
        self.busy.set(false);
        w.surface.set_sensitive(!self.validating_owner.get());
        self.update_status();
        if self.close_requested.get() {
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                w,
                move || w.window.close()
            ));
        }
    }
    pub(super) async fn selected(&self, id: &str) -> Result<StoredEntity> {
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| StoreError::invalid("Workspace storage is unavailable."))?;
        if manager.active_id().as_deref() == Some(id) {
            manager
                .current_record()
                .ok_or_else(|| StoreError::invalid("No workspace is active."))
        } else {
            manager.load(id).await
        }
    }
    async fn snapshot_source(&self, id: &str) -> Result<layer_workspace::Entity> {
        let stored = self.selected(id).await?;
        if let Some(claim) = &stored.claim
            && claim.owner != self.manager.as_ref().unwrap().owner
            && claim.expires_at_ms > now_ms()
        {
            let target = WINDOWS.with(|windows| {
                windows
                    .borrow()
                    .get(&claim.owner.id)
                    .and_then(std::rc::Weak::upgrade)
            });
            let target = target.ok_or_else(||StoreError::new(layer_workspace::ErrorKind::OwnedElsewhere,"This workspace is open in another application process. Close it there to finish saving before duplicating it here."))?;
            let _operation = target.workspaces.begin_operation(&target).await?;
            let manager = target.workspaces.manager.as_ref().unwrap();
            manager.flush().await?;
            return manager
                .current()
                .ok_or_else(|| StoreError::invalid("The source workspace closed."));
        }
        Ok(stored.entity)
    }
    pub async fn perform(&self, w: &Rc<Workspace>, action: A) -> Result<()> {
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| StoreError::invalid("Workspace storage is unavailable."))?;
        match action {
            A::Switch(id) => {
                if manager.active_id().as_deref() == Some(&id) {
                    return Ok(());
                }
                let _operation = self.begin_operation(w).await?;
                let incoming = manager.prepare_switch(&id, now_ms()).await;
                match incoming {
                    Ok(incoming) => {
                        self.ui.close();
                        self.adopt(w, Ok(incoming)).await;
                    }
                    Err(error) => {
                        self.finish_operation(w);
                        return Err(error);
                    }
                }
            }
            A::New | A::Duplicate(_) | A::Rename(_) | A::SaveToolbar(_) => {
                self.named(w, action).await?;
            }
            // These shared variants remain until the core layout-library API is retired.
            A::UseTemplate(_)
            | A::EditAsWorkspace(_)
            | A::SaveAsTemplate(_)
            | A::UpdateFromCurrent(_) => {
                return Err(StoreError::invalid(
                    "Use workspaces to save and load your setup.",
                ));
            }
            A::Reset(id) => {
                let stored = self.selected(&id).await?;
                let prompt = layer_workspace::reset_prompt(&stored.entity)?;
                if !dialog::confirm(w, &prompt.title, &prompt.message, prompt.confirm, false).await
                {
                    return Ok(());
                }
                let _operation = self.begin_operation(w).await?;
                let result = manager.change_layout(&id, None, now_ms()).await;
                match result {
                    Ok(incoming) if manager.active_id().as_deref() == Some(&id) => {
                        self.adopt(w, Ok(incoming)).await
                    }
                    Ok(_) => self.finish_operation(w),
                    Err(error) => {
                        self.finish_operation(w);
                        return Err(error);
                    }
                }
            }
            A::ResetBrushes => {
                let prompt = manager.prompt(&A::ResetBrushes, now_ms()).await?;
                if !dialog::confirm(
                    w,
                    &prompt.title,
                    &prompt.message,
                    &prompt.confirm,
                    prompt.destructive,
                )
                .await
                {
                    return Ok(());
                }
                let _operation = self.begin_operation(w).await?;
                let (change, capture) = {
                    let mut gpu = w.gpu.borrow_mut();
                    let session = &mut gpu.as_mut().unwrap().session;
                    let change = session
                        .reset_workspace_brushes()
                        .map_err(StoreError::invalid)?;
                    (
                        change,
                        session.capture_workspace().map_err(StoreError::invalid)?,
                    )
                };
                manager.observe(capture, now_ms());
                w.changed(Ok(change));
                manager.flush().await?;
            }
            A::Delete(id) => {
                let stored = self.selected(&id).await?;
                let active = manager.active_id().as_deref() == Some(&id);
                let replacement = if active {
                    manager.refresh().await?;
                    let items = manager.items();
                    let now = now_ms();
                    // Prefer Illustrator, then another available built-in workspace.
                    let replacement = [1, 0, 2].into_iter().find_map(|index| {
                        let default_id = layer_workspace::DEFAULT_WORKSPACES[index].0;
                        let item = items.iter().find(|item| item.id == default_id)?;
                        let available = item.id != id
                            && item.metadata.deleted_at_ms.is_none()
                            && !item.claim.as_ref().is_some_and(|claim| {
                                claim.owner != manager.owner && claim.expires_at_ms > now
                            });
                        available.then(|| (item.id.clone(), item.metadata.name.clone()))
                    }).ok_or_else(|| StoreError::invalid("All default workspaces are open in other windows. Close one of those windows before deleting this workspace."))?;
                    Some(replacement)
                } else {
                    None
                };
                let message = format!(
                    "Delete “{}”?{}",
                    stored.entity.metadata.name,
                    replacement
                        .as_ref()
                        .map(|(_, name)| format!(" You’ll switch to “{name}”."))
                        .unwrap_or_default()
                );
                if !dialog::confirm(w, "Delete", &message, "Delete", true).await {
                    return Ok(());
                }
                let _operation = self.begin_operation(w).await?;
                let result = manager
                    .delete_item(
                        &id,
                        replacement.as_ref().map(|(id, _)| id.as_str()),
                        now_ms(),
                    )
                    .await;
                match result {
                    Ok(Some(incoming)) => self.adopt(w, Ok(incoming)).await,
                    Ok(None) => self.finish_operation(w),
                    Err(error) => {
                        self.finish_operation(w);
                        return Err(error);
                    }
                }
            }
            A::ShowToolbar(panel, visible) => {
                w.customize(CustomizationAction::SetPanelVisible { panel, visible });
            }
            A::RenameToolbar(panel) => {
                self.ui.close();
                w.customize(CustomizationAction::RenameToolbar { panel });
            }
            A::DuplicateToolbar(panel) => {
                self.ui.close();
                w.customize(CustomizationAction::DuplicateToolbar { panel });
            }
            A::DeleteToolbar(panel) => {
                self.ui.close();
                w.customize(CustomizationAction::DeleteToolbar { panel });
            }
            A::History(id) => {
                history::show(w, &id).await?;
            }
            A::SwitchToWindow(id) => {
                let stored = self.selected(&id).await?;
                let target = stored.claim.and_then(|claim| {
                    WINDOWS.with(|windows| {
                        windows
                            .borrow()
                            .get(&claim.owner.id)
                            .and_then(std::rc::Weak::upgrade)
                    })
                });
                if let Some(target) = target {
                    target.window.present();
                    self.ui.close();
                } else {
                    return Err(StoreError::new(
                        layer_workspace::ErrorKind::OwnedElsewhere,
                        "This workspace is open in another window. Switch to that window to use it.",
                    ));
                }
            }
            A::AddToolbar(id) => {
                self.add_toolbar(w, &id, None, None, None).await?;
            }
            A::ReplaceToolbar(panel) => {
                let choices = self.library_choices();
                if choices.is_empty() {
                    return Err(StoreError::invalid("Save a toolbar to the Library first."));
                }
                if let Some(id) = dialog::choice_dialog(
                    w,
                    "Replace from Library",
                    "Choose a saved toolbar to replace this one.",
                    "Replace Toolbar",
                    &choices,
                )
                .await
                {
                    self.add_toolbar(w, &id, Some(panel), None, None).await?;
                }
            }
            A::UpdateToolbar(id) => {
                let source = manager
                    .current()
                    .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
                let capture = source.capture()?;
                let choices: Vec<_> = capture
                    .history
                    .layout()
                    .panels
                    .iter()
                    .filter(|p| p.id.kind() == PanelKind::Tiles)
                    .map(|p| (serde_json::to_string(&p.id).unwrap(), p.title().to_string()))
                    .collect();
                let target = self.selected(&id).await?;
                if let Some(panel) = dialog::choice_dialog(
                    w,
                    "Update Saved Toolbar",
                    &format!(
                        "Which toolbar from “{}” should replace the saved “{}”?",
                        source.metadata.name, target.entity.metadata.name
                    ),
                    "Update",
                    &choices,
                )
                .await
                {
                    let panel: Panel = serde_json::from_str(&panel)
                        .map_err(|e| StoreError::invalid(e.to_string()))?;
                    let _operation = self.begin_operation(w).await?;
                    let current = manager.current().unwrap().capture()?;
                    let definition = layer_workspace::ToolbarDefinition::capture(
                        current
                            .history
                            .layout()
                            .panel(panel)
                            .map_err(StoreError::invalid)?,
                    )?;
                    let result = manager
                        .update_reusable(&id, ReusableContent::Toolbar { definition }, now_ms())
                        .await;
                    self.finish_operation(w);
                    result?;
                }
            }
            A::SaveAsNew | A::RetryStorage | A::RecoverInterrupted => {
                self.storage_action(w, action).await?;
            }
        }
        if let Err(error) = manager.refresh().await {
            self.show_error(error);
        }
        self.sync_binding(w);
        Ok(())
    }
    async fn named(&self, w: &Rc<Workspace>, action: A) -> Result<()> {
        let manager = self.manager.as_ref().unwrap();
        let source_id = match &action {
            A::Duplicate(id) | A::Rename(id) => Some(id.as_str()),
            _ => None,
        };
        let source = if let Some(id) = source_id {
            Some(self.selected(id).await?)
        } else {
            None
        };
        let mut name = match &action {
            A::Rename(_) => source.as_ref().unwrap().entity.metadata.name.clone(),
            A::Duplicate(_) => format!("{} Copy", source.as_ref().unwrap().entity.metadata.name),
            A::SaveToolbar(panel) => manager
                .current()
                .unwrap()
                .capture()?
                .history
                .layout()
                .panel(*panel)
                .map_err(StoreError::invalid)?
                .title()
                .into(),
            _ => "New Workspace".into(),
        };
        let mut description = if matches!(action, A::Rename(_)) {
            source.as_ref().unwrap().entity.metadata.description.clone()
        } else {
            String::new()
        };
        let (title, message, confirm) = match &action {
            A::Rename(_) => ("Rename", "", "Rename"),
            A::Duplicate(_)
                if source
                    .as_ref()
                    .is_some_and(|s| s.entity.metadata.kind != ItemKind::Workspace) =>
            {
                ("Duplicate", "Make a separate copy.", "Duplicate")
            }
            A::Duplicate(_) => (
                "Duplicate Workspace",
                "Copy this workspace for another task.",
                "Duplicate and Switch",
            ),
            A::SaveToolbar(_) => (
                "Save to Toolbar Library",
                "Save this toolbar to reuse in any workspace.",
                "Save to Library",
            ),
            _ => (
                "New Workspace",
                "Copy your current tool settings and layout into a new workspace.",
                "Create and Switch",
            ),
        };
        let mut error = None;
        loop {
            let show_description = matches!(action, A::Rename(_))
                && source
                    .as_ref()
                    .is_some_and(|s| s.entity.metadata.kind == ItemKind::Toolbar);
            let desc = show_description.then_some(description.as_str());
            let Some(values) = dialog::name_dialog(
                w,
                title,
                message,
                confirm,
                &name,
                desc,
                &[],
                None,
                error.as_deref(),
            )
            .await
            else {
                return Ok(());
            };
            name = values.name;
            if show_description {
                description = values.description;
            }
            let _operation = self.begin_operation(w).await?;
            let outcome: Result<Option<StoredEntity>> = match &action {
                A::New => manager
                    .create_workspace(&name, None, false, now_ms())
                    .await
                    .map(Some),
                A::Duplicate(id)
                    if source.as_ref().unwrap().entity.metadata.kind == ItemKind::Workspace =>
                {
                    async {
                        let snapshot = self.snapshot_source(id).await?;
                        manager
                            .create_from_snapshot(snapshot, &name, true, now_ms())
                            .await
                            .map(Some)
                    }
                    .await
                }
                A::Duplicate(id) => manager
                    .duplicate_reusable(id, &name, now_ms())
                    .await
                    .map(|_| None),
                A::Rename(id) => manager
                    .rename(id, &name, &description, now_ms())
                    .await
                    .map(|_| None),
                A::SaveToolbar(panel) => manager
                    .save_toolbar(*panel, &name, now_ms())
                    .await
                    .map(|_| None),
                _ => unreachable!(),
            };
            match outcome {
                Ok(Some(incoming)) => {
                    self.ui.close();
                    self.adopt(w, Ok(incoming)).await;
                    return Ok(());
                }
                Ok(None) => {
                    self.finish_operation(w);
                    self.sync_binding(w);
                    return Ok(());
                }
                Err(failure) => {
                    self.finish_operation(w);
                    if matches!(
                        failure.kind,
                        layer_workspace::ErrorKind::InvalidData
                            | layer_workspace::ErrorKind::NameCollision
                    ) {
                        error = Some(failure.to_string());
                    } else {
                        return Err(failure);
                    }
                }
            }
        }
    }
}

impl NativeWorkspaces {
    fn library_choices(&self) -> Vec<(String, String)> {
        self.manager
            .as_ref()
            .unwrap()
            .items()
            .into_iter()
            .filter(|i| i.metadata.kind == ItemKind::Toolbar && i.metadata.deleted_at_ms.is_none())
            .map(|i| (i.id, i.metadata.name))
            .collect()
    }
    async fn add_toolbar(
        &self,
        w: &Rc<Workspace>,
        id: &str,
        replace: Option<Panel>,
        group: Option<u32>,
        name: Option<&str>,
    ) -> Result<()> {
        let stored = self.selected(id).await?;
        let ItemContent::Reusable { current, .. } = stored.entity.content else {
            return Err(StoreError::invalid("Choose a saved toolbar."));
        };
        let ReusableContent::Toolbar { mut definition } = current.content else {
            return Err(StoreError::invalid("Choose a saved toolbar."));
        };
        if let Some(name) = name {
            definition.name = name.into();
        }
        self.install_toolbar(w, definition, replace, group).await
    }
    async fn install_toolbar(
        &self,
        w: &Rc<Workspace>,
        definition: layer_workspace::ToolbarDefinition,
        replace: Option<Panel>,
        group: Option<u32>,
    ) -> Result<()> {
        definition.validate()?;
        let _operation = self.begin_operation(w).await?;
        let result = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| StoreError::invalid("Canvas unavailable."))?
            .session
            .install_workspace_toolbar(
                PanelConfig {
                    id: Panel::Toolbar,
                    hide_tab: definition.hide_tab,
                    tile_style: definition.tile_style,
                    content: PanelContent::Toolbar {
                        name: definition.name,
                        tiles: definition.tiles,
                    },
                },
                replace,
                group,
            )
            .map_err(StoreError::invalid);
        self.finish_operation(w);
        let (_, change) = result?;
        w.changed(Ok(change));
        self.ui.close();
        Ok(())
    }
    async fn new_toolbar(&self, w: &Rc<Workspace>, group: Option<u32>) -> Result<()> {
        let mut choices = vec![(String::new(), "Empty toolbar".into())];
        choices.extend(self.library_choices());
        let mut name = "New Toolbar".to_string();
        let mut error = None;
        loop {
            let Some(values) = dialog::name_dialog(
                w,
                "New Toolbar",
                "Keep the tools you use most together. Start empty or use a saved toolbar.",
                "Add to Workspace",
                &name,
                None,
                &choices,
                None,
                error.as_deref(),
            )
            .await
            else {
                return Ok(());
            };
            name = values.name;
            let result = if let Some(id) = values.choice.filter(|id| !id.is_empty()) {
                self.add_toolbar(w, &id, None, group, Some(&name)).await
            } else {
                self.install_toolbar(
                    w,
                    layer_workspace::ToolbarDefinition {
                        name: name.clone(),
                        tiles: Vec::new(),
                        tile_style: TileStyle::Small,
                        hide_tab: false,
                    },
                    None,
                    group,
                )
                .await
            };
            match result {
                Ok(()) => return Ok(()),
                Err(e)
                    if matches!(
                        e.kind,
                        layer_workspace::ErrorKind::InvalidData
                            | layer_workspace::ErrorKind::NameCollision
                    ) =>
                {
                    error = Some(e.to_string())
                }
                Err(e) => return Err(e),
            }
        }
    }
}
