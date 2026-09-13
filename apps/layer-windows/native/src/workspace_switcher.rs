//! Application preferences have their own async lifetime, independent of a
//! dialog preview. Closing a dialog never cancels an accepted preference write.
use super::*;
use layer_workspace::SwitcherEdit;

impl<S: WorkspaceStore + 'static> WorkspaceService<S> {
    pub(crate) fn refresh_switcher(&mut self) {
        if !self.status.close_requested {
            self.refresh_preferences = true;
        }
    }

    pub(super) fn sync_switcher(&mut self, notify: bool) {
        let items = self.manager.items();
        let shortcuts = |ids: Vec<String>| {
            ids.into_iter()
                .filter_map(|id| {
                    let item = items.iter().find(|item| item.id == id)?;
                    let key = layer_workspace::DEFAULT_WORKSPACES
                        .iter()
                        .find(|(default, _)| *default == id)
                        .map(|(_, preset)| preset.name().to_lowercase())
                        .unwrap_or_else(|| id.clone());
                    Some(WorkspaceShortcut {
                        id,
                        key,
                        name: item.metadata.name.clone(),
                    })
                })
                .collect::<Vec<_>>()
        };
        let pinned = shortcuts(self.manager.switcher_ids());
        let order = self.manager.workspace_ids();
        if notify
            && self.status.ready
            && (pinned != self.status.switcher || order != self.status.order)
        {
            self.status.switcher_revision = self.status.switcher_revision.wrapping_add(1);
        }
        self.status.switcher = pinned;
        self.status.order = order;
        self.status.switcher_display = shortcuts(self.manager.switcher_display_ids());
    }

    pub(super) fn edit_switcher(&mut self, native: &mut NativeHost, edit: SwitcherEdit) {
        if !self.status.ready
            || self.status.close_requested
            || self.preferences_edited
            || self.status.owner_lost
        {
            return;
        }
        if self.operation.busy() {
            self.status.switcher_error =
                Some("Wait for the current workspace operation, then try again.".into());
            native.invalidate_snapshot();
            return;
        }
        // Menu activation can enqueue a focus refresh after the UI displayed an
        // enabled action. Supersede that read; the edit performs its own refresh.
        self.preferences.cancel_read();
        self.refresh_preferences = false;
        self.status.switcher_error = None;
        self.preferences_edited = true;
        self.status.switcher_busy = true;
        let manager = self.manager.clone();
        let started = self
            .preferences
            .start(async move { manager.edit_switcher(edit).await });
        debug_assert!(started.is_ok());
        native.invalidate_snapshot();
    }

    pub(super) fn poll_switcher(&mut self, native: &mut NativeHost, wall_ms: u64) {
        if let Some(result) = self.preferences.poll() {
            let edited = self.preferences_edited;
            self.preferences_edited = false;
            match result {
                Ok(()) => {
                    self.status.switcher_error = None;
                    if edited {
                        self.status.switcher_revision =
                            self.status.switcher_revision.wrapping_add(1);
                    }
                }
                Err(error) => {
                    self.status.switcher_error = Some(error.to_string());
                    if edited && self.status.close_requested {
                        self.status.error = Some(error.to_string());
                    }
                }
            }
            self.sync_switcher(false);
            self.refresh_switcher_rows(native, wall_ms);
            native.invalidate_snapshot();
        }
        if self.refresh_preferences
            && self.status.ready
            && !self.status.close_requested
            && !self.operation.busy()
            && self.incoming.is_none()
            && !self.preferences.busy()
        {
            self.refresh_preferences = false;
            let manager = self.manager.clone();
            let started = self.preferences.start(async move {
                manager.refresh().await?;
                manager.refresh_switcher().await
            });
            debug_assert!(started.is_ok());
        }
        self.status.switcher_busy = self.preferences.busy();
    }
}
