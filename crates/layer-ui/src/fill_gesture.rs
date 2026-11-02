use super::*;
use layer_engine::ToolKind;

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
    anchor:f32,
    fraction:f32,
    left:f32,
    right:f32,
    caption:std::sync::Arc<str>,
    label:std::sync::Arc<str>,
}
impl Gesture {
    pub(in crate::session) fn holding(&self)->bool {self.hold.is_some()}
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
            && self.selection_masks.target().is_none() && self.editing_vector_layer().is_none()
    }
    pub(in crate::session) fn fill_pen(&mut self,event:PenEvent)->bool {
        if !self.uses_fill_gesture() {return false;}
        if event.phase!=PenPhase::Down && self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.device!=event.device_id){return true;}
        if matches!(event.phase,PenPhase::Move|PenPhase::Up) && self.region_tools.gesture.as_ref().is_some_and(|gesture|gesture.touch.is_some()
            && (event.surface_position.x-gesture.start.x).hypot(event.surface_position.y-gesture.start.y)>self.interaction.touch_policy.slop) {
            self.region_tools.cancel();self.clear_fill_preview();self.layer_interaction.fill_texture=None;return true;
        }
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        match event.phase {
            PenPhase::Down=> {
                self.region_tools.cancel();self.clear_fill_preview();
                let previous=self.region_tools.completed.take().filter(|completed|completed.checkpoint==self.engine.checkpoint()
                    && completed.owner==self.engine.document().owner
                    && self.engine.document().drawing_content()==Some(layer_core::SourceTarget::Paint(completed.before.0)));
                let point=self.state.camera.input_transform().map(event.surface_position);
                let hit=previous.as_ref().and_then(|completed| {
                    let masks=completed.ready.result.gaps.as_ref()?;let limit=6.*scale/self.state.camera.zoom;
                    if !masks.seals.iter().any(|seal|seal.reach==layer_render::RegionGapMasks::STOPS[masks.active] && seal_distance([point.x,point.y],seal)<=limit){return None;}
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
                    original_reach:self.region_tools.gap_reach,original_close:self.region_tools.refinement.close_gaps,request:None,ready:None,revision:0,
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
                if let Err(cause)=self.continue_fill_trail(){self.notify(cause);}
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
        let selection=Selection::pixels(ready.result.pixels.clone()).transformed(ready.basis).map_err(error)?;
        let mut operation=ready.operation.clone();
        if let layer_core::RasterOperationKind::Fill {underfill,edges,..}=&mut operation {
            if let Some(edges)=edges {edges.anchors=ready.result.anchors.clone();}
            *underfill=ready.result.underfill.clone().map(Selection::pixels).map(|selection|selection.transformed(ready.basis)).transpose().map_err(error)?;
        }
        Ok(Some((selection,operation,ready.color)))
    }
    pub(in crate::session) fn fill_query_trail(&self)->Option<layer_render::RegionFillTrail> {
        self.region_tools.gesture.as_ref().map(|gesture|layer_render::RegionFillTrail {transaction:gesture.transaction,
            points:if gesture.dragging {gesture.path.clone().into()}else{Default::default()}})
    }
    fn continue_fill_trail(&mut self)->Result<(),String> {
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
        let Some((selection,kind,_))=self.fill_payload()? else{return Ok(());};
        let (target,operation)=self.paint_operation_data(Some(selection),kind)?;
        let gesture=self.region_tools.gesture.as_ref().unwrap();
        self.engine.set_fill_preview(Some(layer_render::FillPreview {transaction:gesture.transaction,revision:gesture.revision,target,operation}));
        self.region_tools.preview_active=true;
        Ok(())
    }
    fn commit_fill_gesture(&mut self)->Result<(),String> {
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
            if let (Some(before),Some(request),Some(mut ready))=(gesture.before,gesture.request,gesture.ready) {
                if let Some(gaps)=&mut ready.result.gaps {let selected=gaps.regions[gaps.active].clone();gaps.regions=std::array::from_fn(|_|selected.clone());}
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
    fn restore_fill_cancel(&mut self) {
        if let Some((checkpoint,owner,handle,source))=self.region_tools.rollback.take()
            && self.engine.checkpoint()==checkpoint && self.engine.document().owner==owner {
            let change=layer_core::RecordChange::replace(&self.engine.document().artwork.paint,handle,Some(source));
            if let Err(cause)=change.map(layer_core::Edit::Paint).map_err(str::to_string).and_then(|edit|self.engine.preview_edit(edit).map_err(error)){self.notify(cause);}
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
            && prepared==Some((gesture.transaction,gesture.revision))
            && self.region_tools.now_ns.saturating_sub(gesture.started)>=u64::from(self.interaction.touch_policy.tap_ms)*1_000_000
            && let Some(ready)=&gesture.ready {
            let line=ready.result.gaps.as_ref().is_some_and(|masks|masks.line_bounded);
            let fraction=if line {ready.result.gaps.as_ref().unwrap().active as f32/4.}else{self.region_tools.tolerance};
            let span=if line {160.}else{200.};let anchor=gesture.point.x/scale;
            let left=anchor-(span*fraction).min(anchor.max(0.));
            let right=anchor+(span*(1.-fraction)).min((width-anchor).max(0.));
            let caption=if line {
                let reach=layer_render::RegionGapMasks::STOPS[ready.result.gaps.as_ref().unwrap().active];
                if reach==0 {localizer.text(MessageId::COMMAND_CLOSE_GAPS_OFF)}else{NumericControl::whole_pixels(0.,64.).compact_text(f64::from(reach)).into()}
            }else{NumericControl::percent().compact_text(f64::from(self.region_tools.tolerance)).into()};
            gesture.hold=Some(Hold {line,anchor,fraction,left,right,caption,
                label:localizer.text(if line {MessageId::TOOL_CONTROL_CLOSE_GAPS}else{MessageId::TOOL_CONTROL_TOLERANCE})});
            gesture.tick=gesture.tick.wrapping_add(1);
        }
    }
    fn slide_fill_gesture(&mut self)->Result<(),String> {
        let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
        let mut query=None;let mut show=false;
        if let Some(gesture)=&mut self.region_tools.gesture && let Some(hold)=&mut gesture.hold {
            let x=gesture.point.x/scale;
            let fraction=if x<hold.anchor {hold.fraction+(x-hold.anchor)/(hold.anchor-hold.left).max(f32::EPSILON)*hold.fraction}
                else{hold.fraction+(x-hold.anchor)/(hold.right-hold.anchor).max(f32::EPSILON)*(1.-hold.fraction)}.clamp(0.,1.);
            if hold.line {
                let slot=(fraction*4.).round() as usize;
                let ready=gesture.ready.as_mut().unwrap();let masks=ready.result.gaps.as_mut().unwrap();
                if masks.active!=slot {
                    masks.active=slot;ready.result.pixels=masks.regions[slot].pixels.clone();ready.result.underfill=masks.regions[slot].underfill.clone();
                    let reach=layer_render::RegionGapMasks::STOPS[slot];
                    self.region_tools.refinement.close_gaps=reach>0;self.region_tools.gap_reach=(reach>0).then_some(reach);
                    hold.caption=if reach==0 {self.state.localization.text(MessageId::COMMAND_CLOSE_GAPS_OFF)}else{NumericControl::whole_pixels(0.,64.).compact_text(f64::from(reach)).into()};
                    gesture.tick=gesture.tick.wrapping_add(1);gesture.revision=gesture.revision.wrapping_add(1);show=true;
                }
            }else {
                let value=(fraction*100.).round()/100.;
                if (self.region_tools.tolerance-value).abs()>0.00001 {
                    self.region_tools.tolerance=value;hold.caption=NumericControl::percent().compact_text(f64::from(value)).into();
                    let mut request=gesture.request.clone().unwrap();request.tolerance=value;
                    let ready=gesture.ready.as_ref().unwrap();
                    query=Some((request,ready.operation.clone(),ready.color,ready.basis));gesture.waiting=true;
                }
            }
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
            let mut seals=Vec::new();
            if gesture.hold.is_some() && let Some(masks)=gesture.ready.as_ref().and_then(|ready|ready.result.gaps.as_ref()) {
                let reach=layer_render::RegionGapMasks::STOPS[masks.active];
                for seal in masks.seals.iter().filter(|seal|seal.reach==reach) {
                    let point=|[x,y]:[u32;2]|transform(Point {x:x as f32+0.5,y:y as f32+0.5});
                    let start=point(seal.start);let end=point(seal.end);
                    if let Some(bend)=seal.bend {let bend=point(bend);seals.extend([[start,bend],[bend,end]]);}else{seals.push([start,end]);}
                }
            }
            FillGestureView {transaction:gesture.transaction,tip,ring,pill,resize_cursor:gesture.hold.is_some(),seals:seals.into(),tick:gesture.tick}
        }).or_else(||self.region_tools.completed.as_ref().filter(|completed|completed.checkpoint==self.engine.checkpoint() && completed.owner==self.engine.document().owner)
            .and_then(|completed|completed.ready.result.gaps.as_ref()).map(|masks| {
                let mut seals=Vec::new();let reach=layer_render::RegionGapMasks::STOPS[masks.active];
                for seal in masks.seals.iter().filter(|seal|seal.reach==reach) {
                    let point=|[x,y]:[u32;2]|transform(Point {x:x as f32+0.5,y:y as f32+0.5});let start=point(seal.start);let end=point(seal.end);
                    if let Some(bend)=seal.bend {let bend=point(bend);seals.extend([[start,bend],[bend,end]]);}else{seals.push([start,end]);}
                }
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
fn seal_distance(point:[f32;2],seal:&layer_render::RegionSeal)->f32 {
    let distance=|a:[u32;2],b:[u32;2]| {
        let a=a.map(|v|v as f32+0.5);let b=b.map(|v|v as f32+0.5);let d=[b[0]-a[0],b[1]-a[1]];
        let t=(((point[0]-a[0])*d[0]+(point[1]-a[1])*d[1])/(d[0]*d[0]+d[1]*d[1]).max(f32::EPSILON)).clamp(0.,1.);
        (point[0]-a[0]-t*d[0]).hypot(point[1]-a[1]-t*d[1])
    };
    seal.bend.map_or_else(||distance(seal.start,seal.end),|bend|distance(seal.start,bend).min(distance(bend,seal.end)))
}
