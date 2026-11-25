use super::*;
use layer_core::raster::{RasterData,RasterRevision,RasterTile,TileKey,RasterPlane};
use crate::frame_demand::{Allowance,Outstanding,Producer,UnitWait};

pub(super) struct AreasJob {
    request:RegionRequest,
    areas:Option<flood::Areas>,
    phase:Phase,
    cores:Vec<[u32;4]>,
    capture:Option<raster::NativeCapture>,
    data:RasterData,
    submission:Option<crate::submission::Submission>,
    wait:UnitWait,
}
enum Phase {Cache,Label(usize),Join,Replay {at:usize,next:usize},Pages {at:usize,next:u32},Finish}
impl AreasJob {
    pub fn new(request:RegionRequest)->Self {
        Self {request,areas:None,phase:Phase::Cache,cores:Vec::new(),capture:None,data:RasterData::default(),submission:None,wait:UnitWait::default()}
    }
    pub fn storage_bytes(&self)->u64 {
        self.data.tiles.values().map(|tile|tile.descriptor().byte_len([PAGE_SIZE;2]).unwrap_or(0) as u64).sum()
    }
    pub fn demand(&self,r:&WgpuRasterizer)->Option<Outstanding> {
        match self.submission.clone().and_then(|piece|UnitWait::input(Producer::Gpu(piece)).demand(r)) {
            Some(Outstanding::Blocked(input))=>Some(Outstanding::Blocked(input)),
            _=>self.wait.demand(r),
        }
    }
    pub fn awaits_result(&self)->bool {
        let in_flight=|piece:&crate::submission::Submission|piece.submitted() && piece.pending();
        self.submission.as_ref().is_some_and(in_flight) || matches!(&self.wait,UnitWait::Input(Producer::Gpu(piece),_) if in_flight(piece))
    }
    pub fn advance(&mut self,r:&mut WgpuRasterizer,regions:&mut RegionRequests)->Result<Option<RegionOutput>,GpuRasterError> {
        if self.submission.as_ref().is_some_and(|owner|owner.discarded()) {return Err(GpuRasterError::Invariant("prepared advance state is inconsistent"));}
        if let Some(error)=self.wait.ended() {return Err(error);}
        if self.submission.as_ref().is_some_and(crate::submission::Submission::pending) {return Ok(None);}
        let Some(allowance)=self.wait.eligible(r) else {return Ok(None);};
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&wgpu::CommandEncoderDescriptor {label:Some("flat areas")});
        regions.awaiting=None;
        let result=self.encode(r,regions,&mut encoder,allowance);
        r.uploads.finish(&encoder);
        r.metrics.command_passes+=encoder.pass_count();
        let worked=encoder.has_work();
        self.wait=match &result {
            Err(error@GpuRasterError::Waiting(_))=>Self::stopped(regions,error,allowance),
            Err(error@(GpuRasterError::NotReady|GpuRasterError::NotPublished))=>Self::stopped(regions,error,if worked {Allowance::Fresh}else {allowance}),
            _=>UnitWait::default(),
        };
        if worked {
            self.submission=Some(encoder.submission());
            encoder.submit(&r.queue);
        }
        match result {Err(GpuRasterError::NotReady|GpuRasterError::Waiting(_)|GpuRasterError::NotPublished)=>Ok(None),other=>other}
    }
    fn stopped(regions:&mut RegionRequests,error:&GpuRasterError,allowance:Allowance)->UnitWait {
        let awaiting=regions.awaiting.take();
        match (error,regions.compiling.take()) {
            (GpuRasterError::Waiting(waiting),_)=>match UnitWait::refused(waiting.need().clone(),allowance) {UnitWait::End(None)=>UnitWait::End(Some(error.clone())),wait=>wait},
            (_,Some(needs))=>UnitWait::Input(Producer::Compile(Arc::new(needs)),allowance),
            _=>awaiting.map_or(UnitWait::Ready(allowance),|producer|UnitWait::Input(producer,allowance)),
        }
    }
    fn encode(&mut self,r:&mut WgpuRasterizer,regions:&mut RegionRequests,encoder:&mut crate::submission::CommandEncoder,allowance:Allowance)->Result<Option<RegionOutput>,GpuRasterError> {
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
            if let Some(write)=regions.cache.as_ref().filter(|cache|cache.pending()).and_then(|cache|cache.lastwrite.clone()) {
                regions.awaiting=Some(Producer::Gpu(write));
                return Err(GpuRasterError::NotReady);
            }
            {
                let native=r.native_edit.as_ref().unwrap();
                let ready=regions.raw.prepare(&r.compilation,startup::BRUSH)
                    & r.compilation.require(native.required_pipelines(r.document_color.depth,false),startup::BRUSH);
                if !regions.flood.prepare_areas(&r.compilation,startup::BRUSH) || !ready {
                    regions.compiling=Some(RegionRequests::recipes(regions.raw.pipelines().chain(native.required_pipelines(r.document_color.depth,false)).chain(regions.flood.area_pipelines())));
                    return Err(GpuRasterError::NotReady);
                }
            }
            r.drain_image_decodes(encoder)?;
            regions.build_cache(r,&self.request,false,encoder,None,true,(crate::residency::Class::Visible,allowance))?;
            let mask=regions.raw.mask().ok_or(GpuRasterError::InvalidExtent)?;
            let (paper,grain,radius)=regions.awaited_paper(r,&self.request)?;
            let reach=self.request.fill.as_ref().and_then(|fill|fill.reach).unwrap_or(16);
            let reach=(self.request.refinement.close_gaps && reach>0).then_some(reach);
            if !regions.scoped_lines(&flood::Input {device:&r.device,mask:&mask,paper:&paper},encoder,(grain,radius),None)? {return Err(regions.await_lines());}
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
        let (paper,_,_)=regions.awaited_paper(r,&self.request)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flat_areas_tests::{border,frame,receive,source};
    use layer_render::{RegionEdges,RegionRefinement,RegionSource,RegionTask};
    #[test]
    fn a_flat_areas_request_behind_an_unsubmitted_cache_write_sleeps_on_that_write_and_awaits_no_result() {
        let extent=[129,97];
        let document=source(extent,|x,y|border([12,12,117,85],x,y));
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
        drop(receive(&mut r,&document));
        let mut write=crate::submission::CommandEncoder::new(&r.device,&wgpu::CommandEncoderDescriptor {label:Some("held region cache write")});
        r.regions.as_mut().unwrap().cache.as_mut().unwrap().lastwrite=Some(write.submission());
        let request=RegionRequest {request_id:85,source:RegionSource::Source(crate::layer_tests::placement::target(&document)),position:[0,0],tolerance:0.1,
            contiguous:false,selection:None,limit:None,enclosure:None,reference_count:None,paper:None,fill:None,fill_areas:Some(layer_core::FillAreas::TransparentOrWhite),
            colour_mode:Default::default(),refinement:RegionRefinement {close_gaps:false,edges:RegionEdges::LineMiddle,smooth_edges:false}};
        assert!(r.request_region(RegionTask::FlatAreas(request)).unwrap());
        for _ in 0..3 {assert!(r.take_region().is_none(),"no flat areas result lands before the cache write it reads");}
        assert!(!r.region_step_runnable(),"a flat areas job behind an unsubmitted cache write asks for a frame that cannot advance it");
        assert!(!r.region_results_awaited(),"a flat areas job with nothing in flight is counted as a GPU result a completion delivers");
        drop(write);
        let RegionOutput::FlatAreas {request_id,..}=crate::test_support::wait_until("flat areas readback timed out",||r.take_region().ok_or(())).unwrap() else {panic!("expected flat areas raster")};
        assert_eq!(request_id,85);
    }
}
