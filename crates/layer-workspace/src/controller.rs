//! Toolkit-independent manager interaction and lifecycle. Hosts schedule `tick`
//! on their editor owner and render `view`; storage futures never borrow a UI
//! session and preview replies are fenced by a selection/lifetime generation.
use crate::*;
use layer_render::CanvasRenderer;
use layer_ui::{DockLayout, Platform, PreparedWorkspace, UiChange, UiSession, WorkspaceCapture};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

type Result<T> = std::result::Result<T, StoreError>;
struct Signal(std::sync::atomic::AtomicBool);
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}
struct Task<T> {
    future: Pin<Box<dyn Future<Output = Result<T>>>>,
    signal: Arc<Signal>,
}
impl<T> Task<T> {
    fn new(future: impl Future<Output = Result<T>> + 'static) -> Self {
        Self {
            future: Box::pin(future),
            signal: Arc::new(Signal(std::sync::atomic::AtomicBool::new(true))),
        }
    }
    fn poll(&mut self) -> Option<Result<T>> {
        if !self
            .signal
            .0
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return None;
        }
        match self
            .future
            .as_mut()
            .poll(&mut Context::from_waker(&Waker::from(self.signal.clone())))
        {
            Poll::Ready(r) => Some(r),
            Poll::Pending => None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceRow {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub options: bool,
    pub delete: bool,
}
#[derive(Clone, Serialize, Default)]
pub struct WorkspaceView {
    pub ready: bool,
    pub busy: bool,
    pub dirty: bool,
    pub retry: bool,
    pub id: Option<String>,
    pub name: String,
    pub page: Option<String>,
    pub title: String,
    pub intro: String,
    pub rows: Vec<WorkspaceRow>,
    pub selected: Option<String>,
    pub primary: String,
    pub enabled: bool,
    pub form: Option<WorkspaceForm>,
    pub error: Option<String>,
    pub focus_window: Option<String>,
    pub defaults: Vec<WorkspaceRow>,
}
#[derive(Clone, Serialize)]
pub struct WorkspaceForm {
    pub message: String,
    pub confirm: String,
    pub kind: String,
    pub title: String,
    pub name: String,
    pub id: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceInput {
    Open {
        page: String,
    },
    Select {
        id: Option<String>,
    },
    Filter {
        query: String,
    },
    Cancel,
    Confirm,
    Form {
        kind: String,
        id: Option<String>,
    },
    Submit {
        name: String,
        source: Option<String>,
    },
    Switch {
        id: String,
    },
    Retry,
    Suspend,
    Close,
    Resume,
}
enum Outcome {
    Adopt(StoredEntity),
    Done,
    Closed,
    Focus(String),
}
pub struct WorkspaceController<S: WorkspaceStore + 'static> {
    pub manager: Rc<WorkspaceManager<S>>,
    pub view: WorkspaceView,
    task: Option<Task<Outcome>>,
    incoming: Option<StoredEntity>,
    incoming_renew: Option<Task<StoredEntity>>,
    queued: Option<WorkspaceInput>,
    terminating: bool,
    renew: Option<Task<()>>,
    preview: Option<(u64, Task<DockLayout>)>,
    selection_generation: u64,
    query: String,
    generation: Option<u64>,
    last_renew: u64,
    last_edit: u64,
    last_observed: u64,
    preview_open: bool,
    transition: bool,
    suspended: bool,
    close_after_task: bool,
    legacy: Option<WorkspaceCapture>,
    source: String,
    legacy_error: Option<String>,
    binding_key: Option<String>,
    pending_binding: Option<layer_ui::ManagedWorkspace>,
}
impl<S: WorkspaceStore + 'static> WorkspaceController<S> {
    pub fn new(
        store: S,
        platform: Platform,
        source: String,
        legacy: Option<WorkspaceCapture>,
        now: u64,
    ) -> Self {
        Self::new_owned(store, platform, source, legacy, Owner::fresh(), now)
    }
    pub fn new_owned(
        store: S,
        platform: Platform,
        source: String,
        legacy: Option<WorkspaceCapture>,
        owner: Owner,
        now: u64,
    ) -> Self {
        let mut manager = WorkspaceManager::new(store, platform);
        manager.owner = owner;
        let mut c = Self {
            manager: Rc::new(manager),
            view: Default::default(),
            task: None,
            incoming: None,
            incoming_renew: None,
            queued: None,
            terminating: false,
            renew: None,
            preview: None,
            selection_generation: 0,
            query: String::new(),
            generation: None,
            last_renew: now,
            last_edit: now,
            last_observed: 0,
            preview_open: false,
            transition: false,
            suspended: false,
            close_after_task: false,
            legacy,
            source,
            legacy_error: None,
            binding_key: None,
            pending_binding: None,
        };
        c.initialize(now);
        c
    }
    fn initialize(&mut self, now: u64) {
        let m = self.manager.clone();
        let legacy = self.legacy.clone();
        let source = self.source.clone();
        let legacy_error = self.legacy_error.clone();
        self.task = Some(Task::new(async move {
            m.store.execute(StoreRequest::Reopen).await?;
            if let Some(error) = legacy_error {
                if !matches!(
                    m.store
                        .execute(StoreRequest::LegacyImport {
                            source: source.clone()
                        })
                        .await?,
                    StoreResponse::Binding(Some(_))
                ) {
                    return Err(StoreError::invalid(error));
                }
            }
            if let Some(legacy) = legacy {
                m.migrate_legacy_capture(&source, legacy, now).await?;
            }
            m.initialize_catalog(now).await?;
            let resume = m
                .store
                .execute(StoreRequest::Binding {
                    key: format!("window:{}", m.owner.id),
                })
                .await?;
            Ok(Outcome::Adopt(
                if let StoreResponse::Binding(Some(id)) = resume {
                    m.prepare_switch(&id, now).await?
                } else {
                    m.initialize(now).await?
                },
            ))
        }));
    }
    pub fn observe<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, now: u64) {
        if !self.view.ready || self.transition || self.suspended {
            return;
        }
        // The host calls this on accepted state changes, never pointer motion.
        if let Some(generation) = session.workspace_layout_generation() {
            if self.generation != Some(generation) {
                if let Ok(capture) = session.capture_workspace() {
                    self.manager.observe(capture, now);
                    self.generation = Some(generation);
                }
            } else {
                self.manager
                    .observe_working(session.workspace_working_state());
            }
            self.last_edit = now;
        }
    }
    pub fn legacy_error(&mut self, error: String, now: u64) {
        self.legacy_error = Some(error);
        self.initialize(now);
    }
    fn stop_preview<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>) -> UiChange {
        self.selection_generation = self.selection_generation.wrapping_add(1);
        self.preview = None;
        self.preview_open = false;
        session.cancel_workspace_layout_preview()
    }
    fn end_transition<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>) {
        self.transition = false;
        session.end_workspace_transition();
    }
    fn start_transition<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>) -> Result<()> {
        if !self.transition {
            session
                .begin_workspace_transition()
                .map_err(StoreError::invalid)?;
            self.transition = true;
        }
        Ok(())
    }
    fn rows(&mut self, now: u64) {
        self.view.id = self.manager.active_id();
        self.view.name = self.manager.active_name().unwrap_or_default();
        self.view.defaults = DEFAULT_WORKSPACES
            .iter()
            .map(|(id, preset)| WorkspaceRow {
                id: (*id).into(),
                title: self
                    .manager
                    .items()
                    .iter()
                    .find(|i| i.id == *id)
                    .map(|i| i.metadata.name.clone())
                    .unwrap_or_else(|| preset.name().into()),
                subtitle: String::new(),
                options: false,
                delete: false,
            })
            .collect();
        let page = self.view.page.as_deref();
        self.view.title = match page {
            Some("history") => format!("Layout History — {}", self.view.name),
            _ => "Workspaces".into(),
        };
        self.view.intro = match page {
            Some("workspaces") => {
                "Workspaces save your tool settings and layout for different tasks."
            }
            _ => "",
        }
        .into();
        self.view.primary = match page {
            Some("history") => "Restore This Version",
            _ => "Switch to Workspace",
        }
        .into();
        self.view.rows = if page == Some("history") {
            self.manager
                .current()
                .and_then(|e| e.capture().ok())
                .map(|capture| {
                    let mut revisions: Vec<_> = capture.history.revisions.into_values().collect();
                    revisions.sort_by_key(|r| std::cmp::Reverse(r.timestamp_ms));
                    revisions
                        .into_iter()
                        .filter(|r| {
                            r.description
                                .to_lowercase()
                                .contains(&self.query.to_lowercase())
                        })
                        .map(|r| WorkspaceRow {
                            id: r.id,
                            title: r.description,
                            subtitle: date(r.timestamp_ms),
                            options: false,
                            delete: false,
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            self.manager
                .rows(ManagerPage::Workspaces, &self.query, now)
                .into_iter()
                .map(|r| {
                    let actions = self
                        .manager
                        .items()
                        .iter()
                        .find(|i| i.id == r.id)
                        .map(|i| self.manager.summary_actions(i, true, now))
                        .unwrap_or_default();
                    let options = actions
                        .iter()
                        .any(|b| b.enabled && matches!(b.action, ManagerAction::Rename(_)));
                    let delete = actions
                        .iter()
                        .any(|b| b.enabled && matches!(b.action, ManagerAction::Delete(_)));
                    WorkspaceRow {
                        id: r.id,
                        title: r.title,
                        subtitle: r.subtitle,
                        options,
                        delete,
                    }
                })
                .collect()
        };
        let current = if page == Some("history") {
            self.manager
                .current()
                .and_then(|e| e.capture().ok())
                .map(|c| c.history.current)
        } else {
            self.manager.active_id()
        };
        self.view.enabled = self.preview.is_none()
            && self.view.selected.as_ref().is_some_and(|id| {
                self.view.rows.iter().any(|r| &r.id == id) && current.as_ref() != Some(id)
            });
        if page == Some("workspaces")
            && self.view.selected.as_ref().is_some_and(|id| {
                self.manager.items().iter().any(|i| {
                    &i.id == id
                        && i.claim
                            .as_ref()
                            .is_some_and(|c| c.owner != self.manager.owner && c.expires_at_ms > now)
                })
            })
        {
            self.view.primary = "Switch to Window".into();
        }
    }
    fn select<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        id: Option<String>,
    ) -> Result<UiChange> {
        let change = self.stop_preview(session);
        self.view.selected = id.clone();
        self.view.enabled = false;
        let Some(id) = id else {
            return Ok(change);
        };
        session
            .begin_workspace_layout_preview()
            .map_err(StoreError::invalid)?;
        self.preview_open = true;
        let m = self.manager.clone();
        let page = self.view.page.clone();
        self.preview = Some((
            self.selection_generation,
            Task::new(async move {
                if page.as_deref() == Some("history") {
                    return m
                        .current()
                        .and_then(|e| e.capture().ok())
                        .and_then(|c| c.history.revisions.get(&id).map(|r| r.layout.clone()))
                        .ok_or_else(|| {
                            StoreError::invalid("This layout version is no longer retained.")
                        });
                }
                let stored = if m.active_id().as_deref() == Some(&id) {
                    m.current_record().unwrap()
                } else {
                    m.load(&id).await?
                };
                m.details(&stored, true, 0)
                    .preview
                    .ok_or_else(|| StoreError::invalid("This item has no layout."))
            }),
        ));
        Ok(change)
    }
    pub fn input<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        input: WorkspaceInput,
        now: u64,
    ) -> Result<UiChange> {
        if matches!(input, WorkspaceInput::Submit { .. })
            && self.manager.has_failed_operation()
            && self
                .view
                .form
                .as_ref()
                .is_some_and(|form| form.kind != "recover")
        {
            return self.input(session, WorkspaceInput::Retry, now);
        }
        if !matches!(input, WorkspaceInput::Resume) {
            self.view.error = if matches!(input, WorkspaceInput::Cancel) {
                self.manager.error().map(|error| error.to_string())
            } else {
                None
            };
        }
        self.view.focus_window = None;
        let mut change = UiChange::default();
        match input {
            WorkspaceInput::Cancel => {
                self.queued = None;
                change = self.stop_preview(session);
                if self.view.form.take().is_none() {
                    self.view.page = None;
                }
                if self.view.page.is_none() && self.task.is_none() {
                    self.end_transition(session);
                } else if self.view.page.is_some() && self.task.is_none() {
                    change = self.select(session, self.view.selected.clone())?;
                }
            }
            WorkspaceInput::Suspend | WorkspaceInput::Close => {
                self.terminating |= matches!(input, WorkspaceInput::Close);
                self.queued = None;
                self.observe(session, now);
                change = self.stop_preview(session);
                self.view.page = None;
                self.view.form = None;
                if self.task.is_none() {
                    self.end_transition(session);
                }
                self.suspended = true;
                session.set_workspace_read_only(true);
                self.close_after_task = true;
            }
            WorkspaceInput::Resume => {
                self.suspended = false;
                session.set_workspace_read_only(true);
                self.close_after_task = false;
                self.last_renew = 0;
            }
            WorkspaceInput::Select { id } => {
                if self.view.page.is_some() && self.view.form.is_none() {
                    change = self.select(session, id)?;
                }
            }
            WorkspaceInput::Filter { query } => {
                self.query = query;
                self.rows(now);
                if self
                    .view
                    .selected
                    .as_ref()
                    .is_some_and(|id| !self.view.rows.iter().any(|r| &r.id == id))
                {
                    change = self.select(session, None)?;
                }
            }
            input => {
                if matches!(input, WorkspaceInput::Retry)
                    && self.incoming.is_some()
                    && self.task.is_none()
                {
                    // Adoption may have failed after publication. Retry that
                    // validated capture without queuing behind itself.
                    self.view.error = None;
                    return Ok(change);
                }
                if self.task.is_some() || self.incoming.is_some() {
                    if matches!(
                        input,
                        WorkspaceInput::Open { .. }
                            | WorkspaceInput::Form { .. }
                            | WorkspaceInput::Switch { .. }
                            | WorkspaceInput::Retry
                    ) {
                        self.queued = Some(input);
                        return Ok(change);
                    }
                    return Err(StoreError::invalid(
                        "Wait for the current workspace operation to finish.",
                    ));
                }
                self.observe(session, now);
                match input {
                    WorkspaceInput::Open { page } => {
                        self.start_transition(session)?;
                        change = self.stop_preview(session);
                        self.query.clear();
                        self.view.form = None;
                        self.view.page = Some(page.clone());
                        let id = if page == "workspaces" {
                            self.manager.active_id()
                        } else if page == "history" {
                            self.manager
                                .current()
                                .and_then(|e| e.capture().ok())
                                .map(|c| c.history.current)
                        } else {
                            None
                        };
                        self.view.selected = id.clone();
                        let m = self.manager.clone();
                        self.task = Some(Task::new(async move {
                            m.refresh().await?;
                            Ok(Outcome::Done)
                        }));
                    }
                    WorkspaceInput::Form { kind, id } => {
                        self.start_transition(session)?;
                        change = self.stop_preview(session);
                        let name = if kind == "new" {
                            "New Workspace".into()
                        } else if kind == "recover" {
                            format!("{} Recovered", self.view.name)
                        } else {
                            self.manager
                                .items()
                                .iter()
                                .find(|i| Some(&i.id) == id.as_ref())
                                .map(|i| i.metadata.name.clone())
                                .unwrap_or_default()
                        };
                        let title: String = match kind.as_str() {
                            "new" => "New Workspace",
                            "rename" => "Rename",
                            "delete" => "Delete",
                            "reset" => "Restore Starting Layout",
                            "reset_brushes" => "Reset All Brushes?",
                            _ => "Save as New Workspace",
                        }
                        .into();
                        self.view.form = Some(WorkspaceForm {
                            message: match kind.as_str() {
                                "reset_brushes" => "Restore every brush’s settings in this workspace to their defaults.".into(),
                                "new" => "Copy your current tool settings and layout into a new workspace.".into(),
                                "delete" => format!("Delete “{name}”?"),
                                "reset" => format!("Restore “{}” to its starting layout?", self.view.name),
                                _ => String::new(),
                            },
                            confirm: match kind.as_str() { "reset_brushes" => "Reset Brushes".into(), "new" => "Create and Switch".into(), _ => title.clone() },
                            kind,
                            title,
                            name,
                            id,
                        });
                    }
                    WorkspaceInput::Submit { name, source: _ } => {
                        let form =
                            self.view.form.clone().ok_or_else(|| {
                                StoreError::invalid("Open a workspace dialog first.")
                            })?;
                        if !matches!(form.kind.as_str(), "delete" | "reset" | "reset_brushes") {
                            validate_name(&name)?;
                        }
                        change = self.stop_preview(session);
                        self.start_transition(session)?;
                        let m = self.manager.clone();
                        if form.kind == "reset_brushes" {
                            let reset = session
                                .reset_workspace_brushes()
                                .map_err(StoreError::invalid)?;
                            change.regions |= reset.regions;
                            change.revision = reset.revision;
                            m.observe_working(session.workspace_working_state());
                            self.task = Some(Task::new(async move {
                                m.flush().await?;
                                Ok(Outcome::Done)
                            }));
                            self.rows(now);
                            self.view.busy = true;
                            return Ok(change);
                        }
                        let capture = session.capture_workspace().map_err(StoreError::invalid)?;
                        self.task = Some(Task::new(async move {
                            Ok(match form.kind.as_str() {
                                "new" => Outcome::Adopt(
                                    m.create_workspace(&name, None, false, now).await?,
                                ),
                                "rename" => {
                                    let id = form
                                        .id
                                        .ok_or_else(|| StoreError::invalid("Choose an item."))?;
                                    let description =
                                        m.load(&id).await?.entity.metadata.description;
                                    m.rename(&id, &name, &description, now).await?;
                                    Outcome::Done
                                }
                                "delete" => match m
                                    .delete_item(
                                        &form.id.ok_or_else(|| {
                                            StoreError::invalid("Choose an item.")
                                        })?,
                                        None,
                                        now,
                                    )
                                    .await?
                                {
                                    Some(s) => Outcome::Adopt(s),
                                    None => Outcome::Done,
                                },
                                "reset" => Outcome::Adopt(
                                    m.change_layout(
                                        &m.active_id().ok_or_else(|| {
                                            StoreError::invalid("Open a workspace.")
                                        })?,
                                        None,
                                        now,
                                    )
                                    .await?,
                                ),
                                _ => Outcome::Adopt(m.save_as_new(capture, &name, now).await?),
                            })
                        }));
                    }
                    WorkspaceInput::Confirm | WorkspaceInput::Switch { .. } => {
                        let id = if let WorkspaceInput::Switch { id } = &input {
                            id.clone()
                        } else {
                            if !self.view.enabled {
                                return Ok(change);
                            }
                            self.view
                                .selected
                                .clone()
                                .ok_or_else(|| StoreError::invalid("Choose an item."))?
                        };
                        if matches!(input, WorkspaceInput::Switch { .. })
                            && self.manager.active_id().as_deref() == Some(&id)
                        {
                            return Ok(change);
                        }
                        if self.view.primary == "Switch to Window"
                            && !matches!(input, WorkspaceInput::Switch { .. })
                        {
                            self.view.focus_window = Some(id);
                            return Ok(change);
                        }
                        let page = if matches!(input, WorkspaceInput::Switch { .. }) {
                            "workspaces".into()
                        } else {
                            self.view.page.clone().unwrap_or_default()
                        };
                        change = self.stop_preview(session);
                        self.start_transition(session)?;
                        let m = self.manager.clone();
                        self.task =
                            Some(Task::new(async move {
                                if page != "history" {
                                    let stored = m.load(&id).await?;
                                    if stored.claim.as_ref().is_some_and(|c| {
                                        c.owner != m.owner && c.expires_at_ms > now
                                    }) {
                                        return Ok(Outcome::Focus(id));
                                    }
                                }
                                Ok(Outcome::Adopt(match page.as_str() {
                                    "history" => {
                                        m.change_layout(
                                            &m.active_id().ok_or_else(|| {
                                                StoreError::invalid("Open a workspace.")
                                            })?,
                                            Some(&id),
                                            now,
                                        )
                                        .await?
                                    }
                                    _ => m.prepare_switch(&id, now).await?,
                                }))
                            }));
                    }
                    WorkspaceInput::Retry => {
                        if self.incoming.is_some() {
                            // Retry validated adoption without re-publishing it.
                        } else if !self.view.ready {
                            self.initialize(now);
                        } else {
                            let m = self.manager.clone();
                            self.start_transition(session)?;
                            self.task = Some(Task::new(async move {
                                m.store.execute(StoreRequest::Reopen).await?;
                                m.revalidate_owner(now).await?;
                                m.flush().await?;
                                Ok(match m.retry_failed_operation().await? {
                                    Some(s) => Outcome::Adopt(s),
                                    None => Outcome::Done,
                                })
                            }));
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        self.rows(now);
        self.view.busy = self.task.is_some() || self.incoming.is_some();
        Ok(change)
    }
    pub fn tick<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, now: u64) -> UiChange {
        let mut change = UiChange::default();
        let mut presentation_changed = false;
        if !self.view.ready {
            session.set_workspace_read_only(true);
        }
        if let Some(result) = self.task.as_mut().and_then(Task::poll) {
            self.task = None;
            presentation_changed = true;
            match result {
                Ok(outcome) => {
                    let adopting = matches!(outcome, Outcome::Adopt(_));
                    if let Outcome::Adopt(incoming) = outcome {
                        self.incoming = Some(incoming);
                    } else if let Outcome::Focus(id) = outcome {
                        self.view.focus_window = Some(id);
                    } else if matches!(outcome, Outcome::Closed) {
                        self.close_after_task = false;
                    }
                    if !adopting {
                        self.view.form = None;
                        if self.view.page.is_some() && !self.suspended {
                            match self.select(session, self.view.selected.clone()) {
                                Ok(c) => change = c,
                                Err(e) => self.view.error = Some(e.to_string()),
                            }
                        }
                    }
                    if self.view.page.is_none() && self.incoming.is_none() {
                        self.end_transition(session);
                    }
                    self.pending_binding = self.manager.binding();
                }
                Err(e) => {
                    self.view.error = Some(e.to_string());
                    if self.view.page.is_none() && self.view.form.is_none() {
                        self.end_transition(session);
                    }
                    self.close_after_task = false;
                }
            }
        }
        if let Some(result) = self.incoming_renew.as_mut().and_then(Task::poll) {
            self.incoming_renew = None;
            match result {
                Ok(incoming) => self.incoming = Some(incoming),
                Err(e) => self.view.error = Some(e.to_string()),
            }
        }
        if self.incoming.is_some()
            && self.terminating
            && self.task.is_none()
            && self.incoming_renew.is_none()
        {
            let incoming = self.incoming.take().unwrap();
            let m = self.manager.clone();
            self.task = Some(Task::new(async move {
                m.release(&incoming).await;
                m.close().await?;
                Ok(Outcome::Closed)
            }));
        }
        if let Some(incoming) = &self.incoming {
            if !self.terminating
                && self.incoming_renew.is_none()
                && self.view.error.is_none()
                && incoming
                    .claim
                    .as_ref()
                    .is_none_or(|c| c.expires_at_ms <= now.saturating_add(OWNER_RENEW_MS))
            {
                let m = self.manager.clone();
                let original = incoming.clone();
                self.incoming_renew = Some(Task::new(async move {
                    let claimed = m.claim(&original.entity.id).await?;
                    if claimed.generations != original.generations {
                        m.release(&claimed).await;
                        return Err(StoreError::conflict());
                    }
                    Ok(claimed)
                }));
            }
        }
        if self.incoming.is_some()
            && !self.terminating
            && self.incoming_renew.is_none()
            && self.view.error.is_none()
            && session.require_workspace_idle().is_ok()
        {
            presentation_changed = true;
            let incoming = self.incoming.take().unwrap();
            let prepared = incoming
                .entity
                .capture()
                .and_then(|c| PreparedWorkspace::new(c).map_err(StoreError::invalid));
            match prepared.and_then(|p| session.adopt_workspace(p).map_err(StoreError::invalid)) {
                Ok(c) => {
                    change = c;
                    let id = incoming.entity.id.clone();
                    if let Some(outgoing) = self.manager.activate(incoming) {
                        if outgoing.entity.id != id {
                            let m = self.manager.clone();
                            self.task = Some(Task::new(async move {
                                m.release(&outgoing).await;
                                Ok(Outcome::Done)
                            }));
                        }
                    }
                    self.view.ready = true;
                    self.view.error = None;
                    self.view.page = None;
                    self.view.form = None;
                    self.generation = session.workspace_layout_generation();
                    session.set_workspace_read_only(self.suspended);
                    self.end_transition(session);
                    self.pending_binding = self.manager.binding();
                }
                Err(e) => {
                    self.incoming = Some(incoming);
                    self.view.error = Some(e.to_string());
                }
            }
        }
        if let Some((generation, result)) = self
            .preview
            .as_mut()
            .and_then(|(g, task)| task.poll().map(|r| (*g, r)))
        {
            self.preview = None;
            presentation_changed = true;
            if generation == self.selection_generation
                && self.preview_open
                && self.view.page.is_some()
            {
                match result.and_then(|layout| {
                    session
                        .preview_workspace_layout(&layout)
                        .map_err(StoreError::invalid)
                }) {
                    Ok(c) => change = c,
                    Err(e) => {
                        self.view.error = Some(e.to_string());
                        self.view.selected = None;
                    }
                }
            }
        }
        if let Some(result) = self.renew.as_mut().and_then(Task::poll) {
            self.renew = None;
            presentation_changed = true;
            match result {
                Ok(()) => {
                    session.set_workspace_read_only(self.suspended);
                }
                Err(e) => {
                    session.set_workspace_read_only(true);
                    self.view.error = Some(e.to_string());
                }
            }
        }
        if self.view.ready
            && !self.suspended
            && self.task.is_none()
            && self.renew.is_none()
            && (self.last_renew == 0 || now.saturating_sub(self.last_renew) >= OWNER_RENEW_MS)
        {
            self.last_renew = now;
            let m = self.manager.clone();
            if !m.lease_valid(now) {
                session.set_workspace_read_only(true);
            }
            self.renew = Some(Task::new(async move { m.revalidate_owner(now).await }));
        }
        if self.view.ready && self.task.is_none() && self.incoming.is_none() {
            if self.close_after_task && self.renew.is_none() {
                let m = self.manager.clone();
                self.task = Some(Task::new(async move {
                    m.close().await?;
                    Ok(Outcome::Closed)
                }));
            } else if !self.transition
                && !self.suspended
                && self.manager.dirty()
                && self.view.error.is_none()
                && (now.saturating_sub(self.last_edit) >= 250
                    || now.saturating_sub(self.last_observed) >= 2000)
            {
                self.last_observed = now;
                let m = self.manager.clone();
                self.task = Some(Task::new(async move {
                    m.save_once().await?;
                    Ok(Outcome::Done)
                }));
            }
        }
        if self.task.is_none() && self.incoming.is_none() && !self.terminating {
            if let Some(input) = self.queued.take() {
                match self.input(session, input, now) {
                    Ok(c) => {
                        change.regions |= c.regions;
                        change.canvas_wake |= c.canvas_wake;
                        change.revision = change.revision.max(c.revision);
                    }
                    Err(e) => self.view.error = Some(e.to_string()),
                }
            }
        }
        if presentation_changed {
            self.rows(now);
        }
        // Autosave acknowledgements do not change menus. Reconfiguring them
        // during a resize refreshes command state and invalidates retained tool
        // controls. Publish actual catalog changes only at an idle boundary.
        if self.pending_binding.is_some() && session.require_workspace_idle().is_ok() {
            let binding = self.pending_binding.take().unwrap();
            if let Ok(key) = serde_json::to_string(&binding) {
                if self.binding_key.as_ref() != Some(&key) {
                    match session.configure_workspace_manager(binding) {
                        Ok(c) => {
                            self.binding_key = Some(key);
                            change.regions |= c.regions;
                            change.revision = c.revision;
                        }
                        Err(e) => self.view.error = Some(e),
                    }
                }
            }
        }
        self.view.busy = self.task.is_some() || self.incoming.is_some();
        self.view.dirty = self.manager.dirty();
        self.view.retry = self.view.error.is_some()
            && (self.manager.has_failed_operation() || self.manager.dirty());
        change
    }
}
