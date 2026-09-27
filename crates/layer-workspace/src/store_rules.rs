use crate::protocol::{PreparedWrite, unpack};
use crate::*;

type Result<T> = std::result::Result<T, StoreError>;

pub(crate) fn advance(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| StoreError::invalid("Workspace generation exhausted."))
}
pub(crate) fn parse_counter(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| StoreError::invalid("Invalid workspace generation."))
}
pub(crate) fn not_found() -> StoreError {
    StoreError::new(
        ErrorKind::NotFound,
        "This workspace item is no longer available.",
    )
}
pub(crate) fn missing_resource() -> StoreError {
    StoreError::invalid("A referenced workspace resource is missing.")
}
pub(crate) fn recovered_elsewhere() -> StoreError {
    StoreError::new(
        ErrorKind::Conflict,
        "These interrupted changes were already recovered into an independent item.",
    )
}
pub(crate) fn already_saved() -> StoreError {
    StoreError::new(
        ErrorKind::Conflict,
        "The interrupted changes already finished saving. Refresh the manager to view them.",
    )
}
/// Only a reset replaces a store, and never a newer one: a window still running
/// an older build must not discard the newer build's workspaces.
pub(crate) fn newer_schema() -> StoreError {
    StoreError::new(
        ErrorKind::UnsupportedSchema,
        "A newer version of Capy Canvas updated workspace storage. Reload or update Capy Canvas to continue.",
    )
}
pub(crate) fn other_schema() -> StoreError {
    StoreError::new(
        ErrorKind::UnsupportedSchema,
        "Workspace storage was written by an earlier version of Capy Canvas.",
    )
}
pub(crate) fn owned_elsewhere() -> StoreError {
    StoreError::new(
        ErrorKind::OwnedElsewhere,
        "This workspace is open in another window. Switch to that window or duplicate it.",
    )
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
        return Err(StoreError::new(
            ErrorKind::Conflict,
            "Workspace preferences changed in another window. Try again.",
        ));
    }
    for id in ids {
        if !is_workspace(id)? {
            return Err(StoreError::invalid(
                "This workspace is no longer available.",
            ));
        }
    }
    Ok(true)
}
pub(crate) fn validate_payload(
    batch: &CommitBatch,
    mut lookup: impl FnMut(&str) -> Result<String>,
) -> Result<()> {
    for (id, json) in &batch.components {
        if content_id(json.as_bytes()) != *id {
            return Err(StoreError::invalid("Invalid workspace component."));
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    for write in &batch.writes {
        if !ids.insert(&write.id) {
            return Err(StoreError::invalid(
                "An item may only be written once in one operation.",
            ));
        }
        if let Some(metadata) = &write.metadata {
            metadata.validate()?;
            if write.metadata_json.as_deref() != Some(&serde_json::to_string(metadata)?) {
                return Err(StoreError::invalid("Inconsistent metadata payload."));
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
            return Err(StoreError::invalid(
                "Included workspaces cannot be deleted.",
            ));
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
            return Err(StoreError::invalid("Item type cannot change."));
        }
        if stored.builtin && metadata.name != stored.name {
            return Err(StoreError::invalid(
                "Included workspaces cannot be renamed.",
            ));
        }
    }
    if write.working_json.is_some() && stored.kind != ItemKind::Workspace {
        return Err(StoreError::invalid(
            "Reusable items cannot store working values.",
        ));
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
                return Err(StoreError::new(
                    ErrorKind::NameCollision,
                    "An item with this name already exists.",
                ));
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
        return Err(StoreError::invalid(
            "The replacement workspace is unavailable.",
        ));
    }
    let fence = claim.ok_or_else(StoreError::conflict)?.fence;
    check_claim(claim, owner, fence, now)
}
pub(crate) fn summary_fallback(
    id: String,
    kind: Option<&str>,
    name: &str,
    builtin: bool,
    generations: Generations,
    claim: Option<Claim>,
    error: String,
) -> ItemSummary {
    let kind = match kind {
        Some("toolbar") => ItemKind::Toolbar,
        _ => ItemKind::Workspace,
    };
    let mut metadata = Metadata::new(kind, name, 0);
    metadata.builtin = builtin;
    ItemSummary {
        id,
        metadata,
        generations,
        claim,
        error: Some(error),
    }
}
