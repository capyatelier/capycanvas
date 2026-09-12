//! The header switches stable default workspace identities, never reapplies a
//! preset. Normal ownership, saving and in-flight gesture guards still apply.
use super::*;
use layer_workspace::{DEFAULT_WORKSPACES, ManagerAction};

pub(super) fn build() -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    root.set_widget_name("workspace-switcher");
    root.add_css_class("workspace-switcher");
    root.set_valign(gtk::Align::Center);
    let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
    for (_, preset) in DEFAULT_WORKSPACES {
        let button = gtk::ToggleButton::new();
        button.set_widget_name(&format!(
            "workspace-switch-{}",
            preset.name().to_lowercase()
        ));
        button.set_group(buttons.first());
        let label = gtk::Label::new(Some(preset.name()));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        button.set_child(Some(&label));
        root.append(&button);
        buttons.push(button);
    }
    (root, buttons)
}

impl NativeWorkspaces {
    pub(super) fn update_switcher(&self) {
        let active = self.manager.as_ref().and_then(|m| m.active_id());
        let items = self.manager.as_ref().map(|m| m.items()).unwrap_or_default();
        for ((id, preset), button) in DEFAULT_WORKSPACES.iter().zip(&self.switch_buttons) {
            let name = items
                .iter()
                .find(|i| i.id == *id)
                .map(|i| i.metadata.name.as_str())
                .unwrap_or(preset.name());
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_text(name);
            }
            button.set_tooltip_text(Some(&format!("Switch to {name} workspace")));
            button.update_property(&[gtk::accessible::Property::Label(name)]);
            button.set_active(active.as_deref() == Some(*id));
        }
        self.switcher.set_sensitive(
            self.manager.is_some()
                && self.ready.get()
                && !self.busy.get()
                && !self.switch_pending.get()
                && !self.validating_owner.get(),
        );
    }

    pub(super) fn bind_switcher(&self, w: &Rc<Workspace>) {
        for ((id, _), button) in DEFAULT_WORKSPACES.into_iter().zip(&self.switch_buttons) {
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
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
                            .is_none_or(|m| m.active_id().as_deref() == Some(id))
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
                                let stored = manager.load(id).await?;
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
    }
}
