//! Full-stack color comparison, reduced only after native linear composition.
//! All GPU/readback work stays on the worker; this image never feeds artwork.
use super::*;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::cell::{Cell, RefCell};

struct Image {
    extent: [u32; 2],
    bytes: Vec<u8>,
    clipped: Option<u64>,
    hdr: bool,
}
fn thumbnail(
    gpu: &SnapshotGpu,
    project: Project,
    background: [f32; 4],
    time: f32,
    control: CaptureControl,
    view: crate::display_color::ViewColor,
    output: Option<ExportRecipe>,
) -> Result<Image, String> {
    let mut renderer =
        gpu.capture(project, background, time, Default::default(), control)
            .map_err(|e| e.to_string())?;
    let hdr = output.as_ref().is_some_and(|r| r.format.is_hdr());
    let mut hdr_clipped = None;
    if let Some(recipe) = output.as_ref().filter(|r| r.format.is_hdr()) {
        renderer.set_output_extent(recipe.size.extent(renderer.extent())?)?;
        let count = renderer.inspect_hdr_output()?.clipped_channels;
        if count > 0 && !recipe.format.maps_hdr_range() {
            return Err("Some colors exceed HDR PNG’s range. Adjust the artwork or enable ‘Clip out-of-range colors’ in Advanced.".into());
        }
        hdr_clipped = Some(count);
    }
    let (preview, clipped) = if let Some(recipe) = output.filter(|r| !r.format.is_hdr()) {
        recipe.validate()?;
        renderer.set_output_extent(recipe.size.extent(renderer.extent())?)?;
        let (preview, statistics) = renderer.preview_output(
            [220, 160],
            view.space(),
            &recipe.interpretation(),
            recipe.encoding,
            recipe.background.matte(),
        )?;
        (preview, Some(statistics.clipped_channels))
    } else {
        (renderer.preview_document([220, 160], view.space())?, hdr_clipped)
    };
    let mut bytes = Vec::with_capacity(preview.pixels.len() * 4);
    // Match the canvas's linear alpha-over-checker. Opaque, tagged bytes keep
    // GTK/theme composition from changing the artwork's translucent edges.
    for (i, pixel) in preview.pixels.iter().enumerate() {
        let x = i as u32 % preview.extent[0];
        let y = i as u32 / preview.extent[0];
        let checker = if (x / 8 + y / 8) % 2 == 0 { 0.94 } else { 0.80 };
        for c in &pixel[..3] {
            let linear = f64::from(*c) + checker * (1. - f64::from(pixel[3]));
            bytes.push((preview.space.encode(linear).clamp(0., 1.) * 255.).round() as u8);
        }
        bytes.push(255);
    }
    Ok(Image {
        extent: preview.extent,
        bytes,
        clipped,
        hdr,
    })
}

struct Pending {
    project: Project,
    background: [f32; 4],
    time: f32,
    serial: u64,
    output: Option<ExportRecipe>,
}
/// One worker plus one replaceable request. Cancellation is acknowledged before
/// starting its successor, so rapid profile changes cannot accumulate GPU jobs.
pub(super) struct Comparison {
    pub widget: gtk::Box,
    gpu: SnapshotGpu,
    view: crate::display_color::ViewColor,
    before: gtk::Picture,
    after: gtk::Picture,
    labels: [gtk::Label; 2],
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
    pub fn new(gpu: SnapshotGpu, original: Project, view: crate::display_color::ViewColor) -> Rc<Self> {
        Self::with_labels(gpu, original, view, ["Before", "After"])
    }
    pub fn for_output(gpu: SnapshotGpu, original: Project, view: crate::display_color::ViewColor) -> Rc<Self> {
        Self::with_labels(gpu, original, view, ["Master", "Output"])
    }
    fn with_labels(
        gpu: SnapshotGpu,
        original: Project,
        view: crate::display_color::ViewColor,
        labels: [&str; 2],
    ) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let pictures = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        pictures.set_homogeneous(true);
        pictures.set_halign(gtk::Align::Center);
        let labels = labels.map(|label| gtk::Label::new(Some(label)));
        let make = |label: &gtk::Label, name: &str| {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let picture = gtk::Picture::new();
            picture.set_widget_name(name);
            picture.set_alternative_text(Some(&label.text()));
            picture.set_can_shrink(true);
            picture.set_size_request(160, 100);
            column.append(label);
            // Measure a bounded preview area, not the image's height-for-width
            // request at the entire dialog width. Contain retains all edges.
            let preview = gtk::Overlay::new();
            preview.set_child(Some(&gtk::DrawingArea::builder().content_width(160).content_height(120).build()));
            preview.add_overlay(&picture);
            column.append(&preview);
            pictures.append(&column);
            picture
        };
        let before = make(&labels[0], "color-preview-before");
        let after = make(&labels[1], "color-preview-after");
        let status = gtk::Label::builder()
            .label("Choose a profile to preview the complete canvas.")
            .wrap(true)
            .xalign(0.)
            .build();
        status.set_widget_name("color-preview-status");
        widget.append(&pictures);
        widget.append(&status);
        Rc::new(Self {
            gpu,
            view,
            widget,
            before,
            after,
            labels,
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
        self.after.set_paintable(None::<&gtk::gdk::Paintable>);
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
        self.request_image(project, background, time, None);
    }
    pub fn request_output(self: &Rc<Self>, snapshot: &DocumentExport, recipe: ExportRecipe) {
        self.labels[0].parent().unwrap().set_visible(!recipe.format.is_hdr());
        let labels = if recipe.format.is_hdr() { ["SDR master preview", "SDR preview"] } else { ["Master", "Output"] };
        for (label, text) in self.labels.iter().zip(labels) { label.set_label(text); }
        self.request_image(
            snapshot.project.clone(),
            snapshot.background,
            snapshot.time,
            Some(recipe),
        );
    }
    fn request_image(
        self: &Rc<Self>,
        project: Project,
        background: [f32; 4],
        time: f32,
        output: Option<ExportRecipe>,
    ) {
        self.invalidate("Rendering the complete canvas…");
        *self.pending.borrow_mut() = Some(Pending {
            project,
            background,
            time,
            serial: self.serial.get(),
            output,
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
                let view = this.view;
                let gpu = this.gpu.clone();
                let result = gio::spawn_blocking(move || {
                    let before = original.filter(|_| !next.output.as_ref().is_some_and(|r| r.format.is_hdr()))
                        .map(|project| {
                            thumbnail(
                                &gpu,
                                project,
                                next.background,
                                next.time,
                                control.clone(),
                                view,
                                None,
                            )
                        })
                        .transpose()?;
                    let after = thumbnail(
                        &gpu,
                        next.project,
                        next.background,
                        next.time,
                        control,
                        view,
                        next.output,
                    )?;
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
                            picture.set_paintable(Some(&view.rgba8(image.extent, image.bytes)));
                        };
                        if let Some(before) = before {
                            set(&this.before, before);
                            this.original.borrow_mut().take();
                        }
                        let description = if after.hdr { if after.clipped == Some(0) { "HDR range checked" } else { "HDR range checked · out-of-range colors will be clipped" } } else { match after.clipped {
                            Some(0) => "Output preview",
                            Some(_) => "Output preview · some colors exceed the output gamut",
                            None => "Complete canvas",
                        } };
                        set(&this.after, after);
                        this.status
                            .set_label(&format!("{description} · {} view", view.space().name()));
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
