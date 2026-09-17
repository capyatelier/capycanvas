//! Reproduce large-photo interaction without host UI automation.
//! Usage: photo_interaction INPUT.capy OUTPUT.csv [samples-per-frame] [cache-MiB] [stroke-frames] [brush-px] [circles|zigzag]
//! Reads a copy of a project; never modifies its input. Timings include an
//! offscreen managed presentation and queue completion, not display latency.
use layer_core::*;
use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags,
    ToolKind, ViewTransform, input_queue};
use layer_render::{CanvasRenderer, ViewState};
use layer_render_wgpu::{SdrSurfaceColor, ViewportPresenter, WgpuRasterizer};
use std::{collections::BTreeMap, io::{BufReader, BufWriter, Write}, time::{Duration, Instant}};

const SIZE: [u32; 2] = [2752, 2064];
const DRAW_SCALE: f32 = 0.2;
type Engine = CanvasEngine<WgpuRasterizer>;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn camera(extent: [u32; 2], scale: f32, angle: f32) -> (ViewState, ViewTransform) {
    let (sin, cos) = angle.sin_cos();
    let a = scale * cos;
    let b = scale * sin;
    let [x, y] = extent.map(|v| v as f32 * 0.5);
    let tx = SIZE[0] as f32 * 0.5 - a*x + b*y;
    let ty = SIZE[1] as f32 * 0.5 - b*x - a*y;
    (ViewState { width_px: SIZE[0], height_px: SIZE[1],
        document_to_surface: [a, b, -b, a, tx, ty], background_rgba_linear: [0.; 4] },
     ViewTransform { revision: 0,
        surface_to_document: [a/(scale*scale), -b/(scale*scale), b/(scale*scale),
            a/(scale*scale), (-a*tx-b*ty)/(scale*scale), (b*tx-a*ty)/(scale*scale)] })
}

fn roots(engine: &Engine) -> Result<BTreeMap<LayerId, Vec<u8>>> {
    engine.document().layers.iter().map(|l| {
        let mut bytes = Vec::new();
        for (key, tile) in &l.raster.wait_data()?.tiles {
            bytes.extend(format!("{key:?}").as_bytes());
            bytes.extend(tile.wait_backing()?.digest);
        }
        Ok((l.id, bytes))
    }).collect()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("INPUT.capy is required")?;
    let output = args.next().ok_or("OUTPUT.csv is required")?;
    let samples: u64 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(2);
    let allowance: u64 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(0);
    let frames: u64 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(360);
    let diameter: f32 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(570.7);
    let path = args.next().unwrap_or_else(|| "circles".into());
    assert!((1..=256).contains(&samples) && frames > 0);
    assert!(diameter.is_finite() && diameter > 0.);
    assert!(matches!(path.as_str(), "circles" | "zigzag"));
    let project = Project::read(BufReader::new(std::fs::File::open(input)?), Default::default())?;
    let extent = [project.document.width, project.document.height];
    let mut gpu = WgpuRasterizer::new_native_headless(project.document.color)?;
    gpu.set_complete_display_allowance(allowance * 1024 * 1024);
    gpu.set_telemetry_enabled(true);
    let (mut input, consumer) = input_queue(samples as usize + 8);
    let (mut view, inverse) = camera(extent, DRAW_SCALE, 0.);
    // Keep the stroke on the visible document. A viewport-sized path can lie
    // mostly outside a zoomed-out canvas and understate the drawing cost.
    let path_radius: [f32; 2] = std::array::from_fn(|axis| {
        let visible = (extent[axis] as f32 * DRAW_SCALE).min(SIZE[axis] as f32);
        0.42 * (visible - diameter * DRAW_SCALE).max(0.)
    });
    let mut engine = CanvasEngine::new(gpu, project.document, consumer, view, inverse)?;
    engine.set_instant_feedback(InstantFeedbackConfig {
        enabled: true, use_platform_prediction: false, prediction_horizon_micros: 8_000,
        ..Default::default()
    })?;
    let mut brush = default_brush(DefaultBrushPreset::GPen);
    brush.diameter = diameter;
    brush.color_rgba_linear = [0.8, 0.04, 0.2, 1.];
    engine.set_brush(brush)?;
    engine.render_frame()?;
    engine.backend_mut().wait_idle()?;
    let format = wgpu::TextureFormat::Rgba16Float;
    let target = engine.backend().device().create_texture(&wgpu::TextureDescriptor {
        label: Some("photo interaction presentation"),
        size: wgpu::Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
    }).create_view(&Default::default());
    let mut presenter = ViewportPresenter::for_surface(engine.backend(), format,
        SdrSurfaceColor::ExtendedLinearSrgb)?;
    let original = roots(&engine)?;
    let mut csv = BufWriter::new(std::fs::File::create(&output)?);
    writeln!(csv, "phase,frame,cpu_ms,completed_ms,prepare_ms,paint_ms,capture_ms,prediction_ms,composition_ms,submission_ms,dabs,composited_pixels,source_misses,display_batches,display_bytes")?;
    let mut sequence = 0;
    for phase in ["circles", "zoom"] {
        for frame in 0..if phase == "circles" { frames } else { 360 } {
            let before = engine.backend().metrics();
            let now = 1_000_000_000 + (frame + 1) * samples * 4_166_667;
            let start = Instant::now();
            if phase == "circles" {
                for j in 0..samples {
                    let index = frame * samples + j;
                    let t = index as f32 / 240.;
                    let angle = t * std::f32::consts::TAU * 2.;
                    let position = if path == "zigzag" {
                        let triangle = |phase: f32| 1. - 4. * (phase.fract() - 0.5).abs();
                        [triangle(t * 2.), triangle(t * 0.7 + 0.25)]
                    } else { [angle.cos(), angle.sin()] };
                    sequence += 1;
                    input.push(PenEvent { device_id: 1, sequence,
                        timestamp_ns: 1_000_000_000 + index * 4_166_667, view_revision: 0,
                        surface_position: Point { x: SIZE[0] as f32 * 0.5 + path_radius[0]*position[0],
                            y: SIZE[1] as f32 * 0.5 + path_radius[1]*position[1] },
                        pressure: 1., tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
                        tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
                        phase: if index == 0 { PenPhase::Down }
                            else if index == frames*samples-1 { PenPhase::Up } else { PenPhase::Move },
                    }).map_err(|_| "Input queue overflow")?;
                }
            } else {
                let t = (frame % 120) as f32 / 119.;
                let scale = 0.1 * 40f32.powf((t * std::f32::consts::TAU).cos()*0.5+0.5);
                let inverse;
                (view, inverse) = camera(extent, scale, -0.12);
                engine.set_view(view, inverse);
            }
            engine.render_frame_at(now)?;
            assert!(!engine.has_pending_input(), "Input exceeded one render drain");
            presenter.present(engine.backend(), &target, view, [0.1, 0.1, 0.1, 1.])?;
            let cpu = start.elapsed().as_secs_f64()*1000.;
            engine.backend().device().poll(wgpu::PollType::Wait {
                submission_index: None, timeout: Some(Duration::from_secs(30)),
            })?;
            let completed = start.elapsed().as_secs_f64()*1000.;
            let after = engine.backend().metrics();
            let [prepare, paint, capture, prediction, composition, submission] = after.frame_cpu_ms;
            writeln!(csv, "{phase},{frame},{cpu:.6},{completed:.6},{prepare:.6},{paint:.6},{capture:.6},{prediction:.6},{composition:.6},{submission:.6},{},{},{},{},{}",
                after.dabs-before.dabs, after.composited_pixels-before.composited_pixels,
                after.source_tile_misses-before.source_tile_misses,
                after.display_composition_submissions-before.display_composition_submissions,
                after.composite_storage_bytes)?;
        }
        csv.flush()?;
        println!("Completed {phase}");
    }
    let painted = roots(&engine)?;
    std::fs::write(format!("{output}.roots"), format!("{painted:?}"))?;
    assert_ne!(painted, original);
    assert!(engine.undo()?);
    engine.render_frame()?;
    engine.backend_mut().wait_idle()?;
    assert_eq!(roots(&engine)?, original);
    assert!(engine.redo()?);
    engine.render_frame()?;
    engine.backend_mut().wait_idle()?;
    assert_eq!(roots(&engine)?, painted);
    println!("Exact native Undo/Redo roots preserved");
    Ok(())
}
