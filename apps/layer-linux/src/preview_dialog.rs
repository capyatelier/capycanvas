//! Native presentation of the shared one-value dialogs the session previews
//! on the canvas: Refine (Grow, Shrink, Feather, Border or Smooth) and
//! Frequency Separation. The canvas stays visible and undimmed behind them.
use crate::number_control::NumberControl;
use crate::workspace::Workspace;
use gtk::glib;
use layer_ui::{NumericControl, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// What the dialog shows: its title and one labelled value.
pub struct PreviewValue<'a> {
    pub title: &'a str,
    pub label: &'a str,
    pub value: f64,
    pub numeric: &'a NumericControl,
}

pub struct PreviewDialog {
    dialog: adw::AlertDialog,
    field: &'static str,
    shown: Rc<Cell<bool>>,
    number: RefCell<Option<(String, NumericControl, NumberControl)>>,
    workspace: RefCell<Weak<Workspace>>,
    value: fn(f64) -> UiAction,
}
impl PreviewDialog {
    pub const PREVIEW_CLASS: &'static str = "canvas-preview-dialog";
    /// `name` and `field` name the dialog and its value field; `value` is the
    /// action for a new value.
    pub fn new(name: &'static str, field: &'static str, value: fn(f64) -> UiAction) -> Self {
        use adw::prelude::*;
        let dialog = adw::AlertDialog::new(None, None);
        dialog.set_widget_name(name);
        dialog.set_presentation_mode(adw::DialogPresentationMode::BottomSheet);
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("apply", "Apply");
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("apply"));
        Self {
            dialog,
            field,
            shown: Rc::new(Cell::new(false)),
            number: RefCell::new(None),
            workspace: RefCell::new(Weak::new()),
            value,
        }
    }
    /// `respond` is the action for Apply (true) or Cancel.
    pub fn bind(&self, w: &Rc<Workspace>, respond: fn(bool) -> UiAction) {
        use adw::prelude::*;
        *self.workspace.borrow_mut() = Rc::downgrade(w);
        let shown = self.shown.clone();
        self.dialog.connect_response(
            None,
            glib::clone!(
                #[weak]
                w,
                move |_, response| {
                    if !shown.replace(false) {
                        return;
                    }
                    w.window.remove_css_class(Self::PREVIEW_CLASS);
                    w.dispatch(respond(response == "apply"));
                }
            ),
        );
    }
    /// The value field, rebuilt when its label or range changes.
    fn number(&self, view: &PreviewValue, localization: &std::sync::Arc<layer_ui::Localizer>) -> NumberControl {
        use adw::prelude::*;
        if let Some((label, numeric, number)) = self.number.borrow().as_ref()
            && label == view.label
            && numeric == view.numeric
        {
            return number.clone();
        }
        let number = NumberControl::panel(view.numeric.clone(), view.label, localization.clone());
        number.set_widget_name(self.field);
        let workspace = self.workspace.borrow().clone();
        let value = self.value;
        number.connect_value_changed(move |number| {
            if let Some(w) = workspace.upgrade() {
                w.dispatch(value(number.value()));
            }
        });
        self.dialog.set_extra_child(Some(&number));
        *self.number.borrow_mut() = Some((view.label.into(), view.numeric.clone(), number.clone()));
        number
    }
    pub fn refresh(&self, w: &Workspace, view: Option<PreviewValue>) {
        use adw::prelude::*;
        if let Some(view) = view {
            self.dialog.set_heading(Some(view.title));
            self.number(&view, &w.localization()).set_value(view.value);
            if !self.shown.replace(true) {
                w.window.add_css_class(Self::PREVIEW_CLASS);
                self.dialog.present(Some(&w.window));
            }
        } else if self.shown.replace(false) {
            w.window.remove_css_class(Self::PREVIEW_CLASS);
            self.dialog.close();
        }
    }
}
