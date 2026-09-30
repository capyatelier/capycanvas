//! Operation-tool transaction and handles. Hosts paint the shared overlay and
//! render ordinary tool controls; no platform owns transform math or history.
use super::error;
use crate::*;
use layer_core::{Affine, Document, ImageTransform, Interpolation, LayerId, LayerKind, LayerOperationKind, MeshMap, Point, Projective, Rect, Selection, TransformMap};
use std::sync::Arc;
use layer_engine::{PenEvent, PenPhase};
use layer_render::{CanvasRenderer, CursorSegment, TransformPreview};
#[path = "operation/placement.rs"]
mod placement;
use placement::Placement;
pub(crate) use placement::PlacementInsertion;

/// Translate, rotate, shear x by y, then scale, all about the box centre.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    offset: Point,
    scale: [f32; 2],
    angle: f32,
    shear: f32,
}
impl Pose {
    fn identity() -> Self {
        Self {
            offset: Point::default(),
            scale: [1.; 2],
            angle: 0.,
            shear: 0.,
        }
    }
    fn linear(self) -> [f32; 4] {
        let (s, c) = self.angle.sin_cos();
        let [sx, sy] = self.scale;
        let k = self.shear;
        [c * sx, s * sx, (c * k - s) * sy, (s * k + c) * sy]
    }
    fn affine(self, pivot: Point) -> Affine {
        let [a, b, c, d] = self.linear();
        Affine::translation(sub(Point::default(), pivot))
            .then(Affine([a, b, c, d, 0., 0.]))
            .then(Affine::translation(add(pivot, self.offset)))
    }
    fn from_affine(affine: Affine, pivot: Point) -> Option<Self> {
        let [a, b, c, d, _, _] = affine.0;
        let sx = a.hypot(b);
        let angle = b.atan2(a);
        let (s, co) = angle.sin_cos();
        let sy = co * d - s * c;
        let pose = Self {
            offset: sub(affine.map(pivot), pivot),
            scale: [sx, sy],
            angle,
            shear: (co * c + s * d) / sy,
        };
        let det = a * d - b * c;
        (det.abs() > 1e-6 * (a * a + b * b + c * c + d * d) && pose.shear.is_finite()).then_some(pose)
    }
    fn map_linear(self, v: Point) -> Point {
        let [a, b, c, d] = self.linear();
        Point {
            x: a * v.x + c * v.y,
            y: b * v.x + d * v.y,
        }
    }
}
pub(super) const DISTORT_PLACEMENT: &str = "Select All, then Transform, to distort this photo's pixels";
pub(super) const OUTLINE_AFFINE: &str =
    "A selection outline can be moved, scaled, rotated and skewed; use Transform to distort or warp the pixels";
pub(super) const OUTLINE_PIXELS: &str = "Transform Outline moves no pixels";
/// Skew is presented as an angle; its tangent is the pose shear.
const MAX_SKEW: f32 = 85. * std::f32::consts::PI / 180.;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransformMode {
    Free,
    Distort,
    Warp,
}
#[derive(Clone, Copy)]
enum Handle {
    Move,
    Scale([f32; 2]),
    Rotate,
    Corner(usize),
    Edge(usize),
    Node(u32),
    Tangent(u32, u8),
}
/// A warp `mesh` maps the source first; `inner` then maps the source, or the
/// mesh's hull, onto a quad before the pose, which acts about the `frame`
/// box: the source rectangle, or the quad's bounds once a distortion has
/// been folded in.
#[derive(Clone)]
struct Geometry {
    pose: Pose,
    inner: Option<Projective>,
    mesh: Option<Arc<MeshMap>>,
    frame: Rect,
}
#[derive(Clone)]
struct Drag {
    handle: Handle,
    press: Point,
    current: Point,
    start: Geometry,
}
/// `bounds` is the source rectangle. An `outline` transaction moves only that
/// document selection's placement, never pixels. A `pixel_move` is a Move-tool
/// drag of the selected pixels by whole layer pixels, applied on release.
struct Transaction {
    placement: Option<Placement>,
    request: TransformPreview,
    revision: u64,
    basis: Affine,
    bounds: Rect,
    geometry: Geometry,
    cells: [u16; 2],
    node: Option<u32>,
    mode: TransformMode,
    perspective: bool,
    start: Pose,
    drag: Option<Drag>,
    outline: Option<Selection>,
    pixel_move: bool,
    keep_source: bool,
}
#[derive(Default)]
pub(super) struct Operation {
    current: Option<Transaction>,
    /// An open crop is a canvas operation too, so idle checks wait on it.
    pub crop: Option<super::crop::CropSession>,
    pub crop_options: super::crop::CropOptions,
    serial: u64,
    pub aspect: bool,
    pub interpolation: Option<Interpolation>,
    /// Move drags of selected pixels leave the originals in place.
    pub leave_copy: bool,
    /// The target and layer-local selection the renderer prepares a Move
    /// drag of.
    moving_pixels: Option<(LayerId, Selection)>,
    pub changed: bool,
}
impl Operation {
    /// A transform or crop session is open. A Move drag of selected pixels
    /// is part of its contact instead.
    pub fn active(&self) -> bool {
        self.transforming() || self.crop.is_some()
    }
    pub fn transforming(&self) -> bool {
        self.current.as_ref().is_some_and(|t| !t.pixel_move)
    }
    pub fn moving_pixels(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.pixel_move)
    }
    pub fn placing(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.placement.is_some())
    }
    pub fn outline(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.outline.is_some())
    }
    /// The outline being transformed, placed as it is now.
    pub fn outline_selection(&self) -> Option<Selection> {
        let t = self.current.as_ref()?;
        t.outline.as_ref()?.transformed(t.pose_affine()).ok()
    }
    pub fn serial(&self) -> u64 {
        self.serial
    }
    #[cfg(test)]
    pub(super) fn quad(&self) -> [Point; 4] {
        self.current.as_ref().unwrap().quad()
    }
    #[cfg(test)]
    pub(super) fn distorted(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.geometry.inner.is_some())
    }
    #[cfg(test)]
    pub(super) fn mesh(&self) -> Option<Arc<MeshMap>> {
        self.current.as_ref().and_then(|t| t.geometry.mesh.clone())
    }
    #[cfg(test)]
    pub(super) fn shear(&self) -> f32 {
        self.current.as_ref().map_or(0., |t| t.geometry.pose.shear)
    }
    pub fn dragging(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.drag.is_some())
            || self.crop.as_ref().is_some_and(super::crop::CropSession::dragging)
    }
    pub fn placement_count(&self) -> usize {
        self.current
            .as_ref()
            .and_then(|t| t.placement.as_ref())
            .map_or(0, |p| p.count())
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
pub(super) fn local_handle(bounds: Rect, side: [f32; 2]) -> Point {
    let c = center(bounds);
    Point {
        x: c.x + side[0] * (bounds.max.x - c.x),
        y: c.y + side[1] * (bounds.max.y - c.y),
    }
}
/// Half the side of a drawn transform handle, in logical pixels.
pub(super) const HANDLE_HALF_SIZE: f32 = 3.5;
pub(super) const HANDLES: [[f32; 2]; 8] = [
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
            ("Transform", "transform", true),
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
// regions may leave extra transparent room; operations remain bounded
// by the finite local editing area. Selecting an area uses that area's bounds instead.
fn content_bounds(doc: &Document, target: layer_core::LayerId) -> Rect {
    let layer = doc.target_owner(target).unwrap();
    let operations = layer.target_operations(target).unwrap();
    let extent = doc.target_extent(target);
    let canvas = Rect::from_extent(extent);
    let mut bounds = if doc
        .target_raster(target)
        .is_some_and(|r| r.try_data().is_none())
    {
        canvas
    } else if target != layer.id {
        layer
            .mask
            .as_ref()
            .and_then(|m| m.initial.as_ref())
            .map_or(Rect::EMPTY, |s| s.bounds())
    } else {
        layer.source.as_ref().map_or(Rect::EMPTY, |source| Rect::from_extent(source.extent))
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
        bounds = match &op.kind {
            LayerOperationKind::Transform(t) if op.coverage.initial.is_none() => {
                t.forward_bounds(bounds)
            }
            LayerOperationKind::ApplyMask | LayerOperationKind::Erase { .. } => bounds,
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
                        && (l.source.is_some()
                            || !l.raster.is_empty()
                            || !l.pending_operations.is_empty())
                }
            })
    }
    pub(super) fn begin_transform(&mut self) -> Result<(), String> {
        self.require_idle()?;
        if self.cropping() {
            return Err("Apply or cancel the crop first".into());
        }
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
        let t = self.pixel_transaction()?;
        self.operation.serial = t.request.transaction;
        self.operation.current = Some(t);
        self.layer_interaction.tool = LayerCanvasTool::Transform;
        self.state.layer_tools.tool = LayerCanvasTool::Transform;
        self.layer_interaction.changed = true;
        self.update_transform()?;
        Ok(())
    }
    /// Why a Move drag cannot move the active layer, or with a selection its
    /// selected pixels.
    pub(super) fn move_refusal(&self) -> Option<&'static str> {
        let doc = self.engine.document();
        let layer = doc.layer(doc.active_layer)?;
        if layer.kind == LayerKind::Background {
            Some("The paper can't be moved")
        } else if doc.is_locked(layer.id) {
            Some("The active layer is locked")
        } else if !self.moves_selected_pixels() {
            None
        } else if !doc.active_mask && layer.kind != LayerKind::Paint {
            Some("Choose a paint layer or a mask to move selected pixels")
        } else if !self.can_transform() {
            Some("This layer has no pixels to move")
        } else {
            None
        }
    }
    /// Start dragging the selected pixels with Move from `p`, in document
    /// pixels: a translation by whole layer pixels that keeps the Move tool,
    /// and with `keep_source` leaves the originals in place.
    pub(super) fn begin_move_transform(&mut self, p: Point, keep_source: bool) -> Result<(), String> {
        let mut t = self.pixel_transaction()?;
        let press = t.basis.inverse().ok_or("Invalid layer placement")?.map(p);
        t.pixel_move = true;
        t.keep_source = keep_source;
        t.drag = Some(Drag { handle: Handle::Move, press, current: press, start: t.geometry.clone() });
        self.operation.serial = t.request.transaction;
        self.operation.current = Some(t);
        self.layer_interaction.path = vec![press];
        self.update_transform()
    }
    /// A transform of the active target's pixels, bounded by the selection.
    fn pixel_transaction(&self) -> Result<Transaction, String> {
        let doc = self.engine.document();
        let target = doc.active_target();
        let basis = doc.layer_transform(target);
        let inverse = basis.inverse().ok_or("Invalid layer placement")?;
        let selection = doc.selection.as_ref().map(|s| s.transformed(inverse)).transpose().map_err(error)?;
        let serial = self.operation.serial.wrapping_add(1);
        let mut t = Transaction::new(serial, target, selection, doc.revision, basis, content_bounds(doc, target), Pose::identity());
        let bounds = &mut t.bounds;
        if let Some(companion) = t.request.companion(&doc.layers) {
            let other = content_bounds(doc, companion.layer);
            if !other.is_empty() {
                *bounds = bounds.union(doc.layer_transform(companion.layer).then(inverse).bounds(other));
            }
        }
        if bounds.is_empty() && doc.active_mask {
            *bounds = Rect {
                min: Point::default(),
                max: Point {
                    x: doc.target_extent(target)[0] as f32,
                    y: doc.target_extent(target)[1] as f32,
                },
            };
        }
        if let Some(s) = t.request.selection.as_ref().filter(|s| !s.inverted) {
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
        t.geometry.frame = t.bounds;
        Ok(t)
    }
    pub(super) fn outline_refusal(&self) -> Option<&'static str> {
        if self.selection_masks.target().is_some() {
            return Some("Return to the artwork first");
        }
        let empty = |s: &Selection| {
            !s.inverted
                && match &s.shape {
                    layer_core::SelectionShape::Contours(paths) => paths.is_empty(),
                    layer_core::SelectionShape::Pixels(pixels) => {
                        let [x0, y0, x1, y1] = pixels.bounds();
                        x0 >= x1 || y0 >= y1
                    }
                }
        };
        match &self.engine.document().selection {
            None => Some("Make a selection first"),
            Some(s) if empty(s) => Some("The selection is empty"),
            Some(_) => None,
        }
    }
    /// Transform the selection's placement only, in document space, as the
    /// selection display previews it.
    pub(super) fn begin_outline_transform(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        if let Some(reason) = self.outline_refusal() {
            return Err(reason.into());
        }
        self.cancel_layer_gesture()?;
        let doc = self.engine.document();
        let selection = doc.selection.clone().ok_or("Make a selection first")?;
        let mut bounds = outline_bounds(&selection, [doc.width, doc.height]);
        bounds.max.x = bounds.max.x.max(bounds.min.x + 1.);
        bounds.max.y = bounds.max.y.max(bounds.min.y + 1.);
        let serial = self.operation.serial.wrapping_add(1);
        let t = Transaction::new(serial, doc.active_layer, None, doc.revision, Affine::IDENTITY, bounds, Pose::identity());
        self.operation.serial = serial;
        self.operation.current = Some(Transaction { outline: Some(selection), ..t });
        self.layer_interaction.tool = LayerCanvasTool::Transform;
        self.state.layer_tools.tool = LayerCanvasTool::Transform;
        self.layer_interaction.changed = true;
        self.update_transform()
    }
    pub(super) fn finish_transform(&mut self, apply: bool) -> Result<(), String> {
        self.require_idle()?;
        if self.cropping() {
            return self.finish_crop(apply);
        }
        if self.operation.placing() {
            return self.finish_layer_placement(apply);
        }
        if self.operation.outline() {
            if apply
                && let Some(selection) = self.operation.outline_selection()
                && self.engine.document().selection.as_ref() != Some(&selection)
            {
                self.layer_edit(layer_core::Edit::SetSelection(Some(selection)))?;
            }
            self.cancel_transform()?;
            return Ok(());
        }
        if apply {
            if self.queue_transform_selection() {
                return Ok(());
            }
            self.engine.commit_transform(None).map_err(error)?;
        }
        self.cancel_transform()?;
        Ok(())
    }
    pub(super) fn apply_transform_selection(
        &mut self,
        pixels: std::sync::Arc<layer_core::SelectionPixels>,
    ) -> Result<(), String> {
        self.engine.commit_transform(Some(pixels)).map_err(error)?;
        self.cancel_transform()?;
        Ok(())
    }
    pub(super) fn cancel_transform(&mut self) -> Result<bool, String> {
        if self.cropping() {
            self.finish_crop(false)?;
            return Ok(true);
        }
        if self.operation.placing() {
            self.finish_layer_placement(false)?;
            return Ok(true);
        }
        if self.region_tools.applying_transform() {
            self.region_tools.cancel();
        }
        let Some(transaction) = self.operation.current.take() else {
            return Ok(false);
        };
        if transaction.outline.is_some() {
            self.sync_selection_overlay();
        } else {
            if !transaction.pixel_move {
                self.engine.backend_mut().prepare_moving_layer(None);
            }
            self.engine.set_transform_preview(None).map_err(error)?;
        }
        self.layer_interaction.path.clear();
        self.layer_interaction.tool = LayerCanvasTool::Move;
        self.state.layer_tools.tool = LayerCanvasTool::Move;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        self.operation.changed = true;
        Ok(true)
    }
    pub(super) fn reconcile_transform(&mut self) {
        self.reconcile_crop();
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
        let chosen = self.operation.interpolation;
        let Some(t) = &mut self.operation.current else {
            return Ok(());
        };
        let transform = ImageTransform {
            map: t.map().ok_or("Invalid transform")?,
            interpolation: t.interpolation(chosen),
            keep_source: t.keep_source,
        };
        if transform != t.request.transform && self.region_tools.applying_transform() {
            self.region_tools.cancel();
        }
        t.request.transform = transform;
        let moving = t.drag.is_some();
        t.request.moving = moving;
        let placing = t.placement.is_some();
        let affine = t.pose_affine();
        if t.outline.is_some() {
            self.sync_selection_overlay();
        } else if let Some(placement) = &t.placement {
            let mut edits = Vec::new();
            for layer in placement.preview_layers(self.engine.document(), affine)? {
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
        } else {
            self.engine
                .set_transform_preview(Some(t.request.clone()))
                .map_err(error)?;
        }
        if !moving {
            self.layer_interaction.changed |= placing;
            self.refresh_tools();
            self.operation.changed = true;
        }
        Ok(())
    }
    pub(super) fn transform_controls(&self) -> Vec<tool_settings::ToolSetting> {
        let Some(t) = self.operation.current.as_ref().filter(|t| t.mode != TransformMode::Warp) else {
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
        let degrees = |limit: f64| NumericControl {
            scale: 180. / std::f64::consts::PI,
            step: std::f64::consts::PI / 180.,
            resolution: 0.00001,
            digits: 1,
            ..NumericControl::number(-limit, limit, 0.01, 3).unit("°")
        };
        let pose = t.geometry.pose;
        [
            (
                "transform_x",
                "X",
                "Position",
                pixels(t.bounds.max.x - t.bounds.min.x),
                pose.offset.x,
            ),
            (
                "transform_y",
                "Y",
                "Position",
                pixels(t.bounds.max.y - t.bounds.min.y),
                pose.offset.y,
            ),
            (
                "transform_width",
                "Width",
                "Scale",
                percent(),
                pose.scale[0],
            ),
            (
                "transform_height",
                "Height",
                "Scale",
                percent(),
                pose.scale[1],
            ),
            (
                "transform_angle",
                "Angle",
                "Rotation",
                degrees(std::f64::consts::PI),
                pose.angle,
            ),
            (
                "transform_skew",
                "Skew",
                "Skew",
                degrees(f64::from(MAX_SKEW)),
                pose.shear.atan(),
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
        let mut pose = t.geometry.pose;
        match id {
            "transform_x" => pose.offset.x = value,
            "transform_y" => pose.offset.y = value,
            "transform_angle" => pose.angle = value,
            "transform_skew" => pose.shear = value.tan(),
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
        t.geometry.pose = pose;
        self.update_transform()
    }
    pub(super) fn transform_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        let reach = self.ruler_reach();
        let Some(t) = &mut self.operation.current else {
            return Ok(());
        };
        let pixel_move = t.pixel_move;
        let p = t.basis.inverse().ok_or("Invalid layer placement")?.map(p);
        match event.phase {
            PenPhase::Down => {
                if let Some(handle) = t.hit(p, reach) {
                    if let Handle::Node(node) = handle {
                        t.node = Some(node);
                    }
                    t.drag = Some(Drag {
                        handle,
                        press: p,
                        current: p,
                        start: t.geometry.clone(),
                    });
                    self.layer_interaction.path = vec![p];
                }
            }
            PenPhase::Move | PenPhase::Up => {
                let Some(drag) = &mut t.drag else {
                    return Ok(());
                };
                drag.current = p;
                let drag = drag.clone();
                t.apply_drag(drag, p, self.interaction.modifiers, self.operation.aspect);
                if event.phase == PenPhase::Up {
                    t.drag = None;
                    self.layer_interaction.path.clear();
                }
                self.update_transform()?;
                if pixel_move && event.phase == PenPhase::Up {
                    self.engine.commit_transform(None).map_err(error)?;
                    self.cancel_transform()?;
                }
            }
            PenPhase::Cancel if pixel_move => {
                self.cancel_transform()?;
            }
            PenPhase::Cancel => {
                self.cancel_transform_drag()?;
            }
            PenPhase::Hover => (),
        }
        Ok(())
    }
    pub(super) fn reorient_transform(&mut self, command: CommandId) -> Result<(), String> {
        self.require_idle()?;
        if command == CommandId::ResetTransform && self.cropping() {
            return self.reset_crop();
        }
        let t = self.operation.current.as_mut().ok_or("Start a transform first")?;
        let quarter = std::f32::consts::FRAC_PI_2;
        let (flip, turn) = match command {
            CommandId::TransformFlipHorizontal => ([-1., 1.], 0.),
            CommandId::TransformFlipVertical => ([1., -1.], 0.),
            CommandId::TransformRotateLeft => ([1., 1.], -quarter),
            CommandId::TransformRotateRight => ([1., 1.], quarter),
            CommandId::ResetTransform => {
                t.reset();
                return self.update_transform();
            }
            _ => return Err("Not a transform command".into()),
        };
        if t.mode == TransformMode::Warp {
            let mesh = t.geometry.mesh.clone().ok_or("Start a warp first")?;
            let mesh = mesh.post(Affine::around(center(mesh.bounds()), flip, turn, Point::default()));
            t.geometry.frame = mesh.bounds();
            t.geometry.mesh = Some(Arc::new(mesh));
        } else {
            let pose = &mut t.geometry.pose;
            if flip != [1., 1.] {
                pose.angle = -pose.angle;
                pose.shear = -pose.shear;
                pose.scale = [pose.scale[0] * flip[0], pose.scale[1] * flip[1]];
            } else {
                let pi = std::f32::consts::PI;
                pose.angle = (pose.angle + turn + pi).rem_euclid(std::f32::consts::TAU) - pi;
            }
        }
        self.update_transform()
    }
    pub(super) fn cancel_transform_drag(&mut self) -> Result<bool, String> {
        if self.cancel_crop_drag() {
            return Ok(true);
        }
        let Some(t) = &mut self.operation.current else {
            return Ok(false);
        };
        let Some(drag) = t.drag.take() else {
            return Ok(false);
        };
        t.geometry = drag.start;
        self.layer_interaction.path.clear();
        self.update_transform()?;
        Ok(true)
    }
    /// Whether a finger at `position` manipulates a transform: a handle or
    /// the inside of an open transform's box, or the selected area under
    /// Move, which drags the selected pixels.
    pub(super) fn transform_touch_hit(&self, position: [f32; 2]) -> bool {
        if self.cropping() {
            return self.crop_touch_hit(position);
        }
        let p = self.state.camera.input_transform().map(Point { x: position[0], y: position[1] });
        let Some(t) = self.operation.current.as_ref() else {
            return self.moves_selected_pixels()
                && self.engine.document().selection.as_ref().is_some_and(|s| {
                    let b = s.coverage_bounds();
                    s.inverted != (b.min.x <= p.x && p.x < b.max.x && b.min.y <= p.y && p.y < b.max.y)
                });
        };
        let Some(inverse) = t.basis.inverse() else { return false; };
        t.hit(inverse.map(p), self.ruler_reach()).is_some()
    }
    /// Let the renderer prepare, while the canvas is idle, the Move drag of
    /// the selected pixels that the next press would start.
    pub(super) fn sync_moving_pixels(&mut self) {
        let doc = self.engine.document();
        let target = doc.active_target();
        let next = (self.moves_selected_pixels() && self.move_refusal().is_none())
            .then(|| {
                let inverse = doc.layer_transform(target).inverse()?;
                Some((target, doc.selection.as_ref()?.transformed(inverse).ok()?))
            })
            .flatten();
        if self.operation.moving_pixels != next {
            self.engine.backend_mut().prepare_moving_pixels(next.clone());
            self.operation.moving_pixels = next;
        }
    }
    /// Move drags the selected pixels rather than the whole layer.
    pub(super) fn moves_selected_pixels(&self) -> bool {
        self.layer_interaction.tool == LayerCanvasTool::Move
            && self.engine.document().selection.is_some()
            && self.selection_masks.target().is_none()
    }
    pub(super) fn update_transform_drag(&mut self) -> Result<bool, String> {
        let Some(t) = &mut self.operation.current else {
            return Ok(false);
        };
        let Some(drag) = t.drag.clone() else {
            return Ok(false);
        };
        let current = drag.current;
        t.apply_drag(drag, current, self.interaction.modifiers, self.operation.aspect);
        self.update_transform()?;
        Ok(true)
    }
    pub(super) fn set_transform_mode(&mut self, mode: TransformMode, uniform: bool) -> Result<(), String> {
        self.require_idle()?;
        let t = self.operation.current.as_mut().ok_or("Start a transform first")?;
        if mode != TransformMode::Free && t.outline.is_some() {
            return Err(OUTLINE_AFFINE.into());
        }
        if mode != TransformMode::Free && t.placement.is_some() {
            return Err(DISTORT_PLACEMENT.into());
        }
        t.set_mode(mode);
        self.operation.aspect = uniform;
        self.update_transform()
    }
    pub(super) fn toggle_transform_perspective(&mut self) -> Result<(), String> {
        let t = self.operation.current.as_mut().ok_or("Start a transform first")?;
        t.perspective = !t.perspective;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn transform_mode(&self) -> Option<(TransformMode, bool)> {
        self.operation.current.as_ref().map(|t| (t.mode, t.perspective))
    }
    pub(super) fn warp_cells(&self) -> Option<[u16; 2]> {
        self.operation.current.as_ref().filter(|t| t.mode == TransformMode::Warp).map(|t| t.cells)
    }
    pub(super) fn set_warp_cells(&mut self, cells: [u16; 2]) -> Result<(), String> {
        self.require_idle()?;
        let t = self
            .operation
            .current
            .as_mut()
            .filter(|t| t.mode == TransformMode::Warp)
            .ok_or("Choose Warp first")?;
        t.set_cells(cells);
        self.update_transform()
    }
    pub(super) fn transform_interpolation(&self) -> Option<Interpolation> {
        self.operation
            .current
            .as_ref()
            .filter(|t| t.placement.is_none())
            .map(|t| t.interpolation(self.operation.interpolation))
    }
    pub(super) fn set_transform_interpolation(&mut self, interpolation: Interpolation) -> Result<(), String> {
        self.require_idle()?;
        let t = self.operation.current.as_ref().ok_or("Start a transform first")?;
        if t.outline.is_some() {
            return Err(OUTLINE_PIXELS.into());
        }
        if t.placement.is_some() {
            return Err("Placed photos keep their original pixels".into());
        }
        self.operation.interpolation = Some(interpolation);
        self.update_transform()
    }
    pub(super) fn document_to_logical(&self) -> impl Fn(Point) -> [f32; 2] + use<R> {
        let camera = Affine(self.state.camera.view().document_to_surface);
        let dpi = self
            .logical_viewport
            .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        move |p| {
            let p = camera.map(p);
            [p.x / dpi, p.y / dpi]
        }
    }
    fn transform_surface_map(&self, t: &Transaction) -> impl Fn(Point) -> [f32; 2] + use<R> {
        let (map, basis) = (self.document_to_logical(), t.basis);
        move |p| map(basis.map(p))
    }
    /// Every transform handle centre, in logical surface pixels.
    pub(super) fn transform_handle_points(&self) -> Vec<[f32; 2]> {
        let Some(t) = &self.operation.current else {
            return Vec::new();
        };
        let map = self.transform_surface_map(t);
        t.handles(self.ruler_reach()).into_iter().map(|(_, p)| map(p)).collect()
    }
    pub(super) fn transform_document_bounds(&self) -> Option<[f32; 4]> {
        let t = self.operation.current.as_ref()?;
        let bounds = Rect::around(t.corners().map(|p| t.basis.map(p)));
        Some([bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y])
    }
    /// The transformed box corners, in logical surface pixels.
    pub(super) fn transform_hull(&self) -> Option<[[f32; 2]; 4]> {
        let t = self.operation.current.as_ref()?;
        Some(t.corners().map(self.transform_surface_map(t)))
    }
    pub(super) fn append_transform_overlay(&self, segments: &mut Vec<CursorSegment>) {
        let Some(t) = self.operation.current.as_ref().filter(|t| !t.pixel_move) else {
            return;
        };
        let map = self.transform_surface_map(t);
        let reach = self.ruler_reach();
        let mut line = |a, b, solid| {
            segments.push(CursorSegment {
                from: a,
                to: b,
                distance: 0.,
                marker: f32::from(solid),
                scale: 1.,
            })
        };
        if let (TransformMode::Warp, Some(mesh)) = (t.mode, t.geometry.mesh.as_deref()) {
            let source = t.bounds;
            let [columns, rows] = mesh.cells;
            let along = |from: Point, to: Point, cells: u16| -> Vec<[f32; 2]> {
                let steps = 8 * usize::from(cells);
                (0..=steps)
                    .filter_map(|s| {
                        let f = s as f32 / steps as f32;
                        mesh.map(Point { x: from.x + (to.x - from.x) * f, y: from.y + (to.y - from.y) * f })
                    })
                    .map(&map)
                    .collect()
            };
            let span = sub(source.max, source.min);
            for i in 0..=columns {
                let x = source.min.x + span.x * f32::from(i) / f32::from(columns);
                let curve = along(Point { x, y: source.min.y }, Point { x, y: source.max.y }, rows);
                for pair in curve.windows(2) {
                    line(pair[0], pair[1], true);
                }
            }
            for j in 0..=rows {
                let y = source.min.y + span.y * f32::from(j) / f32::from(rows);
                let curve = along(Point { x: source.min.x, y }, Point { x: source.max.x, y }, columns);
                for pair in curve.windows(2) {
                    line(pair[0], pair[1], true);
                }
            }
            if let Some((node, at)) = t.node.and_then(|node| mesh.node(node).map(|p| (node, p))) {
                for side in 0..4 {
                    if let Some(tangent) = mesh.tangent(node, side) {
                        line(map(at), map(tangent), true);
                    }
                }
            }
        } else {
            let corners = t.corners().map(&map);
            for i in 0..4 {
                line(corners[i], corners[(i + 1) % 4], false);
            }
            if t.mode == TransformMode::Free {
                line(
                    map(t.pose_affine().map(local_handle(t.geometry.frame, [0., -1.]))),
                    map(t.rotate_handle(reach)),
                    true,
                );
            }
        }
        for (_, p) in t.handles(reach) {
            let [x, y] = map(p);
            segments.push(CursorSegment {
                from: [x - HANDLE_HALF_SIZE, y - HANDLE_HALF_SIZE],
                to: [x + HANDLE_HALF_SIZE, y + HANDLE_HALF_SIZE],
                distance: 0.,
                marker: 2.,
                scale: 1.,
            });
        }
    }
}

/// The box a selection outline transforms: its shape's exact bounds, or the
/// canvas for an inverted selection, which covers everything outside them.
fn outline_bounds(selection: &Selection, extent: [u32; 2]) -> Rect {
    if selection.inverted {
        return Rect::from_extent(extent);
    }
    let mut local = Rect::EMPTY;
    match &selection.shape {
        layer_core::SelectionShape::Contours(paths) => {
            for p in paths.iter().flat_map(|c| c.iter()) {
                local.include_circle(*p, 0.);
            }
        }
        layer_core::SelectionShape::Pixels(pixels) => {
            let [x0, y0, x1, y1] = pixels.bounds();
            if x0 < x1 && y0 < y1 {
                local = Rect {
                    min: Point { x: x0 as f32, y: y0 as f32 },
                    max: Point { x: x1 as f32, y: y1 as f32 },
                };
            }
        }
    }
    selection.affine.bounds(local)
}
fn quad_of(bounds: Rect, inner: Option<Projective>, outer: Affine) -> [Point; 4] {
    [0, 2, 4, 6].map(|i| {
        let corner = local_handle(bounds, HANDLES[i]);
        outer.map(inner.and_then(|m| m.map(corner)).unwrap_or(corner))
    })
}
fn midpoint(a: Point, b: Point) -> Point {
    Point { x: (a.x + b.x) * 0.5, y: (a.y + b.y) * 0.5 }
}
/// The handle nearest `p` within `reach`, measured after `to_world`.
pub(super) fn nearest_handle<H>(handles: impl IntoIterator<Item = (H, Point)>, p: Point, reach: f32, to_world: Affine) -> Option<H> {
    let world = to_world.map(p);
    handles
        .into_iter()
        .map(|(handle, q)| {
            let q = to_world.map(q);
            ((world.x - q.x).hypot(world.y - q.y), handle)
        })
        .filter(|(d, _)| *d <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, handle)| handle)
}
pub(super) fn inside_convex(quad: &[Point; 4], p: Point) -> bool {
    let side = |a: Point, b: Point| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let sides = [0, 1, 2, 3].map(|i| side(quad[i], quad[(i + 1) % 4]));
    sides.iter().all(|s| *s >= 0.) || sides.iter().all(|s| *s <= 0.)
}
impl Transaction {
    fn new(transaction: u64, layer: LayerId, selection: Option<Selection>, revision: u64, basis: Affine, bounds: Rect, pose: Pose) -> Self {
        Self {
            placement: None,
            request: TransformPreview {
                transaction,
                moving: false,
                layer,
                selection,
                transform: ImageTransform::default(),
            },
            revision,
            basis,
            bounds,
            geometry: Geometry {
                pose,
                inner: None,
                mesh: None,
                frame: bounds,
            },
            cells: MeshMap::PRESETS[0],
            node: None,
            mode: TransformMode::Free,
            perspective: false,
            start: pose,
            drag: None,
            outline: None,
            pixel_move: false,
            keep_source: false,
        }
    }
    fn pose_affine(&self) -> Affine {
        self.geometry.pose.affine(center(self.geometry.frame))
    }
    fn outer_bounds(&self) -> Rect {
        self.geometry.mesh.as_ref().map_or(self.bounds, |mesh| mesh.bounds())
    }
    fn map(&self) -> Option<TransformMap> {
        if self.pixel_move {
            let offset = self.geometry.pose.offset;
            return Some(TransformMap::Affine(Affine::translation(Point { x: offset.x.round(), y: offset.y.round() })));
        }
        let pose = self.pose_affine();
        let outer = match self.geometry.inner {
            Some(inner) => inner.then(Projective::from_affine(pose))?.into(),
            None => TransformMap::Affine(pose),
        };
        Some(match (&self.geometry.mesh, outer) {
            (None, outer) => outer,
            (Some(mesh), TransformMap::Affine(affine)) => TransformMap::Mesh(Arc::new(mesh.post(affine))),
            (Some(mesh), outer) => {
                TransformMap::Mesh(Arc::new(MeshMap::fit(self.bounds, mesh.cells, |p| outer.map(mesh.map(p)?))?))
            }
        })
    }
    /// The source rectangle's corners as displayed, top-left clockwise.
    fn quad(&self) -> [Point; 4] {
        quad_of(self.outer_bounds(), self.geometry.inner, self.pose_affine())
    }
    fn corners(&self) -> [Point; 4] {
        match self.mode {
            TransformMode::Distort => self.quad(),
            TransformMode::Free => {
                let affine = self.pose_affine();
                [0, 2, 4, 6].map(|i| affine.map(local_handle(self.geometry.frame, HANDLES[i])))
            }
            TransformMode::Warp => {
                let hull = self.outer_bounds();
                [0, 2, 4, 6].map(|i| local_handle(hull, HANDLES[i]))
            }
        }
    }
    fn handles(&self, reach: f32) -> Vec<(Handle, Point)> {
        match self.mode {
            TransformMode::Warp => {
                let Some(mesh) = &self.geometry.mesh else {
                    return Vec::new();
                };
                let tangents = self.node.into_iter().flat_map(|node| {
                    (0..4u8).filter_map(move |side| mesh.tangent(node, side).map(|p| (Handle::Tangent(node, side), p)))
                });
                tangents
                    .chain((0..mesh.node_count()).filter_map(|node| mesh.node(node).map(|p| (Handle::Node(node), p))))
                    .collect()
            }
            TransformMode::Distort => {
                let q = self.quad();
                (0..4)
                    .map(|i| (Handle::Corner(i), q[i]))
                    .chain((0..4).map(|i| (Handle::Edge(i), midpoint(q[i], q[(i + 1) % 4]))))
                    .collect()
            }
            TransformMode::Free => {
                let affine = self.pose_affine();
                HANDLES
                    .into_iter()
                    .map(|h| (Handle::Scale(h), affine.map(local_handle(self.geometry.frame, h))))
                    .chain([(Handle::Rotate, self.rotate_handle(reach))])
                    .collect()
            }
        }
    }
    fn fold(&mut self) {
        let outer = Projective::from_affine(self.pose_affine());
        let Some(inner) = self.geometry.inner.map_or(Some(outer), |m| m.then(outer)) else {
            return;
        };
        let bounds = self.outer_bounds();
        self.geometry.inner = Some(inner);
        self.geometry.pose = Pose::identity();
        self.geometry.frame = inner.bounds(bounds).unwrap_or(bounds);
    }
    fn enter_warp(&mut self) {
        let mesh = match self.map() {
            Some(TransformMap::Mesh(mesh)) => Some(mesh),
            Some(map) => MeshMap::fit(self.bounds, self.cells, |p| map.map(p)).map(Arc::new),
            None => None,
        };
        if let Some(mesh) = mesh {
            self.geometry = Geometry {
                pose: Pose::identity(),
                inner: None,
                frame: mesh.bounds(),
                mesh: Some(mesh),
            };
        }
    }
    fn set_cells(&mut self, cells: [u16; 2]) {
        self.cells = cells;
        self.node = None;
        let refit = self.geometry.mesh.as_deref().and_then(|mesh| MeshMap::fit(self.bounds, cells, |p| mesh.map(p)));
        if let Some(mesh) = refit {
            self.geometry.frame = mesh.bounds();
            self.geometry.mesh = Some(Arc::new(mesh));
        }
    }
    fn interpolation(&self, chosen: Option<Interpolation>) -> Interpolation {
        if self.pixel_move {
            return Interpolation::Nearest;
        }
        match (self.placement.is_some(), chosen, self.mode) {
            (true, ..) => Interpolation::Linear,
            (false, Some(chosen), _) => chosen,
            (false, None, TransformMode::Distort | TransformMode::Warp) => Interpolation::Bicubic,
            (false, None, TransformMode::Free) => Interpolation::Linear,
        }
    }
    fn set_mode(&mut self, mode: TransformMode) {
        match mode {
            TransformMode::Warp => self.enter_warp(),
            TransformMode::Distort => self.fold(),
            TransformMode::Free => {
                let bounds = self.outer_bounds();
                if let Some(inner) = self.geometry.inner {
                    let combined = inner.then(Projective::from_affine(self.pose_affine()));
                    match combined.and_then(Projective::as_affine).and_then(|a| Pose::from_affine(a, center(bounds))) {
                        Some(pose) => {
                            self.geometry.pose = pose;
                            self.geometry.inner = None;
                            self.geometry.frame = bounds;
                        }
                        None => self.fold(),
                    }
                } else if self.mode == TransformMode::Warp {
                    self.geometry.frame = bounds;
                }
            }
        }
        self.mode = mode;
    }
    fn reset(&mut self) {
        self.geometry = Geometry {
            pose: self.start,
            inner: None,
            mesh: None,
            frame: self.bounds,
        };
        self.node = None;
        self.mode = TransformMode::Free;
    }
    fn apply_drag(&mut self, drag: Drag, p: Point, modifiers: Modifiers, aspect: bool) {
        if p == drag.press {
            self.geometry = drag.start;
            return;
        }
        let delta = sub(p, drag.press);
        let warped = match (drag.handle, drag.start.mesh.as_deref()) {
            (Handle::Node(node), Some(mesh)) => Some(mesh.move_node(node, delta)),
            (Handle::Tangent(node, side), Some(mesh)) => {
                Some(mesh.tangent(node, side).and_then(|t| mesh.move_tangent(node, side, add(t, delta))))
            }
            (Handle::Move, Some(mesh)) if self.mode == TransformMode::Warp => {
                Some(Some(mesh.post(Affine::translation(delta))))
            }
            _ => None,
        };
        if let Some(mesh) = warped {
            if let Some(mesh) = mesh.filter(MeshMap::valid) {
                self.geometry.frame = mesh.bounds();
                self.geometry.mesh = Some(Arc::new(mesh));
            }
            return;
        }
        let distorts = matches!(drag.handle, Handle::Corner(_) | Handle::Edge(_))
            || (self.mode == TransformMode::Distort && matches!(drag.handle, Handle::Move));
        if distorts {
            self.geometry.pose = drag.start.pose;
            if let Some(inner) = self.drag_quad(&drag, p, modifiers) {
                self.geometry.inner = Some(inner);
            }
        } else {
            self.geometry.pose = self.drag_pose(&drag, p, modifiers, aspect);
        }
    }
    fn drag_quad(&self, drag: &Drag, p: Point, modifiers: Modifiers) -> Option<Projective> {
        let outer = drag.start.pose.affine(center(self.geometry.frame));
        let bounds = self.outer_bounds();
        let mut q = quad_of(bounds, drag.start.inner, outer);
        let mut delta = sub(p, drag.press);
        match drag.handle {
            Handle::Corner(i) => {
                if self.perspective || modifiers.shift {
                    let (along_x, along_y) = ([1, 0, 3, 2][i], [3, 2, 1, 0][i]);
                    if delta.x.abs() >= delta.y.abs() {
                        q[along_x].x -= delta.x;
                        delta.y = 0.;
                    } else {
                        q[along_y].y -= delta.y;
                        delta.x = 0.;
                    }
                }
                q[i] = add(q[i], delta);
            }
            Handle::Edge(i) => {
                q[i] = add(q[i], delta);
                q[(i + 1) % 4] = add(q[(i + 1) % 4], delta);
            }
            Handle::Move => q = q.map(|c| add(c, delta)),
            Handle::Scale(_) | Handle::Rotate | Handle::Node(_) | Handle::Tangent(..) => return drag.start.inner,
        }
        let inverse = outer.inverse()?;
        Projective::rect_to_quad(bounds, q.map(|c| inverse.map(c)))
    }
    fn rotate_handle(&self, reach: f32) -> Point {
        let top = self
            .pose_affine()
            .map(local_handle(self.geometry.frame, [0., -1.]));
        let (s, c) = self.geometry.pose.angle.sin_cos();
        let [a, b, d, e, _, _] = self.basis.0;
        let length = (a * s - d * c).hypot(b * s - e * c);
        let distance = reach * 2.5 * self.geometry.pose.scale[1].signum() / length;
        add(top, Point { x: s * distance, y: -c * distance })
    }
    fn hit(&self, p: Point, reach: f32) -> Option<Handle> {
        nearest_handle(self.handles(reach), p, reach, self.basis)
            .or_else(|| inside_convex(&self.corners(), p).then_some(Handle::Move))
    }
    fn drag_pose(&self, drag: &Drag, p: Point, modifiers: Modifiers, aspect: bool) -> Pose {
        let mut pose = drag.start.pose;
        let pivot = center(self.geometry.frame);
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
            Handle::Scale(side) if modifiers.command && (side[0] == 0. || side[1] == 0.) => {
                let start = pose.affine(pivot);
                let moving = local_handle(self.geometry.frame, side);
                let fixed = local_handle(self.geometry.frame, side.map(|v| -v));
                let top_bottom = side[0] == 0.;
                let along = pose.map_linear(if top_bottom {
                    Point { x: 1., y: 0. }
                } else {
                    Point { x: 0., y: 1. }
                });
                let delta = sub(p, drag.press);
                let travel = (delta.x * along.x + delta.y * along.y) / (along.x * along.x + along.y * along.y);
                let lever = if top_bottom { moving.y - fixed.y } else { moving.x - fixed.x };
                let limit = MAX_SKEW.tan();
                let skew = if top_bottom {
                    (travel / lever).clamp(-limit - pose.shear, limit - pose.shear)
                } else {
                    travel / lever
                };
                let shear = if top_bottom {
                    Affine([1., 0., skew, 1., -skew * fixed.y, 0.])
                } else {
                    Affine([1., skew, 0., 1., 0., -skew * fixed.x])
                };
                if let Some(next) = Pose::from_affine(shear.then(start), pivot)
                    && next.shear.abs() <= limit
                {
                    pose = next;
                }
            }
            Handle::Corner(_) | Handle::Edge(_) | Handle::Node(_) | Handle::Tangent(..) => {}
            Handle::Scale(side) => {
                let start = pose.affine(pivot);
                let moving = local_handle(self.geometry.frame, side);
                let fixed = if modifiers.alt {
                    pivot
                } else {
                    local_handle(self.geometry.frame, side.map(|v| -v))
                };
                let anchor = start.map(fixed);
                let current = add(start.map(moving), sub(p, drag.press));
                let (s, c) = pose.angle.sin_cos();
                let delta = sub(current, anchor);
                let local = [delta.x * c + delta.y * s, -delta.x * s + delta.y * c];
                let original = pose.scale;
                let span = [moving.x - fixed.x, moving.y - fixed.y];
                if side[1] != 0. {
                    pose.scale[1] = local[1] / span[1];
                }
                if side[0] != 0. {
                    pose.scale[0] = (local[0] - pose.shear * pose.scale[1] * span[1]) / span[0];
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
                pose.offset = sub(sub(anchor, pose.map_linear(sub(fixed, pivot))), pivot);
            }
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transaction() -> Transaction {
        let bounds = Rect {
            min: Point { x: 20., y: 40. },
            max: Point { x: 240., y: 190. },
        };
        Transaction::new(1, layer_core::LayerId(1), None, 0, Affine::translation(Point { x: 7., y: 11. }), bounds, Pose::identity())
    }
    fn near(a: Point, b: Point) {
        assert!((a.x - b.x).hypot(a.y - b.y) < 0.001, "{a:?} != {b:?}");
    }
    #[test]
    fn distorting_a_warp_follows_the_true_perspective() {
        let mut t = transaction();
        t.set_mode(TransformMode::Warp);
        let warp = Arc::new(t.geometry.mesh.as_ref().unwrap().move_node(5, Point { x: 30., y: -20. }).unwrap());
        t.geometry.mesh = Some(warp.clone());
        t.set_mode(TransformMode::Distort);
        let hull = t.outer_bounds();
        let width = hull.max.x - hull.min.x;
        let keystone = [
            Point { x: hull.min.x + width / 3., y: hull.min.y },
            Point { x: hull.max.x - width / 3., y: hull.min.y },
            hull.max,
            Point { x: hull.min.x, y: hull.max.y },
        ];
        let keystone = Projective::rect_to_quad(hull, keystone).unwrap();
        t.geometry.inner = Some(keystone);
        let Some(TransformMap::Mesh(fitted)) = t.map() else { panic!("a mesh") };
        let worst = (0..=20 * 20)
            .map(|n| Point {
                x: t.bounds.min.x + (t.bounds.max.x - t.bounds.min.x) * (n % 21) as f32 / 20.,
                y: t.bounds.min.y + (t.bounds.max.y - t.bounds.min.y) * (n / 21) as f32 / 20.,
            })
            .map(|p| {
                let [a, b] = [fitted.map(p).unwrap(), keystone.map(warp.map(p).unwrap()).unwrap()];
                (a.x - b.x).hypot(a.y - b.y)
            })
            .fold(0f32, f32::max);
        assert!(worst < 0.5, "{worst}px from the perspective of the warp");
    }
    #[test]
    fn poses_decompose_every_invertible_affine_exactly() {
        let pivot = Point { x: 40., y: 25. };
        for pose in [
            Pose::identity(),
            Pose { offset: Point { x: 3., y: -8. }, scale: [1.5, 0.5], angle: 0.8, shear: 0.3 },
            Pose { offset: Point { x: -30., y: 2. }, scale: [0.7, -2.], angle: -2.5, shear: -1.1 },
        ] {
            let affine = pose.affine(pivot);
            let back = Pose::from_affine(affine, pivot).unwrap().affine(pivot);
            for (a, b) in affine.0.iter().zip(back.0) {
                assert!((a - b).abs() < 1e-4, "{affine:?} != {back:?}");
            }
        }
        assert!(Pose::from_affine(Affine([1., 2., 2., 4., 0., 0.]), pivot).is_none());
    }
    #[test]
    fn control_drag_of_an_edge_skews_about_the_opposite_edge() {
        let mut t = transaction();
        let pivot = center(t.bounds);
        let command = Modifiers { command: true, ..Default::default() };
        for (angle, scale) in [(0., [1., 1.]), (0.6, [1.4, -0.8])] {
            t.geometry.pose = Pose { offset: Point { x: 12., y: 5. }, scale, angle, shear: 0. };
            let original = t.geometry.pose.affine(pivot);
            for side in [[0., 1.], [0., -1.], [1., 0.], [-1., 0.]] {
                let press = original.map(local_handle(t.bounds, side));
                let along = t.geometry.pose.map_linear(if side[0] == 0. {
                    Point { x: 1., y: 0. }
                } else {
                    Point { x: 0., y: 1. }
                });
                let drag = Drag { handle: Handle::Scale(side), press, current: press, start: t.geometry.clone() };
                let target = add(press, Point { x: along.x * 0.2, y: along.y * 0.2 });
                let next = t.drag_pose(&drag, target, command, false);
                let fixed = local_handle(t.bounds, side.map(|v| -v));
                near(original.map(fixed), next.affine(pivot).map(fixed));
                near(next.affine(pivot).map(local_handle(t.bounds, side)), target);
                assert_ne!(next.affine(pivot), original);
            }
        }
    }
    #[test]
    fn every_handle_keeps_its_anchor_and_grab_offset_under_rotation_and_reflection() {
        let mut t = transaction();
        for (angle, shear) in [(0., 0.), (0.7, 0.), (-2.1, 0.), (0.7, 0.45), (-2.1, -0.3)] {
            for scale in [[1., 1.], [-1.5, 0.7], [2., -3.]] {
                t.geometry.pose = Pose {
                    offset: Point { x: 35., y: -17. },
                    angle,
                    scale,
                    shear,
                };
                let pivot = center(t.bounds);
                let original = t.geometry.pose.affine(pivot);
                for side in HANDLES {
                    let press = add(
                        original.map(local_handle(t.bounds, side)),
                        Point { x: 2., y: -3. },
                    );
                    let drag = Drag {
                        handle: Handle::Scale(side),
                        press,
                        current: press,
                        start: t.geometry.clone(),
                    };
                    for alt in [false, true] {
                        for shift in [false, true] {
                            let modifiers = Modifiers {
                                alt,
                                shift,
                                ..Default::default()
                            };
                            let unchanged = t.drag_pose(&drag, press, modifiers, false);
                            for p in [t.bounds.min, t.bounds.max] {
                                near(unchanged.affine(pivot).map(p), original.map(p));
                            }
                            let next = t.drag_pose(
                                &drag,
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
                &Drag {
                    handle: Handle::Scale([1., 1.]),
                    press,
                    current: press,
                    start: t.geometry.clone(),
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
            &Drag {
                handle: Handle::Rotate,
                press,
                current,
                start: t.geometry.clone(),
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
