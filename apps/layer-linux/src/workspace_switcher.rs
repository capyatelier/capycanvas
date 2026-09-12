//! Configurable header choices use normal workspace switching and ownership.
use super::*;
use layer_workspace::{DEFAULT_WORKSPACES, ManagerAction};

pub(super) fn build() -> (gtk::Box, gtk::Box) {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.set_widget_name("workspace-switcher");
    root.add_css_class("workspace-switcher");
    root.set_valign(gtk::Align::Center);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(true)
        .max_content_width(420)
        .child(&buttons)
        .build();
    root.append(&scroll);
    (root, buttons)
}

impl NativeWorkspaces {
    pub(super) fn bind_switcher(&self, w: &Rc<Workspace>) {
        *self.switch_owner.borrow_mut() = Rc::downgrade(w);
        self.update_switcher();
    }
    pub(super) fn update_switcher(&self) {
        let Some(w) = self.switch_owner.borrow().upgrade() else {
            return;
        };
        let ids = self
            .manager
            .as_ref()
            .map(|m| m.switcher_display_ids())
            .unwrap_or_default();
        let active = self.manager.as_ref().and_then(|m| m.active_id());
        let items = self.manager.as_ref().map(|m| m.items()).unwrap_or_default();
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
                    .map(|(_, p)| p.name().to_lowercase())
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
        for (id, button) in buttons.iter() {
            let Some(item) = items.iter().find(|i| &i.id == id) else {
                continue;
            };
            let name = &item.metadata.name;
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_text(name);
            }
            button.set_tooltip_text(Some(&format!("Switch to {name} workspace")));
            button.update_property(&[gtk::accessible::Property::Label(name)]);
            button.set_active(active.as_ref() == Some(id));
        }
        self.switcher.set_visible(!ids.is_empty());
        self.switcher.set_sensitive(
            self.manager.is_some()
                && self.ready.get()
                && !self.busy.get()
                && !self.switch_pending.get()
                && !self.validating_owner.get(),
        );
    }
}

fn bind_button(w: &Rc<Workspace>, button: &gtk::ToggleButton, id: String) {
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| {
            let id = id.clone();
            let native = &w.workspaces;
            // Selection reflects the workspace actually adopted, even if
            // the requested switch fails or focuses a different window.
            native.update_switcher();
            if !native.ready.get()
                || native.busy.get()
                || native.switch_pending.get()
                || native
                    .manager
                    .as_ref()
                    .is_none_or(|m| m.active_id().as_deref() == Some(id.as_str()))
            {
                return;
            }
            native.switch_pending.set(true);
            native.update_switcher();
            glib::spawn_future_local(glib::clone!(
                #[weak]
                w,
                async move {
                    let native = &w.workspaces;
                    let manager = native.manager.as_ref().unwrap();
                    let result = async {
                        let stored = manager.load(&id).await?;
                        let elsewhere = stored.claim.as_ref().is_some_and(|c| {
                            c.owner != manager.owner && c.expires_at_ms > now_ms()
                        });
                        native
                            .perform(
                                &w,
                                if elsewhere {
                                    ManagerAction::SwitchToWindow(id.into())
                                } else {
                                    ManagerAction::Switch(id.into())
                                },
                            )
                            .await
                    }
                    .await;
                    if let Err(error) = result {
                        w.status.set_text(&error.to_string());
                        w.status.set_visible(true);
                    }
                    native.switch_pending.set(false);
                    native.update_status();
                }
            ));
        }
    ));
}
