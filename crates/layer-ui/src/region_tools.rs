//! Shared click-to-select/fill policy. Hosts forward input and render controls;
//! source choice, cancellation, stale-result handling and edits stay here.
use crate::localization::{Localizer, MessageId};
use super::*;
use layer_core::{Point, Selection, authored::{OccurrenceHandle,SceneScope,SourceTarget}};
use layer_render::RegionRequest;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillScope { #[default] Connected, Everywhere }

pub(super) struct RegionTools {
    pub scope: FillScope,
    pub tolerance: f32,
    pub refinement: layer_render::RegionRefinement,
    pub source: [RegionSource; 2],
    pub enclose_source: RegionSource,
    contact: Option<Point>,
    generation: u64,
    queued: Option<RegionRequest>,
    pending: bool,
    target: Option<Target>,
    filled_below: Option<OccurrenceHandle>,
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
            scope: FillScope::Connected,
            tolerance: 0.1,
            refinement: layer_render::RegionRefinement {
                smooth_edges: true,
                ..Default::default()
            },
            source: [RegionSource::Visible; 2],
            enclose_source: RegionSource::Reference,
            contact: None,
            generation: 0,
            queued: None,
            pending: false,
            target: None,
            filled_below: None,
        }
    }
}
impl RegionTools {
    pub(super) fn fields(&self) -> [(&'static str, MessageId, Option<MessageId>, NumericControl, f32); 3] {
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
                NumericControl::whole_pixels(0., distance),
                self.refinement.gap_closing as f32,
            ),
            (
                "expansion",
                MessageId::TOOL_CONTROL_EXPANSION,
                Some(MessageId::TOOL_CONTROL_GROUP_EDGES),
                NumericControl::whole_pixels(-distance, distance),
                self.refinement.expansion as f32,
            ),
        ]
    }
    pub fn controls(&self, localizer: &Localizer) -> Vec<ToolSetting> {
        self.fields().into_iter()
        .map(|(id, label, group, numeric, value)| ToolSetting { preview: false,
            id,
            label: localizer.text(label),
            label_id: label,
            group_id: group.map_or("", MessageId::key),
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
                self.request_region_at(point, None, None);
            }
            PenPhase::Cancel => self.region_tools.cancel(),
            _ => (),
        }
    }
    pub(super) fn enclose_fill(&mut self, enclosure: Selection) {
        self.request_region_at(Point { x: 0., y: 0. }, Some(enclosure), None);
    }
    fn fill_vector_layer(&mut self, layer: OccurrenceHandle, point: Point, source: layer_render::RegionSource, paths: bool) -> Result<(), String> {
        use layer_core::authored::vector::{PaintEntry, Record, Style};
        let doc = self.engine.document();
        if doc.is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
        let object = doc.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
        let [ox, oy] = doc.scene().occurrence_offset64(layer);
        let local = [f64::from(point.x) - ox, f64::from(point.y) - oy];
        let tolerance = 0.25 * self.document_per_surface_pixel();
        let fill = if self.state.colors.transparent() { None } else {
            self.vector_figure_style(FigurePaint::Fill, layer_core::authored::vector::Cap::Butt, layer_core::authored::vector::Join::Miter)?.entries.first().cloned()
        };
        let innermost = object.paths().filter(|(_, path)| paths && path.pickable() && path.encloses(local, tolerance))
            .map(|(position, path)| (path.area_around(local, tolerance), position, path)).min_by(|a, b| a.0.total_cmp(&b.0));
        let found = innermost.map(|(_, position, path)| {
            let mut entries: Vec<PaintEntry> = path.style.entries.iter().filter(|entry| !matches!(entry, PaintEntry::Fill(_))).cloned().collect();
            if let Some(fill) = fill { entries.insert(0, fill); }
            (*path.style.entries != entries[..]).then(|| {
                let restyled = Record::Path(path.with_style(std::sync::Arc::new(Style::new(entries))));
                doc.vector_object_edit(layer, object.replace([(position, Some(restyled))])).map_err(|e| e.to_string())
            }).transpose()
        }).transpose()?;
        if let Some(edit) = found { return edit.map_or(Ok(()), |edit| self.layer_edit(edit)); }
        if self.state.colors.transparent() { return Ok(()); }
        if !self.engine.backend().region_fits(doc.composition().size) { return Err(self.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).to_string()); }
        self.new_paint_layer_under()?;
        self.request_region_at(point, None, Some(source));
        self.region_tools.filled_below = Some(layer);
        Ok(())
    }
    fn region_source(&mut self, source: RegionSource, from: Option<layer_render::RegionSource>, source_target: Option<SourceTarget>) -> Option<layer_render::RegionSource> {
        let doc = self.engine.document();
        Some(match (from, source) {
                (Some(from), _) => from,
                (None, RegionSource::Visible) => layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::All},
                (None, RegionSource::Editing) => {
                    if let Some(target) = source_target {
                        layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::Raw(target)}
                    } else if let Some(handle) = doc.working.occurrence.filter(|handle| doc.scene().holds_objects(*handle)) {
                        layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::RawObjects(handle)}
                    } else {
                        let handle = doc.working.occurrence.filter(|handle| doc.scene().effect(*handle).is_some_and(|effect| effect.constant_color().is_some()))?;
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
                (None, RegionSource::Reference) => {
                    if doc.scene().references().is_empty() {
                        self.notify_missing_reference();
                        return None;
                    }
                    layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:doc.reference_scope()}
                }
            })
    }
    fn request_region_at(&mut self, point: Point, enclosure: Option<Selection>, from: Option<layer_render::RegionSource>) {
        let enclosed = enclosure.is_some();
        let Some((fill, source, contiguous)) = self.layer_interaction.tool.region() else {
            return;
        };
        let contiguous = contiguous && (enclosed || !fill || self.region_tools.scope == FillScope::Connected);
        let extent = self.engine.document().composition().size;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.
            || point.y < 0.
            || point.x >= extent[0] as f32
            || point.y >= extent[1] as f32
        {
            return;
        }
        let doc = self.engine.document();
        let mask_target = self.selection_masks.target().filter(|_| fill);
        let source_target=if mask_target.is_some() {
            self.selection_masks.artwork().and_then(|h|doc.scene().source_target(h))
        } else {doc.drawing_target().or(doc.active_target())};
        let Some(resolved) = self.region_source(source, from.clone(), source_target) else { return };
        if fill && from.is_none() && !enclosed && self.selection_masks.target().is_none() && let Some(layer) = self.editing_vector_layer() {
            if let Err(error) = self.fill_vector_layer(layer, point, resolved, source != RegionSource::Reference) { self.notify(error); }
            return;
        }
        if !self.engine.backend().region_fits(extent) {
            self.notify(self.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).to_string());
            return;
        }
        let source = resolved;
        let doc = self.engine.document();
        let basis = layer_core::Affine::IDENTITY;
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
            source,
            position: [point.x as u32, point.y as u32],
            tolerance: self.region_tools.tolerance,
            refinement: layer_render::RegionRefinement {
                gap_closing: if contiguous { self.region_tools.refinement.gap_closing } else { 0 },
                smooth_edges: if fill { self.region_tools.refinement.smooth_edges } else { self.selection_tools.options.antialias },
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
        if self.region_tools.pending && !self.engine.raster_backing_pending()
            && let Some(result) = self.engine.backend_mut().take_region()
        {
            self.region_tools.pending = false;
            let queued = self.region_tools.queued.is_some();
            let target = self.region_tools.target.take_if(|t| {
                result.as_ref().map_or(!queued, |result| t.generation == result.request_id)
            });
            let result = result.map_err(|cause| { if target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Transform)) { self.discard_mode_actions(); } error(cause) })?;
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
                        return match self.apply_transform_selection(result.pixels) {
                            Ok(changed) => Ok(changed),
                            Err(cause) => { self.set_host_error(Some(cause)); Ok(regions::HOST) },
                        };
                    }
                    Purpose::Region { operation, .. } if operation.is_some() && result.pixels.bounds() == [0; 4] => {
                        self.region_tools.filled_below = None;
                        return Ok(0);
                    }
                    Purpose::Region { operation, color, basis } => {
                        let selection = Selection::pixels(result.pixels).transformed(basis).map_err(error)?;
                        if let Some(operation) = operation {
                            self.paint_operation(Some(selection), operation, color.as_slice())?;
                            if let Some(vector) = self.region_tools.filled_below.take() { self.notify_filled_below(vector); }
                        } else {
                            self.set_mask_coverage(layer_core::SelectionTarget::Current,selection)?;
                        }
                    }
                }
            } else {
                self.region_tools.filled_below = None;
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
