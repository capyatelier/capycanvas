use crate::*;
use layer_ui::{DockLayout, Platform, PreparedWorkspace, WorkspaceCapture, WorkspaceWorkingState};
use std::cell::{Cell, RefCell};

type Result<T> = std::result::Result<T, StoreError>;
#[derive(Clone)]
struct PendingSave {
    batch: CommitBatch,
    snapshot: Entity,
}
#[derive(Default)]
struct State {
    saved: Option<StoredEntity>,
    latest: Option<Entity>,
    pending: Option<PendingSave>,
    failed_operation: Option<CommitBatch>,
    older_failed_operations: Vec<CommitBatch>,
    items: Vec<ItemSummary>,
    error: Option<StoreError>,
    error_operation: Option<String>,
}
/// Called from one UI/input owner. RefCell borrows never span storage awaits;
/// accepted live edits can continue while an immutable save is in flight.
pub struct WorkspaceManager<S: WorkspaceStore> {
    pub store: S,
    pub owner: Owner,
    pub platform: Platform,
    state: RefCell<State>,
    saving: Cell<bool>,
    transitioning: Cell<bool>,
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    pub fn new(store: S, platform: Platform) -> Self {
        Self {
            store,
            owner: Owner::fresh(),
            platform,
            state: RefCell::new(State::default()),
            saving: Cell::new(false),
            transitioning: Cell::new(false),
        }
    }
    pub fn active_id(&self) -> Option<String> {
        self.state.borrow().latest.as_ref().map(|e| e.id.clone())
    }
    pub fn active_name(&self) -> Option<String> {
        self.state
            .borrow()
            .latest
            .as_ref()
            .map(|e| e.metadata.name.clone())
    }
    pub fn current(&self) -> Option<Entity> {
        self.state.borrow().latest.clone()
    }
    pub fn current_record(&self) -> Option<StoredEntity> {
        let state = self.state.borrow();
        let mut saved = state.saved.clone()?;
        saved.entity = state.latest.clone()?;
        Some(saved)
    }
    pub fn items(&self) -> Vec<ItemSummary> {
        self.state.borrow().items.clone()
    }
    pub fn binding(&self) -> Option<layer_ui::ManagedWorkspace> {
        let current = self.current()?;
        let ItemContent::Workspace { baseline, .. } = current.content else {
            return None;
        };
        let mut items = self.items();
        items.retain(|i| {
            i.metadata.kind == ItemKind::Workspace && i.metadata.deleted_at_ms.is_none()
        });
        items.sort_by_key(|i| std::cmp::Reverse(i.metadata.last_used_ms));
        Some(layer_ui::ManagedWorkspace {
            id: current.id,
            name: current.metadata.name,
            baseline,
            choices: items
                .into_iter()
                .map(|i| layer_ui::WorkspaceChoice {
                    id: i.id,
                    name: i.metadata.name,
                })
                .collect(),
        })
    }
    pub fn error(&self) -> Option<StoreError> {
        self.state.borrow().error.clone()
    }
    pub fn set_error(&self, error: StoreError) {
        let mut state = self.state.borrow_mut();
        state.error = Some(error);
        state.error_operation = None;
    }
    pub fn saving(&self) -> bool {
        self.saving.get()
    }
    pub fn transitioning(&self) -> bool {
        self.transitioning.get()
    }
    pub fn dirty(&self) -> bool {
        let s = self.state.borrow();
        match (&s.saved, &s.latest) {
            (Some(saved), Some(latest)) => &saved.entity != latest || s.pending.is_some(),
            _ => false,
        }
    }
    pub fn observe_working(&self, working: WorkspaceWorkingState) {
        if let Some(entity) = &mut self.state.borrow_mut().latest {
            entity.working = Some(working);
        }
    }
    pub fn observe(&self, capture: WorkspaceCapture, now: u64) {
        if let Some(entity) = &mut self.state.borrow_mut().latest {
            if let ItemContent::Workspace { history, .. } = &mut entity.content {
                let mut incoming = capture.history;
                for (id, revision) in &mut incoming.revisions {
                    if revision.timestamp_ms == 0 {
                        revision.timestamp_ms = history
                            .revisions
                            .get(id)
                            .filter(|r| r.timestamp_ms != 0)
                            .map_or(now, |r| r.timestamp_ms);
                    }
                }
                *history = incoming;
            }
            entity.working = Some(capture.working);
        }
    }
    pub fn activate(&self, incoming: StoredEntity) -> Option<StoredEntity> {
        let mut s = self.state.borrow_mut();
        let outgoing = s.saved.take();
        s.latest = Some(incoming.entity.clone());
        s.saved = Some(incoming);
        s.pending = None;
        s.error = None;
        s.error_operation = None;
        self.transitioning.set(false);
        outgoing
    }
    pub fn finish_transition(&self) {
        self.transitioning.set(false);
    }
    pub fn begin_transition(&self) -> Result<()> {
        if self.transitioning.replace(true) {
            return Err(StoreError::new(
                ErrorKind::Conflict,
                "A workspace change is already in progress.",
            ));
        }
        Ok(())
    }
    async fn publish(&self, batch: CommitBatch) -> Result<CommitReceipt> {
        let id = batch.operation_id.clone();
        let response = match self
            .store
            .execute(StoreRequest::Commit {
                batch: batch.clone(),
            })
            .await
        {
            Ok(response) => response,
            Err(error) => {
                if let Ok(StoreResponse::Receipt(Some(receipt))) = self
                    .store
                    .execute(StoreRequest::Receipt {
                        operation_id: id.clone(),
                    })
                    .await
                {
                    StoreResponse::Committed(receipt)
                } else {
                    let mut state = self.state.borrow_mut();
                    if matches!(
                        error.kind,
                        ErrorKind::Unavailable
                            | ErrorKind::FailedWrite
                            | ErrorKind::StorageFull
                            | ErrorKind::Conflict
                    ) && batch
                        .writes
                        .iter()
                        .any(|write| write.id != DEFAULT_TEMPLATE_ID)
                        && state
                            .pending
                            .as_ref()
                            .is_none_or(|p| p.batch.operation_id != id)
                    {
                        if let Some(previous) = state.failed_operation.take()
                            && previous.operation_id != id
                        {
                            state.older_failed_operations.retain(|b| {
                                b.operation_id != previous.operation_id && b.operation_id != id
                            });
                            state.older_failed_operations.push(previous);
                        }
                        state.failed_operation = Some(batch);
                        state.error = Some(error.clone());
                        state.error_operation = Some(id);
                    }
                    return Err(error);
                }
            }
        };
        let StoreResponse::Committed(receipt) = response else {
            return Err(StoreError::invalid("Unexpected workspace commit reply."));
        };
        // A lost acknowledgement leaves delivery bookkeeping for later cleanup;
        // it does not turn a successfully committed operation into a failed save.
        let _ = self
            .store
            .execute(StoreRequest::Acknowledge { operation_id: id })
            .await;
        let mut state = self.state.borrow_mut();
        if state
            .failed_operation
            .as_ref()
            .is_some_and(|b| b.operation_id == receipt.operation_id)
        {
            state.failed_operation = None;
        }
        state
            .older_failed_operations
            .retain(|b| b.operation_id != receipt.operation_id);
        if state.error_operation.as_deref() == Some(&receipt.operation_id) {
            state.error = None;
            state.error_operation = None;
        }
        Ok(receipt)
    }
    pub fn has_failed_operation(&self) -> bool {
        let state = self.state.borrow();
        state.failed_operation.is_some() || !state.older_failed_operations.is_empty()
    }
    pub async fn retry_failed_operation(&self) -> Result<Option<StoredEntity>> {
        let batch = {
            let state = self.state.borrow();
            state
                .failed_operation
                .clone()
                .or_else(|| state.older_failed_operations.last().cloned())
        };
        let Some(batch) = batch else {
            return Ok(None);
        };
        self.flush().await?;
        let incoming = batch
            .bindings
            .iter()
            .find(|(key, _)| key == &format!("window:{}", self.owner.id))
            .and_then(|(_, id)| id.clone())
            .or_else(|| self.active_id());
        self.publish(batch).await?;
        self.refresh().await?;
        match incoming {
            Some(id) => self.claim(&id).await.map(Some),
            None => Ok(None),
        }
    }
    pub async fn refresh(&self) -> Result<()> {
        let StoreResponse::List(items) = self.store.execute(StoreRequest::List).await? else {
            return Err(StoreError::invalid("Unexpected workspace list reply."));
        };
        self.state.borrow_mut().items = items;
        Ok(())
    }
    pub async fn load(&self, id: &str) -> Result<StoredEntity> {
        match self
            .store
            .execute(StoreRequest::Load { id: id.into() })
            .await?
        {
            StoreResponse::Entity(entity) => Ok(entity),
            _ => Err(StoreError::invalid("Unexpected workspace load reply.")),
        }
    }
    pub(crate) async fn claim(&self, id: &str) -> Result<StoredEntity> {
        match self
            .store
            .execute(StoreRequest::Claim {
                id: id.into(),
                owner: self.owner.clone(),
            })
            .await?
        {
            StoreResponse::Entity(entity) => Ok(entity),
            _ => Err(StoreError::invalid("Unexpected workspace claim reply.")),
        }
    }
    pub async fn release(&self, entity: &StoredEntity) {
        if let Some(claim) = &entity.claim {
            let _ = self
                .store
                .execute(StoreRequest::Release {
                    id: entity.entity.id.clone(),
                    owner: self.owner.clone(),
                    fence: claim.fence.to_string(),
                })
                .await;
        }
    }
    pub async fn initialize(&self, now: u64) -> Result<StoredEntity> {
        if self.has_failed_operation()
            && let Some(incoming) = self.retry_failed_operation().await?
        {
            return Ok(incoming);
        }
        self.store
            .execute(StoreRequest::Maintenance {
                owner: None,
                clear_older: false,
                apply: true,
            })
            .await?;
        self.refresh().await?;
        if !self.items().iter().any(|i| i.id == DEFAULT_TEMPLATE_ID) {
            let mut default = Entity::reusable(
                "Default",
                "The standard editor layout.",
                ReusableContent::Layout {
                    layout: DockLayout::for_platform(self.platform),
                },
                now,
            );
            default.id = DEFAULT_TEMPLATE_ID.into();
            default.metadata.builtin = true;
            let result = self
                .publish(CommitBatch::prepare(
                    self.owner.clone(),
                    vec![Mutation::Create {
                        entity: default,
                        claim: false,
                        name_policy: NamePolicy::Exact,
                    }],
                )?)
                .await;
            if let Err(error) = result {
                if !matches!(error.kind, ErrorKind::Conflict | ErrorKind::NameCollision) {
                    return Err(error);
                }
            }
            self.refresh().await?;
        }
        let binding = match self
            .store
            .execute(StoreRequest::Binding {
                key: "last_workspace".into(),
            })
            .await?
        {
            StoreResponse::Binding(binding) => binding,
            _ => None,
        };
        let existing = binding
            .filter(|id| {
                self.items()
                    .iter()
                    .any(|i| &i.id == id && i.metadata.deleted_at_ms.is_none())
            })
            .or_else(|| {
                self.items()
                    .iter()
                    .filter(|i| {
                        i.metadata.kind == ItemKind::Workspace && i.metadata.deleted_at_ms.is_none()
                    })
                    .max_by_key(|i| i.metadata.last_used_ms)
                    .map(|i| i.id.clone())
            });
        if let Some(id) = existing {
            match self.prepare_switch(&id, now).await {
                Ok(entity) => return Ok(entity),
                Err(error) if error.kind == ErrorKind::OwnedElsewhere => {
                    let source = self.load(&id).await?.entity.capture()?;
                    let baseline = source.history.layout().clone();
                    let capture = WorkspaceCapture {
                        history: layer_ui::LayoutHistory::new(&baseline),
                        working: source.working,
                    };
                    return self
                        .create_and_bind(
                            Entity::workspace("My Workspace", capture, baseline, None, now),
                            NamePolicy::Unique,
                        )
                        .await;
                }
                Err(error) => return Err(error),
            }
        }
        let template = self.load(DEFAULT_TEMPLATE_ID).await?;
        let entity = self.workspace_from_template(&template.entity, "My Workspace", now)?;
        self.create_and_bind(entity, NamePolicy::Unique).await
    }
    fn workspace_from_template(&self, template: &Entity, name: &str, now: u64) -> Result<Entity> {
        let ItemContent::Reusable { current, .. } = &template.content else {
            return Err(StoreError::invalid("Choose a saved layout."));
        };
        let ReusableContent::Layout { layout } = &current.content else {
            return Err(StoreError::invalid("Choose a saved layout."));
        };
        Ok(Entity::workspace(
            name,
            WorkspaceCapture::from_template(layout).map_err(StoreError::invalid)?,
            layout.clone(),
            Some(TemplateOrigin {
                id: template.id.clone(),
                version: current.id.clone(),
                name: template.metadata.name.clone(),
                timestamp_ms: current.timestamp_ms,
            }),
            now,
        ))
    }
    pub(crate) async fn create_and_bind(
        &self,
        entity: Entity,
        name_policy: NamePolicy,
    ) -> Result<StoredEntity> {
        let id = entity.id.clone();
        let mut batch = CommitBatch::prepare(
            self.owner.clone(),
            vec![Mutation::Create {
                entity,
                claim: true,
                name_policy,
            }],
        )?;
        batch
            .bindings
            .push(("last_workspace".into(), Some(id.clone())));
        batch
            .bindings
            .push((format!("window:{}", self.owner.id), Some(id.clone())));
        self.publish(batch).await?;
        self.refresh().await?;
        self.load(&id).await
    }
    pub async fn prepare_switch(&self, id: &str, now: u64) -> Result<StoredEntity> {
        self.flush().await?;
        let mut incoming = self.claim(id).await?;
        let outcome = async {
            if incoming.entity.metadata.deleted_at_ms.is_some() {
                return Err(StoreError::invalid(
                    "This workspace was deleted. Choose another workspace.",
                ));
            }
            PreparedWorkspace::new(incoming.entity.capture()?).map_err(StoreError::invalid)?;
            let mut metadata = incoming.entity.metadata.clone();
            metadata.last_used_ms = now;
            let mut batch = CommitBatch::prepare(
                self.owner.clone(),
                vec![update(&incoming, Some(metadata.clone()), None, None)?],
            )?;
            batch
                .bindings
                .push(("last_workspace".into(), Some(id.into())));
            batch
                .bindings
                .push((format!("window:{}", self.owner.id), Some(id.into())));
            let receipt = self.publish(batch).await?;
            incoming.entity.metadata = metadata;
            incoming.generations = receipt.items[0].1;
            Ok(incoming.clone())
        }
        .await;
        if outcome.is_err() && self.active_id().as_deref() != Some(id) {
            self.release(&incoming).await;
        }
        outcome
    }
    /// One pending operation keeps its payload/ID across retries. New live edits
    /// remain in `latest` and are compared against the acknowledged snapshot.
    pub async fn save_once(&self) -> Result<()> {
        if self.saving.replace(true) {
            return Err(StoreError::new(
                ErrorKind::Conflict,
                "Workspace save is already in progress.",
            ));
        }
        struct Clear<'a>(&'a Cell<bool>);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _clear = Clear(&self.saving);
        let pending = {
            let mut s = self.state.borrow_mut();
            if let Some(pending) = &s.pending {
                Some(pending.clone())
            } else if let (Some(saved), Some(latest)) = (&s.saved, &s.latest) {
                if &saved.entity == latest {
                    None
                } else {
                    let mutation = update(
                        saved,
                        (saved.entity.metadata != latest.metadata).then(|| latest.metadata.clone()),
                        (saved.entity.content != latest.content).then(|| latest.content.clone()),
                        (saved.entity.working != latest.working)
                            .then(|| latest.working.clone())
                            .flatten(),
                    )?;
                    let pending = PendingSave {
                        batch: CommitBatch::prepare(self.owner.clone(), vec![mutation])?,
                        snapshot: latest.clone(),
                    };
                    s.pending = Some(pending.clone());
                    Some(pending)
                }
            } else {
                None
            }
        };
        let Some(pending) = pending else {
            return Ok(());
        };
        match self.publish(pending.batch.clone()).await {
            Ok(receipt) => {
                let mut s = self.state.borrow_mut();
                if let Some(saved) = &mut s.saved
                    && saved.entity.id == pending.snapshot.id
                {
                    saved.entity = pending.snapshot;
                    saved.generations = receipt.items[0].1;
                    s.pending = None;
                    if s.error_operation.as_deref() == Some(&pending.batch.operation_id) {
                        s.error = None;
                        s.error_operation = None;
                    }
                }
                Ok(())
            }
            Err(error) => {
                let mut state = self.state.borrow_mut();
                state.error = Some(error.clone());
                state.error_operation = Some(pending.batch.operation_id.clone());
                Err(error)
            }
        }
    }
    pub async fn flush(&self) -> Result<()> {
        while self.dirty() {
            self.save_once().await?;
        }
        Ok(())
    }
    pub async fn renew(&self) -> Result<()> {
        let saved = self
            .state
            .borrow()
            .saved
            .as_ref()
            .map(|s| (s.entity.id.clone(), s.claim.clone()));
        if let Some((id, Some(claim))) = saved {
            let result = self
                .store
                .execute(StoreRequest::Renew {
                    id: id.clone(),
                    owner: self.owner.clone(),
                    fence: claim.fence.to_string(),
                })
                .await;
            match result {
                Ok(StoreResponse::Claim(claim)) => {
                    if let Some(saved) = &mut self.state.borrow_mut().saved
                        && saved.entity.id == id
                    {
                        saved.claim = Some(claim);
                    }
                }
                Ok(_) => return Err(StoreError::invalid("Unexpected ownership reply.")),
                Err(error) => {
                    if self.active_id().as_deref() == Some(&id) {
                        self.set_error(error.clone());
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub fn lease_valid(&self, now: u64) -> bool {
        self.state
            .borrow()
            .saved
            .as_ref()
            .and_then(|s| s.claim.as_ref())
            .is_some_and(|c| c.owner == self.owner && c.expires_at_ms > now)
    }
    /// Resume an expired owner only after a coherent claim proves no intervening
    /// writes. Dirty memory is preserved; an unresolved old-fence delivery needs
    /// explicit recovery as a new workspace, never silent payload mutation.
    pub async fn revalidate_owner(&self, now: u64) -> Result<()> {
        if self.lease_valid(now) {
            let before = self.error();
            self.renew().await?;
            let mut state = self.state.borrow_mut();
            if state.error_operation.is_none()
                && state.error == before
                && state.error.as_ref().is_some_and(|e| {
                    matches!(
                        e.kind,
                        ErrorKind::Unavailable | ErrorKind::Conflict | ErrorKind::OwnedElsewhere
                    )
                })
            {
                state.error = None;
            }
            return Ok(());
        }
        let pending = self.state.borrow().pending.clone();
        if let Some(pending) = pending {
            match self
                .store
                .execute(StoreRequest::Receipt {
                    operation_id: pending.batch.operation_id.clone(),
                })
                .await?
            {
                StoreResponse::Receipt(Some(_)) => self.save_once().await?,
                _ => {
                    return Err(StoreError::new(
                        ErrorKind::Conflict,
                        "Ownership expired while a save was awaiting confirmation. Your changes remain in memory. Use Save as New Workspace to keep them.",
                    ));
                }
            }
        }
        let saved = self
            .state
            .borrow()
            .saved
            .clone()
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        let incoming = self.claim(&saved.entity.id).await?;
        if incoming.entity.metadata.deleted_at_ms.is_some()
            || incoming.generations != saved.generations
        {
            self.release(&incoming).await;
            return Err(StoreError::new(
                ErrorKind::Conflict,
                "This workspace changed while the window was suspended. Your changes remain in memory. Use Save as New Workspace to keep them.",
            ));
        }
        if self.active_id().as_deref() != Some(&saved.entity.id) {
            self.release(&incoming).await;
            return Ok(());
        }
        let mut state = self.state.borrow_mut();
        state.saved.as_mut().unwrap().claim = incoming.claim;
        if state
            .error
            .as_ref()
            .is_some_and(|e| matches!(e.kind, ErrorKind::Conflict | ErrorKind::OwnedElsewhere))
        {
            state.error = None;
            state.error_operation = None;
        }
        Ok(())
    }
    pub async fn create_workspace(
        &self,
        name: &str,
        template: Option<&str>,
        duplicate: bool,
        now: u64,
    ) -> Result<StoredEntity> {
        validate_name(name.trim())?;
        self.flush().await?;
        let entity = if let Some(id) = template {
            self.workspace_from_template(&self.load(id).await?.entity, name, now)?
        } else if let Some(current) = self.current() {
            let capture = current.capture()?;
            let ItemContent::Workspace {
                baseline, origin, ..
            } = current.content
            else {
                unreachable!()
            };
            if duplicate {
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
            }
        } else {
            return Err(StoreError::invalid("No workspace is active."));
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
    pub async fn save_template(&self, name: &str, description: &str, now: u64) -> Result<String> {
        self.flush().await?;
        let current = self
            .current()
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        let entity = Entity::reusable(
            name,
            description,
            ReusableContent::Layout {
                layout: current.capture()?.history.layout().clone(),
            },
            now,
        );
        let id = entity.id.clone();
        self.publish(CommitBatch::prepare(
            self.owner.clone(),
            vec![Mutation::Create {
                entity,
                claim: false,
                name_policy: NamePolicy::Exact,
            }],
        )?)
        .await?;
        self.refresh().await?;
        Ok(id)
    }
    pub async fn rename(&self, id: &str, name: &str, description: &str, now: u64) -> Result<()> {
        self.flush().await?;
        let stored = self.claim(id).await?;
        let outcome: Result<()> = async {
            let mut metadata = stored.entity.metadata.clone();
            metadata.rename(name, description, now)?;
            let receipt = self
                .publish(CommitBatch::prepare(
                    self.owner.clone(),
                    vec![update(&stored, Some(metadata.clone()), None, None)?],
                )?)
                .await?;
            if self.active_id().as_deref() == Some(id) {
                let mut s = self.state.borrow_mut();
                if let Some(saved) = &mut s.saved {
                    saved.entity.metadata = metadata.clone();
                    saved.generations = receipt.items[0].1;
                }
                if let Some(latest) = &mut s.latest {
                    latest.metadata = metadata;
                }
            }
            Ok(())
        }
        .await;
        if self.active_id().as_deref() != Some(id) {
            self.release(&stored).await;
        }
        outcome?;
        self.refresh().await
    }
    pub async fn close(&self) -> Result<()> {
        self.flush().await?;
        let saved = self.state.borrow().saved.clone();
        if let Some(saved) = saved {
            self.release(&saved).await;
        }
        Ok(())
    }

    /// Conflict recovery never requires overwriting or flushing the old owner.
    pub async fn save_as_new(
        &self,
        capture: WorkspaceCapture,
        name: &str,
        now: u64,
    ) -> Result<StoredEntity> {
        let (baseline, origin) = match self.current().map(|e| e.content) {
            Some(ItemContent::Workspace {
                baseline, origin, ..
            }) => (baseline, origin),
            _ => (capture.history.layout().clone(), None),
        };
        self.create_and_bind(
            Entity::workspace(name, capture, baseline, origin, now),
            NamePolicy::Unique,
        )
        .await
    }
}
fn update(
    stored: &StoredEntity,
    metadata: Option<Metadata>,
    content: Option<ItemContent>,
    working: Option<WorkspaceWorkingState>,
) -> Result<Mutation> {
    Ok(Mutation::Update {
        id: stored.entity.id.clone(),
        generations: stored.generations,
        fence: stored
            .claim
            .as_ref()
            .ok_or_else(StoreError::conflict)?
            .fence,
        metadata,
        content,
        working,
        name_policy: NamePolicy::Exact,
    })
}

#[cfg(all(test, feature = "native"))]
#[path = "manager_tests.rs"]
mod tests;

#[path = "manager_operations.rs"]
mod operations;

#[path = "manager_recovery.rs"]
mod recovery;
