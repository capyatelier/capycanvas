use super::*;
use layer_core::raster::{RasterData,RasterRevision,RasterTile,TileKey,RasterPlane};
use std::sync::atomic::{AtomicU32,Ordering};

pub(super) struct AreasJob {
    request:RegionRequest,
    areas:Option<flood::Areas>,
    phase:Phase,
    cores:Vec<[u32;4]>,
    capture:Option<raster::NativeCapture>,
    data:RasterData,
    submission:Option<crate::submission::Submission>,
    in_flight:Arc<AtomicU32>,
}
enum Phase {Cache,Label(usize),Join,Replay {at:usize,next:usize},Pages {at:usize,next:u32},Finish}
impl AreasJob {
    pub fn new(request:RegionRequest)->Self {
        Self {request,areas:None,phase:Phase::Cache,cores:Vec::new(),capture:None,data:RasterData::default(),submission:None,in_flight:Arc::new(AtomicU32::new(0))}
    }
    pub fn storage_bytes(&self)->u64 {
        self.data.tiles.values().map(|tile|tile.descriptor().byte_len([PAGE_SIZE;2]).unwrap_or(0) as u64).sum()
    }
    pub fn advance(&mut self,r:&mut WgpuRasterizer,regions:&mut RegionRequests)->Result<Option<RegionOutput>,GpuRasterError> {
        if self.submission.as_ref().is_some_and(|owner|owner.discarded()) {return Err(GpuRasterError::Invariant("prepared advance state is inconsistent"));}
        if self.submission.as_ref().is_some_and(|owner|!owner.submitted()) || self.in_flight.load(Ordering::Acquire)>0 {return Ok(None);}
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&wgpu::CommandEncoderDescriptor {label:Some("flat areas")});
        let result=self.encode(r,regions,&mut encoder);
        r.uploads.finish(&encoder);
        r.metrics.command_passes+=encoder.pass_count();
        if !encoder.has_work() {return match result {Err(GpuRasterError::NotReady)=>Ok(None),other=>other};}
        self.submission=Some(encoder.submission());
        self.in_flight.fetch_add(1,Ordering::AcqRel);
        let done=crate::InFlightCompletion(self.in_flight.clone());
        encoder.hold(done);
        encoder.submit(&r.queue);
        if let Some(refiner)=&mut regions.refiner {refiner.timed(&r.queue,regions.chunk_timing.take());}
        match result {Err(GpuRasterError::NotReady)=>Ok(None),other=>other}
    }
    fn encode(&mut self,r:&mut WgpuRasterizer,regions:&mut RegionRequests,encoder:&mut crate::submission::CommandEncoder)->Result<Option<RegionOutput>,GpuRasterError> {
        r.compilation.check()?;
        if matches!(self.phase,Phase::Cache) {
            if r.native_edit.is_none() || !self.request.source.valid_parameters(self.request.reference_count,self.request.tolerance)
                || self.request.source.sources().iter().any(|source|!matches!(source,layer_render::RegionSource::Composite|layer_render::RegionSource::Source(_)|layer_render::RegionSource::Objects(_)|layer_render::RegionSource::Scene {..}))
                || self.request.fill_areas!=Some(layer_core::FillAreas::TransparentOrWhite) || self.request.enclosure.is_some()
                || self.request.selection.is_some() || self.request.limit.is_some()
                || self.request.fill.as_ref().is_some_and(|fill|fill.trail.is_some() || !fill.excluded_seals.is_empty() || fill.reach.is_some_and(|reach|!layer_render::RegionGapMasks::STOPS.contains(&reach))) {return Err(GpuRasterError::InvalidExtent);}
            if !flood::fits(r.document_extent,&r.device.limits()) {return Err(GpuRasterError::SizeOverflow);}
            r.prepare_source_backing()?;
            let pages=r.document_extent.map(|n|u64::from(n.div_ceil(PAGE_SIZE)));
            let bytes=r.document_color().paint_descriptor().byte_len([PAGE_SIZE;2]).ok_or(GpuRasterError::SizeOverflow)? as u64;
            if pages[0].checked_mul(pages[1]).and_then(|count|count.checked_mul(bytes)).is_none_or(|bytes|bytes>layer_core::raster::MAX_PUBLICATION_BYTES) {return Err(GpuRasterError::SizeOverflow);}
            if let Some(binding)=&self.request.paper {r.prepare_paper_binding(binding,startup::BRUSH)?;}
            if regions.cache.as_ref().is_some_and(|cache|cache.pending()) {return Err(GpuRasterError::NotReady);}
            {
                let ready=regions.raw.prepare(&r.compilation,startup::BRUSH)
                    & r.compilation.require(r.native_edit.as_ref().unwrap().required_pipelines(r.document_color.depth,false),startup::BRUSH);
                if !regions.flood.prepare_areas(&r.compilation,startup::BRUSH) || !ready {return Err(GpuRasterError::NotReady);}
            }
            r.drain_image_decodes(encoder)?;
            regions.refiner(r).poll_timing();
            regions.raw.batch_limit(Some(16));
            regions.build_cache(r,&self.request,false,encoder,true,true)?;
            let mask=regions.raw.mask().ok_or(GpuRasterError::InvalidExtent)?;
            let (paper,_,_)=RegionRequests::paper(r,&self.request)?;
            let reach=self.request.fill.as_ref().and_then(|fill|fill.reach).unwrap_or(16);
            let reach=(self.request.refinement.close_gaps && reach>0).then_some(reach);
            let areas=regions.flood.begin_areas(&flood::Input {device:&r.device,mask:&mask,paper:&paper},encoder,r.document_extent,reach)?;
            let side=areas.windows[0].1[2].max(areas.windows[0].1[3]);
            if side<PAGE_SIZE && areas.extent.iter().any(|&n|n>side) {return Err(GpuRasterError::SizeOverflow);}
            let side=(side/PAGE_SIZE).max(1)*PAGE_SIZE;
            let [w,h]=areas.extent;
            self.cores=(0..h.div_ceil(side)).flat_map(|y|(0..w.div_ceil(side)).map(move|x|[x*side,y*side,side.min(w-x*side),side.min(h-y*side)])).collect();
            self.areas=Some(areas);self.phase=Phase::Label(0);
            return Ok(None);
        }
        if matches!(self.phase,Phase::Finish) {
            r.submit_private_tiles(self.capture.take().unwrap())?;
            return Ok(Some(RegionOutput::FlatAreas {request_id:self.request.request_id,raster:RasterRevision::backed(std::mem::take(&mut self.data))}));
        }
        let areas=self.areas.as_ref().unwrap();
        let mask=regions.raw.mask().ok_or(GpuRasterError::InvalidExtent)?;
        let (paper,_,_)=RegionRequests::paper(r,&self.request)?;
        let input=flood::Input {device:&r.device,mask:&mask,paper:&paper};
        match self.phase {
            Phase::Label(at)=>{
                regions.flood.label_area_core(&input,encoder,areas,at);
                self.phase=if at+1==areas.windows.len(){Phase::Join}else{Phase::Label(at+1)};
            },
            Phase::Join=>{regions.flood.join_areas(&input,encoder,areas);self.phase=Phase::Replay {at:0,next:0};},
            Phase::Replay {at,next}=>{
                let core=self.cores[at];let window=Flood::area_window(core,areas.extent);
                if next==0 {encoder.clear_buffer(regions.flood.area_tokens(),0,None);}
                let following=(next..areas.windows.len()).find(|&index|intersects(areas.windows[index].1,window));
                if let Some(index)=following {
                    regions.flood.replay_area_core(&input,encoder,areas,index,window,regions.flood.area_tokens());
                    self.phase=Phase::Replay {at,next:index+1};
                } else {
                    regions.flood.assign_area_tokens(&input,encoder,core,window);
                    self.phase=Phase::Pages {at,next:0};
                }
            },
            Phase::Pages {at,next}=>{
                let core=self.cores[at];let window=Flood::area_window(core,areas.extent);
                let columns=core[2].div_ceil(PAGE_SIZE);let count=columns*core[3].div_ceil(PAGE_SIZE);
                let end=(next+16).min(count);
                let mut inputs=Vec::new();
                for index in next..end {
                    let page=[core[0]+index%columns*PAGE_SIZE,core[1]+index/columns*PAGE_SIZE];
                    let texture=r.device.create_texture(&wgpu::TextureDescriptor {label:Some("flat area page"),size:wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba32Float,usage:wgpu::TextureUsages::STORAGE_BINDING|wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
                    regions.flood.colour_area_tokens(&r.device,encoder,core,window,page,&texture.create_view(&Default::default()));
                    let tile=RasterTile::pending(r.document_color().paint_descriptor());
                    self.data.tiles.insert(TileKey {plane:RasterPlane::Color,coordinate:page.map(|n|n/PAGE_SIZE)},tile.clone());
                    inputs.push((texture,tile,layer_core::color::LayerColorMode::FullColor));
                }
                let capture=r.encode_private_tiles(inputs,encoder)?;
                if let Some(existing)=&mut self.capture {existing.append(capture);}else{self.capture=Some(capture);}
                self.phase=if end<count {Phase::Pages {at,next:end}}else if at+1<self.cores.len(){Phase::Replay {at:at+1,next:0}}else{Phase::Finish};
            },
            Phase::Cache|Phase::Finish=>unreachable!(),
        }
        Ok(None)
    }
}
fn intersects(a:[u32;4],b:[u32;4])->bool {a[0]<b[0]+b[2] && b[0]<a[0]+a[2] && a[1]<b[1]+b[3] && b[1]<a[1]+a[3]}
