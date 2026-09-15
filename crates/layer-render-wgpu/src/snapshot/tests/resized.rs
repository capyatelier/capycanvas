use super::*;

// Integrate the already independently rendered composition over target pixel
// rectangles. No production resampling kernel is used by this oracle.
fn area(source: [u32; 2], target: [u32; 2], pixels: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut output = Vec::new();
    let ratio = [
        f64::from(source[0]) / f64::from(target[0]),
        f64::from(source[1]) / f64::from(target[1]),
    ];
    for y in 0..target[1] {
        let top = f64::from(y) * ratio[1];
        let bottom = f64::from(y + 1) * ratio[1];
        for x in 0..target[0] {
            let left = f64::from(x) * ratio[0];
            let right = f64::from(x + 1) * ratio[0];
            let mut sum = [0.; 4];
            for sy in top.floor() as u32..(bottom.ceil() as u32).min(source[1]) {
                for sx in left.floor() as u32..(right.ceil() as u32).min(source[0]) {
                    let weight = (bottom.min(f64::from(sy + 1)) - top.max(f64::from(sy)))
                        * (right.min(f64::from(sx + 1)) - left.max(f64::from(sx)))
                        / (ratio[0] * ratio[1]);
                    for c in 0..4 {
                        sum[c] += f64::from(pixels[(sy * source[0] + sx) as usize][c]) * weight;
                    }
                }
            }
            output.push(sum.map(|v| v as f32));
        }
    }
    output
}

#[test]
fn snapshot_resized_composition_matches_area_before_profile_quantization_and_matte() {
    use layer_core::color::{OutputDither, OutputEncoding};
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
        let project = rich_project(
            DocumentColor {
                space: RgbSpace::ProPhoto,
                depth,
            },
            2,
        );
        let original = project.clone();
        let (_gpu, pixels) = frame(&project);
        let source_extent = [project.document.width, project.document.height];
        let extent = [137, 83];
        let resized = area(source_extent, extent, &pixels);
        let mut renderer =
            SnapshotRenderer::new(project.clone(), [0.; 4], 0., Default::default()).unwrap();
        renderer.set_output_extent(extent).unwrap();
        assert_eq!(renderer.extent(), source_extent);
        assert!(renderer.set_output_extent([0, 1]).is_err());
        for (profile, channels, matte) in [
            (
                ColorProfile::Builtin(RgbSpace::DisplayP3),
                SourceChannels::Rgba,
                None,
            ),
            (
                layer_color::gray_profile(RgbSpace::Srgb).unwrap(),
                SourceChannels::Gray,
                Some([0.25, 0.5, 0.75]),
            ),
        ] {
            let target = SourceInterpretation {
                channels,
                depth,
                profile,
                profile_assumed: false,
            };
            let options = OutputEncoding {
                dither: if depth == IntegerDepth::U8 {
                    OutputDither::Stochastic8
                } else {
                    OutputDither::None
                },
                ..Default::default()
            };
            let encoder =
                layer_color::WorkingEncoder::new(RgbSpace::ProPhoto, &target, options).unwrap();
            let mut expected = vec![0; resized.len() * target.pixel_bytes()];
            for (y, (input, output)) in resized
                .chunks_exact(extent[0] as usize)
                .zip(expected.chunks_exact_mut(extent[0] as usize * target.pixel_bytes()))
                .enumerate()
            {
                encoder
                    .encode_premultiplied(input, output, matte, [0, y as u32])
                    .unwrap();
            }
            for tiff in [false, true] {
                let mut file = Cursor::new(Vec::new());
                if tiff {
                    renderer.write_tiff(&mut file, &target, options, matte)
                } else {
                    renderer.write_png(&mut file, &target, options, matte)
                }
                .unwrap();
                let decoded = decode(file.into_inner());
                assert_eq!(decoded.extent, extent);
                assert_eq!(decoded.interpretation.depth, depth);
                assert_eq!(decoded.interpretation.channels, channels);
                assert_eq!(
                    layer_color::profile_bytes(&decoded.interpretation.profile).unwrap(),
                    layer_color::profile_bytes(&target.profile).unwrap()
                );
                let actual = raw_rows(&decoded);
                let code = |v: &[u8]| {
                    if depth == IntegerDepth::U8 {
                        u16::from(v[0])
                    } else {
                        u16::from_le_bytes(v.try_into().unwrap())
                    }
                };
                for (a, b) in actual
                    .chunks_exact(depth.bytes())
                    .zip(expected.chunks_exact(depth.bytes()))
                {
                    assert!(
                        code(a).abs_diff(code(b)) <= 1,
                        "{depth:?} {channels:?} tiff={tiff}: {} vs {}",
                        code(a),
                        code(b)
                    );
                }
                assert_eq!(renderer.control().output_rows(), extent[1]);
            }
        }
        assert_eq!(project, original);
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth,
            profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
            profile_assumed: false,
        };
        renderer.limits.planned_pixel_bytes = 1;
        assert!(
            renderer
                .write_png(&mut Vec::new(), &target, Default::default(), None)
                .is_err()
        );
        assert!(renderer.renderer.composite_texture.is_none());
        renderer.control().cancel();
        let mut output = Vec::new();
        assert!(
            renderer
                .write_png(&mut output, &target, Default::default(), None)
                .is_err()
        );
        assert!(output.is_empty());
    }
}

#[test]
fn snapshot_enlarged_jpeg_matches_profiled_png_and_reset_restores_exact_identity() {
    let project = source_project(
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: IntegerDepth::U16,
        },
        [33, 17],
    );
    let source = project.document.layers[0].source.as_ref().unwrap().clone();
    let mut renderer = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
    let extent = [97, 50];
    renderer.set_output_extent(extent).unwrap();
    let target = SourceInterpretation {
        channels: SourceChannels::Rgb,
        depth: IntegerDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    };
    let mut png = Vec::new();
    let mut jpeg = Vec::new();
    let a = renderer
        .write_png(
            &mut png,
            &target,
            Default::default(),
            Some([0.25, 0.5, 0.75]),
        )
        .unwrap();
    let b = renderer
        .write_jpeg(
            &mut jpeg,
            &target,
            Default::default(),
            [0.25, 0.5, 0.75],
            100,
        )
        .unwrap();
    assert_eq!(a, b);
    let png = decode(png);
    let jpeg = decode(jpeg);
    assert_eq!(png.extent, extent);
    assert_eq!(jpeg.extent, extent);
    assert_eq!(
        layer_color::profile_bytes(&jpeg.interpretation.profile).unwrap(),
        layer_color::profile_bytes(&target.profile).unwrap()
    );
    let max = raw_rows(&png)
        .into_iter()
        .zip(raw_rows(&jpeg))
        .map(|(a, b)| a.abs_diff(b))
        .max()
        .unwrap();
    assert!(max <= 4, "quality 100 JPEG differs by {max} codes");
    renderer.set_output_extent(source.extent).unwrap();
    let mut identity = Vec::new();
    renderer
        .write_png(
            &mut identity,
            &source.interpretation,
            Default::default(),
            None,
        )
        .unwrap();
    assert_eq!(raw_rows(&decode(identity)), raw_rows(&source));
}

#[test]
fn output_preview_matches_the_delivered_samples_after_profile_depth_resize_and_matte() {
    use layer_core::color::{OutputDither, OutputEncoding};
    let project = source_project(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: IntegerDepth::U16,
        },
        [513, 35],
    );
    let original = project.clone();
    let mut renderer =
        SnapshotRenderer::new(project.clone(), [0.; 4], 0., Default::default()).unwrap();
    let full = renderer.read_region([0, 0, 513, 35]).unwrap();
    for view in [RgbSpace::Srgb, RgbSpace::DisplayP3] {
        let before = renderer.preview_document([73, 41], view).unwrap();
        let expected = area([513, 35], before.extent, &full);
        let matrix = project.document.color.space.linear_transform(view);
        for (actual, expected) in before.pixels.iter().zip(expected) {
            for c in 0..3 {
                let value: f64 = (0..3).map(|k| matrix[c][k] * f64::from(expected[k])).sum();
                assert!((f64::from(actual[c]) - value).abs() < 2e-7);
            }
            assert!((actual[3] - expected[3]).abs() < 2e-7);
        }
        let mut cases = vec![
            (
                [513, 35],
                ColorProfile::Builtin(RgbSpace::ProPhoto),
                SourceChannels::Rgba,
                IntegerDepth::U16,
                None,
            ),
            (
                [257, 19],
                ColorProfile::Builtin(RgbSpace::Srgb),
                SourceChannels::Rgba,
                IntegerDepth::U8,
                None,
            ),
            (
                [257, 19],
                ColorProfile::Builtin(RgbSpace::DisplayP3),
                SourceChannels::Rgb,
                IntegerDepth::U8,
                Some([0.25, 0.5, 0.75]),
            ),
            // Exercise the encoder's canonical gray ICC, not a guessed RGB tag.
            (
                [769, 53],
                ColorProfile::Builtin(RgbSpace::Srgb),
                SourceChannels::GrayAlpha,
                IntegerDepth::U16,
                None,
            ),
        ];
        if let Some(path) = std::env::var_os("LAYER_TEST_CMYK_PROFILE") {
            let bytes = std::fs::read(path).unwrap();
            eprintln!("Output preview CMYK profile: {} bytes", bytes.len());
            let profile = ColorProfile::Icc(bytes.into());
            for (depth, matte) in [(IntegerDepth::U8, [1.; 3]), (IntegerDepth::U16, [0.; 3])] {
                cases.push((
                    [257, 19],
                    profile.clone(),
                    SourceChannels::Cmyk,
                    depth,
                    Some(matte),
                ));
            }
        }
        for (extent, profile, channels, depth, matte) in cases {
            renderer.set_output_extent(extent).unwrap();
            let target = SourceInterpretation {
                channels,
                depth,
                profile,
                profile_assumed: false,
            };
            let options = OutputEncoding {
                dither: if depth == IntegerDepth::U8 {
                    OutputDither::Stochastic8
                } else {
                    OutputDither::None
                },
                ..Default::default()
            };
            let mut file = Cursor::new(Vec::new());
            let expected_stats = if channels == SourceChannels::Cmyk {
                renderer.write_tiff(&mut file, &target, options, matte)
            } else {
                renderer.write_png(&mut file, &target, options, matte)
            }
            .unwrap();
            let source = decode(file.into_inner());
            assert_eq!(source.extent, extent);
            let decoder =
                layer_color::WorkingDecoder::new(&source.interpretation, view, Default::default())
                    .unwrap();
            let mut pixels = vec![[0.; 4]; (extent[0] * extent[1]) as usize];
            decoder
                .decode_pixels(&raw_rows(&source), &mut pixels)
                .unwrap();
            for pixel in &mut pixels {
                for c in 0..3 {
                    pixel[c] *= pixel[3];
                }
            }
            let (preview, stats) = renderer
                .preview_output([73, 41], view, &target, options, matte)
                .unwrap();
            assert_eq!(preview.space, view);
            assert_eq!(stats, expected_stats);
            assert_eq!(renderer.control().output_rows(), extent[1]);
            let expected = area(extent, preview.extent, &pixels);
            for (actual, expected) in preview.pixels.iter().zip(expected) {
                for c in 0..4 {
                    // Embedded ICC round-trip uses LCMS; generated built-in RGB
                    // coordinates in the direct preview use exact transfer math.
                    assert!(
                        (actual[c] - expected[c]).abs() < 5e-4,
                        "{view:?} {channels:?} {depth:?}: {actual:?} vs {expected:?}"
                    );
                }
            }
        }
    }
    assert_eq!(project, original);
    assert!(renderer.preview_document([0, 1], RgbSpace::Srgb).is_err());
    assert!(
        renderer
            .preview_document([1025, 1], RgbSpace::Srgb)
            .is_err()
    );
    renderer.control().cancel();
    assert!(renderer.preview_document([73, 41], RgbSpace::Srgb).is_err());
    let target = original.document.layers[0]
        .source
        .as_ref()
        .unwrap()
        .interpretation
        .clone();
    assert!(
        renderer
            .preview_output([73, 41], RgbSpace::Srgb, &target, Default::default(), None)
            .is_err()
    );
}
