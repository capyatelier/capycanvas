use super::*;
use crate::raster::{RasterPlane, RasterTile, TileBlob};
use crate::operation_test_support as fixture;
fn owner(doc:&Document,target:SourceTarget)->&Occurrence {doc.scene().occurrence(doc.scene().source_owner(target).unwrap()).unwrap()}
fn paint_source(doc:&Document,target:SourceTarget)->&PaintSource {let SourceTarget::Paint(h)=target else {panic!("paint")};doc.artwork.paint.get(h).unwrap()}
fn mask_source(doc:&Document,target:SourceTarget)->&CoverageSource {let SourceTarget::Coverage(h)=target else {panic!("coverage")};doc.artwork.coverage.get(h).unwrap()}
fn mask_target(doc:&Document,name:&str)->SourceTarget {SourceTarget::Coverage(fixture::occurrence(doc,name).mask.as_ref().unwrap().source)}
fn saved_selection(doc:&Document,h:OccurrenceHandle)->Selection {doc.saved_selection(h).unwrap()}
fn resolution_edit(doc:&Document,resolution:Option<ImageResolution>)->Edit {
    let mut composition=doc.composition().clone();composition.resolution=resolution;
    Edit::Composition(RecordChange::replace(&doc.artwork.compositions,doc.artwork.root,Some(composition)).unwrap())
}


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

fn fixture() -> Document {
    let mut doc=fixture::document([512,256],&["Saved","Group","Child","Current ink"]);
    let color=doc.composition().color;
    fixture::paint_mut(&mut doc,"Current ink").raster=raster(RasterPlane::Color,color,&[[0,0],[1,0]]);
    let ink=fixture::id(&doc,"Current ink");let mask=fixture::add_mask(&mut doc,ink,[512,256],Point::default());
    let coverage=doc.artwork.coverage.get_mut(mask).unwrap();
    coverage.raster=raster(RasterPlane::Mask,color,&[[1,0]]);
    coverage.initial=Some(Selection::polygon(vec![Point{x:10.,y:10.},Point{x:60.,y:10.},Point{x:60.,y:40.}]).unwrap());coverage.default_coverage=0.;
    fixture::nest(&mut doc,"Group",&["Child"]);
    fixture::occurrence_mut(&mut doc,"Group").translation=Point{x:5.,y:7.};
    fixture::occurrence_mut(&mut doc,"Child").translation=Point{x:-5.,y:-7.};
    fixture::paint_mut(&mut doc,"Child").raster=raster(RasterPlane::Color,color,&[[0,0]]);
    let square=Selection::polygon(vec![Point{x:100.,y:100.},Point{x:200.,y:100.},Point{x:200.,y:200.},Point{x:100.,y:200.}]).unwrap();
    fixture::saved(&mut doc,"Saved",square.clone());doc.working.selection=Some(square);
    doc.artwork.guides.insert(PortableId::random(),Guides {rulers:vec![(PortableId::from_bytes([4;16]),RulerGeometry::Straight {start:Point{x:1.,y:2.},end:Point{x:30.,y:40.}})]}).unwrap();
    fixture::activate(&mut doc,"Current ink");doc
}

fn apply(doc: &Document, geometry: CanvasGeometry) -> Editor {
    let mut editor = Editor::new(doc.clone());
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    assert!(plan.operations.is_empty(), "a crop that keeps its pixels needs no pixel work");
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    assert!(editor.document().extents_cover_canvas());
    editor
}

fn same_state(a:&Document,b:&Document) {
    assert_eq!(a.composition(),b.composition());assert_eq!(a.working.selection,b.working.selection);
    macro_rules! records {($($store:ident),*)=>{$(assert_eq!(a.artwork.$store.iter().collect::<Vec<_>>(),b.artwork.$store.iter().collect::<Vec<_>>());)*};}
    records!(stacks,occurrences,paint,coverage,selections,guides,effects,definitions,outputs);
}

fn document_point(doc: &Document, id: SourceTarget, local: Point) -> Point {
    doc.affine_edit_transform(id).unwrap().map(local)
}

#[test]
fn growing_left_and_up_rebases_whole_tiles_with_masks_and_their_initial_coverage() {
    let doc = fixture();
    let paint = fixture::target(&doc,"Current ink");
    let mask = mask_target(&doc,"Current ink");
    let before_source=paint_source(&doc,paint).clone();
    let editor = apply(&doc, rect([-100, -300], [612, 556]));
    let after = owner(editor.document(),paint);
    assert_eq!(after.translation, Point { x: -156., y: -212. });
    let old = before_source.raster.wait_data().unwrap();
    let new = paint_source(editor.document(),paint).raster.wait_data().unwrap();
    assert_eq!(new.tiles.len(), old.tiles.len());
    for (key, tile) in &old.tiles {
        let moved = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + 1, key.coordinate[1] + 2] };
        assert!(new.tiles[&moved].same_capture(tile), "tiles are re-keyed, not copied");
    }
    assert_eq!(paint_source(editor.document(),paint).domain,[768,768]);
    assert_eq!(editor.document().target_extent(paint), [768, 768]);
    let new_mask=mask_source(editor.document(),mask);
    let old_mask=mask_source(&doc,mask);
    let new_mask_use=after.mask.as_ref().unwrap();
    assert_eq!(new_mask_use.translation, Point { x: -156., y: -212. });
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
    for (handle,_,old) in doc.artwork.occurrences.iter() {
        let new=result.artwork.occurrences.get(handle).unwrap();
        if doc.scene().parent(handle).is_some() {
            assert_eq!(new.translation, old.translation, "{}", old.name);
        } else {
            assert_eq!(new.translation, Point { x: old.translation.x - 40., y: old.translation.y - 30. });
        }
        let world = |d: &Document| d.layer_offset(handle);
        assert_eq!(world(result).x, world(&doc).x - 40.);
        if let Some(target)=doc.scene().source_target(handle) {assert_eq!(result.target_raster(target),doc.target_raster(target),"a crop never rebases");}
    }
}

#[test]
fn selection_saved_selections_and_rulers_move_with_the_canvas() {
    let doc = fixture();
    let editor = apply(&doc, rect([-30, 20], [600, 200]));
    let result = editor.document();
    let delta = Point { x: 30., y: -20. };
    assert_eq!(result.working.selection, doc.working.selection.as_ref().map(|s| s.translated(delta)));
    let saved=fixture::id(&doc,"Saved");
    let bounds = |d: &Document| saved_selection(d,saved).bounds();
    assert_eq!(bounds(result).min, Point { x: bounds(&doc).min.x + 30., y: bounds(&doc).min.y - 20. });
    assert_eq!(result.rulers().next().unwrap().geometry, doc.rulers().next().unwrap().geometry.translated(delta));
}

#[test]
fn undo_and_redo_restore_the_exact_document() {
    let doc = fixture();
    let mut editor = apply(&doc, rect([-300, -20], [900, 600]));
    let grown = editor.document().clone();
    assert_eq!(editor.next_history_edit(false).and_then(|edit|edit.canvas_origin_from(editor.document().composition().origin)), Some([300, 20]));
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
    assert_eq!(editor.document().artwork.paint,doc.artwork.paint);
    assert_eq!(editor.next_history_edit(true).and_then(|edit|edit.canvas_origin_from(editor.document().composition().origin)), Some([-300, -20]));
    assert!(editor.redo().unwrap());
    same_state(editor.document(), &grown);
}

#[test]
fn hidden_pixels_survive_a_crop_and_return_when_the_canvas_grows_back() {
    let doc = fixture();
    let mut editor = apply(&doc, rect([300, 100], [100, 100]));
    let paint = fixture::target(&doc,"Current ink");
    assert_eq!(paint_source(editor.document(),paint).domain,[512,256]);
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
    fixture::occurrence_mut(&mut doc,"Current ink").locked = true;
    fixture::insert_paint(&mut doc,"Photo",0,None);
    let photo=fixture::target(&doc,"Photo");
    fixture::paint_mut(&mut doc,"Photo").original=Some(Arc::new(photo_source([300,200])));
    fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    let editor = apply(&doc, rect([-20, -20], [700, 300]));
    let result = editor.document();
    assert_eq!(fixture::occurrence(result,"Current ink").translation, Point { x: -236., y: -236. });
    let placed = owner(result,photo);
    assert_eq!(placed.translation, Point { x: 20., y: 20. });
    assert_eq!(paint_source(result,photo).domain,doc.target_extent(photo));
    assert_eq!(result.target_extent(photo), doc.target_extent(photo));
    assert!(Arc::ptr_eq(paint_source(result,photo).original.as_ref().unwrap(),paint_source(&doc,photo).original.as_ref().unwrap()));
}

#[test]
fn rebasing_conjugates_a_placement_so_pixels_stay_put() {
    let mut doc = fixture();
    fixture::occurrence_mut(&mut doc,"Current ink").placement = LayerPlacement::from_affine(Affine::around(Point { x: 20., y: 10. }, [1.5, 0.5], 0.4, Point { x: 3., y: -8. }));
    let paint = fixture::target(&doc,"Current ink");
    let editor = apply(&doc, rect([-400, 0], [900, 256]));
    let result = editor.document();
    let first = |d: &Document| d.target_raster(paint).unwrap().wait_data().unwrap().tiles.keys().next().unwrap().coordinate;
    let [x, y] = [0, 1].map(|i| (first(result)[i] - first(&doc)[i]) as f32 * 256.);
    assert!(x > 0.);
    for local in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
        let was = document_point(&doc, paint, local);
        let is = document_point(result, paint, Point { x: local.x + x, y: local.y + y });
        assert!((is.x - was.x - 400.).abs() < 1e-2 && (is.y - was.y).abs() < 1e-2, "{is:?} {was:?}");
    }
}

fn tiled() -> Document {
    let mut doc=fixture::document([1024,768],&["Current ink"]);let color=doc.composition().color;
    let keys:Vec<_>=(0..3).flat_map(|y|(0..4).map(move |x|[x,y])).collect();
    fixture::paint_mut(&mut doc,"Current ink").raster=raster(RasterPlane::Color,color,&keys);
    let owner=fixture::id(&doc,"Current ink");let mask=fixture::add_mask(&mut doc,owner,[1024,768],Point::default());
    doc.artwork.coverage.get_mut(mask).unwrap().raster=raster(RasterPlane::Mask,color,&keys);doc
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
    let paint = fixture::target(&doc,"Current ink");
    let plan = doc.canvas_geometry_plan(&deleting([300, 260], [400, 300]), limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
    let result = editor.document();
    assert!(result.extents_cover_canvas());
    let layer = owner(result,paint);
    assert_eq!(layer.translation, Point { x: -44., y: -4. }, "rebased down by whole tiles");
    assert_eq!(paint_source(result,paint).domain,[512,512], "the minimal tile-aligned extent");
    let keys = |raster: &RasterRevision| raster.wait_data().unwrap().tiles.keys().map(|k| k.coordinate).collect::<Vec<_>>();
    assert_eq!(keys(result.target_raster(paint).unwrap()), [[0, 0], [0, 1], [1, 0], [1, 1]]);
    assert_eq!(keys(result.target_raster(mask_target(&doc,"Current ink")).unwrap()), [[0, 0], [0, 1], [1, 0], [1, 1]]);
    let old=paint_source(&doc,paint).raster.wait_data().unwrap();
    let new=paint_source(result,paint).raster.wait_data().unwrap();
    for (key, tile) in &new.tiles {
        let was = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + 1, key.coordinate[1] + 1] };
        assert!(old.tiles[&was].same_capture(tile), "kept tiles are shared");
    }
    let strips: Vec<_> = plan.operations.iter().map(|(id, op)| {
        assert_eq!(*id, paint, "masks are trimmed but never erased");
        assert!(matches!(op.kind, RasterOperationKind::Erase { alpha_locked: false }));
        assert!(op.bounds(result.target_extent(paint)).max.y <= 512.);
        let bounds = op.coverage.source.initial.as_ref().unwrap().coverage_bounds();
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
    let center = Point { x: doc.composition().size[0] as f32 / 2., y: doc.composition().size[1] as f32 / 2. };
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
    fixture::occurrence_mut(&mut doc,"Current ink").locked = true;
    let geometry = straighten(&doc, 0.2, CanvasRect { origin: [40, 30], size: [430, 190] }, false);
    let to_canvas = geometry.to_canvas();
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
    let result = editor.document().clone();
    assert!(result.extents_cover_canvas());
    assert_eq!(result.composition().size, [430, 190]);
    let paint = fixture::target(&doc,"Current ink");
    let mask = mask_target(&doc,"Current ink");
    let child = fixture::target(&doc,"Child");
    let targets: Vec<_> = plan.operations.iter().map(|(id, _)| *id).collect();
    assert_eq!(targets, [child, paint, mask], "the locked layer and its mask follow");
    for (id, op) in &plan.operations {
        let RasterOperationKind::Transform(transform) = &op.kind else { panic!("a resample") };
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
    let saved=fixture::id(&doc,"Saved");
    let before = saved_selection(&doc,saved);
    let moved = saved_selection(&result,saved);
    for p in [Point { x: 100., y: 100. }, Point { x: 200., y: 150. }] {
        near(moved.affine.map(before.affine.inverse().unwrap().map(p)), to_canvas.map(p));
    }
    assert_eq!(result.working.selection, Some(doc.working.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
    assert_eq!(result.rulers().next().unwrap().geometry, doc.rulers().next().unwrap().geometry.transformed(to_canvas));
    let (start, _) = result.rulers().next().unwrap().geometry.handles();
    near(start, to_canvas.map(Point { x: 1., y: 2. }));
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
}

#[test]
fn straightening_turns_a_photo_placement_without_touching_its_pixels() {
    let mut doc = fixture();
    fixture::insert_paint(&mut doc,"Photo",0,None);
    let photo=fixture::target(&doc,"Photo");
    fixture::paint_mut(&mut doc,"Photo").original=Some(Arc::new(photo_source([300,200])));
    fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    fixture::occurrence_mut(&mut doc,"Photo").translation=Point{x:20.,y:10.};
    let photo_handle=fixture::id(&doc,"Photo");
    let mask_handle=fixture::add_mask(&mut doc,photo_handle,[300,200],Point{x:20.,y:10.});
    fixture::occurrence_mut(&mut doc,"Photo").mask.as_mut().unwrap().linked=false;
    let geometry = straighten(&doc, -0.3, CanvasRect { origin: [10, 5], size: [490, 240] }, false);
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    assert!(plan.operations.iter().all(|(id, _)| *id != photo && SourceTarget::Coverage(mask_handle) != *id));
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    let result = editor.document();
    let before = owner(&doc,photo);
    assert!(Arc::ptr_eq(paint_source(&doc,photo).original.as_ref().unwrap(),paint_source(result,photo).original.as_ref().unwrap()));
    assert_eq!(result.target_raster(photo),doc.target_raster(photo),"losslessly placed");
    let mask=SourceTarget::Coverage(before.mask.as_ref().unwrap().source);
    for id in [photo, mask] {
        for p in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
            near(result.affine_edit_transform(id).unwrap().map(p), geometry.to_canvas().map(doc.affine_edit_transform(id).unwrap().map(p)));
        }
    }
}

#[test]
fn straightening_with_deleted_pixels_frames_the_canvas_and_erases_beyond_it() {
    let doc = tiled();
    let paint = fixture::target(&doc,"Current ink");
    let geometry = straighten(&doc, 0.1, CanvasRect { origin: [100, 80], size: [800, 600] }, true);
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
    let result = editor.document();
    assert_eq!(owner(result,paint).translation, Point::default());
    let extent = result.target_extent(paint);
    let kinds: Vec<_> = plan.operations.iter().filter(|(id, _)| *id == paint).map(|(_, op)| {
        let bounds = op.coverage.source.initial.as_ref().unwrap().coverage_bounds();
        (matches!(op.kind, RasterOperationKind::Erase { .. }), [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y])
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

fn with_photo() -> (Document, SourceTarget) {
    let mut doc = fixture();
    fixture::insert_paint(&mut doc,"Photo",0,None);
    let photo=fixture::target(&doc,"Photo");
    fixture::paint_mut(&mut doc,"Photo").original=Some(Arc::new(photo_source([300,200])));
    fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    fixture::occurrence_mut(&mut doc,"Photo").translation=Point{x:20.,y:10.};
    (doc, photo)
}

fn transforms(plan: &CanvasGeometryPlan) -> Vec<(SourceTarget, ImageTransform)> {
    plan.operations
        .iter()
        .map(|(id, op)| match &op.kind {
            RasterOperationKind::Transform(transform) => (*id, transform.clone()),
            kind => panic!("a resample, not {kind:?}"),
        })
        .collect()
}

#[test]
fn flips_and_turns_move_pixel_centres_onto_pixel_centres_in_one_step() {
    let (doc, photo) = with_photo();
    for orientation in ORIENTATIONS {
        let geometry = CanvasGeometry::orient(doc.composition().size, orientation);
        let quarter = matches!(orientation, ImageOrientation::RotateLeft | ImageOrientation::RotateRight);
        let to_canvas = geometry.to_canvas();
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
        let result = editor.document();
        assert!(result.extents_cover_canvas());
        assert_eq!(result.composition().size, if quarter { [256, 512] } else { [512, 256] });
        let resampled = transforms(&plan);
        assert_eq!(resampled.len(), 3, "the child, the paint layer and its mask");
        for (id, transform) in resampled {
            assert_eq!(transform.placement.interpolation, Interpolation::Nearest);
            let map = transform.as_affine().unwrap();
            assert!(map.0.iter().all(|v| v.fract() == 0.), "{orientation:?} {map:?}");
            let extent = result.target_extent(id);
            if quarter && id == fixture::target(&doc,"Current ink") {
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
        assert!(result.target_raster(photo) == doc.target_raster(photo), "photos only move");
        assert_eq!(result.working.selection, Some(doc.working.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
        assert_eq!(result.rulers().next().unwrap().geometry, doc.rulers().next().unwrap().geometry.transformed(to_canvas));
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
        let paint = fixture::target(&doc,"Current ink");
        let mask = mask_target(&doc,"Current ink");
        let samples = [Point { x: 0.5, y: 0.5 }, Point { x: 300.5, y: 200.5 }];
        let mut tracked: Vec<(SourceTarget, Point)> = [paint, mask].into_iter().flat_map(|id| samples.map(|p| (id, p))).collect();
        for _ in 0..times {
            let current = editor.document().clone();
            let plan = current.canvas_geometry_plan(&CanvasGeometry::orient(current.composition().size, orientation), limits()).unwrap();
            let maps: BTreeMap<_, _> = transforms(&plan).into_iter().map(|(id, t)| (id, t.as_affine().unwrap())).collect();
            for (id, p) in &mut tracked {
                *p = maps[id].map(*p);
            }
            editor.perform(Edit::Batch(plan.edits)).unwrap();
        }
        let result = editor.document();
        assert_eq!(result.composition().size, doc.composition().size);
        for (id, p) in tracked.iter().zip(samples.iter().cycle()).map(|((id, p), start)| (*id, (*p, *start))) {
            near(result.affine_edit_transform(id).unwrap().map(p.0), doc.affine_edit_transform(id).unwrap().map(p.1));
        }
        for p in [Point { x: 0., y: 0. }, Point { x: 300., y: 200. }] {
            near(result.affine_edit_transform(photo).unwrap().map(p), doc.affine_edit_transform(photo).unwrap().map(p));
        }
        let selection = result.working.selection.as_ref().unwrap();
        for p in [Point { x: 100., y: 100. }, Point { x: 200., y: 200. }] {
            near(selection.affine.map(p), doc.working.selection.as_ref().unwrap().affine.map(p));
        }
        let (start, end) = result.rulers().next().unwrap().geometry.handles();
        let (was_start, was_end) = doc.rulers().next().unwrap().geometry.handles();
        near(start, was_start);
        near(end.unwrap(), was_end.unwrap());
    }
}

#[test]
fn a_quarter_turn_swaps_the_resolution_and_a_flip_keeps_it() {
    let mut doc = fixture();
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().resolution = Some(ImageResolution { unit: ResolutionUnit::Inch, density: [[300, 1], [150, 1]] });
    let turn = doc.canvas_geometry_plan(&CanvasGeometry::orient([512, 256], ImageOrientation::RotateLeft), limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(turn.edits)).unwrap();
    assert_eq!(editor.document().composition().resolution.unwrap().density, [[150, 1], [300, 1]]);
    assert!(editor.undo().unwrap());
    assert_eq!(editor.document().composition().resolution, doc.composition().resolution);
    let flip = doc.canvas_geometry_plan(&CanvasGeometry::orient([512, 256], ImageOrientation::FlipVertical), limits()).unwrap();
    assert!(!flip.edits.iter().any(|e|matches!(e,Edit::Composition(c) if c.value.as_ref().is_some_and(|v|v.resolution!=doc.composition().resolution))));
}

#[test]
fn setting_the_resolution_is_one_undoable_metadata_edit() {
    let doc = fixture();
    let mut editor = Editor::new(doc.clone());
    let resolution = ImageResolution::ppi(240);
    let edit = resolution_edit(&doc,Some(resolution));
    assert!(!edit.changes_image(&doc));
    editor.perform(edit).unwrap();
    assert_eq!(editor.document().composition().resolution, Some(resolution));
    assert!(editor.undo().unwrap());
    assert_eq!(editor.document().composition().resolution, None);
    let invalid = ImageResolution { unit: ResolutionUnit::Inch, density: [[0, 1], [72, 1]] };
    assert!(editor.perform(resolution_edit(editor.document(),Some(invalid))).is_err());
}

#[test]
fn image_size_scales_layers_placements_selections_guides_and_pixel_distances() {
    let (mut doc, photo) = with_photo();
    let effect=fixture::insert_paint(&mut doc,"Blur",0,None);fixture::effect(&mut doc,"Blur","gaussian_blur");
    let h=doc.scene().effect_handle(effect).unwrap();let definition=doc.artwork.effects.get(h).unwrap().definition;
    let program=doc.artwork.definitions.get(definition).unwrap().program.clone();
    let index=program.parameters.iter().position(|p|p.key.as_ref()=="sigma").unwrap();
    doc.artwork.effects.get_mut(h).unwrap().values[index]=EffectValue::Number(3.);
    let sigma=|doc:&Document|match doc.scene().effect(effect).unwrap().value("sigma") {Some(EffectValue::Number(v))=>*v,_=>panic!("a number")};
    for (size, expected) in [([256, 128], 1.5), ([2048, 1024], 12.), ([5120, 2560], 30.)] {
        let geometry = CanvasGeometry::resize([512, 256], size, Interpolation::Lanczos);
        let to_canvas = geometry.to_canvas();
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(plan.edits.clone())).unwrap();
        let result = editor.document();
        assert_eq!(result.composition().size, size);
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
        assert_eq!(result.working.selection, Some(doc.working.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
        assert_eq!(result.rulers().next().unwrap().geometry, doc.rulers().next().unwrap().geometry.transformed(to_canvas));
        assert_eq!(result.composition().resolution, doc.composition().resolution);
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
            let mut doc = Document::new(PortableId::random(), 256, 128, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            let id = fixture::target(&doc,"Current ink");
            let mut data = raster(RasterPlane::Color, doc.composition().color, &[[0, 0], [3, 2]]).wait_data().unwrap().as_ref().clone();
            for plane in [RasterPlane::WatercolorWetness] {
                data.tiles.insert(TileKey { plane, coordinate: [3, 2] }, tile(plane, doc.composition().color, 17));
            }
            data.watercolor = Some(crate::raster::RasterWatercolor { wet_edge: 0.8, burnt_edge: 0.6, edge_width: 7. });
            fixture::paint_mut(&mut doc,"Current ink").raster = RasterRevision::backed(data);
            fixture::paint_mut(&mut doc,"Current ink").domain=[1024,768];
            fixture::occurrence_mut(&mut doc,"Current ink").placement = LayerPlacement::from_affine(map);
            fixture::occurrence_mut(&mut doc,"Current ink").translation = Point { x: -20., y: 15. };
            let owner=fixture::id(&doc,"Current ink");let mask_handle=fixture::add_mask(&mut doc,owner,[1024,768],Point{x:-20.,y:15.});
            fixture::occurrence_mut(&mut doc,"Current ink").mask.as_mut().unwrap().linked=linked;
            let mask=doc.artwork.coverage.get_mut(mask_handle).unwrap();mask.default_coverage=0.;
            mask.initial=Some(Selection::polygon(vec![Point{x:11.,y:9.},Point{x:71.,y:9.},Point{x:71.,y:49.}]).unwrap());
            let color=doc.composition().color;doc.artwork.coverage.get_mut(mask_handle).unwrap().raster=raster(RasterPlane::Mask,color,&[[0,0],[3,2]]);
            let mask_id=SourceTarget::Coverage(mask_handle);
            let before_source=paint_source(&doc,id).clone();
            let mut editor = Editor::new(doc.clone());
            let edits = doc.paint_extent_plan(&[id], limits()).unwrap();
            assert!(!edits.is_empty());
            assert!(editor.next_history_edit(false).is_none(), "planning adds no history");
            same_state(editor.document(), &doc);
            editor.perform(Edit::Batch(edits)).unwrap();
            let result = editor.document();
            assert_eq!(result.composition().size, [256, 128]);
            assert_eq!(result.working.selection, doc.working.selection);
            assert_eq!(result.rulers().collect::<Vec<_>>(),doc.rulers().collect::<Vec<_>>());
            let old = before_source.raster.wait_data().unwrap();
            let new=paint_source(result,id).raster.wait_data().unwrap();
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
            let new_mask=mask_source(result,mask_id);
            let old_mask=mask_source(&doc,mask_id);
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
    let mut doc = Document::new(PortableId::random(), 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let id = fixture::target(&doc,"Current ink");
    fixture::occurrence_mut(&mut doc,"Current ink").placement = LayerPlacement::from_affine(Affine([0.5, 0., 0., 0.5, 20., 30.]));
    fixture::insert_paint(&mut doc,"Unselected",0,None);
    fixture::occurrence_mut(&mut doc,"Unselected").placement=LayerPlacement::from_affine(Affine([1./128.,0.,0.,1./128.,0.,0.]));
    let other=fixture::target(&doc,"Unselected");let unrelated=fixture::occurrence(&doc,"Unselected").clone();let unrelated_source=fixture::paint(&doc,"Unselected").clone();
    fixture::insert_paint(&mut doc,"Photo",0,None);
    fixture::paint_mut(&mut doc,"Photo").original=Some(Arc::new(photo_source([300,200])));fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    fixture::occurrence_mut(&mut doc,"Photo").placement=LayerPlacement::from_affine(Affine([0.1,0.,0.,0.1,100.,50.]));
    let photo=fixture::target(&doc,"Photo");let source=fixture::occurrence(&doc,"Photo").clone();let photo_source=fixture::paint(&doc,"Photo").clone();
    let edits = doc.paint_extent_plan(&[id, photo], limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(edits)).unwrap();
    assert_eq!(fixture::occurrence(editor.document(),"Unselected"),&unrelated);
    assert_eq!(fixture::paint(editor.document(),"Unselected"),&unrelated_source);
    assert_eq!(fixture::occurrence(editor.document(),"Photo"),&source);
    assert_eq!(fixture::paint(editor.document(),"Photo"),&photo_source);
    assert!(editor.document().target_extent(id)[0] > 512);
    assert_eq!(editor.document().composition().size, [512, 256]);
    assert!(doc.validate_paint_extents(&[id], limits()).is_ok());
    assert_eq!(doc.validate_paint_extents(&[other], limits()), Err(CanvasGeometryError::ExtentTooLarge { limit: 32768 }));
}

#[test]
fn paint_extent_plan_cap_refusal_and_identity_are_atomic_and_add_no_history() {
    let mut doc = Document::new(PortableId::random(), 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let id = fixture::target(&doc,"Current ink");
    assert!(doc.paint_extent_plan(&[id], limits()).unwrap().is_empty());
    assert!(doc.validate_paint_extents(&[id], limits()).is_ok());
    fixture::occurrence_mut(&mut doc,"Current ink").placement = LayerPlacement::from_affine(Affine([1. / 128., 0., 0., 1. / 128., 0., 0.]));
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
