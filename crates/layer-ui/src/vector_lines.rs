use super::*;
use layer_core::authored::vector::{Cap, Eraser, ItemId, Join, Paint, PaintEntry, PathItem, Record, Stroke, Style, VectorObject};
use layer_engine::{SampleFlags, VectorSample, VectorStroke};
use std::sync::Arc;

const LINE_TOLERANCE: f64 = 0.5;

struct Line { layer: OccurrenceHandle, stroke: VectorStroke, paint: Paint, shown: Vec<VectorSample>, continues: Option<(PathItem, bool)> }
struct Drawn { line: Line, id: ItemId, checkpoint: u64 }
struct Erasing { layer: OccurrenceHandle, original: VectorObject, current: VectorObject, shown: VectorObject, last: [f64; 2], pending: Vec<[f64; 2]>, radius: f64, mode: layer_core::authored::vector::EraseMode }

#[derive(Default)]
pub(super) struct VectorLines { line: Option<Line>, drawn: Option<Drawn>, erasing: Option<Erasing>, revision: u64, pen: Option<u32>, restore: Option<(u32, u32)>, on_vector: bool }
impl VectorLines {
    pub fn active(&self) -> bool { self.line.is_some() }
    pub fn erasing(&self) -> bool { self.erasing.is_some() }
    pub fn erase_waiting(&self) -> bool { self.erasing.as_ref().is_some_and(|erasing| !erasing.pending.is_empty()) }
    pub fn corrects(&self, event: &PenEvent) -> bool { self.drawn.as_ref().is_some_and(|drawn| drawn.line.stroke.corrects(event)) }
}

fn path(id: ItemId, paint: &Paint, line: (layer_core::authored::vector::PathGeometry, Option<layer_core::authored::vector::WidthProfile>, f64)) -> PathItem {
    let (geometry, profile, width) = line;
    let mut stroke = Stroke::new(paint.clone(), width);
    stroke.cap = Cap::Round;
    stroke.join = Join::Round;
    stroke.profile = profile.map(Arc::new);
    PathItem::new(id, Arc::new(geometry), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])))
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn vector_contact(&self) -> bool {
        self.vector_lines.active() || self.vector_lines.erasing() || self.vector_moves.gesturing() || self.vector_points.gesturing() || self.vector_paths.placing()
    }
    pub(super) fn editing_vector_layer(&self) -> Option<OccurrenceHandle> {
        let document = self.engine.document();
        let layer = document.working.occurrence?;
        (document.scene().vector_layer(layer).is_some() && !matches!(document.working.target, Some(SourceTarget::Coverage(_)))
            && self.selection_masks.target().is_none()).then_some(layer)
    }
    fn draws_paths(&self, id: u32) -> bool {
        tools::tool(id) == Tool::Eraser || tools::preset(id).is_ok_and(|preset| self.tools.brush(preset).draws_vector_lines())
    }
    pub(super) fn vector_pen(&self) -> Option<u32> {
        self.vector_lines.pen.into_iter().chain(tools::group_presets(ToolGroup::Pen)).chain(tools::brush_ids())
            .find(|id| tools::tool(*id) != Tool::Eraser && self.draws_paths(*id))
    }
    pub(super) fn path_brush_refusal(&self, action: &UiAction) -> Option<Arc<str>> {
        self.editing_vector_layer()?;
        let usable = match *action {
            UiAction::SelectBrush { id } => self.draws_paths(id),
            UiAction::SelectToolGroup { group } | UiAction::SelectBrushSet { group } => tools::group_presets(group).any(|id| self.draws_paths(id)),
            _ => true,
        };
        if usable { return None; }
        let l = self.localization();
        let blocker = match *action {
            UiAction::SelectBrush { id } => tools::preset(id).ok().and_then(|preset| {
                let setting = self.tools.path_blocker(preset)?;
                crate::tool_settings::controls(preset as u32, &self.tools.brush(preset), l).into_iter().find(|control| control.id == setting).map(|control| control.label)
            }),
            _ => None,
        };
        Some(match blocker {
            Some(setting) => {
                let mut args = FluentArgs::new(); args.set("setting", setting.as_ref());
                l.format(MessageId::OBJECTS_VECTOR_UNAVAILABLE_SETTING, &args).into()
            }
            None => l.text(MessageId::OBJECTS_VECTOR_UNAVAILABLE),
        })
    }
    pub(super) fn mark_path_brushes(&self, mut view: ToolSetView) -> ToolSetView {
        for item in view.groups.iter_mut().chain(&mut view.subtools) {
            if let Some(reason) = self.path_brush_refusal(&item.action) { item.disabled_reason = Some(reason); }
        }
        view
    }
    pub(super) fn vector_eraser_modes(&self) -> Vec<ToolSetItem> {
        if self.vector_erasing_layer(false).is_none() { return Vec::new(); }
        [CommandId::EraseTouchedPart, CommandId::EraseWholePaths, CommandId::EraseToIntersection].into_iter().map(|command| ToolSetItem {
            enabled: true, disabled_reason: None, label: command.localized_label(self.localization()), icon: command.icon().unwrap_or("eraser"),
            action: UiAction::Invoke { command }, selected: super::vector_eraser(command) == Some(self.state.settings.vector_eraser), preview: None,
        }).collect()
    }
    pub(super) fn vector_path_tools(&self) -> Vec<ToolSetItem> {
        if self.editing_vector_layer().is_none() { return Vec::new(); }
        let l = self.localization();
        [(CommandId::Move, LayerCanvasTool::Move, l.text(MessageId::TOOL_SELECT_PATHS)),
            (CommandId::EditPoints, LayerCanvasTool::EditPoints, CommandId::EditPoints.localized_label(l))]
            .into_iter().map(|(command, tool, label)| {
                let state = self.command(command);
                ToolSetItem { enabled: state.enabled, disabled_reason: state.disabled_reason, label, icon: command.icon().unwrap_or("move"),
                action: UiAction::Invoke { command }, selected: self.layer_interaction.tool == tool, preview: None,
            }}).collect()
    }
    pub(super) fn applies_to_paths(&self, setting: &str) -> bool {
        if self.editing_vector_layer().is_none() || self.layer_interaction.tool != LayerCanvasTool::Paint { return true; }
        if self.state.brush.tool == Tool::Eraser || self.state.colors.transparent() { return setting == "size"; }
        !self.engine.configured_brush().draws_vector_lines() || !matches!(setting, "spacing" | "angle" | "rotation_jitter")
    }
    pub(super) fn remember_vector_pen(&mut self, id: u32) {
        if tools::tool(id) != Tool::Eraser && self.engine.configured_brush().draws_vector_lines() { self.vector_lines.pen = Some(id); }
    }
    pub(super) fn forget_brush_restore(&mut self) {
        self.vector_lines.restore = None;
    }
    pub(super) fn follow_layer_brush(&mut self) -> bool {
        let vector = self.editing_vector_layer().is_some();
        if std::mem::replace(&mut self.vector_lines.on_vector, vector) == vector { return false; }
        if self.layer_interaction.tool != LayerCanvasTool::Paint {
            self.forget_brush_restore();
            return false;
        }
        let current = self.state.brush.preset;
        if !vector {
            if let Some((brush, pen)) = self.vector_lines.restore.take() && pen == current { return self.select_brush(brush).is_ok(); }
        } else if self.state.brush.tool != Tool::Eraser && !self.engine.configured_brush().draws_vector_lines()
            && let Some(pen) = self.vector_pen() && self.select_brush(pen).is_ok() {
            self.vector_lines.restore = Some((current, pen));
            return true;
        }
        false
    }
    pub(super) fn vector_drawing_layer(&self) -> Option<OccurrenceHandle> {
        let layer = self.editing_vector_layer()?;
        let erasing = self.state.brush.tool == Tool::Eraser || self.state.colors.transparent();
        (!erasing && self.engine.configured_brush().draws_vector_lines()).then_some(layer)
    }
    pub(super) fn vector_brush_accepts(&self, erasing: bool) -> bool {
        self.vector_drawing_layer().or_else(|| self.vector_erasing_layer(erasing)).is_some_and(|layer| !self.engine.document().is_locked(layer))
    }
    pub(super) fn erasing_event(&self, event: &PenEvent) -> bool {
        event.tool == layer_engine::ToolKind::Eraser || event.flags.contains(SampleFlags::INVERTED)
    }
    pub(super) fn vector_erasing_layer(&self, eraser_end: bool) -> Option<OccurrenceHandle> {
        let layer = self.editing_vector_layer()?;
        let erasing = eraser_end || self.state.brush.tool == Tool::Eraser || self.state.colors.transparent();
        (erasing && self.layer_interaction.tool == LayerCanvasTool::Paint).then_some(layer)
    }
    pub(super) fn vector_erase_pen(&mut self, event: PenEvent) -> Result<(), String> {
        if event.flags.contains(SampleFlags::PREDICTED) || event.flags.contains(SampleFlags::CORRECTION) { return Ok(()); }
        let p = self.state.camera.input_transform().map(event.surface_position);
        let point = [f64::from(p.x), f64::from(p.y)];
        match event.phase {
            PenPhase::Down => {
                self.cancel_vector_erase();
                let Some(layer) = self.vector_erasing_layer(self.erasing_event(&event)) else { return Ok(()) };
                if self.engine.document().is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
                let document = self.engine.document();
                let object = document.scene().vector_layer(layer).ok_or("The vector layer was removed")?.clone();
                let offset = document.scene().occurrence_offset64(layer);
                let local = [point[0] - offset[0], point[1] - offset[1]];
                let radius = f64::from(self.engine.configured_brush().diameter).max(1.) * 0.5;
                self.vector_lines.erasing = Some(Erasing { layer, original: object.clone(), current: object.clone(), shown: object, last: local, pending: vec![local], radius, mode: self.state.settings.vector_eraser.mode() });
                Ok(())
            }
            PenPhase::Move => { self.queue_vector_erase(point); Ok(()) }
            PenPhase::Up => {
                self.queue_vector_erase(point);
                self.erase_queued_samples();
                self.finish_vector_erase()
            }
            PenPhase::Cancel => { self.cancel_vector_erase(); Ok(()) }
            PenPhase::Hover => Ok(()),
        }
    }
    fn queue_vector_erase(&mut self, point: [f64; 2]) {
        let Some(erasing) = &mut self.vector_lines.erasing else { return };
        let offset = self.engine.document().scene().occurrence_offset64(erasing.layer);
        erasing.pending.push([point[0] - offset[0], point[1] - offset[1]]);
    }
    fn erase_queued_samples(&mut self) {
        let Some(erasing) = &mut self.vector_lines.erasing else { return };
        let erasers: Vec<Eraser> = std::mem::take(&mut erasing.pending).into_iter().map(|local| {
            let eraser = Eraser { from: erasing.last, to: local, radius: erasing.radius };
            erasing.last = local;
            eraser
        }).collect();
        if let Some(erased) = erasing.current.erase_along(&mut self.vector_moves.index, &erasers, erasing.mode) { erasing.current = erased; }
    }
    pub(super) fn sync_vector_erase(&mut self) -> Result<(), String> {
        self.erase_queued_samples();
        let Some(erasing) = self.vector_lines.erasing.as_mut().filter(|erasing| !erasing.shown.same_root(&erasing.current)) else { return Ok(()) };
        erasing.shown = erasing.current.clone();
        let edit = self.engine.document().vector_object_edit(erasing.layer, erasing.current.clone()).map_err(|e| e.to_string())?;
        self.engine.preview_edit(edit).map_err(|e| e.to_string())
    }
    fn finish_vector_erase(&mut self) -> Result<(), String> {
        let Some(erasing) = self.vector_lines.erasing.take() else { return Ok(()) };
        if erasing.current.same_root(&erasing.original) { return Ok(()); }
        if !erasing.shown.same_root(&erasing.original) {
            let restore = self.engine.document().vector_object_edit(erasing.layer, erasing.original).map_err(|e| e.to_string())?;
            self.engine.preview_edit(restore).map_err(|e| e.to_string())?;
        }
        let edit = self.engine.document().vector_object_edit(erasing.layer, erasing.current).map_err(|e| e.to_string())?;
        self.layer_edit(edit)
    }
    pub(super) fn cancel_vector_erase(&mut self) {
        let Some(erasing) = self.vector_lines.erasing.take() else { return };
        if erasing.shown.same_root(&erasing.original) { return; }
        if let Ok(edit) = self.engine.document().vector_object_edit(erasing.layer, erasing.original) { let _ = self.engine.preview_edit(edit); }
        self.layer_interaction.changed = true;
    }
    pub(super) fn vector_line_pen(&mut self, event: PenEvent) -> Result<(), String> {
        if event.flags.contains(SampleFlags::CORRECTION) { return self.correct_vector_line(event); }
        if event.flags.contains(SampleFlags::PREDICTED) {
            if event.phase == PenPhase::Move { self.push_vector_sample(event); }
            return Ok(());
        }
        match event.phase {
            PenPhase::Down => {
                self.cancel_vector_line();
                self.vector_lines.drawn = None;
                let Some(layer) = self.vector_drawing_layer() else { return Ok(()) };
                if self.engine.document().is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
                let brush = self.engine.configured_brush();
                let space = self.engine.document().composition().color.space;
                let [r, g, b, a] = brush.color_rgba_linear;
                let color = layer_core::color::RgbColor::from_linear(space, [r, g, b, (a * brush.opacity).clamp(0., 1.)]).map_err(|e| e.to_string())?;
                let offset = self.engine.document().scene().occurrence_offset64(layer);
                let p = self.state.camera.input_transform().map(event.surface_position);
                let local = [f64::from(p.x) - offset[0], f64::from(p.y) - offset[1]];
                let continues = self.selected_open_end(layer, local, 6. * self.document_per_surface_pixel());
                let stroke = self.engine.begin_vector_stroke(self.vector_lines.revision + 1, event, offset, LINE_TOLERANCE * self.document_per_surface_pixel());
                self.vector_lines.line = Some(Line { layer, stroke, paint: Paint::color(color), shown: Vec::new(), continues });
            }
            PenPhase::Move => self.push_vector_sample(event),
            PenPhase::Up => {
                self.push_vector_sample(event);
                self.finish_vector_line()?;
            }
            PenPhase::Cancel => self.cancel_vector_line(),
            PenPhase::Hover => {}
        }
        Ok(())
    }
    fn push_vector_sample(&mut self, event: PenEvent) {
        let Some(line) = &mut self.vector_lines.line else { return };
        self.engine.push_vector_stroke(&mut line.stroke, event);
    }
    pub(super) fn sync_vector_line_preview(&mut self, now_ns: u64, presentation_ns: u64) {
        let Some(line) = &mut self.vector_lines.line else { return };
        let samples = line.stroke.preview(Some(now_ns), Some(presentation_ns));
        if samples == line.shown { return; }
        let Some(preview) = layer_engine::polyline(&samples) else { return };
        line.shown = samples;
        self.vector_lines.revision += 1;
        let preview = layer_render::VectorPreview { owner: line.layer, path: Arc::new(path(ItemId(1), &line.paint, preview)), revision: self.vector_lines.revision };
        self.engine.backend_mut().set_vector_preview(Some(preview));
    }
    pub(super) fn cancel_vector_stroke(&mut self) -> bool {
        let active = self.vector_lines.active() || self.vector_lines.erasing();
        self.cancel_vector_line();
        self.cancel_vector_erase();
        active
    }
    pub(super) fn cancel_vector_line(&mut self) {
        if self.vector_lines.line.take().is_some() { self.engine.backend_mut().set_vector_preview(None); }
    }
    fn finish_vector_line(&mut self) -> Result<(), String> {
        let Some(line) = self.vector_lines.line.take() else { return Ok(()) };
        let result = (|| {
            let Some(fitted) = line.stroke.fitted() else { return Ok(()) };
            let document = self.engine.document();
            let object = document.scene().vector_layer(line.layer).ok_or("The vector layer was removed")?;
            let (id, edit) = match &line.continues {
                Some((original, at_end)) => (original.id, self.continue_path(line.layer, original, &path(original.id, &line.paint, fitted), *at_end, false)?),
                None => {
                    let id = object.allocate_id();
                    (id, document.vector_object_edit(line.layer, object.insert_front(vec![Record::Path(path(id, &line.paint, fitted))])).map_err(|e| e.to_string())?)
                }
            };
            self.layer_edit(edit)?;
            self.layer_interaction.changed = true;
            if line.stroke.awaits_corrections() { self.vector_lines.drawn = Some(Drawn { line, id, checkpoint: self.engine.checkpoint() }); }
            Ok(())
        })();
        self.engine.backend_mut().set_vector_preview(None);
        result
    }
    fn correct_vector_line(&mut self, event: PenEvent) -> Result<(), String> {
        if let Some(line) = &mut self.vector_lines.line {
            self.engine.push_vector_stroke(&mut line.stroke, event);
            return Ok(());
        }
        let Some(mut drawn) = self.vector_lines.drawn.take() else { return Ok(()) };
        if drawn.checkpoint != self.engine.checkpoint() || !self.engine.push_vector_stroke(&mut drawn.line.stroke, event) {
            if drawn.checkpoint == self.engine.checkpoint() && drawn.line.stroke.awaits_corrections() { self.vector_lines.drawn = Some(drawn); }
            return Ok(());
        }
        let Some(fitted) = drawn.line.stroke.fitted() else { return Ok(()) };
        let document = self.engine.document();
        let object = document.scene().vector_layer(drawn.line.layer).ok_or("The vector layer was removed")?;
        let Some(position) = object.find(drawn.id) else { return Ok(()) };
        let edit = match &drawn.line.continues {
            Some((original, at_end)) => self.continue_path(drawn.line.layer, original, &path(drawn.id, &drawn.line.paint, fitted), *at_end, false)?,
            None => document.vector_object_edit(drawn.line.layer, object.replace([(position, Some(Record::Path(path(drawn.id, &drawn.line.paint, fitted))))])).map_err(|e| e.to_string())?,
        };
        self.engine.amend_edit(edit).map_err(|e| e.to_string())?;
        self.layer_interaction.changed = true;
        drawn.checkpoint = self.engine.checkpoint();
        if drawn.line.stroke.awaits_corrections() { self.vector_lines.drawn = Some(drawn); }
        Ok(())
    }
}
