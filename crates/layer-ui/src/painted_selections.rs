//! Ordered selection gestures. Completed contacts survive asynchronous capture;
//! subsequent contacts resolve their base only after preceding edits commit.
use super::*;
use layer_core::{BrushSnapshot, BrushTip, Selection, SelectionTarget};
use layer_engine::{SampleFlags, SelectionStroke, ToolKind};
use layer_render::{Dab, SelectionOverlay, SelectionPaint, SelectionPaintMode};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionBrushOptions {
    pub subtract: bool,
    pub pressure_size: bool,
    size: f32,
    hardness: f32,
    opacity: f32,
}
impl Default for SelectionBrushOptions {
    fn default() -> Self {
        Self {
            subtract: false,
            pressure_size: false,
            size: 32.,
            hardness: 1.,
            opacity: 1.,
        }
    }
}
impl SelectionBrushOptions {
    pub fn validate(&self) -> Result<(), String> {
        for c in self.controls() {
            c.numeric.validate(c.value, c.label)?;
        }
        Ok(())
    }
    pub fn controls(&self) -> Vec<ToolSetting> {
        [
            (
                "selection_brush_size",
                "Size",
                self.size,
                NumericControl::brush_size(),
            ),
            (
                "selection_brush_hardness",
                "Hardness",
                self.hardness,
                NumericControl::percent(),
            ),
            (
                "selection_brush_opacity",
                "Opacity",
                self.opacity,
                NumericControl::percent(),
            ),
        ]
        .into_iter()
        .map(|(id, label, value, numeric)| ToolSetting {
            id,
            label,
            value,
            numeric,
            group: "",
        })
        .collect()
    }
    pub fn edit(&mut self, id: &str, value: f32) -> Result<(), String> {
        let control = self
            .controls()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("Unknown selection brush setting")?;
        control.numeric.validate(value, control.label)?;
        match id {
            "selection_brush_size" => self.size = value,
            "selection_brush_hardness" => self.hardness = value,
            _ => self.opacity = value,
        }
        Ok(())
    }
    fn brush(&self) -> BrushSnapshot {
        BrushSnapshot {
            diameter: self.size,
            hardness: self.hardness,
            flow: 1.,
            spacing: 0.08,
            color_rgba_linear: [1.; 4],
            mappings: if self.pressure_size {
                Arc::from([layer_core::BrushMapping::pressure_size()])
            } else {
                Arc::from([])
            },
            ..Default::default()
        }
    }
}
struct Gesture {
    id: u64,
    target: SelectionTarget,
    mode: SelectionPaintMode,
    opacity: f32,
    stroke: SelectionStroke,
    before: Option<Arc<Selection>>,
    chunks: VecDeque<(Vec<Dab>, Option<Arc<Selection>>)>,
    ended: bool,
    capturing: bool,
    update: Option<SelectionPaint>,
}
#[derive(Default)]
pub(super) struct PaintedSelections {
    next_id: u64,
    gestures: VecDeque<Gesture>,
    deferred: VecDeque<UiAction>,
}
impl PaintedSelections {
    pub fn renderer_replaced(&mut self) {
        self.gestures.clear();
        self.deferred.clear();
    }
    pub fn busy(&self) -> bool {
        !self.gestures.is_empty() || !self.deferred.is_empty()
    }
    pub fn has_contact(&self) -> bool {
        self.gestures.back().is_some_and(|g| !g.ended)
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn selection_brush_cursor(&self, event: PenEvent) -> Vec<Dab> {
        if let Some(gesture) = self.painted_selections.gestures.back().filter(|g| !g.ended) {
            gesture.stroke.cursor(event)
        } else {
            SelectionStroke::new(
                0,
                self.selection_tools.options.brush.brush(),
                self.state.camera.input_transform(),
                PressureCurve {
                    gamma: self.state.settings.pressure_gamma,
                    ..Default::default()
                },
                None,
            )
            .cursor(event)
        }
    }
    pub(super) fn selection_brush_active(&self) -> bool {
        self.layer_interaction.tool.selection_tool() == Some(SelectionTool::Brush)
    }
    pub(super) fn sync_selection_overlay(&mut self) {
        let overlay = self.selection_brush_active().then_some(SelectionOverlay {
            color: [1., 0., 0., 0.5],
            protected: false,
        });
        self.engine.backend_mut().set_selection_overlay(overlay);
    }
    pub(super) fn cancel_selection_contact(&mut self) -> bool {
        if self
            .painted_selections
            .gestures
            .back()
            .is_none_or(|g| g.ended)
        {
            return false;
        }
        self.painted_selections.gestures.pop_back();
        if self.painted_selections.gestures.is_empty() {
            self.engine.backend_mut().cancel_selection_paint();
        }
        true
    }
    pub(super) fn defer_selection_action(&mut self, action: &UiAction) -> bool {
        // Layout measurement and chrome remain live. Commands/target changes
        // serialize after completed contacts without waiting on the UI thread.
        if !matches!(
            action,
            UiAction::Invoke { .. }
                | UiAction::Layer { .. }
                | UiAction::SelectLayer { .. }
                | UiAction::SetLayerVisibility { .. }
                | UiAction::SetLayerOpacity { .. }
                | UiAction::MoveLayer { .. }
        ) {
            return false;
        }
        self.cancel_selection_contact();
        if self.painted_selections.gestures.is_empty() {
            return false;
        }
        self.painted_selections.deferred.push_back(action.clone());
        true
    }
    pub(super) fn selection_brush_pen(&mut self, event: PenEvent) -> Result<(), String> {
        if event.flags.contains(SampleFlags::PREDICTED)
            || event.flags.contains(SampleFlags::CORRECTION)
        {
            return Ok(());
        }
        if event.phase == PenPhase::Cancel {
            self.cancel_selection_contact();
            return Ok(());
        }
        if event.phase == PenPhase::Down {
            self.cancel_selection_contact();
            if !self.painted_selections.deferred.is_empty() {
                return Ok(());
            }
            if self.painted_selections.gestures.len() >= 32 {
                return Err("Selection capture is still catching up".into());
            }
            let physical_eraser =
                event.tool == ToolKind::Eraser || event.flags.contains(SampleFlags::INVERTED);
            let subtract = physical_eraser
                || (self.selection_tools.options.brush.subtract ^ self.interaction.modifiers.alt);
            if subtract
                && self.engine.document().selection.is_none()
                && self.painted_selections.gestures.is_empty()
            {
                return Ok(());
            }
            self.painted_selections.next_id = self.painted_selections.next_id.wrapping_add(1);
            let id = self.painted_selections.next_id;
            let stroke = SelectionStroke::new(
                id,
                self.selection_tools.options.brush.brush(),
                self.state.camera.input_transform(),
                PressureCurve {
                    gamma: self.state.settings.pressure_gamma,
                    ..Default::default()
                },
                Some(
                    6. * self
                        .logical_viewport
                        .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]),
                ),
            );
            self.painted_selections.gestures.push_back(Gesture {
                id,
                target: SelectionTarget::Current,
                mode: if subtract {
                    SelectionPaintMode::Subtract
                } else {
                    SelectionPaintMode::Add
                },
                opacity: self.selection_tools.options.brush.opacity,
                stroke,
                before: None,
                chunks: VecDeque::new(),
                ended: false,
                capturing: false,
                update: None,
            });
        }
        let Some(gesture) = self
            .painted_selections
            .gestures
            .back_mut()
            .filter(|g| !g.ended)
        else {
            return Ok(());
        };
        if gesture.chunks.back().is_none_or(|(_, area)| area.is_some()) {
            gesture.chunks.push_back((Vec::new(), None));
        }
        let (dabs, _) = gesture.chunks.back_mut().unwrap();
        let areas = gesture.stroke.push(event, dabs);
        for area in areas {
            gesture.chunks.push_back((Vec::new(), Some(Arc::new(area))));
        }
        if event.phase == PenPhase::Up {
            gesture.ended = true;
        }
        self.initial_fit = false;
        Ok(())
    }
    pub(super) fn poll_selection_paint(&mut self) -> Result<u32, String> {
        let mut changed = 0;
        if self
            .painted_selections
            .gestures
            .front()
            .is_some_and(|g| g.capturing)
            && let Some(result) = self.engine.backend_mut().take_selection_paint()
        {
            let gesture = self.painted_selections.gestures.pop_front().unwrap();
            let result = result.map_err(error)?;
            if result.request_id != gesture.id {
                return Err("Unexpected selection capture".into());
            }
            if result.changed {
                let selection = Selection::pixels(result.pixels);
                let edit = self
                    .engine
                    .document()
                    .selection_edit(gesture.target, selection)
                    .map_err(error)?;
                self.layer_edit(edit)?;
                changed |= regions::DOCUMENT | regions::COMMANDS;
            } else {
                self.engine.backend_mut().cancel_selection_paint();
            }
        }
        if let Some(gesture) = self
            .painted_selections
            .gestures
            .front_mut()
            .filter(|g| !g.capturing)
        {
            if gesture.before.is_none() {
                gesture.before = Some(Arc::new(match gesture.target {
                    SelectionTarget::Current => self
                        .engine
                        .document()
                        .selection
                        .clone()
                        .unwrap_or_else(Selection::empty),
                    SelectionTarget::Saved(id) => {
                        self.engine.document().saved_selection(id).map_err(error)?
                    }
                }));
            }
            if gesture.update.is_none() {
                if gesture.ended && gesture.chunks.is_empty() {
                    gesture.chunks.push_back((Vec::new(), None));
                }
                let finish = gesture.ended && gesture.chunks.len() == 1;
                if let Some((dabs, area)) = gesture.chunks.pop_front() {
                    gesture.update = Some(SelectionPaint {
                        id: gesture.id,
                        before: gesture.before.clone().unwrap(),
                        mode: gesture.mode,
                        opacity: gesture.opacity,
                        gray: 0.,
                        tip: BrushTip::AnalyticEllipse,
                        dabs,
                        enclosed: area,
                        finish,
                    });
                }
            }
            if let Some(update) = gesture.update.as_ref()
                && self
                    .engine
                    .backend_mut()
                    .paint_selection(update)
                    .map_err(error)?
            {
                gesture.capturing = update.finish;
                gesture.update = None;
            }
        }
        if self.painted_selections.gestures.is_empty() {
            while let Some(action) = self.painted_selections.deferred.pop_front() {
                changed |= self.dispatch(action)?.regions;
            }
        }
        Ok(changed)
    }
}
