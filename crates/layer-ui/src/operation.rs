//! Operation-tool transaction and handles. Hosts paint the shared overlay and
//! render ordinary tool controls; no platform owns transform math or history.
use super::error;
use crate::*;
use layer_core::{Affine, Document, ImageTransform, LayerKind, LayerOperationKind, Point, Rect};
use layer_engine::{PenEvent, PenPhase};
use layer_render::{CanvasRenderer, CursorSegment, TransformPreview};
#[path = "operation/placement.rs"]
mod placement;
use placement::Placement;
pub(crate) use placement::PlacementInsertion;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    offset: Point,
    scale: [f32; 2],
    angle: f32,
}
impl Pose {
    fn identity() -> Self {
        Self {
            offset: Point::default(),
            scale: [1.; 2],
            angle: 0.,
        }
    }
    fn affine(self, pivot: Point) -> Affine {
        Affine::around(pivot, self.scale, self.angle, self.offset)
    }
}
#[derive(Clone, Copy)]
enum Handle {
    Move,
    Scale([f32; 2]),
    Rotate,
}
#[derive(Clone, Copy)]
struct Drag {
    handle: Handle,
    press: Point,
    current: Point,
    pose: Pose,
}
struct Transaction {
    placement: Option<Placement>,
    request: TransformPreview,
    revision: u64,
    basis: Affine,
    bounds: Rect,
    pose: Pose,
    drag: Option<Drag>,
}
#[derive(Default)]
pub(super) struct Operation {
    current: Option<Transaction>,
    serial: u64,
    pub aspect: bool,
    pub changed: bool,
}
impl Operation {
    pub fn active(&self) -> bool {
        self.current.is_some()
    }
    pub fn placing(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.placement.is_some())
    }
}
fn center(b: Rect) -> Point {
    Point {
        x: (b.min.x + b.max.x) * 0.5,
        y: (b.min.y + b.max.y) * 0.5,
    }
}
fn add(a: Point, b: Point) -> Point {
    Point {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}
fn sub(a: Point, b: Point) -> Point {
    Point {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}
fn local_handle(bounds: Rect, side: [f32; 2]) -> Point {
    let c = center(bounds);
    Point {
        x: c.x + side[0] * (bounds.max.x - c.x),
        y: c.y + side[1] * (bounds.max.y - c.y),
    }
}
const HANDLES: [[f32; 2]; 8] = [
    [-1., -1.],
    [0., -1.],
    [1., -1.],
    [1., 0.],
    [1., 1.],
    [0., 1.],
    [-1., 1.],
    [-1., 0.],
];

pub(crate) fn tool_set(transform: bool) -> ToolSetView {
    ToolSetView {
        groups: [
            ("Move", "move", false),
            ("Scale / rotate", "transform", true),
        ]
        .into_iter()
        .map(|(label, icon, item)| ToolSetItem {
            label,
            icon,
            preview: None,
            selected: item == transform,
            action: if item {
                UiAction::Invoke {
                    command: CommandId::ScaleRotate,
                }
            } else {
                UiAction::Layer {
                    action: LayerAction::Tool {
                        tool: LayerCanvasTool::Move,
                    },
                }
            },
        })
        .collect(),
        subtools: Vec::new(),
    }
}

// Conservative allocated tile geometry avoids a readback at interaction start. Erased
// regions may leave extra transparent room; operations and assets remain bounded
// by the finite local editing area. Selecting an area uses that area's bounds instead.
fn content_bounds(doc: &Document, target: layer_core::LayerId) -> Rect {
    let layer = doc.target_owner(target).unwrap();
    let operations = layer.target_operations(target).unwrap();
    let extent = doc.target_extent(target);
    let canvas = Rect {
        min: Point::default(),
        max: Point {
            x: extent[0] as f32,
            y: extent[1] as f32,
        },
    };
    let mut bounds = if doc
        .target_raster(target)
        .is_some_and(|r| r.try_data().is_none())
        || (target == layer.id && layer.asset.is_some())
    {
        canvas
    } else if target != layer.id {
        layer
            .mask
            .as_ref()
            .and_then(|m| m.initial.as_ref())
            .map_or(Rect::EMPTY, |s| s.bounds())
    } else {
        layer.source.as_ref().map_or(Rect::EMPTY, |source| Rect {
            min: Point::default(),
            max: Point { x: source.extent[0] as f32, y: source.extent[1] as f32 },
        })
    };
    if let Some(Ok(data)) = doc.target_raster(target).and_then(|r| r.try_data()) {
        for key in data.tiles.keys() {
            let [x, y] = key
                .coordinate
                .map(|v| (v * layer_core::raster::TILE_SIZE) as f32);
            bounds = bounds.union(Rect {
                min: Point { x, y },
                max: Point {
                    x: x + 256.,
                    y: y + 256.,
                },
            });
        }
    }
    for op in operations {
        bounds = match op.kind {
            LayerOperationKind::Transform(t) if op.coverage.initial.is_none() => {
                t.affine.bounds(bounds)
            }
            LayerOperationKind::ApplyMask => bounds,
            _ => bounds.union(op.bounds(extent)),
        };
    }
    Rect {
        min: Point {
            x: bounds.min.x.max(0.),
            y: bounds.min.y.max(0.),
        },
        max: Point {
            x: bounds.max.x.min(canvas.max.x),
            y: bounds.max.y.min(canvas.max.y),
        },
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn can_transform(&self) -> bool {
        let doc = self.engine.document();
        !doc.is_locked(doc.active_target())
            && doc.layer(doc.active_layer).is_some_and(|l| {
                if doc.active_mask {
                    l.mask.is_some()
                } else {
                    l.kind == LayerKind::Paint
                        && (l.asset.is_some()
                            || l.source.is_some()
                            || !l.raster.is_empty()
                            || !l.pending_operations.is_empty())
                }
            })
    }
    pub(super) fn begin_transform(&mut self) -> Result<(), String> {
        self.require_idle()?;
        if !self.can_transform() {
            return Err("Select unlocked paint content or a layer mask".into());
        }
        if self.operation.active() {
            return Ok(());
        }
        self.cancel_layer_gesture()?;
        if !self.engine.document().active_mask
            && self.engine.document().selection.is_none()
            && self.engine.document().layer(self.engine.document().active_layer).is_some_and(|l| l.source.is_some())
        {
            return self.begin_layer_placement(None);
        }
        let doc = self.engine.document();
        let target = doc.active_target();
        let basis = doc.layer_transform(target);
        let inverse = basis.inverse().ok_or("Invalid layer placement")?;
        let selection = doc.selection.as_ref().map(|s| s.transformed(inverse)).transpose().map_err(error)?;
        let mut bounds = content_bounds(doc, target);
        let request = TransformPreview {
            transaction: 0,
            layer: target,
            selection: None,
            transform: Default::default(),
        };
        if let Some(companion) = request.companion(&doc.layers) {
            let other = content_bounds(doc, companion.layer);
            if !other.is_empty() {
                bounds = bounds.union(doc.layer_transform(companion.layer).then(inverse).bounds(other));
            }
        }
        if bounds.is_empty() && doc.active_mask {
            // A constant mask has no allocated content, but its finite editing
            // area is still selectable. Transforming it preserves that constant.
            bounds = Rect {
                min: Point::default(),
                max: Point {
                    x: doc.target_extent(target)[0] as f32,
                    y: doc.target_extent(target)[1] as f32,
                },
            };
        }
        if let Some(s) = selection.as_ref().filter(|s| !s.inverted) {
            let b = s.bounds();
            bounds.min.x = bounds.min.x.max(b.min.x);
            bounds.min.y = bounds.min.y.max(b.min.y);
            bounds.max.x = bounds.max.x.min(b.max.x);
            bounds.max.y = bounds.max.y.min(b.max.y);
        }
        if bounds.is_empty() {
            return Err("The selection does not overlap this layer".into());
        }
        bounds.max.x = bounds.max.x.max(bounds.min.x + 1.);
        bounds.max.y = bounds.max.y.max(bounds.min.y + 1.);
        self.operation.serial = self.operation.serial.wrapping_add(1);
        self.operation.current = Some(Transaction {
            placement: None,
            request: TransformPreview {
                transaction: self.operation.serial,
                layer: target,
                selection,
                transform: ImageTransform::default(),
            },
            revision: doc.revision,
            basis,
            bounds,
            pose: Pose::identity(),
            drag: None,
        });
        self.layer_interaction.tool = LayerCanvasTool::Transform;
        self.state.layer_tools.tool = LayerCanvasTool::Transform;
        self.layer_interaction.changed = true;
        self.update_transform()?;
        Ok(())
    }
    pub(super) fn finish_transform(&mut self, apply: bool) -> Result<(), String> {
        self.require_idle()?;
        if self.operation.placing() {
            return self.finish_layer_placement(apply);
        }
        if apply {
            self.engine.commit_transform().map_err(error)?;
        }
        self.cancel_transform()?;
        Ok(())
    }
    pub(super) fn cancel_transform(&mut self) -> Result<bool, String> {
        if self.operation.placing() {
            self.finish_layer_placement(false)?;
            return Ok(true);
        }
        if self.operation.current.take().is_none() {
            return Ok(false);
        }
        self.engine.set_transform_preview(None).map_err(error)?;
        self.layer_interaction.path.clear();
        self.layer_interaction.tool = LayerCanvasTool::Move;
        self.state.layer_tools.tool = LayerCanvasTool::Move;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        self.operation.changed = true;
        Ok(true)
    }
    pub(super) fn reconcile_transform(&mut self) {
        if self
            .operation
            .current
            .as_ref()
            .is_some_and(|t| t.revision != self.engine.document().revision)
        {
            let _ = self.cancel_transform();
        }
    }
    fn update_transform(&mut self) -> Result<(), String> {
        let Some(t) = &mut self.operation.current else {
            return Ok(());
        };
        t.request.transform.affine = t.pose.affine(center(t.bounds));
        if let Some(placement) = &t.placement {
            let mut edits = Vec::new();
            for layer in placement.preview_layers(self.engine.document(), t.request.transform.affine)? {
                if self.engine.document().is_locked(layer.id) {
                    return Err("The destination layer is locked".into());
                }
                if self.engine.document().layer(layer.id) != Some(&layer) {
                    edits.push(layer_core::Edit::ReplaceLayer(Box::new(layer)));
                }
            }
            if !edits.is_empty() {
                self.engine.preview_edit(layer_core::Edit::Batch(edits)).map_err(error)?;
            }
            t.revision = self.engine.document().revision;
            self.layer_interaction.changed = true;
        } else {
            self.engine
                .set_transform_preview(Some(t.request.clone()))
                .map_err(error)?;
        }
        self.refresh_tools();
        self.operation.changed = true;
        Ok(())
    }
    pub(super) fn transform_controls(&self) -> Vec<tool_settings::ToolSetting> {
        let Some(t) = &self.operation.current else {
            return Vec::new();
        };
        let pixels = |span: f32| NumericControl {
            soft_min: -f64::from(span.clamp(256., 65536.)),
            soft_max: f64::from(span.clamp(256., 65536.)),
            ..NumericControl::number(-65536., 65536., 1., 1).unit("px")
        };
        let percent = || NumericControl {
            digits: 0,
            min: -100.,
            max: 100.,
            soft_min: 0.01,
            soft_max: 4.,
            ..NumericControl::percent()
        };
        [
            (
                "transform_x",
                "X",
                "Position",
                pixels(t.bounds.max.x - t.bounds.min.x),
                t.pose.offset.x,
            ),
            (
                "transform_y",
                "Y",
                "Position",
                pixels(t.bounds.max.y - t.bounds.min.y),
                t.pose.offset.y,
            ),
            (
                "transform_width",
                "Width",
                "Scale",
                percent(),
                t.pose.scale[0],
            ),
            (
                "transform_height",
                "Height",
                "Scale",
                percent(),
                t.pose.scale[1],
            ),
            (
                "transform_angle",
                "Angle",
                "Rotation",
                NumericControl {
                    scale: 180. / std::f64::consts::PI,
                    step: std::f64::consts::PI / 180.,
                    resolution: 0.00001,
                    digits: 1,
                    ..NumericControl::number(-std::f64::consts::PI, std::f64::consts::PI, 0.01, 3)
                        .unit("°")
                },
                t.pose.angle,
            ),
        ]
        .into_iter()
        .map(
            |(id, label, group, numeric, value)| tool_settings::ToolSetting {
                id,
                label,
                group,
                numeric,
                value,
            },
        )
        .collect()
    }
    pub(super) fn set_transform_control(&mut self, id: &str, value: f32) -> Result<(), String> {
        self.require_idle()?;
        let control = self
            .transform_controls()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("No transform setting")?;
        control.numeric.validate(value, control.label)?;
        let t = self.operation.current.as_mut().ok_or("No transform")?;
        let mut pose = t.pose;
        match id {
            "transform_x" => pose.offset.x = value,
            "transform_y" => pose.offset.y = value,
            "transform_angle" => pose.angle = value,
            "transform_width" | "transform_height" => {
                if value.abs() < 0.001 {
                    return Err("Scale cannot be zero".into());
                }
                let axis = usize::from(id == "transform_height");
                if self.operation.aspect {
                    pose.scale[1 - axis] *= value / pose.scale[axis];
                }
                pose.scale[axis] = value;
            }
            _ => unreachable!(),
        }
        if pose
            .scale
            .iter()
            .any(|v| !v.is_finite() || !(0.001..=100.).contains(&v.abs()))
        {
            return Err("Scale must be between 0.1% and 10000%".into());
        }
        t.pose = pose;
        self.update_transform()
    }
    pub(super) fn transform_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        let reach = self.ruler_reach();
        let Some(t) = &mut self.operation.current else {
            return Ok(());
        };
        let p = t.basis.inverse().ok_or("Invalid layer placement")?.map(p);
        match event.phase {
            PenPhase::Down => {
                if let Some(handle) = t.hit(p, reach) {
                    t.drag = Some(Drag {
                        handle,
                        press: p,
                        current: p,
                        pose: t.pose,
                    });
                    self.layer_interaction.path = vec![p];
                }
            }
            PenPhase::Move | PenPhase::Up => {
                if let Some(drag) = &mut t.drag {
                    drag.current = p;
                }
                if let Some(drag) = t.drag {
                    t.pose =
                        t.drag_pose(drag, p, self.interaction.modifiers, self.operation.aspect);
                }
                if event.phase == PenPhase::Up {
                    t.drag = None;
                    self.layer_interaction.path.clear();
                }
                self.update_transform()?;
            }
            PenPhase::Cancel => {
                self.cancel_transform()?;
            }
            PenPhase::Hover => (),
        }
        Ok(())
    }
    /// A contact on active photo handles/body directly manipulates placement.
    /// Touch outside them keeps the existing two-finger camera gesture route.
    pub(super) fn placement_touch_hit(&self, position: [f32; 2]) -> bool {
        let Some(t) = self.operation.current.as_ref().filter(|t| t.placement.is_some()) else { return false; };
        let Some(inverse) = t.basis.inverse() else { return false; };
        let p = self.state.camera.input_transform().map(Point { x: position[0], y: position[1] });
        t.hit(inverse.map(p), self.ruler_reach()).is_some()
    }
    pub(super) fn update_transform_drag(&mut self) -> Result<bool, String> {
        let Some(t) = &mut self.operation.current else {
            return Ok(false);
        };
        let Some(drag) = t.drag else {
            return Ok(false);
        };
        t.pose = t.drag_pose(
            drag,
            drag.current,
            self.interaction.modifiers,
            self.operation.aspect,
        );
        self.update_transform()?;
        Ok(true)
    }
    pub(super) fn append_transform_overlay(&self, segments: &mut Vec<CursorSegment>) {
        let Some(t) = &self.operation.current else {
            return;
        };
        let camera = Affine(self.state.camera.view().document_to_surface);
        let dpi = self
            .logical_viewport
            .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let map = |p| {
            let p = camera.map(t.basis.map(p));
            [p.x / dpi, p.y / dpi]
        };
        let affine = t.pose.affine(center(t.bounds));
        let corners = [0, 2, 4, 6].map(|i| map(affine.map(local_handle(t.bounds, HANDLES[i]))));
        let mut line = |a, b, solid| {
            segments.push(CursorSegment {
                from: a,
                to: b,
                distance: 0.,
                marker: f32::from(solid),
                scale: 1.,
            })
        };
        for i in 0..4 {
            line(corners[i], corners[(i + 1) % 4], false);
        }
        let rotate = t.rotate_handle(self.ruler_reach());
        line(
            map(affine.map(local_handle(t.bounds, [0., -1.]))),
            map(rotate),
            true,
        );
        for p in HANDLES
            .into_iter()
            .map(|h| affine.map(local_handle(t.bounds, h)))
            .chain([rotate])
        {
            let [x, y] = map(p);
            segments.push(CursorSegment {
                from: [x - 3.5, y - 3.5],
                to: [x + 3.5, y + 3.5],
                distance: 0.,
                marker: 2.,
                scale: 1.,
            });
        }
    }
}

impl Transaction {
    fn rotate_handle(&self, reach: f32) -> Point {
        let top = self
            .pose
            .affine(center(self.bounds))
            .map(local_handle(self.bounds, [0., -1.]));
        let (s, c) = self.pose.angle.sin_cos();
        let [a, b, d, e, _, _] = self.basis.0;
        let length = (a * s - d * c).hypot(b * s - e * c);
        let distance = reach * 2.5 * self.pose.scale[1].signum() / length;
        add(top, Point { x: s * distance, y: -c * distance })
    }
    fn hit(&self, p: Point, reach: f32) -> Option<Handle> {
        let world = self.basis.map(p);
        let distance = |q: Point| {
            let q = self.basis.map(q);
            (world.x - q.x).hypot(world.y - q.y)
        };
        if distance(self.rotate_handle(reach)) <= reach { return Some(Handle::Rotate); }
        let affine = self.pose.affine(center(self.bounds));
        if let Some((_, h)) = HANDLES.into_iter()
            .map(|h| (distance(affine.map(local_handle(self.bounds, h))), h))
            .filter(|(d, _)| *d <= reach).min_by(|a, b| a.0.total_cmp(&b.0))
        { return Some(Handle::Scale(h)); }
        let q = affine.inverse()?.map(p);
        (q.x >= self.bounds.min.x && q.y >= self.bounds.min.y
            && q.x <= self.bounds.max.x && q.y <= self.bounds.max.y).then_some(Handle::Move)
    }
    fn drag_pose(&self, drag: Drag, p: Point, modifiers: Modifiers, aspect: bool) -> Pose {
        let mut pose = drag.pose;
        let pivot = center(self.bounds);
        match drag.handle {
            Handle::Move => {
                let mut delta = sub(p, drag.press);
                if modifiers.shift {
                    if delta.x.abs() > delta.y.abs() {
                        delta.y = 0.;
                    } else {
                        delta.x = 0.;
                    }
                }
                pose.offset = add(pose.offset, delta);
            }
            Handle::Rotate => {
                let c = add(pivot, pose.offset);
                pose.angle +=
                    (p.y - c.y).atan2(p.x - c.x) - (drag.press.y - c.y).atan2(drag.press.x - c.x);
                if modifiers.shift {
                    let step = std::f32::consts::PI / 12.;
                    pose.angle = (pose.angle / step).round() * step;
                }
                pose.angle = (pose.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
            }
            Handle::Scale(side) => {
                let start = pose.affine(pivot);
                let moving = local_handle(self.bounds, side);
                let fixed = if modifiers.alt {
                    pivot
                } else {
                    local_handle(self.bounds, side.map(|v| -v))
                };
                let anchor = start.map(fixed);
                let current = add(start.map(moving), sub(p, drag.press));
                let (s, c) = pose.angle.sin_cos();
                let delta = sub(current, anchor);
                let local = [delta.x * c + delta.y * s, -delta.x * s + delta.y * c];
                let original = pose.scale;
                let span = [moving.x - fixed.x, moving.y - fixed.y];
                for axis in 0..2 {
                    if side[axis] != 0. {
                        pose.scale[axis] = local[axis] / span[axis];
                    }
                }
                if aspect || modifiers.shift {
                    let axis = if side[0] == 0. {
                        1
                    } else if side[1] == 0.
                        || (pose.scale[0] / original[0] - 1.).abs()
                            >= (pose.scale[1] / original[1] - 1.).abs()
                    {
                        0
                    } else {
                        1
                    };
                    let factor = pose.scale[axis] / original[axis];
                    pose.scale = original.map(|v| v * factor);
                }
                pose.scale = pose.scale.map(|v| {
                    if v.abs() < 0.001 {
                        v.signum() * 0.001
                    } else {
                        v.clamp(-100., 100.)
                    }
                });
                let fixed_delta = sub(fixed, pivot);
                let q = Point {
                    x: fixed_delta.x * pose.scale[0],
                    y: fixed_delta.y * pose.scale[1],
                };
                pose.offset = sub(
                    sub(
                        anchor,
                        Point {
                            x: c * q.x - s * q.y,
                            y: s * q.x + c * q.y,
                        },
                    ),
                    pivot,
                );
            }
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transaction() -> Transaction {
        Transaction {
            placement: None,
            request: TransformPreview {
                transaction: 1,
                layer: layer_core::LayerId(1),
                selection: None,
                transform: Default::default(),
            },
            revision: 0,
            basis: Affine::translation(Point { x: 7., y: 11. }),
            bounds: Rect {
                min: Point { x: 20., y: 40. },
                max: Point { x: 240., y: 190. },
            },
            pose: Pose::identity(),
            drag: None,
        }
    }
    fn near(a: Point, b: Point) {
        assert!((a.x - b.x).hypot(a.y - b.y) < 0.001, "{a:?} != {b:?}");
    }
    #[test]
    fn every_handle_keeps_its_anchor_and_grab_offset_under_rotation_and_reflection() {
        let mut t = transaction();
        for angle in [0., 0.7, -2.1] {
            for scale in [[1., 1.], [-1.5, 0.7], [2., -3.]] {
                t.pose = Pose {
                    offset: Point { x: 35., y: -17. },
                    angle,
                    scale,
                };
                let pivot = center(t.bounds);
                let original = t.pose.affine(pivot);
                for side in HANDLES {
                    let press = add(
                        original.map(local_handle(t.bounds, side)),
                        Point { x: 2., y: -3. },
                    );
                    let drag = Drag {
                        handle: Handle::Scale(side),
                        press,
                        current: press,
                        pose: t.pose,
                    };
                    for alt in [false, true] {
                        for shift in [false, true] {
                            let modifiers = Modifiers {
                                alt,
                                shift,
                                ..Default::default()
                            };
                            let unchanged = t.drag_pose(drag, press, modifiers, false);
                            for p in [t.bounds.min, t.bounds.max] {
                                near(unchanged.affine(pivot).map(p), original.map(p));
                            }
                            let next = t.drag_pose(
                                drag,
                                add(press, Point { x: 50., y: -30. }),
                                modifiers,
                                false,
                            );
                            let fixed = if alt {
                                pivot
                            } else {
                                local_handle(t.bounds, side.map(|v| -v))
                            };
                            near(original.map(fixed), next.affine(pivot).map(fixed));
                            assert!(next.affine(pivot).inverse().is_some());
                            if shift {
                                assert!(
                                    (next.scale[0] / scale[0] - next.scale[1] / scale[1]).abs()
                                        < 0.00001
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn hit_testing_and_zero_crossing_and_rotation_are_stable() {
        let t = transaction();
        let pivot = center(t.bounds);
        assert!(matches!(t.hit(pivot, 12.), Some(Handle::Move)));
        assert!(matches!(
            t.hit(t.rotate_handle(12.), 12.),
            Some(Handle::Rotate)
        ));
        assert!(t.hit(Point { x: -100., y: -100. }, 12.).is_none());
        let press = t.bounds.max;
        for delta in [-220., -221., -219.99] {
            let next = t.drag_pose(
                Drag {
                    handle: Handle::Scale([1., 1.]),
                    press,
                    current: press,
                    pose: t.pose,
                },
                add(press, Point { x: delta, y: -150. }),
                Modifiers::default(),
                false,
            );
            assert!(next.affine(pivot).inverse().is_some());
        }
        let press = add(pivot, Point { x: 80., y: 0. });
        let current = add(pivot, Point { x: 30., y: 70. });
        let next = t.drag_pose(
            Drag {
                handle: Handle::Rotate,
                press,
                current,
                pose: t.pose,
            },
            current,
            Modifiers {
                shift: true,
                ..Default::default()
            },
            false,
        );
        let units = next.angle / (std::f32::consts::PI / 12.);
        assert!((units - units.round()).abs() < 0.00001);
        near(next.affine(pivot).map(pivot), pivot);
    }
}
