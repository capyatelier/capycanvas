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
        pub interacting: Cell<bool>,
        pub compact: Cell<bool>,
        pub value_label: OnceCell<gtk::Label>,
        pub face: OnceCell<gtk::Box>,
        pub icon: OnceCell<gtk::Image>,
        pub title: OnceCell<gtk::Label>,
        pub interaction_end_pending: Cell<bool>,
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
            SIGNALS.get_or_init(|| {
                vec![
                    glib::subclass::Signal::builder("value-changed").build(),
                    glib::subclass::Signal::builder("interaction")
                        .param_types([u32::static_type()])
                        .build(),
                ]
            })
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
        Self::build(spec, title, description, false, false)
    }
    /// One-line slider with only an editable value. Numeric policy is unchanged.
    pub fn inline(spec: NumericControl, title: &str) -> Self {
        Self::build(spec, title, "", true, false)
    }
    /// Compact editable value for grouped components (e.g. a color wheel).
    pub fn value_only(spec: NumericControl, title: &str) -> Self {
        let control = Self::inline(spec, title);
        control.set_halign(gtk::Align::Center);
        if let Some(slider) = control.imp().slider.get() {
            slider.set_visible(false);
        }
        control
    }
    /// Compact toolbar presentation; units remain in the tooltip and accessible value.
    pub fn compact(spec: NumericControl, title: &str) -> Self {
        Self::build(spec, title, "", true, true)
    }
    pub fn value_button(&self) -> gtk::Button {
        self.imp().display.get().unwrap().clone()
    }
    pub fn set_slider_visible(&self, visible: bool) {
        if let Some(slider) = self.imp().slider.get() {
            slider.set_visible(visible);
        }
        if let Some(stack) = self.imp().stack.get() {
            stack.set_hexpand(!visible);
        }
    }
    pub fn set_icon(&self, icon: &str) {
        if let Some(image) = self.imp().icon.get() {
            crate::icons::set(image, Some(&format!("layer-{icon}-symbolic")));
        }
    }
    pub fn set_face(&self, show_icon: bool, title: &str, stacked: bool) {
        let imp = self.imp();
        if let Some(image) = imp.icon.get() {
            image.set_visible(show_icon);
        }
        if let Some(label) = imp.title.get() {
            label.set_text(title);
            label.set_visible(!title.is_empty());
        }
        if let Some(face) = imp.face.get() {
            face.set_orientation(if stacked {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            });
        }
        if let Some(stack) = imp.stack.get() {
            stack.set_halign(gtk::Align::Fill);
            stack.set_valign(gtk::Align::Fill);
        }
    }
    fn build(
        spec: NumericControl,
        title: &str,
        description: &str,
        inline: bool,
        compact: bool,
    ) -> Self {
        let control: Self = glib::Object::new();
        control.imp().spec.set(spec.clone()).unwrap();
        control.imp().compact.set(compact);
        if compact {
            control.add_css_class("number-compact");
        }
        control.set_orientation(gtk::Orientation::Vertical);
        control.set_hexpand(true);
        control.add_css_class("number-control");
        if inline {
            control.add_css_class("number-inline");
            control.set_tooltip_text(Some(title));
        }
        let header = gtk::Box::new(gtk::Orientation::Horizontal, if compact { 2 } else { 6 });
        header.set_vexpand(compact);
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
        if spec.kind == NumericKind::Number && !compact {
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
            value_label.set_xalign(if compact { 0.5 } else { 1.0 });
            if compact {
                let face = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                face.set_halign(gtk::Align::Center);
                let icon = crate::icons::image("layer-settings-symbolic");
                icon.set_visible(false);
                icon.set_pixel_size(14);
                let title_label = gtk::Label::new(None);
                title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                title_label.set_visible(false);
                face.append(&icon);
                let caption = gtk::Box::new(gtk::Orientation::Vertical, 0);
                caption.append(&title_label);
                caption.append(&value_label);
                face.append(&caption);
                display.set_child(Some(&face));
                control.imp().face.set(face).unwrap();
                control.imp().icon.set(icon).unwrap();
                control.imp().title.set(title_label).unwrap();
            } else {
                display.set_child(Some(&value_label));
            }
            control.imp().value_label.set(value_label.clone()).unwrap();
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
                let chars = [
                    spec.min,
                    spec.max,
                    (99.9 / spec.scale).clamp(spec.min, spec.max),
                ]
                .into_iter()
                .filter_map(|v| spec.resolve(v, NumericOperation::Format).ok())
                .map(|v| {
                    if compact {
                        spec.compact_text(v.value).chars().count()
                    } else {
                        v.text.chars().count()
                    }
                })
                .max()
                .unwrap_or(1) as i32;
                value_label.set_width_chars(if compact { 0 } else { chars });
                value_label.set_max_width_chars(if compact { -1 } else { chars });
                entry.set_width_chars(chars);
                entry.set_max_width_chars(chars);
            }
            gtk::prelude::EditableExt::set_alignment(&entry, 1.0);
            entry.add_css_class("number-entry");
            let stack = gtk::Stack::new();
            stack.set_hhomogeneous(inline && !compact);
            stack.set_vhomogeneous(false);
            stack.set_halign(gtk::Align::End);
            stack.set_valign(gtk::Align::Center);
            stack.add_named(&display, Some("value"));
            stack.add_named(&entry, Some("entry"));
            if inline && !compact {
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
            control.install_value_gestures(&display);
            control.imp().display.set(display).unwrap();
            control.imp().stack.set(stack).unwrap();
            let track = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            track.add_css_class("number-track");
            let minus = crate::icons::button("layer-minus-symbolic");
            let plus = crate::icons::button("layer-plus-symbolic");
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
        // Observe native input without claiming the sequence from GtkRange or
        // GtkSpinButton. Hosts may coalesce the resulting value changes into a
        // single undo step; the widget still owns native dragging/repetition.
        let events = gtk::EventControllerLegacy::new();
        events.set_propagation_phase(gtk::PropagationPhase::Capture);
        events.connect_event(glib::clone!(
            #[weak]
            control,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                use gtk::gdk::EventType as E;
                match event.event_type() {
                    E::ButtonPress
                        if event
                            .downcast_ref::<gtk::gdk::ButtonEvent>()
                            .is_some_and(|e| e.button() == 1) =>
                    {
                        control.begin_interaction()
                    }
                    E::TouchBegin => control.begin_interaction(),
                    E::ButtonRelease
                        if event
                            .downcast_ref::<gtk::gdk::ButtonEvent>()
                            .is_some_and(|e| e.button() == 1) =>
                    {
                        control.defer_interaction_end()
                    }
                    E::TouchEnd => control.defer_interaction_end(),
                    E::TouchCancel => control.end_interaction(true),
                    E::KeyPress => {
                        if let Some(e) = event.downcast_ref::<gtk::gdk::KeyEvent>() {
                            match e.keyval() {
                                gtk::gdk::Key::Escape => control.end_interaction(true),
                                gtk::gdk::Key::Left
                                | gtk::gdk::Key::Right
                                | gtk::gdk::Key::Up
                                | gtk::gdk::Key::Down
                                | gtk::gdk::Key::Page_Up
                                | gtk::gdk::Key::Page_Down => control.begin_interaction(),
                                _ => (),
                            }
                        }
                    }
                    E::KeyRelease => {
                        if event.downcast_ref::<gtk::gdk::KeyEvent>().is_some_and(|e| {
                            matches!(
                                e.keyval(),
                                gtk::gdk::Key::Left
                                    | gtk::gdk::Key::Right
                                    | gtk::gdk::Key::Up
                                    | gtk::gdk::Key::Down
                                    | gtk::gdk::Key::Page_Up
                                    | gtk::gdk::Key::Page_Down
                            )
                        }) {
                            control.defer_interaction_end();
                        }
                    }
                    _ => (),
                }
                glib::Propagation::Proceed
            }
        ));
        control.add_controller(events);
        control.connect_unmap(|control| control.end_interaction(true));
        control
    }
    fn install_value_gestures(&self, display: &gtk::Button) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let origin = std::rc::Rc::new(Cell::new(0.));
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to=control)]
            self,
            #[strong]
            origin,
            move |g, _, _| {
                if !crate::input::touch_or_pen(g) {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                origin.set(control.value());
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to=control)]
            self,
            #[strong]
            origin,
            move |g, dx, dy| {
                if !control.drag_check_threshold(0, 0, dx as i32, dy as i32) {
                    return;
                }
                g.set_state(gtk::EventSequenceState::Claimed);
                if let Ok(value) = control
                    .spec()
                    .resolve(origin.get(), NumericOperation::Step { steps: -dy / 4. })
                {
                    control.apply(NumericOperation::Value { value: value.value });
                }
            }
        ));
        display.add_controller(drag);
        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to=control)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _, dy| {
                control.begin_interaction();
                control.apply(NumericOperation::Step { steps: -dy });
                control.defer_interaction_end();
                glib::Propagation::Stop
            }
        ));
        display.add_controller(scroll);
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
        if let Some(label) = imp.value_label.get() {
            label.set_text(&if imp.compact.get() {
                self.spec().compact_text(value)
            } else {
                result.text.clone()
            });
            imp.display
                .get()
                .unwrap()
                .update_property(&[gtk::accessible::Property::ValueText(&result.text)]);
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
    pub fn is_interacting(&self) -> bool {
        self.imp().interacting.get()
    }
    pub fn connect_interaction(&self, f: impl Fn(&Self, layer_ui::ContactPhase) + 'static) {
        self.connect_closure(
            "interaction",
            false,
            glib::closure_local!(move |s: Self, phase: u32| {
                f(
                    &s,
                    match phase {
                        0 => layer_ui::ContactPhase::Down,
                        1 => layer_ui::ContactPhase::Up,
                        _ => layer_ui::ContactPhase::Cancel,
                    },
                );
            }),
        );
    }
    fn defer_interaction_end(&self) {
        self.imp().interaction_end_pending.set(true);
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to=control)]
            self,
            move || if control.imp().interaction_end_pending.replace(false) {
                control.end_interaction(false);
            }
        ));
    }
    fn begin_interaction(&self) {
        if self.imp().interaction_end_pending.replace(false) {
            self.end_interaction(false);
        }
        if !self.imp().interacting.replace(true) {
            self.emit_by_name::<()>("interaction", &[&0u32]);
        }
    }
    fn end_interaction(&self, cancel: bool) {
        self.imp().interaction_end_pending.set(false);
        if self.imp().interacting.replace(false) {
            self.emit_by_name::<()>("interaction", &[&if cancel { 2u32 } else { 1u32 }]);
        }
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
