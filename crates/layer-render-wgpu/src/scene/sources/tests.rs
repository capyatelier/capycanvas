use super::*;
use layer_core::color::source::{SourceBuilder, SourceInterpretation};

#[test]
fn source_decode_preserves_all_integer_codes_and_extended_linear_rgb() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let scene = Scene::new(&r);
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
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
                        let mut cache = SourceTiles {
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
                        assert_eq!(cache.in_flight.count.load(Ordering::Acquire), 0);
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
                                <= SOURCE_SLOTS as u64 * FLOAT_TILE_BYTES
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
    let cache = SourceTiles::default();
    let abandoned = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for _ in 0..SOURCE_SLOTS {
        cache.charge_upload(&abandoned, FLOAT_TILE_BYTES);
    }
    assert!(cache.uploads_full());
    drop(abandoned);
    assert!(!cache.uploads_full());
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
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
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
            let mut cache = SourceTiles {
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
