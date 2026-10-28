use super::*;

#[derive(Clone, Copy)]
struct ActiveEdge {
    id: u64,
    target: EdgeTarget,
    grab: EdgeGrab,
    viewport: [f32; 2],
    point: [f32; 2],
    start_ns: Option<u64>,
    settled: bool,
}

#[derive(Default)]
pub(crate) struct EdgeContacts {
    pending: Option<PendingEdge>,
    active: Option<ActiveEdge>,
}

#[derive(Clone, Copy)]
struct PendingEdge {
    id: u64,
    kind: PointerKind,
    grab: EdgeGrab,
}

fn edge_action(target: EdgeTarget, phase: ContactPhase, position: [f32; 2], viewport: [f32; 2]) -> UiAction {
    match target {
        EdgeTarget::Divider { id, .. } => UiAction::DragDivider { id, phase, position, viewport },
        EdgeTarget::Floating { group, edge } => UiAction::ResizeFloating { group, edge, phase, position, viewport },
    }
}

fn elapsed_ms(start_ns: Option<u64>, now_ns: u64) -> f32 {
    start_ns.map_or(0., |start| now_ns.saturating_sub(start) as f32 / 1_000_000.)
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(crate) fn edge_catching_up(&self) -> bool {
        self.edge_contacts.active.is_some_and(|active| !active.settled)
    }

    pub(crate) fn edge_contact_input(&mut self, input: UiInput) -> Result<InputReply, String> {
        let mut reply = InputReply::default();
        if self.workspace_transition || self.workspace_read_only {
            reply.change = self.cancel_edge_contact()?;
            return Ok(reply);
        }
        if let UiInput::EdgeDoublePress { kind, contact, previous_position, position, viewport } = input {
            let layout = self.layout(viewport);
            if contact == EdgeContact::Free && let Some(id) = layout.edge_reset_at(position, kind)
                && previous_position.is_none_or(|point| layout.edge_reset_at(point, kind) == Some(id)) {
                reply.change = self.dispatch(UiAction::ResetColumnWidth { id, viewport })?;
                reply.handled = true;
            }
            return Ok(reply);
        }
        let UiInput::EdgeContact { id, phase, kind, contact, position, viewport, time_ns } = input else {
            return Ok(reply);
        };
        if phase == ContactPhase::Down && let Some(active) = self.edge_contacts.active.filter(|active| active.id == id) {
            self.edge_contacts.active = None;
            reply.change = self.dispatch(edge_action(active.target, ContactPhase::Cancel, active.point, active.viewport))?;
        }
        if let Some(mut active) = self.edge_contacts.active.filter(|active| active.id == id) {
            active.point = position;
            reply.handled = true;
            reply.edge_cursor = Some(active.target.cursor());
            reply.change = if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                self.edge_contacts.active = None;
                self.dispatch(edge_action(active.target, phase, position, active.viewport))?
            } else {
                let point = active.grab.position(position, elapsed_ms(active.start_ns, time_ns));
                self.edge_contacts.active = Some(active);
                self.dispatch(edge_action(active.target, ContactPhase::Move, point, active.viewport))?
            };
            return Ok(reply);
        }
        let pending = self.edge_contacts.pending.take();
        match phase {
            ContactPhase::Down => {
                if pending.is_some_and(|pending| pending.id != id) {
                    self.edge_contacts.pending = pending.filter(|_| kind != PointerKind::Touch);
                    reply.edge_retired = self.edge_contacts.pending.is_none();
                    return Ok(reply);
                }
                if self.edge_contacts.active.is_none() {
                    self.edge_contacts.pending = self.layout(viewport).edge_grab_at(position, kind, contact).map(|grab| PendingEdge { id, kind, grab });
                }
                reply.edge_pending = self.edge_contacts.pending.is_some_and(|pending| pending.id == id);
                reply.edge_select_on_release = reply.edge_pending && self.edge_contacts.pending.is_some_and(|pending| pending.grab.resizes(Axis::Vertical));
            }
            ContactPhase::Move => {
                let Some(mut pending) = pending.filter(|pending| pending.id == id && pending.kind == kind) else {
                    self.edge_contacts.pending = pending;
                    return Ok(reply);
                };
                match pending.grab.track(position) {
                    EdgeGrabProgress::Pending => {
                        reply.edge_pending = true;
                        reply.edge_hold = pending.grab.resizes(Axis::Vertical);
                        self.edge_contacts.pending = Some(pending);
                    }
                    EdgeGrabProgress::Abandoned => {}
                    EdgeGrabProgress::Claimed(target) => {
                        reply.change = self.dispatch(edge_action(target, ContactPhase::Down, pending.grab.anchor(position), viewport))?;
                        self.edge_contacts.active = Some(ActiveEdge {
                            id,
                            target,
                            grab: pending.grab,
                            viewport,
                            point: position,
                            start_ns: (time_ns > 0).then_some(time_ns),
                            settled: false,
                        });
                        reply.handled = true;
                        reply.change.canvas_wake = true;
                        reply.edge_cursor = Some(target.cursor());
                    }
                }
            }
            ContactPhase::Up | ContactPhase::Cancel => {
                self.edge_contacts.pending = pending.filter(|pending| pending.id != id);
            }
        }
        Ok(reply)
    }

    pub(crate) fn advance_edge_contact(&mut self, now_ns: u64) -> Result<UiChange, String> {
        let Some(mut active) = self.edge_contacts.active.filter(|active| !active.settled) else {
            return Ok(UiChange::default());
        };
        let start = *active.start_ns.get_or_insert(now_ns);
        let elapsed = elapsed_ms(Some(start), now_ns);
        let point = active.grab.position(active.point, elapsed);
        active.settled = elapsed >= EDGE_GRAB_CATCH_UP_MS;
        self.edge_contacts.active = Some(active);
        self.dispatch(edge_action(active.target, ContactPhase::Move, point, active.viewport))
    }

    pub(crate) fn cancel_edge_contact(&mut self) -> Result<UiChange, String> {
        self.edge_contacts.pending = None;
        if let Some(active) = self.edge_contacts.active.take() {
            return self.dispatch(edge_action(active.target, ContactPhase::Cancel, active.point, active.viewport));
        }
        Ok(UiChange::default())
    }
}

#[cfg(test)]
#[path = "edge_contact_tests.rs"]
mod tests;
