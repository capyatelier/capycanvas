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
                        assert!(
                            cache.gpu_bytes()
                                <= SOURCE_SLOTS as u64 * FLOAT_TILE_BYTES + 3 * 256 * 256 * 4
                        );
                    }
                }
            }
        }
    }
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
