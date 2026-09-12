use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// One asynchronous record protocol; native and browser implementations provide
/// transport without duplicating application decisions or workspace semantics.
#[allow(async_fn_in_trait)]
pub trait WorkspaceStore {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError>;
}

pub fn content_id(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamePolicy {
    #[default]
    Exact,
    Unique,
}
pub fn available_name(
    stem: &str,
    mut exists: impl FnMut(&str) -> Result<bool, StoreError>,
) -> Result<String, StoreError> {
    validate_name(stem)?;
    if !exists(&name_key(stem))? {
        return Ok(stem.into());
    }
    for n in 2..100_000 {
        let suffix = format!(" ({n})");
        let prefix: String = stem.chars().take(100 - suffix.len()).collect();
        let candidate = format!("{prefix}{suffix}");
        if !exists(&name_key(&candidate))? {
            return Ok(candidate);
        }
    }
    Err(StoreError::invalid("Too many items have this name."))
}

#[derive(Clone, Debug)]
pub enum Mutation {
    Create {
        entity: Entity,
        claim: bool,
        name_policy: NamePolicy,
    },
    Update {
        id: String,
        generations: Generations,
        fence: u64,
        metadata: Option<Metadata>,
        content: Option<ItemContent>,
        working: Option<layer_ui::WorkspaceWorkingState>,
        name_policy: NamePolicy,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PreparedWrite {
    pub id: String,
    pub create: bool,
    pub claim: bool,
    pub name_policy: NamePolicy,
    pub expected: Generations,
    #[serde(with = "model::counter")]
    pub fence: u64,
    pub metadata: Option<Metadata>,
    pub metadata_json: Option<String>,
    pub content_json: Option<String>,
    pub working_json: Option<String>,
}
/// Immutable validated bytes are prepared before the backend opens a transaction.
/// The browser can perform the complete comparison/write without a Rust round trip.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitBatch {
    pub operation_id: String,
    pub owner: Owner,
    pub(crate) writes: Vec<PreparedWrite>,
    pub(crate) components: BTreeMap<String, Vec<u8>>,
    pub bindings: Vec<(String, Option<String>)>,
    pub legacy_imports: Vec<(String, String)>,
}
impl CommitBatch {
    pub fn prepare(owner: Owner, mutations: Vec<Mutation>) -> Result<Self, StoreError> {
        let mut batch = Self {
            operation_id: new_id(),
            owner,
            writes: Vec::new(),
            components: BTreeMap::new(),
            bindings: Vec::new(),
            legacy_imports: Vec::new(),
        };
        for mutation in mutations {
            let (id, create, claim, expected, fence, metadata, content, working, name_policy) =
                match mutation {
                    Mutation::Create {
                        entity,
                        claim,
                        name_policy,
                    } => {
                        entity.validate()?;
                        (
                            entity.id,
                            true,
                            claim,
                            Generations::default(),
                            0,
                            Some(entity.metadata),
                            Some(entity.content),
                            entity.working,
                            name_policy,
                        )
                    }
                    Mutation::Update {
                        id,
                        generations,
                        fence,
                        metadata,
                        content,
                        working,
                        name_policy,
                    } => (
                        id,
                        false,
                        false,
                        generations,
                        fence,
                        metadata,
                        content,
                        working,
                        name_policy,
                    ),
                };
            if let Some(metadata) = &metadata {
                metadata.validate()?;
            }
            if let Some(content) = &content {
                content.validate()?;
            }
            if let Some(working) = &working {
                layer_ui::WorkspaceCapture {
                    history: layer_ui::LayoutHistory::new(&layer_ui::DockLayout::default()),
                    working: working.clone(),
                }
                .validate()
                .map_err(StoreError::invalid)?;
            }
            let metadata_json = metadata.as_ref().map(serde_json::to_string).transpose()?;
            let content_json = content
                .map(|c| pack(serde_json::to_value(c)?, &mut batch.components))
                .transpose()?
                .map(|c| serde_json::to_string(&c))
                .transpose()?;
            let working_json = working.as_ref().map(serde_json::to_string).transpose()?;
            if batch.writes.iter().any(|w| w.id == id) {
                return Err(StoreError::invalid(
                    "An item may only be written once in one operation.",
                ));
            }
            batch.writes.push(PreparedWrite {
                id,
                create,
                claim,
                expected,
                fence,
                metadata,
                metadata_json,
                content_json,
                working_json,
                name_policy,
            });
        }
        Ok(batch)
    }
    pub fn encoded(&self) -> Result<String, StoreError> {
        let text = serde_json::to_string(self)?;
        if text.len() > MAX_PACKAGE_BYTES {
            return Err(StoreError::invalid(
                "Workspace operation exceeds the supported size.",
            ));
        }
        Ok(text)
    }
}
fn intern(value: Value, components: &mut BTreeMap<String, Vec<u8>>) -> Result<Value, StoreError> {
    let bytes = serde_json::to_vec(&value)?;
    let id = content_id(&bytes);
    components.entry(id.clone()).or_insert(bytes);
    Ok(serde_json::json!({ "$workspace_component": id }))
}
fn pack(value: Value, components: &mut BTreeMap<String, Vec<u8>>) -> Result<Value, StoreError> {
    match value {
        Value::Object(mut object) => {
            if object.contains_key("bands")
                && object.contains_key("panels")
                && object.contains_key("next_id")
            {
                if let Some(Value::Array(panels)) = object.get_mut("panels") {
                    for panel in panels {
                        *panel = intern(panel.take(), components)?;
                    }
                }
                return intern(Value::Object(object), components);
            }
            if let Some(definition) = object.get_mut("definition") {
                *definition = intern(definition.take(), components)?;
            }
            for value in object.values_mut() {
                *value = pack(value.take(), components)?;
            }
            Ok(Value::Object(object))
        }
        Value::Array(values) => Ok(Value::Array(
            values
                .into_iter()
                .map(|v| pack(v, components))
                .collect::<Result<_, _>>()?,
        )),
        value => Ok(value),
    }
}
pub(crate) fn unpack(
    text: &str,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, StoreError>,
) -> Result<ItemContent, StoreError> {
    fn resolve(
        value: Value,
        read: &mut impl FnMut(&str) -> Result<Vec<u8>, StoreError>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Value, StoreError> {
        if depth > 96 {
            return Err(StoreError::invalid(
                "Workspace content is nested too deeply.",
            ));
        }
        match value {
            Value::Object(mut object) => {
                if object.len() == 1
                    && let Some(id) = object.get("$workspace_component").and_then(Value::as_str)
                {
                    let bytes = read(id)?;
                    *remaining = remaining.checked_sub(bytes.len()).ok_or_else(|| {
                        StoreError::invalid("Workspace content exceeds supported limits.")
                    })?;
                    if content_id(&bytes) != id {
                        return Err(StoreError::invalid("A workspace resource is corrupt."));
                    }
                    return resolve(serde_json::from_slice(&bytes)?, read, depth + 1, remaining);
                }
                for v in object.values_mut() {
                    *v = resolve(v.take(), read, depth + 1, remaining)?;
                }
                Ok(Value::Object(object))
            }
            Value::Array(values) => Ok(Value::Array(
                values
                    .into_iter()
                    .map(|v| resolve(v, read, depth + 1, remaining))
                    .collect::<Result<_, _>>()?,
            )),
            v => Ok(v),
        }
    }
    if text.len() > MAX_PACKAGE_BYTES {
        return Err(StoreError::invalid(
            "Workspace content exceeds supported limits.",
        ));
    }
    let mut remaining = MAX_PACKAGE_BYTES;
    let value = resolve(serde_json::from_str(text)?, &mut read, 0, &mut remaining)?;
    let content: ItemContent = serde_json::from_value(value)?;
    content.validate()?;
    Ok(content)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitReceipt {
    pub operation_id: String,
    pub items: Vec<(String, Generations)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoreRequest {
    List,
    Load {
        id: String,
    },
    Claim {
        id: String,
        owner: Owner,
    },
    Renew {
        id: String,
        owner: Owner,
        fence: String,
    },
    Release {
        id: String,
        owner: Owner,
        fence: String,
    },
    Commit {
        batch: CommitBatch,
    },
    Acknowledge {
        operation_id: String,
    },
    Receipt {
        operation_id: String,
    },
    Binding {
        key: String,
    },
    LegacyImport {
        source: String,
    },
    Pending,
    Raw {
        id: String,
    },
    Reopen,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum StoreResponse {
    List(Vec<ItemSummary>),
    Entity(StoredEntity),
    Claim(Claim),
    Committed(CommitReceipt),
    Receipt(Option<CommitReceipt>),
    Binding(Option<String>),
    Pending(Vec<CommitBatch>),
    Raw(String),
    Done,
}
