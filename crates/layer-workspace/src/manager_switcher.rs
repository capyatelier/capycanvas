//! Application-level switcher preferences never claim or edit a workspace.
use crate::WorkspaceRefusal;
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SwitcherEdit {
    Show { id: String, visible: bool },
    Toggle { id: String },
    Move { id: String, before: Option<String> },
}

impl SwitcherEdit {
    pub(crate) fn apply_order(&self, ids: &mut Vec<String>) -> Result<bool> {
        let Self::Move { id, before } = self else { return Ok(false); };
        if !ids.contains(id) || before.as_ref().is_some_and(|target| !ids.contains(target)) {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisWorkspaceIsNoLongerAvailable));
        }
        if before.as_ref() == Some(id) { return Ok(false); }
        ids.retain(|candidate| candidate != id);
        let index = before.as_ref().and_then(|target| ids.iter().position(|candidate| candidate == target)).unwrap_or(ids.len());
        ids.insert(index, id.clone());
        Ok(true)
    }
}

pub(crate) fn validate_ids(ids: &[String]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    if ids.len() > 10_000
        || ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 512 || !seen.insert(id))
    {
        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InvalidWorkspaceSwitcherOrder));
    }
    Ok(())
}

pub(crate) fn with_created_pins(
    pins: Option<Vec<String>>,
    created: &[String],
) -> Result<Vec<String>> {
    let mut ids = pins.unwrap_or_else(|| DEFAULT_WORKSPACES.map(|(id, _)| id.to_string()).to_vec());
    for id in created {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    validate_ids(&ids)?;
    Ok(ids)
}

impl<S: WorkspaceStore> WorkspaceManager<S> {
    /// Complete dialog order. New workspaces follow saved entries alphabetically.
    /// Existing switcher-only preferences seed the order on upgrade.
    pub fn workspace_ids(&self) -> Vec<String> {
        let state = self.state.borrow();
        let saved = state.workspace_order.as_ref().or(state.switcher.as_ref());
        let defaults = DEFAULT_WORKSPACES
            .iter()
            .map(|(id, _)| (*id).into())
            .collect();
        let saved = saved.unwrap_or(&defaults);
        let mut items: Vec<_> = state
            .items
            .iter()
            .filter(|i| i.metadata.kind == ItemKind::Workspace)
            .collect();
        items.sort_by_key(|i| {
            (
                saved
                    .iter()
                    .position(|id| id == &i.id)
                    .unwrap_or(usize::MAX),
                name_key(&i.metadata.name),
                &i.id,
            )
        });
        items.into_iter().map(|i| i.id.clone()).collect()
    }

    /// Persistently pinned choices. None in storage is the original three defaults.
    /// Visibility never changes the dialog order.
    pub fn switcher_ids(&self) -> Vec<String> {
        self.switcher_ids_in(&self.workspace_ids())
    }

    pub(crate) fn switcher_ids_in(&self, order: &[String]) -> Vec<String> {
        let state = self.state.borrow();
        let defaults = DEFAULT_WORKSPACES
            .iter()
            .map(|(id, _)| (*id).into())
            .collect();
        let pinned = state.switcher.as_ref().unwrap_or(&defaults);
        order.iter()
            .filter(|id| pinned.contains(id))
            .cloned()
            .collect()
    }

    /// Header choices include the current workspace while it is unpinned.
    /// This does not change saved visibility or order; previews do not change it.
    pub fn switcher_display_ids(&self) -> Vec<String> {
        self.switcher_display_ids_in(&self.workspace_ids())
    }

    pub(crate) fn switcher_display_ids_in(&self, order: &[String]) -> Vec<String> {
        let mut ids = self.switcher_ids_in(order);
        if let Some(active) = self.active_id()
            && !ids.contains(&active)
        {
            ids.insert(0, active);
        }
        ids
    }

    /// Hosts with a switcher call this at startup and when refreshing the list.
    pub async fn refresh_switcher(&self) -> Result<()> {
        let StoreResponse::Switcher(ids) = self.execute(StoreRequest::Switcher).await? else {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::UnexpectedWorkspaceSwitcherReply));
        };
        if let Some(ids) = &ids {
            validate_ids(ids)?;
        }
        let StoreResponse::WorkspaceOrder(order) =
            self.execute(StoreRequest::WorkspaceOrder).await?
        else {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::UnexpectedWorkspaceOrderReply));
        };
        if let Some(order) = &order {
            validate_ids(order)?;
        }
        let mut state = self.state.borrow_mut();
        state.switcher = ids;
        state.workspace_order = order;
        Ok(())
    }

    pub async fn edit_switcher(&self, edit: SwitcherEdit) -> Result<()> {
        // Refresh first so independent edits compose across windows. A competing
        // write during publication is rejected by the store's atomic comparison.
        self.refresh().await?;
        self.refresh_switcher().await?;
        let request = match edit {
            SwitcherEdit::Show { id, visible } => self.switcher_visibility_request(id, visible).await?,
            SwitcherEdit::Toggle { id } => {
                let visible = !self.switcher_ids().contains(&id);
                self.switcher_visibility_request(id, visible).await?
            }
            edit @ SwitcherEdit::Move { .. } => {
                let expected = self.state.borrow().workspace_order.clone();
                let mut ids = self.workspace_ids();
                if !edit.apply_order(&mut ids)? { return Ok(()); }
                StoreRequest::UpdateWorkspaceOrder { expected, ids }
            }
        };
        let response = self.execute(request).await;
        match response {
            Ok(StoreResponse::Switcher(ids)) => {
                self.state.borrow_mut().switcher = ids;
                Ok(())
            }
            Ok(StoreResponse::WorkspaceOrder(ids)) => {
                self.state.borrow_mut().workspace_order = ids;
                Ok(())
            }
            Err(error) => {
                let _ = self.refresh_switcher().await;
                Err(error)
            }
            _ => Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::UnexpectedWorkspaceSwitcherReply)),
        }
    }

    async fn switcher_visibility_request(&self, id: String, visible: bool) -> Result<StoreRequest> {
        let expected = self.state.borrow().switcher.clone();
        let mut ids = self.switcher_ids();
        if !self
            .items()
            .iter()
            .any(|i| i.id == id && i.metadata.kind == ItemKind::Workspace)
        {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisWorkspaceIsNoLongerAvailable));
        }
        // Freeze the initial/legacy row order before visibility changes.
        if self.state.borrow().workspace_order.is_none() {
            let order = self.workspace_ids();
            let StoreResponse::WorkspaceOrder(saved) = self
                .execute(StoreRequest::UpdateWorkspaceOrder {
                    expected: None,
                    ids: order,
                })
                .await?
            else {
                return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::UnexpectedWorkspaceOrderReply));
            };
            self.state.borrow_mut().workspace_order = saved;
        }
        if visible {
            if !ids.contains(&id) {
                ids.push(id);
            }
        } else {
            ids.retain(|i| i != &id);
        }
        Ok(StoreRequest::UpdateSwitcher { expected, ids })
    }
}
