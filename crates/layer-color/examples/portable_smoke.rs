use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
use layer_core::color::{ColorProfile, RgbSpace, SampleDepth};

/// HEIC dispatch, high precision and a rotated partial grid in actual Wasm.
#[unsafe(no_mangle)]
pub extern "C" fn portable_heif() -> u32 {
    use layer_color::photo::{DecodeLimits, read_photo};
    use std::io::Cursor;
    for (encoded, extent, depth, profile) in [
        (
            include_bytes!("../tests/fixtures/heif/flat-red-8bit.heic").as_slice(),
            [64, 64],
            SampleDepth::U8,
            RgbSpace::Srgb,
        ),
        (
            include_bytes!("../tests/fixtures/heif/p3-grid-8bit.heic").as_slice(),
            [51, 101],
            SampleDepth::U8,
            RgbSpace::DisplayP3,
        ),
        (
            include_bytes!("../tests/fixtures/heif/p3-gray-10bit.heic").as_slice(),
            [64, 64],
            SampleDepth::U16,
            RgbSpace::DisplayP3,
        ),
    ] {
        let limits = DecodeLimits {
            codec_bytes: 16 * 1024 * 1024,
            source_bytes: 1024 * 1024,
            dimension: 128,
        };
        assert!(
            read_photo(
                Cursor::new(encoded),
                DecodeLimits {
                    codec_bytes: 1024 * 1024,
                    ..limits
                }
            )
            .is_err()
        );
        let source = read_photo(Cursor::new(encoded), limits).unwrap();
        assert_eq!(source.extent, extent);
        assert_eq!(source.interpretation.depth, depth);
        assert_eq!(
            source.interpretation.profile,
            ColorProfile::Builtin(profile)
        );
        let mut row = vec![0; source.row_bytes()];
        let mut rows = source.rows();
        for y in 0..extent[1] {
            rows.read(y, &mut row).unwrap();
            if depth == SampleDepth::U16 {
                for p in row.chunks_exact(8) {
                    let value = u16::from_le_bytes([p[0], p[1]]);
                    assert_eq!(value, 33504);
                    assert_eq!(&p[0..2], &p[2..4]);
                    assert_eq!(&p[0..2], &p[4..6]);
                    assert_eq!(&p[6..8], &[255, 255]);
                }
            } else {
                for p in row.chunks_exact(4) {
                    assert_eq!(p[3], 255);
                }
                if extent == [64, 64] {
                    assert!(row[0] >= 250 && row[1] <= 2 && row[2] <= 2);
                } else if y < 35 {
                    assert!(row[2] >= 250 && row[0] <= 2);
                } else if y > 40 {
                    assert!(row[0] >= 250 && row[2] <= 2);
                }
            }
        }
    }
    3
}

/// Execute both lossy and lossless AVIF export in actual Wasm. The odd
/// extent exercises padded grids; decoded HDR and alpha must survive both.
#[unsafe(no_mangle)]
pub extern "C" fn portable_avif_export() -> u32 {
    use layer_color::photo::{GainMapFormat, read_photo, write_gainmap_rows};
    use std::{io::Cursor, sync::atomic::AtomicBool};
    let cancel = AtomicBool::new(false);
    let extent = [23, 17];
    let pixels = |y, row: &mut [[f32; 4]]| {
        for (x, p) in row.iter_mut().enumerate() {
            let a = if x < 11 { 0.375 } else { 1. };
            *p = [(if y < 8 { 4. } else { 0.02 }) * a, 0.2 * a, 0.1 * a, a];
        }
        Ok(())
    };
    let guide =
        layer_color::build_local_tone_guide(extent, RgbSpace::Srgb, || false, pixels).unwrap();
    for quality in [30, 100] {
        let mut bytes = Vec::new();
        write_gainmap_rows(
            &mut bytes,
            extent,
            RgbSpace::Srgb,
            Default::default(),
            &guide,
            GainMapFormat::Avif,
            quality,
            None,
            None,
            false,
            &cancel,
            pixels,
        )
        .unwrap();
        let source = read_photo(Cursor::new(&bytes), Default::default()).unwrap();
        assert_eq!(source.extent, extent);
        assert_eq!(source.interpretation.depth, SampleDepth::F16);
        let mut row = vec![0; source.row_bytes()];
        let mut rows = source.rows();
        for y in 0..extent[1] {
            rows.read(y, &mut row).unwrap();
            for (x, p) in row.chunks_exact(8).enumerate() {
                let pixel = layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| {
                    u16::from_le_bytes([p[c * 2], p[c * 2 + 1]])
                }))
                .unwrap();
                let expected = [
                    if y < 8 { 4. } else { 0.02 },
                    0.2,
                    0.1,
                    if x < 11 { 0.375 } else { 1. },
                ];
                for c in 0..3 {
                    let tolerance = if quality == 100 { 0.01 } else { 0.03 + 0.04 * expected[c] };
                    assert!((pixel[c] - expected[c]).abs() < tolerance);
                }
                assert!((pixel[3] - expected[3]).abs() < 0.0003);
            }
        }
    }
    2
}
#[unsafe(no_mangle)]
pub extern "C" fn portable_smoke() -> u32 {
    let profile = ColorProfile::Builtin(RgbSpace::DisplayP3);
    let bytes = layer_color::profile_bytes(&profile).unwrap();
    let embedded = ColorProfile::Icc(bytes.into());
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgb,
        depth: SampleDepth::U8,
        profile: embedded.clone(),
        profile_assumed: false,
    };
    let mut values = [[0.; 4]];
    layer_color::WorkingDecoder::new(&interpretation, RgbSpace::Srgb, Default::default())
        .unwrap()
        .decode_pixels(&[64, 128, 191], &mut values)
        .unwrap();
    assert_eq!(values[0][3], 1.);
    assert!(values[0].iter().all(|v| v.is_finite()));
    let mut builder = SourceBuilder::new([16, 8], interpretation, 1024 * 1024).unwrap();
    for _ in 0..8 {
        builder.push_row(&[40, 100, 170].repeat(16)).unwrap();
    }
    let source = builder.finish().unwrap();
    let mut jpeg = Vec::new();
    layer_color::photo::write_jpeg(&mut jpeg, &source, 100).unwrap();
    let restored =
        layer_color::photo::read_photo(std::io::Cursor::new(jpeg), Default::default()).unwrap();
    assert_eq!(restored.extent, source.extent);
    assert_eq!(restored.interpretation.profile, embedded);
    let mut row = [0; 48];
    restored.rows().read(0, &mut row).unwrap();
    assert!(row.chunks_exact(3).all(|p| {
        p.iter()
            .zip([40u8, 100, 170])
            .all(|(a, b)| a.abs_diff(b) <= 2)
    }));
    let gray = SourceInterpretation {
        channels: SourceChannels::Gray,
        depth: SampleDepth::U8,
        profile: layer_color::gray_profile(RgbSpace::Srgb).unwrap(),
        profile_assumed: false,
    };
    layer_color::WorkingDecoder::new(&gray, RgbSpace::DisplayP3, Default::default())
        .unwrap()
        .decode_pixels(&[128], &mut values)
        .unwrap();
    let encoded = RgbSpace::DisplayP3.encode(f64::from(values[0][0]));
    assert!(encoded > 0.49 && encoded < 0.51);
    1
}

/// Exercises application dispatch, Rust AV1, HDR reconstruction and raster
/// storage together in browsers, with no host codec imports.
#[unsafe(no_mangle)]
pub extern "C" fn portable_avif() -> u32 {
    use layer_color::photo::{DecodeLimits, read_photo_detailed_with_cancel};
    use std::{
        io::Cursor,
        sync::atomic::{AtomicBool, Ordering},
    };
    let cancel = AtomicBool::new(false);
    let limits = DecodeLimits {
        codec_bytes: 16 * 1024 * 1024,
        source_bytes: 8 * 1024 * 1024,
        dimension: 128,
    };
    let fixtures: [(&[u8], &[u8], SampleDepth, [u32; 2]); 3] = [
        (
            include_bytes!("../tests/fixtures/avif/p3-12bit.avif"),
            include_bytes!("../tests/fixtures/avif/p3-12bit.rgba16"),
            SampleDepth::U16,
            [64, 32],
        ),
        (
            include_bytes!("../tests/fixtures/avif/hdr-rgb.avif"),
            include_bytes!("../tests/fixtures/avif/hdr-rgb.rgba16f"),
            SampleDepth::F16,
            [16, 12],
        ),
        (
            include_bytes!("../tests/fixtures/avif/hdr-small-gray.avif"),
            include_bytes!("../tests/fixtures/avif/hdr-small-gray.rgba16f"),
            SampleDepth::F16,
            [16, 12],
        ),
    ];
    for (encoded, expected, depth, extent) in fixtures {
        cancel.store(true, Ordering::Release);
        assert!(read_photo_detailed_with_cancel(Cursor::new(encoded), limits, &cancel).is_err());
        cancel.store(false, Ordering::Release);
        assert!(
            read_photo_detailed_with_cancel(
                Cursor::new(encoded),
                DecodeLimits {
                    codec_bytes: 128 * 1024,
                    ..limits
                },
                &cancel
            )
            .is_err()
        );
        let photo = read_photo_detailed_with_cancel(Cursor::new(encoded), limits, &cancel).unwrap();
        assert_eq!(photo.source.extent, extent);
        assert_eq!(photo.source.interpretation.depth, depth);
        let mut rows = photo.source.rows();
        let mut row = vec![0; photo.source.row_bytes()];
        for (y, expected) in expected.chunks_exact(row.len()).enumerate() {
            rows.read(y as u32, &mut row).unwrap();
            if depth == SampleDepth::U16 {
                assert_eq!(row, expected);
            } else {
                for (a, b) in row.chunks_exact(8).zip(expected.chunks_exact(8)) {
                    let pixel = |b: &[u8]| {
                        layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| {
                            u16::from_le_bytes(b[c * 2..c * 2 + 2].try_into().unwrap())
                        }))
                        .unwrap()
                    };
                    let (a, b) = (pixel(a), pixel(b));
                    for c in 0..4 {
                        assert!((a[c].max(0.) - b[c]).abs() < 0.004);
                    }
                }
            }
        }
    }
    3
}
