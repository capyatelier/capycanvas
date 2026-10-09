use super::*;
use crate::interaction::Restore;

pub(super) const ERASER_END: &str = "pen.eraser";

fn hold_token(chord: &KeyChord) -> String {
    let mut token = String::new();
    for (on, part) in [(chord.command, "primary+"), (chord.shift, "shift+"), (chord.alt, "alt+")] {
        if on {
            token.push_str(part);
        }
    }
    token + &chord.key
}

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

pub(super) fn merge_change(a: UiChange, b: UiChange) -> UiChange {
    UiChange {
        revision: a.revision.max(b.revision),
        regions: a.regions | b.regions,
        canvas_wake: a.canvas_wake || b.canvas_wake,
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn temporary_tool(&self) -> bool {
        self.interaction.applying_hold || self.interaction.hold_base.is_some() || self.interaction.spring.is_some()
            || self.interaction.restores.iter().any(|restore| matches!(restore, Restore::Tool(..)))
    }
    pub(super) fn press_hold(&mut self, token: String, action: UiAction) {
        self.interaction.holds.retain(|(t, _)| *t != token);
        self.interaction.holds.push((token, action));
    }

    pub(super) fn press_momentary(&mut self, token: String, action: UiAction) -> Result<UiChange, String> {
        let disabled = matches!(action, UiAction::Invoke { command } if !self.command(command).enabled);
        if disabled || self.interaction.momentary.iter().any(|(t, _)| *t == token) {
            return Ok(UiChange::default());
        }
        let restore = match action {
            UiAction::Invoke { command } => Restore::Toggle(command, self.command(command).selected),
            _ => Restore::Slot(self.state.colors.slot),
        };
        let change = self.dispatch(action)?;
        self.interaction.momentary.push((token, restore));
        Ok(change)
    }

    pub(super) fn release_hold(&mut self, token: &str) -> bool {
        let count = self.interaction.holds.len() + self.interaction.momentary.len();
        self.interaction.holds.retain(|(t, _)| t != token);
        let (released, kept) = std::mem::take(&mut self.interaction.momentary).into_iter().partition(|(t, _)| t == token);
        self.interaction.momentary = kept;
        self.interaction.restores.extend(released.into_iter().map(|(_, restore): (String, Restore)| restore));
        count != self.interaction.holds.len() + self.interaction.momentary.len()
    }

    /// What pressing this key's action replaces, so holding it can return.
    pub(super) fn spring_restore(&self, action: &UiAction) -> Option<Restore> {
        if self.interaction.spring.is_some() || self.interaction.hold_base.is_some() {
            return None;
        }
        match action {
            UiAction::Invoke { command } if crate::shortcuts::MOMENTARY_COMMANDS.contains(command) => {
                Some(Restore::Toggle(*command, self.command(*command).selected))
            }
            UiAction::Color { action: ColorAction::ToggleTransparent } => Some(Restore::Slot(self.state.colors.slot)),
            action if selects_tool(action) => Some(Restore::Tool(self.layer_interaction.tool, self.state.brush.preset)),
            _ => None,
        }
    }

    pub(super) fn dispatch_spring(&mut self, key: String, action: UiAction, repeat: bool) -> Result<UiChange, String> {
        if let UiAction::Invoke { command } = action
            && let Some(mode) = navigation::command_mode(command)
        {
            if !repeat {
                self.interaction.navigation = Some((key, mode));
                self.interaction.navigation_tap = true;
            }
            return Ok(UiChange::default());
        }
        let restore = (!repeat).then(|| self.spring_restore(&action)).flatten();
        let started = restore.is_some();
        if let Some(restore) = restore {
            self.interaction.spring = Some(crate::interaction::Spring { key, restore, used: false });
        }
        let change = self.dispatch(action);
        if started && change.is_err() { self.interaction.spring = None; }
        change
    }

    pub(super) fn release_spring(&mut self, key: &str) -> UiChange {
        if !self.interaction.spring.as_ref().is_some_and(|s| s.key == key) { return UiChange::default(); }
        let spring = self.interaction.spring.take().unwrap();
        if spring.used {
            self.interaction.restores.push(spring.restore);
            return UiChange::default();
        }
        let brush_changed = self.layer_interaction.tool == LayerCanvasTool::Paint && self.tools.remember(self.state.brush.preset);
        let memory = self.state.tool_slots.clone();
        self.remember_tool_slots();
        if brush_changed || memory != self.state.tool_slots { self.changed(regions::BRUSH | regions::COMMANDS, false) }
        else { UiChange::default() }
    }

    /// Put the modifier keys that are fully held into effect; more specific
    /// combinations replace the keys they contain.
    pub(super) fn in_modifier_hold(&self, name: &str) -> bool {
        self.interaction.modifier_holds.iter().any(|(token, _)| {
            token.split('+').any(|part| part == name || (part == "primary" && matches!(name, "control" | "meta")))
        })
    }

    pub(super) fn sync_modifier_keys(&mut self, allowed: bool) -> Result<(UiChange, bool), String> {
        let platform = self.state.platform;
        let category = self.binding_category();
        let pressed = &self.interaction.pressed;
        let parts = |chord: &KeyChord| {
            let mut parts = vec![chord.key.clone()];
            parts.extend(chord.command.then(|| if platform.apple() { "meta" } else { "control" }.to_string()));
            parts.extend(chord.shift.then(|| "shift".to_string()));
            parts.extend(chord.alt.then(|| "alt".to_string()));
            parts
        };
        let mut active: Vec<(Vec<String>, String, String)> = if allowed {
            self.state
                .settings
                .hold_keys(platform)
                .into_iter()
                .filter_map(|hold| {
                    let target = hold.actions.get(&category)?.clone();
                    let keys = parts(&hold.key);
                    keys.iter().all(|k| pressed.contains(k)).then(|| (keys, hold_token(&hold.key), target))
                })
                .collect()
        } else {
            Vec::new()
        };
        self.interaction.suppressed.retain(|token| active.iter().any(|(_, t, _)| t == token));
        active.retain(|(_, token, _)| !self.interaction.suppressed.contains(token));
        let snapshot = active.clone();
        active.retain(|(keys, _, _)| {
            !snapshot.iter().any(|(other, _, _)| other.len() > keys.len() && keys.iter().all(|k| other.contains(k)))
        });
        active.sort_by_key(|(keys, _, _)| keys.iter().filter_map(|k| pressed.iter().position(|p| p == k)).max());
        let desired: Vec<(String, String)> = active.into_iter().map(|(_, token, target)| (token, target)).collect();
        if desired == self.interaction.modifier_holds {
            return Ok((UiChange::default(), false));
        }
        let mut change = UiChange::default();
        for (token, target) in std::mem::take(&mut self.interaction.modifier_holds) {
            if !desired.contains(&(token.clone(), target)) {
                if self.interaction.navigation.as_ref().map(|(token, _)| token.as_str()) == Some(&token) {
                    self.interaction.navigation = None;
                }
                self.release_hold(&token);
            }
        }
        let definitions = crate::shortcuts::definitions(platform);
        for (token, target) in &desired {
            if self.interaction.holds.iter().any(|(t, _)| t == token)
                || self.interaction.momentary.iter().any(|(t, _)| t == token)
                || self.interaction.navigation.as_ref().map(|(token, _)| token.as_str()) == Some(token)
            {
                continue;
            }
            let Some(hold) = crate::shortcuts::hold_id(target) else { continue };
            let Some((definition, _)) = definitions.iter().find(|(d, _)| d.id == hold) else { continue };
            match &definition.action {
                ShortcutAction::Navigate { mode } => { self.interaction.navigation = Some((token.clone(), *mode)); self.interaction.navigation_tap = false; },
                ShortcutAction::Hold { action } => self.press_hold(token.clone(), (**action).clone()),
                ShortcutAction::Momentary { action } => {
                    change = merge_change(change, self.press_momentary(token.clone(), (**action).clone())?);
                }
                ShortcutAction::Action { .. } => {}
            }
        }
        self.interaction.modifier_holds = desired;
        Ok((change, true))
    }

    /// Use the eraser end's tool while that end is in use, then go back.
    pub(super) fn eraser_end(&mut self, event: &mut layer_engine::PenEvent) {
        let eraser = event.tool == layer_engine::ToolKind::Eraser;
        let config = self.state.settings.eraser_end;
        if eraser && !config.erases() {
            event.tool = layer_engine::ToolKind::Pen;
        }
        let target = config.tool.filter(|_| eraser).map(|command| UiAction::Invoke { command });
        match (target, self.interaction.eraser_end) {
            (Some(action), false) => {
                self.press_hold(ERASER_END.into(), action);
                self.interaction.eraser_end = true;
            }
            (None, true) => {
                self.release_hold(ERASER_END);
                self.interaction.eraser_end = false;
            }
            _ => {}
        }
        if event.phase == layer_engine::PenPhase::Down
            && let Ok(Some(change)) = self.settle_holds()
        {
            self.interaction.hold_regions |= change.regions;
        }
    }

    pub(super) fn held_modifiers(&self, mut modifiers: Modifiers) -> Modifiers {
        for (token, _) in &self.interaction.modifier_holds {
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

    /// The bindings of a pressed key, most specific first. Keys with held
    /// modifiers fall back to the chord without them.
    pub(super) fn held_shortcut_matches(&self, key: &str, modifiers: Modifiers, canvas: bool) -> Vec<ShortcutDefinition> {
        let context = canvas.then(|| self.binding_category());
        let settings = &self.state.settings;
        let matches = settings.shortcut_matches(&KeyChord::new(key, modifiers), self.state.platform, context);
        let released = self.held_modifiers(modifiers);
        if matches.is_empty() && released != modifiers {
            return settings.shortcut_matches(&KeyChord::new(key, released), self.state.platform, context);
        }
        matches
    }

    /// Whether a key binding would do anything now. Dispatch skips disabled
    /// bindings so a less specific one can run instead.
    pub(super) fn binding_enabled(&self, action: &ShortcutAction) -> bool {
        match action {
            ShortcutAction::Action { action } => match &**action {
                UiAction::Invoke { command } => if navigation::command_mode(*command).is_some() { self.navigation_idle() } else { self.command_flags(*command).0 },
                UiAction::StepToolSetting { id, .. } => self.state.tool_settings.iter().any(|c| c.id == *id),
                _ => true,
            },
            ShortcutAction::Navigate { .. } | ShortcutAction::Hold { .. } | ShortcutAction::Momentary { .. } => true,
        }
    }

    fn hold_active(&self, action: &UiAction) -> bool {
        *action != UiAction::Invoke { command: CommandId::Eyedropper } || self.eyedropper.picking.previous.is_some()
    }

    fn restore(&mut self, restore: Restore) -> Result<UiChange, String> {
        match restore {
            Restore::Tool(tool, preset) if (self.layer_interaction.tool, self.state.brush.preset) != (tool, preset) => {
                if tool != LayerCanvasTool::Paint && self.state.brush.preset != preset { self.select_brush(preset)?; }
                self.dispatch(if tool == LayerCanvasTool::Paint {
                    UiAction::SelectBrush { id: preset }
                } else {
                    UiAction::Layer { action: LayerAction::Tool { tool } }
                })
            }
            Restore::Toggle(command, selected) if self.command(command).selected != selected => {
                self.dispatch(UiAction::Invoke { command })
            }
            Restore::Slot(slot) if self.state.colors.slot != slot => {
                self.dispatch(UiAction::Color { action: ColorAction::Select { slot } })
            }
            _ => Ok(UiChange::default()),
        }
    }

    pub(super) fn note_tool(&mut self) {
        let tool = Some((self.layer_interaction.tool, self.state.brush.preset));
        if self.interaction.hold_base.is_none() && self.interaction.tools[0] != tool {
            self.interaction.tools = [tool, self.interaction.tools[0]];
        }
    }

    pub(super) fn settle_holds(&mut self) -> Result<Option<UiChange>, String> {
        let target = self.interaction.holds.last().map(|(_, action)| action.clone());
        let settled = target == self.interaction.held_tool && target.as_ref().is_none_or(|a| self.hold_active(a));
        if (settled && self.interaction.restores.is_empty()) || self.require_idle().is_err() || self.operation.active() {
            return Ok(None);
        }
        let mut change = UiChange::default();
        self.interaction.applying_hold = true;
        for restore in std::mem::take(&mut self.interaction.restores) {
            let restored = self.restore(restore);
            change = merge_change(change, restored.inspect_err(|_| self.interaction.applying_hold = false)?);
        }
        self.interaction.applying_hold = false;
        if !settled {
            self.interaction.applying_hold = true;
            let switched = self.switch_hold(target);
            self.interaction.applying_hold = false;
            change = merge_change(change, switched?);
        }
        Ok(Some(change))
    }

    pub(super) fn settle_holds_into(&mut self, reply: &mut InputReply) -> Result<(), String> {
        if let Some(change) = self.settle_holds()? {
            reply.change = merge_change(reply.change, change);
        }
        Ok(())
    }

    pub(super) fn end_holds_for_tool_choice(&mut self, action: &UiAction) {
        if !self.interaction.applying_hold && self.interaction.hold_base.is_some() && selects_tool(action) {
            let held: Vec<String> = self.interaction.holds.drain(..).map(|(token, _)| token).collect();
            self.interaction.modifier_holds.retain(|(token, _)| !held.contains(token));
            self.interaction.suppressed.extend(held);
            if self.interaction.held_tool.take() == Some(UiAction::Invoke { command: CommandId::Eyedropper }) {
                self.cancel_picker();
            }
            self.interaction.hold_base = None;
        }
    }

    fn switch_hold(&mut self, target: Option<UiAction>) -> Result<UiChange, String> {
        let mut change = UiChange::default();
        if let Some(held) = self.interaction.held_tool.take() {
            if held == (UiAction::Invoke { command: CommandId::Eyedropper }) && self.cancel_picker() {
                change = self.changed(
                    regions::BRUSH | regions::COMMANDS | regions::CUSTOMIZATION | regions::COLOR_PREVIEW,
                    true,
                );
            }
            if let Some((tool, preset)) = self.interaction.hold_base {
                change = merge_change(change, self.restore(Restore::Tool(tool, preset))?);
            }
        }
        match target {
            Some(action) => {
                if self.interaction.hold_base.is_none() {
                    self.interaction.hold_base = Some((self.layer_interaction.tool, self.state.brush.preset));
                }
                change = merge_change(change, self.dispatch(action.clone())?);
                self.interaction.held_tool = Some(action);
            }
            None => self.interaction.hold_base = None,
        }
        Ok(change)
    }
}
