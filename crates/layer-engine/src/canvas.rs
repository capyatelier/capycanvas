//! Platform-client canvas orchestration for Layer.
//!
//! The engine has one mutable owner. Platform callbacks only touch the SPSC
//! producer. Renderers receive one borrowed packet per display frame. The two
//! queue halves may also run sequentially on one event loop.

use crate::brush::{DabGenerator, dabs_cover_point, damage_for_dabs, lock_dab_tail};
use crate::feedback::{
    FeedbackConfigError, InstantFeedbackConfig, TipSource, estimate_tip, finalized_count,
    surface_distance,
};
use crate::input::{
    InputConsumer, PenEvent, PenPhase, PressureCurve, SampleFlags, StrokeBuilder, ToolKind,
    ViewTransform,
};
use layer_core::{
    BrushError, BrushExecution, BrushSnapshot, Document, DocumentError, Edit, Editor, LayerId,
    Rect, Stroke, StrokeId, StrokeTool,
};
use layer_render::{
    CanvasRenderer, Dab, DabBatch, DabBatchKind, DabMode, DabStyle, FramePacket, ViewState,
};
use std::{collections::VecDeque, fmt};

const TRANSFORM_HISTORY: usize = 16;
const INPUT_BATCH: usize = 4096;
// A small wet microbatch amortizes page ping-pong and reservoir passes while
// keeping exchange far below the eight-sample display-frame cadence that made
// carried color advance in visible bands.
const MAX_WET_DABS_PER_BATCH: u32 = 3;
// Smudge contacts compose into one bounded semi-Lagrangian backtrace. Live
// input retains an incomplete chunk as replaceable GPU preview work, so these
// boundaries depend on the stroke rather than display-frame packet cadence.
const MAX_SMUDGE_DABS_PER_BATCH: usize = 3;
const MAX_SMUDGE_TRAVEL_DIAMETERS: f32 = 0.14;
const MAX_SMUDGE_DAMAGE_DIAMETERS_SQUARED: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineCapacity {
    pub stroke_points: usize,
    pub dabs_per_frame: usize,
    pub batches_per_frame: usize,
}

impl Default for EngineCapacity {
    fn default() -> Self {
        Self {
            stroke_points: 65_536,
            dabs_per_frame: 32_768,
            batches_per_frame: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineMetrics {
    pub input_events: u64,
    pub stale_transform_fallbacks: u64,
    pub frames: u64,
    pub committed_strokes: u64,
    pub feedback_frames: u64,
    pub platform_prediction_frames: u64,
    pub engine_prediction_frames: u64,
    pub preview_dabs: u64,
    pub last_tip_gap_surface_px: f32,
    pub maximum_tip_gap_surface_px: f32,
    pub last_endpoint_correction_surface_px: f32,
    pub maximum_endpoint_correction_surface_px: f32,
}

#[derive(Clone, Debug)]
struct ActiveStroke {
    id: StrokeId,
    layer_id: LayerId,
    tool: StrokeTool,
    brush: BrushSnapshot,
    style: DabStyle,
    feedback: InstantFeedbackConfig,
    persistent_started: bool,
    committed_smudge_dabs: usize,
    material_updates: Vec<u32>,
    ruler: Option<layer_core::RulerConstraint>,
}

pub struct CanvasEngine<B: CanvasRenderer> {
    backend: B,
    editor: Editor,
    input: InputConsumer<PenEvent>,
    view: ViewState,
    transforms: VecDeque<ViewTransform>,
    pressure: PressureCurve,
    brush: BrushSnapshot,
    tool: StrokeTool,
    instant_feedback: InstantFeedbackConfig,
    ruler_snapping: Option<f32>,
    builder: StrokeBuilder,
    dab_generator: DabGenerator,
    finalized_real_points: usize,
    active_stroke: Option<ActiveStroke>,
    pending_smudge_dabs: Vec<Dab>,
    dabs: Vec<Dab>,
    batches: Vec<DabBatch>,
    transform_preview: Option<layer_render::TransformPreview>,
    rebuild_all: bool,
    composite_all: bool,
    animation_origin_ns: Option<u64>,
    metrics: EngineMetrics,
}

impl<B: CanvasRenderer> CanvasEngine<B> {
    pub fn new(
        backend: B,
        document: Document,
        input: InputConsumer<PenEvent>,
        view: ViewState,
        input_transform: ViewTransform,
    ) -> Result<Self, B::Error> {
        Self::with_capacity(
            backend,
            document,
            input,
            view,
            input_transform,
            EngineCapacity::default(),
        )
    }

    pub fn with_capacity(
        mut backend: B,
        document: Document,
        input: InputConsumer<PenEvent>,
        view: ViewState,
        input_transform: ViewTransform,
        capacity: EngineCapacity,
    ) -> Result<Self, B::Error> {
        backend.resize_surface(view.width_px, view.height_px)?;
        let mut transforms = VecDeque::with_capacity(TRANSFORM_HISTORY);
        transforms.push_back(input_transform);
        Ok(Self {
            backend,
            editor: Editor::new(document),
            input,
            view,
            transforms,
            pressure: PressureCurve::default(),
            brush: BrushSnapshot::default(),
            tool: StrokeTool::Brush,
            instant_feedback: InstantFeedbackConfig::default(),
            ruler_snapping: Some(12.),
            builder: StrokeBuilder::with_capacity(capacity.stroke_points),
            dab_generator: DabGenerator::default(),
            finalized_real_points: 0,
            active_stroke: None,
            pending_smudge_dabs: Vec::with_capacity(MAX_SMUDGE_DABS_PER_BATCH),
            dabs: Vec::with_capacity(capacity.dabs_per_frame),
            batches: Vec::with_capacity(capacity.batches_per_frame),
            transform_preview: None,
            rebuild_all: true,
            composite_all: true,
            animation_origin_ns: None,
            metrics: EngineMetrics::default(),
        })
    }

    pub fn document(&self) -> &Document {
        self.editor.document()
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

    pub fn metrics(&self) -> EngineMetrics {
        self.metrics
    }

    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    pub fn has_active_stroke(&self) -> bool {
        self.active_stroke.is_some()
    }

    pub fn brush(&self) -> &BrushSnapshot {
        self.active_stroke
            .as_ref()
            .map_or(&self.brush, |active| &active.brush)
    }

    /// Editable configuration for the next stroke, independent of an active
    /// stroke's immutable snapshot. UI edits must not restore old stroke values.
    pub fn configured_brush(&self) -> &BrushSnapshot {
        &self.brush
    }

    /// Cursor-only evaluation at current input, sharing the live stroke's
    /// pressure curve, sensors and random sequence. No paint is submitted.
    pub fn cursor_contacts(
        &self,
        event: PenEvent,
        hover: &mut DabGenerator,
        hover_start_ns: u64,
    ) -> Vec<Dab> {
        let mut point = crate::input::to_stroke_point(
            event,
            *self.transforms.back().unwrap(),
            self.pressure,
            hover_start_ns,
        );
        let ruler = self.active_stroke.as_ref().map_or_else(
            || {
                self.ruler_snapping.and_then(|reach| {
                    layer_core::choose_ruler(&self.document().rulers, point.position, reach)
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
            .map_or(self.document().active_target(), |s| s.layer_id);
        let offset = self.document().layer_offset(target);
        // Stroke dynamics run in layer-local coordinates; outlines are returned
        // in document coordinates, including for translated layers.
        point.position.x -= offset.x;
        point.position.y -= offset.y;
        let mut contacts = if self.active_stroke.is_some() {
            point.elapsed_micros = self
                .builder
                .elapsed_micros_at(event.timestamp_ns)
                .unwrap_or(0);
            let mut preview = self.dab_generator.clone();
            let mut ignored = Vec::new();
            for &sample in &self.builder.real_points()[self.finalized_real_points..] {
                preview.append(sample, self.brush(), &mut ignored);
            }
            preview.cursor_contacts(point, self.brush())
        } else {
            // Hovering pens report zero pressure; show the nominal footprint.
            point.pressure = 1.0;
            hover.cursor_seed(self.document().next_stroke_id(), self.brush());
            hover.cursor_contacts(point, self.brush())
        };
        for dab in &mut contacts {
            dab.center.x += offset.x;
            dab.center.y += offset.y;
        }
        contacts
    }

    pub fn create_paint_layer(
        &mut self,
        name: impl Into<std::sync::Arc<str>>,
        index: usize,
    ) -> Result<LayerId, DocumentError> {
        let id = self.editor.allocate_layer_id();
        self.apply_edit(Edit::InsertLayer {
            index,
            layer: layer_core::Layer::paint(id, name),
        })?;
        Ok(id)
    }

    pub fn allocate_layer_id(&mut self) -> LayerId {
        self.editor.allocate_layer_id()
    }
    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        self.editor.allocate_stroke_id()
    }
    pub fn preview_edit(&mut self, edit: Edit) -> Result<(), DocumentError> {
        let image = edit.changes_image();
        self.editor.preview(edit)?;
        self.composite_all |= image;
        Ok(())
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
            .and_then(|p| p.companion(&self.document().layers));
        for p in preview.iter().chain(companion.iter()) {
            let doc = self.document();
            if self.has_active_stroke()
                || doc.is_locked(p.layer)
                || doc
                    .target_owner(p.layer)
                    .is_none_or(|l| l.id == p.layer && l.kind != layer_core::LayerKind::Paint)
                || p.transform.affine.inverse().is_none()
                || p.selection.as_ref().is_some_and(|s| {
                    s.affine.inverse().is_none() || s.transformed(p.transform.affine).is_err()
                })
            {
                return Err(DocumentError::InvalidLayerOperation(
                    "Select an unlocked paint layer and finish the stroke",
                ));
            }
        }
        self.transform_preview = preview;
        Ok(())
    }

    /// Append an ordered raster operation without replaying unchanged strokes.
    /// Undo/device recovery still replay the same durable operation definition.
    pub fn append_layer_operation(
        &mut self,
        id: LayerId,
        operation: layer_core::LayerOperation,
    ) -> Result<(), DocumentError> {
        self.append_operations(vec![(id, operation)], None)
    }

    /// Commit the displayed pixels and moved selection as one undoable edit.
    /// The GPU can keep the matching preview result instead of resampling it.
    pub fn commit_transform(&mut self) -> Result<bool, DocumentError> {
        let preview = self
            .transform_preview
            .as_ref()
            .ok_or(DocumentError::InvalidLayerOperation(
                "No transform to apply",
            ))?
            .clone();
        if preview.transform.affine == layer_core::Affine::IDENTITY {
            self.transform_preview = None;
            return Ok(false);
        }
        let selection = self.display_selection().map(|s| s.into_owned());
        let companion = preview.companion(&self.document().layers);
        let mut operations = Vec::with_capacity(2);
        for target in std::iter::once(preview).chain(companion) {
            let mut coverage = layer_core::LayerMask::reveal_all(
                self.allocate_layer_id(),
                layer_core::Point::default(),
            );
            // Inversion is in the immutable packed selection, not mask metadata.
            coverage.default_coverage = f32::from(target.selection.is_none());
            coverage.initial = target.selection;
            operations.push((
                target.layer,
                layer_core::LayerOperation {
                    after_stroke: 0,
                    coverage,
                    kind: layer_core::LayerOperationKind::Transform(target.transform),
                },
            ));
        }
        self.append_operations(operations, Some(selection))?;
        Ok(true)
    }

    /// Provisional selection placement is view state, never another history edit.
    pub fn display_selection(&self) -> Option<std::borrow::Cow<'_, layer_core::Selection>> {
        if let Some(preview) = &self.transform_preview {
            let selection = preview.selection.as_ref()?;
            let offset = self.document().layer_offset(preview.layer);
            return selection
                .transformed(preview.transform.affine)
                .ok()
                .map(|s| std::borrow::Cow::Owned(s.translated(offset)));
        }
        self.document()
            .selection
            .as_ref()
            .map(std::borrow::Cow::Borrowed)
    }

    fn append_operations(
        &mut self,
        operations: Vec<(LayerId, layer_core::LayerOperation)>,
        // None preserves the selection; Some(None) explicitly clears it.
        selection_after: Option<Option<layer_core::Selection>>,
    ) -> Result<(), DocumentError> {
        if self.has_active_stroke() {
            return Err(DocumentError::InvalidLayerOperation(
                "Finish the stroke first",
            ));
        }
        let mut layers = std::collections::BTreeMap::new();
        let mut batches = Vec::with_capacity(operations.len());
        for (id, mut operation) in operations {
            let owner = self
                .document()
                .target_owner(id)
                .ok_or(DocumentError::MissingLayer(id))?;
            if (owner.id == id && owner.kind != layer_core::LayerKind::Paint)
                || self.document().is_locked(id)
            {
                return Err(DocumentError::InvalidLayerOperation(
                    "Select an unlocked paint layer",
                ));
            }
            let layer = layers.entry(owner.id).or_insert_with(|| owner.clone());
            let (strokes, history) = layer.target_history_mut(id).unwrap();
            let index = history.len() as u32;
            operation.after_stroke = strokes.len();
            let damage = operation.bounds([self.document().width, self.document().height]);
            history.push(operation);
            batches.push(DabBatch {
                material_update: 0,
                stroke_id: StrokeId(0),
                layer_id: id,
                kind: DabBatchKind::LayerOperation(index),
                stroke_start: false,
                stroke_end: false,
                first_dab: 0,
                dab_count: 0,
                style: style_for(&BrushSnapshot::default(), StrokeTool::Brush),
                damage,
            });
        }
        let mut edits: Vec<_> = layers
            .into_values()
            .map(|l| Edit::ReplaceLayer(Box::new(l)))
            .collect();
        if let Some(selection) = selection_after {
            edits.push(Edit::SetSelection(selection));
        }
        self.editor.perform(Edit::Batch(edits))?;
        self.transform_preview = None;
        self.batches.extend(batches);
        Ok(())
    }

    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), DocumentError> {
        self.apply_edit(Edit::RemoveLayer { id })
    }

    pub fn move_layer(&mut self, id: LayerId, to: usize) -> Result<(), DocumentError> {
        self.apply_edit(Edit::MoveLayer { id, to })
    }

    pub fn set_active_layer(&mut self, id: LayerId) -> Result<(), DocumentError> {
        self.apply_edit(Edit::SetActiveLayer { id })
    }

    /// Readbacks must follow the frame that applies document edits, not capture
    /// old GPU pixels under the new document revision.
    pub fn has_pending_document_edits(&self) -> bool {
        self.rebuild_all
            || self.composite_all
            || self
                .batches
                .iter()
                .any(|b| matches!(b.kind, DabBatchKind::LayerOperation(_)))
    }
    pub fn wants_continuous_frames(&self) -> bool {
        self.has_active_stroke() || self.editor.document().has_animated_effects()
    }

    pub fn set_layer_opacity(&mut self, id: LayerId, opacity: f32) -> Result<(), DocumentError> {
        self.apply_edit(Edit::SetLayerOpacity { id, opacity })
    }

    pub fn set_layer_visibility(
        &mut self,
        id: LayerId,
        visible: bool,
    ) -> Result<(), DocumentError> {
        self.apply_edit(Edit::SetLayerVisibility { id, visible })
    }

    pub fn set_brush(&mut self, brush: BrushSnapshot) -> Result<(), BrushError> {
        brush.validate()?;
        self.brush = brush;
        Ok(())
    }

    pub fn set_tool(&mut self, tool: StrokeTool) {
        self.tool = tool;
    }

    pub fn set_pressure_curve(&mut self, pressure: PressureCurve) {
        self.pressure = pressure;
    }

    /// UI supplies a logical hit distance converted into document units.
    /// A stroke's selected guide remains fixed until that stroke ends.
    pub fn set_ruler_snapping(&mut self, reach: Option<f32>) {
        self.ruler_snapping = reach.filter(|r| r.is_finite() && *r >= 0.);
    }
    pub fn active_ruler_constraint(&self) -> Option<layer_core::RulerConstraint> {
        self.active_stroke.as_ref().and_then(|s| s.ruler)
    }

    pub fn set_instant_feedback(
        &mut self,
        config: InstantFeedbackConfig,
    ) -> Result<(), FeedbackConfigError> {
        config.validate()?;
        self.instant_feedback = config;
        Ok(())
    }

    /// Both transforms must describe the same platform-view revision.
    pub fn set_view(&mut self, view: ViewState, input_transform: ViewTransform) {
        self.composite_all |= self.view.background_rgba_linear != view.background_rgba_linear;
        self.view = view;
        if self.transforms.back().map(|item| item.revision) != Some(input_transform.revision) {
            if self.transforms.len() == TRANSFORM_HISTORY {
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
        let image = self.editor.undo_changes_image();
        let changed = self.editor.undo()?;
        self.transform_preview = None;
        self.rebuild_all |= changed && image;
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        let image = self.editor.redo_changes_image();
        let changed = self.editor.redo()?;
        self.transform_preview = None;
        self.rebuild_all |= changed && image;
        Ok(changed)
    }

    pub fn apply_edit(&mut self, edit: Edit) -> Result<(), DocumentError> {
        fn rebuild_needed(document: &Document, edit: &Edit) -> bool {
            match edit {
                Edit::InsertStroke(_) | Edit::RemoveStroke { .. } => true,
                Edit::InsertLayer { layer, .. } => {
                    !layer.strokes.is_empty() || layer.asset.is_some()
                }
                Edit::Batch(edits) => edits.iter().any(|e| rebuild_needed(document, e)),
                // A batch may replace a layer inserted earlier in that batch;
                // it is absent from this pre-edit snapshot, so be conservative.
                Edit::ReplaceLayer(layer) => document.layer(layer.id).is_none_or(|old| {
                    old.strokes != layer.strokes
                        || old.operations != layer.operations
                        || old.asset != layer.asset
                        || old.mask.as_ref().map(|m| {
                            (
                                &m.initial,
                                m.id,
                                m.default_coverage,
                                &m.strokes,
                                &m.operations,
                            )
                        }) != layer.mask.as_ref().map(|m| {
                            (
                                &m.initial,
                                m.id,
                                m.default_coverage,
                                &m.strokes,
                                &m.operations,
                            )
                        })
                }),
                _ => false,
            }
        }
        let rebuild = rebuild_needed(self.document(), &edit);
        let changes_composite =
            edit.changes_image() && !matches!(&edit, Edit::SetActiveLayer { .. });
        self.editor.perform(edit)?;
        self.transform_preview = None;
        self.rebuild_all |= rebuild;
        self.composite_all |= changes_composite;
        Ok(())
    }

    /// Drain platform input, encode the newest incremental work, and return without
    /// waiting for GPU completion or presentation.
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
        let mut rebuilt = false;
        if self.rebuild_all {
            self.build_full_scene();
            self.rebuild_all = false;
            rebuilt = true;
        }
        self.process_input()?;
        if let Some(timestamp_ns) = timestamp_ns {
            self.append_continuous(timestamp_ns);
        }
        self.advance_finalized_prefix();
        self.record_material_update();
        if self.rebuild_all {
            self.build_full_scene();
            self.rebuild_all = false;
            rebuilt = true;
        }
        self.build_predicted_preview(timestamp_ns, presentation_timestamp_ns);
        self.composite_all |= rebuilt;

        let packet = FramePacket {
            time_seconds: timestamp_ns.map_or(0., |now| {
                now.saturating_sub(*self.animation_origin_ns.get_or_insert(now)) as f32 * 1e-9
            }),
            view: self.view,
            document_extent: [self.editor.document().width, self.editor.document().height],
            layers: &self.editor.document().layers,
            dabs: &self.dabs,
            dab_batches: &self.batches,
            reset_layers: rebuilt,
            composite_all: self.composite_all,
        };
        let selection = self.display_selection().map(|s| s.into_owned());
        let result = self
            .backend
            .set_selection_outline(selection.as_ref())
            .and_then(|_| {
                self.backend
                    .set_transform_preview(self.transform_preview.as_ref())
            })
            .and_then(|_| self.backend.submit(packet))
            .map_err(EngineError::Backend);

        self.dabs.clear();
        self.batches.clear();
        if result.is_err() {
            self.rebuild_all = true;
        } else {
            self.composite_all = false;
        }
        self.metrics.frames = self.metrics.frames.saturating_add(1);
        result
    }

    fn append_continuous(&mut self, timestamp_ns: u64) {
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        if active.brush.path.continuous_rate_hz <= 0.0 {
            return;
        }
        if let Some(point) = self.builder.append_stationary(timestamp_ns)
            && !active.feedback.enabled
        {
            self.append_real_dab(point);
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
            self.finalized_real_points
        } else {
            self.builder.real_points().len()
        } as u32;
        if end > active.material_updates.last().copied().unwrap_or(0) {
            active.material_updates.push(end);
        }
    }

    fn advance_finalized_prefix(&mut self) {
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        if !active.feedback.enabled {
            return;
        }
        let count = finalized_count(
            self.builder.real_points(),
            self.finalized_real_points,
            active.feedback.finalization_lag_micros,
        );
        while self.finalized_real_points < count {
            let point = self.builder.real_points()[self.finalized_real_points];
            self.append_real_dab(point);
            self.finalized_real_points += 1;
        }
    }

    fn finalize_active_tail(&mut self) {
        while self.finalized_real_points < self.builder.real_points().len() {
            let point = self.builder.real_points()[self.finalized_real_points];
            self.append_real_dab(point);
            self.finalized_real_points += 1;
        }
    }

    fn process_input(&mut self) -> Result<(), EngineError<B::Error>> {
        for _ in 0..INPUT_BATCH {
            let Some(event) = self.input.pop() else {
                break;
            };
            self.process_event(event)?;
        }
        Ok(())
    }

    fn process_event(&mut self, event: PenEvent) -> Result<(), EngineError<B::Error>> {
        self.metrics.input_events = self.metrics.input_events.saturating_add(1);
        let mut transform = self
            .transforms
            .iter()
            .rev()
            .find(|item| item.revision == event.view_revision)
            .copied()
            .unwrap_or_else(|| {
                self.metrics.stale_transform_fallbacks =
                    self.metrics.stale_transform_fallbacks.saturating_add(1);
                *self
                    .transforms
                    .back()
                    .expect("one transform is always retained")
            });

        let point = transform.map(event.surface_position);
        let mut ruler = if event.phase == PenPhase::Down {
            self.ruler_snapping
                .and_then(|reach| layer_core::choose_ruler(&self.document().rulers, point, reach))
        } else {
            self.active_ruler_constraint()
        };
        if let Some(snap) = &mut ruler {
            if !event.flags.contains(SampleFlags::PREDICTED) {
                snap.resolve(point);
            }
            transform.surface_to_document = snap.transform(transform.surface_to_document);
        }
        if let Some(active) = &mut self.active_stroke {
            active.ruler = ruler;
        }
        let target_id = self
            .active_stroke
            .as_ref()
            .map_or(self.document().active_target(), |s| s.layer_id);
        let offset = self.document().layer_offset(target_id);
        transform.surface_to_document[4] -= offset.x;
        transform.surface_to_document[5] -= offset.y;
        match event.phase {
            PenPhase::Down => {
                self.transform_preview = None;
                if self.active_stroke.is_some() {
                    self.cancel_active();
                }
                let layer_id = self.document().active_target();
                let Some(owner) = self.document().target_owner(layer_id) else {
                    return Ok(());
                };
                let is_mask = owner.id != layer_id;
                if self.document().is_locked(layer_id)
                    || (!is_mask && owner.kind != layer_core::LayerKind::Paint)
                {
                    return Ok(());
                }
                let alpha_locked = !is_mask && owner.properties.alpha_locked;
                let inverted = is_mask && owner.mask.as_ref().is_some_and(|m| m.inverted);
                let id = self.editor.allocate_stroke_id();
                let mut tool = if matches!(event.tool, ToolKind::Eraser)
                    || event.flags.contains(SampleFlags::INVERTED)
                {
                    StrokeTool::Eraser
                } else {
                    self.tool
                };
                let mut brush = self.brush.clone();
                let mut feedback = self.instant_feedback;
                if is_mask {
                    // Coverage brushes use the same tip/dynamics, not pigment or fluid state.
                    brush.color_rgba_linear = [1.0, 1.0, 1.0, brush.color_rgba_linear[3]];
                    brush.color_dynamics = Default::default();
                    brush.execution = BrushExecution::Dry;
                    brush.grain = None;
                    brush.dual = None;
                    brush.rendering = Default::default();
                    brush.transport = None;
                    brush.wet_mix = Default::default();
                    brush.deform = Default::default();
                    feedback.enabled = false;
                    if inverted {
                        tool = if tool == StrokeTool::Brush {
                            StrokeTool::Eraser
                        } else {
                            StrokeTool::Brush
                        };
                    }
                }
                let mut style = style_for(&brush, tool);
                style.alpha_locked = alpha_locked;
                style.selection = self.document().selection.as_ref().map(|selection| {
                    std::sync::Arc::new(selection.translated(layer_core::Point {
                        x: -offset.x,
                        y: -offset.y,
                    }))
                });
                let active = ActiveStroke {
                    id,
                    layer_id,
                    tool,
                    brush,
                    style,
                    feedback,
                    persistent_started: false,
                    committed_smudge_dabs: 0,
                    material_updates: Vec::new(),
                    ruler,
                };
                self.active_stroke = Some(active);
                self.pending_smudge_dabs.clear();
                self.finalized_real_points = 0;
                self.builder.begin(event, transform, self.pressure);
                self.dab_generator
                    .reset_for_stroke(id, &self.active_stroke.as_ref().expect("set above").brush);
                let point = *self
                    .builder
                    .real_points()
                    .last()
                    .expect("begin adds a point");
                if !self
                    .active_stroke
                    .as_ref()
                    .expect("set above")
                    .feedback
                    .enabled
                {
                    self.append_real_dab(point);
                }
            }
            PenPhase::Move => {
                if self.active_stroke.is_none() {
                    return Ok(());
                }
                self.builder.push(event, transform, self.pressure);
                if !event.flags.contains(SampleFlags::PREDICTED)
                    && !self
                        .active_stroke
                        .as_ref()
                        .expect("checked above")
                        .feedback
                        .enabled
                {
                    let point = *self
                        .builder
                        .real_points()
                        .last()
                        .expect("real move adds a point");
                    self.append_real_dab(point);
                }
            }
            PenPhase::Up => {
                if self.active_stroke.is_none() {
                    return Ok(());
                }
                self.builder.push(event, transform, self.pressure);
                if !event.flags.contains(SampleFlags::PREDICTED) {
                    if self
                        .active_stroke
                        .as_ref()
                        .expect("checked above")
                        .feedback
                        .enabled
                    {
                        self.finalize_active_tail();
                    } else {
                        let point = *self.builder.real_points().last().expect("up adds a point");
                        self.append_real_dab(point);
                    }
                }
                self.flush_smudge_chunks(true);
                self.finish_persistent_stroke();
                self.record_material_update();
                let active = self.active_stroke.take().expect("checked above");
                let points = self.builder.finish().unwrap_or_default();
                let has_end_taper = active.brush.taper.end_distance_diameters > 0.0;
                let alpha_locked = active.style.alpha_locked;
                let mut stroke = Stroke::new(
                    active.id,
                    active.layer_id,
                    active.tool,
                    active.brush,
                    points,
                )
                .map_err(EngineError::Document)?;
                stroke.alpha_locked = alpha_locked;
                stroke.material_updates = active.material_updates.into();
                stroke.selection = active.style.selection.clone();
                self.editor
                    .perform(Edit::InsertStroke(Box::new(stroke)))
                    .map_err(EngineError::Document)?;
                if has_end_taper {
                    // End taper depends on final stroke length. Replay after
                    // pen-up so the stored stroke and visible result agree.
                    self.rebuild_all = true;
                }
                self.metrics.committed_strokes = self.metrics.committed_strokes.saturating_add(1);
                self.finalized_real_points = 0;
            }
            PenPhase::Cancel => self.cancel_active(),
            PenPhase::Hover => {}
        }
        Ok(())
    }

    fn append_real_dab(&mut self, point: layer_core::StrokePoint) {
        let active = self.active_stroke.as_ref().expect("stroke is active");
        if active.style.execution == BrushExecution::Smudge {
            self.dab_generator
                .append(point, &active.brush, &mut self.pending_smudge_dabs);
            self.flush_smudge_chunks(false);
            return;
        }
        let stroke_id = active.id;
        let layer_id = active.layer_id;
        let style = active.style.clone();
        let stroke_start = !active.persistent_started;
        let material_update = active.material_updates.len() as u32;
        let start = self.dabs.len();
        let damage = self
            .dab_generator
            .append(point, &active.brush, &mut self.dabs);
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
                    layer_id,
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

    fn flush_smudge_chunks(&mut self, flush_tail: bool) {
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        if active.style.execution != BrushExecution::Smudge {
            return;
        }
        let stroke_id = active.id;
        let layer_id = active.layer_id;
        let style = active.style.clone();

        let mut consumed = 0;
        loop {
            let pending = &self.pending_smudge_dabs[consumed..];
            let chunk_len = next_smudge_chunk_len(pending);
            if chunk_len == 0 {
                break;
            }
            let reached_boundary =
                chunk_len == MAX_SMUDGE_DABS_PER_BATCH || chunk_len < pending.len();
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
                    layer_id,
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
            active.committed_smudge_dabs = active.committed_smudge_dabs.saturating_add(chunk_len);
        }
        if consumed > 0 {
            // One compaction after the event keeps a long coalesced segment
            // linear; draining each tiny chunk would repeatedly shift the
            // unconsumed tail.
            self.pending_smudge_dabs.drain(..consumed);
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
            layer_id: active.layer_id,
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
        let Some(active) = self.active_stroke.as_ref() else {
            return;
        };
        let start = self.dabs.len();
        if active.style.execution == BrushExecution::Smudge {
            self.dabs.extend_from_slice(&self.pending_smudge_dabs);
        }
        if !active.feedback.enabled {
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
        let fallback_timestamp = timestamp_ns.map(|timestamp| {
            timestamp.saturating_add(
                u64::from(active.feedback.prediction_horizon_micros).saturating_mul(1_000),
            )
        });
        let requested_elapsed = presentation_timestamp_ns
            .or(fallback_timestamp)
            .and_then(|timestamp| self.builder.elapsed_micros_at(timestamp))
            .unwrap_or_else(|| {
                latest
                    .elapsed_micros
                    .saturating_add(active.feedback.prediction_horizon_micros)
            });
        let Some(estimate) = estimate_tip(
            self.builder.real_points(),
            self.builder.predicted_points(),
            requested_elapsed,
            self.view.document_to_surface,
            active.feedback,
        ) else {
            self.push_active_preview(start);
            return;
        };
        let prediction_start = self.dabs.len();
        let mut generator = self.dab_generator.clone();
        for point in self.builder.real_points()[self.finalized_real_points..]
            .iter()
            .copied()
        {
            generator.append(point, &active.brush, &mut self.dabs);
        }
        if estimate.source == TipSource::Platform {
            for point in self
                .builder
                .predicted_points()
                .iter()
                .copied()
                .filter(|point| {
                    point.elapsed_micros > latest.elapsed_micros
                        && point.elapsed_micros < estimate.point.elapsed_micros
                })
            {
                generator.append(point, &active.brush, &mut self.dabs);
            }
        }
        if estimate.point.elapsed_micros > latest.elapsed_micros
            || estimate.point.position != latest.position
        {
            generator.append(estimate.point, &active.brush, &mut self.dabs);
        }

        let modeled_endpoint = generator.modeled_position().unwrap_or(latest.position);
        let locked_endpoint = layer_core::Point {
            x: modeled_endpoint.x
                + (estimate.point.position.x - modeled_endpoint.x) * active.feedback.tip_lock,
            y: modeled_endpoint.y
                + (estimate.point.position.y - modeled_endpoint.y) * active.feedback.tip_lock,
        };
        lock_dab_tail(
            &mut self.dabs[prediction_start..],
            modeled_endpoint,
            estimate.point.position,
            active.feedback.tip_lock,
            active.feedback.correction_easing,
        );
        if !dabs_cover_point(&self.dabs[start..], locked_endpoint) {
            generator.append_terminal_copy(locked_endpoint, &mut self.dabs);
        }
        if self.dabs.len() > start {
            self.push_active_preview(start);
            let tip_gap = surface_distance(
                locked_endpoint,
                estimate.point.position,
                self.view.document_to_surface,
            );
            let correction = surface_distance(
                modeled_endpoint,
                estimate.point.position,
                self.view.document_to_surface,
            );
            self.metrics.feedback_frames = self.metrics.feedback_frames.saturating_add(1);
            self.metrics.last_tip_gap_surface_px = tip_gap;
            self.metrics.maximum_tip_gap_surface_px =
                self.metrics.maximum_tip_gap_surface_px.max(tip_gap);
            self.metrics.last_endpoint_correction_surface_px = correction;
            self.metrics.maximum_endpoint_correction_surface_px = self
                .metrics
                .maximum_endpoint_correction_surface_px
                .max(correction);
            match estimate.source {
                TipSource::Platform => {
                    self.metrics.platform_prediction_frames =
                        self.metrics.platform_prediction_frames.saturating_add(1);
                }
                TipSource::Engine => {
                    self.metrics.engine_prediction_frames =
                        self.metrics.engine_prediction_frames.saturating_add(1);
                }
                TipSource::Real => {}
            }
        }
    }

    fn push_active_preview(&mut self, start: usize) {
        if self.dabs.len() == start {
            return;
        }
        let active = self.active_stroke.as_ref().expect("stroke is active");
        let damage = damage_for_dabs(&self.dabs[start..]);
        push_batches(
            &mut self.batches,
            &self.dabs,
            DabBatch {
                material_update: 0,
                stroke_id: active.id,
                layer_id: active.layer_id,
                kind: DabBatchKind::Preview,
                stroke_start: !active.persistent_started,
                stroke_end: false,
                first_dab: start.min(u32::MAX as usize) as u32,
                dab_count: self.dabs.len().saturating_sub(start).min(u32::MAX as usize) as u32,
                style: active.style.clone(),
                damage,
            },
        );
        self.metrics.preview_dabs = self
            .metrics
            .preview_dabs
            .saturating_add((self.dabs.len() - start) as u64);
    }

    fn build_full_scene(&mut self) {
        self.dabs.clear();
        self.batches.clear();
        enum Replay {
            Stroke(StrokeId),
            Operation(LayerId, u32),
        }
        let mut strokes = Vec::new();
        for layer in &self.editor.document().layers {
            for target in layer.masks().map(|m| m.id).chain([layer.id]) {
                let (ink, operations) = layer.target_history(target).unwrap();
                for index in 0..=ink.len() {
                    for (op, _) in operations
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| o.after_stroke == index)
                    {
                        strokes.push(Replay::Operation(target, op as u32));
                    }
                    if let Some(stroke) = ink.get(index).and_then(|id| self.document().stroke(*id))
                    {
                        strokes.push(Replay::Stroke(stroke.id));
                    }
                }
            }
        }
        for replay in strokes {
            let stroke = match replay {
                Replay::Stroke(id) => self
                    .editor
                    .document()
                    .stroke(id)
                    .expect("replay stroke exists"),
                Replay::Operation(layer_id, index) => {
                    self.batches.push(DabBatch {
                        material_update: 0,
                        stroke_id: StrokeId(0),
                        layer_id,
                        kind: DabBatchKind::LayerOperation(index),
                        stroke_start: false,
                        stroke_end: false,
                        first_dab: 0,
                        dab_count: 0,
                        style: style_for(&BrushSnapshot::default(), StrokeTool::Brush),
                        damage: Rect::EMPTY,
                    });
                    continue;
                }
            };
            let mut style = style_for(&stroke.brush, stroke.tool);
            style.alpha_locked = stroke.alpha_locked;
            style.selection = stroke.selection.clone();
            let mut generator = DabGenerator::default();
            generator.reset_for_replay(stroke);
            let mut started = false;
            for (point_index, point) in stroke.points.iter().copied().enumerate() {
                let start = self.dabs.len();
                let damage = generator.append(point, &stroke.brush, &mut self.dabs);
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
                        layer_id: stroke.layer_id,
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
            let mut generator = DabGenerator::default();
            generator.reset_for_stroke(active.id, &active.brush);
            let point_count = if active.feedback.enabled {
                self.finalized_real_points
            } else {
                self.builder.real_points().len()
            };
            if active.style.execution == BrushExecution::Smudge {
                let mut replay_dabs = Vec::new();
                for point in self.builder.real_points()[..point_count].iter().copied() {
                    generator.append(point, &active.brush, &mut replay_dabs);
                }
                replay_dabs.truncate(active.committed_smudge_dabs.min(replay_dabs.len()));
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
                            layer_id: active.layer_id,
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
                        layer_id: active.layer_id,
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

    fn cancel_active(&mut self) {
        self.builder.cancel();
        self.active_stroke = None;
        self.pending_smudge_dabs.clear();
        self.finalized_real_points = 0;
        self.dab_generator.reset();
        self.rebuild_all = true;
    }
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
        (BrushExecution::Dry, _) => u32::MAX,
        (BrushExecution::Wet | BrushExecution::Watercolor, _) => MAX_WET_DABS_PER_BATCH,
        (BrushExecution::Liquify, _) => 1,
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
            || (matches!(
                batch.style.execution,
                BrushExecution::Wet | BrushExecution::Watercolor
            ) && last.dab_count.saturating_add(batch.dab_count) <= MAX_WET_DABS_PER_BATCH)
            || (batch.style.execution == BrushExecution::Smudge
                && smudge_batch_fits(
                    &dabs[last.first_dab as usize
                        ..batch.first_dab.saturating_add(batch.dab_count) as usize],
                )))
        && last.stroke_id == batch.stroke_id
        && last.material_update == batch.material_update
        && last.layer_id == batch.layer_id
        && last.kind == batch.kind
        && !last.stroke_end
        && last.first_dab.saturating_add(last.dab_count) == batch.first_dab
        && last.style == batch.style
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

fn next_smudge_chunk_len(dabs: &[Dab]) -> usize {
    if dabs.is_empty() {
        return 0;
    }
    let mut damage = Rect::EMPTY;
    let mut travel = 0.0;
    let mut maximum_diameter = 0.0_f32;
    for (index, dab) in dabs.iter().take(MAX_SMUDGE_DABS_PER_BATCH).enumerate() {
        let candidate_damage = damage.union(damage_for_dabs(std::slice::from_ref(dab)));
        let candidate_travel = travel + dab.motion[0].hypot(dab.motion[1]);
        let candidate_diameter = maximum_diameter.max(dab.radii[0] * 2.0);
        if index > 0
            && (candidate_travel > candidate_diameter.max(0.01) * MAX_SMUDGE_TRAVEL_DIAMETERS
                || rect_area(candidate_damage)
                    > candidate_diameter.max(0.01).powi(2) * MAX_SMUDGE_DAMAGE_DIAMETERS_SQUARED)
        {
            return index;
        }
        damage = candidate_damage;
        travel = candidate_travel;
        maximum_diameter = candidate_diameter;
    }
    dabs.len().min(MAX_SMUDGE_DABS_PER_BATCH)
}

fn smudge_batch_fits(dabs: &[Dab]) -> bool {
    !dabs.is_empty()
        && dabs.len() <= MAX_SMUDGE_DABS_PER_BATCH
        && next_smudge_chunk_len(dabs) == dabs.len()
}

fn rect_area(rect: Rect) -> f32 {
    if rect.is_empty() {
        0.0
    } else {
        (rect.max.x - rect.min.x).max(0.0) * (rect.max.y - rect.min.y).max(0.0)
    }
}

fn style_for(brush: &BrushSnapshot, tool: StrokeTool) -> DabStyle {
    DabStyle {
        alpha_locked: false,
        selection: None,
        tip: brush.tip.clone(),
        mode: match tool {
            StrokeTool::Brush => DabMode::Paint,
            StrokeTool::Eraser => DabMode::Erase,
        },
        execution: brush.execution_class(),
        grain: brush.grain.clone(),
        dual: brush.dual.clone(),
        rendering: brush.rendering,
        wet_mix: brush.wet_mix,
        transport: brush.transport.clone(),
        deform: brush.deform,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{SampleFlags, ToolKind, input_queue};
    use layer_core::{AssetId, DefaultBrushPreset, Point, default_brush};
    use layer_render::{BackendError, CanvasRenderer, FramePacket, HostImage, ReadbackImage};

    #[derive(Default)]
    struct RecordingRenderer {
        size: [u32; 2],
        persistent_dabs: usize,
        persistent: Vec<Dab>,
        persistent_batches: Vec<(StrokeId, bool, bool, u32)>,
        material_batches: Vec<(u32, u32)>,
        preview: Vec<Dab>,
        styles: Vec<DabStyle>,
        saw_reset: bool,
        transform: Option<layer_render::TransformPreview>,
    }

    impl CanvasRenderer for RecordingRenderer {
        type Error = BackendError;
        fn set_transform_preview(
            &mut self,
            preview: Option<&layer_render::TransformPreview>,
        ) -> Result<(), Self::Error> {
            self.transform = preview.cloned();
            Ok(())
        }

        fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
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
            if self.size != [packet.view.width_px, packet.view.height_px] {
                return Err(BackendError("surface size mismatch"));
            }
            if packet.reset_layers {
                self.persistent.clear();
                self.material_batches.clear();
            }
            self.preview.clear();
            self.styles.clear();
            for batch in packet.dab_batches {
                self.styles.push(batch.style.clone());
                let start = batch.first_dab as usize;
                let end = start + batch.dab_count as usize;
                let dabs = &packet.dabs[start..end];
                match batch.kind {
                    DabBatchKind::LayerOperation(_) => {}
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

        fn request_readback(&mut self, _request_id: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
            None
        }
    }

    #[test]
    fn material_update_history_preserves_live_and_active_replay() {
        for feedback in [false, true] {
            for coalesced in [1, 2, 4] {
                let (mut input, consumer) = input_queue(32);
                let mut canvas = CanvasEngine::new(
                    RecordingRenderer::default(),
                    Document::new("material replay", 128, 128),
                    consumer,
                    view(128, 128),
                    ViewTransform {
                        revision: 1,
                        ..ViewTransform::IDENTITY
                    },
                )
                .unwrap();
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
                let stroke = canvas.document().strokes().next().unwrap();
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
        use layer_core::{LayerMask, Selection};
        use std::sync::Arc;
        for mask_target in [false, true] {
            let (mut producer, consumer) = input_queue(32);
            let selection = Selection::polygon(vec![
                Point { x: 4., y: 4. },
                Point { x: 50., y: 4. },
                Point { x: 50., y: 40. },
                Point { x: 4., y: 40. },
            ])
            .unwrap();
            let mut doc = Document::new("selected brush", 128, 128);
            doc.selection = Some(selection.clone());
            let layer = doc
                .layers
                .iter_mut()
                .find(|l| l.id == doc.active_layer)
                .unwrap();
            layer.properties.offset = Point { x: 3., y: 7. };
            let offset = if mask_target {
                let mut mask = LayerMask::reveal_all(LayerId(99), Point { x: 11., y: 2. });
                mask.linked = false;
                layer.mask = Some(mask);
                Point { x: 11., y: 2. }
            } else {
                layer.properties.offset
            };
            doc.apply(Edit::SetMaskTarget(mask_target)).unwrap();
            let expected = Arc::new(selection.translated(Point {
                x: -offset.x,
                y: -offset.y,
            }));
            let mut engine = CanvasEngine::new(
                RecordingRenderer::default(),
                doc,
                consumer,
                view(128, 128),
                ViewTransform {
                    revision: 1,
                    ..ViewTransform::IDENTITY
                },
            )
            .unwrap();
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
                .apply_edit(Edit::SetSelection(Some(inverse)))
                .unwrap();
            producer.push(event(2, PenPhase::Up, 48.)).unwrap();
            engine.render_frame().unwrap();
            let stroke = engine.document().strokes().next().unwrap();
            assert_eq!(stroke.selection.as_ref(), Some(&expected));
            assert_eq!(stroke.layer_id, engine.document().active_target());
            engine.apply_edit(Edit::SetSelection(None)).unwrap();
            engine.rebuild_all = true;
            engine.render_frame().unwrap();
            assert!(!engine.backend.styles.is_empty());
            assert!(
                engine
                    .backend
                    .styles
                    .iter()
                    .all(|s| s.selection.as_ref() == Some(&expected))
            );
            engine.undo().unwrap(); // Deselect.
            engine.undo().unwrap(); // Stroke.
            engine.render_frame().unwrap();
            assert_eq!(engine.document().strokes().count(), 0);
            engine.redo().unwrap();
            engine.redo().unwrap();
            engine.render_frame().unwrap();
            assert_eq!(
                engine
                    .document()
                    .strokes()
                    .next()
                    .unwrap()
                    .selection
                    .as_ref(),
                Some(&expected)
            );
            assert!(
                engine
                    .backend
                    .styles
                    .iter()
                    .all(|s| s.selection.as_ref() == Some(&expected))
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
            assert_eq!(engine.document().strokes().count(), 1);
        }
    }

    #[test]
    fn transform_preview_is_not_history_and_edits_or_new_strokes_cancel_it() {
        let (mut producer, consumer) = input_queue(32);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("preview", 128, 128),
            consumer,
            view(128, 128),
            ViewTransform::IDENTITY,
        )
        .unwrap();
        engine.render_frame().unwrap();
        let initial = engine.document().clone();
        let mut preview = layer_render::TransformPreview {
            transaction: 1,
            layer: initial.active_layer,
            selection: None,
            transform: layer_core::ImageTransform::default(),
        };
        for x in [12., 100., -23.] {
            preview.transform.affine = layer_core::Affine::translation(Point { x, y: 4. });
            engine.set_transform_preview(Some(preview.clone())).unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.backend.transform.as_ref(), Some(&preview));
            assert_eq!(engine.document().revision, initial.revision);
            assert_eq!(engine.document().layers, initial.layers);
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
        engine.set_layer_opacity(initial.active_layer, 0.5).unwrap();
        engine.render_frame().unwrap();
        assert!(engine.backend.transform.is_none());
    }

    #[test]
    fn linked_mask_transform_commits_both_histories_and_selection_as_one_edit() {
        use layer_core::{Affine, ImageTransform, LayerMask, LayerOperationKind, Selection};
        for primary_mask in [false, true] {
            let (_, consumer) = input_queue(32);
            let mut doc = Document::new("linked transform", 128, 128);
            doc.layers[0].properties.offset = Point { x: 7., y: 3. };
            doc.layers[0].mask = Some(LayerMask::reveal_all(LayerId(9), Point { x: 15., y: 11. }));
            doc.selection = Some(
                Selection::polygon(vec![
                    Point { x: 20., y: 20. },
                    Point { x: 60., y: 20. },
                    Point { x: 60., y: 60. },
                ])
                .unwrap(),
            );
            let before = doc.clone();
            let target = if primary_mask {
                LayerId(9)
            } else {
                doc.active_layer
            };
            let origin = doc.layer_offset(target);
            let preview = layer_render::TransformPreview {
                transaction: 1,
                layer: target,
                selection: doc.selection.as_ref().map(|s| {
                    s.translated(Point {
                        x: -origin.x,
                        y: -origin.y,
                    })
                }),
                transform: ImageTransform {
                    affine: Affine::around(
                        Point { x: 30., y: 30. },
                        [1.2, 0.7],
                        0.2,
                        Point { x: 5., y: 8. },
                    ),
                    ..Default::default()
                },
            };
            let companion = preview.companion(&doc.layers).unwrap();
            let mut engine = CanvasEngine::new(
                RecordingRenderer::default(),
                doc,
                consumer,
                view(128, 128),
                ViewTransform::IDENTITY,
            )
            .unwrap();
            engine.render_frame().unwrap();
            engine.set_transform_preview(Some(preview.clone())).unwrap();
            let moved = engine.display_selection().unwrap().into_owned();
            assert!(engine.commit_transform().unwrap());
            let layer = &engine.document().layers[0];
            for p in [&preview, &companion] {
                let (_, ops) = layer.target_history(p.layer).unwrap();
                assert_eq!(ops.len(), 1);
                assert_eq!(ops[0].kind, LayerOperationKind::Transform(p.transform));
                assert_eq!(ops[0].coverage.initial, p.selection);
            }
            assert_eq!(engine.batches.len(), 2);
            assert_eq!(engine.document().selection.as_ref(), Some(&moved));
            assert!(engine.undo().unwrap());
            assert_eq!(engine.document().layers, before.layers);
            assert_eq!(engine.document().selection, before.selection);
            assert!(!engine.can_undo());
            assert!(engine.redo().unwrap());
            engine.build_full_scene();
            assert_eq!(
                engine
                    .batches
                    .iter()
                    .filter(|b| matches!(b.kind, DabBatchKind::LayerOperation(_)))
                    .map(|b| b.layer_id)
                    .collect::<Vec<_>>(),
                [LayerId(9), before.active_layer]
            );
        }
    }

    #[test]
    fn applying_transform_moves_selection_atomically_and_cancel_keeps_original() {
        use layer_core::{Affine, ImageTransform, Selection};
        let (_, consumer) = input_queue(32);
        let mut doc = Document::new("selection transform", 128, 128);
        let layer = doc.active_layer;
        doc.layers[0].properties.offset = Point { x: 12., y: 7. };
        let selection = Selection::polygon(vec![
            Point { x: 20., y: 20. },
            Point { x: 60., y: 20. },
            Point { x: 20., y: 60. },
        ])
        .unwrap();
        doc.selection = Some(selection.clone());
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            doc,
            consumer,
            view(128, 128),
            ViewTransform::IDENTITY,
        )
        .unwrap();
        let preview = layer_render::TransformPreview {
            transaction: 1,
            layer,
            selection: Some(selection.translated(Point { x: -12., y: -7. })),
            transform: ImageTransform {
                affine: Affine::around(
                    Point { x: 28., y: 33. },
                    [1.5, 0.7],
                    0.5,
                    Point { x: 3., y: -2. },
                ),
                ..Default::default()
            },
        };
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        let placed = engine.display_selection().unwrap().into_owned();
        assert_ne!(placed, selection);
        assert_eq!(engine.document().selection.as_ref(), Some(&selection));
        engine.set_transform_preview(None).unwrap();
        assert_eq!(engine.display_selection().as_deref(), Some(&selection));
        assert!(!engine.can_undo());
        engine.set_transform_preview(Some(preview.clone())).unwrap();
        assert!(engine.commit_transform().unwrap());
        assert!(engine.transform_preview.is_none());
        assert_eq!(engine.document().selection.as_ref(), Some(&placed));
        let operation = &engine.document().layer(layer).unwrap().operations[0];
        assert_eq!(operation.coverage.initial, preview.selection);
        assert_eq!(
            operation.kind,
            layer_core::LayerOperationKind::Transform(preview.transform)
        );
        assert!(engine.undo().unwrap());
        assert_eq!(engine.document().selection.as_ref(), Some(&selection));
        assert!(
            engine
                .document()
                .layer(layer)
                .unwrap()
                .operations
                .is_empty()
        );
        assert!(
            !engine.can_undo(),
            "one undo restores both pixels and selection"
        );
        assert!(engine.redo().unwrap());
        assert_eq!(engine.document().selection.as_ref(), Some(&placed));
        let mut inverted = preview.clone();
        inverted.selection.as_mut().unwrap().inverted = true;
        engine.set_transform_preview(Some(inverted)).unwrap();
        assert!(engine.commit_transform().unwrap());
        assert!(engine.document().selection.as_ref().unwrap().inverted);
        assert!(engine.undo().unwrap());
        assert_eq!(engine.document().selection.as_ref(), Some(&placed));
        engine
            .set_transform_preview(Some(layer_render::TransformPreview {
                transform: Default::default(),
                ..preview
            }))
            .unwrap();
        assert!(
            !engine.commit_transform().unwrap(),
            "identity must not add history"
        );
    }

    #[test]
    fn appended_raster_operations_are_incremental_ordered_and_replayed_after_undo() {
        for kind in [
            layer_core::LayerOperationKind::Transform(layer_core::ImageTransform {
                affine: layer_core::Affine::translation(Point { x: 10., y: 4. }),
                ..Default::default()
            }),
            layer_core::LayerOperationKind::Gradient {
                start: Point::default(),
                end: Point { x: 128.0, y: 0.0 },
                colors: [[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]],
                radial: false,
                alpha_locked: false,
            },
        ] {
            let (mut producer, consumer) = input_queue(32);
            let mut engine = CanvasEngine::new(
                RecordingRenderer::default(),
                Document::new("gradient", 128, 128),
                consumer,
                view(128, 128),
                ViewTransform {
                    revision: 1,
                    ..ViewTransform::IDENTITY
                },
            )
            .unwrap();
            let id = engine.document().active_layer;
            engine.render_frame().unwrap();
            engine.backend.saw_reset = false;
            producer.push(event(1, PenPhase::Down, 20.0)).unwrap();
            producer.push(event(2, PenPhase::Up, 80.0)).unwrap();
            engine.render_frame().unwrap();
            let count = engine.backend.persistent_dabs;
            let coverage =
                layer_core::LayerMask::reveal_all(engine.allocate_layer_id(), Point::default());
            let op = layer_core::LayerOperation {
                after_stroke: 0,
                coverage,
                kind,
            };
            engine.append_layer_operation(id, op).unwrap();
            assert!(engine.has_pending_document_edits());
            assert_eq!(
                engine.document().layer(id).unwrap().operations[0].after_stroke,
                1
            );
            assert!(matches!(
                engine.batches[0].kind,
                DabBatchKind::LayerOperation(0)
            ));
            engine.render_frame().unwrap();
            assert!(!engine.backend.saw_reset);
            assert_eq!(
                engine.backend.persistent_dabs, count,
                "must not replay old strokes when appending a raster operation"
            );
            assert!(!engine.has_pending_document_edits());
            engine.undo().unwrap();
            assert!(engine.document().layer(id).unwrap().operations.is_empty());
            engine.render_frame().unwrap();
            assert!(engine.backend.saw_reset);
            engine.redo().unwrap();
            engine.render_frame().unwrap();
            assert_eq!(engine.document().layer(id).unwrap().operations.len(), 1);
        }
    }

    fn event(sequence: u64, phase: PenPhase, x: f32) -> PenEvent {
        PenEvent {
            device_id: 1,
            sequence,
            timestamp_ns: sequence * 1_000_000,
            view_revision: 1,
            surface_position: Point { x, y: 16.0 },
            pressure: 0.8,
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }
    }

    fn view(width_px: u32, height_px: u32) -> ViewState {
        ViewState {
            width_px,
            height_px,
            document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            background_rgba_linear: [1.0; 4],
        }
    }

    #[test]
    fn platform_events_commit_and_render_through_the_renderer_contract() {
        let (mut producer, consumer) = input_queue(32);
        producer.push(event(1, PenPhase::Down, 4.0)).unwrap();
        producer.push(event(2, PenPhase::Move, 16.0)).unwrap();
        producer.push(event(3, PenPhase::Up, 28.0)).unwrap();
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("test", 32, 32),
            consumer,
            view(32, 32),
            ViewTransform {
                revision: 1,
                ..ViewTransform::IDENTITY
            },
        )
        .unwrap();
        engine.render_frame().unwrap();
        assert_eq!(engine.document().strokes().count(), 1);
        assert_eq!(engine.metrics().committed_strokes, 1);
        assert!(engine.backend().persistent_dabs > 0);
        assert!(engine.backend().saw_reset);
    }

    #[test]
    fn rulers_project_real_prediction_and_replay_without_repainting_guide_edits() {
        use layer_core::{Ruler, RulerGeometry};
        for kind in [
            layer_core::RulerKind::Straight,
            layer_core::RulerKind::Parallel,
            layer_core::RulerKind::Radial,
        ] {
            let (mut producer, consumer) = input_queue(64);
            let mut engine = CanvasEngine::new(
                RecordingRenderer::default(),
                Document::new("ruler", 128, 128),
                consumer,
                view(128, 128),
                ViewTransform {
                    revision: 1,
                    ..ViewTransform::IDENTITY
                },
            )
            .unwrap();
            engine
                .set_brush(default_brush(DefaultBrushPreset::GPen))
                .unwrap();
            engine.render_frame().unwrap();
            engine.backend.saw_reset = false;
            let guide = Ruler {
                id: 1,
                geometry: RulerGeometry::from_drag(
                    kind,
                    Point { x: 0., y: 16. },
                    Point { x: 100., y: 16. },
                ),
            };
            engine.apply_edit(Edit::SetRulers(vec![guide])).unwrap();
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
                .apply_edit(Edit::SetRulers(vec![Ruler {
                    id: 1,
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
            let stroke = engine.document().strokes().next().unwrap().clone();
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
                    .document()
                    .strokes()
                    .last()
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
    fn ruler_input_and_cursor_respect_view_layer_offsets_and_radial_origin() {
        use layer_core::{Ruler, RulerGeometry};
        let (mut producer, consumer) = input_queue(64);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("ruler-view", 128, 128),
            consumer,
            view(256, 256),
            ViewTransform {
                revision: 1,
                surface_to_document: [0., 0.5, -0.5, 0., 64., 0.],
            },
        )
        .unwrap();
        engine
            .set_brush(default_brush(DefaultBrushPreset::GPen))
            .unwrap();
        let mut layer = engine.document().layers[0].clone();
        layer.properties.offset = Point { x: 3., y: 5. };
        engine
            .apply_edit(Edit::ReplaceLayer(Box::new(layer)))
            .unwrap();
        engine
            .apply_edit(Edit::SetRulers(vec![Ruler {
                id: 1,
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
                .document()
                .strokes()
                .next()
                .unwrap()
                .points
                .iter()
                .all(|p| (p.position.y - 27.).abs() < 0.001)
        );
        engine
            .apply_edit(Edit::SetRulers(vec![Ruler {
                id: 1,
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
                .document()
                .strokes()
                .last()
                .unwrap()
                .points
                .iter()
                .all(|p| (p.position.y - 27.).abs() < 0.001)
        );
    }

    #[test]
    fn persistent_batches_are_incremental_and_preserve_stroke_boundaries() {
        let (mut producer, consumer) = input_queue(32);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("incremental-batches", 64, 64),
            consumer,
            view(64, 64),
            ViewTransform {
                revision: 1,
                ..ViewTransform::IDENTITY
            },
        )
        .unwrap();
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
        let (mut producer, consumer) = input_queue(16);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("brush-snapshot", 64, 64),
            consumer,
            view(64, 64),
            ViewTransform {
                revision: 1,
                ..ViewTransform::IDENTITY
            },
        )
        .unwrap();
        producer.push(event(1, PenPhase::Down, 8.0)).unwrap();
        engine.render_frame().unwrap();

        let next_brush = BrushSnapshot {
            diameter: 40.0,
            ..BrushSnapshot::default()
        };
        engine.set_brush(next_brush).unwrap();
        producer.push(event(2, PenPhase::Up, 24.0)).unwrap();
        engine.render_frame().unwrap();

        let stroke = engine.document().strokes().next().unwrap();
        assert_eq!(stroke.brush.diameter, BrushSnapshot::default().diameter);
    }

    #[test]
    fn feedback_tail_reaches_platform_prediction_without_committing_it() {
        let (mut producer, consumer) = input_queue(32);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("feedback", 128, 128),
            consumer,
            view(128, 128),
            ViewTransform {
                revision: 1,
                ..ViewTransform::IDENTITY
            },
        )
        .unwrap();
        engine
            .set_brush(BrushSnapshot {
                diameter: 8.0,
                spacing: 0.08,
                stabilization: layer_core::BrushStabilization {
                    streamline: 0.85,
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
        assert!(dabs_cover_point(
            &engine.backend().preview,
            layer_core::Point { x: 52.0, y: 16.0 }
        ));
        assert!(engine.metrics().last_tip_gap_surface_px < 0.001);
        assert_eq!(engine.metrics().platform_prediction_frames, 1);

        let mut up = event(4, PenPhase::Up, 28.0);
        up.timestamp_ns = 9_000_000;
        producer.push(up).unwrap();
        engine
            .render_frame_for(up.timestamp_ns, up.timestamp_ns)
            .unwrap();
        let stroke = engine.document().strokes().next().unwrap();
        assert_eq!(stroke.points.len(), 3);
        assert_eq!(stroke.points.last().unwrap().position.x, 28.0);
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, &mut replay);
        assert_eq!(engine.backend().persistent, replay);
        assert!(engine.backend().preview.is_empty());
    }

    #[test]
    fn disabled_feedback_has_no_preview_work() {
        let (mut producer, consumer) = input_queue(16);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("feedback-off", 64, 64),
            consumer,
            view(64, 64),
            ViewTransform {
                revision: 1,
                ..ViewTransform::IDENTITY
            },
        )
        .unwrap();
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
        assert_eq!(engine.metrics().feedback_frames, 0);
    }

    #[test]
    fn finalized_contacts_are_independent_of_frame_cadence() {
        let render = |one_event_per_frame: bool| {
            let (mut producer, consumer) = input_queue(32);
            let mut engine = CanvasEngine::new(
                RecordingRenderer::default(),
                Document::new("cadence", 128, 128),
                consumer,
                view(128, 128),
                ViewTransform {
                    revision: 1,
                    ..ViewTransform::IDENTITY
                },
            )
            .unwrap();
            let events = [
                event(1, PenPhase::Down, 8.0),
                event(2, PenPhase::Move, 19.0),
                event(3, PenPhase::Move, 37.0),
                event(4, PenPhase::Move, 54.0),
                event(5, PenPhase::Up, 72.0),
            ];
            for event in events {
                producer.push(event).unwrap();
                if one_event_per_frame {
                    engine.render_frame_at(event.timestamp_ns).unwrap();
                }
            }
            if !one_event_per_frame {
                engine
                    .render_frame_at(events.last().unwrap().timestamp_ns)
                    .unwrap();
            }
            engine.backend().persistent.clone()
        };
        assert_eq!(render(true), render(false));
    }

    #[test]
    fn smudge_chunks_are_live_and_independent_of_frame_cadence() {
        let render = |one_event_per_frame: bool| {
            let (mut producer, consumer) = input_queue(32);
            let mut engine = CanvasEngine::new(
                RecordingRenderer::default(),
                Document::new("smudge-cadence", 256, 256),
                consumer,
                view(256, 256),
                ViewTransform {
                    revision: 1,
                    ..ViewTransform::IDENTITY
                },
            )
            .unwrap();
            let mut brush = default_brush(DefaultBrushPreset::NaturalBlender);
            brush.diameter = 40.0;
            engine.set_brush(brush).unwrap();
            engine
                .set_instant_feedback(InstantFeedbackConfig {
                    enabled: false,
                    ..InstantFeedbackConfig::default()
                })
                .unwrap();
            let events = [
                event(1, PenPhase::Down, 8.0),
                event(2, PenPhase::Move, 70.0),
                event(3, PenPhase::Move, 130.0),
                event(4, PenPhase::Move, 190.0),
                event(5, PenPhase::Up, 248.0),
            ];
            for (index, event) in events.into_iter().enumerate() {
                producer.push(event).unwrap();
                if one_event_per_frame {
                    engine.render_frame_at(event.timestamp_ns).unwrap();
                    if index == 0 {
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
            let stroke = engine.document().strokes().next().unwrap();
            let mut replay = Vec::new();
            DabGenerator::generate(stroke, &mut replay);
            assert_eq!(engine.backend().persistent, replay);
            assert!(engine.backend().preview.is_empty());
            (
                engine.backend().persistent.clone(),
                engine
                    .backend()
                    .persistent_batches
                    .iter()
                    .map(|batch| (batch.1, batch.2, batch.3))
                    .collect::<Vec<_>>(),
            )
        };

        let incremental = render(true);
        let single_frame = render(false);
        assert_eq!(incremental, single_frame);
        assert!(incremental.1.iter().any(|batch| batch.2 > 1));
        assert!(
            incremental
                .1
                .iter()
                .all(|batch| batch.2 as usize <= MAX_SMUDGE_DABS_PER_BATCH)
        );
    }

    #[test]
    fn active_smudge_rebuild_restores_only_the_committed_frontier() {
        let (mut producer, consumer) = input_queue(16);
        let mut engine = CanvasEngine::new(
            RecordingRenderer::default(),
            Document::new("active-smudge-rebuild", 256, 256),
            consumer,
            view(256, 256),
            ViewTransform {
                revision: 1,
                ..ViewTransform::IDENTITY
            },
        )
        .unwrap();
        let mut brush = default_brush(DefaultBrushPreset::NaturalBlender);
        brush.diameter = 40.0;
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
        let stroke = engine.document().strokes().next().unwrap();
        let mut replay = Vec::new();
        DabGenerator::generate(stroke, &mut replay);
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
            })
            .collect::<Vec<_>>();
        let make_batch = |execution, first_dab, dab_count| {
            let mut style = style_for(&BrushSnapshot::default(), StrokeTool::Brush);
            style.execution = execution;
            DabBatch {
                material_update: 0,
                stroke_id: StrokeId(7),
                layer_id: LayerId(3),
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

        let mut smudge_dabs = test_dabs.clone();
        for dab in &mut smudge_dabs {
            dab.radii = [10.0, 10.0];
            dab.motion = [0.05, 0.0];
        }
        let mut smudge = Vec::new();
        push_batches(
            &mut smudge,
            &smudge_dabs,
            make_batch(BrushExecution::Smudge, 0, 4),
        );
        assert_eq!(
            smudge
                .iter()
                .map(|batch| batch.dab_count)
                .collect::<Vec<_>>(),
            vec![MAX_SMUDGE_DABS_PER_BATCH as u32, 1]
        );

        let mut liquify = Vec::new();
        push_batches(
            &mut liquify,
            &test_dabs,
            make_batch(BrushExecution::Liquify, 0, 4),
        );
        assert_eq!(liquify.len(), 4);
        assert!(liquify.iter().all(|batch| batch.dab_count == 1));
    }
}
