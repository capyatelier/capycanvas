//! Bristle paintbrush invariants, exercised through real input and GPU painting.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;

struct Pose {
    position: [f32; 2],
    pressure: f32,
    tilt: [f32; 2],
    twist: Option<f32>,
}

/// Paints one stroke of `samples` spans, stopping early without a pen-up after
/// `until`, and presents a frame every `cadence` samples.
fn paint(
    brush: BrushSnapshot,
    extent: [u32; 2],
    samples: u64,
    until: u64,
    cadence: u64,
    feedback: bool,
    pose: impl Fn(f32) -> Pose,
) -> Vec<u8> {
    let (mut input, consumer) = input_queue(1024);
    let mut engine = CanvasEngine::new(
        WgpuRasterizer::new_native_headless(color::DocumentColor::default()).unwrap(),
        Document::new("bristle", extent[0], extent[1]),
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
        .set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() })
        .unwrap();
    engine.set_brush(brush).unwrap();
    engine.render_frame_at(0).unwrap();
    let mut time = 1_000_000_000;
    for i in 0..=samples.min(until) {
        time += 4_000_000;
        let pose = pose(i as f32 / samples as f32);
        input
            .push(PenEvent {
                device_id: 1,
                sequence: time,
                timestamp_ns: time,
                view_revision: 0,
                surface_position: Point { x: pose.position[0], y: pose.position[1] },
                pressure: pose.pressure,
                tilt_radians: pose.tilt,
                twist_radians: pose.twist.unwrap_or(0.),
                distance: 0.,
                phase: if i == 0 {
                    PenPhase::Down
                } else if i == samples {
                    PenPhase::Up
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: if pose.twist.is_some() {
                    SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::BARREL_TWIST.0)
                } else {
                    SampleFlags::PRIMARY
                },
            })
            .unwrap();
        if i % cadence == 0 || i == samples || i == until {
            engine.render_frame_at(time).unwrap();
            engine.backend_mut().wait_idle().unwrap();
            while engine.has_pending_input() {
                engine.render_frame_at(time).unwrap();
                engine.backend_mut().wait_idle().unwrap();
            }
        }
    }
    let mut bytes = vec![0; extent[0] as usize * extent[1] as usize * 4];
    engine.backend_mut().copy_rgba8_srgb(&mut bytes, extent[0] as usize * 4).unwrap();
    bytes
}

fn ink(bytes: &[u8], top: usize, bottom: usize) -> f64 {
    (top..bottom)
        .flat_map(|y| (64..192).map(move |x| (255 - bytes[(y * 256 + x) * 4]) as f64))
        .sum()
}

fn render(brush: BrushSnapshot, pressure: f32, cadence: u64, feedback: bool) -> Vec<u8> {
    render_extent(brush, pressure, cadence, feedback, [256; 2])
}

fn render_extent(brush: BrushSnapshot, pressure: f32, cadence: u64, feedback: bool, extent: [u32; 2]) -> Vec<u8> {
    paint(brush, extent, 64, 64, cadence, feedback, |t| Pose {
        position: [extent[0] as f32 * (0.125 + t * 0.75), extent[1] as f32 * 0.5],
        pressure,
        tilt: [0.; 2],
        twist: None,
    })
}

fn render_bristle_line(brush: BrushSnapshot, degrees: f32, extent: [u32; 2], tilt: [f32; 2], twist: Option<f32>) -> Vec<u8> {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let center = [extent[0] as f32 * 0.5, extent[1] as f32 * 0.5];
    render_bristle_path(brush, extent, 120, 120, 0.7, tilt, twist, |t| {
        let s = (t - 0.5) * 300.;
        [center[0] + cos * s, center[1] + sin * s]
    })
}

#[allow(clippy::too_many_arguments)]
fn render_bristle_path(
    brush: BrushSnapshot,
    extent: [u32; 2],
    samples: u64,
    until: u64,
    pressure: f32,
    tilt: [f32; 2],
    twist: Option<f32>,
    path: impl Fn(f32) -> [f32; 2],
) -> Vec<u8> {
    paint(brush, extent, samples, until, 4, false, |t| Pose { position: path(t), pressure, tilt, twist })
}

#[test]
fn bristle_paint_scales_with_the_configured_size_and_preserves_frame_cadence() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 64.;
    brush
        .contact
        .as_mut()
        .unwrap()
        .bristles
        .as_mut()
        .unwrap()
        .load = 0.65;
    let small = render(brush.clone(), 0.8, 1, false);
    let predicted = render(brush.clone(), 0.8, 8, true);
    let maximum = small
        .iter()
        .zip(&predicted)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(
        maximum <= 5,
        "frame grouping/prediction changed bristles: {maximum}"
    );
    brush.diameter *= 2.;
    let large = render_extent(brush.clone(), 0.8, 4, false, [512; 2]);
    let mut difference = 0.;
    for y in 80..176 {
        for x in 48..208 {
            let averaged = (0..2)
                .flat_map(|dy| (0..2).map(move |dx| (2 * y + dy) * 512 + 2 * x + dx))
                .map(|p| f64::from(large[p * 4]))
                .sum::<f64>()
                / 4.;
            difference += (f64::from(small[(y * 256 + x) * 4]) - averaged).abs();
        }
    }
    assert!(
        difference / (96. * 160.) < 8.,
        "material changed scale: {}",
        difference / (96. * 160.)
    );
    brush.diameter = 64.;
    brush
        .contact
        .as_mut()
        .unwrap()
        .bristles
        .as_mut()
        .unwrap()
        .texture_scale = 2.;
    let coarse = render(brush.clone(), 0.8, 4, false);
    assert!(
        small
            .iter()
            .zip(&coarse)
            .filter(|(a, b)| a.abs_diff(**b) > 20)
            .count()
            > 200,
        "bristle scale must visibly change texture"
    );
    brush
        .contact
        .as_mut()
        .unwrap()
        .bristles
        .as_mut()
        .unwrap()
        .load = 0.;
    let empty = render(brush, 0.8, 4, false);
    assert!(
        empty.chunks_exact(4).all(|p| p[0] == 255),
        "empty brush must deposit no paint"
    );
}

#[test]
fn bristle_pressure_lean_and_roll_preserve_committed_paint_with_prediction() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 52.;
    let draw = |cadence, feedback| paint(brush.clone(), [256; 2], 64, 64, cadence, feedback, |t| {
        let turn = t * std::f32::consts::TAU;
        Pose {
            position: [256. * (0.125 + t * 0.75), 256. * (0.5 + turn.sin() * 0.12)],
            pressure: 0.08 + 0.9 * (t * std::f32::consts::PI).sin(),
            tilt: [0.9 * turn.cos(), 0.9 * turn.sin()],
            twist: Some(5.8 + turn),
        }
    });
    let real = draw(1, false);
    let predicted = draw(8, true);
    assert!(ink(&real, 70, 186) > 10_000.);
    let error = real.iter().zip(&predicted).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(error <= 5, "coupled pressure/tilt/twist changed committed paint by {error}");
}

#[test]
fn bristle_fan_keeps_a_steady_solid_band_at_every_angle_lean_and_roll() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 90.;
    brush.contact.as_mut().unwrap().bristles.as_mut().unwrap().load = 1.;
    let extent = [512_u32; 2];
    let upright = [0_f32, 30., 60., 80., 90.].map(|degrees| (degrees, [0.; 2], None));
    let rolled = [(0., [0.9, 0.], Some(0.9)), (0., [0.9, 0.], Some(-0.9)), (45., [0.6, 0.6], Some(2.0))];
    for (degrees, tilt, twist) in upright.into_iter().chain(rolled) {
        let bytes = render_bristle_line(brush.clone(), degrees, extent, tilt, twist);
        let painted = |x: f32, y: f32| {
            let (x, y) = (x.round() as i32, y.round() as i32);
            (0..extent[0] as i32).contains(&x) && (0..extent[1] as i32).contains(&y)
                && bytes[(y as usize * extent[0] as usize + x as usize) * 4 + 3] > 0
                && bytes[(y as usize * extent[0] as usize + x as usize) * 4] < 250
        };
        let (sin, cos) = degrees.to_radians().sin_cos();
        let mut holes = 0;
        let widths: Vec<f32> = (-80..=80)
            .map(|s| {
                let base = [256. + cos * s as f32, 256. + sin * s as f32];
                let across: Vec<i32> = (-60..=60)
                    .filter(|&o| painted(base[0] - sin * o as f32, base[1] + cos * o as f32))
                    .collect();
                if let (Some(&low), Some(&high)) = (across.first(), across.last()) {
                    let quarter = (high - low) / 4;
                    holes += (low + quarter..=high - quarter).filter(|o| !across.contains(o)).count();
                }
                across.len() as f32
            })
            .collect();
        let label = format!("{degrees} degrees, tilt {tilt:?}, twist {twist:?}");
        let mean = widths.iter().sum::<f32>() / widths.len() as f32;
        let step = widths.windows(4).fold(0_f32, |m, run| m.max((run[3] - run[0]).abs()));
        assert!(mean > 8., "{label}: the stroke must paint a band, width {mean}");
        assert!(step <= 3., "{label}: the width jumps by {step}: {widths:?}");
        assert_eq!(holes, 0, "{label}: a loaded brush leaves no gaps in the middle of its band");
    }
}

#[test]
fn a_bristle_stroke_paints_over_its_own_earlier_passes() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 90.;
    brush.contact.as_mut().unwrap().bristles.as_mut().unwrap().load = 1.;
    let corners = [[100., 256.], [400., 256.], [400., 120.], [250., 120.], [250., 420.]];
    let leg = |t: f32, legs: usize| {
        let x = t * legs as f32;
        let i = (x.floor() as usize).min(legs - 1);
        let f = x - i as f32;
        let (a, b) = (corners[i], corners[i + 1]);
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
    };
    let first = render_bristle_path(brush.clone(), [512; 2], 100, 100, 0.7, [0.; 2], None, |t| leg(t, 1));
    let crossed = render_bristle_path(brush, [512; 2], 400, 400, 0.7, [0.; 2], None, |t| leg(t, 4));
    let changed = |x0: usize, x1: usize, y0: usize, y1: usize| {
        let mut count = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * 512 + x) * 4;
                count += usize::from((0..3).any(|c| first[i + c].abs_diff(crossed[i + c]) > 12));
            }
        }
        count as f32 / ((x1 - x0) * (y1 - y0)) as f32
    };
    let untouched = changed(130, 200, 236, 276);
    let crossing = changed(246, 254, 236, 276);
    assert!(untouched < 0.01, "the earlier pass is replayed identically: {untouched}");
    assert!(crossing > 0.3, "the later pass must paint on top where it crosses: {crossing}");
}

#[test]
fn a_bristle_stroke_shows_the_paint_under_the_brush_before_its_hairs_leave() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 90.;
    brush.contact.as_mut().unwrap().bristles.as_mut().unwrap().load = 1.;
    let path = |t: f32| [106. + t * 300., 256.];
    let drawing = render_bristle_path(brush.clone(), [512; 2], 120, 60, 0.7, [0.; 2], None, path);
    let lifted = render_bristle_path(brush, [512; 2], 120, 120, 0.7, [0.; 2], None, path);
    let ink = |bytes: &[u8], x0: usize, x1: usize| {
        let mut ink = 0.;
        for y in 236..276 {
            for x in x0..x1 {
                ink += f64::from(255 - bytes[(y * 512 + x) * 4 + 1]);
            }
        }
        ink / ((x1 - x0) * 40) as f64
    };
    let under = ink(&drawing, 246, 262);
    let behind = ink(&drawing, 150, 230);
    assert!(under > behind * 0.8, "the paint under the brush shows while drawing: {under} vs {behind}");
    let mut changed = 0;
    for y in 200..312 {
        for x in 150..230 {
            let i = (y * 512 + x) * 4;
            changed += usize::from((0..3).any(|c| drawing[i + c].abs_diff(lifted[i + c]) > 12));
        }
    }
    assert!(changed < 40, "paint the hairs have left does not change: {changed} pixels");
}

#[test]
fn a_light_bristle_tap_misses_some_hair_tips_and_a_heavy_one_is_solid() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 150.;
    brush.contact.as_mut().unwrap().bristles.as_mut().unwrap().load = 1.;
    let filled = |pressure: f32| {
        let bytes = render_bristle_path(brush.clone(), [512; 2], 20, 20, pressure, [0.5, 0.], Some(0.), |_| [256., 256.]);
        let painted = |x: usize, y: usize| bytes[(y * 512 + x) * 4 + 1] < 250;
        let rows: Vec<bool> = (0..512).map(|y| (0..512).any(|x| painted(x, y))).collect();
        let columns: Vec<bool> = (0..512).map(|x| (0..512).any(|y| painted(x, y))).collect();
        let extent = |lines: &[bool]| {
            let first = lines.iter().position(|&l| l).unwrap();
            let last = lines.iter().rposition(|&l| l).unwrap();
            (first, last)
        };
        let (rows, columns) = (extent(&rows), extent(&columns));
        let along_rows = rows.1 - rows.0 > columns.1 - columns.0;
        let (first, last) = if along_rows { rows } else { columns };
        let lines = (first..=last)
            .filter(|&i| (0..512).any(|j| if along_rows { painted(j, i) } else { painted(i, j) }))
            .count();
        lines as f32 / (last - first + 1) as f32
    };
    let light = filled(0.15);
    let heavy = filled(0.9);
    assert!(light < 0.97, "a light tap leaves gaps across its imprint: {light}");
    assert!(heavy > 0.99, "a heavy tap is solid: {heavy}");
}

#[test]
fn a_dragged_bristle_stroke_smears_its_pen_down_imprint_into_streaks() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 90.;
    brush.contact.as_mut().unwrap().bristles.as_mut().unwrap().load = 1.;
    let bytes = render_bristle_line(brush, 0., [512; 2], [0.; 2], None);
    let grain = |x0: usize, x1: usize| {
        let (mut along, mut across) = (0., 0.);
        for y in 236..276 {
            for x in x0..x1 {
                let at = |x: usize, y: usize| f64::from(bytes[(y * 512 + x) * 4 + 1]);
                along += (at(x + 1, y) - at(x, y)).abs();
                across += (at(x, y + 1) - at(x, y)).abs();
            }
        }
        along / across.max(1.)
    };
    let start = grain(104, 124);
    let body = grain(220, 300);
    assert!(start < body * 2. + 0.1, "pen-down must be streaked like the body, not stamped: {start} vs {body}");
}

#[test]
fn bristle_pen_down_leaves_an_imprint_no_denser_than_the_stroke() {
    let mut brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
    brush.diameter = 90.;
    brush.contact.as_mut().unwrap().bristles.as_mut().unwrap().load = 1.;
    let bytes = render_bristle_line(brush, 0., [512; 2], [0.; 2], None);
    let density = |x0: usize, x1: usize| {
        let mut ink = 0.;
        for y in 216..296 {
            for x in x0..x1 {
                ink += f64::from(255 - bytes[(y * 512 + x) * 4 + 1]);
            }
        }
        ink / ((x1 - x0) * 80) as f64
    };
    let start = density(98, 112);
    let body = density(200, 312);
    assert!(body > 60., "the loaded body must be painted: {body}");
    assert!(start < body * 1.1, "pen-down must not be denser than the body: {start} vs {body}");
}
