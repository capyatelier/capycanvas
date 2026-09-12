//! Application-level switcher preferences never claim or edit a workspace.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SwitcherEdit {
    Show { id: String, visible: bool },
    Move { id: String, before: Option<String> },
}

pub(crate) fn validate_ids(ids: &[String]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    if ids.len() > 10_000
        || ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 512 || !seen.insert(id))
    {
        return Err(StoreError::invalid("Invalid workspace switcher order."));
    }
    Ok(())
}

impl<S: WorkspaceStore> WorkspaceManager<S> {
    /// None in storage is the original three defaults; an empty list hides it.
    pub fn switcher_ids(&self) -> Vec<String> {
        let state = self.state.borrow();
        let ids = state.switcher.clone().unwrap_or_else(|| {
            DEFAULT_WORKSPACES
                .iter()
                .map(|(id, _)| (*id).into())
                .collect()
        });
        ids.into_iter()
            .filter(|id| {
                state.items.iter().any(|i| {
                    &i.id == id
                        && i.metadata.kind == ItemKind::Workspace
                        && i.metadata.deleted_at_ms.is_none()
                })
            })
            .collect()
    }

    /// Hosts with a switcher call this at startup and when refreshing the list.
    pub async fn refresh_switcher(&self) -> Result<()> {
        let StoreResponse::Switcher(ids) = self.store.execute(StoreRequest::Switcher).await? else {
            return Err(StoreError::invalid("Unexpected workspace switcher reply."));
        };
        if let Some(ids) = &ids {
            validate_ids(ids)?;
        }
        self.state.borrow_mut().switcher = ids;
        Ok(())
    }

    pub async fn edit_switcher(&self, edit: SwitcherEdit) -> Result<()> {
        // Refresh first so independent edits compose across windows. A competing
        // write during publication is rejected by the store's atomic comparison.
        self.refresh().await?;
        self.refresh_switcher().await?;
        let expected = self.state.borrow().switcher.clone();
        let mut ids = self.switcher_ids();
        match edit {
            SwitcherEdit::Show { id, visible } => {
                if !self.items().iter().any(|i| {
                    i.id == id
                        && i.metadata.kind == ItemKind::Workspace
                        && i.metadata.deleted_at_ms.is_none()
                }) {
                    return Err(StoreError::invalid(
                        "This workspace is no longer available.",
                    ));
                }
                if visible {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                } else {
                    ids.retain(|i| i != &id);
                }
            }
            SwitcherEdit::Move { id, before } => {
                if !ids.contains(&id) || before.as_ref().is_some_and(|target| !ids.contains(target))
                {
                    return Err(StoreError::invalid(
                        "Only workspaces shown in the top bar can be reordered.",
                    ));
                }
                if before.as_ref() == Some(&id) {
                    return Ok(());
                }
                ids.retain(|i| i != &id);
                let index = before
                    .as_ref()
                    .and_then(|target| ids.iter().position(|i| i == target))
                    .unwrap_or(ids.len());
                ids.insert(index, id);
            }
        }
        let response = self
            .store
            .execute(StoreRequest::UpdateSwitcher { expected, ids })
            .await;
        match response {
            Ok(StoreResponse::Switcher(ids)) => {
                self.state.borrow_mut().switcher = ids;
                Ok(())
            }
            Err(error) => {
                let _ = self.refresh_switcher().await;
                Err(error)
            }
            _ => Err(StoreError::invalid("Unexpected workspace switcher reply.")),
        }
    }
}
