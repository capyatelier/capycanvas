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
    // Alive until the worker and its borrowed Wayland handles are gone.
    _parent: gtk::gdk::Surface,
    pub needs_present: bool,
}
impl GpuCanvas {
    pub fn update_cursor(&mut self) -> bool {
        self.session.update_canvas_cursor(&mut self.cursor, false);
        self.session.append_layer_overlay(&mut self.cursor.segments);
        let renderer = self.session.renderer_mut();
        if renderer.cursor == self.cursor.segments {
            return false;
        }
        std::mem::swap(&mut renderer.cursor, &mut self.cursor.segments);
        self.needs_present = true;
        true
    }
    pub fn new(area: &gtk::Picture) -> Result<Self, String> {
        let parent = area
            .native()
            .and_then(|native| native.surface())
            .ok_or("GTK surface unavailable")?;
        let renderer = RenderWorker::new(Parent::new(&parent)?, area.downgrade().into())?;
        let mut session = UiSession::blank(renderer, extent(area))?;
        session.set_platform(layer_ui::Platform::Gtk);
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
                .and_then(|mode| load_filter_directory(&mut session, &directory, mode));
            if let Err(error) = result {
                eprintln!("{error}; using bundled filters");
            }
        }
        // Read once while creating this window, before it can accept input.
        match crate::preferences::load() {
            Ok(Some(settings)) => {
                session.dispatch(layer_ui::UiAction::RestoreSettings { settings })?;
            }
            Ok(None) => {}
            Err(error) => eprintln!("{error}; using default preferences"),
        }
        session.dispatch(layer_ui::UiAction::SystemThemeChanged {
            theme: if adw::StyleManager::default().is_dark() {
                layer_ui::Theme::Dark
            } else {
                layer_ui::Theme::Light
            },
        })?;
        Ok(Self {
            session,
            cursor: CanvasCursor::default(),
            _parent: parent,
            needs_present: true,
        })
    }
    pub fn render(&mut self, area: &gtk::Picture, now_ns: u64) -> Result<UiChange, String> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let renderer = self.session.renderer_mut();
        if !renderer.ready()? {
            // Preserve queued pen samples; never stall GTK or drop paint.
            self.needs_present = true;
            return Ok(UiChange::default());
        }
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
        let mut changed = self
            .session
            .frame(now_ns, now_ns.saturating_add(8_333_333))?;
        changed.regions |= resized.regions;
        self.needs_present = false;
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
) -> Result<UiChange, String> {
    let manifest =
        std::fs::read_to_string(directory.join("manifest.json")).map_err(|e| e.to_string())?;
    session.load_effect_package(
        &manifest,
        |name| {
            std::fs::read_to_string(directory.join(name))
                .map(std::sync::Arc::from)
                .map_err(|e| e.to_string())
        },
        mode,
    )
}
fn extent(area: &gtk::Picture) -> [u32; 2] {
    let scale = area.scale_factor() as u32;
    [
        area.width().max(1) as u32 * scale,
        area.height().max(1) as u32 * scale,
    ]
}

/// Independent 120 Hz pacing, not GTK's scene frame clock (which can fall back
/// to 60 Hz when only our child changes). timerfd uses absolute kernel intervals;
/// missed expirations coalesce, preserving input without replaying stale frames.
pub const FRAME_NS: u64 = 8_333_333;
pub fn schedule(
    deadline_ns: u64,
    mut frame: impl FnMut() -> gtk::glib::ControlFlow + 'static,
) -> u64 {
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
    let timer = unsafe { OwnedFd::from_raw_fd(raw) };
    let first = deadline_ns.max(gtk::glib::monotonic_time().max(0) as u64 * 1000 + 1);
    let interval = libc::itimerspec {
        it_interval: libc::timespec {
            tv_sec: 0,
            tv_nsec: FRAME_NS as _,
        },
        it_value: libc::timespec {
            tv_sec: (first / 1_000_000_000) as _,
            tv_nsec: (first % 1_000_000_000) as _,
        },
    };
    assert_eq!(
        unsafe {
            libc::timerfd_settime(
                raw,
                libc::TFD_TIMER_ABSTIME,
                &interval,
                std::ptr::null_mut(),
            )
        },
        0
    );
    glib_unix::unix_fd_add_local(raw, gtk::glib::IOCondition::IN, move |_, _| {
        let mut elapsed = 0u64;
        let bytes = unsafe { libc::read(timer.as_raw_fd(), (&mut elapsed as *mut u64).cast(), 8) };
        if bytes != 8 {
            return gtk::glib::ControlFlow::Break;
        }
        frame()
    });
    first
}
