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
        for extent in [[0, 1], [32769, 1], [1, 0]] {
            let mut output = Cursor::new(Vec::new());
            let provider = |_, _: &mut [u8]| panic!("invalid output called provider");
            let result = if tiff {
                write_tiff_rows(&mut output, extent, &interpretation, None, provider)
            } else {
                write_png_rows(&mut output, extent, &interpretation, None, provider)
            };
            assert!(result.is_err());
            assert!(output.into_inner().is_empty());
        }
        let mut calls = 0;
        let provider = |y, row: &mut [u8]| {
            calls += 1;
            if y == 3 {
                return Err("cancelled row provider".into());
            }
            row.fill(0);
            Ok(())
        };
        let mut output = Cursor::new(Vec::new());
        let result = if tiff {
            write_tiff_rows(&mut output, [513, 257], &interpretation, None, provider)
        } else {
            write_png_rows(&mut output, [513, 257], &interpretation, None, provider)
        };
        assert_eq!(result.unwrap_err(), "cancelled row provider");
        assert_eq!(calls, 4);
    }
}

fn webp_interpretation(channels: SourceChannels, depth: SampleDepth) -> SourceInterpretation {
    SourceInterpretation {
        channels,
        depth,
        profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
        profile_assumed: false,
    }
}

fn webp_options(available: u64) -> WebpEncodeOptions {
    WebpEncodeOptions::from_memory_budget(PhotoMemoryBudget::from_available_memory(available))
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
        write_webp_rows(&mut output, extent, &interpretation, Some(resolution), webp_options(1 << 30), |y, row| {
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
        let photo = read_photo_detailed(Cursor::new(&output), DecodeLimits::default()).unwrap();
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
        write_webp_rows(&mut bare, [1, 1], &interpretation, None, webp_options(1 << 30), |_, row| {
            row.fill(200);
            Ok(())
        })
        .unwrap();
        assert!(riff_chunk(&bare, b"EXIF").is_none());
        assert_eq!(read_photo(Cursor::new(bare), DecodeLimits::default()).unwrap().resolution, None);
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
        let error = write_webp_rows(&mut output, extent, &interpretation, None, webp_options(available), |_, _| {
            panic!("a refused export requested pixels")
        })
        .unwrap_err();
        assert!(error.contains(message), "{extent:?}: {error}");
        assert!(output.is_empty());
    }
    let admitted = 64 * 64 * 12 + profile_bytes(&rgba.profile).unwrap().len();
    let fits = |available: usize| {
        write_webp_rows(Vec::new(), [64, 64], &rgba, None, WebpEncodeOptions { codec_bytes: available }, |_, row| {
            row.fill(9);
            Ok(())
        })
    };
    assert!(fits(admitted).is_ok());
    assert!(fits(admitted - 1).unwrap_err().contains("memory budget"));
    let mut calls = 0;
    let mut output = Vec::new();
    let error = write_webp_rows(&mut output, [513, 257], &rgba, None, webp_options(1 << 30), |y, row| {
        calls += 1;
        if y == 3 {
            return Err("Export cancelled".into());
        }
        row.fill(0);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error, "Export cancelled");
    assert_eq!(calls, 4, "rows stop at the cancelled one");
    assert!(output.is_empty(), "nothing is encoded after a cancellation");
}
