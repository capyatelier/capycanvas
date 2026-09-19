//! Window-owned, CPU-backed surround beneath every transient GPU canvas.
//! Nine slices preserve rounded corners without a window-sized backing image.
use super::{Geometry, Parent, error};
use gtk::{gdk, prelude::*};
use std::{
    fs::File,
    io::Write,
    os::fd::{AsFd, FromRawFd},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::{Backend, ObjectId},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_subcompositor,
        wl_subsurface, wl_surface,
    },
};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};

struct Part {
    surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
    viewport: wp_viewport::WpViewport,
}
impl Drop for Part {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.subsurface.destroy();
        self.surface.destroy();
    }
}

pub(crate) struct Backdrop {
    parts: Vec<Part>,
    buffer: Option<wl_buffer::WlBuffer>,
    shm: wl_shm::WlShm,
    connection: Connection,
    events: EventQueue<Events>,
    state: Events,
    previous: Option<(Geometry, [i32; 2], [u8; 3])>,
    sample: Option<(i32, [u8; 3])>,
    // Our borrowed Wayland parent must outlive all child protocol objects.
    _parent: gdk::Surface,
}
impl Backdrop {
    pub fn new(area: &gtk::Picture) -> Result<Self, String> {
        let native = area
            .native()
            .and_then(|n| n.surface())
            .ok_or("GTK surface unavailable")?;
        let parent = Parent::new(&native)?;
        let connection = Connection::from_backend(unsafe {
            Backend::from_foreign_display(parent.display as *mut _)
        });
        let (globals, events) = registry_queue_init::<Events>(&connection).map_err(error)?;
        let qh = events.handle();
        let compositor: wl_compositor::WlCompositor =
            globals.bind(&qh, 4..=4, ()).map_err(error)?;
        let subcompositor: wl_subcompositor::WlSubcompositor =
            globals.bind(&qh, 1..=1, ()).map_err(error)?;
        let shm = globals.bind(&qh, 1..=1, ()).map_err(error)?;
        let viewporter: wp_viewporter::WpViewporter =
            globals.bind(&qh, 1..=1, ()).map_err(error)?;
        let id = unsafe {
            ObjectId::from_ptr(wl_surface::WlSurface::interface(), parent.surface as *mut _)
        }
        .map_err(error)?;
        let parent = wl_surface::WlSurface::from_id(&connection, id).map_err(error)?;
        let empty = compositor.create_region(&qh, ());
        let parts = (0..9)
            .map(|_| {
                let surface = compositor.create_surface(&qh, ());
                surface.set_input_region(Some(&empty));
                let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
                subsurface.place_below(&parent);
                // Keep synchronized: bounds, pixels and GTK's transparent chrome
                // become visible together on the next GTK-owned parent commit.
                let viewport = viewporter.get_viewport(&surface, &qh, ());
                Part {
                    surface,
                    subsurface,
                    viewport,
                }
            })
            .collect();
        empty.destroy();
        viewporter.destroy();
        subcompositor.destroy();
        Ok(Self {
            parts,
            buffer: None,
            shm,
            connection,
            events,
            state: Events,
            previous: None,
            sample: None,
            _parent: native,
        })
    }

    pub fn update(&mut self, area: &gtk::Picture, color: [u8; 3]) -> Result<(), String> {
        self.events
            .dispatch_pending(&mut self.state)
            .map_err(error)?;
        let geometry = Geometry::of(area);
        let size = [area.width().max(1), area.height().max(1)];
        let state = (geometry, size, color);
        if self.previous == Some(state) {
            return Ok(());
        }
        let radius = if geometry.rounded {
            12.min(size[0] / 2).min(size[1] / 2)
        } else {
            0
        };
        // The sample stays bounded even on an unusually high-scale output.
        let scale = geometry.scale.clamp(1, 8);
        let r = radius * scale;
        let side = 2 * r + 1;
        if self.sample != Some((r, color)) {
            let pixels = rounded_sample(r, color);
            let raw = unsafe {
                libc::memfd_create(c"capy-window-background".as_ptr(), libc::MFD_CLOEXEC)
            };
            if raw < 0 {
                return Err(error(std::io::Error::last_os_error()));
            }
            let mut file = unsafe { File::from_raw_fd(raw) };
            file.write_all(&pixels).map_err(error)?;
            let qh = self.events.handle();
            let pool = self
                .shm
                .create_pool(file.as_fd(), pixels.len() as i32, &qh, ());
            let buffer =
                pool.create_buffer(0, side, side, side * 4, wl_shm::Format::Argb8888, &qh, ());
            pool.destroy();
            // Immutable buffers can be destroyed client-side after replacement;
            // the compositor retains its reference until all old surfaces release it.
            if let Some(old) = self.buffer.replace(buffer) {
                old.destroy();
            }
            self.sample = Some((r, color));
        }
        let buffer = self.buffer.as_ref().unwrap();
        let xs = slices(size[0], radius);
        let ys = slices(size[1], radius);
        let source = [(0, r), (r, 1), (r + 1, r)];
        for (index, part) in self.parts.iter().enumerate() {
            let (x, y) = (index % 3, index / 3);
            let (left, width) = xs[x];
            let (top, height) = ys[y];
            if width == 0 || height == 0 {
                part.surface.attach(None, 0, 0);
            } else {
                part.subsurface
                    .set_position(geometry.position[0] + left, geometry.position[1] + top);
                part.viewport.set_source(
                    source[x].0 as f64,
                    source[y].0 as f64,
                    source[x].1 as f64,
                    source[y].1 as f64,
                );
                part.viewport.set_destination(width, height);
                part.surface.attach(Some(buffer), 0, 0);
                part.surface.damage_buffer(0, 0, side, side);
            }
            part.surface.commit();
        }
        self.connection.flush().map_err(error)?;
        self.previous = Some(state);
        super::request_parent_commit(area);
        Ok(())
    }
}
impl Drop for Backdrop {
    fn drop(&mut self) {
        self.parts.clear();
        if let Some(buffer) = self.buffer.take() {
            buffer.destroy();
        }
        let _ = self.connection.flush();
    }
}

fn slices(length: i32, radius: i32) -> [(i32, i32); 3] {
    [
        (0, radius),
        (radius, length - 2 * radius),
        (length - radius, radius),
    ]
}
fn rounded_sample(radius: i32, color: [u8; 3]) -> Vec<u8> {
    let side = 2 * radius + 1;
    let mut pixels = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let dx = (radius as f32 - (x as f32 + 0.5).min((side - x) as f32 - 0.5)).max(0.);
            let dy = (radius as f32 - (y as f32 + 0.5).min((side - y) as f32 - 0.5)).max(0.);
            let alpha = if radius == 0 {
                255
            } else {
                ((radius as f32 + 0.5 - dx.hypot(dy)).clamp(0., 1.) * 255.).round() as u32
            };
            let [r, g, b] = color.map(|c| (c as u32 * alpha + 127) / 255);
            pixels.extend_from_slice(&((alpha << 24) | (r << 16) | (g << 8) | b).to_ne_bytes());
        }
    }
    pixels
}

struct Events;
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
wayland_client::delegate_noop!(Events: ignore wl_surface::WlSurface);
wayland_client::delegate_noop!(Events: ignore wl_subsurface::WlSubsurface);
wayland_client::delegate_noop!(Events: ignore wl_region::WlRegion);
wayland_client::delegate_noop!(Events: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(Events: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(Events: ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(Events: ignore wp_viewporter::WpViewporter);
wayland_client::delegate_noop!(Events: ignore wp_viewport::WpViewport);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_is_opaque_except_for_rounded_outer_corners() {
        for radius in [0, 1, 12, 24, 96] {
            let bytes = rounded_sample(radius, [51, 90, 120]);
            let side = (2 * radius + 1) as usize;
            assert_eq!(bytes.len(), side * side * 4);
            let pixel =
                |x, y| u32::from_ne_bytes(bytes[(y * side + x) * 4..][..4].try_into().unwrap());
            for y in 0..side {
                for x in 0..side {
                    let p = pixel(x, y);
                    assert_eq!(p, pixel(side - 1 - x, side - 1 - y));
                    if x == radius as usize || y == radius as usize {
                        assert_eq!(p, 0xff335a78, "stretched edges/center must stay opaque");
                    }
                    let alpha = p >> 24;
                    assert!(
                        [p & 255, (p >> 8) & 255, (p >> 16) & 255]
                            .iter()
                            .all(|c| *c <= alpha)
                    );
                }
            }
            if radius >= 12 {
                assert_eq!(pixel(0, 0), 0);
            }
            assert!(bytes.len() <= 150_000);
        }
    }

    #[test]
    fn background_slices_cover_small_and_large_windows_without_gaps() {
        for length in [1, 2, 23, 24, 25, 800, 1600, 7680] {
            for radius in [0, 12.min(length / 2)] {
                let parts = slices(length, radius);
                let mut end = 0;
                for (start, size) in parts {
                    assert_eq!(start, end);
                    assert!(size >= 0);
                    end += size;
                }
                assert_eq!(end, length);
            }
        }
    }
}
