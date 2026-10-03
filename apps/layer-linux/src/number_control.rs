//! Touch-first native numeric presentation; all numeric policy lives in layer-ui.
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::{NumericControl, NumericKind, NumericOperation};
use std::cell::{Cell, OnceCell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct NumberControl {
        pub spec: OnceCell<NumericControl>,
        pub localization: RefCell<Option<std::sync::Arc<layer_ui::Localizer>>>,
        pub value: Cell<f64>,
        pub updating: Cell<bool>,
        pub composing: Cell<bool>,
        pub(super) composition_keys: crate::input::CompositionKeys,
        pub input_valid: Cell<bool>,
        pub editing: Cell<bool>,
        pub entry: OnceCell<gtk::Entry>,
        pub display: OnceCell<gtk::Button>,
        pub stack: OnceCell<gtk::Stack>,
        pub slider: OnceCell<gtk::Scale>,
        pub spin: OnceCell<gtk::SpinButton>,
        pub steps: OnceCell<[gtk::Button; 2]>,
        pub compact: Cell<bool>,
        pub readout_scale: Cell<f64>,
        pub value_label: OnceCell<gtk::Label>,
        pub face: OnceCell<gtk::Box>,
        pub icon: OnceCell<gtk::Image>,
        pub title: OnceCell<gtk::Label>,
        pub unit: OnceCell<gtk::Label>,
        pub icon_row: OnceCell<gtk::Box>,
        pub value_row: OnceCell<gtk::Box>,
        pub separate_unit: Cell<bool>,
        pub show_units: Cell<bool>,
        pub popover_enabled: Cell<bool>,
        pub editor_title: RefCell<String>,
        pub caption: OnceCell<gtk::Label>,
        pub inline_caption: OnceCell<gtk::Label>,
        pub caption_description: RefCell<String>,
        pub description: OnceCell<gtk::Label>,
        pub width_reserve: OnceCell<gtk::Widget>,
        pub popover: OnceCell<gtk::Popover>,
        pub popover_control: OnceCell<super::NumberControl>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for NumberControl {
        const NAME: &'static str = "CapyNumberControl";
        type Type = super::NumberControl;
        type ParentType = gtk::Box;
    }
    impl ObjectImpl for NumberControl {
        fn dispose(&self) {
            if let Some(popover) = self.popover.get().filter(|p| p.parent().is_some()) {
                popover.unparent();
            }
        }
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::OnceLock<Vec<glib::subclass::Signal>> =
                std::sync::OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    glib::subclass::Signal::builder("value-changed").build(),
                    glib::subclass::Signal::builder("input-changed").build(),
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
    pub(crate) fn composing(&self) -> bool { self.imp().composing.get() || self.imp().composition_keys.active() }

    pub(crate) fn set_caption(&self, title: &str, description: &str, localization: std::sync::Arc<layer_ui::Localizer>) {
        let imp = self.imp();
        if imp.localization.borrow().as_ref().is_some_and(|current| current.language() == localization.language())
            && *imp.editor_title.borrow() == title && *imp.caption_description.borrow() == description { return; }
        *imp.localization.borrow_mut() = Some(localization.clone());
        *imp.editor_title.borrow_mut() = title.to_string();
        *imp.caption_description.borrow_mut() = description.to_string();
        if let Some(label) = imp.inline_caption.get() {
            label.set_text(title);
            label.set_tooltip_text(Some(if description.is_empty() { title } else { description }));
            label.update_property(&[gtk::accessible::Property::Description(description)]);
        }
        self.update_property(&[gtk::accessible::Property::Label(title), gtk::accessible::Property::Description(description)]);
        if let Some(label) = imp.caption.get() { label.set_text(title); label.set_tooltip_text(Some(title)); }
        if let Some(label) = imp.description.get() { label.set_text(description); }
        let captions = layer_ui::NumericLabels::new(title, &localization);
        if let Some(display) = imp.display.get() { display.set_tooltip_text(Some(&captions.edit)); }
        if let Some(steps) = imp.steps.get() {
            for (button, caption) in steps.iter().zip([&captions.decrease, &captions.increase]) {
                button.set_tooltip_text(Some(caption));
                button.update_property(&[gtk::accessible::Property::Label(caption)]);
            }
        }
        self.update_localization(localization);
    }

    pub(crate) fn update_localization(&self, localization: std::sync::Arc<layer_ui::Localizer>) {
        *self.imp().localization.borrow_mut() = Some(localization.clone());
        if !self.imp().input_valid.get() {
            let text = self.imp().entry.get().map(|entry| entry.text().to_string())
                .or_else(|| self.imp().spin.get().map(|spin| spin.text().to_string()));
            if let Some(text) = text {
                if let Err(error) = self.spec().resolve(self.value(), NumericOperation::Expression { text }) {
                    self.feedback(Some(error));
                }
            }
        }
        if let Some(control) = self.imp().popover_control.get() { control.update_localization(localization); }
    }

    pub fn new(spec: NumericControl, title: &str, description: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Self {
        Self::build(spec, title, description, false, false, localization)
    }
    /// One-line slider with only an editable value. Numeric policy is unchanged.
    pub fn inline(spec: NumericControl, title: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Self {
        Self::build(spec, title, "", true, false, localization)
    }
    /// Panel row with its label, slider and editable value on one line.
    pub fn labeled_inline(
        spec: NumericControl,
        title: &str,
        tooltip: &str,
        labels: &gtk::SizeGroup,
        values: &gtk::SizeGroup,
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Self {
        let control = Self::inline(spec, title, localization);
        control.set_tooltip_text(Some(tooltip));
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        label.set_tooltip_text(Some(tooltip));
        label.update_property(&[gtk::accessible::Property::Description(tooltip)]);
        let row=control.first_child().and_downcast::<gtk::Box>().unwrap();
        labels.add_widget(&label);
        values.add_widget(&row.last_child().unwrap());
        row.prepend(&label);
        control.imp().inline_caption.set(label).unwrap();
        control
    }
    /// Fit a grouped value to its current readout; the editor shares that width.
    pub fn value_only(spec: NumericControl, title: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Self {
        let control = Self::inline(spec, title, localization);
        control.set_halign(gtk::Align::Center);
        let imp = control.imp();
        if let Some(reserve) = imp.width_reserve.get() { reserve.set_visible(false); }
        if let Some(label) = imp.value_label.get() {
            label.set_width_chars(0);
            label.set_max_width_chars(-1);
        }
        if let Some(entry) = imp.entry.get() {
            entry.set_width_chars(1);
            entry.set_max_width_chars(1);
        }
        if let Some(slider) = control.imp().slider.get() {
            slider.set_visible(false);
        }
        control
    }
    /// Compact toolbar presentation using the same editor as panel controls.
    pub fn compact(spec: NumericControl, title: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Self {
        Self::build(spec, title, "", true, true, localization)
    }
    /// GtkBox's layout manager owns allocation. The tile owner supplies its
    /// final width before allocating the row, so native minimum sizes cannot
    /// expand the readout beyond the tile. All measurements include theme CSS.
    pub fn fit_width(&self, width: i32) {
        let imp = self.imp();
        if !imp.compact.get() || imp.slider.get().is_some_and(|s| s.is_visible()) {
            return;
        }
        let label = imp.value_label.get().unwrap();
        let face = imp.face.get().unwrap();
        let display = imp.display.get().unwrap();
        let horizontal = gtk::Orientation::Horizontal;
        let inset = (display.measure(horizontal, -1).0 - face.measure(horizontal, -1).0).max(0);
        let icon = imp.icon_row.get().unwrap();
        let icon_width = if icon.is_visible() && face.orientation() == horizontal {
            icon.measure(horizontal, -1).0 + face.spacing()
        } else {
            0
        };
        let mut available = (width - inset - icon_width).max(1);
        let unit = imp.unit.get().unwrap();
        let unit_width = unit.layout().pixel_size().0 + imp.value_row.get().unwrap().spacing();
        let unscaled = label
            .create_pango_layout(Some(&label.text()))
            .pixel_size()
            .0;
        // Keep normal app typography before spending scarce width on units.
        let show_unit =
            imp.separate_unit.get() && imp.show_units.get() && unscaled + unit_width <= available;
        unit.set_visible(show_unit);
        if show_unit {
            available -= unit_width;
        }
        // Use the label's shaped text (including tabular digits), and verify
        // the scaled glyphs: font hinting rounds individual glyph advances.
        let layout = label.layout().copy();
        layout.set_width(-1);
        let base = layout
            .attributes()
            .and_then(|a| a.copy())
            .unwrap_or_default();
        base.change(gtk::pango::AttrFloat::new_scale(1.));
        layout.set_attributes(Some(&base));
        let text_width = layout.pixel_size().0.max(1);
        let mut scale = (available as f64 / text_width as f64).min(1.);
        loop {
            let attrs = base.copy().unwrap();
            attrs.change(gtk::pango::AttrFloat::new_scale(scale));
            layout.set_attributes(Some(&attrs));
            let overflow = layout.pixel_size().0 - available;
            if overflow <= 0 || scale <= 0.01 {
                break;
            }
            scale = (scale - (overflow as f64 / text_width as f64).max(0.01)).max(0.01);
        }
        if (imp.readout_scale.replace(scale) - scale).abs() > 0.001 {
            let attrs = gtk::pango::AttrList::new();
            attrs.insert(gtk::pango::AttrFloat::new_scale(scale));
            label.set_attributes(Some(&attrs));
        }
    }
    pub fn set_slider_visible(&self, visible: bool) {
        if let Some(slider) = self.imp().slider.get() {
            slider.set_visible(visible);
        }
        if let Some(stack) = self.imp().stack.get() {
            stack.set_hexpand(!visible);
            if self.imp().compact.get() {
                stack.set_halign(gtk::Align::Fill);
            }
        }
    }
    pub fn set_icon(&self, icon: &str) {
        if let Some(image) = self.imp().icon.get() {
            crate::icons::set(image, Some(&format!("layer-{icon}-symbolic")));
        }
    }
    pub fn set_face(
        &self,
        show_icon: bool,
        title: &str,
        stacked: bool,
        icon_size: i32,
        show_units: bool,
    ) {
        let imp = self.imp();
        if let Some(reserve) = imp.width_reserve.get() {
            reserve.set_visible(!show_icon && !stacked);
        }
        if let Some(label) = imp.value_label.get() {
            label.set_attributes(None);
            imp.readout_scale.set(1.);
        }
        if let Some(image) = imp.icon.get() {
            image.set_visible(show_icon);
            image.set_pixel_size(icon_size);
        }
        if let Some(row) = imp.icon_row.get() {
            row.set_visible(show_icon);
        }
        if let Some(label) = imp.title.get() {
            label.set_text(title);
            label.set_visible(!title.is_empty());
        }
        if let Some(face) = imp.face.get() {
            face.set_spacing(if stacked { 0 } else { 4 });
            face.set_orientation(if stacked {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            });
        }
        imp.separate_unit.set(stacked);
        imp.show_units
            .set(show_units && !self.spec().unit.is_empty());
        if let Some(unit) = imp.unit.get() {
            unit.set_visible(stacked && show_units && !self.spec().unit.is_empty());
        }
        self.set_value(self.value());
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
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Self {
        let control: Self = glib::Object::new();
        control.imp().spec.set(spec.clone()).unwrap();
        control.imp().input_valid.set(true);
        *control.imp().localization.borrow_mut() = Some(localization);
        *control.imp().editor_title.borrow_mut() = title.to_string();
        *control.imp().caption_description.borrow_mut() = description.to_string();
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
        control.imp().caption.set(label).unwrap();
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
            control.imp().description.set(description).unwrap();
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
            spin.connect_input(glib::clone!(#[weak] control, #[upgrade_or] Some(Err(())), move |spin| {
                Some(control.text_input(&spin.text()).map(|value| value.value * control.spec().scale))
            }));
            spin.connect_output(glib::clone!(#[weak] control, #[upgrade_or] glib::Propagation::Proceed, move |_| {
                if control.input_valid() { glib::Propagation::Proceed } else { glib::Propagation::Stop }
            }));
            spin.connect_changed(glib::clone!(#[weak] control, move |spin| {
                if !control.imp().updating.get() && !control.imp().composing.get() {
                    let _ = control.text_input(&spin.text());
                }
            }));
            control.track_preedit(&spin);
            spin.connect_change_value(glib::clone!(#[weak] control, move |spin, _| {
                if !control.commit_text() { spin.stop_signal_emission_by_name("change-value"); }
            }));
            control.track_keys(&spin);
            spin.connect_value_changed(glib::clone!(
                #[weak]
                control,
                move |spin| {
                    if !control.imp().updating.get() && control.input_valid() {
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
            value_label.add_css_class("number-readout");
            value_label.set_xalign(if compact { 0.5 } else { 1.0 });
            if compact {
                let face = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                face.set_halign(gtk::Align::Center);
                face.set_valign(gtk::Align::Center);
                let icon = crate::icons::image("layer-settings-symbolic");
                icon.set_visible(false);
                icon.set_pixel_size(14);
                let title_label = gtk::Label::new(None);
                title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                title_label.set_visible(false);
                let icon_row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                icon_row.set_halign(gtk::Align::Center);
                icon_row.set_valign(gtk::Align::Center);
                icon_row.set_visible(false);
                icon_row.append(&icon);
                face.append(&icon_row);
                let caption = gtk::Box::new(gtk::Orientation::Vertical, 0);
                caption.set_valign(gtk::Align::Center);
                caption.append(&title_label);
                let value_row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                value_row.set_halign(gtk::Align::Center);
                value_row.append(&value_label);
                let unit = gtk::Label::new(Some(&spec.unit));
                unit.add_css_class("number-unit");
                unit.set_visible(false);
                value_row.append(&unit);
                caption.append(&value_row);
                face.append(&caption);
                display.set_child(Some(&face));
                control.imp().unit.set(unit).unwrap();
                control.imp().value_row.set(value_row).unwrap();
                control.imp().icon_row.set(icon_row).unwrap();
                control.imp().face.set(face).unwrap();
                control.imp().icon.set(icon).unwrap();
                control.imp().title.set(title_label).unwrap();
            } else {
                display.set_child(Some(&value_label));
            }
            control.imp().value_label.set(value_label.clone()).unwrap();
            display.add_css_class("number-value");
            display.add_css_class("flat");
            let captions = layer_ui::NumericLabels::new(title, control.imp().localization.borrow().as_ref().unwrap());
            display.set_tooltip_text(Some(&captions.edit));
            let entry = gtk::Entry::builder()
                .has_frame(false)
                .width_chars(3)
                .max_width_chars(10)
                .build();
            if inline {
                // Reserve the full formatted range, not the current value's
                // digit count. Expressions scroll inside this same footprint.
                let chars = spec
                    .width_samples(compact)
                    .iter()
                    .map(|v| v.chars().count())
                    .max()
                    .unwrap_or(1) as i32;
                value_label.set_width_chars(if compact { 0 } else { chars });
                value_label.set_max_width_chars(if compact { -1 } else { chars });
                entry.set_width_chars(if compact { 1 } else { chars });
                entry.set_max_width_chars(if compact { 1 } else { chars });
            }
            gtk::prelude::EditableExt::set_alignment(&entry, if compact { 0.5 } else { 1.0 });
            entry.add_css_class("number-entry");
            let stack = gtk::Stack::new();
            stack.set_hhomogeneous(inline);
            stack.set_vhomogeneous(compact);
            stack.set_halign(gtk::Align::End);
            stack.set_valign(gtk::Align::Center);
            stack.connect_visible_child_name_notify(glib::clone!(#[weak] control, move |stack| {
                control.imp().editing.set(stack.visible_child_name().as_deref() == Some("entry"));
            }));
            stack.add_named(&display, Some("value"));
            stack.add_named(&entry, Some("entry"));
            if inline {
                // Invisible stack pages participate in measurement. Measure all
                // range samples in the actual font; expressions scroll within
                // this fixed footprint instead of resizing nearby controls.
                let reserve = gtk::Stack::new();
                reserve.set_hhomogeneous(true);
                for text in spec.width_samples(compact) {
                    let sample = gtk::Button::new();
                    sample.add_css_class("number-value");
                    sample.add_css_class("flat");
                    let label = gtk::Label::new(Some(
                        &text
                            .chars()
                            .map(|c| if c.is_ascii_digit() { '8' } else { c })
                            .collect::<String>(),
                    ));
                    label.add_css_class("number-readout");
                    sample.set_child(Some(&label));
                    reserve.add_child(&sample);
                }
                stack.add_named(&reserve, Some("measure"));
                control.imp().width_reserve.set(reserve.upcast()).unwrap();
            }
            header.append(&stack);
            display.connect_clicked(glib::clone!(
                #[weak]
                control,
                move |_| {
                    if control.imp().popover_enabled.get() {
                        control.open_popover();
                        return;
                    }
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
            control.track_preedit(&entry);
            entry.connect_changed(glib::clone!(#[weak] control, move |entry| {
                if !control.imp().updating.get() && !control.imp().composing.get() {
                    let _ = control.text_input(&entry.text());
                }
            }));
            entry.connect_activate(glib::clone!(
                #[weak]
                control,
                move |_| {
                    if !control.imp().composing.get() && !control.imp().composition_keys.active() { control.finish(false); }
                }
            ));
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak]
                control,
                move |_| {
                    if control.imp().composing.replace(false) {
                        control.imp().entry.get().unwrap().reset_im_context();
                    } else { control.finish(false); }
                    control.imp().composition_keys.clear();
                    control.validate_input();
                    control.emit_by_name::<()>("input-changed", &[]);
                }
            ));
            entry.add_controller(focus);
            control.track_keys(&entry);
            control.imp().entry.set(entry).unwrap();
            control.install_value_gestures(&display);
            control.imp().display.set(display).unwrap();
            control.imp().stack.set(stack).unwrap();
            let track = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            track.add_css_class("number-track");
            let minus = crate::icons::button("layer-minus-symbolic");
            let plus = crate::icons::button("layer-plus-symbolic");
            for (button, steps, caption) in [(&minus, -1.0, &captions.decrease), (&plus, 1.0, &captions.increase)] {
                button.add_css_class("flat");
                button.add_css_class("number-step");
                button.set_tooltip_text(Some(caption));
                button.connect_clicked(glib::clone!(
                    #[weak]
                    control,
                    move |_| {
                        if control.commit_text() { control.apply(NumericOperation::Step { steps }); }
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
                        let position = slider.value();
                        if control.commit_text() {
                            control.apply(NumericOperation::Position { position });
                        } else { control.set_value(control.value()); }
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
        control.connect_unmap(|control| {
            control.cancel_edit();
        });
        control
    }
    pub fn set_popover_editor(&self, enabled: bool) {
        if self.imp().popover_enabled.replace(enabled) != enabled {
            self.cancel_edit();
        }
    }
    pub fn present_popover(&self) {
        if let Some(popover) = self.imp().popover.get().filter(|p| p.is_visible()) {
            popover.present();
        }
    }
    fn open_popover(&self) {
        let imp = self.imp();
        let popover = imp.popover.get_or_init(|| {
            let editor =
                NumberControl::new(self.spec().clone(), &imp.editor_title.borrow(), "", imp.localization.borrow().as_ref().unwrap().clone());
            editor.set_size_request(240, -1);
            editor.set_margin_start(12);
            editor.set_margin_end(12);
            editor.set_margin_top(8);
            editor.set_margin_bottom(8);
            editor.connect_value_changed(glib::clone!(
                #[weak(rename_to=control)]
                self,
                move |editor| {
                    control.apply(NumericOperation::Value {
                        value: editor.value(),
                    });
                }
            ));
            let popover = gtk::Popover::new();
            popover.set_widget_name("toolbar-number-popover");
            popover.set_child(Some(&editor));
            // GtkBox would lay the popover out as another numeric row. Attach
            // it to the value button, outside the control's box layout.
            popover.set_parent(imp.display.get().unwrap());
            popover.connect_closed(glib::clone!(
                #[weak]
                editor,
                move |_| editor.cancel_edit()
            ));
            imp.popover_control.set(editor).unwrap();
            popover
        });
        imp.popover_control.get().unwrap().set_value(self.value());
        popover.popup();
        popover.present();
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
                if !control.commit_text() {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                if let Ok(value) = control
                    .spec()
                    .resolve(control.value(), NumericOperation::Format)
                {
                    origin.set(value.fill);
                }
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to=control)]
            self,
            #[strong]
            origin,
            move |g, dx, dy| {
                if !control.input_valid() || !control.drag_check_threshold(0, 0, dx as i32, dy as i32) {
                    return;
                }
                g.set_state(gtk::EventSequenceState::Claimed);
                // Native distance becomes normalized slider travel. The core
                // applies the same log/power/linear mapping as the track.
                control.apply(NumericOperation::Position {
                    position: origin.get() - dy / 200.,
                });
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
                if control.commit_text() { control.apply(NumericOperation::Step { steps: -dy }); }
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
        if let Some(editor) = imp.popover_control.get() {
            editor.set_value(value);
        }
        if let Some(label) = imp.value_label.get() {
            label.set_text(&if imp.compact.get() {
                if imp.separate_unit.get() {
                    self.spec().compact_value(value)
                } else {
                    self.spec().compact_text(value)
                }
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
            if spin.value() != value * self.spec().scale { spin.set_value(value * self.spec().scale); }
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
    /// Accept text typed into the field that it has not committed yet.
    pub fn commit_text(&self) -> bool {
        if let Some(editor) = self.imp().popover_control.get().filter(|_| self.imp().popover.get().is_some_and(|popover| popover.is_visible())) {
            if !editor.commit_text() { return false; }
        }
        if !self.input_valid() { return false; }
        if let Some(spin) = self.imp().spin.get() {
            if self.text_input(&spin.text()).is_err() { return false; }
            spin.update();
            self.input_valid()
        } else if self.imp().editing.get() { self.finish(false) } else { true }
    }
    pub fn input_valid(&self) -> bool {
        self.imp().input_valid.get() && !self.imp().composing.get() && !self.imp().composition_keys.active()
            && self.imp().popover_control.get().filter(|_| self.imp().popover.get().is_some_and(|popover| popover.is_visible())).is_none_or(|editor| editor.input_valid())
    }
    pub fn connect_input_changed(&self, f: impl Fn(&Self) + 'static) {
        self.connect_closure("input-changed", false, glib::closure_local!(move |s: Self| f(&s)));
    }
    fn track_keys(&self, widget: &impl IsA<gtk::Widget>) {
        let capture = gtk::EventControllerKey::new();
        capture.set_name(Some("numeric-composition-capture"));
        capture.set_propagation_phase(gtk::PropagationPhase::Capture);
        capture.connect_key_pressed(glib::clone!(#[weak(rename_to=control)] self, #[upgrade_or] glib::Propagation::Proceed, move |_, _, keycode, _| {
            let was_owned = control.imp().composition_keys.active();
            if control.imp().composition_keys.capture(keycode, control.imp().composing.get()) != was_owned {
                control.emit_by_name::<()>("input-changed", &[]);
            }
            glib::Propagation::Proceed
        }));
        capture.connect_key_released(glib::clone!(#[weak(rename_to=control)] self, move |_, _, keycode, _| {
            if control.imp().composition_keys.release(keycode) {
                control.validate_input();
                control.emit_by_name::<()>("input-changed", &[]);
            }
        }));
        widget.add_controller(capture);
        let keys = gtk::EventControllerKey::new();
        keys.set_name(Some("numeric-editor-cancel"));
        keys.connect_key_pressed(glib::clone!(#[weak(rename_to=control)] self, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, _| {
            if control.imp().composition_keys.active() { return glib::Propagation::Stop; }
            if !control.imp().composing.get() && key == gtk::gdk::Key::Escape {
                control.cancel_edit();
                glib::Propagation::Stop
            } else { glib::Propagation::Proceed }
        }));
        widget.add_controller(keys);
        widget.connect_unmap(glib::clone!(#[weak(rename_to=control)] self, move |_| {
            control.imp().composition_keys.clear();
            control.validate_input();
        }));
        if widget.is::<gtk::SpinButton>() {
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(#[weak(rename_to=control)] self, move |_| {
                control.imp().composition_keys.clear();
                control.validate_input();
                control.emit_by_name::<()>("input-changed", &[]);
            }));
            widget.add_controller(focus);
        }
    }
    fn validate_input(&self) {
        if let Some(spin) = self.imp().spin.get() { let _ = self.text_input(&spin.text()); }
        else if self.imp().editing.get() {
            if let Some(entry) = self.imp().entry.get() { let _ = self.text_input(&entry.text()); }
        }
    }
    fn track_preedit(&self, editable: &impl IsA<gtk::Editable>) {
        if let Some(text) = editable.delegate().and_downcast::<gtk::Text>() {
            text.connect_preedit_changed(glib::clone!(#[weak(rename_to=control)] self, move |_, preedit| {
                control.imp().composing.set(!preedit.is_empty());
                control.emit_by_name::<()>("input-changed", &[]);
            }));
        }
    }
    fn feedback(&self, error: Option<layer_ui::NumericError>) {
        let valid = error.is_none();
        if let Some(error) = error {
            self.add_css_class("error");
            self.set_tooltip_text(Some(&error.message(self.imp().localization.borrow().as_ref().unwrap())));
        } else {
            self.remove_css_class("error");
            self.set_tooltip_text(None);
        }
        if self.imp().input_valid.replace(valid) != valid { self.emit_by_name::<()>("input-changed", &[]); }
    }
    fn text_input(&self, text: &str) -> Result<layer_ui::NumericValue, ()> {
        if self.imp().composing.get() || self.imp().composition_keys.active() { return Err(()); }
        match self.spec().resolve(0., NumericOperation::Expression { text: text.into() }) {
            Ok(value) => { self.feedback(None); Ok(value) }
            Err(error) => { self.feedback(Some(error)); Err(()) }
        }
    }
    pub fn cancel_edit(&self) {
        if let Some(popover) = self.imp().popover.get() {
            popover.popdown();
        }
        self.finish(true);
        self.imp().composition_keys.clear();
        if let Some(spin) = self.imp().spin.get() {
            self.imp().composing.set(false);
            self.feedback(None);
            spin.set_value(self.value() * self.spec().scale);
        }
    }
    /// A click away accepts valid text and retires invalid unfinished input.
    /// It does not claim the contact from the next control or the canvas.
    pub fn dismiss_toolbar_edit(&self) -> bool {
        if !self.imp().compact.get() {
            return false;
        }
        self.finish(false);
        self.finish(true);
        true
    }
    fn apply(&self, op: NumericOperation) -> bool {
        match self.spec().resolve(self.value(), op) {
            Ok(v) => {
                self.feedback(None);
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
                self.feedback(Some(e));
                false
            }
        }
    }
    fn finish(&self, cancel: bool) -> bool {
        if !cancel && (self.imp().composing.get() || self.imp().composition_keys.active()) { return false; }
        if !self.imp().editing.get() { return true; }
        let Some(stack) = self.imp().stack.get() else {
            return true;
        };
        if stack.visible_child_name().as_deref() != Some("entry") {
            return true;
        }
        if cancel
            || self.apply(NumericOperation::Expression {
                text: self.imp().entry.get().unwrap().text().into(),
            })
        {
            // Switch first so focus-leave cannot commit twice.
            stack.set_visible_child_name("value");
            self.imp().composing.set(false);
            self.imp().entry.get().unwrap().reset_im_context();
            self.feedback(None);
            true
        } else { false }
    }
}
