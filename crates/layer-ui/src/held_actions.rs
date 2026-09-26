use super::*;

pub(super) fn selects_tool(action: &UiAction) -> bool {
    match action {
        UiAction::Invoke { command } => CommandId::TOOLS.contains(command),
        UiAction::Layer {
            action: LayerAction::Tool { .. },
        }
        | UiAction::SelectBrush { .. }
        | UiAction::SelectToolGroup { .. }
        | UiAction::SelectBrushSet { .. }
        | UiAction::CycleTool { .. } => true,
        _ => false,
    }
}

fn merge(a: UiChange, b: UiChange) -> UiChange {
    UiChange {
        revision: a.revision.max(b.revision),
        regions: a.regions | b.regions,
        canvas_wake: a.canvas_wake || b.canvas_wake,
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn press_hold(&mut self, token: String, command: CommandId) {
        self.interaction.holds.retain(|(t, _)| *t != token);
        self.interaction.holds.push((token, command));
    }

    pub(super) fn release_hold(&mut self, token: &str) -> bool {
        let count = self.interaction.holds.len();
        self.interaction.holds.retain(|(t, _)| t != token);
        count != self.interaction.holds.len()
    }

    pub(super) fn held_modifiers(&self, mut modifiers: Modifiers) -> Modifiers {
        for (token, _) in &self.interaction.holds {
            match KeyChord::modifier_name(token) {
                Some("shift") => modifiers.shift = false,
                Some("alt") => modifiers.alt = false,
                Some("control") if !self.state.platform.apple() => modifiers.command = false,
                _ => (),
            }
        }
        modifiers
    }

    pub(super) fn binding_category(&self) -> ToolCategory {
        match self.interaction.hold_base {
            Some((tool, preset)) => Self::tool_category(tool, tools::group(preset).tool()),
            None => Self::tool_category(self.layer_interaction.tool, self.state.brush.tool),
        }
    }

    fn hold_active(&self, command: CommandId) -> bool {
        command != CommandId::Eyedropper
            || self.eyedropper.picking.previous.is_some()
    }

    pub(super) fn settle_holds(&mut self) -> Result<Option<UiChange>, String> {
        let target = self.interaction.holds.last().map(|(_, command)| *command);
        if (target == self.interaction.held_tool && target.is_none_or(|c| self.hold_active(c)))
            || self.require_idle().is_err()
            || self.operation.active()
        {
            return Ok(None);
        }
        self.interaction.applying_hold = true;
        let change = self.switch_hold(target);
        self.interaction.applying_hold = false;
        change.map(Some)
    }

    pub(super) fn settle_holds_into(&mut self, reply: &mut InputReply) -> Result<(), String> {
        if let Some(change) = self.settle_holds()? {
            reply.change = merge(reply.change, change);
        }
        Ok(())
    }

    pub(super) fn end_holds_for_tool_choice(&mut self, action: &UiAction) {
        if !self.interaction.applying_hold && self.interaction.hold_base.is_some() && selects_tool(action) {
            self.interaction.holds.clear();
            self.interaction.held_tool = None;
            self.interaction.hold_base = None;
        }
    }

    fn switch_hold(&mut self, target: Option<CommandId>) -> Result<UiChange, String> {
        let mut change = UiChange::default();
        if let Some(held) = self.interaction.held_tool.take() {
            if held == CommandId::Eyedropper && self.cancel_picker() {
                change = self.changed(
                    regions::BRUSH | regions::COMMANDS | regions::CUSTOMIZATION | regions::COLOR_PREVIEW,
                    true,
                );
            }
            if let Some((tool, preset)) = self.interaction.hold_base {
                change = merge(change, self.dispatch(if tool == LayerCanvasTool::Paint {
                    UiAction::SelectBrush { id: preset }
                } else {
                    UiAction::Layer { action: LayerAction::Tool { tool } }
                })?);
            }
        }
        match target {
            Some(command) => {
                if self.interaction.hold_base.is_none() {
                    self.interaction.hold_base = Some((self.layer_interaction.tool, self.state.brush.preset));
                }
                change = merge(change, self.dispatch(UiAction::Invoke { command })?);
                self.interaction.held_tool = Some(command);
            }
            None => self.interaction.hold_base = None,
        }
        Ok(change)
    }
}
