use super::*;
use layer_core::color::SampleDepth;

fn interpretation(space: RgbSpace, depth: SampleDepth) -> SourceInterpretation {
    SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth,
        profile: ColorProfile::Builtin(space),
        profile_assumed: false,
    }
}

#[test]
fn every_integer_code_and_hidden_rgb_survive_builtin_working_decode() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let source = interpretation(space, depth);
            let decoder = WorkingDecoder::new(&source, space, Default::default()).unwrap();
            let maximum = depth.maximum();
            let mut bytes = Vec::new();
            for code in 0..=maximum {
                for value in [
                    code,
                    maximum - code,
                    code.wrapping_mul(617) % (maximum + 1),
                    code % 3,
                ] {
                    bytes.extend_from_slice(&(value as u16).to_le_bytes()[..depth.bytes()]);
                }
            }
            let mut linear = vec![[0.; 4]; maximum as usize + 1];
            decoder.decode_pixels(&bytes, &mut linear).unwrap();
            for (code, pixel) in linear.into_iter().enumerate() {
                let code = code as u32;
                assert_eq!(pixel[3], (code % 3) as f32 / maximum as f32);
                for (value, expected) in pixel[..3].iter().zip([
                    code,
                    maximum - code,
                    code.wrapping_mul(617) % (maximum + 1),
                ]) {
                    assert_eq!(
                        (space.encode(f64::from(*value)) * f64::from(maximum)).round() as u32,
                        expected,
                        "{space:?} {depth:?} {code}"
                    );
                }
            }
        }
    }
}

#[test]
fn wide_gamut_working_conversion_retains_negative_and_above_one_values() {
    let options = ConversionOptions {
        black_point_compensation: false,
        ..Default::default()
    };
    for from in RgbSpace::ALL {
        for to in RgbSpace::ALL {
            for embedded in [false, true] {
                let mut source = interpretation(from, SampleDepth::U16);
                if embedded {
                    source.profile =
                        ColorProfile::Icc(profile_bytes(&source.profile).unwrap().into());
                }
                let decoder = WorkingDecoder::new(&source, to, options).unwrap();
                let values = [
                    [65535u16, 0, 0, 1],
                    [0, 65535, 0, 257],
                    [0, 0, 65535, 65535],
                    [65535; 4],
                ];
                let bytes: Vec<_> = values
                    .iter()
                    .flatten()
                    .flat_map(|v| v.to_le_bytes())
                    .collect();
                let mut output = [[0.; 4]; 4];
                decoder.decode_pixels(&bytes, &mut output).unwrap();
                for (original, actual) in values.into_iter().zip(output) {
                    let input = [original[0], original[1], original[2]]
                        .map(|v| from.decode(f64::from(v) / 65535.));
                    let expected = layer_core::color::rgb::apply(from.linear_transform(to), input);
                    for (a, b) in actual[..3].iter().zip(expected) {
                        // Serialized ICC XYZ tags have s15Fixed16 precision;
                        // builtin analytical primary matrices retain Float64.
                        let tolerance = if embedded { 0.0003 } else { 0.000002 };
                        assert!(
                            (f64::from(*a) - b).abs() <= tolerance,
                            "{from:?}->{to:?}, embedded={embedded}, actual={actual:?}, expected={expected:?}"
                        );
                    }
                    assert_eq!(actual[3], original[3] as f32 / 65535.);
                }
                if from == RgbSpace::DisplayP3 && to == RgbSpace::Srgb {
                    assert!(output[0][0] > 1.2 && output[0][1] < -0.04 && output[0][2] < -0.01);
                }
            }
        }
    }
}

#[test]
fn gray_profiles_alpha_and_source_edge_padding_are_independent() {
    let mut source = interpretation(RgbSpace::Srgb, SampleDepth::U16);
    source.channels = SourceChannels::GrayAlpha;
    source.profile = matrix_profile(
        RgbSpace::Srgb.white(),
        RgbSpace::Srgb.primaries(),
        Some(0.5),
        true,
    )
    .unwrap();
    let decoder = WorkingDecoder::new(&source, RgbSpace::Srgb, Default::default()).unwrap();
    let mut builder = SourceBuilder::new([1, 1], source, 1024 * 1024).unwrap();
    let bytes: Vec<_> = [32768u16, 257]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    builder.push_row(&bytes).unwrap();
    let image = builder.finish().unwrap();
    let mut output = vec![[0.; 4]; (TILE_SIZE * TILE_SIZE) as usize];
    let cache = layer_core::raster::DecodedTileCache::new(0);
    decoder
        .decode_tile_cached(&image, [0, 0], &mut output, &cache)
        .unwrap();
    for value in &output[0][..3] {
        assert!((*value - (32768. / 65535f32).powi(2)).abs() < 0.00003);
    }
    assert_eq!(output[0][3], 257. / 65535.);
    assert!(output[1..].iter().all(|p| *p == [0.; 4]));
    assert!(
        decoder
            .decode_tile_cached(&image, [1, 0], &mut output, &cache)
            .is_err()
    );
    assert!(
        decoder
            .decode_pixels(&bytes[..3], &mut output[..1])
            .is_err()
    );
    let mut changed = image.clone();
    changed.interpretation.profile = ColorProfile::default();
    assert!(
        decoder
            .decode_tile_cached(&changed, [0, 0], &mut output, &cache)
            .is_err()
    );
}
