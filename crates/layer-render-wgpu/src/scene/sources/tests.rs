use super::*;
use layer_core::color::source::{SourceBuilder, SourceInterpretation};

#[test]
fn source_decode_preserves_all_integer_codes_and_extended_linear_rgb() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let scene = Scene::new(&r);
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            for channels in [
                SourceChannels::Gray,
                SourceChannels::GrayAlpha,
                SourceChannels::Rgb,
                SourceChannels::Rgba,
            ] {
                let interpretation = SourceInterpretation {
                    channels,
                    depth,
                    profile: ColorProfile::Builtin(space),
                    profile_assumed: false,
                };
                let mut builder =
                    SourceBuilder::new([PAGE_SIZE; 2], interpretation, 4 * 1024 * 1024).unwrap();
                let maximum = depth.maximum();
                for y in 0..PAGE_SIZE {
                    let mut row = Vec::new();
                    for x in 0..PAGE_SIZE {
                        let code = (y * PAGE_SIZE + x) & maximum;
                        for c in 0..channels.count() {
                            let value = code.wrapping_mul([1, 101, 237, 317][c]) & maximum;
                            row.extend_from_slice(&(value as u16).to_le_bytes()[..depth.bytes()]);
                        }
                    }
                    builder.push_row(&row).unwrap();
                }
                let original = builder.finish().unwrap();
                for embedded in [false, true] {
                    // These generated ICC profiles describe RGB channels. Gray
                    // ICC transforms have separate native CMM fixtures.
                    if embedded
                        && matches!(channels, SourceChannels::Gray | SourceChannels::GrayAlpha)
                    {
                        continue;
                    }
                    let mut source = original.clone();
                    if embedded {
                        source.interpretation.profile = ColorProfile::Icc(
                            layer_color::profile_bytes(&source.interpretation.profile)
                                .unwrap()
                                .into(),
                        );
                    }
                    let source = Arc::new(source);
                    for destination in [space, RgbSpace::Srgb] {
                        let mut cache = DecodedTiles {
                            destination,
                            ..Default::default()
                        };
                        let (_, pending) = cache.plan(&r, &source, [0, 0]).unwrap();
                        let pending = pending.unwrap();
                        let bytes: Vec<_> = pending
                            .data
                            .unwrap_or([0.; 24])
                            .into_iter()
                            .flat_map(f32::to_ne_bytes)
                            .collect();
                        r.queue.write_buffer(&scene.buffer, 0, &bytes);
                        let mut encoder =
                            crate::submission::CommandEncoder::new(&r.device, &Default::default());
                        let uploaded = cache
                            .encode(&r, &mut encoder, &pending, &scene.binding, 0)
                            .unwrap();
                        assert_eq!(cache.charge_upload(&encoder, uploaded), uploaded);
                        assert_eq!(
                            uploaded,
                            u64::from(PAGE_SIZE * PAGE_SIZE)
                                * if embedded {
                                    16
                                } else {
                                    4 * depth.bytes() as u64
                                }
                        );
                        encoder.submit(&r.queue);
                        let read = crate::layer_tests::page_bytes(&r, &pending.texture);
                        assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
                        let decoder = layer_color::WorkingDecoder::new(
                            &source.interpretation,
                            destination,
                            Default::default(),
                        )
                        .unwrap();
                        let mut reference = vec![[0.; 4]; (PAGE_SIZE * PAGE_SIZE) as usize];
                        decoder
                            .decode_tile(&source, [0, 0], &mut reference)
                            .unwrap();
                        let mut max_error = 0f32;
                        let mut code_error = 0f64;
                        for (pixel, reference) in read.chunks_exact(16).zip(reference) {
                            let actual: [f32; 4] = std::array::from_fn(|c| {
                                f32::from_ne_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap())
                            });
                            assert!((actual[3] - reference[3]).abs() <= 0.00000012);
                            assert_eq!(
                                (actual[3] * maximum as f32).round(),
                                (reference[3] * maximum as f32).round()
                            );
                            if reference[3] == 1. {
                                assert_eq!(actual[3], 1.);
                            }
                            for c in 0..3 {
                                max_error =
                                    max_error.max((actual[c] - reference[c] * reference[3]).abs());
                            }
                            if actual[3] > 0. {
                                let straight = std::array::from_fn::<_, 3, _>(|c| {
                                    f64::from(actual[c]) / f64::from(actual[3])
                                });
                                for c in 0..3 {
                                    let expected = destination.encode(reference[c] as f64);
                                    let measured = destination.encode(straight[c]);
                                    code_error = code_error.max(
                                        ((measured * maximum as f64).round()
                                            - (expected * maximum as f64).round())
                                        .abs(),
                                    );
                                }
                            }
                        }
                        eprintln!(
                            "{space:?} embedded={embedded} to {destination:?} {depth:?} {channels:?}: max linear error {max_error}, max integer error {code_error}"
                        );
                        assert!(
                            max_error <= 0.000003,
                            "{space:?} embedded={embedded} to {destination:?} {depth:?} {channels:?}: {max_error}"
                        );
                        assert!(
                            code_error <= 2.,
                            "{space:?} embedded={embedded} to {destination:?} {depth:?} {channels:?}: {code_error}"
                        );
                        if !embedded && destination == space {
                            assert_eq!(code_error, 0., "native integer decode must be exact");
                        }
                        assert!(
                            cache.gpu_bytes()
                                <= DECODED_SLOTS as u64 * FLOAT_TILE_BYTES
                                    + 3 * 256 * 256 * 4
                                    + 3 * transfer::TABLE_BYTES
                        );
                    }
                }
            }
        }
    }
    let mut tables = transfer::Tables::default();
    let srgb = tables.prepare(&r.device, RgbSpace::Srgb).unwrap().clone();
    assert_eq!(
        *tables.prepare(&r.device, RgbSpace::DisplayP3).unwrap(),
        srgb
    );
    for space in [RgbSpace::AdobeRgb, RgbSpace::ProPhoto] {
        tables.prepare(&r.device, space).unwrap();
    }
    assert_eq!(tables.gpu_bytes(), 3 * transfer::TABLE_BYTES);
    for sizes in [
        [256 * 1024; 3],                       // Native U8.
        [512 * 1024; 3],                       // Native U16.
        [1024 * 1024; 3],                      // ICC Float32.
        [256 * 1024, 1024 * 1024, 512 * 1024], // Mixed source representations.
    ] {
        let cache = DecodedTiles::default();
        let abandoned = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let mut uploads = 0;
        while !cache.uploads_full() {
            let bytes = sizes[uploads % sizes.len()];
            let total = cache.charge_upload(&abandoned, bytes);
            assert!(
                total <= 16 * 1024 * 1024,
                "staging exceeds its existing ceiling"
            );
            uploads += 1;
            assert!(
                uploads <= 64,
                "admission must eventually drain pending uploads"
            );
        }
        if sizes[0] < 1024 * 1024 {
            assert!(
                uploads > 16,
                "small uploads must use the available byte allowance"
            );
        }
        drop(abandoned);
        assert!(!cache.uploads_full());
        assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
    }
    let cache = DecodedTiles::default();
    // A final maximum-sized upload must also fit after mixed smaller inputs.
    let abandoned = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    while !cache.uploads_full() {
        cache.charge_upload(&abandoned, FLOAT_TILE_BYTES / 4);
        if cache.uploads_full() { break; }
        assert!(cache.charge_upload(&abandoned, FLOAT_TILE_BYTES)
            <= SOURCE_SLOTS as u64 * FLOAT_TILE_BYTES);
    }
    drop(abandoned);
    assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
}

#[test]
#[ignore = "hardware native source decode, upload and table residency benchmark"]
fn native_source_decode_workloads() {
    let r = WgpuRasterizer::new_headless().unwrap();
    #[cfg(target_os = "linux")]
    let _affinity = pin_benchmark_thread();
    let scene = Scene::new(&r);
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let mut builder = SourceBuilder::new(
                [1280, 1024],
                SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth,
                    profile: ColorProfile::Builtin(space),
                    profile_assumed: false,
                },
                16 * 1024 * 1024,
            )
            .unwrap();
            let maximum = depth.maximum();
            for y in 0..1024 {
                let row: Vec<_> = (0..1280)
                    .flat_map(|x| {
                        [
                            (x * 8191 + y * 31) & maximum,
                            (x * 17 + y * 16381) & maximum,
                            if (x / 173 + y / 111) % 2 == 0 {
                                maximum
                            } else {
                                0
                            },
                            maximum,
                        ]
                    })
                    .flat_map(|v| v.to_le_bytes().into_iter().take(depth.bytes()))
                    .collect();
                builder.push_row(&row).unwrap();
            }
            let source = Arc::new(builder.finish().unwrap());
            let uniform: Vec<_> = builtin_settings(&source, [0, 0], space)
                .unwrap()
                .into_iter()
                .flat_map(f32::to_ne_bytes)
                .collect();
            r.queue.write_buffer(&scene.buffer, 0, &uniform);
            let mut cache = DecodedTiles {
                destination: space,
                ..Default::default()
            };
            let mut times = Vec::new();
            let mut peak_upload = 0;
            for frame in 0..120 {
                let start = std::time::Instant::now();
                let mut commands =
                    crate::submission::CommandEncoder::new(&r.device, &Default::default());
                for tile in 0..16 {
                    let index = (frame * 16 + tile) % 20;
                    let (_, pending) = cache.plan(&r, &source, [index % 5, index / 5]).unwrap();
                    if let Some(pending) = pending {
                        let uploaded = cache
                            .encode(&r, &mut commands, &pending, &scene.binding, 0)
                            .unwrap();
                        peak_upload = peak_upload.max(cache.charge_upload(&commands, uploaded));
                    }
                }
                commands.submit(&r.queue);
                let cpu = start.elapsed().as_secs_f64() * 1000.;
                r.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(READBACK_TIMEOUT),
                    })
                    .unwrap();
                let complete = start.elapsed().as_secs_f64() * 1000.;
                if frame == 0 {
                    println!(
                        "NATIVE_SOURCE cold space={space:?} depth={depth:?} cpu_ms={cpu:.4} complete_ms={complete:.4}"
                    );
                }
                if frame >= 20 {
                    times.push([cpu, complete]);
                }
            }
            for axis in 0..2 {
                times.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
                println!(
                    "NATIVE_SOURCE warm space={space:?} depth={depth:?} kind={} p95_ms={:.4} p99_ms={:.4}",
                    ["cpu", "complete"][axis],
                    times[94][axis],
                    times[98][axis]
                );
            }
            println!(
                "NATIVE_SOURCE residency space={space:?} depth={depth:?} gpu_bytes={} peak_upload={peak_upload} hits={} misses={}",
                cache.gpu_bytes(),
                cache.hits,
                cache.misses
            );
            assert_eq!(cache.misses, 120 * 16);
            assert!(peak_upload <= 16 * 1024 * 1024);
        }
    }
}

#[cfg(target_os = "linux")]
fn pin_benchmark_thread() -> Option<BenchmarkAffinity> {
    let cpu: usize = std::env::var("LAYER_BENCH_CPU").ok()?.parse().unwrap();
    assert!(cpu < libc::CPU_SETSIZE as usize);
    // pid 0 changes this calling thread only. Device/compiler workers have
    // already started and retain their ordinary affinity. Restore on return.
    let mut previous: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    let mut selected: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe {
        assert_eq!(
            libc::sched_getaffinity(0, std::mem::size_of_val(&previous), &mut previous),
            0
        );
        assert!(libc::CPU_ISSET(cpu, &previous));
        libc::CPU_SET(cpu, &mut selected);
        assert_eq!(
            libc::sched_setaffinity(0, std::mem::size_of_val(&selected), &selected),
            0
        );
    }
    println!("NATIVE_SOURCE benchmark_thread_cpu={cpu}");
    Some(BenchmarkAffinity(previous))
}
#[cfg(target_os = "linux")]
struct BenchmarkAffinity(libc::cpu_set_t);
#[cfg(target_os = "linux")]
impl Drop for BenchmarkAffinity {
    fn drop(&mut self) {
        unsafe {
            libc::sched_setaffinity(0, std::mem::size_of_val(&self.0), &self.0);
        }
    }
}

fn raster_fixture(
    depth: SampleDepth,
    alpha: AlphaAssociation,
    alpha_code: Option<u32>,
) -> (Arc<TileBlob>, Vec<u8>) {
    let maximum = depth.maximum();
    let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE)
        .flat_map(|i| {
            [
                i & maximum,
                (i * 101) & maximum,
                (i * 237) & maximum,
                alpha_code.unwrap_or((i * 317) & maximum),
            ]
        })
        .flat_map(|v| (v as u16).to_le_bytes().into_iter().take(depth.bytes()))
        .collect();
    let descriptor = PixelDescriptor {
            sample: layer_core::color::SampleType::Unsigned,
        channels: 4,
        bits_per_channel: depth.bits(),
        encoding: TransferEncoding::Profile,
        alpha,
    };
    (
        Arc::new(TileBlob::encode(descriptor, &bytes).unwrap()),
        bytes,
    )
}
fn encode_pending(
    r: &WgpuRasterizer,
    scene: &Scene,
    cache: &mut DecodedTiles,
    pending: &PendingTile,
    encoder: &mut crate::submission::CommandEncoder,
) {
    let bytes: Vec<_> = pending
        .data
        .unwrap()
        .into_iter()
        .flat_map(f32::to_ne_bytes)
        .collect();
    // Each helper call submits before the next uniform write.
    r.queue.write_buffer(&scene.buffer, 0, &bytes);
    let count = cache
        .encode(r, encoder, pending, &scene.binding, 0)
        .unwrap();
    cache.charge_upload(encoder, count);
}

#[test]
fn neighboring_layer_tiles_stay_decoded_across_bounded_upload_submissions() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let scene = Scene::new(&r);
    let mut cache = DecodedTiles::new(RgbSpace::Srgb);
    // An ordinary layered stroke needs neighboring tiles from eight layers.
    // Keep their distinct immutable backings alive across queue submissions.
    let tiles: Vec<_> = (0..32)
        .map(|i| {
            let pixel = [i, 129, 231, 255];
            Arc::new(
                TileBlob::encode(
                    layer_core::color::DocumentColor::default().paint_descriptor(),
                    &pixel.repeat(256 * 256),
                )
                .unwrap(),
            )
        })
        .collect();
    for tile in &tiles {
        if cache.uploads_full() {
            r.wait_idle().unwrap();
        }
        let (_, pending) = cache
            .plan_raster(&r, tile, RgbSpace::Srgb, RgbSpace::Srgb)
            .unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        encode_pending(&r, &scene, &mut cache, &pending.unwrap(), &mut encoder);
        encoder.submit(&r.queue);
    }
    r.wait_idle().unwrap();
    for (i, tile) in tiles.iter().enumerate() {
        let (decoded, pending) = cache
            .plan_raster(&r, tile, RgbSpace::Srgb, RgbSpace::Srgb)
            .unwrap();
        assert!(
            pending.is_none(),
            "unchanged layer tile must not upload again"
        );
        let bytes = crate::layer_tests::page_bytes(&r, &decoded.texture);
        let expected = [
            RgbSpace::Srgb.decode(i as f64 / 255.) as f32,
            RgbSpace::Srgb.decode(129. / 255.) as f32,
            RgbSpace::Srgb.decode(231. / 255.) as f32,
            1.,
        ];
        for pixel in bytes.chunks_exact(16) {
            for (actual, expected) in pixel.chunks_exact(4).zip(expected) {
                let actual = f32::from_ne_bytes(actual.try_into().unwrap());
                assert!((actual - expected).abs() <= 3e-6);
            }
        }
    }
    assert_eq!(cache.misses, tiles.len() as u64);
    assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
}

#[test]
fn equal_native_samples_share_decoded_pixels_across_allocations_and_encodings() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let scene = Scene::new(&r);
    let mut cache = DecodedTiles::default();
    let (first, bytes) = raster_fixture(SampleDepth::U16, AlphaAssociation::Straight, Some(32767));
    let second = Arc::new(TileBlob::encode(first.descriptor, &bytes).unwrap());
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(first.digest, second.digest);
    let weak = Arc::downgrade(&first);
    let (decoded, pending) = cache.plan_raster(&r, &first, RgbSpace::ProPhoto, RgbSpace::Srgb).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encode_pending(&r, &scene, &mut cache, &pending.unwrap(), &mut encoder);
    encoder.submit(&r.queue);
    let original = crate::layer_tests::page_bytes(&r, &decoded.texture);
    drop(first);
    assert!(weak.upgrade().is_none(), "decoded pixels must not retain history backing");

    let (reused, pending) = cache.plan_raster(&r, &second, RgbSpace::ProPhoto, RgbSpace::Srgb).unwrap();
    assert!(pending.is_none(), "identical native samples must not upload again");
    assert_eq!(decoded.texture, reused.texture);
    assert_eq!(original, crate::layer_tests::page_bytes(&r, &reused.texture));
    assert_eq!((cache.hits, cache.misses, cache.slots.len()), (1, 1, 1));
    assert!(cache.prepared_raster_view(&second, RgbSpace::ProPhoto).is_some());
    for (source, destination) in [(RgbSpace::Srgb, RgbSpace::Srgb), (RgbSpace::ProPhoto, RgbSpace::ProPhoto)] {
        assert!(cache.plan_raster(&r, &second, source, destination).unwrap().1.is_some());
    }
}

#[test]
fn native_raster_decode_preserves_codes_alpha_and_profile_meaning() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let scene = Scene::new(&r);
    let mut cache = DecodedTiles::default();
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            for association in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                let maximum = depth.maximum();
                for alpha_code in [
                    Some(0),
                    Some(1),
                    Some(2),
                    Some(17),
                    Some(maximum / 2),
                    Some(maximum),
                    None,
                ] {
                    let (blob, bytes) = raster_fixture(depth, association, alpha_code);
                    for destination in [space, RgbSpace::Srgb] {
                        let (tile, pending) =
                            cache.plan_raster(&r, &blob, space, destination).unwrap();
                        if let Some(pending) = pending {
                            let mut commands = crate::submission::CommandEncoder::new(
                                &r.device,
                                &Default::default(),
                            );
                            encode_pending(&r, &scene, &mut cache, &pending, &mut commands);
                            commands.submit(&r.queue);
                        }
                        let actual = crate::layer_tests::page_bytes(&r, &tile.texture);
                        let matrix = space.linear_transform(destination);
                        let mut linear_error = 0f64;
                        let mut code_error = 0f64;
                        for (input, output) in bytes
                            .chunks_exact(4 * depth.bytes())
                            .zip(actual.chunks_exact(16))
                        {
                            let code: [u32; 4] = std::array::from_fn(|c| {
                                if depth == SampleDepth::U8 {
                                    u32::from(input[c])
                                } else {
                                    u32::from(u16::from_le_bytes(
                                        input[c * 2..c * 2 + 2].try_into().unwrap(),
                                    ))
                                }
                            });
                            let measured: [f32; 4] = std::array::from_fn(|c| {
                                f32::from_ne_bytes(output[c * 4..c * 4 + 4].try_into().unwrap())
                            });
                            let a = f64::from(code[3]) / f64::from(maximum);
                            assert_eq!(
                                (f64::from(measured[3]) * f64::from(maximum)).round() as u32,
                                code[3]
                            );
                            if code[3] == 0 {
                                assert_eq!(measured, [0.; 4]);
                                continue;
                            }
                            if code[3] == maximum {
                                assert_eq!(measured[3], 1.);
                            }
                            let rgb = std::array::from_fn::<_, 3, _>(|c| {
                                space.decode(f64::from(code[c]) / f64::from(maximum))
                            });
                            for c in 0..3 {
                                let reference =
                                    matrix[c].iter().zip(rgb).map(|(m, v)| m * v).sum::<f64>()
                                        * if association == AlphaAssociation::Straight {
                                            a
                                        } else {
                                            1.
                                        };
                                linear_error =
                                    linear_error.max((f64::from(measured[c]) - reference).abs());
                                if destination == space {
                                    let v = f64::from(measured[c])
                                        / if association == AlphaAssociation::Straight {
                                            f64::from(measured[3])
                                        } else {
                                            1.
                                        };
                                    code_error = code_error.max(
                                        ((space.encode(v) * f64::from(maximum)).round()
                                            - f64::from(code[c]))
                                        .abs(),
                                    );
                                }
                            }
                        }
                        assert!(
                            linear_error <= 0.000003,
                            "{space:?} {depth:?} {association:?} alpha={alpha_code:?} to={destination:?} linear={linear_error}"
                        );
                        assert_eq!(
                            code_error, 0.,
                            "{space:?} {depth:?} {association:?} alpha={alpha_code:?}"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(cache.slots.len(), DECODED_SLOTS);
    assert_eq!(
        cache.gpu_bytes(),
        DECODED_SLOTS as u64 * FLOAT_TILE_BYTES + 3 * 256 * 256 * 4 + 3 * transfer::TABLE_BYTES
    );
    assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
}

#[test]
fn native_and_source_cache_share_slots_without_retaining_history_or_discarded_values() {
    check_cache_ownership(WgpuRasterizer::new_headless().unwrap());
    check_cache_ownership(
        WgpuRasterizer::new_native_headless(Default::default()).unwrap(),
    );
}

fn check_cache_ownership(r: WgpuRasterizer) {
    let scene = Scene::new(&r);
    let mut cache = DecodedTiles::default();
    let mut source_builder = SourceBuilder::new(
        [256; 2],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        4 * 1024 * 1024,
    )
    .unwrap();
    for _ in 0..256 {
        source_builder.push_row(&[127; 1024]).unwrap();
    }
    let source = Arc::new(source_builder.finish().unwrap());
    let (blob, _) = raster_fixture(SampleDepth::U16, AlphaAssociation::Straight, Some(65535));
    let weak = Arc::downgrade(&blob);
    let mut identities = Vec::new();
    for i in 0..DECODED_SLOTS * 4 {
        if cache.uploads_full() {
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
        }
        let mut bytes = blob.decode().unwrap();
        bytes[..2].copy_from_slice(&(i as u16).to_le_bytes());
        let other = Arc::new(TileBlob::encode(blob.descriptor, &bytes).unwrap());
        let (tile, pending) = if i % 2 == 0 {
            cache
                .plan_raster(&r, &other, RgbSpace::ProPhoto, RgbSpace::Srgb)
                .unwrap()
        } else {
            cache
                .plan(&r, &Arc::new((*source).clone()), [0, 0])
                .unwrap()
        };
        let pending = pending.unwrap();
        if !identities.contains(&tile.texture) {
            identities.push(tile.texture.clone());
        }
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        encode_pending(&r, &scene, &mut cache, &pending, &mut encoder);
        encoder.submit(&r.queue);
    }
    assert_eq!(identities.len(), DECODED_SLOTS);
    let (_, pending) = cache
        .plan_raster(&r, &blob, RgbSpace::Srgb, RgbSpace::Srgb)
        .unwrap();
    drop(pending);
    let (_, pending) = cache
        .plan_raster(&r, &blob, RgbSpace::Srgb, RgbSpace::Srgb)
        .unwrap();
    let pending = pending.unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encode_pending(&r, &scene, &mut cache, &pending, &mut encoder);
    drop(pending);
    drop(encoder);
    let (_, pending) = cache
        .plan_raster(&r, &blob, RgbSpace::Srgb, RgbSpace::Srgb)
        .unwrap();
    let pending = pending.unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encode_pending(&r, &scene, &mut cache, &pending, &mut encoder);
    drop(pending);
    encoder.submit(&r.queue);
    assert!(
        cache
            .plan_raster(&r, &blob, RgbSpace::Srgb, RgbSpace::Srgb)
            .unwrap()
            .1
            .is_none()
    );
    assert!(
        cache
            .plan_raster(&r, &blob, RgbSpace::ProPhoto, RgbSpace::Srgb)
            .unwrap()
            .1
            .is_some()
    );
    assert!(
        cache
            .plan_raster(&r, &blob, RgbSpace::Srgb, RgbSpace::ProPhoto)
            .unwrap()
            .1
            .is_some()
    );
    drop(blob);
    assert!(weak.upgrade().is_none());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    assert_eq!(cache.in_flight.bytes.load(Ordering::Acquire), 0);
    assert_eq!(cache.slots.len(), DECODED_SLOTS);
}
