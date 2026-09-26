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
    let input = [[0.5, 0.25, 1., 0.375]];
    let (mut a, mut b) = ([[0.; 4]], [[0.; 4]]);
    relative.transform_pixels(&input, &mut a);
    absolute.transform_pixels(&input, &mut b);
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
        let source = rgba16(ColorProfile::Icc(bytes.into()));
        assert!(WorkingDecoder::new(&source, RgbSpace::Srgb, Default::default()).is_err());
    }
}

fn rgba16(profile: ColorProfile) -> layer_core::color::source::SourceInterpretation {
    layer_core::color::source::SourceInterpretation {
        channels: layer_core::color::source::SourceChannels::Rgba,
        depth: layer_core::color::SampleDepth::U16,
        profile,
        profile_assumed: false,
    }
}

#[test]
fn portable_cmm_matches_independent_float64_standard_space_conversion() {
    let codes: Vec<[u16; 4]> = (0..9 * 9 * 9)
        .map(|i| {
            [
                0.1 + (i % 9) as f64 * 0.1,
                0.1 + (i / 9 % 9) as f64 * 0.1,
                0.1 + (i / 81) as f64 * 0.1,
                (i % 11) as f64 / 10.,
            ]
            .map(|v| (v * 65535.).round() as u16)
        })
        .collect();
    let bytes: Vec<u8> = codes
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    for source in RgbSpace::ALL {
        let profile = ColorProfile::Icc(
            profile_bytes(&ColorProfile::Builtin(source))
                .unwrap()
                .into(),
        );
        for destination in RgbSpace::ALL {
            let decoder =
                WorkingDecoder::new(&rgba16(profile.clone()), destination, Default::default())
                    .unwrap();
            let mut linear = vec![[0.; 4]; codes.len()];
            decoder.decode_pixels(&bytes, &mut linear).unwrap();
            for (code, after) in codes.iter().zip(linear) {
                let before = code.map(|v| f64::from(v) / 65535.);
                assert_eq!(after[3], code[3] as f32 / 65535.);
                let expected = source.convert(destination, [before[0], before[1], before[2]]);
                // Test the unclipped in-gamut interior. ICC serialized XYZ tags have
                // s15Fixed16 rounding, unlike the analytic reference's full matrices.
                if expected.into_iter().all(|v| (0.05..=1.).contains(&v)) {
                    for (actual, reference) in after[..3].iter().zip(expected) {
                        let actual = destination.encode(f64::from(*actual));
                        assert!(
                            (actual - reference).abs() < 0.0003,
                            "{source:?}->{destination:?}: {before:?}: {actual} vs {expected:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn gray_profile_is_converted_before_rgb_expansion() {
    use layer_core::color::source::SourceChannels;
    let profile = matrix_profile(
        RgbSpace::Srgb.white(),
        RgbSpace::Srgb.primaries(),
        Some(1. / 2.2),
        true,
    )
    .unwrap();
    assert_eq!(profile_channels(&profile).unwrap(), ProfileChannels::Gray);
    let mut source = rgba16(profile);
    assert!(WorkingDecoder::new(&source, RgbSpace::Srgb, Default::default()).is_err());
    source.channels = SourceChannels::Gray;
    let decoder = WorkingDecoder::new(&source, RgbSpace::Srgb, Default::default()).unwrap();
    let codes = [0u16, 32768, 65535];
    let bytes: Vec<u8> = codes.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut output = [[0.; 4]; 3];
    decoder.decode_pixels(&bytes, &mut output).unwrap();
    for (code, pixel) in codes.into_iter().zip(output) {
        let expected = RgbSpace::Srgb.encode((f64::from(code) / 65535.).powf(2.2));
        assert!(
            pixel[..3]
                .iter()
                .all(|v| (RgbSpace::Srgb.encode(f64::from(*v)) - expected).abs() < 0.0002)
        );
        assert_eq!(pixel[3], 1.);
    }
    assert!(decoder.decode_pixels(&bytes[..2], &mut output).is_err());
}
