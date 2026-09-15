//! Full-stack color comparison, reduced only after native linear composition.
//! All GPU/readback work stays on the worker; this image never feeds artwork.
use super::*;
use gtk::gdk;
use layer_core::color::{
    ColorProfile, IntegerDepth, RgbSpace,
    source::{SourceChannels, SourceInterpretation},
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotRenderer};
use std::cell::{Cell, RefCell};

struct Image {
    extent: [u32; 2],
    bytes: Vec<u8>,
}
fn thumbnail(
    project: Project,
    background: [f32; 4],
    time: f32,
    control: CaptureControl,
) -> Result<Image, String> {
    let source = [project.document.width, project.document.height];
    let scale = (220. / source[0] as f64)
        .min(160. / source[1] as f64)
        .min(1.);
    let extent = source.map(|v| (v as f64 * scale).round().max(1.) as u32);
    let space = project.document.color.space;
    let mut renderer = SnapshotRenderer::with_control(
        project,
        background,
        time,
        Default::default(),
        control.clone(),
    )
    .map_err(|e| e.to_string())?;
    let mut sums = vec![[0f64; 4]; (extent[0] * extent[1]) as usize];
    // Area weights are integral in this common grid, avoiding fractional edge
    // drift. Include every source pixel; no selected-pixel shortcut can miss
    // a thin stroke, clipping, mask or adjustment.
    for first in (0..source[1]).step_by(16) {
        if control.is_cancelled() {
            return Err("Preview cancelled".into());
        }
        let height = 16.min(source[1] - first);
        let pixels = renderer
            .read_region([0, first, source[0], height])
            .map_err(|e| e.to_string())?;
        accumulate(source, extent, first, &pixels, &mut sums);
    }
    drop(renderer);
    let area = f64::from(source[0]) * f64::from(source[1]);
    let linear: Vec<_> = sums
        .into_iter()
        .map(|p| p.map(|v| (v / area) as f32))
        .collect();
    let target = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: IntegerDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    };
    let encoder = layer_color::WorkingEncoder::new(space, &target, Default::default())?;
    let mut bytes = vec![0; linear.len() * 4];
    // Explicit sRGB preview contract, matching GTK's current canvas fallback.
    // Monitor-managed wide previews will replace this display-only encoding.
    encoder.encode_premultiplied(&linear, &mut bytes, None, [0, 0])?;
    Ok(Image { extent, bytes })
}

fn accumulate(
    source: [u32; 2],
    extent: [u32; 2],
    first: u32,
    pixels: &[[f32; 4]],
    sums: &mut [[f64; 4]],
) {
    for (i, pixel) in pixels.iter().enumerate() {
        let x = i as u32 % source[0];
        let y = first + i as u32 / source[0];
        let x0 = x * extent[0];
        let x1 = (x + 1) * extent[0];
        let y0 = y * extent[1];
        let y1 = (y + 1) * extent[1];
        for dy in y0 / source[1]..=(y1 - 1) / source[1] {
            let wy = y1.min((dy + 1) * source[1]) - y0.max(dy * source[1]);
            for dx in x0 / source[0]..=(x1 - 1) / source[0] {
                let wx = x1.min((dx + 1) * source[0]) - x0.max(dx * source[0]);
                let weight = f64::from(wx) * f64::from(wy);
                let target = &mut sums[(dy * extent[0] + dx) as usize];
                for c in 0..4 {
                    target[c] += f64::from(pixel[c]) * weight;
                }
            }
        }
    }
}

struct Pending {
    project: Project,
    background: [f32; 4],
    time: f32,
    serial: u64,
}
/// One worker plus one replaceable request. Cancellation is acknowledged before
/// starting its successor, so rapid profile changes cannot accumulate GPU jobs.
pub(super) struct Comparison {
    pub widget: gtk::Box,
    before: gtk::Picture,
    after: gtk::Picture,
    pub status: gtk::Label,
    original: RefCell<Option<Project>>,
    pending: RefCell<Option<Pending>>,
    control: RefCell<Option<CaptureControl>>,
    running: Cell<bool>,
    closed: Cell<bool>,
    serial: Cell<u64>,
    pub ready: Cell<bool>,
    pub changed: RefCell<Option<Box<dyn Fn(bool)>>>,
}
impl Comparison {
    pub fn new(original: Project) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let pictures = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        pictures.set_homogeneous(true);
        let make = |label: &str, name: &str| {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let picture = gtk::Picture::new();
            picture.set_widget_name(name);
            picture.set_can_shrink(true);
            picture.set_size_request(160, 100);
            column.append(&gtk::Label::new(Some(label)));
            column.append(&picture);
            pictures.append(&column);
            picture
        };
        let before = make("Before", "color-preview-before");
        let after = make("After", "color-preview-after");
        let status = gtk::Label::builder()
            .label("Choose a profile to preview the complete canvas.")
            .wrap(true)
            .xalign(0.)
            .build();
        status.set_widget_name("color-preview-status");
        widget.append(&pictures);
        widget.append(&status);
        Rc::new(Self {
            widget,
            before,
            after,
            status,
            original: RefCell::new(Some(original)),
            pending: RefCell::new(None),
            control: RefCell::new(None),
            running: Cell::new(false),
            closed: Cell::new(false),
            serial: Cell::new(0),
            ready: Cell::new(false),
            changed: RefCell::new(None),
        })
    }
    fn mark_ready(&self, ready: bool) {
        self.ready.set(ready);
        if let Some(changed) = self.changed.borrow().as_ref() {
            changed(ready);
        }
    }
    pub fn invalidate(&self, message: &str) {
        self.serial.set(self.serial.get() + 1);
        self.pending.borrow_mut().take();
        if let Some(control) = self.control.borrow().as_ref() {
            control.cancel();
        }
        self.after.set_paintable(None::<&gdk::Paintable>);
        self.status.set_label(message);
        self.mark_ready(false);
    }
    pub fn close(&self) {
        self.closed.set(true);
        self.invalidate("");
    }
    pub async fn finish(&self) {
        while self.running.get() {
            glib::timeout_future(std::time::Duration::from_millis(10)).await;
        }
    }
    pub fn request(self: &Rc<Self>, project: Project, background: [f32; 4], time: f32) {
        self.invalidate("Rendering the complete canvas…");
        *self.pending.borrow_mut() = Some(Pending {
            project,
            background,
            time,
            serial: self.serial.get(),
        });
        if self.running.replace(true) {
            return;
        }
        let this = self.clone();
        glib::MainContext::default().spawn_local(async move {
            loop {
                let Some(next) = this.pending.borrow_mut().take() else {
                    break;
                };
                if this.closed.get() {
                    break;
                }
                let control = CaptureControl::default();
                *this.control.borrow_mut() = Some(control.clone());
                let original = this.original.borrow().clone();
                let serial = next.serial;
                let result = gio::spawn_blocking(move || {
                    let before = original
                        .map(|project| {
                            thumbnail(project, next.background, next.time, control.clone())
                        })
                        .transpose()?;
                    let after = thumbnail(next.project, next.background, next.time, control)?;
                    Ok::<_, String>((before, after))
                })
                .await
                .map_err(|_| "Preview worker failed".to_string())
                .and_then(|r| r);
                this.control.borrow_mut().take();
                if this.closed.get() || this.serial.get() != serial {
                    continue;
                }
                match result {
                    Ok((before, after)) => {
                        let set = |picture: &gtk::Picture, image: Image| {
                            picture.set_paintable(Some(&gdk::MemoryTexture::new(
                                image.extent[0] as i32,
                                image.extent[1] as i32,
                                gdk::MemoryFormat::R8g8b8a8,
                                &glib::Bytes::from_owned(image.bytes),
                                image.extent[0] as usize * 4,
                            )));
                        };
                        if let Some(before) = before {
                            set(&this.before, before);
                            this.original.borrow_mut().take();
                        }
                        set(&this.after, after);
                        this.status.set_label("Complete canvas · sRGB preview");
                        this.mark_ready(true);
                    }
                    Err(error) => {
                        this.status.set_label(&error);
                        this.mark_ready(false);
                    }
                }
            }
            this.running.set(false);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn area_reduction_keeps_fractional_edges_and_premultiplied_coverage() {
        let pixels: Vec<_> = (0..6)
            .map(|v| [v as f32, -(v as f32), 2. * v as f32, 1.])
            .collect();
        let mut all = [[0.; 4]; 2];
        accumulate([3, 2], [2, 1], 0, &pixels, &mut all);
        assert_eq!(all, [[11., -11., 22., 6.], [19., -19., 38., 6.]]);
        let mut bands = [[0.; 4]; 2];
        accumulate([3, 2], [2, 1], 0, &pixels[..3], &mut bands);
        accumulate([3, 2], [2, 1], 1, &pixels[3..], &mut bands);
        assert_eq!(all, bands, "strip boundaries cannot change reduction");
        let mut average = [[0.; 4]];
        accumulate(
            [2, 1],
            [1, 1],
            0,
            &[[0., 0., 0., 0.], [1., 0.2, 0., 1.]],
            &mut average,
        );
        let value = average[0].map(|v| v / 2.);
        assert_eq!(value[3], 0.5);
        assert_eq!(
            value[0] / value[3],
            1.,
            "transparent pixels do not darken straight color"
        );
        assert!((value[1] / value[3] - 0.2).abs() < 1e-8);
        let mut identity = [[0.; 4]; 6];
        accumulate([3, 2], [3, 2], 0, &pixels, &mut identity);
        for (sum, source) in identity.iter().zip(pixels) {
            assert_eq!(sum.map(|v| v / 6.), source.map(f64::from));
        }
    }
}
