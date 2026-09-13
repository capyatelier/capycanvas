//! Transactional browser storage policy. IndexedDB holds the serialized snapshot
//! and invokes this synchronous reducer inside its read/write transaction. No
//! JavaScript number arithmetic is used for fences or generation counters.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserDatabase {
    schema: u32,
    items: BTreeMap<String, BrowserRecord>,
    fences: BTreeMap<String, String>,
    components: BTreeMap<String, Vec<u8>>,
    receipts: BTreeMap<String, (String, CommitReceipt)>,
    acknowledged: Vec<String>,
    pending: BTreeMap<String, CommitBatch>,
    bindings: BTreeMap<String, String>,
    legacy_imports: BTreeMap<String, String>,
    tombstones: BTreeSet<String>,
    cancelled: BTreeSet<String>,
    #[serde(default)]
    switcher: Option<Vec<String>>,
    #[serde(default)]
    workspace_order: Option<Vec<String>>,
}
/// Keep each entity opaque until it is opened, like SQLite's JSON columns.
/// One incompatible workspace must not make the whole catalog undecodable.
/// The on-disk representation is unchanged; ownership and counters stay strict.
#[derive(Clone, Serialize, Deserialize)]
struct BrowserRecord {
    entity: serde_json::Value,
    generations: Generations,
    claim: Option<Claim>,
}
impl BrowserRecord {
    fn stored(&self) -> Result<StoredEntity> {
        Ok(StoredEntity {
            entity: serde_json::from_value(self.entity.clone())?,
            generations: self.generations,
            claim: self.claim.clone(),
        })
    }
    fn metadata(&self) -> Result<Metadata> {
        Ok(serde_json::from_value(self.entity["metadata"].clone())?)
    }
    fn from_stored(item: StoredEntity) -> Result<Self> {
        Ok(Self {
            entity: serde_json::to_value(item.entity)?,
            generations: item.generations,
            claim: item.claim,
        })
    }
}
impl Default for BrowserDatabase {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            items: Default::default(),
            fences: Default::default(),
            components: Default::default(),
            receipts: Default::default(),
            acknowledged: Vec::new(),
            pending: Default::default(),
            bindings: Default::default(),
            legacy_imports: Default::default(),
            tombstones: Default::default(),
            cancelled: Default::default(),
            switcher: None,
            workspace_order: None,
        }
    }
}
fn advance(n: u64) -> Result<u64> {
    n.checked_add(1)
        .ok_or_else(|| StoreError::invalid("Workspace generation exhausted."))
}
fn check_owner(item: &StoredEntity, owner: &Owner, fence: u64, now: u64) -> Result<()> {
    if item
        .claim
        .as_ref()
        .is_none_or(|c| &c.owner != owner || c.fence != fence || c.expires_at_ms <= now)
    {
        return Err(StoreError::conflict());
    }
    Ok(())
}
impl BrowserDatabase {
    pub fn decode(text: &str) -> Result<Self> {
        let mut value: serde_json::Value = serde_json::from_str(text)?;
        let version = value.get("schema").and_then(|v| v.as_u64());
        if !version.is_some_and(|v| (2..=SCHEMA_VERSION as u64).contains(&v)) {
            return Err(StoreError::new(
                ErrorKind::UnsupportedSchema,
                "This workspace database uses an unsupported version. Its data has been preserved.",
            ));
        }
        value["schema"] = serde_json::json!(SCHEMA_VERSION);
        Ok(serde_json::from_value(value)?)
    }
    pub fn encoded(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    fn record(&self, id: &str) -> Result<&BrowserRecord> {
        self.items.get(id).ok_or_else(|| {
            StoreError::new(
                ErrorKind::NotFound,
                "This workspace item is no longer available.",
            )
        })
    }
    fn item(&self, id: &str) -> Result<StoredEntity> {
        self.record(id)?.stored()
    }
    fn claim(
        &mut self,
        id: &str,
        owner: Owner,
        reset_invalid_default: Option<layer_ui::Platform>,
        now: u64,
    ) -> Result<StoredEntity> {
        let record = self.record(id)?;
        let live = record.claim.as_ref().filter(|c| c.expires_at_ms > now);
        if live.is_some_and(|c| c.owner != owner) {
            return Err(StoreError::new(
                ErrorKind::OwnedElsewhere,
                "This workspace is open in another window.",
            ));
        }
        let loaded = record.stored().and_then(|s| {
            s.entity.validate()?;
            Ok(s)
        });
        let reset = loaded.is_err();
        let mut item = match loaded {
            Ok(item) => item,
            Err(error) => {
                let metadata = &record.entity["metadata"];
                let replacement = reset_invalid_default
                    .filter(|_| {
                        error.kind == ErrorKind::InvalidData
                            && record.entity["id"].as_str() == Some(id)
                            && metadata["builtin"].as_bool() == Some(true)
                            && metadata["kind"].as_str() == Some("workspace")
                            && metadata["deleted_at_ms"].is_null()
                    })
                    .and_then(|platform| Entity::included_workspace(id, platform, now));
                let Some(mut entity) = replacement else {
                    return Err(error);
                };
                entity.metadata = self.resolve_name(entity.metadata, id, NamePolicy::Unique)?;
                entity.validate()?;
                StoredEntity {
                    entity,
                    generations: Generations {
                        metadata: advance(record.generations.metadata)?,
                        layout: advance(record.generations.layout)?,
                        working: advance(record.generations.working)?,
                    },
                    claim: None,
                }
            }
        };
        let fence = if !reset && live.is_some() {
            self.fence(id)?
        } else {
            advance(self.fence(id)?)?
        };
        item.claim = Some(Claim {
            owner,
            fence,
            expires_at_ms: now.saturating_add(OWNER_LEASE_MS),
        });
        self.fences.insert(id.into(), fence.to_string());
        self.items
            .insert(id.into(), BrowserRecord::from_stored(item.clone())?);
        Ok(item)
    }
    fn fence(&self, id: &str) -> Result<u64> {
        self.fences.get(id).map_or(Ok(0), |s| {
            s.parse()
                .map_err(|_| StoreError::invalid("Invalid workspace fence."))
        })
    }
    fn validated_batch(&self, batch: &CommitBatch) -> Result<String> {
        let hash = content_id(batch.encoded()?.as_bytes());
        if self.cancelled.contains(&batch.operation_id) {
            return Err(StoreError::conflict());
        }
        if let Some((original, _)) = self.receipts.get(&batch.operation_id)
            && original != &hash
        {
            return Err(StoreError::invalid(
                "An operation ID cannot be reused for a different change.",
            ));
        }
        if let Some(original) = self.pending.get(&batch.operation_id)
            && content_id(original.encoded()?.as_bytes()) != hash
        {
            return Err(StoreError::invalid(
                "An operation ID is already bound to another payload.",
            ));
        }
        for (id, bytes) in &batch.components {
            if content_id(bytes) != *id {
                return Err(StoreError::invalid("Invalid workspace component."));
            }
        }
        let mut ids = BTreeSet::new();
        for w in &batch.writes {
            if !ids.insert(&w.id) {
                return Err(StoreError::invalid(
                    "An item may only be written once in one operation.",
                ));
            }
            if let Some(m) = &w.metadata {
                m.validate()?;
                if w.metadata_json.as_deref() != Some(&serde_json::to_string(m)?) {
                    return Err(StoreError::invalid("Inconsistent metadata payload."));
                }
            }
            if let Some(c) = &w.content_json {
                self.content(c, batch)?;
            }
        }
        Ok(hash)
    }
    fn content(&self, text: &str, batch: &CommitBatch) -> Result<ItemContent> {
        protocol::unpack(text, |id| {
            batch
                .components
                .get(id)
                .or_else(|| self.components.get(id))
                .cloned()
                .ok_or_else(|| StoreError::invalid("A referenced workspace resource is missing."))
        })
    }
    /// Persist this in a separate completed transaction before publication. If
    /// publication aborts or the tab disappears, the immutable delivery survives.
    pub fn prepare_delivery(&mut self, batch: &CommitBatch) -> Result<()> {
        self.validated_batch(batch)?;
        if !self.receipts.contains_key(&batch.operation_id) {
            self.pending
                .insert(batch.operation_id.clone(), batch.clone());
        }
        Ok(())
    }
    /// Failure leaves the original snapshot unchanged, including multi-item
    /// mutations, ownership changes, migrations, bindings and receipts.
    pub fn execute(&mut self, request: StoreRequest, now: u64) -> Result<StoreResponse> {
        let mut transaction = self.clone();
        let response = transaction.apply(request, now)?;
        *self = transaction;
        Ok(response)
    }
    fn apply(&mut self, request: StoreRequest, now: u64) -> Result<StoreResponse> {
        use StoreRequest::*;
        Ok(match request {
            Switcher => StoreResponse::Switcher(self.switcher.clone()),
            WorkspaceOrder => StoreResponse::WorkspaceOrder(self.workspace_order.clone()),
            UpdateSwitcher { expected, ids } => {
                validate_switcher_ids(&ids)?;
                if self.switcher.as_ref() != Some(&ids) {
                    if self.switcher != expected {
                        return Err(StoreError::new(
                            ErrorKind::Conflict,
                            "Workspace preferences changed in another window. Try again.",
                        ));
                    }
                    for id in &ids {
                        let item = self.item(id)?;
                        if item.entity.metadata.kind != ItemKind::Workspace
                            || item.entity.metadata.deleted_at_ms.is_some()
                        {
                            return Err(StoreError::invalid(
                                "This workspace is no longer available.",
                            ));
                        }
                    }
                    self.switcher = Some(ids);
                }
                StoreResponse::Switcher(self.switcher.clone())
            }
            UpdateWorkspaceOrder { expected, ids } => {
                validate_switcher_ids(&ids)?;
                if self.workspace_order.as_ref() != Some(&ids) {
                    if self.workspace_order != expected {
                        return Err(StoreError::new(
                            ErrorKind::Conflict,
                            "Workspace preferences changed in another window. Try again.",
                        ));
                    }
                    for id in &ids {
                        let item = self.item(id)?;
                        if item.entity.metadata.kind != ItemKind::Workspace
                            || item.entity.metadata.deleted_at_ms.is_some()
                        {
                            return Err(StoreError::invalid(
                                "This workspace is no longer available.",
                            ));
                        }
                    }
                    self.workspace_order = Some(ids);
                }
                StoreResponse::WorkspaceOrder(self.workspace_order.clone())
            }
            List => StoreResponse::List(
                self.items
                    .iter()
                    .map(|(id, s)| {
                        let metadata = s.metadata().and_then(|m| {
                            m.validate()?;
                            Ok(m)
                        });
                        let error =
                            metadata
                                .as_ref()
                                .err()
                                .map(ToString::to_string)
                                .or_else(|| {
                                    s.stored()
                                        .and_then(|s| s.entity.validate())
                                        .err()
                                        .map(|e| e.to_string())
                                });
                        ItemSummary {
                            id: id.clone(),
                            metadata: metadata.unwrap_or_else(|_| {
                                let kind = match s.entity["metadata"]["kind"].as_str() {
                                    Some("template") => ItemKind::Template,
                                    Some("toolbar") => ItemKind::Toolbar,
                                    _ => ItemKind::Workspace,
                                };
                                let mut metadata = Metadata::new(
                                    kind,
                                    s.entity["metadata"]["name"]
                                        .as_str()
                                        .unwrap_or("Unreadable workspace"),
                                    0,
                                );
                                metadata.builtin =
                                    s.entity["metadata"]["builtin"].as_bool() == Some(true);
                                metadata
                            }),
                            generations: s.generations,
                            claim: s.claim.clone(),
                            error,
                        }
                    })
                    .collect(),
            ),
            Load { id } => {
                let s = self.item(&id)?;
                s.entity.validate()?;
                StoreResponse::Entity(Box::new(s))
            }
            Raw { id } => StoreResponse::Raw(serde_json::to_string(self.record(&id)?)?),
            Claim {
                id,
                owner,
                reset_invalid_default,
            } => StoreResponse::Entity(Box::new(self.claim(
                &id,
                owner,
                reset_invalid_default,
                now,
            )?)),
            Renew { id, owner, fence } => {
                let fence = fence
                    .parse()
                    .map_err(|_| StoreError::invalid("Invalid workspace fence."))?;
                check_owner(&self.item(&id)?, &owner, fence, now)?;
                let claim = crate::Claim {
                    owner,
                    fence,
                    expires_at_ms: now.saturating_add(OWNER_LEASE_MS),
                };
                self.items.get_mut(&id).unwrap().claim = Some(claim.clone());
                StoreResponse::Claim(claim)
            }
            Release { id, owner, fence } => {
                if let Some(s) = self.items.get_mut(&id)
                    && s.claim
                        .as_ref()
                        .is_some_and(|c| c.owner == owner && c.fence.to_string() == fence)
                {
                    s.claim = None;
                }
                StoreResponse::Done
            }
            Commit { batch } => StoreResponse::Committed(self.commit(batch, now)?),
            Receipt { operation_id } => {
                StoreResponse::Receipt(self.receipts.get(&operation_id).map(|(_, r)| r.clone()))
            }
            Acknowledge { operation_id } => {
                if self.receipts.contains_key(&operation_id) {
                    self.pending.remove(&operation_id);
                    self.acknowledged.retain(|id| id != &operation_id);
                    self.acknowledged.push(operation_id);
                    while self.acknowledged.len() > 256 {
                        let id = self.acknowledged.remove(0);
                        self.receipts.remove(&id);
                    }
                }
                StoreResponse::Done
            }
            Binding { key } => StoreResponse::Binding(self.bindings.get(&key).cloned()),
            LegacyImport { source } => {
                StoreResponse::Binding(self.legacy_imports.get(&source).cloned())
            }
            Pending => StoreResponse::Pending(self.pending.values().cloned().collect()),
            Reopen => StoreResponse::Done,
            DeletePermanently { id, owner, fence } => {
                let item = self.item(&id)?;
                check_owner(
                    &item,
                    &owner,
                    fence.parse().map_err(|_| StoreError::conflict())?,
                    now,
                )?;
                if item.entity.metadata.builtin || item.entity.metadata.deleted_at_ms.is_none() {
                    return Err(StoreError::invalid(
                        "Only deleted user items can be permanently removed.",
                    ));
                }
                self.remove(&id);
                StoreResponse::Done
            }
            Maintenance {
                owner,
                clear_older,
                apply,
            } => {
                let items: Vec<_> = self
                    .items
                    .values()
                    .filter_map(|r| r.stored().ok().filter(|s| s.entity.validate().is_ok()))
                    .collect();
                let plan = retention::retention_plan(
                    &items,
                    owner.as_ref(),
                    now,
                    if clear_older {
                        0
                    } else {
                        HISTORY_BUDGET_BYTES as usize
                    },
                )?;
                let mut report = plan.report;
                report.database_bytes = self.encoded()?.len() as u64;
                report.component_bytes = self.components.values().map(|v| v.len() as u64).sum();
                if apply {
                    // execute() publishes this cloned transaction only on success.
                    // Vacate all renamed labels before ordinary collision resolution.
                    for id in &plan.renamed {
                        self.items.get_mut(id).unwrap().entity["metadata"]["name"] =
                            serde_json::json!(format!("\0catalog:{id}"));
                    }
                    for mut entity in plan.changed {
                        entity.metadata =
                            self.resolve_name(entity.metadata, &entity.id, NamePolicy::Unique)?;
                        let s = self.items.get_mut(&entity.id).unwrap();
                        let old = s.stored()?;
                        if old.entity.metadata != entity.metadata {
                            s.generations.metadata = advance(s.generations.metadata)?;
                        }
                        if old.entity.content != entity.content {
                            s.generations.layout = advance(s.generations.layout)?;
                        }
                        s.entity = serde_json::to_value(entity)?;
                    }
                    for id in plan.expired {
                        self.remove(&id);
                    }
                    // Item content is stored expanded. Only interrupted deliveries
                    // still reference the interned components.
                    self.components.clear();
                    for batch in self.pending.values() {
                        self.components.extend(batch.components.clone());
                    }
                }
                StoreResponse::Storage(report)
            }
        })
    }
    fn remove(&mut self, id: &str) {
        self.items.remove(id);
        self.fences.remove(id);
        self.bindings.retain(|_, v| v != id);
        self.tombstones.insert(id.into());
    }
    fn resolve_name(
        &self,
        mut metadata: Metadata,
        id: &str,
        policy: NamePolicy,
    ) -> Result<Metadata> {
        if metadata.deleted_at_ms.is_some() {
            return Ok(metadata);
        }
        let exists = |key: &str| {
            Ok(self.items.iter().any(|(other, s)| {
                let m = &s.entity["metadata"];
                other != id
                    && m["kind"].as_str() == Some(metadata.kind.key())
                    && m["deleted_at_ms"].is_null()
                    && m["name"].as_str().is_some_and(|name| name_key(name) == key)
            }))
        };
        if policy == NamePolicy::Unique {
            metadata.name = available_name(&metadata.name, exists)?;
        } else if exists(&name_key(&metadata.name))? {
            return Err(StoreError::new(
                ErrorKind::NameCollision,
                "An item with this name already exists. Choose another name.",
            ));
        }
        Ok(metadata)
    }
    fn commit(&mut self, batch: CommitBatch, now: u64) -> Result<CommitReceipt> {
        let hash = self.validated_batch(&batch)?;
        if let Some((_, receipt)) = self.receipts.get(&batch.operation_id) {
            return Ok(receipt.clone());
        }
        for id in &batch.abandon_operations {
            if id == &batch.operation_id || id.len() > 128 {
                return Err(StoreError::invalid("Invalid recovery delivery."));
            }
            if self.receipts.contains_key(id) {
                return Err(StoreError::conflict());
            }
            if let Some(original) = self.pending.get(id)
                && original.owner != batch.owner
                && self.items.values().any(|s| {
                    s.claim
                        .as_ref()
                        .is_some_and(|c| c.owner == original.owner && c.expires_at_ms > now)
                })
            {
                return Err(StoreError::new(
                    ErrorKind::OwnedElsewhere,
                    "The source window is still saving these changes.",
                ));
            }
            self.cancelled.insert(id.clone());
            self.pending.remove(id);
        }
        let mut receipt = CommitReceipt {
            operation_id: batch.operation_id.clone(),
            items: Vec::new(),
        };
        for w in &batch.writes {
            let mut s = if w.create {
                if self.items.contains_key(&w.id) || self.tombstones.contains(&w.id) {
                    return Err(StoreError::conflict());
                }
                let metadata = self.resolve_name(
                    w.metadata
                        .clone()
                        .ok_or_else(|| StoreError::invalid("A new item needs metadata."))?,
                    &w.id,
                    w.name_policy,
                )?;
                let content = self.content(
                    w.content_json
                        .as_ref()
                        .ok_or_else(|| StoreError::invalid("A new item needs content."))?,
                    &batch,
                )?;
                let working = w
                    .working_json
                    .as_ref()
                    .map(|j| serde_json::from_str(j))
                    .transpose()?;
                self.fences
                    .insert(w.id.clone(), if w.claim { "1" } else { "0" }.into());
                StoredEntity {
                    entity: Entity {
                        id: w.id.clone(),
                        metadata,
                        content,
                        working,
                    },
                    generations: Generations {
                        metadata: 1,
                        layout: 1,
                        working: u64::from(w.working_json.is_some()),
                    },
                    claim: w.claim.then(|| crate::Claim {
                        owner: batch.owner.clone(),
                        fence: 1,
                        expires_at_ms: now.saturating_add(OWNER_LEASE_MS),
                    }),
                }
            } else {
                let mut s = self.item(&w.id)?;
                check_owner(&s, &batch.owner, w.fence, now)?;
                if s.entity.metadata.builtin && s.entity.metadata.kind != ItemKind::Workspace {
                    return Err(StoreError::invalid("Included layouts cannot be modified."));
                }
                if w.metadata.is_some() && w.expected.metadata != s.generations.metadata
                    || w.content_json.is_some() && w.expected.layout != s.generations.layout
                    || w.working_json.is_some() && w.expected.working != s.generations.working
                {
                    return Err(StoreError::conflict());
                }
                if let Some(m) = &w.metadata {
                    if s.entity.metadata.builtin && m.name != s.entity.metadata.name {
                        return Err(StoreError::invalid(
                            "Included workspaces cannot be renamed.",
                        ));
                    }
                    if s.entity.metadata.builtin && m.deleted_at_ms.is_some() {
                        return Err(StoreError::invalid(
                            "Included workspaces cannot be deleted.",
                        ));
                    }
                    if m.kind != s.entity.metadata.kind || m.builtin != s.entity.metadata.builtin {
                        return Err(StoreError::invalid("Item type cannot be changed."));
                    }
                    s.entity.metadata = self.resolve_name(m.clone(), &w.id, w.name_policy)?;
                    s.generations.metadata = advance(s.generations.metadata)?;
                }
                if let Some(c) = &w.content_json {
                    s.entity.content = self.content(c, &batch)?;
                    s.generations.layout = advance(s.generations.layout)?;
                }
                if let Some(j) = &w.working_json {
                    s.entity.working = Some(serde_json::from_str(j)?);
                    s.generations.working = advance(s.generations.working)?;
                }
                s
            };
            s.entity.validate()?;
            if s.entity.metadata.deleted_at_ms.is_some() {
                s.claim = None;
                self.fences
                    .insert(w.id.clone(), advance(self.fence(&w.id)?)?.to_string());
            }
            receipt.items.push((w.id.clone(), s.generations));
            self.items
                .insert(w.id.clone(), BrowserRecord::from_stored(s)?);
        }
        if !batch.pin_workspaces.is_empty() {
            self.switcher = Some(with_created_pins(
                self.switcher.clone(),
                &batch.pin_workspaces,
            )?);
        }
        for (key, id) in &batch.bindings {
            if let Some(id) = id {
                let s = self.item(id)?;
                if s.entity.metadata.kind != ItemKind::Workspace
                    || s.entity.metadata.deleted_at_ms.is_some()
                {
                    return Err(StoreError::invalid(
                        "The replacement workspace is unavailable.",
                    ));
                }
                check_owner(
                    &s,
                    &batch.owner,
                    s.claim.as_ref().ok_or_else(StoreError::conflict)?.fence,
                    now,
                )?;
                self.bindings.insert(key.clone(), id.clone());
            } else {
                self.bindings.remove(key);
            }
        }
        for (source, id) in &batch.legacy_imports {
            self.item(id)?;
            if self
                .legacy_imports
                .insert(source.clone(), id.clone())
                .is_some()
            {
                return Err(StoreError::conflict());
            }
        }
        self.components.extend(batch.components);
        self.receipts
            .insert(batch.operation_id, (hash, receipt.clone()));
        Ok(receipt)
    }
}
