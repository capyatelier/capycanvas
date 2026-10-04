//! Toolkit-independent manager interaction and lifecycle. Hosts schedule `tick`
//! on their editor owner and render `view`; storage futures never borrow a UI
//! session and preview replies are fenced by a selection/lifetime generation.
use crate::*;
use layer_render::CanvasRenderer;
use layer_ui::{
    CustomizationAction, DockLayout, HostRequestKind, Panel, PanelConfig, Platform,
    PreparedWorkspace, UiAction, UiChange, UiSession, WorkspaceCommand, regions,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

type Result<T> = std::result::Result<T, StoreError>;
type WakeSlot = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
/// Accepted editor changes that can alter the saved layout or working state.
pub const OBSERVED_REGIONS: u32 = regions::LAYOUT
    | regions::BRUSH
    | regions::DOCUMENT
    | regions::COMMANDS
    | regions::CUSTOMIZATION;
struct Signal {
    ready: AtomicBool,
    wake: WakeSlot,
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        if let Ok(slot) = self.wake.lock()
            && let Some(wake) = slot.as_ref()
        {
            wake();
        }
    }
}
fn merge(into: &mut UiChange, change: UiChange) {
    into.regions |= change.regions;
    into.canvas_wake |= change.canvas_wake;
    into.revision = into.revision.max(change.revision);
}
struct Task<T> {
    future: Pin<Box<dyn Future<Output = Result<T>>>>,
    signal: Arc<Signal>,
}
impl<T> Task<T> {
    fn new(wake: &WakeSlot, future: impl Future<Output = Result<T>> + 'static) -> Self {
        Self {
            future: Box::pin(future),
            signal: Arc::new(Signal {
                ready: AtomicBool::new(true),
                wake: wake.clone(),
            }),
        }
    }
    fn poll(&mut self) -> Option<Result<T>> {
        if !self.signal.ready.swap(false, Ordering::AcqRel) {
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
/// Another window owns this workspace; the host brings that window forward.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FocusTarget {
    pub id: String,
    pub owner: String,
}
#[derive(Clone, Serialize, Default)]
pub struct WorkspaceView {
    pub ready: bool,
    pub busy: bool,
    pub loading: bool,
    pub dirty: bool,
    pub saving: bool,
    pub retry: bool,
    pub owner_lost: bool,
    pub closing: bool,
    pub closed: bool,
    pub id: Option<String>,
    pub name: String,
    pub owner: String,
    pub page: Option<ManagerPage>,
    pub title: String,
    pub intro: String,
    pub query: String,
    pub rows: Vec<WorkspaceRow>,
    pub selected: Option<String>,
    pub details: Option<ManagerDetails>,
    pub primary: String,
    pub enabled: bool,
    pub prompt: Option<ManagerPrompt>,
    pub prompt_action: Option<ManagerAction>,
    pub error: Option<String>,
    pub interrupted: usize,
    pub focus_window: Option<FocusTarget>,
    /// Persistently pinned choices, used by the manager's visibility controls.
    pub switcher: Vec<WorkspaceRow>,
    /// Header choices, including the current workspace when it is unpinned.
    pub switcher_display: Vec<WorkspaceRow>,
    pub switcher_options_label: String,
    pub switcher_options: layer_ui::ContextMenu,
    pub switcher_menu: layer_ui::ContextMenu,
    pub order: Vec<String>,
    pub switcher_busy: bool,
    pub switcher_error: Option<String>,
    pub switcher_revision: u64,
}
#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceInput {
    Open {
        page: ManagerPage,
    },
    Select {
        id: Option<String>,
    },
    Search {
        query: String,
    },
    Cancel,
    Dismiss,
    Confirm,
    Form {
        action: ManagerAction,
    },
    Action {
        action: ManagerAction,
    },
    Submit {
        #[serde(default)]
        name: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        choice: Option<String>,
    },
    Switch {
        id: String,
    },
    EditSwitcher {
        edit: SwitcherEdit,
    },
    RefreshSwitcher,
    Retry,
    Suspend,
    Close,
    Resume,
    DiscardClose,
    Detach,
    FocusFailed {
        error: String,
    },
    Import {
        text: String,
        kind: PackageKind,
    },
}
struct Install {
    config: PanelConfig,
    replace: Option<Panel>,
    group: Option<u32>,
    exact_name: bool,
}
enum Outcome {
    Adopt(Box<StoredEntity>),
    Editing(Option<Box<StoredEntity>>),
    Done,
    Dismiss,
    Closed,
    Focus(FocusTarget),
    Install(Box<Install>),
    Show(ManagerPage, String),
}
impl Outcome {
    fn adopt(entity: StoredEntity) -> Self {
        Self::Adopt(Box::new(entity))
    }
}
/// Any failure before the first adoption moves startup to the next stage. The
/// last stage reads nothing that was stored, so startup always ends adopted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Startup {
    Stored,
    Replaced,
    InMemory,
}
impl Startup {
    fn next(self) -> Option<Self> {
        match self {
            Self::Stored => Some(Self::Replaced),
            Self::Replaced => Some(Self::InMemory),
            Self::InMemory => None,
        }
    }
}
enum WorkspaceNoticeCopy {
    Reset,
    Volatile(StoreError),
    SwitcherError(StoreError),
}
impl WorkspaceNoticeCopy {
    fn text(&self, localization: &layer_ui::Localizer) -> String {
        match self {
            Self::SwitcherError(error) => error.localized_message(localization),
            Self::Reset => localization.text(layer_ui::MessageId::WORKSPACE_STORAGE_RESET_NOTICE).to_string(),
            Self::Volatile(error) => message(localization, layer_ui::MessageId::WORKSPACE_STORAGE_VOLATILE_NOTICE,
                &[("detail", error.localized_message(localization))]),
        }
    }
}
struct WorkspaceNotice {
    id: u64,
    generation: u64,
    copy: WorkspaceNoticeCopy,
}
type Selection = (Option<StoredEntity>, Option<DockLayout>);
pub struct WorkspaceController<S: WorkspaceStore + 'static> {
    localization: Arc<layer_ui::Localizer>,
    localization_generation: u64,
    pub manager: Rc<WorkspaceManager<S>>,
    pub view: WorkspaceView,
    wake: WakeSlot,
    task: Option<Task<Outcome>>,
    quiet: bool,
    preferences: Option<Task<()>>,
    preferences_edited: bool,
    preference_edits: VecDeque<SwitcherEdit>,
    refresh_preferences: bool,
    incoming: Option<StoredEntity>,
    incoming_renew: Option<Task<StoredEntity>>,
    install: Option<Box<Install>>,
    queued: Option<WorkspaceInput>,
    terminating: bool,
    discard: bool,
    resume_key: Option<String>,
    renew: Option<Task<Vec<manager::InterruptedChange>>>,
    preview: Option<(u64, Task<Selection>)>,
    interrupted: Vec<manager::InterruptedChange>,
    selection_generation: u64,
    generation: Option<u64>,
    last_renew: u64,
    last_edit: u64,
    last_observed: u64,
    preview_open: bool,
    transition: bool,
    suspended: bool,
    close_after_task: bool,
    binding_key: Option<String>,
    pending_binding: Option<layer_ui::ManagedWorkspace>,
    selected_elsewhere: bool,
    error: Option<StoreError>,
    switcher_error: Option<StoreError>,
    renew_error: Option<StoreError>,
    details_source: Option<StoredEntity>,
    prompt_source: Option<Metadata>,
    routed: UiChange,
    startup: Option<Startup>,
    startup_error: Option<StoreError>,
    notice: Option<WorkspaceNotice>,
}
impl<S: WorkspaceStore + 'static> WorkspaceController<S> {
    pub fn new_localized(store: S, platform: Platform, now: u64, localization: Arc<layer_ui::Localizer>) -> Self {
        Self::new_owned_localized(store, platform, Owner::fresh(), now, localization)
    }
    pub fn localization(&self) -> &Arc<layer_ui::Localizer> {
        &self.localization
    }
    pub fn set_localization(&mut self, localization: Arc<layer_ui::Localizer>) -> bool {
        if self.localization.language() == localization.language() {
            return false;
        }
        self.manager.set_localization(localization.clone());
        self.localization = localization;
        self.localization_generation = self.localization_generation.wrapping_add(1);
        let now = self.manager.clock.get();
        self.refresh_details(now);
        if let (Some(action), Some(previous)) = (&self.view.prompt_action, &self.view.prompt) {
            let refreshed = match action {
                ManagerAction::RecoverInterrupted => recover_prompt(&self.localization, self.interrupted_choices()),
                _ => self.manager.form_prompt(action, self.prompt_source.as_ref()),
            };
            if let Ok(mut prompt) = refreshed {
                prompt.name = previous.name.clone();
                prompt.description = previous.description.clone();
                prompt.selected = previous.selected.clone();
                self.view.prompt = Some(prompt);
            }
        }
        if let Some(error) = &self.error {
            self.view.error = Some(error.localized_message(&self.localization));
        }
        if let Some(error) = &self.switcher_error {
            self.view.switcher_error = Some(error.localized_message(&self.localization));
        }
        self.pending_binding = self.manager.binding();
        self.present(now);
        true
    }
    pub fn set_error(&mut self, error: StoreError) {
        self.view.error = Some(error.localized_message(&self.localization));
        self.error = Some(error);
    }
    fn clear_error(&mut self) {
        self.view.error = None;
        self.error = None;
    }
    fn set_switcher_error(&mut self, error: Option<StoreError>) {
        self.view.switcher_error = error.as_ref().map(|error| error.localized_message(&self.localization));
        self.switcher_error = error;
    }
    fn interrupted_choices(&self) -> Vec<(String, String)> {
        self.interrupted.iter().map(|change| (change.id.clone(), change.label(&self.localization))).collect()
    }
    fn refresh_details(&mut self, now: u64) {
        self.view.details = if self.view.page == Some(ManagerPage::ThisWorkspace) {
            self.view.selected.as_ref().and_then(|id| serde_json::from_str(id).ok())
                .and_then(|panel| self.manager.toolbar_details(panel, true).ok())
        } else {
            self.details_source.as_ref().map(|stored| {
                let mut details = self.manager.details(stored, true, now);
                details.preview = None;
                details
            })
        };
    }
    pub fn new_owned_localized(store: S, platform: Platform, owner: Owner, now: u64, localization: Arc<layer_ui::Localizer>) -> Self {
        let mut manager = WorkspaceManager::new_localized(store, platform, localization.clone());
        manager.owner = owner;
        manager.clock.set(now);
        let mut c = Self {
            localization,
            localization_generation: 0,
            manager: Rc::new(manager),
            view: Default::default(),
            wake: Default::default(),
            task: None,
            quiet: false,
            preferences: None,
            preferences_edited: false,
            preference_edits: VecDeque::new(),
            refresh_preferences: false,
            incoming: None,
            incoming_renew: None,
            install: None,
            queued: None,
            terminating: false,
            discard: false,
            resume_key: None,
            renew: None,
            preview: None,
            interrupted: Vec::new(),
            selection_generation: 0,
            generation: None,
            last_renew: now,
            last_edit: now,
            last_observed: 0,
            preview_open: false,
            transition: false,
            suspended: false,
            close_after_task: false,
            binding_key: None,
            pending_binding: None,
            selected_elsewhere: false,
            error: None,
            switcher_error: None,
            renew_error: None,
            details_source: None,
            prompt_source: None,
            routed: UiChange::default(),
            startup: None,
            startup_error: None,
            notice: None,
        };
        c.view.owner = c.manager.owner.id.clone();
        c.initialize(now);
        c
    }
    /// Scene restoration identity: startup prefers the workspace bound to
    /// `key` over this window's binding, and every adoption rebinds it.
    pub fn with_resume_key(mut self, key: String, now: u64) -> Self {
        self.resume_key = Some(key);
        self.initialize(now);
        self
    }
    /// Storage replies call `wake` from any thread; the host then schedules
    /// `tick` on its editor owner.
    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(wake);
        }
    }
    /// After `stop` returns, no storage reply calls the wake callback.
    pub fn stop(&mut self) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = None;
        }
    }
    fn spawn<T>(&self, future: impl Future<Output = Result<T>> + 'static) -> Option<Task<T>> {
        Some(Task::new(&self.wake, future))
    }
    fn run(&mut self, future: impl Future<Output = Result<Outcome>> + 'static) {
        self.task = self.spawn(future);
        self.quiet = false;
    }
    /// Autosaves and releases keep editing available and are not shown as busy.
    fn run_quietly(&mut self, future: impl Future<Output = Result<Outcome>> + 'static) {
        self.task = self.spawn(future);
        self.quiet = true;
    }
    fn initialize(&mut self, now: u64) {
        self.start(Startup::Stored, now);
    }
    fn start(&mut self, stage: Startup, now: u64) {
        self.startup = Some(stage);
        self.incoming = None;
        self.incoming_renew = None;
        let m = self.manager.clone();
        let keys: Vec<String> = self
            .resume_key
            .iter()
            .cloned()
            .chain([format!("window:{}", m.owner.id)])
            .collect();
        self.run(async move {
            match stage {
                Startup::Stored => {}
                Startup::Replaced => m.replace_storage().await?,
                Startup::InMemory => m.use_memory(),
            }
            m.execute(StoreRequest::Reopen).await?;
            m.load_editing().await?;
            m.refresh_switcher().await?;
            let mut bound = Vec::new();
            for key in keys {
                if let StoreResponse::Binding(Some(id)) =
                    m.execute(StoreRequest::Binding { key }).await?
                {
                    bound.push(id);
                }
            }
            if bound.is_empty() {
                return Ok(Outcome::adopt(m.initialize(now).await?));
            }
            m.initialize_catalog(now).await?;
            let items = m.items();
            Ok(Outcome::adopt(
                match bound
                    .iter()
                    .find(|id| items.iter().any(|item| &item.id == *id))
                {
                    Some(id) => m.resume_startup(id, now).await?,
                    None => m.initialize(now).await?,
                },
            ))
        });
    }
    /// Returns false once startup has finished, is closing, or has no next stage.
    fn restart(&mut self, error: StoreError, now: u64) -> bool {
        let Some(next) = self
            .startup
            .and_then(Startup::next)
            .filter(|_| !self.terminating)
        else {
            return false;
        };
        self.startup_error = Some(error);
        self.start(next, now);
        true
    }
    fn startup_notice_copy(&self) -> Option<WorkspaceNoticeCopy> {
        match self.startup? {
            Startup::Stored => None,
            Startup::Replaced => Some(WorkspaceNoticeCopy::Reset),
            Startup::InMemory => Some(WorkspaceNoticeCopy::Volatile(self.startup_error.clone()?)),
        }
    }
    fn notify<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, copy: WorkspaceNoticeCopy) {
        session.notify(copy.text(&self.localization));
        self.notice = session.state().notice.as_ref().map(|notice| WorkspaceNotice {
            id: notice.id, generation: self.localization_generation, copy,
        });
    }
    fn refresh_notice<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>) {
        if let Some(notice) = &mut self.notice
            && notice.generation != self.localization_generation
        {
            if session.update_notice_text(notice.id, notice.copy.text(&self.localization)) {
                notice.generation = self.localization_generation;
            } else {
                self.notice = None;
            }
        }
    }
    pub fn observe<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, now: u64) {
        if !self.view.ready || self.transition || self.suspended {
            return;
        }
        self.manager.observe_editing(session.editing_state());
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
    /// Observe the regions of accepted host changes, ignoring camera-only motion.
    pub fn observe_regions<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        changed: u32,
        now: u64,
    ) {
        if changed & OBSERVED_REGIONS != 0 {
            self.observe(session, now);
        }
    }
    fn capture<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, now: u64) -> Result<()> {
        let capture = session.capture_workspace().map_err(StoreError::invalid)?;
        self.manager.observe(capture, now);
        self.manager.observe_editing(session.editing_state());
        self.generation = session.workspace_layout_generation();
        self.last_edit = now;
        Ok(())
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
    fn close_prompt(&mut self) {
        self.view.prompt = None;
        self.view.prompt_action = None;
        self.prompt_source = None;
    }
    fn dismiss<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>) -> UiChange {
        let change = self.stop_preview(session);
        self.close_prompt();
        self.view.page = None;
        self.view.details = None;
        self.details_source = None;
        self.view.selected = None;
        self.view.query.clear();
        if self.task.is_none() && self.incoming.is_none() && self.install.is_none() {
            self.end_transition(session);
        }
        change
    }
    fn focus_target(&self, id: &str, now: u64) -> Option<FocusTarget> {
        self.manager
            .items()
            .into_iter()
            .find(|i| i.id == id)
            .and_then(|i| i.claim)
            .filter(|c| c.owner != self.manager.owner && c.expires_at_ms > now)
            .map(|c| FocusTarget {
                id: id.into(),
                owner: c.owner.id,
            })
    }
    fn present(&mut self, now: u64) {
        let m = self.manager.clone();
        self.view.id = m.active_id();
        self.view.name = m.active_name().unwrap_or_default();
        self.view.order = m.workspace_ids();
        let items = m.items();
        let active = self.view.id.clone();
        let switcher_rows = |ids: Vec<String>| {
            ids.into_iter()
                .filter_map(|id| {
                    let item = items.iter().find(|item| item.id == id)?;
                    Some(WorkspaceRow {
                        current: active.as_ref() == Some(&id),
                        title: m.summary_display_name(item),
                        subtitle: String::new(),
                        actions: Vec::new(),
                        id,
                    })
                })
                .collect()
        };
        self.view.switcher = switcher_rows(m.switcher_ids());
        self.view.switcher_display = switcher_rows(m.switcher_display_ids());
        self.view.present_switcher_menus(&self.localization, switcher_rows(self.view.order.clone()));
        let page = self.view.page;
        let (title, intro) = match page {
            Some(ManagerPage::History) => (message(&self.localization, layer_ui::MessageId::WORKSPACE_HISTORY_TITLE, &[("name", self.view.name.clone())]), String::new()),
            Some(ManagerPage::ThisWorkspace) => (
                self.localization.text(layer_ui::MessageId::WORKSPACE_MANAGE_TOOLBARS).to_string(),
                self.localization.text(layer_ui::MessageId::WORKSPACE_TOOLBARS_INTRO).to_string(),
            ),
            Some(ManagerPage::ToolbarLibrary) => (
                self.localization.text(layer_ui::MessageId::WORKSPACE_MANAGE_TOOLBARS).to_string(),
                self.localization.text(layer_ui::MessageId::WORKSPACE_LIBRARY_INTRO).to_string(),
            ),
            _ => (
                self.localization.text(layer_ui::MessageId::WORKSPACE_WORKSPACES).to_string(),
                self.localization.text(layer_ui::MessageId::WORKSPACE_INTRO).to_string(),
            ),
        };
        self.view.title = title;
        self.view.intro = intro;
        let history = m
            .current()
            .and_then(|e| e.capture().ok())
            .map(|c| c.history);
        self.view.rows = match (page, &history) {
            (None, _) => Vec::new(),
            (Some(ManagerPage::History), history) => history
                .iter()
                .flat_map(|history| {
                    layout_history_versions(history).into_iter().map(|r| {
                        let current = r.id == history.current;
                        WorkspaceRow {
                            subtitle: if current {
                                message(&self.localization, layer_ui::MessageId::WORKSPACE_HISTORY_CURRENT, &[("date", date(r.timestamp_ms))])
                            } else {
                                date(r.timestamp_ms)
                            },
                            current,
                            id: r.id,
                            title: r.description.display(&self.localization),
                            actions: Vec::new(),
                        }
                    })
                })
                .collect(),
            (Some(ManagerPage::ThisWorkspace), history) => m
                .rows(ManagerPage::ThisWorkspace, &self.view.query, now)
                .into_iter()
                .map(|r| {
                    let actions = serde_json::from_str::<Panel>(&r.id)
                        .ok()
                        .zip(history.as_ref())
                        .map(|(panel, history)| {
                            toolbar_actions(
                                &self.localization,
                                panel,
                                history.layout().panel_group(panel).is_some(),
                                true,
                            )
                        })
                        .unwrap_or_default();
                    WorkspaceRow {
                        actions,
                        ..r
                    }
                })
                .collect(),
            (Some(page), _) => m
                .rows(page, &self.view.query, now)
                .into_iter()
                .map(|r| WorkspaceRow {
                    actions: items
                        .iter()
                        .find(|i| i.id == r.id)
                        .map(|i| m.summary_actions(i, true, now))
                        .unwrap_or_default(),
                    current: active.as_ref() == Some(&r.id),
                    ..r
                })
                .collect(),
        };
        let selected = self
            .view
            .selected
            .clone()
            .filter(|id| self.view.rows.iter().any(|r| &r.id == id));
        self.selected_elsewhere = page == Some(ManagerPage::Workspaces)
            && selected
                .as_ref()
                .is_some_and(|id| self.focus_target(id, now).is_some());
        let idle = self.preview.is_none();
        match page {
            Some(ManagerPage::Workspaces | ManagerPage::History) => {
                let current = if page == Some(ManagerPage::History) {
                    history.map(|h| h.current)
                } else {
                    active
                };
                self.view.primary = if page == Some(ManagerPage::History) {
                    self.localization.text(layer_ui::MessageId::WORKSPACE_RESTORE_VERSION).to_string()
                } else if self.selected_elsewhere {
                    ManagerAction::SwitchToWindow(String::new()).label(&self.localization)
                } else {
                    self.localization.text(layer_ui::MessageId::WORKSPACE_SWITCH_WORKSPACE).to_string()
                };
                self.view.enabled = idle && selected.is_some_and(|id| current != Some(id));
            }
            Some(_) => {
                let primary = self
                    .view
                    .details
                    .as_ref()
                    .and_then(|d| d.actions.iter().find(|a| a.primary));
                self.view.primary = primary.map(|a| a.label.clone()).unwrap_or_default();
                self.view.enabled =
                    idle && selected.is_some() && primary.is_some_and(|a| a.enabled);
            }
            None => {
                self.view.primary.clear();
                self.view.enabled = false;
            }
        }
    }
    fn offered(&self, action: &ManagerAction) -> bool {
        self.view
            .rows
            .iter()
            .flat_map(|r| &r.actions)
            .chain(self.view.details.iter().flat_map(|d| &d.actions))
            .any(|b| b.enabled && &b.action == action)
    }
    fn select<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        id: Option<String>,
        _now: u64,
    ) -> Result<UiChange> {
        let change = self.stop_preview(session);
        self.view.selected = id.clone();
        self.view.details = None;
        self.details_source = None;
        self.view.enabled = false;
        let (Some(id), Some(page)) = (id, self.view.page) else {
            return Ok(change);
        };
        if page == ManagerPage::ThisWorkspace {
            let panel = serde_json::from_str(&id)
                .map_err(|error| {
                    eprintln!("Invalid toolbar identity: {error}");
                    let mut reason = StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InvalidToolbarIdentity); reason.message = error.to_string(); reason
                })?;
            self.view.details = Some(self.manager.toolbar_details(panel, true)?);
            return Ok(change);
        }
        if page != ManagerPage::ToolbarLibrary {
            session
                .begin_workspace_layout_preview()
                .map_err(StoreError::invalid)?;
            self.preview_open = true;
        }
        let m = self.manager.clone();
        self.preview = self
            .spawn(async move {
                if page == ManagerPage::History {
                    return m
                        .current()
                        .and_then(|e| e.capture().ok())
                        .and_then(|c| c.history.revisions.get(&id).map(|r| r.layout.clone()))
                        .map(|layout| (None, Some(layout)))
                        .ok_or_else(|| {
                            StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisLayoutVersionIsNoLongerRetained)
                        });
                }
                let stored = match m.current_record().filter(|s| s.entity.id == id) {
                    Some(stored) => stored,
                    None => m.load(&id).await?,
                };
                let layout = match &stored.entity.content {
                    ItemContent::Workspace { history, .. } => Some(history.layout().clone()),
                    ItemContent::Toolbar { .. } => None,
                };
                Ok((Some(stored), layout))
            })
            .map(|task| (self.selection_generation, task));
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
                .prompt_action
                .as_ref()
                .is_some_and(|action| *action != ManagerAction::SaveAsNew)
        {
            return self.input(session, WorkspaceInput::Retry, now);
        }
        if !matches!(
            input,
            WorkspaceInput::Resume
                | WorkspaceInput::RefreshSwitcher
                | WorkspaceInput::EditSwitcher { .. }
                | WorkspaceInput::Search { .. }
                | WorkspaceInput::FocusFailed { .. }
        ) && (self.view.ready || !matches!(input, WorkspaceInput::Close))
        {
            self.clear_error();
            if matches!(input, WorkspaceInput::Cancel | WorkspaceInput::Dismiss)
                && let Some(error) = self.manager.error()
            {
                self.set_error(error);
            }
        }
        self.view.focus_window = None;
        let mut change = UiChange::default();
        match input {
            WorkspaceInput::EditSwitcher { edit } => {
                if !self.view.ready || self.terminating {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::OpenAWorkspaceBeforeEditingItsSwitcher));
                }
                self.set_switcher_error(None);
                self.preference_edits.push_back(edit);
            }
            WorkspaceInput::RefreshSwitcher => self.refresh_preferences = true,
            WorkspaceInput::FocusFailed { error } => {
                self.error = None;
                self.view.error = Some(error);
            },
            WorkspaceInput::Search { query } => {
                if self.view.page.is_some() && self.view.prompt.is_none() {
                    self.view.query = query;
                    self.present(now);
                    if self
                        .view
                        .selected
                        .as_ref()
                        .is_some_and(|id| !self.view.rows.iter().any(|row| &row.id == id))
                    {
                        change = self.select(session, None, now)?;
                    }
                }
            }
            WorkspaceInput::Cancel | WorkspaceInput::Dismiss => {
                self.queued = None;
                if matches!(input, WorkspaceInput::Dismiss) || self.view.prompt.is_none() {
                    change = self.dismiss(session);
                } else {
                    change = self.stop_preview(session);
                    self.close_prompt();
                    if self.view.page.is_none() && self.task.is_none() {
                        self.end_transition(session);
                    } else if self.view.page.is_some() && self.task.is_none() {
                        merge(
                            &mut change,
                            self.select(session, self.view.selected.clone(), now)?,
                        );
                    }
                }
            }
            WorkspaceInput::Suspend | WorkspaceInput::Close | WorkspaceInput::Detach => {
                if self.view.closed {
                    return Ok(change);
                }
                self.terminating |= !matches!(input, WorkspaceInput::Suspend);
                self.discard |= matches!(input, WorkspaceInput::Detach);
                self.queued = None;
                self.observe(session, now);
                change = self.dismiss(session);
                self.suspended = true;
                session.set_workspace_read_only(true);
                self.close_after_task = true;
            }
            WorkspaceInput::Resume => {
                if self.view.closed || (self.terminating && self.task.is_some()) {
                    return Ok(change);
                }
                if self.terminating {
                    self.terminating = false;
                    self.discard = false;
                    session.reset_document_close();
                }
                self.suspended = false;
                session.set_workspace_read_only(true);
                self.close_after_task = false;
                self.last_renew = 0;
                self.refresh_preferences = true;
            }
            WorkspaceInput::DiscardClose => {
                if !self.terminating || self.view.closed {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::CloseTheWindowFirst));
                }
                self.discard = true;
                self.close_after_task = true;
            }
            WorkspaceInput::Select { id } => {
                if self.view.page.is_some() && self.view.prompt.is_none() {
                    change = self.select(session, id, now)?;
                }
            }
            input => {
                if matches!(input, WorkspaceInput::Retry)
                    && self.incoming.is_some()
                    && self.task.is_none()
                {
                    // Adoption may have failed after publication. Retry that
                    // validated capture without queuing behind itself.
                    self.clear_error();
                    return Ok(change);
                }
                if self.task.is_some() || self.incoming.is_some() || self.install.is_some() {
                    if matches!(
                        input,
                        WorkspaceInput::Open { .. }
                            | WorkspaceInput::Form { .. }
                            | WorkspaceInput::Action { .. }
                            | WorkspaceInput::Switch { .. }
                            | WorkspaceInput::Import { .. }
                            | WorkspaceInput::Retry
                    ) {
                        self.queued = Some(input);
                        self.view.busy = true;
                        return Ok(change);
                    }
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::WaitForTheCurrentWorkspaceOperationToFinish));
                }
                self.observe(session, now);
                change = match input {
                    WorkspaceInput::Open { page } => self.open(session, page)?,
                    WorkspaceInput::Form { action } => self.form(session, action)?,
                    WorkspaceInput::Action { action } => self.act(session, action, now)?,
                    WorkspaceInput::Submit {
                        name,
                        description,
                        choice,
                    } => self.submit(session, name, description, choice, now)?,
                    WorkspaceInput::Switch { id } => self.switch(session, id, now)?,
                    WorkspaceInput::Confirm => self.confirm(session, now)?,
                    WorkspaceInput::Retry => self.retry(session, now)?,
                    WorkspaceInput::Import { text, kind } => {
                        self.import(session, text, kind, now)?
                    }
                    _ => unreachable!(),
                };
            }
        }
        self.present(now);
        self.publish_flags();
        Ok(change)
    }
    fn publish_flags(&mut self) {
        self.view.switcher_busy = self.preferences.is_some() || !self.preference_edits.is_empty();
        self.view.busy = (self.task.is_some() && !self.quiet)
            || self.incoming.is_some()
            || self.install.is_some()
            || self.queued.is_some();
        self.view.loading = self.preview.is_some();
        self.view.dirty = self.manager.dirty();
        self.view.saving = self.manager.saving();
        self.view.closing = self.terminating && !self.view.closed;
        self.view.update_switcher_menu_availability();
    }
    fn open<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        page: ManagerPage,
    ) -> Result<UiChange> {
        let history = self
            .manager
            .current()
            .and_then(|e| e.capture().ok())
            .map(|c| c.history.current);
        if page == ManagerPage::History && history.is_none() {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::OpenAWorkspaceFirst));
        }
        self.start_transition(session)?;
        let change = self.stop_preview(session);
        self.close_prompt();
        if self.view.page != Some(page) {
            self.view.query.clear();
        }
        self.view.page = Some(page);
        self.view.details = None;
        self.details_source = None;
        self.view.selected = match page {
            ManagerPage::Workspaces => self.manager.active_id(),
            ManagerPage::History => history,
            _ => None,
        };
        let m = self.manager.clone();
        self.run(async move {
            m.refresh().await?;
            m.refresh_switcher().await?;
            Ok(Outcome::Done)
        });
        Ok(change)
    }
    fn form<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        action: ManagerAction,
    ) -> Result<UiChange> {
        let current = self.manager.current();
        let source = match &action {
            ManagerAction::Reset(id) => {
                if self.manager.active_id().as_ref() != Some(id) {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::SwitchToThisWorkspaceBeforeRestoringItsLayout));
                }
                current.as_ref().map(|e| e.metadata.clone())
            }
            ManagerAction::Rename(id)
            | ManagerAction::Delete(id)
            | ManagerAction::UpdateToolbar(id) => self
                .manager
                .items()
                .into_iter()
                .find(|i| &i.id == id)
                .map(|i| i.metadata),
            _ => None,
        };
        let prompt = match &action {
            ManagerAction::RecoverInterrupted => recover_prompt(&self.localization, self.interrupted_choices())?,
            _ => self.manager.form_prompt(&action, source.as_ref())?,
        };
        self.start_transition(session)?;
        let mut change = self.stop_preview(session);
        if let (ManagerAction::Reset(_), Some(entity)) = (&action, &current) {
            session
                .begin_workspace_layout_preview()
                .map_err(StoreError::invalid)?;
            self.preview_open = true;
            merge(
                &mut change,
                session
                    .preview_workspace_layout(&entity.starting_layout(self.manager.platform)?)
                    .map_err(StoreError::invalid)?,
            );
        }
        self.prompt_source = source;
        self.view.prompt = Some(prompt);
        self.view.prompt_action = Some(action);
        Ok(change)
    }
    fn submit<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        name: String,
        description: Option<String>,
        choice: Option<String>,
        now: u64,
    ) -> Result<UiChange> {
        let (Some(action), Some(prompt)) =
            (self.view.prompt_action.clone(), self.view.prompt.clone())
        else {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::OpenAWorkspaceDialogFirst));
        };
        if prompt.name.is_some() {
            validate_name(&name)?;
        }
        let choice = choice.or(prompt.selected).unwrap_or_default();
        if !prompt.choices.is_empty() && !prompt.choices.iter().any(|c| c.id == choice) {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseOneOfTheListedOptions));
        }
        let mut change = self.stop_preview(session);
        self.start_transition(session)?;
        let m = self.manager.clone();
        if action == ManagerAction::ResetBrushes {
            merge(
                &mut change,
                session
                    .reset_brushes()
                    .map_err(StoreError::invalid)?,
            );
            m.observe_working(session.workspace_working_state());
            m.observe_editing(session.editing_state());
            self.run(async move {
                m.flush().await?;
                Ok(Outcome::Done)
            });
            return Ok(change);
        }
        let capture = (action == ManagerAction::SaveAsNew)
            .then(|| session.capture_workspace())
            .transpose()
            .map_err(StoreError::invalid)?;
        self.run(async move {
            let install = |config, replace, group, exact_name| {
                Outcome::Install(Box::new(Install {
                    config,
                    replace,
                    group,
                    exact_name,
                }))
            };
            Ok(match action {
                ManagerAction::New => Outcome::adopt(m.create_workspace(&name, now).await?),
                ManagerAction::Rename(id) => {
                    let metadata = m.load(&id).await?.entity.metadata;
                    let description = match description {
                        Some(text) if metadata.kind == ItemKind::Toolbar => text,
                        _ => metadata.description,
                    };
                    m.rename(&id, &name, &description, now).await?;
                    Outcome::Done
                }
                ManagerAction::Delete(id) => match m.delete_item(&id, None, now).await? {
                    Some(s) => Outcome::adopt(s),
                    None => Outcome::Done,
                },
                ManagerAction::Reset(id) => Outcome::adopt(m.change_layout(&id, None, now).await?),
                ManagerAction::SaveAsNew => {
                    Outcome::adopt(m.save_as_new(capture.unwrap(), &name, now).await?)
                }
                ManagerAction::SaveToolbar(panel) => {
                    m.save_toolbar(panel, &name, now).await?;
                    Outcome::Done
                }
                ManagerAction::UpdateToolbar(id) => {
                    let panel = serde_json::from_str(&choice)
                        .map_err(|_| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAToolbar))?;
                    m.update_toolbar_from(&id, panel, now).await?;
                    Outcome::Done
                }
                ManagerAction::ReplaceToolbar(panel) => {
                    if choice.is_empty() {
                        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseASavedToolbar));
                    }
                    m.flush().await?;
                    let config = m.toolbar_config(Some(&choice), None).await?;
                    install(config, Some(panel), None, false)
                }
                ManagerAction::NewToolbar(group) => {
                    m.flush().await?;
                    let config = m.toolbar_config(Some(&choice), Some(&name)).await?;
                    install(config, None, group, true)
                }
                ManagerAction::RecoverInterrupted => {
                    Outcome::Editing(m.recover_interrupted(&choice, now).await?.map(Box::new))
                }
                _ => {
                    return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisActionDoesNotUseAWorkspaceForm));
                }
            })
        });
        Ok(change)
    }
    fn act<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        action: ManagerAction,
        now: u64,
    ) -> Result<UiChange> {
        if self.view.page.is_some() && !self.offered(&action) {
            return Ok(UiChange::default());
        }
        let customize = |action| UiAction::Customize { action };
        Ok(match action {
            ManagerAction::Switch(id) | ManagerAction::SwitchToWindow(id) => {
                self.switch(session, id, now)?
            }
            ManagerAction::History(_) => self.open(session, ManagerPage::History)?,
            ManagerAction::RetryStorage => self.retry(session, now)?,
            ManagerAction::AddToolbar(id) => {
                self.start_transition(session)?;
                let change = self.stop_preview(session);
                let m = self.manager.clone();
                self.run(async move {
                    m.flush().await?;
                    let config = m.toolbar_config(Some(&id), None).await?;
                    Ok(Outcome::Install(Box::new(Install {
                        config,
                        replace: None,
                        group: None,
                        exact_name: false,
                    })))
                });
                change
            }
            ManagerAction::ShowToolbar(panel, visible) => {
                self.end_transition(session);
                let result = session.dispatch(customize(CustomizationAction::SetPanelVisible {
                    panel,
                    visible,
                }));
                self.start_transition(session)?;
                let mut change = result.map_err(StoreError::invalid)?;
                self.capture(session, now)?;
                merge(
                    &mut change,
                    self.select(session, self.view.selected.clone(), now)?,
                );
                change
            }
            ManagerAction::RenameToolbar(panel)
            | ManagerAction::DuplicateToolbar(panel)
            | ManagerAction::DeleteToolbar(panel) => {
                let mut change = self.dismiss(session);
                let action = match action {
                    ManagerAction::RenameToolbar(_) => CustomizationAction::RenameToolbar { panel },
                    ManagerAction::DuplicateToolbar(_) => {
                        CustomizationAction::DuplicateToolbar { panel }
                    }
                    _ => CustomizationAction::DeleteToolbar { panel },
                };
                merge(
                    &mut change,
                    session
                        .dispatch(customize(action))
                        .map_err(StoreError::invalid)?,
                );
                change
            }
            action => self.form(session, action)?,
        })
    }
    fn switch<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        id: String,
        now: u64,
    ) -> Result<UiChange> {
        if self.manager.active_id().as_deref() == Some(&id) {
            return Ok(UiChange::default());
        }
        let change = self.stop_preview(session);
        self.start_transition(session)?;
        let m = self.manager.clone();
        self.run(async move {
            m.refresh().await?;
            if let Some(claim) = m
                .items()
                .into_iter()
                .find(|item| item.id == id)
                .and_then(|item| item.claim)
                .filter(|c| c.owner != m.owner && c.expires_at_ms > now)
            {
                return Ok(Outcome::Focus(FocusTarget {
                    id,
                    owner: claim.owner.id,
                }));
            }
            Ok(Outcome::adopt(m.prepare_switch(&id, now).await?))
        });
        Ok(change)
    }
    fn import<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        text: String,
        kind: PackageKind,
        now: u64,
    ) -> Result<UiChange> {
        let change = self.stop_preview(session);
        self.close_prompt();
        self.start_transition(session)?;
        let m = self.manager.clone();
        self.run(async move {
            let bytes = text.as_bytes();
            Ok(match kind {
                PackageKind::WorkspaceBackup => {
                    Outcome::adopt(m.import_workspace_package(bytes, now).await?)
                }
                PackageKind::Toolbar => Outcome::Show(
                    ManagerPage::ToolbarLibrary,
                    m.import_reusable_package(bytes, kind, now).await?,
                ),
            })
        });
        Ok(change)
    }
    fn confirm<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        now: u64,
    ) -> Result<UiChange> {
        let Some(id) = self.view.selected.clone().filter(|_| self.view.enabled) else {
            return Ok(UiChange::default());
        };
        match self.view.page {
            Some(ManagerPage::History) => {
                let change = self.stop_preview(session);
                self.start_transition(session)?;
                let m = self.manager.clone();
                self.run(async move {
                    let active = m
                        .active_id()
                        .ok_or_else(|| StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::OpenAWorkspace))?;
                    Ok(Outcome::adopt(
                        m.change_layout(&active, Some(&id), now).await?,
                    ))
                });
                Ok(change)
            }
            Some(ManagerPage::Workspaces) => self.switch(session, id, now),
            _ => {
                let primary = self
                    .view
                    .details
                    .as_ref()
                    .and_then(|d| d.actions.iter().find(|a| a.primary && a.enabled))
                    .map(|a| a.action.clone());
                match primary {
                    Some(action) => self.act(session, action, now),
                    None => Ok(UiChange::default()),
                }
            }
        }
    }
    fn retry<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        now: u64,
    ) -> Result<UiChange> {
        if self.incoming.is_some() {
            return Ok(UiChange::default());
        }
        if !self.view.ready {
            self.initialize(now);
            return Ok(UiChange::default());
        }
        self.close_after_task |= self.terminating;
        let m = self.manager.clone();
        let key = self.resume_key.clone();
        self.start_transition(session)?;
        self.run(async move {
            m.execute(StoreRequest::Reopen).await?;
            m.revalidate_owner(now).await?;
            m.flush().await?;
            Ok(match m.retry_failed_operation().await? {
                Some(s) => Outcome::adopt(s),
                None => {
                    if let Some(key) = key {
                        m.bind_resume_key(&key).await?;
                    }
                    Outcome::Done
                }
            })
        });
        Ok(UiChange::default())
    }
    fn command_input<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        command: WorkspaceCommand,
    ) -> Result<Option<WorkspaceInput>> {
        let form = |action| Some(WorkspaceInput::Form { action });
        let simple = matches!(self.manager.platform, Platform::Web | Platform::Android);
        Ok(match command {
            WorkspaceCommand::Manage => Some(WorkspaceInput::Open {
                page: ManagerPage::Workspaces,
            }),
            WorkspaceCommand::LayoutHistory => Some(WorkspaceInput::Open {
                page: ManagerPage::History,
            }),
            WorkspaceCommand::New => form(ManagerAction::New),
            WorkspaceCommand::ResetBrushes => form(ManagerAction::ResetBrushes),
            WorkspaceCommand::ResetLayout => {
                form(ManagerAction::Reset(self.manager.active_id().ok_or_else(
                    || StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::OpenAWorkspaceFirst),
                )?))
            }
            WorkspaceCommand::Switch { id } => Some(WorkspaceInput::Switch { id }),
            WorkspaceCommand::ShowInSwitcher { id, visible } => Some(WorkspaceInput::EditSwitcher {
                edit: SwitcherEdit::Show { id, visible },
            }),
            WorkspaceCommand::SaveToolbar { panel } => form(ManagerAction::SaveToolbar(panel)),
            WorkspaceCommand::ManageToolbars if !simple => Some(WorkspaceInput::Open {
                page: ManagerPage::ThisWorkspace,
            }),
            WorkspaceCommand::NewToolbar { group } if !simple => {
                form(ManagerAction::NewToolbar(group))
            }
            WorkspaceCommand::ManageToolbars | WorkspaceCommand::NewToolbar { .. } => {
                let action = match command {
                    WorkspaceCommand::NewToolbar { group } => {
                        CustomizationAction::NewToolbar { group }
                    }
                    _ => CustomizationAction::ManageToolbars,
                };
                let customized = session
                    .dispatch(UiAction::Customize { action })
                    .map_err(StoreError::invalid)?;
                merge(&mut self.routed, customized);
                None
            }
        })
    }
    /// Complete each host workspace request once and run it as controller input.
    fn route_requests<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, now: u64) {
        let requests: Vec<_> = session
            .state()
            .requests
            .iter()
            .filter_map(|r| match &r.kind {
                HostRequestKind::Workspace { command } => Some((r.id, command.clone())),
                _ => None,
            })
            .collect();
        for (id, command) in requests {
            match session.dispatch(UiAction::CompleteRequest { id, error: None }) {
                Ok(c) => merge(&mut self.routed, c),
                Err(error) => {
                    self.error = None;
                    self.view.error = Some(error);
                    continue;
                }
            }
            if self.terminating {
                continue;
            }
            match self
                .command_input(session, command)
                .and_then(|input| match input {
                    Some(input) => self.input(session, input, now),
                    None => Ok(UiChange::default()),
                }) {
                Ok(c) => merge(&mut self.routed, c),
                Err(error) => self.set_error(error),
            }
        }
    }
    fn install_toolbar<R: CanvasRenderer>(
        &mut self,
        session: &mut UiSession<R>,
        install: Install,
        now: u64,
    ) -> Result<UiChange> {
        if self.view.owner_lost || !self.manager.lease_valid(now) {
            return Err(StoreError::known(ErrorKind::OwnedElsewhere, WorkspaceRefusal::WorkspaceOwnershipChangedWhileLoadingTheToolbar));
        }
        let (_, change) = session
            .install_workspace_toolbar(
                install.config,
                install.replace,
                install.group,
                install.exact_name,
            )
            .map_err(StoreError::invalid)?;
        self.close_prompt();
        self.capture(session, now)?;
        let m = self.manager.clone();
        self.run(async move {
            m.flush().await?;
            Ok(Outcome::Dismiss)
        });
        Ok(change)
    }
    pub fn tick<R: CanvasRenderer>(&mut self, session: &mut UiSession<R>, now: u64) -> UiChange {
        self.manager.clock.set(now);
        self.refresh_notice(session);
        self.route_requests(session, now);
        let mut change = std::mem::take(&mut self.routed);
        let mut presentation_changed = false;
        if !self.view.ready {
            session.set_workspace_read_only(true);
        }
        // App preferences run independently of layout previews and workspace
        // publication. Their acknowledgement must not select/reload a row.
        if let Some(result) = self.preferences.as_mut().and_then(Task::poll) {
            self.preferences = None;
            presentation_changed = true;
            match result {
                Ok(()) => {
                    self.set_switcher_error(None);
                    if self.preferences_edited {
                        self.view.switcher_revision = self.view.switcher_revision.wrapping_add(1);
                    }
                    self.pending_binding = self.manager.binding();
                }
                Err(error) => {
                    if self.preferences_edited && self.view.page.is_none() {
                        self.notify(session, WorkspaceNoticeCopy::SwitcherError(error.clone()));
                    }
                    self.set_switcher_error(Some(error));
                },
            }
            self.preferences_edited = false;
        }
        if self.view.ready
            && (self.task.is_none() || self.quiet)
            && self.incoming.is_none()
            && self.preferences.is_none()
        {
            let manager = self.manager.clone();
            if let Some(edit) = self.preference_edits.pop_front() {
                self.preferences_edited = true;
                self.preferences = self.spawn(async move { manager.edit_switcher(edit).await });
            } else if self.refresh_preferences && !self.terminating {
                self.refresh_preferences = false;
                self.preferences = self.spawn(async move {
                    manager.refresh().await?;
                    manager.refresh_switcher().await
                });
            }
        }
        if let Some(result) = self.task.as_mut().and_then(Task::poll) {
            self.task = None;
            presentation_changed = true;
            match result {
                Ok(outcome) => {
                    let keep_prompt = matches!(outcome, Outcome::Adopt(_) | Outcome::Editing(Some(_)) | Outcome::Install(_));
                    match outcome {
                        Outcome::Adopt(incoming) => {
                            if self
                                .view
                                .switcher
                                .iter()
                                .map(|row| &row.id)
                                .ne(self.manager.switcher_ids().iter())
                            {
                                self.view.switcher_revision =
                                    self.view.switcher_revision.wrapping_add(1);
                            }
                            self.incoming = Some(*incoming);
                        }
                        Outcome::Editing(incoming) => {
                            if let Some(editing) = self.manager.editing() {
                                match session.restore_editing(*editing) {
                                    Ok(c) => merge(&mut change, c),
                                    Err(e) => self.set_error(StoreError::invalid(e)),
                                }
                            }
                            self.incoming = incoming.map(|s| *s);
                        }
                        Outcome::Focus(target) => {
                            self.view.focus_window = Some(target);
                            merge(&mut change, self.dismiss(session));
                        }
                        Outcome::Dismiss => merge(&mut change, self.dismiss(session)),
                        Outcome::Closed => {
                            self.close_after_task = false;
                            if self.terminating {
                                self.view.closed = true;
                                session.set_workspace_read_only(true);
                            }
                        }
                        Outcome::Install(install) => self.install = Some(install),
                        Outcome::Show(page, id) => {
                            self.view.page = Some(page);
                            self.view.query.clear();
                            self.view.selected = Some(id);
                        }
                        Outcome::Done => {}
                    }
                    if !keep_prompt {
                        self.close_prompt();
                        if self.view.page.is_some() && !self.suspended {
                            match self.select(session, self.view.selected.clone(), now) {
                                Ok(c) => merge(&mut change, c),
                                Err(e) => self.set_error(e),
                            }
                        }
                    }
                    if self.view.page.is_none() && self.incoming.is_none() && self.install.is_none()
                    {
                        self.end_transition(session);
                    }
                    self.pending_binding = self.manager.binding();
                }
                Err(e) => {
                    if !self.restart(e.clone(), now) {
                        self.set_error(e);
                        if self.view.page.is_none() && self.view.prompt.is_none() {
                            self.end_transition(session);
                        }
                        self.close_after_task &= self.discard;
                    }
                }
            }
        }
        if self.install.is_some() && self.task.is_none() && session.require_workspace_idle().is_ok()
        {
            presentation_changed = true;
            let install = *self.install.take().unwrap();
            if self.terminating {
                self.end_transition(session);
            } else {
                match self.install_toolbar(session, install, now) {
                    Ok(c) => merge(&mut change, c),
                    Err(e) => {
                        self.set_error(e);
                        if self.view.page.is_none() && self.view.prompt.is_none() {
                            self.end_transition(session);
                        }
                    }
                }
            }
        }
        if let Some(result) = self.incoming_renew.as_mut().and_then(Task::poll) {
            self.incoming_renew = None;
            match result {
                Ok(incoming) => self.incoming = Some(incoming),
                Err(e) => {
                    if !self.restart(e.clone(), now) {
                        self.set_error(e);
                    }
                }
            }
        }
        if self.incoming.is_some()
            && self.terminating
            && self.task.is_none()
            && self.incoming_renew.is_none()
        {
            let incoming = self.incoming.take().unwrap();
            let current = self.manager.current_record().filter(|_| self.discard);
            let discard = self.discard;
            let m = self.manager.clone();
            self.run(async move {
                for claimed in [incoming].iter().chain(&current) {
                    m.release(claimed).await;
                }
                if !discard {
                    m.close().await?;
                }
                Ok(Outcome::Closed)
            });
        }
        if let Some(incoming) = &self.incoming
            && !self.terminating
            && self.incoming_renew.is_none()
            && (self.startup.is_some() || self.view.error.is_none())
            && incoming
                .claim
                .as_ref()
                .is_none_or(|c| c.expires_at_ms <= now.saturating_add(OWNER_RENEW_MS))
        {
            let m = self.manager.clone();
            let original = incoming.clone();
            self.incoming_renew = self.spawn(async move {
                let claimed = m.claim(&original.entity.id).await?;
                if claimed.generations != original.generations {
                    m.release(&claimed).await;
                    return Err(StoreError::conflict());
                }
                Ok(claimed)
            });
        }
        if self.incoming.is_some()
            && !self.terminating
            && self.incoming_renew.is_none()
            && (self.startup.is_some() || self.view.error.is_none())
            && session.require_workspace_idle().is_ok()
        {
            presentation_changed = true;
            let incoming = self.incoming.take().unwrap();
            let prepared = incoming
                .entity
                .capture()
                .and_then(|c| PreparedWorkspace::new(c).map_err(StoreError::workspace));
            if prepared.is_ok()
                && let Some(copy) = self.startup_notice_copy()
            {
                self.notify(session, copy);
            }
            let editing = self.startup.is_some().then(|| self.manager.editing()).flatten();
            match prepared.and_then(|p| {
                if let Some(editing) = editing { merge(&mut change, session.restore_editing(*editing).map_err(StoreError::invalid)?); }
                session.adopt_workspace(p).map_err(StoreError::invalid)
            }) {
                Ok(c) => {
                    merge(&mut change, c);
                    self.startup = None;
                    self.startup_error = None;
                    let id = incoming.entity.id.clone();
                    let outgoing = self
                        .manager
                        .activate(incoming)
                        .filter(|outgoing| outgoing.entity.id != id);
                    let key = self.resume_key.clone();
                    if outgoing.is_some() || key.is_some() {
                        let m = self.manager.clone();
                        self.run_quietly(async move {
                            if let Some(outgoing) = outgoing {
                                m.release(&outgoing).await;
                            }
                            if let Some(key) = key {
                                m.bind_resume_key(&key).await?;
                            }
                            Ok(Outcome::Done)
                        });
                    }
                    if !self.view.ready {
                        self.last_renew = 0;
                    }
                    self.view.ready = true;
                    self.clear_error();
                    self.view.owner_lost = false;
                    self.view.page = None;
                    self.view.details = None;
                    self.details_source = None;
                    self.view.selected = None;
                    self.view.query.clear();
                    self.close_prompt();
                    self.generation = session.workspace_layout_generation();
                    session.set_workspace_read_only(self.suspended);
                    self.end_transition(session);
                    self.pending_binding = self.manager.binding();
                }
                Err(e) => {
                    if !self.restart(e.clone(), now) {
                        self.incoming = Some(incoming);
                        self.set_error(e);
                    }
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
            if generation == self.selection_generation && self.view.page.is_some() {
                let shown = result.and_then(|(details, layout)| {
                    self.details_source = details;
                    self.refresh_details(now);
                    match layout.filter(|_| self.preview_open) {
                        Some(layout) => session
                            .preview_workspace_layout(&layout)
                            .map_err(StoreError::invalid),
                        None => Ok(UiChange::default()),
                    }
                });
                match shown {
                    Ok(c) => merge(&mut change, c),
                    Err(e) => {
                        self.set_error(e);
                        self.view.selected = None;
                    }
                }
            }
        }
        if let Some(result) = self.renew.as_mut().and_then(Task::poll) {
            self.renew = None;
            presentation_changed = true;
            match result {
                Ok(interrupted) => {
                    self.interrupted = interrupted;
                    self.view.owner_lost = false;
                    session.set_workspace_read_only(self.suspended);
                    if let Some(error) = self.renew_error.take()
                        && self.error.as_ref() == Some(&error)
                    {
                        self.clear_error();
                    }
                }
                Err(e) => {
                    if matches!(e.kind, ErrorKind::OwnedElsewhere | ErrorKind::Conflict)
                        || !self.manager.lease_valid(now)
                    {
                        self.view.owner_lost = true;
                        session.set_workspace_read_only(true);
                    }
                    self.renew_error = Some(e.clone());
                    self.set_error(e);
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
                self.view.owner_lost = true;
                session.set_workspace_read_only(true);
            }
            self.renew = self.spawn(async move {
                m.revalidate_owner(now).await?;
                Ok(m.interrupted_change_sources(now).await.unwrap_or_default())
            });
        }
        if (self.view.ready || self.discard) && self.task.is_none() && self.incoming.is_none() {
            if self.close_after_task && self.renew.is_none()
                && self.preferences.is_none() && self.preference_edits.is_empty()
            {
                let current = self.manager.current_record().filter(|_| self.discard);
                let discard = self.discard;
                let m = self.manager.clone();
                self.run(async move {
                    if let Some(current) = current {
                        m.release(&current).await;
                    } else if !discard {
                        m.close().await?;
                    }
                    Ok(Outcome::Closed)
                });
            } else if self.view.ready
                && !self.transition
                && !self.suspended
                && self.manager.dirty()
                && self.view.error.is_none()
                && (now.saturating_sub(self.last_edit) >= 250
                    || now.saturating_sub(self.last_observed) >= 2000)
            {
                self.last_observed = now;
                let m = self.manager.clone();
                self.run_quietly(async move {
                    m.save_once().await?;
                    Ok(Outcome::Done)
                });
            }
        }
        if self.task.is_none()
            && self.incoming.is_none()
            && self.install.is_none()
            && !self.terminating
            && let Some(input) = self.queued.take()
        {
            match self.input(session, input, now) {
                Ok(c) => merge(&mut change, c),
                Err(e) => self.set_error(e),
            }
        }
        if presentation_changed {
            self.present(now);
            if self.view.page.is_some()
                && self.view.prompt.is_none()
                && self
                    .view
                    .selected
                    .as_ref()
                    .is_some_and(|id| !self.view.rows.iter().any(|row| &row.id == id))
            {
                match self.select(session, None, now) {
                    Ok(c) => merge(&mut change, c),
                    Err(error) => self.set_error(error),
                }
            }
        }
        // Autosave acknowledgements do not change menus. Reconfiguring them
        // during a resize refreshes command state and invalidates retained tool
        // controls. Publish actual catalog changes only at an idle boundary.
        if self.pending_binding.is_some() && session.require_workspace_idle().is_ok() {
            let binding = self.pending_binding.take().unwrap();
            if let Ok(key) = serde_json::to_string(&binding)
                && self.binding_key.as_ref() != Some(&key)
            {
                match session.configure_workspace_manager(binding) {
                    Ok(c) => {
                        self.binding_key = Some(key);
                        merge(&mut change, c);
                    }
                    Err(e) => {
                        self.error = None;
                        self.view.error = Some(e);
                    },
                }
            }
        }
        self.publish_flags();
        self.view.interrupted = self.interrupted.len();
        self.view.retry = self.view.error.is_some()
            && (self.manager.has_failed_operation() || self.manager.dirty());
        change
    }
}

#[cfg(test)]
#[path = "controller_localization_tests.rs"]
mod localization_tests;
