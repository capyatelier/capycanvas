use super::*;
use super::held_actions::merge_change;

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

    pub(super) fn recognize_tap(&mut self, input: &UiInput) -> Option<u8> {
        match *input {
            UiInput::Pointer { id, phase, kind: PointerKind::Touch, position, time_ns, .. } => {
                if phase == ContactPhase::Down && !self.interaction.taps.active() {
                    self.interaction.taps.camera = Some(self.state.camera.clone());
                }
                let eligible = !self.workspace_transition
                    && !self.workspace_read_only
                    && !self.state.settings_open
                    && self.interaction.pointer.is_none()
                    && self.eyedropper.picking.previous.is_none()
                    && self.touch_canvas_idle()
                    && (phase != ContactPhase::Down || !self.transform_touch_hit(position));
                let policy = self.interaction.touch_policy;
                self.interaction.taps.contact(id, phase, position, time_ns, policy, eligible)
            }
            UiInput::Pointer { phase: ContactPhase::Down, .. } | UiInput::ColorPickerHold { .. } => {
                self.interaction.taps.fail();
                None
            }
            UiInput::Blur => {
                self.interaction.taps = Default::default();
                None
            }
            _ => None,
        }
    }

    pub(super) fn perform_tap(&mut self, fingers: u8, reply: &mut InputReply) -> Result<(), String> {
        let trigger = format!("touch.tap.{fingers}");
        let Some(definition) = self.state.settings.gesture_definition(&trigger, self.state.platform) else {
            return Ok(());
        };
        let ShortcutAction::Action { action } = definition.action else {
            return Ok(());
        };
        reply.handled = true;
        if let Some(camera) = self.interaction.taps.camera.take()
            && camera.viewport == self.state.camera.viewport
            && camera != self.state.camera
        {
            let revision = self.state.camera.revision + 1;
            self.state.camera = Camera { revision, ..camera };
            self.sync_camera();
            reply.change = merge_change(reply.change, self.changed(regions::CAMERA, true));
        }
        if !matches!(*action, UiAction::Invoke { command } if !self.command(command).enabled) {
            let change = self.dispatch(*action)?;
            reply.change = merge_change(reply.change, change);
        }
        Ok(())
    }

    pub(super) fn pen_button(&mut self, button: PenButton, pressed: bool, reply: &mut InputReply) -> Result<(), String> {
        let trigger = button.trigger();
        if !pressed {
            if self.interaction.pan_key.as_deref() == Some(trigger) {
                self.interaction.pan_key = None;
                reply.handled = true;
            }
            reply.handled |= self.release_hold(trigger);
            return Ok(());
        }
        if self.state.settings_open || self.interaction.facts.popup_open || self.state.command_search.is_some() {
            return Ok(());
        }
        let Some(definition) = self.state.settings.gesture_definition(trigger, self.state.platform) else {
            return Ok(());
        };
        reply.handled = true;
        match definition.action {
            ShortcutAction::Pan => self.interaction.pan_key = Some(trigger.into()),
            ShortcutAction::Hold { command } => self.press_hold(trigger.into(), command),
            ShortcutAction::Action { action } => {
                if !matches!(*action, UiAction::Invoke { command } if !self.command(command).enabled) {
                    let change = self.dispatch(*action)?;
                    reply.change = merge_change(reply.change, change);
                }
            }
        }
        Ok(())
    }
}
