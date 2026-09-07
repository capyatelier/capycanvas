//! Our child surface only. GTK retains its connection, parent, input and chrome.
use gtk::{gdk, glib::translate::*, prelude::*};
use std::ptr::NonNull;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::{Backend, ObjectId},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_compositor, wl_region, wl_registry, wl_subcompositor, wl_subsurface, wl_surface,
    },
};
#[cfg(test)]
use wayland_protocols::wp::presentation_time::client::{wp_presentation, wp_presentation_feedback};

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
    #[cfg(test)]
    presentation: Option<wp_presentation::WpPresentation>,
}

#[derive(Default)]
struct Events {
    #[cfg(test)]
    presented: Vec<[u64; 4]>,
}

impl Child {
    pub fn new(parent: Parent) -> Result<Self, String> {
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
        #[cfg(test)]
        let presentation = globals.bind(&qh, 1..=1, ()).ok();
        Ok(Self {
            surface,
            subsurface,
            connection,
            events,
            state: Events::default(),
            geometry: None,
            #[cfg(test)]
            presentation,
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
        self.connection.flush().map_err(error)
    }

    #[cfg(test)]
    pub fn feedback(&self, id: u64) {
        if let Some(presentation) = &self.presentation {
            presentation.feedback(&self.surface, &self.events.handle(), id);
        }
    }

    #[cfg(test)]
    pub fn take_presented(&mut self) -> Vec<[u64; 4]> {
        std::mem::take(&mut self.state.presented)
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // The caller drops the Vulkan surface/swapchain BEFORE this object.
        self.subsurface.destroy();
        self.surface.destroy();
        #[cfg(test)]
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
#[cfg(test)]
wayland_client::delegate_noop!(Events: ignore wp_presentation::WpPresentation);
#[cfg(test)]
impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, u64> for Events {
    fn event(
        state: &mut Self,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        id: &u64,
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
            state.presented.push([
                *id,
                ((u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo)) * 1_000_000_000
                    + u64::from(tv_nsec),
                u64::from(refresh),
                1,
            ]);
        } else if matches!(event, wp_presentation_feedback::Event::Discarded) {
            state.presented.push([*id, 0, 0, 0]);
        }
    }
}
