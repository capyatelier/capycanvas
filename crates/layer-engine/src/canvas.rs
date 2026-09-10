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
    builder: StrokeBuilder,
    dab_generator: DabGenerator,
    finalized_real_points: usize,
    active_stroke: Option<ActiveStroke>,
    pending_smudge_dabs: Vec<Dab>,
    dabs: Vec<Dab>,
    batches: Vec<DabBatch>,
    rebuild_all: bool,
    composite_all: bool,
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
            builder: StrokeBuilder::with_capacity(capacity.stroke_points),
            dab_generator: DabGenerator::default(),
            finalized_real_points: 0,
            active_stroke: None,
            pending_smudge_dabs: Vec::with_capacity(MAX_SMUDGE_DABS_PER_BATCH),
            dabs: Vec::with_capacity(capacity.dabs_per_frame),
            batches: Vec::with_capacity(capacity.batches_per_frame),
            rebuild_all: true,
            composite_all: true,
            metrics: EngineMetrics::default(),
        })
    }

    pub fn document(&self) -> &Document {
        self.editor.document()
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
        if self.active_stroke.is_some() {
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
        }
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
        self.editor.preview(edit)?;
        self.composite_all = true;
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
        self.rebuild_all || self.composite_all
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
        let changed = self.editor.undo()?;
        self.rebuild_all |= changed;
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        let changed = self.editor.redo()?;
        self.rebuild_all |= changed;
        Ok(changed)
    }

    pub fn apply_edit(&mut self, edit: Edit) -> Result<(), DocumentError> {
        let rebuild = match &edit {
            Edit::InsertStroke(_) | Edit::RemoveStroke { .. } => true,
            Edit::InsertLayer { layer, .. } => !layer.strokes.is_empty() || layer.asset.is_some(),
            Edit::Batch(_) => true,
            Edit::ReplaceLayer(layer) => self.document().layer(layer.id).is_some_and(|old| {
                old.strokes != layer.strokes
                    || old.operations != layer.operations
                    || old.asset != layer.asset
                    || old
                        .mask
                        .as_ref()
                        .map(|m| (&m.initial, m.id, m.default_coverage))
                        != layer
                            .mask
                            .as_ref()
                            .map(|m| (&m.initial, m.id, m.default_coverage))
            }),
            _ => false,
        };
        let changes_composite = !matches!(&edit, Edit::SetActiveLayer { .. });
        self.editor.perform(edit)?;
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
        if self.rebuild_all {
            self.build_full_scene();
            self.rebuild_all = false;
            rebuilt = true;
        }
        self.build_predicted_preview(timestamp_ns, presentation_timestamp_ns);
        self.composite_all |= rebuilt;

        let packet = FramePacket {
            view: self.view,
            document_extent: [self.editor.document().width, self.editor.document().height],
            layers: &self.editor.document().layers,
            dabs: &self.dabs,
            dab_batches: &self.batches,
            reset_layers: rebuilt,
            composite_all: self.composite_all,
        };
        let result = self.backend.submit(packet).map_err(EngineError::Backend);

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

        let target_id = self
            .active_stroke
            .as_ref()
            .map_or(self.document().active_target(), |s| s.layer_id);
        let offset = self.document().layer_offset(target_id);
        transform.surface_to_document[4] -= offset.x;
        transform.surface_to_document[5] -= offset.y;
        match event.phase {
            PenPhase::Down => {
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
                let active = ActiveStroke {
                    id,
                    layer_id,
                    tool,
                    brush,
                    style,
                    feedback,
                    persistent_started: false,
                    committed_smudge_dabs: 0,
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
            for id in layer
                .mask
                .iter()
                .chain(layer.operations.iter().map(|o| &o.coverage))
                .flat_map(|m| m.strokes.iter())
            {
                if let Some(stroke) = self.editor.document().stroke(*id) {
                    strokes.push(Replay::Stroke(stroke.id));
                }
            }
            for index in 0..=layer.strokes.len() {
                for (op, _) in layer
                    .operations
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.after_stroke == index)
                {
                    strokes.push(Replay::Operation(layer.id, op as u32));
                }
                if let Some(stroke) = layer
                    .strokes
                    .get(index)
                    .and_then(|id| self.document().stroke(*id))
                {
                    strokes.push(Replay::Stroke(stroke.id));
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
            let mut generator = DabGenerator::default();
            generator.reset_for_replay(stroke);
            let mut started = false;
            for point in stroke.points.iter().copied() {
                let start = self.dabs.len();
                let damage = generator.append(point, &stroke.brush, &mut self.dabs);
                if self.dabs.len() == start {
                    continue;
                }
                push_batches(
                    &mut self.batches,
                    &self.dabs,
                    DabBatch {
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
            for point in self.builder.real_points()[..point_count].iter().copied() {
                let start = self.dabs.len();
                let damage = generator.append(point, &active.brush, &mut self.dabs);
                if self.dabs.len() == start {
                    continue;
                }
                push_batches(
                    &mut self.batches,
                    &self.dabs,
                    DabBatch {
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
        preview: Vec<Dab>,
        saw_reset: bool,
    }

    impl CanvasRenderer for RecordingRenderer {
        type Error = BackendError;

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
            }
            self.preview.clear();
            for batch in packet.dab_batches {
                let start = batch.first_dab as usize;
                let end = start + batch.dab_count as usize;
                let dabs = &packet.dabs[start..end];
                match batch.kind {
                    DabBatchKind::LayerOperation(_) => {}
                    DabBatchKind::Persistent => {
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
