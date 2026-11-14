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

impl Cache {
    pub fn approximating(&self) -> bool { !self.output_complete() || self.preparing || self.overview.as_ref().is_some_and(|overview| overview.approximating()) }


    pub fn has_pending_work(&self, r: &WgpuRasterizer) -> bool {
        use crate::frame_demand::Outstanding::{Blocked,Runnable};
        match self.outstanding(r) {Some(Runnable)=>true,Some(Blocked(input))=>!r.wakes(input),None=>false}
    }
    fn refining(&self, r: &WgpuRasterizer) -> bool {
        r.moving_layer.is_none() && !self.transform.as_ref().is_some_and(|preview| preview.moving)
    }
    pub(crate) fn outstanding(&self, r: &WgpuRasterizer) -> Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::Outstanding::Runnable;
        if self.contact || self.preparing {return Some(Runnable);}
        let own=self.cells_demand(r,self.refining(r)).or_else(||self.pixels.committed_root().is_none().then_some(Runnable));
        let overview=self.overview.as_ref().filter(|overview|!overview.held(r)).and_then(|overview|overview.outstanding(r));
        if own==Some(Runnable) || overview==Some(Runnable) {Some(Runnable)}else {own.or(overview)}
    }

    pub(super) fn refine(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut RegionRecords, budget: &mut WorkBudget,
    ) -> Result<PixelRect, GpuRasterError> {
        if exact_at_view_level(packet.scene) || self.gesture.is_some() || !self.refining(r) {return Ok(PixelRect::EMPTY);}
        let page_limit=(budget.remaining()/object_cache::PASS_TAPS).max(1) as usize;
        let mut missing=Vec::new();
        for cache in std::iter::once(&*self).chain(self.overview.as_deref()) {
            if cache.graph.root.is_none() {continue;}
            for coordinate in pages::coordinates(cache.plan.bounds,cache.plan.level) {
                let key=cache.refinement_interest(self.evaluation,cache.plan.level,pages::bounds(coordinate,cache.plan.level,cache.plan.extent)).key;
                self.pages.lock().unwrap().refinement_frontier(&key,page_limit,&mut missing,0);
                if missing.len()>=page_limit {break;}
            }
            if missing.len()>=page_limit {break;}
        }
        let (reductions,native):(Vec<_>,Vec<_>)=missing.into_iter().partition(|key|key.level>0);
        #[cfg(test)] crate::test_support::PassCounts {destination_units:reductions.len() as u64,..Default::default()}.record();
        let mut changed=PixelRect::EMPTY;
        if !reductions.is_empty() {
            for key in reductions {self.reduce_exact(scene,r,encoder,commands,key)?;}
            changed=self.project_exact(scene,r,encoder,commands)?;
        }
        let passes=packet.scene.order().iter().filter_map(|handle|packet.scene.effect(*handle).filter(|effect|
            effect.program.image_boundary() && packet.scene.visible(*handle)))
            .map(|effect|effect.program.passes.len().max(1) as u64).sum::<u64>();
        for key in native {
            let region=key.plan().bounds;
            if std::iter::once(&*self).chain(self.overview.as_deref()).any(|cache|cache.plan.level==key.level && cache.refused(region)) {continue;}
            let window=scene.cached_capture_window(packet.scene,region);
            let work=region.area().saturating_add(window.area().saturating_mul(passes));
            if !budget.fits(Class::Refinement,!changed.is_empty(),work) {break;}
            let plan=key.plan();
            let destination=Image {texture:r.empty_texture.clone(),view:r.empty_view.clone(),plan};
            let prepared=r.ensure_exact_preview(encoder).and_then(|()|scene.prepare_region(r,packet,(region,window),PixelRect::EMPTY,false,encoder))
                .and_then(|()|scene.declare_capture(r,packet,&destination,&[region],scene::Output::Display,None,None));
            let inputs=match prepared {Ok(inputs)=>inputs,Err(GpuRasterError::NotReady|GpuRasterError::SourceWorkingSetExceeded)=>continue,Err(error)=>return Err(error)};
            let scratch=self.graph.root.as_ref().unwrap().cost().1 as usize+1;
            let interest=Self::exact_interest(key.clone(),self.evaluation,region);
            let handle=match self.accept_working(scene,r,interest,vec![(key.clone(),region)],inputs,[scratch,0],plan,true,Class::Refinement,Class::Refinement,encoder)? {
                Ok(handle)=>handle,Err(None)=>continue,Err(Some(blocked))=>{self.refuse(region,&blocked);continue;},
            };
            #[cfg(test)] crate::test_support::PassCounts {destination_units:1,..Default::default()}.record();
            let image=self.pages.lock().unwrap().allocated(&key,Class::Refinement).expect("accepted exact page");
            let write=self.pages.lock().unwrap().prepare_write(key.clone(),region,pages::Accuracy::Exact,0)?;
            let mut held=self.pages.lock().unwrap().take_inputs(&handle)?;
            held.clone().track(encoder);
            let result=(|| {
                #[cfg(test)]
                let _record=crate::scene::sources::record_reads(&held);
                let accepted=&mut held;
                scene.capture_prepared_regions(r,packet,&image,&[region],scene::Output::Display,false,None,encoder, accepted)
            })();
            match result {
                Ok(())=>{
                    self.pages.lock().unwrap().commit(write,work,encoder);
                    self.pages.lock().unwrap().finish_inputs(&handle);
                    r.metrics.composited_pixels+=region.area();r.metrics.frame_composited_pages.push((0,key.coordinate));
                    budget.spend(work.max(object_cache::PASS_TAPS));
                    if self.plan.level.max(self.overview.as_ref().map_or(0,|cache|cache.plan.level))>0 {
                        self.reduce_exact(scene,r,encoder,commands,pages::Key {level:1,coordinate:key.coordinate.map(|value|value/2),..key})?;
                    }
                    changed=changed.union(self.project_exact(scene,r,encoder,commands)?);
                },
                Err(GpuRasterError::NotReady)=>self.pages.lock().unwrap().hold_inputs(&handle,held)?,
                Err(GpuRasterError::SourceWorkingSetExceeded)=>self.pages.lock().unwrap().finish_inputs(&handle),
                Err(error)=>return Err(error),
            }
        }
        Ok(changed)
    }

    fn reduce_exact(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,commands:&mut RegionRecords,
        mut key:pages::Key,
    )->Result<(),GpuRasterError> {
        let level=self.plan.level.max(self.overview.as_ref().map_or(0,|cache|cache.plan.level));
        loop {
            if self.pages.lock().unwrap().get(&key,key.plan().bounds,pages::Accuracy::Exact,0,Class::Refinement).is_none() {
                let inputs=self.pages.lock().unwrap().reduction(&key,0,Class::Refinement);
                let Some((inputs,pages::Accuracy::Exact))=inputs else {break;};
                let image=match self.claim_page(scene,r,key.clone(),Class::Refinement)? {Ok(image)=>image,Err(blocked)=>{self.refuse(key.plan().bounds,&blocked);break;}};
                let write=self.pages.lock().unwrap().prepare_write(key.clone(),image.plan.bounds,pages::Accuracy::Exact,0)?;
                let work=inputs.len() as u64*object_cache::PASS_TAPS;
                for (_,input) in inputs {
                    let covered=input.plan.bounds.intersect(image.plan.bounds);
                    let [x,y,width,height]=paint_transform::texel_rect(covered.window_local(image.plan.bounds),1<<image.plan.level);
                    let mut values=[0;20];
                    values[..8].copy_from_slice(&[x,y,width,height,input.plan.bounds.width(),input.plan.bounds.height(),2,(input.plan.level<<8)|8]);
                    values[14]=((x*2) as f32).to_bits();values[15]=((y*2) as f32).to_bits();
                    let binding=RegionRecords::binding(r,&input.view,&r.empty_view,&image.view);
                    commands.reduce(r,encoder,values,&binding,"reduce exact composite page",image.texture.format())?;
                    input.track(encoder);
                }
                self.pages.lock().unwrap().commit(write,work,encoder);
            }
            if key.level>=level {break;}
            key=pages::Key {level:key.level+1,coordinate:key.coordinate.map(|value|value/2),..key};
        }
        Ok(())
    }

    fn project_exact(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,commands:&mut RegionRecords)->Result<PixelRect,GpuRasterError> {
        let mut changed=self.copy_level(scene,r,encoder,commands)?;
        if let Some(cache)=&mut self.overview {changed=changed.union(cache.copy_level(scene,r,encoder,commands)?);}
        Ok(changed)
    }
}

impl Scene {
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
