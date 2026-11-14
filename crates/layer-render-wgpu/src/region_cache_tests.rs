use super::*;
use crate::test_support::{flush_engine,submit_settled};
use layer_render::RegionPreparation;

#[test]
fn fill_preview_matches_commit_and_swaps_without_changing_the_document() {
    let extent=[64,48];
    let mut document=paint_document(extent,"Fill preview");
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y|[(3*x) as u8,(4*y) as u8,151,77]));
    let target=target(&document);
    let (_producer,consumer)=layer_engine::input_queue(64);
    let mut engine=layer_engine::CanvasEngine::new(r,document,consumer,crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    flush_engine(&mut engine);
    let original=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let authored=engine.document().artwork.clone();
    let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(),extent,[0;2]);
    coverage.source.default_coverage=0.;coverage.selection=Some(rectangle([20,12,40,36]));
    let operation=layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,
        kind:layer_core::RasterOperationKind::Fill {color:[1.,0.,0.,1.],texture:Default::default(),alpha_locked:false,
            edges: None, underfill:Some(rectangle([20,12,24,36]))}};
    engine.set_fill_preview(Some(layer_render::FillPreview {transaction:1,revision:0,target,operation:operation.clone()}));
    flush_engine(&mut engine);
    let shown=engine.backend_mut().readback_srgb_rgba8().unwrap();
    assert_ne!(shown,original);
    for y in 0..extent[1] {for x in 0..extent[0] {
        let at=(y*extent[0]+x) as usize*4;
        if !(20..40).contains(&x) || !(12..36).contains(&y) {assert_eq!(&shown[at..at+4],&original[at..at+4]);}
    }}
    let mut swapped=operation.clone();swapped.coverage.selection=Some(rectangle([28,16,36,32]));
    engine.set_fill_preview(Some(layer_render::FillPreview {transaction:1,revision:1,target,operation:swapped}));
    flush_engine(&mut engine);
    assert_ne!(engine.backend_mut().readback_srgb_rgba8().unwrap(),shown);
    engine.set_fill_preview(None);flush_engine(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),original);
    assert_eq!(engine.document().artwork,authored);assert!(!engine.can_undo());
    engine.append_raster_operation(target,operation).unwrap();flush_engine(&mut engine);
    let committed=engine.backend_mut().readback_srgb_rgba8().unwrap();
    for y in 0..extent[1] {for x in 0..extent[0] {
        let at=(y*extent[0]+x) as usize*4;
        if (20..24).contains(&x) && (12..36).contains(&y) {
            assert!(committed[at..at+4].iter().zip(&shown[at..at+4]).all(|(a,b)|a.abs_diff(*b)<=3));
        }else{assert_eq!(&committed[at..at+4],&shown[at..at+4]);}
    }}
    assert!(engine.undo().unwrap());flush_engine(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),original);
    assert!(!engine.can_undo());
}

#[test]
fn fill_tolerance_preview_and_commit_keep_the_latest_three_band_mask() {
    let extent=[384,256];
    let mut document=paint_document(extent,"Fill tolerance");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,_| {
        let gray=if x<128 {64}else if x<256 {80}else{160};[gray,gray,gray,255]
    }));
    let target=target(&document);
    let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
        crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    flush_engine(&mut engine);
    let original=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let source=RegionSource::Scene {snapshot:engine.document().snapshot(),scope:layer_core::SceneScope::Raw(target)};
    let mut latest=None;let mut shown=Vec::new();
    for (revision,tolerance) in [0.1,0.8,0.2].into_iter().enumerate() {
        let mut query=request(source.clone(),rectangle([0,0,1,1]),RegionRefinement {close_gaps:true,edges:RegionEdges::LineMiddle,smooth_edges:true},tolerance);
        query.enclosure=None;query.position=[85,110];query.fill=Some(layer_render::RegionFillQuery {trail:Some(layer_render::RegionFillTrail {transaction:51,points:Default::default()}),..Default::default()});
        let result=receive_request(engine.backend_mut(),query);
        assert_eq!(value(&result.pixels,64,128),255);
        assert_eq!(value(&result.pixels,192,128),255);
        assert_eq!(value(&result.pixels,320,128),if tolerance==0.8 {255}else{0});
        let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(),extent,[0;2]);
        coverage.source.default_coverage=0.;coverage.selection=Some(Selection::pixels(result.pixels));
        let operation=layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,
            kind:layer_core::RasterOperationKind::Fill {color:[1.,0.,0.,1.],texture:Default::default(),alpha_locked:false,
                edges: None, underfill:result.underfill.map(Selection::pixels)}};
        engine.set_fill_preview(Some(layer_render::FillPreview {transaction:51,revision:revision as u64,target,operation:operation.clone()}));
        flush_engine(&mut engine);
        shown=engine.backend_mut().readback_srgb_rgba8().unwrap();
        let far=(128*384+320)*4;
        assert_eq!(&shown[far..far+4],if tolerance==0.8 {&[255,0,0,255]}else{&original[far..far+4]});
        latest=Some(operation);
    }
    engine.set_fill_preview(None);
    engine.append_raster_operation(target,latest.unwrap()).unwrap();flush_engine(&mut engine);
    let committed=engine.backend_mut().readback_srgb_rgba8().unwrap();
    assert_eq!(committed,shown);
    let far=(128*384+320)*4;assert_eq!(&committed[far..far+4],&original[far..far+4]);
    assert!(engine.undo().unwrap());flush_engine(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),original);
}

#[test]
fn fill_amendment_restores_the_original_source_and_keeps_one_undo_entry() {
    let extent=[32;2];let mut document=paint_document(extent,"Seal removal");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|_,_|[0,0,255,255]));
    let target=target(&document);let SourceTarget::Paint(handle)=target else{unreachable!()};
    let original_source=document.artwork.paint.get(handle).unwrap().clone();
    let r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(r,document,layer_engine::input_queue(8).1,crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    flush_engine(&mut engine);let original=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(),extent,[0;2]);
    coverage.source.default_coverage=0.;coverage.selection=Some(rectangle([8,8,16,24]));
    let mut operation=layer_core::RasterOperation {placement:Affine::IDENTITY,coverage,kind:layer_core::RasterOperationKind::Fill {
        color:[1.,0.,0.,1.],texture:Default::default(),alpha_locked:false,edges: None, underfill:None}};
    engine.append_raster_operation(target,operation.clone()).unwrap();flush_engine(&mut engine);
    let checkpoint=engine.checkpoint();let filled=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let filled_source=engine.document().artwork.paint.get(handle).unwrap().clone();
    let replace=|engine:&layer_engine::CanvasEngine<WgpuRasterizer>,source|layer_core::Edit::Paint(layer_core::RecordChange::replace(&engine.document().artwork.paint,handle,Some(source)).unwrap());
    engine.preview_edit(replace(&engine,original_source.clone())).unwrap();flush_engine(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),original);
    engine.preview_edit(replace(&engine,filled_source)).unwrap();flush_engine(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),filled);
    engine.preview_edit(replace(&engine,original_source)).unwrap();flush_engine(&mut engine);
    operation.coverage.selection=Some(rectangle([8,8,24,24]));
    engine.amend_raster_operation(target,operation,checkpoint).unwrap();flush_engine(&mut engine);
    assert_ne!(engine.backend_mut().readback_srgb_rgba8().unwrap(),filled);
    assert!(engine.undo().unwrap());flush_engine(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),original);assert!(!engine.can_undo());
}

#[test]
fn region_cache_restore_keeps_all_pages_while_frame_time_advances() {
    fn settle(r:&mut WgpuRasterizer,document:&Document,elapsed:&mut f32) {
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        loop {
            *elapsed+=1.;
            let frame=Arc::new(artwork::Frame::new(FramePacket {time_seconds:*elapsed,..packet(document.scene(),document.composition().size)},
                layer_core::EvaluationContext {elapsed:*elapsed,..Default::default()}));
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.prefetch_regions(&frame,&mut encoder).unwrap();
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
            r.wait_idle().unwrap();
            if !r.regions.as_ref().unwrap().background_pending() {break;}
            assert!(std::time::Instant::now()<deadline,"reference preparation must finish");
        }
    }
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
    let extent=[384,256];
    let mut document=paint_document(extent,"Restored reference");
    let paint=target(&document);
    let empty=placement::paint(&document).raster.clone();
    let bytes=[255,0,0,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize);
    let tile=RasterTile::backed(TileBlob::encode(document.composition().color.paint_descriptor(),&bytes).unwrap());
    placement::paint_mut(&mut document).raster=RasterRevision::backed(RasterData {tiles:(0..2).map(|x|
        (TileKey {plane:RasterPlane::Color,coordinate:[x,0]},tile.clone())).collect(),..Default::default()});
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r,&document,extent);
    r.startup=Some(startup::Startup::new(r.compilation.clone()).unwrap());
    r.shader_idle(true,true);
    let leaf=placement::occurrence_id(&document);
    let source=|document:&Document|RegionSource::reference(document.snapshot(),leaf).unwrap();
    r.prepare_regions(Some(&RegionPreparation {source:source(&document),reference_count:None,paper:None,tolerance:0.,refinement:Default::default(),enclosed:false}));
    let mut elapsed=0.;
    settle(&mut r,&document,&mut elapsed);
    placement::paint_mut(&mut document).raster=empty.clone();document.revision+=1;
    submit_ready(&mut r,FramePacket {restore_rasters:&[(paint,empty)],time_seconds:elapsed,..packet(document.scene(),extent)}).unwrap();
    settle(&mut r,&document,&mut elapsed);
    let (_,classified,dirty)=crate::region_requests::tests::cached_pages(&r);
    assert!(dirty.is_empty());
    let classified=pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,classified.as_ref().unwrap())).unwrap();
    assert!(classified.iter().all(|byte|*byte==255),"restored transparent pages remain eligible");
    for _ in 0..2 {
        let mut query=request(source(&document),rectangle([0,0,1,1]),Default::default(),0.);
        query.enclosure=None;query.position=[192,128];
        let result=receive_request(&mut r,query);
        assert_eq!(result.pixels.bounds(),[0,0,extent[0],extent[1]]);
        for point in [[64,128],[320,128]] {assert_eq!(value(&result.pixels,point[0],point[1]),255);}
    }
}

#[test]
fn region_cache_reference_edits_match_fresh_masks_across_page_and_window_seams() {
    let extent=[4097,257];
    let alpha=(0..extent[0]*extent[1]).map(|i| {
        let (x,y)=(i%extent[0],i/extent[0]);
        f32::from((barrier([80,32,2100,225],x,y) && !(y==32 && (252..260).contains(&x)))
            || (barrier([3300,32,3900,225],x,y) && !(y==32 && (3596..3604).contains(&x))))
    }).collect::<Vec<_>>();
    let mut document=Document::new(layer_core::authored::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Reference".into(),paper:"Paper".into()});
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y|
        [0,0,0,(alpha[(y*extent[0]+x) as usize]*255.) as u8]));
    assert!(document.bottom_paper().is_some());
    let reference=placement::occurrence_id(&document);
    let paint=target(&document);
    let SourceTarget::Paint(handle)=paint else {unreachable!()};
    let original=document.artwork.paint.get(handle).unwrap().raster.clone();
    let empty=paint_document(extent,"Fill target");
    let (_,stop)=placement::append_paint(&mut document,"Fill target",placement::paint(&empty).clone());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r,&document,extent);
    let preparation=|document:&Document| {
        let snapshot=document.snapshot();
        RegionPreparation {source:RegionSource::Intersection(vec![RegionSource::reference(snapshot.clone(),reference).unwrap(),
            RegionSource::Scene {snapshot,scope:layer_core::SceneScope::Raw(stop)}]),reference_count:Some(1),paper:None,tolerance:0.,refinement:Default::default(),enclosed:false}
    };
    r.prepare_regions(Some(&preparation(&document)));
    submit_settled(&mut r,packet(document.scene(),extent));
    for stage in 1..=4 {
        if stage%2==1 {
            document.artwork.paint.get_mut(handle).unwrap().raster=layer_core::raster::RasterRevision::pending();
            document.revision+=1;
            let mut ink=dab([0.,0.,0.,1.]);
            ink.center=Point {x:if stage==1 {256.}else{1888.},y:128.};ink.radii=[2.,112.];
            let mut stroke=batch(paint);stroke.damage=ink.bounds();
            submit_ready(&mut r,FramePacket {dabs:&[ink],dab_batches:&[stroke],..packet(document.scene(),extent)}).unwrap();
        }else{
            document.artwork.paint.get_mut(handle).unwrap().raster=original.clone();document.revision+=1;
            submit_ready(&mut r,FramePacket {restore_rasters:&[(paint,original.clone())],..packet(document.scene(),extent)}).unwrap();
        }
        submit_settled(&mut r,packet(document.scene(),extent));
        let prepared=preparation(&document);
        r.prepare_regions(Some(&prepared));
        assert!(!r.regions.as_ref().unwrap().background_pending(),"preparation must adopt the current reference without a second build, stage {stage}");
        let mut queries=Vec::new();let mut cached=Vec::new();
        for edges in [RegionEdges::Exact,RegionEdges::LineMiddle,RegionEdges::GrowShrink(2)] {
            for position in [[120,128],[3600,128]] {
                let mut query=request(prepared.source.clone(),rectangle([0,0,1,1]),RegionRefinement {edges,..Default::default()},0.);
                query.reference_count=Some(1);query.enclosure=None;query.position=position;
                cached.push(receive_request(&mut r,query.clone()));queries.push(query);
            }
        }
        r.prepare_regions(None);r.regions=None;
        for (query,cached) in queries.into_iter().zip(cached) {
            let fresh=receive_request(&mut r,query);
            assert_eq!((cached.pixels.bounds(),cached.pixels.words()),(fresh.pixels.bounds(),fresh.pixels.words()),"stage {stage}");
            assert_eq!(cached.underfill.as_deref().map(|pixels|(pixels.bounds(),pixels.words())),
                fresh.underfill.as_deref().map(|pixels|(pixels.bounds(),pixels.words())),"underfill stage {stage}");
        }
        r.prepare_regions(Some(&prepared));
        submit_settled(&mut r,packet(document.scene(),extent));
    }
}

#[test]
fn region_cache_tracks_clipping_group_children_and_topology() {
    let extent=[129;2];
    let alpha=(0..extent[0]*extent[1]).map(|i|f32::from(barrier([16,16,112,112],i%extent[0],i/extent[0]))).collect::<Vec<_>>();
    let mut document=uniform_document(extent,&alpha);
    let child=placement::occurrence_id(&document);let paint=target(&document);
    let SourceTarget::Paint(handle)=paint else {unreachable!()};
    let original=document.artwork.paint.get(handle).unwrap().raster.clone();
    document.apply(document.group_layers_edit(&[child],layer_core::LayerBlend::Normal,"Clip base").unwrap()).unwrap();
    let group=document.scene().children(None)[0];
    let opaque=uniform_document(extent,&vec![1.;(extent[0]*extent[1]) as usize]);
    let (reference,_)=placement::append_paint(&mut document,"Clipped reference",placement::paint(&opaque).clone());
    document.apply(document.drop_layers_edit(&[reference],group,layer_core::OccurrenceDropPosition::Above).unwrap().edit).unwrap();
    let mut clipped=document.scene().occurrence(reference).unwrap().clone();clipped.attachment=layer_core::Attachment::Clip;
    document.apply(layer_core::Edit::Occurrence(layer_core::authored::RecordChange::replace(&document.artwork.occurrences,reference,Some(clipped)).unwrap())).unwrap();
    assert_eq!(document.scene().clipping_base(reference),Some(group));
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    for stage in 0..4 {
        if stage==1 {
            document.artwork.paint.get_mut(handle).unwrap().raster=layer_core::raster::RasterRevision::pending();document.revision+=1;
            let mut ink=dab([0.,0.,0.,1.]);ink.center=Point {x:64.,y:64.};ink.radii=[2.,54.];
            let mut stroke=batch(paint);stroke.damage=ink.bounds();
            submit_ready(&mut r,FramePacket {dabs:&[ink],dab_batches:&[stroke],..packet(document.scene(),extent)}).unwrap();
        }else if stage==2 {
            document.artwork.paint.get_mut(handle).unwrap().raster=original.clone();document.revision+=1;
            submit_ready(&mut r,FramePacket {restore_rasters:&[(paint,original.clone())],..packet(document.scene(),extent)}).unwrap();
        }else if stage==3 {
            let alpha=(0..extent[0]*extent[1]).map(|i|f32::from(i%extent[0]==64 && (16..112).contains(&(i/extent[0])))).collect::<Vec<_>>();
            let added=uniform_document(extent,&alpha);
            let (added,_)=placement::append_paint(&mut document,"New clip child",placement::paint(&added).clone());
            let edit=document.drop_layers_edit(&[added],group,layer_core::OccurrenceDropPosition::Into).unwrap().edit;
            document.apply(edit).unwrap();submit_settled(&mut r,packet(document.scene(),extent));
        }
        let source=RegionSource::reference(document.snapshot(),reference).unwrap();
        let prepared=RegionPreparation {source:source.clone(),reference_count:None,paper:None,tolerance:0.,refinement:Default::default(),enclosed:false};
        r.prepare_regions(Some(&prepared));submit_settled(&mut r,packet(document.scene(),extent));
        let mut query=request(source,rectangle([0,0,1,1]),Default::default(),0.);query.enclosure=None;query.position=[40,64];
        let cached=receive_request(&mut r,query.clone());
        assert_eq!(value(&cached.pixels,80,64),if stage%2==0 {255}else{0},"stage {stage}");
        r.prepare_regions(None);r.regions=None;
        let fresh=receive_request(&mut r,query);
        assert_eq!((cached.pixels.bounds(),cached.pixels.words()),(fresh.pixels.bounds(),fresh.pixels.words()),"stage {stage}");
        r.prepare_regions(Some(&prepared));submit_settled(&mut r,packet(document.scene(),extent));
    }
}

#[test]
fn region_cache_prewarm_pauses_for_contact_resumes_idle_and_promotes_demand() {
    let extent=[64;2];
    let alpha=(0..extent[0]*extent[1]).map(|i|f32::from(barrier([12,12,52,52],i%extent[0],i/extent[0]))).collect::<Vec<_>>();
    let document=uniform_document(extent,&alpha);
    let source=RegionSource::Scene {snapshot:document.snapshot(),scope:layer_core::SceneScope::Raw(target(&document))};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit_settled(&mut r,packet(document.scene(),extent));
    r.startup=Some(startup::Startup::new(r.compilation.clone()).unwrap());
    r.shader_idle(false,false);
    r.prepare_regions(Some(&RegionPreparation {source:source.clone(),reference_count:None,paper:None,tolerance:0.,refinement:Default::default(),enclosed:false}));
    assert!(r.regions.as_ref().unwrap().background_pending());
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
    let frame=r.artwork_frame.clone().unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    r.prefetch_regions(&frame,&mut encoder).unwrap();
    assert_eq!(encoder.pass_count(),0);
    assert!(r.regions.as_ref().unwrap().raw.mask().is_none());
    r.shader_idle(true,true);
    assert!(r.next_frame()==layer_render::FrameDemand::Work);
    r.prefetch_regions(&frame,&mut encoder).unwrap();
    r.shader_idle(false,false);
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
    let mut query=request(source,rectangle([0,0,1,1]),exact(),0.);
    query.enclosure=None;query.position=[32,32];
    check(&receive_request(&mut r,query),&rectangles(extent,&[[13,13,51,51]]),extent);
    assert!(crate::region_requests::tests::cache_ready(&r));
    assert!(r.regions.as_ref().unwrap().background_pending());
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
    r.shader_idle(true,true);
    submit_settled(&mut r,packet(document.scene(),extent));
    assert!(!r.regions.as_ref().unwrap().background_pending());
    r.prepare_regions(None);
}

#[test]
fn region_cache_prewarm_completes_reuses_and_schedules_changed_preparation() {
    let extent = [513, 257];
    let alpha = (0..extent[0] * extent[1]).map(|i| f32::from(barrier([40, 40, 473, 225], i % extent[0], i / extent[0]))).collect::<Vec<_>>();
    let document = uniform_document(extent, &alpha);
    let source = RegionSource::Scene { snapshot: document.snapshot(), scope: layer_core::SceneScope::Raw(target(&document)) };
    let expected = [vec![[41., 41.], [472., 41.], [472., 224.], [41., 224.]]];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert!(r.regions.is_none());
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: Some(2), paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
    assert!(r.regions.is_none());
    frame(&mut r, &document, extent);
    r.set_refine_chunk(Some(1_048_576.));
    for enclosed in [false, true] {
        let mut preparation = RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed };
        r.prepare_regions(Some(&preparation));
        assert!(r.regions.as_ref().unwrap().background_pending());
        submit_ready(&mut r, packet(document.scene(), extent)).unwrap();
        assert!(r.regions.as_ref().unwrap().background_pending());
        submit_settled(&mut r, packet(document.scene(), extent));
        assert!(!r.regions.as_ref().unwrap().background_pending());
        let mut query = request(source.clone(), rectangle([10, 10, 503, 247]), exact(), 0.);
        query.position = [100, 100];
        if !enclosed { query.enclosure = None; }
        assert!(r.request_region(query.clone().into()).unwrap());
        assert!(!r.regions.as_ref().unwrap().background_pending(), "a prepared tap must reuse its reference geometry");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let result = loop {
            if let Some(result) = r.take_region() { break crate::test_support::mask_result(result.unwrap()); }
            assert!(std::time::Instant::now() < deadline, "prepared region readback timed out");
            std::thread::yield_now();
        };
        assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
        score_geometry(&result.pixels, &expected, [0, 0], &[], "prepared region").unwrap();
        preparation.tolerance = 0.25;
        r.prepare_regions(Some(&preparation));
        assert!(r.regions.as_ref().unwrap().background_pending(), "changed preparation must schedule work immediately");
        assert!(r.next_frame()==layer_render::FrameDemand::Work);
        submit_settled(&mut r, packet(document.scene(), extent));
        assert!(!r.regions.as_ref().unwrap().background_pending());
        query.tolerance = preparation.tolerance;
        let result = receive_request(&mut r, query);
        assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
        score_geometry(&result.pixels, &expected, [0, 0], &[], "changed preparation").unwrap();
    }
    r.prepare_regions(None);
    assert!(!r.regions.as_ref().unwrap().background_pending());
}

#[test]
fn region_cache_foreground_promotion_cancel_drops_the_old_request() {
    let extent = [513, 257];
    let alpha = (0..extent[0] * extent[1]).map(|i| f32::from(barrier([40, 40, 180, 225], i % extent[0], i / extent[0]))).collect::<Vec<_>>();
    let mut document = uniform_document(extent, &alpha);
    let first = target(&document);
    let alpha = (0..extent[0] * extent[1]).map(|i| f32::from(barrier([210, 60, 500, 240], i % extent[0], i / extent[0]))).collect::<Vec<_>>();
    let second = uniform_document(extent, &alpha);
    let (_, second) = placement::append_paint(&mut document, "Other reference", placement::paint(&second).clone());
    let snapshot = document.snapshot();
    let source = RegionSource::Scene { snapshot: snapshot.clone(), scope: layer_core::SceneScope::Raw(first) };
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    r.set_refine_chunk(Some(1_048_576.));
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
    submit_ready(&mut r, packet(document.scene(), extent)).unwrap();
    assert!(r.regions.as_ref().unwrap().background_pending());
    let mut query = request(source, rectangle([0, 0, 1, 1]), exact(), 0.);
    query.enclosure = None; query.position = [100, 100]; query.request_id = 90;
    assert!(r.request_region(query.into()).unwrap());
    assert!(r.region_pending());
    r.prepare_regions(Some(&RegionPreparation { source: RegionSource::Composite, reference_count: Some(2), paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
    assert!(r.region_pending());
    r.cancel_region();
    assert!(!r.region_pending());
    assert!(r.take_region().is_none());
    let source = RegionSource::Scene { snapshot, scope: layer_core::SceneScope::Raw(second) };
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
    let mut query = request(source, rectangle([0, 0, 1, 1]), exact(), 0.);
    query.enclosure = None; query.position = [300, 100];
    let result = receive_request(&mut r, query);
    assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
    score_geometry(&result.pixels, &[vec![[211., 61.], [499., 61.], [499., 239.], [211., 239.]]], [0, 0], &[], "request after cancellation").unwrap();
    assert!(!r.region_pending());
    assert!(r.take_region().is_none());
    r.prepare_regions(None);
}

#[test]
fn region_cache_prewarm_rebuilds_for_an_opaque_seed() {
    let extent=[64;2];
    let mut document=paint_document(extent,"Opaque reference");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y| {
        let wall=(16..48).contains(&x) && (16..48).contains(&y)
            && (!(20..44).contains(&x) || !(20..44).contains(&y))
            && !((31..33).contains(&x) && y<20);
        if wall {[0,0,0,255]}else{[255;4]}
    }));
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r,&document,extent);
    r.prepare_regions(Some(&RegionPreparation {source:RegionSource::Composite,reference_count:None,paper:None,tolerance:0.,refinement:Default::default(),enclosed:false}));
    submit_settled(&mut r,packet(document.scene(),extent));
    for close_gaps in [false,true] {
        let mut query=request(RegionSource::Composite,rectangle([0,0,1,1]),RegionRefinement {close_gaps,..exact()},0.);
        query.enclosure=None;query.position=[32,32];
        let result=receive_request(&mut r,query);
        assert_eq!(value(&result.pixels,32,32),255);
        assert_eq!(value(&result.pixels,0,0),if close_gaps{0}else{255});
        if close_gaps {score_geometry(&result.pixels,&[vec![[20.,20.],[44.,20.],[44.,44.],[20.,44.]]],[0;2],&[],"opaque seed").unwrap();}
    }
}

#[test]
fn region_cache_reference_parent_offset_moves_warm_geometry() {
    let extent = [192, 160];
    let alpha = (0..extent[0] * extent[1]).map(|i| f32::from(barrier([20, 20, 80, 100], i % extent[0], i / extent[0]))).collect::<Vec<_>>();
    let mut document = uniform_document(extent, &alpha);
    let leaf = placement::occurrence_id(&document);
    let target = target(&document);
    document.apply(document.group_layers_edit(&[leaf], layer_core::LayerBlend::Normal, "Reference group").unwrap()).unwrap();
    let group = document.scene().children(None)[0];
    let source = RegionSource::Scene { snapshot: document.snapshot(), scope: layer_core::SceneScope::Raw(target) };
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
    submit_settled(&mut r, packet(document.scene(), extent));
    assert!(!r.regions.as_ref().unwrap().background_pending());
    let mut query = request(source, rectangle([0, 0, 1, 1]), exact(), 0.);
    query.enclosure = None; query.position = [40, 60];
    let before = receive_request(&mut r, query);
    assert_eq!((before.request_id, before.placement, before.pixels.extent()), (91, Affine::IDENTITY, extent));
    score_geometry(&before.pixels, &[vec![[21., 21.], [79., 21.], [79., 99.], [21., 99.]]], [0, 0], &[], "original reference position").unwrap();
    let mut owner = document.scene().occurrence(group).unwrap().clone();
    owner.offset = [40, 10];
    let change = layer_core::authored::RecordChange::replace(&document.artwork.occurrences, group, Some(owner)).unwrap();
    document.apply(layer_core::Edit::Occurrence(change)).unwrap();
    let source = RegionSource::Scene { snapshot: document.snapshot(), scope: layer_core::SceneScope::Raw(target) };
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
    assert!(r.regions.as_ref().unwrap().background_pending());
    submit_settled(&mut r, packet(document.scene(), extent));
    assert!(!r.regions.as_ref().unwrap().background_pending());
    let mut query = request(source, rectangle([0, 0, 1, 1]), exact(), 0.);
    query.enclosure = None; query.position = [90, 70];
    let after = receive_request(&mut r, query);
    assert_eq!((after.request_id, after.placement, after.pixels.extent()), (91, Affine::IDENTITY, extent));
    score_geometry(&after.pixels, &[vec![[61., 31.], [119., 31.], [119., 109.], [61., 109.]]], [0, 0], &[], "moved reference parent").unwrap();
    assert_eq!(value(&after.pixels, 90, 70), 255);
    assert_eq!(value(&after.pixels, 40, 60), 0);
    assert_eq!(value(&after.pixels, 0, 0), 0);
    assert_eq!(value(&before.pixels, 40, 60), 255);
    assert_eq!(value(&before.pixels, 90, 70), 0);
    r.prepare_regions(None);
}

#[test]
fn region_cache_stop_target_dab_and_restore_keep_reference_geometry_ready() {
    let extent = [192, 160];
    let alpha = (0..extent[0] * extent[1]).map(|i| f32::from(barrier([20, 20, 160, 140], i % extent[0], i / extent[0]))).collect::<Vec<_>>();
    let mut document = uniform_document(extent, &alpha);
    let leaf = placement::occurrence_id(&document);
    let empty = paint_document(extent, "Stop target");
    let (_, stop) = placement::append_paint(&mut document, "Stop target", placement::paint(&empty).clone());
    let SourceTarget::Paint(handle) = stop else { unreachable!() };
    let original = document.artwork.paint.get(handle).unwrap().raster.clone();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    for stage in 0..3 {
        if stage == 1 {
            document.artwork.paint.get_mut(handle).unwrap().raster = layer_core::raster::RasterRevision::pending();
            document.revision += 1;
            let mut ink = dab([1., 0., 0., 1.]);
            ink.center = Point { x: 96., y: 80. }; ink.radii = [400.; 2];
            let mut stroke = batch(stop);
            stroke.damage = ink.bounds(); stroke.style.selection = Some(Arc::new(rectangle([90, 0, 110, 160])));
            submit_ready(&mut r, FramePacket { dabs: &[ink], dab_batches: &[stroke], ..packet(document.scene(), extent) }).unwrap();
            assert!(!r.regions.as_ref().unwrap().background_pending(), "painting only Stop must preserve the reference cache");
        } else if stage == 2 {
            document.artwork.paint.get_mut(handle).unwrap().raster = original.clone();
            document.revision += 1;
            submit_ready(&mut r, FramePacket { restore_rasters: &[(stop, original.clone())], ..packet(document.scene(), extent) }).unwrap();
            assert!(!r.regions.as_ref().unwrap().background_pending(), "restoring only Stop must preserve the reference cache");
        }
        let snapshot = document.snapshot();
        let source = RegionSource::Intersection(vec![RegionSource::reference(snapshot.clone(), leaf).unwrap(),
            RegionSource::Scene { snapshot, scope: layer_core::SceneScope::Raw(stop) }]);
        r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: Some(1), paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
        if stage == 0 { submit_settled(&mut r, packet(document.scene(), extent)); }
        assert!(!r.regions.as_ref().unwrap().background_pending(), "Stop changes must not reclassify references");
        let mut query = request(source, rectangle([0, 0, 1, 1]), exact(), 0.);
        query.reference_count = Some(1); query.enclosure = None; query.position = [40, 80];
        let result = receive_request(&mut r, query);
        assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
        let right = if stage == 1 { 90. } else { 159. };
        score_geometry(&result.pixels, &[vec![[21., 21.], [right, 21.], [right, 139.], [21., 139.]]], [0, 0], &[], "Stop target history").unwrap();
        assert_eq!(value(&result.pixels, 40, 80), 255);
        assert_eq!(value(&result.pixels, 120, 80), if stage == 1 { 0 } else { 255 });
        assert_eq!(value(&result.pixels, 0, 0), 0);
        assert!(!r.regions.as_ref().unwrap().background_pending());
    }
    r.prepare_regions(None);
}

#[test]
fn fill_keeps_the_tapped_thin_region_between_target_colours() {
    let extent=[64;2];
    let mut document=uniform_document(extent,&vec![0.;4096]);let reference=placement::occurrence_id(&document);
    let mut colours=paint_document(extent,"Target colours");
    placement::set_source(&mut colours,layer_core::color::source::rgba8_source(extent,|x,y|
        if x==21 && (20..44).contains(&y){[0;4]}else{[220,40,20,255]}));
    let (_,stop)=placement::append_paint(&mut document,"Target colours",placement::paint(&colours).clone());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let snapshot=document.snapshot();
    let source=RegionSource::Intersection(vec![RegionSource::reference(snapshot.clone(),reference).unwrap(),
        RegionSource::Scene {snapshot,scope:layer_core::SceneScope::Raw(stop)}]);
    let mut query=request(source,rectangle([0,0,1,1]),exact(),0.);
    query.position=[21,30];query.enclosure=None;query.reference_count=Some(1);
    check(&receive_request(&mut r,query),&rectangles(extent,&[[21,20,22,44]]),extent);
}

#[test]
fn fill_drag_accumulates_each_crossed_region_without_filling_the_dividing_ink() {
    let extent=[160,120];
    let alpha=(0..extent[0]*extent[1]).map(|i| {let (x,y)=(i%extent[0],i/extent[0]);
        f32::from(barrier([24,20,136,100],x,y) || x==70 && (20..100).contains(&y))}).collect::<Vec<_>>();
    let document=uniform_document(extent,&alpha);
    let source=RegionSource::Scene {snapshot:document.snapshot(),scope:layer_core::SceneScope::Raw(target(&document))};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let mut query=request(source,rectangle([0,0,1,1]),exact(),0.);query.enclosure=None;query.position=[40,60];
    query.fill=Some(layer_render::RegionFillQuery {trail:Some(layer_render::RegionFillTrail {transaction:71,points:Default::default()}),..Default::default()});
    let first=receive_request(&mut r,query.clone());assert_eq!(first.gaps.unwrap().next,None);
    query.fill.as_mut().unwrap().trail.as_mut().unwrap().points=vec![[40.,60.],[110.,60.]].into();
    let path=receive_request(&mut r,query.clone());let next=path.gaps.unwrap().next.unwrap();
    assert!((71..135).contains(&next[0]),"{next:?}");query.position=next;
    let complete=receive_request(&mut r,query.clone());assert_eq!(complete.gaps.as_ref().unwrap().next,None);
    check(&complete,&rectangles(extent,&[[25,21,70,99],[71,21,135,99]]),extent);
    query.fill.as_mut().unwrap().trail.as_mut().unwrap().transaction=72;
    let reset=receive_request(&mut r,query);assert!(reset.gaps.unwrap().next.is_some());
}

#[test]
fn fill_gap_64_stop_closes_a_gap_that_auto_leaves_open() {
    let extent=[160,120];
    let alpha=(0..extent[0]*extent[1]).map(|i| {let (x,y)=(i%extent[0],i/extent[0]);
        f32::from(barrier([24,20,136,100],x,y) && !(y==20 && (52..108).contains(&x)))}).collect::<Vec<_>>();
    let document=uniform_document(extent,&alpha);let source=RegionSource::Scene {snapshot:document.snapshot(),scope:layer_core::SceneScope::Raw(target(&document))};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let mut query=request(source,rectangle([0,0,1,1]),RegionRefinement {close_gaps:true,edges:RegionEdges::Exact,..exact()},0.);
    query.enclosure=None;query.position=[80,60];let auto=receive_request(&mut r,query.clone());
    assert_eq!(auto.pixels.bounds(),[0,0,160,120]);
    query.fill=Some(layer_render::RegionFillQuery {reach:Some(48),..Default::default()});
    let previous=receive_request(&mut r,query.clone());
    assert_eq!((&previous.pixels,&previous.underfill),(&auto.pixels,&auto.underfill));
    query.fill.as_mut().unwrap().reach=Some(64);
    let selected=receive_request(&mut r,query);let masks=selected.gaps.as_ref().unwrap();assert_eq!(masks.active,4);
    score_geometry(&selected.pixels,&[vec![[25.,21.],[135.,21.],[135.,99.],[25.,99.]]],[0,0],&[],"64 px gap stop").unwrap();
}

#[test]
fn fill_gap_requests_match_off_and_auto() {
    let extent=[160,120];
    let expected=[vec![[25.,21.],[135.,21.],[135.,99.],[25.,99.]]];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for thin in [true,false] {
        let alpha=(0..extent[0]*extent[1]).map(|i| {
            let (x,y)=(i%extent[0],i/extent[0]);
            f32::from(if thin {barrier([24,20,136,100],x,y) && !(y==20 && (76..84).contains(&x))}
                else{!(25..135).contains(&x) || !(21..99).contains(&y)})
        }).collect::<Vec<_>>();
        let document=uniform_document(extent,&alpha);
        let source=RegionSource::Scene {snapshot:document.snapshot(),scope:layer_core::SceneScope::Raw(target(&document))};
        frame(&mut r,&document,extent);
        for edges in [RegionEdges::Exact,RegionEdges::LineMiddle] {
            let mut query=request(source.clone(),rectangle([0,0,1,1]),RegionRefinement {close_gaps:true,edges,..exact()},0.);
            query.enclosure=None;query.position=[80,60];
            let auto=receive_request(&mut r,query.clone());
            let mut off=query.clone();off.refinement.close_gaps=false;
            let off=receive_request(&mut r,off);
            query.fill=Some(Default::default());
            let result=receive_request(&mut r,query.clone());
            assert_eq!((&result.pixels,&result.underfill),(&auto.pixels,&auto.underfill));
            let masks=result.gaps.unwrap();
            assert_eq!((masks.active,masks.line_bounded),(1,thin));
            for reach in [0,16,64] {
                query.fill.as_mut().unwrap().reach=Some(reach);
                let selected=receive_request(&mut r,query.clone());
                let slot=(reach/16) as usize;
                if reach==0 {assert_eq!((&selected.pixels,&selected.underfill),(&off.pixels,&off.underfill));}
                else if reach==16 {assert_eq!((&selected.pixels,&selected.underfill),(&auto.pixels,&auto.underfill));}
                if thin && reach>0 {score_geometry(&selected.pixels,&expected,[0,0],&[],"requested gap stop").unwrap();}
                assert_eq!(selected.gaps.unwrap().active,slot);
            }
            if thin {
                let ids=masks.seals.iter().map(|seal|seal.id).collect::<std::collections::BTreeSet<_>>();
                assert!(!ids.is_empty());
                query.fill.as_mut().unwrap().reach=None;
                query.fill.as_mut().unwrap().excluded_seals=ids.into_iter().collect::<Vec<_>>().into();
                let opened=receive_request(&mut r,query.clone());
                assert_eq!((&opened.pixels,&opened.underfill),(&off.pixels,&off.underfill));
                query.fill=None;
                let restored=receive_request(&mut r,query);
                assert_eq!((&restored.pixels,&restored.underfill),(&auto.pixels,&auto.underfill));
            }
        }
    }
}

#[test]
fn region_cache_overflow_matches_off_for_all_edges_and_warm_requests() {
    let extent = [1280, 1152];
    let expected = [vec![[1061., 129.], [1239., 129.], [1239., 299.], [1061., 299.]]];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for dense in [false, true] {
        let alpha = (0..extent[0] * extent[1]).map(|i| {
            let (x, y) = (i % extent[0], i / extent[0]);
            f32::from((barrier([1060, 128, 1240, 300], x, y) && !(y == 128 && (1148..1156).contains(&x)))
                || (dense && x < 1024 && x % 4 < 2 && y % 4 == 0))
        }).collect::<Vec<_>>();
        let document = uniform_document(extent, &alpha);
        let source = RegionSource::Scene { snapshot: document.snapshot(), scope: layer_core::SceneScope::Raw(target(&document)) };
        frame(&mut r, &document, extent);
        r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., refinement:Default::default(),enclosed: false }));
        submit_settled(&mut r, packet(document.scene(), extent));
        assert!(!r.regions.as_ref().unwrap().background_pending());
        for edges in [RegionEdges::Exact,RegionEdges::LineMiddle,RegionEdges::GrowShrink(2)] {
            let mut off=None;
            for close_gaps in [false,true,true] {
                let mut query = request(source.clone(), rectangle([0, 0, 1, 1]), RegionRefinement { close_gaps,edges,..exact() }, 0.);
                query.enclosure = None;
                query.position = [1150, 210];
                let result = receive_request(&mut r, query);
                assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
                assert!(!r.regions.as_ref().unwrap().background_pending());
                assert_eq!(value(&result.pixels, 1150, 210), 255);
                assert_eq!(value(&result.pixels, 1270, 400), if close_gaps && !dense {0}else{255});
                let actual=(result.pixels.bounds(),result.pixels.words().to_vec(),
                    result.underfill.as_ref().map(|pixels|(pixels.bounds(),pixels.words().to_vec())));
                if !close_gaps {off=Some(actual);}
                else if dense {assert_eq!(&actual,off.as_ref().unwrap(),"overflow Auto must equal Off for {edges:?}");}
                else if !matches!(edges,RegionEdges::GrowShrink(_)) {
                    score_geometry(&result.pixels, &expected, [0, 0], &[], "sparse contour").unwrap();
                }
            }
        }
        r.prepare_regions(None);
    }
}

#[test]
fn region_cache_paper_deletion_rebuilds_reference_without_resetting_layers() {
    let extent = [64, 64];
    let mut document = Document::new(layer_core::authored::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Reference".into(), paper: "Paper".into() });
    placement::set_source(&mut document, layer_core::color::source::rgba8_source(extent, |x, y|
        [0, 0, 0, if barrier([12, 12, 52, 52], x, y) { 255 } else { 0 }]));
    let target = target(&document);
    let mut preparation = RegionPreparation { source: RegionSource::Scene { snapshot: document.snapshot(), scope: layer_core::SceneScope::Raw(target) },
        reference_count: None, paper: document.nearest_paper_binding(document.drawing_content()), tolerance: 0., refinement:Default::default(),enclosed: false };
    assert!(preparation.paper.is_some());
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    r.set_refine_chunk(Some(1_048_576.));
    r.prepare_regions(Some(&preparation));
    submit_settled(&mut r, packet(document.scene(), extent));
    assert!(!r.regions.as_ref().unwrap().background_pending());
    let paper = document.bottom_paper().unwrap();
    let edit = document.delete_layers_edit(&[paper]).unwrap();
    document.apply(edit).unwrap();
    preparation.source = RegionSource::Scene { snapshot: document.snapshot(), scope: layer_core::SceneScope::Raw(target) };
    preparation.paper = document.nearest_paper_binding(document.drawing_content());
    assert!(preparation.paper.is_none());
    r.prepare_regions(Some(&preparation));
    submit_settled(&mut r, packet(document.scene(), extent));
    let mut query = request(preparation.source, rectangle([0, 0, 1, 1]), exact(), 0.);
    query.enclosure = None;
    query.position = [32, 32];
    check(&receive_request(&mut r, query), &rectangles(extent, &[[13, 13, 51, 51]]), extent);
    assert!(!r.regions.as_ref().unwrap().background_pending());
    r.prepare_regions(None);
}

fn varying_reach_document() -> Document {
    let extent=[304,136];
    let mut document=paint_document(extent,"Different gap reaches");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y| {
        let ink=(barrier([24,20,280,112],x,y) && !(y==20 && ((72..80).contains(&x)||(190..230).contains(&x))))
            || x==152 && (20..112).contains(&y);
        if ink {[51,77,102,255]}else{[0;4]}
    }));
    document
}

#[test]
fn fill_drag_with_different_auto_reaches_commits_and_reopens_the_second_seal_in_one_undo() {
    use layer_ui::{UiSession,UiAction,CommandId,Platform};
    use layer_engine::{PenEvent,PenPhase,SampleFlags,ToolKind};
    let extent=[304,136];let mut document=varying_reach_document();
    document.working.occurrence=Some(placement::occurrence_id(&document));
    let reference=target(&document);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let source=RegionSource::Scene {snapshot:document.snapshot(),scope:SceneScope::Raw(reference)};
    for (position,active) in [([80,70],1),([220,70],3)] {
        let mut query=request(source.clone(),rectangle([0,0,1,1]),Default::default(),0.);
        query.enclosure=None;query.position=position;query.fill=Some(Default::default());
        assert_eq!(receive_request(&mut r,query).gaps.unwrap().active,active);
    }
    let mut s=UiSession::new(r,document,[1200,900],Platform::Gtk).unwrap();
    s.dispatch(UiAction::Invoke {command:CommandId::Fill}).unwrap();
    s.dispatch(UiAction::SetColor {rgba:[1.,0.,0.,1.]}).unwrap();
    let clock=std::time::Instant::now();
    let settle=|s:&mut UiSession<WgpuRasterizer>| {
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        loop {
            let now=clock.elapsed().as_nanos() as u64;
            s.frame(now,now).unwrap();
            crate::test_support::complete(s.renderer_mut());
            if s.paint_ready() && !s.holding_input() && !s.engine().has_pending_input() && s.require_document_idle().is_ok() && !s.engine().raster_backing_pending() && !s.engine().has_pending_document_edits() && s.renderer_mut().next_frame()!=layer_render::FrameDemand::Work {break;}
            assert!(std::time::Instant::now()<deadline,"fill must settle");
            std::thread::yield_now();
        }
    };
    let pen=|s:&mut UiSession<WgpuRasterizer>,sequence,phase,point:[f32;2]| {
        let [a,b,c,d,x,y]=s.camera().document_to_surface();
        s.pen(PenEvent {device_id:1,sequence,timestamp_ns:clock.elapsed().as_nanos() as u64,view_revision:s.camera().revision,
            surface_position:Point {x:a*point[0]+c*point[1]+x,y:b*point[0]+d*point[1]+y},
            pressure:1.,tilt_radians:[0.;2],twist_radians:0.,distance:0.,phase,tool:ToolKind::Pen,flags:SampleFlags::PRIMARY});
    };
    settle(&mut s);
    let before=s.renderer_mut().readback_srgb_rgba8().unwrap();
    pen(&mut s,1,PenPhase::Down,[80.,70.]);
    let now=clock.elapsed().as_nanos() as u64;s.frame(now,now).unwrap();
    pen(&mut s,2,PenPhase::Move,[220.,70.]);pen(&mut s,3,PenPhase::Up,[220.,70.]);settle(&mut s);
    let filled=s.renderer_mut().readback_srgb_rgba8().unwrap();
    let pixel=|bytes:&[u8],x,y|bytes[((y*304+x)*4) as usize..][..4].to_vec();
    for [x,y] in [[80,70],[220,70]] {assert_eq!(pixel(&filled,x,y),[255,0,0,255],"filled {x},{y}");}
    for [x,y] in [[8,70],[152,70],[24,70],[280,70]] {assert_eq!(pixel(&filled,x,y),pixel(&before,x,y));}
    pen(&mut s,4,PenPhase::Down,[210.,20.]);pen(&mut s,5,PenPhase::Up,[210.,20.]);settle(&mut s);
    let reopened=s.renderer_mut().readback_srgb_rgba8().unwrap();
    assert_eq!(pixel(&filled,210,20),[0;4]);
    assert_eq!(pixel(&reopened,210,20),[255,0,0,255]);
    assert_eq!(pixel(&reopened,8,70),pixel(&before,8,70));
    for (index,ink) in before.chunks_exact(4).enumerate().filter(|(_,pixel)|pixel[3]!=0) {
        assert_eq!(&filled[index*4..][..4],ink,"committed ink {index}");
        assert_eq!(&reopened[index*4..][..4],ink,"reopened ink {index}");
    }
    s.dispatch(UiAction::Invoke {command:CommandId::Undo}).unwrap();
    assert!(!s.engine().can_undo());
    settle(&mut s);
    assert_eq!(s.renderer_mut().readback_srgb_rgba8().unwrap(),before);
}
