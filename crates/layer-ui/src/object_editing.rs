//! Image objects inside object layers: typed selection, Move-tool picking,
//! binary64 transform gestures, ordering and the object rows of the layer panel.
use super::*;
use super::operation::Snapping;
use layer_core::{Edit, ImageInterpolation, ObjectOrder, Point, Rect};
use layer_core::authored::{Affine64, ImageObjectHandle, OccurrenceHandle, SourceTarget};
use layer_engine::{PenEvent, PenPhase};
use layer_render::CursorSegment;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

type InsertedObjects = (Edit, Vec<ImageObjectHandle>, Vec<(SourceTarget, layer_core::RasterOperation)>);

const UNIT_HANDLES: [[f64; 2]; 8] = [[0., 0.], [0.5, 0.], [1., 0.], [1., 0.5], [1., 1.], [0.5, 1.], [0., 1.], [0., 0.5]];

pub fn object_token(handle: ImageObjectHandle) -> u64 { handle.wire_id() }
pub fn object_handle(token: u64) -> Result<ImageObjectHandle, String> {
    ImageObjectHandle::from_wire_id(token).ok_or_else(|| "Invalid image identity".into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ObjectAction {
    Select { id: u64, #[serde(default)] extend: bool },
    Expand { layer: u64, expanded: bool },
    Visibility { id: u64, visible: bool },
    Rename { id: u64, name: String },
    Order { order: ObjectOrder },
    Drop { id: u64, target: u64, below: bool },
    Interpolation { nearest: bool },
    SelectAll,
    Deselect,
    Delete,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObjectRow {
    pub id: u64,
    pub layer: u64,
    pub label: String,
    pub visible: bool,
    pub selected: bool,
    pub editable: bool,
    pub index: u32,
    pub can_raise: bool,
    pub can_lower: bool,
    pub nearest: bool,
    pub thumbnail_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ObjectHandle { Move, Scale([f64; 2]), Rotate, Pivot }

#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame { unit: Affine64, pivot: [f64; 2] }
impl Frame {
    fn moved(self, delta: Affine64) -> Self { Self { unit: delta.compose(self.unit), pivot: delta.map(self.pivot) } }
    fn handles(self, reach: f64) -> Vec<(ObjectHandle, [f64; 2])> {
        let top = self.unit.map([0.5, 0.]);
        let centre = self.unit.map([0.5, 0.5]);
        let outward = [top[0] - centre[0], top[1] - centre[1]];
        let length = outward[0].hypot(outward[1]).max(f64::MIN_POSITIVE);
        let rotate = [top[0] + outward[0] / length * reach * 2.5, top[1] + outward[1] / length * reach * 2.5];
        UNIT_HANDLES.into_iter().map(|unit| (ObjectHandle::Scale(unit), self.unit.map(unit)))
            .chain([(ObjectHandle::Rotate, rotate), (ObjectHandle::Pivot, self.pivot)]).collect()
    }
}

struct Gesture {
    handle: ObjectHandle,
    nudge: Option<String>,
    press: [f64; 2],
    setup: Option<(Edit, Edit)>,
    copy: bool,
    start: Vec<(ImageObjectHandle, Affine64)>,
    frame: Frame,
    delta: Affine64,
    snapping: Option<(Snapping, [f64; 2])>,
    moved: bool,
    pivot: bool,
}

struct Placement { insert: Edit, inverse: Edit, objects: Vec<ImageObjectHandle> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObjectDestination { Existing(OccurrenceHandle), New { index: usize, parent: Option<OccurrenceHandle> } }

#[derive(Default)]
pub(super) struct ObjectEditing {
    gesture: Option<Gesture>,
    placement: Option<Placement>,
    frame: Option<(BTreeSet<ImageObjectHandle>, u64, Frame)>,
    session: Option<(OccurrenceHandle, Vec<(ImageObjectHandle, Affine64)>)>,
    last: Option<(u64, u64, Affine64)>,
    pub expanded: BTreeSet<OccurrenceHandle>,
    pub layer_move: bool,
    pub changed: bool,
}
impl ObjectEditing {
    pub fn dragging(&self) -> bool { self.gesture.as_ref().is_some_and(|gesture| gesture.nudge.is_none()) }
    pub fn placing(&self) -> bool { self.placement.is_some() }
    pub fn snapping(&self) -> Option<&(Snapping, [f64; 2])> { self.gesture.as_ref().and_then(|gesture| gesture.snapping.as_ref()) }
}

fn translation(delta: [f64; 2]) -> Affine64 { Affine64([1., 0., 0., 1., delta[0], delta[1]]) }
fn about(pivot: [f64; 2], linear: [f64; 4]) -> Affine64 {
    translation(pivot).compose(Affine64([linear[0], linear[1], linear[2], linear[3], 0., 0.])).compose(translation([-pivot[0], -pivot[1]]))
}
fn rect(bounds: [[f64; 2]; 2], origin: [f64; 2]) -> Rect {
    let point = |p: [f64; 2]| Point { x: (p[0] - origin[0]) as f32, y: (p[1] - origin[1]) as f32 };
    Rect { min: point(bounds[0]), max: point(bounds[1]) }
}
pub(super) fn original_size(affine: Affine64, extent: [u32; 2]) -> Affine64 {
    let [a, b, c, d, _, _] = affine.0;
    let sign = if a * d - b * c < 0. { -1. } else { 1. };
    let column = [a + sign * d, b - sign * c];
    let length = column[0].hypot(column[1]);
    let [cos, sin] = if length > 0. { column.map(|v| v / length) } else { [1., 0.] };
    let half = extent.map(|v| f64::from(v) * 0.5);
    let centre = affine.map(half);
    let linear = [cos, sin, -sign * sin, sign * cos];
    Affine64([linear[0], linear[1], linear[2], linear[3],
        centre[0] - (linear[0] * half[0] + linear[2] * half[1]), centre[1] - (linear[1] * half[0] + linear[3] * half[1])])
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn object_target(&self) -> Option<OccurrenceHandle> {
        self.object_target_for(self.layer_interaction.tool)
    }
    pub(super) fn object_target_for(&self, tool: LayerCanvasTool) -> Option<OccurrenceHandle> {
        let doc = self.engine.document();
        let layer = doc.working.occurrence?;
        (doc.scene().object_layer(layer).is_some()
            && self.selection_masks.target().is_none()
            && !matches!(doc.working.target, Some(SourceTarget::Coverage(_)))
            && !self.objects.layer_move
            && tool.selection_tool().is_none()
            && !matches!(tool, LayerCanvasTool::Crop | LayerCanvasTool::Ruler { .. })).then_some(layer)
    }
    pub(super) fn selected_objects(&self) -> &BTreeSet<ImageObjectHandle> { &self.engine.document().working.objects }
    fn object_selection_edit(&self, layer: OccurrenceHandle, objects: BTreeSet<ImageObjectHandle>) -> Edit {
        let mut working = self.engine.document().working.clone();
        if working.occurrence != Some(layer) {
            working.occurrence = Some(layer);
            working.layer_selection = BTreeSet::from([layer]);
            working.layer_anchor = Some(layer);
        }
        working.target = None;
        working.inspect_mask = None;
        working.objects = objects;
        Edit::Working(working)
    }
    pub(super) fn select_objects(&mut self, layer: OccurrenceHandle, objects: BTreeSet<ImageObjectHandle>) -> Result<(), String> {
        let doc = self.engine.document();
        if doc.working.occurrence == Some(layer) && doc.working.objects == objects && doc.working.target.is_none() && doc.working.inspect_mask.is_none() { return Ok(()); }
        self.layer_edit(self.object_selection_edit(layer, objects))?;
        self.objects.changed = true;
        Ok(())
    }
    fn session_frame(&self) -> Option<Frame> {
        if let Some(gesture) = &self.objects.gesture { return Some(gesture.frame.moved(gesture.delta)); }
        let doc = self.engine.document();
        let objects = &doc.working.objects;
        if let Some((selected, revision, frame)) = &self.objects.frame && selected == objects && *revision == doc.revision { return Some(*frame); }
        let unit = if let [object] = objects.iter().copied().collect::<Vec<_>>()[..] {
            let [w, h] = doc.scene().object(object)?.image.extent.map(f64::from);
            doc.object_document_affine(object)?.compose(Affine64([w, 0., 0., h, 0., 0.]))
        } else {
            let [min, max] = doc.object_document_bounds(objects.iter().copied())?;
            Affine64([max[0] - min[0], 0., 0., max[1] - min[1], min[0], min[1]])
        };
        unit.inverse()?;
        Some(Frame { unit, pivot: unit.map([0.5, 0.5]) })
    }
    fn object_hit(&self, point: [f64; 2]) -> Option<ObjectHandle> {
        let frame = self.session_frame()?;
        let reach = f64::from(self.ruler_reach());
        frame.handles(reach).into_iter().map(|(handle, at)| ((at[0] - point[0]).hypot(at[1] - point[1]), handle))
            .filter(|(distance, _)| *distance <= reach).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, handle)| handle)
            .or_else(|| self.selected_objects().iter().any(|h| self.engine.document().object_contains(*h, point)).then_some(ObjectHandle::Move))
    }
    pub(super) fn pointer64(&self, surface: [f32; 2]) -> [f64; 2] {
        self.state.camera.surface_to_document64(surface.map(f64::from))
    }
    pub(super) fn object_touch_target(&self, position: [f32; 2]) -> bool {
        self.object_target().is_some() && matches!(self.layer_interaction.tool, LayerCanvasTool::Move | LayerCanvasTool::Transform) && {
            let point = self.pointer64(position);
            self.object_hit(point).is_some() || self.engine.document().pick_image_object(point).is_some()
        }
    }
    pub(super) fn object_pen(&mut self, event: PenEvent) -> Result<bool, String> {
        if self.objects.gesture.as_ref().is_some_and(|gesture| gesture.nudge.is_some()) { self.finish_object_gesture(true)?; }
        if self.objects.gesture.is_none() {
            if event.phase == PenPhase::Cancel && self.objects.placing() { return Ok(true); }
            if event.phase != PenPhase::Down || self.object_target().is_none()
                || !matches!(self.layer_interaction.tool, LayerCanvasTool::Move | LayerCanvasTool::Transform) { return Ok(false); }
        }
        let point = self.pointer64([event.surface_position.x, event.surface_position.y]);
        if event.phase != PenPhase::Cancel && !point.iter().all(|v| v.is_finite()) { return Err("Invalid canvas point".into()); }
        match event.phase {
            PenPhase::Down => self.begin_object_gesture(point)?,
            PenPhase::Move => self.update_object_gesture(point)?,
            PenPhase::Up => { self.update_object_gesture(point)?; self.finish_object_gesture(true)?; }
            PenPhase::Cancel => { self.finish_object_gesture(false)?; }
            PenPhase::Hover => (),
        }
        self.layer_interaction.changed = true;
        Ok(true)
    }
    fn begin_object_gesture(&mut self, point: [f64; 2]) -> Result<(), String> {
        self.require_document_idle_except_placement()?;
        let modifiers = self.interaction.modifiers;
        let doc = self.engine.document();
        let active = doc.working.occurrence.ok_or("Select an object layer")?;
        let selected = doc.working.objects.clone();
        let hit = self.object_hit(point);
        let (handle, layer, objects) = match hit {
            Some(handle) if handle != ObjectHandle::Move || !modifiers.shift => (handle, active, selected.clone()),
            _ if modifiers.shift => match doc.pick_image_object_in(active, point) {
                Some(object) => {
                    let mut objects = selected.clone();
                    if !objects.remove(&object) { objects.insert(object); }
                    (ObjectHandle::Pivot, active, objects)
                }
                None => {
                    if doc.pick_image_object(point).is_some() { self.raise_message_notice(MessageId::OBJECTS_SHIFT_SAME_LAYER); }
                    (ObjectHandle::Pivot, active, selected.clone())
                }
            },
            _ => match doc.pick_image_object(point) {
                Some((layer, object)) if layer == active && selected.contains(&object) => (ObjectHandle::Move, layer, selected.clone()),
                Some((layer, object)) => (ObjectHandle::Move, layer, BTreeSet::from([object])),
                None => (ObjectHandle::Pivot, active, BTreeSet::new()),
            },
        };
        let setup = if layer != active || objects != selected {
            let edit = self.object_selection_edit(layer, objects.clone());
            let inverse = self.engine.document().clone().apply(edit.clone()).map_err(error)?;
            self.engine.preview_edit(edit.clone()).map_err(error)?;
            Some((edit, inverse))
        } else { None };
        let frame = self.session_frame();
        if objects.is_empty() || frame.is_none() || handle == ObjectHandle::Pivot {
            let pivot = hit == Some(ObjectHandle::Pivot) && setup.is_none() && frame.is_some();
            self.objects.gesture = Some(Gesture { handle: ObjectHandle::Pivot, nudge: None, press: point, setup, copy: false,
                start: Vec::new(), frame: frame.unwrap_or(Frame { unit: Affine64::default(), pivot: point }), delta: Affine64::default(), snapping: None, moved: false, pivot });
            return Ok(());
        }
        let doc = self.engine.document();
        let start = objects.iter().map(|h| Ok((*h, doc.scene().object(*h).ok_or("Unknown image")?.affine))).collect::<Result<Vec<_>, String>>()?;
        let snapping = (handle == ObjectHandle::Move).then(|| self.object_snapping(point.map(f64::round))).flatten();
        let copy = handle == ObjectHandle::Move && self.operation.leave_copy != modifiers.alt && !self.objects.placing();
        self.objects.gesture = Some(Gesture { handle, nudge: None, press: point, setup, copy, start,
            frame: frame.unwrap(), delta: Affine64::default(), snapping, moved: false, pivot: false });
        Ok(())
    }
    fn gesture_delta(&mut self, point: [f64; 2]) -> Option<Affine64> {
        let modifiers = self.interaction.modifiers;
        let aspect = self.operation.aspect;
        let units = self.ruler_reach() / 12.;
        let gesture = self.objects.gesture.as_mut()?;
        let frame = gesture.frame;
        Some(match gesture.handle {
            ObjectHandle::Move => {
                let mut delta = [point[0] - gesture.press[0], point[1] - gesture.press[1]];
                if modifiers.shift { if delta[0].abs() >= delta[1].abs() { delta[1] = 0. } else { delta[0] = 0. } }
                if let Some((snapping, origin)) = &mut gesture.snapping {
                    let bounds = frame.moved(translation(delta)).unit.bounds([1, 1]);
                    let direction = modifiers.shift.then(|| if delta[0] != 0. { Point { x: 1., y: 0. } } else { Point { x: 0., y: 1. } });
                    let press = Point { x: (gesture.press[0] - origin[0]) as f32, y: (gesture.press[1] - origin[1]) as f32 };
                    let correction = snapping.correction(rect(bounds, *origin), press, direction, units);
                    delta = [delta[0] + f64::from(correction.x), delta[1] + f64::from(correction.y)];
                    snapping.retain_guides(rect(frame.moved(translation(delta)).unit.bounds([1, 1]), *origin), units);
                }
                translation(delta)
            }
            ObjectHandle::Rotate => {
                let pivot = frame.pivot;
                let mut angle = (point[1] - pivot[1]).atan2(point[0] - pivot[0]) - (gesture.press[1] - pivot[1]).atan2(gesture.press[0] - pivot[0]);
                if modifiers.shift { let step = std::f64::consts::PI / 12.; angle = (angle / step).round() * step; }
                let (sin, cos) = angle.sin_cos();
                about(pivot, [cos, sin, -sin, cos])
            }
            ObjectHandle::Scale(handle) => {
                let inverse = frame.unit.inverse()?;
                let anchor = if modifiers.alt { inverse.map(frame.pivot) } else { handle.map(|v| 1. - v) };
                let q = inverse.map(point);
                let mut scale = [0, 1].map(|axis| {
                    let span = handle[axis] - anchor[axis];
                    if handle[axis] == 0.5 || span == 0. { 1. } else { (q[axis] - anchor[axis]) / span }
                });
                if (aspect || modifiers.shift) && handle[0] != 0.5 && handle[1] != 0.5 {
                    let factor = if (scale[0] - 1.).abs() >= (scale[1] - 1.).abs() { scale[0] } else { scale[1] };
                    scale = [factor; 2];
                }
                if scale.iter().any(|v| !v.is_finite()) { return None; }
                let [a, b, c, d, _, _] = frame.unit.0;
                let sides = [a.hypot(b), c.hypot(d)];
                let scale = [0, 1].map(|axis| {
                    let least = (1. / sides[axis]).min(1.);
                    if scale[axis].abs() >= least { scale[axis] } else if scale[axis] < 0. { -least } else { least }
                });
                frame.unit.compose(about(anchor, [scale[0], 0., 0., scale[1]])).compose(inverse)
            }
            ObjectHandle::Pivot => return None,
        })
    }
    fn move_objects(&mut self, delta: Affine64) -> Result<(), String> {
        let gesture = self.objects.gesture.as_ref().ok_or("Move an image first")?;
        if !gesture.moved {
            if gesture.copy {
                let selected: BTreeSet<_> = gesture.start.iter().map(|(h, _)| *h).collect();
                let (copies, duplicate) = self.engine.document().duplicate_image_objects_edit(&selected).map_err(error)?;
                let inverse = self.engine.document().clone().apply(duplicate.clone()).map_err(error)?;
                self.engine.preview_edit(duplicate.clone()).map_err(error)?;
                let gesture = self.objects.gesture.as_mut().unwrap();
                gesture.setup = Some(match gesture.setup.take() {
                    Some((forward, backward)) => (Edit::Batch(vec![forward, duplicate]), Edit::Batch(vec![inverse, backward])),
                    None => (duplicate, inverse),
                });
                gesture.start = copies.into_iter().zip(gesture.start.iter().map(|(_, a)| *a)).collect();
            }
            let objects: Vec<_> = self.objects.gesture.as_ref().unwrap().start.iter().map(|(h, _)| *h).collect();
            self.start_object_motion(&objects)?;
            self.objects.gesture.as_mut().unwrap().moved = true;
        }
        if self.preview_object_motion(delta).is_ok() { self.objects.gesture.as_mut().unwrap().delta = delta; }
        Ok(())
    }
    fn update_object_gesture(&mut self, point: [f64; 2]) -> Result<(), String> {
        let Some(gesture) = &self.objects.gesture else { return Ok(()); };
        if gesture.handle == ObjectHandle::Pivot {
            if gesture.pivot { self.objects.gesture.as_mut().unwrap().frame.pivot = point; }
            return Ok(());
        }
        let Some(delta) = self.gesture_delta(point) else { return Ok(()); };
        if !self.objects.gesture.as_ref().unwrap().moved && delta == Affine64::default() { return Ok(()); }
        self.move_objects(delta)
    }
    pub(super) fn finish_object_gesture(&mut self, apply: bool) -> Result<bool, String> {
        let Some(gesture) = self.objects.gesture.take() else { return Ok(false); };
        self.objects.changed = true;
        let changed = gesture.moved && gesture.delta != Affine64::default();
        let commit = if apply && changed { Some(self.object_motion_edit(gesture.delta)?) } else { None };
        if gesture.moved { self.cancel_object_motion()?; }
        if self.objects.placing() {
            if let Some(commit) = commit {
                self.engine.preview_edit(commit).map_err(error)?;
            }
            let layer = self.engine.document().working.occurrence;
            self.engine.backend_mut().prepare_moving_layer(layer);
            return Ok(true);
        }
        if let Some((_, inverse)) = &gesture.setup { self.engine.preview_edit(inverse.clone()).map_err(error)?; }
        if !apply { return Ok(true); }
        let edits: Vec<_> = gesture.setup.map(|(forward, _)| forward).into_iter().chain(commit).collect();
        if edits.is_empty() {
            if gesture.pivot {
                let objects = self.selected_objects().clone();
                self.objects.frame = Some((objects, self.engine.document().revision, gesture.frame));
            }
            return Ok(true);
        }
        let frame = gesture.frame.moved(gesture.delta);
        self.layer_edit(Edit::Batch(edits))?;
        if changed {
            self.remember_transform(gesture.delta);
            let doc = self.engine.document();
            self.objects.frame = Some((doc.working.objects.clone(), doc.revision, frame));
        }
        self.refresh_document();
        self.refresh_commands();
        Ok(true)
    }
    pub(super) fn object_snap_targets(&self) -> Vec<(Option<OccurrenceHandle>, [[f64; 2]; 2])> {
        let canvas = self.engine.document().composition().size.map(f64::from);
        let wide = |r: Rect| [[f64::from(r.min.x), f64::from(r.min.y)], [f64::from(r.max.x), f64::from(r.max.y)]];
        std::iter::once((None, [[0.; 2], canvas])).chain(self.measured_snap_bounds().into_iter()
            .map(|(h, bounds)| (Some(h), self.object_layer_bounds64(h).unwrap_or_else(|| wide(bounds))))).collect()
    }
    fn object_snapping(&self, origin: [f64; 2]) -> Option<(Snapping, [f64; 2])> {
        if !self.operation.snapping { return None; }
        let targets = self.object_snap_targets().into_iter().map(|(h, b)| (h, rect(b, origin))).collect();
        let shift = Point { x: -origin[0] as f32, y: -origin[1] as f32 };
        let rulers = if self.rulers.visible { self.engine.document().rulers().map(|ruler| layer_core::Ruler { geometry: ruler.geometry.translated(shift), ..ruler }).collect() } else { Vec::new() };
        Some((Snapping::new(targets, rulers), origin))
    }
    pub(super) fn object_layer_bounds64(&self, layer: OccurrenceHandle) -> Option<[[f64; 2]; 2]> {
        let doc = self.engine.document();
        let children = doc.object_layer_children(layer)?;
        doc.object_document_bounds(children.iter().copied().filter(|h| doc.scene().object(*h).is_some_and(|o| o.visible)))
    }
    pub(super) fn object_layer_bounds(&self, layer: OccurrenceHandle) -> Option<Rect> {
        self.object_layer_bounds64(layer).map(|bounds| rect(bounds, [0.; 2]))
    }
    pub(super) fn object_nudge(&mut self, key: &str, pressed: bool, allowed: bool, modifiers: Modifiers) -> Result<bool, String> {
        let held = self.objects.gesture.as_ref().and_then(|g| g.nudge.clone());
        if !pressed { return if held.as_deref() == Some(key) { self.finish_object_gesture(true) } else { Ok(false) }; }
        if allowed && key == "escape" && held.is_some() { return self.finish_object_gesture(false); }
        let step = if modifiers.shift { 10. } else { 1. };
        let offset = match key { "arrowleft" => [-step, 0.], "arrowright" => [step, 0.], "arrowup" => [0., -step], "arrowdown" => [0., step], _ => return Ok(false) };
        if !allowed || modifiers.command || modifiers.alt || self.objects.dragging() || self.object_target().is_none() || self.selected_objects().is_empty() { return Ok(false); }
        if held.as_deref().is_some_and(|held| held != key) { self.finish_object_gesture(true)?; }
        if self.objects.gesture.is_none() {
            self.require_document_idle_except_placement()?;
            let doc = self.engine.document();
            let start = doc.working.objects.iter().map(|h| Ok((*h, doc.scene().object(*h).ok_or("Unknown image")?.affine))).collect::<Result<Vec<_>, String>>()?;
            let frame = self.session_frame().ok_or("Select images first")?;
            self.objects.gesture = Some(Gesture { handle: ObjectHandle::Move, nudge: Some(key.into()), press: [0.; 2], setup: None, copy: false, start,
                frame, delta: Affine64::default(), snapping: None, moved: false, pivot: false });
        }
        let delta = translation(offset).compose(self.objects.gesture.as_ref().unwrap().delta);
        self.move_objects(delta)?;
        Ok(true)
    }
    pub(super) fn cancel_object_interaction(&mut self) -> Result<bool, String> {
        if self.finish_object_gesture(false)? { return Ok(true); }
        if self.objects.placing() { self.finish_object_placement(false)?; return Ok(true); }
        Ok(false)
    }
    pub(super) fn clear_object_selection(&mut self) -> Result<bool, String> {
        let Some(layer) = self.object_target().filter(|_| !self.selected_objects().is_empty()) else { return Ok(false); };
        self.select_objects(layer, BTreeSet::new())?;
        Ok(true)
    }
    fn transform_selected(&mut self, delta: Affine64) -> Result<(), String> {
        self.object_target().ok_or("Select images first")?;
        let objects: Vec<_> = self.selected_objects().iter().copied().collect();
        let frame = self.session_frame().map(|frame| frame.moved(delta));
        self.begin_object_motion(&objects)?;
        let edit = self.preview_object_motion(delta).and_then(|_| self.object_motion_edit(delta));
        self.cancel_object_motion()?;
        let edit = edit?;
        if self.objects.placing() { self.engine.preview_edit(edit).map_err(error)?; } else { self.layer_edit(edit)?; self.remember_transform(delta); }
        let doc = self.engine.document();
        self.objects.frame = frame.map(|frame| (doc.working.objects.clone(), doc.revision, frame));
        self.objects.changed = true;
        Ok(())
    }
    fn object_original_size(&mut self) -> Result<(), String> {
        let doc = self.engine.document();
        self.object_target().ok_or("Select images first")?;
        let affines: Vec<_> = doc.working.objects.iter().map(|h| {
            let object = doc.scene().object(*h).ok_or("Unknown image")?;
            let affine = original_size(object.affine, object.image.extent);
            self.engine.backend().preflight_image_object_affine(doc.scene(), *h, affine, self.engine.view()).map_err(error)?;
            Ok((*h, affine))
        }).collect::<Result<_, String>>()?;
        let edit = doc.set_image_object_affines_edit(&affines).map_err(error)?;
        if self.objects.placing() { self.engine.preview_edit(edit).map_err(error)?; } else { self.layer_edit(edit)?; }
        self.objects.frame = None;
        Ok(())
    }
    pub(super) fn object_command_enabled(&self, id: CommandId) -> Option<bool> {
        let layer = self.object_target()?;
        let doc = self.engine.document();
        let idle = self.require_document_idle_except_placement().is_ok();
        let selected = !doc.working.objects.is_empty();
        let editable = doc.objects_editable(layer);
        Some(match id {
            CommandId::SelectAll => idle && doc.object_layer_children(layer).is_some_and(|c| !c.is_empty()),
            CommandId::Deselect => idle && selected,
            CommandId::ClearSelected | CommandId::CopySelectionToLayer => idle && selected && editable && !self.objects.placing(),
            CommandId::CutSelectionToLayer | CommandId::InvertSelection | CommandId::TransformBicubic | CommandId::TransformLanczos
            | CommandId::TransformDistort | CommandId::TransformWarp | CommandId::TransformPerspective => false,
            CommandId::TransformAgain => idle && selected && editable && self.last_transform().is_some(),
            CommandId::TransformFlipHorizontal | CommandId::TransformFlipVertical | CommandId::TransformRotateLeft | CommandId::TransformRotateRight
            | CommandId::PlacementOriginalSize | CommandId::TransformNearest | CommandId::TransformBilinear => idle && selected && editable,
            CommandId::ScaleRotate => idle && selected && editable,
            CommandId::ResetTransform => self.objects.placing() || !self.session_changes().is_empty(),
            CommandId::ApplyTransform | CommandId::CancelTransform if self.objects.placing() => !self.objects.dragging(),
            _ => return None,
        })
    }
    pub(super) fn object_command_selected(&self, id: CommandId) -> Option<bool> {
        self.object_target()?;
        let doc = self.engine.document();
        let nearest = |nearest: bool| !doc.working.objects.is_empty() && doc.working.objects.iter()
            .all(|h| doc.scene().object(*h).is_some_and(|o| (o.interpolation == ImageInterpolation::Nearest) == nearest));
        match id {
            CommandId::TransformNearest => Some(nearest(true)),
            CommandId::TransformBilinear => Some(nearest(false)),
            _ => None,
        }
    }
    pub(super) fn object_command_label(&self, id: CommandId) -> Option<std::sync::Arc<str>> {
        self.object_target()?;
        let l = self.localization();
        Some(match id {
            CommandId::ClearSelected => l.text(MessageId::OBJECTS_DELETE_IMAGES),
            CommandId::CopySelectionToLayer => l.text(MessageId::OBJECTS_DUPLICATE_IMAGES),
            CommandId::SelectAll => l.text(MessageId::OBJECTS_SELECT_ALL_IMAGES),
            CommandId::Deselect => l.text(MessageId::OBJECTS_DESELECT_IMAGES),
            CommandId::TransformBilinear => l.text(MessageId::OBJECTS_INTERPOLATION_LINEAR),
            CommandId::PlacementOriginalSize => l.text(MessageId::OBJECTS_ORIGINAL_SIZE),
            _ => return None,
        })
    }
    pub(super) fn object_disabled_reason(&self, id: CommandId) -> Option<std::sync::Arc<str>> {
        let layer = self.object_target()?;
        self.object_command_enabled(id)?;
        let l = self.localization();
        let doc = self.engine.document();
        Some(match id {
            CommandId::CutSelectionToLayer => l.text(MessageId::OBJECTS_CUT_TO_LAYER_UNAVAILABLE),
            CommandId::TransformBicubic | CommandId::TransformLanczos => l.text(MessageId::OBJECTS_INTERPOLATION_UNAVAILABLE),
            CommandId::TransformDistort | CommandId::TransformWarp | CommandId::TransformPerspective => l.text(MessageId::OBJECTS_DISTORTION_UNAVAILABLE),
            CommandId::InvertSelection => l.text(MessageId::OBJECTS_PAINT_ONLY),
            CommandId::TransformAgain if self.last_transform().is_none() => l.text(MessageId::COMMANDS_TRANSFORM_AGAIN_EMPTY),
            _ if !doc.objects_editable(layer) && doc.scene().object_layer(layer).is_some() => l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED),
            _ if doc.working.objects.is_empty() => l.text(MessageId::OBJECTS_SELECT_IMAGES_FIRST),
            _ => return None,
        })
    }
    pub(super) fn object_command(&mut self, id: CommandId) -> Option<Result<(), String>> {
        let layer = self.object_target()?;
        if matches!(id, CommandId::ClearSelected | CommandId::Deselect) && self.selected_objects().is_empty() { return Some(Ok(())); }
        if !self.object_command_enabled(id).unwrap_or(false) {
            return matches!(id, CommandId::SelectAll | CommandId::Deselect | CommandId::ClearSelected | CommandId::CopySelectionToLayer | CommandId::CutSelectionToLayer
                | CommandId::TransformAgain | CommandId::TransformFlipHorizontal | CommandId::TransformFlipVertical
                | CommandId::TransformRotateLeft | CommandId::TransformRotateRight | CommandId::PlacementOriginalSize | CommandId::TransformNearest
                | CommandId::TransformBilinear | CommandId::TransformBicubic | CommandId::TransformLanczos | CommandId::InvertSelection).then(|| Err(self.object_disabled_reason(id).map_or_else(|| self.localization().text(MessageId::OBJECTS_SELECT_IMAGES_FIRST).to_string(), |r| r.to_string())));
        }
        let selected = self.selected_objects().clone();
        let pivot = self.session_frame().map_or([0.; 2], |frame| frame.pivot);
        Some(match id {
            CommandId::SelectAll => self.select_objects(layer, self.engine.document().object_layer_children(layer).unwrap_or_default().iter().copied().collect()),
            CommandId::Deselect => self.select_objects(layer, BTreeSet::new()),
            CommandId::ClearSelected => self.engine.document().delete_image_objects_edit(&selected).map_err(error).and_then(|edit| self.layer_edit(edit)),
            CommandId::CopySelectionToLayer => self.engine.document().duplicate_image_objects_edit(&selected).map_err(error).and_then(|(_, edit)| self.layer_edit(edit)),
            CommandId::TransformAgain => self.last_transform().ok_or_else(|| self.localization().text(MessageId::COMMANDS_TRANSFORM_AGAIN_EMPTY).to_string())
                .and_then(|delta| self.transform_selected(delta)),
            CommandId::ResetTransform => self.reset_object_session(),
            CommandId::TransformFlipHorizontal => self.transform_selected(about(pivot, [-1., 0., 0., 1.])),
            CommandId::TransformFlipVertical => self.transform_selected(about(pivot, [1., 0., 0., -1.])),
            CommandId::TransformRotateLeft => self.transform_selected(about(pivot, [0., -1., 1., 0.])),
            CommandId::TransformRotateRight => self.transform_selected(about(pivot, [0., 1., -1., 0.])),
            CommandId::PlacementOriginalSize => self.object_original_size(),
            CommandId::TransformNearest | CommandId::TransformBilinear => {
                let interpolation = if id == CommandId::TransformNearest { ImageInterpolation::Nearest } else { ImageInterpolation::Linear };
                self.engine.document().set_image_objects_interpolation_edit(&selected, interpolation).map_err(error).and_then(|edit| self.layer_edit(edit))
            }
            CommandId::ScaleRotate => self.object_tool(LayerCanvasTool::Transform),
            _ => return None,
        }.map(|()| { self.objects.changed = true; self.refresh_document(); }))
    }
    pub(super) fn object_tool(&mut self, tool: LayerCanvasTool) -> Result<(), String> {
        self.layer_interaction.tool = tool;
        self.state.layer_tools.tool = tool;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn object_action(&mut self, action: ObjectAction) -> Result<(), String> {
        let doc = self.engine.document();
        match action {
            ObjectAction::Select { id, extend } => {
                let object = object_handle(id)?;
                let layer = doc.scene().object_owner(object).ok_or("Unknown image")?;
                let mut objects = if extend && doc.working.occurrence == Some(layer) { doc.working.objects.clone() } else { BTreeSet::new() };
                if extend && !objects.insert(object) { objects.remove(&object); } else { objects.insert(object); }
                self.objects.layer_move = false;
                if !matches!(self.layer_interaction.tool, LayerCanvasTool::Move | LayerCanvasTool::Transform) { self.object_tool(LayerCanvasTool::Move)?; }
                self.return_to_artwork()?;
                self.select_objects(layer, objects)
            }
            ObjectAction::Expand { layer, expanded } => {
                let layer = occurrence_handle(layer)?;
                if doc.scene().object_layer(layer).is_none() { return Err("Choose an object layer".into()); }
                if expanded { self.objects.expanded.insert(layer); } else { self.objects.expanded.remove(&layer); }
                Ok(())
            }
            ObjectAction::Visibility { id, visible } => { let edit = doc.set_image_object_visible_edit(object_handle(id)?, visible).map_err(error)?; self.layer_edit(edit) }
            ObjectAction::Rename { id, name } => { let edit = doc.rename_image_object_edit(object_handle(id)?, &name).map_err(error)?; self.layer_edit(edit) }
            ObjectAction::Order { order } => { let edit = doc.reorder_image_objects_edit(&doc.working.objects, order).map_err(error)?; self.layer_edit(edit) }
            ObjectAction::Drop { id, target, below } => {
                let (object, target) = (object_handle(id)?, object_handle(target)?);
                let layer = doc.scene().object_owner(object).ok_or("Unknown image")?;
                if doc.scene().object_owner(target) != Some(layer) { return Err(self.localization().text(MessageId::OBJECTS_REORDER_WITHIN_LAYER).to_string()); }
                let children = doc.object_layer_children(layer).unwrap_or_default();
                let from = children.iter().position(|h| *h == object).ok_or("Unknown image")?;
                let to = children.iter().position(|h| *h == target).ok_or("Unknown image")? + usize::from(below);
                let edit = doc.move_image_object_edit(object, if to > from { to - 1 } else { to }).map_err(error)?;
                self.layer_edit(edit)
            }
            ObjectAction::Interpolation { nearest } => self.object_command(if nearest { CommandId::TransformNearest } else { CommandId::TransformBilinear }).unwrap_or_else(|| Err(self.localization().text(MessageId::OBJECTS_SELECT_IMAGES_FIRST).to_string())),
            ObjectAction::SelectAll => self.object_command(CommandId::SelectAll).unwrap_or(Ok(())),
            ObjectAction::Deselect => self.object_command(CommandId::Deselect).unwrap_or(Ok(())),
            ObjectAction::Delete => self.object_command(CommandId::ClearSelected).unwrap_or_else(|| Err(self.localization().text(MessageId::OBJECTS_SELECT_IMAGES_FIRST).to_string())),
            ObjectAction::Duplicate => self.object_command(CommandId::CopySelectionToLayer).unwrap_or_else(|| Err(self.localization().text(MessageId::OBJECTS_SELECT_IMAGES_FIRST).to_string())),
        }?;
        self.objects.changed = true;
        Ok(())
    }
    pub(super) fn object_rows(&self, layer: OccurrenceHandle) -> Vec<ObjectRow> {
        let doc = self.engine.document();
        let Some(children) = doc.object_layer_children(layer).filter(|_| self.objects.expanded.contains(&layer)) else { return Vec::new(); };
        let editable = doc.objects_editable(layer);
        let selected = (doc.working.occurrence == Some(layer)).then_some(&doc.working.objects);
        let unnamed = self.localization().text(MessageId::OBJECTS_UNNAMED_IMAGE);
        let rendition = self.effective_sdr_rendition().parameters().into_iter().fold(0u64, |h, v| h.wrapping_mul(1099511628211).wrapping_add(u64::from(v.to_bits())));
        children.iter().enumerate().filter_map(|(index, &h)| {
            let object = doc.scene().object(h)?;
            let image = object.image.id().bytes().into_iter().fold(rendition, |h, v| h.wrapping_mul(1099511628211).wrapping_add(u64::from(v)));
            Some(ObjectRow { id: object_token(h), layer: occurrence_token(layer),
                label: if object.name.is_empty() { unnamed.to_string() } else { object.name.to_string() },
                visible: object.visible, selected: selected.is_some_and(|s| s.contains(&h)), editable, index: index as u32,
                can_raise: editable && index > 0, can_lower: editable && index + 1 < children.len(),
                nearest: object.interpolation == ImageInterpolation::Nearest,
                thumbnail_revision: image & ((1u64 << 53) - 1) })
        }).collect()
    }
    pub fn object_menu(&self, id: u64) -> Result<ContextMenu, String> {
        let object = object_handle(id)?;
        let doc = self.engine.document();
        let layer = doc.scene().object_owner(object).ok_or("Unknown image")?;
        let value = doc.scene().object(object).ok_or("Unknown image")?;
        let l = self.localization();
        let editable = doc.objects_editable(layer);
        let selected = doc.working.occurrence == Some(layer) && doc.working.objects.contains(&object);
        let item = |label: std::sync::Arc<str>, action: ObjectAction, enabled: bool| ContextMenuItem { enabled, ..ContextMenuItem::command(label.as_ref(), UiAction::Object { action }) };
        let children = doc.object_layer_children(layer).unwrap_or_default();
        let index = children.iter().position(|h| *h == object).unwrap_or_default();
        Ok(ContextMenu { title: if value.name.is_empty() { l.text(MessageId::OBJECTS_UNNAMED_IMAGE).to_string() } else { value.name.to_string() }, sections: vec![
            vec![item(l.text(if selected { MessageId::OBJECTS_REMOVE_FROM_SELECTION } else { MessageId::OBJECTS_ADD_TO_SELECTION }), ObjectAction::Select { id, extend: true }, true),
                item(l.text(if value.visible { MessageId::OBJECTS_HIDE_IMAGE } else { MessageId::OBJECTS_SHOW_IMAGE }), ObjectAction::Visibility { id, visible: !value.visible }, editable)],
            [(MessageId::OBJECTS_BRING_TO_FRONT, ObjectOrder::Front, index > 0), (MessageId::OBJECTS_BRING_FORWARD, ObjectOrder::Forward, index > 0),
                (MessageId::OBJECTS_SEND_BACKWARD, ObjectOrder::Backward, index + 1 < children.len()), (MessageId::OBJECTS_SEND_TO_BACK, ObjectOrder::Back, index + 1 < children.len())]
                .into_iter().map(|(label, order, enabled)| item(l.text(label), ObjectAction::Order { order }, editable && selected && enabled)).collect(),
            vec![item(l.text(MessageId::OBJECTS_DUPLICATE_IMAGES), ObjectAction::Duplicate, editable && selected),
                item(l.text(MessageId::OBJECTS_DELETE_IMAGES), ObjectAction::Delete, editable && selected)],
        ] }.with_shortcuts_localized(&self.state.settings, self.state.platform, l))
    }
    pub(super) fn append_object_overlay(&self, segments: &mut Vec<CursorSegment>) {
        if self.object_target().is_none() || !matches!(self.layer_interaction.tool, LayerCanvasTool::Move | LayerCanvasTool::Transform) { return; }
        let map = self.document_to_logical();
        let point = |p: [f64; 2]| map(Point { x: p[0] as f32, y: p[1] as f32 });
        if let Some((snapping, origin)) = self.objects.snapping() {
            let shift = |p: Point| point([f64::from(p.x) + origin[0], f64::from(p.y) + origin[1]]);
            segments.extend(snapping.guides.iter().map(|[from, to]| CursorSegment { from: shift(*from), to: shift(*to), distance: 0., marker: 0., scale: 1. }));
        }
        let doc = self.engine.document();
        let mut line = |a: [f32; 2], b: [f32; 2], solid: bool| segments.push(CursorSegment { from: a, to: b, distance: 0., marker: f32::from(solid), scale: 1. });
        for &h in &doc.working.objects {
            let (Some(affine), Some(object)) = (doc.object_document_affine(h), doc.scene().object(h)) else { continue; };
            let [w, hgt] = object.image.extent.map(f64::from);
            let corners = [[0., 0.], [w, 0.], [w, hgt], [0., hgt]].map(|p| point(affine.map(p)));
            for i in 0..4 { line(corners[i], corners[(i + 1) % 4], doc.working.objects.len() == 1); }
        }
        let Some(frame) = self.session_frame().filter(|_| !doc.working.objects.is_empty()) else { return; };
        if doc.working.objects.len() > 1 {
            let corners = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| point(frame.unit.map(p)));
            for i in 0..4 { line(corners[i], corners[(i + 1) % 4], true); }
        }
        let handles = frame.handles(f64::from(self.ruler_reach()));
        if let Some((_, rotate)) = handles.iter().find(|(handle, _)| *handle == ObjectHandle::Rotate) { line(point(frame.unit.map([0.5, 0.])), point(*rotate), true); }
        for (handle, at) in handles {
            let [x, y] = point(at);
            let half = operation::HANDLE_HALF_SIZE;
            segments.push(CursorSegment { from: [x - half, y - half], to: [x + half, y + half], distance: 0., marker: if handle == ObjectHandle::Pivot { 5. } else { 2. }, scale: 1. });
        }
    }
    pub(super) fn require_document_idle_except_placement(&self) -> Result<(), String> {
        if self.objects.placing() { self.require_idle() } else { self.require_document_idle() }
    }
    pub(super) fn begin_object_placement(&mut self, insert: Edit, objects: Vec<ImageObjectHandle>) -> Result<(), String> {
        let inverse = self.engine.document().clone().apply(insert.clone()).map_err(error)?;
        self.engine.preview_edit(insert.clone()).map_err(error)?;
        let layer = self.engine.document().scene().object_owner(objects[0]);
        self.objects.placement = Some(Placement { insert, inverse, objects });
        self.objects.layer_move = false;
        self.engine.backend_mut().prepare_moving_layer(layer);
        self.object_tool(LayerCanvasTool::Transform)?;
        self.objects.changed = true;
        self.refresh_document();
        self.refresh_commands();
        Ok(())
    }
    pub(super) fn finish_object_placement(&mut self, apply: bool) -> Result<(), String> {
        self.finish_object_gesture(false)?;
        let Some(placement) = self.objects.placement.take() else { return Err("No active placement".into()); };
        let doc = self.engine.document();
        let affines: Vec<_> = placement.objects.iter().filter_map(|h| Some((*h, doc.scene().object(*h)?.affine))).collect();
        let finish = (apply && affines.len() == placement.objects.len()).then(|| doc.set_image_object_affines_edit(&affines)).transpose().map_err(error)?;
        self.engine.preview_edit(placement.inverse).map_err(error)?;
        self.engine.backend_mut().prepare_moving_layer(None);
        self.objects.changed = true;
        if let Some(finish) = finish { self.layer_edit(Edit::Batch(vec![placement.insert, finish]))?; }
        self.object_tool(LayerCanvasTool::Move)?;
        self.refresh_document();
        self.refresh_commands();
        Ok(())
    }
    pub(super) fn object_destination(&self, destination: Option<ImageLayerDestination>) -> Result<ObjectDestination, String> {
        let doc = self.engine.document();
        let into_objects = |layer: OccurrenceHandle| -> Result<ObjectDestination, String> {
            if doc.is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
            Ok(ObjectDestination::Existing(layer))
        };
        match destination {
            Some(ImageLayerDestination { target, position: LayerDropPosition::Into }) if doc.scene().object_layer(target).is_some() => into_objects(target),
            None if doc.working.occurrence.is_some_and(|layer| doc.objects_editable(layer)) => into_objects(doc.working.occurrence.unwrap()),
            destination => self.image_layer_destination(destination).map(|(index, parent)| ObjectDestination::New { index, parent }),
        }
    }
    pub(super) fn insert_objects_edit(&self, objects: Vec<layer_core::ImageObject>, destination: Option<ImageLayerDestination>, masked: bool) -> Result<InsertedObjects, String> {
        if objects.is_empty() { return Err("Copy an image to paste".into()); }
        for object in &objects { object.validate()?; }
        let mut candidate = self.engine.document().clone();
        let mut edits = Vec::new();
        let mut operations = Vec::new();
        let target = if masked {
            let (index, parent) = self.image_layer_destination(destination)?;
            ObjectDestination::New { index, parent }
        } else { self.object_destination(destination)? };
        let (layer, at) = match target {
            ObjectDestination::Existing(layer) => (layer, 0),
            ObjectDestination::New { index, parent } => {
                let (layer, edit) = candidate.create_object_layer_edit(self.localization().text(MessageId::OBJECTS_LAYER_NAME).as_ref(), parent, index).map_err(error)?;
                candidate.apply(edit.clone()).map_err(error)?;
                edits.push(edit);
                if masked {
                    let occurrence = candidate.scene().occurrence(layer).ok_or("Unknown layer")?.clone();
                    let (coverage, mask, operation) = self.selection_mask(&occurrence, false, parent, candidate.composition().size)?;
                    let coverage = layer_core::RecordChange::insert(&candidate.artwork.coverage, coverage.value.ok_or("Missing pasted mask")?);
                    operations.extend(operation.map(|mut operation| {
                        operation.coverage.target = coverage.handle;
                        operation.coverage.use_.source = coverage.handle;
                        (SourceTarget::Coverage(coverage.handle), operation)
                    }));
                    let mut occurrence = occurrence;
                    occurrence.mask = Some(layer_core::authored::MaskUse { source: coverage.handle, ..mask });
                    let edit = Edit::Batch(vec![Edit::Coverage(coverage), Edit::Occurrence(layer_core::RecordChange::replace(&candidate.artwork.occurrences, layer, Some(occurrence))?)]);
                    candidate.apply(edit.clone()).map_err(error)?;
                    edits.push(edit);
                }
                (layer, 0)
            }
        };
        let offset = candidate.scene().occurrence_offset64(layer);
        let objects = objects.into_iter().map(|object| layer_core::ImageObject { affine: translation(offset.map(|v| -v)).compose(object.affine), ..object }).collect();
        let (handles, edit) = candidate.import_image_objects_edit(layer, objects, at).map_err(error)?;
        candidate.apply(edit.clone()).map_err(error)?;
        edits.push(edit);
        let mut working = candidate.working.clone();
        working.occurrence = Some(layer);
        working.target = None;
        working.inspect_mask = None;
        working.layer_selection = BTreeSet::from([layer]);
        working.layer_anchor = Some(layer);
        working.objects = handles.iter().copied().collect();
        if masked { working.selection = None; }
        edits.push(Edit::Working(working));
        Ok((Edit::Batch(edits), handles, operations))
    }
    pub(super) fn fitted_affine(&self, extent: [u32; 2], centre: [f64; 2], fit: bool) -> Affine64 {
        let canvas = self.engine.document().composition().size.map(f64::from);
        let size = extent.map(f64::from);
        let scale = if fit { 1_f64.min(canvas[0] / size[0]).min(canvas[1] / size[1]) } else { 1. };
        Affine64([scale, 0., 0., scale, centre[0] - size[0] * scale * 0.5, centre[1] - size[1] * scale * 0.5])
    }
    pub(super) fn view_centre64(&self) -> [f64; 2] {
        let camera = &self.state.camera;
        self.pointer64(camera.work_area_center())
    }
    pub(super) fn place_image_objects(&mut self, sources: Vec<(String, layer_core::color::source::SourceImage)>, centre: Option<Point>,
        destination: Option<ImageLayerDestination>, interactive: bool, fit: bool) -> Result<(), String> {
        self.require_document_idle()?;
        if self.operation.placing() || self.objects.placing() { return Err(self.localization().text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST).to_string()); }
        if !self.engine.backend().supports_tiled_sources() { return Err("This renderer does not support tiled photo layers".into()); }
        if sources.is_empty() { return Err("Choose at least one image".into()); }
        let centre = centre.map_or_else(|| if interactive { self.engine.document().composition().size.map(|v| f64::from(v) * 0.5) } else { self.view_centre64() },
            |p| [f64::from(p.x), f64::from(p.y)]);
        if !centre.iter().all(|v| v.is_finite()) { return Err("Invalid drop position".into()); }
        let objects = sources.into_iter().map(|(name, source)| {
            let name = layer_core::bounded_name(&name);
            if name.is_empty() { return Err("Use an image name with 1 to 128 characters".to_string()); }
            let mut object = layer_core::ImageObject::new(layer_core::Image::new(std::sync::Arc::new(source)), name);
            object.affine = self.fitted_affine(object.image.extent, centre, fit);
            Ok(object)
        }).collect::<Result<Vec<_>, String>>()?;
        let (edit, handles, _) = self.insert_objects_edit(objects, destination, false)?;
        self.source_edit_candidates(&edit, Default::default())?;
        if interactive { self.begin_object_placement(edit, handles) }
        else {
            self.layer_edit(edit)?;
            self.object_tool(LayerCanvasTool::Move)?;
            self.refresh_document();
            self.refresh_commands();
            Ok(())
        }
    }
    pub(super) fn sync_object_session(&mut self) {
        let doc = self.engine.document();
        let layer = doc.working.occurrence.filter(|_| !doc.working.objects.is_empty());
        let same = matches!((&self.objects.session, layer), (Some((owner, start)), Some(active))
            if *owner == active && start.iter().map(|(h, _)| *h).eq(doc.working.objects.iter().copied()));
        if !same {
            self.objects.session = layer.map(|layer| (layer, doc.working.objects.iter().filter_map(|h| Some((*h, doc.scene().object(*h)?.affine))).collect()));
        }
    }
    fn session_changes(&self) -> Vec<(ImageObjectHandle, Affine64)> {
        let doc = self.engine.document();
        self.objects.session.as_ref().filter(|(layer, _)| doc.working.occurrence == Some(*layer)).map_or_else(Vec::new, |(_, start)| {
            start.iter().copied().filter(|(h, affine)| doc.scene().object(*h).is_some_and(|object| object.affine != *affine)).collect()
        })
    }
    pub(super) fn reset_object_session(&mut self) -> Result<(), String> {
        let changes = self.session_changes();
        if changes.is_empty() { return Ok(()); }
        let edit = self.engine.document().set_image_object_affines_edit(&changes).map_err(error)?;
        if self.objects.placing() { self.engine.preview_edit(edit).map_err(error)?; } else { self.layer_edit(edit)?; }
        self.objects.frame = None;
        self.objects.changed = true;
        Ok(())
    }
    fn remember_transform(&mut self, delta: Affine64) {
        self.objects.last = Some((self.engine.document().owner, self.engine.checkpoint(), delta));
    }
    fn last_transform(&self) -> Option<Affine64> {
        self.objects.last.filter(|(owner, checkpoint, _)| *owner == self.engine.document().owner && self.engine.history_reaches(*checkpoint)).map(|(.., delta)| delta)
    }
}

#[cfg(test)]
#[path = "object_editing_tests.rs"]
mod tests;
