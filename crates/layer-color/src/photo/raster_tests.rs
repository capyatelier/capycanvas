use super::*;
use std::io::Cursor;

fn limits() -> DecodeLimits {
    DecodeLimits {
        source_bytes: 16 << 20,
        codec_bytes: 64 << 20,
        dimension: 32768,
    }
}
fn pixels(source: &SourceImage) -> Vec<u8> {
    let mut pixels = vec![0; source.row_bytes() * source.extent[1] as usize];
    let mut rows = source.rows();
    for (y, row) in pixels.chunks_exact_mut(source.row_bytes()).enumerate() {
        rows.read(y as u32, row).unwrap();
    }
    pixels
}

#[test]
#[ignore = "Google gallery files and independent libwebp RGBA in LAYER_WEBP_REFERENCES"]
fn external_webp_reference_samples() {
    let directory = std::path::PathBuf::from(std::env::var_os("LAYER_WEBP_REFERENCES").expect("WebP reference directory"));
    for (name, extent, lossless) in [
        ("1_webp_ll.webp", [400, 301], true),
        ("1_webp_a.webp", [400, 301], false),
        ("2_webp_a.webp", [386, 395], false),
    ] {
        let encoded = std::fs::read(directory.join(name)).unwrap();
        let reference = std::fs::read(directory.join(format!("{name}.rgba"))).unwrap();
        let photo = read_photo_detailed(Cursor::new(&encoded), limits()).unwrap();
        assert!(!photo.first_frame);
        assert_eq!(photo.source.extent, extent);
        assert_eq!(photo.source.interpretation.channels, SourceChannels::Rgba);
        assert_eq!(photo.source.interpretation.depth, SampleDepth::U8);
        assert!(photo.source.interpretation.profile_assumed);
        let actual = pixels(&photo.source);
        assert_eq!(actual.len(), reference.len());
        let mut maximum = 0;
        let mut total = 0u64;
        let mut partial = 0;
        for (a, b) in actual.chunks_exact(4).zip(reference.chunks_exact(4)) {
            assert_eq!(a[3], b[3], "{name}: alpha, including compressed ALPH, must be exact");
            partial += usize::from(a[3] > 0 && a[3] < 255);
            for c in 0..3 {
                let difference = a[c].abs_diff(b[c]);
                maximum = maximum.max(difference);
                total += u64::from(difference);
            }
        }
        let mean = total as f64 / (extent[0] * extent[1] * 3) as f64;
        eprintln!("{name}: RGB max={maximum}, mean={mean:.6}, partial-alpha pixels={partial}");
        assert!(partial > 1000, "reference must exercise transparency edges");
        if lossless {
            assert!(actual == reference, "lossless samples, including hidden RGB, match libwebp");
        } else {
            // Independent VP8 RGB conversion may differ by one rounding level.
            assert!(maximum <= 1, "{name}: unexpected VP8 RGB deviation");
        }
    }
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn bmp_v5(profile: &[u8]) -> Vec<u8> {
    let mut file = vec![0; 14 + 124 + 8 + profile.len()];
    file[..2].copy_from_slice(b"BM");
    let length = file.len() as u32;
    put32(&mut file, 2, length);
    put32(&mut file, 10, 138);
    let header = &mut file[14..138];
    put32(header, 0, 124);
    put32(header, 4, 2);
    put32(header, 8, 1);
    header[12..14].copy_from_slice(&1u16.to_le_bytes());
    header[14..16].copy_from_slice(&32u16.to_le_bytes());
    put32(header, 16, 3);
    put32(header, 20, 8);
    put32(header, 24, 3780);
    put32(header, 28, 7560);
    for (at, value) in [(40, 0xff0000), (44, 0xff00), (48, 0xff), (52, 0xff000000)] {
        put32(header, at, value);
    }
    put32(
        header,
        56,
        if profile.is_empty() {
            0x73524742
        } else {
            0x4d424544
        },
    );
    put32(header, 112, 132);
    put32(header, 116, profile.len() as u32);
    file[138..146].copy_from_slice(&[30, 20, 10, 128, 90, 80, 70, 0]);
    file[146..].copy_from_slice(profile);
    file
}

#[test]
fn bmp_v5_and_dib_preserve_profile_alpha_hidden_rgb_and_density() {
    let profile = profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    let file = bmp_v5(&profile);
    for bytes in [&file[..], &file[14..]] {
        let photo = read_photo_detailed(Cursor::new(bytes), limits()).unwrap();
        assert!(!photo.first_frame);
        assert_eq!(photo.source.extent, [2, 1]);
        assert_eq!(
            photo.source.interpretation.profile,
            ColorProfile::Icc(profile.clone().into())
        );
        assert_eq!(photo.source.interpretation.depth, SampleDepth::U8);
        assert_eq!(pixels(&photo.source), [10, 20, 30, 128, 70, 80, 90, 0]);
        assert_eq!(
            photo.source.resolution.unwrap().density,
            [[3780, 1], [7560, 1]]
        );
    }
}

#[test]
fn bmp_palette_bottom_up_and_top_down_have_identical_pixels() {
    let mut file = vec![0; 14 + 40 + 8 + 8];
    file[..2].copy_from_slice(b"BM");
    let length = file.len() as u32;
    put32(&mut file, 2, length);
    put32(&mut file, 10, 62);
    put32(&mut file, 14, 40);
    put32(&mut file, 18, 2);
    put32(&mut file, 22, 2);
    file[26..28].copy_from_slice(&1u16.to_le_bytes());
    file[28..30].copy_from_slice(&8u16.to_le_bytes());
    put32(&mut file, 46, 2);
    file[54..62].copy_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // red, blue
    file[62..].copy_from_slice(&[1, 0, 0, 0, 0, 1, 0, 0]);
    let a = read_photo(Cursor::new(&file), limits()).unwrap();
    assert_eq!(pixels(&a), [255, 0, 0, 0, 0, 255, 0, 0, 255, 255, 0, 0]);
    put32(&mut file, 22, (-2i32) as u32);
    file[62..].copy_from_slice(&[0, 1, 0, 0, 1, 0, 0, 0]);
    let b = read_photo(Cursor::new(&file), limits()).unwrap();
    assert_eq!(pixels(&a), pixels(&b));
    assert!(b.interpretation.profile_assumed);
}

#[test]
fn bmp_calibration_becomes_a_profile_and_bad_declarations_do_not_become_srgb() {
    let mut file = bmp_v5(&[]);
    put32(&mut file, 14 + 56, 0);
    let matrix = RgbSpace::AdobeRgb.to_xyz();
    for c in 0..3 {
        for i in 0..3 {
            put32(
                &mut file,
                14 + 60 + c * 12 + i * 4,
                (matrix[i][c] * (1u64 << 30) as f64).round() as i32 as u32,
            );
        }
        put32(&mut file, 14 + 96 + c * 4, (2.2 * 65536.) as u32);
    }
    let source = read_photo(Cursor::new(&file), limits()).unwrap();
    assert!(!source.interpretation.profile_assumed);
    assert_eq!(
        profile_channels(&source.interpretation.profile).unwrap(),
        ProfileChannels::Rgb
    );
    assert_eq!(
        crate::suggested_working_space(&source.interpretation.profile).unwrap(),
        Some(RgbSpace::AdobeRgb)
    );
    assert_eq!(pixels(&source), [10, 20, 30, 128, 70, 80, 90, 0]);
    put32(&mut file, 14 + 96, 0);
    assert!(
        read_photo(Cursor::new(&file), limits())
            .unwrap_err()
            .contains("gamma")
    );
    put32(&mut file, 14 + 56, 0x4c494e4b);
    assert!(
        read_photo(Cursor::new(&file), limits())
            .unwrap_err()
            .contains("linked ICC")
    );
    put32(&mut file, 14 + 56, 0x4d424544);
    put32(&mut file, 14 + 112, u32::MAX);
    assert!(
        read_photo(Cursor::new(&file), limits())
            .unwrap_err()
            .contains("profile range")
    );
}

fn gif_file(animation: bool, transparent: bool, profile: Option<&[u8]>) -> Vec<u8> {
    let mut file = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut file, 4, 2, &[0, 0, 255, 255, 0, 0]).unwrap();
        if let Some(profile) = profile {
            let mut blocks = vec![b"ICCRGBG1012".as_slice()];
            blocks.extend(profile.chunks(255));
            encoder
                .write_raw_extension(gif::Extension::Application.into(), &blocks)
                .unwrap();
        }
        let mut frame = gif::Frame::from_indexed_pixels(2, 1, [1, 0], transparent.then_some(0));
        frame.left = 1;
        frame.top = 1;
        encoder.write_frame(&frame).unwrap();
        if animation {
            encoder
                .write_frame(&gif::Frame::from_indexed_pixels(4, 2, [0; 8], None))
                .unwrap();
        }
    }
    file
}

#[test]
fn gif_profile_first_frame_offset_transparency_background_and_notice() {
    let icc = profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    for animated in [false, true] {
        for transparent in [false, true] {
            let file = gif_file(animated, transparent, Some(&icc));
            let photo = read_photo_detailed(Cursor::new(file), limits()).unwrap();
            assert_eq!(photo.first_frame, animated);
            let long_name = photo.display_name(&"a".repeat(200));
            assert_eq!(long_name.chars().count(), 128);
            assert_eq!(long_name.ends_with(" (first frame)"), animated);
            assert_eq!(
                photo.display_name("Photo"),
                if animated {
                    "Photo (first frame)"
                } else {
                    "Photo"
                }
            );
            assert_eq!(photo.source.extent, [4, 2]);
            assert_eq!(
                photo.source.interpretation.profile,
                ColorProfile::Icc(icc.clone().into())
            );
            let actual = pixels(&photo.source);
            assert_eq!(&actual[20..24], &[255, 0, 0, 255]);
            assert_eq!(actual[27], if transparent { 0 } else { 255 });
            assert_eq!(
                &actual[..4],
                if transparent {
                    &[0, 0, 0, 0]
                } else {
                    &[0, 0, 255, 255]
                }
            );
        }
    }
}

fn webp_file(icc: Option<&[u8]>, exif: Option<&[u8]>) -> Vec<u8> {
    let mut file = Vec::new();
    let mut encoder = image_webp::WebPEncoder::new(&mut file);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc.to_vec());
    }
    if let Some(exif) = exif {
        encoder.set_exif_metadata(exif.to_vec());
    }
    encoder
        .encode(
            &[10, 20, 30, 255, 40, 50, 60, 128],
            2,
            1,
            image_webp::ColorType::Rgba8,
        )
        .unwrap();
    file
}

#[test]
fn webp_retains_profile_and_normalizes_exif_once_including_density_axes() {
    let icc = profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    let resolution = layer_core::ImageResolution {
        unit: layer_core::ResolutionUnit::Inch,
        density: [[72, 1], [144, 1]],
    };
    let mut exif = super::metadata::exif_output(resolution).unwrap();
    // The existing EXIF writer emits orientation first in IFD0.
    exif[24..26].copy_from_slice(&6u16.to_le_bytes());
    let file = webp_file(Some(&icc), Some(&exif));
    let photo = read_photo_detailed(Cursor::new(file), limits()).unwrap();
    assert!(!photo.first_frame);
    assert_eq!(photo.source.extent, [1, 2]);
    assert_eq!(pixels(&photo.source), [10, 20, 30, 255, 40, 50, 60, 128]);
    assert_eq!(
        photo.source.interpretation.profile,
        ColorProfile::Icc(icc.into())
    );
    assert_eq!(
        photo.source.resolution.unwrap().density,
        [[144, 1], [72, 1]]
    );
    let mut png = Vec::new();
    write_png(&mut png, &photo.source).unwrap();
    let reopened = read_photo(Cursor::new(png), limits()).unwrap();
    assert_eq!(reopened.extent, photo.source.extent);
    assert_eq!(pixels(&reopened), pixels(&photo.source));
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(kind);
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(data);
    if data.len() & 1 != 0 {
        file.push(0);
    }
}
fn animated_webp() -> Vec<u8> {
    let still = webp_file(None, None);
    let mut file = b"RIFF\0\0\0\0WEBP".to_vec();
    // Alpha, animation; 4x2 canvas, first 2x1 image at (2,0).
    chunk(&mut file, b"VP8X", &[0x12, 0, 0, 0, 3, 0, 0, 1, 0, 0]);
    chunk(&mut file, b"ANIM", &[90, 80, 70, 128, 0, 0]);
    let mut frame = vec![1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 100, 0, 0, 2];
    frame.extend_from_slice(&still[12..]);
    chunk(&mut file, b"ANMF", &frame);
    chunk(&mut file, b"ANMF", &frame);
    let length = file.len() as u32 - 8;
    put32(&mut file, 4, length);
    file
}

#[test]
fn webp_animation_first_frame_composes_on_declared_background() {
    let photo = read_photo_detailed(Cursor::new(animated_webp()), limits()).unwrap();
    assert!(photo.first_frame);
    assert_eq!(photo.source.extent, [4, 2]);
    let actual = pixels(&photo.source);
    assert_eq!(&actual[..4], &[70, 80, 90, 128]);
    assert_eq!(&actual[8..16], &[10, 20, 30, 255, 40, 50, 60, 128]);
    assert_eq!(&actual[16..20], &[70, 80, 90, 128]);
}

#[test]
fn webp_entropy_tables_obey_the_decoder_budget() {
    let file = webp_file(None, None);
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(&file)).unwrap();
    decoder.set_memory_limit(1);
    let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
    assert!(matches!(
        decoder.read_image(&mut decoded),
        Err(image_webp::DecodingError::MemoryLimitExceeded)
    ));
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(file)).unwrap();
    decoder.set_memory_limit(1 << 20);
    decoder.read_image(&mut decoded).unwrap();
    assert_eq!(decoded, [10, 20, 30, 255, 40, 50, 60, 128]);
}

#[test]
#[ignore = "emit codec fixtures for the native GTK file/clipboard workflow"]
fn write_native_raster_fixtures() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("LAYER_RASTER_FIXTURES").expect("fixture output directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let icc = profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    for (name, bytes) in [
        ("Profiled.bmp", bmp_v5(&icc)),
        ("Animation.gif", gif_file(true, true, Some(&icc))),
        ("Profiled.webp", webp_file(Some(&icc), None)),
    ] {
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}

#[test]
fn new_raster_formats_reject_corruption_budgets_and_cancelled_io() {
    for file in [
        bmp_v5(&[]),
        gif_file(true, true, None),
        webp_file(None, None),
    ] {
        for size in 0..file.len() {
            // GIF's optional later frames are part of the structural validation.
            assert!(
                read_photo(Cursor::new(&file[..size]), limits()).is_err(),
                "truncated at {size}/{}",
                file.len()
            );
        }
        let budget = DecodeLimits {
            codec_bytes: 100,
            ..limits()
        };
        assert!(
            read_photo(Cursor::new(&file), budget)
                .unwrap_err()
                .contains("budget")
        );
        let budget = DecodeLimits {
            source_bytes: 1,
            ..limits()
        };
        assert!(read_photo(Cursor::new(&file), budget).is_err());
        let budget = DecodeLimits {
            dimension: 1,
            ..limits()
        };
        assert!(
            read_photo(Cursor::new(&file), budget)
                .unwrap_err()
                .contains("dimension")
        );
        // Decoders see an image-relative origin, including their absolute seeks.
        let mut prefixed = b"prefix".to_vec();
        prefixed.extend_from_slice(&file);
        let mut input = Cursor::new(prefixed);
        input.set_position(6);
        assert_eq!(
            pixels(&read_photo(input, limits()).unwrap()),
            pixels(&read_photo(Cursor::new(&file), limits()).unwrap())
        );
        struct Cancel(Cursor<Vec<u8>>);
        impl Read for Cancel {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("Image import cancelled"))
            }
        }
        impl Seek for Cancel {
            fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
                self.0.seek(from)
            }
        }
        assert!(
            read_photo(std::io::BufReader::new(Cancel(Cursor::new(file))), limits())
                .unwrap_err()
                .contains("cancelled")
        );
    }
    let mut bmp = bmp_v5(&[]);
    put32(&mut bmp, 18, 32768);
    put32(&mut bmp, 22, 32768);
    assert!(
        read_photo(Cursor::new(bmp), limits())
            .unwrap_err()
            .contains("budget")
    );
    let mut webp = animated_webp();
    webp[20] |= 0x20;
    assert!(
        read_photo(Cursor::new(&webp), limits())
            .unwrap_err()
            .contains("ICC")
    );
    webp[20] &= !0x20;
    webp[24] = 1; // Outer width 2 while first frame reaches x=4.
    assert!(
        read_photo(Cursor::new(webp), limits())
            .unwrap_err()
            .contains("outside")
    );
}
