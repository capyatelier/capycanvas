//! Managed intensity arc. GtkRange retains keyboard and accessible range actions;
//! native capture gestures use the same angular geometry as the visible track.
use crate::display_color::{ViewColor, picker_texture};
use gtk::{cairo, gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::color::RgbColor;
use layer_ui::HdrIntensityArc;
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct HdrColorScale {
        pub color: RefCell<Option<(RgbColor, ViewColor, f32)>>,
        pub updating: Cell<bool>,
        pub reset: Cell<bool>,
        pub texture: RefCell<Option<(i32, i32, f64, f64, gdk::Texture)>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for HdrColorScale {
        const NAME: &'static str = "CapyHdrColorScale";
        type Type = super::HdrColorScale;
        type ParentType = gtk::Scale;
    }
    impl ObjectImpl for HdrColorScale {}
    impl RangeImpl for HdrColorScale {
        fn value_changed(&self) {
            self.parent_value_changed();
            self.obj().queue_draw();
        }
    }
    impl ScaleImpl for HdrColorScale {}
    impl WidgetImpl for HdrColorScale {
        fn contains(&self, x: f64, y: f64) -> bool {
            self.obj()
                .geometry()
                .is_some_and(|g| g.contains([x as f32, y as f32]))
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(g) = obj.geometry() else {
                return;
            };
            let Some((base, view, headroom)) = *self.color.borrow() else {
                return;
            };
            let min = obj.adjustment().lower();
            let max = obj.adjustment().upper();
            let dpi = obj.scale_factor();
            let width = obj.width() * dpi;
            let top = (g.center[1] + g.radius * 0.5 - g.width * 0.5 - 2.).floor();
            let height =
                ((g.center[1] + g.radius + g.width * 0.5 + 2. - top) * dpi as f32).ceil() as i32;
            let bounds =
                gtk::graphene::Rect::new(0., top, obj.width() as f32, height as f32 / dpi as f32);
            let mut cache = self.texture.borrow_mut();
            if cache
                .as_ref()
                .is_none_or(|(w, d, a, b, _)| *w != width || *d != dpi || *a != min || *b != max)
            {
                let linear = base.linear_in(base.space).expect("validated picker color");
                let pixels: Vec<_> = (0..width * height)
                    .map(|i| {
                        let point = [
                            (i % width) as f32 / dpi as f32,
                            top + (i / width) as f32 / dpi as f32,
                        ];
                        let gain = (min + f64::from(g.fraction(point)) * (max - min)).exp2();
                        [preview_gain(linear[0], gain), preview_gain(linear[1], gain), preview_gain(linear[2], gain), 1.]
                    })
                    .collect();
                *cache = Some((
                    width,
                    dpi,
                    min,
                    max,
                    picker_texture(
                        view,
                        headroom,
                        base.space,
                        [width as u32, height as u32],
                        &pixels,
                    ),
                ));
            }
            let path = gtk::gsk::PathBuilder::new();
            let start = g.point(0.);
            path.move_to(start[0], start[1]);
            for i in 1..=128 {
                let p = g.point(i as f32 / 128.);
                path.line_to(p[0], p[1]);
            }
            let stroke = gtk::gsk::Stroke::new(g.width);
            stroke.set_line_cap(gtk::gsk::LineCap::Round);
            snapshot.push_stroke(&path.to_path(), &stroke);
            snapshot.append_texture(&cache.as_ref().unwrap().4, &bounds);
            snapshot.pop();
            let [cx, cy] = g.point(((obj.value() - min) / (max - min)) as f32);
            let radius = g.marker_radius;
            let thumb =
                gtk::graphene::Rect::new(cx - radius, cy - radius, radius * 2., radius * 2.);
            let mut p = base.linear_in(base.space).unwrap();
            for v in &mut p[..3] {
                *v = preview_gain(*v, obj.value().exp2());
            }
            p[3] = 1.;
            let disc = gtk::gsk::PathBuilder::new();
            disc.add_circle(&gtk::graphene::Point::new(cx, cy), radius);
            snapshot.push_fill(&disc.to_path(), gtk::gsk::FillRule::Winding);
            snapshot.append_texture(
                &picker_texture(view, headroom, base.space, [1, 1], &[p]),
                &thumb,
            );
            snapshot.pop();
            let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(
                0.,
                0.,
                obj.width() as f32,
                obj.height() as f32,
            ));
            cr.arc(
                cx as f64,
                cy as f64,
                radius as f64,
                0.,
                std::f64::consts::TAU,
            );
            cr.set_source_rgba(0., 0., 0., 0.5);
            cr.set_line_width(4.);
            let _ = cr.stroke_preserve();
            cr.set_source_rgb(1., 1., 1.);
            cr.set_line_width(2.);
            let _ = cr.stroke();
            let ink = obj.color();
            cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.9);
            if obj.has_visible_focus() {
                cr.arc(
                    cx as f64,
                    cy as f64,
                    (radius + 4.) as f64,
                    0.,
                    std::f64::consts::TAU,
                );
                let _ = cr.stroke();
            }
            let radius = g.width * 0.5;
            let zero = g.point(((0. - min) / (max - min)) as f32);
            let dx = (zero[0] - g.center[0]) / g.radius;
            let dy = (zero[1] - g.center[1]) / g.radius;
            cr.move_to(
                (zero[0] + dx * (radius + 2.)) as f64,
                (zero[1] + dy * (radius + 2.)) as f64,
            );
            cr.line_to(
                (zero[0] + dx * (radius + 5.)) as f64,
                (zero[1] + dy * (radius + 5.)) as f64,
            );
            cr.set_line_width(1.);
            let _ = cr.stroke();
            // Read-only type follows the lower arc; all numeric editing is in the sheet.
            let font = (obj.width() as f64 * 0.044).clamp(9., 12.);
            cr.select_font_face(
                "Adwaita Sans",
                cairo::FontSlant::Normal,
                cairo::FontWeight::Normal,
            );
            cr.set_font_size(font);
            let text = format!("{:+.2} EV", obj.value());
            let radius = (g.radius + g.width * 0.5 + 3.) as f64 + font;
            let advance: f64 = text
                .chars()
                .map(|c| cr.text_extents(&c.to_string()).unwrap().x_advance())
                .sum();
            let mut cursor = -advance * 0.5;
            for c in text.chars() {
                let text = c.to_string();
                let width = cr.text_extents(&text).unwrap().x_advance();
                let a = 76f64.to_radians() - (cursor + width * 0.5) / radius;
                let _ = cr.save();
                cr.translate(
                    g.center[0] as f64 + radius * a.cos(),
                    g.center[1] as f64 + radius * a.sin(),
                );
                cr.rotate(a - std::f64::consts::FRAC_PI_2);
                cr.move_to(-width * 0.5, 0.);
                let _ = cr.show_text(&text);
                let _ = cr.restore();
                cursor += width;
            }
        }
    }
}
glib::wrapper! {
    pub struct HdrColorScale(ObjectSubclass<imp::HdrColorScale>)
        @extends gtk::Widget, gtk::Range, gtk::Scale,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
impl HdrColorScale {
    pub fn new() -> Self {
        let obj: Self = glib::Object::builder()
            .property("orientation", gtk::Orientation::Horizontal)
            .property("draw-value", false)
            .build();
        obj.set_range(-2., 6.);
        obj.set_increments(0.1, 1.);
        obj.set_round_digits(2);
        obj.set_has_origin(false);
        obj.add_css_class("hdr-color-scale");
        obj.set_widget_name("color-hdr-intensity-ramp");
        obj.update_property(&[gtk::accessible::Property::Label("Color intensity"), gtk::accessible::Property::Description("Exposure in stops. Double-click to reset to 1× (0 EV). Use Edit Color for numeric entry.")]);
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak]
            obj,
            move |g, count, x, y| {
                g.set_state(gtk::EventSequenceState::Claimed);
                obj.grab_focus();
                obj.imp().reset.set(count == 2);
                if count == 2 {
                    obj.set_value(0.);
                } else {
                    obj.pick([x as f32, y as f32]);
                }
            }
        ));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(|g, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
        });
        drag.connect_drag_update(glib::clone!(
            #[weak]
            obj,
            move |g, dx, dy| {
                if !obj.imp().reset.get()
                    && let Some((x, y)) = g.start_point()
                {
                    obj.pick([(x + dx) as f32, (y + dy) as f32]);
                }
            }
        ));
        obj.add_controller(click.clone());
        obj.add_controller(drag.clone());
        drag.group_with(&click);
        obj
    }
    pub(crate) fn geometry(&self) -> Option<HdrIntensityArc> {
        HdrIntensityArc::new(self.width() as f32)
    }
    fn pick(&self, point: [f32; 2]) {
        if let Some(g) = self.geometry() {
            let a = self.adjustment();
            self.set_value(
                ((a.lower() + g.fraction(point) as f64 * (a.upper() - a.lower())) * 100.).round()
                    / 100.,
            );
        }
    }
    pub fn refresh(&self, state: &layer_ui::ColorState, view: ViewColor, headroom: f32) {
        let color = (state.picker_base(), view, headroom);
        if self.imp().color.borrow().as_ref() != Some(&color) {
            *self.imp().color.borrow_mut() = Some(color);
            self.imp().texture.borrow_mut().take();
            self.queue_draw();
        }
        self.imp().updating.set(true);
        let stops = state.hdr_intensity() as f64;
        self.set_range(
            (-2f64).min(stops.floor()),
            6f64.max(stops.ceil()).min(f64::from(state.hdr_depth().max_linear().log2())),
        );
        self.set_value(stops);
        self.update_property(&[gtk::accessible::Property::ValueText(&format!(
            "{stops:+.2} EV"
        ))]);
        self.set_tooltip_text(Some("Color intensity · Double-click to reset to 1× (0 EV)"));
        self.imp().updating.set(false);
    }
    pub fn updating(&self) -> bool {
        self.imp().updating.get()
    }
}

// Preview saturation never changes the stored paint definition.
fn preview_gain(value: f32, gain: f64) -> f32 {
    (f64::from(value) * gain).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
}
