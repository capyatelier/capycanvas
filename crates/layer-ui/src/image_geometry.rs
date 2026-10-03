use super::crop::{CropFrame, CropRatio};
use super::*;
use layer_core::{
    Affine, CanvasGeometry, CanvasGeometryError, CanvasRect, ContentBoundsCache, ContentBoundsRequest, ContentScope, Edit,
    ImageOrientation, Point, Rect,
};

/// Float noise in bounds must not add a pixel of canvas.
const TOLERANCE: f32 = 1e-3;

/// What a content bounds scan is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContentUse {
    Trim,
    RevealAll,
    FitContent,
    Transform,
    Move,
    PrepareMove,
    PrepareSnap(layer_core::LayerId),
}
impl ContentUse {
    fn scope(self, target: layer_core::LayerId) -> ContentScope {
        match self {
            Self::PrepareSnap(id) => ContentScope::PlacedTarget(id),
            Self::Trim => ContentScope::Canvas,
            Self::FitContent => ContentScope::Visible,
            Self::RevealAll => ContentScope::All,
            Self::Transform | Self::Move | Self::PrepareMove => ContentScope::Target(target),
        }
    }
}

pub(super) struct PendingMove {
    pub press: Point,
    pub latest: Option<(layer_engine::PenEvent, Point)>,
    pub keep_source: bool,
}
struct ContentJob {
    purpose: ContentUse,
    revision: u64,
    target: layer_core::LayerId,
    roots: Vec<layer_core::LayerId>,
    request: ContentBoundsRequest,
    submitted: bool,
}

struct PixelBake {
    epoch: u64,
    revision: u64,
    plan: layer_core::TransformPixelsPlan,
    submitted: bool,
}

#[derive(Default)]
pub(super) struct ContentBounds {
    cache: ContentBoundsCache,
    job: Option<ContentJob>,
    bake: Option<PixelBake>,
    pub(super) moving: Option<PendingMove>,
}
impl ContentBounds {
    pub(super) fn busy(&self) -> bool { self.job.is_some() || self.bake.is_some() }
    pub(super) fn baking(&self) -> bool { self.bake.is_some() }
}

pub(super) fn orientation(command: CommandId) -> Option<ImageOrientation> {
    Some(match command {
        CommandId::RotateImageLeft => ImageOrientation::RotateLeft,
        CommandId::RotateImageRight => ImageOrientation::RotateRight,
        CommandId::RotateImage180 => ImageOrientation::Rotate180,
        CommandId::FlipImageHorizontal => ImageOrientation::FlipHorizontal,
        CommandId::FlipImageVertical => ImageOrientation::FlipVertical,
        _ => return None,
    })
}

/// The whole pixels covering `bounds`.
fn pixel_rect(bounds: Rect) -> CanvasRect {
    let min = [bounds.min.x, bounds.min.y].map(|v| (v + TOLERANCE).floor());
    let max = [bounds.max.x, bounds.max.y].map(|v| (v - TOLERANCE).ceil());
    CanvasRect {
        origin: min.map(|v| v as i32),
        size: std::array::from_fn(|axis| (max[axis] - min[axis]).max(1.) as u32),
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Move the canvas in one undo step, with `then` in the same step, and
    /// keep the image still on screen.
    pub(super) fn apply_canvas_geometry(&mut self, geometry: &CanvasGeometry, then: Vec<Edit>) -> Result<(), CanvasGeometryError> {
        if self.operation.placing() {
            return Err(CanvasGeometryError::Unsupported("Apply or cancel the photo placement first"));
        }
        self.engine.apply_canvas_geometry_with(geometry, then)?;
        let to_canvas = geometry.to_canvas();
        if let Some(reselect) = &mut self.selection_masks.reselect {
            *reselect = reselect.transformed(to_canvas)?;
        }
        self.follow_canvas_map(to_canvas);
        Ok(())
    }

    /// Keep the image where it was on screen after the canvas moves through
    /// `to_canvas`: exactly for a crop, and about the view's centre for a turn,
    /// flip or scale.
    fn follow_canvas_map(&mut self, to_canvas: Affine) {
        let camera = &mut self.state.camera;
        let [a, b, c, d, x, y] = to_canvas.0;
        if [a, b, c, d] == [1., 0., 0., 1.] {
            camera.follow_document_origin([-x, -y]);
        } else {
            let [cx, cy] = camera.work_area_center();
            let center = to_canvas.map(camera.input_transform().map(Point { x: cx, y: cy }));
            camera.center_on([center.x, center.y]);
        }
        self.sync_camera();
    }

    pub(super) fn orient_image(&mut self, orientation: ImageOrientation) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.canvas_geometry_refusal())?;
        let doc = self.engine.document();
        let geometry = CanvasGeometry::orient([doc.width, doc.height], orientation);
        self.apply_canvas_geometry(&geometry, Vec::new()).map_err(|e| e.to_string())
    }

    /// Why a content bounds command can't run.
    pub(super) fn content_bounds_refusal(&self, purpose: ContentUse) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        match purpose {
            ContentUse::PrepareSnap(_) => None,
            ContentUse::FitContent => (!self.cropping()).then_some(l.text(MessageId::COMMANDS_CHOOSE_THE_CROP_TOOL_FIRST)),
            ContentUse::Trim | ContentUse::RevealAll => self.canvas_geometry_refusal(),
            ContentUse::Transform | ContentUse::Move | ContentUse::PrepareMove => (!self.can_transform()).then_some(l.text(MessageId::COMMANDS_SELECT_UNLOCKED_PAINT_CONTENT_OR_A_LAYER_MASK)),
        }
    }

    /// Fit Content edits the open crop; Trim and Reveal All edit the document.
    fn require_content_idle(&self, purpose: ContentUse) -> Result<(), String> {
        match purpose {
            ContentUse::FitContent | ContentUse::Transform | ContentUse::Move | ContentUse::PrepareMove | ContentUse::PrepareSnap(_) => self.require_idle(),
            ContentUse::Trim | ContentUse::RevealAll => self.require_document_idle(),
        }
    }

    pub(super) fn request_content_bounds(&mut self, purpose: ContentUse) -> Result<(), String> {
        self.cancel_auto_levels();self.cancel_histogram();
        self.require_content_idle(purpose)?;
        refused(self.content_bounds_refusal(purpose))?;
        let doc = self.engine.document();
        let mut request = ContentBoundsRequest::new(doc, purpose.scope(doc.active_target()));
        if !matches!(request.scope, ContentScope::Target(_)) && doc.has_animated_effects() {
            request.time = self.engine.animation_time();
        }
        if matches!(request.scope, ContentScope::Target(_)) {
            if let Some(bounds) = self.measured_target_bounds() { return self.use_content_bounds(purpose, bounds); }
            if let Some(target) = self.bounds_targets().into_iter().find(|id| {
                let request = ContentBoundsRequest::new(doc, ContentScope::Target(*id));
                self.content_bounds.cache.get(&request).or_else(|| request.known_bounds()).is_none()
            }) { request = ContentBoundsRequest::new(doc, ContentScope::Target(target)); }
        } else if let Some(bounds) = self.content_bounds.cache.get(&request) {
            return self.use_content_bounds(purpose, bounds);
        }
        let revision = doc.revision;
        let target = doc.active_target();
        let roots = if matches!(request.scope, ContentScope::Target(_) | ContentScope::PlacedTarget(_)) { self.transform_roots() } else { Vec::new() };
        if let Some(job) = &mut self.content_bounds.job && job.request == request {
            job.purpose = purpose;
            return Ok(());
        }
        self.engine.backend_mut().cancel_content_bounds();
        let submitted = match self.engine.backend_mut().request_content_bounds(request.clone()) {
            Ok(submitted) => submitted,
            Err(reason) => {
                if matches!(purpose, ContentUse::PrepareSnap(_)) { self.content_bounds.cache.insert(request, Rect::EMPTY); }
                return Err(error(reason));
            }
        };
        self.content_bounds.job = Some(ContentJob { purpose, revision, target, roots, request, submitted });
        Ok(())
    }

    fn bounds_targets(&self) -> Vec<layer_core::LayerId> {
        let doc = self.engine.document();
        if self.retained_transforming() {
            return doc.retained_transform_targets(&self.transform_roots()).unwrap_or_default().into_iter()
                .filter(|id| doc.layer(*id).is_some_and(|layer| layer.kind == layer_core::LayerKind::Paint)).collect();
        }
        std::iter::once(doc.active_target()).chain(self.bounds_companion()).collect()
    }
    fn snap_target_ids(&self) -> Vec<layer_core::LayerId> {
        let doc = self.engine.document();
        let roots = self.transform_roots();
        let mut excluded = doc.layer_subtrees(&roots);
        for root in roots {
            let mut parent = doc.layer(root).and_then(|layer| layer.properties.parent);
            while let Some(id) = parent {
                if !excluded.insert(id) { break; }
                parent = doc.layer(id).and_then(|layer| layer.properties.parent);
            }
        }
        let mut ids: Vec<_> = doc.layers.iter().filter(|layer| {
            if !matches!(layer.kind, layer_core::LayerKind::Paint | layer_core::LayerKind::Group)
                || excluded.contains(&layer.id) { return false; }
            let mut current = Some(*layer);
            while let Some(layer) = current {
                if !layer.visible || layer.properties.clipped || layer.opacity <= 0. { return false; }
                current = layer.properties.parent.and_then(|id| doc.layer(id));
            }
            true
        }).map(|layer| layer.id).collect();
        ids.sort_unstable();
        ids
    }
    pub(super) fn measured_snap_bounds(&self) -> Vec<(layer_core::LayerId, Rect)> {
        let doc = self.engine.document();
        self.snap_target_ids().into_iter().filter_map(|id| {
            self.content_bounds.cache.current(doc, ContentScope::PlacedTarget(id))
                .filter(|bounds| !bounds.is_empty()).map(|bounds| (id, bounds))
        }).collect()
    }
    pub(super) fn prepare_transform_snapping(&mut self) -> Result<(), String> {
        if !self.operation.snapping {
            if self.content_bounds.job.as_ref().is_some_and(|job| matches!(job.purpose, ContentUse::PrepareSnap(_))) {
                self.cancel_content_bounds();
            }
            return Ok(());
        }
        if self.operation.dragging() || self.operation.nudging() || self.content_bounds.busy() || !self.canvas_idle()
            || (!self.operation.transforming() && self.layer_interaction.tool != LayerCanvasTool::Move) { return Ok(()); }
        if self.layer_interaction.tool == LayerCanvasTool::Move && self.retained_transforming()
            && self.measured_target_bounds().is_none() {
            return self.request_content_bounds(ContentUse::PrepareMove);
        }
        let doc = self.engine.document();
        if let Some(id) = self.snap_target_ids().into_iter().find(|id|
            self.content_bounds.cache.current(doc, ContentScope::PlacedTarget(*id)).is_none()) {
            self.request_content_bounds(ContentUse::PrepareSnap(id))?;
        }
        Ok(())
    }
    pub(super) fn measured_target_bounds(&self) -> Option<Rect> {
        let doc = self.engine.document();
        let measured = |id| {
            let request = ContentBoundsRequest::new(doc, ContentScope::Target(id));
            self.content_bounds.cache.get(&request).or_else(|| request.known_bounds())
        };
        let targets = self.bounds_targets();
        if self.retained_transforming() && (targets.len() != 1 || self.transform_roots() != targets) {
            return targets.into_iter().try_fold(Rect::EMPTY, |bounds, id|
                Some(bounds.union(doc.layer_geometry(id).forward_bounds(measured(id)?))));
        }
        let bounds = measured(doc.active_target())?;
        if bounds.is_empty() { return Some(bounds); }
        let Some(companion) = self.bounds_companion() else { return Some(bounds); };
        let to = doc.affine_edit_transform(companion)?.then(doc.affine_edit_transform(doc.active_target())?.inverse()?);
        let other = measured(companion)?;
        Some(if other.is_empty() { bounds } else { bounds.union(to.bounds(other)) })
    }

    fn bounds_companion(&self) -> Option<layer_core::LayerId> {
        let doc = self.engine.document();
        let owner = doc.target_owner(doc.active_target())?;
        if owner.kind != layer_core::LayerKind::Paint
            || (!doc.active_mask && doc.selection.is_none()) { return None; }
        let mask = owner.mask.as_ref().filter(|mask| mask.linked)?;
        Some(if doc.active_target() == owner.id { mask.id } else { owner.id })
    }

    pub(super) fn cancel_content_bounds(&mut self) -> bool {
        self.content_bounds.moving = None;
        let job = self.content_bounds.job.take();
        let bake = self.content_bounds.bake.take().is_some();
        if job.is_none() && !bake { return false; }
        self.engine.backend_mut().cancel_content_bounds();
        bake || job.is_some_and(|job| !matches!(job.purpose, ContentUse::PrepareSnap(_)))
    }

    pub(super) fn transform_pixels_refusal(&self) -> Option<std::sync::Arc<str>> {
        use layer_core::TransformPixelsRefusal::*;
        let doc = self.engine.document();
        let l = self.localization();
        if self.selection_masks.target().is_some() { return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST)); }
        doc.transform_pixels_refusal(doc.active_target()).map(|reason| l.text(match reason {
            Target => MessageId::COMMANDS_TRANSFORM_PIXELS_SELECT_LAYER,
            Locked => MessageId::COMMANDS_THE_LAYER_IS_LOCKED,
            Unchanged => MessageId::COMMANDS_TRANSFORM_PIXELS_UNCHANGED,
            Pending => MessageId::COMMANDS_WAIT_FOR_CURRENT_EDIT,
        }))
    }

    pub(super) fn apply_transform_pixels(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.transform_pixels_refusal())?;
        self.cancel_content_bounds();
        let doc = self.engine.document();
        let interpolation = doc.layer_geometry(doc.active_target()).placement.interpolation;
        let plan = doc.transform_pixels_plan(doc.active_target(), interpolation, Default::default())?;
        self.engine.validate_edit(&plan.reserved_edit()).map_err(error)?;
        let mut bake = PixelBake { epoch: self.state.document_file.epoch, revision: doc.revision, plan, submitted: false };
        self.cancel_auto_levels();self.cancel_histogram();
        bake.submitted = self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::TransformPixels(bake.plan.clone())).map_err(error)?;
        self.content_bounds.bake = Some(bake);
        self.refresh_tools();
        self.refresh_commands();
        Ok(())
    }

    fn poll_transform_pixels(&mut self) -> u32 {
        let l = self.localization().clone();
        let Some(bake) = self.content_bounds.bake.as_mut() else { return 0; };
        let changed = bake.epoch != self.state.document_file.epoch || bake.revision != self.engine.document().revision
            || self.engine.document().active_target() != bake.plan.target;
        let result = if changed {
            self.engine.backend_mut().cancel_snapshot();
            Err(l.text(MessageId::COMMANDS_TRANSFORM_PIXELS_DRAWING_CHANGED).to_string())
        } else if !bake.submitted {
            match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::TransformPixels(bake.plan.clone())).map_err(error) {
                Ok(accepted) => { bake.submitted = accepted; return 0; }
                Err(error) => Err(error),
            }
        } else {
            match self.engine.backend_mut().take_snapshot() {
                None => return 0,
                Some(result) => result.map_err(error),
            }
        };
        let bake = self.content_bounds.bake.take().unwrap();
        let result = result.and_then(|result| {
            let layer_render::SnapshotResult::TransformPixels(layer) = result else { return Err(l.text(MessageId::COMMANDS_TRANSFORM_PIXELS_UNEXPECTED_RESULT).to_string()); };
            if layer.id != bake.plan.output.id { return Err(l.text(MessageId::COMMANDS_TRANSFORM_PIXELS_LAYER_CHANGED).to_string()); }
            self.require_document_idle()?;
            refused(self.transform_pixels_refusal())?;
            let edit = Edit::ReplaceLayer(layer);
            self.source_edit_candidates(&edit, &[], Default::default())?;
            self.engine.apply_edit(edit).map_err(error)?;
            self.refresh_document();
            Ok(())
        });
        if let Err(message) = result { self.notify(message); }
        self.refresh_tools();
        self.refresh_commands();
        regions::DOCUMENT | regions::BRUSH | regions::COMMANDS | regions::HOST
    }

    pub(super) fn poll_content_bounds(&mut self) -> u32 {
        if self.content_bounds.baking() { return self.poll_transform_pixels(); }
        self.content_bounds.cache.discard_changed(self.engine.document());
        let Some(job) = &self.content_bounds.job else { return 0 };
        let roots = if job.roots.is_empty() { Vec::new() } else { self.transform_roots() };
        let job = self.content_bounds.job.as_mut().unwrap();
        let doc = self.engine.document();
        let changed = doc.revision != job.revision || doc.id != job.request.document.id
            || (!job.roots.is_empty() && job.roots != roots)
            || (matches!(job.purpose, ContentUse::Transform | ContentUse::Move | ContentUse::PrepareMove) && (doc.active_target() != job.target
                || doc.selection != job.request.document.selection));
        let cancelled = (job.purpose == ContentUse::FitContent && self.operation.crop.is_none()) || changed;
        let result = if cancelled {
            self.engine.backend_mut().cancel_content_bounds();
            Err("The content bounds scan stopped because the drawing changed".into())
        } else {
            if !job.submitted {
                match self.engine.backend_mut().request_content_bounds(job.request.clone()).map_err(error) {
                    Ok(accepted) => { job.submitted = accepted; return 0; }
                    Err(message) => {
                        let notify = !matches!(job.purpose, ContentUse::PrepareMove | ContentUse::PrepareSnap(_));
                        if matches!(job.purpose, ContentUse::PrepareSnap(_)) { self.content_bounds.cache.insert(job.request.clone(), Rect::EMPTY); }
                        self.content_bounds.job = None;
                        self.content_bounds.moving = None;
                        if notify { self.notify(message); }
                        return regions::HOST;
                    }
                }
            }
            match self.engine.backend_mut().take_content_bounds() {
                None => return 0,
                Some(result) => result.map_err(error),
            }
        };
        let job = self.content_bounds.job.take().unwrap();
        if matches!(job.purpose, ContentUse::PrepareSnap(_)) && !cancelled && result.is_err() {
            self.content_bounds.cache.insert(job.request.clone(), Rect::EMPTY);
        }
        let outcome = result.and_then(|bounds| {
            refused(self.content_bounds_refusal(job.purpose))?;
            self.require_content_idle(job.purpose)?;
            let paired = matches!(job.request.scope, ContentScope::Target(_));
            self.content_bounds.cache.insert(job.request, bounds);
            if paired { self.request_content_bounds(job.purpose) } else { self.use_content_bounds(job.purpose, bounds) }
        });
        if let Err(message) = outcome {
            self.content_bounds.moving = None;
            if !matches!(job.purpose, ContentUse::PrepareMove | ContentUse::PrepareSnap(_)) { self.notify(message); }
        }
        self.refresh_tools();
        regions::DOCUMENT | regions::BRUSH | regions::COMMANDS | regions::CAMERA | regions::HOST
    }

    fn use_content_bounds(&mut self, purpose: ContentUse, bounds: Rect) -> Result<(), String> {
        let doc = self.engine.document();
        let canvas = Rect { min: Point::default(), max: Point { x: doc.width as f32, y: doc.height as f32 } };
        let whole = CanvasRect { origin: [0; 2], size: [doc.width, doc.height] };
        match purpose {
            ContentUse::PrepareMove => Ok(()),
            ContentUse::PrepareSnap(_) => self.prepare_transform_snapping(),
            ContentUse::Move => {
                let Some(moving) = self.content_bounds.moving.take() else { return Ok(()); };
                if bounds.is_empty() { return Err("The selection does not overlap this layer".into()); }
                self.begin_move_transform(moving.press, moving.keep_source)?;
                if let Some((event, point)) = moving.latest { self.transform_pen(event, point)?; }
                Ok(())
            }
            ContentUse::Transform => {
                if bounds.is_empty() { return Err("The selection does not overlap this layer".into()); }
                self.begin_transform()
            }
            ContentUse::Trim => {
                if bounds.is_empty() {
                    return Err("There are no visible pixels to trim to".into());
                }
                let rect = pixel_rect(bounds);
                if rect == whole {
                    return Err("The visible pixels already reach every edge of the canvas".into());
                }
                self.apply_canvas_geometry(&CanvasGeometry::crop(rect), Vec::new()).map_err(|e| e.to_string())
            }
            ContentUse::RevealAll => {
                let rect = pixel_rect(bounds.union(canvas));
                if rect == whole {
                    return Err("Every pixel is already on the canvas".into());
                }
                self.apply_canvas_geometry(&CanvasGeometry::crop(rect), Vec::new()).map_err(|e| e.to_string())
            }
            ContentUse::FitContent => {
                if bounds.is_empty() {
                    return Err("There are no visible pixels to fit the crop to".into());
                }
                let rect = pixel_rect(bounds);
                self.fit_crop(rect)
            }
        }
    }

    /// Set the crop frame to `rect`, upright and with a free ratio.
    fn fit_crop(&mut self, rect: CanvasRect) -> Result<(), String> {
        let session = self.operation.crop.as_mut().ok_or("Choose the Crop tool first")?;
        let size = rect.size.map(|v| v as f32);
        session.set_frame(CropFrame {
            center: Point { x: rect.origin[0] as f32 + size[0] / 2., y: rect.origin[1] as f32 + size[1] / 2. },
            size,
            angle: 0.,
        });
        self.operation.crop_options.ratio = CropRatio::Free;
        self.layer_interaction.changed = true;
        Ok(())
    }
}
