use super::*;
use layer_core::color::source::{SourceBuilder, SourceInterpretation};

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
    }, &blob.decode().unwrap()).unwrap();
    assert_eq!(reinterpreted.compressed().unwrap(), blob.compressed().unwrap());
    assert!(!key.matches(&Key::raster(&reinterpreted, RgbSpace::Srgb, RgbSpace::Srgb)));
    assert!(!key.matches(&Key::raster(&blob, RgbSpace::DisplayP3, RgbSpace::Srgb)));
    assert!(!key.matches(&Key::raster(&blob, RgbSpace::Srgb, RgbSpace::DisplayP3)));
    let loaded = |id| Arc::new(TileBlob::from_verified_resource(
        id, blob.descriptor, blob.compressed().unwrap(),
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
    let mut r = WgpuRasterizer::native_capture_on_gpu(gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone(), Default::default()).unwrap();
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
        limits: SourceLimits { slots: 2, ..Default::default() },
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
    assert_eq!(cache.gpu_bytes(), 2 * FLOAT_TILE_BYTES);
    drop((first_write, second_write, third_write, fourth_write));
}

fn scan_source(tiles: usize, offset: u8) -> Arc<SourceImage> {
    layer_core::color::source::rgba8_source([tiles as u32 * PAGE_SIZE, PAGE_SIZE], |x, _| {
        let id = (x / PAGE_SIZE) as u8 + offset;
        [id.wrapping_mul(17), id.wrapping_mul(43).wrapping_add(61), id.wrapping_mul(97).wrapping_add(11), 255]
    })
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
    for capacity in [8, 16] {
        for tiles in [capacity + 1, capacity * 13 / 10 + 1] {
            let extent = [tiles as u32 * PAGE_SIZE, PAGE_SIZE];
            let (mut doc, paint, effect, parameter) = threshold_document(threshold_source(tiles, 0));
            let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            r.source_tiles.get_mut().limits.slots = capacity;
            let mut allocated = None;
            for (round, threshold) in [0.2, 0.35, 0.5, 0.65, 0.8, 0.45].into_iter().enumerate() {
                doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(threshold);
                let mut frame = crate::test_support::packet(doc.scene(), extent);
                frame.view.width_px = extent[0] / 4; frame.view.height_px = extent[1] / 4;
                frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
                let before = r.source_cache_work();
                crate::test_support::submit_published(&mut r, frame);
                let after = r.source_cache_work();
                let work = [after[0] - before[0], after[1] - before[1]];
                eprintln!("native source scan tiles={tiles} round={round} work={work:?}");
                if round > 0 { assert!(work[0] >= capacity as u64, "warm traversal must reuse resident tiles: {work:?}"); }
                let sources = r.source_tiles.borrow();
                if let Some(bytes) = allocated { assert_eq!(sources.gpu_bytes(), bytes); }
                else { allocated = Some(sources.gpu_bytes()); }
                assert_eq!(sources.slots.len(), capacity);
                drop(sources);
                let cache = r.scale_display.as_ref().unwrap();
                assert_eq!(cache.plan.level, 2);
                assert_eq!(cache.plan.bounds, PixelRect::full(extent));
                assert_threshold_pixels(&r, threshold, 0);
            }
            let replacement = threshold_source(capacity, 64);
            doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((replacement).into()));
            doc.artwork.paint.get_mut(paint).unwrap().domain[0] = capacity as u32 * PAGE_SIZE;
            let composition = doc.artwork.root;
            doc.artwork.compositions.get_mut(composition).unwrap().size[0] = capacity as u32 * PAGE_SIZE;
            let frame = crate::test_support::packet(doc.scene(), doc.composition().size);
            crate::test_support::submit_published(&mut r, frame);
            assert_threshold_pixels(&r, 0.45, 64);
            doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(0.6);
            let frame = crate::test_support::packet(doc.scene(), doc.composition().size);
            let before = r.source_cache_work();
            crate::test_support::submit_published(&mut r, frame);
            let after = r.source_cache_work();
            assert_eq!(after[1], before[1], "a replacement set within capacity must be warm on its next update");
            assert_eq!(r.source_tiles.borrow().gpu_bytes(), allocated.unwrap());
            assert_threshold_pixels(&r, 0.6, 64);
        }
    }
}

#[test]
fn discarded_source_decodes_cannot_be_reused_as_valid_pixels() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_gpu(gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone(), Default::default()).unwrap();
    drop(gpu);
    r.source_tiles.get_mut().limits.slots = 2;
    let source = scan_source(3, 32);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.paint_base_tile(&layer_core::authored::PaintBase::new(source.clone().into()), [0, 0], &mut encoder).unwrap();
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_some());
    drop(encoder);
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_none());
    for x in 0..2 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let tile = r.paint_base_tile(&layer_core::authored::PaintBase::new(source.clone().into()), [x, 0], &mut encoder).unwrap().unwrap();
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
fn source_residency_and_upload_window_follow_admitted_headroom() {
    let gib = 1024 * 1024 * 1024;
    for (allowance, slots) in [(0, 64), (gib / 4, 64), (gib / 2 - 1, 127), (gib / 2, 128),
        (gib - 1, 255), (gib, 256), (3 * gib / 2, 384), (2 * gib, 512), (3 * gib, 768),
        (4 * gib, 1024), (u64::MAX, 1024)] {
        let limits = SourceLimits::admitted(allowance);
        assert_eq!(limits.slots, slots);
        assert_eq!(limits.upload_bytes, (slots as u64 * FLOAT_TILE_BYTES / 4).min(64 * 1024 * 1024));
        let cache = DecodedTiles { limits, ..Default::default() };
        // Simulate already-admitted mixed uploads without a GPU allocation.
        // The next worst-case tile always fits; completion releases the charge.
        let mut charges = Vec::new();
        while !cache.uploads_full() {
            let bytes = if charges.len() % 2 == 0 { FLOAT_TILE_BYTES / 4 } else { FLOAT_TILE_BYTES };
            let total = cache.in_flight.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
            charges.push(UploadCharge(cache.in_flight.clone(), bytes));
            assert!(total <= limits.upload_bytes);
        }
        drop(charges);
        assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
        assert!(!cache.uploads_full());
        assert_eq!(cache.admitted_bytes(), [limits.slots as u64 * FLOAT_TILE_BYTES, limits.upload_bytes]);
    }
}

#[test]
fn decoded_sources_preserve_srgb_codes_and_padding() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_gpu(gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone(), Default::default()).unwrap();
    drop(gpu);
    let mut scene = Scene::new(&r).unwrap();
    let mut exact = DecodedTiles::new(RgbSpace::Srgb);
    exact.admit(2 * 1024 * 1024 * 1024);
    let mut builder = SourceBuilder::new([255, 19], SourceInterpretation {
        channels: SourceChannels::Rgb, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 4 * 1024 * 1024).unwrap();
    for y in 0..19 {
        let row: Vec<_> = (0..255).flat_map(|x| [x as u8, (y * 11) as u8, (255 - x) as u8]).collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let (source_tile, _) = exact.plan(&r, &source, [0, 0]).unwrap();
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
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    crate::test_support::query_ready(&mut r, &mut encoder, |r, encoder| {
        let (_, pending) = exact.plan(r, &source, [0, 0])?;
        scene.encode_decode(r, pending, encoder)
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
    let (query, pending_query) = exact.plan(&r, &source, [0, 0]).unwrap();
    assert_eq!(query.texture.format(), wgpu::TextureFormat::Rgba32Float);
    assert_eq!(query.view, source_view);
    assert!(pending_query.is_none());
    drop(r);
    startup::finish_shader_compiler_shutdown();
}

#[test]
fn source_mip_reservations_wait_for_retired_slots_and_release_discarded_charges() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache=DecodedTiles::default();let source=scan_source(25,0);
    let first:Vec<_>=(0..24).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    assert_eq!(cache.gpu_bytes(),24*FLOAT_TILE_BYTES);
    let bytes=cache.mip_budget();assert_eq!(bytes,44*FLOAT_TILE_BYTES);
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    assert!(!cache.reserve_mips(bytes,&encoder).unwrap());
    assert_eq!(cache.slots.len(),MIN_DECODED_SLOTS);
    assert_eq!(cache.gpu_bytes(),24*FLOAT_TILE_BYTES);
    encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert_eq!(cache.gpu_bytes(),MIN_DECODED_SLOTS as u64*FLOAT_TILE_BYTES);
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    assert!(cache.reserve_mips(bytes,&encoder).unwrap());
    assert!(cache.reserve_mips(bytes+1,&encoder).is_err());
    cache.reserve_mips(0,&encoder).unwrap();
    let second:Vec<_>=(0..24).map(|x|cache.plan(&r,&source,[x,0]).unwrap()).collect();
    assert!(!cache.reserve_mips(bytes,&encoder).unwrap());
    drop((encoder,first,second));
    cache.reap_retired();
    assert_eq!(cache.retired_bytes.load(Ordering::Acquire),0);
    assert_eq!(cache.gpu_bytes(),MIN_DECODED_SLOTS as u64*FLOAT_TILE_BYTES);
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
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let hit=Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],PreparedSourcePixels::NativeSamples(Arc::new(Vec::new()))).unwrap();
    assert_eq!(tile.view,hit.view);
    assert_eq!(r.source_cache_work(),[1,1]);
    let alpha=128./255.;
    let expected=[64,128,192].map(|code| RgbSpace::Srgb.decode(f64::from(code)/255.) as f32*alpha);
    for pixel in crate::test_support::float_pixels(&r,&tile.texture) {
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
    let mut bytes=Vec::with_capacity(FLOAT_TILE_BYTES as usize);
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
    let reads=r.device.source_samples.stats();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let tile=Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],prepared).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    let after=r.device.source_samples.stats();assert_eq!((reads.hits,reads.misses),(after.hits,after.misses));
    let alpha=192f32/255.;let value=RgbSpace::Srgb.decode(96./255.) as f32*alpha;
    for pixel in crate::test_support::float_pixels(&r,&tile.texture) {
        assert!((pixel[3]-alpha).abs()<2e-6);for channel in &pixel[..3] {assert!((*channel-value).abs()<2e-6);}
    }
}

#[test]
fn abandoned_source_writes_are_reused_before_resident_tiles_are_evicted() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = DecodedTiles { limits: SourceLimits { slots: 2, ..Default::default() }, ..Default::default() };
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
fn a_later_display_allowance_admits_matching_source_residency() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (allowance, slots) in [(3 << 29, 384), (0, 64)] {
        r.set_complete_display_allowance(allowance);
        assert_eq!(r.native_edit.as_ref().unwrap().color_cache_bytes, allowance.max(256<<20));
        assert_eq!(r.source_tiles.borrow().admitted_bytes()[0], slots * FLOAT_TILE_BYTES);
        let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let budget = r.source_tiles.borrow().mip_budget();
        assert!(r.source_tiles.get_mut().reserve_mips(budget, &encoder).unwrap());
    }
    assert_eq!(r.source_tiles.borrow().slot_limit(), MIN_DECODED_SLOTS);
}

#[test]
fn exact_native_batches_reading_more_tiles_than_are_free_shrink_until_they_fit() {
    let capacity = 8;
    let tiles = 2 * capacity + 3;
    let (doc, ..) = threshold_document(threshold_source(tiles, 0));
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.test.reference = true;
    r.source_tiles.get_mut().limits.slots = capacity;
    crate::test_support::submit_published(&mut r, crate::test_support::packet(doc.scene(), doc.composition().size));
    assert!(r.scale_display.as_ref().unwrap().evaluation == crate::scene::scale::Evaluation::Native);
    assert_eq!(r.source_tiles.borrow().slots.len(), capacity);
    assert_threshold_pixels(&r, 0.5, 0);
}

#[test]
fn native_threshold_larger_than_the_decoded_floor_publishes_every_page_once() {
    let extent = [2560, 1792];
    let (doc, ..) = threshold_document(layer_core::color::source::rgba8_source(extent, |x, y|
        [(x / 7) as u8, (y / 5) as u8, ((x + y) / 3) as u8, 255]));
    let render = |allowance| {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.source_tiles.get_mut().admit(allowance);
        let mut frame = crate::test_support::packet(doc.scene(), extent);
        frame.view.width_px = extent[0] / 4; frame.view.height_px = extent[1] / 4;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        crate::test_support::submit_published(&mut r, frame);
        r
    };
    let floor = render(0);
    let reference = render(1 << 32);
    let pages = page_coordinates(PixelRect::full(extent)).count() as u64;
    assert!(floor.source_tiles.borrow().admitted_bytes()[0] < pages * FLOAT_TILE_BYTES);
    assert!(reference.source_tiles.borrow().admitted_bytes()[0] > pages * FLOAT_TILE_BYTES);
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
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.source_tiles.get_mut().admit(1 << 32);
    let pages = page_coordinates(PixelRect::full(extent)).count() as u64;
    let frame = crate::test_support::packet(doc.scene(), extent);
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
