//! Contact material invariants, exercised through real input and GPU painting.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;

fn render(
    brush: BrushSnapshot,
    pressure: f32,
    tilt: [f32; 2],
    cadence: usize,
    passes: usize,
    feedback: bool,
) -> Vec<u8> {
    let gpu = WgpuRasterizer::new_headless().expect("physical GPU required");
    let (mut input, consumer) = input_queue(256);
    let mut engine = CanvasEngine::new(
        gpu,
        Document::new("contact-invariants", 256, 256),
        consumer,
        ViewState {
            width_px: 256,
            height_px: 256,
            document_to_surface: Affine::IDENTITY.0,
            background_rgba_linear: [1.; 4],
        },
        ViewTransform::IDENTITY,
    )
    .unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig {
            enabled: feedback,
            ..Default::default()
        })
        .unwrap();
    engine.set_brush(brush).unwrap();
    engine.render_frame_at(0).unwrap();
    let mut time = 1_000_000_000;
    for _ in 0..passes {
        for i in 0..65 {
            time += 4_000_000;
            input
                .push(PenEvent {
                    device_id: 1,
                    sequence: time,
                    timestamp_ns: time,
                    view_revision: 0,
                    surface_position: Point {
                        x: 32. + i as f32 * 3.,
                        y: 128.,
                    },
                    pressure,
                    tilt_radians: tilt,
                    twist_radians: 0.,
                    distance: 0.,
                    phase: if i == 0 {
                        PenPhase::Down
                    } else if i == 64 {
                        PenPhase::Up
                    } else {
                        PenPhase::Move
                    },
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                })
                .unwrap();
            if i % cadence == 0 || i == 64 {
                engine.render_frame_at(time).unwrap();
                engine.backend_mut().wait_idle().unwrap();
                while engine.has_pending_input() {
                    engine.render_frame_at(time).unwrap();
                    engine.backend_mut().wait_idle().unwrap();
                }
            }
        }
    }
    let mut bytes = vec![0; 256 * 256 * 4];
    engine
        .backend_mut()
        .copy_rgba8_srgb(&mut bytes, 256 * 4)
        .unwrap();
    bytes
}

fn ink(bytes: &[u8], top: usize, bottom: usize) -> f64 {
    (top..bottom)
        .flat_map(|y| (64..192).map(move |x| (255 - bytes[(y * 256 + x) * 4]) as f64))
        .sum()
}

#[test]
fn paper_is_stationary_pressure_fills_tooth_and_tilt_reverses_the_shading_side() {
    let mut brush = default_brush(DefaultBrushPreset::PointyPencil);
    brush.diameter = 36.;
    let light = render(brush.clone(), 0.2, [0.; 2], 4, 1, false);
    let heavy = render(brush.clone(), 0.9, [0.; 2], 4, 1, false);
    assert!(ink(&heavy, 100, 156) > ink(&light, 100, 156) * 2.);
    brush.seed ^= 0x1234_5678;
    let other_seed = render(brush.clone(), 0.2, [0.; 2], 4, 1, false);
    assert_eq!(
        light, other_seed,
        "stroke randomness must not shift the paper"
    );
    let twice = render(brush.clone(), 0.2, [0.; 2], 4, 2, false);
    assert!(ink(&twice, 100, 156) > ink(&light, 100, 156));
    for (first, second) in light.chunks_exact(4).zip(twice.chunks_exact(4)) {
        assert!(
            second[0] <= first[0],
            "another pass deposits; it never smears pigment away"
        );
        if first[0] == 255 {
            assert!(
                second[0] >= 254,
                "uncontacted tooth stays fixed within readback rounding"
            );
        }
    }
    let forward = render(brush.clone(), 0.65, [0., 1.], 4, 1, false);
    let backward = render(brush, 0.65, [0., -1.], 4, 1, false);
    assert!(ink(&forward, 96, 126) > ink(&forward, 130, 160) * 1.5);
    assert!(ink(&backward, 130, 160) > ink(&backward, 96, 126) * 1.5);
}

#[test]
fn ink_is_continuous_and_frame_cadence_and_prediction_preserve_the_committed_result() {
    for preset in [
        DefaultBrushPreset::GPen,
        DefaultBrushPreset::CalligraphyPen,
        DefaultBrushPreset::WetInk,
        DefaultBrushPreset::BlottyInk,
        DefaultBrushPreset::BrushedInk,
    ] {
        let mut brush = default_brush(preset);
        brush.diameter = 44.;
        let reference = render(brush.clone(), 0.65, [0.; 2], 1, 1, false);
        let batched = render(brush.clone(), 0.65, [0.; 2], 8, 1, false);
        let predicted = render(brush, 0.65, [0.; 2], 2, 1, true);
        for other in [&batched, &predicted] {
            let max = reference
                .iter()
                .zip(other.iter())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                max <= 5,
                "{preset:?}: cadence/prediction changed a pixel by {max}"
            );
        }
        let centers: Vec<_> = (64..192).map(|x| reference[(128 * 256 + x) * 4]).collect();
        assert!(
            centers.iter().all(|v| *v < 125),
            "{preset:?}: continuous ink must not leave contact gaps"
        );
    }
}

#[test]
fn contact_spacing_does_not_break_the_nib_or_change_graphite_density() {
    for preset in [
        DefaultBrushPreset::Pencil,
        DefaultBrushPreset::CalligraphyPen,
    ] {
        let mut brush = default_brush(preset);
        brush.diameter = 48.;
        brush.spacing = 0.08;
        let fine = render(brush.clone(), 0.4, [0.; 2], 4, 1, false);
        brush.spacing = 0.4;
        let coarse = render(brush, 0.4, [0.; 2], 4, 1, false);
        let fine_ink = ink(&fine, 105, 151);
        let coarse_ink = ink(&coarse, 105, 151);
        assert!(
            (coarse_ink / fine_ink - 1.).abs() < 0.12,
            "{preset:?}: spacing changed density by {:.1}%",
            (coarse_ink / fine_ink - 1.) * 100.
        );
    }
}
