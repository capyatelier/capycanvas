use super::*;

pub(super) fn exact_at_view_level(scene: SceneView<'_>) -> bool {
    scene.order().iter().any(|&owner| scene.object_layer(owner).is_some() || scene.vector_layer(owner).is_some())
}

pub(super) fn exact_input_level(scene: SceneView<'_>, required_input_level: u32) -> u32 {
    if exact_at_view_level(scene) {required_input_level} else {0}
}

pub(super) fn presented_accuracy(r: &WgpuRasterizer, scene: SceneView<'_>, preview: bool) -> pages::Accuracy {
    if !preview && r.moving_layer.is_none() && exact_at_view_level(scene) {pages::Accuracy::Exact} else {pages::Accuracy::Display}
}

pub(super) fn streams_exact(cache: &Cache, r: &WgpuRasterizer, packet: FramePacket<'_>, presented: pages::Accuracy) -> bool {
    presented == pages::Accuracy::Display && cache.plan.level > 0 && refining(r) && !exact_at_view_level(packet.scene)
        && packet.dab_batches.is_empty() && cache.graph.motion.is_none() && cache.graph.prepared.is_none()
        && !packet.scene.order().iter().any(|&handle| packet.scene.visible(handle) && packet.scene.effect(handle).is_some_and(|effect| effect.animated()))
}

fn refining(r: &WgpuRasterizer) -> bool {
    r.moving_layer.is_none() && !r.transform_preview.as_ref().is_some_and(|preview| preview.moving)
}

fn fold_kernels(r: &WgpuRasterizer) -> [&Deferred<wgpu::ComputePipeline>; 5] {
    r.scene_pipelines.scale_pipelines(r.device.working_format())
}

enum Outcome {Committed,Waiting(crate::frame_demand::Input),Refused(PixelRect,crate::residency::Blocked),Skipped,Deferred}

struct Admitted {interest:pages::Interest,view:layer_render::ViewState,work:u64,waiting:Option<crate::frame_demand::Input>}

#[derive(Default)]
pub(super) struct Streaming {cells:Vec<(pages::Key,Vec<pages::Held>)>,units:Vec<Admitted>,turned:u64}
impl Streaming {
    fn work(&self)->u64 {self.units.iter().map(|unit|unit.work).sum()}
}

#[derive(Clone)]
enum Closed {Full,Refused(crate::residency::Blocked)}
impl Closed {
    fn applies(&self,class:Class)->bool {match self {Self::Full=>true,Self::Refused(refusal)=>class>=refusal.class()}}
}

struct Pass {started:bool,owed:bool,closed:Option<Closed>}

#[derive(Default)]
pub(super) struct Exact {pub(super) committed:bool,pub(super) blocks:Vec<(pages::Key,Option<crate::frame_demand::Input>)>,pub(super) refusals:Vec<(PixelRect,crate::residency::Blocked)>,
    pub(super) pivot_refusal:Option<crate::residency::Blocked>}
impl Exact {
    fn refuse(&mut self,region:PixelRect,refusal:crate::residency::Blocked,pivot:bool) {
        if pivot {self.pivot_refusal.get_or_insert_with(||refusal.clone());}
        self.refusals.push((region,refusal));
    }
}

impl Cache {
    pub fn approximating(&self) -> bool { !self.output_complete() || self.preparing || self.overview.as_ref().is_some_and(|overview| overview.approximating()) }


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
        use crate::frame_demand::Outstanding::Runnable;
        if self.contact || self.preparing {return Some(Runnable);}
        self.cells_demand(r,refining(r)).or_else(||self.pixels.committed_root().is_none().then(||self.residual_demand(r)))
    }

    pub(super) fn release_streaming(&mut self,encoder:&crate::submission::CommandEncoder) {
        for page in std::mem::take(&mut self.streaming).cells.into_iter().flat_map(|(_,pins)|pins) {page.track(encoder);}
    }
    fn current(&self,cell:&pages::Key)->bool {
        std::iter::once(self).chain(self.overview.as_deref()).any(|cache|cache.graph.content.as_ref()==Some(&cell.expression))
            || self.pivot_halves().contains(&cell.expression)
    }
    fn full(&self,r:&WgpuRasterizer,budget:&WorkBudget,work:u64)->bool {
        let flight=std::iter::once(self).chain(r.scale_display.as_ref()).chain(r.navigator.cache()).map(|cache|cache.streaming.work()).sum::<u64>();
        flight!=0 && flight.saturating_add(budget.spent()).saturating_add(work)>budget.capacity()
    }
    fn flight(&self,region:PixelRect,closed:&Closed)->Outcome {
        match closed {
            Closed::Refused(refusal)=>Outcome::Refused(region,refusal.clone()),
            Closed::Full=>self.streaming.units.iter().map(|unit|unit.waiting).collect::<Option<Vec<_>>>()
                .and_then(|waiting|waiting.first().copied()).map_or(Outcome::Deferred,Outcome::Waiting),
        }
    }
    fn retain_exact(&self,key:&pages::Key) {
        if key.level!=self.plan.level || self.graph.content.as_ref()!=Some(&key.expression) && !self.pivot_halves().contains(&key.expression) {self.pages.lock().unwrap().set_optional(key);}
    }
    pub(super) fn pivot_missing(&self,region:PixelRect)->bool {
        let halves=self.pivot_halves().len();
        halves>0 && self.half_cells(region).len()<pages::coordinates(region.intersect(self.plan.bounds),self.plan.level).count()
    }
    pub(super) fn note_pivot_edit(&mut self) {
        let Some((owner,active))=self.graph.active().zip(self.graph.parts.as_ref().map(|parts|parts.nodes[1].clone())) else {return;};
        let Some(stamp)=self.pages.lock().unwrap().expected().and_then(|frame|frame.authored_revision(&active).ok()) else {return;};
        self.pivot_edit=Some(match self.pivot_edit {
            Some((held,base,edited)) if held==owner=>(held,base,edited || base!=stamp),
            selected=>(owner,stamp,selected.is_some()),
        });
    }
    pub(super) fn pivot_edited(&self)->bool {self.pivot_edit.is_some_and(|(_,_,edited)|edited)}
    fn folds_halves(&self,key:&pages::Key,class:Class)->bool {
        let halves=self.pivot_halves();
        let region=key.plan().bounds;
        !halves.is_empty() && (self.pivot_edited() && class==Class::Refinement || halves.into_iter().all(|half|
            self.pages.lock().unwrap().get(&pages::Key {expression:half,..key.clone()},region,pages::Accuracy::Exact,0,class).is_some()))
    }
    pub(super) fn wanted_halves(&self,region:PixelRect,view:layer_render::ViewState)->Vec<pages::Key> {
        let halves=self.pivot_halves();
        let mut cells=pages::coordinates(region.intersect(self.plan.bounds),self.plan.level).map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent)).collect::<Vec<_>>();
        let near=self.pointer.map_or(view,|point|centred(view,point));
        cells.sort_by_key(|cell|centre_distance(near,*cell));
        cells.into_iter().flat_map(|cell|{let cell=self.refinement_interest(self.plan.level,cell).key;halves.iter().map(move|half|pages::Key {expression:half.clone(),..cell.clone()})}).collect()
    }
    pub(super) fn pivot_halves(&self)->Vec<graph::Node> {
        self.graph.parts.iter().flat_map(|parts|[&parts.nodes[0],&parts.nodes[2]]).filter(|node|!graph::Expression::is_clear(node) && self.graph.caches(node)).cloned().collect()
    }
    fn pivot_demand(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::Outstanding::{Blocked,Runnable};
        if !refining(r) || self.gesture.is_some() || self.graph.parts.is_none() {return None;}
        if self.pivot_refusal.as_ref().is_some_and(|refusal|r.refuses(refusal)) {return None;}
        let halves=if self.pivot_edited() {self.pivot_halves()}else {Vec::new()};
        if halves.is_empty() {return self.pivot_input(r);}
        let held=self.half_cells(self.plan.bounds);
        let mut naming=false;
        for coordinate in pages::coordinates(self.plan.bounds,self.plan.level) {
            let region=pages::bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds);
            if held.contains(&region) || self.refused(r,region) {continue;}
            let cell=self.refinement_interest(self.plan.level,region).key;
            for half in &halves {
                let mut frontier=Vec::new();
                self.pages.lock().unwrap().refinement_frontier(&pages::Key {expression:half.clone(),..cell.clone()},usize::MAX,&mut frontier,0);
                if !frontier.is_empty() {return Some(match self.pivot_waiting {Some((input,since)) if pages::waits(r,input,since)=>Blocked(input),_=>Runnable});}
            }
            naming=true;
        }
        if naming {return Some(Blocked(crate::frame_demand::Input::Name));}
        self.pivot_input(r)
    }
    fn pivot_input(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::{Input,Outstanding::{Blocked,Runnable}};
        let folding=!self.pivot_halves().is_empty() && !fold_kernels(r).iter().all(|kernel|kernel.ready());
        self.pivot_pending.then(||if folding {Blocked(Input::Compile)}else if r.image_decode_waiting() {Blocked(Input::Decode)}else {Runnable})
    }
    pub(super) fn pivot_inputs_pending(&self,scene:&Scene,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        if self.graph.parts.is_none() || self.pivot_missing(self.plan.bounds) {return false;}
        let root=self.graph.root.clone().unwrap();
        let tiled=use_tiles(r,self.plan,packet,Some(scene)) && !(self.plan.level>0 && root.fused_transform(r));
        !self.pivot_halves().is_empty() && !r.compilation.require(fold_kernels(r),crate::startup::DOCUMENT)
            || self.region_sources(scene,r,packet,self.plan.bounds,tiled).iter().any(|(id,request)|!self.source_ready(scene,r,*id,request))
    }

    pub(super) fn refine(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords, budget: &mut WorkBudget,
    ) -> Result<PixelRect, GpuRasterError> {
        if exact_at_view_level(packet.scene) || self.gesture.is_some() || !refining(r) {return Ok(PixelRect::EMPTY);}
        let since=r.arrival_token();
        let mut keys=Vec::new();
        for cache in std::iter::once(&*self).chain(self.overview.as_deref()).filter(|cache|cache.graph.root.is_some()) {
            let mut coordinates=pages::coordinates(cache.plan.bounds,cache.plan.level).collect::<Vec<_>>();
            coordinates.sort_by_key(|coordinate|centre_distance(packet.view,pages::bounds(*coordinate,cache.plan.level,cache.plan.extent)));
            keys.extend(coordinates.into_iter().map(|coordinate|cache.refinement_interest(cache.plan.level,pages::bounds(coordinate,cache.plan.level,cache.plan.extent)).key));
        }
        let halves=if self.pivot_edited() {self.pivot_halves()}else {Vec::new()};
        if !halves.is_empty() {keys.extend(self.wanted_halves(self.plan.bounds,packet.view));}
        let mut exact=self.exact_units(scene,r,packet,encoder,commands,budget,keys,&|_|Class::Refinement,false)?;
        self.pivot_refusal=exact.pivot_refusal.take();
        self.pivot_waiting=exact.blocks.iter().find(|(key,_)|halves.contains(&key.expression)).and_then(|(_,input)|input.map(|input|(input,since)));
        for cache in std::iter::once(&*self).chain(self.overview.as_deref()) {
            for (key,input) in exact.blocks.iter().filter(|(key,_)|key.level==cache.plan.level) {cache.block(since,key.plan().bounds,*input);}
        }
        for (region,refusal) in &exact.refusals {self.refuse(None,*region,refusal);}
        self.pivot_pending=self.pivot_inputs_pending(scene,r,packet);
        if self.pivot_pending {
            let refusal=self.prepare_pivot(scene,r,packet,encoder,budget,self.plan.bounds,&|_|Class::Refinement)?;
            self.pivot_refusal=self.pivot_refusal.take().or(refusal);
            self.pivot_pending=self.pivot_inputs_pending(scene,r,packet);
        }
        if exact.committed {self.project_exact(scene,r,encoder,commands)}else {Ok(PixelRect::EMPTY)}
    }

    #[expect(clippy::too_many_arguments, reason = "Exact units share the frame's scene, encoding and budget with the pass that selected their cells")]
    pub(super) fn exact_units(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords, budget: &mut WorkBudget,
        cells: Vec<pages::Key>, class: &dyn Fn(PixelRect)->Class, owed: bool,
    ) -> Result<Exact, GpuRasterError> {
        let mut exact=Exact::default();
        let halves=self.pivot_halves();
        let mut outcomes=std::collections::HashMap::<pages::Key,(bool,Option<crate::frame_demand::Input>)>::new();
        {let units=std::mem::take(&mut self.streaming.units);let pages=self.pages.lock().unwrap();
            self.streaming.units=units.into_iter().filter(|unit|self.current(&unit.interest.key) && pages.accepted(&unit.interest.key,unit.interest.key.plan().bounds,0)).collect();}
        let mut previous=std::mem::take(&mut self.streaming.cells);
        let admitted=self.streaming.units.iter().map(|unit|unit.interest.key.clone()).filter(|unit|cells.iter().any(|cell|cell.expression==unit.expression)).collect::<Vec<_>>();
        let mut pass=Pass {started:false,owed,closed:None};
        let flying=admitted.len();
        for (index,cell) in admitted.into_iter().chain(cells).enumerate() {
            if index>=flying && std::iter::once(&*self).chain(self.overview.as_deref()).any(|cache|cache.plan.level==cell.level && cache.refused(r,cell.plan().bounds)) {continue;}
            let carried=previous.iter().position(|(key,_)|*key==cell).map(|index|previous.swap_remove(index).1).unwrap_or_default();
            if index>=flying && let Some(closed)=pass.closed.as_ref().filter(|closed|closed.applies(class(cell.plan().bounds))) {
                match self.flight(cell.plan().bounds,closed) {
                    Outcome::Waiting(input)=>exact.blocks.push((cell.clone(),Some(input))),
                    Outcome::Refused(region,refusal)=>exact.refuse(region,refusal,halves.contains(&cell.expression)),
                    _=>{},
                }
                if self.has_admitted_unit(&cell) {self.streaming.cells.push((cell,carried));}else {for page in carried {page.track(encoder);}}
                continue;
            }
            for page in carried {page.track(encoder);}
            let mut frontier=Vec::new();
            self.pages.lock().unwrap().refinement_frontier(&cell,usize::MAX,&mut frontier,0);
            if frontier.is_empty() {continue;}
            let (mut pending,mut waiting)=(false,None);
            let mut pinned=self.exact_children(&cell,class);
            for key in frontier {
                if let Some((held,outcome))=outcomes.get(&key) {pending|=*held;waiting=waiting.or(*outcome);continue;}
                let outcome=if key.level>0 {
                    #[cfg(test)] crate::test_support::PassCounts {destination_units:1,..Default::default()}.record();
                    self.reduce_exact(scene,r,encoder,commands,key.clone(),class)?
                } else {
                    self.exact_unit(scene,r,packet,encoder,commands,budget,&mut pass,&mut pinned,&key,class)?
                };
                let input=match outcome {
                    Outcome::Committed=>{exact.committed=true;None},
                    Outcome::Waiting(input)=>Some(input),
                    Outcome::Deferred=>None,
                    Outcome::Refused(region,refusal)=>{exact.refuse(region,refusal,halves.contains(&cell.expression));continue;},
                    Outcome::Skipped=>continue,
                };
                let held=!matches!(outcome,Outcome::Committed);
                pending|=held;waiting=waiting.or(input);
                outcomes.insert(key,(held,input));
            }
            if self.has_admitted_unit(&cell) {
                let pins=self.exact_children(&cell,class);
                if !pins.is_empty() {self.streaming.cells.push((cell.clone(),pins));}
            }
            for page in pinned {page.track(encoder);}
            if waiting.is_some() {exact.blocks.push((cell,waiting));}
            else if !pending && cell.level==self.plan.level && let Some(refusal)=self.prepare_pivot(scene,r,packet,encoder,budget,cell.plan().bounds,class)? {
                exact.pivot_refusal.get_or_insert(refusal);
            }
        }
        for (cell,pins) in previous {
            if self.current(&cell) && self.has_admitted_unit(&cell) {self.streaming.cells.push((cell,pins));}else {for page in pins {page.track(encoder);}}
        }
        Ok(exact)
    }

    fn cell_parts(&self,cell:&pages::Key)->Vec<pages::Key> {
        std::iter::once(cell.expression.clone()).chain(self.graph.parts.as_ref().filter(|_|self.graph.content.as_ref()==Some(&cell.expression))
            .map(|parts|[parts.nodes[0].clone(),parts.nodes[2].clone()]).into_iter().flatten().filter(|node|!graph::Expression::is_clear(node)))
            .map(|expression|pages::Key {expression,..cell.clone()}).collect()
    }
    fn has_admitted_unit(&self,cell:&pages::Key)->bool {
        let parts=self.cell_parts(cell);
        self.streaming.units.iter().map(|unit|&unit.interest.key).any(|unit|unit.level<=cell.level
            && unit.coordinate.map(|value|value>>(cell.level-unit.level))==cell.coordinate && parts.iter().any(|part|part.expression==unit.expression))
    }
    fn continues_admitted(&self,key:&pages::Key)->bool {self.streaming.units.iter().any(|unit|self.cell_parts(&unit.interest.key).contains(key))}
    fn exact_level(&self)->u32 {self.plan.level.max(self.overview.as_ref().map_or(0,|cache|cache.plan.level))}
    fn ancestors(&self,r:&WgpuRasterizer,key:&pages::Key)->Vec<pages::Key> {
        let level=std::iter::once(self).chain(r.scale_display.as_ref()).chain(r.navigator.cache()).map(Cache::exact_level).fold(0,u32::max);
        self.cell_parts(key).into_iter().flat_map(|part|(part.level+1..=level)
            .map(move|level|pages::Key {level,coordinate:part.coordinate.map(|value|value>>(level-part.level)),..part.clone()})).collect()
    }
    fn exact_children(&self,cell:&pages::Key,class:&dyn Fn(PixelRect)->Class)->Vec<pages::Held> {
        let parts=self.cell_parts(cell);
        let mut store=self.pages.lock().unwrap();
        let mut held=Vec::new();
        for part in parts {store.exact_inputs(&part,class(cell.plan().bounds),&mut held);}
        held
    }
    pub(super) fn streamed_interests(&self,view:layer_render::ViewState)->impl Iterator<Item=pages::Interest>+'_ {
        let visible=display_mips::view_bounds(view,self.plan.extent,0).unwrap_or(PixelRect::EMPTY);
        self.streaming.units.iter().filter(move|unit|unit.view==view || !unit.interest.key.plan().bounds.intersect(visible).is_empty()).map(|unit|unit.interest.clone())
    }

    #[expect(clippy::too_many_arguments, reason = "The pivot's display sources share the frame's scene, encoding and budget with the exact cell they follow")]
    fn prepare_pivot(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget, region: PixelRect, class: &dyn Fn(PixelRect)->Class,
    ) -> Result<Option<crate::residency::Blocked>, GpuRasterError> {
        if self.graph.parts.is_none() || self.pivot_missing(region) {return Ok(None);}
        let root=self.graph.root.clone().unwrap();
        let tiled=use_tiles(r,self.plan,packet,Some(scene)) && !(self.plan.level>0 && root.fused_transform(r));
        let mut refusal=None;
        for (id,request) in self.region_sources(scene,r,packet,region,tiled) {
            if !self.source_ready(scene,r,id,&request) {refusal=refusal.or(self.prepare_source(scene,r,packet,encoder,id,request,class(region),budget)?.refusal);}
        }
        Ok(refusal)
    }

    #[expect(clippy::too_many_arguments, reason = "An exact unit shares the frame's scene, encoding and budget with its cell")]
    fn exact_unit(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords, budget: &mut WorkBudget, pass: &mut Pass, pinned: &mut Vec<pages::Held>,
        key: &pages::Key, class: &dyn Fn(PixelRect)->Class,
    ) -> Result<Outcome, GpuRasterError> {
        let region=key.plan().bounds;
        let Some(graph::Parts {nodes,scopes})=self.graph.parts.as_ref().filter(|_|self.graph.content.as_ref()==Some(&key.expression) && self.folds_halves(key,class(region)))
            .map(|parts|graph::Parts {nodes:parts.nodes.clone(),scopes:parts.scopes.clone()}) else {
            let scope=self.graph.parts.as_ref().and_then(|parts|parts.nodes.iter().position(|node|*node==key.expression).map(|index|parts.scopes[index].clone()));
            return self.exact_capture(scene,r,packet,encoder,commands,budget,pass,pinned,key,scope.as_ref(),None,class);
        };
        let mut halves=[None,None];
        for (half,index) in [(0,0),(1,2)] {
            if graph::Expression::is_clear(&nodes[index]) {continue;}
            let part=pages::Key {expression:nodes[index].clone(),..key.clone()};
            if self.pages.lock().unwrap().get(&part,region,pages::Accuracy::Exact,0,class(region)).is_none() {
                match self.exact_capture(scene,r,packet,encoder,commands,budget,pass,pinned,&part,Some(&scopes[index]),None,class)? {
                    Outcome::Committed=>{},
                    outcome=>return Ok(outcome),
                }
            }
            halves[half]=Some(self.pages.lock().unwrap().get(&part,region,pages::Accuracy::Exact,0,class(region))
                .ok_or(GpuRasterError::Invariant("a committed exact part is missing"))?);
        }
        self.exact_capture(scene,r,packet,encoder,commands,budget,pass,pinned,key,Some(&scopes[1]),Some(halves),class)
    }

    #[expect(clippy::too_many_arguments, reason = "An exact capture shares the frame's scene, encoding and budget with its cell")]
    fn exact_capture(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords, budget: &mut WorkBudget, pass: &mut Pass, pinned: &mut Vec<pages::Held>,
        key: &pages::Key, scope: Option<&layer_core::authored::SceneScope>, halves: Option<[Option<pages::Held>;2]>, class: &dyn Fn(PixelRect)->Class,
    ) -> Result<Outcome, GpuRasterError> {
        let admitted=self.streaming.units.iter().position(|unit|unit.interest.key==*key);
        let bounded=!pass.owed && !self.continues_admitted(key);
        if bounded && pass.closed.is_none() && self.full(r,budget,self.streaming.turned) {pass.closed=Some(Closed::Full);}
        if bounded && let Some(closed)=pass.closed.as_ref().filter(|closed|closed.applies(class(key.plan().bounds))) {return Ok(self.flight(key.plan().bounds,closed));}
        let packet=match scope {Some(scope)=>FramePacket {scene:packet.scene.with_scope(scope),..packet},None=>packet};
        let passes=packet.scene.order().iter().filter_map(|handle|packet.scene.effect(*handle).filter(|effect|
            effect.program.image_boundary() && packet.scene.visible(*handle)))
            .map(|effect|effect.program.passes.len().max(1) as u64).sum::<u64>();
        let region=key.plan().bounds;
        let window=scene.cached_capture_window(packet.scene,region);
        let plan=key.plan();
        let destination=Image {texture:r.empty_texture.clone(),view:r.empty_view.clone(),plan};
        let prepared=r.ensure_exact_preview(encoder).and_then(|()|scene.prepare_region(r,packet,(region,window),PixelRect::EMPTY,false,encoder))
            .and_then(|()|scene.declare_capture(r,packet,&destination,&[region],scene::Output::Display,None,None));
        let inputs=match prepared {
            Ok(inputs)=>inputs,
            Err(error@(GpuRasterError::NotReady|GpuRasterError::NotPublished))=>return Ok(waiting_input(scene,r,error==GpuRasterError::NotPublished).map_or(Outcome::Deferred,Outcome::Waiting)),
            Err(GpuRasterError::SourceWorkingSetExceeded)=>return Ok(Outcome::Skipped),
            Err(error)=>return Err(error),
        };
        let folds=halves.as_ref().map_or(0,|halves|halves.iter().flatten().count() as u64+1);
        let identity=folds==0 && inputs.reads()<=1 && !packet.scene.order().iter().any(|handle|packet.scene.visible(*handle) && packet.scene.effect(*handle).is_some());
        let work=region.area().saturating_mul((inputs.reads()+folds).max(1)).saturating_add(window.area().saturating_mul(passes));
        let scratch=self.graph.root.as_ref().unwrap().cost().1 as usize+1;
        let interest=Self::exact_interest(key.clone(),region);
        if bounded && self.full(r,budget,work) {
            self.streaming.turned=work;pass.closed=Some(Closed::Full);
            return Ok(self.flight(region,&Closed::Full));
        }
        let ancestors=self.ancestors(r,key);
        let parts=if halves.is_some() {self.cell_parts(key)}else {vec![key.clone()]};
        let outputs=parts.into_iter().map(|part|(part,region)).chain(ancestors.iter().map(|ancestor|(ancestor.clone(),ancestor.plan().bounds))).collect();
        let handle=match self.accept_working(scene,r,interest.clone(),outputs,inputs,[scratch,0],plan,true,class(region),class(region),encoder)? {
            Ok(handle)=>handle,
            Err(None)=>return Ok(waiting_input(scene,r,false).map_or(Outcome::Deferred,Outcome::Waiting)),
            Err(Some(refusal))=>{let refusal=refusal.alone();pass.closed=Some(Closed::Refused(refusal.clone()));return Ok(Outcome::Refused(region,refusal));},
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
                let intermediate=key.level<self.plan.level && self.graph.content.as_ref()==Some(&key.expression);
                self.pages.lock().unwrap().commit(write,if intermediate {0}else {work},encoder);
                self.retain_exact(key);
                if intermediate && identity {self.pages.lock().unwrap().set_transient(key);}
                pinned.extend(halves.into_iter().flatten().flatten());
                r.metrics.composited_pixels+=region.area();r.metrics.frame_composited_pages.push((0,key.coordinate));
                pinned.push(image);
                let reduced=ancestors.iter().filter(|parent|parent.level==key.level+1).try_for_each(|parent|
                    match self.reduce_exact(scene,r,encoder,commands,parent.clone(),class)? {
                        Outcome::Refused(..)=>Err(GpuRasterError::Invariant("an admitted exact unit's reserved ancestor was refused")),
                        _=>Ok(()),
                    });
                let mut store=self.pages.lock().unwrap();
                store.finish_inputs(&handle);store.release_unwanted(&ancestors);
                reduced.map(|()|Outcome::Committed)
            },
            Err(error@(GpuRasterError::NotReady|GpuRasterError::NotPublished))=>{
                self.pages.lock().unwrap().hold_inputs(&handle,held)?;
                let input=waiting_input(scene,r,error==GpuRasterError::NotPublished);self.streaming.units[unit].waiting=input;
                Ok(input.map_or(Outcome::Deferred,Outcome::Waiting))
            },
            Err(GpuRasterError::SourceWorkingSetExceeded)=>{self.streaming.units.remove(unit);self.pages.lock().unwrap().finish_inputs(&handle);Ok(Outcome::Skipped)},
            Err(error)=>Err(error),
        }
    }

    fn reduce_exact(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,commands:&mut RegionRecords,
        mut key:pages::Key,class:&dyn Fn(PixelRect)->Class,
    )->Result<Outcome,GpuRasterError> {
        let level=self.exact_level();
        loop {
            let region=key.plan().bounds;
            if self.pages.lock().unwrap().get(&key,region,pages::Accuracy::Exact,0,class(region)).is_none() {
                let inputs=self.pages.lock().unwrap().reduction(&key,0,class(region));
                let Some((inputs,pages::Accuracy::Exact))=inputs else {break;};
                let image=match self.claim_page(scene,r,key.clone(),class(region))? {Ok(image)=>image,Err(blocked)=>return Ok(Outcome::Refused(region,blocked.alone()))};
                let write=self.pages.lock().unwrap().prepare_write(key.clone(),image.plan.bounds,pages::Accuracy::Exact,0)?;
                let work=inputs.len() as u64*object_cache::PASS_TAPS;
                let children=inputs.iter().map(|(child,_)|child.clone()).collect::<Vec<_>>();
                for (_,input) in inputs {
                    let covered=input.plan.bounds.intersect(image.plan.bounds);
                    let [x,y,width,height]=paint_transform::texel_rect(covered.window_local(image.plan.bounds),1<<image.plan.level);
                    let mut values=[0;20];
                    values[..8].copy_from_slice(&[x,y,width,height,input.plan.bounds.width(),input.plan.bounds.height(),2,(input.plan.level<<8)|8]);
                    values[14]=((x*2) as f32).to_bits();values[15]=((y*2) as f32).to_bits();
                    let binding=RegionRecords::binding(r,&input.view,&r.empty_view,&image.view);
                    match commands.reduce(r,encoder,values,&binding,"reduce exact composite page",image.texture.format()) {
                        Err(GpuRasterError::NotReady)=>return Ok(Outcome::Waiting(crate::frame_demand::Input::Compile)),
                        result=>result?,
                    }
                    input.track(encoder);
                }
                self.pages.lock().unwrap().commit(write,work,encoder);
                self.retain_exact(&key);
                let mut store=self.pages.lock().unwrap();
                for child in &children {store.release_transient(child,encoder);}
            }
            if key.level>=level {break;}
            key=pages::Key {level:key.level+1,coordinate:key.coordinate.map(|value|value/2),..key};
        }
        Ok(Outcome::Committed)
    }

    fn project_exact(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,commands:&mut RegionRecords)->Result<PixelRect,GpuRasterError> {
        let mut changed=self.copy_level(scene,r,encoder,commands)?;
        if let Some(cache)=&mut self.overview {changed=changed.union(cache.copy_level(scene,r,encoder,commands)?);}
        Ok(changed)
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
