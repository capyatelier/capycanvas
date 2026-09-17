use super::*;

#[test]
fn builtin_profiles_are_stable_and_preserve_original_payloads() {
    for space in RgbSpace::ALL {
        let profile = ColorProfile::Builtin(space);
        let bytes = profile_bytes(&profile).unwrap();
        assert_eq!(&bytes[24..36], &[7, 234, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes, profile_bytes(&profile).unwrap());
        let embedded = ColorProfile::Icc(bytes.clone().into());
        assert_eq!(profile_bytes(&embedded).unwrap(), bytes);
        assert_eq!(profile_channels(&embedded).unwrap(), ProfileChannels::Rgb);
        assert_eq!(profile_description(&embedded).unwrap(), space.name());
    }
}

#[test]
fn unsupported_black_point_compensation_is_explicit_for_all_conversion_paths() {
    use layer_core::color::{OutputEncoding, source::*};
    let options = ConversionOptions {
        black_point_compensation: true,
        ..Default::default()
    };
    let profile = ColorProfile::default();
    let source = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: layer_core::color::SampleDepth::U8,
        profile: profile.clone(),
        profile_assumed: false,
    };
    assert!(
        RgbTransform::new(&profile, &profile, options)
            .err()
            .unwrap()
            .contains("Black point")
    );
    assert!(
        WorkingDecoder::new(&source, RgbSpace::Srgb, options)
            .err()
            .unwrap()
            .contains("Black point")
    );
    assert!(
        WorkingEncoder::new(
            RgbSpace::Srgb,
            &source,
            OutputEncoding {
                conversion: options,
                ..Default::default()
            }
        )
        .err()
        .unwrap()
        .contains("Black point")
    );
    assert!(
        InputTransform::new(&gray_profile(RgbSpace::Srgb).unwrap(), &profile, options)
            .err()
            .unwrap()
            .contains("Black point")
    );
}

#[test]
fn absolute_intent_preserves_media_white_and_alpha() {
    let mut input = linear_profile(RgbSpace::Srgb).unwrap();
    let output = input.clone();
    let white = input.media_white_point.as_mut().unwrap();
    white.x *= 0.8;
    white.y *= 0.8;
    white.z *= 0.8;
    let relative =
        CompiledTransform::<4, 4>::new(&input, &output, ConversionOptions::default()).unwrap();
    let absolute = CompiledTransform::<4, 4>::new(
        &input,
        &output,
        ConversionOptions {
            intent: RenderingIntent::AbsoluteColorimetric,
            ..Default::default()
        },
    )
    .unwrap();
    let mut a = [[0.5, 0.25, 1., 0.375]];
    let mut b = a;
    relative.transform_in_place(&mut a);
    absolute.transform_in_place(&mut b);
    for c in 0..3 {
        assert!((b[0][c] - a[0][c] * 0.8).abs() < 1e-6);
    }
    assert_eq!(a[0][3], b[0][3]);
}

#[test]
fn malformed_profiles_fail_even_for_identity_requests() {
    let valid = profile_bytes(&ColorProfile::default()).unwrap();
    let mut cases = vec![vec![], valid[..128].to_vec()];
    let mut truncated_tag = valid.clone();
    truncated_tag[140..144].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(truncated_tag);
    let mut tag_count = valid;
    tag_count[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(tag_count);
    for bytes in cases {
        let profile = ColorProfile::Icc(bytes.into());
        assert!(RgbTransform::new(&profile, &profile, ConversionOptions::default()).is_err());
    }
}

#[test]
fn identity_preserves_every_u16_sample_and_alpha_bits() {
    for space in RgbSpace::ALL {
        let profile = ColorProfile::Builtin(space);
        let transform =
            RgbTransform::new(&profile, &profile, ConversionOptions::default()).unwrap();
        let original: Vec<_> = (0..=65535)
            .map(|v| [v as f32 / 65535., 0.33333334, -0.125, 0.])
            .collect();
        let mut pixels = original.clone();
        transform.apply(&mut pixels);
        assert_eq!(pixels, original);
    }
}

#[test]
fn portable_cmm_matches_independent_float64_standard_space_conversion() {
    let options = ConversionOptions {
        black_point_compensation: false,
        ..Default::default()
    };
    for source in RgbSpace::ALL {
        for destination in RgbSpace::ALL {
            let transform = RgbTransform::new(
                &ColorProfile::Builtin(source),
                &ColorProfile::Builtin(destination),
                options,
            )
            .unwrap();
            // Test the unclipped in-gamut interior. ICC serialized XYZ tags have
            // s15Fixed16 rounding, unlike the analytic reference's full matrices.
            let original: Vec<_> = (0..9 * 9 * 9)
                .map(|i| {
                    [
                        0.1 + (i % 9) as f32 * 0.1,
                        0.1 + (i / 9 % 9) as f32 * 0.1,
                        0.1 + (i / 81) as f32 * 0.1,
                        (i % 11) as f32 / 10.,
                    ]
                })
                .collect();
            let mut converted = original.clone();
            transform.apply(&mut converted);
            for (before, after) in original.into_iter().zip(converted) {
                assert_eq!(before[3].to_bits(), after[3].to_bits());
                let expected = source.convert(
                    destination,
                    [before[0] as f64, before[1] as f64, before[2] as f64],
                );
                if expected.into_iter().all(|v| (0.05..=1.).contains(&v)) {
                    for (actual, reference) in after[..3].iter().zip(expected) {
                        assert!(
                            (f64::from(*actual) - reference).abs() < 0.0003,
                            "{source:?}->{destination:?}: {before:?}: {after:?} vs {expected:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn gray_profile_is_converted_before_rgb_expansion() {
    let source = matrix_profile(
        RgbSpace::Srgb.white(),
        RgbSpace::Srgb.primaries(),
        Some(1. / 2.2),
        true,
    )
    .unwrap();
    assert_eq!(profile_channels(&source).unwrap(), ProfileChannels::Gray);
    assert!(RgbTransform::new(&source, &ColorProfile::default(), Default::default()).is_err());
    let transform =
        InputTransform::new(&source, &ColorProfile::default(), Default::default()).unwrap();
    let mut output = [[0.; 4]; 3];
    transform.gray(&[[0.], [0.5], [1.]], &mut output).unwrap();
    for (value, pixel) in [0f64, 0.5, 1.].into_iter().zip(output) {
        let expected = RgbSpace::Srgb.encode(value.powf(2.2));
        assert!(
            pixel[..3]
                .iter()
                .all(|v| (f64::from(*v) - expected).abs() < 0.0002)
        );
        assert_eq!(pixel[3], 1.);
    }
    assert!(transform.gray(&[[0.]], &mut output).is_err());
    assert!(transform.cmyk_percent(&[[0.; 4]; 3], &mut output).is_err());
}

#[test]
fn transform_moves_to_a_worker_and_outlives_construction_profiles() {
    let transform = RgbTransform::new(
        &ColorProfile::Builtin(RgbSpace::DisplayP3),
        &ColorProfile::Builtin(RgbSpace::ProPhoto),
        Default::default(),
    )
    .unwrap();
    std::thread::spawn(move || {
        let mut pixels = [[0.25, 0.5, 0.75, 0.03125]; 128];
        transform.apply(&mut pixels);
        assert!(
            pixels
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && p[3] == 0.03125)
        );
    })
    .join()
    .unwrap();
}
