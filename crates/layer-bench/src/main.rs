use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::{
    BrushStabilization, DefaultBrushPreset as Preset, Document, Edit, Layer, LayerId, Point,
    StrokeTool, default_brush,
};
use layer_engine::{
    CanvasEngine, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind,
    ViewTransform, input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::{GpuRasterMetrics, WgpuRasterizer};
use std::error::Error;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod previews;

const WIDTH: u32 = 4096;
const HEIGHT: u32 = 4096;
const EVENTS_PER_FRAME: usize = 8;
const FRAME_BUDGET_MICROS: u64 = 8_333;

// One explicit mode for every canvas in this process, including warm-up and
// report probes. Initialized once before GPU work.
static DOCUMENT_COLOR: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();

fn document_mode() -> String {
    let (space, depth) = DOCUMENT_COLOR.get().copied().unwrap_or((0, 8));
    format!("{} integer{depth}; native integer backing, Float32 working tiles",
        ["sRGB", "Display P3", "Adobe RGB", "ProPhoto RGB"][space as usize])
}

fn document_color() -> DocumentColor {
    let (space, depth) = DOCUMENT_COLOR.get().copied().unwrap_or((0, 8));
    DocumentColor {
        space: [
            RgbSpace::Srgb,
            RgbSpace::DisplayP3,
            RgbSpace::AdobeRgb,
            RgbSpace::ProPhoto,
        ][space as usize],
        depth: if depth == 16 {
            SampleDepth::U16
        } else {
            SampleDepth::U8
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScenarioKind {
    GPen,
    Pencil,
    Eraser,
    Paintbrush,
    Airbrush,
    Chalk,
    Marker,
    Spray,
    DualTexture,
    MultiplyGlaze,
    Smudge,
    WetRound,
    LiquifyPush,
    LiquifyTwirl,
    Layers,
    TexturedFlat,
    DryScumble,
    PastelBlock,
    TransparentGlaze,
    OpaqueGouache,
    WatercolorWash,
    WetWatercolor,
    LoadedOil,
    PaletteKnife,
    NaturalBlender,
}

impl ScenarioKind {
    const LEGACY: [Self; 15] = [
        Self::GPen,
        Self::Pencil,
        Self::Eraser,
        Self::Paintbrush,
        Self::Airbrush,
        Self::Chalk,
        Self::Marker,
        Self::Spray,
        Self::DualTexture,
        Self::MultiplyGlaze,
        Self::Smudge,
        Self::WetRound,
        Self::LiquifyPush,
        Self::LiquifyTwirl,
        Self::Layers,
    ];
    const PAINTER: [Self; 10] = [
        Self::TexturedFlat,
        Self::DryScumble,
        Self::PastelBlock,
        Self::TransparentGlaze,
        Self::OpaqueGouache,
        Self::WatercolorWash,
        Self::WetWatercolor,
        Self::LoadedOil,
        Self::PaletteKnife,
        Self::NaturalBlender,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::GPen => "gpen_inking",
            Self::Pencil => "pencil_shading",
            Self::Eraser => "large_eraser",
            Self::Paintbrush => "large_paintbrush",
            Self::Airbrush => "soft_airbrush",
            Self::Chalk => "anchored_grain_chalk",
            Self::Marker => "flat_marker",
            Self::Spray => "scatter_spray",
            Self::DualTexture => "dual_texture",
            Self::MultiplyGlaze => "multiply_glaze",
            Self::Smudge => "smudge_pickup",
            Self::WetRound => "wet_round_oklab",
            Self::LiquifyPush => "liquify_push",
            Self::LiquifyTwirl => "liquify_twirl",
            Self::Layers => "layered_composite",
            Self::TexturedFlat => "textured_flat_filbert",
            Self::DryScumble => "dry_scumble",
            Self::PastelBlock => "pastel_block",
            Self::TransparentGlaze => "transparent_glaze",
            Self::OpaqueGouache => "opaque_gouache",
            Self::WatercolorWash => "watercolor_wash_edge",
            Self::WetWatercolor => "wet_watercolor",
            Self::LoadedOil => "loaded_oil_mixer",
            Self::PaletteKnife => "palette_knife",
            Self::NaturalBlender => "natural_blender",
        }
    }

    fn features(self) -> &'static str {
        match self {
            Self::TexturedFlat | Self::PastelBlock => "advanced dry",
            Self::DryScumble => "coverage",
            Self::TransparentGlaze => "wetness",
            Self::OpaqueGouache => "reservoir + wetness",
            Self::WatercolorWash | Self::WetWatercolor => {
                "coverage + R8 wetness + event-driven capillary transport + live edge"
            }
            Self::LoadedOil => "reservoir + wetness",
            Self::PaletteKnife => "reservoir + wetness",
            Self::Smudge | Self::NaturalBlender => "smudge advection",
            Self::LiquifyPush | Self::LiquifyTwirl => "bilinear deformation",
            Self::WetRound => "reservoir + Oklab mixing",
            _ => "existing path",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::LEGACY
            .into_iter()
            .chain(Self::PAINTER)
            .find(|kind| kind.name() == value)
    }
}

#[derive(Debug)]
struct Options {
    color: (u32, u32),
    scenarios: Vec<ScenarioKind>,
    output_dir: PathBuf,
    report_path: PathBuf,
    repeats: usize,
}

#[derive(Clone, Copy)]
struct Sample {
    x: f32,
    y: f32,
    pressure: f32,
}

#[derive(Clone, Copy)]
struct Brush {
    preset: Preset,
    diameter: f32,
    opacity: f32,
    color: [f32; 4],
}

struct StrokeSpec {
    layer: u64,
    brush: Brush,
    samples: Vec<Sample>,
}

#[derive(Clone, Copy)]
struct FrameMeasurement {
    backing_reserved_bytes: u64,
    submit_micros: u64,
    completed_micros: u64,
    commit: bool,
}

struct BenchResult {
    samples: Vec<FrameMeasurement>,
    backing_reserved_bytes: u64,
    name: &'static str,
    repeats: usize,
    frames: usize,
    p50_micros: u64,
    p95_micros: u64,
    p99_micros: u64,
    max_micros: u64,
    submit_p95_micros: u64,
    submit_p50_micros: u64,
    submit_p99_micros: u64,
    commit_submit_p99_micros: u64,
    over_budget: usize,
    commit_over_budget: usize,
    dabs: u64,
    candidate_pixels: u64,
    composited_pixels: u64,
    paint_pages: u64,
    preview_pages: u64,
    coverage_pages: u64,
    material_pages: u64,
    storage_bytes: u64,
    commit_p99_micros: u64,
}

struct Canvas {
    producer: InputProducer<PenEvent>,
    engine: CanvasEngine<WgpuRasterizer>,
    sequence: u64,
    real_timestamp_ns: u64,
    submitted_events: u64,
}

impl Canvas {
    fn new() -> Result<Self, String> {
        Self::configured([WIDTH, HEIGHT], [0.93, 0.92, 0.88, 1.0])
    }

    fn configured(extent: [u32; 2], background_rgba_linear: [f32; 4]) -> Result<Self, String> {
        let color = document_color();
        let mut document = Document::new("untitled", extent[0], extent[1]);
        document.color = color;
        let (producer, consumer) = input_queue(16_384);
        let view = ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            background_rgba_linear,
        };
        let backend = WgpuRasterizer::new_native_headless(color).map_err(|e| e.to_string())?;
        let mut engine =
            CanvasEngine::new(backend, document, consumer, view, ViewTransform::IDENTITY)
                .map_err(|e| e.to_string())?;
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: false,
                ..InstantFeedbackConfig::default()
            })
            .map_err(|e| e.to_string())?;
        let mut canvas = Self {
            producer,
            engine,
            sequence: 0,
            real_timestamp_ns: 0,
            submitted_events: 0,
        };
        canvas.draw()?;
        canvas.wait_idle()?;
        Ok(canvas)
    }

    fn draw(&mut self) -> Result<(), String> {
        self.engine.render_frame().map_err(|e| e.to_string())
    }

    // Backpressure can defer consumption. Never record an empty submission as
    // a completed drawing frame. Count actual CPU frame creation separately
    // from time spent awaiting bounded backing capacity.
    fn drain_submitted(&mut self) -> Result<u64, String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut cpu = 0;
        while self.engine.metrics().input_events < self.submitted_events {
            if Instant::now() >= deadline {
                return Err("Input did not drain within the capture deadline".into());
            }
            self.wait_idle()?;
            std::thread::yield_now();
            let start = Instant::now();
            self.draw()?;
            cpu += start.elapsed().as_micros() as u64;
        }
        Ok(cpu)
    }

    fn wait_idle(&mut self) -> Result<(), String> {
        self.engine
            .backend_mut()
            .wait_idle()
            .map_err(|e| e.to_string())
    }

    fn undo(&mut self) -> Result<(), String> {
        if !self.engine.undo().map_err(|e| e.to_string())? {
            return Err("warm-up stroke was not committed".to_owned());
        }
        // A capture queue can defer the undo submission. Finish the actual
        // restoration here, outside the drawing measurement window.
        let frames = self.engine.metrics().frames;
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.engine.metrics().frames == frames {
            if Instant::now() >= deadline {
                return Err("Warm-up undo did not finish".into());
            }
            self.draw()?;
            self.wait_idle()?;
            std::thread::yield_now();
        }
        self.wait_idle()
    }

    fn set_brush(&mut self, brush: Brush) -> Result<(), String> {
        let mut snapshot = default_brush(brush.preset);
        snapshot.diameter = brush.diameter;
        snapshot.opacity = brush.opacity;
        snapshot.color_rgba_linear = brush.color;
        snapshot.stabilization = BrushStabilization {
            streamline: 0.0,
            pressure_smoothing: 0.0,
            stabilization: 0.0,
            motion_filtering: 0.0,
            expression: 1.0,
            ..snapshot.stabilization
        };
        self.engine.set_brush(snapshot).map_err(|e| e.to_string())?;
        self.engine.set_tool(if brush.preset == Preset::Eraser {
            StrokeTool::Eraser
        } else {
            StrokeTool::Brush
        });
        Ok(())
    }

    fn set_active_layer(&mut self, layer: u64) -> Result<(), String> {
        self.engine
            .set_active_layer(LayerId(layer))
            .map_err(|e| e.to_string())
    }

    fn add_layer(&mut self, name: &str, index: usize) -> Result<u64, String> {
        let id = self.engine.allocate_layer_id();
        self.engine
            .apply_edit(Edit::InsertLayer {
                index,
                layer: Layer::paint(id, name),
            })
            .map_err(|e| e.to_string())?;
        Ok(id.0)
    }

    fn set_layer_opacity(&mut self, layer: u64, opacity: f32) -> Result<(), String> {
        self.engine
            .set_layer_opacity(LayerId(layer), opacity)
            .map_err(|e| e.to_string())
    }

    fn submit(&mut self, events: &[PenEvent]) -> Result<(), String> {
        for (accepted, event) in events.iter().enumerate() {
            if self.producer.push(*event).is_err() {
                return Err(format!(
                    "event ingress accepted {accepted} of {} records",
                    events.len()
                ));
            }
        }
        self.submitted_events += events.len() as u64;
        Ok(())
    }

    fn raster_metrics(&self) -> GpuRasterMetrics {
        self.engine.backend().metrics()
    }

    fn write_png(&mut self, path: &Path) -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.engine.has_pending_input() || self.engine.has_pending_document_edits() {
            if Instant::now() >= deadline {
                return Err("Canvas work did not finish before readback".into());
            }
            self.draw()?;
            if self.engine.has_pending_input() || self.engine.has_pending_document_edits() {
                self.wait_idle()?;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let [width, height] = self.engine.backend().document_extent();
        let stride = width as usize * 4;
        let mut rgba = vec![0; stride * height as usize];
        self.engine
            .backend_mut()
            .copy_rgba8_srgb(&mut rgba, stride)
            .map_err(|e| e.to_string())?;
        let file = BufWriter::new(File::create(path)?);
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;
        Ok(())
    }

    fn next_event(&mut self, sample: Sample, phase: PenPhase) -> PenEvent {
        self.real_timestamp_ns = self.real_timestamp_ns.saturating_add(1_000_000);
        self.sequence = self.sequence.saturating_add(1);
        PenEvent {
            device_id: 1,
            sequence: self.sequence,
            timestamp_ns: self.real_timestamp_ns,
            view_revision: 0,
            surface_position: Point {
                x: sample.x,
                y: sample.y,
            },
            pressure: sample.pressure,
            tilt_radians: [0.0, 0.0],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }
    }
}

fn phase(index: usize, count: usize) -> PenPhase {
    if index == 0 {
        PenPhase::Down
    } else if index + 1 == count {
        PenPhase::Up
    } else {
        PenPhase::Move
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() == Some("--brush-previews") {
        return previews::generate(Path::new("apps/layer-web/brush-previews"));
    }
    let options = parse_options()?;
    DOCUMENT_COLOR.set(options.color).expect("benchmark color initialized once");
    eprintln!("Document mode: {}", document_mode());
    fs::create_dir_all(&options.output_dir)?;
    if let Some(parent) = options.report_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut results = Vec::with_capacity(options.scenarios.len());
    for scenario in &options.scenarios {
        eprintln!("measuring {} at {}x{}", scenario.name(), WIDTH, HEIGHT);
        let (result, mut canvas) = measure_scenario(*scenario, options.repeats)?;
        let image_path = options.output_dir.join(format!("{}.png", scenario.name()));
        canvas.write_png(&image_path)?;
        print_result(&result);
        results.push(result);
    }
    write_report(&options.report_path, &results)?;
    write_frame_samples(&options.report_path.with_extension("frames.csv"), &results)?;
    write_gallery(&options.output_dir, &results)?;

    Ok(())
}

fn parse_options() -> Result<Options, Box<dyn Error>> {
    let mut color = (0, 8);
    let mut scenarios = ScenarioKind::LEGACY.to_vec();
    let mut output_dir = PathBuf::from("artifacts/images");
    let mut report_path = PathBuf::from("artifacts/benchmarks/gpu-4k.md");
    let mut repeats = 1;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--space" => {
                color.0 = match arguments.next().ok_or("--space needs a value")?.as_str() {
                    "srgb" => 0,
                    "p3" => 1,
                    "adobe-rgb" => 2,
                    "prophoto" => 3,
                    _ => return Err("--space needs srgb, p3, adobe-rgb, or prophoto".into()),
                };
            }
            "--depth" => {
                color.1 = arguments.next().ok_or("--depth needs a value")?.parse()?;
                if !matches!(color.1, 8 | 16) { return Err("--depth needs 8 or 16".into()); }
            }
            "--scenario" => {
                let value = arguments.next().ok_or("--scenario needs a value")?;
                scenarios = if value == "all" {
                    ScenarioKind::LEGACY
                        .into_iter()
                        .chain(ScenarioKind::PAINTER)
                        .collect()
                } else if value == "legacy" {
                    ScenarioKind::LEGACY.to_vec()
                } else if value == "painter" {
                    ScenarioKind::PAINTER.to_vec()
                } else if value == "dry" {
                    ScenarioKind::LEGACY[..9].to_vec()
                } else if value == "watercolor" {
                    vec![ScenarioKind::WatercolorWash, ScenarioKind::WetWatercolor]
                } else {
                    vec![ScenarioKind::parse(&value).ok_or("unknown scenario")?]
                };
            }
            "--output-dir" => {
                output_dir = PathBuf::from(arguments.next().ok_or("--output-dir needs a value")?);
            }
            "--report" => {
                report_path = PathBuf::from(arguments.next().ok_or("--report needs a value")?);
            }
            "--repeats" => {
                repeats = arguments
                    .next()
                    .ok_or("--repeats needs a value")?
                    .parse::<usize>()?;
                if repeats == 0 {
                    return Err("--repeats must be at least 1".into());
                }
            }
            "--help" | "-h" => {
                println!(
                    "gpu-bench [--scenario all|legacy|painter|dry|watercolor|SCENARIO_NAME] \
                     [--output-dir PATH] [--report PATH] [--repeats N] \
                     [--space srgb|p3|adobe-rgb|prophoto] [--depth 8|16]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(Options {
        color,
        scenarios,
        output_dir,
        report_path,
        repeats,
    })
}

fn measure_scenario(
    kind: ScenarioKind,
    repeats: usize,
) -> Result<(BenchResult, Canvas), Box<dyn Error>> {
    let mut all_measurements = Vec::new();
    let mut aggregate_metrics = GpuRasterMetrics::default();
    let mut final_canvas = None;
    for repeat in 0..repeats {
        eprintln!("  repetition {}/{}", repeat + 1, repeats);
        let mut canvas = Canvas::new()?;
        for index in 0..31 {
            let _ = canvas.add_layer(&format!("Empty benchmark layer {index}"), 1)?;
        }
        canvas.draw()?;
        canvas.wait_idle()?;
        let strokes = prepare_scenario(kind, &mut canvas)?;
        // Prime this scenario's actual raster pipeline and device clocks, then
        // restore its starting pixels before starting the timing window.
        if !strokes.is_empty() {
            run_strokes(&mut canvas, &strokes[..1], None)?;
            canvas.undo()?;
        }
        let before = canvas.raster_metrics();
        let mut measurements = Vec::with_capacity(
            strokes
                .iter()
                .map(|stroke| stroke.samples.len().div_ceil(EVENTS_PER_FRAME))
                .sum(),
        );
        run_strokes(&mut canvas, &strokes, Some(&mut measurements))?;
        let after = canvas.raster_metrics();
        merge_metrics_delta(&mut aggregate_metrics, &before, &after);
        all_measurements.extend(measurements);
        // Retain only the final repetition for the gallery export. Keeping the
        // previous full 4K canvas alive while allocating the next one needlessly
        // doubles benchmark VRAM without changing any measured work.
        if repeat + 1 == repeats {
            final_canvas = Some(canvas);
        }
    }
    Ok((
        summarize(kind, repeats, &all_measurements, &aggregate_metrics),
        final_canvas.expect("repeats is validated as non-zero"),
    ))
}

fn merge_metrics_delta(
    aggregate: &mut GpuRasterMetrics,
    before: &GpuRasterMetrics,
    after: &GpuRasterMetrics,
) {
    aggregate.dabs = aggregate
        .dabs
        .saturating_add(after.dabs.saturating_sub(before.dabs));
    aggregate.raster_candidate_pixels = aggregate.raster_candidate_pixels.saturating_add(
        after
            .raster_candidate_pixels
            .saturating_sub(before.raster_candidate_pixels),
    );
    aggregate.composited_pixels = aggregate.composited_pixels.saturating_add(
        after
            .composited_pixels
            .saturating_sub(before.composited_pixels),
    );
    aggregate.paint_pages = aggregate.paint_pages.max(after.paint_pages);
    aggregate.preview_pages = aggregate.preview_pages.max(after.preview_pages);
    aggregate.coverage_pages = aggregate.coverage_pages.max(after.coverage_pages);
    aggregate.material_pages = aggregate.material_pages.max(after.material_pages);
    aggregate.paint_storage_bytes = aggregate.paint_storage_bytes.max(after.paint_storage_bytes);
    aggregate.preview_storage_bytes = aggregate
        .preview_storage_bytes
        .max(after.preview_storage_bytes);
    aggregate.destination_storage_bytes = aggregate
        .destination_storage_bytes
        .max(after.destination_storage_bytes);
    aggregate.paint_state_storage_bytes = aggregate
        .paint_state_storage_bytes
        .max(after.paint_state_storage_bytes);
    aggregate.composite_storage_bytes = aggregate
        .composite_storage_bytes
        .max(after.composite_storage_bytes);
}

fn prepare_scenario(
    kind: ScenarioKind,
    canvas: &mut Canvas,
) -> Result<Vec<StrokeSpec>, Box<dyn Error>> {
    let strokes = match kind {
        ScenarioKind::GPen => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::GPen, 24.0, 1.0, [0.004, 0.006, 0.009, 1.0]),
                samples: lissajous(1400, 1780.0, 1360.0, 0.0, 0.3),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::GPen, 13.0, 0.92, [0.025, 0.01, 0.008, 1.0]),
                samples: bezier(
                    900,
                    [
                        (340.0, 3280.0),
                        (1160.0, 420.0),
                        (2820.0, 3740.0),
                        (3760.0, 680.0),
                    ],
                ),
            },
        ],
        ScenarioKind::Pencil => pencil_hatching(1),
        ScenarioKind::Eraser => {
            let underpaint = vec![StrokeSpec {
                layer: 1,
                brush: brush(Preset::Paintbrush, 720.0, 0.9, [0.18, 0.035, 0.012, 1.0]),
                samples: lissajous(700, 1500.0, 1320.0, 0.4, 1.1),
            }];
            run_strokes(canvas, &underpaint, None)?;
            vec![
                StrokeSpec {
                    layer: 1,
                    brush: brush(Preset::Eraser, 360.0, 0.85, [0.0, 0.0, 0.0, 1.0]),
                    samples: bezier(
                        600,
                        [
                            (280.0, 700.0),
                            (1260.0, 3680.0),
                            (2740.0, 300.0),
                            (3810.0, 3400.0),
                        ],
                    ),
                },
                StrokeSpec {
                    layer: 1,
                    brush: brush(Preset::Eraser, 620.0, 0.5, [0.0, 0.0, 0.0, 1.0]),
                    samples: lissajous(520, 1300.0, 1000.0, 1.2, 0.2),
                },
            ]
        }
        ScenarioKind::Paintbrush => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::Paintbrush, 680.0, 0.82, [0.28, 0.025, 0.008, 1.0]),
                samples: bezier(
                    560,
                    [
                        (220.0, 700.0),
                        (1400.0, 3600.0),
                        (2400.0, 300.0),
                        (3880.0, 3150.0),
                    ],
                ),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::Paintbrush, 520.0, 0.72, [0.01, 0.06, 0.24, 1.0]),
                samples: bezier(
                    520,
                    [
                        (300.0, 3300.0),
                        (1300.0, 500.0),
                        (2880.0, 3900.0),
                        (3780.0, 760.0),
                    ],
                ),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::Paintbrush, 880.0, 0.5, [0.5, 0.2, 0.006, 1.0]),
                samples: lissajous(500, 1250.0, 980.0, 0.2, 1.8),
            },
        ],
        ScenarioKind::Airbrush => vec![StrokeSpec {
            layer: 1,
            brush: brush(Preset::Airbrush, 420.0, 0.82, [0.025, 0.12, 0.56, 1.0]),
            samples: lissajous(520, 1460.0, 1180.0, 0.2, 0.8),
        }],
        ScenarioKind::Chalk => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::Chalk, 150.0, 0.9, [0.68, 0.08, 0.025, 1.0]),
                samples: bezier(
                    480,
                    [
                        (260.0, 700.0),
                        (1180.0, 3600.0),
                        (2800.0, 280.0),
                        (3820.0, 3340.0),
                    ],
                ),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(Preset::Chalk, 96.0, 0.72, [0.03, 0.18, 0.52, 1.0]),
                samples: lissajous(420, 1320.0, 1040.0, 1.1, 0.0),
            },
        ],
        ScenarioKind::Marker => vec![StrokeSpec {
            layer: 1,
            brush: brush(Preset::Marker, 230.0, 0.78, [0.78, 0.035, 0.12, 0.78]),
            samples: lissajous(480, 1520.0, 1120.0, 0.0, 1.2),
        }],
        ScenarioKind::Spray => vec![StrokeSpec {
            layer: 1,
            brush: brush(Preset::Spray, 42.0, 0.74, [0.025, 0.42, 0.09, 1.0]),
            samples: lissajous(420, 1420.0, 1080.0, 0.9, 0.3),
        }],
        ScenarioKind::DualTexture => vec![StrokeSpec {
            layer: 1,
            brush: brush(Preset::DualTexture, 340.0, 0.88, [0.09, 0.018, 0.48, 1.0]),
            samples: bezier(
                480,
                [
                    (240.0, 3180.0),
                    (1260.0, 320.0),
                    (2860.0, 3820.0),
                    (3850.0, 760.0),
                ],
            ),
        }],
        ScenarioKind::MultiplyGlaze => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(Preset::MultiplyGlaze, 320.0, 0.66, [0.06, 0.22, 0.76, 1.0]),
                samples: lissajous(360, 1280.0, 940.0, 0.6, 1.4),
            }]
        }
        ScenarioKind::Smudge => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(Preset::Smudge, 260.0, 0.94, [0.0, 0.0, 0.0, 1.0]),
                samples: bezier(
                    360,
                    [
                        (420.0, 860.0),
                        (1250.0, 3500.0),
                        (2870.0, 420.0),
                        (3680.0, 3210.0),
                    ],
                ),
            }]
        }
        ScenarioKind::WetRound => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(Preset::WetRound, 300.0, 0.86, [0.015, 0.12, 0.62, 1.0]),
                samples: lissajous(360, 1260.0, 920.0, 0.3, 1.0),
            }]
        }
        ScenarioKind::LiquifyPush => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(Preset::LiquifyPush, 520.0, 1.0, [0.0; 4]),
                samples: bezier(
                    300,
                    [
                        (520.0, 1120.0),
                        (1480.0, 3280.0),
                        (2720.0, 620.0),
                        (3560.0, 3020.0),
                    ],
                ),
            }]
        }
        ScenarioKind::LiquifyTwirl => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(Preset::LiquifyTwirl, 720.0, 1.0, [0.0; 4]),
                samples: lissajous(240, 980.0, 760.0, 0.2, 0.5),
            }]
        }
        ScenarioKind::Layers => {
            let color = canvas.add_layer("Color wash", 1)?;
            let detail = canvas.add_layer("Texture detail", 0)?;
            canvas.set_layer_opacity(color, 0.68)?;
            canvas.set_layer_opacity(detail, 0.78)?;
            canvas.draw()?;
            let mut strokes = vec![
                StrokeSpec {
                    layer: color,
                    brush: brush(Preset::Paintbrush, 760.0, 0.76, [0.03, 0.2, 0.16, 1.0]),
                    samples: lissajous(520, 1440.0, 1160.0, 0.8, 0.0),
                },
                StrokeSpec {
                    layer: 1,
                    brush: brush(Preset::GPen, 21.0, 0.95, [0.008, 0.008, 0.012, 1.0]),
                    samples: lissajous(1000, 1660.0, 1320.0, 0.0, 0.7),
                },
            ];
            strokes.extend(pencil_hatching(detail).into_iter().take(6));
            strokes
        }
        kind @ (ScenarioKind::TexturedFlat
        | ScenarioKind::DryScumble
        | ScenarioKind::PastelBlock
        | ScenarioKind::TransparentGlaze
        | ScenarioKind::OpaqueGouache
        | ScenarioKind::WatercolorWash
        | ScenarioKind::WetWatercolor
        | ScenarioKind::LoadedOil
        | ScenarioKind::PaletteKnife
        | ScenarioKind::NaturalBlender) => {
            prepare_painter_base(canvas)?;
            painter_strokes(kind)
        }
    };
    Ok(strokes)
}

fn painter_strokes(kind: ScenarioKind) -> Vec<StrokeSpec> {
    let (preset, diameter, opacity) = match kind {
        ScenarioKind::TexturedFlat => (Preset::TexturedFlat, 420.0, 0.88),
        ScenarioKind::DryScumble => (Preset::DryScumble, 520.0, 0.82),
        ScenarioKind::PastelBlock => (Preset::PastelBlock, 340.0, 0.84),
        ScenarioKind::TransparentGlaze => (Preset::TransparentGlaze, 620.0, 0.72),
        ScenarioKind::OpaqueGouache => (Preset::OpaqueGouache, 480.0, 0.92),
        ScenarioKind::WatercolorWash => (Preset::WatercolorWash, 700.0, 0.78),
        ScenarioKind::WetWatercolor => (Preset::WetWatercolor, 620.0, 0.82),
        ScenarioKind::LoadedOil => (Preset::LoadedOil, 560.0, 0.94),
        ScenarioKind::PaletteKnife => (Preset::PaletteKnife, 680.0, 0.90),
        ScenarioKind::NaturalBlender => (Preset::NaturalBlender, 520.0, 0.88),
        _ => unreachable!("only painter scenarios call painter_strokes"),
    };
    // A restrained warm/cool palette makes pickup and mixing legible without
    // turning the gallery into a synthetic rainbow stress test.
    let palette = [
        [0.72, 0.018, 0.008, 1.0],
        [0.006, 0.032, 0.46, 1.0],
        [0.88, 0.30, 0.006, 1.0],
        [0.004, 0.34, 0.18, 1.0],
    ];
    vec![
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter, opacity, palette[0]),
            samples: bezier(
                320,
                [
                    (260.0, 820.0),
                    (1180.0, 3500.0),
                    (2760.0, 320.0),
                    (3840.0, 3080.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter * 0.78, opacity * 0.90, palette[1]),
            samples: bezier(
                300,
                [
                    (320.0, 3220.0),
                    (1320.0, 420.0),
                    (2920.0, 3780.0),
                    (3760.0, 860.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter * 0.58, opacity * 0.82, palette[2]),
            samples: lissajous(280, 1180.0, 860.0, 0.45, 1.2),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter * 0.44, opacity * 0.76, palette[3]),
            samples: lissajous(260, 1420.0, 620.0, 1.1, 0.2),
        },
    ]
}

fn run_strokes(
    canvas: &mut Canvas,
    strokes: &[StrokeSpec],
    mut measurements: Option<&mut Vec<FrameMeasurement>>,
) -> Result<(), String> {
    for stroke in strokes {
        canvas.set_active_layer(stroke.layer)?;
        canvas.set_brush(stroke.brush)?;
        for start in (0..stroke.samples.len()).step_by(EVENTS_PER_FRAME) {
            let end = (start + EVENTS_PER_FRAME).min(stroke.samples.len());
            let events = (start..end)
                .map(|index| {
                    canvas.next_event(stroke.samples[index], phase(index, stroke.samples.len()))
                })
                .collect::<Vec<_>>();
            let commit = end == stroke.samples.len();
            let started = Instant::now();
            let submit = canvas.submit(&events);
            let draw = canvas.draw();
            let mut submit_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            submit?;
            draw?;
            submit_micros += canvas.drain_submitted()?;
            let drained_micros = started.elapsed().as_micros();
            canvas.wait_idle()?;
            if std::env::var_os("CAPY_TRACE_SLOW_FRAMES").is_some()
                && started.elapsed().as_millis() > 8
            {
                eprintln!(
                    "slow frame start={start} commit={commit} measured={} cpu_us={submit_micros} drained_us={drained_micros} completed_us={}",
                    measurements.is_some(),
                    started.elapsed().as_micros()
                );
            }
            if let Some(output) = measurements.as_deref_mut() {
                output.push(FrameMeasurement {
                    backing_reserved_bytes: canvas.raster_metrics().raster_backing_reserved_bytes,
                    submit_micros,
                    completed_micros: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    commit,
                });
            }
        }
    }
    Ok(())
}

fn summarize(
    kind: ScenarioKind,
    repeats: usize,
    measurements: &[FrameMeasurement],
    metrics: &GpuRasterMetrics,
) -> BenchResult {
    let mut move_times: Vec<u64> = measurements
        .iter()
        .filter(|sample| !sample.commit)
        .map(|sample| sample.completed_micros)
        .collect();
    move_times.sort_unstable();
    let mut submit_times: Vec<u64> = measurements
        .iter()
        .filter(|sample| !sample.commit)
        .map(|sample| sample.submit_micros)
        .collect();
    submit_times.sort_unstable();
    let mut commit_times: Vec<u64> = measurements
        .iter()
        .filter(|sample| sample.commit)
        .map(|sample| sample.completed_micros)
        .collect();
    commit_times.sort_unstable();
    let quantile = |fraction: f32| {
        let index = ((move_times.len().saturating_sub(1)) as f32 * fraction).round() as usize;
        move_times.get(index).copied().unwrap_or(0)
    };
    BenchResult {
        samples: measurements.to_vec(),
        backing_reserved_bytes: measurements
            .iter()
            .map(|m| m.backing_reserved_bytes)
            .max()
            .unwrap_or(0),
        name: kind.name(),
        repeats,
        frames: measurements.len(),
        p50_micros: quantile(0.50),
        p95_micros: quantile(0.95),
        p99_micros: quantile(0.99),
        max_micros: move_times.last().copied().unwrap_or(0),
        submit_p95_micros: quantile_sorted(&submit_times, 0.95),
        submit_p50_micros: quantile_sorted(&submit_times, 0.5),
        submit_p99_micros: quantile_sorted(&submit_times, 0.99),
        commit_submit_p99_micros: {
            let mut times: Vec<_> = measurements
                .iter()
                .filter(|m| m.commit)
                .map(|m| m.submit_micros)
                .collect();
            times.sort_unstable();
            quantile_sorted(&times, 0.99)
        },
        over_budget: move_times
            .iter()
            .filter(|micros| **micros > FRAME_BUDGET_MICROS)
            .count(),
        commit_over_budget: commit_times
            .iter()
            .filter(|micros| **micros > FRAME_BUDGET_MICROS)
            .count(),
        dabs: metrics.dabs,
        candidate_pixels: metrics.raster_candidate_pixels,
        composited_pixels: metrics.composited_pixels,
        paint_pages: metrics.paint_pages,
        preview_pages: metrics.preview_pages,
        coverage_pages: metrics.coverage_pages,
        material_pages: metrics.material_pages,
        storage_bytes: metrics
            .paint_storage_bytes
            .saturating_add(metrics.preview_storage_bytes)
            .saturating_add(metrics.destination_storage_bytes)
            .saturating_add(metrics.paint_state_storage_bytes)
            .saturating_add(metrics.composite_storage_bytes),
        commit_p99_micros: quantile_sorted(&commit_times, 0.99),
    }
}

fn print_result(result: &BenchResult) {
    println!(
        "{:<28} frames {:>4}  move p50 {:>7.3} ms  p95 {:>7.3} ms  p99 {:>7.3} ms  pen-up p99 {:>7.3} ms  submit p95 {:>7.3} ms  >8.33ms move/pen {:>3}/{:<3}",
        result.name,
        result.frames,
        result.p50_micros as f64 / 1000.0,
        result.p95_micros as f64 / 1000.0,
        result.p99_micros as f64 / 1000.0,
        result.commit_p99_micros as f64 / 1000.0,
        result.submit_p95_micros as f64 / 1000.0,
        result.over_budget,
        result.commit_over_budget,
    );
}

// Serialize the measurements already collected by the timing loop, after all
// scenarios finish. Retaining individual samples permits matched-stroke and
// tail analysis without adding logging or file I/O to frame creation.
fn write_frame_samples(path: &Path, results: &[BenchResult]) -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(output, "scenario,space,depth,repetition,stroke,frame,commit,submit_us,completed_us,backing_reserved_bytes")?;
    let (space, depth) = DOCUMENT_COLOR.get().copied().unwrap_or((0, 8));
    for result in results {
        let per_repeat = result.samples.len() / result.repeats;
        let mut stroke = 1;
        for (i, sample) in result.samples.iter().enumerate() {
            if i % per_repeat == 0 { stroke = 1; }
            writeln!(output, "{},{},{},{},{},{},{},{},{},{}",
                result.name, space, depth, i / per_repeat + 1, stroke, i % per_repeat + 1,
                sample.commit, sample.submit_micros, sample.completed_micros, sample.backing_reserved_bytes)?;
            if sample.commit { stroke += 1; }
        }
    }
    output.flush()?;
    Ok(())
}

fn write_report(path: &Path, results: &[BenchResult]) -> Result<(), Box<dyn Error>> {
    let probe = Canvas::new().map_err(|error| format!("GPU probe failed: {error}"))?;
    let gpu = probe.engine.backend().adapter_info();
    let mode = document_mode();
    let mut report = format!(
        "# GPU raster benchmark — 4096×4096\n\n\
         Adapter: `{}`; backend code {}; device type code {}.\n\n\
         Document: {mode}.\n\n\
         Release build with debug symbols. Each frame submits eight simulated coalesced pen samples to `CanvasEngine`, renders one frame, then waits for that submission to complete. Every scenario has at least 32 visible paint layers. Each repetition creates a fresh canvas, warms the exact scenario pipeline, undoes the warm-up stroke, and contributes every measured frame to the reported distribution. Setup, shader/pipeline creation, canvas allocation, scenario warm-up/undo, brush selection, layer creation, and PNG export are outside the timing window. Submit latency is the production non-blocking path; completed-work latency serializes each measured frame to isolate its GPU work. Concurrent system/GPU load is not controlled, so these are reproducible workload references rather than cross-machine scores.\n\n\
         | scenario | state features | repeats | frames | move completed p50 ms | move completed p95 ms | move completed p99 ms | pen-up completed p99 ms | max move ms | submit p95 ms | move/pen-up frames > 8.33 ms | dabs | conservative contact Mpx | composite visits Mpx | paint pages | coverage pages | material pages | preview pages | resident canvas MiB |\n\
         |---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        gpu.name, gpu.backend, gpu.device_type,
    );
    for result in results {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {}/{} | {} | {:.1} | {:.1} | {} | {} | {} | {} | {:.1} |\n",
            result.name,
            ScenarioKind::parse(result.name).map_or("existing path", ScenarioKind::features),
            result.repeats,
            result.frames,
            result.p50_micros as f64 / 1000.0,
            result.p95_micros as f64 / 1000.0,
            result.p99_micros as f64 / 1000.0,
            result.commit_p99_micros as f64 / 1000.0,
            result.max_micros as f64 / 1000.0,
            result.submit_p95_micros as f64 / 1000.0,
            result.over_budget,
            result.commit_over_budget,
            result.dabs,
            result.candidate_pixels as f64 / 1_000_000.0,
            result.composited_pixels as f64 / 1_000_000.0,
            result.paint_pages,
            result.coverage_pages,
            result.material_pages,
            result.preview_pages,
            result.storage_bytes as f64 / (1024.0 * 1024.0),
        ));
    }
    report.push_str("\nCPU input submission and frame creation exclude GPU/capture-capacity waits; deferred input is drained before recording completion. Warm-up undo must submit its actual restoration frame before measurement. Background tile backing remains asynchronous.\n\n| scenario | CPU p50 ms | CPU p95 ms | CPU p99 ms | pen-up CPU p99 ms | capture allocated/reserved peak MiB |\n|---|---:|---:|---:|---:|---:|\n");
    for result in results {
        report.push_str(&format!(
            "| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} |\n",
            result.name,
            result.submit_p50_micros as f64 / 1000.,
            result.submit_p95_micros as f64 / 1000.,
            result.submit_p99_micros as f64 / 1000.,
            result.commit_submit_p99_micros as f64 / 1000.,
            result.backing_reserved_bytes as f64 / 1048576.
        ));
    }
    report.push_str(
        "\nThe 120 Hz budget is 8.33 ms for both move and pen-up work. These offscreen completed-work results exclude surface acquisition and presentation scheduling; target-device acceptance still requires input-to-present traces. Conservative contact pixels sum rotated contact bounding rectangles.\n",
    );
    let pass = results.iter().all(|result| {
        result.p99_micros < FRAME_BUDGET_MICROS && result.commit_p99_micros < FRAME_BUDGET_MICROS
    });
    report.push_str(&format!(
        "\n120 Hz completed-work gate: **{}**.\n",
        if pass { "PASS" } else { "FAIL" }
    ));
    fs::write(path, report)?;
    Ok(())
}

fn write_gallery(output_dir: &Path, results: &[BenchResult]) -> Result<(), Box<dyn Error>> {
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>Layer GPU brush gallery</title>\
         <style>body{font:16px system-ui;background:#18191c;color:#eee;margin:32px}\
         main{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:24px}\
         figure{margin:0;background:#24262b;padding:12px;border-radius:8px}\
         img{display:block;width:100%;height:auto;background:white}figcaption{padding:10px 2px 2px}</style>\
         <h1>Layer GPU brush benchmark gallery</h1><main>",
    );
    for result in results {
        let label = result.name.replace('_', " ");
        html.push_str(&format!(
            "<figure><img src=\"{}.png\" alt=\"{} brush benchmark\"><figcaption>{}</figcaption></figure>",
            result.name, label, label
        ));
    }
    html.push_str("</main>");
    fs::write(output_dir.join("index.html"), html)?;
    Ok(())
}

fn brush(preset: Preset, diameter: f32, opacity: f32, color: [f32; 4]) -> Brush {
    Brush {
        preset,
        diameter,
        opacity,
        color,
    }
}

fn prepare_destination_base(canvas: &mut Canvas) -> Result<(), Box<dyn Error>> {
    let underpaint = vec![
        StrokeSpec {
            layer: 1,
            brush: brush(Preset::Paintbrush, 760.0, 0.92, [0.62, 0.025, 0.008, 1.0]),
            samples: lissajous(260, 1360.0, 1100.0, 0.0, 0.4),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(Preset::Airbrush, 620.0, 0.72, [0.015, 0.18, 0.68, 1.0]),
            samples: bezier(
                260,
                [
                    (320.0, 3280.0),
                    (1260.0, 440.0),
                    (2840.0, 3720.0),
                    (3760.0, 720.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(Preset::Chalk, 130.0, 0.8, [0.82, 0.42, 0.015, 1.0]),
            samples: lissajous(300, 1160.0, 880.0, 1.1, 0.0),
        },
    ];
    run_strokes(canvas, &underpaint, None).map_err(Into::into)
}

fn prepare_painter_base(canvas: &mut Canvas) -> Result<(), Box<dyn Error>> {
    // Localized, opaque swatches expose pickup and mixing while leaving enough
    // clean canvas for grain, glaze, coverage, and edge behavior to be read.
    let underpaint = vec![
        StrokeSpec {
            layer: 1,
            brush: brush(Preset::Marker, 620.0, 0.86, [0.48, 0.025, 0.010, 1.0]),
            samples: bezier(
                120,
                [
                    (520.0, 1200.0),
                    (1180.0, 820.0),
                    (1740.0, 1360.0),
                    (2180.0, 1080.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(Preset::Marker, 660.0, 0.82, [0.012, 0.055, 0.42, 1.0]),
            samples: bezier(
                120,
                [
                    (1600.0, 2860.0),
                    (2250.0, 2500.0),
                    (2880.0, 3160.0),
                    (3520.0, 2700.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(Preset::Marker, 440.0, 0.76, [0.64, 0.24, 0.008, 1.0]),
            samples: bezier(
                100,
                [
                    (760.0, 3320.0),
                    (1460.0, 2540.0),
                    (2760.0, 1660.0),
                    (3420.0, 760.0),
                ],
            ),
        },
    ];
    run_strokes(canvas, &underpaint, None).map_err(Into::into)
}

fn lissajous(
    count: usize,
    radius_x: f32,
    radius_y: f32,
    phase_x: f32,
    phase_y: f32,
) -> Vec<Sample> {
    (0..count)
        .map(|index| {
            let t = index as f32 / (count - 1) as f32 * std::f32::consts::TAU;
            Sample {
                x: 2048.0 + radius_x * (t * 2.0 + phase_x).sin(),
                y: 2048.0 + radius_y * (t * 3.0 + phase_y).sin(),
                pressure: (0.58 + 0.34 * (t * 5.0 + 0.2).sin()).clamp(0.08, 1.0),
            }
        })
        .collect()
}

fn bezier(count: usize, control: [(f32, f32); 4]) -> Vec<Sample> {
    (0..count)
        .map(|index| {
            let t = index as f32 / (count - 1) as f32;
            let inverse = 1.0 - t;
            let a = inverse * inverse * inverse;
            let b = 3.0 * inverse * inverse * t;
            let c = 3.0 * inverse * t * t;
            let d = t * t * t;
            Sample {
                x: a * control[0].0 + b * control[1].0 + c * control[2].0 + d * control[3].0,
                y: a * control[0].1 + b * control[1].1 + c * control[2].1 + d * control[3].1,
                pressure: 0.18 + 0.8 * (std::f32::consts::PI * t).sin(),
            }
        })
        .collect()
}

fn pencil_hatching(layer: u64) -> Vec<StrokeSpec> {
    let mut strokes = Vec::with_capacity(12);
    for line in 0..12 {
        let offset = 320.0 + line as f32 * 285.0;
        let reverse = line % 2 != 0;
        let samples = (0..360)
            .map(|index| {
                let t = index as f32 / 359.0;
                let t = if reverse { 1.0 - t } else { t };
                Sample {
                    x: 240.0 + t * 3600.0,
                    y: (offset + (t * std::f32::consts::TAU * 1.4).sin() * 150.0)
                        .clamp(120.0, 3976.0),
                    pressure: 0.22 + 0.64 * (std::f32::consts::PI * t).sin(),
                }
            })
            .collect();
        strokes.push(StrokeSpec {
            layer,
            brush: brush(Preset::Pencil, 84.0, 0.74, [0.012, 0.014, 0.018, 1.0]),
            samples,
        });
    }
    strokes
}

fn quantile_sorted(values: &[u64], fraction: f32) -> u64 {
    let index = ((values.len().saturating_sub(1)) as f32 * fraction).round() as usize;
    values.get(index).copied().unwrap_or(0)
}
