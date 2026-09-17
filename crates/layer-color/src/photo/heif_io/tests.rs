use super::*;
use std::{
    io::{BufReader, Cursor},
    path::Path,
};

fn assert_reference_samples(photo: &DecodedPhoto, expected: &[u8]) {
    let source = &photo.source;
    assert_eq!(
        expected.len(),
        source.extent[0] as usize * source.extent[1] as usize * 8
    );
    assert_eq!(source.interpretation.channels, SourceChannels::Rgba);
    let mut rows = source.rows();
    let mut row = vec![0; source.row_bytes()];
    for (y, expected) in expected
        .chunks_exact(source.extent[0] as usize * 8)
        .enumerate()
    {
        rows.read(y as u32, &mut row).unwrap();
        for (c, pair) in expected.chunks_exact(2).enumerate() {
            let actual = if source.interpretation.depth == IntegerDepth::U8 {
                row[c] as u16 * 257
            } else {
                u16::from_le_bytes(row[c * 2..c * 2 + 2].try_into().unwrap())
            };
            assert_eq!(
                actual,
                u16::from_le_bytes(pair.try_into().unwrap()),
                "row {y}, channel {c}"
            );
        }
    }
}

#[test]
#[ignore = "requires tools/validation/avif_reference.py fixtures and the native bundle"]
fn heif_avif_sequences_and_grids_match_independent_reference() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
    );
    for (name, extent, depth, sequence, tagged) in [
        (
            "colors-animated-12bpc-keyframes-0-2-3",
            [64, 64],
            IntegerDepth::U16,
            true,
            false,
        ),
        (
            "sequence-different-poster",
            [64, 64],
            IntegerDepth::U16,
            true,
            false,
        ),
        (
            "colors-animated-8bpc-alpha-exif-xmp",
            [150, 150],
            IntegerDepth::U8,
            true,
            true,
        ),
        (
            "color_grid_alpha_nogrid",
            [80, 80],
            IntegerDepth::U8,
            false,
            true,
        ),
        (
            "sofa_grid1x5_420",
            [1024, 770],
            IntegerDepth::U8,
            false,
            true,
        ),
    ] {
        let bytes = std::fs::read(root.join(format!("{name}.avif"))).unwrap();
        let photo = read_photo_detailed(Cursor::new(bytes), Default::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(photo.first_frame, sequence, "{name}");
        assert!(
            !photo.primary_image,
            "{name}: sequence must not be described as a primary still"
        );
        assert_eq!(photo.source.extent, extent);
        assert_eq!(photo.source.interpretation.depth, depth);
        assert_eq!(photo.source.interpretation.profile_assumed, !tagged);
        assert_eq!(
            photo.display_name("Photo"),
            if sequence {
                "Photo (first frame)"
            } else {
                "Photo"
            }
        );
        assert_reference_samples(
            &photo,
            &std::fs::read(root.join(format!("{name}.reference.rgba16"))).unwrap(),
        );
        println!(
            "{name}: exact first-frame/grid samples, metadata and disclosure passed; density={:?}",
            photo.source.resolution
        );
    }
    assert_ne!(
        std::fs::read(root.join("sequence-different-poster.reference.rgba16")).unwrap(),
        std::fs::read(root.join("sequence-different-poster.poster.rgba16")).unwrap()
    );
}

#[test]
#[ignore = "requires tools/validation/avif_reference.py fixtures and the native bundle"]
fn heif_avif_crop_rotation_mirror_preserve_samples_and_density() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
    );
    for depth in [10, 12] {
        for rotation in 0..4 {
            for mirror in 0..2 {
                let name = format!("p3-{depth}bit-crop-r{rotation}-m{mirror}");
                let photo = read_photo_detailed(
                    Cursor::new(std::fs::read(root.join(format!("{name}.avif"))).unwrap()),
                    Default::default(),
                )
                .unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(
                    photo.source.extent,
                    if rotation % 2 == 0 {
                        [48, 24]
                    } else {
                        [24, 48]
                    }
                );
                assert_eq!(
                    photo.source.interpretation.profile,
                    ColorProfile::Builtin(RgbSpace::DisplayP3)
                );
                assert_eq!(photo.source.interpretation.depth, IntegerDepth::U16);
                assert!(!photo.source.interpretation.profile_assumed);
                assert_eq!(
                    photo.source.resolution.unwrap().density,
                    if rotation % 2 == 0 {
                        [[300, 1], [150, 1]]
                    } else {
                        [[150, 1], [300, 1]]
                    }
                );
                assert_reference_samples(
                    &photo,
                    &std::fs::read(root.join(format!("{name}.rgba16"))).unwrap(),
                );
                println!("{name}: exact cropped/oriented source and density passed");
            }
        }
    }
}

#[test]
#[ignore = "requires tools/validation/avif_reference.py fixtures and the native bundle"]
fn heif_avif_unsupported_track_geometry_and_invalid_properties_are_explicit() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
    );
    let mut bytes = std::fs::read(root.join("colors-animated-12bpc-keyframes-0-2-3.avif")).unwrap();
    let header = bytes.windows(4).position(|b| b == b"tkhd").unwrap() + 4;
    let matrix = header + if bytes[header] == 1 { 52 } else { 40 };
    bytes[matrix..matrix + 4].copy_from_slice(&(-65536_i32).to_be_bytes());
    let message = read_photo_detailed(Cursor::new(bytes), Default::default())
        .err()
        .unwrap();
    assert!(message.contains("track transformations"), "{message}");
    let bytes = std::fs::read(root.join("clap_irot_imir_non_essential.avif")).unwrap();
    assert!(read_photo_detailed(Cursor::new(bytes), Default::default()).is_err());
}

#[test]
#[ignore = "requires pinned HEIF/AVIF source fixtures and the native bundle"]
fn heif_native_cancellation_after_parse_discards_decode_and_allows_retry() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_HEIF_REFERENCES").expect("HEIF references"),
    );
    for name in ["examples/example.heic", "examples/example.avif"] {
        let bytes = std::fs::read(root.join(name)).unwrap();
        let cancelled = AtomicBool::new(false);
        let mut photo =
            bridge::Photo::open(bytes.clone(), 128 * 1024 * 1024, 32768, &cancelled).unwrap();
        cancelled.store(true, Ordering::Release);
        let message = photo.decode(128 * 1024 * 1024, &cancelled).err().unwrap();
        assert!(message.contains("cancelled"), "{name}: {message}");
        drop(cancelled);
        // Metadata remains valid after the token's lifetime: the C context must
        // not retain a cancellation callback on an error return.
        photo.metadata(false).unwrap();
        drop(photo);
        read_photo_detailed(Cursor::new(bytes), Default::default()).unwrap();
    }
}

#[test]
fn heif_sdr_profiles_preserve_transfer_and_reject_hdr() {
    let mut info = bridge::Info {
        bits: 10,
        nclx: 1,
        primaries: 12,
        transfer: 13,
        ..Default::default()
    };
    let (profile, assumed) = source_profile(&info, Vec::new()).unwrap();
    assert_eq!(profile, ColorProfile::Builtin(RgbSpace::DisplayP3));
    assert!(!assumed);
    for transfer in [16, 18] {
        info.transfer = transfer;
        assert!(
            source_profile(&info, Vec::new())
                .unwrap_err()
                .contains("HDR")
        );
    }
    info.transfer = 2;
    assert!(source_profile(&info, Vec::new()).unwrap().1);
}

#[test]
fn heif_cancellation_precedes_file_parsing() {
    let result = read_photo_detailed_with_cancel(
        Cursor::new(b"invalid"),
        Default::default(),
        &AtomicBool::new(true),
    );
    assert!(result.err().unwrap().contains("cancelled"));
}

#[test]
fn heif_missing_bundle_hides_formats_and_reports_error() {
    const CHILD: &str = "CAPY_CODEC_UNAVAILABLE_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        assert!(!available());
        assert!(!extensions().any(|name| matches!(name, "heif" | "heic" | "avif")));
        assert!(
            !mime_types().any(|mime| matches!(mime, "image/heif" | "image/heic" | "image/avif"))
        );
        let message = read_photo_detailed(
            Cursor::new(b"\0\0\0\x10ftypavif\0\0\0\0"),
            Default::default(),
        )
        .err()
        .unwrap();
        assert!(message.contains("not installed"), "{message}");
        return;
    }
    // The library capability cache is process-wide. Exercise a real fresh
    // process rather than mutating environment under other decoding threads.
    let directory = std::env::temp_dir().join(format!(
        "capy-no-photo-codecs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "photo::heif_io::tests::heif_missing_bundle_hides_formats_and_reports_error",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("CAPY_PHOTO_CODEC_DIR", &directory)
        .output()
        .unwrap();
    std::fs::remove_dir(&directory).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
}

#[test]
#[ignore = "requires the pinned codec bundle and its upstream sample files"]
fn heif_bundled_reference_files_decode() {
    assert!(available(), "build tools/build/photo-codecs.py first");
    let root = std::env::var_os("LAYER_HEIF_REFERENCES")
        .expect("LAYER_HEIF_REFERENCES is the libheif source directory");
    let root = Path::new(&root);
    let limits = DecodeLimits {
        source_bytes: 128 * 1024 * 1024,
        codec_bytes: 256 * 1024 * 1024,
        dimension: 32768,
    };
    for name in [
        "examples/example.heic",
        "examples/example.avif",
        "tests/data/with-alpha-512x512.heic",
    ] {
        let photo = read_photo_detailed(
            BufReader::new(std::fs::File::open(root.join(name)).unwrap()),
            limits,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        photo.source.validate().unwrap();
        assert!(photo.source.extent.iter().all(|n| *n > 0));
        assert_eq!(photo.source.interpretation.depth, IntegerDepth::U8);
        let mut rows = photo.source.rows();
        let mut row = vec![0; photo.source.row_bytes()];
        let mut colored = false;
        let mut transparent = false;
        for y in 0..photo.source.extent[1] {
            rows.read(y, &mut row).unwrap();
            for pixel in row.chunks_exact(4) {
                colored |= pixel[0] != pixel[1] || pixel[1] != pixel[2];
                transparent |= pixel[3] < 255;
            }
        }
        assert!(colored);
        if name.contains("with-alpha") {
            assert!(transparent);
        }
        println!(
            "{name}: {:?}, {:?}, alpha={transparent}, primary={}",
            photo.source.extent, photo.source.interpretation.depth, photo.primary_image
        );
    }
}

#[test]
#[ignore = "requires the pinned codec bundle and its upstream sample files"]
fn heif_native_admission_and_truncated_input() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_HEIF_REFERENCES").expect("HEIF references"),
    );
    for name in ["examples/example.heic", "examples/example.avif"] {
        let encoded = std::fs::read(root.join(name)).unwrap();
        for (kind, limits) in [
            (
                "encoded budget",
                DecodeLimits {
                    codec_bytes: encoded.len() - 1,
                    ..Default::default()
                },
            ),
            (
                "decoded budget",
                DecodeLimits {
                    codec_bytes: encoded.len() + 4096,
                    ..Default::default()
                },
            ),
            (
                "source budget",
                DecodeLimits {
                    source_bytes: 1,
                    ..Default::default()
                },
            ),
            (
                "dimension",
                DecodeLimits {
                    dimension: 64,
                    ..Default::default()
                },
            ),
        ] {
            let message = read_photo_detailed(Cursor::new(&encoded), limits)
                .err()
                .unwrap_or_else(|| panic!("{name}: ignored {kind}"));
            println!("{name}: {kind} rejected: {message}");
        }
        // The HEIC collection stores its primary before other images; cutting
        // the file in half can leave that primary complete and is not a decode
        // failure. Cut inside the beginning of mdat instead.
        let media = encoded.windows(4).position(|w| w == b"mdat").unwrap();
        for length in [16, media + 4 + 16] {
            assert!(
                read_photo_detailed(Cursor::new(&encoded[..length]), Default::default()).is_err(),
                "{name}: accepted truncated input"
            );
        }
        // A rejected member must not poison later jobs in the same process.
        read_photo_detailed(Cursor::new(&encoded), Default::default()).unwrap();
    }
}

#[test]
#[ignore = "requires independent libavif/AOM fixtures from tools/validation/avif_precision.c"]
fn heif_avif_precision_alpha_and_orientation_match_independent_encoder() {
    let root =
        std::path::PathBuf::from(std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF fixtures"));
    for depth in [10, 12] {
        for rotated in [false, true] {
            let name = format!("p3-{depth}bit{}", if rotated { "-rotated" } else { "" });
            let photo = read_photo_detailed(
                BufReader::new(std::fs::File::open(root.join(format!("{name}.avif"))).unwrap()),
                Default::default(),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                photo.source.extent,
                if rotated { [32, 64] } else { [64, 32] }
            );
            assert_eq!(photo.source.interpretation.depth, IntegerDepth::U16);
            assert_eq!(
                photo.source.interpretation.profile,
                ColorProfile::Builtin(RgbSpace::DisplayP3)
            );
            assert!(!photo.source.interpretation.profile_assumed);
            if rotated {
                assert_eq!(
                    photo.source.resolution.unwrap().density,
                    [[150, 1], [300, 1]]
                );
            }
            let expected = std::fs::read(root.join(format!("{name}.rgba16"))).unwrap();
            let mut rows = photo.source.rows();
            let mut actual = vec![0; photo.source.row_bytes()];
            for (y, row) in expected.chunks_exact(actual.len()).enumerate() {
                rows.read(y as u32, &mut actual).unwrap();
                if actual != row {
                    let at = actual.iter().zip(row).position(|(a, b)| a != b).unwrap();
                    panic!(
                        "{name}: differing byte at row {y}, offset {at}: {} != {}",
                        actual[at], row[at]
                    );
                }
            }
            println!(
                "{name}: exact P3/U16 samples and {} passed",
                if rotated {
                    "container rotation"
                } else {
                    "alpha and hidden RGB"
                }
            );
            if !rotated {
                let mut encoded = std::fs::read(root.join(format!("{name}.avif"))).unwrap();
                let colr = encoded.windows(8).position(|b| b == b"colrnclx").unwrap();
                encoded[colr..colr + 4].copy_from_slice(b"free");
                // These libavif/AOM fixtures carry color only in the container.
                // Independent sequence-header inspection confirms 2/2/2
                // (unspecified) in the AV1 payload. Removing colr must not
                // invent a P3 tag or silently mark an assumption as embedded.
                let untagged = read_photo_detailed(Cursor::new(encoded), Default::default())
                    .unwrap_or_else(|e| panic!("{name} untagged color: {e}"));
                assert_eq!(
                    untagged.source.interpretation.profile,
                    ColorProfile::default()
                );
                assert!(untagged.source.interpretation.profile_assumed);
                assert_eq!(untagged.source.extent, photo.source.extent);
            }
        }
    }
}

#[test]
#[ignore = "requires the pinned external libavif samples"]
fn heif_avif_real_metadata_and_sdr_policy() {
    let root =
        std::path::PathBuf::from(std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF fixtures"));
    for name in [
        "colors_sdr_srgb.avif",
        "colors_text_wcg_sdr_rec2020.avif",
        "paris_icc_exif_xmp.avif",
        "seine_sdr_gainmap_srgb.avif",
    ] {
        let encoded = std::fs::read(root.join(name)).unwrap();
        let photo = read_photo_detailed(Cursor::new(&encoded), Default::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!photo.source.interpretation.profile_assumed, "{name}");
        photo.source.validate().unwrap();
        if name == "paris_icc_exif_xmp.avif" {
            // The fixed fixture contains one embedded ICC. Compare its original
            // header-declared bytes independently of the HEIF metadata API.
            let at = encoded.windows(4).position(|w| w == b"acsp").unwrap() - 36;
            let length = u32::from_be_bytes(encoded[at..at + 4].try_into().unwrap()) as usize;
            let ColorProfile::Icc(icc) = &photo.source.interpretation.profile else {
                panic!("ICC not retained");
            };
            assert_eq!(icc.as_ref(), &encoded[at..at + length]);
        }
        println!(
            "{name}: {:?}, {:?}, retained profile/density {:?}",
            photo.source.extent, photo.source.interpretation.depth, photo.source.resolution
        );
    }
    for name in [
        "seine_hdr_rec2020.avif",
        "cosmos1650_yuv444_10bpc_p3pq.avif",
    ] {
        let message = read_photo_detailed(
            Cursor::new(std::fs::read(root.join(name)).unwrap()),
            Default::default(),
        )
        .err()
        .unwrap();
        assert!(message.contains("HDR"), "{name}: {message}");
    }
}

#[test]
#[ignore = "requires independent libavif/AOM references from tools/validation/avif_reference.c"]
fn heif_avif_bitstream_color_rotation_alpha_and_gainmap_match_reference() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
    );
    for (name, extent, depth, profile, tolerance) in [
        (
            "p3-10bit-bitstream-no-colr",
            [64, 32],
            IntegerDepth::U16,
            RgbSpace::DisplayP3,
            0,
        ),
        (
            "p3-12bit-bitstream-no-colr",
            [64, 32],
            IntegerDepth::U16,
            RgbSpace::DisplayP3,
            0,
        ),
        (
            "abc_color_irot_alpha_irot",
            [256, 512],
            IntegerDepth::U8,
            RgbSpace::Srgb,
            257,
        ),
        (
            "seine_sdr_gainmap_srgb",
            [400, 300],
            IntegerDepth::U8,
            RgbSpace::Srgb,
            257,
        ),
    ] {
        let encoded = std::fs::read(root.join(format!("{name}.avif"))).unwrap();
        if name.ends_with("-no-colr") {
            assert!(!encoded.windows(8).any(|w| w == b"colrnclx"));
        }
        let photo = read_photo_detailed(Cursor::new(encoded), Default::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(photo.source.extent, extent);
        assert_eq!(photo.source.interpretation.depth, depth);
        assert_eq!(
            photo.source.interpretation.profile,
            ColorProfile::Builtin(profile),
            "{name}"
        );
        assert!(
            !photo.source.interpretation.profile_assumed,
            "{name}: lost source color"
        );
        let expected = std::fs::read(root.join(format!("{name}.reference.rgba16"))).unwrap();
        assert_eq!(expected.len(), extent[0] as usize * extent[1] as usize * 8);
        let mut rows = photo.source.rows();
        let mut row = vec![0; photo.source.row_bytes()];
        let mut max_rgb_error = 0;
        let mut partial_alpha = 0;
        for (y, expected) in expected.chunks_exact(extent[0] as usize * 8).enumerate() {
            rows.read(y as u32, &mut row).unwrap();
            for (channel, expected) in expected.chunks_exact(2).enumerate() {
                let expected = u16::from_le_bytes(expected.try_into().unwrap());
                let actual = if depth == IntegerDepth::U8 {
                    row[channel] as u16 * 257
                } else {
                    u16::from_le_bytes(row[channel * 2..channel * 2 + 2].try_into().unwrap())
                };
                if channel % 4 == 3 {
                    assert_eq!(actual, expected, "{name}: alpha at {channel}/{y}");
                    partial_alpha += usize::from(actual != 0 && actual != 65535);
                } else {
                    max_rgb_error = max_rgb_error.max(actual.abs_diff(expected));
                }
            }
        }
        println!(
            "{name}: max RGB difference {max_rgb_error}/65535, exact alpha ({partial_alpha} partial)"
        );
        // Lossless identity-matrix sources must match exactly. The separate
        // bilinear YUV converters can round by one 8-bit code on lossy files.
        assert!(
            max_rgb_error <= tolerance,
            "{name}: RGB differs by {max_rgb_error}/65535"
        );
        if name != "seine_sdr_gainmap_srgb" {
            assert!(partial_alpha > 0);
        }
    }
    // This independent AOM decode reports P3/PQ from the AV1 payload even with
    // colr absent. The application must still detect and reject that HDR input.
    let encoded = std::fs::read(root.join("cosmos1650_yuv444_10bpc_p3pq-no-colr.avif")).unwrap();
    let message = read_photo_detailed(Cursor::new(encoded), Default::default())
        .err()
        .unwrap();
    assert!(message.contains("HDR"), "bitstream-only PQ: {message}");
}
