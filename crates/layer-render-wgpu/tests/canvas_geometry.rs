//! Canvas size changes are metadata: the composite after a crop is the old
//! composite's sub-rectangle, hidden pixels come back, and history is exact.
mod support;
use layer_core::*;
use layer_engine::{InputProducer, PenEvent};
use support::*;
use support::Image;

fn geometry(origin: [i32; 2], size: [u32; 2]) -> CanvasGeometry {
    CanvasGeometry::crop(CanvasRect { origin, size })
}

/// Two painted layers across tile boundaries, the upper one masked by a
/// selection so its mask offset and initial coverage are exercised too.
fn painted() -> (Engine, InputProducer<PenEvent>) {
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let upper = add_paint(&mut doc, "Upper", 0);
    let lower = doc.scene().order()[1];
    let (mut engine, mut input) = engine(doc);
    add_selection_mask(&mut engine, upper, Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point { x: 300., y: 0. },
        Point { x: 300., y: 200. },
        Point { x: 0., y: 200. },
    ]).unwrap(), 0.);
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

/// A plan's photo moves, finished by the renderer's snapshot worker.
fn remapped(engine: &mut Engine, plan: CanvasGeometryPlan) -> CanvasGeometryPlan {
    let Some(remap) = plan.remap_plan(engine.document()) else { return plan };
    let results = pollster::block_on(engine.backend().snapshot_gpu().remap(remap, Default::default())).unwrap();
    plan.with_remapped(results).unwrap()
}

/// A canvas change, with any photo moves finished first as the snapshot
/// worker would.
fn prepared(engine: &mut Engine, geometry: &CanvasGeometry) {
    let plan = engine.canvas_geometry_plan(geometry).unwrap();
    let plan = remapped(engine, plan);
    engine.apply_canvas_plan(plan, Vec::new()).unwrap();
}

/// A 300 × 200 photo at (40, 30) on the paint layer, with a stroke across it.
fn edited_photo_with(pixel: impl Fn(u32, u32) -> [u8; 4]) -> (Engine, InputProducer<PenEvent>, OccurrenceHandle) {
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = doc.scene().order()[0];
    let mut base = layer_core::authored::PaintBase::new(color::source::rgba8_source([300, 200], pixel).into());
    base.offset = [40, 30];
    paint_mut(&mut doc, photo).base = Some(base);
    let (mut engine, mut input) = engine(doc);
    draw(&mut engine, &mut input, [0.05, 0.6, 0.2, 0.8], Point { x: 20., y: 200. }, Point { x: 330., y: 60. }, 1_000_000_000);
    (engine, input, photo)
}
fn edited_photo() -> (Engine, InputProducer<PenEvent>, OccurrenceHandle) {
    edited_photo_with(|x, y| [(x * 5 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8, 255])
}

#[test]
fn a_photo_crops_as_metadata_and_grows_with_its_base_into_paintable_canvas() {
    let (mut engine, mut input, photo) = edited_photo();
    let original = image(&mut engine, 2_000_000_000);
    let source = paint(engine.document(), photo).base.clone().unwrap();
    engine.apply_canvas_geometry(&geometry([100, 60], [200, 120])).unwrap();
    image(&mut engine, 3_000_000_000).assert_eq(&original.crop([100, 60], [200, 120]), "photo crop");
    engine.apply_canvas_geometry(&geometry([-400, -300], [684, 556])).unwrap();
    let base = paint(engine.document(), photo).base.clone().unwrap();
    assert!(base.image.same_owner(&source.image), "growing rebases the photo without copying it");
    assert_eq!(base.offset.map(|v| v % 256), source.offset.map(|v| v % 256), "the photo moves by whole tiles");
    let grown = image(&mut engine, 4_000_000_000);
    grown.crop([300, 240], SIZE).assert_eq(&original, "the whole photo is back");
    draw(&mut engine, &mut input, [0.9, 0.1, 0.1, 1.], Point { x: 20., y: 40. }, Point { x: 20., y: 200. }, 5_000_000_000);
    let painted = image(&mut engine, 6_000_000_000);
    let at = ((120 * painted.size[0] + 20) * 4) as usize;
    assert_ne!(painted.rgba[at..at + 4], grown.rgba[at..at + 4], "the new strip beside the photo takes paint");
    for _ in 0..3 { assert!(engine.undo().unwrap()); }
    image(&mut engine, 6_100_000_000).assert_eq(&original, "undo every step");
}

#[test]
fn turning_an_edited_photo_moves_every_sample_and_stroke_exactly() {
    use ImageOrientation::*;
    let (mut engine, _input, photo) = edited_photo();
    let source = paint(engine.document(), photo).base.clone().unwrap();
    let mut object = layer_core::ImageObject::new(source.image.clone());
    object.affine = layer_core::Affine64([1., 0., 0., 1., 8., 210.]);
    let (_, added) = engine.document().create_object_layer_edit("Reference", object, None, 0).unwrap();
    engine.apply_edit(added).unwrap();
    let original = image(&mut engine, 2_000_000_000);
    let mut time = 3_000_000_000;
    for orientation in [FlipHorizontal, FlipVertical, Rotate180, RotateRight, RotateLeft] {
        let geometry = CanvasGeometry::orient(engine.document().composition().size, orientation);
        prepared(&mut engine, &geometry);
        assert!(engine.document().extents_cover_canvas());
        let base = paint(engine.document(), photo).base.clone().unwrap();
        assert_eq!(base.image.interpretation, source.image.interpretation, "{orientation:?}: the photo keeps its own samples");
        assert!(!base.image.same_owner(&source.image), "{orientation:?}: a turned photo is a new image");
        let shared = engine.document().artwork.objects.iter().next().unwrap().2.image.clone();
        assert!(shared.same_owner(&source.image), "{orientation:?}: other users keep the original image");
        let oriented = image(&mut engine, time);
        assert_permutation(&oriented, &original, geometry.to_canvas(), &format!("{orientation:?}"));
        assert!(engine.undo().unwrap());
        image(&mut engine, time + 1_000_000).assert_eq(&original, &format!("{orientation:?}: one undo step"));
        assert!(engine.redo().unwrap());
        image(&mut engine, time + 2_000_000).assert_eq(&oriented, &format!("{orientation:?}: redo"));
        assert!(engine.undo().unwrap());
        time += 1_000_000_000;
    }
    for _ in 0..4 {
        let geometry = CanvasGeometry::orient(engine.document().composition().size, RotateRight);
        prepared(&mut engine, &geometry);
        settled(&mut engine, time);
    }
    image(&mut engine, time).assert_eq(&original, "four turns");
}

#[test]
fn deleting_cropped_pixels_cuts_a_photo_to_its_kept_samples() {
    let (mut engine, mut input, photo) = edited_photo();
    let original = image(&mut engine, 2_000_000_000);
    let (mut blank_engine, _) = self::engine(Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }));
    let blank = image(&mut blank_engine, 0);
    let source = paint(engine.document(), photo).base.clone().unwrap();
    prepared(&mut engine, &CanvasGeometry { delete_outside: true, ..geometry([60, 50], [200, 120]) });
    let base = paint(engine.document(), photo).base.clone().unwrap();
    assert_eq!(base.image.extent, [200, 120], "only the kept photo samples remain");
    assert_eq!(base.image.interpretation, source.image.interpretation);
    let cropped = image(&mut engine, 3_000_000_000);
    cropped.assert_eq(&original.crop([60, 50], [200, 120]), "crop");
    engine.apply_canvas_geometry(&geometry([-60, -50], SIZE)).unwrap();
    let grown = image(&mut engine, 4_000_000_000);
    grown.crop([60, 50], [200, 120]).assert_eq(&cropped, "the kept pixels");
    for y in (0..SIZE[1]).step_by(3) {
        for x in (0..SIZE[0]).step_by(3) {
            if (60..260).contains(&x) && (50..170).contains(&y) { continue; }
            let i = ((y * SIZE[0] + x) * 4) as usize;
            assert_eq!(grown.rgba[i..i + 4], blank.rgba[i..i + 4], "no photo or paint returns at {x}, {y}");
        }
    }
    draw(&mut engine, &mut input, [0.9, 0.1, 0.1, 1.], Point { x: 20., y: 40. }, Point { x: 20., y: 200. }, 5_000_000_000);
    let painted = image(&mut engine, 6_000_000_000);
    let at = ((120 * SIZE[0] + 20) * 4) as usize;
    assert_ne!(painted.rgba[at..at + 4], blank.rgba[at..at + 4], "paint lands beside the cut photo");
    for _ in 0..3 { assert!(engine.undo().unwrap()); }
    image(&mut engine, 6_100_000_000).assert_eq(&original, "one undo step per change restores the photo");
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
                coverage: CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), SIZE, [0, 0]),
                kind: RasterOperationKind::Gradient { start, end, gradient:layer_core::GradientDefinition {stops:colors.into_iter().enumerate().map(|(i,rgba)|layer_core::GradientStop {position:i as f32,color:layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb,rgba).unwrap()}).collect(),interpolation:layer_core::ColorMixSpace::LinearRgb},shape:layer_core::GradientShape::Linear,reverse:false,opacity:1.,alpha_locked:false },
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
        linear: CanvasGeometry::rotation([f64::from(center.x), f64::from(center.y)], -f64::from(angle)),
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
fn straightening_folds_an_edited_photo_into_paint_once() {
    let (mut engine, _input, photo) = edited_photo_with(|x, y| [(60 + x / 3) as u8, (40 + y / 2) as u8, (200 - (x + y) / 6) as u8, 255]);
    let original = image(&mut engine, 2_000_000_000);
    let geometry = straightened(-0.15, false);
    engine.apply_canvas_geometry(&geometry).unwrap();
    let straight = image(&mut engine, 3_000_000_000);
    assert!(paint(engine.document(), photo).base.is_none(), "the resampled photo becomes the layer's paint");
    let to_canvas = geometry.to_canvas();
    for p in [Point { x: 120.5, y: 80.5 }, Point { x: 250.5, y: 170.5 }, Point { x: 175.5, y: 130.5 }, Point { x: 140.5, y: 146.5 }] {
        let at = to_canvas.map(p);
        assert!(at.x >= 0. && at.y >= 0. && (at.x as u32) < straight.size[0] && (at.y as u32) < straight.size[1]);
        let i = ((at.y as u32 * straight.size[0] + at.x as u32) * 4) as usize;
        let expected = bilinear(&original, p);
        for c in 0..4 {
            assert!((straight.rgba[i + c] as f32 - expected[c]).abs() <= 3., "photo and stroke move together at {p:?}: {:?} vs {expected:?}", &straight.rgba[i..i + 4]);
        }
    }
    assert!(engine.undo().unwrap());
    image(&mut engine, 3_100_000_000).assert_eq(&original, "one undo step restores the photo");
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
            coverage: CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), extent, [0, 0]),
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
    let domain = [SIZE[0] + 256, SIZE[1]];
    paint_mut(&mut doc, upper).domain = domain;
    doc.apply(occurrence_edit(&doc, upper, |o| o.offset = [-256, 0])).unwrap();
    let lower = doc.scene().order()[1];
    let (mut engine, mut input) = engine(doc);
    add_selection_mask(&mut engine, upper, Selection::polygon(vec![
        Point { x: 256., y: 0. },
        Point { x: 556., y: 0. },
        Point { x: 556., y: 200. },
        Point { x: 256., y: 200. },
    ]).unwrap(), 0.);
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
            let old = Affine::translation(layer_core::offsets::point(document.scene().target_offset(id)));
            let new = Affine::translation(layer_core::offsets::point(engine.document().scene().target_offset(id))).inverse().unwrap();
            let extent = document.target_extent(id).map(|v| v as f32);
            let page = |[x, y]: [u32; 2]| Rect {
                min: Point { x: (x * 256) as f32, y: (y * 256) as f32 },
                max: Point { x: ((x + 1) * 256) as f32, y: ((y + 1) * 256) as f32 },
            };
            let held = document.target_raster(id).unwrap().wait_data().unwrap().tiles.keys()
                .filter(|k| k.plane == plane)
                .map(|k| k.coordinate)
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

#[test]
fn flipping_an_edited_photo_layer_moves_its_samples_and_stroke_exactly() {
    let (mut engine, _input, photo) = edited_photo();
    let original = image(&mut engine, 2_000_000_000);
    let target = engine.document().scene().source_target(photo).unwrap();
    let flip = LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(Affine([-1., 0., 0., 1., SIZE[0] as f32, 0.])) };
    let plan = engine.document().exact_layer_transform_plan(target, &flip, engine.geometry_limits()).unwrap().unwrap();
    let plan = remapped(&mut engine, plan);
    engine.apply_edit(Edit::Batch(plan.edits)).unwrap();
    assert!(engine.document().extents_cover_canvas());
    let flipped = image(&mut engine, 3_000_000_000);
    assert_permutation(&flipped, &original, CanvasGeometry::orient(SIZE, ImageOrientation::FlipHorizontal).to_canvas(), "the flipped photo layer");
    assert!(engine.undo().unwrap());
    image(&mut engine, 3_100_000_000).assert_eq(&original, "one undo step");
}

/// Wetness in 64-pixel blocks of 0, 1, 2 and 3 out of 255 around the 2/255
/// membership threshold, under wet color, on a layer with or without a photo.
fn wet_layer(photo: bool) -> (Engine, InputProducer<PenEvent>, OccurrenceHandle) {
    use raster::{RasterData, RasterPlane, RasterTile, RasterWatercolor, TileBlob, TileKey};
    let mut doc = Document::new(PortableId::random(), SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let layer = doc.scene().order()[0];
    if photo {
        let mut base = layer_core::authored::PaintBase::new(color::source::rgba8_source([300, 200], |x, y| [(60 + x / 3) as u8, (40 + y / 2) as u8, 90, 255]).into());
        base.offset = [40, 30];
        paint_mut(&mut doc, layer).base = Some(base);
    }
    let color = doc.composition().color;
    let mut data = RasterData { watercolor: Some(RasterWatercolor { wet_edge: 0.9, burnt_edge: 0.6, edge_width: 6. }), ..Default::default() };
    for coordinate in [[0u32, 0u32], [1, 0]] {
        for plane in [RasterPlane::Color, RasterPlane::WatercolorWetness] {
            let descriptor = plane.descriptor(color);
            assert_eq!(descriptor.bytes_per_pixel(), Some(if plane == RasterPlane::Color { 4 } else { 1 }));
            let bytes: Vec<u8> = (0..256 * 256).flat_map(|i| {
                let [x, y] = [coordinate[0] * 256 + i % 256, coordinate[1] * 256 + i / 256];
                let wetness = ((x / 64 + y / 64) % 4) as u8;
                if plane == RasterPlane::Color { vec![40 + wetness * 40, 90, 150, 200] } else { vec![wetness] }
            }).collect();
            data.tiles.insert(TileKey { plane, coordinate }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()));
        }
    }
    paint_mut(&mut doc, layer).raster = layer_core::raster::RasterRevision::backed(data);
    let (engine, input) = engine(doc);
    (engine, input, layer)
}

/// The wetness byte under document pixel `p`, if the layer stores one there.
fn wetness_at(document: &Document, layer: OccurrenceHandle, p: [u32; 2]) -> Option<u8> {
    let target = document.scene().source_target(layer).unwrap();
    let local = document.local_to_document(target).inverse().unwrap().map(Point { x: p[0] as f32 + 0.5, y: p[1] as f32 + 0.5 });
    let [x, y] = [local.x.floor() as i64, local.y.floor() as i64];
    if x < 0 || y < 0 { return None; }
    let data = document.target_raster(target).unwrap().wait_data().unwrap();
    let key = raster::TileKey { plane: raster::RasterPlane::WatercolorWetness, coordinate: [(x / 256) as u32, (y / 256) as u32] };
    let bytes = data.tiles.get(&key)?.wait_backing().unwrap().decode().unwrap();
    Some(bytes[((y % 256) * 256 + x % 256) as usize])
}

#[test]
fn exact_turns_keep_every_wetness_sample_with_or_without_a_photo() {
    for photo in [false, true] {
        let (mut engine, _input, layer) = wet_layer(photo);
        let original = image(&mut engine, 1_000_000_000);
        let before = engine.document().clone();
        let geometry = CanvasGeometry::orient(SIZE, ImageOrientation::RotateRight);
        prepared(&mut engine, &geometry);
        let turned = image(&mut engine, 2_000_000_000);
        assert_permutation(&turned, &original, geometry.to_canvas(), &format!("photo {photo}: wet color"));
        let back = geometry.to_canvas().inverse().unwrap();
        for y in (0..geometry.rect.size[1]).step_by(7) {
            for x in (0..geometry.rect.size[0]).step_by(5) {
                let source = back.map(Point { x: x as f32 + 0.5, y: y as f32 + 0.5 });
                let was = wetness_at(&before, layer, [source.x as u32, source.y as u32]);
                assert_eq!(wetness_at(engine.document(), layer, [x, y]).unwrap_or(0), was.unwrap_or(0), "photo {photo}: wetness at {x}, {y}");
            }
        }
        assert!(engine.undo().unwrap());
        image(&mut engine, 2_100_000_000).assert_eq(&original, &format!("photo {photo}: undo"));
        let restored = engine.document().target_raster(engine.document().scene().source_target(layer).unwrap()).unwrap().wait_data().unwrap();
        let stored = before.target_raster(before.scene().source_target(layer).unwrap()).unwrap().wait_data().unwrap();
        assert!(restored.tiles.iter().all(|(key, tile)| stored.tiles[key].same_capture(tile)), "photo {photo}: undo keeps the original wetness bytes");
    }
}

#[test]
fn halving_resamples_wetness_without_moving_uniform_blocks_across_the_threshold() {
    for photo in [false, true] {
        let (mut engine, mut input, layer) = wet_layer(photo);
        image(&mut engine, 1_000_000_000);
        let before = engine.document().clone();
        let half = [SIZE[0] / 2, SIZE[1] / 2];
        engine.apply_canvas_geometry(&CanvasGeometry::resize(SIZE, half, Interpolation::Bicubic)).unwrap();
        image(&mut engine, 2_000_000_000);
        assert!(engine.document().scene().paint_source(layer).unwrap().raster.wait_data().unwrap().watercolor.is_some(), "photo {photo}: the layer stays wet");
        for block_y in 0..2u32 {
            for block_x in 0..6u32 {
                let centre = [block_x * 32 + 16, block_y * 32 + 16];
                let expected = ((block_x + block_y) % 4) as u8;
                assert_eq!(wetness_at(engine.document(), layer, centre), Some(expected), "photo {photo}: block {block_x}, {block_y} keeps wetness {expected}/255");
            }
        }
        let mut brush = default_brush(DefaultBrushPreset::WetWatercolor);
        brush.diameter = 30.;
        brush.color_rgba_linear = [0.8, 0.2, 0.1, 1.];
        engine.set_brush(brush).unwrap();
        let resampled = image(&mut engine, 3_000_000_000);
        draw_with_brush(&mut engine, &mut input, Point { x: 20., y: 20. }, Point { x: 150., y: 100. }, 4_000_000_000);
        let wet = image(&mut engine, 5_000_000_000);
        assert_ne!(wet.rgba, resampled.rgba, "photo {photo}: wet painting continues on the resampled layer");
        assert!(engine.undo().unwrap());
        assert!(engine.undo().unwrap());
        image(&mut engine, 5_100_000_000);
        let restored = engine.document().target_raster(engine.document().scene().source_target(layer).unwrap()).unwrap().wait_data().unwrap();
        let stored = before.target_raster(before.scene().source_target(layer).unwrap()).unwrap().wait_data().unwrap();
        assert!(restored.tiles.iter().all(|(key, tile)| stored.tiles[key].same_capture(tile)), "photo {photo}: undo restores the original wetness");
    }
}

#[test]
fn turned_and_cut_photos_save_and_reopen_with_only_their_kept_samples() {
    let (mut engine, _input, photo) = edited_photo();
    let source = paint(engine.document(), photo).base.clone().unwrap();
    prepared(&mut engine, &CanvasGeometry::orient(SIZE, ImageOrientation::RotateLeft));
    prepared(&mut engine, &CanvasGeometry { delete_outside: true, ..geometry([20, 70], [180, 200]) });
    let edited = image(&mut engine, 2_000_000_000);
    let base = paint(engine.document(), photo).base.clone().unwrap();
    assert_eq!(base.image.interpretation, source.image.interpretation);
    let bytes = package_bytes(engine.document());
    let reopened = reopen(&bytes);
    let images = reopened.artwork.images().unwrap();
    assert_eq!(images.len(), 1, "the package keeps only the cut photo");
    let saved = images.values().next().unwrap();
    assert_eq!((saved.id(), saved.extent), (base.image.id(), base.image.extent));
    assert!(saved.extent[0] < 200 && saved.extent[1] < 300, "excluded samples are not saved");
    let (mut reopened_engine, _) = self::engine(reopened);
    image(&mut reopened_engine, 0).assert_eq(&edited, "the reopened drawing");
    assert!(engine.undo().unwrap());
    assert!(engine.undo().unwrap());
    assert!(paint(engine.document(), photo).base.as_ref().unwrap().image.same_owner(&source.image), "undo restores the original photo");
}

/// A 16-bit Display P3 photo of `extent` at `offset` on an otherwise blank
/// 384 × 258 canvas, untouched, with an image object sharing its image.
fn p3_photo(extent: [u32; 2], offset: [u32; 2]) -> (Engine, OccurrenceHandle, layer_core::authored::ImageObjectHandle, layer_core::authored::Image) {
    use color::source::*;
    let mut doc = Document::new(PortableId::random(), 384, 258, layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() });
    let photo = doc.scene().order()[0];
    let interpretation = SourceInterpretation { channels: SourceChannels::Rgb, depth: color::SampleDepth::U16, profile: color::ColorProfile::Builtin(color::RgbSpace::DisplayP3), profile_assumed: false };
    let mut builder = SourceBuilder::new(extent, interpretation, 1 << 26).unwrap();
    for y in 0..extent[1] {
        builder.push_row(&(0..extent[0]).flat_map(|x| [x * 170, y * 250, 30000 + (x + y) * 20].map(|v| v as u16).into_iter().flat_map(u16::to_le_bytes)).collect::<Vec<_>>()).unwrap();
    }
    let image = layer_core::authored::Image::new(std::sync::Arc::new(builder.finish().unwrap()));
    let mut base = layer_core::authored::PaintBase::new(image.clone());
    base.offset = offset;
    paint_mut(&mut doc, photo).base = Some(base);
    let (layer, edit) = doc.create_object_layer_edit("Reference", layer_core::ImageObject::new(image.clone()), None, 0).unwrap();
    doc.apply(edit).unwrap();
    doc.artwork.occurrences.get_mut(layer).unwrap().visible = false;
    let object = doc.scene().object_handle(layer).unwrap();
    let (engine, _) = engine(doc);
    (engine, photo, object, image)
}

fn image_of(engine: &mut Engine, time: u64) -> Image {
    image(engine, time)
}

/// Every 16-bit sample of `image`, read once, indexed by pixel.
struct Samples { width: usize, channels: usize, values: Vec<u16> }
impl Samples {
    fn of(image: &color::source::SourceImage) -> Self {
        let mut rows = image.rows();
        let mut row = vec![0; image.row_bytes()];
        let mut values = Vec::new();
        for y in 0..image.extent[1] {
            rows.read(y, &mut row).unwrap();
            values.extend(row.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])));
        }
        Self { width: image.extent[0] as usize, channels: image.interpretation.channels.count(), values }
    }
    fn at(&self, [x, y]: [u32; 2]) -> &[u16] {
        let start = (y as usize * self.width + x as usize) * self.channels;
        &self.values[start..start + self.channels]
    }
}

/// The Display P3 transfer function, shared with sRGB, in double precision.
fn p3_linear(code: u16) -> f64 {
    let v = f64::from(code) / 65535.;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
fn p3_code(linear: f64) -> f64 {
    let v = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1. / 2.4) - 0.055 };
    v * 65535.
}

#[test]
fn resizing_an_untouched_photo_keeps_its_own_samples_depth_and_profile() {
    let (mut engine, photo, object, image) = p3_photo([384, 258], [0, 0]);
    prepared(&mut engine, &CanvasGeometry::resize([384, 258], [128, 86], Interpolation::Nearest));
    let base = paint(engine.document(), photo).base.clone().unwrap();
    assert_eq!((base.policy, base.offset, base.image.extent), (layer_core::authored::PaintBasePolicy::SourceProfile, [0, 0], [128, 86]));
    assert_eq!(base.image.interpretation, image.interpretation, "a fully covered resample keeps RGB, 16 bits and Display P3");
    let (source, result) = (Samples::of(&image), Samples::of(&base.image));
    for y in 0..86 {
        for x in 0..128 {
            let expected = source.at([3 * x + 1, 3 * y + 1]);
            let actual = result.at([x, y]);
            for c in 0..3 {
                assert!(actual[c].abs_diff(expected[c]) <= 1, "nearest sample {x}, {y}: {actual:?} vs {expected:?}");
            }
        }
    }
    assert!(engine.document().artwork.objects.get(object).unwrap().image.same_owner(&image), "the object keeps the original image");
    assert!(engine.undo().unwrap());
    assert!(paint(engine.document(), photo).base.as_ref().unwrap().image.same_owner(&image), "undo restores the original photo");
}

#[test]
fn smooth_resizing_keeps_the_photo_interpretation_and_averages_in_linear_light() {
    let (mut engine, photo, _, image) = p3_photo([384, 258], [0, 0]);
    prepared(&mut engine, &CanvasGeometry::resize([384, 258], [192, 129], Interpolation::Linear));
    let edited = image_of(&mut engine, 1_000_000_000);
    let base = paint(engine.document(), photo).base.clone().unwrap();
    assert_eq!(base.image.interpretation, image.interpretation);
    assert_eq!(base.image.extent, [192, 129]);
    let (source, result) = (Samples::of(&image), Samples::of(&base.image));
    let mut worst = 0f64;
    for y in 1..128 {
        for x in 1..191 {
            let actual = result.at([x, y]);
            for c in 0..3 {
                let linear = [[0, 0], [1, 0], [0, 1], [1, 1]].iter().map(|[dx, dy]| p3_linear(source.at([2 * x + dx, 2 * y + dy])[c])).sum::<f64>() / 4.;
                worst = worst.max((f64::from(actual[c]) - p3_code(linear)).abs());
            }
        }
    }
    assert!(worst <= 1., "{worst} codes from the linear-light average");
    let bytes = package_bytes(engine.document());
    let reopened = reopen(&bytes);
    let saved = reopened.artwork.images().unwrap();
    assert!(saved.values().any(|saved| saved.id() == base.image.id() && saved.interpretation == image.interpretation), "the resampled photo saves at its own depth and profile");
    let (mut reopened, _) = self::engine(reopened);
    image_of(&mut reopened, 0).assert_eq(&edited, "the reopened drawing");
}

#[test]
fn straightening_an_untouched_photo_adds_alpha_where_it_turns_away() {
    let (mut engine, photo, _, image) = p3_photo([200, 150], [80, 50]);
    let geometry = CanvasGeometry { linear: CanvasGeometry::rotation([192., 129.], -0.2), ..CanvasGeometry::crop(CanvasRect { origin: [0, 0], size: [384, 258] }) };
    let geometry = CanvasGeometry { interpolation: Interpolation::Bicubic, ..geometry };
    prepared(&mut engine, &geometry);
    let base = paint(engine.document(), photo).base.clone().unwrap();
    let interpretation = &base.image.interpretation;
    assert_eq!((interpretation.channels, interpretation.depth, &interpretation.profile),
        (color::source::SourceChannels::Rgba, image.interpretation.depth, &image.interpretation.profile), "turned corners gain alpha, keeping depth and profile");
    let (source, result) = (Samples::of(&image), Samples::of(&base.image));
    assert_eq!(result.at([0, 0])[3], 0, "the corner the photo turned away from is transparent");
    let to_canvas = geometry.to_canvas64();
    let document = to_canvas.map([180.5, 125.5]);
    let local = engine.document().local_to_document(engine.document().scene().source_target(photo).unwrap()).inverse().unwrap()
        .map(Point { x: document[0] as f32, y: document[1] as f32 });
    let at = [local.x as u32 - base.offset[0], local.y as u32 - base.offset[1]];
    let actual = result.at(at);
    let expected = source.at([100, 75]);
    assert_eq!(actual[3], u16::MAX);
    for c in 0..3 {
        assert!(actual[c].abs_diff(expected[c]) <= 600, "interior sample {actual:?} vs {expected:?}");
    }
    assert!(engine.undo().unwrap());
    assert!(paint(engine.document(), photo).base.as_ref().unwrap().image.same_owner(&image));
}
