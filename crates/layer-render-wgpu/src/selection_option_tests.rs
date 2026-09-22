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
    assert!(
        r.request_region(RegionRequest {
            request_id: 42,
            source: RegionSource::Selection(Arc::new(incoming)),
            contiguous: false,
            selection: Some(SelectionRefinement {
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
    let mut r = WgpuRasterizer::new_headless().unwrap();
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
fn byte_selection_restores_and_transforms_in_brush_fill_and_mask() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "selection")],
        &[],
        &[],
        true,
    );
    let pixels = receive(&mut r, rect(38., 85.), None, SelectionMode::New, 10., true);
    // A distinct core allocation exercises upload after renderer/history restore.
    let restored =
        SelectionPixels::bytes(pixels.extent(), pixels.bounds(), pixels.words().to_vec()).unwrap();
    let base = Selection::pixels(Arc::new(restored));
    let white = Dab {
        radii: [200.; 2],
        ..dab([1.; 4])
    };
    for affine in [
        Affine::IDENTITY,
        Affine::translation(Point { x: 7., y: -3. }),
        Affine::around(
            Point { x: 64., y: 64. },
            [0.8, 1.1],
            0.2,
            Point { x: 0., y: 0. },
        ),
    ] {
        let selection = base.transformed(affine).unwrap();
        let mut mask = LayerMask::reveal_all(LayerId(2), Point::default());
        mask.default_coverage = 0.;
        mask.initial = Some(selection.clone());
        let mut layer = Layer::paint(LayerId(1), "byte coverage");
        layer.pending_operations = vec![LayerOperation {
            placement: Affine::IDENTITY,
            coverage: mask.clone(),
            kind: LayerOperationKind::Fill {
                color: [1.; 4],
                alpha_locked: false,
            },
        }];
        submit(
            &mut r,
            &[layer.clone()],
            &[],
            &[DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                ..batch(1)
            }],
            true,
        );
        let filled = r.readback_srgb_rgba8().unwrap();
        if affine == Affine::IDENTITY {
            for y in 0..128 {
                for x in 0..128 {
                    assert!(
                        filled[((y * 128 + x) * 4 + 3) as usize].abs_diff(coverage(&pixels, x, y))
                            <= 1,
                        "{x},{y}: filled={} mask={}",
                        filled[((y * 128 + x) * 4 + 3) as usize],
                        coverage(&pixels, x, y)
                    );
                }
            }
        }
        layer.pending_operations.clear();
        let selected = DabBatch {
            style: DabStyle {
                selection: Some(Arc::new(selection)),
                ..batch(1).style
            },
            ..batch(1)
        };
        submit(&mut r, &[layer.clone()], &[white], &[selected], true);
        assert!(
            r.readback_srgb_rgba8()
                .unwrap()
                .iter()
                .zip(&filled)
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
        layer.mask = Some(mask);
        submit(&mut r, &[layer], &[white], &[batch(1)], true);
        assert!(
            r.readback_srgb_rgba8()
                .unwrap()
                .iter()
                .zip(&filled)
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
    }
}
