use super::*;

#[test]
fn gray_and_gray_alpha_delivery_preserve_samples_and_embed_matching_profiles() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            for channels in [SourceChannels::Gray, SourceChannels::GrayAlpha] {
                let mut builder = SourceBuilder::new([513, 3], SourceInterpretation {
                    channels, depth, profile: ColorProfile::Builtin(space), profile_assumed: false,
                }, 8*1024*1024).unwrap();
                let maximum = depth.maximum();
                for y in 0..3 {
                    let row: Vec<_> = (0..513).flat_map(|x| {
                        (0..channels.count()).map(move |c| if c == 0 { (x*127 + y*257) & maximum } else { x%3 })
                    }).flat_map(|v| (v as u16).to_le_bytes()[..depth.bytes()].to_vec()).collect();
                    builder.push_row(&row).unwrap();
                }
                let source = builder.finish().unwrap();
                for tiff in [false, true] {
                    let mut file = std::io::Cursor::new(Vec::new());
                    if tiff { write_tiff(&mut file, &source).unwrap(); } else { write_png(&mut file, &source).unwrap(); }
                    file.set_position(0);
                    let actual = read_photo(file, DecodeLimits::default()).unwrap();
                    assert_eq!(actual.interpretation.channels, channels);
                    assert_eq!(actual.interpretation.depth, depth);
                    assert!(!actual.interpretation.profile_assumed);
                    if let ColorProfile::Icc(_) = actual.interpretation.profile {
                        assert_eq!(profile_channels(&actual.interpretation.profile).unwrap(), ProfileChannels::Gray);
                        assert_eq!(actual.interpretation.profile, crate::gray_profile(space).unwrap());
                    } else { assert_eq!(space, RgbSpace::Srgb); }
                    for (key, expected) in &source.tiles {
                        assert_eq!(actual.tiles[key].digest, expected.digest, "{space:?} {depth:?} {channels:?} TIFF={tiff}");
                    }
                }
            }
        }
    }
}
use std::io::Cursor;

pub(super) fn fixture(depth: SampleDepth, profile: ColorProfile) -> SourceImage {
    let mut builder = SourceBuilder::new(
        [257, 259],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth,
            profile,
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..259u32 {
        let mut row = Vec::new();
        for x in 0..257u32 {
            for value in [
                x * 251,
                (x + y) * 617,
                x ^ (y * 17),
                if x % 3 == 0 { 0 } else { 65535 - y * 253 },
            ] {
                if depth == SampleDepth::U8 {
                    row.push(value as u8);
                } else {
                    row.extend_from_slice(&(value as u16).to_le_bytes());
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}

pub(super) fn exact_pixels(before: &SourceImage, after: &SourceImage) {
    assert_eq!(before.extent, after.extent);
    assert_eq!(before.interpretation.depth, after.interpretation.depth);
    assert_eq!(
        before.interpretation.channels,
        after.interpretation.channels
    );
    assert_eq!(before.tiles.len(), after.tiles.len());
    for (key, tile) in &before.tiles {
        assert_eq!(tile.digest, after.tiles[key].digest);
    }
}

#[test]
fn profiled_png_tiff_roundtrip_every_source_code_including_transparency() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for space in RgbSpace::ALL {
            // Store an actual embedded payload, not just a built-in label.
            let profile =
                ColorProfile::Icc(profile_bytes(&ColorProfile::Builtin(space)).unwrap().into());
            let source = fixture(depth, profile.clone());
            for tiff in [false, true] {
                let mut encoded = Cursor::new(Vec::new());
                if tiff {
                    write_tiff(&mut encoded, &source).unwrap();
                } else {
                    write_png(&mut encoded, &source).unwrap();
                }
                encoded.set_position(0);
                let after = read_photo(encoded, DecodeLimits::default()).unwrap();
                exact_pixels(&source, &after);
                assert_eq!(after.interpretation.profile, profile);
                assert!(!after.interpretation.profile_assumed);
            }
        }
    }
}

fn tagged_png(mut info: png::Info<'static>) -> Vec<u8> {
    info.width = 1;
    info.height = 1;
    info.color_type = png::ColorType::Rgb;
    info.bit_depth = png::BitDepth::Eight;
    let cicp = info.coding_independent_code_points;
    let mut encoded = Vec::new();
    let mut writer = png::Encoder::with_info(&mut encoded, info)
        .unwrap()
        .write_header()
        .unwrap();
    // png 0.18.1 reads cICP but its Info encoder does not emit this field.
    if let Some(cicp) = cicp {
        writer
            .write_chunk(
                png::chunk::cICP,
                &[
                    cicp.color_primaries,
                    cicp.transfer_function,
                    cicp.matrix_coefficients,
                    u8::from(cicp.is_video_full_range_image),
                ],
            )
            .unwrap();
    }
    writer.write_image_data(&[64, 128, 192]).unwrap();
    writer.finish().unwrap();
    encoded
}

#[test]
fn png_color_precedence_and_explicit_unsupported_interpretation() {
    let mut info = png::Info::default();
    let source = read_png(
        Cursor::new(tagged_png(info.clone())),
        DecodeLimits::default(),
    )
    .unwrap();
    assert!(source.interpretation.profile_assumed);
    info.srgb = Some(png::SrgbRenderingIntent::Perceptual);
    info.coding_independent_code_points = Some(png::CodingIndependentCodePoints {
        color_primaries: 12,
        transfer_function: 13,
        matrix_coefficients: 0,
        is_video_full_range_image: true,
    });
    let source = read_png(
        Cursor::new(tagged_png(info.clone())),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        source.interpretation.profile,
        ColorProfile::Builtin(RgbSpace::DisplayP3)
    );
    assert!(!source.interpretation.profile_assumed);
    info.coding_independent_code_points
        .as_mut()
        .unwrap()
        .transfer_function = 16;
    assert!(
        read_png(Cursor::new(tagged_png(info)), DecodeLimits::default())
            .unwrap_err()
            .contains("HDR")
    );
}

#[test]
fn gamma_only_png_retains_its_actual_interpretation() {
    let mut info = png::Info::default();
    info.gama_chunk = Some(png::ScaledFloat::from_scaled(100000));
    info.source_gamma = info.gama_chunk;
    let source = read_png(Cursor::new(tagged_png(info)), DecodeLimits::default()).unwrap();
    assert!(!source.interpretation.profile_assumed);
    let transform = crate::RgbTransform::new(
        &source.interpretation.profile,
        &ColorProfile::default(),
        Default::default(),
    )
    .unwrap();
    let mut gray = [[0.5, 0.5, 0.5, 1.]];
    transform.apply(&mut gray);
    assert!((gray[0][0] - 0.735357).abs() < 0.0002);
}

#[test]
fn malformed_png_profile_is_not_an_untagged_image() {
    let mut encoded = Vec::new();
    let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
    encoder.set_color(png::ColorType::Rgb);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_chunk(png::chunk::iCCP, b"source\0\0not a zlib stream")
        .unwrap();
    writer.write_image_data(&[64, 128, 192]).unwrap();
    writer.finish().unwrap();
    assert!(
        read_png(Cursor::new(encoded), DecodeLimits::default())
            .unwrap_err()
            .contains("unreadable ICC")
    );
}

#[test]
fn interrupted_output_and_tiny_source_budget_return_errors() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let source = fixture(SampleDepth::U16, ColorProfile::default());
    assert!(
        write_png(Broken, &source)
            .unwrap_err()
            .contains("disk full")
    );
    let mut encoded = Vec::new();
    write_png(&mut encoded, &source).unwrap();
    assert!(
        read_png(
            Cursor::new(encoded),
            DecodeLimits {
                source_bytes: 1,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("memory budget")
    );
}
