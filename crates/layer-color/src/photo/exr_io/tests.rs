use super::*;
use std::io::Cursor;

#[test]
fn float32_exr_lossless_channels_primaries_white_alpha_and_subnormals() {
    for space in RgbSpace::ALL {
        let pixels = [
            [1.0000001, -0.12345679, 100000., 1.],
            [1e-30, -1e10, 0.125, 0.125],
            [f32::from_bits(1), -0., f32::MIN_POSITIVE, 1.],
            [0.; 4],
        ];
        let mut output = Cursor::new(Vec::new());
        write_exr_rows(
            &mut output,
            [4, 1],
            space,
            Some(layer_core::ImageResolution::ppi(300)),
            |_, row| {
                row.copy_from_slice(&pixels);
                Ok(())
            },
        )
        .unwrap();
        let bytes = output.into_inner();
        let header = block::read(Cursor::new(&bytes), true).unwrap().headers()[0].clone();
        assert_eq!(header.own_attributes.white_luminance, Some(203.));
        assert_eq!(
            header.shared_attributes.chromaticities,
            Some(chromaticities(space))
        );
        // Inspect codec bytes before alpha conversion: no channel is half rounded.
        let block = block::read(Cursor::new(&bytes), true)
            .unwrap()
            .all_chunks(true)
            .unwrap()
            .sequential_decompressor(true)
            .next()
            .unwrap()
            .unwrap();
        for (channel, plane) in [3, 2, 1, 0].into_iter().zip(block.data.chunks_exact(16)) {
            for (pixel, actual) in pixels.iter().zip(plane.chunks_exact(4)) {
                assert_eq!(
                    u32::from_ne_bytes(actual.try_into().unwrap()),
                    pixel[channel].to_bits()
                );
            }
        }
        let image = read_photo(Cursor::new(bytes), Default::default()).unwrap();
        assert_eq!(image.interpretation.depth, SampleDepth::F32);
        assert_eq!(image.resolution.unwrap().pixels_per_inch(), [300.; 2]);
        assert_eq!(image.interpretation.profile, ColorProfile::Builtin(space));
        let mut row = vec![0; image.row_bytes()];
        image.rows().read(0, &mut row).unwrap();
        for (bytes, input) in row.chunks_exact(16).zip(pixels) {
            let actual = hdr::decode_samples(SampleDepth::F32, bytes).unwrap();
            let expected = if input[3] == 0. {
                [0.; 4]
            } else {
                [
                    input[0] / input[3],
                    input[1] / input[3],
                    input[2] / input[3],
                    input[3],
                ]
            };
            assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
        }
    }
}

#[test]
fn exr_limits_cancellation_truncation_and_invalid_pixels_fail() {
    let mut output = Cursor::new(Vec::new());
    write_exr_rows(&mut output, [2, 2], RgbSpace::Srgb, None, |_, row| {
        row.fill([1., -2., 1e6, 1.]);
        Ok(())
    })
    .unwrap();
    let bytes = output.into_inner();
    for end in [0, 7, 32, bytes.len() - 1] {
        assert!(
            read_exr(
                Cursor::new(&bytes[..end]),
                Default::default(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    assert!(
        read_exr(
            Cursor::new(&bytes),
            DecodeLimits {
                codec_bytes: 64,
                ..Default::default()
            },
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        read_exr(
            Cursor::new(&bytes),
            DecodeLimits {
                source_bytes: 1,
                ..Default::default()
            },
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        read_exr(
            Cursor::new(&bytes),
            Default::default(),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    for pixel in [[f32::INFINITY, 0., 0., 1.], [0., 0., 0., -0.1]] {
        assert!(
            write_exr_rows(
                Cursor::new(Vec::new()),
                [1, 1],
                RgbSpace::Srgb,
                None,
                |_, row| {
                    row[0] = pixel;
                    Ok(())
                }
            )
            .is_err()
        );
    }
    assert!(
        write_exr_rows(
            Cursor::new(Vec::new()),
            [1, 1],
            RgbSpace::Srgb,
            None,
            |_, _| Err("cancelled".into())
        )
        .is_err()
    );
}

fn fixture(mut header: Header, pixels: &[[f32; 4]]) -> Vec<u8> {
    header.own_attributes.layer_name = None;
    let mut output = Cursor::new(Vec::new());
    block::write(&mut output, vec![header].into(), true, |meta, writer| {
        let mut compressor = SequentialBlocksCompressor::new(&meta, writer);
        for (index, location) in block::enumerate_ordered_header_block_indices(&meta.headers) {
            let mut data = Vec::new();
            for y in location.pixel_position.1..location.pixel_position.1 + location.pixel_size.1 {
                for channel in &meta.headers[0].channels.list {
                    let c = match channel.name.to_string().as_str() {
                        "R" => 0,
                        "G" => 1,
                        "B" => 2,
                        _ => 3,
                    };
                    data.extend_from_slice(&pixels[y][c].to_ne_bytes());
                }
            }
            compressor.compress_block(
                index,
                UncompressedBlock {
                    index: location,
                    data,
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
    output.into_inner()
}

#[test]
fn exr_rgb_white_scaling_lossless_codecs_and_unsupported_metadata() {
    let header = || {
        Header::new(
            "RGB".try_into().unwrap(),
            (1, 2),
            ["B", "G", "R"]
                .into_iter()
                .map(|n| ChannelDescription::new(n, SampleType::F32, true))
                .collect(),
        )
        .with_encoding(
            Compression::Uncompressed,
            BlockDescription::ScanLines,
            LineOrder::Increasing,
        )
    };
    for compression in [
        Compression::Uncompressed,
        Compression::RLE,
        Compression::ZIP1,
        Compression::ZIP16,
    ] {
        let mut h = header();
        h = h.with_encoding(compression, BlockDescription::ScanLines, LineOrder::Increasing);
        h.own_attributes.white_luminance = Some(406.);
        let bytes = fixture(h, &[[100000.125, -0.25, 1e-30, 1.]; 2]);
        let image = read_photo(Cursor::new(bytes), Default::default()).unwrap();
        assert_eq!(image.interpretation.channels, SourceChannels::Rgb);
        assert!(image.interpretation.profile_assumed);
        let mut row = vec![0; image.row_bytes()];
        image.rows().read(1, &mut row).unwrap();
        assert_eq!(
            hdr::decode_samples(SampleDepth::F32, &row).unwrap(),
            [200000.25, -0.5, 2e-30, 1.]
        );
    }
    let mut h = header();
    h.own_attributes.adopted_neutral = Some(Vec2(0.3127, 0.329));
    assert!(
        read_photo(Cursor::new(fixture(h, &[[0.; 4]; 2])), Default::default())
            .unwrap_err()
            .contains("adopted-neutral")
    );
    let mut h = header();
    let mut primaries = chromaticities(RgbSpace::Srgb);
    primaries.red = Vec2(0.61, 0.33);
    h.shared_attributes.chromaticities = Some(primaries);
    assert!(
        read_photo(Cursor::new(fixture(h, &[[0.; 4]; 2])), Default::default())
            .unwrap_err()
            .contains("primaries")
    );
    let mut h = header();
    h.channels = exr::meta::attribute::ChannelList::new(["A", "B", "G", "R"]
        .into_iter()
        .map(|n| ChannelDescription::new(n, SampleType::F32, true))
        .collect());
    assert!(
        read_photo(
            Cursor::new(fixture(h, &[[1., 0., 0., 0.]; 2])),
            Default::default()
        )
        .unwrap_err()
        .contains("zero-alpha")
    );
    let mut malformed = Vec::from([0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0]);
    malformed.extend_from_slice(b"oversized\0string\0");
    malformed.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        read_photo(Cursor::new(malformed), Default::default())
            .unwrap_err()
            .contains("budget")
    );
}
