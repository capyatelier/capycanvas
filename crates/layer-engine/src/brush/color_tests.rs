use super::*;
use layer_core::BrushCurve;

fn point(x: f32) -> StrokePoint {
    StrokePoint {
        position: Point { x, y: 16. },
        pressure: 1.,
        tilt: [0.; 2],
        twist: 0.,
        elapsed_micros: (x * 1000.) as u32,
    }
}
fn adjusted_brush(space: RgbSpace, rgb: [f64; 3], delta: [f32; 3]) -> BrushSnapshot {
    let linear = rgb.map(|v| space.decode(v) as f32);
    BrushSnapshot {
        color_rgba_linear: [linear[0], linear[1], linear[2], 1. / 65535.],
        opacity: 0.37,
        mappings: [
            BrushTarget::Hue,
            BrushTarget::Saturation,
            BrushTarget::Lightness,
        ]
        .into_iter()
        .zip(delta)
        .map(|(target, output_bias)| BrushMapping {
            sensor: BrushSensor::Pressure,
            target,
            combine: BrushCombine::Replace,
            input_min: 0.,
            input_max: 1.,
            output_scale: 0.,
            output_bias,
            curve: BrushCurve::LINEAR,
        })
        .collect(),
        ..Default::default()
    }
}
fn emit(generator: &mut DabGenerator, brush: &BrushSnapshot) -> Vec<Dab> {
    generator.reset_for_stroke(StrokeId(91), brush);
    let mut dabs = Vec::new();
    generator.append(point(8.), brush, &mut dabs);
    dabs
}

#[test]
fn document_hsl_dynamics_match_independent_encoded_reference_vectors() {
    // Python colorsys.rgb_to_hls/hls_to_rgb references, independently evaluated
    // in Float64. The coordinate algorithm also matches CSS Color 4 §7 for
    // bounded sRGB; applying it in other encoded document spaces is our policy.
    let fixtures = [
        (
            [0.8, 0.2, 0.1],
            [0.15, 0.1, -0.1],
            [0.6308888888888887, 0.6572222222222222, 0.0427777777777778],
        ),
        (
            [0.01, 0.31, 0.97],
            [-0.4, -0.2, 0.07],
            [0.7057836734693879, 0.9030204081632653, 0.21697959183673476],
        ),
        ([0.5, 0.5, 0.5], [0.7, 0.3, 0.1], [0.528, 0.48, 0.72]),
        (
            [0.1, 0.8, 0.8],
            [0.4, -0.2, 0.3],
            [0.8944444444444444, 0.6055555555555556, 0.7788888888888889],
        ),
        (
            [0.92, 0.01, 0.2],
            [-0.9, 0.15, -0.3],
            [0.33, 0.1290989010989011, 0.0],
        ),
    ];
    for space in RgbSpace::ALL {
        for (input, delta, expected) in fixtures {
            let brush = adjusted_brush(space, input, delta);
            brush.validate().unwrap();
            for mode in 0..2 {
                let mut generator = DabGenerator::new(space);
                let dabs = if mode == 0 {
                    emit(&mut generator, &brush)
                } else {
                    generator.cursor_seed(StrokeId(91), &brush);
                    generator.cursor_contacts(point(8.), &brush)
                };
                assert!(!dabs.is_empty());
                for dab in dabs {
                    for c in 0..3 {
                        let linear = f64::from(dab.color_rgba_linear[c]);
                        assert!(
                            (linear - space.decode(expected[c])).abs() <= 2e-6,
                            "{space:?}, input {input:?}, delta {delta:?}, {c}: {linear}"
                        );
                        let code = (space.encode(linear) * 65535.).round();
                        assert!((code - (expected[c] * 65535.).round()).abs() <= 1.);
                    }
                    assert_eq!(
                        dab.color_rgba_linear[3],
                        brush.color_rgba_linear[3] * brush.opacity
                    );
                }
            }
        }
    }
}

#[test]
fn document_hue_rotation_preserves_extended_channels_and_dark_chroma() {
    for space in RgbSpace::ALL {
        for input in [
            [-0.2, 1.2, 0.4],
            [-2., -1., -0.5],
            [4., 2., 3.],
            [1e-12, 3e-12, 2e-12],
            [0., 1., 0.],
        ] {
            let brush = adjusted_brush(space, input, [0.5, 0., 0.]);
            let dabs = emit(&mut DabGenerator::new(space), &brush);
            let low = input.into_iter().fold(f64::INFINITY, f64::min);
            let high = input.into_iter().fold(f64::NEG_INFINITY, f64::max);
            let expected = input.map(|v| space.decode(low + high - v));
            for c in 0..3 {
                let actual = f64::from(dabs[0].color_rgba_linear[c]);
                let tolerance = 2e-6 * expected[c].abs().max(1e-20);
                assert!(
                    (actual - expected[c]).abs() <= tolerance,
                    "{space:?}, {input:?}, {c}: {actual} vs {}",
                    expected[c]
                );
            }
        }
    }
}

#[test]
fn color_dynamics_identity_preserves_all_integer16_codes_and_secondary_endpoints() {
    let brush = BrushSnapshot::default();
    let mut generator = DabGenerator::default();
    let point = generator.characterize(point(8.));
    let mut values = generator.evaluate(point, &brush).values;
    values.opacity = 1.;
    for space in RgbSpace::ALL {
        for code in 0..=65535 {
            let v = space.decode(f64::from(code) / 65535.) as f32;
            let color = [
                v,
                1. - v,
                -v,
                [0., 1. / 65535., 0.37, 1.][code as usize % 4],
            ];
            let actual = resolve_color(space, color, values, &brush, 321, 123);
            assert_eq!(actual.map(f32::to_bits), color.map(f32::to_bits));
        }
        let mut brush = brush.clone();
        brush.color_rgba_linear = [1e20, -1e20, 0.5, 0.37];
        brush.color_dynamics.secondary_color_rgba_linear = [0.4, -0.7, 1.3, 1. / 65535.];
        brush.validate().unwrap();
        values.secondary_color = 1.;
        assert_eq!(
            resolve_color(space, brush.color_rgba_linear, values, &brush, 321, 123),
            brush.color_dynamics.secondary_color_rgba_linear
        );
        values.secondary_color = 0.5;
        let actual = resolve_color(space, brush.color_rgba_linear, values, &brush, 321, 123);
        for c in 0..4 {
            let expected = ((f64::from(brush.color_rgba_linear[c])
                + f64::from(brush.color_dynamics.secondary_color_rgba_linear[c]))
                * 0.5) as f32;
            assert_eq!(actual[c], expected);
        }
        values.secondary_color = 0.;
    }
}

#[test]
fn document_color_survives_generator_reset_clone_and_stroke_correction() {
    for space in RgbSpace::ALL {
        let mut brush = adjusted_brush(space, [0.8, 0.15, 0.31], [0.07, 0.11, -0.03]);
        brush.color_dynamics.stamp_hue_jitter = 0.13;
        brush.color_dynamics.stroke_lightness_jitter = 0.09;
        brush.shape.count = 3;
        let mut generator = DabGenerator::new(space);
        let expected = emit(&mut generator, &brush);
        generator.reset();
        assert_eq!(emit(&mut generator, &brush), expected);
        let mut copy = generator.clone();
        let mut a = Vec::new();
        let mut b = Vec::new();
        generator.append(point(24.), &brush, &mut a);
        copy.append(point(24.), &brush, &mut b);
        assert_eq!(a, b);
        let stroke = Stroke::new(
            StrokeId(91),
            layer_core::LayerId(1),
            layer_core::StrokeTool::Brush,
            brush.clone(),
            vec![point(8.), point(24.)],
        )
        .unwrap();
        let mut corrected = Vec::new();
        DabGenerator::generate(&stroke, space, &mut corrected);
        assert_eq!(corrected, expected.into_iter().chain(a).collect::<Vec<_>>());
    }
}
