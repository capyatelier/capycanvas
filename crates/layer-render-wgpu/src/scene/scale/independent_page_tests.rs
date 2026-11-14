use super::*;

struct Pages {
    stores:Vec<GatedStore>,
    source:Arc<SourceImage>,
    r:WgpuRasterizer,
    doc:Document,
    ink:SourceTarget,
    scene:Box<Scene>,
    cache:Cache,
    commands:RegionRecords,
    wakes:std::sync::mpsc::Receiver<()>,
}

impl Pages {
    fn new(ready:&[u32])->Self {
        let extent=[4*PAGE_SIZE,PAGE_SIZE];
        let mut doc=document_at(extent);
        composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
        let bottom=doc.scene().order()[0];
        let base=rgba8_source(extent,|_,_|[31,67,113,255]);
        paint_mut(&mut doc,0).base=Some(PaintBase::new(base.clone().into()));
        let source=rgba8_source(extent,|x,_|[99+(x/PAGE_SIZE*20) as u8,41,83,193]);
        let top=paint_occurrence(&mut doc,"Stored pages",Some(source.clone()));
        let ink_owner=paint_occurrence(&mut doc,"Ink",None);
        set_root_entries(&mut doc,vec![ink_owner,top,bottom]);
        let ink=doc.scene().source_target(ink_owner).unwrap();
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.ensure_document_metadata(extent,doc.scene()).unwrap();
        working_scene_frame(&mut r,frame(&doc,0));
        let mut required=crate::compile::Needs::capture_scene(&r,doc.scene(),false);
        required.render.extend(crate::startup::SourceKinds::for_scene(doc.scene()).pipelines(&r.scene_pipelines.source).cloned());
        required.compute.extend(r.scene_pipelines.scale_pipelines(r.display_precision(doc.scene()).format()).into_iter().cloned());
        pollster::block_on(required.prepare(&r.compilation)).unwrap();
        let (wake,wakes)=std::sync::mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        for (input,pages) in [(&base,(0..4).collect::<Vec<_>>()),(&source,ready.to_vec())] {
            for page in pages {
                let pixels=crate::prepare_image_tile(input,[page,0],r.device.working_space(),&r.device.source_samples,None).unwrap();
                Scene::decode_prepared_for(&mut r,&mut encoder,input,[page,0],pixels,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
            }
        }
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        let stores=(0..4).map(|page| {
            let mut inventory=(*source).clone();inventory.tiles.retain(|coordinate,_|*coordinate==[page,0]);
            GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([&inventory]),false)
        }).collect();
        let scene=r.scene.take().unwrap_or_else(||Scene::new(&r).unwrap());
        let cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},doc.scene().order().len());
        let commands=RegionRecords::new(&r);
        Self {stores,source,r,doc,ink,scene,cache,commands,wakes}
    }

    fn attempt(&mut self,dirty:PixelRect)->Result<PixelRect,GpuRasterError> {
        for _ in self.wakes.try_iter() {}
        let frame=FramePacket {composite_all:false,..frame(&self.doc,0)};
        let mut encoder=crate::submission::CommandEncoder::new(&self.r.device,&Default::default());
        self.scene.begin_frame();self.commands.begin();
        self.scene.scale_sources.prepare(&self.r,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&self.r,frame,&self.scene);
        let prepared=self.cache.prepare_graph(&self.r,frame,&self.scene);
        let result=prepared.and_then(|()|
            self.cache.render_graph(&mut self.scene,&mut self.r,frame,dirty,
                &mut Encoding {encoder:&mut encoder,commands:&mut self.commands, retained:0, budget:&mut Scene::begin_work(false)},Destination::View,None));
        self.cache.unchanged=true;
        if result.is_ok() {self.scene.encode_jobs(&mut self.r,&mut encoder).unwrap();}
        else {self.scene.discard_unencoded_jobs();}
        self.r.uploads.finish(&encoder);
        if encoder.has_work() {encoder.submit(&self.r.queue);crate::test_support::complete(&self.r);}
        result
    }

    fn ready(&mut self,page:u32) {
        self.stores[page as usize].gate.release();
        self.r.request_exact_image_decode(self.source.tiles[&[page,0]].resource_id(),self.source.clone(),[page,0],crate::residency::Class::Visible).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&self.r.device,&Default::default());
        crate::test_support::query_ready(&mut self.r,&mut encoder,|r,_|
            r.source_tiles.borrow().prepared_view(&self.source,[page,0])
                .filter(|view|view.texture().format()==wgpu::TextureFormat::Rgba32Float)
                .cloned().ok_or(GpuRasterError::NotReady));
        self.r.uploads.finish(&encoder);encoder.submit(&self.r.queue);crate::test_support::complete(&self.r);
    }

    fn assert_ready(&self,pages:&BTreeSet<[u32;2]>) {
        let coverage=self.cache.coverage_snapshot();
        for coordinate in page_coordinates(self.cache.plan.bounds) {
            assert_eq!(coverage.ready(page_rect(coordinate),pages::Accuracy::Display) || self.cache.displays(page_rect(coordinate)),pages.contains(&coordinate),
                "a pending input must not hold independent ready page units: {coordinate:?}");
        }
        assert!(!coverage.ready(page_rect([0,0]),pages::Accuracy::Display) && !self.cache.displays(page_rect([0,0])),"the held page must remain invalid");
        assert!(!pages::Coverage::covers(&coverage.written,page_rect([0,0])),"an unready page must not commit a composite prefix");
        let window=pixels(&self.r,self.cache.texture());
        let back=[31.,67.,113.].map(|code|layer_core::color::RgbSpace::Srgb.decode(code/255.) as f32);
        let alpha=193./255.;
        for &page in pages {
            let front=[99.+page[0] as f64*20.,41.,83.].map(|code|layer_core::color::RgbSpace::Srgb.decode(code/255.) as f32);
            let expected:[f32;4]=std::array::from_fn(|channel|if channel==3 {1.}else{front[channel]*alpha+back[channel]*(1.-alpha)});
            let image=(!self.cache.displays(page_rect(page))).then(|| {
                let key=self.cache.page_key(self.cache.graph.root.clone().unwrap(),page_rect(page));
                self.cache.pages.lock().unwrap().get(&key,page_rect(page),pages::Accuracy::Display,
                    self.cache.required_input_level(),Class::Visible).expect("ready unit has committed current pixels")
            });
            let stored=image.as_ref().map(|image|pixels(&self.r,&image.texture));
            let (actual,width,origin,format)=match (&image,&stored) {
                (Some(image),Some(pixels))=>(pixels.as_slice(),PAGE_SIZE,0,image.texture.format()),
                _=>(window.as_slice(),self.doc.composition().size[0],page[0]*PAGE_SIZE,self.cache.texture().format()),
            };
            let expected=committed_color(format,expected);
            for y in 0..PAGE_SIZE {for x in 0..PAGE_SIZE {
                let pixel=actual[(y*width+origin+x) as usize];
                assert!(pixel.iter().zip(expected).all(|(actual,expected)|(*actual-expected).abs()<2e-5),
                    "ready page {page:?} at [{x},{y}]: {pixel:?} != {expected:?}");
            }}
        }
    }

    fn stroke(r:&mut WgpuRasterizer,doc:&Document,ink:SourceTarget,step:u32)->PixelRect {
        let mut dab=crate::tests::test_dab([64.+step as f32*8.,64.],[0.8,0.1,0.2,1.],1.);dab.radii=[8.;2];
        let mut batch=crate::test_support::dab_batch(ink,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
        batch.stroke_start=step==0;batch.stroke_end=false;
        crate::test_support::submit_ready(r,FramePacket {commit_rasters:false,composite_all:false,
            dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame(doc,0)}).unwrap();
        crate::test_support::complete(r);
        r.transform_damage.iter().filter(|(target,_)|*target==ink).fold(PixelRect::EMPTY,|dirty,(_,region)|dirty.union(*region))
    }

    fn finish(&mut self,strokes:u32) {
        for store in &self.stores {store.gate.release();}
        crate::test_support::settle_decodes(&mut self.r);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        while !self.cache.coverage_complete() {
            let result=self.attempt(PixelRect::EMPTY);
            assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"released page units: {result:?}");
            assert!(std::time::Instant::now()<deadline,"released page units must complete coverage");
            crate::test_support::settle_decodes(&mut self.r);
        }
        assert!(self.cache.coverage_complete());
        let mut reference=WgpuRasterizer::new_native_headless(self.doc.composition().color).unwrap();
        for step in 0..strokes {Self::stroke(&mut reference,&self.doc,self.ink,step);}
        crate::test_support::submit_settled(&mut reference,frame(&self.doc,0));
        let expected=pixels(&reference,reference.scale_display.as_ref().unwrap().texture()).into_iter()
            .map(|pixel|committed_color(self.cache.texture().format(),pixel)).collect::<Vec<_>>();
        let actual=pixels(&self.r,self.cache.texture());
        assert_eq!(actual.len(),expected.len());
        let error=crate::test_support::max_error(&actual,&expected);
        let first=actual.iter().zip(&expected).position(|(a,b)|a.iter().zip(b).any(|(a,b)|(*a-*b).abs()>=2e-5))
            .map(|i|([i%self.doc.composition().size[0] as usize,i/self.doc.composition().size[0] as usize],actual[i],expected[i]));
        assert!(error<2e-5,"committed pixels differ: max={error}, formats={:?}/{:?}, first={first:?}",
            self.cache.texture().format(),reference.scale_display.as_ref().unwrap().texture().format());
    }
}

#[test]
fn a_pending_stored_page_does_not_block_other_ready_page_units() {
    let mut pages=Pages::new(&[1,2,3]);
    let ready=(1..4).map(|page|[page,0]).collect();
    for attempt in 0..3 {
        let result=pages.attempt(PixelRect::full(pages.doc.composition().size));
        assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"attempt {attempt}: {result:?}");
        pages.stores[0].gate.entered();
        pages.assert_ready(&ready);
    }
    pages.finish(0);
}

#[test]
fn cold_stroke_frames_commit_new_ready_pages_outside_the_dab_damage() {
    let mut pages=Pages::new(&[]);
    let result=pages.attempt(PixelRect::full(pages.doc.composition().size));
    assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    pages.stores[0].gate.entered();
    assert!(pages.cache.coverage_snapshot().written.is_empty());
    assert!(committed_composite_pages(&pages.cache).is_empty());
    let mut ready=BTreeSet::new();
    for page in 1..4 {
        pages.ready(page);
        let dirty=Pages::stroke(&mut pages.r,&pages.doc,pages.ink,page-1);
        assert!(!dirty.is_empty() && dirty.intersect(page_rect([0,0]))==dirty,"actual dab damage: {dirty:?}");
        let coverage=pages.cache.coverage_snapshot();
        let before=page_coordinates(pages.cache.plan.bounds).filter(|coordinate|
            (coverage.ready(page_rect(*coordinate),pages::Accuracy::Display) || pages.cache.displays(page_rect(*coordinate)))
                && dirty.intersect(page_rect(*coordinate)).is_empty()).count();
        pages.cache.unchanged=false;
        let result=pages.attempt(dirty);
        assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"frame {page}: {result:?}");
        ready.insert([page,0]);
        pages.assert_ready(&ready);
        assert_eq!(ready.len(),before+1,"each cold stroke frame must publish its newly ready page outside the dab damage");
    }
    pages.finish(3);
}

#[test]
fn one_remaining_source_preparation_attempt_does_not_discover_every_cold_page() {
    let mut pages=Pages::new(&[]);
    let frame=frame(&pages.doc,0);
    let handle=pages.doc.scene().order()[1];
    let target=pages.doc.scene().source_target(handle).unwrap();
    let extent=pages.doc.composition().size;
    let plan=display_mips::Plan::at(extent,0);
    let before_misses=pages.r.source_tiles.borrow().misses;
    let before_slots=pages.r.source_tiles.borrow().allocated_slots();
    let mut encoder=submission::CommandEncoder::new(&pages.r.device,&Default::default());
    pages.scene.begin_frame();
    pages.scene.scale_sources.prepare(&pages.r,frame,&[]);
    crate::scene::scale::tests::prepare_fixture_frame(&pages.r,frame,&pages.scene);
    let mut budget=Scene::begin_work(false);budget.spend(object_cache::IDLE_TAPS-object_cache::PASS_TAPS);
    let changed=pages.cache.prepare_source(&mut pages.scene,&mut pages.r,frame,&mut encoder,target,
        SourceRequest {plan,required:PixelRect::full(extent).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut budget).unwrap();
    assert!(changed.is_empty());assert_eq!(encoder.pass_count(),0);
    assert_eq!(pages.r.source_tiles.borrow().misses,before_misses);
    assert!(source_chunks(&pages.cache,target,0).is_empty());assert_eq!(pages.scene.scale_sources.entries[&target].updates,0);
    assert!(pages.cache.coverage_snapshot().written.is_empty());assert!(committed_composite_pages(&pages.cache).is_empty());
    let requested=pages.r.moving_decode.as_ref().map(|(_,queue)|queue.requested_image_tiles()).unwrap_or_default();
    assert_eq!(requested.len(),1,"one remaining preparation attempt must discover one cold input, not every page: {requested:?}");
    assert_eq!(pages.r.source_tiles.borrow().allocated_slots()-before_slots,1);
    assert_eq!(budget.used,object_cache::IDLE_TAPS,"a pending attempt consumes the remaining frame budget");
    pages.scene.discard_unencoded_jobs();pages.r.uploads.finish(&encoder);drop(encoder);
}
