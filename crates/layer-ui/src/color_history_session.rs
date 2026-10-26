use super::*;
use crate::color::{ColorHistory, PaintPair};
use crate::interaction::{TouchGesture, TouchGestures};

#[derive(Default)]
pub(crate) struct ColorHistories {
    artwork: ColorHistory,
    mask: ColorHistory,
    contact: Option<(bool, ColorState)>,
    touches: TouchGestures,
    surface: u64,
    claimed: bool,
}

pub(super) fn paint_edit(action: &UiAction) -> Option<bool> {
    match action {
        UiAction::Color { action: ColorAction::Undo | ColorAction::Redo } => None,
        UiAction::Color { action } => Some(matches!(action, ColorAction::Swap)),
        UiAction::SetColor { .. } | UiAction::Invoke { command: CommandId::ResetMaskColors } => Some(false),
        UiAction::Invoke { command: CommandId::SwapMaskColors } => Some(true),
        _ => None,
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    fn paint_mask(&self) -> bool {
        self.selection_masks.target().is_some()
    }
    fn paints(&mut self, mask: bool) -> (&mut ColorHistory, &mut ColorState) {
        if mask {
            (&mut self.color_history.mask, &mut self.selection_masks.colors)
        } else {
            (&mut self.color_history.artwork, &mut self.state.colors)
        }
    }
    pub(super) fn paint_before(&self) -> (bool, PaintPair) {
        let mask = self.paint_mask();
        (mask, if mask { &self.selection_masks.colors } else { &self.state.colors }.remembered_paints())
    }
    pub(super) fn record_paint(&mut self, (mask, mut before): (bool, PaintPair), swapped: bool) {
        if self.paint_mask() != mask {
            return;
        }
        let ColorHistories { artwork, mask: masks, contact, .. } = &mut self.color_history;
        let history = if mask { masks } else { artwork };
        let mut start = contact.as_mut().filter(|(started, _)| *started == mask).map(|(_, start)| start);
        if swapped {
            before.swap(0, 1);
            history.swap();
            if let Some(start) = start.as_mut() {
                start.apply(ColorAction::Swap).expect("swap is infallible");
            }
        }
        if start.is_none() {
            history.record(before, if mask { &self.selection_masks.colors } else { &self.state.colors });
        }
    }
    pub(super) fn forget_mask_paint_history(&mut self) {
        self.color_history.mask = ColorHistory::default();
    }
    pub(super) fn forget_paint_history(&mut self) {
        self.color_history = ColorHistories::default();
    }
    fn can_step_paint(&self, redo: bool) -> bool {
        if self.paint_mask() {
            self.color_history.mask.can_step(&self.selection_masks.colors, redo)
        } else {
            self.color_history.artwork.can_step(&self.state.colors, redo)
        }
    }
    fn paint_restored(&mut self, mask: bool) -> Result<(u32, bool), String> {
        if mask {
            self.refresh_tools();
            return Ok((regions::BRUSH | regions::DOCUMENT, true));
        }
        self.state.brush.color = self.state.colors.preview(self.state.colors.definition());
        self.apply_brush()?;
        Ok((regions::BRUSH, false))
    }
    pub(super) fn end_color_contact(&mut self) {
        let mask = self.paint_mask();
        if let Some((started, start)) = self.color_history.contact.take()
            && started == mask
        {
            let (history, colors) = self.paints(mask);
            history.record(start.remembered_paints(), colors);
        }
    }
    pub(super) fn step_paint(&mut self, redo: bool) -> Result<(u32, bool), String> {
        self.end_color_contact();
        let mask = self.paint_mask();
        let (history, colors) = self.paints(mask);
        if !history.step(colors, redo)? {
            return Ok((0, false));
        }
        self.paint_restored(mask)
    }
    pub(super) fn color_contact(&mut self, phase: ContactPhase) -> Result<(u32, bool), String> {
        let mask = self.paint_mask();
        match phase {
            ContactPhase::Down => {
                self.color_history.contact = Some((mask, self.paints(mask).1.clone()));
                Ok((0, false))
            }
            ContactPhase::Move => Ok((0, false)),
            ContactPhase::Up => {
                self.end_color_contact();
                Ok((0, false))
            }
            ContactPhase::Cancel => {
                let Some((started, start)) = self.color_history.contact.take() else { return Ok((0, false)) };
                let colors = self.paints(mask).1;
                if started != mask || *colors == start || start.rgb_space() != colors.rgb_space() || start.hdr_depth() != colors.hdr_depth() {
                    return Ok((0, false));
                }
                *colors = start;
                self.paint_restored(mask)
            }
        }
    }

    pub(super) fn color_touch_input(&mut self, input: UiInput) -> Result<InputReply, String> {
        let UiInput::ColorTouch { surface, id, phase, position, time_ns } = input else { return Ok(InputReply::default()) };
        if phase == ContactPhase::Down && (!self.color_history.touches.active() || surface != self.color_history.surface) {
            self.forget_color_touches();
            self.color_history.surface = surface;
        }
        let eligible = surface == self.color_history.surface
            && !self.rendering_suspended
            && !self.workspace_transition
            && !self.workspace_read_only
            && !self.state.settings_open;
        let gesture = self.color_history.touches.contact(id, phase, position, time_ns, self.interaction.touch_policy, eligible);
        let mut change = UiChange::default();
        if !self.color_history.claimed && self.color_history.touches.repeat_fingers() >= 2 {
            self.color_history.claimed = true;
            change = self.dispatch(UiAction::ColorContact { phase: ContactPhase::Cancel })?;
        }
        if let Some(TouchGesture::Tap(fingers @ 2..=3)) = gesture {
            let redo = match self.state.settings.gesture_binding(if fingers == 2 { "touch.tap.2" } else { "touch.tap.3" }) {
                "command.Undo" => Some(false),
                "command.Redo" => Some(true),
                _ => None,
            };
            if let Some(redo) = redo {
                let step = self.dispatch(UiAction::Color { action: if redo { ColorAction::Redo } else { ColorAction::Undo } })?;
                change = held_actions::merge_change(change, step);
            }
        }
        Ok(InputReply { handled: self.color_history.claimed, change, ..Default::default() })
    }
    pub(super) fn forget_color_touches(&mut self) {
        self.color_history.touches = TouchGestures::default();
        self.color_history.claimed = false;
    }

    pub(super) fn color_wheel_menu(&self) -> Result<ContextMenu, String> {
        let copy = crate::native_copy::ColorCopy::new(self.localization());
        Ok(ContextMenu { title: copy.color.to_string(), sections: vec![self.paint_history_items()] })
    }
    pub(super) fn paint_history_items(&self) -> Vec<ContextMenuItem> {
        [(MessageId::WORKSPACE_COLOR_UNDO, ColorAction::Undo, false), (MessageId::WORKSPACE_COLOR_REDO, ColorAction::Redo, true)]
            .into_iter()
            .map(|(label, action, redo)| {
                let mut item = ContextMenuItem::command(
                    self.localization().text(label).to_string(),
                    UiAction::ColorEdit { context: self.color_edit_context(), action },
                );
                item.enabled = self.can_step_paint(redo);
                item
            })
            .collect()
    }
}
