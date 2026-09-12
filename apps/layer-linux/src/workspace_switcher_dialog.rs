//! Pinning and ordering affect app chrome, never the active workspace preview.
use super::*;
use layer_workspace::SwitcherEdit;

fn picked(row: &adw::ActionRow, x: f64, y: f64) -> (bool, bool) {
    let mut widget = row.pick(x, y, gtk::PickFlags::DEFAULT);
    let mut handle = false;
    while let Some(w) = widget {
        if w.is::<gtk::Button>() || w.is::<gtk::MenuButton>() || w.is::<gtk::CheckButton>() {
            return (true, false);
        }
        handle |= w.has_css_class("workspace-reorder-handle");
        if w == *row.upcast_ref::<gtk::Widget>() {
            break;
        }
        widget = w.parent();
    }
    (false, handle)
}

impl ManagerUi {
    fn refresh_order(&self, w: &Rc<Workspace>) {
        if !self.presented.get()
            || self.page.get() != ManagerPage::Workspaces
            || self.dragged.borrow().is_some()
        {
            return;
        }
        let scroll = self
            .list
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>();
        let position = scroll.as_ref().map(|s| s.vadjustment().value());
        self.preserve_preview.set(true);
        self.rows(w);
        self.preserve_preview.set(false);
        if let (Some(scroll), Some(position)) = (scroll, position) {
            scroll.vadjustment().set_value(position);
        }
    }

    fn edit_switcher(&self, w: &Rc<Workspace>, edit: SwitcherEdit) {
        if self.switcher_pending.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let manager = w.workspaces.manager.as_ref().unwrap();
                let result = manager.edit_switcher(edit).await;
                w.workspaces.ui.switcher_pending.set(false);
                if let Err(error) = result {
                    w.workspaces.ui.error(&error.to_string());
                }
                w.workspaces.update_switcher();
                w.workspaces.ui.refresh_order(&w);
                // Update other windows in this app without changing their selection.
                let windows = WINDOWS.with(|windows| {
                    windows
                        .borrow()
                        .values()
                        .filter_map(|v| v.upgrade())
                        .collect::<Vec<_>>()
                });
                for other in windows {
                    if Rc::ptr_eq(&other, &w) {
                        continue;
                    }
                    if let Some(manager) = &other.workspaces.manager {
                        let _ = manager.refresh().await;
                        let _ = manager.refresh_switcher().await;
                        other.workspaces.update_switcher();
                        other.workspaces.ui.refresh_order(&other);
                    }
                }
            }
        ));
    }

    pub(super) fn add_switcher_actions(
        &self,
        w: &Rc<Workspace>,
        menu: &gtk::MenuButton,
        id: &str,
        pinned: &[String],
    ) {
        let popup = menu.popover().unwrap();
        let content = popup.child().and_downcast::<gtk::Box>().unwrap();
        let controls = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let check = gtk::CheckButton::with_label("Show in top bar");
        check.set_widget_name(&format!("workspace-pin-{id}"));
        check.set_active(pinned.iter().any(|i| i == id));
        let id = id.to_string();
        check.connect_toggled(glib::clone!(
            #[weak]
            w,
            #[weak]
            popup,
            #[strong]
            id,
            move |check| {
                popup.popdown();
                w.workspaces.ui.edit_switcher(
                    &w,
                    SwitcherEdit::Show {
                        id: id.clone(),
                        visible: check.is_active(),
                    },
                );
            }
        ));
        controls.append(&check);
        if let Some(index) = pinned.iter().position(|i| i == &id) {
            for (label, enabled, before) in [
                (
                    "Move Up",
                    index > 0,
                    index.checked_sub(1).and_then(|i| pinned.get(i)).cloned(),
                ),
                (
                    "Move Down",
                    index + 1 < pinned.len(),
                    pinned.get(index + 2).cloned(),
                ),
            ] {
                let button = gtk::Button::with_label(label);
                button.add_css_class("flat");
                button.set_sensitive(enabled);
                button.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    #[weak]
                    popup,
                    #[strong]
                    id,
                    move |_| {
                        popup.popdown();
                        w.workspaces.ui.edit_switcher(
                            &w,
                            SwitcherEdit::Move {
                                id: id.clone(),
                                before: before.clone(),
                            },
                        );
                    }
                ));
                controls.append(&button);
            }
        }
        controls.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.prepend(&controls);
    }

    pub(super) fn bind_reorder_row(&self, w: &Rc<Workspace>, row: &adw::ActionRow, id: &str) {
        let held = Rc::new(Cell::new(false));
        let moved = Rc::new(Cell::new(false));
        let valid = Rc::new(Cell::new(false));
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
            #[strong]
            id,
            #[upgrade_or]
            None,
            move |source, x, y| {
                if w.workspaces.ui.switcher_pending.get() || picked(&row, x, y).0 {
                    return None;
                }
                if source.current_event_device().is_some_and(|d| {
                    matches!(
                        d.source(),
                        gdk::InputSource::Touchscreen | gdk::InputSource::Pen
                    )
                }) && !held.get()
                {
                    return None;
                }
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
            }
        ));
        row.add_controller(source.clone());

        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak]
            row,
            #[strong]
            held,
            #[strong]
            moved,
            #[strong]
            valid,
            move |g, _, x, y| {
                moved.set(false);
                let (button, handle) = picked(&row, x, y);
                valid.set(!button);
                held.set(handle && !button);
                if button {
                    return;
                }
                let needs_hold = g.current_event_device().is_some_and(|d| {
                    matches!(
                        d.source(),
                        gdk::InputSource::Touchscreen | gdk::InputSource::Pen
                    )
                });
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
            #[strong]
            moved,
            #[strong]
            valid,
            move |_, _, x, y| {
                if valid.get()
                    && !moved.get()
                    && x >= 0.
                    && y >= 0.
                    && x < row.width() as f64
                    && y < row.height() as f64
                {
                    w.workspaces.ui.list.select_row(Some(&row));
                }
            }
        ));
        click.connect_stopped(glib::clone!(
            #[strong]
            valid,
            move |_| valid.set(false)
        ));
        row.add_controller(click.clone());
        click.group_with(&source);

        let hold = gtk::GestureLongPress::new();
        hold.set_touch_only(false);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed(glib::clone!(
            #[weak]
            row,
            #[strong]
            held,
            #[strong]
            valid,
            move |g, x, y| {
                if !picked(&row, x, y).0 {
                    held.set(true);
                    valid.set(false);
                    g.set_state(gtk::EventSequenceState::Claimed);
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

    fn reorder_target(&self, w: &Workspace, y: f64) -> Option<Option<String>> {
        let pinned = w.workspaces.manager.as_ref()?.switcher_ids();
        self.clear_reorder_hint();
        let row = self
            .list
            .row_at_y(y as i32)
            .or_else(|| self.list.last_child().and_downcast::<gtk::ListBoxRow>())?;
        let id = self.rows.borrow().get(row.index() as usize)?.clone();
        let Some(index) = pinned.iter().position(|i| i == &id) else {
            let first = self
                .rows
                .borrow()
                .iter()
                .position(|i| !pinned.contains(i))?;
            self.list
                .row_at_index(first as i32)?
                .add_css_class("workspace-drop-before");
            return Some(None);
        };
        let bounds = row.compute_bounds(&self.list)?;
        let after = y > bounds.y() as f64 + row.height() as f64 / 2.;
        row.add_css_class(if after {
            "workspace-drop-after"
        } else {
            "workspace-drop-before"
        });
        Some(if after {
            pinned.get(index + 1).cloned()
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
                        && w.workspaces.ui.reorder_target(&w, y).is_some()
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
                let Some(before) = w.workspaces.ui.reorder_target(&w, y) else {
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
