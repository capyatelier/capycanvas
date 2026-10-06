//! Shared click-to-select/fill policy. Hosts forward input and render controls;
//! source choice, cancellation, stale-result handling and edits stay here.
use crate::localization::{Localizer, MessageId};
use super::*;
use layer_core::{Point, Selection, authored::{OccurrenceHandle,SceneScope,SourceTarget}};
use layer_render::RegionRequest;

pub(super) struct RegionTools {
    pub tolerance: f32,
    pub refinement: layer_render::RegionRefinement,
    pub source: [RegionSource; 2],
    pub enclose_source: RegionSource,
    contact: Option<Point>,
    generation: u64,
    queued: Option<RegionRequest>,
    pending: bool,
    target: Option<Target>,
}
struct Target {
    generation: u64,
    revision: u64,
    layer: Option<OccurrenceHandle>,
    source: Option<SourceTarget>,
    owner: u64,
    purpose: Purpose,
}
enum Purpose {
    Region {
        operation: Option<layer_core::RasterOperationKind>,
        color: Option<layer_core::color::RgbColor>,
        basis: layer_core::Affine,
    },
    Tonal,
    Transform,
    Refine,
}
impl Default for RegionTools {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            refinement: layer_render::RegionRefinement {
                smoothing: 1.,
                ..Default::default()
            },
            source: [RegionSource::Visible; 2],
            enclose_source: RegionSource::Reference,
            contact: None,
            generation: 0,
            queued: None,
            pending: false,
            target: None,
        }
    }
}
impl RegionTools {
    pub(super) fn fields(&self) -> [(&'static str, MessageId, Option<MessageId>, NumericControl, f32); 4] {
        let distance = layer_render::RegionRefinement::MAX_DISTANCE as f64;
        [
            (
                "tolerance",
                MessageId::TOOL_CONTROL_TOLERANCE,
                None,
                NumericControl::percent(),
                self.tolerance,
            ),
            (
                "gap_closing",
                MessageId::TOOL_CONTROL_GAP_CLOSING,
                Some(MessageId::TOOL_CONTROL_GROUP_EDGES),
                NumericControl::number(0., distance, 1., 0).unit("px"),
                self.refinement.gap_closing as f32,
            ),
            (
                "expansion",
                MessageId::TOOL_CONTROL_EXPANSION,
                Some(MessageId::TOOL_CONTROL_GROUP_EDGES),
                NumericControl::number(-distance, distance, 1., 0).unit("px"),
                self.refinement.expansion as f32,
            ),
            (
                "smoothing",
                MessageId::TOOL_CONTROL_SMOOTHING,
                Some(MessageId::TOOL_CONTROL_GROUP_EDGES),
                NumericControl::percent(),
                self.refinement.smoothing,
            ),
        ]
    }
    pub fn controls(&self, localizer: &Localizer) -> Vec<ToolSetting> {
        self.fields().into_iter()
        .map(|(id, label, group, numeric, value)| ToolSetting {
            id,
            label: localizer.text(label),
            label_id: label,
            group: group.map(|id| localizer.text(id)).unwrap_or_else(|| std::sync::Arc::from("")),
            numeric,
            value,
        })
        .collect()
    }
    pub fn edit(&mut self, id: &str, value: f32, localizer: &Localizer) -> Result<(), String> {
        self.edit_value(id, value).map_err(|reason| reason.message(localizer))
    }
    pub fn edit_value(&mut self, id: &str, value: f32) -> Result<(), WorkspaceValidationError> {
        let (_, label, _, numeric, _) = self.fields().into_iter().find(|field| field.0 == id).ok_or("Unknown region setting")?;
        numeric.validate(value, label)?;
        if matches!(id, "gap_closing" | "expansion") && value.fract() != 0. {
            return Err(NumericError::WholePixels { label: label.into() }.into());
        }
        match id {
            "tolerance" => self.tolerance = value,
            "gap_closing" => self.refinement.gap_closing = value as u32,
            "expansion" => self.refinement.expansion = value as i32,
            "smoothing" => self.refinement.smoothing = value,
            _ => unreachable!(),
        }
        self.cancel();
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.contact = None;
        self.queued = None;
        self.target = None;
    }
    /// Cancel, and report whether the renderer was working on a request.
    pub fn abandon(&mut self) -> bool {
        self.cancel();
        std::mem::take(&mut self.pending)
    }
    pub fn renderer_replaced(&mut self) {
        self.cancel();
        self.pending = false;
    }
    pub fn busy(&self) -> bool {
        self.pending || self.queued.is_some()
    }
    pub fn publishing_edit(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Region { .. }))
    }
    pub fn applying_transform(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Transform))
    }
    pub fn refining(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Refine))
    }
    pub fn cancellable(&self) -> bool {
        self.contact.is_some() || self.target.is_some()
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn region_pen(&mut self, event: PenEvent) {
        match event.phase {
            PenPhase::Down => {
                self.region_tools.cancel();
                self.region_tools.contact = Some(
                    self.state
                        .camera
                        .input_transform()
                        .map(event.surface_position),
                );
            }
            PenPhase::Up => {
                let Some(point) = self.region_tools.contact.take() else {
                    return;
                };
                self.request_region_at(point, None);
            }
            PenPhase::Cancel => self.region_tools.cancel(),
            _ => (),
        }
    }
    pub(super) fn enclose_fill(&mut self, enclosure: Selection) {
        self.request_region_at(Point { x: 0., y: 0. }, Some(enclosure));
    }
    fn request_region_at(&mut self, point: Point, enclosure: Option<Selection>) {
        let enclosed = enclosure.is_some();
        let Some((fill, source, contiguous)) = self.layer_interaction.tool.region() else {
            return;
        };
        let doc = self.engine.document();
        let mask_target = self.selection_masks.target().filter(|_| fill);
        let source_target=if mask_target.is_some() {
            self.selection_masks.artwork().and_then(|h|doc.scene().source_target(h))
        } else {doc.drawing_target().or(doc.active_target())};
        let basis = layer_core::Affine::IDENTITY;
        let extent = doc.composition().size;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.
            || point.y < 0.
            || point.x >= extent[0] as f32
            || point.y >= extent[1] as f32
        {
            return;
        }
        if fill && mask_target.is_none() && doc.drawing_content().is_none() {
            self.notify_drawing_refusal();
            return;
        }
        let request = RegionRequest {
            enclosure: enclosure.map(std::sync::Arc::new), request_id: self.region_tools.generation,
            contiguous,
            selection: if enclosed {
                doc.working.selection.clone().map(|previous| layer_render::SelectionRefinement {
                    resize: 0, mode: layer_core::SelectionMode::Intersect, antialias: true,
                    feather: 0., previous: Some(std::sync::Arc::new(previous)),
                    source_to_document: basis, keep_canvas_edges: false,
                })
            } else if fill { None } else { self.selection_refinement(basis) },
            source: match source {
                RegionSource::Visible => layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::All},
                RegionSource::Editing => {
                    if let Some(target) = source_target {
                        layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::Raw(target)}
                    } else if let Some(handle) = doc.working.occurrence.filter(|handle| doc.scene().object_layer(*handle).is_some()) {
                        layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::RawObjects(handle)}
                    } else {
                        let Some(handle) = doc.working.occurrence.filter(|handle| doc.scene().effect(*handle).is_some_and(|effect| effect.constant_color().is_some())) else { return; };
                        let mut snapshot = doc.snapshot();
                        let occurrence = std::sync::Arc::make_mut(&mut snapshot).artwork.occurrences.get_mut(handle).unwrap();
                        occurrence.visible = true;
                        occurrence.opacity = 1.;
                        occurrence.blend = layer_core::LayerBlend::Normal;
                        occurrence.attachment = layer_core::Attachment::None;
                        occurrence.mask = None;
                        layer_render::RegionSource::Scene {snapshot,scope:SceneScope::Members(vec![handle].into())}
                    }
                }
                RegionSource::Reference => {
                    if doc.scene().references().is_empty() {
                        self.notify_missing_reference();
                        return;
                    }
                    layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:doc.reference_scope()}
                }
            },
            position: [point.x as u32, point.y as u32],
            tolerance: self.region_tools.tolerance,
            refinement: layer_render::RegionRefinement {
                gap_closing: if contiguous { self.region_tools.refinement.gap_closing } else { 0 },
                smoothing: if !fill && !self.selection_tools.options.antialias { 0. } else { self.region_tools.refinement.smoothing },
                ..self.region_tools.refinement
            },
            limit: if !enclosed && fill && mask_target.is_none() {
                doc.working.selection.clone().map(std::sync::Arc::new)
            } else {
                None
            },
        };
        if let Some(target) = mask_target {
            if let Err(error) = self.queue_mask_region(target, request, basis, true) { self.notify(error); }
            return;
        }
        let purpose = Purpose::Region {
            operation: fill.then(|| self.fill_operation()),
            color: (fill
                && !self.state.colors.transparent()
                && self.state.brush.opacity > 0.)
                .then(|| self.state.colors.definition()),
            basis: if !fill && self.selection_refinement(basis).is_some() { layer_core::Affine::IDENTITY } else { basis },
        };
        self.queue_region(request, purpose);
    }
    fn queue_region(&mut self, mut request: RegionRequest, purpose: Purpose) {
        self.region_tools.cancel();
        request.request_id = self.region_tools.generation;
        let doc = self.engine.document();
        self.region_tools.target = Some(Target {
            generation: request.request_id,
            revision: doc.revision,
            layer: doc.working.occurrence,
            source:doc.working.target,
            owner:doc.owner,
            purpose,
        });
        self.region_tools.queued = Some(request);
    }
    pub(super) fn queue_selection(&mut self, selection: Selection, options: layer_render::SelectionRefinement) {
        let request = RegionRequest {
            enclosure: None, request_id: 0, contiguous: false, selection: Some(options),
            source: layer_render::RegionSource::Selection(std::sync::Arc::new(selection)),
            position: [0, 0], tolerance: 0., refinement: Default::default(), limit: None,
        };
        let basis = layer_core::Affine::IDENTITY;
        self.queue_region(request, Purpose::Region { operation: None, color: None, basis });
    }
    pub(super) fn queue_transform_selection(&mut self) -> bool {
        self.region_tools.cancel();
        let Some(request) = self.engine.transform_selection_request(0) else {
            return false;
        };
        self.queue_region(request, Purpose::Transform);
        true
    }
    pub(super) fn queue_tonal_region(&mut self, request: RegionRequest) {
        self.queue_region(request, Purpose::Tonal);
    }
    /// A selection refinement job, superseding any other pending result.
    pub(super) fn queue_refine_region(&mut self, request: RegionRequest) {
        self.queue_region(request, Purpose::Refine);
    }
    pub(super) fn poll_region_tool(&mut self) -> Result<u32, String> {
        if self.region_tools.pending
            && let Some(result) = self.engine.backend_mut().take_region()
        {
            self.region_tools.pending = false;
            let queued = self.region_tools.queued.is_some();
            let target = self.region_tools.target.take_if(|t| {
                result.as_ref().map_or(!queued, |result| t.generation == result.request_id)
            });
            let result = result.map_err(error)?;
            let doc = self.engine.document();
            if let Some(target) = target.filter(|t| doc.owner == t.owner && doc.revision == t.revision && doc.working.occurrence == t.layer && doc.working.target == t.source) {
                match target.purpose {
                    Purpose::Tonal => {
                        self.tonal_result(result)?;
                        return Ok(0);
                    }
                    Purpose::Refine => {
                        self.refine_result(result)?;
                        return Ok(0);
                    }
                    Purpose::Transform => {
                        let Err(cause) = self.apply_transform_selection(result.pixels) else {
                            return Ok(0);
                        };
                        self.set_host_error(Some(cause));
                        return Ok(regions::HOST);
                    }
                    Purpose::Region { operation, .. } if operation.is_some() && result.pixels.bounds() == [0; 4] => {
                        return Ok(0);
                    }
                    Purpose::Region { operation, color, basis } => {
                        let selection = Selection::pixels(result.pixels).transformed(basis).map_err(error)?;
                        if let Some(operation) = operation {
                            self.paint_operation(Some(selection), operation, color.as_slice())?;
                        } else {
                            self.set_mask_coverage(layer_core::SelectionTarget::Current,selection)?;
                        }
                    }
                }
            }
        }
        if !self.region_tools.pending
            && let Some(request) = self.region_tools.queued.as_ref()
            && self
                .engine
                .backend_mut()
                .request_region(request.clone())
                .map_err(error)?
        {
            self.region_tools.queued = None;
            self.region_tools.pending = true;
        }
        Ok(0)
    }
}
