use super::*;

enum EditInput {
    Action(UiAction),
    Pen { event: PenEvent, camera: Camera },
}
pub(super) struct DeferredEdit {
    drawing: (u64, u64),
    input: EditInput,
}

fn configures_brush(action: &UiAction) -> bool {
    matches!(action, UiAction::SetBrushSize { .. } | UiAction::SetBrushOpacity { .. }
        | UiAction::SetColor { .. } | UiAction::Color { .. }
        | UiAction::SetToolSetting { .. } | UiAction::StepToolSetting { .. }
        | UiAction::ResetToolSetting { .. })
}

impl<R: CanvasRenderer> UiSession<R> {
    fn defer_edit(&mut self, input: EditInput) {
        self.deferred_edits.push_back(DeferredEdit {
            drawing: (self.engine.document().owner, self.state.document_file.epoch),
            input,
        });
    }
    pub(super) fn defer_pen(&mut self, event: PenEvent) {
        self.defer_edit(EditInput::Pen { event, camera: self.state.camera.clone() });
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
    pub(super) fn defer_document_action(&mut self, action: &UiAction) -> bool {
        let configures_brush = configures_brush(action);
        if self.rendering_suspended || self.workspace_read_only || self.workspace_transition
            || (self.deferred_contact().unwrap_or(self.pen_contact) && !configures_brush)
            || self.state.document_file.close_ready
            || matches!(action, UiAction::Invoke { command } if NAVIGATOR_COMMANDS.contains(command) || matches!(command, CommandId::FitCanvas | CommandId::ActualPixels))
            || !(configures_brush || held_actions::selects_tool(action) || matches!(action, UiAction::Invoke { .. } | UiAction::Selection { .. }
                | UiAction::Layer { .. } | UiAction::SelectLayer { .. }
                | UiAction::SetLayerVisibility { .. } | UiAction::SetLayerOpacity { .. }
                | UiAction::Effect { .. } | UiAction::Object { .. } | UiAction::FilterPicker { .. }
                | UiAction::CanvasSize { .. } | UiAction::ImageSize { .. }
                | UiAction::FrequencySeparation { .. } | UiAction::Tonal { .. }
                | UiAction::TransformReference { .. } | UiAction::ToolbarEdit { .. }
                | UiAction::CanvasBarEdit { .. }))
        {
            return false;
        }
        if self.deferred_edits.is_empty() && configures_brush { return false; }
        if !configures_brush { self.cancel_selection_contact(); }
        let history = matches!(action, UiAction::Invoke { command: CommandId::Undo | CommandId::Redo });
        if self.deferred_edits.is_empty() && !self.painted_selections.busy()
            && !(history && (self.region_tools.publishing_edit() || self.files.pending.is_some()))
            && !((self.engine.raster_backing_pending() || self.engine.has_pending_input())
                && !held_actions::selects_tool(action) && !configures_brush)
        {
            return false;
        }
        self.defer_edit(EditInput::Action(action.clone()));
        self.refresh_commands();
        true
    }
    pub(super) fn poll_deferred_edits(&mut self) -> u32 {
        let mut changed = 0;
        let mut deferred = std::mem::take(&mut self.deferred_edits);
        while let Some(edit) = deferred.front() {
            if self.state.document_file.close_ready || self.rendering_suspended
                || edit.drawing != (self.engine.document().owner, self.state.document_file.epoch) {
                deferred.pop_front();
                continue;
            }
            if self.engine.raster_backing_pending() || self.files.pending.is_some()
                || (matches!(&edit.input, EditInput::Action(action) if !configures_brush(action))
                    && (self.painted_selections.busy() || self.region_tools.publishing_edit()
                        || self.engine.has_pending_input() || self.engine.has_active_stroke()))
            {
                break;
            }
            let edit = deferred.pop_front().unwrap();
            match edit.input {
                EditInput::Action(action) => {
                    if matches!(&action, UiAction::Invoke { command } if !self.command_flags(*command).0) { continue; }
                    match self.dispatch(action) {
                        Ok(change) => changed |= change.regions,
                        Err(error) => { self.notify(error); changed |= regions::COMMANDS; }
                    }
                },
                EditInput::Pen { event, camera } => {
                    let current = std::mem::replace(&mut self.state.camera, camera);
                    self.sync_camera();
                    let result = self.pen_ready(event);
                    let camera = std::mem::replace(&mut self.state.camera, current);
                    self.sync_camera();
                    if let Err(event) = result {
                        deferred.push_front(DeferredEdit { input: EditInput::Pen { event, camera }, ..edit });
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
