use super::*;
use std::collections::BTreeMap;

/// Update only the untouched first Photographer arrangement, after its owner
/// has been claimed. Brush edits and renamed workspaces remain intact.
pub(super) fn updated_photographer_default(
    entity: &Entity,
    platform: Platform,
) -> Option<ItemContent> {
    use layer_ui::{Panel, TileStyle, WorkspacePreset};
    if entity.id != DEFAULT_WORKSPACES[2].0 || !entity.metadata.builtin {
        return None;
    }
    let ItemContent::Workspace {
        history, baseline, ..
    } = &entity.content
    else {
        return None;
    };
    if history.revisions.len() != 1 {
        return None;
    }
    let layout = WorkspacePreset::Photographer.layout(platform);
    let mut previous = layout.clone();
    for panel in [Panel::Toolbar, Panel::Commands] {
        previous
            .panels
            .iter_mut()
            .find(|p| p.id == panel)?
            .tile_style = TileStyle::Medium;
    }
    previous.bands[0].extent += TileStyle::Medium.size()[0] - TileStyle::Small.size()[0];
    if baseline != &previous || history.layout() != &previous {
        return None;
    }
    let mut content = entity.content.clone();
    if let ItemContent::Workspace {
        history, baseline, ..
    } = &mut content
    {
        history.revisions.values_mut().next()?.layout = layout.clone();
        *baseline = layout;
    }
    Some(content)
}

impl<S: WorkspaceStore> WorkspaceManager<S> {
    /// Migrate immutable legacy inputs atomically, once per source. Distinct
    /// scene sources remain distinct even when their layouts happen to match.
    /// A fallback aliases an identical scene instead of creating another copy.
    /// The host keeps the original files and supplies validated source names.
    pub async fn migrate_legacy(
        &self,
        scenes: &[(String, layer_ui::WorkspaceState)],
        fallback: Option<&(String, layer_ui::WorkspaceState)>,
        now: u64,
    ) -> Result<BTreeMap<String, String>> {
        // Re-read source mappings when another first-start window wins part
        // of our batch. Iteration keeps stack use bounded on native UI workers.
        for _ in 0..8 {
            if let Some(mappings) =
                Box::pin(self.migrate_legacy_once(scenes, fallback, now)).await?
            {
                return Ok(mappings);
            }
        }
        Err(StoreError::new(
            ErrorKind::Conflict,
            "Other windows are still importing saved workspaces. Retry after their import finishes.",
        ))
    }
    async fn migrate_legacy_once(
        &self,
        scenes: &[(String, layer_ui::WorkspaceState)],
        fallback: Option<&(String, layer_ui::WorkspaceState)>,
        now: u64,
    ) -> Result<Option<BTreeMap<String, String>>> {
        let mut mappings = BTreeMap::new();
        let mut mutations = Vec::new();
        let mut imports = Vec::new();
        let mut sources = std::collections::BTreeSet::new();
        for (source, _) in scenes.iter().chain(fallback) {
            if !sources.insert(source) {
                return Err(StoreError::invalid("A legacy source was supplied twice."));
            }
            let StoreResponse::Binding(existing) = self
                .store
                .execute(StoreRequest::LegacyImport {
                    source: source.clone(),
                })
                .await?
            else {
                return Err(StoreError::invalid("Unexpected migration reply."));
            };
            if let Some(id) = existing {
                mappings.insert(source.clone(), id);
            }
        }
        // Validate every unimported input before publishing anything. Previously
        // acknowledged sources are owned by the database, not the legacy file.
        for (source, workspace) in scenes.iter().chain(fallback) {
            if !mappings.contains_key(source) {
                workspace.validate().map_err(StoreError::invalid)?;
            }
        }
        for (source, workspace) in scenes.iter().chain(fallback) {
            if mappings.contains_key(source) {
                continue;
            }
            let identical_scene = fallback
                .is_some_and(|(key, _)| key == source)
                .then(|| scenes.iter().find(|(_, old)| old == workspace))
                .flatten()
                .and_then(|(key, _)| mappings.get(key))
                .cloned();
            let id = if let Some(id) = identical_scene {
                id
            } else {
                let capture = WorkspaceCapture::from_legacy(workspace.clone())
                    .map_err(StoreError::invalid)?;
                let entity = Entity::workspace(
                    "Imported Workspace",
                    capture,
                    workspace.layout.clone(),
                    None,
                    now,
                );
                let id = entity.id.clone();
                mutations.push(Mutation::Create {
                    entity,
                    claim: false,
                    name_policy: NamePolicy::Unique,
                });
                id
            };
            imports.push((source.clone(), id.clone()));
            mappings.insert(source.clone(), id);
        }
        if !imports.is_empty() {
            let known_sources = mappings.len() - imports.len();
            let mut batch = CommitBatch::prepare(self.owner.clone(), mutations)?;
            batch.legacy_imports = imports;
            let operation = batch.operation_id.clone();
            if let Err(error) = self.publish(batch).await {
                // Two native windows may read the legacy files at first start.
                // Use its completed mappings and retire our redundant pending
                // delivery. A partial overlap retries only the unmapped inputs.
                let mut completed = BTreeMap::new();
                for source in mappings.keys() {
                    if let StoreResponse::Binding(Some(id)) = self
                        .store
                        .execute(StoreRequest::LegacyImport {
                            source: source.clone(),
                        })
                        .await?
                    {
                        completed.insert(source.clone(), id);
                    }
                }
                if completed.len() <= known_sources {
                    return Err(error);
                }
                let mut cleanup = CommitBatch::prepare(self.owner.clone(), Vec::new())?;
                cleanup.abandon_operations.push(operation.clone());
                self.publish(cleanup).await?;
                let mut state = self.state.borrow_mut();
                if state
                    .failed_operation
                    .as_ref()
                    .is_some_and(|batch| batch.operation_id == operation)
                {
                    state.failed_operation = None;
                }
                state
                    .older_failed_operations
                    .retain(|batch| batch.operation_id != operation);
                if state.error_operation.as_ref() == Some(&operation) {
                    state.error = None;
                    state.error_operation = None;
                }
                if completed.len() != mappings.len() {
                    return Ok(None);
                }
                mappings = completed;
            }
            self.refresh().await?;
        }
        Ok(Some(mappings))
    }

    /// Local scene/window restoration identity stays outside portable exports.
    pub async fn bind_resume_key(&self, key: &str) -> Result<()> {
        let id = self
            .active_id()
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        let mut batch = CommitBatch::prepare(self.owner.clone(), Vec::new())?;
        batch.bindings.push((key.into(), Some(id)));
        self.publish(batch).await.map(|_| ())
    }
}
