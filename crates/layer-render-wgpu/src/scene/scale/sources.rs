use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const REDUCTION_BATCH:usize=32;
pub(crate) const SOURCE_WORKING_BYTES:u64=REDUCTION_BATCH as u64*4*super::super::sources::NORMALIZED_PAGE_BYTES;

struct PageInputs {
    source: wgpu::TextureView,
    base: wgpu::TextureView,
    level: u32,
    over: bool,
    empty: bool,
}

pub(super) struct Source {
    pub extent: [u32; 2],
    /// How far right and down the levels hold the source's pixels.
    pub phase: [u32; 2],
    /// The document's pixels in the levels, whose texels at the source's
    /// edges also average the transparent pixels around it, unless the
    /// source is moving and its levels are resampled.
    window: Option<[u32; 2]>,
    pub updates: u64,
    pub blend_space: layer_core::BlendSpace,
    raster: u64,
    pub(super) watercolor: Option<WatercolorLayerStyle>,
    raw_material: bool,
    source: Option<Arc<layer_core::color::source::SourceImage>>,
    base: Option<([u32; 2], layer_core::authored::PaintBasePolicy)>,
    mask: Option<metadata::MaskMetadata>,
    node: graph::Node,
    pub(super) damage: Damage,
}

impl Source {
    fn node(id:SourceTarget,extent:[u32;2],phase:[u32;2])->graph::Node {
        Arc::new(graph::Expression::Source {id,frame:SourceFrame::at([0;2],phase),extent,outside:0f32.to_bits()})
    }
    pub(super) fn levels_extent(&self) -> [u32; 2] { [self.extent[0] + self.phase[0], self.extent[1] + self.phase[1]] }
    /// The pixels of the levels that texels of `level` average: those in the
    /// document, and around paint, the transparent rest of its edge texels.
    pub(super) fn valid_extent(&self, level: u32) -> [u32; 2] {
        let side = 1 << level;
        let extent = self.levels_extent();
        let Some(window) = self.window else { return extent; };
        std::array::from_fn(|i| if self.mask.is_some() { extent[i] } else { extent[i].div_ceil(side) * side }.min(window[i]))
    }
    pub(super) fn interest(&self,r:&WgpuRasterizer,id:SourceTarget,plan:display_mips::Plan,region:PixelRect,evaluation:Evaluation)->pages::Interest {
        let span=PAGE_SIZE<<plan.level;
        pages::Interest::new(self.key(r,id,plan.level,[region.min_x()/span,region.min_y()/span]),evaluation,region,plan.level)
    }
    pub(super) fn key(&self,r:&WgpuRasterizer,id:SourceTarget,level:u32,coordinate:[u32;2])->pages::Key {
        pages::Key {source:Some(id),expression:self.node.clone(),extent:self.valid_extent(level),color:r.document_color(),blend_space:self.blend_space,
            level,coordinate,format:wgpu::TextureFormat::Rgba32Float}
    }
}

#[derive(Default)]
pub(super) struct Prepared {pub(super) unpublished:bool,pub(super) refusal:Option<crate::residency::Blocked>}

#[derive(Default)]
pub(crate) struct Sources {
    pub(super) entries: HashMap<SourceTarget, Source>,
    pub(super) object_moving: Option<OccurrenceHandle>,
    pub(in crate::scene) object_damage: BTreeMap<Option<OccurrenceHandle>, Damage>,
    pub(in crate::scene) object_refinements: BTreeMap<super::super::object_cache::DamageKey, Damage>,
    pub(in crate::scene) vector_preview: Option<(OccurrenceHandle, Arc<layer_core::authored::vector::PathItem>, DocRect)>,
    pub(in crate::scene) preview_damage: Vec<DocRect>,
    pub(in crate::scene) vector_motion: Option<(layer_render::VectorMotion, DocRect)>,
    pub(in crate::scene) motion_damage: Vec<DocRect>,
    pub(in crate::scene) exclusion_damage: Vec<DocRect>,
    pub(in crate::scene) motion_split: Option<super::super::MotionSplit>,
    pub(in crate::scene) moving: Option<(OccurrenceHandle, super::super::vectors::Moving)>,
    pub(in crate::scene) prepared: Option<(super::super::MotionSplit, (OccurrenceHandle, super::super::vectors::Moving), DocRect)>,
}
impl Sources {
    pub fn storage_bytes(&self) -> u64 {
        self.object_refinements.values().chain(self.object_damage.values()).map(|damage| std::mem::size_of::<(super::super::object_cache::DamageKey, Damage)>() as u64 + damage.regions.capacity() as u64 * std::mem::size_of::<PixelRect>() as u64).sum::<u64>()
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, batch_tiles: &[Vec<brush_tiles::BrushTile>]) {
        let mut wanted = BTreeSet::new();
        self.object_moving = r.moving_layer;
        for &handle in packet.scene.order() {
            let occurrence = packet.scene.occurrence(handle).unwrap();
            let visible = packet.scene.visible(handle);
            if visible && occurrence.kind() == LayerKind::Paint && occurrence.is_artwork() && stack::has_content(r, packet.scene, handle) {
                let id = packet.scene.source_target(handle).unwrap();
                if wanted.insert(id) { self.update(r, packet, handle, false); }
                let source = self.entries.get_mut(&id).unwrap();
                source.raster = packet.scene.paint_source(handle).unwrap().raster.identity();
                let watercolor = r.watercolor_style(id, packet.dab_batches);
                source.watercolor = watercolor;
                source.damage=Damage::from_regions(packet.dab_batches.iter().zip(batch_tiles).filter(|(batch,_)|batch.target==id).flat_map(|(_,tiles)|tiles.iter().map(Damage::tile_region)))
                    .expand(watercolor.map_or(0,|style|style.radius()),source.extent).intersect(PixelRect::full(source.extent)).map(|region|region.translated(source.phase));
            }
            if let Some((mask, _)) = packet.scene.mask(handle).filter(|(mask, _)| mask.enabled && (visible || packet.inspect_mask == Some(handle)))
                && wanted.insert(SourceTarget::Coverage(mask.source)) { self.update(r, packet, handle, true); }
        }
        self.entries.retain(|id, _| wanted.contains(id));
    }
    fn update(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle, is_mask: bool) {
        let scene = packet.scene;
        let (id, image, mask) = if is_mask {
            (SourceTarget::Coverage(scene.mask(handle).unwrap().0.source), None, metadata::mask_metadata(scene, handle))
        } else { (scene.source_target(handle).unwrap(), scene.paint_source(handle).unwrap().base.as_ref().map(|base| base.image.storage().clone()), None) };
        let base = scene.paint_source(handle).filter(|_| !is_mask).and_then(|paint| paint.base.as_ref()).map(|base| (base.offset, base.policy));
        let extent = scene.target_extent(id);
        let raw_material = !is_mask && mapped_material(r, packet, id);
        let blend_space = if is_mask || r.moving_layer == Some(handle) || raw_material {
            layer_core::BlendSpace::Linear
        } else { packet.blend_space };
        let origin = scene.target_offset(id);
        let settled = if raw_material { [0; 2] } else { crate::scene::page_phase(origin) };
        let window = |phase: [u32; 2]| Some(std::array::from_fn(|i| (i64::from(packet.document_extent[i]) - origin[i] + i64::from(phase[i])).clamp(0, i64::from(u32::MAX)) as u32));
        let moving = r.moving_layer.is_some_and(|layer| std::iter::successors(Some(handle), |h| scene.parent(*h)).any(|h| h == layer));
        let source = self.entries.entry(id).or_insert_with(|| Source {
            extent, phase: settled, window: window(settled), updates: 0, blend_space, raster: 0, watercolor: None, raw_material, source: None, base: None, mask: None,
            node: Source::node(id, extent, settled), damage: Damage::EMPTY,
        });
        let (phase, window) = if raw_material { ([0; 2], None) } else if moving { (source.phase, None) } else { (settled, window(settled)) };
        if source.extent!=extent || source.phase!=phase {source.node=Source::node(id,extent,phase);}
        source.extent=extent;source.phase=phase;source.window=window;
        source.raw_material = raw_material;
        source.blend_space = blend_space;
        source.source = image;
        source.base = base;
        source.mask = mask;

    }

}

impl Cache {
    pub(super) fn half_cells(&self,region:PixelRect)->Vec<PixelRect> {
        let halves=self.pivot_halves();
        let required_input_level=self.required_input_level();
        let store=self.pages.lock().unwrap();
        pages::coordinates(region.intersect(self.plan.bounds),self.plan.level).map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds))
            .filter(|cell|!halves.is_empty() && halves.iter().all(|half| {
                let key=self.page_key(half.clone(),*cell);
                store.has(&key,*cell,pages::Accuracy::Display,required_input_level) || store.has(&key,*cell,pages::Accuracy::Exact,0)
            })).collect()
    }
    pub(super) fn held_halves(&self,region:PixelRect)->Vec<(pages::Key,PixelRect,pages::Held)> {
        let halves=self.pivot_halves();
        if halves.is_empty() {return Vec::new();}
        let required_input_level=self.required_input_level();
        let mut store=self.pages.lock().unwrap();
        pages::coordinates(region.intersect(self.plan.bounds),self.plan.level).flat_map(|coordinate| {
            let cell=pages::bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds);
            let held:Vec<_>=halves.iter().map(|half| {
                let key=self.page_key(half.clone(),cell);
                if let Some(page)=store.get(&key,cell,pages::Accuracy::Display,required_input_level,Class::Visible) {return Some((key,cell,page));}
                let exact=pages::Key {format:wgpu::TextureFormat::Rgba32Float,..key};
                store.get(&exact,cell,pages::Accuracy::Exact,0,Class::Visible).map(|page|(exact,cell,page))
            }).collect();
            held.iter().all(Option::is_some).then(||held.into_iter().flatten()).into_iter().flatten()
        }).collect()
    }
    pub(super) fn region_sources(&self,scene:&Scene,r:&WgpuRasterizer,packet:FramePacket<'_>,region:PixelRect,tiled:bool)->Vec<(SourceTarget,SourceRequest)> {
        let root=self.graph.root.as_ref().unwrap();
        let reduced=&root.source_domains(packet.scene)[0];
        let input=input_plan(self.plan,packet.scene,Some(self.graph.objects()));
        let expected=scene.scale_pages.lock().unwrap().expected().cloned();
        let unit=unit_plan(input,packet.scene,region,tiled,Some(self.graph.objects()));
        let read=if unit==input {Damage::EMPTY}else {unit.doc_bounds.in_frame(self.plan.extent).into()};
        let whole=root.required(region.into(),input).union(region.into()).union(read);
        let held=self.half_cells(region);
        let inside:BTreeSet<SourceTarget>=self.pivot_halves().iter().flat_map(|node|node.reads().0).collect();
        let open=Damage::from_regions(pages::coordinates(region.intersect(self.plan.bounds),self.plan.level)
            .map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent).intersect(region))
            .filter(|cell|!held.contains(&cell.intersect(self.plan.bounds))));
        let partial=if open.is_empty() {Damage::EMPTY}else {root.required(open.clone(),input).union(open)};
        targets(r,packet).filter(|(_,id)|reduced.contains(id) && !r.transforms.as_ref().is_some_and(|t|t.display_source(*id)))
            .filter_map(|(handle,id)| {
                let required=if inside.contains(&id) {&partial}else {&whole};
                if required.is_empty() {return None;}
                let frame=source_frame(Some(&scene.scale_sources),packet.scene,id);
                let extent=packet.scene.target_extent(id);let moving=r.moving_layer==Some(handle);
                let level=expected.as_ref().map_or(0,|expected|expected.source_level(id,frame,extent,region.into(),input.level,moving));
                let plan=source_plan(input,frame,extent,moving,level);
                if tiled && self.plan.level==0 && plan.level==0 {return None;}
                let (required,covered)=local_damage(required,PixelRect::EMPTY,frame,extent,plan.level,moving);
                Some((id,SourceRequest {plan,required,covered}))
            }).collect()
    }
    #[expect(clippy::too_many_arguments,reason="A unit declaration evaluates the same region, input plan and preview stage as its reader")]
    pub(super) fn declare_unit(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,expected:Arc<SignatureFrame>,region:PixelRect,input:display_mips::Plan,tiled:bool,
        preview:bool,retained:u64,class:Class,encoder:&mut submission::CommandEncoder,mut evaluate:impl FnMut(&mut Evaluator<'_>)->Result<(),GpuRasterError>,
    )->Result<super::super::sources::Inputs,GpuRasterError> {
        let placements=scene.source_placements(packet,scene::Output::Display)?;
        let declaration=super::super::sources::Declaration::scene(expected,scene.submission.clone());
        let used=self.used.clone();let mut unit=Unit::default();self.declaring=true;
        let cells=pages::coordinates(region,self.plan.level).map(|coordinate|pages::bounds(coordinate,self.plan.level,self.plan.extent).intersect(region)).collect::<Vec<_>>();
        let cells=if cells.len()>1 {cells}else {vec![region]};
        let declared=scene.declare(r,placements,declaration,|scene,r,accepted| cells.iter().try_for_each(|cell|
            evaluate(&mut Evaluator {cache:&mut *self,scene:&mut *scene,packet,r:&mut *r,encoder:&mut *encoder,
            region:(*cell).into(),input,tiled,unit:&mut unit,accepted:&mut *accepted,previous_motion:None,retained,approximate:false,preview,class})));
        self.declaring=false;self.output.truncate(used.len());self.used.truncate(used.len());self.used.clone_from_slice(&used);
        declared
    }
    #[expect(clippy::too_many_arguments,reason="A parent unit accepts the declaration of the roots it evaluates")]
    pub(super) fn accept_parent(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,region:PixelRect,
        requests:&[(SourceTarget,SourceRequest)],retained:u64,tiled:bool,roots:&[graph::Node],target:Root,preview:bool,class:Class,objects:Class,encoder:&mut submission::CommandEncoder,
    )->Result<Result<pages::Accepted,Option<crate::residency::Blocked>>,GpuRasterError> {
        let root=self.graph.root.clone().unwrap();let interest=self.parent_interest(region);let parent=interest.key.clone();
        let input=unit_plan(input_plan(self.plan,packet.scene,Some(self.graph.objects())),packet.scene,region,tiled,Some(self.graph.objects()));
        let expected=scene.scale_pages.lock().unwrap().expected().cloned().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?;
        let native=match self.declare_unit(scene,r,packet,expected,region,input,tiled,preview,retained,class,encoder,
            |compositor|roots.iter().try_for_each(|root|compositor.evaluate_root(root,Root::Store).map(drop))) {
            Err(GpuRasterError::SourceWorkingSetExceeded|GpuRasterError::NotReady)=>return Ok(Err(None)),
            native=>native?,
        };
        let mut areas=if target==Root::Store {vec![(parent,region)]}else {Vec::new()};
        areas.extend(self.held_halves(region).into_iter().map(|(key,cell,_)|(key,cell)));
        for (id,request) in requests {
            let source=&scene.scale_sources.entries[id];
            for area in &request.required.regions {for coordinate in pages::coordinates(*area,request.plan.level) {
                let key=source.key(r,*id,request.plan.level,coordinate);
                let area=key.plan().bounds.intersect(*area);
                if !area.is_empty() {areas.push((key,area));}
            }}
        }
        let cost=root.cost();let scratch=if cost.0==0 {0}else {cost.1 as usize+1};
        self.accept_working(scene,r,interest,areas,native,[scratch,0],input,tiled,class,objects,encoder)
    }
    #[expect(clippy::too_many_arguments,reason="Accepted output, inputs and scratch share one working reservation")]
    pub(super) fn accept_working(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,interest:pages::Interest,areas:Vec<(pages::Key,PixelRect)>,
        native:super::super::sources::Inputs,scratch:[usize;2],plan:display_mips::Plan,tiled:bool,class:Class,objects:Class,_encoder:&mut submission::CommandEncoder,
    )->Result<Result<pages::Accepted,Option<crate::residency::Blocked>>,GpuRasterError> {
        let Some(acceptance)=self.pages.lock().unwrap().accept_version(&interest)? else {return Ok(Err(None));};
        let native=native.for_unit(acceptance.input_unit())?;
        if self.pages.lock().unwrap().holds(&acceptance,&native,&areas) {
            if self.pages.lock().unwrap().held_refusal(&acceptance).is_none() {return Ok(Ok(acceptance));}
            self.pages.lock().unwrap().finish_inputs(&acceptance);
        }
        #[cfg(test)]
        crate::test_support::PassCounts {unit_acceptances:1,..Default::default()}.record();
        let scratch_bytes=if tiled {scratch.into_iter().enumerate().map(|(scalar,count)| {
            let format=if scalar==0 {r.device.working_format()}else {r.device.scalar_format()};
            let available=scene.used.iter().enumerate().filter(|(index,used)|!used.occupied() && scene.pool[*index].texture.format()==format).count();
            count.saturating_sub(available) as u64*super::super::scratch_page_bytes(format)
        }).sum()}else {
            let available=self.used.iter().enumerate().filter(|(index,used)|!used.occupied() && self.output[*index].plan.size==plan.size).count();
            scratch[0].saturating_sub(available) as u64*plan.size.map(u64::from).into_iter().product::<u64>()*16
        };
        let keys=areas.iter().map(|(key,_)|key.clone()).collect::<std::collections::HashSet<_>>();
        let (mut held,missing,bytes)=self.pages.lock().unwrap().reserve_inputs(&keys.into_iter().collect::<Vec<_>>());
        let refusal=if native.objects().is_empty() {None}else {scene.object_results.admit(r,objects,native.objects(),None)?};
        let (mut reservations,mut refused)=(None,None);
        let mut context=(&mut *self,&mut *scene,&mut held,&mut reservations,&mut refused);
        let result=native.accept(r,&mut context,|(cache,scene,_,reservations,refused),r,inputs| {
            let native_bytes=inputs.additional_bytes(&r.source_tiles.borrow());
            let store=cache.pages.lock().unwrap().tier();
            let admitted=r.admit_display(Some(scene),class,native_bytes).and_then(|native|
                Ok((native,r.admit_tier(Some(scene),&store,class,bytes)?,r.admit_tier(Some(scene),&scene.tiers.gpu,class,scratch_bytes)?)));
            let (native,pages,scratch)=admitted.map_err(|blocked|**refused=Some(blocked.covering(native_bytes+bytes+scratch_bytes))).ok()?;
            **reservations=Some((pages,scratch));
            Some(native)
        },|(cache,scene,held,reservations,_),r| {
            let (pages,scratch_reservation)=reservations.as_mut().unwrap();
            let mut leases=Vec::new();
            let scratch_limit=scene.pool_bytes()+scratch_bytes;
            for (scalar,count) in scratch.into_iter().enumerate() {for _ in 0..count {
                leases.push(if tiled {let slot=scene.reserve_with_reserved(r,scalar!=0,scratch_limit,scratch_reservation)?;scene.used[slot].lease(slot)}else {cache.allocate_reserved(r,plan,scratch_reservation)?});
            }}
            for key in missing {let image=cache.pages.lock().unwrap().claim_reserved(r,key.clone(),cache.window,class,pages)?;held.push((key,image));}
            Ok(std::mem::take(*held))
        },|(_,_,_,reservations,_),_,_| {reservations.take();Ok(())});
        let handle=acceptance.clone();
        let result=result.and_then(|accepted| {
            let Some((images,mut sources))=accepted else {return Err(GpuRasterError::NotReady);};
            if let Some(refusal)=refusal.clone() {sources.fall_back_objects(refusal);}
            let images=images.into_iter().collect::<std::collections::HashMap<_,_>>();
            let inputs=areas.into_iter().map(|(key,area)| {let image=images[&key].clone();(key,area,image)}).collect();
            let mut pages=self.pages.lock().unwrap();pages.accept(&interest,inputs,acceptance)?;
            pages.hold_inputs(&handle,sources)?;Ok(())
        });
        if result.is_err() {drop(held);self.pages.lock().unwrap().finish_inputs(&handle);}
        match result {Ok(())=>Ok(Ok(handle)),Err(GpuRasterError::SourceWorkingSetExceeded|GpuRasterError::NotReady)=>Ok(Err(refused)),Err(error)=>Err(error)}
    }
    pub(super) fn source_ready(&self,scene:&Scene,r:&WgpuRasterizer,id:SourceTarget,request:&SourceRequest)->bool {
        let source=&scene.scale_sources.entries[&id];
        let mut pages=self.pages.lock().unwrap();
        request.required.regions.iter().all(|area|pages::coordinates(*area,request.plan.level).all(|coordinate| {
            let key=source.key(r,id,request.plan.level,coordinate);
            let region=key.plan().bounds.intersect(*area).intersect(PixelRect::full(key.extent));
            region.is_empty() || pages.get(&key,region,pages::Accuracy::Display,request.plan.level,Class::Visible).is_some()
        }))
    }
    pub(super) fn prepare_source(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,
        id:SourceTarget,request:SourceRequest,class:Class,budget:&mut WorkBudget,
    )->Result<Prepared,GpuRasterError> {
        if !Arc::ptr_eq(&self.pages,&scene.scale_pages) {self.pages=scene.scale_pages.clone();}
        let SourceRequest {plan,required,covered}=request;
        let limit=(budget.allowance(class)/object_cache::PASS_TAPS).max(1) as usize;
        let source=&scene.scale_sources.entries[&id];
        let candidates=nearest_pages(required.bounds().intersect(plan.bounds),work_view(packet,0),limit,std::iter::empty(),|coordinate| {
            let region=page_rect(*coordinate).intersect(PixelRect::full(source.valid_extent(plan.level)));
            let key=source.key(r,id,plan.level,coordinate.map(|value|value>>plan.level));
            required.intersects(region) && region.intersect(covered).is_empty()
                && self.pages.lock().unwrap().get(&key,region,pages::Accuracy::Display,plan.level,class).is_none()
        });
        let mut ready=Vec::new();let mut handles=Vec::new();let mut prepared=Prepared::default();
        for coordinate in candidates {
            if !budget.fits(class,!ready.is_empty(),object_cache::PASS_TAPS) {break;}
            budget.spend(object_cache::PASS_TAPS);
            let source=&scene.scale_sources.entries[&id];
            let valid=source.valid_extent(plan.level);
            let region=page_rect(coordinate).intersect(PixelRect::full(valid));
            if region.is_empty() {continue;}
            let interest=source.interest(r,id,plan,region,self.evaluation);let key=interest.key.clone();
            let derived=(0..plan.level).rev().find_map(|level| {
                let input=source.key(r,id,level,coordinate.map(|value|value>>level));
                self.pages.lock().unwrap().get(&input,region,pages::Accuracy::Display,plan.level,class).map(|image|(input,image))
            });
            let material=source.watercolor.is_some() && source.raw_material;
            let scratch=if derived.is_some() {[0,0]}else if matches!(id,SourceTarget::Coverage(_)) {[0,usize::from(source.phase!=[0;2])]}
                else {[usize::from(source.phase!=[0;2])*2+usize::from(source.watercolor.is_some())+usize::from(material)*12,0]};
            let mut native=super::super::sources::Inputs::default();
            native.source_placement(id,scene.reduced_placement(packet,id,source.phase))?;
            if derived.is_none() {
                let expected=scene.scale_pages.lock().unwrap().expected().cloned().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?;
                let declaration=super::super::sources::Declaration::scene(expected,Some(encoder.submission()));
                native=match scene.declare(r,native,declaration,|scene,r,accepted| {
                    let output=scene.reserve(r)?;let output=scene.pool[output].view.clone();
                    scene.reduce_source(r,packet,id,key.plan(),&output,coordinate,accepted)
                }) {Err(GpuRasterError::NotReady)=>continue,Err(GpuRasterError::NotPublished)=>{prepared.unpublished=true;continue;},native=>native?};
            }
            let mut areas=vec![(key.clone(),region)];
            if let Some((input,_))=&derived {areas.push((input.clone(),region));}
            let handle=match self.accept_working(scene,r,interest,areas,native,scratch,plan,true,class,Class::Prefetch,encoder)? {
                Ok(handle)=>handle,Err(refusal)=>{prepared.refusal=prepared.refusal.or(refusal);continue;},
            };
            let image=self.pages.lock().unwrap().allocated(&key,class).expect("accepted source page");
            let write=self.pages.lock().unwrap().prepare_write(key.clone(),region,pages::Accuracy::Display,plan.level)?;
            let mut unit=Unit::default();unit.sources=self.pages.lock().unwrap().take_inputs(&handle)?;
            unit.reads.push(image.clone());
            let start=scene.jobs.len();let reads=r.metrics.display_reduction_reads;
            let mut sources=std::mem::take(&mut unit.sources);
            let result=(|| {
                #[cfg(test)]
                let _record=crate::scene::sources::record_reads(&sources);
                let accepted=&mut sources;
                if let Some((_,input))=derived {
                    let bounds=input.plan.bounds;
                    let [x,y,width,height]=paint_transform::texel_rect(region.window_local(image.plan.bounds),1<<plan.level);
                    let sources=[input.view.clone(),r.empty_view.clone(),r.empty_view.clone(),r.empty_view.clone()];
                    let mut values=[0;20];
                    values[..8].copy_from_slice(&[x,y,width,height,valid[0].min(bounds.max_x()).saturating_sub(bounds.min_x()),
                        valid[1].min(bounds.max_y()).saturating_sub(bounds.min_y()),1<<(plan.level-input.plan.level),(input.plan.level<<8)|8]);
                    values[14]=((bounds.min_x() as f32-image.plan.bounds.min_x() as f32)/(1<<input.plan.level) as f32).to_bits();
                    values[15]=((bounds.min_y() as f32-image.plan.bounds.min_y() as f32)/(1<<input.plan.level) as f32).to_bits();
                    scene.jobs.push(Job::Reduce {output:image.view.clone(),sources,values,
                        size:[width,height],kernel:Reduction::Pages,format:image.texture.format()});
                    unit.reads.push(input);
                } else {
                    scene.reduce_source(r,packet,id,image.plan,&image.view,coordinate,accepted)?;
                    if matches!(id,SourceTarget::Paint(_)) {unit.updates.push((id,1));}
                }
                unit.ready(scene,r,encoder,start,class)
            })();
            unit.sources.extend(sources)?;unit.sources.check_unit(handle.input_unit())?;
            match result {
                Ok(true)=>{
                    #[cfg(test)] crate::test_support::record_source_preparation((id,plan.level,coordinate));
                    budget.spend((r.metrics.display_reduction_reads-reads).max(u64::from(PAGE_SIZE).pow(2)));
                    unit.pages.push(write);
                    ready.push(unit);handles.push(handle);
                },
                Ok(false)|Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=>{
                    if class.demanded() {self.pages.lock().unwrap().hold_inputs(&handle,unit.sources)?;}
                    else {self.pages.lock().unwrap().finish_inputs(&handle);}
                    scene.discard_jobs_from(start);if !r.decode_queue_has_room() {break;}
                },
                Err(GpuRasterError::SourceWorkingSetExceeded)=>{
                    self.pages.lock().unwrap().finish_inputs(&handle);scene.discard_jobs_from(start);
                },
                Err(error)=>{scene.discard_unencoded_jobs();return Err(error);},
            }
        }
        if !ready.is_empty() {self.encode_units(scene,r,encoder,ready)?;}
        let mut pages=self.pages.lock().unwrap();
        for handle in handles {pages.finish_inputs(&handle);}
        Ok(prepared)
    }
}

impl Scene {
    pub(in crate::scene) fn source_placements(&mut self,packet:FramePacket<'_>,output:scene::Output)->Result<crate::scene::sources::Inputs,GpuRasterError> {
        let placed=|sources:&Sources,id|(source_frame(Some(sources),packet.scene,id),packet.scene.target_extent(id));
        if self.placement_frame.len()!=packet.scene.targets().count() || packet.scene.targets().any(|id|self.placement_frame.get(&id)
            .is_none_or(|held|{let (frame,extent)=placed(&self.scale_sources,id);held.frame!=frame || held.bounds!=PixelRect::full(extent)})) {
            self.placement_frame=Arc::new(packet.scene.targets().map(|id|{let (frame,extent)=placed(&self.scale_sources,id);(id,super::super::placement::Placement::source(frame,extent))}).collect());
        }
        let overrides=match output {scene::Output::Source(id)=>[(id,self.raw_placement(Default::default(),PixelRect::full(packet.scene.target_extent(id)),packet.scene.target_offset(id)))].into(),_=>Default::default()};
        Ok(crate::scene::sources::Inputs::placed(crate::scene::sources::PlacementSet::new(self.placement_frame.clone(),overrides)))
    }
    #[expect(clippy::too_many_arguments,reason="A source page reduction keeps its plan, output and coordinate explicit")]
    fn reduce_source(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,id:SourceTarget,plan:display_mips::Plan,output:&wgpu::TextureView,coordinate:[u32;2],
        accepted:&mut crate::scene::sources::AcceptedInputs,
    )->Result<(),GpuRasterError> {
        let handle=packet.scene.source_owner(id).unwrap();
        if matches!(id,SourceTarget::Paint(_)) {self.reduce_color_pages(r,packet,handle,plan,output,&[coordinate],None,[0;2],&BTreeMap::new(),accepted)?;}
        else {let (mask,coverage)=packet.scene.mask(handle).unwrap();self.reduce_mask_pages(r,mask,coverage,plan,output,&[coordinate],[0;2],accepted)?;}
        Ok(())
    }

    pub(crate) fn copy_reduced_source(&self,r:&WgpuRasterizer,scene:SceneView<'_>,id:SourceTarget,extent:[u32;2],level:u32,
        region:PixelRect,output:&wgpu::Texture,encoder:&mut submission::CommandEncoder,
    )->bool {
        let Some(source)=self.scale_sources.entries.get(&id) else {return false;};
        let Some(paint)=scene.source_owner(id).and_then(|owner|scene.paint_source(owner)) else {return false;};
        if source.blend_space!=layer_core::BlendSpace::Linear || source.extent!=extent || source.raster!=paint.raster.identity()
            || self.scale_pages.lock().unwrap().expected().is_none_or(|frame|frame.preview_for(id).is_some()) || source.watercolor.is_some() || source.phase!=crate::scene::page_phase(scene.target_offset(id))
            || source.base!=paint.base.as_ref().map(|base|(base.offset,base.policy))
            || !match (&source.source,paint.base.as_ref().map(|base|base.image.storage())) {(Some(a),Some(b))=>Arc::ptr_eq(a,b),(None,None)=>true,_=>false} {return false;}
        let Some(cache)=r.scale_display.as_ref() else {return false;};
        let mut store=cache.pages.lock().unwrap();
        let inputs=pages::coordinates(region,level).map(|coordinate| {
            let key=source.key(r,id,level,coordinate);let area=key.plan().bounds.intersect(region);
            if area!=region {return None;}
            store.get(&key,area,pages::Accuracy::Display,level,Class::Visible).map(|image|(key,image,area))
        }).collect::<Option<Vec<_>>>();
        let Some(inputs)=inputs else {return false;};
        for (_,image,area) in inputs {
            let [sx,sy,width,height]=paint_transform::texel_rect(area.window_local(image.plan.bounds),1<<level);
            encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:sx,y:sy,z:0},..image.texture.as_image_copy()},
                wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:area.min_x()>>level,y:area.min_y()>>level,z:0},..output.as_image_copy()},
                wgpu::Extent3d {width,height,depth_or_array_layers:1});
            image.track(encoder);
        }
        true
    }
    /// Reduce `missing` pages of a layer's levels, or of `placement` of it,
    /// into `output`, whose `plan` lies `shift` pixels before the pages.
    #[expect(clippy::too_many_arguments, reason = "Color reduction keeps source placement, missing pages, and GPU output bindings explicit")]
    pub(super) fn reduce_color_pages(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle, plan: display_mips::Plan,
        output: &wgpu::TextureView, missing: &[[u32; 2]], placement: Option<&super::super::placement::Placement>, shift: [i64; 2],
        stale: &BTreeMap<[u32; 2], PixelRect>,
     accepted:&mut crate::scene::sources::AcceptedInputs) -> Result<PixelRect, GpuRasterError> {
        let level = plan.level;
        let id = packet.scene.source_target(handle).unwrap();
        let held=if placement.is_none() {Some(accepted.placement(id)?)}else {None};
        let geometry=placement.or(held.as_deref()).unwrap();
        let phase = geometry.frame.phase;
        let extent = if placement.is_some() { packet.document_extent } else { self.scale_sources.entries[&id].valid_extent(level) };
        let blend_space = if placement.is_some() { packet.blend_space } else { self.scale_sources.entries[&id].blend_space };
        let phased = geometry.packet(packet);
        let mut changed = PixelRect::EMPTY;
        for &tile in missing {
            if placement.is_none() && phase != [0; 2] && r.watercolor_style(id, packet.dab_batches).is_none()
                && let Some((region, reads)) = self.reduce_shifted_page(r, packet.scene, id, plan, output, tile, phase, shift, extent, blend_space, stale.get(&tile).copied(), accepted)? {
                changed = changed.union(region);
                r.metrics.display_reduction_reads += reads;
                continue;
            }
            let entry = &self.scale_sources.entries[&id];
            let mut scratch = [None; 2];
            let preview = r.preview_layer_id == Some(id) && !r.preview_damage.intersect(page_rect(tile)).is_empty();
            let wet_nearby = entry.watercolor.is_some() && !entry.raw_material
                && Self::watercolor_nearby(r, packet, id, r.paint_layers.iter().find(|layer| layer.id == id), tile, preview);
            let (source, base, input_level, over, empty) = if placement.is_some() || phase != [0; 2] || wet_nearby
                || (r.preview_contribution && r.preview_layer_id==Some(id)) {
                let (color, flow, pigment) = match &placement {
                    Some(_) => { let (page, pigment) = self.placed_material_inputs(r, packet, handle, geometry, tile, accepted)?; (page, None, pigment) }
                    None if phase != [0; 2] && r.watercolor_style(id, packet.dab_batches).is_some() =>
                        (self.paint_tile(r, phased, handle, geometry, tile, accepted)?, None, r.empty_view.clone()),
                    None if phase != [0; 2] => { let (color, flow) = self.placed_color(r, packet.scene, id, geometry, tile, accepted)?; (color, flow, r.empty_view.clone()) }
                    None => (self.local_color_tile(r, packet, handle, tile, accepted)?, None, r.empty_view.clone()),
                };
                scratch = [Some(color), flow];
                match flow {
                    Some(flow) => (self.pool[flow].view.clone(), self.pool[color].view.clone(), 0, true, false),
                    None => (self.pool[color].view.clone(), pigment, 0, false, false),
                }
            } else {
                let inputs = self.page_inputs(r, packet.scene, id, tile, accepted)?;
                (inputs.source, inputs.base, inputs.level, inputs.over, inputs.empty)
            };
            let valid = page_rect(tile).intersect(PixelRect::full(extent));
            let side=1u32<<level;
            let part=if placement.is_none() && phase==[0;2] {stale.get(&tile).copied().map_or(valid,|part|part.intersect(valid))} else {valid};
            if part.is_empty() {for slot in scratch.into_iter().flatten() {self.free(slot);} continue;}
            let low=[(part.min_x()-valid.min_x())/side,(part.min_y()-valid.min_y())/side];
            let high=[(part.max_x()-valid.min_x()).div_ceil(side),(part.max_y()-valid.min_y()).div_ceil(side)];
            changed=changed.union(PixelRect::new(valid.min_x()+low[0]*side,valid.min_y()+low[1]*side,
                (valid.min_x()+high[0]*side).min(valid.max_x()),(valid.min_y()+high[1]*side).min(valid.max_y())));
            let size=[high[0]-low[0],high[1]-low[1]];
            let origin = output_origin(tile, shift, plan);
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[
                origin[0]+low[0],
                origin[1]+low[1],
                size[0],
                size[1],
                valid.width(),
                valid.height(),
                1 << (level - input_level),
                u32::from(over) | if empty { 4 } else { 0 } | (input_level << 8)
                    | if placement.is_some() { 128 } else { 0 }
                    | if blend_space == layer_core::BlendSpace::Perceptual { 2 } else { 0 },
            ]);
            if low!=[0;2] {
                values[7]|=8;
                values[14]=((origin[0]*values[6]) as f32).to_bits();
                values[15]=((origin[1]*values[6]) as f32).to_bits();
            }
            let reads = if values[7] & 4 != 0 { 0 } else { values[6].pow(2) * (1 + (values[7] & 1) + (values[7] >> 7 & 1)) };
            r.metrics.display_reduction_reads += u64::from(size[0] * size[1] * reads);
            let sources = [source, base, r.empty_view.clone(), r.empty_view.clone()];
            self.jobs.push(Job::Reduce { output: output.clone(), sources, values, size, kernel: Reduction::Pages, format:output.texture().format() });
            for slot in scratch.into_iter().flatten() { self.free(slot); }
        }
        Ok(changed)
    }
    fn has_page_preview(r:&WgpuRasterizer,id:SourceTarget,c:[u32;2])->bool {
        r.preview_layer_id==Some(id) && r.preview_page(c).is_some()
    }
    fn page_inputs(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, id: SourceTarget, c: [u32; 2], accepted:&mut crate::scene::sources::AcceptedInputs) -> Result<PageInputs, GpuRasterError> {
        let [base,overlay]=accepted.color(r,scene,id,c)?;
        let level=accepted.level(id,RasterPlane::Color,c);
        let over=overlay.is_some();
        let empty=base.is_none() && overlay.is_none();
        let base=base.map(|read|self.read_input(read));
        let source=overlay.map(|read|self.read_input(read)).or_else(||base.clone()).unwrap_or_else(||r.empty_view.clone());
        Ok(PageInputs { source, base: base.unwrap_or_else(|| r.empty_view.clone()), level, over, empty })
    }
    /// Reduce level page `tile` of a layer whose levels lie `phase` pixels
    /// after its own pages, or of the page only the texels over `stale`.
    /// `None` while a stroke previews a flow that needs its base.
    #[expect(clippy::too_many_arguments, reason = "A shifted reduction keeps its plan, output, phase, frame and stale part explicit")]
    fn reduce_shifted_page(
        &mut self, r: &WgpuRasterizer, scene: SceneView<'_>, id: SourceTarget, plan: display_mips::Plan, output: &wgpu::TextureView,
        tile: [u32; 2], phase: [u32; 2], shift: [i64; 2], extent: [u32; 2], blend_space: layer_core::BlendSpace, stale: Option<PixelRect>,
     accepted:&mut crate::scene::sources::AcceptedInputs) -> Result<Option<(PixelRect, u64)>, GpuRasterError> {
        let side = 1u32 << plan.level;
        let valid = page_rect(tile).intersect(PixelRect::full(extent));
        let size = [valid.width(), valid.height()];
        let count = size.map(|n| n.div_ceil(side));
        let part = stale.map_or(valid, |stale| stale.intersect(valid));
        if part.is_empty() { return Ok(Some((PixelRect::EMPTY, 0))); }
        let low: [u32; 2] = std::array::from_fn(|axis| ([part.min_x(), part.min_y()][axis] - tile[axis] * PAGE_SIZE) / side);
        let high: [u32; 2] = std::array::from_fn(|axis| ([part.max_x(), part.max_y()][axis] - tile[axis] * PAGE_SIZE).div_ceil(side));
        let pages = scene.target_extent(id).map(|n| n.div_ceil(PAGE_SIZE));
        let neighbors:[Option<([u32;2],bool)>;4]=std::array::from_fn(|index| {
            let at=[(index%2) as u32,(index/2) as u32];
            let [Some(x),Some(y)]=std::array::from_fn(|axis|(tile[axis]+at[axis]).checked_sub(1).filter(|c|*c<pages[axis])) else {return None;};
            Some(([x,y],Self::has_page_preview(r,id,[x,y])))
        });
        if !r.preview_requires_base && neighbors.iter().flatten().any(|(_,preview)|*preview) {return Ok(None);}
        let levels=neighbors.each_ref().map(|neighbor|if neighbor.as_ref().is_some_and(|(_,preview)|*preview) {r.preview_level}else {0});
        let mut inputs: [Option<PageInputs>; 4] = Default::default();
        for (index,neighbor) in neighbors.into_iter().enumerate() {
            let at=[(index%2) as u32,(index/2) as u32];
            if (0..2).any(|axis|phase[axis]==0 && at[axis]==0) {continue;}
            if let Some((coordinate,_))=neighbor {inputs[index]=Some(self.page_inputs(r,scene,id,coordinate, accepted)?);}
        }
        let reduced=levels.into_iter().max().unwrap();
        let units = |axis: usize| {
            let mut halves = [0u64; 2];
            for texel in low[axis]..high[axis] {
                let length = side.min(size[axis] - texel * side);
                let cell = if length == side { 1 << reduced } else { 1 };
                let snapped = (PAGE_SIZE - phase[axis] + texel * side + cell / 2) & !(cell - 1);
                for unit in (snapped..snapped + length).step_by(cell as usize) { halves[(unit / PAGE_SIZE) as usize] += 1; }
            }
            halves
        };
        let [x, y] = [units(0), units(1)];
        let reads = (0..4).filter(|&page| inputs[page].as_ref().is_some_and(|input| !input.empty)).map(|page| x[page % 2] * y[page / 2]).sum();
        let origin = output_origin(tile, shift, plan);
        let mut values = [0; 20];
        values[..8].copy_from_slice(&[origin[0], origin[1], count[0], count[1], size[0], size[1], side,
            if blend_space == layer_core::BlendSpace::Perceptual { 2 } else { 0 }]);
        for ((word,input),level) in values[8..12].iter_mut().zip(&inputs).zip(levels) { *word=level | if input.as_ref().is_some_and(|input|!input.empty) {16}else {0}; }
        values[12..16].copy_from_slice(&[phase[0], phase[1], low[0] | low[1] << 16, high[0] | high[1] << 16]);
        let views: [&wgpu::TextureView; 4] = std::array::from_fn(|i| inputs[i].as_ref().map_or(&r.empty_view, |input| &input.source));
        self.jobs.push(Job::Reduce { output: output.clone(), sources: views.map(Clone::clone), values,
            size: [high[0] - low[0], high[1] - low[1]], kernel: Reduction::Phased, format:output.texture().format() });
        Ok(Some((part, reads)))
    }
    /// Reduce `missing` pages of a mask's levels into `output`, whose `plan`
    /// lies `shift` pixels before the pages.
    #[expect(clippy::too_many_arguments, reason = "Mask reduction keeps missing pages and GPU output bindings explicit")]
    pub(super) fn reduce_mask_pages(
        &mut self, r: &mut WgpuRasterizer, mask: &MaskUse, coverage: &CoverageSource,
        plan: display_mips::Plan, output: &wgpu::TextureView, missing: &[[u32; 2]], shift: [i64; 2],
     accepted:&mut crate::scene::sources::AcceptedInputs) -> Result<PixelRect, GpuRasterError> {
        let level = plan.level;
        let id = SourceTarget::Coverage(mask.source);
        let entry = &self.scale_sources.entries[&id];
        let extent=entry.valid_extent(level);
        let phase=accepted.placement(id)?.frame.phase;
        let default = if mask.inverted { 1. - coverage.default_coverage } else { coverage.default_coverage };
        let mut changed = PixelRect::EMPTY;
        for &tile in missing {
            let scratch = (phase != [0; 2]).then(|| self.mask_at(r, mask, coverage, phase.map(i64::from), tile, accepted)).transpose()?;
            let source = match scratch {
                Some(slot) => Some(self.pool[slot].view.clone()),
                None => self.mask_page(r,id,coverage,tile, accepted)?,
            };
            let valid = page_rect(tile).intersect(PixelRect::full(extent));
            changed = changed.union(valid);
            let size = [valid.width(), valid.height()].map(|n| n.div_ceil(1 << level));
            let origin = output_origin(tile, shift, plan);
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[
                origin[0], origin[1], size[0], size[1], valid.width(), valid.height(), 1 << level,
                match (&source, scratch) { (None, _) => 4, (Some(_), Some(_)) => 0, (Some(_), None) => 32 | if mask.inverted { 64 } else { 0 } },
            ]);
            values[8..12].fill(default.to_bits());
            let sources = [source.unwrap_or_else(|| r.empty_view.clone()), r.empty_view.clone(), r.empty_view.clone(), r.empty_view.clone()];
            self.jobs.push(Job::Reduce { output: output.clone(), sources, values, size, kernel: Reduction::Pages, format:output.texture().format() });
            if let Some(slot) = scratch { self.free(slot); }
        }
        Ok(changed)
    }
}

/// The texel of `plan` where page `tile` of levels lying `shift` pixels
/// after the plan begins.
fn output_origin(tile: [u32; 2], shift: [i64; 2], plan: display_mips::Plan) -> [u32; 2] {
    std::array::from_fn(|i| ((i64::from(tile[i] * PAGE_SIZE) + shift[i] - i64::from([plan.bounds.min_x(), plan.bounds.min_y()][i])) >> plan.level) as u32)
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    use layer_render::CanvasRenderer;

    fn source_renderer(document:&layer_core::Document)->WgpuRasterizer {
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.ensure_document_metadata(document.composition().size,document.scene()).unwrap();
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,crate::test_support::packet(document.scene(),document.composition().size));
        let mut required=crate::compile::Needs::capture_scene(&renderer,document.scene(),false);
        required.render.extend(crate::startup::SourceKinds::for_scene(document.scene()).pipelines(&renderer.scene_pipelines.source).cloned());
        pollster::block_on(required.prepare(&renderer.compilation)).unwrap();
        renderer
    }
    #[test]
    fn accepted_pages_and_scratch_are_charged_once_before_settle() {
        for tiled in [true,false] {
            let extent=[256;2];
            let document=crate::scene::scale::tests::document_at(extent);
            let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
            r.ensure_document_metadata(extent,document.scene()).unwrap();
            let frame=crate::test_support::packet(document.scene(),extent);
            crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
            let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
            let plan=display_mips::Plan::at(extent,1);
            let mut cache=Cache::new(&r,Request {plan,evaluation:Evaluation::Display},document.scene().order().len());
            crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
            cache.prepare_graph(&r,frame,&scene).unwrap();
            let area=plan.bounds;
            let interest=cache.parent_interest(area);let key=interest.key.clone();
            let before=r.scene_tiers.display.used();
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            let accepted=cache.accept_working(&mut scene,&mut r,interest,vec![(key.clone(),area)],
                super::super::super::sources::Inputs::default(),[2,0],plan,tiled,Class::Visible,Class::Visible,&mut encoder).unwrap();
            assert!(accepted.is_ok(),"tiled={tiled}");
            let pages=cache.pages.lock().unwrap().bytes();
            let scratch=if tiled {scene.pool_bytes()}else {cache.output.iter().map(|image|image.plan.size.map(u64::from).into_iter().product::<u64>()*16).sum()};
            assert!(pages>0 && scratch>0,"fixture must allocate pages and scratch, tiled={tiled}");
            assert_eq!(r.scene_tiers.display.used()-before,pages+scratch,"accepted page and scratch reservations must become the child charges, tiled={tiled}");
            r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        }
    }

    #[test]
    fn a_unit_refused_on_its_scratch_waits_until_its_pages_and_scratch_fit_together() {
        let extent=[256;2];
        let document=crate::scene::scale::tests::document_at(extent);
        let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=crate::test_support::packet(document.scene(),extent);
        crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
        let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
        let plan=display_mips::Plan::at(extent,1);
        let mut cache=Cache::new(&r,Request {plan,evaluation:Evaluation::Display},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        cache.prepare_graph(&r,frame,&scene).unwrap();
        let area=plan.bounds;
        let interest=cache.parent_interest(area);let key=interest.key.clone();
        let scratch=2*super::super::super::scratch_page_bytes(r.device.working_format());
        r.scene_tiers.display.update_limit(r.scene_tiers.display.used()+key.bytes()+scratch-1);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let refused=cache.accept_working(&mut scene,&mut r,interest,vec![(key.clone(),area)],
            super::super::super::sources::Inputs::default(),[2,0],plan,true,Class::Visible,Class::Visible,&mut encoder).unwrap();
        let Err(Some(refused))=refused else {panic!("the page fits but its scratch does not, so the unit is refused");};
        assert!(!refused.fits(),"the refused unit cannot fit once its page reservation is released: {refused:?}");
    }

    fn placed_page_case(transform:layer_core::ImageTransform,offset:[i64;2],native:bool) {
        let extent=[768;2];
        let mut document=crate::scene::scale::tests::document_at(extent);
        let handle=document.scene().order()[0];
        document.artwork.occurrences.get_mut(handle).unwrap().offset=offset;
        let target=document.scene().source_target(handle).unwrap();
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.ensure_document_metadata(extent,document.scene()).unwrap();
        let packet=crate::test_support::packet(document.scene(),extent);
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,packet);
        let mut scene=Scene::new(&renderer).unwrap();scene.ordered=true;
        scene.scale_sources.prepare(&renderer,packet,&[]);
        let geometry=scene.raw_placement(transform,PixelRect::full(extent),offset);
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        renderer.prepare_uploads(packet,&mut [],&mut encoder).unwrap();
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,packet,&scene);
        let expected=scene.scale_pages.lock().unwrap().expected().unwrap().clone();
        let warm=crate::scene::sources::declare_colors(&renderer,packet.scene,expected.clone(),Some(encoder.submission()),target,PixelRect::full(extent)).unwrap();
        let warm_leases=crate::test_support::prepare_raster(&mut renderer,|r| {
            warm.acquire(r,Class::Visible)
        }).unwrap();
        let geometry=if native {super::super::super::placement::Placement::source(source_frame(Some(&scene.scale_sources),packet.scene,target),extent)}else {geometry};
        let mut placed=super::super::super::sources::Inputs::default();placed.source_placement(target,geometry.clone()).unwrap();
        let declaration=super::super::super::sources::Declaration::scene(expected.clone(),Some(encoder.submission()));
        let requested=scene.declare(&mut renderer,placed,declaration,|scene,r,accepted|scene.placed_color(r,packet.scene,target,&geometry,[0;2],accepted).map(drop))
            .unwrap().for_unit(crate::scene::sources::InputUnit::tile([0;2])).unwrap();
        super::super::super::sources::Inputs::begin_accepted_reads();
        let mut accepted=crate::test_support::prepare_raster(&mut renderer,|r| {
            requested.acquire(r,Class::Visible)
        }).unwrap();
        let geometry=accepted.placement(target).unwrap();
        let actual={let _record=crate::scene::sources::record_reads(&accepted);scene.placed_color(&renderer,packet.scene,target,&geometry,[0;2],&mut accepted)};
        let (actual,flow)=actual.unwrap();assert!(flow.is_none());let _actual=scene.own(actual);
        let actual=scene.pool[actual].texture.clone();
        let reads=super::super::super::sources::Inputs::take_accepted_read_results();
        assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"the placed page must read its source footprint");
        assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{reads:?}");
        let mut warm_leases=warm_leases;
        let reference=scene.placed_color(&renderer,packet.scene,target,&geometry,[0;2],&mut warm_leases);
        let (reference,flow)=reference.unwrap();assert!(flow.is_none());let _reference=scene.own(reference);
        let reference=scene.pool[reference].texture.clone();
        let declaration=crate::scene::sources::Inputs::default().for_unit(crate::scene::sources::InputUnit::tile([0;2])).unwrap();let mut accepted=Some(accepted);
        pollster::block_on(scene.complete_capture_plan(&mut renderer,&mut encoder,None,&declaration,&mut accepted,|scene,r,encoder,_|scene.encode_jobs(r,encoder))).unwrap();
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        let actual=crate::test_support::float_pixels(&renderer,&actual);
        let reference=crate::test_support::float_pixels(&renderer,&reference);
        for (index,(actual,reference)) in actual.into_iter().zip(reference).enumerate() {
            assert_eq!(actual.map(f32::to_bits),reference.map(f32::to_bits),"pixel {index}");
        }
        drop((accepted,warm_leases));
    }
    #[test]
    fn placed_page_reads_stay_inside_their_accepted_footprint() {
        placed_page_case(Default::default(),[-256,0],true);
    }
    #[test]
    fn subpixel_placement_keeps_its_sampling_halo() {
        placed_page_case(layer_core::ImageTransform::affine(layer_core::Affine([1.,0.,0.,1.,-255.5,0.])),[0;2],false);
    }
    #[test]
    fn scaled_placement_declares_its_page_crossing_sources() {
        placed_page_case(layer_core::ImageTransform::affine(layer_core::Affine([2.,0.,0.,2.,0.,0.])),[-255,-129],false);
    }
    #[test]
    fn subpage_demand_declares_the_complete_output_tile() {
        placed_page_case(Default::default(),[-255,0],true);
    }
    #[test]
    fn graph_fast_path_reads_its_accepted_source_descriptor() {
        let extent=[256;2];let document=crate::scene::scale::tests::document_at(extent);
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        let frame=crate::test_support::packet(document.scene(),extent);
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
        super::super::super::sources::Inputs::begin_accepted_reads();
        crate::test_support::submit_published(&mut renderer,frame);
        let reads=super::super::super::sources::Inputs::take_accepted_read_results();
        assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"the graph fast path must read its source");
        assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{reads:?}");
    }

    #[test]
    fn seeded_accepted_source_reads_stay_inside_the_collected_inputs() {
        use layer_core::raster::*;
        let mut failures=Vec::new();
        for (name,seed,base_offset,material,mask) in [
            ("phase-x",0x0025u32,[0,0],false,false),
            ("phase-y",0x6500,[0,0],false,false),
            ("phase-xy",0x6525,[0,0],false,false),
            ("paint-base",0x6525,[13,29],false,false),
            ("watercolor",0x6525,[0,0],true,false),
            ("mask",0x6525,[0,0],false,true),
            ("placed",0x6525,[0,0],false,false),
            ("placed-watercolor",0x6525,[0,0],true,false),
            ("preview-contribution",0x0000,[0,0],false,false),
        ] {
            let offset=[i64::from(seed&255),i64::from((seed>>8)&255)];
            let extent=[768;2];
            let mut document=crate::scene::scale::tests::document_at(extent);
            let handle=document.scene().order()[0];
            document.artwork.occurrences.get_mut(handle).unwrap().offset=offset;
            crate::scene::scale::tests::paint_mut(&mut document,0).base.as_mut().unwrap().offset=base_offset;
            if material {
                let mut data=RasterData {watercolor:Some(RasterWatercolor {wet_edge:0.9,burnt_edge:0.6,edge_width:8.}),..Default::default()};
                let plane=RasterPlane::WatercolorWetness;
                let tile=RasterTile::backed(TileBlob::encode(plane.descriptor(document.composition().color),&vec![127;256*256]).unwrap());
                for y in 0..3 {for x in 0..3 {data.tiles.insert(TileKey {plane,coordinate:[x,y]},tile.clone());}}
                crate::scene::scale::tests::paint_mut(&mut document,0).raster=RasterRevision::backed(data);
            }
            if name=="preview-contribution" {
                let filter=crate::scene::scale::tests::effect_occurrence(&mut document,layer_core::EffectInstance::new(crate::tests::fixture("threshold").program()),"threshold");
                document.artwork.occurrences.get_mut(filter).unwrap().attachment=layer_core::authored::Attachment::Effect;
                crate::scene::scale::tests::insert_occurrence(&mut document,filter,0);
            }
            let id=if mask {
                let coverage=crate::scene::scale::tests::coverage_mask(&mut document,handle,[0;2],Some(layer_core::Selection::polygon(vec![layer_core::Point::default(),layer_core::Point {x:768.,y:0.},layer_core::Point {x:0.,y:768.}]).unwrap()));
                SourceTarget::Coverage(coverage)
            }else {document.scene().source_target(handle).unwrap()};
            let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
            renderer.ensure_document_metadata(extent,document.scene()).unwrap();
            if material || mask {
                let data=if mask {document.artwork.coverage.get(match id {SourceTarget::Coverage(id)=>id,_=>unreachable!()}).unwrap().raster.wait_data().unwrap()}else {document.scene().paint_source(handle).unwrap().raster.wait_data().unwrap()};
                renderer.publish_cold_raster(id,&data).unwrap();
                assert!(renderer.native_backing(id).is_some());
                if material {assert!(renderer.watercolor_style(id,&[]).is_some());}
            }
            let mut frame=crate::test_support::packet(document.scene(),extent);
            crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
            if name=="preview-contribution" {
                let dab=crate::tests::test_dab([384.,384.],[0.02,0.07,0.15,0.63],0.9);
                let mut preview=crate::test_support::dab_batch(id,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
                preview.kind=layer_render::DabBatchKind::Preview;preview.stroke_end=false;
                frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
                crate::test_support::submit_published(&mut renderer,frame);
                crate::test_support::submit_ready(&mut renderer,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&preview),..frame}).unwrap();
                assert!(renderer.preview_contribution && !renderer.preview_requires_base,"real contribution fixture");
                renderer.paint_layers.iter_mut().find(|layer|layer.id==id).unwrap().pages.retain(|page|page.coordinate!=[1,1]);
            }
            let mut scene=Scene::new(&renderer).unwrap();scene.scale_sources.prepare(&renderer,frame,&[]);
            let placed=name.starts_with("placed");
            let plan=display_mips::Plan::at(extent,if placed {0}else {1});
            let mut cache=Cache::new(&renderer,Request {plan,evaluation:Evaluation::Display},1);
            let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
            renderer.prepare_uploads(frame,&mut [],&mut encoder).unwrap();
            crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
            let expected=scene.scale_pages.lock().unwrap().expected().unwrap().clone();
            let warm=crate::scene::sources::declare_colors(&renderer,frame.scene,expected.clone(),Some(encoder.submission()),id,PixelRect::full(extent)).unwrap();
            let warm_leases=crate::test_support::prepare_raster(&mut renderer,|r| {
                warm.acquire(r,Class::Visible)
            }).unwrap();
            warm.take_reads();
            super::super::super::sources::Inputs::begin_accepted_reads();
            let outcome=if placed {
                let area=page_rect([1,1]);
                crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
                cache.prepare_graph(&renderer,frame,&scene).unwrap();
                let interest=cache.parent_interest(area);let input_level=interest.input_level();let parent=interest.key.clone();
                let placement=scene.raw_placement(Default::default(),PixelRect::full(extent),offset);
                let declaration=super::super::super::sources::Declaration::scene(expected.clone(),Some(encoder.submission()));
                let inputs=scene.declare(&mut renderer,Default::default(),declaration,|scene,r,accepted| {
                    let output=scene.reserve(r)?;let output=scene.pool[output].view.clone();
                    scene.reduce_color_pages(r,frame,handle,parent.plan(),&output,&[[1,1]],Some(&placement),[0;2],&BTreeMap::new(),accepted).map(drop)
                }).unwrap();
                assert!(cache.accept_working(&mut scene,&mut renderer,interest,vec![(parent.clone(),area)],inputs,[16,0],plan,true,crate::residency::Class::Visible,crate::residency::Class::Visible,&mut encoder).unwrap().is_ok(),"{name}: real native parent must be accepted");
                let image=cache.pages.lock().unwrap().claim(&renderer,parent.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap();
                let mut inputs=cache.pages.lock().unwrap().take_fixture_inputs(&parent,area,input_level);
                let _record=crate::scene::sources::record_reads(&inputs);
                scene.reduce_color_pages(&mut renderer,frame,handle,image.plan,&image.view,&[[1,1]],Some(&placement),[0;2],&BTreeMap::new(),&mut inputs).map(drop)
            }else {
                crate::test_support::prepare_raster(&mut renderer,|r| {
                    cache.prepare_source(&mut scene,r,frame,&mut encoder,id,SourceRequest {
                        plan,required:page_rect([1,1]).into(),covered:PixelRect::EMPTY,
                    },Class::Prefetch,&mut Scene::begin_work(false))
                }).map(drop)
            };
            let reads=super::super::super::sources::Inputs::take_accepted_read_results();
            drop(warm_leases);
            let count=reads.iter().map(|(count,_)|*count).sum::<usize>();
            if name=="phase-x" || name=="phase-y" {assert_eq!(count,2,"{name}");}
            if name=="phase-xy" {assert_eq!(count,4,"{name}");}
            if count==0 || reads.iter().any(|(_,missing)|!missing.is_empty()) || outcome.is_err() {
                failures.push(format!("{name}: seed={seed:#x} offset={offset:?} base={base_offset:?} reads={reads:?} outcome={outcome:?}"));
            }
            renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        }
        assert!(failures.is_empty(),"{}",failures.join("\n"));
    }

    #[test]
    fn overlapping_source_plan_keeps_accepted_input_leases() {
        let extent=[768,256];
        let mut document=crate::scene::scale::tests::document_at(extent);
        let source=layer_core::color::source::rgba8_source(extent,|_,_|[31,71,113,255]);
        crate::scene::scale::tests::paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=source_renderer(&document);
        let frame=crate::test_support::packet(document.scene(),extent);
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        let pixels=crate::prepare_image_tile(&source,[1,0],renderer.device.working_space(),&renderer.device.source_samples,None).unwrap();
        Scene::decode_prepared_for(&mut renderer,&mut encoder,&source,[1,0],pixels,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        let view=renderer.source_tiles.borrow().prepared_view(&source,[1,0]).unwrap().clone();
        let unleased_bytes=renderer.source_tiles.borrow().leased_bytes();
        let lease=renderer.source_tiles.borrow().lease(&view).unwrap();
        let leased_bytes=renderer.source_tiles.borrow().leased_bytes();
        assert!(leased_bytes>unleased_bytes);
        let held=Arc::downgrade(&lease);
        let mut scene=Scene::new(&renderer).unwrap();scene.scale_sources.prepare(&renderer,frame,&[]);
        let id=document.scene().source_target(document.scene().order()[0]).unwrap();
        let before=display_mips::Plan::window(extent,1,PixelRect::new(0,0,512,256));
        let mut cache=Cache::new(&renderer,Request {plan:before,evaluation:Evaluation::Display},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let key=scene.scale_sources.entries[&id].key(&renderer,id,1,[0;2]);
        let image=cache.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap();
        let parent=cache.page_key(cache.graph.root.clone().unwrap(),before.bounds);
        drop(cache.pages.lock().unwrap().claim(&renderer,parent.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap());
        pages::accept_fixture_inputs(&mut cache.pages.lock().unwrap(),&parent,parent.plan().bounds,Evaluation::Display,vec![(key.clone(),parent.plan().bounds,image)],parent.level);
        pages::hold_fixture_inputs(&mut cache.pages.lock().unwrap(),&key,page_rect([1,0]),Evaluation::Display,vec![lease].into(),key.level);
        assert!(held.upgrade().is_some());
        assert_eq!(renderer.source_tiles.borrow().leased_bytes(),leased_bytes);
        let after=display_mips::Plan::window(extent,1,PixelRect::new(256,0,768,256));
        scene.scale_sources.prepare(&renderer,FramePacket {composite_all:false,..frame},&[]);
        (cache,_)=Cache::select(Some(cache),&renderer,frame,Request {plan:after,evaluation:Evaluation::Display},true);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        assert!(held.upgrade().is_some(),"overlapping accepted input must remain pinned while its page is incomplete");
        assert_eq!(renderer.source_tiles.borrow().leased_bytes(),leased_bytes);
        assert!(cache.pages.lock().unwrap().get(&key,page_rect([1,0]),pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_none());
        let inputs=cache.pages.lock().unwrap().take_fixture_inputs(&key,page_rect([1,0]),key.level);
        assert_eq!(inputs.len(),1);
        assert!(held.upgrade().is_some());
        drop(inputs);
        assert!(held.upgrade().is_none());
        assert_eq!(renderer.source_tiles.borrow().leased_bytes(),unleased_bytes);

    }

    #[test]
    fn tiled_main_keeps_the_display_overview_source_request() {
        let extent=[4096,2048];
        let mut document=crate::scene::scale::tests::document_at(extent);
        let source=layer_core::color::source::rgba8_source([256;2],|_,_|[31,71,113,255]);
        crate::scene::scale::tests::paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.into()));
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=crate::test_support::packet(document.scene(),extent);
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
        let id=document.scene().source_target(document.scene().order()[0]).unwrap();
        renderer.set_transform_preview(Some(&layer_render::TransformPreview {transaction:1,target:id,moving:true,selection:None,transform:Default::default()})).unwrap();
        let mut scene=Scene::new(&renderer).unwrap();scene.scale_sources.prepare(&renderer,frame,&[]);
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::window(extent,0,PixelRect::new(0,0,512,512)),evaluation:Evaluation::Tiled(2)},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let overview=cache.overview.as_ref().unwrap();
        assert!(overview.evaluation==Evaluation::Display);
        let level=source_plan(overview.plan,source_frame(Some(&scene.scale_sources),document.scene(),id),extent,false,0).level;
        assert!(level>0);
        scene.scale_sources.prepare(&renderer,FramePacket {composite_all:false,..frame},&[]);
        let key=scene.scale_sources.entries[&id].key(&renderer,id,level,[0;2]);
        let held=Arc::new(());
        let parent=overview.page_key(overview.graph.root.clone().unwrap(),page_rect([0;2]));
        let image=cache.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap();
        drop(cache.pages.lock().unwrap().claim(&renderer,parent.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap());
        pages::accept_fixture_inputs(&mut cache.pages.lock().unwrap(),&parent,parent.plan().bounds,Evaluation::Display,vec![(key.clone(),page_rect([0;2]),image)],parent.level);
        pages::hold_fixture_inputs(&mut cache.pages.lock().unwrap(),&key,page_rect([0;2]),Evaluation::Display,vec![held.clone().into()].into(),key.level);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        assert_eq!(Arc::strong_count(&held),2,"tiled main inputs cannot cancel the display overview's accepted source unit");
        assert_eq!(cache.pages.lock().unwrap().take_fixture_inputs(&key,page_rect([0;2]),key.level).len(),1);
    }

    fn partial_source()->(layer_core::Document,Arc<layer_core::color::source::SourceImage>,WgpuRasterizer,pages::Key,wgpu::Texture) {
        let extent=[512;2];
        let mut document=crate::scene::scale::tests::document_at(extent);
        let source=layer_core::color::source::rgba8_source(extent,|x,y|[x as u8,y as u8,83,255]);
        crate::scene::scale::tests::paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let handle=document.scene().order()[0];let id=document.scene().source_target(handle).unwrap();
        let mut previous=source_renderer(&document);
        let frame=crate::test_support::packet(document.scene(),extent);
        let mut encoder=submission::CommandEncoder::new(&previous.device,&Default::default());
        let pixels=crate::prepare_image_tile(&source,[0;2],previous.device.working_space(),&previous.device.source_samples,None).unwrap();
        Scene::decode_prepared_for(&mut previous,&mut encoder,&source,[0;2],pixels,super::super::super::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        let mut scene=Scene::new(&previous).unwrap();scene.scale_sources.prepare(&previous,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&previous,frame,&scene);
        let mut cache=Cache::new(&previous,Request {plan:display_mips::Plan::at(extent,1),evaluation:Evaluation::Display},document.scene().order().len());
        cache.prepare_source(&mut scene,&mut previous,frame,&mut encoder,id,SourceRequest {
            plan:display_mips::Plan::at(extent,1),required:page_rect([0;2]).into(),covered:PixelRect::EMPTY,
        },Class::Prefetch,&mut Scene::begin_work(false)).unwrap();
        let key=scene.scale_sources.entries[&id].key(&previous,id,1,[0;2]);
        let texture=cache.pages.lock().unwrap().get(&key,page_rect([0;2]),pages::Accuracy::Display,key.level,crate::residency::Class::Visible).unwrap().texture.clone();
        let composite=pages::Key {source:None,..key.clone()};
        assert!(cache.pages.lock().unwrap().get(&composite,page_rect([0;2]),pages::Accuracy::Display,composite.level,crate::residency::Class::Visible).is_none(),"raw source pixels cannot satisfy a composite page with the same expression");
        assert!(cache.pages.lock().unwrap().coverage(&composite,page_rect([0;2]),composite.level).written.is_empty());
        assert!(cache.pages.lock().unwrap().get(&key,page_rect([1,0]),pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_none(),"an unwritten chunk sharing the source allocation cannot be sampled");
        previous.uploads.finish(&encoder);encoder.submit(&previous.queue);previous.wait_idle().unwrap();
        previous.scene=Some(scene);previous.scale_display=Some(cache);
        (document,source,previous,key,texture)
    }

    #[test]
    fn dropping_display_keeps_committed_raw_source_pages_with_the_scene() {
        let (document,_,mut renderer,key,_)=partial_source();
        let source={
            let image=renderer.scale_display.as_ref().unwrap().pages.lock().unwrap().get(&key,page_rect([0;2]),pages::Accuracy::Display,key.level,crate::residency::Class::Visible).unwrap();
            Arc::downgrade(image.image())
        };
        renderer.scale_display=None;
        let retained=source.upgrade().expect("dropping a display cannot discard the retained scene's committed raw source page");
        assert!(renderer.composition_storage_bytes()>=retained.bytes(),"retained source storage stays reported without a display window");
        let mut scene=renderer.scene.take().unwrap();
        let frame=crate::test_support::packet(document.scene(),document.composition().size);
        scene.scale_sources.prepare(&renderer,FramePacket {composite_all:false,..frame},&[]);
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::at(document.composition().size,1),evaluation:Evaluation::Display},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let restored=cache.pages.lock().unwrap().get(&key,page_rect([0;2]),pages::Accuracy::Display,key.level,crate::residency::Class::Visible).unwrap();
        assert!(Arc::ptr_eq(&retained,restored.image()),"a new display reuses the committed source allocation without reduction");
        assert!(cache.pages.lock().unwrap().get(&key,page_rect([1,0]),pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_none());
    }

    #[test]
    fn same_device_replacement_keeps_partial_source_preparation() {
        let (document,source,mut previous,key,texture)=partial_source();
        let id=key.source.unwrap();let color=document.composition().color;
        let frame=crate::test_support::packet(document.scene(),document.composition().size);
        let factory=previous.factory.clone();
        let mut next=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(color),Initialization::Headless).unwrap();
        next.adopt_committed_pages(&mut previous);
        assert!(next.source_tiles.borrow().prepared_view(&source,[0;2]).is_some(),"decoded inputs survive replacement");
        let mut scene=next.scene.take().expect("reduced source progress survives replacement");
        scene.scale_sources.prepare(&next,FramePacket {reset_layers:true,..frame},&[]);
        let cache=next.scale_display.as_ref().unwrap();
        let next_key=scene.scale_sources.entries[&id].key(&next,id,1,[0;2]);
        assert!(next_key==key);
        let kept=cache.pages.lock().unwrap().get(&next_key,page_rect([0;2]),pages::Accuracy::Display,next_key.level,crate::residency::Class::Visible);
        assert_eq!(kept.expect("physical raster reset cannot discard unchanged source preparation").texture,texture);
        assert!(cache.pages.lock().unwrap().get(&next_key,page_rect([1,0]),pages::Accuracy::Display,next_key.level,crate::residency::Class::Visible).is_none());
        assert!(next.source_tiles.borrow().prepared_view(&source,[0;2]).is_some());
    }
}
