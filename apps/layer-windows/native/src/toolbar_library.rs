//! Saved toolbar transport. Shared Core owns identities, installation and history.
use super::*;
use layer_workspace::{ItemContent, ReusableContent, ToolbarDefinition};

impl<S: WorkspaceStore + 'static> WorkspaceService<S> {
    pub(super) fn toolbar_page(&mut self, native: &mut NativeHost, page: Page) -> Result<()> {
        let Some(view) = self.ui.view.as_ref() else {
            return Ok(());
        };
        if !view.page.toolbars() || !page.toolbars() || view.page == page || view.prompt.is_some() {
            return Ok(());
        }
        self.preview_restore(native);
        self.ui.prompt = None;
        self.ui.pending = None;
        self.ui.retry = false;
        self.ui.toolbar_installed = false;
        let view = self.ui.view.as_mut().unwrap();
        view.page = page;
        view.selected = None;
        view.query.clear();
        view.error = None;
        view.can_retry = false;
        view.rows.clear();
        view.intro = if page == Page::ThisWorkspace {
            "Arrange the toolbars in this workspace."
        } else {
            "Save toolbars to reuse in any workspace."
        }
        .into();
        view.loading = true;
        let manager = self.manager.clone();
        let _ = self.ui.read.start(async move {
            manager.refresh().await?;
            Ok(ReadReply::Refresh)
        });
        native.invalidate_snapshot();
        Ok(())
    }

    pub(super) fn toolbar_action(
        &mut self,
        native: &mut NativeHost,
        action: ManagerAction,
    ) -> Result<()> {
        let Some(view) = self.ui.view.as_ref() else {
            return Ok(());
        };
        // Delayed native callbacks must still belong to this selection/page.
        if !view.page.toolbars()
            || view.prompt.is_some()
            || view.loading
            || !view
                .toolbar_actions
                .iter()
                .any(|a| a.enabled && a.action == action)
        {
            return Ok(());
        }
        match action {
            ManagerAction::SaveToolbar(panel) => self.prompt(native, Mutation::SaveToolbar(panel)),
            ManagerAction::ReplaceToolbar(panel) => {
                self.prompt(native, Mutation::ReplaceToolbar(panel))
            }
            ManagerAction::AddToolbar(id) => {
                self.queue_mutation(native, Mutation::AddToolbar(id), String::new(), None)
            }
            ManagerAction::Rename(id) => self.prompt(native, Mutation::Rename(id)),
            ManagerAction::Delete(id) => self.prompt(native, Mutation::Delete(id)),
            ManagerAction::ShowToolbar(panel, visible) => {
                native.session.end_workspace_transition();
                let result = native
                    .dispatch(UiAction::Customize {
                        action: CustomizationAction::SetPanelVisible { panel, visible },
                    })
                    .map_err(StoreError::invalid);
                native
                    .session
                    .begin_workspace_transition()
                    .map_err(StoreError::invalid)?;
                result?;
                self.observe(native, Instant::now(), now_ms(), true)?;
                self.rows(native, false, now_ms())
            }
            ManagerAction::RenameToolbar(panel)
            | ManagerAction::DuplicateToolbar(panel)
            | ManagerAction::DeleteToolbar(panel) => {
                let customize = match action {
                    ManagerAction::RenameToolbar(_) => CustomizationAction::RenameToolbar { panel },
                    ManagerAction::DuplicateToolbar(_) => {
                        CustomizationAction::DuplicateToolbar { panel }
                    }
                    _ => CustomizationAction::DeleteToolbar { panel },
                };
                self.close_manager(native);
                native
                    .dispatch(UiAction::Customize { action: customize })
                    .map_err(StoreError::invalid)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn submit_toolbar(
        &mut self,
        native: &mut NativeHost,
        operation: Mutation,
        name: String,
        choice: Option<String>,
        now: Instant,
        wall_ms: u64,
    ) {
        if self.ui.toolbar_installed {
            // Retry persistence of the accepted installation without inserting twice.
            self.flush_toolbar(native, now, wall_ms);
            return;
        }
        let manager = self.manager.clone();
        let _ = self.operation.start(async move {
            let result = async {
                manager.flush().await?;
                let (source, replace, group, rename) = match operation {
                    Mutation::NewToolbar { group } => (choice, None, group, Some(name)),
                    Mutation::AddToolbar(id) => (Some(id), None, None, None),
                    Mutation::ReplaceToolbar(panel) => (choice, Some(panel), None, None),
                    _ => unreachable!("Expected a toolbar installation"),
                };
                let mut definition = if let Some(id) = source.filter(|id| !id.is_empty()) {
                    let stored = manager.load(&id).await?;
                    if stored.entity.metadata.deleted_at_ms.is_some() {
                        return Err(StoreError::invalid("This saved toolbar was deleted."));
                    }
                    let ItemContent::Reusable { current, .. } = stored.entity.content else {
                        return Err(StoreError::invalid("Choose a saved toolbar."));
                    };
                    let ReusableContent::Toolbar { mut definition } = current.content else {
                        return Err(StoreError::invalid("Choose a saved toolbar."));
                    };
                    definition.name = stored.entity.metadata.name;
                    definition
                } else {
                    if replace.is_some() {
                        return Err(StoreError::invalid("Choose a saved toolbar."));
                    }
                    ToolbarDefinition {
                        name: "New Toolbar".into(),
                        tiles: Vec::new(),
                        tile_style: layer_ui::TileStyle::Small,
                        hide_tab: false,
                    }
                };
                if let Some(name) = rename {
                    definition.name = name.trim().into();
                }
                definition.validate()?;
                Ok((definition, replace, group))
            }
            .await;
            Completion::Toolbar(result)
        });
    }

    fn flush_toolbar(&mut self, native: &mut NativeHost, now: Instant, wall_ms: u64) {
        if let Err(error) = self.observe(native, now, wall_ms, true) {
            self.ui.submitted = false;
            self.ui_error(native, error);
            return;
        }
        let manager = self.manager.clone();
        let _ = self.operation.start(async move {
            let result = async {
                manager.flush().await?;
                manager.refresh().await?;
                Ok(None)
            }
            .await;
            Completion::Manager(result)
        });
    }

    pub(in crate::workspace_service) fn toolbar_completed(
        &mut self,
        native: &mut NativeHost,
        result: Result<(ToolbarDefinition, Option<Panel>, Option<u32>)>,
        now: Instant,
        wall_ms: u64,
    ) {
        let result = result.and_then(|(definition, replace, group)| {
            if self.status.owner_lost || !self.manager.lease_valid(wall_ms) {
                return Err(StoreError::new(
                    ErrorKind::OwnedElsewhere,
                    "Workspace ownership changed while loading the toolbar. Try again.",
                ));
            }
            let before = native.session.state().revision;
            let (_, change) = native
                .session
                .install_workspace_toolbar(
                    layer_ui::PanelConfig {
                        id: Panel::Toolbar,
                        hide_tab: definition.hide_tab,
                        tile_style: definition.tile_style,
                        content: layer_ui::PanelContent::Toolbar {
                            name: definition.name,
                            tiles: definition.tiles,
                        },
                    },
                    replace,
                    group,
                )
                .map_err(StoreError::invalid)?;
            native.apply_change(before, change);
            self.ui.toolbar_installed = true;
            Ok(())
        });
        match result {
            Ok(()) => self.flush_toolbar(native, now, wall_ms),
            Err(error) => {
                self.ui.submitted = false;
                self.ui_error(native, error);
            }
        }
        native.invalidate_snapshot();
    }
}
