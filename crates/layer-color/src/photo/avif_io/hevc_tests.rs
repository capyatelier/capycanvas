use super::*;
use std::io::Cursor;

const RED: &[u8] = include_bytes!("../../../tests/fixtures/heif/flat-red-8bit.heic");

// Upstream's test-only ISO box writer is independent of our BMFF parser.
// Codestreams were encoded losslessly by x265, not by the decoder under test.
#[allow(dead_code)]
#[path = "../../../../../vendor/heif-oxide/src/test_builder.rs"]
mod tb;

fn item(id: u32, name: &str) -> tb::TestItem {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/heif-oxide/testdata");
    let (config, payload) =
        tb::annex_b_to_item(&std::fs::read(root.join(format!("{name}.h265"))).unwrap());
    tb::TestItem {
        id,
        item_type: *b"hvc1",
        payload,
        props: vec![(true, config), (false, tb::ispe(64, 64))],
    }
}
fn grid(rotation: u8, mirror: Option<u8>) -> Vec<u8> {
    let mut props = vec![
        (false, tb::ispe(101, 51)),
        (false, tb::colr_nclx(12, 13, 6, false)),
    ];
    if rotation != 0 {
        props.push((true, tb::irot(rotation)));
    }
    if let Some(axis) = mirror {
        props.push((true, tb::imir(axis)));
    }
    let grid = tb::TestItem {
        id: 1,
        item_type: *b"grid",
        payload: tb::grid_payload(1, 2, 101, 51),
        props,
    };
    tb::make_heic(
        &[grid, item(2, "flat_red_64"), item(3, "flat_blue_64")],
        1,
        &[(*b"dimg", 1, vec![2, 3])],
    )
}
fn read(bytes: &[u8]) -> Result<DecodedPhoto, String> {
    read_photo_detailed(Cursor::new(bytes), Default::default())
}
fn rgba8(source: &SourceImage, x: u32, y: u32) -> [u8; 4] {
    let mut row = vec![0; source.row_bytes()];
    source.rows().read(y, &mut row).unwrap();
    row[x as usize * 4..x as usize * 4 + 4].try_into().unwrap()
}

#[test]
fn rust_heif_grid_color_orientation_and_partial_edges() {
    for rotation in 0..4 {
        for mirror in [None, Some(0), Some(1)] {
            let source = read(&grid(rotation, mirror)).unwrap().source;
            let [w, h] = if rotation % 2 == 0 {
                [101, 51]
            } else {
                [51, 101]
            };
            assert_eq!(source.extent, [w, h]);
            assert_eq!(
                source.interpretation.profile,
                ColorProfile::Builtin(RgbSpace::DisplayP3)
            );
            assert!(!source.interpretation.profile_assumed);
            // Clockwise movement of the input's red-left / blue-right split
            // under a counterclockwise image rotation, then output mirroring.
            for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
                let xx = if mirror == Some(1) { w - 1 - x } else { x };
                let yy = if mirror == Some(0) { h - 1 - y } else { y };
                let blue = match rotation {
                    0 => xx == w - 1,
                    1 => yy == 0,
                    2 => xx == 0,
                    3 => yy == h - 1,
                    _ => unreachable!(),
                };
                let p = rgba8(&source, x, y);
                assert!(
                    if blue {
                        p[2] >= 250 && p[0] <= 2
                    } else {
                        p[0] >= 250 && p[2] <= 2
                    },
                    "r{rotation} m{mirror:?} ({x},{y}): {p:?}"
                );
                assert!(p[1] <= 2);
                assert_eq!(p[3], 255);
            }
        }
    }
}

#[test]
fn rust_heif_ten_bit_and_embedded_profile_preserve_source_samples() {
    let mut tile = item(1, "flat_mid_64_10bit");
    tile.props.push((false, tb::colr_nclx(12, 13, 6, false)));
    let encoded = tb::make_heic(&[tile], 1, &[]);
    let source = read(&encoded).unwrap().source;
    assert_eq!(source.interpretation.depth, SampleDepth::U16);
    assert_eq!(
        source.interpretation.profile,
        ColorProfile::Builtin(RgbSpace::DisplayP3)
    );
    let expected = ((448f64 / 876. * 1023.).round() as u32 * 65535 + 511) / 1023;
    let mut row = vec![0; source.row_bytes()];
    let mut rows = source.rows();
    for y in 0..64 {
        rows.read(y, &mut row).unwrap();
        for p in row.chunks_exact(8) {
            for c in 0..3 {
                assert_eq!(
                    u16::from_le_bytes([p[c * 2], p[c * 2 + 1]]) as u32,
                    expected
                );
            }
            assert_eq!(&p[6..], &[255, 255]);
        }
    }
    let mut tile = item(1, "flat_red_64");
    tile.props.push((false, tb::colr_nclx(1, 13, 6, false)));
    let icc = crate::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    let mut colr = b"prof".to_vec();
    colr.extend_from_slice(&icc);
    tile.props.push((false, tb::plain_box(b"colr", &colr)));
    let source = read(&tb::make_heic(&[tile], 1, &[])).unwrap().source;
    assert_eq!(source.interpretation.profile, ColorProfile::Icc(icc.into()));
    assert!(!source.interpretation.profile_assumed);
    assert!(rgba8(&source, 0, 0)[0] >= 250);
}

#[test]
fn rust_heif_rejects_hdr_sequences_and_invalid_coded_extent() {
    for transfer in [16, 18] {
        let mut tile = item(1, "flat_red_64");
        tile.props
            .push((false, tb::colr_nclx(12, transfer, 6, false)));
        assert!(
            read(&tb::make_heic(&[tile], 1, &[]))
                .err()
                .unwrap()
                .contains("HDR")
        );
    }
    let mut movie = RED.to_vec();
    movie.extend_from_slice(&tb::plain_box(b"moov", &[]));
    assert!(read(&movie).err().unwrap().contains("sequences"));
    let mut tile = item(1, "flat_red_64");
    tile.props[1].1 = tb::ispe(8, 8);
    assert!(
        read(&tb::make_heic(&[tile], 1, &[]))
            .err()
            .unwrap()
            .contains("dimensions disagree")
    );
    let mut tile = item(1, "flat_red_64");
    tile.payload.truncate(6);
    assert!(read(&tb::make_heic(&[tile], 1, &[])).is_err());
    let mut tile = item(1, "flat_red_64");
    tile.payload = vec![0, 0, 0, 6, 0x28, 1, 0, 0, 1, 0x80];
    assert!(
        read(&tb::make_heic(&[tile], 1, &[]))
            .err()
            .unwrap()
            .contains("Unescaped")
    );
}

#[test]
fn rust_heif_retains_print_density_and_applies_container_rotation_once() {
    use layer_core::{ImageResolution, ResolutionUnit};
    let density = ImageResolution {
        unit: ResolutionUnit::Inch,
        density: [[300, 1], [150, 1]],
    };
    let mut tile = item(1, "flat_red_64");
    tile.props.push((true, tb::irot(1)));
    let exif = super::super::metadata::exif_output(density).unwrap();
    let mut payload = vec![0; 4];
    payload.extend_from_slice(&exif[6..]);
    let metadata = tb::TestItem {
        id: 2,
        item_type: *b"Exif",
        payload,
        props: vec![],
    };
    let source = read(&tb::make_heic(
        &[tile, metadata],
        1,
        &[(*b"cdsc", 2, vec![1])],
    ))
    .unwrap()
    .source;
    assert_eq!(source.resolution, Some(density.swapped()));
}

#[test]
fn rust_heif_retains_supported_auxiliary_alpha() {
    let mut color = item(1, "flat_red_64");
    color.props.push((false, tb::colr_nclx(1, 13, 6, false)));
    let mut alpha = item(2, "flat_gray_64");
    alpha.props.push((
        true,
        tb::full_box(b"auxC", 0, 0, b"urn:mpeg:hevc:2015:auxid:1\0"),
    ));
    let bytes = tb::make_heic(&[color, alpha], 1, &[(*b"auxl", 2, vec![1])]);
    let source = read(&bytes).unwrap().source;
    let p = rgba8(&source, 0, 0);
    assert!(p[0] >= 250 && p[1] <= 2 && p[2] <= 2, "{p:?}");
    // The x265 fixture stores luma 128; alpha uses the coded coverage value,
    // without the limited-range expansion used for color luma.
    assert_eq!(p[3], 128);
}

#[test]
fn rust_heif_cancels_during_coding_blocks_and_retries() {
    use heif_oxide::hevc::{
        DecoderLimits, build_annex_b, decode_first_frame_with_limits, parse_hvcc,
    };
    let tile = item(1, "flat_red_64");
    let config = parse_hvcc(&tile.props[0].1[8..]).unwrap();
    let annex = build_annex_b(&config, &tile.payload).unwrap();
    let limits = DecoderLimits {
        expected_extent: Some([64, 64]),
        memory_bytes: 8 * 1024 * 1024,
        still_only: true,
    };
    let count = std::cell::Cell::new(0);
    decode_first_frame_with_limits(&annex, limits, &|| {
        count.set(count.get() + 1);
        false
    })
    .unwrap();
    let total = count.get();
    assert!(total > 12);
    for stop in 1..total {
        count.set(0);
        let result = decode_first_frame_with_limits(&annex, limits, &|| {
            count.set(count.get() + 1);
            count.get() >= stop
        });
        assert!(
            result.err().unwrap().to_string().contains("cancelled"),
            "check {stop}"
        );
    }
    decode_first_frame_with_limits(&annex, limits, &|| false).unwrap();
}

#[test]
#[ignore = "regenerate portable fixtures in LAYER_HEIF_FIXTURE_OUTPUT"]
fn rust_heif_write_validation_fixtures() {
    let root = std::path::PathBuf::from(std::env::var_os("LAYER_HEIF_FIXTURE_OUTPUT").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let mut red = item(1, "flat_red_64");
    red.props.push((false, tb::colr_nclx(1, 13, 6, false)));
    std::fs::write(
        root.join("flat-red-8bit.heic"),
        tb::make_heic(&[red], 1, &[]),
    )
    .unwrap();
    std::fs::write(root.join("p3-grid-8bit.heic"), grid(1, Some(1))).unwrap();
    let mut tile = item(1, "flat_mid_64_10bit");
    tile.props.push((false, tb::colr_nclx(12, 13, 6, false)));
    std::fs::write(
        root.join("p3-gray-10bit.heic"),
        tb::make_heic(&[tile], 1, &[]),
    )
    .unwrap();
}

#[test]
fn rust_heif_decodes_independent_lossless_red() {
    let cancel = AtomicBool::new(false);
    let source = read_heif(Cursor::new(RED), Default::default(), &cancel)
        .unwrap()
        .source;
    assert_eq!(source.extent, [64, 64]);
    assert_eq!(source.interpretation.depth, SampleDepth::U8);
    let mut row = vec![0; source.row_bytes()];
    let mut rows = source.rows();
    for y in 0..64 {
        rows.read(y, &mut row).unwrap();
        for p in row.chunks_exact(4) {
            assert!(p[0] >= 250 && p[1] <= 2 && p[2] <= 2, "{p:?}");
            assert_eq!(p[3], 255);
        }
    }
}

#[test]
fn rust_heif_rejects_unadmitted_and_cancelled_input() {
    let cancel = AtomicBool::new(false);
    for limits in [
        DecodeLimits {
            codec_bytes: RED.len() - 1,
            ..Default::default()
        },
        DecodeLimits {
            codec_bytes: 1024 * 1024,
            ..Default::default()
        },
        DecodeLimits {
            dimension: 16,
            ..Default::default()
        },
        DecodeLimits {
            source_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(read_heif(Cursor::new(RED), limits, &cancel).is_err());
    }
    cancel.store(true, std::sync::atomic::Ordering::Release);
    assert!(
        read_heif(Cursor::new(RED), Default::default(), &cancel)
            .err()
            .unwrap()
            .contains("cancelled")
    );
    cancel.store(false, std::sync::atomic::Ordering::Release);
    assert!(read_heif(Cursor::new(RED), Default::default(), &cancel).is_ok());
}

#[test]
#[ignore = "external libheif reference fixtures via LAYER_HEIF_REFERENCES"]
fn rust_heif_opens_external_photographic_still() {
    let root = std::path::PathBuf::from(std::env::var_os("LAYER_HEIF_REFERENCES").unwrap());
    let bytes = std::fs::read(root.join("examples/example.heic")).unwrap();
    let photo = read_heif(
        Cursor::new(bytes),
        Default::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    photo.source.validate().unwrap();
    assert!(photo.primary_image);
    eprintln!(
        "HEIC photograph {:?} {:?}",
        photo.source.extent, photo.source.interpretation
    );
    let bytes = std::fs::read(root.join("tests/data/with-alpha-512x512.heic")).unwrap();
    let message = read(&bytes)
        .err()
        .expect("unsupported alpha representation must not be dropped");
    assert!(
        message.contains("chroma_format") || message.contains("channels disagree"),
        "{message}"
    );
}

#[test]
#[ignore = "independent heif_decode_reference.c output via LAYER_HEIF_YUV_REFERENCE"]
fn rust_heif_photograph_matches_libde265_planes() {
    let root = std::path::PathBuf::from(std::env::var_os("LAYER_HEIF_REFERENCES").unwrap());
    let bytes = std::fs::read(root.join("examples/example.heic")).unwrap();
    let expected = std::fs::read(std::env::var_os("LAYER_HEIF_YUV_REFERENCE").unwrap()).unwrap();
    let cancel = AtomicBool::new(false);
    let container = Container::parse_heif(&bytes, 128 * 1024 * 1024, &cancel).unwrap();
    let image = decode_item(
        &container,
        container.primary.unwrap(),
        None,
        false,
        &mut vec![],
        128 * 1024 * 1024,
        &cancel,
    )
    .unwrap();
    let mut at = 0;
    for c in 0..3 {
        let [w, h] = image.plane_extent(c);
        for y in 0..h {
            for x in 0..w {
                let reference = u16::from_le_bytes([expected[at], expected[at + 1]]);
                assert_eq!(image.sample(c, x, y), reference, "plane {c} ({x},{y})");
                at += 2;
            }
        }
    }
    assert_eq!(at, expected.len());
    eprintln!("HEVC photograph: {} exact YUV samples", at / 2);
}
