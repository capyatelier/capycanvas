use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    WorkspaceBackup,
    Template,
    Toolbar,
}
impl PackageKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::WorkspaceBackup => "Workspace Backup",
            Self::Template => "Layout",
            Self::Toolbar => "Saved Toolbar",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::WorkspaceBackup => "capyworkspace",
            Self::Template => "capytemplate",
            Self::Toolbar => "capytoolbar",
        }
    }
    pub fn for_entity(entity: &Entity) -> Self {
        match entity.metadata.kind {
            ItemKind::Workspace => Self::WorkspaceBackup,
            ItemKind::Template => Self::Template,
            ItemKind::Toolbar => Self::Toolbar,
        }
    }
}

/// Portable JSON plus hash-verified, package-local immutable components. No
/// ownership, bindings, write generations, pending operations or artwork.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    format: String,
    version: u32,
    kind: PackageKind,
    metadata: Metadata,
    content: serde_json::Value,
    working: Option<layer_ui::WorkspaceWorkingState>,
    components: BTreeMap<String, Vec<u8>>,
}
pub fn export_package(entity: &Entity) -> Result<Vec<u8>, StoreError> {
    entity.validate()?;
    let mut metadata = entity.metadata.clone();
    metadata.builtin = false;
    metadata.deleted_at_ms = None;
    let mut content = entity.content.clone();
    if let ItemContent::Reusable { previous, .. } = &mut content {
        previous.clear();
        metadata.previous.clear();
    }
    let mut components = BTreeMap::new();
    let mut value = serde_json::to_value(content)?;
    // History's counter is local navigation bookkeeping, not portable identity.
    if let Some(history) = value.get_mut("history") {
        history["generation"] = "0".into();
    }
    let content = protocol::pack(value, &mut components)?;
    let bytes = serde_json::to_vec(&Package {
        format: "capycanvas-workspace".into(),
        version: 1,
        kind: PackageKind::for_entity(entity),
        metadata,
        content,
        working: entity.working.clone(),
        components,
    })?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(StoreError::invalid(
            "The package exceeds the supported size.",
        ));
    }
    Ok(bytes)
}
pub fn import_package(bytes: &[u8], expected: PackageKind, now: u64) -> Result<Entity, StoreError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(StoreError::invalid(
            "The package exceeds the supported size.",
        ));
    }
    let header: serde_json::Value = serde_json::from_slice(bytes)?;
    if header.get("format").and_then(|v| v.as_str()) != Some("capycanvas-workspace")
        || header.get("version").and_then(|v| v.as_u64()) != Some(1)
    {
        return Err(StoreError::new(
            ErrorKind::UnsupportedSchema,
            "This package needs a supported version of Capy Canvas. The original file has been preserved.",
        ));
    }
    let package: Package = serde_json::from_value(header)?;
    if package.kind != expected {
        return Err(StoreError::invalid(format!(
            "Choose a {} file.",
            expected.label()
        )));
    }
    for (id, bytes) in &package.components {
        if content_id(bytes) != *id {
            return Err(StoreError::invalid("A packaged resource is corrupt."));
        }
    }
    let mut used = std::collections::BTreeSet::new();
    let mut content = protocol::unpack(&serde_json::to_string(&package.content)?, |id| {
        used.insert(id.to_string());
        package.components.get(id).cloned().ok_or_else(||StoreError::invalid("A required resource is missing. Obtain a complete export from the source application before importing."))
    })?;
    if used.len() != package.components.len() {
        return Err(StoreError::invalid(
            "The package contains unreferenced resources.",
        ));
    }
    match &mut content {
        ItemContent::Workspace {
            history, origin, ..
        } => {
            let mapping: BTreeMap<_, _> = history
                .revisions
                .keys()
                .map(|id| (id.clone(), new_id()))
                .collect();
            history.current = mapping[&history.current].clone();
            for id in history.undo.iter_mut().chain(&mut history.redo) {
                *id = mapping[id].clone();
            }
            history.revisions = std::mem::take(&mut history.revisions)
                .into_iter()
                .map(|(id, mut revision)| {
                    revision.id = mapping[&id].clone();
                    (revision.id.clone(), revision)
                })
                .collect();
            history.generation = 0;
            if let Some(origin) = origin {
                origin.id = new_id();
                origin.version = new_id();
            }
        }
        ItemContent::Reusable { current, previous } => {
            if !previous.is_empty() {
                return Err(StoreError::invalid(
                    "Reusable exports must contain only the selected configuration.",
                ));
            }
            current.id = new_id();
        }
    }
    let mut metadata = package.metadata;
    metadata.builtin = false;
    metadata.deleted_at_ms = None;
    metadata.last_used_ms = now;
    for version in &mut metadata.previous {
        version.id = new_id();
    }
    let entity = Entity {
        id: new_id(),
        metadata,
        content,
        working: package.working,
    };
    if PackageKind::for_entity(&entity) != expected {
        return Err(StoreError::invalid(
            "The package contents do not match its type.",
        ));
    }
    entity.validate()?;
    Ok(entity)
}

impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub async fn import_reusable_package(
        &self,
        bytes: &[u8],
        kind: PackageKind,
        now: u64,
    ) -> Result<String, StoreError> {
        if kind == PackageKind::WorkspaceBackup {
            return Err(StoreError::invalid("Use Import Workspace Backup."));
        }
        self.save_reusable(import_package(bytes, kind, now)?, NamePolicy::Unique)
            .await
    }
    pub async fn import_workspace_package(
        &self,
        bytes: &[u8],
        now: u64,
    ) -> Result<StoredEntity, StoreError> {
        let entity = import_package(bytes, PackageKind::WorkspaceBackup, now)?;
        self.flush().await?;
        self.create_and_bind(entity, NamePolicy::Unique).await
    }
}
