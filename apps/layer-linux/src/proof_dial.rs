//! Circular Proof controls. Rust defines geometry and recipe mapping; GTK owns
//! capture, keyboard/accessibility, drawing and one bounded thumbnail worker.
use gtk::{cairo, gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::color::{
    RgbSpace,
    hdr::{LocalToneGuide, SdrRendition},
};
use layer_ui::{
    ContactPhase,
    parameter_pad::{ParameterArc, ParameterDialGeometry},
    proof_panel::{sdr_from_pad, sdr_pad_values},
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

pub(crate) struct PreviewSource {
    pub image: layer_render_wgpu::snapshot::SnapshotPreview,
    pub guide: Arc<LocalToneGuide>,
}
impl PreviewSource {
    fn accent(&self) -> [f64; 3] {
        let matrix = self.image.space.linear_transform(RgbSpace::Srgb);
        let mut sum = [0.; 3];
        let mut weight = 0.;
        for p in &self.image.pixels {
            if p[3] <= 0. {
                continue;
            }
            let rgb =
                layer_core::color::rgb::apply(matrix, [p[0] as f64, p[1] as f64, p[2] as f64]);
            let peak = rgb.into_iter().fold(0f64, f64::max);
            if peak <= 0. {
                continue;
            }
            let rgb = rgb.map(|v| RgbSpace::Srgb.encode((v / peak).clamp(0., 1.)));
            let minimum = rgb.into_iter().fold(1f64, f64::min);
            let w = (1. - minimum).powi(2) * f64::from(p[3]);
            for c in 0..3 {
                sum[c] += rgb[c] * w;
            }
            weight += w;
        }
        if weight > 1e-6 {
            sum.map(|v| v / weight)
        } else {
            [0.65; 3]
        }
    }
    fn render(&self, recipe: SdrRendition) -> Vec<u8> {
        let image = &self.image;
        let mapper = recipe.mapper(image.space, RgbSpace::Srgb);
        let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
        for (i, &pixel) in image.pixels.iter().enumerate() {
            let position = [i as u32 % image.extent[0], i as u32 / image.extent[0]];
            let position = std::array::from_fn(|c| {
                (position[c] as f32 + 0.5) * self.guide.document_extent[c] as f32
                    / image.extent[c] as f32
            });
            let pixel = if recipe.is_local() {
                self.guide.adjust(pixel, position, image.space, recipe)
            } else {
                pixel
            };
            let pixel = mapper.map_premultiplied(pixel);
            let a = pixel[3].clamp(0., 1.);
            // Cairo's ARGB32 transport is a disposable SDR UI preview only.
            let rgb: [u8; 3] = std::array::from_fn(|c| {
                (RgbSpace::Srgb
                    .encode(if a > 0. { f64::from(pixel[c] / a) } else { 0. })
                    .clamp(0., 1.)
                    * f64::from(a)
                    * 255.)
                    .round() as u8
            });
            bytes.extend_from_slice(
                &((u32::from((a * 255.).round() as u8) << 24)
                    | (u32::from(rgb[0]) << 16)
                    | (u32::from(rgb[1]) << 8)
                    | u32::from(rgb[2]))
                .to_ne_bytes(),
            );
        }
        bytes
    }
}

mod layout {
    use super::*;
    #[derive(Default)]
    pub struct DialLayout {
        pub(super) owner: RefCell<Weak<ProofDial>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for DialLayout {
        const NAME: &'static str = "CapyParameterDialLayout";
        type Type = super::DialLayout;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for DialLayout {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for DialLayout {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }
        fn measure(&self, _: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            (
                128,
                if for_size < 0 { 226 } else { for_size.max(128) },
                -1,
                -1,
            )
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(p) = self.owner.borrow().upgrade() else {
                return;
            };
            let size = width.min(height).max(1) as f32;
            let Some(g) = ParameterDialGeometry::new(size) else {
                return;
            };
            let offset = [(width as f32 - size) * 0.5, 0.];
            let allocate = |widget: &gtk::Widget, rect: [f32; 4]| {
                widget.allocate(
                    rect[2].round() as i32,
                    rect[3].round() as i32,
                    baseline,
                    Some(
                        gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(
                            offset[0] + rect[0],
                            offset[1] + rect[1],
                        )),
                    ),
                );
            };
            allocate(p.field.upcast_ref(), [0., 0., size, size]);
            for arc in &p.arcs {
                allocate(arc.upcast_ref(), [0., 0., size, size]);
            }
            allocate(p.reset.upcast_ref(), g.reset);
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(p) = self.owner.borrow().upgrade() else {
                return;
            };
            for child in [
                p.field.upcast_ref::<gtk::Widget>(),
                p.arcs[0].upcast_ref(),
                p.arcs[1].upcast_ref(),
                p.reset.upcast_ref(),
            ] {
                obj.snapshot_child(child, snapshot);
            }
            let size = obj.width().min(obj.height()) as f32;
            let Some(g) = ParameterDialGeometry::new(size) else {
                return;
            };
            let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(
                0.,
                0.,
                obj.width() as f32,
                obj.height() as f32,
            ));
            cr.translate((obj.width() as f64 - f64::from(size)) * 0.5, 0.);
            let v = p.pad.get();
            let strength = v.map_or("—".into(), |v| format!("{:.0}%", v[1] * 100.));
            let balance = v.map_or("—".into(), |v| format!("{:+.0}%", v[0] * 100.));
            // Side openings carry the inner control's readouts; the outer arcs
            // carry their own values above and below. All names are in tooltips.
            let radius = f64::from(g.field.disc_radius()) + 7.;
            curved_text(
                &cr,
                obj.upcast_ref(),
                &strength,
                g.field.center,
                radius,
                180.,
                true,
                size,
            );
            curved_text(
                &cr,
                obj.upcast_ref(),
                &balance,
                g.field.center,
                radius,
                0.,
                false,
                size,
            );
        }
    }
}
glib::wrapper! { pub struct DialLayout(ObjectSubclass<layout::DialLayout>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget; }

mod arc_scale {
    use super::*;
    #[derive(Default)]
    pub struct ArcScale {
        pub index: Cell<usize>,
        pub(super) owner: RefCell<Weak<ProofDial>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for ArcScale {
        const NAME: &'static str = "CapyParameterArcScale";
        type Type = super::ArcScale;
        type ParentType = gtk::Scale;
    }
    impl ObjectImpl for ArcScale {}
    impl WidgetImpl for ArcScale {
        fn contains(&self, x: f64, y: f64) -> bool {
            self.obj()
                .geometry()
                .is_some_and(|g| g.contains([x as f32, y as f32]))
        }
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(g) = obj.geometry() else {
                return;
            };
            let index = self.index.get();
            let a = obj.adjustment();
            let f = ((obj.value() - a.lower()) / (a.upper() - a.lower())) as f32;
            let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(
                0.,
                0.,
                obj.width() as f32,
                obj.height() as f32,
            ));
            let start = g.point(0.);
            let end = g.point(1.);
            let gradient = cairo::LinearGradient::new(start[0] as f64, 0., end[0] as f64, 0.);
            if index == 0 {
                gradient.add_color_stop_rgb(0., 0.04, 0.04, 0.04);
                gradient.add_color_stop_rgb(0.5, 0.55, 0.55, 0.55);
                gradient.add_color_stop_rgb(1., 1., 1., 1.);
            } else {
                let color = self
                    .owner
                    .borrow()
                    .upgrade()
                    .map_or([0.82, 0.42, 0.18], |p| p.accent.get());
                gradient.add_color_stop_rgb(0., 0.95, 0.95, 0.95);
                gradient.add_color_stop_rgb(1., color[0], color[1], color[2]);
            }
            let _ = cr.set_source(&gradient);
            cr.set_line_width(g.width as f64);
            cr.set_line_cap(cairo::LineCap::Round);
            if g.sweep > 0. {
                cr.arc(
                    g.center[0] as f64,
                    g.center[1] as f64,
                    g.radius as f64,
                    g.start as f64,
                    (g.start + g.sweep) as f64,
                );
            } else {
                cr.arc_negative(
                    g.center[0] as f64,
                    g.center[1] as f64,
                    g.radius as f64,
                    g.start as f64,
                    (g.start + g.sweep) as f64,
                );
            }
            let _ = cr.stroke();
            marker(&cr, g.point(f), g.marker_radius, obj.has_visible_focus());
            let label = if index == 0 {
                format!("{:+.0}%", obj.value() * 25.)
            } else {
                format!("{:.0}%", obj.value() * 100.)
            };
            curved_text(
                &cr,
                obj.upcast_ref(),
                &label,
                g.center,
                (g.radius + g.width * 0.5 + 5.) as f64,
                if index == 0 { -90. } else { 90. },
                index == 1,
                obj.width() as f32,
            );
        }
    }
    impl RangeImpl for ArcScale {}
    impl ScaleImpl for ArcScale {}
}
glib::wrapper! { pub struct ArcScale(ObjectSubclass<arc_scale::ArcScale>) @extends gtk::Scale,gtk::Range,gtk::Widget, @implements gtk::Accessible,gtk::Buildable,gtk::ConstraintTarget,gtk::Orientable; }
impl ArcScale {
    fn geometry(&self) -> Option<ParameterArc> {
        Some(
            ParameterDialGeometry::new(self.width().min(self.height()) as f32)?.arcs
                [self.imp().index.get()],
        )
    }
}

type Changed = Box<dyn Fn(ContactPhase, SdrRendition)>;
pub(crate) struct ProofDial {
    pub root: DialLayout,
    pub field: gtk::DrawingArea,
    pub arcs: [ArcScale; 2],
    pub reset: gtk::Button,
    recipe: Cell<SdrRendition>,
    pad: Cell<Option<[f64; 2]>>,
    before: Cell<(SdrRendition, Option<[f64; 2]>)>,
    active: Cell<Option<usize>>,
    origin: Cell<[f64; 2]>,
    double: Cell<bool>,
    updating: Cell<bool>,
    changed: RefCell<Vec<Changed>>,
    source: RefCell<Option<Arc<PreviewSource>>>,
    image: RefCell<Option<cairo::ImageSurface>>,
    accent: Cell<[f64; 3]>,
    rendering: Cell<bool>,
    dirty: Cell<bool>,
    serial: Cell<u64>,
}
impl ProofDial {
    pub fn new() -> Rc<Self> {
        let root: DialLayout = glib::Object::new();
        root.set_widget_name("sdr-proof-dial");
        root.add_css_class("color-panel");
        root.set_hexpand(true);
        root.set_vexpand(true);
        let field = gtk::DrawingArea::new();
        field.set_widget_name("sdr-tone-pad-surface");
        field.set_focusable(true);
        field.set_parent(&root);
        field.update_property(&[gtk::accessible::Property::Label("Strength and balance"),gtk::accessible::Property::Description("Up increases local tone-mapping strength. Right favors texture; left favors tone compression. Arrow keys adjust; Escape cancels; double-click resets.")]);
        field.set_tooltip_text(Some("Strength ↑ · Texture →\nDrag to adjust. Arrow keys fine-tune; Shift moves farther. Double-click resets."));
        let arcs = std::array::from_fn(|i| {
            let arc: ArcScale = glib::Object::builder()
                .property("orientation", gtk::Orientation::Horizontal)
                .property("draw-value", false)
                .build();
            arc.imp().index.set(i);
            arc.add_css_class("parameter-arc-scale");
            arc.set_has_origin(false);
            arc.set_focusable(true);
            let spec = &layer_ui::proof_panel::sdr_number_controls()[i];
            arc.set_range(spec.numeric.min, spec.numeric.max);
            arc.set_increments(spec.numeric.step, spec.numeric.step * 10.);
            arc.set_widget_name(&format!("sdr-appearance-{}", spec.key));
            arc.update_property(&[gtk::accessible::Property::Label(spec.label)]);
            arc.set_tooltip_text(Some(if i==0{"Brightness · Darker ← → Brighter\nBlack and white stay fixed. Double-click resets."}else{"Color intensity · White ← → Color\nRetain more highlight color by lowering brightness. Double-click resets."}));
            arc.set_parent(&root);
            arc
        });
        let reset: crate::tool_panels::WheelButton = glib::Object::new();
        reset.set_icon_name("view-refresh-symbolic");
        reset.add_css_class("flat");
        reset.add_css_class("color-utility");
        reset.add_css_class("color-swap");
        reset.set_widget_name("sdr-appearance-reset");
        reset.set_tooltip_text(Some("Reset SDR appearance"));
        reset.update_property(&[gtk::accessible::Property::Label("Reset SDR appearance")]);
        reset.set_parent(&root);
        let recipe = SdrRendition::default();
        let pad = sdr_pad_values(recipe);
        let p = Rc::new(Self {
            root,
            field,
            arcs,
            reset: reset.upcast(),
            recipe: Cell::new(recipe),
            pad: Cell::new(pad),
            before: Cell::new((recipe, pad)),
            active: Cell::new(None),
            origin: Cell::new([0.; 2]),
            double: Cell::new(false),
            updating: Cell::new(false),
            changed: Default::default(),
            source: Default::default(),
            image: Default::default(),
            accent: Cell::new([0.82, 0.42, 0.18]),
            rendering: Cell::new(false),
            dirty: Cell::new(false),
            serial: Cell::new(0),
        });
        *p.root.imp().owner.borrow_mut() = Rc::downgrade(&p);
        for a in &p.arcs {
            *a.imp().owner.borrow_mut() = Rc::downgrade(&p);
        }
        p.field.set_draw_func(glib::clone!(
            #[weak]
            p,
            move |area, cr, w, h| p.draw_field(area, cr, w, h)
        ));
        for (part, widget) in [
            p.field.upcast_ref::<gtk::Widget>(),
            p.arcs[0].upcast_ref(),
            p.arcs[1].upcast_ref(),
        ]
        .into_iter()
        .enumerate()
        {
            p.wire(part, widget);
        }
        for i in 0..2 {
            p.arcs[i].connect_value_changed(glib::clone!(
                #[weak]
                p,
                move |arc| {
                    if p.updating.get() {
                        return;
                    }
                    let own = p.active.get().is_none();
                    if own {
                        p.begin(i + 1);
                    }
                    let mut r = p.recipe.get();
                    if i == 0 {
                        r.exposure = arc.value() as f32;
                    } else {
                        r.highlight_color = arc.value() as f32;
                    }
                    p.change(r, p.pad.get());
                    if own {
                        p.end(false);
                    }
                }
            ));
        }
        p.reset.connect_clicked(glib::clone!(
            #[weak]
            p,
            move |_| {
                p.end(true);
                p.begin(0);
                let r = SdrRendition {
                    headroom: p.recipe.get().headroom,
                    ..Default::default()
                };
                p.change(r, sdr_pad_values(r));
                p.end(false);
            }
        ));
        p.refresh();
        p
    }
    pub fn recipe(&self) -> SdrRendition {
        self.recipe.get()
    }
    pub fn set_recipe(self: &Rc<Self>, recipe: SdrRendition) {
        if self.recipe.get() == recipe {
            return;
        }
        self.recipe.set(recipe);
        self.pad.set(sdr_pad_values(recipe));
        self.refresh();
        self.render();
    }
    pub fn connect_changed(&self, f: impl Fn(ContactPhase, SdrRendition) + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }
    pub fn set_preview(self: &Rc<Self>, source: Option<Arc<PreviewSource>>) {
        if self
            .source
            .borrow()
            .as_ref()
            .zip(source.as_ref())
            .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
        {
            return;
        }
        if source.is_none() && self.source.borrow().is_none() {
            return;
        }
        self.accent
            .set(source.as_ref().map_or([0.65; 3], |source| source.accent()));
        self.arcs[1].queue_draw();
        *self.source.borrow_mut() = source;
        self.image.borrow_mut().take();
        self.serial.set(self.serial.get() + 1);
        self.render();
        self.field.queue_draw();
    }
    fn refresh(&self) {
        self.updating.set(true);
        let r = self.recipe.get();
        // Preserve older numeric recipes beyond the current soft brightness
        // range rather than displaying a silently clamped value.
        self.arcs[0].set_range((-4f64).min(r.exposure as f64), 4f64.max(r.exposure as f64));
        self.arcs[0].set_value(r.exposure as f64);
        self.arcs[1].set_value(r.highlight_color as f64);
        for (i, a) in self.arcs.iter().enumerate() {
            a.update_property(&[gtk::accessible::Property::ValueText(&if i == 0 {
                format!("{:+.0}% brightness", r.exposure * 25.)
            } else {
                format!("{:.0}% color intensity", r.highlight_color * 100.)
            })]);
            a.queue_draw();
        }
        let text = self.pad.get().map_or(
            "Saved custom appearance. Drag or reset to use Strength and Balance.".into(),
            |v| {
                format!(
                    "Strength {:.0} percent, balance {:+.0} percent; positive favors texture.",
                    v[1] * 100.,
                    v[0] * 100.
                )
            },
        );
        self.field
            .update_property(&[gtk::accessible::Property::Description(&text)]);
        self.field.queue_draw();
        self.root.queue_draw();
        self.updating.set(false);
    }
    fn emit(&self, phase: ContactPhase) {
        for f in self.changed.borrow().iter() {
            f(phase, self.recipe.get());
        }
    }
    fn begin(&self, part: usize) {
        if self.active.get().is_none() {
            self.before.set((self.recipe.get(), self.pad.get()));
            self.active.set(Some(part));
            self.emit(ContactPhase::Down);
        }
    }
    fn change(self: &Rc<Self>, recipe: SdrRendition, pad: Option<[f64; 2]>) {
        self.recipe.set(recipe);
        self.pad.set(pad);
        self.refresh();
        self.render();
        self.emit(ContactPhase::Move);
    }
    fn end(self: &Rc<Self>, cancel: bool) {
        if self.active.replace(None).is_some() {
            if cancel {
                let (r, p) = self.before.get();
                self.recipe.set(r);
                self.pad.set(p);
                self.refresh();
                self.render();
            }
            self.emit(if cancel {
                ContactPhase::Cancel
            } else {
                ContactPhase::Up
            });
        }
    }
    fn at(self: &Rc<Self>, part: usize, x: f64, y: f64) {
        if part == 0 {
            let size = self.field.width().min(self.field.height()) as f32;
            let g = ParameterDialGeometry::new(size).unwrap().field;
            let f = g.disc_components([x as f32, y as f32]);
            let v = layer_ui::proof_panel::sdr_tone_pad().values(f.map(f64::from));
            self.change(sdr_from_pad(self.recipe.get(), v), Some(v));
        } else {
            let arc = &self.arcs[part - 1];
            if let Some(g) = arc.geometry() {
                let a = arc.adjustment();
                let v =
                    a.lower() + g.fraction([x as f32, y as f32]) as f64 * (a.upper() - a.lower());
                arc.set_value((v / a.step_increment()).round() * a.step_increment());
            }
        }
    }
    fn wire(self: &Rc<Self>, part: usize, widget: &gtk::Widget) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |g, x, y| {
                if part == 0 {
                    let field =
                        ParameterDialGeometry::new(p.field.width().min(p.field.height()) as f32)
                            .unwrap()
                            .field;
                    if (x - field.center[0] as f64).hypot(y - field.center[1] as f64)
                        > field.disc_radius() as f64
                    {
                        g.set_state(gtk::EventSequenceState::Denied);
                        return;
                    }
                }
                g.set_state(gtk::EventSequenceState::Claimed);
                g.widget().unwrap().grab_focus();
                p.double.set(false);
                p.origin.set([x, y]);
                p.begin(part);
                p.at(part, x, y);
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_, x, y| {
                if p.active.get() == Some(part) && !p.double.get() {
                    let [a, b] = p.origin.get();
                    p.at(part, a + x, b + y);
                }
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_, _, _| p.end(false)
        ));
        drag.connect_cancel(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_, _| p.end(true)
        ));
        widget.add_controller(drag.clone());
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_, n, _, _| if n == 2 {
                p.end(true);
                p.double.set(true);
                p.begin(part);
                if part == 0 {
                    let v = layer_ui::proof_panel::sdr_tone_pad().defaults();
                    p.change(sdr_from_pad(p.recipe.get(), v), Some(v));
                } else {
                    p.arcs[part - 1].set_value(0.);
                }
                p.end(false);
            }
        ));
        widget.add_controller(click.clone());
        click.group_with(&drag);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to=p)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, mods| {
                if key == gdk::Key::Escape {
                    p.end(true);
                    return glib::Propagation::Stop;
                }
                let (axis, sign) = match key {
                    gdk::Key::Left => (0, -1.),
                    gdk::Key::Right => (0, 1.),
                    gdk::Key::Down => (1, -1.),
                    gdk::Key::Up => (1, 1.),
                    _ => return glib::Propagation::Proceed,
                };
                p.begin(part);
                let step = if mods.contains(gdk::ModifierType::SHIFT_MASK) {
                    10.
                } else {
                    1.
                };
                if part == 0 {
                    let spec = layer_ui::proof_panel::sdr_tone_pad();
                    let mut v = p.pad.get().unwrap_or(spec.defaults());
                    let a = &spec.axes[axis].numeric;
                    v[axis] = (v[axis] + sign * step * a.step).clamp(a.min, a.max);
                    p.change(sdr_from_pad(p.recipe.get(), v), Some(v));
                } else {
                    let a = &p.arcs[part - 1];
                    a.set_value(a.value() + sign * step * a.adjustment().step_increment());
                }
                glib::Propagation::Stop
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_, key, _, _| if matches!(
                key,
                gdk::Key::Left | gdk::Key::Right | gdk::Key::Up | gdk::Key::Down
            ) {
                p.end(false);
            }
        ));
        widget.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_| p.end(true)
        ));
        widget.add_controller(focus);
        widget.connect_unmap(glib::clone!(
            #[weak(rename_to=p)]
            self,
            move |_| p.end(true)
        ));
    }
    fn draw_field(&self, area: &gtk::DrawingArea, cr: &cairo::Context, w: i32, h: i32) {
        let g = ParameterDialGeometry::new(w.min(h) as f32).unwrap().field;
        let r = g.disc_radius() as f64;
        let center = g.center.map(f64::from);
        cr.arc(center[0], center[1], r, 0., std::f64::consts::TAU);
        let _ = cr.save();
        cr.clip();
        let dark = adw::StyleManager::default().is_dark();
        let base = if dark { 0.18 } else { 0.78 };
        cr.set_source_rgb(base, base, base);
        let _ = cr.paint();
        if let Some(image) = self.image.borrow().as_ref() {
            // A live central crop. Illumination used full document coordinates.
            let scale = (2. * r / image.width() as f64).max(2. * r / image.height() as f64);
            cr.translate(
                center[0] - image.width() as f64 * scale * 0.5,
                center[1] - image.height() as f64 * scale * 0.5,
            );
            cr.scale(scale, scale);
            if cr.set_source_surface(image, 0., 0.).is_ok() {
                let _ = cr.paint();
            }
        } else {
            let gradient = cairo::LinearGradient::new(0., 0., 2. * r, 2. * r);
            gradient.add_color_stop_rgb(0., 0.65, 0.65, 0.65);
            gradient.add_color_stop_rgb(1., 0.12, 0.12, 0.12);
            let _ = cr.set_source(&gradient);
            let _ = cr.paint();
        }
        let _ = cr.restore();
        if let Some(v) = self.pad.get() {
            let f = layer_ui::proof_panel::sdr_tone_pad().fractions(v);
            let point = g.disc_marker(f.map(|v| v as f32));
            let size = self.root.width().min(self.root.height()) as f32;
            let marker_radius = ParameterDialGeometry::new(size).unwrap().arcs[0].marker_radius;
            marker(cr, point, marker_radius, area.has_visible_focus());
        }
    }
    fn render(self: &Rc<Self>) {
        self.dirty.set(true);
        if self.rendering.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to=p)]
            self,
            async move {
                loop {
                    p.dirty.set(false);
                    let Some(source) = p.source.borrow().clone() else {
                        break;
                    };
                    let recipe = p.recipe.get();
                    let serial = p.serial.get();
                    let extent = source.image.extent;
                    let result = gtk::gio::spawn_blocking(move || source.render(recipe)).await;
                    if serial == p.serial.get() && recipe == p.recipe.get() {
                        if let Ok(bytes) = result {
                            if let Ok(image) = cairo::ImageSurface::create_for_data(
                                bytes,
                                cairo::Format::ARgb32,
                                extent[0] as i32,
                                extent[1] as i32,
                                extent[0] as i32 * 4,
                            ) {
                                *p.image.borrow_mut() = Some(image);
                                p.field.queue_draw();
                            }
                        }
                    }
                    if !p.dirty.get() {
                        break;
                    }
                    glib::timeout_future(std::time::Duration::from_millis(16)).await;
                }
                p.rendering.set(false);
            }
        ));
    }
}

fn marker(cr: &cairo::Context, p: [f32; 2], radius: f32, focus: bool) {
    cr.arc(
        p[0] as f64,
        p[1] as f64,
        radius as f64,
        0.,
        std::f64::consts::TAU,
    );
    cr.set_source_rgba(0., 0., 0., 0.65);
    cr.set_line_width(4.);
    let _ = cr.stroke_preserve();
    cr.set_source_rgb(1., 1., 1.);
    cr.set_line_width(2.);
    let _ = cr.stroke();
    if focus {
        cr.arc(
            p[0] as f64,
            p[1] as f64,
            radius as f64 + 3.,
            0.,
            std::f64::consts::TAU,
        );
        cr.set_source_rgba(1., 1., 1., 0.65);
        cr.set_line_width(1.);
        let _ = cr.stroke();
    }
}
fn curved_text(
    cr: &cairo::Context,
    widget: &gtk::Widget,
    text: &str,
    center: [f32; 2],
    radius: f64,
    angle: f64,
    reverse: bool,
    size: f32,
) {
    let ink = widget.color();
    cr.set_source_rgba(
        ink.red() as f64,
        ink.green() as f64,
        ink.blue() as f64,
        0.62,
    );
    cr.select_font_face(
        "Adwaita Sans",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cr.set_font_size((size as f64 * 0.044).clamp(9., 12.));
    let advance: f64 = text
        .chars()
        .map(|c| cr.text_extents(&c.to_string()).unwrap().x_advance())
        .sum();
    let mut cursor = -advance * 0.5;
    for c in text.chars() {
        let s = c.to_string();
        let w = cr.text_extents(&s).unwrap().x_advance();
        let a = angle.to_radians() + (cursor + w * 0.5) / radius * if reverse { -1. } else { 1. };
        let _ = cr.save();
        cr.translate(
            center[0] as f64 + radius * a.cos(),
            center[1] as f64 + radius * a.sin(),
        );
        cr.rotate(
            a + if reverse {
                -std::f64::consts::FRAC_PI_2
            } else {
                std::f64::consts::FRAC_PI_2
            },
        );
        cr.move_to(-w * 0.5, 0.);
        let _ = cr.show_text(&s);
        let _ = cr.restore();
        cursor += w;
    }
}
