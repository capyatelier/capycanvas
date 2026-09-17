//! The supported interchange subset is deliberate; unsupported layouts fail
//! before pixel adoption instead of being partially decoded as another image.
use super::*;
use std::io::Cursor;
use tiff::{
    encoder::{Compression, TiffEncoder, colortype},
    tags::Tag,
};

#[test]
fn classic_and_big_tiff_interleaved_strips_decode_all_lossless_codec_variants() {
    let values: Vec<u16> = (0..33 * 17 * 3).map(|i| (i * 137) as u16).collect();
    let profile = profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
    let directory = std::env::temp_dir().join(format!("capy-tiff-policy-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    eprintln!("TIFF policy fixtures: {}", directory.display());
    for big in [false, true] {
        for compression in [
            Compression::Uncompressed,
            Compression::Lzw,
            Compression::Deflate(Default::default()),
            Compression::Packbits,
        ] {
            let mut file = Cursor::new(Vec::new());
            macro_rules! encode {
                ($encoder:expr) => {{
                    let mut encoder = $encoder.unwrap().with_compression(compression);
                    let mut image = encoder.new_image::<colortype::RGB16>(33, 17).unwrap();
                    image
                        .encoder()
                        .write_tag(Tag::IccProfile, profile.as_slice())
                        .unwrap();
                    image.rows_per_strip(3).unwrap();
                    // The pinned encoder enables compression in write_data;
                    // direct write_strip only writes raw data despite the tag.
                    image.write_data(&values).unwrap();
                }};
            }
            if big {
                encode!(TiffEncoder::new_big(&mut file));
            } else {
                encode!(TiffEncoder::new(&mut file));
            }
            let name = match compression {
                Compression::Uncompressed => "raw",
                Compression::Lzw => "lzw",
                Compression::Deflate(_) => "zip",
                Compression::Packbits => "packbits",
            };
            std::fs::write(directory.join(format!("{big}-{name}.tif")), file.get_ref()).unwrap();
            let source = read_tiff(Cursor::new(file.into_inner()), Default::default())
                .unwrap_or_else(|e| panic!("big={big} codec={name}: {e}"));
            assert_eq!(source.extent, [33, 17]);
            assert_eq!(source.interpretation.depth, SampleDepth::U16);
            assert_eq!(
                source.interpretation.profile,
                ColorProfile::Icc(profile.clone().into())
            );
            let mut rows = source.rows();
            let mut row = vec![0; source.row_bytes()];
            for y in 0..17 {
                rows.read(y, &mut row).unwrap();
                let decoded: Vec<_> = row
                    .chunks_exact(2)
                    .map(|v| u16::from_le_bytes(v.try_into().unwrap()))
                    .collect();
                assert_eq!(&decoded, &values[y as usize * 99..(y as usize + 1) * 99]);
            }
        }
    }
}

#[test]
fn unsupported_tiff_layouts_and_classic_output_overflow_fail_explicitly() {
    // Minimal valid planar RGB TIFF: three four-byte planes, each in one strip.
    let mut planar = b"II*\0\x08\0\0\0".to_vec();
    planar.extend_from_slice(&10u16.to_le_bytes());
    for (tag, ty, count, value) in [
        (256u16, 4u16, 1u32, 2u32),
        (257, 4, 1, 2),
        (258, 3, 3, 134),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 3, 140),
        (277, 3, 1, 3),
        (278, 4, 1, 2),
        (279, 4, 3, 152),
        (284, 3, 1, 2),
    ] {
        planar.extend_from_slice(&tag.to_le_bytes());
        planar.extend_from_slice(&ty.to_le_bytes());
        planar.extend_from_slice(&count.to_le_bytes());
        planar.extend_from_slice(&value.to_le_bytes());
    }
    planar.extend_from_slice(&0u32.to_le_bytes());
    for v in [8u16; 3] {
        planar.extend_from_slice(&v.to_le_bytes());
    }
    for v in [164u32, 168, 172, 4, 4, 4] {
        planar.extend_from_slice(&v.to_le_bytes());
    }
    planar.extend_from_slice(&[127; 12]);
    assert!(
        read_tiff(Cursor::new(planar), Default::default())
            .unwrap_err()
            .contains("Planar TIFF")
    );
    for (tag, value, reason) in [(Tag::ExtraSamples, 1, "unassociated alpha")] {
        let mut file = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut file).unwrap();
            let mut image = encoder.new_image::<colortype::RGBA8>(2, 2).unwrap();
            image.encoder().write_tag(tag, &[value as u16][..]).unwrap();
            image.write_data(&[127; 16]).unwrap();
        }
        let error = read_tiff(Cursor::new(file.into_inner()), Default::default()).unwrap_err();
        assert!(error.contains(reason), "{reason}: {error}");
    }
    let mut file = Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut file).unwrap();
        for _ in 0..2 {
            encoder
                .write_image::<colortype::RGB8>(2, 2, &[100; 12])
                .unwrap();
        }
    }
    assert!(
        read_tiff(Cursor::new(file.into_inner()), Default::default())
            .unwrap_err()
            .contains("Multi-page TIFF")
    );
    let mut file = Cursor::new(Vec::new());
    TiffEncoder::new(&mut file)
        .unwrap()
        .write_image::<colortype::RGB32Float>(2, 2, &[2.; 12])
        .unwrap();
    assert!(
        read_tiff(Cursor::new(file.into_inner()), Default::default())
            .unwrap_err()
            .contains("unsigned integer")
    );
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U16,
        profile: Default::default(),
        profile_assumed: false,
    };
    let mut output = Cursor::new(Vec::new());
    let error = write_tiff_rows(
        &mut output,
        [32768, 32768],
        &interpretation,
        None,
        |_, _| panic!("overflow requested pixels"),
    )
    .unwrap_err();
    assert!(error.contains("4 GiB"));
    assert!(output.into_inner().is_empty());
}
