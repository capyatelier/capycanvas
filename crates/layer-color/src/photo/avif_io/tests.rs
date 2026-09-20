use super::*;
use std::io::Cursor;

const SDR: &[u8] = include_bytes!("../../../tests/fixtures/avif/p3-12bit.avif");
const HDR: &[u8] = include_bytes!("../../../tests/fixtures/avif/hdr-rgb.avif");

#[test]
fn rust_avif_dispatch_and_failure_recovery_without_native_features() {
    assert!(formats().any(|f| f.name == "AVIF"));
    let cancel = AtomicBool::new(true);
    let limits = DecodeLimits {
        codec_bytes: 8 * 1024 * 1024,
        source_bytes: 8 * 1024 * 1024,
        dimension: 128,
    };
    let message = read_photo_detailed_with_cancel(Cursor::new(SDR), limits, &cancel)
        .err()
        .unwrap();
    assert!(message.contains("cancelled"));
    cancel.store(false, std::sync::atomic::Ordering::Release);
    for limits in [
        DecodeLimits {
            codec_bytes: SDR.len() - 1,
            ..limits
        },
        DecodeLimits {
            codec_bytes: 128 * 1024,
            ..limits
        },
        DecodeLimits {
            source_bytes: 1,
            ..limits
        },
        DecodeLimits {
            dimension: 16,
            ..limits
        },
    ] {
        assert!(read_photo_detailed_with_cancel(Cursor::new(SDR), limits, &cancel).is_err());
    }
    let photo = read_photo_detailed_with_cancel(Cursor::new(SDR), limits, &cancel).unwrap();
    assert_eq!(photo.source.extent, [64, 32]);
    let hdr = read_photo_detailed_with_cancel(Cursor::new(HDR), limits, &cancel).unwrap();
    assert_eq!(hdr.source.interpretation.depth, SampleDepth::F16);
}

#[test]
fn rust_avif_rejects_truncation_record_counts_and_unadmitted_av1() {
    let cancel = AtomicBool::new(false);
    for bytes in [SDR, HDR] {
        for length in (0..bytes.len()).step_by(31) {
            assert!(
                read(
                    Cursor::new(&bytes[..length]),
                    DecodeLimits::default(),
                    &cancel
                )
                .is_err(),
                "truncated at {length}"
            );
        }
    }
    for (kind, offset, count) in [(b"iinf", 8, 2), (b"iloc", 10, 2), (b"ipma", 8, 4)] {
        let mut bytes = SDR.to_vec();
        let at = bytes.windows(4).position(|w| w == kind).unwrap() + offset;
        bytes[at..at + count].fill(0xff);
        assert!(
            read(Cursor::new(bytes), DecodeLimits::default(), &cancel).is_err(),
            "inflated {kind:?} count"
        );
    }
    let parsed = Container::parse(SDR, 1024 * 1024, &cancel).unwrap();
    let property = parsed
        .properties
        .iter()
        .find(|p| &p.kind == b"ispe")
        .unwrap();
    let at = property.data.as_ptr() as usize - SDR.as_ptr() as usize + 4;
    for width in [0, 1, u32::MAX] {
        let mut bytes = SDR.to_vec();
        bytes[at..at + 4].copy_from_slice(&width.to_be_bytes());
        assert!(
            read(Cursor::new(bytes), DecodeLimits::default(), &cancel).is_err(),
            "unadmitted dimension {width}"
        );
    }
    let payload = parsed.payload(parsed.primary.unwrap(), SDR.len()).unwrap();
    let at = payload.as_ptr() as usize - SDR.as_ptr() as usize;
    let mut bytes = SDR.to_vec();
    bytes[at] = 0xff;
    assert!(read(Cursor::new(bytes), DecodeLimits::default(), &cancel).is_err());
}

#[test]
fn rust_avif_validates_gain_metadata_and_honors_sdr_alternatives() {
    let cancel = AtomicBool::new(false);
    let parsed = Container::parse(HDR, 1024 * 1024, &cancel).unwrap();
    let tmap = parsed.items.iter().find(|v| &v.kind == b"tmap").unwrap().id;
    let payload = parsed.payload(tmap, HDR.len()).unwrap();
    let at = payload.as_ptr() as usize - HDR.as_ptr() as usize;
    for range in [18..22, 38..42] {
        // alternate headroom denominator, first gamma numerator
        let mut bytes = HDR.to_vec();
        bytes[at + range.start..at + range.end].fill(0);
        let error = read(Cursor::new(bytes), DecodeLimits::default(), &cancel)
            .err()
            .unwrap();
        assert!(error.contains("gain-map"), "{error}");
    }
    for version_offset in [0, 2] {
        // unsupported item version / minimum metadata version
        let mut bytes = HDR.to_vec();
        bytes[at + version_offset] = 1;
        let photo = read(Cursor::new(bytes), DecodeLimits::default(), &cancel).unwrap();
        assert_eq!(photo.source.interpretation.depth, SampleDepth::U16);
    }
    let group = container::boxes(parsed.groups.unwrap())
        .find_map(|b| {
            let b = b.unwrap();
            (&b.kind == b"altr").then_some(b)
        })
        .unwrap();
    let mut r = Reader::new(group.data);
    r.take(8).unwrap();
    assert_eq!(r.u32().unwrap(), 2);
    let at = r.data.as_ptr() as usize - HDR.as_ptr() as usize;
    let mut bytes = HDR.to_vec();
    for i in 0..4 {
        bytes.swap(at + i, at + 4 + i);
    }
    let photo = read(Cursor::new(bytes), DecodeLimits::default(), &cancel).unwrap();
    assert_eq!(photo.source.interpretation.depth, SampleDepth::U16);
}

#[test]
fn rust_avif_synthetic_precision_and_geometry() {
    for (encoded, expected, extent, density) in [
        (
            SDR,
            include_bytes!("../../../tests/fixtures/avif/p3-12bit.rgba16").as_slice(),
            [64, 32],
            None,
        ),
        (
            include_bytes!("../../../tests/fixtures/avif/p3-10bit-crop-r1-m1.avif").as_slice(),
            include_bytes!("../../../tests/fixtures/avif/p3-10bit-crop-r1-m1.rgba16").as_slice(),
            [24, 48],
            Some([[150, 1], [300, 1]]),
        ),
    ] {
        let photo = read(
            Cursor::new(encoded),
            DecodeLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(photo.source.extent, extent);
        assert_eq!(photo.source.resolution.map(|v| v.density), density);
        assert_eq!(photo.source.interpretation.depth, SampleDepth::U16);
        assert_eq!(
            photo.source.interpretation.profile,
            ColorProfile::Builtin(RgbSpace::DisplayP3)
        );
        assert!(!photo.source.interpretation.profile_assumed);
        let mut rows = photo.source.rows();
        let mut row = vec![0; photo.source.row_bytes()];
        for (y, expected) in expected.chunks_exact(row.len()).enumerate() {
            rows.read(y as u32, &mut row).unwrap();
            assert_eq!(row, expected, "row {y}");
        }
    }
}

#[test]
fn rust_avif_synthetic_hdr_matches_libavif() {
    for (name, encoded, expected) in [
        (
            "RGB",
            HDR,
            include_bytes!("../../../tests/fixtures/avif/hdr-rgb.rgba16f").as_slice(),
        ),
        (
            "reduced gray",
            include_bytes!("../../../tests/fixtures/avif/hdr-small-gray.avif").as_slice(),
            include_bytes!("../../../tests/fixtures/avif/hdr-small-gray.rgba16f").as_slice(),
        ),
        (
            "alternate space",
            include_bytes!("../../../tests/fixtures/avif/hdr-alternate.avif").as_slice(),
            include_bytes!("../../../tests/fixtures/avif/hdr-alternate.rgba16f").as_slice(),
        ),
    ] {
        let photo = read(
            Cursor::new(encoded),
            DecodeLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(photo.source.extent, [16, 12]);
        assert_eq!(photo.source.interpretation.depth, SampleDepth::F16);
        assert_eq!(
            photo.source.interpretation.profile,
            ColorProfile::Builtin(RgbSpace::Srgb)
        );
        let mut rows = photo.source.rows();
        let mut row = vec![0; photo.source.row_bytes()];
        let mut largest = 0f32;
        for (y, expected) in expected.chunks_exact(row.len()).enumerate() {
            rows.read(y as u32, &mut row).unwrap();
            for (x, (actual, expected)) in row
                .chunks_exact(8)
                .zip(expected.chunks_exact(8))
                .enumerate()
            {
                let pixel = |b: &[u8]| {
                    layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| {
                        u16::from_le_bytes(b[c * 2..c * 2 + 2].try_into().unwrap())
                    }))
                    .unwrap()
                };
                let actual = pixel(actual);
                let expected = pixel(expected);
                for c in 0..3 {
                    // libavif's delivery utility clips negative RGB. The
                    // editing source intentionally retains out-of-gamut values.
                    let error = (actual[c].max(0.) - expected[c]).abs();
                    largest = largest.max(error);
                    assert!(
                        error < 0.004,
                        "{name} ({x},{y}) channel={c}: {actual:?} != {expected:?}"
                    );
                }
                assert!((actual[3] - expected[3]).abs() < 0.001);
            }
        }
        eprintln!("{name}: native gain-map reference max error {largest}");
    }
}

#[test]
#[ignore = "requires tools/validation/avif_reference.py fixtures"]
fn rust_avif_lossless_stills_preserve_alpha_color_geometry_and_density() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
    );
    for depth in [10, 12] {
        let names = [String::new(), "-rotated".into(), "-bitstream".into()]
            .into_iter()
            .chain((0..4).flat_map(|r| (0..2).map(move |m| format!("-crop-r{r}-m{m}"))));
        for suffix in names {
            let name = format!("p3-{depth}bit{suffix}");
            let bytes = std::fs::read(root.join(format!("{name}.avif"))).unwrap();
            let photo = read(
                Cursor::new(&bytes),
                DecodeLimits::default(),
                &AtomicBool::new(false),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(photo.source.interpretation.depth, SampleDepth::U16);
            assert_eq!(
                photo.source.interpretation.profile,
                ColorProfile::Builtin(RgbSpace::DisplayP3)
            );
            assert!(!photo.source.interpretation.profile_assumed);
            let rotation = if suffix == "-rotated" {
                Some(1)
            } else {
                suffix
                    .strip_prefix("-crop-r")
                    .map(|v| v.as_bytes()[0] - b'0')
            };
            assert_eq!(
                photo.source.resolution.map(|v| v.density),
                rotation.map(|r| {
                    if r % 2 == 0 {
                        [[300, 1], [150, 1]]
                    } else {
                        [[150, 1], [300, 1]]
                    }
                })
            );
            let expected = std::fs::read(root.join(format!("{name}.rgba16"))).unwrap();
            let mut rows = photo.source.rows();
            let mut row = vec![0; photo.source.row_bytes()];
            assert_eq!(
                expected.len(),
                row.len() * photo.source.extent[1] as usize,
                "{name}"
            );
            for (y, expected) in expected.chunks_exact(row.len()).enumerate() {
                rows.read(y as u32, &mut row).unwrap();
                if row != expected {
                    let i = row.iter().zip(expected).position(|(a, b)| a != b).unwrap();
                    panic!("{name} row={y} byte={i}: {} != {}", row[i], expected[i]);
                }
            }
            println!("{name}: exact source samples retained");
        }
    }
}

#[test]
#[ignore = "requires independent capy-hdr-codec outputs in LAYER_AVIF_HDR_REFERENCE"]
fn rust_avif_gainmap_matches_native_hdr_reference() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_HDR_REFERENCE").expect("HDR references"),
    );
    let photo = read(
        Cursor::new(std::fs::read(root.join("source")).unwrap()),
        DecodeLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(photo.source.interpretation.depth, SampleDepth::F16);
    let base = std::fs::read(root.join("decoded")).unwrap();
    let gain = std::fs::read(root.join("decoded-gain")).unwrap();
    let metadata = std::fs::read(root.join("decoded-metadata")).unwrap();
    let m: Vec<f32> = metadata
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let primaries = u32::from_le_bytes(base[8..12].try_into().unwrap());
    let matrix = match primaries {
        1 => RgbSpace::Srgb.linear_transform(RgbSpace::Srgb),
        9 => layer_core::color::hdr::bt2020_to_srgb(),
        12 => RgbSpace::DisplayP3.linear_transform(RgbSpace::Srgb),
        _ => panic!("unexpected oracle primaries"),
    };
    let mut rows = photo.source.rows();
    let mut row = vec![0; photo.source.row_bytes()];
    let mut largest = 0f32;
    for y in 0..photo.source.extent[1] {
        rows.read(y, &mut row).unwrap();
        for (x, p) in row.chunks_exact(8).enumerate() {
            let i = y as usize * photo.source.extent[0] as usize + x;
            let normalized = |bytes: &[u8], at| {
                f32::from(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())) / 65535.
            };
            let actual = layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| {
                u16::from_le_bytes(p[c * 2..c * 2 + 2].try_into().unwrap())
            }))
            .unwrap();
            let linear = std::array::from_fn(|c| {
                let base =
                    RgbSpace::Srgb.decode(f64::from(normalized(&base, 20 + i * 8 + c * 2))) as f32;
                let gain = normalized(&gain, i * 6 + c * 2).powf(1. / m[6 + c]);
                f64::from((base + m[9 + c]) * (m[c] + gain * (m[3 + c] - m[c])).exp2() - m[12 + c])
            });
            let expected = layer_core::color::rgb::apply(matrix, linear);
            for c in 0..3 {
                let error = (actual[c] - expected[c] as f32).abs();
                largest = largest.max(error);
                assert!(
                    error < 0.02 + 0.005 * expected[c].abs() as f32,
                    "({x},{y}) channel={c}: {actual:?} != {expected:?}"
                );
            }
            assert!((actual[3] - normalized(&base, 20 + i * 8 + 6)).abs() < 0.001);
        }
    }
    println!("AVIF HDR reference maximum absolute error: {largest}");
}

#[test]
#[ignore = "requires tools/validation/avif_reference.py fixtures"]
fn rust_avif_grids_and_sequences_match_native_reference() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
    );
    for name in [
        "color_grid_alpha_nogrid",
        "sofa_grid1x5_420",
        "colors-animated-12bpc-keyframes-0-2-3",
        "sequence-different-poster",
        "colors-animated-8bpc-alpha-exif-xmp",
    ] {
        let bytes = std::fs::read(root.join(format!("{name}.avif"))).unwrap();
        let photo = read(
            Cursor::new(bytes),
            DecodeLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = std::fs::read(root.join(format!("{name}.reference.rgba16"))).unwrap();
        let mut rows = photo.source.rows();
        let mut row = vec![0; photo.source.row_bytes()];
        assert_eq!(
            photo.first_frame,
            name.starts_with("colors-") || name.starts_with("sequence-")
        );
        let wide = photo.source.interpretation.depth == SampleDepth::U16;
        let expected_row = if wide { row.len() } else { row.len() * 2 };
        assert_eq!(
            expected.len(),
            expected_row * photo.source.extent[1] as usize
        );
        for (y, expected) in expected.chunks_exact(expected_row).enumerate() {
            rows.read(y as u32, &mut row).unwrap();
            for (i, expected) in expected.chunks_exact(2).enumerate() {
                let actual = if wide {
                    u16::from_le_bytes(row[i * 2..i * 2 + 2].try_into().unwrap())
                } else {
                    row[i] as u16 * 257
                };
                let expected = u16::from_le_bytes(expected.try_into().unwrap());
                assert_eq!(actual, expected, "{name} row={y} byte={i}");
            }
        }
        println!(
            "{name}: exact source samples and first-frame disclosure passed; density {:?}",
            photo.source.resolution
        );
    }
}
