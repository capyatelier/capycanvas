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
    render_extent(brush, pressure, tilt, cadence, passes, feedback, [256; 2])
}

fn render_extent(
    brush: BrushSnapshot,
    pressure: f32,
    tilt: [f32; 2],
    cadence: usize,
    passes: usize,
    feedback: bool,
    extent: [u32; 2],
) -> Vec<u8> {
    // Reuse one physical device across swatches. Repeated Vulkan instance /
    // device creation can exhaust the driver before the image assertions run.
    static GPU: std::sync::OnceLock<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> =
        std::sync::OnceLock::new();
    let (adapter, device, queue) = GPU.get_or_init(|| {
        let gpu = WgpuRasterizer::new_headless().expect("physical GPU required");
        (
            gpu.adapter().clone(),
            gpu.device().clone(),
            gpu.queue().clone(),
        )
    });
    #[allow(deprecated)]
    let gpu = WgpuRasterizer::from_wgpu(adapter.clone(), device.clone(), queue.clone()).unwrap();
    render_with_gpu(
        gpu, brush, pressure, tilt, cadence, passes, feedback, extent,
    )
}

fn render_with_gpu(
    gpu: WgpuRasterizer,
    brush: BrushSnapshot,
    pressure: f32,
    tilt: [f32; 2],
    cadence: usize,
    passes: usize,
    feedback: bool,
    extent: [u32; 2],
) -> Vec<u8> {
    let (mut input, consumer) = input_queue(256);
    let mut engine = CanvasEngine::new(
        gpu,
        Document::new("contact-invariants", extent[0], extent[1]),
        consumer,
        ViewState {
            width_px: extent[0],
            height_px: extent[1],
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
                        x: extent[0] as f32 * (0.125 + i as f32 * 0.75 / 64.),
                        y: extent[1] as f32 * 0.5,
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
    let mut bytes = vec![0; extent[0] as usize * extent[1] as usize * 4];
    engine
        .backend_mut()
        .copy_rgba8_srgb(&mut bytes, extent[0] as usize * 4)
        .unwrap();
    bytes
}

#[test]
fn long_contact_batches_preserve_ink_across_tile_boundaries() {
    for preset in [DefaultBrushPreset::GPen, DefaultBrushPreset::CalligraphyPen] {
        let mut brush = default_brush(preset);
        brush.diameter = 36.;
        let reference = render_extent(brush.clone(), 0.65, [0.; 2], 1, 1, false, [1536, 256]);
        let batched = render_extent(brush, 0.65, [0.; 2], 64, 1, false, [1536, 256]);
        assert!(reference.chunks_exact(4).any(|p| p[0] < 100));
        let difference = reference
            .iter()
            .zip(&batched)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            difference <= 5,
            "{preset:?}: batching across tiles changed ink by {difference}"
        );
    }
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
    let mut smooth = brush.clone();
    smooth.grain.as_mut().unwrap().depth = 0.;
    let no_tooth = render(smooth, 0.2, [0.; 2], 4, 1, false);
    assert!(
        ink(&no_tooth, 100, 156) > ink(&light, 100, 156) * 1.5,
        "Texture strength must control the paper's restriction of pigment"
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

#[test]
fn integrated_dry_materials_build_up_and_replay_independently_of_frame_cadence() {
    use DefaultBrushPreset::*;
    for preset in [
        Pencil,
        PointyPencil,
        ShadingPencil,
        Charcoal,
        Paintbrush,
        Airbrush,
        Chalk,
        Marker,
        DualTexture,
        TexturedFlat,
        PastelBlock,
        TransparentGlaze,
    ] {
        let mut brush = default_brush(preset);
        brush.diameter = 40.;
        let reference = render(brush.clone(), 0.65, [0.; 2], 1, 1, false);
        let predicted = render(brush.clone(), 0.65, [0.; 2], 8, 1, true);
        let repeated = render(brush, 0.65, [0.; 2], 4, 2, false);
        let error = reference
            .iter()
            .zip(&predicted)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            error <= 5,
            "{preset:?}: cadence/prediction changed a pixel by {error}"
        );
        let single = ink(&reference, 96, 160);
        assert!(single > 100., "{preset:?}: must deposit visible pigment");
        assert!(
            ink(&repeated, 96, 160) > single * 1.05,
            "{preset:?}: repeated passes must build pigment"
        );
    }
}

#[test]
fn integrated_graphite_has_no_contact_cadence_banding_on_smooth_paper() {
    let mut brush = default_brush(DefaultBrushPreset::Pencil);
    brush.diameter = 40.;
    brush.contact.as_mut().unwrap().paper = 0.;
    let pixels = render(brush, 0.7, [0.; 2], 4, 1, false);
    let centers: Vec<_> = (70..186).map(|x| pixels[(128 * 256 + x) * 4]).collect();
    let variation = centers.iter().max().unwrap() - centers.iter().min().unwrap();
    assert!(
        variation <= 3,
        "uniform pressure/travel must not expose span boundaries: {variation}"
    );
}

#[test]
fn graphite_hardness_changes_the_native_deposit_without_span_banding() {
    let mut brush = default_brush(DefaultBrushPreset::Pencil);
    brush.diameter = 48.;
    brush.contact.as_mut().unwrap().paper = 0.;
    let mut deposits = Vec::new();
    for hardness in [0.2, 0.9] {
        brush.hardness = hardness;
        let pixels = render_with_gpu(
            WgpuRasterizer::new_native_headless(color::DocumentColor::default()).unwrap(),
            brush.clone(),
            0.6,
            [0.; 2],
            4,
            1,
            true,
            [256; 2],
        );
        let center: Vec<_> = (70..186).map(|x| pixels[(128 * 256 + x) * 4]).collect();
        assert!(
            center.iter().max().unwrap() - center.iter().min().unwrap() <= 3,
            "Hardness must not introduce contact boundaries into a steady stroke"
        );
        deposits.push(ink(&pixels, 96, 160));
    }
    assert!(
        deposits[1] > deposits[0] * 1.05,
        "A firmer graphite contact must visibly fill more of its pressure footprint"
    );
}
