use crate::WorkspaceRefusal;
use super::*;

#[derive(Clone)]
pub(crate) struct InterruptedChange {
    pub id: String,
    names: Vec<Option<(String, Metadata)>>,
}
impl InterruptedChange {
    pub fn label(&self, localization: &layer_ui::Localizer) -> String {
        let names = self.names.iter().map(|name| match name {
            Some((id, metadata)) => workspace_display_name(id, metadata, localization),
            None => localization.text(layer_ui::MessageId::WORKSPACE_CHANGES).to_string(),
        }).collect::<Vec<_>>();
        name_list(localization, &names)
    }
}
impl<S: WorkspaceStore> WorkspaceManager<S> {
    async fn interrupted_batches(&self) -> Result<Vec<CommitBatch>> {
        let StoreResponse::Pending(mut batches) = self.execute(StoreRequest::Pending).await? else {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::UnexpectedInterruptedChangeReply));
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
        Ok(self.interrupted_change_sources(now).await?.into_iter()
            .map(|change| (change.id.clone(), change.label(&self.localization()))).collect())
    }
    pub(crate) async fn interrupted_change_sources(&self, now: u64) -> Result<Vec<InterruptedChange>> {
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
                self.execute(StoreRequest::Receipt {
                    operation_id: batch.operation_id.clone()
                })
                .await?,
                StoreResponse::Receipt(Some(_))
            ) {
                let _ = self
                    .execute(StoreRequest::Acknowledge {
                        operation_id: batch.operation_id,
                    })
                    .await;
                continue;
            }
            let names: Vec<_> = batch.writes.iter()
                .filter(|w| !(w.delete || w.create && model::is_default_item(&w.id)))
                .map(|write| write.metadata.clone().or_else(|| self.items().into_iter()
                    .find(|item| item.id == write.id).map(|item| item.metadata))
                    .map(|metadata| (write.id.clone(), metadata)))
                .collect();
            if !names.is_empty() {
                choices.push(InterruptedChange { id: batch.operation_id, names });
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
                StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::TheseChangesHaveAlreadyBeenRecoveredOrSaved)
            })?;
        self.flush().await?;
        let mut entities = Vec::new();
        for write in &batch.writes {
            if write.delete || (write.create && model::is_default_item(&write.id)) {
                continue;
            }
            let base = if write.create {
                None
            } else {
                Some(self.load(&write.id).await?.entity)
            };
            let mut entity=Entity {
                id:new_id(),
                metadata:write.metadata.clone().or_else(||base.as_ref().map(|e|e.metadata.clone())).ok_or_else(||StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::TheInterruptedItemIsMissingMetadata))?,
                content:if let Some(content)=&write.content_json {
                    protocol::unpack(content,|id|batch.components.get(id).cloned().ok_or_else(||StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::AnInterruptedResourceIsMissing)))?
                } else {base.as_ref().ok_or_else(||StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::TheInterruptedItemIsMissingItsLayout))?.content.clone()},
                working:if let Some(working)=&write.working_json {Some(serde_json::from_str(working)?)} else {base.as_ref().and_then(|e|e.working.clone())},
            };
            entity.metadata.name = message(&self.localization(), layer_ui::MessageId::WORKSPACE_RECOVERED_NAME,
                &[("name", self.display_name(&write.id, &entity.metadata).chars().take(90).collect())])
                .chars().take(100).collect();
            entity.metadata.builtin = false;
            entity.metadata.created_at_ms = now;
            entity.metadata.last_used_ms = now;
            entity.validate()?;
            entities.push(entity);
        }
        if entities.is_empty() {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::NoRecoverableItemsWereFound));
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
            self.bind_window(&mut recovery, id);
        }
        self.publish(recovery).await?;
        self.forget_operation(operation);
        self.refresh().await?;
        match incoming {
            Some(id) => self.load(&id).await.map(Some),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    #[test]
    fn interrupted_labels_reproject_names_without_changing_recovery_identity() {
        let english = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        let source = Entity::included_workspace(DEFAULT_WORKSPACES[0].0, Platform::Web, 1000, &english).unwrap();
        let change = InterruptedChange {
            id: "operation-id".into(),
            names: vec![Some((source.id.clone(), source.metadata.clone())), None],
        };
        let label = change.label(&english);
        for language in layer_ui::UiLanguage::ALL.into_iter().filter(|language| *language != layer_ui::UiLanguage::English) {
            let localization = layer_ui::Localizer::shared(language);
            assert_ne!(change.label(&localization), label);
            assert!(change.label(&localization).contains(&workspace_display_name(&source.id, &source.metadata, &localization)));
            assert_eq!(change.id, "operation-id");
            assert_eq!(change.names[0].as_ref().unwrap().1, source.metadata);
        }
    }
}
