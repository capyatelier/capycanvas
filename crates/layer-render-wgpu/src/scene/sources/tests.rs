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
        id, blob.descriptor, blob.compressed().unwrap(), None,
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
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
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
    let pixels = crate::test_support::float_pixels(r, cache.texture());
    for (i, actual) in pixels.into_iter().enumerate() {
        let id = (i as u32 % cache.plan.size[0] / (PAGE_SIZE >> cache.plan.level)) as u8 + offset;
        let codes = [id.wrapping_mul(17), id.wrapping_mul(43).wrapping_add(61), id.wrapping_mul(97).wrapping_add(11)];
        let luminance: f64 = codes.into_iter().zip(r.document_color().space.to_xyz()[1])
            .map(|(code, weight)| f64::from(code) / 255. * weight).sum();
        let expected = if luminance >= f64::from(threshold) { 1. } else { 0. };
        assert_eq!(actual, [expected, expected, expected, 1.], "offset={offset} threshold={threshold} pixel={i}");
    }
}

#[test]
fn repeated_native_threshold_updates_reuse_an_oversized_decoded_working_set() {
    use layer_core::{Document, DocumentNames, EffectInstance, EffectValue};
    use layer_core::authored::*;
    for capacity in [8, 16] {
        for tiles in [capacity + 1, capacity * 13 / 10 + 1] {
            let extent = [tiles as u32 * PAGE_SIZE, PAGE_SIZE];
            let mut doc = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
            let SourceTarget::Paint(paint) = doc.working.target.unwrap() else { unreachable!() };
            doc.artwork.paint.get_mut(paint).unwrap().original = Some(scan_source(tiles, 0));
            let program = crate::tests::fixture("threshold").program();
            let parameter = program.parameters.iter().position(|parameter| parameter.key.as_ref() == "threshold").unwrap();
            let definition = doc.artwork.definitions.insert(PortableId::random(), Definition { program: program.clone(), dimensions: Default::default() }).unwrap();
            let effect = doc.artwork.effects.insert(PortableId::random(), EffectApplication { definition, values: EffectInstance::new(program).values, domain: extent }).unwrap();
            let handle = doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "Threshold")).unwrap();
            let stack = doc.composition().result;
            doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0, handle);
            let mut doc = Document::from_artwork(doc.artwork).unwrap();
            let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            r.source_tiles.get_mut().limits.slots = capacity;
            r.native_edit.as_mut().unwrap().display_complete_bytes = u64::MAX;
            let mut allocated = None;
            for (round, threshold) in [0.2, 0.35, 0.5, 0.65, 0.8, 0.45].into_iter().enumerate() {
                doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(threshold);
                let mut frame = crate::test_support::packet(doc.scene(), extent);
                frame.view.width_px = extent[0] / 4; frame.view.height_px = extent[1] / 4;
                frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
                let before = r.source_cache_work();
                r.submit(frame).unwrap();
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
                assert!(cache.resident_bytes() > 0, "oversized decoded scans must exercise resident native output");
                assert_eq!(cache.plan.level, 2);
                assert_eq!(cache.plan.bounds, PixelRect::full(extent));
                assert_threshold_pixels(&r, threshold, 0);
            }
            let replacement = scan_source(capacity, 64);
            doc.artwork.paint.get_mut(paint).unwrap().original = Some(replacement);
            doc.artwork.paint.get_mut(paint).unwrap().domain[0] = capacity as u32 * PAGE_SIZE;
            let composition = doc.artwork.root;
            doc.artwork.compositions.get_mut(composition).unwrap().size[0] = capacity as u32 * PAGE_SIZE;
            let frame = crate::test_support::packet(doc.scene(), doc.composition().size);
            r.submit(frame).unwrap();
            assert_threshold_pixels(&r, 0.45, 64);
            doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(0.6);
            let frame = crate::test_support::packet(doc.scene(), doc.composition().size);
            let before = r.source_cache_work();
            r.submit(frame).unwrap();
            let after = r.source_cache_work();
            assert_eq!(after[1], before[1], "a replacement set within capacity must be warm on its next update");
            assert_eq!(r.source_tiles.borrow().gpu_bytes(), allocated.unwrap());
            assert_threshold_pixels(&r, 0.6, 64);
        }
    }
}

#[test]
fn discarded_source_decodes_cannot_be_reused_as_valid_pixels() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.source_tiles.get_mut().limits.slots = 2;
    let source = scan_source(2, 32);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.original_source_tile(&source, [0, 0], &mut encoder).unwrap();
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_some());
    drop(encoder);
    assert!(r.source_tiles.borrow().prepared_view(&source, [0, 0]).is_none());
    for x in 0..2 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let tile = r.original_source_tile(&source, [x, 0], &mut encoder).unwrap().unwrap();
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
}

#[test]
fn effect_mask_bindings_reuse_retire_and_bound_real_texture_views() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let layout = scene.effects.masks.clone();
    let views = std::array::from_fn::<_, { crate::effects::MASK_SLOTS }, _>(|_| r.empty_view.clone());
    let create = |views: &[wgpu::TextureView; crate::effects::MASK_SLOTS]| r.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mask binding lifetime oracle"), layout: &layout,
        entries: &std::array::from_fn::<_, { crate::effects::MASK_SLOTS }, _>(|i| wgpu::BindGroupEntry {
            binding: i as u32, resource: wgpu::BindingResource::TextureView(&views[i]),
        }),
    });
    let original = scene.mask_bindings.get(&views, || create(&views)).clone();
    scene.begin_frame();
    assert_eq!(*scene.mask_bindings.get(&views, || panic!("an unchanged mask set must reuse its binding")), original);
    scene.forget_bindings(&[views[0].clone()]);
    assert!(scene.mask_bindings.entries.is_empty());
    assert_ne!(*scene.mask_bindings.get(&views, || create(&views)), original);
    for _ in 0..=4096 {
        let mut distinct = views.clone();
        distinct[0] = r.empty_view.texture().create_view(&Default::default());
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
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
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
    let (_, pending) = exact.plan(&r, &source, [0, 0]).unwrap();
    let pending = pending.unwrap();
    let source_view = pending.view.clone();
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
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&pending.view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
        ],
    });
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.encode_decode(&mut r, Some(pending), &mut encoder).unwrap();
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline); pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(32, 32, 1);
    }
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    let read = crate::layer_tests::page_bytes(&r, &output);
    let mut maximum_error = 0_f64;
    for (i, p) in read.chunks_exact(16).enumerate() {
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
