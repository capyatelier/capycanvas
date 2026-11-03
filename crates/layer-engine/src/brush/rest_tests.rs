use super::*;
use crate::{
    feedback::{PreviewTip, TipInput, InstantFeedbackConfig},
    test_support::point,
};
use layer_core::{default_brush, DefaultBrushPreset};

#[path = "../../tests/support/slow_stroke.rs"]
mod slow_stroke;

const SLOW_BRUSHES: [(DefaultBrushPreset, f32); 4] = [
    (DefaultBrushPreset::Pencil, 9.),
    (DefaultBrushPreset::Pencil, 36.),
    (DefaultBrushPreset::Charcoal, 9.),
    (DefaultBrushPreset::Charcoal, 36.),
];

const PAUSE_DEVIATIONS: [[f32; 3]; 16] = [
    [-1.0166115, -2.2812957, -0.0400000],
    [-1.0166115, -2.2812957, -0.0318126],
    [-1.0166115, -2.2812957, -0.0264074],
    [-0.6274779, -0.4134544, -0.0246615],
    [-0.6274779, -0.4134544, -0.0183874],
    [-0.6274779, -0.4134544, -0.0009601],
    [-0.6274779, -0.4134544, 0.0053379],
    [-0.6274779, -0.4134544, 0.0094994],
    [-0.6274779, -0.4134544, 0.0099618],
    [-0.6274779, -0.4134544, 0.0103923],
    [-0.6274779, -0.4134544, 0.0122737],
    [1.9408038, 2.1548273, 0.0162678],
    [1.9408038, 2.1548273, 0.0167780],
    [1.9408038, 2.1548273, 0.0195124],
    [1.9408038, 2.1548273, 0.0208279],
    [0.3064427, 1.5322135, 0.0213779],
];

fn brush() -> BrushSnapshot {
    let mut brush = default_brush(DefaultBrushPreset::Pencil);
    brush.diameter = 36.;
    brush.stabilization.strength = 0.;
    brush.stabilization.pressure_smoothing = 0.;
    brush.stabilization.pressure_fall_micros = 0;
    brush.stabilization.input_scale = 1.;
    brush
}

#[test]
fn slow_jitter_strokes_keep_clean_swept_distance() {
    for (preset, diameter) in SLOW_BRUSHES {
        for smoothed in [false, true] {
            let mut brush = default_brush(preset);
            brush.diameter = diameter;
            if !smoothed {
                brush.stabilization.strength = 0.;
            }
            let swept = |jitter| {
                let stroke = Stroke::new(
                    StrokeId(1),
                    layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(
                        0,
                    )),
                    layer_core::StrokeTool::Brush,
                    brush.clone(),
                    slow_stroke::points(5., 4., jitter, 0.01),
                )
                .unwrap();
                let mut dabs = Vec::new();
                DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut dabs);
                dabs.iter()
                    .filter(|dab| !dab.is_completion())
                    .map(|dab| dab.motion[0].hypot(dab.motion[1]))
                    .sum::<f32>()
            };
            let actual = swept(0.15);
            let clean = swept(0.);
            assert!(
                actual <= 1.5 * clean,
                "{preset:?} diameter={diameter} smoothed={smoothed}: {actual} versus clean {clean}"
            );
        }
    }
}

#[test]
fn accepted_slow_samples_never_rewind_committed_contacts() {
    for (preset, diameter) in SLOW_BRUSHES {
        for smoothed in [false, true] {
            let mut brush = default_brush(preset);
            brush.diameter = diameter;
            if !smoothed {
                brush.stabilization.strength = 0.;
            }
            let points = slow_stroke::points(5., 4., 0.15, 0.01);
            let mut generator = DabGenerator::default();
            generator.reset_for_stroke(StrokeId(1), &brush);
            let mut dabs = Vec::new();
            let mut distance = 0.;
            for &point in &points {
                generator.append(point, &brush, &mut dabs);
                if let Some(last) = generator.last {
                    assert!(last.stroke_distance >= distance);
                    distance = last.stroke_distance;
                }
            }
            generator.finish(&brush, &mut dabs);
            assert!(generator.last.unwrap().stroke_distance >= distance);
            let mut previous = points[0].position;
            for dab in dabs.iter().filter(|dab| !dab.is_completion()) {
                assert_eq!(
                    dab.motion,
                    [dab.center.x - previous.x, dab.center.y - previous.y],
                    "{preset:?} diameter={diameter} smoothed={smoothed}"
                );
                previous = dab.center;
            }
        }
    }
}

#[test]
fn slow_clean_strokes_reach_the_last_sample() {
    for (preset, diameter) in SLOW_BRUSHES {
        let mut brush = default_brush(preset);
        brush.diameter = diameter;
        let points = slow_stroke::points(1.5, 6., 0., 0.);
        let last = points.last().unwrap().position;
        let epsilon = 3. / brush.stabilization.input_scale;
        let stroke = Stroke::new(
            StrokeId(1),
            layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(0)),
            layer_core::StrokeTool::Brush,
            brush,
            points,
        )
        .unwrap();
        let mut dabs = Vec::new();
        DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut dabs);
        let end = dabs
            .iter()
            .rev()
            .find(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
            .unwrap();
        assert!(
            (end.center.x - last.x).hypot(end.center.y - last.y) <= epsilon,
            "{preset:?} diameter={diameter}: {:?} versus {last:?}",
            end.center
        );
    }
}

fn stopped(brush: &BrushSnapshot) -> (DabGenerator, Vec<Dab>) {
    let mut generator = DabGenerator::default();
    let mut output = Vec::new();
    for i in 0..=40 {
        generator.append(
            point(20. + i as f32 * 5., 100., 0.6, i * 4_000),
            brush,
            &mut output,
        );
    }
    assert!(!generator.frozen());
    for i in 1..=30 {
        generator.append(
            point(220., 100., 0.6, 160_000 + i * 4_000),
            brush,
            &mut output,
        );
    }
    assert!(generator.frozen());
    (generator, output)
}

pub(super) fn lift_now(
    generator: &DabGenerator,
    brush: &BrushSnapshot,
    mut dabs: Vec<Dab>,
) -> Vec<Dab> {
    let mut lift = generator.clone();
    lift.finish_lift(brush, &mut dabs);
    dabs
}

#[test]
fn frozen_dabs_ignore_recorded_pause_deviations_and_pressure_noise() {
    let brush = brush();
    let (mut generator, mut committed) = stopped(&brush);
    let mut expected = Vec::new();
    expected.extend(lift_now(&generator, &brush, Vec::new()));
    assert!(!expected.is_empty());
    let count = committed.len();
    for i in 0..160 {
        let [dx, dy, dp] = PAUSE_DEVIATIONS[i % PAUSE_DEVIATIONS.len()];
        generator.append(
            point(220. + dx, 100. + dy, 0.6 + dp, 284_000 + i as u32 * 4_000),
            &brush,
            &mut committed,
        );
        let mut preview = Vec::new();
        preview.extend(lift_now(&generator, &brush, Vec::new()));
        assert_eq!(preview, expected, "pause sample {i}");
        assert_eq!(committed.len(), count);
    }
    let before = committed.len();
    generator.append(point(220., 100., 0., 1_000_000), &brush, &mut committed);
    generator.finish(&brush, &mut committed);
    assert_eq!(
        committed[before..].to_vec(),
        expected
            .into_iter()
            .map(|dab| dab.with_preview_tag(layer_render::DabPreview::Shared))
            .collect::<Vec<_>>()
    );
}

#[test]
fn unfreeze_feeds_the_first_sample_at_epsilon() {
    for scale in [0.5, 1., 2.] {
        let mut brush = brush();
        brush.stabilization.input_scale = scale;
        let epsilon = 3. / scale;
        let mut generator = DabGenerator::default();
        let mut output = Vec::new();
        generator.append(point(0., 0., 0.6, 0), &brush, &mut output);
        generator.append(point(0.99 * epsilon, 0., 0.6, 4_000), &brush, &mut output);
        assert!(generator.frozen());
        assert_eq!(generator.modeled_position(), Some(Point::default()));
        generator.append(point(epsilon, 0., 0.6, 8_000), &brush, &mut output);
        assert!(!generator.frozen());
        assert_eq!(
            generator.modeled_position(),
            Some(Point { x: epsilon, y: 0. })
        );
    }
}

#[test]
fn frozen_tap_raises_only_pressure_in_steps_and_commits_its_preview() {
    let brush = brush();
    let mut generator = DabGenerator::default();
    let mut output = Vec::new();
    let opening = StrokePoint {
        tilt: [0.4, 0.2],
        twist: 0.7,
        ..point(100., 100., 0.2, 0)
    };
    generator.append(opening, &brush, &mut output);
    for (i, pressure) in [0.24, 0.3, 0.34, 0.4, 0.44, 0.39, 0.2]
        .into_iter()
        .enumerate()
    {
        generator.append(
            StrokePoint {
                tilt: [0.8, -0.4],
                twist: 1.4,
                ..point(101., 100., pressure, (i as u32 + 1) * 4_000)
            },
            &brush,
            &mut output,
        );
        let expected = if i == 0 {
            0.2
        } else if i < 3 {
            0.3
        } else {
            0.4
        };
        let pose = generator.rest.unwrap().pose.point;
        assert_eq!(pose.pressure, expected);
        assert_eq!(pose.position, opening.position);
        assert_eq!(pose.tilt, opening.tilt);
        assert_eq!(pose.twist, opening.twist);
        assert!(output.is_empty());
    }
    let mut preview = Vec::new();
    preview.extend(lift_now(&generator, &brush, Vec::new()));
    generator.finish(&brush, &mut output);
    assert_eq!(output, preview);
    assert!(
        preview
            .iter()
            .all(|dab| dab.preview_tag() == DabPreview::Shared)
    );
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].motion, [0.; 2]);
}

#[test]
fn midstroke_rest_freezes_the_whole_pose_and_the_next_segments_previous_pose() {
    let brush = brush();
    let (mut generator, mut output) = stopped(&brush);
    let old = generator.last_contact.unwrap();
    generator.append(
        StrokePoint {
            tilt: [1., 0.5],
            twist: 1.,
            ..point(221., 100., 0.8, 300_000)
        },
        &brush,
        &mut output,
    );
    let raised = generator.last_contact.unwrap();
    assert_eq!(raised, old);
    assert_eq!(raised.rotation, old.rotation);
    let mut preview = Vec::new();
    preview.extend(lift_now(&generator, &brush, Vec::new()));
    assert_eq!(preview.last().unwrap().contact, raised.contact);
    generator.append(point(230., 100., 0.8, 304_000), &brush, &mut output);
    assert!(!generator.frozen());
    generator.flush_contact(&brush, &mut output);
    assert_eq!(
        output
            .iter()
            .rev()
            .find(|dab| !dab.is_completion())
            .unwrap()
            .previous_contact,
        raised.contact
    );
}

#[test]
fn smoothing_settles_before_freezing_and_frozen_preview_has_no_continuation() {
    let mut brush = brush();
    brush.stabilization.strength = 0.5;
    let mut generator = DabGenerator::default();
    let mut output = Vec::new();
    let mut real = Vec::new();
    for i in 0..=80 {
        let p = point(20. + i as f32 * 5., 100., 0.6, i * 4_000);
        real.push(p);
        generator.append(p, &brush, &mut output);
    }
    assert!(!generator.frozen());
    for i in 1..=100 {
        let p = point(420., 100., 0.6, 320_000 + i * 4_000);
        real.push(p);
        generator.append(p, &brush, &mut output);
        if i <= 10 {
            assert!(!generator.frozen());
        }
    }
    assert!(generator.frozen());
    let mut expected = Vec::new();
    expected.extend(lift_now(&generator, &brush, Vec::new()));
    let mut tip = PreviewTip::default();
    for now in [720_000, 800_000, 1_000_000] {
        let mut preview = Vec::new();
        let source = tip.build(
            &generator,
            TipInput {
                real: &real,
                pending: &[],
                platform: &[],
                brush: &brush,
                feedback: InstantFeedbackConfig::default(),
                transform: [1., 0., 0., 1., 0., 0.],
                now,
                requested: now + 16_000,
                ending: None,
            },
            0,
            &mut preview,
        );
        assert_eq!(source, None);
        assert_eq!(preview, expected);
        assert!(
            preview
                .iter()
                .all(|dab| dab.preview_tag() == DabPreview::Shared)
        );
    }
}

#[test]
fn moving_tip_preserves_the_control_prediction_endpoint() {
    let mut flushed = 0;
    for strength in [0., 0.5, 1.] {
        for platform in [false, true] {
            let mut brush = brush();
            brush.stabilization.strength = strength;
            let mut generator = DabGenerator::default();
            let mut real = Vec::new();
            for i in 0..=120 {
                let p = point(
                    20. + i as f32 * 5.,
                    100. + (i as f32 * 0.08).sin() * 20.,
                    0.6,
                    i * 4_000,
                );
                real.push(p);
                generator.append(p, &brush, &mut Vec::new());
            }
            assert!(!generator.frozen());
            let latest = *real.last().unwrap();
            let predicted = [StrokePoint {
                position: Point {
                    x: latest.position.x + 20.,
                    y: latest.position.y,
                },
                elapsed_micros: 496_000,
                ..latest
            }];
            let mut control = brush.clone();
            control.rendering.accumulation = layer_core::BrushAccumulation::Uniform;
            let build = |brush: &BrushSnapshot| {
                let mut output = Vec::new();
                let source = PreviewTip::default().build(
                    &generator,
                    TipInput {
                        real: &real,
                        pending: &[],
                        platform: if platform { &predicted } else { &[] },
                        brush,
                        feedback: InstantFeedbackConfig {
                            use_platform_prediction: platform,
                            ..Default::default()
                        },
                        transform: [1., 0., 0., 1., 0., 0.],
                        now: 480_000,
                        requested: 496_000,
                        ending: None,
                    },
                    0,
                    &mut output,
                );
                (
                    source,
                    output
                        .into_iter()
                        .filter(|dab| !dab.is_completion())
                        .map(|dab| dab.with_preview_tag(DabPreview::Shared))
                        .collect::<Vec<_>>(),
                )
            };
            let (source, candidate) = build(&brush);
            let (control_source, control) = build(&control);
            let mut real = Vec::new();
            generator.preview().flush_contact(&brush, &mut real);
            if let Some(real) = real.iter().find(|dab| !dab.is_completion()) {
                let actual = candidate
                    .iter()
                    .find(|dab| dab.center == real.center)
                    .expect("the real modeled endpoint precedes prediction");
                assert_eq!(
                    actual, real,
                    "prediction preserves the real endpoint radius and pose"
                );
                flushed += 1;
            }
            assert!(!candidate.is_empty() && !control.is_empty());
            assert_eq!(
                source, control_source,
                "strength={strength}, platform={platform}"
            );
            let endpoint = |dab: &Dab| {
                (
                    dab.center,
                    dab.radii,
                    dab.rotation,
                    dab.color_rgba_linear,
                    dab.flow,
                    dab.hardness,
                    dab.material,
                    dab.contact,
                )
            };
            assert_eq!(
                endpoint(candidate.last().unwrap()),
                endpoint(control.last().unwrap()),
                "strength={strength}, platform={platform}"
            );
        }
    }
    assert!(flushed > 0);
}

#[test]
fn odd_timestamps_and_a_mouse_gap_do_not_create_rest_samples() {
    let brush = brush();
    let mut generator = DabGenerator::default();
    let mut output = Vec::new();
    for (x, t) in [
        (0., 0),
        (20., 4_000),
        (40., 4_000),
        (60., 3_000),
        (80., 64_000),
    ] {
        generator.append(point(x, 0., 0.6, t), &brush, &mut output);
        if x > 0. {
            assert!(!generator.frozen());
        }
    }
    let mut preview = Vec::new();
    generator.preview().flush_contact(&brush, &mut preview);
    let end = preview.last().unwrap().center;
    generator.finish(&brush, &mut output);
    assert_eq!(output.last().unwrap().center, end);
}

#[test]
fn slow_frozen_lifts_match_their_preview_at_both_scales() {
    for scale in [1., 0.1] {
        for diameter in [36., 14.] {
            let mut brush = brush();
            brush.diameter = diameter;
            brush.stabilization.input_scale = scale;
            let mut generator = DabGenerator::default();
            let mut output = Vec::new();
            generator.append(point(0., 0., 0.6, 0), &brush, &mut output);
            generator.append(point(10. / scale, 0., 0.6, 4_000), &brush, &mut output);
            for i in 1..=100 {
                generator.append(
                    point((10. + i as f32 * 0.02) / scale, 0., 0.6, 4_000 + i * 4_000),
                    &brush,
                    &mut output,
                );
            }
            assert!(generator.frozen());
            let expected = lift_now(&generator, &brush, Vec::new());
            let before = output.len();
            generator.finish(&brush, &mut output);
            assert_eq!(&output[before..], expected);
            let end = output.last().unwrap().center;
            assert!((end.x - 12. / scale).hypot(end.y) <= 3. / scale);
        }
    }
}

#[test]
fn small_brush_anchor_steps_are_limited_by_the_current_minor_radius() {
    let mut brush = brush();
    brush.diameter = 14.;
    let mut generator = DabGenerator::default();
    let mut output = Vec::new();
    generator.append(point(0., 0., 0.6, 0), &brush, &mut output);
    generator.append(point(4., 0., 0.6, 4_000), &brush, &mut output);
    let anchor = generator.anchor.clone().unwrap();
    let step = 0.25 * generator.anchor_radius(&anchor, &brush);
    assert!(step < 1.5);
    generator.append(point(4. + 0.99 * step, 0., 0.6, 8_000), &brush, &mut output);
    assert_eq!(
        generator.anchor.clone().unwrap().pose.position(),
        anchor.pose.position()
    );
    generator.append(point(4. + step, 0., 0.6, 12_000), &brush, &mut output);
    assert_eq!(
        generator.anchor.clone().unwrap().pose.position().x,
        4. + step
    );
}

#[test]
fn dry_uniform_taps_keep_their_deferred_peak_pressure_imprint() {
    let mut brush = brush();
    brush.rendering.accumulation = layer_core::BrushAccumulation::Uniform;
    let mut generator = DabGenerator::default();
    let mut output = Vec::new();
    for (pressure, time) in [(0.2, 0), (0.8, 4_000), (0.1, 8_000)] {
        generator.append(point(100., 100., pressure, time), &brush, &mut output);
        assert!(output.is_empty());
        assert!(!generator.frozen());
    }
    generator.finish(&brush, &mut output);
    let mut expected = Vec::new();
    let mut reference = DabGenerator::default();
    reference.append(point(100., 100., 0.8, 0), &brush, &mut expected);
    reference.finish(&brush, &mut expected);
    assert_eq!(output, expected);
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].motion, [0.; 2]);
    let mut drag = DabGenerator::default();
    let mut body = Vec::new();
    drag.append(point(0., 0., 0.6, 0), &brush, &mut body);
    drag.append(point(0.99, 0., 0.6, 4_000), &brush, &mut body);
    assert!(body.is_empty());
    drag.append(point(1.01, 0., 0.6, 8_000), &brush, &mut body);
    assert!(drag.rest.is_none());
    drag.finish(&brush, &mut body);
    assert!(body.iter().all(|dab| dab.motion != [0.; 2]));
}

#[test]
fn unfreezing_keeps_the_opening_contact_at_the_raised_rest_pose() {
    for scale in [0.5, 1., 2.] {
        let mut brush = brush();
        brush.stabilization.input_scale = scale;
        let mut generator = DabGenerator::default();
        let mut output = Vec::new();
        let opening = StrokePoint {
            tilt: [0.4, 0.2],
            twist: 0.7,
            ..point(100., 100., 0.2, 0)
        };
        generator.append(opening, &brush, &mut output);
        generator.append(
            StrokePoint {
                tilt: [0.9, 0.8],
                twist: 1.4,
                ..point(100., 100., 0.8, 4_000)
            },
            &brush,
            &mut output,
        );
        let mut frozen = Vec::new();
        frozen.extend(lift_now(&generator, &brush, Vec::new()));
        let expected = frozen[0].with_preview_tag(layer_render::DabPreview::Shared);
        generator.append(
            StrokePoint {
                position: Point {
                    x: 100. + 4. / scale,
                    y: 100.,
                },
                elapsed_micros: 8_000,
                ..opening
            },
            &brush,
            &mut output,
        );
        let start = output
            .iter()
            .find(|dab| dab.is_completion() && dab.previous_contact[..2] == [0.; 2])
            .unwrap();
        assert_eq!(start.center, expected.center);
        assert_eq!(start.radii, expected.radii);
        assert_eq!(start.rotation, expected.rotation);
        assert_eq!(start.contact, expected.contact);
        let body = output.iter().find(|dab| !dab.is_completion()).unwrap();
        assert_eq!(body.previous_contact, expected.contact);
    }
}

fn moving_gap_stroke(brush: &BrushSnapshot) -> (DabGenerator, Vec<StrokePoint>, Vec<Dab>) {
    let mut generator = DabGenerator::default();
    generator.reset_for_stroke(StrokeId(1), brush);
    let real: Vec<_> = (0..=12)
        .map(|i| point(20. + i as f32 * 5., 100., 0.6, i * 4_000))
        .collect();
    let mut output = Vec::new();
    for &sample in &real {
        generator.append(sample, brush, &mut output);
    }
    assert!(!generator.frozen());
    (generator, real, output)
}

#[test]
fn gap_freeze_uses_the_latest_modeled_time_at_the_exact_window() {
    let brush = brush();
    let (mut generator, _, _) = moving_gap_stroke(&brush);
    generator.append(point(80.4, 100., 0.9, 52_000), &brush, &mut Vec::new());
    let modeled = generator.stabilized_input;
    let tip = (
        generator.last.map(|p| (p.point, p.stroke_distance)),
        generator.last_contact,
    );
    assert!(!generator.freeze_at(151_999, &brush));
    assert!(!generator.frozen());
    assert!(generator.freeze_at(152_000, &brush));
    assert!(generator.frozen());
    assert_eq!(generator.stabilized_input, modeled);
    assert_eq!(
        (
            generator.last.map(|p| (p.point, p.stroke_distance)),
            generator.last_contact
        ),
        tip
    );
    assert!(generator.freeze_at(152_000, &brush));
}

#[test]
fn gap_freeze_preserves_an_already_frozen_raised_tap_pose() {
    let brush = brush();
    let mut generator = DabGenerator::default();
    generator.append(point(100., 100., 0.2, 0), &brush, &mut Vec::new());
    generator.append(point(100., 100., 0.8, 4_000), &brush, &mut Vec::new());
    let raised = generator.rest.unwrap().pose;
    assert!(raised.point.pressure > generator.anchor.clone().unwrap().pose.point.pressure);
    assert!(!generator.freeze_at(103_999, &brush));
    assert!(generator.freeze_at(104_000, &brush));
    let current = generator.rest.unwrap().pose;
    assert_eq!(
        (
            current.point,
            current.speed,
            current.direction_turns,
            current.stroke_distance
        ),
        (
            raised.point,
            raised.speed,
            raised.direction_turns,
            raised.stroke_distance
        )
    );
}

#[test]
fn pending_real_input_prevents_a_premature_preview_gap_freeze() {
    let brush = brush();
    let (generator, mut real, _) = moving_gap_stroke(&brush);
    let pending = point(80.4, 100., 0.6, 90_000);
    real.push(pending);
    let mut tip = PreviewTip::default();
    let mut preview = Vec::new();
    tip.build(
        &generator,
        TipInput {
            real: &real,
            pending: &[pending],
            platform: &[],
            brush: &brush,
            feedback: InstantFeedbackConfig::default(),
            transform: [1., 0., 0., 1., 0., 0.],
            now: 160_000,
            requested: 300_000,
            ending: None,
        },
        0,
        &mut preview,
    );
    assert!(
        preview
            .iter()
            .any(|dab| !dab.is_completion() && dab.center == pending.position)
    );
    let mut reference = generator.clone();
    let mut expected = Vec::new();
    reference.append(pending, &brush, &mut expected);
    assert!(reference.freeze_at(190_000, &brush));
    let mut frozen = Vec::new();
    frozen.extend(lift_now(&reference, &brush, expected));
    preview.clear();
    assert_eq!(
        tip.build(
            &generator,
            TipInput {
                real: &real,
                pending: &[pending],
                platform: &[],
                brush: &brush,
                feedback: InstantFeedbackConfig::default(),
                transform: [1., 0., 0., 1., 0., 0.],
                now: 190_000,
                requested: 300_000,
                ending: None
            },
            0,
            &mut preview
        ),
        None
    );
    assert_eq!(preview, frozen);
    assert!(!generator.frozen());
    assert_eq!(generator.stabilized_input.unwrap().elapsed_micros, 48_000);
}

#[test]
fn pending_up_preview_keeps_the_stored_up_time_when_rendered_late() {
    let brush = brush();
    let (generator, mut real, _) = moving_gap_stroke(&brush);
    let up = point(80.4, 100., 0.9, 52_000);
    real.push(up);
    let build = |now| {
        let mut preview = Vec::new();
        PreviewTip::default().build(
            &generator,
            TipInput {
                real: &real,
                pending: &[up],
                platform: &[],
                brush: &brush,
                feedback: InstantFeedbackConfig::default(),
                transform: [1., 0., 0., 1., 0., 0.],
                now,
                requested: now,
                ending: Some(now),
            },
            0,
            &mut preview,
        );
        preview
    };
    let immediate = build(up.elapsed_micros);
    assert!(
        immediate
            .iter()
            .any(|dab| !dab.is_completion() && dab.center == up.position)
    );
    assert_eq!(build(3_000_000), immediate);
}

#[test]
fn ignored_late_up_pose_does_not_change_tapered_replay() {
    let mut brush = brush();
    brush.taper.end_distance_diameters = 2.;
    brush.taper.end_size = 0.;
    brush.taper.end_opacity = 0.;
    let (_, real, _) = moving_gap_stroke(&brush);
    let latest = *real.last().unwrap();
    let render = |position| {
        let mut points = real.clone();
        points.push(StrokePoint {
            position,
            elapsed_micros: 3_000_000,
            ..latest
        });
        let stroke = Stroke::new(
            StrokeId(1),
            layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(0)),
            layer_core::StrokeTool::Brush,
            brush.clone(),
            points,
        )
        .unwrap();
        let mut output = Vec::new();
        DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut output);
        output
    };
    assert_eq!(render(Point { x: 500., y: 20. }), render(latest.position));
}
