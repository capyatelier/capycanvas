//! Windows lifecycle adapter for the shared workspace controller. Futures run on
//! the canvas owner; StoreWorker performs every SQLite operation on its I/O thread.
use crate::workspace_async::AsyncTask;
use layer_host::NativeHost;
use layer_ui::Platform;
use layer_workspace::{
    ErrorKind, StoreError, WorkspaceController, WorkspaceInput, WorkspaceStore, WorkspaceView,
};
use serde::Serialize;
use std::sync::Arc;

type Result<T> = std::result::Result<T, StoreError>;
/// The shared controller view plus the Windows window-close and backup state.
#[derive(Serialize)]
pub(crate) struct WorkspaceStatus<'a> {
    #[serde(flatten)]
    view: &'a WorkspaceView,
    can_switch: bool,
    close_attempt: u32,
    notice: Option<&'a str>,
}

pub(crate) struct WorkspaceService<S: WorkspaceStore + 'static> {
    controller: WorkspaceController<S>,
    directory: std::path::PathBuf,
    export: AsyncTask<std::result::Result<(), String>>,
    notice: Option<String>,
    close_attempt: u32,
    owner_lost: bool,
    published: String,
}
impl<S: WorkspaceStore + 'static> WorkspaceService<S> {
    pub(crate) fn new(
        store: S,
        directory: std::path::PathBuf,
        localization: Arc<layer_ui::Localizer>,
        wake: impl Fn() + Clone + Send + Sync + 'static,
    ) -> Self {
        let mut controller = WorkspaceController::new_localized(store, Platform::Windows, now_ms(), localization);
        controller.set_wake(Arc::new(wake.clone()));
        Self {
            controller,
            directory,
            export: AsyncTask::new(wake),
            notice: None,
            close_attempt: 0,
            owner_lost: false,
            published: String::new(),
        }
    }
    pub(crate) fn view(&self) -> &WorkspaceView {
        &self.controller.view
    }
    pub(crate) fn status(&self, native: &NativeHost) -> WorkspaceStatus<'_> {
        WorkspaceStatus {
            view: &self.controller.view,
            can_switch: self.accepts_input(now_ms())
                && native.session.require_workspace_idle().is_ok(),
            close_attempt: self.close_attempt,
            notice: self.notice.as_deref(),
        }
    }
    pub(crate) fn input(&mut self, native: &mut NativeHost, input: WorkspaceInput) -> Result<()> {
        let view = &self.controller.view;
        if matches!(input, WorkspaceInput::Close)
            || (matches!(input, WorkspaceInput::Retry) && view.closing)
        {
            self.close_attempt = self.close_attempt.saturating_add(1);
        }
        let before = native.session.state().revision;
        let result = self
            .controller
            .input(&mut native.session, input, now_ms())
            .map(|change| native.apply_change(before, change));
        self.publish(native);
        result
    }
    pub(crate) fn report_error(&mut self, native: &mut NativeHost, error: StoreError) {
        self.controller.view.error = Some(error.localized_message(native.session.localization()));
        self.publish(native);
    }
    fn publish(&mut self, native: &mut NativeHost) {
        let status = serde_json::to_string(&self.status(native)).unwrap_or_default();
        if status != self.published {
            self.published = status;
            native.invalidate_snapshot();
        }
    }
    /// Called after accepted changes and on the host's idle service deadline.
    /// This never waits for a reply or repeatedly polls an unwoken future.
    pub(crate) fn poll(&mut self, native: &mut NativeHost, wall_ms: u64) {
        let changed = native.take_service_changes();
        self.controller
            .observe_regions(&mut native.session, changed, wall_ms);
        let view = &self.controller.view;
        if view.ready || view.closing || native.startup.brush_ready {
            let before = native.session.state().revision;
            let change = self.controller.tick(&mut native.session, wall_ms);
            native.apply_change(before, change);
        }
        if let Some(result) = self.export.poll() {
            self.notice = Some(match result {
                Ok(()) => "Workspace backup saved.".into(),
                Err(error) => error,
            });
        }
        let lost = self.controller.view.owner_lost;
        if lost && !self.owner_lost {
            let _ = native.input(layer_ui::UiInput::Blur);
        }
        self.owner_lost = lost;
        self.publish(native);
    }
    pub(crate) fn accepts_input(&self, wall_ms: u64) -> bool {
        let view = &self.controller.view;
        view.ready
            && !view.busy
            && !view.owner_lost
            && !view.closing
            && !view.closed
            && view.page.is_none()
            && view.prompt.is_none()
            && view.focus_window.is_none()
            && self.controller.manager.lease_valid(wall_ms)
    }
    pub(crate) fn stop(&mut self) {
        self.controller.stop();
        self.export.close();
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WorkspaceAction {
    Input { input: WorkspaceInput },
    PreferencesRetry,
    PreferencesKeepOpen,
    PreferencesDiscardClose,
    ExportBackup { path: String },
    BackupDatabase { path: String },
    Failure { error: String },
}
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl<S: WorkspaceStore + 'static> WorkspaceService<S> {
    fn require_idle(&self) -> Result<()> {
        if self.controller.view.busy || self.export.busy() {
            Err(StoreError::new(
                ErrorKind::Conflict,
                "Wait for the current workspace operation.",
            ))
        } else {
            Ok(())
        }
    }
    pub(crate) fn export_backup(&mut self, native: &mut NativeHost, path: String) -> Result<()> {
        use std::{path::Path, sync::atomic::AtomicBool};
        self.require_idle()?;
        crate::document_io::location(&path).map_err(StoreError::invalid)?;
        if !self.controller.view.ready {
            return Err(StoreError::invalid(
                "Back up the original workspace database instead.",
            ));
        }
        self.controller.observe(&mut native.session, now_ms());
        let entity = self
            .controller
            .manager
            .current()
            .ok_or_else(|| StoreError::invalid("No workspace is open."))?;
        let database = self.directory.join("workspaces.sqlite3");
        let localization = native.session.localization().clone();
        let job = crate::workspace_async::BlockingTask::start(move || {
            layer_workspace::validate_database_export_destination(&database, Path::new(&path))
                .map_err(|e| e.to_string())?;
            let bytes = layer_workspace::export_package(&entity).map_err(|e| e.localized_message(&localization))?;
            crate::document_io::atomic_write(Path::new(&path), &AtomicBool::new(false), |file| {
                file.write_all(&bytes)
                    .map_err(|_| "Could not write the workspace backup.".into())
            })
        })
        .map_err(|_| {
            StoreError::new(ErrorKind::Unavailable, "Could not start workspace export.")
        })?;
        self.notice = None;
        let _ = self.export.start(async move { job.await.and_then(|r| r) });
        self.publish(native);
        Ok(())
    }
}
impl WorkspaceService<layer_workspace::StoreWorker> {
    pub(crate) fn backup_database(&mut self, native: &mut NativeHost, path: String) -> Result<()> {
        self.require_idle()?;
        crate::document_io::location(&path).map_err(StoreError::invalid)?;
        let manager = self.controller.manager.clone();
        let localization = native.session.localization().clone();
        self.notice = None;
        let _ = self.export.start(async move {
            manager
                .store
                .backup_database(std::path::Path::new(&path))
                .await
                .map(|_| ())
                .map_err(|e| e.localized_message(&localization))
        });
        self.publish(native);
        Ok(())
    }
}

#[cfg(test)]
#[path = "workspace_service_tests.rs"]
mod tests;
