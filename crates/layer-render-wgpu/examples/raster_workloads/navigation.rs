//! Unchanged photo navigation, including the actual managed presentation pass.
//! Completion is offscreen GPU work, not compositor/input-to-present latency.
use super::*;
use layer_render_wgpu::{SdrSurfaceColor, ViewportPresenter};

const SIZE: [u32; 2] = [1600, 1000];
const STEPS: usize = 96;

fn camera(
    extent: [u32; 2],
    scale: f32,
    angle: f32,
    center: [f32; 2],
) -> (ViewState, ViewTransform) {
    let (sin, cos) = angle.sin_cos();
    let a = scale * cos;
    let b = scale * sin;
    let c = -b;
    let d = a;
    let [x, y] = std::array::from_fn(|i| center[i] * extent[i] as f32);
    let tx = SIZE[0] as f32 * 0.5 - a * x - c * y;
    let ty = SIZE[1] as f32 * 0.5 - b * x - d * y;
    let determinant = a * d - b * c;
    (
        ViewState {
            width_px: SIZE[0],
            height_px: SIZE[1],
            document_to_surface: [a, b, c, d, tx, ty],
            background_rgba_linear: [0.; 4],
        },
        ViewTransform {
            revision: 0,
            surface_to_document: [
                d / determinant,
                -b / determinant,
                -c / determinant,
                a / determinant,
                (c * ty - d * tx) / determinant,
                (b * tx - a * ty) / determinant,
            ],
        },
    )
}

pub(crate) fn run(selected: &str, color: DocumentColor, output: &Path) -> Result<()> {
    if selected == "multiple" {
        return Err("Use --photo for the multiple-document worker fixture".into());
    }
    for (name, extent) in [
        ("24mp", [6000, 4000]),
        ("45mp", [8192, 5504]),
        ("60mp", [8192, 7324]),
    ] {
        if selected != "all" && selected != name {
            continue;
        }
        let mut canvas = Canvas::new(extent, name, color)?;
        canvas.stroke(0)?;
        adjustments(&mut canvas, &mut Observations::default())?;
        canvas.settle()?;
        let roots = native_roots(&canvas)?;
        let revision = canvas.engine.backend().canvas_preview_revision();
        let format = wgpu::TextureFormat::Rgba16Float;
        let target = canvas
            .engine
            .backend()
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("navigation managed presentation target"),
                size: wgpu::Extent3d {
                    width: SIZE[0],
                    height: SIZE[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        let target = target.create_view(&Default::default());
        let mut presenter = ViewportPresenter::for_surface(
            canvas.engine.backend(),
            format,
            SdrSurfaceColor::ExtendedLinearSrgb,
        )?;
        let fit = (SIZE[0] as f32 / extent[0] as f32).min(SIZE[1] as f32 / extent[1] as f32);
        let mut file = BufWriter::new(std::fs::File::create(
            output.join(format!("navigation-{name}.csv")),
        )?);
        writeln!(
            file,
            "phase,repeat,step,scale,angle,frame_cpu_ms,total_cpu_ms,completed_ms,composited_pixels,source_misses,display_bytes"
        )?;
        for (phase, fixed_scale) in [
            ("fit-pan-rotate", fit),
            ("half-pan-rotate", 0.5),
            ("native-pan-rotate", 1.),
            ("double-pan-rotate", 2.),
            ("zoom-pan-rotate", 0.),
        ] {
            for repeat in 0..2 {
                let mut cpu = Vec::new();
                let mut completed = Vec::new();
                let mut regenerated = 0;
                for step in 0..STEPS {
                    let t = step as f32 / (STEPS - 1) as f32;
                    let angle = t * std::f32::consts::TAU;
                    let scale = if fixed_scale == 0. {
                        fit * (2. / fit).powf((angle.sin() + 1.) * 0.5)
                    } else {
                        fixed_scale
                    };
                    let center = [0.5 + 0.4 * angle.cos(), 0.5 + 0.4 * angle.sin()];
                    let (view, inverse) = camera(extent, scale, angle, center);
                    let before = canvas.engine.backend().metrics();
                    let start = Instant::now();
                    canvas.engine.set_view(view, inverse);
                    canvas.engine.render_frame()?;
                    let frame_cpu = ms(start);
                    presenter.present(
                        canvas.engine.backend(),
                        &target,
                        view,
                        [0.1, 0.1, 0.1, 1.],
                    )?;
                    let total_cpu = ms(start);
                    // The presenter submits after the engine. Wait for the
                    // complete queue, not just the engine's last submission.
                    canvas
                        .engine
                        .backend()
                        .device()
                        .poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: Some(Duration::from_secs(30)),
                        })?;
                    let complete = ms(start);
                    let after = canvas.engine.backend().metrics();
                    let pixels = after.composited_pixels - before.composited_pixels;
                    regenerated += pixels;
                    writeln!(
                        file,
                        "{phase},{repeat},{step},{scale},{angle},{frame_cpu:.6},{total_cpu:.6},{complete:.6},{pixels},{},{}",
                        after.source_tile_misses - before.source_tile_misses,
                        after.composite_storage_bytes
                    )?;
                    cpu.push(total_cpu);
                    completed.push(complete);
                    assert_eq!(
                        canvas.engine.backend().canvas_preview_revision(),
                        revision,
                        "Navigation changed artwork revision"
                    );
                }
                let misses = completed.iter().filter(|&&v| v > 1000. / 120.).count();
                println!(
                    "{name} {phase} repeat={repeat} n={STEPS} CPU {:?}; completed {:?} ms; misses={misses}; recomposited={regenerated}",
                    quantiles(&mut cpu),
                    quantiles(&mut completed)
                );
            }
        }
        assert_eq!(native_roots(&canvas)?, roots);
        println!("Navigation preserved native paint/mask roots and artwork revision");
        canvas.memory()?;
    }
    Ok(())
}
