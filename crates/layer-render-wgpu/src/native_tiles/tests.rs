use super::*;
use crate::test_support::{page_texture as texture, upload_page as upload, prepare_raster};
use crate::{WgpuRasterizer, layer_tests::page_bytes};
use layer_core::color::RgbSpace;

#[test]
fn encoder_bindings_follow_tile_count_and_optional_tracking() {
    for in_place in [false,true] {for tracked in [false,true] {
        for (format,name) in [(Some(wgpu::TextureFormat::Rgba8Uint),"rgba8uint"),
            (Some(wgpu::TextureFormat::Rgba16Uint),"rgba16uint"),
            (Some(wgpu::TextureFormat::Rgba32Uint),"rgba32uint"),(None,"gray_alpha")] {
            for count in [1,2] {
                let (entries,source)=NativeTileEncoder::pipeline_source(in_place,tracked,count,format,name);
                let module=naga::front::wgsl::parse_str(&source).unwrap();
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
                let flags=module.global_variables.iter().filter(|(_,v)|v.name.as_deref().is_some_and(|n|n.starts_with("changes"))).count();
                assert_eq!(flags,if tracked {count} else {0});
                let mut bindings:Vec<_>=entries.iter().map(|entry|entry.binding).collect();bindings.sort();
                let shared=count as u32*if in_place {2} else {3};
                assert_eq!(bindings,(0..shared+2+flags as u32).collect::<Vec<_>>());
            }
        }
    }}
}

fn working_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}
fn submit(
    r: &WgpuRasterizer,
    encoder: &NativeTileEncoder,
    batches: &[NativeTileBatch],
) {
    let mut commands = r.device.create_command_encoder(&Default::default());
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        for batch in batches {
            encoder.encode(&mut pass, batch);
        }
    }
    r.queue.submit([commands.finish()]);
}
fn format(depth: SampleDepth) -> wgpu::TextureFormat {
    if depth == SampleDepth::U8 {
        wgpu::TextureFormat::Rgba8Uint
    } else {
        wgpu::TextureFormat::Rgba16Uint
    }
}
fn code(bytes: &[u8], component: usize, depth: SampleDepth) -> u32 {
    if depth == SampleDepth::U8 {
        u32::from(bytes[component])
    } else {
        u32::from(u16::from_le_bytes(
            bytes[component * 2..component * 2 + 2].try_into().unwrap(),
        ))
    }
}
fn reference(
    pixel: [f32; 4],
    depth: SampleDepth,
    space: RgbSpace,
    alpha: AlphaAssociation,
) -> [u32; 4] {
    let max = depth.maximum() as f64;
    let coverage = (f64::from(pixel[3]) * max).round() as u32;
    if coverage == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    out[3] = coverage;
    for c in 0..3 {
        // Reference follows the declared Float32 unassociation, then uses Float64
        // transfer/rounding independently of the GPU table and analytic estimate.
        let limit = if alpha == AlphaAssociation::Straight {
            pixel[3]
        } else {
            1.
        };
        let mut value = pixel[c].clamp(0., limit);
        if alpha == AlphaAssociation::Straight {
            value /= pixel[3];
        }
        out[c] = (space.encode(f64::from(value)).clamp(0., 1.) * max).round() as u32;
    }
    out
}
fn native_boundary(space: RgbSpace, maximum: u32, code: u32) -> f32 {
    let boundary = space.decode((f64::from(code) + 0.5) / f64::from(maximum));
    let rounded = boundary as f32;
    if f64::from(rounded) < boundary { rounded.next_up() } else { rounded }
}

#[test]
fn sdr_endpoints_and_adjacent_boundaries_match_native_transfer() {
    for space in RgbSpace::ALL {
        assert_eq!((space.decode(0.) as f32).to_bits(), 0);
        assert_eq!((space.decode(1.) as f32).to_bits(), 1f32.to_bits());
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let maximum = depth.maximum();
            assert_eq!(maximum * (65535 / maximum), 65535);
            for (value, code) in [(0f32, 0), (1., maximum)] {
                assert_eq!((space.encode(f64::from(value)) * f64::from(maximum)).round() as u32, code);
            }
            for code in [0, maximum - 1] {
                let boundary = native_boundary(space, maximum, code);
                assert!(boundary > 0. && boundary < 1.);
                for (value, expected) in [(boundary.next_down(), code), (boundary, code + 1), (boundary.next_up(), code + 1)] {
                    assert_eq!((space.encode(f64::from(value)) * f64::from(maximum)).round() as u32, expected,
                        "{space:?} {depth:?} code={code} value={value}");
                }
            }
        }
    }
}
fn assert_pixels(
    actual: &[u8],
    pixels: &[[f32; 4]],
    depth: SampleDepth,
    space: RgbSpace,
    alpha: AlphaAssociation,
    region: [u32; 4],
    sentinel: u32,
) {
    let mut errors = [0; 4];
    let mut first = None;
    for (i, (bytes, pixel)) in actual
        .chunks_exact(4 * depth.bytes())
        .zip(pixels)
        .enumerate()
    {
        let x = i as u32 % 256;
        let y = i as u32 / 256;
        let inside = x >= region[0]
            && x < region[0] + region[2]
            && y >= region[1]
            && y < region[1] + region[3];
        let expected = if inside {
            reference(*pixel, depth, space, alpha)
        } else {
            [sentinel; 4]
        };
        for c in 0..4 {
            let got = code(bytes, c, depth);
            errors[c] = errors[c].max(got.abs_diff(expected[c]));
            if got != expected[c] && first.is_none() {
                first = Some((i, c, got, expected[c], *pixel));
            }
        }
    }
    assert_eq!(
        errors, [0; 4],
        "{depth:?} {space:?} {alpha:?} first={first:?}"
    );
}

#[test]
fn native_writeback_codes_boundaries_and_partial_tiles() { writeback_corpus(false); }

#[test]
fn native_in_place_writeback_codes_boundaries_and_partial_tiles() { writeback_corpus(true); }

#[test]
fn four_storage_buffer_devices_batch_rgba_and_bound_gray_native_writes() {
    portable_native_writes(4);
}

#[test]
fn eight_storage_buffer_devices_batch_rgba_and_bound_gray_native_writes() {
    portable_native_writes(8);
}

fn portable_native_writes(storage_buffers: u32) {
    use layer_core::color::LayerColorMode;
    let instance = WgpuRasterizer::headless_instance();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default()
    })).unwrap();
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let features = adapter.features() & (wgpu::Features::FLOAT32_FILTERABLE
        | wgpu::Features::FLOAT32_BLENDABLE | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    limits.max_storage_buffers_per_shader_stage = storage_buffers;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features, required_limits: limits, ..Default::default()
    })).unwrap();
    let mut r = WgpuRasterizer::native_capture_on_gpu(adapter, device.into(), queue, Default::default()).unwrap();
    assert_eq!(r.device.limits().max_storage_buffers_per_shader_stage, storage_buffers);
    let working: Vec<_> = (0..2).map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float)).collect();
    let canonical: Vec<_> = (0..2).map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float)).collect();
    let rgba: Vec<_> = (0..2).map(|_| texture(&r, wgpu::TextureFormat::Rgba8Uint)).collect();
    let gray: Vec<_> = (0..2).map(|_| r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("limited-device native gray bytes"), size: 256 * 256 * 2,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false,
    })).collect();
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let pixels = vec![[0.3, 0.3, 0.3, 0.5]; 256 * 256];
    for in_place in [false, true] {
        let encoder = if in_place { NativeTileEncoder::new(&r.device, true) } else { NativeTileEncoder::with_device(&r.device) };
        for mode in [LayerColorMode::FullColor, LayerColorMode::Grayscale, LayerColorMode::TwoTone] {
            for image in &working { upload(&r, image, &working_bytes(&pixels)); }
            let requests: Vec<_> = (0..2).map(|i| NativeTileRequest {
                working: &working[i], canonical: if in_place { &working[i] } else { &canonical[i] },
                encoded: if mode == LayerColorMode::FullColor { (&rgba[i]).into() } else { (&gray[i]).into() },
                mode, space: RgbSpace::Srgb, transfer: &transfer, depth: SampleDepth::U8,
                alpha: AlphaAssociation::Straight, region: [0, 0, 256, 256],
            }).collect();
            let batch = encoder.prepare(&r.device, &requests, &mut Default::default()).unwrap();
            assert_eq!(batch.jobs.len(), if mode == LayerColorMode::FullColor { 1 } else { 2 },
                "{storage_buffers}-buffer device {mode:?} in_place={in_place}");
            submit(&r, &encoder, &[batch]);
            for i in 0..2 {
                if mode == LayerColorMode::FullColor {
                    assert_pixels(&page_bytes(&r, &rgba[i]), &pixels, SampleDepth::U8, RgbSpace::Srgb, AlphaAssociation::Straight, [0, 0, 256, 256], 0);
                } else {
                    let actual = pollster::block_on(crate::local_tone::read_buffer_async(&r.device, &r.queue, &gray[i])).unwrap();
                    let expected = if mode == LayerColorMode::TwoTone { [255, 255] }
                        else { let rgba = reference(pixels[0], SampleDepth::U8, RgbSpace::Srgb, AlphaAssociation::Straight); [rgba[0] as u8, rgba[3] as u8] };
                    let differing = actual.as_chunks::<2>().0.iter().filter(|pixel| **pixel != expected).count();
                    assert_eq!(differing, 0, "{storage_buffers}-buffer device {mode:?} in_place={in_place} native bytes expected={expected:?}");
                }
            }
        }
    }
}

#[test]
fn gray_alpha_packed_writes_preserve_unedited_half_words() {
    use layer_core::{color::LayerColorMode, raster::RasterTile};
    use crate::raster::{CaptureSource, TileCapture};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let candidate = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let output = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gray alpha partial words"), size: 256 * 256 * 2,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pixels = (0..65536).map(|i| {
        let value = (i % 256) as f32 / 255.; [value; 4]
    }).collect::<Vec<_>>();
    for in_place in [false, true] {
        let encoder = if in_place { NativeTileEncoder::new(&r.device, true) } else { NativeTileEncoder::with_device(&r.device) };
        for mode in [LayerColorMode::Grayscale, LayerColorMode::TwoTone] {
            for region in [[0, 0, 256, 256], [1, 7, 253, 243], [3, 255, 1, 1], [255, 0, 1, 256], [17, 31, 0, 71]] {
                upload(&r, &working, &working_bytes(&pixels));
                r.queue.write_buffer(&output, 0, &vec![0xa5; output.size() as usize]);
                let request = NativeTileRequest {
                    working: &working, canonical: if in_place { &working } else { &candidate }, encoded: (&output).into(),
                    mode, space: RgbSpace::Srgb, transfer: &transfer, depth: SampleDepth::U8, alpha: AlphaAssociation::Straight, region,
                };
                let descriptor = request.descriptor();
                let batch = encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
                submit(&r, &encoder, &[batch]);
                let tile = RasterTile::pending(descriptor);
                r.capture_tiles(&[TileCapture { source: CaptureSource::Packed(&output), tile: tile.clone() }]).unwrap().finish().unwrap();
                let bytes = tile.wait_backing().unwrap().decode().unwrap();
                for (i, code) in bytes.as_chunks::<2>().0.iter().enumerate() {
                    let x = i as u32 % 256; let y = i as u32 / 256;
                    if x >= region[0] && x < region[0] + region[2] && y >= region[1] && y < region[1] + region[3] {
                        let alpha = if mode == LayerColorMode::TwoTone { if pixels[i][3] >= 0.5 { 255 } else { 0 } } else { (i % 256) as u8 };
                        assert_eq!(code[1], alpha, "{mode:?} {region:?} pixel {i}");
                        assert_eq!(code[0], if alpha == 0 { 0 } else { 255 });
                    } else { assert_eq!(*code, [0xa5; 2], "{mode:?} {region:?} untouched {i}"); }
                }
            }
        }
    }
}

#[test]
fn reduced_color_writeback_stores_invalid_pixels_as_defined_coverage() {
    use layer_core::color::LayerColorMode;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let candidate = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    for mode in [LayerColorMode::Grayscale, LayerColorMode::TwoTone] {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let output = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("invalid reduced color pixels"), size: mode.descriptor(layer_core::color::DocumentColor { depth, ..Default::default() }).byte_len([256; 2]).unwrap() as u64,
                usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false,
            });
            for (pixel, alpha) in [([f32::NAN, 0., 0., 1.], 1.), ([f32::INFINITY, 0., 0., 1.], 1.), ([f32::NEG_INFINITY, 1., 0., 0.5], 0.5),
                ([f32::INFINITY, f32::NEG_INFINITY, 0., 1.], 1.), ([0., 0., 0., -0.1], 0.), ([0., 0., 0., 1.1], 1.), ([0., 0., 0., f32::NAN], 0.)] {
                upload(&r, &working, &working_bytes(&vec![pixel; 65536]));
                let request = NativeTileRequest { working: &working, canonical: &candidate, encoded: (&output).into(), mode, space: RgbSpace::Srgb,
                    transfer: &transfer, depth, alpha: AlphaAssociation::Straight, region: [0, 0, 256, 256] };
                let batch = encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
                submit(&r, &encoder, &[batch]);
                let stored = page_bytes(&r, &candidate);
                let stored = stored.as_chunks::<4>().0;
                assert!(stored.iter().all(|v| f32::from_le_bytes(*v).is_finite()), "{mode:?} {depth:?} {pixel:?} saturates");
                let coverage = f32::from_le_bytes(stored[3]);
                assert!((0. ..=1.).contains(&coverage) && (alpha == 0.5 || coverage == alpha), "{mode:?} {depth:?} {pixel:?} coverage {coverage}");
            }
        }
    }
}

fn before_endpoint_encoder(r: &WgpuRasterizer, in_place: bool) -> NativeTileEncoder {
    let mut encoder = NativeTileEncoder::new(&r.device, in_place);
    let mut index = 0;
    for (format, name) in [(Some(wgpu::TextureFormat::Rgba8Uint), "rgba8uint"),
        (Some(wgpu::TextureFormat::Rgba16Uint), "rgba16uint"),
        (Some(wgpu::TextureFormat::Rgba32Uint), "rgba32uint"), (None, "gray_alpha")] {
        for tracked in [false, true] {
            for count in 1..=if format.is_some() {encoder.tiles_per_dispatch} else {encoder.gray_tiles_per_dispatch} {
                let (_, source) = NativeTileEncoder::pipeline_source(in_place, tracked, count, format, name);
                let source = source.replace("\r\n", "\n");
                let decode = "fn decode_quantized(code:u32)->f32 {\n    if code==0u {return 0.;}\n    if code==65535u {return 1.;}\n    return transfer[code].x;\n}\n";
                let endpoints = "    if value==0. {return 0u;}\n    if value==1. {return settings.maximum;}\n";
                assert!(source.contains(decode) && source.contains(endpoints));
                let source = source.replace(decode, "").replace(endpoints, "")
                    .replace("vec3(decode_quantized(code.r),decode_quantized(code.g),decode_quantized(code.b))",
                        "vec3(transfer[code.r].x,transfer[code.g].x,transfer[code.b].x)");
                let layout = r.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("native endpoint baseline"), bind_group_layouts: &[Some(&encoder.layouts[index])], immediate_size: 0,
                });
                encoder.pipelines[index] = crate::Deferred::compute(&r.device, "native endpoint baseline", &layout,
                    &crate::Deferred::wgsl(&r.device, "native endpoint baseline", source), "main");
                index += 1;
            }
        }
    }
    encoder
}

fn writeback_corpus(in_place: bool) {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let encoder = if in_place { NativeTileEncoder::new(&r.device, true) } else { NativeTileEncoder::with_device(&r.device) };
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = if in_place { working.clone() } else { texture(&r, wgpu::TextureFormat::Rgba32Float) };
    let outputs = [
        texture(&r, format(SampleDepth::U8)),
        texture(&r, format(SampleDepth::U16)),
    ];
    let baseline = before_endpoint_encoder(&r, in_place);
    let mut tables = transfer::Tables::default();
    let mut cases = 0;
    for space in RgbSpace::ALL {
        let transfer = tables.prepare(&r.device, space).unwrap();
        for (depth, encoded) in [SampleDepth::U8, SampleDepth::U16]
            .into_iter()
            .zip(&outputs)
        {
            let max = depth.maximum();
            for alpha in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                for mode in 0..12 {
                    let first = native_boundary(space, max, 0);
                    let last = native_boundary(space, max, max - 1);
                    let endpoints = [0., -0., f32::from_bits(1), 1., 1f32.next_down(), 1f32.next_up(),
                        first.next_down(), first, first.next_up(), last.next_down(), last, last.next_up()];
                    let pixels: Vec<[f32; 4]> = (0..65536u32)
                        .map(|i| {
                            let coverage = match mode {
                                0..=4 => {
                                    [1., 1. / max as f32, 2. / max as f32, 17. / max as f32, 0.]
                                        [mode]
                                }
                                5..=7 => {
                                    let boundary = (f64::from(i % max) + 0.5) / f64::from(max);
                                    let rounded = boundary as f32;
                                    [rounded.next_down(), rounded, rounded.next_up()][mode - 5]
                                }
                                8 => 0.25 / max as f32,
                                10 => [1., 1. / max as f32, 0.37, 0.5, 0., 1e-30, 1e-8][i as usize % 7],
                                11 if i as usize / 7 % endpoints.len() < 6 => [1., 1. / max as f32, 0.37, 0.5, 0., 1e-30, 1e-8][i as usize % 7],
                                _ => 1.,
                            };
                            let mut p = [0.; 4];
                            p[3] = coverage;
                            for c in 0..3 {
                                let n = i.wrapping_mul([1, 101, 237][c]) & max;
                                let linear = if mode == 10 {
                                    endpoints[(i as usize + c * 5) % endpoints.len()]
                                } else if mode == 11 {
                                    endpoints[i as usize / 7 % endpoints.len()]
                                } else if mode == 9 {
                                    let boundary =
                                        space.decode((f64::from(n) + 0.5) / f64::from(max)) as f32;
                                    [boundary.next_down(), boundary, boundary.next_up()][c]
                                } else {
                                    space.decode(f64::from(n) / f64::from(max)) as f32
                                };
                                p[c] = if alpha == AlphaAssociation::Straight {
                                    linear * coverage
                                } else {
                                    linear
                                };
                            }
                            if i < 8 && mode == 8 {
                                p[0] = f32::MAX;
                            }
                            p
                        })
                        .collect();
                    let bytes = working_bytes(&pixels);
                    upload(&r, &working, &bytes);
                    upload(&r, &canonical, &bytes);
                    let seed = vec![0x39; 256 * 256 * 4 * depth.bytes()];
                    upload(&r, encoded, &seed);
                    let region = if mode == 4 {
                        [13, 7, 229, 231]
                    } else {
                        [0, 0, 256, 256]
                    };
                    let batch = encoder
                        .prepare(
                            &r.device,
                            &[NativeTileRequest { mode: Default::default(), space,
                                working: &working,
                                canonical: &canonical,
                                encoded: encoded.into(),
                                transfer,
                                depth,
                                alpha,
                                region,
                            }], &mut Default::default(),
                        )
                        .unwrap();
                    submit(&r, &encoder, &[batch]);
                    let native_bytes = page_bytes(&r, encoded);
                    let canonical_bytes = page_bytes(&r, &canonical);
                    if mode >= 10 {
                        upload(&r, &working, &bytes);
                        upload(&r, &canonical, &bytes);
                        upload(&r, encoded, &seed);
                        let batch = baseline.prepare(&r.device, &[NativeTileRequest { mode: Default::default(), space,
                            working: &working, canonical: &canonical, encoded: encoded.into(), transfer,
                            depth, alpha, region }], &mut Default::default()).unwrap();
                        submit(&r, &baseline, &[batch]);
                        let before_native = page_bytes(&r, encoded);
                        let before_canonical = page_bytes(&r, &canonical);
                        assert!(native_bytes == before_native, "endpoint native differs from baseline {depth:?} {space:?} {alpha:?} in_place={in_place} mode={mode}");
                        assert!(canonical_bytes == before_canonical, "endpoint canonical bits differ from baseline {depth:?} {space:?} {alpha:?} in_place={in_place} mode={mode}");
                        if mode == 10 {
                            let first = pixels.iter().enumerate().find_map(|(i, pixel)| {
                                let expected = reference(*pixel, depth, space, alpha);
                                (0..4).find_map(|c| {
                                    let actual = code(&before_native[i * 4 * depth.bytes()..], c, depth);
                                    (actual != expected[c]).then_some((i, c, actual, expected[c], *pixel))
                                })
                            });
                            eprintln!("ENDPOINT_BASELINE {depth:?} {space:?} {alpha:?} in_place={in_place} cpu_first={first:?} pixels=65536 exact_native_and_canonical=true");
                            cases += 1;
                            continue;
                        }
                    }
                    assert_pixels(
                        &native_bytes,
                        &pixels,
                        depth,
                        space,
                        alpha,
                        region,
                        if depth == SampleDepth::U8 {
                            0x39
                        } else {
                            0x3939
                        },
                    );
                    if !in_place { assert!(page_bytes(&r, &working) == bytes, "writeback modified working source"); }
                    for (i, encoded) in canonical_bytes.as_chunks::<16>().0.iter().enumerate() {
                        let x = i as u32 % 256;
                        let y = i as u32 / 256;
                        if x < region[0]
                            || x >= region[0] + region[2]
                            || y < region[1]
                            || y >= region[1] + region[3]
                        {
                            assert!(
                                encoded == &bytes[i * 16..i * 16 + 16],
                                "canonical changed outside region at {i}"
                            );
                            continue;
                        }
                        let codes = reference(pixels[i], depth, space, alpha);
                        let coverage = codes[3] as f32 / max as f32;
                        for c in 0..4 {
                            let actual =
                                f32::from_le_bytes(encoded[c * 4..c * 4 + 4].try_into().unwrap());
                            let expected = if c == 3 {
                                coverage
                            } else {
                                let decoded =
                                    space.decode(f64::from(codes[c]) / f64::from(max)) as f32;
                                if alpha == AlphaAssociation::Straight {
                                    decoded * coverage
                                } else {
                                    decoded
                                }
                            };
                            if mode == 11 && (coverage == 0. || coverage == 1.) {
                                assert_eq!(actual.to_bits(), expected.to_bits(),
                                    "endpoint canonical {depth:?} {space:?} {alpha:?} pixel={i} component={c}");
                            }
                            assert!(
                                (actual - expected).abs()
                                    <= 0.00000024 * expected.abs().max(0.0000001),
                                "canonical {depth:?} {space:?} {alpha:?} pixel={i} component={c} actual={actual} expected={expected}"
                            );
                        }
                    }
                    cases += 1;
                }
            }
        }
    }
    eprintln!(
        "NATIVE_WRITEBACK in_place={in_place} cases={cases} pixels={} max_code_error=0",
        cases * 65536
    );
}

#[test]
fn native_restore_writeback_capture_round_trip_preserves_committed_codes() {
    use crate::raster::TileCapture;
    use layer_core::raster::RasterTile;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let encoded8 = texture(&r, wgpu::TextureFormat::Rgba8Uint);
    let encoded16 = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    for space in RgbSpace::ALL {
        let transfer = r.prepare_native_transfer(space).unwrap();
        assert_eq!(transfer, r.prepare_native_transfer(space).unwrap());
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let maximum = depth.maximum();
            for association in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                let encoded = if depth == SampleDepth::U8 {
                    &encoded8
                } else {
                    &encoded16
                };
                let pixels: Vec<_> = (0..65536u32)
                    .map(|i| {
                        let a = [0, 1, 2, 17, maximum / 2, maximum][i as usize % 6] as f32
                            / maximum as f32;
                        let rgb: [f32; 3] = std::array::from_fn(|c| {
                            space.decode(
                                f64::from(i.wrapping_mul([1, 101, 237][c]) & maximum)
                                    / f64::from(maximum),
                            ) as f32
                                * a
                        });
                        [rgb[0], rgb[1], rgb[2], a]
                    })
                    .collect();
                upload(&r, &working, &working_bytes(&pixels));
                let request = NativeTileRequest { mode: Default::default(), space: RgbSpace::Srgb,
                    working: &working,
                    encoded: encoded.into(),
                    canonical: &canonical,
                    transfer: &transfer,
                    depth,
                    alpha: association,
                    region: [0, 0, 256, 256],
                };
                let descriptor = request.descriptor();
                let batch = encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
                let mut first = None;
                for cycle in 0..4 {
                    submit(&r, &encoder, std::slice::from_ref(&batch));
                    let ticket = RasterTile::pending(descriptor);
                    let capture = r
                        .capture_tiles(
                            &[TileCapture {
                                source: crate::raster::CaptureSource::Texture(encoded),
                                tile: ticket.clone(),
                            }],
                        )
                        .unwrap();
                    capture.finish().unwrap();
                    let backing = ticket.wait_backing().unwrap();
                    let bytes = backing.decode().unwrap();
                    if let Some(first) = &first {
                        assert!(
                            first == &bytes,
                            "native code drift {space:?} {depth:?} {association:?} cycle={cycle}"
                        );
                    } else {
                        first = Some(bytes);
                    }
                    prepare_raster(&mut r,|r|r.restore_native_tiles(&[NativeTileRestore {
                        blob: &backing,
                        space,
                        destination: space,
                        working: &working,
                    }]))
                    .unwrap();
                    let restored = page_bytes(&r, &working);
                    let canonical_bytes = page_bytes(&r, &canonical);
                    let mut max_error = 0f32;
                    for (a, b) in restored
                        .as_chunks::<4>().0.iter()
                        .zip(canonical_bytes.as_chunks::<4>().0.iter())
                    {
                        let a = f32::from_le_bytes(*a);
                        let b = f32::from_le_bytes(*b);
                        max_error = max_error.max((a - b).abs());
                    }
                    assert!(max_error <= 0.00000024, "canonical mismatch: {max_error}");
                }
            }
        }
    }
}

#[test]
fn half_gray_alpha_publication_saturates_and_matches_canonical_pixels() {
    use layer_core::{color::{DocumentColor,LayerColorMode,f16},raster::RasterTile};
    use crate::raster::{CaptureSource,TileCapture};
    let mut r=WgpuRasterizer::new_native_headless(DocumentColor {depth:SampleDepth::F16,..Default::default()}).unwrap();
    let working=texture(&r,wgpu::TextureFormat::Rgba32Float);
    let candidate=texture(&r,wgpu::TextureFormat::Rgba32Float);
    let transfer=r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let output=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("half gray alpha saturation"),size:256*256*4,
        usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false});
    let pixels:Vec<_>=(0..65536).map(|i| {
        let alpha=[1.,0.37,1./65536.,1./16777216.,1./67108864.,0.,-0.][i%7];
        let rgb=if i/7%2==0 {65536.*alpha} else {-65536.*alpha};
        [rgb,rgb,rgb,alpha]
    }).collect();
    for in_place in [false,true] {
        upload(&r,&working,&working_bytes(&pixels));
        let canonical=if in_place {&working} else {&candidate};
        let encoder=NativeTileEncoder::new(&r.device, in_place);
        let request=NativeTileRequest {working:&working,canonical,encoded:(&output).into(),mode:LayerColorMode::Grayscale,
            space:RgbSpace::Srgb,transfer:&transfer,depth:SampleDepth::F16,alpha:AlphaAssociation::Straight,region:[0,0,256,256]};
        let tile=RasterTile::pending(request.descriptor());
        let batch=encoder.prepare(&r.device,&[request], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        r.capture_tiles(&[TileCapture {source:CaptureSource::Packed(&output),tile:tile.clone()}]).unwrap().finish().unwrap();
        let bytes=tile.wait_backing().unwrap().decode().unwrap();
        let cache=page_bytes(&r,canonical);
        for (i,((encoded,linear),pixel)) in bytes.as_chunks::<4>().0.iter().zip(cache.as_chunks::<16>().0.iter()).zip(&pixels).enumerate() {
            let alpha=f16::from_f32(pixel[3]);
            let expected=if alpha.to_bits()&32767==0 {[0u16;2]} else {[f16::from_f32(if pixel[0]>0. {65504.} else {-65504.}).to_bits(),alpha.to_bits()]};
            let actual:[u16;2]=std::array::from_fn(|c|u16::from_le_bytes(encoded[c*2..c*2+2].try_into().unwrap()));
            assert_eq!(actual,expected,"gray pixel{i} in_place={in_place}");
            for c in 0..4 {
                let actual=f32::from_le_bytes(linear[c*4..c*4+4].try_into().unwrap());
                let expected=if c==3 {f16::from_bits(expected[1]).to_f32()} else {f16::from_bits(expected[0]).to_f32()*f16::from_bits(expected[1]).to_f32()};
                assert_eq!(actual,expected,"gray canonical pixel{i} channel{c} in_place={in_place}");
            }
        }
    }
}

#[test]
fn hdr_half_publication_preserves_finite_codes_subnormals_and_canonical_cache() {
    use layer_core::color::{DocumentColor, f16};
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F16 }).unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let output = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    for alpha in [1., 0.5, 1./65536., 0.] {
        let pixels: Vec<_> = (0..65536u32).map(|code| {
            let v=f16::from_bits(code as u16).to_f32();
            let v=if v.is_finite() {v} else {0.};
            [v*alpha, -v*alpha, 4.*alpha, alpha]
        }).collect();
        upload(&r, &working, &working_bytes(&pixels));
        let request = NativeTileRequest { mode: Default::default(), space: RgbSpace::Srgb, working: &working, encoded: (&output).into(), canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F16, alpha: AlphaAssociation::Straight, region: [0,0,256,256] };
        let batch=encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        let actual=page_bytes(&r,&output);
        let cache=page_bytes(&r,&canonical);
        for (i, ((bytes,linear),pixel)) in actual.as_chunks::<8>().0.iter().zip(cache.as_chunks::<16>().0.iter()).zip(&pixels).enumerate() {
            let straight=if alpha>0. { [pixel[0]/alpha,pixel[1]/alpha,pixel[2]/alpha,alpha] } else { [0.;4] };
            let expected=layer_core::color::hdr::encode_pixel(straight).unwrap();
            for c in 0..4 {
                let bits=u16::from_le_bytes(bytes[c*2..c*2+2].try_into().unwrap());
                // Signed zero of premultiplied RGB is not an artwork distinction.
                if expected[c]&32767 != 0 { assert_eq!(bits,expected[c],"pixel={i} channel={c} alpha={alpha}"); }
                else { assert_eq!(bits&32767,0); }
                let decoded=f32::from_le_bytes(linear[c*4..c*4+4].try_into().unwrap());
                let value=f16::from_bits(bits).to_f32()*if c<3 {alpha} else {1.};
                assert_eq!(decoded,value,"canonical pixel={i} channel={c}");
            }
        }
    }
    for (pixel, expected) in [([f32::NAN,0.,0.,1.], [0,0,0,0x3c00]), ([1.,0.,0.,-0.1], [0;4]), ([1.,0.,0.,1.1], [0x3c00,0,0,0x3c00])] {
        upload(&r,&working,&working_bytes(&vec![pixel;65536]));
        let batch=encoder.prepare(&r.device,&[NativeTileRequest { mode: Default::default(), space: RgbSpace::Srgb, working:&working, encoded: (&output).into(), canonical:&canonical, transfer:&transfer,
            depth:SampleDepth::F16,alpha:AlphaAssociation::Straight,region:[0,0,256,256] }], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        let stored = page_bytes(&r, &output);
        assert_eq!(stored[..8].as_chunks::<2>().0.iter().map(|v| u16::from_le_bytes(*v) & 32767).collect::<Vec<_>>(), expected, "{pixel:?}");
        assert!(page_bytes(&r, &canonical).as_chunks::<4>().0.iter().all(|v| f32::from_le_bytes(*v).is_finite()), "{pixel:?} canonical");
    }
}

#[test]
fn half_publication_saturates_finite_rgb_and_reconstructs_quantized_alpha() {
    use layer_core::color::{DocumentColor, f16};
    let mut r=WgpuRasterizer::new_native_headless(DocumentColor {depth:SampleDepth::F16,..Default::default()}).unwrap();
    let transfer=r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let working=texture(&r,wgpu::TextureFormat::Rgba32Float);
    let output=texture(&r,wgpu::TextureFormat::Rgba16Uint);
    let pixels:Vec<_>=(0..65536).map(|i| {
        let alpha=[1.,0.37,1./65536.,1./16777216.,1./67108864.,0.,-0.][i%7];
        let rgb=[65504.,65505.,65536.,f32::MAX,f32::MIN,-65505.,0.,f32::INFINITY][i/7%8];
        [rgb,-rgb,0.,alpha]
    }).collect();
    for in_place in [false,true] {
        let encoder=NativeTileEncoder::new(&r.device, in_place);
        let canonical=if in_place {working.clone()} else {texture(&r,wgpu::TextureFormat::Rgba32Float)};
        upload(&r,&working,&working_bytes(&pixels));
        let batch=encoder.prepare(&r.device,&[NativeTileRequest {mode:Default::default(),space:RgbSpace::Srgb,
            working:&working,canonical:&canonical,encoded:(&output).into(),transfer:&transfer,depth:SampleDepth::F16,
            alpha:AlphaAssociation::Straight,region:[0,0,256,256]}], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        let bytes=page_bytes(&r,&output);let cache=page_bytes(&r,&canonical);
        for (i,((encoded,linear),pixel)) in bytes.as_chunks::<8>().0.iter().zip(cache.as_chunks::<16>().0.iter()).zip(&pixels).enumerate() {
            let alpha=f16::from_f32(pixel[3]);
            let expected=if alpha.to_bits()&32767==0 {[0u16;4]} else {
                let rgb=[0,1,2].map(|c|f16::from_f32((f64::from(pixel[c])/f64::from(pixel[3])).clamp(-65504.,65504.) as f32).to_bits());
                [rgb[0],rgb[1],rgb[2],alpha.to_bits()]
            };
            for c in 0..4 {
                let code=u16::from_le_bytes(encoded[c*2..c*2+2].try_into().unwrap());
                assert_eq!(code,expected[c],"pixel={i} channel={c} in_place={in_place}");
                let canonical=f32::from_le_bytes(linear[c*4..c*4+4].try_into().unwrap());
                let value=f16::from_bits(code).to_f32()*if c<3 {alpha.to_f32()} else {1.};
                assert_eq!(canonical,value,"canonical pixel={i} channel={c}");
            }
        }
    }
}

#[test]
fn float32_publication_retains_precision_range_and_saturates_overflow() {
    use layer_core::color::DocumentColor;
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 }).unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let output = texture(&r, wgpu::TextureFormat::Rgba32Uint);
    for region in [[0,0,256,256], [5,7,11,13]] {
        let pixels: Vec<_> = (0..65536).map(|i| {
            let a = [1., 0.5, 1./65536.][i%3];
            [1.0000001*a, -100000.125*a, 1e30*a, a]
        }).collect();
        upload(&r, &working, &working_bytes(&pixels));
        let batch = encoder.prepare(&r.device, &[NativeTileRequest { mode: Default::default(), space: RgbSpace::Srgb, working: &working, encoded: (&output).into(), canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F32, alpha: AlphaAssociation::Straight, region }], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        let actual = page_bytes(&r, &output);
        for y in region[1]..region[1]+region[3] { for x in region[0]..region[0]+region[2] {
            let i = (y*256+x) as usize;
            let p = pixels[i];
            let expected = [p[0]/p[3],p[1]/p[3],p[2]/p[3],p[3]];
            assert_eq!(&actual[i*16..][..16], working_bytes(&[expected]));
        }}
    }
    for pixel in [[f32::MAX,f32::NEG_INFINITY,f32::INFINITY,0.125],[f32::INFINITY,0.,0.,1.]] {
        upload(&r, &working, &working_bytes(&vec![pixel;65536]));
        let batch = encoder.prepare(&r.device, &[NativeTileRequest { mode: Default::default(), space: RgbSpace::Srgb, working: &working, encoded: (&output).into(), canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F32, alpha: AlphaAssociation::Straight, region: [0,0,256,256] }], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        let stored = page_bytes(&r, &output);
        let straight: Vec<_> = stored[..16].as_chunks::<4>().0.iter().map(|v| f32::from_le_bytes(*v)).collect();
        assert_eq!(straight[0], f32::MAX, "{pixel:?}");
        assert!(straight.iter().all(|v| v.is_finite()), "{pixel:?} saturates to {straight:?}");
        assert!(page_bytes(&r, &canonical)[..16].as_chunks::<4>().0.iter().all(|v| f32::from_le_bytes(*v).is_finite()), "{pixel:?} canonical");
    }
    for pixel in [[f32::NAN,0.,0.,1.],[0.,0.,0.,1.1]] {
        upload(&r, &working, &working_bytes(&vec![pixel;65536]));
        let batch = encoder.prepare(&r.device, &[NativeTileRequest { mode: Default::default(), space: RgbSpace::Srgb, working: &working, encoded: (&output).into(), canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F32, alpha: AlphaAssociation::Straight, region: [0,0,256,256] }], &mut Default::default()).unwrap();
        submit(&r, &encoder, &[batch]);
        assert_eq!(&page_bytes(&r, &output)[..16], working_bytes(&[[0., 0., 0., 1.]]), "{pixel:?}");
    }
}

#[test]
fn writeback_shaders_validate_for_every_format_tile_count_and_mode() {
    let formats = [
        (Some(wgpu::TextureFormat::Rgba8Uint), "rgba8uint"),
        (Some(wgpu::TextureFormat::Rgba16Uint), "rgba16uint"),
        (Some(wgpu::TextureFormat::Rgba32Uint), "rgba32uint"),
        (None, "gray_alpha"),
    ];
    for in_place in [false, true] {
        for (format, name) in formats {
            for count in 1..=2 {
                let (_, source) = NativeTileEncoder::pipeline_source(in_place, false, count, format, name);
                let module = naga::front::wgsl::parse_str(&source)
                    .unwrap_or_else(|error| panic!("{name} x{count}: {}", error.emit_to_string(&source)));
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                    .validate(&module)
                    .unwrap_or_else(|error| panic!("{name} x{count}: {error:?}"));
                assert_eq!(module.entry_points.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), ["main"]);
            }
        }
    }
}

#[test]
fn native_in_place_features_require_float_storage_read_write_for_both_planes() {
    let feature = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    let unrelated = wgpu::Features::FLOAT32_FILTERABLE;
    assert_eq!(in_place_features(unrelated, |_| panic!("missing feature must not query formats")), wgpu::Features::empty());
    for mask in 0u32..16 {
        let mut queried = Vec::new();
        let actual = in_place_features(feature | unrelated, |format| {
            queried.push(format);
            let shift = match format {
                wgpu::TextureFormat::Rgba32Float => 0,
                wgpu::TextureFormat::R32Float => 2,
                _ => panic!("unexpected in-place format"),
            };
            wgpu::TextureFormatFeatures {
                allowed_usages: wgpu::TextureUsages::TEXTURE_BINDING
                    | if mask & (1 << shift) != 0 { wgpu::TextureUsages::STORAGE_BINDING } else { wgpu::TextureUsages::empty() },
                flags: if mask & (2 << shift) != 0 { wgpu::TextureFormatFeatureFlags::STORAGE_READ_WRITE } else { wgpu::TextureFormatFeatureFlags::empty() },
            }
        });
        assert_eq!(actual, if mask == 15 { feature } else { wgpu::Features::empty() }, "capability mask {mask:04b}");
        let expected = if mask & 3 == 3 { vec![wgpu::TextureFormat::Rgba32Float, wgpu::TextureFormat::R32Float] }
            else { vec![wgpu::TextureFormat::Rgba32Float] };
        assert_eq!(queried, expected, "capability mask {mask:04b}");
    }
}
