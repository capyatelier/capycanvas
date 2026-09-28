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
    let mut doc = Document::new("geometry", 512, 256);
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
    let edit = doc.canvas_geometry_edit(&geometry, limits()).unwrap();
    editor.perform(edit).unwrap();
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
    doc.layer_transform(id).map(local)
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
    let back = cropped.canvas_geometry_edit(&rect([-300, -100], [512, 256]), limits()).unwrap();
    editor.perform(back).unwrap();
    assert!(editor.document().extents_cover_canvas());
    same_state(editor.document(), &doc);
    let usage = |d: &Document| d.canvas_geometry_edit(&rect([0, 0], [1, 1]), GeometryLimits {
        project: ProjectLimits { tiles: 3, ..Default::default() },
        ..limits()
    });
    assert_eq!(usage(&cropped).unwrap_err(), CanvasGeometryError::TooManyTiles { limit: 3 });
}

#[test]
fn limits_are_refused_before_anything_changes() {
    let doc = fixture();
    let refuse = |geometry: CanvasGeometry, limits: GeometryLimits| {
        let error = doc.canvas_geometry_edit(&geometry, limits).unwrap_err();
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
    let mut resampled = rect([0, 0], [10, 10]);
    resampled.linear = Affine([2., 0., 0., 2., 0., 0.]);
    assert!(matches!(refuse(resampled, limits()), CanvasGeometryError::Unsupported(_)));
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
    assert_eq!(placed.properties.extent, None);
    assert_eq!(result.target_extent(photo), [700, 300]);
}

#[test]
fn rebasing_conjugates_a_placement_so_pixels_stay_put() {
    let mut doc = fixture();
    doc.layers[3].properties.placement = Affine::around(Point { x: 20., y: 10. }, [1.5, 0.5], 0.4, Point { x: 3., y: -8. });
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
