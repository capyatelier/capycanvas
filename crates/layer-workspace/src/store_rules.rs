use crate::protocol::{PreparedWrite, unpack};
use crate::*;

type Result<T> = std::result::Result<T, StoreError>;

pub(crate) fn advance(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::WorkspaceGenerationExhausted))
}
pub(crate) fn parse_counter(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InvalidWorkspaceGeneration))
}
pub(crate) fn not_found() -> StoreError {
    StoreError::known(ErrorKind::NotFound, WorkspaceRefusal::ThisWorkspaceItemIsNoLongerAvailable)
}
pub(crate) fn missing_resource() -> StoreError {
    StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::AReferencedWorkspaceResourceIsMissing)
}
pub(crate) fn recovered_elsewhere() -> StoreError {
    StoreError::known(ErrorKind::Conflict, WorkspaceRefusal::TheseInterruptedChangesWereAlreadyRecoveredIntoAnIndependentItem)
}
pub(crate) fn already_saved() -> StoreError {
    StoreError::known(ErrorKind::Conflict, WorkspaceRefusal::TheInterruptedChangesAlreadyFinishedSaving)
}
/// Only a reset replaces a store, and never a newer one: a window still running
/// an older build must not discard the newer build's workspaces.
pub(crate) fn newer_schema() -> StoreError {
    StoreError::known(ErrorKind::UnsupportedSchema, WorkspaceRefusal::ANewerVersionOfCapyCanvasUpdatedWorkspaceStorage)
}
pub(crate) fn other_schema() -> StoreError {
    StoreError::known(ErrorKind::UnsupportedSchema, WorkspaceRefusal::WorkspaceStorageWasWrittenByAnEarlierVersionOfCapyCanvas)
}
pub(crate) fn owned_elsewhere() -> StoreError {
    StoreError::known(ErrorKind::OwnedElsewhere, WorkspaceRefusal::ThisWorkspaceIsOpenInAnotherWindow)
}
pub(crate) fn claim_fence(
    claim: Option<&Claim>,
    owner: &Owner,
    fence: u64,
    now: u64,
) -> Result<u64> {
    match claim.filter(|c| c.expires_at_ms > now) {
        Some(c) if &c.owner != owner => Err(owned_elsewhere()),
        Some(_) => Ok(fence),
        None => advance(fence),
    }
}
pub(crate) fn check_claim(
    claim: Option<&Claim>,
    owner: &Owner,
    fence: u64,
    now: u64,
) -> Result<()> {
    if claim.is_none_or(|c| &c.owner != owner || c.fence != fence || c.expires_at_ms <= now) {
        return Err(StoreError::conflict());
    }
    Ok(())
}
pub(crate) fn update_preference(
    current: Option<&Vec<String>>,
    expected: Option<Vec<String>>,
    ids: &[String],
    mut is_workspace: impl FnMut(&str) -> Result<bool>,
) -> Result<bool> {
    validate_switcher_ids(ids)?;
    if current.is_some_and(|current| current == ids) {
        return Ok(false);
    }
    if current != expected.as_ref() {
        return Err(StoreError::known(ErrorKind::Conflict, WorkspaceRefusal::WorkspacePreferencesChangedInAnotherWindow));
    }
    for id in ids {
        if !is_workspace(id)? {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisWorkspaceIsNoLongerAvailable));
        }
    }
    Ok(true)
}
pub(crate) fn validate_payload(
    batch: &CommitBatch,
    mut lookup: impl FnMut(&str) -> Result<String>,
) -> Result<()> {
    if let Some(json) = &batch.editing_json { serde_json::from_str::<layer_ui::EditingState>(json)?.validate().map_err(StoreError::workspace)?; }
    for (id, json) in &batch.components {
        if content_id(json.as_bytes()) != *id {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InvalidWorkspaceComponent));
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    for write in &batch.writes {
        if !ids.insert(&write.id) {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::AnItemMayOnlyBeWrittenOnceInOneOperation));
        }
        if let Some(metadata) = &write.metadata {
            metadata.validate()?;
            if write.metadata_json.as_deref() != Some(&serde_json::to_string(metadata)?) {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InconsistentMetadataPayload));
            }
        }
        if let Some(content) = &write.content_json {
            unpack(content, |id| match batch.components.get(id) {
                Some(json) => Ok(json.clone()),
                None => lookup(id),
            })?;
        }
    }
    Ok(())
}
pub(crate) fn check_update(
    stored: &Metadata,
    generations: Generations,
    write: &PreparedWrite,
) -> Result<()> {
    if write.delete {
        if write.expected != generations {
            return Err(StoreError::conflict());
        }
        if stored.builtin {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::IncludedWorkspacesCannotBeDeleted));
        }
        return Ok(());
    }
    if (write.metadata.is_some() && write.expected.metadata != generations.metadata)
        || (write.content_json.is_some() && write.expected.layout != generations.layout)
        || (write.working_json.is_some() && write.expected.working != generations.working)
    {
        return Err(StoreError::conflict());
    }
    if let Some(metadata) = &write.metadata {
        if metadata.kind != stored.kind || metadata.builtin != stored.builtin {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ItemTypeCannotChange));
        }
        if stored.builtin && metadata.name != stored.name {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::IncludedWorkspacesCannotBeRenamed));
        }
    }
    if write.working_json.is_some() && stored.kind != ItemKind::Workspace {
        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ReusableItemsCannotStoreWorkingValues));
    }
    Ok(())
}
pub(crate) fn apply_name_policy(
    mut metadata: Metadata,
    policy: NamePolicy,
    mut exists: impl FnMut(&str) -> Result<bool>,
) -> Result<Metadata> {
    match policy {
        NamePolicy::Unique => metadata.name = available_name(&metadata.name, exists)?,
        NamePolicy::Exact => {
            if exists(&name_key(&metadata.name))? {
                return Err(StoreError::known(ErrorKind::NameCollision, WorkspaceRefusal::AnItemWithThisNameAlreadyExists));
            }
        }
    }
    Ok(metadata)
}
pub(crate) fn check_binding(
    workspace: bool,
    claim: Option<&Claim>,
    owner: &Owner,
    now: u64,
) -> Result<()> {
    if !workspace {
        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::TheReplacementWorkspaceIsUnavailable));
    }
    let fence = claim.ok_or_else(StoreError::conflict)?.fence;
    check_claim(claim, owner, fence, now)
}
pub(crate) fn summary_fallback(
    id: String,
    kind: Option<&str>,
    name: Option<&str>,
    builtin: bool,
    generations: Generations,
    claim: Option<Claim>,
    error: StoreError,
) -> ItemSummary {
    let kind = match kind {
        Some("toolbar") => ItemKind::Toolbar,
        _ => ItemKind::Workspace,
    };
    let mut metadata = Metadata::new(kind, name.unwrap_or_default(), 0);
    metadata.name = name.unwrap_or_default().into();
    metadata.builtin = builtin;
    ItemSummary {
        id,
        metadata,
        generations,
        claim,
        error: Some(error),
        unavailable_name: name.is_none(),
    }
}
