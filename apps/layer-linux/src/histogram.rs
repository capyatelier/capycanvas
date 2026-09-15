//! Nonmodal document inspection. One cancellable worker per drawing; refresh
//! only after the committed revision settles, never from a display thumbnail.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::{Revision, color::histogram::Histogram};
use layer_render_wgpu::snapshot::CaptureControl;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
struct Key {
    epoch: u64,
    revision: Revision,
    background: [f32; 4],
}
pub(crate) struct Inspector {
    pub(crate) window: adw::Window,
    workspace: Weak<Workspace>,
    chart: gtk::DrawingArea,
    channel: gtk::DropDown,
    logarithmic: gtk::CheckButton,
    automatic: gtk::CheckButton,
    status: gtk::Label,
    description: gtk::Label,
    range: gtk::Label,
    pub(crate) result: RefCell<Option<Histogram>>,
    wanted: Cell<Option<Key>>,
    completed: Cell<Option<Key>>,
    changed_at: Cell<Instant>,
    sampled_time: Cell<Option<f32>>,
    running: Cell<bool>,
    control: RefCell<Option<CaptureControl>>,
    timer: RefCell<Option<glib::SourceId>>,
}
impl Drop for Inspector {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.get_mut().take() {
            timer.remove();
        }
        if let Some(control) = self.control.get_mut().take() {
            control.cancel();
        }
        self.window.destroy();
    }
}
impl Inspector {
    fn key(&self) -> Option<Key> {
        let w = self.workspace.upgrade()?;
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref()?.session;
        Some(Key {
            epoch: session.state().document_file.epoch,
            revision: session.engine().document().revision,
            background: session.engine().view().background_rgba_linear,
        })
    }
    fn cancel(&self) {
        if let Some(control) = self.control.borrow().as_ref() {
            control.cancel();
        }
    }
    fn pause(&self) {
        self.cancel();
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
        self.status
            .set_label("Paused · showing the last completed inspection");
    }
    fn start(self: &Rc<Self>) {
        if self.timer.borrow().is_some() || !self.automatic.is_active() {
            return;
        }
        self.completed.set(None);
        // Closing/reopening keeps the same state and active worker, so it cannot
        // accumulate abandoned GPU captures. A successor waits for cancellation.
        let weak = Rc::downgrade(self);
        *self.timer.borrow_mut() = Some(glib::timeout_add_local(
            Duration::from_millis(150),
            move || {
                let Some(state) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                state.tick();
                glib::ControlFlow::Continue
            },
        ));
        self.tick();
    }
    fn tick(self: &Rc<Self>) {
        if !self.window.is_visible() {
            return;
        }
        let Some(key) = self.key() else {
            self.cancel();
            return;
        };
        if self.wanted.get() != Some(key) {
            self.wanted.set(Some(key));
            self.changed_at.set(Instant::now());
            self.cancel();
            self.status
                .set_label("Updating · showing the previous inspection");
        }
        if self.running.get()
            || self.completed.get() == Some(key)
            || self.changed_at.get().elapsed() < Duration::from_millis(300)
        {
            return;
        }
        let Some(w) = self.workspace.upgrade() else {
            return;
        };
        let snapshot = {
            let gpu = w.gpu.borrow();
            let Some(gpu) = gpu.as_ref() else {
                return;
            };
            // An active contact / transform stays out of the snapshot. Leave
            // the last result marked stale until the operation is committed.
            if gpu.session.require_document_snapshot_idle().is_err()
                || gpu.session.state().host_error.is_some()
            {
                return;
            }
            let Ok(snapshot_gpu) = gpu.session.engine().backend().snapshot_gpu() else {
                return;
            };
            gpu.session
                .capture_project_recovery()
                .map(|p| (p, gpu.session.engine().animation_time(), snapshot_gpu))
        };
        let (project, time, snapshot_gpu) = match snapshot {
            Ok(p) => p,
            Err(error) => {
                self.status.set_label(&error);
                self.completed.set(Some(key));
                return;
            }
        };
        let animated = project.document.has_animated_effects();
        self.running.set(true);
        self.status
            .set_label("Updating · complete composite at full resolution");
        let control = CaptureControl::default();
        *self.control.borrow_mut() = Some(control.clone());
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let worker_control = control.clone();
            let result = gio::spawn_blocking(move || {
                let mut renderer = snapshot_gpu.capture(
                    project,
                    key.background,
                    time,
                    Default::default(),
                    worker_control,
                )
                .map_err(|e| e.to_string())?;
                renderer.histogram().map_err(|e| e.to_string())
            })
            .await
            .map_err(|_| "Histogram worker failed".to_string())
            .and_then(|r| r);
            let Some(state) = weak.upgrade() else {
                return;
            };
            state.running.set(false);
            state.control.borrow_mut().take();
            if control.is_cancelled() || state.key() != Some(key) || !state.window.is_visible() {
                return;
            }
            state.completed.set(Some(key));
            match result {
                Ok(result) => {
                    state.sampled_time.set(animated.then_some(time));
                    state.result.replace(Some(result));
                    state
                        .status
                        .set_label("Current · complete committed composite");
                    state.refresh();
                }
                Err(error) => state.status.set_label(&error),
            }
        });
    }
    fn refresh(&self) {
        self.chart.queue_draw();
        let result = self.result.borrow();
        let Some(result) = result.as_ref() else {
            return;
        };
        self.description.set_label(&format!("{} · {}-bit document\n{} pixels counted · {} fully transparent pixels excluded\nRGB is profile-encoded; luminance is linear relative Y. Partial coverage is unassociated; each pixel counts once.", result.color.space.name(), result.color.depth.bits(), result.pixels, result.transparent));
        if let Some(time) = self.sampled_time.get() {
            self.description.set_label(&format!(
                "{}\nAnimation sampled at {time:.2} s. Pause and resume to sample again.",
                self.description.text()
            ));
        }
        let indices: &[usize] = match self.channel.selected() {
            1 => &[0],
            2 => &[1],
            3 => &[2],
            4 => &[3],
            _ => &[0, 1, 2],
        };
        let percent = |count: u64| {
            if result.pixels == 0 {
                0.
            } else {
                count as f64 * 100. / result.pixels as f64
            }
        };
        let text = indices
            .iter()
            .map(|&i| {
                let c = &result.channels[i];
                format!(
                    "{}: ≤0 {:.2}% · ≥1 {:.2}%\nOutside SDR: <0 {:.2}% · >1 {:.2}%",
                    ["Red", "Green", "Blue", "Luminance"][i],
                    percent(c.black),
                    percent(c.white),
                    percent(c.below),
                    percent(c.above)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.range.set_label(&text);
    }
}

pub(crate) fn show(w: &Rc<Workspace>) {
    if let Some(inspector) = w.histogram.borrow().as_ref() {
        inspector.window.present();
        inspector.start();
        return;
    }
    let window = adw::Window::builder()
        .title("Histogram")
        .transient_for(&w.window)
        .destroy_with_parent(true)
        .hide_on_close(true)
        .default_width(430)
        .default_height(720)
        .build();
    window.set_widget_name("histogram-window");
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    let label = |text: &str, name: &str| {
        let label = gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.)
            .build();
        label.set_widget_name(name);
        label
    };
    body.append(&label(
        "Visible composite · document values",
        "histogram-source",
    ));
    let channel = gtk::DropDown::from_strings(&["RGB", "Red", "Green", "Blue", "Luminance"]);
    channel.set_widget_name("histogram-channel");
    channel.set_tooltip_text(Some("Profile-encoded RGB or linear relative luminance"));
    body.append(&channel);
    let chart = gtk::DrawingArea::builder()
        .content_width(384)
        .content_height(170)
        .hexpand(true)
        .build();
    chart.set_widget_name("histogram-chart");
    body.append(&chart);
    let axis = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let zero = label("0", "histogram-axis-zero");
    zero.set_hexpand(true);
    axis.append(&zero);
    axis.append(&label("1", "histogram-axis-one"));
    body.append(&axis);
    let options = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let logarithmic = gtk::CheckButton::with_label("Log scale");
    let automatic = gtk::CheckButton::with_label("Update automatically");
    automatic.set_active(true);
    automatic.set_widget_name("histogram-automatic");
    options.append(&logarithmic);
    options.append(&automatic);
    body.append(&options);
    let status = label("Waiting for a committed canvas…", "histogram-status");
    let range = label("", "histogram-range");
    let description = label("", "histogram-description");
    for label in [&status, &range, &description] {
        body.append(label);
    }
    body.append(&label("Endpoint counts include black and white artwork; they do not prove lost detail. Outside-SDR counts describe this document space, not a monitor or export profile.", "histogram-help"));
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(720)
        .child(&body)
        .build();
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    let state = Rc::new(Inspector {
        window,
        workspace: Rc::downgrade(w),
        chart,
        channel,
        logarithmic,
        automatic,
        status,
        description,
        range,
        result: RefCell::new(None),
        wanted: Cell::new(None),
        completed: Cell::new(None),
        changed_at: Cell::new(Instant::now()),
        sampled_time: Cell::new(None),
        running: Cell::new(false),
        control: RefCell::new(None),
        timer: RefCell::new(None),
    });
    let weak = Rc::downgrade(&state);
    state.window.connect_close_request(move |_| {
        if let Some(state) = weak.upgrade() {
            state.pause();
        }
        glib::Propagation::Proceed
    });
    let weak = Rc::downgrade(&state);
    state.automatic.connect_toggled(move |button| {
        if let Some(state) = weak.upgrade() {
            if button.is_active() {
                state.start();
            } else {
                state.pause();
            }
        }
    });
    let weak = Rc::downgrade(&state);
    state.channel.connect_selected_notify(move |_| {
        if let Some(state) = weak.upgrade() {
            state.refresh();
        }
    });
    let weak = Rc::downgrade(&state);
    state.logarithmic.connect_toggled(move |_| {
        if let Some(state) = weak.upgrade() {
            state.chart.queue_draw();
        }
    });
    let weak = Rc::downgrade(&state);
    state.chart.set_draw_func(move |_, cr, width, height| {
        let Some(state) = weak.upgrade() else {
            return;
        };
        let result = state.result.borrow();
        let Some(result) = result.as_ref() else {
            return;
        };
        let indices: &[usize] = match state.channel.selected() {
            1 => &[0],
            2 => &[1],
            3 => &[2],
            4 => &[3],
            _ => &[0, 1, 2],
        };
        let scale = |count: u64| {
            if state.logarithmic.is_active() {
                (count as f64).ln_1p()
            } else {
                count as f64
            }
        };
        let max = indices
            .iter()
            .flat_map(|&i| &result.channels[i].bins)
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.25);
        cr.rectangle(0.5, 0.5, width as f64 - 1., height as f64 - 1.);
        let _ = cr.stroke();
        for &i in indices {
            let rgb = [
                [0.9, 0.2, 0.2],
                [0.2, 0.7, 0.3],
                [0.3, 0.5, 1.],
                [0.6, 0.6, 0.6],
            ][i];
            cr.set_source_rgba(rgb[0], rgb[1], rgb[2], 0.65);
            let bins = &result.channels[i].bins;
            for (x, &count) in bins.iter().enumerate() {
                let h = (height as f64 - 2.) * scale(count) / scale(max);
                cr.rectangle(
                    x as f64 * width as f64 / bins.len() as f64,
                    height as f64 - 1. - h,
                    width as f64 / bins.len() as f64 + 0.1,
                    h,
                );
            }
            let _ = cr.fill();
        }
    });
    *w.histogram.borrow_mut() = Some(state.clone());
    state.window.present();
    state.start();
}
