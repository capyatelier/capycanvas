use super::*;
use layer_core::{Affine, SelectionMode, SelectionPixels};
use layer_render::{RegionRequest, RegionSource, SelectionRefinement};
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
    assert!(
        r.request_region(RegionRequest {
            request_id: 42,
            source: RegionSource::Selection(Arc::new(incoming)),
            contiguous: false,
            selection: Some(SelectionRefinement {
                resize,
                mode,
                antialias,
                feather,
                previous: previous.map(Arc::new),
                source_to_document: Affine::IDENTITY
            }),
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
        })
        .unwrap()
    );
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        if let Some(reply) = r.take_region() {
            let reply = reply.unwrap();
            assert_eq!(reply.request_id, 42);
            return reply.pixels;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
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
