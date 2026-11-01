use super::*;
use crate::test_support::submit_settled;
use layer_render::RegionPreparation;

#[test]
fn region_cache_prewarm_pauses_for_contact_resumes_idle_and_promotes_demand() {
    let extent=[64;2];
    let alpha=(0..extent[0]*extent[1]).map(|i|f32::from(barrier([12,12,52,52],i%extent[0],i/extent[0]))).collect::<Vec<_>>();
    let document=uniform_document(extent,&alpha);
    let source=RegionSource::Scene {snapshot:document.snapshot(),scope:layer_core::SceneScope::Raw(target(&document))};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit_settled(&mut r,packet(document.scene(),extent));
    r.startup=Some(startup::Startup::new(&r.device).unwrap());
    r.startup.as_ref().unwrap().compiler.start();
    r.shader_idle(false,false);
    r.prepare_regions(Some(&RegionPreparation {source:source.clone(),reference_count:None,paper:None,tolerance:0.,enclosed:false}));
    assert!(r.regions.as_ref().unwrap().background_pending());
    assert!(!r.has_pending_work());
    let frame=r.artwork_frame.clone().unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    r.prefetch_regions(&frame,&mut encoder).unwrap();
    assert_eq!(encoder.pass_count(),0);
    assert!(r.regions.as_ref().unwrap().raw.mask().is_none());
    r.shader_idle(true,true);
    assert!(r.has_pending_work());
    r.prefetch_regions(&frame,&mut encoder).unwrap();
    r.shader_idle(false,false);
    assert!(!r.has_pending_work());
    let mut query=request(source,rectangle([0,0,1,1]),exact(),0.);
    query.enclosure=None;query.position=[32,32];
    check(&receive_request(&mut r,query),&rectangles(extent,&[[13,13,51,51]]),extent);
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
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: Some(2), paper: None, tolerance: 0., enclosed: false }));
    assert!(r.regions.is_none());
    frame(&mut r, &document, extent);
    r.set_refine_chunk(Some(1_048_576.));
    for enclosed in [false, true] {
        let mut preparation = RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., enclosed };
        r.prepare_regions(Some(&preparation));
        assert!(r.regions.as_ref().unwrap().background_pending());
        submit_ready(&mut r, packet(document.scene(), extent)).unwrap();
        assert!(r.regions.as_ref().unwrap().background_pending());
        submit_settled(&mut r, packet(document.scene(), extent));
        assert!(!r.regions.as_ref().unwrap().background_pending());
        let mut query = request(source.clone(), rectangle([10, 10, 503, 247]), exact(), 0.);
        query.position = [100, 100];
        if !enclosed { query.enclosure = None; }
        assert!(r.request_region(query.clone()).unwrap());
        assert!(!r.regions.as_ref().unwrap().background_pending(), "a prepared tap must reuse its reference geometry");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let result = loop {
            if let Some(result) = r.take_region() { break result.unwrap(); }
            assert!(std::time::Instant::now() < deadline, "prepared region readback timed out");
            std::thread::yield_now();
        };
        assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
        score_geometry(&result.pixels, &expected, [0, 0], &[], "prepared region").unwrap();
        preparation.tolerance = 0.25;
        r.prepare_regions(Some(&preparation));
        assert!(r.regions.as_ref().unwrap().background_pending(), "changed preparation must schedule work immediately");
        assert!(r.has_pending_work());
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
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., enclosed: false }));
    submit_ready(&mut r, packet(document.scene(), extent)).unwrap();
    assert!(r.regions.as_ref().unwrap().background_pending());
    let mut query = request(source, rectangle([0, 0, 1, 1]), exact(), 0.);
    query.enclosure = None; query.position = [100, 100]; query.request_id = 90;
    assert!(r.request_region(query).unwrap());
    assert!(r.region_pending());
    r.prepare_regions(Some(&RegionPreparation { source: RegionSource::Composite, reference_count: Some(2), paper: None, tolerance: 0., enclosed: false }));
    assert!(r.region_pending());
    r.cancel_region();
    assert!(!r.region_pending());
    assert!(r.take_region().is_none());
    let source = RegionSource::Scene { snapshot, scope: layer_core::SceneScope::Raw(second) };
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., enclosed: false }));
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
    r.prepare_regions(Some(&RegionPreparation {source:RegionSource::Composite,reference_count:None,paper:None,tolerance:0.,enclosed:false}));
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
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., enclosed: false }));
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
    r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., enclosed: false }));
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
        r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: Some(1), paper: None, tolerance: 0., enclosed: false }));
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
fn region_cache_sparse_overflow_preserves_seals_across_blocks_and_warm_requests() {
    let extent = [1280, 1152];
    let expected = [vec![[1061., 129.], [1239., 129.], [1239., 299.], [1061., 299.]]];
    let mut control = Vec::new();
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
        r.prepare_regions(Some(&RegionPreparation { source: source.clone(), reference_count: None, paper: None, tolerance: 0., enclosed: false }));
        submit_settled(&mut r, packet(document.scene(), extent));
        assert!(!r.regions.as_ref().unwrap().background_pending());
        assert_eq!(r.regions.as_ref().unwrap().flood.needs_seals(true), dense);
        assert!(!r.regions.as_ref().unwrap().flood.needs_seals(false));
        for (index, close_gaps) in [true, false, true].into_iter().enumerate() {
            let mut query = request(source.clone(), rectangle([0, 0, 1, 1]), RegionRefinement { close_gaps, ..exact() }, 0.);
            query.enclosure = None;
            query.position = [1150, 210];
            let result = receive_request(&mut r, query);
            assert_eq!((result.request_id, result.placement, result.pixels.extent()), (91, Affine::IDENTITY, extent));
            assert!(!r.regions.as_ref().unwrap().background_pending());
            assert_eq!(value(&result.pixels, 1150, 210), 255);
            assert_eq!(value(&result.pixels, 1270, 400), if close_gaps { 0 } else { 255 });
            if close_gaps {
                assert!(!r.regions.as_ref().unwrap().flood.needs_seals(true));
                score_geometry(&result.pixels, &expected, [0, 0], &[], "overflow contour").unwrap();
            }
            let pixels = (0..extent[1]).flat_map(|y| (1056..extent[0]).map(move |x| (x, y)))
                .map(|(x, y)| value(&result.pixels, x, y)).collect::<Vec<_>>();
            if dense { assert_eq!(pixels, control[index]); } else { control.push(pixels); }
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
        reference_count: None, paper: document.nearest_paper_binding(document.drawing_content()), tolerance: 0., enclosed: false };
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
