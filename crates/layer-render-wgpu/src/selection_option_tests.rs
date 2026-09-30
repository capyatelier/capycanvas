use super::*;
use layer_core::{Affine, SelectionMode, SelectionPixels, SelectionShape};
use super::page_bytes;
use layer_render::{ModifyStep, RegionRequest, RegionResult, RegionSource, SelectionModify, SelectionRefinement};
use std::sync::Arc;

fn rect(left: f32, right: f32) -> Selection {
    Selection::polygon(vec![
        Point { x: left, y: 16. },
        Point { x: right, y: 16. },
        Point { x: right, y: 112. },
        Point { x: left, y: 112. },
    ])
    .unwrap()
}
fn coverage(p: &SelectionPixels, x: u32, y: u32) -> u8 {
    assert_eq!(p.coverage_format(), 2);
    ((p.words()[(y * p.extent()[0].div_ceil(4) + x / 4) as usize] >> ((x % 4) * 8)) & 255) as u8
}
fn receive(
    r: &mut WgpuRasterizer,
    incoming: Selection,
    previous: Option<Selection>,
    mode: SelectionMode,
    feather: f32,
    antialias: bool,
) -> Arc<SelectionPixels> {
    receive_with_resize(r, incoming, previous, mode, feather, antialias, 0)
}
fn receive_with_resize(
    r: &mut WgpuRasterizer,
    incoming: Selection,
    previous: Option<Selection>,
    mode: SelectionMode,
    feather: f32,
    antialias: bool,
    resize: i32,
) -> Arc<SelectionPixels> {
    receive_refined(r, incoming, SelectionRefinement {
        resize,
        mode,
        antialias,
        feather,
        previous: previous.map(Arc::new),
        source_to_document: Affine::IDENTITY,
        keep_canvas_edges: false,
    })
}
fn receive_refined(r: &mut WgpuRasterizer, incoming: Selection, options: SelectionRefinement) -> Arc<SelectionPixels> {
    let reply = crate::test_support::receive_request(
        r,
        RegionRequest {
            request_id: 42,
            source: RegionSource::Selection(Arc::new(incoming)),
            contiguous: false,
            selection: Some(options),
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
        },
    );
    assert_eq!(reply.request_id, 42);
    reply.pixels
}

#[test]
fn selection_options_boolean_modes_antialias_and_gaussian_feather() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "selection")],
        &[],
        &[],
        true,
    );
    for mode in [
        SelectionMode::New,
        SelectionMode::Add,
        SelectionMode::Subtract,
        SelectionMode::Intersect,
    ] {
        for inverted in [false, true] {
            let mut previous = rect(16., 64.);
            previous.inverted = inverted;
            let p = receive(&mut r, rect(48., 96.), Some(previous), mode, 0., true);
            for y in 0..128 {
                for x in 0..128 {
                    let a = ((16..64).contains(&x) && (16..112).contains(&y)) != inverted;
                    let b = (48..96).contains(&x) && (16..112).contains(&y);
                    let expected = match mode {
                        SelectionMode::New => b,
                        SelectionMode::Add => a || b,
                        SelectionMode::Subtract => a && !b,
                        SelectionMode::Intersect => a && b,
                    };
                    assert_eq!(
                        coverage(&p, x, y),
                        if expected { 255 } else { 0 },
                        "{mode:?}, inverted={inverted}, {x},{y}"
                    );
                }
            }
        }
    }
    let smooth = receive(&mut r, rect(48.5, 96.5), None, SelectionMode::New, 0., true);
    let hard = receive(
        &mut r,
        rect(48.5, 96.5),
        None,
        SelectionMode::New,
        0.,
        false,
    );
    assert_eq!(coverage(&smooth, 48, 64), 128);
    assert_eq!(coverage(&hard, 48, 64), 255);
    assert!(
        hard.words()
            .iter()
            .all(|w| (0..4).all(|i| matches!((w >> (8 * i)) & 255, 0 | 255)))
    );
    let feathered = receive(&mut r, rect(48., 96.), None, SelectionMode::New, 12., true);
    // Independent Gaussian convolution of a vertical step, away from corners.
    let sigma = 6_f64;
    let total: f64 = (-18..=18)
        .map(|d| (-0.5 * (d * d) as f64 / (sigma * sigma)).exp())
        .sum();
    for x in 20..120 {
        let expected: f64 = (-18..=18)
            .filter(|d| (48..96).contains(&(x + d)))
            .map(|d| (-0.5 * (d * d) as f64 / (sigma * sigma)).exp())
            .sum();
        assert!(
            coverage(&feathered, x as u32, 64).abs_diff((expected / total * 255.).round() as u8)
                <= 1,
            "{x}"
        );
    }
    let distinct: std::collections::BTreeSet<_> =
        (30..66).map(|x| coverage(&feathered, x, 64)).collect();
    assert!(
        distinct.len() > 20,
        "feather retains smooth partial coverage"
    );
    // Reapplying a feathered selection must preserve old feather values.
    let added = receive(
        &mut r,
        rect(105., 110.),
        Some(Selection::pixels(feathered.clone())),
        SelectionMode::Add,
        0.,
        true,
    );
    for x in 20..100 {
        assert_eq!(coverage(&added, x, 64), coverage(&feathered, x, 64));
    }
    let empty = receive(
        &mut r,
        rect(80., 100.),
        Some(rect(16., 40.)),
        SelectionMode::Intersect,
        0.,
        true,
    );
    assert_eq!(empty.bounds(), [0; 4]);
}

#[test]
fn selection_resize_uses_circular_extrema_preserves_soft_values_and_clips_edges() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "resize")],
        &[],
        &[],
        true,
    );
    // Independent CPU reference for a soft rectangle, including document edges.
    let mut bytes = vec![0u8; 128 * 128];
    for y in 0..75 {
        for x in 0..60 {
            bytes[y * 128 + x] = if x < 55 { 180 } else { 80 };
        }
    }
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let input = Selection::pixels(Arc::new(
        SelectionPixels::bytes([128, 128], [0, 0, 60, 75], words).unwrap(),
    ));
    for radius in [5i32, -5] {
        let start = std::time::Instant::now();
        let p = receive_with_resize(
            &mut r,
            input.clone(),
            None,
            SelectionMode::New,
            0.,
            true,
            radius,
        );
        eprintln!("selection resize {radius}px 128²: {:?}", start.elapsed());
        for y in 0i32..128 {
            for x in 0i32..128 {
                let mut expected = if radius > 0 { 0 } else { 255 };
                for dy in -5i32..=5 {
                    for dx in -5i32..=5 {
                        if dx * dx + dy * dy > 25 {
                            continue;
                        }
                        let (xx, yy) = (x + dx, y + dy);
                        let value = if (0..128).contains(&xx) && (0..128).contains(&yy) {
                            bytes[(yy * 128 + xx) as usize]
                        } else {
                            0
                        };
                        expected = if radius > 0 {
                            expected.max(value)
                        } else {
                            expected.min(value)
                        };
                    }
                }
                assert_eq!(
                    coverage(&p, x as u32, y as u32),
                    expected,
                    "radius={radius} {x},{y}"
                );
            }
        }
    }
}

/// Binary coverage of a 128² document from a predicate.
fn binary(selected: impl Fn(i32, i32) -> bool) -> Vec<u8> {
    (0..128 * 128).map(|i| if selected(i % 128, i / 128) { 255 } else { 0 }).collect()
}
fn from_bytes(bytes: &[u8]) -> Selection {
    let words: Vec<u32> = bytes.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
    let covered = |i: usize| bytes[i] != 0;
    let (xs, ys): (Vec<u32>, Vec<u32>) = (0..bytes.len()).filter(|&i| covered(i)).map(|i| (i as u32 % 128, i as u32 / 128)).unzip();
    let bounds = match (xs.iter().min(), ys.iter().min()) {
        (Some(&x0), Some(&y0)) => [x0, y0, xs.iter().max().unwrap() + 1, ys.iter().max().unwrap() + 1],
        _ => [0; 4],
    };
    Selection::pixels(Arc::new(SelectionPixels::bytes([128, 128], bounds, words).unwrap()))
}
fn bytes_of(p: &SelectionPixels) -> Vec<u8> {
    (0..128 * 128).map(|i| coverage(p, i % 128, i / 128)).collect()
}
/// Independent circular extrema over `dx² + dy² ≤ radius²`. Samples beyond
/// the canvas are unselected, or ignored when erosion keeps the canvas edges.
fn morph(bytes: &[u8], radius: i32, keep_edges: bool) -> Vec<u8> {
    let r = radius.abs();
    (0..128 * 128)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            let mut value = if radius > 0 { 0 } else { 255 };
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy > r * r {
                        continue;
                    }
                    let (xx, yy) = (x + dx, y + dy);
                    let inside = (0..128).contains(&xx) && (0..128).contains(&yy);
                    if !inside && keep_edges && radius < 0 {
                        continue;
                    }
                    let sample = if inside { bytes[(yy * 128 + xx) as usize] } else { 0 };
                    value = if radius > 0 { value.max(sample) } else { value.min(sample) };
                }
            }
            value
        })
        .collect()
}
fn chained(resize: i32, keep_canvas_edges: bool, previous: Option<Selection>) -> SelectionRefinement {
    SelectionRefinement {
        resize,
        mode: if previous.is_some() { SelectionMode::Subtract } else { SelectionMode::New },
        antialias: true,
        feather: 0.,
        previous: previous.map(Arc::new),
        source_to_document: Affine::IDENTITY,
        keep_canvas_edges,
    }
}

#[test]
fn border_is_the_grown_selection_minus_the_shrunk_one() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "border")], &[], &[], true);
    let square = binary(|x, y| (32..96).contains(&x) && (32..96).contains(&y));
    let edge = binary(|x, y| x < 40 && (48..80).contains(&y));
    for (input, radius) in [(&square, 5), (&square, 1), (&edge, 4)] {
        let original = from_bytes(input);
        let grown = receive_refined(&mut r, original.clone(), chained(radius, false, None));
        let ring = receive_refined(&mut r, original, chained(-radius, false, Some(Selection::pixels(grown))));
        let expected: Vec<u8> = morph(input, radius, false)
            .iter()
            .zip(morph(input, -radius, false))
            .map(|(g, s)| g - s.min(*g))
            .collect();
        assert_eq!(bytes_of(&ring), expected, "radius {radius}");
    }
    let ring = {
        let original = from_bytes(&square);
        let grown = receive_refined(&mut r, original.clone(), chained(5, false, None));
        bytes_of(&receive_refined(&mut r, original, chained(-5, false, Some(Selection::pixels(grown)))))
    };
    let row: Vec<_> = (0..128).filter(|x| ring[64 * 128 + x] == 255).collect();
    assert_eq!(row, (27..37).chain(91..101).collect::<Vec<_>>(), "a band 2r wide, centred on the edge");
    assert!(ring.iter().all(|v| matches!(v, 0 | 255)));
}

#[test]
fn smooth_fills_notches_removes_spikes_and_keeps_the_canvas_edges() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "smooth")], &[], &[], true);
    let smooth = |r: &mut WgpuRasterizer, input: &[u8], radius: i32| {
        let mut last = from_bytes(input);
        for (resize, keep) in [(radius, false), (-2 * radius, true), (radius, false)] {
            last = Selection::pixels(receive_refined(r, last, chained(resize, keep, None)));
        }
        let SelectionShape::Pixels(pixels) = last.shape else { unreachable!() };
        bytes_of(&pixels)
    };
    let shape = binary(|x, y| {
        let body = (32..96).contains(&x) && (32..96).contains(&y);
        let notch = (60..64).contains(&x) && (32..48).contains(&y);
        let spike = (96..112).contains(&x) && (62..66).contains(&y);
        (body && !notch) || spike
    });
    let result = smooth(&mut r, &shape, 4);
    let expected = morph(&morph(&morph(&shape, 4, false), -8, true), 4, false);
    assert_eq!(result, expected, "close, then open");
    let at = |x: i32, y: i32| result[(y * 128 + x) as usize];
    assert_eq!(at(61, 40), 255, "the notch is filled");
    assert_eq!(at(104, 63), 0, "the spike is removed");
    assert_eq!((at(64, 64), at(40, 40), at(20, 20)), (255, 255, 0));
    let wide = binary(|x, y| (32..96).contains(&x) && (32..96).contains(&y) && !((56..72).contains(&x) && y < 48));
    assert_eq!(smooth(&mut r, &wide, 4)[40 * 128 + 64], 0, "a notch wider than 2r stays");

    let edge = binary(|x, _| x < 64);
    assert_eq!(smooth(&mut r, &edge, 6), edge, "a selection keeps the canvas edges it touches");
    let full = vec![255; 128 * 128];
    assert_eq!(smooth(&mut r, &full, 6), full);
    let shrunk = receive_refined(&mut r, from_bytes(&full), chained(-6, false, None));
    assert_eq!(coverage(&shrunk, 0, 64), 0, "a plain shrink erodes from the canvas edge");
}

fn modify_request(selection: Selection, steps: &[ModifyStep], preview: Option<f32>) -> RegionRequest {
    RegionRequest {
        request_id: 7,
        source: RegionSource::Modify(Arc::new(SelectionModify { selection: Arc::new(selection), steps: steps.to_vec(), preview })),
        contiguous: false,
        selection: None,
        position: [0, 0],
        tolerance: 0.,
        refinement: Default::default(),
        limit: None,
    }
}
fn modify(r: &mut WgpuRasterizer, selection: Selection, steps: &[ModifyStep], preview: Option<f32>) -> RegionResult {
    crate::test_support::receive_request(r, modify_request(selection, steps, preview))
}
/// Independent separable Gaussian with the renderer's reach and sigma, whose
/// taps beyond the canvas repeat its edge pixels.
fn gaussian(bytes: &[u8], feather: f32) -> Vec<u8> {
    let reach = (feather * 1.5).ceil() as i32;
    let sigma = f64::from(feather) * 0.5;
    let weights: Vec<f64> = (-reach..=reach).map(|d| (-0.5 * f64::from(d * d) / (sigma * sigma)).exp()).collect();
    let total: f64 = weights.iter().sum();
    let at = |v: i32| v.clamp(0, 127) as usize;
    let rows: Vec<f64> = (0..128 * 128)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            (-reach..=reach).map(|d| weights[(d + reach) as usize] * f64::from(bytes[y as usize * 128 + at(x + d)]) / 255.).sum::<f64>() / total
        })
        .collect();
    (0..128 * 128)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            let v: f64 = (-reach..=reach).map(|d| weights[(d + reach) as usize] * rows[at(y + d) * 128 + x as usize]).sum::<f64>() / total;
            (v.clamp(0., 1.) * 255.).round() as u8
        })
        .collect()
}
fn resize(resize: i32) -> ModifyStep {
    ModifyStep { resize, ..Default::default() }
}

#[test]
fn modify_matches_its_steps_within_a_window_across_chunks_and_inverted() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "modify")], &[], &[], true);
    let square = binary(|x, y| (44..90).contains(&x) && (30..100).contains(&y));
    let edge = binary(|x, y| x < 40 && (48..80).contains(&y));
    let soft: Vec<u8> = (0..128 * 128)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            if (50..80).contains(&x) && (40..90).contains(&y) { (40 + (x * 7 + y * 3) % 200) as u8 } else { 0 }
        })
        .collect();
    let feather = ModifyStep { feather: 6.5, ..Default::default() };
    let border = [resize(3), ModifyStep { subtract: true, ..resize(-3) }];
    let smooth = [resize(4), ModifyStep { chained: true, keep_canvas_edges: true, ..resize(-8) }, ModifyStep { chained: true, ..resize(4) }];
    for (name, input) in [("square", &square), ("edge", &edge), ("soft", &soft)] {
        for inverted in [false, true] {
            let effective: Vec<u8> = input.iter().map(|v| if inverted { 255 - v } else { *v }).collect();
            let cases: [(&str, &[ModifyStep], Vec<u8>); 5] = [
                ("grow", &[resize(5)], morph(&effective, 5, false)),
                ("shrink", &[resize(-4)], morph(&effective, -4, false)),
                ("border", &border, morph(&effective, 3, false).iter().zip(morph(&effective, -3, false)).map(|(g, s)| g - s.min(*g)).collect()),
                ("smooth", &smooth, morph(&morph(&morph(&effective, 4, false), -8, true), 4, false)),
                ("feather", &[feather], gaussian(&effective, 6.5)),
            ];
            for (label, steps, expected) in cases {
                let mut selection = from_bytes(input);
                selection.inverted = inverted;
                for chunk in [None, Some(3000.)] {
                    if let Some(taps) = chunk {
                        r.set_refine_chunk(Some(taps));
                    }
                    let result = modify(&mut r, selection.clone(), steps, None);
                    r.set_refine_chunk(None);
                    assert_eq!(result.placement, Affine::IDENTITY);
                    assert_eq!(result.pixels.extent(), [128, 128]);
                    let actual = bytes_of(&result.pixels);
                    let worst = actual.iter().zip(&expected).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                    let tolerance = if label == "feather" { 1 } else { 0 };
                    assert!(worst <= tolerance, "{name} inverted={inverted} {label} chunk={chunk:?}: off by {worst}");
                }
            }
        }
    }
}

#[test]
fn a_modify_preview_covers_the_selection_window_on_cells_that_approximate_the_result() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "preview")], &[], &[], true);
    let square = binary(|x, y| (44..90).contains(&x) && (30..100).contains(&y));
    let steps = [ModifyStep { feather: 12., ..Default::default() }];
    let exact = bytes_of(&modify(&mut r, from_bytes(&square), &steps, None).pixels);
    let near = modify(&mut r, from_bytes(&square), &steps, Some(1.));
    let [s, _, _, _, ox, oy] = near.placement.0;
    assert_eq!((s, near.placement.0[1..3].to_vec()), (1., vec![0., 0.]), "a cheap preview keeps every pixel");
    assert!(ox > 0. && oy > 0., "but covers only the window around the selection: {:?}", near.placement);
    let [gw, gh] = near.pixels.extent();
    for y in 0..128 {
        for x in 0..128 {
            let (i, j) = (x as i32 - ox as i32, y as i32 - oy as i32);
            let shown = if (0..gw as i32).contains(&i) && (0..gh as i32).contains(&j) { coverage(&near.pixels, i as u32, j as u32) } else { 0 };
            assert_eq!(shown, exact[y * 128 + x], "{x},{y}");
        }
    }
    let mut scales = std::collections::BTreeSet::new();
    for taps in [3_000., 20_000., 300_000., 20_000.] {
        r.set_refine_chunk(Some(taps));
        let preview = modify(&mut r, from_bytes(&square), &steps, Some(1.));
        let [s, _, _, _, ox, oy] = preview.placement.0;
        scales.insert(s as u32);
        assert!(s > 1. && s <= 6., "a costly preview uses cells within half the radius: {:?}", preview.placement);
        assert_eq!(preview.placement.0[1..3], [0., 0.]);
        let [gw, gh] = preview.pixels.extent();
        assert!(ox >= 0. && oy >= 0. && ox + gw as f32 * s <= 128. + s && oy + gh as f32 * s <= 128. + s);
        let cells = &preview.pixels;
        let sample = |x: f32, y: f32| {
            let q = [(x - ox) / s - 0.5, (y - oy) / s - 0.5];
            let at = |i: i32, j: i32| {
                if i < 0 || j < 0 || i >= gw as i32 || j >= gh as i32 { 0. } else { f32::from(coverage(cells, i as u32, j as u32)) }
            };
            let [i, j] = q.map(|v| v.floor() as i32);
            let [fx, fy] = q.map(|v| v - v.floor());
            let top = at(i, j) * (1. - fx) + at(i + 1, j) * fx;
            let bottom = at(i, j + 1) * (1. - fx) + at(i + 1, j + 1) * fx;
            top * (1. - fy) + bottom * fy
        };
        for y in 0..128 {
            for x in 0..128 {
                let shown = sample(x as f32 + 0.5, y as f32 + 0.5);
                let expected = f32::from(exact[y * 128 + x]);
                assert!((shown - expected).abs() <= 24., "{x},{y}: preview {shown} exact {expected} at scale {s}");
            }
        }
    }
    assert_eq!(scales, [2, 4].into(), "budgets choose among the pyramid's levels");
    r.set_refine_chunk(Some(20_000.));
    let grown = [resize(2)];
    let fine = modify(&mut r, from_bytes(&square), &grown, Some(1.)).placement.0[0];
    let coarse = modify(&mut r, from_bytes(&square), &grown, Some(4.)).placement.0[0];
    assert_eq!(fine, 1., "a small radius needs every pixel at full zoom");
    assert!(coarse > 1. && coarse <= 4., "but a zoomed-out view hides cells up to a display pixel wide: {coarse}");
    r.set_refine_chunk(None);
}

#[test]
fn refinement_chunks_follow_the_timed_pace_of_the_gpu() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "pace")], &[], &[], true);
    let square = binary(|x, y| (4..124).contains(&x) && (4..124).contains(&y));
    let steps = [ModifyStep { feather: 60., ..Default::default() }];
    modify(&mut r, from_bytes(&square), &steps, None);
    let (initial, timed) = r.refine_chunk().unwrap();
    if !timed {
        return;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while r.refine_chunk().unwrap().0 == initial {
        assert!(std::time::Instant::now() < deadline, "timed chunks change the budget");
        modify(&mut r, from_bytes(&square), &steps, None);
        crate::test_support::complete(&r);
        let _ = r.take_region();
    }
    let exact = bytes_of(&modify(&mut r, from_bytes(&square), &steps, None).pixels);
    assert_eq!(exact, gaussian(&square, 60.).iter().zip(&exact).map(|(e, a)| if e.abs_diff(*a) <= 1 { *a } else { *e }).collect::<Vec<_>>(), "any budget keeps the result");
}

#[test]
fn cancelling_a_modify_job_drops_its_result() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "cancel")], &[], &[], true);
    let square = binary(|x, y| (44..90).contains(&x) && (30..100).contains(&y));
    r.set_refine_chunk(Some(3000.));
    let mut long = modify_request(from_bytes(&square), &[ModifyStep { feather: 20., ..Default::default() }], None);
    long.request_id = 1;
    assert!(r.request_region(long).unwrap());
    for _ in 0..3 {
        crate::test_support::complete(&r);
        assert!(r.take_region().is_none(), "the long job is still running");
    }
    assert!(!r.request_region(modify_request(from_bytes(&square), &[resize(2)], None)).unwrap(), "one job at a time");
    r.cancel_region();
    let grown = modify(&mut r, from_bytes(&square), &[resize(2)], None);
    assert_eq!(grown.request_id, 7);
    assert_eq!(bytes_of(&grown.pixels), morph(&square, 2, false));
    for _ in 0..3 {
        crate::test_support::complete(&r);
        assert!(r.take_region().is_none(), "no cancelled result follows");
    }
}

#[test]
fn replacing_a_displayed_preview_repaints_only_around_either_outline() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "outline")], &[], &[], true);
    let cells = |x0: i32, x1: i32| Selection {
        affine: Affine([4., 0., 0., 4., 16., 16.]),
        ..from_bytes(&binary(|x, y| (x0..x1).contains(&x) && (2..6).contains(&y)))
    };
    let device = r.device.clone();
    let texture = |label| crate::create_target(&device, [128, 128], wgpu::TextureFormat::Rgba8UnormSrgb, label).0;
    let (retained, fresh) = (texture("retained outline"), texture("fresh outline"));
    let presenter = |r: &WgpuRasterizer| crate::ViewportPresenter::for_surface(r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    let mut kept = presenter(&r);
    kept.set_target_retention(true);
    r.set_selection_outline(Some(&cells(2, 6))).unwrap();
    kept.present(&r, &retained.create_view(&Default::default()), view(), [1.; 4]).unwrap();
    assert_eq!(kept.damage_area_pixels(), 128 * 128);
    r.set_selection_outline(Some(&cells(3, 8))).unwrap();
    kept.present(&r, &retained.create_view(&Default::default()), view(), [1.; 4]).unwrap();
    let repainted = kept.damage_area_pixels();
    assert!(repainted > 0 && repainted < 128 * 128 / 2, "a new preview repaints its outlines only: {repainted}");
    let mut full = presenter(&r);
    full.present(&r, &fresh.create_view(&Default::default()), view(), [1.; 4]).unwrap();
    assert!(page_bytes(&r, &retained) == page_bytes(&r, &fresh), "and matches a complete redraw");
    let outlined = page_bytes(&r, &fresh).chunks_exact(4).filter(|p| p[0] < 10 || p[0] > 245).count();
    assert!(outlined > 100, "the preview draws an outline: {outlined}");
}

#[test]
fn moving_a_pixel_selection_outline_keeps_its_uploaded_coverage() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "outline")], &[], &[], true);
    let selection = from_bytes(&binary(|x, y| x < 40 && y < 40));
    r.set_selection_outline(Some(&selection)).unwrap();
    let uploaded = r.display_selection.as_ref().unwrap().1.clone();
    let moved = selection.transformed(Affine([2., 0., 0., 1., 5., 0.])).unwrap();
    r.set_selection_outline(Some(&moved)).unwrap();
    let (shown, buffer) = r.display_selection.as_ref().unwrap();
    assert_eq!(shown, &moved, "the outline follows the new placement");
    assert_eq!(buffer, &uploaded, "without uploading the coverage again");
    r.set_selection_outline(Some(&from_bytes(&binary(|x, _| x < 20)))).unwrap();
    assert_ne!(r.display_selection.as_ref().unwrap().1, uploaded, "new coverage is uploaded");
}

#[test]
#[ignore = "hardware completed-selection refinement benchmark; run serially"]
fn selection_resize_latency() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "resize timing")],
        &[],
        &[],
        true,
    );
    r.document_extent = [2048, 2048];
    let bytes: Vec<u8> = (0..2048 * 2048)
        .map(|i| (20 + (i % 2048 * 13 + i / 2048 * 7) % 200) as u8)
        .collect();
    let words: Vec<_> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let input = Selection::pixels(Arc::new(
        SelectionPixels::bytes([2048, 2048], [0, 0, 2048, 2048], words).unwrap(),
    ));
    for radius in [5, 32, 128, -128] {
        let start = std::time::Instant::now();
        let result = receive_with_resize(
            &mut r,
            input.clone(),
            None,
            SelectionMode::New,
            0.,
            true,
            radius,
        );
        eprintln!(
            "soft selection 2048² resize {radius}px GPU plus capture: {:.3}ms",
            start.elapsed().as_secs_f64() * 1000.
        );
        if radius.abs() == 128 {
            assert_eq!(
                coverage(&result, 1024, 1024),
                if radius > 0 { 219 } else { 20 }
            );
        }
    }
}
