use super::*;
use crate::frame_demand::Allowance;

pub(super) fn objects_or_vectors(scene: SceneView<'_>) -> bool {
    scene.order().iter().any(|&owner| scene.object_layer(owner).is_some() || scene.vector_layer(owner).is_some())
}

pub(super) fn exact_at_view_level(scene: SceneView<'_>, level: u32) -> bool {
    level > 0 || objects_or_vectors(scene)
}

pub(super) fn presented_accuracy(r: &WgpuRasterizer, scene: SceneView<'_>, level: u32, preview: bool, approximate: bool) -> pages::Accuracy {
    if !preview && r.moving_layer.is_none() && exact_at_view_level(scene, level) && !approximate {pages::Accuracy::Exact} else {pages::Accuracy::Display}
}

pub(super) fn refining(r: &WgpuRasterizer) -> bool {
    r.moving_layer.is_none() && !r.transform_preview.as_ref().is_some_and(|preview| preview.moving)
}

pub(super) fn refines_at(r: &WgpuRasterizer, view: layer_render::ViewState) -> bool {
    refining(r) && r.artwork_frame.as_ref().is_none_or(|frame| frame.view.document_to_surface == view.document_to_surface)
}

enum Outcome {Committed,Waiting(crate::frame_demand::UnitWait),Refused(PixelRect,crate::residency::Blocked),Skipped,Deferred}

struct Admitted {interest:pages::Interest,view:layer_render::ViewState,work:u64,waiting:Option<crate::frame_demand::UnitWait>}

#[derive(Default)]
pub(super) struct Streaming {units:Vec<Admitted>,turned:u64}
impl Streaming {
    fn work(&self)->u64 {self.units.iter().map(|unit|unit.work).sum()}
}

#[derive(Clone)]
enum Closed {Full,Refused(crate::residency::Blocked)}
impl Closed {
    fn applies(&self,class:Class,allowance:Allowance)->bool {match self {Self::Full=>true,Self::Refused(refusal)=>allowance==Allowance::Fresh && class>=refusal.class()}}
}

struct Pass {started:bool,owed:bool,closed:Option<Closed>}

fn asleep(wait:Option<&crate::frame_demand::UnitWait>,r:&WgpuRasterizer)->crate::frame_demand::Outstanding {
    use crate::frame_demand::{Input,Outstanding::{Blocked,Runnable},Turn};
    match wait.map(|wait|wait.observe(r)) {Some(Turn::Sleep(input))=>Blocked(input),Some(Turn::Dormant)=>Blocked(Input::Residency),_=>Runnable}
}

#[derive(Default)]
pub(super) struct Exact {pub(super) committed:bool,pub(super) blocks:Vec<(pages::Key,Option<crate::frame_demand::UnitWait>)>,pub(super) refusals:Vec<(PixelRect,crate::residency::Blocked,Allowance)>,
    pub(super) pivot:sources::Prepared}
impl Exact {
    fn refuse(&mut self,region:PixelRect,refusal:crate::residency::Blocked,allowance:Allowance,pivot:bool) {
        if pivot {self.pivot.refusal.get_or_insert_with(||refusal.clone());}
        self.refusals.push((region,refusal,allowance));
    }
}

impl Cache {
    pub fn approximating(&self) -> bool { !self.output_complete() || !self.preparing.is_empty() || self.overview.as_ref().is_some_and(|overview| overview.approximating()) }


    pub fn has_pending_work(&self, r: &WgpuRasterizer) -> bool {
        use crate::frame_demand::Outstanding::{Blocked,Runnable};
        match self.outstanding(r) {Some(Runnable)=>true,Some(Blocked(input))=>!r.wakes(input),None=>false}
    }
    pub(crate) fn outstanding(&self, r: &WgpuRasterizer) -> Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::Outstanding::Runnable;
        let own=self.display_demand(r).or_else(||self.pivot_demand(r));
        let overview=self.overview.as_ref().filter(|overview|!overview.held(r)).and_then(|overview|overview.display_demand(r));
        if own==Some(Runnable) || overview==Some(Runnable) {Some(Runnable)}else {own.or(overview)}
    }
    fn display_demand(&self, r: &WgpuRasterizer) -> Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::Outstanding::{self,Runnable};
        if self.contact || self.retired.is_some() {return Some(Runnable);}
        let prepared=self.preparing.iter().map(|wait|wait.demand(r)).fold(None,Outstanding::either);
        Outstanding::either(self.cells_demand(r,refining(r)).or_else(||self.pixels.committed_root().is_none().then(||self.residual_demand(r)).flatten())
            .or_else(||self.reduction_demand(r)),prepared)
    }
    fn reduction_demand(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {
        (!self.pixels.unreduced().is_empty() && self.pixels.committed_root().is_some() && self.coverage_complete())
            .then(||asleep(self.reduction_wait.as_ref(),r))
    }

    pub(super) fn at_rest(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {self.gesture.is_none() && packet.dab_batches.is_empty() && refining(r)}
    pub(super) fn approximates(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        self.graph.root.as_ref().is_some_and(|root|super::cell_plan::Footprint::approximate(root,packet.scene,self.plan.level,self.at_rest(r,packet)))
    }
    pub(super) fn domains(&self,packet:FramePacket<'_>,at_rest:bool)->[BTreeSet<SourceTarget>;2] {
        self.graph.root.as_ref().map(|root|super::cell_plan::Footprint::of(root,packet.scene,self.plan.bounds,self.plan.extent,self.plan.level,at_rest).domains()).unwrap_or_default()
    }
    pub(super) fn release_streaming(&mut self) {self.streaming=Streaming::default();}
    fn settles_at_view(&self)->bool {
        self.pages.lock().unwrap().expected().is_some_and(|frame|exact_at_view_level(frame.scene(),self.plan.level))
    }
    pub(super) fn shows(&self,expression:&graph::Node)->bool {self.graph.content.as_ref()==Some(expression) || self.pivot_halves().contains(expression)}
    fn current(&self,cell:&pages::Key)->bool {self.shows(&cell.expression) || self.overview.as_ref().is_some_and(|overview|overview.graph.content.as_ref()==Some(&cell.expression))}
    fn full(&self,r:&WgpuRasterizer,budget:&WorkBudget,work:u64)->bool {
        let flight=std::iter::once(self).chain(r.scale_display.as_ref()).chain(r.navigator.cache()).map(|cache|cache.streaming.work()).sum::<u64>();
        flight!=0 && flight.saturating_add(budget.spent()).saturating_add(work)>budget.capacity()
    }
    fn flight(&self,region:PixelRect,closed:&Closed)->Outcome {
        match closed {
            Closed::Refused(refusal)=>Outcome::Refused(region,refusal.clone()),
            Closed::Full=>self.streaming.units.iter().map(|unit|unit.waiting.clone()).collect::<Option<Vec<_>>>()
                .and_then(|waiting|waiting.into_iter().next()).map_or(Outcome::Deferred,Outcome::Waiting),
        }
    }
    fn retain_exact(&self,key:&pages::Key) {
        if key.level!=self.plan.level || !self.shows(&key.expression) {self.pages.lock().unwrap().set_optional(key);}
    }
    pub(super) fn pivot_missing(&self,region:PixelRect)->bool {
        let halves=self.pivot_halves().len();
        halves>0 && self.half_cells(region).len()<pages::coordinates(region.intersect(self.plan.bounds),self.plan.level).count()
    }
    pub(super) fn note_pivot_edit(&mut self) {
        let Some((owner,active))=self.graph.active().zip(self.graph.parts.as_ref().map(|parts|parts.nodes[1].clone())) else {return;};
        let Some(stamp)=self.pages.lock().unwrap().expected().and_then(|frame|frame.authored_revision(&active).ok()) else {return;};
        if self.pivot_edit.is_some_and(|(held,..)|held!=owner) {self.pivot_refusal=None;}
        self.pivot_edit=Some(match self.pivot_edit {
            Some((held,base,edited)) if held==owner=>(held,base,edited || base!=stamp),
            selected=>(owner,stamp,selected.is_some()),
        });
    }
    pub(super) fn pivot_edited(&self)->bool {self.pivot_edit.is_some_and(|(_,_,edited)|edited)}
    pub(super) fn pivot_allowance(&self,r:&WgpuRasterizer)->Option<Allowance> {self.pivot_refusal.as_ref().map_or(Some(Allowance::Fresh),|wait|wait.eligible(r))}
    pub(super) fn settle_pivot(&mut self,prepared:sources::Prepared,allowance:Allowance) {
        self.pivot_refusal=prepared.refusal.map(|refusal|crate::frame_demand::UnitWait::parked(refusal,allowance));
        if prepared.waiting.is_some() {self.pivot_waiting=prepared.waiting;}
    }
    fn folds_halves(&self,key:&pages::Key,class:Class)->bool {
        let halves=self.pivot_halves();
        let region=key.plan().bounds;
        !halves.is_empty() && (self.pivot_edited() && class==Class::Refinement || halves.into_iter().all(|half|
            self.pages.lock().unwrap().get(&pages::Key {expression:half,..key.clone()},region,pages::Accuracy::Exact,0,class).is_some()))
    }
    pub(super) fn wanted_cells(&self,region:PixelRect,view:layer_render::ViewState)->Vec<PixelRect> {
        let mut cells=pages::coordinates(region.intersect(self.plan.bounds),self.plan.level).map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent)).collect::<Vec<_>>();
        let near=self.pointer.map_or(view,|point|centred(view,point));
        cells.sort_by_key(|cell|centre_distance(near,*cell));
        cells
    }
    pub(super) fn half_keys(&self,cells:&[PixelRect])->Vec<pages::Key> {
        let halves=self.pivot_halves();
        let settles=self.settles_at_view();
        cells.iter().flat_map(|cell|{let cell=if settles {self.page_key(self.graph.root.clone().unwrap(),*cell)}else {self.refinement_interest(self.plan.level,*cell).key};
            halves.iter().map(move|half|pages::Key {expression:half.clone(),..cell.clone()})}).collect()
    }
    #[expect(clippy::too_many_arguments, reason = "Halves share the frame's scene, encoding and budget with the pass that wants them")]
    pub(super) fn build_halves(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget, retained: u64,
        cells: Vec<PixelRect>, class: Class, owed: bool, pivot: Option<Allowance>,
    ) -> Result<Exact, GpuRasterError> {
        if cells.is_empty() {return Ok(Exact::default());}
        if !exact_at_view_level(packet.scene,self.plan.level) {
            let keys=self.half_keys(&cells);
            return self.exact_units(scene,r,packet,encoder,budget,keys,&|_|class,owed,pivot);
        }
        let halves=self.pivot_halves();
        let cells=cells.into_iter().filter(|cell|self.pivot_missing(*cell)).collect::<Vec<_>>();
        let mut completed=Vec::new();
        let built=self.half_units(scene,r,packet,encoder,retained,budget,cells,&halves,class,pivot,&mut completed)?;
        self.encode_units(scene,r,encoder,completed.iter_mut().map(|ready|std::mem::take(&mut ready.unit)))?;
        for ready in completed {self.pages.lock().unwrap().finish_inputs(&ready.handle);}
        let mut exact=Exact::default();
        for (region,wait) in built.blocks {exact.blocks.extend(halves.first().map(|half|(self.page_key(half.clone(),region),wait)));}
        for (region,refusal,allowance) in built.refusals {exact.refuse(region,refusal,allowance,true);}
        Ok(exact)
    }
    pub(super) fn pivot_halves(&self)->Vec<graph::Node> {
        self.graph.parts.iter().flat_map(|parts|[&parts.nodes[0],&parts.nodes[2]]).filter(|node|!graph::Expression::is_clear(node) && self.graph.caches(node)).cloned().collect()
    }
    fn pivot_demand(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::Outstanding::Blocked;
        if !refining(r) || self.gesture.is_some() || self.graph.parts.is_none() {return None;}
        let waiting=||Some(asleep(self.pivot_waiting.as_ref(),r));
        let flying=self.pivot_halves();
        if self.streaming.units.iter().any(|unit|flying.contains(&unit.interest.key.expression)) {return waiting();}
        self.pivot_allowance(r)?;
        let halves=if self.pivot_edited() {self.pivot_halves()}else {Vec::new()};
        if halves.is_empty() {return self.pivot_input(r);}
        let held=self.half_cells(self.plan.bounds);
        let settles=self.settles_at_view();
        let mut naming=false;
        for coordinate in pages::coordinates(self.plan.bounds,self.plan.level) {
            let region=pages::bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds);
            if held.contains(&region) || self.refusals_hold(r) && self.refused(r,region) {continue;}
            if settles {return waiting();}
            let cell=self.refinement_interest(self.plan.level,region).key;
            for half in &halves {
                let mut frontier=Vec::new();
                self.pages.lock().unwrap().refinement_frontier(&pages::Key {expression:half.clone(),..cell.clone()},&mut frontier);
                if !frontier.is_empty() {return waiting();}
            }
            naming=true;
        }
        if naming {return Some(Blocked(crate::frame_demand::Input::Name));}
        self.pivot_input(r)
    }
    fn pivot_input(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {self.pivot_pending.then(||asleep(self.pivot_waiting.as_ref(),r))}
    pub(super) fn pivot_inputs_pending(&self,scene:&Scene,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        if self.graph.parts.is_none() || self.pivot_missing(self.plan.bounds) {return false;}
        let root=self.graph.root.clone().unwrap();
        let tiled=use_tiles(r,self.plan,packet,Some(scene)) && !(self.plan.level>0 && root.fused_transform(r));
        self.region_sources(scene,r,packet,self.plan.bounds,tiled).iter().any(|(id,request)|!self.source_ready(scene,r,*id,request))
    }

    pub(super) fn refine(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords, budget: &mut WorkBudget,
    ) -> Result<PixelRect, GpuRasterError> {
        if self.gesture.is_some() || !refining(r) || self.graph.root.is_none() {return Ok(PixelRect::EMPTY);}
        self.pairing=self.overview.as_deref().filter(|_|self.plan.level==0).map_or_else(Default::default,Cache::pairing);
        let pivot=self.pivot_allowance(r);
        let halves=if self.pivot_edited() {self.pivot_halves()}else {Vec::new()};
        let wanted=if halves.is_empty() {Vec::new()}else {self.wanted_cells(self.plan.bounds,packet.view)};
        let mut exact=if exact_at_view_level(packet.scene,self.plan.level) {
            self.build_halves(scene,r,packet,encoder,budget,0,wanted,Class::Refinement,false,pivot)?
        } else {
            let mut coordinates=pages::coordinates(self.plan.bounds,self.plan.level)
                .filter(|coordinate|!self.awaits_compose(pages::bounds(*coordinate,self.plan.level,self.plan.extent))).collect::<Vec<_>>();
            coordinates.sort_by_key(|coordinate|centre_distance(packet.view,pages::bounds(*coordinate,self.plan.level,self.plan.extent)));
            let mut keys=coordinates.into_iter().map(|coordinate|self.refinement_interest(self.plan.level,pages::bounds(coordinate,self.plan.level,self.plan.extent)).key).collect::<Vec<_>>();
            keys.extend(self.half_keys(&wanted));
            self.exact_units(scene,r,packet,encoder,budget,keys,&|_|Class::Refinement,false,pivot)?
        };
        self.pivot_waiting=exact.blocks.iter().find(|(key,_)|halves.contains(&key.expression)).and_then(|(_,wait)|wait.clone());
        for (key,wait) in &exact.blocks {self.block(key.plan().bounds,wait.clone());}
        for (region,refusal,allowance) in &exact.refusals {self.refuse(*region,refusal,*allowance);}
        self.pivot_pending=self.pivot_inputs_pending(scene,r,packet);
        if let Some(allowance)=pivot {
            let mut prepared=std::mem::take(&mut exact.pivot);
            if self.pivot_pending && prepared.refusal.is_none() {
                prepared=prepared.or(self.prepare_pivot(scene,r,packet,encoder,budget,self.plan.bounds,&|_|Class::Refinement,allowance)?);
                self.pivot_pending=self.pivot_inputs_pending(scene,r,packet);
            }
            self.settle_pivot(prepared,allowance);
        }
        if exact.committed {self.project_exact(scene,r,encoder,commands)}else {Ok(PixelRect::EMPTY)}
    }

    #[expect(clippy::too_many_arguments, reason = "Exact units share the frame's scene, encoding and budget with the pass that selected their cells")]
    pub(super) fn exact_units(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget,
        cells: Vec<pages::Key>, class: &dyn Fn(PixelRect)->Class, owed: bool, pivot: Option<Allowance>,
    ) -> Result<Exact, GpuRasterError> {
        let mut exact=Exact::default();
        let halves=self.pivot_halves();
        let mut outcomes=std::collections::HashMap::<pages::Key,(bool,Option<crate::frame_demand::UnitWait>)>::new();
        {let units=std::mem::take(&mut self.streaming.units);let pages=self.pages.lock().unwrap();
            self.streaming.units=units.into_iter().filter(|unit|self.current(&unit.interest.key) && pages.accepted(&unit.interest.key,unit.interest.key.plan().bounds,0)).collect();}
        let admitted=self.streaming.units.iter().map(|unit|unit.interest.key.clone()).filter(|unit|cells.iter().any(|cell|cell.expression==unit.expression)).collect::<Vec<_>>();
        let mut pass=Pass {started:false,owed,closed:None};
        let flying=admitted.len();
        for (index,cell) in admitted.into_iter().chain(cells).enumerate() {
            let half=halves.contains(&cell.expression);
            if index>=flying && (self.refused(r,cell.plan().bounds) || half && pivot.is_none()) {continue;}
            if index>=flying && !half && let Some(wait)=self.pairing.awaits(cell.plan().bounds) {exact.blocks.push((cell,wait));continue;}
            let allowance=if !owed && self.cell_allowance(r,cell.plan().bounds)==Allowance::Spent || half && pivot!=Some(Allowance::Fresh) {Allowance::Spent}else {Allowance::Fresh};
            if index>=flying && let Some(closed)=pass.closed.as_ref().filter(|closed|closed.applies(class(cell.plan().bounds),allowance)) {
                match self.flight(cell.plan().bounds,closed) {
                    Outcome::Waiting(input)=>exact.blocks.push((cell.clone(),Some(input))),
                    Outcome::Refused(region,refusal)=>exact.refuse(region,refusal,allowance,half),
                    _=>{},
                }
                continue;
            }
            let mut frontier=Vec::new();
            self.pages.lock().unwrap().refinement_frontier(&cell,&mut frontier);
            if frontier.is_empty() {continue;}
            let (mut pending,mut waiting)=(false,None);
            let mut pinned=Vec::new();
            for key in frontier {
                if let Some((held,outcome))=outcomes.get(&key) {pending|=*held;waiting=waiting.or_else(||outcome.clone());continue;}
                let outcome=self.exact_unit(scene,r,packet,encoder,budget,&mut pass,&mut pinned,&key,class,allowance)?;
                let held=!matches!(outcome,Outcome::Committed);
                let input=match outcome {
                    Outcome::Committed=>{exact.committed=true;None},
                    Outcome::Waiting(input)=>Some(input),
                    Outcome::Deferred=>None,
                    Outcome::Refused(region,refusal)=>{exact.refuse(region,refusal,allowance,half);if allowance==Allowance::Spent {break;}continue;},
                    Outcome::Skipped=>continue,
                };
                pending|=held;waiting=waiting.or_else(||input.clone());
                outcomes.insert(key,(held,input));
            }
            for page in pinned {page.track(encoder);}
            if waiting.is_some() {exact.blocks.push((cell,waiting));}
            else if !pending && let Some(allowance)=pivot.filter(|_|exact.pivot.refusal.is_none()) {
                let prepared=self.prepare_pivot(scene,r,packet,encoder,budget,cell.plan().bounds,class,allowance)?;
                exact.pivot=std::mem::take(&mut exact.pivot).or(prepared);
            }
        }
        Ok(exact)
    }

    fn cell_parts(&self,cell:&pages::Key)->Vec<pages::Key> {
        std::iter::once(cell.expression.clone()).chain(self.graph.parts.as_ref().filter(|_|self.graph.content.as_ref()==Some(&cell.expression))
            .map(|parts|[parts.nodes[0].clone(),parts.nodes[2].clone()]).into_iter().flatten().filter(|node|!graph::Expression::is_clear(node)))
            .map(|expression|pages::Key {expression,..cell.clone()}).collect()
    }
    fn continues_admitted(&self,key:&pages::Key)->bool {self.streaming.units.iter().any(|unit|self.cell_parts(&unit.interest.key).contains(key))}
    fn keep_committed_halves(&self,key:&pages::Key) {
        let Some(unit)=self.streaming.units.iter().find(|unit|unit.interest.key==*key) else {return;};
        let mut store=self.pages.lock().unwrap();
        let committed=self.cell_parts(key).into_iter().skip(1).filter(|half|store.has(half,half.plan().bounds,pages::Accuracy::Exact,0)).collect::<Vec<_>>();
        store.keep_outputs(&unit.interest,&committed);
    }
    pub(super) fn streamed_interests(&self,view:layer_render::ViewState)->impl Iterator<Item=pages::Interest>+'_ {
        let visible=display_mips::view_bounds(view,self.plan.extent,0).unwrap_or(PixelRect::EMPTY);
        self.streaming.units.iter().filter(move|unit|unit.view==view || !unit.interest.key.plan().bounds.intersect(visible).is_empty()).map(|unit|unit.interest.clone())
    }

    #[expect(clippy::too_many_arguments, reason = "The pivot's display sources share the frame's scene, encoding and budget with the exact cell they follow")]
    fn prepare_pivot(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget, region: PixelRect, class: &dyn Fn(PixelRect)->Class, allowance: Allowance,
    ) -> Result<sources::Prepared, GpuRasterError> {
        let mut prepared=sources::Prepared::default();
        if self.graph.parts.is_none() || self.pivot_missing(region) {return Ok(prepared);}
        let root=self.graph.root.clone().unwrap();
        let tiled=use_tiles(r,self.plan,packet,Some(scene)) && !(self.plan.level>0 && root.fused_transform(r));
        for (id,request) in self.region_sources(scene,r,packet,region,tiled) {
            if prepared.refusal.is_some() {break;}
            if !self.source_ready(scene,r,id,&request) {prepared=prepared.or(self.prepare_source(scene,r,packet,encoder,id,request,class(region),allowance,budget)?);}
        }
        Ok(prepared)
    }

    #[expect(clippy::too_many_arguments, reason = "An exact unit shares the frame's scene, encoding and budget with its cell")]
    fn exact_unit(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget, pass: &mut Pass, pinned: &mut Vec<pages::Held>,
        key: &pages::Key, class: &dyn Fn(PixelRect)->Class, allowance: Allowance,
    ) -> Result<Outcome, GpuRasterError> {
        let region=key.plan().bounds;
        let Some(graph::Parts {nodes,scopes})=self.graph.parts.as_ref().filter(|_|self.graph.content.as_ref()==Some(&key.expression) && self.folds_halves(key,class(region)))
            .map(|parts|graph::Parts {nodes:parts.nodes.clone(),scopes:parts.scopes.clone()}) else {
            let scope=self.graph.parts.as_ref().and_then(|parts|parts.nodes.iter().position(|node|*node==key.expression).map(|index|parts.scopes[index].clone()));
            return self.exact_capture(scene,r,packet,encoder,budget,pass,pinned,key,scope.as_ref(),None,class,allowance);
        };
        let mut halves=[None,None];
        for (half,index) in [(0,0),(1,2)] {
            if graph::Expression::is_clear(&nodes[index]) {continue;}
            let part=pages::Key {expression:nodes[index].clone(),..key.clone()};
            if self.pages.lock().unwrap().get(&part,region,pages::Accuracy::Exact,0,class(region)).is_none() {
                match self.exact_capture(scene,r,packet,encoder,budget,pass,pinned,&part,Some(&scopes[index]),None,class,allowance)? {
                    Outcome::Committed=>{},
                    outcome=>{if matches!(outcome,Outcome::Refused(..)) {self.keep_committed_halves(key);}return Ok(outcome);},
                }
            }
            halves[half]=Some(self.pages.lock().unwrap().get(&part,region,pages::Accuracy::Exact,0,class(region))
                .ok_or(GpuRasterError::Invariant("a committed exact part is missing"))?);
        }
        self.exact_capture(scene,r,packet,encoder,budget,pass,pinned,key,Some(&scopes[1]),Some(halves),class,allowance)
    }

    #[expect(clippy::too_many_arguments, reason = "An exact capture shares the frame's scene, encoding and budget with its cell")]
    fn exact_capture(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget, pass: &mut Pass, pinned: &mut Vec<pages::Held>,
        key: &pages::Key, scope: Option<&layer_core::authored::SceneScope>, halves: Option<[Option<pages::Held>;2]>, class: &dyn Fn(PixelRect)->Class, allowance: Allowance,
    ) -> Result<Outcome, GpuRasterError> {
        let admitted=self.streaming.units.iter().position(|unit|unit.interest.key==*key);
        let bounded=!pass.owed && !self.continues_admitted(key);
        if bounded && pass.closed.is_none() && self.full(r,budget,self.streaming.turned) {pass.closed=Some(Closed::Full);}
        if bounded && let Some(closed)=pass.closed.as_ref().filter(|closed|closed.applies(class(key.plan().bounds),allowance)) {return Ok(self.flight(key.plan().bounds,closed));}
        let packet=match scope {Some(scope)=>FramePacket {scene:packet.scene.with_scope(scope),..packet},None=>packet};
        let passes=packet.scene.order().iter().filter_map(|handle|packet.scene.effect(*handle).filter(|effect|
            effect.program.image_boundary() && packet.scene.visible(*handle)))
            .map(|effect|effect.program.passes.len().max(1) as u64).sum::<u64>();
        let region=key.plan().bounds;
        let window=scene.cached_capture_window(packet.scene,region);
        let plan=key.plan();
        let destination=Image {texture:r.empty_texture.clone(),view:r.empty_view.clone(),plan};
        let prepared=r.ensure_exact_preview(packet.scene,encoder).and_then(|()|scene.prepare_region(r,packet,(region,window),PixelRect::EMPTY,false,encoder))
            .and_then(|()|scene.declare_capture(r,packet,&destination,&[region],scene::Output::Display,None,None));
        let inputs=match prepared {
            Ok(inputs)=>inputs,
            Err(error@(GpuRasterError::NotReady|GpuRasterError::Waiting(_)|GpuRasterError::NotPublished))=>return Ok(waiting_on(scene,r,self.declarations(),&error).map_or(Outcome::Deferred,Outcome::Waiting)),
            Err(GpuRasterError::SourceWorkingSetExceeded)=>return Ok(Outcome::Skipped),
            Err(error)=>return Err(error),
        };
        let folds=halves.as_ref().map_or(0,|halves|halves.iter().flatten().count() as u64+1);
        let work=region.area().saturating_mul((inputs.reads()+folds).max(1)).saturating_add(window.area().saturating_mul(passes));
        let scratch=self.graph.root.as_ref().unwrap().cost().1 as usize+1;
        let interest=Self::exact_interest(key.clone(),region);
        if bounded && self.full(r,budget,work) {
            self.streaming.turned=work;pass.closed=Some(Closed::Full);
            return Ok(self.flight(region,&Closed::Full));
        }
        let parts=if halves.is_some() {self.cell_parts(key)}else {vec![key.clone()]};
        let outputs=parts.into_iter().map(|part|(part,region)).collect();
        #[cfg(test)] crate::test_support::PassCounts {destination_admissions:1,..Default::default()}.record();
        let handle=match self.accept_working(scene,r,interest.clone(),outputs,inputs,[scratch,0],plan,true,class(region),class(region),allowance,None,Default::default(),encoder)? {
            Ok(handle)=>handle,
            Err(None)=>return Ok(waiting_on(scene,r,self.declarations(),&GpuRasterError::NotReady).map_or(Outcome::Deferred,Outcome::Waiting)),
            Err(Some(refusal))=>{let refusal=refusal.alone();if allowance==Allowance::Fresh {pass.closed=Some(Closed::Refused(refusal.clone()));}return Ok(Outcome::Refused(region,refusal));},
        };
        let unit=admitted.unwrap_or_else(||{self.streaming.units.push(Admitted {interest,view:packet.view,work,waiting:None});self.streaming.units.len()-1});
        if pass.started && bounded && work>budget.remaining() {self.streaming.units[unit].waiting=None;return Ok(Outcome::Deferred);}
        #[cfg(test)] crate::test_support::PassCounts {destination_units:1,..Default::default()}.record();
        let image=self.pages.lock().unwrap().allocated(key,class(region)).expect("accepted exact page");
        let write=self.pages.lock().unwrap().prepare_write(key.clone(),region,pages::Accuracy::Exact,0)?;
        let mut held=self.pages.lock().unwrap().take_inputs(&handle)?;
        held.clone().track(encoder);
        let result=(|| {
            #[cfg(test)]
            let _record=crate::scene::sources::record_reads(&held);
            let accepted=&mut held;
            scene.capture_prepared_regions(r,packet,&image,&[region],scene::Output::Display,false,None,encoder, accepted)?;
            if let Some(halves)=&halves {scene.fold_over(r,encoder,&image,halves)?;}
            Ok(())
        })();
        match result {
            Ok(())=>{
                budget.spend(work.max(object_cache::PASS_TAPS));pass.started=true;
                self.streaming.units.remove(unit);
                self.pages.lock().unwrap().commit(write,work,encoder);
                self.retain_exact(key);
                pinned.extend(halves.into_iter().flatten().flatten());
                r.metrics.composited_pixels+=region.area();r.metrics.frame_composited_pages.push((0,key.coordinate));
                pinned.push(image);
                self.pages.lock().unwrap().finish_inputs(&handle);
                Ok(Outcome::Committed)
            },
            Err(error@(GpuRasterError::NotReady|GpuRasterError::Waiting(_)|GpuRasterError::NotPublished))=>{
                self.pages.lock().unwrap().hold_inputs(&handle,held)?;
                let wait=waiting_on(scene,r,self.declarations(),&error);self.streaming.units[unit].waiting=wait.clone();
                Ok(wait.map_or(Outcome::Deferred,Outcome::Waiting))
            },
            Err(GpuRasterError::SourceWorkingSetExceeded)=>{self.streaming.units.remove(unit);self.pages.lock().unwrap().finish_inputs(&handle);Ok(Outcome::Skipped)},
            Err(error)=>Err(error),
        }
    }

    fn project_exact(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,commands:&mut RegionRecords)->Result<PixelRect,GpuRasterError> {
        let view=self.copy_level(scene,r,encoder,commands)?;
        self.underived.add(view);
        Ok(self.presented(view,PixelRect::EMPTY))
    }
}

impl Scene {
    fn fold_over(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,root:&pages::Held,[above,below]:&[Option<pages::Held>;2])->Result<(),GpuRasterError> {
        let over=|front:Option<&wgpu::TextureView>,back:Option<&wgpu::TextureView>,output:&wgpu::TextureView| {
            let mut values=[0;20];
            values[..8].copy_from_slice(&[0,0,root.plan.size[0],root.plan.size[1],0,0,1,if back.is_some() {2}else {0}]);
            values[12]=(if front.is_some() {1f32}else {0.}).to_bits();
            values[14]=1f32.to_bits();
            let sources=[front,back,None,None].map(|source|source.unwrap_or(&r.empty_view).clone());
            Job::Compose {values,output:output.clone(),sources,_leases:Vec::new(),format:output.texture().format()}
        };
        let slot=self.reserve(r)?;
        let middle=self.pool[slot].view.clone();
        self.jobs.push(over(Some(&root.view),below.as_ref().map(|page|&page.view),&middle));
        self.jobs.push(over(above.as_ref().map(|page|&page.view),Some(&middle),&root.view));
        let result=self.encode_jobs(r,encoder);
        self.free(slot);
        result
    }
    pub fn refine_display(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget,
    ) -> Result<PixelRect, GpuRasterError> {
        let write = self.begin_write(r,encoder)?;
        let Some(mut cache) = r.scale_display.take() else { return Ok(PixelRect::EMPTY) };
        cache.submission_valid = Some(self.valid.clone());
        let mut commands = self.scale_commands.take().unwrap_or_else(|| RegionRecords::charged(r, &self.tiers.gpu));
        commands.begin();
        let result = cache.refine(self, r, packet, encoder, &mut commands, budget);
        self.scale_commands = Some(commands);
        r.scale_display = Some(cache);
        if matches!(&result, Err(error) if error.is_preparation_pending()) { self.clear_unencoded_jobs(); }
        if result.is_ok() || matches!(&result,Err(error) if error.is_preparation_pending()) {write.track(encoder);}
        result
    }
}


#[cfg(all(test, not(target_arch="wasm32")))]
mod tests {
    use super::*;
    use crate::frame_demand::{Input, Turn, UnitWait};
    use crate::residency::Search;
    use crate::test_support::{PassCounts, complete, packet, submit_settled, work};

    #[test]
    fn a_spent_exact_capture_enters_a_refused_pass_without_searching_again() {
        let extent = [512, 256];
        let document = super::super::tests::document_at(extent);
        let frame = packet(document.scene(), extent);
        let mut r = crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        submit_settled(&mut r, frame);
        complete(&r);

        let mut scene = r.scene.take().unwrap();
        let mut cache = r.scale_display.take().unwrap();
        let region = PixelRect::new(0, 0, PAGE_SIZE, PAGE_SIZE);
        let key = cache.refinement_interest(0, region).key;
        pages::evict_composite_pages_where(&cache, &[region], |page| page.level == 0);
        let tier = cache.pages.lock().unwrap().tier();
        let mut frontier = Vec::new();
        cache.pages.lock().unwrap().refinement_frontier(&key, &mut frontier);
        assert!(frontier == vec![key.clone()], "the exact leaf still needs capture");
        tier.update_limit(0);

        let reclaim = Search::open([]);
        let retirement = reclaim.hold();
        let need = tier.wanting(Class::Refinement, 1).reclaiming(Some(reclaim.seal()));
        let wait = UnitWait::parked(need.clone(), Allowance::Fresh);
        assert_eq!(wait.observe(&r), Turn::Sleep(Input::Residency));
        drop(retirement);
        assert_eq!(wait.observe(&r), Turn::Enter(Allowance::Spent));
        let allowance = wait.eligible(&r).unwrap();

        for closed in [false, true] {
            let earlier = Search::open([]);
            let earlier_retirement = earlier.hold();
            let sibling = tier.wanting(Class::Refinement, 1).reclaiming(Some(earlier.seal()));
            let mut pass = Pass {
                started: false,
                owed: false,
                closed: closed.then(|| Closed::Refused(sibling.clone())),
            };
            assert_eq!(pass.closed.as_ref().is_some_and(|closed| closed.applies(Class::Refinement, Allowance::Fresh)), closed);
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            let before = PassCounts::snapshot();
            let mut pinned = Vec::new();
            let outcome = {
                let mut budget = work(false);
                cache.exact_capture(&mut scene, &mut r, frame, &mut encoder, &mut budget, &mut pass,
                    &mut pinned, &key, None, None, &|_|Class::Refinement, allowance).unwrap()
            };
            let counts = PassCounts::snapshot().delta(before);
            assert_eq!(counts.destination_admissions, 1, "the spent unit gets its own admission turn even after a sibling's refusal");
            assert_eq!(counts.room_scans, 0, "a spent turn cannot start another search");
            assert_eq!(counts.destination_units, 0, "the refused unit encoded no capture");
            let Outcome::Refused(refused_region, refusal) = outcome else {panic!("the exact capture must refuse without room");};
            assert_eq!(refused_region, region);
            assert!(refusal.waiting().is_none(), "a spent refusal owns no new reclaim batch");
            match (&pass.closed, closed) {
                (Some(Closed::Refused(saved)), true) => assert!(saved.same(&sibling), "the earlier refusal still owns the pass closure"),
                (None, false) => {},
                _ => panic!("a spent refusal cannot close the pass against Fresh siblings"),
            }
            drop(earlier_retirement);
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            complete(&r);
        }
    }
}
