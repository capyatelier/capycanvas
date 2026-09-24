//! Ordered selection gestures. Completed contacts survive asynchronous capture;
//! subsequent contacts resolve their base only after preceding edits commit.
use super::*;
use layer_core::{BrushSnapshot, Selection, SelectionTarget};
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
    gray: f32,
    gradient: Option<layer_render::SelectionGradient>,
    region: Option<RegionJob>,
    stroke: SelectionStroke,
    before: Option<Arc<Selection>>,
    chunks: VecDeque<(Vec<Dab>, Option<Arc<Selection>>)>,
    ended: bool,
    capturing: bool,
    restart: bool,
    update: Option<SelectionPaint>,
}
struct RegionJob {
    request: layer_render::RegionRequest,
    basis: layer_core::Affine,
    started: bool,
    paint: bool,
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
    fn selection_paint_brush(&self) -> BrushSnapshot {
        if self.selection_masks.target().is_some() {
            let mut brush = self.engine.configured_brush().clone();
            brush.color_rgba_linear = [1.; 4];
            brush
        } else {
            self.selection_tools.options.brush.brush()
        }
    }
    pub(super) fn queue_mask_fill(
        &mut self,
        target: SelectionTarget,
        area: Selection,
    ) -> Result<(), String> {
        if self.painted_selections.gestures.len() >= 32 {
            return Err("Selection capture is still catching up".into());
        }
        // Validate the destination before accepting an asynchronous operation.
        self.engine
            .document()
            .selection_edit(target, self.mask_coverage(target)?)
            .map_err(error)?;
        self.painted_selections.next_id = self.painted_selections.next_id.wrapping_add(1);
        let id = self.painted_selections.next_id;
        self.painted_selections.gestures.push_back(Gesture {
            id,
            target,
            mode: SelectionPaintMode::Gray,
            opacity: self.state.brush.opacity,
            gray: self.mask_paint_value(self.state.brush.tool == Tool::Eraser),
            gradient: None,
            region: None,
            stroke: SelectionStroke::new(
                id,
                BrushSnapshot::default(),
                self.state.camera.input_transform(),
                PressureCurve::default(),
                None,
            ),
            before: None,
            chunks: VecDeque::from([(Vec::new(), Some(Arc::new(area)))]),
            ended: true,
            capturing: false,
            restart: false,
            update: None,
        });
        Ok(())
    }
    pub(super) fn queue_mask_gradient(
        &mut self,
        target: SelectionTarget,
        gradient: layer_render::SelectionGradient,
    ) -> Result<(), String> {
        self.queue_mask_fill(target, Selection::full())?;
        self.painted_selections
            .gestures
            .back_mut()
            .unwrap()
            .gradient = Some(gradient);
        Ok(())
    }
    pub(super) fn queue_mask_region(
        &mut self,
        target: SelectionTarget,
        mut request: layer_render::RegionRequest,
        basis: layer_core::Affine,
        paint: bool,
    ) -> Result<(), String> {
        self.queue_mask_fill(target, Selection::empty())?;
        let gesture = self.painted_selections.gestures.back_mut().unwrap();
        gesture.chunks.clear();
        request.request_id = gesture.id;
        gesture.region = Some(RegionJob {
            request,
            basis,
            started: false,
            paint,
        });
        Ok(())
    }
    pub(super) fn selection_brush_cursor(&self, event: PenEvent) -> Vec<Dab> {
        if let Some(gesture) = self.painted_selections.gestures.back().filter(|g| !g.ended) {
            gesture.stroke.cursor(event)
        } else {
            SelectionStroke::new(
                0,
                self.selection_paint_brush(),
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
        let display = &self.selection_tools.options.display;
        let properties = self.mask_properties();
        let target = self.selection_masks.target();
        let visible = match target {
            Some(SelectionTarget::Saved(id)) => self
                .engine
                .document()
                .layer(id)
                .is_some_and(|l| self.engine.document().layer_is_visible(l.id)),
            Some(SelectionTarget::Current) => self.selection_masks.quick_visible,
            _ => true,
        };
        let active = visible && (target.is_some() || self.selection_brush_active());
        let overlay = display.overlay.then_some(SelectionOverlay {
            active,
            editing: match target {
                Some(SelectionTarget::Saved(id)) => Some(id),
                _ => None,
            },
            color: if target.is_some() {
                let mut color = properties.color.encoded_in(layer_core::color::RgbSpace::Srgb).unwrap_or(properties.color.rgba);
                color[3] *= properties.opacity; color
            } else { display.color },
            protected: target.is_some() && properties.protected,
        });
        let selection = if let Some(target) = target {
            Some(if display.overlay && active {
                self.mask_coverage(target).ok()
            } else {
                None
            })
        } else if !display.outline && !self.selection_brush_active() {
            Some(None)
        } else {
            None
        };
        let quick = self.selection_masks.quick().then(|| self.current_selection().unwrap_or_else(Selection::empty));
        self.engine.backend_mut().set_quick_mask_thumbnail(quick.as_ref());
        self.engine.set_selection_display(selection);
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
                | UiAction::Selection { .. }
                | UiAction::SelectBrush { .. }
                | UiAction::SelectBrushSet { .. }
                | UiAction::SelectToolGroup { .. }
                | UiAction::CycleTool { .. }
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
            if let Some(reason) = self.mask_brush_reason() {
                return Err(reason.into());
            }
            let physical_eraser =
                event.tool == ToolKind::Eraser || event.flags.contains(SampleFlags::INVERTED);
            let subtract = physical_eraser
                || (self.effective_selection_mode() == SelectionMode::Subtract);
            let target = self
                .selection_masks
                .target()
                .unwrap_or(SelectionTarget::Current);
            let mask = self.selection_masks.target().is_some();
            if !mask
                && subtract
                && self.engine.document().selection.is_none()
                && self.painted_selections.gestures.is_empty()
            {
                return Ok(());
            }
            self.painted_selections.next_id = self.painted_selections.next_id.wrapping_add(1);
            let id = self.painted_selections.next_id;
            let stroke = SelectionStroke::new(
                id,
                self.selection_paint_brush(),
                self.state.camera.input_transform(),
                PressureCurve {
                    gamma: self.state.settings.pressure_gamma,
                    ..Default::default()
                },
                (!mask).then_some(
                    6. * self
                        .logical_viewport
                        .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]),
                ),
            );
            self.painted_selections.gestures.push_back(Gesture {
                id,
                target,
                mode: if mask {
                    SelectionPaintMode::Gray
                } else if subtract {
                    SelectionPaintMode::Subtract
                } else {
                    SelectionPaintMode::Add
                },
                opacity: if mask {
                    1.
                } else {
                    self.selection_tools.options.brush.opacity
                },
                gradient: None,
                region: None,
                gray: self.mask_paint_value(physical_eraser || self.state.brush.tool == Tool::Eraser),
                stroke,
                before: None,
                chunks: VecDeque::new(),
                ended: false,
                capturing: false,
                restart: false,
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
        if gesture.stroke.input_budget_exhausted() {
            self.cancel_selection_contact();
            return Err("Stroke exceeded the live input budget and was canceled".into());
        }
        if gesture.chunks.back().is_none_or(|(_, area)| area.is_some()) {
            gesture.chunks.push_back((Vec::new(), None));
        }
        let (dabs, _) = gesture.chunks.back_mut().unwrap();
        let areas = gesture.stroke.push(event, dabs);
        for area in areas {
            gesture.chunks.push_back((Vec::new(), Some(Arc::new(area))));
        }
        if event.phase == PenPhase::Up {
            if let Some(dabs) = gesture.stroke.finished_replay() {
                gesture.chunks.clear();
                gesture.chunks.push_back((dabs, None));
                // An already submitted chunk must retain its acknowledgement.
                // The final replay follows it and replaces the GPU footprint.
                gesture.restart = true;
            }
            gesture.ended = true;
        }
        self.initial_fit = false;
        Ok(())
    }
    pub(super) fn poll_selection_paint(&mut self) -> Result<u32, String> {
        let mut changed = 0;
        if let Some(job) = self
            .painted_selections
            .gestures
            .front_mut()
            .and_then(|g| g.region.as_mut())
        {
            if !job.started {
                job.started = self
                    .engine
                    .backend_mut()
                    .request_region(job.request.clone())
                    .map_err(error)?;
                return Ok(0);
            }
            let Some(result) = self.engine.backend_mut().take_region() else {
                return Ok(0);
            };
            let result = result.map_err(error)?;
            if result.request_id != job.request.request_id {
                return Err("Unexpected mask region capture".into());
            }
            let selection = Selection::pixels(result.pixels)
                .transformed(job.basis)
                .map_err(error)?;
            if job.paint {
                let gesture = self.painted_selections.gestures.front_mut().unwrap();
                gesture.region = None;
                gesture
                    .chunks
                    .push_back((Vec::new(), Some(Arc::new(selection))));
            } else {
                let target = self.painted_selections.gestures.pop_front().unwrap().target;
                self.set_mask_coverage(target, selection)?;
                changed |= regions::DOCUMENT | regions::COMMANDS;
            }
        }
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
        if let Some(target) = self
            .painted_selections
            .gestures
            .front()
            .filter(|g| g.before.is_none())
            .map(|g| g.target)
        {
            let before = Arc::new(self.mask_coverage(target)?);
            self.painted_selections.gestures.front_mut().unwrap().before = Some(before);
        }
        if let Some(gesture) = self
            .painted_selections
            .gestures
            .front_mut()
            .filter(|g| !g.capturing)
        {
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
                        gray: gesture.gray,
                        gradient: gesture.gradient,
                        style: gesture.stroke.style(),
                        dabs,
                        enclosed: area,
                        finish,
                        restart: std::mem::take(&mut gesture.restart),
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
                if !self.painted_selections.gestures.is_empty() {
                    break;
                }
            }
        }
        Ok(changed)
    }
}
