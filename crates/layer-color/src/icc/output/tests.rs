use super::*;
use layer_core::color::SampleDepth;

fn destination(
    space: RgbSpace,
    depth: SampleDepth,
    channels: SourceChannels,
) -> SourceInterpretation {
    SourceInterpretation {
        channels,
        depth,
        profile: ColorProfile::Builtin(space),
        profile_assumed: false,
    }
}
fn codes(bytes: &[u8], depth: SampleDepth) -> Vec<u32> {
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
        for depth in [SampleDepth::U8, SampleDepth::U16] {
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
                    .encode_straight(&working, &mut output, None, [0, 0])
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
            &destination(space, SampleDepth::U16, SourceChannels::Rgba),
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
                .encode_premultiplied(&input, &mut output, None, [0, 0])
                .unwrap();
            for (i, pixel) in codes(&output, SampleDepth::U16)
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
        &destination(RgbSpace::Srgb, SampleDepth::U8, SourceChannels::Rgb),
        Default::default(),
    )
    .unwrap();
    let mut output = [13; 3];
    assert!(
        encoder
            .encode_premultiplied(&[[0.5, 0., 0., 0.5]], &mut output, None, [0, 0])
            .is_err()
    );
    assert_eq!(output, [13; 3]);
    encoder
        .encode_premultiplied(
            &[[0.5, 0., 0., 0.5]],
            &mut output,
            Some([0., 0., 1.]),
            [0, 0],
        )
        .unwrap();
    assert_eq!(output, [188, 0, 188]);
    let encoder = WorkingEncoder::new(
        RgbSpace::DisplayP3,
        &destination(RgbSpace::Srgb, SampleDepth::U16, SourceChannels::Rgba),
        Default::default(),
    )
    .unwrap();
    let mut output = [0; 8];
    let statistics = encoder
        .encode_straight(&[[1., 0., 0., 1.]], &mut output, None, [0, 0])
        .unwrap();
    assert_eq!(codes(&output, SampleDepth::U16), [65535, 0, 0, 65535]);
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
            &destination(space, SampleDepth::U16, SourceChannels::GrayAlpha),
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
        encoder
            .encode_straight(&input, &mut output, None, [0, 0])
            .unwrap();
        let values = codes(&output, SampleDepth::U16);
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
    let mut target = destination(RgbSpace::Srgb, SampleDepth::U16, SourceChannels::Cmyk);
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
                .encode_premultiplied(&[input], &mut [0; 8], None, [0, 0])
                .is_err()
        );
    }
}

#[test]
fn icc_rgb_output_matches_direct_encoded_cmm_conversion() {
    for source in RgbSpace::ALL {
        for target in RgbSpace::ALL {
            let mut destination = destination(target, SampleDepth::U16, SourceChannels::Rgba);
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
                    black_point_compensation: false,
                };
                let encoder = WorkingEncoder::new(
                    source,
                    &destination,
                    layer_core::color::OutputEncoding {
                        conversion: options,
                        ..Default::default()
                    },
                )
                .unwrap();
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
                encoder
                    .encode_straight(&linear, &mut output, None, [0, 0])
                    .unwrap();
                for (i, (actual, expected)) in codes(&output, SampleDepth::U16)
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
#[ignore = "requires a CMYK profile and independently generated reference samples"]
fn cmyk_output_matches_independent_reference_samples() {
    // tools/validation/icc_reference.py generates these with a separate CMM.
    let bytes =
        std::fs::read(std::env::var("LAYER_TEST_CMYK_PROFILE").expect("CMYK profile")).unwrap();
    let directory = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_CMYK_REFERENCE").expect("reference directory"),
    );
    let reference = |name: &str, count: usize| {
        let bytes = std::fs::read(directory.join(name)).unwrap();
        assert_eq!(bytes.len(), count * 4);
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    let destination = SourceInterpretation {
        channels: SourceChannels::Cmyk,
        depth: SampleDepth::U16,
        profile: ColorProfile::Icc(bytes.into()),
        profile_assumed: false,
    };
    let linear: Vec<_> = (0..729)
        .map(|i| {
            let rgb = [
                (i % 9) as f64 / 8.,
                (i / 9 % 9) as f64 / 8.,
                (i / 81) as f64 / 8.,
            ]
            .map(|v| RgbSpace::Srgb.decode(v) as f32);
            [rgb[0], rgb[1], rgb[2], 1.]
        })
        .collect();
    for (id, intent) in [
        RenderingIntent::Perceptual,
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::Saturation,
        RenderingIntent::AbsoluteColorimetric,
    ]
    .into_iter()
    .enumerate()
    {
        let conversion = ConversionOptions {
            intent,
            black_point_compensation: false,
        };
        let encoder = WorkingEncoder::new(
            RgbSpace::Srgb,
            &destination,
            OutputEncoding {
                conversion,
                ..Default::default()
            },
        )
        .unwrap();
        let expected = reference(&format!("rgb-to-cmyk-{id}.f32le"), 729 * 4);
        let mut output = vec![0; linear.len() * 8];
        encoder
            .encode_straight(&linear, &mut output, None, [0, 0])
            .unwrap();
        let max = codes(&output, SampleDepth::U16)
            .iter()
            .zip(expected)
            .map(|(a, b)| (*a as f32 / 65535. - b / 100.).abs())
            .fold(0f32, f32::max);
        eprintln!("{intent:?} maximum ink difference: {max}");
        assert!(max < 0.02, "{intent:?} maximum ink difference: {max}");
        let input: Vec<[f32; 4]> = (0..625)
            .map(|i| std::array::from_fn(|c| ((i / 5usize.pow(c as u32)) % 5) as f32 * 25.))
            .collect();
        let decoder =
            InputTransform::new(&destination.profile, &ColorProfile::default(), conversion)
                .unwrap();
        let mut rgb = vec![[0.; 4]; input.len()];
        decoder.cmyk_percent(&input, &mut rgb).unwrap();
        let expected = reference(&format!("cmyk-to-rgb-{id}.f32le"), 625 * 3);
        // Compare extended linear RGB: encoded sRGB magnifies negative
        // out-of-gamut differences by 12.92. Different CMMs also implement
        // perceptual mapping differently; this is a 2% interoperability check,
        // not a bit-exact LittleCMS-equivalence claim.
        let max = rgb
            .iter()
            .flat_map(|p| p[..3].iter())
            .zip(expected)
            .map(|(a, b)| {
                let linear = |v: f32| {
                    if v <= 0.04045 {
                        f64::from(v) / 12.92
                    } else {
                        ((f64::from(v) + 0.055) / 1.055).powf(2.4)
                    }
                };
                (linear(*a) - linear(b)).abs()
            })
            .fold(0f64, f64::max);
        eprintln!("{intent:?} maximum linear RGB difference: {max}");
        assert!(
            max < 0.02,
            "{intent:?} maximum linear RGB difference: {max}"
        );
    }
}

#[test]
fn output_dither_is_stable_across_chunks_preserves_neutrals_and_does_not_touch_alpha() {
    for space in RgbSpace::ALL {
        let target = destination(space, SampleDepth::U8, SourceChannels::Rgba);
        let dithered = WorkingEncoder::new(
            space,
            &target,
            OutputEncoding {
                dither: OutputDither::Stochastic8,
                ..Default::default()
            },
        )
        .unwrap();
        let normal = WorkingEncoder::new(space, &target, Default::default()).unwrap();
        let input: Vec<_> = (0..4097)
            .map(|i| {
                let code = 90. + i as f64 / 4096.;
                let linear = space.decode(code / 255.) as f32;
                [linear, linear, linear, (i % 257) as f32 / 256.]
            })
            .collect();
        let mut whole = vec![0; input.len() * 4];
        let mut rounded = whole.clone();
        let mut split = whole.clone();
        let mut next_row = whole.clone();
        let stats = dithered
            .encode_straight(&input, &mut whole, None, [17, 31])
            .unwrap();
        assert_eq!(
            stats,
            normal
                .encode_straight(&input, &mut rounded, None, [17, 31])
                .unwrap()
        );
        dithered
            .encode_straight(&input, &mut next_row, None, [17, 32])
            .unwrap();
        assert_ne!(whole, next_row);
        assert_ne!(whole, rounded);
        for (chunk, (input, output)) in input.chunks(173).zip(split.chunks_mut(173 * 4)).enumerate()
        {
            dithered
                .encode_straight(input, output, None, [17 + (chunk * 173) as u32, 31])
                .unwrap();
        }
        assert_eq!(whole, split, "{space:?}");
        for (i, (pixel, normal)) in whole
            .chunks_exact(4)
            .zip(rounded.chunks_exact(4))
            .enumerate()
        {
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[0], pixel[2]);
            assert_eq!(pixel[3], normal[3]);
            assert!((f64::from(pixel[0]) - (90. + i as f64 / 4096.)).abs() <= 1.0001);
        }
        let mut high_depth = target;
        high_depth.depth = SampleDepth::U16;
        assert!(
            WorkingEncoder::new(
                space,
                &high_depth,
                OutputEncoding {
                    dither: OutputDither::Stochastic8,
                    ..Default::default()
                }
            )
            .err()
            .unwrap()
            .contains("8-bit")
        );
    }
}
