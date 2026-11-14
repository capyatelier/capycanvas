use super::*;
use layer_render::ViewState;

pub(crate) const EXTENT: [u32; 2] = [2560, 2048];

pub(crate) fn color(x: u32, y: u32, seed: u8) -> [u8; 4] {
    [20 + seed + (x / PAGE_SIZE) as u8 * 13, 30 + (y / PAGE_SIZE) as u8 * 11,
        40 + seed + ((x / PAGE_SIZE + 10 * (y / PAGE_SIZE)) % 160) as u8, 255]
}

pub(crate) fn photo(seed: u8) -> Document {
    let mut doc = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Cache progress".into(), paper: "Paper".into() });
    let ink = doc.scene().order()[0];
    set_root_entries(&mut doc, vec![ink]);
    composition_mut(&mut doc).color.depth = SampleDepth::F32;
    composition_mut(&mut doc).blend = layer_core::BlendSpace::Linear;
    paint_mut(&mut doc, 0).base = Some(PaintBase::new(rgba8_source(EXTENT, |x, y| color(x, y, seed)).into()));
    doc
}

fn present(r: &WgpuRasterizer, presenter: &mut ViewportPresenter, view: ViewState) -> Vec<[f32; 4]> {
    let (texture, target) = create_target(&r.device, [view.width_px, view.height_px], wgpu::TextureFormat::Rgba32Float, "source progress presentation");
    presenter.present(r, &target, view, [0.; 4]).unwrap();
    pixels(r, &texture)
}

#[test]
fn scene_page_output_binding_survives_frames_and_both_compute_layouts() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for watercolor_first in [false, true] {
        let mut scene = Scene::new(&r).unwrap();
        let slot = scene.alloc(&r, wgpu::Color::WHITE).unwrap();
        let layout = if watercolor_first { &r.pipelines.watercolor_compute.0 } else { &r.scene_pipelines.constant.0 };
        let accepted = scene.pool[slot].scene_output(&r, layout).clone();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
        scene.encode_jobs(&mut r, &mut encoder).unwrap();
        for _ in 0..3 { scene.begin_frame(); }
        let target = scene.pool[slot].view.clone();
        scene.jobs.push(Job::Clear(target.clone(), wgpu::Color::TRANSPARENT));
        scene.jobs.push(Job::Watercolor { coordinates: (r.target_bind_group.clone(), 0), target,
            views: std::iter::repeat_n(r.empty_view.clone(),5).chain(std::iter::repeat_n(r.empty_scalar_view.clone(),5)).collect(), record: 0 });
        pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
        scene.encode_jobs(&mut r, &mut encoder).unwrap();
        assert_eq!(scene.pool[slot].scene_output(&r, &r.scene_pipelines.constant.0), &accepted);
        assert!(scene.output_bindings.entries.is_empty(), "page bindings have one owner");
        let (_texture, external) = create_color_target(&r.device, [PAGE_SIZE; 2], "external binding target");
        scene.jobs.push(Job::Clear(external.clone(), wgpu::Color::WHITE));
        pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
        scene.encode_jobs(&mut r, &mut encoder).unwrap();
        assert!(scene.output_bindings.entries.contains_key(&external), "external images retain the bounded fallback");
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        crate::test_support::complete(&r);
    }
}

fn deferred_object_document()->Document {
    let extent=[1024,512];
    let mut artwork = Artwork::new(extent).unwrap();
    let mut object = ImageObject::new(layer_core::authored::Image::new(rgba8_source([512; 2], |x, y|
        [40 + (x / 32) as u8 * 9, 30 + (y / 32) as u8 * 11, 150, 255])));
    object.affine = Affine64([1., 0., 0., 1., 512., 0.]);
    object.interpolation = ImageInterpolation::Nearest;
    let object = artwork.objects.insert(PortableId::random(), object).unwrap();
    let owner = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Objects(object), "Image")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries = vec![owner];
    let mut doc = Document::from_artwork(artwork).unwrap();
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    doc
}

#[test]
fn deferred_fine_objects_keep_committed_coarse_pages_without_backup() {
    let extent=[1024,512];
    let mut doc=deferred_object_document();
    let coarse=ViewState {width_px:256,height_px:128,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let fine=ViewState {width_px:640,height_px:384,document_to_surface:[1.,0.,0.,1.,-128.,-64.]};
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let coarse_frame=FramePacket {view:coarse,..packet(doc.scene(),extent)};
    crate::memory::memory_tests::legacy_scene_frame(&mut r,coarse_frame);
    crate::test_support::submit_settled(&mut r,coarse_frame);
    let mut presenter=crate::test_support::float_presenter(&r);
    let expected=present(&r,&mut presenter,fine);
    let retained=r.scale_display.as_ref().unwrap().pages.clone();
    let coarse_pages=retained.lock().unwrap().retained_pages();
    assert!(!coarse_pages.is_empty());
    let frame=FramePacket {view:fine,composite_all:false,..packet(doc.scene(),extent)};
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    let request=request(&r,frame).unwrap();assert_eq!(request.plan.level,0);
    let (cache,_)=Cache::select(r.scale_display.take(),&r,frame,request,true);
    assert!(Arc::ptr_eq(&cache.pages,&retained));
    r.scale_display=Some(cache);r.moving_images=Default::default();
    let mut scene=r.scene.take().unwrap();scene.object_results.clear();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let result=scene.compose(&mut r,frame,PixelRect::EMPTY,&mut encoder,None,&mut Scene::begin_work(false));
    assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    r.scene=Some(scene);r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    assert!(r.has_presentable_frame(),"a pending fine window must retain its committed coarse fallback");
    assert_eq!(present(&r,&mut presenter,fine),expected,"current-camera presentation must not expose unwritten fine pages");
    assert_eq!(presenter.presented_camera(),Some(fine));
    let written=r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages();
    assert!(written.iter().any(|page|!page.current.is_empty()));
    occurrence_mut(&mut doc,0).opacity=0.5;
    let frame=FramePacket {view:fine,composite_all:false,..packet(doc.scene(),extent)};
    let mut cache=r.scale_display.take().unwrap();let mut scene=r.scene.take().unwrap();
    scene.scale_sources.prepare(&r,frame,&[]);prepare_fixture_frame(&r,frame,&scene);
    cache.prepare_graph(&r,frame,&scene).unwrap();
    let stale=cache.pages.lock().unwrap().retained_pages();
    assert!(written.iter().all(|before|stale.iter().any(|after|after.key==before.key && after.written==before.written)),
        "authored changes preserve the complete written fallback coverage");
    let coverage=cache.coverage_snapshot();
    r.scale_display=Some(cache);r.scene=Some(scene);
    assert!(!coverage.written.is_empty() && coverage.matching.is_empty() && coverage.exact.is_empty());
    assert!(r.has_presentable_frame(),"authored changes keep the committed fallback until replacement");
    assert_eq!(present(&r,&mut presenter,fine),expected);
    {
        let cache=r.scale_display.as_ref().unwrap();
        let key=cache.page_key(cache.graph.root.as_ref().unwrap().clone(),PixelRect::full([PAGE_SIZE;2]));
        let area=key.plan().bounds;
        assert!(cache.pages.lock().unwrap().get(&key,area,pages::Accuracy::Exact,0,crate::residency::Class::Visible).is_none(),"stale fallback cannot satisfy an exact dependency");
        let mut abandoned=pages::Store::default();
        abandoned.set_expected(cache.pages.lock().unwrap().expected().unwrap().clone());
        let image=abandoned.claim(&r,key.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let write=abandoned.prepare_write(key.clone(),area,pages::Accuracy::Exact,0).unwrap();
        {
            let attachments=[Some(attachment(&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)))];
            let _pass=encoder.begin_render_pass(&descriptor(&attachments));
        }
        abandoned.commit(write,0,&mut encoder);
        drop(encoder);
        assert!(abandoned.get(&key,area,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_none(),"abandoned page writes must not publish coverage");
        assert!(abandoned.retained_pages().is_empty());
        let replacement=pages::Key {expression:Arc::new(graph::Expression::Color([0;4])),..key};
        assert!(cache.pages.lock().unwrap().get(&replacement,area,pages::Accuracy::Exact,0,crate::residency::Class::Visible).is_none(),"a different content key cannot resurrect the previous drawing's pages");
    }
    crate::test_support::submit_settled(&mut r,frame);
    assert!(!crate::scene::scale::tests::publication_pending(&r));
    let mut reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    crate::test_support::submit_settled(&mut reference,frame);
    assert_eq!(display_pixels(&r),display_pixels(&reference),"settling restores the exact fine output");
}

#[test]
fn reduced_photo_larger_than_decoded_cache_publishes_only_after_prefixes_finish() {
    let doc = photo(0);
    let mut previous = doc.clone();
    paint_mut(&mut previous, 0).base = Some(PaintBase::new(rgba8_source([2048; 2], |x, y| color(x, y, 27)).into()));
    let mut r = crate::memory::memory_tests::renderer_for_restore(&previous);
    assert_ne!(r.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let view = ViewState { width_px: 320, height_px: 256, document_to_surface: [0.125, 0., 0., 0.125, 0., 0.] };
    let previous_frame = FramePacket { view, blend_space: layer_core::BlendSpace::Linear, ..packet(previous.scene(), EXTENT) };
    crate::memory::memory_tests::declare_source_budget(&mut r, previous_frame, 64, 16 << 20, 44 << 20);
    assert!(r.source_tiles.borrow().free_slots() >= 64);
    crate::test_support::submit_settled(&mut r, previous_frame);
    let mut presenter = crate::test_support::float_presenter(&r);
    present(&r, &mut presenter, view);
    let published = present(&r, &mut presenter, view);
    let presented_storage = |r: &WgpuRasterizer| r.presentation_views().map(|views|
        views.map(|view| (view.texture().format(), view.texture().size(), view.texture().mip_level_count())));
    let previous_storage = presented_storage(&r);
    let target = source_at(&doc, 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut prefix = 0;
    let mut partial = false;
    let mut settled = false;
    let mut presentation_change = None;
    for step in 0..256 {
        assert!(std::time::Instant::now() < deadline, "photo progress timed out at {prefix}/80 pages");
        r.poll_pending(view).unwrap();
        let frame = FramePacket { view, composite_all: step == 0, blend_space: layer_core::BlendSpace::Linear, ..packet(doc.scene().with_owner(doc.owner, 1), EXTENT) };
        crate::memory::memory_tests::declare_source_budget(&mut r, frame, 64, 16 << 20, 44 << 20);
        if r.can_submit() && r.raster_dependencies_ready(frame, None) { crate::test_support::submit_ready(&mut r, frame).unwrap(); }
        r.wait_idle().unwrap();
        if let Some(cache) = r.scale_display.as_ref() {
            let completed = source_levels(cache,target).values().map(BTreeSet::len).max().unwrap_or(0);
            assert!(completed >= prefix, "completed source pages regressed");
            prefix = completed;
            partial |= prefix > 0 && prefix < 80;
        }
        if crate::scene::scale::tests::publication_pending(&r) {
            let shown = present(&r, &mut presenter, view);
            assert_eq!(shown.len(), published.len());
            if presentation_change.is_none() && let Some((index, (actual, expected))) = shown.iter().zip(&published).enumerate().find(|(_, (a, b))| a != b) {
                eprintln!("retained presentation storage: before={previous_storage:?} pending={:?}", presented_storage(&r));
                presentation_change = Some((step, prefix, index, *actual, *expected));
            }
        }
        if r.next_frame()!=layer_render::FrameDemand::Work && !r.has_pending_submission() { settled = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let levels = r.scale_display.as_ref().map(|cache|source_levels(cache,target).into_iter().map(|(level,chunks)|(level,chunks.len())).collect::<Vec<_>>());
    eprintln!("progress display level={:?} size={:?} source_levels={levels:?}", r.scale_display.as_ref().map(|cache| cache.plan.level), r.scale_display.as_ref().map(|cache| cache.plan.size));
    assert!(settled && partial && prefix == 80, "source prefixes must finish: settled={settled} partial={partial} pages={prefix}");
    eprintln!("source progress: settled={settled} partial={partial} pages={prefix} pending_presentation_change={presentation_change:?}");
    let actual = display_pixels(&r);
    let plan = r.scale_display.as_ref().unwrap().plan;
    assert_eq!((plan.level, plan.size), (3, [320, 256]));
    assert_eq!(actual.len(), 320 * 256);
    for (index, pixel) in actual.iter().enumerate() {
        let rgba = color((index as u32 % 320) * 8, (index as u32 / 320) * 8, 0);
        for c in 0..4 {
            let expected = if c == 3 { 1. } else { doc.composition().color.space.decode(f64::from(rgba[c]) / 255.) as f32 };
            assert!((pixel[c] - expected).abs() < 2e-6, "reduced pixel {index} channel {c}: {pixel:?} vs {expected}");
        }
    }
    assert!(presentation_change.is_none(), "pending source preparation replaced the published frame: {presentation_change:?}");
}

#[test]
fn deferred_composition_adopts_raster_damage_once() {
    let extent=[1;2];
    let mut doc=Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Reference".into(),paper:"Paper".into()});
    let target=doc.scene().order()[0];
    let reference=paint_occurrence(&mut doc,"Reference",Some(rgba8_source(extent,|x,y|[8*x as u8,8*y as u8,80,192])));
    set_root_entries(&mut doc,vec![reference,target]);
    let view=ViewState {width_px:1,height_px:1,document_to_surface:[0.5,0.,0.,0.5,0.,0.]};
    let mut r=crate::memory::memory_tests::renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,budget_bytes:Some(192<<20),..Default::default()}).unwrap();
    crate::test_support::submit_settled(&mut r,FramePacket {view,..packet(doc.scene(),extent)});
    let SourceTarget::Paint(reference)=doc.scene().source_target(reference).unwrap() else {unreachable!()};
    doc.artwork.paint.get_mut(reference).unwrap().base=Some(PaintBase::new(rgba8_source(extent,|x,y|[8*x as u8,8*y as u8,120,192]).into()));
    let source=doc.scene().source_target(target).unwrap();let SourceTarget::Paint(paint)=source else {unreachable!()};
    let coverage=CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),extent,[0;2]);
    doc.artwork.paint.get_mut(paint).unwrap().operations=Arc::new(vec![layer_core::RasterOperation {placement:layer_core::Affine::IDENTITY,coverage,
        kind:layer_core::RasterOperationKind::Fill {color:[0.,0.,0.5,0.5],texture:Default::default(),alpha_locked:false,edges:None,underfill:None}}]);
    doc.artwork.paint.get_mut(paint).unwrap().raster=layer_core::raster::RasterRevision::pending();
    let mut stored=crate::layer_tests::batch(source);stored.kind=DabBatchKind::RasterOperation(0);stored.dab_count=0;stored.damage=layer_core::Rect::from_extent(extent);
    let frame=FramePacket {view,dab_batches:std::slice::from_ref(&stored),..packet(doc.scene().with_owner(doc.owner,1),extent)};
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    assert!(crate::scene::scale::tests::publication_pending(&r),"the changed reference must defer composition after the fill's mips were prepared");
    let input=&r.scene.as_ref().unwrap().scale_sources.entries[&source];
    assert!(!r.transform_damage.is_empty());
    let updates=input.updates;let valid:usize=source_levels(r.scale_display.as_ref().unwrap(),source).values().map(BTreeSet::len).sum();
    assert!(valid>0,"the first attempt prepared the fill source");
    let idle=FramePacket {composite_all:false,dab_batches:&[],..frame};
    crate::test_support::submit_settled(&mut r,idle);
    let input=&r.scene.as_ref().unwrap().scale_sources.entries[&source];
    let after_valid:usize=source_levels(r.scale_display.as_ref().unwrap(),source).values().map(BTreeSet::len).sum();
    let after_updates=input.updates;
    assert!(r.transform_damage.is_empty());
    let mut ample=doc.clone();ample.artwork.paint.get_mut(paint).unwrap().raster=layer_core::raster::RasterRevision::pending();
    let mut control=crate::memory::memory_tests::legacy_scene_renderer(ample.composition().color);
    crate::test_support::submit_settled(&mut control,FramePacket {scene:ample.scene().with_owner(ample.owner,1),..frame});
    let actual=display_pixels(&r);let expected=display_pixels(&control);assert_eq!(actual.len(),expected.len());
    for (actual,expected) in actual.iter().zip(expected) {for channel in 0..4 {
        assert!((actual[channel]-expected[channel]).abs()<2e-6,"{actual:?} != {expected:?}");
    }}
    assert!(after_updates<=updates+after_valid.saturating_sub(valid) as u64,"the continuation re-staled prepared source mips: {updates}->{after_updates}, valid {valid}->{after_valid}");
}

#[test]
fn fill_preview_becomes_ready_after_deferred_object_composition() {
    let extent=[1024,512];
    let mut doc=deferred_object_document();
    let (_,target)=crate::test_support::add_paint(&mut doc.artwork,"Fill preview",extent);
    reindex(&mut doc);
    let coarse=ViewState {width_px:256,height_px:128,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let fine=ViewState {width_px:640,height_px:384,document_to_surface:[1.,0.,0.,1.,-128.,-64.]};
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    crate::test_support::submit_settled(&mut r,FramePacket {view:coarse,..packet(doc.scene(),extent)});
    let authored=doc.artwork.clone();
    let frame=FramePacket {view:fine,composite_all:false,..packet(doc.scene(),extent)};
    let request=request(&r,frame).unwrap();
    let (cache,_)=Cache::select(r.scale_display.take(),&r,frame,request,true);
    r.scale_display=Some(cache);r.moving_images=Default::default();
    r.scene.as_mut().unwrap().object_results.clear();
    let operation=layer_core::RasterOperation {placement:layer_core::Affine::IDENTITY,
        coverage:CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),extent,[0;2]),
        kind:layer_core::RasterOperationKind::Fill {color:[1.,0.,0.,1.],texture:Default::default(),alpha_locked:false,edges: None, underfill:None}};
    r.set_fill_preview(Some(layer_render::FillPreview {transaction:91,revision:3,target,operation}));
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    r.wait_idle().unwrap();
    assert!(crate::scene::scale::tests::publication_pending(&r),"cold fine objects must defer the first preview composition");
    assert_eq!(r.fill_preview_ready(),None);
    let idle=FramePacket {composite_all:false,reset_layers:false,..frame};
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
    while crate::scene::scale::tests::publication_pending(&r) {
        assert!(std::time::Instant::now()<deadline,"fine object composition must publish");
        crate::test_support::submit_ready(&mut r,idle).unwrap();
        r.wait_idle().unwrap();
    }
    assert_eq!(r.fill_preview_ready(),Some((91,3)));
    assert_eq!(doc.artwork,authored);
    r.set_fill_preview(None);
    crate::test_support::submit_settled(&mut r,idle);
    assert_eq!(r.fill_preview_ready(),None);
}

#[test]
fn an_empty_store_rebuilds_without_a_cache_readiness_flag() {
    let doc=deferred_object_document();let extent=doc.composition().size;
    let view=ViewState {width_px:256,height_px:128,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let frame=FramePacket {view,..packet(doc.scene(),extent)};
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::test_support::submit_settled(&mut r,frame);
    let mut cache=r.scale_display.take().unwrap();
    assert_eq!(cache.plan.bounds,PixelRect::full(extent));
    assert!(cache.pixels.committed_root().is_some());
    reset_page_store(&mut cache);
    assert!(cache.coverage_snapshot().written.is_empty());
    let mut scene=r.scene.take().unwrap();let mut commands=scene.scale_commands.take().unwrap();
    scene.begin_frame();commands.begin();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.render_coarse_preview(&mut scene,&mut r,frame,PixelRect::full(extent),
        &mut Encoding {encoder:&mut encoder,commands:&mut commands, retained:0, budget:&mut Scene::begin_work(false)},None).unwrap();
    scene.encode_jobs(&mut r,&mut encoder).unwrap();r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    assert!(cache.coverage_complete(),"ready without valid coverage must rebuild the coarse preview");
}
