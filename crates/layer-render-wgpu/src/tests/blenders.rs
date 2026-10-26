//! Smudge blenders and the wet blenders on fixed scenes: flat and faint paint
//! stay put, a colour edge blends along its length, the colour mixer carries
//! one colour, the amount is independent of spacing, paint pulled into
//! transparency fades without stripes, the texture-keeping blenders keep fine
//! detail, the paper decides what a pressed tip reaches and the Tissue lifts pigment.
use super::brush_blending::Canvas;
use super::*;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendSpace, BrushExecution, BrushSnapshot, DefaultBrushPreset as P, Stroke, StrokeId, StrokePoint, StrokeTool};

const RED: [f32; 4] = [0.8, 0.05, 0.03, 1.];
const BLUE: [f32; 4] = [0.03, 0.12, 0.8, 1.];

fn palette(pixels: &[[f64; 4]]) -> std::collections::BTreeSet<[u64; 4]> {
    pixels.iter().map(|p| p.map(f64::to_bits)).collect()
}

#[test]
fn dither_only_picks_up_texels_inside_the_actual_contact() {
    for (radii, rotation, hardness, center, reach) in [([12., 12.], 0f32, 1., [64., 64.], 0.5),
        ([24., 3.], 0.7, 1., [64., 64.], 0.5), ([18., 9.], -0.4, 0.2, [64.3, 63.7], 0.5),
        ([0.8, 0.8], 0., 1., [64.2, 64.2], 0.01), ([32., 32.], 0., 1., [64., 64.], 0.08),
        ([40., 10.], 0.7, 0.9, [64.3, 63.7], 0.08)] {
        let mut brush = brush(P::DitherBlender, 64.);
        brush.blender.cell_size = 1.;
        brush.blender.reach = reach;
        let mut dab = test_dab(center, [1.; 4], 1.);
        dab.radii = radii;
        dab.rotation = [rotation.cos(), rotation.sin()];
        dab.hardness = hardness;
        dab.motion = [radii[0] * 2., 0.];
        let mut mask = canvas();
        mask.paint(mask.base, test_style(BrushExecution::Dry), dab);
        let covered = mask.composite();
        let expected: Vec<_> = covered.iter().map(|p| if p[3] > 0. { BLUE.map(f64::from) } else { RED.map(f64::from) }).collect();
        let bytes: Vec<_> = (0..PAGE_SIZE).flat_map(|y| {
            let expected = &expected;
            (0..PAGE_SIZE).flat_map(move |x| {
                let color = if x < 128 && y < 128 { expected[(y * 128 + x) as usize].map(|c| c as f32) } else { RED };
                color.into_iter().flat_map(f32::to_le_bytes)
            })
        }).collect();
        let mut canvas = canvas();
        canvas.fill(canvas.base, BLUE);
        let page = canvas.r.paint_layers.iter().find(|l| l.id == canvas.base).unwrap().pages.first().unwrap();
        crate::test_support::upload_page(&canvas.r, &page.active().texture, &bytes);
        canvas.stroke(100, &brush, &vec![dab; 24], 6, false);
        for (i, (actual, expected)) in canvas.composite().iter().zip(&expected).enumerate() {
            assert_eq!(actual, expected, "outside paint entered pixel={i} radii={radii:?} rotation={rotation} hardness={hardness} center={center:?}");
        }
    }
}

#[test]
fn dither_reclassifies_uniform_pages_after_new_paint() {
    let mut canvas = canvas();
    canvas.fill(canvas.base, BLUE);
    let mut brush = brush(P::DitherBlender, 48.);
    brush.blender.cell_size = 1.;
    let dabs = contacts(&brush, &line([64., 8.], [64., 120.]));
    let uniform = canvas.composite();
    canvas.stroke(100, &brush, &dabs, 6, false);
    assert!(canvas.composite().iter().zip(&uniform).all(|(a, b)| a == b), "uniform paint stays uniform");
    let mut mark = test_dab([64., 64.], RED, 1.);
    mark.radii = [3., 30.];
    canvas.paint(canvas.base, test_style(BrushExecution::Dry), mark);
    let before = canvas.composite();
    canvas.stroke(101, &brush, &dabs, 6, false);
    let after = canvas.composite();
    assert!(changed(&before, &after) > 0.005, "a new stroke must pick up newly authored paint on a previously uniform page");
    let colors = palette(&before);
    assert!(after.iter().all(|p| colors.contains(&p.map(f64::to_bits))), "reclassification preserves authored tuples");
}

#[test]
fn dither_new_sources_only_receive_their_own_contact_exposure() {
    let mut reference = None;
    for chunk in [1, 6, usize::MAX] {
        let mut canvas = canvas();
        halves(&mut canvas, BLUE, RED);
        let before = canvas.composite();
        let mut brush = brush(P::DitherBlender, 64.);
        brush.blender.cell_size = 1.;
        brush.blender.reach = 0.5;
        let mut narrow = test_dab([45., 64.], [1.; 4], 1.);
        narrow.radii = [10.; 2];
        narrow.motion = [20., 0.];
        narrow.hardness = 1.;
        let mut wide = narrow;
        wide.radii = [36.; 2];
        wide.motion = [72., 0.];
        wide.flow = 0.0000001;
        let mut dabs = vec![narrow; 12];
        dabs.push(wide);
        canvas.stroke(100, &brush, &dabs, chunk, false);
        let after = canvas.composite();
        for (i, (actual, expected)) in after.iter().zip(&before).enumerate() {
            assert_eq!(actual, expected, "a weak expanding contact cannot unlock past exposure, pixel={i} chunk={chunk}");
        }
        if let Some(expected) = &reference { assert_eq!(&after, expected); } else { reference = Some(after); }
    }
    let mut canvas = canvas();
    halves(&mut canvas, BLUE, RED);
    let before = canvas.composite();
    let mut brush = brush(P::DitherBlender, 64.);
    brush.blender.cell_size = 1.;
    brush.blender.reach = 0.5;
    let mut wide = test_dab([45., 64.], [1.; 4], 1.);
    wide.radii = [36.; 2];
    wide.motion = [72., 0.];
    canvas.stroke(100, &brush, &[wide; 12], 6, false);
    assert!(changed(&before, &canvas.composite()) > 0.01, "an expanding contact with real exposure can pick up new paint");
}

#[test]
fn dither_preserves_source_rgba_across_colors_transparency_and_partial_controls() {
    for (alpha, opacity) in [(1., 1.), (0.35, 1.), (0.35, 0.3)] {
        let mut canvas = canvas();
        halves(&mut canvas, [0.; 4], [0.8, 0.05, 0.03, alpha]);
        let mut mark = test_dab([66., 64.], [0.03, 0.8, 0.12, 0.7], 1.);
        mark.radii = [5., 24.];
        canvas.paint(canvas.base, test_style(BrushExecution::Dry), mark);
        let before = canvas.composite();
        let colors = palette(&before);
        let mut brush = brush(P::DitherBlender, 48.);
        brush.blender.cell_size = 1.;
        brush.opacity = opacity;
        for pass in 1..=3 {
            canvas.stroke(100 + pass, &brush, &contacts(&brush, &line([66., 12.], [66., 116.])), 6, false);
            let after = canvas.composite();
            for (i, p) in after.iter().enumerate() {
                assert!(colors.contains(&p.map(f64::to_bits)), "alpha={alpha} opacity={opacity} pass={pass} pixel={i} invented {p:?}");
            }
            assert!(changed(&before, &after) > 0.01, "Dither moves pigment at alpha={alpha} opacity={opacity}");
            assert!(after.iter().any(|p| p[1] > p[0] && p[1] > p[2]), "Dither retains the third authored color");
        }
    }
}

#[test]
fn dither_does_not_expand_a_thin_intermediate_shade_into_a_blurred_band() {
    let mut canvas = canvas();
    halves(&mut canvas, [0., 0., 0., 1.], [1.; 4]);
    let mut mark = test_dab([66.5, 64.], [0.5, 0.5, 0.5, 1.], 1.);
    mark.radii = [1., 10000.];
    mark.hardness = 1.;
    canvas.paint(canvas.base, test_style(BrushExecution::Dry), mark);
    let before = canvas.composite();
    let intermediate = |pixels: &[[f64; 4]]| pixels.iter().filter(|p| p[0] > 0.01 && p[0] < 0.99).count();
    let mut brush = brush(P::DitherBlender, 64.);
    brush.blender.cell_size = 1.;
    brush.flow = 1.;
    let path = line([66., 4.], [66., 124.]);
    let dabs: Vec<_> = contacts(&brush, &path).into_iter().cycle().take(1000).collect();
    canvas.stroke(100, &brush, &dabs, 6, false);
    let after = canvas.composite();
    assert!(intermediate(&after) < intermediate(&before) * 2,
        "intermediate shades keep their area weight: {} -> {}", intermediate(&before), intermediate(&after));
    assert!(changed(&before, &after) > 0.1, "the high-exposure stroke really dithers");
}

#[test]
fn dither_is_identical_across_contact_batches_and_weak_final_contacts() {
    for opacity in [1., 0.3] {
        let mut reference = None;
        for chunk in [1, 6, usize::MAX] {
            let mut canvas = canvas();
            halves(&mut canvas, BLUE, RED);
            let mut brush = brush(P::DitherBlender, 48.);
            brush.blender.cell_size = 1.;
            brush.opacity = opacity;
            let mut dabs = contacts(&brush, &line([66., 8.], [66., 120.]));
            for (i, dab) in dabs.iter_mut().enumerate() {
                let scale = [1., 0.2, 1.3, 0.5][i % 4];
                dab.radii = [dab.radii[0] * scale, dab.radii[1] * scale * 0.4];
                let angle = i as f32 * 0.17;
                dab.rotation = [angle.cos(), angle.sin()];
            }
            let mut last = *dabs.last().unwrap();
            last.flow = 0.0001;
            last.radii = [2.; 2];
            dabs.push(last);
            canvas.stroke(100, &brush, &dabs, chunk, false);
            let after = canvas.composite();
            if let Some(expected) = &reference { assert_eq!(&after, expected, "opacity={opacity} chunk={chunk}"); }
            else { reference = Some(after); }
        }
    }
}

#[test]
fn dither_alpha_lock_preserves_destination_coverage_and_source_hues() {
    let mut canvas = canvas();
    halves(&mut canvas, [0.; 4], [0.8, 0.05, 0.03, 0.35]);
    let mut mark = test_dab([66., 64.], [0.03, 0.8, 0.12, 1.], 1.);
    mark.radii = [5., 20.];
    canvas.paint(canvas.base, test_style(BrushExecution::Dry), mark);
    let before = canvas.composite();
    let mut brush = brush(P::DitherBlender, 48.);
    brush.blender.cell_size = 1.;
    let mut style = layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush);
    style.alpha_locked = true;
    canvas.styled_stroke(100, style, &contacts(&brush, &line([66., 8.], [66., 120.])), 6);
    let after = canvas.composite();
    let hues: Vec<_> = before.iter().filter(|p| p[3] > 0.).map(|p| [p[0] / p[3], p[1] / p[3], p[2] / p[3]]).collect();
    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        assert_eq!(a[3], b[3], "alpha lock at pixel {i}");
        if a[3] > 0. {
            let hue = [a[0] / a[3], a[1] / a[3], a[2] / a[3]];
            assert!(hues.iter().any(|h| h.iter().zip(hue).all(|(a, b)| (a - b).abs() < 1e-5)), "alpha lock invented hue at {i}: {hue:?}");
        }
    }
    assert!(changed(&before, &after) > 0.01, "alpha lock still blends colored paint");
}

#[test]
fn dither_moves_black_and_transparency_as_whole_samples() {
    let mut canvas = canvas();
    halves(&mut canvas, [0.; 4], [0., 0., 0., 1.]);
    let before = canvas.composite();
    let colors = palette(&before);
    let mut brush = brush(P::DitherBlender, 48.);
    brush.blender.cell_size = 1.;
    brush.blender.reach = 0.5;
    let dabs: Vec<_> = contacts(&brush, &line([66., 4.], [66., 124.])).into_iter().cycle().take(1000).collect();
    canvas.stroke(100, &brush, &dabs, 6, false);
    let after = canvas.composite();
    assert!(after.iter().all(|p| colors.contains(&p.map(f64::to_bits))), "black dithers without inventing coverage");
    assert!((24..104).any(|y| pixel(&after, 62, y)[3] > 0.5), "black moves into transparency");
    assert!((24..104).any(|y| pixel(&after, 70, y)[3] == 0.), "transparency moves into black");
    let alpha = |pixels: &[[f64; 4]]| pixels.iter().map(|p| p[3]).sum::<f64>();
    assert!((alpha(&after) - alpha(&before)).abs() < alpha(&before) * 0.05, "the boundary does not turn into an opaque smear");
}

#[test]
fn dither_keeps_extended_and_perceptual_source_colors_and_zero_controls() {
    for (depth, space, red) in [(SampleDepth::F32, BlendSpace::Linear, [2., -0.1, 0.3, 0.6]),
        (SampleDepth::U8, BlendSpace::Perceptual, RED)] {
        let mut canvas = Canvas::new(DocumentColor { depth, ..Default::default() }, space, None);
        halves(&mut canvas, BLUE, red);
        let before = canvas.composite();
        let colors = palette(&before);
        let mut brush = brush(P::DitherBlender, 48.);
        brush.blender.cell_size = 1.;
        for (flow, opacity) in [(0., 1.), (1., 0.), (1., 0.3)] {
            brush.flow = flow;
            brush.opacity = opacity;
            canvas.stroke(100, &brush, &contacts(&brush, &line([66., 4.], [66., 124.])), 6, false);
            let after = canvas.composite();
            assert!(after.iter().all(|p| colors.contains(&p.map(f64::to_bits))), "{depth:?}/{space:?} preserves source tuples");
            if flow == 0. || opacity == 0. { assert_eq!(after, before, "zero controls leave paint unchanged"); }
            else { assert!(changed(&before, &after) > 0.01, "{depth:?}/{space:?} blends real pigment"); }
        }
    }
}

#[test]
fn dither_selection_weights_samples_without_mixing_or_crossing_excluded_paint() {
    use layer_core::{Selection, SelectionPixels};
    for feather in [false, true] {
        let mut canvas = canvas();
        halves(&mut canvas, BLUE, RED);
        let before = canvas.composite();
        let words: Vec<u32> = (0..128).flat_map(|_| (0..16).map(|word| (0..8).fold(0u32, |v, bit| {
            let x = word * 8 + bit;
            let coverage = if x < 60 { 4 } else if feather && x < 70 { 2 } else { 0 };
            v | coverage << (bit * 4)
        }))).collect();
        let selection = Arc::new(Selection::pixels(Arc::new(SelectionPixels::new([128, 128], [0, 0, 128, 128], words).unwrap())));
        let mut brush = brush(P::DitherBlender, 48.);
        brush.blender.cell_size = 1.;
        brush.blender.reach = 0.5;
        let mut style = DabStyle::for_brush(&brush, StrokeTool::Brush);
        style.selection = Some(selection);
        canvas.styled_stroke(100, style, &contacts(&brush, &line([62., 4.], [62., 124.])), 6);
        let after = canvas.composite();
        if !feather { assert_eq!(after, before, "excluded red paint cannot contaminate the selected blue area"); }
        else {
            let colors = palette(&before);
            assert!(after.iter().all(|p| colors.contains(&p.map(f64::to_bits))), "feathering changes density, not colors");
            assert!(changed(&before, &after) > 0.005, "the feathered boundary still blends");
        }
        for y in 0..128 { for x in 70..128 { assert_eq!(pixel(&after, x, y), pixel(&before, x, y), "excluded destination ({x}, {y})"); }}
    }
}

#[test]
fn dither_poststroke_edges_decode_exposure_without_darkening_the_interior() {
    let mut canvas = canvas();
    canvas.fill(canvas.base, [0.6, 0.5, 0.4, 1.]);
    let before = canvas.composite();
    let mut brush = brush(P::DitherBlender, 48.);
    brush.blender.cell_size = 1.;
    brush.rendering.edge_after_stroke = true;
    brush.rendering.burnt_edge = 1.;
    brush.rendering.edge_width = 4.;
    let dabs: Vec<_> = contacts(&brush, &line([8., 64.], [120., 64.])).into_iter().cycle().take(1000).collect();
    canvas.stroke(100, &brush, &dabs, 6, false);
    let after = canvas.composite();
    assert_eq!(pixel(&after, 64, 64), pixel(&before, 64, 64), "the center is not an edge");
    assert!(after.iter().zip(&before).any(|(a, b)| a[0] < b[0] - 0.01), "the requested burnt edge is visible");
}

fn canvas() -> Canvas {
    Canvas::new(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None)
}

fn brush(preset: P, diameter: f32) -> BrushSnapshot {
    let mut brush = layer_core::default_brush(preset);
    brush.diameter = diameter * brush.aspect.min(1.);
    brush.stabilization.strength = 0.;
    brush
}

fn contacts(brush: &BrushSnapshot, path: &[[f32; 2]]) -> Vec<Dab> {
    pressed(brush, path, 1.)
}

fn pressed(brush: &BrushSnapshot, path: &[[f32; 2]], pressure: f32) -> Vec<Dab> {
    let points: Vec<_> = path.iter().enumerate().map(|(i, &[x, y])| StrokePoint {
        position: Point { x, y }, pressure, tilt: [0.; 2], twist: 0., elapsed_micros: i as u32 * 4_000,
    }).collect();
    let stroke = Stroke::new(StrokeId(9), SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(1)), StrokeTool::Brush,
        brush.clone(), points).unwrap();
    let mut dabs = Vec::new();
    layer_engine::DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut dabs);
    dabs
}

fn line(from: [f32; 2], to: [f32; 2]) -> Vec<[f32; 2]> {
    (0..=64).map(|i| { let t = i as f32 / 64.; [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t] }).collect()
}

/// Paint `left` on the whole layer and `right` from x = 66 rightwards.
fn halves(canvas: &mut Canvas, left: [f32; 4], right: [f32; 4]) {
    canvas.fill(canvas.base, left);
    let mut dab = test_dab([1066., 64.], right, 1.);
    dab.radii = [1000.; 2];
    canvas.paint(canvas.base, test_style(BrushExecution::Dry), dab);
}

fn pixel(pixels: &[[f64; 4]], x: usize, y: usize) -> [f64; 4] {
    pixels[y * 128 + x]
}

fn blenders() -> impl Iterator<Item = P> {
    P::ALL.into_iter().filter(|&p| layer_core::default_brush(p).execution == BrushExecution::Smudge).chain([P::WetBlender, P::ColorMixer])
}

fn on_paper(brush: &BrushSnapshot, paper: &str) -> layer_render::DabStyle {
    let mut style = layer_render::DabStyle::for_brush(brush, StrokeTool::Brush);
    style.paper = Some(layer_core::paper::PaperBinding {
        surface: std::sync::Arc::new(layer_core::paper::PaperSurface {
            texture: layer_core::paper::built_in_paper(paper).unwrap().texture.clone(),
            mapping: layer_core::paper::reference([128, 128], layer_core::paper::paper_ppi(None)).mapping,
        }),
        target_offset: [0, 0],
    });
    style
}

/// The share of pixels around the edge at x = 66 that a stroke changed.
fn changed(before: &[[f64; 4]], after: &[[f64; 4]]) -> f64 {
    let band: Vec<_> = (24..104).flat_map(|y| (56..76).map(move |x| (x, y))).collect();
    let moved = band.iter().filter(|&&(x, y)| {
        let (a, b) = (pixel(before, x, y), pixel(after, x, y));
        (0..4).any(|c| (a[c] - b[c]).abs() > 0.004)
    }).count();
    moved as f64 / band.len() as f64
}

fn blend_on_paper(preset: P, depth: f32, pressure: f32, passes: u64) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
    let mut canvas = canvas();
    halves(&mut canvas, BLUE, RED);
    let before = canvas.composite();
    let mut brush = brush(preset, 40.);
    brush.grain = Some(layer_core::BrushGrain { depth });
    let style = on_paper(&brush, "cold_press");
    for pass in 0..passes {
        canvas.styled_stroke(pass + 1, style.clone(), &pressed(&brush, &line([66., 4.], [66., 124.]), pressure), 6);
    }
    (before, canvas.composite())
}

#[test]
fn every_blender_leaves_flat_paint_unchanged() {
    for preset in blenders().filter(|&p| layer_core::default_brush(p).blender.lift == 0.) {
        let mut canvas = canvas();
        canvas.fill(canvas.base, [0.4, 0.3, 0.2, 1.]);
        let before = canvas.composite();
        let brush = brush(preset, 48.);
        let path: Vec<_> = (0..=64).map(|i| { let t = i as f32 / 64.; [20. + 88. * t, 64. + 24. * (t * 6.).sin()] }).collect();
        canvas.stroke(1, &brush, &contacts(&brush, &path), 6, false);
        for (i, (a, b)) in canvas.composite().iter().zip(&before).enumerate() {
            assert!(a.iter().all(|v| v.is_finite()), "{preset:?} pixel {i} is finite: {a:?}");
            assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.0005), "{preset:?} pixel {i}: {a:?} != {b:?}");
        }
    }
}

#[test]
fn blenders_mix_an_edge_they_stroke_along() {
    for preset in [P::Smudge, P::NaturalBlender, P::SoftBlender, P::Blur] {
        let mut canvas = canvas();
        halves(&mut canvas, BLUE, RED);
        let before = canvas.composite();
        let brush = brush(preset, 48.);
        canvas.stroke(1, &brush, &contacts(&brush, &line([66., 4.], [66., 124.])), 6, false);
        let after = canvas.composite();
        for x in [60, 72] {
            let moved = (pixel(&after, x, 64)[0] - pixel(&before, x, 64)[0]).abs();
            assert!(moved > 0.04, "{preset:?} mixes {} px from the edge: {moved}", x.abs_diff(66));
        }
        assert_eq!(pixel(&after, 20, 64), pixel(&before, 20, 64), "{preset:?} leaves paint beyond its reach");
    }
}

#[test]
fn color_mixer_lays_one_averaged_color_where_the_wet_blender_keeps_both() {
    let spread = |preset| {
        let mut canvas = canvas();
        halves(&mut canvas, BLUE, RED);
        let brush = brush(preset, 40.);
        canvas.stroke(1, &brush, &contacts(&brush, &line([66., 4.], [66., 124.])), 3, false);
        let after = canvas.composite();
        let [a, b] = [pixel(&after, 58, 96), pixel(&after, 74, 96)];
        (0..3).map(|c| (a[c] - b[c]).abs()).fold(0., f64::max)
    };
    assert!(spread(P::ColorMixer) < 0.1, "the color mixer carries one color across the tip");
    assert!(spread(P::WetBlender) > 0.3, "the wet blender keeps each side's color");
}

#[test]
fn blending_amount_follows_distance_not_spacing() {
    let mixed = |spacing| {
        let mut canvas = canvas();
        halves(&mut canvas, BLUE, RED);
        let before = canvas.composite();
        let mut brush = brush(P::SoftBlender, 48.);
        brush.spacing = spacing;
        canvas.stroke(1, &brush, &contacts(&brush, &line([66., 4.], [66., 124.])), 6, false);
        (pixel(&canvas.composite(), 60, 64)[0] - pixel(&before, 60, 64)[0]).abs()
    };
    let [dense, sparse] = [mixed(0.02), mixed(0.06)];
    assert!((dense - sparse).abs() < 0.3 * dense.max(sparse), "{dense} vs {sparse}");
}

#[test]
fn paint_pulled_into_transparency_fades_without_stripes() {
    for preset in [P::Smudge, P::SoftBlender, P::BlendingStump, P::TextureBlender] {
        let mut canvas = canvas();
        let mut block = test_dab([-934., 64.], RED, 1.);
        block.radii = [1000.; 2];
        canvas.paint(canvas.base, test_style(BrushExecution::Dry), block);
        let brush = brush(preset, 24.);
        canvas.stroke(1, &brush, &contacts(&brush, &line([30., 64.], [122., 64.])), 6, false);
        let after = canvas.composite();
        let tail: Vec<f64> = (66..128).map(|x| (60..69).map(|y| pixel(&after, x, y)[3]).sum::<f64>() / 9.)
            .take_while(|&alpha| alpha > 0.01).collect();
        let fall: f64 = tail.windows(2).map(|w| w[0] - w[1]).sum();
        let wobble: f64 = tail.windows(3).map(|w| (w[0] - 2. * w[1] + w[2]).abs()).sum();
        assert!(tail.len() > 8 && tail.windows(2).all(|w| w[1] <= w[0]), "{preset:?} tail fades: {tail:?}");
        assert!(wobble < 0.35 * fall, "{preset:?} tail wobbles {wobble} over a fall of {fall}: {tail:?}");
    }
}

#[test]
fn blender_samples_stay_inside_a_page_neighbourhood() {
    assert!(layer_core::SMUDGE_CHUNK_TRAVEL_LIMIT + layer_core::BLENDER_REACH_LIMIT + 4. <= PAGE_SIZE as f32);
    let source = include_str!("../material_blend.wgsl");
    assert!(source.contains(&format!("const BLEND_REACH_LIMIT: f32 = {:.1};", layer_core::BLENDER_REACH_LIMIT)));
    assert!(source.contains(&format!("const SMUDGE_TRAVEL_LIMIT: f32 = {:.1};", layer_core::SMUDGE_CHUNK_TRAVEL_LIMIT)));
    for (index, kind) in layer_core::BlenderKind::ALL.into_iter().enumerate() {
        assert_eq!(kind as usize, index);
    }
}

#[test]
fn blenders_stay_finite_over_faint_paint() {
    for alpha in [1e-6, 1e-39] {
        for preset in blenders() {
            let mut canvas = canvas();
            canvas.fill(canvas.base, [0.4, 0.3, 0.2, alpha]);
            let brush = brush(preset, 40.);
            canvas.stroke(1, &brush, &contacts(&brush, &line([30., 64.], [98., 64.])), 6, false);
            for (i, p) in canvas.composite().iter().enumerate() {
                assert!(p.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)), "{preset:?} over alpha {alpha}: pixel {i} {p:?}");
            }
        }
    }
}

#[test]
fn texture_keeping_blenders_keep_fine_detail_that_soft_blending_removes() {
    let kept = |preset| {
        let mut canvas = canvas();
        canvas.fill(canvas.base, [0.6, 0.45, 0.3, 1.]);
        for y in (34..96).step_by(5) {
            for x in (22..108).step_by(5) {
                let mut dab = test_dab([x as f32 + 0.5, y as f32 + 0.5], [0.3, 0.22, 0.15, 1.], 1.);
                dab.radii = [1.; 2];
                dab.hardness = 1.;
                canvas.paint(canvas.base, test_style(BrushExecution::Dry), dab);
            }
        }
        let detail = |pixels: &[[f64; 4]]| {
            let mut sum = 0.;
            for y in 50..78 {
                for x in 44..86 {
                    let mean = (0..25).map(|k| pixel(pixels, x + k % 5 - 2, y + k / 5 - 2)[0]).sum::<f64>() / 25.;
                    sum += (pixel(pixels, x, y)[0] - mean).powi(2);
                }
            }
            sum.sqrt()
        };
        let before = detail(&canvas.composite());
        let brush = brush(preset, 48.);
        for pass in 0..4 {
            let [from, to] = if pass % 2 == 0 { [[30., 64.], [98., 64.]] } else { [[98., 64.], [30., 64.]] };
            canvas.stroke(pass + 1, &brush, &contacts(&brush, &line(from, to)), 6, false);
        }
        detail(&canvas.composite()) / before
    };
    let soft = kept(P::SoftBlender);
    let texture = kept(P::TextureBlender);
    assert!(texture > 1.5 * soft, "texture-keeping keeps {texture} of the detail, soft {soft}");
    assert!(kept(P::HueBlender) > 0.9, "hue blending keeps lightness detail");
}

#[test]
fn zero_texture_strength_blends_as_if_there_were_no_paper() {
    for preset in [P::Smudge, P::SoftBlender, P::GrainyBlender, P::BlendingKnife, P::BlendingStump] {
        let mut blind = canvas();
        halves(&mut blind, BLUE, RED);
        let mut brush = brush(preset, 40.);
        brush.grain = None;
        blind.stroke(1, &brush, &pressed(&brush, &line([66., 4.], [66., 124.]), 0.6), 6, false);
        let mut flat = canvas();
        halves(&mut flat, BLUE, RED);
        brush.grain = Some(layer_core::BrushGrain { depth: 0. });
        flat.styled_stroke(1, on_paper(&brush, "cold_press"), &pressed(&brush, &line([66., 4.], [66., 124.]), 0.6), 6);
        assert_eq!(blind.composite(), flat.composite(), "{preset:?} at 0% Texture strength ignores the paper");
    }
}

#[test]
fn a_light_touch_blends_only_the_paper_it_reaches() {
    let (before, blind) = blend_on_paper(P::SoftBlender, 0., 0.15, 1);
    let (_, light) = blend_on_paper(P::SoftBlender, 1., 0.15, 1);
    let (_, firm) = blend_on_paper(P::SoftBlender, 1., 1., 1);
    let [blind, light, firm] = [changed(&before, &blind), changed(&before, &light), changed(&before, &firm)];
    assert!(blind > 0.7, "the paper-blind blender reaches most pixels: {blind}");
    assert!(light < blind - 0.15, "a light touch leaves the valleys alone: {light} vs {blind}");
    assert!(firm > light + 0.2, "pressure reaches deeper into the tooth: {firm} vs {light}");
}

#[test]
fn light_passes_never_reach_what_pressure_reaches() {
    let (before, light) = blend_on_paper(P::SoftBlender, 1., 0.15, 10);
    let (_, firm) = blend_on_paper(P::SoftBlender, 1., 1., 1);
    let [light, firm] = [changed(&before, &light), changed(&before, &firm)];
    assert!(light < firm, "ten light passes reach less paper than one firm pass: {light} vs {firm}");
}

#[test]
fn tissue_lifts_pigment_where_it_presses_without_shifting_its_colour() {
    let lifted = |pressure| {
        let mut canvas = canvas();
        canvas.fill(canvas.base, [0.4, 0.3, 0.2, 1.]);
        let brush = brush(P::Tissue, 48.);
        canvas.stroke(1, &brush, &pressed(&brush, &line([20., 64.], [108., 64.]), pressure), 6, false);
        let after = canvas.composite();
        let p = pixel(&after, 64, 64);
        for (i, a) in after.iter().enumerate() {
            assert!(a[3] <= 1. + 1e-6, "pixel {i} never gains paint: {a:?}");
            if a[3] > 0.01 { assert!(((a[0] / a[3]) - 0.4).abs() < 0.01, "pixel {i} keeps its colour: {a:?}"); }
        }
        1. - p[3]
    };
    let [light, firm] = [lifted(0.3), lifted(1.)];
    assert!(light > 0.0005, "a light touch lifts a little: {light}");
    assert!(firm > 2. * light, "pressing lifts more: {firm} vs {light}");
}

#[test]
fn repeated_blenders_preserve_pigment_color_at_transparent_edges() {
    let pigment = [0.24, 0.09, 0.42, 1.];
    let mut failures = Vec::new();
    for preset in std::iter::once(P::GrainyBlender).chain(blenders().filter(|p| *p != P::GrainyBlender)).chain([P::MixingBrush]) {
        for mixing in layer_core::ColorMixSpace::ALL {
            let mut canvas = canvas();
            let mut block = test_dab([-950., 64.], pigment, 1.);
            block.radii = [1000.; 2];
            canvas.paint(canvas.base, test_style(BrushExecution::Dry), block);
            let mut brush = brush(preset, 40.);
            brush.wet_mix.mix_space = mixing;
            brush.color_rgba_linear = pigment;
            let style = on_paper(&brush, "cold_press");
            let dabs = contacts(&brush, &line([34., 64.], [112., 64.]));
            for pass in 0..12 { canvas.styled_stroke(pass + 1, style.clone(), &dabs, 6); }
            let mut maximum = 0f64;
            let mut worst = (0, [0.; 4]);
            for (i, p) in canvas.composite().into_iter().enumerate() {
                if !p.iter().all(|v| v.is_finite()) || !(0. ..=1.).contains(&p[3]) {
                    maximum = f64::INFINITY;
                    worst = (i, p);
                    break;
                }
                if p[3] < 1e-5 { continue; }
                for c in 0..3 {
                    let error = (p[c] / p[3] - f64::from(pigment[c])).abs();
                    if error > maximum { maximum = error; worst = (i, p); }
                }
            }
            eprintln!("{preset:?}/{mixing:?}: pigment error {maximum}, worst {worst:?}");
            if maximum > 0.002 { failures.push(format!("{preset:?}/{mixing:?}: {maximum}, {worst:?}")); }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn blenders_preserve_faint_uniform_pigment_color() {
    let mut failures = Vec::new();
    for preset in blenders().chain([P::MixingBrush]) {
        for mixing in layer_core::ColorMixSpace::ALL {
            let mut canvas = canvas();
            let pigment = [0.24, 0.09, 0.42, 1e-8];
            canvas.fill(canvas.base, pigment);
            let mut brush = brush(preset, 40.);
            brush.wet_mix.mix_space = mixing;
            brush.color_rgba_linear = [pigment[0], pigment[1], pigment[2], 1.];
            canvas.stroke(1, &brush, &contacts(&brush, &line([30., 64.], [98., 64.])), 6, false);
            let actual = pixel(&canvas.composite(), 64, 64);
            let error = (0..3).map(|c| (actual[c] / actual[3] - f64::from(pigment[c])).abs()).fold(0f64, f64::max);
            let alpha_error = if brush.blender.lift == 0. && brush.wet_mix.amount_of_paint == 0. && brush.wet_mix.refill == 0. {
                (actual[3] / f64::from(pigment[3]) - 1.).abs()
            } else { 0. };
            if !actual.iter().all(|v| v.is_finite()) || actual[3] <= 0. || error > 0.0001 || alpha_error > 0.0001 {
                failures.push(format!("{preset:?}/{mixing:?}: color error {error}, relative alpha error {alpha_error}, {actual:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn hue_blender_preserves_hdr_lightness() {
    let pigment = [1.4, 1.2, 1.1, 1.];
    let mut canvas = canvas();
    canvas.fill(canvas.base, pigment);
    let brush = brush(P::HueBlender, 40.);
    canvas.stroke(1, &brush, &contacts(&brush, &line([30., 64.], [98., 64.])), 6, false);
    let actual = pixel(&canvas.composite(), 64, 64);
    for c in 0..4 {
        assert!((actual[c] - f64::from(pigment[c])).abs() < 2e-5, "HDR hue blender: {actual:?}");
    }
}

#[test]
fn dither_uses_document_luminance_in_every_profile() {
    let mut reference = Vec::new();
    for space in RgbSpace::ALL {
        let mut canvas = Canvas::new(DocumentColor { space, depth: SampleDepth::F32 }, BlendSpace::Linear, None);
        let transform = RgbSpace::Srgb.linear_transform(space);
        let colors = [[0., 0.23, 0.], [0.8, 0., 0.]].map(|c| {
            let rgb = layer_core::color::rgb::apply(transform, c);
            [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.]
        });
        halves(&mut canvas, colors[0], colors[1]);
        let mut brush = brush(P::DitherBlender, 40.);
        brush.blender.cell_size = 1.;
        brush.wet_mix.mix_space = layer_core::ColorMixSpace::LinearRgb;
        canvas.stroke(1, &brush, &contacts(&brush, &line([30., 64.], [98., 64.])), 6, false);
        let inverse = space.linear_transform(RgbSpace::Srgb);
        let actual: Vec<_> = canvas.composite().into_iter().map(|p| {
            let rgb = layer_core::color::rgb::apply(inverse, [p[0], p[1], p[2]]);
            [rgb[0], rgb[1], rgb[2], p[3]]
        }).collect();
        if reference.is_empty() { reference = actual; continue; }
        let error = actual.iter().zip(&reference).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs())).fold(0f64, f64::max);
        assert!(error < 5e-5, "{space:?}: Dither changes physical colors by {error}");
    }
}

#[test]
fn color_mixer_color_is_independent_of_uniform_coverage_scale() {
    let draw = |alpha: f32, mixing| {
        let mut canvas = canvas();
        canvas.fill(canvas.base, [0.2, 0.3, 0.4, alpha]);
        let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|i| {
            let rgb = if i % PAGE_SIZE < 66 { [0.05, 0.2, 0.6] } else { [0.8, 0.05, 0.02] };
            [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha].into_iter().flat_map(f32::to_le_bytes)
        }).collect();
        crate::test_support::upload_page(&canvas.r, &canvas.r.paint_layers[0].pages[0].active().texture, &bytes);
        let seeded = canvas.composite();
        assert!((pixel(&seeded, 30, 64)[2] / f64::from(alpha) - 0.6).abs() < 1e-6);
        let mut brush = brush(P::ColorMixer, 40.);
        brush.wet_mix.mix_space = mixing;
        canvas.stroke(1, &brush, &contacts(&brush, &line([30., 64.], [98., 64.])), 3, false);
        canvas.composite().into_iter().map(|p| {
            assert!(p.iter().all(|v| v.is_finite()) && p[3] > 0., "{mixing:?}: invalid pigment {p:?}");
            [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
        }).collect::<Vec<_>>()
    };
    for mixing in layer_core::ColorMixSpace::ALL {
        let (dense, faint) = (draw(0.5, mixing), draw(1e-8, mixing));
        let error = dense.iter().zip(&faint).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs())).fold(0f64, f64::max);
        assert!(error < 0.0001, "{mixing:?}: changing coverage changes color by {error}");
    }
}


#[test]
fn painterly_blends_high_contrast_hdr_without_a_variance_ceiling() {
    let draw = |scale: f32| {
        let mut canvas = canvas();
        canvas.fill(canvas.base, [0., 0., 0., 1.]);
        let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|i| {
            let mut hash = i.wrapping_mul(747796405).wrapping_add(2891336453);
            hash = ((hash >> ((hash >> 28) + 4)) ^ hash).wrapping_mul(277803737);
            hash = (hash >> 22) ^ hash;
            let value = if hash & 1 == 0 { 0. } else { scale };
            [value, value, value, 1.].into_iter().flat_map(f32::to_le_bytes)
        }).collect();
        crate::test_support::upload_page(&canvas.r, &canvas.r.paint_layers[0].pages[0].active().texture, &bytes);
        let mut brush = brush(P::PainterlyBlender, 20.);
        brush.wet_mix.mix_space = layer_core::ColorMixSpace::LinearRgb;
        let dabs = contacts(&brush, &line([30., 64.], [98., 64.]));
        canvas.stroke(1, &brush, &dabs, dabs.len(), false);
        canvas.composite().into_iter().map(|p| [p[0] / f64::from(scale), p[1] / f64::from(scale), p[2] / f64::from(scale), p[3]]).collect::<Vec<_>>()
    };
    let low = draw(1.);
    let high = draw(65504.);
    let mut worst = (0., 0, [0.; 4], [0.; 4]);
    for (i, (a, b)) in low.into_iter().zip(high).enumerate() {
        if !(48..88).contains(&(i % 128)) || !(60..68).contains(&(i / 128)) { continue; }
        assert!(a.iter().chain(&b).all(|v| v.is_finite()), "nonfinite Painterly pigment: {a:?}, {b:?}");
        let error = (0..4).map(|c| (a[c] - b[c]).abs()).fold(0f64, f64::max);
        if error > worst.0 { worst = (error, i, a, b); }
    }
    eprintln!("Painterly HDR scale worst {worst:?}");
    assert!(worst.0 < 0.001, "Painterly HDR scale mismatch: {worst:?}");
}

fn dither_cell_pattern(canvas: &mut Canvas, color: impl Fn(u32, u32) -> [f32; 4]) {
    let mut dab = test_dab([128.; 2], [1.; 4], 1.);
    dab.radii = [10000.; 2];
    canvas.paint(canvas.base, test_style(BrushExecution::Dry), dab);
    let layer = canvas.r.paint_layers.iter().find(|l| l.id == canvas.base).unwrap();
    for page in &layer.pages {
        let bytes: Vec<_> = (0..PAGE_SIZE).flat_map(|y| (0..PAGE_SIZE).flat_map({
            let color = &color;
            move |x| color(page.coordinate[0] * PAGE_SIZE + x, page.coordinate[1] * PAGE_SIZE + y)
                .into_iter().flat_map(f32::to_le_bytes)
        })).collect();
        crate::test_support::upload_page(&canvas.r, &page.active().texture, &bytes);
    }
}

fn dither_cell_coherence(before: &[[f64; 4]], after: &[[f64; 4]], width: usize, size: usize) -> usize {
    let height = after.len() / width;
    let mut changed = 0;
    for y in (0..height).step_by(size) { for x in (0..width).step_by(size) {
        let indices: Vec<_> = (y..(y + size).min(height)).flat_map(|y|
            (x..(x + size).min(width)).map(move |x| y * width + x)).collect();
        if indices.iter().any(|&i| before[i] != after[i]) {
            changed += 1;
            assert!(indices.iter().all(|&i| after[i] == after[indices[0]]), "cell ({x},{y}) size={size} contains mixed donor colors");
        }
    }}
    changed
}

#[test]
fn dither_cells_keep_native_tuples_and_square_cells() {
    for size in [2, 3, 4, 50] {
        let mut canvas = canvas();
        dither_cell_pattern(&mut canvas, |x,y| match (x + 3*y) % 5 {
            0 => [0.;4], 1 => [0.,0.,0.,1.], 2 => [0.14,0.02,0.01,0.35], 3 => BLUE, _ => RED,
        });
        let before = canvas.composite();
        let colors = palette(&before);
        let mut brush = brush(P::DitherBlender, 128.);
        brush.blender.cell_size = size as f32;
        brush.blender.reach = 0.5;
        let mut dab = test_dab([64.;2], [1.;4], 1.);
        dab.radii = [100.;2]; dab.motion = [200.,0.]; dab.hardness = 1.;
        canvas.stroke(100, &brush, &[dab;16], 4, false);
        let after = canvas.composite();
        assert!(after.iter().all(|p| colors.contains(&p.map(f64::to_bits))), "size={size}: invented RGBA");
        assert!(dither_cell_coherence(&before,&after,128,size)>2, "size={size}: fixture must transfer paint");
    }
}

#[test]
fn dither_cells_sample_minority_native_texels_without_center_bias() {
    let mut canvas = canvas();
    dither_cell_pattern(&mut canvas, |x,y| if x%2==1 && y%2==1 {BLUE} else {RED});
    let mut brush = brush(P::DitherBlender,128.);
    brush.blender.cell_size = 2.;
    brush.blender.reach = 0.2;
    let mut dab = test_dab([64.;2],[1.;4],1.);
    dab.radii=[100.;2]; dab.motion=[200.,0.]; dab.hardness=1.;
    canvas.stroke(100,&brush,&[dab;24],6,false);
    let after=canvas.composite();
    let blue=(24..104).flat_map(|y|(24..104).map(move|x|y*128+x)).filter(|&i|after[i]==BLUE.map(f64::from)).count();
    let fraction=blue as f64/6400.;
    assert!((fraction-0.25).abs()<0.055,"3R1B native area fraction became {fraction}");
}

#[test]
fn dither_cells_global_three_pixel_grid_crosses_pages_and_contact_batches() {
    let mut reference=None;
    for chunk in [1,usize::MAX] {
        let mut canvas=Canvas::at(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None,[263,131]);
        dither_cell_pattern(&mut canvas,|x,y|if (x/5+y/7)%2==0 {BLUE} else {RED});
        let before=canvas.composite();
        let mut brush=brush(P::DitherBlender,128.); brush.blender.cell_size=3.;
        let mut dabs=Vec::new();
        for (radii,angle,flow) in [([80.,60.],0.3f32,1.),([95.,23.],-0.4,0.8),([2.,2.],0.,0.0000001)] {
            let mut dab=test_dab([255.5,64.5],[1.;4],1.);
            dab.radii=radii; dab.rotation=[angle.cos(),angle.sin()]; dab.motion=[160.,0.]; dab.flow=flow;
            dabs.extend([dab;6]);
        }
        canvas.stroke(100,&brush,&dabs,chunk,false);
        let after=canvas.composite();
        assert!(dither_cell_coherence(&before,&after,263,3)>5);
        assert!((24..105).any(|y| (254..258).any(|x| before[y*263+x]!=after[y*263+x])),"the fixture must change paint across page256");
        if let Some(expected)=&reference {assert_eq!(&after,expected,"N3 page crossing depends on contact batches");}
        else {reference=Some(after);}
    }
}

#[test]
fn dither_cells_selection_and_alpha_lock_preserve_destination_coverage() {
    use layer_core::{Selection,SelectionPixels};
    for locked in [false,true] {
        let mut canvas=canvas();
        dither_cell_pattern(&mut canvas,|x,y|if (x/3+y/4)%2==0 {[0.2,0.01,0.03,0.35]} else {BLUE});
        let before=canvas.composite();
        let words:Vec<u32>=(0..128).flat_map(|_|(0..16).map(|word|(0..8).fold(0u32,|v,bit| {
            let x=word*8+bit;v | (if x<61 {4} else if x<71 {2} else {0}) << (bit*4)
        }))).collect();
        let selection=Arc::new(Selection::pixels(Arc::new(SelectionPixels::new([128,128],[0,0,128,128],words).unwrap())));
        let mut brush=brush(P::DitherBlender,128.);brush.blender.cell_size=3.;
        let mut style=DabStyle::for_brush(&brush,StrokeTool::Brush);style.selection=Some(selection);style.alpha_locked=locked;
        let mut dab=test_dab([64.;2],[1.;4],1.);dab.radii=[100.;2];dab.motion=[200.,0.];dab.hardness=1.;
        canvas.styled_stroke(100,style,&[dab;16],4);
        let after=canvas.composite();
        assert!(changed(&before,&after)>0.01,"selection/alpha fixture must change pigment");
        for (i,(a,b)) in after.iter().zip(&before).enumerate() {
            if i%128>=71 {assert_eq!(a,b,"excluded destination {i}");}
            if locked {
                assert_eq!(a[3],b[3],"locked destination alpha {i}");
                assert!([before[0],before[3]].iter().any(|source|(0..3).all(|c|(a[c]/a[3]-source[c]/source[3]).abs()<1e-5)),"invented hue {i}");
            }
        }
        if !locked {let colors=palette(&before);assert!(after.iter().all(|p|colors.contains(&p.map(f64::to_bits))));}
    }
}

#[test]
#[ignore]
fn dither_cells_synthetic_completion_observations() {
    for size in [1, 2, 3, 4, 50] {
        let mut canvas=Canvas::at(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None,[512,512]);
        dither_cell_pattern(&mut canvas,|x,y|if (x/7+y/11)%2==0 {BLUE} else {RED});
        let original=canvas.composite();
        if let Ok(directory)=std::env::var("DITHER_CELLS_ARTIFACTS") {
            dither_cell_observation_png(&directory,"original",&original);
        }
        let mut brush=brush(P::DitherBlender,512.);brush.blender.cell_size=size as f32;brush.blender.reach=0.2;
        let mut dab=test_dab([256.;2],[1.;4],1.);dab.radii=[220.;2];dab.motion=[440.,0.];dab.hardness=0.8;
        for repetition in 0..4 {
            let start=std::time::Instant::now();
            canvas.stroke(100+repetition,&brush,&[dab;8],8,false);
            let submit=start.elapsed();
            let output=canvas.composite();
            if repetition==0 && let Ok(directory)=std::env::var("DITHER_CELLS_ARTIFACTS") {
                dither_cell_observation_png(&directory,&format!("cell-{size}"),&output);
            }
            eprintln!("DITHER_CELLS_OBSERVATION size={size} repetition={repetition} first={} submit_us={} completed_with_readback_us={} pixels={}",
                repetition==0,submit.as_micros(),start.elapsed().as_micros(),output.len());
        }
    }
}

fn dither_cell_observation_png(directory: &str, name: &str, pixels: &[[f64;4]]) {
    std::fs::create_dir_all(directory).unwrap();
    let file=std::fs::File::create(std::path::Path::new(directory).join(format!("{name}.png"))).unwrap();
    let mut encoder=png::Encoder::new(file,512,512);
    encoder.set_color(png::ColorType::Rgba);encoder.set_depth(png::BitDepth::Eight);
    let bytes:Vec<_>=pixels.iter().flat_map(|p|p.map(|v|(v.clamp(0.,1.)*255.+0.5) as u8)).collect();
    encoder.write_header().unwrap().write_image_data(&bytes).unwrap();
}

#[test]
fn dither_cells_only_pick_up_native_texels_inside_the_same_actual_contact() {
    let mut dab=test_dab([64.3,63.7],[1.;4],1.);
    dab.radii=[80.,65.];dab.rotation=[0.7f32.cos(),0.7f32.sin()];dab.hardness=0.2;dab.motion=[160.,0.];
    let mut mask=canvas();mask.paint(mask.base,test_style(BrushExecution::Dry),dab);
    let covered=mask.composite();
    for size in [2,3,50] {
        let mut canvas=canvas();
        dither_cell_pattern(&mut canvas,|x,y|if x<128 && y<128 && covered[(y*128+x) as usize][3]>0. {BLUE} else {RED});
        let before=canvas.composite();
        let mut brush=brush(P::DitherBlender,128.);brush.blender.cell_size=size as f32;brush.blender.reach=0.5;
        canvas.stroke(100,&brush,&[dab;24],6,false);
        assert_eq!(canvas.composite(),before,"N{size}: outside-contact pigment entered a covered destination");
    }
}

fn scatter_solid_dab(center: [f32;2], radius: f32) -> Dab {
    let mut dab=test_dab(center,[1.;4],1.);
    dab.radii=[radius;2];dab.motion=[2.*radius,0.];dab.hardness=1.;dab
}

#[test]
fn scatter_cells_preserve_opaque_native_palette_and_staggered_squares() {
    for size in [1,3,4,50] {
        let mut canvas=canvas();
        dither_cell_pattern(&mut canvas,|x,y|if (x+3*y)%5==0 {BLUE} else {RED});
        let before=canvas.composite();let colors=palette(&before);
        let mut brush=brush(P::ScatterBlender,128.);brush.blender.cell_size=size as f32;
        brush.blender.reach=0.5;brush.blender.character=1.;
        canvas.stroke(100,&brush,&[scatter_solid_dab([64.;2],100.);16],4,false);
        let after=canvas.composite();
        assert!(after.iter().all(|p|colors.contains(&p.map(f64::to_bits))),"Scatter N{size} invented an opaque native color");
        assert!(changed(&before,&after)>0.01,"Scatter N{size} must move pigment");
        assert!(scatter_cell_coherence(&before,&after,128,size,100)>0,"Scatter N{size} must change whole staggered cells");
    }
}

#[test]
fn scatter_cells_sample_minority_native_colors() {
    let mut canvas=canvas();
    dither_cell_pattern(&mut canvas,|x,y|if x%2==1 && y%2==1 {BLUE} else {RED});
    let mut brush=brush(P::ScatterBlender,128.);brush.blender.cell_size=4.;brush.blender.reach=0.3;
    canvas.stroke(100,&brush,&[scatter_solid_dab([64.;2],100.);24],6,false);
    let after=canvas.composite();
    let blue=(24..104).flat_map(|y|(24..104).map(move|x|y*128+x)).filter(|&i|after[i]==BLUE.map(f64::from)).count();
    let fraction=blue as f64/6400.;
    assert!((fraction-0.25).abs()<0.07,"Scatter 3R1B native area fraction became {fraction}");
}

#[test]
fn scatter_cells_cross_pages_without_chronological_chunk_dependence() {
    let mut reference=None;
    for chunk in [1,6,usize::MAX] {
        let mut canvas=Canvas::at(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None,[263,131]);
        dither_cell_pattern(&mut canvas,|x,y|if (x/5+y/7)%2==0 {BLUE} else {RED});
        let before=canvas.composite();
        let mut brush=brush(P::ScatterBlender,128.);brush.blender.cell_size=3.;brush.blender.reach=0.5;
        let mut dabs=Vec::new();
        for (radii,angle,flow,pull) in [([80.,60.],0.3f32,1.,0.1),([95.,23.],-0.4,0.8,0.3),([2.,2.],0.,0.0000001,0.5)] {
            let mut dab=scatter_solid_dab([255.5,64.5],80.);dab.radii=radii;
            dab.rotation=[angle.cos(),angle.sin()];dab.flow=flow;dab.material[1]=pull;dabs.extend([dab;6]);
        }
        canvas.stroke(100,&brush,&dabs,chunk,false);
        let after=canvas.composite();
        assert!(scatter_cell_coherence(&before,&after,263,3,100)>5);
        assert!((24..105).any(|y|(254..258).any(|x|before[y*263+x]!=after[y*263+x])),"Scatter fixture must transfer across page256");
        if let Some(expected)=&reference {assert_eq!(&after,expected,"Scatter must preserve cumulative state across chronological chunks");}
        else {reference=Some(after);}
    }
}

#[test]
fn scatter_cells_selection_transparency_alpha_lock_and_zero_controls() {
    use layer_core::{Selection,SelectionPixels};
    for locked in [false,true] {
        let mut canvas=canvas();
        dither_cell_pattern(&mut canvas,|x,y|match (x/3+y/4)%3 {0=>[0.;4],1=>[0.2,0.01,0.03,0.35],_=>BLUE});
        let before=canvas.composite();
        let words:Vec<u32>=(0..128).flat_map(|_|(0..16).map(|word|(0..8).fold(0u32,|v,bit| {
            let x=word*8+bit;v | (if x<61 {4} else if x<71 {2} else {0}) << (bit*4)
        }))).collect();
        let selection=Arc::new(Selection::pixels(Arc::new(SelectionPixels::new([128,128],[0,0,128,128],words).unwrap())));
        let mut brush=brush(P::ScatterBlender,128.);brush.blender.cell_size=4.;brush.blender.reach=0.5;
        let mut style=DabStyle::for_brush(&brush,StrokeTool::Brush);style.selection=Some(selection);style.alpha_locked=locked;
        canvas.styled_stroke(100,style,&[scatter_solid_dab([64.;2],100.);16],4);
        let after=canvas.composite();
        assert!(changed(&before,&after)>0.01,"Scatter selection/alpha fixture must move pigment");
        for (i,(a,b)) in after.iter().zip(&before).enumerate() {
            if i%128>=71 {assert_eq!(a,b,"Scatter excluded destination {i}");}
            if locked {assert_eq!(a[3],b[3],"Scatter locked alpha {i}");}
            else {assert!(a[3]>=b[3],"Scatter transparent donor erased pixel{i}");}
            if a[3]>0. {
                assert!([[0.2f64/0.35,0.01/0.35,0.03/0.35],[f64::from(BLUE[0]),f64::from(BLUE[1]),f64::from(BLUE[2])]].iter()
                    .any(|h|(0..3).all(|c|(a[c]/a[3]-h[c]).abs()<1e-5)),"Scatter invented native hue {i}");
            }
        }
    }
    let mut canvas=canvas();dither_cell_pattern(&mut canvas,|x,_|if x<64 {BLUE} else {RED});
    let before=canvas.composite();let mut brush=brush(P::ScatterBlender,128.);brush.blender.cell_size=4.;brush.blender.reach=0.;brush.wet_mix.pull=0.;
    let dab=scatter_solid_dab([64.;2],100.);
    canvas.stroke(100,&brush,&[dab;16],4,false);
    assert_eq!(canvas.composite(),before,"zero reach and Pull are identity");
    let mut pulled=dab;pulled.material[1]=0.25;
    canvas.stroke(101,&brush,&[pulled],1,false);
    assert!(changed(&before,&canvas.composite())>0.01,"Pull remains active when reach is zero");
}

#[test]
#[ignore]
fn scatter_cells_synthetic_completion_observations() {
    for size in [1,4,50] {
        let mut canvas=Canvas::at(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None,[512,512]);
        dither_cell_pattern(&mut canvas,|x,y|if (x/7+y/11)%2==0 {BLUE} else {RED});
        let original=canvas.composite();
        if let Ok(directory)=std::env::var("SCATTER_CELLS_ARTIFACTS") {dither_cell_observation_png(&directory,"original",&original);}
        let mut brush=brush(P::ScatterBlender,512.);brush.blender.cell_size=size as f32;brush.blender.reach=0.2;
        let mut dab=scatter_solid_dab([256.;2],220.);dab.hardness=0.8;
        for repetition in 0..4 {
            let start=std::time::Instant::now();canvas.stroke(100+repetition,&brush,&[dab;8],8,false);
            let submit=start.elapsed();let output=canvas.composite();let completed=start.elapsed();
            if repetition==0 && let Ok(directory)=std::env::var("SCATTER_CELLS_ARTIFACTS") {
                dither_cell_observation_png(&directory,&format!("cell-{size}"),&output);
            }
            eprintln!("SCATTER_CELLS_OBSERVATION size={size} repetition={repetition} first={} submit_us={} completed_with_readback_us={} pixels={}",
                repetition==0,submit.as_micros(),completed.as_micros(),output.len());
        }
    }
}

fn scatter_cell_hash(mut value: u32) -> u32 {
    value=value.wrapping_mul(747796405).wrapping_add(2891336453);
    value=((value>>((value>>28)+4))^value).wrapping_mul(277803737);
    (value>>22)^value
}

fn scatter_cell_coherence(before: &[[f64;4]], after: &[[f64;4]], width: usize, size: i32, seed: u32) -> usize {
    let phase_y=(scatter_cell_hash(seed^0x243f6a88) % size as u32) as i32;
    let mut cells=std::collections::BTreeMap::<(i32,i32),Vec<usize>>::new();
    for i in 0..after.len() {
        let row=(i as i32/width as i32-phase_y).div_euclid(size);
        let phase_x=(scatter_cell_hash(row as u32^scatter_cell_hash(seed^0x85a308d3))%size as u32) as i32;
        let col=(i as i32%width as i32-phase_x).div_euclid(size);
        cells.entry((col,row)).or_default().push(i);
    }
    cells.values().filter(|indices| {
        if !indices.iter().any(|&i|before[i]!=after[i]) {return false;}
        assert!(indices.iter().all(|&i|after[i]==after[indices[0]]),"Scatter staggered cell beginningpixel{} has mixed donor colors",indices[0]);
        true
    }).count()
}

#[test]
fn scatter_cells_grit_controls_color_mixing_and_strokes_have_independent_seeds() {
    let draw=|grit,id| {
        let mut canvas=canvas();
        dither_cell_pattern(&mut canvas,|x,_|if x<64 {BLUE} else {RED});
        let before=canvas.composite();
        let mut brush=brush(P::ScatterBlender,128.);brush.blender.cell_size=4.;brush.blender.reach=0.5;brush.blender.character=grit;
        let mut dab=scatter_solid_dab([64.;2],100.);dab.flow=0.15;
        canvas.stroke(id,&brush,&[dab],1,false);
        (before,canvas.composite())
    };
    let (before,solid)=draw(1.,100);let (_,mixed)=draw(0.,100);let (_,next)=draw(1.,101);
    let colors=palette(&before);
    assert!(solid.iter().all(|p|colors.contains(&p.map(f64::to_bits))),"full Grit picks whole native opaque colors");
    assert!(changed(&before,&solid)>0.005,"partial exposure must activate Scatter cells");
    assert!(mixed.iter().any(|p|!colors.contains(&p.map(f64::to_bits))),"lower Grit must produce intermediate colors");
    assert_ne!(solid,mixed,"Grit must change partial-exposure output");
    assert_ne!(solid,next,"subsequent stroke seeds change the scatter pattern");
    assert!(mixed.iter().all(|p|p[3]==1. && p.iter().all(|v|v.is_finite())),"lower Grit keeps opaque pixels finite and opaque");
}
