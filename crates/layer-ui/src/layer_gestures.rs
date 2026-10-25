use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Contact {
    row: u64,
    origin: [f32; 2],
    position: [f32; 2],
    time_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LayerGesture {
    Opacity(u64),
    Contents(u64),
    Merge(u64, u64),
}

#[derive(Default)]
pub(crate) struct LayerTouches {
    surface: u64,
    pub epoch: u64,
    contacts: BTreeMap<u64, Contact>,
    pair: Option<[Contact; 2]>,
    first_ns: u64,
    last_ns: u64,
    lift_ns: Option<u64>,
    hold_ns: Option<u64>,
    candidate: Option<LayerGesture>,
    failed: bool,
    moved: bool,
    claimed: bool,
    finished: bool,
}

impl LayerTouches {
    pub(super) fn active(&self) -> bool { !self.contacts.is_empty() }
    pub(super) fn pending(&self) -> bool {
        self.claimed && !self.failed && !self.finished && !self.moved && self.lift_ns.is_none()
            && self.pair.is_some_and(|pair| pair[0].row == pair[1].row)
    }
    pub(super) fn cancel(&mut self) { self.failed = true; self.hold_ns = None; }
    pub(super) fn advance(&mut self, now_ns: u64, policy: TouchPolicy) -> Option<LayerGesture> {
        if !self.pending() { return None; }
        let deadline = self.hold_ns.get_or_insert(now_ns.saturating_add(u64::from(policy.tap_ms) * 1_000_000));
        if now_ns < *deadline { return None; }
        self.finished = true;
        Some(LayerGesture::Contents(self.pair?[0].row))
    }
    pub(super) fn contact(&mut self, input: &UiInput, epoch: u64, policy: TouchPolicy) -> (bool, Option<LayerGesture>) {
        let UiInput::LayerTouch { surface, id, phase, position, row, time_ns } = *input else { return (false, None); };
        if phase == ContactPhase::Down && !self.active() {
            *self = Self { surface, epoch, first_ns: time_ns, ..Self::default() };
        }
        if surface != self.surface { self.cancel(); return (false, None); }
        self.failed |= epoch != self.epoch || time_ns == 0 || self.contacts.get(&id).is_some_and(|contact| time_ns < contact.time_ns)
            || !position.into_iter().all(f32::is_finite);
        self.last_ns = self.last_ns.max(time_ns);
        let mut action = None;
        match phase {
            ContactPhase::Down => {
                self.failed |= self.lift_ns.is_some() || self.contacts.contains_key(&id) || row.is_none();
                self.first_ns = self.first_ns.min(time_ns);
                self.contacts.insert(id, Contact { row: row.unwrap_or(0), origin: position, position, time_ns });
                if self.contacts.len() == 2 {
                    self.claimed = true;
                    self.failed |= self.moved || time_ns.saturating_sub(self.first_ns) > u64::from(policy.tap_ms) * 1_000_000;
                    let mut contacts = self.contacts.values().copied();
                    self.pair = Some([contacts.next().unwrap(), contacts.next().unwrap()]);
                } else if self.contacts.len() > 2 { self.failed = true; }
            }
            ContactPhase::Move | ContactPhase::Up => {
                let Some(contact) = self.contacts.get_mut(&id) else { return (self.claimed, None); };
                contact.position = position;
                contact.time_ns = time_ns;
                self.moved |= (position[0] - contact.origin[0]).hypot(position[1] - contact.origin[1]) > policy.slop;
                if phase == ContactPhase::Up {
                    if self.lift_ns.is_none() {
                        self.candidate = self.recognize(policy);
                        self.lift_ns = Some(time_ns);
                    }
                    self.contacts.remove(&id);
                    if !self.active() && !self.failed && !self.finished
                        && time_ns.saturating_sub(self.lift_ns.unwrap_or(time_ns)) <= u64::from(policy.tap_ms) * 1_000_000 {
                        action = self.candidate.take();
                        self.finished = true;
                    }
                }
            }
            ContactPhase::Cancel => { self.contacts.remove(&id); self.cancel(); }
        }
        (self.claimed, action)
    }
    fn recognize(&self, policy: TouchPolicy) -> Option<LayerGesture> {
        let origins = self.pair?;
        let mut contacts = self.contacts.values().copied();
        let current = [contacts.next()?, contacts.next()?];
        let delta: [[f32; 2]; 2] = std::array::from_fn(|i| std::array::from_fn(|axis| current[i].position[axis] - origins[i].origin[axis]));
        if origins[0].row == origins[1].row {
            if !self.moved && self.last_ns.saturating_sub(self.first_ns) <= u64::from(policy.tap_ms) * 1_000_000 {
                return Some(LayerGesture::Opacity(origins[0].row));
            }
        } else {
            let before = [origins[1].origin[0] - origins[0].origin[0], origins[1].origin[1] - origins[0].origin[1]];
            let after = [current[1].position[0] - current[0].position[0], current[1].position[1] - current[0].position[1]];
            let length = before[0].hypot(before[1]);
            if length >= policy.slop * 4. && after[0].hypot(after[1]) < length * 0.55
                && delta[0][0] * before[0] + delta[0][1] * before[1] > policy.slop * length
                && delta[1][0] * before[0] + delta[1][1] * before[1] < -policy.slop * length {
                return Some(LayerGesture::Merge(origins[0].row, origins[1].row));
            }
        }
        None
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn layer_touch_input(&mut self, mut input: UiInput) -> Result<InputReply, String> {
        if let UiInput::LayerTouch { ref mut row, .. } = input {
            *row = row.filter(|id| self.state.layers.iter().any(|layer| layer.id == *id));
        }
        let eligible = !self.workspace_transition && !self.workspace_read_only && !self.state.settings_open && self.require_idle().is_ok();
        let (handled, action) = self.layer_interaction.touches.contact(&input, self.state.document_file.epoch, self.interaction.touch_policy);
        let mut change = UiChange::default();
        if !eligible { self.layer_interaction.touches.cancel(); }
        if eligible && let Some(action) = action { change = self.perform_layer_gesture(action)?; }
        change.canvas_wake |= self.layer_interaction.touches.pending();
        Ok(InputReply { handled, change, ..Default::default() })
    }
    pub(super) fn advance_layer_touches(&mut self, now_ns: u64) -> Result<u32, String> {
        if self.state.document_file.epoch != self.layer_interaction.touches.epoch || self.workspace_transition
            || self.workspace_read_only || self.state.settings_open || self.require_idle().is_err() {
            self.layer_interaction.touches.cancel();
        }
        let Some(action) = self.layer_interaction.touches.advance(now_ns, self.interaction.touch_policy) else { return Ok(0); };
        Ok(self.perform_layer_gesture(action)?.regions)
    }
    fn perform_layer_gesture(&mut self, gesture: LayerGesture) -> Result<UiChange, String> {
        match gesture {
            LayerGesture::Opacity(id) => {
                if self.state.layer_tools.editing_layer.as_ref().is_none_or(|layer| layer.id != id || layer.mask_selected) {
                    self.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: false } })?;
                }
                if self.state.layer_tools.controls.opacity {
                    self.state.layer_tools.opacity_popup = Some(id);
                    self.state.layer_tools.opacity_surface = self.layer_interaction.touches.surface;
                }
                Ok(self.changed(regions::DOCUMENT, false))
            }
            LayerGesture::Contents(id) => self.dispatch(UiAction::Selection { action: SelectionAction::LoadThumbnail {
                id, mask: false, shift: false, alt: false,
            } }),
            LayerGesture::Merge(first, last) => {
                self.require_idle()?;
                self.merge(layer_core::MergeKind::Range { first: occurrence_handle(first)?, last: occurrence_handle(last)? })?;
                self.refresh_document();
                Ok(self.changed(regions::DOCUMENT | regions::BRUSH, true))
            }
        }
    }
}

#[cfg(test)]
#[path = "layer_gesture_tests.rs"]
mod tests;
