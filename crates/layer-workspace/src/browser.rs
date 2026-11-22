//! Transactional browser storage policy. IndexedDB holds the serialized snapshot
//! and invokes this synchronous reducer inside its read/write transaction. No
//! JavaScript number arithmetic is used for fences or generation counters.
use crate::store_rules::*;
use crate::stored_workspace::{StoredHistory, StoredWorkspace};
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserDatabase {
    schema: u32,
    editing: Option<String>,
    #[serde(deserialize_with = "layer_ui::lenient::map")]
    items: BTreeMap<String, BrowserRecord>,
    fences: BTreeMap<String, String>,
    receipts: BTreeMap<String, (String, CommitReceipt)>,
    acknowledged: Vec<String>,
    pending: BTreeMap<String, serde_json::Value>,
    bindings: BTreeMap<String, String>,
    tombstones: BTreeSet<String>,
    cancelled: BTreeSet<String>,
    switcher: Option<serde_json::Value>,
    workspace_order: Option<serde_json::Value>,
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
    fn content(&self) -> Result<String> {
        Ok(serde_json::to_string(&self.entity["content"])?)
    }
    fn working(&self) -> Option<serde_json::Value> {
        Some(self.entity["working"].clone()).filter(|working| !working.is_null())
    }
    fn stored(&self, id: &str) -> Result<StoredEntity> {
        StoredEntity::read(id.into(), self.metadata()?, &self.content()?, self.working(), self.generations, self.claim.clone(), |_| Err(missing_resource()))
    }
    fn item(&self, id: &str) -> Result<Entity> {
        Entity::read(id.into(), self.metadata()?, &self.content()?, self.working(), |_| Err(missing_resource()))
    }
    fn workspace(&self) -> Result<StoredWorkspace> {
        StoredWorkspace::parse(&self.content()?)
    }
    fn set_workspace(&mut self, content: &StoredWorkspace) -> Result<()> {
        self.entity["content"] = serde_json::from_str(&content.encode()?)?;
        self.generations.layout = advance(self.generations.layout)?;
        Ok(())
    }
    fn metadata(&self) -> Result<Metadata> {
        Ok(serde_json::from_value(self.entity["metadata"].clone())?)
    }
}
impl Default for BrowserDatabase {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            editing: None,
            items: Default::default(),
            fences: Default::default(),
            receipts: Default::default(),
            acknowledged: Vec::new(),
            pending: Default::default(),
            bindings: Default::default(),
            tombstones: Default::default(),
            cancelled: Default::default(),
            switcher: None,
            workspace_order: None,
        }
    }
}
fn content(text: &str, batch: &CommitBatch) -> Result<ItemContent> {
    protocol::unpack(text, |id| {
        batch
            .components
            .get(id)
            .cloned()
            .ok_or_else(missing_resource)
    })
}
fn update_ids(
    field: &mut Option<serde_json::Value>,
    expected: Option<Vec<String>>,
    ids: Vec<String>,
    items: &BTreeMap<String, BrowserRecord>,
) -> Result<Option<Vec<String>>> {
    let current = field.clone().and_then(manager::readable_ids);
    if update_preference(current.as_ref(), expected, &ids, |id| {
        Ok(items.get(id).ok_or_else(not_found)?.metadata()?.kind == ItemKind::Workspace)
    })? {
        *field = Some(serde_json::to_value(ids)?);
    }
    Ok(field.clone().and_then(manager::readable_ids))
}
fn schema(text: &str) -> serde_json::Result<Option<u64>> {
    #[derive(Deserialize)]
    struct Schema {
        #[serde(default)]
        schema: serde_json::Value,
    }
    Ok(serde_json::from_str::<Schema>(text)?.schema.as_u64())
}
impl BrowserDatabase {
    pub fn decode(text: &str) -> Result<Self> {
        let damaged = |error: String| StoreError::new(ErrorKind::DamagedStorage, error);
        match schema(text).map_err(|error| damaged(error.to_string()))? {
            Some(schema) if schema > u64::from(SCHEMA_VERSION) => Err(newer_schema()),
            Some(schema) if schema == u64::from(SCHEMA_VERSION) => serde_json::from_str(text).map_err(|error| damaged(error.to_string())),
            schema => Err(damaged(format!("Workspace storage schema {schema:?}"))),
        }
    }
    pub fn encoded(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    pub fn release_unlocked(&mut self, live: &[String], queried_at: u64) -> bool {
        let mut released = false;
        for record in self.items.values_mut() {
            if record.claim.as_ref().is_some_and(|c| {
                !live.contains(&c.owner.id)
                    && c.expires_at_ms <= queried_at.saturating_add(OWNER_LEASE_MS)
            }) {
                record.claim = None;
                released = true;
            }
        }
        released
    }
    fn record(&self, id: &str) -> Result<&BrowserRecord> {
        self.items.get(id).ok_or_else(not_found)
    }
    fn item(&self, id: &str) -> Result<StoredEntity> {
        self.record(id)?.stored(id)
    }
    fn claim(&mut self, id: &str, owner: Owner, now: u64) -> Result<StoredEntity> {
        let record = self.record(id)?;
        let fence = claim_fence(record.claim.as_ref(), &owner, self.fence(id)?, now)?;
        let mut item = record.stored(id)?;
        item.claim = Some(Claim {
            owner,
            fence,
            expires_at_ms: now.saturating_add(OWNER_LEASE_MS),
        });
        self.fences.insert(id.into(), fence.to_string());
        self.items.get_mut(id).unwrap().claim = item.claim.clone();
        Ok(item)
    }
    fn fence(&self, id: &str) -> Result<u64> {
        self.fences.get(id).map_or(Ok(0), |s| parse_counter(s))
    }
    fn validated_batch(&self, batch: &CommitBatch) -> Result<String> {
        let hash = content_id(batch.encoded()?.as_bytes());
        if self.cancelled.contains(&batch.operation_id) {
            return Err(recovered_elsewhere());
        }
        if let Some((original, _)) = self.receipts.get(&batch.operation_id)
            && original != &hash
        {
            return Err(StoreError::invalid(
                "An operation ID cannot be reused for a different change.",
            ));
        }
        if let Some(original) = self.pending.get(&batch.operation_id)
            && original != &serde_json::to_value(batch)?
        {
            return Err(StoreError::invalid(
                "An operation ID is already bound to another payload.",
            ));
        }
        validate_payload(batch, |_| Err(missing_resource()))?;
        Ok(hash)
    }
    /// Persist this in a separate completed transaction before publication. If
    /// publication aborts or the tab disappears, the immutable delivery survives.
    pub fn prepare_delivery(&mut self, batch: &CommitBatch) -> Result<()> {
        self.validated_batch(batch)?;
        if !self.receipts.contains_key(&batch.operation_id) {
            self.pending
                .insert(batch.operation_id.clone(), serde_json::to_value(batch)?);
        }
        Ok(())
    }
    /// Failure leaves the original snapshot unchanged, including multi-item
    /// mutations, ownership changes, bindings and receipts.
    pub fn execute(&mut self, request: StoreRequest, now: u64) -> Result<StoreResponse> {
        let mut transaction = self.clone();
        let response = transaction.apply(request, now)?;
        *self = transaction;
        Ok(response)
    }
    /// Consume a transaction-local database without copying its retained history.
    /// On failure the candidate is dropped; the caller must reload its durable
    /// snapshot.
    pub fn execute_owned(mut self, request: StoreRequest, now: u64) -> Result<(Self, StoreResponse)> {
        let response = self.apply(request, now)?;
        Ok((self, response))
    }

    fn apply(&mut self, request: StoreRequest, now: u64) -> Result<StoreResponse> {
        use StoreRequest::*;
        Ok(match request {
            Editing { language } => StoreResponse::Editing(self.editing.as_ref()
                .map(|json| Box::new(layer_ui::EditingState::restore(json, &layer_ui::Localizer::shared(language))))),
            Switcher => StoreResponse::Switcher(self.switcher.clone().and_then(manager::readable_ids)),
            WorkspaceOrder => StoreResponse::WorkspaceOrder(self.workspace_order.clone().and_then(manager::readable_ids)),
            UpdateSwitcher { expected, ids } => {
                StoreResponse::Switcher(update_ids(&mut self.switcher, expected, ids, &self.items)?)
            }
            UpdateWorkspaceOrder { expected, ids } => StoreResponse::WorkspaceOrder(update_ids(
                &mut self.workspace_order,
                expected,
                ids,
                &self.items,
            )?),
            List => StoreResponse::List(
                self.items
                    .iter()
                    .filter_map(|(id, s)| {
                        let metadata = s.metadata().ok().filter(|metadata| metadata.validate().is_ok())?;
                        Some(ItemSummary { id: id.clone(), metadata, generations: s.generations, claim: s.claim.clone() })
                    })
                    .collect(),
            ),
            Load { id } => StoreResponse::Entity(Box::new(self.item(&id)?)),
            Item { id } => StoreResponse::Item(Box::new(self.record(&id)?.item(&id)?)),
            Revision { id, revision } => StoreResponse::Revision(Box::new(
                self.record(&id)?.workspace()?.revision(&revision, &mut |_| Err(missing_resource()))?,
            )),
            Versions { id } => StoreResponse::Versions(self.record(&id)?.workspace()?.versions()),
            Claim { id, owner } => StoreResponse::Entity(Box::new(self.claim(&id, owner, now)?)),
            Renew { id, owner, fence } => {
                let fence = parse_counter(&fence)?;
                check_claim(self.record(&id)?.claim.as_ref(), &owner, fence, now)?;
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
            Pending => StoreResponse::Pending(self.pending.values().filter_map(|value| serde_json::from_value(value.clone()).ok())
                .filter(|batch| validate_payload(batch, |_| Err(missing_resource())).is_ok()).collect()),
            Reopen | Adopted => StoreResponse::Done,
            SetAside => return Err(StoreError::new(ErrorKind::Unavailable,"Only persistent storage can be set aside.")),
            Maintenance { owner, clear_older } => {
                let mut items: Vec<_> = self
                    .items
                    .iter()
                    .filter(|(_, record)| record.metadata().is_ok_and(|metadata| metadata.kind == ItemKind::Workspace && metadata.validate().is_ok()))
                    .filter_map(|(id, record)| Some(retention::Retained { id: id.clone(), claim: record.claim.clone(), content: record.workspace().ok()? }))
                    .collect();
                let budget = if clear_older { 0 } else { HISTORY_BUDGET_BYTES };
                for index in retention::retention_plan(&mut items, owner.as_ref(), now, budget, |revision| revision.get().len() as u64) {
                    let item = &items[index];
                    self.items.get_mut(&item.id).unwrap().set_workspace(&item.content)?;
                }
                StoreResponse::Done
            }
        })
    }
    fn remove(&mut self, id: &str) {
        self.items.remove(id);
        self.fences.remove(id);
        self.bindings.retain(|_, v| v != id);
        self.tombstones.insert(id.into());
    }
    fn resolve_name(&self, metadata: Metadata, id: &str, policy: NamePolicy) -> Result<Metadata> {
        let kind = metadata.kind.key();
        apply_name_policy(metadata, policy, |key| {
            Ok(self.items.iter().any(|(other, s)| {
                let m = &s.entity["metadata"];
                other != id
                    && m["kind"].as_str() == Some(kind)
                    && m["name"].as_str().is_some_and(|name| name_key(name) == key)
            }))
        })
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
                return Err(already_saved());
            }
            let original = self.pending.get(id).map(|value| serde_json::from_value::<Owner>(value["owner"].clone())).transpose()?;
            if let Some(original) = original
                && original != batch.owner
                && self.items.values().any(|s| {
                    s.claim
                        .as_ref()
                        .is_some_and(|c| c.owner == original && c.expires_at_ms > now)
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
            let record = if w.create {
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
                let content = content(
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
                let entity = Entity { id: w.id.clone(), metadata, content, working };
                entity.validate()?;
                BrowserRecord {
                    entity: serde_json::to_value(entity)?,
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
                let mut record = self.record(&w.id)?.clone();
                let stored = record.metadata()?;
                check_claim(record.claim.as_ref(), &batch.owner, w.fence, now)?;
                check_update(&stored, record.generations, w)?;
                if w.delete {
                    self.remove(&w.id);
                    receipt.items.push((w.id.clone(), record.generations));
                    continue;
                }
                if let Some(m) = &w.metadata {
                    record.entity["metadata"] = serde_json::to_value(self.resolve_name(m.clone(), &w.id, w.name_policy)?)?;
                    record.generations.metadata = advance(record.generations.metadata)?;
                }
                if let Some(c) = &w.content_json {
                    let content = content(c, &batch)?;
                    if content.kind() != stored.kind {
                        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InconsistentItemType));
                    }
                    record.entity["content"] = serde_json::to_value(content)?;
                    record.generations.layout = advance(record.generations.layout)?;
                }
                if let Some(layout) = &w.layout_json {
                    let mut content = record.workspace()?;
                    content.apply(StoredHistory::parse(layout)?.resolved(|id| batch.components.get(id).cloned().ok_or_else(missing_resource))?)?;
                    record.set_workspace(&content)?;
                }
                if let Some(j) = &w.working_json {
                    record.entity["working"] = serde_json::from_str(j)?;
                    record.generations.working = advance(record.generations.working)?;
                }
                record
            };
            receipt.items.push((w.id.clone(), record.generations));
            self.items.insert(w.id.clone(), record);
        }
        if !batch.pin_workspaces.is_empty() {
            self.switcher = Some(serde_json::to_value(with_created_pins(
                self.switcher.clone().and_then(manager::readable_ids),
                &batch.pin_workspaces,
            )?)?);
        }
        for (key, id) in &batch.bindings {
            if let Some(id) = id {
                let record = self.record(id)?;
                check_binding(
                    record.metadata()?.kind == ItemKind::Workspace,
                    record.claim.as_ref(),
                    &batch.owner,
                    now,
                )?;
                self.bindings.insert(key.clone(), id.clone());
            } else {
                self.bindings.remove(key);
            }
        }
        if let Some(editing) = merge_editing(self.editing.as_deref(), &batch)? { self.editing = Some(editing); }
        self.receipts
            .insert(batch.operation_id, (hash, receipt.clone()));
        Ok(receipt)
    }
}
