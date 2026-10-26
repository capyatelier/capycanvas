use super::*;

#[derive(Clone,PartialEq)]
struct Key {
    target:SourceTarget,index:u32,snapshot:usize,scope:layer_core::SceneScope,offset:layer_core::Point,
    damage:PixelRect,extent:[u32;2],coverage:layer_core::CoverageSnapshot,low:Option<SourceTarget>,
}
struct Input {key:Key,snapshot:Arc<layer_core::SceneSnapshot>}
pub(crate) struct BakePreflight {
    color:layer_core::color::DocumentColor,owner:u64,revision:u64,inputs:Vec<Input>,next:usize,
    renderer:Option<Box<WgpuRasterizer>>,pipelines:Option<startup::Requirements>,scene:Option<Box<Scene>>,source:Option<Arc<layer_core::SceneSnapshot>>,initialized:bool,windowed:bool,
    steps:Vec<([u32;2],PixelRect,DocRect)>,retiring:Option<Arc<std::sync::atomic::AtomicBool>>,coordinate:usize,output:Option<Image>,output_recorded:bool,planning:bool,
    pages:Vec<(usize,[u32;2],Image)>,failure:Option<GpuRasterError>,projected_bytes:u64,
}
impl BakePreflight {
    pub fn matches(&self,packet:FramePacket<'_>,color:layer_core::color::DocumentColor)->bool {self.color==color && self.owner==packet.scene.owner() && self.revision==packet.scene.revision()}
    fn complete(&self)->bool {self.failure.is_some() || self.next==self.inputs.len()}
    pub fn pending(&self)->bool {!self.complete()}
    pub fn projected_bytes(&self)->u64 {self.projected_bytes}
    pub fn waiting(&self)->bool {self.renderer.as_ref().is_some_and(|r|r.image_decode_waiting())}
    fn advance(&mut self,parent:&mut WgpuRasterizer,packet:FramePacket<'_>)->Result<(),GpuRasterError> {
        if self.complete() {return Ok(());}
        let input=&self.inputs[self.next];
        if self.source.is_none() {
            let required=crate::snapshot::capture_targets(input.snapshot.view().with_scope(&input.key.scope),&input.key.scope);
            for target in &required {
                if let Some(root)=input.snapshot.view().raster(*target) {
                    match root.try_data() {Some(Ok(_))=>(),Some(Err(error))=>return Err(GpuRasterError::Effect(error)),None=>return Ok(()),}
                }
            }
            let mut source=(*input.snapshot).clone();
            source.artwork=input.snapshot.scoped_transfer_artwork(&required);
            let paint:Vec<_>=source.artwork.paint.iter().map(|(handle,_,_)|handle).collect();
            for handle in paint {source.artwork.paint.get_mut(handle).unwrap().operations=Arc::default();}
            let coverage:Vec<_>=source.artwork.coverage.iter().map(|(handle,_,_)|handle).collect();
            for handle in coverage {source.artwork.coverage.get_mut(handle).unwrap().operations=Arc::default();}
            self.source=Some(Arc::new(source));
        }
        let scene=self.source.as_ref().unwrap().view().with_scope(&input.key.scope).with_offset(input.key.offset);
        let source=FramePacket {scene,document_extent:input.key.extent,dabs:&[],dab_batches:&[],restore_rasters:&[],
            inspect_mask:None,selection_overlays:None,reset_layers:!self.initialized,commit_rasters:true,composite_all:true,
            view:packet.view,blend_space:scene.composition().blend,time_seconds:input.snapshot.context.elapsed};
        if self.renderer.is_none() {
            for target in scene.targets() {
                if let Some(root)=scene.raster(target) {
                    match root.try_data() {Some(Ok(_))=>(),Some(Err(error))=>return Err(GpuRasterError::Effect(error)),None=>return Ok(()),}
                }
            }
            let retained=self.projected_bytes.checked_add(parent.projected_restore_bytes()).and_then(|bytes|bytes.checked_add(parent.scale_display.as_ref().map_or(0,|cache|cache.resident_bytes())))
                .ok_or(GpuRasterError::SourceWorkingSetExceeded)?;
            let budget=parent.native_edit.as_ref().map_or(windows::DEFAULT_IMAGE_PIXEL_BYTES,|native|native.image_pixel_budget(retained)).min(windows::DEFAULT_IMAGE_PIXEL_BYTES);
            let plan=windows::Plan::new(scene,input.key.extent,budget,parent.device.limits().max_texture_dimension_2d)?;
            let damage=input.key.damage;
            let outputs:Vec<_>=plan.map_or_else(||vec![damage],|plan|plan.outputs(DocRect::from(damage).aligned(PAGE_SIZE).in_frame(input.key.extent)).map(|output|output.intersect(damage)).collect());
            self.windowed=plan.is_some();
            self.steps=outputs.into_iter().flat_map(|output| {
                let window=Scene::capture_window(scene,output,input.key.extent);
                page_coordinates(output).map(move |coordinate|(coordinate,output,window))
            }).collect();
            let mut renderer=parent.native_preparation()?;
            renderer.effect_analyses=parent.bake_analysis_entries(&input.snapshot,&input.key.scope,input.key.offset,input.key.extent)?.unwrap_or_default();
            renderer.ensure_document_metadata(input.key.extent,scene)?;
            let watercolor=scene.targets().any(|target|scene.raster(target).and_then(|root|root.try_data()).is_some_and(|data|data.is_ok_and(|data|data.watercolor.is_some())));
            self.pipelines=Some(startup::Requirements::capture_scene(&renderer,scene,watercolor));
            self.renderer=Some(Box::new(renderer));
        }
        let r=self.renderer.as_mut().unwrap();
        if !self.initialized {
            if let Some(required)=&self.pipelines
                && let Some(compiler)=r.startup.as_ref().map(|startup|&startup.compiler).or(r.preparation_compiler.as_ref())
                && !required.require(compiler,startup::DOCUMENT) {return Ok(());}
            self.pipelines=None;
            if !r.prepare_raster_restores(source) {return Ok(());}
            r.reconcile_rasters(source,true,&[])?;
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            r.prepare_uploads(source,&mut [],&mut encoder)?;
            r.layer_masks.prepare(&r.device,&mut encoder,(scene,&[]),input.key.extent,false,&mut r.selection_clip)?;
            if encoder.has_work() {r.uploads.finish(&encoder);encoder.submit(&r.queue);}
            self.scene=Some(Scene::new(r)?);self.scene.as_mut().unwrap().ordered=true;self.initialized=true;
        }
        let Some(&(coordinate,output,window))=self.steps.get(self.coordinate) else {
            self.next+=1;self.renderer=None;self.scene=None;self.source=None;self.initialized=false;self.coordinate=0;self.planning=false;return Ok(());
        };
        if self.retiring.as_ref().is_some_and(|retired|!retired.load(std::sync::atomic::Ordering::Acquire)) {
            #[cfg(not(target_arch="wasm32"))]
            let _=r.device.poll(wgpu::PollType::Poll);
            return Ok(());
        }
        let compositor=self.scene.as_mut().unwrap();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let completed=self.pages.len();
        let result=(|| {
            r.drain_image_decodes(&mut encoder)?;
            if !compositor.jobs.is_empty() {
                if !compositor.encode_ordered_jobs(r,&mut encoder)? {return Ok(());}
                if self.output_recorded {
                    self.pages.push((self.next,coordinate,self.output.take().unwrap()));self.coordinate+=1;
                    self.output_recorded=false;self.planning=false;return Ok(());
                }
            }
            let mut objects=std::mem::take(&mut compositor.exact_object_results);
            let advanced=objects.advance(r,compositor,&mut encoder,false);
            compositor.exact_object_results=objects;advanced?;
            compositor.object_display=false;compositor.object_query=false;compositor.object_live=false;
            if !compositor.prepare_object_pages(r,source,None,page_coordinates(window.in_frame(input.key.extent)))? {return Ok(());}
            if !self.planning {r.source_tiles.borrow_mut().invalidate();compositor.begin_frame();self.planning=true;}
            let prepared=(|| {
                compositor.image_window=Some(window);compositor.image_output=Some(output);
                compositor.update_images(r,source,output,&mut encoder)?;
                compositor.jobs.clear();compositor.source_jobs.clear();compositor.used.fill(false);
                let tile=compositor.group(r,source,None,coordinate)?;
                let (texture,view)=create_color_target(&r.device,[PAGE_SIZE;2],"prepared bake page");
                let image=Image {texture,view,plan:display_mips::Plan::window(input.key.extent,0,page_rect(coordinate).intersect(PixelRect::full(input.key.extent)))};
                compositor.copy_window_tile(tile,&image,coordinate);self.output=Some(image);self.output_recorded=true;
                if compositor.encode_ordered_jobs(r,&mut encoder)? {
                    self.pages.push((self.next,coordinate,self.output.take().unwrap()));self.coordinate+=1;
                    self.output_recorded=false;self.planning=false;
                }
                Ok(())
            })();
            match prepared {Err(GpuRasterError::DeferredObjectWork)=>Ok(()),result=>result,}
        })();
        let finished=self.windowed && self.pages.len()>completed && self.steps.get(self.coordinate).is_none_or(|next|next.1!=output);
        if finished {
            parent.metrics.image_window_peak_bytes=parent.metrics.image_window_peak_bytes.max(compositor.images.storage_bytes());
            parent.metrics.image_window_submissions+=1;
        }
        if encoder.has_work() {r.uploads.finish(&encoder);r.last_submission=Some(encoder.submit(&r.queue));}
        if finished {
            let complete=Arc::new(std::sync::atomic::AtomicBool::new(false));let signal=complete.clone();
            r.queue.on_submitted_work_done(move||signal.store(true,std::sync::atomic::Ordering::Release));
            self.retiring=Some(complete);
        }
        result
    }
}
impl WgpuRasterizer {
    pub(crate) fn prepare_bakes(&mut self,packet:FramePacket<'_>)->bool {
        if self.snapshot_worker {return true;}
        let inputs:Vec<_>=packet.dab_batches.iter().filter_map(|batch| {
            let DabBatchKind::RasterOperation(index)=batch.kind else {return None;};
            let operation=packet.scene.operations(batch.target)?.get(index as usize)?;
            let (snapshot,scope,offset,low)=match &operation.kind {
                layer_core::RasterOperationKind::Bake {scene,scope,offset}=>(scene,scope,*offset,None),
                layer_core::RasterOperationKind::FrequencyDetail {scene,scope,offset,low}=>(scene,scope,*offset,Some(SourceTarget::Paint(*low))),_=>return None,
            };
            Some(Input {snapshot:snapshot.clone(),key:Key {target:batch.target,index,snapshot:Arc::as_ptr(snapshot) as usize,scope:scope.clone(),offset,low,
                damage:batch_pixel_rect(batch,packet.scene.target_extent(batch.target)),extent:packet.scene.target_extent(batch.target),coverage:operation.coverage.clone()}})
        }).collect();
        if inputs.is_empty() {self.bake_preflight=None;return true;}
        let projected=inputs.iter().try_fold(0u64,|bytes,input| {
            let damage=input.key.damage;
            let columns=u64::from(damage.max_x().div_ceil(PAGE_SIZE).saturating_sub(damage.min_x()/PAGE_SIZE));
            let rows=u64::from(damage.max_y().div_ceil(PAGE_SIZE).saturating_sub(damage.min_y()/PAGE_SIZE));
            columns.checked_mul(rows).and_then(|pages|pages.checked_mul(u64::from(PAGE_SIZE).pow(2)*u64::from(self.device.working_format().block_copy_size(None).unwrap())))
                .and_then(|added|bytes.checked_add(added)).ok_or(GpuRasterError::SourceWorkingSetExceeded)
        }).and_then(|bytes|self.admit_preparation_bytes(self.projected_restore_bytes(),bytes).map(|()|bytes));
        let mut state=self.bake_preflight.take().filter(|old|old.color==self.document_color() && old.inputs.iter().map(|input|&input.key).eq(inputs.iter().map(|input|&input.key)))
            .unwrap_or(BakePreflight {color:self.document_color(),owner:packet.scene.owner(),revision:packet.scene.revision(),inputs,next:0,renderer:None,pipelines:None,scene:None,source:None,initialized:false,windowed:false,steps:Vec::new(),retiring:None,coordinate:0,
                output:None,output_recorded:false,planning:false,pages:Vec::new(),failure:None,projected_bytes:0});
        match projected {Ok(bytes)=>state.projected_bytes=bytes,Err(error)=>state.failure=Some(error)}
        if let Err(error)=state.advance(self,packet) {state.failure=Some(error);}
        let complete=state.complete();self.bake_preflight=Some(state);complete
    }
    pub(crate) fn take_bake_failure(&mut self)->Option<GpuRasterError> {self.bake_preflight.as_mut().and_then(|state|state.failure.take())}
    pub(crate) fn prepared_bake(&self,target:SourceTarget,index:u32,coordinate:[u32;2])->Option<wgpu::TextureView> {
        let state=self.bake_preflight.as_ref()?;
        let input=state.inputs.iter().position(|input|input.key.target==target && input.key.index==index)?;
        state.pages.iter().find(|(id,c,_)|*id==input && *c==coordinate).map(|(_,_,image)|image.view.clone())
    }
}

#[cfg(test)]
#[path="bake_preflight_tests.rs"]
mod tests;
