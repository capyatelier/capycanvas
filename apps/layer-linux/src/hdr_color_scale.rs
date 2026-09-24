//! Managed intensity arc. GtkRange retains keyboard and accessible range actions;
//! native capture gestures use the same angular geometry as the visible track.
use crate::display_color::{ViewColor, picker_texture_with_coverage};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::color::RgbColor;
use layer_ui::HdrIntensityArc;
use std::cell::{Cell, RefCell};

struct ArcPaths {
    key: (i32, f64, f64),
    zero: gtk::gsk::Path,
}
impl ArcPaths {
    fn new(key: (i32, f64, f64), g: &HdrIntensityArc) -> Self {
        Self { key, zero: Self::zero(key.1, key.2, g) }
    }
    fn zero(min: f64, max: f64, g: &HdrIntensityArc) -> gtk::gsk::Path {
        let point = g.point(((0. - min) / (max - min)) as f32);
        let dx = (point[0] - g.center[0]) / g.radius;
        let dy = (point[1] - g.center[1]) / g.radius;
        let zero = gtk::gsk::PathBuilder::new();
        zero.move_to(point[0] + dx * (g.width * 0.5 + 2.), point[1] + dy * (g.width * 0.5 + 2.));
        zero.line_to(point[0] + dx * (g.width * 0.5 + 5.), point[1] + dy * (g.width * 0.5 + 5.));
        zero.to_path()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ArcKey {
    width: i32,
    dpi: i32,
    min: f64,
    max: f64,
    color: (RgbColor, ViewColor, f32),
}
impl ArcKey {
    fn compatible(self, other: Self) -> bool {
        self.width == other.width && self.dpi == other.dpi
            && self.min == other.min && self.max == other.max
            && self.color.1 == other.color.1 && self.color.2 == other.color.2
    }
    fn bounds(self) -> gtk::graphene::Rect {
        let width = self.width as f32 / self.dpi as f32;
        let g = HdrIntensityArc::new(width).unwrap();
        let top = (g.center[1] + g.radius * 0.5 - g.width * 0.5 - 2.).floor();
        let height = ((g.center[1] + g.radius + g.width * 0.5 + 2. - top) * self.dpi as f32).ceil();
        gtk::graphene::Rect::new(0., top, width, height / self.dpi as f32)
    }
    fn render(self) -> gdk::Texture {
        let (base, view, headroom) = self.color;
        let bounds = self.bounds();
        let height = (bounds.height() * self.dpi as f32).round() as i32;
        let g = HdrIntensityArc::new(bounds.width()).unwrap();
        let linear = base.linear_in(base.space).expect("validated picker color");
        let mut coverage = Vec::with_capacity((self.width * height) as usize);
        let pixels: Vec<_> = (0..self.width * height).map(|i| {
            let point = [((i % self.width) as f32 + 0.5) / self.dpi as f32,
                bounds.y() + ((i / self.width) as f32 + 0.5) / self.dpi as f32];
            let fraction = g.fraction(point);
            let closest = g.point(fraction);
            let distance = (point[0] - closest[0]).hypot(point[1] - closest[1]);
            coverage.push(((g.width * 0.5 - distance) * self.dpi as f32 + 0.5).clamp(0., 1.));
            let gain = (self.min + f64::from(fraction) * (self.max - self.min)).exp2();
            [preview_gain(linear[0], gain), preview_gain(linear[1], gain), preview_gain(linear[2], gain), 1.]
        }).collect();
        // Bake native-DPI antialiasing into the worker result. A changing texture
        // under a GTK stroke mask requires additional GPU work on the UI thread.
        picker_texture_with_coverage(view, headroom, base.space, [self.width as u32, height as u32], &pixels, &coverage)
    }
}

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct HdrColorScale {
        pub color: RefCell<Option<(RgbColor, ViewColor, f32)>>,
        pub updating: Cell<bool>,
        pub reset: Cell<bool>,
        pub(crate) texture: RefCell<Option<(ArcKey, gdk::Texture)>>,
        pub preview: Cell<bool>,
        pub(super) raster: crate::color_preview_raster::PreviewRaster<ArcKey, ArcKey>,
        pub(super) paths: RefCell<Option<ArcPaths>>,
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
            let key = ArcKey { width: obj.width() * dpi, dpi, min, max, color: (base, view, headroom) };
            let bounds = key.bounds();
            let mut cache = self.texture.borrow_mut();
            let ready = cache.as_ref().is_some_and(|(k, _)| *k == key);
            if self.preview.get() {
                self.raster.request(&*obj, key, || (!ready).then_some(key), ArcKey::render,
                    |scale, key, texture| { *scale.imp().texture.borrow_mut() = Some((key, texture)); },
                    ArcKey::compatible);
            } else if !ready {
                *cache = Some((key, key.render()));
            }
            let mut paths = self.paths.borrow_mut();
            if paths.as_ref().is_none_or(|p| p.key.0 != obj.width()) {
                *paths = Some(ArcPaths::new((obj.width(), min, max), &g));
            } else if let Some(paths) = paths.as_mut() && paths.key != (obj.width(), min, max) {
                paths.key = (obj.width(), min, max);
                paths.zero = ArcPaths::zero(min, max, &g);
            }
            let paths = paths.as_ref().unwrap();
            if let Some((_, texture)) = cache.as_ref() { snapshot.append_texture(texture, &bounds); }
            let [cx, cy] = g.point(((obj.value() - min) / (max - min)) as f32);
            let radius = g.marker_radius;
            let thumb =
                gtk::graphene::Rect::new(cx - radius, cy - radius, radius * 2., radius * 2.);
            let mut p = base.linear_in(base.space).unwrap();
            for v in &mut p[..3] {
                *v = preview_gain(*v, obj.value().exp2());
            }
            p[3] = 1.;
            snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(thumb, radius));
            crate::display_color::append_picker_solid(snapshot, view, headroom, base.space, p, &thumb);
            snapshot.pop();
            crate::display_color::marker_outline(snapshot, [cx, cy], radius);
            let mut ink = obj.color();
            ink.set_alpha(0.9);
            if obj.has_visible_focus() {
                let r = radius + 5.;
                snapshot.append_border(&gtk::gsk::RoundedRect::from_rect(
                    gtk::graphene::Rect::new(cx-r, cy-r, r*2., r*2.), r), &[2.;4], &[ink;4]);
            }
            snapshot.append_stroke(&paths.zero, &gtk::gsk::Stroke::new(1.), &ink);
            // Read-only type follows the lower arc; all numeric editing is in the sheet.
            let font = (obj.width() as f64 * 0.044).clamp(9., 12.);
            let text = format!("{:+.2} EV", obj.value());
            let radius = (g.radius + g.width * 0.5 + 3.) as f64 + font;
            // Retained glyph nodes share GTK's font atlas. The curved label
            // must not rasterize/upload a new Cairo surface on every preview.
            let glyphs: Vec<_> = text.chars().map(|c| {
                let layout = crate::color_readout::layout(obj.upcast_ref(), font, false, &c.to_string());
                let width = crate::color_readout::advance(&layout);
                (layout, width)
            }).collect();
            let mut cursor = -glyphs.iter().map(|(_, width)| width).sum::<f64>() * 0.5;
            for (layout, width) in glyphs {
                let a = 76f64.to_radians() - (cursor + width * 0.5) / radius;
                crate::color_readout::append(snapshot, &layout, [
                    g.center[0] as f64 + radius * a.cos(), g.center[1] as f64 + radius * a.sin(),
                ], a.to_degrees() - 90., true, ink);
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
    pub fn refresh_color(&self, state: &layer_ui::ColorState, view: ViewColor, headroom: f32, preview: bool) {
        let changed = self.imp().preview.replace(preview) != preview;
        if changed || !preview { self.imp().raster.cancel(); }
        if changed { self.queue_draw(); }
        let color = (state.picker_base(), view, headroom);
        if self.imp().color.borrow().as_ref() != Some(&color) {
            *self.imp().color.borrow_mut() = Some(color);
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
