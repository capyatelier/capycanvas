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

fn pixels(engine: &mut Engine) -> Vec<u8> {
    let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
    engine
        .backend_mut()
        .copy_rgba8_srgb(&mut pixels, WIDTH as usize * 4)
        .unwrap();
    pixels
}

fn save_image(path: &Path, pixels: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = png::Encoder::new(fs::File::create(path)?, WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(pixels)?;
    Ok(())
}

fn viewport_pixels(engine: &mut Engine) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use layer_render_wgpu::{SdrSurfaceColor, ViewportPresenter};
    let camera = engine.view();
    let r = engine.backend_mut();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = r.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("contact gallery viewport"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    ViewportPresenter::for_surface(r, format, SdrSurfaceColor::Srgb)?.present(
        r,
        &target.create_view(&Default::default()),
        camera,
        [1.; 4],
    )?;
    r.wait_idle()?;
    let stride = (WIDTH * 4).div_ceil(256) * 256;
    let readback = r.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("contact gallery viewport pixels"),
        size: u64::from(stride * HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut commands = r.device().create_command_encoder(&Default::default());
    commands.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        target.size(),
    );
    r.queue().submit([commands.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |result| {
        send.send(result).unwrap();
    });
    r.device().poll(wgpu::PollType::wait_indefinitely())?;
    receive.recv()??;
    let bytes = readback
        .get_mapped_range(..)?
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..WIDTH as usize * 4].iter().copied())
        .collect();
    readback.unmap();
    Ok(bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/contact-gallery".into());
    let output = Path::new(&output);
    let gestures = std::env::args().nth(3).as_deref() == Some("gestures");
    let large = std::env::args().nth(3).as_deref() == Some("large");
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
    let mut presets = CONTACT_BRUSH_PRESETS.to_vec();
    if let Some(filter) = std::env::args().nth(2) {
        let ids: Vec<u32> = filter
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        presets.retain(|p| ids.contains(&(*p as u32)));
    }
    for preset in presets {
        let gpu = WgpuRasterizer::new_native_headless(color::DocumentColor::default())?;
        println!("{preset:?}: {}", gpu.adapter_info().name);
        let (mut input, consumer) = input_queue(128);
        let scale = if large { WIDTH as f32 / 9504. } else { 1. };
        let extent = if large { [9504, 6336] } else { [WIDTH, HEIGHT] };
        let view = ViewState {
            width_px: WIDTH,
            height_px: HEIGHT,
            document_to_surface: [scale, 0., 0., scale, 0., 0.],
            background_rgba_linear: [1.; 4],
        };
        let mut engine = CanvasEngine::new(
            gpu,
            Document::new("contact-gallery", extent[0], extent[1]),
            consumer,
            view,
            ViewTransform {
                revision: 0,
                surface_to_document: [1. / scale, 0., 0., 1. / scale, 0., 0.],
            },
        )?;
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: false,
                ..Default::default()
            })
            .unwrap();
        engine.render_frame_at(0)?;
        let mut brush = default_brush(preset);
        brush.diameter = brush.diameter.min(48.);
        let diameter = brush.diameter;
        engine.set_brush(brush.clone())?;
        let mut clock = 1_000_000_000;
        let mut cpu = Vec::new();
        if large {
            brush.diameter = 1000.;
            brush.color_rgba_linear = [0.006, 0.006, 0.006, 1.];
            engine.set_brush(brush)?;
            engine.set_instant_feedback(InstantFeedbackConfig::default())?;
            stroke(
                &mut engine,
                &mut input,
                &mut clock,
                361,
                |t| {
                    let tilt = 0.8 * (t * std::f32::consts::PI).sin().powi(2);
                    (
                        500. - 330. + 660. * t,
                        333. + 82. * (t * 3. * std::f32::consts::PI).sin(),
                        0.15 + 0.85 * (t * std::f32::consts::PI).sin(),
                        [tilt * 0.5_f32.sin(), tilt * 0.5_f32.cos()],
                    )
                },
                &mut cpu,
            );
            let name = format!("{:02}-{preset:?}", preset as u32);
            save_image(
                &output.join(format!("{name}.png")),
                &viewport_pixels(&mut engine)?,
            )?;
            let transform = Affine([1., 0., 0., 1., 500. - 500. / scale, 380. - 251. / scale]);
            engine.set_view(
                ViewState {
                    document_to_surface: transform.0,
                    ..view
                },
                ViewTransform {
                    revision: 1,
                    surface_to_document: transform.inverse().unwrap().0,
                },
            );
            clock += 20_000_000;
            engine.render_frame_at(clock)?;
            save_image(
                &output.join(format!("{name}-detail.png")),
                &viewport_pixels(&mut engine)?,
            )?;
            println!("{preset:?}: 1000px pressure/tilt curve captured at fit and 100%");
            continue;
        }
        if preset == DefaultBrushPreset::Eraser {
            let mut base = default_brush(DefaultBrushPreset::GPen);
            base.diameter = 1800.;
            base.color_rgba_linear = [0.08, 0.18, 0.3, 1.];
            engine.set_brush(base)?;
            stroke(
                &mut engine,
                &mut input,
                &mut clock,
                8,
                |t| (499. + t, 380., 1., [0.; 2]),
                &mut cpu,
            );
            engine.set_brush(brush.clone())?;
            engine.set_tool(StrokeTool::Eraser);
        }
        if gestures {
            brush.diameter = 32.;
            engine.set_brush(brush)?;
            engine.set_instant_feedback(InstantFeedbackConfig::default())?;
            // Sharp corners and reversals, with a stable input rate independent
            // of the renderer cadence. The lower pair retraces the same line.
            for (x, points) in [
                (0., [[80., 80.], [380., 80.], [380., 200.], [80., 200.]]),
                (500., [[80., 80.], [220., 200.], [360., 80.], [220., 200.]]),
            ] {
                stroke(
                    &mut engine,
                    &mut input,
                    &mut clock,
                    361,
                    |t| {
                        let segment = (t * 3.).floor().min(2.) as usize;
                        let u = t * 3. - segment as f32;
                        let a = points[segment];
                        let b = points[segment + 1];
                        (
                            x + a[0] + (b[0] - a[0]) * u,
                            a[1] + (b[1] - a[1]) * u,
                            0.65,
                            [0.; 2],
                        )
                    },
                    &mut cpu,
                );
            }
            for row in 0..2 {
                for _ in 0..=row {
                    stroke(
                        &mut engine,
                        &mut input,
                        &mut clock,
                        181,
                        |t| (80. + t * 800., 310. + row as f32 * 100., 0.35, [0.6, 0.2]),
                        &mut cpu,
                    );
                }
            }
            // A quarter-second and a one-second stationary contact. Airbrush
            // must build with elapsed input time; dry tools should not smear.
            for (x, samples) in [(170., 61), (410., 241)] {
                stroke(
                    &mut engine,
                    &mut input,
                    &mut clock,
                    samples,
                    |_| (x, 580., 0.7, [0.; 2]),
                    &mut cpu,
                );
            }
            for i in 0..8 {
                stroke(
                    &mut engine,
                    &mut input,
                    &mut clock,
                    20,
                    |t| {
                        (
                            590. + i as f32 * 40. + 40. * t,
                            690. - t * 160.,
                            0.65,
                            [0.; 2],
                        )
                    },
                    &mut cpu,
                );
            }
            let before = pixels(&mut engine);
            assert!(engine.undo()?);
            clock += 20_000_000;
            engine.render_frame_at(clock)?;
            engine.backend_mut().wait_idle()?;
            assert!(engine.redo()?);
            clock += 20_000_000;
            engine.render_frame_at(clock)?;
            engine.backend_mut().wait_idle()?;
            let after = pixels(&mut engine);
            let difference = before
                .iter()
                .zip(&after)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                difference <= 5,
                "{preset:?}: undo/redo changed a pixel by {difference}"
            );
            let filename = format!("{:02}-{preset:?}.png", preset as u32);
            save_image(&output.join(&filename), &after)?;
            html.push_str(&format!("<h2>{preset:?}</h2><p>Corners/reversal; one/two passes; short/long dwell; rapid lifts. Undo/redo maximum channel difference: {difference}.</p><img src='{filename}'>"));
            fs::write(output.join("index.html"), &html)?;
            println!("{preset:?}: gestures and undo/redo passed (difference {difference})");
            continue;
        }
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
