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
) -> Result<Image, layer_ui::ColorFeatureError> {
    let hdr_document = project.document.color.depth.is_float();
    let mut renderer =
        gpu.capture(project, background, time, control)
            .map_err(|e| e.to_string())?;
    let hdr = output.as_ref().is_some_and(|r| r.format.is_hdr());
    let display_hdr = hdr_document && headroom > 1. && (hdr || output.is_none());
    let (preview, clipped, range_blocked, transparent, fallback) = if let Some(recipe) = output {
        let output = layer_host::export::preview_recipe(&mut renderer, [220, 160], view.space(), headroom, &recipe)?;
        let fallback = output.sdr_base.map(|base| Box::new(present(base, false, false, None, false, None, None)));
        (output.after, Some(output.clipped), output.range_blocked, output.transparent, fallback)
    } else {
        let (preview, alpha) = renderer.preview_document_with_coverage([220, 160], view.space(), headroom)?;
        (preview, None, false, Some(alpha), None)
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
    let checker = crate::display_color::checker_linear().map(f64::from);
    let cell = layer_ui::TRANSPARENCY_CHECKER_CELL as u32;
    for (i, pixel) in preview.pixels.iter().enumerate() {
        let x = i as u32 % preview.extent[0];
        let y = i as u32 / preview.extent[0];
        let checker = checker[((x / cell + y / cell) % 2) as usize];
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
    copy: RefCell<layer_ui::color_feature_copy::ComparisonCopy>,
    localization: RefCell<std::sync::Arc<layer_ui::Localizer>>,
    localization_bound: Cell<bool>,
    output_labels: Cell<Option<(bool, bool)>>,
    failure: RefCell<Option<layer_ui::ColorFeatureError>>,
    pub widget: gtk::Box,
    gpu: SnapshotGpu,
    view: crate::display_color::ViewColor,
    before: gtk::Picture,
    after: gtk::Picture,
    labels: [gtk::Label; 2],
    pub status: gtk::Label,
    status_copy: RefCell<Option<((u8, bool, layer_core::color::RgbSpace), String)>>,
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
    pub fn new(gpu: SnapshotGpu, original: Project, view: crate::display_color::ViewColor, localization: &std::sync::Arc<layer_ui::Localizer>) -> Rc<Self> {
        let copy = layer_ui::color_feature_copy::ComparisonCopy::new(localization);
        let labels = [copy.before.clone(), copy.after.clone()];
        Self::with_labels(gpu, original, view, labels, copy, localization.clone())
    }
    pub fn for_output(gpu: SnapshotGpu, original: Project, view: crate::display_color::ViewColor, localization: &std::sync::Arc<layer_ui::Localizer>) -> Rc<Self> {
        let copy = layer_ui::color_feature_copy::ComparisonCopy::new(localization);
        let labels = [copy.master.clone(), copy.output.clone()];
        let hdr_document = original.document.color.depth.is_float();
        let comparison = Self::with_labels(gpu, original, view, labels, copy, localization.clone());
        comparison.output_labels.set(Some((hdr_document, false)));
        comparison
    }
    fn with_labels(
        gpu: SnapshotGpu,
        original: Project,
        view: crate::display_color::ViewColor,
        labels: [std::sync::Arc<str>; 2],
        copy: layer_ui::color_feature_copy::ComparisonCopy,
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let pictures = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        pictures.set_homogeneous(true);
        pictures.set_halign(gtk::Align::Center);
        let labels = labels.map(|label| gtk::Label::builder().label(label.as_ref()).wrap(true).build());
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
            .label(copy.choose_profile.as_ref())
            .wrap(true)
            .xalign(0.)
            .build();
        status.set_widget_name("color-preview-status");
        widget.append(&pictures);
        widget.append(&status);
        Rc::new(Self {
            localization_bound: Cell::new(false), copy: RefCell::new(copy), localization: RefCell::new(localization), output_labels: Cell::new(None), failure: RefCell::new(None),
            gpu,
            view,
            widget,
            before,
            after,
            labels,
            status,
            status_copy: RefCell::new(None),
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
        self.failure.borrow_mut().take();
        self.status_copy.borrow_mut().take();
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
    pub fn invalidate_error(&self, error: layer_ui::ColorFeatureError) {
        self.invalidate(&error.message(&self.localization.borrow()));
        *self.failure.borrow_mut() = Some(error);
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
    pub fn bind_localization(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.localization_bound.replace(true) { return; }
        let weak = Rc::downgrade(self);
        w.on_localization(move |localization| {
            let Some(this) = weak.upgrade().filter(|this| !this.closed.get()) else { return false };
            *this.copy.borrow_mut() = layer_ui::color_feature_copy::ComparisonCopy::new(localization);
            *this.localization.borrow_mut() = localization.clone();
            this.refresh_labels();
            if let Some(error) = this.failure.borrow().as_ref() { this.status.set_label(&error.message(localization)); }
            else if let Some((key, text)) = this.status_copy.borrow_mut().as_mut() {
                *text = this.status_message(*key);
                this.status.set_label(text);
            } else if this.running.get() { this.status.set_label(&this.copy.borrow().rendering); }
            else if !this.ready.get() { this.status.set_label(&this.copy.borrow().choose_profile); }
            true
        });
    }
    pub fn follow_display(self: &Rc<Self>, w: &Rc<Workspace>, refresh: Rc<dyn Fn()>) {
        self.bind_localization(w);
        glib::timeout_add_local(std::time::Duration::from_millis(250), glib::clone!(
            #[weak(rename_to = this)] self, #[weak] w, #[upgrade_or] glib::ControlFlow::Break,
            move || {
                if this.closed.get() { return glib::ControlFlow::Break; }
                if this.set_headroom(w.picker_headroom()) { refresh(); }
                glib::ControlFlow::Continue
            }
        ));
    }
    pub fn show_fallback(&self,show:bool){
        let copy = self.copy.borrow();
        self.show_sdr.set(show);
        if let Some(texture)=&self.textures.borrow()[usize::from(show)]{self.after.set_paintable(Some(texture));}
        let label=if show{copy.sdr_fallback.as_ref()}else if self.headroom.get()>1.{copy.hdr_output.as_ref()}else{copy.hdr_sdr_display.as_ref()};
        self.labels[1].set_label(label);self.after.set_alternative_text(Some(label));
    }
    fn refresh_labels(&self) {
        let copy = self.copy.borrow();
        let hdr_view = self.headroom.get() > 1.;
        let labels = match self.output_labels.get() {
            Some((hdr_document, hdr_output)) => [
                if hdr_document { if hdr_view { &copy.hdr_master } else { &copy.master_sdr } } else { &copy.master },
                if self.show_sdr.get() { &copy.sdr_fallback } else if hdr_output { if hdr_view { &copy.hdr_output } else { &copy.output_sdr } }
                    else if hdr_document { &copy.sdr_output } else { &copy.output },
            ],
            None => [&copy.before, &copy.after],
        };
        for ((label, picture), text) in self.labels.iter().zip([&self.before, &self.after]).zip(labels) {
            label.set_label(text); picture.set_alternative_text(Some(text));
        }
    }
    fn status_message(&self, (kind, display_hdr, space): (u8, bool, layer_core::color::RgbSpace)) -> String {
        let copy = self.copy.borrow();
        let localization = self.localization.borrow();
        let description = [&copy.outside_hdr, &copy.hdr_checked, &copy.hdr_clipped, &copy.output_preview, &copy.output_gamut, &copy.complete_canvas][kind as usize];
        let viewing = if display_hdr { copy.hdr_view.to_string() } else {
            let mut args = layer_ui::FluentArgs::new(); args.set("space", space.name());
            localization.format(layer_ui::MessageId::COLOR_FEATURES_COMPARISON_VIEW, &args)
        };
        let mut args = layer_ui::FluentArgs::new(); args.set("description", description.as_ref()); args.set("view", viewing.as_str());
        localization.format(layer_ui::MessageId::COLOR_FEATURES_COMPARISON_STATUS, &args)
    }
    pub fn request_output(self: &Rc<Self>, snapshot: &DocumentExport, recipe: ExportRecipe) {
        self.output_labels.set(Some((snapshot.project.document.color.depth.is_float(), recipe.format.is_hdr())));
        self.refresh_labels();
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
        self.invalidate(self.copy.borrow().rendering.as_ref());
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
                    Ok::<_, layer_ui::ColorFeatureError>((before, after))
                })
                .await
                .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Preview worker failed".into()))
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
                        let mut after=match after {Ok(after)=>after,Err(error)=>{this.status.set_label(&error.message(&this.localization.borrow()));*this.failure.borrow_mut() = Some(error);this.status.set_visible(true);this.mark_ready(false);continue;}};
                        let ready = !after.range_blocked;
                        let show_status=after.range_blocked || after.clipped.is_some_and(|count|count>0);
                        this.range_exceeded.set(after.range_blocked);
                        let kind = if after.range_blocked {0} else if after.hdr {if after.clipped == Some(0) {1} else {2}} else {match after.clipped {Some(0)=>3,Some(_)=>4,None=>5}};
                        let key = (kind, after.display_hdr, view.space());
                        if this.status_copy.borrow().as_ref().is_none_or(|(previous,_)| *previous != key) {
                            *this.status_copy.borrow_mut() = Some((key, this.status_message(key)));
                        }
                        let fallback=after.fallback.take().map(|i|texture(*i));
                        let hdr=texture(after);
                        this.after.set_paintable(Some(if this.show_sdr.get(){fallback.as_ref().unwrap_or(&hdr)}else{&hdr}));
                        *this.textures.borrow_mut()=[Some(hdr),fallback];
                        if this.show_sdr.get(){this.labels[1].set_label(this.copy.borrow().sdr_fallback.as_ref());this.after.set_alternative_text(Some(this.copy.borrow().sdr_fallback.as_ref()));}
                        this.status.set_label(&this.status_copy.borrow().as_ref().unwrap().1);
                        this.status.set_visible(show_status);
                        this.mark_ready(ready);
                    }
                    Err(error) => {
                        this.status.set_label(&error.message(&this.localization.borrow()));
                        *this.failure.borrow_mut() = Some(error);
                        this.status.set_visible(true);
                        this.mark_ready(false);
                    }
                }
            }
            this.running.set(false);
        });
    }
}
