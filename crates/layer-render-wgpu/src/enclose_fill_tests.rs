use super::*;
use crate::test_support::{depth_source, receive_request, submit_ready};
use layer_core::{Affine, SelectionMode, SelectionPixels, color::{RgbSpace, SampleDepth}};
use layer_render::{RegionRefinement, RegionRequest, RegionResult, RegionSource, SelectionRefinement};
use std::{collections::VecDeque, sync::Arc};

fn polygon(points: &[[u32; 2]]) -> Selection {
    Selection::polygon(points.iter().map(|&[x, y]| Point { x: x as f32, y: y as f32 }).collect()).unwrap()
}
fn rectangle([x0, y0, x1, y1]: [u32; 4]) -> Selection {
    polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}
fn within(points: &[[u32; 2]], x: u32, y: u32) -> bool {
    let (x, y) = (x as f64 + 0.5, y as f64 + 0.5);
    let mut inside = false;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        let [ax, ay] = a.map(f64::from);
        let [bx, by] = b.map(f64::from);
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            inside = !inside;
        }
    }
    inside
}
fn barrier([x0, y0, x1, y1]: [u32; 4], x: u32, y: u32) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
        && (x == x0 || x + 1 == x1 || y == y0 || y + 1 == y1)
}
fn document(extent: [u32; 2], alpha: &[f32]) -> Document {
    let mut document = paint_document(extent, "Enclosure reference");
    placement::set_source(&mut document, depth_source(extent, SampleDepth::F32, RgbSpace::Srgb,
        extent[0] as usize * extent[1] as usize * 16 + 4096, |x, y| {
            [x as f32 / extent[0] as f32, y as f32 / extent[1] as f32, 0.3,
                alpha[(y * extent[0] + x) as usize]]
        }));
    document
}
fn frame(r: &mut WgpuRasterizer, document: &Document, extent: [u32; 2]) {
    submit_ready(r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
}
fn request(source: RegionSource, enclosure: Selection, refinement: RegionRefinement, tolerance: f32) -> RegionRequest {
    RegionRequest {
        request_id: 91,
        source,
        contiguous: true,
        position: [0, 0],
        tolerance,
        refinement,
        limit: None,
        enclosure: Some(Arc::new(enclosure)),
        selection: None,
    }
}
fn value(pixels: &SelectionPixels, x: u32, y: u32) -> u8 {
    let count = pixels.pixels_per_word();
    let bits = 32 / count;
    let value = (pixels.words()[(y * pixels.extent()[0].div_ceil(count) + x / count) as usize]
        >> ((x % count) * bits)) & ((1 << bits) - 1);
    if count == 4 { value as u8 } else { (value as f32 * 255. / 4.).round() as u8 }
}
fn check(result: &RegionResult, expected: &[u8], extent: [u32; 2]) {
    assert_eq!(result.request_id, 91);
    assert_eq!(result.placement, Affine::IDENTITY);
    assert_eq!(result.pixels.extent(), extent);
    let mut bounds = [extent[0], extent[1], 0, 0];
    for y in 0..extent[1] {
        for x in 0..extent[0] {
            let expected = expected[(y * extent[0] + x) as usize];
            assert_eq!(value(&result.pixels, x, y), expected, "{x},{y}");
            if expected != 0 {
                bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x + 1), bounds[3].max(y + 1)];
            }
        }
    }
    if bounds[2] == 0 { bounds = [0; 4]; }
    assert_eq!(result.pixels.bounds(), bounds);
}
fn box_morph(input: &[bool], [w, h]: [u32; 2], lower: i32, upper: i32, erode: bool, extend: bool) -> Vec<bool> {
    (0..w * h).map(|i| {
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        let mut selected = erode;
        for dy in lower..=upper {
            for dx in lower..=upper {
                let (xx, yy) = (x + dx, y + dy);
                let sample = if extend || ((0..w as i32).contains(&xx) && (0..h as i32).contains(&yy)) {
                    input[(yy.clamp(0, h as i32 - 1) as u32 * w + xx.clamp(0, w as i32 - 1) as u32) as usize]
                } else { false };
                selected = if erode { selected && sample } else { selected || sample };
            }
        }
        selected
    }).collect()
}
fn enclosed(alpha: &[f32], extent: [u32; 2], points: &[[u32; 2]], tolerance: f32, refinement: RegionRefinement) -> Vec<u8> {
    expected_region(alpha, extent, Some(points), None, tolerance, refinement)
}
fn expected_region(alpha: &[f32], extent: [u32; 2], points: Option<&[[u32; 2]]>, seed: Option<[u32; 2]>, tolerance: f32, refinement: RegionRefinement) -> Vec<u8> {
    let [w, h] = extent;
    let reference = seed.map_or(0., |[x, y]| alpha[(y * w + x) as usize]);
    let mut eligible: Vec<_> = alpha.iter().map(|a| (*a - reference).abs() <= tolerance).collect();
    let gap = refinement.gap_closing as i32;
    if gap > 0 {
        eligible = box_morph(&box_morph(&eligible, extent, -gap / 2, (gap + 1) / 2, true, true),
            extent, -(gap + 1) / 2, gap / 2, false, true);
    }
    let inside = |x: u32, y: u32| points.is_none_or(|points| within(points, x, y));
    let mut selected = vec![false; alpha.len()];
    let mut seen = vec![false; alpha.len()];
    let starts: Vec<usize> = match (points, seed) {
        (None, Some([x, y])) => vec![(y * w + x) as usize],
        _ => (0..alpha.len()).collect(),
    };
    for start in starts {
        if seen[start] || !eligible[start] { continue; }
        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        let mut contained = true;
        seen[start] = true;
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            contained &= x > 0 && x + 1 < w && y > 0 && y + 1 < h && inside(x, y);
            component.push(i);
            for (xx, yy) in [(x as i32 - 1, y as i32), (x as i32 + 1, y as i32),
                (x as i32, y as i32 - 1), (x as i32, y as i32 + 1)] {
                if !(0..w as i32).contains(&xx) || !(0..h as i32).contains(&yy) { continue; }
                let next = (yy as u32 * w + xx as u32) as usize;
                if eligible[next] && !seen[next] { seen[next] = true; queue.push_back(next); }
            }
        }
        if contained || points.is_none() { for i in component { selected[i] = true; } }
    }
    let expansion = refinement.expansion;
    if expansion != 0 {
        selected = box_morph(&selected, extent, -expansion.abs(), expansion.abs(), expansion < 0, false);
    }
    (0..w * h).map(|i| {
        if !inside(i % w, i / w) { return 0; }
        let center = selected[i as usize];
        if !refinement.smooth_edges { return if center { 255 } else { 0 }; }
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        let at = |dx: i32, dy: i32| selected[((y + dy).clamp(0, h as i32 - 1) as u32 * w
            + (x + dx).clamp(0, w as i32 - 1) as u32) as usize] as u32;
        let mut samples = 0u32;
        for dy in [-1, 1] {
            for dx in [-1, 1] {
                let count = center as u32 + at(dx, 0) + at(0, dy) + at(dx, dy);
                samples += u32::from(count > 2 || (count == 2 && center));
            }
        }
        samples = if center { samples.max(1) } else { samples.min(3) };
        let coverage = samples as f32 * 0.25;
        ((coverage * 4.).round() * 255. / 4.).round() as u8
    }).collect()
}

#[test]
fn enclose_fill_unions_closed_islands_excludes_open_partial_and_canvas_edge_regions() {
    let extent = [259, 97];
    let alpha: Vec<_> = (0..extent[0] * extent[1]).map(|i| {
        let (x, y) = (i % extent[0], i / extent[0]);
        let closed = barrier([18, 18, 63, 70], x, y) || barrier([94, 24, 141, 72], x, y)
            || barrier([225, 26, 257, 71], x, y);
        let open = barrier([166, 20, 206, 75], x, y) && !(y == 20 && x == 184);
        let edge = barrier([0, 33, 12, 65], x, y) && x != 0;
        if closed || open || edge { 1. } else { 0. }
    }).collect();
    let document = document(extent, &alpha);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    let before = r.readback_srgb_rgba8().unwrap();
    let points = [[4, 4], [247, 4], [247, 90], [4, 90]];
    let expected = enclosed(&alpha, extent, &points, 0., Default::default());
    assert_eq!(expected.iter().filter(|v| **v != 0).count(), 43 * 50 + 45 * 46);
    let mut held = Vec::new();
    for source in [RegionSource::Source(target(&document)), RegionSource::Composite] {
        for seed in [[0, 0], [18, 18], [33, 40]] {
            let mut query = request(source.clone(), polygon(&points), Default::default(), 0.);
            query.position = seed;
            let result = receive_request(&mut r, query);
            check(&result, &expected, extent);
            held.push(result.pixels);
        }
    }
    for bounds in [[80, 10, 155, 85], [150, 10, 220, 85], [4, 4, 247, 90], [0, 0, 259, 97]] {
        let points = [[bounds[0], bounds[1]], [bounds[2], bounds[1]], [bounds[2], bounds[3]], [bounds[0], bounds[3]]];
        check(&receive_request(&mut r, request(RegionSource::Composite, polygon(&points), Default::default(), 0.)),
            &enclosed(&alpha, extent, &points, 0., Default::default()), extent);
    }
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    assert_eq!(value(&held[0], 33, 40), 255);
    assert_eq!(value(&held[0], 112, 40), 255);
    for corner_reference in [true, false] {
        let mut corner_alpha = alpha.clone();
        if corner_reference { for y in 0..4 { for x in 0..4 { corner_alpha[(y * extent[0] + x) as usize] = 1.; } } }
        let mut references = self::document(extent, &corner_alpha);
        let ink = placement::occurrence_id(&references);
        let mut target = paint_document(extent, "Target flat");
        placement::set_source(&mut target, layer_core::color::source::rgba8_source(extent, |x, y|
            if !corner_reference && x < 4 && y < 4 { [30, 190, 80, 255] } else { [0; 4] }));
        let (target, _) = placement::append_paint(&mut references, "Target flat", placement::paint(&target).clone());
        frame(&mut r, &references, extent);
        let snapshot = references.snapshot();
        let source = RegionSource::Intersection([ink, target].into_iter()
            .map(|leaf| RegionSource::reference(snapshot.clone(), leaf).unwrap()).collect());
        let result = receive_request(&mut r, request(source, polygon(&points), Default::default(), 0.));
        assert_eq!(value(&result.pixels, 33, 40), 255, "opaque corner in {} must not seed enclosure matching", if corner_reference { "reference" } else { "target" });
        check(&result, &expected, extent);
    }
}

#[test]
fn enclose_fill_uses_alpha_thresholds_and_intersects_selection_after_containment() {
    let extent = [97, 83];
    let alpha: Vec<_> = (0..extent[0] * extent[1]).map(|i| {
        let (x, y) = (i % extent[0], i / extent[0]);
        if barrier([15, 17, 78, 68], x, y) { 0.5 }
        else if (16..77).contains(&x) && (18..67).contains(&y) {
            if x == 35 { 0.25 } else if x == 36 { 0.125 } else { 0. }
        } else { 0. }
    }).collect();
    let document = document(extent, &alpha);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    let points = [[8, 8], [89, 8], [89, 75], [8, 75]];
    for tolerance in [0., 0.124, 0.125, 0.249, 0.25, 0.499, 0.5, 1.] {
        for source in [RegionSource::Source(target(&document)), RegionSource::Composite] {
            check(&receive_request(&mut r, request(source, polygon(&points), Default::default(), tolerance)),
                &enclosed(&alpha, extent, &points, tolerance, Default::default()), extent);
        }
    }
    let mut query = request(RegionSource::Composite, polygon(&points), Default::default(), 0.25);
    query.selection = Some(SelectionRefinement {
        resize: 0,
        mode: SelectionMode::Intersect,
        antialias: true,
        feather: 0.,
        previous: Some(Arc::new(rectangle([0, 0, 40, 83]))),
        source_to_document: Affine::IDENTITY,
        keep_canvas_edges: false,
    });
    let mut expected = enclosed(&alpha, extent, &points, 0.25, Default::default());
    for y in 0..extent[1] { for x in 40..extent[0] { expected[(y * extent[0] + x) as usize] = 0; } }
    check(&receive_request(&mut r, query), &expected, extent);
}

#[test]
fn enclose_fill_keeps_diagonal_components_separate_and_rejects_each_canvas_edge() {
    let extent = [33, 33];
    let holes = [[15, 15], [16, 16], [0, 8], [8, 0], [32, 8], [8, 32]];
    let alpha: Vec<_> = (0..33 * 33).map(|i| if holes.contains(&[i % 33, i / 33]) { 0. } else { 1. }).collect();
    let document = document(extent, &alpha);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    for points in [vec![[1, 1], [16, 1], [16, 16], [1, 16]],
        vec![[0, 0], [33, 0], [33, 33], [0, 33]]] {
        let expected = enclosed(&alpha, extent, &points, 0., Default::default());
        assert_eq!(expected.iter().filter(|v| **v != 0).count(), if points[0] == [0, 0] { 2 } else { 1 });
        check(&receive_request(&mut r, request(RegionSource::Composite, polygon(&points), Default::default(), 0.)),
            &expected, extent);
    }
}

#[test]
fn enclose_fill_preserves_polygon_containment_gap_closing_expansion_and_smoothing() {
    let extent = [193, 101];
    let alpha: Vec<_> = (0..extent[0] * extent[1]).map(|i| {
        let (x, y) = (i % extent[0], i / extent[0]);
        let left = barrier([20, 18, 72, 79], x, y);
        let gap = barrier([106, 16, 163, 77], x, y) && !(y == 16 && x == 128);
        if left || gap { 1. } else { 0. }
    }).collect();
    let document = document(extent, &alpha);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    for points in [vec![[8, 8], [185, 8], [185, 92], [8, 92]],
        vec![[8, 8], [185, 8], [185, 34], [85, 34], [85, 92], [8, 92]],
        vec![[20, 18], [72, 18], [72, 79], [20, 79]]] {
        for refinement in [RegionRefinement::default(),
            RegionRefinement { gap_closing: 1, ..Default::default() },
            RegionRefinement { gap_closing: 2, expansion: 2, smooth_edges: false },
            RegionRefinement { gap_closing: 3, expansion: -2, smooth_edges: true },
            RegionRefinement { smooth_edges: true, ..Default::default() }] {
            let expected = enclosed(&alpha, extent, &points, 0., refinement);
            let result = receive_request(&mut r, request(RegionSource::Composite, polygon(&points), refinement, 0.));
            check(&result, &expected, extent);
        }
    }
}

fn uniform_document(extent: [u32; 2], alpha: &[f32]) -> Document {
    let mut document = paint_document(extent, "Windowed region reference");
    placement::set_source(&mut document, depth_source(extent, SampleDepth::F32, RgbSpace::Srgb,
        extent[0] as usize * extent[1] as usize * 16 + 4096, |x, y| [0.2, 0.3, 0.4, alpha[(y * extent[0] + x) as usize]]));
    document
}

#[test]
fn regions_beyond_one_storage_binding_label_bounded_windows_and_match_a_whole_canvas_flood() {
    let limit = 4u64 << 20;
    let mut r = limited_renderer(limit);
    let extent = [1300u32, 900u32];
    assert!(u64::from(extent[0]) * u64::from(extent[1]) * 4 > limit, "one parent per pixel exceeds the binding");
    let alpha: Vec<_> = (0..extent[0] * extent[1]).map(|i| {
        let (x, y) = (i % extent[0], i / extent[0]);
        let wall = x % 90 == 45 && if (x / 90) % 2 == 0 { y < 870 } else { y > 30 };
        let boxed = barrier([230, 200, 790, 700], x, y) || barrier([511, 511, 517, 517], x, y);
        let ring = barrier([1100, 100, 1290, 850], x, y) && !(x == 1100 && y == 513);
        if wall || boxed || ring || x == y { 1. } else { 0. }
    }).collect();
    let document = uniform_document(extent, &alpha);
    frame(&mut r, &document, extent);
    let refinements = [RegionRefinement::default(), RegionRefinement { gap_closing: 2, expansion: 2, smooth_edges: true },
        RegionRefinement { gap_closing: 1, expansion: -1, smooth_edges: false }];
    for refinement in refinements {
        for seed in [[3, 890], [240, 300], [514, 514], [512, 511], [230, 600], [1180, 513], [1299, 0]] {
            let mut query = request(RegionSource::Composite, rectangle([0, 0, 1, 1]), refinement, 0.);
            (query.enclosure, query.position) = (None, seed);
            check(&receive_request(&mut r, query), &expected_region(&alpha, extent, None, Some(seed), 0., refinement), extent);
        }
        let points = [[6, 9], [1290, 9], [1290, 450], [1250, 450], [1250, 890], [6, 890]];
        check(&receive_request(&mut r, request(RegionSource::Composite, polygon(&points), refinement, 0.)),
            &enclosed(&alpha, extent, &points, 0., refinement), extent);
    }
    let mut query = request(RegionSource::Composite, rectangle([0, 0, 1, 1]), Default::default(), 0.);
    (query.enclosure, query.position, query.contiguous) = (None, [3, 890], false);
    let expected: Vec<u8> = alpha.iter().map(|a| if *a == 0. { 255 } else { 0 }).collect();
    check(&receive_request(&mut r, query), &expected, extent);
    assert!(r.regions.as_ref().unwrap().flood.storage_bytes() <= limit, "label storage stays within one binding");
}

#[test]
fn regions_whose_coverage_exceeds_the_device_are_refused_and_the_renderer_keeps_working() {
    let mut r = limited_renderer(4 << 20);
    let large = [4096u32, 2100u32];
    let extent = [300u32, 200u32];
    assert!(!layer_render::CanvasRenderer::region_fits(&r, large) && layer_render::CanvasRenderer::region_fits(&r, extent));
    let document = paint_document(large, "Oversized region");
    frame(&mut r, &document, large);
    let mut query = request(RegionSource::Composite, rectangle([0, 0, 1, 1]), Default::default(), 0.);
    query.enclosure = None;
    assert!(matches!(r.request_region(query.clone()), Err(GpuRasterError::SizeOverflow)));
    assert_eq!(r.regions.as_ref().unwrap().flood.storage_bytes(), 48);
    let alpha = vec![0.; (extent[0] * extent[1]) as usize];
    frame(&mut r, &uniform_document(extent, &alpha), extent);
    check(&receive_request(&mut r, query), &vec![255; alpha.len()], extent);
}

#[test]
#[ignore = "hardware enclosure completion timing at reference canvas sizes"]
fn enclose_fill_workstation_completion_timings() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    eprintln!("ENCLOSE_FILL workstation adapter={:?} profile={} build={}", r.adapter.get_info(),
        if cfg!(debug_assertions) { "test" } else { "release" }, env!("CARGO_PKG_VERSION"));
    for extent in [[4248, 2832], [6000, 4000], [9504, 6336]] {
        let [w, h] = extent;
        let limits = r.device.limits();
        if !crate::flood::fits(extent, &limits) {
            eprintln!("ENCLOSE_FILL extent={w}x{h} unsupported max_storage_buffer_binding_size={} max_buffer_size={} qualification=unmeasured",
                limits.max_storage_buffer_binding_size, limits.max_buffer_size);
            continue;
        }
        let boxes = [[w / 8, h / 4, w / 3, 3 * h / 4], [w / 2, h / 4, 7 * w / 8, 3 * h / 4]];
        let mut document = paint_document(extent, "Generated enclosure benchmark");
        placement::set_source(&mut document, depth_source(extent, SampleDepth::U8, RgbSpace::Srgb,
            w as usize * h as usize * 4 + 4096, |x, y| {
                let alpha = if boxes.iter().any(|bounds| barrier(*bounds, x, y)) { 1. } else { 0. };
                [0.2, 0.3, 0.4, alpha]
            }));
        submit_ready(&mut r, FramePacket { reset_layers: true, view: view(), ..packet(document.scene(), extent) }).unwrap();
        let baseline = r.telemetry().resident_bytes;
        for (enclosed, refinement) in [false, true].into_iter().flat_map(|enclosed| [RegionRefinement::default(),
            RegionRefinement { gap_closing: 2, expansion: 2, smooth_edges: true }].map(|refinement| (enclosed, refinement))) {
            let mut samples = Vec::new();
            for run in 0..22 {
                let started = std::time::Instant::now();
                let mut query = request(RegionSource::Composite, rectangle([8, 8, w - 8, h - 8]), refinement, 0.);
                if !enclosed { (query.enclosure, query.position) = (None, [w / 4, h / 2]); }
                let result = receive_request(&mut r, query);
                let elapsed = started.elapsed().as_secs_f64() * 1000.;
                assert_eq!(value(&result.pixels, w / 4, h / 2), 255);
                assert_eq!(value(&result.pixels, 3 * w / 4, h / 2), if enclosed { 255 } else { 0 });
                assert_eq!(value(&result.pixels, 0, 0), 0);
                if run >= 2 { samples.push(elapsed); }
                eprintln!("ENCLOSE_FILL extent={w}x{h} enclosed={enclosed} refinement={refinement:?} run={run} completed_ms={elapsed:.3}");
            }
            samples.sort_by(f64::total_cmp);
            eprintln!("ENCLOSE_FILL extent={w}x{h} enclosed={enclosed} refinement={refinement:?} warm_median_ms={:.3} warm_max_ms={:.3} baseline_gpu_bytes={baseline} gpu_bytes={} region_bytes={} qualification=workstation_only",
                (samples[9] + samples[10]) * 0.5, samples[19], r.telemetry().resident_bytes,
                r.regions.as_ref().unwrap().storage_bytes());
        }
    }
}

#[test]
fn reference_intersection_matches_each_opaque_seed_across_classification_batches() {
    let extent = [1153, 1025];
    let mut document = paint_document(extent, "Red reference");
    placement::set_source(&mut document, layer_core::color::source::rgba8_source(extent, |x, _|
        if x < 700 { [230, 20, 40, 255] } else { [20, 20, 20, 255] }));
    let first = placement::occurrence_id(&document);
    let first_target = target(&document);
    let mut second = paint_document(extent, "Green reference");
    placement::set_source(&mut second, layer_core::color::source::rgba8_source(extent, |_, y|
        if y < 600 { [20, 210, 50, 255] } else { [30, 40, 220, 255] }));
    let (last, _) = placement::append_paint(&mut document, "Green reference", placement::paint(&second).clone());
    let root = document.composition().result;
    let stack = layer_core::authored::RecordChange::replace(&document.artwork.stacks, root,
        Some(layer_core::authored::Stack { entries: vec![last, first] })).unwrap();
    document.apply(layer_core::Edit::Stack(stack)).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    let before = r.readback_srgb_rgba8().unwrap();
    for (source, x_bound, y_bound) in [(RegionSource::Source(first_target), 700, extent[1]),
        (RegionSource::Composite, extent[0], 600)] {
        let mut query = request(source, rectangle([0, 0, extent[0], extent[1]]), Default::default(), 0.);
        query.position = [100, 100]; query.enclosure = None;
        let result = receive_request(&mut r, query);
        for y in (10..extent[1]-10).step_by(19) { for x in (10..extent[0]-10).step_by(17) {
            if x.abs_diff(x_bound) > 3 && y.abs_diff(y_bound) > 3 {
                assert_eq!(value(&result.pixels, x, y), if x < x_bound && y < y_bound { 255 } else { 0 }, "{x},{y}");
            }
        }}
    }
    document.artwork.occurrences.get_mut(first).unwrap().opacity = 0.17;
    document.artwork.occurrences.get_mut(first).unwrap().blend = layer_core::LayerBlend::Multiply;
    document.artwork.occurrences.get_mut(last).unwrap().opacity = 0.31;
    document.artwork.occurrences.get_mut(last).unwrap().blend = layer_core::LayerBlend::Screen;
    document.revision += 1;
    let snapshot = document.snapshot();
    let originals = [first, last].map(|h| snapshot.view().occurrence(h).unwrap().clone());
    let sources = [first, last].into_iter().map(|h| RegionSource::reference(snapshot.clone(), h).unwrap()).collect();
    for (h, original) in [first, last].into_iter().zip(&originals) {
        assert_eq!(snapshot.view().occurrence(h), Some(original));
        assert_eq!(document.scene().occurrence(h), Some(original));
    }
    let alpha: Vec<_> = (0..extent[0]*extent[1]).map(|i| if i%extent[0] < 700 && i/extent[0] < 600 { 0. } else { 1. }).collect();
    let expected = expected_region(&alpha, extent, None, Some([100, 100]), 0., Default::default());
    let mut query = request(RegionSource::Intersection(sources), rectangle([0, 0, extent[0], extent[1]]), Default::default(), 0.);
    query.position = [100, 100]; query.enclosure = None;
    let result = receive_request(&mut r, query);
    for y in (10..extent[1]-10).step_by(19) { for x in (10..extent[0]-10).step_by(17) {
        if x.abs_diff(700) > 3 && y.abs_diff(600) > 3 {
            assert_eq!(value(&result.pixels, x, y), expected[(y*extent[0]+x) as usize], "{x},{y}");
        }
    }}
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    let extent = [128; 2];
    let mut opaque = Document::new(layer_core::authored::PortableId::random(), 128, 128,
        layer_core::DocumentNames { paint: "Lines".into(), paper: "Paper".into() });
    placement::set_source(&mut opaque, layer_core::color::source::rgba8_source(extent, |x, y|
        if barrier([20, 20, 108, 108], x, y) { [0, 0, 0, 255] } else { [0; 4] }));
    let empty = paint_document(extent, "Empty target");
    let (target, _) = placement::append_paint(&mut opaque, "Empty target", placement::paint(&empty).clone());
    frame(&mut r, &opaque, extent);
    let snapshot = opaque.snapshot();
    let all = RegionSource::Scene { snapshot: snapshot.clone(), scope: layer_core::SceneScope::All };
    for source in [RegionSource::Composite, RegionSource::Intersection(vec![all,
        RegionSource::reference(snapshot, target).unwrap()])] {
        let result = receive_request(&mut r, request(source, rectangle([4, 4, 124, 124]), Default::default(), 0.));
        assert!(result.pixels.words().iter().all(|word| *word == 0), "opaque All visible layers has no transparent enclosed region");
    }
}

#[test]
fn reference_scene_keeps_nested_position_mask_and_clipping_without_changing_the_snapshot() {
    let extent = [128; 2];
    let mut document = paint_document(extent, "Clip base");
    placement::set_source(&mut document, layer_core::color::source::rgba8_source(extent, |x, y|
        if x < 50 && y < 100 { [20, 40, 210, 255] } else { [0; 4] }));
    let base = placement::occurrence_id(&document);
    let mut ink = paint_document(extent, "Reference");
    placement::set_source(&mut ink, layer_core::color::source::rgba8_source(extent, |_, _| [230, 40, 20, 255]));
    placement::set_mask(&mut ink, left_mask());
    let (leaf, _) = placement::append_paint(&mut document, "Reference", placement::paint(&ink).clone());
    let mask = placement::mask_snapshot(&ink);
    let source = layer_core::authored::RecordChange::insert(&document.artwork.coverage, mask.source);
    let mut owner = document.scene().occurrence(leaf).unwrap().clone();
    let mut use_ = mask.use_; use_.source = source.handle;
    owner.mask = Some(use_);
    owner.attachment = layer_core::Attachment::Clip;
    owner.offset = [7, 9]; owner.opacity = 0.2; owner.blend = layer_core::LayerBlend::Multiply;
    let owner = layer_core::authored::RecordChange::replace(&document.artwork.occurrences, leaf, Some(owner)).unwrap();
    let root = document.composition().result;
    let stack = layer_core::authored::RecordChange::replace(&document.artwork.stacks, root,
        Some(layer_core::authored::Stack { entries: vec![leaf, base] })).unwrap();
    document.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Coverage(source), layer_core::Edit::Occurrence(owner), layer_core::Edit::Stack(stack)])).unwrap();
    document.artwork.occurrences.get_mut(base).unwrap().offset = [7, 9];
    document.apply(document.group_layers_edit(&[leaf, base], layer_core::LayerBlend::Normal, "Translated group").unwrap()).unwrap();
    let group = document.scene().children(None)[0];
    document.artwork.occurrences.get_mut(group).unwrap().offset = [11, 13];
    document.artwork.occurrences.get_mut(group).unwrap().reference = true;
    let ancestor_mask = left_mask();
    let source = layer_core::authored::RecordChange::insert(&document.artwork.coverage, ancestor_mask.source);
    let mut owner = document.scene().occurrence(group).unwrap().clone();
    let mut use_ = ancestor_mask.use_; use_.source = source.handle; use_.offset = [-30, 0];
    owner.mask = Some(use_);
    let owner = layer_core::authored::RecordChange::replace(&document.artwork.occurrences, group, Some(owner)).unwrap();
    document.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Coverage(source), layer_core::Edit::Occurrence(owner)])).unwrap();
    let snapshot = document.snapshot();
    let original = snapshot.view().occurrence(leaf).unwrap().clone();
    let source = RegionSource::reference(snapshot.clone(), leaf).unwrap();
    let RegionSource::Scene { snapshot: normalized, .. } = &source else { unreachable!() };
    let adjusted = normalized.view().occurrence(leaf).unwrap();
    assert_eq!((adjusted.opacity, adjusted.blend), (1., layer_core::LayerBlend::Normal));
    assert_eq!((adjusted.offset, adjusted.attachment, &adjusted.mask), (original.offset, original.attachment, &original.mask));
    assert_eq!(snapshot.view().occurrence(leaf), Some(&original));
    assert_eq!(normalized.view().occurrence(group), snapshot.view().occurrence(group));
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut renderer, &document, extent);
    let mut query = request(RegionSource::Intersection(vec![source]), rectangle([0, 0, 128, 128]), Default::default(), 0.);
    query.position = [30, 40]; query.enclosure = None;
    let result = receive_request(&mut renderer, query);
    for y in (4u32..128).step_by(7) { for x in (4u32..128).step_by(7) {
        if [18, 45].iter().all(|edge| x.abs_diff(*edge) > 3) && [22, 122].iter().all(|edge| y.abs_diff(*edge) > 3) {
            assert_eq!(value(&result.pixels, x, y), if (18..45).contains(&x) && (22..122).contains(&y) { 255 } else { 0 }, "{x},{y}");
        }
    }}
    document.artwork.occurrences.get_mut(group).unwrap().visible = false;
    assert!(RegionSource::reference(document.snapshot(), leaf).is_none());
    assert_eq!(snapshot.view().occurrence(leaf), Some(&original));
}
