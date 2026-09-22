//! Completed native brush frames, including managed offscreen presentation.
//! brush_frames OUTPUT.csv [dx12|vulkan] [frames=240] [repeats=3] [preset-ids]
//! 9504x6336 (61 MP camera class), 1000 px, 1600x1000 fit, two 240 Hz samples/frame.
//! Excludes initialization, warmup, readback and file I/O. This is not display FPS.
//! Set CAPY_BRUSH_CAPTURES to save the first completed stroke per preset as PNG.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind,
    ViewTransform, input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::{SdrSurfaceColor, ViewportPresenter, WgpuRasterizer};
use std::{
    io::Write,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Engine = CanvasEngine<WgpuRasterizer>;
const EXTENT: [u32; 2] = [9504, 6336];
const SIZE: [u32; 2] = [1600, 1000];
const SCALE: f32 = SIZE[1] as f32 / EXTENT[1] as f32;

fn device(backend: wgpu::Backends) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backend;
    descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    assert_ne!(
        adapter.get_info().device_type,
        wgpu::DeviceType::Cpu,
        "Hardware GPU required"
    );
    println!("Adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & (wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    Ok((adapter, device, queue))
}

fn stroke(
    engine: &mut Engine,
    input: &mut InputProducer<PenEvent>,
    presenter: &mut ViewportPresenter,
    target: &wgpu::TextureView,
    clock: &mut u64,
    frames: u32,
) -> Result<Vec<[f64; 2]>> {
    let mut timings = Vec::with_capacity(frames as usize);
    for frame in 0..frames {
        let start = Instant::now();
        for sample in 0..2 {
            *clock += 4_166_667;
            let t = (frame * 2 + sample) as f32 / 240.;
            let angle = t * std::f32::consts::TAU * 0.8;
            input
                .push(PenEvent {
                    device_id: 1,
                    sequence: *clock,
                    timestamp_ns: *clock,
                    view_revision: 0,
                    surface_position: Point {
                        x: 800. + 550. * angle.cos(),
                        y: 500. + 320. * angle.sin(),
                    },
                    pressure: 1.,
                    tilt_radians: [0.; 2],
                    twist_radians: 0.,
                    distance: 0.,
                    phase: if frame == 0 && sample == 0 {
                        PenPhase::Down
                    } else if frame + 1 == frames && sample == 1 {
                        PenPhase::Up
                    } else {
                        PenPhase::Move
                    },
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                })
                .map_err(|_| "Input overflow")?;
        }
        engine.render_frame_at(*clock)?;
        // Account for capacity backpressure instead of timing an unconsumed update.
        while engine.has_pending_input() {
            engine.backend_mut().wait_idle()?;
            engine.render_frame_at(*clock)?;
        }
        presenter.present(
            engine.backend(),
            target,
            engine.view(),
            [0.12, 0.12, 0.12, 1.],
        )?;
        let submitted = start.elapsed().as_secs_f64() * 1000.;
        engine.backend().device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })?;
        timings.push([submitted, start.elapsed().as_secs_f64() * 1000.]);
    }
    Ok(timings)
}
fn roots(engine: &Engine) -> Result<Vec<(LayerId, layer_core::raster::TileKey, [u8; 32])>> {
    let mut roots = Vec::new();
    for layer in &engine.document().layers {
        for (coord, tile) in &layer.raster.wait_data()?.tiles {
            roots.push((layer.id, *coord, tile.wait_backing()?.digest));
        }
    }
    Ok(roots)
}
fn capture(gpu: &WgpuRasterizer, target: &wgpu::Texture, path: &std::path::Path) -> Result<()> {
    let stride = (SIZE[0] * 8).div_ceil(256) * 256;
    let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("completed brush frame readback"),
        size: u64::from(stride * SIZE[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut commands = gpu.device().create_command_encoder(&Default::default());
    commands.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        target.size(),
    );
    gpu.queue().submit([commands.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = send.send(result);
    });
    gpu.device().poll(wgpu::PollType::wait_indefinitely())?;
    receive.recv()??;
    let mapped = buffer.get_mapped_range(..)?;
    let mut pixels = Vec::with_capacity((SIZE[0] * SIZE[1] * 4) as usize);
    for row in mapped.chunks_exact(stride as usize) {
        for pixel in row[..SIZE[0] as usize * 8].as_chunks::<8>().0 {
            for channel in 0..4 {
                let value = layer_core::color::f16::from_bits(u16::from_le_bytes([
                    pixel[channel * 2],
                    pixel[channel * 2 + 1],
                ]))
                .to_f32();
                let value = if channel == 3 {
                    value
                } else if value <= 0.0031308 {
                    value * 12.92
                } else {
                    1.055 * value.powf(1. / 2.4) - 0.055
                };
                pixels.push((value.clamp(0., 1.) * 255.).round() as u8);
            }
        }
    }
    drop(mapped);
    buffer.unmap();
    let mut encoder = png::Encoder::new(std::fs::File::create(path)?, SIZE[0], SIZE[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args.next().ok_or("OUTPUT.csv required")?;
    if output == "--adapters" {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        for (index, adapter) in
            pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY))
                .iter()
                .enumerate()
        {
            println!("LAYER_GPU_INDEX={index}: {:?}", adapter.get_info());
        }
        return Ok(());
    }
    let backend = match args.next().as_deref().unwrap_or("dx12") {
        "dx12" => wgpu::Backends::DX12,
        "vulkan" => wgpu::Backends::VULKAN,
        _ => return Err("Unknown backend".into()),
    };
    let frames: u32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(240);
    let repeats: u32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(3);
    assert!(frames >= 2 && repeats > 0);
    let filter: Option<Vec<u32>> = args
        .next()
        .map(|s| s.split(',').map(str::parse).collect())
        .transpose()?;
    let captures = std::env::var_os("CAPY_BRUSH_CAPTURES").map(std::path::PathBuf::from);
    if let Some(path) = &captures {
        std::fs::create_dir_all(path)?;
    }
    let mut csv = std::io::BufWriter::new(std::fs::File::create(output)?);
    writeln!(
        csv,
        "preset,id,repetition,frame,pen_up,submit_ms,completed_ms"
    )?;
    let (adapter, device, queue) = device(backend)?;
    for preset in CONTACT_BRUSH_PRESETS {
        if filter
            .as_ref()
            .is_some_and(|ids| !ids.contains(&(preset as u32)))
        {
            continue;
        }
        let document = Document::new("brush frames", EXTENT[0], EXTENT[1]);
        let mut brush = default_brush(preset);
        brush.diameter = 1000.;
        let mut gpu = WgpuRasterizer::from_wgpu_native_staged(
            adapter.clone(),
            device.clone(),
            queue.clone(),
            Default::default(),
        )?;
        gpu.prepare_startup(&document, &brush, false)?;
        gpu.finish_startup_cache();
        let startup = Instant::now();
        while !gpu.poll_startup()?.complete {
            if startup.elapsed() > Duration::from_secs(180) {
                return Err("Brush preparation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let (mut input, consumer) = input_queue(128);
        let tx = (SIZE[0] as f32 - EXTENT[0] as f32 * SCALE) * 0.5;
        let view = ViewState {
            width_px: SIZE[0],
            height_px: SIZE[1],
            document_to_surface: [SCALE, 0., 0., SCALE, tx, 0.],
            background_rgba_linear: [1.; 4],
        };
        let mut engine = CanvasEngine::new(
            gpu,
            document,
            consumer,
            view,
            ViewTransform {
                revision: 0,
                surface_to_document: [1. / SCALE, 0., 0., 1. / SCALE, -tx / SCALE, 0.],
            },
        )?;
        engine.set_instant_feedback(InstantFeedbackConfig {
            enabled: true,
            use_platform_prediction: false,
            prediction_horizon_micros: 16_000,
            ..Default::default()
        })?;
        engine.render_frame_at(0)?;
        engine.set_brush(brush)?;
        let format = wgpu::TextureFormat::Rgba16Float;
        let texture = engine
            .backend()
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("brush benchmark presentation"),
                size: wgpu::Extent3d {
                    width: SIZE[0],
                    height: SIZE[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let target = texture.create_view(&Default::default());
        let mut presenter = ViewportPresenter::for_surface(
            engine.backend(),
            format,
            SdrSurfaceColor::ExtendedLinearSrgb,
        )?;
        let mut clock = 1_000_000_000;
        if preset == DefaultBrushPreset::Eraser {
            let mut ink = default_brush(DefaultBrushPreset::GPen);
            ink.diameter = 1400.;
            engine.set_brush(ink)?;
            stroke(
                &mut engine,
                &mut input,
                &mut presenter,
                &target,
                &mut clock,
                frames,
            )?;
            let mut eraser = default_brush(preset);
            eraser.diameter = 1000.;
            engine.set_brush(eraser)?;
            engine.set_tool(StrokeTool::Eraser);
        }
        let baseline = roots(&engine)?;
        stroke(
            &mut engine,
            &mut input,
            &mut presenter,
            &target,
            &mut clock,
            64,
        )?;
        assert!(engine.undo()?);
        clock += 20_000_000;
        engine.render_frame_at(clock)?;
        engine.backend_mut().wait_idle()?;
        assert_eq!(roots(&engine)?, baseline);
        let mut total = 0.;
        for repetition in 1..=repeats {
            let times = stroke(
                &mut engine,
                &mut input,
                &mut presenter,
                &target,
                &mut clock,
                frames,
            )?;
            if repetition == 1
                && let Some(path) = &captures
            {
                capture(
                    engine.backend(),
                    &texture,
                    &path.join(format!("{:02}-{preset:?}.png", preset as u32)),
                )?;
            }
            let painted = roots(&engine)?;
            assert_ne!(painted, baseline, "{preset:?} must change content");
            assert!(engine.undo()?);
            clock += 20_000_000;
            engine.render_frame_at(clock)?;
            engine.backend_mut().wait_idle()?;
            assert_eq!(roots(&engine)?, baseline, "{preset:?} exact native Undo");
            assert!(engine.redo()?);
            clock += 20_000_000;
            engine.render_frame_at(clock)?;
            engine.backend_mut().wait_idle()?;
            assert_eq!(roots(&engine)?, painted, "{preset:?} exact native Redo");
            assert!(engine.undo()?);
            clock += 20_000_000;
            engine.render_frame_at(clock)?;
            engine.backend_mut().wait_idle()?;
            for (frame, [submit, complete]) in times.into_iter().enumerate() {
                total += complete;
                writeln!(
                    csv,
                    "{preset:?},{},{repetition},{},{},{submit:.6},{complete:.6}",
                    preset as u32,
                    frame + 1,
                    frame + 1 == frames as usize
                )?;
            }
            csv.flush()?;
        }
        println!(
            "{preset:?}: {:.2} completed frames/s; exact native Undo/Redo passed",
            f64::from(frames * repeats) * 1000. / total
        );
    }
    Ok(())
}
