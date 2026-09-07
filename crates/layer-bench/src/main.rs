use layer_ffi::{
    LayerBrushSettings, LayerCanvas, LayerCanvasConfig, LayerCanvasMetrics, LayerGpuInfo,
    LayerInstantFeedbackSettings, LayerPenEvent, LayerStatus, layer_canvas_add_paint_layer,
    layer_canvas_copy_rgba8_srgb, layer_canvas_create, layer_canvas_destroy,
    layer_canvas_draw_frame, layer_canvas_draw_frame_for, layer_canvas_get_gpu_info,
    layer_canvas_get_metrics, layer_canvas_set_active_layer, layer_canvas_set_brush,
    layer_canvas_set_instant_feedback, layer_canvas_set_layer_opacity,
    layer_canvas_submit_pen_events, layer_canvas_undo, layer_canvas_wait_idle,
};
use std::error::Error;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Instant;

mod previews;

const WIDTH: u32 = 4096;
const HEIGHT: u32 = 4096;
const EVENTS_PER_FRAME: usize = 8;
const FRAME_BUDGET_MICROS: u64 = 8_333;

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
    const INTERACTION: [Self; 10] = [
        Self::Smudge,
        Self::WetRound,
        Self::OpaqueGouache,
        Self::WatercolorWash,
        Self::WetWatercolor,
        Self::LoadedOil,
        Self::PaletteKnife,
        Self::NaturalBlender,
        Self::LiquifyPush,
        Self::LiquifyTwirl,
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
    scenarios: Vec<ScenarioKind>,
    output_dir: PathBuf,
    report_path: PathBuf,
    feedback_comparison: bool,
    brush_validation: Option<BrushValidation>,
    repeats: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrushValidation {
    Blank,
    Destination,
    Watercolor,
    Transport,
}

#[derive(Clone, Copy)]
struct Sample {
    x: f32,
    y: f32,
    pressure: f32,
}

struct StrokeSpec {
    layer: u64,
    brush: LayerBrushSettings,
    samples: Vec<Sample>,
}

#[derive(Clone, Copy)]
struct FrameMeasurement {
    submit_micros: u64,
    completed_micros: u64,
    commit: bool,
    tip_gap_px: f32,
    correction_px: f32,
}

struct BenchResult {
    name: &'static str,
    repeats: usize,
    frames: usize,
    p50_micros: u64,
    p95_micros: u64,
    p99_micros: u64,
    max_micros: u64,
    submit_p95_micros: u64,
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

struct FeedbackBenchResult {
    name: &'static str,
    off: BenchResult,
    on: BenchResult,
    tip_gap_p95_px: f32,
    tip_gap_p99_px: f32,
    correction_p95_px: f32,
    correction_p99_px: f32,
    preview_dabs: u64,
}

struct Canvas {
    raw: *mut LayerCanvas,
    extent: [u32; 2],
    sequence: u64,
    real_timestamp_ns: u64,
}

impl Canvas {
    fn new() -> Result<Self, String> {
        Self::with_stroke_capacity(65_536)
    }

    fn with_stroke_capacity(stroke_point_capacity: u32) -> Result<Self, String> {
        let config = LayerCanvasConfig {
            document_width: WIDTH,
            document_height: HEIGHT,
            surface_width: WIDTH,
            surface_height: HEIGHT,
            input_capacity: 16_384,
            stroke_point_capacity,
            dab_capacity: 65_536,
            batch_capacity: 64,
            background_rgba_linear: [0.93, 0.92, 0.88, 1.0],
        };
        Self::configured(config)
    }

    fn configured(config: LayerCanvasConfig) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        check(
            unsafe { layer_canvas_create(&config, &mut raw) },
            "create canvas",
        )?;
        let mut canvas = Self {
            raw,
            extent: [config.surface_width, config.surface_height],
            sequence: 0,
            real_timestamp_ns: 0,
        };
        canvas.draw()?;
        canvas.wait_idle()?;
        Ok(canvas)
    }

    fn draw(&mut self) -> Result<(), String> {
        check(unsafe { layer_canvas_draw_frame(self.raw) }, "draw frame")
    }

    fn draw_for(&mut self, now_ns: u64, presentation_ns: u64) -> Result<(), String> {
        check(
            unsafe { layer_canvas_draw_frame_for(self.raw, now_ns, presentation_ns) },
            "draw predicted frame",
        )
    }

    fn wait_idle(&mut self) -> Result<(), String> {
        check(
            unsafe { layer_canvas_wait_idle(self.raw) },
            "wait for GPU completion",
        )
    }

    fn gpu_info(&self) -> Result<LayerGpuInfo, String> {
        let mut info = LayerGpuInfo::default();
        check(
            unsafe { layer_canvas_get_gpu_info(self.raw, &mut info) },
            "get GPU info",
        )?;
        Ok(info)
    }

    fn undo(&mut self) -> Result<(), String> {
        let mut changed = 0;
        check(
            unsafe { layer_canvas_undo(self.raw, &mut changed) },
            "undo warm-up stroke",
        )?;
        if changed == 0 {
            return Err("warm-up stroke was not committed".to_owned());
        }
        self.draw()?;
        self.wait_idle()
    }

    fn set_brush(&mut self, brush: LayerBrushSettings) -> Result<(), String> {
        check(
            unsafe { layer_canvas_set_brush(self.raw, &brush) },
            "set brush",
        )
    }

    fn set_feedback(&mut self, enabled: bool) -> Result<(), String> {
        let settings = LayerInstantFeedbackSettings {
            enabled: u8::from(enabled),
            ..LayerInstantFeedbackSettings::default()
        };
        check(
            unsafe { layer_canvas_set_instant_feedback(self.raw, &settings) },
            "set instant feedback",
        )
    }

    fn set_active_layer(&mut self, layer: u64) -> Result<(), String> {
        check(
            unsafe { layer_canvas_set_active_layer(self.raw, layer) },
            "set active layer",
        )
    }

    fn add_layer(&mut self, name: &str, index: usize) -> Result<u64, String> {
        let mut id = 0;
        check(
            unsafe {
                layer_canvas_add_paint_layer(self.raw, name.as_ptr(), name.len(), index, &mut id)
            },
            "add paint layer",
        )?;
        Ok(id)
    }

    fn set_layer_opacity(&mut self, layer: u64, opacity: f32) -> Result<(), String> {
        check(
            unsafe { layer_canvas_set_layer_opacity(self.raw, layer, opacity) },
            "set layer opacity",
        )
    }

    fn submit(&mut self, events: &[LayerPenEvent]) -> Result<(), String> {
        let mut accepted = 0;
        check(
            unsafe {
                layer_canvas_submit_pen_events(
                    self.raw,
                    events.as_ptr(),
                    events.len(),
                    &mut accepted,
                )
            },
            "submit pen events",
        )?;
        if accepted != events.len() {
            return Err(format!(
                "event ingress accepted {accepted} of {} records",
                events.len()
            ));
        }
        Ok(())
    }

    fn metrics(&self) -> Result<LayerCanvasMetrics, String> {
        let mut metrics = LayerCanvasMetrics::default();
        check(
            unsafe { layer_canvas_get_metrics(self.raw, &mut metrics) },
            "get metrics",
        )?;
        Ok(metrics)
    }

    fn write_png(&mut self, path: &Path) -> Result<(), Box<dyn Error>> {
        let [width, height] = self.extent;
        let stride = width as usize * 4;
        let mut rgba = vec![0; stride * height as usize];
        check(
            unsafe {
                layer_canvas_copy_rgba8_srgb(self.raw, rgba.as_mut_ptr(), rgba.len(), stride)
            },
            "copy canvas pixels",
        )?;
        let file = BufWriter::new(File::create(path)?);
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;
        Ok(())
    }

    fn next_event(&mut self, sample: Sample, phase: u32) -> LayerPenEvent {
        self.real_timestamp_ns = self.real_timestamp_ns.saturating_add(1_000_000);
        self.event_at(sample, phase, self.real_timestamp_ns, false)
    }

    fn predicted_event(&mut self, sample: Sample, timestamp_ns: u64) -> LayerPenEvent {
        self.event_at(sample, 2, timestamp_ns, true)
    }

    fn event_at(
        &mut self,
        sample: Sample,
        phase: u32,
        timestamp_ns: u64,
        predicted: bool,
    ) -> LayerPenEvent {
        self.sequence = self.sequence.saturating_add(1);
        LayerPenEvent {
            device_id: 1,
            sequence: self.sequence,
            timestamp_ns,
            view_revision: 0,
            x_physical_px: sample.x,
            y_physical_px: sample.y,
            pressure: sample.pressure,
            tilt_x_radians: 0.0,
            tilt_y_radians: 0.0,
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: 1,
            flags: 2 | u32::from(predicted),
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe { layer_canvas_destroy(self.raw) };
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() == Some("--brush-previews") {
        return previews::generate(Path::new("apps/layer-web/brush-previews"));
    }
    let options = parse_options()?;
    if let Some(validation) = options.brush_validation {
        run_brush_validation(&options.output_dir, validation)?;
        return Ok(());
    }
    if options.feedback_comparison {
        run_feedback_comparison(&options)?;
        return Ok(());
    }
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
    write_gallery(&options.output_dir, &results)?;

    Ok(())
}

fn parse_options() -> Result<Options, Box<dyn Error>> {
    let mut scenarios = ScenarioKind::LEGACY.to_vec();
    let mut output_dir = PathBuf::from("artifacts/images");
    let mut report_path = PathBuf::from("artifacts/benchmarks/gpu-4k.md");
    let mut feedback_comparison = false;
    let mut brush_validation = None;
    let mut repeats = 1;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
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
            "--feedback-comparison" => {
                feedback_comparison = true;
                report_path = PathBuf::from("artifacts/benchmarks/instant-feedback.md");
            }
            "--brush-validation" => {
                brush_validation = Some(
                    match arguments
                        .next()
                        .ok_or(
                            "--brush-validation needs blank, destination, watercolor, or transport",
                        )?
                        .as_str()
                    {
                        "blank" => BrushValidation::Blank,
                        "destination" => BrushValidation::Destination,
                        "watercolor" => BrushValidation::Watercolor,
                        "transport" => BrushValidation::Transport,
                        _ => {
                            return Err(
                                "--brush-validation needs blank, destination, watercolor, or transport".into(),
                            );
                        }
                    },
                );
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
                     [--feedback-comparison] [--brush-validation blank|destination|watercolor|transport]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(Options {
        scenarios,
        output_dir,
        report_path,
        feedback_comparison,
        brush_validation,
        repeats,
    })
}

fn run_brush_validation(
    output_dir: &Path,
    validation: BrushValidation,
) -> Result<(), Box<dyn Error>> {
    if validation == BrushValidation::Watercolor {
        return run_watercolor_validation(output_dir);
    }
    if validation == BrushValidation::Transport {
        return run_transport_validation(output_dir);
    }
    fs::create_dir_all(output_dir)?;
    let scenarios: &[ScenarioKind] = match validation {
        BrushValidation::Blank => &ScenarioKind::PAINTER,
        BrushValidation::Destination => &ScenarioKind::INTERACTION,
        BrushValidation::Watercolor => unreachable!(),
        BrushValidation::Transport => unreachable!(),
    };
    let mut names = Vec::with_capacity(scenarios.len() + 2);
    let mut reference = Canvas::new()?;
    match validation {
        BrushValidation::Blank => {
            reference.write_png(&output_dir.join("blank_reference.png"))?;
            names.push("blank_reference");
        }
        BrushValidation::Destination => {
            prepare_validation_destination(&mut reference)?;
            reference.wait_idle()?;
            reference.write_png(&output_dir.join("destination_reference.png"))?;
            names.push("destination_reference");

            let mut liquify_reference = Canvas::new()?;
            prepare_liquify_destination(&mut liquify_reference)?;
            liquify_reference.wait_idle()?;
            liquify_reference.write_png(&output_dir.join("liquify_reference.png"))?;
            names.push("liquify_reference");
        }
        BrushValidation::Watercolor => unreachable!(),
        BrushValidation::Transport => unreachable!(),
    }
    for &kind in scenarios {
        eprintln!("rendering {} {:?} validation", kind.name(), validation);
        let mut canvas = Canvas::new()?;
        canvas.set_feedback(false)?;
        if validation == BrushValidation::Destination {
            if matches!(kind, ScenarioKind::LiquifyPush | ScenarioKind::LiquifyTwirl) {
                prepare_liquify_destination(&mut canvas)?;
            } else {
                prepare_validation_destination(&mut canvas)?;
            }
        }
        let strokes = match validation {
            BrushValidation::Blank => blank_validation_strokes(kind),
            BrushValidation::Destination => destination_validation_strokes(kind),
            BrushValidation::Watercolor => unreachable!(),
            BrushValidation::Transport => unreachable!(),
        };
        run_strokes(&mut canvas, &strokes, None)?;
        canvas.wait_idle()?;
        canvas.write_png(&output_dir.join(format!("{}.png", kind.name())))?;
        names.push(kind.name());
    }
    write_validation_gallery(output_dir, validation, &names)?;
    Ok(())
}

const WATERCOLOR_VALIDATION_NAMES: [&str; 12] = [
    "01_single_wash",
    "02_pressure_levels",
    "03_opacity_levels",
    "04_diameter_levels",
    "05_red_blue_mix",
    "06_yellow_blue_mix",
    "07_three_pigment_mix",
    "08_repeated_glazing",
    "09_loop_and_inner_edge",
    "10_wet_pull_curves",
    "11_dynamic_pressure_curve",
    "12_lower_layer_isolation",
];

fn run_watercolor_validation(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(output_dir)?;
    for (index, name) in WATERCOLOR_VALIDATION_NAMES.iter().enumerate() {
        eprintln!("rendering watercolor validation {}/12: {name}", index + 1);
        let mut canvas = Canvas::new()?;
        canvas.set_feedback(false)?;
        let strokes = watercolor_validation_strokes(index, &mut canvas)?;
        run_strokes(&mut canvas, &strokes, None)?;
        canvas.wait_idle()?;
        canvas.write_png(&output_dir.join(format!("{name}.png")))?;
    }
    write_validation_gallery(
        output_dir,
        BrushValidation::Watercolor,
        &WATERCOLOR_VALIDATION_NAMES,
    )?;
    Ok(())
}

const TRANSPORT_VALIDATION_NAMES: [&str; 12] = [
    "01_long_narrow_low_16px",
    "02_long_narrow_medium_48px",
    "03_long_narrow_high_88px",
    "04_long_broad_low_16px",
    "05_long_broad_medium_48px",
    "06_long_broad_high_88px",
    "07_short_narrow_low_16px",
    "08_short_narrow_medium_48px",
    "09_short_narrow_high_88px",
    "10_short_broad_low_16px",
    "11_short_broad_medium_48px",
    "12_short_broad_high_88px",
];

fn run_transport_validation(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(output_dir)?;
    for (index, name) in TRANSPORT_VALIDATION_NAMES.iter().enumerate() {
        eprintln!("rendering transport validation {}/12: {name}", index + 1);
        let field = (index / 3 + 1) as u32;
        let level = index % 3;
        let (distance, wet_flow, dry_flow) = match level {
            0 => (16.0, 0.30, 0.16),
            1 => (48.0, 0.68, 0.48),
            _ => (88.0, 1.0, 0.86),
        };
        let mut canvas = Canvas::new()?;
        canvas.set_feedback(false)?;

        // Establish an already-wet blue field without transport. A red stroke
        // inside it exposes wet-to-wet mixing; the isolated gold stroke below
        // exposes wet-to-dry capillary bleed. Several submitted updates make
        // the event-driven, incremental behavior visible without simulating
        // flow between input events.
        let mut base = transport_brush(field, 0.0, 0.0, 0.0, [0.0, 0.012, 0.82, 1.0]);
        base.diameter_document_px = 1320.0;
        base.opacity = 0.88;
        // Represent an older but still wet wash. Fresh strokes below recharge
        // much more water and therefore establish the gradient under test.
        base.transport_water_load = 0.42;
        let mut active = transport_brush(field, distance, wet_flow, dry_flow, [1.0, 0.0, 0.0, 1.0]);
        active.diameter_document_px = 150.0;
        active.opacity = 1.0;
        active.transport_water_load = 0.98;
        let mut accent = transport_brush(
            field,
            distance,
            wet_flow * 0.78,
            dry_flow,
            [1.0, 0.08, 0.0, 1.0],
        );
        accent.diameter_document_px = 170.0;
        accent.opacity = 1.0;
        accent.transport_water_load = 0.98;
        let strokes = vec![
            StrokeSpec {
                layer: 1,
                brush: base,
                samples: linear_samples(2, (1150.0, 1400.0), (1150.0, 1400.0), 1.0, 1.0),
            },
            StrokeSpec {
                layer: 1,
                brush: active,
                samples: linear_samples(48, (1150.0, 900.0), (1150.0, 1850.0), 1.0, 1.0),
            },
            StrokeSpec {
                layer: 1,
                brush: accent,
                samples: linear_samples(48, (2950.0, 2300.0), (2950.0, 3250.0), 1.0, 1.0),
            },
        ];
        run_strokes(&mut canvas, &strokes, None)?;
        canvas.wait_idle()?;
        canvas.write_png(&output_dir.join(format!("{name}.png")))?;
    }
    write_validation_gallery(
        output_dir,
        BrushValidation::Transport,
        &TRANSPORT_VALIDATION_NAMES,
    )?;
    Ok(())
}

fn watercolor_validation_strokes(
    index: usize,
    canvas: &mut Canvas,
) -> Result<Vec<StrokeSpec>, Box<dyn Error>> {
    let red = [0.68, 0.018, 0.012, 1.0];
    let blue = [0.008, 0.055, 0.68, 1.0];
    let yellow = [0.92, 0.48, 0.008, 1.0];
    let green = [0.008, 0.42, 0.16, 1.0];
    let violet = [0.34, 0.018, 0.58, 1.0];
    let horizontal = |y, pressure| linear_samples(180, (420.0, y), (3670.0, y), pressure, pressure);
    let cross_a = || {
        bezier(
            220,
            [
                (430.0, 820.0),
                (1220.0, 3220.0),
                (2830.0, 820.0),
                (3660.0, 3220.0),
            ],
        )
    };
    let cross_b = || {
        bezier(
            220,
            [
                (430.0, 3220.0),
                (1240.0, 820.0),
                (2820.0, 3220.0),
                (3660.0, 820.0),
            ],
        )
    };

    let strokes = match index {
        0 => vec![StrokeSpec {
            layer: 1,
            brush: brush(20, 720.0, 0.82, red),
            samples: bezier(
                220,
                [
                    (420.0, 2350.0),
                    (1280.0, 900.0),
                    (2820.0, 3160.0),
                    (3670.0, 1740.0),
                ],
            ),
        }],
        1 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(20, 500.0, 0.86, red),
                samples: horizontal(900.0, 0.18),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 500.0, 0.86, blue),
                samples: horizontal(2048.0, 0.52),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 500.0, 0.86, green),
                samples: horizontal(3190.0, 0.96),
            },
        ],
        2 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(20, 520.0, 0.28, red),
                samples: horizontal(900.0, 0.78),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 520.0, 0.58, blue),
                samples: horizontal(2048.0, 0.78),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 520.0, 0.92, violet),
                samples: horizontal(3190.0, 0.78),
            },
        ],
        3 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(20, 150.0, 0.84, red),
                samples: horizontal(850.0, 0.82),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 380.0, 0.84, yellow),
                samples: horizontal(2000.0, 0.82),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 760.0, 0.84, blue),
                samples: horizontal(3220.0, 0.82),
            },
        ],
        4 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(20, 660.0, 0.78, red),
                samples: cross_a(),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 580.0, 0.82, blue),
                samples: cross_b(),
            },
        ],
        5 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(21, 700.0, 0.78, yellow),
                samples: cross_a(),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(21, 560.0, 0.86, blue),
                samples: cross_b(),
            },
        ],
        6 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(21, 620.0, 0.76, red),
                samples: cross_a(),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(21, 540.0, 0.80, yellow),
                samples: cross_b(),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(21, 430.0, 0.84, blue),
                samples: horizontal(2048.0, 0.88),
            },
        ],
        7 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(20, 820.0, 0.34, red),
                samples: horizontal(1460.0, 0.74),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 760.0, 0.34, yellow),
                samples: horizontal(2048.0, 0.74),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(20, 700.0, 0.34, blue),
                samples: horizontal(2630.0, 0.74),
            },
        ],
        8 => vec![StrokeSpec {
            layer: 1,
            brush: brush(20, 420.0, 0.86, violet),
            samples: circular_samples(360, (2048.0, 2048.0), 1120.0, 1.0),
        }],
        9 => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(21, 760.0, 0.72, red),
                samples: lissajous(260, 1320.0, 900.0, 0.3, 0.7),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(21, 620.0, 0.80, blue),
                samples: bezier(
                    240,
                    [
                        (350.0, 2700.0),
                        (1420.0, 300.0),
                        (2740.0, 3800.0),
                        (3760.0, 1320.0),
                    ],
                ),
            },
        ],
        10 => {
            let mut samples = bezier(
                300,
                [
                    (330.0, 3100.0),
                    (1150.0, 160.0),
                    (2940.0, 3940.0),
                    (3760.0, 980.0),
                ],
            );
            let last = samples.len().saturating_sub(1).max(1) as f32;
            for (sample_index, sample) in samples.iter_mut().enumerate() {
                let phase = sample_index as f32 / last;
                sample.pressure = 0.12 + 0.86 * (phase * std::f32::consts::PI).sin().abs();
            }
            vec![StrokeSpec {
                layer: 1,
                brush: brush(20, 780.0, 0.88, green),
                samples,
            }]
        }
        11 => {
            let lower = vec![
                StrokeSpec {
                    layer: 1,
                    brush: brush(1, 980.0, 1.0, red),
                    samples: linear_samples(2, (900.0, 2048.0), (900.0, 2048.0), 1.0, 1.0),
                },
                StrokeSpec {
                    layer: 1,
                    brush: brush(1, 980.0, 1.0, yellow),
                    samples: linear_samples(2, (2048.0, 2048.0), (2048.0, 2048.0), 1.0, 1.0),
                },
                StrokeSpec {
                    layer: 1,
                    brush: brush(1, 980.0, 1.0, blue),
                    samples: linear_samples(2, (3190.0, 2048.0), (3190.0, 2048.0), 1.0, 1.0),
                },
            ];
            run_strokes(canvas, &lower, None)?;
            let watercolor_layer = canvas.add_layer("Wet watercolor", 0)?;
            canvas.draw()?;
            vec![StrokeSpec {
                layer: watercolor_layer,
                brush: brush(21, 620.0, 0.76, green),
                samples: horizontal(2048.0, 0.84),
            }]
        }
        _ => unreachable!("watercolor validation has exactly twelve cases"),
    };
    Ok(strokes)
}

fn measure_scenario(
    kind: ScenarioKind,
    repeats: usize,
) -> Result<(BenchResult, Canvas), Box<dyn Error>> {
    let mut all_measurements = Vec::new();
    let mut aggregate_metrics = LayerCanvasMetrics::default();
    let mut final_canvas = None;
    for repeat in 0..repeats {
        eprintln!("  repetition {}/{}", repeat + 1, repeats);
        let mut canvas = Canvas::new()?;
        canvas.set_feedback(false)?;
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
        let before = canvas.metrics()?;
        let mut measurements = Vec::with_capacity(
            strokes
                .iter()
                .map(|stroke| stroke.samples.len().div_ceil(EVENTS_PER_FRAME))
                .sum(),
        );
        run_strokes(&mut canvas, &strokes, Some(&mut measurements))?;
        let after = canvas.metrics()?;
        merge_metrics_delta(&mut aggregate_metrics, before, after);
        all_measurements.extend(measurements);
        // Retain only the final repetition for the gallery export. Keeping the
        // previous full 4K canvas alive while allocating the next one needlessly
        // doubles benchmark VRAM without changing any measured work.
        if repeat + 1 == repeats {
            final_canvas = Some(canvas);
        }
    }
    Ok((
        summarize(
            kind,
            repeats,
            &all_measurements,
            LayerCanvasMetrics::default(),
            aggregate_metrics,
        ),
        final_canvas.expect("repeats is validated as non-zero"),
    ))
}

fn merge_metrics_delta(
    aggregate: &mut LayerCanvasMetrics,
    before: LayerCanvasMetrics,
    after: LayerCanvasMetrics,
) {
    aggregate.raster_dabs = aggregate
        .raster_dabs
        .saturating_add(after.raster_dabs.saturating_sub(before.raster_dabs));
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
                brush: brush(1, 24.0, 1.0, [0.004, 0.006, 0.009, 1.0]),
                samples: lissajous(1400, 1780.0, 1360.0, 0.0, 0.3),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(1, 13.0, 0.92, [0.025, 0.01, 0.008, 1.0]),
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
                brush: brush(4, 720.0, 0.9, [0.18, 0.035, 0.012, 1.0]),
                samples: lissajous(700, 1500.0, 1320.0, 0.4, 1.1),
            }];
            run_strokes(canvas, &underpaint, None)?;
            vec![
                StrokeSpec {
                    layer: 1,
                    brush: brush(3, 360.0, 0.85, [0.0, 0.0, 0.0, 1.0]),
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
                    brush: brush(3, 620.0, 0.5, [0.0, 0.0, 0.0, 1.0]),
                    samples: lissajous(520, 1300.0, 1000.0, 1.2, 0.2),
                },
            ]
        }
        ScenarioKind::Paintbrush => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(4, 680.0, 0.82, [0.28, 0.025, 0.008, 1.0]),
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
                brush: brush(4, 520.0, 0.72, [0.01, 0.06, 0.24, 1.0]),
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
                brush: brush(4, 880.0, 0.5, [0.5, 0.2, 0.006, 1.0]),
                samples: lissajous(500, 1250.0, 980.0, 0.2, 1.8),
            },
        ],
        ScenarioKind::Airbrush => vec![StrokeSpec {
            layer: 1,
            brush: brush(5, 420.0, 0.82, [0.025, 0.12, 0.56, 1.0]),
            samples: lissajous(520, 1460.0, 1180.0, 0.2, 0.8),
        }],
        ScenarioKind::Chalk => vec![
            StrokeSpec {
                layer: 1,
                brush: brush(6, 150.0, 0.9, [0.68, 0.08, 0.025, 1.0]),
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
                brush: brush(6, 96.0, 0.72, [0.03, 0.18, 0.52, 1.0]),
                samples: lissajous(420, 1320.0, 1040.0, 1.1, 0.0),
            },
        ],
        ScenarioKind::Marker => vec![StrokeSpec {
            layer: 1,
            brush: brush(7, 230.0, 0.78, [0.78, 0.035, 0.12, 0.78]),
            samples: lissajous(480, 1520.0, 1120.0, 0.0, 1.2),
        }],
        ScenarioKind::Spray => vec![StrokeSpec {
            layer: 1,
            brush: brush(8, 42.0, 0.74, [0.025, 0.42, 0.09, 1.0]),
            samples: lissajous(420, 1420.0, 1080.0, 0.9, 0.3),
        }],
        ScenarioKind::DualTexture => vec![StrokeSpec {
            layer: 1,
            brush: brush(9, 340.0, 0.88, [0.09, 0.018, 0.48, 1.0]),
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
                brush: brush(14, 320.0, 0.66, [0.06, 0.22, 0.76, 1.0]),
                samples: lissajous(360, 1280.0, 940.0, 0.6, 1.4),
            }]
        }
        ScenarioKind::Smudge => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(10, 260.0, 0.94, [0.0, 0.0, 0.0, 1.0]),
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
                brush: brush(11, 300.0, 0.86, [0.015, 0.12, 0.62, 1.0]),
                samples: lissajous(360, 1260.0, 920.0, 0.3, 1.0),
            }]
        }
        ScenarioKind::LiquifyPush => {
            prepare_destination_base(canvas)?;
            vec![StrokeSpec {
                layer: 1,
                brush: brush(12, 520.0, 1.0, [0.0; 4]),
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
                brush: brush(13, 720.0, 1.0, [0.0; 4]),
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
                    brush: brush(4, 760.0, 0.76, [0.03, 0.2, 0.16, 1.0]),
                    samples: lissajous(520, 1440.0, 1160.0, 0.8, 0.0),
                },
                StrokeSpec {
                    layer: 1,
                    brush: brush(1, 21.0, 0.95, [0.008, 0.008, 0.012, 1.0]),
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
        ScenarioKind::TexturedFlat => (15, 420.0, 0.88),
        ScenarioKind::DryScumble => (16, 520.0, 0.82),
        ScenarioKind::PastelBlock => (17, 340.0, 0.84),
        ScenarioKind::TransparentGlaze => (18, 620.0, 0.72),
        ScenarioKind::OpaqueGouache => (19, 480.0, 0.92),
        ScenarioKind::WatercolorWash => (20, 700.0, 0.78),
        ScenarioKind::WetWatercolor => (21, 620.0, 0.82),
        ScenarioKind::LoadedOil => (22, 560.0, 0.94),
        ScenarioKind::PaletteKnife => (23, 680.0, 0.90),
        ScenarioKind::NaturalBlender => (24, 520.0, 0.88),
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

fn painter_validation_settings(kind: ScenarioKind) -> (u32, f32) {
    match kind {
        ScenarioKind::Smudge => (10, 360.0),
        ScenarioKind::WetRound => (11, 360.0),
        ScenarioKind::TexturedFlat => (15, 340.0),
        ScenarioKind::DryScumble => (16, 320.0),
        ScenarioKind::PastelBlock => (17, 300.0),
        ScenarioKind::TransparentGlaze => (18, 420.0),
        ScenarioKind::OpaqueGouache => (19, 350.0),
        ScenarioKind::WatercolorWash => (20, 460.0),
        ScenarioKind::WetWatercolor => (21, 420.0),
        ScenarioKind::LoadedOil => (22, 380.0),
        // This preset's 0.24 aspect produces a blade over four times wider
        // than its nominal diameter. At 280 px, each row gets a visible blade
        // while the two validation strokes remain spatially distinct.
        ScenarioKind::PaletteKnife => (23, 280.0),
        ScenarioKind::NaturalBlender => (24, 380.0),
        _ => unreachable!("only paint and smudge scenarios have validation settings"),
    }
}

fn blank_validation_strokes(kind: ScenarioKind) -> Vec<StrokeSpec> {
    let (preset, diameter) = painter_validation_settings(kind);
    vec![
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter, 0.92, [0.025, 0.018, 0.014, 1.0]),
            samples: linear_samples(220, (360.0, 760.0), (3730.0, 760.0), 0.78, 0.78),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter * 0.88, 0.88, [0.025, 0.11, 0.48, 1.0]),
            samples: bezier(
                260,
                [
                    (360.0, 2050.0),
                    (1220.0, 1160.0),
                    (2820.0, 2940.0),
                    (3730.0, 2050.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter * 0.72, 0.84, [0.48, 0.075, 0.018, 1.0]),
            samples: linear_samples(220, (360.0, 3330.0), (3730.0, 3330.0), 0.12, 1.0),
        },
    ]
}

fn destination_validation_strokes(kind: ScenarioKind) -> Vec<StrokeSpec> {
    if kind == ScenarioKind::LiquifyPush {
        return vec![StrokeSpec {
            layer: 1,
            brush: brush(12, 760.0, 1.0, [0.0; 4]),
            samples: bezier(
                320,
                [
                    (480.0, 3200.0),
                    (1260.0, 420.0),
                    (2860.0, 3720.0),
                    (3640.0, 880.0),
                ],
            ),
        }];
    }
    if kind == ScenarioKind::LiquifyTwirl {
        return vec![
            StrokeSpec {
                layer: 1,
                brush: brush(13, 920.0, 1.0, [0.0; 4]),
                samples: circular_samples(220, (1320.0, 1320.0), 210.0, 2.25),
            },
            StrokeSpec {
                layer: 1,
                brush: brush(13, 1060.0, 1.0, [0.0; 4]),
                samples: circular_samples(240, (2780.0, 2780.0), 240.0, 2.5),
            },
        ];
    }
    let (preset, diameter) = painter_validation_settings(kind);
    vec![
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter, 0.92, [0.018, 0.36, 0.10, 1.0]),
            samples: bezier(
                300,
                [
                    (300.0, 1600.0),
                    (1280.0, 1200.0),
                    (2840.0, 2000.0),
                    (3790.0, 1600.0),
                ],
            ),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(preset, diameter * 0.72, 0.88, [0.56, 0.018, 0.28, 1.0]),
            samples: linear_samples(260, (300.0, 2780.0), (3790.0, 2780.0), 0.82, 0.82),
        },
    ]
}

fn prepare_validation_destination(canvas: &mut Canvas) -> Result<(), Box<dyn Error>> {
    // Put the wells on an opaque neutral paint ground. A blender should
    // exchange paint colors here, not accidentally demonstrate transparent
    // alpha transport against the application's visual background.
    let mut strokes = [1600.0, 2780.0]
        .into_iter()
        .map(|y| StrokeSpec {
            layer: 1,
            brush: brush(1, 1050.0, 1.0, [0.82, 0.80, 0.76, 1.0]),
            samples: linear_samples(2, (0.0, y), (4095.0, y), 1.0, 1.0),
        })
        .collect::<Vec<_>>();
    // Two rows of separated, single-contact color wells make pickup direction
    // and color carry visible without repeated source-dab edges.
    let swatches = [
        (820.0, [0.66, 0.012, 0.006, 1.0]),
        (2048.0, [0.78, 0.34, 0.004, 1.0]),
        (3270.0, [0.006, 0.045, 0.62, 1.0]),
    ];
    strokes.extend(swatches.into_iter().flat_map(|(x, color)| {
        [1600.0, 2780.0].map(|y| StrokeSpec {
            layer: 1,
            brush: brush(1, 900.0, 1.0, color),
            samples: linear_samples(2, (x, y), (x, y), 1.0, 1.0),
        })
    }));
    run_strokes(canvas, &strokes, None).map_err(Into::into)
}

fn prepare_liquify_destination(canvas: &mut Canvas) -> Result<(), Box<dyn Error>> {
    // Fine grid lines expose interpolation quality and local continuity while
    // large, separated color wells make displacement direction unambiguous.
    let grid_color = [0.055, 0.070, 0.095, 1.0];
    let mut strokes = Vec::new();
    for coordinate in (420..=3780).step_by(420) {
        let coordinate = coordinate as f32;
        strokes.push(StrokeSpec {
            layer: 1,
            brush: brush(1, 22.0, 0.72, grid_color),
            samples: linear_samples(2, (coordinate, 240.0), (coordinate, 3850.0), 1.0, 1.0),
        });
        strokes.push(StrokeSpec {
            layer: 1,
            brush: brush(1, 22.0, 0.72, grid_color),
            samples: linear_samples(2, (240.0, coordinate), (3850.0, coordinate), 1.0, 1.0),
        });
    }
    for (x, y, color) in [
        (980.0, 980.0, [0.72, 0.015, 0.008, 1.0]),
        (3110.0, 980.0, [0.006, 0.075, 0.64, 1.0]),
        (980.0, 3110.0, [0.86, 0.36, 0.004, 1.0]),
        (3110.0, 3110.0, [0.015, 0.46, 0.12, 1.0]),
    ] {
        strokes.push(StrokeSpec {
            layer: 1,
            brush: brush(1, 720.0, 1.0, color),
            samples: linear_samples(2, (x, y), (x, y), 1.0, 1.0),
        });
    }
    run_strokes(canvas, &strokes, None).map_err(Into::into)
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
            let mut events = [LayerPenEvent::default(); EVENTS_PER_FRAME];
            for (slot, index) in (start..end).enumerate() {
                let phase = if index == 0 {
                    1
                } else if index + 1 == stroke.samples.len() {
                    3
                } else {
                    2
                };
                events[slot] = canvas.next_event(stroke.samples[index], phase);
            }
            let events = &events[..end - start];
            let commit = end == stroke.samples.len();
            let started = Instant::now();
            let submit = canvas.submit(events);
            let draw = canvas.draw();
            let submit_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            submit?;
            draw?;
            canvas.wait_idle()?;
            if let Some(output) = measurements.as_deref_mut() {
                output.push(FrameMeasurement {
                    submit_micros,
                    completed_micros: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    commit,
                    tip_gap_px: 0.0,
                    correction_px: 0.0,
                });
            }
        }
    }
    Ok(())
}

fn run_feedback_comparison(options: &Options) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = options.report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut results = Vec::with_capacity(options.scenarios.len());
    for scenario in &options.scenarios {
        eprintln!("measuring instant feedback off/on for {}", scenario.name());
        let (off, _) = measure_feedback_scenario(*scenario, false)?;
        let (on, on_measurements) = measure_feedback_scenario(*scenario, true)?;
        let mut tip_gaps = on_measurements
            .iter()
            .filter(|sample| !sample.commit)
            .map(|sample| sample.tip_gap_px)
            .collect::<Vec<_>>();
        let mut corrections = on_measurements
            .iter()
            .filter(|sample| !sample.commit)
            .map(|sample| sample.correction_px)
            .collect::<Vec<_>>();
        tip_gaps.sort_by(f32::total_cmp);
        corrections.sort_by(f32::total_cmp);
        let result = FeedbackBenchResult {
            name: scenario.name(),
            preview_dabs: on.dabs.saturating_sub(off.dabs),
            tip_gap_p95_px: quantile_f32(&tip_gaps, 0.95),
            tip_gap_p99_px: quantile_f32(&tip_gaps, 0.99),
            correction_p95_px: quantile_f32(&corrections, 0.95),
            correction_p99_px: quantile_f32(&corrections, 0.99),
            off,
            on,
        };
        println!(
            "{:<20} off p99 {:>7.3} ms  on p99 {:>7.3} ms  delta {:+7.3} ms  tip p99 {:>6.3} px",
            result.name,
            result.off.p99_micros as f64 / 1000.0,
            result.on.p99_micros as f64 / 1000.0,
            (result.on.p99_micros as i64 - result.off.p99_micros as i64) as f64 / 1000.0,
            result.tip_gap_p99_px,
        );
        results.push(result);
    }
    write_feedback_report(&options.report_path, &results)?;
    Ok(())
}

fn measure_feedback_scenario(
    kind: ScenarioKind,
    enabled: bool,
) -> Result<(BenchResult, Vec<FrameMeasurement>), Box<dyn Error>> {
    let mut canvas = Canvas::new()?;
    canvas.set_feedback(enabled)?;
    for index in 0..31 {
        let _ = canvas.add_layer(&format!("Empty feedback layer {index}"), 1)?;
    }
    canvas.draw()?;
    canvas.wait_idle()?;
    let strokes = prepare_scenario(kind, &mut canvas)?;
    if !strokes.is_empty() {
        run_strokes_feedback(&mut canvas, &strokes[..1], enabled, None)?;
        canvas.undo()?;
    }
    let before = canvas.metrics()?;
    let mut measurements = Vec::with_capacity(
        strokes
            .iter()
            .map(|stroke| stroke.samples.len().div_ceil(EVENTS_PER_FRAME))
            .sum(),
    );
    run_strokes_feedback(&mut canvas, &strokes, enabled, Some(&mut measurements))?;
    let after = canvas.metrics()?;
    Ok((
        summarize(kind, 1, &measurements, before, after),
        measurements,
    ))
}

fn run_strokes_feedback(
    canvas: &mut Canvas,
    strokes: &[StrokeSpec],
    enabled: bool,
    mut measurements: Option<&mut Vec<FrameMeasurement>>,
) -> Result<(), String> {
    for stroke in strokes {
        canvas.set_active_layer(stroke.layer)?;
        canvas.set_brush(LayerBrushSettings {
            streamline: 0.68,
            pressure_smoothing: 0.18,
            stabilization: 0.24,
            motion_filtering: 0.18,
            stabilization_expression: 1.0,
            ..stroke.brush
        })?;
        for start in (0..stroke.samples.len()).step_by(EVENTS_PER_FRAME) {
            let end = (start + EVENTS_PER_FRAME).min(stroke.samples.len());
            let commit = end == stroke.samples.len();
            let mut events = Vec::with_capacity(EVENTS_PER_FRAME + 2);
            for index in start..end {
                let phase = if index == 0 {
                    1
                } else if index + 1 == stroke.samples.len() {
                    3
                } else {
                    2
                };
                events.push(canvas.next_event(stroke.samples[index], phase));
            }
            let now_ns = canvas.real_timestamp_ns;
            if enabled && !commit && end >= 2 {
                let previous = stroke.samples[end - 2];
                let current = stroke.samples[end - 1];
                for future_ms in [4_u64, 8] {
                    let future = future_ms as f32;
                    let prediction = Sample {
                        x: current.x + (current.x - previous.x) * future,
                        y: current.y + (current.y - previous.y) * future,
                        pressure: current.pressure,
                    };
                    events.push(
                        canvas.predicted_event(
                            prediction,
                            now_ns.saturating_add(future_ms * 1_000_000),
                        ),
                    );
                }
            }
            let presentation_ns = now_ns.saturating_add(8_000_000);
            let started = Instant::now();
            let submit = canvas.submit(&events);
            let draw = canvas.draw_for(now_ns, presentation_ns);
            let submit_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            submit?;
            draw?;
            canvas.wait_idle()?;
            let completed_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            if let Some(output) = measurements.as_deref_mut() {
                let metrics = canvas.metrics()?;
                output.push(FrameMeasurement {
                    submit_micros,
                    completed_micros,
                    commit,
                    tip_gap_px: metrics.last_tip_gap_surface_px,
                    correction_px: metrics.last_endpoint_correction_surface_px,
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
    before: LayerCanvasMetrics,
    after: LayerCanvasMetrics,
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
        name: kind.name(),
        repeats,
        frames: measurements.len(),
        p50_micros: quantile(0.50),
        p95_micros: quantile(0.95),
        p99_micros: quantile(0.99),
        max_micros: move_times.last().copied().unwrap_or(0),
        submit_p95_micros: quantile_sorted(&submit_times, 0.95),
        over_budget: move_times
            .iter()
            .filter(|micros| **micros > FRAME_BUDGET_MICROS)
            .count(),
        commit_over_budget: commit_times
            .iter()
            .filter(|micros| **micros > FRAME_BUDGET_MICROS)
            .count(),
        dabs: after.raster_dabs.saturating_sub(before.raster_dabs),
        candidate_pixels: after
            .raster_candidate_pixels
            .saturating_sub(before.raster_candidate_pixels),
        composited_pixels: after
            .composited_pixels
            .saturating_sub(before.composited_pixels),
        paint_pages: after.paint_pages,
        preview_pages: after.preview_pages,
        coverage_pages: after.coverage_pages,
        material_pages: after.material_pages,
        storage_bytes: after
            .paint_storage_bytes
            .saturating_add(after.preview_storage_bytes)
            .saturating_add(after.destination_storage_bytes)
            .saturating_add(after.paint_state_storage_bytes)
            .saturating_add(after.composite_storage_bytes),
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

fn write_feedback_report(
    path: &Path,
    results: &[FeedbackBenchResult],
) -> Result<(), Box<dyn Error>> {
    let probe = Canvas::new().map_err(|error| format!("GPU probe failed: {error}"))?;
    let gpu = probe.gpu_info()?;
    let name_end = gpu
        .name_utf8
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(gpu.name_utf8.len());
    let gpu_name = String::from_utf8_lossy(&gpu.name_utf8[..name_end]);
    let mut report = format!(
        "# Instant stroke feedback — 4096×4096\n\n\
         Adapter: `{gpu_name}`; backend code {}; device type code {}.\n\n\
         Release build with debug symbols. Each scenario has at least 32 visible paint layers. The on run keeps an 8 ms replaceable real-input tail, submits platform-style predictions at +4/+8 ms, locks terminal coverage to the expected +8 ms presentation position, submits once, and then waits only for benchmark measurement. The off run bypasses all tail and preview work. Setup, warm-up, undo, and export are outside the timing window. Concurrent GPU load is not controlled.\n\n\
         | brush | off completed p95 ms | on completed p95 ms | off completed p99 ms | on completed p99 ms | p99 delta ms | p99 regression | off submit p95 ms | on submit p95 ms | on frames >8.33 ms | tip gap p95/p99 px | correction p95/p99 px | extra preview dabs | preview pages | on resident MiB |\n\
         |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        gpu.backend, gpu.device_type,
    );
    for result in results {
        let delta = result.on.p99_micros as i64 - result.off.p99_micros as i64;
        let regression = if result.off.p99_micros == 0 {
            0.0
        } else {
            delta as f64 / result.off.p99_micros as f64 * 100.0
        };
        report.push_str(&format!(
            "| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:+.3} | {:+.1}% | {:.3} | {:.3} | {} | {:.3}/{:.3} | {:.3}/{:.3} | {} | {} | {:.1} |\n",
            result.name,
            result.off.p95_micros as f64 / 1000.0,
            result.on.p95_micros as f64 / 1000.0,
            result.off.p99_micros as f64 / 1000.0,
            result.on.p99_micros as f64 / 1000.0,
            delta as f64 / 1000.0,
            regression,
            result.off.submit_p95_micros as f64 / 1000.0,
            result.on.submit_p95_micros as f64 / 1000.0,
            result.on.over_budget,
            result.tip_gap_p95_px,
            result.tip_gap_p99_px,
            result.correction_p95_px,
            result.correction_p99_px,
            result.preview_dabs,
            result.on.preview_pages,
            result.on.storage_bytes as f64 / (1024.0 * 1024.0),
        ));
    }
    let all_pass = results
        .iter()
        .all(|result| result.on.p99_micros < FRAME_BUDGET_MICROS);
    report.push_str(&format!(
        "\n120 Hz completed-work gate: **{}**. Tip gap measures the distance from the configured presentation-time estimate to terminal brush coverage; zero means the estimate is covered. Correction is the surface-space displacement smoothly distributed over the provisional tail, not committed geometry. Offscreen timing excludes surface acquisition and scanout.\n",
        if all_pass { "PASS" } else { "FAIL" }
    ));
    fs::write(path, report)?;
    Ok(())
}

fn write_report(path: &Path, results: &[BenchResult]) -> Result<(), Box<dyn Error>> {
    let probe = Canvas::new().map_err(|error| format!("GPU probe failed: {error}"))?;
    let gpu = probe.gpu_info()?;
    let name_end = gpu
        .name_utf8
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(gpu.name_utf8.len());
    let gpu_name = String::from_utf8_lossy(&gpu.name_utf8[..name_end]);
    let mut report = format!(
        "# GPU raster benchmark — 4096×4096\n\n\
         Adapter: `{gpu_name}`; backend code {}; device type code {}.\n\n\
         Release build with debug symbols. Each frame submits eight simulated coalesced pen samples through the public C ABI, calls the ABI frame function, then waits for that submission to complete. Every scenario has at least 32 visible paint layers. Each repetition creates a fresh canvas, warms the exact scenario pipeline, undoes the warm-up stroke, and contributes every measured frame to the reported distribution. Setup, shader/pipeline creation, canvas allocation, scenario warm-up/undo, brush selection, layer creation, and PNG export are outside the timing window. Submit latency is the production non-blocking path; completed-work latency serializes each measured frame to isolate its GPU work. Concurrent system/GPU load is not controlled, so these are reproducible workload references rather than cross-machine scores.\n\n\
         | scenario | state features | repeats | frames | move completed p50 ms | move completed p95 ms | move completed p99 ms | pen-up completed p99 ms | max move ms | submit p95 ms | move/pen-up frames > 8.33 ms | dabs | conservative contact Mpx | composite visits Mpx | paint pages | coverage pages | material pages | preview pages | resident canvas MiB |\n\
         |---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        gpu.backend, gpu.device_type,
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

fn write_validation_gallery(
    output_dir: &Path,
    validation: BrushValidation,
    names: &[&str],
) -> Result<(), Box<dyn Error>> {
    let phase = match validation {
        BrushValidation::Blank => "blank-canvas ink deposition",
        BrushValidation::Destination => "destination pickup, mixing, and smudge",
        BrushValidation::Watercolor => "layer-wide wet watercolor: twelve controlled cases",
        BrushValidation::Transport => {
            "event-driven capillary transport: field, rate, and distance matrix"
        }
    };
    let mut html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Layer brush validation</title>\
         <style>body{{font:16px system-ui;background:#18191c;color:#eee;margin:32px}}\
         main{{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:24px}}\
         figure{{margin:0;background:#24262b;padding:12px;border-radius:8px}}\
         img{{display:block;width:100%;height:auto;background:white}}figcaption{{padding:10px 2px 2px}}</style>\
         <h1>Layer brush validation: {phase}</h1><main>"
    );
    for name in names {
        let label = name.replace('_', " ");
        html.push_str(&format!(
            "<figure><img src=\"{name}.png\" alt=\"{label}\"><figcaption>{label}</figcaption></figure>"
        ));
    }
    html.push_str("</main>");
    fs::write(output_dir.join("index.html"), html)?;
    Ok(())
}

fn brush(preset: u32, diameter: f32, opacity: f32, color: [f32; 4]) -> LayerBrushSettings {
    LayerBrushSettings {
        preset,
        diameter_document_px: diameter,
        opacity,
        color_rgba_linear: color,
        ..LayerBrushSettings::default()
    }
}

fn transport_brush(
    field: u32,
    distance: f32,
    wet_flow: f32,
    dry_flow: f32,
    color: [f32; 4],
) -> LayerBrushSettings {
    LayerBrushSettings {
        preset: 21,
        diameter_document_px: 220.0,
        opacity: 0.94,
        color_rgba_linear: color,
        transport_field: field,
        // Match the large watercolor presets: one 256-texel field spans 1024
        // document pixels, so a test stroke sees connected structures rather
        // than several tiny repetitions.
        transport_scale: 0.25,
        transport_rotation_radians: 0.0,
        transport_contrast: 0.90,
        transport_wet_flow: wet_flow,
        transport_dry_flow: dry_flow,
        transport_distance_px: distance,
        transport_water_load: 0.94,
        ..LayerBrushSettings::default()
    }
}

fn prepare_destination_base(canvas: &mut Canvas) -> Result<(), Box<dyn Error>> {
    let underpaint = vec![
        StrokeSpec {
            layer: 1,
            brush: brush(4, 760.0, 0.92, [0.62, 0.025, 0.008, 1.0]),
            samples: lissajous(260, 1360.0, 1100.0, 0.0, 0.4),
        },
        StrokeSpec {
            layer: 1,
            brush: brush(5, 620.0, 0.72, [0.015, 0.18, 0.68, 1.0]),
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
            brush: brush(6, 130.0, 0.8, [0.82, 0.42, 0.015, 1.0]),
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
            brush: brush(7, 620.0, 0.86, [0.48, 0.025, 0.010, 1.0]),
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
            brush: brush(7, 660.0, 0.82, [0.012, 0.055, 0.42, 1.0]),
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
            brush: brush(7, 440.0, 0.76, [0.64, 0.24, 0.008, 1.0]),
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

fn circular_samples(
    count: usize,
    center: (f32, f32),
    radius: f32,
    revolutions: f32,
) -> Vec<Sample> {
    (0..count)
        .map(|index| {
            let t = index as f32 / count.saturating_sub(1).max(1) as f32;
            let angle = t * revolutions * std::f32::consts::TAU;
            Sample {
                x: center.0 + angle.cos() * radius,
                y: center.1 + angle.sin() * radius,
                pressure: 0.88,
            }
        })
        .collect()
}

fn linear_samples(
    count: usize,
    start: (f32, f32),
    end: (f32, f32),
    start_pressure: f32,
    end_pressure: f32,
) -> Vec<Sample> {
    (0..count)
        .map(|index| {
            let t = index as f32 / (count - 1) as f32;
            Sample {
                x: start.0 + (end.0 - start.0) * t,
                y: start.1 + (end.1 - start.1) * t,
                pressure: start_pressure + (end_pressure - start_pressure) * t,
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
            brush: brush(2, 84.0, 0.74, [0.012, 0.014, 0.018, 1.0]),
            samples,
        });
    }
    strokes
}

fn check(status: LayerStatus, operation: &str) -> Result<(), String> {
    if status == LayerStatus::Ok {
        Ok(())
    } else {
        Err(format!("{operation} failed with {status:?}"))
    }
}

fn quantile_sorted(values: &[u64], fraction: f32) -> u64 {
    let index = ((values.len().saturating_sub(1)) as f32 * fraction).round() as usize;
    values.get(index).copied().unwrap_or(0)
}

fn quantile_f32(values: &[f32], fraction: f32) -> f32 {
    let index = ((values.len().saturating_sub(1)) as f32 * fraction).round() as usize;
    values.get(index).copied().unwrap_or(0.0)
}
