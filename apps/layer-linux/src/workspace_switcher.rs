//! Configurable header choices use normal workspace switching and ownership.
use super::*;
use layer_workspace::DEFAULT_WORKSPACES;

pub(super) fn build() -> (gtk::Box, gtk::Box) {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.set_widget_name("workspace-switcher");
    root.add_css_class("workspace-switcher");
    root.set_valign(gtk::Align::Center);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let scroll = crate::input::pen_scroller(
        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .max_content_width(420)
            .child(&buttons)
            .build(),
    );
    root.append(&scroll);
    (root, buttons)
}

impl NativeWorkspaces {
    pub fn switcher_popup(&self, w: &Rc<Workspace>) -> gtk::PopoverMenu {
        let menu = gtk::gio::Menu::new();
        let view = self.view();
        let action = gtk::gio::SimpleAction::new_stateful(
            "select",
            Some(glib::VariantTy::STRING),
            &view.id.clone().unwrap_or_default().to_variant(),
        );
        action.set_enabled(self.controller.borrow().is_some() && view.ready);
        for row in &view.switcher_display {
            let item = gtk::gio::MenuItem::new(Some(&row.title), None);
            item.set_action_and_target_value(Some("switcher.select"), Some(&row.id.to_variant()));
            menu.append_item(&item);
        }
        let popup = gtk::PopoverMenu::from_model(Some(&menu));
        popup.set_widget_name("workspace-switcher-popup");
        action.connect_activate(glib::clone!(
            #[weak]
            w,
            #[weak]
            popup,
            move |_, value| {
                if let Some(id) = value.and_then(|v| v.get::<String>()) {
                    popup.popdown();
                    w.workspaces.switch_to(&w, id);
                }
            }
        ));
        let actions = gtk::gio::SimpleActionGroup::new();
        actions.add_action(&action);
        popup.insert_action_group("switcher", Some(&actions));
        w.watch_popover(popup.upcast_ref());
        popup
    }

    pub(super) fn bind_switcher(&self, w: &Rc<Workspace>) {
        *self.switch_owner.borrow_mut() = Rc::downgrade(w);
        self.update_switcher();
    }
    pub(super) fn update_switcher(&self) {
        let Some(w) = self.switch_owner.borrow().upgrade() else {
            return;
        };
        let view = self.view();
        let ids: Vec<_> = view
            .switcher_display
            .iter()
            .map(|row| row.id.clone())
            .collect();
        let active = view.id.clone();
        let mut buttons = self.switch_buttons.borrow_mut();
        if buttons.iter().map(|(id, _)| id).ne(ids.iter()) {
            let body = &self.switch_body;
            while let Some(child) = body.first_child() {
                body.remove(&child);
            }
            buttons.clear();
            for id in &ids {
                let button = gtk::ToggleButton::new();
                let suffix = DEFAULT_WORKSPACES
                    .iter()
                    .find(|(key, _)| *key == id)
                    // Test/accessibility identity follows the stable workspace
                    // ID, not a display label that can be shortened or renamed.
                    .map(|(key, _)| key.rsplit(':').next().unwrap().to_string())
                    .unwrap_or_else(|| id.clone());
                button.set_widget_name(&format!("workspace-switch-{suffix}"));
                button.set_group(buttons.first().map(|(_, b)| b));
                let label = gtk::Label::new(None);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(14);
                button.set_child(Some(&label));
                bind_button(&w, &button, id.clone());
                body.append(&button);
                buttons.push((id.clone(), button));
            }
            if active.is_some()
                && ids.first() == active.as_ref()
                && let Some(scroll) = self
                    .switcher
                    .first_child()
                    .and_downcast::<gtk::ScrolledWindow>()
            {
                scroll.hadjustment().set_value(0.);
            }
        }
        for ((id, button), row) in buttons.iter().zip(&view.switcher_display) {
            let name = &row.title;
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_text(name);
            }
            button.set_tooltip_text(Some(&format!("Switch to {name} workspace")));
            button.update_property(&[gtk::accessible::Property::Label(name)]);
            button.set_active(active.as_ref() == Some(id));
        }
        self.switcher.set_visible(!ids.is_empty());
        self.switcher
            .set_sensitive(self.controller.borrow().is_some() && view.ready);
    }

    fn switch_to(&self, w: &Rc<Workspace>, id: String) {
        // Both presentations reflect the workspace actually adopted, even if
        // the requested switch fails or focuses a different window.
        self.update_switcher();
        let view = self.view();
        if !view.ready
            || view.busy
            || !self.accepts_input(w)
            || view.id.as_deref() == Some(id.as_str())
        {
            return;
        }
        self.send(w, WorkspaceInput::Switch { id });
    }
}

fn bind_button(w: &Rc<Workspace>, button: &gtk::ToggleButton, id: String) {
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| w.workspaces.switch_to(&w, id.clone())
    ));
}
