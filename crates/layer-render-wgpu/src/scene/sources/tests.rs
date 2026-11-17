use super::*;
use layer_core::color::source::{SourceBuilder, SourceInterpretation};

fn standalone() -> (Arc<crate::residency::Tier>, DecodedTiles) {
    let display = Arc::new(crate::residency::Tier::default());
    let mut cache = DecodedTiles::within(&display, Default::default());
    cache.admit_limits(16 * NORMALIZED_PAGE_BYTES);
    (display, cache)
}
fn evict(display: &crate::residency::Tier, cache: &mut DecodedTiles, bytes: u64) -> u64 {
    display.heap().evict(bytes, crate::residency::Class::Fallback);
    cache.retire_evicted()
}


#[test]
fn raster_cache_keys_share_generated_contents_and_keep_loaded_owner_identity() {
    let source = scan_source(1, 0);
    let blob = source.tiles[&[0, 0]].clone();
    let independent = scan_source(1, 0);
    let replacement = independent.tiles[&[0, 0]].clone();
    assert_ne!(blob.owner_identity(), replacement.owner_identity());
    let key = Key::raster(&blob, RgbSpace::Srgb, RgbSpace::Srgb);
    assert!(key.matches(&Key::raster(&replacement, RgbSpace::Srgb, RgbSpace::Srgb)));
    let reinterpreted = TileBlob::encode(PixelDescriptor {
        alpha: AlphaAssociation::PremultipliedLinear, ..blob.descriptor
    }, &blob.decode_on_worker().unwrap()).unwrap();
    assert_eq!(&*reinterpreted.compressed_on_worker().unwrap(), &*blob.compressed_on_worker().unwrap());
    assert!(!key.matches(&Key::raster(&reinterpreted, RgbSpace::Srgb, RgbSpace::Srgb)));
    assert!(!key.matches(&Key::raster(&blob, RgbSpace::DisplayP3, RgbSpace::Srgb)));
    assert!(!key.matches(&Key::raster(&blob, RgbSpace::Srgb, RgbSpace::DisplayP3)));
    let loaded = |id| Arc::new(TileBlob::from_verified_resource(
        id, blob.descriptor, Arc::from(&*blob.compressed_on_worker().unwrap()),
    ).unwrap());
    let first = loaded(blob.resource_id());
    let second = loaded(blob.resource_id());
    let loaded_key = Key::raster(&first, RgbSpace::Srgb, RgbSpace::Srgb);
    assert!(loaded_key.matches(&Key::raster(&first.clone(), RgbSpace::Srgb, RgbSpace::Srgb)));
    assert!(!loaded_key.matches(&Key::raster(&second, RgbSpace::Srgb, RgbSpace::Srgb)));
    assert!(!loaded_key.matches(&key));
    let weak = Arc::downgrade(&blob);
    drop((source, blob));
    assert_eq!(weak.strong_count(), 0);
}

#[test]
fn equal_generated_rasters_reuse_gpu_pixels_without_another_sample_decode() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_factory(gpu.factory.clone(), gpu.device.clone(), Default::default()).unwrap();
    drop(gpu);
    let mut scene = Scene::new(&r).unwrap();
    let descriptor = r.document_color().paint_descriptor();
    let bytes = [64, 128, 192, 255].repeat((PAGE_SIZE * PAGE_SIZE) as usize);
    let first = Arc::new(TileBlob::encode(descriptor, &bytes).unwrap());
    let second = Arc::new(TileBlob::encode(descriptor, &bytes).unwrap());
    assert_ne!(first.owner_identity(), second.owner_identity());
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let original = scene.raster_tile_for_query(&mut r, &first, RgbSpace::Srgb, &mut encoder).unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let reused = scene.raster_tile_for_query(&mut r, &second, RgbSpace::Srgb, &mut encoder).unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    assert_eq!(original.view, reused.view);
    assert_eq!(r.source_cache_work(), [1, 1]);
    let samples = r.device.source_samples.stats();
    assert_eq!((samples.entries, samples.misses), (1, 1));
    assert_eq!(samples.resident_bytes, bytes.len());
    let expected = [64, 128, 192].map(|code| RgbSpace::Srgb.decode(f64::from(code) / 255.) as f32);
    for pixel in crate::test_support::float_pixels(&r, &reused.texture) {
        assert_eq!(pixel[3], 1.);
        for c in 0..3 { assert!((pixel[c] - expected[c]).abs() < 2e-6); }
    }
}

#[test]
fn borrowed_source_tiles_survive_eviction_and_release_their_capacity() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (display, mut cache) = standalone();
    let source = scan_source(4, 0);
    let key = |id: u32| Key::raster(&source.tiles[&[id - 1, 0]], RgbSpace::Srgb, RgbSpace::Srgb);
    let (first, first_write) = cache.plan_key(&r, key(1)).unwrap();
    let first_lease = cache.lease(&first.view).unwrap();
    let (second, second_write) = cache.plan_key(&r, key(2)).unwrap();
    let second_lease = cache.lease(&second.view).unwrap();
    assert_eq!(evict(&display, &mut cache, u64::MAX), 0, "leased slots are not victims");
    let (hit, write) = cache.plan_key(&r, key(1)).unwrap();
    assert_eq!(hit.view, first.view);
    assert!(write.is_none());
    drop(second_lease);
    let page = cache.slots[0].bytes();
    assert_eq!(evict(&display, &mut cache, u64::MAX), page, "a released slot becomes a victim");
    assert!(cache.lease(&second.view).is_none() && cache.lease(&first.view).is_some());
    let (third, third_write) = cache.plan_key(&r, key(3)).unwrap();
    assert_ne!(third.view, first.view);
    drop(first_lease);
    assert_eq!(evict(&display, &mut cache, u64::MAX), 2 * page);
    assert_eq!(cache.gpu_bytes(), cache.retired_bytes.used() + cache.non_slot_bytes());
    assert!(cache.slots.is_empty() && cache.views.is_empty() && cache.keys.is_empty());
    drop((first_write, second_write, third_write));
}

fn scan_source(tiles: usize, offset: u8) -> Arc<SourceImage> {
    let columns = (tiles as u32).min(128);
    layer_core::color::source::rgba8_source([columns * PAGE_SIZE, (tiles as u32).div_ceil(columns) * PAGE_SIZE], |x, y| {
        let id = (y / PAGE_SIZE * columns + x / PAGE_SIZE) as u8 + offset;
        [id.wrapping_mul(17), id.wrapping_mul(43).wrapping_add(61), id.wrapping_mul(97).wrapping_add(11), 255]
    })
}

fn lease_foreign_sources(r: &mut WgpuRasterizer, count: usize) -> (Option<Arc<SourceImage>>, Vec<crate::scene::sources::Lease>) {
    if count == 0 { return (None, Vec::new()); }
    let source = scan_source(count, 173);
    pollster::block_on(r.compilation.prepare_all(crate::startup::SourceKinds::for_source(&source).pipelines(&r.scene_pipelines.source))).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut leases = Vec::with_capacity(count);
    for x in 0..count as u32 {
        let prepared = crate::prepare_image_tile(&source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
        let tile=Scene::decode_prepared_for(r,&mut encoder,&source,[x,0],prepared,SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        assert_eq!(tile.texture.format(), wgpu::TextureFormat::Rgba32Float);
        leases.push(r.source_tiles.borrow().lease(&tile.view).unwrap());
    }
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    crate::test_support::complete(r);
    (Some(source), leases)
}

fn drained_source_renderer() -> (WgpuRasterizer, layer_core::MemoryReservation, Option<Arc<SourceImage>>, Vec<crate::scene::sources::Lease>, usize) {
    let (doc, ..) = threshold_document(threshold_source(1, 0));
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    let frame = crate::test_support::packet(doc.scene(), doc.composition().size);
    r.begin_frame_memory(frame);
    let (source, foreign) = lease_foreign_sources(&mut r, SOURCE_SLOTS / 2);
    r.publish_memory();
    budget.flush_reports();
    let pressure = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    r.begin_frame_memory(frame);
    let capacity = r.source_tiles.borrow().free_slots();
    let sources = r.source_tiles.borrow();
    let foreign_bytes = sources.slots.iter().filter(|slot| held_by(&foreign, slot))
        .map(|slot| page_bytes(slot.layer.format())).sum::<u64>();
    assert!(capacity as u64 <= (sources.decoded.headroom(crate::residency::Class::Visible) + sources.decoded.used() - foreign_bytes) / NORMALIZED_PAGE_BYTES);
    assert!(capacity > SOURCE_SLOTS);
    drop(sources);
    (r, pressure, source, foreign, capacity)
}

fn held_by(foreign: &[crate::scene::sources::Lease], slot: &Slot) -> bool {
    slot.pin.lock().unwrap().upgrade().is_some_and(|pin| {let pin: Arc<dyn Send + Sync> = pin; foreign.iter().any(|lease| Arc::ptr_eq(lease, &pin))})
}
fn assert_working_source_slots(r: &WgpuRasterizer, capacity: usize, foreign: &[crate::scene::sources::Lease]) {
    let sources = r.source_tiles.borrow();
    let retained = sources.slots.iter().filter(|slot| held_by(foreign, slot)).count();
    assert_eq!(retained, foreign.len(), "unrelated source leases survive the traversal");
    assert!(sources.slots.iter().filter(|slot| held_by(foreign, slot))
        .all(|slot| slot.valid.load(Ordering::Acquire) && slot.layer.format() == wgpu::TextureFormat::Rgba32Float));
    assert!(sources.slots.iter().filter(|slot| !held_by(foreign, slot)
        && slot.layer.format() == wgpu::TextureFormat::Rgba32Float).count() <= capacity);
    assert!(r.scene_tiers.display.used() <= r.scene_tiers.display.limit());
    assert_eq!(sources.gpu_bytes(), sources.slots.iter().map(|slot| page_bytes(slot.layer.format())).sum::<u64>()
        + sources.non_slot_bytes() + sources.retired_bytes.used());
    for (index, slot) in sources.slots.iter().enumerate() {
        if let Some(key) = &slot.key {
            assert!(sources.slots[index + 1..].iter().all(|other| other.key.as_ref().is_none_or(|other| !key.matches(other))));
        }
    }
}

fn assert_threshold_pixels(r: &WgpuRasterizer, threshold: f32, offset: u8) {
    let cache = r.scale_display.as_ref().unwrap();
    assert!(matches!(cache.plan.level, 0 | 2));
    let pixels = crate::test_support::float_pixels(r, cache.texture());
    for (i, actual) in pixels.into_iter().enumerate() {
        let id = (i as u32 % cache.plan.size[0] / (PAGE_SIZE >> cache.plan.level)) as u8 + offset;
        let codes = [id.wrapping_mul(17), id.wrapping_mul(43).wrapping_add(61), id.wrapping_mul(97).wrapping_add(11)];
        let luminance: f64 = codes.into_iter().zip(r.document_color().space.to_xyz()[1])
            .map(|(code, weight)| f64::from(code) / 255. * weight).sum();
        let expected = if luminance >= f64::from(threshold) { 1. } else { 0. };
        let x = i as u32 % cache.plan.size[0]; let y = i as u32 / cache.plan.size[0];
        let expected = if x < (4 >> cache.plan.level) && y < (4 >> cache.plan.level) {
            if cache.plan.level == 0 { ((x + y) & 1) as f32 } else { 0.5 }
        } else { expected };
        assert_eq!(actual, [expected, expected, expected, 1.], "offset={offset} threshold={threshold} pixel={i}");
    }
}

fn threshold_document(source: Arc<SourceImage>) -> (layer_core::Document, layer_core::authored::PaintHandle, layer_core::authored::EffectHandle, usize) {
    use layer_core::{Document, DocumentNames, EffectInstance};
    use layer_core::authored::*;
    let extent = source.extent;
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let SourceTarget::Paint(paint) = doc.working.target.unwrap() else { unreachable!() };
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(PaintBase::new(source.into()));
    let program = crate::tests::fixture("threshold").program();
    let parameter = program.parameters.iter().position(|parameter| parameter.key.as_ref() == "threshold").unwrap();
    let size = doc.composition().size;
    let effect = doc.artwork.effects.insert(PortableId::random(), EffectApplication::new(program.clone(), EffectInstance::new(program).values, size)).unwrap();
    let handle = doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "Threshold")).unwrap();
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0, handle);
    (Document::from_artwork(doc.artwork).unwrap(), paint, effect, parameter)
}

fn threshold_source(tiles: usize, offset: u8) -> Arc<SourceImage> {
    layer_core::color::source::rgba8_source([tiles as u32 * PAGE_SIZE, PAGE_SIZE], |x, y| {
        if x < 4 && y < 4 { let value = if (x + y) & 1 == 0 { 0 } else { 255 }; return [value, value, value, 255]; }
        let id = (x / PAGE_SIZE) as u8 + offset;
        [id.wrapping_mul(17), id.wrapping_mul(43).wrapping_add(61), id.wrapping_mul(97).wrapping_add(11), 255]
    })
}

#[test]
fn repeated_native_threshold_updates_reuse_an_oversized_decoded_working_set() {
    use layer_core::EffectValue;
    for numerator in [10, 13] {
        let (mut r, _pressure, _foreign_source, foreign, capacity) = drained_source_renderer();
        let tiles = capacity * numerator / 10 + 1;
        let extent = [tiles as u32 * PAGE_SIZE, PAGE_SIZE];
        let (mut doc, paint, effect, parameter) = threshold_document(threshold_source(tiles, 0));
        let mut view = crate::test_support::view([extent[0] / 4, extent[1] / 4]);
        view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        let mut allocated = None;
        for (round, threshold) in [0.2, 0.35, 0.5, 0.65, 0.8, 0.45].into_iter().enumerate() {
            doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(threshold);
            let frame = FramePacket {view, ..crate::test_support::packet(doc.scene(), extent)};
            r.begin_frame_memory(frame);
            let before = r.source_cache_work();
            crate::test_support::submit_published(&mut r, frame);
            let after = r.source_cache_work();
            let work = [after[0] - before[0], after[1] - before[1]];
            if round > 0 { assert!(work[0] >= capacity as u64, "warm traversal must reuse resident tiles: {work:?}"); }
            assert_threshold_pixels(&r, threshold, 0);
            let sources = r.source_tiles.borrow();
            if let Some(bytes) = allocated { assert_eq!(sources.gpu_bytes(), bytes); }
            else { allocated = Some(sources.gpu_bytes()); }
            drop(sources);
            assert_working_source_slots(&r, capacity, &foreign);
            let cache = r.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, 2);
            assert_eq!(cache.plan.bounds, PixelRect::full(extent));
        }
        let replacement = threshold_source(capacity, 64);
        doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((replacement).into()));
        doc.artwork.paint.get_mut(paint).unwrap().domain[0] = capacity as u32 * PAGE_SIZE;
        let composition = doc.artwork.root;
        doc.artwork.compositions.get_mut(composition).unwrap().size[0] = capacity as u32 * PAGE_SIZE;
        let frame = FramePacket {view, ..crate::test_support::packet(doc.scene(), doc.composition().size)};
        r.begin_frame_memory(frame);
        crate::test_support::submit_published(&mut r, frame);
        assert_threshold_pixels(&r, 0.45, 64);
        doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(0.6);
        let frame = FramePacket {view, ..crate::test_support::packet(doc.scene(), doc.composition().size)};
        r.begin_frame_memory(frame);
        let before = r.source_cache_work();
        crate::test_support::submit_published(&mut r, frame);
        let after = r.source_cache_work();
        assert_eq!(after[1], before[1], "a replacement set within capacity must be warm on its next update");
        assert_threshold_pixels(&r, 0.6, 64);
        assert_eq!(r.source_tiles.borrow().gpu_bytes(), allocated.unwrap());
        assert_working_source_slots(&r, capacity, &foreign);
    }
}

#[test]
fn discarded_source_decodes_cannot_be_reused_as_valid_pixels() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_factory(gpu.factory.clone(), gpu.device.clone(), Default::default()).unwrap();
    drop(gpu);
    let encoded_bytes = u64::from(PAGE_SIZE).pow(2) * u64::from(wgpu::TextureFormat::Rgba8Unorm.block_copy_size(None).unwrap());
    let source = scan_source(3, 32);
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let prepared = crate::prepare_image_tile(&source, [0, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
    Scene::decode_prepared(&mut r, &mut encoder, &source, [0, 0], prepared).unwrap();
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_some());
    drop(encoder);
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_none());
    for x in 0..2 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let prepared = crate::prepare_image_tile(&source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
        let raw = Scene::decode_prepared(&mut r, &mut encoder, &source, [x, 0], prepared).unwrap();
        assert_eq!(crate::resident::Storage::storage_bytes(&raw), encoded_bytes);
        let tile = Scene::promote_source(&mut r, &mut encoder, raw).unwrap();
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        let id = x as u8 + 32;
        let expected = [id.wrapping_mul(17), id.wrapping_mul(43).wrapping_add(61), id.wrapping_mul(97).wrapping_add(11)]
            .map(|code| RgbSpace::Srgb.decode(f64::from(code) / 255.) as f32);
        for pixel in crate::test_support::float_pixels(&r, &tile.texture) {
            assert_eq!(pixel[3], 1.);
            for c in 0..3 { assert!((pixel[c] - expected[c]).abs() < 2e-6); }
        }
    }
    assert_eq!(r.source_cache_work(), [0, 3]);
    let (_, pending) = r.source_tiles.borrow_mut().plan(&r, &source, [0, 0]).unwrap();
    assert!(pending.is_none());
    use layer_core::{Document, DocumentNames, authored::PortableId};
    let extent = [3 * PAGE_SIZE, PAGE_SIZE];
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let SourceTarget::Paint(paint) = doc.working.target.unwrap() else { unreachable!() };
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(source.clone().into()));
    let doc = Document::from_artwork(doc.artwork).unwrap();
    let capture = crate::artwork::Capture::default();
    let warm = PixelRect::full([PAGE_SIZE; 2]);
    let blocks = |region| capture.would_block(&r, crate::test_support::packet(doc.scene(), extent), region);
    crate::test_support::complete(&r);
    assert!(!blocks(warm));
    assert!(!blocks(PixelRect::EMPTY));
    let before = r.source_cache_work();
    let allocated = r.source_tiles.borrow().gpu_bytes();
    let pending = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let upload_bytes = r.source_tiles.borrow().in_flight.limit();
    assert_eq!(r.source_tiles.borrow().in_flight.used(), 0);
    let reservation = r.source_tiles.borrow().admit_upload(crate::residency::Class::Authored, upload_bytes).unwrap();
    assert_eq!(r.source_tiles.borrow().charge_upload(&pending, reservation, upload_bytes), upload_bytes);
    assert!(r.source_tiles.borrow().uploads_full(crate::residency::Class::Visible));
    assert!(blocks(warm));
    assert!(!blocks(PixelRect::EMPTY));
    let mut pages = [[2, 0], [0, 0], [1, 0]];
    crate::artwork::resident_first(&r, doc.scene(), &mut pages);
    assert_eq!(pages, [[0, 0], [1, 0], [2, 0]]);
    assert_eq!(r.source_cache_work(), before);
    assert_eq!(r.source_tiles.borrow().gpu_bytes(), allocated);
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_some());
    assert!(r.source_tiles.borrow().prepared_view(&source, [1, 0]).is_some());
    assert!(r.source_tiles.borrow().prepared_view(&source, [2, 0]).is_none());
    drop(pending);
    assert_eq!(r.source_tiles.borrow().in_flight.used(), 0);
    assert!(!r.source_tiles.borrow().uploads_full(crate::residency::Class::Visible));
    assert!(!blocks(warm));
}
#[test]
fn effect_mask_bindings_reuse_retire_and_bound_real_texture_views() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r).unwrap();
    let layout = scene.effects.masks.clone();
    let views = (std::array::from_fn::<_, { crate::effects::MASK_SLOTS }, _>(|_| r.empty_view.clone()), r.changed_cells.as_ref().unwrap().disabled.clone());
    let create = |key: &([wgpu::TextureView; crate::effects::MASK_SLOTS], wgpu::Buffer)| {
        let mut entries: Vec<_> = key.0.iter().enumerate().map(|(i, view)| wgpu::BindGroupEntry {
            binding: i as u32, resource: wgpu::BindingResource::TextureView(view),
        }).collect();
        entries.push(wgpu::BindGroupEntry { binding: crate::effects::MASK_SLOTS as u32, resource: key.1.as_entire_binding() });
        r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mask binding lifetime oracle"), layout: &layout, entries: &entries,
        })
    };
    let original = scene.mask_bindings.get(&views, || create(&views)).clone();
    scene.begin_frame();
    assert_eq!(*scene.mask_bindings.get(&views, || panic!("an unchanged mask set must reuse its binding")), original);
    let distinct_flags = (views.0.clone(), r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("distinct changed-cell binding"), size: 20, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false,
    }));
    assert_ne!(*scene.mask_bindings.get(&distinct_flags, || create(&distinct_flags)), original,
        "matching mask textures must not share bindings with a different changed-cell buffer");
    scene.forget_bindings(&[views.0[0].clone()]);
    assert!(scene.mask_bindings.entries.is_empty());
    assert_ne!(*scene.mask_bindings.get(&views, || create(&views)), original);
    for _ in 0..=4096 {
        let mut distinct = views.clone();
        distinct.0[0] = r.empty_view.texture().create_view(&Default::default());
        scene.mask_bindings.get(&distinct, || create(&distinct));
        assert!(scene.mask_bindings.entries.len() <= 4096);
    }
    scene.begin_frame();
    assert!(!scene.mask_bindings.entries.is_empty());
    scene.begin_frame();
    assert!(scene.mask_bindings.entries.is_empty(), "unused bindings must retire after one previous frame");
}

#[test]
fn source_residency_and_upload_window_follow_explicit_grants() {
    for uploads in [0, NORMALIZED_PAGE_BYTES - 1, NORMALIZED_PAGE_BYTES, 5 * NORMALIZED_PAGE_BYTES, 8 * NORMALIZED_PAGE_BYTES] {
        let (_, mut cache) = standalone();
        cache.admit_limits(uploads);
        let mut charges = Vec::new();
        while !cache.uploads_full(crate::residency::Class::Visible) {
            let bytes = if charges.len() % 2 == 0 { NORMALIZED_PAGE_BYTES / 4 } else { NORMALIZED_PAGE_BYTES };
            charges.push(cache.in_flight.charge(bytes));
            assert!(cache.in_flight.used() <= uploads);
        }
        drop(charges);
        assert_eq!(cache.in_flight.used(), 0);
        assert_eq!(cache.uploads_full(crate::residency::Class::Visible), uploads < NORMALIZED_PAGE_BYTES);
        assert_eq!(cache.upload_limit(), uploads);
    }
}

#[test]
fn decoded_sources_preserve_srgb_codes_and_padding() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_factory(gpu.factory.clone(), gpu.device.clone(), Default::default()).unwrap();
    drop(gpu);
    r.source_tiles.get_mut().admit_limits(64 << 20);
    let mut builder = SourceBuilder::new([255, 19], SourceInterpretation {
        channels: SourceChannels::Rgb, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 4 * 1024 * 1024).unwrap();
    for y in 0..19 {
        let row: Vec<_> = (0..255).flat_map(|x| [x as u8, (y * 11) as u8, (255 - x) as u8]).collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let prepared = crate::prepare_image_tile(&source, [0, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let raw = Scene::decode_prepared(&mut r, &mut encoder, &source, [0, 0], prepared).unwrap();
    assert_eq!(raw.texture.format(), wgpu::TextureFormat::Rgba8Unorm);
    let source_tile = crate::test_support::query_ready(&mut r, &mut encoder, |r, encoder| {
        Scene::promote_source(r, encoder, RawTile {texture:raw.texture.clone(),view:raw.view.clone()})
    });
    let source_view = source_tile.view.clone();
    let (output, view) = create_target(&r.device, [PAGE_SIZE; 2], wgpu::TextureFormat::Rgba32Float, "sampled display codes");
    let shader = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("display source sampling oracle"),
        source: wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var input: texture_2d<f32>;
            @group(0) @binding(1) var output: texture_storage_2d<rgba32float, write>;
            @compute @workgroup_size(8,8) fn sample(@builtin(global_invocation_id) id: vec3<u32>) {
                textureStore(output, vec2<i32>(id.xy), textureLoad(input, vec2<i32>(id.xy), 0));
            }".into()),
    });
    let pipeline = r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("display source sampling oracle"), layout: None, module: &shader,
        entry_point: Some("sample"), compilation_options: Default::default(), cache: None,
    });
    let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &pipeline.get_bind_group_layout(0), entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
        ],
    });
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline); pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(32, 32, 1);
    }
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    let read = crate::layer_tests::page_bytes(&r, &output);
    let mut maximum_error = 0_f64;
    for (i, p) in read.as_chunks::<16>().0.iter().enumerate() {
        let actual = std::array::from_fn::<_, 4, _>(|c| f32::from_ne_bytes(p[c * 4..c * 4 + 4].try_into().unwrap()));
        let (x, y) = (i % PAGE_SIZE as usize, i / PAGE_SIZE as usize);
        if x >= 255 || y >= 19 { assert_eq!(actual, [0.; 4]); continue; }
        assert_eq!(actual[3], 1.);
        for (c, code) in [x, y * 11, 255 - x].into_iter().enumerate() {
            let encoded = RgbSpace::Srgb.encode(f64::from(actual[c])) * 255.;
            maximum_error = maximum_error.max((encoded - code as f64).abs());
            assert_eq!(encoded.round() as usize, code, "8-bit source code changed");
        }
    }
    eprintln!("maximum display source error {maximum_error} of one 8-bit code");
    let (query, pending_query) = r.source_tiles.borrow_mut().plan(&r, &source, [0, 0]).unwrap();
    assert_eq!(query.texture.format(), wgpu::TextureFormat::Rgba32Float);
    assert_eq!(query.view, source_view);
    assert!(pending_query.is_none());
    drop(r);
    startup::finish_shader_compiler_shutdown();
}

#[test]
fn decoded_grant_shrink_waits_for_retired_slots_and_releases_discarded_charges() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (display,mut cache)=standalone();let source=scan_source(25,0);
    let first:Vec<_>=(0..24).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    let page_bytes=crate::resident::Storage::storage_bytes(&first[0].0);
    let overhead=cache.non_slot_bytes();
    assert_eq!(first[0].0.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(cache.gpu_bytes(),24*page_bytes+overhead);
    assert_eq!(evict(&display,&mut cache,4*page_bytes),4*page_bytes);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.flush_retired(&mut encoder);
    assert_eq!(cache.slots.len(),20);
    assert_eq!(cache.views.len(),20);assert_eq!(cache.retired_bytes.used(),4*page_bytes);
    for (index,slot) in cache.slots.iter().enumerate() {assert_eq!(cache.views.get(&slot.view),Some(&index));}
    assert_eq!(cache.gpu_bytes(),24*page_bytes+overhead);
    drop(first);encoder.submit(&r.queue);crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    assert_eq!(cache.gpu_bytes(),20*page_bytes+overhead);
    let second:Vec<_>=(0..24).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    assert_eq!(evict(&display,&mut cache,4*page_bytes),4*page_bytes);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.flush_retired(&mut encoder);
    drop((encoder,second));r.device.retirement_handle().flush().unwrap();
    assert_eq!(cache.retired_bytes.used(),0);
    assert_eq!(cache.gpu_bytes(),20*page_bytes+overhead);
}

#[test]
fn discarded_required_source_writes_reuse_their_own_slots() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_, mut cache) = standalone();
    let source = scan_source(1, 0);
    let (first, pending) = cache.plan(&r, &source, [0, 0]).unwrap();
    drop(pending);
    let (second, _) = cache.plan(&r, &source, [0, 0]).unwrap();
    assert_eq!(first.view, second.view);
    assert_eq!(cache.slots.len(), 1);
}

#[test]
fn prepared_source_samples_upload_without_renderer_decompression_and_share_cached_pixels() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let source=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[64,128,192,128]);
    let bytes=Arc::new([64,128,192,128].repeat((PAGE_SIZE*PAGE_SIZE) as usize));
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tile=Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],PreparedSourcePixels::NativeSamples(bytes)).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    assert_eq!(r.device.source_samples.stats().misses,0);
    assert_eq!(tile.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(crate::layer_tests::page_bytes(&r,&tile.texture),[64,128,192,128].repeat((PAGE_SIZE*PAGE_SIZE) as usize));
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let hit=Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],PreparedSourcePixels::NativeSamples(Arc::new(Vec::new()))).unwrap();
    assert_eq!(tile.view,hit.view);
    assert_eq!(r.source_cache_work(),[1,1]);
    let exact=Scene::promote_source(&mut r,&mut encoder,hit).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    let alpha=128./255.;
    let expected=[64,128,192].map(|code| RgbSpace::Srgb.decode(f64::from(code)/255.) as f32*alpha);
    for pixel in crate::test_support::float_pixels(&r,&exact.texture) {
        assert!((pixel[3]-alpha).abs()<2e-6);
        for c in 0..3 {assert!((pixel[c]-expected[c]).abs()<2e-6);}
    }
}

#[test]
fn prepared_icc_working_pixels_upload_exactly_without_renderer_color_transform() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut source=(*scan_source(1,0)).clone();
    source.interpretation.profile=ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::ProPhoto)).unwrap().into());
    let source=Arc::new(source);let expected=[0.25f32,0.125,0.0625,0.5];
    let mut bytes=Vec::with_capacity(NORMALIZED_PAGE_BYTES as usize);
    for _ in 0..PAGE_SIZE*PAGE_SIZE {for value in expected {bytes.extend_from_slice(&value.to_ne_bytes());}}
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tile=Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],PreparedSourcePixels::PremultipliedWorkingPixels(Arc::new(bytes))).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    assert_eq!(r.device.source_samples.stats().misses,0);
    assert!(r.source_tiles.borrow().decoders.is_empty());
    assert!(crate::test_support::float_pixels(&r,&tile.texture).into_iter().all(|pixel| pixel==expected));
}

#[test]
fn prepared_gray_alpha_samples_are_expanded_by_shared_worker_before_upload() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut builder=SourceBuilder::new([PAGE_SIZE;2],SourceInterpretation {channels:SourceChannels::GrayAlpha,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},1024*1024).unwrap();
    let row=[96,192].repeat(PAGE_SIZE as usize);for _ in 0..PAGE_SIZE {builder.push_row(&row).unwrap();}
    let source=Arc::new(builder.finish().unwrap());
    let prepared=crate::prepare_image_tile(&source,[0;2],RgbSpace::Srgb,&r.device.source_samples,None).unwrap();
    let PreparedSourcePixels::NativeSamples(bytes)=&prepared else {panic!("Expected native working samples")};
    assert!(bytes.as_chunks::<4>().0.iter().all(|pixel|*pixel==[96,96,96,192]));
    let codes=bytes.clone();
    let reads=r.device.source_samples.stats();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tile=Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],prepared).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    assert_eq!(tile.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(crate::layer_tests::page_bytes(&r,&tile.texture).as_slice(),codes.as_slice());
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let exact=Scene::promote_source(&mut r,&mut encoder,tile).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    let after=r.device.source_samples.stats();assert_eq!((reads.hits,reads.misses),(after.hits,after.misses));
    let alpha=192f32/255.;let value=RgbSpace::Srgb.decode(96./255.) as f32*alpha;
    for pixel in crate::test_support::float_pixels(&r,&exact.texture) {
        assert!((pixel[3]-alpha).abs()<2e-6);for channel in &pixel[..3] {assert!((*channel-value).abs()<2e-6);}
    }
}

#[test]
fn abandoned_source_writes_are_reused_before_resident_tiles_are_evicted() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_, mut cache) = standalone();
    let source = scan_source(3, 0);
    let key = |x: u32| Key::raster(&source.tiles[&[x, 0]], RgbSpace::Srgb, RgbSpace::Srgb);
    let (resident, write) = cache.plan_key(&r, key(0)).unwrap();
    drop(cache.plan_key(&r, key(1)).unwrap());
    let (replacement, pending) = cache.plan_key(&r, key(2)).unwrap();
    assert_ne!(replacement.view, resident.view);
    let (hit, none) = cache.plan_key(&r, key(0)).unwrap();
    assert!(none.is_none() && hit.view == resident.view, "a deferred frame keeps decoded tiles resident");
    assert_eq!(cache.slots.len(), 2, "the abandoned write's slot serves the next key");
    drop((write, pending));
}

#[test]
fn exact_native_batches_reading_more_tiles_than_are_free_shrink_until_they_fit() {
    use layer_core::authored::{PaintBase, SourceTarget};
    let (mut r, _pressure, _foreign_source, foreign, capacity) = drained_source_renderer();
    let tiles = capacity + 1;
    let (mut doc, ..) = threshold_document(threshold_source(tiles, 0));
    let extent = doc.composition().size;
    let (_, SourceTarget::Paint(paint)) = crate::test_support::add_paint(&mut doc.artwork, "Second base", extent) else { unreachable!() };
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(PaintBase::new(threshold_source(tiles, 64).into()));
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    r.test.reference = true;
    let frame = crate::test_support::packet(doc.scene(), extent);
    r.begin_frame_memory(frame);
    assert_eq!(r.source_tiles.borrow().free_slots(), capacity);
    assert!(2 * SOURCE_SLOTS > capacity);
    assert!(tiles > capacity);
    crate::test_support::submit_published(&mut r, frame);
    assert!(r.scale_display.as_ref().unwrap().evaluation == crate::scene::scale::Evaluation::Native);
    assert_working_source_slots(&r, capacity, &foreign);
    assert_threshold_pixels(&r, 0.5, 0);
}

#[test]
fn native_threshold_larger_than_the_decoded_floor_publishes_every_page_once() {
    let extent = [2560, 1792];
    let (doc, ..) = threshold_document(layer_core::color::source::rgba8_source(extent, |x, y|
        [(x / 7) as u8, (y / 5) as u8, ((x + y) / 3) as u8, 255]));
    let render = |slots| {
        let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
        let mut frame = crate::test_support::packet(doc.scene(), extent);
        frame.view.width_px = extent[0] / 4; frame.view.height_px = extent[1] / 4;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        crate::memory::memory_tests::declare_source_budget(&mut r, frame, slots, 16 << 20, 44 << 20);
        crate::test_support::submit_published(&mut r, frame);
        r
    };
    let floor = render(64);
    let reference = render(1024);
    let pages = page_coordinates(PixelRect::full(extent)).count() as u64;
    assert!(floor.scene_tiers.display.grant() < reference.scene_tiers.display.grant());
    assert!(reference.scene_tiers.display.grant() > pages * NORMALIZED_PAGE_BYTES);
    let [floor_cache, reference_cache] = [&floor, &reference].map(|r| r.scale_display.as_ref().unwrap());
    assert_eq!((floor_cache.plan.level, floor_cache.plan.bounds), (2, PixelRect::full(extent)));
    let texels = floor_cache.plan.size.into_iter().map(u64::from).product::<u64>();
    assert_eq!(floor.metrics.composited_pixels, texels, "resumed composition draws each output texel once");
    assert_eq!(crate::test_support::float_pixels(&floor, floor_cache.texture()),
        crate::test_support::float_pixels(&reference, reference_cache.texture()));
}

#[test]
fn a_deferred_frame_acquires_each_unfinished_page_at_most_once() {
    let extent = [2560, 2048];
    let (doc, ..) = threshold_document(layer_core::color::source::rgba8_source(extent, |x, y|
        [(x / 7) as u8, (y / 5) as u8, ((x + y) / 3) as u8, 255]));
    let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
    let pages = page_coordinates(PixelRect::full(extent)).count() as u64;
    let frame = crate::test_support::packet(doc.scene(), extent);
    crate::memory::memory_tests::declare_source_budget(&mut r, frame, 1024, 64 << 20, 1004 << 20);
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert!(crate::scene::scale::tests::publication_pending(&r), "the cold frame stops at its work budget");
    r.wait_idle().unwrap();
    crate::test_support::settle_decodes(&mut r);
    let before = r.source_cache_work();
    crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
    let after = r.source_cache_work();
    let lookups = after[0] + after[1] - before[0] - before[1];
    assert!(lookups <= pages, "units the last frame accepted but did not reach keep their inputs: {lookups} lookups for {pages} pages");
}

#[test]
fn cold_native_threshold_frames_replan_only_unfinished_pages() {
    let extent = [2560, 2048];
    let (doc, ..) = threshold_document(layer_core::color::source::rgba8_source(extent, |x, y|
        [(x / 7) as u8, (y / 5) as u8, ((x + y) / 3) as u8, 255]));
    let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
    let pages = page_coordinates(PixelRect::full(extent)).count() as u64;
    let frame = crate::test_support::packet(doc.scene(), extent);
    crate::memory::memory_tests::declare_source_budget(&mut r, frame, 1024, 64 << 20, 1004 << 20);
    let idle = FramePacket { composite_all: false, ..frame };
    let mut next = frame;
    let mut deferred = 0;
    loop {
        let before = r.source_cache_work();
        crate::test_support::submit_ready(&mut r, next).unwrap();
        let after = r.source_cache_work();
        if !crate::scene::scale::tests::publication_pending(&r) { break; }
        let lookups = after[0] + after[1] - before[0] - before[1];
        assert!(2 * lookups < pages, "deferred frame {deferred} looked up {lookups} of {pages} pages");
        deferred += 1;
        assert!(deferred < pages, "each deferred frame must finish the pages whose decodes arrived");
        r.wait_idle().unwrap();
        crate::test_support::settle_decodes(&mut r);
        next = idle;
    }
    let plan = r.scale_display.as_ref().unwrap().plan;
    assert_eq!((plan.level, plan.bounds), (0, PixelRect::full(extent)));
    assert_eq!(r.metrics.composited_pixels, u64::from(extent[0]) * u64::from(extent[1]), "resumed composition draws each pixel once");
}

#[test]
fn a_settled_blur_keeps_no_more_unused_decoded_tiles_than_the_view_floor() {
    use layer_core::authored::{PaintBase, SourceTarget};
    let extent = [4096, 4096];
    let mut doc = layer_core::Document::new(layer_core::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() });
    let SourceTarget::Paint(paint) = doc.working.target.unwrap() else { unreachable!() };
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(PaintBase::new(layer_core::color::source::rgba8_source(extent, |x, y|
        [(x / 7) as u8, (y / 5) as u8, ((x + y) / 3) as u8, 255]).into()));
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(8 << 30), ..Default::default() }).unwrap();
    let mut view = crate::test_support::view([1024; 2]);view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    crate::test_support::submit_settled(&mut r, FramePacket { view, composite_all: false, ..crate::test_support::packet(doc.scene(), extent) });
    crate::test_support::complete(&r);
    let mut blurred = doc.clone();
    crate::artwork_sample_tests::insert_effect(&mut blurred, layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program()), 0);
    let frame = FramePacket { view, composite_all: false, ..crate::test_support::packet(blurred.scene(), extent) };
    crate::test_support::submit_settled(&mut r, frame);
    for _ in 0..2 { crate::test_support::submit_ready(&mut r, frame).unwrap(); crate::test_support::complete(&r); }
    let floor = r.scene_tiers.display.floor();
    let sources = r.source_tiles.borrow();
    let epoch = sources.decoded.heap().epoch();
    let unused = |slot: &&Slot| slot.used.load(Ordering::Relaxed).saturating_add(1) < epoch && slot.ready() && !slot.leased();
    let arrays = |slots: &mut dyn Iterator<Item = &Slot>| slots.map(|slot| Arc::as_ptr(&slot.layer.array) as usize).collect::<std::collections::HashSet<_>>();
    let live = arrays(&mut sources.slots.iter().filter(|slot| !unused(slot)));
    let held = arrays(&mut sources.slots.iter().filter(unused));
    let only_unused = sources.arrays.iter().filter(|array| held.contains(&(Arc::as_ptr(array) as usize)) && !live.contains(&(Arc::as_ptr(array) as usize)))
        .map(|array| crate::texture_bytes(&array.texture)).sum::<u64>();
    assert!(only_unused <= floor, "{only_unused} bytes of decoded arrays hold only unused tiles above the view floor {floor}");
}

fn stored_code_source(depth: SampleDepth, tiles: u32) -> Arc<SourceImage> {
    let pixel = match depth {
        SampleDepth::U8 => vec![64, 128, 192, 128],
        SampleDepth::U16 => [16384u16, 32768, 49152, 32768].into_iter().flat_map(u16::to_le_bytes).collect(),
        _ => unreachable!(),
    };
    let extent = [PAGE_SIZE * tiles, PAGE_SIZE];
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, extent[0] as usize * extent[1] as usize * pixel.len() * 2).unwrap();
    let row = pixel.repeat(extent[0] as usize);
    for _ in 0..extent[1] { builder.push_row(&row).unwrap(); }
    Arc::new(builder.finish().unwrap())
}

fn stored_code_tile(r: &mut WgpuRasterizer, source: &Arc<SourceImage>, x: u32,
    encoder: &mut crate::submission::CommandEncoder) -> RawTile {
    pollster::block_on(r.compilation.prepare_all(crate::startup::SourceKinds::for_source(source).pipelines(&r.scene_pipelines.source))).unwrap();
    let prepared = crate::prepare_image_tile(source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
    Scene::decode_prepared(r, encoder, source, [x, 0], prepared).unwrap_or_else(|error| {
        let cache = r.source_tiles.borrow();
        panic!("source {x}: {error}; {} slots, {} GPU bytes", cache.slots.len(), cache.gpu_bytes())
    })
}

fn stored_code_format(depth: SampleDepth) -> wgpu::TextureFormat {
    match depth {
        SampleDepth::U8 => wgpu::TextureFormat::Rgba8Unorm,
        SampleDepth::U16 => wgpu::TextureFormat::Rg32Uint,
        _ => unreachable!(),
    }
}

fn stored_code_renderer(source: &Arc<SourceImage>) -> (WgpuRasterizer, layer_core::Document, layer_core::authored::SourceTarget) {
    let mut doc = layer_core::Document::new(layer_core::authored::PortableId::random(), source.extent[0], source.extent[1],
        layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() });
    let target = doc.working.target.unwrap();
    let layer_core::authored::SourceTarget::Paint(paint) = target else { unreachable!() };
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(source.clone().into()));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.ensure_document_metadata(source.extent, doc.scene()).unwrap();
    (r, doc, target)
}

fn assert_stored_code_upload(depth: SampleDepth) {
    let source = stored_code_source(depth, 1);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let tile = stored_code_tile(&mut r, &source, 0, &mut encoder);
    r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(&r);
    assert_eq!(tile.texture.format(), stored_code_format(depth));
    let sources = r.source_tiles.borrow();
    assert_eq!(sources.slots.len(), 1);
    assert_eq!(page_bytes(sources.slots[0].layer.format()), u64::from(PAGE_SIZE).pow(2) * if depth == SampleDepth::U8 {4} else {8});
    assert_eq!(sources.gpu_bytes(), page_bytes(sources.slots[0].layer.format()) + sources.non_slot_bytes());
}

#[test]
fn stored_source_u8_upload_retains_four_byte_codes() { assert_stored_code_upload(SampleDepth::U8); }

#[test]
fn stored_source_u16_upload_retains_eight_byte_packed_codes() { assert_stored_code_upload(SampleDepth::U16); }

#[test]
fn stored_source_encoded_slot_count_does_not_overstate_exact_working_capacity() {
    let source=stored_code_source(SampleDepth::U8,32);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    for x in 0..32 {stored_code_tile(&mut r,&source,x,&mut encoder);}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let cache=r.source_tiles.borrow_mut();
    assert_eq!(cache.slots.len(),32);
    let encoded=cache.slots.iter().map(Slot::bytes).sum::<u64>();
    assert_eq!(encoded,8*NORMALIZED_PAGE_BYTES);
    assert_eq!(cache.free_slots() as u64,(cache.decoded.headroom(crate::residency::Class::Visible)+encoded)/NORMALIZED_PAGE_BYTES,
        "exact work needs normalized pages, not the count of smaller encoded pages");
}

#[test]
fn stored_source_cancelled_promotion_does_not_leave_a_protected_duplicate_key() {
    let source=stored_code_source(SampleDepth::U8,1);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tile=stored_code_tile(&mut r,&source,0,&mut encoder);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut cancelled=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    Scene::promote_source(&mut r,&mut cancelled,tile).unwrap();
    r.uploads.finish(&cancelled);drop(cancelled);
    assert!(r.source_tiles.borrow().prepared_view(&source,[0,0]).is_none());
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tile=stored_code_tile(&mut r,&source,0,&mut encoder);
    assert_eq!(tile.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    let key=Key::Image(Arc::downgrade(&source),[0,0]);
    assert_eq!(r.source_tiles.borrow().slots.iter().filter(|slot|slot.key.as_ref().is_some_and(|stored|stored.matches(&key))).count(),1);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    Scene::promote_source(&mut r,&mut encoder,tile).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert_eq!(r.source_tiles.borrow().prepared_view(&source,[0,0]).unwrap().texture().format(),wgpu::TextureFormat::Rgba32Float);
}

#[test]
fn stored_source_new_exact_leases_do_not_retain_the_retired_encoded_representation() {
    let source=stored_code_source(SampleDepth::U16,1);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let encoded=stored_code_tile(&mut r,&source,0,&mut encoder);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    let encoded_lease=r.source_tiles.borrow().lease(&encoded.view).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let exact=Scene::promote_source(&mut r,&mut encoder,encoded).unwrap();
    let exact_lease=r.source_tiles.borrow().lease(&exact.view).unwrap();
    assert!(Arc::ptr_eq(&encoded_lease,&exact_lease));
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    assert_eq!(r.source_tiles.borrow().retired_bytes.used(),NORMALIZED_PAGE_BYTES/2);
    drop(encoded_lease);r.device.retirement_handle().flush().unwrap();
    let cache=r.source_tiles.borrow();
    assert_eq!(cache.gpu_bytes(),NORMALIZED_PAGE_BYTES+cache.non_slot_bytes());
    assert_eq!(cache.retired_bytes.used(),0);
    assert!(Arc::ptr_eq(&exact_lease,&cache.lease(&exact.view).unwrap()));
}

#[test]
fn stored_source_display_decode_stays_within_its_profile_bound_of_exact_promotion() {
    let mut differences=Vec::new();
    for depth in [SampleDepth::U8,SampleDepth::U16] { for space in [RgbSpace::Srgb,RgbSpace::DisplayP3,RgbSpace::AdobeRgb,RgbSpace::ProPhoto] {
        let extent=[257,259];
        let interpretation=SourceInterpretation {channels:SourceChannels::Rgba,depth,profile:ColorProfile::Builtin(space),profile_assumed:false};
        let mut builder=SourceBuilder::new(extent,interpretation,16<<20).unwrap();
        for y in 0..extent[1] {
            let row=(0..extent[0]).flat_map(|x| {
                let index=x+y*256;
                let codes=[index%65536,(index*17)%65536,(index*31)%65536,if x%3==0 {0}else if x%3==1 {65535}else{32768}];
                codes.into_iter().flat_map(|code|if depth==SampleDepth::U8 {vec![(code>>8) as u8]}else{(code as u16).to_le_bytes().to_vec()})
            }).collect::<Vec<_>>();
            builder.push_row(&row).unwrap();
        }
        let source=Arc::new(builder.finish().unwrap());
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut maximum=[0f64;3];
        for coordinate in [[0,0],[1,1]] {
            let pixels=crate::prepare_image_tile(&source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
            pollster::block_on(r.compilation.prepare_all([r.scene_pipelines.source.for_format(if depth==SampleDepth::U8 {wgpu::TextureFormat::Rgba8Unorm}else{wgpu::TextureFormat::Rg32Uint})])).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let raw=Scene::decode_prepared(&mut r,&mut encoder,&source,coordinate,pixels.clone()).unwrap();
            assert_eq!(encoder.pass_count(),0,"upload performs no normalization");
            let expected=match pixels {PreparedSourcePixels::NativeSamples(bytes)=>bytes,_=>unreachable!()};
            r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
            assert_eq!(crate::layer_tests::page_bytes(&r,&raw.texture),*expected,"stored codes and padding are lossless");
            let display=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            Scene::display_source(&mut r,&mut encoder,&source,coordinate,&raw.texture,&display.create_view(&Default::default())).unwrap();
            let exact=Scene::promote_source(&mut r,&mut encoder,raw).unwrap();
            r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
            let a=crate::layer_tests::page_bytes(&r,&display);
            let b=crate::layer_tests::page_bytes(&r,&exact.texture);
            for (a,b) in a.as_chunks::<16>().0.iter().zip(b.as_chunks::<16>().0) {
                let a:[f32;4]=std::array::from_fn(|channel|f32::from_le_bytes(a[channel*4..channel*4+4].try_into().unwrap()));
                let b:[f32;4]=std::array::from_fn(|channel|f32::from_le_bytes(b[channel*4..channel*4+4].try_into().unwrap()));
                if b[3]==0. || b[3]==1. {assert_eq!(a[3],b[3]);}else{assert!((a[3]-b[3]).abs()<=f32::EPSILON);}
                for channel in 0..3 {
                    let code=|pixel:[f32;4]|if pixel[3]==0. {0.}else{(RgbSpace::Srgb.encode(f64::from(pixel[channel]/pixel[3])).clamp(0.,1.)*255.).round()};
                    maximum[channel]=maximum[channel].max((code(a)-code(b)).abs());
                }
            }
        }
        let bound=if depth==SampleDepth::U8 && space!=r.device.working_space()
            && matches!(space,RgbSpace::Srgb|RgbSpace::DisplayP3) {2.}else{1.};
        differences.push((depth,space,maximum,bound));
    }}
    assert!(differences.iter().all(|(_,_,maximum,bound)|maximum.iter().all(|error|error<=bound)),"eight-bit display-code differences: {differences:?}");
}

#[test]
fn stored_source_exact_neighborhood_promotes_one_slot_and_reuses_its_lease() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let source = stored_code_source(depth, 2);
        let (mut r, _doc, target) = stored_code_renderer(&source);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let original = stored_code_tile(&mut r, &source, 0, &mut encoder);
        stored_code_tile(&mut r, &source, 1, &mut encoder);
        r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(&r);
        let lease = r.source_tiles.borrow().lease(&original.view).unwrap();
        let index = r.source_tiles.borrow().slots.iter().position(|slot| slot.view == original.view).unwrap();
        let expected_key = Key::Image(Arc::downgrade(&source), [0, 0]);
        let sample_misses = r.device.source_samples.stats().misses;
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.read_raw_neighborhood(target, [0, 0], [Some([0, 0])], &mut encoder).unwrap();
        let base = layer_core::authored::PaintBase::new(source.clone().into());
        let exact = r.paint_base_tile(&base, [0, 0], &mut encoder).unwrap().unwrap();
        assert_eq!(encoder.pass_count(), 1, "the first exact acquisition normalizes the encoded source once");
        assert_eq!(exact.texture.format(), wgpu::TextureFormat::Rgba32Float);
        {
            let sources = r.source_tiles.borrow();
            assert_eq!(sources.slots.len(), 2);
            assert_eq!(sources.slots[index].view, exact.view);
            assert!(sources.slots[index].key.as_ref().unwrap().matches(&expected_key));
            assert!(Arc::ptr_eq(&lease, &sources.lease(&exact.view).unwrap()));
            assert_eq!(sources.prepared_view(&source, [1, 0]).unwrap().texture().format(), stored_code_format(depth));
        }
        r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(&r);
        let misses = r.source_tiles.borrow().misses;
        for _ in 0..3 {
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            r.read_raw_neighborhood(target, [0, 0], [Some([0, 0])], &mut encoder).unwrap();
            let reused = r.paint_base_tile(&base, [0, 0], &mut encoder).unwrap().unwrap();
            assert_eq!(reused.view, exact.view);
            assert!(Arc::ptr_eq(&lease, &r.source_tiles.borrow().lease(&reused.view).unwrap()));
            assert_eq!(encoder.pass_count(), 0, "resident exact acquisitions cannot normalize again");
            r.uploads.finish(&encoder); encoder.submit(&r.queue);
        }
        assert_eq!(r.source_tiles.borrow().misses, misses);
        assert_eq!(r.device.source_samples.stats().misses, sample_misses);
        assert_ne!(original.view, exact.view);
        let maximum = depth.maximum() as f64;
        let codes = if depth == SampleDepth::U8 {[64., 128., 192., 128.]} else {[16384., 32768., 49152., 32768.]};
        let alpha = (codes[3] / maximum) as f32;
        let rgb = codes.map(|value| RgbSpace::Srgb.decode(value / maximum) as f32 * alpha);
        for pixel in crate::test_support::float_pixels(&r, &exact.texture) {
            assert!((pixel[3] - alpha).abs() < 2e-6);
            for channel in 0..3 { assert!((pixel[channel] - rgb[channel]).abs() < 2e-6); }
        }
    }
}

#[test]
fn stored_source_promoted_slot_reloads_encoded_after_ordinary_eviction() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let source = stored_code_source(depth, 1);
        let (mut r, _doc, _) = stored_code_renderer(&source);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        stored_code_tile(&mut r, &source, 0, &mut encoder);
        let base = layer_core::authored::PaintBase::new(source.clone().into());
        let promoted = r.paint_base_tile(&base, [0, 0], &mut encoder).unwrap().unwrap();
        assert_eq!(promoted.texture.format(), wgpu::TextureFormat::Rgba32Float);
        r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(&r);
        drop(promoted);
        crate::test_support::evict_sources(&mut r);
        assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_none());
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let reloaded = stored_code_tile(&mut r, &source, 0, &mut encoder);
        assert_eq!(reloaded.texture.format(), stored_code_format(depth));
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
    }
}

#[path = "smudge_tests.rs"]
mod smudge_tests;

#[test]
fn source_display_records_reuse_scene_uniform_storage_and_preserve_encoded_order() {
    let fixtures=[(RgbSpace::Srgb,[64u8,128,192,128]),(RgbSpace::ProPhoto,[192,64,128,255])];
    let sources=fixtures.map(|(space,pixel)| {
        let mut builder=SourceBuilder::new([PAGE_SIZE;2],SourceInterpretation {channels:SourceChannels::Rgba,
            depth:SampleDepth::U8,profile:ColorProfile::Builtin(space),profile_assumed:false},8<<20).unwrap();
        let row=pixel.repeat(PAGE_SIZE as usize);
        for _ in 0..PAGE_SIZE {builder.push_row(&row).unwrap();}
        Arc::new(builder.finish().unwrap())
    });
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tiles=sources.each_ref().map(|source|stored_code_tile(&mut r,source,0,&mut encoder));
    if r.scene.is_none() {r.scene=Some(Scene::new(&r).unwrap());}
    r.scene.as_mut().unwrap().begin_frame();
    let scene=r.scene.as_ref().unwrap();
    let (buffer,binding)=(scene.buffer.clone(),scene.binding.clone());
    let mut outputs=Vec::new();
    for (step,index) in [0usize,1,1,0].into_iter().enumerate() {
        if step==2 {r.scene.as_mut().unwrap().begin_frame();}
        let output=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        Scene::display_source(&mut r,&mut encoder,&sources[index],[0;2],&tiles[index].texture,&output.create_view(&Default::default())).unwrap();
        let scene=r.scene.as_ref().unwrap();
        assert_eq!(scene.buffer,buffer);
        assert_eq!(scene.binding,binding);
        assert_eq!(scene.record_count,step%2+1);
        outputs.push((index,output));
    }
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    for (index,output) in outputs {
        let (space,pixel)=fixtures[index];
        let alpha=f32::from(pixel[3])/255.;
        let rgb=layer_core::color::rgb::apply(space.linear_transform(r.device.working_space()),
            std::array::from_fn(|c|space.decode(f64::from(pixel[c])/255.)));
        for actual in crate::test_support::float_pixels(&r,&output) {
            assert_eq!(actual[3],alpha);
            for c in 0..3 {
                let code=|linear:f64|(r.device.working_space().encode(linear).clamp(0.,1.)*255.).round() as i16;
                assert!((code(f64::from(actual[c]/alpha))-code(rgb[c])).abs()<=1,"record {index}, channel {c}: {actual:?}");
            }
        }
    }
}

fn assert_promoted_source_reuse_before_completion(hold_encoded: bool) {
    let source=stored_code_source(SampleDepth::U8,2);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let encoded=stored_code_tile(&mut r,&source,0,&mut encoder);
    assert_eq!(encoded.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    let encoded_lease=hold_encoded.then(||r.source_tiles.borrow().lease(&encoded.view).unwrap());
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let exact=Scene::promote_source(&mut r,&mut encoder,encoded).unwrap();
    assert_eq!(exact.texture.format(),wgpu::TextureFormat::Rgba32Float);
    assert_eq!(encoder.pass_count(),1);
    if let Some(lease)=&encoded_lease {
        assert!(Arc::ptr_eq(lease,&r.source_tiles.borrow().lease(&exact.view).unwrap()));
    }
    let live=r.source_tiles.borrow().non_slot_bytes()+NORMALIZED_PAGE_BYTES;
    {
        let cache=r.source_tiles.borrow();
        assert_eq!(cache.retired_bytes.used(),NORMALIZED_PAGE_BYTES/4);
        assert_eq!(cache.gpu_bytes(),live+NORMALIZED_PAGE_BYTES/4,"the old encoded allocation stays charged until GPU completion");
    }
    r.scene_tiers.display.heap().evict(u64::MAX,crate::residency::Class::Authored);
    r.source_tiles.borrow_mut().retire_evicted();
    assert_eq!(r.source_tiles.borrow().prepared_view(&source,[0,0]).is_some(),hold_encoded,"a real encoded reader protects the promoted slot");
    drop(encoded_lease);r.device.retirement_handle().flush().unwrap();
    r.scene_tiers.display.heap().evict(u64::MAX,crate::residency::Class::Authored);
    r.source_tiles.borrow_mut().retire_evicted();
    assert!(r.source_tiles.borrow().prepared_view(&source,[0,0]).is_none(),"retirement bookkeeping does not lease the promoted slot after its real readers drop");
    let mut retired=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    r.source_tiles.borrow_mut().flush_retired(&mut retired);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);retired.submit(&r.queue);crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    let cache=r.source_tiles.borrow();
    assert_eq!(cache.gpu_bytes(),cache.non_slot_bytes());
    assert_eq!(cache.retired_bytes.used(),0);
}

#[test]
fn stored_source_promotion_retirement_does_not_lease_the_unborrowed_exact_slot() {
    assert_promoted_source_reuse_before_completion(false);
}

#[test]
fn stored_source_promoted_slot_reuses_after_the_real_encoded_reader_drops_before_completion() {
    assert_promoted_source_reuse_before_completion(true);
}

fn cold_restore_inputs_remain_owned_before_the_store_completes(base: bool) {
    use crate::layer_tests::placement::{paint_document, paint_mut, target};
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileKey};
    let mut document = paint_document([PAGE_SIZE; 2], "Cold accepted restore");
    let color = document.composition().color;
    let image = layer_core::color::source::rgba8_source([PAGE_SIZE; 2], |_, _| [255, 0, 0, 255]);
    let blob = TileBlob::encode(RasterPlane::Color.descriptor(color),
        &[255, 0, 0, 255].repeat((PAGE_SIZE * PAGE_SIZE) as usize)).unwrap();
    let key = if base {
        paint_mut(&mut document).base = Some(layer_core::authored::PaintBase::new(image.clone().into()));
        Key::Image(Arc::downgrade(&image), [0; 2])
    } else {
        paint_mut(&mut document).raster = RasterRevision::backed(RasterData {
            tiles: [(TileKey {plane: RasterPlane::Color, coordinate: [0; 2]}, RasterTile::backed(blob))].into(),
            ..Default::default()
        });
        let stored = paint_mut(&mut document).raster.try_data().unwrap().unwrap();
        let stored = stored.tiles[&TileKey {plane: RasterPlane::Color, coordinate: [0; 2]}].try_backing().unwrap().unwrap();
        Key::raster(&stored, color.space, color.space)
    };
    let mut renderer = WgpuRasterizer::new_native_headless(color).unwrap();
    let store = crate::test_support::GatedStore::new(&layer_core::Editor::new(document.clone()).retained_tiles(), false);
    let mut dab = crate::layer_tests::dab([0.; 4]);
    dab.center = layer_core::Point {x: 128., y: 128.};
    let batch = crate::test_support::dab_batch(target(&document), crate::tests::test_style(layer_core::BrushExecution::Dry), dab.bounds());
    let frame = layer_render::FramePacket {dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch),
        reset_layers: true, ..crate::test_support::packet(document.scene(), [PAGE_SIZE; 2])};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while store.gate.reads.load(Ordering::Acquire) == 0 {
        assert!(!renderer.raster_dependencies_ready(frame, None), "closed input store must defer restoration: base={base}");
        assert!(std::time::Instant::now() < deadline, "decode worker did not request stored source bytes: base={base}");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let accepted_view = {
        let cache = renderer.source_tiles.borrow();
        let slot = cache.slots.iter().find(|slot| slot.key.as_ref().is_some_and(|stored| stored.matches(&key)))
            .expect("cold restore must own its source slot before decoder completion");
        assert!(slot.leased(), "accepted cold source must have a real lease: base={base}");
        assert!(!slot.ready(), "closed store cannot have produced ready input pixels");
        slot.view.clone()
    };
    let count = 4;
    let pressure = scan_source(count, 0);
    let before = renderer.source_tiles.borrow().evictions;
    let mut writes = Vec::new();
    for coordinate in page_coordinates(PixelRect::full(pressure.extent)).take(count) {
        let (_, write) = renderer.source_tiles.borrow_mut().plan_key_for(&renderer,
            Key::Image(Arc::downgrade(&pressure), coordinate), SourceUse::Exact).unwrap();
        writes.extend(write);
    }
    crate::test_support::evict_sources(&mut renderer);
    {
        let cache = renderer.source_tiles.borrow();
        assert!(cache.evictions > before, "unrelated requests must actually recycle source storage");
        let slot = cache.slots.iter().find(|slot| slot.key.as_ref().is_some_and(|stored| stored.matches(&key))).unwrap();
        assert_eq!(slot.view, accepted_view, "pressure must not evict an accepted cold source");
        assert!(slot.leased());
    }
    drop(writes);
    store.gate.release();
    crate::test_support::submit_settled(&mut renderer, frame);
    let page = renderer.paint_layers.iter().find(|layer| layer.id == target(&document)).unwrap()
        .pages.iter().find(|page| page.coordinate == [0; 2]).unwrap();
    let expected=renderer.current_raster_version(target(&document),RasterPlane::Color,page.coordinate).unwrap();
    let read=renderer.read_layer_color(target(&document),page.coordinate,&expected,None).unwrap();
    assert!(crate::test_support::float_pixels(&renderer, &read.value().texture).iter().all(|pixel| *pixel == [1., 0., 0., 1.]),
        "accepted cold restore preserves its pixels after pressure: base={base}");
}

#[test]
fn cold_paint_base_restore_owns_its_source_before_decode_completion() {
    cold_restore_inputs_remain_owned_before_the_store_completes(true);
}

#[test]
fn cold_raster_color_restore_owns_its_source_before_decode_completion() {
    cold_restore_inputs_remain_owned_before_the_store_completes(false);
}

#[test]
fn same_encoder_preview_pages_are_accepted_without_waiting_for_submission() {
    use crate::{scene::{Declared,PreviewDeclaration,PreviewPass,PreviewQuality,PreviewStage,SignatureFrame},submission::ColorPass};
    let mut artwork=layer_core::Artwork::new([256;2]).unwrap();
    let (_,target)=crate::test_support::add_paint(&mut artwork,"Paint",[256;2]);
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.ensure_document_metadata([256;2],document.scene()).unwrap();
    let dab=crate::tests::test_dab([32.,32.],[0.2,0.4,0.6,1.],1.);
    let mut batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
    batch.kind=layer_render::DabBatchKind::Preview;
    let packet=FramePacket {dab_batches:std::slice::from_ref(&batch),dabs:std::slice::from_ref(&dab),..crate::test_support::packet(document.scene(),[256;2])};
    let declaration=Arc::new(PreviewDeclaration::from_scene(packet,Default::default(),Default::default(),PreviewQuality::default(),None,None).unwrap().unwrap());
    declaration.accept_inputs(target,layer_core::raster::RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0),PreviewPass::Color,Vec::new()).unwrap();
    let name=declaration.version(target,layer_core::raster::RasterPlane::Color,[0;2],PreviewStage::Final).unwrap();
    r.preview_declaration=Some(declaration.clone());
    let expected=Arc::new(SignatureFrame::capture(&r,packet,Declared {preview:Some(declaration),..Default::default()}).unwrap());
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let receipt=encoder.submission();
    let declare=|r:&WgpuRasterizer,consumer:Option<crate::submission::Submission>|super::declare(Inputs::default(),Declaration::scene(expected.clone(),consumer),
        |accepted|accepted.color(r,packet.scene,target,[0;2]).map(drop));
    let mut page=LayerPage {derived:None,coordinate:[0;2],primary:crate::resident::Resident::unwritten(r.create_page_surface(&r.raster_tiers.paint,"same encoder preview")),secondary:None,active_secondary:false,primary_needs_clear:false,stroke_start:None};
    let write=page.active_mut().initialize(&receipt).unwrap();
    drop(encoder.color_pass("preview output",&write.value().view,wgpu::LoadOp::Clear(wgpu::Color {r:0.2,g:0.4,b:0.6,a:1.})));
    write.publish(&name);
    assert!(page.active().read(&name,None).is_none());
    assert!(page.active().read(&name,Some(&receipt)).is_some());
    r.preview_pages.push(page);r.preview_layer_id=Some(target);r.preview_requires_base=true;
    r.preview_damage=batch_pixel_rect(&batch,[256;2]);r.preview_footprint=[[0;2]].into();
    assert!(r.preview_page([0;2]).is_some());
    let inputs=declare(&r,Some(receipt.clone())).unwrap();
    assert!(inputs.tiles.is_empty());
    assert!(inputs.bases.is_empty());
    let mut accepted=inputs.acquire(&r,crate::residency::Class::Visible).unwrap();
    let [Some(selected),None]=accepted.color(&r,packet.scene,target,[0;2]).unwrap() else {panic!("accepted preview missing")};
    assert_eq!(accepted.witnesses(target,RasterPlane::Color,[0;2]).len(),1);
    let previews=std::mem::take(&mut r.preview_pages);
    let [Some(kept),None]=accepted.color(&r,packet.scene,target,[0;2]).unwrap() else {panic!("accepted preview was reselected")};
    assert_eq!(kept.tile.view,selected.tile.view);
    r.preview_pages=previews;
    assert!(matches!(declare(&r,None),Err(GpuRasterError::NotReady)));
    let page=&r.preview_pages[0];
    assert!(matches!(r.read_scene_color_input(document.scene(),target,page,true,None,None),Err(GpuRasterError::NotReady)));
    let mut unrelated=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    assert!(matches!(r.read_scene_color_input(document.scene(),target,page,true,None,Some(&unrelated.submission())),Err(GpuRasterError::NotReady)));
    assert!(r.read_scene_color_input(document.scene(),target,page,true,None,Some(&receipt)).unwrap().is_some());
    encoder.submit(&r.queue);
    assert!(declare(&r,None).is_ok());
    assert!(r.read_scene_color_input(document.scene(),target,&r.preview_pages[0],true,None,None).unwrap().is_some());
    for pixel in crate::test_support::float_pixels(&r,&r.preview_pages[0].primary.read(&name,None).unwrap().value().texture) {
        for (actual,expected) in pixel.into_iter().zip([0.2,0.4,0.6,1.]) {assert!((actual-expected).abs()<2e-5);}
    }
}
#[test]
fn a_persistent_read_for_an_earlier_frame_does_not_take_a_later_write_under_its_name() {
    use crate::scene::{Declared,SignatureFrame};
    let mut artwork=layer_core::Artwork::new([256;2]).unwrap();
    let (_,target)=crate::test_support::add_paint(&mut artwork,"Paint",[256;2]);
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.ensure_document_metadata([256;2],document.scene()).unwrap();
    let dab=crate::tests::test_dab([32.,32.],[0.2,0.4,0.6,1.],1.);
    let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
    crate::test_support::submit_ready(&mut r,FramePacket {dab_batches:std::slice::from_ref(&batch),dabs:std::slice::from_ref(&dab),..crate::test_support::packet(document.scene(),[256;2])}).unwrap();
    let frame=SignatureFrame::capture(&r,crate::test_support::packet(document.scene(),[256;2]),Declared::default()).unwrap();
    let bytes=vec![0;(PAGE_SIZE*PAGE_SIZE*r.device.working_format().block_copy_size(None).unwrap()) as usize];
    crate::test_support::upload_layer_page(&mut r,document.scene(),target,[0;2],&bytes);
    let page=r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter().find(|page|page.coordinate==[0;2]).unwrap();
    assert!(r.read_scene_color_input(document.scene(),target,page,false,None,None).unwrap().is_some(),"a read of current names takes the later write");
    assert!(!matches!(r.read_scene_color_input(document.scene(),target,page,false,Some(&frame),None),Ok(Some(_))),"a read for the earlier frame does not take the later write");
}

#[test]
fn retired_source_texture_waits_for_its_fence_and_last_encoded_consumer_after_cache_drop() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_,mut cache)=standalone();
    let source=scan_source(1,0);
    let (raw,_)=cache.plan(&r,&source,[0,0]).unwrap();
    let lease=cache.lease(&raw.view).unwrap();
    let bytes=crate::resident::Storage::storage_bytes(&raw);
    let retired=cache.retired_bytes.clone();
    let slot=cache.remove_slot(0);
    let completion=cache.retire(slot);
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let usable=|texture:&TileTexture| {
        let scope=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut commands=r.device.create_command_encoder(&Default::default());
        let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:bytes,usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        commands.copy_texture_to_buffer(texture.as_image_copy(),wgpu::TexelCopyBufferInfo {buffer:&buffer,layout:wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(PAGE_SIZE*4),rows_per_image:None}},wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1});
        r.device.submit(&r.queue,[commands.finish()]);
        pollster::block_on(scope.pop()).is_none()
    };
    assert!(usable(&raw.texture));
    encoder.on_submitted_work_done(move|| {
        #[cfg(not(target_arch="wasm32"))]
        completion.complete();
        #[cfg(target_arch="wasm32")]
        drop(completion);
    });
    encoder.submit(&r.queue);
    crate::test_support::complete(&r);
    assert!(usable(&raw.texture),"completed GPU work cannot retire an encoded consumer still held by CPU");
    drop(cache);
    assert!(usable(&raw.texture),"cache drop cannot invalidate the held encoded input");
    drop(lease);
    r.device.retirement_handle().flush().unwrap();
    assert_eq!(retired.used(),0,"the poll owner must credit actual resource destruction");
    assert!(!usable(&raw.texture),"the last encoded consumer must release the retired physical texture without a cache reap");
}

#[test]
fn discarded_retirement_encoder_keeps_a_previously_used_source_until_its_gpu_fence() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert_ne!(r.adapter.get_info().device_type,wgpu::DeviceType::Cpu);
    let (_,mut cache)=standalone();
    let source=scan_source(1,0);
    let (raw,_)=cache.plan(&r,&source,[0,0]).unwrap();
    let bytes=crate::resident::Storage::storage_bytes(&raw);
    let retired=cache.retired_bytes.clone();
    let (mut busy,duration,_raw)=crate::test_support::bounded_outstanding_gpu_work(&r);
    let output=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:bytes,
        usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    busy.copy_texture_to_buffer(raw.texture.as_image_copy(),wgpu::TexelCopyBufferInfo {
        buffer:&output,layout:wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(PAGE_SIZE*4),rows_per_image:None},
    },wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1});
    let fence=Arc::new(std::sync::atomic::AtomicBool::new(false));
    busy.submit(&r.queue);
    let completed=fence.clone();
    r.queue.on_submitted_work_done(move||completed.store(true,Ordering::Release));
    r.device.request_poll();
    let before_discard=!fence.load(Ordering::Acquire);
    let slot=cache.remove_slot(0);
    let completion=cache.retire(slot);
    cache.retiring.push(completion);
    let mut abandoned=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.flush_retired(&mut abandoned);
    drop(abandoned);
    let before_registration=!fence.load(Ordering::Acquire);
    let (registered,registration)=std::sync::mpsc::channel();
    r.device.retirement_handle().run(move|| {let _=registered.send(());});
    let registration=registration.recv_timeout(std::time::Duration::from_secs(10));
    let during=!fence.load(Ordering::Acquire);
    let retained=retired.used();
    let scope=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut check=r.device.create_command_encoder(&Default::default());
    check.copy_texture_to_buffer(raw.texture.as_image_copy(),wgpu::TexelCopyBufferInfo {
        buffer:&output,layout:wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(PAGE_SIZE*4),rows_per_image:None},
    },wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1});
    r.device.submit(&r.queue,[check.finish()]);
    let usable=pollster::block_on(scope.pop()).is_none();
    let actual=duration();
    r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let final_flush=r.device.retirement_handle().flush();
    let released=retired.used();
    registration.unwrap();final_flush.unwrap();
    assert!(before_discard && before_registration && during,"the real prior-use fence must remain outstanding through the discarded encoder and observation");
    assert!((40. ..=75.).contains(&actual),"bounded GPU prerequisite: {actual} ms");
    assert_eq!(released,0,"actual completion must release the physical charge");
    assert_eq!(retained,bytes,"discard is not completion of the source texture's previous GPU use");
    assert!(usable,"the retired source texture must stay usable until its actual fence");
}

#[test]
fn accepted_inputs_reject_a_different_selection_or_storage() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let source=scan_source(2,0);let address=(SourceTarget::default(),RasterPlane::Color,[0;2]);
    let decision=|coordinate|AcceptedInputs {decisions:Decisions([(address,(false,[Some(Selected::Decoded(Key::Image(Arc::downgrade(&source),coordinate))),None]))].into()),..Default::default()};
    let (_,mut cache)=standalone();
    let (tile,pending)=cache.plan(&r,&source,[0;2]).unwrap();
    let key=Key::Image(Arc::downgrade(&source),[0;2]);
    let mut encoded=cache.lease(&tile.view).unwrap();
    encoded.read=Some(Arc::new(Read {key:key.clone(),tile:tile.clone(),pending:pending.into_iter().collect(),assembly:None}));
    let (tile,pending)=cache.promote(&r,tile).unwrap();
    let mut exact=cache.lease(&tile.view).unwrap();
    exact.read=Some(Arc::new(Read {key,tile,pending:pending.into_iter().collect(),assembly:None}));
    for (mut held,incoming) in [(decision([0;2]),decision([1,0])),(vec![encoded].into(),vec![exact].into())] {
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||held.extend(incoming)));
        if cfg!(debug_assertions) {assert!(result.is_err(),"incompatible accepted input did not assert");}
        else {assert!(matches!(result,Ok(Err(GpuRasterError::Invariant(_)))));}
    }
}

#[test]
fn accepted_input_reuse_releases_other_units_pins() {
    let unit=InputUnit::new(page_rect([2,3]),4);
    for (next,matching) in [(unit,true),(InputUnit::new(page_rect([3,3]),4),false),(InputUnit::new(page_rect([2,3]),0),false)] {
        let pin=Arc::new(());let weak=Arc::downgrade(&pin);
        let mut inputs=AcceptedInputs::default().for_unit(unit).unwrap();inputs.hold(pin);
        let mut slot=Some(inputs);
        let reused=AcceptedInputs::take_for(&mut slot,next);
        assert!(slot.is_none());
        assert_eq!(reused.is_some(),matching);
        assert_eq!(weak.strong_count(),usize::from(matching));
        if let Some(inputs)=reused {inputs.check_unit(next).unwrap();}
        assert_eq!(weak.strong_count(),0);
    }
}
#[test]
#[should_panic(expected="is reachable in declare mode")]
fn an_encode_reached_from_a_declare_pass_fails() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene=Scene::new(&r).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let _=scene.declare(&mut r,Inputs::default(),Declaration::raw(None,None,None),|scene,r,_|scene.encode_jobs(r,&mut encoder));
}

fn upload_window_renderer(uploads: u64) -> (WgpuRasterizer, std::sync::mpsc::Receiver<()>) {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
    r.source_tiles.get_mut().admit_limits(uploads);
    let (woken, wake) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = woken.send(()); }));
    (r, wake)
}

fn queued_gpu_work(r: &WgpuRasterizer) -> crate::submission::CommandEncoder {
    let (busy, _, raw) = crate::test_support::bounded_outstanding_gpu_work(r);
    for _ in 0..4 { r.device.submit(&r.queue, [raw(1)]).unwrap(); }
    busy
}

fn after_wake<T>(r: &mut WgpuRasterizer, wake: &std::sync::mpsc::Receiver<()>, mut attempt: impl FnMut(&mut WgpuRasterizer) -> Result<T, GpuRasterError>) -> (T, usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    for wakes in 1.. {
        wake.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).expect("the upload release wakes the yielded work");
        match attempt(r) {
            Ok(value) => return (value, wakes),
            Err(GpuRasterError::NotReady) => {},
            Err(error) => panic!("{error}"),
        }
    }
    unreachable!()
}

#[test]
fn a_full_upload_window_yields_an_upload_without_waiting_for_the_gpu_and_its_release_wakes_the_retry() {
    let (mut r, wake) = upload_window_renderer(4 * NORMALIZED_PAGE_BYTES);
    let source = scan_source(1, 32);
    let busy = queued_gpu_work(&r);
    let limit = r.source_tiles.borrow().in_flight.limit();
    let reserved = r.source_tiles.borrow().admit_upload(crate::residency::Class::Authored, limit).unwrap();
    r.source_tiles.borrow().charge_upload(&busy, reserved, limit);
    busy.submit(&r.queue);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let pixels = crate::prepare_image_tile(&source, [0, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
    let started = std::time::Instant::now();
    let result = Scene::decode_prepared(&mut r, &mut encoder, &source, [0, 0], pixels);
    let elapsed = started.elapsed();
    assert!(matches!(result, Err(GpuRasterError::NotReady)), "a full upload window must yield the upload: {:?}", result.map(drop));
    assert!(elapsed < std::time::Duration::from_millis(100), "the upload waited {elapsed:?} for the GPU");
    assert_eq!(r.source_tiles.borrow().in_flight.used(), limit);
    after_wake(&mut r, &wake, |r| {
        let pixels = crate::prepare_image_tile(&source, [0, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
        Scene::decode_prepared(r, &mut encoder, &source, [0, 0], pixels)
    });
    assert!(r.source_tiles.borrow().in_flight.used() <= limit);
}

#[test]
fn background_uploads_cannot_take_the_upload_floor_reserved_for_visible_work() {
    use crate::residency::Class;
    let (mut r, _wake) = upload_window_renderer(4 * NORMALIZED_PAGE_BYTES);
    let source = scan_source(2, 32);
    let busy = queued_gpu_work(&r);
    let reserved = r.source_tiles.borrow().admit_upload(crate::residency::Class::Authored, 2 * NORMALIZED_PAGE_BYTES).unwrap();
    r.source_tiles.borrow().charge_upload(&busy, reserved, 2 * NORMALIZED_PAGE_BYTES);
    busy.submit(&r.queue);
    let mut scene = Scene::new(&r).unwrap();
    for (x, class) in [(0, Class::Prefetch), (1, Class::Visible)] {
        let (_, pending) = r.source_tiles.borrow_mut().plan(&r, &source, [x, 0]).unwrap();
        let pending = pending.unwrap();
        pending.set_prepared(Some(crate::prepare_image_tile(&source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap()));
        scene.jobs = vec![crate::scene::Job::DecodedTile(pending)];
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.prepare_jobs(&mut r, &mut encoder, 0, class);
        assert_eq!((class, result.as_ref().ok().copied()), (class, (class == Class::Visible).then_some(true)), "{result:?}");
        scene.jobs.clear();
        Scene::submit_chunk(&mut r, &mut encoder, "test floor").unwrap();
    }
}

#[test]
fn decode_results_keep_their_requester_class_and_navigator_results_cannot_take_the_upload_floor() {
    use crate::{object_image_mips::{PreparedImageWork, PreparedMipTile}, residency::Class};
    let (mut r, _wake) = upload_window_renderer(4 * NORMALIZED_PAGE_BYTES);
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.integer])).unwrap();
    let source = scan_source(2, 32);
    let busy = queued_gpu_work(&r);
    let reserved = r.source_tiles.borrow().admit_upload(crate::residency::Class::Authored, 2 * NORMALIZED_PAGE_BYTES).unwrap();
    r.source_tiles.borrow().charge_upload(&busy, reserved, 2 * NORMALIZED_PAGE_BYTES);
    busy.submit(&r.queue);
    let space = r.device.working_space();
    for (x, class) in [(0, Class::Navigator), (1, Class::Visible)] {
        let pixels = crate::prepare_image_tile(&source, [x, 0], space, &r.device.source_samples, None);
        r.source_results.push_back((space, PreparedImageWork::Image(PreparedMipTile {
            class, use_: SourceUse::Native, id: layer_core::authored::PortableId::random(), source: source.clone(), coordinate: [x, 0], pixels,
        }), r.scene_tiers.ram.charge(0)));
    }
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let upload = r.drain_image_decode(&mut encoder).unwrap().expect("a visible result may use the floor");
    assert_eq!((upload.class, upload.coordinate), (Class::Visible, [1, 0]));
    assert!(r.drain_image_decode(&mut encoder).unwrap().is_none());
    assert!(matches!(r.source_results.iter().map(|(_, held, _)| held.class()).collect::<Vec<_>>()[..], [Class::Navigator]));
    Scene::submit_chunk(&mut r, &mut encoder, "test floor").unwrap();
}

#[test]
fn an_ordered_batch_larger_than_its_upload_window_resumes_after_each_release_without_waiting() {
    let (mut r, wake) = upload_window_renderer(NORMALIZED_PAGE_BYTES + 1);
    let source = scan_source(4, 32);
    let pending: Vec<_> = (0..4).map(|x| {
        let (_, pending) = r.source_tiles.borrow_mut().plan(&r, &source, [x, 0]).unwrap();
        let pending = pending.unwrap();
        pending.set_prepared(Some(crate::prepare_image_tile(&source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap()));
        pending
    }).collect();
    let mut scene = Scene::new(&r).unwrap();
    scene.ordered = true;
    scene.jobs = pending.iter().cloned().map(crate::scene::Job::DecodedTile).collect();
    queued_gpu_work(&r).submit(&r.queue);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut attempt = |r: &mut WgpuRasterizer| {
        let started = std::time::Instant::now();
        let result = scene.encode_query_jobs(r, &mut encoder, None);
        assert!(started.elapsed() < std::time::Duration::from_millis(100), "an attempt waited {:?} for the GPU", started.elapsed());
        assert!(r.source_tiles.borrow().in_flight.used() <= NORMALIZED_PAGE_BYTES + 1);
        Scene::submit_chunk(r, &mut encoder, "test batch").unwrap();
        result
    };
    assert!(matches!(attempt(&mut r), Err(GpuRasterError::NotReady)), "the first attempt must yield at its full upload window");
    assert_eq!(pending.iter().filter(|pending| pending.recorded()).count(), 1);
    let (_, wakes) = after_wake(&mut r, &wake, &mut attempt);
    assert!(wakes >= 3, "each upload window release resumes one tile, not {wakes} wakes");
    assert!(pending.iter().all(|pending| pending.recorded()));
}

#[test]
fn a_retired_window_finishes_its_gpu_work_before_the_next_window_starts() {
    let (mut r, _wake) = upload_window_renderer(4 * NORMALIZED_PAGE_BYTES);
    queued_gpu_work(&r).submit(&r.queue);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = crate::CompletionFlag(finished.clone());
    encoder.on_submitted_work_done(move || drop(flag));
    Scene::retire_window(&mut r, &mut encoder, "test window").unwrap();
    assert!(finished.load(Ordering::Acquire), "the next window started while the previous window's GPU work was in flight");
}

#[test]
fn a_capture_yields_its_next_window_until_the_previous_window_retires() {
    let extent = [1100, 700];
    let (doc, _) = crate::tests::image_windows::blurred_photo(extent, layer_core::color::DocumentColor::default(), layer_core::BlendSpace::Perceptual);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let (woken, wake) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = woken.send(()); }));
    let packet = crate::test_support::packet(doc.scene(), extent);
    let mut capture = crate::artwork::Capture::default();
    let (whole, _) = crate::create_color_target(&r.device, extent, "whole window");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    crate::test_support::query_ready(&mut r, &mut encoder, |r, encoder| capture.region_into(r, packet, PixelRect::full(extent), &whole, encoder));
    let page = PixelRect::new(0, 0, PAGE_SIZE, PAGE_SIZE);
    assert!(capture.peak_image_bytes > 0);
    queued_gpu_work(&r).submit(&r.queue);
    while wake.try_recv().is_ok() {}
    let (tile, _) = crate::create_color_target(&r.device, [PAGE_SIZE; 2], "page window");
    let result = capture.region_into(&mut r, packet, page, &tile, &mut encoder);
    assert!(matches!(result, Err(GpuRasterError::NotReady)), "the next window started while the previous one was in flight: {result:?}");
    after_wake(&mut r, &wake, |r| capture.region_into(r, packet, page, &tile, &mut encoder));
}

#[test]
fn a_snapshot_worker_batch_larger_than_its_upload_window_waits_in_place_within_the_window() {
    let (mut r, _wake) = upload_window_renderer(NORMALIZED_PAGE_BYTES + 1);
    r.snapshot_worker = true;
    let source = scan_source(4, 32);
    let pending: Vec<_> = (0..4).map(|x| {
        let (_, pending) = r.source_tiles.borrow_mut().plan(&r, &source, [x, 0]).unwrap();
        let pending = pending.unwrap();
        pending.set_prepared(Some(crate::prepare_image_tile(&source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap()));
        pending
    }).collect();
    let mut scene = Scene::new(&r).unwrap();
    scene.jobs = pending.iter().cloned().map(crate::scene::Job::DecodedTile).collect();
    queued_gpu_work(&r).submit(&r.queue);
    r.metrics.source_upload_peak_bytes = 0;
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.encode_jobs(&mut r, &mut encoder).expect("a worker waits for its upload window instead of abandoning the batch");
    assert!(pending.iter().all(|pending| pending.recorded()));
    assert!(r.metrics.source_upload_peak_bytes <= NORMALIZED_PAGE_BYTES + 1, "{} bytes of uploads were in flight", r.metrics.source_upload_peak_bytes);
}

#[test]
fn source_lease_counters_match_a_slot_walk_after_seeded_leases_requirements_promotions_and_evictions() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (display, mut cache) = standalone();
    let source = scan_source(24, 0);
    let walk = |cache: &DecodedTiles| {
        let leased = cache.slots.iter().filter(|slot| slot.leased()).map(Slot::bytes).sum::<u64>();
        [leased, leased + cache.retired_bytes.used()]
    };
    let mut seed = 0x2545_f491_4f6c_dd1du64; let mut counts = [0; 5]; let mut leases = Vec::new();
    for _ in 0..256 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let op = ((seed >> 59) % 5) as usize;
        let applied = match op {
            0 => cache.plan(&r, &source, [(seed >> 33) as u32 % 24, 0]).ok()
                .and_then(|(tile, _)| cache.lease(&tile.view).map(|lease| leases.push((tile, lease)))).is_some(),
            1 => !leases.is_empty() && { leases.swap_remove((seed >> 8) as usize % leases.len()); true },
            2 => {
                display.heap().begin_frame();
                evict(&display, &mut cache, 4 * NORMALIZED_PAGE_BYTES);
                true
            }
            3 => { let before = cache.keys.len(); cache.release_batch(cache.batch_checkpoint().saturating_sub((seed >> 8) % 4)); before != cache.keys.len() }
            _ => leases.last().is_some_and(|(tile, _)| cache.promote(&r, RawTile {texture: tile.texture.clone(), view: tile.view.clone()})
                .is_ok_and(|(_, pending)| pending.is_some())),
        };
        counts[op] += usize::from(applied);
        r.device.retirement_handle().flush().unwrap();
        assert_eq!([cache.leased.used(), cache.pinned_bytes()],
            walk(&cache), "source counters after op {op}");
        assert!(cache.keys.len() == cache.slots.iter().filter(|slot| slot.key.is_some()).count()
            && cache.keys.iter().all(|(key, index)| cache.slots[*index].key.as_ref() == Some(key)), "slot key index after op {op}");
    }
    drop(leases);
    assert_eq!(cache.leased.used(), 0);
    assert!(counts.iter().all(|count| *count > 0), "the seeded sequence must lease, release, require, re-key and promote slots: {counts:?}");
}

#[test]
fn a_still_frame_after_a_stroke_leaves_no_source_tile_leased() {
    let doc=crate::scene::scale::tests::document_at([1024,512]);let extent=doc.composition().size;let target=doc.working.target.unwrap();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=crate::test_support::packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.document_to_surface=[0.5,0.,0.,0.5,0.,0.];
    crate::test_support::submit_settled(&mut r,frame);
    for step in 0..4 {
        let dab=crate::tests::test_dab([200.+40.*step as f32,200.],[0.9,0.1,0.3,1.],0.8);
        let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
        crate::test_support::submit_ready(&mut r,layer_render::FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
    }
    crate::test_support::submit_settled(&mut r,frame);
    let leased=r.source_tiles.borrow().slots.iter().filter(|slot|slot.leased()).map(|slot|slot.bytes()).sum::<u64>();
    assert_eq!(leased,0,"a still frame after a stroke holds no source tile lease outside held or admitted units");
}

#[test]
fn declaring_another_unit_preserves_the_retained_job_prefix() {
    let document=crate::layer_tests::placement::paint_document([PAGE_SIZE;2],"retained declaration prefix");
    let packet=crate::test_support::packet(document.scene(),[PAGE_SIZE;2]);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.ensure_document_metadata([PAGE_SIZE;2],document.scene()).unwrap();
    let expected=Arc::new(crate::scene::SignatureFrame::capture(&r,packet,crate::scene::Declared::default()).unwrap());
    let mut scene=crate::scene::Scene::new(&r).unwrap();
    let back=scene.alloc(&r,wgpu::Color::TRANSPARENT).unwrap();
    let front=scene.alloc(&r,wgpu::Color::BLUE).unwrap();
    let views=[scene.pool[back].view.clone(),scene.pool[front].view.clone()];
    scene.source_jobs.push(0..2);
    scene.declare(&mut r,Inputs::default(),Declaration::scene(expected,None),|scene,r,_| {
        scene.combine(r,front,back,1.,layer_core::LayerBlend::Normal,false,layer_core::BlendSpace::Linear).map(drop)
    }).unwrap();
    assert_eq!(scene.jobs.len(),2,"a declaration must leave the retained prefix intact");
    for (job,(view,color)) in scene.jobs.iter().zip(views.into_iter().zip([wgpu::Color::TRANSPARENT,wgpu::Color::BLUE])) {
        assert!(matches!(job,crate::scene::Job::Clear(actual,clear) if *actual==view && *clear==color),
            "a declaration must not replace another unit's jobs");
    }
    assert_eq!(scene.source_jobs,vec![0..2]);
}

#[test]
fn a_refusal_behind_retiring_source_slots_agrees_with_its_fit_while_the_retirement_actor_is_held() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let display=crate::residency::Tier::retiring_into(&r.scene_tiers.retiring);
    let mut cache=DecodedTiles::within(&display,Default::default());cache.admit_limits(16*NORMALIZED_PAGE_BYTES);
    let source=scan_source(8,0);
    let planned:Vec<_>=(0..8).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    let page=crate::resident::Storage::storage_bytes(&planned[0].0);
    let resident=display.used()-4*page;
    let (entered,ready)=std::sync::mpsc::channel();let (release,held)=std::sync::mpsc::channel::<()>();
    r.device.retirement_handle().run(move||{let _=entered.send(());let _=held.recv_timeout(crate::READBACK_TIMEOUT);});
    ready.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    let retiring=r.scene_tiers.retiring.used();
    assert_eq!(evict(&display,&mut cache,4*page),4*page);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.flush_retired(&mut encoder);encoder.submit(&r.queue);
    assert_eq!(r.scene_tiers.retiring.used()-retiring,4*page,"retired source slots are retiring until their GPU work completes");
    let floor=8*page;
    display.set_limit(u64::MAX,floor);display.set_grant(resident+page+floor);
    display.begin_frame();
    let attempt=display.admit(crate::residency::Class::Prefetch,page);
    display.end_frame();
    release.send(()).unwrap();crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    assert_eq!(r.scene_tiers.retiring.used(),retiring);
    assert_eq!(display.used(),resident+attempt.as_ref().map_or(0,|reservation|reservation.bytes()));
    if let Err(blocked)=&attempt {assert!(!blocked.fits(),"demand reports the refused unit runnable while its next attempt is refused again");}
    drop((attempt,planned));
}

#[test]
fn evicted_decoded_slots_return_their_memory_beside_long_lived_textures() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let reserved=||r.device.generate_allocator_report().map(|report|report.total_reserved_bytes);
    let Some(start)=reserved() else {return;};
    let (display,mut cache)=standalone();
    let source=scan_source(192,0);
    let mut kept=Vec::new();
    for index in 0..192u32 {
        let (tile,_)=cache.plan_exact(&r,&source,[index%128,index/128]).unwrap();
        assert_eq!(tile.texture.format(),wgpu::TextureFormat::Rgba32Float);
        if index%4==3 {kept.push(r.device.create_texture(&wgpu::TextureDescriptor {label:Some("long-lived page"),
            size:wgpu::Extent3d {width:PAGE_SIZE*2,height:PAGE_SIZE*2,depth_or_array_layers:1},mip_level_count:1,sample_count:1,
            dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba16Float,usage:wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[]}));}
    }
    let kept_bytes=kept.iter().map(crate::texture_bytes).sum::<u64>();
    evict(&display,&mut cache,u64::MAX);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.flush_retired(&mut encoder);
    encoder.submit(&r.queue);crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    assert!(cache.slots.is_empty());
    let grown=reserved().unwrap().saturating_sub(start);
    assert!(grown<=kept_bytes+(16<<20),"{} MiB stay reserved for {} MiB of long-lived textures",grown>>20,kept_bytes>>20);
}
