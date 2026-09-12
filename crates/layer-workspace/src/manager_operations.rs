use super::*;

impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn create_from_library_version(
        &self,
        id: &str,
        version: &str,
        name: &str,
        now: u64,
    ) -> Result<StoredEntity> {
        self.flush().await?;
        let mut template = self.load(id).await?.entity;
        let ItemContent::Reusable { current, previous } = &mut template.content else {
            return Err(StoreError::invalid("Choose a Workspace Template."));
        };
        let selected = std::iter::once(&*current)
            .chain(previous.iter())
            .find(|v| v.id == version)
            .cloned()
            .ok_or_else(|| StoreError::invalid("This Workspace Template version is no longer available."))?;
        *current = selected;
        self.create_and_bind(
            self.workspace_from_template(&template, name, now)?,
            NamePolicy::Unique,
        )
        .await
    }
    pub async fn create_from_workspace(
        &self,
        id: &str,
        name: &str,
        duplicate: bool,
        now: u64,
    ) -> Result<StoredEntity> {
        self.flush().await?;
        let source = if self.active_id().as_deref() == Some(id) {
            self.current().unwrap()
        } else {
            let source = self.claim(id).await?;
            self.release(&source).await;
            source.entity
        };
        self.create_from_snapshot(source, name, duplicate, now)
            .await
    }
    pub async fn create_from_snapshot(
        &self,
        source: Entity,
        name: &str,
        duplicate: bool,
        now: u64,
    ) -> Result<StoredEntity> {
        self.flush().await?;
        let capture = source.capture()?;
        let ItemContent::Workspace {
            baseline, origin, ..
        } = source.content
        else {
            unreachable!()
        };
        let entity = if duplicate {
            Entity::workspace(name, capture, baseline, origin, now)
        } else {
            let baseline = capture.history.layout().clone();
            Entity::workspace(
                name,
                WorkspaceCapture {
                    history: layer_ui::LayoutHistory::new(&baseline),
                    working: capture.working,
                },
                baseline,
                None,
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
    pub async fn save_template_from(
        &self,
        id: &str,
        name: &str,
        description: &str,
        now: u64,
    ) -> Result<String> {
        self.flush().await?;
        let source = if self.active_id().as_deref() == Some(id) {
            self.current().unwrap()
        } else {
            let source = self.claim(id).await?;
            self.release(&source).await;
            source.entity
        };
        self.save_template_snapshot(source, name, description, now)
            .await
    }
    pub async fn save_template_snapshot(
        &self,
        source: Entity,
        name: &str,
        description: &str,
        now: u64,
    ) -> Result<String> {
        let entity = Entity::reusable(
            name,
            description,
            ReusableContent::Layout {
                layout: source.capture()?.history.layout().clone(),
            },
            now,
        );
        self.save_reusable(entity, NamePolicy::Exact).await
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
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        let capture = current.capture()?;
        let mut definition = ToolbarDefinition::capture(
            capture
                .history
                .layout()
                .panel(panel)
                .map_err(StoreError::invalid)?,
        )?;
        definition.name = name.trim().into();
        self.save_reusable(
            Entity::reusable(name, "", ReusableContent::Toolbar { definition }, now),
            NamePolicy::Exact,
        )
        .await
    }
    pub async fn duplicate_reusable(&self, id: &str, name: &str, now: u64) -> Result<String> {
        let mut entity = self.load(id).await?.entity;
        if entity.metadata.kind == ItemKind::Workspace {
            return Err(StoreError::invalid("Choose a Workspace Template or saved toolbar."));
        }
        entity.id = new_id();
        entity.metadata.builtin = false;
        entity
            .metadata
            .rename(name, &entity.metadata.description.clone(), now)?;
        entity.metadata.created_at_ms = now;
        entity.metadata.deleted_at_ms = None;
        self.save_reusable(entity, NamePolicy::Unique).await
    }
    pub async fn update_reusable(
        &self,
        id: &str,
        content: ReusableContent,
        now: u64,
    ) -> Result<()> {
        self.flush().await?;
        let stored = self.claim(id).await?;
        let result: Result<()> = async {
            let ItemContent::Reusable { current, previous } = &stored.entity.content else {
                return Err(StoreError::invalid(
                    "Choose a Workspace Template or saved toolbar.",
                ));
            };
            if current.content.kind() != content.kind() {
                return Err(StoreError::invalid(
                    "The library item has a different type.",
                ));
            }
            let mut previous = previous.clone();
            previous.push(current.clone());
            let mut metadata = stored.entity.metadata.clone();
            metadata.modified_at_ms = now;
            let content = ItemContent::Reusable {
                current: ReusableVersion {
                    id: new_id(),
                    name: metadata.name.clone(),
                    description: metadata.description.clone(),
                    content,
                    timestamp_ms: now,
                },
                previous,
            };
            self.publish(CommitBatch::prepare(
                self.owner.clone(),
                vec![update(&stored, Some(metadata), Some(content), None)?],
            )?)
            .await?;
            Ok(())
        }
        .await;
        self.release(&stored).await;
        result?;
        self.refresh().await
    }
    pub async fn restore_reusable_version(&self, id: &str, version: &str, now: u64) -> Result<()> {
        let stored = self.load(id).await?;
        let ItemContent::Reusable { current, previous } = &stored.entity.content else {
            return Err(StoreError::invalid(
                "Choose a Workspace Template or saved toolbar.",
            ));
        };
        let selected = std::iter::once(current)
            .chain(previous)
            .find(|v| v.id == version)
            .ok_or_else(|| StoreError::invalid("This previous version is no longer retained."))?;
        self.update_reusable(id, selected.content.clone(), now)
            .await
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
            let mut content = stored.entity.content.clone();
            let ItemContent::Workspace {
                history, baseline, ..
            } = &mut content
            else {
                return Err(StoreError::invalid("Choose a workspace."));
            };
            let layout = if let Some(revision) = revision {
                history
                    .revisions
                    .get(revision)
                    .ok_or_else(|| {
                        StoreError::invalid("This layout version is no longer retained.")
                    })?
                    .layout
                    .clone()
            } else {
                baseline.clone()
            };
            history.append(
                &layout,
                if revision.is_some() {
                    "Restored earlier layout"
                } else {
                    "Reset to starting layout"
                },
            );
            for r in history
                .revisions
                .values_mut()
                .filter(|r| r.timestamp_ms == 0)
            {
                r.timestamp_ms = now;
            }
            self.publish(CommitBatch::prepare(
                self.owner.clone(),
                vec![update(&stored, None, Some(content), None)?],
            )?)
            .await?;
            self.load(id).await
        }
        .await;
        if self.active_id().as_deref() != Some(id) {
            self.release(&stored).await;
        }
        result
    }
    pub async fn open_history_as_workspace(
        &self,
        id: &str,
        revision: &str,
        name: &str,
        now: u64,
    ) -> Result<StoredEntity> {
        self.flush().await?;
        let source = if self.active_id().as_deref() == Some(id) {
            self.current().unwrap()
        } else {
            self.load(id).await?.entity
        };
        let capture = source.capture()?;
        let baseline = capture
            .history
            .revisions
            .get(revision)
            .ok_or_else(|| StoreError::invalid("This layout version is no longer retained."))?
            .layout
            .clone();
        let entity = Entity::workspace(
            name,
            WorkspaceCapture {
                history: layer_ui::LayoutHistory::new(&baseline),
                working: capture.working,
            },
            baseline,
            None,
            now,
        );
        self.create_and_bind(entity, NamePolicy::Unique).await
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
            let mut metadata = deleting.entity.metadata.clone();
            metadata.deleted_at_ms = Some(now);
            let mut mutations = vec![update(&deleting, Some(metadata), None, None)?];
            let replacement_id = if active {
                if let Some(replacement) = replacement {
                    if replacement == id {
                        return Err(StoreError::invalid(
                            "Choose a different replacement workspace.",
                        ));
                    }
                    let claimed = self.claim(replacement).await?;
                    PreparedWorkspace::new(claimed.entity.capture()?)
                        .map_err(StoreError::invalid)?;
                    let mut metadata = claimed.entity.metadata.clone();
                    metadata.last_used_ms = now;
                    mutations.push(update(&claimed, Some(metadata), None, None)?);
                    incoming = Some(claimed);
                    Some(replacement.to_string())
                } else {
                    let template = self.load(DEFAULT_TEMPLATE_ID).await?;
                    let entity =
                        self.workspace_from_template(&template.entity, "My Workspace", now)?;
                    let replacement = entity.id.clone();
                    mutations.push(Mutation::Create {
                        entity,
                        claim: true,
                        name_policy: NamePolicy::Unique,
                    });
                    Some(replacement)
                }
            } else {
                None
            };
            let mut batch = CommitBatch::prepare(self.owner.clone(), mutations)?;
            if let Some(id) = &replacement_id {
                batch
                    .bindings
                    .push(("last_workspace".into(), Some(id.clone())));
                batch
                    .bindings
                    .push((format!("window:{}", self.owner.id), Some(id.clone())));
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
                .store
                .execute(StoreRequest::Binding {
                    key: format!("window:{}", self.owner.id),
                })
                .await?
            else {
                return Err(StoreError::invalid("Replacement binding is unavailable."));
            };
            self.load(&id).await.map(Some)
        } else {
            Ok(None)
        }
    }
    pub async fn restore_deleted(&self, id: &str, now: u64) -> Result<()> {
        let stored = self.claim(id).await?;
        let mut metadata = stored.entity.metadata.clone();
        metadata.deleted_at_ms = None;
        metadata.modified_at_ms = now;
        let mut mutation = update(&stored, Some(metadata), None, None)?;
        if let Mutation::Update { name_policy, .. } = &mut mutation {
            *name_policy = NamePolicy::Unique;
        }
        let result = self
            .publish(CommitBatch::prepare(self.owner.clone(), vec![mutation])?)
            .await;
        self.release(&stored).await;
        result?;
        self.refresh().await
    }
}
