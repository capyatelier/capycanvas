//! Transactional browser storage policy. IndexedDB holds the serialized snapshot
//! and invokes this synchronous reducer inside its read/write transaction. No
//! JavaScript number arithmetic is used for fences or generation counters.
use crate::store_rules::*;
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
    receipts: BTreeMap<String, (String, CommitReceipt)>,
    acknowledged: Vec<String>,
    pending: BTreeMap<String, CommitBatch>,
    bindings: BTreeMap<String, String>,
    tombstones: BTreeSet<String>,
    cancelled: BTreeSet<String>,
    switcher: Option<Vec<String>>,
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
    field: &mut Option<Vec<String>>,
    expected: Option<Vec<String>>,
    ids: Vec<String>,
    items: &BTreeMap<String, BrowserRecord>,
) -> Result<Option<Vec<String>>> {
    if update_preference(field.as_ref(), expected, &ids, |id| {
        Ok(items.get(id).ok_or_else(not_found)?.metadata()?.kind == ItemKind::Workspace)
    })? {
        *field = Some(ids);
    }
    Ok(field.clone())
}
impl BrowserDatabase {
    /// An unversioned or older snapshot decodes as an empty current database,
    /// like a first start; the next write replaces it (see newer_schema).
    pub fn decode(text: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let current = u64::from(SCHEMA_VERSION);
        match value.get("schema").and_then(|v| v.as_u64()) {
            Some(schema) if schema > current => Err(newer_schema()),
            Some(schema) if schema == current => Ok(serde_json::from_value(value)?),
            _ => Ok(Self::default()),
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
        self.record(id)?.stored()
    }
    fn claim(&mut self, id: &str, owner: Owner, now: u64) -> Result<StoredEntity> {
        let record = self.record(id)?;
        let fence = claim_fence(record.claim.as_ref(), &owner, self.fence(id)?, now)?;
        let mut item = record.stored()?;
        item.entity.validate()?;
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
            && content_id(original.encoded()?.as_bytes()) != hash
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
                .insert(batch.operation_id.clone(), batch.clone());
        }
        Ok(())
    }
    /// Failure leaves the original snapshot unchanged, including multi-item
    /// mutations, ownership changes, bindings and receipts.
    #[cfg(test)]
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
            Switcher => StoreResponse::Switcher(self.switcher.clone()),
            WorkspaceOrder => StoreResponse::WorkspaceOrder(self.workspace_order.clone()),
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
                        match metadata {
                            Ok(metadata) => ItemSummary {
                                id: id.clone(),
                                metadata,
                                generations: s.generations,
                                claim: s.claim.clone(),
                                error,
                            },
                            Err(error) => summary_fallback(
                                id.clone(),
                                s.entity["metadata"]["kind"].as_str(),
                                s.entity["metadata"]["name"]
                                    .as_str()
                                    .unwrap_or("Unreadable workspace"),
                                s.entity["metadata"]["builtin"].as_bool() == Some(true),
                                s.generations,
                                s.claim.clone(),
                                error.to_string(),
                            ),
                        }
                    })
                    .collect(),
            ),
            Load { id } => {
                let s = self.item(&id)?;
                s.entity.validate()?;
                StoreResponse::Entity(Box::new(s))
            }
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
            Pending => StoreResponse::Pending(self.pending.values().cloned().collect()),
            Reopen => StoreResponse::Done,
            Maintenance { owner, clear_older } => {
                let items: Vec<_> = self
                    .items
                    .values()
                    .filter_map(|r| r.stored().ok().filter(|s| s.entity.validate().is_ok()))
                    .collect();
                let changed = retention::retention_plan(
                    &items,
                    owner.as_ref(),
                    now,
                    if clear_older {
                        0
                    } else {
                        HISTORY_BUDGET_BYTES as usize
                    },
                )?;
                for entity in changed {
                    let s = self.items.get_mut(&entity.id).unwrap();
                    if s.stored()?.entity.content != entity.content {
                        s.generations.layout = advance(s.generations.layout)?;
                    }
                    s.entity = serde_json::to_value(entity)?;
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
            let s = if w.create {
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
                check_claim(s.claim.as_ref(), &batch.owner, w.fence, now)?;
                check_update(&s.entity.metadata, s.generations, w)?;
                if w.delete {
                    self.remove(&w.id);
                    receipt.items.push((w.id.clone(), s.generations));
                    continue;
                }
                if let Some(m) = &w.metadata {
                    s.entity.metadata = self.resolve_name(m.clone(), &w.id, w.name_policy)?;
                    s.generations.metadata = advance(s.generations.metadata)?;
                }
                if let Some(c) = &w.content_json {
                    s.entity.content = content(c, &batch)?;
                    s.generations.layout = advance(s.generations.layout)?;
                }
                if let Some(j) = &w.working_json {
                    s.entity.working = Some(serde_json::from_str(j)?);
                    s.generations.working = advance(s.generations.working)?;
                }
                s
            };
            s.entity.validate()?;
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
                check_binding(
                    s.entity.metadata.kind == ItemKind::Workspace,
                    s.claim.as_ref(),
                    &batch.owner,
                    now,
                )?;
                self.bindings.insert(key.clone(), id.clone());
            } else {
                self.bindings.remove(key);
            }
        }
        self.receipts
            .insert(batch.operation_id, (hash, receipt.clone()));
        Ok(receipt)
    }
}
