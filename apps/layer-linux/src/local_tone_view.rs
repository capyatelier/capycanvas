//! One superseding local-analysis worker per window. Pad changes reuse its
//! immutable document-space guide; no image work occurs on the GTK thread.
use crate::workspace::Workspace;
use gtk::{gio, glib, prelude::*};
use layer_core::{Layer, color::DocumentColor};
use layer_render_wgpu::snapshot::CaptureControl;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq)]
struct Key {
    epoch: u64,
    owner: u64,
    color: DocumentColor,
    extent: [u32; 2],
    background: [f32; 4],
    layers: Vec<Layer>,
}
pub(crate) struct LocalToneView {
    pub label: gtk::Label,
    wanted: RefCell<Option<Key>>,
    published: RefCell<Option<Key>>,
    control: RefCell<Option<CaptureControl>>,
    running: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    changed: Cell<Instant>,
    failed: Cell<bool>,
    closed: Cell<bool>,
    analysed_time: Cell<f32>,
    last_start: Cell<Instant>,
    completed: Cell<u64>,
}
impl LocalToneView {
    pub fn new() -> Rc<Self> {
        let label = gtk::Label::new(None);
        label.set_visible(false);
        label.add_css_class("status-bubble");
        label.set_widget_name("local-tone-status");
        Rc::new(Self {
            label,
            wanted: Default::default(),
            published: Default::default(),
            control: Default::default(),
            running: Cell::new(false),
            timer: Default::default(),
            changed: Cell::new(Instant::now()),
            failed: Cell::new(false),
            closed: Cell::new(false),
            analysed_time: Cell::new(0.),
            last_start: Cell::new(Instant::now()),
            completed: Cell::new(0),
        })
    }
    fn key(w: &Workspace) -> Option<Key> {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref()?.session;
        if session.rendering_suspended() {
            return None;
        }
        let d = session.engine().document();
        if !d.color.depth.is_float() { return None; }
        Some(Key {
            epoch: session.state().document_file.epoch,
            owner: session.engine().backend().proof_owner,
            color: d.color,
            extent: [d.width, d.height],
            background: session.engine().view().background_rgba_linear,
            layers: d.layers.iter().map(Layer::composite_snapshot).collect(),
        })
    }
    pub fn suspend(&self) {
        self.closed.set(true);
        if let Some(control) = self.control.borrow().as_ref() { control.cancel(); }
    }
    pub fn resume(&self) { self.closed.set(false); }
    pub async fn pause(&self) {
        self.suspend();
        while self.running.get() { glib::timeout_future(Duration::from_millis(5)).await; }
        self.wanted.borrow_mut().take();
        self.published.borrow_mut().take();
    }
    pub fn sync(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.timer.borrow().is_none() {
            *self.timer.borrow_mut() = Some(glib::timeout_add_local(
                Duration::from_millis(100),
                glib::clone!(
                    #[weak(rename_to=state)]
                    self,
                    #[weak]
                    w,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        state.tick(&w);
                        glib::ControlFlow::Continue
                    }
                ),
            ));
        }
        self.tick(w);
    }
    fn tick(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.closed.get() {
            return;
        }
        let key = Self::key(w);
        if *self.wanted.borrow() != key {
            if let Some(control) = self.control.borrow().as_ref() {
                control.cancel();
            }
            *self.wanted.borrow_mut() = key.clone();
            self.published.borrow_mut().take();
            self.changed.set(Instant::now());
            if let Some(g) = w.gpu.borrow().as_ref() {
                let _ = g.session.engine().backend().set_local_tone(None);
            }
            self.failed.set(false);
            self.label.set_tooltip_text(None);
            self.label.set_label("Preparing SDR…");
        }
        self.label
            .set_visible(key.is_some() && (self.failed.get() || *self.published.borrow() != key));
        let Some(key) = key else {
            return;
        };
        let time = w
            .gpu
            .borrow()
            .as_ref()
            .map(|g| {
                if g.session.engine().document().has_animated_effects() {
                    g.session.engine().animation_time()
                } else {
                    0.
                }
            })
            .unwrap_or(0.);
        // Moving imagery reuses the latest completed guide while a superseding
        // frame is analysed, at most twice a second. Never cancel every frame.
        let refresh = time != self.analysed_time.get()
            && self.last_start.get().elapsed() >= Duration::from_millis(500);
        if self.running.get()
            || (self.published.borrow().as_ref() == Some(&key) && !refresh)
            || self.changed.get().elapsed() < Duration::from_millis(180)
        {
            return;
        }
        let snapshot = (|| {
            let gpu = w.gpu.borrow();
            let session = &gpu.as_ref()?.session;
            if session.require_document_snapshot_idle().is_err() {
                return None;
            }
            Some((
                session.capture_project_recovery().ok()?,
                w.snapshot_gpu().ok()?,
            ))
        })();
        let Some((project, gpu)) = snapshot else {
            return;
        };
        self.last_start.set(Instant::now());
        self.running.set(true);
        let control = CaptureControl::default();
        *self.control.borrow_mut() = Some(control.clone());
        // Snapshot workers own live GPU objects. Keep the application/driver
        // alive until cancellation has finished and those objects are dropped.
        let hold = w.window.application().map(|app| app.hold());
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to=state)]
            self,
            #[weak]
            w,
            async move {
                let _hold = hold;
                let c = control.clone();
                let background = key.background;
                let result = gio::spawn_blocking(move || {
                    let mut renderer = gpu
                        .capture(project, background, time, Default::default(), c)
                        .map_err(|e| e.to_string())?;
                    renderer.local_tone_guide()
                })
                .await
                .map_err(|_| "Local tone worker stopped".to_string())
                .and_then(|r| r);
                state.control.borrow_mut().take();
                if !state.closed.get()
                    && !control.is_cancelled()
                    && Self::key(&w).as_ref() == Some(&key)
                {
                    let result = match result {
                        Ok(guide) => Self::publish(&w, guide).await,
                        Err(e) => Err(e),
                    };
                    if Self::key(&w).as_ref() == Some(&key) {
                        *state.published.borrow_mut() = Some(key);
                        state.analysed_time.set(time);
                        state.completed.set(state.completed.get() + 1);
                        state.failed.set(result.is_err());
                        match result {
                            Ok(()) => state.label.set_visible(false),
                            Err(e) => {
                                state.label.set_label("Local SDR unavailable");
                                state.label.set_tooltip_text(Some(&e));
                                state.label.set_visible(true);
                                eprintln!("Local SDR: {e}");
                            }
                        }
                    }
                }
                state.running.set(false);
                w.wake();
            }
        ));
    }
    #[cfg(test)]
    pub fn worker_state(&self) -> (bool, Option<bool>) {
        (self.running.get(), self.control.borrow().as_ref().map(CaptureControl::is_cancelled))
    }
    #[cfg(test)]
    pub fn ready_count(&self) -> Option<u64> {
        (!self.failed.get()
            && self.published.borrow().is_some()
            && *self.published.borrow() == *self.wanted.borrow())
        .then_some(self.completed.get())
    }
    async fn publish(
        w: &Workspace,
        guide: Arc<layer_core::color::hdr::LocalToneGuide>,
    ) -> Result<(), String> {
        let rx = w
            .gpu
            .borrow()
            .as_ref()
            .ok_or("Canvas unavailable")?
            .session
            .engine()
            .backend()
            .set_local_tone(Some(guide))?;
        let start = Instant::now();
        loop {
            match rx.try_recv() {
                Ok(r) => return r,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("Local tone GPU publication stopped".into());
                }
                Err(_) => {}
            }
            if start.elapsed() > Duration::from_secs(30) {
                return Err("Local tone GPU publication timed out".into());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    }
}
impl Drop for LocalToneView {
    fn drop(&mut self) {
        if let Some(c) = self.control.get_mut().take() {
            c.cancel();
        }
        if let Some(t) = self.timer.get_mut().take() {
            t.remove();
        }
    }
}
