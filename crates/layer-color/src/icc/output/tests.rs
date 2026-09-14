use super::*;
use layer_core::color::IntegerDepth;

fn destination(
    space: RgbSpace,
    depth: IntegerDepth,
    channels: SourceChannels,
) -> SourceInterpretation {
    SourceInterpretation {
        channels,
        depth,
        profile: ColorProfile::Builtin(space),
        profile_assumed: false,
    }
}
fn codes(bytes: &[u8], depth: IntegerDepth) -> Vec<u32> {
    bytes
        .chunks_exact(depth.bytes())
        .map(|c| {
            if c.len() == 1 {
                u32::from(c[0])
            } else {
                u32::from(u16::from_le_bytes([c[0], c[1]]))
            }
        })
        .collect()
}

#[test]
fn every_native_integer_code_and_straight_hidden_color_round_trips() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let destination = destination(space, depth, SourceChannels::Rgba);
            let decoder = WorkingDecoder::new(&destination, space, Default::default()).unwrap();
            let encoder = WorkingEncoder::new(space, &destination, Default::default()).unwrap();
            let maximum = depth.maximum();
            let input: Vec<_> = (0..=maximum)
                .flat_map(|i| [i, maximum - i, (i * 617) % (maximum + 1), i % 3])
                .flat_map(|code| (code as u16).to_le_bytes()[..depth.bytes()].to_vec())
                .collect();
            let mut working = vec![[0.; 4]; maximum as usize + 1];
            decoder.decode_pixels(&input, &mut working).unwrap();
            let mut output = vec![0; input.len()];
            assert_eq!(
                encoder
                    .encode_straight(&working, &mut output, None)
                    .unwrap()
                    .clipped_channels,
                0
            );
            assert_eq!(
                input.iter().zip(&output).position(|(a, b)| a != b),
                None,
                "{space:?} {depth:?}"
            );
        }
    }
}

#[test]
fn premultiplied_output_preserves_integer16_at_low_alpha() {
    for space in RgbSpace::ALL {
        let encoder = WorkingEncoder::new(
            space,
            &destination(space, IntegerDepth::U16, SourceChannels::Rgba),
            Default::default(),
        )
        .unwrap();
        for alpha in [0u32, 1, 2, 17, 257, 32768, 65535] {
            let coverage = alpha as f32 / 65535.;
            let input: Vec<_> = (0..=65535u32)
                .map(|i| {
                    let rgb = [i, 65535 - i, (i * 617) % 65536]
                        .map(|code| space.decode(f64::from(code) / 65535.) as f32 * coverage);
                    [rgb[0], rgb[1], rgb[2], coverage]
                })
                .collect();
            let mut output = vec![0; input.len() * 8];
            encoder
                .encode_premultiplied(&input, &mut output, None)
                .unwrap();
            for (i, pixel) in codes(&output, IntegerDepth::U16)
                .chunks_exact(4)
                .enumerate()
            {
                let i = i as u32;
                let expected = if alpha == 0 {
                    [0; 4]
                } else {
                    [i, 65535 - i, (i * 617) % 65536, alpha]
                };
                for c in 0..3 {
                    assert!(
                        pixel[c].abs_diff(expected[c]) <= 1,
                        "{space:?} {i} alpha={alpha} channel={c}"
                    );
                }
                assert_eq!(pixel[3], alpha);
            }
        }
    }
}

#[test]
fn matte_is_explicit_linear_and_precedes_profile_conversion() {
    let encoder = WorkingEncoder::new(
        RgbSpace::Srgb,
        &destination(RgbSpace::Srgb, IntegerDepth::U8, SourceChannels::Rgb),
        Default::default(),
    )
    .unwrap();
    let mut output = [13; 3];
    assert!(
        encoder
            .encode_premultiplied(&[[0.5, 0., 0., 0.5]], &mut output, None)
            .is_err()
    );
    assert_eq!(output, [13; 3]);
    encoder
        .encode_premultiplied(&[[0.5, 0., 0., 0.5]], &mut output, Some([0., 0., 1.]))
        .unwrap();
    assert_eq!(output, [188, 0, 188]);
    let encoder = WorkingEncoder::new(
        RgbSpace::DisplayP3,
        &destination(RgbSpace::Srgb, IntegerDepth::U16, SourceChannels::Rgba),
        Default::default(),
    )
    .unwrap();
    let mut output = [0; 8];
    let statistics = encoder
        .encode_straight(&[[1., 0., 0., 1.]], &mut output, None)
        .unwrap();
    assert_eq!(codes(&output, IntegerDepth::U16), [65535, 0, 0, 65535]);
    assert_eq!(statistics.clipped_channels, 3);
}

#[test]
fn gray_outputs_have_stable_matching_profiles_and_independent_alpha() {
    for space in RgbSpace::ALL {
        let gray = gray_profile(space).unwrap();
        assert_eq!(gray, gray_profile(space).unwrap());
        assert_eq!(profile_channels(&gray).unwrap(), ProfileChannels::Gray);
        let encoder = WorkingEncoder::new(
            space,
            &destination(space, IntegerDepth::U16, SourceChannels::GrayAlpha),
            Default::default(),
        )
        .unwrap();
        assert_eq!(encoder.interpretation().profile, gray);
        let input = [
            [0., 0., 0., 0.],
            [0.18, 0.18, 0.18, 1. / 65535.],
            [1., 1., 1., 1.],
        ];
        let mut output = [0; 12];
        encoder.encode_straight(&input, &mut output, None).unwrap();
        let values = codes(&output, IntegerDepth::U16);
        assert_eq!([values[1], values[3], values[5]], [0, 1, 65535]);
        assert_eq!(values[0], 0);
        assert_eq!(values[4], 65535);
        let decoder =
            WorkingDecoder::new(encoder.interpretation(), space, Default::default()).unwrap();
        let mut restored = [[0.; 4]; 3];
        decoder.decode_pixels(&output, &mut restored).unwrap();
        for (actual, expected) in restored.iter().zip(input) {
            for c in 0..3 {
                assert!(
                    (actual[c] - expected[c]).abs() < 0.0001,
                    "{space:?}: {restored:?}"
                );
            }
        }
    }
}

#[test]
fn unsupported_profiles_channels_and_nonfinite_working_data_fail() {
    let mut target = destination(RgbSpace::Srgb, IntegerDepth::U16, SourceChannels::Cmyk);
    assert!(WorkingEncoder::new(RgbSpace::Srgb, &target, Default::default()).is_err());
    target.channels = SourceChannels::Rgba;
    target.profile = gray_profile(RgbSpace::Srgb).unwrap();
    assert!(WorkingEncoder::new(RgbSpace::Srgb, &target, Default::default()).is_err());
    target.profile = ColorProfile::Icc(vec![0; 132].into());
    assert!(WorkingEncoder::new(RgbSpace::Srgb, &target, Default::default()).is_err());
    target.profile = ColorProfile::Builtin(RgbSpace::Srgb);
    let encoder = WorkingEncoder::new(RgbSpace::Srgb, &target, Default::default()).unwrap();
    for input in [
        [f32::NAN, 0., 0., 1.],
        [0., 0., 0., -1.],
        [0., 0., 0., 1.1],
        [f32::MAX, 0., 0., f32::MIN_POSITIVE],
    ] {
        assert!(
            encoder
                .encode_premultiplied(&[input], &mut [0; 8], None)
                .is_err()
        );
    }
}

#[test]
fn icc_rgb_output_matches_direct_encoded_cmm_conversion() {
    for source in RgbSpace::ALL {
        for target in RgbSpace::ALL {
            let mut destination = destination(target, IntegerDepth::U16, SourceChannels::Rgba);
            destination.profile =
                ColorProfile::Icc(profile_bytes(&destination.profile).unwrap().into());
            for intent in [
                RenderingIntent::RelativeColorimetric,
                RenderingIntent::AbsoluteColorimetric,
                RenderingIntent::Perceptual,
                RenderingIntent::Saturation,
            ] {
                let options = ConversionOptions {
                    intent,
                    black_point_compensation: true,
                };
                let encoder = WorkingEncoder::new(source, &destination, options).unwrap();
                let reference = RgbTransform::new(
                    &ColorProfile::Builtin(source),
                    &destination.profile,
                    options,
                )
                .unwrap();
                let original: Vec<_> = (0..729)
                    .map(|i| {
                        [
                            0.1 + (i % 9) as f32 * 0.1,
                            0.1 + (i / 9 % 9) as f32 * 0.1,
                            0.1 + (i / 81) as f32 * 0.1,
                            (i % 11) as f32 / 10.,
                        ]
                    })
                    .collect();
                let linear: Vec<_> = original
                    .iter()
                    .map(|p| {
                        [
                            source.decode(f64::from(p[0])) as f32,
                            source.decode(f64::from(p[1])) as f32,
                            source.decode(f64::from(p[2])) as f32,
                            p[3],
                        ]
                    })
                    .collect();
                let mut expected = original;
                reference.apply(&mut expected);
                let mut output = vec![0; linear.len() * 8];
                encoder.encode_straight(&linear, &mut output, None).unwrap();
                for (i, (actual, expected)) in codes(&output, IntegerDepth::U16)
                    .chunks_exact(4)
                    .zip(expected)
                    .enumerate()
                {
                    for c in 0..4 {
                        let code = (f64::from(expected[c]).clamp(0., 1.) * 65535.).round() as u32;
                        assert!(
                            actual[c].abs_diff(code) <= if c == 3 { 0 } else { 2 },
                            "{source:?}->{target:?} {intent:?} pixel {i} channel {c}: {} vs {code}",
                            actual[c]
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires LAYER_TEST_CMYK_PROFILE pointing to a licensed output ICC profile"]
fn cmyk_output_matches_independent_cmm_percent_samples() {
    let path = std::env::var("LAYER_TEST_CMYK_PROFILE").expect("set LAYER_TEST_CMYK_PROFILE");
    let bytes = std::fs::read(path).unwrap();
    let destination = SourceInterpretation {
        channels: SourceChannels::Cmyk,
        depth: IntegerDepth::U16,
        profile: ColorProfile::Icc(bytes.clone().into()),
        profile_assumed: false,
    };
    for intent in [
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::AbsoluteColorimetric,
        RenderingIntent::Perceptual,
        RenderingIntent::Saturation,
    ] {
        for bpc in [false, true] {
            let options = ConversionOptions {
                intent,
                black_point_compensation: bpc,
            };
            let encoder = WorkingEncoder::new(RgbSpace::Srgb, &destination, options).unwrap();
            // Separate direct encoded-RGB -> CMYK transform, with independently
            // selected LCMS formatters. CMYK float samples are percentages.
            let input_profile = Profile::new_srgb();
            let output_profile = Profile::new_icc(&bytes).unwrap();
            let reference: Transform<[f32; 3], [f32; 4]> = Transform::new_flags(
                &input_profile,
                PixelFormat::RGB_FLT,
                &output_profile,
                PixelFormat::CMYK_FLT,
                super::super::intent(intent),
                flags(options),
            )
            .unwrap();
            let encoded: Vec<_> = (0..729)
                .map(|i| {
                    [
                        (i % 9) as f32 / 8.,
                        (i / 9 % 9) as f32 / 8.,
                        (i / 81) as f32 / 8.,
                    ]
                })
                .collect();
            let linear: Vec<_> = encoded
                .iter()
                .map(|p| {
                    [
                        RgbSpace::Srgb.decode(f64::from(p[0])) as f32,
                        RgbSpace::Srgb.decode(f64::from(p[1])) as f32,
                        RgbSpace::Srgb.decode(f64::from(p[2])) as f32,
                        1.,
                    ]
                })
                .collect();
            let mut expected = vec![[0.; 4]; encoded.len()];
            reference.transform_pixels(&encoded, &mut expected);
            let mut output = vec![0; encoded.len() * 8];
            encoder.encode_straight(&linear, &mut output, None).unwrap();
            for (actual, expected) in codes(&output, IntegerDepth::U16)
                .chunks_exact(4)
                .zip(expected)
            {
                for c in 0..4 {
                    let code =
                        (f64::from(expected[c] / 100.).clamp(0., 1.) * 65535.).round() as u32;
                    assert!(
                        actual[c].abs_diff(code) <= 2,
                        "{intent:?} bpc={bpc} channel {c}: {} vs {code}",
                        actual[c]
                    );
                }
            }
        }
    }
}
