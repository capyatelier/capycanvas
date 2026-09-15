use super::*;
use layer_core::{ImageResolution, ResolutionUnit};
use std::io::Cursor;

#[test]
fn png_tiff_and_jpeg_keep_physical_density_without_changing_encoded_samples() {
    let density = ImageResolution {
        unit: ResolutionUnit::Inch,
        density: [[601, 2], [300, 1]],
    };
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
        let mut source = super::tests::fixture(depth, ColorProfile::Builtin(RgbSpace::ProPhoto));
        let mut before = vec![0; source.row_bytes()];
        let mut after = before.clone();
        source.resolution = Some(density);
        for tiff in [false, true] {
            let mut bytes = Cursor::new(Vec::new());
            if tiff {
                write_tiff(&mut bytes, &source).unwrap();
            } else {
                write_png(&mut bytes, &source).unwrap();
            }
            let image = read_photo(Cursor::new(bytes.into_inner()), Default::default()).unwrap();
            assert_eq!(image.extent, source.extent);
            let actual = image.resolution.unwrap();
            if tiff {
                assert_eq!(actual, density);
            }
            for (a, e) in actual
                .pixels_per_inch()
                .into_iter()
                .zip(density.pixels_per_inch())
            {
                assert!((a - e).abs() <= if tiff { 1e-10 } else { 0.01271 });
            }
            let mut expected = source.rows();
            let mut rows = image.rows();
            for y in 0..source.extent[1] {
                expected.read(y, &mut before).unwrap();
                rows.read(y, &mut after).unwrap();
                assert_eq!(before, after);
            }
        }
    }
    let mut builder = SourceBuilder::new(
        [37, 19],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: IntegerDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for y in 0..19 {
        builder
            .push_row(
                &(0..37)
                    .flat_map(|x| [x * 6, y * 13, 100])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    let mut source = builder.finish().unwrap();
    let mut plain = Vec::new();
    write_jpeg(&mut plain, &source, 93).unwrap();
    source.resolution = Some(density);
    let mut tagged = Vec::new();
    write_jpeg(&mut tagged, &source, 93).unwrap();
    let a = read_jpeg(Cursor::new(plain), Default::default()).unwrap();
    let b = read_jpeg(Cursor::new(&tagged), Default::default()).unwrap();
    assert!(a.resolution.is_none());
    assert_eq!(b.resolution, Some(density));
    let mut ar = a.rows();
    let mut br = b.rows();
    let mut av = vec![0; a.row_bytes()];
    let mut bv = av.clone();
    for y in 0..19 {
        ar.read(y, &mut av).unwrap();
        br.read(y, &mut bv).unwrap();
        assert_eq!(av, bv);
    }
    // Read the actual JFIF segment directly; Exif above retains fractional PPI.
    let marker = tagged.windows(5).position(|w| w == b"JFIF\0").unwrap();
    assert_eq!(tagged[marker + 7], 1);
    assert_eq!(&tagged[marker + 8..marker + 12], &[1, 45, 1, 44]); // 301 × 300
}

#[test]
fn png_density_precedes_exif_and_orientation_swaps_density_axes() {
    let density = ImageResolution {
        unit: ResolutionUnit::Inch,
        density: [[300, 1], [150, 1]],
    };
    let mut exif = super::metadata::exif_output(density).unwrap();
    exif[24..26].copy_from_slice(&6u16.to_le_bytes());
    let mut info = png::Info::with_size(3, 2);
    info.color_type = png::ColorType::Rgb;
    info.bit_depth = png::BitDepth::Eight;
    info.exif_metadata = Some(exif[6..].to_vec().into());
    info.pixel_dims = Some(png::PixelDimensions {
        xppu: 6000,
        yppu: 3000,
        unit: png::Unit::Meter,
    });
    let mut bytes = Vec::new();
    png::Encoder::with_info(&mut bytes, info)
        .unwrap()
        .write_header()
        .unwrap()
        .write_image_data(&[100; 18])
        .unwrap();
    let image = read_png(Cursor::new(bytes), Default::default()).unwrap();
    assert_eq!(image.extent, [2, 3]);
    assert_eq!(
        image.resolution,
        Some(ImageResolution {
            unit: ResolutionUnit::Metre,
            density: [[3000, 1], [6000, 1]]
        })
    );
    // Unitless/zero density is not silently replaced with a guessed print size.
    assert!(super::metadata::physical(1, [Some([300, 1]); 2]).is_none());
    assert!(super::metadata::physical(2, [Some([0, 1]); 2]).is_none());
    assert!(super::metadata::physical(3, [Some([300, 0]); 2]).is_none());
}

#[test]
fn gray_and_cmyk_jpeg_resolution_is_retained_with_the_delivery_profile() {
    let mut cases = vec![(
        SourceChannels::Gray,
        crate::gray_profile(RgbSpace::ProPhoto).unwrap(),
    )];
    if let Some(path) = std::env::var_os("LAYER_TEST_CMYK_PROFILE") {
        let bytes = std::fs::read(path).unwrap();
        eprintln!("Resolution CMYK ICC: {} bytes", bytes.len());
        cases.push((SourceChannels::Cmyk, ColorProfile::Icc(bytes.into())));
    }
    for (channels, profile) in cases {
        let interpretation = SourceInterpretation {
            channels,
            profile,
            depth: IntegerDepth::U8,
            profile_assumed: false,
        };
        let mut builder = SourceBuilder::new([7, 3], interpretation.clone(), 1024 * 1024).unwrap();
        for _ in 0..3 {
            builder.push_row(&vec![75; 7 * channels.count()]).unwrap();
        }
        let mut source = builder.finish().unwrap();
        source.resolution = Some(ImageResolution::ppi(300));
        let mut encoded = Vec::new();
        write_jpeg(&mut encoded, &source, 95).unwrap();
        let decoded = read_jpeg(Cursor::new(encoded), Default::default()).unwrap();
        assert_eq!(decoded.resolution, source.resolution);
        assert_eq!(decoded.interpretation, interpretation);
    }
}
