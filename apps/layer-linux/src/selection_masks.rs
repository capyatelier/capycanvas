//! Native presentation of shared selection destinations and menus.
use crate::{number_control::NumberControl, workspace::Workspace};
use gtk::{glib, prelude::*};
use layer_ui::{CommandId, NumericControl, UiAction, UiState};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy)]
pub enum Menu {
    Selection,
    QuickMask,
    Overlay,
}
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
            let model = w.gpu.borrow().as_ref().map(|g| match kind {
                Menu::Selection => g
                    .session
                    .application_menu(layer_ui::ApplicationMenu::Select),
                Menu::QuickMask => g.session.quick_mask_menu(),
                Menu::Overlay => g.session.selection_overlay_menu(),
            });
            if let Some(model) = model {
                w.populate_workspace_menu(popover, model);
            }
        }
    ));
    button
}

fn command_button(w: &Rc<Workspace>, label: &str, command: CommandId) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_size_request(-1, 44);
    w.bind_action_tooltip(&button, UiAction::Invoke { command });
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| w.dispatch(UiAction::Invoke { command })
    ));
    button
}

/// Remains visible with Layers and Tool Settings closed, or with overlay hidden.
pub struct MaskActions {
    pub root: gtk::Box,
    label: gtk::Label,
    reason: gtk::Label,
    controls: gtk::Box,
    gray: NumberControl,
    updating: Rc<Cell<bool>>,
}
impl MaskActions {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_widget_name("selection-mask-actions");
        root.add_css_class("selection-mask-actions");
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::End);
        root.set_margin_bottom(40);
        root.set_visible(false);
        let label = gtk::Label::new(None);
        label.add_css_class("heading");
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_max_width_chars(44);
        root.append(&label);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let gray = NumberControl::inline(NumericControl::percent(), "Foreground mask gray");
        gray.set_widget_name("selection-mask-gray");
        gray.set_size_request(150, 44);
        controls.append(&gray);
        root.append(&controls);
        let reason = gtk::Label::new(None);
        reason.set_wrap(true);
        reason.set_max_width_chars(52);
        reason.add_css_class("dim-label");
        root.append(&reason);
        Self {
            root,
            label,
            reason,
            controls,
            gray,
            updating: Rc::new(Cell::new(false)),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        let updating = self.updating.clone();
        self.gray.connect_value_changed(glib::clone!(
            #[weak]
            w,
            move |value| {
                if !updating.get() {
                    w.dispatch(UiAction::SetToolSetting {
                        id: "mask_gray".into(),
                        value: value.value() as f32,
                    });
                }
            }
        ));
        self.controls
            .append(&command_button(w, "Swap", CommandId::SwapMaskColors));
        self.controls
            .append(&menu_button(w, "Overlay", Menu::Overlay));
        let done = command_button(w, "Done", CommandId::ReturnToArtwork);
        done.set_widget_name("selection-mask-done");
        self.controls.append(&done);
    }
    pub fn refresh(&self, state: &UiState) {
        self.root
            .set_visible(state.layer_tools.mask_editing.is_some());
        let Some(view) = &state.layer_tools.mask_editing else {
            return;
        };
        self.label.set_text(&view.label);
        self.reason
            .set_text(view.reason.unwrap_or("Black protects · White selects"));
        self.updating.set(true);
        self.gray.set_value(view.gray as f64);
        self.updating.set(false);
    }
}

pub struct QuickMaskRow {
    pub root: gtk::Box,
    eye: gtk::Button,
}
impl QuickMaskRow {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.set_widget_name("quick-mask-layer");
        root.add_css_class("selection-mask-row");
        root.set_visible(false);
        root.set_margin_start(6);
        root.set_margin_end(6);
        root.set_margin_top(6);
        root.set_margin_bottom(6);
        let eye = crate::icons::button("layer-eye-symbolic");
        eye.set_size_request(44, 44);
        eye.set_tooltip_text(Some("Show or hide Quick Mask overlay"));
        root.append(&eye);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let label = gtk::Label::new(Some("Quick Mask"));
        label.set_xalign(0.);
        label.add_css_class("heading");
        let caption = gtk::Label::new(Some("Temporary"));
        caption.set_xalign(0.);
        caption.add_css_class("dim-label");
        text.append(&label);
        text.append(&caption);
        root.append(&text);
        Self { root, eye }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.eye.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| w.dispatch(UiAction::Invoke {
                command: CommandId::MaskOverlay
            })
        ));
        self.root
            .append(&menu_button(w, "Actions", Menu::QuickMask));
    }
    pub fn refresh(&self, state: &UiState) {
        self.root.set_visible(state.layer_tools.quick_mask);
        let visible = state
            .layer_tools
            .mask_editing
            .as_ref()
            .is_some_and(|m| m.overlay);
        crate::icons::set_button(
            &self.eye,
            if visible {
                "layer-eye-symbolic"
            } else {
                "layer-eye-hidden-symbolic"
            },
        );
    }
}
