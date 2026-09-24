//! Native presentation of shared selection destinations and menus.
use crate::number_control::NumberControl;
use crate::workspace::Workspace;
use gtk::{glib, prelude::*};
use std::{cell::Cell, rc::Rc};

pub use layer_ui::SelectionMenu as Menu;
pub fn menu_button(w: &Rc<Workspace>, label: &str, kind: Menu) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_label(label);
    button.set_size_request(-1, 44);
    let popover = gtk::PopoverMenu::from_model(None::<&gtk::gio::Menu>);
    button.set_popover(Some(&popover));
    w.watch_popover(popover.upcast_ref());
    popover.connect_show(glib::clone!(
        #[weak]
        w,
        move |popover| {
            let model = w
                .gpu
                .borrow()
                .as_ref()
                .map(|g| g.session.selection_menu(kind));
            if let Some(model) = model {
                w.populate_workspace_menu(popover, model);
            }
        }
    ));
    button
}

/// A numeric operation dialog, separate from the mask's persistent properties.
pub struct ResizeDialog {
    dialog: adw::AlertDialog,
    shown: Rc<Cell<bool>>,
    updating: Rc<Cell<bool>>,
    number: NumberControl,
}
impl ResizeDialog {
    pub fn new() -> Self {
        use adw::prelude::*;
        let dialog = adw::AlertDialog::new(None, None);
        dialog.set_widget_name("selection-resize-dialog");
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("apply", "Apply");
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("apply"));
        let number = NumberControl::new(
            layer_ui::NumericControl::number(
                1.,
                layer_render::SelectionRefinement::MAX_RESIZE as f64,
                1.,
                0,
            )
            .unit("px"),
            "Distance",
            "",
        );
        number.set_widget_name("selection-resize-distance");
        dialog.set_extra_child(Some(&number));
        Self {
            dialog,
            shown: Rc::new(Cell::new(false)),
            updating: Rc::new(Cell::new(false)),
            number,
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        use adw::prelude::*;
        let updating = self.updating.clone();
        self.number.connect_value_changed(glib::clone!(
            #[weak]
            w,
            move |number| {
                if !updating.get() {
                    w.dispatch(layer_ui::UiAction::Selection {
                        action: layer_ui::SelectionAction::ResizeRadius {
                            radius: number.value() as f32,
                        },
                    });
                }
            }
        ));
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
    pub fn refresh(&self, w: &Workspace, state: &layer_ui::UiState) {
        use adw::prelude::*;
        if let Some(view) = &state.layer_tools.selection_resize {
            self.dialog.set_heading(Some(view.title));
            self.updating.set(true);
            self.number.set_value(view.radius as f64);
            self.updating.set(false);
            if !self.shown.replace(true) {
                self.dialog.present(Some(&w.window));
            }
        } else if self.shown.replace(false) {
            self.dialog.close();
        }
    }
}
