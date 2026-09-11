//! Independent pixel geometry oracle and GPU operation/replay checks.
use super::*;
use layer_core::{Figure, FigurePaint, FigureShape};

fn figure(shape: FigureShape, paint: FigurePaint, width: f32) -> Figure {
    Figure {
        shape,
        paint,
        start: Point { x: 20., y: 32. },
        end: Point { x: 108., y: 96. },
        width,
        colors: [[1., 0., 0., 0.6], [0., 0., 1., 0.6]],
        alpha_locked: false,
        erase: false,
    }
}
fn operation(f: Figure, mask: LayerMask) -> LayerOperation {
    LayerOperation {
        after_stroke: 0,
        coverage: mask,
        kind: LayerOperationKind::Figure(f),
    }
}
fn run(r: &mut WgpuRasterizer, f: Figure) -> Vec<u8> {
    let mut layer = Layer::paint(LayerId(1), "figure");
    layer.operations.push(operation(
        f,
        LayerMask::reveal_all(LayerId(9), Point::default()),
    ));
    let op = DabBatch {
        kind: DabBatchKind::LayerOperation(0),
        dab_count: 0,
        ..batch(1)
    };
    submit(r, &[layer], &[], &[op], true);
    r.readback_srgb_rgba8().unwrap()
}
// No copy of the shader solver: dense parametric segments approximate the
// true ellipse to <0.0002px in these fixtures. Geometry only, test-only.
fn distance(f: &Figure, p: [f64; 2], ellipse: &[[f64; 2]]) -> f64 {
    let a = [f.start.x as f64, f.start.y as f64];
    let b = [f.end.x as f64, f.end.y as f64];
    let segment = |a: [f64; 2], b: [f64; 2]| {
        let d = [b[0] - a[0], b[1] - a[1]];
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]))
            .clamp(0., 1.);
        (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
    };
    if f.shape == FigureShape::Line {
        return segment(a, b) - f.width as f64 / 2.;
    }
    let r = [(b[0] - a[0]).abs() / 2., (b[1] - a[1]).abs() / 2.];
    let q = [
        (p[0] - (a[0] + b[0]) / 2.).abs(),
        (p[1] - (a[1] + b[1]) / 2.).abs(),
    ];
    if f.shape == FigureShape::Rectangle {
        if q[0] <= r[0] && q[1] <= r[1] {
            return -(r[0] - q[0]).min(r[1] - q[1]);
        }
        return (q[0] - r[0]).max(0.).hypot((q[1] - r[1]).max(0.));
    }
    let d = ellipse
        .windows(2)
        .map(|s| segment(s[0], s[1]))
        .fold(f64::INFINITY, f64::min);
    d * if (q[0] / r[0]).powi(2) + (q[1] / r[1]).powi(2) < 1. {
        -1.
    } else {
        1.
    }
}
fn oracle(f: &Figure) -> impl Fn(u32, u32) -> [u8; 4] + '_ {
    let ellipse: Vec<_> = (0..=2048)
        .map(|i| {
            let t = i as f64 / 2048. * std::f64::consts::TAU;
            [
                (f.start.x + f.end.x) as f64 / 2. + (f.end.x - f.start.x) as f64 / 2. * t.cos(),
                (f.start.y + f.end.y) as f64 / 2. + (f.end.y - f.start.y) as f64 / 2. * t.sin(),
            ]
        })
        .collect();
    move |x, y| {
        let d = distance(f, [x as f64 + 0.5, y as f64 + 0.5], &ellipse);
        let h = f.width as f64 / 2.;
        let (fg, bg) = if f.shape == FigureShape::Line {
            ((0.5 - d).clamp(0., 1_f64.min(f.width as f64)), 0.)
        } else if f.paint == FigurePaint::Fill {
            ((0.5 - d).clamp(0., 1.), 0.)
        } else {
            let outer = (0.5 + h - d).clamp(0., 1.);
            let inner = (0.5 - h - d).clamp(0., 1.);
            (
                outer - inner,
                if f.paint == FigurePaint::Both {
                    inner
                } else {
                    0.
                },
            )
        };
        let alpha = fg * f.colors[0][3] as f64 + bg * f.colors[1][3] as f64;
        let mut result = [0; 4];
        if alpha > 0.000001 {
            for (i, c) in result[..3].iter_mut().enumerate() {
                let linear = (fg * f.colors[0][i] as f64 * f.colors[0][3] as f64
                    + bg * f.colors[1][i] as f64 * f.colors[1][3] as f64)
                    / alpha;
                let s = if linear <= 0.0031308 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1. / 2.4) - 0.055
                };
                *c = (s * 255.).round() as u8;
            }
        }
        result[3] = (alpha * 255.).round() as u8;
        result
    }
}

#[test]
fn figure_gpu_pixels_match_independent_geometry() {
    let mut r = WgpuRasterizer::new().unwrap();
    for shape in [
        FigureShape::Line,
        FigureShape::Rectangle,
        FigureShape::Ellipse,
    ] {
        for paint in [FigurePaint::Outline, FigurePaint::Fill, FigurePaint::Both] {
            if shape == FigureShape::Line && paint != FigurePaint::Outline {
                continue;
            }
            for width in [0.25, 1., 7., 60.] {
                let f = figure(shape, paint, width);
                let actual = run(&mut r, f.clone());
                let expected = oracle(&f);
                for y in (0..128).step_by(3) {
                    for x in (0..128).step_by(3) {
                        let e = expected(x, y);
                        let a = &actual[((y * 128 + x) * 4) as usize..][..4];
                        assert!(
                            a[3].abs_diff(e[3]) <= 2,
                            "{shape:?} {paint:?} width {width} at {x},{y}: {a:?} vs {e:?}"
                        );
                        // Canvas tiles store quantized premultiplied linear RGBA;
                        // compare in that space, not amplified near-zero-alpha sRGB.
                        let linear = |c: u8| {
                            let c = c as f32 / 255.;
                            if c <= 0.04045 {
                                c / 12.92
                            } else {
                                ((c + 0.055) / 1.055).powf(2.4)
                            }
                        };
                        for i in 0..3 {
                            let error =
                                (linear(a[i]) * a[3] as f32 - linear(e[i]) * e[3] as f32).abs();
                            assert!(
                                error <= 2.,
                                "{shape:?} {paint:?} width {width} at {x},{y}: {a:?} vs {e:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    for size in [[100., 3.], [3., 100.], [100., 100.], [2., 2.]] {
        let mut f = figure(FigureShape::Ellipse, FigurePaint::Outline, 1.);
        f.start = Point { x: 10.5, y: 10.5 };
        f.end = Point {
            x: 10.5 + size[0],
            y: 10.5 + size[1],
        };
        let actual = run(&mut r, f.clone());
        let expected = oracle(&f);
        for y in (0..128).step_by(2) {
            for x in (0..128).step_by(2) {
                let e = expected(x, y);
                let a = &actual[((y * 128 + x) * 4) as usize..][..4];
                assert!(
                    a[3].abs_diff(e[3]) <= 3,
                    "ellipse {size:?} at {x},{y}: {a:?} vs {e:?}"
                );
            }
        }
    }
}

#[test]
fn figure_uses_existing_mask_clipping_and_alpha_lock_and_can_erase() {
    let mut r = WgpuRasterizer::new().unwrap();
    for erase in [false, true] {
        for lock in [false, true] {
            let mut f = figure(FigureShape::Rectangle, FigurePaint::Both, 8.);
            f.alpha_locked = lock;
            f.erase = erase;
            let mut layer = Layer::paint(LayerId(1), "figure");
            layer.properties.clipped = true;
            layer.mask = Some(left_mask(10));
            let mut op = operation(f, left_mask(9));
            op.after_stroke = 1;
            layer.operations.push(op);
            let command = DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                ..batch(1)
            };
            submit(
                &mut r,
                &[layer, Layer::paint(LayerId(2), "base")],
                &[dab([0., 1., 0., 0.5]), dab([0., 0., 0., 0.5])],
                &[
                    batch(1),
                    DabBatch {
                        first_dab: 1,
                        ..batch(2)
                    },
                    command,
                ],
                true,
            );
            let inside = pixel(&mut r, 40, 64);
            let outside = pixel(&mut r, 90, 64);
            assert!(
                inside[3].abs_diff(128) <= 3,
                "clip retains base alpha, erase {erase}, lock {lock}: {inside:?}"
            );
            assert!(
                outside[3].abs_diff(128) <= 2,
                "mask hides right half: {outside:?}"
            );
            if !erase {
                assert!(inside[2] > 100, "blue interior: {inside:?}");
            }
            let green = if erase && !lock {
                0.2
            } else if erase {
                0.5
            } else {
                0.2
            };
            let encoded = (1.055 * f32::powf(green, 1. / 2.4) - 0.055) * 255.;
            assert!(
                inside[1].abs_diff(encoded.round() as u8) <= 3,
                "green erase {erase}, lock {lock}: {inside:?}"
            );
        }
    }
}

#[test]
fn figures_are_incremental_sparse_and_match_replay_at_tile_boundaries() {
    let mut r = WgpuRasterizer::new().unwrap();
    let extent = [512, 384];
    let v = ViewState {
        width_px: 512,
        height_px: 384,
        ..view()
    };
    let mut layer = Layer::paint(LayerId(1), "figures");
    layer.properties.offset = Point { x: 11., y: -9. };
    let mut f = figure(FigureShape::Ellipse, FigurePaint::Both, 5.);
    f.start = Point { x: 230., y: 220. };
    f.end = Point { x: 290., y: 280. };
    let mask = LayerMask::reveal_all(LayerId(9), Point::default());
    layer.operations.push(operation(f, mask));
    let mut f = figure(FigureShape::Rectangle, FigurePaint::Fill, 1.);
    f.start = Point { x: 180., y: 240. };
    f.end = Point { x: 330., y: 300. };
    let mut mask = LayerMask::reveal_all(LayerId(10), Point { x: 5., y: -7. });
    mask.default_coverage = 0.;
    mask.inverted = true;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 245., y: 250. },
            Point { x: 280., y: 250. },
            Point { x: 280., y: 340. },
            Point { x: 245., y: 340. },
        ])
        .unwrap(),
    );
    layer.operations.push(operation(f, mask));
    let ops: Vec<_> = (0..2)
        .map(|i| DabBatch {
            kind: DabBatchKind::LayerOperation(i),
            dab_count: 0,
            damage: layer.operations[i as usize].bounds(extent),
            ..batch(1)
        })
        .collect();
    let render = |r: &mut WgpuRasterizer, layer: &Layer, ops: &[DabBatch], reset| {
        r.submit(FramePacket {
            view: v,
            document_extent: extent,
            layers: std::slice::from_ref(layer),
            dabs: &[],
            dab_batches: ops,
            reset_layers: reset,
            time_seconds: 0.,
            composite_all: false,
        })
        .unwrap();
    };
    let mut first = layer.clone();
    first.operations.truncate(1);
    render(&mut r, &first, &ops[..1], true);
    assert_eq!(
        r.paint_layers[0].pages.len(),
        4,
        "only four touched tiles allocated"
    );
    render(&mut r, &layer, &ops[1..], false);
    let incremental = r.readback_srgb_rgba8().unwrap();
    for (x, y, covered) in [
        (252, 251, true),
        (270, 251, true),
        (305, 255, true),
        (300, 270, true),
        (268, 285, false),
        (30, 30, false),
    ] {
        let a = incremental[((y * 512 + x) * 4 + 3) as usize];
        assert_eq!(a > 0, covered, "boundary/offset/mask {x},{y}: {a}");
    }
    render(&mut r, &layer, &ops, true);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), incremental);
    render(&mut r, &layer, &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        incremental,
        "idle never reapplies pigment"
    );
    let mut small = layer.clone();
    small.operations.truncate(1);
    if let LayerOperationKind::Figure(f) = &mut small.operations[0].kind {
        f.start = Point { x: 32., y: 32. };
        f.end = Point { x: 96., y: 96. };
    }
    let op = DabBatch {
        damage: small.operations[0].bounds(extent),
        ..ops[0].clone()
    };
    render(&mut r, &small, &[op], true);
    assert_eq!(
        r.paint_layers[0].pages.len(),
        1,
        "small figure never allocates untouched tiles"
    );
}

#[test]
#[ignore = "hardware GPU figure latency; release, serial"]
fn figure_latency() {
    let mut r = WgpuRasterizer::new().unwrap();
    r.set_telemetry_enabled(true);
    let extent = [2048, 1536];
    let v = ViewState {
        width_px: 2048,
        height_px: 1536,
        ..view()
    };
    r.submit(FramePacket {
        view: v,
        document_extent: extent,
        layers: &[Layer::paint(LayerId(1), "benchmark")],
        dabs: &[],
        dab_batches: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: false,
    })
    .unwrap();
    r.wait_idle().unwrap();
    for (name, shape, paint, size) in [
        (
            "small rectangle",
            FigureShape::Rectangle,
            FigurePaint::Fill,
            [96., 64.],
        ),
        (
            "large rectangle",
            FigureShape::Rectangle,
            FigurePaint::Both,
            [1920., 1400.],
        ),
        (
            "large ellipse",
            FigureShape::Ellipse,
            FigurePaint::Both,
            [1920., 1400.],
        ),
        (
            "thin ellipse",
            FigureShape::Ellipse,
            FigurePaint::Outline,
            [1920., 10.],
        ),
        (
            "large line",
            FigureShape::Line,
            FigurePaint::Outline,
            [1920., 1400.],
        ),
    ] {
        let mut f = figure(shape, paint, 24.);
        f.start = Point { x: 32., y: 32. };
        f.end = Point {
            x: 32. + size[0],
            y: 32. + size[1],
        };
        let mut layer = Layer::paint(LayerId(1), "benchmark");
        let op = operation(f, LayerMask::reveal_all(LayerId(9), Point::default()));
        let mut completed = Vec::new();
        for i in 0..160 {
            let mut op = op.clone();
            op.coverage.id = LayerId(9 + i as u64);
            layer.operations.push(op);
            let batch = DabBatch {
                kind: DabBatchKind::LayerOperation(i),
                dab_count: 0,
                damage: layer.operations.last().unwrap().bounds(extent),
                ..batch(1)
            };
            let start = std::time::Instant::now();
            r.submit(FramePacket {
                view: v,
                document_extent: extent,
                layers: std::slice::from_ref(&layer),
                dabs: &[],
                dab_batches: &[batch],
                reset_layers: i == 0,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
            r.wait_idle().unwrap();
            if i == 0 {
                eprintln!(
                    "{name} first commit completed {:.3}ms",
                    start.elapsed().as_secs_f32() * 1000.
                );
            }
            if i >= 40 {
                completed.push(start.elapsed().as_secs_f32() * 1000.);
            }
        }
        let summary = |mut v: Vec<f32>| {
            assert_eq!(v.len(), 120);
            v.sort_by(f32::total_cmp);
            [v[59], v[113], v[118]]
        };
        let stats = r.telemetry();
        assert!(stats.gpu_timestamps);
        eprintln!(
            "{name} median/p95/p99 ms CPU {:.3?} GPU {:.3?} completed {:.3?}",
            summary(stats.cpu.ordered()),
            summary(stats.gpu.ordered()),
            summary(completed)
        );
    }
}
