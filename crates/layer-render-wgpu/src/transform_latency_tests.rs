//! Continuous transform drags over 24-megapixel paint and photo layers: a new
//! moving transform every frame, then the frame that releases it, with CPU
//! submission, GPU execution and serialized completion kept apart.
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth, source::*};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Affine, ImageTransform, Interpolation, Projective, TransformMap};
use std::time::Instant;

const EXTENT: [u32; 2] = [6000, 4000];
const FRAMES: usize = 200;
const WARMUP: usize = 40;

fn center() -> Point {
    Point {
        x: EXTENT[0] as f32 * 0.5,
        y: EXTENT[1] as f32 * 0.5,
    }
}

fn cases() -> Vec<(&'static str, Box<dyn Fn(f32) -> ImageTransform>)> {
    let affine = |t: f32| {
        Affine::around(
            center(),
            [0.9 + t.sin() * 0.05; 2],
            0.1 + t.cos() * 0.02,
            Point {
                x: t.sin() * 40.,
                y: t.cos() * 30.,
            },
        )
    };
    let quad = |t: f32, depth: f32| {
        let [w, h] = EXTENT.map(|v| v as f32);
        let inset = w * depth * 0.5;
        Projective::rect_to_quad(
            layer_core::Rect {
                min: Point::default(),
                max: Point { x: w, y: h },
            },
            [
                [inset + t.sin() * 60., 150.],
                [w - inset + t.cos() * 40., 100. + t.sin() * 50.],
                [w - 100., h - 80.],
                [120. + t.cos() * 30., h - 60.],
            ]
            .map(|[x, y]| Point { x, y }),
        )
        .unwrap()
    };
    vec![
        (
            "affine bilinear",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Affine(affine(t)),
                interpolation: Interpolation::Linear,
            }),
        ),
        (
            "perspective bilinear",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Projective(quad(t, 0.3)),
                interpolation: Interpolation::Linear,
            }),
        ),
        (
            "deep perspective bilinear",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Projective(quad(t, 0.9)),
                interpolation: Interpolation::Linear,
            }),
        ),
        (
            "affine bicubic",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Affine(affine(t)),
                interpolation: Interpolation::Bicubic,
            }),
        ),
        (
            "perspective bicubic",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Projective(quad(t, 0.3)),
                interpolation: Interpolation::Bicubic,
            }),
        ),
        (
            "deep perspective bicubic",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Projective(quad(t, 0.9)),
                interpolation: Interpolation::Bicubic,
            }),
        ),
        (
            "quarter scale bicubic",
            Box::new(move |t| ImageTransform {
                map: TransformMap::Affine(Affine::around(
                    center(),
                    [0.25 + t.sin() * 0.01; 2],
                    t.cos() * 0.02,
                    Point::default(),
                )),
                interpolation: Interpolation::Bicubic,
            }),
        ),
        ("3x3 warp bicubic", Box::new(move |t| warp(t, [3, 3]))),
        ("5x5 warp bicubic", Box::new(move |t| warp(t, [5, 5]))),
    ]
}

/// A mesh seeded from a keystone with two nodes dragged, as a Warp drag moves
/// one handle every frame.
fn warp(t: f32, cells: [u16; 2]) -> ImageTransform {
    let [w, h] = EXTENT.map(|v| v as f32);
    let bounds = layer_core::Rect {
        min: Point::default(),
        max: Point { x: w, y: h },
    };
    let keystone = Projective::rect_to_quad(
        bounds,
        [
            [300., 150.],
            [w - 250., 100.],
            [w - 100., h - 80.],
            [120., h - 60.],
        ]
        .map(|[x, y]| Point { x, y }),
    )
    .unwrap();
    let columns = u32::from(cells[0]) + 1;
    let mesh = layer_core::MeshMap::from_projective(bounds, cells, &keystone)
        .unwrap()
        .move_node(
            columns + 1,
            Point {
                x: 400. * t.sin(),
                y: 250. * t.cos(),
            },
        )
        .unwrap()
        .move_node(
            columns * 2 - 2,
            Point {
                x: -300. * t.cos(),
                y: 180.,
            },
        )
        .unwrap();
    ImageTransform {
        map: TransformMap::Mesh(std::sync::Arc::new(mesh)),
        interpolation: Interpolation::Bicubic,
    }
}

fn submit(r: &mut WgpuRasterizer, layer: &Layer, reset: bool) {
    let zoom = (1920. / EXTENT[0] as f32).min(1080. / EXTENT[1] as f32);
    r.submit(FramePacket {
        view: ViewState { width_px: 1920, height_px: 1080, document_to_surface: [zoom, 0., 0., zoom, 0., 0.], ..view() },
        reset_layers: reset,
        composite_all: reset,
        ..packet(std::slice::from_ref(layer), EXTENT)
    })
    .unwrap();
}

fn percentiles(mut values: Vec<f64>) -> [f64; 3] {
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 0.99].map(|p| values[(values.len() as f64 * p).ceil() as usize - 1])
}

/// Drag each case's transform across FRAMES frames of one transaction.
fn drag(r: &mut WgpuRasterizer, layer: &Layer, label: &str) -> f64 {
    let selection = Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point {
            x: EXTENT[0] as f32,
            y: 0.,
        },
        Point {
            x: EXTENT[0] as f32,
            y: EXTENT[1] as f32,
        },
        Point {
            x: 0.,
            y: EXTENT[1] as f32,
        },
    ])
    .unwrap();
    let mut worst = 0f64;
    for (transaction, (name, transform)) in cases().into_iter().enumerate() {
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        r.telemetry = telemetry::Telemetry::new(r.device(), r.queue());
        r.set_telemetry_enabled(true);
        let mut preview = layer_render::TransformPreview {
            transaction: transaction as u64 + 1,
            moving: true,
            layer: layer.id,
            selection: Some(selection.clone()),
            transform: transform(0.),
        };
        for i in 0..FRAMES {
            preview.transform = transform(i as f32 * 0.04);
            let start = Instant::now();
            r.set_transform_preview(Some(&preview)).unwrap();
            submit(r, layer, false);
            let submitted = start.elapsed().as_secs_f64() * 1000.;
            r.wait_idle().unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            if i == 0 {
                eprintln!("{label} {name}: first frame {elapsed:.3}ms");
            } else if i >= WARMUP {
                cpu.push(submitted);
                completed.push(elapsed);
            }
        }
        let gpu = r
            .telemetry()
            .gpu
            .ordered()
            .into_iter()
            .map(f64::from)
            .collect();
        let (cpu, gpu, completed) = (percentiles(cpu), percentiles(gpu), percentiles(completed));
        preview.moving = false;
        let start = Instant::now();
        r.set_transform_preview(Some(&preview)).unwrap();
        submit(r, layer, false);
        r.wait_idle().unwrap();
        let settled = start.elapsed().as_secs_f64() * 1000.;
        eprintln!(
            "{label} {name} 6000x4000, full selection: CPU submit p50/p95/p99 {cpu:.3?}ms, GPU execution {gpu:.3?}ms, completion {completed:.3?}ms; release {settled:.3}ms"
        );
        worst = worst.max(completed[2]).max(settled);
        r.set_transform_preview(None).unwrap();
        submit(r, layer, false);
        r.wait_idle().unwrap();
    }
    worst
}

#[test]
#[ignore = "hardware 24-megapixel paint transform benchmark; release, serial"]
fn large_paint_transform_latency() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut data = RasterData::default();
    for ty in 0..EXTENT[1].div_ceil(PAGE_SIZE) {
        for tx in 0..EXTENT[0].div_ceil(PAGE_SIZE) {
            let pixels: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE)
                .flat_map(|i| {
                    let (x, y) = (
                        tx * PAGE_SIZE + i % PAGE_SIZE,
                        ty * PAGE_SIZE + i / PAGE_SIZE,
                    );
                    [(x % 200) as u8, (y % 220) as u8, ((x ^ y) % 256) as u8, 230]
                })
                .collect();
            data.tiles.insert(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [tx, ty],
                },
                RasterTile::backed(
                    TileBlob::encode(DocumentColor::default().paint_descriptor(), &pixels).unwrap(),
                ),
            );
        }
    }
    let mut layer = Layer::paint(LayerId(1), "large paint transform");
    layer.raster = RasterRevision::backed(data);
    submit(&mut r, &layer, true);
    r.wait_idle().unwrap();
    let worst = drag(&mut r, &layer, "paint");
    assert!(worst < 8.333, "a transform drag exceeds the 120 Hz budget");
}

#[test]
#[ignore = "hardware 24-megapixel photo transform benchmark; release, serial"]
fn large_photo_transform_latency() {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        512 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let row: Vec<_> = (0..EXTENT[0])
            .flat_map(|x| {
                [
                    ((x * 8191 + y * 31) % 65536) as u16,
                    ((x * 17 + y * 16381) % 65536) as u16,
                    ((x / 173 + y / 111) % 2 * 65535) as u16,
                    65535,
                ]
            })
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    let mut layer = Layer::paint(LayerId(1), "large photo transform");
    layer.source = Some(std::sync::Arc::new(builder.finish().unwrap()));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &layer, true);
    r.wait_idle().unwrap();
    let worst = drag(&mut r, &layer, "photo");
    assert!(
        worst < 8.333,
        "a photo transform drag exceeds the 120 Hz budget"
    );
}

/// The GTK photo24 workload without its adjustment layers: a 16-bit ProPhoto
/// photo in a native document, alone or with painted strokes above it and a
/// second photo below.
fn native_photo_document(layered: bool) -> layer_core::Document {
    let mut document = layer_core::Document::new("photo", EXTENT[0], EXTENT[1]);
    document.color = layer_core::color::DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let source = |pixel: &dyn Fn(u32, u32) -> [u16; 4]| {
        let mut builder = SourceBuilder::new(
            EXTENT,
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: SampleDepth::U16,
                profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
                profile_assumed: false,
            },
            512 * 1024 * 1024,
        )
        .unwrap();
        for y in 0..EXTENT[1] {
            let row: Vec<_> = (0..EXTENT[0])
                .flat_map(|x| pixel(x, y))
                .flat_map(u16::to_le_bytes)
                .collect();
            builder.push_row(&row).unwrap();
        }
        std::sync::Arc::new(builder.finish().unwrap())
    };
    document.layers[0].source = Some(source(&|x, y| {
        [
            ((x * 55000 / EXTENT[0] + (x * 7 + y * 13) % 1024) % 65536) as u16,
            ((y * 55000 / EXTENT[1] + (x * 11 + y * 5) % 1024) % 65536) as u16,
            (((x + y) * 13 % 60000) + (x ^ y) % 1024) as u16,
            65535,
        ]
    }));
    if layered {
        let mut strokes = Layer::paint(document.allocate_layer_id(), "strokes");
        strokes.source = Some(source(&|x, y| {
            let band = (x + 2 * y) % 900;
            let alpha = if band < 60 {
                65535
            } else if band < 80 {
                ((80 - band) * 3276) as u16
            } else {
                0
            };
            [52000, 21000, 9000, alpha]
        }));
        let mut backdrop = Layer::paint(document.allocate_layer_id(), "backdrop");
        backdrop.source = Some(source(&|x, y| {
            [
                (x * 11 % 50000) as u16,
                (y * 13 % 50000) as u16,
                ((x ^ y) % 40000) as u16,
                65535,
            ]
        }));
        document.layers.insert(0, strokes);
        document.layers.insert(2, backdrop);
    }
    document
}

fn native_submit(r: &mut WgpuRasterizer, layers: &[Layer], reset: bool) {
    let [width, height] = [1600u32, 1000];
    let zoom = (width as f32 / EXTENT[0] as f32).min(height as f32 / EXTENT[1] as f32) * 0.95;
    let offset = [
        (width as f32 - EXTENT[0] as f32 * zoom) * 0.5,
        (height as f32 - EXTENT[1] as f32 * zoom) * 0.5,
    ];
    r.submit(FramePacket {
        view: ViewState { width_px: width, height_px: height, document_to_surface: [zoom, 0., 0., zoom, offset[0], offset[1]], ..view() },
        reset_layers: reset,
        composite_all: reset,
        ..packet(layers, EXTENT)
    })
    .unwrap();
}

/// Free drags translate the selected photo; Distort drags one corner.
fn native_photo_drag(r: &mut WgpuRasterizer, layers: &[Layer], photo: LayerId, label: &str) -> f64 {
    let selection = Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point {
            x: EXTENT[0] as f32,
            y: 0.,
        },
        Point {
            x: EXTENT[0] as f32,
            y: EXTENT[1] as f32,
        },
        Point {
            x: 0.,
            y: EXTENT[1] as f32,
        },
    ])
    .unwrap();
    let [w, h] = EXTENT.map(|v| v as f32);
    let bounds = layer_core::Rect {
        min: Point::default(),
        max: Point { x: w, y: h },
    };
    let warp_start = layer_core::MeshMap::identity(bounds, [4, 4]).unwrap();
    let cases: [(&str, Box<dyn Fn(f32) -> TransformMap>); 3] = [
        (
            "free",
            Box::new(|t: f32| {
                TransformMap::Affine(Affine([
                    1.,
                    0.,
                    0.,
                    1.,
                    w * 0.17 * t.sin(),
                    h * 0.13 * (t * 1.3).sin(),
                ]))
            }),
        ),
        (
            "distort",
            Box::new(move |t: f32| {
                TransformMap::Projective(
                    Projective::rect_to_quad(
                        bounds,
                        [
                            [w * 0.17 * t.sin(), h * 0.13 * (t * 1.3).sin()],
                            [w, 0.],
                            [w, h],
                            [0., h],
                        ]
                        .map(|[x, y]| Point { x, y }),
                    )
                    .unwrap(),
                )
            }),
        ),
        (
            "warp",
            Box::new(move |t: f32| {
                TransformMap::Mesh(std::sync::Arc::new(
                    warp_start
                        .move_node(6, Point { x: w * 0.08 * t.sin(), y: h * 0.06 * (t * 1.3).sin() })
                        .unwrap(),
                ))
            }),
        ),
    ];
    let mut worst = 0f64;
    for (transaction, (name, map)) in cases.into_iter().enumerate() {
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        r.telemetry = telemetry::Telemetry::new(r.device(), r.queue());
        r.set_telemetry_enabled(true);
        let mut preview = layer_render::TransformPreview {
            transaction: transaction as u64 + 1,
            moving: true,
            layer: photo,
            selection: Some(selection.clone()),
            transform: ImageTransform {
                map: map(0.),
                interpolation: Interpolation::Bicubic,
            },
        };
        let mut idle = Vec::new();
        let still = layer_render::TransformPreview {
            moving: false,
            transform: ImageTransform::default(),
            ..preview.clone()
        };
        loop {
            let start = Instant::now();
            r.set_transform_preview(Some(&still)).unwrap();
            native_submit(r, layers, false);
            r.wait_idle().unwrap();
            idle.push(start.elapsed().as_secs_f64() * 1000.);
            if !r.has_pending_work() || idle.len() > 64 {
                break;
            }
        }
        eprintln!(
            "{label} {name}: transform start {} frames, {:.3}ms in all, longest {:.3}ms",
            idle.len(),
            idle.iter().sum::<f64>(),
            idle.iter().copied().fold(0f64, f64::max)
        );
        for i in 0..FRAMES {
            preview.transform.map = map(i as f32 / 120. * 2.);
            let start = Instant::now();
            r.set_transform_preview(Some(&preview)).unwrap();
            native_submit(r, layers, false);
            let submitted = start.elapsed().as_secs_f64() * 1000.;
            r.wait_idle().unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            if i == 0 {
                eprintln!("{label} {name}: first frame {elapsed:.3}ms");
            } else if i >= WARMUP {
                cpu.push(submitted);
                completed.push(elapsed);
            }
        }
        let gpu = r
            .telemetry()
            .gpu
            .ordered()
            .into_iter()
            .map(f64::from)
            .collect();
        let (cpu, gpu, completed) = (percentiles(cpu), percentiles(gpu), percentiles(completed));
        preview.moving = false;
        let mut release = Vec::new();
        loop {
            let start = Instant::now();
            r.set_transform_preview(Some(&preview)).unwrap();
            native_submit(r, layers, false);
            r.wait_idle().unwrap();
            release.push(start.elapsed().as_secs_f64() * 1000.);
            if !r.has_pending_work() {
                break;
            }
        }
        let settled = release.iter().copied().fold(0f64, f64::max);
        eprintln!(
            "{label} {name}: CPU submit p50/p95/p99 {cpu:.3?}ms, GPU execution {gpu:.3?}ms, completion {completed:.3?}ms; release {} frames, longest {settled:.3}ms",
            release.len()
        );
        worst = worst.max(completed[2]).max(settled);
        r.set_transform_preview(None).unwrap();
        native_submit(r, layers, false);
        r.wait_idle().unwrap();
    }
    worst
}

#[test]
#[ignore = "hardware 24-megapixel native photo transform benchmark; release, serial"]
fn native_photo_transform_latency() {
    let mut worst = 0f64;
    for layered in [false, true] {
        let document = native_photo_document(layered);
        let mut r = WgpuRasterizer::new_native_headless(document.color).unwrap();
        native_submit(&mut r, &document.layers, true);
        r.wait_idle().unwrap();
        for _ in 0..3 {
            native_submit(&mut r, &document.layers, false);
            r.wait_idle().unwrap();
        }
        let label = if layered { "layered photo" } else { "photo" };
        let photo = document.layers[usize::from(layered)].id;
        worst = worst.max(native_photo_drag(&mut r, &document.layers, photo, label));
    }
    assert!(
        worst < 8.333,
        "a native photo transform drag exceeds the 120 Hz budget"
    );
}
