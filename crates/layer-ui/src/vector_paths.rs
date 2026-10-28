use super::*;
use layer_core::{Point, authored::vector::{Anchor, AnchorKind, Cap, Join, PathGeometry, PathItem, Record, Subpath}};
use layer_engine::SampleFlags;
use layer_render::CursorSegment;
use std::sync::Arc;

const CLOSE_REACH: f64 = 6.;

struct Draft { layer: OccurrenceHandle, paint: Option<SourceTarget>, origin: [f64; 2], anchors: Vec<Anchor>, dragging: bool, handle_drag: bool, continues: Option<(PathItem, [f64; 2], bool)> }

#[derive(Default)]
pub(super) struct VectorPaths { draft: Option<Draft>, revision: u64 }
impl VectorPaths {
    pub fn drafting(&self) -> bool { self.draft.is_some() }
    pub fn placing(&self) -> bool { self.draft.as_ref().is_some_and(|draft| draft.dragging) }
}

fn simplified(points: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    fn reduce(points: &[[f64; 2]], tolerance: f64, out: &mut Vec<[f64; 2]>) {
        let (first, last) = (points[0], points[points.len() - 1]);
        let d = [last[0] - first[0], last[1] - first[1]];
        let length = d[0].hypot(d[1]);
        let distance = |p: &[f64; 2]| if length > 0. { ((p[0] - first[0]) * d[1] - (p[1] - first[1]) * d[0]).abs() / length } else { (p[0] - first[0]).hypot(p[1] - first[1]) };
        let farthest = points[1..points.len() - 1].iter().enumerate().map(|(i, p)| (i + 1, distance(p))).max_by(|a, b| a.1.total_cmp(&b.1));
        match farthest {
            Some((at, far)) if far > tolerance => { reduce(&points[..=at], tolerance, out); reduce(&points[at..], tolerance, out); }
            _ => out.push(last),
        }
    }
    if points.len() < 3 { return points.to_vec(); }
    let mut out = vec![points[0]];
    reduce(points, tolerance, &mut out);
    if out.len() > 1 && out.last() == out.first() { out.pop(); }
    out
}

impl<R: CanvasRenderer> UiSession<R> {
    fn vector_path_paint(&self) -> Option<FigurePaint> {
        if let LayerCanvasTool::Figure { shape: FigureShape::Path, paint } = self.layer_interaction.tool { Some(paint) } else { None }
    }
    fn vector_path_target(&self) -> Option<(OccurrenceHandle, Option<SourceTarget>, [f64; 2])> {
        let document = self.engine.document();
        if let Some(layer) = self.vector_figure_layer() { return Some((layer, None, document.scene().occurrence_offset64(layer))); }
        let target = document.drawing_content().filter(|target| matches!(target, SourceTarget::Paint(_)) && !self.state.colors.transparent() && self.selection_masks.target().is_none())?;
        Some((document.target_owner(target)?, Some(target), document.target_offset(target).map(|v| v as f64)))
    }
    pub(super) fn vector_path_pen(&mut self, event: PenEvent, p: Point) -> Result<bool, String> {
        if self.vector_path_paint().is_none() { return Ok(false); }
        if event.flags.contains(SampleFlags::PREDICTED) || event.flags.contains(SampleFlags::CORRECTION) { return Ok(true); }
        let Some((layer, paint, [ox, oy])) = self.vector_path_target() else {
            self.cancel_vector_path();
            if event.phase == PenPhase::Down {
                if self.state.colors.transparent() { return Err(self.localization().text(MessageId::COMMANDS_PATHS_NEED_A_COLOR).to_string()); }
                return Err(self.engine.document().drawing_refusal().map(|refusal| super::notices::drawing_refusal_text(refusal, self.localization()))
                    .unwrap_or_else(|| self.localization().text(MessageId::COMMANDS_DRAW_PATHS_ON_A_VECTOR_LAYER)).to_string());
            }
            return Ok(true);
        };
        let point = [f64::from(p.x) - ox, f64::from(p.y) - oy];
        match event.phase {
            PenPhase::Down => {
                if self.engine.document().is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
                if self.vector_paths.draft.as_ref().is_some_and(|draft| draft.layer != layer) { self.cancel_vector_path(); }
                let reach = CLOSE_REACH * self.document_per_surface_pixel();
                let near = |p: [f64; 2]| (p[0] - point[0]).hypot(p[1] - point[1]) <= reach;
                let first = self.vector_paths.draft.as_ref().filter(|draft| draft.anchors.len() >= 2).map(|draft| draft.continues.as_ref().map_or(draft.anchors[0].point(), |(_, far, _)| *far));
                if first.is_some_and(near) { return self.finish_vector_path(true).map(|()| true); }
                if self.vector_paths.draft.is_none() && paint.is_none() && let Some((original, at_end)) = self.selected_open_end(layer, point, reach) {
                    let [start, end] = original.open_ends().unwrap();
                    let (from, far) = if at_end { (end, start) } else { (start, end) };
                    self.vector_paths.draft = Some(Draft { layer, paint, origin: [ox, oy], anchors: vec![Anchor::corner(from)], dragging: true, handle_drag: false, continues: Some((original, far, at_end)) });
                    self.show_vector_path_preview()?;
                    self.layer_interaction.changed = true;
                    return Ok(true);
                }
                let last = self.vector_paths.draft.as_ref().and_then(|draft| draft.anchors.last()).map(Anchor::point);
                let point = match last.filter(|_| self.interaction.modifiers.shift) {
                    Some(last) => {
                        let end = FigureShape::Line.constrained_end(Point { x: last[0] as f32, y: last[1] as f32 }, Point { x: point[0] as f32, y: point[1] as f32 });
                        [f64::from(end.x), f64::from(end.y)]
                    }
                    None => point,
                };
                let draft = self.vector_paths.draft.get_or_insert(Draft { layer, paint, origin: [ox, oy], anchors: Vec::new(), dragging: false, handle_drag: false, continues: None });
                draft.anchors.push(Anchor::corner(point));
                draft.dragging = true;
                draft.handle_drag = false;
            }
            PenPhase::Move | PenPhase::Up => {
                let alt = self.interaction.modifiers.alt;
                let reach = CLOSE_REACH * self.document_per_surface_pixel();
                let Some(draft) = self.vector_paths.draft.as_mut().filter(|draft| draft.dragging) else { return Ok(true) };
                draft.dragging = event.phase == PenPhase::Move;
                let last = draft.anchors.last_mut().unwrap();
                let at = last.point();
                let out = [point[0] - at[0], point[1] - at[1]];
                draft.handle_drag |= out[0].hypot(out[1]) > reach * 0.5;
                if draft.handle_drag {
                    *last = if alt { Anchor::new(at, last.in_handle(), out, AnchorKind::CORNER) } else { Anchor::new(at, [-out[0], -out[1]], out, AnchorKind::SYMMETRIC) };
                }
            }
            PenPhase::Cancel => { self.cancel_vector_path(); }
            PenPhase::Hover => {}
        }
        self.show_vector_path_preview()?;
        self.layer_interaction.changed = true;
        Ok(true)
    }
    fn drafted_path(&self, closed: bool) -> Result<Option<(OccurrenceHandle, PathItem)>, String> {
        let (Some(draft), Some(paint)) = (&self.vector_paths.draft, self.vector_path_paint()) else { return Ok(None) };
        if draft.anchors.len() < 2 { return Ok(None); }
        let geometry = PathGeometry::new(vec![Subpath::new(&draft.anchors, closed)]).map_err(str::to_string)?;
        let style = self.vector_figure_style(paint, Cap::Butt, Join::Miter)?;
        let id = match draft.paint {
            Some(_) => layer_core::authored::vector::ItemId(1),
            None => self.engine.document().scene().vector_layer(draft.layer).ok_or("The vector layer was removed")?.allocate_id(),
        };
        Ok(Some((draft.layer, PathItem::new(id, Arc::new(geometry), Arc::new(style)))))
    }
    fn show_vector_path_preview(&mut self) -> Result<(), String> {
        let on_paint = self.vector_paths.draft.as_ref().is_some_and(|draft| draft.paint.is_some());
        let preview = self.drafted_path(false)?.filter(|_| !on_paint).map(|(owner, path)| {
            self.vector_paths.revision += 1;
            layer_render::VectorPreview { owner, path: Arc::new(path), revision: self.vector_paths.revision }
        });
        self.engine.backend_mut().set_vector_preview(preview);
        Ok(())
    }
    pub(super) fn finish_vector_path(&mut self, closed: bool) -> Result<(), String> {
        let Some(draft) = &self.vector_paths.draft else { return Ok(()) };
        let current = match draft.paint { Some(target) => self.engine.document().drawing_content() == Some(target), None => self.editing_vector_layer() == Some(draft.layer) };
        let refusal = if self.engine.document().is_locked(draft.layer) { Some(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED) } else { (!current).then_some(MessageId::OBJECTS_NOTICE_STALE) };
        if let Some(refusal) = refusal {
            self.cancel_vector_path();
            return Err(self.localization().text(refusal).to_string());
        }
        let continues = self.vector_paths.draft.as_ref().and_then(|draft| draft.continues.as_ref().map(|(original, _, at_end)| (original.clone(), *at_end)));
        let paint = self.vector_paths.draft.as_ref().and_then(|draft| draft.paint);
        let drafted = self.drafted_path(closed && continues.is_none());
        self.cancel_vector_path();
        let Some((layer, path)) = drafted? else { return Ok(()) };
        if paint.is_some() { return self.paint_path(path); }
        let (id, edit) = match continues {
            Some((original, at_end)) => (original.id, self.continue_path(layer, &original, &path, at_end, closed)?),
            None => {
                let document = self.engine.document();
                let object = document.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
                (path.id, document.vector_object_edit(layer, object.insert_front(vec![Record::Path(path)])).map_err(|e| e.to_string())?)
            }
        };
        self.layer_edit(edit)?;
        self.select_vector_items(layer, [id].into()).map(|_| ())
    }
    pub(super) fn lasso_fill_path(&mut self, layer: OccurrenceHandle, outline: &layer_core::Selection) -> Result<(), String> {
        if self.engine.document().is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
        let [ox, oy] = self.engine.document().scene().occurrence_offset64(layer);
        let tolerance = 0.5 * self.document_per_surface_pixel();
        let subpaths: Vec<Subpath> = outline.contours().iter().filter_map(|contour| {
            let points: Vec<[f64; 2]> = contour.iter().map(|p| [f64::from(p.x) - ox, f64::from(p.y) - oy]).collect();
            let kept = simplified(&points, tolerance);
            (kept.len() >= 3).then(|| Subpath::new(&kept.into_iter().map(Anchor::corner).collect::<Vec<_>>(), true))
        }).collect();
        if subpaths.is_empty() { return Ok(()); }
        let style = self.vector_figure_style(FigurePaint::Fill, Cap::Butt, Join::Miter)?;
        let document = self.engine.document();
        let object = document.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
        let id = object.allocate_id();
        let mut path = PathItem::new(id, Arc::new(PathGeometry::new(subpaths).map_err(str::to_string)?), Arc::new(style));
        path.fill_rule = layer_core::authored::vector::FillRule::EvenOdd;
        let edit = document.vector_object_edit(layer, object.insert_front(vec![Record::Path(path)])).map_err(|e| e.to_string())?;
        self.layer_edit(edit)?;
        self.select_vector_items(layer, [id].into()).map(|_| ())
    }
    pub(super) fn cancel_vector_path(&mut self) -> bool {
        if self.vector_paths.draft.take().is_none() { return false; }
        self.engine.backend_mut().set_vector_preview(None);
        self.layer_interaction.changed = true;
        true
    }
    pub(super) fn remove_vector_path_point(&mut self) -> Result<bool, String> {
        let Some(draft) = &mut self.vector_paths.draft else { return Ok(false) };
        draft.anchors.pop();
        if draft.anchors.is_empty() { self.cancel_vector_path(); } else { self.show_vector_path_preview()?; }
        self.layer_interaction.changed = true;
        Ok(true)
    }
    pub(super) fn vector_path_key(&mut self, key: &str) -> Result<bool, String> {
        if !self.vector_paths.drafting() { return Ok(false); }
        match key {
            "enter" => self.finish_vector_path(false)?,
            "backspace" => { self.remove_vector_path_point()?; }
            "escape" => { self.cancel_vector_path(); }
            _ => return Ok(false),
        }
        self.refresh_tools();
        Ok(true)
    }
    pub(super) fn append_vector_path_overlay(&self, segments: &mut Vec<CursorSegment>) {
        let Some(draft) = &self.vector_paths.draft else { return };
        let map = self.document_to_logical();
        let [ox, oy] = draft.origin;
        let point = |p: [f64; 2]| map(Point { x: (p[0] + ox) as f32, y: (p[1] + oy) as f32 });
        if draft.paint.is_some() && draft.anchors.len() >= 2 && let Ok(geometry) = PathGeometry::new(vec![Subpath::new(&draft.anchors, false)]) {
            let tolerance = 0.25 / f64::from(self.state.camera.zoom.max(1e-3));
            for segment in geometry.segments() {
                let mut last = point(segment.from);
                segment.flatten(tolerance, |p, _| { let next = point(p); segments.push(CursorSegment { from: last, to: next, distance: 0., marker: 0., scale: 1. }); last = next; });
            }
        }
        let half = super::operation::HANDLE_HALF_SIZE;
        for anchor in &draft.anchors {
            let at = point(anchor.point());
            for handle in [anchor.incoming(), anchor.outgoing()].into_iter().filter(|handle| *handle != anchor.point()) {
                let end = point(handle);
                segments.push(CursorSegment { from: at, to: end, distance: 0., marker: 1., scale: 1. });
                segments.push(CursorSegment { from: [end[0] - half * 0.6, end[1] - half * 0.6], to: [end[0] + half * 0.6, end[1] + half * 0.6], distance: 0., marker: 2., scale: 1. });
            }
            segments.push(CursorSegment { from: [at[0] - half, at[1] - half], to: [at[0] + half, at[1] + half], distance: 0., marker: 2., scale: 1. });
        }
        if let (Some(last), false, Some(event)) = (draft.anchors.last(), draft.dragging, self.cursor.event) {
            let to = self.state.camera.input_transform().map(event.surface_position);
            segments.push(CursorSegment { from: point(last.point()), to: map(to), distance: 0., marker: 0., scale: 1. });
        }
    }
}
