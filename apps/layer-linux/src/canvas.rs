//! Native session/pacing adapter. GPU ownership lives in render_thread.
use crate::{
    render_thread::RenderWorker,
    wayland::{Geometry, Parent},
};
use gtk::prelude::*;
use layer_render::ReadbackImage;
use layer_ui::{CanvasCursor, UiChange, UiSession};

pub struct GpuCanvas {
    pub session: UiSession<RenderWorker>,
    cursor: CanvasCursor,
    view_color: crate::display_color::ViewColor,
    // Alive until the worker and its borrowed Wayland handles are gone.
    _parent: gtk::gdk::Surface,
    pub needs_present: bool,
}
impl GpuCanvas {
    pub fn reattach(&mut self, area: &gtk::Picture) -> Result<(), String> {
        let parent = area
            .native()
            .and_then(|native| native.surface())
            .ok_or("GTK surface unavailable")?;
        let renderer = RenderWorker::new(
            Parent::new(&parent)?,
            area.downgrade().into(),
            self.session.engine().document().color,
        )?;
        let (previous, _) = self.session.replace_renderer(renderer)?;
        drop(previous);
        self._parent = parent;
        self.session.renderer_mut().finish_startup_cache()?;
        self.needs_present = true;
        Ok(())
    }
    pub fn update_cursor(&mut self) -> bool {
        self.session.update_canvas_cursor(&mut self.cursor);
        self.session.append_layer_overlay(&mut self.cursor.segments);
        let picker = self.session.color_picker_overlay();
        let renderer = self.session.renderer_mut();
        if renderer.cursor == self.cursor.segments && renderer.picker == picker {
            return false;
        }
        renderer.picker = picker;
        std::mem::swap(&mut renderer.cursor, &mut self.cursor.segments);
        self.needs_present = true;
        true
    }
    pub fn with_project(
        area: &gtk::Picture,
        project: Option<(layer_core::Project, Option<layer_ui::DocumentLocation>)>,
    ) -> Result<Self, String> {
        let parent = area
            .native()
            .and_then(|native| native.surface())
            .ok_or("GTK surface unavailable")?;
        // Load once before allocating document resources so a new window uses
        // the same explicit creation defaults as File → New.
        let settings = match crate::preferences::load() {
            Ok(settings) => settings.unwrap_or_default(),
            Err(error) => {
                eprintln!("{error}; using default preferences");
                Default::default()
            }
        };
        let (project, location) = match project {
            Some(project) => project,
            None => (settings.new_document.defaults.project()?, None),
        };
        let color = project.document.color;
        let renderer = RenderWorker::new(Parent::new(&parent)?, area.downgrade().into(), color)?;
        let mut session = UiSession::from_project(renderer, project, location, extent(area))?;
        session.set_platform(layer_ui::Platform::Gtk);
        session.dispatch(layer_ui::UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState::for_platform(
                layer_ui::Platform::Gtk,
            )),
        })?;
        // Prefer installed/development resources. The same runtime loader can
        // replace these files without recompiling the executable.
        let filters = std::env::var_os("CAPY_FILTERS_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.join("filters")))
                    .filter(|p| p.is_dir())
            })
            .or_else(|| {
                let p = std::path::PathBuf::from("assets/filters");
                p.is_dir().then_some(p)
            });
        if let Some(directory) = filters {
            let mode = std::env::var("CAPY_FILTERS_MODE").unwrap_or_else(|_| "merge".into());
            let result = serde_json::from_value(serde_json::Value::String(mode))
                .map_err(|e| e.to_string())
                .and_then(|mode| load_filter_directory(&mut session, &directory, mode, false));
            if let Err(error) = result {
                eprintln!("{error}; using bundled filters");
            }
        }
        session.renderer_mut().finish_startup_cache()?;
        session.dispatch(layer_ui::UiAction::RestoreSettings { settings })?;
        let (theme, accent) = crate::workspace::system_appearance();
        session.dispatch(layer_ui::UiAction::SystemThemeChanged { theme, accent })?;
        Ok(Self {
            session,
            cursor: CanvasCursor::default(),
            view_color: Default::default(),
            _parent: parent,
            needs_present: true,
        })
    }
    pub fn stroke_pacing(&self) -> bool {
        let engine = self.session.engine();
        engine.backend().startup.complete
            && engine.backend().clock.stroke_work_fits()
            && self.session.state().layer_tools.tool == layer_ui::LayerCanvasTool::Paint
            && (engine.has_active_stroke() || engine.has_pending_input())
            && !engine.has_pending_document_edits()
            && engine.transform_preview().is_none()
    }
    pub fn render(&mut self, area: &gtk::Picture, now_ns: u64, paced: bool) -> Result<UiChange, String> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let engine = self.session.engine();
        let transform = engine.transform_preview().is_some();
        if engine
            .backend()
            .startup_needs_update(engine.document(), engine.brush(), transform)
        {
            let (document, brush) = (engine.document().clone(), engine.brush().clone());
            self.session
                .renderer_mut()
                .prepare_startup(document, brush, transform)?;
        }
        let renderer = self.session.renderer_mut();
        if !renderer.ready()? {
            // Preserve queued pen samples; never stall GTK or drop paint.
            self.needs_present = true;
            return Ok(UiChange::default());
        }
        let view_color_changed = self.view_color != renderer.view_color;
        self.view_color = renderer.view_color;
        let geometry = Geometry::of(area);
        if renderer.geometry != Some(geometry) {
            renderer.geometry = Some(geometry);
        }
        let resized = self.session.set_viewport(
            [area.width().max(1) as f32, area.height().max(1) as f32],
            extent(area),
        )?;
        let surround = self.session.state().palette.surround_linear;
        self.session.renderer_mut().surround = surround;
        let presentation_ns = self.session.engine().backend().clock.presentation(now_ns);
        let clock = &self.session.engine().backend().clock;
        let period = clock.period();
        let lead = clock.stroke_lead(period);
        // Only train on a normal paced stroke frame. Immediate terminal wakes,
        // startup, resize, and the first frame during a mode change have their
        // own timing and must not be mistaken for compositor deadline misses.
        let target = (paced && self.stroke_pacing()
            && presentation_ns.saturating_sub(now_ns).abs_diff(lead) <= period / 8)
            .then_some(crate::wayland::StrokeTarget {
                presentation_ns, period_ns: period, lead_ns: lead,
            });
        self.session.renderer_mut().stroke_target = target;
        let mut changed = self.session.frame(now_ns, presentation_ns)?;
        changed.regions |= resized.regions;
        if view_color_changed {
            changed.regions |= layer_ui::regions::BRUSH
                | layer_ui::regions::SETTINGS
                | layer_ui::regions::DOCUMENT;
        }
        self.needs_present = !self.session.engine().backend().startup.complete;
        #[cfg(test)]
        {
            self.session
                .engine()
                .backend()
                .stats
                .lock()
                .unwrap()
                .input_cpu
                .push(start.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(changed)
    }
    pub fn capture(&mut self) -> Result<ReadbackImage, String> {
        self.session.renderer_mut().capture()
    }
}

/// Native file transport only; shared Rust owns all replacement policy. Hosts
/// may call this for additional packages without adding a shader editor UI.
pub(super) fn load_filter_directory(
    session: &mut UiSession<RenderWorker>,
    directory: &std::path::Path,
    mode: layer_core::EffectInstallMode,
    migrate: bool,
) -> Result<UiChange, String> {
    let manifest =
        std::fs::read_to_string(directory.join("manifest.json")).map_err(|e| e.to_string())?;
    let read = |name: &str| {
        std::fs::read_to_string(directory.join(name))
            .map(std::sync::Arc::from)
            .map_err(|e| e.to_string())
    };
    if migrate {
        session.load_effect_package(&manifest, read, mode)
    } else {
        session.load_effect_library(&manifest, read, mode)
    }
}
fn extent(area: &gtk::Picture) -> [u32; 2] {
    let scale = area.scale_factor() as u32;
    [
        area.width().max(1) as u32 * scale,
        area.height().max(1) as u32 * scale,
    ]
}

/// Independent display pacing (120 Hz fallback), not GTK's scene frame clock
/// which can fall back to 60 Hz when only our child changes. Uses the compositor's
/// presentation phase when available. timerfd uses absolute kernel intervals;
/// missed expirations coalesce, preserving input without replaying stale frames.
pub const FRAME_NS: u64 = 8_333_333;
pub struct FrameTimer {
    timer: std::rc::Rc<std::os::fd::OwnedFd>,
    period_ns: u64,
    expedited: std::cell::Cell<bool>,
}
impl FrameTimer {
    fn arm(&self, deadline_ns: u64) -> u64 {
        use std::os::fd::AsRawFd;
        let first = deadline_ns.max(gtk::glib::monotonic_time().max(0) as u64 * 1000 + 1);
        let interval = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: (self.period_ns / 1_000_000_000) as _,
                tv_nsec: (self.period_ns % 1_000_000_000) as _,
            },
            it_value: libc::timespec {
                tv_sec: (first / 1_000_000_000) as _,
                tv_nsec: (first % 1_000_000_000) as _,
            },
        };
        assert_eq!(
            unsafe {
                libc::timerfd_settime(
                    self.timer.as_raw_fd(),
                    libc::TFD_TIMER_ABSTIME,
                    &interval,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        first
    }
    /// Complete/cancel a contact promptly. Rearming the same timer coalesces
    /// terminal events and never adds a second frame source. The workspace
    /// realigns continued movement with presentation after this wake.
    pub fn expedite(&self) -> u64 {
        self.expedited.set(true);
        self.arm(0)
    }
    pub fn take_expedited(&self) -> bool {
        self.expedited.replace(false)
    }
}
pub fn schedule(
    deadline_ns: u64,
    period_ns: u64,
    immediate: bool,
    mut frame: impl FnMut() -> gtk::glib::ControlFlow + 'static,
) -> (u64, FrameTimer) {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let raw = unsafe {
        libc::timerfd_create(
            libc::CLOCK_MONOTONIC,
            libc::TFD_NONBLOCK | libc::TFD_CLOEXEC,
        )
    };
    assert!(
        raw >= 0,
        "canvas timerfd: {}",
        std::io::Error::last_os_error()
    );
    let timer = FrameTimer {
        timer: std::rc::Rc::new(unsafe { OwnedFd::from_raw_fd(raw) }),
        period_ns,
        expedited: std::cell::Cell::new(immediate),
    };
    let first = timer.arm(if immediate { 0 } else { deadline_ns });
    let descriptor = timer.timer.clone();
    glib_unix::unix_fd_add_local(raw, gtk::glib::IOCondition::IN, move |_, _| {
        if !timer_fired(descriptor.as_raw_fd()) {
            return gtk::glib::ControlFlow::Continue;
        }
        frame()
    });
    (first, timer)
}

fn timer_fired(descriptor: std::os::fd::RawFd) -> bool {
    let mut elapsed = 0u64;
    let bytes = unsafe { libc::read(descriptor, (&mut elapsed as *mut u64).cast(), 8) };
    if bytes == 8 {
        return true;
    }
    let error = std::io::Error::last_os_error();
    // Input may rearm a timer after GLib polled it but before this source runs.
    // Rearming clears its old expiration count. Keep the source alive to receive
    // the new expiration instead of leaving the workspace without a timer.
    if bytes < 0
        && matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        )
    {
        return false;
    }
    panic!("canvas timerfd read returned {bytes}: {error}");
}

#[cfg(test)]
mod timer_tests {
    use super::*;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    #[test]
    fn rearm_clears_old_readiness_and_next_expiration_remains_usable() {
        let raw = unsafe {
            libc::timerfd_create(
                libc::CLOCK_MONOTONIC,
                libc::TFD_NONBLOCK | libc::TFD_CLOEXEC,
            )
        };
        assert!(raw >= 0);
        let timer = FrameTimer {
            timer: std::rc::Rc::new(unsafe { OwnedFd::from_raw_fd(raw) }),
            period_ns: 1_000_000_000,
            expedited: std::cell::Cell::new(false),
        };
        let ready = || {
            let mut descriptor = libc::pollfd {
                fd: timer.timer.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(unsafe { libc::poll(&mut descriptor, 1, 1000) }, 1);
            assert_ne!(descriptor.revents & libc::POLLIN, 0);
        };
        timer.expedite();
        ready();
        // Simulate a terminal input handler changing an already-polled timer.
        timer.arm(gtk::glib::monotonic_time() as u64 * 1000 + 1_000_000_000);
        assert!(!timer_fired(raw));
        timer.expedite();
        ready();
        assert!(timer_fired(raw));
        assert!(!timer_fired(raw));
        assert!(timer.take_expedited());
        assert!(!timer.take_expedited());
    }
}
