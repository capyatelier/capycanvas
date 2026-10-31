//! Platform-client canvas orchestration for Layer.
//!
//! The engine has one mutable owner. Platform callbacks only touch the SPSC
//! producer. Renderers receive one borrowed packet per display frame. The two
//! queue halves may also run sequentially on one event loop.

use crate::brush::{DabGenerator, damage_for_dabs, pen_up_settlement_micros};
use crate::feedback::{
    FeedbackConfigError, InstantFeedbackConfig, MAX_FINALIZATION_LAG_MICROS, PreviewTip,
    TipInput, TipSource, finalized_count, tip_times,
};
use crate::input::{
    InputConsumer, PenEvent, PenPhase, PressureCurve, SampleFlags, StrokeBuilder, ToolKind,
    ViewTransform,
};
use layer_core::{
    BrushError, BrushExecution, BrushSnapshot, CloneSource, Document, DocumentError, DrawingRefusal,
    Edit, Editor, SourceTarget, Rect, Retouch, RetouchSource, Stroke, StrokeId, StrokeTool,
};
use layer_render::{
    CanvasRenderer, Dab, DabBatch, DabBatchKind, DabStyle, FramePacket, RetouchPreparation,
    ViewState,
};
use std::{collections::VecDeque, fmt, sync::Arc};

#[path = "corrections.rs"]
mod corrections;
#[path = "color_transition.rs"]
mod color_transition;
#[path = "frame_steps.rs"]
mod frame_steps;

struct PreparedFrame {
    reset: bool,
    time: f32,
    steps: Option<frame_steps::FrameSteps>,
}

const TRANSFORM_HISTORY: usize = 16;
const INPUT_BATCH: usize = 4096;
const STROKE_POINT_CAPACITY: usize = 65_536;
const DAB_CAPACITY: usize = 32_768;
const BATCH_CAPACITY: usize = 64;
// A small wet microbatch amortizes page ping-pong and reservoir passes while
// keeping exchange far below the eight-sample display-frame cadence that made
// carried color advance in visible bands.
pub(crate) const MAX_CONTACT_POINTS: usize = 131_072;
const CORRECTION_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_WET_DABS_PER_BATCH: u32 = 3;
// Smudge contacts compose into one bounded semi-Lagrangian backtrace. Live
// input retains an incomplete chunk as replaceable GPU preview work, so these
// boundaries depend on the stroke rather than display-frame packet cadence.
const MAX_SMUDGE_DABS_PER_BATCH: usize = 6;
const MAX_DITHER_DABS_PER_BATCH: usize = 64;
const MAX_SMUDGE_TRAVEL_DIAMETERS: f32 = 0.24;
const MAX_SMUDGE_DAMAGE_DIAMETERS_SQUARED: f32 = 4.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineMetrics {
    pub input_events: u64,
    pub last_consumed_paint_ns: u64,
    pub frames: u64,
    pub committed_strokes: u64,
    pub input_budget_cancellations: u64,
    pub platform_prediction_frames: u64,
    pub engine_prediction_frames: u64,
}

/// Why a stroke starting now would not paint as the brush is configured.
/// Every refusal except `DryMask` stops the stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeRefusal {
    Target(DrawingRefusal),
    /// Erasing keeps an alpha-locked layer's transparency, so it changes nothing.
    AlphaLocked,
    /// Masks take dry coverage; the stroke paints without its wet, smudge or
    /// liquify behavior.
    DryMask,
    /// A retouching stroke would copy nothing: its layer is empty and it
    /// samples no reference layer below.
    EmptySource(RetouchSource),
    /// The Clone Stamp or Healing Brush has no source point yet.
    NoCloneSource,
}

#[derive(Clone, Copy)]
struct StrokeTarget {
    target: SourceTarget,
    mask: bool,
    alpha_locked: bool,
    inverted: bool,
}

#[derive(Clone, Debug)]
struct ActiveStroke {
    paint_color: Option<layer_core::color::RgbColor>,
    before: layer_core::raster::RasterRevision,
    id: StrokeId,
    target: SourceTarget,
    tool: StrokeTool,
    brush: BrushSnapshot,
    style: DabStyle,
    feedback: InstantFeedbackConfig,
    tip: PreviewTip,
    persistent_started: bool,
    committed_chunk_dabs: usize,
    material_updates: Vec<u32>,
    ruler: Option<layer_core::RulerConstraint>,
    /// The renderer could not sample this retouching stroke's whole source
    /// while the pen was down.
    replay_after_contact: bool,
    /// The Clone source as this stroke found it, before anchoring it.
    clone_start: Option<CloneSource>,
    barrel_twist: bool,
    momentum: Option<(Vec<Dab>, usize)>,
}

#[derive(Clone)]
struct ContactSettings {
    brush: BrushSnapshot,
    tool: StrokeTool,
    retouch: Option<RetouchSource>,
    clone_source: CloneSource,
    clone_generation: u64,
    paint_color: Option<layer_core::color::RgbColor>,
    pressure: PressureCurve,
    tilt: crate::TiltCurve,
    instant_feedback: InstantFeedbackConfig,
    ruler_snapping: Option<f32>,
}
pub struct CanvasEngine<B: CanvasRenderer> {
    settings: ContactSettings,
    used_colors: Vec<layer_core::color::RgbColor>,
    pub recording: crate::recording::Recording,
    backend: B,
    editor: Editor,
    input: InputConsumer<PenEvent>,
    queued_contacts: VecDeque<((u64, u64), ContactSettings)>,
    contact_settings: Option<ContactSettings>,
    view: ViewState,
    transforms: VecDeque<ViewTransform>,
    /// The live clone stroke's document offset.
    clone_stroke: Option<[f32; 2]>,
    retouch_points: Vec<layer_core::Point>,
    prepared_retouch: Option<(u64, layer_core::Revision, RetouchPreparation)>,
    builder: StrokeBuilder,
    dab_generator: DabGenerator,
    finalized_real_points: usize,
    active_stroke: Option<ActiveStroke>,
    pending_up: Option<(PenEvent, u32)>,
    frame_clock_ns: Option<u64>,
    completed_stroke: Option<Stroke>,
    completed_at: Option<web_time::Instant>,
    completed_publication: Option<layer_core::raster::RasterRevision>,
    completed_before: Option<layer_core::raster::RasterRevision>,
    completed_clone_start: Option<CloneSource>,
    restore_rasters: Vec<(SourceTarget, layer_core::raster::RasterRevision)>,
    pending_frame: Option<PreparedFrame>,
    rebuild_completed: bool,
    estimates: std::collections::BTreeMap<(u64, u64), corrections::EstimatedPoint>,
    pending_chunk_dabs: Vec<Dab>,
    dabs: Vec<Dab>,
    batches: Vec<DabBatch>,
    liquify_preview: Option<DabBatch>,
    transform_preview: Option<layer_render::TransformPreview>,
    transform_selection: std::sync::OnceLock<Option<layer_core::Selection>>,
    selection_display: Option<Option<layer_core::Selection>>,
    scene_preview: Option<ScenePreview>,
    rebuild_all: bool,
    composite_all: bool,
    raster_dirty: bool,
    animation_origin_ns: Option<u64>,
    animation_time: f32,
    animation_seed: f32,
    evaluation_context: layer_core::authored::EvaluationContext,
    metrics: EngineMetrics,
}

/// Published pixels with no watercolor or wet material state, so an erase
/// can rewrite only the pages it covers instead of settling the whole layer.
fn plain_color(raster: &layer_core::raster::RasterRevision) -> bool {
    matches!(raster.try_data(), Some(Ok(data)) if data.watercolor.is_none()
        && data.tiles.keys().all(|key| key.plane == layer_core::raster::RasterPlane::Color))
}

#[derive(Clone, Debug)]
pub struct ScenePreview {
    pub above: layer_core::authored::OccurrenceHandle,
    pub scene: Arc<layer_core::SceneSnapshot>,
}

impl<B: CanvasRenderer> CanvasEngine<B> {
    pub fn new(
        mut backend: B,
        document: Document,
        input: InputConsumer<PenEvent>,
        view: ViewState,
        input_transform: ViewTransform,
    ) -> Result<Self, EngineError<B::Error>> {
        if backend.document_color() != document.composition().color {
            return Err(EngineError::Document(DocumentError::InvalidLayerOperation(
                "The renderer is not configured for this document's color space and precision",
            )));
        }
        backend
            .resize_surface(view.width_px, view.height_px)
            .map_err(EngineError::Backend)?;
        let mut transforms = VecDeque::with_capacity(TRANSFORM_HISTORY);
        transforms.push_back(input_transform);
        let dab_generator = DabGenerator::new(document.composition().color.space);
        let evaluation_context = document.output().context.clone();
        backend.seed_evaluation_context(evaluation_context.clone());
        Ok(Self {
            settings: ContactSettings {
                brush: BrushSnapshot::default(), tool: StrokeTool::Brush, retouch: None,
                clone_source: CloneSource::default(), clone_generation: 0, paint_color: None,
                pressure: PressureCurve::default(), tilt: crate::TiltCurve::default(), instant_feedback: InstantFeedbackConfig::default(), ruler_snapping: Some(12.),
            },
            used_colors: Vec::new(),
            recording: Default::default(),
            backend,
            editor: Editor::new(document),
            input,
            queued_contacts: VecDeque::new(),
            contact_settings: None,
            view,
            transforms,
            clone_stroke: None,
            retouch_points: Vec::new(),
            prepared_retouch: None,
            builder: StrokeBuilder::with_capacity(STROKE_POINT_CAPACITY),
            dab_generator,
            finalized_real_points: 0,
            active_stroke: None,
            pending_up: None,
            frame_clock_ns: None,
            completed_stroke: None,
            completed_at: None,
            completed_publication: None,
            completed_before: None,
            completed_clone_start: None,
            restore_rasters: Vec::new(),
            pending_frame: None,
            rebuild_completed: false,
            estimates: Default::default(),
            pending_chunk_dabs: Vec::with_capacity(MAX_SMUDGE_DABS_PER_BATCH),
            dabs: Vec::with_capacity(DAB_CAPACITY),
            batches: Vec::with_capacity(BATCH_CAPACITY),
            liquify_preview: None,
            transform_preview: None,
            transform_selection: Default::default(),
            selection_display: None,
            scene_preview: None,
            rebuild_all: true,
            composite_all: true,
            raster_dirty: false,
            animation_origin_ns: None,
            animation_time: evaluation_context.elapsed,
            animation_seed: evaluation_context.elapsed,
            evaluation_context,
            metrics: EngineMetrics::default(),
        })
    }

    /// Time of the last successfully submitted document frame, frozen when
    /// capturing an export so animated filters match that revision's view.
    pub fn animation_time(&self) -> f32 {
        self.animation_time
    }

    pub fn document(&self) -> &Document {
        self.editor.document()
    }

    pub fn raster_edit_pending(&self) -> bool {
        self.rebuild_completed || self.batches.iter().any(|batch| matches!(batch.kind, DabBatchKind::RasterOperation(_))
            || self.pending_frame.is_some() && batch.stroke_end)
    }

    pub fn capture_artwork(&self, session_generation: u64) -> Result<layer_core::authored::ArtworkCapture, DocumentError> {
        if self.raster_edit_pending() {
            return Err(DocumentError::InvalidLayerOperation("Wait for the preceding raster edit"));
        }
        self.editor.capture(session_generation, self.evaluation_context.clone())
    }

    pub fn capture_session(&self, session_generation: u64) -> Result<layer_core::package::session::EditorCapture, DocumentError> {
        let Self {editor,settings:_,used_colors:_,recording:_,backend:_,input:_,queued_contacts:_,contact_settings:_,
            view:_,transforms:_,clone_stroke:_,retouch_points:_,prepared_retouch:_,builder:_,dab_generator:_,
            finalized_real_points:_,active_stroke:_,pending_up:_,frame_clock_ns:_,completed_stroke:_,completed_at:_,completed_publication:_,completed_before:_,
            completed_clone_start:_,restore_rasters:_,pending_frame:_,rebuild_completed:_,estimates:_,
            pending_chunk_dabs:_,dabs:_,batches:_,liquify_preview:_,transform_preview:_,transform_selection:_,selection_display:_,
            scene_preview:_,rebuild_all:_,composite_all:_,raster_dirty:_,animation_origin_ns:_,animation_time:_,
            animation_seed:_,evaluation_context:_,metrics:_}=self;
        editor.capture_session(self.capture_artwork(session_generation)?)
    }

    pub fn restore_editor(&mut self, editor: Editor) -> Result<(), DocumentError> {
        if self.has_pending_input() || self.has_active_stroke() || self.pending_frame.is_some()
            || !self.batches.is_empty() || self.rebuild_completed || self.raster_dirty
        {
            return Err(DocumentError::InvalidLayerOperation("Finish the current operation before restoring a session"));
        }
        if editor.document() != self.document() {
            return Err(DocumentError::InvalidLayerOperation("The restored session does not match the prepared drawing"));
        }
        self.editor = editor;
        Ok(())
    }

    pub fn scene_snapshot(&self) -> Arc<layer_core::SceneSnapshot> {
        let mut context = self.evaluation_context.clone();
        context.retain_effects(&self.document().artwork);
        self.document().snapshot_with_context(context)
    }

    pub fn retained_images(&self) -> Vec<layer_core::Image> {
        self.editor.retained_images()
    }

    pub fn checkpoint(&self) -> u64 {
        self.editor.checkpoint()
    }

    pub fn history_reaches(&self, checkpoint: u64) -> bool {
        self.editor.history_reaches(checkpoint)
    }

    pub fn view(&self) -> ViewState {
        self.view
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn retained_tiles(&self) -> layer_core::raster_storage::RetainedTiles {
        self.editor.retained_tiles()
    }

    /// Normal tab parking waits for submission, unlike device-failure recovery.
    pub fn can_park(&self) -> bool {
        !self.has_pending_input() && !self.has_active_stroke() && !self.has_pending_document_edits()
    }

    pub fn release_idle_buffers(&mut self) {
        if !self.can_park() { return; }
        self.builder = StrokeBuilder::with_capacity(0);
        self.dabs = Vec::new();
        self.batches = Vec::new();
        self.pending_chunk_dabs = Vec::new();
        self.completed_stroke = None;
        self.completed_publication = None;
        self.completed_at = None;
        self.completed_before = None;
        self.estimates.clear();
    }

    /// Replace GPU state while retaining committed raster roots and history.
    /// Prepare sources and resize before adoption; failure leaves live input intact.
    /// The active builder and queued samples survive; completed history restores
    /// immutable rasters without replaying historical strokes.
    pub fn replace_backend(&mut self, mut backend: B) -> Result<B, EngineError<B::Error>> {
        self.clear_liquify_preview();
        if backend.document_color() != self.editor.document().composition().color {
            return Err(EngineError::Document(DocumentError::InvalidLayerOperation(
                "The replacement renderer is not configured for this document's color space and precision",
            )));
        }
        if self.pending_frame.is_some() {
            return Err(EngineError::Document(DocumentError::InvalidLayerOperation(
                "Raster restoration is busy; retry renderer replacement",
            )));
        }
        backend
            .resize_surface(self.view.width_px, self.view.height_px)
            .map_err(EngineError::Backend)?;
        backend.seed_evaluation_context(self.evaluation_context.clone());
        self.editor.discard_liquify_maps();
        if let Some(active) = &mut self.active_stroke && let Some(context) = &mut active.style.liquify {
            let context = Arc::make_mut(context);
            context.field = None;
            context.original = context.current.clone();
        }
        self.restore_rasters.clear();
        self.rebuild_all = true;
        self.composite_all = true;
        self.prepared_retouch = None;
        Ok(std::mem::replace(&mut self.backend, backend))
    }

    pub fn metrics(&self) -> EngineMetrics {
        self.metrics
    }

    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    pub fn raster_backing_pending(&self) -> bool {
        self.pending_up.is_some() || self.pending_frame.is_some() || self.backend.has_pending_submission()
            || self.batches.iter().any(raster_edit_batch)
    }

    pub fn has_active_stroke(&self) -> bool {
        self.active_stroke.is_some()
    }

    pub fn brush(&self) -> &BrushSnapshot {
        self.active_stroke
            .as_ref()
            .map_or(&self.settings.brush, |active| &active.brush)
    }

    /// Optional source definition, captured alongside the brush at contact down.
    /// It is metadata for completed artwork, never a display RGB approximation.
    pub fn set_paint_color(&mut self, color: layer_core::color::RgbColor) {
        self.settings.paint_color = Some(color);
    }
    pub fn take_used_colors(&mut self) -> impl Iterator<Item = layer_core::color::RgbColor> + '_ {
        self.used_colors.drain(..)
    }

    /// Editable configuration for the next stroke, independent of an active
    /// stroke's immutable snapshot. UI edits must not restore old stroke values.
    pub fn configured_brush(&self) -> &BrushSnapshot {
        &self.settings.brush
    }

    /// Cursor-only evaluation at current input, sharing the live stroke's
    /// pressure curve, sensors and random sequence. No paint is submitted.
    pub fn cursor_contacts(
        &self,
        event: PenEvent,
        hover: &mut DabGenerator,
        hover_start_ns: u64,
    ) -> Vec<Dab> {
        let mut point = crate::input::calibrated_stroke_point(
            event,
            *self.transforms.back().unwrap(),
            self.pressure_for_event(event).clone(),
            self.tilt_for_event(event),
            hover_start_ns,
        );
        let ruler = self.active_stroke.as_ref().map_or_else(
            || {
                self.settings.ruler_snapping.and_then(|reach| {
                    layer_core::choose_ruler(self.document().rulers(), point.position, reach)
                })
            },
            |active| active.ruler,
        );
        if let Some(ruler) = ruler {
            point.position = ruler.project(point.position);
        }
        let target = self
            .active_stroke
            .as_ref()
            .map_or_else(|| self.document().drawing_target().or(self.document().working.target), |s| Some(s.target));
        let offset = layer_core::offsets::point(target.map_or_default(|target| self.document().target_offset(target)));
        // Stroke dynamics run in layer-local coordinates; outlines are returned
        // in document coordinates, including for translated layers.
        point.position.x -= offset.x;
        point.position.y -= offset.y;
        let mut contacts = if self.active_stroke.is_some() {
            point.elapsed_micros = self
                .builder
                .elapsed_micros_at(event.timestamp_ns)
                .unwrap_or(0);
            let mut preview = self.dab_generator.preview();
            let mut ignored = Vec::new();
            for &sample in &self.builder.real_points()[self.finalized_real_points..] {
                preview.append(sample, self.brush(), &mut ignored);
            }
            preview.cursor_contacts(point, self.brush())
        } else {
            // Hovering pens report zero pressure; show the nominal footprint.
            point.pressure = 1.0;
            hover.set_space(self.document().composition().color.space);
            hover.set_barrel_twist(event.flags.contains(SampleFlags::BARREL_TWIST));
            hover.cursor_seed(self.document().next_stroke_id(), self.brush());
            hover.cursor_contacts(point, self.brush())
        };
        for dab in &mut contacts {
            dab.center.x += offset.x;
            dab.center.y += offset.y;
        }
        contacts
    }

    pub fn allocate_coverage_handle(&mut self) -> layer_core::authored::CoverageHandle {
        self.editor.allocate_coverage_handle()
    }
    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        self.editor.allocate_stroke_id()
    }
    pub fn amend_edit(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.require_renderer_color(edit.resulting_color(self.document().composition().color))?;
        let image = edit.changes_image(self.editor.document());
        self.editor.amend(edit)?;
        self.composite_all |= image;
        Ok(())
    }
    pub fn preview_edit(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.require_renderer_color(edit.resulting_color(self.document().composition().color))?;
        let image = edit.changes_image(self.editor.document());
        self.editor.preview(edit)?;
        self.composite_all |= image;
        Ok(())
    }

    /// Show `preview` on the canvas until it is replaced or cleared. History
    /// and the document never hold it.
    pub fn set_scene_preview(&mut self, preview: Option<ScenePreview>) {
        self.scene_preview = preview;
        self.composite_all = true;
    }

    /// Current disposable transform, including its startup shader dependency.
    pub fn transform_preview(&self) -> Option<&layer_render::TransformPreview> {
        self.transform_preview.as_ref()
    }

    /// Disposable absolute pixel transform. History changes only on Apply.
    pub fn set_transform_preview(
        &mut self,
        preview: Option<layer_render::TransformPreview>,
    ) -> Result<(), DocumentError> {
        let companion = preview
            .as_ref()
            .and_then(|p| p.companion(self.document().scene()));
        for p in preview.iter().chain(companion.iter()) {
            let doc = self.document();
            if self.has_active_stroke()
                || doc.target_owner(p.target).is_some_and(|owner| doc.is_locked(owner))
                || doc.target_raster(p.target).is_none()
                || p.transform.validate().is_err()
                || p.selection.as_ref().is_some_and(|s| {
                    s.affine.inverse().is_none()
                        || p.transform.as_affine().is_some_and(|a| s.transformed(a).is_err())
                })
            {
                return Err(DocumentError::InvalidLayerOperation(
                    "Select an unlocked paint layer and finish the stroke",
                ));
            }
        }
        self.transform_preview = preview;
        self.transform_selection = Default::default();
        Ok(())
    }

    /// Append a command for the next raster submission. Its resulting immutable
    /// pixels become the edit's undo/save state; the command is then discarded.
    pub fn append_raster_operation(
        &mut self,
        id: SourceTarget,
        operation: layer_core::RasterOperation,
    ) -> Result<(), DocumentError> {
        self.append_operations(Vec::new(), vec![(id, operation)], None, false)
    }

    /// Apply `prefix` edits, such as inserting layers, and run pixel operations
    /// on their result as one undo step. An inserted target starts from the
    /// pixels it was inserted with. `selection_after`: None keeps the
    /// selection; Some(None) clears it.
    pub fn insert_with_operations(
        &mut self,
        prefix: Vec<Edit>,
        operations: Vec<(SourceTarget, layer_core::RasterOperation)>,
        selection_after: Option<Option<layer_core::Selection>>,
    ) -> Result<(), DocumentError> {
        self.append_operations(prefix, operations, selection_after, false)
    }

    /// Commit the displayed pixels and moved selection as one undoable edit.
    /// The GPU can keep the matching preview result instead of resampling it.
    /// `resampled` is the coverage `transform_selection_request` produced; the
    /// preview's inversion is kept.
    pub fn commit_transform(
        &mut self,
        resampled: Option<std::sync::Arc<layer_core::SelectionPixels>>,
    ) -> Result<bool, DocumentError> {
        let preview = self
            .transform_preview
            .clone()
            .ok_or(DocumentError::InvalidLayerOperation("No transform to apply"))?;
        if preview.transform.is_identity() {
            self.transform_preview = None;
            return Ok(false);
        }
        let basis = self.document().local_to_document(preview.target);
        let selection = match (resampled, &preview.selection) {
            (Some(pixels), _) if pixels.extent() != self.document().target_extent(preview.target) => {
                return Err(DocumentError::InvalidLayerOperation(
                    "The resampled selection belongs to another layer",
                ));
            }
            (Some(pixels), selection) => Some(
                layer_core::Selection {
                    inverted: selection.as_ref().is_some_and(|s| s.inverted),
                    ..layer_core::Selection::pixels(pixels)
                }
                .transformed(basis)?,
            ),
            (None, Some(selection)) if self.selection_display.is_none() => {
                Some(selection.mapped(&preview.transform.placement)?.transformed(basis)?)
            }
            (None, _) => self.display_selection().map(|s| s.into_owned()),
        };
        let companion = preview.companion(self.document().scene());
        let mut operations = Vec::with_capacity(2);
        for target in std::iter::once(preview).chain(companion) {
            let mut coverage = layer_core::CoverageSnapshot::reveal_all(
                self.allocate_coverage_handle(),
                self.document().target_extent(target.target),
                [0; 2],
            );
            coverage.source.default_coverage = f32::from(target.selection.is_none());
            coverage.selection = target.selection;
            operations.push((
                target.target,
                layer_core::RasterOperation {
                    placement: layer_core::Affine::IDENTITY,
                    coverage,
                    kind: layer_core::RasterOperationKind::Transform(target.transform.clone()),
                },
            ));
        }
        self.append_operations(Vec::new(), operations, Some(selection), false)?;
        Ok(true)
    }

    /// The GPU request that carries the preview's pixel selection through a
    /// map its metadata cannot express, in the transformed target's pixels.
    /// Apply waits for its result and passes it to `commit_transform`.
    pub fn transform_selection_request(
        &self,
        request_id: u64,
    ) -> Option<layer_render::RegionRequest> {
        let preview = self.transform_preview.as_ref()?;
        let selection = preview.selection.as_ref().filter(|s| {
            !preview.transform.is_identity()
                && self.selection_display.is_none()
                && s.needs_resample(&preview.transform.placement)
        })?;
        Some(layer_render::RegionRequest {
            enclosure: None, contiguous: false,
            selection: None,
            request_id,
            source: layer_render::RegionSource::TransformedSelection {
                target: preview.target,
                selection: std::sync::Arc::new(selection.clone()),
                map: preview.transform.placement.clone(),
            },
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
        })
    }

    /// Override only the display mask. Artwork clipping always uses the document selection.
    pub fn set_selection_display(&mut self, selection: Option<Option<layer_core::Selection>>) {
        self.selection_display = selection;
    }

    /// Provisional selection placement is view state, never another history edit.
    pub fn display_selection(&self) -> Option<std::borrow::Cow<'_, layer_core::Selection>> {
        if let Some(selection) = &self.selection_display {
            return selection.as_ref().map(std::borrow::Cow::Borrowed);
        }
        if let Some(preview) = &self.transform_preview {
            let mapped = self.transform_selection.get_or_init(|| {
                preview.selection.as_ref()?.mapped(&preview.transform.placement).ok()
            });
            let basis = self.document().local_to_document(preview.target);
            return mapped.as_ref()?.transformed(basis).ok().map(std::borrow::Cow::Owned);
        }
        self.document()
            .working.selection
            .as_ref()
            .map(std::borrow::Cow::Borrowed)
    }

    pub fn set_layer_color_mode(&mut self, owner: layer_core::OccurrenceHandle, mode: layer_core::color::LayerColorMode) -> Result<(), DocumentError> {
        self.flush_pending_edits()?;
        if self.has_active_stroke() { return Err(DocumentError::InvalidLayerOperation("Finish the stroke first")); }
        let doc = self.document();
        let Some(layer_core::OccurrenceContent::Paint(handle)) = doc.scene().occurrence(owner).map(|o| &o.content) else {
            return Err(DocumentError::InvalidLayerOperation("Select a paint layer"));
        };
        if doc.is_locked(owner) { return Err(DocumentError::InvalidLayerOperation("Select an unlocked paint layer")); }
        let source = doc.artwork.paint.get(*handle).unwrap();
        if source.color_mode == mode { return Ok(()); }
        if source.base.is_none() && source.raster.is_empty() && source.operations.is_empty() {
            let mut source = source.clone(); source.color_mode = mode;
            let change = layer_core::RecordChange::replace(&doc.artwork.paint, *handle, Some(source)).map_err(DocumentError::InvalidLayerOperation)?;
            return self.apply_edit(layer_core::Edit::Paint(change));
        }
        let operation = layer_core::RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: layer_core::CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), source.domain, [0; 2]),
            kind: layer_core::RasterOperationKind::ColorMode(mode),
        };
        self.append_operations(Vec::new(), vec![(SourceTarget::Paint(*handle), operation)], None, false)
    }

    fn append_operations(
        &mut self,
        prefix: Vec<Edit>,
        operations: Vec<(SourceTarget, layer_core::RasterOperation)>,
        selection_after: Option<Option<layer_core::Selection>>,
        reaches_locked_sources: bool,
    ) -> Result<(), DocumentError> {
        self.flush_pending_edits()?;
        self.end_corrections();
        if self.has_active_stroke() {
            return Err(DocumentError::InvalidLayerOperation("Finish the stroke first"));
        }
        let mut staged = self.document().clone();
        if !prefix.is_empty() { staged.apply(Edit::Batch(prefix.clone()))?; }
        let document = &staged;
        let mut sources = std::collections::BTreeMap::new();
        let mut batches = Vec::with_capacity(operations.len());
        let mut restores = Vec::new();
        let mut reservations = std::collections::BTreeMap::<SourceTarget, Rect>::new();
        for (target, operation) in operations {
            let pixels = document.target_raster(target)
                .ok_or(DocumentError::InvalidLayerOperation("Missing paint source"))?.clone();
            if matches!(target, SourceTarget::Selection(_)) || (document.target_owner(target).is_some_and(|owner| document.is_locked(owner)) && !reaches_locked_sources) {
                return Err(DocumentError::InvalidLayerOperation("Select an unlocked paint layer"));
            }
            if operation.kind == layer_core::RasterOperationKind::Coverage && !target.is_coverage() {
                return Err(DocumentError::InvalidLayerOperation("Mask coverage applies only to masks"));
            }
            let replaced = self.document().target_raster(target).is_none_or(|current| *current != pixels);
            if replaced && !restores.iter().any(|(t, _)| *t == target) { restores.push((target, pixels.clone())); }
            let extent = document.target_extent(target);
            let damage = if matches!(operation.kind, layer_core::RasterOperationKind::Erase { .. }) && !plain_color(&pixels) {
                Rect::from_extent(extent)
            } else { operation.bounds(extent) };
            let plane = if matches!(target, SourceTarget::Paint(_)) { layer_core::raster::RasterPlane::Color } else { layer_core::raster::RasterPlane::Mask };
            let color = document.composition().color;
            let mut page_bytes = layer_core::raster::TileBlob::max_compressed_len(plane.descriptor(color)).ok_or(DocumentError::InvalidLayerOperation("Unsupported raster capture representation"))? as u64;
            if matches!(target, SourceTarget::Paint(_)) && !plain_color(&pixels) {
                let wetness = layer_core::raster::TileBlob::max_compressed_len(layer_core::raster::RasterPlane::WatercolorWetness.descriptor(color)).ok_or(DocumentError::InvalidLayerOperation("Unsupported raster capture representation"))? as u64;
                page_bytes = page_bytes.checked_add(wetness).ok_or(DocumentError::InvalidLayerOperation("Raster capture exceeds the memory limit"))?;
            }
            let output_bounds = if pixels.is_empty() { damage } else { Rect::from_extent(extent) };
            let bytes = layer_core::raster::page_count(output_bounds, extent).checked_mul(page_bytes)
                .ok_or(DocumentError::InvalidLayerOperation("Raster capture exceeds the memory limit"))?;
            let (_, history) = sources.entry(target).or_insert_with(|| (
                layer_core::raster::RasterRevision::pending_within(bytes),
                document.target_operations(target).unwrap_or_default().to_vec(),
            ));
            let index = history.len() as u32;
            history.push(operation);
            reservations.entry(target).and_modify(|r| *r = r.union(damage)).or_insert(damage);
            batches.push(DabBatch {
                material_update: 0, stroke_id: StrokeId(0), target,
                kind: DabBatchKind::RasterOperation(index), stroke_start: false, stroke_end: false,
                first_dab: 0, dab_count: 0,
                style: DabStyle::for_brush(&BrushSnapshot::default(), StrokeTool::Brush), damage,
            });
        }
        for (target, damage) in reservations {
            let plane = if matches!(target, SourceTarget::Paint(_)) { layer_core::raster::RasterPlane::Color } else { layer_core::raster::RasterPlane::Mask };
            let tile = layer_core::raster::TileBlob::max_compressed_len(plane.descriptor(document.composition().color)).unwrap_or(0) as u64;
            let bytes = layer_core::raster::page_count(damage, document.target_extent(target)).checked_mul(tile)
                .ok_or(DocumentError::InvalidLayerOperation("Raster capture exceeds the memory limit"))?;
            let raster = &mut sources.get_mut(&target).unwrap().0;
            raster.reserve_pending_bytes(bytes);
        }
        let mut edits = prefix;
        for (target, (raster, operations)) in sources {
            match target {
                SourceTarget::Paint(handle) => {
                    let mut source = document.artwork.paint.get(handle).unwrap().clone();
                    source.raster = raster;
                    if let Some(mode) = operations.iter().rev().find_map(|op| if let layer_core::RasterOperationKind::ColorMode(mode) = op.kind { Some(mode) } else { None }) { source.color_mode = mode; }
                    source.operations = Arc::new(operations);
                    edits.push(Edit::Paint(layer_core::RecordChange::replace(&document.artwork.paint, handle, Some(source)).map_err(DocumentError::InvalidLayerOperation)?));
                }
                SourceTarget::Coverage(handle) => {
                    let mut source = document.artwork.coverage.get(handle).unwrap().clone();
                    source.raster = raster; source.operations = Arc::new(operations);
                    edits.push(Edit::Coverage(layer_core::RecordChange::replace(&document.artwork.coverage, handle, Some(source)).map_err(DocumentError::InvalidLayerOperation)?));
                }
                SourceTarget::Selection(_) => unreachable!(),
            }
        }
        if let Some(selection) = selection_after {
            let mut working = document.working.clone(); working.selection = selection;
            edits.push(Edit::Working(working));
        }
        let removes = edits.iter().any(|edit| matches!(edit, Edit::Stack(_) | Edit::Occurrence(_)));
        self.editor.perform(Edit::Batch(edits))?;
        self.transform_preview = None;
        self.composite_all |= removes;
        self.batches.extend(batches);
        self.restore_rasters.extend(restores);
        Ok(())
    }

    /// The project limits, and the renderer's, that a canvas change must fit.
    pub fn geometry_limits(&self) -> layer_core::GeometryLimits {
        layer_core::GeometryLimits {
            project: layer_core::ProjectLimits::default(),
            device_dimension: self.backend.max_document_dimension(),
        }
    }

    /// Move the canvas to `geometry` in one undo step. Limits are checked
    /// before anything changes. A crop that keeps hidden pixels changes only
    /// metadata; resampling and erasing run as pixel operations in the same
    /// step, on locked layers too.
    pub fn apply_canvas_geometry(
        &mut self,
        geometry: &layer_core::CanvasGeometry,
    ) -> Result<(), layer_core::CanvasGeometryError> {
        self.apply_canvas_geometry_with(geometry, Vec::new())
    }

    /// `apply_canvas_geometry` with `then` applied in the same undo step.
    pub fn apply_canvas_geometry_with(
        &mut self,
        geometry: &layer_core::CanvasGeometry,
        then: Vec<Edit>,
    ) -> Result<(), layer_core::CanvasGeometryError> {
        let plan = self.canvas_geometry_plan(geometry)?;
        self.apply_canvas_plan(plan, then)
    }

    /// The batch a canvas change needs. Exact moves of base images in its
    /// `remaps` must be completed on a worker before it applies.
    pub fn canvas_geometry_plan(
        &mut self,
        geometry: &layer_core::CanvasGeometry,
    ) -> Result<layer_core::CanvasGeometryPlan, layer_core::CanvasGeometryError> {
        self.flush_pending_edits()?;
        if self.has_active_stroke() {
            return Err(DocumentError::InvalidLayerOperation("Finish the stroke first").into());
        }
        self.document().canvas_geometry_plan(geometry, self.geometry_limits())
    }

    /// Apply a completed canvas plan, with `then` in the same undo step.
    pub fn apply_canvas_plan(
        &mut self,
        mut plan: layer_core::CanvasGeometryPlan,
        then: Vec<Edit>,
    ) -> Result<(), layer_core::CanvasGeometryError> {
        if !plan.remaps.is_empty() {
            return Err(layer_core::CanvasGeometryError::Unsupported("Move the layers' pixels before applying the canvas change"));
        }
        self.flush_pending_edits()?;
        if self.has_active_stroke() {
            return Err(DocumentError::InvalidLayerOperation("Finish the stroke first").into());
        }
        plan.edits.extend(then);
        if plan.operations.is_empty() {
            self.apply_edit(Edit::Batch(plan.edits))?;
        } else {
            for (_, operation) in &mut plan.operations {
                let target = self.allocate_coverage_handle();
                operation.coverage.target = target;
                operation.coverage.use_.source = target;
            }
            self.append_operations(plan.edits, plan.operations, None, true)?;
            self.rebuild_all = true;
            self.composite_all = true;
        }
        debug_assert!(self.document().extents_cover_canvas());
        Ok(())
    }

    /// Where the canvas origin moves if the next undo, or redo, runs.
    pub fn history_view_origin_shift(&self, redo: bool) -> Option<[i64; 2]> {
        self.editor.next_history_edit(redo).and_then(|edit| edit.view_origin_shift(self.document().working.view_origin))
    }

    pub fn move_layer(&mut self, id: layer_core::authored::OccurrenceHandle, to: usize) -> Result<(), DocumentError> {
        self.apply_edit(self.document().move_occurrence_edit(id, to)?)
    }

    pub fn set_active_layer(&mut self, id: layer_core::authored::OccurrenceHandle) -> Result<(), DocumentError> {
        self.apply_edit(self.document().select_occurrence_edit(id)?)
    }

    /// Readbacks must follow the frame that applies document edits, not capture
    /// old GPU pixels under the new document revision.
    pub fn has_pending_document_edits(&self) -> bool {
        self.pending_up.is_some() || self.pending_frame.is_some()
            || self.backend.has_pending_submission()
            || self.rebuild_all
            || self.rebuild_completed
            || self.composite_all
            || self.raster_dirty
            || self
                .batches
                .iter()
                .any(raster_edit_batch)
    }
    pub fn wants_continuous_frames(&self) -> bool {
        self.has_pending_input()
            || self.has_active_stroke()
            || self.has_pending_document_edits()
            || self.editor.document().has_animated_effects()
            || self.backend.has_pending_work()
    }
    pub fn has_pending_input(&self) -> bool {
        !self.input.is_empty()
    }

    pub fn set_layer_opacity(&mut self, id: layer_core::authored::OccurrenceHandle, opacity: f32) -> Result<(), DocumentError> {
        let mut occurrence = self.document().artwork.occurrences.get(id)
            .ok_or(DocumentError::InvalidLayerOperation("Missing layer"))?.clone();
        occurrence.opacity = opacity;
        self.apply_edit(Edit::Occurrence(layer_core::RecordChange::replace(&self.document().artwork.occurrences, id, Some(occurrence)).map_err(DocumentError::InvalidLayerOperation)?))
    }

    pub fn set_brush(&mut self, brush: BrushSnapshot) -> Result<(), BrushError> {
        brush.validate()?;
        self.settings.brush = brush;
        self.refresh_retouch();
        Ok(())
    }

    pub fn set_tool(&mut self, tool: StrokeTool) {
        self.settings.tool = tool;
    }

    /// Strokes copy from `source` while a retouching tool is selected; None
    /// returns to ordinary painting. The renderer prepares before pen-down.
    pub fn set_retouch(&mut self, source: Option<RetouchSource>) {
        self.settings.retouch = source;
        self.refresh_retouch();
    }

    /// Document points a retouching stroke is likely to sample next, such as
    /// its source and the hovering pen. The renderer may cache around them,
    /// so each is kept as the center of its raster tile.
    pub fn set_retouch_points(&mut self, points: &[layer_core::Point]) {
        let tile = layer_core::raster::TILE_SIZE as f32;
        let center = |v: f32| (v / tile).floor() * tile + tile / 2.;
        let points: Vec<_> = points.iter().map(|p| layer_core::Point { x: center(p.x), y: center(p.y) }).collect();
        if self.retouch_points != points {
            self.retouch_points = points;
            self.refresh_retouch();
        }
    }

    /// Where the Clone tool copies from. Strokes anchor and move an aligned
    /// source as they start and end.
    pub fn clone_source(&self) -> CloneSource {
        self.settings.clone_source
    }

    pub fn set_clone_source(&mut self, source: CloneSource) {
        self.settings.clone_generation = self.settings.clone_generation.wrapping_add(1);
        self.settings.clone_source = source;
    }

    /// The document offset the Clone stroke in contact copies with.
    pub fn clone_stroke_offset(&self) -> Option<[f32; 2]> {
        self.clone_stroke.filter(|_| self.active_stroke.is_some())
    }

    /// Tell the renderer what the selected retouching tool will sample, when
    /// that changed with the document, the source or the focus points.
    fn refresh_retouch(&mut self) {
        let document = self.editor.document();
        let fresh = self.prepared_retouch.as_ref().is_some_and(|(id, revision, prepared)| {
            *id == document.owner && *revision == document.revision && prepared.points == self.retouch_points
                && prepared.execution == self.settings.brush.execution
        });
        if self.settings.retouch.is_some() && fresh {
            return;
        }
        let prepared = self.settings.retouch.zip(document.try_drawing_content().ok()).map(|(source, target)| RetouchPreparation {
            execution: self.settings.brush.execution,
            target,
            retouch: Retouch::for_target(document, target, source),
            points: self.retouch_points.clone(),
        });
        if prepared.as_ref() == self.prepared_retouch.as_ref().map(|(.., p)| p) {
            return;
        }
        self.backend.prepare_retouch(prepared.as_ref());
        self.prepared_retouch = prepared.map(|p| (document.owner, document.revision, p));
    }

    /// Map a stroke that copies from the source point, starting at `first` in
    /// brush space shifted by the target's `offset`, to its source.
    fn begin_clone_stroke(&mut self, first: layer_core::Point, offset: layer_core::Point) {
        let Some(active) = self.active_stroke.as_mut().filter(|a| a.brush.execution.copies_from_source()) else {
            return;
        };
        active.clone_start = Some(self.settings.clone_source);
        let first = layer_core::Point { x: first.x + offset.x, y: first.y + offset.y };
        self.clone_stroke = self.settings.clone_source.begin_stroke(first);
        if let (Some(offset), Some(retouch)) = (self.clone_stroke, active.style.retouch.take()) {
            active.style.retouch = Some(clone_mapping(self.editor.document(), active.target, retouch, offset, self.settings.clone_source.flip));
        }
    }

    /// A corrected first point moves where the live Clone stroke anchors.
    fn reanchor_clone_stroke(&mut self) {
        let Some(active) = self.active_stroke.as_ref() else { return };
        if let Some(start) = active.clone_start {
            self.settings.clone_source = start;
            let first = self.builder.real_points()[0].position;
            self.begin_clone_stroke(first, layer_core::offsets::point(self.document().target_offset(active.target)));
        }
    }

    fn begin_completed_correction_window(&mut self) {
        let Some(publication) = self.completed_publication.as_ref() else { return; };
        let owned = self.completed_stroke.as_ref()
            .and_then(|stroke| self.document().target_raster(stroke.target))
            .is_some_and(|revision| revision.identity() == publication.identity());
        if !owned || publication.failed() {
            self.end_corrections();
        } else if publication.host_backed() {
            self.completed_publication = None;
            self.completed_at = Some(web_time::Instant::now());
        }
    }

    /// The completed stroke can no longer be corrected or replayed.
    fn end_corrections(&mut self) {
        self.completed_stroke = None;
        self.completed_publication = None;
        self.completed_before = None;
        self.completed_clone_start = None;
        self.completed_at = None;
        self.estimates.clear();
        if self.active_stroke.is_none() && self.pending_up.is_none() {
            self.backend.retire_stroke_sources();
        }
    }

    /// Replay retouching strokes whose source the renderer could not sample
    /// during contact: the live stroke once it ends, a completed one now.
    fn replay_retouch_misses(&mut self) -> Result<(), DocumentError> {
        while let Some(stroke) = self.backend.take_retouch_miss() {
            if let Some(active) = self.active_stroke.as_mut().filter(|a| a.id == stroke) {
                active.replay_after_contact = true;
            } else if self.completed_stroke.as_ref().is_some_and(|s| s.id == stroke) {
                self.replay_completed()?;
            }
        }
        Ok(())
    }

    /// Restore the completed stroke's layer and paint the stroke again, as a
    /// late correction does, amending its history entry. A Clone stroke maps
    /// its possibly corrected first point from the source it found.
    fn replay_completed(&mut self) -> Result<(), DocumentError> {
        if let (Some(mut start), Some(stroke)) = (self.completed_clone_start, self.completed_stroke.as_mut())
            && let (Some(retouch), Some(first)) = (stroke.retouch.take(), stroke.points.first())
        {
            let shift = layer_core::offsets::point(self.editor.document().target_offset(stroke.target));
            let first = layer_core::Point { x: first.position.x + shift.x, y: first.position.y + shift.y };
            let offset = start.begin_stroke(first).unwrap_or(retouch.offset);
            stroke.retouch = Some(clone_mapping(self.editor.document(), stroke.target, retouch, offset, start.flip));
        }
        let Some((stroke, _)) = self.completed_stroke.as_ref().zip(self.completed_before.as_ref()) else {
            return Ok(());
        };
        let layer = stroke.target;
        let publication = layer_core::raster::RasterRevision::pending();
        self.editor.amend(Edit::SetRaster { target: layer, revision: publication.clone() })?;
        self.completed_publication = Some(publication);
        self.rebuild_completed = true;
        self.completed_at = None;
        self.rebuild_all |= !self.backend.supports_raster_damage();
        Ok(())
    }

    /// Why a retouching stroke on `target` would copy nothing.
    fn retouch_refusal(&self, target: SourceTarget) -> Option<StrokeRefusal> {
        let source = self.settings.retouch?;
        let document = self.document();
        let layer = document.scene().paint(match target { SourceTarget::Paint(handle) => handle, _ => return None })?;
        if self.settings.brush.execution.copies_from_source() && self.settings.clone_source.point.is_none() {
            return Some(StrokeRefusal::NoCloneSource);
        }
        let empty = layer.base.is_none()
            && layer.raster.try_data().is_some_and(|data| data.is_ok_and(|data| data.tiles.is_empty()));
        (empty && Retouch::for_target(document, target, source).references.is_empty())
            .then_some(StrokeRefusal::EmptySource(source))
    }

    /// Why a stroke that starts with `event` would not paint as configured.
    /// Pen-down in `process_event` applies the same rule.
    pub fn stroke_refusal(&self, event: &PenEvent) -> Option<StrokeRefusal> {
        match self.stroke_target(self.stroke_tool(event)) {
            Err(refusal) => Some(refusal),
            Ok(target)
                if target.mask && self.settings.brush.execution_class() != BrushExecution::Dry =>
            {
                Some(StrokeRefusal::DryMask)
            }
            Ok(target) => self.retouch_refusal(target.target),
        }
    }

    fn stroke_tool(&self, event: &PenEvent) -> StrokeTool {
        if matches!(event.tool, ToolKind::Eraser) || event.flags.contains(SampleFlags::INVERTED) {
            StrokeTool::Eraser
        } else {
            self.settings.tool
        }
    }

    fn stroke_target(&self, tool: StrokeTool) -> Result<StrokeTarget, StrokeRefusal> {
        let document = self.document();
        let layer = if self.settings.retouch.is_some() {
            document.try_drawing_content()
        } else {
            document.try_drawing_target()
        }
        .map_err(StrokeRefusal::Target)?;
        let owner = document
            .target_owner(layer)
            .and_then(|owner| document.artwork.occurrences.get(owner))
            .ok_or(StrokeRefusal::Target(DrawingRefusal::NoLayer))?;
        let mask = matches!(layer, SourceTarget::Coverage(_));
        let alpha_locked = !mask && owner.alpha_locked;
        if alpha_locked && tool == StrokeTool::Eraser {
            return Err(StrokeRefusal::AlphaLocked);
        }
        Ok(StrokeTarget {
            target: layer,
            mask,
            alpha_locked,
            inverted: mask && owner.mask.as_ref().is_some_and(|m| m.inverted),
        })
    }

    pub fn record_raw_input(&mut self, event: PenEvent, transform: ViewTransform) {
        let transform = self
            .transforms
            .iter()
            .rev()
            .find(|t| t.revision == event.view_revision)
            .copied()
            .unwrap_or(transform);
        self.recording.raw(event, transform, self.pressure_for_event(event).clone());
    }
    fn pressure_for_event(&self, event: PenEvent) -> &PressureCurve {
        if event.flags.contains(SampleFlags::CORRECTION) {
            self.estimates.get(&(event.device_id, event.sequence)).map(|e| &e.curve)
        } else if event.phase != PenPhase::Hover {
            self.queued_contacts.iter().rev().find(|((device, _), _)| *device == event.device_id).map(|(_, s)| &s.pressure)
                .or_else(|| self.active_stroke.as_ref().and(self.contact_settings.as_ref()).map(|s| &s.pressure))
        } else { None }.unwrap_or(&self.settings.pressure)
    }

    pub fn configured_pressure_curve(&self) -> &PressureCurve { &self.settings.pressure }
    pub fn configured_tilt_curve(&self) -> &crate::TiltCurve { &self.settings.tilt }
    pub fn set_tilt_curve(&mut self, tilt: crate::TiltCurve) { self.settings.tilt = tilt; }
    fn tilt_for_event(&self, event: PenEvent) -> &crate::TiltCurve {
        if event.flags.contains(SampleFlags::CORRECTION) {
            self.estimates.get(&(event.device_id,event.sequence)).map(|e| &e.tilt)
        } else if event.phase != PenPhase::Hover {
            self.queued_contacts.iter().rev().find(|((device,_),_)| *device == event.device_id).map(|(_,s)| &s.tilt)
                .or_else(|| self.active_stroke.as_ref().and(self.contact_settings.as_ref()).map(|s| &s.tilt))
        } else { None }.unwrap_or(&self.settings.tilt)
    }
    pub(crate) fn instant_feedback(&self) -> InstantFeedbackConfig { self.settings.instant_feedback }

    pub fn set_pressure_curve(&mut self, pressure: PressureCurve) {
        self.settings.pressure = pressure;
    }

    /// UI supplies a logical hit distance converted into document units.
    /// A stroke's selected guide remains fixed until that stroke ends.
    pub fn set_ruler_snapping(&mut self, reach: Option<f32>) {
        self.settings.ruler_snapping = reach.filter(|r| r.is_finite() && *r >= 0.);
    }
    pub fn active_ruler_constraint(&self) -> Option<layer_core::RulerConstraint> {
        self.active_stroke.as_ref().and_then(|s| s.ruler)
    }

    pub fn set_instant_feedback(
        &mut self,
        config: InstantFeedbackConfig,
    ) -> Result<(), FeedbackConfigError> {
        config.validate()?;
        self.settings.instant_feedback = config;
        Ok(())
    }

    /// Both transforms must describe the same platform-view revision.
    pub fn set_view(&mut self, view: ViewState, input_transform: ViewTransform) {
        self.view = view;
        if self.transforms.back().map(|item| item.revision) != Some(input_transform.revision) {
            while self.input.is_empty() && self.transforms.len() >= TRANSFORM_HISTORY {
                self.transforms.pop_front();
            }
            self.transforms.push_back(input_transform);
        }
        // Camera movement only changes the viewport presentation. Persistent
        // canvas composition is document-space and remains reusable.
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), B::Error> {
        self.view.width_px = width;
        self.view.height_px = height;
        self.backend.resize_surface(width, height)?;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, DocumentError> {
        self.navigate(false)
    }

    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        self.navigate(true)
    }

    fn navigate(&mut self, redo: bool) -> Result<bool, DocumentError> {
        self.clear_liquify_preview();
        self.require_renderer_color(self.history_color(redo))?;
        self.flush_pending_edits()?;
        self.completed_stroke = None;
        self.completed_publication = None;
        self.completed_before = None;
        self.estimates.clear();
        let next = self.editor.next_history_edit(redo);
        let image = next.is_some_and(|edit| edit.changes_image(self.editor.document()));
        let raster_only = self.backend.supports_raster_damage() && next.is_some_and(|edit| edit.only_raster_updates());
        let frame = |document: &Document| (document.composition().size, document.working.view_origin,
            document.scene().targets().map(|target| document.scene().target_extent(target)).collect::<Vec<_>>());
        let before = frame(self.document());
        let rasters: std::collections::BTreeMap<_, _> = self.document().scene().targets().filter_map(|target|
            self.document().target_raster(target).map(|raster| (target, raster.clone()))).collect();
        let changed = if redo { self.editor.redo()? } else { self.editor.undo()? };
        if changed {
            let changed: Vec<_> = self.document().scene().targets().filter_map(|target| {
                let raster = self.document().target_raster(target)?;
                (rasters.get(&target) != Some(raster)).then(|| (target, raster.clone()))
            }).collect();
            let document = self.editor.document();
            self.restore_rasters.retain(|(target, _)| document.target_raster(*target).is_some() && !changed.iter().any(|(id, _)| id == target));
            self.restore_rasters.extend(changed);
        }
        self.transform_preview = None;
        self.rebuild_all |= changed && frame(self.document()) != before;
        self.composite_all |= changed && image && !raster_only;
        self.raster_dirty |= changed && raster_only;
        Ok(changed)
    }

    pub fn validate_edit(&self, edit: &Edit) -> Result<(), DocumentError> {
        self.editor.validate_edit(edit)
    }

    pub fn refine_selection(&mut self, target: layer_core::SelectionTarget, coverage: layer_core::Selection, revision: u64) -> Result<(), DocumentError> {
        self.flush_pending_edits()?;
        self.editor.refine_selection(target, coverage, revision)
    }

    pub fn withdraw_selection(&mut self, target: layer_core::SelectionTarget, revision: u64) -> Result<(), DocumentError> {
        self.flush_pending_edits()?;
        self.editor.withdraw_selection(target, revision)
    }

    fn require_renderer_color(&self, color: layer_core::color::DocumentColor) -> Result<(), DocumentError> {
        if color != self.document().composition().color || self.backend.document_color() != color {
            return Err(DocumentError::InvalidLayerOperation(
                "Prepare the matching renderer before applying document color or its history",
            ));
        }
        Ok(())
    }

    pub fn liquify_context(&self) -> Option<(SourceTarget, Arc<layer_core::liquify::Context>, bool)> {
        let document = self.document();
        let target = document.working.target?;
        let SourceTarget::Paint(handle) = target else { return None; };
        let owner = document.scene().source_owner(target)?;
        if document.is_locked(owner) { return None; }
        let alpha_locked = document.scene().occurrence(owner)?.alpha_locked;
        let context = layer_core::liquify::Context::for_source(document.scene().paint(handle)?, alpha_locked);
        context.field.is_some().then(|| (target, Arc::new(context), alpha_locked))
    }

    fn liquify_batch(&mut self, target: SourceTarget, context: &layer_core::liquify::Context,
        alpha_locked: bool, amount: f32, expected: &layer_core::raster::RasterRevision,
        kind: DabBatchKind) -> Result<DabBatch, DocumentError> {
        if !amount.is_finite() || !(0. ..=1.).contains(&amount) { return Err("Invalid Liquify amount".into()); }
        let document = self.document();
        let SourceTarget::Paint(handle) = target else { return Err("Liquify requires a paint source".into()); };
        let owner = document.scene().source_owner(target).ok_or("Missing Liquify target")?;
        let mut current = document.scene().paint(handle).ok_or("Missing Liquify target")?.clone();
        if document.working.target != Some(target) || document.is_locked(owner)
            || document.scene().occurrence(owner).is_none_or(|o| o.alpha_locked != alpha_locked)
            || current.raster.identity() != expected.identity() { return Err("Liquify target changed".into()); }
        current.raster = context.current.raster.clone();
        if current != *context.current { return Err("Liquify source changed".into()); }
        let field = context.field.as_ref().ok_or(DocumentError::InvalidLayerOperation("No Liquify changes"))?;
        let damage = field.tiles().iter().fold(Rect::EMPTY, |bounds, tile| {
            let min = layer_core::Point { x: (tile[0] * 256) as f32, y: (tile[1] * 256) as f32 };
            bounds.union(Rect { min, max: layer_core::Point { x: min.x + 256., y: min.y + 256. } })
        });
        let mut context = context.clone(); context.amount = Some(amount);
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::LiquifyReconstruct);
        let mut style = DabStyle::for_brush(&brush, StrokeTool::Brush);
        style.alpha_locked = alpha_locked;
        style.liquify = Some(Arc::new(context));
        Ok(DabBatch { material_update: 0, stroke_id: self.editor.allocate_stroke_id(), target, kind,
            stroke_start: true, stroke_end: kind == DabBatchKind::Persistent, first_dab: 0, dab_count: 0, style, damage })
    }

    pub fn preview_liquify(&mut self, target: SourceTarget, context: &layer_core::liquify::Context,
        alpha_locked: bool, amount: f32, expected: &layer_core::raster::RasterRevision) -> Result<(), DocumentError> {
        if self.active_stroke.is_some() || self.pending_up.is_some()
            || self.batches.iter().any(|batch| batch.kind != DabBatchKind::Preview)
            || self.pending_frame.is_some() && self.liquify_preview.is_none() {
            return Err(DocumentError::InvalidLayerOperation("Raster backing is busy; retry the edit"));
        }
        let batch = self.liquify_batch(target, context, alpha_locked, amount, expected, DabBatchKind::Preview)?;
        self.clear_liquify_preview();
        self.liquify_preview = Some(batch);
        self.raster_dirty = true;
        Ok(())
    }

    pub fn clear_liquify_preview(&mut self) {
        if let Some(preview) = self.liquify_preview.take() {
            self.batches.retain(|batch| batch.kind != DabBatchKind::Preview || batch.stroke_id != preview.stroke_id);
            if self.batches.is_empty() { self.pending_frame = None; }
            self.raster_dirty = true;
        }
    }

    pub fn adjust_liquify(&mut self, target: SourceTarget, context: &layer_core::liquify::Context,
        alpha_locked: bool, amount: f32, expected: &layer_core::raster::RasterRevision,
        amend: Option<u64>) -> Result<layer_core::raster::RasterRevision, DocumentError> {
        self.clear_liquify_preview();
        if amend != Some(self.checkpoint()) || self.pending_up.is_some() || self.pending_frame.is_some() {
            self.flush_pending_edits()?;
        }
        self.end_corrections();
        let batch = self.liquify_batch(target, context, alpha_locked, amount, expected, DabBatchKind::Persistent)?;
        let revision = layer_core::raster::RasterRevision::pending();
        let edit = Edit::SetRaster { target, revision: revision.clone() };
        if amend == Some(self.checkpoint()) {
            self.editor.amend(edit)?;
            self.batches.retain(|batch| batch.target != target
                || batch.style.liquify.as_ref().is_none_or(|context| context.amount.is_none()));
        } else { self.editor.perform(edit)?; }
        self.batches.push(batch);
        self.raster_dirty = true;
        Ok(revision)
    }

    pub fn apply_edit(&mut self, mut edit: Edit) -> Result<(), DocumentError> {
        self.clear_liquify_preview();
        self.require_renderer_color(edit.resulting_color(self.document().composition().color))?;
        fn source_operations(edit: &mut Edit, document: &Document, batches: &mut Vec<DabBatch>, replacements: &mut Vec<SourceTarget>) {
            let (target, domain, raster, operations) = match edit {
                Edit::Batch(edits) => {
                    for edit in edits { source_operations(edit, document, batches, replacements); }
                    return;
                }
                Edit::SetRaster { target, .. } => { replacements.push(*target); return; }
                Edit::Paint(change) => {
                    let Some(source) = change.value.as_mut() else { return; };
                    (SourceTarget::Paint(change.handle), source.domain, &mut source.raster, &mut source.operations)
                }
                Edit::Coverage(change) => {
                    let Some(source) = change.value.as_mut() else { return; };
                    (SourceTarget::Coverage(change.handle), source.domain, &mut source.raster, &mut source.operations)
                }
                _ => return,
            };
            if let Some(old) = document.target_operations(target) && operations.starts_with(old) {
                Arc::make_mut(operations).drain(..old.len());
            }
            if !operations.is_empty() {
                *raster = layer_core::raster::RasterRevision::pending();
                for (index, operation) in operations.iter().enumerate() {
                    batches.push(DabBatch {
                        material_update: 0, stroke_id: StrokeId(0), target,
                        kind: DabBatchKind::RasterOperation(index as u32),
                        stroke_start: false, stroke_end: false, first_dab: 0, dab_count: 0,
                        style: DabStyle::for_brush(&BrushSnapshot::default(), StrokeTool::Brush),
                        damage: operation.bounds(domain),
                    });
                }
            } else { replacements.push(target); }
        }
        let mut operation_batches = Vec::new();
        let mut replacements = Vec::new();
        source_operations(&mut edit, self.document(), &mut operation_batches, &mut replacements);
        self.flush_pending_edits()?;
        self.end_corrections();
        fn rebuild_needed(document: &Document, edit: &Edit) -> bool {
            match edit {
                Edit::Composition(change) => change.value.as_ref().is_none_or(|next| document.artwork.compositions.get(change.handle)
                    .is_none_or(|old| old.size != next.size || old.color != next.color)),
                Edit::Paint(change) => change.value.as_ref().is_some_and(|next| document.artwork.paint.get(change.handle)
                    .map_or(next.base.is_some(), |old| old.base != next.base || old.domain != next.domain)),
                Edit::Coverage(change) => change.value.as_ref().is_some_and(|next| document.artwork.coverage.get(change.handle)
                    .is_some_and(|old| old.domain != next.domain || old.default_coverage != next.default_coverage)),
                Edit::Batch(edits) => edits.iter().any(|e| rebuild_needed(document, e)),
                _ => false,
            }
        }
        let rebuild = rebuild_needed(self.document(), &edit);
        let changes_composite = edit.changes_image(self.editor.document());
        self.editor.perform(edit)?;
        for (target, raster) in &mut self.restore_rasters {
            if replacements.contains(target)
                && let Some(current) = self.editor.document().target_raster(*target)
                && current.try_data().is_some_and(|data| data.is_ok()) { *raster = current.clone(); }
        }
        self.batches.extend(operation_batches);
        self.transform_preview = None;
        self.rebuild_all |= rebuild;
        self.composite_all |= changes_composite;
        Ok(())
    }

    fn flush_pending_edits(&mut self) -> Result<(), DocumentError> {
        if self.pending_up.is_some() || self.backend.has_pending_submission() {
            return Err(DocumentError::InvalidLayerOperation("Raster backing is busy; retry the edit"));
        }
        if self.pending_frame.is_some()
            || self
                .batches
                .iter()
                .any(raster_edit_batch)
        {
            if !self.backend.can_submit() || !self.backend.can_capture_raster() {
                return Err(DocumentError::InvalidLayerOperation(
                    "Raster backing is busy; retry the edit",
                ));
            }
            self.render_frame().map_err(|_| {
                DocumentError::InvalidLayerOperation("Could not submit the preceding raster edit")
            })?;
            if self.pending_frame.is_some() {
                return Err(DocumentError::InvalidLayerOperation(
                    "Raster restoration is busy; retry the edit",
                ));
            }
        }
        Ok(())
    }

    /// Drain platform input, encode the newest incremental work, and return without
    /// waiting for GPU completion or presentation. Untimed calls publish pending
    /// pen-up boundaries; timed calls animate their settlement.
    pub fn render_frame(&mut self) -> Result<(), EngineError<B::Error>> {
        self.render_frame_inner(None, None)
    }

    /// Timed display-frame entry point. Native and Wasm frontends pass their
    /// monotonic display timestamp so stationary airbrushes advance without the
    /// shared core reading a platform clock.
    pub fn render_frame_at(&mut self, timestamp_ns: u64) -> Result<(), EngineError<B::Error>> {
        self.render_frame_inner(Some(timestamp_ns), None)
    }

    /// Frame entry point for adapters that know both the current monotonic time
    /// and the display's expected presentation time.
    pub fn render_frame_for(
        &mut self,
        timestamp_ns: u64,
        presentation_timestamp_ns: u64,
    ) -> Result<(), EngineError<B::Error>> {
        self.render_frame_inner(Some(timestamp_ns), Some(presentation_timestamp_ns))
    }

    fn render_frame_inner(
        &mut self,
        timestamp_ns: Option<u64>,
        presentation_timestamp_ns: Option<u64>,
    ) -> Result<(), EngineError<B::Error>> {
        self.frame_clock_ns = timestamp_ns;
        let view = self.view();
        self.backend.poll_pending(view).map_err(EngineError::Backend)?;
        if !self.backend.can_submit() {
            return Ok(());
        }
        self.begin_completed_correction_window();
        if let Some(frame) = self.pending_frame.take() {
            return self.submit_prepared_frame(frame);
        }
        if !self.backend.can_capture_raster()
            && (self.input.peek().is_some_and(|e| {
                e.phase == PenPhase::Up || e.flags.contains(SampleFlags::CORRECTION)
            }) || self.document().artwork.paint.iter().any(|(_, _, source)| source.raster.try_data().is_none())
                || self.document().artwork.coverage.iter().any(|(_, _, source)| source.raster.try_data().is_none()))
        {
            return Ok(());
        }
        self.replay_retouch_misses().map_err(EngineError::Document)?;
        if self
            .completed_at
            .is_some_and(|at| at.elapsed() >= CORRECTION_WINDOW)
            && self.completed_stroke.as_ref().map(|stroke| stroke.id) != self.backend.retouch_waiting()
        {
            self.end_corrections();
        }
        if self.settings.retouch.is_some() {
            self.refresh_retouch();
        }
        self.cancel_over_budget_contact();
        let mut rebuilt = false;
        if self.rebuild_all || self.rebuild_completed {
            rebuilt |= self.rebuild_all;
            self.build_full_scene();
            self.rebuild_all = false;
        }
        // Complete queued raster operations before starting another contact.
        if !self
            .batches
            .iter()
            .any(|b| matches!(b.kind, DabBatchKind::RasterOperation(_)))
        {
            self.process_input()?;
        }
        let awaiting_boundary = self
            .input
            .peek()
            .is_some_and(|e| e.phase == PenPhase::Up || e.flags.contains(SampleFlags::CORRECTION));
        if let Some(timestamp_ns) = timestamp_ns.filter(|_| !awaiting_boundary) {
            self.append_continuous(timestamp_ns);
        }
        self.advance_finalized_prefix();
        self.record_material_update();
        if self.rebuild_all || self.rebuild_completed {
            rebuilt |= self.rebuild_all;
            self.build_full_scene();
            self.rebuild_all = false;
        }
        if !awaiting_boundary {
            self.build_predicted_preview(timestamp_ns, presentation_timestamp_ns);
        }
        self.composite_all |= rebuilt;

        let time = timestamp_ns.filter(|now| *now != 0).map_or(self.animation_time, |now| {
            self.animation_seed + now.saturating_sub(*self.animation_origin_ns.get_or_insert(now)) as f32 * 1e-9
        });
        let steps = frame_steps::FrameSteps::new(self.document(), &self.batches, &self.dabs);
        self.submit_prepared_frame(PreparedFrame { reset: rebuilt, time, steps })
    }

    fn submit_prepared_frame(
        &mut self,
        frame: PreparedFrame,
    ) -> Result<(), EngineError<B::Error>> {
        let document = self.editor.document();
        let step = frame.steps.map(|steps| steps.next(document, &self.batches));
        let commit_rasters = step.as_ref().is_none_or(|(_, next)| next.is_none());
        let scene = self.scene_preview.as_ref().map_or_else(|| self.editor.document().scene(), |preview| preview.scene.view());
        let packet = FramePacket {
            time_seconds: frame.time,
            view: self.view(),
            document_extent: [self.editor.document().composition().size[0], self.editor.document().composition().size[1]],
            scene,
            inspect_mask: self.editor.document().working.inspect_mask,
            selection_overlays: Some(&self.editor.document().working.selection_overlays),
            dabs: &self.dabs,
            dab_batches: step.as_ref().map_or(&self.batches, |(batches, _)| batches),
            restore_rasters: &self.restore_rasters,
            reset_layers: frame.reset,
            commit_rasters,
            composite_all: self.composite_all,
            blend_space: self.editor.document().composition().blend,
        };
        if !self.backend.raster_dependencies_ready(packet, self.transform_preview.as_ref()) {
            self.pending_frame = Some(frame);
            return Ok(());
        }
        let selection = self.display_selection().map(|s| s.into_owned());
        self.backend.set_vector_selection(self.editor.document().working.vector_items.as_ref());
        let result = self
            .backend
            .set_selection_outline(selection.as_ref())
            .and_then(|_| {
                self.backend
                    .set_transform_preview(self.transform_preview.as_ref())
            })
            .and_then(|_| self.backend.submit(packet))
            .map_err(EngineError::Backend);

        self.metrics.frames = self.metrics.frames.saturating_add(1);
        if result.is_ok() && !commit_rasters {
            self.restore_rasters.clear();
            self.pending_frame = Some(PreparedFrame { reset: false, time: frame.time, steps: step.unwrap().1 });
            return Ok(());
        }
        self.dabs.clear();
        self.batches.clear();
        self.restore_rasters.clear();
        if result.is_err() {
            self.rebuild_all = true;
        } else {
            self.animation_time = frame.time;
            self.evaluation_context = self.backend.evaluation_context();
            self.composite_all = false;
            self.raster_dirty = false;
            self.editor.finish_raster_submission();
        }
        self.begin_completed_correction_window();
        result
    }

    fn append_continuous(&mut self, timestamp_ns: u64) {
        if self.pending_up.is_some() { return; }
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        if active.brush.path.continuous_rate_hz <= 0.0 {
            return;
        }
        let feedback = active.feedback.enabled;
        let id = active.id;
        if let Some(point) = self.builder.append_stationary(timestamp_ns) {
            self.recording
                .event(crate::recording::Event::Stationary(point.into()));
            let source = self.builder.last_real_source();
            for estimate in self.estimates.values_mut().filter(|e| {
                e.stroke == id
                    && (e.index == source || e.copies.iter().any(|(index, _)| *index == source))
            }) {
                estimate
                    .copies
                    .push((self.builder.real_points().len() - 1, point));
            }
            if !feedback {
                self.append_real_dab(point, self.builder.real_points().len() - 1, false);
            }
        }
    }

    fn record_material_update(&mut self) {
        let Some(active) = &mut self.active_stroke else {
            return;
        };
        if active.brush.execution != BrushExecution::Watercolor {
            return;
        }
        let end = if active.feedback.enabled {
            // Sensor latency may hold back persistent ink, but must not
            // change the watercolor update boundaries of the real samples.
            finalized_count(self.builder.real_points(), self.finalized_real_points)
        } else {
            self.builder.real_points().len()
        } as u32;
        if end > active.material_updates.last().copied().unwrap_or(0) {
            active.material_updates.push(end);
        }
    }

    fn advance_finalized_prefix(&mut self) {
        if self.pending_up.is_some() { return; }
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        if !active.feedback.enabled {
            return;
        }
        let real = self.builder.real_points();
        // A driver may never resolve its estimated sensor values. Give updates
        // the maximum feedback window, but never keep replaying the whole stroke
        // while waiting. Retain the tokens: late corrections rebuild persistent
        // ink through the existing correction path.
        let estimate_cutoff = real.last().map_or(0, |point| {
            point
                .elapsed_micros
                .saturating_sub(MAX_FINALIZATION_LAG_MICROS)
        });
        let before_estimate_window =
            real.partition_point(|point| point.elapsed_micros < estimate_cutoff);
        let count = finalized_count(real, self.finalized_real_points).min(
            self.estimates
                .values()
                .filter(|e| e.stroke == active.id)
                .map(|e| e.index)
                .min()
                .unwrap_or(usize::MAX)
                .max(before_estimate_window),
        );
        while self.finalized_real_points < count {
            let point = self.builder.real_points()[self.finalized_real_points];
            self.append_real_dab(point, self.finalized_real_points, false);
            self.finalized_real_points += 1;
        }
    }

    fn finalize_active_tail(&mut self) {
        while self.finalized_real_points < self.builder.real_points().len() {
            let point = self.builder.real_points()[self.finalized_real_points];
            let terminal = self.finalized_real_points + 1 == self.builder.real_points().len();
            self.append_real_dab(point, self.finalized_real_points, terminal);
            self.finalized_real_points += 1;
        }
    }

    /// Retire queued and active input after a renderer stops. Only already
    /// submitted raster boundaries belong to history; CPU-only sample draining
    /// cannot create a saveable raster edit. The producer must be quiescent.
    pub fn discard_unsubmitted_input(&mut self) {
        if self.pending_frame.take().is_some() {
            for root in self.document().artwork.paint.iter().map(|(_, _, source)| &source.raster)
                .chain(self.document().artwork.coverage.iter().map(|(_, _, source)| &source.raster))
            {
                if root.try_data().is_none() {
                    let _ = root.publish(Err("Renderer stopped before raster submission".into()));
                }
            }
            self.dabs.clear();
            self.batches.clear();
            self.restore_rasters.clear();
        }
        while self.input.pop().is_some() {}
        self.queued_contacts.clear();
        self.contact_settings = None;
        if self.active_stroke.is_some() {
            self.cancel_active();
        }
        self.pending_up = None;
        self.completed_stroke = None;
        self.completed_publication = None;
        self.completed_before = None;
        self.completed_at = None;
        self.rebuild_completed = false;
        self.estimates.clear();
    }

    /// Retired asynchronous frame producers have published their failures.
    /// Discard only the affected history suffix and restore the surviving roots.
    pub fn recover_failed_rasters(&mut self) -> Result<usize, DocumentError> {
        let discarded = self.editor.recover_failed_rasters()?;
        self.rebuild_all = true;
        self.composite_all = true;
        Ok(discarded)
    }

    pub fn capture_queued_contact(&mut self, event: PenEvent) {
        if event.phase == PenPhase::Down && !event.flags.contains(SampleFlags::CORRECTION) {
            self.queued_contacts.push_back(((event.device_id, event.sequence), self.settings.clone()));
        }
    }

    fn process_captured_event(&mut self, event: PenEvent) -> Result<(), EngineError<B::Error>> {
        if event.phase == PenPhase::Down && !event.flags.contains(SampleFlags::CORRECTION) {
            let key = (event.device_id, event.sequence);
            self.contact_settings = Some(if self.queued_contacts.front().is_some_and(|(found, _)| *found == key) {
                self.queued_contacts.pop_front().unwrap().1
            } else { self.settings.clone() });
        }
        let Some(mut settings) = self.contact_settings.take() else { return self.process_event(event); };
        std::mem::swap(&mut settings, &mut self.settings);
        let source = self.settings.clone_source;
        let result = self.process_event(event);
        if source != self.settings.clone_source {
            for (_, queued) in &mut self.queued_contacts {
                if queued.clone_generation == self.settings.clone_generation { queued.clone_source = self.settings.clone_source; }
            }
            if settings.clone_generation == self.settings.clone_generation { settings.clone_source = self.settings.clone_source; }
        }
        std::mem::swap(&mut settings, &mut self.settings);
        self.contact_settings = Some(settings);
        result
    }

    fn process_input(&mut self) -> Result<(), EngineError<B::Error>> {
        if let Some((up, _)) = self.pending_up {
            for _ in 0..INPUT_BATCH {
                if !self.input.peek().is_some_and(|event| event.phase == PenPhase::Hover || event.flags.contains(SampleFlags::CORRECTION)) { break; }
                if self.input.peek().is_some_and(|event| event.flags.contains(SampleFlags::CORRECTION)) && !self.backend.can_capture_raster() { break; }
                let event = self.input.pop().unwrap();
                self.process_captured_event(event)?;
            }
            let ready = self.pending_up.unwrap().1;
            let now = self.builder.elapsed_micros_at(self.frame_clock_ns.unwrap_or(up.timestamp_ns)).unwrap_or(ready);
            let active = self.active_stroke.as_ref().unwrap();
            let duration = active.momentum.as_ref().map_or_else(
                || pen_up_settlement_micros(&active.brush.stabilization),
                |(tail, _)| (tail.len() as u32 * 1_000_000).div_ceil(60));
            let interrupted = self.input.peek().is_some_and(|event| event.phase != PenPhase::Hover && !event.flags.contains(SampleFlags::CORRECTION));
            let finish = self.frame_clock_ns.is_none() || now.saturating_sub(ready) >= duration || interrupted
                || active.momentum.as_ref().is_some_and(|(tail, emitted)| *emitted == tail.len());
            self.advance_momentum(if finish { u32::MAX } else { now.saturating_sub(ready) });
            if finish && self.backend.can_capture_raster() {
                self.process_captured_event(up)?;
            }
            return Ok(());
        }
        for _ in 0..INPUT_BATCH {
            if self.input.peek().is_some_and(|e| {
                e.phase == PenPhase::Up || e.flags.contains(SampleFlags::CORRECTION)
            }) && !self.backend.can_capture_raster()
            {
                break;
            }
            let Some(event) = self.input.pop() else {
                break;
            };
            self.cancel_over_budget_contact();
            self.process_captured_event(event)?;
            // Publish one contact boundary before consuming the next contact.
            // This keeps each undo revision tied to its exact GPU queue point.
            if matches!(event.phase, PenPhase::Up | PenPhase::Cancel) {
                break;
            }
            if self.active_stroke.as_ref().is_some_and(|stroke| stroke.style.execution != BrushExecution::Dry && !stroke.style.execution.retouches()) {
                self.advance_finalized_prefix();
                if frame_steps::material_page_jobs(&self.batches) >= frame_steps::MATERIAL_PAGE_JOBS_PER_FRAME {
                    break;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn stroke_brush(&self, event: &PenEvent) -> BrushSnapshot {
        let mut brush = self.settings.brush.clone();
        brush.stabilization.input_scale = self.view.document_to_surface[0].hypot(self.view.document_to_surface[1]).max(f32::EPSILON);
        if !matches!(event.tool, ToolKind::Pen | ToolKind::Brush | ToolKind::Pencil | ToolKind::Airbrush) {
            brush.stabilization.pressure_fall_micros = 0;
        }
        brush
    }

    pub(crate) fn event_transform(&self, event: &PenEvent) -> ViewTransform {
        self.transforms.iter().rev().find(|item| item.revision == event.view_revision).copied()
            .unwrap_or_else(|| *self.transforms.back().expect("one transform is always retained"))
    }

    pub(crate) fn snap_to_ruler(&self, event: &PenEvent, transform: &mut ViewTransform, active: Option<layer_core::RulerConstraint>) -> Option<layer_core::RulerConstraint> {
        let point = transform.map(event.surface_position);
        let mut ruler = if event.phase == PenPhase::Down {
            self.settings.ruler_snapping.and_then(|reach| layer_core::choose_ruler(self.document().rulers(), point, reach))
        } else { active };
        if let Some(snap) = &mut ruler {
            if !event.flags.contains(SampleFlags::PREDICTED) { snap.resolve(point); }
            transform.surface_to_document = snap.transform(transform.surface_to_document);
        }
        ruler
    }

    fn process_event(&mut self, event: PenEvent) -> Result<(), EngineError<B::Error>> {
        if event.phase != PenPhase::Up || self.pending_up.is_none() || event.flags.contains(SampleFlags::CORRECTION) {
            self.metrics.input_events = self.metrics.input_events.saturating_add(1);
        }
        if matches!(event.phase, PenPhase::Down | PenPhase::Move) && !event.flags.contains(SampleFlags::PREDICTED) {
            self.metrics.last_consumed_paint_ns = self.metrics.last_consumed_paint_ns.max(event.timestamp_ns);
        }
        if event.flags.contains(SampleFlags::CORRECTION) {
            let changed = self.correct_input(event)?;
            if changed && let Some((_, ready)) = &mut self.pending_up {
                let now = self.builder.elapsed_micros_at(self.frame_clock_ns.unwrap_or(event.timestamp_ns)).unwrap();
                *ready = now;
            }
            return Ok(());
        }
        let mut transform = self.event_transform(&event);
        let ruler = self.snap_to_ruler(&event, &mut transform, self.active_ruler_constraint());
        if let Some(active) = &mut self.active_stroke {
            active.ruler = ruler;
        }
        let target_id = self
            .active_stroke
            .as_ref()
            .map_or_else(|| self.document().drawing_target().or(self.document().working.target), |s| Some(s.target));
        let offset = layer_core::offsets::point(target_id.map_or_default(|target| self.document().target_offset(target)));
        transform.surface_to_document[4] -= offset.x;
        transform.surface_to_document[5] -= offset.y;
        match event.phase {
            PenPhase::Down => {
                self.completed_stroke = None;
                self.completed_publication = None;
                self.estimates.clear();
                self.transform_preview = None;
                if self.active_stroke.is_some() {
                    self.cancel_active();
                }
                let mut tool = self.stroke_tool(&event);
                let Ok(StrokeTarget {
                    target,
                    mask: is_mask,
                    alpha_locked,
                    inverted,
                }) = self.stroke_target(tool)
                else {
                    return Ok(());
                };
                if self.retouch_refusal(target).is_some() {
                    return Ok(());
                }
                let id = self.editor.allocate_stroke_id();
                let mut brush = self.stroke_brush(&event);
                let mut feedback = self.settings.instant_feedback;
                if is_mask {
                    // Coverage brushes use the same tip/dynamics, not pigment or fluid state.
                    brush.color_rgba_linear = [1.0, 1.0, 1.0, brush.color_rgba_linear[3]];
                    brush.color_dynamics = Default::default();
                    brush.execution = BrushExecution::Dry;
                    if !brush.contact.is_some_and(|c| c.dry.is_some()) {
                        brush.grain = None;
                        if let Some(contact) = &mut brush.contact { contact.paper = 0.; }
                        brush.rendering = Default::default();
                    }
                    brush.transport = None;
                    brush.wet_mix = Default::default();
                    brush.deform = Default::default();
                    feedback.enabled = false;
                    brush.stabilization.strength = 0.;
                    if inverted {
                        tool = if tool == StrokeTool::Brush {
                            StrokeTool::Eraser
                        } else {
                            StrokeTool::Brush
                        };
                    }
                }
                let mut style = DabStyle::for_brush(&brush, tool);
                style.alpha_locked = alpha_locked;
                if brush.execution == BrushExecution::Liquify
                    && let SourceTarget::Paint(handle) = target
                    && let Some(source) = self.document().scene().paint(handle)
                {
                    style.liquify = Some(std::sync::Arc::new(layer_core::liquify::Context::for_source(source, alpha_locked)));
                    if brush.deform.mode == layer_core::LiquifyMode::Reconstruct && style.liquify.as_ref().is_some_and(|c| c.field.is_none()) { return Ok(()); }
                }
                style.blend_space = if is_mask { layer_core::BlendSpace::Linear } else { self.document().composition().blend };
                style.paper = style.grain.is_some().then(|| self.document().paper_binding(target));
                style.retouch = self.settings.retouch.map(|source| Retouch::for_target(self.document(), target, source));
                style.selection = self.document().working.selection.as_ref().map(|selection| {
                    std::sync::Arc::new(selection.translated(layer_core::offsets::point(self.document().target_offset(target).map(|v| -v))))
                });
                let active = ActiveStroke {
                    paint_color: (!is_mask
                        && tool == StrokeTool::Brush
                        && !matches!(brush.execution, BrushExecution::Smudge | BrushExecution::Liquify)
                        && !brush.execution.retouches()
                        && brush.opacity > 0.
                        && brush.flow > 0.
                        && brush.color_rgba_linear[3] > 0.)
                        .then_some(self.settings.paint_color)
                        .flatten(),
                    before: self.document().target_raster(target).unwrap().clone(),
                    id,
                    target,
                    tool,
                    brush,
                    style,
                    feedback,
                    tip: PreviewTip::default(),
                    persistent_started: false,
                    committed_chunk_dabs: 0,
                    material_updates: Vec::new(),
                    ruler,
                    replay_after_contact: false,
                    clone_start: None,
                    barrel_twist: event.flags.contains(SampleFlags::BARREL_TWIST),
                    momentum: None,
                };
                self.active_stroke = Some(active);
                self.recording.begin(
                    event.timestamp_ns,
                    crate::recording::Policy {
                        config: feedback,
                        transform: self.view.document_to_surface,
                    },
                );
                if feedback.enabled {
                    self.recording.observe(event);
                    self.active_stroke
                        .as_mut()
                        .unwrap()
                        .tip
                        .prediction
                        .observe(event);
                }
                self.pending_chunk_dabs.clear();
                self.finalized_real_points = 0;
                self.builder.begin_calibrated(event, transform, self.settings.pressure.clone(), self.settings.tilt.clone());
                self.record_builder_sample(event);
                self.track_estimate(event, transform);
                let active = self.active_stroke.as_ref().expect("set above");
                self.dab_generator.reset_for_stroke(id, &active.brush);
                self.dab_generator.set_barrel_twist(active.barrel_twist);
                let point = *self
                    .builder
                    .real_points()
                    .last()
                    .expect("begin adds a point");
                self.begin_clone_stroke(point.position, offset);
                if !self
                    .active_stroke
                    .as_ref()
                    .expect("set above")
                    .feedback.enabled
                {
                    self.append_real_dab(point, self.builder.real_points().len() - 1, false);
                }
            }
            PenPhase::Move => {
                if self.active_stroke.is_none() {
                    return Ok(());
                }
                self.builder.push(event, transform, self.settings.pressure.clone());
                self.record_builder_sample(event);
                let active = self.active_stroke.as_mut().unwrap();
                if active.feedback.enabled {
                    self.recording.observe(event);
                    active.tip.prediction.observe(event);
                }
                self.track_estimate(event, transform);
                if !event.flags.contains(SampleFlags::PREDICTED)
                    && !self
                        .active_stroke
                        .as_ref()
                        .expect("checked above")
                        .feedback.enabled
                {
                    let point = *self
                        .builder
                        .real_points()
                        .last()
                        .expect("real move adds a point");
                    self.append_real_dab(point, self.builder.real_points().len() - 1, false);
                }
            }
            PenPhase::Up => {
                if self.active_stroke.is_none() {
                    return Ok(());
                }
                let momentum = self.active_stroke.as_ref().unwrap().momentum.is_some();
                if self.pending_up.take().is_none() && !momentum {
                    self.builder.push(event, transform, self.settings.pressure.clone());
                    self.record_builder_sample(event);
                    self.track_estimate(event, transform);
                    let active = self.active_stroke.as_ref().unwrap();
                    if self.frame_clock_ns.is_some() && pen_up_settlement_micros(&active.brush.stabilization) > 0 && active.tip.curve_preview.is_present() {
                        let now = self.builder.elapsed_micros_at(self.frame_clock_ns.unwrap_or(event.timestamp_ns)).unwrap();
                        self.pending_up = Some((event, now));
                        return Ok(());
                    }
                }
                if !momentum && !event.flags.contains(SampleFlags::PREDICTED) {
                    if self
                        .active_stroke
                        .as_ref()
                        .expect("checked above")
                        .feedback.enabled
                    {
                        self.finalize_active_tail();
                    } else {
                        let point = *self.builder.real_points().last().expect("up adds a point");
                        self.append_real_dab(point, self.builder.real_points().len() - 1, true);
                    }
                    let tail = self.dab_generator.momentum(&self.active_stroke.as_ref().unwrap().brush);
                    if !tail.is_empty() {
                        self.active_stroke.as_mut().unwrap().momentum = Some((tail, 0));
                        if let Some(now) = self.frame_clock_ns {
                            self.pending_up = Some((event, self.builder.elapsed_micros_at(now).unwrap()));
                            return Ok(());
                        }
                        self.advance_momentum(u32::MAX);
                    }
                }
                self.flush_material_chunks(true);
                self.finish_persistent_stroke();
                self.record_material_update();
                self.replay_retouch_misses().map_err(EngineError::Document)?;
                let active = self.active_stroke.take().expect("checked above");
                self.recording.end(false);
                let points = self.builder.finish().unwrap_or_default();
                if self.clone_stroke.take().is_some()
                    && let Some(last) = points.last()
                {
                    self.settings.clone_source
                        .end_stroke(layer_core::Point { x: last.position.x + offset.x, y: last.position.y + offset.y });
                }
                if active.brush.execution == BrushExecution::Liquify && !active.persistent_started {
                    self.batches.retain(|b| b.stroke_id != active.id);
                    self.finalized_real_points = 0;
                    return Ok(());
                }
                let replay = active.brush.taper.end_distance_diameters > 0.0 || active.replay_after_contact;
                let alpha_locked = active.style.alpha_locked;
                let mut stroke = Stroke::new(
                    active.id,
                    active.target,
                    active.tool,
                    active.brush,
                    points,
                )
                .map_err(EngineError::Document)?;
                stroke.alpha_locked = alpha_locked;
                stroke.blend_space = active.style.blend_space;
                stroke.barrel_twist = active.barrel_twist;
                stroke.material_updates = active.material_updates.into();
                stroke.selection = active.style.selection.clone();
                stroke.retouch = active.style.retouch.clone();
                stroke.paper = active.style.paper.clone();
                self.completed_stroke = Some(stroke);
                let publication = layer_core::raster::RasterRevision::pending();
                self.completed_at = None;
                self.completed_before = Some(active.before);
                self.completed_clone_start = active.clone_start;
                self.editor
                    .perform(Edit::SetRaster {
                        target: active.target,
                        revision: publication.clone(),
                    })
                    .map_err(EngineError::Document)?;
                self.completed_publication = Some(publication);
                if active.persistent_started
                    && let Some(color) = active.paint_color
                {
                    self.used_colors.retain(|c| *c != color);
                    if self.used_colors.len() == 64 {
                        self.used_colors.remove(0);
                    }
                    self.used_colors.push(color);
                }
                if replay {
                    // End taper depends on final stroke length, and a retouch
                    // source missed during contact can wait now. Replay after
                    // pen-up so the stored stroke and visible result agree.
                    self.rebuild_all |= !self.backend.supports_raster_damage();
                    self.rebuild_completed = true;
                }
                self.metrics.committed_strokes = self.metrics.committed_strokes.saturating_add(1);
                self.finalized_real_points = 0;
            }
            PenPhase::Cancel => self.cancel_active(),
            PenPhase::Hover => {}
        }
        Ok(())
    }

    fn append_real_dab(
        &mut self,
        point: layer_core::StrokePoint,
        point_index: usize,
        terminal: bool,
    ) {
        let active = self.active_stroke.as_ref().expect("stroke is active");
        if matches!(active.style.execution, BrushExecution::Smudge | BrushExecution::Liquify) {
            self.dab_generator
                .append(point, &active.brush, &mut self.pending_chunk_dabs);
            if terminal { self.dab_generator.finish_contact(&active.brush, &mut self.pending_chunk_dabs); }
            self.flush_material_chunks(false);
            return;
        }
        let stroke_id = active.id;
        let target = active.target;
        let style = active.style.clone();
        let stroke_start = !active.persistent_started;
        let material_update = active
            .material_updates
            .partition_point(|end| *end as usize <= point_index)
            as u32;
        let start = self.dabs.len();
        let mut damage = self
            .dab_generator
            .append(point, &active.brush, &mut self.dabs);
        if terminal {
            damage = damage.union(self.dab_generator.finish(&active.brush, &mut self.dabs));
        }
        if self.dabs.len() > start {
            self.active_stroke
                .as_mut()
                .expect("stroke remains active")
                .persistent_started = true;
            push_batches(
                &mut self.batches,
                &self.dabs,
                DabBatch {
                    material_update,
                    stroke_id,
                    target,
                    kind: DabBatchKind::Persistent,
                    stroke_start,
                    stroke_end: false,
                    first_dab: start.min(u32::MAX as usize) as u32,
                    dab_count: self.dabs.len().saturating_sub(start).min(u32::MAX as usize) as u32,
                    style,
                    damage,
                },
            );
        }
    }

    fn advance_momentum(&mut self, elapsed_micros: u32) {
        let Some((tail, emitted)) = self.active_stroke.as_mut().and_then(|a| a.momentum.as_mut()) else { return; };
        let end = ((u64::from(elapsed_micros) * 60 / 1_000_000) as usize).min(tail.len()).max(*emitted);
        self.pending_chunk_dabs.extend_from_slice(&tail[*emitted..end]);
        *emitted = end;
        self.flush_material_chunks(false);
    }

    pub fn complete_liquify_momentum(&mut self) {
        self.advance_momentum(u32::MAX);
    }

    fn flush_material_chunks(&mut self, flush_tail: bool) {
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        if !matches!(active.style.execution, BrushExecution::Smudge | BrushExecution::Liquify) {
            return;
        }
        let stroke_id = active.id;
        let target = active.target;
        let style = active.style.clone();
        let liquify = active.style.execution == BrushExecution::Liquify;

        let mut consumed = 0;
        loop {
            let pending = &self.pending_chunk_dabs[consumed..];
            let chunk_len = if liquify { pending.len().min(layer_core::liquify::CHUNK_DABS) } else { next_smudge_chunk_len(pending, DabBatchKind::Persistent, false) };
            if chunk_len == 0 {
                break;
            }
            let reached_boundary =
                chunk_len == if liquify { layer_core::liquify::CHUNK_DABS } else { MAX_SMUDGE_DABS_PER_BATCH } || chunk_len < pending.len();
            if !flush_tail && !reached_boundary {
                break;
            }

            let stroke_start = !self
                .active_stroke
                .as_ref()
                .expect("stroke remains active")
                .persistent_started;
            let start = self.dabs.len();
            self.dabs.extend_from_slice(&pending[..chunk_len]);
            consumed += chunk_len;
            let damage = damage_for_dabs(&self.dabs[start..]);
            push_batches(
                &mut self.batches,
                &self.dabs,
                DabBatch {
                    material_update: 0,
                    stroke_id,
                    target,
                    kind: DabBatchKind::Persistent,
                    stroke_start,
                    stroke_end: false,
                    first_dab: start.min(u32::MAX as usize) as u32,
                    dab_count: chunk_len.min(u32::MAX as usize) as u32,
                    style: style.clone(),
                    damage,
                },
            );
            let active = self.active_stroke.as_mut().expect("stroke remains active");
            active.persistent_started = true;
            active.committed_chunk_dabs = active.committed_chunk_dabs.saturating_add(chunk_len);
        }
        if consumed > 0 {
            // One compaction after the event keeps a long coalesced segment
            // linear; draining each tiny chunk would repeatedly shift the
            // unconsumed tail.
            self.pending_chunk_dabs.drain(..consumed);
        }
    }

    fn finish_persistent_stroke(&mut self) {
        let active = self.active_stroke.as_ref().expect("stroke is active");
        if let Some(batch) =
            self.batches.iter_mut().rev().find(|batch| {
                batch.stroke_id == active.id && batch.kind == DabBatchKind::Persistent
            })
        {
            batch.stroke_end = true;
            return;
        }
        // Preserve pen-up even when spacing emitted no contact in this frame.
        // Stateful renderers still need to finalize accumulated stroke state.
        self.batches.push(DabBatch {
            material_update: 0,
            stroke_id: active.id,
            target: active.target,
            kind: DabBatchKind::Persistent,
            stroke_start: !active.persistent_started,
            stroke_end: true,
            first_dab: self.dabs.len().min(u32::MAX as usize) as u32,
            dab_count: 0,
            style: active.style.clone(),
            damage: Rect::EMPTY,
        });
    }

    fn build_predicted_preview(
        &mut self,
        timestamp_ns: Option<u64>,
        presentation_timestamp_ns: Option<u64>,
    ) {
        if let Some(batch) = &self.liquify_preview { self.batches.push(batch.clone()); }
        let Some(active) = self.active_stroke.as_mut() else {
            return;
        };
        let start = self.dabs.len();
        if matches!(active.style.execution, BrushExecution::Smudge | BrushExecution::Liquify) {
            self.dabs.extend_from_slice(&self.pending_chunk_dabs);
        }
        if active.momentum.is_some() {
            self.push_active_preview(start);
            return;
        }
        if !active.feedback.enabled {
            // Only the current pending contact is provisional; pressure
            // limiting needs no retained input window or endpoint prediction.
            self.dab_generator
                .preview()
                .flush_contact(&active.brush, &mut self.dabs);
            self.push_active_preview(start);
            return;
        }
        let latest = match self.builder.real_points().last().copied() {
            Some(point) => point,
            None => {
                self.push_active_preview(start);
                return;
            }
        };
        let (now_elapsed, requested_elapsed) = tip_times(active.feedback, latest,
            |timestamp| self.builder.elapsed_micros_at(timestamp).unwrap_or(latest.elapsed_micros), timestamp_ns, presentation_timestamp_ns);
        self.recording.query(
            crate::recording::Policy {
                config: active.feedback,
                transform: self.view.document_to_surface,
            },
            now_elapsed,
            requested_elapsed,
        );
        let input = TipInput {
            real: self.builder.real_points(), pending: &self.builder.real_points()[self.finalized_real_points..],
            platform: self.builder.predicted_points(), brush: &active.brush, feedback: active.feedback,
            transform: self.view.document_to_surface, now: now_elapsed, requested: requested_elapsed,
            ending: self.pending_up.map(|(_, ready)| ready),
        };
        let source = active.tip.build(&self.dab_generator, input, start, &mut self.dabs);
        self.push_active_preview(start);
        match source {
            Some(TipSource::Platform) => self.metrics.platform_prediction_frames = self.metrics.platform_prediction_frames.saturating_add(1),
            Some(TipSource::Engine) => self.metrics.engine_prediction_frames = self.metrics.engine_prediction_frames.saturating_add(1),
            _ => {}
        }
    }

    fn push_active_preview(&mut self, start: usize) {
        if self.active_stroke.as_ref().is_some_and(|active| active.brush.contact.is_some_and(|c| c.dry.is_some())) {
            self.dabs.extract_if(start.., |dab| dab.motion == [0.;2]).for_each(drop);
        }
        let active = self.active_stroke.as_ref().expect("stroke is active");
        if self.dabs.len() == start && !(active.persistent_started && active.style.execution == BrushExecution::Liquify) {
            return;
        }
        let damage = damage_for_dabs(&self.dabs[start..]);
        push_batches(
            &mut self.batches,
            &self.dabs,
            DabBatch {
                material_update: 0,
                stroke_id: active.id,
                target: active.target,
                kind: DabBatchKind::Preview,
                stroke_start: !active.persistent_started,
                stroke_end: false,
                first_dab: start.min(u32::MAX as usize) as u32,
                dab_count: self.dabs.len().saturating_sub(start).min(u32::MAX as usize) as u32,
                style: active.style.clone(),
                damage,
            },
        );
    }

    fn build_full_scene(&mut self) {
        if self.active_stroke.is_none() && !self.rebuild_completed {
            return;
        }
        self.dabs.clear();
        self.batches.clear();
        if self.rebuild_completed
            && let Some(stroke) = self.completed_stroke.as_ref()
        {
            self.rebuild_completed = false;
            if let Some(before) = &self.completed_before {
                self.restore_rasters.push((stroke.target, before.clone()));
            }
            let mut style = DabStyle::for_brush(&stroke.brush, stroke.tool);
            style.alpha_locked = stroke.alpha_locked;
            if stroke.brush.execution == BrushExecution::Liquify
                && let SourceTarget::Paint(handle) = stroke.target
                && let Some(source) = self.document().scene().paint(handle)
            {
                let mut source = source.clone();
                if let Some(before) = &self.completed_before { source.raster = before.clone(); }
                style.liquify = Some(std::sync::Arc::new(layer_core::liquify::Context::for_source(&source, stroke.alpha_locked)));
            }
            style.blend_space = stroke.blend_space;
            style.selection = stroke.selection.clone();
            style.retouch = stroke.retouch.clone();
            style.paper = stroke.paper.clone();
            let mut generator = DabGenerator::new(self.document().composition().color.space);
            generator.reset_for_replay(stroke);
            let mut started = false;
            let liquify = stroke.brush.execution == BrushExecution::Liquify;
            for (point_index, point) in stroke.points.iter().copied().enumerate() {
                let start = if liquify { 0 } else { self.dabs.len() };
                let mut damage = generator.append(point, &stroke.brush, &mut self.dabs);
                if point_index + 1 == stroke.points.len() {
                    damage = damage.union(generator.finish(&stroke.brush, &mut self.dabs));
                }
                if liquify {
                    if point_index + 1 != stroke.points.len() { continue; }
                    damage = damage_for_dabs(&self.dabs);
                }
                if self.dabs.len() == start {
                    continue;
                }
                push_batches(
                    &mut self.batches,
                    &self.dabs,
                    DabBatch {
                        material_update: stroke
                            .material_updates
                            .partition_point(|end| *end as usize <= point_index)
                            as u32,
                        stroke_id: stroke.id,
                        target: stroke.target,
                        kind: DabBatchKind::Persistent,
                        stroke_start: !started,
                        stroke_end: false,
                        first_dab: start.min(u32::MAX as usize) as u32,
                        dab_count: self.dabs.len().saturating_sub(start).min(u32::MAX as usize)
                            as u32,
                        style: style.clone(),
                        damage,
                    },
                );
                started = true;
            }
            if let Some(batch) = self
                .batches
                .iter_mut()
                .rev()
                .find(|batch| batch.stroke_id == stroke.id)
            {
                batch.stroke_end = true;
            }
        }
        if let Some(active) = self.active_stroke.as_ref() {
            let mut generator = DabGenerator::new(self.document().composition().color.space);
            generator.reset_for_stroke(active.id, &active.brush);
            generator.set_barrel_twist(active.barrel_twist);
            let point_count = if active.feedback.enabled {
                self.finalized_real_points
            } else {
                self.builder.real_points().len()
            };
            if matches!(active.style.execution, BrushExecution::Smudge | BrushExecution::Liquify) {
                let mut replay_dabs = Vec::new();
                for point in self.builder.real_points()[..point_count].iter().copied() {
                    generator.append(point, &active.brush, &mut replay_dabs);
                }
                if let Some((tail, emitted)) = &active.momentum {
                    generator.finish_contact(&active.brush, &mut replay_dabs);
                    replay_dabs.extend_from_slice(&tail[..*emitted]);
                }
                replay_dabs.truncate(active.committed_chunk_dabs.min(replay_dabs.len()));
                if !replay_dabs.is_empty() {
                    let start = self.dabs.len();
                    self.dabs.extend_from_slice(&replay_dabs);
                    let damage = damage_for_dabs(&self.dabs[start..]);
                    push_batches(
                        &mut self.batches,
                        &self.dabs,
                        DabBatch {
                            material_update: 0,
                            stroke_id: active.id,
                            target: active.target,
                            kind: DabBatchKind::Persistent,
                            stroke_start: true,
                            stroke_end: false,
                            first_dab: start.min(u32::MAX as usize) as u32,
                            dab_count: replay_dabs.len().min(u32::MAX as usize) as u32,
                            style: active.style.clone(),
                            damage,
                        },
                    );
                }
                return;
            }
            let mut started = false;
            for (point_index, point) in self.builder.real_points()[..point_count]
                .iter()
                .copied()
                .enumerate()
            {
                let start = self.dabs.len();
                let damage = generator.append(point, &active.brush, &mut self.dabs);
                if self.dabs.len() == start {
                    continue;
                }
                push_batches(
                    &mut self.batches,
                    &self.dabs,
                    DabBatch {
                        material_update: active
                            .material_updates
                            .partition_point(|end| *end as usize <= point_index)
                            as u32,
                        stroke_id: active.id,
                        target: active.target,
                        kind: DabBatchKind::Persistent,
                        stroke_start: !started,
                        stroke_end: false,
                        first_dab: start.min(u32::MAX as usize) as u32,
                        dab_count: self.dabs.len().saturating_sub(start).min(u32::MAX as usize)
                            as u32,
                        style: active.style.clone(),
                        damage,
                    },
                );
                started = true;
            }
        }
    }

    fn record_builder_sample(&mut self, event: PenEvent) {
        use crate::recording::Event;
        if event.flags.contains(SampleFlags::PREDICTED) {
            if let Some(&p) = self.builder.predicted_points().last() {
                self.recording.event(Event::Predicted(p.into()));
            }
        } else if let Some(&p) = self.builder.real_points().last() {
            self.recording.event(Event::Sample(p.into()));
        }
    }

    fn cancel_active(&mut self) {
        self.pending_up = None;
        self.recording.end(true);
        self.dabs.clear();
        self.batches.clear();
        if let Some(active) = &self.active_stroke {
            self.estimates.retain(|_, e| e.stroke != active.id);
        }
        self.builder.cancel();
        self.active_stroke = None;
        self.clone_stroke = None;
        self.pending_chunk_dabs.clear();
        self.finalized_real_points = 0;
        self.dab_generator.reset();
        self.rebuild_all = true;
    }

    fn cancel_over_budget_contact(&mut self) {
        if self.active_stroke.is_some() && self.builder.real_points().len()>=MAX_CONTACT_POINTS {
            self.cancel_active();
            self.metrics.input_budget_cancellations=self.metrics.input_budget_cancellations.saturating_add(1);
        }
    }
}

/// `retouch` copying through the document offset `offset`, in `layer`'s pixels.
fn clone_mapping(document: &Document, layer: SourceTarget, retouch: Retouch, offset: [f32; 2], flip: [bool; 2]) -> Retouch {
    retouch.cloning(offset, flip, layer_core::offsets::point(document.target_offset(layer)))
}

fn raster_edit_batch(batch: &DabBatch) -> bool {
    matches!(batch.kind, DabBatchKind::RasterOperation(_))
        || batch.kind == DabBatchKind::Persistent && batch.style.liquify.as_ref().is_some_and(|context| context.amount.is_some())
}

fn push_batches(batches: &mut Vec<DabBatch>, dabs: &[Dab], batch: DabBatch) {
    if batch.style.execution == BrushExecution::Smudge {
        let original_first = batch.first_dab;
        let original_end = original_first.saturating_add(batch.dab_count);
        for first_dab in original_first..original_end {
            let index = first_dab as usize;
            push_mergeable_batch(
                batches,
                dabs,
                DabBatch {
                    stroke_start: batch.stroke_start && first_dab == original_first,
                    stroke_end: batch.stroke_end && first_dab + 1 == original_end,
                    first_dab,
                    dab_count: 1,
                    damage: damage_for_dabs(&dabs[index..index.saturating_add(1).min(dabs.len())]),
                    ..batch.clone()
                },
            );
        }
        return;
    }
    let max_dabs = match (batch.style.execution, batch.kind) {
        // Prediction is a short disposable tail. Evaluating its watercolor
        // contacts together avoids repeated full-page color/coverage copies;
        // persistent replay keeps the fixed microbatch boundary below.
        (BrushExecution::Watercolor, DabBatchKind::Preview) => u32::MAX,
        (BrushExecution::Dry | BrushExecution::Clone | BrushExecution::Heal | BrushExecution::SpotHeal, _) => u32::MAX,
        (BrushExecution::Wet | BrushExecution::Watercolor, _) => MAX_WET_DABS_PER_BATCH,
        (BrushExecution::Liquify, DabBatchKind::Preview) => u32::MAX,
        (BrushExecution::Liquify, _) => layer_core::liquify::CHUNK_DABS as u32,
        (BrushExecution::Smudge, _) => unreachable!("handled above"),
    };
    if batch.dab_count <= max_dabs {
        push_mergeable_batch(batches, dabs, batch);
        return;
    }

    let original_first = batch.first_dab;
    let original_end = original_first.saturating_add(batch.dab_count);
    let mut first_dab = original_first;
    while first_dab < original_end {
        let dab_count = (original_end - first_dab).min(max_dabs);
        let start = first_dab as usize;
        let end = start.saturating_add(dab_count as usize).min(dabs.len());
        push_mergeable_batch(
            batches,
            dabs,
            DabBatch {
                stroke_start: batch.stroke_start && first_dab == original_first,
                stroke_end: batch.stroke_end && first_dab + dab_count == original_end,
                first_dab,
                dab_count,
                damage: damage_for_dabs(&dabs[start..end]),
                ..batch.clone()
            },
        );
        first_dab += dab_count;
    }
}

fn push_mergeable_batch(batches: &mut Vec<DabBatch>, dabs: &[Dab], batch: DabBatch) {
    if let Some(last) = batches.last_mut()
        && (batch.style.execution == BrushExecution::Dry
            || batch.style.execution.retouches()
            || (matches!(
                batch.style.execution,
                BrushExecution::Wet | BrushExecution::Watercolor
            ) && last.dab_count.saturating_add(batch.dab_count) <= MAX_WET_DABS_PER_BATCH)
            || (batch.style.execution == BrushExecution::Smudge
                && smudge_batch_fits(
                    &dabs[last.first_dab as usize
                        ..batch.first_dab.saturating_add(batch.dab_count) as usize],
                    batch.kind,
                    batch.style.blender.kind == layer_core::BlenderKind::Dither,
                )))
        && last.stroke_id == batch.stroke_id
        && last.material_update == batch.material_update
        && last.target == batch.target
        && last.kind == batch.kind
        && !last.stroke_end
        && last.first_dab.saturating_add(last.dab_count) == batch.first_dab
        && last.style == batch.style
        && (batch.style.blender.kind != layer_core::BlenderKind::Dither || batch.style.execution != BrushExecution::Smudge
            || frame_steps::covered_tiles(last.damage.union(batch.damage)).saturating_mul(
                last.dab_count.saturating_add(batch.dab_count).div_ceil(MAX_SMUDGE_DABS_PER_BATCH as u32)) <= frame_steps::MATERIAL_PAGE_JOBS_PER_FRAME)
    {
        last.dab_count = batch
            .first_dab
            .saturating_add(batch.dab_count)
            .saturating_sub(last.first_dab);
        last.damage = last.damage.union(batch.damage);
        last.stroke_end = batch.stroke_end;
        return;
    }
    batches.push(batch);
}

/// Committed chunks keep a fixed size so replays split a stroke the same way;
/// the disposable prediction tail takes as few passes as the travel limit allows.
fn next_smudge_chunk_len(dabs: &[Dab], kind: DabBatchKind, dither: bool) -> usize {
    let (most, diameters) = if dither { (MAX_DITHER_DABS_PER_BATCH, f32::INFINITY) }
        else if kind == DabBatchKind::Preview { (usize::MAX, f32::INFINITY) }
        else { (MAX_SMUDGE_DABS_PER_BATCH, MAX_SMUDGE_TRAVEL_DIAMETERS) };
    let mut damage = Rect::EMPTY;
    let mut travel = 0.0;
    let mut maximum_diameter = 0.0_f32;
    for (index, dab) in dabs.iter().take(most).enumerate() {
        let candidate_damage = damage.union(damage_for_dabs(std::slice::from_ref(dab)));
        let candidate_travel = travel + dab.motion[0].hypot(dab.motion[1]);
        let candidate_diameter = maximum_diameter.max(dab.radii[0].max(dab.radii[1]) * 2.0);
        if index > 0
            && ((!dither && candidate_travel > (candidate_diameter.max(0.01) * diameters).min(layer_core::SMUDGE_CHUNK_TRAVEL_LIMIT))
                || rect_area(candidate_damage)
                    > candidate_diameter.max(0.01).powi(2) * MAX_SMUDGE_DAMAGE_DIAMETERS_SQUARED)
        {
            return index;
        }
        damage = candidate_damage;
        travel = candidate_travel;
        maximum_diameter = candidate_diameter;
    }
    dabs.len().min(most)
}

fn smudge_batch_fits(dabs: &[Dab], kind: DabBatchKind, dither: bool) -> bool {
    !dabs.is_empty() && next_smudge_chunk_len(dabs, kind, dither) == dabs.len()
}

fn rect_area(rect: Rect) -> f32 {
    if rect.is_empty() {
        0.0
    } else {
        (rect.max.x - rect.min.x).max(0.0) * (rect.max.y - rect.min.y).max(0.0)
    }
}

#[derive(Debug)]
pub enum EngineError<E> {
    Document(DocumentError),
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for EngineError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => write!(formatter, "document edit failed: {error}"),
            Self::Backend(error) => write!(formatter, "canvas renderer failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for EngineError<E> {}

impl<E> From<DocumentError> for EngineError<E> {
    fn from(error: DocumentError) -> Self { Self::Document(error) }
}

#[cfg(test)]
mod tests {
    include!("dry_tap_tests.rs");
    include!("canvas_fullscreen_tests.rs");
    include!("recording/canvas_tests.rs");
    use super::*;
    use crate::brush::dabs_cover_point;
    fn assert_authored_eq(left: &layer_core::authored::Artwork, right: &layer_core::authored::Artwork) {
        assert_eq!(left.id, right.id);
        assert_eq!(left.root, right.root);
        assert_eq!(left.default_output, right.default_output);
        assert_eq!(left.metadata, right.metadata);
        assert_eq!(left.extensions, right.extensions);
        macro_rules! records { ($($field:ident),*) => { $(assert_eq!(left.$field.iter().collect::<Vec<_>>(), right.$field.iter().collect::<Vec<_>>()));* }; }
        records!(compositions, stacks, occurrences, paint, coverage, effects, selections, guides, outputs);
    }
    fn active_paint(document: &Document) -> &layer_core::authored::PaintSource {
        let SourceTarget::Paint(handle) = document.working.target.unwrap() else { panic!("paint target"); };
        document.artwork.paint.get(handle).unwrap()
    }
    fn active_paint_mut(document: &mut Document) -> &mut layer_core::authored::PaintSource {
        let SourceTarget::Paint(handle) = document.working.target.unwrap() else { panic!("paint target"); };
        document.artwork.paint.get_mut(handle).unwrap()
    }
    fn composition_mut(document: &mut Document) -> &mut layer_core::authored::Composition {
        document.artwork.compositions.get_mut(document.artwork.root).unwrap()
    }
    fn active_occurrence(document: &Document) -> &layer_core::authored::Occurrence {
        document.artwork.occurrences.get(document.working.occurrence.unwrap()).unwrap()
    }
    fn active_occurrence_mut(document: &mut Document) -> &mut layer_core::authored::Occurrence {
        document.artwork.occurrences.get_mut(document.working.occurrence.unwrap()).unwrap()
    }
    fn paint_insert(document: &Document, source: layer_core::authored::PaintSource, name: &str, position: usize) -> (layer_core::authored::OccurrenceHandle, SourceTarget, Vec<Edit>) {
        let paint = layer_core::RecordChange::insert(&document.artwork.paint, source);
        let target = SourceTarget::Paint(paint.handle);
        let occurrence = layer_core::RecordChange::insert(&document.artwork.occurrences,
            layer_core::authored::Occurrence::new(layer_core::authored::OccurrenceContent::Paint(paint.handle), name));
        let handle = occurrence.handle;
        let stack = document.composition().result;
        let mut entries = document.artwork.stacks.get(stack).unwrap().clone(); entries.entries.insert(document.content_insertion(None, position).0, handle);
        (handle, target, vec![Edit::Paint(paint), Edit::Occurrence(occurrence),
            Edit::Stack(layer_core::RecordChange::replace(&document.artwork.stacks, stack, Some(entries)).unwrap())])
    }
    fn empty_paint(document: &Document) -> layer_core::authored::PaintSource {
        layer_core::authored::PaintSource { color_mode:Default::default(), domain:document.composition().size, raster:Default::default(), base:None, operations:Arc::default() }
    }
    fn mask_edit(engine: &mut CanvasEngine<RecordingRenderer>, offset: [i64; 2]) -> Edit {
        let mut candidate = engine.document().clone();
        let handle = candidate.allocate_coverage_handle();
        let coverage = layer_core::CoverageSnapshot::reveal_all(handle, candidate.composition().size, offset);
        let mut occurrence = active_occurrence(&candidate).clone(); occurrence.mask = Some(coverage.use_);
        Edit::Batch(vec![Edit::Coverage(layer_core::RecordChange::insert(&engine.document().artwork.coverage, coverage.source)),
            replace_occurrence(engine.document(), occurrence)])
    }
    fn photo_handle() -> layer_core::authored::OccurrenceHandle { layer_core::authored::OccurrenceHandle::from_index(2) }
    fn replace_occurrence(document: &Document, value: layer_core::authored::Occurrence) -> Edit {
        Edit::Occurrence(layer_core::RecordChange::replace(&document.artwork.occurrences, document.working.occurrence.unwrap(), Some(value)).unwrap())
    }
    fn references(document: &Document, members: std::collections::BTreeSet<layer_core::authored::OccurrenceHandle>) -> Edit {
        Edit::Batch(document.artwork.occurrences.iter().filter_map(|(handle, _, occurrence)| {
            let reference = members.contains(&handle);
            (reference != occurrence.reference).then(|| {
                let mut value = occurrence.clone(); value.reference = reference;
                Edit::Occurrence(layer_core::RecordChange::replace(&document.artwork.occurrences, handle, Some(value)).unwrap())
            })
        }).collect())
    }
    fn selection_edit(document: &Document, selection: Option<layer_core::Selection>) -> Edit {
        let mut working = document.working.clone(); working.selection = selection; Edit::Working(working)
    }
    fn mask_target_edit(document: &Document, mask: bool) -> Edit {
        let mut working = document.working.clone();
        working.target = if mask { Some(SourceTarget::Coverage(active_occurrence(document).mask.as_ref().unwrap().source)) }
            else { document.scene().source_target(working.occurrence.unwrap()) };
        Edit::Working(working)
    }
    fn add_mask(document: &mut Document, offset: [i64; 2]) -> layer_core::authored::CoverageHandle {
        let coverage = layer_core::CoverageSnapshot::reveal_all(document.allocate_coverage_handle(), document.composition().size, offset);
        let target = coverage.target;
        document.artwork.coverage.install(target, coverage.source).unwrap();
        let mut occurrence = active_occurrence(document).clone(); occurrence.mask = Some(coverage.use_);
        document.apply(replace_occurrence(document, occurrence)).unwrap();
        target
    }
    fn rulers_edit(document: &Document, rulers: Vec<layer_core::Ruler>) -> Edit {
        let value = layer_core::authored::Guides { rulers:rulers.into_iter().map(|r| (r.id,r.geometry)).collect() };
        if let Some((handle, _, _)) = document.artwork.guides.iter().next() {
            Edit::Guides(layer_core::RecordChange::replace(&document.artwork.guides, handle, Some(value)).unwrap())
        } else { Edit::Guides(layer_core::RecordChange::insert(&document.artwork.guides, value)) }
    }
    fn color_edit(document: &Document, color: layer_core::color::DocumentColor) -> Edit {
        let mut composition = document.composition().clone(); composition.color = color;
        Edit::Composition(layer_core::RecordChange::replace(&document.artwork.compositions, document.artwork.root, Some(composition)).unwrap())
    }

    use crate::feedback::surface_distance;
    use crate::input::{InputProducer, SampleFlags, ToolKind, input_queue};
    use crate::test_support::{event, view};
    use layer_core::{AssetId, DefaultBrushPreset, Point, default_brush};
    use layer_render::{BackendError, CanvasRenderer, FramePacket, HostImage};

    const TRANSFORM: ViewTransform = ViewTransform {
        revision: 1,
        ..ViewTransform::IDENTITY
    };

    fn engine(
        _name: &str,
        width: u32,
        height: u32,
    ) -> (InputProducer<PenEvent>, CanvasEngine<RecordingRenderer>) {
        engine_with(
            RecordingRenderer::default(),
            Document::new(layer_core::authored::PortableId::random(), width, height, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            view(width, height),
            TRANSFORM,
        )
    }

    #[test]
    fn liquify_zero_dab_preview_continues_only_after_persistent_work_starts() {
        let (mut input, mut engine) = engine("liquify zero dab preview", 256, 128);
        engine.set_brush(default_brush(DefaultBrushPreset::LiquifyPush)).unwrap();
        input.push(event(1, PenPhase::Down, 32.)).unwrap();
        engine.render_frame_at(10_000_000).unwrap();
        assert!(engine.active_stroke.is_some());

        engine.dabs.clear();
        engine.pending_chunk_dabs.clear();
        engine.batches.clear();
        engine.active_stroke.as_mut().unwrap().persistent_started = false;
        engine.push_active_preview(0);
        assert!(engine.batches.is_empty());

        engine.active_stroke.as_mut().unwrap().persistent_started = true;
        engine.push_active_preview(0);
        assert_eq!(engine.batches.len(), 1);
        assert_eq!(engine.batches[0].kind, DabBatchKind::Preview);
        assert_eq!(engine.batches[0].dab_count, 0);
        assert!(!engine.batches[0].stroke_start);
        assert!(!engine.batches[0].stroke_end);

        engine.batches.clear();
        engine.active_stroke.take();
        engine.build_predicted_preview(None, None);
        assert!(engine.batches.is_empty());
    }

    #[derive(Debug)]
    struct PreviewField;
    impl layer_core::liquify::Field for PreviewField {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn resources(&self, _: &mut dyn FnMut(u64, usize)) {}
        fn tiles(&self) -> Vec<[u32; 2]> { vec![[0, 0]] }
    }

    #[test]
    fn liquify_amount_preview_rejects_other_raster_work() {
        let (mut input, mut engine) = engine("liquify amount busy", 256, 128);
        let target = engine.document().working.target.unwrap();
        let SourceTarget::Paint(handle) = target else { panic!() };
        let source = engine.document().scene().paint(handle).unwrap().clone();
        let mut original = source.clone();
        original.raster = original.raster.without_liquify();
        source.raster.set_liquify(Arc::new(layer_core::liquify::State::new(
            Arc::new(original), Arc::new(PreviewField), &source, false)));
        let (_, context, alpha_locked) = engine.liquify_context().unwrap();
        let expected = source.raster.clone();
        engine.preview_liquify(target, &context, alpha_locked, 0.5, &expected).unwrap();
        engine.clear_liquify_preview();

        let other = engine.liquify_batch(target, &context, alpha_locked, 0.5, &expected, DabBatchKind::Persistent).unwrap();
        engine.batches.push(other);
        assert!(engine.preview_liquify(target, &context, alpha_locked, 0.4, &expected).is_err());
        assert!(engine.liquify_preview.is_none());
        engine.batches.clear();

        engine.pending_frame = Some(PreparedFrame { reset: false, time: 0., steps: None });
        assert!(engine.preview_liquify(target, &context, alpha_locked, 0.4, &expected).is_err());
        assert!(engine.liquify_preview.is_none());
        engine.pending_frame = None;

        engine.set_brush(default_brush(DefaultBrushPreset::LiquifyPush)).unwrap();
        input.push(event(1, PenPhase::Down, 32.)).unwrap();
        engine.render_frame_at(10_000_000).unwrap();
        assert!(engine.active_stroke.is_some());
        assert!(engine.preview_liquify(target, &context, alpha_locked, 0.4, &expected).is_err());
        assert!(engine.liquify_preview.is_none());
    }

    #[test]
    fn liquify_momentum_is_one_stroke_and_matches_replay_at_every_frame_rate() {
        for preset in [DefaultBrushPreset::LiquifyPush, DefaultBrushPreset::LiquifyTwirl,
            DefaultBrushPreset::LiquifyCrystals, DefaultBrushPreset::LiquifyEdge] {
            let mut expected = None;
            for (interval, interrupt) in [(8_333_333, false), (33_333_333, false), (16_666_667, true)] {
                let (mut input, mut engine) = engine("momentum", 256, 128);
                let mut brush = default_brush(preset);
                brush.diameter = 64.; brush.deform.momentum = 0.5;
                engine.set_brush(brush).unwrap();
                engine.settings.instant_feedback.enabled = false;
                for (sequence, phase, x) in [(1, PenPhase::Down, 32.), (21, PenPhase::Move, 64.),
                    (41, PenPhase::Move, 96.), (42, PenPhase::Up, 96.)] {
                    input.push(event(sequence, phase, x)).unwrap();
                }
                engine.render_frame_at(42_000_000).unwrap();
                assert!(engine.active_stroke.as_ref().unwrap().momentum.is_some(), "{preset:?}");
                assert_eq!(engine.metrics.committed_strokes, 0);
                if interrupt { engine.complete_liquify_momentum(); }
                let mut now = 42_000_000;
                while engine.has_active_stroke() {
                    now += interval;
                    assert!(now < 1_000_000_000);
                    engine.render_frame_at(now).unwrap();
                }
                assert_eq!(engine.metrics.committed_strokes, 1);
                let mut replay = Vec::new();
                DabGenerator::generate(engine.completed_stroke.as_ref().unwrap(),
                    engine.document().composition().color.space, &mut replay);
                assert_eq!(engine.backend().persistent, replay, "{preset:?}, {interval}");
                if let Some(previous) = &expected { assert_eq!(&replay, previous, "{preset:?}"); }
                expected = Some(replay);
                assert!(engine.undo().unwrap());
                assert!(!engine.can_undo());
            }
        }
    }

    #[test]
    fn liquify_momentum_cancellation_and_sensor_correction_rebuild_the_tail() {
        for cancel in [false, true] {
            let (mut input, mut engine) = engine("momentum correction", 256, 128);
            let mut brush = default_brush(DefaultBrushPreset::LiquifyPush);
            brush.diameter = 64.; brush.deform.momentum = 1.;
            engine.set_brush(brush).unwrap();
            engine.settings.instant_feedback.enabled = false;
            input.push(event(1, PenPhase::Down, 32.)).unwrap();
            let mut sample = event(21, PenPhase::Move, 96.);
            sample.flags = SampleFlags::ESTIMATED;
            input.push(sample).unwrap();
            input.push(event(22, PenPhase::Up, 96.)).unwrap();
            engine.render_frame_at(22_000_000).unwrap();
            engine.render_frame_at(122_000_000).unwrap();
            if cancel {
                input.push(event(123, PenPhase::Cancel, 96.)).unwrap();
                engine.render_frame().unwrap();
                engine.render_frame().unwrap();
                assert!(!engine.has_active_stroke());
                assert_eq!(engine.metrics.committed_strokes, 1);
            } else {
                sample.flags = SampleFlags::CORRECTION;
                sample.pressure = 0.4;
                input.push(sample).unwrap();
                engine.render_frame_at(140_000_000).unwrap();
                engine.render_frame().unwrap();
                let mut replay = Vec::new();
                DabGenerator::generate(engine.completed_stroke.as_ref().unwrap(),
                    engine.document().composition().color.space, &mut replay);
                assert_eq!(engine.backend().persistent, replay);
            }
        }
    }

    #[test]
    fn capture_uses_successful_renderer_phases_and_seeds_replacement() {
        use layer_core::authored::{EffectApplication, EvaluationContext, PortableId};
        let mut document = Document::new(PortableId::random(), 64, 64, layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let program = layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program();
        let values = layer_core::EffectInstance::new(program.clone()).values;

        let effect = document.artwork.effects.insert(PortableId::random(), EffectApplication::new(program, values, document.composition().size)).unwrap();
        let saved = EvaluationContext {elapsed:3.,phases:vec![(effect,7.)].into()};
        document.artwork.outputs.get_mut(document.artwork.default_output).unwrap().context = saved.clone();
        let (_, mut canvas) = engine_with(RecordingRenderer::default(), document, view(64,64), TRANSFORM);
        assert_eq!(canvas.backend().evaluation_context(), saved);
        canvas.backend_mut().phases = vec![(effect,-2.)];
        canvas.render_frame_at(1_000_000_000).unwrap();
        let captured = canvas.capture_artwork(9).unwrap();
        assert_eq!(captured.output().context.phases.as_slice(), &[(effect,-2.)]);
        assert_eq!(captured.checkpoint.session_generation,9);
        assert_eq!(captured.checkpoint.owner,canvas.document().owner);
        assert_eq!(captured.artwork.paint,canvas.document().artwork.paint);
        assert_eq!(canvas.document().output().context,saved);
        canvas.backend_mut().fail_submit = true;
        assert!(canvas.render_frame_at(2_000_000_000).is_err());
        assert_eq!(canvas.capture_artwork(9).unwrap().output().context,captured.output().context);
        canvas.replace_backend(RecordingRenderer::default()).unwrap();
        assert_eq!(canvas.backend().evaluation_context(),captured.output().context);
        assert_eq!(canvas.scene_snapshot().context,captured.output().context);
    }

    #[test]
    fn restored_editor_keeps_redo_and_checkpoint_without_rendering() {
        let (_, source) = engine("Session history", 64, 64);
        let mut editor = Editor::new(source.document().clone());
        let mut occurrence = active_occurrence(editor.document()).clone();
        occurrence.opacity = 0.25;
        editor.perform(replace_occurrence(editor.document(), occurrence)).unwrap();
        let changed = editor.checkpoint();
        editor.undo().unwrap();
        let (_, mut restored) = engine_with(RecordingRenderer::default(), editor.document().clone(), view(64,64), TRANSFORM);
        restored.restore_editor(editor).unwrap();
        assert_eq!(restored.checkpoint(), 0);
        assert!(restored.can_redo());
        assert_eq!(restored.metrics().frames, 0);
        restored.redo().unwrap();
        assert_eq!(restored.checkpoint(), changed);
        assert_eq!(active_occurrence(restored.document()).opacity, 0.25);
        restored.render_frame().unwrap();
        restored.undo().unwrap();
        assert_eq!(restored.checkpoint(), 0);
        assert_eq!(active_occurrence(restored.document()).opacity, 1.);
    }

    #[test]
    fn exhausting_contact_input_cancels_only_that_contact_and_keeps_rendering() {
        for remaining in [0,1] {
            let (mut input,mut canvas)=engine("Input budget",64,64);
            for (sequence,phase) in [(1,PenPhase::Down),(2,PenPhase::Up)] {
                input.push(event(sequence,phase,16.)).unwrap();canvas.render_frame().unwrap();
            }
            let document=canvas.document().clone();let checkpoint=canvas.checkpoint();
            input.push(event(3,PenPhase::Down,24.)).unwrap();canvas.render_frame().unwrap();
            for sequence in 4..(MAX_CONTACT_POINTS-remaining+3) {
                canvas.builder.push(event(sequence as u64,PenPhase::Move,24.),TRANSFORM,PressureCurve::default());
            }
            if remaining!=0 {
                for sequence in [200_000,200_001] {input.push(event(sequence,PenPhase::Move,24.)).unwrap();}
            }
            canvas.render_frame().expect("a contact limit cannot stop the canvas");
            input.push(event(200_002,PenPhase::Up,24.)).unwrap();canvas.render_frame().unwrap();
            assert!(!canvas.has_active_stroke());
            assert_eq!(canvas.metrics().input_budget_cancellations,1);
            assert_eq!(canvas.checkpoint(),checkpoint);
            assert_eq!(canvas.document().artwork,document.artwork);
            assert_eq!(canvas.document().working,document.working);
            for (sequence,phase) in [(200_003,PenPhase::Down),(200_004,PenPhase::Up)] {
                input.push(event(sequence,phase,32.)).unwrap();canvas.render_frame().unwrap();
            }
            assert_eq!(canvas.metrics().committed_strokes,2);
            canvas.undo().unwrap();assert_eq!(canvas.checkpoint(),checkpoint);
            canvas.redo().unwrap();assert!(canvas.capture_artwork(0).is_ok());
        }
    }

    #[test]
    fn restoring_an_editor_rejects_another_document_or_queued_input() {
        let (mut input, mut canvas) = engine("Session restore fence", 64, 64);
        let document = canvas.document().clone();
        let (_, different) = engine("Other session", 64, 64);
        assert!(canvas.restore_editor(Editor::new(different.document().clone())).is_err());
        assert_eq!(canvas.document(), &document);
        input.push(event(1, PenPhase::Down, 16.)).unwrap();
        assert!(canvas.restore_editor(Editor::new(document.clone())).is_err());
        assert_eq!(canvas.document(), &document);
        assert!(canvas.has_pending_input());
    }

    #[test]
    fn contact_preview_and_camera_wait_do_not_replace_the_capture_boundary() {
        let (mut input, mut canvas) = engine("Capture boundary",64,64);
        canvas.render_frame_at(1_000_000_000).unwrap();
        let before = canvas.capture_artwork(3).unwrap();
        input.push(event(1, PenPhase::Down, 16.)).unwrap();
        canvas.backend_mut().restore_blocked = true;
        canvas.render_frame_at(2_000_000_000).unwrap();
        assert!(canvas.has_active_stroke());
        let contact = canvas.capture_artwork(3).unwrap();
        assert_eq!(before.artwork.paint,contact.artwork.paint);
        assert_eq!(before.output().context,contact.output().context);
    }

    #[test]
    fn frame_time_stays_precise_after_untimed_preparation_and_flushes() {
        let (_, mut engine) = engine("Frame timing", 64, 64);
        engine.render_frame_for(0, 0).unwrap();
        let uptime = 30 * 24 * 60 * 60 * 1_000_000_000u64;
        for step in 0..12 {
            engine.render_frame_at(uptime + step * 16_666_667).unwrap();
            let time = engine.backend().time_seconds;
            assert!((time - step as f32 / 60.).abs() < 1e-6, "frame {step}: {time}");
            engine.render_frame().unwrap();
            assert_eq!(engine.backend().time_seconds, time);
            engine.render_frame_for(0, 0).unwrap();
            assert_eq!(engine.backend().time_seconds, time);
        }
    }

    fn engine_with(
        renderer: RecordingRenderer,
        document: Document,
        view: ViewState,
        transform: ViewTransform,
    ) -> (InputProducer<PenEvent>, CanvasEngine<RecordingRenderer>) {
        let (input, consumer) = input_queue(2 * INPUT_BATCH);
        let engine = CanvasEngine::new(renderer, document, consumer, view, transform).unwrap();
        (input, engine)
    }

    #[derive(Default)]
    struct RecordingRenderer {
        color: layer_core::color::DocumentColor,
        prepared_color: Option<layer_core::color::DocumentColor>,
        fail_color_adoption: bool,
        color_adoptions: usize,
        capture_blocked: bool,
        restore_blocked: bool,
        unrelated_submission: bool,
        time_seconds: f32,
        phases: Vec<(layer_core::authored::EffectHandle, f32)>,
        fail_submit: bool,
        fail_resize: bool,
        size: [u32; 2],
        persistent_dabs: usize,
        persistent: Vec<Dab>,
        persistent_batches: Vec<(StrokeId, bool, bool, u32)>,
        material_page_jobs: u32,
        material_batches: Vec<(u32, u32)>,
        operation_batches: Vec<(SourceTarget, u32, layer_core::Rect, bool)>,
        restored_rasters: Vec<(SourceTarget, u64)>,
        preview: Vec<Dab>,
        styles: Vec<DabStyle>,
        saw_reset: bool,
        transform: Option<layer_render::TransformPreview>,
        visibility: Vec<(layer_core::authored::OccurrenceHandle, bool)>,
        bake_members: Vec<Vec<layer_core::authored::OccurrenceHandle>>,
        retouch: Vec<Option<RetouchPreparation>>,
        retouch_misses: Vec<StrokeId>,
        retouch_waiting: Option<StrokeId>,
        retired_sources: usize,
    }

    impl CanvasRenderer for RecordingRenderer {
        type Error = BackendError;
        fn evaluation_context(&self) -> layer_core::authored::EvaluationContext {
            layer_core::authored::EvaluationContext { elapsed:self.time_seconds, phases:self.phases.clone().into() }
        }
        fn seed_evaluation_context(&mut self, context: layer_core::authored::EvaluationContext) {
            self.time_seconds = context.elapsed;
            self.phases = context.phases.as_ref().clone();
        }
        fn document_color(&self) -> layer_core::color::DocumentColor {
            self.color
        }
        fn adopt_prepared_color(&mut self, color: layer_core::color::DocumentColor) -> Result<bool, Self::Error> {
            self.color_adoptions += 1;
            if self.fail_color_adoption { return Err(BackendError("color adoption failed")); }
            if self.prepared_color != Some(color) { return Ok(false); }
            self.color = color;
            self.prepared_color = None;
            Ok(true)
        }
        fn poll_pending(&mut self, _view: ViewState) -> Result<(), Self::Error> {
            self.unrelated_submission = false;
            Ok(())
        }
        fn has_pending_submission(&self) -> bool { self.unrelated_submission }
        fn raster_dependencies_ready(&mut self, _packet: FramePacket<'_>, _preview: Option<&layer_render::TransformPreview>) -> bool {
            !self.restore_blocked
        }
        fn can_capture_raster(&self) -> bool {
            !self.capture_blocked
        }
        fn prepare_retouch(&mut self, retouch: Option<&RetouchPreparation>) {
            self.retouch.push(retouch.cloned());
        }
        fn take_retouch_miss(&mut self) -> Option<StrokeId> {
            self.retouch_misses.pop()
        }
        fn retouch_waiting(&self) -> Option<StrokeId> {
            self.retouch_waiting
        }
        fn retire_stroke_sources(&mut self) {
            self.retired_sources += 1;
        }
        fn set_transform_preview(
            &mut self,
            preview: Option<&layer_render::TransformPreview>,
        ) -> Result<(), Self::Error> {
            self.transform = preview.cloned();
            Ok(())
        }

        fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
            if self.fail_resize {
                return Err(BackendError("replacement resize failed"));
            }
            self.size = [width, height];
            Ok(())
        }

        fn prepare_asset(
            &mut self,
            _asset: &AssetId,
            _image: HostImage<'_>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn release_asset(&mut self, _asset: &AssetId) {}

        fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
            if self.fail_submit { return Err(BackendError("submission failed")); }
            self.restored_rasters = packet.restore_rasters.iter().map(|(target, raster)| (*target, raster.identity())).collect();
            self.time_seconds = packet.time_seconds;
            self.bake_members = packet.dab_batches.iter().filter_map(|batch| {
                let DabBatchKind::RasterOperation(index) = batch.kind else { return None; };
                let operation = packet.scene.operations(batch.target)?.get(index as usize)?;
                match &operation.kind {
                    layer_core::RasterOperationKind::Bake { scene, scope:layer_core::SceneScope::Members(members), .. } => {
                        assert!(members.iter().all(|h| scene.view().occurrence(*h).is_some()));
                        Some(members.to_vec())
                    }
                    _ => None,
                }
            }).collect();
            self.operation_batches.extend(packet.dab_batches.iter().filter_map(|batch| {
                let DabBatchKind::RasterOperation(index) = batch.kind else { return None; };
                Some((batch.target, index, batch.damage, packet.commit_rasters))
            }));
            self.visibility = packet.scene.order().iter().map(|h| (*h, packet.scene.occurrence(*h).unwrap().visible)).collect();
            if self.size != [packet.view.width_px, packet.view.height_px] {
                return Err(BackendError("surface size mismatch"));
            }
            if packet.reset_layers
                && packet
                    .dab_batches
                    .iter()
                    .any(|b| b.kind == DabBatchKind::Persistent)
            {
                self.persistent.clear();
                self.material_batches.clear();
            }
            for (revision, plane) in packet.scene.artwork().paint.iter().map(|(_, _, source)| (&source.raster, layer_core::raster::RasterPlane::Color))
                .chain(packet.scene.artwork().coverage.iter().map(|(_, _, source)| (&source.raster, layer_core::raster::RasterPlane::Mask)))
                .filter(|_| packet.commit_rasters)
            {
                if revision.try_data().is_none() {
                    use layer_core::raster::*;
                    let descriptor = plane.descriptor(self.color);
                    let blob = TileBlob::encode(descriptor, &vec![1; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap();
                    let mut data = RasterData::default();
                    data.tiles.insert(TileKey { plane, coordinate: [0, 0] }, RasterTile::backed(blob));
                    revision.publish(Ok(data)).unwrap();
                }
            }
            self.preview.clear();
            self.styles.clear();
            self.material_page_jobs = frame_steps::material_page_jobs(packet.dab_batches);
            for batch in packet.dab_batches {
                self.styles.push(batch.style.clone());
                let start = batch.first_dab as usize;
                let end = start + batch.dab_count as usize;
                let dabs = &packet.dabs[start..end];
                match batch.kind {
                    DabBatchKind::RasterOperation(_) => {}
                    DabBatchKind::Persistent => {
                        if batch.dab_count > 0 {
                            self.material_batches
                                .push((batch.material_update, batch.dab_count));
                        }
                        self.persistent_dabs += dabs.len();
                        self.persistent.extend_from_slice(dabs);
                        self.persistent_batches.push((
                            batch.stroke_id,
                            batch.stroke_start,
                            batch.stroke_end,
                            batch.dab_count,
                        ));
                    }
                    DabBatchKind::Preview => self.preview.extend_from_slice(dabs),
                }
            }
            self.saw_reset |= packet.reset_layers;
            Ok(())
        }
    }

    fn retouch_document() -> Document {
        let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let (_, _, edits) = paint_insert(&document, empty_paint(&document), "Photo", document.scene().order().len());
        document.apply(Edit::Batch(edits)).unwrap();
        document
    }

    #[test]
    fn retouch_strokes_capture_their_source_and_replay_with_it() {
        let (mut input, mut engine) =
            engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
        let target = engine.document().working.target.unwrap();
        engine
            .apply_edit(references(engine.document(), [photo_handle()].into()))
            .unwrap();
        engine.set_retouch(Some(RetouchSource::References));
        engine.set_retouch_points(&[Point { x: 8., y: 8. }]);
        let prepared = engine.backend().retouch.last().cloned().flatten().unwrap();
        assert_eq!(prepared.target, target);
        assert_eq!(*prepared.retouch.references, [photo_handle()].into());
        assert_eq!(prepared.points, [Point { x: 128., y: 128. }], "a focus point is kept as its tile's center");
        let sent = engine.backend().retouch.len();
        engine.render_frame().unwrap();
        assert_eq!(engine.backend().retouch.len(), sent, "an unchanged tool is prepared once");
        for (sequence, phase, x) in [(1, PenPhase::Down, 8.), (2, PenPhase::Move, 20.), (3, PenPhase::Up, 30.)] {
            input.push(event(sequence, phase, x)).unwrap();
            engine.render_frame().unwrap();
            assert!(engine.backend().styles.iter().all(|style| style.retouch == Some(prepared.retouch.clone())));
        }
        let stroke = engine.completed_stroke.clone().unwrap();
        assert_eq!(stroke.retouch, Some(prepared.retouch.clone()));
        engine.completed_stroke.as_mut().unwrap().retouch = Some(Retouch::default());
        engine.replay_completed().unwrap();
        engine.render_frame().unwrap();
        assert!(!engine.backend().styles.is_empty());
        assert!(
            engine.backend().styles.iter().all(|style| style.retouch == Some(Retouch::default())),
            "a replay samples what its stroke captured"
        );
        let retired = engine.backend().retired_sources;
        engine.completed_at = Some(web_time::Instant::now() - CORRECTION_WINDOW);
        engine.render_frame().unwrap();
        assert_eq!(engine.backend().retired_sources, retired + 1);
        engine.apply_edit(references(engine.document(), Default::default())).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend().retouch.last().cloned().flatten().unwrap().retouch.references.is_empty());
        engine.set_retouch(None);
        assert_eq!(engine.backend().retouch.last(), Some(&None));
    }

    #[test]
    fn active_stroke_sources_survive_edits_and_retire_after_contact() {
        let (mut input, mut engine) = engine("Stroke source lifetime", 64, 64);
        engine.set_brush(default_brush(DefaultBrushPreset::DitherBlender)).unwrap();
        engine.render_frame().unwrap();
        input.push(event(1, PenPhase::Down, 16.)).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.has_active_stroke());
        let retired = engine.backend().retired_sources;
        assert!(engine.append_operations(Vec::new(), Vec::new(), None, false).is_err());
        assert_eq!(engine.backend().retired_sources, retired, "a rejected operation cannot retire active originals");
        let mut occurrence = active_occurrence(engine.document()).clone();
        occurrence.opacity = 0.8;
        engine.apply_edit(replace_occurrence(engine.document(), occurrence)).unwrap();
        assert_eq!(engine.backend().retired_sources, retired, "an edit cannot retire active originals");
        input.push(event(2, PenPhase::Up, 32.)).unwrap();
        engine.render_frame().unwrap();
        engine.flush_pending_edits().unwrap();
        assert!(!engine.has_active_stroke());
        engine.end_corrections();
        assert_eq!(engine.backend().retired_sources, retired + 1, "completed originals can be released");
    }

    #[test]
    fn a_retouch_miss_replays_the_stroke_once_contact_ends() {
        for late in [false, true] {
            let (mut input, mut engine) =
                engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
            engine.apply_edit(references(engine.document(), [photo_handle()].into())).unwrap();
            engine.set_retouch(Some(RetouchSource::References));
            engine.render_frame().unwrap();
            engine.backend_mut().saw_reset = false;
            let target = engine.document().working.target.unwrap();
            let empty = engine.document().target_raster(target).unwrap().identity();
            input.push(event(1, PenPhase::Down, 8.)).unwrap();
            engine.render_frame().unwrap();
            input.push(event(2, PenPhase::Move, 20.)).unwrap();
            engine.render_frame().unwrap();
            let stroke = engine.active_stroke.as_ref().unwrap().id;
            if !late {
                engine.backend_mut().retouch_misses.push(stroke);
            }
            engine.render_frame().unwrap();
            assert!(!engine.backend().saw_reset, "contact is never replayed while the pen is down");
            let live: Vec<_> = engine.backend().persistent.clone();
            input.push(event(3, PenPhase::Up, 30.)).unwrap();
            engine.render_frame().unwrap();
            if late {
                assert!(!engine.backend().saw_reset);
                engine.backend_mut().retouch_misses.push(stroke);
                engine.render_frame().unwrap();
            }
            assert!(engine.backend().saw_reset, "late={late}");
            let batches = &engine.backend().persistent_batches;
            let replay = &batches[batches.iter().rposition(|b| b.1).unwrap()..];
            assert!(replay.iter().all(|b| b.0 == stroke));
            assert!(replay.last().unwrap().2, "the replay paints the whole stroke to its end");
            assert!(engine.backend().persistent.starts_with(&live[..1]));
            let replays = engine.backend().persistent_batches.len();
            engine.render_frame().unwrap();
            assert_eq!(engine.backend().persistent_batches.len(), replays, "a stroke replays once");
            assert!(engine.completed_stroke.is_some());
            engine.undo().unwrap();
            assert_eq!(
                engine.document().target_raster(target).unwrap().identity(),
                empty,
                "the replay stays one undo step"
            );
        }
    }

    #[test]
    fn a_stroke_waiting_for_source_pixels_stays_correctable_until_it_replays() {
        let (mut input, mut engine) =
            engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
        engine.apply_edit(references(engine.document(), [photo_handle()].into())).unwrap();
        engine.set_retouch(Some(RetouchSource::References));
        engine.render_frame().unwrap();
        for (sequence, phase, x) in [(1, PenPhase::Down, 8.), (2, PenPhase::Move, 20.), (3, PenPhase::Up, 30.)] {
            input.push(event(sequence, phase, x)).unwrap();
            engine.render_frame().unwrap();
        }
        let stroke = engine.completed_stroke.as_ref().unwrap().id;
        engine.backend_mut().retouch_waiting = Some(stroke);
        engine.completed_at = Some(web_time::Instant::now() - CORRECTION_WINDOW * 2);
        engine.render_frame().unwrap();
        assert!(engine.completed_stroke.is_some(), "a stroke waiting for its source stays replayable");
        engine.backend_mut().saw_reset = false;
        engine.backend_mut().retouch_waiting = None;
        engine.backend_mut().retouch_misses.push(stroke);
        engine.render_frame().unwrap();
        assert!(engine.backend().saw_reset, "the reported stroke replays");
        engine.completed_at = Some(web_time::Instant::now() - CORRECTION_WINDOW * 2);
        engine.render_frame().unwrap();
        assert!(engine.completed_stroke.is_none(), "corrections end once nothing waits");
    }

    #[test]
    fn dabs_follow_the_documents_blending_and_masks_blend_linearly() {
        let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        composition_mut(&mut document).blend = layer_core::BlendSpace::Perceptual;
        let (mut input, mut engine) = engine_with(RecordingRenderer::default(), document, view(64, 64), TRANSFORM);
        let mut spaces = Vec::new();
        let mut paint = |engine: &mut CanvasEngine<RecordingRenderer>, input: &mut InputProducer<PenEvent>, sequence: u64| {
            spaces.clear();
            for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up].into_iter().enumerate() {
                input.push(event(sequence + i as u64, phase, 8. + 6. * i as f32)).unwrap();
                engine.render_frame().unwrap();
                spaces.extend(engine.backend().styles.iter().map(|s| s.blend_space));
            }
            assert!(!spaces.is_empty());
            spaces.clone()
        };
        let painted = paint(&mut engine, &mut input, 1);
        assert!(painted.iter().all(|s| *s == layer_core::BlendSpace::Perceptual));
        assert_eq!(engine.completed_stroke.as_ref().unwrap().blend_space, layer_core::BlendSpace::Perceptual);
        engine.replay_completed().unwrap();
        engine.render_frame().unwrap();
        let replayed = &engine.backend().styles;
        assert!(!replayed.is_empty() && replayed.iter().all(|s| s.blend_space == layer_core::BlendSpace::Perceptual), "a replay keeps the stroke's space");
        let edit = mask_edit(&mut engine, [0; 2]);
        engine.apply_edit(edit).unwrap();
        engine.apply_edit(mask_target_edit(engine.document(), true)).unwrap();
        let mask = paint(&mut engine, &mut input, 10);
        assert!(mask.iter().all(|s| *s == layer_core::BlendSpace::Linear), "mask coverage blends linearly");
    }

    #[test]
    fn retouch_strokes_refuse_masks_and_empty_sources() {
        let (_input, mut engine) =
            engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
        let down = event(1, PenPhase::Down, 8.);
        assert_eq!(engine.stroke_refusal(&down), None);
        engine.set_retouch(Some(RetouchSource::References));
        assert_eq!(engine.stroke_refusal(&down), Some(StrokeRefusal::EmptySource(RetouchSource::References)));
        engine.set_retouch(Some(RetouchSource::Editing));
        assert_eq!(engine.stroke_refusal(&down), Some(StrokeRefusal::EmptySource(RetouchSource::Editing)));
        engine.apply_edit(references(engine.document(), [photo_handle()].into())).unwrap();
        assert_eq!(engine.stroke_refusal(&down), Some(StrokeRefusal::EmptySource(RetouchSource::Editing)));
        engine.set_retouch(Some(RetouchSource::References));
        assert_eq!(engine.stroke_refusal(&down), None);
        let edit = mask_edit(&mut engine, [0; 2]);
        engine.apply_edit(edit).unwrap();
        engine.apply_edit(mask_target_edit(engine.document(), true)).unwrap();
        assert_eq!(engine.stroke_refusal(&down), Some(StrokeRefusal::Target(DrawingRefusal::Mask)));
        engine.set_retouch(None);
        assert_eq!(engine.stroke_refusal(&down), None, "ordinary brushes still paint masks");
    }

    fn clone_stroke(engine: &mut CanvasEngine<RecordingRenderer>, input: &mut InputProducer<PenEvent>, sequence: u64, x: f32) -> Stroke {
        for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up].into_iter().enumerate() {
            input.push(event(sequence + i as u64, phase, x + 6. * i as f32)).unwrap();
            engine.render_frame().unwrap();
        }
        let stroke = engine.completed_stroke.clone().unwrap();
        let mapping = stroke.retouch.clone().unwrap();
        assert!(engine.backend().styles.iter().rev().take(1).all(|s| s.retouch.as_ref() == Some(&mapping)));
        stroke
    }

    #[test]
    fn retouch_execution_changes_prepare_before_the_next_frame() {
        let (_,mut engine)=engine_with(RecordingRenderer::default(),retouch_document(),view(64,64),TRANSFORM);
        engine.set_brush(default_brush(DefaultBrushPreset::CloneStamp)).unwrap();
        engine.set_retouch(Some(RetouchSource::Editing));
        let original=engine.backend().retouch.last().unwrap().as_ref().unwrap().clone();
        for (preset,execution) in [(DefaultBrushPreset::HealingBrush,BrushExecution::Heal),(DefaultBrushPreset::SpotHealingBrush,BrushExecution::SpotHeal)] {
            engine.set_brush(default_brush(preset)).unwrap();
            let prepared=engine.backend().retouch.last().unwrap().as_ref().unwrap();
            assert_eq!(prepared.execution,execution);assert_eq!(prepared.target,original.target);
            assert_eq!(prepared.retouch,original.retouch);assert_eq!(prepared.points,original.points);
        }
        let preparations=engine.backend().retouch.len();
        engine.set_brush(default_brush(DefaultBrushPreset::SpotHealingBrush)).unwrap();
        assert_eq!(engine.backend().retouch.len(),preparations);
    }

    #[test]
    fn clone_strokes_copy_through_their_source_and_an_aligned_source_follows_them() {
        let (mut input, mut engine) =
            engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
        engine.apply_edit(references(engine.document(), [photo_handle()].into())).unwrap();
        engine.set_brush(default_brush(DefaultBrushPreset::CloneStamp)).unwrap();
        engine.set_retouch(Some(RetouchSource::References));
        let down = event(1, PenPhase::Down, 8.);
        assert_eq!(engine.stroke_refusal(&down), Some(StrokeRefusal::NoCloneSource));
        input.push(down).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.active_stroke.is_none(), "a clone without a source paints nothing");
        let at = |x, y| Point { x, y };
        engine.set_clone_source(CloneSource { point: Some(at(40., 30.)), ..CloneSource::default() });
        assert_eq!(engine.stroke_refusal(&down), None);

        let first = clone_stroke(&mut engine, &mut input, 10, 8.);
        let start = first.points[0].position;
        let end = first.points.last().unwrap().position;
        let offset = [40. - start.x, 30. - start.y];
        assert_eq!(first.retouch.as_ref().unwrap().offset, offset);
        assert_eq!(engine.clone_source().offset, Some(offset), "an aligned source keeps its first offset");
        assert_eq!(engine.clone_source().point, Some(at(end.x + offset[0], end.y + offset[1])), "and follows the stroke");
        assert_eq!(engine.clone_stroke_offset(), None);
        let second = clone_stroke(&mut engine, &mut input, 20, 30.);
        assert_eq!(second.retouch.as_ref().unwrap().offset, offset);

        engine.set_clone_source(CloneSource { point: Some(at(40., 30.)), aligned: false, ..CloneSource::default() });
        let loose = clone_stroke(&mut engine, &mut input, 30, 20.);
        let start = loose.points[0].position;
        assert_eq!(loose.retouch.as_ref().unwrap().offset, [40. - start.x, 30. - start.y], "each stroke starts at the source");
        assert_eq!(engine.clone_source().point, Some(at(40., 30.)));

        let mut moved = active_occurrence(engine.document()).clone();
        moved.offset = [4, 6];
        engine.apply_edit(replace_occurrence(engine.document(), moved.clone())).unwrap();
        engine.set_clone_source(CloneSource { point: Some(at(40., 30.)), flip: [true, false], ..CloneSource::default() });
        let flipped = clone_stroke(&mut engine, &mut input, 40, 20.);
        let first = flipped.points[0].position;
        let first = at(first.x + 4., first.y + 6.);
        let mapping = flipped.retouch.unwrap();
        assert_eq!(mapping.flip, [true, false]);
        assert_eq!(mapping.offset, [40. + first.x - 8., 30. - first.y], "layer pixels map through the layer's position");
    }

    #[test]
    fn queued_clone_contacts_share_alignment_without_overwriting_a_later_source() {
        let (mut input, mut engine) = engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
        engine.apply_edit(references(engine.document(), [photo_handle()].into())).unwrap();
        engine.set_brush(default_brush(DefaultBrushPreset::CloneStamp)).unwrap();
        engine.set_retouch(Some(RetouchSource::References));
        let source = |x| CloneSource { point: Some(Point { x, y: 30. }), ..CloneSource::default() };
        engine.set_clone_source(source(40.));
        for (sequence, x) in [(10, 8.), (20, 20.), (30, 30.)] {
            if sequence == 30 { engine.set_clone_source(source(20.)); }
            for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up].into_iter().enumerate() {
                let event = event(sequence + i as u64, phase, x + 6. * i as f32);
                input.push(event).unwrap();
                engine.capture_queued_contact(event);
            }
        }
        engine.set_clone_source(source(7.));
        engine.render_frame().unwrap();
        let offset = engine.completed_stroke.as_ref().unwrap().retouch.as_ref().unwrap().offset;
        engine.render_frame().unwrap();
        assert_eq!(engine.completed_stroke.as_ref().unwrap().retouch.as_ref().unwrap().offset, offset);
        engine.render_frame().unwrap();
        let third = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(third.retouch.as_ref().unwrap().offset, [20. - third.points[0].position.x, 30. - third.points[0].position.y]);
        assert_eq!(engine.clone_source(), source(7.));
    }

    #[test]
    fn healing_maps_through_the_source_and_spot_healing_needs_none() {
        let (mut input, mut engine) =
            engine_with(RecordingRenderer::default(), retouch_document(), view(64, 64), TRANSFORM);
        engine.apply_edit(references(engine.document(), [photo_handle()].into())).unwrap();
        engine.set_retouch(Some(RetouchSource::References));
        let down = event(1, PenPhase::Down, 8.);
        engine.set_brush(default_brush(DefaultBrushPreset::HealingBrush)).unwrap();
        assert_eq!(engine.stroke_refusal(&down), Some(StrokeRefusal::NoCloneSource));
        engine.set_clone_source(CloneSource { point: Some(Point { x: 40., y: 30. }), ..CloneSource::default() });
        let healed = clone_stroke(&mut engine, &mut input, 10, 8.);
        let start = healed.points[0].position;
        assert_eq!(healed.retouch.unwrap().offset, [40. - start.x, 30. - start.y]);
        assert_eq!(engine.backend().styles.last().unwrap().execution, BrushExecution::Heal);

        engine.set_brush(default_brush(DefaultBrushPreset::SpotHealingBrush)).unwrap();
        engine.set_clone_source(CloneSource::default());
        assert_eq!(engine.stroke_refusal(&down), None, "spot healing finds its own source");
        let spot = clone_stroke(&mut engine, &mut input, 20, 30.);
        let retouch = spot.retouch.unwrap();
        assert_eq!(retouch.offset, [0.; 2]);
        assert_eq!(*retouch.references, [photo_handle()].into());
        assert_eq!(engine.clone_source(), CloneSource::default(), "spot healing leaves the clone source alone");
    }

    #[test]
    fn backend_replacement_restores_committed_roots_and_rebuilds_only_the_active_contact() {
        for preset in [
            DefaultBrushPreset::GPen,
            DefaultBrushPreset::NaturalBlender,
            DefaultBrushPreset::WatercolorWash,
        ] {
            let (mut input, mut canvas) = engine("device recovery", 128, 128);
            canvas.set_brush(default_brush(preset)).unwrap();
            for (sequence, phase, x) in [
                (1, PenPhase::Down, 10.),
                (2, PenPhase::Move, 40.),
                (3, PenPhase::Up, 70.),
                (4, PenPhase::Down, 20.),
                (5, PenPhase::Move, 60.),
            ] {
                input.push(event(sequence, phase, x)).unwrap();
                canvas.render_frame_at(sequence * 16_000_000).unwrap();
            }
            let checkpoint = canvas.checkpoint();
            let document = canvas.document().clone();
            input.push(event(6, PenPhase::Up, 90.)).unwrap();
            canvas
                .replace_backend(RecordingRenderer::default())
                .unwrap();
            assert!(canvas.has_active_stroke() && canvas.has_pending_input());
            assert_eq!(canvas.document(), &document);
            assert_eq!(canvas.checkpoint(), checkpoint);
            canvas.render_frame_at(96_000_000).unwrap();
            canvas.render_frame_at(128_000_000).unwrap();
            assert!(canvas.backend().saw_reset);
            assert!(!canvas.has_active_stroke() && !canvas.has_pending_input());
            assert_eq!(canvas.metrics().committed_strokes, 2);
            assert!(canvas.backend().persistent_dabs > 0);
            assert!(
                canvas
                    .backend()
                    .persistent_batches
                    .iter()
                    .all(|(id, _, _, _)| id.0 == 2),
                "only the live contact is rebuilt; completed ink comes from raster roots"
            );
            assert!(canvas.undo().unwrap());
            canvas.render_frame().unwrap();
            assert_eq!(
                active_paint(canvas.document()).raster,
                active_paint(&document).raster
            );
            assert_eq!(canvas.checkpoint(), checkpoint);
            assert!(canvas.undo().unwrap());
            canvas.render_frame().unwrap();
            assert!(active_paint(canvas.document()).raster.is_empty());
            canvas
                .replace_backend(RecordingRenderer::default())
                .unwrap();
            canvas.render_frame().unwrap();
            assert!(canvas.can_redo());
            assert!(canvas.redo().unwrap());
            canvas.render_frame().unwrap();
            assert_eq!(
                canvas.backend().persistent_dabs,
                0,
                "history restoration uses raster roots"
            );
            assert_eq!(
                active_paint(canvas.document()).raster,
                active_paint(&document).raster
            );
            assert_eq!(canvas.checkpoint(), checkpoint);
        }
    }

    #[test]
    fn input_retirement_preserves_only_submitted_raster_boundaries() {
        let (mut input, mut canvas) = engine("retirement", 128, 128);
        input.push(event(1, PenPhase::Down, 20.)).unwrap();
        input.push(event(2, PenPhase::Up, 80.)).unwrap();
        canvas.render_frame().unwrap();
        let root = active_paint(canvas.document()).raster.clone();
        let checkpoint = canvas.checkpoint();
        for index in 0..=INPUT_BATCH + 1 {
            let phase = if index == 0 {
                PenPhase::Down
            } else if index == INPUT_BATCH + 1 {
                PenPhase::Up
            } else {
                PenPhase::Move
            };
            input.push(event(index as u64 + 3, phase, 30.)).unwrap();
        }
        canvas.discard_unsubmitted_input();
        assert!(!canvas.has_active_stroke() && !canvas.has_pending_input());
        assert_eq!(active_paint(canvas.document()).raster, root);
        assert_eq!(canvas.checkpoint(), checkpoint);
        assert_eq!(canvas.metrics().committed_strokes, 1);
        assert!(canvas.undo().unwrap());
        assert!(active_paint(canvas.document()).raster.is_empty());
        assert!(canvas.redo().unwrap());
        assert_eq!(active_paint(canvas.document()).raster, root);
        canvas.discard_unsubmitted_input();
        assert_eq!(canvas.checkpoint(), checkpoint, "retirement is idempotent");
    }

    #[test]
    fn failed_backend_replacement_retains_backend_history_and_pending_input() {
        let (mut input, mut canvas) = engine("failed recovery", 128, 128);
        canvas.render_frame().unwrap();
        input.push(event(1, PenPhase::Down, 20.)).unwrap();
        input.push(event(2, PenPhase::Up, 80.)).unwrap();
        assert!(
            canvas
                .replace_backend(RecordingRenderer {
                    fail_resize: true,
                    ..Default::default()
                })
                .is_err()
        );
        assert!(!canvas.backend().fail_resize);
        canvas.render_frame().unwrap();
        assert_eq!(canvas.metrics().committed_strokes, 1);
        assert!(canvas.undo().unwrap());
        assert!(canvas.redo().unwrap());
    }

    #[test]
    fn material_update_history_preserves_live_and_active_replay() {
        for feedback in [false, true] {
            for coalesced in [1, 2, 4] {
                let (mut input, mut canvas) = engine("material replay", 128, 128);
                canvas
                    .set_brush(default_brush(DefaultBrushPreset::WatercolorWash))
                    .unwrap();
                canvas
                    .set_instant_feedback(InstantFeedbackConfig {
                        enabled: feedback,
                        ..Default::default()
                    })
                    .unwrap();
                for i in 0..9 {
                    let mut p = event(
                        i + 1,
                        if i == 0 {
                            PenPhase::Down
                        } else {
                            PenPhase::Move
                        },
                        8. + i as f32 * 9.,
                    );
                    p.timestamp_ns = (i + 1) * 16_000_000;
                    input.push(p).unwrap();
                    if i % coalesced == 0 {
                        canvas.render_frame().unwrap();
                    }
                }
                canvas.render_frame().unwrap();
                let before = canvas.backend().material_batches.clone();
                let updates = canvas
                    .active_stroke
                    .as_ref()
                    .unwrap()
                    .material_updates
                    .clone();
                canvas.render_frame().unwrap();
                assert_eq!(
                    canvas.active_stroke.as_ref().unwrap().material_updates,
                    updates,
                    "idle is not a material update"
                );
                canvas.rebuild_all = true;
                canvas.render_frame().unwrap();
                assert_eq!(
                    canvas.backend().material_batches,
                    before,
                    "active replay: feedback={feedback}, coalesced={coalesced}"
                );
                let mut up = event(10, PenPhase::Up, 89.);
                up.timestamp_ns = 160_000_000;
                input.push(up).unwrap();
                canvas.render_frame().unwrap();
                let before = canvas.backend().material_batches.clone();
                let stroke = canvas.completed_stroke.as_ref().unwrap();
                assert_eq!(
                    stroke.material_updates.last().copied(),
                    Some(stroke.points.len() as u32)
                );
                assert!(stroke.material_updates.windows(2).all(|w| w[0] < w[1]));
                canvas.rebuild_all = true;
                canvas.render_frame().unwrap();
                assert_eq!(
                    canvas.backend().material_batches,
                    before,
                    "committed replay: feedback={feedback}, coalesced={coalesced}"
                );
            }
        }
    }

    #[test]
    fn strokes_capture_layer_local_selection_for_preview_commit_and_replay() {
        use layer_core::Selection;
        use std::sync::Arc;
        for mask_target in [false, true] {
            let selection = Selection::polygon(vec![
                Point { x: 4., y: 4. },
                Point { x: 50., y: 4. },
                Point { x: 50., y: 40. },
                Point { x: 4., y: 40. },
            ])
            .unwrap();
            let mut doc = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            doc.working.selection = Some(selection.clone());
            active_occurrence_mut(&mut doc).offset = [3, 7];
            let offset = if mask_target {
                add_mask(&mut doc, [11, 2]);
                active_occurrence_mut(&mut doc).mask.as_mut().unwrap().linked = false;
                Point { x: 11., y: 2. }
            } else { layer_core::offsets::point(active_occurrence(&doc).offset) };
            doc.apply(mask_target_edit(&doc, mask_target)).unwrap();
            let expected = Arc::new(selection.translated(Point {
                x: -offset.x,
                y: -offset.y,
            }));
            let (mut producer, mut engine) =
                engine_with(RecordingRenderer::default(), doc, view(128, 128), TRANSFORM);
            producer.push(event(1, PenPhase::Down, 8.)).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(
                engine
                    .active_stroke
                    .as_ref()
                    .unwrap()
                    .style
                    .selection
                    .as_ref(),
                Some(&expected)
            );
            assert!(!engine.backend.styles.is_empty());
            assert!(
                engine
                    .backend
                    .styles
                    .iter()
                    .all(|s| s.selection.as_ref() == Some(&expected))
            );
            // Even a programmatic selection edit during contact must not change
            // its preview, final dabs, or recorded geometry.
            let mut inverse = selection;
            inverse.inverted = true;
            engine
                .apply_edit(selection_edit(engine.document(), Some(inverse)))
                .unwrap();
            producer.push(event(2, PenPhase::Up, 48.)).unwrap();
            engine.render_frame().unwrap();
            let stroke = engine.completed_stroke.as_ref().unwrap();
            assert_eq!(stroke.selection.as_ref(), Some(&expected));
            assert_eq!(Some(stroke.target), engine.document().active_target());
            let committed = engine
                .document()
                .target_raster(stroke.target)
                .unwrap()
                .identity();
            engine.apply_edit(selection_edit(engine.document(), None)).unwrap();
            engine.rebuild_all = true;
            engine.render_frame().unwrap();
            assert!(engine.backend.styles.is_empty());
            engine.undo().unwrap(); // Deselect.
            engine.undo().unwrap(); // Stroke.
            engine.render_frame().unwrap();
            assert!(
                engine
                    .document()
                    .target_raster(engine.document().active_target().unwrap())
                    .unwrap()
                    .is_empty()
            );
            engine.redo().unwrap();
            engine.redo().unwrap();
            engine.render_frame().unwrap();
            assert_eq!(
                engine
                    .document()
                    .target_raster(engine.document().active_target().unwrap())
                    .unwrap()
                    .identity(),
                committed
            );
            producer.push(event(3, PenPhase::Down, 60.)).unwrap();
            engine.render_frame().unwrap();
            assert!(
                engine
                    .active_stroke
                    .as_ref()
                    .unwrap()
                    .style
                    .selection
                    .is_none()
            );
            producer.push(event(4, PenPhase::Cancel, 64.)).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.metrics().committed_strokes, 1);
        }
    }

    #[test]
    fn transform_preview_is_not_history_and_edits_or_new_strokes_cancel_it() {
        let (mut producer, mut engine) = engine("preview", 128, 128);
        engine.render_frame().unwrap();
        let initial = engine.document().clone();
        let mut preview = layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            target: initial.working.target.unwrap(),
            selection: None,
            transform: layer_core::ImageTransform::default(),
        };
        for x in [12., 100., -23.] {
            preview.transform.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(Point { x, y: 4. }));
            engine.set_transform_preview(Some(preview.clone())).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.backend.transform.as_ref(), Some(&preview));
            assert_eq!(engine.document().revision, initial.revision);
            assert_authored_eq(&engine.document().artwork, &initial.artwork);
            assert!(!engine.can_undo());
        }
        engine.set_transform_preview(None).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend.transform.is_none());
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        producer.push(event(1, PenPhase::Down, 20.)).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend.transform.is_none());
        assert!(engine.set_transform_preview(Some(preview.clone())).is_err());
        producer.push(event(2, PenPhase::Up, 80.)).unwrap();
        engine.render_frame().unwrap();
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        engine.undo().unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend.transform.is_none());
        engine.set_transform_preview(Some(preview)).unwrap();
        engine.set_layer_opacity(initial.working.occurrence.unwrap(), 0.5).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend.transform.is_none());
    }

    #[test]
    fn linked_mask_transform_commits_both_histories_and_selection_as_one_edit() {
        use layer_core::{Affine, ImageTransform, RasterOperationKind, Selection};
        for primary_mask in [false, true] {
            let mut doc = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            active_occurrence_mut(&mut doc).offset = [7, 3];
            let mask_handle = add_mask(&mut doc, [15, 11]);
            doc.working.selection = Some(
                Selection::polygon(vec![
                    Point { x: 20., y: 20. },
                    Point { x: 60., y: 20. },
                    Point { x: 60., y: 60. },
                ])
                .unwrap(),
            );
            let before = doc.clone();
            let target = if primary_mask {
                SourceTarget::Coverage(mask_handle)
            } else {
                doc.working.target.unwrap()
            };
            let origin = layer_core::offsets::point(doc.target_offset(target));
            let preview = layer_render::TransformPreview {
                transaction: 1,
                moving: false,
                target,
                selection: doc.working.selection.as_ref().map(|s| {
                    s.translated(Point {
                        x: -origin.x,
                        y: -origin.y,
                    })
                }),
                transform: ImageTransform::affine(Affine::around(
                    Point { x: 30., y: 30. },
                    [1.2, 0.7],
                    0.2,
                    Point { x: 5., y: 8. },
                )),
            };
            let companion = preview.companion(doc.scene()).unwrap();
            let (_, mut engine) =
                engine_with(RecordingRenderer::default(), doc, view(128, 128), TRANSFORM);
            engine.render_frame().unwrap();
            engine.set_transform_preview(Some(preview.clone())).unwrap();
            let moved = engine.display_selection().unwrap().into_owned();
            assert!(engine.commit_transform(None).unwrap());
            let document = engine.document();
            for p in [&preview, &companion] {
                let ops = document.target_operations(p.target).unwrap();
                assert_eq!(ops.len(), 1);
                assert_eq!(ops[0].kind, RasterOperationKind::Transform(p.transform.clone()));
                assert_eq!(ops[0].coverage.selection, p.selection);
            }
            assert_eq!(engine.batches.len(), 2);
            assert!(engine.retained_tiles().resident_bytes() < layer_core::raster::MAX_CAPTURE_BYTES as usize,
                "linked small-source producers reserve their source domains");
            assert_eq!(engine.document().working.selection.as_ref(), Some(&moved));
            assert!(engine.undo().unwrap());
            assert_authored_eq(&engine.document().artwork, &before.artwork);
            assert_eq!(engine.document().working.selection, before.working.selection);
            assert!(!engine.can_undo());
            assert!(engine.redo().unwrap());
            engine.render_frame().unwrap();
            assert!(
                engine.batches.is_empty(),
                "redo must restore pixels without replaying operations"
            );
            assert_eq!(engine.document().working.selection.as_ref(), Some(&moved));
            assert!(!active_paint(engine.document()).raster.is_empty());
            assert!(!engine.document().target_raster(SourceTarget::Coverage(mask_handle)).unwrap().is_empty());
        }
    }

    #[test]
    fn applying_transform_moves_selection_atomically_and_cancel_keeps_original() {
        use layer_core::{Affine, ImageTransform, Selection};
        let mut doc = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let layer = doc.working.target.unwrap();
        active_occurrence_mut(&mut doc).offset = [12, 7];
        let selection = Selection::polygon(vec![
            Point { x: 20., y: 20. },
            Point { x: 60., y: 20. },
            Point { x: 20., y: 60. },
        ])
        .unwrap();
        doc.working.selection = Some(selection.clone());
        let (_, mut engine) =
            engine_with(RecordingRenderer::default(), doc, view(128, 128), TRANSFORM);
        let preview = layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            target: layer,
            selection: Some(selection.translated(Point { x: -12., y: -7. })),
            transform: ImageTransform::affine(Affine::around(
                Point { x: 28., y: 33. },
                [1.5, 0.7],
                0.5,
                Point { x: 3., y: -2. },
            )),
        };
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        let placed = engine.display_selection().unwrap().into_owned();
        assert_ne!(placed, selection);
        assert_eq!(engine.document().working.selection.as_ref(), Some(&selection));
        engine.set_transform_preview(None).unwrap();
        assert_eq!(engine.display_selection().as_deref(), Some(&selection));
        assert!(!engine.can_undo());
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        assert!(engine.commit_transform(None).unwrap());
        assert!(engine.transform_preview.is_none());
        assert_eq!(engine.document().working.selection.as_ref(), Some(&placed));
        let operation = &engine.document().target_operations(layer).unwrap()[0];
        assert_eq!(operation.coverage.selection, preview.selection);
        assert_eq!(
            operation.kind,
            layer_core::RasterOperationKind::Transform(preview.transform.clone())
        );
        assert!(engine.undo().unwrap());
        assert_eq!(engine.document().working.selection.as_ref(), Some(&selection));
        assert!(
            engine.document().target_operations(layer).unwrap()
                .is_empty()
        );
        assert!(
            !engine.can_undo(),
            "one undo restores both pixels and selection"
        );
        assert!(engine.redo().unwrap());
        assert_eq!(engine.document().working.selection.as_ref(), Some(&placed));
        let mut inverted = preview.clone();
        inverted.selection.as_mut().unwrap().inverted = true;
        engine.set_transform_preview(Some(inverted)).unwrap();
        assert!(engine.commit_transform(None).unwrap());
        assert!(engine.document().working.selection.as_ref().unwrap().inverted);
        assert!(engine.undo().unwrap());
        assert_eq!(engine.document().working.selection.as_ref(), Some(&placed));
        engine
            .set_transform_preview(Some(layer_render::TransformPreview {
                transform: Default::default(),
                ..preview
            }))
            .unwrap();
        assert!(
            !engine.commit_transform(None).unwrap(),
            "identity must not add history"
        );
    }

    #[test]
    fn perspective_transform_carries_contours_and_waits_for_pixel_coverage() {
        use layer_core::{ImageTransform, Projective, Selection, SelectionPixels, LayerPlacement};
        let mut doc = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let layer = doc.working.target.unwrap();
        let selection = Selection::polygon(vec![
            Point { x: 20., y: 20. },
            Point { x: 60., y: 20. },
            Point { x: 20., y: 60. },
        ])
        .unwrap();
        doc.working.selection = Some(selection.clone());
        let (_, mut engine) =
            engine_with(RecordingRenderer::default(), doc, view(128, 128), TRANSFORM);
        let source = layer_core::Rect {
            min: Point { x: 20., y: 20. },
            max: Point { x: 60., y: 60. },
        };
        let quad = [
            Point { x: 30., y: 10. },
            Point { x: 50., y: 10. },
            Point { x: 80., y: 70. },
            Point { x: 0., y: 70. },
        ];
        let map = LayerPlacement::from_projective(Projective::rect_to_quad(source, quad).unwrap());
        let preview = layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            target: layer,
            selection: Some(selection.clone()),
            transform: ImageTransform { placement: map.clone(), ..Default::default() },
        };
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        let expected = selection.mapped(&map).unwrap();
        assert_eq!(engine.display_selection().as_deref(), Some(&expected));
        assert!(engine.commit_transform(None).unwrap());
        assert_eq!(engine.document().working.selection.as_ref(), Some(&expected));
        assert!(engine.undo().unwrap());
        assert_eq!(engine.document().working.selection.as_ref(), Some(&selection));
        let pixels = Selection::pixels(std::sync::Arc::new(
            SelectionPixels::new([128, 128], [0, 0, 8, 8], vec![0; 16 * 128]).unwrap(),
        ));
        engine
            .set_transform_preview(Some(layer_render::TransformPreview {
                selection: Some(pixels),
                ..preview
            }))
            .unwrap();
        assert!(engine.display_selection().is_none(), "no outline until coverage is resampled");
        assert_eq!(
            engine.commit_transform(None),
            Err(DocumentError::InvalidLayerOperation(Selection::RESAMPLE_PIXELS))
        );
        assert!(engine.transform_preview().is_some());
        assert!(engine.can_redo(), "the refused commit leaves history untouched");
        let request = engine.transform_selection_request(41).unwrap();
        assert_eq!(request.request_id, 41);
        let layer_render::RegionSource::TransformedSelection {
            target,
            selection: coverage,
            map: requested,
        } = &request.source
        else {
            panic!("a mapped selection request")
        };
        assert_eq!((*target, requested), (layer, &map));
        assert!(matches!(coverage.shape, layer_core::SelectionShape::Pixels(_)));
        let pending = engine.transform_preview().unwrap().clone();
        let before = engine.document().working.selection.clone();
        let moved = std::sync::Arc::new(
            SelectionPixels::bytes([128, 128], [4, 4, 12, 12], vec![0x80ff_ff80; 32 * 128])
                .unwrap(),
        );
        let elsewhere = std::sync::Arc::new(SelectionPixels::bytes([64, 64], [0; 4], vec![0; 16 * 64]).unwrap());
        assert!(engine.commit_transform(Some(elsewhere)).is_err());
        assert!(engine.can_redo(), "a mismatched result leaves history untouched");
        for inverted in [false, true] {
            let mut preview = pending.clone();
            preview.selection.as_mut().unwrap().inverted = inverted;
            engine.set_transform_preview(Some(preview)).unwrap();
            assert!(engine.commit_transform(Some(moved.clone())).unwrap());
            let mut expected = Selection::pixels(moved.clone());
            expected.inverted = inverted;
            assert_eq!(engine.document().working.selection.as_ref(), Some(&expected));
            assert!(engine.transform_preview().is_none());
            assert!(engine.undo().unwrap());
            assert_eq!(engine.document().working.selection, before, "pixels and selection are one step");
        }
        let contours = layer_render::TransformPreview {
            selection: Some(selection.clone()),
            ..pending
        };
        engine.set_transform_preview(Some(contours)).unwrap();
        assert!(engine.transform_selection_request(42).is_none(), "contours map on the CPU");
    }

    fn erase(
        engine: &mut CanvasEngine<RecordingRenderer>,
        selection: &layer_core::Selection,
    ) -> layer_core::RasterOperation {
        let mut coverage =
            layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(), engine.document().composition().size, [0; 2]);
        coverage.source.default_coverage = f32::from(selection.inverted);
        coverage.selection = Some(selection.clone());
        layer_core::RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage,
            kind: layer_core::RasterOperationKind::Erase { alpha_locked: false },
        }
    }

    #[test]
    fn inserted_layers_take_operations_from_their_source_pixels_in_one_step() {
        let (_, mut engine) = engine("insert with operations", 1024, 768);
        engine.render_frame().unwrap();
        let source = engine.document().working.target.unwrap();
        let pixels = engine.document().target_raster(source).unwrap().clone();
        let selection = layer_core::Selection::polygon(vec![
            Point { x: 300., y: 280. },
            Point { x: 420., y: 280. },
            Point { x: 420., y: 400. },
            Point { x: 300., y: 400. },
        ])
        .unwrap();
        engine
            .apply_edit(selection_edit(engine.document(), Some(selection.clone())))
            .unwrap();
        let before = engine.document().clone();
        let (_, id, insertion) = paint_insert(&before, active_paint(&before).clone(), "Copy", 0);
        let mut outside = selection.clone();
        outside.inverted = true;
        let operations = vec![
            (id, erase(&mut engine, &outside)),
            (source, erase(&mut engine, &selection)),
        ];
        engine
            .insert_with_operations(
                insertion,
                operations,
                Some(None),
            )
            .unwrap();
        assert_eq!(engine.restore_rasters, [(id, pixels)], "the copy starts from its source pixels");
        assert!(engine.document().working.selection.is_none());
        let damage: Vec<_> = engine.batches.iter().map(|b| (b.target, b.damage)).collect();
        assert_eq!(damage[0], (id, Rect::from_extent([1024, 768])), "erasing outside touches every page");
        assert_eq!(damage[1], (source, selection.bounds()), "erasing inside stays within the selection");
        for layer in [id, source] {
            assert!(engine.document().target_raster(layer).unwrap().try_data().is_none());
        }
        engine.render_frame().unwrap();
        assert!(engine.restore_rasters.is_empty());
        assert!(engine.undo().unwrap());
        assert_authored_eq(&engine.document().artwork, &before.artwork);
        assert_eq!(engine.document().working.selection, before.working.selection);
        assert!(engine.redo().unwrap());
        assert_eq!(engine.document().artwork.occurrences.len(), before.artwork.occurrences.len() + 1);
    }

    #[test]
    fn undo_then_operation_before_a_frame_restores_the_operation_input() {
        for mask in [false, true] {
            let (_, mut engine) = engine("undo then operation", 64, 64);
            let target = if mask {
                let edit = mask_edit(&mut engine, [0; 2]);
                engine.apply_edit(edit).unwrap();
                SourceTarget::Coverage(active_occurrence(engine.document()).mask.as_ref().unwrap().source)
            } else { engine.document().working.target.unwrap() };
            engine.render_frame().unwrap();
            let original = engine.document().target_raster(target).unwrap().identity();
            let selection = layer_core::Selection::polygon(Rect::from_extent([32, 32]).corners().to_vec()).unwrap();
            let operation = erase(&mut engine, &selection);
            engine.append_raster_operation(target, operation).unwrap();
            engine.render_frame().unwrap();
            assert_ne!(engine.document().target_raster(target).unwrap().identity(), original);
            assert!(engine.undo().unwrap());
            let operation = erase(&mut engine, &selection);
            engine.append_raster_operation(target, operation).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.backend().restored_rasters, [(target, original)]);
            assert!(engine.restore_rasters.is_empty());
            engine.render_frame().unwrap();
            assert!(engine.backend().restored_rasters.is_empty(), "the restored input is submitted once");
        }
    }

    #[test]
    fn canvas_remap_replaces_a_queued_history_restore_before_rendering() {
        use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
        use layer_core::{CanvasGeometry, ImageOrientation, RecordChange};
        use layer_core::color::{SampleDepth, source::{SourceBuilder, SourceChannels, SourceInterpretation}};
        let (_, mut engine) = engine("queued canvas restore", 512, 256);
        let target = engine.document().working.target.unwrap();
        let SourceTarget::Paint(handle) = target else { unreachable!() };
        let descriptor = engine.document().composition().color.paint_descriptor();
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![255; descriptor.byte_len([256; 2]).unwrap()]).unwrap());
        let mut source = active_paint(engine.document()).clone();
        let mut photo = SourceBuilder::new([512, 256], SourceInterpretation {
            channels: SourceChannels::Rgba, depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false,
        }, 1024 * 1024).unwrap();
        for _ in 0..256 { photo.push_row(&[255; 512 * 4]).unwrap(); }
        source.base = Some(layer_core::authored::PaintBase::new(std::sync::Arc::new(photo.finish().unwrap()).into()));
        source.raster = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [1, 0] }, tile)].into(),
            ..Default::default()
        });
        engine.apply_edit(Edit::Paint(RecordChange::replace(&engine.document().artwork.paint, handle, Some(source)).unwrap())).unwrap();
        engine.render_frame().unwrap();
        let original = engine.document().target_raster(target).unwrap().identity();
        for (index, orientation) in [ImageOrientation::FlipHorizontal, ImageOrientation::RotateRight].into_iter().enumerate() {
            let geometry = CanvasGeometry::orient(engine.document().composition().size, orientation);
            let plan = engine.canvas_geometry_plan(&geometry).unwrap();
            let remap = plan.remap_plan(engine.document()).unwrap();
            let results = remap.run(&engine.document().artwork, &Default::default(), &std::sync::atomic::AtomicBool::new(false)).unwrap();
            engine.apply_canvas_plan(plan.with_remapped(results).unwrap(), Vec::new()).unwrap();
            if index == 0 {
                engine.render_frame().unwrap();
                assert!(engine.undo().unwrap());
                assert_eq!(engine.restore_rasters.iter().map(|(id, raster)| (*id, raster.identity())).collect::<Vec<_>>(), [(target, original)]);
            }
        }
        assert_eq!(active_paint(engine.document()).domain, [256, 512]);
        let raster = engine.document().target_raster(target).unwrap();
        let current = raster.identity();
        assert_ne!(current, original);
        let data = raster.try_data().unwrap().unwrap();
        assert!(data.tiles.contains_key(&TileKey { plane: RasterPlane::Color, coordinate: [0, 1] }));
        assert!(!data.tiles.contains_key(&TileKey { plane: RasterPlane::Color, coordinate: [1, 0] }));
        engine.render_frame().unwrap();
        assert_eq!(engine.backend().restored_rasters, [(target, current)]);
    }

    #[test]
    fn consecutive_history_steps_keep_each_targets_latest_restore_until_submission() {
        for redo in [false, true] {
            let (_, mut engine) = engine("multiple history restores", 64, 64);
            let paint = engine.document().working.target.unwrap();
            let edit = mask_edit(&mut engine, [0; 2]);
            engine.apply_edit(edit).unwrap();
            let mask = SourceTarget::Coverage(active_occurrence(engine.document()).mask.as_ref().unwrap().source);
            engine.render_frame().unwrap();
            let original: std::collections::BTreeMap<_, _> = [paint, mask].map(|target|
                (target, engine.document().target_raster(target).unwrap().identity())).into();
            let selection = layer_core::Selection::polygon(Rect::from_extent([32, 32]).corners().to_vec()).unwrap();
            for target in [paint, mask] {
                let operation = erase(&mut engine, &selection);
                engine.append_raster_operation(target, operation).unwrap();
                engine.render_frame().unwrap();
            }
            let changed_paint = engine.document().target_raster(paint).unwrap().identity();
            assert!(engine.undo().unwrap());
            assert!(engine.undo().unwrap());
            let mut expected = original;
            if redo {
                assert!(engine.redo().unwrap());
                expected.insert(paint, changed_paint);
            }
            let operation = erase(&mut engine, &selection);
            engine.append_raster_operation(mask, operation).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.backend().restored_rasters.iter().copied().collect::<std::collections::BTreeMap<_, _>>(), expected);
            assert_eq!(engine.backend().restored_rasters.len(), 2, "only the latest restored version of each target is submitted");
        }
    }

    #[test]
    fn undoing_a_layer_insertion_drops_its_queued_raster_restore() {
        let (_, mut engine) = engine("removed restore target", 64, 64);
        engine.render_frame().unwrap();
        let (_, target, insertion) = paint_insert(engine.document(), empty_paint(engine.document()), "Pasted", 0);
        engine.apply_edit(Edit::Batch(insertion)).unwrap();
        engine.render_frame().unwrap();
        let selection = layer_core::Selection::polygon(Rect::from_extent([32, 32]).corners().to_vec()).unwrap();
        let operation = erase(&mut engine, &selection);
        engine.append_raster_operation(target, operation).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.undo().unwrap());
        assert_eq!(engine.restore_rasters.len(), 1);
        assert!(engine.undo().unwrap());
        assert!(engine.document().target_raster(target).is_none());
        engine.render_frame().unwrap();
        assert!(engine.backend().restored_rasters.is_empty());
    }

    #[test]
    fn inserting_paint_and_effect_masks_preserves_existing_raster_pages() {
        use layer_core::authored::{EffectApplication, Occurrence, OccurrenceContent};
        use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
        use layer_core::{CoverageSnapshot, RecordChange};
        let (_, mut engine) = engine("source insertion", 64, 64);
        let target = engine.document().working.target.unwrap();
        let descriptor = engine.document().composition().color.paint_descriptor();
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![47; descriptor.byte_len([256; 2]).unwrap()]).unwrap());
        let raster = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile)].into(),
            ..Default::default()
        });
        engine.apply_edit(Edit::SetRaster { target, revision: raster.clone() }).unwrap();
        engine.render_frame().unwrap();
        engine.backend_mut().saw_reset = false;
        for copied in [false, true] {
            let source = if copied { active_paint(engine.document()).clone() } else { empty_paint(engine.document()) };
            let (_, _, edits) = paint_insert(engine.document(), source, "Inserted paint", 0);
            engine.apply_edit(Edit::Batch(edits)).unwrap();
            engine.render_frame().unwrap();
            assert!(!engine.backend().saw_reset, "new paint initializes its own pages");
            assert_eq!(engine.document().target_raster(target), Some(&raster));
        }
        for kind in 0..2 {
            let document = engine.document();
            let mut mask = CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(), [64; 2], [0; 2]);
            mask.source.default_coverage = 0.25;
            if kind == 1 {
                let descriptor = document.composition().color.coverage_descriptor();
                let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![97; descriptor.byte_len([256; 2]).unwrap()]).unwrap());
                mask.source.raster = RasterRevision::backed(RasterData {
                    tiles: [(TileKey { plane: RasterPlane::Mask, coordinate: [0, 0] }, tile)].into(),
                    ..Default::default()
                });
            }
            let coverage = RecordChange::insert(&document.artwork.coverage, mask.source.clone());
            let handle = coverage.handle;
            mask.use_.source = handle;
            let draft = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("exposure").unwrap().program());

            let effect = RecordChange::insert(&document.artwork.effects, EffectApplication::new(draft.program, draft.values, document.composition().size));
            let mut occurrence = Occurrence::new(OccurrenceContent::Effect(effect.handle), "Masked exposure");
            occurrence.mask = Some(mask.use_);
            let occurrence = RecordChange::insert(&document.artwork.occurrences, occurrence);
            let stack = document.composition().result;
            let mut entries = document.artwork.stacks.get(stack).unwrap().clone();
            entries.entries.insert(0, occurrence.handle);
            engine.apply_edit(Edit::Batch(vec![Edit::Coverage(coverage), Edit::Effect(effect), Edit::Occurrence(occurrence),
                Edit::Stack(RecordChange::replace(&document.artwork.stacks, stack, Some(entries)).unwrap())])).unwrap();
            engine.render_frame().unwrap();
            assert!(!engine.backend().saw_reset, "new mask initializes without restoring existing paint");
            assert_eq!(engine.document().target_raster(target), Some(&raster));
            assert_eq!(engine.document().artwork.coverage.get(handle), Some(&mask.source));
        }
        let handle = engine.document().artwork.coverage.iter().next().unwrap().0;
        for kind in 0..2 {
            let mut source = engine.document().artwork.coverage.get(handle).unwrap().clone();
            match kind {
                0 => source.domain = [128; 2],
                _ => source.default_coverage = 0.75,
            }
            engine.backend_mut().saw_reset = false;
            engine.apply_edit(Edit::Coverage(RecordChange::replace(&engine.document().artwork.coverage, handle, Some(source)).unwrap())).unwrap();
            engine.render_frame().unwrap();
            assert!(engine.backend().saw_reset, "changed mask initialization still replaces resident pages");
        }
    }

    #[test]
    fn a_bake_retains_its_removed_occurrences_and_roots_until_it_has_run() {
        let (_, mut engine) = engine("bake", 1024, 768);
        let lower = engine.document().working.occurrence.unwrap();
        let (upper, _, insertion) = paint_insert(engine.document(), empty_paint(engine.document()), "Upper", 0);
        engine.apply_edit(Edit::Batch(insertion)).unwrap();
        engine.set_active_layer(upper).unwrap();
        engine.render_frame().unwrap();
        let before = engine.document().clone();
        let plan = engine.document().merge_plan(layer_core::MergeKind::Down).unwrap();
        let target = plan.target;
        engine.insert_with_operations(plan.edits, vec![(target, plan.operation)], None).unwrap();
        assert!(engine.composite_all, "the members' area is recomposited");
        assert!(engine.document().scene().occurrence(upper).is_none() && engine.document().scene().occurrence(lower).is_none());
        engine.render_frame().unwrap();
        assert_eq!(engine.backend().bake_members, [vec![upper, lower]], "the bake resolves both retired occurrences against its immutable scene");
        engine.render_frame().unwrap();
        assert!(engine.backend().bake_members.is_empty(), "released once the bake has run");
        assert!(engine.undo().unwrap());
        assert_authored_eq(&engine.document().artwork, &before.artwork);
    }

    #[test]
    fn large_operations_reserve_their_pages_and_erase_settles_material_state() {
        use layer_core::raster::{RasterPlane, TileBlob, TILE_SIZE};
        let [width, height] = [12288, 8192];
        let (_, mut engine) = engine("large erase", width, height);
        engine.render_frame().unwrap();
        let layer = engine.document().working.target.unwrap();
        let mut everything = layer_core::Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 1., y: 0. },
            Point { x: 1., y: 1. },
        ])
        .unwrap();
        everything.inverted = true;
        let op = erase(&mut engine, &everything);
        engine.append_raster_operation(layer, op).unwrap();
        let pages = u64::from(width.div_ceil(TILE_SIZE) * height.div_ceil(TILE_SIZE));
        let tile = TileBlob::max_compressed_len(RasterPlane::Color.descriptor(engine.document().composition().color)).unwrap() as u64;
        assert!(pages * tile > layer_core::raster::MAX_CAPTURE_BYTES);
        assert!(
            engine.retained_tiles().resident_bytes() as u64 >= pages * tile,
            "the pending pixels reserve every page they may write"
        );
        engine.render_frame().unwrap();

        let small = layer_core::Selection::polygon(vec![
            Point { x: 10., y: 10. },
            Point { x: 20., y: 10. },
            Point { x: 20., y: 20. },
        ])
        .unwrap();
        let op = erase(&mut engine, &small);
        engine.append_raster_operation(layer, op).unwrap();
        assert_eq!(engine.batches[0].damage, small.bounds(), "plain pixels erase only the selected pages");

        let mut document = Document::new(layer_core::authored::PortableId::random(), 512, 512, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        active_paint_mut(&mut document).raster = layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
            watercolor: Some(layer_core::raster::RasterWatercolor { wet_edge: 0.5, burnt_edge: 0.2, edge_width: 2. }),
            ..Default::default()
        });
        let (_, mut engine) = engine_with(RecordingRenderer::default(), document, view(512, 512), TRANSFORM);
        let layer = engine.document().working.target.unwrap();
        let op = erase(&mut engine, &small);
        engine.append_raster_operation(layer, op).unwrap();
        assert_eq!(
            engine.batches[0].damage,
            Rect::from_extent([512, 512]),
            "watercolor settles across the whole layer before erasing"
        );
    }

    #[test]
    fn appended_raster_operations_are_incremental_and_undo_restores_revisions() {
        for kind in [
            layer_core::RasterOperationKind::Transform(layer_core::ImageTransform::affine(layer_core::Affine::translation(Point { x: 10., y: 4. }))),
            layer_core::RasterOperationKind::Gradient { texture: Default::default(),
                start: Point::default(),
                end: Point { x: 128.0, y: 0.0 },
                gradient: layer_core::GradientDefinition::new(vec![layer_core::GradientStop {position:0.,color:layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb,[1.,0.,0.,1.]).unwrap()},layer_core::GradientStop {position:1.,color:layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb,[0.,0.,1.,1.]).unwrap()}]),
                shape:layer_core::GradientShape::Linear, reverse:false, opacity:1.,
                alpha_locked: false,
            },
        ] {
            let (mut producer, mut engine) = engine("gradient", 128, 128);
            let id = engine.document().working.target.unwrap();
            engine.render_frame().unwrap();
            engine.backend.saw_reset = false;
            producer.push(event(1, PenPhase::Down, 20.0)).unwrap();
            producer.push(event(2, PenPhase::Up, 80.0)).unwrap();
            engine.render_frame().unwrap();
            let count = engine.backend.persistent_dabs;
            let coverage =
                layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(), engine.document().composition().size, [0; 2]);
            let op = layer_core::RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind };
            engine.append_raster_operation(id, op).unwrap();
            assert!(engine.has_pending_document_edits());
            let operated = engine.document().target_raster(id).unwrap().identity();
            assert_eq!(
                engine.document().target_operations(id).unwrap()
                    .len(),
                1
            );
            assert!(matches!(
                engine.batches[0].kind,
                DabBatchKind::RasterOperation(0)
            ));
            engine.render_frame().unwrap();
            assert!(!engine.backend.saw_reset);
            assert_eq!(
                engine.backend.persistent_dabs, count,
                "must not replay old strokes when appending a raster operation"
            );
            assert!(!engine.has_pending_document_edits());
            engine.undo().unwrap();
            assert!(
                engine.document().target_operations(id).unwrap()
                    .is_empty()
            );
            engine.render_frame().unwrap();
            assert!(!engine.backend.saw_reset);
            engine.redo().unwrap();
            engine.render_frame().unwrap();
            assert_eq!(
                engine.document().target_raster(id).unwrap().identity(),
                operated
            );
            assert!(
                engine.document().target_operations(id).unwrap()
                    .is_empty()
            );
            assert_eq!(engine.backend.persistent_dabs, count);
        }
    }

    #[test]
    fn captured_rapid_lifts_match_replay_without_repainting_the_committed_prefix() {
        let rows: Vec<Vec<f32>> = include_str!("../tests/fixtures/wacom-rapid-lift.csv")
            .lines()
            .skip(1)
            .map(|line| line.split(',').map(|v| v.parse().unwrap()).collect())
            .collect();
        for capture in 0..5 {
            let samples: Vec<_> = rows.iter().filter(|r| r[0] as usize == capture).collect();
            let mut reference = None;
            for feedback in [false, true] {
                for cadence in [1, 4, 64] {
                    let (mut input, mut engine) = engine("rapid lift", 1024, 512);
                    let mut brush = default_brush(DefaultBrushPreset::GPen);
                    brush.diameter = samples[0][6];
                    brush.stabilization.strength = 0.;
                    engine.set_brush(brush).unwrap();
                    engine
                        .set_instant_feedback(InstantFeedbackConfig {
                            enabled: feedback,
                            ..Default::default()
                        })
                        .unwrap();
                    engine.render_frame().unwrap();
                    engine.backend.saw_reset = false;
                    let mut before_up = None;
                    for (i, row) in samples.iter().enumerate() {
                        let phase = if i == 0 {
                            PenPhase::Down
                        } else if i + 1 == samples.len() {
                            PenPhase::Up
                        } else {
                            PenPhase::Move
                        };
                        input
                            .push(PenEvent {
                                timestamp_ns: 1_000_000_000 + row[5] as u64 * 1000,
                                surface_position: Point {
                                    x: row[2],
                                    y: row[3],
                                },
                                pressure: row[4],
                                ..event(i as u64 + 1, phase, row[2])
                            })
                            .unwrap();
                        if (i + 1) % cadence == 0 || i + 1 == samples.len() {
                            engine.render_frame().unwrap();
                        }
                        if cadence == 1 && i + 2 == samples.len() {
                            let mut visible = engine.backend.persistent.clone();
                            visible.extend_from_slice(&engine.backend.preview);
                            before_up = Some(visible);
                        }
                    }
                    let stroke = engine.completed_stroke.as_ref().unwrap();
                    assert_eq!(stroke.points.len(), samples.len());
                    for (point, row) in stroke.points.iter().zip(&samples) {
                        assert_eq!(point.pressure, row[4], "raw pressure must survive release");
                    }
                    let mut replay = Vec::new();
                    DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
                    assert_eq!(
                        engine.backend.persistent, replay,
                        "capture={capture}, cadence={cadence}, feedback={feedback}"
                    );
                    assert_eq!(
                        replay.last().unwrap().center,
                        stroke.points.last().unwrap().position
                    );
                    assert!(replay.last().unwrap().contact[0] > 0.);
                    if !feedback
                        && let Some(preview) = before_up
                    {
                        assert_eq!(preview, replay, "preview capture={capture}");
                    }
                    if let Some(reference) = &reference {
                        assert_eq!(&replay, reference);
                    } else {
                        reference = Some(replay);
                    }
                    assert!(
                        !engine.backend.saw_reset,
                        "ordinary release must not repaint the stroke"
                    );
                    assert!(engine.backend.preview.is_empty());
                    if !feedback {
                        assert_eq!(engine.metrics.engine_prediction_frames, 0);
                    }
                    let raster = active_paint(engine.document()).raster.identity();
                    assert!(engine.undo().unwrap());
                    assert!(active_paint(engine.document()).raster.is_empty());
                    assert!(!engine.undo().unwrap());
                    assert!(engine.redo().unwrap());
                    assert_eq!(active_paint(engine.document()).raster.identity(), raster);
                }
            }
        }
    }

    #[test]
    fn pressure_limiter_is_pressure_tool_only_and_cancellation_needs_no_release_tail() {
        for tool in [
            ToolKind::Pen,
            ToolKind::Mouse,
            ToolKind::Finger,
            ToolKind::Eraser,
        ] {
            let (mut input, mut engine) = engine("release tools", 128, 128);
            let mut brush = default_brush(DefaultBrushPreset::GPen);
            brush.stabilization.strength = 0.;
            engine
                .set_brush(brush)
                .unwrap();
            engine
                .set_instant_feedback(InstantFeedbackConfig {
                    enabled: false,
                    ..Default::default()
                })
                .unwrap();
            engine.render_frame().unwrap();
            for (i, phase) in [PenPhase::Down, PenPhase::Move].into_iter().enumerate() {
                input
                    .push(PenEvent {
                        tool,
                        ..event(i as u64 + 1, phase, 16. + i as f32 * 20.)
                    })
                    .unwrap();
                engine.render_frame().unwrap();
            }
            assert_eq!(
                engine.active_stroke.as_ref().unwrap().brush.stabilization.pressure_fall_micros > 0,
                tool == ToolKind::Pen
            );
            assert!(!engine.active_stroke.as_ref().unwrap().feedback.enabled);
            assert!(engine.backend.preview.is_empty());
            input
                .push(PenEvent {
                    tool,
                    ..event(3, PenPhase::Cancel, 36.)
                })
                .unwrap();
            engine.render_frame().unwrap();
            assert!(!engine.has_active_stroke());
            assert!(engine.backend.preview.is_empty());
            assert_eq!(engine.metrics.committed_strokes, 0);
            assert!(!engine.undo().unwrap());
        }
    }

    #[test]
    fn native_color_dynamics_match_cursor_corrections_recovery_and_next_contact() {
        use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
        use layer_core::{BrushCombine, BrushCurve, BrushMapping, BrushSensor, BrushTarget};
        for space in RgbSpace::ALL {
            let mut depths = Vec::new();
            for depth in [SampleDepth::U8, SampleDepth::U16] {
                let color = DocumentColor { space, depth };
                let mut variants = Vec::new();
                for recover in [false, true] {
                    for correct_after_up in [false, true] {
                        let mut document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
                        composition_mut(&mut document).color = color;
                        let (mut input, mut engine) = engine_with(
                            RecordingRenderer {
                                color,
                                ..Default::default()
                            },
                            document,
                            view(128, 128),
                            TRANSFORM,
                        );
                        let mut brush = BrushSnapshot {
                            color_rgba_linear: [0.8, 0.15, 0.31, 0.37],
                            mappings: [BrushMapping {
                                sensor: BrushSensor::Pressure,
                                target: BrushTarget::Hue,
                                combine: BrushCombine::Replace,
                                input_min: 0.,
                                input_max: 1.,
                                output_scale: 0.2,
                                output_bias: 0.1,
                                curve: BrushCurve::LINEAR,
                            }]
                            .into(),
                            ..Default::default()
                        };
                        brush.color_dynamics.stamp_hue_jitter = 0.15;
                        brush.color_dynamics.stroke_saturation_jitter = 0.12;
                        brush.color_dynamics.stamp_secondary_jitter = 0.3;
                        brush.shape.count = 3;
                        engine.set_brush(brush.clone()).unwrap();
                        engine.render_frame().unwrap();
                        let down = event(1, PenPhase::Down, 8.);
                        // The host may reuse the same hover state across documents.
                        let mut hover = DabGenerator::default();
                        let actual = engine.cursor_contacts(down, &mut hover, down.timestamp_ns);
                        let mut expected_hover = DabGenerator::new(space);
                        expected_hover.cursor_seed(engine.document().next_stroke_id(), &brush);
                        let expected = expected_hover.cursor_contacts(
                            layer_core::StrokePoint {
                                position: down.surface_position,
                                pressure: 1.,
                                tilt: [0.; 2],
                                twist: 0.,
                                elapsed_micros: 0,
                            },
                            &brush,
                        );
                        assert_eq!(actual, expected);
                        let mut estimated = down;
                        estimated.pressure = 0.3;
                        estimated.flags = SampleFlags::ESTIMATED;
                        input.push(estimated).unwrap();
                        input.push(event(2, PenPhase::Move, 32.)).unwrap();
                        engine.render_frame_at(2_000_000).unwrap();
                        if recover {
                            engine
                                .replace_backend(RecordingRenderer {
                                    color,
                                    ..Default::default()
                                })
                                .unwrap();
                            engine.render_frame_at(2_000_000).unwrap();
                        }
                        let mut correction = down;
                        correction.flags = SampleFlags::CORRECTION;
                        let up = event(3, PenPhase::Up, 64.);
                        for e in if correct_after_up {
                            [up, correction]
                        } else {
                            [correction, up]
                        } {
                            input.push(e).unwrap();
                            engine.render_frame_at(3_000_000).unwrap();
                        }
                        engine.render_frame().unwrap();
                        let stroke = engine.completed_stroke.as_ref().unwrap();
                        let mut expected = Vec::new();
                        DabGenerator::generate(stroke, space, &mut expected);
                        assert_eq!(
                            engine.backend().persistent,
                            expected,
                            "{color:?}, recover {recover}, late {correct_after_up}"
                        );
                        variants.push(expected);
                        assert_eq!(engine.metrics().committed_strokes, 1);
                        assert!(engine.undo().unwrap());
                        engine.render_frame().unwrap();
                        assert!(!engine.can_undo());
                        // Undo/reset does not reset document color for the next contact.
                        // The recorder logs submitted dabs, not raster restoration.
                        engine.backend_mut().persistent.clear();
                        input.push(event(5, PenPhase::Down, 16.)).unwrap();
                        input.push(event(6, PenPhase::Up, 48.)).unwrap();
                        engine.render_frame().unwrap();
                        let mut expected = Vec::new();
                        DabGenerator::generate(
                            engine.completed_stroke.as_ref().unwrap(),
                            space,
                            &mut expected,
                        );
                        assert_eq!(engine.backend().persistent, expected);
                    }
                }
                assert!(variants.windows(2).all(|pair| pair[0] == pair[1]));
                depths.push(variants.remove(0));
            }
            assert_eq!(
                depths[0], depths[1],
                "depth never changes dab color arithmetic"
            );
        }
    }

    #[test]
    fn native_document_adoption_and_recovery_require_matching_renderer_interpretation() {
        use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
        for space in RgbSpace::ALL {
            for depth in [SampleDepth::U8, SampleDepth::U16] {
                let color = DocumentColor { space, depth };
                let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
                composition_mut(&mut document).color = color;
                if color != Default::default() {
                    let (_, consumer) = input_queue(8);
                    let result = CanvasEngine::new(
                        RecordingRenderer {
                            fail_resize: true,
                            ..Default::default()
                        },
                        document.clone(),
                        consumer,
                        view(64, 64),
                        ViewTransform::IDENTITY,
                    );
                    assert!(
                        matches!(result, Err(EngineError::Document(_))),
                        "color mismatch must fail before resize"
                    );
                }
                let (mut input, mut engine) = engine_with(
                    RecordingRenderer {
                        color,
                        ..Default::default()
                    },
                    document,
                    view(64, 64),
                    TRANSFORM,
                );
                engine.render_frame().unwrap();
                input.push(event(1, PenPhase::Down, 8.)).unwrap();
                input.push(event(2, PenPhase::Up, 24.)).unwrap();
                let wrong = DocumentColor {
                    space,
                    depth: if depth == SampleDepth::U8 {
                        SampleDepth::U16
                    } else {
                        SampleDepth::U8
                    },
                };
                let before = engine.checkpoint();
                assert!(matches!(
                    engine.replace_backend(RecordingRenderer {
                        color: wrong,
                        fail_resize: true,
                        ..Default::default()
                    }),
                    Err(EngineError::Document(_))
                ));
                assert_eq!(engine.document().composition().color, color);
                assert_eq!(engine.backend().color, color);
                assert_eq!(engine.checkpoint(), before);
                assert_eq!(engine.metrics().input_events, 0);
                engine.render_frame().unwrap();
                assert_eq!(engine.metrics().input_events, 2);
                engine
                    .replace_backend(RecordingRenderer {
                        color,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(engine.document().composition().color, color);
            }
        }
    }

    #[test]
    fn color_edits_and_history_reject_an_unprepared_renderer_without_consuming_input() {
        use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
        let (mut input, mut engine) = engine("color edit", 64, 64);
        engine.render_frame().unwrap();
        let original = engine.document().clone();
        let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
        let edit = color_edit(&original, color);
        input.push(event(1, PenPhase::Down, 8.)).unwrap();
        input.push(event(2, PenPhase::Up, 24.)).unwrap();
        for edit in [edit.clone(), Edit::Batch(vec![edit.clone()])] {
            assert!(engine.preview_edit(edit.clone()).unwrap_err().to_string().contains("matching renderer"));
            assert!(engine.apply_edit(edit).unwrap_err().to_string().contains("matching renderer"));
            assert_eq!(engine.document(), &original);
            assert_eq!(engine.checkpoint(), 0);
            assert_eq!(engine.metrics().input_events, 0);
        }
        // Arrange a valid model color history to check both direction guards.
        engine.editor.perform(edit).unwrap();
        engine.backend.color = color;
        let converted = engine.document().clone();
        let checkpoint = engine.checkpoint();
        assert!(engine.undo().unwrap_err().to_string().contains("matching renderer"));
        assert_eq!(engine.document(), &converted);
        assert_eq!(engine.checkpoint(), checkpoint);
        engine.editor.undo().unwrap();
        engine.backend.color = original.composition().color;
        let restored = engine.document().clone();
        assert!(engine.redo().unwrap_err().to_string().contains("matching renderer"));
        assert_eq!(engine.document(), &restored);
        assert_eq!(engine.checkpoint(), 0);
        assert_eq!(engine.metrics().input_events, 0);
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 2);
    }

    #[test]
    fn prepared_color_and_history_publish_together_and_reject_failure_or_staleness() {
        use layer_core::{ColorTransition, color::{DocumentColor, SampleDepth, RgbSpace}};
        let (_, mut engine) = engine("atomic color", 64, 64);
        engine.render_frame().unwrap();
        let original = engine.document().clone();
        let old_brush = engine.brush().clone();
        let target = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
        let prepare = |engine: &CanvasEngine<RecordingRenderer>| engine.prepare_color_transition(ColorTransition::Apply {
            edit: Box::new(color_edit(engine.document(), target)),
        }).unwrap();
        for fail in [false, true] {
            let prepared = prepare(&engine);
            engine.backend.fail_color_adoption = fail;
            let error = engine.commit_color_transition(prepared).unwrap_err();
            assert_eq!(matches!(error, EngineError::Backend(_)), fail);
            assert_eq!(engine.document(), &original);
            assert_eq!(engine.checkpoint(), 0);
            assert!(!engine.can_undo());
            assert_eq!(engine.backend.color, original.composition().color);
            assert_eq!(engine.brush(), &old_brush);
        }
        engine.backend.fail_color_adoption = false;
        let stale = prepare(&engine);
        engine.set_layer_opacity(original.working.occurrence.unwrap(), 0.5).unwrap();
        let changed = engine.document().clone();
        let adoptions = engine.backend.color_adoptions;
        engine.backend.prepared_color = Some(target);
        assert!(engine.commit_color_transition(stale).unwrap_err().to_string().contains("changed during"));
        assert_eq!(engine.document(), &changed);
        assert_eq!(engine.backend.color_adoptions, adoptions);
        assert_eq!(engine.backend.color, original.composition().color);
        let before_color_checkpoint = engine.checkpoint();
        let prepared = prepare(&engine);
        let expected = prepared.document().clone();
        engine.commit_color_transition(prepared).unwrap();
        assert_eq!(engine.document(), &expected);
        assert_eq!(engine.backend.color, target);
        let after_color_checkpoint = engine.checkpoint();
        assert_eq!(engine.history_color(false), original.composition().color);
        engine.render_frame().unwrap();
        assert!(engine.backend.saw_reset);
        for _ in 0..3 {
            let prepared = engine.prepare_color_transition(ColorTransition::Undo).unwrap();
            engine.backend.prepared_color = Some(original.composition().color);
            engine.commit_color_transition(prepared).unwrap();
            assert_eq!(engine.document().composition().color, original.composition().color);
            assert_eq!(engine.backend.color, original.composition().color);
            assert_authored_eq(&engine.document().artwork, &changed.artwork);
            assert_eq!(engine.checkpoint(), before_color_checkpoint);
            let prepared = engine.prepare_color_transition(ColorTransition::Redo).unwrap();
            engine.backend.prepared_color = Some(target);
            engine.commit_color_transition(prepared).unwrap();
            assert_eq!(engine.document().composition().color, target);
            assert_eq!(engine.backend.color, target);
            assert_authored_eq(&engine.document().artwork, &expected.artwork);
            assert_eq!(engine.checkpoint(), after_color_checkpoint);
        }
    }

    #[test]
    fn prepared_color_does_not_consume_queued_input_or_overflow_tool_coordinates() {
        use layer_core::{ColorTransition, color::{DocumentColor, SampleDepth, RgbSpace}};
        let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        composition_mut(&mut document).color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
        let (mut input, mut engine) = engine_with(
            RecordingRenderer { color: document.composition().color, ..Default::default() }, document,
            view(64, 64), TRANSFORM,
        );
        engine.render_frame().unwrap();
        let target = DocumentColor::default();
        let prepare = |engine: &CanvasEngine<RecordingRenderer>| engine.prepare_color_transition(ColorTransition::Apply {
            edit: Box::new(color_edit(engine.document(), target)),
        }).unwrap();
        let mut brush = engine.configured_brush().clone();
        brush.color_rgba_linear = [f32::MAX, 0., 0., 1.];
        engine.set_brush(brush).unwrap();
        let prepared = prepare(&engine);
        let before = engine.document().clone();
        engine.backend.prepared_color = Some(target);
        assert!(matches!(engine.commit_color_transition(prepared), Err(EngineError::Document(DocumentError::InvalidBrush(_)))));
        assert_eq!(engine.document(), &before);
        assert_eq!(engine.backend.color_adoptions, 0);
        engine.set_brush(default_brush(DefaultBrushPreset::GPen)).unwrap();
        let prepared = prepare(&engine);
        input.push(event(1, PenPhase::Down, 8.)).unwrap();
        assert!(engine.commit_color_transition(prepared).unwrap_err().to_string().contains("current canvas operation"));
        assert_eq!(engine.document(), &before);
        assert_eq!(engine.backend.color_adoptions, 0);
        assert_eq!(engine.metrics().input_events, 0);
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 1);
        assert!(engine.has_active_stroke());
        assert!(engine.prepare_color_transition(ColorTransition::Apply {
            edit: Box::new(color_edit(engine.document(), target)),
        }).is_err());
    }

    #[test]
    fn pending_restore_retains_one_prepared_frame_without_consuming_the_next_contact() {
        let (mut input, mut engine) = engine("deferred frame", 64, 64);
        engine.render_frame().unwrap();
        let frames = engine.metrics().frames;
        engine.backend_mut().restore_blocked = true;
        input.push(event(1, PenPhase::Down, 8.)).unwrap();
        input.push(event(2, PenPhase::Up, 24.)).unwrap();
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 2);
        assert_eq!(engine.metrics().frames, frames);
        assert!(engine.has_pending_document_edits());
        assert!(active_paint(engine.document()).raster.try_data().is_none());
        assert!(engine.can_undo());
        assert!(engine.raster_backing_pending());
        assert!(engine.undo().is_err());
        input.push(event(3, PenPhase::Down, 32.)).unwrap();
        input.push(event(4, PenPhase::Up, 48.)).unwrap();
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 2);
        assert_eq!(engine.backend().persistent_dabs, 0);
        assert!(
            engine
                .replace_backend(RecordingRenderer::default())
                .is_err()
        );
        assert!(
            engine.backend().restore_blocked,
            "a prepared frame keeps its owning renderer"
        );
        assert_eq!(engine.metrics().input_events, 2);
        engine.backend_mut().restore_blocked = false;
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().frames, frames + 1);
        assert_eq!(engine.metrics().input_events, 2);
        assert!(active_paint(engine.document()).raster.host_backed());
        assert!(engine.backend().persistent_dabs > 0);
        assert!(engine.can_undo());
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 4);
        assert_eq!(engine.metrics().committed_strokes, 2);
        assert!(engine.undo().unwrap());
        assert!(engine.undo().unwrap());
        assert!(!engine.undo().unwrap());
    }

    #[test]
    fn capture_backpressure_allows_live_moves_and_defers_only_the_commit_boundary() {
        let (mut input, mut engine) = engine("backpressure", 64, 64);
        engine.render_frame().unwrap();
        engine.backend_mut().capture_blocked = true;
        for (sequence, phase, x) in [
            (1, PenPhase::Down, 8.),
            (2, PenPhase::Move, 16.),
            (3, PenPhase::Up, 24.),
        ] {
            input.push(event(sequence, phase, x)).unwrap();
        }
        engine.render_frame_for(10_000_000, 18_000_000).unwrap();
        assert_eq!(engine.metrics().input_events, 2);
        assert!(engine.backend().persistent_dabs > 0);
        assert!(engine.has_active_stroke() && engine.has_pending_input());
        assert_eq!(engine.metrics().committed_strokes, 0);
        assert!(active_paint(engine.document()).raster.is_empty());
        let frames = engine.metrics().frames;
        let dabs = engine.backend().persistent_dabs;
        engine.render_frame_for(200_000_000, 208_000_000).unwrap();
        assert_eq!(engine.metrics().frames, frames);
        assert_eq!(
            engine.backend().persistent_dabs,
            dabs,
            "waiting pen-up must not advance continuous ink"
        );
        engine.backend_mut().capture_blocked = false;
        engine.render_frame_for(210_000_000, 218_000_000).unwrap();
        assert_eq!(engine.metrics().input_events, 3);
        assert_eq!(engine.metrics().committed_strokes, 1);
        assert!(!engine.has_active_stroke() && !engine.has_pending_input());
        assert!(engine.undo().unwrap());
        assert!(!engine.undo().unwrap());
    }

    #[test]
    fn live_contact_budget_cancels_without_committing_partial_pixels() {
        let (mut input, mut engine) = engine("bounded", 64, 64);
        input.push(event(1, PenPhase::Down, 8.)).unwrap();
        engine.render_frame().unwrap();
        let checkpoint = engine.checkpoint();
        for sequence in 2..=MAX_CONTACT_POINTS as u64 {
            engine.builder.push(
                event(sequence, PenPhase::Move, 8.),
                ViewTransform::IDENTITY,
                PressureCurve::default(),
            );
        }
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_budget_cancellations,1);
        assert!(!engine.has_active_stroke());
        assert_eq!(engine.checkpoint(), checkpoint);
        assert!(!engine.can_undo());
        engine.render_frame().unwrap();
        assert!(active_paint(engine.document()).raster.is_empty());
    }

    #[test]
    fn unresolved_estimates_bound_the_preview_and_preserve_late_corrections() {
        for platform_prediction in [false, true] {
            let (mut input, mut engine) = engine("unresolved estimates", 128, 128);
            engine
                .set_instant_feedback(InstantFeedbackConfig {
                    use_platform_prediction: platform_prediction,
                    ..InstantFeedbackConfig::default()
                })
                .unwrap();
            let mut down = event(1, PenPhase::Down, 8.);
            down.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
            // A driver may mark every sample as awaiting sensor updates without
            // ever delivering a correction. Reproduce two seconds at 240 Hz.
            for index in 0..481 {
                let sample = PenEvent {
                    sequence: index + 1,
                    timestamp_ns: down.timestamp_ns + index * 4_166_667,
                    phase: if index == 0 {
                        PenPhase::Down
                    } else {
                        PenPhase::Move
                    },
                    surface_position: Point {
                        x: 8. + (index as f32 * 0.07).sin() * 6.,
                        y: 16.,
                    },
                    ..down
                };
                input.push(sample).unwrap();
                input
                    .push(PenEvent {
                        timestamp_ns: sample.timestamp_ns + 8_000_000,
                        phase: PenPhase::Move,
                        flags: SampleFlags::PREDICTED,
                        ..sample
                    })
                    .unwrap();
                engine
                    .render_frame_for(sample.timestamp_ns, sample.timestamp_ns + 8_000_000)
                    .unwrap();
                assert!(
                    engine.builder.real_points().len() - engine.finalized_real_points <= 13,
                    "unresolved estimates must not make preview work grow with stroke length"
                );
            }
            assert_eq!(engine.estimates.len(), 481, "keep late correction tokens");
            assert!(!engine.backend().persistent.is_empty());
            let correction = PenEvent {
                flags: SampleFlags::CORRECTION,
                pressure: 0.2,
                ..down
            };
            engine.backend_mut().saw_reset = false;
            input.push(correction).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.builder.real_points()[0].pressure, 0.2);
            assert!(
                engine.backend().saw_reset,
                "rebuild corrected persistent ink"
            );

            input
                .push(PenEvent {
                    sequence: 482,
                    timestamp_ns: down.timestamp_ns + 2_010_000_000,
                    phase: PenPhase::Up,
                    flags: SampleFlags::PRIMARY,
                    ..down
                })
                .unwrap();
            engine.render_frame().unwrap();
            let stroke = engine.completed_stroke.as_ref().unwrap();
            assert_eq!(stroke.points.len(), 482);
            assert_eq!(stroke.points[0].pressure, 0.2);
            let mut replay = Vec::new();
            DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
            assert_eq!(engine.backend().persistent, replay);
            assert!(engine.backend().preview.is_empty());
            let raster = active_paint(engine.document()).raster.identity();
            assert!(engine.undo().unwrap());
            assert!(active_paint(engine.document()).raster.is_empty());
            assert!(
                !engine.undo().unwrap(),
                "corrections must not add history steps"
            );
            assert!(engine.redo().unwrap());
            assert_eq!(active_paint(engine.document()).raster.identity(), raster);
        }
    }

    #[test]
    fn calibration_changes_keep_contact_cursor_and_recorded_pressure_on_the_captured_curve() {
        let (mut input,mut engine)=engine("pressure preview",512,512);
        engine.settings.instant_feedback.enabled=false;
        let original=layer_core::PressureResponse::default();engine.set_pressure_curve(original.clone().into());
        engine.recording.lock().unwrap().start("pressure-test").unwrap();
        let mut down=event(1,PenPhase::Down,20.);down.pressure=0.2;
        engine.capture_queued_contact(down);input.push(down).unwrap();engine.record_raw_input(down,ViewTransform::IDENTITY);
        engine.render_frame().unwrap();
        let mut cursor=DabGenerator::new(layer_core::color::RgbSpace::Srgb);
        let before=engine.cursor_contacts(down,&mut cursor,0);
        assert!(!before.is_empty());
        engine.set_pressure_curve(PressureCurve::default());
        let after=engine.cursor_contacts(down,&mut cursor,0);
        assert_eq!(before.iter().map(|d|d.radii).collect::<Vec<_>>(),after.iter().map(|d|d.radii).collect::<Vec<_>>());
        let mut moved=event(2,PenPhase::Move,24.);moved.pressure=0.2;
        engine.record_raw_input(moved,ViewTransform::IDENTITY);input.push(moved).unwrap();engine.render_frame().unwrap();
        assert!(engine.builder.real_points().iter().all(|p|(p.pressure-original.map(0.2)).abs()<0.00101));
        let mut recorder=engine.recording.lock().unwrap();recorder.stop(crate::recording::StopReason::Manual);
        let records=crate::recording::read(recorder.bytes().unwrap().as_slice()).unwrap();
        for record in records {if let crate::recording::Record::Raw {pressure,..}=record {assert_eq!(pressure,original);}}
    }

    #[test]
    fn tilt_calibration_is_captured_for_queued_cursor_predicted_and_corrected_input() {
        use layer_core::{TiltAxes,TiltCalibration,TiltResponse};
        let (mut input,mut engine)=engine("tilt calibration",512,512);
        engine.settings.instant_feedback.enabled=false;
        let source=TiltCalibration{response:TiltResponse::try_from(vec![[0.,0.],[0.5,1.]]).unwrap(),axes:TiltAxes{reverse_horizontal:true,..Default::default()}};
        let curve=crate::TiltCurve::new(&source,TiltAxes::default());engine.set_tilt_curve(curve.clone());
        let mut down=event(1,PenPhase::Down,20.);down.tilt_radians=[0.3,0.];down.flags=SampleFlags::ESTIMATED;
        engine.capture_queued_contact(down);input.push(down).unwrap();
        engine.set_tilt_curve(crate::TiltCurve::default());engine.render_frame().unwrap();
        assert_eq!(engine.builder.real_points()[0].tilt,curve.projected(down.tilt_radians));
        let mut cursor=DabGenerator::new(layer_core::color::RgbSpace::Srgb);
        let before=engine.cursor_contacts(down,&mut cursor,0);
        engine.set_tilt_curve(curve.clone());let after=engine.cursor_contacts(down,&mut cursor,0);
        assert_eq!(before.iter().map(|d|d.radii).collect::<Vec<_>>(),after.iter().map(|d|d.radii).collect::<Vec<_>>());
        let mut moved=event(2,PenPhase::Move,24.);moved.tilt_radians=[0.4,0.];
        engine.set_tilt_curve(crate::TiltCurve::default());input.push(moved).unwrap();engine.render_frame().unwrap();
        let mut predicted=moved;predicted.sequence=3;predicted.flags=SampleFlags::PREDICTED;predicted.surface_position.x=28.;
        input.push(predicted).unwrap();engine.render_frame().unwrap();
        assert!(engine.builder.real_points().iter().all(|p|p.tilt[0]<0.));
        assert!(engine.builder.predicted_points().iter().all(|p|p.tilt[0]<0.));
        let mut correction=down;correction.flags=SampleFlags::CORRECTION;correction.tilt_radians=[0.,0.2];
        input.push(correction).unwrap();engine.render_frame().unwrap();
        assert_eq!(engine.builder.real_points()[0].tilt,curve.projected(correction.tilt_radians));
        let mut up=moved;up.phase=PenPhase::Up;up.sequence=4;input.push(up).unwrap();engine.render_frame().unwrap();
        assert_eq!(engine.completed_stroke.as_ref().unwrap().points[0].tilt,curve.projected(correction.tilt_radians));
        assert_eq!(engine.configured_tilt_curve(),&crate::TiltCurve::default());
    }

    #[test]
    fn queued_contacts_keep_their_brush_pressure_and_camera() {
        let (mut input, mut engine) = engine("queued contacts", 512, 512);
        engine.settings.instant_feedback.enabled = false;
        engine.settings.brush.diameter = 19.;
        engine.settings.pressure = layer_core::PressureResponse::try_from(vec![[0.,0.],[0.5,0.],[1.,1.]]).unwrap().into();
        let first_pressure=engine.settings.pressure.map(event(1,PenPhase::Down,20.).pressure);
        for event in [event(1, PenPhase::Down, 20.), event(2, PenPhase::Up, 24.)] {
            input.push(event).unwrap();
            engine.capture_queued_contact(event);
        }
        engine.settings.brush.diameter = 43.;
        engine.settings.pressure = layer_core::PressureResponse::try_from(vec![[0.,0.],[1./3.,0.],[2./3.,0.],[1.,1.]]).unwrap().into();
        let second_pressure=engine.settings.pressure.map(event(3,PenPhase::Down,30.).pressure);
        assert!((first_pressure-second_pressure).abs()>0.01);
        for revision in 2..100 {
            engine.set_view(view(512, 512), ViewTransform { revision, surface_to_document: [1., 0., 0., 1., 100., 80.] });
        }
        for mut event in [event(3, PenPhase::Down, 30.), event(4, PenPhase::Up, 34.)] {
            event.view_revision = 99;
            input.push(event).unwrap();
            engine.capture_queued_contact(event);
        }
        engine.settings.brush.diameter = 7.;
        engine.settings.pressure = PressureCurve::default();
        engine.render_frame().unwrap();
        let first = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(first.brush.diameter, 19.);
        assert_eq!(first.points[0].position, Point { x: 20., y: 16. });
        assert_eq!(first.points[0].pressure,first_pressure);
        engine.render_frame().unwrap();
        let second = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(second.brush.diameter, 43.);
        assert_eq!(second.points[0].position, Point { x: 130., y: 96. });
        assert_eq!(second.points[0].pressure,second_pressure);
        assert_eq!(engine.brush().diameter, 7.);
        assert_eq!(engine.settings.pressure, PressureCurve::default());
        assert!(engine.queued_contacts.is_empty());
        assert!(engine.undo().unwrap());
        assert!(engine.undo().unwrap());
        assert!(!engine.undo().unwrap());
    }

    #[test]
    fn delayed_stroke_publication_preserves_corrections_then_expires_and_keeps_one_undo_step() {
        let (mut input, mut engine) = engine("delayed stroke publication", 64, 64);
        engine.render_frame().unwrap();
        let before = active_paint(engine.document()).raster.identity();
        engine.backend_mut().restore_blocked = true;
        let mut down = event(1, PenPhase::Down, 8.);
        down.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
        input.push(down).unwrap();
        input.push(event(2, PenPhase::Up, 12.)).unwrap();
        engine.render_frame().unwrap();
        let pending = active_paint(engine.document()).raster.clone();
        assert!(pending.try_data().is_none());
        engine.completed_at = Some(web_time::Instant::now() - CORRECTION_WINDOW * 2);
        down.flags = SampleFlags::CORRECTION;
        down.pressure = 0.1;
        let corrected = engine.settings.pressure.map(down.pressure);
        input.push(down).unwrap();
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 2);
        assert!(engine.completed_stroke.is_some());
        assert!(pending.try_data().is_none());
        engine.backend_mut().restore_blocked = false;
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 2);
        assert!(pending.try_data().is_some_and(|data| data.is_ok()));
        engine.backend_mut().saw_reset = false;
        engine.render_frame().unwrap();
        assert_eq!(engine.metrics().input_events, 3);
        assert_eq!(engine.completed_stroke.as_ref().map(|stroke| stroke.points[0].pressure), Some(corrected));
        assert!(engine.backend().saw_reset);
        assert_eq!(engine.metrics().committed_strokes, 1);
        let repaired = active_paint(engine.document()).raster.identity();
        assert_ne!(repaired, pending.identity());
        engine.completed_at = Some(web_time::Instant::now() - CORRECTION_WINDOW * 2);
        engine.render_frame().unwrap();
        assert!(engine.completed_stroke.is_none());
        assert!(engine.estimates.is_empty());
        assert_eq!(active_paint(engine.document()).raster.identity(), repaired);
        assert!(engine.undo().unwrap());
        engine.render_frame().unwrap();
        assert_eq!(active_paint(engine.document()).raster.identity(), before);
        assert!(!engine.can_undo());
        assert!(engine.redo().unwrap());
        engine.render_frame().unwrap();
        assert_eq!(active_paint(engine.document()).raster.identity(), repaired);
        assert!(engine.completed_stroke.is_none());
        assert!(engine.estimates.is_empty());
    }

    #[test]
    fn unrelated_submission_completion_does_not_extend_the_correction_window() {
        let (mut input, mut engine) = engine("unrelated submission", 64, 64);
        let mut down = event(1, PenPhase::Down, 8.);
        down.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
        input.push(down).unwrap();
        input.push(event(2, PenPhase::Up, 12.)).unwrap();
        engine.render_frame().unwrap();
        let root = active_paint(engine.document()).raster.identity();
        engine.completed_at = Some(web_time::Instant::now() - CORRECTION_WINDOW * 2);
        engine.backend_mut().unrelated_submission = true;
        down.flags = SampleFlags::CORRECTION;
        down.pressure = 0.1;
        input.push(down).unwrap();
        engine.render_frame().unwrap();
        assert!(!engine.backend().unrelated_submission);
        assert_eq!(engine.metrics().input_events, 3);
        assert!(engine.completed_stroke.is_none());
        assert!(engine.completed_before.is_none());
        assert!(engine.estimates.is_empty());
        assert_eq!(active_paint(engine.document()).raster.identity(), root);
    }

    #[test]
    fn completed_contact_estimates_expire_without_retaining_historical_input() {
        let (mut input, mut engine) = engine("expiry", 64, 64);
        let mut down = event(1, PenPhase::Down, 8.);
        down.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
        input.push(down).unwrap();
        input.push(event(2, PenPhase::Up, 12.)).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.completed_stroke.is_some());
        let root = active_paint(engine.document()).raster.identity();
        engine.completed_at = Some(
            web_time::Instant::now() - CORRECTION_WINDOW - std::time::Duration::from_millis(1),
        );
        down.flags = SampleFlags(SampleFlags::CORRECTION.0);
        down.pressure = 0.1;
        input.push(down).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.completed_stroke.is_none());
        assert!(engine.completed_before.is_none());
        assert!(engine.estimates.is_empty());
        assert_eq!(active_paint(engine.document()).raster.identity(), root);
    }

    #[test]
    fn estimated_samples_use_original_transforms_and_close_the_window_on_undo() {
        for feedback in [false, true] {
            let (mut input, mut engine) = engine("estimates", 64, 64);
            engine.settings.instant_feedback.enabled = feedback;
            let mut down = event(1, PenPhase::Down, 8.);
            down.pressure = 0.2;
            down.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
            input.push(down).unwrap();
            engine.render_frame().unwrap();
            // Evict the original camera from the ordinary input history. Its
            // captured transform must remain attached to the estimated point.
            for revision in 2..32 {
                engine.set_view(
                    view(64, 64),
                    ViewTransform {
                        revision,
                        surface_to_document: [1., 0., 0., 1., 500., 300.],
                    },
                );
            }
            engine.settings.pressure = layer_core::PressureResponse::try_from(vec![[0.,0.],[1./3.,0.],[2./3.,0.],[1.,1.]]).unwrap().into();
            let mut correction = down;
            correction.flags = SampleFlags(SampleFlags::CORRECTION.0 | SampleFlags::ESTIMATED.0);
            correction.surface_position.x = 12.;
            correction.pressure = 0.9;
            correction.tilt_radians = [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4];
            correction.twist_radians = 1.7;
            input.push(correction).unwrap();
            engine.render_frame().unwrap();
            let corrected = engine.builder.real_points()[0];
            assert_eq!(corrected.position, Point { x: 12., y: 16. });
            assert_eq!(corrected.pressure, 0.9);
            let axis = 2f32.sqrt().atan() / 2f32.sqrt();
            assert!((corrected.tilt[0] - axis).abs() < 1e-6);
            assert!((corrected.tilt[1] + axis).abs() < 1e-6);
            assert_eq!(corrected.twist, 1.7);
            let mut up = event(3, PenPhase::Up, -480.);
            up.surface_position.y = -284.;
            up.view_revision = 31;
            input.push(up).unwrap();
            engine.render_frame().unwrap();
            let original = engine
                .document()
                .target_raster(engine.document().active_target().unwrap())
                .unwrap()
                .identity();
            let saved = engine.checkpoint();
            correction.pressure = 0.7;
            correction.twist_radians = 2.1;
            correction.flags = SampleFlags::CORRECTION;
            input.push(correction).unwrap();
            engine.render_frame().unwrap();
            let stroke = engine.completed_stroke.as_ref().unwrap();
            assert_eq!(stroke.points.len(), 2);
            assert_eq!(stroke.points[0].pressure, 0.7);
            assert_eq!(stroke.points[0].twist, 2.1);
            let corrected = engine
                .document()
                .target_raster(stroke.target)
                .unwrap()
                .identity();
            assert_ne!(corrected, original);
            assert_ne!(engine.checkpoint(), saved);
            assert_eq!(engine.metrics.committed_strokes, 1);
            assert!(engine.estimates.is_empty());
            engine.undo().unwrap();
            engine.render_frame().unwrap();
            assert!(
                engine
                    .document()
                    .target_raster(engine.document().active_target().unwrap())
                    .unwrap()
                    .is_empty()
            );
            correction.pressure = 0.1;
            input.push(correction).unwrap();
            engine.render_frame().unwrap();
            engine.redo().unwrap();
            engine.render_frame().unwrap();
            assert_eq!(
                engine
                    .document()
                    .target_raster(engine.document().active_target().unwrap())
                    .unwrap()
                    .identity(),
                corrected
            );
            assert!(engine.completed_stroke.is_none());
            engine.undo().unwrap();
            assert!(
                !engine.undo().unwrap(),
                "sensor updates must not add undo steps"
            );
        }
    }

    #[test]
    fn repeated_terminal_estimates_correct_each_copy_without_adding_samples() {
        let (mut input, mut engine) = engine("duplicate-estimate", 64, 64);
        let mut down = event(1, PenPhase::Down, 8.);
        down.flags = SampleFlags::ESTIMATED;
        input.push(down).unwrap();
        engine.render_frame().unwrap();
        let mut up = down;
        up.phase = PenPhase::Up;
        up.pressure = 0.1;
        input.push(up).unwrap();
        engine.render_frame().unwrap();
        let mut correction = down;
        correction.flags = SampleFlags::CORRECTION;
        input.push(correction).unwrap();
        engine.render_frame().unwrap();
        let stroke = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(stroke.points.len(), 2);
        assert!(stroke.points.iter().all(|p| p.pressure == down.pressure));
        assert_eq!(engine.metrics.committed_strokes, 1);
        assert!(engine.estimates.is_empty());
    }

    #[test]
    fn estimated_stationary_airbrush_samples_and_cancellation_keep_contact_ownership() {
        let (mut input, mut engine) = engine("stationary", 64, 64);
        engine.settings.brush.path.continuous_rate_hz = 60.;
        let mut down = event(1, PenPhase::Down, 8.);
        down.flags = SampleFlags::ESTIMATED;
        input.push(down).unwrap();
        engine.render_frame_at(2_000_000).unwrap();
        engine.render_frame_at(3_000_000).unwrap();
        let mut correction = down;
        correction.pressure = 0.3;
        correction.flags = SampleFlags::CORRECTION;
        input.push(correction).unwrap();
        engine.render_frame_at(4_000_000).unwrap();
        assert!(engine.builder.real_points().len() >= 4);
        assert!(
            engine
                .builder
                .real_points()
                .iter()
                .all(|p| p.pressure == 0.3)
        );
        input.push(event(5, PenPhase::Cancel, 8.)).unwrap();
        engine.render_frame().unwrap();
        input.push(event(6, PenPhase::Down, 30.)).unwrap();
        input.push(correction).unwrap();
        engine.render_frame().unwrap();
        assert!(
            engine
                .builder
                .real_points()
                .iter()
                .all(|p| p.pressure == 0.8)
        );
        assert!(
            engine
                .document()
                .target_raster(engine.document().active_target().unwrap())
                .unwrap()
                .is_empty()
        );
        assert!(engine.estimates.is_empty());
    }

    #[test]
    fn rulers_project_real_prediction_and_replay_without_repainting_guide_edits() {
        use layer_core::{Ruler, RulerGeometry};
        for kind in [
            layer_core::RulerKind::Straight,
            layer_core::RulerKind::Parallel,
            layer_core::RulerKind::Radial,
        ] {
            let (mut producer, mut engine) = engine("ruler", 128, 128);
            engine
                .set_brush(default_brush(DefaultBrushPreset::GPen))
                .unwrap();
            engine.render_frame().unwrap();
            engine.backend.saw_reset = false;
            let guide = Ruler {
                id: layer_core::authored::PortableId::random(),
                geometry: RulerGeometry::from_drag(
                    kind,
                    Point { x: 0., y: 16. },
                    Point { x: 100., y: 16. },
                ),
            };
            engine.apply_edit(rulers_edit(engine.document(), vec![guide])).unwrap();
            assert!(!engine.has_pending_document_edits());
            engine.render_frame().unwrap();
            assert!(!engine.backend.saw_reset);
            engine.undo().unwrap();
            engine.render_frame().unwrap();
            assert!(!engine.backend.saw_reset);
            engine.redo().unwrap();
            engine.render_frame().unwrap();
            assert!(!engine.backend.saw_reset);
            producer.push(event(1, PenPhase::Down, 4.)).unwrap();
            engine.render_frame().unwrap();
            // A subsequent guide change does not redirect an active stroke.
            engine
                .apply_edit(rulers_edit(engine.document(), vec![Ruler {
                    id: layer_core::authored::PortableId::random(),
                    geometry: RulerGeometry::Parallel {
                        start: Point { x: 0., y: 0. },
                        end: Point { x: 0., y: 100. },
                    },
                }]))
                .unwrap();
            let mut predicted = event(3, PenPhase::Move, 50.);
            predicted.flags = SampleFlags::PREDICTED;
            predicted.surface_position.y = 45.;
            producer.push(predicted).unwrap();
            let mut real = event(2, PenPhase::Move, 28.);
            real.surface_position.y = 30.;
            producer.push(real).unwrap();
            engine.render_frame().unwrap();
            assert!(
                engine
                    .builder
                    .real_points()
                    .iter()
                    .chain(engine.builder.predicted_points())
                    .all(|p| (p.position.y - 16.).abs() < 0.001)
            );
            assert!(
                engine
                    .backend
                    .preview
                    .iter()
                    .all(|d| (d.center.y - 16.).abs() < 0.001)
            );
            let mut up = event(4, PenPhase::Up, 60.);
            up.surface_position.y = 50.;
            producer.push(up).unwrap();
            engine.render_frame().unwrap();
            let stroke = engine.completed_stroke.as_ref().unwrap().clone();
            assert!(
                stroke
                    .points
                    .iter()
                    .all(|p| (p.position.y - 16.).abs() < 0.001)
            );
            assert_eq!(
                stroke.points.len(),
                3,
                "predicted samples are not document truth"
            );
            assert_eq!(stroke.points[1].pressure, 0.8);
            let pigment = engine.backend.persistent.clone();
            engine.rebuild_all = true;
            engine.render_frame().unwrap();
            assert_eq!(
                engine.backend.persistent, pigment,
                "replay ignores changed rulers"
            );
            engine.set_ruler_snapping(None);
            producer.push(event(5, PenPhase::Down, 3.)).unwrap();
            let mut free = event(6, PenPhase::Up, 30.);
            free.surface_position.y = 35.;
            producer.push(free).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(
                engine
                    .completed_stroke
                    .as_ref()
                    .unwrap()
                    .points
                    .last()
                    .unwrap()
                    .position
                    .y,
                35.
            );
        }
    }

    #[test]
    fn a_hovering_bristle_fan_turns_with_a_measured_barrel() {
        let (_producer, mut engine) = engine("hover-twist", 256, 256);
        engine.set_brush(default_brush(DefaultBrushPreset::BristlePaintbrush)).unwrap();
        let facing = |twist: f32, flags: SampleFlags| {
            let mut hover = DabGenerator::default();
            let mut sample = event(1, PenPhase::Hover, 128.);
            sample.tilt_radians = [0.3, 0.];
            sample.twist_radians = twist;
            sample.flags = flags;
            let outline = engine.cursor_contacts(sample, &mut hover, 0);
            outline[0].rotation[1].atan2(outline[0].rotation[0])
        };
        let measured = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::BARREL_TWIST.0);
        let turned = facing(1.1, measured) - facing(0.6, measured);
        assert!((turned - 0.5).abs() < 0.1, "the outline follows the barrel: {turned}");
        assert_eq!(facing(1.1, SampleFlags::PRIMARY), facing(0.6, SampleFlags::PRIMARY), "pens without a sensor face their lean");
    }

    #[test]
    fn ruler_input_and_cursor_respect_view_layer_offsets_and_radial_origin() {
        use layer_core::{Ruler, RulerGeometry};
        let (mut producer, mut engine) = engine_with(
            RecordingRenderer::default(),
            Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            view(256, 256),
            ViewTransform {
                revision: 1,
                surface_to_document: [0., 0.5, -0.5, 0., 64., 0.],
            },
        );
        engine
            .set_brush(default_brush(DefaultBrushPreset::GPen))
            .unwrap();
        let mut layer = active_occurrence(engine.document()).clone();
        layer.offset = [3, 5];
        engine
            .apply_edit(replace_occurrence(engine.document(), layer))
            .unwrap();
        engine
            .apply_edit(rulers_edit(engine.document(), vec![Ruler {
                id: layer_core::authored::PortableId::random(),
                geometry: RulerGeometry::Straight {
                    start: Point { x: 0., y: 32. },
                    end: Point { x: 100., y: 32. },
                },
            }]))
            .unwrap();
        let sample = |sequence, phase, p: [f32; 2]| {
            let mut e = event(sequence, phase, 0.);
            e.surface_position = Point {
                x: 2. * p[1],
                y: 2. * (64. - p[0]),
            };
            e
        };
        let mut hover = DabGenerator::default();
        let down = sample(1, PenPhase::Down, [10., 34.]);
        assert!(
            engine
                .cursor_contacts(down, &mut hover, 0)
                .iter()
                .all(|d| (d.center.y - 32.).abs() < 0.001)
        );
        producer.push(down).unwrap();
        engine.render_frame().unwrap();
        let next = sample(2, PenPhase::Move, [40., 45.]);
        let contacts = engine.cursor_contacts(next, &mut hover, 0);
        assert!(!contacts.is_empty());
        assert!(
            contacts.iter().all(|d| (d.center.y - 32.).abs() < 0.001),
            "outline is document-local, not layer-local"
        );
        producer.push(next).unwrap();
        producer.push(sample(3, PenPhase::Up, [60., 45.])).unwrap();
        engine.render_frame().unwrap();
        assert!(
            engine
                .completed_stroke
                .as_ref()
                .unwrap()
                .points
                .iter()
                .all(|p| (p.position.y - 27.).abs() < 0.001)
        );
        engine
            .apply_edit(rulers_edit(engine.document(), vec![Ruler {
                id: layer_core::authored::PortableId::random(),
                geometry: RulerGeometry::Radial {
                    center: Point { x: 16., y: 32. },
                },
            }]))
            .unwrap();
        producer
            .push(sample(4, PenPhase::Down, [16., 32.]))
            .unwrap();
        engine.render_frame().unwrap();
        assert!(
            engine
                .active_ruler_constraint()
                .unwrap()
                .direction
                .is_none()
        );
        let mut prediction = sample(6, PenPhase::Move, [50., 60.]);
        prediction.flags = SampleFlags::PREDICTED;
        producer.push(prediction).unwrap();
        engine.render_frame().unwrap();
        assert!(
            engine
                .active_ruler_constraint()
                .unwrap()
                .direction
                .is_none(),
            "prediction cannot fix the ray"
        );
        producer
            .push(sample(5, PenPhase::Move, [40., 32.]))
            .unwrap();
        engine.render_frame().unwrap();
        assert_eq!(
            engine.active_ruler_constraint().unwrap().direction,
            Some(Point { x: 1., y: 0. })
        );
        producer.push(sample(7, PenPhase::Up, [60., 45.])).unwrap();
        engine.render_frame().unwrap();
        assert!(
            engine
                .completed_stroke
                .as_ref()
                .unwrap()
                .points
                .iter()
                .all(|p| (p.position.y - 27.).abs() < 0.001)
        );
    }

    #[test]
    fn persistent_batches_are_incremental_and_preserve_stroke_boundaries() {
        let (mut producer, mut engine) = engine("incremental-batches", 64, 64);
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: false,
                ..InstantFeedbackConfig::default()
            })
            .unwrap();

        producer.push(event(1, PenPhase::Down, 8.0)).unwrap();
        engine.render_frame().unwrap();
        producer.push(event(2, PenPhase::Up, 32.0)).unwrap();
        engine.render_frame().unwrap();
        producer.push(event(3, PenPhase::Down, 12.0)).unwrap();
        producer.push(event(4, PenPhase::Up, 40.0)).unwrap();
        engine.render_frame().unwrap();

        let batches = &engine.backend().persistent_batches;
        assert_eq!(batches.len(), 3);
        assert_eq!((batches[0].1, batches[0].2), (true, false));
        assert_eq!((batches[1].1, batches[1].2), (false, true));
        assert_eq!(batches[0].0, batches[1].0);
        assert_ne!(batches[1].0, batches[2].0);
        assert_eq!((batches[2].1, batches[2].2), (true, true));
        assert!(batches.iter().all(|batch| batch.3 > 0));
    }

    #[test]
    fn brush_changes_apply_to_the_next_stroke_only() {
        let (mut producer, mut engine) = engine("brush-snapshot", 64, 64);
        producer.push(event(1, PenPhase::Down, 8.0)).unwrap();
        engine.render_frame().unwrap();

        let next_brush = BrushSnapshot {
            diameter: 40.0,
            stabilization: layer_core::BrushStabilization { strength: 1., ..Default::default() },
            ..BrushSnapshot::default()
        };
        engine.set_brush(next_brush).unwrap();
        producer.push(event(2, PenPhase::Up, 24.0)).unwrap();
        engine.render_frame().unwrap();

        let stroke = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(stroke.brush.diameter, BrushSnapshot::default().diameter);
        assert_eq!(stroke.brush.stabilization, BrushSnapshot::default().stabilization);
    }

    #[test]
    fn vector_strokes_smooth_weigh_and_predict_input_like_raster_strokes() {
        for tool in [ToolKind::Pen, ToolKind::Mouse] {
            let (mut input, mut engine) = engine("vector smoothing", 1024, 512);
            let mut brush = default_brush(DefaultBrushPreset::GPen);
            brush.stabilization.strength = 0.5;
            engine.set_brush(brush).unwrap();
            engine.set_view(ViewState { document_to_surface: [2., 0., 0., 2., 0., 0.], ..view(1024, 512) },
                ViewTransform { revision: 2, surface_to_document: [0.5, 0., 0., 0.5, 0., 0.] });
            let events: Vec<PenEvent> = (0..120u64).map(|i| {
                let mut sample = event(i + 1, if i == 0 { PenPhase::Down } else if i == 119 { PenPhase::Up } else { PenPhase::Move }, 100. + i as f32 * 3.);
                sample.view_revision = 2;
                sample.tool = tool;
                sample.timestamp_ns = 1_000_000_000 + i * 4_000_000;
                sample.surface_position.y = 200. + (i as f32 * 0.05).sin() * 60.;
                sample.pressure = 0.9 - 0.7 * (i as f32 / 119.);
                sample
            }).collect();
            let centers = |samples: Vec<crate::VectorSample>| samples.into_iter().map(|(p, r)| [p[0] as f32, p[1] as f32, r as f32]).collect::<Vec<_>>();
            let same = |drawn: &[[f32; 3]], expected: &[[f32; 3]], label: String| {
                assert_eq!(drawn.len(), expected.len(), "{label}");
                for (a, b) in drawn.iter().zip(expected) { assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3), "{label}: {a:?} vs {b:?}"); }
            };
            let mut vector = engine.begin_vector_stroke(1, events[0], [0.; 2], 0.5);
            let mut predicted_frames = 0;
            for (i, sample) in events.iter().enumerate() {
                let (now, presented) = (sample.timestamp_ns, sample.timestamp_ns + 8_000_000);
                input.push(*sample).unwrap();
                if i > 0 { engine.push_vector_stroke(&mut vector, *sample); }
                if sample.phase == PenPhase::Move && i % 2 == 0 {
                    let mut predicted = *sample;
                    predicted.sequence += 1_000;
                    predicted.timestamp_ns = presented;
                    predicted.surface_position.x += 6.;
                    predicted.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0);
                    input.push(predicted).unwrap();
                    engine.push_vector_stroke(&mut vector, predicted);
                }
                engine.render_frame_for(now, presented).unwrap();
                if sample.phase == PenPhase::Up { continue; }
                let backend = engine.backend();
                predicted_frames += usize::from(!backend.preview.is_empty() && i % 2 == 0 && tool == ToolKind::Pen);
                same(&centers(vector.preview(Some(now), Some(presented))),
                    &centers(crate::vector_stroke::samples(backend.persistent.iter().chain(&backend.preview))), format!("{tool:?} frame {i}"));
            }
            assert!(tool == ToolKind::Mouse || predicted_frames > 20 && engine.metrics().platform_prediction_frames > 20, "{tool:?}");
            if engine.pending_up.is_some() {
                let now = 1_000_000_000 + 119 * 4_000_000 + u64::from(pen_up_settlement_micros(&engine.settings.brush.stabilization)) * 1000;
                engine.render_frame_for(now, now + 8_000_000).unwrap();
            }
            let stroke = engine.completed_stroke.as_ref().unwrap();
            assert_eq!(vector.brush(), &stroke.brush, "{tool:?}");
            let mut raster = Vec::new();
            DabGenerator::generate(stroke, layer_core::color::RgbSpace::Srgb, &mut raster);
            let expected: Vec<_> = raster.iter().map(|dab| [dab.center.x, dab.center.y, dab.radii[0]]).collect();
            same(&centers(vector.samples()), &expected, format!("{tool:?} commit"));
        }
    }

    #[test]
    fn smoothing_committed_ink_matches_replay_across_delivery_cadences() {

            for feedback in [false, true] {
                let mut reference = None;
                for cadence in [1, 7, 256] {
                    let (mut input, mut engine) = engine("smoothing replay", 1024, 512);
                    let mut brush = default_brush(DefaultBrushPreset::GPen);
                    brush.diameter = 8.;
                    brush.stabilization.strength = 0.5;

                    engine.set_brush(brush).unwrap();
                    engine.set_instant_feedback(InstantFeedbackConfig { enabled: feedback, use_platform_prediction: false, ..Default::default() }).unwrap();
                    let mut prefix = Vec::new();
                    for i in 0..180 {
                        let mut sample = event(i + 1, if i == 0 { PenPhase::Down } else if i == 179 { PenPhase::Up } else { PenPhase::Move }, 100. + i as f32 * 2.);
                        sample.timestamp_ns = 1_000_000_000 + i * 4_000_000;
                        sample.surface_position.y = 100. + (i as f32 * 0.04).sin() * 30.;
                        input.push(sample).unwrap();
                        if i % cadence == 0 || i == 179 {
                            engine.render_frame_for(sample.timestamp_ns, sample.timestamp_ns + 8_000_000).unwrap();
                            assert!(engine.backend.persistent.starts_with(&prefix), "Curve fit: committed prefix is immutable");
                            prefix = engine.backend.persistent.clone();
                        }
                    }
                    if engine.pending_up.is_some() {
                        let now = 1_000_000_000 + 179 * 4_000_000 + u64::from(pen_up_settlement_micros(&engine.settings.brush.stabilization)) * 1000;
                        engine.render_frame_for(now, now + 8_000_000).unwrap();
                    }
                    let stroke = engine.completed_stroke.as_ref().unwrap();
                    assert_eq!(stroke.points.len(), 180);
                    assert_eq!(stroke.points.last().unwrap().position.x, 458.);
                    let mut restored = stroke.clone();
                    restored.brush = serde_json::from_slice(&serde_json::to_vec(&stroke.brush).unwrap()).unwrap();
                    assert_eq!(restored, *stroke);
                    let mut replay = Vec::new();
                    DabGenerator::generate(&restored, engine.document().composition().color.space, &mut replay);
                    assert_eq!(engine.backend.persistent, replay, "Curve fit, feedback={feedback}, cadence={cadence}");
                    assert!(engine.backend.preview.is_empty());
                    if let Some(reference) = &reference { assert_eq!(reference, &replay); } else { reference = Some(replay); }
                }
            }

    }

    #[test]
    fn recorded_curve_preview_has_no_hook_and_pen_up_matches_replay() {
        use crate::brush::PositionField;
        use crate::recording::{Event, Record};
        let records = crate::recording::read(include_bytes!("../tests/data/wacom-pro-27.capystrokes").as_slice()).unwrap();
        let begin = records.iter().position(|r| matches!(r, Record::Begin { id: 49, .. })).unwrap();
        let first = records[..begin].iter().rposition(|r| matches!(r, Record::Raw { event, .. } if event.phase == PenPhase::Down)).unwrap();
        let (mut input, mut engine) = engine("recorded curve tail", 8192, 8192);
        let mut brush = default_brush(DefaultBrushPreset::GPen);
        brush.diameter = 8.;
        brush.stabilization.strength = 1.;
        engine.set_brush(brush).unwrap();
        let mut start = 0;
        let mut last_time = 0;
        let mut policy = crate::recording::Policy { config: InstantFeedbackConfig::default(), transform: [1.,0.,0.,1.,0.,0.] };
        let mut checked = false;
        for record in &records[first..] {
            match record {
                Record::Raw { event, transform, pressure, .. } => {
                    engine.set_view(ViewState { width_px: 8192, height_px: 8192, document_to_surface: policy.transform }, *transform);
                    engine.set_pressure_curve(pressure.clone().into());
                    last_time = event.timestamp_ns;
                    input.push(*event).unwrap();
                }
                Record::Begin { policy: recorded, .. } | Record::Predictor(Event::Policy(recorded)) => {
                    if let Record::Begin { timestamp_ns, .. } = record { start = *timestamp_ns; }
                    policy = *recorded;
                    policy.config.enabled = true;
                    policy.config.use_platform_prediction = false;
                    policy.config.prediction_horizon_micros = 8000;
                    engine.set_instant_feedback(policy.config).unwrap();
                }
                Record::Predictor(Event::Query(query, now, _)) => {
                    let clock = start + u64::from(*now) * 1000;
                    engine.render_frame_for(clock, clock + 8_000_000).unwrap();
                    if *query == 427 {
                        let latest = *engine.builder.real_points().last().unwrap();
                        let preview = &engine.active_stroke.as_ref().unwrap().tip.curve_preview;
                        let end = preview.endpoint(latest).unwrap().elapsed_micros;
                        let y = |time| { let p = preview.position(time); policy.transform[1] * p.x + policy.transform[3] * p.y };
                        let mut previous = y(f64::from(latest.elapsed_micros) * 1e-6);
                        let mut reversal = 0.;
                        for i in 1..=256 {
                            let time = (f64::from(latest.elapsed_micros) + f64::from(end - latest.elapsed_micros) * i as f64 / 256.) * 1e-6;
                            let next = y(time);
                            reversal += (previous - next).max(0.);
                            previous = next;
                        }
                        assert!(reversal < 0.2, "recorded preview hook: {reversal} physical pixels");
                        checked = true;
                    }
                }
                Record::End { cancelled, interrupted } => {
                    assert!(!cancelled && !interrupted);
                    engine.render_frame_for(last_time, last_time + 8_000_000).unwrap();
                    let clock = last_time + 100_000_000;
                    engine.render_frame_for(clock, clock + 8_000_000).unwrap();
                    break;
                }
                _ => (),
            }
        }
        assert!(checked);
        assert!(engine.pending_up.is_none());
        let stroke = engine.completed_stroke.as_ref().unwrap();
        let endpoint = stroke.points.last().unwrap().position;
        let rendered = engine.backend.persistent.last().unwrap().center;
        assert!((rendered.x - endpoint.x).hypot(rendered.y - endpoint.y) < 0.001);
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
        assert_eq!(engine.backend.persistent, replay);
        assert!(engine.backend.preview.is_empty());
    }

    #[test]
    fn smoothing_strength_reduces_preview_reach_on_the_fitted_curve() {
        use crate::brush::PositionField;
        let mut offsets = Vec::new();
        for strength in [0.06, 0.5, 1.] {
            let (mut input, mut engine) = engine("initial preview tracking", 2048, 512);
            let mut brush = default_brush(DefaultBrushPreset::GPen);
            brush.diameter = 8.;

            brush.stabilization.strength = strength;
            engine.set_brush(brush).unwrap();
            engine.set_instant_feedback(InstantFeedbackConfig { use_platform_prediction: false, prediction_horizon_micros: 8000, ..Default::default() }).unwrap();
            let mut tips = Vec::new();
            for i in 0..100 {
                let mut sample = event(i + 1, if i == 0 { PenPhase::Down } else { PenPhase::Move }, 100. + i as f32 * 2.);
                sample.timestamp_ns = 1_000_000_000 + i * 4_000_000;
                input.push(sample).unwrap();
                engine.render_frame_for(sample.timestamp_ns, sample.timestamp_ns + 8_000_000).unwrap();
                if i > 30 {
                    let active = engine.active_stroke.as_ref().unwrap();
                    let source = active.tip.curve_preview.endpoint(*engine.builder.real_points().last().unwrap()).unwrap().elapsed_micros;
                    let tip = active.tip.curve_preview.position(f64::from(source) * 1e-6);
                    assert!(tip.x <= sample.surface_position.x + 4.01, "strength={strength}, tip={tip:?}, pen={:?}", sample.surface_position);
                    tips.push(tip);
                }
            }
            offsets.push(tips.last().unwrap().x - 298.);
        }
        assert!(offsets[0] > 0. && offsets[1] < 0. && offsets[2] < offsets[1], "{offsets:?}");
    }

    #[test]
    fn smoothed_preview_of_a_spaced_brush_follows_the_pen() {
        let (mut input, mut engine) = engine("spaced smoothing preview", 1600, 1000);
        let mut brush = default_brush(DefaultBrushPreset::LiquifyPinch);
        brush.diameter = 300.;
        brush.stabilization.strength = 0.06;
        engine.set_brush(brush).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { use_platform_prediction: false, ..Default::default() }).unwrap();
        for i in 0..120 {
            let mut sample = event(i + 1, if i == 0 { PenPhase::Down } else { PenPhase::Move }, 480. + i as f32 * 1.78);
            sample.timestamp_ns = 1_000_000_000 + i * 4_166_667;
            input.push(sample).unwrap();
            engine.render_frame_for(sample.timestamp_ns, sample.timestamp_ns + 8_333_333).unwrap();
            let tip = engine.backend.preview.last().map(|dab| dab.center.x);
            assert!(i < 30 || tip.is_some_and(|x| (x - sample.surface_position.x).abs() < 15.), "sample {i}: preview tip {tip:?}, pen {}", sample.surface_position.x);
        }
    }

    #[test]
    fn disabling_prediction_publishes_curve_fit_without_a_smoothing_wait() {

            for strength in [0.06, 0.3, 1.] {
                let (mut input, mut engine) = engine("no prediction wait", 1024, 512);
                let mut brush = default_brush(DefaultBrushPreset::GPen);

                brush.stabilization.strength = strength;
                engine.set_brush(brush).unwrap();
                engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
                for i in 0..50 {
                    let mut sample = event(i + 1, if i == 0 { PenPhase::Down } else if i == 49 { PenPhase::Up } else { PenPhase::Move }, 100. + i as f32 * 4.);
                    sample.timestamp_ns = 1_000_000_000 + i * 4_000_000;
                    input.push(sample).unwrap();
                    engine.render_frame_for(sample.timestamp_ns, sample.timestamp_ns + 8_000_000).unwrap();
                    assert!(engine.pending_up.is_none());
                    if let Some(active) = engine.active_stroke.as_ref() {
                        assert!(!active.feedback.enabled);
                        assert!(!active.tip.curve_preview.is_present());
                        assert!(engine.dab_generator.modeled_position().is_some());
                    }
                }
                assert_eq!(engine.metrics.committed_strokes, 1);
                assert!(engine.backend.preview.is_empty());
                let mut replay = Vec::new();
                DabGenerator::generate(engine.completed_stroke.as_ref().unwrap(), engine.document().composition().color.space, &mut replay);
                assert_eq!(engine.backend.persistent, replay);
            }

    }

    fn settling_stroke() -> (InputProducer<PenEvent>, CanvasEngine<RecordingRenderer>, PenEvent) {
        let (mut input, mut engine) = engine("settling", 2048, 512);
        let mut brush = default_brush(DefaultBrushPreset::GPen);
        brush.diameter = 8.;
        brush.stabilization.strength = 1.;

        engine.set_brush(brush).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { use_platform_prediction: false, ..Default::default() }).unwrap();
        let mut last = event(1, PenPhase::Down, 100.);
        for index in 0..200 {
            last = PenEvent { sequence: index + 1, timestamp_ns: 1_000_000_000 + index * 4_000_000,
                phase: if index == 0 { PenPhase::Down } else { PenPhase::Move },
                surface_position: Point { x: 100. + index as f32 * 4., y: 100. }, ..last };
            input.push(last).unwrap();
            engine.render_frame_for(last.timestamp_ns, last.timestamp_ns + 8_000_000).unwrap();
        }
        let committed = engine.backend.persistent.clone();
        last.phase = PenPhase::Up;
        last.surface_position.y += 30.;
        last.flags = SampleFlags::ESTIMATED;
        last.sequence += 1;
        last.timestamp_ns += 1_000_000;
        input.push(last).unwrap();
        engine.render_frame_for(last.timestamp_ns, last.timestamp_ns + 8_000_000).unwrap();
        assert!(engine.pending_up.is_some());
        assert_eq!(engine.backend.persistent, committed);
        (input, engine, last)
    }

    #[test]
    fn pen_up_lands_on_final_ink_before_publishing_its_raster_boundary() {
        let (_, mut engine, up) = settling_stroke();
        let final_point = engine.builder.real_points().last().unwrap().position;
        let mut distances = Vec::new();
        for milliseconds in [0, 8, 16, 24, 31] {
            engine.render_frame_for(up.timestamp_ns + milliseconds * 1_000_000, up.timestamp_ns + milliseconds * 1_000_000).unwrap();
            let latest = *engine.builder.real_points().last().unwrap();
            let point = engine.active_stroke.as_ref().unwrap().tip.curve_preview.endpoint(latest).map_or(final_point, |point| point.position);
            distances.push((point.x - final_point.x).hypot(point.y - final_point.y));
            assert_eq!(engine.metrics.committed_strokes, 0);
            assert!(!engine.backend.preview.is_empty());
            assert_eq!(engine.builder.real_points().len(), 201);
        }
        assert!(distances[0] > 0.001);
        assert!(distances[3] < distances[2] && distances[4] < distances[3], "late landing: {distances:?}");
        assert!(distances[3] - distances[4] < distances[2] - distances[3], "late corrections shrink: {distances:?}");
        assert!(distances[4] < distances[0] * 0.02);
        let landing_radii = engine.backend.preview.last().unwrap().radii;
        engine.render_frame_for(up.timestamp_ns + 32_000_000, up.timestamp_ns + 32_000_000).unwrap();
        assert!(!engine.has_active_stroke());
        assert!(engine.backend.preview.is_empty());
        assert_eq!(engine.metrics.committed_strokes, 1);
        let final_radii = engine.backend.persistent.last().unwrap().radii;
        for axis in 0..2 { assert!((landing_radii[axis] / final_radii[axis] - 1.).abs() < 0.001, "publication width seam: {landing_radii:?} -> {final_radii:?}"); }
        let stroke = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(stroke.points.len(), 201);
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
        assert_eq!(engine.backend.persistent, replay);
    }

    #[test]
    fn following_contact_does_not_wait_for_the_pen_up_settling_deadline() {
        let (mut input, mut engine, up) = settling_stroke();
        let mut down = up;
        down.phase = PenPhase::Down;
        down.sequence += 1;
        down.timestamp_ns += 2_000_000;
        down.surface_position.y += 20.;
        input.push(down).unwrap();
        engine.render_frame_for(down.timestamp_ns, down.timestamp_ns + 8_000_000).unwrap();
        assert_eq!(engine.metrics.committed_strokes, 1);
        assert!(engine.has_pending_input());
        assert!(engine.pending_up.is_none());
        engine.render_frame_for(down.timestamp_ns, down.timestamp_ns + 8_000_000).unwrap();
        assert!(engine.has_active_stroke());
        assert_eq!(engine.builder.real_points().len(), 1);
        assert_eq!(engine.builder.real_points()[0].position, down.surface_position);
        assert!(!engine.backend.preview.is_empty());
        assert!(engine.metrics.input_events == 202);
    }

    #[test]
    fn untimed_frames_publish_a_settling_stroke_when_capture_capacity_returns() {
        let (_, mut engine, _) = settling_stroke();
        engine.backend_mut().capture_blocked = true;
        engine.render_frame().unwrap();
        assert!(engine.pending_up.is_some());
        assert_eq!(engine.metrics.committed_strokes, 0);
        engine.backend_mut().capture_blocked = false;
        engine.render_frame().unwrap();
        assert!(engine.pending_up.is_none());
        assert_eq!(engine.metrics.committed_strokes, 1);
        assert_eq!(engine.completed_stroke.as_ref().unwrap().points.len(), 201);
    }

    #[test]
    fn only_changed_sensor_corrections_restart_pen_up_settlement() {
        for correction_kind in 0..3 {
            let (mut input, mut engine, up) = settling_stroke();
            let original_ready = engine.pending_up.unwrap().1;
            let mut correction = up;
            correction.flags = SampleFlags::CORRECTION;
            if correction_kind == 0 { correction.sequence += 100; }
            if correction_kind == 2 { correction.surface_position.y += 30.; }
            input.push(correction).unwrap();
            engine.render_frame_at(up.timestamp_ns + 20_000_000).unwrap();
            assert_eq!(engine.builder.real_points().len(), 201);
            assert_eq!(engine.pending_up.unwrap().1, original_ready + if correction_kind == 2 { 20_000 } else { 0 });
            engine.render_frame_at(up.timestamp_ns + 32_000_000).unwrap();
            if correction_kind == 2 {
                assert_eq!(engine.metrics.committed_strokes, 0);
                engine.render_frame_at(up.timestamp_ns + 52_000_000).unwrap();
            }
            assert_eq!(engine.metrics.committed_strokes, 1);
            let stroke = engine.completed_stroke.as_ref().unwrap();
            let mut replay = Vec::new();
            DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
            assert_eq!(engine.backend.persistent, replay);
        }
    }

    #[test]
    fn manual_prediction_amount_reaches_beyond_the_cursor_with_display_timing() {
        let mut leads = Vec::new();
        for horizon in [0, 8_000, 16_000, 64_000] {
            let (mut input, mut engine) = engine("manual prediction", 1024, 128);
            engine
                .set_instant_feedback(InstantFeedbackConfig {
                    use_platform_prediction: false,
                    prediction_horizon_micros: horizon,
                    ..Default::default()
                })
                .unwrap();
            engine
                .set_brush(BrushSnapshot {
                    diameter: 4.,
                    stabilization: layer_core::BrushStabilization { strength: 0., ..Default::default() },
                    ..Default::default()
                })
                .unwrap();
            let mut last = event(1, PenPhase::Down, 100.);
            // Constant 400 px/s motion with steady pressure, long enough for
            // the lead filter to settle. The host supplies a display target.
            for index in 0..101 {
                last = PenEvent {
                    timestamp_ns: 1_000_000_000 + index * 10_000_000,
                    surface_position: Point {
                        x: 100. + index as f32 * 4.,
                        y: 16.,
                    },
                    phase: if index == 0 {
                        PenPhase::Down
                    } else {
                        PenPhase::Move
                    },
                    sequence: index + 1,
                    ..last
                };
                input.push(last).unwrap();
                engine
                    .render_frame_for(last.timestamp_ns, last.timestamp_ns + 8_000_000)
                    .unwrap();
            }
            let tip = engine.backend().preview.last().unwrap().center.x;
            let lead = tip - last.surface_position.x;
            eprintln!(
                "manual={horizon}us cursor={} predicted_tip={tip} lead={lead}px engine_frames={}",
                last.surface_position.x,
                engine.metrics().engine_prediction_frames
            );
            leads.push(lead);
            let mut up = last;
            up.phase = PenPhase::Up;
            up.timestamp_ns += 1_000_000;
            input.push(up).unwrap();
            engine
                .render_frame_for(up.timestamp_ns, up.timestamp_ns + 8_000_000)
                .unwrap();
            let stroke = engine.completed_stroke.as_ref().unwrap();
            assert_eq!(
                stroke.points.last().unwrap().position,
                last.surface_position
            );
            let mut replay = Vec::new();
            DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
            assert_eq!(engine.backend().persistent, replay);
            assert!(engine.backend().preview.is_empty());
        }
        assert!(leads[0].abs() < 0.05);
        assert!(leads.windows(2).all(|pair| pair[1] > pair[0] + 1.));
        for (lead, maximum) in leads.into_iter().zip([0., 3.2, 6.4, 25.6]) {
            assert!(lead <= maximum + 0.25, "prediction amount is an upper limit");
        }
    }

    #[test]
    fn preview_renders_the_prediction_curve_and_expires_without_new_input() {
        let (mut input, mut engine) = engine("trajectory preview", 256, 256);
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                use_platform_prediction: false,
                prediction_horizon_micros: 16_000,
                ..Default::default()
            })
            .unwrap();
        engine
            .set_brush(BrushSnapshot {
                diameter: 2.,
                spacing: 0.1,
                stabilization: layer_core::BrushStabilization { strength: 0., ..Default::default() },
                ..Default::default()
            })
            .unwrap();
        let position = |t: f32| Point {
            x: 100. + 30. * (40. * t).cos(),
            y: 100. + 30. * (40. * t).sin(),
        };
        let mut last = event(1, PenPhase::Down, 130.);
        for i in 0..100 {
            last = PenEvent {
                timestamp_ns: 1_000_000_000 + i * 4_000_000,
                surface_position: position(i as f32 * 0.004),
                phase: if i == 0 {
                    PenPhase::Down
                } else {
                    PenPhase::Move
                },
                sequence: i + 1,
                ..last
            };
            input.push(last).unwrap();
            engine
                .render_frame_for(last.timestamp_ns, last.timestamp_ns + 8_000_000)
                .unwrap();
        }
        assert!(engine.metrics().engine_prediction_frames > 0);
        let preview = &engine.backend().preview;
        assert!(!preview.is_empty());
        // Follow the predictor's complete accepted curve, including adaptive
        // horizon and fitted-anchor correction, instead of assuming a perfect
        // circle at a fixed 16 ms lookahead from the retired model.
        let now = engine.builder.elapsed_micros_at(last.timestamp_ns).unwrap();
        let active = engine.active_stroke.as_mut().unwrap();
        let tip = active.tip.prediction.estimate_for(engine.builder.real_points(), &[],
            now + 16_000, now, engine.view.document_to_surface, active.feedback).unwrap();
        for point in active.tip.prediction.engine_intermediates().chain([tip.point]) {
            assert!(dabs_cover_point(&engine.backend.preview, point.position));
        }
        let rendered = engine.backend.preview.last().unwrap().center;
        assert!((rendered.x - tip.point.position.x).hypot(rendered.y - tip.point.position.y) < 0.3);
        engine
            .render_frame_for(
                last.timestamp_ns + 500_000_000,
                last.timestamp_ns + 508_000_000,
            )
            .unwrap();
        let tip = engine.backend().preview.last().unwrap().center;
        assert!((tip.x - last.surface_position.x).hypot(tip.y - last.surface_position.y) < 0.3);
    }

    #[test]
    fn prediction_taper_changes_only_predicted_width_and_preserves_swept_joins() {
        let render = |prediction, contact| {
            let (mut input, mut engine) = engine("taper", 256, 128);
            engine
                .set_instant_feedback(InstantFeedbackConfig {
                    use_platform_prediction: false,
                    prediction_horizon_micros: if prediction { 16_000 } else { 0 },
                    ..Default::default()
                })
                .unwrap();
            let mut brush = if contact {
                layer_core::default_brush(layer_core::DefaultBrushPreset::GPen)
            } else {
                BrushSnapshot::default()
            };
            brush.diameter = 10.;
            brush.stabilization.strength = 0.;
            engine.set_brush(brush).unwrap();
            let mut last = event(1, PenPhase::Down, 20.);
            for i in 0..41 {
                last = PenEvent {
                    timestamp_ns: 1_000_000_000 + i * 4_000_000,
                    surface_position: Point {
                        x: 20. + 4. * i as f32,
                        y: 40.,
                    },
                    phase: if i == 0 {
                        PenPhase::Down
                    } else {
                        PenPhase::Move
                    },
                    sequence: i + 1,
                    ..last
                };
                input.push(last).unwrap();
                engine
                    .render_frame_for(last.timestamp_ns, last.timestamp_ns + 8_000_000)
                    .unwrap();
            }
            let now = engine.builder.elapsed_micros_at(last.timestamp_ns).unwrap();
            let active = engine.active_stroke.as_mut().unwrap();
            let expected = active.tip.prediction.estimate_for(engine.builder.real_points(), &[], now + 16_000,
                now, engine.view.document_to_surface, active.feedback).unwrap().point.position;
            let preview = engine.backend().preview.clone();
            last.phase = PenPhase::Up;
            last.timestamp_ns += 1_000_000;
            input.push(last).unwrap();
            engine
                .render_frame_for(last.timestamp_ns, last.timestamp_ns + 8_000_000)
                .unwrap();
            (preview, engine.backend().persistent.clone(), expected)
        };
        for contact in [false, true] {
            let (plain, plain_ink, _) = render(false, contact);
            let (tapered, tapered_ink, expected) = render(true, contact);
            assert_eq!(plain_ink, tapered_ink);
            let tip = tapered.last().unwrap();
            assert!(surface_distance(tip.center, expected, [1., 0., 0., 1., 0., 0.]) < 0.01);
            for axis in 0..2 {
                assert!((tip.radii[axis] / plain.last().unwrap().radii[axis] - 0.75).abs() < 0.001);
            }
            for dab in &tapered {
                if let Some(original) = plain.iter().find(|p| p.center == dab.center) {
                    assert_eq!(original.contact, dab.contact);
                    assert_eq!(original.color_rgba_linear, dab.color_rgba_linear);
                    if dab.center.x <= 180. {
                        assert_eq!(
                            original.radii, dab.radii,
                            "measured tail keeps normal width"
                        );
                    }
                }
            }
            if contact {
                for pair in tapered.windows(2) {
                    assert_eq!(&pair[1].previous[..2], &pair[0].radii, "swept join width");
                }
            }
        }
    }

    #[test]
    fn feedback_tail_reaches_platform_prediction_without_committing_it() {
        let (mut producer, mut engine) = engine("feedback", 128, 128);
        engine
            .set_brush(BrushSnapshot {
                diameter: 8.0,
                spacing: 0.08,
                stabilization: layer_core::BrushStabilization {
                    strength: 0.,
                    ..layer_core::BrushStabilization::default()
                },
                ..BrushSnapshot::default()
            })
            .unwrap();

        let mut down = event(1, PenPhase::Down, 4.0);
        down.timestamp_ns = 1_000_000;
        let mut moved = event(2, PenPhase::Move, 20.0);
        moved.timestamp_ns = 5_000_000;
        let mut predicted = event(3, PenPhase::Move, 52.0);
        predicted.timestamp_ns = 13_000_000;
        predicted.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0);
        producer.push(down).unwrap();
        producer.push(moved).unwrap();
        producer.push(predicted).unwrap();
        engine
            .render_frame_for(moved.timestamp_ns, predicted.timestamp_ns)
            .unwrap();

        assert!(!engine.backend().preview.is_empty());
        assert!(dabs_cover_point(&engine.backend().preview, Point { x: 52., y: 16. }));
        assert_eq!(engine.metrics().platform_prediction_frames, 1);

        let mut up = event(4, PenPhase::Up, 28.0);
        up.timestamp_ns = 9_000_000;
        producer.push(up).unwrap();
        engine
            .render_frame_for(up.timestamp_ns, up.timestamp_ns)
            .unwrap();
        let stroke = engine.completed_stroke.as_ref().unwrap();
        assert_eq!(stroke.points.len(), 3);
        assert_eq!(stroke.points.last().unwrap().position.x, 28.0);
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
        assert_eq!(engine.backend().persistent, replay);
        assert!(engine.backend().preview.is_empty());
    }

    #[test]
    fn native_lift_prediction_uses_raw_pressure_and_never_changes_commit_or_next_contact() {
        for floor in [0., 0.5] {
            let (mut producer, mut engine) = engine("lift", 128, 128);
            let mut brush = engine.settings.brush.clone();
            brush.stabilization.strength = 0.;
            engine.set_brush(brush).unwrap();
            engine.set_pressure_curve(layer_core::PressureResponse::try_from(vec![[0., floor], [1., 1.]]).unwrap().into());
            let mut inputs = Vec::new();
            for (i, pressure) in [0.8, 0.6, 0.4, 0.2].into_iter().enumerate() {
                let mut sample = event(
                    i as u64 + 1,
                    if i == 0 {
                        PenPhase::Down
                    } else {
                        PenPhase::Move
                    },
                    4. + i as f32 * 16.,
                );
                sample.timestamp_ns = 1_000_000 + i as u64 * 4_000_000;
                sample.pressure = pressure;
                inputs.push(sample);
                producer.push(sample).unwrap();
                producer.push(PenEvent {
                    timestamp_ns: sample.timestamp_ns + 8_000_000,
                    surface_position: Point { x: sample.surface_position.x + 32., y: sample.surface_position.y },
                    flags: SampleFlags::PREDICTED,
                    phase: PenPhase::Move,
                    ..sample
                }).unwrap();
                engine
                    .render_frame_for(sample.timestamp_ns, sample.timestamp_ns + 8_000_000)
                    .unwrap();
            }
            assert!(dabs_cover_point(
                &engine.backend().preview,
                Point { x: 60., y: 16. }
            ));
            assert!(
                engine
                    .backend()
                    .preview
                    .iter()
                    .all(|dab| dab.center.x <= 60.01)
            );
            let mut up = event(5, PenPhase::Up, 56.);
            up.timestamp_ns = 14_000_000;
            up.pressure = 0.;
            inputs.push(up);
            producer.push(up).unwrap();
            engine
                .render_frame_for(up.timestamp_ns, up.timestamp_ns)
                .unwrap();
            let stroke = engine.completed_stroke.as_ref().unwrap();
            assert_eq!(stroke.points.len(), inputs.len());
            for (point, input) in stroke.points.iter().zip(inputs) {
                assert_eq!(point.position, input.surface_position);
                assert_eq!(point.pressure, floor + (1. - floor) * input.pressure);
            }
            let mut replay = Vec::new();
            DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
            assert_eq!(engine.backend().persistent, replay);
            assert!(engine.backend().preview.is_empty());
            for phase in [PenPhase::Up, PenPhase::Cancel] {
                let mut down = event(6, PenPhase::Down, 4.);
                down.timestamp_ns = 20_000_000;
                let mut moved = event(7, PenPhase::Move, 20.);
                moved.timestamp_ns = 24_000_000;
                producer.push(down).unwrap();
                producer.push(moved).unwrap();
                producer.push(PenEvent {
                    timestamp_ns: 32_000_000,
                    surface_position: Point { x: 52., y: 16. },
                    flags: SampleFlags::PREDICTED,
                    ..moved
                }).unwrap();
                engine.render_frame_for(24_000_000, 32_000_000).unwrap();
                assert!(
                    dabs_cover_point(&engine.backend().preview, Point { x: 52., y: 16. }),
                    "new contact has no old pressure or lead history"
                );
                producer
                    .push(PenEvent {
                        phase,
                        timestamp_ns: 25_000_000,
                        ..moved
                    })
                    .unwrap();
                engine.render_frame().unwrap();
                assert!(engine.backend().preview.is_empty());
            }
        }
    }

    #[test]
    fn disabled_feedback_has_no_preview_work() {
        let (mut producer, mut engine) = engine("feedback-off", 64, 64);
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: false,
                ..InstantFeedbackConfig::default()
            })
            .unwrap();
        producer.push(event(1, PenPhase::Down, 8.0)).unwrap();
        let mut predicted = event(2, PenPhase::Move, 24.0);
        predicted.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0);
        producer.push(predicted).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend().preview.is_empty());
    }

    const MATERIAL_STROKE_SAMPLES: usize = 3000;

    fn material_stroke(preset: DefaultBrushPreset, taper: f32) -> (InputProducer<PenEvent>, CanvasEngine<RecordingRenderer>) {
        let mut brush = default_brush(preset);
        brush.diameter = 512.;
        brush.taper.end_distance_diameters = taper;
        let (producer, mut engine) = engine("material stroke", 4096, 4096);
        engine.set_brush(brush).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: true, ..Default::default() }).unwrap();
        (producer, engine)
    }

    fn queue_material_samples(producer: &mut InputProducer<PenEvent>, samples: std::ops::Range<usize>) {
        let count = MATERIAL_STROKE_SAMPLES;
        for index in samples {
            let t = index as f32 / (count - 1) as f32;
            let phase = if index == 0 { PenPhase::Down } else if index + 1 == count { PenPhase::Up } else { PenPhase::Move };
            let mut sample = event(index as u64 + 1, phase, 256. + 3584. * (t * 3.).fract());
            sample.surface_position.y = 256. + 3584. * t;
            producer.push(sample).unwrap();
        }
    }

    fn assert_replays_completed_stroke(engine: &CanvasEngine<RecordingRenderer>, preset: DefaultBrushPreset) {
        let stroke = engine.completed_stroke.as_ref().unwrap();
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
        assert_eq!(engine.backend().persistent, replay, "{preset:?}");
    }

    #[test]
    fn material_replays_are_submitted_in_bounded_windows() {
        for preset in [DefaultBrushPreset::OpaqueGouache, DefaultBrushPreset::WatercolorWash] {
            for rebuild in [false, true] {
                let (mut producer, mut engine) = material_stroke(preset, if rebuild { 0. } else { 1. });
                let mut clock = MATERIAL_STROKE_SAMPLES as u64;
                let mut largest = 0;
                let mut drain = |engine: &mut CanvasEngine<RecordingRenderer>| {
                    while engine.has_pending_input() || engine.has_pending_document_edits() {
                        assert!(clock < 10 * MATERIAL_STROKE_SAMPLES as u64, "{preset:?} replay did not finish");
                        engine.render_frame_at(clock * 1_000_000).unwrap();
                        largest = largest.max(engine.backend().material_page_jobs);
                        clock += 1;
                    }
                };
                queue_material_samples(&mut producer, 0..MATERIAL_STROKE_SAMPLES / 2);
                drain(&mut engine);
                if rebuild {
                    engine.recover_failed_rasters().unwrap();
                }
                queue_material_samples(&mut producer, MATERIAL_STROKE_SAMPLES / 2..MATERIAL_STROKE_SAMPLES);
                drain(&mut engine);
                assert!(largest <= 2 * frame_steps::MATERIAL_PAGE_JOBS_PER_FRAME, "{preset:?} rebuild {rebuild}: {largest}");
                assert_replays_completed_stroke(&engine, preset);
            }
        }
    }

    #[test]
    fn material_input_backlog_is_consumed_in_bounded_frames() {
        for preset in [DefaultBrushPreset::OpaqueGouache, DefaultBrushPreset::NaturalBlender, DefaultBrushPreset::DitherBlender, DefaultBrushPreset::WatercolorWash, DefaultBrushPreset::LiquifyPush] {
            let (mut producer, mut engine) = material_stroke(preset, 0.);
            let count = MATERIAL_STROKE_SAMPLES;
            queue_material_samples(&mut producer, 0..count);
            let mut frames = 0;
            while engine.completed_stroke.is_none() {
                assert!(frames < 1000, "{preset:?} backlog did not drain");
                engine.render_frame_at((count as u64 + frames) * 1_000_000).unwrap();
                let jobs = engine.backend().material_page_jobs;
                assert!(engine.completed_stroke.is_some() || jobs <= frame_steps::MATERIAL_PAGE_JOBS_PER_FRAME * 5 / 4, "{preset:?} frame {frames}: {jobs}");
                frames += 1;
            }
            assert!(frames > 3, "{preset:?} drained in {frames} frames");
            assert_replays_completed_stroke(&engine, preset);
        }
    }

    #[test]
    fn finalized_contacts_are_independent_of_frame_cadence() {
        let mut smudge = default_brush(DefaultBrushPreset::NaturalBlender);
        smudge.diameter = 40.0;
        for brush in [BrushSnapshot::default(), smudge] {
            let chunked = brush.execution_class() == BrushExecution::Smudge;
            let render = |one_event_per_frame: bool, feedback: bool| {
                let (mut producer, mut engine) = engine("cadence", 256, 256);
                engine.set_brush(brush.clone()).unwrap();
                engine
                    .set_instant_feedback(InstantFeedbackConfig {
                        enabled: feedback,
                        ..Default::default()
                    })
                    .unwrap();
                let events = [
                    event(1, PenPhase::Down, 8.0),
                    event(2, PenPhase::Move, 70.0),
                    event(3, PenPhase::Move, 130.0),
                    event(4, PenPhase::Move, 190.0),
                    event(5, PenPhase::Up, 248.0),
                ];
                for event in events {
                    producer.push(event).unwrap();
                    if one_event_per_frame {
                        engine.render_frame_at(event.timestamp_ns).unwrap();
                        if chunked && event.phase == PenPhase::Down {
                            assert!(engine.backend().persistent.is_empty());
                            assert!(!engine.backend().preview.is_empty());
                        }
                    }
                }
                if !one_event_per_frame {
                    engine
                        .render_frame_at(events.last().unwrap().timestamp_ns)
                        .unwrap();
                }
                engine.render_frame().unwrap();
                let stroke = engine.completed_stroke.as_ref().unwrap();
                let mut replay = Vec::new();
                DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
                assert_eq!(engine.backend().persistent, replay);
                assert!(engine.backend().preview.is_empty());
                let batches: Vec<_> = engine
                    .backend()
                    .persistent_batches
                    .iter()
                    .map(|batch| (batch.1, batch.2, batch.3))
                    .collect();
                (replay, batches)
            };
            let (reference, batches) = render(true, false);
            for feedback in [false, true] {
                for one_event_per_frame in [true, false] {
                    let (dabs, chunks) = render(one_event_per_frame, feedback);
                    assert_eq!(dabs, reference);
                    assert!(!chunked || chunks == batches);
                }
            }
            if chunked {
                assert!(batches.iter().any(|batch| batch.2 > 1));
                assert!(
                    batches
                        .iter()
                        .all(|batch| batch.2 as usize <= MAX_SMUDGE_DABS_PER_BATCH)
                );
            }
        }
    }

    #[test]
    fn active_smudge_rebuild_restores_only_the_committed_frontier() {
        let (mut producer, mut engine) = engine("active-smudge-rebuild", 256, 256);
        let mut brush = default_brush(DefaultBrushPreset::NaturalBlender);
        brush.diameter = 40.0;
        brush.stabilization.strength = 0.;
        engine.set_brush(brush).unwrap();
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: false,
                ..InstantFeedbackConfig::default()
            })
            .unwrap();

        producer.push(event(1, PenPhase::Down, 8.0)).unwrap();
        producer.push(event(2, PenPhase::Move, 100.0)).unwrap();
        engine.render_frame().unwrap();
        let committed = engine.backend().persistent.clone();
        let preview = engine.backend().preview.clone();
        assert!(!committed.is_empty());
        assert!(!preview.is_empty());

        engine.rebuild_all = true;
        engine.render_frame().unwrap();
        assert_eq!(engine.backend().persistent, committed);
        assert_eq!(engine.backend().preview, preview);

        producer.push(event(3, PenPhase::Up, 200.0)).unwrap();
        engine.render_frame().unwrap();
        let stroke = engine.completed_stroke.as_ref().unwrap();
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, engine.document().composition().color.space, &mut replay);
        assert_eq!(engine.backend().persistent, replay);
        assert!(engine.backend().preview.is_empty());
    }

    #[test]
    fn destination_feedback_batches_preserve_the_required_ordering() {
        let test_dabs = (0..7)
            .map(|index| Dab {
                center: Point {
                    x: index as f32,
                    y: 1.0,
                },
                radii: [1.0, 1.0],
                rotation: [1.0, 0.0],
                motion: [1.0, 0.0],
                color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
                flow: 1.0,
                hardness: 1.0,
                texture_sign: [1.0, 1.0],
                material: [0.0; 4],
                previous: [0.0; 4],
                contact: [0.0; 4],
                previous_contact: [0.0; 4],
            })
            .collect::<Vec<_>>();
        let make_batch = |execution, first_dab, dab_count| {
            let mut style = DabStyle::for_brush(&BrushSnapshot::default(), StrokeTool::Brush);
            style.execution = execution;
            DabBatch {
                material_update: 0,
                stroke_id: StrokeId(7),
                target: SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(3)),
                kind: DabBatchKind::Persistent,
                stroke_start: first_dab == 0,
                stroke_end: false,
                first_dab,
                dab_count,
                style,
                damage: Rect {
                    min: Point { x: 1.0, y: 1.0 },
                    max: Point { x: 2.0, y: 2.0 },
                },
            }
        };

        for execution in [BrushExecution::Wet, BrushExecution::Watercolor] {
            let mut wet = Vec::new();
            push_batches(&mut wet, &test_dabs, make_batch(execution, 0, 7));
            assert_eq!(
                wet.iter().map(|batch| batch.dab_count).collect::<Vec<_>>(),
                vec![3, 3, 1]
            );
        }

        let mut watercolor_preview = make_batch(BrushExecution::Watercolor, 0, 7);
        watercolor_preview.kind = DabBatchKind::Preview;
        let mut preview = Vec::new();
        push_batches(&mut preview, &test_dabs, watercolor_preview);
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].dab_count, 7);

        let smudge_dabs = vec![Dab { radii: [10.0, 10.0], motion: [0.05, 0.0], ..test_dabs[0] }; MAX_SMUDGE_DABS_PER_BATCH + 1];
        let counts = |dabs: &[Dab], kind| {
            let mut batches = Vec::new();
            push_batches(&mut batches, dabs, DabBatch { kind, ..make_batch(BrushExecution::Smudge, 0, dabs.len() as u32) });
            batches.iter().map(|batch| batch.dab_count).collect::<Vec<_>>()
        };
        assert_eq!(counts(&smudge_dabs, DabBatchKind::Persistent), vec![MAX_SMUDGE_DABS_PER_BATCH as u32, 1]);
        assert_eq!(counts(&smudge_dabs, DabBatchKind::Preview), vec![smudge_dabs.len() as u32], "prediction takes one pass");
        let blade: Vec<_> = smudge_dabs.iter().map(|dab| Dab { radii: [2.0, 10.0], ..*dab }).collect();
        assert_eq!(counts(&blade, DabBatchKind::Persistent), vec![MAX_SMUDGE_DABS_PER_BATCH as u32, 1],
            "a broadside blade is chunked by its length");

        for kind in [DabBatchKind::Persistent, DabBatchKind::Preview] {
            let mut dither = make_batch(BrushExecution::Smudge, 0, smudge_dabs.len() as u32);
            dither.style.blender.kind = layer_core::BlenderKind::Dither;
            dither.kind = kind;
            let mut batches = Vec::new();
            push_batches(&mut batches, &smudge_dabs, dither);
            assert_eq!(batches.len(), 1, "Dither exposure can accumulate without repeated color selection");
            assert_eq!(batches[0].dab_count as usize, smudge_dabs.len());
        }
        let large: Vec<_> = smudge_dabs.iter().copied().cycle().take(192).map(|d| Dab {
            center: Point { x: 4096., y: 4096. }, radii: [2048.; 2], ..d
        }).collect();
        let mut dither = make_batch(BrushExecution::Smudge, 0, large.len() as u32);
        dither.style.blender.kind = layer_core::BlenderKind::Dither;
        let mut batches = Vec::new();
        push_batches(&mut batches, &large, dither);
        assert!(batches.iter().all(|batch| frame_steps::material_page_jobs(std::slice::from_ref(batch)) <= frame_steps::MATERIAL_PAGE_JOBS_PER_FRAME),
            "large Dither contacts cannot merge beyond the per-frame work budget");

        let mut liquify = Vec::new();
        push_batches(
            &mut liquify,
            &test_dabs,
            make_batch(BrushExecution::Liquify, 0, 4),
        );
        assert_eq!(liquify.len(), 1);
        assert_eq!(liquify[0].dab_count, 4);
    }
    #[test]
    fn large_selected_bake_and_cut_erase_stay_bounded_ordered_and_settle_once() {
        use layer_core::{CoverageSnapshot, RasterOperation, RasterOperationKind, Rect, Selection};
        use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};
        let mut document = Document::new(layer_core::authored::PortableId::random(), 3072, 2048, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let descriptor = RasterPlane::Color.descriptor(document.composition().color);
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![255; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap());
        active_paint_mut(&mut document).raster = RasterRevision::backed(RasterData {
            tiles: [[0, 0], [11, 7]].map(|coordinate| (TileKey { plane: RasterPlane::Color, coordinate }, tile.clone())).into(),
            ..Default::default()
        });
        let (_, mut engine) = engine_with(RecordingRenderer::default(), document, view(3072, 2048), TRANSFORM);
        engine.render_frame().unwrap();
        let original = engine.document().clone();
        let source_id = original.working.target.unwrap();
        let (_, output_id, insertion) = paint_insert(engine.document(), empty_paint(engine.document()), "Selected pixels", 0);
        let selected = Rect { min: Point { x: 300., y: 300. }, max: Point { x: 2700., y: 1700. } };
        let selection = Selection::polygon(selected.corners().to_vec()).unwrap();
        let mut coverage = CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(), engine.document().composition().size, [0; 2]);
        coverage.source.default_coverage = 0.;
        coverage.selection = Some(selection);
        let bake = RasterOperation { placement: layer_core::Affine::IDENTITY, coverage: coverage.clone(),
            kind: RasterOperationKind::Bake { scene: original.snapshot(), scope: layer_core::SceneScope::Members(vec![original.working.occurrence.unwrap()].into()), offset: Point::default() } };
        let erase = RasterOperation { placement: layer_core::Affine::IDENTITY, coverage,
            kind: RasterOperationKind::Erase { alpha_locked: false } };
        engine.insert_with_operations(insertion, vec![(output_id, bake), (source_id, erase)], None).unwrap();
        let mut frames = 0;
        while engine.wants_continuous_frames() {
            engine.render_frame().unwrap();
            frames += 1;
            assert!(frames <= 8, "only the bounded bake pieces and one erase are submitted");
        }
        let operations = &engine.backend().operation_batches;
        assert!(operations.len() > 2, "Cut does not disable bounded bake stepping");
        let (last, pieces) = operations.split_last().unwrap();
        assert_eq!(last.0, source_id);
        assert!(last.3, "the mixed operation publishes one final settlement");
        assert!(pieces.iter().all(|(target, _, damage, commit)| *target == output_id && !commit
            && damage.max.x - damage.min.x <= 1024. && damage.max.y - damage.min.y <= 1024.));
        let area: f32 = pieces.iter().map(|(_, _, damage, _)| (damage.max.x - damage.min.x) * (damage.max.y - damage.min.y)).sum();
        assert_eq!(area, 2560. * 1536., "only the page-aligned selected window is baked");
        let after = engine.document().clone();
        assert!(engine.undo().unwrap());
        assert_authored_eq(&engine.document().artwork, &original.artwork);
        assert!(!engine.can_undo(), "Bake and Cut are one undo step");
        assert!(engine.redo().unwrap());
        assert_authored_eq(&engine.document().artwork, &after.artwork);
    }

}
