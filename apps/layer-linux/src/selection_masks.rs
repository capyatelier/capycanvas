//! Native presentation of shared selection destinations and menus.
use crate::number_control::NumberControl;
use crate::workspace::Workspace;
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// The Refine dialog: one value for Grow, Shrink, Feather, Border or Smooth.
/// The session previews every value on the canvas, which stays visible and
/// undimmed behind the dialog.
pub struct RefineDialog {
    dialog: adw::AlertDialog,
    shown: Rc<Cell<bool>>,
    number: RefCell<Option<(layer_ui::RefineKind, NumberControl)>>,
    workspace: RefCell<Weak<Workspace>>,
}
impl RefineDialog {
    pub const PREVIEW_CLASS: &'static str = "canvas-preview-dialog";
    pub fn new() -> Self {
        use adw::prelude::*;
        let dialog = adw::AlertDialog::new(None, None);
        dialog.set_widget_name("selection-refine-dialog");
        dialog.set_presentation_mode(adw::DialogPresentationMode::BottomSheet);
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("apply", "Apply");
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("apply"));
        Self {
            dialog,
            shown: Rc::new(Cell::new(false)),
            number: RefCell::new(None),
            workspace: RefCell::new(Weak::new()),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
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
                    w.dispatch(layer_ui::UiAction::Selection {
                        action: if response == "apply" {
                            layer_ui::SelectionAction::ApplyResize
                        } else {
                            layer_ui::SelectionAction::CancelResize
                        },
                    });
                }
            ),
        );
    }
    /// The value field for `view`, rebuilt when the operation changes.
    fn number(&self, view: &layer_ui::SelectionRefineView) -> NumberControl {
        use adw::prelude::*;
        if let Some((kind, number)) = self.number.borrow().as_ref()
            && *kind == view.kind
        {
            return number.clone();
        }
        let number = NumberControl::new(view.numeric.clone(), view.label, "");
        number.set_widget_name("selection-refine-value");
        let workspace = self.workspace.borrow().clone();
        number.connect_value_changed(move |number| {
            if let Some(w) = workspace.upgrade() {
                w.dispatch(layer_ui::UiAction::Selection {
                    action: layer_ui::SelectionAction::ResizeRadius { radius: number.value() as f32 },
                });
            }
        });
        self.dialog.set_extra_child(Some(&number));
        *self.number.borrow_mut() = Some((view.kind, number.clone()));
        number
    }
    pub fn refresh(&self, w: &Workspace, state: &layer_ui::UiState) {
        use adw::prelude::*;
        if let Some(view) = &state.layer_tools.selection_resize {
            self.dialog.set_heading(Some(view.title));
            self.number(view).set_value(view.radius as f64);
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
