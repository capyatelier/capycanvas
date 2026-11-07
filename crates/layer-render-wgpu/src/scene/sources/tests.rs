use super::*;
use layer_core::color::source::{SourceBuilder, SourceInterpretation};

fn cache_with_slots(slots: usize) -> DecodedTiles {
    let mut cache = DecodedTiles::default();
    cache.admit_limits(slots as u64 * NORMALIZED_PAGE_BYTES, 16 * NORMALIZED_PAGE_BYTES, 44 * NORMALIZED_PAGE_BYTES);
    cache
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
    let mut cache = DecodedTiles {
        limits: SourceLimits { bytes: NORMALIZED_PAGE_BYTES / 2, ..Default::default() },
        ..Default::default()
    };
    let source = scan_source(4, 0);
    let key = |id: u32| Key::raster(&source.tiles[&[id - 1, 0]], RgbSpace::Srgb, RgbSpace::Srgb);
    let (first, first_write) = cache.plan_key(&r, key(1)).unwrap();
    let first_lease = cache.lease(&first.view).unwrap();
    let (second, second_write) = cache.plan_key(&r, key(2)).unwrap();
    let second_lease = cache.lease(&second.view).unwrap();
    assert!(matches!(cache.plan_key(&r, key(3)), Err(GpuRasterError::SourceWorkingSetExceeded)));
    let (hit, write) = cache.plan_key(&r, key(1)).unwrap();
    assert_eq!(hit.view, first.view);
    assert!(write.is_none());
    drop(second_lease);
    let (third, third_write) = cache.plan_key(&r, key(3)).unwrap();
    assert_eq!(third.view, second.view);
    assert_ne!(third.view, first.view);
    drop(first_lease);
    let (fourth, fourth_write) = cache.plan_key(&r, key(4)).unwrap();
    assert_eq!(fourth.view, first.view);
    assert_eq!(cache.gpu_bytes(), NORMALIZED_PAGE_BYTES / 2);
    drop((first_write, second_write, third_write, fourth_write));
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
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut leases = Vec::with_capacity(count);
    for x in 0..count as u32 {
        let prepared = crate::prepare_image_tile(&source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
        let tile=Scene::decode_prepared_for(r,&mut encoder,&source,[x,0],prepared,SourceUse::Exact).unwrap();
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
    r.publish_memory(true);
    budget.flush_reports();
    let pressure = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    r.begin_frame_memory(frame);
    let capacity = r.source_tiles.borrow().free_slots();
    let sources = r.source_tiles.borrow();
    let foreign_bytes = sources.slots.iter().filter(|slot| foreign.iter().any(|lease| Arc::ptr_eq(lease, &slot.lease)))
        .map(|slot| texture_bytes(&slot.texture)).sum::<u64>();
    assert_eq!(capacity as u64, sources.limits.bytes.saturating_sub(sources.non_slot_bytes())
        .saturating_sub(sources.retired_bytes.load(Ordering::Acquire)).saturating_sub(foreign_bytes) / NORMALIZED_PAGE_BYTES);
    assert!(capacity > SOURCE_SLOTS);
    drop(sources);
    (r, pressure, source, foreign, capacity)
}

fn assert_working_source_slots(r: &WgpuRasterizer, capacity: usize, foreign: &[crate::scene::sources::Lease]) {
    let sources = r.source_tiles.borrow();
    let retained = sources.slots.iter().filter(|slot| foreign.iter().any(|lease| Arc::ptr_eq(lease, &slot.lease))).count();
    assert_eq!(retained, foreign.len(), "unrelated source leases survive the traversal");
    assert!(sources.slots.iter().filter(|slot| foreign.iter().any(|lease| Arc::ptr_eq(lease, &slot.lease)))
        .all(|slot| slot.valid.load(Ordering::Acquire) && slot.texture.format() == wgpu::TextureFormat::Rgba32Float));
    assert!(sources.slots.iter().filter(|slot| !foreign.iter().any(|lease| Arc::ptr_eq(lease, &slot.lease))
        && slot.texture.format() == wgpu::TextureFormat::Rgba32Float).count() <= capacity);
    assert!(sources.gpu_bytes() <= sources.limits.bytes);
    assert_eq!(sources.gpu_bytes(), sources.slots.iter().map(|slot| texture_bytes(&slot.texture)).sum::<u64>()
        + sources.non_slot_bytes() + sources.retired_bytes.load(Ordering::Acquire));
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
    r.source_tiles.get_mut().limits.bytes = 2 * NORMALIZED_PAGE_BYTES + transfer::TABLE_BYTES + encoded_bytes;
    let source = scan_source(3, 32);
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
        assert_eq!(texture_bytes(&raw.texture), encoded_bytes);
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
    let upload_bytes = r.source_tiles.borrow().limits.upload_bytes;
    assert_eq!(r.source_tiles.borrow().in_flight.bytes.load(Ordering::Acquire), 0);
    assert_eq!(r.source_tiles.borrow().charge_upload(&pending, upload_bytes), upload_bytes);
    assert!(r.source_tiles.borrow().uploads_full());
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
    assert_eq!(r.source_tiles.borrow().in_flight.bytes.load(Ordering::Acquire), 0);
    assert!(!r.source_tiles.borrow().uploads_full());
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
    for (decoded, uploads) in [(0, 0), (NORMALIZED_PAGE_BYTES - 1, NORMALIZED_PAGE_BYTES - 1),
        (NORMALIZED_PAGE_BYTES, NORMALIZED_PAGE_BYTES), (3 * NORMALIZED_PAGE_BYTES + 1, 5 * NORMALIZED_PAGE_BYTES), (2048 * NORMALIZED_PAGE_BYTES, 8 * NORMALIZED_PAGE_BYTES)] {
        let limits = SourceLimits::admitted(decoded, uploads);
        assert_eq!(limits.bytes, decoded);
        assert_eq!(limits.upload_bytes, uploads);
        let cache = DecodedTiles { limits, ..Default::default() };
        let mut charges = Vec::new();
        while !cache.uploads_full() {
            let bytes = if charges.len() % 2 == 0 { NORMALIZED_PAGE_BYTES / 4 } else { NORMALIZED_PAGE_BYTES };
            let total = cache.in_flight.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
            charges.push(UploadCharge(cache.in_flight.clone(), bytes));
            assert!(total <= uploads);
        }
        drop(charges);
        assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
        assert_eq!(cache.uploads_full(), uploads < NORMALIZED_PAGE_BYTES);
        assert_eq!(cache.admitted_bytes(), [decoded, uploads]);
    }
}

#[test]
fn decoded_sources_preserve_srgb_codes_and_padding() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_factory(gpu.factory.clone(), gpu.device.clone(), Default::default()).unwrap();
    drop(gpu);
    r.source_tiles.get_mut().admit_limits(512 << 20, 64 << 20, 492 << 20);
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
    let mut cache=cache_with_slots(24);let source=scan_source(25,0);
    let first:Vec<_>=(0..24).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    let page_bytes=texture_bytes(&first[0].0.texture);
    let overhead=cache.non_slot_bytes();
    assert_eq!(first[0].0.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(cache.gpu_bytes(),24*page_bytes+overhead);
    let mip_bytes=cache.mip_budget();
    cache.admit_limits(20*page_bytes+overhead,16*NORMALIZED_PAGE_BYTES,mip_bytes);
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    assert_eq!(cache.trim_unused(&encoder),4*page_bytes);
    assert_eq!(cache.slots.len(),20);
    assert_eq!(cache.gpu_bytes(),24*page_bytes+overhead);
    assert!(cache.reserve_mips(mip_bytes,&encoder).unwrap());
    assert!(!cache.reserve_mips(mip_bytes+1,&encoder).unwrap());
    drop(first);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert_eq!(cache.gpu_bytes(),20*page_bytes+overhead);
    cache.admit_limits(24*page_bytes+overhead,16*NORMALIZED_PAGE_BYTES,mip_bytes);
    let second:Vec<_>=(0..24).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    cache.admit_limits(20*page_bytes+overhead,16*NORMALIZED_PAGE_BYTES,mip_bytes);
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    assert_eq!(cache.trim_unused(&encoder),4*page_bytes);
    drop((encoder,second));cache.reap_retired();
    assert_eq!(cache.retired_bytes.load(Ordering::Acquire),0);
    assert_eq!(cache.gpu_bytes(),20*page_bytes+overhead);
}

#[test]
fn canonical_source_requirements_survive_optional_uploads_with_separate_mip_grants() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = cache_with_slots(64);
    let page_bytes = u64::from(PAGE_SIZE).pow(2) * u64::from(wgpu::TextureFormat::Rgba8Unorm.block_copy_size(None).unwrap());
    let pages = (cache.admitted_bytes()[0] / page_bytes) as u32 + 64;
    let source = scan_source(pages as usize, 0);
    let columns = source.extent[0] / PAGE_SIZE;
    let coordinate = |index: u32| [index % columns, index / columns];
    cache.require_images((0..36).chain(0..36).map(|x| (source.clone(), [x, 0])));
    assert_eq!(cache.required.len(), 36);
    assert_eq!(cache.mip_budget(), 44 * NORMALIZED_PAGE_BYTES);
    let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for x in 0..36 {
        let (_, pending) = cache.plan(&r, &source, coordinate(x)).unwrap();
        pending.unwrap().write.track(&encoder);
    }
    assert!(cache.reserve_mips(cache.mip_budget(), &encoder).unwrap());
    for x in 36..pages {
        let (_, pending) = cache.plan(&r, &source, coordinate(x)).unwrap();
        pending.unwrap().write.track(&encoder);
        assert!((0..36).all(|x| cache.prepared_view(&source, coordinate(x)).is_some()));
        assert!(cache.gpu_bytes() <= cache.admitted_bytes()[0]);
    }
    cache.require_images(std::iter::empty());
    assert_eq!(cache.mip_budget(), 44 * NORMALIZED_PAGE_BYTES);
    for x in 36..pages {
        let (_, pending) = cache.plan(&r, &source, coordinate(x)).unwrap();
        if let Some(pending) = pending { pending.write.track(&encoder); }
    }
    assert!(cache.prepared_view(&source, [0, 0]).is_none());
    drop(encoder);
    assert!((0..36).all(|x| cache.prepared_view(&source, coordinate(x)).is_none()));
}

#[test]
fn discarded_required_source_writes_reuse_their_own_slots() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = cache_with_slots(64);
    let source = scan_source(1, 0);
    cache.require_images([(source.clone(), [0, 0])]);
    let (first, pending) = cache.plan(&r, &source, [0, 0]).unwrap();
    drop(pending);
    let (second, _) = cache.plan(&r, &source, [0, 0]).unwrap();
    assert_eq!(first.view, second.view);
    assert_eq!(cache.slots.len(), 1);
}

#[test]
fn canonical_source_demand_does_not_reduce_an_independent_mip_grant() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = cache_with_slots(64);
    let source = scan_source(36, 0);
    let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let previous = cache.mip_budget();
    assert!(cache.reserve_mips(previous, &encoder).unwrap());
    cache.require_images((0..36).map(|x| (source.clone(), [x, 0])));
    assert_eq!(cache.mip_budget(), previous);
    assert!(cache.reserve_mips(previous, &encoder).unwrap());
    for x in 0..36 {
        let (_, pending) = cache.plan(&r, &source, [x, 0]).unwrap();
        pending.unwrap().write.track(&encoder);
        assert!(cache.gpu_bytes() <= cache.admitted_bytes()[0]);
    }
    assert!(cache.reserve_mips(cache.mip_budget(), &encoder).unwrap());
    for x in 0..36 {
        let (_, pending) = cache.plan(&r, &source, [x, 0]).unwrap();
        if let Some(pending) = pending { pending.write.track(&encoder); }
    }
    assert!((0..36).all(|x| cache.prepared_view(&source, [x, 0]).is_some()));
    assert!(cache.gpu_bytes() <= cache.admitted_bytes()[0]);
}

#[test]
fn canonical_requirements_leave_streaming_capacity_beside_other_composition_leases() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = cache_with_slots(20);
    let page_bytes = u64::from(PAGE_SIZE).pow(2) * u64::from(wgpu::TextureFormat::Rgba8Unorm.block_copy_size(None).unwrap());
    let pages = (cache.admitted_bytes()[0] / page_bytes) as u32 + 20;
    let source = scan_source(pages as usize, 0);
    let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let previous = cache.mip_budget();
    assert!(cache.reserve_mips(previous, &encoder).unwrap());
    let (outside, pending) = cache.plan(&r, &source, [19, 0]).unwrap();
    pending.unwrap().write.track(&encoder);
    let lease = cache.lease(&outside.view).unwrap();
    cache.require_images((0..19).map(|x| (source.clone(), [x, 0])));
    assert_eq!(cache.mip_budget(), previous);
    for x in (0..19).chain(20..pages) {
        let (_, pending) = cache.plan(&r, &source, [x, 0]).unwrap();
        pending.unwrap().write.track(&encoder);
        assert_eq!(cache.prepared_view(&source, [19, 0]), Some(&outside.view));
        assert!((0..18.min(x + 1)).all(|x| cache.prepared_view(&source, [x, 0]).is_some()));
        assert!(cache.gpu_bytes() <= cache.admitted_bytes()[0]);
    }
    assert!(cache.reserve_mips(cache.mip_budget(), &encoder).unwrap());
    for x in 0..19 {
        let (_, pending) = cache.plan(&r, &source, [x, 0]).unwrap();
        if let Some(pending) = pending { pending.write.track(&encoder); }
    }
    assert!((0..19).all(|x| cache.prepared_view(&source, [x, 0]).is_some()));
    drop(lease);
    assert_eq!(cache.mip_budget(), previous);
}

#[test]
fn prepared_source_samples_upload_without_renderer_decompression_and_share_cached_pixels() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let source=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[64,128,192,128]);
    let bytes=Arc::new([64,128,192,128].repeat((PAGE_SIZE*PAGE_SIZE) as usize));
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
    let mut cache = DecodedTiles { limits: SourceLimits { bytes: 2 * NORMALIZED_PAGE_BYTES, ..SourceLimits::admitted(64 * NORMALIZED_PAGE_BYTES, 16 * NORMALIZED_PAGE_BYTES) }, ..cache_with_slots(64) };
    let source = scan_source(3, 0);
    let key = |x: u32| Key::raster(&source.tiles[&[x, 0]], RgbSpace::Srgb, RgbSpace::Srgb);
    let (resident, write) = cache.plan_key(&r, key(0)).unwrap();
    drop(cache.plan_key(&r, key(1)).unwrap());
    let (replacement, pending) = cache.plan_key(&r, key(2)).unwrap();
    assert_ne!(replacement.view, resident.view);
    let (hit, none) = cache.plan_key(&r, key(0)).unwrap();
    assert!(none.is_none() && hit.view == resident.view, "a deferred frame keeps decoded tiles resident");
    drop((write, pending));
}

#[test]
fn later_source_grants_change_residency_independently_of_display_allowance() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = cache_with_slots(64);
    for slots in [384, 64, 3] {
        cache.admit_limits(slots * NORMALIZED_PAGE_BYTES, 16 * NORMALIZED_PAGE_BYTES, 44 * NORMALIZED_PAGE_BYTES);
        assert_eq!(cache.admitted_bytes()[0], slots * NORMALIZED_PAGE_BYTES);
        let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        assert!(cache.reserve_mips(cache.mip_budget(), &encoder).unwrap());
        assert_eq!(cache.slot_limit() as u64, slots);
    }
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
    assert!(floor.source_tiles.borrow().admitted_bytes()[0] < pages * NORMALIZED_PAGE_BYTES);
    assert!(reference.source_tiles.borrow().admitted_bytes()[0] > pages * NORMALIZED_PAGE_BYTES);
    let [floor_cache, reference_cache] = [&floor, &reference].map(|r| r.scale_display.as_ref().unwrap());
    assert_eq!((floor_cache.plan.level, floor_cache.plan.bounds), (2, PixelRect::full(extent)));
    let texels = floor_cache.plan.size.into_iter().map(u64::from).product::<u64>();
    assert_eq!(floor.metrics.composited_pixels, texels, "resumed composition draws each output texel once");
    assert_eq!(crate::test_support::float_pixels(&floor, floor_cache.texture()),
        crate::test_support::float_pixels(&reference, reference_cache.texture()));
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
        if !r.object_deferred { break; }
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
    let prepared = crate::prepare_image_tile(source, [x, 0], r.document_color().space, &r.device.source_samples, None).unwrap();
    Scene::decode_prepared(r, encoder, source, [x, 0], prepared).unwrap_or_else(|error| {
        let cache = r.source_tiles.borrow();
        panic!("source {x}: {error}; {} slots, {} GPU bytes, {} granted", cache.slots.len(), cache.gpu_bytes(), cache.limits.bytes)
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
    assert_eq!(crate::texture_bytes(&sources.slots[0].texture), u64::from(PAGE_SIZE).pow(2) * if depth == SampleDepth::U8 {4} else {8});
    assert_eq!(sources.gpu_bytes(), crate::texture_bytes(&sources.slots[0].texture) + sources.non_slot_bytes());
}

#[test]
fn stored_source_u8_upload_retains_four_byte_codes() { assert_stored_code_upload(SampleDepth::U8); }

#[test]
fn stored_source_u16_upload_retains_eight_byte_packed_codes() { assert_stored_code_upload(SampleDepth::U16); }

#[test]
fn stored_source_encoded_slot_count_does_not_overstate_exact_working_capacity() {
    let source=stored_code_source(SampleDepth::U8,32);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let overhead=r.source_tiles.borrow().non_slot_bytes();
    r.source_tiles.get_mut().admit_limits(overhead+8*NORMALIZED_PAGE_BYTES,16*NORMALIZED_PAGE_BYTES,0);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    for x in 0..32 {stored_code_tile(&mut r,&source,x,&mut encoder);}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut cache=r.source_tiles.borrow_mut();
    assert_eq!(cache.slots.len(),32);
    assert_eq!(cache.free_slots(),8,"exact work needs normalized pages, not the count of smaller encoded pages");
    cache.require_images((0..32).map(|x|(source.clone(),[x,0])));
    assert_eq!(cache.required.len(),7,"leave space for the next exact page");
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
    r.source_tiles.borrow_mut().require_images([(source.clone(),[0,0])]);
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
fn stored_source_promotion_reuses_an_unleased_normalized_resource_under_pressure() {
    let source=stored_code_source(SampleDepth::U8,3);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let target=stored_code_tile(&mut r,&source,0,&mut encoder);
    let middle=stored_code_tile(&mut r,&source,1,&mut encoder);
    let reusable=Scene::promote_source(&mut r,&mut encoder,middle).unwrap();
    let untouched=stored_code_tile(&mut r,&source,2,&mut encoder);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let lease=r.source_tiles.borrow().lease(&target.view).unwrap();
    let grant=r.source_tiles.borrow().non_slot_bytes()+2*NORMALIZED_PAGE_BYTES;
    r.source_tiles.borrow_mut().admit_limits(grant,16*NORMALIZED_PAGE_BYTES,0);
    let before=r.source_tiles.borrow().gpu_bytes();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let promoted=Scene::promote_source(&mut r,&mut encoder,target).unwrap();
    assert_eq!(promoted.view,reusable.view,"ordinary LRU storage can serve the new representation");
    let cache=r.source_tiles.borrow();
    assert!(Arc::ptr_eq(&lease,&cache.lease(&promoted.view).unwrap()));
    assert!(cache.prepared_view(&source,[1,0]).is_none());
    assert_eq!(cache.prepared_view(&source,[2,0]).unwrap(),&untouched.view);
    assert_eq!(cache.gpu_bytes(),before,"reuse needs no new source allocation");
    assert!(cache.gpu_bytes()<=grant);drop(cache);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
}

#[test]
fn stored_source_new_exact_leases_do_not_retain_the_retired_encoded_representation() {
    let source=stored_code_source(SampleDepth::U16,1);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let encoded=stored_code_tile(&mut r,&source,0,&mut encoder);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let encoded_lease=r.source_tiles.borrow().lease(&encoded.view).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let exact=Scene::promote_source(&mut r,&mut encoder,encoded).unwrap();
    let exact_lease=r.source_tiles.borrow().lease(&exact.view).unwrap();
    assert!(Arc::ptr_eq(&encoded_lease,&exact_lease));
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert_eq!(r.source_tiles.borrow().retired_bytes.load(Ordering::Acquire),NORMALIZED_PAGE_BYTES/2);
    drop(encoded_lease);
    let cache=r.source_tiles.borrow();
    assert_eq!(cache.gpu_bytes(),NORMALIZED_PAGE_BYTES+cache.non_slot_bytes());
    assert_eq!(cache.retired_bytes.load(Ordering::Acquire),0);
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
        r.prepare_raw_neighborhood(target, [0, 0], [Some([0, 0])], false, &mut encoder).unwrap();
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
            r.prepare_raw_neighborhood(target, [0, 0], [Some([0, 0])], false, &mut encoder).unwrap();
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
        let overhead = r.source_tiles.borrow().non_slot_bytes();
        r.source_tiles.borrow_mut().admit_limits(overhead, 16 * NORMALIZED_PAGE_BYTES, 44 * NORMALIZED_PAGE_BYTES);
        let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        assert!(r.source_tiles.borrow_mut().trim_unused(&encoder) > 0);
        drop(encoder);
        assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_none());
        r.source_tiles.borrow_mut().admit_limits(overhead + 4 * NORMALIZED_PAGE_BYTES, 16 * NORMALIZED_PAGE_BYTES, 44 * NORMALIZED_PAGE_BYTES);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let reloaded = stored_code_tile(&mut r, &source, 0, &mut encoder);
        assert_eq!(reloaded.texture.format(), stored_code_format(depth));
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
    }
}

#[test]
fn stored_source_promotion_under_a_full_leased_grant_defers_without_overgrant() {
    let source = stored_code_source(SampleDepth::U8, 8);
    let (mut r, _doc, target) = stored_code_renderer(&source);
    let grant = r.source_tiles.borrow().non_slot_bytes() + 2 * NORMALIZED_PAGE_BYTES;
    r.source_tiles.borrow_mut().admit_limits(grant, 16 * NORMALIZED_PAGE_BYTES, 44 * NORMALIZED_PAGE_BYTES);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut leases = Vec::new();
    for x in 0..8 {
        let tile = stored_code_tile(&mut r, &source, x, &mut encoder);
        leases.push(r.source_tiles.borrow().lease(&tile.view).unwrap());
    }
    r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(&r);
    let before = r.source_tiles.borrow().gpu_bytes();
    assert_eq!(before, grant);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let result = r.prepare_raw_neighborhood(target, [0, 0], [Some([0, 0])], false, &mut encoder);
    assert!(matches!(result, Err(GpuRasterError::SourceWorkingSetExceeded | GpuRasterError::DeferredObjectWork)),
        "promotion cannot fit while every encoded slot is leased");
    assert!(r.source_tiles.borrow().gpu_bytes() <= grant);
    assert_eq!(r.source_tiles.borrow().slots.len(), 8);
    assert!(r.source_tiles.borrow().slots.iter().all(|slot| slot.texture.format() == wgpu::TextureFormat::Rgba8Unorm));
    drop((encoder, leases));
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    assert!(matches!(r.prepare_raw_neighborhood(target,[0,0],[Some([0,0])],false,&mut encoder),Err(GpuRasterError::DeferredObjectWork)));
    assert!(encoder.has_work(),"retirement must be submitted before admission can resume");
    assert!(r.source_tiles.borrow().gpu_bytes()<=grant);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    r.prepare_raw_neighborhood(target,[0,0],[Some([0,0])],false,&mut encoder).unwrap();
    assert!(r.source_tiles.borrow().gpu_bytes()<=grant);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert_eq!(r.source_tiles.borrow().prepared_view(&source,[0,0]).unwrap().texture().format(),wgpu::TextureFormat::Rgba32Float);
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
