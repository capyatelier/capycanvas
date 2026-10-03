//! Operation-tool transaction and handles. Hosts paint the shared overlay and
//! render ordinary tool controls; no platform owns transform math or history.
use super::{error, refused};
use crate::*;
use crate::localization::MessageId;
use layer_core::{Affine, Document, ImageTransform, Interpolation, LayerId, LayerKind, MeshMap, Point, Projective, Rect, Selection, LayerPlacement};
use std::sync::Arc;
use std::collections::BTreeSet;
use layer_engine::{PenEvent, PenPhase};
use layer_render::{CanvasRenderer, CursorSegment, TransformPreview};
#[path = "operation/placement.rs"]
mod placement;
use placement::Placement;
#[path = "operation/snapping.rs"]
mod snapping;
use snapping::Snapping;
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
pub(super) const OUTLINE_AFFINE: MessageId = MessageId::COMMANDS_REFUSAL_OPERATION_A_SELECTION_OUTLINE_CAN_BE_MOVED_SCALED_ROTATED_AND_SKEWED_USE_TRANSFORM_TO_DISTORT_OR_WARP_THE_PIXELS;
pub(super) const OUTLINE_PIXELS: MessageId = MessageId::COMMANDS_REFUSAL_OPERATION_TRANSFORM_OUTLINE_MOVES_NO_PIXELS;
/// Skew is presented as an angle; its tangent is the pose shear.
const MAX_SKEW: f32 = 85. * std::f32::consts::PI / 180.;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransformMode {
    Free,
    Distort,
    Warp,
}
#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Move,
    Pivot,
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
    exact_affine: Option<(Pose, Projective)>,
    inner: Option<Projective>,
    mesh: Option<Arc<MeshMap>>,
    frame: Rect,
    pivot: Point,
    interpolation: Option<Interpolation>,
    nodes: BTreeSet<u32>,
}
impl Geometry {
    fn outer(&self) -> Option<Projective> {
        if self.inner.is_none() && let Some((pose, exact)) = self.exact_affine
            && self.pose == pose { return Some(exact); }
        if self.pose == Pose::identity() { return Some(self.inner.unwrap_or(Projective::IDENTITY)); }
        let pose = Projective::from_affine(self.pose.affine(center(self.frame)));
        self.inner.map_or(Some(pose), |inner| inner.then(pose))
    }
}
struct WarpSplit {
    axes: [bool; 2],
    surface: layer_core::Tessellation,
    hover: Option<Point>,
}
#[derive(Clone)]
struct Drag {
    handle: Handle,
    press: Point,
    current: Point,
    start: Geometry,
    anchor: Point,
    snapping: Option<Snapping>,
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
    accepted: Geometry,
    node: Option<u32>,
    select_points: bool,
    split: Option<WarpSplit>,
    mode: TransformMode,
    perspective: bool,
    start: Geometry,
    source: Rect,
    drag: Option<Drag>,
    outline: Option<Selection>,
    pixel_move: bool,
    retained_move: bool,
    keep_source: bool,
    reference: CanvasAnchor,
}
#[derive(Default)]
pub(super) struct Operation {
    current: Option<Transaction>,
    /// An open crop is a canvas operation too, so idle checks wait on it.
    pub crop: Option<super::crop::CropSession>,
    pub crop_options: super::crop::CropOptions,
    serial: u64,
    pub aspect: bool,
    interpolation: Option<Interpolation>,
    pub snapping: bool,
    nudging: Option<String>,
    pub last_transform: Option<Projective>,
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
        self.current.as_ref().is_some_and(|t| !t.pixel_move && !t.retained_move)
    }
    pub fn moving_layer(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.retained_move)
    }
    pub fn nudging(&self) -> bool { self.nudging.is_some() }
    pub fn moving_pixels(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.pixel_move)
    }
    pub fn placing(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.placement.is_some())
    }
    pub fn warp_available(&self) -> bool {
        self.current.as_ref().is_some_and(|t| t.outline.is_none() && t.placement.as_ref().is_none_or(Placement::single_leaf))
    }
    pub fn original_size_available(&self) -> bool {
        self.current.as_ref().and_then(|t| t.placement.as_ref())
            .is_some_and(|placement| placement.members.iter().all(|layer| layer.source.is_some()
                && layer.properties.placement.as_affine().is_some()))
            && self.current.as_ref().is_some_and(|t| t.map().is_some_and(|map| map.as_affine().is_some()))
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

pub(crate) fn tool_set(transform: bool, localizer: &crate::localization::Localizer) -> ToolSetView {
    ToolSetView {
        groups: [
            (localizer.text(crate::localization::MessageId::TOOL_OPERATION_MOVE), "move", false),
            (localizer.text(crate::localization::MessageId::TOOL_OPERATION_TRANSFORM), "transform", true),
        ]
        .into_iter()
        .map(|(label, icon, item)| ToolSetItem { enabled: true,
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

fn source_frame(doc: &Document, target: layer_core::LayerId) -> Rect {
    Rect::from_extent(doc.layer(target).and_then(|layer| layer.source.as_ref())
        .map_or_else(|| doc.target_extent(target), |source| source.extent))
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn transform_roots(&self) -> Vec<LayerId> {
        let doc = self.engine.document();
        let roots = doc.layer_roots(&self.layer_interaction.selected);
        if roots.is_empty() { vec![doc.active_layer] } else { roots }
    }
    pub(super) fn retained_transforming(&self) -> bool {
        let doc = self.engine.document();
        !doc.active_mask && doc.selection.is_none()
    }
    pub(super) fn can_transform(&self) -> bool {
        let doc = self.engine.document();
        if self.retained_transforming() {
            return doc.retained_transform_targets(&self.transform_roots()).is_ok();
        }
        self.transform_roots().len() == 1 && !doc.is_locked(doc.active_target())
            && doc.affine_edit_transform(doc.active_target()).is_some()
            && doc.layer(doc.active_layer).is_some_and(|l| if doc.active_mask { l.mask.is_some() }
                else { l.kind == LayerKind::Paint && (l.source.is_some() || !l.raster.is_empty() || !l.pending_operations.is_empty()) })
    }
    pub(super) fn begin_transform(&mut self) -> Result<(), String> {
        self.require_idle()?;
        if self.cropping() {
            return Err("Apply or cancel the crop first".into());
        }
        if self.retained_transforming() {
            self.engine.document().retained_transform_targets(&self.transform_roots()).map_err(error)?;
        } else if self.transform_roots().len() > 1 {
            return Err("Clear the pixel selection to transform several layers".into());
        } else if self.engine.document().affine_edit_transform(self.engine.document().active_target()).is_none() {
            return Err(self.localization().text(MessageId::COMMANDS_APPLY_TRANSFORM_BEFORE_EDITING).to_string());
        } else if !self.can_transform() {
            return Err("Select unlocked paint content or a layer mask".into());
        }
        if self.operation.active() {
            return Ok(());
        }
        self.cancel_layer_gesture()?;
        if self.measured_target_bounds().is_none() {
            return self.request_content_bounds(super::image_geometry::ContentUse::Transform);
        }
        if !self.engine.document().active_mask
            && self.engine.document().selection.is_none()
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
            (!doc.active_mask && doc.retained_transform_targets(&self.transform_roots()).is_err())
                .then_some("The selected layers cannot be moved together")
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
        let snapping = self.transform_snapping();
        if !self.moves_selected_pixels() {
            self.begin_retained_placement(None, true)?;
            let t = self.operation.current.as_mut().unwrap();
            let press = t.basis.inverse().ok_or("Invalid layer placement")?.map(p);
            t.drag = Some(Drag { handle: Handle::Move, press, current: press, start: t.geometry.clone(), anchor: t.reference_point(), snapping });
            self.layer_interaction.path = vec![press];
            return self.update_transform();
        }
        if self.measured_target_bounds().is_none() {
            self.content_bounds.moving = Some(super::image_geometry::PendingMove { press: p, latest: None, keep_source });
            let result = self.request_content_bounds(super::image_geometry::ContentUse::Move);
            if result.is_err() { self.content_bounds.moving = None; }
            return result;
        }
        let mut t = self.pixel_transaction()?;
        let press = t.basis.inverse().ok_or("Invalid layer placement")?.map(p);
        t.pixel_move = true;
        t.keep_source = keep_source;
        t.drag = Some(Drag { handle: Handle::Move, press, current: press, start: t.geometry.clone(), anchor: t.reference_point(), snapping });
        self.operation.serial = t.request.transaction;
        self.operation.current = Some(t);
        self.layer_interaction.path = vec![press];
        self.update_transform()
    }
    /// A transform of the active target's pixels, bounded by the selection.
    fn pixel_transaction(&self) -> Result<Transaction, String> {
        let doc = self.engine.document();
        let target = doc.active_target();
        let basis = doc.affine_edit_transform(target).ok_or("Apply Transform to Pixels before editing this layer")?;
        let inverse = basis.inverse().ok_or("Invalid layer placement")?;
        let selection = doc.selection.as_ref().map(|s| s.transformed(inverse)).transpose().map_err(error)?;
        let serial = self.operation.serial.wrapping_add(1);
        let mut t = Transaction::new(serial, target, selection, doc.revision, basis, self.measured_target_bounds().ok_or("The content bounds are still being measured")?, Pose::identity());
        t.geometry.interpolation = self.operation.interpolation;
        t.start = t.geometry.clone();
        t.accepted = t.geometry.clone();
        let bounds = &mut t.bounds;
        if bounds.is_empty() {
            return Err("The selection does not overlap this layer".into());
        }
        bounds.max.x = bounds.max.x.max(bounds.min.x + 1.);
        bounds.max.y = bounds.max.y.max(bounds.min.y + 1.);
        t.geometry.frame = t.bounds;
        Ok(t)
    }
    pub(super) fn outline_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
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
            None => Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST)),
            Some(s) if empty(s) => Some(l.text(MessageId::COMMANDS_REFUSAL_OPERATION_THE_SELECTION_IS_EMPTY)),
            Some(_) => None,
        }
    }
    /// Transform the selection's placement only, in document space, as the
    /// selection display previews it.
    pub(super) fn begin_outline_transform(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.outline_refusal())?;
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
        if !apply && self.cancel_content_bounds() { return Ok(()); }
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
        if self.cancel_content_bounds() { return Ok(true); }
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
        self.operation.nudging = None;
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
        let request = self.operation.current.as_ref().map(|t| t.request.clone());
        let result = self.preview_transform();
        if let Some(t) = &mut self.operation.current {
            if result.is_ok() { t.accepted = t.geometry.clone(); }
            else {
                t.geometry = t.accepted.clone();
                t.request = request.unwrap();
            }
        }
        result
    }

    fn preview_transform(&mut self) -> Result<(), String> {
        let Some(t) = &mut self.operation.current else {
            return Ok(());
        };
        let transform = ImageTransform {
            placement: t.map().ok_or("Invalid transform")?,
            keep_source: t.keep_source,
            source_from_owner: None,
        };
        if transform != t.request.transform && self.region_tools.applying_transform() {
            self.region_tools.cancel();
        }
        t.request.transform = transform;
        let moving = t.drag.is_some();
        t.request.moving = moving;
        let placing = t.placement.is_some();
        if t.outline.is_some() {
            self.sync_selection_overlay();
        } else if let Some(placement) = &t.placement {
            let mut edits = Vec::new();
            for layer in placement.preview_layers(self.engine.document(), &t.request.transform.placement, t.geometry.interpolation)? {
                if self.engine.document().is_locked(layer.id) {
                    return Err("The destination layer is locked".into());
                }
                if self.engine.document().layer(layer.id) != Some(&layer) {
                    edits.push(layer_core::Edit::ReplaceLayer(Box::new(layer)));
                }
            }
            if !edits.is_empty() {
                let edit = layer_core::Edit::Batch(edits);
                let mut candidate = self.engine.document().clone();
                candidate.apply(edit.clone()).map_err(error)?;
                candidate.validate_paint_extents(&placement.members.iter().map(|l| l.id).collect::<Vec<_>>(),
                    self.engine.geometry_limits()).map_err(error)?;
                self.engine.preview_edit(edit).map_err(error)?;
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
        let localizer = self.localization();
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
        let position = t.basis.map(t.reference_point());
        [
            (
                "transform_x",
                MessageId::TOOL_CONTROL_TRANSFORM_X,
                MessageId::TOOL_CONTROL_GROUP_POSITION,
                pixels(t.bounds.max.x - t.bounds.min.x),
                position.x,
            ),
            (
                "transform_y",
                MessageId::TOOL_CONTROL_TRANSFORM_Y,
                MessageId::TOOL_CONTROL_GROUP_POSITION,
                pixels(t.bounds.max.y - t.bounds.min.y),
                position.y,
            ),
            (
                "transform_width",
                MessageId::TOOL_CONTROL_TRANSFORM_WIDTH,
                MessageId::TOOL_CONTROL_GROUP_SCALE,
                percent(),
                pose.scale[0],
            ),
            (
                "transform_height",
                MessageId::TOOL_CONTROL_TRANSFORM_HEIGHT,
                MessageId::TOOL_CONTROL_GROUP_SCALE,
                percent(),
                pose.scale[1],
            ),
            (
                "transform_angle",
                MessageId::TOOL_CONTROL_TRANSFORM_ANGLE,
                MessageId::TOOL_CONTROL_GROUP_ROTATION,
                degrees(std::f64::consts::PI),
                pose.angle,
            ),
            (
                "transform_skew",
                MessageId::TOOL_CONTROL_TRANSFORM_SKEW,
                MessageId::TOOL_CONTROL_GROUP_SKEW,
                degrees(f64::from(MAX_SKEW)),
                pose.shear.atan(),
            ),
        ]
        .into_iter()
        .map(
            |(id, label, group, numeric, value)| tool_settings::ToolSetting {
                id,
                label: localizer.text(label),
                label_id: label,
                group: localizer.text(group),
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
        control.numeric.validate(value, control.label.as_ref()).map_err(|reason| reason.message(self.localization()))?;
        let t = self.operation.current.as_mut().ok_or("No transform")?;
        let mut pose = t.geometry.pose;
        let pivot = t.pivot();
        match id {
            "transform_x" | "transform_y" => {
                let point = t.reference_point();
                let mut document = t.basis.map(point);
                if id == "transform_x" { document.x = value; } else { document.y = value; }
                let local = t.basis.inverse().ok_or("Invalid transform basis")?.map(document);
                pose.offset = add(pose.offset, sub(local, point));
            }
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
        if !matches!(id, "transform_x" | "transform_y") { t.keep_pivot(pivot); }
        self.update_transform()
    }
    pub(super) fn finish_transform_nudge(&mut self, apply: bool) -> Result<bool, String> {
        if self.operation.nudging.take().is_none() { return Ok(false); }
        if self.operation.moving_layer() { self.finish_layer_placement(apply)?; }
        Ok(true)
    }
    pub(super) fn transform_nudge(&mut self, key: &str, pressed: bool, allowed: bool, modifiers: Modifiers) -> Result<bool, String> {
        if !pressed {
            return if self.operation.nudging.as_deref() == Some(key) { self.finish_transform_nudge(true) } else { Ok(false) };
        }
        if allowed && key == "escape" && self.operation.nudging.is_some() {
            if self.operation.moving_layer() { return self.finish_transform_nudge(false); }
            self.operation.nudging = None;
            return Ok(false);
        }
        if !allowed || modifiers.command || modifiers.alt || self.operation.dragging() || self.cropping() { return Ok(false); }
        let step = if modifiers.shift { 10. } else { 1. };
        let delta = match key {
            "arrowleft" => Point { x: -step, y: 0. }, "arrowright" => Point { x: step, y: 0. },
            "arrowup" => Point { x: 0., y: -step }, "arrowdown" => Point { x: 0., y: step },
            _ => return Ok(false),
        };
        if self.operation.current.is_none() && (self.layer_interaction.tool != LayerCanvasTool::Move || !self.retained_transforming()) {
            return Ok(false);
        }
        self.require_idle()?;
        if self.operation.nudging.as_deref().is_some_and(|held| held != key) { self.finish_transform_nudge(true)?; }
        if self.operation.current.is_none() { self.begin_retained_placement(None, true)?; }
        let t = self.operation.current.as_mut().unwrap();
        let inverse = t.basis.inverse().ok_or("Invalid transform basis")?;
        let delta = sub(inverse.map(delta), inverse.map(Point::default()));
        t.geometry.pose.offset = add(t.geometry.pose.offset, delta);
        self.operation.nudging = Some(key.into());
        self.update_transform()?;
        Ok(true)
    }
    pub(super) fn transform_again_refusal(&self) -> Option<Arc<str>> {
        let l = self.localization();
        if self.operation.last_transform.is_none() { return Some(l.text(MessageId::COMMANDS_TRANSFORM_AGAIN_EMPTY)); }
        if !self.retained_transforming() { return Some(l.text(MessageId::COMMANDS_TRANSFORM_AGAIN_WHOLE_LAYER)); }
        self.engine.document().retained_transform_targets(&self.transform_roots()).err().map(|reason| reason.to_string().into())
    }
    pub(super) fn transform_again(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.transform_again_refusal())?;
        let delta = self.operation.last_transform.unwrap();
        let roots = self.transform_roots();
        let mut candidate = self.engine.document().clone();
        let targets = candidate.retained_transform_targets(&roots).map_err(error)?;
        let edit = candidate.retained_transform_edit(&roots, delta).map_err(error)?;
        candidate.apply(edit.clone()).map_err(error)?;
        let mut edits = vec![edit];
        edits.extend(candidate.paint_extent_plan(&targets, self.engine.geometry_limits()).map_err(error)?);
        self.layer_edit(layer_core::Edit::Batch(edits))
    }
    pub(super) fn set_transform_reference(&mut self, reference: CanvasAnchor) -> Result<(), String> {
        let t = self.operation.current.as_mut().ok_or("Start a transform first")?;
        t.reference = reference;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn transform_extra(&self) -> Vec<ToolOption> {
        let Some(t) = self.operation.current.as_ref().filter(|t| !t.pixel_move && !t.retained_move && t.mode != TransformMode::Warp) else { return Vec::new(); };
        vec![ToolOption::Choice { id: "transform-reference", label: self.localization().text(MessageId::TOOLS_TRANSFORM_REFERENCE),
            segmented: true, columns: Some(3), beside: Some("transform_x"), items: CanvasAnchor::ALL.into_iter().map(|reference| ToolSetItem { enabled: true,
                label: reference.localized_label(self.localization()),
                icon: "ellipse-fill", preview: None, selected: t.reference == reference,
                action: UiAction::TransformReference { reference },
            }).collect() }]
    }
    pub(super) fn transform_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        if event.phase == PenPhase::Down { self.finish_transform_nudge(true)?; }
        let reach = self.ruler_reach();
        let snapping = (event.phase == PenPhase::Down).then(|| self.transform_snapping()).flatten();
        let Some(t) = &mut self.operation.current else {
            return Ok(());
        };
        let pixel_move = t.pixel_move;
        let retained_move = t.retained_move;
        let p = t.basis.inverse().ok_or("Invalid layer placement")?.map(p);
        if t.split.is_some() {
            t.split_hover(Some(p));
            if event.phase == PenPhase::Up { t.insert_split(); self.update_transform()?; self.refresh_tools(); }
            else if event.phase == PenPhase::Cancel { t.split = None; }
            self.operation.changed = true;
            return Ok(());
        }
        match event.phase {
            PenPhase::Down => {
                if let Some(handle) = t.hit(p, reach) {
                    if let Handle::Node(node) = handle {
                        let toggle = t.select_points || self.interaction.modifiers.shift;
                        if toggle && t.geometry.nodes.remove(&node) {
                            t.node = t.geometry.nodes.last().copied();
                            self.operation.changed = true;
                            return Ok(());
                        }
                        if !toggle && !t.geometry.nodes.contains(&node) { t.geometry.nodes.clear(); }
                        t.geometry.nodes.insert(node);
                        t.node = Some(node);
                    }
                    t.drag = Some(Drag {
                        handle,
                        press: p,
                        current: p,
                        start: t.geometry.clone(),
                        anchor: t.handles(reach).into_iter().find(|(candidate, _)| *candidate == handle).map_or(t.reference_point(), |(_, point)| point),
                        snapping,
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
                t.apply_drag(drag, p, self.interaction.modifiers, self.operation.aspect, reach / 12.);
                if event.phase == PenPhase::Up {
                    t.drag = None;
                    self.layer_interaction.path.clear();
                }
                let update = self.update_transform();
                if retained_move && event.phase == PenPhase::Up {
                    if let Err(cause) = update { self.notify(cause); }
                    return self.finish_layer_placement(true);
                }
                if let Err(cause) = update { self.notify(cause); }
                if pixel_move && event.phase == PenPhase::Up {
                    self.engine.commit_transform(None).map_err(error)?;
                    self.cancel_transform()?;
                }
            }
            PenPhase::Cancel if pixel_move || retained_move => {
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
        let pivot = t.pivot();
        let pose = &mut t.geometry.pose;
        if flip != [1., 1.] {
            pose.angle = -pose.angle;
            pose.shear = -pose.shear;
            pose.scale = [pose.scale[0] * flip[0], pose.scale[1] * flip[1]];
        } else {
            let pi = std::f32::consts::PI;
            pose.angle = (pose.angle + turn + pi).rem_euclid(std::f32::consts::TAU) - pi;
        }
        t.keep_pivot(pivot);
        self.update_transform()
    }
    pub(super) fn cancel_transform_drag(&mut self) -> Result<bool, String> {
        if self.cancel_content_bounds() { return Ok(true); }
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
                let inverse = doc.affine_edit_transform(target)?.inverse()?;
                Some((target, doc.selection.as_ref()?.transformed(inverse).ok()?))
            })
            .flatten();
        if self.operation.moving_pixels != next {
            self.engine.backend_mut().prepare_moving_pixels(next.clone());
            self.operation.moving_pixels = next;
            if self.operation.moving_pixels.is_some() && self.measured_target_bounds().is_none()
                && !self.content_bounds.busy() {
                let _ = self.request_content_bounds(super::image_geometry::ContentUse::PrepareMove);
            }
        }
    }
    /// Move drags the selected pixels rather than the whole layer.
    pub(super) fn moves_selected_pixels(&self) -> bool {
        self.layer_interaction.tool == LayerCanvasTool::Move
            && self.engine.document().selection.is_some()
            && self.selection_masks.target().is_none()
    }
    pub(super) fn update_transform_drag(&mut self) -> Result<bool, String> {
        let units = self.ruler_reach() / 12.;
        let Some(t) = &mut self.operation.current else {
            return Ok(false);
        };
        let Some(drag) = t.drag.clone() else {
            return Ok(false);
        };
        let current = drag.current;
        t.apply_drag(drag, current, self.interaction.modifiers, self.operation.aspect, units);
        self.update_transform()?;
        Ok(true)
    }
    pub(super) fn set_transform_mode(&mut self, mode: TransformMode, uniform: bool) -> Result<(), String> {
        self.require_idle()?;
        let t = self.operation.current.as_mut().ok_or("Start a transform first")?;
        if mode != TransformMode::Free && t.outline.is_some() {
            return Err(self.localization().text(OUTLINE_AFFINE).to_string());
        }
        if mode == TransformMode::Warp && t.placement.as_ref().is_some_and(|p| !p.single_leaf()) {
            return Err(self.localization().text(MessageId::COMMANDS_TRANSFORM_SINGLE_WARP).to_string());
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
        self.operation.current.as_ref().filter(|t| t.mode == TransformMode::Warp)
            .and_then(|t| t.geometry.mesh.as_ref()).map(|mesh| mesh.cells())
    }
    pub(super) fn warp_grid_selected(&self, cells: [u16; 2]) -> bool {
        self.operation.current.as_ref().filter(|t| t.mode == TransformMode::Warp)
            .and_then(|t| t.geometry.mesh.as_ref()).is_some_and(|mesh| mesh.cells() == cells
                && (0..2).all(|axis| mesh.breakpoints[axis].iter().enumerate().all(|(i, value)| *value == i as f32 / f32::from(cells[axis]))))
    }
    pub(super) fn warp_grid_available(&self, cells: [u16; 2]) -> bool {
        self.operation.current.as_ref().filter(|t| t.mode == TransformMode::Warp)
            .and_then(|t| t.geometry.mesh.as_ref())
            .is_some_and(|mesh| mesh.can_refine(cells) || mesh.is_identity())
    }
    pub(super) fn set_warp_cells(&mut self, cells: [u16; 2]) -> Result<(), String> {
        self.require_idle()?;
        let t = self
            .operation
            .current
            .as_mut()
            .filter(|t| t.mode == TransformMode::Warp)
            .ok_or("Choose Warp first")?;
        if !t.set_cells(cells) {
            return Err(self.localization().text(MessageId::COMMANDS_WARP_RESET_GRID).to_string());
        }
        self.update_transform()
    }
    pub(super) fn warp_split_axes(command: CommandId) -> Option<[bool; 2]> {
        Some(match command { CommandId::WarpSplitVertical => [true, false],
            CommandId::WarpSplitHorizontal => [false, true], CommandId::WarpSplitCross => [true, true], _ => return None })
    }
    pub(super) fn warp_command_available(&self, command: CommandId) -> bool {
        let Some(cells) = self.warp_cells() else { return false; };
        Self::warp_split_axes(command).is_none_or(|axes| (0..2).all(|i| !axes[i] || cells[i] < MeshMap::MAX_CELLS))
    }
    pub(super) fn warp_command_selected(&self, command: CommandId) -> bool {
        self.operation.current.as_ref().is_some_and(|t| if command == CommandId::WarpSelectPoints { t.select_points }
            else { Self::warp_split_axes(command).is_some_and(|axes| t.split.as_ref().is_some_and(|s| s.axes == axes)) })
    }
    pub(super) fn warp_command(&mut self, command: CommandId) -> Result<(), String> {
        self.require_idle()?;
        if !self.warp_command_available(command) { return Err("Choose a warp grid with room to split".into()); }
        let t = self.operation.current.as_mut().unwrap();
        let mesh = t.geometry.mesh.as_ref().unwrap();
        if let Some(axes) = Self::warp_split_axes(command) {
            let tolerance = 0.5 / t.outer().ok_or("Invalid warp")?.magnification(mesh.drawn_bounds()).max(1e-6);
            t.split = Some(WarpSplit { axes, surface: mesh.tessellate(tolerance), hover: None });
        } else if command == CommandId::WarpSelectPoints { t.select_points = !t.select_points; t.split = None; }
        else if command == CommandId::WarpResetGrid {
            t.geometry.mesh = MeshMap::identity(t.source, mesh.cells()).map(Arc::new);
            t.geometry.nodes.clear(); t.node = None; t.split = None;
            self.update_transform()?;
        }
        self.operation.changed = true;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn cancel_warp_split(&mut self) -> bool {
        let cancelled = self.operation.current.as_mut().is_some_and(|t| t.split.take().is_some());
        if cancelled { self.operation.changed = true; self.refresh_tools(); }
        cancelled
    }
    pub(super) fn warp_hover(&mut self, point: Option<Point>) {
        if let Some(t) = self.operation.current.as_mut().filter(|t| t.split.is_some()) {
            t.split_hover(point.and_then(|p| t.basis.inverse().map(|map| map.map(p))));
        }
    }
    pub(super) fn transform_interpolation(&self) -> Option<Interpolation> {
        self.operation
            .current
            .as_ref()
            .filter(|t| t.outline.is_none())
            .and_then(|t| if t.placement.as_ref().is_some_and(|p| !p.single_leaf()) { t.geometry.interpolation } else { Some(t.interpolation()) })
    }
    pub(super) fn set_transform_interpolation(&mut self, interpolation: Interpolation) -> Result<(), String> {
        self.require_idle()?;
        let t = self.operation.current.as_ref().ok_or("Start a transform first")?;
        if t.outline.is_some() {
            return Err(self.localization().text(OUTLINE_PIXELS).to_string());
        }
        self.operation.current.as_mut().unwrap().geometry.interpolation = Some(interpolation);
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
        let Some(t) = self.operation.current.as_ref() else { return; };
        if let Some(snapping) = t.drag.as_ref().and_then(|drag| drag.snapping.as_ref()) {
            let map = self.document_to_logical();
            segments.extend(snapping.guides.iter().map(|[from, to]| CursorSegment {
                from: map(*from), to: map(*to), distance: 0., marker: 0., scale: 1.,
            }));
        }
        if t.retained_move || t.pixel_move { return; }
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
            let [columns, rows] = mesh.cells();
            let along = |from: Point, to: Point, cells: u16| -> Vec<[f32; 2]> {
                let steps = 8 * usize::from(cells);
                (0..=steps)
                    .filter_map(|s| {
                        let f = s as f32 / steps as f32;
                        mesh.map(mesh.frame.map(Point { x: from.x + (to.x - from.x) * f, y: from.y + (to.y - from.y) * f }))
                    })
                    .filter_map(|p| t.outer()?.map(p))
                    .map(&map)
                    .collect()
            };
            for &u in mesh.breakpoints[0].iter() {
                for pair in along(Point { x: u, y: 0. }, Point { x: u, y: 1. }, rows).windows(2) {
                    line(pair[0], pair[1], true);
                }
            }
            for &v in mesh.breakpoints[1].iter() {
                for pair in along(Point { x: 0., y: v }, Point { x: 1., y: v }, columns).windows(2) {
                    line(pair[0], pair[1], true);
                }
            }
            if let Some(split) = &t.split && let Some(p) = split.hover {
                let curves = [along(Point { x: p.x, y: 0. }, Point { x: p.x, y: 1. }, rows),
                    along(Point { x: 0., y: p.y }, Point { x: 1., y: p.y }, columns)];
                for (curve, enabled) in curves.iter().zip(split.axes) {
                    if enabled { for pair in curve.windows(2) { line(pair[0], pair[1], false); } }
                }
            }
            if let Some((node, at)) = t.node.and_then(|node| mesh.node(node).map(|p| (node, p))) {
                for side in 0..4 {
                    if let Some(outer) = t.outer()
                        && let Some(a) = outer.map(at)
                        && let Some(b) = mesh.tangent(node, side).and_then(|p| outer.map(p)) {
                        line(map(a), map(b), true);
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
        for (handle, p) in t.handles(reach) {
            let [x, y] = map(p);
            let half = HANDLE_HALF_SIZE + f32::from(matches!(handle, Handle::Node(node) if t.geometry.nodes.contains(&node))) * 2.;
            segments.push(CursorSegment {
                from: [x - half, y - half],
                to: [x + half, y + half],
                distance: 0.,
                marker: if matches!(handle, Handle::Pivot) { 5. } else { 2. },
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
        let geometry = Geometry { pose, exact_affine: None, inner: None, mesh: None, frame: bounds, pivot: center(bounds), interpolation: None, nodes: BTreeSet::new() };
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
            geometry: geometry.clone(),
            accepted: geometry.clone(),
            node: None,
            select_points: false,
            split: None,
            mode: TransformMode::Free,
            perspective: false,
            start: geometry,
            source: bounds,
            drag: None,
            outline: None,
            pixel_move: false,
            retained_move: false,
            keep_source: false,
            reference: CanvasAnchor::Center,
        }
    }
    fn pose_affine(&self) -> Affine {
        self.geometry.pose.affine(center(self.geometry.frame))
    }
    fn pivot(&self) -> Point { self.pose_affine().map(self.geometry.pivot) }
    fn keep_pivot(&mut self, position: Point) {
        self.geometry.pose.offset = add(self.geometry.pose.offset, sub(position, self.pivot()));
    }
    fn reference_point(&self) -> Point {
        let [x, y] = self.reference.cell().map(|v| v as f32 * 0.5);
        let q = self.corners();
        let mix = |a: Point, b: Point, v: f32| Point { x: a.x + (b.x - a.x) * v, y: a.y + (b.y - a.y) * v };
        mix(mix(q[0], q[1], x), mix(q[3], q[2], x), y)
    }
    fn outer_bounds(&self) -> Rect {
        self.mapped_mesh().map_or(self.bounds, |mesh| mesh.bounds())
    }
    fn mapped_mesh(&self) -> Option<&Arc<MeshMap>> {
        self.geometry.mesh.as_ref().filter(|mesh| !(self.placement.is_some()
            && self.start.mesh.is_none() && mesh.cells() == MeshMap::PRESETS[0]
            && mesh.can_refine(MeshMap::PRESETS[0]) && mesh.is_identity()))
    }
    fn outer(&self) -> Option<Projective> { self.geometry.outer() }
    fn map(&self) -> Option<LayerPlacement> {
        let outer = if self.pixel_move {
            let offset = self.geometry.pose.offset;
            Projective::from_affine(Affine::translation(Point { x: offset.x.round(), y: offset.y.round() }))
        } else { self.outer()? };
        Some(LayerPlacement { outer, mesh: self.mapped_mesh().cloned(), interpolation: self.interpolation() })
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
                self.quad()
            }
        }
    }
    fn handles(&self, reach: f32) -> Vec<(Handle, Point)> {
        let mut handles: Vec<(Handle, Point)> = match self.mode {
            TransformMode::Warp => {
                let Some(mesh) = &self.geometry.mesh else {
                    return Vec::new();
                };
                let tangents = self.node.into_iter().flat_map(|node| {
                    (0..4u8).filter_map(move |side| self.outer()?.map(mesh.tangent(node, side)?).map(|p| (Handle::Tangent(node, side), p)))
                });
                tangents
                    .chain((0..mesh.node_count()).filter_map(|node| self.outer()?.map(mesh.node(node)?).map(|p| (Handle::Node(node), p))))
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
        };
        if self.mode != TransformMode::Warp { handles.push((Handle::Pivot, self.pivot())); }
        handles
    }
    fn fold(&mut self) {
        let pivot = self.pivot();
        let Some(inner) = self.outer() else { return; };
        let bounds = self.outer_bounds();
        if self.mapped_mesh().is_none() && let Some(pose) = inner.as_affine().and_then(|map| Pose::from_affine(map, center(bounds))) {
            self.geometry.frame = bounds;
            self.geometry.pose = pose;
            self.geometry.inner = None;
            self.geometry.exact_affine = Some((pose, inner));
            self.geometry.pivot = self.pose_affine().inverse().map_or(pivot, |map| map.map(pivot));
            return;
        }
        self.geometry.inner = Some(inner);
        self.geometry.exact_affine = None;
        self.geometry.pose = Pose::identity();
        self.geometry.frame = inner.bounds(bounds).unwrap_or(bounds);
        self.geometry.pivot = pivot;
    }
    fn enter_warp(&mut self) {
        if self.geometry.mesh.is_none() {
            self.geometry.mesh = MeshMap::identity(self.source, MeshMap::PRESETS[0]).map(Arc::new);
        }
    }
    fn set_cells(&mut self, cells: [u16; 2]) -> bool {
        let Some(mesh) = self.geometry.mesh.as_deref().and_then(|mesh| mesh.refine(cells).or_else(|| {
            mesh.is_identity().then(|| MeshMap::identity(self.source, cells)).flatten()
        })) else { return false; };
        self.node = None;
        self.geometry.nodes.clear();
        self.geometry.mesh = Some(Arc::new(mesh));
        true
    }
    fn split_hover(&mut self, point: Option<Point>) {
        let Some(inverse) = self.outer().and_then(Projective::inverse) else { return; };
        let Some(mesh) = &self.geometry.mesh else { return; };
        let Some(split) = &mut self.split else { return; };
        split.hover = point.and_then(|p| inverse.map(p)).and_then(|p| split.surface.source_at(p))
            .and_then(|p| mesh.frame.inverse().map(|map| map.map(p)));
    }
    fn insert_split(&mut self) {
        let Some(split) = &self.split else { return; };
        let Some(unit) = split.hover else { return; };
        let Some(original) = &self.geometry.mesh else { return; };
        let mut mesh = (**original).clone();
        let mut nodes = self.geometry.nodes.clone();
        let mut active = self.node;
        for (axis, enabled) in split.axes.into_iter().enumerate() {
            if !enabled { continue; }
            let value = [unit.x, unit.y][axis];
            let at = mesh.breakpoints[axis].partition_point(|p| *p < value) as u32;
            let width = u32::from(mesh.cells()[0]) + 1;
            let Some(next) = mesh.split(axis, value) else { return; };
            let remap = |node: u32| {
                let mut p = [node % width, node / width];
                p[axis] += u32::from(p[axis] >= at);
                p[1] * (u32::from(next.cells()[0]) + 1) + p[0]
            };
            nodes = nodes.into_iter().map(remap).collect();
            active = active.map(remap);
            mesh = next;
        }
        self.geometry.mesh = Some(Arc::new(mesh));
        self.geometry.nodes = nodes; self.node = active; self.split = None;
    }
    fn interpolation(&self) -> Interpolation {
        if self.pixel_move { return Interpolation::Nearest; }
        self.geometry.interpolation.unwrap_or(match self.mode {
            TransformMode::Distort | TransformMode::Warp => Interpolation::Bicubic,
            TransformMode::Free => Interpolation::Linear,
        })
    }
    fn set_mode(&mut self, mode: TransformMode) {
        if mode == self.mode { return; }
        match mode {
            TransformMode::Warp => self.enter_warp(),
            TransformMode::Distort | TransformMode::Free => self.fold(),
        }
        self.mode = mode;
        self.split = None;
    }
    fn reset(&mut self) {
        if let Some(placement) = &mut self.placement {
            placement.reset();
            if !placement.single_leaf() { self.bounds = self.start.frame; }
        }
        self.geometry = self.start.clone();
        self.node = self.geometry.nodes.last().copied();
        self.split = None;
        self.select_points = false;
        self.mode = TransformMode::Free;
    }
    fn apply_drag(&mut self, mut drag: Drag, p: Point, modifiers: Modifiers, aspect: bool, units: f32) {
        let snapping = drag.snapping.take();
        self.apply_geometry_drag(drag.clone(), p, modifiers, aspect);
        if let Some(mut snapping) = snapping {
            if !matches!(drag.handle, Handle::Rotate) {
                let bounds = if matches!(drag.handle, Handle::Move) { Rect::around(self.corners().map(|p| self.basis.map(p))) }
                    else {
                        let point = self.handles(units * 12.).into_iter().find(|(handle, _)| *handle == drag.handle)
                            .map_or(p, |(_, point)| point);
                        let point = self.basis.map(point);
                        Rect { min: point, max: point }
                    };
                let direction = match drag.handle {
                    Handle::Move if self.mode != TransformMode::Distort && modifiers.shift => Some(sub(p, drag.press)),
                    Handle::Corner(_) if self.perspective || modifiers.shift => Some(sub(p, drag.press)),
                    _ => None,
                }.map(|delta| if delta.x.abs() >= delta.y.abs() { Point { x: 1., y: 0. } } else { Point { x: 0., y: 1. } })
                    .or_else(|| match drag.handle {
                        Handle::Scale(side) => {
                            let moving = local_handle(drag.start.frame, side);
                            let fixed = if modifiers.alt { drag.start.pivot } else { local_handle(drag.start.frame, side.map(|value| -value)) };
                            let direction = if modifiers.command && (side[0] == 0. || side[1] == 0.) {
                                Some(if side[0] == 0. { Point { x: 1., y: 0. } } else { Point { x: 0., y: 1. } })
                            } else if aspect || modifiers.shift { Some(sub(moving, fixed)) }
                            else if side[0] == 0. { Some(Point { x: 0., y: moving.y - fixed.y }) }
                            else if side[1] == 0. { Some(Point { x: moving.x - fixed.x, y: 0. }) }
                            else { None };
                            direction.map(|direction| drag.start.pose.map_linear(direction))
                        }
                        _ => None,
                    }).map(|direction| sub(self.basis.map(direction), self.basis.map(Point::default())));
                let delta = snapping.correction(bounds, self.basis.map(drag.anchor), direction, units);
                if delta != Point::default() && let Some(inverse) = self.basis.inverse() {
                    let delta = sub(inverse.map(delta), inverse.map(Point::default()));
                    let pointer = match drag.handle {
                        Handle::Scale(side) => {
                            let start = drag.start.pose.affine(center(drag.start.frame)).map(local_handle(drag.start.frame, side));
                            let current = self.pose_affine().map(local_handle(self.geometry.frame, side));
                            add(drag.press, sub(current, start))
                        }
                        Handle::Move if self.mode != TransformMode::Distort && modifiers.shift => {
                            let delta = sub(p, drag.press);
                            add(drag.press, if delta.x.abs() >= delta.y.abs() { Point { x: delta.x, y: 0. } } else { Point { x: 0., y: delta.y } })
                        }
                        Handle::Corner(_) if self.perspective || modifiers.shift => {
                            let delta = sub(p, drag.press);
                            add(drag.press, if delta.x.abs() >= delta.y.abs() { Point { x: delta.x, y: 0. } } else { Point { x: 0., y: delta.y } })
                        }
                        _ => p,
                    };
                    self.apply_geometry_drag(drag.clone(), add(pointer, delta), modifiers, aspect);
                }
                let actual = if matches!(drag.handle, Handle::Move) { Rect::around(self.corners().map(|point| self.basis.map(point))) }
                    else { let point = self.handles(units * 12.).into_iter().find(|(handle, _)| *handle == drag.handle)
                        .map_or(p, |(_, point)| point); let point = self.basis.map(point); Rect { min: point, max: point } };
                snapping.retain_guides(actual, units);
            }
            if let Some(drag) = &mut self.drag { drag.snapping = Some(snapping); }
        }
    }
    fn apply_geometry_drag(&mut self, drag: Drag, p: Point, modifiers: Modifiers, aspect: bool) {
        if p == drag.press {
            self.geometry = drag.start;
            return;
        }
        if matches!(drag.handle, Handle::Pivot) {
            if let Some(inverse) = drag.start.pose.affine(center(drag.start.frame)).inverse() {
                self.geometry.pivot = add(drag.start.pivot, sub(inverse.map(p), inverse.map(drag.press)));
            }
            return;
        }
        let inner_delta = || {
            let outer = drag.start.inner.unwrap_or(Projective::IDENTITY)
                .then(Projective::from_affine(drag.start.pose.affine(center(drag.start.frame))))?;
            let inverse = outer.inverse()?;
            Some(sub(inverse.map(p)?, inverse.map(drag.press)?))
        };
        let warped = match (drag.handle, drag.start.mesh.as_deref()) {
            (Handle::Node(_), Some(mesh)) => Some(inner_delta().and_then(|delta| mesh.move_nodes(&drag.start.nodes, delta))),
            (Handle::Tangent(node, side), Some(mesh)) => Some(inner_delta().and_then(|delta|
                mesh.tangent(node, side).and_then(|t| mesh.move_tangent(node, side, add(t, delta))))),
            _ => None,
        };
        if let Some(mesh) = warped {
            if let Some(mesh) = mesh.filter(MeshMap::valid) {
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
            Handle::Scale(_) | Handle::Rotate | Handle::Node(_) | Handle::Tangent(..) | Handle::Pivot => return drag.start.inner,
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
                let c = pose.affine(pivot).map(drag.start.pivot);
                pose.angle +=
                    (p.y - c.y).atan2(p.x - c.x) - (drag.press.y - c.y).atan2(drag.press.x - c.x);
                if modifiers.shift {
                    let step = std::f32::consts::PI / 12.;
                    pose.angle = (pose.angle / step).round() * step;
                }
                pose.angle = (pose.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                pose.offset = add(pose.offset, sub(c, pose.affine(pivot).map(drag.start.pivot)));
            }
            Handle::Scale(side) if modifiers.command && (side[0] == 0. || side[1] == 0.) => {
                let start = pose.affine(pivot);
                let moving = local_handle(self.geometry.frame, side);
                let fixed = if modifiers.alt { drag.start.pivot } else { local_handle(self.geometry.frame, side.map(|v| -v)) };
                let top_bottom = side[0] == 0.;
                let along = pose.map_linear(if top_bottom {
                    Point { x: 1., y: 0. }
                } else {
                    Point { x: 0., y: 1. }
                });
                let delta = sub(p, drag.press);
                let travel = (delta.x * along.x + delta.y * along.y) / (along.x * along.x + along.y * along.y);
                let lever = if top_bottom { moving.y - fixed.y } else { moving.x - fixed.x };
                if lever == 0. { return pose; }
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
            Handle::Corner(_) | Handle::Edge(_) | Handle::Node(_) | Handle::Tangent(..) | Handle::Pivot => {}
            Handle::Scale(side) => {
                let start = pose.affine(pivot);
                let moving = local_handle(self.geometry.frame, side);
                let fixed = if modifiers.alt {
                    drag.start.pivot
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
                let active = [side[0] != 0. && span[0] != 0., side[1] != 0. && span[1] != 0.];
                if !active[0] && !active[1] { return pose; }
                if active[1] {
                    pose.scale[1] = local[1] / span[1];
                }
                if active[0] {
                    pose.scale[0] = (local[0] - pose.shear * pose.scale[1] * span[1]) / span[0];
                }
                if aspect || modifiers.shift {
                    let axis = if !active[0] {
                        1
                    } else if !active[1]
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
    fn alt_scale_and_skew_keep_custom_pivots_under_rotated_reflected_poses() {
        for scale in [[1.3, 0.7], [-1.3, 0.7], [1.3, -0.7]] {
            let mut t = transaction();
            t.geometry.pose = Pose { offset: Point { x: 12., y: -7. }, angle: 0.63, scale, shear: 0.31 };
            t.geometry.pivot = Point { x: 83., y: 112. };
            let center = center(t.geometry.frame);
            let original = t.geometry.pose.affine(center);
            let pivot = original.map(t.geometry.pivot);
            for side in [[1., 1.], [0., 1.], [1., 0.]] {
                let press = original.map(local_handle(t.geometry.frame, side));
                let drag = Drag { handle: Handle::Scale(side), press, current: press, start: t.geometry.clone(), anchor: t.reference_point(), snapping: None };
                for command in [false, true] {
                    let modifiers = Modifiers { alt: true, command, ..Default::default() };
                    let unchanged = t.drag_pose(&drag, press, modifiers, false);
                    near(unchanged.affine(center).map(t.geometry.pivot), pivot);
                    let next = t.drag_pose(&drag, add(press, Point { x: 31., y: 19. }), modifiers, false);
                    near(next.affine(center).map(t.geometry.pivot), pivot);
                    assert!(next.affine(center).inverse().is_some());
                    assert_ne!(next.affine(center), original);
                }
            }
        }
    }
    #[test]
    fn pivot_coincident_with_handle_keeps_zero_span_axes_unchanged() {
        let mut t = transaction();
        t.geometry.pose = Pose { offset: Point { x: 12., y: -7. }, angle: 0.63, scale: [-1.3, 0.7], shear: 0.31 };
        let center = center(t.geometry.frame);
        for (pivot, side) in [(t.geometry.frame.max, [1., 1.]), (local_handle(t.geometry.frame, [1., 0.]), [1., 0.]), (Point { x: t.geometry.frame.max.x, y: 80. }, [1., 1.])] {
            t.geometry.pivot = pivot;
            let original = t.geometry.pose.affine(center);
            let press = original.map(local_handle(t.geometry.frame, side));
            let drag = Drag { handle: Handle::Scale(side), press, current: press, start: t.geometry.clone(), anchor: t.reference_point(), snapping: None };
            for command in [false, true] {
                let next = t.drag_pose(&drag, add(press, Point { x: 31., y: 19. }), Modifiers { alt: true, command, ..Default::default() }, false);
                near(next.affine(center).map(pivot), original.map(pivot));
                assert!(next.affine(center).inverse().is_some());
                if pivot == local_handle(t.geometry.frame, side) {
                    assert_eq!(next.affine(center), original);
                } else if !command {
                    assert_eq!(next.scale[0], t.geometry.pose.scale[0]);
                    assert_ne!(next.scale[1], t.geometry.pose.scale[1]);
                }
            }
        }
    }
    #[test]
    fn changing_reference_point_preserves_geometry_and_custom_pivot() {
        let mut t = transaction();
        t.geometry.pose = Pose { offset: Point { x: 12., y: -7. }, angle: 0.63, scale: [1.3, 0.7], shear: 0.31 };
        t.geometry.pivot = Point { x: 83., y: 112. };
        let original = t.map().unwrap();
        let pivot = t.pivot();
        for reference in CanvasAnchor::ALL {
            t.reference = reference;
            let _ = t.reference_point();
            assert_eq!(t.map().unwrap(), original);
            near(t.pivot(), pivot);
        }
    }
    #[test]
    fn one_axis_alt_scale_snaps_without_following_the_inactive_pivot_offset() {
        let mut t = transaction();
        t.geometry.pivot = Point { x: 80., y: 60. };
        let side = [1., 0.];
        let press = local_handle(t.geometry.frame, side);
        let target = t.basis.map(add(press, Point { x: 8., y: 0. }));
        let snapping = Snapping::new(vec![(layer_core::LayerId(2), Rect { min: target, max: add(target, Point { x: 1000., y: 1000. }) })], vec![]);
        let drag = Drag { handle: Handle::Scale(side), press, current: press, start: t.geometry.clone(), anchor: t.reference_point(), snapping: Some(snapping) };
        t.drag = Some(drag.clone());
        t.apply_drag(drag, add(press, Point { x: 5., y: 0. }), Modifiers { alt: true, ..Default::default() }, false, 1.);
        near(t.pose_affine().map(local_handle(t.geometry.frame, side)), add(press, Point { x: 8., y: 0. }));
        near(t.pose_affine().map(t.geometry.pivot), t.geometry.pivot);
        assert!(!t.drag.as_ref().unwrap().snapping.as_ref().unwrap().guides.is_empty());
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
        let fitted = t.map().expect("valid retained geometry");
        assert!(Arc::ptr_eq(fitted.mesh.as_ref().unwrap(), &warp));
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
        assert!(worst < 0.001, "{worst}px from the perspective of the warp");
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
                let drag = Drag { handle: Handle::Scale(side), press, current: press, start: t.geometry.clone(), anchor: t.reference_point(), snapping: None };
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
                        anchor: t.reference_point(),
                        snapping: None,
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
        assert!(matches!(t.hit(pivot, 12.), Some(Handle::Pivot)));
        assert!(matches!(t.hit(add(pivot, Point { x: 30., y: 20. }), 12.), Some(Handle::Move)));
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
                    anchor: t.reference_point(),
                    snapping: None,
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
                anchor: t.reference_point(),
                snapping: None,
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
