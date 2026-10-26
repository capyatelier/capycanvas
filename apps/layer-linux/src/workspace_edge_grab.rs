use super::*;

#[derive(Clone)]
struct PendingEdge {
    id: u64,
    origin: [f32; 2],
    sequence: Option<gdk::EventSequence>,
    canvas: bool,
}

#[derive(Default)]
pub(super) struct EdgeGrabs {
    pending: RefCell<Option<PendingEdge>>,
    next: Cell<u64>,
    select_on_release: Cell<bool>,
}

impl Workspace {
    pub(super) fn edge_input(self: &Rc<Self>, id: u64, phase: ContactPhase, kind: PointerKind, contact: EdgeContact, position: [f32; 2]) -> InputReply {
        self.interact(UiInput::EdgeContact {
            id,
            phase,
            kind,
            contact,
            position,
            viewport: [self.surface.width() as f32, self.surface.height() as f32],
            time_ns: glib::monotonic_time().max(0) as u64 * 1000,
        })
    }

    pub(super) fn edge_grab_press(self: &Rc<Self>, event: &gdk::Event, point: [f32; 2], sequence: Option<gdk::EventSequence>, starting: bool) {
        self.edge_grabs.select_on_release.set(false);
        let pending = self.edge_grabs.pending.borrow().clone();
        if let Some(pending) = pending {
            if sequence.is_some() {
                self.edge_grabs.pending.take();
                self.edge_input(pending.id, ContactPhase::Cancel, PointerKind::Touch, EdgeContact::Free, point);
            }
            return;
        }
        let Some((contact, canvas)) = self.edge_contact_at(point).filter(|_| starting) else { return };
        let id = self.edge_grabs.next.get() + 1;
        self.edge_grabs.next.set(id);
        let reply = self.edge_input(id, ContactPhase::Down, crate::input::pointer_kind(event), contact, point);
        self.edge_grabs.select_on_release.set(reply.edge_select_on_release);
        if reply.edge_pending {
            *self.edge_grabs.pending.borrow_mut() = Some(PendingEdge { id, origin: point, sequence, canvas });
        }
    }

    pub(super) fn edge_grab_motion(self: &Rc<Self>, event: &gdk::Event, point: [f32; 2], sequence: Option<gdk::EventSequence>) -> bool {
        let Some(pending) = self.edge_grabs.pending.borrow().clone().filter(|p| p.sequence == sequence) else {
            return false;
        };
        let kind = crate::input::pointer_kind(event);
        if self.workspace_drag.borrow().as_ref().is_some_and(|drag| drag.started || drag.held) {
            self.edge_grabs.pending.take();
            self.edge_input(pending.id, ContactPhase::Cancel, kind, EdgeContact::Free, point);
            return false;
        }
        let reply = self.edge_input(pending.id, ContactPhase::Move, kind, EdgeContact::Free, point);
        let Some(cursor) = reply.edge_cursor else {
            if !reply.edge_pending {
                self.edge_grabs.pending.take();
            }
            return reply.edge_hold;
        };
        self.edge_grabs.pending.take();
        let held = self.workspace_drag.borrow_mut().take();
        if let Some(held) = held {
            self.restore_drag_cursor(&held);
        }
        self.dismiss_context();
        if pending.canvas && let Some(sequence) = &pending.sequence {
            self.input.cancel_touch(self, sequence, event.time());
        }
        let surface: gtk::Widget = self.surface.clone().upcast();
        let mut drag = NativeWorkspaceDrag {
            target: DragTarget::Edge(pending.id, cursor),
            origin: pending.origin,
            point,
            started: true,
            held: false,
            context: false,
            parent: surface.parent(),
            source: surface,
            wait_for_hold: false,
            tear: None,
            pull: false,
            tearing: false,
            device: if pending.sequence.is_some() { None } else { event.device() },
            sequence: pending.sequence,
            cursor: None,
            tab: None,
            tab_grab: None,
        };
        self.reset_drag_recognizers(&drag);
        self.set_drag_cursor(&mut drag, cursor);
        *self.workspace_drag.borrow_mut() = Some(drag);
        true
    }

    pub(super) fn edge_grab_release(self: &Rc<Self>, phase: ContactPhase, point: [f32; 2], sequence: &Option<gdk::EventSequence>) {
        let pending = self.edge_grabs.pending.borrow().clone().filter(|p| p.sequence == *sequence);
        if let Some(pending) = pending {
            self.edge_grabs.pending.take();
            self.edge_input(pending.id, phase, PointerKind::Mouse, EdgeContact::Free, point);
        }
    }

    pub(super) fn edge_select_on_release(&self) -> bool {
        self.edge_grabs.select_on_release.get()
    }

    fn surface_slot(&self, widget: &gtk::Widget) -> Option<Slot> {
        let surface: gtk::Widget = self.surface.clone().upcast();
        let mut child = widget.clone();
        while let Some(parent) = child.parent().filter(|p| *p != surface) {
            child = parent;
        }
        self.surface.imp().children.borrow().iter().find(|(_, w)| *w == child).map(|(slot, _)| *slot)
    }

    pub(super) fn edge_contact_at(&self, point: [f32; 2]) -> Option<(EdgeContact, bool)> {
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
        Some((contact, slot == Slot::Canvas))
    }

    pub(super) fn edge_reset_contact(&self, point: [f32; 2]) -> Option<EdgeContact> {
        let (contact, canvas) = self.edge_contact_at(point)?;
        let button = !canvas
            && std::iter::successors(self.surface.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT), |w| w.parent())
                .any(|w| w.is::<gtk::Button>());
        Some(if button { EdgeContact::Pickup } else { contact })
    }
}
