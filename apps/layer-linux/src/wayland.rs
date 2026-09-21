//! Our child surface only. GTK retains its connection, parent, input and chrome.
mod color;
mod hdr;
mod pacing;
pub(crate) use pacing::StrokeTarget;
pub(crate) mod backdrop;
use gtk::{gdk, glib::translate::*, prelude::*};
use std::{
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::{Backend, ObjectId},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_compositor, wl_region, wl_registry, wl_subcompositor, wl_subsurface, wl_surface,
    },
};
use wayland_protocols::wp::presentation_time::client::{wp_presentation, wp_presentation_feedback};

/// The compositor's display phase, not the frequency of GTK scene updates.
/// Presentation feedback corrects startup/monitor phase shifts without input-thread locks.
#[derive(Default)]
pub struct FrameClock {
    phase_ns: AtomicU64,
    period_ns: AtomicU64,
    stroke_lead_ns: AtomicU64,
    stroke_feedback_ns: AtomicU64,
    draw_work_ns: AtomicU64,
}
impl FrameClock {
    pub fn stroke_work_fits(&self) -> bool {
        self.draw_work_ns.load(Ordering::Relaxed) <= self.period() / 8
    }
    fn observe_draw_work(&self, elapsed_ns: u64) {
        // React immediately to expensive/acquisition-blocked frames, and let
        // the recent peak decay over several refreshes after work becomes cheap.
        let previous = self.draw_work_ns.load(Ordering::Relaxed);
        self.draw_work_ns.store(elapsed_ns.max(previous.saturating_sub(previous / 16 + 1)), Ordering::Relaxed);
    }
    fn lead(period: u64) -> u64 {
        // Test-only override for the conservative rendering margin.
        // Eligible strokes can learn a shorter margin from presentation feedback.
        #[cfg(test)]
        {
            static OVERRIDE: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
            if let Some(lead) = *OVERRIDE.get_or_init(||
                std::env::var("LAYER_PACING_LEAD_NS").ok().map(|v| v.parse().unwrap()))
            {
                assert!(lead > 0 && lead < period);
                return lead;
            }
        }
        period * 3 / 4
    }
    fn observe(&self, time: u64, period: u64) {
        let previous = self.phase_ns.load(Ordering::Relaxed);
        let old_period = self.period_ns.load(Ordering::Relaxed);
        if old_period != period {
            self.stroke_lead_ns.store(0, Ordering::Relaxed);
        }
        let drift = if period > 0 {
            let delta = (time % period).abs_diff(previous % period);
            delta.min(period - delta)
        } else { 0 };
        // Ignore small compositor timestamp jitter, but correct a real phase
        // change immediately instead of submitting near a stale cutoff for 1 s.
        if old_period != period || time.saturating_sub(previous) >= 1_000_000_000
            || drift > period / 16
        {
            self.period_ns.store(period, Ordering::Relaxed);
            self.phase_ns.store(if period > 0 { time } else { 0 }, Ordering::Release);
        }
    }
    pub fn period(&self) -> u64 {
        match self.period_ns.load(Ordering::Relaxed) {
            n @ 1_000_000..=1_000_000_000 => n,
            _ => crate::canvas::FRAME_NS,
        }
    }
    pub fn deadline(&self, now: u64) -> Option<u64> {
        self.deadline_for(now, false)
    }
    pub fn stroke_lead(&self, period: u64) -> u64 {
        self.stroke_lead_at(period, gtk::glib::monotonic_time().max(0) as u64 * 1000)
    }
    fn stroke_lead_at(&self, period: u64, now: u64) -> u64 {
        if now.saturating_sub(self.stroke_feedback_ns.load(Ordering::Acquire)) > 2_000_000_000 {
            return Self::lead(period);
        }
        match self.stroke_lead_ns.load(Ordering::Relaxed) {
            0 => Self::lead(period),
            lead => lead.clamp(period / 4, period * 3 / 4),
        }
    }
    pub fn deadline_for(&self, now: u64, stroke: bool) -> Option<u64> {
        let phase = self.phase_ns.load(Ordering::Acquire);
        if phase == 0 {
            return None;
        }
        let period = self.period();
        // Leave three quarters of a refresh for canvas work plus GTK's overlay and
        // compositor. Half was too short for live transforms with wet paint
        // and numeric controls updating, despite each renderer fitting 120Hz.
        let lead = if stroke { self.stroke_lead(period) } else { Self::lead(period) };
        let base = phase.saturating_sub(lead);
        Some(if base > now {
            base
        } else {
            base + ((now - base) / period + 1) * period
        })
    }
    /// Start a camera burst after a small input-coalescing window, away from
    /// the compositor cutoff. Retain this phase until the burst goes idle.
    pub fn navigation_start(&self, input: u64, now: u64) -> u64 {
        let period = self.period();
        let earliest = input + period / 4;
        // Corrected feedback may be newer than the last input. Predict from a
        // future deadline while retaining the input's phase through whole periods.
        let earliest = if earliest <= now {
            earliest + ((now - earliest) / period + 1) * period
        } else { earliest };
        let present = self.presentation(earliest);
        if present - earliest < period * 3 / 8 {
            present + period / 8
        } else {
            earliest
        }
    }
    pub fn navigation_aligned(&self, deadline: u64, interval: u64) -> bool {
        interval == self.period()
            && self.presentation(deadline) - deadline >= interval * 3 / 8
    }
    /// A running timer must also follow newly available/corrected presentation
    /// phase, not just refresh-rate changes. Ignore small feedback
    /// jitter rather than continuously rearming an otherwise aligned timer.
    pub fn aligned(&self, deadline: u64, interval: u64) -> bool {
        self.aligned_for(deadline, interval, false)
    }
    pub fn aligned_for(&self, deadline: u64, interval: u64, stroke: bool) -> bool {
        let phase = self.phase_ns.load(Ordering::Acquire);
        let period = self.period();
        if interval != period {
            return false;
        }
        if phase == 0 {
            return true;
        }
        let lead = if stroke { self.stroke_lead(period) } else { Self::lead(period) };
        let drift = ((deadline + lead) % period).abs_diff(phase % period);
        drift.min(period - drift) <= period / 16
    }
    pub fn presentation(&self, now: u64) -> u64 {
        let phase = self.phase_ns.load(Ordering::Acquire);
        let period = self.period();
        if phase == 0 {
            now + period
        } else if phase > now {
            phase
        } else {
            phase + ((now - phase) / period + 1) * period
        }
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    #[test]
    fn first_stroke_after_idle_starts_with_the_conservative_margin() {
        let clock = FrameClock::default();
        clock.stroke_lead_ns.store(4_500_000, Ordering::Relaxed);
        clock.stroke_feedback_ns.store(1_000_000_000, Ordering::Release);
        assert_eq!(clock.stroke_lead_at(8_000_000, 1_500_000_000), 4_500_000);
        assert_eq!(clock.stroke_lead_at(8_000_000, 3_100_000_000), 6_000_000);
    }
    #[test]
    fn expensive_frames_keep_the_original_margin_until_work_recovers() {
        let clock = FrameClock::default();
        clock.observe(100_000_000, 8_000_000);
        assert!(clock.stroke_work_fits());
        clock.observe_draw_work(2_000_000);
        assert!(!clock.stroke_work_fits());
        for _ in 0..4 { clock.observe_draw_work(200_000); }
        assert!(!clock.stroke_work_fits());
        for _ in 0..32 { clock.observe_draw_work(200_000); }
        assert!(clock.stroke_work_fits());
    }
    #[test]
    fn presentation_phase_shift_realigns_navigation_but_small_jitter_does_not() {
        let clock = FrameClock::default();
        let period = 8_000_000;
        let present = 100_000_000;
        clock.observe(present, period);
        let deadline = present - 4_000_000;
        assert!(clock.navigation_aligned(deadline, period));
        clock.observe(present + period + 100_000, period);
        assert_eq!(clock.phase_ns.load(Ordering::Relaxed), present);
        clock.observe(present + 2 * period - 2_000_000, period);
        assert!(!clock.navigation_aligned(deadline + 2 * period, period));
        let corrected = clock.navigation_start(present + 2 * period, present + 2 * period);
        assert!(clock.navigation_aligned(corrected, period));
        clock.observe(present + 3 * period, period * 2);
        assert!(!clock.navigation_aligned(corrected, period));
    }
    #[test]
    fn navigation_burst_coalesces_input_and_avoids_the_compositor_cutoff() {
        let clock = FrameClock::default();
        let period = 8_000_000;
        clock.period_ns.store(period, Ordering::Relaxed);
        clock.phase_ns.store(100_000_000, Ordering::Release);
        for now in (100_000_000..108_000_000).step_by(10_000) {
            let start = clock.navigation_start(now, now);
            assert!(start >= now + period / 4);
            assert!(start < now + period);
            assert!(clock.presentation(start) - start >= period * 3 / 8);
            let resumed = clock.navigation_start(now, now + period * 5);
            assert_eq!(resumed, start + period * 5);
            assert!(resumed > now + period * 5);
            clock.phase_ns.store(100_000_000 + period * 5, Ordering::Release);
            let resumed = clock.navigation_start(now, now + period * 5);
            assert_eq!(resumed, start + period * 5);
            assert!(clock.navigation_aligned(resumed, period));
            clock.phase_ns.store(100_000_000, Ordering::Release);
        }
    }
    #[test]
    fn display_phase_survives_idle_and_period_changes() {
        let clock = FrameClock::default();
        assert_eq!(clock.deadline(0), None);
        assert_eq!(clock.period(), crate::canvas::FRAME_NS);
        assert!(clock.aligned(123, crate::canvas::FRAME_NS));
        for period in [8_333_333, 16_666_667, 6_944_444] {
            let phase = 10_000_000_000;
            clock.period_ns.store(period, Ordering::Relaxed);
            clock.phase_ns.store(phase, Ordering::Release);
            for now in [phase, phase + period / 2, phase + period * 1234 + 1] {
                let next = clock.deadline(now).unwrap();
                assert!(next > now && next - now <= period);
                assert_eq!((next + period * 3 / 4 - phase) % period, 0);
                assert!(clock.aligned(next, period));
                assert!(clock.aligned(next + 123 * period, period));
                assert!(!clock.aligned(next, period * 2));
                assert!(clock.aligned(next + period / 32, period));
                assert!(clock.aligned(next - period / 32, period));
                assert!(!clock.aligned(next + period / 4, period));
                assert!(!clock.aligned(next - period / 4, period));
                // Feedback can establish a different phase while input keeps
                // the timer alive; it need not change the refresh rate.
                clock.phase_ns.store(phase + period / 4, Ordering::Release);
                assert!(!clock.aligned(next, period));
                assert!(clock.aligned(clock.deadline(now).unwrap(), period));
                clock.phase_ns.store(phase, Ordering::Release);
            }
        }
    }
}

unsafe extern "C" {
    fn gdk_wayland_display_get_wl_display(display: *mut gdk::ffi::GdkDisplay) -> *mut libc::c_void;
    fn gdk_wayland_surface_get_wl_surface(surface: *mut gdk::ffi::GdkSurface) -> *mut libc::c_void;
    fn gdk_wayland_surface_force_next_commit(surface: *mut gdk::ffi::GdkSurface);
}

/// Borrowed addresses, sent only while GpuCanvas keeps the GDK parent alive.
/// Its Drop joins the worker before GTK's unrealize destroys that parent.
#[derive(Clone, Copy)]
pub struct Parent {
    display: usize,
    surface: usize,
}

impl Parent {
    pub fn new(surface: &gdk::Surface) -> Result<Self, String> {
        if surface.type_().name() != "GdkWaylandToplevel" {
            return Err("The native GPU canvas requires a Wayland window".into());
        }
        let (display, surface) = unsafe {
            (
                gdk_wayland_display_get_wl_display(surface.display().to_glib_none().0) as usize,
                gdk_wayland_surface_get_wl_surface(surface.to_glib_none().0) as usize,
            )
        };
        if display == 0 || surface == 0 {
            return Err("GTK Wayland handles unavailable".into());
        }
        Ok(Self { display, surface })
    }
}

pub fn request_parent_commit(area: &gtk::Picture) {
    if let Some(surface) = area.native().and_then(|native| native.surface()) {
        // Requests a commit, never commits or modifies GTK's surface ourselves.
        unsafe { gdk_wayland_surface_force_next_commit(surface.to_glib_none().0) };
        surface.queue_render();
        area.queue_draw();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub position: [i32; 2],
    pub scale: i32,
    pub rounded: bool,
}

impl Geometry {
    pub fn of(area: &gtk::Picture) -> Self {
        let native = area.native().expect("realized canvas");
        let (dx, dy) = native.surface_transform();
        let root = native.dynamic_cast::<gtk::Widget>().expect("native widget");
        let bounds = area.compute_bounds(&root).expect("canvas in native window");
        let rounded = !root
            .downcast_ref::<gtk::Window>()
            .is_some_and(|w| w.is_maximized() || w.is_fullscreen());
        Self {
            position: [
                (bounds.x() as f64 + dx).round() as i32,
                (bounds.y() as f64 + dy).round() as i32,
            ],
            scale: area.scale_factor(),
            rounded,
        }
    }
}

pub struct Child {
    pub surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
    connection: Connection,
    events: EventQueue<Events>,
    state: Events,
    geometry: Option<Geometry>,
    presentation: Option<wp_presentation::WpPresentation>,
    color: Option<color::ColorSurface>,
}

#[derive(Default)]
struct Events {
    color: color::State,
    hdr: hdr::HdrState,
    clock: Arc<FrameClock>,
    monotonic: bool,
    feedback_pending: usize,
    stroke_pacer: pacing::StrokePacer,
    #[cfg(test)]
    presented: Vec<[u64; 4]>,
}

impl Child {
    pub fn new(parent: Parent, clock: Arc<FrameClock>) -> Result<Self, String> {
        // The system backend creates its OWN queue on the borrowed display.
        // Neither this backend nor Vulkan dispatches GTK's event queue.
        let backend = unsafe { Backend::from_foreign_display(parent.display as *mut _) };
        let connection = Connection::from_backend(backend);
        let (globals, events) = registry_queue_init::<Events>(&connection).map_err(error)?;
        let qh = events.handle();
        let compositor: wl_compositor::WlCompositor =
            globals.bind(&qh, 4..=4, ()).map_err(error)?;
        let subcompositor: wl_subcompositor::WlSubcompositor =
            globals.bind(&qh, 1..=1, ()).map_err(error)?;
        let parent = unsafe {
            ObjectId::from_ptr(wl_surface::WlSurface::interface(), parent.surface as *mut _)
        }
        .map_err(error)?;
        let parent = wl_surface::WlSurface::from_id(&connection, parent).map_err(error)?;
        let surface = compositor.create_surface(&qh, ());
        let region = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
        subsurface.place_below(&parent);
        subsurface.set_desync();
        subcompositor.destroy();
        let presentation = globals.bind(&qh, 1..=1, ()).ok();
        let color = color::ColorSurface::bind(&globals, &qh);
        Ok(Self {
            surface,
            subsurface,
            connection,
            events,
            state: Events {
                clock,
                ..Default::default()
            },
            geometry: None,
            presentation,
            color,
        })
    }

    pub fn target(&self) -> wgpu::SurfaceTargetUnsafe {
        use raw_window_handle::*;
        wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
                NonNull::new(self.connection.backend().display_ptr().cast()).unwrap(),
            ))),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(
                NonNull::new(self.surface.id().as_ptr().cast()).unwrap(),
            )),
        }
    }

    pub fn geometry(&mut self, geometry: Geometry) -> bool {
        if self.geometry != Some(geometry) {
            self.subsurface
                .set_position(geometry.position[0], geometry.position[1]);
            self.surface.set_buffer_scale(geometry.scale);
            self.geometry = Some(geometry);
            return true;
        }
        false
    }

    pub fn dispatch(&mut self) -> Result<(), String> {
        // GTK/Vulkan read the shared socket. Dispatch only our pending events.
        self.events
            .dispatch_pending(&mut self.state)
            .map_err(error)?;
        self.poll_hdr_feedback();
        self.connection.flush().map_err(error)
    }

    pub fn feedback_pending(&self) -> bool {
        self.state.feedback_pending != 0
    }
    pub fn observe_draw_work(&self, elapsed_ns: u64) {
        self.state.clock.observe_draw_work(elapsed_ns);
    }

    pub fn feedback(&mut self, id: u64, stroke: Option<StrokeTarget>) {
        if let Some(presentation) = &self.presentation {
            self.state.feedback_pending += 1;
            presentation.feedback(&self.surface, &self.events.handle(), Presentation { id, stroke });
        }
    }

    #[cfg(test)]
    pub fn take_presented(&mut self) -> Vec<[u64; 4]> {
        std::mem::take(&mut self.state.presented)
    }
}

struct Presentation {
    #[cfg_attr(not(test), allow(dead_code))]
    id: u64,
    stroke: Option<StrokeTarget>,
}

impl Drop for Child {
    fn drop(&mut self) {
        // The caller drops the Vulkan surface/swapchain BEFORE this object.
        self.color.take();
        self.subsurface.destroy();
        self.surface.destroy();
        if let Some(presentation) = &self.presentation {
            presentation.destroy();
        }
        let _ = self.connection.flush();
    }
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Events {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
wayland_client::delegate_noop!(Events: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(Events: ignore wl_subcompositor::WlSubcompositor);
wayland_client::delegate_noop!(Events: ignore wl_subsurface::WlSubsurface);
wayland_client::delegate_noop!(Events: ignore wl_surface::WlSurface);
wayland_client::delegate_noop!(Events: ignore wl_region::WlRegion);
impl Dispatch<wp_presentation::WpPresentation, ()> for Events {
    fn event(
        state: &mut Self,
        _: &wp_presentation::WpPresentation,
        event: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation::Event::ClockId { clk_id } = event {
            state.monotonic = clk_id == libc::CLOCK_MONOTONIC as u32;
        }
    }
}
impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, Presentation> for Events {
    fn event(
        state: &mut Self,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        data: &Presentation,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation_feedback::Event::Presented {
            tv_sec_hi,
            tv_sec_lo,
            tv_nsec,
            refresh,
            ..
        } = event
        {
            state.feedback_pending -= 1;
            let time = ((u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo)) * 1_000_000_000
                + u64::from(tv_nsec);
            if state.monotonic {
                state.clock.observe(time, u64::from(refresh));
                if let Some(sample) = data.stroke
                    && let Some(lead) = state.stroke_pacer.observe(sample, time, u64::from(refresh))
                {
                    state.clock.stroke_lead_ns.store(lead, Ordering::Relaxed);
                    state.clock.stroke_feedback_ns.store(time, Ordering::Release);
                }
            }
            #[cfg(test)]
            state.presented.push([data.id, time, u64::from(refresh), 1]);
        } else if matches!(event, wp_presentation_feedback::Event::Discarded) {
            state.feedback_pending -= 1;
            #[cfg(test)]
            state.presented.push([data.id, 0, 0, 0]);
        }
    }
}
