//! Shared click-to-select/fill policy. Hosts forward input and render controls;
//! source choice, cancellation, stale-result handling and edits stay here.
use crate::localization::{Localizer, MessageId};
use super::*;
use layer_core::{Point, Selection, authored::{OccurrenceHandle,SceneScope,SourceTarget}};
use layer_render::RegionRequest;
#[path="fill_gesture.rs"]
mod gesture;
pub use gesture::{FillGestureView,FillPillView};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillScope { #[default] Connected, Everywhere }

pub(super) struct RegionTools {
    pub saved_values: std::collections::BTreeMap<String, f32>,
    pub scope: FillScope,
    pub stop_at_colors: bool,
    pub tolerance: f32,
    pub texture_strength: f32,
    pub refinement: layer_render::RegionRefinement,
    pub source: [RegionSource; 3],
    pub last_fill: Option<CommandId>,
    pub gap_reach: Option<u32>,
    gesture: Option<gesture::Gesture>,
    reveal: Option<gesture::Reveal>,
    preview_active: bool,
    pub(super) reduced_motion: bool,
    completed:Option<gesture::Completed>,
    rollback:Option<(u64,u64,layer_core::authored::PaintHandle,layer_core::authored::PaintSource)>,
    now_ns: u64,
    finger:Option<u64>,
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
            saved_values: Default::default(),
            scope: FillScope::Connected,
            stop_at_colors: true,
            tolerance: 0.1,
            texture_strength: 1.,
            refinement: layer_render::RegionRefinement {
                smooth_edges: true,
                ..Default::default()
            },
            source: [RegionSource::Auto; 3],
            last_fill: None,
            gap_reach: None,
            gesture: None,
            reveal: None,
            preview_active: false,
            reduced_motion: false,
            completed:None,rollback:None,
            now_ns: 0,
            finger:None,
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
        let distance = layer_render::RegionRefinement::MAX_EDGE_OFFSET as f64;
        [
            ("grain_depth", MessageId::TOOL_SETTING_GRAIN_DEPTH, None, tool_settings::texture_strength_control(), self.texture_strength),
            (
                "tolerance",
                MessageId::TOOL_CONTROL_TOLERANCE,
                None,
                NumericControl::percent(),
                self.tolerance,
            ),
            ("edge_offset", MessageId::TOOL_CONTROL_EDGE_OFFSET, Some(MessageId::TOOL_CONTROL_GROUP_EDGES),
                NumericControl::whole_pixels(-distance, distance),
                match self.refinement.edges { layer_render::RegionEdges::GrowShrink(value) => value as f32, _ => 0. }),
        ]
    }
    pub fn controls(&self, localizer: &Localizer) -> Vec<ToolSetting> {
        self.fields().into_iter()
        .filter(|field| field.0 != "edge_offset" || matches!(self.refinement.edges, layer_render::RegionEdges::GrowShrink(_)))
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
    pub fn restore_value(&mut self, id: &str, value: f32) -> Result<(), WorkspaceValidationError> {
        if matches!(id, "gap_closing" | "expansion" | "smoothing") && value.is_finite() {
            self.saved_values.insert(id.into(), value);
            Ok(())
        } else { self.edit_value(id, value) }
    }
    pub fn edit_value(&mut self, id: &str, value: f32) -> Result<(), WorkspaceValidationError> {
        let (_, label, _, numeric, _) = self.fields().into_iter().find(|field| field.0 == id).ok_or("Unknown region setting")?;
        numeric.validate(value, label)?;
        if id == "edge_offset" && value.fract() != 0. {
            return Err(NumericError::WholePixels { label: label.into() }.into());
        }
        self.cancel();
        match id {
            "tolerance" => self.tolerance = value,
            "grain_depth" => self.texture_strength = value,
            "edge_offset" => self.refinement.edges = layer_render::RegionEdges::GrowShrink(value as i32),
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        if let Some(gesture)=self.gesture.take() {
            if let Some((checkpoint,source))=gesture.amendment && let Some((handle,_))=gesture.before {self.rollback=Some((checkpoint,gesture.owner,handle,source));}
            self.tolerance=gesture.original_tolerance;self.gap_reach=gesture.original_reach;self.refinement.close_gaps=gesture.original_close;
        }
        self.reveal=None;
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
        self.pending || self.queued.is_some() || self.reveal.is_some() || self.rollback.is_some() || self.preview_active && self.gesture.is_none()
            || self.gesture.as_ref().is_some_and(|gesture|!gesture.released && !gesture.holding() && gesture.tool!=layer_engine::ToolKind::Finger)
    }
    pub fn publishing_edit(&self) -> bool {
        self.gesture.is_some() || self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Region { .. }))
    }
    pub fn applying_transform(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Transform))
    }
    pub fn refining(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Refine))
    }
    pub fn cancellable(&self) -> bool {
        self.gesture.is_some() || self.contact.is_some() || self.target.is_some()
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn region_pen(&mut self, event: PenEvent) {
        if self.fill_pen(event) {return;}
        match event.phase {
            PenPhase::Down => {
                self.region_tools.cancel();
                self.layer_interaction.fill_texture = matches!(self.layer_interaction.tool, LayerCanvasTool::Region { fill: true, .. }).then(|| self.capture_fill_texture());
                self.region_tools.contact = Some(
                    self.state
                        .camera
                        .input_transform()
                        .map(event.surface_position),
                );
            }
            PenPhase::Up => {
                if let Some(point) = self.region_tools.contact.take() { self.request_region_at(point, None, None); }
                self.layer_interaction.fill_texture = None;
            }
            PenPhase::Cancel => { self.region_tools.cancel(); self.layer_interaction.fill_texture = None; },
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
    pub(super) fn region_controls(&self) -> Vec<ToolOption> {
        let Some((fill, source, contiguous)) = self.layer_interaction.tool.region() else { return Vec::new(); };
        let mut controls = vec![ToolOption::Choice { id: "selection-source", label: self.localization().text(MessageId::TOOL_ACTION_GROUP_LINES_FROM),
            segmented: true, labeled: true, columns: None, beside: None,
            items: [CommandId::SelectionReference, CommandId::SelectionVisible, CommandId::SelectionEditing].into_iter()
                .map(|command| { let state = self.command(command); state.choice_item(state.label.clone()) }).collect() }];
        for (id, label, commands) in [
            ("close-gaps", MessageId::TOOL_CONTROL_CLOSE_GAPS, &[CommandId::CloseGapsAuto, CommandId::CloseGapsOff][..]),
            ("region-edges", MessageId::TOOL_CONTROL_GROUP_EDGES, &[CommandId::EdgesLineMiddle, CommandId::EdgesExact, CommandId::EdgesGrowShrink][..]),
        ] {
            if id == "close-gaps" && (!contiguous || fill && self.region_tools.scope == FillScope::Everywhere) { continue; }
            let beside=(id=="region-edges" && matches!(self.region_tools.refinement.edges,layer_render::RegionEdges::GrowShrink(_))).then_some("edge_offset");
            controls.push(ToolOption::Choice { id, label:self.localization().text(label), segmented:true, labeled:beside.is_none(), columns:None, beside,
                items:commands.iter().map(|command| {let state=self.command(*command);
                    let label=if matches!(self.layer_interaction.tool,LayerCanvasTool::Region {fill:true,..}) && *command==CommandId::CloseGapsAuto {self.region_tools.gap_reach.map(|reach|NumericControl::whole_pixels(0.,64.).compact_text(f64::from(reach)).into()).unwrap_or_else(||state.label.clone())}else{state.label.clone()};
                    state.choice_item(label)}).collect() });
        }
        if self.lines_from(source) != RegionSource::Reference { return controls; }
        let scene = self.engine.document().scene();
        let items = scene.order().iter().filter_map(|id| {
            let layer = scene.occurrence(*id)?;
            if !matches!(layer.kind(), LayerKind::Paint | LayerKind::Object | LayerKind::Group) { return None; }
            Some(ToolSetItem { label: layer.name.clone(), icon: "reference", enabled: true, disabled_reason: None,
                action: UiAction::Layer { action: LayerAction::Reference { id: occurrence_token(*id) } }, selected: layer.reference, preview: None })
        }).collect();
        controls.push(ToolOption::Checklist { id: "reference-layers", label: self.localization().text(MessageId::COMMAND_SELECTION_REFERENCE), items });
        controls
    }
    pub(super) fn lines_from(&self, source: RegionSource) -> RegionSource {
        if source != RegionSource::Auto { return source; }
        if self.engine.document().scene().references().is_empty() { RegionSource::Visible } else { RegionSource::Reference }
    }
    fn region_source(&mut self, source: RegionSource, from: Option<layer_render::RegionSource>, source_target: Option<SourceTarget>) -> Option<layer_render::RegionSource> {
        let doc = self.engine.document();
        let source = self.lines_from(source);
        let mut sources = match (from, source) {
                (Some(from), _) => return Some(from),
                (None, RegionSource::Auto) => unreachable!(),
                (None, RegionSource::Visible) => vec![layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::All}],
                (None, RegionSource::Editing) => {
                    if let Some(target) = source_target {
                        vec![layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::Raw(target)}]
                    } else if let Some(handle) = doc.working.occurrence.filter(|handle| doc.scene().holds_objects(*handle)) {
                        vec![layer_render::RegionSource::Scene {snapshot:doc.snapshot(),scope:SceneScope::RawObjects(handle)}]
                    } else {
                        let handle = doc.working.occurrence.filter(|handle| doc.scene().effect(*handle).is_some_and(|effect| effect.constant_color().is_some()))?;
                        let mut snapshot = doc.snapshot();
                        let occurrence = std::sync::Arc::make_mut(&mut snapshot).artwork.occurrences.get_mut(handle).unwrap();
                        occurrence.visible = true;
                        occurrence.opacity = 1.;
                        occurrence.blend = layer_core::LayerBlend::Normal;
                        occurrence.attachment = layer_core::Attachment::None;
                        occurrence.mask = None;
                        vec![layer_render::RegionSource::Scene {snapshot,scope:SceneScope::Members(vec![handle].into())}]
                    }
                }
                (None, RegionSource::Reference) => {
                    if doc.scene().reference_leaves().is_empty() {
                        self.notify_missing_reference();
                        return None;
                    }
                    let snapshot = doc.snapshot();
                    doc.scene().reference_leaves().into_iter().filter_map(|leaf| layer_render::RegionSource::reference(snapshot.clone(), leaf)).collect()
                }
            };
        if self.region_tools.stop_at_colors && source == RegionSource::Reference {
            let doc = self.engine.document();
            if let Some(target) = source_target.and_then(|target| doc.target_owner(target))
                && !doc.scene().reference_leaves().contains(&target)
                && let Some(target) = layer_render::RegionSource::reference(doc.snapshot(), target) { sources.push(target); }
        }
        if source == RegionSource::Reference { Some(layer_render::RegionSource::Intersection(sources)) }
        else { sources.pop() }
    }
    pub(super) fn prepare_region_tools(&mut self) {
        let tool=self.layer_interaction.tool;
        if tool.region().is_some_and(|(fill,_,_)|fill) {
            self.region_tools.last_fill=Some(if matches!(tool,LayerCanvasTool::EncloseFill {..}) {CommandId::EncloseFill}else{CommandId::Fill});
        }
        let (source,enclosed)=if let Some((_,source,_))=tool.region() {
            (source,matches!(tool,LayerCanvasTool::EncloseFill {..}))
        }else if self.region_tools.last_fill.is_some() || !self.engine.document().scene().references().is_empty() {
            let enclosed=self.region_tools.last_fill==Some(CommandId::EncloseFill);
            (self.region_tools.source[if enclosed {2}else{1}],enclosed)
        }else{self.engine.backend_mut().prepare_regions(None);return;};
        let source=self.lines_from(source);let doc=self.engine.document();
        if source==RegionSource::Reference && doc.scene().reference_leaves().is_empty() {self.engine.backend_mut().prepare_regions(None);return;}
        let target=doc.drawing_target().or(doc.active_target());
        let reference_count=(source==RegionSource::Reference).then(||doc.scene().reference_leaves().len());
        let paper=doc.nearest_paper_binding(doc.drawing_content());
        let Some(source)=self.region_source(source,None,target) else {return;};
        self.engine.backend_mut().prepare_regions(Some(&layer_render::RegionPreparation {
            source,reference_count,paper,tolerance:self.region_tools.tolerance,refinement:self.region_tools.refinement,
            enclosed,
        }));
    }
    fn request_region_at(&mut self, point: Point, enclosure: Option<Selection>, from: Option<layer_render::RegionSource>) {
        let enclosed = enclosure.is_some();
        let Some((fill, source, contiguous)) = self.layer_interaction.tool.region() else {
            return;
        };
        let source = self.lines_from(source);
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
        let request = RegionRequest { fill: (fill && !enclosed && self.region_tools.gesture.is_some()).then(||layer_render::RegionFillQuery {
                reach:self.region_tools.gap_reach.or_else(||(!self.region_tools.refinement.close_gaps).then_some(0)),excluded_seals:Default::default(),
                trail:self.fill_query_trail()}),
            reference_count: (self.lines_from(self.layer_interaction.tool.region().unwrap().1) == RegionSource::Reference).then(|| doc.scene().reference_leaves().len()),
            paper: doc.nearest_paper_binding(doc.drawing_content()),
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
                close_gaps: contiguous && self.region_tools.refinement.close_gaps,
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
            operation: fill.then(|| self.fill_region_operation(&request)),
            color: (fill
                && !self.state.colors.transparent()
                && self.state.brush.opacity > 0.)
                .then(|| self.state.colors.definition()),
            basis: if !fill && self.selection_refinement(basis).is_some() { layer_core::Affine::IDENTITY } else { basis },
        };
        self.queue_region(request, purpose);
    }
    fn fill_region_operation(&self,request:&RegionRequest)->layer_core::RasterOperationKind {
        let mut operation=self.fill_operation();
        if !matches!(request.refinement.edges,layer_render::RegionEdges::LineMiddle) {return operation;}
        let doc=self.engine.document();let scene=doc.scene();
        let Some(target)=doc.drawing_content().and_then(|target|scene.source_owner(target)) else{return operation;};
        let order=scene.order();let Some(index)=order.iter().position(|id|*id==target) else{return operation;};
        let mut leaves=std::collections::BTreeSet::new();
        for source in request.source.sources().iter().take(request.reference_count.unwrap_or(usize::MAX)) {
            match source {
                layer_render::RegionSource::Source(_)=>(),
                layer_render::RegionSource::Scene {scope:SceneScope::Raw(_),..}=>(),
                layer_render::RegionSource::Scene {scope:SceneScope::Members(members),..}=>leaves.extend(members.iter().copied()),
                _=>leaves.extend(order.iter().copied().filter(|id|scene.visible(*id) && scene.effect(*id).is_none())),
            }
        }
        let snapshot=doc.snapshot();
        let lines=leaves.into_iter().filter(|leaf|scene.visible(*leaf) && scene.occurrence(*leaf).is_some_and(|layer|matches!(layer.kind(),layer_core::LayerKind::Paint|layer_core::LayerKind::Object))
                && order.iter().position(|id|id==leaf).is_some_and(|position|position!=index))
            .filter_map(|leaf|match if request.reference_count.is_some() {layer_render::RegionSource::reference(snapshot.clone(),leaf)}
                else{Some(layer_render::RegionSource::Scene {snapshot:snapshot.clone(),scope:SceneScope::Members(vec![leaf].into())})} {
                Some(layer_render::RegionSource::Scene {snapshot,scope})=>Some(layer_core::FillLine {snapshot,scope,
                    fill_above:order.iter().position(|id|*id==leaf).unwrap()>index}),_=>None,
            }).collect();
        if let layer_core::RasterOperationKind::Fill {edges,..}=&mut operation {*edges=Some(Box::new(layer_core::FillEdges {
            snapshot,position:Point{x:request.position[0] as f32,y:request.position[1] as f32},tolerance:request.tolerance,lines,
        }));}
        operation
    }
    fn queue_region(&mut self, mut request: RegionRequest, mut purpose: Purpose) {
        if let Purpose::Region {operation:Some(layer_core::RasterOperationKind::Fill {edges:Some(edges),..}),..}=&mut purpose {
            edges.position=Point{x:request.position[0] as f32,y:request.position[1] as f32};
            edges.tolerance=request.tolerance;
        }
        let gesture=matches!(&purpose,Purpose::Region {operation:Some(_),..}).then(||self.region_tools.gesture.take()).flatten();
        let reveal=gesture.as_ref().and(self.region_tools.reveal.take());
        self.region_tools.cancel();
        self.region_tools.gesture=gesture;self.region_tools.reveal=reveal;
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
        if let Some(gesture)=&mut self.region_tools.gesture {gesture.request=Some(request.clone());}
        self.region_tools.queued = Some(request);
    }
    pub(super) fn queue_selection(&mut self, selection: Selection, options: layer_render::SelectionRefinement) {
        let request = RegionRequest { fill: None, reference_count: None, paper: None,
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
            let result = result.map_err(|cause| {
                if target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Transform)) { self.discard_mode_actions(); }
                if target.is_some() {self.region_tools.cancel();}
                error(cause)
            })?;
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
                    Purpose::Region { operation, .. } if operation.is_some() && result.pixels.bounds() == [0; 4] && self.region_tools.gesture.is_none() => {
                        self.region_tools.filled_below = None;
                        return Ok(0);
                    }
                    Purpose::Region {operation:Some(operation),color,basis} if self.region_tools.gesture.is_some()=> {
                        return self.accept_fill_result(result,operation,color,basis);
                    }
                    Purpose::Region { operation, color, basis } => {
                        let selection = Selection::pixels(result.pixels).transformed(basis).map_err(error)?;
                        if let Some(mut operation) = operation {
                            if let layer_core::RasterOperationKind::Fill { underfill, .. } = &mut operation {
                                *underfill = result.underfill.map(Selection::pixels).map(|selection| selection.transformed(basis)).transpose().map_err(error)?;
                            }
                            self.paint_operation(Some(selection), operation, color.as_slice())?;
                            if let Some(vector) = self.region_tools.filled_below.take() { self.notify_filled_below(vector); }
                        } else {
                            self.set_mask_coverage(layer_core::SelectionTarget::Current,selection)?;
                        }
                    }
                }
            } else {
                self.region_tools.filled_below = None;
                if !queued {self.region_tools.cancel();}
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
