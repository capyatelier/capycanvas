//! Independent f64 models of layer transforms on the native document path:
//! the renderer's paint pages after a preview are compared with the
//! premultiplied source, the selection and each filter evaluated on the CPU.
use super::*;
use super::transforms::{mask_values, masked, packed};
use crate::pixel_transform::EXACT_TAPS;
use crate::test_support::preimage;
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

/// A Float32 sRGB photo of straight `pixel` colors.
fn source_image(pixel: impl Fn(u32, u32) -> [f32; 4]) -> Arc<SourceImage> {
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
            .flat_map(|x| pixel(x, y))
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
    Arc::new(SelectionPixels::bytes(EXTENT, [40, 30, 230, 180], packed(EXTENT, 4, coverage_byte)).unwrap())
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
    fn bicubic(&self, s: [f64; 2]) -> [f64; 4] {
        self.clamped(s, 2, |d| {
            if d < 1. {
                (1.5 * d - 2.5) * d * d + 1.
            } else if d < 2. {
                ((-0.5 * d + 2.5) * d - 4.) * d + 2.
            } else {
                0.
            }
        })
    }
    /// Lanczos-3 with normalized weights, clamped as bicubic is.
    fn lanczos(&self, s: [f64; 2]) -> [f64; 4] {
        self.clamped(s, 3, |d| {
            if d < 1e-4 {
                1.
            } else if d >= 3. {
                0.
            } else {
                let p = std::f64::consts::PI * d;
                3. * p.sin() * (p / 3.).sin() / (p * p)
            }
        })
    }
    /// A separable filter of `radius` taps each side, of the distance to each
    /// tap, with normalized weights and the bicubic clamp.
    fn clamped(&self, [u, v]: [f64; 2], radius: i32, kernel: impl Fn(f64) -> f64) -> [f64; 4] {
        let [left, top] = [(u - 0.5).floor() as i32, (v - 0.5).floor() as i32];
        let kernel = &kernel;
        let taps = |at: f64, first: i32| (first + 1 - radius..=first + radius).map(move |i| (i, kernel((at - 0.5 - i as f64).abs())));
        let [total_x, total_y] = [taps(u, left).map(|(_, w)| w).sum::<f64>(), taps(v, top).map(|(_, w)| w).sum::<f64>()];
        let mut sum = [0.; 4];
        let mut low = [f64::INFINITY; 4];
        let mut high = [f64::NEG_INFINITY; 4];
        let mut brightest = [0f64; 3];
        for (y, wy) in taps(v, top) {
            for (x, wx) in taps(u, left) {
                let w = wx * wy / (total_x * total_y);
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
    /// The values of a nearest sample at `s` within `slack` of a texel edge.
    fn nearest(&self, s: [f64; 2], slack: f64) -> Vec<[f64; 4]> {
        let [xs, ys] = s.map(|v| [(v - slack).floor() as i32, (v + slack).floor() as i32]);
        xs.into_iter().flat_map(|x| ys.map(|y| self.selected(x, y))).collect()
    }
    /// A filtered sample at `s`, or with `count` taps along either axis the
    /// mean of that grid of bilinear taps, each at `tap` of its offset within
    /// the pixel.
    fn filtered(
        &self,
        interpolation: Interpolation,
        s: [f64; 2],
        [nx, ny]: [u32; 2],
        tap: impl Fn([f64; 2]) -> Option<[f64; 2]>,
    ) -> [f64; 4] {
        match (nx, ny, interpolation) {
            (1, 1, Interpolation::Bicubic) => self.bicubic(s),
            (1, 1, Interpolation::Lanczos) => self.lanczos(s),
            (1, 1, _) => self.bilinear(s),
            _ => {
                let mut sum = [0.; 4];
                for j in 0..ny {
                    for i in 0..nx {
                        let o = [(i as f64 + 0.5) / nx as f64 - 0.5, (j as f64 + 0.5) / ny as f64 - 0.5];
                        let value = tap(o).map_or([0.; 4], |p| self.bilinear(p));
                        for k in 0..4 {
                            sum[k] += value[k] / (nx * ny) as f64;
                        }
                    }
                }
                sum
            }
        }
    }
    /// Every value a destination pixel may take: its source footprint sets
    /// the tap grid, and a footprint on a grid boundary allows either count.
    fn moved(&self, h: [f64; 9], interpolation: Interpolation, x: u32, y: u32) -> Vec<[f64; 4]> {
        let center = [x as f64 + 0.5, y as f64 + 0.5];
        let Some(source) = preimage(h, center) else {
            return vec![[0.; 4]];
        };
        if interpolation == Interpolation::Nearest {
            return self.nearest(source, 1e-3);
        }
        let step = 1e-4;
        let counts = [[step, 0.], [0., step]].map(|[dx, dy]| {
            let ends = [-1., 1.].map(|s| preimage(h, [center[0] + s * dx, center[1] + s * dy]));
            let [Some(a), Some(b)] = ends else {
                return vec![1];
            };
            let reach = (b[0] - a[0]).hypot(b[1] - a[1]) / (2. * step) + 0.5;
            let count = |r: f64| (r.floor() as u32).clamp(1, EXACT_TAPS);
            let mut counts = vec![count(reach)];
            for other in [count(reach - 1e-3), count(reach + 1e-3)] {
                if !counts.contains(&other) {
                    counts.push(other);
                }
            }
            counts
        });
        let tap = |o: [f64; 2]| preimage(h, [center[0] + o[0], center[1] + o[1]]);
        // A clamped filter's nearest four texels change across a texel
        // boundary, which Float32 evaluation may place either side of.
        let slack: &[[f64; 2]] = if matches!(interpolation, Interpolation::Bicubic | Interpolation::Lanczos) {
            &[[0., 0.], [-2e-4, 0.], [2e-4, 0.], [0., -2e-4], [0., 2e-4]]
        } else {
            &[[0., 0.]]
        };
        counts[0]
            .iter()
            .flat_map(|&nx| counts[1].iter().map(move |&ny| [nx, ny]))
            .flat_map(|count| {
                slack.iter().map(move |[ox, oy]| {
                    let at = if count == [1, 1] { [source[0] + ox, source[1] + oy] } else { source };
                    self.filtered(interpolation, at, count, tap)
                })
            })
            .collect()
    }
    /// The moved pixel over the original, cut where the selection moved it
    /// unless the transform keeps its source.
    fn composed(&self, x: u32, y: u32, moved: [f64; 4], keep_source: bool) -> [f64; 4] {
        let base = self.color(x as i32, y as i32);
        let m = if keep_source { 0. } else { self.coverage(x as i32, y as i32) };
        std::array::from_fn(|k| moved[k] + base[k] * (1. - m) * (1. - moved[3]))
    }
}

fn frame(r: &mut WgpuRasterizer, layer: &Layer, reset: bool) {
    r.submit(FramePacket {
        reset_layers: reset,
        composite_all: reset,
        ..packet(std::slice::from_ref(layer), EXTENT)
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
    layer.source = Some(source_image(straight));
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
            Interpolation::Lanczos,
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
                        ..Default::default()
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
                check_pages(&oracle, h, interpolation, false, texels, format_args!("mode {mode} {interpolation:?} {map:?}"));
                r.set_transform_preview(None).unwrap();
                frame(&mut r, &layer, false);
            }
        }
    }
}

#[test]
fn transforms_that_keep_their_source_place_the_moved_copy_over_the_original() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "oracle");
    layer.source = Some(source_image(straight));
    frame(&mut r, &layer, true);
    let maps = [
        Affine::translation(Point { x: 37., y: -12. }),
        Affine::translation(Point { x: -150., y: 90. }),
        Affine::around(Point { x: 150., y: 110. }, [1.3, 0.8], 0.4, Point { x: 20.5, y: -6.25 }),
    ];
    let pixels = selection_pixels();
    let mut transaction = 0;
    for mode in 0..3 {
        let oracle = Oracle { mode };
        let selection = (mode != 0).then(|| Selection { inverted: mode == 2, ..Selection::pixels(pixels.clone()) });
        for interpolation in [Interpolation::Nearest, Interpolation::Bicubic] {
            for affine in maps {
                transaction += 1;
                let transform = ImageTransform {
                    map: TransformMap::Affine(affine),
                    interpolation,
                    keep_source: true,
                };
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction,
                    moving: false,
                    layer: layer.id,
                    selection: selection.clone(),
                    transform,
                }))
                .unwrap();
                frame(&mut r, &layer, false);
                let h = Projective::from_affine(affine).0.map(f64::from);
                check_pages(&oracle, h, interpolation, true, pages(&r), format_args!("mode {mode} {interpolation:?} {affine:?}"));
                r.set_transform_preview(None).unwrap();
                frame(&mut r, &layer, false);
            }
        }
    }
}

#[test]
fn minified_native_transforms_average_the_pixel_footprint() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "checkerboard");
    layer.source = Some(source_image(|x, y| {
        let on = f32::from((x + y) % 2 == 0);
        [on, on, on, 1.]
    }));
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
                    ..Default::default()
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
fn bicubic_and_lanczos_mask_transforms_keep_scalar_coverage_within_the_unit_interval() {
    for interpolation in [Interpolation::Bicubic, Interpolation::Lanczos] {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let layer = masked([[20., 20.], [60., 20.], [60., 60.], [20., 60.]]);
        frame(&mut r, &layer, true);
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            layer: LayerId(9),
            selection: None,
            transform: ImageTransform {
                map: TransformMap::Affine(Affine([3.7, 0.3, -0.2, 3.9, 1.5, 0.5])),
                interpolation,
                ..Default::default()
            },
        }))
        .unwrap();
        frame(&mut r, &layer, false);
        let values: Vec<f32> = mask_values(&r).into_values().flatten().collect();
        assert!(!values.is_empty(), "the preview draws mask pages");
        assert!(
            values.iter().all(|v| (0. ..=1.).contains(v)),
            "{interpolation:?} lobes leave the unit interval"
        );
        assert!(values.iter().any(|v| *v > 0.999) && values.iter().any(|v| *v > 0.01 && *v < 0.99));
    }
}

/// Source positions of the mesh's triangles at pixel centers, rasterized on
/// the CPU with later triangles winning: centers more than 0.02 px inside a
/// triangle, and centers within 0.02 px of its edges, where snapped vertices
/// allow either rasterization.
/// Hardware interpolates them to about 1/32768 of their size, so nearest
/// sampling and the minification grid allow either side of a boundary.
fn mesh_positions(
    geometry: &crate::paint_transform::mesh::MeshGeometry,
) -> ([Vec<Option<[f64; 2]>>; 2], usize) {
    let size = (EXTENT[0] * EXTENT[1]) as usize;
    let mut maps = [vec![None; size], vec![None; size]];
    let mut hits = vec![0u32; size];
    let vertex = |i: u32| geometry.vertices[i as usize].map(f64::from);
    for triangle in geometry.triangles() {
        let [a, b, c] = [
            vertex(triangle[0]),
            vertex(triangle[1]),
            vertex(triangle[2]),
        ];
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 {
            continue;
        }
        let low = [0, 1].map(|k| a[k].min(b[k]).min(c[k]).floor().max(0.) as u32);
        let high = [0, 1].map(|k| (a[k].max(b[k]).max(c[k]).ceil().max(0.) as u32).min(EXTENT[k]));
        for y in low[1]..high[1] {
            for x in low[0]..high[0] {
                let p = [x as f64 + 0.5, y as f64 + 0.5];
                let weight = |u: [f64; 4], v: [f64; 4]| {
                    ((u[0] - p[0]) * (v[1] - p[1]) - (u[1] - p[1]) * (v[0] - p[0])) / area
                };
                let l = [weight(b, c), weight(c, a), weight(a, b)];
                let edges = [(b, c), (c, a), (a, b)].map(|(u, v)| (u[0] - v[0]).hypot(u[1] - v[1]));
                let nearest_edge = (0..3)
                    .map(|i| l[i] * area.abs() / edges[i])
                    .fold(f64::INFINITY, f64::min);
                let source = [2, 3].map(|k| l[0] * a[k] + l[1] * b[k] + l[2] * c[k]);
                let index = (y * EXTENT[0] + x) as usize;
                if nearest_edge > 0.02 {
                    maps[0][index] = Some(source);
                    hits[index] += 1;
                }
                if nearest_edge > -0.02 {
                    maps[1][index] = Some(source);
                }
            }
        }
    }
    (maps, hits.iter().filter(|h| **h > 1).count())
}

impl Oracle {
    /// The moved value at a pixel of a mesh transform, from rasterized
    /// source positions.
    fn meshed(
        &self,
        positions: &[Option<[f64; 2]>],
        interpolation: Interpolation,
        x: u32,
        y: u32,
    ) -> Vec<[f64; 4]> {
        let at = |x: i64, y: i64| -> Option<[f64; 2]> {
            let [x, y] = [
                x.clamp(0, EXTENT[0] as i64 - 1),
                y.clamp(0, EXTENT[1] as i64 - 1),
            ];
            positions[(y as u32 * EXTENT[0] + x as u32) as usize]
        };
        let Some(s) = at(x as i64, y as i64) else {
            return vec![[0.; 4]];
        };
        if interpolation == Interpolation::Nearest {
            return self.nearest(s, 5e-3);
        }
        let step = |dx: i64, dy: i64| -> [f64; 2] {
            let [after, before] = [
                at(x as i64 + dx, y as i64 + dy),
                at(x as i64 - dx, y as i64 - dy),
            ];
            match (after, before) {
                (Some(a), Some(b)) => [(a[0] - b[0]) * 0.5, (a[1] - b[1]) * 0.5],
                (Some(a), None) => [a[0] - s[0], a[1] - s[1]],
                (None, Some(b)) => [s[0] - b[0], s[1] - b[1]],
                (None, None) => [0.; 2],
            }
        };
        let [dx, dy] = [step(1, 0), step(0, 1)];
        let reach = [dx[0].hypot(dx[1]), dy[0].hypot(dy[1])];
        let count = |r: f64| ((r + 0.5).floor() as u32).clamp(1, EXACT_TAPS);
        let mut values = Vec::new();
        let mut counts = Vec::new();
        for offset in [0., -1e-2, 1e-2] {
            let n = [count(reach[0] + offset), count(reach[1] + offset)];
            if !counts.contains(&n) {
                counts.push(n);
            }
        }
        for n in counts {
            if n == [1, 1] && matches!(interpolation, Interpolation::Bicubic | Interpolation::Lanczos) {
                // Its clamp follows the nearest four texels, which change
                // across a texel boundary.
                for [ox, oy] in [[0., 0.], [-5e-3, 0.], [5e-3, 0.], [0., -5e-3], [0., 5e-3]] {
                    values.push(self.filtered(interpolation, [s[0] + ox, s[1] + oy], n, |_| None));
                }
                continue;
            }
            values.push(self.filtered(interpolation, s, n, |o| {
                Some([s[0] + dx[0] * o[0] + dy[0] * o[1], s[1] + dx[1] * o[0] + dy[1] * o[1]])
            }));
        }
        values
    }
}

#[test]
fn native_mesh_transforms_match_the_cpu_tessellation_including_folds() {
    use crate::paint_transform::mesh::MeshGeometry;
    use layer_core::MeshMap;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "mesh oracle");
    layer.source = Some(source_image(straight));
    frame(&mut r, &layer, true);
    let source = Rect {
        min: Point { x: 40., y: 30. },
        max: Point { x: 230., y: 180. },
    };
    let quad =
        [[30.3, 20.1], [250.2, 45.4], [280.1, 200.3], [10.2, 170.4]].map(|[x, y]| Point { x, y });
    let projective = Projective::rect_to_quad(source, quad).unwrap();
    let seeded = MeshMap::fit(source, [3, 3], |p| projective.map(p))
    .unwrap();
    let warped = seeded
        .move_node(5, Point { x: 21.5, y: -14.25 })
        .unwrap()
        .move_tangent(10, 1, Point { x: 190.3, y: 150.7 })
        .unwrap();
    let folded = MeshMap::identity(source, [2, 2])
        .unwrap()
        .move_node(4, Point { x: 150.25, y: 20.5 })
        .unwrap();
    let shrunk = MeshMap::from_affine(
        source,
        [3, 3],
        Affine::around(
            Point { x: 135., y: 105. },
            [0.3, 0.4],
            0.2,
            Point::default(),
        ),
    )
    .unwrap()
    .move_node(6, Point { x: 3., y: 2. })
    .unwrap();
    let pixels = selection_pixels();
    let mut transaction = 100;
    for (mesh, folds) in [(warped, false), (folded, true), (shrunk, false)] {
        let geometry = MeshGeometry::new(&mesh, None);
        let (positions, stacked) = mesh_positions(&geometry);
        let covered = positions[0].iter().filter(|p| p.is_some()).count();
        assert!(covered > 1000, "the mesh covers the layer");
        assert_eq!(
            stacked > 1000,
            folds,
            "only the folded mesh overlaps itself"
        );
        for mode in [0, 1] {
            let oracle = Oracle { mode };
            let selection = (mode == 1).then(|| Selection::pixels(pixels.clone()));
            for interpolation in [
                Interpolation::Nearest,
                Interpolation::Linear,
                Interpolation::Bicubic,
                Interpolation::Lanczos,
            ] {
                transaction += 1;
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction,
                    moving: false,
                    layer: layer.id,
                    selection: selection.clone(),
                    transform: ImageTransform {
                        map: TransformMap::Mesh(Arc::new(mesh.clone())),
                        interpolation,
                        ..Default::default()
                    },
                }))
                .unwrap();
                frame(&mut r, &layer, false);
                let texels = pages(&r);
                assert!(!texels.is_empty());
                let mut mismatches = Vec::new();
                for ([x, y], actual) in &texels {
                    let expected: Vec<_> = positions
                        .iter()
                        .flat_map(|map| oracle.meshed(map, interpolation, *x, *y))
                        .map(|moved| oracle.composed(*x, *y, moved, false))
                        .collect();
                    if !expected.iter().any(|e| {
                        e.iter()
                            .zip(actual)
                            .all(|(e, a)| (e - f64::from(*a)).abs() <= 4e-3 + e.abs() * 2e-4)
                    }) {
                        mismatches.push(([*x, *y], *actual, expected));
                    }
                }
                // Where the mesh folds, sliver triangles stack several source
                // positions within a pixel's rasterization tolerance.
                let allowed = if folds { texels.len() / 1000 } else { 0 };
                assert!(
                    mismatches.len() <= allowed,
                    "mesh {transaction} mode {mode} {interpolation:?}: {} pixels differ, first {:?}",
                    mismatches.len(),
                    mismatches.first()
                );
                r.set_transform_preview(None).unwrap();
                frame(&mut r, &layer, false);
            }
        }
    }
}

#[test]
fn mask_warps_seeded_from_an_affine_match_the_affine() {
    use layer_core::MeshMap;
    let affine = Affine::around(Point { x: 128., y: 96. }, [1.3, 0.8], 0.4, Point { x: 6.5, y: -3.25 });
    let bounds = Rect {
        min: Point::default(),
        max: Point { x: EXTENT[0] as f32, y: EXTENT[1] as f32 },
    };
    let maps = [
        TransformMap::Affine(affine),
        TransformMap::Mesh(Arc::new(MeshMap::from_affine(bounds, [4, 3], affine).unwrap())),
    ];
    for interpolation in [Interpolation::Nearest, Interpolation::Linear, Interpolation::Bicubic] {
        let drawn: Vec<_> = maps
            .iter()
            .map(|map| {
                let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
                let layer = masked([[20., 20.], [150., 30.], [120., 160.], [30., 110.]]);
                frame(&mut r, &layer, true);
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction: 1,
                    moving: false,
                    layer: LayerId(9),
                    selection: None,
                    transform: ImageTransform {
                        map: map.clone(),
                        interpolation,
                        ..Default::default()
                    },
                }))
                .unwrap();
                frame(&mut r, &layer, false);
                mask_values(&r)
            })
            .collect();
        assert!(!drawn[0].is_empty(), "{interpolation:?}: the affine draws mask pages");
        let mut largest = 0f32;
        let mut moved = 0;
        for (c, affine) in &drawn[0] {
            let warped = drawn[1].get(c).unwrap_or_else(|| panic!("{interpolation:?}: the warp draws page {c:?}"));
            for (a, w) in affine.iter().zip(warped) {
                largest = largest.max((a - w).abs());
                moved += usize::from(*a > 0.5);
            }
        }
        assert!(moved > 1000, "{interpolation:?}: the mask moves");
        assert!(largest <= 4e-3, "{interpolation:?}: the warp differs from its affine by {largest}");
    }
}

const PLATE: u32 = 1024;
/// A zone plate whose frequency rises from the centre, reaching one cycle per
/// two pixels at the middle of each edge.
fn zone(x: u32, y: u32) -> f32 {
    let [dx, dy] = [x, y].map(|v| v as f64 + 0.5 - f64::from(PLATE) / 2.);
    (0.5 + 0.5 * (std::f64::consts::PI * (dx * dx + dy * dy) / f64::from(PLATE)).cos()) as f32
}
fn zone_plate() -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        [PLATE; 2],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        64 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..PLATE {
        let row: Vec<u8> = (0..PLATE).flat_map(|x| { let v = zone(x, y); [v, v, v, 1.] }).flat_map(f32::to_le_bytes).collect();
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}
/// The zone plate reduced by `RowResampler`'s area average over footprints
/// half a pixel up and left: the area of each pixel of the plate moved half a
/// pixel is the mean of its 2×2 neighbourhood, with transparency beyond.
fn area_reduced(size: u32) -> Vec<f32> {
    let mut resampler = layer_color::RowResampler::new([PLATE; 2], [size; 2]).unwrap();
    let mut out = vec![[0f32; 4]; size as usize];
    let mut values = Vec::new();
    let pixel = |x: u32, y: u32| if x == 0 || y == 0 { 0. } else { zone(x - 1, y - 1) };
    for y in 0..size {
        resampler
            .read_row(y, &mut out, |sy, row| {
                for (x, out) in row.iter_mut().enumerate() {
                    let x = x as u32;
                    let v = (pixel(x, sy) + pixel(x + 1, sy) + pixel(x, sy + 1) + pixel(x + 1, sy + 1)) / 4.;
                    *out = [v, v, v, 1.];
                }
                Ok(())
            })
            .unwrap();
        values.extend(out.iter().map(|p| p[0]));
    }
    values
}
fn largest_difference(actual: impl Iterator<Item = f32>, expected: &[f32]) -> f32 {
    actual.zip(expected).map(|(a, e)| (a - e).abs()).fold(0., f32::max)
}

/// An eighth of the zone plate, placed half a source pixel to the right and
/// down, so a destination pixel covers the source from -0.5 to 7.5 pixels
/// past its multiple of eight.
const EIGHTH: Affine = Affine([0.125, 0., 0., 0.125, 0.0625, 0.0625]);

#[test]
fn a_zone_plate_reduced_to_an_eighth_matches_an_area_reduction() {
    let size = PLATE / 8;
    let expected = area_reduced(size);
    let extent = [PLATE; 2];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "zone plate");
    layer.source = Some(zone_plate());
    let submit = |r: &mut WgpuRasterizer, layer: &Layer, batches: &[DabBatch], reset: bool| {
        r.submit(FramePacket {
            dab_batches: batches,
            reset_layers: reset,
            composite_all: reset,
            ..packet(std::slice::from_ref(layer), extent)
        })
        .unwrap();
    };
    submit(&mut r, &layer, &[], true);
    let transform = ImageTransform {
        map: TransformMap::Affine(EIGHTH),
        interpolation: Interpolation::Bicubic,
        ..Default::default()
    };
    let reduced = |r: &WgpuRasterizer| {
        let page = r.paint_layers[0].pages.iter().find(|p| p.coordinate == [0, 0]).unwrap();
        let bytes = page_bytes(r, &page.active().texture);
        (0..size * size)
            .map(|i| {
                let offset = (((i / size) * PAGE_SIZE + i % size) * 16) as usize;
                f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
            })
            .collect::<Vec<_>>()
    };
    r.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction: 1,
        moving: true,
        layer: layer.id,
        selection: None,
        transform: transform.clone(),
    }))
    .unwrap();
    submit(&mut r, &layer, &[], false);
    let preview = largest_difference(reduced(&r).into_iter(), &expected);
    r.set_transform_preview(None).unwrap();
    submit(&mut r, &layer, &[], false);
    let mut coverage = LayerMask::reveal_all(LayerId(40), Point::default());
    coverage.default_coverage = 1.;
    let operation = LayerOperation { placement: Affine::IDENTITY, coverage, kind: LayerOperationKind::Transform(transform) };
    let batch = DabBatch {
        kind: DabBatchKind::LayerOperation(0),
        dab_count: 0,
        damage: operation.bounds(extent),
        ..crate::test_support::dab_batch(layer.id, crate::tests::test_style(BrushExecution::Dry), operation.bounds(extent))
    };
    let mut committed = layer.clone();
    committed.pending_operations.push(operation);
    submit(&mut r, &committed, &[batch], false);
    let commit = largest_difference(reduced(&r).into_iter(), &expected);
    assert!(preview > 0.1, "a moving preview averages at most four taps and aliases: {preview}");
    assert!(commit < 1e-3, "the commit averages the whole footprint: {commit} from the area reduction");

    let mut document = layer_core::Document::new("placed zone plate", size, size, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.layers[1].visible = false;
    document.layers[0].source = Some(zone_plate());
    document.layers[0].properties.placement = EIGHTH;
    let mut capture = r.snapshot_gpu().capture(layer_core::Project { document }, [0.; 4], 0., Default::default()).unwrap();
    let exported = capture.read_region([0, 0, size, size]).unwrap();
    let placed = largest_difference(exported.iter().map(|p| p[0]), &expected);
    assert!(placed < 1e-3, "exact capture of a photo placed at an eighth: {placed} from the area reduction");
}

fn check_pages(oracle: &Oracle, map: [f64; 9], interpolation: Interpolation, keep_source: bool,
    texels: Vec<([u32; 2], [f32; 4])>, context: std::fmt::Arguments<'_>) {
    for ([x, y], actual) in texels {
        let expected: Vec<_> = oracle.moved(map, interpolation, x, y).into_iter()
            .map(|moved| oracle.composed(x, y, moved, keep_source)).collect();
        assert!(expected.iter().any(|e| e.iter().zip(actual)
            .all(|(e, a)| (e - f64::from(a)).abs() <= 2e-4 + e.abs() * 2e-5)),
            "{context} at {x},{y}: {actual:?} not in {expected:?}");
    }
}
