//! Captured physical Wacom input, including repeated nonzero pressure on up.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;

// Retain one hardware device across trace variants. Recreating D3D12 devices
// recompiles the same material pipelines and obscures the actual regression run.
fn renderer() -> WgpuRasterizer {
    static GPU: std::sync::OnceLock<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> = std::sync::OnceLock::new();
    let (adapter, device, queue) = GPU.get_or_init(|| {
        let gpu = WgpuRasterizer::new_headless().expect("physical GPU required");
        (gpu.adapter().clone(), gpu.device().clone(), gpu.queue().clone())
    });
    #[allow(deprecated)]
    WgpuRasterizer::from_wgpu(adapter.clone(), device.clone(), queue.clone()).unwrap()
}

fn pixels(engine: &mut CanvasEngine<WgpuRasterizer>) -> Vec<u8> {
    engine.backend_mut().wait_idle().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while engine.has_pending_input() || engine.has_pending_document_edits() {
        assert!(
            std::time::Instant::now() < deadline,
            "pending raster work timed out"
        );
        engine.render_frame().unwrap();
        engine.backend_mut().wait_idle().unwrap();
    }
    let mut bytes = vec![0; 1024 * 512 * 4];
    engine
        .backend_mut()
        .copy_rgba8_srgb(&mut bytes, 1024 * 4)
        .unwrap();
    bytes
}

fn max_channel_difference(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap()
}

#[test]
fn physical_release_pixels_survive_batching_prediction_and_history() {
    let limited = default_brush(DefaultBrushPreset::GPen).stabilization.pressure_fall_micros;
    let rows: Vec<Vec<f32>> =
        include_str!("../../layer-engine/tests/fixtures/wacom-rapid-lift.csv")
            .lines()
            .skip(1)
            .map(|line| line.split(',').map(|v| v.parse().unwrap()).collect())
            .collect();
    for capture in 0..5 {
        let samples: Vec<_> = rows.iter().filter(|r| r[0] as usize == capture).collect();
        let mut reference: Option<Vec<u8>> = None;
        let mut before: Option<Vec<u8>> = None;
        for (release, feedback, cadence) in [
            (0, false, 1),
            (limited, false, 1),
            (limited, false, 4),
            (limited, false, 64),
            (limited, true, 1),
            (limited, true, 4),
            (limited, true, 64),
        ] {
            let (mut input, consumer) = input_queue(128);
            let mut engine = CanvasEngine::new(
                renderer(),
                Document::new("captured release", 1024, 512),
                consumer,
                ViewState {
                    width_px: 1024,
                    height_px: 512,
                    document_to_surface: Affine::IDENTITY.0,
                    background_rgba_linear: [1.; 4],
                },
                ViewTransform::IDENTITY,
            )
            .unwrap();
            let mut brush = default_brush(DefaultBrushPreset::GPen);
            brush.diameter = samples[0][6];
            brush.stabilization.pressure_fall_micros = release;
            engine.set_brush(brush).unwrap();
            engine
                .set_instant_feedback(InstantFeedbackConfig {
                    enabled: feedback,
                    ..Default::default()
                })
                .unwrap();
            engine.render_frame().unwrap();
            let blank = pixels(&mut engine);
            let mut before_up = None;
            for (i, row) in samples.iter().enumerate() {
                let time = 1_000_000_000 + row[5] as u64 * 1000;
                input
                    .push(PenEvent {
                        device_id: 1,
                        sequence: i as u64 + 1,
                        timestamp_ns: time,
                        view_revision: 0,
                        surface_position: Point {
                            x: row[2],
                            y: row[3],
                        },
                        pressure: row[4],
                        tilt_radians: [0.; 2],
                        twist_radians: 0.,
                        distance: 0.,
                        phase: if i == 0 {
                            PenPhase::Down
                        } else if i + 1 == samples.len() {
                            PenPhase::Up
                        } else {
                            PenPhase::Move
                        },
                        tool: ToolKind::Pen,
                        flags: SampleFlags::PRIMARY,
                    })
                    .unwrap();
                if (i + 1) % cadence == 0 || i + 1 == samples.len() {
                    engine.render_frame_for(time, time + 8_000_000).unwrap();
                }
                if release > 0 && !feedback && cadence == 1 && i + 2 == samples.len() {
                    before_up = Some(pixels(&mut engine));
                }
            }
            let painted = pixels(&mut engine);
            assert_ne!(painted, blank);
            if let Some(preview) = before_up {
                let max = max_channel_difference(&preview, &painted);
                assert!(
                    max <= 1,
                    "release preview must match final ink: capture={capture}, feedback={feedback}, max={max}"
                );
            }
            if release == 0 {
                before = Some(painted.clone());
            } else if let Some(reference) = &reference {
                let max = max_channel_difference(reference, &painted);
                // Separate tile submissions can round an edge by one 8-bit code.
                assert!(
                    max <= 1,
                    "capture={capture}, feedback={feedback}, cadence={cadence}, max={max}"
                );
            } else {
                assert_ne!(
                    before.as_ref().unwrap(),
                    &painted,
                    "pressure limiter must soften the abrupt pressure step"
                );
                reference = Some(painted.clone());
            }
            assert!(engine.undo().unwrap());
            engine.render_frame().unwrap();
            assert!(
                pixels(&mut engine) == blank,
                "undo capture={capture}, release={release}"
            );
            assert!(engine.redo().unwrap());
            engine.render_frame().unwrap();
            assert!(
                pixels(&mut engine) == painted,
                "redo capture={capture}, release={release}"
            );
        }
    }
}

#[test]
fn steady_light_keeps_literal_pixels_and_falling_pressure_is_smoothed() {
    let limited = default_brush(DefaultBrushPreset::GPen).stabilization.pressure_fall_micros;
    for (interval_micros, falling) in [(4000, false), (16000, true), (4000, true)] {
        let mut reference = None;
        for release in [0, limited] {
            let (mut input, consumer) = input_queue(16);
            let mut engine = CanvasEngine::new(
                renderer(),
                Document::new("release control", 512, 256),
                consumer,
                ViewState {
                    width_px: 512,
                    height_px: 256,
                    document_to_surface: Affine::IDENTITY.0,
                    background_rgba_linear: [1.; 4],
                },
                ViewTransform::IDENTITY,
            )
            .unwrap();
            let mut brush = default_brush(DefaultBrushPreset::GPen);
            brush.diameter = 128.;
            brush.stabilization.pressure_fall_micros = release;
            engine.set_brush(brush).unwrap();
            for i in 0..=6 {
                input
                    .push(PenEvent {
                        device_id: 1,
                        sequence: i + 1,
                        timestamp_ns: 1_000_000_000 + i * interval_micros * 1000,
                        view_revision: 0,
                        surface_position: Point {
                            x: 100. + i.min(5) as f32 * 40.,
                            y: 128.,
                        },
                        pressure: if falling && i < 4 { 0.12 } else { 0.04 },
                        tilt_radians: [0.; 2],
                        twist_radians: 0.,
                        distance: 0.,
                        phase: if i == 0 {
                            PenPhase::Down
                        } else if i == 6 {
                            PenPhase::Up
                        } else {
                            PenPhase::Move
                        },
                        tool: ToolKind::Pen,
                        flags: SampleFlags::PRIMARY,
                    })
                    .unwrap();
                engine.render_frame().unwrap();
            }
            let painted = pixels(&mut engine);
            if let Some(reference) = &reference {
                if falling {
                    assert_ne!(reference, &painted, "falling pressure must be smoothed");
                } else {
                    assert_eq!(
                        reference, &painted,
                        "steady light input must remain literal"
                    );
                }
            } else {
                reference = Some(painted);
            }
        }
    }
}

#[test]
fn varying_radius_sweeps_preserve_the_edge_when_subdivided() {
    use layer_render::{CanvasRenderer, DabBatch, DabBatchKind, DabMode, DabStyle, FramePacket};
    let mut gpu = renderer();
    let document = Document::new("analytic taper", 384, 256);
    let brush = default_brush(DefaultBrushPreset::GPen);
    let style = DabStyle {
        brush_to_layer: Affine::IDENTITY,
        alpha_locked: false,
        selection: None,
        tip: brush.tip.clone(),
        mode: DabMode::Paint,
        execution: brush.execution_class(),
        grain: brush.grain.clone(),
        dual: brush.dual.clone(),
        rendering: brush.rendering,
        wet_mix: brush.wet_mix,
        transport: brush.transport.clone(),
        deform: brush.deform,
        contact: brush.contact,
    };
    let stroke = Stroke::new(
        StrokeId(1),
        document.layers[0].id,
        StrokeTool::Brush,
        brush,
        vec![StrokePoint {
            position: Point { x: 100., y: 128. },
            pressure: 1.,
            tilt: [0.; 2],
            twist: 0.,
            elapsed_micros: 0,
        }],
    )
    .unwrap();
    let mut prototype = Vec::new();
    layer_engine::DabGenerator::generate(&stroke, document.color.space, &mut prototype);
    for aspect in [1., 2.] {
        for (r0, r1, length) in [(64., 1., 160.), (1., 64., 160.), (64., 1., 30.)] {
            let mut images = Vec::new();
            for count in [1, 64] {
                let mut dabs = Vec::<layer_render::Dab>::new();
                for i in 0..=count {
                    let t = i as f32 / count as f32;
                    let mut dab = prototype[0];
                    dab.center.x = 100. + length * t;
                    let radius = r0 + (r1 - r0) * t;
                    dab.radii = [radius, radius / aspect];
                    let previous = dabs.last().copied().unwrap_or(dab);
                    dab.previous = [previous.radii[0], previous.radii[1], 1., 0.];
                    dab.motion = [dab.center.x - previous.center.x, 0.];
                    dabs.push(dab);
                }
                let damage = dabs
                    .iter()
                    .fold(Rect::EMPTY, |damage, d| damage.union(d.bounds()));
                let batches = [DabBatch {
                    material_update: 0,
                    stroke_id: stroke.id,
                    layer_id: stroke.layer_id,
                    kind: DabBatchKind::Persistent,
                    stroke_start: true,
                    stroke_end: true,
                    first_dab: 0,
                    dab_count: dabs.len() as u32,
                    style: style.clone(),
                    damage,
                }];
                gpu.submit(FramePacket {
                    restore_rasters: &[],
                    time_seconds: 0.,
                    document_extent: [384, 256],
                    view: ViewState {
                        width_px: 384,
                        height_px: 256,
                        document_to_surface: Affine::IDENTITY.0,
                        background_rgba_linear: [1.; 4],
                    },
                    layers: &document.layers,
                    dabs: &dabs,
                    dab_batches: &batches,
                    reset_layers: true,
                    composite_all: true,
                })
                .unwrap();
                let mut pixels = vec![0; 384 * 256 * 4];
                gpu.copy_rgba8_srgb(&mut pixels, 384 * 4).unwrap();
                images.push(pixels);
            }
            let edge = |image: &[u8], x: usize| -> f32 {
                for y in 1..=128 {
                    let a = image[((y - 1) * 384 + x) * 4] as f32;
                    let b = image[(y * 384 + x) * 4] as f32;
                    if a >= 128. && b < 128. {
                        return (y - 1) as f32 + (a - 128.) / (a - b);
                    }
                }
                128.
            };
            let max = (50..=(100. + length) as usize)
                .map(|x| (edge(&images[0], x) - edge(&images[1], x)).abs())
                .fold(0_f32, f32::max);
            assert!(
                max < 0.5,
                "aspect={aspect}, radii={r0}->{r1}, length={length}, edge difference={max}"
            );
        }
    }
}
