//! Pinning and ordering affect app chrome, never the active workspace preview.
use super::*;
use layer_workspace::SwitcherEdit;

fn picked(row: &gtk::ListBoxRow, x: f64, y: f64) -> (bool, bool, bool) {
    let mut widget = row.pick(x, y, gtk::PickFlags::DEFAULT);
    let mut handle = false;
    let mut button = false;
    let mut name = false;
    while let Some(w) = widget {
        if w.is::<gtk::Button>() || w.is::<gtk::MenuButton>() || w.is::<gtk::CheckButton>() || w.is::<gtk::Editable>() {
            button = true;
        }
        handle |= w.has_css_class("workspace-reorder-handle");
        name |= w.has_css_class("workspace-name");
        if w == *row.upcast_ref::<gtk::Widget>() {
            break;
        }
        widget = w.parent();
    }
    (button && !handle, handle, name)
}

impl ManagerUi {
    pub(super) fn edit_switcher(&self, w: &Rc<Workspace>, edit: SwitcherEdit) {
        if !self.switcher_pending.get() {
            w.workspaces.send(w, WorkspaceInput::EditSwitcher { edit });
        }
    }

    pub(super) fn add_switcher_actions(
        &self,
        w: &Rc<Workspace>,
        popup: &gtk::Popover,
        id: &str,
    ) {
        let content = popup.child().and_downcast::<gtk::Box>().unwrap();
        for (label, up) in [
            (layer_ui::MessageId::NATIVE_HEADER_MOVE_UP, true),
            (layer_ui::MessageId::NATIVE_HEADER_MOVE_DOWN, false),
        ] {
            let button = gtk::Button::with_label(&w.localization().text(label));
            button.add_css_class("flat");
            let id = id.to_string();
            button.connect_map(glib::clone!(#[weak] w, #[strong] id, move |button| {
                let order = w.workspaces.view().order;
                button.set_label(&w.localization().text(label));
                button.set_sensitive(!w.workspaces.ui.switcher_pending.get() && order.iter().position(|row| row == &id)
                    .is_some_and(|index| if up { index > 0 } else { index + 1 < order.len() }));
            }));
            button.connect_clicked(glib::clone!(#[weak] w, #[weak] popup, #[strong] id, move |_| {
                popup.popdown();
                let order = w.workspaces.view().order;
                let Some(index) = order.iter().position(|row| row == &id) else { return; };
                let before = if up {
                    if index == 0 { return; }
                    order.get(index - 1).cloned()
                } else {
                    if index + 1 >= order.len() { return; }
                    order.get(index + 2).cloned()
                };
                w.workspaces.ui.edit_switcher(&w, SwitcherEdit::Move { id: id.clone(), before });
            }));
            content.append(&button);
        }
    }

    pub(super) fn bind_reorder_row(
        &self,
        w: &Rc<Workspace>,
        row: &gtk::ListBoxRow,
        id: &str,
        menu: &gtk::MenuButton,
        context: &gtk::Popover,
    ) {
        let popup = menu.popover().unwrap();
        popup.connect_closed(|p| p.set_pointing_to(None));
        context.connect_closed(|p| p.set_pointing_to(None));
        let popovers = [popup.clone(), context.clone()];
        // While a hold owns the contact, leave the popup ungrabbed so the
        // grouped DragSource can take over. On release, restore normal dismissal.
        let release = gtk::EventControllerLegacy::new();
        release.set_propagation_phase(gtk::PropagationPhase::Capture);
        release.connect_event(glib::clone!(
            #[strong]
            popovers,
            move |_, event| {
                for popup in &popovers { if matches!(
                    event.event_type(),
                    gdk::EventType::TouchEnd
                        | gdk::EventType::TouchCancel
                        | gdk::EventType::ButtonRelease
                ) && popup.is_visible()
                    && !popup.is_autohide()
                {
                    let position = popup.pointing_to();
                    popup.popdown();
                    popup.set_autohide(true);
                    if event.event_type() != gdk::EventType::TouchCancel {
                        popup.set_pointing_to(position.0.then_some(&position.1));
                        popup.popup();
                    }
                } }
                glib::Propagation::Proceed
            }
        ));
        row.add_controller(release);
        let secondary = gtk::GestureClick::new();
        secondary.set_button(3);
        secondary.set_propagation_phase(gtk::PropagationPhase::Capture);
        secondary.connect_pressed(glib::clone!(
            #[weak]
            row,
            #[weak]
            context,
            move |gesture, _, x, y| {
                if picked(&row, x, y).0 { return; }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                row_menu(&row, &context, x, y, true);
            }
        ));
        row.add_controller(secondary);
        let keys = gtk::EventControllerKey::new();
        let key_id = id.to_string();
        keys.connect_key_pressed(glib::clone!(#[weak] w, #[weak] row, #[weak] context, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, modifiers| {
            if gtk::prelude::GtkWindowExt::focus(&w.window).is_some_and(|widget| widget.is::<gtk::Editable>()) { return glib::Propagation::Proceed; }
            if key == gdk::Key::Menu || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK)) {
                row_menu(&row, &context, row.width() as f64 / 2., row.height() as f64 / 2., true);
                glib::Propagation::Stop
            } else if key == gdk::Key::F2 && let Some(action) = w.workspaces.ui.workspace_name_action(&w, &key_id) {
                w.workspaces.send(&w, WorkspaceInput::Action { action });
                glib::Propagation::Stop
            } else { glib::Propagation::Proceed }
        }));
        row.add_controller(keys);
        let held = Rc::new(Cell::new(false));
        let moved = Rc::new(Cell::new(false));
        let valid = Rc::new(Cell::new(false));
        let press_origin = Rc::new(Cell::new((0., 0.)));
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        source.set_propagation_phase(gtk::PropagationPhase::Capture);
        let id = id.to_string();
        source.connect_prepare(glib::clone!(
            #[weak]
            w,
            #[weak]
            row,
            #[strong]
            held,
            #[weak]
            popup,
            #[weak]
            context,
            #[strong]
            id,
            #[upgrade_or]
            None,
            move |source, x, y| {
                if w.workspaces.ui.switcher_pending.get() || picked(&row, x, y).0
                    || w.workspaces.ui.workspace_rows.borrow().get(&id).is_some_and(|row| !row.swipe.is_at_rest()) {
                    return None;
                }
                if crate::input::touch_or_pen(source) && !held.get() {
                    return None;
                }
                popup.popdown();
                context.popdown();
                popup.set_autohide(true);
                let color = w.gpu.borrow().as_ref()?.session.state().palette.panel;
                let preview = crate::layers::drag_preview(row.upcast_ref(), color);
                source.set_icon(preview.as_ref(), x as i32, y as i32);
                Some(gdk::ContentProvider::for_value(
                    &format!("capy-workspace:{id}").to_value(),
                ))
            }
        ));
        source.connect_drag_begin(glib::clone!(
            #[weak]
            w,
            #[weak]
            row,
            #[strong]
            id,
            #[strong]
            moved,
            move |_, _| {
                moved.set(true);
                *w.workspaces.ui.dragged.borrow_mut() = Some(id.clone());
                row.add_css_class("workspace-row-dragging");
            }
        ));
        source.connect_drag_end(glib::clone!(
            #[weak]
            w,
            #[weak]
            row,
            #[strong]
            held,
            move |_, _, _| {
                held.set(false);
                row.remove_css_class("workspace-row-dragging");
                *w.workspaces.ui.dragged.borrow_mut() = None;
                w.workspaces.ui.clear_reorder_hint();
                w.workspaces.render(&w);
            }
        ));
        row.add_controller(source.clone());

        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak]
            w,
            #[weak]
            row,
            #[strong]
            held,
            #[strong]
            moved,
            #[strong]
            valid,
            #[strong]
            id,
            #[strong]
            press_origin,
            move |g, count, x, y| {
                moved.set(false);
                press_origin.set((x, y));
                let (button, handle, name) = picked(&row, x, y);
                let ui = w.workspaces.ui.workspace_rows.borrow().get(&id).cloned();
                if let Some(ui) = ui.filter(|ui| !ui.swipe.is_at_rest()) {
                    valid.set(false);
                    if !row.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|picked| ui.swipe.is_action(&picked)) {
                        ui.swipe.reveal(false);
                        g.set_state(gtk::EventSequenceState::Claimed);
                    }
                    return;
                }
                if count == 2 && name && !button && let Some(action) = w.workspaces.ui.workspace_name_action(&w, &id) {
                    valid.set(false);
                    g.set_state(gtk::EventSequenceState::Claimed);
                    w.workspaces.send(&w, WorkspaceInput::Action { action });
                    return;
                }
                valid.set(!button);
                held.set(handle && !button);
                if button {
                    return;
                }
                let needs_hold = crate::input::touch_or_pen(g);
                // Touch/pen bodies remain available to scrolling until a hold. The handle
                // and mouse claim immediately, preserving the grouped DragSource.
                if !needs_hold || handle {
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        ));
        click.connect_released(glib::clone!(
            #[weak]
            w,
            #[weak]
            row,
            #[weak]
            menu,
            #[strong]
            moved,
            #[strong]
            valid,
            move |_, _, x, y| {
                if valid.replace(false)
                    && !moved.get()
                    && x >= 0.
                    && y >= 0.
                    && x < row.width() as f64
                    && y < row.height() as f64
                {
                    if picked(&row, x, y).1 {
                        menu.grab_focus();
                        row_menu(&row, &menu.popover().unwrap(), x, y, true);
                    } else {
                        if !w.workspaces.ui.finish_active_workspace_name(&w) { return; }
                        row.grab_focus();
                        w.workspaces.ui.list.select_row(Some(&row));
                    }
                }
            }
        ));
        click.connect_update(glib::clone!(
            #[weak]
            row,
            #[strong]
            press_origin,
            #[strong]
            valid,
            move |gesture, sequence| {
                if valid.get() && let Some((x, y)) = gesture.point(sequence) {
                    let (start_x, start_y) = press_origin.get();
                    if row.drag_check_threshold(start_x as i32, start_y as i32, x as i32, y as i32) { valid.set(false); }
                }
            }
        ));
        click.connect_cancel(glib::clone!(
            #[strong]
            valid,
            move |_, _| valid.set(false)
        ));
        row.add_controller(click.clone());
        click.group_with(&source);

        let hold = gtk::GestureLongPress::new();
        hold.set_touch_only(false);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed(glib::clone!(
            #[weak]
            row,
            #[weak]
            context,
            #[strong]
            held,
            #[strong]
            valid,
            move |g, x, y| {
                if crate::input::touch_or_pen(g) && !picked(&row, x, y).0 {
                    held.set(true);
                    valid.set(false);
                    g.set_state(gtk::EventSequenceState::Claimed);
                    row_menu(&row, &context, x, y, false);
                }
            }
        ));
        row.add_controller(hold.clone());
        hold.group_with(&source);
    }

    fn clear_reorder_hint(&self) {
        let mut child = self.list.first_child();
        while let Some(row) = child {
            row.remove_css_class("workspace-drop-before");
            row.remove_css_class("workspace-drop-after");
            child = row.next_sibling();
        }
    }

    fn reorder_target(&self, y: f64) -> Option<Option<String>> {
        self.clear_reorder_hint();
        let row = self
            .list
            .row_at_y(y as i32)
            .or_else(|| self.list.last_child().and_downcast::<gtk::ListBoxRow>())?;
        let id = self.rows.borrow().get(row.index() as usize)?.clone();
        let index = row.index() as usize;
        let bounds = row.compute_bounds(&self.list)?;
        let after = y > bounds.y() as f64 + row.height() as f64 / 2.;
        row.add_css_class(if after {
            "workspace-drop-after"
        } else {
            "workspace-drop-before"
        });
        Some(if after {
            self.rows.borrow().get(index + 1).cloned()
        } else {
            Some(id)
        })
    }

    pub(super) fn bind_reorder_drop(&self, w: &Rc<Workspace>) {
        let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        for enter in [true, false] {
            let callback = glib::clone!(
                #[weak]
                w,
                #[upgrade_or]
                gdk::DragAction::empty(),
                move |_: &gtk::DropTarget, _: f64, y: f64| {
                    if w.workspaces.ui.dragged.borrow().is_some()
                        && w.workspaces.ui.reorder_target(y).is_some()
                    {
                        gdk::DragAction::MOVE
                    } else {
                        gdk::DragAction::empty()
                    }
                }
            );
            if enter {
                drop.connect_enter(callback);
            } else {
                drop.connect_motion(callback);
            }
        }
        drop.connect_leave(glib::clone!(
            #[weak]
            w,
            move |_| w.workspaces.ui.clear_reorder_hint()
        ));
        drop.connect_drop(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            false,
            move |_, value, _, y| {
                let Some(id) = value
                    .get::<String>()
                    .ok()
                    .and_then(|s| s.strip_prefix("capy-workspace:").map(str::to_string))
                else {
                    return false;
                };
                if w.workspaces.ui.dragged.borrow().as_ref() != Some(&id) {
                    return false;
                }
                let Some(before) = w.workspaces.ui.reorder_target(y) else {
                    return false;
                };
                w.workspaces.ui.clear_reorder_hint();
                w.workspaces
                    .ui
                    .edit_switcher(&w, SwitcherEdit::Move { id, before });
                true
            }
        ));
        self.list.add_controller(drop);
    }
}

fn row_menu(row: &gtk::ListBoxRow, popup: &gtk::Popover, x: f64, y: f64, autohide: bool) {
    if let Some(p) = popup.parent().and_then(|parent| row.compute_point(&parent, &gtk::graphene::Point::new(x as f32, y as f32))) {
        popup.set_pointing_to(Some(&gdk::Rectangle::new(p.x() as i32, p.y() as i32, 1, 1)));
    }
    popup.set_autohide(autohide);
    popup.popup();
    popup.present();
}
