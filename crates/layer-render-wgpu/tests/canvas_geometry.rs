//! Canvas size changes are metadata: the composite after a crop is the old
//! composite's sub-rectangle, hidden pixels come back, and history is exact.
mod support;
use layer_core::*;
use layer_engine::{InputProducer, PenEvent};
use support::*;

fn geometry(origin: [i32; 2], size: [u32; 2]) -> CanvasGeometry {
    CanvasGeometry::crop(CanvasRect { origin, size })
}

/// Two painted layers across tile boundaries, the upper one masked by a
/// selection so its mask offset and initial coverage are exercised too.
fn painted() -> (Engine, InputProducer<PenEvent>) {
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let upper = add_paint(&mut doc, "Upper", 0);
    let mut mask = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), SIZE, Point::default());
    mask.source.default_coverage = 0.;
    mask.source.initial = Some(Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point { x: 300., y: 0. },
        Point { x: 300., y: 200. },
        Point { x: 0., y: 200. },
    ]).unwrap());
    doc.apply(mask_edit(&doc, upper, mask)).unwrap();
    let lower = doc.scene().order()[1];
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
    let lower = engine.document().scene().order()[1];
    engine.set_active_layer(lower).unwrap();
    draw(&mut engine, &mut input, [0.1, 0.7, 0.2, 1.], Point { x: 10., y: 10. }, Point { x: 330., y: 300. }, 5_000_000_000);
    let painted = image(&mut engine, 6_000_000_000);
    let corner = painted.crop([10, 10], [8, 8]);
    assert!(corner.rgba.chunks(4).all(|p| p[1] > p[0] && p[1] > p[2]), "the stroke starts in the new corner");
    assert_ne!(painted.crop([0, 0], [300, 280]).rgba, grown.crop([0, 0], [300, 280]).rgba);
    let archive = package_bytes(engine.document());
    let (mut reopened, _) = self::engine(reopen(&archive));
    image(&mut reopened, 0).assert_eq(&painted, "save and reopen");
    assert!(engine.undo().unwrap());
    image(&mut engine, 6_100_000_000).assert_eq(&grown, "undo the stroke");
    assert!(engine.undo().unwrap());
    image(&mut engine, 6_200_000_000).assert_eq(&original, "undo the growth");
}

#[test]
fn a_placed_photo_crops_as_metadata_and_never_rebases() {
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = doc.scene().order()[0];
    paint_mut(&mut doc, photo).original = Some(color::source::rgba8_source(SIZE, |x, y| {
        [(x * 5 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let (mut engine, _input) = engine(doc);
    let original = image(&mut engine, 0);
    engine.apply_canvas_geometry(&geometry([100, 60], [200, 120])).unwrap();
    image(&mut engine, 1_000_000_000).assert_eq(&original.crop([100, 60], [200, 120]), "photo crop");
    engine.apply_canvas_geometry(&geometry([-400, -300], [684, 556])).unwrap();
    let layer = engine.document().scene().occurrence(photo).unwrap();
    assert_eq!(layer.translation, Point { x: 300., y: 240. });
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
    let (mut blank_engine, _) = self::engine(Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }));
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
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let upper = add_paint(&mut doc, "Upper", 0);
    let paper = *doc.scene().order().last().unwrap();
    doc.apply(occurrence_edit(&doc, paper, |o| o.visible = false)).unwrap();
    let lower = doc.scene().order()[1];
    let (mut engine, input) = engine(doc);
    for (id, start, end, colors) in [
        (lower, Point { x: 0., y: 0. }, Point { x: SIZE[0] as f32, y: 0. }, [[0.9, 0.2, 0.05, 1.], [0.05, 0.3, 0.9, 1.]]),
        (upper, Point { x: 0., y: 0. }, Point { x: 0., y: SIZE[1] as f32 }, [[0.1, 0.8, 0.1, 0.6], [0.9, 0.9, 0.2, 0.2]]),
    ] {
        engine
            .append_raster_operation(engine.document().scene().source_target(id).unwrap(), RasterOperation {
                placement: Affine::IDENTITY,
                coverage: CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), SIZE, Point::default()),
                kind: RasterOperationKind::Gradient { start, end, colors, radial: false, alpha_locked: false },
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
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = doc.scene().order()[0];
    paint_mut(&mut doc, photo).original = Some(color::source::rgba8_source(SIZE, |x, y| {
        [(60 + x / 3) as u8, (40 + y / 2) as u8, (200 - (x + y) / 6) as u8, 255]
    }));
    let (mut engine, _input) = engine(doc);
    let original = image(&mut engine, 0);
    let source = paint(engine.document(), photo).original.clone().unwrap();
    let geometry = straightened(-0.15, false);
    engine.apply_canvas_geometry(&geometry).unwrap();
    let layer = paint(engine.document(), photo);
    assert!(std::sync::Arc::ptr_eq(layer.original.as_ref().unwrap(), &source), "the original is kept");
    assert!(layer.operations.is_empty(), "no pixel work");
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

/// The coordinates of a target's tiles in `plane`.
fn tiles(engine: &Engine, id: SourceTarget, plane: raster::RasterPlane) -> std::collections::BTreeSet<[u32; 2]> {
    let raster = engine.document().target_raster(id).unwrap().wait_data().unwrap();
    raster.tiles.keys().filter(|k| k.plane == plane).map(|k| k.coordinate).collect()
}

/// The pages of 256 pixels that `rect` touches.
fn pages_of(rect: Rect) -> std::collections::BTreeSet<[u32; 2]> {
    let [x0, y0] = [rect.min.x, rect.min.y].map(|v| (v.max(0.) / 256.).floor() as u32);
    let [x1, y1] = [rect.max.x, rect.max.y].map(|v| (v.max(0.) / 256.).ceil() as u32);
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| [x, y])).collect()
}

fn settled(engine: &mut Engine, time: u64) {
    engine.render_frame_at(time).unwrap();
    while engine.has_pending_document_edits() {
        std::thread::yield_now();
        engine.render_frame_at(time).unwrap();
    }
}

/// A 6000 × 4000 paint layer filled edge to edge, halved by Image Size,
/// keeps only the tiles of the halved canvas.
#[test]
fn resizing_a_whole_layer_leaves_no_vacated_tiles() {
    let extent = [6000, 4000];
    let doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let paint = doc.scene().source_target(doc.scene().order()[0]).unwrap();
    let (mut engine, _input) = engine(doc);
    engine
        .append_raster_operation(paint, RasterOperation {
            placement: Affine::IDENTITY,
            coverage: CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), extent, Point::default()),
            kind: RasterOperationKind::Fill { color: [0.2, 0.4, 0.8, 0.5], alpha_locked: false },
        })
        .unwrap();
    settled(&mut engine, 0);
    let full = tiles(&engine, paint, raster::RasterPlane::Color);
    assert_eq!(full.len(), 24 * 16);
    let half = [3000, 2000];
    engine.apply_canvas_geometry(&CanvasGeometry::resize(extent, half, Interpolation::Bicubic)).unwrap();
    settled(&mut engine, 1_000_000_000);
    let canvas = Rect { min: Point::default(), max: Point { x: half[0] as f32, y: half[1] as f32 } };
    assert_eq!(tiles(&engine, paint, raster::RasterPlane::Color), pages_of(canvas), "only the halved canvas keeps tiles");
    let rgba = engine.backend_mut().readback_srgb_rgba8().unwrap();
    let first = &rgba[..4];
    assert!(first[3] > 0 && rgba.chunks(4).all(|p| p == first), "the halved fill stays uniform to the edges");
    assert!(engine.undo().unwrap());
    settled(&mut engine, 1_100_000_000);
    assert_eq!(tiles(&engine, paint, raster::RasterPlane::Color), full, "undo restores every tile");
    assert!(engine.redo().unwrap());
    settled(&mut engine, 1_200_000_000);
    assert_eq!(tiles(&engine, paint, raster::RasterPlane::Color), pages_of(canvas), "redo prunes again");
}

/// A non-square drawing on a layer moved left of the canvas, with a mask.
fn oriented_fixture() -> (Engine, InputProducer<PenEvent>, OccurrenceHandle) {
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let upper = add_paint(&mut doc, "Upper", 0);
    let translation = Point { x: -256., y: 0. };
    let domain = [SIZE[0] + 256, SIZE[1]];
    paint_mut(&mut doc, upper).domain = domain;
    doc.apply(occurrence_edit(&doc, upper, |o| o.translation = translation)).unwrap();
    let mut mask = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), domain, translation);
    mask.source.default_coverage = 0.;
    mask.source.initial = Some(Selection::polygon(vec![
        Point { x: 256., y: 0. },
        Point { x: 556., y: 0. },
        Point { x: 556., y: 200. },
        Point { x: 256., y: 200. },
    ]).unwrap());
    doc.apply(mask_edit(&doc, upper, mask)).unwrap();
    let lower = doc.scene().order()[1];
    let (mut engine, mut input) = engine(doc);
    engine.set_active_layer(lower).unwrap();
    draw(&mut engine, &mut input, [0.8, 0.1, 0.05, 1.], Point { x: 20., y: 30. }, Point { x: 370., y: 230. }, 1_000_000_000);
    engine.set_active_layer(upper).unwrap();
    draw(&mut engine, &mut input, [0.05, 0.2, 0.9, 0.9], Point { x: 10., y: 220. }, Point { x: 360., y: 20. }, 2_000_000_000);
    draw(&mut engine, &mut input, [0.9, 0.8, 0.1, 0.7], Point { x: 5., y: 10. }, Point { x: 60., y: 250. }, 3_000_000_000);
    (engine, input, upper)
}

/// Every pixel of `after` is the pixel of `before` that `linear` moved there.
fn assert_permutation(after: &Image, before: &Image, linear: Affine, what: &str) {
    let back = linear.inverse().unwrap();
    for y in 0..after.size[1] {
        for x in 0..after.size[0] {
            let p = back.map(Point { x: x as f32 + 0.5, y: y as f32 + 0.5 });
            let [sx, sy] = [p.x.floor() as u32, p.y.floor() as u32];
            let i = ((y * after.size[0] + x) * 4) as usize;
            let j = ((sy * before.size[0] + sx) * 4) as usize;
            assert_eq!(after.rgba[i..i + 4], before.rgba[j..j + 4], "{what}: pixel {x}, {y} from {sx}, {sy}");
        }
    }
}

#[test]
fn flips_and_turns_move_every_pixel_exactly_in_one_undo_step() {
    use ImageOrientation::*;
    let (mut engine, _input, upper) = oriented_fixture();
    let paint_target = engine.document().scene().source_target(upper).unwrap();
    let mask = SourceTarget::Coverage(engine.document().scene().occurrence(upper).unwrap().mask.as_ref().unwrap().source);
    let original = image(&mut engine, 4_000_000_000);
    let mut time = 5_000_000_000;
    for orientation in [FlipHorizontal, FlipVertical, Rotate180, RotateRight, RotateLeft] {
        let canvas = engine.document().composition().size;
        let geometry = CanvasGeometry::orient(canvas, orientation);
        let before = [paint_target, mask].map(|id| (id, engine.document().clone()));
        engine.apply_canvas_geometry(&geometry).unwrap();
        assert!(engine.document().extents_cover_canvas());
        let oriented = image(&mut engine, time);
        assert_eq!(oriented.size, geometry.rect.size);
        assert_permutation(&oriented, &original, geometry.to_canvas(), &format!("{orientation:?}"));
        for (id, document) in before {
            let plane = if id == paint_target { raster::RasterPlane::Color } else { raster::RasterPlane::Mask };
            let old = document.scene().target_geometry(id).as_affine().unwrap();
            let new = engine.document().scene().target_geometry(id).as_affine().unwrap().inverse().unwrap();
            let extent = document.target_extent(id).map(|v| v as f32);
            let initial = match id { SourceTarget::Coverage(h) => document.scene().coverage(h).unwrap().initial.as_ref(), _ => None };
            let page = |[x, y]: [u32; 2]| Rect {
                min: Point { x: (x * 256) as f32, y: (y * 256) as f32 },
                max: Point { x: ((x + 1) * 256) as f32, y: ((y + 1) * 256) as f32 },
            };
            let held = document.target_raster(id).unwrap().wait_data().unwrap().tiles.keys()
                .filter(|k| k.plane == plane)
                .map(|k| k.coordinate)
                .chain(initial.into_iter().flat_map(|s| pages_of(s.bounds())))
                .map(page)
                .fold(Rect::EMPTY, Rect::union);
            let held = Rect { min: held.min, max: Point { x: held.max.x.min(extent[0]), y: held.max.y.min(extent[1]) } };
            let reach = pages_of(old.then(geometry.to_canvas()).then(new).bounds(held));
            let kept = tiles(&engine, id, plane);
            assert!(kept.is_subset(&reach), "{orientation:?}: {id:?} keeps tiles {kept:?} outside its content {reach:?}");
        }
        assert!(engine.undo().unwrap());
        image(&mut engine, time + 1_000_000).assert_eq(&original, &format!("{orientation:?}: one undo step"));
        assert!(engine.redo().unwrap());
        image(&mut engine, time + 2_000_000).assert_eq(&oriented, &format!("{orientation:?}: redo"));
        assert!(engine.undo().unwrap());
        image(&mut engine, time + 3_000_000).assert_eq(&original, &format!("{orientation:?}: undo again"));
        time += 1_000_000_000;
    }
    for (orientation, times) in [(FlipHorizontal, 2), (FlipVertical, 2), (Rotate180, 2), (RotateRight, 4), (RotateLeft, 4)] {
        for step in 0..times {
            let canvas = engine.document().composition().size;
            engine.apply_canvas_geometry(&CanvasGeometry::orient(canvas, orientation)).unwrap();
            settled(&mut engine, time + step);
        }
        image(&mut engine, time).assert_eq(&original, &format!("{orientation:?} {times} times"));
        time += 1_000_000_000;
    }
}
