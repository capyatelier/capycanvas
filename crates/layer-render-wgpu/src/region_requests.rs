//! Single-flight region queries. Keep the GPU result for rendering and capture
//! one packed history copy asynchronously, with no CPU flood/raster algorithm.
//! Refinements run across frames, one bounded chunk per poll.
use super::*;
use flood::{Flood, Region};
use layer_render::{RegionRequest, RegionResult, RegionTask, RegionOutput};
use selection_refine::{Downsample, Job, ModifyPlan, SelectionRefiner, Stage};
use std::sync::{Arc, Weak};
#[path = "region_cache.rs"]
mod cache;
#[path = "region_areas.rs"]
mod areas;

pub(super) struct RegionRequests {
    pub(super) flood: Flood,
    pub(super) raw: region_sources::RawRegions,
    cache: Option<cache::Cache>,
    sampling: region_sources::Sampling,
    cache_request: Option<RegionRequest>,
    stop_mask: Option<wgpu::Buffer>,
    chunk_timing:Option<u64>,
    preparation:Option<Preparation>,
    refiner: Option<selection_refine::SelectionRefiner>,
    readback: Option<wgpu::Buffer>,
    fill_seed:Option<(SourceTarget,layer_core::Point,Option<wgpu::Buffer>)>,
    pending: Option<Region>,
    waiting: Option<RegionRequest>,
    areas: Option<areas::AreasJob>,
    running: Option<Running>,
    /// Rasterizes a warp mesh for selections resampled through it.
    positions: Option<paint_transform::mesh::Positions>,
    rx: Option<mpsc::Receiver<Result<(RegionResult,bool), GpuRasterError>>>,
    modify_source: Option<ModifySource>,
    charge: crate::residency::Charge,
}
struct Preparation {request:RegionRequest,enclosed:bool,recipes_ready:bool}
/// The selection Select › Modify refines, prepared at document resolution,
/// and the levels of its preview pyramid built so far, each halving the last
/// over `region`.
struct ModifySource {
    selection: Weak<layer_core::Selection>,
    extent: [u32; 2],
    document: wgpu::Buffer,
    region: [u32; 4],
    levels: Vec<wgpu::Buffer>,
}
struct Running {
    job: Job,
    request_id: u64,
    probe: Option<layer_render::TonalProbe>,
    placement: layer_core::Affine,
    /// Pyramid levels this job builds, for later previews.
    levels: Vec<wgpu::Buffer>,
    underfill: Option<(wgpu::Buffer,u64)>,
}
impl RegionRequests {
    fn refinement(request:&RegionRequest)->layer_render::RegionRefinement {
        layer_render::RegionRefinement {close_gaps:request.refinement.close_gaps || request.fill.is_some(),..request.refinement}
    }
    pub fn storage_bytes(&self) -> u64 {
        self.flood.storage_bytes()
            + self.raw.storage_bytes()
            + self.areas.as_ref().map_or(0,areas::AreasJob::storage_bytes)
            + self.cache.as_ref().map_or(0,|cache|cache.seeds.size())
            + self.cache.as_ref().and_then(|cache|cache.mask.as_ref()).filter(|mask|self.raw.mask().as_ref()!=Some(mask)).map_or(0,wgpu::Buffer::size)
            + self.stop_mask.as_ref().map_or(0,wgpu::Buffer::size)
            + self.readback.as_ref().map_or(0,wgpu::Buffer::size)
            + self.fill_seed.as_ref().and_then(|(_,_,buffer)|buffer.as_ref()).map_or(0,wgpu::Buffer::size)
            + self.pending.as_ref().map_or(0, Region::storage_bytes)
            + self.running.as_ref().map_or(0, |r| r.job.storage_bytes())
            + self.modify_source.as_ref().map_or(0, |s| s.document.size() + s.levels.iter().map(wgpu::Buffer::size).sum::<u64>())
    }
    pub(super) fn new(r: &WgpuRasterizer) -> Self {
        let device=&r.device;
        Self {
            flood: Flood::new(device),
            raw: region_sources::RawRegions::new(device),
            cache:None,sampling:region_sources::Sampling::Pixels,cache_request:None,stop_mask:None,chunk_timing:None,preparation:None,
            refiner: None,
            readback: None,fill_seed:None,
            pending: None,
            waiting: None,
            areas: None,
            running: None,
            positions: None,
            rx: None,
            modify_source: None,
            charge: r.raster_tiers.paint.charge(0),
        }
    }
    /// Drop the request in progress; a readback already in flight is ignored.
    pub(super) fn cancel(&mut self) {
        self.raw.cancel();
        self.cache_request=None;self.stop_mask=None;self.fill_seed=None;
        self.waiting = None;
        self.areas = None;
        self.running = None;
        if self.pending.take().is_some() {
            self.rx = None;
            self.readback=None;
        }
    }
    fn refiner(&mut self, r: &WgpuRasterizer) -> &mut selection_refine::SelectionRefiner {
        self.refiner.get_or_insert_with(|| selection_refine::SelectionRefiner::new(&r.device, &r.queue))
    }
    fn positions(&mut self, r: &WgpuRasterizer) -> &mut paint_transform::mesh::Positions {
        self.positions.get_or_insert_with(|| {
            r.transforms.as_ref().map_or_else(
                || paint_transform::mesh::Positions::new(&r.device),
                paint_transform::mesh::Positions::sharing,
            )
        })
    }
    /// A pixel selection's coverage resampled through `map` into `extent`,
    /// through a warp mesh a window of positions at a time.
    fn resample_mapped(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        selection: &layer_core::Selection,
        map: &layer_core::LayerPlacement,
    ) -> Result<wgpu::Buffer, GpuRasterError> {
        use paint_transform::mesh::{MeshGeometry, WINDOW_PAGES};
        let source = r.selection_clip.mapped_source(&r.device, selection, map)?;
        let [w, h] = extent;
        let bytes = 32 + u64::from(w.div_ceil(4)) * u64::from(h) * 4 + 32;
        let limits = r.device.limits();
        let mesh = map.mesh.as_ref();
        if extent.contains(&0)
            || bytes > limits.max_storage_buffer_binding_size
            || (mesh.is_none() && h > limits.max_compute_workgroups_per_dimension)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let output = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mapped selection coverage"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut header = [0u8; 32];
        for (dst, value) in header.as_chunks_mut::<4>().0.iter_mut().zip([0, 0, w, h, 0, 2, 0, 0]) {
            dst.copy_from_slice(&value.to_ne_bytes());
        }
        r.uploads.write(encoder, &output, &header)?;
        let Some(mesh) = mesh else {
            r.selection_clip.resample_window(&r.device, encoder, &source, &output, PixelRect::full(extent), None);
            return Ok(output);
        };
        let geometry = std::sync::Arc::new(MeshGeometry::new(mesh, map.outer, None));
        let side = WINDOW_PAGES * PAGE_SIZE;
        let positions = self.positions(r);
        positions.upload(r, encoder, &geometry)?;
        let view = positions.view(&r.device, [side; 2]);
        let drawn = pixel_rect(map.forward_bounds(mesh.bounds()), extent);
        for y in (drawn.min_y() / side..drawn.max_y().div_ceil(side)).map(|n| n * side) {
            for x in (drawn.min_x() / side..drawn.max_x().div_ceil(side)).map(|n| n * side) {
                positions.draw(r, encoder, [x / PAGE_SIZE, y / PAGE_SIZE])?;
                let rect = PixelRect::new(x, y, (x + side).min(w), (y + side).min(h));
                r.selection_clip.resample_window(&r.device, encoder, &source, &output, rect, Some((&view, w)));
            }
        }
        Ok(output)
    }
    fn paper(r:&WgpuRasterizer,request:&RegionRequest)->Result<(wgpu::TextureView,[f32;4],u32),GpuRasterError> {
        request.paper.as_ref().map(|binding| {
            let tile=paper::PaperTile::new(&binding.surface)?;
            let view=paper::PaperTile::id_for(binding).and_then(|id|r.papers.view(&id)).ok_or(GpuRasterError::NotReady)?;
            Ok((view.clone(),tile.grain(binding),tile.grain_radius()))
        }).unwrap_or_else(||Ok((r.empty_paper_view.clone(),[1.,1.,0.,0.],1)))
    }
    fn build_cache(&mut self,r:&mut WgpuRasterizer,request:&RegionRequest,enclosed:bool,encoder:&mut crate::submission::CommandEncoder,demand:bool,areas:bool)->Result<(),GpuRasterError> {
        let source=cache::source(r,request);
        let base=RegionRequest {source:source.clone(),limit:None,reference_count:None,..request.clone()};
        if self.flood.lines_discarded() || self.cache.as_ref().is_none_or(|cache|!cache.matches(r,request,&source,enclosed) || matches!(cache.sampling,region_sources::Sampling::Areas(_))!=areas) {
            self.raw.cancel();self.flood.cancel_lines();self.stop_mask=None;
            if areas {self.sampling=region_sources::Sampling::Areas(layer_core::FillAreas::TransparentOrWhite);} else if self.sampling!=region_sources::Sampling::Retained {self.sampling=if enclosed {region_sources::Sampling::Enclosed}else{region_sources::Sampling::Transparent};}
            self.cache=Some(cache::Cache::new(r,request,source.clone(),enclosed));
            self.cache.as_mut().unwrap().sampling=self.sampling;
        }
        if !self.cache.as_ref().unwrap().ready {
            if !self.flood.lines_ready(demand)? {return Err(GpuRasterError::NotReady);}
            if !self.cache.as_ref().unwrap().classified {
                let cache=self.cache.as_ref().unwrap();
                if !cache.dirty.is_empty() {self.sampling=cache.sampling;}
                if !self.raw.processing_with(self.sampling,|old|old.sources().len()==base.source.sources().len()
                    && old.sources().iter().zip(base.source.sources()).all(|(old,new)|cache::same_source(r,old,new))) {
                    let pages=(!cache.dirty.is_empty()).then(||cache.dirty.values().flatten().copied().collect::<std::collections::BTreeSet<_>>().into_iter().collect());
                    let [width,height]=r.document_extent;
                    let bytes=u64::from(width.div_ceil(32))*u64::from(height)*4;
                    let mut working=self.raw.mask().filter(|mask|mask.size()==bytes);
                    if !cache.dirty.is_empty() {
                        let saved=cache.mask.as_ref().unwrap();
                        if let Some(working)=&working {encoder.copy_buffer_to_buffer(saved,0,working,0,saved.size());}
                        else{working=Some(Self::copy(&r.device,encoder,saved,"region working eligibility"));}
                    }
                    self.raw.classification(working,pages);
                    if self.sampling==region_sources::Sampling::Retained && !cache.dirty.is_empty() {encoder.copy_buffer_to_buffer(&cache.seeds,0,self.raw.seeds(),0,cache.seeds.size());}
                }
                self.raw.encode_sampling(r,&base,encoder,self.sampling)?;
                let cache=self.cache.as_mut().unwrap();cache.classified=true;cache.source=source;cache.sampling=self.sampling;
                let classified=self.raw.mask().unwrap();
                if let Some(saved)=&cache.mask {encoder.copy_buffer_to_buffer(&classified,0,saved,0,saved.size());}
                else{cache.mask=Some(Self::copy(&r.device,encoder,&classified,"cached reference classification"));}
            }
            if !self.flood.prepare_cache(&r.compilation,if demand {startup::BRUSH}else{startup::OTHER}) {return Err(GpuRasterError::NotReady);}
            let classified=self.raw.mask().ok_or(GpuRasterError::InvalidExtent)?;
            let (paper,grain,radius)=Self::paper(r,request)?;
            const DEMAND_CHUNKS:f64=3.;
            let budget=self.refiner(r).chunk()*if demand {DEMAND_CHUNKS}else{1.};
            self.chunk_timing=self.refiner(r).begin_timing(encoder);
            let dirty=self.cache.as_ref().unwrap().dirty.values().flatten().copied().collect::<std::collections::BTreeSet<_>>();
            let (ready,taps)=self.flood.build_lines(&flood::Input {device:&r.device,mask:&classified,paper:&paper},encoder,r.document_extent,(grain,radius,request.paper.is_some()),(budget,demand),&dirty)?;
            self.refiner.as_mut().unwrap().end_timing(encoder,self.chunk_timing,taps);
            if !ready {return Err(GpuRasterError::NotReady);}
            let cache=self.cache.as_mut().unwrap();
            if !areas {encoder.copy_buffer_to_buffer(self.raw.seeds(),0,&cache.seeds,0,cache.seeds.size());}
            cache.lastwrite=Some(encoder.submission());
            if std::mem::take(&mut cache.reclassify) {
                cache.classified=false;self.raw.cancel();self.flood.cancel_lines();
                return Err(GpuRasterError::NotReady);
            }
            cache.ready=true;cache.dirty.clear();
        }
        Ok(())
    }
    fn sample_fill_seed(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError> {
        let Some((target,position,buffer))=&mut self.fill_seed else {return Ok(());};
        let rect=scene::fill_edges::seed_rect(*position,r.target_extent(*target));
        let tile=if rect.is_empty() {None}else{r.raw_layer_tile(*target,[rect.min_x()/PAGE_SIZE,rect.min_y()/PAGE_SIZE],encoder)?};
        let buffer=buffer.get_or_insert_with(||r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill seed texel"),size:16,
            usage:wgpu::BufferUsages::COPY_SRC|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false}));
        if let Some(tile)=tile {
            encoder.copy_texture_to_buffer(tile.texture.copy_at([rect.min_x()%PAGE_SIZE,rect.min_y()%PAGE_SIZE]),
                wgpu::TexelCopyBufferInfo {buffer,layout:wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:None,rows_per_image:None}},
                wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1});
        }else {encoder.clear_buffer(buffer,0,None);}
        Ok(())
    }
    fn classify_target(&mut self,r:&mut WgpuRasterizer,request:&RegionRequest,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError> {
        let stop=self.raw.encode_sampling(r,request,encoder,request.fill_areas.map_or(region_sources::Sampling::Target,region_sources::Sampling::Areas))?;
        self.stop_mask=Some(Self::copy(&r.device,encoder,&stop,"Stop target eligibility"));
        Ok(())
    }
    fn sample_colour_mode(&mut self,r:&mut WgpuRasterizer,request:&RegionRequest,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError> {
        self.raw.encode_sampling(r,request,encoder,region_sources::Sampling::TargetSeed)?;
        let mask=self.raw.mask().ok_or(GpuRasterError::InvalidExtent)?;
        let (paper,_,_)=Self::paper(r,request)?;
        self.flood.colour_mode(&flood::Input {device:&r.device,mask:&mask,paper:&paper},encoder,request.colour_mode,Some(self.raw.seed()));
        Ok(())
    }
    fn classify_lines(&mut self,r:&mut WgpuRasterizer,request:&RegionRequest,encoder:&mut crate::submission::CommandEncoder)->Result<wgpu::Buffer,GpuRasterError> {
        let target=if let layer_render::RegionColourMode::Target(target)=request.colour_mode {
            let source=match request.source.sources().first() {
                Some(layer_render::RegionSource::Scene {snapshot,..})=>layer_render::RegionSource::Scene {snapshot:snapshot.clone(),scope:SceneScope::Raw(target)},
                _=>layer_render::RegionSource::Source(target),
            };
            Some(RegionRequest {source,reference_count:None,limit:None,..request.clone()})
        }else{None};
        let target_sampled=target.as_ref().is_some_and(|target|self.raw.processing(&target.source,region_sources::Sampling::TargetSeed));
        if target_sampled {self.sample_colour_mode(r,target.as_ref().unwrap(),encoder)?;}

        if let Some(count)=request.reference_count && count<request.source.sources().len()
            && self.raw.processing(&request.source.sources()[count],request.fill_areas.map_or(region_sources::Sampling::Target,region_sources::Sampling::Areas)) {
            let target=RegionRequest {source:request.source.sources()[count].clone(),reference_count:None,limit:None,..request.clone()};
            self.classify_target(r,&target,encoder)?;
        }
        self.build_cache(r,request,request.enclosure.is_some(),encoder,true,false)?;
        if !self.flood.prepare(&r.compilation,Self::refinement(request),request.enclosure.is_some(),request.contiguous,startup::BRUSH) {return Err(GpuRasterError::NotReady);}
        let source=cache::source(r,request);
        let base=RegionRequest {source,limit:None,reference_count:None,..request.clone()};
        let classified={
            self.raw.encode_sampling(r,&base,encoder,region_sources::Sampling::Seeds)?;
            let mask=self.raw.mask().ok_or(GpuRasterError::InvalidExtent)?;
            let (paper,_,_)=Self::paper(r,request)?;
            self.flood.check_seeds(&flood::Input {device:&r.device,mask:&mask,paper:&paper},encoder,&self.cache.as_ref().unwrap().seeds,self.raw.seeds(),request.position,request.tolerance);
            mask
        };
        if let Some(count)=request.reference_count && count<request.source.sources().len() && self.stop_mask.is_none() {
            let target=RegionRequest {source:request.source.sources()[count].clone(),reference_count:None,limit:None,..request.clone()};
            self.classify_target(r,&target,encoder)?;
        }
        if let Some(target)=&target {
            if !target_sampled {self.sample_colour_mode(r,target,encoder)?;}
        }else{
            let (paper,_,_)=Self::paper(r,request)?;
            self.flood.colour_mode(&flood::Input {device:&r.device,mask:&classified,paper:&paper},encoder,request.colour_mode,None);
        }
        Ok(classified)
    }
    pub fn changed(&mut self,packet:FramePacket<'_>,restored:&[(SourceTarget,PixelRect)]) {
        let Some(cache)=&mut self.cache else {return;};
        if packet.reset_layers {self.cache=None;self.raw.cancel();self.flood.cancel_lines();return;}
        let mut damage=restored.to_vec();
        damage.extend(packet.dab_batches.iter().filter(|batch|batch.kind!=DabBatchKind::Preview)
            .map(|batch|(batch.target,batch_pixel_rect(batch,packet.scene.target_extent(batch.target)))));
        damage.retain(|(target,rect)|cache.reads(*target) && !rect.is_empty());
        if damage.is_empty() {return;}
        if (!cache.ready && !cache.classified && cache.lastwrite.is_none())
            || cache.lastwrite.as_ref().is_some_and(|owner|owner.discarded()) || self.flood.lines_discarded()
            || !cache.local() {
            self.cache=None;self.raw.cancel();self.flood.cancel_lines();return;
        }
        for (target,rect) in damage {
            let rect=DocRect::from(rect).translated(packet.scene.target_offset(target)).in_frame(packet.document_extent);
            cache.dirty.entry(target).or_default().extend(page_coordinates(rect));
        }
        if !cache.ready && cache.classified {cache.reclassify=true;return;}
        cache.ready=false;cache.classified=false;
        self.raw.cancel();self.flood.cancel_lines();self.stop_mask=None;
    }
    pub fn background_pending(&self)->bool {self.preparation.as_ref().is_some_and(|preparation|!preparation.recipes_ready || self.cache.as_ref().is_none_or(|cache|!cache.ready || !cache.submitted()))}
    fn prewarm_idle(&self,r:&WgpuRasterizer)->bool {
        self.pending.is_none() && self.waiting.is_none() && self.running.is_none() && self.areas.is_none() && r.compilation.is_idle()
            && flood::fits(r.document_extent,&r.device.limits())
    }
    fn prewarm(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError> {
        if !self.prewarm_idle(r) {return Ok(());}
        let Some(preparation)=&self.preparation else {return Ok(());};
        let (request,enclosed)=(cache::current(r,&preparation.request),preparation.enclosed);
        if self.cache.as_ref().is_some_and(|cache|cache.pending()) || (!self.flood.lines_ready(false)? && !self.flood.lines_discarded()) {return Ok(());}
        if let Some(binding)=&request.paper {
            r.prepare_paper_binding(binding,startup::OTHER)?;
            if paper::PaperTile::id_for(binding).is_some_and(|id|r.papers.view(&id).is_none()) {return Ok(());}
        }
        {
            let ready=self.raw.prepare(&r.compilation,startup::OTHER);
            self.flood.prepare_cache(&r.compilation,startup::OTHER);
            if !ready {return Ok(());}
        }
        self.refiner(r).poll_timing();
        self.raw.batch_limit(Some(1));
        if self.cache.as_ref().is_none_or(|cache|!cache.matches(r,&request,&cache::source(r,&request),enclosed)) {self.sampling=region_sources::Sampling::Transparent;}
        match self.build_cache(r,&request,enclosed,encoder,false,false) {
            Ok(())=>{
                self.preparation.as_mut().unwrap().recipes_ready=self.flood.prepare(&r.compilation,Self::refinement(&request),enclosed,true,startup::OTHER)
                    & (!enclosed || r.compilation.require(r.selection_clip.pipelines(),startup::OTHER));
                Ok(())
            },
            Err(GpuRasterError::NotReady)=>Ok(()),Err(error)=>Err(error),
        }
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        request: RegionRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.pending.is_some() || self.waiting.is_some() || self.running.is_some() || self.areas.is_some() {
            return Ok(false);
        }
        if let layer_render::RegionSource::Modify(modify) = &request.source {
            let modify = modify.clone();
            return self.start_modify(r, request, &modify);
        }
        let extent = match request.source.raw_source() { layer_render::RegionSource::TransformedSelection {target,..} => r.target_extent(*target), _ => r.document_extent };
        let mapped = matches!(
            request.source,
            layer_render::RegionSource::TransformedSelection { .. }
        );
        let tone = if let layer_render::RegionSource::Tonal(t) = &request.source {
            Some(t.as_ref())
        } else {
            None
        };
        if !request.source.valid_parameters(request.reference_count, request.tolerance)
            || tone.is_some_and(|t| !t.valid(extent))
            || extent.contains(&0)
            || request.position[0] >= extent[0]
            || request.position[1] >= extent[1]
            || !request.refinement.is_valid()
            || request.fill.as_ref().is_some_and(|fill| request.enclosure.is_some() || request.selection.is_some()
                || fill.reach.is_some_and(|reach|!layer_render::RegionGapMasks::STOPS.contains(&reach)))
            || request.selection.as_ref().is_some_and(|s| !s.is_valid())
            || (matches!(
                request.source,
                layer_render::RegionSource::Selection(_)
                    | layer_render::RegionSource::Coverage(_)
                    | layer_render::RegionSource::ObjectCoverage(_)
                    | layer_render::RegionSource::Tonal(_)
            ) && request.selection.is_none())
            || request.enclosure.as_ref().is_some_and(|area| area.validate().is_err()
                || !request.contiguous || request.limit.is_some()
                || !matches!(request.source, layer_render::RegionSource::Composite | layer_render::RegionSource::Source(_) | layer_render::RegionSource::Scene { .. } | layer_render::RegionSource::Intersection(_)))
            || (mapped && (request.selection.is_some() || request.limit.is_some()))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let lines=!mapped && tone.is_none() && !matches!(request.source,layer_render::RegionSource::Selection(_)|layer_render::RegionSource::Coverage(_)|layer_render::RegionSource::ObjectCoverage(_));
        if lines && !flood::fits(extent,&r.device.limits()) { return Err(GpuRasterError::SizeOverflow); }
        if lines && (self.cache.as_ref().is_some_and(|cache|cache.pending()) || !self.flood.lines_ready(true)? && !self.flood.lines_discarded()) {self.waiting=Some(request);return Ok(true);}
        if mapped || request.selection.is_some() {
            self.refiner(r);
        }
        if let Some(binding)=&request.paper {
            r.prepare_paper_binding(binding,startup::BRUSH)?;
            if paper::PaperTile::id_for(binding).is_some_and(|id|r.papers.view(&id).is_none()) {self.waiting=Some(request);return Ok(true);}
        }
        {
            r.compilation.check()?;
            let mut ready = true;
            if let layer_render::RegionSource::TransformedSelection { map, .. } = &request.source {
                if map.mesh.is_some() {
                    ready &= r.compilation.require([&self.positions(r).pipelines.pipeline], startup::BRUSH);
                }
                ready &= r.compilation.require([&r.selection_clip.resample], startup::BRUSH);
                ready &= self.refiner.as_ref().unwrap().prepare_bounds(&r.compilation);
            } else if tone.is_some() {
                ready &= self.raw.prepare_tonal(&r.compilation);
            } else if !matches!(request.source, layer_render::RegionSource::Selection(_)) {
                ready &= self.raw.prepare(&r.compilation,startup::BRUSH);
                if !matches!(request.source, layer_render::RegionSource::Coverage(_) | layer_render::RegionSource::ObjectCoverage(_)) {
                    let prepared=self.flood.prepare(&r.compilation,Self::refinement(&request),request.enclosure.is_some(),request.contiguous,startup::BRUSH);
                    if !lines || self.cache.as_ref().is_some_and(|cache|cache.ready) {ready &= prepared;}
                }
            }
            if let Some(options) = &request.selection {
                ready &= self
                    .refiner
                    .as_ref()
                    .unwrap()
                    .prepare(&r.compilation, [&Stage::refinement(options)], false);
            }
            if request.limit.is_some() || request.enclosure.is_some() || request.selection.is_some() {
                ready &= r.compilation
                    .require(r.selection_clip.pipelines(), startup::BRUSH);
            }
            if !ready {
                self.waiting = Some(request);
                return Ok(true);
            }
        }
        self.raw.batch_limit(None);
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("connected region request"),
            },
        );
        if let Some(selection) = &request.limit {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, selection)?;
        }
        let input = if let layer_render::RegionSource::TransformedSelection {
            selection, map, ..
        } = &request.source
        {
            flood::Region { gaps: None,
                coverage: self.resample_mapped(r, &mut encoder, extent, selection, map)?,
                bounds_offset: 0,
                underfill: None,
            }
        } else if let layer_render::RegionSource::Selection(selection) = &request.source {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, selection)?;
            let buffer = r.selection_clip.buffer.as_ref().unwrap();
            // The next prepare may reuse the clip allocation for the old mask.
            let copy = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("incoming selection"),
                size: buffer.size(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());
            flood::Region { gaps: None,
                coverage: copy,
                bounds_offset: 0,
                underfill: None,
            }
        } else {
            if !lines && self.cache.as_ref().is_some_and(|cache|!cache.ready) {
                self.cache=None;self.raw.cancel();self.flood.cancel_lines();
            }
            r.drain_image_decodes(&mut encoder)?;
            if lines {self.cache_request=Some(request.clone());}
            let classified = match self.sample_fill_seed(r,&mut encoder).and_then(|()|if lines {self.classify_lines(r,&request,&mut encoder)} else {self.raw.encode(r,&request,&mut encoder)}) {
                Err(GpuRasterError::NotReady) => {
                    r.uploads.finish(&encoder);
                    encoder.submit(&r.queue);
                    if let Some(refiner)=&mut self.refiner {refiner.timed(&r.queue,self.chunk_timing.take());}
                    self.waiting = Some(request);
                    return Ok(true);
                },
                result => match result {
                    Ok(classified) => classified,
                    Err(error) => { self.raw.cancel(); return Err(error); },
                },
            };
            if !lines {
                flood::Region { gaps: None,
                    coverage: classified,
                    bounds_offset: 0,
                    underfill: None,
                }
            } else {
                if let Some(enclosure) = &request.enclosure {
                    r.selection_clip.prepare(&r.device, &mut encoder, extent, enclosure)?;
                }
                let (paper,_,_)=Self::paper(r,&request)?;
                self.flood.encode_input(
                    &r.device,
                    &mut encoder,
                    extent,
                    request.position,
                    request.tolerance,
                    request.enclosure.as_ref().or(request.limit.as_ref()).and(r.selection_clip.buffer.as_ref()),
                    request.refinement,
                    &classified,
                    request.contiguous,
                    request.enclosure.is_some(),
                    &paper, self.stop_mask.as_ref().map(|stop|(stop,request.fill_areas.is_none())),request.fill.as_ref(),
                )?
            }
        };
        let direct_tonal = tone.is_some()
            && extent == r.document_extent
            && request.selection.as_ref().is_some_and(|s| {
                s.mode == layer_core::SelectionMode::New
                    && s.antialias
                    && s.feather == 0.
                    && s.resize == 0
                    && s.source_to_document == layer_core::Affine::IDENTITY
            });
        let (region, extent, byte_coverage) = if mapped {
            (
                self.refiner.as_ref().unwrap().bounds_only(
                    &r.device,
                    &mut encoder,
                    extent,
                    input.coverage,
                ),
                extent,
                true,
            )
        } else if direct_tonal {
            self.raw.release_mask();
            (
                self.refiner.as_ref().unwrap().bounds_only(
                    &r.device,
                    &mut encoder,
                    extent,
                    input.coverage,
                ),
                extent,
                true,
            )
        } else if let Some(options) = &request.selection {
            let extent = r.document_extent;
            let external = match &options.previous {
                Some(previous) => {
                    r.selection_clip.prepare(&r.device, &mut encoder, extent, previous)?;
                    Some(Self::copy(&r.device, &mut encoder, r.selection_clip.buffer.as_ref().unwrap(), "previous selection"))
                }
                None => None,
            };
            let [w, h] = extent;
            let underfill = input.underfill;
            let job = self.refiner.as_ref().unwrap().job(
                &r.device,
                &mut encoder,
                extent,
                [0, 0, w, h],
                vec![Stage::refinement(options)],
                input.coverage,
                external,
                Vec::new(),
            )?;
            let probe = tone.and_then(|t| t.probe);
            let running = Running { job, request_id: request.request_id, probe, placement: layer_core::Affine::IDENTITY, levels: Vec::new(), underfill };
            self.advance(r, encoder, running)?;
            return Ok(true);
        } else {
            (input, extent, false)
        };
        let probe = tone.and_then(|t| t.probe);
        self.finish(r, encoder, region, extent, byte_coverage, request.request_id, probe, layer_core::Affine::IDENTITY);
        Ok(true)
    }
    fn copy(device: &wgpu::Device, encoder: &mut crate::submission::CommandEncoder, buffer: &wgpu::Buffer, label: &'static str) -> wgpu::Buffer {
        let copy = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: buffer.size(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());
        copy
    }
    fn start_modify(
        &mut self,
        r: &mut WgpuRasterizer,
        request: RegionRequest,
        modify: &Arc<layer_render::SelectionModify>,
    ) -> Result<bool, GpuRasterError> {
        let extent = r.document_extent;
        if !modify.is_valid() || extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let selection = &modify.selection;
        if self
            .modify_source
            .as_ref()
            .is_some_and(|s| s.extent != extent || s.selection.upgrade().is_none_or(|s| !Arc::ptr_eq(&s, selection)))
        {
            self.modify_source = None;
        }
        let built = self.modify_source.as_ref().map_or(0, |s| s.levels.len() as u32);
        let preview = self.refiner(r).preview();
        let plan = ModifyPlan::new(modify, extent, preview, built);
        {
            r.compilation.check()?;
            let ready = self.refiner.as_ref().unwrap().prepare(&r.compilation, &plan.stages, plan.level > built)
                & r.compilation.require(r.selection_clip.pipelines(), startup::BRUSH);
            if !ready {
                self.waiting = Some(request);
                return Ok(true);
            }
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("modify selection"),
            },
        );
        if self.modify_source.is_none() {
            r.selection_clip.prepare(&r.device, &mut encoder, extent, selection)?;
            self.modify_source = Some(ModifySource {
                selection: Arc::downgrade(selection),
                extent,
                document: Self::copy(&r.device, &mut encoder, r.selection_clip.buffer.as_ref().unwrap(), "selection to modify"),
                region: selection_refine::pyramid_region(modify, extent),
                levels: Vec::new(),
            });
        }
        let source = self.modify_source.as_ref().unwrap();
        let [x0, y0, x1, y1] = source.region;
        let mut downsamples: Vec<Downsample> = Vec::new();
        for level in built + 1..=plan.level {
            let scale = 1 << level;
            let origin = [x0 / scale, y0 / scale];
            let cells = [x1.div_ceil(scale) - origin[0], y1.div_ceil(scale) - origin[1]];
            let (input, bounds, from) = match level {
                1 => (source.document.clone(), extent, [x0, y0]),
                _ => (
                    downsamples.last().map_or_else(|| source.levels[level as usize - 2].clone(), |d| d.output.clone()),
                    extent.map(|v| v.div_ceil(scale / 2)),
                    origin.map(|v| v * 2),
                ),
            };
            downsamples.push(Downsample {
                input,
                bounds,
                origin: from,
                output: SelectionRefiner::coverage_buffer(&r.device, &mut encoder, origin, cells, false, "selection pyramid level"),
                extent: cells,
                row: 0,
            });
        }
        let levels: Vec<_> = downsamples.iter().map(|d| d.output.clone()).collect();
        let input = match plan.level {
            0 => source.document.clone(),
            level => levels.last().cloned().unwrap_or_else(|| source.levels[level as usize - 1].clone()),
        };
        let job = self.refiner.as_ref().unwrap().job(
            &r.device,
            &mut encoder,
            plan.grid,
            plan.active,
            plan.stages.clone(),
            input,
            None,
            downsamples,
        )?;
        let running = Running {
            job,
            request_id: request.request_id,
            probe: None,
            placement: plan.placement(),
            levels,
            underfill: None,
        };
        self.advance(r, encoder, running)?;
        Ok(true)
    }
    /// Encode and submit the next chunk of `running`, or its readback once
    /// the job is complete.
    fn advance(
        &mut self,
        r: &mut WgpuRasterizer,
        mut encoder: crate::submission::CommandEncoder,
        mut running: Running,
    ) -> Result<(), GpuRasterError> {
        let refiner = self.refiner.as_mut().unwrap();
        let timing = refiner.begin_timing(&mut encoder);
        let region = match refiner.advance(&r.device, &mut encoder, &mut running.job) {
            Ok(region) => region,
            Err(error) => {
                refiner.timed(&r.queue, timing);
                return Err(error);
            }
        };
        refiner.end_timing(&mut encoder, timing, running.job.taps());
        let Some(mut region) = region else {
            running.job.track(&encoder);
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            self.refiner.as_mut().unwrap().timed(&r.queue, timing);
            self.running = Some(running);
            return Ok(());
        };
        if let Some(source) = &mut self.modify_source {
            source.levels.extend(running.levels);
        }
        let extent = running.job.extent();
        region.underfill = running.underfill;
        self.finish(r, encoder, region, extent, true, running.request_id, running.probe, running.placement);
        self.refiner.as_mut().unwrap().timed(&r.queue, timing);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        r: &mut WgpuRasterizer,
        mut encoder: crate::submission::CommandEncoder,
        region: Region,
        extent: [u32; 2],
        byte_coverage: bool,
        request_id: u64,
        probe: Option<layer_render::TonalProbe>,
        placement: layer_core::Affine,
    ) {
        self.stop_mask=None;
        let band_bytes=u64::from(extent[0].div_ceil(8))*u64::from(extent[1])*4;
        let seal_metadata=region.gaps.as_ref().and_then(|gaps|gaps.seal_metadata.as_ref());
        let gaps=region.gaps.is_some();
        self.readback=None;
        let stale=self.cache_request.is_some();
        if stale {self.flood.copy_stale(&mut encoder,&region.coverage,region.bounds_offset+20);}
        let mask_size=region.bounds_offset+32;
        let band_size=if region.underfill.is_some() {band_bytes+32}else{0};
        let extra_size=if probe.is_some() {self.raw.tonal_statistics.size()}else{0};
        let metadata_bytes=seal_metadata.map_or(0,wgpu::Buffer::size);
        let seed_offset=self.fill_seed.as_ref().and_then(|(_,_,buffer)|buffer.as_ref()).map(|_|mask_size+band_size+metadata_bytes+extra_size);
        let size=mask_size+band_size+metadata_bytes+extra_size+if seed_offset.is_some(){16}else{0};
        let readback=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("region history snapshot"),size,
            usage:wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        encoder.copy_buffer_to_buffer(&region.coverage,0,&readback,0,mask_size);
        if let Some((buffer,offset))=&region.underfill {encoder.copy_buffer_to_buffer(buffer,offset-32,&readback,mask_size,band_size);}
        if let Some(metadata)=seal_metadata {encoder.copy_buffer_to_buffer(metadata,0,&readback,mask_size+band_size,metadata_bytes);}
        if extra_size>0 {encoder.copy_buffer_to_buffer(&self.raw.tonal_statistics,0,&readback,mask_size+band_size+metadata_bytes,extra_size);}
        if let (Some(offset),Some((_,_,Some(seed))))=(seed_offset,&self.fill_seed) {encoder.copy_buffer_to_buffer(seed,0,&readback,offset,16);}
        let capture=selection_readback::Capture {buffer:readback.clone(),size,seed_offset,coverage_size:region.bounds_offset,
            underfill_offset:(band_size>0).then_some(mask_size+32),seal_records:(metadata_bytes/20) as usize};
        self.readback=Some(readback);
        if gaps {encoder.clear_buffer(&region.coverage,24,Some(8));}
        r.uploads.finish(&encoder);
        r.metrics.command_passes += encoder.pass_count();
        encoder.submit(&r.queue);
        if let Some(refiner)=&mut self.refiner {refiner.timed(&r.queue,self.chunk_timing.take());}
        self.pending = Some(region);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        selection_readback::capture_selection(
            &r.device,
            capture,extent,byte_coverage,request_id,gaps,tx,
            move |mut result, source_stale, extra| {
                result.tonal_sample = probe.and_then(|p| tonal::sample(extra, p));
                result.placement = placement;
                (result,stale && source_stale)
            },
        );
    }
}
impl WgpuRasterizer {
    pub(super) fn prepare_region_cache(&mut self,preparation:Option<&layer_render::RegionPreparation>) {
        let invalid=preparation.is_some_and(|p|!p.is_valid());
        let preparation=preparation.filter(|_|!invalid);
        let mut regions=match self.regions.take() {
            Some(regions)=>regions,
            None if preparation.is_none()=>return,
            None=>RegionRequests::new(self),
        };
        if regions.areas.is_some() {self.keep_regions(regions);return;}
        if invalid && regions.pending.is_none() && regions.waiting.is_none() && regions.running.is_none() {
            regions.raw.cancel();regions.flood.cancel_lines();
            if regions.cache.as_ref().is_some_and(|cache|!cache.ready) {regions.cache=None;}
        }
        let previous=regions.preparation.take();
        regions.preparation=preparation.map(|p|Preparation {request:RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None,
            source:p.source.clone(),reference_count:p.reference_count,paper:p.paper.clone(),tolerance:p.tolerance,
            enclosure:None,
            position:[0;2],request_id:0,refinement:p.refinement,contiguous:true,limit:None,selection:None,
        },enclosed:p.enclosed,recipes_ready:previous.as_ref().is_some_and(|old|
            old.recipes_ready && old.enclosed==p.enclosed && old.request.refinement==p.refinement)});
        if let Some(preparation)=&regions.preparation {
            let request=cache::current(self,&preparation.request);let source=cache::source(self,&request);
            if regions.cache.as_ref().is_some_and(|cache|!cache.matches(self,&request,&source,preparation.enclosed)) {
                regions.cache.as_mut().unwrap().ready=false;
            }
        }
        self.keep_regions(regions);
    }
    pub(super) fn prefetch_regions(&mut self,frame:&Arc<artwork::Frame>,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError> {
        let Some(mut regions)=self.regions.take() else {return Ok(());};
        let old=self.artwork_frame.replace(frame.clone());
        let result=self.drain_image_decodes(encoder).and_then(|()|regions.prewarm(self,encoder));
        self.artwork_frame=old;self.keep_regions(regions);result
    }
    pub(super) fn region_chunks_submitted(&mut self) {
        if let Some(regions)=&mut self.regions && regions.flood.lines_submitted() && regions.cache.as_ref().is_none_or(|cache|cache.submitted()) && let Some(refiner)=&mut regions.refiner {refiner.timed(&self.queue,regions.chunk_timing.take());}
    }
    pub fn region_step_runnable(&self) -> bool {self.regions.as_ref().is_some_and(|regions|regions.areas.is_some())}
    pub(super) fn region_prewarm_runnable(&self) -> bool {self.regions.as_ref().is_some_and(|regions|regions.background_pending() && regions.prewarm_idle(self))}
    pub fn region_pending(&self) -> bool {
        self.regions
            .as_ref()
            .is_some_and(|r| r.pending.is_some() || r.waiting.is_some() || r.running.is_some() || r.areas.is_some())
    }
    /// Limit refinement chunks and previews to `taps`, or with None restore
    /// this GPU's budgets.
    #[cfg(test)]
    pub(super) fn set_refine_chunk(&mut self, taps: Option<f64>) {
        let mut regions = self.regions.take().unwrap_or_else(|| RegionRequests::new(self));
        regions.refiner(self).budget = taps;
        self.keep_regions(regions);
    }
    /// The pixel taps a refinement chunk takes on this GPU, once timed.
    #[cfg(test)]
    pub(super) fn refine_chunk(&self) -> Option<(f64, bool)> {
        self.regions.as_ref()?.refiner.as_ref().map(|r| (r.chunk(), r.times_chunks()))
    }
    pub(super) fn cancel_region(&mut self) {
        if let Some(mut regions) = self.regions.take() {
            regions.cancel();
            self.keep_regions(regions);
        }
    }
    fn keep_regions(&mut self, mut regions: RegionRequests) {
        let bytes = regions.storage_bytes();
        regions.charge.resize(bytes);
        self.regions = Some(regions);
    }
    pub(super) fn start_region(&mut self, task: RegionTask) -> Result<bool, GpuRasterError> {
        let mut regions = self
            .regions
            .take()
            .unwrap_or_else(|| RegionRequests::new(self));
        if regions.pending.is_some() || regions.waiting.is_some() || regions.running.is_some() || regions.areas.is_some() {self.keep_regions(regions);return Ok(false);}
        regions.sampling=region_sources::Sampling::Transparent;
        regions.cache_request=None;
        if let RegionTask::Fill {request,..}=&task {
            let refinement=RegionRequests::refinement(request);
            let prepare_ends=refinement.close_gaps && request.fill.as_ref().is_none_or(|fill|fill.reach!=Some(0));
            self.regions=Some(regions);
            let mut required=compile::Needs::default();
            required.regions(self,refinement,request.enclosure.is_some(),request.contiguous,prepare_ends);
            required.fill(self,self.artwork_frame.as_ref().map(|frame|frame.scene.view()));
            required.require(&self.compilation,startup::DOCUMENT);
            regions=self.regions.take().unwrap();
        }
        regions.fill_seed=None;
        let result = match task {
            RegionTask::Fill {request,target,position}=>{
                regions.fill_seed=Some((target,position,None));
                regions.start(self,request)
            },
            RegionTask::Mask(request)=>regions.start(self,request),
            RegionTask::FlatAreas(request)=>{regions.areas=Some(areas::AreasJob::new(request));Ok(true)},
        };
        if result.is_err() { regions.cancel(); }
        self.keep_regions(regions);
        result
    }
    pub(super) fn poll_region(&mut self) -> Option<Result<RegionOutput, GpuRasterError>> {
        if self.regions.as_ref()?.areas.is_some() {
            let mut regions=self.regions.take().unwrap();
            let mut job=regions.areas.take().unwrap();
            let result=job.advance(self,&mut regions);
            if matches!(result,Ok(None)) {regions.areas=Some(job);}
            if result.is_err() {regions.cancel();}
            self.keep_regions(regions);
            return result.transpose();
        }
        if self.regions.as_ref()?.waiting.is_some() {
            let mut requests = self.regions.take().unwrap();
            let request = requests.waiting.take().unwrap();
            let result = requests.start(self, request);
            if result.is_err() { requests.cancel(); }
            self.keep_regions(requests);
            if let Err(error) = result {
                return Some(Err(error));
            }
        }
        if let Some(refiner) = self.regions.as_mut()?.refiner.as_mut() {
            refiner.poll_timing();
        }
        if self.regions.as_ref()?.running.as_ref().is_some_and(|r| r.job.idle()) {
            let mut requests = self.regions.take().unwrap();
            let running = requests.running.take().unwrap();
            let encoder = crate::submission::CommandEncoder::new(
                &self.device,
                &wgpu::CommandEncoderDescriptor { label: Some("refine selection") },
            );
            let result = requests.advance(self, encoder, running);
            self.keep_regions(requests);
            if let Err(error) = result {
                return Some(Err(error));
            }
        }
        let requests = self.regions.as_mut()?;
        if requests.running.is_some() {
            return None;
        }
        requests.pending.as_ref()?;
        let incoming=requests.rx.as_ref()?.try_recv().ok()?;
        let region=requests.pending.take().unwrap();
        requests.readback=None;
        let result=match incoming {
            Ok((_,true))=>{
                if let Some(cache)=&mut requests.cache {cache.ready=false;cache.classified=false;}
                let request=requests.cache_request.take().unwrap();
                requests.stop_mask=None;requests.raw.cancel();requests.sampling=region_sources::Sampling::Retained;
                let mut requests=self.regions.take().unwrap();
                let rebuilt=requests.start(self,request);
                if rebuilt.is_err(){requests.cancel();}
                self.keep_regions(requests);
                return rebuilt.err().map(Err);
            },
            Ok((result,false))=>{requests.cache_request=None;Ok(result)},
            Err(error)=>{if let Some(cache)=&mut requests.cache {cache.ready=false;}requests.cache_request=None;Err(error)},
        };
        if let Ok(result)=&result {self.selection_clip.remember_pixels(&result.pixels,region.coverage.clone());}
        Some(result.map(RegionOutput::Mask))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn cache_ready(r:&WgpuRasterizer)->bool {r.regions.as_ref().unwrap().cache.as_ref().unwrap().ready}
    pub(crate) fn request_encoded(r:&WgpuRasterizer)->bool {r.regions.as_ref().is_some_and(|r|r.waiting.is_none()&&r.pending.is_some()&&r.readback.is_some()&&r.rx.is_some())}
    pub(crate) fn cached_pages(r:&WgpuRasterizer)->(wgpu::Buffer,Option<wgpu::Buffer>,std::collections::BTreeSet<[u32;2]>) {
        let cache=r.regions.as_ref().unwrap().cache.as_ref().unwrap();
        (cache.seeds.clone(),cache.mask.clone(),cache.dirty.values().flatten().copied().collect())
    }
    use crate::{layer_tests::placement, test_support};
    use layer_render::{CanvasRenderer, RegionPreparation, RegionRefinement, RegionSource, SelectionRefinement, TonalRequest};

    fn prepared_reference() -> (WgpuRasterizer, RegionSource) {
        let extent = [64, 64];
        let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Reference".into(),paper:"Paper".into()});
        placement::set_source(&mut document, layer_core::color::source::rgba8_source(extent, |x, y| {
            let ink = (12..52).contains(&x) && (12..52).contains(&y)
                && (x == 12 || x == 51 || y == 12 || y == 51);
            [0, 0, 0, if ink { 255 } else { 0 }]
        }));
        let source = RegionSource::Scene { snapshot: document.snapshot(), scope: SceneScope::Raw(placement::target(&document)) };
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        test_support::submit_ready(&mut r, test_support::packet(document.scene(), extent)).unwrap();
        r.set_refine_chunk(Some(1_048_576.));
        r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
        test_support::submit_settled(&mut r, test_support::packet(document.scene(), extent));
        r.wait_idle().unwrap();
        (r, source)
    }

    fn reference_query(source: RegionSource) -> RegionRequest {
        RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, source, reference_count: None, paper: None, enclosure: None,
            contiguous: true, selection: None, request_id: 91, position: [32, 32], tolerance: 0.,
            refinement: RegionRefinement { close_gaps: false, edges: layer_render::RegionEdges::Exact, smooth_edges: false }, limit: None }
    }

    #[test]
    fn region_cache_demand_build_uses_three_prewarm_chunks_per_call() {
        let mut counts=[0usize;2];
        for (demand,count) in [false,true].into_iter().zip(&mut counts) {
            let (mut r,source)=prepared_reference();
            r.set_refine_chunk(Some(65_536.));
            let request=reference_query(source);
            let mut regions=r.regions.take().unwrap();
            assert!(regions.cache.as_ref().unwrap().ready);
            regions.cache.as_mut().unwrap().ready=false;regions.flood.cancel_lines();
            for call in 1..=1024 {
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                let result=regions.build_cache(&mut r,&request,false,&mut encoder,demand,false);
                assert!(matches!(result,Ok(())|Err(GpuRasterError::NotReady)),"{result:?}");
                r.uploads.finish(&encoder);encoder.submit(&r.queue);test_support::complete(&r);
                if regions.cache.as_ref().unwrap().ready {*count=call;break;}
            }
            assert!(*count>0,"cache build did not finish");
            r.regions=Some(regions);
        }
        assert!(counts[1]<=counts[0].div_ceil(3)+2,"prewarm {}, demand {}",counts[0],counts[1]);
    }

    #[test]
    fn cold_reference_classification_prepares_source_decodes_by_batch() {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
        let extent=[PAGE_SIZE*8,PAGE_SIZE*4];let pages=32usize;let mut counts=Vec::new();
        for native in [false,true] {
            let image=layer_core::color::source::rgba8_source(extent,|x,y|[(x/PAGE_SIZE+1)as u8,(y/PAGE_SIZE+1)as u8,37,255]);
            assert_eq!(image.tiles.len(),pages);
            let mut artwork=layer_core::Artwork::new(extent).unwrap();
            let (owner,target)=test_support::add_paint(&mut artwork,"Cold reference",extent);
            let SourceTarget::Paint(handle)=target else {unreachable!()};
            if native {
                let (_,second)=test_support::add_paint(&mut artwork,"Fill target",extent);
                for (index,target) in [target,second].into_iter().enumerate() {
                    let SourceTarget::Paint(handle)=target else {unreachable!()};
                    let descriptor=layer_core::color::DocumentColor::default().paint_descriptor();
                    let tiles=image.tiles.keys().map(|&coordinate| {
                        let bytes=[coordinate[0]as u8+1,coordinate[1]as u8+1,137+index as u8,255].repeat((PAGE_SIZE*PAGE_SIZE)as usize);
                        (TileKey {plane:RasterPlane::Color,coordinate},RasterTile::backed(TileBlob::encode(descriptor,&bytes).unwrap()))
                    }).collect();
                    artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
                }
                assert!(artwork.paint.iter().all(|(_,_,paint)|paint.base.is_none()));
            }else {artwork.paint.get_mut(handle).unwrap().base=Some(layer_core::authored::PaintBase::new(image.clone().into()));}
            let document=layer_core::Document::from_artwork(artwork).unwrap();
            let mut r=crate::memory::memory_tests::renderer_for_restore(&document);
            r.ensure_document_metadata(extent,document.scene()).unwrap();
            r.artwork_frame=Some(Arc::new(crate::artwork::Frame::new(test_support::packet(document.scene(),extent),document.scene().snapshot().context)));
            if native {for target in document.scene().targets() {
                let data=document.scene().raster(target).unwrap().try_data().unwrap().unwrap();
                r.publish_cold_raster(target,&data).unwrap();
            }}
            assert!(r.paint_layers.iter().all(|layer|layer.pages.is_empty()));
            if native {
                assert_eq!(document.scene().targets().count(),2);
                for target in document.scene().targets() {for &coordinate in image.tiles.keys() {
                    let blob=r.native_color_tile(target,coordinate).unwrap().expect("cold native colour tile");
                    let space=r.document_color().space;
                    assert!(r.source_tiles.borrow().prepared_raster_any_view(&blob,space,space).is_none());
                }}
            }else {assert!(image.tiles.keys().all(|&coordinate|r.source_tiles.borrow().prepared_view(&image,coordinate).is_none()));}
            assert!(r.moving_decode.is_none());
            let before=r.device.source_samples.stats();
            let source=RegionSource::reference(document.snapshot(),owner).unwrap();
            let request=reference_query(source);r.set_refine_chunk(Some(1.));
            let mut regions=r.regions.take().unwrap();
            pollster::block_on(crate::compile::Needs::capture_scene(&r,document.scene(),false).prepare(&r.compilation)).unwrap();
            pollster::block_on(r.compilation.prepare_all(regions.raw.pipelines())).unwrap();
            pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
            let mut calls=0;let mut defers=0;
            for call in 1..=pages+4 {
                test_support::settle_decodes(&mut r);
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                r.drain_image_decodes(&mut encoder).unwrap();
                let result=regions.build_cache(&mut r,&request,false,&mut encoder,true,false);
                assert!(matches!(result,Ok(())|Err(GpuRasterError::NotReady)),"{result:?}");
                r.uploads.finish(&encoder);encoder.submit(&r.queue);test_support::complete(&r);
                if regions.cache.as_ref().unwrap().classified {calls=call;break;}
                defers+=1;
            }
            let decode_misses=r.device.source_samples.stats().misses-before.misses;
            assert!(calls>0,"classification did not finish: native={native}, pages={pages}, defers={defers}, decode_misses={decode_misses}");
            assert!(!regions.cache.as_ref().unwrap().ready);
            assert!(decode_misses>=pages as u64,"native={native}, cold pages={pages}, decode_misses={decode_misses}");
            r.regions=Some(regions);counts.push((native,calls,defers,decode_misses));
        }
        assert!(counts.iter().all(|(_,calls,_,_)|*calls<=pages.div_ceil(16)+1),"native, classification calls, defers, decode misses: {counts:?}");
    }

    #[test]
    fn reference_dirty_pages_keep_the_cache_with_unchanged_paper() {
        let (mut r,source)=prepared_reference();
        let RegionSource::Scene {snapshot,scope:SceneScope::Raw(target)}=source else {unreachable!()};
        assert!(snapshot.view().order().iter().any(|&id|layer_core::paper::is_paper_layer(snapshot.view(),id)));
        let mask=r.regions.as_ref().unwrap().cache.as_ref().unwrap().mask.clone();
        let mut next=snapshot.clone();
        let next_mut=std::sync::Arc::make_mut(&mut next);
        let SourceTarget::Paint(paint)=target else {unreachable!()};
        next_mut.artwork.paint.get_mut(paint).unwrap().raster=layer_core::raster::RasterRevision::pending();
        next_mut.revision+=1;
        r.prepare_regions(Some(&layer_render::RegionPreparation {
            source:RegionSource::Scene {snapshot:next.clone(),scope:SceneScope::Raw(target)},
            reference_count:None,paper:None,tolerance:0.,refinement:Default::default(),enclosed:false,
        }));
        let regions=r.regions.as_mut().unwrap();
        regions.changed(test_support::packet(next.view(),[64;2]),&[(target,PixelRect::new(16,16,32,32))]);
        let cache=regions.cache.as_ref().expect("unchanged Paper must not discard reference analysis");
        assert_eq!(cache.mask,mask);assert!(!cache.ready);
        assert_eq!(cache.dirty[&target],std::collections::BTreeSet::from([[0;2]]));
    }

    #[test]
    fn background_cache_prepares_query_recipes_before_becoming_idle() {
        let (mut r,source)=prepared_reference();
        let RegionSource::Scene {snapshot,..}=&source else {unreachable!()};
        r.regions=None;
        r.startup=Some(startup::Startup::new(r.compilation.clone()).unwrap());
        r.shader_idle(true,true);
        r.prepare_regions(Some(&RegionPreparation {source:source.clone(),reference_count:None,paper:None,tolerance:0.,refinement:Default::default(),enclosed:false}));
        test_support::submit_settled(&mut r,test_support::packet(snapshot.view(),[64;2]));
        assert!(r.regions.as_ref().unwrap().cache.as_ref().unwrap().ready);
        assert!(r.regions.as_ref().unwrap().flood.prepare(&r.startup.as_ref().unwrap().compiler,Default::default(),false,true,startup::OTHER));
        assert_eq!(r.startup.as_ref().unwrap().compiler.pending(),0);
        let result=test_support::receive_request(&mut r,reference_query(source));
        check_reference(&result);
    }

    fn hold_reference(r: &mut WgpuRasterizer) -> crate::submission::CommandEncoder {
        r.set_refine_chunk(Some(64.));
        let mut regions = r.regions.take().unwrap();
        assert!(regions.cache.as_ref().unwrap().ready);
        regions.cache.as_mut().unwrap().ready = false;
        regions.flood.cancel_lines();
        let mut held = crate::submission::CommandEncoder::new(&r.device, &wgpu::CommandEncoderDescriptor { label: Some("held region chunk") });
        regions.prewarm(r, &mut held).unwrap();
        assert!(held.has_work());
        assert!(regions.background_pending());
        r.regions = Some(regions);
        held
    }

    fn check_reference(result: &RegionResult) {
        assert_eq!((result.request_id, result.placement, result.pixels.extent(), result.pixels.bounds()),
            (91, layer_core::Affine::IDENTITY, [64, 64], [13, 13, 51, 51]));
        assert_eq!(result.pixels.pixels_per_word(), 8);
        for y in 0..64 { for x in 0..64 {
            let actual = (result.pixels.words()[(y * 8 + x / 8) as usize] >> (x % 8 * 4)) & 15;
            assert_eq!(actual, if (13..51).contains(&x) && (13..51).contains(&y) { 4 } else { 0 }, "{x},{y}");
        }}
    }

    #[test]
    fn held_cache_publication_waits_for_its_owner_and_rebuilds_after_discard() {
        let (mut r,source)=prepared_reference();r.set_refine_chunk(Some(1e12));
        {let regions=r.regions.as_mut().unwrap();regions.cache.as_mut().unwrap().ready=false;regions.flood.cancel_lines();}
        let mut held=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        test_support::query_ready(&mut r,&mut held,|r,encoder| {
            r.region_chunks_submitted();
            let mut regions=r.regions.take().unwrap();
            let result=regions.prewarm(r,encoder);
            let ready=regions.cache.as_ref().unwrap().ready;r.regions=Some(regions);result?;
            if ready {Ok(())}else{Err(GpuRasterError::NotReady)}
        });
        assert!(r.regions.as_ref().unwrap().cache.as_ref().unwrap().pending());
        let timing=r.regions.as_ref().unwrap().chunk_timing;
        let unrelated=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        unrelated.submit(&r.queue);r.region_chunks_submitted();
        assert_eq!(r.regions.as_ref().unwrap().chunk_timing,timing);
        assert!(r.regions.as_ref().unwrap().cache.as_ref().unwrap().pending());
        assert!(r.request_region(reference_query(source).into()).unwrap());
        assert!(r.regions.as_ref().unwrap().waiting.is_some());assert!(r.take_region().is_none());
        drop(held);
        let result=test_support::prepare_raster(&mut r,|r|r.take_region().unwrap_or(Err(GpuRasterError::NotReady))).unwrap();
        let RegionOutput::Mask(result)=result else {panic!("expected region mask")};
        check_reference(&result);
    }
    #[test]
    fn busy_region_chunks_keep_their_encoder_and_timing_until_submission() {
        let (mut r, source) = prepared_reference();
        let held = hold_reference(&mut r);
        let mut regions = r.regions.take().unwrap();
        let timing = regions.chunk_timing;
        let mut idle = crate::submission::CommandEncoder::new(&r.device, &wgpu::CommandEncoderDescriptor { label: Some("busy region poll") });
        regions.prewarm(&mut r, &mut idle).unwrap();
        assert!(!idle.has_work());
        assert_eq!(regions.chunk_timing, timing);
        r.regions = Some(regions);
        let unrelated=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        unrelated.submit(&r.queue);r.region_chunks_submitted();
        assert!(!r.regions.as_mut().unwrap().flood.lines_ready(true).unwrap());
        assert!(r.request_region(reference_query(source).into()).unwrap());
        assert!(r.region_pending());
        assert!(r.regions.as_ref().unwrap().waiting.is_some());
        assert_eq!(r.regions.as_ref().unwrap().chunk_timing, timing);
        assert!(r.take_region().is_none());
        assert!(r.region_pending());
        assert_eq!(r.regions.as_ref().unwrap().chunk_timing, timing);
        held.submit(&r.queue);
        r.region_chunks_submitted();
        r.set_refine_chunk(Some(1_048_576.));
        let result = test_support::prepare_raster(&mut r, |r| r.take_region().unwrap_or(Err(GpuRasterError::NotReady))).unwrap();
        let RegionOutput::Mask(result)=result else {panic!("expected region mask")};
        check_reference(&result);
        assert!(!r.region_pending());
        r.prepare_regions(None);
    }

    #[test]
    fn tonal_interruption_keeps_partial_reference_input() {
        for partial in [false, true] {
            let (mut r, source) = prepared_reference();
            if partial {
                hold_reference(&mut r).submit(&r.queue);
                r.region_chunks_submitted();
                r.wait_idle().unwrap();
            }
            r.set_refine_chunk(Some(1_048_576.));
            let mut tonal = reference_query(RegionSource::Tonal(Box::new(TonalRequest { source: source.clone(), bands: vec![], invert: false, probe: None })));
            tonal.contiguous = false;
            tonal.refinement = Default::default();
            tonal.selection = Some(SelectionRefinement { resize: 0, mode: layer_core::SelectionMode::New, antialias: true,
                feather: 0., previous: None, source_to_document: layer_core::Affine::IDENTITY, keep_canvas_edges: false });
            test_support::receive_request(&mut r, tonal);
            check_reference(&test_support::receive_request(&mut r, reference_query(source)));
            r.prepare_regions(None);
        }
    }
}
