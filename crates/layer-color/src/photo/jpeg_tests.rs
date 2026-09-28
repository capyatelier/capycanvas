use super::test_support::rows;
use super::*;
use std::io::Cursor;

fn pattern(channels: SourceChannels, space: RgbSpace) -> SourceImage {
    let mut builder = SourceBuilder::new(
        [257, 17],
        SourceInterpretation {
            channels,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(space),
            profile_assumed: false,
        },
        8 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..17 {
        let row: Vec<_> = (0..257)
            .flat_map(|x| {
                [32 + x / 2, 16 + y * 7, 192 - x / 3, 0][..channels.count()]
                    .iter()
                    .map(|v| *v as u8)
                    .collect::<Vec<_>>()
            })
            .collect();
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}

#[test]
fn camera_mpf_preview_does_not_replace_primary_pixels_or_profile() {
    let source = pattern(SourceChannels::Rgb, RgbSpace::DisplayP3);
    let mut jpeg = Vec::new();
    write_jpeg(&mut jpeg, &source, 100).unwrap();
    let expected = read_photo(Cursor::new(&jpeg), DecodeLimits::default()).unwrap();
    for little in [false, true] {
        let directory = super::jpeg_mpf::tests::directory(little, 0x010002);
        let mut camera = jpeg[..2].to_vec();
        camera.extend([0xff, 0xe2]);
        camera.extend(((directory.len() + 2) as u16).to_be_bytes());
        camera.extend(directory);
        camera.extend_from_slice(&jpeg[2..]);
        let preview = pattern(SourceChannels::Gray, RgbSpace::Srgb);
        write_jpeg(&mut camera, &preview, 90).unwrap();
        let actual = read_photo(Cursor::new(camera), DecodeLimits::default()).unwrap();
        assert_eq!(actual.extent, expected.extent);
        assert_eq!(actual.interpretation, expected.interpretation);
        assert_eq!(rows(&actual), rows(&expected));
    }
}

#[test]
fn profiled_rgb_gray_jpeg_rows_preserve_interpretation_and_archive_decoded_samples() {
    for space in RgbSpace::ALL {
        for channels in [SourceChannels::Rgb, SourceChannels::Gray] {
            let source = pattern(channels, space);
            for quality in [90, 100] {
                let mut bytes = Vec::new();
                write_jpeg(&mut bytes, &source, quality).unwrap();
                let decoded = read_photo(Cursor::new(&bytes), DecodeLimits::default()).unwrap();
                assert_eq!(decoded.extent, source.extent);
                assert_eq!(decoded.interpretation.channels, channels);
                assert_eq!(decoded.interpretation.depth, SampleDepth::U8);
                assert!(!decoded.interpretation.profile_assumed);
                let profile = if channels == SourceChannels::Gray {
                    crate::gray_profile(space).unwrap()
                } else {
                    source.interpretation.profile.clone()
                };
                assert_eq!(
                    profile_bytes(&decoded.interpretation.profile).unwrap(),
                    profile_bytes(&profile).unwrap()
                );
                let max = rows(&source)
                    .concat()
                    .iter()
                    .zip(rows(&decoded).concat())
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .unwrap();
                assert!(
                    max <= if quality == 100 { 3 } else { 8 },
                    "{channels:?} {space:?} q={quality}, max={max}"
                );
                let mut project = layer_core::Project {
                    document: layer_core::Document::new("JPEG master", 257, 17),
                };
                project.document.layers[0].source = Some(std::sync::Arc::new(decoded));
                let mut archive = Vec::new();
                project.write(&mut archive).unwrap();
                let reopened =
                    layer_core::Project::read(Cursor::new(archive), Default::default()).unwrap();
                assert_eq!(reopened, project);
            }
        }
    }
}

#[test]
fn jpeg_validation_provider_failure_and_truncation_do_not_publish_fake_success() {
    let source = pattern(SourceChannels::Rgb, RgbSpace::Srgb);
    for (depth, channels, quality) in [
        (SampleDepth::U16, SourceChannels::Rgb, 90),
        (SampleDepth::U8, SourceChannels::Rgba, 90),
        (SampleDepth::U8, SourceChannels::Rgb, 0),
        (SampleDepth::U8, SourceChannels::Rgb, 101),
    ] {
        let target = SourceInterpretation {
            depth,
            channels,
            ..source.interpretation.clone()
        };
        let mut bytes = Vec::new();
        assert!(
            write_jpeg_rows(
                &mut bytes,
                source.extent,
                &target,
                &Default::default(),
                JpegEncodeOptions::from_memory_budget(quality, PhotoMemoryBudget::current()),
                |_, _| panic!("invalid output requested pixels")
            )
            .is_err()
        );
        assert!(bytes.is_empty());
    }
    let mut calls = 0;
    let mut bytes = Vec::new();
    let error = write_jpeg_rows(
        &mut bytes,
        source.extent,
        &source.interpretation,
        &Default::default(),
        JpegEncodeOptions::from_memory_budget(90, PhotoMemoryBudget::current()),
        |y, row| {
            assert_eq!(y, calls);
            calls += 1;
            if y == 3 {
                Err("cancelled row provider".into())
            } else {
                row.fill(0);
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(calls, 4);
    assert_eq!(error, "cancelled row provider");
    assert!(read_photo(Cursor::new(bytes), DecodeLimits::default()).is_err());
    let mut bytes = Vec::new();
    write_jpeg(&mut bytes, &source, 100).unwrap();
    for length in [0, 1, 10, bytes.len() / 2, bytes.len() - 2] {
        assert!(read_photo(Cursor::new(&bytes[..length]), DecodeLimits::default()).is_err());
    }
    let mut prefixed = Cursor::new([b"prefix".as_slice(), &bytes].concat());
    prefixed.set_position(6);
    assert_eq!(
        read_photo(prefixed, DecodeLimits::default()).unwrap().extent,
        source.extent
    );
}

#[test]
fn baseline_60mp_jpeg_checks_full_image_memory_before_decoding() {
    let extent = [8192, 7324];
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgb,
        depth: SampleDepth::U8,
        profile: ColorProfile::default(),
        profile_assumed: false,
    };
    let mut bytes = Vec::new();
    write_jpeg_rows(
        &mut bytes,
        extent,
        &interpretation,
        &Default::default(),
        JpegEncodeOptions {
            quality: 100,
            codec_bytes: 2 * 1024 * 1024 * 1024,
        },
        |_, row| {
            for p in row.chunks_exact_mut(3) {
                p.copy_from_slice(&[40, 100, 170]);
            }
            Ok(())
        },
    )
    .unwrap();
    let error = read_photo(
        Cursor::new(&bytes),
        DecodeLimits {
            codec_bytes: 8 * 1024 * 1024,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.contains("codec memory budget"), "{error}");
    let source = read_photo(
        Cursor::new(bytes),
        DecodeLimits {
            source_bytes: 16 * 1024 * 1024,
            codec_bytes: 512 * 1024 * 1024,
            dimension: 32768,
        },
    )
    .unwrap();
    assert_eq!(source.extent, extent);
    let mut rows = source.rows();
    let mut row = vec![0; source.row_bytes()];
    for y in [0, 1, 255, 256, extent[1] - 1] {
        rows.read(y, &mut row).unwrap();
        for p in row.chunks_exact(3) {
            for (a, b) in p.iter().zip([40u8, 100, 170]) {
                assert!(a.abs_diff(b) <= 2);
            }
        }
    }
}

#[test]
#[ignore = "requires independently produced CMYK and YCCK fixtures"]
fn external_cmyk_jpeg_variants_match_reference_ink_samples() {
    let dir = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_JPEG_FIXTURES").expect("fixture directory"),
    );
    for name in ["cmyk", "ycck"] {
        let expected = std::fs::read(dir.join(format!("{name}.raw"))).unwrap();
        let decoded = read_photo(
            std::io::BufReader::new(std::fs::File::open(dir.join(format!("{name}.jpg"))).unwrap()),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.interpretation.channels, SourceChannels::Cmyk);
        let actual = rows(&decoded).concat();
        assert_eq!(actual.len(), expected.len());
        assert!(
            actual
                .iter()
                .zip(&expected)
                .all(|(a, b)| a.abs_diff(*b) <= 1),
            "{name}: ink values changed"
        );
        let mut output = Vec::new();
        write_jpeg(&mut output, &decoded, 100).unwrap();
        std::fs::write(dir.join(format!("capy-{name}.jpg")), &output).unwrap();
        let restored = read_photo(Cursor::new(output), DecodeLimits::default()).unwrap();
        assert_eq!(
            restored.interpretation.profile,
            decoded.interpretation.profile
        );
        assert_eq!(restored.extent, decoded.extent);
    }
}

#[test]
#[ignore = "requires Pillow progressive and EXIF-oriented JPEG fixtures"]
fn external_progressive_and_oriented_jpeg_match_reference_samples_and_budget() {
    let dir = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_JPEG_FIXTURES").expect("fixture directory"),
    );
    for (name, extent, channels, assumed) in [
        ("progressive", [513, 259], SourceChannels::Rgb, false),
        ("progressive-gray", [513, 259], SourceChannels::Gray, true),
        ("oriented", [259, 513], SourceChannels::Rgb, false),
    ] {
        let bytes = std::fs::read(dir.join(format!("{name}.jpg"))).unwrap();
        let decoded = read_photo(Cursor::new(&bytes), DecodeLimits::default()).unwrap();
        assert_eq!(decoded.extent, extent);
        assert_eq!(decoded.interpretation.channels, channels);
        assert_eq!(decoded.interpretation.profile_assumed, assumed);
        let expected = std::fs::read(dir.join(format!("{name}.raw"))).unwrap();
        assert_eq!(expected.len(), decoded.row_bytes() * extent[1] as usize);
        let maximum = rows(&decoded)
            .concat()
            .iter()
            .zip(&expected)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(maximum <= 1, "{name}: maximum code difference {maximum}");
        let error = read_photo(
            Cursor::new(&bytes),
            DecodeLimits {
                codec_bytes: 1024,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("codec memory budget"), "{name}: {error}");
    }
    let file =
        || std::io::BufReader::new(std::fs::File::open(dir.join("progressive-60mp.jpg")).unwrap());
    let error = read_photo(
        file(),
        DecodeLimits {
            codec_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.contains("codec memory budget"), "{error}");
    let source = read_photo(
        file(),
        DecodeLimits {
            source_bytes: 16 * 1024 * 1024,
            codec_bytes: 768 * 1024 * 1024,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(source.extent, [8192, 7324]);
    let mut rows = source.rows();
    let mut row = vec![0; source.row_bytes()];
    for y in [0, 255, 256, 7323] {
        rows.read(y, &mut row).unwrap();
        assert!(row.chunks_exact(3).all(|p| {
            p.iter()
                .zip([40u8, 100, 170])
                .all(|(a, b)| a.abs_diff(b) <= 2)
        }));
    }
}

#[test]
fn export_budget_rejects_before_requesting_rows_or_writing_output() {
    let source = pattern(SourceChannels::Rgb, RgbSpace::Srgb);
    let options =
        JpegEncodeOptions::from_memory_budget(100, PhotoMemoryBudget::from_available_memory(1024));
    let mut output = Vec::new();
    let error = write_jpeg_rows(
        &mut output,
        source.extent,
        &source.interpretation,
        &Default::default(),
        options,
        |_, _| panic!("over-budget export requested pixels"),
    )
    .unwrap_err();
    assert!(error.contains("codec memory budget"));
    assert!(output.is_empty());
    jpeg_codec::admit([8, 8], 3, 0, 3 * 1024 * 1024).unwrap();
    assert!(
        jpeg_codec::admit([8, 8], 3, 512 * 1024, 3 * 1024 * 1024)
            .unwrap_err()
            .contains("codec memory budget")
    );
}
