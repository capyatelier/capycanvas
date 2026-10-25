use super::*;
use super::held_actions::merge_change;
use crate::interaction::{Restore, TouchGesture};

impl<R: CanvasRenderer> UiSession<R> {
    pub fn set_touch_policy(&mut self, policy: TouchPolicy) -> Result<(), String> {
        if policy.tap_ms == 0 || policy.tap_ms > 2000 || !(policy.slop.is_finite() && policy.slop > 0.) {
            return Err("Invalid touch policy".into());
        }
        self.interaction.touch_policy = policy;
        Ok(())
    }

    fn touch_canvas_idle(&self) -> bool {
        !(self.painted_selections.has_contact()
            || self.input_pending
            || self.engine.has_active_stroke()
            || !self.layer_interaction.path.is_empty())
    }

    pub(super) fn recognize_touch_gesture(&mut self, input: &UiInput) -> Option<TouchGesture> {
        match *input {
            UiInput::Pointer { id, phase, kind: PointerKind::Touch, position, time_ns, .. }
            | UiInput::TrackpadTouch { id, phase, position, time_ns } => {
                let indirect = matches!(input, UiInput::TrackpadTouch { .. });
                if phase == ContactPhase::Down && !self.interaction.gestures.active() {
                    self.interaction.gestures.camera = Some(self.state.camera.clone());
                }
                let eligible = !self.workspace_transition
                    && !self.workspace_read_only
                    && !self.state.settings_open
                    && !self.interaction.facts.popup_open
                    && self.state.command_search.is_none()
                    && self.interaction.pointer.is_none()
                    && self.eyedropper.picking.previous.is_none()
                    && self.touch_canvas_idle()
                    && (indirect || phase != ContactPhase::Down || !self.object_touch_hit(position));
                let policy = self.interaction.touch_policy;
                self.interaction.gestures.contact(id, phase, position, time_ns, policy, eligible)
                    .filter(|gesture| !indirect || matches!(gesture, TouchGesture::Scrub | TouchGesture::QuickPinch { .. }))
            }
            UiInput::Pointer { phase: ContactPhase::Down, .. } | UiInput::ColorPickerHold { .. } => {
                self.interaction.gestures.fail();
                None
            }
            UiInput::Blur => {
                self.interaction.gestures = Default::default();
                None
            }
            _ => None,
        }
    }

    pub(super) fn perform_touch_gesture(&mut self, gesture: TouchGesture, reply: &mut InputReply) -> Result<(), String> {
        if matches!(gesture, TouchGesture::QuickPinch { outward: true })
            && !self.navigation.quick_fit.as_ref().is_some_and(|(_, fit)| self.interaction.gestures.camera.as_ref().is_some_and(|camera| camera.view() == *fit)) {
            return Ok(());
        }
        let action = match gesture {
            TouchGesture::Scrub => UiAction::Invoke { command: CommandId::ClearLayer },
            TouchGesture::QuickPinch { .. } => UiAction::Invoke { command: CommandId::ResetView },
            TouchGesture::Hold => {
                let Some(command) = self.touch_repeat_command() else { return Ok(()); };
                UiAction::Invoke { command }
            }
            TouchGesture::Tap(fingers) => {
                let trigger = match fingers { 2 => "touch.tap.2", 3 => "touch.tap.3", 4 => "touch.tap.4", _ => return Ok(()) };
                let Some(definition) = self.state.settings.gesture_definition(trigger, self.state.platform) else { return Ok(()); };
                let ShortcutAction::Action { action } = definition.action else { return Ok(()); };
                *action
            }
        };
        reply.handled = true;
        if let Some(camera) = self.interaction.gestures.camera.take()
            && camera.viewport == self.state.camera.viewport
            && camera != self.state.camera
        {
            let revision = self.state.camera.revision + 1;
            self.state.camera = Camera { revision, ..camera };
            self.sync_camera();
            reply.change = merge_change(reply.change, self.changed(regions::CAMERA, true));
        }
        if let TouchGesture::QuickPinch { outward } = gesture {
            if outward {
                if let Some((previous, _)) = self.navigation.quick_fit.take() { previous.restore(&mut self.state.camera); }
                self.initial_fit = false;
                self.sync_camera();
            } else {
                let previous = super::navigation::ViewPosition::capture(&self.state.camera);
                self.navigate_command(CommandId::ResetView)?;
                self.navigation.quick_fit = Some((previous, self.state.camera.view()));
            }
            reply.change = merge_change(reply.change, self.changed(regions::CAMERA, true));
            return Ok(());
        }
        if !matches!(action, UiAction::Invoke { command } if !self.command(command).enabled) {
            let change = self.dispatch(action)?;
            reply.change = merge_change(reply.change, change);
        }
        Ok(())
    }

    pub(super) fn touch_repeat_pending(&self) -> bool {
        self.interaction.gestures.repeat_pending() && self.touch_repeat_command().is_some()
    }

    fn touch_repeat_command(&self) -> Option<CommandId> {
        let trigger = match self.interaction.gestures.repeat_fingers() { 2 => "touch.tap.2", 3 => "touch.tap.3", _ => return None };
        match self.state.settings.gesture_binding(trigger) {
            "command.Undo" => Some(CommandId::Undo),
            "command.Redo" => Some(CommandId::Redo),
            _ => None,
        }
    }

    pub(super) fn advance_touch_gestures(&mut self, now_ns: u64) -> Result<u32, String> {
        if self.state.settings_open || self.interaction.facts.popup_open || self.state.command_search.is_some()
            || self.workspace_transition || self.workspace_read_only || !self.touch_canvas_idle() {
            self.interaction.gestures.fail();
        }
        let mut reply = InputReply::default();
        if !self.touch_repeat_pending() { return Ok(0); }
        if let Some(gesture) = self.interaction.gestures.advance(now_ns, self.interaction.touch_policy) {
            self.perform_touch_gesture(gesture, &mut reply)?;
        }
        Ok(reply.change.regions)
    }

    pub(super) fn pen_button(&mut self, button: PenButton, pressed: bool, reply: &mut InputReply) -> Result<(), String> {
        let trigger = button.trigger();
        if !pressed {
            if self.interaction.navigation.as_ref().map(|(token, _)| token.as_str()) == Some(trigger) {
                self.interaction.navigation = None;
                reply.handled = true;
            }
            reply.handled |= self.release_hold(trigger);
            return Ok(());
        }
        if self.state.settings_open || self.interaction.facts.popup_open || self.state.command_search.is_some() {
            return Ok(());
        }
        let category = self.binding_category();
        let Some(target) = self.state.settings.pen_actions(trigger).remove(&category) else {
            return Ok(());
        };
        let id = crate::shortcuts::hold_id(&target).unwrap_or(target);
        let Some((definition, _)) = crate::shortcuts::definitions(self.state.platform).into_iter().find(|(d, _)| d.id == id)
        else {
            return Ok(());
        };
        reply.handled = true;
        match definition.action {
            ShortcutAction::Navigate { mode } => self.interaction.navigation = Some((trigger.into(), mode)),
            ShortcutAction::Hold { action } => self.press_hold(trigger.into(), *action),
            ShortcutAction::Momentary { action } => {
                let change = self.press_momentary(trigger.into(), *action)?;
                reply.change = merge_change(reply.change, change);
            }
            ShortcutAction::Action { action } => {
                if !matches!(*action, UiAction::Invoke { command } if !self.command(command).enabled) {
                    let change = self.dispatch(*action)?;
                    reply.change = merge_change(reply.change, change);
                }
            }
        }
        Ok(())
    }

    pub(super) fn stylus_action(&mut self, action: StylusAction, reply: &mut InputReply) -> Result<(), String> {
        if self.state.settings_open || self.interaction.facts.popup_open || self.state.command_search.is_some() {
            return Ok(());
        }
        self.note_tool();
        let restore = match (action, self.interaction.tools[1]) {
            (StylusAction::SwitchEraser, _) if !self.command(CommandId::Eraser).selected => Restore::Toggle(CommandId::Eraser, true),
            (_, Some((tool, preset))) => Restore::Tool(tool, preset),
            (_, None) => return Ok(()),
        };
        reply.handled = true;
        self.interaction.restores.push(restore);
        self.settle_holds_into(reply)
    }

    pub(super) fn navigation_axes(&mut self, pan: [f32; 2], zoom: f32, reply: &mut InputReply) -> Result<(), String> {
        reply.handled = true;
        reply.change.canvas_wake |= self.interaction.axes.set(pan, zoom)?;
        Ok(())
    }

    pub(super) fn advance_axes(&mut self, now_ns: u64) -> Result<u32, String> {
        let Some((pan, zoom)) = self.interaction.axes.advance(now_ns) else {
            return Ok(0);
        };
        if self.require_idle().is_err() || self.interaction.pointer.is_some() || self.touch.is_active() {
            return Ok(0);
        }
        let viewport = self.state.camera.viewport.map(|v| v as f32);
        let center = [viewport[0] / 2., viewport[1] / 2.];
        let speed = 1.5 * viewport[0].max(viewport[1]) * self.state.settings.pan_speed;
        let to = [center[0] - pan[0] * speed, center[1] - pan[1] * speed];
        let scale = (zoom * 2. * self.state.settings.zoom_speed).exp();
        self.state.camera.gesture(center, to, scale, 0.)?;
        self.initial_fit = false;
        self.sync_camera();
        Ok(regions::CAMERA)
    }
}
