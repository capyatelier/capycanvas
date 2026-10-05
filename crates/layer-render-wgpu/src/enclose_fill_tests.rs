use super::*;
use crate::test_support::{depth_source, receive_request};
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
    r.submit(FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
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
    let [w, h] = extent;
    let mut eligible: Vec<_> = alpha.iter().map(|a| *a <= tolerance).collect();
    let gap = refinement.gap_closing as i32;
    if gap > 0 {
        eligible = box_morph(&box_morph(&eligible, extent, -gap / 2, (gap + 1) / 2, true, true),
            extent, -(gap + 1) / 2, gap / 2, false, true);
    }
    let mut selected = vec![false; alpha.len()];
    let mut seen = vec![false; alpha.len()];
    for start in 0..alpha.len() {
        if seen[start] || !eligible[start] { continue; }
        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        let mut contained = true;
        seen[start] = true;
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            contained &= x > 0 && x + 1 < w && y > 0 && y + 1 < h && within(points, x, y);
            component.push(i);
            for (xx, yy) in [(x as i32 - 1, y as i32), (x as i32 + 1, y as i32),
                (x as i32, y as i32 - 1), (x as i32, y as i32 + 1)] {
                if !(0..w as i32).contains(&xx) || !(0..h as i32).contains(&yy) { continue; }
                let next = (yy as u32 * w + xx as u32) as usize;
                if eligible[next] && !seen[next] { seen[next] = true; queue.push_back(next); }
            }
        }
        if contained { for i in component { selected[i] = true; } }
    }
    let expansion = refinement.expansion;
    if expansion != 0 {
        selected = box_morph(&selected, extent, -expansion.abs(), expansion.abs(), expansion < 0, false);
    }
    (0..w * h).map(|i| {
        if !within(points, i % w, i / w) { return 0; }
        let center = selected[i as usize];
        if refinement.smoothing == 0. { return if center { 255 } else { 0 }; }
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
        let coverage = (if center { 1. } else { 0. }) * (1. - refinement.smoothing)
            + samples as f32 * 0.25 * refinement.smoothing;
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
            RegionRefinement { gap_closing: 2, expansion: 2, smoothing: 0. },
            RegionRefinement { gap_closing: 3, expansion: -2, smoothing: 1. },
            RegionRefinement { smoothing: 1., ..Default::default() }] {
            let expected = enclosed(&alpha, extent, &points, 0., refinement);
            let result = receive_request(&mut r, request(RegionSource::Composite, polygon(&points), refinement, 0.));
            check(&result, &expected, extent);
        }
    }
}

#[test]
fn enclose_fill_rejects_unsupported_parent_storage_before_allocating_it() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let limits = r.device.limits();
    let w = 8192.min(limits.max_texture_dimension_2d);
    let limit = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
    let h = (limit / 4 / u64::from(w)) as u32 + 1;
    assert!(h <= limits.max_texture_dimension_2d);
    let document = paint_document([w, h], "Oversized region parent storage");
    r.submit(FramePacket { reset_layers: true, view: view(), ..packet(document.scene(), [w, h]) }).unwrap();
    for enclosed in [false, true] {
        let mut query = request(RegionSource::Composite, rectangle([0, 0, w, h]), Default::default(), 0.);
        if !enclosed { query.enclosure = None; }
        assert!(matches!(r.request_region(query), Err(GpuRasterError::SizeOverflow)));
        assert_eq!(r.regions.as_ref().unwrap().flood.storage_bytes(), 48);
    }
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
        let parent_bytes = u64::from(w) * u64::from(h) * 4;
        if parent_bytes > limits.max_storage_buffer_binding_size || parent_bytes > limits.max_buffer_size {
            eprintln!("ENCLOSE_FILL extent={w}x{h} unsupported=parent_storage required_bytes={parent_bytes} max_storage_buffer_binding_size={} max_buffer_size={} qualification=unmeasured",
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
        r.submit(FramePacket { reset_layers: true, view: view(), ..packet(document.scene(), extent) }).unwrap();
        let baseline = r.telemetry().resident_bytes;
        for refinement in [RegionRefinement::default(), RegionRefinement { gap_closing: 2, expansion: 2, smoothing: 1. }] {
            let mut samples = Vec::new();
            for run in 0..22 {
                let started = std::time::Instant::now();
                let result = receive_request(&mut r, request(RegionSource::Composite,
                    rectangle([8, 8, w - 8, h - 8]), refinement, 0.));
                let elapsed = started.elapsed().as_secs_f64() * 1000.;
                assert_eq!(value(&result.pixels, w / 4, h / 2), 255);
                assert_eq!(value(&result.pixels, 3 * w / 4, h / 2), 255);
                assert_eq!(value(&result.pixels, 0, 0), 0);
                if run >= 2 { samples.push(elapsed); }
                eprintln!("ENCLOSE_FILL extent={w}x{h} refinement={refinement:?} run={run} completed_ms={elapsed:.3}");
            }
            samples.sort_by(f64::total_cmp);
            eprintln!("ENCLOSE_FILL extent={w}x{h} refinement={refinement:?} warm_median_ms={:.3} warm_max_ms={:.3} baseline_gpu_bytes={baseline} gpu_bytes={} region_bytes={} qualification=workstation_only",
                (samples[9] + samples[10]) * 0.5, samples[19], r.telemetry().resident_bytes,
                r.regions.as_ref().unwrap().storage_bytes());
        }
    }
}
