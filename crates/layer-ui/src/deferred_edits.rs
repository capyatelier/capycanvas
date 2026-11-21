use super::*;

#[cfg(test)]
#[path = "held_contact_tests.rs"]
mod tests;

const HELD_SAMPLES: usize = 4096;

enum EditInput {
    Action(UiAction),
    Pen { event: PenEvent, camera: Camera, pressure:layer_engine::PressureCurve, tilt:layer_engine::TiltCurve },
}
pub(super) struct DeferredEdit {
    drawing: (u64, u64),
    input: EditInput,
}

/// Keeps a drawing switch open. Contacts that start while one is open belong
/// to the drawing that is active when it closes, if that drawing can paint.
pub struct DocumentSwitch {
    _request: std::sync::Arc<()>,
}
#[derive(Clone)]
enum SwitchInput {
    Pointer(UiInput),
    Pen(PenEvent),
}
#[derive(Clone, Default)]
pub(super) struct SwitchContacts {
    request: std::sync::Weak<()>,
    contacts: Vec<u64>,
    carried: Vec<u64>,
    pen_contact: Option<u64>,
    held: std::collections::VecDeque<(u64, SwitchInput)>,
}

fn real(event: &PenEvent) -> bool {
    !event.flags.contains(layer_engine::SampleFlags::PREDICTED) && !event.flags.contains(layer_engine::SampleFlags::CORRECTION)
}
fn starts_contact(event: &PenEvent) -> bool {
    event.phase == PenPhase::Down && real(event)
}

fn configures_brush(action: &UiAction) -> bool {
    match action {
        UiAction::ToolbarEdit { action, .. } => configures_brush(action),
        UiAction::SetBrushSize { .. } | UiAction::SetBrushOpacity { .. } | UiAction::SetBrushStrength { .. }
            | UiAction::SetColor { .. } | UiAction::Color { .. } | UiAction::ColorEdit { .. } | UiAction::ColorContact { .. }
            | UiAction::SetToolSetting { .. } | UiAction::StepToolSetting { .. } | UiAction::ToolSettingContact { .. }
            | UiAction::ResetToolSetting { .. } => true,
        _ => false,
    }
}

fn configures_transform(action: &UiAction) -> bool {
    match action {
        UiAction::ToolbarEdit { action, .. } | UiAction::CanvasBarEdit { action, .. } => configures_transform(action),
        UiAction::Invoke { command } => matches!(transform_choice(*command), Some(TransformChoice::Mode(..) | TransformChoice::Cells(_))),
        _ => false,
    }
}

fn mode_command(action: &UiAction) -> Option<CommandId> {
    match action {
        UiAction::CanvasBarEdit { action, .. } | UiAction::ToolbarEdit { action, .. } => mode_command(action),
        UiAction::Invoke { command } => Some(*command),
        _ => None,
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn open_document_switch(&mut self) -> DocumentSwitch {
        let request = self.switch_contacts.request.upgrade().unwrap_or_else(|| {
            let request = std::sync::Arc::new(());
            self.switch_contacts.request = std::sync::Arc::downgrade(&request);
            request
        });
        DocumentSwitch { _request: request }
    }
    pub fn carries_switch_contact(&self, id: u64) -> bool {
        self.switch_contacts.contacts.contains(&id) || self.switch_contacts.carried.contains(&id)
    }
    pub(super) fn hold_switch_input(&mut self, input: &UiInput) -> Option<InputReply> {
        let UiInput::Pointer { id, phase, kind, .. } = *input else { return None };
        let starts = phase == ContactPhase::Down && (kind != PointerKind::Touch || self.finger_drawing())
            && self.switch_contacts.request.strong_count() > 0;
        let switch = &mut self.switch_contacts;
        if phase != ContactPhase::Move { switch.carried.retain(|contact| *contact != id); }
        let held = starts || switch.contacts.contains(&id);
        switch.pen_contact = held.then_some(id);
        if !held { return None; }
        switch.contacts.retain(|contact| *contact != id);
        if !matches!(phase, ContactPhase::Up | ContactPhase::Cancel) { switch.contacts.push(id); }
        switch.held.push_back((id, SwitchInput::Pointer(input.clone())));
        self.bound_switch_contacts();
        Some(InputReply { handled: true, paint: true, change: self.changed(0, false), ..Default::default() })
    }
    pub(super) fn hold_switch_pen(&mut self, event: PenEvent) -> bool {
        let predicted = event.flags.contains(layer_engine::SampleFlags::PREDICTED);
        let Some(id) = self.switch_contacts.pen_contact else { return predicted && self.switch_contacts.contacts.contains(&event.device_id) };
        if !predicted {
            self.switch_contacts.held.push_back((id, SwitchInput::Pen(event)));
            self.bound_switch_contacts();
        }
        true
    }
    fn bound_switch_contacts(&mut self) {
        let held = &mut self.switch_contacts.held;
        if held.len() <= HELD_SAMPLES { return; }
        let oldest = held[0].0;
        held.retain(|(id, _)| *id != oldest);
    }
    pub(super) fn release_switch_contacts(&mut self) {
        if self.switch_contacts.request.strong_count() > 0 { return; }
        let SwitchContacts { contacts, held, carried, .. } = std::mem::take(&mut self.switch_contacts);
        self.switch_contacts.carried = carried;
        if self.rendering_suspended { return; }
        self.replay_switch_input(held);
        self.switch_contacts.carried.extend(contacts);
    }
    pub(super) fn adopt_switch_contacts(&mut self, previous: &Self) {
        let open = previous.switch_contacts.request.strong_count() > 0;
        self.switch_contacts = if open { previous.switch_contacts.clone() } else { Default::default() };
    }
    fn replay_switch_input(&mut self, held: std::collections::VecDeque<(u64, SwitchInput)>) {
        let mut painting = false;
        for (_, input) in held {
            match input {
                SwitchInput::Pointer(input) => painting = match self.input(input) {
                    Ok(reply) => reply.paint,
                    Err(error) => { self.notify(error); false }
                },
                SwitchInput::Pen(event) => if painting { self.pen(event) },
            }
        }
    }
    fn defer_edit(&mut self, input: EditInput) {
        self.deferred_edits.push_back(DeferredEdit {
            drawing: (self.engine.document().owner, self.state.document_file.epoch),
            input,
        });
    }
    fn defer_pen(&mut self, event: PenEvent) {
        if event.flags.contains(layer_engine::SampleFlags::PREDICTED) { return; }
        if event.phase == PenPhase::Cancel && real(&event)
            && let Some(index) = self.held_contact(event.device_id) {
            self.discard_held(index);
            return;
        }
        let holding = self.holding_input();
        let contact = self.held_contact(event.device_id).filter(|_| !starts_contact(&event));
        let contact_end = contact.map(|start| {
            start + 1 + self.deferred_edits.iter().skip(start + 1)
                .rposition(|edit| matches!(&edit.input, EditInput::Pen { event: queued, .. } if queued.device_id == event.device_id))
                .map_or(0, |last| last + 1)
        });
        self.defer_edit(EditInput::Pen { event, camera: self.state.camera.clone(), pressure:self.engine.configured_pressure_curve().clone(), tilt:self.engine.configured_tilt_curve().clone() });
        if let Some(position) = contact_end.filter(|position| *position + 1 < self.deferred_edits.len()) {
            let edit = self.deferred_edits.pop_back().unwrap();
            self.deferred_edits.insert(position, edit);
        }
        let samples = self.deferred_edits.iter().filter(|edit| matches!(edit.input, EditInput::Pen { .. })).count();
        if samples > HELD_SAMPLES && let Some(index) = self.deferred_edits.iter().position(|edit|
            matches!(&edit.input, EditInput::Pen { event, .. } if starts_contact(event))) {
            self.discard_held_contact(index);
        }
        if holding != self.holding_input() { self.refresh_commands(); }
    }
    pub(super) fn admit_pen(&mut self, event: PenEvent) {
        if let Some(index) = self.discarded_contacts.iter().position(|id| *id == event.device_id) {
            if real(&event) && matches!(event.phase, PenPhase::Down | PenPhase::Up | PenPhase::Cancel) {
                self.discarded_contacts.swap_remove(index);
            }
            if event.phase != PenPhase::Down || !real(&event) { return; }
        }
        let held = starts_contact(&event) && !self.paint_ready();
        if self.deferred_edits.is_empty() && !held {
            if let Err(event) = self.pen_ready(event) { self.defer_pen(event); }
            return;
        }
        self.defer_pen(event);
    }
    pub(super) fn contact_waits_for_raster_backing(&self, event: &PenEvent) -> bool {
        matches!(event.phase, PenPhase::Down | PenPhase::Up) && self.engine.raster_backing_pending()
            || event.phase==PenPhase::Down && self.fill_waits_for_commit()
    }
    pub(super) fn apply_loaded_paper(&mut self) -> u32 {
        if self.rendering_suspended || !self.deferred_edits.is_empty() || !self.canvas_idle() || self.engine.raster_backing_pending() { return 0; }
        let Some(crate::paper_library::PendingPaper { layer, epoch, image: Some(image), .. }) = self.papers.pending.take_if(|pending| pending.image.is_some()) else { return 0 };
        if epoch != self.state.document_file.epoch { return 0; }
        if let Err(error) = self.apply_paper_texture(layer, Some(layer_core::TextureSource::Image(image))) {
            self.notify(error);
        }
        regions::DOCUMENT
    }
    pub fn holding_input(&self) -> bool {
        self.deferred_edits.iter().any(|edit| matches!(edit.input, EditInput::Pen { .. }))
    }
    pub fn paint_ready(&self) -> bool {
        let engine = &self.engine;
        engine.backend().paint_ready(engine.document(), engine.brush(), engine.transform_preview().is_some())
    }
    fn held_contact(&self, id: u64) -> Option<usize> {
        let index = self.deferred_edits.iter().rposition(|edit| matches!(&edit.input, EditInput::Pen { event, .. } if starts_contact(event) && event.device_id == id))?;
        let ended = self.deferred_edits.iter().skip(index + 1).any(|edit| matches!(&edit.input, EditInput::Pen { event, .. }
            if event.device_id == id && real(event) && matches!(event.phase, PenPhase::Up | PenPhase::Cancel)));
        (!ended).then_some(index)
    }
    fn discard_held(&mut self, index: usize) -> bool {
        let EditInput::Pen { event, .. } = &self.deferred_edits[index].input else { return true };
        let id = event.device_id;
        let (mut position, mut ended) = (0, false);
        self.deferred_edits.retain(|edit| {
            let discarded = position >= index && !ended && matches!(&edit.input, EditInput::Pen { event, .. } if event.device_id == id);
            if discarded && let EditInput::Pen { event, .. } = &edit.input {
                ended = real(event) && matches!(event.phase, PenPhase::Up | PenPhase::Cancel);
            }
            position += 1;
            !discarded
        });
        self.refresh_commands();
        ended
    }
    fn discard_held_contact(&mut self, index: usize) {
        let EditInput::Pen { event, .. } = &self.deferred_edits[index].input else { return };
        let id = event.device_id;
        if !self.discard_held(index) { self.discarded_contacts.push(id); }
    }
    pub(super) fn deferred_contact(&self) -> Option<bool> {
        self.deferred_edits.iter().rev().find_map(|edit| match edit.input {
            EditInput::Pen { event, .. } if !event.flags.contains(layer_engine::SampleFlags::PREDICTED)
                && !event.flags.contains(layer_engine::SampleFlags::CORRECTION) => match event.phase {
                    PenPhase::Down => Some(true),
                    PenPhase::Up | PenPhase::Cancel => Some(false),
                    _ => None,
                },
            _ => None,
        })
    }
    pub(super) fn history_pending(&self) -> bool {
        !self.deferred_edits.is_empty() || self.painted_selections.busy()
            || self.region_tools.publishing_edit() || self.engine.has_pending_input()
    }
    pub(super) fn undo_deferred(&self) -> bool {
        self.deferred_edits.iter().any(|edit| matches!(edit.input,
            EditInput::Action(UiAction::Invoke { command: CommandId::Undo })))
    }
    pub fn file_drop_pending(&self, id: u32) -> bool {
        self.state.requests.iter().any(|request| request.id == id)
            || self.deferred_edits.iter().any(|edit| matches!(&edit.input, EditInput::Action(UiAction::FileDrop { id: pending, .. }) if *pending == id))
    }
    pub(super) fn mode_apply_pending(&self) -> bool {
        self.applying_mode() || self.deferred_edits.iter().any(|edit| matches!(&edit.input, EditInput::Action(action) if mode_command(action) == Some(CommandId::ApplyTransform)))
    }
    pub(super) fn discard_mode_actions(&mut self) { self.deferred_edits.clear(); }
    fn deferrable(&self, action: &UiAction) -> bool {
        if self.mode_apply_pending() && matches!(mode_command(action), Some(CommandId::CancelTransform | CommandId::ResetTransform | CommandId::Undo)) { return false; }
        let configures_brush = configures_brush(action);
        !(self.rendering_suspended || self.workspace_read_only || self.workspace_transition
            || (self.deferred_contact().unwrap_or(self.pen_contact) && !configures_brush
                && !(self.deferred_contact() == Some(true) && held_actions::selects_tool(action)))
            || self.state.document_file.close_ready
            || matches!(action, UiAction::Invoke { command } if Self::command_without_document(*command) || NAVIGATOR_COMMANDS.contains(command) || matches!(command, CommandId::FitCanvas | CommandId::ActualPixels))
            || !(configures_brush || held_actions::selects_tool(action) || matches!(action, UiAction::Invoke { .. } | UiAction::FileDrop { .. } | UiAction::Selection { .. }
                | UiAction::Layer { .. } | UiAction::SelectLayer { .. }
                | UiAction::SetLayerVisibility { .. } | UiAction::SetLayerOpacity { .. }
                | UiAction::Effect { .. } | UiAction::FilterPicker { .. }
                | UiAction::CanvasSize { .. } | UiAction::ImageSize { .. }
                | UiAction::FrequencySeparation { .. } | UiAction::Tonal { .. }
                | UiAction::TransformReference { .. } | UiAction::ToolbarEdit { .. }
                | UiAction::CanvasBarEdit { .. })))
            && !(self.deferred_edits.is_empty() && configures_brush && !self.fill_waits_for_commit())
    }
    fn edits_pending(&self, action: &UiAction) -> bool {
        let history = matches!(action, UiAction::Invoke { command: CommandId::Undo | CommandId::Redo });
        let applying = self.applying_mode() && self.applies_open_mode(action);
        applying || self.fill_waits_for_commit() || !self.deferred_edits.is_empty() || self.painted_selections.busy()
            || (history && (self.region_tools.publishing_edit() || self.files.pending.is_some()))
            || (self.engine.raster_backing_pending() && !held_actions::selects_tool(action) && !configures_brush(action)
                && !(configures_transform(action) && self.files.pending.is_none() && !self.region_tools.publishing_edit()))
            || (self.engine.has_pending_input() && !held_actions::selects_tool(action) && !configures_brush(action))
    }
    pub fn defers(&self, action: &UiAction) -> bool {
        self.deferrable(action) && self.edits_pending(action)
    }
    pub(super) fn defer_document_action(&mut self, action: &UiAction) -> bool {
        if !self.deferrable(action) { return false; }
        if !configures_brush(action) { self.cancel_selection_contact(); }
        if !self.edits_pending(action) { return false; }
        self.defer_edit(EditInput::Action(action.clone()));
        self.refresh_commands();
        true
    }
    fn deferred_edit_retired(&self, edit: &DeferredEdit) -> bool {
        self.state.document_file.close_ready || self.rendering_suspended
            || edit.drawing != (self.engine.document().owner, self.state.document_file.epoch)
    }
    fn deferred_edit_waits(&self, edit: &DeferredEdit) -> bool {
        self.applying_mode() || (self.engine.raster_backing_pending()
            && !matches!(&edit.input, EditInput::Action(action) if configures_transform(action) || configures_brush(action) || held_actions::selects_tool(action))) || self.files.pending.is_some()
            || matches!(&edit.input,EditInput::Action(_)) && self.fill_waits_for_commit()
            || matches!(&edit.input, EditInput::Pen { event, .. } if starts_contact(event) && !self.paint_ready() || self.contact_waits_for_raster_backing(event))
            || (matches!(&edit.input, EditInput::Action(action) if !configures_brush(action))
                && (self.painted_selections.busy() || self.region_tools.publishing_edit()
                    || self.input_pending || self.engine.has_pending_input() || self.engine.has_active_stroke()))
    }
    pub(super) fn deferred_edits_runnable(&self) -> bool {
        self.deferred_edits.front().is_some_and(|edit| self.deferred_edit_retired(edit) || !self.deferred_edit_waits(edit))
    }
    pub(super) fn poll_deferred_edits(&mut self) -> u32 {
        let mut changed = 0;
        let mut deferred = std::mem::take(&mut self.deferred_edits);
        while let Some(edit) = deferred.front() {
            if self.deferred_edit_retired(edit) {
                deferred.pop_front();
                continue;
            }
            if self.deferred_edit_waits(edit) {
                break;
            }
            let edit = deferred.pop_front().unwrap();
            match edit.input {
                EditInput::Action(action) => {
                    let applying = mode_command(&action) == Some(CommandId::ApplyTransform);
                    match self.dispatch(action) {
                        Ok(change) => changed |= change.regions,
                        Err(error) => { if applying { self.deferred_edits.append(&mut deferred); self.discard_mode_actions(); } self.notify(error); changed |= regions::COMMANDS; }
                    }
                },
                EditInput::Pen { event, camera, pressure, tilt } => {
                    let current_pressure=self.engine.configured_pressure_curve().clone();
                    let current_tilt=self.engine.configured_tilt_curve().clone();
                    self.engine.set_pressure_curve(pressure.clone());self.engine.set_tilt_curve(tilt.clone());
                    let current = std::mem::replace(&mut self.state.camera, camera);
                    self.sync_camera();
                    let result = self.pen_ready(event);
                    self.engine.set_pressure_curve(current_pressure);self.engine.set_tilt_curve(current_tilt);
                    let camera = std::mem::replace(&mut self.state.camera, current);
                    self.sync_camera();
                    if let Err(event) = result {
                        deferred.push_front(DeferredEdit { input: EditInput::Pen { event, camera, pressure, tilt }, ..edit });
                        break;
                    }
                }
            }
            if !self.deferred_edits.is_empty() { break; }
        }
        if self.deferred_edits.is_empty() { self.deferred_edits = deferred; }
        else { self.deferred_edits.append(&mut deferred); }
        changed
    }
}
