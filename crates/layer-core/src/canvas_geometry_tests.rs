use super::*;
use crate::raster::{RasterPlane, RasterTile, TileBlob};

fn tile(plane: RasterPlane, color: color::DocumentColor, seed: u8) -> RasterTile {
    let descriptor = plane.descriptor(color);
    let bytes: Vec<u8> = (0..descriptor.byte_len([TILE_SIZE; 2]).unwrap())
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap())
}

fn raster(plane: RasterPlane, color: color::DocumentColor, keys: &[[u32; 2]]) -> RasterRevision {
    RasterRevision::backed(RasterData {
        tiles: keys
            .iter()
            .enumerate()
            .map(|(i, c)| (TileKey { plane, coordinate: *c }, tile(plane, color, i as u8 + 3)))
            .collect(),
        watercolor: None,
    })
}

fn photo_source(extent: [u32; 2]) -> color::source::SourceImage {
    use color::source::*;
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: color::SampleDepth::U8,
        profile: Default::default(),
        profile_assumed: false,
    }, 8 * 1024 * 1024).unwrap();
    for _ in 0..extent[1] {
        builder.push_row(&vec![200; extent[0] as usize * 4]).unwrap();
    }
    builder.finish().unwrap()
}

fn limits() -> GeometryLimits {
    GeometryLimits { project: ProjectLimits::default(), device_dimension: 16384 }
}

fn rect(origin: [i32; 2], size: [u32; 2]) -> CanvasGeometry {
    CanvasGeometry::crop(CanvasRect { origin, size })
}

/// A 512×256 drawing: a masked paint layer, a group with a child, a Selection
/// Layer, a selection and a ruler.
fn fixture() -> Document {
    let mut doc = Document::new("geometry", 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let color = doc.color;
    doc.layers[0].raster = raster(RasterPlane::Color, color, &[[0, 0], [1, 0]]);
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point::default());
    mask.raster = raster(RasterPlane::Mask, color, &[[1, 0]]);
    mask.initial = Some(Selection::polygon(vec![
        Point { x: 10., y: 10. },
        Point { x: 60., y: 10. },
        Point { x: 60., y: 40. },
    ]).unwrap());
    mask.default_coverage = 0.;
    doc.layers[0].mask = Some(mask);
    let group = doc.allocate_layer_id();
    let mut layer = Layer::paint(group, "Group");
    layer.kind = LayerKind::Group;
    layer.properties.offset = Point { x: 5., y: 7. };
    doc.layers.insert(0, layer);
    let child = doc.allocate_layer_id();
    let mut layer = Layer::paint(child, "Child");
    layer.properties.parent = Some(group);
    layer.properties.offset = Point { x: -5., y: -7. };
    layer.raster = raster(RasterPlane::Color, color, &[[0, 0]]);
    doc.layers.insert(0, layer);
    let saved = doc.allocate_layer_id();
    let square = Selection::polygon(vec![
        Point { x: 100., y: 100. },
        Point { x: 200., y: 100. },
        Point { x: 200., y: 200. },
        Point { x: 100., y: 200. },
    ]).unwrap();
    doc.layers.insert(0, Layer::selection(saved, "Saved", square.clone()));
    doc.selection = Some(square);
    doc.rulers = vec![Ruler { id: 4, geometry: RulerGeometry::Straight { start: Point { x: 1., y: 2. }, end: Point { x: 30., y: 40. } } }];
    doc
}

fn apply(doc: &Document, geometry: CanvasGeometry) -> Editor {
    let mut editor = Editor::new(doc.clone());
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    assert!(plan.operations.is_empty(), "a crop that keeps its pixels needs no pixel work");
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    assert!(editor.document().extents_cover_canvas());
    editor
}

fn same_state(a: &Document, b: &Document) {
    assert_eq!([a.width, a.height], [b.width, b.height]);
    assert_eq!(a.layers, b.layers);
    assert_eq!(a.selection, b.selection);
    assert_eq!(a.rulers, b.rulers);
}

fn document_point(doc: &Document, id: LayerId, local: Point) -> Point {
    doc.affine_edit_transform(id).unwrap().map(local)
}

#[test]
fn growing_left_and_up_rebases_whole_tiles_with_masks_and_their_initial_coverage() {
    let doc = fixture();
    let paint = doc.layers[3].id;
    let mask = doc.layers[3].mask.as_ref().unwrap().id;
    let before = doc.layers[3].clone();
    let editor = apply(&doc, rect([-100, -300], [612, 556]));
    let after = editor.document().layer(paint).unwrap();
    assert_eq!(after.properties.offset, Point { x: -156., y: -212. });
    let old = before.raster.wait_data().unwrap();
    let new = after.raster.wait_data().unwrap();
    assert_eq!(new.tiles.len(), old.tiles.len());
    for (key, tile) in &old.tiles {
        let moved = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + 1, key.coordinate[1] + 2] };
        assert!(new.tiles[&moved].same_capture(tile), "tiles are re-keyed, not copied");
    }
    assert_eq!(after.properties.extent, Some([768, 768]));
    assert_eq!(editor.document().target_extent(paint), [768, 768]);
    let new_mask = after.mask.as_ref().unwrap();
    let old_mask = before.mask.as_ref().unwrap();
    assert_eq!(new_mask.offset, Point { x: -156., y: -212. });
    assert!(new_mask.raster.wait_data().unwrap().tiles.keys().all(|k| k.coordinate == [2, 2]));
    assert_eq!(new_mask.initial, old_mask.initial.as_ref().map(|s| s.translated(Point { x: 256., y: 512. })));
    for (id, local) in [(paint, Point { x: 0., y: 0. }), (mask, Point { x: 300., y: 10. })] {
        let moved = Point { x: local.x + 256., y: local.y + 512. };
        let was = document_point(&doc, id, local);
        let is = document_point(editor.document(), id, moved);
        assert_eq!(is, Point { x: was.x + 100., y: was.y + 300. });
    }
}

#[test]
fn only_root_offsets_carry_the_canvas_origin() {
    let doc = fixture();
    let editor = apply(&doc, rect([40, 30], [200, 100]));
    let result = editor.document();
    for (old, new) in doc.layers.iter().zip(&result.layers) {
        if old.properties.parent.is_some() {
            assert_eq!(new.properties.offset, old.properties.offset, "{}", old.name);
        } else if old.kind != LayerKind::Background {
            assert_eq!(new.properties.offset, Point { x: old.properties.offset.x - 40., y: old.properties.offset.y - 30. });
        }
        let world = |d: &Document| d.layer_offset(old.id);
        assert_eq!(world(result).x, world(&doc).x - if old.kind == LayerKind::Background { 0. } else { 40. });
        assert!(new.raster == old.raster, "a crop never rebases");
    }
    assert_eq!(result.layers.last().unwrap(), doc.layers.last().unwrap(), "paper does not move");
}

#[test]
fn selection_saved_selections_and_rulers_move_with_the_canvas() {
    let doc = fixture();
    let editor = apply(&doc, rect([-30, 20], [600, 200]));
    let result = editor.document();
    let delta = Point { x: 30., y: -20. };
    assert_eq!(result.selection, doc.selection.as_ref().map(|s| s.translated(delta)));
    let saved = doc.layers[0].id;
    let bounds = |d: &Document| d.saved_selection(saved).unwrap().bounds();
    assert_eq!(bounds(result).min, Point { x: bounds(&doc).min.x + 30., y: bounds(&doc).min.y - 20. });
    assert_eq!(result.rulers[0].geometry, doc.rulers[0].geometry.translated(delta));
}

#[test]
fn undo_and_redo_restore_the_exact_document() {
    let doc = fixture();
    let mut editor = apply(&doc, rect([-300, -20], [900, 600]));
    let grown = editor.document().clone();
    assert_eq!(editor.next_history_edit(false).and_then(Edit::canvas_origin), Some([300, 20]));
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
    assert!(editor.document().layers.iter().zip(&doc.layers).all(|(a, b)| a.raster == b.raster));
    assert_eq!(editor.next_history_edit(true).and_then(Edit::canvas_origin), Some([-300, -20]));
    assert!(editor.redo().unwrap());
    same_state(editor.document(), &grown);
}

#[test]
fn hidden_pixels_survive_a_crop_and_return_when_the_canvas_grows_back() {
    let doc = fixture();
    let mut editor = apply(&doc, rect([300, 100], [100, 100]));
    let paint = doc.layers[3].id;
    assert_eq!(editor.document().layer(paint).unwrap().properties.extent, Some([512, 256]));
    let cropped = editor.document().clone();
    let back = cropped.canvas_geometry_plan(&rect([-300, -100], [512, 256]), limits()).unwrap();
    editor.perform(Edit::Batch(back.edits)).unwrap();
    assert!(editor.document().extents_cover_canvas());
    same_state(editor.document(), &doc);
    let usage = |d: &Document| d.canvas_geometry_plan(&rect([0, 0], [1, 1]), GeometryLimits {
        project: ProjectLimits { tiles: 3, ..Default::default() },
        ..limits()
    });
    assert_eq!(usage(&cropped).unwrap_err(), CanvasGeometryError::TooManyTiles { limit: 3 });
}

#[test]
fn limits_are_refused_before_anything_changes() {
    let doc = fixture();
    let refuse = |geometry: CanvasGeometry, limits: GeometryLimits| {
        let error = doc.canvas_geometry_plan(&geometry, limits).unwrap_err();
        assert_eq!(doc.check_canvas_geometry(&geometry, limits), Err(error.clone()));
        error
    };
    assert_eq!(refuse(rect([0, 0], [16385, 10]), limits()), CanvasGeometryError::CanvasTooLarge { limit: 16384 });
    let small = GeometryLimits { project: ProjectLimits { dimension: 1000, ..Default::default() }, device_dimension: 16384 };
    assert_eq!(refuse(rect([0, 0], [1001, 10]), small), CanvasGeometryError::CanvasTooLarge { limit: 1000 });
    assert_eq!(refuse(rect([-900, 0], [1000, 256]), small), CanvasGeometryError::ExtentTooLarge { limit: 1000 });
    assert_eq!(refuse(rect([0, 0], [512, 256]), limits()), CanvasGeometryError::Unchanged);
    assert_eq!(refuse(rect([0, 0], [0, 256]), limits()), CanvasGeometryError::Empty);
    let bytes = GeometryLimits { project: ProjectLimits { raster_bytes: 1024, ..Default::default() }, ..limits() };
    assert_eq!(refuse(rect([0, 0], [10, 10]), bytes), CanvasGeometryError::RasterTooLarge);
    let mut singular = rect([0, 0], [10, 10]);
    singular.linear = Affine([2., 0., 4., 0., 0., 0.]);
    assert!(matches!(refuse(singular, limits()), CanvasGeometryError::Unsupported(_)));
}

#[test]
fn locked_layers_follow_and_photos_never_rebase() {
    let mut doc = fixture();
    doc.layers[3].properties.locked = true;
    let photo = doc.allocate_layer_id();
    let mut layer = Layer::paint(photo, "Photo");
    layer.source = Some(Arc::new(photo_source([300, 200])));
    doc.layers.insert(0, layer);
    let editor = apply(&doc, rect([-20, -20], [700, 300]));
    let result = editor.document();
    assert_eq!(result.layer(doc.layers[4].id).unwrap().properties.offset, Point { x: -236., y: -236. });
    let placed = result.layer(photo).unwrap();
    assert_eq!(placed.properties.offset, Point { x: 20., y: 20. });
    assert_eq!(placed.properties.extent, Some(doc.target_extent(photo)));
    assert_eq!(result.target_extent(photo), doc.target_extent(photo));
    assert!(Arc::ptr_eq(placed.source.as_ref().unwrap(), doc.layer(photo).unwrap().source.as_ref().unwrap()));
}

#[test]
fn rebasing_conjugates_a_placement_so_pixels_stay_put() {
    let mut doc = fixture();
    doc.layers[3].properties.placement = LayerPlacement::from_affine(Affine::around(Point { x: 20., y: 10. }, [1.5, 0.5], 0.4, Point { x: 3., y: -8. }));
    let paint = doc.layers[3].id;
    let editor = apply(&doc, rect([-400, 0], [900, 256]));
    let result = editor.document();
    let first = |d: &Document| d.layer(paint).unwrap().raster.wait_data().unwrap().tiles.keys().next().unwrap().coordinate;
    let [x, y] = [0, 1].map(|i| (first(result)[i] - first(&doc)[i]) as f32 * 256.);
    assert!(x > 0.);
    for local in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
        let was = document_point(&doc, paint, local);
        let is = document_point(result, paint, Point { x: local.x + x, y: local.y + y });
        assert!((is.x - was.x - 400.).abs() < 1e-2 && (is.y - was.y).abs() < 1e-2, "{is:?} {was:?}");
    }
}

/// A 1024×768 drawing whose paint layer fills every tile, with a mask.
fn tiled() -> Document {
    let mut doc = Document::new("tiled", 1024, 768, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let color = doc.color;
    let keys: Vec<_> = (0..3).flat_map(|y| (0..4).map(move |x| [x, y])).collect();
    doc.layers[0].raster = raster(RasterPlane::Color, color, &keys);
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point::default());
    mask.raster = raster(RasterPlane::Mask, color, &keys);
    doc.layers[0].mask = Some(mask);
    doc
}

fn deleting(origin: [i32; 2], size: [u32; 2]) -> CanvasGeometry {
    CanvasGeometry { delete_outside: true, ..rect(origin, size) }
}

fn near(a: Point, b: Point) {
    assert!((a.x - b.x).abs() < 1e-2 && (a.y - b.y).abs() < 1e-2, "{a:?} != {b:?}");
}

#[test]
fn deleting_cropped_pixels_keeps_only_the_window_tiles_and_erases_edge_strips() {
    let doc = tiled();
    let paint = doc.layers[0].id;
    let plan = doc.canvas_geometry_plan(&deleting([300, 260], [400, 300]), limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
    let result = editor.document();
    assert!(result.extents_cover_canvas());
    let layer = result.layer(paint).unwrap();
    assert_eq!(layer.properties.offset, Point { x: -44., y: -4. }, "rebased down by whole tiles");
    assert_eq!(layer.properties.extent, Some([512, 512]), "the minimal tile-aligned extent");
    let keys = |raster: &RasterRevision| raster.wait_data().unwrap().tiles.keys().map(|k| k.coordinate).collect::<Vec<_>>();
    assert_eq!(keys(&layer.raster), [[0, 0], [0, 1], [1, 0], [1, 1]]);
    assert_eq!(keys(&layer.mask.as_ref().unwrap().raster), [[0, 0], [0, 1], [1, 0], [1, 1]]);
    let old = doc.layers[0].raster.wait_data().unwrap();
    let new = layer.raster.wait_data().unwrap();
    for (key, tile) in &new.tiles {
        let was = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + 1, key.coordinate[1] + 1] };
        assert!(old.tiles[&was].same_capture(tile), "kept tiles are shared");
    }
    let strips: Vec<_> = plan.operations.iter().map(|(id, op)| {
        assert_eq!(*id, paint, "masks are trimmed but never erased");
        assert!(matches!(op.kind, LayerOperationKind::Erase { alpha_locked: false }));
        assert!(op.bounds(result.target_extent(paint)).max.y <= 512.);
        let bounds = op.coverage.initial.as_ref().unwrap().coverage_bounds();
        [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
    }).collect();
    assert_eq!(strips, [[0., 0., 512., 4.], [0., 304., 512., 512.], [0., 4., 44., 304.], [444., 4., 512., 304.]]);
    for local in [Point { x: 44., y: 4. }, Point { x: 443., y: 303. }] {
        let doc_point = document_point(result, paint, local);
        assert!(doc_point.x >= 0. && doc_point.y >= 0. && doc_point.x < 400. && doc_point.y < 300.);
    }
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
}

#[test]
fn a_tile_limit_refusal_clears_when_cropped_pixels_are_deleted() {
    let doc = tiled();
    let tight = GeometryLimits { project: ProjectLimits { tiles: 20, ..Default::default() }, ..limits() };
    let crop = rect([300, 260], [400, 300]);
    let error = doc.canvas_geometry_plan(&crop, tight).unwrap_err();
    assert_eq!(error, CanvasGeometryError::TooManyTiles { limit: 20 });
    assert!(error.exceeds_raster_limits());
    doc.canvas_geometry_plan(&deleting([300, 260], [400, 300]), tight).unwrap();
    let whole = doc.canvas_geometry_plan(&deleting([0, 0], [1024, 768]), limits());
    assert_eq!(whole.unwrap_err(), CanvasGeometryError::Unchanged, "nothing hidden to delete");
}

fn straighten(doc: &Document, angle: f32, rect: CanvasRect, delete_outside: bool) -> CanvasGeometry {
    let center = Point { x: doc.width as f32 / 2., y: doc.height as f32 / 2. };
    CanvasGeometry {
        rect,
        linear: Affine::around(center, [1., 1.], -angle, Point::default()),
        interpolation: Interpolation::Bicubic,
        delete_outside,
    }
}

#[test]
fn straightening_resamples_paint_and_masks_into_a_frame_that_keeps_hidden_corners() {
    let mut doc = fixture();
    doc.layers[3].properties.locked = true;
    let geometry = straighten(&doc, 0.2, CanvasRect { origin: [40, 30], size: [430, 190] }, false);
    let to_canvas = geometry.to_canvas();
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
    let result = editor.document().clone();
    assert!(result.extents_cover_canvas());
    assert_eq!([result.width, result.height], [430, 190]);
    let paint = doc.layers[3].id;
    let mask = doc.layers[3].mask.as_ref().unwrap().id;
    let child = doc.layers[1].id;
    let targets: Vec<_> = plan.operations.iter().map(|(id, _)| *id).collect();
    assert_eq!(targets, [child, paint, mask], "the locked layer and its mask follow");
    for (id, op) in &plan.operations {
        let LayerOperationKind::Transform(transform) = &op.kind else { panic!("a resample") };
        assert_eq!(transform.placement.interpolation, Interpolation::Bicubic);
        let map = transform.as_affine().unwrap();
        let extent = doc.target_extent(*id);
        let after = result.affine_edit_transform(*id).unwrap().inverse().unwrap();
        let new_extent = result.target_extent(*id);
        for corner in Rect::from_extent(extent).corners() {
            let expected = after.map(to_canvas.map(doc.affine_edit_transform(*id).unwrap().map(corner)));
            near(map.map(corner), expected);
            assert!(expected.x >= -0.01 && expected.y >= -0.01, "{expected:?}");
            assert!(expected.x <= new_extent[0] as f32 + 0.01 && expected.y <= new_extent[1] as f32 + 0.01);
        }
        assert!(is_translation(result.affine_edit_transform(*id).unwrap()));
    }
    let saved = doc.layers[0].id;
    let before = doc.saved_selection(saved).unwrap();
    let moved = result.saved_selection(saved).unwrap();
    for p in [Point { x: 100., y: 100. }, Point { x: 200., y: 150. }] {
        near(moved.affine.map(before.affine.inverse().unwrap().map(p)), to_canvas.map(p));
    }
    assert_eq!(result.selection, Some(doc.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
    assert_eq!(result.rulers[0].geometry, doc.rulers[0].geometry.transformed(to_canvas));
    let (start, _) = result.rulers[0].geometry.handles();
    near(start, to_canvas.map(Point { x: 1., y: 2. }));
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
}

#[test]
fn straightening_turns_a_photo_placement_without_touching_its_pixels() {
    let mut doc = fixture();
    let photo = doc.allocate_layer_id();
    let mut layer = Layer::paint(photo, "Photo");
    layer.source = Some(Arc::new(photo_source([300, 200])));
    layer.properties.offset = Point { x: 20., y: 10. };
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point { x: 20., y: 10. });
    mask.linked = false;
    layer.mask = Some(mask);
    doc.layers.insert(0, layer);
    let geometry = straighten(&doc, -0.3, CanvasRect { origin: [10, 5], size: [490, 240] }, false);
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    assert!(plan.operations.iter().all(|(id, _)| *id != photo && doc.layers[0].mask.as_ref().unwrap().id != *id));
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    let result = editor.document();
    let before = doc.layer(photo).unwrap();
    let after = result.layer(photo).unwrap();
    assert!(Arc::ptr_eq(before.source.as_ref().unwrap(), after.source.as_ref().unwrap()));
    assert!(after.raster == before.raster, "losslessly placed");
    let mask = before.mask.as_ref().unwrap().id;
    for id in [photo, mask] {
        for p in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
            near(result.affine_edit_transform(id).unwrap().map(p), geometry.to_canvas().map(doc.affine_edit_transform(id).unwrap().map(p)));
        }
    }
}

#[test]
fn straightening_with_deleted_pixels_frames_the_canvas_and_erases_beyond_it() {
    let doc = tiled();
    let paint = doc.layers[0].id;
    let geometry = straighten(&doc, 0.1, CanvasRect { origin: [100, 80], size: [800, 600] }, true);
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
    let result = editor.document();
    assert_eq!(result.layer(paint).unwrap().properties.offset, Point::default());
    let extent = result.target_extent(paint);
    let kinds: Vec<_> = plan.operations.iter().filter(|(id, _)| *id == paint).map(|(_, op)| {
        let bounds = op.coverage.initial.as_ref().unwrap().coverage_bounds();
        (matches!(op.kind, LayerOperationKind::Erase { .. }), [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y])
    }).collect();
    assert!(!kinds[0].0);
    assert_eq!(&kinds[1..], [
        (true, [800., 0., extent[0] as f32, extent[1] as f32]),
        (true, [0., 600., 800., extent[1] as f32]),
    ]);
    let tight = GeometryLimits { project: ProjectLimits { tiles: 20, ..Default::default() }, ..limits() };
    assert_eq!(doc.canvas_geometry_plan(&geometry, tight).unwrap_err(), CanvasGeometryError::TooManyTiles { limit: 20 });
}

const ORIENTATIONS: [ImageOrientation; 5] = [
    ImageOrientation::FlipHorizontal,
    ImageOrientation::FlipVertical,
    ImageOrientation::RotateLeft,
    ImageOrientation::RotateRight,
    ImageOrientation::Rotate180,
];

/// The fixture with a placed photo on top.
fn with_photo() -> (Document, LayerId) {
    let mut doc = fixture();
    let photo = doc.allocate_layer_id();
    let mut layer = Layer::paint(photo, "Photo");
    layer.source = Some(Arc::new(photo_source([300, 200])));
    layer.properties.offset = Point { x: 20., y: 10. };
    doc.layers.insert(0, layer);
    (doc, photo)
}

fn transforms(plan: &CanvasGeometryPlan) -> Vec<(LayerId, ImageTransform)> {
    plan.operations
        .iter()
        .map(|(id, op)| match &op.kind {
            LayerOperationKind::Transform(transform) => (*id, transform.clone()),
            kind => panic!("a resample, not {kind:?}"),
        })
        .collect()
}

#[test]
fn flips_and_turns_move_pixel_centres_onto_pixel_centres_in_one_step() {
    let (doc, photo) = with_photo();
    for orientation in ORIENTATIONS {
        let geometry = CanvasGeometry::orient([doc.width, doc.height], orientation);
        let quarter = matches!(orientation, ImageOrientation::RotateLeft | ImageOrientation::RotateRight);
        let to_canvas = geometry.to_canvas();
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
        let result = editor.document();
        assert!(result.extents_cover_canvas());
        assert_eq!([result.width, result.height], if quarter { [256, 512] } else { [512, 256] });
        let resampled = transforms(&plan);
        assert_eq!(resampled.len(), 3, "the child, the paint layer and its mask");
        for (id, transform) in resampled {
            assert_eq!(transform.placement.interpolation, Interpolation::Nearest);
            let map = transform.as_affine().unwrap();
            assert!(map.0.iter().all(|v| v.fract() == 0.), "{orientation:?} {map:?}");
            let extent = result.target_extent(id);
            if quarter && id == doc.layers[4].id {
                assert_eq!(extent, [512, 512], "a square scratch extent holds both orientations");
            }
            for p in [Point { x: 0.5, y: 0.5 }, Point { x: 511.5, y: 255.5 }, Point { x: 100.5, y: 7.5 }] {
                let q = map.map(p);
                assert_eq!([q.x.fract(), q.y.fract()], [0.5, 0.5]);
                assert!(q.x > 0. && q.y > 0. && q.x < extent[0] as f32 && q.y < extent[1] as f32);
                near(result.affine_edit_transform(id).unwrap().map(q), to_canvas.map(doc.affine_edit_transform(id).unwrap().map(p)));
            }
        }
        for p in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
            near(result.affine_edit_transform(photo).unwrap().map(p), to_canvas.map(doc.affine_edit_transform(photo).unwrap().map(p)));
        }
        assert!(result.layer(photo).unwrap().raster == doc.layer(photo).unwrap().raster, "photos only move");
        assert_eq!(result.selection, Some(doc.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
        assert_eq!(result.rulers[0].geometry, doc.rulers[0].geometry.transformed(to_canvas));
        assert!(editor.undo().unwrap());
        same_state(editor.document(), &doc);
    }
}

#[test]
fn turning_four_times_or_flipping_twice_puts_every_pixel_back() {
    let (doc, photo) = with_photo();
    for (orientation, times) in [
        (ImageOrientation::RotateRight, 4),
        (ImageOrientation::RotateLeft, 4),
        (ImageOrientation::Rotate180, 2),
        (ImageOrientation::FlipHorizontal, 2),
        (ImageOrientation::FlipVertical, 2),
    ] {
        let mut editor = Editor::new(doc.clone());
        let paint = doc.layers[4].id;
        let mask = doc.layers[4].mask.as_ref().unwrap().id;
        let samples = [Point { x: 0.5, y: 0.5 }, Point { x: 300.5, y: 200.5 }];
        let mut tracked: Vec<(LayerId, Point)> = [paint, mask].into_iter().flat_map(|id| samples.map(|p| (id, p))).collect();
        for _ in 0..times {
            let current = editor.document().clone();
            let plan = current.canvas_geometry_plan(&CanvasGeometry::orient([current.width, current.height], orientation), limits()).unwrap();
            let maps: BTreeMap<_, _> = transforms(&plan).into_iter().map(|(id, t)| (id, t.as_affine().unwrap())).collect();
            for (id, p) in &mut tracked {
                *p = maps[id].map(*p);
            }
            editor.perform(Edit::Batch(plan.edits)).unwrap();
        }
        let result = editor.document();
        assert_eq!([result.width, result.height], [doc.width, doc.height]);
        for (id, p) in tracked.iter().zip(samples.iter().cycle()).map(|((id, p), start)| (*id, (*p, *start))) {
            near(result.affine_edit_transform(id).unwrap().map(p.0), doc.affine_edit_transform(id).unwrap().map(p.1));
        }
        for p in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
            near(result.affine_edit_transform(photo).unwrap().map(p), doc.affine_edit_transform(photo).unwrap().map(p));
        }
        let selection = result.selection.as_ref().unwrap();
        for p in [Point { x: 100., y: 100. }, Point { x: 200., y: 200. }] {
            near(selection.affine.map(p), doc.selection.as_ref().unwrap().affine.map(p));
        }
        let (start, end) = result.rulers[0].geometry.handles();
        let (was_start, was_end) = doc.rulers[0].geometry.handles();
        near(start, was_start);
        near(end.unwrap(), was_end.unwrap());
    }
}

#[test]
fn a_quarter_turn_swaps_the_resolution_and_a_flip_keeps_it() {
    let mut doc = fixture();
    doc.resolution = Some(ImageResolution { unit: ResolutionUnit::Inch, density: [[300, 1], [150, 1]] });
    let turn = doc.canvas_geometry_plan(&CanvasGeometry::orient([512, 256], ImageOrientation::RotateLeft), limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(turn.edits)).unwrap();
    assert_eq!(editor.document().resolution.unwrap().density, [[150, 1], [300, 1]]);
    assert!(editor.undo().unwrap());
    assert_eq!(editor.document().resolution, doc.resolution);
    let flip = doc.canvas_geometry_plan(&CanvasGeometry::orient([512, 256], ImageOrientation::FlipVertical), limits()).unwrap();
    assert!(!flip.edits.iter().any(|e| matches!(e, Edit::SetResolution(_))));
}

#[test]
fn setting_the_resolution_is_one_undoable_metadata_edit() {
    let doc = fixture();
    let mut editor = Editor::new(doc.clone());
    let resolution = ImageResolution::ppi(240);
    let edit = Edit::SetResolution(Some(resolution));
    assert!(!edit.changes_image());
    editor.perform(edit).unwrap();
    assert_eq!(editor.document().resolution, Some(resolution));
    assert!(editor.undo().unwrap());
    assert_eq!(editor.document().resolution, None);
    let invalid = ImageResolution { unit: ResolutionUnit::Inch, density: [[0, 1], [72, 1]] };
    assert!(editor.perform(Edit::SetResolution(Some(invalid))).is_err());
}

#[test]
fn image_size_scales_layers_placements_selections_guides_and_pixel_distances() {
    let (mut doc, photo) = with_photo();
    let effect = doc.allocate_layer_id();
    let mut blur = EffectInstance::new(bundled_effect_catalog().get("gaussian_blur").unwrap().program());
    blur.set("sigma", EffectValue::Number(3.)).unwrap();
    let mut layer = Layer::paint(effect, "Blur");
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(blur));
    doc.layers.insert(0, layer);
    let sigma = |doc: &Document| match doc.layer(effect).unwrap().effect.as_ref().unwrap().value("sigma") {
        Some(EffectValue::Number(v)) => *v,
        _ => panic!("a number"),
    };
    for (size, expected) in [([256, 128], 1.5), ([2048, 1024], 12.), ([5120, 2560], 30.)] {
        let geometry = CanvasGeometry::resize([512, 256], size, Interpolation::Lanczos);
        let to_canvas = geometry.to_canvas();
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
        let result = editor.document();
        assert_eq!([result.width, result.height], size);
        assert!(result.extents_cover_canvas());
        assert_eq!(sigma(result), expected, "clamped to the parameter's range");
        for (id, transform) in transforms(&plan) {
            assert_eq!(transform.placement.interpolation, Interpolation::Lanczos);
            for p in [Point { x: 0., y: 0. }, Point { x: 256., y: 256. }] {
                near(result.affine_edit_transform(id).unwrap().map(transform.as_affine().unwrap().map(p)), to_canvas.map(doc.affine_edit_transform(id).unwrap().map(p)));
            }
        }
        for p in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
            near(result.affine_edit_transform(photo).unwrap().map(p), to_canvas.map(doc.affine_edit_transform(photo).unwrap().map(p)));
        }
        assert_eq!(result.selection, Some(doc.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
        assert_eq!(result.rulers[0].geometry, doc.rulers[0].geometry.transformed(to_canvas));
        assert_eq!(result.resolution, doc.resolution);
        assert!(editor.undo().unwrap());
        assert_eq!(sigma(editor.document()), 3.);
        same_state(editor.document(), &doc);
    }
}

#[test]
fn resampled_tile_predictions_count_only_the_result() {
    let doc = tiled();
    let limits_with = |tiles| GeometryLimits { project: ProjectLimits { tiles, ..Default::default() }, ..limits() };
    let half = CanvasGeometry::resize([1024, 768], [512, 384], Interpolation::Bicubic);
    doc.canvas_geometry_plan(&half, limits_with(12)).unwrap();
    assert_eq!(doc.canvas_geometry_plan(&half, limits_with(11)).unwrap_err(), CanvasGeometryError::TooManyTiles { limit: 11 });
    let double = CanvasGeometry::resize([1024, 768], [2048, 1536], Interpolation::Bicubic);
    assert_eq!(doc.canvas_geometry_plan(&double, limits_with(95)).unwrap_err(), CanvasGeometryError::TooManyTiles { limit: 95 });
    doc.canvas_geometry_plan(&double, limits_with(96)).unwrap();
    let bytes = |megabytes: u64| GeometryLimits { project: ProjectLimits { raster_bytes: megabytes << 20, ..Default::default() }, ..limits() };
    assert_eq!(doc.canvas_geometry_plan(&double, bytes(14)).unwrap_err(), CanvasGeometryError::RasterTooLarge);
    doc.canvas_geometry_plan(&double, bytes(15)).unwrap();
}

#[test]
fn paint_extent_plan_inverse_maps_the_canvas_and_preserves_hidden_material_and_masks() {
    for map in [
        Affine([0.25, 0., 0., 0.5, 200., 120.]),
        Affine([0.4, 0.15, -0.2, 0.3, 180., 90.]),
        Affine::around(Point { x: 100., y: 80. }, [0.3, 0.5], 0.6, Point { x: 140., y: 70. }),
    ] {
        for linked in [true, false] {
            let mut doc = Document::new("affine extent", 256, 128, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            let id = doc.layers[0].id;
            let mut data = raster(RasterPlane::Color, doc.color, &[[0, 0], [3, 2]]).wait_data().unwrap().as_ref().clone();
            for plane in [RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
                data.tiles.insert(TileKey { plane, coordinate: [3, 2] }, tile(plane, doc.color, 17));
            }
            data.watercolor = Some(crate::raster::RasterWatercolor { wet_edge: 0.8, burnt_edge: 0.6, edge_width: 7. });
            doc.layers[0].raster = RasterRevision::backed(data);
            doc.layers[0].properties.extent = Some([1024, 768]);
            doc.layers[0].properties.placement = LayerPlacement::from_affine(map);
            doc.layers[0].properties.offset = Point { x: -20., y: 15. };
            let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point { x: -20., y: 15. });
            mask.linked = linked;
            mask.default_coverage = 0.;
            mask.initial = Some(Selection::polygon(vec![
                Point { x: 11., y: 9. }, Point { x: 71., y: 9. }, Point { x: 71., y: 49. },
            ]).unwrap());
            mask.raster = raster(RasterPlane::Mask, doc.color, &[[0, 0], [3, 2]]);
            let mask_id = mask.id;
            doc.layers[0].mask = Some(mask);
            let before = doc.layers[0].clone();
            let mut editor = Editor::new(doc.clone());
            let edits = doc.paint_extent_plan(&[id], limits()).unwrap();
            assert!(!edits.is_empty());
            assert!(editor.next_history_edit(false).is_none(), "planning adds no history");
            same_state(editor.document(), &doc);
            editor.perform(Edit::Batch(edits)).unwrap();
            let result = editor.document();
            assert_eq!([result.width, result.height], [256, 128]);
            assert_eq!(result.selection, doc.selection);
            assert_eq!(result.rulers, doc.rulers);
            let after = result.layer(id).unwrap();
            let old = before.raster.wait_data().unwrap();
            let new = after.raster.wait_data().unwrap();
            assert_eq!(old.watercolor, new.watercolor);
            assert_eq!(old.tiles.len(), new.tiles.len());
            let key = TileKey { plane: RasterPlane::Color, coordinate: [0, 0] };
            let moved = new.tiles.iter().find(|(_, tile)| tile.same_capture(&old.tiles[&key])).unwrap().0.coordinate;
            let delta = Point { x: (moved[0] * TILE_SIZE) as f32, y: (moved[1] * TILE_SIZE) as f32 };
            assert!(delta.x > 0. || delta.y > 0., "negative inverse origin rebases whole tiles");
            for (key, tile) in &old.tiles {
                let moved = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + moved[0], key.coordinate[1] + moved[1]] };
                assert!(new.tiles[&moved].same_capture(tile), "{map:?} linked={linked}: hidden planes keep their captures");
            }
            let new_mask = after.mask.as_ref().unwrap();
            let old_mask = before.mask.as_ref().unwrap();
            let old_mask_data = old_mask.raster.wait_data().unwrap();
            let new_mask_data = new_mask.raster.wait_data().unwrap();
            let mask_key = old_mask_data.tiles.keys().next().unwrap();
            let mask_position = new_mask_data.tiles.iter().find(|(_, tile)| tile.same_capture(&old_mask_data.tiles[mask_key])).unwrap().0.coordinate;
            let mask_move = [mask_position[0] - mask_key.coordinate[0], mask_position[1] - mask_key.coordinate[1]];
            let mask_delta = Point { x: (mask_move[0] * TILE_SIZE) as f32, y: (mask_move[1] * TILE_SIZE) as f32 };
            assert_eq!(new_mask.initial, old_mask.initial.as_ref().map(|s| s.translated(mask_delta)));
            for (key, tile) in &old_mask_data.tiles {
                let moved = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + mask_move[0], key.coordinate[1] + mask_move[1]] };
                assert!(new_mask.raster.wait_data().unwrap().tiles[&moved].same_capture(tile));
            }
            for target in [id, mask_id] {
                let inverse = result.affine_edit_transform(target).unwrap().inverse().unwrap();
                for corner in Rect::from_extent([256, 128]).corners() {
                    let local = inverse.map(corner);
                    let extent = result.target_extent(target);
                    assert!(local.x >= -0.01 && local.y >= -0.01, "{map:?} linked={linked}: {local:?}");
                    assert!(local.x <= extent[0] as f32 + 0.01 && local.y <= extent[1] as f32 + 0.01);
                }
                let target_delta = if target == mask_id { mask_delta } else { delta };
                for local in [Point { x: 11., y: 9. }, Point { x: 1000., y: 760. }] {
                    near(document_point(result, target, Point { x: local.x + target_delta.x, y: local.y + target_delta.y }),
                        document_point(&doc, target, local));
                }
            }
            let grown = result.clone();
            assert!(editor.undo().unwrap());
            same_state(editor.document(), &doc);
            assert!(editor.redo().unwrap());
            same_state(editor.document(), &grown);
        }
    }
}

#[test]
fn paint_extent_plan_changes_only_selected_paint_and_keeps_photo_domains_fixed() {
    let mut doc = Document::new("selected extent", 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let id = doc.layers[0].id;
    doc.layers[0].properties.placement = LayerPlacement::from_affine(Affine([0.5, 0., 0., 0.5, 20., 30.]));
    let other = doc.allocate_layer_id();
    let mut unrelated = Layer::paint(other, "Unselected");
    unrelated.properties.placement = LayerPlacement::from_affine(Affine([1. / 128., 0., 0., 1. / 128., 0., 0.]));
    doc.layers.insert(0, unrelated.clone());
    let photo = doc.allocate_layer_id();
    let mut source = Layer::paint(photo, "Photo");
    source.source = Some(Arc::new(photo_source([300, 200])));
    source.properties.placement = LayerPlacement::from_affine(Affine([0.1, 0., 0., 0.1, 100., 50.]));
    doc.layers.insert(0, source.clone());
    let edits = doc.paint_extent_plan(&[id, photo], limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(edits)).unwrap();
    assert_eq!(editor.document().layer(other), Some(&unrelated));
    assert_eq!(editor.document().layer(photo), Some(&source));
    assert!(editor.document().target_extent(id)[0] > 512);
    assert_eq!([editor.document().width, editor.document().height], [512, 256]);
    assert!(doc.validate_paint_extents(&[id], limits()).is_ok());
    assert_eq!(doc.validate_paint_extents(&[other], limits()), Err(CanvasGeometryError::ExtentTooLarge { limit: 32768 }));
}

#[test]
fn paint_extent_plan_cap_refusal_and_identity_are_atomic_and_add_no_history() {
    let mut doc = Document::new("extent cap", 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let id = doc.layers[0].id;
    assert!(doc.paint_extent_plan(&[id], limits()).unwrap().is_empty());
    assert!(doc.validate_paint_extents(&[id], limits()).is_ok());
    doc.layers[0].properties.placement = LayerPlacement::from_affine(Affine([1. / 128., 0., 0., 1. / 128., 0., 0.]));
    let editor = Editor::new(doc.clone());
    let error = CanvasGeometryError::ExtentTooLarge { limit: 32768 };
    assert_eq!(doc.paint_extent_plan(&[id], limits()), Err(error.clone()));
    assert_eq!(doc.validate_paint_extents(&[id], limits()), Err(error));
    same_state(editor.document(), &doc);
    assert!(editor.next_history_edit(false).is_none());
    assert!(editor.next_history_edit(true).is_none());
}

mod transform_pixels_plan {
    use super::*;
    include!("transform_pixels_plan_tests.rs");
}
