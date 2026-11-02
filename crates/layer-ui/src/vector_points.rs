use super::*;
use layer_core::{Point, authored::vector::{Anchor, AnchorKind, AnchorRef, ItemId, PathGeometry, PathItem, Position, Record, SegmentHit, VectorObject}};
use layer_render::CursorSegment;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

const REACH: f64 = 6.;
const DOUBLE_CLICK_NS: u64 = 400_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PointRef { path: ItemId, at: AnchorRef }

#[derive(Clone, Copy)]
enum Part { Anchor(PointRef), Handle(PointRef, bool), Segment(ItemId, SegmentHit) }

#[derive(Clone)]
enum Gesture {
    Switch { layer: OccurrenceHandle, original: VectorObject, anchor: PointRef },
    Drag { layer: OccurrenceHandle, part: Part, press: [f64; 2], original: VectorObject, moved: bool, latest: Option<([f64; 2], Modifiers)>, edited: Option<BTreeMap<Position, PathItem>> },
    Marquee { layer: OccurrenceHandle, start: [f64; 2], end: [f64; 2], adding: bool },
}

#[derive(Clone, Default)]
pub(super) struct VectorPoints { gesture: Option<Gesture>, selected: BTreeSet<PointRef>, layer: Option<OccurrenceHandle>, geometries: BTreeMap<ItemId, Arc<PathGeometry>>, click: Option<(PointRef, [f64; 2], u64)> }
impl VectorPoints {
    pub(super) fn gesturing(&self) -> bool { self.gesture.is_some() }
    pub(super) fn waiting(&self) -> bool { matches!(self.gesture, Some(Gesture::Drag { latest: Some(_), .. })) }
}

fn path_of(object: &VectorObject, id: ItemId) -> Option<&PathItem> { object.record(object.find(id)?)?.path() }
fn anchor_of(object: &VectorObject, r: PointRef) -> Option<Anchor> {
    let subpath = path_of(object, r.path)?.geometry.subpaths().get(r.at.subpath as usize)?;
    ((r.at.anchor as usize) < subpath.len()).then(|| *subpath.anchor(r.at.anchor as usize))
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 { (a[0] - b[0]).hypot(a[1] - b[1]) }

impl<R: CanvasRenderer> UiSession<R> {
    fn points_layer(&self) -> Option<OccurrenceHandle> {
        self.editing_vector_layer().filter(|_| self.layer_interaction.tool == LayerCanvasTool::EditPoints)
    }
    pub(super) fn vector_points_touch_target(&self, point: [f64; 2]) -> bool {
        let Some(layer) = self.points_layer().filter(|layer| !self.engine.document().is_locked(*layer)) else { return false };
        let scene = self.engine.document().scene();
        let Some(object) = scene.vector_layer(layer) else { return false };
        let [ox, oy] = scene.occurrence_offset64(layer);
        let local = [point[0] - ox, point[1] - oy];
        let reach = REACH * self.document_per_surface_pixel();
        self.point_part(object, local, reach).is_some() || object.paths().any(|(_, path)| path.pickable() && path.hit(local, reach))
    }
    fn edited_paths<'a>(&'a self, object: &'a VectorObject) -> Vec<&'a PathItem> {
        let replaced = match &self.vector_points.gesture { Some(Gesture::Drag { edited: Some(edited), .. }) => Some(edited), _ => None };
        let mut paths: Vec<&PathItem> = self.selected_vector_items().map(|selection| object.selected_paths(&selection.items).into_iter()
            .map(|(position, path)| replaced.and_then(|replaced| replaced.get(&position)).unwrap_or(path)).collect()).unwrap_or_default();
        paths.sort_by_key(|path| path.id);
        paths
    }
    fn local_point(&self, layer: OccurrenceHandle, p: Point) -> [f64; 2] {
        let [ox, oy] = self.engine.document().scene().occurrence_offset64(layer);
        [f64::from(p.x) - ox, f64::from(p.y) - oy]
    }
    fn point_selected(&self, object: &VectorObject, r: PointRef) -> bool {
        self.vector_points.layer == self.points_layer() && self.vector_points.selected.contains(&r)
            && self.selected_vector_items().is_some_and(|selection| selection.items.contains(&r.path))
            && self.vector_points.geometries.get(&r.path).zip(path_of(object, r.path)).is_some_and(|(geometry, path)| Arc::ptr_eq(geometry, &path.geometry))
    }
    fn remember_vector_points(&mut self, layer: OccurrenceHandle) {
        self.vector_points.layer = Some(layer);
        self.vector_points.geometries = self.engine.document().scene().vector_layer(layer).map(|object| self.vector_points.selected.iter()
            .filter_map(|r| path_of(object, r.path).map(|path| (r.path, path.geometry.clone()))).collect()).unwrap_or_default();
    }
    fn point_part(&self, object: &VectorObject, point: [f64; 2], reach: f64) -> Option<Part> {
        let handles = self.vector_points.selected.iter().filter(|r| self.point_selected(object, **r)).filter_map(|r| anchor_of(object, *r).map(|a| (*r, a)))
            .flat_map(|(r, a)| [(Part::Handle(r, false), a.incoming(), a.in_handle()), (Part::Handle(r, true), a.outgoing(), a.out_handle())])
            .filter(|(_, _, offset)| *offset != [0.; 2]).map(|(part, at, _)| (part, at));
        let paths = self.edited_paths(object);
        let anchors = paths.iter().flat_map(|path| path.geometry.subpaths().iter().enumerate().flat_map(move |(s, subpath)| subpath.anchors().enumerate()
            .map(move |(i, a)| (Part::Anchor(PointRef { path: path.id, at: AnchorRef { subpath: s as u32, anchor: i as u32 } }), a.point()))));
        handles.chain(anchors).map(|(part, at)| (distance(at, point), part)).filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, part)| part)
            .or_else(|| paths.iter().filter_map(|path| path.nearest_segment(point, reach * 0.1).map(|hit| (path.id, hit)))
                .filter(|(_, hit)| hit.distance <= reach).min_by(|a, b| a.1.distance.total_cmp(&b.1.distance)).map(|(id, hit)| Part::Segment(id, hit)))
    }
    pub(super) fn vector_points_pen(&mut self, event: PenEvent, p: Point) -> Result<bool, String> {
        let Some(layer) = self.points_layer() else { return Ok(false) };
        let point = self.local_point(layer, p);
        match event.phase {
            PenPhase::Down => {
                self.cancel_vector_points();
                if self.engine.document().is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
                let object = self.engine.document().scene().vector_layer(layer).ok_or("The vector layer was removed")?.clone();
                let reach = REACH * self.document_per_surface_pixel();
                let selected: BTreeSet<_> = self.vector_points.selected.iter().copied().filter(|r| self.point_selected(&object, *r) && anchor_of(&object, *r).is_some()).collect();
                if selected != self.vector_points.selected || self.vector_points.layer != Some(layer) { self.vector_points.click = None; }
                self.vector_points.selected = selected;
                let Some(part) = self.point_part(&object, point, reach) else {
                    if let Some(id) = self.vector_moves.index.hit(&object, point, reach) {
                        self.vector_points.selected.clear();
                        self.select_vector_items(layer, [id].into())?;
                        return Ok(true);
                    }
                    self.vector_points.gesture = Some(Gesture::Marquee { layer, start: point, end: point, adding: self.interaction.modifiers.shift });
                    return Ok(true);
                };
                if let Part::Anchor(r) = part {
                    let double = self.vector_points.click.is_some_and(|(last, previous, at)| last == r && distance(previous, point) <= reach && event.timestamp_ns.saturating_sub(at) <= DOUBLE_CLICK_NS);
                    self.vector_points.click = Some((r, point, event.timestamp_ns));
                    if double {
                        self.switch_anchor_kind(layer, &object, r, false)?;
                        self.vector_points.gesture = Some(Gesture::Switch { layer, original: object, anchor: r });
                        return Ok(true);
                    }
                    if self.interaction.modifiers.shift {
                        if !self.vector_points.selected.remove(&r) { self.vector_points.selected.insert(r); }
                    } else if !self.vector_points.selected.contains(&r) {
                        self.vector_points.selected = [r].into();
                    }
                }
                self.remember_vector_points(layer);
                self.vector_points.gesture = Some(Gesture::Drag { layer, part, press: point, original: object, moved: false, latest: None, edited: None });
            }
            PenPhase::Move | PenPhase::Up => {
                let (modifiers, reach) = (self.interaction.modifiers, REACH * self.document_per_surface_pixel());
                match &mut self.vector_points.gesture {
                    Some(Gesture::Switch { .. }) => (),
                    Some(Gesture::Marquee { end, .. }) => *end = point,
                    Some(Gesture::Drag { press, moved, latest, .. }) => {
                        *moved |= distance(point, *press) >= reach * 0.5;
                        if *moved { self.vector_points.click = None; }
                        *latest = Some((point, modifiers));
                    }
                    None => return Ok(false),
                }
                if event.phase == PenPhase::Up {
                    self.take_points_sample()?;
                    self.finish_vector_points()?;
                    self.layer_interaction.changed = true;
                }
            }
            PenPhase::Cancel => { self.cancel_vector_points(); }
            PenPhase::Hover => return Ok(false),
        }
        if event.phase == PenPhase::Down { self.layer_interaction.changed = true; }
        Ok(true)
    }
    pub(super) fn sync_vector_points(&mut self) -> Result<(), String> {
        if !self.take_points_sample()? { return Ok(()); }
        let items = self.selected_vector_items().map(|selection| selection.items.clone()).unwrap_or_default();
        let Some(Gesture::Drag { layer, original, edited: Some(edited), .. }) = &self.vector_points.gesture else { return Ok(()) };
        let shown = original.selected_paths(&items).into_iter().map(|(position, path)| edited.get(&position).unwrap_or(path).clone()).collect();
        let layer = *layer;
        self.preview_vector_paths(layer, items, shown);
        Ok(())
    }
    fn take_points_sample(&mut self) -> Result<bool, String> {
        let Some(Gesture::Drag { latest, .. }) = &mut self.vector_points.gesture else { return Ok(false) };
        let Some((point, modifiers)) = latest.take() else { return Ok(false) };
        self.drag_vector_points(point, modifiers)
    }
    fn drag_vector_points(&mut self, point: [f64; 2], modifiers: Modifiers) -> Result<bool, String> {
        let selected = self.vector_points.selected.clone();
        let Some(Gesture::Drag { part, press, original, moved: true, edited, .. }) = &mut self.vector_points.gesture else { return Ok(false) };
        let mut delta = [point[0] - press[0], point[1] - press[1]];
        if modifiers.shift && matches!(part, Part::Anchor(_)) {
            let end = FigureShape::Line.constrained_end(Point { x: 0., y: 0. }, Point { x: delta[0] as f32, y: delta[1] as f32 });
            delta = [f64::from(end.x), f64::from(end.y)];
        }
        if delta == [0.; 2] { *edited = Some(BTreeMap::new()); return Ok(true); }
        let mut changes: BTreeMap<ItemId, Vec<(AnchorRef, Anchor)>> = BTreeMap::new();
        match *part {
            Part::Anchor(_) => for r in &selected {
                let Some(a) = anchor_of(original, *r) else { continue };
                let p = a.point();
                changes.entry(r.path).or_default().push((r.at, Anchor::new([p[0] + delta[0], p[1] + delta[1]], a.in_handle(), a.out_handle(), a.kind)));
            },
            Part::Handle(r, outgoing) => {
                let a = anchor_of(original, r).ok_or("The anchor was removed")?;
                let p = a.point();
                let handle = [point[0] - p[0], point[1] - p[1]];
                let other = if outgoing { a.in_handle() } else { a.out_handle() };
                let length = other[0].hypot(other[1]);
                let (kind, other) = match a.kind {
                    _ if modifiers.alt => (AnchorKind::CORNER, other),
                    AnchorKind::SYMMETRIC => (a.kind, [-handle[0], -handle[1]]),
                    AnchorKind::SMOOTH => { let h = handle[0].hypot(handle[1]).max(1e-9); (a.kind, [-handle[0] / h * length, -handle[1] / h * length]) }
                    kind => (kind, other),
                };
                let (incoming, outgoing) = if outgoing { (other, handle) } else { (handle, other) };
                changes.entry(r.path).or_default().push((r.at, Anchor::new(p, incoming, outgoing, kind)));
            }
            Part::Segment(id, hit) => {
                let path = path_of(original, id).ok_or("The path was removed")?;
                let subpath = &path.geometry.subpaths()[hit.subpath as usize];
                let (a, b) = (hit.segment as usize, (hit.segment as usize + 1) % subpath.len());
                let t = hit.t.clamp(0.1, 0.9);
                let k = 1. / (3. * t * (1. - t));
                let push = [delta[0] * k, delta[1] * k];
                let (first, second) = (*subpath.anchor(a), *subpath.anchor(b));
                let out = first.out_handle();
                let incoming = second.in_handle();
                let at = |anchor| AnchorRef { subpath: hit.subpath, anchor: anchor as u32 };
                changes.entry(id).or_default().extend([
                    (at(a), Anchor::new(first.point(), first.in_handle(), [out[0] + push[0], out[1] + push[1]], AnchorKind::CORNER)),
                    (at(b), Anchor::new(second.point(), [incoming[0] + push[0], incoming[1] + push[1]], second.out_handle(), AnchorKind::CORNER)),
                ]);
            }
        }
        let mut replaced = BTreeMap::new();
        for (id, anchors) in changes {
            let Some(position) = original.find(id) else { continue };
            replaced.insert(position, path_of(original, id).ok_or("The path was removed")?.with_anchors(anchors).map_err(str::to_string)?);
        }
        *edited = Some(replaced);
        Ok(true)
    }
    fn commit_vector_points(&mut self, layer: OccurrenceHandle, original: VectorObject, edited: BTreeMap<Position, PathItem>) -> Result<(), String> {
        self.end_path_preview();
        if edited.iter().all(|(position, path)| original.record(*position).and_then(Record::path) == Some(path)) { return Ok(()); }
        let edit = self.engine.document().vector_object_edit(layer, original.with_paths(edited).map_err(str::to_string)?).map_err(|e| e.to_string())?;
        self.layer_edit(edit)?;
        self.remember_vector_points(layer);
        Ok(())
    }
    fn finish_vector_points(&mut self) -> Result<(), String> {
        let Some(gesture) = self.vector_points.gesture.take() else { return Ok(()) };
        match gesture {
            Gesture::Switch { layer, original, anchor } => self.switch_anchor_kind(layer, &original, anchor, true),
            Gesture::Drag { layer, original, edited: Some(edited), .. } => self.commit_vector_points(layer, original, edited),
            Gesture::Drag { layer, part: Part::Segment(id, hit), original, edited: None, .. } => {
                let path = path_of(&original, id).ok_or("The path was removed")?.with_anchor_inserted(hit.subpath, hit.segment, hit.t).map_err(str::to_string)?;
                let position = original.find(id).ok_or("The path was removed")?;
                let edit = self.engine.document().vector_object_edit(layer, original.replace([(position, Some(Record::Path(path)))])).map_err(|e| e.to_string())?;
                self.layer_edit(edit)?;
                self.vector_points.selected = [PointRef { path: id, at: AnchorRef { subpath: hit.subpath, anchor: hit.segment + 1 } }].into();
                self.remember_vector_points(layer);
                Ok(())
            }
            Gesture::Drag { .. } => Ok(()),
            Gesture::Marquee { layer, start, end, adding } => {
                let object = self.engine.document().scene().vector_layer(layer).ok_or("The vector layer was removed")?;
                let area = [start[0].min(end[0]), start[1].min(end[1]), start[0].max(end[0]), start[1].max(end[1])];
                let inside = |p: [f64; 2]| p[0] >= area[0] && p[0] <= area[2] && p[1] >= area[1] && p[1] <= area[3];
                let mut found = Vec::new();
                for path in self.edited_paths(object) {
                    for (s, subpath) in path.geometry.subpaths().iter().enumerate() {
                        found.extend(subpath.anchors().enumerate().filter(|(_, a)| inside(a.point())).map(|(i, _)| PointRef { path: path.id, at: AnchorRef { subpath: s as u32, anchor: i as u32 } }));
                    }
                }
                if !adding { self.vector_points.selected.clear(); }
                self.vector_points.selected.extend(found);
                self.remember_vector_points(layer);
                Ok(())
            }
        }
    }
    fn switch_anchor_kind(&mut self, layer: OccurrenceHandle, object: &VectorObject, r: PointRef, commit: bool) -> Result<(), String> {
        let original = path_of(object, r.path).ok_or("The path was removed")?;
        let path = original.with_anchor_kind_switched(r.at).map_err(str::to_string)?;
        self.vector_points.click = None;
        if path.geometry == original.geometry { return Ok(()); }
        let position = object.find(r.path).ok_or("The path was removed")?;
        if !commit {
            self.preview_vector_paths(layer, Arc::new([r.path].into()), vec![path].into());
            return Ok(());
        }
        self.end_path_preview();
        let edit = self.engine.document().vector_object_edit(layer, object.replace([(position, Some(Record::Path(path)))])).map_err(|e| e.to_string())?;
        self.layer_edit(edit)?;
        self.remember_vector_points(layer);
        Ok(())
    }
    pub(super) fn cancel_vector_points(&mut self) -> bool {
        let Some(gesture) = self.vector_points.gesture.take() else { return false };
        if matches!(gesture, Gesture::Drag { edited: Some(_), .. } | Gesture::Switch { .. }) { self.end_path_preview(); }
        self.layer_interaction.changed = true;
        true
    }
    fn selected_points(&self) -> BTreeMap<ItemId, BTreeSet<AnchorRef>> {
        let mut grouped: BTreeMap<ItemId, BTreeSet<AnchorRef>> = BTreeMap::new();
        if self.points_layer().is_none() { return grouped; }
        let edited = self.selected_vector_items().map(|selection| selection.items.clone()).unwrap_or_default();
        let object = self.points_layer().and_then(|layer| self.engine.document().scene().vector_layer(layer));
        for r in self.vector_points.selected.iter().filter(|r| edited.contains(&r.path) && object.is_some_and(|object| self.point_selected(object, **r))) { grouped.entry(r.path).or_default().insert(r.at); }
        grouped
    }
    pub(super) fn vector_points_split(&self) -> bool {
        let Some(object) = self.points_layer().and_then(|layer| self.engine.document().scene().vector_layer(layer)) else { return false };
        self.selected_points().iter().any(|(id, anchors)| path_of(object, *id).is_some_and(|path| anchors.iter().any(|at| path.cuts_at(*at))))
    }
    pub(super) fn take_selected_vector_points(&mut self) -> BTreeMap<ItemId, BTreeSet<AnchorRef>> {
        let points = self.selected_points();
        self.vector_points.selected.clear();
        points
    }
    pub(super) fn delete_selected_vector_points(&mut self) -> Result<bool, String> {
        let grouped = self.selected_points();
        if self.points_layer().is_some() && grouped.is_empty() && !self.vector_points.selected.is_empty() { self.vector_points.selected.clear(); return Ok(true); }
        let Some(layer) = self.points_layer().filter(|_| !grouped.is_empty()) else { return Ok(false) };
        let document = self.engine.document();
        let object = document.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
        let mut changes = Vec::new();
        for (id, anchors) in grouped {
            let (Some(position), Some(path)) = (object.find(id), path_of(object, id)) else { continue };
            changes.push((position, path.without_anchors(&anchors).map_err(str::to_string)?.map(Record::Path)));
        }
        let edit = document.vector_object_edit(layer, object.replace(changes)).map_err(|e| e.to_string())?;
        self.vector_points.selected.clear();
        self.layer_edit(edit)?;
        Ok(true)
    }
    pub(super) fn append_vector_points_overlay(&self, segments: &mut Vec<CursorSegment>) {
        let Some(layer) = self.points_layer() else { return };
        let doc = self.engine.document();
        let Some(object) = doc.scene().vector_layer(layer) else { return };
        let map = self.document_to_logical();
        let [ox, oy] = doc.scene().occurrence_offset64(layer);
        let point = |p: [f64; 2]| map(Point { x: (p[0] + ox) as f32, y: (p[1] + oy) as f32 });
        let half = super::operation::HANDLE_HALF_SIZE;
        let mut square = |at: [f32; 2], size: f32, filled: bool| if filled {
            segments.push(CursorSegment { from: [at[0] - size, at[1] - size], to: [at[0] + size, at[1] + size], distance: 0., marker: 2., scale: 1. });
        } else {
            let corners = [[at[0] - size, at[1] - size], [at[0] + size, at[1] - size], [at[0] + size, at[1] + size], [at[0] - size, at[1] + size]];
            for i in 0..4 { segments.push(CursorSegment { from: corners[i], to: corners[(i + 1) % 4], distance: 0., marker: 1., scale: 1. }); }
        };
        let mut handles = Vec::new();
        for path in self.edited_paths(object) {
            let id = path.id;
            for (s, subpath) in path.geometry.subpaths().iter().enumerate() {
                for (i, anchor) in subpath.anchors().enumerate() {
                    let r = PointRef { path: id, at: AnchorRef { subpath: s as u32, anchor: i as u32 } };
                    let selected = self.point_selected(object, r);
                    square(point(anchor.point()), half, selected);
                    if selected { handles.push(*anchor); }
                }
            }
        }
        for anchor in handles {
            for (handle, offset) in [(anchor.incoming(), anchor.in_handle()), (anchor.outgoing(), anchor.out_handle())] {
                if offset == [0.; 2] { continue; }
                segments.push(CursorSegment { from: point(anchor.point()), to: point(handle), distance: 0., marker: 1., scale: 1. });
                let at = point(handle);
                segments.push(CursorSegment { from: [at[0] - half * 0.6, at[1] - half * 0.6], to: [at[0] + half * 0.6, at[1] + half * 0.6], distance: 0., marker: 2., scale: 1. });
            }
        }
        if let Some(Gesture::Marquee { start, end, .. }) = &self.vector_points.gesture {
            let corners = [[start[0], start[1]], [end[0], start[1]], [end[0], end[1]], [start[0], end[1]]].map(point);
            for i in 0..4 { segments.push(CursorSegment { from: corners[i], to: corners[(i + 1) % 4], distance: 0., marker: 0., scale: 1. }); }
        }
    }
}
