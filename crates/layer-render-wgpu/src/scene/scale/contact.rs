use super::*;
use crate::IdMap;

pub(super) struct Contact {
    id:u64,
    stroke:layer_core::StrokeId,
    target:SourceTarget,
    plan:display_mips::Plan,
    window:wgpu::Texture,
    root:graph::Node,
    tiled:bool,
    frame:Arc<SignatureFrame>,
    composed:crate::raster::contents::Versions,
    predicted:BTreeSet<[u32;2]>,
    previewed:BTreeSet<[u32;2]>,
    next:Option<(crate::raster::contents::Versions,BTreeSet<[u32;2]>,BTreeSet<[u32;2]>,BTreeSet<[u32;2]>)>,
    footprints:Vec<(PixelRect,Vec<usize>)>,
    tainted:crate::IdSet<usize>,
    guard:Guard,
    generation:std::sync::Mutex<[u64;3]>,
    exit:std::sync::OnceLock<Option<Arc<SignatureFrame>>>,
    preview_level:std::sync::OnceLock<u32>,
    cells:Vec<(PixelRect,crate::scene::sources::Inputs)>,
    inputs:crate::scene::sources::AcceptedInputs,
    pub(super) pages:IdMap<pages::Key,pages::Held>,
    level:Option<u32>,
    stale:BTreeMap<[u32;2],PixelRect>,
    written:IdMap<[u32;2],(pages::Key,PixelRect,submission::Submission)>,
    submissions:Vec<submission::Submission>,
    stamps:Arc<PreviewStamps>,
}

#[derive(PartialEq)]
struct Guard {inspect_mask:Option<OccurrenceHandle>,clipping:[bool;2],blend_space:layer_core::BlendSpace,watercolor:Option<WatercolorLayerStyle>}
impl Guard {
    fn capture(r:&WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,level:&std::sync::OnceLock<u32>)->Option<Self> {
        if let Some(preview)=&r.preview_declaration && (preview.target()!=target || preview.contact_outputs().is_none() || *level.get_or_init(||preview.level())!=preview.level()) {return None;}
        (r.transform_preview.is_none() && r.moving_layer.is_none() && r.vector_preview.is_none() && r.vector_motion.is_none() && r.fill_preview.is_none())
            .then(||Self {inspect_mask:packet.inspect_mask,clipping:r.clipping_preview,blend_space:packet.blend_space,watercolor:r.watercolor_style(target,packet.dab_batches)})
    }
}
fn reads(node:&graph::Node,target:SourceTarget,tainted:&mut crate::IdSet<usize>)->bool {
    let read=match node.as_ref() {
        graph::Expression::Source {id,..}=>*id==target,
        graph::Expression::Opacity {input,..}=>reads(input,target,tainted),
        graph::Expression::Combine {front,back,..}=>reads(front,target,tainted) | reads(back,target,tainted),
        graph::Expression::Effect {input,masks,..}=>masks.iter().flatten().fold(reads(input,target,tainted),|read,mask|reads(mask,target,tainted) | read),
        _=>false,
    };
    if read {tainted.insert(Arc::as_ptr(node) as *const () as usize);}
    read
}

fn stroke(packet:FramePacket<'_>)->Option<(layer_core::StrokeId,SourceTarget)> {
    let first=packet.dab_batches.first()?;
    packet.dab_batches.iter().all(|batch|batch.stroke_id==first.stroke_id && batch.target==first.target
        && matches!(batch.kind,DabBatchKind::Preview|DabBatchKind::Persistent)).then_some((first.stroke_id,first.target))
}

impl Cache {
    fn tiled(&self,r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene,root:&graph::Node)->bool {
        use_tiles(r,self.plan,packet,Some(scene)) && !(self.plan.level>0 && root.fused_transform(r))
    }
    pub(super) fn contact_id(&self)->Option<u64> {self.gesture.as_ref().map(|contact|contact.id)}
    pub(super) fn contact_reads(&self,node:&graph::Node)->bool {
        self.gesture.as_ref().is_some_and(|contact|contact.tainted.contains(&(Arc::as_ptr(node) as *const () as usize)))
    }
    pub(in crate::scene) fn holds_contact_frame(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        let Some(contact)=self.gesture.as_ref().filter(|contact|contact.exit.get().is_none()) else {return false;};
        if self.ends_contact(packet) || !self.contact_holds(r,packet) || !contact.frame.holds_scene(packet.scene)
            || Guard::capture(r,packet,contact.target,&contact.preview_level).as_ref()!=Some(&contact.guard) {return false;}
        let Some(now)=r.raster_generation(contact.target) else {return false;};
        let mut base=contact.generation.lock().unwrap();
        let delta=|index:usize|now[index].checked_sub(base[index]);
        if delta(2).is_none() || delta(0)!=delta(2) || delta(1)!=delta(2) {return false;}
        *base=now;true
    }
    pub(super) fn end_held_frame(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<(),GpuRasterError> {
        if self.gesture.as_ref().is_none_or(|contact|contact.exit.get().is_some()) {return Ok(());}
        self.exit_contact();
        self.pages.lock().unwrap().set_expected(SignatureFrame::prepare(r,packet)?);
        Ok(())
    }
    pub(in crate::scene) fn exit_contact(&self) {
        let Some(contact)=self.gesture.as_ref().filter(|contact|contact.exit.get().is_none()) else {return;};
        let kept=!contact.submissions.iter().chain(contact.written.values().map(|(_,_,submission)|submission)).any(submission::Submission::discarded);
        let frame=kept.then(||Arc::new(contact.frame.composed(contact.target,contact.composed.clone())));
        if let Some(frame)=&frame {
            self.pages.lock().unwrap().set_expected(frame.clone());
            self.key_contact_cells(frame,&contact.predicted);
        }
        drop(contact.exit.set(frame));
    }
    pub(crate) fn contact_stamps(&self,packet:FramePacket<'_>)->Option<Arc<PreviewStamps>> {
        let contact=self.gesture.as_ref()?;
        (!packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty() && packet.dab_batches.iter().all(|batch|batch.stroke_id==contact.stroke
            && batch.target==contact.target && matches!(batch.kind,DabBatchKind::Preview|DabBatchKind::Persistent))).then(||contact.stamps.clone())
    }
    pub(in crate::scene) fn contact_holds(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        let Some(contact)=self.gesture.as_ref() else {return false;};
        let tier=self.pages.lock().unwrap().tier();
        packet.dab_batches.iter().all(|batch|batch.stroke_id==contact.stroke && batch.target==contact.target && matches!(batch.kind,DabBatchKind::Preview|DabBatchKind::Persistent))
            && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty() && r.transform_preview.is_none()
            && self.plan==contact.plan && self.pixels.root().map(|image|&image.texture)==Some(&contact.window) && tier.used()<=tier.limit()
            && !contact.submissions.iter().any(submission::Submission::discarded)
    }
    pub(super) fn ends_contact(&self,packet:FramePacket<'_>)->bool {
        self.gesture.as_ref().is_some_and(|contact|packet.dab_batches.iter().any(|batch|batch.stroke_end && batch.stroke_id==contact.stroke))
    }
    pub(super) fn stroke_only(&mut self,packet:FramePacket<'_>)->bool {
        let frame=self.pages.lock().unwrap().expected().cloned();
        let previous=std::mem::replace(&mut self.stroked,frame.clone());
        self.gesture.is_some() || stroke(packet).zip(previous.zip(frame)).is_some_and(|((_,target),(previous,frame))|frame.contact_continues(&previous,target).is_some())
    }
    #[expect(clippy::too_many_arguments,reason="A contact begins from the frame's composed cells, encoding and budget")]
    pub(super) fn begin_contact(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,retained:u64,budget:&mut WorkBudget,
        touched:&[PixelRect],
    )->Result<(),GpuRasterError> {
        let Some((stroke,target))=stroke(packet).filter(|(_,target)|!target.is_coverage() && !packet.dab_batches.iter().any(|batch|batch.stroke_end)) else {return Ok(());};
        let Some(root)=self.graph.root.clone() else {return Ok(());};
        let tier=self.pages.lock().unwrap().tier();
        if tier.used()>tier.limit() || r.transform_preview.is_some() || r.moving_layer.is_some() || refinement::exact_at_view_level(packet.scene)
            || root.native_capture(packet.scene,self.plan.level) || !self.contact_ready()
            || packet.scene.order().iter().any(|handle|packet.scene.effect(*handle).is_some_and(|effect|effect.animated())) {return Ok(());}
        let (Some(window),Some(frame))=(self.pixels.root().map(|image|image.texture.clone()),self.pages.lock().unwrap().expected().cloned()) else {return Ok(());};
        let preview_level=std::sync::OnceLock::new();
        let (Some(guard),Some(composed),Some(generation))=(Guard::capture(r,packet,target,&preview_level),frame.target_versions(target),r.raster_generation(target)) else {return Ok(());};
        let tiled=self.tiled(r,packet,scene,&root);let bounds=self.plan.bounds;
        let halves=self.held_halves(bounds);
        let ready=pages::coordinates(bounds,self.plan.level).map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent))
            .filter(|cell|!self.pivot_missing(*cell)).collect::<Vec<_>>();
        if touched.iter().any(|region|pages::coordinates(*region,self.plan.level).any(|coordinate|!ready.contains(&pages::bounds(coordinate,self.plan.level,self.plan.extent)))) {return Ok(());}
        let inside:BTreeSet<SourceTarget>=self.pivot_halves().iter().flat_map(|node|node.reads().0).collect();
        let mut requests=self.region_sources(scene,r,packet,bounds,tiled);
        requests.retain(|(id,_)|!inside.contains(id));
        for (id,request) in requests.iter().filter(|(id,request)|!self.source_ready(scene,r,*id,request)).cloned().collect::<Vec<_>>() {
            self.prepare_source(scene,r,packet,encoder,id,request,Class::Visible,budget)?;
        }
        if !requests.iter().all(|(id,request)|self.source_ready(scene,r,*id,request)) {return Ok(());}
        let input=input_plan(self.plan,packet.scene,Some(self.graph.objects()));
        let wet=frame.watercolor(target);
        let mut cells=Vec::new();
        for cell in ready {
            let unit=unit_plan(input,packet.scene,cell,tiled,Some(self.graph.objects()));
            let declared=self.declare_unit(scene,r,packet,cell,unit,tiled,false,retained,Class::Visible,encoder,|compositor| {
                compositor.evaluate_root(&root,Root::Store)?;
                if wet {compositor.accepted.declare_watercolor_stencils(compositor.r,compositor.packet.scene,target)?;}
                Ok(())
            });
            let Some(declared)=declared.ok().filter(|inputs|inputs.objects().is_empty()) else {return Ok(());};
            cells.push((cell,declared));
        }
        let request=requests.iter().find(|(id,_)|*id==target).map(|(_,request)|request);
        let level=request.map(|request|request.plan.level);
        let reductions=match request {Some(request)=>self.declare_reductions(scene,r,packet,target,request,encoder),None=>Ok(Default::default())};
        let Some(inputs)=reductions.ok().and_then(|inputs|inputs.accept_query(r,Some(scene)).ok()) else {return Ok(());};
        let mut areas=IdMap::<pages::Key,(PixelRect,u32)>::default();
        for (id,request) in &requests {
            let source=&scene.scale_sources.entries[id];
            for area in &request.required.regions {for coordinate in pages::coordinates(*area,request.plan.level) {
                let key=source.key(r,*id,request.plan.level,coordinate);
                let region=key.plan().bounds.intersect(*area).intersect(PixelRect::full(key.extent));
                if region.is_empty() {continue;}
                let held=areas.entry(key).or_insert((region,request.plan.level));held.0=held.0.union(region);
            }}
        }
        let mut held=IdMap::default();
        for (key,_,page) in halves {held.insert(key,page);}
        {
            let mut store=self.pages.lock().unwrap();
            for (key,(region,level)) in areas {
                let Some(page)=store.get(&key,region,pages::Accuracy::Display,level,Class::Visible) else {return Ok(());};
                held.insert(key,page);
            }
        }
        self.touch_cell_keys();
        let footprints=frame.reader_regions(target,self.plan.level).into_iter().map(|(local,regions)|(local,regions.into_iter().flat_map(|region|self.reached_cells(region)).collect())).collect();
        let mut tainted=crate::IdSet::default();reads(&root,target,&mut tainted);
        let predicted=frame.preview_for(target).and_then(|preview|preview.contact_outputs().map(Iterator::collect)).unwrap_or_default();
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
        self.pixels.hold_current(&frame);
        let previewed=r.preview_input_tiles(target);let stale=previewed.iter().map(|tile|(*tile,page_rect(*tile))).collect();
        self.gesture=Some(Contact {id:NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed),stroke,target,plan:self.plan,window,root,tiled,frame,composed,predicted,
            previewed,next:None,
            footprints,tainted,guard,generation:std::sync::Mutex::new(generation),exit:std::sync::OnceLock::new(),preview_level,cells,inputs,pages:held,level,
            stale,written:IdMap::default(),submissions:Vec::new(),stamps:Arc::default()});
        Ok(())
    }
    fn declare_reductions(&self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,request:&SourceRequest,
        encoder:&mut submission::CommandEncoder,
    )->Result<crate::scene::sources::Inputs,GpuRasterError> {
        let level=request.plan.level;
        let handle=packet.scene.source_owner(target).ok_or(GpuRasterError::NotReady)?;
        let source=&scene.scale_sources.entries[&target];let valid=PixelRect::full(source.valid_extent(level));
        let pages=request.required.regions.iter().flat_map(|area|pages::coordinates(*area,level))
            .map(|coordinate|(source.key(r,target,level,coordinate).plan(),coordinate)).collect::<Vec<_>>();
        let tiles=pages.iter().flat_map(|(plan,_)|page_coordinates(plan.bounds.intersect(valid))).collect::<BTreeSet<_>>();
        let expected=self.pages.lock().unwrap().expected().cloned().ok_or(GpuRasterError::NotReady)?;
        let wet=expected.watercolor(target);
        let placements=scene.source_placements(packet,scene::Output::Display)?;
        scene.declare(r,placements,crate::scene::sources::Declaration::scene(expected,Some(encoder.submission())),|scene,r,accepted| {
            let output=scene.reserve(r)?;let output=scene.pool[output].view.clone();
            for tile in &tiles {
                let plan=pages.iter().find(|(plan,_)|!plan.bounds.intersect(page_rect(*tile)).is_empty()).map(|(plan,_)|*plan).ok_or(GpuRasterError::NotReady)?;
                scene.reduce_color_pages(r,packet,handle,plan,&output,&[*tile],None,[0;2],&BTreeMap::new(),accepted)?;
            }
            if wet {accepted.declare_watercolor_stencils(r,packet.scene,target)?;}
            Ok(())
        })
    }
    pub(super) fn contact_continues(&mut self,scene:&Scene,r:&WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder)->Option<Vec<PixelRect>> {
        let Some(contact)=self.gesture.as_mut() else {return None;};
        contact.submissions.retain(|submission|submission.pending() || submission.discarded());
        if contact.exit.get().is_some() || !self.contact_holds(r,packet) || self.graph.root.as_ref()!=self.gesture.as_ref().map(|contact|&contact.root) {return None;}
        let Some(contact)=self.gesture.as_mut() else {return None;};
        let target=contact.target;let wet=contact.frame.watercolor(target);
        let Ok(current)=r.current_raster_versions(target) else {return None;};
        let mut changed=BTreeSet::new();
        if !contact.composed.changes(&current,|coordinate|{changed.insert(coordinate);}) {return None;}
        let predicted:BTreeSet<_>=match &r.preview_declaration {
            Some(preview) if preview.target()==target=>match preview.contact_outputs() {Some(outputs)=>outputs.collect(),None=>return None},
            Some(_)=>return None,
            None=>BTreeSet::new(),
        };
        let persistent=changed.clone();
        changed.extend(contact.predicted.iter().chain(&predicted).copied());
        let previewed=r.preview_input_tiles(target);
        let stale=contact.footprints.iter().filter(|(local,_)|changed.iter().any(|tile|!page_rect(*tile).intersect(*local).is_empty()))
            .flat_map(|(_,cells)|cells.iter().copied()).collect::<Vec<_>>();
        let refreshed:BTreeSet<_>=changed.iter().chain(&contact.previewed).chain(&previewed).copied().collect();
        let planes:&[layer_core::raster::RasterPlane]=if wet {&[layer_core::raster::RasterPlane::Color,layer_core::raster::RasterPlane::WatercolorWetness]}else {&[layer_core::raster::RasterPlane::Color]};
        if !contact.inputs.refresh_contact(r,packet.scene,target,planes,refreshed,&persistent,encoder) {return None;}
        for region in scene.scale_sources.entries.get(&target).into_iter().flat_map(|source|&source.damage.regions) {for tile in page_coordinates(*region) {
            let rect=contact.stale.entry(tile).or_insert(PixelRect::EMPTY);*rect=rect.union(region.intersect(page_rect(tile)));
        }}
        contact.next=Some((current,changed,predicted,previewed));
        self.invalidate_cells(stale);
        let cells=self.contact_cells();
        self.gesture.as_ref().is_some_and(|contact|cells.iter().all(|cell|contact.cells.iter().any(|(declared,_)|declared==cell))).then_some(cells)
    }
    #[cfg(test)]
    pub(super) fn contact_decisions_differ_from_a_declaration(&self,r:&WgpuRasterizer,scene:SceneView<'_>)->Result<(usize,Vec<String>),GpuRasterError> {
        let contact=self.gesture.as_ref().ok_or(GpuRasterError::NotReady)?;
        let wet=contact.frame.watercolor(contact.target);
        let tiles:BTreeSet<_>=contact.predicted.iter().chain(&contact.previewed).copied().collect();
        let declared=crate::scene::sources::declare(Default::default(),crate::scene::sources::Declaration::current(None),|accepted| {
            for coordinate in &tiles {
                accepted.color(r,scene,contact.target,*coordinate)?;
                if wet {accepted.scalar(r,scene,contact.target,layer_core::raster::RasterPlane::WatercolorWetness,*coordinate)?;}
            }
            Ok(())
        })?;
        Ok(contact.inputs.differing(&declared))
    }
    pub(super) fn compose_contact(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,
        composes:Vec<PixelRect>,retained:u64,
    )->Result<(Vec<PixelRect>,Vec<PixelRect>),GpuRasterError> {
        let contact=self.gesture.as_mut().expect("live contact");
        let (root,tiled,target)=(contact.root.clone(),contact.tiled,contact.target);
        let changed=contact.next.as_ref().map(|(_,changed,..)|changed.clone()).unwrap_or_default();
        let mut inputs=std::mem::take(&mut contact.inputs);
        let cells=std::mem::take(&mut contact.cells);
        let stale=std::mem::take(&mut contact.stale);
        let mut reductions=Vec::new();
        if let Some(level)=contact.level {
            let source=&scene.scale_sources.entries[&target];let valid=PixelRect::full(source.valid_extent(level));
            let reduced:BTreeSet<_>=if contact.frame.watercolor(target) {changed.iter().copied().flat_map(crate::scene::watercolor_stencil).flatten().collect()}else {changed};
            for tile in &reduced {
                let key=source.key(r,target,level,tile.map(|value|value>>level));
                let region=page_rect(*tile).intersect(valid);
                if let Some(page)=contact.pages.get(&key).filter(|_|!region.is_empty()) {reductions.push((key,region,*tile,page.clone()));}
            }
        }
        let start=scene.jobs.len();
        let result=(|| {
            let handle=packet.scene.source_owner(target).ok_or(GpuRasterError::NotReady)?;
            let mut reads=Unit::default();
            for (_,_,tile,page) in &reductions {
                scene.reduce_color_pages(r,packet,handle,page.plan,&page.view,&[*tile],None,[0;2],&stale,&mut inputs)?;
                reads.reads.push(page.clone());
            }
            if !reductions.is_empty() {reads.updates.push((target,reductions.len()));}
            if !reads.ready(scene,r,encoder,start,Class::Visible)? {return Err(GpuRasterError::NotReady);}
            let input=input_plan(self.plan,packet.scene,Some(self.graph.objects()));
            let mut units=vec![reads];let mut composed=Vec::new();let mut blocked=Vec::new();
            for region in composes {
                let start=scene.jobs.len();let mut unit=Unit::default();
                let result=(|| {
                    let (_,declared)=cells.iter().find(|(cell,_)|*cell==region).ok_or(GpuRasterError::Invariant("a contact cell has no declared inputs"))?;
                    let mut accepted=declared.accept_query(r,Some(scene))?;
                    accepted.overlay(&inputs);
                    let input=unit_plan(input,packet.scene,region,tiled,Some(self.graph.objects()));
                    let mut compositor=Evaluator {cache:&mut *self,scene:&mut *scene,packet,r:&mut *r,encoder:&mut *encoder,region:region.into(),input,tiled,
                        unit:&mut unit,accepted:&mut accepted,previous_motion:None,retained,approximate:false,preview:false,class:Class::Visible};
                    compositor.evaluate_root(&root,Root::Window)?;
                    if !compositor.unit.ready(compositor.scene,compositor.r,compositor.encoder,start,Class::Visible)? {return Err(GpuRasterError::NotReady);}
                    unit.sources=accepted;
                    Ok(())
                })();
                match result {
                    Ok(())=>{unit.work=scene.planned_work(start);units.push(unit);composed.push(region);},
                    Err(GpuRasterError::NotReady|GpuRasterError::NotPublished|GpuRasterError::SourceWorkingSetExceeded)=>{scene.discard_jobs_from(start);blocked.push(region);},
                    Err(error)=>return Err(error),
                }
            }
            self.encode_units(scene,r,encoder,units)?;
            Ok((composed,blocked))
        })();
        let contact=self.gesture.as_mut().expect("live contact");
        contact.inputs=inputs;contact.cells=cells;contact.stale=stale;
        if result.is_err() {scene.discard_jobs_from(start);return result;}
        let submission=encoder.submission();
        if let Some((versions,_,predicted,previewed))=contact.next.take() {contact.composed=versions;contact.predicted=predicted;contact.previewed=previewed;}
        {
            let mut store=self.pages.lock().unwrap();
            for (key,region,tile,_) in reductions {
                if !contact.previewed.contains(&tile) {contact.stale.remove(&tile);}
                store.forget(&key,region);contact.written.insert(tile,(key,region,submission.clone()));
            }
        }
        contact.submissions.push(submission);
        result
    }
    pub(super) fn end_contact(&mut self,encoder:&mut submission::CommandEncoder) {
        let Some(contact)=self.gesture.take() else {return;};
        if let Some(Some(frame))=contact.exit.get() && !contact.written.values().any(|(_,_,submission)|submission.discarded()) {
            let mut store=self.pages.lock().unwrap();
            for (_,(key,region,_)) in contact.written.iter().filter(|(tile,_)|!contact.predicted.contains(*tile)) {store.materialize_source(key,*region,key.level,frame);}
        }
        self.requeue_keyless_cells();
        let Contact {inputs,pages,..}=contact;
        inputs.track(encoder);
        for page in pages.into_values() {page.track(encoder);}
    }
}
