//! One interval track with two native GtkRange handles and existing numeric editors.
//! GTK owns capture/focus; shared numeric controls and tool actions own values.
use crate::number_control::NumberControl;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_ui::{NumericControl, NumericOperation, ToolSetting};
use std::{cell::{Cell, RefCell}, rc::Rc};

const INSET: f64 = 8.;

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct RangeHandle {
        pub domain: Cell<[f64; 2]>,
        pub other: RefCell<Option<gtk::Adjustment>>,
        pub lower: Cell<bool>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for RangeHandle {
        const NAME: &'static str = "CapyRangeHandle";
        type Type = super::RangeHandle;
        type ParentType = gtk::Scale;
    }
    impl ObjectImpl for RangeHandle {}
    impl RangeImpl for RangeHandle {
        fn value_changed(&self) { self.parent_value_changed(); self.obj().queue_draw(); }
    }
    impl ScaleImpl for RangeHandle {}
    impl WidgetImpl for RangeHandle {
        fn measure(&self, axis: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            if axis == gtk::Orientation::Horizontal { (64, 100, -1, -1) }
            else { (28, 28, -1, -1) }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let x = obj.position(obj.value()) as f32;
            let y = obj.height() as f32 / 2.;
            let mut ink = obj.color();
            if self.lower.get() {
                ink.set_alpha(0.2);
                snapshot.append_color(&ink, &gtk::graphene::Rect::new(INSET as f32, y - 2., (obj.width() as f32 - 2. * INSET as f32).max(0.), 4.));
                if let Some(other) = self.other.borrow().as_ref() {
                    ink.set_alpha(0.65);
                    snapshot.append_color(&ink, &gtk::graphene::Rect::new(x, y - 2., (obj.position(other.value()) as f32 - x).max(0.), 4.));
                }
            }
            ink.set_alpha(if obj.is_sensitive() { 1. } else { 0.4 });
            // Facing half-handles stay distinguishable even at a zero-width range.
            let left = if self.lower.get() { x - 7. } else { x + 1. };
            let rect = gtk::gsk::RoundedRect::from_rect(gtk::graphene::Rect::new(left, y - 7., 6., 14.), 2.);
            snapshot.push_rounded_clip(&rect);
            snapshot.append_color(&ink, rect.bounds());
            snapshot.pop();
            if obj.has_visible_focus() {
                snapshot.append_border(&gtk::gsk::RoundedRect::from_rect(gtk::graphene::Rect::new(left - 3., y - 10., 12., 20.), 4.), &[1.;4], &[ink;4]);
            }
        }
    }
}
glib::wrapper! {
    pub struct RangeHandle(ObjectSubclass<imp::RangeHandle>)
        @extends gtk::Widget, gtk::Range, gtk::Scale,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
impl RangeHandle {
    fn new(field: &ToolSetting, lower: bool) -> Self {
        let scale: Self = glib::Object::builder()
            .property("orientation", gtk::Orientation::Horizontal)
            .property("draw-value", false).build();
        scale.set_adjustment(&gtk::Adjustment::new(field.value as f64, field.numeric.min, field.numeric.max, field.numeric.step, field.numeric.step * 10., 0.));
        scale.set_hexpand(true);
        scale.add_css_class("interval-handle");
        scale.set_focusable(true);
        scale.set_tooltip_text(Some(field.tooltip()));
        scale.update_property(&[gtk::accessible::Property::Label(field.tooltip())]);
        scale.set_widget_name(&format!("range-handle-{}", field.id));
        scale.imp().lower.set(lower);
        scale
    }
    pub fn position(&self, value: f64) -> f64 {
        let [min, max] = self.imp().domain.get();
        INSET + ((value - min) / (max - min)).clamp(0., 1.) * (self.width() as f64 - 2. * INSET).max(1.)
    }
}

#[derive(Clone)]
struct Contact { index: usize, start: f64, offset: f64, before: f64, spec: NumericControl }

pub struct RangeControl {
    pub root: gtk::Box,
    pub inputs: [NumberControl; 2],
    handles: [RangeHandle; 2],
    track: gtk::Overlay,
    spec: NumericControl,
    updating: Cell<bool>,
    contact: RefCell<Option<Contact>>,
}
impl RangeControl {
    pub fn new(id: &str, tooltip: &str, fields: [&ToolSetting; 2], changed: impl Fn(usize, f64) + 'static) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        root.set_widget_name(&format!("tool-range-{id}"));
        root.set_tooltip_text(Some(tooltip));
        let inputs = fields.map(|f| {
            let number = NumberControl::value_only(f.numeric.clone(), f.tooltip());
            number.set_hexpand(false);
            number.set_widget_name(&format!("tool-setting-{}", f.id));
            number
        });
        let handles = [RangeHandle::new(fields[0], true), RangeHandle::new(fields[1], false)];
        handles[0].imp().other.replace(Some(handles[1].adjustment()));
        let track = gtk::Overlay::new();
        track.set_widget_name(&format!("range-track-{id}"));
        track.set_tooltip_text(Some(tooltip));
        track.set_hexpand(true);
        track.set_child(Some(&handles[0]));
        track.add_overlay(&handles[1]);
        root.append(&inputs[0]); root.append(&track); root.append(&inputs[1]);
        let control = Rc::new(Self { root, inputs, handles, track, spec: fields[0].numeric.clone(), updating: Cell::new(false), contact: RefCell::new(None) });
        let changed = Rc::new(changed);
        for i in 0..2 {
            for source in 0..2 {
                let changed = changed.clone();
                let weak = Rc::downgrade(&control);
                let callback = move |value| {
                    if let Some(c) = weak.upgrade() && !c.updating.get() {
                        if let Ok(resolved) = c.spec.resolve(value, NumericOperation::Value { value }) {
                            changed(i, resolved.value);
                        }
                    }
                };
                if source == 0 { control.inputs[i].connect_value_changed(move |n| callback(n.value())); }
                else { control.handles[i].connect_value_changed(move |s| callback(s.value())); }
            }
        }
        control.wire();
        control.set_values(fields.map(|f| f.value as f64));
        control
    }
    pub fn set_values(&self, values: [f64; 2]) {
        self.updating.set(true);
        // Keep the track's scale steady throughout capture; typed extreme values
        // expand the visible domain once the gesture has finished.
        let domain = self.contact.borrow().as_ref().map(|c| [c.spec.soft_min, c.spec.soft_max])
            .unwrap_or([self.spec.soft_min.min(values[0]), self.spec.soft_max.max(values[1])]);
        for i in 0..2 {
            self.inputs[i].set_value(values[i]);
            self.handles[i].imp().domain.set(domain);
            self.handles[i].adjustment().configure(values[i], if i == 0 { self.spec.min } else { values[0] }, if i == 0 { values[1] } else { self.spec.max }, self.spec.step, self.spec.step * 10., 0.);
            self.handles[i].queue_draw();
        }
        self.updating.set(false);
    }
    pub fn retire(&self) {
        self.updating.set(true);
        self.contact.borrow_mut().take();
        for input in &self.inputs { input.cancel_edit(); }
    }
    pub fn set_slider_visible(&self, visible: bool) {
        self.track.set_visible(visible);
    }
    fn move_to(&self, x: f64) {
        let Some(c) = self.contact.borrow().clone() else { return; };
        let position = (x - c.offset - INSET) / (self.track.width() as f64 - 2. * INSET).max(1.);
        if let Ok(v) = c.spec.resolve(c.before, NumericOperation::Position { position }) {
            self.handles[c.index].set_value(v.value);
        }
    }
    fn end(&self, cancel: bool) {
        let contact = self.contact.borrow_mut().take();
        if let Some(c) = contact {
            if cancel { self.handles[c.index].set_value(c.before); }
            self.set_values(self.handles.each_ref().map(|s| s.value()));
        }
    }
    fn wire(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(glib::clone!(#[weak(rename_to=c)] self, move |g, x, _| {
            if c.contact.borrow().is_some() { g.set_state(gtk::EventSequenceState::Denied); return; }
            let positions = c.handles.each_ref().map(|s| s.position(s.value()));
            let index = if (positions[1] - positions[0]).abs() < 1. { usize::from(x >= positions[0]) }
                else { usize::from((x - positions[1]).abs() < (x - positions[0]).abs()) };
            let offset = if (x - positions[index]).abs() <= 12. { x - positions[index] } else { 0. };
            let mut spec = c.spec.clone();
            [spec.soft_min, spec.soft_max] = c.handles[index].imp().domain.get();
            c.contact.replace(Some(Contact { index, start: x, offset, before: c.handles[index].value(), spec }));
            g.set_state(gtk::EventSequenceState::Claimed);
            c.handles[index].grab_focus();
            c.move_to(x);
        }));
        drag.connect_drag_update(glib::clone!(#[weak(rename_to=c)] self, move |_, dx, _| {
            let start = c.contact.borrow().as_ref().map(|v| v.start);
            if let Some(start) = start { c.move_to(start + dx); }
        }));
        drag.connect_drag_end(glib::clone!(#[weak(rename_to=c)] self, move |_, _, _| c.end(false)));
        drag.connect_cancel(glib::clone!(#[weak(rename_to=c)] self, move |_, _| c.end(true)));
        self.track.add_controller(drag.clone());
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(#[weak(rename_to=c)] self, #[weak] drag, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, _| {
            if key == gdk::Key::Escape && c.contact.borrow().is_some() {
                c.end(true); drag.reset(); glib::Propagation::Stop
            } else { glib::Propagation::Proceed }
        }));
        self.track.add_controller(keys);
    }
}
