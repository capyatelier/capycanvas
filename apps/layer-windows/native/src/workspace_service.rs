//! Windows lifecycle adapter for shared workspace policy. Futures run on the
//! canvas owner; StoreWorker performs every SQLite operation on its I/O thread.
use crate::workspace_async::AsyncTask;
use layer_host::NativeHost;
use layer_ui::{Platform, PreparedWorkspace, regions};
use layer_workspace::{
    ErrorKind, StoreError, StoreRequest, StoredEntity, WorkspaceManager, WorkspaceStore,
};
use serde::Serialize;
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

#[path = "workspace_manager.rs"]
mod manager_ui;
pub(crate) use manager_ui::{ManagerInput, ManagerView};

type Result<T> = std::result::Result<T, StoreError>;
enum Completion {
    Open(Result<Box<StoredEntity>>),
    Save(Result<()>),
    Close(Result<()>),
    Manager(Result<Option<Box<StoredEntity>>>),
    Toolbar(
        Result<(
            layer_workspace::ToolbarDefinition,
            Option<layer_ui::Panel>,
            Option<u32>,
        )>,
    ),
    Released,
    Export(std::result::Result<(), String>),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceShortcut {
    pub id: String,
    pub key: String,
    pub name: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceStatus {
    pub ready: bool,
    pub id: Option<String>,
    pub defaults: Vec<WorkspaceShortcut>,
    pub can_switch: bool,
    pub owner: Option<String>,
    pub busy: bool,
    pub dirty: bool,
    pub saving: bool,
    pub owner_lost: bool,
    pub close_requested: bool,
    pub close_ready: bool,
    pub close_attempt: u32,
    pub name: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

pub(crate) struct WorkspaceService<S: WorkspaceStore + 'static> {
    manager: Rc<WorkspaceManager<S>>,
    directory: std::path::PathBuf,
    operation: AsyncTask<Completion>,
    ownership: AsyncTask<Result<()>>,
    incoming: Option<StoredEntity>,
    ui: manager_ui::ManagerUi,
    status: WorkspaceStatus,
    captured_generation: Option<u64>,
    layout_pending: bool,
    last_edit: Instant,
    last_save: Instant,
    last_renew: Instant,
}
impl<S: WorkspaceStore + 'static> WorkspaceService<S> {
    pub(crate) fn new(
        store: S,
        directory: std::path::PathBuf,
        wake: impl Fn() + Clone + Send + 'static,
    ) -> Self {
        let now = Instant::now();
        let manager = Rc::new(WorkspaceManager::new(store, Platform::Windows));
        let owner = manager.owner.id.clone();
        Self {
            manager,
            directory,
            operation: AsyncTask::new(wake.clone()),
            ownership: AsyncTask::new(wake.clone()),
            ui: manager_ui::ManagerUi::new(wake),
            incoming: None,
            status: WorkspaceStatus {
                busy: true,
                owner: Some(owner),
                ..Default::default()
            },
            captured_generation: None,
            layout_pending: false,
            last_edit: now,
            last_save: now,
            last_renew: now,
        }
    }
    pub(crate) fn report_error(&mut self, native: &mut NativeHost, error: StoreError) {
        self.ui_error(native, error);
    }
    pub(crate) fn status(&self) -> &WorkspaceStatus {
        &self.status
    }
    pub(crate) fn start(&mut self, wall_ms: u64) {
        if self.operation.busy() || self.incoming.is_some() || self.status.ready {
            return;
        }
        self.status.busy = true;
        self.status.error = None;
        let manager = self.manager.clone();
        let _ = self.operation.start(async move {
            let result = async {
                manager.store.execute(StoreRequest::Reopen).await?;
                manager.initialize(wall_ms).await
            }
            .await;
            Completion::Open(result.map(Box::new))
        });
    }
    fn observe(
        &mut self,
        native: &mut NativeHost,
        now: Instant,
        wall_ms: u64,
        force: bool,
    ) -> Result<()> {
        let changed = native.take_service_changes();
        self.layout_pending |= changed & (regions::LAYOUT | regions::CUSTOMIZATION) != 0;
        let generation = native.session.workspace_layout_generation();
        if force
            || (self.layout_pending
                && generation.is_some()
                && generation != self.captured_generation)
        {
            let capture = native
                .session
                .capture_workspace()
                .map_err(StoreError::invalid)?;
            self.manager.observe(capture, wall_ms);
            self.captured_generation = generation;
            self.layout_pending = false;
            self.last_edit = now;
        } else {
            if changed & (regions::BRUSH | regions::COMMANDS | regions::LAYOUT) != 0 {
                self.manager
                    .observe_working(native.session.workspace_working_state());
                self.last_edit = now;
            }
            if generation.is_some() && generation == self.captured_generation {
                self.layout_pending = false;
            }
        }
        if force || changed != 0 {
            self.status.dirty = self.manager.dirty();
        }
        Ok(())
    }
    fn adopt(
        &mut self,
        native: &mut NativeHost,
        incoming: StoredEntity,
        now: Instant,
    ) -> Result<()> {
        let prepared =
            PreparedWorkspace::new(incoming.entity.capture()?).map_err(StoreError::invalid)?;
        let revision = native.session.state().revision;
        let change = native
            .session
            .adopt_workspace(prepared)
            .map_err(StoreError::invalid)?;
        native.apply_change(revision, change);
        // Startup has no outgoing editable workspace. Switches will release
        // their old claim after adoption through a separate manager operation.
        self.manager.activate(incoming);
        self.sync_binding(native)?;
        native.session.set_workspace_read_only(false);
        native.take_service_changes();
        self.captured_generation = native.session.workspace_layout_generation();
        self.layout_pending = false;
        self.status.ready = true;
        self.status.busy = false;
        self.status.owner_lost = false;
        self.status.dirty = false;
        self.status.name = self.manager.active_name();
        self.status.error = None;
        self.last_renew = now;
        Ok(())
    }
    /// Called after accepted changes and on the host's idle service deadline.
    /// This never waits for a reply or repeatedly polls an unwoken future.
    pub(crate) fn poll(&mut self, native: &mut NativeHost, now: Instant, wall_ms: u64) {
        let previous = self.status.clone();
        if self.status.ready
            && !self.ui.active()
            && !self.status.busy
            && !self.status.close_requested
            && let Err(error) = self.observe(native, now, wall_ms, false)
        {
            self.status.error = Some(error.to_string());
        }
        if let Some(completion) = self.operation.poll() {
            match completion {
                Completion::Manager(result) => self.manager_completed(native, result, now),
                Completion::Toolbar(result) => self.toolbar_completed(native, result, now, wall_ms),
                Completion::Released => {}
                Completion::Open(Ok(incoming)) => self.incoming = Some(*incoming),
                Completion::Open(Err(error)) => {
                    self.status.busy = false;
                    self.status.error = Some(error.to_string());
                }
                Completion::Save(result) => {
                    self.status.busy = self.ownership.busy() && self.status.owner_lost;
                    self.status.saving = false;
                    self.status.dirty = self.manager.dirty();
                    match result {
                        Err(error) => self.status.error = Some(error.to_string()),
                        Ok(()) if !self.status.owner_lost => self.status.error = None,
                        Ok(()) => {}
                    }
                }
                Completion::Export(result) => {
                    self.status.busy = false;
                    self.status.notice = Some(match result {
                        Ok(()) => "Workspace backup saved.".into(),
                        Err(error) => error,
                    });
                }
                Completion::Close(result) => {
                    self.status.busy = false;
                    match result {
                        Ok(()) => {
                            self.status.close_ready = true;
                            self.status.dirty = false;
                            self.status.error = None;
                        }
                        Err(error) => self.status.error = Some(error.to_string()),
                    }
                }
            }
        }
        if self.incoming.is_some()
            && self.status.error.is_none()
            // Current document/brush readiness is sufficient. Optional catalog
            // shaders continue warming on the normal renderer startup path.
            && native.startup.brush_ready
            && native.session.require_workspace_idle().is_ok()
        {
            let incoming = self.incoming.take().unwrap();
            if let Err(error) = self.adopt(native, incoming.clone(), now) {
                // Retain its claim for retry/close instead of silently leaking
                // or replacing this accepted startup operation.
                self.incoming = Some(incoming);
                self.status.busy = false;
                self.status.error = Some(error.to_string());
            }
        }
        if let Some(result) = self.ownership.poll() {
            match result {
                Ok(()) => {
                    self.status.owner_lost = false;
                    if !self.status.close_requested {
                        self.status.busy = false;
                    }
                    native.session.set_workspace_read_only(false);
                }
                Err(error) => {
                    self.status.error = Some(error.to_string());
                    self.status.owner_lost =
                        matches!(error.kind, ErrorKind::OwnedElsewhere | ErrorKind::Conflict)
                            || !self.manager.lease_valid(wall_ms);
                    native
                        .session
                        .set_workspace_read_only(self.status.owner_lost);
                    self.status.busy = false;
                }
            }
        }
        if self.status.ready && !self.status.close_ready {
            if !self.manager.lease_valid(wall_ms) {
                if !self.status.owner_lost {
                    // An expired lease is checked before every input batch by
                    // the host. Cancel live input once, then require recovery.
                    let _ = native.input(layer_ui::UiInput::Blur);
                }
                self.status.owner_lost = true;
                native.session.set_workspace_read_only(true);
            }
            if !self.ownership.busy()
                && !self.status.close_requested
                && (now.duration_since(self.last_renew)
                    >= Duration::from_millis(layer_workspace::OWNER_RENEW_MS)
                    || (self.status.owner_lost && self.status.error.is_none()))
            {
                let manager = self.manager.clone();
                let expired = self.status.owner_lost;
                self.status.busy |= expired;
                let _ = self.ownership.start(async move {
                    if expired {
                        manager.revalidate_owner(wall_ms).await
                    } else {
                        manager.renew().await
                    }
                });
                self.last_renew = now;
            }
            if !self.ui.active() && !self.operation.busy() && !self.ownership.busy() {
                if self.status.close_requested && self.status.error.is_none() {
                    // Stop accepting editor mutations before taking this final
                    // snapshot. A prior immutable save may have newer edits.
                    let captured = self.observe(native, now, wall_ms, true);
                    if let Err(error) = captured {
                        self.status.busy = false;
                        self.status.error = Some(error.to_string());
                    } else {
                        self.status.busy = true;
                        let manager = self.manager.clone();
                        let _ = self
                            .operation
                            .start(async move { Completion::Close(manager.close().await) });
                    }
                } else if !self.status.owner_lost
                    && self.status.dirty
                    && self.status.error.is_none()
                    && (now.duration_since(self.last_edit) >= Duration::from_millis(250)
                        || now.duration_since(self.last_save) >= Duration::from_secs(2))
                {
                    self.status.saving = true;
                    self.last_save = now;
                    let manager = self.manager.clone();
                    let _ = self
                        .operation
                        .start(async move { Completion::Save(manager.save_once().await) });
                }
            }
        }
        self.poll_manager(native, now, wall_ms);
        if self.status.close_requested && !self.status.close_ready {
            self.status.busy =
                self.operation.busy() || self.ownership.busy() || self.incoming.is_some();
        }
        self.status.can_switch =
            self.accepts_input(wall_ms) && native.session.require_workspace_idle().is_ok();
        if self.status != previous {
            native.invalidate_snapshot();
        }
    }
    pub(crate) fn accepts_input(&self, wall_ms: u64) -> bool {
        self.status.ready
            && !self.ui.active()
            && !self.status.busy
            && !self.status.owner_lost
            && !self.status.close_requested
            && self.manager.lease_valid(wall_ms)
    }
    pub(crate) fn request_close(&mut self, native: &mut NativeHost) {
        if self.ui.active() && !self.ui.has_accepted_write() {
            self.close_manager(native);
        }
        self.status.close_attempt = self.status.close_attempt.saturating_add(1);
        self.status.close_requested = true;
        self.status.close_ready = false;
        if self.status.ready {
            self.status.error = None;
        }
        self.status.busy = self.status.ready || self.operation.busy() || self.incoming.is_some();
        native.invalidate_snapshot();
    }
    pub(crate) fn keep_open(&mut self, native: &mut NativeHost) {
        // A submitted close cannot be cancelled after its claim was released.
        if self.operation.busy() || self.status.close_ready {
            return;
        }
        self.status.close_requested = false;
        self.status.busy = false;
        native.session.reset_document_close();
        native.invalidate_snapshot();
    }
    pub(crate) fn retry(&mut self, native: &mut NativeHost, wall_ms: u64) {
        if self.operation.busy() || self.ownership.busy() {
            return;
        }
        self.status.error = None;
        if self.status.close_requested {
            self.status.close_attempt = self.status.close_attempt.saturating_add(1);
        }
        if !self.status.ready {
            if self.incoming.is_some() {
                self.status.busy = true;
            } else {
                self.start(wall_ms);
            }
        } else if self.manager.has_failed_operation() {
            let manager = self.manager.clone();
            self.status.busy = true;
            let _ = self.operation.start(async move {
                let outgoing = manager.current_record();
                match manager.retry_failed_operation().await {
                    Ok(Some(incoming)) => {
                        if let Some(outgoing) = outgoing
                            && outgoing.entity.id != incoming.entity.id
                        {
                            manager.release(&outgoing).await;
                        }
                        Completion::Open(Ok(Box::new(incoming)))
                    }
                    Ok(None) => Completion::Save(Ok(())),
                    Err(error) => Completion::Open(Err(error)),
                }
            });
        } else if self.status.owner_lost {
            let manager = self.manager.clone();
            self.status.busy = true;
            let _ = self.ownership.start(async move {
                manager.store.execute(StoreRequest::Reopen).await?;
                manager.revalidate_owner(wall_ms).await
            });
        }
        native.invalidate_snapshot();
    }
    pub(crate) fn stop(&mut self) {
        self.operation.close();
        self.ownership.close();
        self.ui.stop();
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WorkspaceAction {
    Manager { dialog: u64, command: ManagerInput },
    Retry,
    KeepOpen,
    DiscardClose,
    SaveAsNew { name: String },
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
    pub(crate) fn save_as_new(
        &mut self,
        native: &mut NativeHost,
        name: String,
        wall_ms: u64,
    ) -> Result<()> {
        self.require_available()?;
        layer_workspace::validate_name(&name)?;
        native
            .session
            .require_workspace_idle()
            .map_err(StoreError::invalid)?;
        let capture = native
            .session
            .capture_workspace()
            .map_err(StoreError::invalid)?;
        let manager = self.manager.clone();
        self.status.busy = true;
        self.status.error = None;
        let _ = self.operation.start(async move {
            let outgoing = manager.current_record();
            let result = manager.save_as_new(capture, &name, wall_ms).await;
            if result.is_ok()
                && let Some(outgoing) = outgoing
            {
                manager.release(&outgoing).await;
            }
            Completion::Open(result.map(Box::new))
        });
        native.invalidate_snapshot();
        Ok(())
    }
    fn require_available(&self) -> Result<()> {
        if self.operation.busy() || self.ownership.busy() {
            Err(StoreError::new(
                ErrorKind::Conflict,
                "Wait for the current workspace operation.",
            ))
        } else {
            Ok(())
        }
    }
    pub(crate) fn discard_close(&mut self, native: &mut NativeHost) -> Result<()> {
        self.require_available()?;
        if !self.status.close_requested {
            return Err(StoreError::invalid("Close the window first."));
        }
        let manager = self.manager.clone();
        let outgoing = self.incoming.take().or_else(|| manager.current_record());
        self.status.busy = true;
        self.status.error = None;
        let _ = self.operation.start(async move {
            if let Some(outgoing) = outgoing {
                manager.release(&outgoing).await;
            }
            Completion::Close(Ok(()))
        });
        native.invalidate_snapshot();
        Ok(())
    }
    pub(crate) fn export_backup(&mut self, native: &mut NativeHost, path: String) -> Result<()> {
        use std::{path::Path, sync::atomic::AtomicBool};
        self.require_available()?;
        crate::document_io::location(&path).map_err(StoreError::invalid)?;
        if !self.status.ready {
            return Err(StoreError::invalid(
                "Back up the original workspace database instead.",
            ));
        }
        self.observe(native, Instant::now(), now_ms(), true)?;
        let entity = self
            .manager
            .current()
            .ok_or_else(|| StoreError::invalid("No workspace is open."))?;
        let database = self.directory.join("workspaces.sqlite3");
        let job = crate::workspace_async::BlockingTask::start(move || {
            layer_workspace::validate_database_export_destination(&database, Path::new(&path))
                .map_err(|e| e.to_string())?;
            let bytes = layer_workspace::export_package(&entity).map_err(|e| e.to_string())?;
            crate::document_io::atomic_write(Path::new(&path), &AtomicBool::new(false), |file| {
                file.write_all(&bytes)
                    .map_err(|_| "Could not write the workspace backup.".into())
            })
        })
        .map_err(|_| {
            StoreError::new(ErrorKind::Unavailable, "Could not start workspace export.")
        })?;
        self.status.busy = true;
        self.status.notice = None;
        let _ = self
            .operation
            .start(async move { Completion::Export(job.await.and_then(|r| r)) });
        native.invalidate_snapshot();
        Ok(())
    }
}
impl WorkspaceService<layer_workspace::StoreWorker> {
    pub(crate) fn backup_database(&mut self, native: &mut NativeHost, path: String) -> Result<()> {
        self.require_available()?;
        crate::document_io::location(&path).map_err(StoreError::invalid)?;
        let manager = self.manager.clone();
        self.status.busy = true;
        self.status.notice = None;
        let _ = self.operation.start(async move {
            Completion::Export(
                manager
                    .store
                    .backup_database(std::path::Path::new(&path))
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            )
        });
        native.invalidate_snapshot();
        Ok(())
    }
}

#[cfg(test)]
#[path = "workspace_service_tests.rs"]
mod tests;
