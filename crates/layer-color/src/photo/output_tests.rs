use super::*;
use crate::{WorkingDecoder, WorkingEncoder};
use std::io::Cursor;

#[test]
fn working_rows_stream_into_profiled_files_without_an_intermediate_image() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let source = super::tests::fixture(depth, ColorProfile::Builtin(space));
            let decoder =
                WorkingDecoder::new(&source.interpretation, space, Default::default()).unwrap();
            let encoder =
                WorkingEncoder::new(space, &source.interpretation, Default::default()).unwrap();
            for tiff in [false, true] {
                let mut rows = source.rows();
                let mut original = vec![0; source.row_bytes()];
                let mut working = vec![[0.; 4]; source.extent[0] as usize];
                let mut calls = 0;
                let mut provider = |y, output: &mut [u8]| {
                    assert_eq!(y, calls);
                    calls += 1;
                    rows.read(y, &mut original)?;
                    decoder.decode_pixels(&original, &mut working)?;
                    encoder.encode_straight(&working, output, None, [0, 0])?;
                    Ok(())
                };
                let mut file = Cursor::new(Vec::new());
                if tiff {
                    write_tiff_rows(
                        &mut file,
                        source.extent,
                        encoder.interpretation(),
                        &mut provider,
                    )
                    .unwrap();
                } else {
                    write_png_rows(
                        &mut file,
                        source.extent,
                        encoder.interpretation(),
                        &mut provider,
                    )
                    .unwrap();
                }
                assert_eq!(calls, source.extent[1]);
                file.set_position(0);
                let restored = read_photo(file, DecodeLimits::default()).unwrap();
                super::tests::exact_pixels(&source, &restored);
                assert!(!restored.interpretation.profile_assumed);
            }
        }
    }
}

#[test]
fn invalid_output_is_rejected_before_writing_and_provider_failure_stops_rows() {
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: IntegerDepth::U16,
        profile: ColorProfile::default(),
        profile_assumed: false,
    };
    for tiff in [false, true] {
        for extent in [[0, 1], [32769, 1], [1, 0]] {
            let mut output = Cursor::new(Vec::new());
            let provider = |_, _: &mut [u8]| panic!("invalid output called provider");
            let result = if tiff {
                write_tiff_rows(&mut output, extent, &interpretation, provider)
            } else {
                write_png_rows(&mut output, extent, &interpretation, provider)
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
            write_tiff_rows(&mut output, [513, 257], &interpretation, provider)
        } else {
            write_png_rows(&mut output, [513, 257], &interpretation, provider)
        };
        assert_eq!(result.unwrap_err(), "cancelled row provider");
        assert_eq!(calls, 4);
    }
}
