use super::*;
use layer_core::color::{SampleDepth, RgbSpace};

#[test]
fn hdr_rasterization_retains_range_alpha_and_rejects_overflow() {
    use layer_core::color::hdr;
    for space in RgbSpace::ALL {
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba, depth: SampleDepth::F16,
            profile: ColorProfile::Builtin(space), profile_assumed: false,
        };
        let pixels = [[8., -0.125, 2., 0.5], [1. / 65536., 4., -2., 1. / 65536.], [8., -1., 4., 0.]];
        let bytes: Vec<_> = pixels.into_iter().flat_map(|p| hdr::encode_pixel(p).unwrap())
            .flat_map(u16::to_le_bytes).collect();
        let mut builder = SourceBuilder::new([3, 1], interpretation, 1024 * 1024).unwrap();
        builder.push_row(&bytes).unwrap();
        let original = builder.finish().unwrap();
        for target in RgbSpace::ALL {
            let (raster, stats) = rasterize_source(&original,
                DocumentColor { space: target, depth: SampleDepth::F16 }, 1024 * 1024, || false).unwrap();
            assert_eq!(stats.clipped_channels, 0);
            let mut actual = vec![0; raster.row_bytes()];
            raster.rows().read(0, &mut actual).unwrap();
            if target == space { assert_eq!(actual, bytes); }
            let matrix = space.linear_transform(target);
            for (input, output) in pixels.into_iter().zip(actual.chunks_exact(8)) {
                let expected = layer_core::color::rgb::apply(matrix, [input[0] as f64, input[1] as f64, input[2] as f64]);
                let output = hdr::decode_pixel(std::array::from_fn(|c| u16::from_le_bytes([output[c * 2], output[c * 2 + 1]]))).unwrap();
                for c in 0..3 { assert!((f64::from(output[c]) - expected[c]).abs() <= expected[c].abs() / 1024. + 1. / 16777216.); }
                assert_eq!(output[3], input[3]);
            }
        }
        assert!(rasterize_source(&original, DocumentColor { space, depth: SampleDepth::F16 }, 1024 * 1024, || true).is_err());
    }
    let target = SourceInterpretation { channels: SourceChannels::Rgba, depth: SampleDepth::F16,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
    let encoder = WorkingEncoder::new(RgbSpace::Srgb, &target, Default::default()).unwrap();
    assert!(encoder.encode_straight(&[[65505., 0., 0., 1.]], &mut [0; 8], None, [0, 0]).is_err());
}

fn fixture(depth: SampleDepth, space: RgbSpace, extent: [u32; 2]) -> SourceImage {
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth,
            profile: ColorProfile::Builtin(space),
            profile_assumed: true,
        },
        4 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let row: Vec<_> = (0..extent[0])
            .flat_map(|x| {
                let code = (x + y * extent[0]) % (depth.maximum() + 1);
                [code, depth.maximum() - code, code / 3, code]
            })
            .flat_map(|code| {
                if depth == SampleDepth::U16 {
                    (code as u16).to_le_bytes().to_vec()
                } else {
                    vec![code as u8]
                }
            })
            .collect();
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}

#[test]
fn exact_integer_identity_and_depth_changes_preserve_samples_alpha_and_extent() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let extent = if depth == SampleDepth::U16 {
                [256, 256]
            } else {
                [256, 1]
            };
            let original = fixture(depth, space, extent);
            let (materialized, stats) = rasterize_source(
                &original,
                DocumentColor { space, depth },
                4 * 1024 * 1024,
                || false,
            )
            .unwrap();
            assert_eq!(materialized.kind, SourceKind::Rasterized);
            assert_eq!(materialized.extent, extent);
            assert_eq!(stats.clipped_channels, 0);
            assert!(!materialized.interpretation.profile_assumed);
            for (key, tile) in &original.tiles {
                assert!(std::sync::Arc::ptr_eq(tile, &materialized.tiles[key]));
            }
            assert!(original.is_original());
        }
        let original = fixture(SampleDepth::U8, space, [256, 3]);
        let (promoted, _) = rasterize_source(
            &original,
            DocumentColor {
                space,
                depth: SampleDepth::U16,
            },
            4 * 1024 * 1024,
            || false,
        )
        .unwrap();
        let mut a = vec![0; original.row_bytes()];
        let mut b = vec![0; promoted.row_bytes()];
        original.rows().read(0, &mut a).unwrap();
        promoted.rows().read(0, &mut b).unwrap();
        for (a, b) in a.into_iter().zip(b.chunks_exact(2)) {
            assert_eq!(
                u16::from_le_bytes([b[0], b[1]]),
                a as u16 * 257,
                "{space:?}"
            );
        }
        let original = fixture(SampleDepth::U16, space, [256, 256]);
        let (reduced, stats) = rasterize_source(
            &original,
            DocumentColor {
                space,
                depth: SampleDepth::U8,
            },
            4 * 1024 * 1024,
            || false,
        )
        .unwrap();
        let mut rows_a = original.rows();
        let mut rows_b = reduced.rows();
        let mut a = vec![0; original.row_bytes()];
        let mut b = vec![0; reduced.row_bytes()];
        for y in 0..256 {
            rows_a.read(y, &mut a).unwrap();
            rows_b.read(y, &mut b).unwrap();
            for (a, b) in a.chunks_exact(2).zip(&b) {
                let code = u16::from_le_bytes([a[0], a[1]]) as u32;
                assert_eq!(*b as u32, (code + 128) / 257, "{space:?} {code}");
            }
        }
        assert_eq!(stats.clipped_channels, 0);
    }
}

#[test]
fn conversion_clipping_cancellation_and_limits_leave_original_intact() {
    let original = fixture(SampleDepth::U16, RgbSpace::ProPhoto, [513, 257]);
    let snapshot = original.clone();
    let color = DocumentColor::default();
    assert!(
        rasterize_source(&original, color, 1024, || true)
            .unwrap_err()
            .contains("cancelled")
    );
    let mut checks = 0;
    assert!(
        rasterize_source(&original, color, 4 * 1024 * 1024, || {
            checks += 1;
            checks == 4
        })
        .unwrap_err()
        .contains("cancelled")
    );
    assert_eq!(checks, 4);
    assert_eq!(original, snapshot);
    assert!(
        rasterize_source(&original, color, 1, || false)
            .unwrap_err()
            .contains("budget")
    );
    assert_eq!(original, snapshot);
    let (result, stats) = rasterize_source(&original, color, 4 * 1024 * 1024, || false).unwrap();
    assert_eq!(result.extent, original.extent);
    assert_eq!(
        result.interpretation.profile,
        ColorProfile::Builtin(RgbSpace::Srgb)
    );
    assert!(stats.clipped_channels > 0);
    assert_eq!(original, snapshot);
    assert!(
        rasterize_source(&result, color, 4 * 1024 * 1024, || false)
            .unwrap_err()
            .contains("already")
    );
}
