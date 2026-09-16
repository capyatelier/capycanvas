//! Reproducible real-GPU contact swatches and end-to-end frame timings.
//! cargo run --release -p layer-render-wgpu --example contact_gallery -- /tmp/contact-gallery
use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind,
    ViewTransform, input_queue,
};
use layer_render::{CanvasRenderer, ViewState};
use layer_render_wgpu::WgpuRasterizer;
use std::{fs, path::Path, time::Instant};

const WIDTH: u32 = 1000;
const HEIGHT: u32 = 760;
type Engine = CanvasEngine<WgpuRasterizer>;

fn stroke(
    engine: &mut Engine,
    input: &mut InputProducer<PenEvent>,
    clock: &mut u64,
    samples: usize,
    pose: impl Fn(f32) -> (f32, f32, f32, [f32; 2]),
    cpu: &mut Vec<f32>,
) {
    for i in 0..samples {
        let t = i as f32 / (samples - 1) as f32;
        let (x, y, pressure, tilt) = pose(t);
        *clock += 4_166_667;
        input
            .push(PenEvent {
                device_id: 1,
                sequence: *clock,
                timestamp_ns: *clock,
                view_revision: 0,
                surface_position: Point { x, y },
                pressure,
                tilt_radians: tilt,
                twist_radians: 0.,
                distance: 0.,
                phase: if i == 0 {
                    PenPhase::Down
                } else if i + 1 == samples {
                    PenPhase::Up
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        // Two 240 Hz samples per 120 Hz frame. Drain capture dependencies outside
        // the timed call; this harness waits, the production painting path doesn't.
        if i % 2 == 1 || i + 1 == samples {
            let start = Instant::now();
            engine.render_frame_at(*clock).unwrap();
            cpu.push(start.elapsed().as_secs_f32() * 1000.);
            engine.backend_mut().wait_idle().unwrap();
            while engine.has_pending_input() {
                engine.render_frame_at(*clock).unwrap();
                engine.backend_mut().wait_idle().unwrap();
            }
        }
    }
}

fn percentile(values: &[f32], p: f32) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted
        .get(((sorted.len().saturating_sub(1)) as f32 * p) as usize)
        .copied()
        .unwrap_or(f32::NAN)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/contact-gallery".into());
    let output = Path::new(&output);
    fs::create_dir_all(output)?;
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>Contact brush review</title><style>body{margin:30px auto;max-width:1000px;background:#eee;font:16px system-ui}img{width:100%;background:white}h2{margin-top:40px}small{color:#555}</style><h1>Contact brush review</h1><p><a href='timings.csv'>Ordinary-frame timings</a> · <a href='stress-timings.csv'>Broad-stroke timings</a></p><p>Rows: pressure ramp and lift; constant light / medium / heavy pressure; opposite tilted shading; broad curves and crossings; repeated hatching. All images rendered by the shared engine, 1000 × 760 document pixels.</p>",
    );
    let mut report = String::from(
        "preset,diameter,cpu_p50_ms,cpu_p95_ms,gpu_p50_ms,gpu_p95_ms,gpu_max_ms,gpu_samples\n",
    );
    let mut stress = String::from(
        "preset,diameter,prediction,cpu_p50_ms,cpu_p95_ms,gpu_p50_ms,gpu_p95_ms,gpu_max_ms,gpu_samples\n",
    );
    for preset in CONTACT_BRUSH_PRESETS {
        let gpu = WgpuRasterizer::new_headless()?;
        println!("{preset:?}: {}", gpu.adapter_info().name);
        let (mut input, consumer) = input_queue(128);
        let view = ViewState {
            width_px: WIDTH,
            height_px: HEIGHT,
            document_to_surface: Affine::IDENTITY.0,
            background_rgba_linear: [1.; 4],
        };
        let mut engine = CanvasEngine::new(
            gpu,
            Document::new("contact-gallery", WIDTH, HEIGHT),
            consumer,
            view,
            ViewTransform::IDENTITY,
        )?;
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: false,
                ..Default::default()
            })
            .unwrap();
        engine.render_frame_at(0)?;
        let brush = default_brush(preset);
        let diameter = brush.diameter;
        engine.set_brush(brush.clone())?;
        let mut clock = 1_000_000_000;
        let mut cpu = Vec::new();
        stroke(
            &mut engine,
            &mut input,
            &mut clock,
            180,
            |t| {
                (
                    60. + 880. * t,
                    65. + 16. * (t * 6.28).sin(),
                    (t * std::f32::consts::PI).sin().max(0.).powf(0.8),
                    [0.; 2],
                )
            },
            &mut cpu,
        );
        for (i, pressure) in [0.15, 0.45, 0.9].into_iter().enumerate() {
            stroke(
                &mut engine,
                &mut input,
                &mut clock,
                140,
                |t| (60. + 880. * t, 135. + i as f32 * 48., pressure, [0.; 2]),
                &mut cpu,
            );
        }
        for (i, tilt) in [[0., 1.0], [0., -1.0]].into_iter().enumerate() {
            stroke(
                &mut engine,
                &mut input,
                &mut clock,
                160,
                |t| (90. + 820. * t, 315. + i as f32 * 90., 0.5, tilt),
                &mut cpu,
            );
        }
        let mut broad = brush.clone();
        broad.diameter = 64.;
        engine.set_brush(broad)?;
        stroke(
            &mut engine,
            &mut input,
            &mut clock,
            300,
            |t| {
                (
                    100. + 800. * t,
                    525. + 48. * (t * 18.85).sin(),
                    0.2 + 0.75 * (t * 9.42).sin().abs(),
                    [0.; 2],
                )
            },
            &mut cpu,
        );
        engine.set_brush(brush.clone())?;
        for i in 0..18 {
            stroke(
                &mut engine,
                &mut input,
                &mut clock,
                32,
                |t| {
                    (
                        60. + i as f32 * 22. + t * 60.,
                        720. - t * 85.,
                        0.5,
                        [0.7, 0.4],
                    )
                },
                &mut cpu,
            );
        }
        // Steady-state default-size stroke, timing includes raster and compositing
        // GPU commands. Warmup, readback, and GPU waits are excluded from CPU.
        engine.backend_mut().set_telemetry_enabled(true);
        cpu.clear();
        stroke(
            &mut engine,
            &mut input,
            &mut clock,
            260,
            |t| {
                (
                    565. + 340. * t,
                    680. + 25. * (t * 12.56).sin(),
                    0.65,
                    [0.2, 0.1],
                )
            },
            &mut cpu,
        );
        let telemetry = engine.backend().telemetry();
        let gpu = telemetry.gpu.ordered();
        let line = format!(
            "{preset:?},{diameter},{:.4},{:.4},{:.4},{:.4},{:.4},{}\n",
            percentile(&cpu, 0.5),
            percentile(&cpu, 0.95),
            percentile(&gpu, 0.5),
            percentile(&gpu, 0.95),
            percentile(&gpu, 1.),
            gpu.len()
        );
        print!("{line}");
        report.push_str(&line);
        let filename = format!("{:02}-{preset:?}.png", preset as u32);
        let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
        engine
            .backend_mut()
            .copy_rgba8_srgb(&mut pixels, WIDTH as usize * 4)?;
        let mut encoder =
            png::Encoder::new(fs::File::create(output.join(&filename))?, WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
        html.push_str(&format!("<h2>{preset:?}</h2><small>Default diameter {diameter}px · CPU p95 {:.3} ms · GPU p95 {:.3} ms</small><img src='{filename}'>", percentile(&cpu,0.95), percentile(&gpu,0.95)));
        fs::write(output.join("index.html"), &html)?;
        fs::write(output.join("timings.csv"), &report)?;
        // Fast, broad strokes with the real replaceable prediction tail. The
        // entire encoder is timed, including coverage and compositing passes.
        engine
            .set_instant_feedback(InstantFeedbackConfig::default())
            .unwrap();
        for diameter in [128., 512.] {
            let mut large = brush.clone();
            large.diameter = diameter;
            engine.set_brush(large)?;
            cpu.clear();
            stroke(
                &mut engine,
                &mut input,
                &mut clock,
                320,
                |t| {
                    (
                        500. - 340. * (t * std::f32::consts::TAU * 3.).cos(),
                        380. + 190. * (t * std::f32::consts::TAU * 2.).sin(),
                        0.7,
                        [0.45, 0.2],
                    )
                },
                &mut cpu,
            );
            let gpu = engine.backend().telemetry().gpu.ordered();
            let cpu = &cpu[20..cpu.len() - 1];
            let line = format!(
                "{preset:?},{diameter},true,{:.4},{:.4},{:.4},{:.4},{:.4},{}\n",
                percentile(cpu, 0.5),
                percentile(cpu, 0.95),
                percentile(&gpu, 0.5),
                percentile(&gpu, 0.95),
                percentile(&gpu, 1.),
                gpu.len()
            );
            print!("stress: {line}");
            stress.push_str(&line);
            fs::write(output.join("stress-timings.csv"), &stress)?;
        }
    }
    Ok(())
}
