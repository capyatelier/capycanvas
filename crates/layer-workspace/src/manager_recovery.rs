use super::*;

impl<S: WorkspaceStore> WorkspaceManager<S> {
    async fn interrupted_batches(&self) -> Result<Vec<CommitBatch>> {
        let StoreResponse::Pending(mut batches) = self.store.execute(StoreRequest::Pending).await?
        else {
            return Err(StoreError::invalid("Unexpected interrupted-change reply."));
        };
        if let Some(batch) = self.state.borrow().failed_operation.clone()
            && !batches.iter().any(|b| b.operation_id == batch.operation_id)
        {
            batches.push(batch);
        }
        for batch in &self.state.borrow().older_failed_operations {
            if !batches.iter().any(|b| b.operation_id == batch.operation_id) {
                batches.push(batch.clone());
            }
        }
        Ok(batches)
    }
    pub async fn interrupted_changes(&self, now: u64) -> Result<Vec<(String, String)>> {
        let batches = self.interrupted_batches().await?;
        let superseded: std::collections::BTreeSet<_> = batches
            .iter()
            .flat_map(|b| b.abandon_operations.iter().cloned())
            .collect();
        let pending_save = self
            .state
            .borrow()
            .pending
            .as_ref()
            .map(|p| p.batch.operation_id.clone());
        let mut choices = Vec::new();
        for batch in batches {
            if superseded.contains(&batch.operation_id) {
                continue;
            }
            if pending_save.as_ref() == Some(&batch.operation_id) {
                continue;
            }
            if batch.owner != self.owner
                && self.items().iter().any(|i| {
                    i.claim
                        .as_ref()
                        .is_some_and(|c| c.owner == batch.owner && c.expires_at_ms > now)
                })
            {
                continue;
            }
            if matches!(
                self.store
                    .execute(StoreRequest::Receipt {
                        operation_id: batch.operation_id.clone()
                    })
                    .await?,
                StoreResponse::Receipt(Some(_))
            ) {
                let _ = self
                    .store
                    .execute(StoreRequest::Acknowledge {
                        operation_id: batch.operation_id,
                    })
                    .await;
                continue;
            }
            let names: Vec<_> = batch
                .writes
                .iter()
                .filter(|w| w.id != DEFAULT_TEMPLATE_ID)
                .map(|write| {
                    write
                        .metadata
                        .as_ref()
                        .map(|m| m.name.clone())
                        .or_else(|| {
                            self.items()
                                .iter()
                                .find(|i| i.id == write.id)
                                .map(|i| i.metadata.name.clone())
                        })
                        .unwrap_or_else(|| "Workspace changes".into())
                })
                .collect();
            if !names.is_empty() {
                choices.push((batch.operation_id, names.join(", ")));
            }
        }
        Ok(choices)
    }
    pub async fn recover_interrupted(
        &self,
        operation: &str,
        now: u64,
    ) -> Result<Option<StoredEntity>> {
        let batch = self
            .interrupted_batches()
            .await?
            .into_iter()
            .find(|b| b.operation_id == operation)
            .ok_or_else(|| {
                StoreError::invalid("These changes have already been recovered or saved.")
            })?;
        self.flush().await?;
        let mut entities = Vec::new();
        for write in &batch.writes {
            if write.id == DEFAULT_TEMPLATE_ID {
                continue;
            }
            let base = if write.create {
                None
            } else {
                Some(self.load(&write.id).await?.entity)
            };
            let mut entity=Entity {
                id:new_id(),
                metadata:write.metadata.clone().or_else(||base.as_ref().map(|e|e.metadata.clone())).ok_or_else(||StoreError::invalid("The interrupted item is missing metadata. Export the original database to preserve it."))?,
                content:if let Some(content)=&write.content_json {
                    protocol::unpack(content,|id|batch.components.get(id).cloned().ok_or_else(||StoreError::invalid("An interrupted resource is missing. Export the original database to preserve it.")))?
                } else {base.as_ref().ok_or_else(||StoreError::invalid("The interrupted item is missing its layout."))?.content.clone()},
                working:if let Some(working)=&write.working_json {Some(serde_json::from_str(working)?)} else {base.as_ref().and_then(|e|e.working.clone())},
            };
            if !entity.metadata.name.ends_with(" Recovered") {
                entity.metadata.name = format!(
                    "{} Recovered",
                    entity.metadata.name.chars().take(90).collect::<String>()
                );
            }
            entity.metadata.builtin = false;
            entity.metadata.deleted_at_ms = None;
            entity.metadata.created_at_ms = now;
            entity.metadata.last_used_ms = now;
            entity.validate()?;
            entities.push(entity);
        }
        if entities.is_empty() {
            return Err(StoreError::invalid("No recoverable items were found."));
        }
        let incoming = entities
            .iter()
            .find(|e| e.metadata.kind == ItemKind::Workspace)
            .map(|e| e.id.clone());
        let mutations = entities
            .into_iter()
            .map(|entity| Mutation::Create {
                claim: incoming.as_ref() == Some(&entity.id),
                entity,
                name_policy: NamePolicy::Unique,
            })
            .collect();
        let mut recovery = CommitBatch::prepare(self.owner.clone(), mutations)?;
        recovery.abandon_operations = batch.abandon_operations;
        recovery.abandon_operations.push(operation.into());
        if let Some(id) = &incoming {
            recovery
                .bindings
                .push(("last_workspace".into(), Some(id.clone())));
            recovery
                .bindings
                .push((format!("window:{}", self.owner.id), Some(id.clone())));
        }
        self.publish(recovery).await?;
        self.state
            .borrow_mut()
            .older_failed_operations
            .retain(|b| b.operation_id != operation);
        if self
            .state
            .borrow()
            .failed_operation
            .as_ref()
            .is_some_and(|b| b.operation_id == operation)
        {
            let mut state = self.state.borrow_mut();
            state.failed_operation = None;
            if state.error_operation.as_deref() == Some(operation) {
                state.error = None;
                state.error_operation = None;
            }
        }
        self.refresh().await?;
        match incoming {
            Some(id) => self.load(&id).await.map(Some),
            None => Ok(None),
        }
    }
}
