use super::*;
use std::io::Cursor;

#[test]
fn invalid_output_is_rejected_before_writing_and_provider_failure_stops_rows() {
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U16,
        profile: ColorProfile::default(),
        profile_assumed: false,
    };
    for tiff in [false, true] {
        for extent in [[0, 1], [layer_core::MAX_EXTENT + 1, 1], [1, 0]] {
            let mut output = Cursor::new(Vec::new());
            let provider = |_, _: &mut [u8]| panic!("invalid output called provider");
            let result = if tiff {
                write_tiff_rows(&mut output, extent, &interpretation, &Default::default(), provider)
            } else {
                write_png_rows(&mut output, extent, &interpretation, &Default::default(), provider)
            };
            assert!(result.is_err());
            assert!(output.into_inner().is_empty());
        }
        let mut output = Cursor::new(Vec::new());
        super::test_support::assert_provider_failure("cancelled row provider", |provider| {
            if tiff {
                write_tiff_rows(&mut output, [513, 257], &interpretation, &Default::default(), provider)
            } else {
                write_png_rows(&mut output, [513, 257], &interpretation, &Default::default(), provider)
            }
        });
    }
}

#[test]
fn writers_accept_the_largest_side_their_format_allows() {
    let rgba = SourceInterpretation { channels: SourceChannels::Rgba, depth: SampleDepth::U8, profile: ColorProfile::default(), profile_assumed: false };
    let rgb = SourceInterpretation { channels: SourceChannels::Rgb, ..rgba.clone() };
    let jpeg = JpegEncodeOptions::from_memory_budget(90, PhotoMemoryBudget::from_limit(1 << 30));
    let wide = [layer_core::MAX_EXTENT, 2];
    let fill = |_, row: &mut [u8]| { row.fill(200); Ok(()) };
    let linear = |_, row: &mut [[f32; 4]]| { row.fill([0.5; 4]); Ok(()) };
    let (mut png, mut tiff, mut openexr, mut pq, mut jpg) = (Vec::new(), Cursor::new(Vec::new()), Cursor::new(Vec::new()), Vec::new(), Vec::new());
    write_png_rows(&mut png, wide, &rgba, &Default::default(), fill).unwrap();
    write_tiff_rows(&mut tiff, wide, &rgba, &Default::default(), fill).unwrap();
    write_exr_rows(&mut openexr, wide, RgbSpace::Srgb, None, linear).unwrap();
    write_hdr_png_rows(&mut pq, wide, RgbSpace::Srgb, &Default::default(), false, linear).unwrap();
    write_jpeg_rows(&mut jpg, [JPEG_MAX_DIMENSION, 2], &rgb, &Default::default(), jpeg, fill).unwrap();
    for (bytes, extent) in [(png, wide), (tiff.into_inner(), wide), (jpg, [JPEG_MAX_DIMENSION, 2])] {
        assert_eq!(read_photo(Cursor::new(bytes), DecodeLimits::from_memory_budget(PhotoMemoryBudget::from_limit(1 << 30))).unwrap().extent, extent);
    }
    assert_eq!(pq[16..24], [wide[0].to_be_bytes(), wide[1].to_be_bytes()].concat());
    assert_eq!(exr::block::read(Cursor::new(openexr.into_inner()), true).unwrap().headers()[0].layer_size, exr::math::Vec2(wide[0] as usize, wide[1] as usize));
    assert!(write_jpeg_rows(Vec::new(), [JPEG_MAX_DIMENSION + 1, 2], &rgb, &Default::default(), jpeg, |_, _| panic!("a refused JPEG requested pixels")).is_err());
}

fn webp_interpretation(channels: SourceChannels, depth: SampleDepth) -> SourceInterpretation {
    SourceInterpretation {
        channels,
        depth,
        profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
        profile_assumed: false,
    }
}

fn riff_chunk<'a>(file: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    let mut at = 12;
    while at + 8 <= file.len() {
        let size = u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap()) as usize;
        if &file[at..at + 4] == kind {
            return Some(&file[at + 8..at + 8 + size]);
        }
        at += 8 + size + (size & 1);
    }
    None
}

#[test]
fn lossless_webp_round_trips_pixels_alpha_profile_and_resolution() {
    let extent = [37, 23];
    let resolution = layer_core::ImageResolution {
        unit: layer_core::ResolutionUnit::Inch,
        density: [[300, 1], [150, 1]],
    };
    for channels in [SourceChannels::Rgb, SourceChannels::Rgba] {
        let interpretation = webp_interpretation(channels, SampleDepth::U8);
        let row_bytes = extent[0] as usize * channels.count();
        let expected: Vec<u8> = (0..row_bytes * extent[1] as usize)
            .map(|i| (i * 37 % 251 + i / row_bytes * 3) as u8)
            .collect();
        let mut output = Vec::new();
        write_webp_rows(&mut output, extent, &interpretation, &crate::photo::DeliveryMetadata::resolution(Some(resolution)), PhotoMemoryBudget::from_limit(1 << 30).encode_bytes, |y, row| {
            row.copy_from_slice(&expected[y as usize * row_bytes..][..row_bytes]);
            Ok(())
        })
        .unwrap();
        assert!(riff_chunk(&output, b"VP8L").is_some(), "lossless bitstream");
        let exif = riff_chunk(&output, b"EXIF").unwrap();
        assert!(exif.starts_with(b"II*\0"), "a WebP EXIF chunk holds bare TIFF data");
        assert_eq!(
            riff_chunk(&output, b"ICCP").unwrap(),
            profile_bytes(&interpretation.profile).unwrap().as_slice()
        );
        let photo = read_photo_detailed(Cursor::new(&output), DecodeLimits::from_memory_budget(PhotoMemoryBudget::from_limit(1 << 30))).unwrap();
        assert!(!photo.first_frame);
        assert_eq!(photo.source.extent, extent);
        assert_eq!(photo.source.interpretation.channels, channels, "opaque rows stay opaque");
        assert_eq!(photo.source.interpretation.depth, SampleDepth::U8);
        assert_eq!(
            profile_bytes(&photo.source.interpretation.profile).unwrap(),
            profile_bytes(&interpretation.profile).unwrap()
        );
        assert_eq!(photo.source.resolution, Some(resolution));
        assert_eq!(super::test_support::rows(&photo.source).concat(), expected);
        let mut bare = Vec::new();
        write_webp_rows(&mut bare, [1, 1], &interpretation, &Default::default(), PhotoMemoryBudget::from_limit(1 << 30).encode_bytes, |_, row| {
            row.fill(200);
            Ok(())
        })
        .unwrap();
        assert!(riff_chunk(&bare, b"EXIF").is_none());
        assert_eq!(read_photo(Cursor::new(bare), DecodeLimits::from_memory_budget(PhotoMemoryBudget::from_limit(1 << 30))).unwrap().resolution, None);
    }
}

#[test]
fn webp_refuses_oversize_unsupported_over_budget_and_cancelled_output() {
    let rgba = webp_interpretation(SourceChannels::Rgba, SampleDepth::U8);
    for (extent, interpretation, available, message) in [
        ([16385, 1], rgba.clone(), 1 << 30, "16,384"),
        ([1, 16385], rgba.clone(), 1 << 30, "16,384"),
        ([4, 4], webp_interpretation(SourceChannels::Rgba, SampleDepth::U16), 1 << 30, "8-bit"),
        ([4, 4], webp_interpretation(SourceChannels::GrayAlpha, SampleDepth::U8), 1 << 30, "RGB"),
        ([64, 64], rgba.clone(), 1024, "memory budget"),
    ] {
        let mut output = Vec::new();
        let error = write_webp_rows(&mut output, extent, &interpretation, &Default::default(), PhotoMemoryBudget::from_limit(available).encode_bytes, |_, _| {
            panic!("a refused export requested pixels")
        })
        .unwrap_err();
        assert!(error.contains(message), "{extent:?}: {error}");
        assert!(output.is_empty());
    }
    let admitted = 64 * 64 * 12 + profile_bytes(&rgba.profile).unwrap().len();
    let fits = |available: usize| {
        write_webp_rows(Vec::new(), [64, 64], &rgba, &Default::default(), available, |_, row| {
            row.fill(9);
            Ok(())
        })
    };
    assert!(fits(admitted).is_ok());
    assert!(fits(admitted - 1).unwrap_err().contains("memory budget"));
    let mut output = Vec::new();
    super::test_support::assert_provider_failure("Export cancelled", |provider| {
        write_webp_rows(&mut output, [513, 257], &rgba, &Default::default(), PhotoMemoryBudget::from_limit(1 << 30).encode_bytes, provider)
    });
    assert!(output.is_empty(), "nothing is encoded after a cancellation");
}
