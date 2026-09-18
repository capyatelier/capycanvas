//! Reusable native presentation for a shared two-parameter definition.
use crate::{number_control::NumberControl, panel_controls};
use gtk::{gdk, glib, prelude::*};
use layer_ui::{ContactPhase, parameter_pad::ParameterPadSpec};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
type Changed = Box<dyn Fn(ContactPhase, [f64; 2])>;
pub(crate) struct ParameterPad {
    pub root: gtk::Box,
    pub area: gtk::DrawingArea,
    spec: ParameterPadSpec,
    values: Cell<[f64; 2]>,
    before: Cell<[f64; 2]>,
    active: Cell<bool>,
    drag_origin: Cell<[f64; 2]>,
    updating: Cell<bool>,
    numbers: [NumberControl; 2],
    changed: RefCell<Vec<Changed>>,
}
impl ParameterPad {
    pub fn new(name: &str, spec: ParameterPadSpec) -> Rc<Self> {
        let root = panel_controls::column();
        root.set_widget_name(name);
        let area = gtk::DrawingArea::builder()
            .content_width(120)
            .content_height(72)
            .hexpand(true)
            .focusable(true)
            .build();
        area.set_widget_name(&format!("{name}-surface"));
        area.update_property(&[gtk::accessible::Property::Label(&format!("{} and {}",spec.axes[0].label,spec.axes[1].label)),gtk::accessible::Property::Description("Drag to adjust both values. Arrow keys adjust one axis. Escape cancels. Double-click resets.")]);
        root.append(&area);
        let readouts = panel_controls::action_row();
        let numbers = spec.axes.each_ref().map(|axis| {
            let label = gtk::Label::new(Some(axis.label));
            readouts.append(&label);
            let number = NumberControl::value_only(axis.numeric.clone(), axis.label);
            number.set_widget_name(&format!("{name}-{}", axis.key));
            number.set_halign(gtk::Align::Fill);
            readouts.append(&number);
            number
        });
        root.append(&readouts);
        let p = Rc::new(Self {
            root,
            area: area.clone(),
            values: Cell::new(spec.defaults()),
            before: Cell::new(spec.defaults()),
            spec,
            active: Cell::new(false),
            drag_origin: Cell::new([0.; 2]),
            updating: Cell::new(false),
            numbers,
            changed: Default::default(),
        });
        area.set_draw_func(glib::clone!(
            #[weak]
            p,
            move |area, cr, w, h| {
                let fg = area.color();
                let accent = adw::StyleManager::default().accent_color_rgba();
                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.055);
                cr.rectangle(7., 7., w as f64 - 14., h as f64 - 14.);
                let _ = cr.fill();
                let defaults = p.spec.fractions(p.spec.defaults());
                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.20);
                cr.set_line_width(1.);
                cr.rectangle(7.5, 7.5, w as f64 - 15., h as f64 - 15.);
                cr.move_to(7. + defaults[0] * (w as f64 - 14.), 7.);
                cr.line_to(7. + defaults[0] * (w as f64 - 14.), h as f64 - 7.);
                cr.move_to(7., 7. + (1. - defaults[1]) * (h as f64 - 14.));
                cr.line_to(w as f64 - 7., 7. + (1. - defaults[1]) * (h as f64 - 14.));
                let _ = cr.stroke();
                let f = p.spec.fractions(p.values.get());
                let x = 7. + f[0] * (w as f64 - 14.);
                let y = 7. + (1. - f[1]) * (h as f64 - 14.);
                cr.set_source_rgba(
                    accent.red() as f64,
                    accent.green() as f64,
                    accent.blue() as f64,
                    1.,
                );
                cr.arc(x, y, 6., 0., std::f64::consts::TAU);
                let _ = cr.fill();
                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 1.);
                cr.set_line_width(1.5);
                cr.arc(x, y, 6., 0., std::f64::consts::TAU);
                let _ = cr.stroke();
                if area.has_focus() {
                    cr.set_source_rgba(
                        accent.red() as f64,
                        accent.green() as f64,
                        accent.blue() as f64,
                        0.7,
                    );
                    cr.rectangle(2.5, 2.5, w as f64 - 5., h as f64 - 5.);
                    let _ = cr.stroke();
                }
            }
        ));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            p,
            move |g, x, y| {
                p.drag_origin.set([x, y]);
                p.area.grab_focus();
                g.set_state(gtk::EventSequenceState::Claimed);
                p.begin();
                p.at(x, y);
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            p,
            move |_, x, y| {
                if p.active.get() {
                    let [sx, sy] = p.drag_origin.get();
                    p.at(sx + x, sy + y);
                }
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak]
            p,
            move |_, _, _| p.end(false)
        ));
        drag.connect_cancel(glib::clone!(
            #[weak]
            p,
            move |_, _| p.end(true)
        ));
        area.add_controller(drag.clone());
        let clicks = gtk::GestureClick::new();
        clicks.set_button(1);
        clicks.connect_pressed(glib::clone!(
            #[weak]
            p,
            move |_, count, _, _| if count == 2 {
                p.end(true);
                p.begin();
                p.change(p.spec.defaults());
                p.end(false);
            }
        ));
        area.add_controller(clicks.clone());
        clicks.group_with(&drag);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            p,
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
                p.begin();
                let mut values = p.values.get();
                let a = &p.spec.axes[axis].numeric;
                values[axis] = (values[axis]
                    + sign
                        * a.step
                        * if mods.contains(gdk::ModifierType::SHIFT_MASK) {
                            10.
                        } else {
                            1.
                        })
                .clamp(a.min, a.max);
                p.change(values);
                glib::Propagation::Stop
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak]
            p,
            move |_, key, _, _| if matches!(
                key,
                gdk::Key::Left | gdk::Key::Right | gdk::Key::Up | gdk::Key::Down
            ) {
                p.end(false);
            }
        ));
        area.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak]
            p,
            move |_| {
                p.end(true);
                p.area.queue_draw();
            }
        ));
        area.add_controller(focus);
        area.connect_unmap(glib::clone!(
            #[weak]
            p,
            move |_| p.end(true)
        ));
        for i in 0..2 {
            p.numbers[i].connect_value_changed(glib::clone!(
                #[weak]
                p,
                move |n| {
                    if !p.updating.get() {
                        let mut v = p.values.get();
                        v[i] = n.value();
                        let own = !p.active.get();
                        if own {
                            p.begin();
                        }
                        p.change(v);
                        if own {
                            p.end(false);
                        }
                    }
                }
            ));
            p.numbers[i].connect_interaction(glib::clone!(
                #[weak]
                p,
                move |_, phase| match phase {
                    ContactPhase::Down => p.begin(),
                    ContactPhase::Up => p.end(false),
                    ContactPhase::Cancel => p.end(true),
                    _ => {}
                }
            ));
        }
        p.set_values(p.spec.defaults());
        p
    }
    pub fn values(&self) -> [f64; 2] {
        self.values.get()
    }
    pub fn set_values(&self, v: [f64; 2]) {
        self.updating.set(true);
        self.values.set(v);
        for i in 0..2 {
            self.numbers[i].set_value(v[i]);
        }
        self.area.queue_draw();
        self.updating.set(false);
    }
    pub fn connect_changed(&self, f: impl Fn(ContactPhase, [f64; 2]) + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }
    fn emit(&self, phase: ContactPhase) {
        for f in self.changed.borrow().iter() {
            f(phase, self.values.get());
        }
    }
    fn begin(&self) {
        if !self.active.replace(true) {
            self.before.set(self.values.get());
            self.emit(ContactPhase::Down);
        }
    }
    fn change(&self, v: [f64; 2]) {
        self.set_values(v);
        self.emit(ContactPhase::Move);
    }
    fn end(&self, cancel: bool) {
        if self.active.replace(false) {
            if cancel {
                self.set_values(self.before.get());
            }
            self.emit(if cancel {
                ContactPhase::Cancel
            } else {
                ContactPhase::Up
            });
        }
    }
    fn at(&self, x: f64, y: f64) {
        let w = (self.area.width() - 14).max(1) as f64;
        let h = (self.area.height() - 14).max(1) as f64;
        self.change(self.spec.values([(x - 7.) / w, 1. - (y - 7.) / h]));
    }
}
