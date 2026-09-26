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
