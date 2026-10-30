use super::*;
use web_time::{Duration, Instant};

#[cfg(test)]
#[path = "held_contact_tests.rs"]
mod tests;

const HOLD: Duration = Duration::from_secs(5);
const HELD_SAMPLES: usize = 4096;

enum EditInput {
    Action(UiAction),
    Pen { event: PenEvent, camera: Camera, pressure:layer_engine::PressureCurve, tilt:layer_engine::TiltCurve },
}
pub(super) struct DeferredEdit {
    drawing: (u64, u64),
    held_since: Option<Instant>,
    input: EditInput,
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
    fn defer_edit(&mut self, input: EditInput) {
        self.queue_edit(input, None);
    }
    fn queue_edit(&mut self, input: EditInput, held_since: Option<Instant>) {
        self.deferred_edits.push_back(DeferredEdit {
            drawing: (self.engine.document().owner, self.state.document_file.epoch),
            held_since,
            input,
        });
    }
    fn defer_pen(&mut self, event: PenEvent, held: bool) {
        if event.flags.contains(layer_engine::SampleFlags::PREDICTED) { return; }
        if event.phase == PenPhase::Cancel && real(&event)
            && let Some(index) = self.held_contact(event.device_id) {
            self.discard_held(index);
            return;
        }
        let holding = self.holding_input();
        let contact = self.held_contact(event.device_id).filter(|_| !starts_contact(&event));
        if real(&event) && event.phase == PenPhase::Up && let Some(start) = contact {
            self.deferred_edits[start].held_since = None;
        }
        let contact_end = contact.map(|start| {
            start + 1 + self.deferred_edits.iter().skip(start + 1)
                .rposition(|edit| matches!(&edit.input, EditInput::Pen { event: queued, .. } if queued.device_id == event.device_id))
                .map_or(0, |last| last + 1)
        });
        self.queue_edit(EditInput::Pen { event, camera: self.state.camera.clone(), pressure:self.engine.configured_pressure_curve().clone(), tilt:self.engine.configured_tilt_curve().clone() }, (held && starts_contact(&event)).then(Instant::now));
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
            if let Err(event) = self.pen_ready(event) { self.defer_pen(event, false); }
            return;
        }
        self.defer_pen(event, held);
    }
    pub(super) fn contact_waits_for_raster_backing(&self, event: &PenEvent) -> bool {
        matches!(event.phase, PenPhase::Down | PenPhase::Up) && self.engine.raster_backing_pending()
    }
    pub(super) fn apply_loaded_paper(&mut self) -> u32 {
        if self.rendering_suspended || !self.deferred_edits.is_empty() || !self.canvas_idle() || self.engine.raster_backing_pending() { return 0; }
        let Some(crate::paper_library::PendingPaper { layer, epoch, image: Some(image), .. }) = self.papers.pending.take_if(|pending| pending.image.is_some()) else { return 0 };
        if epoch != self.state.document_file.epoch { return 0; }
        if let Err(error) = self.apply_paper_texture(layer, layer_core::TextureSource::Image(image)) {
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
    fn expire_held(&mut self) {
        while let Some(index) = self.deferred_edits.iter().position(|edit| edit.held_since.is_some_and(|since| since.elapsed() > HOLD)) {
            self.discard_held_contact(index);
        }
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
            && !(self.deferred_edits.is_empty() && configures_brush)
    }
    fn edits_pending(&self, action: &UiAction) -> bool {
        let history = matches!(action, UiAction::Invoke { command: CommandId::Undo | CommandId::Redo });
        let applying = self.applying_mode() && self.applies_open_mode(action);
        applying || !self.deferred_edits.is_empty() || self.painted_selections.busy()
            || (history && (self.region_tools.publishing_edit() || self.files.pending.is_some()))
            || (self.engine.raster_backing_pending() && !held_actions::selects_tool(action) && !configures_brush(action)
                && !(configures_transform(action) && self.files.pending.is_none() && !self.region_tools.publishing_edit()))
            || (self.engine.has_pending_input() && !held_actions::selects_tool(action) && !configures_brush(action))
    }
    pub(super) fn defers(&self, action: &UiAction) -> bool {
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
    pub(super) fn poll_deferred_edits(&mut self) -> u32 {
        let mut changed = 0;
        self.expire_held();
        let mut deferred = std::mem::take(&mut self.deferred_edits);
        while let Some(edit) = deferred.front() {
            if self.state.document_file.close_ready || self.rendering_suspended
                || edit.drawing != (self.engine.document().owner, self.state.document_file.epoch) {
                deferred.pop_front();
                continue;
            }
            if self.applying_mode() || (self.engine.raster_backing_pending()
                && !matches!(&edit.input, EditInput::Action(action) if configures_transform(action) || configures_brush(action) || held_actions::selects_tool(action))) || self.files.pending.is_some()
                || matches!(&edit.input, EditInput::Pen { event, .. } if starts_contact(event) && !self.paint_ready())
                || (matches!(&edit.input, EditInput::Action(action) if !configures_brush(action))
                    && (self.painted_selections.busy() || self.region_tools.publishing_edit()
                        || self.input_pending || self.engine.has_pending_input() || self.engine.has_active_stroke()))
            {
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
                        deferred.push_front(DeferredEdit { input: EditInput::Pen { event, camera, pressure, tilt }, held_since: None, ..edit });
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
