//! Continuous transform drags over 24-megapixel paint and photo layers: a new
//! moving transform every frame, then the frames that release it, with CPU
//! submission, GPU execution and serialized completion kept apart.
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth, source::*};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Affine, ImageTransform, Interpolation, MeshMap, Projective, TransformMap};
use std::time::Instant;

const EXTENT: [u32; 2] = [6000, 4000];
const FRAMES: usize = 200;
const WARMUP: usize = 40;

type Case = (&'static str, fn(f32) -> TransformMap, Interpolation);

fn bounds() -> layer_core::Rect {
    layer_core::Rect {
        min: Point::default(),
        max: Point { x: EXTENT[0] as f32, y: EXTENT[1] as f32 },
    }
}

fn center() -> Point {
    Point {
        x: EXTENT[0] as f32 * 0.5,
        y: EXTENT[1] as f32 * 0.5,
    }
}

fn affine(t: f32) -> TransformMap {
    TransformMap::Affine(Affine::around(
        center(),
        [0.9 + t.sin() * 0.05; 2],
        0.1 + t.cos() * 0.02,
        Point {
            x: t.sin() * 40.,
            y: t.cos() * 30.,
        },
    ))
}

fn perspective(t: f32, depth: f32) -> TransformMap {
    let [w, h] = EXTENT.map(|v| v as f32);
    let inset = w * depth * 0.5;
    let quad = [
        [inset + t.sin() * 60., 150.],
        [w - inset + t.cos() * 40., 100. + t.sin() * 50.],
        [w - 100., h - 80.],
        [120. + t.cos() * 30., h - 60.],
    ];
    TransformMap::Projective(Projective::rect_to_quad(bounds(), quad.map(|[x, y]| Point { x, y })).unwrap())
}

/// A mesh seeded from a keystone with two nodes dragged, as a Warp drag moves
/// one handle every frame.
fn warp(t: f32, cells: [u16; 2]) -> TransformMap {
    let [w, h] = EXTENT.map(|v| v as f32);
    let keystone = Projective::rect_to_quad(
        bounds(),
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
    let mesh = MeshMap::from_projective(bounds(), cells, &keystone)
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
    TransformMap::Mesh(std::sync::Arc::new(mesh))
}

const CASES: [Case; 9] = {
    use Interpolation::*;
    [
        ("affine bilinear", affine, Linear),
        ("perspective bilinear", |t| perspective(t, 0.3), Linear),
        ("deep perspective bilinear", |t| perspective(t, 0.9), Linear),
        ("affine bicubic", affine, Bicubic),
        ("perspective bicubic", |t| perspective(t, 0.3), Bicubic),
        ("deep perspective bicubic", |t| perspective(t, 0.9), Bicubic),
        (
            "quarter scale bicubic",
            |t| TransformMap::Affine(Affine::around(center(), [0.25 + t.sin() * 0.01; 2], t.cos() * 0.02, Point::default())),
            Bicubic,
        ),
        ("3x3 warp bicubic", |t| warp(t, [3, 3]), Bicubic),
        ("5x5 warp bicubic", |t| warp(t, [5, 5]), Bicubic),
    ]
};

/// Free drags translate the selected photo; Distort drags one corner; Warp
/// drags one node.
const NATIVE_CASES: [Case; 3] = {
    use Interpolation::Bicubic;
    [
        ("free", |t| TransformMap::Affine(Affine::translation(corner(t))), Bicubic),
        (
            "distort",
            |t| {
                let [w, h] = EXTENT.map(|v| v as f32);
                let corners = [corner(t), Point { x: w, y: 0. }, Point { x: w, y: h }, Point { x: 0., y: h }];
                TransformMap::Projective(Projective::rect_to_quad(bounds(), corners).unwrap())
            },
            Bicubic,
        ),
        (
            "warp",
            |t| {
                let node = Point { x: EXTENT[0] as f32 * 0.08 * t.sin(), y: EXTENT[1] as f32 * 0.06 * (t * 1.3).sin() };
                TransformMap::Mesh(std::sync::Arc::new(MeshMap::identity(bounds(), [4, 4]).unwrap().move_node(6, node).unwrap()))
            },
            Bicubic,
        ),
    ]
};

/// Where a native drag moves the photo's top-left corner.
fn corner(t: f32) -> Point {
    Point { x: EXTENT[0] as f32 * 0.17 * t.sin(), y: EXTENT[1] as f32 * 0.13 * (t * 1.3).sin() }
}

/// A drag of `layer` among `layers`, shown in `view`, `step` apart per frame.
/// A native drag first draws until the transform is prepared and times every
/// frame of its release; otherwise only the first release frame is timed.
struct Workload<'a> {
    label: &'a str,
    layers: &'a [Layer],
    layer: LayerId,
    view: ViewState,
    step: f32,
    native: bool,
}

fn submit(r: &mut WgpuRasterizer, work: &Workload<'_>, reset: bool) {
    r.submit(FramePacket {
        view: work.view,
        reset_layers: reset,
        composite_all: reset,
        ..packet(work.layers, EXTENT)
    })
    .unwrap();
}

/// Milliseconds of each frame showing `preview`, until the renderer has no
/// pending work or after `limit` frames.
fn frames(r: &mut WgpuRasterizer, work: &Workload<'_>, preview: &layer_render::TransformPreview, limit: usize) -> Vec<f64> {
    let mut times = Vec::new();
    loop {
        let start = Instant::now();
        r.set_transform_preview(Some(preview)).unwrap();
        submit(r, work, false);
        r.wait_idle().unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.);
        if !r.has_pending_work() || times.len() >= limit {
            return times;
        }
    }
}

fn percentiles(mut values: Vec<f64>) -> [f64; 3] {
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 0.99].map(|p| values[(values.len() as f64 * p).ceil() as usize - 1])
}

/// Drag each case's transform across FRAMES frames of one transaction.
fn drag(r: &mut WgpuRasterizer, work: &Workload<'_>, cases: &[Case]) -> f64 {
    let [w, h] = EXTENT.map(|v| v as f32);
    let selection = Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point { x: w, y: 0. },
        Point { x: w, y: h },
        Point { x: 0., y: h },
    ])
    .unwrap();
    let label = work.label;
    let mut worst = 0f64;
    for (transaction, &(name, map, interpolation)) in cases.iter().enumerate() {
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        r.telemetry = telemetry::Telemetry::new(r.device(), r.queue());
        r.set_telemetry_enabled(true);
        let mut preview = layer_render::TransformPreview {
            transaction: transaction as u64 + 1,
            moving: true,
            layer: work.layer,
            selection: Some(selection.clone()),
            transform: ImageTransform { map: map(0.), interpolation, ..Default::default() },
        };
        if work.native {
            let still = layer_render::TransformPreview {
                moving: false,
                transform: ImageTransform::default(),
                ..preview.clone()
            };
            let start = frames(r, work, &still, 65);
            eprintln!(
                "{label} {name}: transform start {} frames, {:.3}ms in all, longest {:.3}ms",
                start.len(),
                start.iter().sum::<f64>(),
                start.iter().copied().fold(0f64, f64::max)
            );
        }
        for i in 0..FRAMES {
            preview.transform.map = map(i as f32 * work.step);
            let start = Instant::now();
            r.set_transform_preview(Some(&preview)).unwrap();
            submit(r, work, false);
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
        let release = frames(r, work, &preview, if work.native { usize::MAX } else { 1 });
        let settled = release.iter().copied().fold(0f64, f64::max);
        eprintln!(
            "{label} {name} 6000x4000, full selection: CPU submit p50/p95/p99 {cpu:.3?}ms, GPU execution {gpu:.3?}ms, completion {completed:.3?}ms; release {} frames, longest {settled:.3}ms",
            release.len()
        );
        worst = worst.max(completed[2]).max(settled);
        r.set_transform_preview(None).unwrap();
        submit(r, work, false);
        r.wait_idle().unwrap();
    }
    worst
}

/// The drag of a lone 24-megapixel layer fitted to a 1920x1080 view.
fn fitted_drag(r: &mut WgpuRasterizer, layer: &Layer, label: &str) -> f64 {
    let zoom = (1920. / EXTENT[0] as f32).min(1080. / EXTENT[1] as f32);
    let work = Workload {
        label,
        layers: std::slice::from_ref(layer),
        layer: layer.id,
        view: ViewState { width_px: 1920, height_px: 1080, document_to_surface: [zoom, 0., 0., zoom, 0., 0.], ..view() },
        step: 0.04,
        native: false,
    };
    submit(r, &work, true);
    r.wait_idle().unwrap();
    drag(r, &work, &CASES)
}

/// A 16-bit photo of `pixel` colors in `space`.
fn u16_source(space: RgbSpace, pixel: impl Fn(u32, u32) -> [u16; 4]) -> std::sync::Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(space),
            profile_assumed: false,
        },
        512 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let row: Vec<_> = (0..EXTENT[0]).flat_map(|x| pixel(x, y)).flat_map(u16::to_le_bytes).collect();
        builder.push_row(&row).unwrap();
    }
    std::sync::Arc::new(builder.finish().unwrap())
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
    let worst = fitted_drag(&mut r, &layer, "paint");
    assert!(worst < 8.333, "a transform drag exceeds the 120 Hz budget");
}

#[test]
#[ignore = "hardware 24-megapixel photo transform benchmark; release, serial"]
fn large_photo_transform_latency() {
    let mut layer = Layer::paint(LayerId(1), "large photo transform");
    layer.source = Some(u16_source(RgbSpace::Srgb, |x, y| {
        [
            ((x * 8191 + y * 31) % 65536) as u16,
            ((x * 17 + y * 16381) % 65536) as u16,
            ((x / 173 + y / 111) % 2 * 65535) as u16,
            65535,
        ]
    }));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let worst = fitted_drag(&mut r, &layer, "photo");
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
    document.layers[0].source = Some(u16_source(RgbSpace::ProPhoto, |x, y| {
        [
            ((x * 55000 / EXTENT[0] + (x * 7 + y * 13) % 1024) % 65536) as u16,
            ((y * 55000 / EXTENT[1] + (x * 11 + y * 5) % 1024) % 65536) as u16,
            (((x + y) * 13 % 60000) + (x ^ y) % 1024) as u16,
            65535,
        ]
    }));
    if layered {
        let mut strokes = Layer::paint(document.allocate_layer_id(), "strokes");
        strokes.source = Some(u16_source(RgbSpace::ProPhoto, |x, y| {
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
        backdrop.source = Some(u16_source(RgbSpace::ProPhoto, |x, y| {
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

#[test]
#[ignore = "hardware 24-megapixel native photo transform benchmark; release, serial"]
fn native_photo_transform_latency() {
    let [width, height] = [1600u32, 1000];
    let zoom = (width as f32 / EXTENT[0] as f32).min(height as f32 / EXTENT[1] as f32) * 0.95;
    let offset = [
        (width as f32 - EXTENT[0] as f32 * zoom) * 0.5,
        (height as f32 - EXTENT[1] as f32 * zoom) * 0.5,
    ];
    let mut worst = 0f64;
    for layered in [false, true] {
        let document = native_photo_document(layered);
        let work = Workload {
            label: if layered { "layered photo" } else { "photo" },
            layers: &document.layers,
            layer: document.layers[usize::from(layered)].id,
            view: ViewState { width_px: width, height_px: height, document_to_surface: [zoom, 0., 0., zoom, offset[0], offset[1]], ..view() },
            step: 1. / 60.,
            native: true,
        };
        let mut r = WgpuRasterizer::new_native_headless(document.color).unwrap();
        submit(&mut r, &work, true);
        r.wait_idle().unwrap();
        for _ in 0..3 {
            submit(&mut r, &work, false);
            r.wait_idle().unwrap();
        }
        worst = worst.max(drag(&mut r, &work, &NATIVE_CASES));
    }
    assert!(
        worst < 8.333,
        "a native photo transform drag exceeds the 120 Hz budget"
    );
}
