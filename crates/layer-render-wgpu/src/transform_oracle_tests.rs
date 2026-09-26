//! Independent f64 models of layer transforms on the native document path:
//! the renderer's paint pages after a preview are compared with the
//! premultiplied source, the selection and each filter evaluated on the CPU.
use super::*;
use layer_core::color::{ColorProfile, RgbSpace, SampleDepth, source::*};
use layer_core::{
    Affine, ImageTransform, Interpolation, Projective, SelectionPixels, TransformMap,
};
use std::sync::Arc;

const EXTENT: [u32; 2] = [300, 220];

/// Straight linear RGBA of the source fixture.
fn straight(x: u32, y: u32) -> [f32; 4] {
    let alpha = if (x / 3 + y / 5) % 7 == 0 {
        0.
    } else {
        0.35 + ((x * 7 + y * 5) % 13) as f32 / 20.
    };
    [
        (x % 17) as f32 / 16.,
        (y % 11) as f32 / 10.,
        ((x + y) % 5) as f32 / 4. * 1.5,
        alpha,
    ]
}

fn source_image() -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        64 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let row: Vec<u8> = (0..EXTENT[0])
            .flat_map(|x| straight(x, y))
            .flat_map(f32::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

/// Soft byte coverage over part of the layer.
fn coverage_byte(x: u32, y: u32) -> u32 {
    if !(40..230).contains(&x) || !(30..180).contains(&y) {
        0
    } else if x < 60 || y % 23 == 0 {
        (x * 11 + y * 3) % 256
    } else {
        255
    }
}

fn selection_pixels() -> Arc<SelectionPixels> {
    let words: Vec<u32> = (0..EXTENT[1])
        .flat_map(|y| {
            (0..EXTENT[0].div_ceil(4)).map(move |w| {
                (0..4).fold(0, |word, i| {
                    let x = w * 4 + i;
                    word | if x < EXTENT[0] {
                        coverage_byte(x, y) << (i * 8)
                    } else {
                        0
                    }
                })
            })
        })
        .collect();
    Arc::new(SelectionPixels::bytes(EXTENT, [40, 30, 230, 180], words).unwrap())
}

/// The CPU model: premultiplied source texels, coverage and each filter.
struct Oracle {
    /// 0: no selection, 1: selected, 2: inverted.
    mode: u32,
}
impl Oracle {
    fn coverage(&self, x: i32, y: i32) -> f64 {
        if self.mode == 0 {
            return 1.;
        }
        let inside = x >= 0 && y >= 0 && x < EXTENT[0] as i32 && y < EXTENT[1] as i32;
        let m = if inside {
            f64::from(coverage_byte(x as u32, y as u32)) / 255.
        } else {
            0.
        };
        if self.mode == 2 { 1. - m } else { m }
    }
    fn color(&self, x: i32, y: i32) -> [f64; 4] {
        if x < 0 || y < 0 || x >= EXTENT[0] as i32 || y >= EXTENT[1] as i32 {
            return [0.; 4];
        }
        let [r, g, b, a] = straight(x as u32, y as u32).map(f64::from);
        [r * a, g * a, b * a, a]
    }
    fn selected(&self, x: i32, y: i32) -> [f64; 4] {
        let m = self.coverage(x, y);
        self.color(x, y).map(|v| v * m)
    }
    fn bilinear(&self, [u, v]: [f64; 2]) -> [f64; 4] {
        let [left, top] = [(u - 0.5).floor() as i32, (v - 0.5).floor() as i32];
        let mut sum = [0.; 4];
        for y in top..=top + 1 {
            for x in left..=left + 1 {
                let w = (1. - (u - 0.5 - x as f64).abs()) * (1. - (v - 0.5 - y as f64).abs());
                let color = self.selected(x, y);
                for k in 0..4 {
                    sum[k] += color[k] * w;
                }
            }
        }
        sum
    }
    /// Catmull-Rom, clamped to the range of the four nearest texels, with
    /// straight color no brighter than theirs.
    fn bicubic(&self, [u, v]: [f64; 2]) -> [f64; 4] {
        let kernel = |d: f64| {
            let d = d.abs();
            if d < 1. {
                (1.5 * d - 2.5) * d * d + 1.
            } else if d < 2. {
                ((-0.5 * d + 2.5) * d - 4.) * d + 2.
            } else {
                0.
            }
        };
        let [left, top] = [(u - 0.5).floor() as i32, (v - 0.5).floor() as i32];
        let mut sum = [0.; 4];
        let mut low = [f64::INFINITY; 4];
        let mut high = [f64::NEG_INFINITY; 4];
        let mut brightest = [0f64; 3];
        for y in top - 1..=top + 2 {
            for x in left - 1..=left + 2 {
                let w = kernel(u - 0.5 - x as f64) * kernel(v - 0.5 - y as f64);
                let color = self.selected(x, y);
                for k in 0..4 {
                    sum[k] += color[k] * w;
                }
                if (left..=left + 1).contains(&x) && (top..=top + 1).contains(&y) {
                    for k in 0..4 {
                        low[k] = low[k].min(color[k]);
                        high[k] = high[k].max(color[k]);
                    }
                    for k in 0..3 {
                        if color[3] > 0. {
                            brightest[k] = brightest[k].max(color[k] / color[3]);
                        }
                    }
                }
            }
        }
        let mut value: [f64; 4] = std::array::from_fn(|k| sum[k].clamp(low[k], high[k]));
        value[3] = value[3].clamp(0., 1.);
        for k in 0..3 {
            value[k] = value[k].min(value[3] * brightest[k]);
        }
        value
    }
    /// Every value a destination pixel may take: its source footprint sets
    /// the tap grid, and a footprint on a grid boundary allows either count.
    fn moved(&self, h: [f64; 9], interpolation: Interpolation, x: u32, y: u32) -> Vec<[f64; 4]> {
        let center = [x as f64 + 0.5, y as f64 + 0.5];
        let Some(source) = preimage(h, center) else {
            return vec![[0.; 4]];
        };
        if interpolation == Interpolation::Nearest {
            let [xs, ys] = source.map(|v| [(v - 1e-3).floor() as i32, (v + 1e-3).floor() as i32]);
            return xs
                .into_iter()
                .flat_map(|x| ys.map(|y| self.selected(x, y)))
                .collect();
        }
        let step = 1e-4;
        let counts = [[step, 0.], [0., step]].map(|[dx, dy]| {
            let ends = [-1., 1.].map(|s| preimage(h, [center[0] + s * dx, center[1] + s * dy]));
            let [Some(a), Some(b)] = ends else {
                return vec![1];
            };
            let reach = (b[0] - a[0]).hypot(b[1] - a[1]) / (2. * step) + 0.5;
            let count = |r: f64| (r.floor() as u32).clamp(1, 4);
            let mut counts = vec![count(reach)];
            for other in [count(reach - 1e-3), count(reach + 1e-3)] {
                if !counts.contains(&other) {
                    counts.push(other);
                }
            }
            counts
        });
        let mut values = Vec::new();
        for &nx in &counts[0] {
            for &ny in &counts[1] {
                values.push(match (nx, ny, interpolation) {
                    (1, 1, Interpolation::Bicubic) => self.bicubic(source),
                    (1, 1, _) => self.bilinear(source),
                    _ => {
                        let mut sum = [0.; 4];
                        for j in 0..ny {
                            for i in 0..nx {
                                let tap = [
                                    center[0] + (i as f64 + 0.5) / nx as f64 - 0.5,
                                    center[1] + (j as f64 + 0.5) / ny as f64 - 0.5,
                                ];
                                let tap = preimage(h, tap).map_or([0.; 4], |p| self.bilinear(p));
                                for k in 0..4 {
                                    sum[k] += tap[k] / (nx * ny) as f64;
                                }
                            }
                        }
                        sum
                    }
                });
            }
        }
        values
    }
    fn composed(&self, x: u32, y: u32, moved: [f64; 4]) -> [f64; 4] {
        let base = self.color(x as i32, y as i32);
        let m = self.coverage(x as i32, y as i32);
        std::array::from_fn(|k| moved[k] + base[k] * (1. - m) * (1. - moved[3]))
    }
}

/// Solve H(u, v) = p directly. None where the point has no preimage with w > 0.
fn preimage(h: [f64; 9], [x, y]: [f64; 2]) -> Option<[f64; 2]> {
    let [a, b, c, d] = [
        h[0] - x * h[6],
        h[1] - x * h[7],
        h[3] - y * h[6],
        h[4] - y * h[7],
    ];
    let [e, f] = [x * h[8] - h[2], y * h[8] - h[5]];
    let det = a * d - b * c;
    let [u, v] = [(e * d - b * f) / det, (a * f - e * c) / det];
    (det != 0. && h[6] * u + h[7] * v + h[8] > 0. && u.abs() < 1e9 && v.abs() < 1e9)
        .then_some([u, v])
}

fn frame(r: &mut WgpuRasterizer, layer: &Layer, reset: bool) {
    r.submit(FramePacket {
        view: ViewState {
            width_px: EXTENT[0],
            height_px: EXTENT[1],
            ..view()
        },
        document_extent: EXTENT,
        layers: std::slice::from_ref(layer),
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: reset,
    })
    .unwrap();
}

/// Every Float32 texel of the layer's pages, by layer pixel.
fn pages(r: &WgpuRasterizer) -> Vec<([u32; 2], [f32; 4])> {
    let mut texels = Vec::new();
    for page in &r.paint_layers[0].pages {
        let bytes = page_bytes(r, &page.active().texture);
        let origin = page.coordinate.map(|v| v * PAGE_SIZE);
        for (i, texel) in bytes.chunks_exact(16).enumerate() {
            let [x, y] = [
                origin[0] + i as u32 % PAGE_SIZE,
                origin[1] + i as u32 / PAGE_SIZE,
            ];
            if x < EXTENT[0] && y < EXTENT[1] {
                let value = std::array::from_fn(|k| {
                    f32::from_le_bytes(texel[k * 4..k * 4 + 4].try_into().unwrap())
                });
                texels.push(([x, y], value));
            }
        }
    }
    texels
}

#[test]
fn native_transforms_match_an_independent_oracle_for_every_filter_and_map() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "oracle");
    layer.source = Some(source_image());
    frame(&mut r, &layer, true);
    let pivot = Point { x: 150., y: 110. };
    let source = Rect {
        min: Point { x: 40., y: 30. },
        max: Point { x: 230., y: 180. },
    };
    let quad = |q: [[f32; 2]; 4]| {
        TransformMap::Projective(
            Projective::rect_to_quad(source, q.map(|[x, y]| Point { x, y })).unwrap(),
        )
    };
    let maps = [
        TransformMap::Affine(Affine::translation(Point { x: 256., y: 0. })),
        TransformMap::Affine(Affine::around(
            pivot,
            [1.7, 1.3],
            0.31,
            Point { x: 3.5, y: -2.25 },
        )),
        TransformMap::Affine(Affine::around(pivot, [-1., 1.], 0.1, Point::default())),
        TransformMap::Affine(Affine::around(pivot, [0.25, 0.4], -0.7, Point::default())),
        quad([[30.3, 20.1], [250.2, 45.4], [280.1, 200.3], [10.2, 170.4]]),
        quad([[120.3, 60.1], [150.2, 60.4], [290.1, 210.3], [5.2, 210.4]]),
        quad([[250.2, 45.4], [30.3, 20.1], [10.2, 170.4], [280.1, 200.3]]),
    ];
    let pixels = selection_pixels();
    let mut transaction = 0;
    for mode in 0..3 {
        let oracle = Oracle { mode };
        let selection = (mode != 0).then(|| {
            let mut selection = Selection::pixels(pixels.clone());
            selection.inverted = mode == 2;
            selection
        });
        for interpolation in [
            Interpolation::Nearest,
            Interpolation::Linear,
            Interpolation::Bicubic,
        ] {
            for map in &maps {
                transaction += 1;
                let preview = layer_render::TransformPreview {
                    transaction,
                    moving: false,
                    layer: layer.id,
                    selection: selection.clone(),
                    transform: ImageTransform {
                        map: map.clone(),
                        interpolation,
                    },
                };
                r.set_transform_preview(Some(&preview)).unwrap();
                frame(&mut r, &layer, false);
                let h = match map {
                    TransformMap::Affine(affine) => Projective::from_affine(*affine),
                    TransformMap::Projective(projective) => *projective,
                    TransformMap::Mesh(_) => unreachable!(),
                }
                .0
                .map(f64::from);
                let texels = pages(&r);
                assert!(!texels.is_empty(), "the preview draws pages");
                for ([x, y], actual) in texels {
                    let expected: Vec<_> = oracle
                        .moved(h, interpolation, x, y)
                        .into_iter()
                        .map(|moved| oracle.composed(x, y, moved))
                        .collect();
                    assert!(
                        expected.iter().any(|e| e
                            .iter()
                            .zip(actual)
                            .all(|(e, a)| (e - f64::from(a)).abs() <= 2e-4 + e.abs() * 2e-5)),
                        "mode {mode} {interpolation:?} {map:?} at {x},{y}: {actual:?} not in {expected:?}"
                    );
                }
                r.set_transform_preview(None).unwrap();
                frame(&mut r, &layer, false);
            }
        }
    }
}

#[test]
fn minified_native_transforms_average_the_pixel_footprint() {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        64 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let row: Vec<u8> = (0..EXTENT[0])
            .flat_map(|x| {
                let on = f32::from((x + y) % 2 == 0);
                [on, on, on, 1.]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "checkerboard");
    layer.source = Some(Arc::new(builder.finish().unwrap()));
    frame(&mut r, &layer, true);
    for (transaction, interpolation) in [Interpolation::Linear, Interpolation::Bicubic]
        .into_iter()
        .enumerate()
    {
        for scale in [1. / 3., 0.25] {
            r.set_transform_preview(Some(&layer_render::TransformPreview {
                transaction: transaction as u64 * 2 + u64::from(scale < 0.3) + 1,
                moving: false,
                layer: layer.id,
                selection: Some(
                    Selection::polygon(vec![
                        Point::default(),
                        Point { x: 300., y: 0. },
                        Point { x: 300., y: 220. },
                        Point { x: 0., y: 220. },
                    ])
                    .unwrap(),
                ),
                transform: ImageTransform {
                    map: TransformMap::Affine(Affine::around(
                        Point::default(),
                        [scale; 2],
                        0.1,
                        Point { x: 1.3, y: 0.7 },
                    )),
                    interpolation,
                },
            }))
            .unwrap();
            frame(&mut r, &layer, false);
            let values: Vec<f32> = pages(&r)
                .into_iter()
                .filter(|([x, y], _)| (8..60).contains(x) && (12..44).contains(y))
                .map(|(_, v)| v[0])
                .collect();
            let [low, high] = [
                values.iter().copied().fold(f32::INFINITY, f32::min),
                values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            ];
            assert!(
                high - low <= 0.15,
                "{interpolation:?} at scale {scale}: an aliased checkerboard spans {low}..{high}"
            );
        }
    }
}

#[test]
fn bicubic_mask_transforms_keep_scalar_coverage_within_the_unit_interval() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "masked");
    let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
    mask.default_coverage = 0.;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 20., y: 20. },
            Point { x: 60., y: 20. },
            Point { x: 60., y: 60. },
            Point { x: 20., y: 60. },
        ])
        .unwrap(),
    );
    layer.mask = Some(mask);
    frame(&mut r, &layer, true);
    r.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction: 1,
        moving: false,
        layer: LayerId(9),
        selection: None,
        transform: ImageTransform {
            map: TransformMap::Affine(Affine([3.7, 0.3, -0.2, 3.9, 1.5, 0.5])),
            interpolation: Interpolation::Bicubic,
        },
    }))
    .unwrap();
    frame(&mut r, &layer, false);
    let mut values = Vec::new();
    for (&(owner, _), page) in &r.layer_masks.pages {
        if owner == LayerId(9) {
            values.extend(
                page_bytes(&r, &page.texture)
                    .chunks_exact(4)
                    .map(|v| f32::from_le_bytes(v.try_into().unwrap())),
            );
        }
    }
    assert!(!values.is_empty(), "the preview draws mask pages");
    assert!(
        values.iter().all(|v| (0. ..=1.).contains(v)),
        "bicubic lobes leave the unit interval"
    );
    assert!(values.iter().any(|v| *v > 0.999) && values.iter().any(|v| *v > 0.01 && *v < 0.99));
}
