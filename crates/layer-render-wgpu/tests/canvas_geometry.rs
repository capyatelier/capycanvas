//! Canvas size changes are metadata: the composite after a crop is the old
//! composite's sub-rectangle, hidden pixels come back, and history is exact.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;

type Engine = CanvasEngine<WgpuRasterizer>;
const SIZE: [u32; 2] = [384, 256];

struct Image {
    size: [u32; 2],
    rgba: Vec<u8>,
}
impl Image {
    fn crop(&self, origin: [u32; 2], size: [u32; 2]) -> Image {
        let mut rgba = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for y in origin[1]..origin[1] + size[1] {
            let start = ((y * self.size[0] + origin[0]) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + size[0] as usize * 4]);
        }
        Image { size, rgba }
    }
    fn assert_eq(&self, other: &Image, what: &str) {
        assert_eq!(self.size, other.size, "{what}: size");
        let first = self.rgba.iter().zip(&other.rgba).position(|(a, b)| a != b);
        assert_eq!(first.map(|i| [(i / 4) as u32 % self.size[0], (i / 4) as u32 / self.size[0]]), None, "{what}: first differing pixel");
    }
}

fn engine(document: Document) -> (Engine, InputProducer<PenEvent>) {
    let gpu = WgpuRasterizer::new_native_headless(document.color).expect("physical GPU required");
    let (producer, consumer) = input_queue(64);
    let view = ViewState {
        width_px: 1024,
        height_px: 768,
        document_to_surface: Affine::IDENTITY.0,
        background_rgba_linear: [0.; 4],
    };
    let mut engine = CanvasEngine::new(gpu, document, consumer, view, ViewTransform::IDENTITY).unwrap();
    engine.render_frame_at(0).unwrap();
    (engine, producer)
}

fn image(engine: &mut Engine, time: u64) -> Image {
    engine.render_frame_at(time).unwrap();
    while engine.has_pending_document_edits() {
        std::thread::yield_now();
        engine.render_frame_at(time).unwrap();
    }
    let size = [engine.document().width, engine.document().height];
    let mut rgba = vec![0; (size[0] * size[1] * 4) as usize];
    engine.backend_mut().copy_rgba8_srgb(&mut rgba, size[0] as usize * 4).unwrap();
    Image { size, rgba }
}

fn draw(engine: &mut Engine, input: &mut InputProducer<PenEvent>, color: [f32; 4], from: Point, to: Point, start: u64) {
    let mut brush = default_brush(DefaultBrushPreset::GPen);
    brush.diameter = 40.;
    brush.color_rgba_linear = color;
    engine.set_brush(brush).unwrap();
    for i in 0..10u64 {
        let t = i as f32 / 9.;
        let timestamp_ns = start + i * 8_000_000;
        input
            .push(PenEvent {
                device_id: 1,
                sequence: i,
                timestamp_ns,
                view_revision: 0,
                surface_position: Point { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t },
                pressure: 0.8,
                tilt_radians: [0., 0.],
                twist_radians: 0.,
                distance: 0.,
                phase: match i {
                    0 => PenPhase::Down,
                    9 => PenPhase::Up,
                    _ => PenPhase::Move,
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        engine.render_frame_at(timestamp_ns).unwrap();
        while engine.has_pending_input() {
            std::thread::yield_now();
            engine.render_frame_at(timestamp_ns).unwrap();
        }
    }
}

fn geometry(origin: [i32; 2], size: [u32; 2]) -> CanvasGeometry {
    CanvasGeometry::crop(CanvasRect { origin, size })
}

/// Two painted layers across tile boundaries, the upper one masked by a
/// selection so its mask offset and initial coverage are exercised too.
fn painted() -> (Engine, InputProducer<PenEvent>) {
    let mut doc = Document::new("canvas geometry", SIZE[0], SIZE[1]);
    let upper = doc.allocate_layer_id();
    let mut layer = Layer::paint(upper, "Upper");
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point::default());
    mask.default_coverage = 0.;
    mask.initial = Some(Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point { x: 300., y: 0. },
        Point { x: 300., y: 200. },
        Point { x: 0., y: 200. },
    ]).unwrap());
    layer.mask = Some(mask);
    doc.layers.insert(0, layer);
    let lower = doc.layers[1].id;
    let (mut engine, mut input) = engine(doc);
    engine.set_active_layer(lower).unwrap();
    draw(&mut engine, &mut input, [0.8, 0.1, 0.05, 1.], Point { x: 20., y: 30. }, Point { x: 370., y: 230. }, 1_000_000_000);
    engine.set_active_layer(upper).unwrap();
    draw(&mut engine, &mut input, [0.05, 0.2, 0.9, 0.9], Point { x: 10., y: 220. }, Point { x: 360., y: 20. }, 2_000_000_000);
    (engine, input)
}

#[test]
fn resize_reset_restores_every_layer_on_undo_and_redo() {
    let (mut engine, mut input) = painted();
    let original = image(&mut engine, 3_000_000_000);
    for (step, (origin, size)) in [([0, 0], [500, 300]), ([-300, -40], [700, 300])].into_iter().enumerate() {
        let time = 4_000_000_000 + step as u64 * 1_000_000_000;
        engine.apply_canvas_geometry(&geometry(origin, size)).unwrap();
        let grown = image(&mut engine, time);
        let at = [(-origin[0]) as u32, (-origin[1]) as u32];
        grown.crop(at, SIZE).assert_eq(&original, "grown canvas keeps the composite");
        assert!(engine.undo().unwrap());
        image(&mut engine, time + 1_000_000).assert_eq(&original, "undo restores every layer");
        assert!(engine.redo().unwrap());
        image(&mut engine, time + 2_000_000).assert_eq(&grown, "redo restores every layer");
        assert!(engine.undo().unwrap());
        image(&mut engine, time + 3_000_000).assert_eq(&original, "undo again");
    }
    draw(&mut engine, &mut input, [0.9, 0.9, 0.1, 1.], Point { x: 30., y: 200. }, Point { x: 350., y: 190. }, 7_000_000_000);
    let stroked = image(&mut engine, 8_000_000_000);
    engine.apply_canvas_geometry(&geometry([-10, 0], [400, 260])).unwrap();
    assert!(engine.undo().unwrap());
    image(&mut engine, 8_100_000_000).assert_eq(&stroked, "undo while the stroke may still be captured");
}

#[test]
fn a_crop_captures_the_source_subrectangle_and_hidden_pixels_return() {
    let (mut engine, _input) = painted();
    let original = image(&mut engine, 3_000_000_000);
    engine.apply_canvas_geometry(&geometry([50, 30], [200, 150])).unwrap();
    let cropped = image(&mut engine, 4_000_000_000);
    cropped.assert_eq(&original.crop([50, 30], [200, 150]), "crop");
    engine.apply_canvas_geometry(&geometry([-50, -30], SIZE)).unwrap();
    image(&mut engine, 5_000_000_000).assert_eq(&original, "growing back shows the hidden pixels");
    assert!(engine.undo().unwrap());
    image(&mut engine, 5_100_000_000).assert_eq(&cropped, "undo the growth");
    assert!(engine.undo().unwrap());
    image(&mut engine, 5_200_000_000).assert_eq(&original, "undo the crop");
    assert!(engine.redo().unwrap());
    image(&mut engine, 5_300_000_000).assert_eq(&cropped, "redo the crop");
}

#[test]
fn painting_reaches_a_new_strip_on_the_left_and_top() {
    let (mut engine, mut input) = painted();
    let original = image(&mut engine, 3_000_000_000);
    engine.apply_canvas_geometry(&geometry([-300, -280], [684, 536])).unwrap();
    assert!(engine.document().extents_cover_canvas());
    let grown = image(&mut engine, 4_000_000_000);
    grown.crop([300, 280], SIZE).assert_eq(&original, "grown");
    let lower = engine.document().layers[1].id;
    engine.set_active_layer(lower).unwrap();
    draw(&mut engine, &mut input, [0.1, 0.7, 0.2, 1.], Point { x: 10., y: 10. }, Point { x: 330., y: 300. }, 5_000_000_000);
    let painted = image(&mut engine, 6_000_000_000);
    let corner = painted.crop([10, 10], [8, 8]);
    assert!(corner.rgba.chunks(4).all(|p| p[1] > p[0] && p[1] > p[2]), "the stroke starts in the new corner");
    assert_ne!(painted.crop([0, 0], [300, 280]).rgba, grown.crop([0, 0], [300, 280]).rgba);
    let archive = {
        let mut bytes = Vec::new();
        Project::snapshot(engine.document()).unwrap().write(&mut bytes).unwrap();
        bytes
    };
    let (mut reopened, _) = self::engine(Project::read(archive.as_slice(), ProjectLimits::default()).unwrap().document);
    image(&mut reopened, 0).assert_eq(&painted, "save and reopen");
    assert!(engine.undo().unwrap());
    image(&mut engine, 6_100_000_000).assert_eq(&grown, "undo the stroke");
    assert!(engine.undo().unwrap());
    image(&mut engine, 6_200_000_000).assert_eq(&original, "undo the growth");
}

#[test]
fn a_placed_photo_crops_as_metadata_and_never_rebases() {
    let mut doc = Document::new("photo geometry", SIZE[0], SIZE[1]);
    doc.layers[0].source = Some(color::source::rgba8_source(SIZE, |x, y| {
        [(x * 5 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let photo = doc.layers[0].id;
    let (mut engine, _input) = engine(doc);
    let original = image(&mut engine, 0);
    engine.apply_canvas_geometry(&geometry([100, 60], [200, 120])).unwrap();
    image(&mut engine, 1_000_000_000).assert_eq(&original.crop([100, 60], [200, 120]), "photo crop");
    engine.apply_canvas_geometry(&geometry([-400, -300], [684, 556])).unwrap();
    let layer = engine.document().layer(photo).unwrap();
    assert_eq!(layer.properties.offset, Point { x: 300., y: 240. });
    let grown = image(&mut engine, 2_000_000_000);
    grown.crop([300, 240], SIZE).assert_eq(&original, "the whole photo is back");
    assert!(engine.undo().unwrap());
    assert!(engine.undo().unwrap());
    image(&mut engine, 2_100_000_000).assert_eq(&original, "undo both");
}

#[test]
fn deleting_cropped_pixels_leaves_nothing_hidden_to_reveal() {
    let (mut engine, _input) = painted();
    let original = image(&mut engine, 3_000_000_000);
    let (mut blank_engine, _) = self::engine(Document::new("blank", SIZE[0], SIZE[1]));
    let blank = image(&mut blank_engine, 0);
    let delete = CanvasGeometry { delete_outside: true, ..geometry([50, 30], [200, 150]) };
    engine.apply_canvas_geometry(&delete).unwrap();
    assert!(engine.document().extents_cover_canvas());
    let cropped = image(&mut engine, 4_000_000_000);
    cropped.assert_eq(&original.crop([50, 30], [200, 150]), "crop");
    engine.apply_canvas_geometry(&geometry([-50, -30], SIZE)).unwrap();
    let grown = image(&mut engine, 5_000_000_000);
    grown.crop([50, 30], [200, 150]).assert_eq(&cropped, "the kept pixels");
    for y in 0..SIZE[1] {
        for x in 0..SIZE[0] {
            if (50..250).contains(&x) && (30..180).contains(&y) {
                continue;
            }
            let i = ((y * SIZE[0] + x) * 4) as usize;
            assert_eq!(grown.rgba[i..i + 4], blank.rgba[i..i + 4], "no hidden pixel returns at {x}, {y}");
        }
    }
    assert!(engine.undo().unwrap());
    assert!(engine.undo().unwrap());
    image(&mut engine, 5_100_000_000).assert_eq(&original, "one undo step restores every pixel");
    assert!(engine.redo().unwrap());
    image(&mut engine, 5_200_000_000).assert_eq(&cropped, "redo deletes again");
}

/// Two smooth gradients, so interpolation differs from the oracle by less
/// than a code, with the paper hidden so coverage shows in alpha.
fn gradients() -> (Engine, InputProducer<PenEvent>) {
    let mut doc = Document::new("straighten", SIZE[0], SIZE[1]);
    let upper = doc.allocate_layer_id();
    doc.layers.insert(0, Layer::paint(upper, "Upper"));
    doc.layers.last_mut().unwrap().visible = false;
    let lower = doc.layers[1].id;
    let (mut engine, input) = engine(doc);
    for (id, start, end, colors) in [
        (lower, Point { x: 0., y: 0. }, Point { x: SIZE[0] as f32, y: 0. }, [[0.9, 0.2, 0.05, 1.], [0.05, 0.3, 0.9, 1.]]),
        (upper, Point { x: 0., y: 0. }, Point { x: 0., y: SIZE[1] as f32 }, [[0.1, 0.8, 0.1, 0.6], [0.9, 0.9, 0.2, 0.2]]),
    ] {
        engine
            .append_layer_operation(id, LayerOperation {
                placement: Affine::IDENTITY,
                coverage: LayerMask::reveal_all(LayerId(900 + id.0), Point::default()),
                kind: LayerOperationKind::Gradient { start, end, colors, radial: false, alpha_locked: false },
            })
            .unwrap();
        image(&mut engine, 1_000_000);
    }
    (engine, input)
}

/// The largest `SIZE`-shaped crop turned by `angle` about the canvas centre.
fn straightened(angle: f32, delete_outside: bool) -> CanvasGeometry {
    let [w, h] = SIZE.map(|v| v as f32);
    let aspect = w / h;
    let (s, c) = (angle.sin().abs(), angle.cos().abs());
    let height = (w / (aspect * c + s)).min(h / (aspect * s + c));
    let size = [(aspect * height).floor() as u32, height.floor() as u32];
    let center = Point { x: w / 2., y: h / 2. };
    CanvasGeometry {
        rect: CanvasRect { origin: [0, 1].map(|i| ([center.x, center.y][i] - size[i] as f32 / 2.).round() as i32), size },
        linear: Affine::around(center, [1., 1.], -angle, Point::default()),
        interpolation: Interpolation::Bicubic,
        delete_outside,
    }
}

/// Bilinear CPU sample of an 8-bit image at pixel-centre coordinates.
fn bilinear(image: &Image, p: Point) -> [f32; 4] {
    let (x, y) = (p.x - 0.5, p.y - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let texel = |xi: f32, yi: f32| {
        let xi = xi.clamp(0., image.size[0] as f32 - 1.) as u32;
        let yi = yi.clamp(0., image.size[1] as f32 - 1.) as u32;
        let i = ((yi * image.size[0] + xi) * 4) as usize;
        std::array::from_fn::<f32, 4, _>(|c| image.rgba[i + c] as f32)
    };
    let [a, b, c, d] = [texel(x0, y0), texel(x0 + 1., y0), texel(x0, y0 + 1.), texel(x0 + 1., y0 + 1.)];
    std::array::from_fn(|k| (a[k] * (1. - tx) + b[k] * tx) * (1. - ty) + (c[k] * (1. - tx) + d[k] * tx) * ty)
}

fn assert_rotation(result: &Image, original: &Image, to_canvas: Affine, tolerance: f32, what: &str) {
    let back = to_canvas.inverse().unwrap();
    let mut worst = 0f32;
    for y in 2..result.size[1] - 2 {
        for x in 2..result.size[0] - 2 {
            let expected = bilinear(original, back.map(Point { x: x as f32 + 0.5, y: y as f32 + 0.5 }));
            let i = ((y * result.size[0] + x) * 4) as usize;
            for (c, value) in expected.iter().enumerate() {
                worst = worst.max((result.rgba[i + c] as f32 - value).abs());
            }
        }
    }
    assert!(worst <= tolerance, "{what}: {worst} codes from the CPU rotation");
}

#[test]
fn straightening_matches_a_cpu_rotation_and_keeps_the_hidden_corners() {
    let (mut engine, _input) = gradients();
    let original = image(&mut engine, 2_000_000_000);
    let angle = 0.2;
    let geometry = straightened(angle, false);
    let to_canvas = geometry.to_canvas();
    engine.apply_canvas_geometry(&geometry).unwrap();
    assert!(engine.document().extents_cover_canvas());
    let straight = image(&mut engine, 3_000_000_000);
    assert_eq!(straight.size, geometry.rect.size);
    assert_rotation(&straight, &original, to_canvas, 2., "straightened");
    let margin = 120;
    let [w, h] = geometry.rect.size;
    engine.apply_canvas_geometry(&self::geometry([-margin, -margin], [w + 2 * margin as u32, h + 2 * margin as u32])).unwrap();
    let revealed = image(&mut engine, 4_000_000_000);
    let alpha = |p: Point| {
        let [x, y] = [p.x, p.y].map(|v| (v + margin as f32) as u32);
        revealed.rgba[((y * revealed.size[0] + x) * 4 + 3) as usize]
    };
    let [right, bottom] = SIZE.map(|v| v as f32 - 8.);
    for corner in [Point { x: 8., y: 8. }, Point { x: right, y: 8. }, Point { x: 8., y: bottom }, Point { x: right, y: bottom }] {
        let at = to_canvas.map(corner);
        assert!(at.x < 0. || at.y < 0. || at.x > w as f32 || at.y > h as f32, "{at:?} is hidden after the crop");
        assert_eq!(alpha(at), 255, "the hidden corner {corner:?} is kept");
    }
    assert_eq!(alpha(to_canvas.map(Point { x: -20., y: SIZE[1] as f32 / 2. })), 0, "nothing beyond the old canvas");
    assert!(engine.undo().unwrap());
    image(&mut engine, 4_100_000_000).assert_eq(&straight, "undo the growth");
    assert!(engine.undo().unwrap());
    image(&mut engine, 4_200_000_000).assert_eq(&original, "one undo step restores the drawing");
}

#[test]
fn straightening_turns_a_placed_photo_without_resampling_its_pixels() {
    let mut doc = Document::new("photo straighten", SIZE[0], SIZE[1]);
    doc.layers[0].source = Some(color::source::rgba8_source(SIZE, |x, y| {
        [(60 + x / 3) as u8, (40 + y / 2) as u8, (200 - (x + y) / 6) as u8, 255]
    }));
    let photo = doc.layers[0].id;
    let (mut engine, _input) = engine(doc);
    let original = image(&mut engine, 0);
    let source = engine.document().layer(photo).unwrap().source.clone().unwrap();
    let geometry = straightened(-0.15, false);
    engine.apply_canvas_geometry(&geometry).unwrap();
    let layer = engine.document().layer(photo).unwrap();
    assert!(std::sync::Arc::ptr_eq(layer.source.as_ref().unwrap(), &source), "the original is kept");
    assert!(layer.pending_operations.is_empty(), "no pixel work");
    let straight = image(&mut engine, 1_000_000_000);
    assert_rotation(&straight, &original, geometry.to_canvas(), 2., "placed photo");
}

#[test]
fn growing_past_a_filled_edge_adds_transparent_canvas() {
    let (mut engine, _input) = gradients();
    let original = image(&mut engine, 2_000_000_000);
    engine.apply_canvas_geometry(&geometry([0, 0], [600, 400])).unwrap();
    let grown = image(&mut engine, 3_000_000_000);
    grown.crop([0, 0], SIZE).assert_eq(&original, "the drawing");
    for (x, y) in [(SIZE[0], 10), (500, 100), (100, SIZE[1]), (599, 399)] {
        assert_eq!(grown.rgba[((y * 600 + x) * 4 + 3) as usize], 0, "transparent at {x}, {y}");
    }
}

/// Timing, not a gate: `cargo test --release -p layer-render-wgpu --test
/// canvas_geometry -- --ignored --nocapture`.
#[test]
#[ignore = "24 MP timing report"]
fn canvas_geometry_timing_on_a_24_megapixel_photo() {
    let extent = [6000, 4000];
    let mut doc = Document::new("24 MP geometry", extent[0], extent[1]);
    doc.layers[0].source = Some(color::source::rgba8_source(extent, |x, y| {
        [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let paint = doc.allocate_layer_id();
    doc.layers.insert(0, Layer::paint(paint, "Paint"));
    let gpu = WgpuRasterizer::new_native_headless(doc.color).expect("physical GPU required");
    let (_producer, consumer) = input_queue(64);
    let view = ViewState { width_px: 1600, height_px: 1000, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.], background_rgba_linear: [0.; 4] };
    let mut engine = CanvasEngine::new(gpu, doc, consumer, view, ViewTransform::IDENTITY).unwrap();
    engine
        .append_layer_operation(paint, LayerOperation {
            placement: Affine::IDENTITY,
            coverage: LayerMask::reveal_all(LayerId(900), Point::default()),
            kind: LayerOperationKind::Fill { color: [0.2, 0.4, 0.8, 0.5], alpha_locked: false },
        })
        .unwrap();
    let settle = |engine: &mut Engine| {
        let start = std::time::Instant::now();
        engine.render_frame_at(0).unwrap();
        while engine.has_pending_document_edits() {
            engine.render_frame_at(0).unwrap();
        }
        engine.backend_mut().wait_idle().unwrap();
        start.elapsed()
    };
    settle(&mut engine);
    let tiles = engine.document().layer(paint).unwrap().raster.wait_data().unwrap().tiles.len();
    for tile in engine.document().layer(paint).unwrap().raster.wait_data().unwrap().tiles.values() {
        tile.wait_backing().unwrap();
    }
    println!("24 MP photo with a {tiles}-tile paint layer");
    for (label, origin, size) in [
        ("crop to 4000 × 2400", [1000, 800], [4000, 2400]),
        ("canvas size back to 6000 × 4000", [-1000, -800], [6000, 4000]),
        ("canvas size +300 left and top (rebase)", [-300, -300], [6300, 4300]),
    ] {
        let start = std::time::Instant::now();
        engine.apply_canvas_geometry(&geometry(origin, size)).unwrap();
        let apply = start.elapsed();
        let frame = settle(&mut engine);
        println!("{label}: apply {:.2} ms, first frame to GPU idle {:.1} ms", apply.as_secs_f64() * 1e3, frame.as_secs_f64() * 1e3);
    }
    let start = std::time::Instant::now();
    engine.undo().unwrap();
    let undo = start.elapsed();
    let frame = settle(&mut engine);
    println!("undo the rebase: {:.2} ms, first frame to GPU idle {:.1} ms", undo.as_secs_f64() * 1e3, frame.as_secs_f64() * 1e3);
}

