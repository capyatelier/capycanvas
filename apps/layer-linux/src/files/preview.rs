//! Full-stack color comparison, reduced only after native linear composition.
//! All GPU/readback work stays on the worker; this image never feeds artwork.
use super::*;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::cell::{Cell, RefCell};

pub(super) fn display_headroom(w: &Workspace) -> f32 {
    // Cairo cannot preserve HDR through its 8-bit render target. Temporary
    // canvas SDR/proof toggles do not change the master shown in Export.
    if !w.window.renderer().is_some_and(|r| matches!(r.type_().name(), "GskVulkanRenderer" | "GskGLRenderer")) { return 1.; }
    w.gpu.borrow().as_ref().map_or(1., |g| g.session.engine().backend().display_headroom)
}

struct Image {
    extent: [u32; 2],
    bytes: Vec<u8>,
    clipped: Option<u64>,
    hdr: bool,
    display_hdr: bool,
    range_blocked: bool,
    transparent: Option<bool>,
    fallback: Option<Box<Image>>,
}
fn thumbnail(
    gpu: &SnapshotGpu,
    project: Project,
    background: [f32; 4],
    time: f32,
    control: CaptureControl,
    view: crate::display_color::ViewColor,
    headroom: f32,
    output: Option<ExportRecipe>,
) -> Result<Image, String> {
    let hdr_document = project.document.color.depth.is_float();
    let mut renderer =
        gpu.capture(project, background, time, Default::default(), control)
            .map_err(|e| e.to_string())?;
    let hdr = output.as_ref().is_some_and(|r| r.format.is_hdr());
    let display_hdr = hdr_document && headroom > 1. && (hdr || output.is_none());
    let mut range_blocked = false;
    let mut transparent=None;
    let mut fallback=None;
    let (preview, clipped) = if let Some(recipe) = output {
        recipe.validate()?;
        renderer.set_output_extent(recipe.size.extent(renderer.extent())?)?;
        if let Some(format)=recipe.format.gainmap(){
            let (preview,base,stats)=renderer.preview_gainmap_output([220,160],view.space(),headroom,format,recipe.jpeg_quality,recipe.background.matte())?;
            range_blocked=stats.clipped_channels>0&&!recipe.format.maps_hdr_range();
            fallback=Some(Box::new(present(base,false,false,None,false,None,None)));
            (preview,Some(stats.clipped_channels))
        } else if recipe.format.is_hdr() {
            let (preview, stats) = renderer.preview_hdr_output([220, 160], view.space(), headroom)?;
            range_blocked = stats.clipped_channels > 0 && !recipe.format.maps_hdr_range();
            (preview, Some(stats.clipped_channels))
        } else {
            let (preview, stats) = renderer.preview_output(
                [220, 160], view.space(), &recipe.interpretation(), recipe.encoding, recipe.background.matte(),
            )?;
            (preview, Some(stats.clipped_channels))
        }
    } else {
        let (preview,alpha)=renderer.preview_document_with_coverage([220,160],view.space(),headroom)?;
        transparent=Some(alpha);(preview,None)
    };
    Ok(present(preview,display_hdr,hdr,clipped,range_blocked,transparent,fallback))
}
fn present(preview:layer_render_wgpu::snapshot::SnapshotPreview,display_hdr:bool,hdr:bool,clipped:Option<u64>,range_blocked:bool,transparent:Option<bool>,fallback:Option<Box<Image>>)->Image {
    let mut bytes = Vec::with_capacity(preview.pixels.len() * if display_hdr { 8 } else { 4 });
    // Match the canvas's linear alpha-over-checker. Opaque, tagged textures keep
    // GTK/theme composition from changing translucent artwork edges. HDR uses
    // half-float display transport, like the canvas, after Float32 processing.
    let to_srgb = preview.space.linear_transform(layer_core::color::RgbSpace::Srgb);
    let to_2020 = layer_core::color::hdr::srgb_to_bt2020();
    for (i, pixel) in preview.pixels.iter().enumerate() {
        let x = i as u32 % preview.extent[0];
        let y = i as u32 / preview.extent[0];
        let checker = if (x / 8 + y / 8) % 2 == 0 { 0.94 } else { 0.80 };
        let rgb = std::array::from_fn(|c| f64::from(pixel[c]) + checker * (1. - f64::from(pixel[3])));
        if display_hdr {
            let rgb = layer_core::color::rgb::apply(to_2020, layer_core::color::rgb::apply(to_srgb, rgb));
            for v in rgb.into_iter().map(|v| v.clamp(0., 10000. / 203.) as f32).chain([1.]) {
                bytes.extend_from_slice(&half::f16::from_f32(v).to_bits().to_ne_bytes());
            }
        } else {
            for v in rgb { bytes.push((preview.space.encode(v).clamp(0., 1.) * 255.).round() as u8); }
            bytes.push(255);
        }
    }
    Image {
        extent: preview.extent,
        bytes,
        clipped,
        hdr,
        display_hdr,
        range_blocked,
        transparent,
        fallback,
    }
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
    before_ready: Cell<bool>,
    headroom: Cell<f32>,
    pending: RefCell<Option<Pending>>,
    control: RefCell<Option<CaptureControl>>,
    running: Cell<bool>,
    closed: Cell<bool>,
    serial: Cell<u64>,
    pub ready: Cell<bool>,
    pub range_exceeded: Cell<bool>,
    pub has_transparency: Cell<Option<bool>>,
    show_sdr: Cell<bool>,
    textures: RefCell<[Option<gtk::gdk::Texture>;2]>,
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
        let labels = labels.map(|label| gtk::Label::builder().label(label).wrap(true).build());
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
            before_ready: Cell::new(false),
            headroom: Cell::new(1.),
            pending: RefCell::new(None),
            control: RefCell::new(None),
            running: Cell::new(false),
            closed: Cell::new(false),
            serial: Cell::new(0),
            ready: Cell::new(false),
            range_exceeded: Cell::new(false),
            has_transparency: Cell::new(None),
            show_sdr: Cell::new(false),
            textures: RefCell::new([None,None]),
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
        self.range_exceeded.set(false);
        self.serial.set(self.serial.get() + 1);
        self.pending.borrow_mut().take();
        if let Some(control) = self.control.borrow().as_ref() {
            control.cancel();
        }
        self.after.set_paintable(None::<&gtk::gdk::Paintable>);
        self.status.set_label(message);
        self.status.set_visible(!message.is_empty());
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
    pub fn set_headroom(&self, headroom: f32) -> bool {
        if self.headroom.replace(headroom) == headroom { return false; }
        self.before_ready.set(false);
        self.before.set_paintable(None::<&gtk::gdk::Paintable>);
        true
    }
    pub fn follow_display(self: &Rc<Self>, w: &Rc<Workspace>, refresh: Rc<dyn Fn()>) {
        glib::timeout_add_local(std::time::Duration::from_millis(250), glib::clone!(
            #[weak(rename_to = this)] self, #[weak] w, #[upgrade_or] glib::ControlFlow::Break,
            move || {
                if this.closed.get() { return glib::ControlFlow::Break; }
                if this.set_headroom(display_headroom(&w)) { refresh(); }
                glib::ControlFlow::Continue
            }
        ));
    }
    pub fn show_fallback(&self,show:bool){
        self.show_sdr.set(show);
        if let Some(texture)=&self.textures.borrow()[usize::from(show)]{self.after.set_paintable(Some(texture));}
        let label=if show{"SDR fallback"}else if self.headroom.get()>1.{"HDR output"}else{"HDR output (SDR display)"};
        self.labels[1].set_label(label);self.after.set_alternative_text(Some(label));
    }
    pub fn request_output(self: &Rc<Self>, snapshot: &DocumentExport, recipe: ExportRecipe) {
        let hdr_document = snapshot.project.document.color.depth.is_float();
        let hdr_view = self.headroom.get() > 1.;
        let labels = [
            if hdr_document { if hdr_view { "HDR master" } else { "Master (SDR preview)" } } else { "Master" },
            if recipe.format.is_hdr() { if hdr_view { "HDR output" } else { "Output (SDR preview)" } }
                else if hdr_document { "SDR output" } else { "Output" },
        ];
        for ((label, picture), text) in self.labels.iter().zip([&self.before, &self.after]).zip(labels) {
            label.set_label(text); picture.set_alternative_text(Some(text));
        }
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
                let original = (!this.before_ready.get()).then(|| this.original.borrow().clone()).flatten();
                let headroom = this.headroom.get();
                let serial = next.serial;
                let view = this.view;
                let gpu = this.gpu.clone();
                let result = gio::spawn_blocking(move || {
                    let before = original
                        .map(|project| {
                            thumbnail(
                                &gpu,
                                project,
                                next.background,
                                next.time,
                                control.clone(),
                                view,
                                headroom,
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
                        headroom,
                        next.output,
                    );
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
                        let texture = |image: Image| {
                            if image.display_hdr {
                                gtk::gdk::MemoryTextureBuilder::new()
                                    .set_width(image.extent[0] as i32).set_height(image.extent[1] as i32)
                                    .set_format(gtk::gdk::MemoryFormat::R16g16b16a16Float)
                                    .set_stride(image.extent[0] as usize * 8)
                                    .set_color_state(&gtk::gdk::ColorState::rec2100_linear())
                                    .set_bytes(Some(&glib::Bytes::from_owned(image.bytes))).build()
                            } else { view.rgba8(image.extent, image.bytes) }
                        };
                        if let Some(before) = before {
                            this.has_transparency.set(before.transparent);
                            this.before.set_paintable(Some(&texture(before)));
                            this.before_ready.set(true);
                        }
                        let mut after=match after {Ok(after)=>after,Err(error)=>{this.status.set_label(&error);this.status.set_visible(true);this.mark_ready(false);continue;}};
                        let ready = !after.range_blocked;
                        let show_status=after.range_blocked || after.clipped.is_some_and(|count|count>0);
                        this.range_exceeded.set(after.range_blocked);
                        let description = if after.range_blocked { "Some colors exceed this format’s HDR range. Adjust the artwork or enable clipping below." } else if after.hdr { if after.clipped == Some(0) { "HDR range checked" } else { "HDR range checked · out-of-range colors will be clipped" } } else { match after.clipped {
                            Some(0) => "Output preview",
                            Some(_) => "Output preview · some colors exceed the output gamut",
                            None => "Complete canvas",
                        } };
                        let viewing = if after.display_hdr { "HDR view".to_string() } else { format!("{} view", view.space().name()) };
                        let fallback=after.fallback.take().map(|i|texture(*i));
                        let hdr=texture(after);
                        this.after.set_paintable(Some(if this.show_sdr.get(){fallback.as_ref().unwrap_or(&hdr)}else{&hdr}));
                        *this.textures.borrow_mut()=[Some(hdr),fallback];
                        if this.show_sdr.get(){this.labels[1].set_label("SDR fallback");this.after.set_alternative_text(Some("SDR fallback"));}
                        this.status.set_label(&format!("{description} · {viewing}"));
                        this.status.set_visible(show_status);
                        this.mark_ready(ready);
                    }
                    Err(error) => {
                        this.status.set_label(&error);
                        this.status.set_visible(true);
                        this.mark_ready(false);
                    }
                }
            }
            this.running.set(false);
        });
    }
}
