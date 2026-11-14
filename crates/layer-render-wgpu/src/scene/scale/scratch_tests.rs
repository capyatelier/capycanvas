use super::*;
use layer_render::{CanvasRenderer, ViewState};

#[test]
fn deferred_display_graph_releases_all_scratch_leases() {
    let extent = [512; 2];
    let mut artwork = Artwork::new(extent).unwrap();
    let mut entries = Vec::new();
    for seed in 0..5 {
        let image = layer_core::authored::Image::new(rgba8_source(extent, |x,y|
            [30 + seed * 17, (x % 251) as u8, (y % 239) as u8, 200]));
        let object = artwork.objects.insert(PortableId::random(), ImageObject::new(image)).unwrap();
        entries.push(artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Objects(object), "Photo")).unwrap());
    }
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries = entries;
    let doc = Document::from_artwork(artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = ViewState { width_px: 256, height_px: 256, document_to_surface: [0.5,0.,0.,0.5,0.,0.] };
    let mut deferred = false;
    for attempt in 0..24 {
        r.poll_pending(view).unwrap();
        r.submit(FramePacket { view, composite_all: attempt == 0, ..packet(doc.scene(), extent) }).unwrap();
        r.wait_idle().unwrap();
        deferred |= crate::scene::scale::tests::publication_pending(&r);
        let scene = r.scene.as_ref().unwrap();
        assert!(scene.used.iter().all(|usage| !usage.occupied()), "attempt {attempt} retained abandoned reservations");
        let required = r.scale_display.as_ref().map_or(0, |cache|cache.scratch_images());
        assert!(scene.pool.len() <= required + 2, "attempt {attempt} accumulated scratch across frames");
    }
    assert!(deferred, "fixture must exercise an unavailable object dependency");
    crate::test_support::submit_settled(&mut r, FramePacket {view, composite_all:false, ..packet(doc.scene(),extent)});
    assert!(!crate::scene::scale::tests::publication_pending(&r));
    assert!(r.scene.as_ref().unwrap().used.iter().all(|usage| !usage.occupied()));
}

#[test]
fn scratch_cap_and_idle_trim_preserve_leased_pages_and_format_reuse() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r).unwrap();
    let limit = 2 * u64::from(PAGE_SIZE).pow(2) * 16;
    let base = scene.tiers.gpu.used();
    scene.tiers.gpu.update_limit(base + limit);
    let first = scene.reserve(&r).unwrap();
    let first = scene.own(first);
    let second = scene.reserve(&r).unwrap();
    let second = scene.own(second);
    assert!(matches!(scene.reserve(&r), Err(GpuRasterError::NotReady)));
    let texture = scene.pool[first.index].texture.clone();
    let reusable = second.index;
    drop(second);
    let reused = scene.reserve(&r).unwrap();
    assert_eq!(reused, reusable);
    scene.free(reused);
    assert!(matches!(scene.reserve_format(&r, true), Err(GpuRasterError::NotReady)));
    let retired = scene.trim_scratch(1);
    assert_eq!(retired.len(), 1);
    assert!(matches!(scene.reserve_format(&r, true), Err(GpuRasterError::NotReady)), "a retiring page stays charged until it drops");
    drop(retired);
    let scalar = scene.reserve_format(&r, true).unwrap();
    let scalar = scene.own(scalar);
    assert!(scene.pool_bytes() <= limit);
    assert!(scene.trim_scratch(0).is_empty());
    drop(scalar);
    let retired = scene.trim_scratch(0);
    assert_eq!(retired.len(), 1);
    assert_eq!(scene.pool[first.index].texture, texture);
    drop(first);
    assert_eq!(scene.trim_scratch(0).len(), 1);
}

#[test]
fn display_scratch_rejects_an_unadmitted_image_before_allocation() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let doc = Document::new(PortableId::random(), 512, 512, layer_core::DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    let frame = packet(doc.scene(), [512;2]);
    let plan = display_mips::Plan::at([512;2], 0);
    let request = Request {plan, evaluation:Evaluation::Native};
    let (mut cache,_) = Cache::select(None, &r, frame, request, false);
    let mut oversized = plan;
    oversized.size = [8192;2];
    assert!(matches!(cache.allocate(&r, oversized), Err(GpuRasterError::NotReady)));
    assert!(cache.output.is_empty());
}

#[test]
fn a_declaration_takes_its_placeholder_scratch_without_an_admission() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r).unwrap();
    scene.tiers.gpu.update_limit(scene.tiers.gpu.used());
    let mut reached = false;
    scene.declare(&mut r, crate::scene::sources::Inputs::default(), crate::scene::sources::Declaration::raw(None, None, None), |scene, r, _| {
        scene.reserve(r)?;
        reached = true;
        Ok(())
    }).unwrap();
    assert!(reached, "a refused placeholder ends the declaration early, and its reads go undeclared");
}

#[test]
fn held_composition_scratch_is_pinned_in_the_display_report() {
    let r = crate::memory::memory_tests::legacy_scene_renderer(Default::default());
    let doc = Document::new(PortableId::random(), 512, 512, layer_core::DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    let frame = packet(doc.scene(), [512;2]);
    let plan = display_mips::Plan::at([512;2], 0);
    let (mut cache,_) = Cache::select(None, &r, frame, Request {plan, evaluation:Evaluation::Native}, false);
    let before = r.current_display_memory_report().pinned_gpu_bytes;
    let lease = cache.allocate(&r, plan).unwrap();
    let bytes = cache.output[lease.index].bytes();
    assert!(r.current_display_memory_report().pinned_gpu_bytes >= before + bytes, "scratch admitted above the grant must stay covered once its overdraft ends");
    drop(lease);
}

#[test]
fn raster_only_native_stack_resumes_after_scratch_cap_deferral() {
    let extent = [517,259];
    let mut doc = document_at(extent);
    let front = paint_occurrence(&mut doc, "Translucent paint", Some(rgba8_source(extent, |x,y|
        [(x % 251) as u8, 70, (y % 239) as u8, 127])));
    insert_occurrence(&mut doc, front, 0);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.test.reference = true;
    let frame = packet(doc.scene(), extent);
    crate::test_support::submit_settled(&mut r, frame);
    let expected = pixels(&r, crate::test_support::document_texture(&r));
    assert!(matches!(r.scale_display.as_ref().unwrap().evaluation, Evaluation::Native));
    drop(r.scene.as_mut().unwrap().trim_scratch(0));
    assert!(r.scene.as_ref().unwrap().pool.is_empty());
    let committed = r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages();
    let committed_pixels: Vec<_> = committed.iter().map(|page| pixels(&r, &page.texture)).collect();
    let gpu = r.scene_tiers.gpu.clone();
    gpu.update_limit(gpu.used());
    r.submit(frame).unwrap();
    assert!(crate::scene::scale::tests::publication_pending(&r), "the raster-only stack must hit the scratch byte cap");
    let pending = r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages();
    assert_eq!(pending.len(), committed.len());
    for ((before, after), expected) in committed.iter().zip(&pending).zip(committed_pixels) {
        assert!(before.key == after.key);
        assert_eq!(before.written, after.written);
        assert_eq!(before.current, after.current);
        assert_eq!(before.exact, after.exact);
        assert_eq!(pixels(&r, &after.texture), expected);
    }
    assert!(matches!(r.scale_display.as_ref().unwrap().evaluation, Evaluation::Native));
    let scene = r.scene.as_ref().unwrap();
    assert!(scene.used.iter().all(|usage| !usage.occupied()));
    assert!(scene.jobs.is_empty());
    r.wait_idle().unwrap();
    gpu.update_limit(u64::MAX);
    crate::test_support::submit_settled(&mut r, FramePacket {composite_all:false, ..frame});
    assert!(!crate::scene::scale::tests::publication_pending(&r));
    assert!(r.has_presentable_frame());
    assert_eq!(pixels(&r, crate::test_support::document_texture(&r)), expected);
}

#[test]
fn exhausted_overview_scratch_retires_incompatible_capacity_before_resuming() {
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(Default::default());
    let doc = document_at([512;2]);
    let frame = packet(doc.scene(), [512;2]);
    crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
    let plan = display_mips::Plan::at([512;2], 0);
    let (mut cache,_) = Cache::select(None, &r, frame, Request {plan,evaluation:Evaluation::Native}, false);
    let page_bytes = u64::from(PAGE_SIZE).pow(2) * 16;
    let held = r.retired_display_bytes.charge(crate::memory::CACHE_BYTES - 2 * page_bytes);
    let mut small = plan;
    small.size = [PAGE_SIZE;2];
    drop(cache.allocate(&r, small).unwrap());
    let mut next = small;
    next.size[0] *= 2;
    assert!(matches!(cache.allocate(&r, next), Err(GpuRasterError::NotReady)));
    let (mut outer,_) = Cache::select(None, &r, frame, Request {plan,evaluation:Evaluation::Native}, false);
    outer.overview = Some(Box::new(cache));
    r.scale_display = Some(outer);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device,&Default::default());
    r.retire_display_resources(&mut encoder, true);
    assert!(r.scale_display.as_ref().unwrap().overview.as_ref().unwrap().output.is_empty());
    assert_eq!(r.retired_display_bytes.used(), crate::memory::CACHE_BYTES - page_bytes);
    let mut cache = r.scale_display.take().unwrap().overview.take().unwrap();
    assert!(matches!(cache.allocate(&r, next), Err(GpuRasterError::NotReady)));
    r.last_submission = encoder.submit(&r.queue);
    r.wait_idle().unwrap();
    assert_eq!(r.retired_display_bytes.used(), crate::memory::CACHE_BYTES - 2 * page_bytes);
    assert!(cache.allocate(&r, next).is_ok());
    drop(held);
}
