use super::*;
use layer_engine::ToolKind;
use layer_core::authored::vector::{VectorFace,VectorObject,SealId,ItemId,PaintEntry,Paint,Record,PathItem,FillRule};

#[derive(Clone,Debug,PartialEq,Serialize)]
pub struct FillPillView {
    pub bounds:Bounds,
    pub label:std::sync::Arc<str>,
    pub value:std::sync::Arc<str>,
    pub track:[f32;2],
    pub fill_end:f32,
}
#[derive(Clone,Debug,PartialEq,Serialize)]
pub struct FillGestureView {
    pub transaction:u64,
    pub tip:[f32;2],
    pub ring:Option<f32>,
    pub pill:Option<FillPillView>,
    pub seals:std::sync::Arc<[[[f32;2];2]]>,
    pub tick:u64,
    pub resize_cursor:bool,
}
struct Hold {
    line:bool,
    slot:usize,
    anchor:f32,
    fraction:f32,
    left:f32,
    right:f32,
    caption:std::sync::Arc<str>,
    label:std::sync::Arc<str>,
}
type GapView=(bool,usize,Vec<[[f32;2];2]>);
impl Gesture {
    pub(in crate::session) fn holding(&self)->bool {self.hold.is_some()}
    fn gap_view(&self)->Option<GapView> {
        if let Some(vector)=&self.vector {return Some((true,vector.active,vector.slots[vector.active].as_ref().and_then(|face|face.as_ref())
            .into_iter().flat_map(|face|face.seals.iter()).map(|(a,b,_)|[a.map(|v|v as f32),b.map(|v|v as f32)]).collect()));}
        self.ready.as_ref().map(|ready|ready.result.gaps.as_ref().map_or((false,0,Vec::new()),|masks| {
            let seals=masks.seals.iter().flat_map(seal_segments).collect();
            (masks.line_bounded,masks.active,seals)
        }))
    }
}
pub(super) enum Rollback {
    Paint(layer_core::authored::PaintHandle,layer_core::authored::PaintSource),
    Vector(OccurrenceHandle,VectorObject),
}
pub(super) struct VectorFill {
    pub layer:OccurrenceHandle,
    source:RegionSource,
    point:[f64;2],
    pub original:VectorObject,
    pub checkpoint:u64,
    style:std::sync::Arc<Style>,
    preview:std::sync::Arc<Style>,
    slots:[Option<Option<VectorFace>>;5],
    active:usize,
    probed:[f32;2],
    faces:Vec<VectorFace>,
}
pub(super) struct VectorCompleted {
    checkpoint:u64,
    owner:u64,
    layer:OccurrenceHandle,
    source:RegionSource,
    path:ItemId,
    point:[f64;2],
    reach:u32,
    excluded:Vec<SealId>,
    seals:Vec<([f64;2],[f64;2],SealId)>,
}
pub(super) struct Gesture {
    pub transaction:u64,
    pub(super) owner:u64,
    device:u64,
    pub started:u64,
    pub start:Point,
    pub point:Point,
    pub tool:ToolKind,
    touch:Option<u64>,
    admitted:bool,
    pub released:bool,
    hold:Option<Hold>,
    waiting:bool,
    tick:u64,
    pub dragging:bool,
    pub original_tolerance:f32,
    pub original_reach:Option<u32>,
    pub original_close:bool,
    pub vector:Option<VectorFill>,
    pub request:Option<RegionRequest>,
    ready:Option<Ready>,
    revision:u64,
    path:Vec<[f32;2]>,
    pub(super) before:Option<(layer_core::authored::PaintHandle,layer_core::authored::PaintSource)>,
    pub(super) amendment:Option<(u64,layer_core::authored::PaintSource)>,
}
pub(super) struct Completed {
    checkpoint:u64,
    owner:u64,
    before:(layer_core::authored::PaintHandle,layer_core::authored::PaintSource),
    request:RegionRequest,
    ready:Ready,
}
struct Ready {
    result:layer_render::RegionResult,
    operation:layer_core::RasterOperationKind,
    color:Option<layer_core::color::RgbColor>,
    basis:layer_core::Affine,
}
pub(super) struct Reveal {
    transaction:u64,
    center:[f32;2],
    radius:f32,
    started:Option<u64>,
    duration:u64,
}
impl<R:CanvasRenderer> UiSession<R> {
    pub(in crate::session) fn fill_waits_for_commit(&self)->bool {self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.released)}
    pub(in crate::session) fn settled_fill_options(&self)->(f32,Option<u32>,bool) {
        self.region_tools.gesture.as_ref().map_or((self.region_tools.tolerance,self.region_tools.gap_reach,self.region_tools.refinement.close_gaps),
            |gesture|(gesture.original_tolerance,gesture.original_reach,gesture.original_close))
    }
    pub(in crate::session) fn uses_fill_gesture(&self)->bool {
        matches!(self.layer_interaction.tool,LayerCanvasTool::Region {fill:true,..})
            && self.selection_masks.target().is_none() && self.editing_vector_layer().is_none_or(|layer|!self.vector_line_sources(layer,self.lines_from(self.region_tools.source[1])).0)
    }
    pub(in crate::session) fn fill_pen(&mut self,event:PenEvent)->bool {
        if !self.uses_fill_gesture() || event.phase!=PenPhase::Down && self.region_tools.contact.is_some() {return false;}
        if event.phase!=PenPhase::Down && self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.device!=event.device_id){return true;}
        if matches!(event.phase,PenPhase::Move|PenPhase::Up) && self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.touch.is_some()
            && (event.surface_position.x-gesture.start.x).hypot(event.surface_position.y-gesture.start.y)>self.interaction.touch_policy.slop) {
            self.region_tools.cancel();self.clear_fill_preview();self.layer_interaction.fill_texture=None;return true;
        }
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        match event.phase {
            PenPhase::Down=> {
                self.region_tools.cancel();self.clear_fill_preview();
                let point=self.state.camera.input_transform().map(event.surface_position);
                if let Some(layer)=self.editing_vector_layer() {
                    return match self.begin_vector_fill(layer,point,event) {
                        Ok(tap)=>!tap,Err(cause)=>{self.notify(cause);self.region_tools.cancel();self.clear_fill_preview();true},
                    };
                }
                let previous=self.region_tools.completed.take().filter(|completed|completed.checkpoint==self.engine.checkpoint()
                    && completed.owner==self.engine.document().owner
                    && self.engine.document().drawing_content()==Some(layer_core::SourceTarget::Paint(completed.before.0)));
                let hit=previous.as_ref().and_then(|completed| {
                    let masks=completed.ready.result.gaps.as_ref()?;let limit=6.*scale/self.state.camera.zoom;
                    let ids=masks.seals.iter().filter(|seal|seal_distance([point.x,point.y],seal)<=limit).map(|seal|seal.id).collect::<Vec<_>>();
                    (!ids.is_empty()).then_some(ids)
                });
                let before=self.engine.document().drawing_content().and_then(|target|if let layer_core::SourceTarget::Paint(handle)=target {
                    self.engine.document().artwork.paint.get(handle).cloned().map(|source|(handle,source))
                }else{None});
                self.layer_interaction.fill_texture=Some(self.capture_fill_texture());
                let transaction=self.region_tools.generation;
                self.region_tools.gesture=Some(Gesture {transaction,owner:self.engine.document().owner,device:event.device_id,started:event.timestamp_ns,start:event.surface_position,point:event.surface_position,
                    tool:event.tool,touch:(event.tool==ToolKind::Finger).then_some(event.device_id),admitted:event.tool!=ToolKind::Finger,released:false,hold:None,waiting:true,tick:0,dragging:false,original_tolerance:self.region_tools.tolerance,
                    original_reach:self.region_tools.gap_reach,original_close:self.region_tools.refinement.close_gaps,vector:None,request:None,ready:None,revision:0,
                    path:vec![[point.x,point.y]],before,amendment:None});
                if let Some(ids)=hit && let Some(completed)=previous {
                    let after=self.region_tools.gesture.as_ref().unwrap().before.as_ref().unwrap().1.clone();
                    let changed=layer_core::RecordChange::replace(&self.engine.document().artwork.paint,completed.before.0,Some(completed.before.1.clone()));
                    if let Err(cause)=changed.map(layer_core::Edit::Paint).map_err(str::to_string).and_then(|edit|self.engine.preview_edit(edit).map_err(error)) {
                        self.notify(cause);self.region_tools.cancel();return true;
                    }
                    let mut request=completed.request;
                    let query=request.fill.as_mut().unwrap();
                    query.excluded_seals=query.excluded_seals.iter().copied().chain(ids).collect::<Vec<_>>().into();
                    query.reach=completed.ready.result.gaps.as_ref().map(|masks|layer_render::RegionGapMasks::STOPS[masks.active]);
                    query.trail=Some(layer_render::RegionFillTrail {transaction,points:Default::default()});
                    let operation=completed.ready.operation.clone();let color=completed.ready.color;let basis=completed.ready.basis;
                    let gesture=self.region_tools.gesture.as_mut().unwrap();gesture.before=Some(completed.before);gesture.amendment=Some((completed.checkpoint,after));gesture.ready=Some(completed.ready);
                    if let Err(cause)=self.show_fill_preview(){self.notify(cause);}
                    self.queue_region(request,Purpose::Region {operation:Some(operation),color,basis});
                    return true;
                }
                if !self.region_tools.reduced_motion {
                    let center=[event.surface_position.x,event.surface_position.y];
                    let [w,h]=self.state.camera.viewport.map(|v|v as f32);
                    let radius=[[0.,0.],[w,0.],[w,h],[0.,h]].into_iter().map(|[x,y]|(x-center[0]).hypot(y-center[1])).fold(0.,f32::max);
                    self.region_tools.reveal=Some(Reveal {transaction,center,radius,started:None,duration:(150_000_000.+200_000_000.*(radius/2000.).min(1.)) as u64});
                    self.engine.set_fill_reveal(Some(layer_render::FillReveal {transaction,center,radius:-1.}));
                }
                self.request_region_at(point,None,None,None);
                if self.region_tools.queued.is_none(){self.region_tools.cancel();self.clear_fill_preview();}
            },
            PenPhase::Move=> {
                if let Some(gesture)=&mut self.region_tools.gesture {
                    gesture.point=event.surface_position;
                    if gesture.hold.is_none() && (gesture.point.x-gesture.start.x).hypot(gesture.point.y-gesture.start.y)>self.interaction.touch_policy.slop {
                        gesture.dragging=true;
                    }
                    if gesture.dragging {let p=self.state.camera.input_transform().map(event.surface_position);gesture.path.push([p.x,p.y]);}
                }
                self.finish_fill_reveal();
                if let Err(cause)=self.slide_fill_gesture(){self.notify(cause);}
                if self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.vector.is_none()) && let Err(cause)=self.continue_fill_trail(){self.notify(cause);}
            },
            PenPhase::Up=> {
                if self.region_tools.reveal.as_ref().is_some_and(|reveal|reveal.started.is_some()){self.finish_fill_reveal();}
                if let Some(gesture)=&mut self.region_tools.gesture && gesture.touch.is_some() {
                    gesture.admitted=true;
                }
                if let Some(gesture)=&mut self.region_tools.gesture && gesture.dragging {let p=self.state.camera.input_transform().map(event.surface_position);gesture.path.push([p.x,p.y]);}
                if let Some(gesture)=&mut self.region_tools.gesture {gesture.released=true;}
                if let Err(cause)=self.continue_fill_trail(){self.notify(cause);}
                if let Err(cause)=self.commit_fill_gesture(){self.notify(cause);}
                self.layer_interaction.fill_texture=None;
            },
            PenPhase::Cancel=>{self.region_tools.cancel();self.clear_fill_preview();self.layer_interaction.fill_texture=None;},
            PenPhase::Hover=>(),
        }
        true
    }
    fn vector_seal_hits(&self,layer:OccurrenceHandle,point:[f64;2])->Vec<SealId> {
        let Some(completed)=self.region_tools.vector_completed.as_ref().filter(|completed|completed.checkpoint==self.engine.checkpoint()
            && completed.owner==self.engine.document().owner && completed.layer==layer) else{return Vec::new();};
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        let limit=6.*f64::from(scale/self.state.camera.zoom);
        completed.seals.iter().filter(|(a,b,_)|segment_distance(point,*a,*b)<=limit).map(|(_,_,id)|*id).collect()
    }
    pub(super) fn tap_vector_fill_seal(&mut self,point:Point)->Result<bool,String> {
        if !matches!(self.layer_interaction.tool,LayerCanvasTool::Region {fill:true,..}) {return Ok(false);}
        let Some(layer)=self.editing_vector_layer() else{return Ok(false);};
        let ids=self.vector_seal_hits(layer,[f64::from(point.x),f64::from(point.y)]);if ids.is_empty(){return Ok(false);}
        let mut completed=self.region_tools.vector_completed.take().unwrap();
        self.region_tools.cancel();self.clear_fill_preview();
        let excluded=completed.excluded.iter().copied().chain(ids).collect::<Vec<_>>();
        let (_,lines)=self.vector_line_sources(layer,completed.source);
        if let Some(face)=layer_core::authored::vector::vector_face(&lines,completed.point,Some(completed.reach),&excluded) {
            let doc=self.engine.document();let object=doc.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
            if let Some(position)=object.find(completed.path) && let Some(Record::Path(path))=object.record(position) {
                let [ox,oy]=doc.scene().occurrence_offset64(layer);
                let geometry=face.geometry.transformed(|p|[p[0]-ox,p[1]-oy]).map_err(str::to_string)?;
                let changed=object.replace([(position,Some(Record::Path(path.with_geometry(std::sync::Arc::new(geometry)))))]);
                let edit=doc.vector_object_edit(layer,changed).map_err(error)?;
                self.engine.amend_edit(edit).map_err(error)?;self.layer_interaction.changed=true;
                completed.checkpoint=self.engine.checkpoint();completed.excluded=excluded;completed.seals=face.seals;
            }
        }
        self.region_tools.vector_completed=Some(completed);Ok(true)
    }
    fn begin_vector_fill(&mut self,layer:OccurrenceHandle,point:Point,event:PenEvent)->Result<bool,String> {
        let point=[f64::from(point.x),f64::from(point.y)];
        if !self.vector_seal_hits(layer,point).is_empty(){return Ok(true);}
        self.region_tools.vector_completed=None;
        let source=self.lines_from(self.region_tools.source[1]);
        let extent=self.engine.document().composition().size.map(f64::from);
        if !(0. ..extent[0]).contains(&point[0]) || !(0. ..extent[1]).contains(&point[1]) {return Ok(false);}
        let target=self.engine.document().scene().source_target(layer);
        if self.region_source(source,None,target,true).is_none(){return Ok(false);}
        let doc=self.engine.document();let [ox,oy]=doc.scene().occurrence_offset64(layer);
        let object=doc.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
        let hit=self.vector_fill_hit(object,[point[0]-ox,point[1]-oy],source);
        if hit.is_some_and(|(_,path)|path.style.entries.iter().any(|entry|matches!(entry,PaintEntry::Fill(_)))) {return Ok(true);}
        if self.state.colors.transparent(){return Ok(false);}
        let original=object.clone();let checkpoint=self.engine.checkpoint();let style=self.vector_fill_style()?;
        let mut preview=style.as_ref().clone();
        for entry in &mut preview.entries {if let PaintEntry::Fill(fill)=entry && let Paint::Color {color,..}=&mut fill.paint {color.rgba[3]*=0.5;}}
        let (_,lines)=self.vector_line_sources(layer,source);
        let reach=self.region_tools.gap_reach.or_else(||(!self.region_tools.refinement.close_gaps).then_some(0));
        let face=layer_core::authored::vector::vector_face(&lines,point,reach,&[]);
        if hit.is_some_and(|(_,path)|face.as_ref().is_none_or(|face|face.boundary.len()==1 && std::ptr::eq(lines[*face.boundary.first().unwrap()].path,path))) {return Ok(true);}
        let active=layer_render::RegionGapMasks::STOPS.iter().position(|r|Some(*r)==reach.or_else(||face.as_ref().map(|face|face.reach))).unwrap_or(1);
        let mut slots=std::array::from_fn(|_|None);slots[active]=Some(face.clone());
        let vector=VectorFill {layer,source,point,original,checkpoint,style,preview:std::sync::Arc::new(preview),slots,active,probed:point.map(|v|v as f32),faces:face.into_iter().collect()};
        self.region_tools.gesture=Some(Gesture {transaction:self.region_tools.generation,owner:self.engine.document().owner,device:event.device_id,started:event.timestamp_ns,
            start:event.surface_position,point:event.surface_position,tool:event.tool,touch:(event.tool==ToolKind::Finger).then_some(event.device_id),admitted:event.tool!=ToolKind::Finger,
            released:false,hold:None,waiting:false,tick:0,dragging:false,original_tolerance:self.region_tools.tolerance,original_reach:self.region_tools.gap_reach,
            original_close:self.region_tools.refinement.close_gaps,vector:Some(vector),request:None,ready:None,revision:0,path:vec![point.map(|v|v as f32)],before:None,amendment:None});
        if event.tool!=ToolKind::Finger {self.show_fill_preview()?;}
        Ok(false)
    }
    fn vector_fill_slot(&mut self,slot:usize) {
        let Some(vector)=self.region_tools.gesture.as_ref().and_then(|gesture|gesture.vector.as_ref()).filter(|vector|vector.slots[slot].is_none()) else{return;};
        let (_,lines)=self.vector_line_sources(vector.layer,vector.source);
        let face=layer_core::authored::vector::vector_face(&lines,vector.point,Some(layer_render::RegionGapMasks::STOPS[slot]),&[]);
        self.region_tools.gesture.as_mut().unwrap().vector.as_mut().unwrap().slots[slot]=Some(face);
    }
    fn continue_vector_fill(&mut self)->Result<(),String> {
        let tolerance=0.25*self.document_per_surface_pixel();
        let Some(gesture)=self.region_tools.gesture.as_mut().filter(|gesture|gesture.dragging && gesture.vector.is_some()) else{return Ok(());};
        let vector=gesture.vector.as_mut().unwrap();let point=*gesture.path.last().unwrap();
        if point==vector.probed {return Ok(());}vector.probed=point;let point=point.map(f64::from);
        if vector.faces.iter().any(|face| {
            let mut path=PathItem::new(ItemId(0),std::sync::Arc::new(face.geometry.clone()),vector.style.clone());path.fill_rule=FillRule::EvenOdd;
            path.encloses(point,tolerance)
        }) {return Ok(());}
        let (layer,source,active)=(vector.layer,vector.source,vector.active);let doc=self.engine.document();let [ox,oy]=doc.scene().occurrence_offset64(layer);
        if self.vector_fill_hit(doc.scene().vector_layer(layer).ok_or("The vector layer was removed")?,[point[0]-ox,point[1]-oy],source)
            .is_some_and(|(_,path)|path.style.entries.iter().any(|entry|matches!(entry,PaintEntry::Fill(_)))) {return Ok(());}
        let (_,lines)=self.vector_line_sources(layer,source);
        let face=layer_core::authored::vector::vector_face(&lines,point,Some(layer_render::RegionGapMasks::STOPS[active]),&[]);
        if let Some(face)=face {
            let vector=self.region_tools.gesture.as_mut().unwrap().vector.as_mut().unwrap();
            if vector.faces.is_empty() {vector.point=point;}
            vector.faces.push(face);self.show_fill_preview()?;
        }
        Ok(())
    }
    fn commit_vector_fill(&mut self)->Result<(),String> {
        if !self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.admitted && gesture.released){return Ok(());}
        let gesture=self.region_tools.gesture.take().unwrap();let vector=gesture.vector.as_ref().unwrap();
        if self.engine.checkpoint()!=vector.checkpoint || self.engine.document().owner!=gesture.owner {return Ok(());}
        let restore=self.engine.document().vector_object_edit(vector.layer,vector.original.clone()).map_err(error)?;
        self.engine.preview_edit(restore).map_err(error)?;
        let path=vector.original.allocate_id();
        let result=self.insert_vector_fills(vector.layer,vector.faces.iter().map(|face|face.geometry.clone()).collect(),vector.style.clone(),false);
        if let Err(cause)=result {self.region_tools.gesture=Some(gesture);self.region_tools.cancel();self.clear_fill_preview();return Err(cause);}
        if let [face]=vector.faces.as_slice() {
            self.region_tools.vector_completed=Some(VectorCompleted {checkpoint:self.engine.checkpoint(),owner:gesture.owner,layer:vector.layer,source:vector.source,path,point:vector.point,
                reach:layer_render::RegionGapMasks::STOPS[vector.active],excluded:Vec::new(),seals:face.seals.clone()});
        }
        self.refresh_tools();Ok(())
    }
    pub(in crate::session) fn accept_fill_result(&mut self,result:layer_render::RegionResult,operation:layer_core::RasterOperationKind,
        color:Option<layer_core::color::RgbColor>,basis:layer_core::Affine)->Result<u32,String> {
        let Some(gesture)=&mut self.region_tools.gesture else{return Ok(0);};
        gesture.ready=Some(Ready {result,operation,color,basis});gesture.waiting=false;gesture.revision=gesture.revision.wrapping_add(1);
        let released=gesture.released;
        self.continue_fill_trail()?;
        if released {self.commit_fill_gesture()?;}else if self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.admitted){self.show_fill_preview()?;}
        self.engage_fill_hold();
        self.publish_fill_gesture()
    }
    fn fill_payload(&self)->Result<Option<(Selection,layer_core::RasterOperationKind,Option<layer_core::color::RgbColor>)>,String> {
        let Some(ready)=self.region_tools.gesture.as_ref().and_then(|gesture|gesture.ready.as_ref()) else{return Ok(None);};
        let mut operation=ready.operation.clone();
        let selection=Self::apply_region_result(&ready.result,Some(&mut operation),ready.basis)?;
        Ok(Some((selection,operation,ready.color)))
    }
    pub(in crate::session) fn fill_query_trail(&self)->Option<layer_render::RegionFillTrail> {
        self.region_tools.gesture.as_ref().map(|gesture|layer_render::RegionFillTrail {transaction:gesture.transaction,
            points:if gesture.dragging {gesture.path.clone().into()}else{Default::default()}})
    }
    fn continue_fill_trail(&mut self)->Result<(),String> {
        if self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.vector.is_some()) {return self.continue_vector_fill();}
        let Some(gesture)=self.region_tools.gesture.as_mut().filter(|gesture|gesture.dragging && !gesture.waiting) else{return Ok(());};
        let Some(ready)=&gesture.ready else{return Ok(());};let Some(request)=&gesture.request else{return Ok(());};
        let next=ready.result.gaps.as_ref().and_then(|gaps|gaps.next);
        let changed=request.fill.as_ref().and_then(|fill|fill.trail.as_ref()).is_none_or(|trail|trail.points.as_ref()!=gesture.path);
        if next.is_none() && !changed {return Ok(());}
        let mut request=request.clone();request.position=next.unwrap_or(request.position);
        if let Some(fill)=&mut request.fill {fill.trail=Some(layer_render::RegionFillTrail {transaction:gesture.transaction,points:gesture.path.clone().into()});}
        let operation=ready.operation.clone();let color=ready.color;let basis=ready.basis;gesture.waiting=true;
        self.queue_region(request,Purpose::Region {operation:Some(operation),color,basis});Ok(())
    }
    fn show_fill_preview(&mut self)->Result<(),String> {
        if let Some(vector)=self.region_tools.gesture.as_ref().and_then(|gesture|gesture.vector.as_ref()) {
            let object=self.vector_fill_object(vector.layer,vector.original.clone(),vector.faces.iter().map(|face|face.geometry.clone()).collect(),vector.preview.clone(),false)?;
            let edit=self.engine.document().vector_object_edit(vector.layer,object).map_err(error)?;
            self.engine.preview_edit(edit).map_err(error)?;self.layer_interaction.changed=true;return Ok(());
        }
        let Some((selection,kind,_))=self.fill_payload()? else{return Ok(());};
        let (target,operation)=self.paint_operation_data(Some(selection),kind)?;
        let gesture=self.region_tools.gesture.as_ref().unwrap();
        self.engine.set_fill_preview(Some(layer_render::FillPreview {transaction:gesture.transaction,revision:gesture.revision,target,operation}));
        self.region_tools.preview_active=true;
        Ok(())
    }
    fn commit_fill_gesture(&mut self)->Result<(),String> {
        if self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.vector.is_some()) {return self.commit_vector_fill();}
        if !self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.admitted && gesture.released && !gesture.waiting && gesture.ready.is_some()){return Ok(());}
        let empty=self.region_tools.gesture.as_ref().unwrap().ready.as_ref().unwrap().result.pixels.bounds()==[0;4];
        let payload=self.fill_payload()?;
        let gesture=self.region_tools.gesture.take().unwrap();
        if self.region_tools.preview_active {self.engine.set_fill_preview(None);self.region_tools.preview_active=false;}
        if empty && gesture.amendment.is_some() {
            self.region_tools.gesture=Some(gesture);self.region_tools.cancel();self.clear_fill_preview();return Ok(());
        }
        if empty {self.finish_fill_reveal();}
        if !empty && let Some((selection,operation,color))=payload {
            let committed=(||if let Some((checkpoint,_))=&gesture.amendment {
                let (target,mut operation)=self.paint_operation_data(Some(selection),operation)?;
                operation.coverage.target=self.engine.allocate_coverage_handle();operation.coverage.use_.source=operation.coverage.target;
                self.engine.amend_raster_operation(target,operation,*checkpoint).map_err(error).map(|()|{self.layer_interaction.changed=true;})
            }else{self.paint_operation(Some(selection),operation,color.as_slice())})();
            if let Err(cause)=committed {self.region_tools.gesture=Some(gesture);self.region_tools.cancel();self.clear_fill_preview();return Err(cause);}
            if let (Some(before),Some(request),Some(ready))=(gesture.before,gesture.request,gesture.ready) {
                self.region_tools.completed=Some(Completed {checkpoint:self.engine.checkpoint(),owner:self.engine.document().owner,before,request,ready});
            }
        }
        self.refresh_tools();
        Ok(())
    }
    fn clear_fill_preview(&mut self) {
        self.restore_fill_cancel();
        if std::mem::take(&mut self.region_tools.preview_active){self.engine.set_fill_preview(None);}
        self.finish_fill_reveal();
    }
    pub(in crate::session) fn restore_fill_cancel(&mut self) {
        if let Some((checkpoint,owner,rollback))=self.region_tools.rollback.take()
            && self.engine.checkpoint()==checkpoint && self.engine.document().owner==owner {
            let edit=match rollback {
                Rollback::Paint(handle,source)=>layer_core::RecordChange::replace(&self.engine.document().artwork.paint,handle,Some(source)).map(layer_core::Edit::Paint).map_err(str::to_string),
                Rollback::Vector(layer,object)=>self.engine.document().vector_object_edit(layer,object).map_err(error),
            };
            if let Err(cause)=edit.and_then(|edit|self.engine.preview_edit(edit).map_err(error)){self.notify(cause);}
        }
    }
    pub(in crate::session) fn finish_fill_reveal(&mut self) {
        self.region_tools.reveal=None;self.engine.set_fill_reveal(None);
    }
    pub(in crate::session) fn fill_touch_input(&mut self,input:&UiInput) {
        let view_revision=self.state.camera.revision;
        let event=|id,phase,position:[f32;2],time_ns|PenEvent {device_id:id,sequence:0,timestamp_ns:time_ns,view_revision,
            surface_position:Point {x:position[0],y:position[1]},pressure:1.,tilt_radians:[0.;2],twist_radians:0.,distance:0.,phase,
            tool:ToolKind::Finger,flags:layer_engine::SampleFlags::PRIMARY};
        if let UiInput::ColorPickerHold {id,position,..}=*input && self.region_tools.finger==Some(id) {
            let cancel=event(id,PenPhase::Cancel,position,self.region_tools.now_ns);self.admit_pen(cancel);self.region_tools.finger=None;return;
        }
        let UiInput::Pointer {id,kind,phase,position,time_ns,..}=*input else{return;};
        if phase==ContactPhase::Down {
            if let Some(finger)=self.region_tools.finger && (kind!=PointerKind::Touch || finger!=id) {
                let cancel=event(finger,PenPhase::Cancel,position,time_ns);self.admit_pen(cancel);self.region_tools.finger=None;return;
            }
            if self.region_tools.gesture.as_ref().is_some_and(|gesture|!gesture.released && gesture.touch.is_none()
                && (kind==PointerKind::Touch || gesture.device!=id)) {
                self.region_tools.cancel();self.clear_fill_preview();return;
            }
        }
        if kind!=PointerKind::Touch {return;}
        if !self.uses_fill_gesture() {
            self.region_tools.finger=None;return;
        }
        if phase==ContactPhase::Down && !self.touch.is_active() && self.interaction.pointer.is_none() && !self.state.settings_open {
            self.region_tools.finger=Some(id);let down=event(id,PenPhase::Down,position,time_ns);self.admit_pen(down);
        }else if self.region_tools.finger==Some(id) {
            let pen=event(id,pen_phase(phase),position,time_ns);self.admit_pen(pen);
            if matches!(phase,ContactPhase::Up|ContactPhase::Cancel){self.region_tools.finger=None;}
        }
    }
    fn engage_fill_hold(&mut self) {
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        let width=self.state.camera.viewport[0] as f32/scale;
        let localizer=self.state.localization.clone();
        let prepared=self.engine.backend().fill_preview_ready();
        if let Some(gesture)=&mut self.region_tools.gesture
            && !gesture.released && !gesture.waiting && !gesture.dragging && gesture.hold.is_none() && gesture.tool!=ToolKind::Finger
            && (gesture.vector.is_some() || prepared==Some((gesture.transaction,gesture.revision)))
            && self.region_tools.now_ns.saturating_sub(gesture.started)>=u64::from(self.interaction.touch_policy.tap_ms)*1_000_000
            && let Some((line,active,_))=gesture.gap_view() {
            let fraction=if line {active as f32/4.}else{self.region_tools.tolerance};
            let span=if line {160.}else{200.};let anchor=gesture.point.x/scale;
            let left=anchor-(span*fraction).min(anchor.max(0.));
            let right=anchor+(span*(1.-fraction)).min((width-anchor).max(0.));
            let caption=if line {
                let reach=layer_render::RegionGapMasks::STOPS[active];
                if reach==0 {localizer.text(MessageId::COMMAND_CLOSE_GAPS_OFF)}else{NumericControl::whole_pixels(0.,64.).compact_text(f64::from(reach)).into()}
            }else{NumericControl::percent().compact_text(f64::from(self.region_tools.tolerance)).into()};
            gesture.hold=Some(Hold {line,slot:active,anchor,fraction,left,right,caption,
                label:localizer.text(if line {MessageId::TOOL_CONTROL_CLOSE_GAPS}else{MessageId::TOOL_CONTROL_TOLERANCE})});
            gesture.tick=gesture.tick.wrapping_add(1);
        }
    }
    fn slide_fill_gesture(&mut self)->Result<(),String> {
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        let mut request=None;let mut query=None;let mut show=false;let mut vector_slot=None;
        if let Some(gesture)=&mut self.region_tools.gesture && let Some(hold)=&mut gesture.hold {
            let x=gesture.point.x/scale;
            let fraction=if x<hold.anchor {hold.fraction+(x-hold.anchor)/(hold.anchor-hold.left).max(f32::EPSILON)*hold.fraction}
                else{hold.fraction+(x-hold.anchor)/(hold.right-hold.anchor).max(f32::EPSILON)*(1.-hold.fraction)}.clamp(0.,1.);
            if hold.line {
                let slot=(fraction*4.).round() as usize;
                if hold.slot!=slot {
                    hold.slot=slot;
                    let reach=layer_render::RegionGapMasks::STOPS[slot];
                    if let Some(vector)=&mut gesture.vector {
                        vector.active=slot;vector_slot=Some(slot);gesture.revision=gesture.revision.wrapping_add(1);show=true;
                    }else {
                        request=gesture.request.clone();request.as_mut().unwrap().fill.as_mut().unwrap().reach=Some(reach);
                    }
                    self.region_tools.refinement.close_gaps=reach>0;self.region_tools.gap_reach=(reach>0).then_some(reach);
                    hold.caption=if reach==0 {self.state.localization.text(MessageId::COMMAND_CLOSE_GAPS_OFF)}else{NumericControl::whole_pixels(0.,64.).compact_text(f64::from(reach)).into()};
                    gesture.tick=gesture.tick.wrapping_add(1);
                }
            }else {
                let value=(fraction*100.).round()/100.;
                if (self.region_tools.tolerance-value).abs()>0.00001 {
                    self.region_tools.tolerance=value;hold.caption=NumericControl::percent().compact_text(f64::from(value)).into();
                    request=gesture.request.clone();request.as_mut().unwrap().tolerance=value;
                }
            }
            if let Some(request)=request {let ready=gesture.ready.as_ref().unwrap();query=Some((request,ready.operation.clone(),ready.color,ready.basis));gesture.waiting=true;}
        }
        if let Some(slot)=vector_slot {
            self.vector_fill_slot(slot);
            let vector=self.region_tools.gesture.as_mut().unwrap().vector.as_mut().unwrap();
            vector.faces=vector.slots[slot].as_ref().unwrap().iter().cloned().collect();
        }
        if let Some((request,operation,color,basis))=query {self.queue_region(request,Purpose::Region {operation:Some(operation),color,basis});}
        if show {self.show_fill_preview()?;}
        Ok(())
    }
    fn publish_fill_gesture(&mut self)->Result<u32,String> {
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        let width=self.state.camera.viewport[0] as f32/scale;let height=self.state.camera.viewport[1] as f32/scale;
        let transform=self.document_to_logical();
        let view=self.region_tools.gesture.as_ref().filter(|gesture|gesture.admitted && !gesture.released).map(|gesture| {
            let tip=[gesture.point.x/scale,gesture.point.y/scale];
            let elapsed=self.region_tools.now_ns.saturating_sub(gesture.started);
            let ring=(!self.region_tools.reduced_motion && gesture.hold.is_none() && !gesture.dragging && gesture.tool!=ToolKind::Finger && elapsed>=150_000_000)
                .then(||((elapsed-150_000_000) as f64/(u64::from(self.interaction.touch_policy.tap_ms)*1_000_000).saturating_sub(150_000_000).max(1) as f64).min(1.) as f32);
            let pill=gesture.hold.as_ref().map(|hold| {
                let left=(hold.left-12.).max(0.);let right=(hold.right+12.).min(width);
                let y=if tip[1]>=68. {tip[1]-68.}else{tip[1]+24.};
                FillPillView {bounds:Bounds {x:left,y:y.min((height-44.).max(0.)),width:right-left,height:44.},label:hold.label.clone(),value:hold.caption.clone(),
                    track:[hold.left,hold.right],fill_end:tip[0].clamp(hold.left,hold.right)}
            });
            let seals=if gesture.hold.is_some() {gesture.gap_view().into_iter().flat_map(|(_,_,seals)|seals).map(|seal|seal.map(|p|transform(Point{x:p[0],y:p[1]}))).collect::<Vec<_>>()}else{Vec::new()};
            FillGestureView {transaction:gesture.transaction,tip,ring,pill,resize_cursor:gesture.hold.is_some(),seals:seals.into(),tick:gesture.tick}
        }).or_else(||self.region_tools.vector_completed.as_ref().filter(|completed|completed.checkpoint==self.engine.checkpoint()
            && completed.owner==self.engine.document().owner && self.engine.document().working.occurrence==Some(completed.layer) && !completed.seals.is_empty()).map(|completed| {
                let seals=completed.seals.iter().map(|(a,b,_)|[a,b].map(|p|transform(Point{x:p[0] as f32,y:p[1] as f32}))).collect::<Vec<_>>();
                FillGestureView {transaction:0,tip:[0.;2],ring:None,pill:None,seals:seals.into(),tick:0,resize_cursor:false}
            })).or_else(||self.region_tools.completed.as_ref().filter(|completed|completed.checkpoint==self.engine.checkpoint() && completed.owner==self.engine.document().owner)
            .and_then(|completed|completed.ready.result.gaps.as_ref()).map(|masks| {
                let seals=masks.seals.iter().flat_map(seal_segments)
                    .map(|segment|segment.map(|[x,y]|transform(Point {x,y}))).collect::<Vec<_>>();
                FillGestureView {transaction:0,tip:[0.;2],ring:None,pill:None,seals:seals.into(),tick:0,resize_cursor:false}
            }));
        if self.state.fill_gesture!=view {self.state.fill_gesture=view;Ok(regions::COLOR_PREVIEW)}else{Ok(0)}
    }
    pub(in crate::session) fn set_fill_motion(&mut self,reduced:bool)->Result<u32,String> {
        self.region_tools.reduced_motion=reduced;
        if reduced {self.finish_fill_reveal();}
        self.publish_fill_gesture()
    }
    pub(in crate::session) fn advance_fill_gesture(&mut self,now_ns:u64)->Result<u32,String> {
        self.region_tools.now_ns=now_ns;
        self.restore_fill_cancel();
        if self.region_tools.reveal.is_none(){self.engine.set_fill_reveal(None);}
        if self.region_tools.gesture.is_none() && self.region_tools.preview_active {
            self.engine.set_fill_preview(None);self.region_tools.preview_active=false;
        }
        if self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.vector.as_ref().is_some_and(|vector|
            vector.checkpoint!=self.engine.checkpoint() || gesture.owner!=self.engine.document().owner || self.engine.document().working.occurrence!=Some(vector.layer))) {
            self.region_tools.cancel();self.restore_fill_cancel();
        }
        if let Err(cause)=self.continue_vector_fill(){self.notify(cause);}
        let slot=self.region_tools.gesture.as_ref().filter(|gesture|!gesture.released && !gesture.dragging && gesture.tool!=ToolKind::Finger).and_then(|gesture|gesture.vector.as_ref())
            .and_then(|vector|(0..5).filter(|&slot|vector.slots[slot].is_none()).min_by_key(|&slot|(slot.abs_diff(vector.active),slot)));
        if let Some(slot)=slot {self.vector_fill_slot(slot);}
        self.engage_fill_hold();
        let accepted=self.region_tools.gesture.as_ref().is_none_or(|gesture|gesture.admitted && gesture.ready.is_some());
        let prepared=self.region_tools.gesture.as_ref().map_or_else(||!self.engine.has_pending_document_edits(),
            |gesture|self.engine.backend().fill_preview_ready()==Some((gesture.transaction,gesture.revision)));
        if accepted && let Some(reveal)=&mut self.region_tools.reveal {
            let progress=if prepared || reveal.started.is_some() {
                let started=*reveal.started.get_or_insert(now_ns);(now_ns.saturating_sub(started) as f64/reveal.duration as f64).min(1.) as f32
            }else{0.};
            if progress==1. {self.finish_fill_reveal();}else{
                self.engine.set_fill_reveal(Some(layer_render::FillReveal {transaction:reveal.transaction,center:reveal.center,radius:reveal.radius*(1.-(1.-progress).powi(3))}));
            }
        }
        self.publish_fill_gesture()
    }
}
fn seal_segments(seal:&layer_render::RegionSeal)->impl Iterator<Item=[[f32;2];2]> {
    let point=|p:[u32;2]|p.map(|v|v as f32+0.5);
    let (start,end,bend)=(point(seal.start),point(seal.end),seal.bend.map(point));
    std::iter::once([start,bend.unwrap_or(end)]).chain(bend.map(|bend|[bend,end]))
}
fn seal_distance(point:[f32;2],seal:&layer_render::RegionSeal)->f32 {
    seal_segments(seal).map(|[a,b]|segment_distance(point.map(f64::from),a.map(f64::from),b.map(f64::from))).fold(f64::INFINITY,f64::min) as f32
}

fn segment_distance(point:[f64;2],a:[f64;2],b:[f64;2])->f64 {
    let d=[b[0]-a[0],b[1]-a[1]];let square=d[0]*d[0]+d[1]*d[1];
    let t=if square==0. {0.}else{(((point[0]-a[0])*d[0]+(point[1]-a[1])*d[1])/square).clamp(0.,1.)};
    (point[0]-a[0]-t*d[0]).hypot(point[1]-a[1]-t*d[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::{surface_at,vector_gap_fill_session};

    #[test]
    fn vector_fill_finger_prepares_only_the_active_slot() {
        for platform in Platform::ALL {
            let (mut s,_)=vector_gap_fill_session(platform,&[[100.,100.]],false);
            let point=surface_at(&s,[200.,200.]);
            s.input(UiInput::Pointer {id:41,kind:PointerKind::Touch,button:PointerButton::Primary,
                position:[point.x,point.y],time_ns:100_000_000,phase:ContactPhase::Down}).unwrap();
            for now in [100_000_000,110_000_000,120_000_000] {s.frame(now,now).unwrap();}
            let vector=s.region_tools.gesture.as_ref().unwrap().vector.as_ref().unwrap();
            assert!(vector.slots[vector.active].is_some());
            assert_eq!(vector.slots.iter().filter(|slot|slot.is_some()).count(),1,"{platform:?}");
        }
    }
}
