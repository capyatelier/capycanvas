//! GTK lifecycle and asynchronous transport for the shared workspace manager.
use super::*;
use layer_workspace::{StoreError, StoreWorker, StoredEntity, WorkspaceManager};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Manager = WorkspaceManager<StoreWorker>;
thread_local! {
    static WINDOWS: RefCell<std::collections::BTreeMap<String, std::rc::Weak<Workspace>>> = RefCell::new(std::collections::BTreeMap::new());
}
#[path = "workspace_manager_actions.rs"]
mod actions;
#[path = "workspace_manager_dialog.rs"]
mod dialog;
#[path = "workspace_manager_storage.rs"]
mod storage;
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) struct NativeWorkspaces {
    pub ui: dialog::ManagerUi,
    pub manager: Option<Rc<Manager>>,
    pub root: gtk::Box,
    pub label: gtk::Label,
    retry: gtk::Button,
    recovery: gtk::Button,
    pub ready: Cell<bool>,
    pub busy: Cell<bool>,
    operation_generation: Cell<u64>,
    validating_owner: Cell<bool>,
    owner_lost: Cell<bool>,
    interrupted_count: Cell<usize>,
    interruption_error: RefCell<Option<String>>,
    close_ready: Cell<bool>,
    close_requested: Cell<bool>,
    close_prompt: Cell<bool>,
    layout_pending: Cell<bool>,
    captured_generation: Cell<Option<u64>>,
    last_edit: Cell<Instant>,
    last_save: Cell<Instant>,
    last_renew: Cell<Instant>,
    last_maintenance: Cell<Instant>,
    failed_snapshot: RefCell<Option<WorkspaceCapture>>,
}
impl NativeWorkspaces {
    pub fn new() -> Self {
        let directory = std::env::var_os("CAPY_WORKSPACE_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                (!cfg!(test)).then(|| glib::user_data_dir().join("art.capycanvas.CapyCanvas"))
            });
        let manager = directory
            .and_then(|directory| StoreWorker::shared(&directory).ok())
            .map(|worker| Rc::new(Manager::new(worker, Platform::Gtk)));
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.set_widget_name("workspace-save-status");
        root.add_css_class("workspace-save-status");
        for set in [
            gtk::prelude::WidgetExt::set_margin_start,
            gtk::prelude::WidgetExt::set_margin_end,
        ] {
            set(&root, 12);
        }
        let label = gtk::Label::new(None);
        label.set_xalign(0.);
        label.set_hexpand(true);
        label.set_wrap(true);
        let retry = gtk::Button::with_label("Retry");
        retry.set_widget_name("workspace-save-retry");
        retry.set_visible(false);
        root.append(&label);
        root.append(&retry);
        let recovery = gtk::Button::with_label("Storage and Backups…");
        recovery.set_visible(false);
        root.append(&recovery);
        root.set_visible(manager.is_some());
        let now = Instant::now();
        Self {
            ui: dialog::ManagerUi::new(),
            ready: Cell::new(manager.is_none()),
            manager,
            root,
            label,
            retry,
            recovery,
            busy: Cell::new(false),
            operation_generation: Cell::new(0),
            validating_owner: Cell::new(false),
            owner_lost: Cell::new(false),
            interrupted_count: Cell::new(0),
            interruption_error: RefCell::new(None),
            close_ready: Cell::new(false),
            close_requested: Cell::new(false),
            close_prompt: Cell::new(false),
            layout_pending: Cell::new(false),
            captured_generation: Cell::new(None),
            last_edit: Cell::new(now),
            last_save: Cell::new(now),
            last_renew: Cell::new(now),
            last_maintenance: Cell::new(now),
            failed_snapshot: RefCell::new(None),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        if let Some(manager) = &self.manager {
            WINDOWS.with(|windows| {
                windows
                    .borrow_mut()
                    .insert(manager.owner.id.clone(), Rc::downgrade(w));
            });
        }
        self.ui.bind(w);
        w.window.connect_is_active_notify(glib::clone!(
            #[weak]
            w,
            move |window| {
                if window.is_active() {
                    // A live lease already fences other writers. Disabling the
                    // editor on ordinary activation cancels the native click
                    // that focused it, including menu and context-menu grabs.
                    if w.workspaces.owner_lost.get()
                        || w.workspaces
                            .manager
                            .as_ref()
                            .is_some_and(|m| !m.lease_valid(now_ms()))
                    {
                        w.workspaces.revalidate(&w);
                    }
                } else if w.workspaces.ready.get() {
                    w.workspaces.save(&w, false);
                }
            }
        ));
        self.recovery.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.workspaces
                    .ui
                    .run(&w, layer_workspace::ManagerAction::Storage);
            }
        ));
        self.retry.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.workspaces.ready.get() {
                    w.workspaces
                        .ui
                        .run(&w, layer_workspace::ManagerAction::RetryStorage);
                } else {
                    w.workspaces.start(&w);
                }
            }
        ));
        if self.manager.is_none() {
            return;
        }
        glib::timeout_add_local(
            Duration::from_millis(250),
            glib::clone!(
                #[weak]
                w,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    w.workspaces.tick(&w);
                    glib::ControlFlow::Continue
                }
            ),
        );
    }
    pub fn start(&self, w: &Rc<Workspace>) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        if self.busy.replace(true) {
            return;
        }
        let recovery = w
            .gpu
            .borrow_mut()
            .as_mut()
            .and_then(|g| g.session.capture_workspace().ok())
            .filter(|capture| {
                self.failed_snapshot
                    .borrow()
                    .as_ref()
                    .is_some_and(|old| old != capture)
            });
        w.surface.set_sensitive(false);
        self.label.set_text("Opening workspace…");
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                // Startup can still be compiling installed filter resources.
                // Let rendering finish that preparation before the idle-only
                // workspace adoption, while the editor remains insensitive.
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    let result = w
                        .gpu
                        .borrow_mut()
                        .as_mut()
                        .ok_or("Canvas unavailable".to_string())
                        .and_then(|g| g.session.begin_workspace_transition());
                    match result {
                        Ok(()) => break,
                        Err(error)
                            if w.workspaces.failed_snapshot.borrow().is_some()
                                || Instant::now() >= deadline =>
                        {
                            w.workspaces
                                .adopt(&w, Err(StoreError::invalid(error)))
                                .await;
                            return;
                        }
                        Err(_) => {
                            w.wake();
                            glib::timeout_future(Duration::from_millis(16)).await;
                        }
                    }
                }
                let _ = manager
                    .store
                    .request(layer_workspace::StoreRequest::Reopen)
                    .await;
                let mut result = manager.initialize(now_ms()).await;
                if let (Ok(incoming), Some(capture)) = (&result, recovery) {
                    manager.release(incoming).await;
                    result = manager
                        .save_as_new(capture, "Recovered Workspace", now_ms())
                        .await;
                }
                w.workspaces.adopt(&w, result).await;
            }
        ));
    }
    pub async fn adopt(&self, w: &Rc<Workspace>, incoming: Result<StoredEntity, StoreError>) {
        let manager = self.manager.as_ref().unwrap();
        match incoming {
            Ok(incoming) => {
                let result = incoming
                    .entity
                    .capture()
                    .and_then(|capture| {
                        PreparedWorkspace::new(capture).map_err(StoreError::invalid)
                    })
                    .and_then(|prepared| {
                        w.gpu
                            .borrow_mut()
                            .as_mut()
                            .ok_or_else(|| StoreError::invalid("Canvas unavailable."))?
                            .session
                            .adopt_workspace(prepared)
                            .map_err(StoreError::invalid)
                    });
                match result {
                    Ok(change) => {
                        self.owner_lost.set(false);
                        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                            gpu.session.set_workspace_read_only(false);
                        }
                        let outgoing = manager.activate(incoming);
                        self.sync_binding(w);
                        self.ready.set(true);
                        self.layout_pending.set(false);
                        self.captured_generation.set(None);
                        w.changed(Ok(change));
                        if let Some(outgoing) = outgoing
                            && manager.active_id().as_deref() != Some(&outgoing.entity.id)
                        {
                            manager.release(&outgoing).await;
                        }
                        if let Err(error) = manager.refresh().await {
                            self.show_error(error);
                        }
                        self.refresh_interrupted().await;
                    }
                    Err(error) => {
                        manager.release(&incoming).await;
                        self.show_error(error);
                    }
                }
            }
            Err(error) => {
                if !self.ready.get() {
                    *self.failed_snapshot.borrow_mut() = w
                        .gpu
                        .borrow_mut()
                        .as_mut()
                        .and_then(|g| g.session.capture_workspace().ok());
                }
                self.show_error(error);
            }
        }
        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
            gpu.session.end_workspace_transition();
        }
        manager.finish_transition();
        self.busy.set(false);
        w.surface.set_sensitive(!self.validating_owner.get());
        self.update_status();
        if self.close_requested.get() {
            w.window.close();
        }
    }
    pub fn observe(&self, w: &Workspace, regions: u32) {
        if !self.ready.get() || self.busy.get() {
            return;
        }
        let Some(manager) = self.manager.as_ref() else {
            return;
        };
        if regions & (regions::LAYOUT | regions::CUSTOMIZATION) != 0 {
            self.layout_pending.set(true);
        }
        if regions & (regions::BRUSH | regions::LAYOUT | regions::COMMANDS) != 0 {
            if let Some(gpu) = w.gpu.borrow().as_ref() {
                manager.observe_working(gpu.session.workspace_working_state());
            }
            self.last_edit.set(Instant::now());
        }
        self.capture(w);
        self.update_status();
    }
    fn capture(&self, w: &Workspace) {
        if !self.layout_pending.get() {
            return;
        }
        let generation = w
            .gpu
            .borrow()
            .as_ref()
            .and_then(|g| g.session.workspace_layout_generation());
        if generation.is_some() && generation == self.captured_generation.get() {
            self.layout_pending.set(false);
            return;
        }
        if let Some(manager) = &self.manager
            && let Some(gpu) = w.gpu.borrow_mut().as_mut()
            && let Ok(capture) = gpu.session.capture_workspace()
        {
            manager.observe(capture, now_ms());
            self.layout_pending.set(false);
            self.captured_generation.set(generation);
        }
    }
    fn tick(&self, w: &Rc<Workspace>) {
        if !self.ready.get() || self.busy.get() {
            return;
        }
        let Some(manager) = &self.manager else {
            return;
        };
        if !manager.lease_valid(now_ms()) {
            if !self.owner_lost.get() {
                self.revalidate(w);
            }
            return;
        }
        self.capture(w);
        if self.last_maintenance.get().elapsed() >= Duration::from_secs(60)
            && !manager.saving()
            && manager.error().is_none()
            && w.gpu
                .borrow()
                .as_ref()
                .is_some_and(|g| g.session.require_workspace_idle().is_ok())
        {
            self.last_maintenance.set(Instant::now());
            glib::spawn_future_local(glib::clone!(
                #[weak]
                w,
                async move {
                    let Ok(_operation) = w.workspaces.begin_operation(&w).await else {
                        return;
                    };
                    let manager = w.workspaces.manager.as_ref().unwrap();
                    match manager.maintain_storage(false).await {
                        Ok(Some(incoming)) => w.workspaces.adopt(&w, Ok(incoming)).await,
                        Ok(None) => (),
                        Err(error) => w.workspaces.show_error(error),
                    }
                }
            ));
            return;
        }
        if self.last_renew.get().elapsed() >= Duration::from_millis(layer_workspace::OWNER_RENEW_MS)
        {
            self.last_renew.set(Instant::now());
            let manager = manager.clone();
            glib::spawn_future_local(glib::clone!(
                #[weak]
                w,
                async move {
                    let id = manager.active_id();
                    let result = manager.renew().await;
                    if manager.active_id() != id {
                        return;
                    }
                    if let Err(error) = result {
                        w.workspaces.show_error(error);
                        if !manager.lease_valid(now_ms()) {
                            w.workspaces.owner_lost.set(true);
                            if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                                gpu.session.set_workspace_read_only(true);
                            }
                        }
                    }
                    w.workspaces.refresh_interrupted().await;
                    w.workspaces.update_status();
                }
            ));
        }
        if manager.dirty()
            && !manager.saving()
            && manager.error().is_none()
            && (self.last_edit.get().elapsed() >= Duration::from_millis(250)
                || self.last_save.get().elapsed() >= Duration::from_secs(2))
        {
            self.save(w, false);
        }
    }
    pub fn accepts_input(&self, w: &Rc<Workspace>) -> bool {
        if self.manager.is_none() {
            return true;
        }
        if self.busy.get() || self.validating_owner.get() {
            return false;
        }
        if !self.ready.get() {
            return true;
        }
        if self.owner_lost.get() {
            return false;
        }
        if self.manager.as_ref().unwrap().lease_valid(now_ms()) {
            return true;
        }
        self.revalidate(w);
        false
    }
    pub fn revalidate(&self, w: &Rc<Workspace>) {
        if !self.ready.get() || self.busy.get() || self.validating_owner.replace(true) {
            return;
        }
        let manager = self.manager.as_ref().unwrap().clone();
        let id = manager.active_id();
        w.surface.set_sensitive(false);
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                while manager.saving() {
                    glib::timeout_future(Duration::from_millis(10)).await;
                }
                let result = manager.revalidate_owner(now_ms()).await;
                if manager.active_id() == id {
                    w.workspaces.owner_lost.set(result.is_err());
                    if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                        gpu.session.set_workspace_read_only(result.is_err());
                    }
                    if let Err(error) = result {
                        w.workspaces.show_error(error);
                    }
                }
                w.workspaces.validating_owner.set(false);
                w.surface.set_sensitive(!w.workspaces.busy.get());
                w.workspaces.update_status();
            }
        ));
    }
    fn save(&self, w: &Rc<Workspace>, retry: bool) {
        let Some(manager) = self.manager.clone() else {
            return;
        };
        if self.busy.get() || manager.saving() || (!retry && manager.error().is_some()) {
            return;
        }
        self.capture(w);
        self.last_save.set(Instant::now());
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                if let Err(error) = manager.save_once().await {
                    w.workspaces.show_error(error);
                }
                w.workspaces.update_status();
            }
        ));
    }
    pub fn show_error(&self, error: StoreError) {
        if let Some(manager) = &self.manager
            && manager.error().as_ref() != Some(&error)
        {
            manager.set_error(error.clone());
        }
        self.label.set_text(&error.message);
        self.label.add_css_class("error");
        self.retry.set_visible(true);
        self.recovery.set_visible(true);
    }
    pub fn update_status(&self) {
        let Some(manager) = &self.manager else {
            return;
        };
        if let Some(error) = manager.error() {
            self.root.set_visible(true);
            self.label.set_text(&format!(
                "{} — {}",
                manager
                    .active_name()
                    .unwrap_or_else(|| "Session-only workspace".into()),
                error.message
            ));
            self.label.add_css_class("error");
            self.retry.set_visible(true);
            self.recovery.set_visible(true);
        } else if self.interrupted_count.get() > 0 && !self.busy.get() {
            self.root.set_visible(true);
            self.retry.set_visible(false);
            self.recovery.set_visible(true);
            self.label.remove_css_class("error");
            self.label.set_text(
                &self.interruption_error.borrow().clone().unwrap_or_else(|| {
                    format!(
                        "{} interrupted changes are available in Storage and Backups.",
                        self.interrupted_count.get()
                    )
                }),
            );
        } else {
            self.root.set_visible(self.busy.get() || !self.ready.get());
            self.label.remove_css_class("error");
            self.retry.set_visible(false);
            self.recovery.set_visible(false);
            self.label.set_text(&format!(
                "{} · {}",
                manager
                    .active_name()
                    .unwrap_or_else(|| "My Workspace".into()),
                if self.busy.get() {
                    "Opening workspace…"
                } else if manager.dirty() || manager.saving() {
                    "Saving changes…"
                } else {
                    "Changes saved automatically"
                }
            ));
        }
    }
    pub(super) async fn refresh_interrupted(&self) {
        let Some(manager) = &self.manager else {
            return;
        };
        match manager.interrupted_changes(now_ms()).await {
            Ok(changes) => {
                self.interrupted_count.set(changes.len());
                *self.interruption_error.borrow_mut() = None;
            }
            Err(error) => {
                self.interrupted_count.set(1);
                *self.interruption_error.borrow_mut() = Some(format!(
                    "Interrupted changes need recovery: {error}. Export Original Database preserves the stored records."
                ));
            }
        }
    }
    /// Return true while the close must wait for acknowledged workspace writes.
    pub fn request_close(&self, w: &Rc<Workspace>) -> bool {
        let Some(manager) = self.manager.clone() else {
            return false;
        };
        if self.close_ready.get() {
            return false;
        }
        if self.busy.get() {
            self.close_requested.set(true);
            return true;
        }
        if !self.ready.get() {
            let capture = w
                .gpu
                .borrow_mut()
                .as_mut()
                .and_then(|g| g.session.capture_workspace().ok());
            if capture.as_ref() != self.failed_snapshot.borrow().as_ref() {
                self.show_error(StoreError::new(layer_workspace::ErrorKind::Unavailable,
                    "Workspace storage is unavailable. Keep this window open to recover your changes with Retry or export."));
                self.recover_close(w);
                return true;
            }
        }
        self.close_requested.set(true);
        if self.busy.get() {
            return true;
        }
        let captured = w
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.capture_workspace());
        if let Some(Ok(capture)) = captured {
            manager.observe(capture, now_ms());
        }
        let transition = w
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.begin_workspace_transition());
        if let Some(Err(error)) = transition {
            self.close_requested.set(false);
            if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                gpu.session.reset_document_close();
            }
            self.show_error(StoreError::invalid(error));
            self.update_status();
            return true;
        }
        self.busy.set(true);
        w.surface.set_sensitive(false);
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                while manager.saving() {
                    glib::timeout_future(Duration::from_millis(10)).await;
                }
                match manager.close().await {
                    Ok(()) => {
                        w.workspaces.close_ready.set(true);
                        w.window.close();
                    }
                    Err(error) => {
                        w.workspaces.close_requested.set(false);
                        if let Some(gpu) = w.gpu.borrow_mut().as_mut() {
                            gpu.session.end_workspace_transition();
                        }
                        w.workspaces.show_error(error);
                        w.surface.set_sensitive(true);
                        w.workspaces.recover_close(&w);
                    }
                }
                w.workspaces.busy.set(false);
            }
        ));
        true
    }
}
