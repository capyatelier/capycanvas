use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
use layer_core::color::{ColorProfile, RgbSpace, SampleDepth};

/// Execute both lossy-base and lossless AVIF export in actual Wasm. The odd
/// extent exercises padded grids; decoded HDR and alpha must survive both.
#[unsafe(no_mangle)]
pub extern "C" fn portable_avif_export() -> u32 {
    use layer_color::photo::{GainMapFormat, read_photo, write_gainmap_rows};
    use std::{io::Cursor, sync::atomic::AtomicBool};
    let cancel = AtomicBool::new(false);
    let extent = [23, 17];
    for quality in [30, 100] {
        let mut bytes = Vec::new();
        write_gainmap_rows(
            &mut bytes,
            extent,
            RgbSpace::Srgb,
            Default::default(),
            GainMapFormat::Avif,
            quality,
            None,
            None,
            false,
            &cancel,
            |y, row| {
                for (x, p) in row.iter_mut().enumerate() {
                    let a = if x < 11 { 0.375 } else { 1. };
                    *p = [(if y < 8 { 4. } else { 0.02 }) * a, 0.2 * a, 0.1 * a, a];
                }
                Ok(())
            },
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
                    assert!((pixel[c] - expected[c]).abs() < 0.01);
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
    let transform =
        layer_color::RgbTransform::new(&embedded, &ColorProfile::default(), Default::default())
            .unwrap();
    let mut values = [[0.25, 0.5, 0.75, 0.375]];
    transform.apply(&mut values);
    assert_eq!(values[0][3], 0.375);
    assert!(values[0].iter().all(|v| v.is_finite()));
    let mut builder = SourceBuilder::new(
        [16, 8],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: embedded.clone(),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for _ in 0..8 {
        builder.push_row(&[40, 100, 170].repeat(16)).unwrap();
    }
    let source = builder.finish().unwrap();
    let mut jpeg = Vec::new();
    layer_color::photo::write_jpeg(&mut jpeg, &source, 100).unwrap();
    let restored =
        layer_color::photo::read_jpeg(std::io::Cursor::new(jpeg), Default::default()).unwrap();
    assert_eq!(restored.extent, source.extent);
    assert_eq!(restored.interpretation.profile, embedded);
    let mut row = [0; 48];
    restored.rows().read(0, &mut row).unwrap();
    assert!(row.chunks_exact(3).all(|p| {
        p.iter()
            .zip([40u8, 100, 170])
            .all(|(a, b)| a.abs_diff(b) <= 2)
    }));
    let gray = layer_color::gray_profile(RgbSpace::Srgb).unwrap();
    let transform = layer_color::InputTransform::new(&gray, &profile, Default::default()).unwrap();
    transform.gray(&[[0.5]], &mut values).unwrap();
    assert!(values[0][0] > 0.49 && values[0][0] < 0.51);
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
