//! Native slop, contact capture and animated reorder feedback for retained color tiles.
//! The library changes only on a valid drop; cancellation needs no rollback.
use super::*;

pub(super) struct Host {
    window: glib::WeakRef<adw::ApplicationWindow>,
    keys: gtk::EventControllerKey,
    blur: Option<glib::SignalHandlerId>,
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Some(window) = self.window.upgrade() {
            window.remove_controller(&self.keys);
            window.disconnect(self.blur.take().unwrap());
        }
    }
}

pub(super) struct Contact {
    pub held: bool,
    source: gtk::Button,
    palette: u64,
    id: u64,
    origin: gtk::graphene::Point,
    point: gtk::graphene::Point,
    device: Option<gdk::Device>,
    cursor: Option<gdk::Cursor>,
    started: bool,
    action: Option<Action>,
    slot: Option<usize>,
    grab: gtk::graphene::Point,
    visual: Option<gtk::gsk::RenderNode>,
    tick: Option<gtk::TickCallbackId>,
    last_frame: i64,
}
impl PalettePanel {
    pub(super) fn bind_reorder(self: &Rc<Self>, w: &Rc<Workspace>) {
        // GtkGestureSingle resets on buttonless mouse motion even while a pen
        // owns its contact. Popup mapping can synthesize just such a hover.
        // Keep unrelated pointer devices out of the active gesture pair.
        let devices = gtk::EventControllerLegacy::new();
        devices.set_propagation_phase(gtk::PropagationPhase::Capture);
        devices.connect_event(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                if matches!(
                    event.event_type(),
                    gdk::EventType::MotionNotify
                        | gdk::EventType::ButtonPress
                        | gdk::EventType::ButtonRelease
                ) && panel
                    .drag
                    .borrow()
                    .as_ref()
                    .is_some_and(|d| d.device != event.device())
                {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        self.root.add_controller(devices);
        self.root.connect_unmap(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_| panel.cancel_reorder()
        ));
        let blur = w.window.connect_is_active_notify(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |window| {
                if !window.is_active() {
                    panel.cancel_reorder();
                }
            }
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if key == gdk::Key::Escape && panel.drag.borrow().is_some() {
                    panel.cancel_reorder();
                    return glib::Propagation::Stop;
                }
                if modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                    && matches!(key, gdk::Key::z | gdk::Key::Z | gdk::Key::y)
                {
                    let Some(w) = panel.workspace.borrow().upgrade() else {
                        return glib::Propagation::Proceed;
                    };
                    let focus = gtk::prelude::GtkWindowExt::focus(&w.window);
                    if !focus.is_some_and(|f| {
                        f.is_ancestor(&panel.root)
                            && !f.is::<gtk::Editable>()
                            && f.ancestor(gtk::Entry::static_type()).is_none()
                    }) {
                        return glib::Propagation::Proceed;
                    }
                    let palette = panel.library.borrow().as_ref().unwrap().active_palette().id;
                    let redo =
                        key == gdk::Key::y || modifiers.contains(gdk::ModifierType::SHIFT_MASK);
                    if panel
                        .library
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .can_undo_reorder(palette, redo)
                    {
                        panel.apply(if redo {
                            Action::RedoReorder { palette }
                        } else {
                            Action::UndoReorder { palette }
                        });
                    }
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        w.window.add_controller(keys.clone());
        *self.drag_host.borrow_mut() = Some(Host {
            window: w.window.downgrade(),
            keys,
            blur: Some(blur),
        });
    }
    pub(super) fn reorder_gesture(
        self: &Rc<Self>,
        tile: &gtk::Button,
        id: u64,
    ) -> gtk::GestureDrag {
        // Grouped with the tile's LongPress: GTK retains the actual device and
        // contact through menu presentation, motion and release. A raw widget
        // event stream is not a gesture grab and can mix pen/mouse events.
        // This gesture also delivers short taps. Disable the button's competing
        // primary-pointer recognizer; its keyboard/accessibility activation stays
        // native. Otherwise a pen release can click again after drag-end.
        let controllers = tile.observe_controllers();
        for index in 0..controllers.n_items() {
            if let Some(click) = controllers.item(index).and_downcast::<gtk::GestureClick>()
                && click.button() == 1
            {
                click.set_propagation_phase(gtk::PropagationPhase::None);
            }
        }
        let gesture = gtk::GestureDrag::new();
        gesture.set_button(1);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_drag_begin(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[weak]
            tile,
            move |g, x, y| {
                panel.cancel_reorder();
                let Some(origin) =
                    tile.compute_point(&panel.root, &gtk::graphene::Point::new(x as f32, y as f32))
                else {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                };
                let Some(palette) = panel
                    .library
                    .borrow()
                    .as_ref()
                    .map(|l| l.active_palette().id)
                else {
                    return;
                };
                *panel.drag.borrow_mut() = Some(Contact {
                    device: g.current_event_device(),
                    cursor: tile.cursor(),
                    source: tile.clone(),
                    palette,
                    id,
                    origin,
                    point: origin,
                    held: false,
                    started: false,
                    action: None,
                    slot: None,
                    grab: gtk::graphene::Point::new(x as f32, y as f32),
                    visual: None,
                    tick: None,
                    last_frame: 0,
                });
                // A tile owns this contact from the press. That prevents its
                // scrolling ancestors from winning the first movement. Release
                // below slop still activates the ordinary button action.
                tile.grab_focus();
                tile.set_state_flags(gtk::StateFlags::ACTIVE, false);
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        ));
        gesture.connect_drag_update(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            (),
            move |g, _, _| {
                let Some(point) = g
                    .current_event()
                    .and_then(|e| e.position())
                    .and_then(|(x, y)| crate::input::widget_point(&panel.root, x, y))
                else {
                    return;
                };
                let mut pending = panel.drag.borrow_mut();
                let Some(drag) = pending.as_mut().filter(|d| d.id == id) else {
                    return;
                };
                drag.point = point;
                if !drag.started {
                    if !panel.root.drag_check_threshold(
                        drag.origin.x() as i32,
                        drag.origin.y() as i32,
                        point.x() as i32,
                        point.y() as i32,
                    ) {
                        return;
                    }
                    drag.started = true;
                    let snapshot = gtk::Snapshot::new();
                    if let Some(bounds) = drag.source.compute_bounds(&panel.swatches) {
                        snapshot.translate(&gtk::graphene::Point::new(-bounds.x(), -bounds.y()));
                        snapshot.push_shadow(&[gtk::gsk::Shadow::new(
                            gdk::RGBA::new(0., 0., 0., 0.35),
                            0.,
                            3.,
                            8.,
                        )]);
                        panel.swatches.snapshot_child(&drag.source, &snapshot);
                        snapshot.pop();
                        drag.visual = snapshot.to_node();
                    }
                    drag.source.add_css_class("palette-drag-source");
                    drag.source.set_has_tooltip(false);
                    panel.context_menu.popdown();
                    panel.drag_cursor(drag, "grabbing");
                    let weak = Rc::downgrade(&panel);
                    drag.tick = Some(panel.root.add_tick_callback(move |_, clock| {
                        let Some(panel) = weak.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        panel.scroll_reorder(clock.frame_time());
                        glib::ControlFlow::Continue
                    }));
                }
                drop(pending);
                panel.update_reorder();
            }
        ));
        gesture.connect_drag_end(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |g, _, _| {
                if panel.drag.borrow().as_ref().is_some_and(|d| d.id == id) {
                    let released = g.current_event().is_some_and(|e| {
                        matches!(
                            e.event_type(),
                            gdk::EventType::ButtonRelease | gdk::EventType::TouchEnd
                        )
                    });
                    panel.finish_reorder(!released);
                }
            }
        ));
        gesture.connect_cancel(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_, _| {
                if panel.drag.borrow().as_ref().is_some_and(|d| d.id == id) {
                    panel.cancel_reorder();
                }
            }
        ));
        gesture
    }
    fn hit_slot(&self, point: gtk::graphene::Point) -> Option<usize> {
        if self.chooser.is_visible() || self.expanded.is_visible() {
            return None;
        }
        let scroll = self.swatches.ancestor(gtk::ScrolledWindow::static_type())?;
        if !scroll.compute_bounds(&self.root)?.contains_point(&point) {
            return None;
        }
        self.swatches
            .slot_at(self.root.compute_point(&self.swatches, &point)?)
    }
    pub(super) fn hold_swatch(&self, id: u64) -> bool {
        let mut pending = self.drag.borrow_mut();
        let Some(drag) = pending
            .as_mut()
            .filter(|d| d.id == id && !d.started && d.source.is_mapped())
        else {
            return false;
        };
        drag.held = true;
        drag.source.set_has_tooltip(false);
        self.drag_cursor(drag, "grab");
        true
    }
    fn drag_cursor(&self, drag: &Contact, name: &str) {
        if let Some(device) = &drag.device
            && device.source() != gdk::InputSource::Touchscreen
        {
            drag.source.set_cursor_from_name(Some(name));
            if let Some(w) = self.workspace.borrow().upgrade()
                && let Some(surface) = w.window.surface()
                && let Some(cursor) = gdk::Cursor::from_name(name, None)
            {
                surface.set_device_cursor(device, &cursor);
            }
        }
    }
    fn update_reorder(&self) {
        let mut pending = self.drag.borrow_mut();
        let Some(drag) = pending.as_mut() else {
            return;
        };
        if let Some(w) = self.workspace.borrow().upgrade() {
            w.drag_overlay_at(
                drag.visual.as_ref(),
                &self.root,
                gtk::graphene::Point::new(
                    drag.point.x() - drag.grab.x(),
                    drag.point.y() - drag.grab.y(),
                ),
            );
        }
        let slot = self.hit_slot(drag.point);
        if drag.slot == slot && self.swatches.is_reordering() {
            return;
        }
        drag.slot = slot;
        // Outside the grid the lifted color still follows the pointer, while
        // the grid returns to its original order and an outside release cancels.
        let library = self.library.borrow();
        let Some(library) = library.as_ref() else {
            return;
        };
        let original = library
            .active_palette()
            .swatches
            .iter()
            .position(|s| s.id == drag.id);
        let Some(preview) = slot
            .or(original)
            .and_then(|slot| library.preview_reorder(drag.palette, drag.id, slot))
        else {
            return;
        };
        drag.action = slot.and(preview.action);
        let tiles = self.tiles.borrow();
        let widgets: std::collections::HashMap<_, _> =
            tiles.iter().map(|(id, tile)| (*id, tile)).collect();
        let order: Vec<gtk::Widget> = preview
            .order
            .iter()
            .filter_map(|id| widgets.get(id))
            .map(|tile| (*tile).clone().upcast())
            .collect();
        self.swatches
            .preview_reorder(drag.source.upcast_ref(), &order);
    }
    fn scroll_reorder(&self, now: i64) {
        let (point, elapsed) = {
            let mut pending = self.drag.borrow_mut();
            let Some(drag) = pending.as_mut() else { return };
            let elapsed = if drag.last_frame == 0 {
                0.
            } else {
                (now - drag.last_frame).clamp(0, 50_000) as f64 / 1_000_000.
            };
            drag.last_frame = now;
            (drag.point, elapsed)
        };
        let Some(scroll) = self
            .swatches
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
        else {
            return;
        };
        let Some(bounds) = scroll.compute_bounds(&self.root) else {
            return;
        };
        if point.x() < bounds.x() || point.x() > bounds.x() + bounds.width() {
            return;
        }
        let delta = if point.y() < bounds.y() + 20. && point.y() >= bounds.y() - 12. {
            -240. * elapsed
        } else if point.y() > bounds.y() + bounds.height() - 20.
            && point.y() <= bounds.y() + bounds.height() + 12.
        {
            240. * elapsed
        } else {
            0.
        };
        if delta != 0. {
            let a = scroll.vadjustment();
            a.set_value(
                (a.value() + delta).clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower())),
            );
            self.update_reorder();
        }
    }
    pub(super) fn cancel_reorder(&self) {
        self.finish_reorder(true);
    }
    fn finish_reorder(&self, cancel: bool) {
        let Some(drag) = self.drag.borrow_mut().take() else {
            return;
        };
        if let Some(tick) = drag.tick {
            tick.remove();
        }
        if drag.started {
            self.swatches.clear_reorder();
            if let Some(w) = self.workspace.borrow().upgrade() {
                w.drag_overlay_at(None, &self.root, gtk::graphene::Point::zero());
            }
        }
        drag.source.remove_css_class("palette-drag-source");
        drag.source.unset_state_flags(gtk::StateFlags::ACTIVE);
        drag.source.set_has_tooltip(true);
        drag.source.set_cursor(drag.cursor.as_ref());
        if let Some(device) = &drag.device
            && device.source() != gdk::InputSource::Touchscreen
            && let Some(w) = self.workspace.borrow().upgrade()
            && let Some(surface) = w.window.surface()
            && let Some(cursor) = drag
                .cursor
                .or_else(|| gdk::Cursor::from_name("default", None))
        {
            surface.set_device_cursor(device, &cursor);
        }
        // The grouped native gestures finish their own sequence. Resetting
        // controllers here would mutate GTK's contact table during its end or
        // cancel callback (especially for tablet events).
        if drag.started || cancel {
            self.context_menu.popdown();
            self.context_menu.set_autohide(true);
        } else if drag.held && self.context_menu.is_visible() {
            self.context_menu.popdown();
            self.context_menu.set_autohide(true);
            self.context_menu.popup();
        }
        if !cancel
            && drag.started
            && let Some(action) = drag.action
            && let Some(w) = self.workspace.borrow().upgrade()
        {
            w.dispatch(UiAction::Color {
                action: ColorAction::Library { action },
            });
            drag.source.grab_focus();
        }
        if !cancel
            && !drag.started
            && !drag.held
            && drag
                .source
                .compute_bounds(&self.root)
                .is_some_and(|b| b.contains_point(&drag.point))
        {
            drag.source.emit_clicked();
        }
    }
}
