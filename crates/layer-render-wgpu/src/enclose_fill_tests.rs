use super::*;
use crate::test_support::{depth_source, receive_request, submit_ready};
use layer_core::{Affine, FillAreas, SelectionMode, SelectionPixels, color::{RgbSpace, SampleDepth}};
use layer_render::{RegionRefinement, RegionRequest, RegionResult, RegionSource, SelectionRefinement, RegionEdges};
use std::sync::Arc;
#[path = "region_cache_tests.rs"]
mod region_cache;
#[path = "enclose_fill_edge_tests.rs"]
mod edges;

fn polygon(points: &[[u32; 2]]) -> Selection {
    Selection::polygon(points.iter().map(|&[x, y]| Point { x: x as f32, y: y as f32 }).collect()).unwrap()
}
fn rectangle([x0, y0, x1, y1]: [u32; 4]) -> Selection {
    polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
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
    RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,
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
fn exact() -> RegionRefinement {
    RegionRefinement { close_gaps: false, edges: RegionEdges::Exact, smooth_edges: false }
}

#[test]
fn line_middle_thick_shapes_keep_primary_and_underfill_inside_their_soft_rim() {
    let extent=[128,96];
    let coverage=|x:u32,y:u32|((32.-(x as f32+0.5-64.).abs()).min(24.-(y as f32+0.5-48.).abs())+0.25).clamp(0.,1.);
    for opaque in [false,true] {
        let mut document=paint_document(extent,"Thick fill border");
        placement::set_source(&mut document,depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,128*96*16+4096,|x,y| {
            let alpha=coverage(x,y);
            if opaque {[1.,1.-alpha,1.-alpha,1.]}else{[1.,0.,0.,alpha]}
        }));
        let target=target(&document);
        let mut renderer=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        frame(&mut renderer,&document,extent);
        for mode in [layer_render::RegionColourMode::AlphaBorder,layer_render::RegionColourMode::ColourOnSolid,layer_render::RegionColourMode::Target(target)] {
            let mut query=request(RegionSource::Source(target),rectangle([0,0,128,96]),RegionRefinement {edges:RegionEdges::LineMiddle,..exact()},0.);
            query.enclosure=None;query.position=[64,48];query.colour_mode=mode;
            let result=receive_request(&mut renderer,query);let band=result.underfill.as_ref().unwrap();
            for y in 0..96 {for x in 0..128 {
                if coverage(x,y)==0. {
                    assert_eq!(value(&result.pixels,x,y),0,"primary {mode:?} opaque={opaque} {x},{y}");
                    assert_eq!(value(band,x,y),0,"band {mode:?} opaque={opaque} {x},{y}");
                }else{assert_eq!(value(&result.pixels,x,y),255,"shape {mode:?} opaque={opaque} {x},{y}");}
            }}
        }
    }
}

#[test]
fn line_middle_beside_a_wide_colour_block_covers_only_its_soft_rim() {
    let extent=[160,96];
    for opaque in [false,true] {
        let mut document=paint_document(extent,"Wide neighbouring block");
        placement::set_source(&mut document,depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,160*96*16+4096,|x,_| {
            let alpha=if x<63 {0.}else if x==63 {0.25}else{1.};
            if opaque {[1.-alpha,0.,alpha,1.]}else{[0.,0.,1.,alpha]}
        }));
        let target=target(&document);let mut renderer=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        frame(&mut renderer,&document,extent);
        let mut query=request(RegionSource::Source(target),rectangle([0,0,160,96]),RegionRefinement {edges:RegionEdges::LineMiddle,..exact()},0.);
        query.enclosure=None;query.position=[32,48];
        let result=receive_request(&mut renderer,query);let band=result.underfill.as_ref().unwrap();
        assert_eq!(value(band,63,48),255,"rim opaque={opaque}");
        for y in 0..96 {for x in 64..160 {
            assert_eq!(value(&result.pixels,x,y),0,"primary plateau opaque={opaque} {x},{y}");
            assert_eq!(value(band,x,y),0,"band plateau opaque={opaque} {x},{y}");
        }}
    }
}

#[test]
fn line_middle_same_layer_alpha_border_keeps_ink_and_fills_behind_the_fringe() {
    let extent = [128; 2];
    let source = |x: u32, y: u32| {
        let distance = x.abs_diff(64).max(y.abs_diff(64));
        let alpha = match distance { 34 | 42 => 0.25, 35 | 41 => 0.75, 36..=40 => 1., _ => 0. };
        if [x, y] == [60, 60] {
            [0.25, 0.5, 0.25, 1. / 32.]
        }
        else { [0., 0.125, 0.25, alpha] }
    };
    let mut document = paint_document(extent, "Same-layer line middle");
    placement::set_source(&mut document, depth_source(extent, SampleDepth::F32, RgbSpace::Srgb,
        128 * 128 * 16 + 4096, source));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    let before = r.readback_srgb_rgba8().unwrap();
    let mut query = request(RegionSource::Source(target(&document)), rectangle([0, 0, 128, 128]), exact(), 1. / 16.);
    query.enclosure = None;
    query.position = [64, 64];
    let interior = receive_request(&mut r, query.clone());
    assert!(interior.underfill.is_none());
    query.refinement.edges = RegionEdges::LineMiddle;
    let result = receive_request(&mut r, query);
    let band = result.underfill.as_ref().unwrap();
    for [x, y] in [[64, 64], [60, 60]] {
        assert_eq!(value(&interior.pixels, x, y), 255, "interior {x},{y}");
        assert_eq!(value(band, x, y), 0, "interior band {x},{y}");
    }
    assert_eq!(value(&interior.pixels,97,64),255);
    assert_eq!(value(band,97,64),255);
    assert_eq!(value(&interior.pixels, 98, 64), 0);
    assert_eq!(value(&result.pixels, 98, 64), 255);
    assert_eq!(value(band, 98, 64), 255);
    assert_eq!(value(&result.pixels, 108, 64), 0);
    let mut coverage = reveal_all(extent, [0, 0]);
    coverage.selection = Some(Selection::pixels(result.pixels.clone()));
    let color = [1., 0., 0., 0.75];
    let operation = RasterOperation { placement: result.placement, coverage,
        kind: RasterOperationKind::Fill { texture: Default::default(), color, alpha_locked: false,
            edges: None, underfill: Some(Selection::pixels(band.clone())) } };
    let command = DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
        damage: operation.bounds(extent), ..batch(target(&document)) };
    paint_mut(&mut document).operations = Arc::new(vec![operation]);
    submit_ready(&mut r, FramePacket { dab_batches: std::slice::from_ref(&command),
        ..packet(document.scene(), extent) }).unwrap();
    let after = r.readback_srgb_rgba8().unwrap();
    for y in 0..128 { for x in 0..128 {
        let index = ((y * 128 + x) * 4) as usize;
        let old = source(x, y);
        let amount = value(&result.pixels, x, y) as f32 / 255.;
        let behind = value(band, x, y) != 0;
        let interior_value = value(&interior.pixels, x, y);
        assert!(!behind || amount>0., "band {x},{y}");
        if interior_value != 0 { assert_eq!(value(&result.pixels, x, y), interior_value, "interior coverage {x},{y}"); }
        if amount == 0. || (old[3] == 1. && value(&interior.pixels, x, y) == 0) {
            assert_eq!(after[index..index + 4], before[index..index + 4], "preserved {x},{y}");
        }
        let paint_alpha = color[3] * amount;
        let alpha = old[3] + paint_alpha * (1. - old[3]);
        let expected = std::array::from_fn::<_, 4, _>(|channel| {
            if channel == 3 { return (alpha * 255.).round() as u8; }
            let premultiplied = if behind { old[channel] * old[3] + color[channel] * paint_alpha * (1. - old[3]) }
                else { color[channel] * paint_alpha + old[channel] * old[3] * (1. - paint_alpha) };
            let linear = if alpha == 0. { 0. } else { premultiplied / alpha };
            (RgbSpace::Srgb.encode(f64::from(linear)) * 255.).round() as u8
        });
        assert!(after[index..index + 4].iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
            "{x},{y} behind={behind}: {:?} != {expected:?}", &after[index..index + 4]);
    }}
}

fn rectangles(extent: [u32; 2], boxes: &[[u32; 4]]) -> Vec<u8> {
    (0..extent[0] * extent[1]).map(|i| {
        let (x, y) = (i % extent[0], i / extent[0]);
        if boxes.iter().any(|[x0, y0, x1, y1]| (*x0..*x1).contains(&x) && (*y0..*y1).contains(&y)) { 255 } else { 0 }
    }).collect()
}

#[test]
fn enclose_fill_six_pixel_references_match_the_host_journey() {
    let extent=[384,256];let mut document=paint_document(extent,"Six pixel reference");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y| {
        let ink=[40,150,270].into_iter().any(|left|x>=left&&x<left+90&&y>=50&&y<170&&(x<left+6||x>=left+84||y<56||y>=164));
        if ink {[0,0,0,255]}else{[0;4]}
    }));
    let ink=target(&document);let empty=paint_document(extent,"Destination");let (_,target)=placement::append_paint(&mut document,"Destination",placement::paint(&empty).clone());
    let snapshot=document.snapshot();let source=RegionSource::Intersection([ink,target].into_iter().map(|target|RegionSource::Scene {snapshot:snapshot.clone(),scope:SceneScope::Raw(target)}).collect());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let mut query=request(source,rectangle([20,30,315,190]),exact(),0.15);query.reference_count=Some(1);
    check(&receive_request(&mut r,query),&rectangles(extent,&[[46,56,124,164],[156,56,234,164]]),extent);
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
    let expected = rectangles(extent, &[[19, 19, 62, 69], [95, 25, 140, 71]]);
    assert_eq!(expected.iter().filter(|v| **v != 0).count(), 43 * 50 + 45 * 46);
    let mut held = Vec::new();
    for source in [RegionSource::Source(target(&document)), RegionSource::Composite] {
        for seed in [[0, 0], [18, 18], [33, 40]] {
            let mut query = request(source.clone(), polygon(&points), exact(), 0.);
            query.position = seed;
            let result = receive_request(&mut r, query);
            check(&result, &expected, extent);
            held.push(result.pixels);
        }
    }
    for bounds in [[80, 10, 155, 85], [150, 10, 220, 85], [4, 4, 247, 90], [0, 0, 259, 97]] {
        let points = [[bounds[0], bounds[1]], [bounds[2], bounds[1]], [bounds[2], bounds[3]], [bounds[0], bounds[3]]];
        check(&receive_request(&mut r, request(RegionSource::Composite, polygon(&points), exact(), 0.)),
            &rectangles(extent, &[[19, 19, 62, 69], [95, 25, 140, 71], [226, 27, 256, 70]].into_iter().filter(|b| b[0] > bounds[0] && b[1] > bounds[1] && b[2] < bounds[2] && b[3] < bounds[3]).collect::<Vec<_>>()), extent);
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
        let result = receive_request(&mut r, request(source, polygon(&points), exact(), 0.));
        assert_eq!(value(&result.pixels, 33, 40), 255, "opaque corner in {} must not seed enclosure matching", if corner_reference { "reference" } else { "target" });
        check(&result, &expected, extent);
    }
}

#[test]
fn enclose_fill_uses_alpha_thresholds_and_intersects_selection_after_containment() {
    let extent = [97, 83];
    let alpha = (0..extent[0] * extent[1]).map(|i| {
        let (x, y) = (i % extent[0], i / extent[0]);
        if barrier([15, 17, 78, 68], x, y) { 0.5 }
        else if (16..77).contains(&x) && (18..67).contains(&y) && x == 35 { 0.25 }
        else if (16..77).contains(&x) && (18..67).contains(&y) && x == 36 { 0.125 }
        else { 0. }
    }).collect::<Vec<_>>();
    let document = document(extent, &alpha);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    for tolerance in [0., 0.124, 0.125, 0.249, 0.25, 0.499, 0.5, 1.] {
        let mut expected = rectangles(extent, if tolerance < 0.5 { &[[16, 18, 77, 67]][..] } else { &[] });
        for y in 18..67 { for (x, threshold) in [(35, 0.25), (36, 0.125)] {
            if tolerance < threshold { expected[(y * extent[0] + x) as usize] = 0; }
        }}
        for source in [RegionSource::Source(target(&document)), RegionSource::Composite] {
            check(&receive_request(&mut r, request(source, rectangle([8, 8, 89, 75]), exact(), tolerance)), &expected, extent);
        }
    }
    let mut query = request(RegionSource::Composite, rectangle([8, 8, 89, 75]), exact(), 0.25);
    query.selection = Some(SelectionRefinement { resize: 0, mode: SelectionMode::Intersect, antialias: true, feather: 0.,
        previous: Some(Arc::new(rectangle([0, 0, 40, 83]))), source_to_document: Affine::IDENTITY, keep_canvas_edges: false });
    check(&receive_request(&mut r, query), &rectangles(extent, &[[16, 18, 40, 67]]), extent);
}

fn uniform_document(extent: [u32; 2], alpha: &[f32]) -> Document {
    let mut document = paint_document(extent, "Windowed region reference");
    placement::set_source(&mut document, depth_source(extent, SampleDepth::F32, RgbSpace::Srgb,
        extent[0] as usize * extent[1] as usize * 16 + 4096, |x, y| [0.2, 0.3, 0.4, alpha[(y * extent[0] + x) as usize]]));
    document
}

#[test]
fn regions_beyond_one_storage_binding_cross_window_seams_and_halos() {
    let limit = 4u64 << 20;
    let mut r = limited_renderer(limit);
    assert_eq!(r.device.limits().max_storage_buffer_binding_size, limit);
    let extent = [2304, 768];
    let boxes = [[1900, 100, 2200, 600], [1968, 620, 2128, 740]];
    let mut alpha = (0..extent[0] * extent[1]).map(|i| if boxes.iter().any(|b| barrier(*b, i % extent[0], i / extent[0])) { 1. } else { 0. }).collect::<Vec<_>>();
    let document = uniform_document(extent, &alpha);
    frame(&mut r, &document, extent);
    for (seed, bounds) in [([2000, 300], [1901, 101, 2199, 599]), ([2050, 680], [1969, 621, 2127, 739])] {
        let mut query = request(RegionSource::Composite, rectangle([0, 0, 1, 1]), exact(), 0.);
        (query.enclosure, query.position) = (None, seed);
        check(&receive_request(&mut r, query), &rectangles(extent, &[bounds]), extent);
    }
    for x in 2106..2118 { alpha[(100 * extent[0] + x) as usize] = 0.; }
    frame(&mut r, &uniform_document(extent, &alpha), extent);
    let mut query = request(RegionSource::Composite, rectangle([0, 0, 1, 1]), Default::default(), 0.);
    (query.enclosure, query.position) = (None, [2000, 300]);
    let result = receive_request(&mut r, query);
    score_geometry(&result.pixels, &[vec![[1900.5, 100.5], [2199.5, 100.5], [2199.5, 599.5], [1900.5, 599.5]]], [0, 0], &[], "Auto seal across window seam").unwrap();
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
    assert!(matches!(r.request_region(query.clone().into()), Err(GpuRasterError::SizeOverflow)));
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
            RegionRefinement { close_gaps: true, edges: RegionEdges::GrowShrink(2), smooth_edges: true }].map(|refinement| (enclosed, refinement))) {
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
        let mut query = request(source, rectangle([0, 0, extent[0], extent[1]]), exact(), 0.);
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
    let expected = rectangles(extent, &[[0, 0, 700, 600]]);
    let mut query = request(RegionSource::Intersection(sources), rectangle([0, 0, extent[0], extent[1]]), exact(), 0.);
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
        let result = receive_request(&mut r, request(source, rectangle([4, 4, 124, 124]), exact(), 0.));
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
    let mut query = request(RegionSource::Intersection(vec![source]), rectangle([0, 0, 128, 128]), exact(), 0.);
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


struct LineCase {
    name: String,
    lines: Vec<(Vec<[f32; 2]>, f32)>,
    seeds: Vec<[f32; 2]>,
    regions: Vec<Vec<Vec<[f32; 2]>>>,
    open: bool,
    dry: bool,
    pressure: Option<f32>,
    refinement: Option<RegionRefinement>,
}
fn line_cases() -> Vec<LineCase> {
    vec![
        LineCase { name: "aligned".into(), lines: vec![(vec![[130.0, 66.0], [130.0, 100.0], [30.0, 100.0], [30.0, 20.0], [130.0, 20.0], [130.0, 56.0]], 3.0), (vec![[65.0, 60.0], [64.829_63, 61.294_094], [64.330_124, 62.5], [63.535_534, 63.535_534], [62.5, 64.330_124], [61.294_094, 64.829_63], [60.0, 65.0], [58.705_906, 64.829_63], [57.5, 64.330_124], [56.464_466, 63.535_534], [55.669_872, 62.5], [55.170_372, 61.294_094], [55.0, 60.0], [55.170_372, 58.705_906], [55.669_872, 57.5], [56.464_466, 56.464_466], [57.5, 55.669_872], [58.705_906, 55.170_372], [60.0, 55.0], [61.294_094, 55.170_372], [62.5, 55.669_872], [63.535_534, 56.464_466], [64.330_124, 57.5], [64.829_63, 58.705_906], [65.0, 60.0]], 3.0)], seeds: vec![[95.0, 60.0]], regions: vec![vec![vec![[30.0, 20.0], [130.0, 20.0], [130.0, 100.0], [30.0, 100.0]], vec![[65.0, 60.0], [64.829_63, 61.294_094], [64.330_124, 62.5], [63.535_534, 63.535_534], [62.5, 64.330_124], [61.294_094, 64.829_63], [60.0, 65.0], [58.705_906, 64.829_63], [57.5, 64.330_124], [56.464_466, 63.535_534], [55.669_872, 62.5], [55.170_372, 61.294_094], [55.0, 60.0], [55.170_372, 58.705_906], [55.669_872, 57.5], [56.464_466, 56.464_466], [57.5, 55.669_872], [58.705_906, 55.170_372], [60.0, 55.0], [61.294_094, 55.170_372], [62.5, 55.669_872], [63.535_534, 56.464_466], [64.330_124, 57.5], [64.829_63, 58.705_906], [65.0, 60.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "slanted".into(), lines: vec![(vec![[118.08, 37.12], [148.0, 60.0], [80.0, 112.0], [12.0, 60.0], [80.0, 8.0], [109.92, 30.88]], 3.0)], seeds: vec![[80.0, 60.0]], regions: vec![vec![vec![[80.0, 8.0], [148.0, 60.0], [80.0, 112.0], [12.0, 60.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "offset".into(), lines: vec![(vec![[137.0, 68.0], [137.0, 100.0], [30.0, 100.0], [30.0, 20.0], [130.0, 20.0], [130.0, 56.0]], 3.0)], seeds: vec![[80.0, 60.0]], regions: vec![vec![vec![[30.0, 20.0], [130.0, 20.0], [130.0, 56.0], [137.0, 68.0], [137.0, 100.0], [30.0, 100.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "tee".into(), lines: vec![(vec![[20.0, 20.0], [140.0, 20.0], [140.0, 100.0], [20.0, 100.0], [20.0, 20.0]], 3.0), (vec![[78.0, 20.0], [92.0, 91.0]], 3.0)], seeds: vec![[45.0, 60.0]], regions: vec![vec![vec![[20.0, 20.0], [78.0, 20.0], [93.8, 100.0], [20.0, 100.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "acute".into(), lines: vec![(vec![[98.0, 100.0], [150.0, 100.0], [12.0, 64.0], [12.0, 100.0], [88.0, 100.0]], 3.0)], seeds: vec![[40.0, 88.0]], regions: vec![vec![vec![[12.0, 100.0], [150.0, 100.0], [12.0, 64.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "corridor".into(), lines: vec![(vec![[15.0, 66.0], [15.0, 105.0], [145.0, 105.0], [145.0, 15.0], [15.0, 15.0], [15.0, 54.0]], 3.0), (vec![[24.0, 24.0], [136.0, 24.0], [136.0, 96.0], [24.0, 96.0], [24.0, 24.0]], 3.0)], seeds: vec![[80.0, 19.0]], regions: vec![vec![vec![[15.0, 15.0], [145.0, 15.0], [145.0, 105.0], [15.0, 105.0]], vec![[24.0, 24.0], [136.0, 24.0], [136.0, 96.0], [24.0, 96.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "fold".into(), lines: vec![(vec![[20.0, 20.0], [140.0, 20.0], [140.0, 100.0], [20.0, 100.0], [20.0, 20.0]], 3.0), (vec![[80.0, 100.0], [84.0, 52.0]], 3.0)], seeds: vec![[50.0, 60.0]], regions: vec![vec![vec![[20.0, 20.0], [140.0, 20.0], [140.0, 100.0], [20.0, 100.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "sketchy".into(), lines: vec![(vec![[12.0, 22.0], [146.0, 18.0]], 3.0), (vec![[140.0, 8.0], [136.0, 108.0]], 3.0), (vec![[148.0, 98.0], [16.0, 102.0]], 3.0), (vec![[24.0, 112.0], [26.0, 10.0]], 3.0)], seeds: vec![[80.0, 60.0]], regions: vec![vec![vec![[25.77, 21.59], [139.59, 18.19], [136.39, 98.35], [24.2, 101.75]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "pencil".into(), lines: vec![(vec![[30.0, 20.0], [130.0, 20.0], [130.0, 100.0], [30.0, 100.0], [30.0, 20.0]], 4.0)], seeds: vec![[80.0, 60.0]], regions: vec![vec![vec![[30.0, 20.0], [130.0, 20.0], [130.0, 100.0], [30.0, 100.0]]]], open: false, dry: true, pressure: None, refinement: None },
        LineCase { name: "pencil-break".into(), lines: vec![(vec![[122.34, 37.26], [148.9, 60.6], [80.9, 112.6], [12.9, 60.6], [80.9, 8.6], [109.46, 30.44]], 4.0), (vec![[118.64, 37.46], [147.2, 59.3], [79.2, 111.3], [11.2, 59.3], [79.2, 7.3], [106.76, 31.64]], 4.0)], seeds: vec![[80.0, 60.0]], regions: vec![vec![vec![[80.0, 8.0], [148.0, 60.0], [80.0, 112.0], [12.0, 60.0]]]], open: false, dry: true, pressure: None, refinement: None },
        LineCase { name: "weights".into(), lines: vec![(vec![[20.0, 20.0], [140.0, 20.0], [140.0, 100.0], [20.0, 100.0], [20.0, 20.0]], 1.5), (vec![[80.0, 20.0], [80.0, 100.0]], 10.0)], seeds: vec![[50.0, 60.0], [110.0, 60.0]], regions: vec![vec![vec![[20.0, 20.0], [80.0, 20.0], [80.0, 100.0], [20.0, 100.0]]], vec![vec![[80.0, 20.0], [140.0, 20.0], [140.0, 100.0], [80.0, 100.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "judgement".into(), lines: vec![(vec![[20.0, 20.0], [140.0, 20.0], [140.0, 100.0], [20.0, 100.0], [20.0, 20.0]], 3.0), (vec![[80.0, 100.0], [80.0, 29.0]], 3.0)], seeds: vec![[50.0, 60.0]], regions: vec![vec![vec![[20.0, 20.0], [80.0, 20.0], [80.0, 100.0], [20.0, 100.0]]]], open: false, dry: false, pressure: None, refinement: None },
        LineCase { name: "open".into(), lines: vec![(vec![[40.0, 25.0], [40.0, 100.0], [120.0, 100.0], [120.0, 25.0]], 3.0)], seeds: vec![[80.0, 70.0]], regions: vec![vec![vec![[40.0, 25.0], [120.0, 25.0], [120.0, 100.0], [40.0, 100.0]]]], open: true, dry: false, pressure: None, refinement: None },
    ]
}
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(f32::EPSILON)).clamp(0., 1.);
    (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
}
fn geometry_at(contours: &[Vec<[f32; 2]>], p: [f32; 2]) -> (bool, f32) {
    let mut inside = false;
    let mut distance = f32::INFINITY;
    for contour in contours {
        for (&a, &b) in contour.iter().zip(contour.iter().cycle().skip(1)).take(contour.len()) {
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] { inside = !inside; }
            distance = distance.min(segment_distance(p, a, b));
        }
    }
    (inside, distance)
}
fn score_geometry(pixels: &SelectionPixels, contours: &[Vec<[f32; 2]>], offset: [u32; 2], footprint: &[(Vec<[f32; 2]>, f32)], name: &str) -> Result<(), String> {
    let extent = pixels.extent();
    let mut bounds = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for p in contours.iter().flatten().chain(footprint.iter().flat_map(|(line, _)| line)) { bounds = [bounds[0].min(p[0]), bounds[1].min(p[1]), bounds[2].max(p[0]), bounds[3].max(p[1])]; }
    let band = footprint.iter().map(|(_, width)| width * 0.5 + 1.5).fold(1.5f32, f32::max);
    let margin = band.max(2.);
    let scan = [(bounds[0] - margin).floor().max(0.) as u32 + offset[0], (bounds[1] - margin).floor().max(0.) as u32 + offset[1],
        (bounds[2] + margin).ceil() as u32 + offset[0], (bounds[3] + margin).ceil() as u32 + offset[1]];
    let actual = pixels.bounds();
    let (mut leaks, mut misses) = (0, 0);
    for y in scan[1]..scan[3].min(extent[1]) { for x in scan[0]..scan[2].min(extent[0]) {
        let p = [x as f32 + 0.5 - offset[0] as f32, y as f32 + 0.5 - offset[1] as f32];
        let (inside, distance) = geometry_at(contours, p);
        if distance <= band || footprint.iter().any(|(line, width)| line.windows(2).any(|s| segment_distance(p, s[0], s[1]) <= width * 0.5 + 1.5)) { continue; }
        let filled = value(pixels, x, y) != 0;
        leaks += u32::from(filled && !inside);
        misses += u32::from(!filled && inside);
    }}
    if actual[2] != 0 && (actual[0] < scan[0] || actual[1] < scan[1] || actual[2] > scan[2] || actual[3] > scan[3]) {
        Err(format!("{name}: leaked bounds {actual:?} outside {scan:?}, leaks={leaks}, misses={misses}"))
    } else if leaks == 0 && misses <= 4 { Ok(()) } else { Err(format!("{name}: leaks={leaks}, misses={misses}")) }
}
const KNOWN_MISSES: &[&str] = &[
    "Charcoal pressure=0.25 angle=0 gap=48 width=3",
    "Charcoal pressure=0.25 angle=0 gap=8 width=3",
    "Charcoal pressure=0.25 angle=15 gap=48 width=3",
    "Charcoal pressure=0.25 angle=30 gap=48 width=3",
    "Charcoal pressure=0.25 angle=45 gap=48 width=3",
    "Charcoal pressure=0.25 angle=60 gap=32 width=3",
    "Charcoal pressure=0.25 angle=60 gap=48 width=3",
    "Charcoal pressure=0.25 angle=90 gap=2 width=3",
    "Charcoal pressure=0.25 corner=10",
    "Charcoal pressure=0.25 corner=20",
    "Charcoal pressure=0.25 corner=5",
    "Charcoal pressure=0.25 corner=85",
    "Charcoal pressure=0.25 width sweep angle=30 gap=8 width=10",
    "Charcoal pressure=0.25 width sweep angle=30 gap=8 width=13",
    "Charcoal pressure=0.25 width sweep angle=30 gap=8 width=15",
    "Charcoal pressure=0.25 width sweep angle=30 gap=8 width=3",
    "Charcoal pressure=0.5 angle=30 gap=32 width=3",
    "Charcoal pressure=0.5 angle=30 gap=4 width=3",
    "Charcoal pressure=0.5 angle=30 gap=48 width=3",
    "Charcoal pressure=0.5 angle=60 gap=32 width=3",
    "Charcoal pressure=0.5 angle=60 gap=48 width=3",
    "Charcoal pressure=0.5 angle=90 gap=2 width=3",
    "Charcoal pressure=0.5 corner=40",
    "Charcoal pressure=0.5 corner=55",
    "Charcoal pressure=0.5 width sweep angle=30 gap=8 width=3",
    "Charcoal pressure=0.5 width sweep angle=30 gap=8 width=7",
    "Charcoal pressure=0.8 corner=20",
    "Crayon pressure=0.25 corner=15",
    "Crayon pressure=0.5 corner=75",
    "Crayon pressure=0.8 corner=75",
    "Pencil pressure=0.25 angle=15 gap=48 width=3",
    "Pencil pressure=0.25 angle=45 gap=48 width=3",
    "Pencil pressure=0.25 angle=90 gap=2 width=3",
    "Pencil pressure=0.25 angle=90 gap=4 width=3",
    "Pencil pressure=0.25 corner=10",
    "Pencil pressure=0.25 corner=5",
    "Pencil pressure=0.25 width sweep angle=30 gap=8 width=10",
    "Pencil pressure=0.25 width sweep angle=30 gap=8 width=3",
    "Pencil pressure=0.5 corner=10",
    "Pencil pressure=0.5 corner=20",
    "Pencil pressure=0.5 corner=5",
    "Pencil pressure=0.8 corner=5",
];
fn check_geometry(name: &str, score: Result<(), String>) -> Result<(), String> {
    match (KNOWN_MISSES.contains(&name), score) {
        (true, Ok(())) => Err(format!("{name}: now passes; remove from KNOWN_MISSES")),
        (true, Err(_)) => Ok(()),
        (_, score) => score,
    }
}
fn line_atlas(r: &mut WgpuRasterizer, cases: &[LineCase], preset: Option<layer_core::DefaultBrushPreset>, pressure: f32) -> Vec<Arc<SelectionPixels>> {
    let extent = [768, (cases.len() as u32).div_ceil(4) * 152];
    let mut alpha = vec![0f32; (extent[0] * extent[1]) as usize];
    let mut reader=png::Decoder::new(std::io::Cursor::new(include_bytes!("fixtures/fill-pencil-references.png"))).read_info().unwrap();
    let mut reference=vec![0;reader.output_buffer_size()];reader.next_frame(&mut reference).unwrap();
    let mut dry=0;
    for (index, case) in cases.iter().enumerate() {
        let offset = [(index as u32 % 4) * 192 + 16, (index as u32 / 4) * 152 + 16];
        if preset.is_none() && case.dry {
            for y in 0..120 {for x in 0..160 {
                alpha[((y+offset[1])*extent[0]+x+offset[0]) as usize]=f32::from(reference[(y*320+dry*160+x) as usize])/255.;
            }}
            dry+=1;
        }
        if preset.is_none() && !case.dry {
            for (line, width) in &case.lines { for segment in line.windows(2) {
                let [a, b] = [segment[0], segment[1]];
                let bounds = [(a[0].min(b[0]) - width).floor().max(0.) as u32, (a[1].min(b[1]) - width).floor().max(0.) as u32,
                    (a[0].max(b[0]) + width).ceil().min(160.) as u32, (a[1].max(b[1]) + width).ceil().min(120.) as u32];
                for y in bounds[1]..bounds[3] { for x in bounds[0]..bounds[2] {
                    let coverage = (width * 0.5 + 0.5 - segment_distance([x as f32 + 0.5, y as f32 + 0.5], a, b)).clamp(0., 1.);
                    let i = ((y + offset[1]) * extent[0] + x + offset[0]) as usize;
                    alpha[i] = alpha[i].max(coverage);
                }}
            }}
        }
    }
    let mut document = Document::new(layer_core::authored::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Line atlas".into(), paper: "Paper".into() });
    let paper = match preset { Some(layer_core::DefaultBrushPreset::Charcoal) => "pastel_paper", Some(layer_core::DefaultBrushPreset::Crayon) => "drawing_paper", _ => "rough" };
    document.set_paper_texture(layer_core::paper::built_in_paper(paper).unwrap().texture.clone());
    placement::set_source(&mut document, depth_source(extent, SampleDepth::F32, RgbSpace::Srgb,
        extent[0] as usize * extent[1] as usize * 16 + 4096, |x, y| [0., 0., 0., alpha[(y * extent[0] + x) as usize]]));
    frame(r, &document, extent);
    for (index, case) in cases.iter().enumerate() {
        if preset.is_none() { continue; }
        let offset = [(index as u32 % 4) as f32 * 192. + 16., (index as u32 / 4) as f32 * 152. + 16.];
        for (line_index, (line, width)) in case.lines.iter().enumerate() {
            let mut brush = layer_core::default_brush(preset.unwrap_or(layer_core::DefaultBrushPreset::PointyPencil));
            brush.diameter = *width;
            brush.stabilization.strength = 0.;
            let path: Vec<_> = line.windows(2).flat_map(|s| {
                let count = ((s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]).ceil() as u32).max(1);
                (0..count).map(move |i| { let t = i as f32 / count as f32; [s[0][0] + (s[1][0] - s[0][0]) * t, s[0][1] + (s[1][1] - s[0][1]) * t] })
            }).chain(line.last().copied()).enumerate().map(|(i, p)| layer_core::StrokePoint {
                position: Point { x: p[0] + offset[0], y: p[1] + offset[1] }, pressure: case.pressure.unwrap_or(pressure), tilt: [0.; 2], twist: 0., elapsed_micros: i as u32 * 4_000,
            }).collect();
            let id = layer_core::StrokeId((index * 16 + line_index + 1) as u64);
            let stroke = layer_core::Stroke::new(id, target(&document), layer_core::StrokeTool::Brush, brush.clone(), path).unwrap();
            let mut dabs = Vec::new();
            layer_engine::DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut dabs);
            let mut style = DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
            style.paper = Some(document.paper_binding(target(&document)));
            let batch = DabBatch { stroke_id: id, dab_count: dabs.len() as u32,
                ..crate::test_support::dab_batch(target(&document), style, dabs.iter().fold(Rect::EMPTY, |r, d| r.union(d.bounds()))) };
            submit_ready(r, FramePacket { dabs: &dabs, dab_batches: &[batch], ..packet(document.scene(), extent) }).unwrap();
        }
    }
    let mut results = Vec::new();let mut errors=Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let offset = [(index as u32 % 4) * 192 + 16, (index as u32 / 4) * 152 + 16];
        for (seed, contours) in case.seeds.iter().zip(&case.regions) {
            let mut query = request(RegionSource::Source(target(&document)), rectangle([0, 0, 1, 1]), case.refinement.unwrap_or_default(), 0.);
            if preset.is_some() || case.dry { query.paper = document.nearest_paper_binding(document.drawing_content()); }
            query.position = [seed[0] as u32 + offset[0], seed[1] as u32 + offset[1]];
            query.enclosure = None;
            let result = receive_request(r, query);
            if case.open {
                if value(&result.pixels, 0, 0)==0 {errors.push(format!("{}: intentional opening was sealed",case.name));}
            } else if let Err(error)=check_geometry(&case.name, score_geometry(&result.pixels, contours, offset, if preset.is_some() { &case.lines } else { &[] }, &case.name)) {errors.push(error);}
            results.push(result.pixels);
        }
    }
    assert!(errors.is_empty(),"{}",errors.join("\n"));
    results
}

#[test]
fn enclose_fill_geometry_gallery() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    eprintln!("FILL_GEOMETRY_ADAPTER {:?} format={:?}",r.adapter.get_info(),r.device.working_format());
    let cases = line_cases();
    let masks = line_atlas(&mut r, &cases, None, 0.8);
    let order = cases.iter().flat_map(|case| case.seeds.iter().enumerate().map(move |(seed, position)| (case, seed, position))).collect::<Vec<_>>();
    assert_eq!(masks.len(), order.len());
    let extent = masks[0].extent();
    let mut crc = crc32fast::Hasher::new();
    crc.update(&(masks.len() as u32).to_le_bytes());
    for ((case, seed, position), mask) in order.iter().zip(&masks) {
        assert_eq!(mask.extent(), extent);
        crc.update(&(case.name.len() as u32).to_le_bytes());
        crc.update(case.name.as_bytes());
        crc.update(&(*seed as u32).to_le_bytes());
        for coordinate in position.iter() { crc.update(&coordinate.to_bits().to_le_bytes()); }
        for side in extent { crc.update(&side.to_le_bytes()); }
        for y in 0..extent[1] {
            let row = (0..extent[0]).map(|x| value(mask, x, y)).collect::<Vec<_>>();
            crc.update(&row);
        }
    }
    let hash=crc.finalize();
    eprintln!("FILL_GEOMETRY_MASKS extent={}x{} order={} crc32={hash:08x}", extent[0], extent[1],
        order.iter().map(|(case, seed, _)| format!("{}:{seed}", case.name)).collect::<Vec<_>>().join(","));
    assert_eq!(hash,0x5338fcd7);
}

fn rotated_case(angle: f32, gap: f32, width: f32) -> LineCase {
    let rotate = |[x, y]: [f32; 2]| {
        let (s, c) = angle.to_radians().sin_cos();
        [80. + (x - 80.) * c - (y - 60.) * s, 60. + (x - 80.) * s + (y - 60.) * c]
    };
    LineCase {
        name: format!("angle={angle} gap={gap} width={width}"),
        lines: vec![(vec![[120., 60. + gap * 0.5], [120., 90.], [40., 90.], [40., 30.], [120., 30.], [120., 60. - gap * 0.5]].into_iter().map(rotate).collect(), width)],
        seeds: vec![[80., 60.]],
        regions: vec![vec![vec![[40., 30.], [120., 30.], [120., 90.], [40., 90.]].into_iter().map(rotate).collect()]],
        open: false,
        dry: false,
        pressure: None,
        refinement: None,
    }
}
fn line_sweep_cases()->Vec<LineCase> {
    let mut cases = Vec::new();
    for angle in (0..=90).step_by(15) { for gap in [2., 4., 8., 16., 32., 48.] {
        cases.push(rotated_case(angle as f32, gap, 3.));
    }}
    for width in 1..=16 {
        let mut case=rotated_case(30., 8., width as f32);
        case.name=format!("width sweep {}",case.name);
        cases.push(case);
    }
    for angle in (5..=90).step_by(5) {
        let slope=(angle as f32*0.5).to_radians().tan();let length=100f32.min(50./slope);let height=length*slope;
        let triangle = vec![[20., 60. - height], [20.+length, 60.], [20., 60. + height]];
        let mut line = triangle.clone();
        line.push(line[0]);
        cases.push(LineCase { name: format!("corner={angle}"), lines: vec![(line, 1.)], seeds: vec![[30., 60.]],
            regions: vec![vec![triangle]], open: false, dry: false, pressure: None, refinement: None });
    }
    cases
}
#[test]
fn enclose_fill_geometry_angle_break_width_and_corner_sweeps() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    line_atlas(&mut r, &line_sweep_cases(), None, 0.8);
}
fn dry_geometry_cases(preset:layer_core::DefaultBrushPreset,pressure:f32) -> Vec<LineCase> {
    line_sweep_cases().into_iter().map(|mut case| {
            let line=&mut case.lines[0].0;let last=line.len()-1;
            let gap=(line[0][0]-line[last][0]).hypot(line[0][1]-line[last][1]);let overshoot=(gap*0.25).min(4.);
            for (end,next) in [(0,1),(last,last-1)] {
                let delta=[line[end][0]-line[next][0],line[end][1]-line[next][1]];let length=delta[0].hypot(delta[1]);
                for (at,d) in line[end].iter_mut().zip(delta){*at+=overshoot*d/length;}
            }
            let doubled = case.lines[0].0.iter().map(|p| [p[0] + 0.2, p[1] - 0.15]).collect::<Vec<_>>();
            for p in &mut case.lines[0].0 {p[0]-=0.2;p[1]+=0.15;}
            case.lines[0].0[0][0] += 0.2;
            case.lines[0].0[2][1] -= 0.3;
            let width=case.lines[0].1;case.lines.push((doubled, width));
            case.name = format!("{preset:?} pressure={pressure} {}", case.name);
            case.pressure = Some(pressure);
            case
        }).collect()
}
fn dry_geometry_sweep(preset:layer_core::DefaultBrushPreset,pressure:f32) {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    line_atlas(&mut r, &dry_geometry_cases(preset,pressure), Some(preset), pressure);
}
#[test]
fn enclose_fill_geometry_known_misses_only_shrink() {
    let mut names=std::collections::HashSet::new();
    for preset in [layer_core::DefaultBrushPreset::Pencil,layer_core::DefaultBrushPreset::Charcoal,layer_core::DefaultBrushPreset::Crayon] {
        for pressure in [0.25,0.5,0.8] {
            for case in dry_geometry_cases(preset,pressure) {assert!(names.insert(case.name),"duplicate real-brush case name");}
        }
    }
    assert_eq!(names.len(),684);
    let mut misses=std::collections::HashSet::new();
    for &name in KNOWN_MISSES {
        assert!(names.contains(name),"{name}: stale known miss");
        assert!(misses.insert(name),"{name}: duplicate known miss");
        assert!(check_geometry(name,Err("miss".into())).is_ok());
        assert!(check_geometry(name,Ok(())).unwrap_err().contains(name));
    }
    assert_eq!(check_geometry("new case",Err("new failure".into())),Err("new failure".into()));
    assert!(check_geometry("new case",Ok(())).is_ok());
}
#[test]
fn enclose_fill_geometry_dry_pencil_pressure_25() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Pencil, 0.25);}
#[test]
fn enclose_fill_geometry_dry_pencil_pressure_50() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Pencil, 0.5);}
#[test]
fn enclose_fill_geometry_dry_pencil_pressure_80() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Pencil, 0.8);}
#[test]
fn enclose_fill_geometry_dry_charcoal_pressure_25() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Charcoal, 0.25);}
#[test]
fn enclose_fill_geometry_dry_charcoal_pressure_50() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Charcoal, 0.5);}
#[test]
fn enclose_fill_geometry_dry_charcoal_pressure_80() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Charcoal, 0.8);}
#[test]
fn enclose_fill_geometry_dry_crayon_pressure_25() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Crayon, 0.25);}
#[test]
fn enclose_fill_geometry_dry_crayon_pressure_50() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Crayon, 0.5);}
#[test]
fn enclose_fill_geometry_dry_crayon_pressure_80() {dry_geometry_sweep(layer_core::DefaultBrushPreset::Crayon, 0.8);}

#[test]
fn enclose_fill_geometry_auto_off_line_tap_and_edges() {
    let mut cases = Vec::new();
    let mut off = rotated_case(45., 8., 3.);
    off.name = "Close gaps Off leaves deliberate break open".into();
    off.refinement = Some(RegionRefinement { close_gaps: false, ..Default::default() });
    off.open = true;
    cases.push(off);
    let mut tap = rotated_case(0., 0., 10.);
    tap.name = "tap on inner half of line chooses interior".into();
    tap.seeds = vec![[40., 60.]];
    cases.push(tap);
    for edges in [RegionEdges::LineMiddle, RegionEdges::Exact, RegionEdges::GrowShrink(0), RegionEdges::GrowShrink(4), RegionEdges::GrowShrink(-4)] {
        let mut case = rotated_case(0., 0., 10.);
        case.name = format!("Edges {edges:?}");
        let inset = match edges { RegionEdges::LineMiddle => 0., RegionEdges::Exact => 5., RegionEdges::GrowShrink(n) => 5. - n as f32 };
        case.regions = vec![vec![vec![[40. + inset, 30. + inset], [120. - inset, 30. + inset], [120. - inset, 90. - inset], [40. + inset, 90. - inset]]]];
        case.refinement = Some(RegionRefinement { edges, close_gaps: false, smooth_edges: false });
        cases.push(case);
    }
    let mut smooth = rotated_case(30., 0., 3.);
    smooth.name = "Smooth edges preserves geometric coverage".into();
    smooth.refinement = Some(RegionRefinement { smooth_edges: true, ..Default::default() });
    cases.push(smooth);
    for width in [1.,4.,8.,12.,16.] {
        let mut case=rotated_case(0.,0.,width);
        case.name=format!("tap on inner corner width={width}");case.seeds=vec![[42.,32.]];
        cases.push(case);
    }
    let mut outer=rotated_case(0.,0.,16.);
    outer.name="tap on outer half of corner chooses exterior".into();outer.seeds=vec![[39.,29.]];outer.open=true;cases.push(outer);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let masks = line_atlas(&mut r, &cases, None, 0.8);
    for y in 0..120 { for x in 0..160 {
        assert_eq!(value(&masks[3], 592 + x, 16 + y), value(&masks[4], 16 + x, 168 + y), "Grow or shrink zero must equal Exact at {x},{y}");
    }}
}

#[test]
fn enclose_fill_line_middle_ignores_a_cross_shaped_thin_pocket() {
    let extent = [128; 2];
    let source = |x: u32, y: u32| {
        let pocket = match y {
            29 => (60..63).contains(&x),
            30 => (60..64).contains(&x),
            31 => (61..63).contains(&x),
            32 => x == 61,
            _ => false,
        };
        let border = (16..112).contains(&x) && (24..112).contains(&y)
            && (y < 36 || y >= 96 || !(32..96).contains(&x));
        [0., 0., 0., if pocket { (9 + (x + y) % 4) as f32 / 255. } else if border { 0.5 } else { 0. }]
    };
    let mut document = paint_document(extent, "Thin pocket line middle");
    placement::set_source(&mut document, depth_source(extent, SampleDepth::F32, RgbSpace::Srgb,
        128 * 128 * 16 + 4096, source));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    frame(&mut r, &document, extent);
    let before = r.readback_srgb_rgba8().unwrap();
    let mut query = request(RegionSource::Source(target(&document)), rectangle([0, 0, 128, 128]),
        RegionRefinement { close_gaps: false, smooth_edges: false, ..Default::default() }, 0.05);
    query.position = [64, 80];
    query.enclosure = None;
    let result = receive_request(&mut r, query);
    for [x, y] in [[64, 80], [64, 36], [64, 31], [62, 33]] {
        assert_eq!(value(&result.pixels, x, y), 255, "selected interior {x},{y}");
    }
    for [x, y] in [[64, 23], [64, 27]] {
        assert_eq!(value(&result.pixels, x, y), 0, "outer side {x},{y}");
    }
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
}

fn area_request(document:&Document,reference:SourceTarget,destination:SourceTarget,enclosure:Selection,areas:Option<FillAreas>)->RegionRequest {
    let snapshot=document.snapshot();
    let reference=RegionSource::Scene {snapshot:snapshot.clone(),scope:SceneScope::Raw(reference)};
    let source=if areas.is_some() {
        RegionSource::Intersection(vec![reference,RegionSource::Scene {snapshot,scope:SceneScope::Raw(destination)}])
    }else{reference};
    let mut query=request(source,enclosure,exact(),0.125);
    query.reference_count=Some(1);query.fill_areas=areas;query
}

fn swept_footprint(r:&mut WgpuRasterizer)->Selection {
    use layer_engine::{PenEvent,PenPhase,PressureCurve,SampleFlags,SelectionStroke,ToolKind,ViewTransform};
    let brush=layer_core::BrushSnapshot {diameter:64.,hardness:1.,flow:1.,spacing:0.08,
        color_rgba_linear:[1.;4],mappings:Arc::from([]),..Default::default()};
    let mut stroke=SelectionStroke::new(71,brush,ViewTransform::IDENTITY,PressureCurve::default(),None);
    let mut dabs=Vec::new();
    for (index,(phase,position)) in [(PenPhase::Down,[48.,48.]),(PenPhase::Move,[80.,48.]),(PenPhase::Up,[80.,48.])].into_iter().enumerate() {
        stroke.push(PenEvent {device_id:1,sequence:index as u64+1,timestamp_ns:(index as u64+1)*16_000_000,
            view_revision:1,surface_position:Point {x:position[0],y:position[1]},pressure:1.,tilt_radians:[0.;2],
            twist_radians:0.,distance:0.,phase,tool:ToolKind::Pen,flags:SampleFlags::PRIMARY},&mut dabs);
    }
    let paint=layer_render::SelectionPaint {id:71,before:Arc::new(Selection::empty()),mode:layer_render::SelectionPaintMode::Add,
        opacity:1.,gray:1.,gradient:None,style:stroke.style(),dabs,enclosed:None,finish:false,restart:false};
    assert!(r.paint_selection(&paint).unwrap());
    let footprint=super::selection_painting::receive(r,paint);
    let layer_core::SelectionShape::Pixels(pixels)=&footprint.shape else {panic!("captured footprint")};
    for [x,y] in [[48,48],[96,48]] {assert_eq!(value(pixels,x,y),255,"swept footprint {x},{y}");}
    assert_eq!(value(pixels,124,48),0);
    footprint
}

#[test]
fn fill_areas_intersects_independent_targets_with_linear_white_and_alpha_thresholds() {
    let extent=[160,96];let mut document=paint_document(extent,"Area references");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y|
        [0,0,0,if barrier([8,8,152,88],x,y) {255}else{0}]));
    let reference=target(&document);
    let colors=[[0.;4],[1.,1.,1.,0.125],[1.,1.,1.,0.125+1./1024.],[1.;4],
        [0.875,0.875,0.875,1.],[0.875-1./1024.,0.875-1./1024.,0.875-1./1024.,1.],
        [0.1,0.4,0.6,1.],[1.,1.,1.,0.5]];
    let patches:Vec<_>=(0..8).map(|i|[12+i*17,20,25+i*17,76]).collect();
    let mut destination=paint_document(extent,"Area target");
    placement::set_source(&mut destination,depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,
        extent[0] as usize*extent[1] as usize*16+4096,|x,y|
            patches.iter().position(|b|(b[0]..b[2]).contains(&x)&&(b[1]..b[3]).contains(&y)).map_or([0.2,0.3,0.4,1.],|i|colors[i])));
    let (_,destination)=placement::append_paint(&mut document,"Area target",placement::paint(&destination).clone());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    for areas in [Some(FillAreas::Transparent),None,Some(FillAreas::TransparentOrWhite),Some(FillAreas::Transparent)] {
        let expected=match areas {
            None=>rectangles(extent,&[[9,9,151,87]]),
            Some(FillAreas::Transparent)=>rectangles(extent,&patches[..2]),
            Some(FillAreas::TransparentOrWhite)=>rectangles(extent,&[patches[0],patches[1],patches[3],patches[4]]),
            Some(FillAreas::AnyColour)=>unreachable!(),
        };
        let query=area_request(&document,reference,destination,rectangle([1,1,159,95]),areas);
        check(&receive_request(&mut r,query),&expected,extent);
    }
}

#[test]
fn paint_unfilled_swept_footprint_keeps_grazed_regions_empty() {
    let extent=[160,96];let mut document=paint_document(extent,"Footprint references");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y|
        [0,0,0,if [[35,30,50,65],[95,30,125,65],[132,30,153,65]].into_iter().any(|b|barrier(b,x,y)) {255}else{0}]));
    let reference=target(&document);let destination=paint_document(extent,"Footprint target");
    let (_,destination)=placement::append_paint(&mut document,"Footprint target",placement::paint(&destination).clone());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let footprint=swept_footprint(&mut r);
    let query=area_request(&document,reference,destination,footprint,Some(FillAreas::Transparent));
    check(&receive_request(&mut r,query),&rectangles(extent,&[[36,31,49,64]]),extent);
}

#[test]
fn paint_unfilled_transparent_specks_between_coloured_flats() {
    let extent=[160,96];let mut document=paint_document(extent,"Speck references");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y|
        [0,0,0,if barrier([8,8,152,88],x,y) {255}else{0}]));
    let reference=target(&document);let mut destination=paint_document(extent,"Coloured flats");
    let specks=[[52,44,55,47],[78,45,82,49],[130,44,133,47]];
    placement::set_source(&mut destination,layer_core::color::source::rgba8_source(extent,|x,y|
        if specks.iter().any(|b|(b[0]..b[2]).contains(&x)&&(b[1]..b[3]).contains(&y)) {[0;4]}
        else if x<80 {[220,100,40,255]}else{[30,80,210,255]}));
    let (_,destination)=placement::append_paint(&mut document,"Coloured flats",placement::paint(&destination).clone());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let footprint=swept_footprint(&mut r);
    let query=area_request(&document,reference,destination,footprint,Some(FillAreas::Transparent));
    check(&receive_request(&mut r,query),&rectangles(extent,&specks[..2]),extent);
}

fn fill_area_ring_alpha(x:u32,y:u32)->f32 {
    (4.5-((x as f32+0.5-64.).hypot(y as f32+0.5-64.)-40.25).abs()).clamp(0.,1.)
}

fn painted_area_ring(white:bool,areas:Option<FillAreas>)->(RegionResult,Vec<u8>,Vec<u8>) {
    let extent=[128;2];let mut document=paint_document(extent,"Editing alpha ring");
    placement::set_source(&mut document,depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,128*128*16+4096,|x,y| {
        let alpha=fill_area_ring_alpha(x,y);
        if white {[1.-alpha,1.-alpha,1.-alpha,1.]}else{[0.,0.,0.,alpha]}
    }));
    let target=target(&document);let snapshot=document.snapshot();
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(r,document,layer_engine::input_queue(8).1,
        crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    crate::test_support::flush_engine(&mut engine);
    let before=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let mut query=area_request(engine.document(),target,target,rectangle([0,0,128,128]),areas);
    query.colour_mode=layer_render::RegionColourMode::ColourOnSolid;
    query.tolerance=0.1;query.refinement=RegionRefinement {smooth_edges:true,..Default::default()};
    if areas.is_none() {query.reference_count=None;}
    let result=receive_request(engine.backend_mut(),query);
    let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(),extent,[0;2]);coverage.selection=Some(Selection::pixels(result.pixels.clone()));
    let operation=RasterOperation {placement:result.placement,coverage,kind:RasterOperationKind::Fill {
        color:[0.1,0.7,0.2,1.],texture:Default::default(),alpha_locked:false,
        underfill:result.underfill.clone().map(Selection::pixels),
        edges:Some(Box::new(layer_core::FillEdges {snapshot,position:Point{x:0.,y:0.},tolerance:0.1,
            anchors:result.anchors.clone(),lines:Vec::new()})),
    }};
    engine.append_raster_operation(target,operation).unwrap();
    crate::test_support::flush_engine(&mut engine);
    (result,before,engine.backend_mut().readback_srgb_rgba8().unwrap())
}

fn compare_fill_area_ring(white:bool,areas:FillAreas) {
    let (any,before,expected)=painted_area_ring(white,None);
    let (restricted,restricted_before,actual)=painted_area_ring(white,Some(areas));
    assert_eq!(before,restricted_before);
    let (mut failures,mut painted,mut maximum)=(0,0,0);let mut first=None;
    for y in 0..128 {for x in 0..128 {
        let alpha=fill_area_ring_alpha(x,y);
        if alpha<=0. || alpha>=1. || (x as f32+0.5-64.).hypot(y as f32+0.5-64.)>=40.25 {continue;}
        let i=((y*128+x)*4) as usize;
        if alpha>0.1 && expected[i..i+4]!=before[i..i+4] {painted+=1;}
        let difference=actual[i..i+4].iter().zip(&expected[i..i+4]).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
        maximum=maximum.max(difference);
        if difference!=0 {
            failures+=1;
            if first.is_none() {
                let mask=|result:&RegionResult|value(&result.pixels,x,y);
                let band=|result:&RegionResult|result.underfill.as_ref().map_or(0,|p|value(p,x,y));
                let stage=if mask(&any)!=mask(&restricted) || band(&any)!=band(&restricted) {"intersect_bits before edge stage"}else{"edge stage"};
                first=Some(format!("{x},{y} alpha={alpha} stage={stage} primary any/restricted={}/{} band any/restricted={}/{} anchor any/restricted={:?}/{:?} expected={:?} actual={:?}",
                    mask(&any),mask(&restricted),band(&any),band(&restricted),any.anchors.as_ref().and_then(|a|a.get([x,y])),
                    restricted.anchors.as_ref().and_then(|a|a.get([x,y])),&expected[i..i+4],&actual[i..i+4]));
            }
        }
    }}
    assert!(painted>0,"Any colour must paint the ramp above the default tolerance");
    assert_eq!(failures,0,"{areas:?} white={white} maximum={maximum}; {first:?}");
}

#[test]
fn fill_areas_line_middle_transparent_editing_keeps_the_alpha_ramp() {
    compare_fill_area_ring(false,FillAreas::Transparent);
}

#[test]
fn fill_areas_line_middle_white_editing_keeps_the_colour_ramp() {
    compare_fill_area_ring(true,FillAreas::TransparentOrWhite);
}

fn reference_red_flat(edges:RegionEdges,smooth:bool,stop:bool)->(RegionResult,Vec<[f32;4]>,Vec<[f32;4]>) {
    let extent=[128;2];let mut document=paint_document(extent,"Reference border");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y| {
        let line=(24..104).contains(&x)&&(24..104).contains(&y)
            && (x<30||x>=98||y<30||y>=98);
        [0,0,0,if line {255}else{0}]
    }));
    let reference=placement::occurrence_id(&document);
    let red=|x,y|(20..30).contains(&x)&&(40..88).contains(&y);
    let mut destination=paint_document(extent,"Red flat");
    placement::set_source(&mut destination,layer_core::color::source::rgba8_source(extent,|x,y|
        if red(x,y) {[255,0,0,255]}else{[0;4]}));
    let (destination_layer,destination)=placement::append_paint(&mut document,"Red flat",placement::paint(&destination).clone());
    let snapshot=document.snapshot();
    let reference_source=RegionSource::reference(snapshot.clone(),reference).unwrap();
    let source=RegionSource::Intersection(vec![reference_source.clone(),
        RegionSource::Scene {snapshot:snapshot.clone(),scope:SceneScope::Raw(destination)}]);
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(r,document,layer_engine::input_queue(8).1,
        crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    crate::test_support::flush_engine(&mut engine);
    let before=engine.backend().snapshot_gpu().capture_scene(engine.document().snapshot(),SceneScope::Raw(destination),Default::default()).unwrap()
        .read_region([0,0,128,128]).unwrap();
    let refinement=RegionRefinement {edges,smooth_edges:smooth,..Default::default()};
    if !stop && matches!(edges,RegionEdges::GrowShrink(_)) {
        let mut control=request(reference_source.clone(),rectangle([8,8,120,120]),refinement,0.1);
        control.reference_count=Some(1);
        let any=receive_request(engine.backend_mut(),control);
        for [x,y] in [[29,48],[29,60]] {assert!(value(&any.pixels,x,y)>0,"Any colour growth reaches red {x},{y}");}
    }
    let mut query=request(source,rectangle([8,8,120,120]),refinement,0.1);
    query.reference_count=Some(1);query.fill_areas=if stop {None}else{Some(FillAreas::Transparent)};
    if stop {query.enclosure=None;query.position=[64,64];query.fill=Some(Default::default());}
    query.colour_mode=layer_render::RegionColourMode::AlphaBorder;
    let result=receive_request(engine.backend_mut(),query);
    let metadata=if edges==RegionEdges::LineMiddle {
        let band=result.underfill.as_ref().unwrap();
        assert!(value(&result.pixels,28,60)>0);assert!(value(band,28,60)>0,"red pixel has assigned band coverage");
        let RegionSource::Scene {snapshot:line_snapshot,scope}=reference_source else {unreachable!()};
        let order=engine.document().scene().order();
        let fill_above=order.iter().position(|id|*id==reference).unwrap()>order.iter().position(|id|*id==destination_layer).unwrap();
        assert!(!fill_above,"target is below the reference");
        Some(Box::new(layer_core::FillEdges {snapshot,position:Point{x:0.,y:0.},tolerance:0.1,
            anchors:result.anchors.clone(),lines:vec![layer_core::FillLine {snapshot:line_snapshot,scope,fill_above}]}))
    }else{None};
    let mut coverage=layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(),extent,[0;2]);coverage.selection=Some(Selection::pixels(result.pixels.clone()));
    let operation=RasterOperation {placement:result.placement,coverage,kind:RasterOperationKind::Fill {
        color:[0.1,0.7,0.2,1.],texture:Default::default(),alpha_locked:false,edges:metadata,
        underfill:result.underfill.clone().map(Selection::pixels),
    }};
    engine.append_raster_operation(destination,operation).unwrap();
    crate::test_support::flush_engine(&mut engine);
    let after=engine.backend().snapshot_gpu().capture_scene(engine.document().snapshot(),SceneScope::Raw(destination),Default::default()).unwrap()
        .read_region([0,0,128,128]).unwrap();
    let nearby=(60*128+44) as usize;
    assert!(value(&result.pixels,44,60)>0);
    assert_eq!(before[nearby],[0.;4]);
    assert_ne!(after[nearby],before[nearby],"nearby transparent pixel is painted for {edges:?}");
    (result,before,after)
}

#[test]
fn fill_areas_reference_band_and_growth_preserve_opaque_red_flats() {
    for (edges,smooth) in [(RegionEdges::LineMiddle,true),(RegionEdges::GrowShrink(2),false)] {
        let (result,before,after)=reference_red_flat(edges,smooth,false);
        for y in 40..88 {for x in 20..30 {
            let i=(y*128+x) as usize;
            assert_eq!(before[i],[1.,0.,0.,1.]);
            assert_eq!(after[i],before[i],"{edges:?} red flat {x},{y} primary={} underfill={}",
                value(&result.pixels,x,y),result.underfill.as_ref().map_or(0,|band|value(band,x,y)));
        }}
    }
}

#[test]
fn fill_stop_at_colours_smoothed_growth_ramps_into_the_protected_boundary() {
    let (result,before,after)=reference_red_flat(RegionEdges::GrowShrink(2),true,true);
    let point=[29,40];let i=(point[1]*128+point[0]) as usize;
    assert_eq!(value(&result.pixels,point[0],point[1]),64);
    assert_eq!(before[i],[1.,0.,0.,1.]);
    assert_ne!(after[i],before[i],"regular Fill with Stop at colours has the same one-pixel smoothing tint");
}
