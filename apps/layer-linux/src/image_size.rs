//! Native presentation of the shared Image Size dialog. Sizes, units,
//! resampling, validation and history live in layer-ui; this only projects
//! the view.
use crate::number_control::NumberControl;
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_ui::{CanvasSizeUnit, ImageResample, ImageSizeAction, ImageSizeView, NumericControl, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// Width, height and resolution: each field's number control and the spec
/// it was built for.
type Numbers = Rc<RefCell<[Option<(NumericControl, NumberControl)>; 3]>>;

pub struct ImageSizeDialog {
    pub(crate) dialog: adw::AlertDialog,
    shown: Rc<Cell<bool>>,
    updating: Rc<Cell<bool>>,
    can_apply: Rc<Cell<bool>>,
    workspace: RefCell<Weak<Workspace>>,
    fields: [gtk::Box; 3],
    numbers: Numbers,
    unit: gtk::DropDown,
    constrain: gtk::CheckButton,
    resample_label: gtk::Label,
    resample: gtk::DropDown,
    message: gtk::Label,
}

fn send(workspace: &Weak<Workspace>, action: ImageSizeAction) {
    if let Some(w) = workspace.upgrade() {
        w.dispatch(UiAction::ImageSize { action });
    }
}

impl ImageSizeDialog {
    pub fn new() -> Self {
        let dialog = adw::AlertDialog::new(None, None);
        dialog.set_widget_name("image-size-dialog");
        dialog.add_response("cancel", "");
        dialog.add_response("apply", "");
        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("apply"));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let field = |name: &str| {
            let field = gtk::Box::new(gtk::Orientation::Vertical, 0);
            field.set_widget_name(&format!("image-size-{name}"));
            field
        };
        let fields = [field("width"), field("height"), field("resolution")];
        content.append(&fields[0]);
        content.append(&fields[1]);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let unit = gtk::DropDown::from_strings(&[]);
        unit.set_widget_name("image-size-unit");
        unit.set_hexpand(true);
        options.append(&unit);
        let constrain = gtk::CheckButton::new();
        constrain.set_widget_name("image-size-constrain");
        options.append(&constrain);
        content.append(&options);
        content.append(&fields[2]);
        let resample_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let resample_label = gtk::Label::new(None);
        resample_label.set_xalign(0.);
        resample_label.set_hexpand(true);
        resample_row.append(&resample_label);
        let resample = gtk::DropDown::from_strings(&[]);
        resample.set_widget_name("image-size-resample");
        resample_row.append(&resample);
        content.append(&resample_row);
        let message = gtk::Label::new(None);
        message.set_widget_name("image-size-message");
        message.set_wrap(true);
        message.set_xalign(0.);
        message.add_css_class("dim-label");
        content.append(&message);
        dialog.set_extra_child(Some(&content));
        Self {
            dialog,
            shown: Rc::new(Cell::new(false)),
            updating: Rc::new(Cell::new(false)),
            can_apply: Rc::new(Cell::new(false)),
            workspace: RefCell::new(Weak::new()),
            fields,
            numbers: Numbers::default(),
            unit,
            constrain,
            resample_label,
            resample,
            message,
        }
    }

    /// Typed values the fields have not committed yet.
    fn commit(numbers: &Numbers) -> bool {
        let controls: Vec<_> = numbers.borrow().iter().flatten().map(|(_, n)| n.clone()).collect();
        controls.iter().all(NumberControl::commit_text)
    }

    pub fn bind(self: &Rc<Self>, w: &Rc<Workspace>) {
        *self.workspace.borrow_mut() = Rc::downgrade(w);
        let this = Rc::downgrade(self);
        self.unit.connect_selected_notify(glib::clone!(#[strong] this, move |unit| {
            let Some(this) = this.upgrade() else { return };
            if this.updating.get() { return; }
            let Some(&unit) = CanvasSizeUnit::ALL.get(unit.selected() as usize) else { return };
            if !Self::commit(&this.numbers) {
                if let Some(w) = this.workspace.borrow().upgrade() { w.refresh_document_view(); }
                return;
            }
            send(&this.workspace.borrow(), ImageSizeAction::Unit { unit });
        }));
        self.constrain.connect_toggled(glib::clone!(#[strong] this, move |check| {
            let Some(this) = this.upgrade() else { return };
            if this.updating.get() { return; }
            if !Self::commit(&this.numbers) {
                if let Some(w) = this.workspace.borrow().upgrade() { w.refresh_document_view(); }
                return;
            }
            send(&this.workspace.borrow(), ImageSizeAction::Constrain { constrain: check.is_active() });
        }));
        self.resample.connect_selected_notify(glib::clone!(#[strong] this, move |resample| {
            let Some(this) = this.upgrade() else { return };
            if this.updating.get() { return; }
            let Some(&resample) = ImageResample::ALL.get(resample.selected() as usize) else { return };
            if !Self::commit(&this.numbers) {
                if let Some(w) = this.workspace.borrow().upgrade() { w.refresh_document_view(); }
                return;
            }
            send(&this.workspace.borrow(), ImageSizeAction::Resample { resample });
        }));
        let shown = self.shown.clone();
        self.dialog.connect_response(None, glib::clone!(#[strong] this, move |_, response| {
            let Some(this) = this.upgrade() else { return };
            if response == "apply" {
                if !Self::commit(&this.numbers) {
                    if let Some(w) = this.workspace.borrow().upgrade() { w.refresh_document_view(); }
                    return;
                }
            }
            if !shown.replace(false) { return; }
            send(&this.workspace.borrow(), if response == "apply" {
                ImageSizeAction::Apply
            } else {
                ImageSizeAction::Cancel
            });
        }));
    }

    fn number(&self, index: usize, spec: &NumericControl, label: &str, localization: &std::sync::Arc<layer_ui::Localizer>) -> NumberControl {
        if let Some((current, number)) = &self.numbers.borrow()[index]
            && current == spec
        {
            return number.clone();
        }
        let number = NumberControl::new(spec.clone(), label, "", localization.clone());
        let numbers = Rc::downgrade(&self.numbers);
        let can_apply = self.can_apply.clone();
        number.connect_input_changed(glib::clone!(#[weak(rename_to=dialog)] self.dialog, move |_| {
            if let Some(numbers) = numbers.upgrade() {
                dialog.set_response_enabled("apply", can_apply.get() && numbers.borrow().iter().flatten().all(|(_, number)| number.input_valid()));
            }
        }));
        let workspace = self.workspace.borrow().clone();
        let updating = self.updating.clone();
        number.connect_value_changed(move |number| {
            if updating.get() { return; }
            let value = number.value();
            send(&workspace, match index {
                0 => ImageSizeAction::Width { value },
                1 => ImageSizeAction::Height { value },
                _ => ImageSizeAction::Resolution { value },
            });
        });
        let field = &self.fields[index];
        while let Some(child) = field.first_child() {
            field.remove(&child);
        }
        field.append(&number);
        self.numbers.borrow_mut()[index] = Some((spec.clone(), number.clone()));
        number
    }

    pub fn refresh(&self, w: &Workspace, state: &layer_ui::UiState) {
        let Some(view) = &state.layer_tools.image_size else {
            if self.shown.replace(false) {
                self.dialog.close();
            }
            return;
        };
        self.updating.set(true);
        if !self.shown.get() {
            self.dialog.set_response_label("apply", &view.apply_label);
            self.dialog.set_response_label("cancel", &view.cancel_label);
            let units: Vec<_> = view.units.iter().map(|choice| choice.label.as_ref()).collect();
            self.unit.set_model(Some(&gtk::StringList::new(&units)));
            let labels: Vec<_> = view.resamples.iter().map(|choice| choice.label.as_ref()).collect();
            self.resample.set_model(Some(&gtk::StringList::new(&labels)));
        }
        self.present(view, &w.localization);
        self.updating.set(false);
        if !self.shown.replace(true) {
            self.dialog.present(Some(&w.window));
        }
    }

    fn present(&self, view: &ImageSizeView, localization: &std::sync::Arc<layer_ui::Localizer>) {
        self.dialog.set_heading(Some(view.title.as_ref()));
        for axis in 0..2 {
            self.number(axis, &view.numeric[axis], view.labels[axis].as_ref(), localization).set_value(view.values[axis]);
        }
        self.number(2, &view.resolution_numeric, view.resolution_label.as_ref(), localization).set_value(view.resolution);
        self.unit.set_selected(CanvasSizeUnit::ALL.iter().position(|u| *u == view.unit).unwrap_or(0) as u32);
        self.constrain.set_label(Some(view.constrain_label.as_ref()));
        self.constrain.set_active(view.constrain);
        self.resample_label.set_text(view.resample_label.as_ref());
        self.resample.set_selected(ImageResample::ALL.iter().position(|r| *r == view.resample).unwrap_or(0) as u32);
        self.message.set_text(&view.message);
        self.can_apply.set(view.can_apply);
        self.dialog.set_response_enabled("apply", view.can_apply && self.numbers.borrow().iter().flatten().all(|(_, number)| number.input_valid()));
    }
}
