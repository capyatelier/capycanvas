use super::*;

#[derive(Clone)]
pub(super) struct PendingEdgeGrab {
    grab: EdgeGrab,
    origin: [f32; 2],
    sequence: Option<gdk::EventSequence>,
    canvas: bool,
}

#[derive(Default)]
pub(super) struct EdgeGrabs {
    pending: RefCell<Option<PendingEdgeGrab>>,
    follow: RefCell<Option<gtk::TickCallbackId>>,
}

#[derive(Clone, Copy)]
pub(super) struct NativeEdgeGrab {
    grab: EdgeGrab,
    started: Option<std::time::Instant>,
    pub(super) cursor: &'static str,
}
impl NativeEdgeGrab {
    fn elapsed_ms(self) -> f32 {
        self.started.map_or(EDGE_GRAB_CATCH_UP_MS, |t| t.elapsed().as_secs_f32() * 1000.)
    }
    pub(super) fn position(&mut self, point: [f32; 2]) -> [f32; 2] {
        let elapsed = self.elapsed_ms();
        self.grab.position(point, elapsed)
    }
}

impl Workspace {
    pub(super) fn edge_grab_press(&self, event: &gdk::Event, point: [f32; 2], sequence: Option<gdk::EventSequence>, starting: bool) {
        if self.edge_grabs.pending.borrow().is_some() {
            if sequence.is_some() {
                self.edge_grabs.pending.take();
            }
        } else if starting {
            *self.edge_grabs.pending.borrow_mut() = self.edge_grab_candidate(crate::input::pointer_kind(event), point, sequence);
        }
    }

    pub(super) fn edge_grab_motion(self: &Rc<Self>, event: &gdk::Event, point: [f32; 2], sequence: Option<gdk::EventSequence>) -> bool {
        let Some(mut pending) = self.edge_grabs.pending.borrow().clone().filter(|p| p.sequence == sequence) else {
            return false;
        };
        if self.workspace_drag.borrow().as_ref().is_some_and(|drag| drag.started || drag.held) {
            self.edge_grabs.pending.take();
            return false;
        }
        match pending.grab.track(point) {
            EdgeGrabProgress::Pending => {
                let hold = pending.grab.resizes(Axis::Vertical);
                *self.edge_grabs.pending.borrow_mut() = Some(pending);
                hold
            }
            EdgeGrabProgress::Abandoned => {
                self.edge_grabs.pending.take();
                false
            }
            EdgeGrabProgress::Claimed(target) => {
                self.edge_grabs.pending.take();
                self.claim_edge_grab(event, pending, target, point)
            }
        }
    }

    pub(super) fn edge_grab_release(&self, sequence: &Option<gdk::EventSequence>) {
        if self.edge_grabs.pending.borrow().as_ref().is_some_and(|p| p.sequence == *sequence) {
            self.edge_grabs.pending.take();
        }
    }

    pub(super) fn edge_reset_divider(&self, point: [f32; 2], kind: PointerKind) -> Option<u32> {
        let picked = self.surface.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)?;
        let slot = self.surface_slot(&picked)?;
        let surface: gtk::Widget = self.surface.clone().upcast();
        let mut widget = Some(picked).filter(|_| slot != Slot::Canvas);
        while let Some(w) = widget.filter(|w| *w != surface) {
            if w.is::<gtk::Button>() || w.is::<crate::swipe_row::SwipeRow>() || crate::input::swipe_control(&w) {
                return None;
            }
            widget = w.parent();
        }
        matches!(slot, Slot::Canvas | Slot::Group(_) | Slot::Column(_))
            .then(|| self.resolved().edge_reset_at(point, kind))
            .flatten()
    }

    pub(super) fn edge_gap_pending(&self) -> bool {
        self.edge_grabs.pending.borrow().as_ref().is_some_and(|p| p.grab.resizes(Axis::Vertical))
    }

    fn surface_slot(&self, widget: &gtk::Widget) -> Option<Slot> {
        let surface: gtk::Widget = self.surface.clone().upcast();
        let mut child = widget.clone();
        while let Some(parent) = child.parent().filter(|p| *p != surface) {
            child = parent;
        }
        self.surface.imp().children.borrow().iter().find(|(_, w)| *w == child).map(|(slot, _)| *slot)
    }

    pub(super) fn edge_grab_candidate(&self, kind: PointerKind, point: [f32; 2], sequence: Option<gdk::EventSequence>) -> Option<PendingEdgeGrab> {
        if self.header.editing.get() {
            return None;
        }
        let picked = self.surface.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)?;
        let slot = self.surface_slot(&picked)?;
        if !matches!(slot, Slot::Canvas | Slot::Group(_) | Slot::Column(_)) {
            return None;
        }
        let mut contact = EdgeContact::Free;
        if let Some(drag) = self.workspace_drag.borrow().as_ref() {
            if !matches!(drag.target, DragTarget::Dock(DockItem::Panel { .. } | DockItem::Group { .. } | DockItem::Tile { .. }) | DragTarget::Tear(_)) {
                return None;
            }
            contact = if matches!(drag.target, DragTarget::Dock(DockItem::Tile { .. })) && drag.pull {
                EdgeContact::Input
            } else if !drag.wait_for_hold || drag.pull || drag.source.has_css_class("drag-row") {
                EdgeContact::Pickup
            } else {
                EdgeContact::Free
            };
        }
        let surface: gtk::Widget = self.surface.clone().upcast();
        let mut widget = Some(picked).filter(|_| slot != Slot::Canvas);
        while let Some(w) = widget.filter(|w| *w != surface) {
            if crate::input::swipe_control(&w) {
                contact = EdgeContact::Input;
                break;
            }
            if contact == EdgeContact::Free
                && (w.is::<crate::swipe_row::SwipeRow>()
                    || w.has_css_class("drag-row")
                    || w.downcast_ref::<gtk::ScrolledWindow>()
                        .is_some_and(|s| s.hadjustment().upper() - s.hadjustment().lower() > s.hadjustment().page_size()))
            {
                contact = EdgeContact::Pickup;
            }
            widget = w.parent();
        }
        let grab = self.resolved().edge_grab_at(point, kind, contact)?;
        Some(PendingEdgeGrab { grab, origin: point, sequence, canvas: slot == Slot::Canvas })
    }

    fn claim_edge_grab(self: &Rc<Self>, event: &gdk::Event, pending: PendingEdgeGrab, target: EdgeTarget, point: [f32; 2]) -> bool {
        let (drag_target, slot) = match target {
            EdgeTarget::Divider { id, .. } => (DragTarget::Divider(id), Slot::Divider(id)),
            EdgeTarget::Floating { group, edge } => (DragTarget::Resize(group, edge), Slot::FloatingResize(group, edge)),
        };
        let handle = self.surface.imp().children.borrow().iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, widget)| widget.clone());
        let Some(handle) = handle.filter(|h| h.can_target()) else { return false };
        let held = self.workspace_drag.borrow_mut().take();
        if let Some(held) = held {
            self.restore_drag_cursor(&held);
        }
        self.dismiss_context();
        if pending.canvas && let Some(sequence) = &pending.sequence {
            self.input.cancel_touch(self, sequence, event.time());
        }
        let animate = gtk::Settings::for_display(&self.surface.display()).is_gtk_enable_animations();
        let grab = NativeEdgeGrab { grab: pending.grab, started: animate.then(std::time::Instant::now), cursor: target.cursor() };
        let mut drag = NativeWorkspaceDrag {
            target: drag_target,
            origin: pending.origin,
            point,
            started: true,
            held: false,
            context: false,
            parent: handle.parent(),
            source: handle,
            wait_for_hold: false,
            tear: None,
            pull: false,
            tearing: false,
            device: if pending.sequence.is_some() { None } else { event.device() },
            sequence: pending.sequence,
            cursor: None,
            tab: None,
            tab_grab: None,
            grab: Some(grab),
        };
        self.reset_drag_recognizers(&drag);
        self.set_drag_cursor(&mut drag, grab.cursor);
        *self.workspace_drag.borrow_mut() = Some(drag);
        self.dispatch_drag(drag_target, ContactPhase::Down, pending.grab.anchor(point));
        self.follow_edge_grab();
        true
    }

    fn follow_edge_grab(self: &Rc<Self>) {
        if let Some(previous) = self.edge_grabs.follow.take() {
            previous.remove();
        }
        let id = self.surface.add_tick_callback(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |_, _| {
                let drag = w.workspace_drag.borrow().clone();
                let Some((mut drag, mut grab)) = drag.and_then(|d| d.grab.map(|grab| (d, grab))) else {
                    w.edge_grabs.follow.take();
                    return glib::ControlFlow::Break;
                };
                let position = grab.position(drag.point);
                if let Some(current) = w.workspace_drag.borrow_mut().as_mut() {
                    current.grab = Some(grab);
                }
                w.dispatch_drag(drag.target, ContactPhase::Move, position);
                w.set_drag_cursor(&mut drag, grab.cursor);
                if grab.elapsed_ms() < EDGE_GRAB_CATCH_UP_MS {
                    glib::ControlFlow::Continue
                } else {
                    w.edge_grabs.follow.take();
                    glib::ControlFlow::Break
                }
            }
        ));
        *self.edge_grabs.follow.borrow_mut() = Some(id);
    }
}
