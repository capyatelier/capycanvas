//! Configurable header choices use normal workspace switching and ownership.
use super::*;
use layer_workspace::DEFAULT_WORKSPACES;

pub(super) fn build() -> (gtk::Box, gtk::Box, gtk::MenuButton) {
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
    let options = gtk::MenuButton::builder()
        .child(&crate::icons::image("layer-more-symbolic"))
        .build();
    options.set_widget_name("workspace-switcher-options");
    options.add_css_class("workspace-options");
    root.append(&options);
    (root, buttons, options)
}

impl NativeWorkspaces {
    pub fn switcher_popup(&self, w: &Rc<Workspace>) -> gtk::PopoverMenu {
        let popup = gtk::PopoverMenu::from_model(gtk::gio::MenuModel::NONE);
        popup.set_widget_name("workspace-switcher-popup");
        w.populate_workspace_menu(&popup, self.view().switcher_menu);
        w.watch_popover(popup.upcast_ref());
        popup
    }

    fn options_popup(&self, w: &Rc<Workspace>) -> gtk::PopoverMenu {
        let popup = gtk::PopoverMenu::from_model(gtk::gio::MenuModel::NONE);
        popup.set_widget_name("workspace-switcher-options-popup");
        self.populate_options(w, &popup);
        w.watch_popover(popup.upcast_ref());
        popup
    }

    fn populate_options(&self, w: &Rc<Workspace>, popup: &gtk::PopoverMenu) {
        let menu = self.view().switcher_options;
        let title = menu.title.clone();
        let model = w.workspace_menu_model(popup, menu);
        let labeled = gtk::gio::Menu::new();
        if let Some(checklist) = model.item_link(0, gtk::gio::MENU_LINK_SECTION) {
            labeled.append_section(Some(&title), &checklist);
        }
        for index in 1..model.n_items() {
            labeled.append_item(&gtk::gio::MenuItem::from_model(&model, index));
        }
        popup.set_menu_model(Some(&labeled));
    }

    pub(crate) fn show_options(&self, w: &Rc<Workspace>, widget: &gtk::Widget, x: f64, y: f64, held: bool) {
        let popup = &self.switch_context;
        self.populate_options(w, popup);
        popup.set_autohide(!held);
        let mut picked = widget.pick(x, y, gtk::PickFlags::DEFAULT);
        let source = loop {
            let Some(current) = picked else { break None; };
            if current.is_focusable() { break Some(current); }
            if current == *widget { break None; }
            picked = current.parent();
        };
        *self.switch_context_focus.borrow_mut() = source.or_else(|| gtk::prelude::GtkWindowExt::focus(&w.window)
            .filter(|focus| focus == widget || focus.is_ancestor(widget))
            .or_else(|| widget.is_focusable().then(|| widget.clone())))
            .map(|focus| focus.downgrade());
        w.popup_at(popup.upcast_ref(), widget, [x as f32, y as f32]);
    }

    pub(crate) fn finish_options_hold(&self, cancelled: bool) {
        let popup = &self.switch_context;
        if popup.is_visible() && !popup.is_autohide() {
            popup.popdown();
            popup.set_autohide(true);
            if !cancelled { popup.popup(); }
        }
    }

    pub(crate) fn install_switcher_context(&self, w: &Rc<Workspace>, widget: &impl IsA<gtk::Widget>) {
        widget.add_css_class("customizable-target");
        let click = gtk::GestureClick::new();
        click.set_button(3);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(#[weak] w, move |gesture, _, x, y| {
            let Some(widget) = gesture.widget() else { return; };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            w.workspaces.show_options(&w, &widget, x, y, false);
        }));
        widget.add_controller(click);
        let hold = gtk::GestureLongPress::new();
        hold.set_touch_only(false);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed(glib::clone!(#[weak] w, move |gesture, x, y| {
            if !crate::input::touch_or_pen(gesture) { return; }
            let Some(widget) = gesture.widget() else { return; };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            w.workspaces.show_options(&w, &widget, x, y, true);
        }));
        widget.add_controller(hold);
        let release = gtk::EventControllerLegacy::new();
        release.set_propagation_phase(gtk::PropagationPhase::Capture);
        release.connect_event(glib::clone!(#[weak] w, #[upgrade_or] glib::Propagation::Proceed, move |_, event| {
            if matches!(event.event_type(), gdk::EventType::TouchEnd | gdk::EventType::TouchCancel | gdk::EventType::ButtonRelease) {
                w.workspaces.finish_options_hold(event.event_type() == gdk::EventType::TouchCancel);
            }
            glib::Propagation::Proceed
        }));
        widget.add_controller(release);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(#[weak] w, #[upgrade_or] glib::Propagation::Proceed, move |controller, key, _, modifiers| {
            if key == gdk::Key::Menu || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK)) {
                if let Some(widget) = controller.widget() {
                    w.workspaces.show_options(&w, &widget, 0., widget.height() as f64, false);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }));
        widget.add_controller(keys);
    }

    pub(super) fn bind_switcher(&self, w: &Rc<Workspace>) {
        *self.switch_owner.borrow_mut() = Rc::downgrade(w);
        self.switch_context.set_widget_name("workspace-switcher-options-context");
        self.switch_context.set_has_arrow(false);
        self.switch_context.connect_closed(glib::clone!(#[weak] w, move |_| {
            if let Some(source) = w.workspaces.switch_context_focus.take().and_then(|source| source.upgrade())
                .filter(|source| source.is_mapped())
                && w.window.visible_dialog().is_none() { source.grab_focus(); }
        }));
        self.switch_options.set_create_popup_func(glib::clone!(#[weak] w, move |button| {
            button.set_popover(Some(&w.workspaces.options_popup(&w)));
        }));
        self.install_switcher_context(w, &self.switcher);
        self.update_switcher();
    }
    pub(super) fn update_switcher(&self) {
        let Some(w) = self.switch_owner.borrow().upgrade() else {
            return;
        };
        let view = self.view();
        self.switch_options.set_tooltip_text(Some(&view.switcher_options_label));
        self.switch_options.update_property(&[gtk::accessible::Property::Label(&view.switcher_options_label)]);
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
        self.switcher.set_visible(view.ready);
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
