use super::*;
use layer_core::color::source::{SourceBuilder, SourceInterpretation};

#[test]
fn borrowed_source_tiles_survive_eviction_and_release_their_capacity() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache = DecodedTiles {
        limits: SourceLimits { slots: 2, ..Default::default() },
        ..Default::default()
    };
    let key = |id| Key::Raster([id; 32], RgbSpace::Srgb, RgbSpace::Srgb);
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

#[test]
fn source_residency_and_upload_window_follow_admitted_headroom() {
    let gib = 1024 * 1024 * 1024;
    for (allowance, slots) in [(0, 64), (gib / 4, 64), (gib / 2 - 1, 127), (gib / 2, 128),
        (gib - 1, 255), (gib, 256), (3 * gib / 2, 384), (2 * gib, 512), (3 * gib, 768),
        (4 * gib, 1024), (u64::MAX, 1024)] {
        let limits = SourceLimits::admitted(allowance);
        assert_eq!(limits.slots, slots);
        assert_eq!(limits.upload_bytes, (slots as u64 * FLOAT_TILE_BYTES / 4).min(64 * 1024 * 1024));
        let mut cache = DecodedTiles { limits, ..Default::default() };
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
        let original = cache.admitted_bytes();
        let display = cache.split_display_cache();
        assert_eq!(cache.admitted_bytes()[0] + display.admitted_bytes()[0], original[0]);
        assert_eq!(cache.admitted_bytes()[1], original[1]);
        assert!(Arc::ptr_eq(&cache.in_flight, &display.in_flight));
    }
}

#[test]
fn compact_display_sources_preserve_srgb_codes_and_padding() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let scene = Scene::new(&r);
    let mut exact = DecodedTiles::new(RgbSpace::Srgb);
    exact.admit(2 * 1024 * 1024 * 1024);
    let mut display = exact.split_display_cache();
    let mut builder = SourceBuilder::new([255, 19], SourceInterpretation {
        channels: SourceChannels::Rgb, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 4 * 1024 * 1024).unwrap();
    for y in 0..19 {
        let row: Vec<_> = (0..255).flat_map(|x| [x as u8, (y * 11) as u8, (255 - x) as u8]).collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    assert!(display.accepts_display(&source));
    let (_, pending) = display.plan(&r, &source, [0, 0]).unwrap();
    let pending = pending.unwrap();
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
    let bytes = exact.encode(&mut r, &mut encoder, &pending, &scene.binding, 0).unwrap();
    assert_eq!(bytes, FLOAT_TILE_BYTES / 4);
    exact.charge_upload(&encoder, bytes);
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
    // Queries request a distinct, full-precision texture for the same source.
    let (query, pending_query) = exact.plan(&r, &source, [0, 0]).unwrap();
    assert_eq!(query.texture.format(), wgpu::TextureFormat::Rgba32Float);
    assert!(pending_query.is_some());
    drop(r);
    startup::finish_shader_compiler_shutdown();
}
