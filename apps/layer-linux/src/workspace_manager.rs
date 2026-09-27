//! GTK widgets, input gate, window activation and the owner-window registry
//! around the shared workspace controller.
use super::*;
use layer_workspace::{
    ManagerAction, ManagerPage, StoreWorker, WorkspaceController, WorkspaceInput, WorkspaceView,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type Controller = WorkspaceController<StoreWorker>;
thread_local! {
    static WINDOWS: RefCell<std::collections::BTreeMap<String, std::rc::Weak<Workspace>>> = RefCell::new(std::collections::BTreeMap::new());
}
#[path = "workspace_manager_dialog.rs"]
mod dialog;
#[path = "workspace_manager_storage.rs"]
mod storage;
#[path = "workspace_switcher.rs"]
mod switcher;
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn window(owner: &str) -> Option<Rc<Workspace>> {
    WINDOWS.with(|windows| windows.borrow().get(owner).and_then(std::rc::Weak::upgrade))
}

pub(crate) struct NativeWorkspaces {
    pub ui: dialog::ManagerUi,
    store: RefCell<Option<StoreWorker>>,
    controller: RefCell<Option<Controller>>,
    view: RefCell<WorkspaceView>,
    pub root: gtk::Box,
    pub label: gtk::Label,
    pub switcher: gtk::Box,
    switch_body: gtk::Box,
    switch_buttons: RefCell<Vec<(String, gtk::ToggleButton)>>,
    switch_owner: RefCell<std::rc::Weak<Workspace>>,
    retry: gtk::Button,
    recovery: gtk::Button,
    close_requested: Cell<bool>,
    close_prompt: Cell<bool>,
    focused: RefCell<Option<layer_workspace::FocusTarget>>,
    switcher_revision: Cell<u64>,
    #[cfg(test)]
    paused: Cell<bool>,
}
impl NativeWorkspaces {
    pub fn new() -> Self {
        let directory = std::env::var_os("CAPY_WORKSPACE_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                (!cfg!(test)).then(|| glib::user_data_dir().join("art.capycanvas.CapyCanvas"))
            });
        let store = directory.and_then(|directory| StoreWorker::shared(&directory).ok());
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
        let recovery = gtk::Button::with_label("Save as New Workspace…");
        recovery.set_visible(false);
        root.append(&recovery);
        root.set_visible(store.is_some());
        let (switcher, switch_body) = switcher::build();
        switcher.set_sensitive(false);
        Self {
            ui: dialog::ManagerUi::new(),
            store: RefCell::new(store),
            controller: RefCell::new(None),
            view: RefCell::new(WorkspaceView::default()),
            root,
            label,
            switcher,
            switch_body,
            switch_buttons: RefCell::new(Vec::new()),
            switch_owner: RefCell::new(std::rc::Weak::new()),
            retry,
            recovery,
            close_requested: Cell::new(false),
            close_prompt: Cell::new(false),
            focused: RefCell::new(None),
            switcher_revision: Cell::new(0),
            #[cfg(test)]
            paused: Cell::new(false),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        // Pausing input through GTK sensitivity restyles every descendant and
        // clears native focus/gesture state. Intercept new input instead, while
        // still letting releases and cancellation finish existing sequences.
        let gate = gtk::EventControllerLegacy::new();
        gate.set_name(Some("workspace-input-pause"));
        gate.set_propagation_phase(gtk::PropagationPhase::Capture);
        gate.connect_event(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Stop,
            move |_, event| {
                if matches!(
                    event.event_type(),
                    gdk::EventType::ButtonPress
                        | gdk::EventType::KeyPress
                        | gdk::EventType::MotionNotify
                        | gdk::EventType::Scroll
                        | gdk::EventType::TouchBegin
                        | gdk::EventType::TouchUpdate
                        | gdk::EventType::TouchpadSwipe
                        | gdk::EventType::TouchpadPinch
                        | gdk::EventType::TouchpadHold
                ) && !w.workspaces.accepts_input(&w)
                {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        w.surface.add_controller(gate);
        self.ui.bind(w);
        self.bind_switcher(w);
        self.recovery.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let view = w.workspaces.view();
                let action = if view.interrupted > 0 && view.error.is_none() {
                    ManagerAction::RecoverInterrupted
                } else {
                    ManagerAction::SaveAsNew
                };
                w.workspaces.send(&w, WorkspaceInput::Form { action });
            }
        ));
        self.retry.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| w.workspaces.send(&w, WorkspaceInput::Retry)
        ));
        if self.store.borrow().is_none() {
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
        let Some(store) = self.store.borrow_mut().take() else {
            return;
        };
        let mut controller = Controller::new(store, Platform::Gtk, now_ms());
        let owner = controller.manager.owner.id.clone();
        let pending = Arc::new(AtomicBool::new(false));
        let target = owner.clone();
        controller.set_wake(Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
                let pending = pending.clone();
                let target = target.clone();
                glib::idle_add_once(move || {
                    pending.store(false, Ordering::Release);
                    if let Some(w) = window(&target) {
                        w.workspaces.tick(&w);
                    }
                });
            }
        }));
        WINDOWS.with(|windows| windows.borrow_mut().insert(owner, Rc::downgrade(w)));
        *self.controller.borrow_mut() = Some(controller);
        self.tick(w);
    }
    #[cfg(test)]
    pub fn manager(&self) -> Option<Rc<layer_workspace::WorkspaceManager<StoreWorker>>> {
        self.controller.borrow().as_ref().map(|c| c.manager.clone())
    }
    /// The controller view as last rendered.
    pub fn view(&self) -> WorkspaceView {
        self.view.borrow().clone()
    }
    #[cfg(test)]
    pub fn ready(&self) -> bool {
        self.controller.borrow().is_none() && self.store.borrow().is_none()
            || self.view.borrow().ready
    }
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.view.borrow().busy
    }
    /// Startup has not adopted a workspace yet.
    pub fn starting(&self) -> bool {
        self.controller.borrow().is_some() && !self.view.borrow().ready
    }
    pub fn observe(&self, w: &Workspace, regions: u32) {
        if let Ok(mut controller) = self.controller.try_borrow_mut()
            && let Some(controller) = controller.as_mut()
            && let Ok(mut gpu) = w.gpu.try_borrow_mut()
            && let Some(gpu) = gpu.as_mut()
        {
            controller.observe_regions(&mut gpu.session, regions, now_ms());
        }
    }
    fn run(
        &self,
        w: &Rc<Workspace>,
        step: impl FnOnce(
            &mut Controller,
            &mut UiSession<crate::render_thread::RenderWorker>,
        ) -> UiChange,
    ) -> bool {
        let change = {
            let (Ok(mut controller), Ok(mut gpu)) =
                (self.controller.try_borrow_mut(), w.gpu.try_borrow_mut())
            else {
                return false;
            };
            let (Some(controller), Some(gpu)) = (controller.as_mut(), gpu.as_mut()) else {
                return true;
            };
            step(controller, &mut gpu.session)
        };
        if change.regions != 0 || change.canvas_wake {
            w.changed(Ok(change));
        }
        true
    }
    pub fn tick(&self, w: &Rc<Workspace>) {
        let ticked = self.run(w, |controller, session| controller.tick(session, now_ms()));
        if !ticked {
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                w,
                move || w.workspaces.tick(&w)
            ));
            return;
        }
        self.render(w);
    }
    pub fn send(&self, w: &Rc<Workspace>, input: WorkspaceInput) {
        let mut input = Some(input);
        let sent = self.run(w, |controller, session| {
            match controller.input(session, input.take().unwrap(), now_ms()) {
                Ok(change) => change,
                Err(error) => {
                    controller.view.error = Some(error.to_string());
                    UiChange::default()
                }
            }
        });
        if !sent {
            let input = input.take().unwrap();
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                w,
                move || w.workspaces.send(&w, input)
            ));
            return;
        }
        self.tick(w);
    }
    pub fn accepts_input(&self, _w: &Rc<Workspace>) -> bool {
        #[cfg(test)]
        if self.paused.get() {
            return false;
        }
        let controller = self.controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return true;
        };
        let view = &controller.view;
        !view.busy
            && !view.owner_lost
            && !view.closing
            && !view.closed
            && (!view.ready || controller.manager.lease_valid(now_ms()))
    }
    fn render(&self, w: &Rc<Workspace>) {
        let Some(view) = self
            .controller
            .borrow()
            .as_ref()
            .map(|controller| controller.view.clone())
        else {
            return;
        };
        *self.view.borrow_mut() = view.clone();
        w.surface
            .update_state(&[gtk::accessible::State::Busy(view.busy)]);
        self.update_status(&view);
        self.update_switcher();
        self.ui.render(w, &view);
        if view.ready
            && self.switcher_revision.replace(view.switcher_revision) != view.switcher_revision
        {
            for other in WINDOWS.with(|windows| {
                windows
                    .borrow()
                    .values()
                    .filter_map(std::rc::Weak::upgrade)
                    .collect::<Vec<_>>()
            }) {
                if !Rc::ptr_eq(&other, w) {
                    glib::idle_add_local_once(move || {
                        other
                            .workspaces
                            .send(&other, WorkspaceInput::RefreshSwitcher)
                    });
                }
            }
        }
        if view.focus_window != *self.focused.borrow() {
            *self.focused.borrow_mut() = view.focus_window.clone();
            if let Some(target) = view.focus_window {
                match window(&target.owner) {
                    Some(other) => other.window.present(),
                    None => {
                        glib::idle_add_local_once(glib::clone!(
                            #[weak]
                            w,
                            move || {
                                w.workspaces.send(
                                &w,
                                WorkspaceInput::FocusFailed {
                                    error: "This workspace is open in another application window. Close it there or try again after it closes.".into(),
                                },
                            )
                            }
                        ));
                    }
                }
            }
        }
        if self.close_requested.get() {
            if view.closed {
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    w,
                    move || w.window.close()
                ));
            } else if view.closing && view.error.is_some() && !view.busy {
                self.recover_close(w);
            }
        }
    }
    fn update_status(&self, view: &WorkspaceView) {
        if self.controller.borrow().is_none() {
            return;
        }
        let name = if view.name.is_empty() {
            "My Workspace"
        } else {
            &view.name
        };
        if let Some(error) = &view.error {
            self.recovery.set_label("Save as New Workspace…");
            self.root.set_visible(true);
            self.label.set_text(&format!("{name} — {error}"));
            self.label.add_css_class("error");
            self.retry.set_visible(true);
            self.recovery.set_visible(true);
        } else if view.interrupted > 0 && !view.busy {
            self.recovery.set_label("Recover Changes…");
            self.root.set_visible(true);
            self.retry.set_visible(false);
            self.recovery.set_visible(true);
            self.label.remove_css_class("error");
            self.label.set_text(&format!(
                "{} unsaved changes can be recovered.",
                view.interrupted
            ));
        } else {
            // Routine switches and autosaves need no flashing status message.
            // Keep startup and actionable recovery visible.
            self.root.set_visible(!view.ready);
            self.label.remove_css_class("error");
            self.retry.set_visible(false);
            self.recovery.set_visible(false);
            self.label.set_text(&format!(
                "{name} · {}",
                if !view.ready {
                    "Opening workspace…"
                } else if view.dirty || view.saving {
                    "Saving changes…"
                } else {
                    "Changes saved automatically"
                }
            ));
        }
    }
    /// Return true while the close must wait for acknowledged workspace writes.
    pub fn request_close(&self, w: &Rc<Workspace>) -> bool {
        let view = {
            let controller = self.controller.borrow();
            let Some(controller) = controller.as_ref() else {
                return false;
            };
            controller.view.clone()
        };
        if view.closed {
            return false;
        }
        self.close_requested.set(true);
        if !view.closing {
            self.send(w, WorkspaceInput::Close);
        }
        true
    }
    pub fn open(&self, w: &Rc<Workspace>, page: ManagerPage) {
        self.send(w, WorkspaceInput::Open { page });
    }
    #[cfg(test)]
    pub fn pause(&self, w: &Rc<Workspace>, paused: bool) {
        self.paused.set(paused);
        self.render(w);
    }
    #[cfg(test)]
    pub fn report_error(&self, w: &Rc<Workspace>, error: &str) {
        if let Some(controller) = self.controller.borrow_mut().as_mut() {
            controller.view.error = Some(error.into());
        }
        self.render(w);
    }
}
