//! Touch-first native numeric presentation; all numeric policy lives in layer-ui.
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::{NumericControl, NumericKind, NumericOperation};
use std::cell::{Cell, OnceCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct NumberControl {
        pub spec: OnceCell<NumericControl>,
        pub value: Cell<f64>,
        pub updating: Cell<bool>,
        pub entry: OnceCell<gtk::Entry>,
        pub display: OnceCell<gtk::Button>,
        pub stack: OnceCell<gtk::Stack>,
        pub slider: OnceCell<gtk::Scale>,
        pub spin: OnceCell<gtk::SpinButton>,
        pub steps: OnceCell<[gtk::Button; 2]>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for NumberControl {
        const NAME: &'static str = "CapyNumberControl";
        type Type = super::NumberControl;
        type ParentType = gtk::Box;
    }
    impl ObjectImpl for NumberControl {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::OnceLock<Vec<glib::subclass::Signal>> =
                std::sync::OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("value-changed").build()])
        }
    }
    impl WidgetImpl for NumberControl {}
    impl BoxImpl for NumberControl {}
}
glib::wrapper! {
    pub struct NumberControl(ObjectSubclass<imp::NumberControl>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
impl NumberControl {
    pub fn new(spec: NumericControl, title: &str, description: &str) -> Self {
        Self::build(spec, title, description, false)
    }
    /// One-line slider with only an editable value. Numeric policy is unchanged.
    pub fn inline(spec: NumericControl, title: &str) -> Self {
        Self::build(spec, title, "", true)
    }
    fn build(spec: NumericControl, title: &str, description: &str, inline: bool) -> Self {
        let control: Self = glib::Object::new();
        control.imp().spec.set(spec.clone()).unwrap();
        control.set_orientation(gtk::Orientation::Vertical);
        control.set_hexpand(true);
        control.add_css_class("number-control");
        if inline {
            control.add_css_class("number-inline");
            control.set_tooltip_text(Some(title));
        }
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);
        labels.set_hexpand(true);
        labels.set_valign(gtk::Align::Center);
        labels.add_css_class("number-labels");
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.0);
        label.set_valign(gtk::Align::Center);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_tooltip_text(Some(title));
        label.add_css_class("number-title");
        labels.append(&label);
        if !inline {
            header.append(&labels);
        }
        control.append(&header);
        if !description.is_empty() {
            let description = gtk::Label::new(Some(description));
            description.set_xalign(0.0);
            description.set_wrap(true);
            description.add_css_class("dim-label");
            description.add_css_class("subtitle");
            labels.append(&description);
        }
        if spec.kind == NumericKind::Number {
            let spin = gtk::SpinButton::with_range(
                spec.min * spec.scale,
                spec.max * spec.scale,
                spec.step * spec.scale,
            );
            spin.set_digits(spec.digits);
            spin.set_numeric(false);
            spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
            spin.set_width_chars(4);
            spin.set_valign(gtk::Align::Center);
            let input_spec = spec.clone();
            spin.connect_input(move |spin| {
                Some(
                    input_spec
                        .resolve(
                            0.0,
                            NumericOperation::Expression {
                                text: spin.text().into(),
                            },
                        )
                        .map(|v| v.value * input_spec.scale)
                        .map_err(|_| ()),
                )
            });
            spin.connect_value_changed(glib::clone!(
                #[weak]
                control,
                move |spin| {
                    if !control.imp().updating.get() {
                        control.apply(NumericOperation::Value {
                            value: spin.value() / control.spec().scale,
                        });
                    }
                }
            ));
            header.append(&spin);
            control.imp().spin.set(spin).unwrap();
        } else {
            let display = gtk::Button::new();
            let value_label = gtk::Label::new(None);
            value_label.set_xalign(1.0);
            display.set_child(Some(&value_label));
            display.add_css_class("number-value");
            display.add_css_class("flat");
            display.set_tooltip_text(Some(&format!("Edit {title}")));
            let entry = gtk::Entry::builder()
                .has_frame(false)
                .width_chars(3)
                .max_width_chars(10)
                .build();
            if inline {
                // Reserve the full formatted range, not the current value's
                // digit count. Expressions scroll inside this same footprint.
                let chars = [spec.min, spec.max]
                    .into_iter()
                    .filter_map(|v| spec.resolve(v, NumericOperation::Format).ok())
                    .map(|v| v.text.chars().count())
                    .max()
                    .unwrap_or(1) as i32;
                value_label.set_width_chars(chars);
                value_label.set_max_width_chars(chars);
                entry.set_width_chars(chars);
                entry.set_max_width_chars(chars);
            }
            gtk::prelude::EditableExt::set_alignment(&entry, 1.0);
            entry.add_css_class("number-entry");
            let stack = gtk::Stack::new();
            stack.set_hhomogeneous(inline);
            stack.set_vhomogeneous(false);
            stack.set_halign(gtk::Align::End);
            stack.set_valign(gtk::Align::Center);
            stack.add_named(&display, Some("value"));
            stack.add_named(&entry, Some("entry"));
            if inline {
                // GTK's width-chars uses average character width, which can be
                // narrower than digits. Measure the widest formatted value too.
                let reserve = gtk::Button::new();
                reserve.add_css_class("number-value");
                reserve.add_css_class("flat");
                let text = [spec.min, spec.max]
                    .into_iter()
                    .filter_map(|v| spec.resolve(v, NumericOperation::Format).ok())
                    .map(|v| v.text)
                    .max_by_key(|v| v.len())
                    .unwrap_or_default();
                reserve.set_label(
                    &text
                        .chars()
                        .map(|c| if c.is_ascii_digit() { '8' } else { c })
                        .collect::<String>(),
                );
                stack.add_named(&reserve, Some("measure"));
            }
            header.append(&stack);
            display.connect_clicked(glib::clone!(
                #[weak]
                control,
                move |_| {
                    let imp = control.imp();
                    let value = control
                        .spec()
                        .resolve(control.value(), NumericOperation::Format)
                        .unwrap();
                    let entry = imp.entry.get().unwrap();
                    if !control.has_css_class("number-inline") {
                        entry.set_width_chars(value.edit.chars().count().clamp(3, 10) as i32);
                    }
                    entry.set_text(&value.edit);
                    imp.stack.get().unwrap().set_visible_child_name("entry");
                    entry.grab_focus();
                    entry.select_region(0, -1);
                }
            ));
            entry.connect_activate(glib::clone!(
                #[weak]
                control,
                move |_| {
                    control.finish(false);
                }
            ));
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak]
                control,
                move |_| {
                    control.finish(false);
                }
            ));
            entry.add_controller(focus);
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(glib::clone!(
                #[weak]
                control,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, _| {
                    if key == gtk::gdk::Key::Escape {
                        control.finish(true);
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
            ));
            entry.add_controller(keys);
            control.imp().entry.set(entry).unwrap();
            control.imp().display.set(display).unwrap();
            control.imp().stack.set(stack).unwrap();
            let track = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            track.add_css_class("number-track");
            let minus = gtk::Button::from_icon_name("layer-minus-symbolic");
            let plus = gtk::Button::from_icon_name("layer-plus-symbolic");
            for (button, steps, verb) in [(&minus, -1.0, "Decrease"), (&plus, 1.0, "Increase")] {
                button.add_css_class("flat");
                button.add_css_class("number-step");
                button.set_tooltip_text(Some(&format!("{verb} {title}")));
                button.connect_clicked(glib::clone!(
                    #[weak]
                    control,
                    move |_| {
                        control.finish(false);
                        control.apply(NumericOperation::Step { steps });
                    }
                ));
            }
            let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
            slider.set_draw_value(false);
            slider.set_hexpand(true);
            slider.connect_value_changed(glib::clone!(
                #[weak]
                control,
                move |slider| {
                    if !control.imp().updating.get() {
                        control.apply(NumericOperation::Position {
                            position: slider.value(),
                        });
                    }
                }
            ));
            if inline {
                header.prepend(&slider);
            } else {
                track.append(&minus);
                track.append(&slider);
                track.append(&plus);
                control.append(&track);
                control.imp().steps.set([minus, plus]).unwrap();
            }
            control.imp().slider.set(slider).unwrap();
        }
        control.set_value(spec.min);
        control
    }
    fn spec(&self) -> &NumericControl {
        self.imp().spec.get().unwrap()
    }
    pub fn value(&self) -> f64 {
        self.imp().value.get()
    }
    /// Project model state without emitting a user edit back to the core.
    pub fn set_value(&self, value: f64) {
        let Ok(result) = self.spec().resolve(value, NumericOperation::Format) else {
            return;
        };
        let imp = self.imp();
        imp.updating.set(true);
        imp.value.set(value);
        if let Some(display) = imp.display.get() {
            display
                .child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .set_text(&result.text);
        }
        if let Some(slider) = imp.slider.get() {
            slider.set_value(result.fill);
        }
        if let Some(spin) = imp.spin.get() {
            spin.set_value(value * self.spec().scale);
        }
        if let Some([minus, plus]) = imp.steps.get() {
            minus.set_sensitive(value > self.spec().min);
            plus.set_sensitive(value < self.spec().max);
        }
        imp.updating.set(false);
    }
    pub fn connect_value_changed(&self, f: impl Fn(&Self) + 'static) {
        self.connect_closure(
            "value-changed",
            false,
            glib::closure_local!(move |s: Self| f(&s)),
        );
    }
    pub fn cancel_edit(&self) {
        self.finish(true);
    }
    fn apply(&self, op: NumericOperation) -> bool {
        match self.spec().resolve(self.value(), op) {
            Ok(v) => {
                self.remove_css_class("error");
                self.set_tooltip_text(None);
                // GtkRange can re-emit after the synchronous updating guard has
                // ended. Compare at the core's resolution: an f32 model echo is
                // not a new edit of the f64 widget's rounded value.
                let current = self
                    .spec()
                    .resolve(
                        self.value(),
                        NumericOperation::Value {
                            value: self.value(),
                        },
                    )
                    .unwrap()
                    .value;
                if v.value != current {
                    self.set_value(v.value);
                    self.emit_by_name::<()>("value-changed", &[]);
                }
                true
            }
            Err(e) => {
                self.add_css_class("error");
                self.set_tooltip_text(Some(&e));
                false
            }
        }
    }
    fn finish(&self, cancel: bool) {
        let Some(stack) = self.imp().stack.get() else {
            return;
        };
        if stack.visible_child_name().as_deref() != Some("entry") {
            return;
        }
        if cancel
            || self.apply(NumericOperation::Expression {
                text: self.imp().entry.get().unwrap().text().into(),
            })
        {
            // Switch first so focus-leave cannot commit twice.
            stack.set_visible_child_name("value");
            self.remove_css_class("error");
            self.set_tooltip_text(None);
        }
    }
}
