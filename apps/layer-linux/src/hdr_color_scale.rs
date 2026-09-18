//! Native range input/accessibility with a managed color ramp and wheel-style thumb.
use crate::display_color::{ViewColor, picker_texture};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::color::RgbColor;
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct HdrColorScale {
        pub color: RefCell<Option<(RgbColor, ViewColor, f32)>>,
        pub updating: Cell<bool>,
        pub texture: RefCell<Option<(i32, f64, f64, bool, gdk::Texture)>>,
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
            // GtkRange invalidates its private trough. Our custom snapshot
            // does not include that child, so invalidate our own render node.
            self.obj().queue_draw();
        }
    }
    impl ScaleImpl for HdrColorScale {}
    impl WidgetImpl for HdrColorScale {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some((base, view, headroom)) = *self.color.borrow() else {
                return;
            };
            let rect = obj.range_rect();
            let (start, end) = obj.slider_range();
            let radius = (end - start) as f32 * 0.5;
            if radius <= 0. || rect.width() <= 0 {
                return;
            }
            let x = rect.x() as f32 + radius;
            let width = (rect.width() as f32 - radius * 2.).max(1.);
            let cy = rect.y() as f32 + rect.height() as f32 * 0.5;
            let bounds =
                gtk::graphene::Rect::new(rect.x() as f32, cy - 10., rect.width() as f32, 20.);
            let adjustment = obj.adjustment();
            let (min, max) = (adjustment.lower(), adjustment.upper());
            let rtl = obj.direction() == gtk::TextDirection::Rtl;
            let physical = rect.width() * obj.scale_factor();
            let mut cache = self.texture.borrow_mut();
            if cache
                .as_ref()
                .is_none_or(|(w, a, b, r, _)| *w != physical || *a != min || *b != max || *r != rtl)
            {
                let base = base.linear_in(base.space).expect("validated picker color");
                let pixels: Vec<_> = (0..physical)
                    .map(|i| {
                        // End colors align with the native thumb centers, including its caps.
                        let t =
                            ((i as f32 / obj.scale_factor() as f32 - radius) / width).clamp(0., 1.);
                        let t = if rtl { 1. - t } else { t };
                        let gain = (min as f32 + t * (max - min) as f32).exp2();
                        [base[0] * gain, base[1] * gain, base[2] * gain, 1.]
                    })
                    .collect();
                *cache = Some((
                    physical,
                    min,
                    max,
                    rtl,
                    picker_texture(
                        view,
                        headroom,
                        self.color.borrow().unwrap().0.space,
                        [physical as u32, 1],
                        &pixels,
                    ),
                ));
            }
            snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(bounds, 10.));
            snapshot.append_texture(&cache.as_ref().unwrap().4, &bounds);
            snapshot.pop();
            // A small reference tick at 0 EV; no repeated labels or explanations.
            let zero = ((0. - min) / (max - min)) as f32;
            let zero = x + width * if rtl { 1. - zero } else { zero };
            let cx = (start + end) as f32 * 0.5;
            let thumb =
                gtk::graphene::Rect::new(cx - radius, cy - radius, radius * 2., radius * 2.);
            let mut p = base.linear_in(base.space).unwrap();
            for v in &mut p[..3] {
                *v *= (obj.value() as f32).exp2();
            }
            p[3] = 1.;
            snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(thumb, radius));
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
            cr.move_to(zero as f64, (cy + 11.) as f64);
            cr.line_to(zero as f64, (cy + 14.) as f64);
            let ink = obj.color();
            cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.7);
            cr.set_line_width(1.);
            let _ = cr.stroke();
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
            if obj.has_visible_focus() {
                cr.arc(
                    cx as f64,
                    cy as f64,
                    (radius + 4.) as f64,
                    0.,
                    std::f64::consts::TAU,
                );
                cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.9);
                cr.set_line_width(2.);
                let _ = cr.stroke();
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
        // The thumb is fully inside our ramp. GtkScale's fixed-size mode
        // assumes a CSS thumb overhanging each endpoint; GtkRange's ordinary
        // page-size-zero geometry matches this contained circular thumb.
        obj.set_slider_size_fixed(false);
        obj.set_increments(0.1, 1.);
        obj.set_round_digits(2);
        obj.set_has_origin(false);
        obj.set_hexpand(true);
        obj.add_css_class("hdr-color-scale");
        obj.set_widget_name("color-hdr-intensity-ramp");
        obj.update_property(&[gtk::accessible::Property::Label("HDR intensity"),
            gtk::accessible::Property::Description("Exposure in stops. Zero keeps the base color; each stop doubles linear brightness.")]);
        obj
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
        // Exact/sampled colors outside the convenient drag interval remain visible.
        self.set_range(
            (-2f64).min(stops.floor()),
            6f64.max(stops.ceil()).min(f64::from(65504f32.log2())),
        );
        self.set_value(stops);
        self.update_property(&[gtk::accessible::Property::ValueText(&format!(
            "{stops:+.2} EV"
        ))]);
        self.set_tooltip_text(Some(if headroom > 1. {
            "HDR intensity · 0 EV keeps the base color"
        } else {
            "HDR intensity · SDR display preview"
        }));
        self.imp().updating.set(false);
    }
    pub fn updating(&self) -> bool {
        self.imp().updating.get()
    }
}
