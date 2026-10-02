use crate::WorkspaceRefusal;
use super::*;

impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn create_from_snapshot(
        &self,
        source: Entity,
        name: &str,
        duplicate: bool,
        now: u64,
    ) -> Result<StoredEntity> {
        self.flush().await?;
        let capture = source.capture()?;
        let ItemContent::Workspace { baseline, .. } = source.content else {
            unreachable!()
        };
        let entity = if duplicate {
            Entity::workspace(name, capture, *baseline, now)
        } else {
            let baseline = capture.history.layout().clone();
            Entity::workspace(
                name,
                WorkspaceCapture {
                    history: layer_ui::LayoutHistory::new(&baseline),
                    working: capture.working,
                },
                baseline,
                now,
            )
        };
        self.create_and_bind(
            entity,
            if duplicate {
                NamePolicy::Unique
            } else {
                NamePolicy::Exact
            },
        )
        .await
    }
    pub(crate) async fn save_reusable(&self, entity: Entity, policy: NamePolicy) -> Result<String> {
        let id = entity.id.clone();
        self.publish(CommitBatch::prepare(
            self.owner.clone(),
            vec![Mutation::Create {
                entity,
                claim: false,
                name_policy: policy,
            }],
        )?)
        .await?;
        self.refresh().await?;
        Ok(id)
    }
    pub async fn save_toolbar(
        &self,
        panel: layer_ui::Panel,
        name: &str,
        now: u64,
    ) -> Result<String> {
        self.flush().await?;
        let current = self
            .current()
            .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoWorkspaceIsActive))?;
        let capture = current.capture()?;
        let mut definition = ToolbarDefinition::capture(
            capture
                .history
                .layout()
                .panel(panel)
                .map_err(StoreError::invalid)?,
            &self.localization,
        )?;
        definition.name = name.trim().into();
        self.save_reusable(Entity::toolbar(definition, now), NamePolicy::Exact)
            .await
    }
    pub async fn update_toolbar(
        &self,
        id: &str,
        definition: ToolbarDefinition,
        now: u64,
    ) -> Result<()> {
        self.flush().await?;
        let stored = self.claim(id).await?;
        let result: Result<()> = async {
            if !matches!(stored.entity.content, ItemContent::Toolbar { .. }) {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseASavedToolbar));
            }
            let mut metadata = stored.entity.metadata.clone();
            metadata.modified_at_ms = now;
            self.publish(CommitBatch::prepare(
                self.owner.clone(),
                vec![update(
                    &stored,
                    Some(metadata),
                    Some(ItemContent::Toolbar { definition }),
                    None,
                )?],
            )?)
            .await?;
            Ok(())
        }
        .await;
        self.release(&stored).await;
        result?;
        self.refresh().await
    }
    pub async fn update_toolbar_from(
        &self,
        id: &str,
        panel: layer_ui::Panel,
        now: u64,
    ) -> Result<()> {
        let current = self
            .current()
            .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoWorkspaceIsActive))?;
        let definition = ToolbarDefinition::capture(
            current
                .capture()?
                .history
                .layout()
                .panel(panel)
                .map_err(StoreError::invalid)?,
            &self.localization,
        )?;
        self.update_toolbar(id, definition, now).await
    }
    /// A saved toolbar copy, or an empty toolbar when `source` is empty, as an
    /// installable panel. A typed `name` replaces the saved name.
    pub async fn toolbar_config(
        &self,
        source: Option<&str>,
        name: Option<&str>,
    ) -> Result<layer_ui::PanelConfig> {
        let mut definition = match source.filter(|id| !id.is_empty()) {
            Some(id) => {
                let stored = self.load(id).await?;
                let ItemContent::Toolbar { mut definition } = stored.entity.content else {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseASavedToolbar));
                };
                definition.name = stored.entity.metadata.name;
                definition
            }
            None => ToolbarDefinition {
                name: self.localization.text(layer_ui::MessageId::WORKSPACE_NEW_TOOLBAR).to_string(),
                tiles: Vec::new(),
                tile_style: layer_ui::TileStyle::Small,
                hide_tab: false,
            },
        };
        if let Some(name) = name {
            definition.name = name.trim().into();
        }
        definition.validate()?;
        Ok(layer_ui::PanelConfig {
            id: layer_ui::Panel::Toolbar,
            hide_tab: definition.hide_tab,
            tile_style: definition.tile_style,
            content: layer_ui::PanelContent::Toolbar {
                name: Some(definition.name),
                tiles: definition.tiles,
            },
        })
    }
    async fn publish_layout(
        &self,
        stored: &StoredEntity,
        layout: &DockLayout,
        description: layer_ui::LayoutChange,
        now: u64,
    ) -> Result<StoredEntity> {
        let mut content = stored.entity.content.clone();
        let ItemContent::Workspace { history, .. } = &mut content else {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAWorkspace));
        };
        history.append(layout, description);
        for revision in history
            .revisions
            .values_mut()
            .filter(|r| r.timestamp_ms == 0)
        {
            revision.timestamp_ms = now;
        }
        self.publish(CommitBatch::prepare(
            self.owner.clone(),
            vec![update(stored, None, Some(content), None)?],
        )?)
        .await?;
        self.load(&stored.entity.id).await
    }
    pub async fn change_layout(
        &self,
        id: &str,
        revision: Option<&str>,
        now: u64,
    ) -> Result<StoredEntity> {
        self.flush().await?;
        let stored = self.claim(id).await?;
        let result: Result<StoredEntity> = async {
            let ItemContent::Workspace {
                history, ..
            } = &stored.entity.content
            else {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAWorkspace));
            };
            let layout = if let Some(revision) = revision {
                history
                    .revisions
                    .get(revision)
                    .ok_or_else(|| {
                        StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisLayoutVersionIsNoLongerRetained)
                    })?
                    .layout
                    .clone()
            } else {
                stored.entity.starting_layout(self.platform)?
            };
            self.publish_layout(
                &stored,
                &layout,
                if revision.is_some() { layer_ui::LayoutChange::RestoredEarlier } else { layer_ui::LayoutChange::Restored },
                now,
            )
            .await
        }
        .await;
        if self.active_id().as_deref() != Some(id) {
            self.release(&stored).await;
        }
        result
    }
    /// Choose an available included workspace when deleting the active one.
    /// Inactive items need no replacement; this never creates a new workspace.
    pub async fn replacement_for_delete(&self, id: &str, now: u64) -> Result<Option<String>> {
        if self.active_id().as_deref() != Some(id) {
            return Ok(None);
        }
        self.refresh().await?;
        let items = self.items();
        // Prefer Illustrator, then Painter and Photographer. Respect other windows' leases.
        let replacement = [1, 0, 2].into_iter().find_map(|index| {
            let default_id = DEFAULT_WORKSPACES[index].0;
            let item = items.iter().find(|item| item.id == default_id)?;
            let available = item.id != id
                && !item.claim.as_ref().is_some_and(|claim| {
                    claim.owner != self.owner && claim.expires_at_ms > now
                });
            available.then(|| item.id.clone())
        }).ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::AllDefaultWorkspacesAreOpenInOtherWindows))?;
        Ok(Some(replacement))
    }
    pub async fn delete_item(
        &self,
        id: &str,
        replacement: Option<&str>,
        now: u64,
    ) -> Result<Option<StoredEntity>> {
        self.flush().await?;
        let deleting = self.claim(id).await?;
        let active = self.active_id().as_deref() == Some(id);
        let mut incoming = None;
        let result: Result<Option<StoredEntity>> = async {
            if deleting.entity.metadata.builtin {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::IncludedLayoutsAndWorkspacesCannotBeDeleted));
            }
            let mut mutations = vec![Mutation::Delete {
                id: id.into(),
                generations: deleting.generations,
                fence: deleting
                    .claim
                    .as_ref()
                    .ok_or_else(StoreError::conflict)?
                    .fence,
            }];
            let replacement_id = if active {
                let replacement = match replacement {
                    Some(id) => id.to_string(),
                    None => self.replacement_for_delete(id, now).await?.ok_or_else(|| {
                        StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoReplacementWorkspaceIsAvailable)
                    })?,
                };
                if replacement == id {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseADifferentReplacementWorkspace));
                }
                let claimed = self.claim(&replacement).await?;
                incoming = Some(claimed.clone());
                PreparedWorkspace::new(claimed.entity.capture()?).map_err(StoreError::workspace)?;
                let mut metadata = claimed.entity.metadata.clone();
                metadata.last_used_ms = now;
                mutations.push(update(&claimed, Some(metadata), None, None)?);
                Some(replacement)
            } else {
                None
            };
            let mut batch = CommitBatch::prepare(self.owner.clone(), mutations)?;
            if let Some(id) = &replacement_id {
                self.bind_window(&mut batch, id);
            }
            self.publish(batch).await?;
            match replacement_id {
                Some(id) => self.load(&id).await.map(Some),
                None => Ok(None),
            }
        }
        .await;
        if result.is_err() {
            if !active {
                self.release(&deleting).await;
            }
            if let Some(incoming) = incoming {
                self.release(&incoming).await;
            }
        }
        result?;
        self.refresh().await?;
        // Return the new aggregate for synchronous prepared adoption in the host.
        if active {
            let StoreResponse::Binding(Some(id)) = self
                .execute(StoreRequest::Binding {
                    key: format!("window:{}", self.owner.id),
                })
                .await?
            else {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ReplacementBindingIsUnavailable));
            };
            self.load(&id).await.map(Some)
        } else {
            Ok(None)
        }
    }
}
