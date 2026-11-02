//! Shared click-to-select/fill policy. Hosts forward input and render controls;
//! source choice, cancellation, stale-result handling and edits stay here.
use crate::localization::{Localizer, MessageId};
use super::*;
use layer_core::{Point, Selection, authored::{OccurrenceHandle,SceneScope,SourceTarget}};
use layer_render::{RegionRequest,RegionTask,RegionOutput};
use layer_core::authored::vector::{ContourTrace, PathGeometry, Style, Subpath};
#[path="fill_gesture.rs"]
mod gesture;
pub use gesture::{FillGestureView,FillPillView};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillScope { #[default] Connected, Everywhere }

pub(super) struct RegionTools {
    pub saved_values: std::collections::BTreeMap<String, f32>,
    pub scope: FillScope,
    pub fill_areas: layer_core::FillAreas,
    pub unfilled_size:f32,
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
    queued: Option<RegionTask>,
    pending: bool,
    target: Option<Target>,
    trace: Option<TraceJob>,
}
pub(super) struct EnclosurePaint {
    operation:layer_core::RasterOperationKind,
    color:Option<layer_core::color::RgbColor>,
    tolerance:f32,
    refinement:layer_render::RegionRefinement,
    areas:FillAreas,
    vector_style:Option<std::sync::Arc<Style>>,
}
struct Target {
    tool:LayerCanvasTool,
    generation: u64,
    revision: u64,
    layer: Option<OccurrenceHandle>,
    source: Option<SourceTarget>,
    owner: u64,
    purpose: Purpose,
}
struct TraceJob {
    target:Target,
    marching:Option<ContourTrace>,
    contours:std::collections::VecDeque<Vec<[f64;2]>>,
    fitting:Option<(layer_engine::PolylineFit,usize)>,
    subpaths:Vec<Subpath>,
}
impl Target {
    fn current(&self,doc:&layer_core::Document)->bool {
        doc.owner==self.owner && doc.revision==self.revision && doc.working.occurrence==self.layer && doc.working.target==self.source
    }
}
enum Purpose {
    FlatAreas,
    Trace {layer:OccurrenceHandle,style:std::sync::Arc<Style>,at_back:bool},
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
            fill_areas:Default::default(),unfilled_size:32.,
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
            trace: None,
        }
    }
}
impl RegionTools {
    pub(super) fn fields(&self) -> [(&'static str, MessageId, Option<MessageId>, NumericControl, f32); 4] {
        let distance = layer_render::RegionRefinement::MAX_EDGE_OFFSET as f64;
        [
            ("unfilled_size",MessageId::TOOL_CONTROL_SELECTION_BRUSH_SIZE,None,NumericControl::brush_size(),self.unfilled_size),
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
        self.reconfigure();
        match id {
            "unfilled_size" => self.unfilled_size=value,
            "tolerance" => self.tolerance = value,
            "grain_depth" => self.texture_strength = value,
            "edge_offset" => self.refinement.edges = layer_render::RegionEdges::GrowShrink(value as i32),
            _ => unreachable!(),
        }
        Ok(())
    }
    pub(super) fn reconfigure(&mut self) {
        if self.target.as_ref().or_else(||self.trace.as_ref().map(|job|&job.target)).is_none_or(|target|!matches!(target.tool,LayerCanvasTool::PaintUnfilled {..})) {self.cancel();}
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
        self.trace = None;
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
        self.trace.is_some() || self.pending || self.queued.is_some() || self.reveal.is_some() || self.rollback.is_some() || self.preview_active && self.gesture.is_none()
            || self.gesture.as_ref().is_some_and(|gesture|!gesture.released && !gesture.holding() && gesture.tool!=layer_engine::ToolKind::Finger)
    }
    pub fn publishing_edit(&self) -> bool {
        self.trace.is_some() || self.gesture.is_some() || self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Region { .. }|Purpose::Trace {..}|Purpose::FlatAreas))
    }
    pub fn applying_transform(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Transform))
    }
    pub fn refining(&self) -> bool {
        self.target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Refine))
    }
    pub fn cancellable(&self) -> bool {
        self.trace.is_some() || self.gesture.is_some() || self.contact.is_some() || self.target.is_some()
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
                let checkpoint = self.engine.checkpoint();
                if let Some(point) = self.region_tools.contact.take() { self.request_region_at(point, None, None, None); }
                self.finger.admitted_pen |= event.tool.is_pen() && (self.region_tools.target.is_some() || self.engine.checkpoint() != checkpoint);
                self.layer_interaction.fill_texture = None;
            }
            PenPhase::Cancel => { self.region_tools.cancel(); self.layer_interaction.fill_texture = None; },
            _ => (),
        }
    }
    pub(super) fn enclose_fill(&mut self, enclosure: Selection) {
        self.request_region_at(Point { x: 0., y: 0. }, Some(enclosure), None, None);
    }
    pub(super) fn capture_enclosure_paint(&self)->EnclosurePaint {
        EnclosurePaint {operation:self.fill_operation(),color:(!self.state.colors.transparent() && self.state.brush.opacity>0.).then(||self.state.colors.definition()),
            tolerance:self.region_tools.tolerance,refinement:self.region_tools.refinement,areas:self.region_tools.fill_areas,
            vector_style:self.editing_vector_layer().and_then(|_|self.vector_fill_style().ok())}
    }
    pub(super) fn enclose_painted_footprint(&mut self,enclosure:Selection,paint:EnclosurePaint) {
        self.request_region_at(Point{x:0.,y:0.},Some(enclosure),None,Some(paint));
    }
    pub(super) fn can_flat_all_areas(&self)->bool {
        let doc=self.engine.document();let scene=doc.scene();
        self.require_action_idle().is_ok() && scene.order().iter().any(|id|scene.occurrence(*id).is_some_and(|o|matches!(o.content,layer_core::authored::OccurrenceContent::Paint(_)|layer_core::authored::OccurrenceContent::Vector(_))))
            && doc.working.occurrence.and_then(|id|scene.parent(id)).is_none_or(|id|!doc.is_locked(id))
    }
    pub(super) fn flat_all_areas(&mut self)->Result<(),String> {
        self.require_action_idle()?;
        let doc=self.engine.document();let extent=doc.composition().size;
        let pages=extent.map(|n|u64::from(n.div_ceil(layer_core::raster::TILE_SIZE)));
        let bytes=doc.composition().color.paint_descriptor().byte_len([layer_core::raster::TILE_SIZE;2]).map(|bytes|bytes as u64);
        let fits=bytes.and_then(|bytes|pages[0].checked_mul(pages[1]).and_then(|count|count.checked_mul(bytes)))
            .is_some_and(|bytes|bytes<=layer_core::raster::MAX_PUBLICATION_BYTES);
        if !fits || !self.engine.backend().region_fits(extent) {return Err(self.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).to_string());}
        let source=self.lines_from(self.region_tools.source[1]);
        let target=doc.working.occurrence.and_then(|id|doc.scene().source_target(id));
        let reference_count=(source==RegionSource::Reference).then(||doc.scene().reference_leaves().len());
        let paper=doc.nearest_paper_binding(target);
        let Some(source)=self.region_source(source,None,target,false) else{return Ok(());};
        let reach=self.region_tools.gap_reach.or_else(||(!self.region_tools.refinement.close_gaps).then_some(0)).unwrap_or(16);
        let request=RegionRequest {fill_areas:Some(FillAreas::TransparentOrWhite),colour_mode:Default::default(),
            fill:Some(layer_render::RegionFillQuery {reach:Some(reach),..Default::default()}),reference_count,paper,
            enclosure:None,contiguous:false,selection:None,request_id:0,source,position:[0;2],tolerance:self.region_tools.tolerance,
            refinement:layer_render::RegionRefinement {close_gaps:reach>0,edges:layer_render::RegionEdges::LineMiddle,smooth_edges:false},limit:None};
        self.queue_region_task(RegionTask::FlatAreas(request),Purpose::FlatAreas);Ok(())
    }
    fn insert_flat_areas(&mut self,raster:layer_core::raster::RasterRevision)->Result<(),String> {
        use layer_core::{Edit,authored::{Occurrence,OccurrenceContent,PaintSource,RecordChange}};
        let (parent,stack,index,offset)=self.new_layer_insertion(crate::session::art_layers::NewPlacement::Below)?;
        let doc=self.engine.document();let mut entries=doc.artwork.stacks.get(stack).unwrap().clone();
        let paint=RecordChange::insert(&doc.artwork.paint,PaintSource {color_mode:Default::default(),domain:doc.composition().size,raster,base:None,operations:Default::default()});
        let target=SourceTarget::Paint(paint.handle);
        let mut occurrence=Occurrence::new(OccurrenceContent::Paint(paint.handle),self.localization().text(MessageId::DOCUMENTS_FLATS_NAME));
        occurrence.offset=offset;
        let occurrence=RecordChange::insert(&doc.artwork.occurrences,occurrence);let id=occurrence.handle;
        entries.entries.insert(index,id);let mut working=doc.working.clone();
        working.occurrence=Some(id);working.target=Some(target);working.inspect_mask=None;
        working.layer_selection=std::collections::BTreeSet::from([id]);working.layer_anchor=Some(id);
        self.layer_edit(Edit::Batch(vec![Edit::Paint(paint),Edit::Occurrence(occurrence),
            Edit::Stack(RecordChange::replace(&doc.artwork.stacks,stack,Some(entries)).map_err(error)?),Edit::Working(working)]))?;
        if let Some(parent)=parent {self.layer_interaction.collapsed.remove(&parent);}Ok(())
    }
    fn vector_fill_style(&self)->Result<std::sync::Arc<Style>,String> {
        self.vector_figure_style(FigurePaint::Fill,layer_core::authored::vector::Cap::Butt,layer_core::authored::vector::Join::Miter).map(std::sync::Arc::new)
    }
    fn vector_line_sources(&self,layer:OccurrenceHandle,source:RegionSource)->(bool,Vec<layer_core::authored::vector::FaceLine<'_>>) {
        let doc=self.engine.document();let scene=doc.scene();
        let layers=match source {
            RegionSource::Editing=>vec![layer],RegionSource::Reference=>scene.reference_leaves(),
            _=>scene.order().iter().copied().filter(|id|scene.visible(*id)).collect(),
        };
        let raster=layers.iter().any(|id|scene.vector_layer(*id).is_none()
            && scene.occurrence(*id).is_some_and(|o|matches!(o.kind(),LayerKind::Paint|LayerKind::Object)));
        let lines=layers.iter().flat_map(|id|scene.vector_layer(*id).into_iter().flat_map(move |object|object.paths()
            .map(move |(_,path)|layer_core::authored::vector::FaceLine {path,offset:scene.occurrence_offset64(*id)})))
            .filter(|line|!line.path.hidden && line.path.style.strokes().next().is_some()).collect();
        (raster,lines)
    }
    fn insert_vector_fills(&mut self,layer:OccurrenceHandle,geometries:Vec<PathGeometry>,style:std::sync::Arc<Style>,at_back:bool)->Result<(),String> {
        use layer_core::authored::vector::{FillRule,PathItem,Record};
        if geometries.is_empty() {return Ok(());}
        let doc=self.engine.document();
        if doc.is_locked(layer) {return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string());}
        let [ox,oy]=doc.scene().occurrence_offset64(layer);
        let mut object=doc.scene().vector_layer(layer).ok_or("The vector layer was removed")?.clone();
        for geometry in geometries {
            let geometry=geometry.transformed(|p|[p[0]-ox,p[1]-oy]).map_err(str::to_string)?;
            let hull=geometry.hull();let position=(!at_back).then(||object.paths().filter(|(_,path)|path.style.strokes().next().is_some()).filter(|(_,path)| {
                let b=path.visual_bounds();b[0]<=hull[2] && b[2]>=hull[0] && b[1]<=hull[3] && b[3]>=hull[1]
            }).map(|(position,_)|position).max()).flatten();
            let mut path=PathItem::new(object.allocate_id(),std::sync::Arc::new(geometry),style.clone());
            path.fill_rule=FillRule::EvenOdd;
            let record=Record::Path(path);
            object=if let Some(position)=position {object.splice([(position,vec![object.record(position).unwrap().clone(),record])].into())}
                else{object.insert_back(vec![record])};
        }
        let edit=doc.vector_object_edit(layer,object).map_err(|e|e.to_string())?;
        self.layer_edit(edit)
    }
    fn enclose_vector_faces(&mut self,layer:OccurrenceHandle,enclosure:&Selection,source:RegionSource,style:std::sync::Arc<Style>)->Result<bool,String> {
        let (raster,lines)=self.vector_line_sources(layer,source);if raster {return Ok(false);}
        let bounds=enclosure.coverage_bounds();
        let faces=layer_core::authored::vector::vector_faces(&lines,[f64::from(bounds.min.x),f64::from(bounds.min.y),f64::from(bounds.max.x),f64::from(bounds.max.y)]);
        let doc=self.engine.document();let [ox,oy]=doc.scene().occurrence_offset64(layer);let object=doc.scene().vector_layer(layer).unwrap();
        let geometries=faces.into_iter().filter(|face|face.geometry.subpaths()[0].anchors().all(|a| {
            let p=a.point();enclosure.coverage_at(Point{x:p[0] as f32,y:p[1] as f32})>=0.5
        })).filter(|face| {
            let hull=face.geometry.hull();!object.paths().any(|(_,path)|path.style.strokes().next().is_none() && !path.style.entries.is_empty() && {
                let b=path.geometry.hull();[b[0]+ox,b[1]+oy,b[2]+ox,b[3]+oy].into_iter().zip(hull).all(|(a,b)|(a-b).abs()<=0.5)
            })
        }).map(|face|face.geometry).collect();
        self.insert_vector_fills(layer,geometries,style,false)?;Ok(true)
    }
    fn advance_vector_trace(&mut self)->Result<u32,String> {
        let Some(mut job)=self.region_tools.trace.take() else{return Ok(0);};
        if !job.target.current(self.engine.document()) {self.region_tools.cancel();return Ok(0);}
        if let Some(trace)=&mut job.marching {
            if let Some(contours)=trace.advance(262_144) {job.contours=contours.into();job.marching=None;}
            self.region_tools.trace=Some(job);return Ok(0);
        }
        let mut budget=20_000;
        while budget>0 {
            if job.fitting.is_none() {
                let Some(points)=job.contours.pop_front() else{break;};
                let count=points.len();job.fitting=Some((layer_engine::PolylineFit::new(points,0.25,true),count));
            }
            let (fit,count)=job.fitting.as_mut().unwrap();
            if !fit.advance(budget) {self.region_tools.trace=Some(job);return Ok(0);}
            budget=budget.saturating_sub(*count);
            if let Some(path)=fit.take() {job.subpaths.push(path);}
            job.fitting=None;
        }
        if !job.contours.is_empty() {self.region_tools.trace=Some(job);return Ok(0);}
        if !job.subpaths.is_empty() && let Purpose::Trace {layer,style,at_back}=job.target.purpose {
            let geometry=PathGeometry::new(job.subpaths).map_err(str::to_string)?;
            self.insert_vector_fills(layer,vec![geometry],style,at_back)?;return Ok(regions::DOCUMENT);
        }
        Ok(0)
    }
    fn fill_vector_layer(&mut self, layer: OccurrenceHandle, point: Point, source: layer_render::RegionSource, lines_from: RegionSource) -> Result<(), String> {
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
        let innermost = object.paths().filter(|(_, path)| (lines_from!=RegionSource::Reference || path.style.strokes().next().is_none()) && path.pickable() && path.encloses(local, tolerance))
            .map(|(position, path)| (path.area_around(local, tolerance), position, path)).min_by(|a, b| a.0.total_cmp(&b.0)
                .then_with(|| b.2.style.entries.iter().any(|e|matches!(e,PaintEntry::Fill(_))).cmp(&a.2.style.entries.iter().any(|e|matches!(e,PaintEntry::Fill(_))))));
        let found = innermost.map(|(_, position, path)| {
            let paints=path.style.entries.iter().filter_map(|entry|match entry {PaintEntry::Fill(fill)=>Some(&fill.paint),_=>None}).collect::<Vec<_>>();
            let everywhere=self.region_tools.scope==FillScope::Everywhere && !paints.is_empty();
            let changes=object.paths().filter(|(at,candidate)|*at==position || everywhere && candidate.style.entries.iter()
                .filter_map(|entry|match entry {PaintEntry::Fill(fill)=>Some(&fill.paint),_=>None}).eq(paints.iter().copied())).filter_map(|(at,candidate)| {
                let mut entries:Vec<_>=candidate.style.entries.iter().filter(|entry|!matches!(entry,PaintEntry::Fill(_))).cloned().collect();
                if let Some(fill)=fill.clone() {entries.insert(0,fill);}
                (*candidate.style.entries!=entries[..]).then(||(at,(!entries.is_empty()).then(||Record::Path(candidate.with_style(std::sync::Arc::new(Style::new(entries)))))))
            }).collect::<Vec<_>>();
            (!changes.is_empty()).then(||doc.vector_object_edit(layer,object.replace(changes)).map_err(|e|e.to_string())).transpose()
        }).transpose()?;
        if let Some(edit) = found { return edit.map_or(Ok(()), |edit| self.layer_edit(edit)); }
        if self.state.colors.transparent() { return Ok(()); }
        let (raster,lines)=self.vector_line_sources(layer,lines_from);
        if let Some(face)=layer_core::authored::vector::vector_face(&lines,[f64::from(point.x),f64::from(point.y)],
            self.region_tools.gap_reach.or_else(||(!self.region_tools.refinement.close_gaps).then_some(0))) {
            return self.insert_vector_fills(layer,vec![face.geometry],std::sync::Arc::new(Style::new(fill.into_iter().collect())),false);
        }
        if !raster {return Ok(());}
        if !self.engine.backend().region_fits(doc.composition().size) { return Err(self.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).to_string()); }
        self.request_region_at(point, None, Some(source), None);
        Ok(())
    }
    pub(super) fn region_controls(&self) -> Vec<ToolOption> {
        let Some((fill, source, contiguous)) = self.layer_interaction.tool.region() else { return Vec::new(); };
        let mut controls = vec![ToolOption::Choice { id: "selection-source", label: self.localization().text(MessageId::TOOL_ACTION_GROUP_LINES_FROM),
            segmented: true, labeled: true, columns: None, beside: None,
            items: [CommandId::SelectionReference, CommandId::SelectionVisible, CommandId::SelectionEditing].into_iter()
                .map(|command| { let state = self.command(command); state.choice_item(state.label.clone()) }).collect() }];
        if self.layer_interaction.tool.encloses() {
            controls.push(ToolOption::Choice {id:"fill-areas",label:self.localization().text(MessageId::TOOL_ACTION_GROUP_FILL_AREAS),segmented:true,labeled:true,columns:None,beside:None,items:[CommandId::FillAreasTransparent,CommandId::FillAreasTransparentOrWhite,CommandId::FillAreasAnyColour].into_iter().map(|command|{let state=self.command(command);state.choice_item(state.label.clone())}).collect()});
        }
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
    fn region_source(&mut self, source: RegionSource, from: Option<layer_render::RegionSource>, source_target: Option<SourceTarget>,include_stop:bool) -> Option<layer_render::RegionSource> {
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
        if include_stop && self.region_tools.stop_at_colors && source == RegionSource::Reference && !self.layer_interaction.tool.encloses() {
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
            self.region_tools.last_fill=Some(if tool.encloses() {CommandId::EncloseFill}else{CommandId::Fill});
        }
        let (source,enclosed)=if let Some((_,source,_))=tool.region() {
            (source,tool.encloses())
        }else if self.region_tools.last_fill.is_some() || !self.engine.document().scene().references().is_empty() {
            let enclosed=self.region_tools.last_fill==Some(CommandId::EncloseFill);
            (self.region_tools.source[if enclosed {2}else{1}],enclosed)
        }else{self.engine.backend_mut().prepare_regions(None);return;};
        let source=self.lines_from(source);let doc=self.engine.document();
        if source==RegionSource::Reference && doc.scene().reference_leaves().is_empty() {self.engine.backend_mut().prepare_regions(None);return;}
        let target=doc.drawing_target().or(doc.active_target());
        let reference_count=(source==RegionSource::Reference).then(||doc.scene().reference_leaves().len());
        let paper=doc.nearest_paper_binding(doc.drawing_content());
        let Some(source)=self.region_source(source,None,target,true) else {return;};
        self.engine.backend_mut().prepare_regions(Some(&layer_render::RegionPreparation {
            source,reference_count,paper,tolerance:self.region_tools.tolerance,refinement:self.region_tools.refinement,
            enclosed,
        }));
    }
    fn request_region_at(&mut self, point: Point, enclosure: Option<Selection>, from: Option<layer_render::RegionSource>, paint:Option<EnclosurePaint>) {
        let tolerance=paint.as_ref().map_or(self.region_tools.tolerance,|p|p.tolerance);
        let refinement=paint.as_ref().map_or(self.region_tools.refinement,|p|p.refinement);
        let areas=paint.as_ref().map_or(self.region_tools.fill_areas,|p|p.areas);
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
        let Some(mut resolved) = self.region_source(source, from.clone(), source_target,true) else { return };
        let reference=source==RegionSource::Reference;
        let mut reference_count=reference.then(||self.engine.document().scene().reference_leaves().len());
        let vector=(fill && mask_target.is_none()).then(||self.editing_vector_layer()).flatten();
        if let Some(layer)=vector && from.is_none() && !enclosed {
            if let Err(error)=self.fill_vector_layer(layer,point,resolved,source) {self.notify(error);}
            return;
        }
        let trace_style=if let Some(layer)=vector {
            if self.engine.document().is_locked(layer) {self.notify_drawing_refusal();return;}
            let style=if let Some(paint)=&paint {paint.vector_style.clone()}else if self.state.colors.transparent() {None}else {
                match self.vector_fill_style() {Ok(style)=>Some(style),Err(cause)=>{self.notify(cause);return;}}
            };
            let Some(style)=style else{return;};
            if let Some(enclosure)=&enclosure {
                match self.enclose_vector_faces(layer,enclosure,source,style.clone()) {
                    Ok(true)=>return,Ok(false)=>(),Err(cause)=>{self.notify(cause);return;}
                }
            }
            Some(style)
        }else{None};
        let fill_areas=(enclosed && fill && areas!=FillAreas::AnyColour).then_some(areas);
        if fill_areas.is_some() {
            let scope=if let Some(target)=source_target {SceneScope::Raw(target)}else if let Some(layer)=vector {SceneScope::RawObjects(layer)}else{return;};
            let mut sources=resolved.sources().to_vec();reference_count=Some(sources.len());
            sources.push(layer_render::RegionSource::Scene {snapshot:self.engine.document().snapshot(),scope});
            resolved=layer_render::RegionSource::Intersection(sources);
        }
        if !self.engine.backend().region_fits(extent) {
            self.notify(self.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).to_string());
            return;
        }
        let colour_mode=if from.is_some() {layer_render::RegionColourMode::AlphaBorder}else{match source {
            RegionSource::Editing=>layer_render::RegionColourMode::ColourOnSolid,
            RegionSource::Visible=>source_target.map_or(layer_render::RegionColourMode::AlphaBorder,layer_render::RegionColourMode::Target),
            _=>layer_render::RegionColourMode::AlphaBorder,
        }};
        let source = resolved;
        let doc = self.engine.document();
        let basis = layer_core::Affine::IDENTITY;
        if fill && mask_target.is_none() && trace_style.is_none() && doc.drawing_content().is_none() {
            self.notify_drawing_refusal();
            return;
        }
        let request = RegionRequest { fill_areas, colour_mode, fill: (fill && !enclosed && self.region_tools.gesture.is_some()).then(||layer_render::RegionFillQuery {
                reach:self.region_tools.gap_reach.or_else(||(!self.region_tools.refinement.close_gaps).then_some(0)),excluded_seals:Default::default(),
                trail:self.fill_query_trail()}),
            reference_count,
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
            tolerance,
            refinement: layer_render::RegionRefinement {
                close_gaps: contiguous && refinement.close_gaps,
                smooth_edges: if fill { refinement.smooth_edges } else { self.selection_tools.options.antialias },
                ..refinement
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
        if let Some(layer)=vector && let Some(style)=trace_style {
            self.queue_region(request,Purpose::Trace {layer,style,at_back:true});return;
        }
        let (operation,color)=if let Some(paint)=paint {(Some(self.fill_region_operation(&request,paint.operation,reference)),paint.color)}else{(
            fill.then(|| self.fill_region_operation(&request,self.fill_operation(),reference)),
            (fill
                && !self.state.colors.transparent()
                && self.state.brush.opacity > 0.)
                .then(|| self.state.colors.definition()))};
        let purpose = Purpose::Region {operation,color,
            basis: if !fill && self.selection_refinement(basis).is_some() { layer_core::Affine::IDENTITY } else { basis },
        };
        self.queue_region(request, purpose);
    }
    fn fill_region_operation(&self,request:&RegionRequest,mut operation:layer_core::RasterOperationKind,reference:bool)->layer_core::RasterOperationKind {
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
            .filter_map(|leaf|match if reference {layer_render::RegionSource::reference(snapshot.clone(),leaf)}
                else{Some(layer_render::RegionSource::Scene {snapshot:snapshot.clone(),scope:SceneScope::Members(vec![leaf].into())})} {
                Some(layer_render::RegionSource::Scene {snapshot,scope})=>Some(layer_core::FillLine {snapshot,scope,
                    fill_above:order.iter().position(|id|*id==leaf).unwrap()>index}),_=>None,
            }).collect();
        if let layer_core::RasterOperationKind::Fill {edges,..}=&mut operation {*edges=Some(Box::new(layer_core::FillEdges { anchors: None,
            snapshot,position:Point{x:request.position[0] as f32,y:request.position[1] as f32},tolerance:request.tolerance,lines,
        }));}
        operation
    }
    fn queue_region(&mut self,request:RegionRequest,purpose:Purpose) {self.queue_region_task(RegionTask::Mask(request),purpose);}
    fn queue_region_task(&mut self,mut task:RegionTask,mut purpose:Purpose) {
        let request=match &mut task {RegionTask::Mask(request)|RegionTask::FlatAreas(request)=>request};
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
            tool:self.layer_interaction.tool,
            generation: request.request_id,
            revision: doc.revision,
            layer: doc.working.occurrence,
            source:doc.working.target,
            owner:doc.owner,
            purpose,
        });
        if let Some(gesture)=&mut self.region_tools.gesture {gesture.request=Some(request.clone());}
        self.region_tools.queued = Some(task);
    }
    pub(super) fn queue_selection(&mut self, selection: Selection, options: layer_render::SelectionRefinement) {
        let request = RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,
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
        if self.region_tools.trace.is_some() {return self.advance_vector_trace();}
        if self.region_tools.pending && !self.engine.raster_backing_pending()
            && let Some(result) = self.engine.backend_mut().take_region()
        {
            self.region_tools.pending = false;
            let queued = self.region_tools.queued.is_some();
            let target = self.region_tools.target.take_if(|t| {
                result.as_ref().map_or(!queued, |result| t.generation == result.request_id())
            });
            let result = result.map_err(|cause| {
                if target.as_ref().is_some_and(|t| matches!(t.purpose, Purpose::Transform)) { self.discard_mode_actions(); }
                if target.is_some() {self.region_tools.cancel();}
                error(cause)
            })?;
            let doc = self.engine.document();
            if let Some(target) = target.filter(|t| t.current(doc)) {
                let result=match result {
                    RegionOutput::FlatAreas {raster,..}=> {
                        if !matches!(target.purpose,Purpose::FlatAreas) {return Err("Unexpected region result".into());}
                        self.insert_flat_areas(raster)?;return Ok(regions::DOCUMENT|regions::COMMANDS);
                    }
                    RegionOutput::Mask(result)=>result,
                };
                match target.purpose {
                    Purpose::FlatAreas=>return Err("Unexpected region result".into()),
                    Purpose::Trace {..}=> {
                        self.region_tools.trace=Some(TraceJob {target,marching:Some(ContourTrace::new((*result.pixels).clone())),contours:Default::default(),fitting:None,subpaths:Vec::new()});
                        return Ok(0);
                    }
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
                        return Ok(0);
                    }
                    Purpose::Region {operation:Some(operation),color,basis} if self.region_tools.gesture.is_some()=> {
                        return self.accept_fill_result(result,operation,color,basis);
                    }
                    Purpose::Region { operation, color, basis } => {
                        let selection = Selection::pixels(result.pixels).transformed(basis).map_err(error)?;
                        if let Some(mut operation) = operation {
                            if let layer_core::RasterOperationKind::Fill { underfill, edges, .. } = &mut operation {
                                if let Some(edges)=edges {edges.anchors=result.anchors.clone();}
                                *underfill = result.underfill.map(Selection::pixels).map(|selection| selection.transformed(basis)).transpose().map_err(error)?;
                            }
                            self.paint_operation(Some(selection), operation, color.as_slice())?;
                        } else {
                            self.set_mask_coverage(layer_core::SelectionTarget::Current,selection)?;
                        }
                    }
                }
            } else {
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
