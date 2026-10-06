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
        depth: color::SampleDepth::U16,
        profile: color::ColorProfile::Icc(vec![9; 32].into()),
        profile_assumed: false,
    }, 8 * 1024 * 1024).unwrap();
    for y in 0..extent[1] {
        let row: Vec<u8> = (0..extent[0]).flat_map(|x| [x as u16, y as u16, 200, 65535].into_iter().flat_map(u16::to_le_bytes)).collect();
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}
fn photo_sample(image: &color::source::SourceImage, [x, y]: [u32; 2]) -> [u16; 2] {
    let mut row = vec![0; image.row_bytes()];
    image.rows().read(y, &mut row).unwrap();
    let at = x as usize * 8;
    [u16::from_le_bytes([row[at], row[at + 1]]), u16::from_le_bytes([row[at + 2], row[at + 3]])]
}
/// Finish a plan's sample moves as the worker would, with blank resampled
/// photos in their own interpretation.
fn remapped(doc: &Document, plan: CanvasGeometryPlan) -> CanvasGeometryPlan {
    let Some(remap) = plan.remap_plan(doc) else { return plan };
    let resampled = remap.resamples().map(|(target, image, extent, _, _)| {
        let mut builder = color::source::SourceBuilder::new(extent, image.interpretation.clone(), 1 << 26).unwrap();
        for _ in 0..extent[1] { builder.push_row(&vec![0; image.interpretation.pixel_bytes() * extent[0] as usize]).unwrap(); }
        (target, Image::new(Arc::new(builder.finish().unwrap())))
    }).collect();
    let results = remap.run(&doc.artwork, &resampled, &Default::default()).unwrap();
    plan.with_remapped(results).unwrap()
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
    let ink=fixture::id(&doc,"Current ink");let mask=fixture::add_mask(&mut doc,ink,[512,256],[0;2]);
    let coverage=doc.artwork.coverage.get_mut(mask).unwrap();
    coverage.raster=raster(RasterPlane::Mask,color,&[[1,0]]);
    coverage.default_coverage=0.;
    fixture::nest(&mut doc,"Group",&["Child"]);
    fixture::occurrence_mut(&mut doc,"Group").offset=[5,7];
    fixture::occurrence_mut(&mut doc,"Child").offset=[-5,-7];
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
    records!(stacks,occurrences,paint,coverage,selections,guides,effects,outputs);
}

fn document_point(doc: &Document, id: SourceTarget, local: Point) -> Point {
    doc.local_to_document(id).map(local)
}

#[test]
fn growing_left_and_up_rebases_whole_tiles_with_their_masks() {
    let doc = fixture();
    let paint = fixture::target(&doc,"Current ink");
    let mask = mask_target(&doc,"Current ink");
    let before_source=paint_source(&doc,paint).clone();
    let editor = apply(&doc, rect([-100, -300], [612, 556]));
    let after = owner(editor.document(),paint);
    assert_eq!(after.offset, [-156, -212]);
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
    let new_mask_use=after.mask.as_ref().unwrap();
    assert_eq!(new_mask_use.offset, [0, 0]);
    assert!(new_mask.raster.wait_data().unwrap().tiles.keys().all(|k| k.coordinate == [2, 2]));
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
        if doc.scene().parent(handle).is_some() || !old.positioned() {
            assert_eq!(new.offset, old.offset, "{}", old.name);
            continue;
        }
        assert_eq!(new.offset, [old.offset[0] - 40, old.offset[1] - 30]);
        let world = |d: &Document| d.layer_offset(handle);
        assert_eq!(world(result)[0], world(&doc)[0] - 40);
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
    assert_eq!(editor.next_history_edit(false).and_then(|edit|edit.view_origin_shift(editor.document().working.view_origin)), Some([300, 20]));
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
    assert_eq!(editor.document().artwork.paint,doc.artwork.paint);
    assert_eq!(editor.next_history_edit(true).and_then(|edit|edit.view_origin_shift(editor.document().working.view_origin)), Some([-300, -20]));
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
    singular.linear = Affine64([2., 0., 4., 0., 0., 0.]);
    assert!(matches!(refuse(singular, limits()), CanvasGeometryError::Unsupported(_)));
}

#[test]
fn locked_layers_follow_and_photos_rebase_with_their_base() {
    let mut doc = fixture();
    fixture::occurrence_mut(&mut doc,"Current ink").locked = true;
    fixture::insert_paint(&mut doc,"Photo",0,None);
    let photo=fixture::target(&doc,"Photo");
    fixture::paint_mut(&mut doc,"Photo").base=Some(PaintBase::new(Arc::new(photo_source([300,200])).into()));
    fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    let editor = apply(&doc, rect([-20, -20], [700, 300]));
    let result = editor.document();
    assert_eq!(fixture::occurrence(result,"Current ink").offset, [-236, -236]);
    let placed = owner(result,photo);
    assert_eq!(placed.offset, [-236, -236]);
    assert_eq!(paint_source(result,photo).domain,[936,536]);
    assert_eq!(paint_source(result,photo).base.as_ref().unwrap().offset,[256,256]);
    assert!(Arc::ptr_eq(paint_source(result,photo).base.as_ref().unwrap().image.storage(),paint_source(&doc,photo).base.as_ref().unwrap().image.storage()));
    for local in [Point { x: 0., y: 0. }, Point { x: 299., y: 199. }] {
        assert_eq!(document_point(result, photo, Point { x: local.x + 256., y: local.y + 256. }), Point { x: document_point(&doc, photo, local).x + 20., y: document_point(&doc, photo, local).y + 20. });
    }
}

fn tiled() -> Document {
    let mut doc=fixture::document([1024,768],&["Current ink"]);let color=doc.composition().color;
    let keys:Vec<_>=(0..3).flat_map(|y|(0..4).map(move |x|[x,y])).collect();
    fixture::paint_mut(&mut doc,"Current ink").raster=raster(RasterPlane::Color,color,&keys);
    let owner=fixture::id(&doc,"Current ink");let mask=fixture::add_mask(&mut doc,owner,[1024,768],[0;2]);
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
    assert_eq!(layer.offset, [-44, -4], "rebased down by whole tiles");
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
        let bounds = op.coverage.selection.as_ref().unwrap().coverage_bounds();
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
        linear: CanvasGeometry::rotation([f64::from(center.x), f64::from(center.y)], -f64::from(angle)),
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
        let after = result.local_to_document(*id).inverse().unwrap();
        let new_extent = result.target_extent(*id);
        for corner in Rect::from_extent(extent).corners() {
            let expected = after.map(to_canvas.map(doc.local_to_document(*id).map(corner)));
            near(map.map(corner), expected);
            assert!(expected.x >= -0.01 && expected.y >= -0.01, "{expected:?}");
            assert!(expected.x <= new_extent[0] as f32 + 0.01 && expected.y <= new_extent[1] as f32 + 0.01);
        }
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
fn straightening_resamples_photos_and_unlinked_masks_into_frames_that_keep_their_corners() {
    let mut doc = fixture();
    fixture::insert_paint(&mut doc,"Photo",0,None);
    let photo=fixture::target(&doc,"Photo");
    fixture::paint_mut(&mut doc,"Photo").base=Some(PaintBase::new(Arc::new(photo_source([300,200])).into()));
    fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    fixture::occurrence_mut(&mut doc,"Photo").offset=[20,10];
    let photo_handle=fixture::id(&doc,"Photo");
    let mask_handle=fixture::add_mask(&mut doc,photo_handle,[300,200],[20,10]);
    fixture::occurrence_mut(&mut doc,"Photo").mask.as_mut().unwrap().linked=false;
    let color=doc.composition().color;doc.artwork.coverage.get_mut(mask_handle).unwrap().raster=raster(RasterPlane::Mask,color,&[[0,0]]);
    let geometry = straighten(&doc, -0.3, CanvasRect { origin: [10, 5], size: [490, 240] }, false);
    let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
    let mask = SourceTarget::Coverage(mask_handle);
    let [spec] = plan.remaps[..] else { panic!("the untouched photo is resampled in its own interpretation") };
    let Some(BaseRemap::Resample { extent, to_image, .. }) = spec.base else { panic!("a resampled photo") };
    assert_eq!(spec.target, photo);
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(remapped(&doc, plan.clone()).edits)).unwrap();
    let result = editor.document();
    let base = paint_source(result, photo).base.clone().unwrap();
    assert_eq!((base.policy, base.image.extent, base.image.interpretation.clone()), (PaintBasePolicy::SourceProfile, extent, paint_source(&doc, photo).base.as_ref().unwrap().image.interpretation.clone()));
    for corner in [[0., 0.], [300., 0.], [0., 200.], [300., 200.]] {
        let [x, y] = to_image.map(corner);
        assert!(x >= -0.01 && y >= -0.01 && x <= extent[0] as f64 + 0.01 && y <= extent[1] as f64 + 0.01, "{corner:?} lands in the replacement image");
        let local = Point { x: (x + f64::from(base.offset[0])) as f32, y: (y + f64::from(base.offset[1])) as f32 };
        near(result.local_to_document(photo).map(local), geometry.to_canvas().map(doc.local_to_document(photo).map(Point { x: corner[0] as f32, y: corner[1] as f32 })));
    }
    let resampled = transforms(&plan);
    assert!(resampled.iter().all(|(target, _)| *target != photo), "the photo is not folded into paint");
    let (_, transform) = resampled.iter().find(|(target, _)| *target == mask).unwrap();
    let map = transform.as_affine().unwrap();
    let new_extent = result.target_extent(mask);
    for corner in Rect::from_extent([300, 200]).corners() {
        near(result.local_to_document(mask).map(map.map(corner)), geometry.to_canvas().map(doc.local_to_document(mask).map(corner)));
        let local = map.map(corner);
        assert!(local.x >= -0.01 && local.y >= -0.01 && local.x <= new_extent[0] as f32 + 0.01 && local.y <= new_extent[1] as f32 + 0.01);
    }
    assert!(!owner(result,photo).mask.as_ref().unwrap().linked);
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
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
    assert_eq!(owner(result,paint).offset, [0, 0]);
    let extent = result.target_extent(paint);
    let kinds: Vec<_> = plan.operations.iter().filter(|(id, _)| *id == paint).map(|(_, op)| {
        let bounds = op.coverage.selection.as_ref().unwrap().coverage_bounds();
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
    fixture::paint_mut(&mut doc,"Photo").base=Some(PaintBase::new(Arc::new(photo_source([300,200])).into()));
    fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    fixture::occurrence_mut(&mut doc,"Photo").offset=[20,10];
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
    let SourceTarget::Paint(photo_paint) = photo else { panic!("paint") };
    let original = paint_source(&doc, photo).base.clone().unwrap();
    for orientation in ORIENTATIONS {
        let geometry = CanvasGeometry::orient(doc.composition().size, orientation);
        let quarter = matches!(orientation, ImageOrientation::RotateLeft | ImageOrientation::RotateRight);
        let to_canvas = geometry.to_canvas();
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let resampled = transforms(&plan);
        assert_eq!(resampled.len(), 3, "the child, the paint layer and its mask");
        assert_eq!(plan.remaps.len(), 1, "the photo moves its samples exactly");
        let spec = plan.remaps[0];
        assert_eq!(spec.target, SourceTarget::Paint(photo_paint));
        assert!(spec.map.tile_aligned(), "{orientation:?}: every photo tile moves to one tile");
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(remapped(&doc, plan.clone()).edits)).unwrap();
        let result = editor.document();
        assert!(result.extents_cover_canvas());
        assert_eq!(result.composition().size, if quarter { [256, 512] } else { [512, 256] });
        for (id, transform) in resampled {
            assert_eq!(transform.placement.interpolation, Interpolation::Nearest);
            let map = transform.as_affine().unwrap();
            assert!(map.0.iter().all(|v| v.fract() == 0.), "{orientation:?} {map:?}");
            let extent = result.target_extent(id);
            if quarter && id == fixture::target(&doc,"Current ink") {
                assert_eq!(extent, [512, 512], "a square scratch extent holds both orientations");
            }
            let before = doc.target_extent(id).map(|v| v as f32);
            for p in [Point { x: 0.5, y: 0.5 }, Point { x: before[0] - 0.5, y: before[1] - 0.5 }, Point { x: 100.5, y: 7.5 }] {
                let q = map.map(p);
                assert_eq!([q.x.fract(), q.y.fract()], [0.5, 0.5]);
                assert!(q.x > 0. && q.y > 0. && q.x < extent[0] as f32 && q.y < extent[1] as f32);
                near(result.local_to_document(id).map(q), to_canvas.map(doc.local_to_document(id).map(p)));
            }
        }
        let base = paint_source(result, photo).base.clone().unwrap();
        assert_eq!(base.image.interpretation, original.image.interpretation, "the photo keeps its own interpretation");
        assert_ne!(base.image.id(), original.image.id(), "a turned base is a new image");
        assert_eq!(base.image.extent, if quarter { [200, 300] } else { [300, 200] });
        for p in [[0u32, 0u32], [299, 199], [117, 43]] {
            let before = Point { x: p[0] as f32 + 0.5 + original.offset[0] as f32, y: p[1] as f32 + 0.5 + original.offset[1] as f32 };
            let doc_point = to_canvas.map(doc.local_to_document(photo).map(before));
            let local = result.local_to_document(photo).inverse().unwrap().map(doc_point);
            let image = [local.x - base.offset[0] as f32, local.y - base.offset[1] as f32].map(|v| v.floor() as u32);
            assert_eq!(photo_sample(&base.image, image), [p[0] as u16, p[1] as u16], "{orientation:?}: sample {p:?}");
        }
        assert_eq!(result.working.selection, Some(doc.working.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
        assert_eq!(result.rulers().next().unwrap().geometry, doc.rulers().next().unwrap().geometry.transformed(to_canvas));
        assert!(editor.undo().unwrap());
        same_state(editor.document(), &doc);
    }
}

#[test]
fn turning_four_times_or_flipping_twice_puts_every_pixel_back() {
    let (doc, photo) = with_photo();
    let original = paint_source(&doc, photo).base.clone().unwrap();
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
        let samples = [Point { x: 0.5, y: 0.5 }, Point { x: 299.5, y: 199.5 }];
        let mut tracked: Vec<(SourceTarget, Point)> = [paint, mask].into_iter().flat_map(|id| samples.map(|p| (id, p))).collect();
        for _ in 0..times {
            let current = editor.document().clone();
            let plan = current.canvas_geometry_plan(&CanvasGeometry::orient(current.composition().size, orientation), limits()).unwrap();
            let maps: BTreeMap<_, _> = transforms(&plan).into_iter().map(|(id, t)| (id, t.as_affine().unwrap())).collect();
            for (id, p) in &mut tracked {
                *p = maps[id].map(*p);
            }
            editor.perform(Edit::Batch(remapped(&current, plan).edits)).unwrap();
        }
        let result = editor.document();
        assert_eq!(result.composition().size, doc.composition().size);
        for (id, p) in tracked.iter().zip(samples.iter().cycle()).map(|((id, p), start)| (*id, (*p, *start))) {
            near(result.local_to_document(id).map(p.0), doc.local_to_document(id).map(p.1));
        }
        let base = paint_source(result, photo).base.clone().unwrap();
        assert_eq!(base.image.extent, original.image.extent);
        assert_eq!(result.scene().layer_origin(result.scene().source_owner(photo)) [0] + i64::from(base.offset[0]),
            doc.scene().layer_origin(doc.scene().source_owner(photo))[0] + i64::from(original.offset[0]), "{orientation:?}: the photo returns");
        for p in [[0u32, 0u32], [299, 199], [131, 77]] {
            assert_eq!(photo_sample(&base.image, p), [p[0] as u16, p[1] as u16]);
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
fn image_size_scales_layers_selections_guides_and_effect_reference() {
    let (mut doc, photo) = with_photo();
    let effect=fixture::insert_paint(&mut doc,"Blur",0,None);fixture::effect(&mut doc,"Blur","gaussian_blur");
    let h=doc.scene().effect_handle(effect).unwrap();let program=doc.artwork.effects.get(h).unwrap().program.clone();
    let index=program.parameters.iter().position(|p|p.key.as_ref()=="sigma").unwrap();
    doc.artwork.effects.get_mut(h).unwrap().values[index]=EffectValue::Number(3.);
    let sigma=|doc:&Document|match doc.scene().effect(effect).unwrap().value("sigma") {Some(EffectValue::Number(v))=>*v,_=>panic!("a number")};
    let mut edited = doc.clone();
    let color = doc.composition().color;
    fixture::paint_mut(&mut edited, "Photo").raster = raster(RasterPlane::Color, color, &[[0, 0]]);
    for (doc, size) in [(&doc, [256, 128]), (&edited, [256, 128]), (&doc, [2048, 1024]), (&edited, [5120, 2560])] {
        let untouched = paint_source(doc, photo).raster.wait_data().unwrap().tiles.is_empty();
        let geometry = CanvasGeometry::resize([512, 256], size, Interpolation::Lanczos);
        let to_canvas = geometry.to_canvas();
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(remapped(doc, plan.clone()).edits)).unwrap();
        let result = editor.document();
        assert_eq!(result.composition().size, size);
        assert!(result.extents_cover_canvas());
        assert_eq!(sigma(result), 3.);
        let reference=result.artwork.effects.get(h).unwrap().spatial.unwrap();
        assert_eq!(reference.extent, [512.,256.]);
        assert_eq!(reference.mapping, geometry.to_canvas64());
        if untouched {
            let base = paint_source(result, photo).base.clone().unwrap();
            let scale = f64::from(size[0]) / 512.;
            assert_eq!(base.image.extent, [300, 200].map(|v: u32| (f64::from(v) * scale) as u32), "the photo keeps its own samples, resampled");
            assert_eq!(base.image.interpretation, paint_source(doc, photo).base.as_ref().unwrap().image.interpretation);
            assert!(transforms(&plan).iter().all(|(id, _)| *id != photo));
        } else {
            assert!(paint_source(result, photo).base.is_none(), "an edited photo folds into the layer's paint");
            let (_, folded) = transforms(&plan).into_iter().find(|(id, _)| *id == photo).unwrap();
            assert_eq!(folded.source_base, paint_source(doc, photo).base.clone(), "the resample reads the photo it removes");
        }
        for (id, transform) in transforms(&plan) {
            assert_eq!(transform.placement.interpolation, Interpolation::Lanczos);
            for p in [Point { x: 0., y: 0. }, Point { x: 256., y: 256. }] {
                near(result.local_to_document(id).map(transform.as_affine().unwrap().map(p)), to_canvas.map(doc.local_to_document(id).map(p)));
            }
        }
        assert_eq!(result.working.selection, Some(doc.working.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
        assert_eq!(result.rulers().next().unwrap().geometry, doc.rulers().next().unwrap().geometry.transformed(to_canvas));
        assert_eq!(result.composition().resolution, doc.composition().resolution);
        assert!(editor.undo().unwrap());
        assert_eq!(sigma(editor.document()), 3.);
        same_state(editor.document(), doc);
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
fn paint_extent_plan_covers_the_canvas_and_preserves_hidden_material_and_masks() {
    for offset in [[200, 120], [-1300, 90], [140, -900]] {
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
            fixture::occurrence_mut(&mut doc,"Current ink").offset = offset;
            let owner=fixture::id(&doc,"Current ink");let mask_handle=fixture::add_mask(&mut doc,owner,[1024,768],[-20,15]);
            fixture::occurrence_mut(&mut doc,"Current ink").mask.as_mut().unwrap().linked=linked;
            doc.artwork.coverage.get_mut(mask_handle).unwrap().default_coverage=0.;
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
            assert!(delta.x > 0. || delta.y > 0., "a canvas left of or above the layer rebases whole tiles");
            for (key, tile) in &old.tiles {
                let moved = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + moved[0], key.coordinate[1] + moved[1]] };
                assert!(new.tiles[&moved].same_capture(tile), "{offset:?} linked={linked}: hidden planes keep their captures");
            }
            let new_mask=mask_source(result,mask_id);
            let old_mask=mask_source(&doc,mask_id);
            let old_mask_data = old_mask.raster.wait_data().unwrap();
            let new_mask_data = new_mask.raster.wait_data().unwrap();
            let mask_key = old_mask_data.tiles.keys().next().unwrap();
            let mask_position = new_mask_data.tiles.iter().find(|(_, tile)| tile.same_capture(&old_mask_data.tiles[mask_key])).unwrap().0.coordinate;
            let mask_move = [mask_position[0] - mask_key.coordinate[0], mask_position[1] - mask_key.coordinate[1]];
            let mask_delta = Point { x: (mask_move[0] * TILE_SIZE) as f32, y: (mask_move[1] * TILE_SIZE) as f32 };
            for (key, tile) in &old_mask_data.tiles {
                let moved = TileKey { plane: key.plane, coordinate: [key.coordinate[0] + mask_move[0], key.coordinate[1] + mask_move[1]] };
                assert!(new_mask.raster.wait_data().unwrap().tiles[&moved].same_capture(tile));
            }
            for target in [id, mask_id] {
                let inverse = result.local_to_document(target).inverse().unwrap();
                for corner in Rect::from_extent([256, 128]).corners() {
                    let local = inverse.map(corner);
                    let extent = result.target_extent(target);
                    assert!(local.x >= -0.01 && local.y >= -0.01, "{offset:?} linked={linked}: {local:?}");
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
fn paint_extent_plan_changes_only_selected_paint_and_rebases_photos_with_their_base() {
    let mut doc = Document::new(PortableId::random(), 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let id = fixture::target(&doc,"Current ink");
    fixture::occurrence_mut(&mut doc,"Current ink").offset = [20, 30];
    fixture::insert_paint(&mut doc,"Unselected",0,None);
    fixture::occurrence_mut(&mut doc,"Unselected").offset=[40000,0];
    let other=fixture::target(&doc,"Unselected");let unrelated=fixture::occurrence(&doc,"Unselected").clone();let unrelated_source=fixture::paint(&doc,"Unselected").clone();
    fixture::insert_paint(&mut doc,"Photo",0,None);
    fixture::paint_mut(&mut doc,"Photo").base=Some(PaintBase::new(Arc::new(photo_source([300,200])).into()));fixture::paint_mut(&mut doc,"Photo").domain=[300,200];
    fixture::occurrence_mut(&mut doc,"Photo").offset=[100,50];
    let photo=fixture::target(&doc,"Photo");
    let edits = doc.paint_extent_plan(&[id, photo], limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(edits)).unwrap();
    assert_eq!(fixture::occurrence(editor.document(),"Unselected"),&unrelated);
    assert_eq!(fixture::paint(editor.document(),"Unselected"),&unrelated_source);
    assert_eq!(fixture::occurrence(editor.document(),"Photo").offset,[-156,-206]);
    assert_eq!(fixture::paint(editor.document(),"Photo").base.as_ref().unwrap().offset,[256,256]);
    assert_eq!(editor.document().target_extent(photo), [668, 462]);
    assert_eq!(fixture::occurrence(editor.document(),"Current ink").offset,[-236,-226]);
    assert_eq!(editor.document().composition().size, [512, 256]);
    assert!(editor.document().extents_cover_canvas() || editor.document().validate_paint_extents(&[other], limits()).is_err());
    assert!(doc.validate_paint_extents(&[id], limits()).is_ok());
    assert_eq!(doc.validate_paint_extents(&[other], limits()), Err(CanvasGeometryError::ExtentTooLarge { limit: 32768 }));
}
#[test]
fn paint_extent_plan_cap_refusal_and_identity_are_atomic_and_add_no_history() {
    let mut doc = Document::new(PortableId::random(), 512, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let id = fixture::target(&doc,"Current ink");
    assert!(doc.paint_extent_plan(&[id], limits()).unwrap().is_empty());
    assert!(doc.validate_paint_extents(&[id], limits()).is_ok());
    fixture::occurrence_mut(&mut doc,"Current ink").offset = [40000, 0];
    let editor = Editor::new(doc.clone());
    let error = CanvasGeometryError::ExtentTooLarge { limit: 32768 };
    assert_eq!(doc.paint_extent_plan(&[id], limits()), Err(error.clone()));
    assert_eq!(doc.validate_paint_extents(&[id], limits()), Err(error));
    same_state(editor.document(), &doc);
    assert!(editor.next_history_edit(false).is_none());
    assert!(editor.next_history_edit(true).is_none());
}

mod layer_transform_plan {
    use super::*;
    include!("transform_pixels_plan_tests.rs");
}

#[test]
fn spatial_effect_crop_orientation_resize_and_history_preserve_reference_coordinates() {
    let mut doc=fixture::document([512,256], &["Group","Effect","Paint"]);
    fixture::effect(&mut doc,"Effect","motion_blur");
    fixture::nest(&mut doc,"Group", &["Effect","Paint"]);
    fixture::occurrence_mut(&mut doc,"Group").offset=[27,-13];
    let occurrence=fixture::id(&doc,"Effect");
    let effect=doc.scene().effect_handle(occurrence).unwrap();
    let mut application=doc.artwork.effects.get(effect).unwrap().clone();
    let distance=application.program.parameters.iter().position(|p| p.key.as_ref()=="distance").unwrap();
    application.values[distance]=EffectValue::Number(17.);
    let original=application.clone();
    doc.artwork.effects.get_mut(effect).unwrap().clone_from(&application);
    let mut editor=Editor::new(doc.clone());
    let mut expected=Affine64::default();
    for geometry in [
        CanvasGeometry::crop(CanvasRect {origin:[-73,41],size:[640,192]}),
        CanvasGeometry::orient([640,192],ImageOrientation::RotateRight),
        CanvasGeometry::orient([192,640],ImageOrientation::FlipHorizontal),
        CanvasGeometry::resize([192,640],[384,320],Interpolation::Nearest),
    ] {
        let before=editor.document().clone();
        let plan=before.canvas_geometry_plan(&geometry,limits()).unwrap();
        editor.perform(Edit::Batch(plan.edits)).unwrap();
        expected=Affine64(geometry.to_canvas().0.map(f64::from)).compose(expected);
        let application=editor.document().artwork.effects.get(effect).unwrap();
        let spatial=application.spatial.unwrap();
        assert_eq!(spatial.mapping,expected);
        assert_eq!(spatial.extent,original.spatial.unwrap().extent);
        assert_eq!(application.values,original.values);
        let radius=editor.document().scene().effect(occurrence).unwrap().damage_radius().unwrap();
        let [a,b,c,d,_,_]=expected.0;
        assert_eq!(radius,(10.*(a.abs()+c.abs()).max(b.abs()+d.abs())).ceil() as u32);
        for point in [[0.,0.],[117.5,203.25],[512.,256.]] {
            let composition=expected.map(point);
            let restored=spatial.inverse().unwrap().map(composition);
            assert!((restored[0]-point[0]).abs()<1e-9 && (restored[1]-point[1]).abs()<1e-9);
        }
        let after=editor.document().clone();
        assert!(editor.undo().unwrap());same_state(editor.document(),&before);
        assert!(editor.redo().unwrap());same_state(editor.document(),&after);
    }
}

#[test]
fn effect_spatial_admission_matches_consumed_coordinates() {
    for definition in bundled_effect_catalog().filters() {
        let draft=EffectInstance::new(definition.program());
        let application=EffectApplication::new(draft.program,draft.values,[512,256]);
        application.validate().unwrap();
        assert_eq!(application.spatial.is_some(),application.program.uses_spatial_reference());
        if let Some(reference)=application.spatial {
            assert_eq!(reference.map([0.,0.]),[0.,0.]);
            let mut missing=application.clone();missing.spatial=None;
            assert!(missing.validate().is_err());
            for extent in [[0.,256.],[512.,f64::NAN],[512.,-1.]] {
                let mut invalid=application.clone();invalid.spatial.as_mut().unwrap().extent=extent;
                assert!(invalid.validate().is_err());
            }
            let mut singular=application.clone();singular.spatial.as_mut().unwrap().mapping=Affine64([1.,2.,2.,4.,0.,0.]);
            assert!(singular.validate().is_err());
        } else {
            assert!(!matches!(application.program.id.as_ref(),"gradient_fill"|"vignette"|"motion_blur"));
        }
    }
}

#[test]
fn inserting_spatial_effect_after_crop_uses_the_current_local_frame() {
    let doc=fixture::document([512,256],&["Paint"]);
    let editor=apply(&doc,rect([97,-31],[300,400]));
    let mut doc=editor.document().clone();
    assert_ne!(doc.working.view_origin,[0, 0]);
    fixture::insert_paint(&mut doc,"Vignette",0,None);
    fixture::effect(&mut doc,"Vignette","vignette");
    let occurrence=fixture::id(&doc,"Vignette");
    let reference=doc.scene().effect_application(occurrence).unwrap().spatial.unwrap();
    assert_eq!(reference.mapping,Affine64::default());
    assert_eq!(reference.extent,[300.,400.]);
    assert_eq!(reference.map([150.,200.]),[150.,200.]);
}

#[test]
fn canvas_maps_reach_image_objects_and_effect_references_in_double_precision() {
    let mut doc = fixture::document([1000, 500], &["Current ink"]);
    let (layer, edit) = doc.create_object_layer_edit("Images", None, 0).unwrap();
    doc.apply(edit).unwrap();
    let mut object = ImageObject::new(Arc::new(photo_source([30, 20])).into(), "Photo");
    object.affine = Affine64([1.25, 0.5, -0.5, 1.25, 123.375, 77.0625]);
    let (handle, edit) = doc.add_image_object_edit(layer, object.clone(), 0).unwrap();
    doc.apply(edit).unwrap();
    for (geometry, scale) in [(CanvasGeometry::resize([1000, 500], [100, 50], Interpolation::Bicubic), 0.1), (CanvasGeometry::resize([1000, 500], [3000, 1500], Interpolation::Bicubic), 3.)] {
        let plan = doc.canvas_geometry_plan(&geometry, limits()).unwrap();
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(plan.edits)).unwrap();
        let moved = editor.document().artwork.objects.get(handle).unwrap().affine;
        assert_eq!(moved, Affine64([scale, 0., 0., scale, 0., 0.]).compose(object.affine), "no single-precision rounding of {scale}");
    }
    let turned = CanvasGeometry::orient([1000, 500], ImageOrientation::RotateRight);
    assert_eq!(turned.pixel_map().unwrap().source([0, 0]), [0, 499]);
    let plan = doc.canvas_geometry_plan(&turned, limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(editor.document().artwork.objects.get(handle).unwrap().affine, turned.to_canvas64().compose(object.affine));
}

fn photo_document() -> (Document, SourceTarget) {
    let mut doc = fixture::document([512, 256], &["Current ink"]);
    fixture::insert_paint(&mut doc, "Photo", 0, None);
    let photo = fixture::target(&doc, "Photo");
    fixture::paint_mut(&mut doc, "Photo").base = Some(PaintBase::new(Arc::new(photo_source([300, 200])).into()));
    fixture::paint_mut(&mut doc, "Photo").domain = [512, 256];
    let color = doc.composition().color;
    fixture::paint_mut(&mut doc, "Photo").raster = raster(RasterPlane::Color, color, &[[0, 0], [1, 0]]);
    (doc, photo)
}

#[test]
fn deleting_cropped_pixels_recuts_a_photo_base_exactly() {
    let (doc, photo) = photo_document();
    let original = paint_source(&doc, photo).base.clone().unwrap();
    let plan = doc.canvas_geometry_plan(&deleting([260, 50], [150, 100]), limits()).unwrap();
    assert!(plan.operations.iter().all(|(id, _)| *id != photo), "the photo layer is trimmed exactly, never erased on the GPU");
    assert_eq!(plan.remaps.len(), 1);
    let plan = remapped(&doc, plan);
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    let result = editor.document();
    assert!(result.extents_cover_canvas());
    let base = paint_source(result, photo).base.clone().unwrap();
    assert_eq!(base.image.extent, [40, 100], "only the kept samples remain");
    assert_eq!(base.image.interpretation, original.image.interpretation);
    assert_eq!(photo_sample(&base.image, [0, 0]), [260, 50]);
    assert_eq!(photo_sample(&base.image, [39, 99]), [299, 149]);
    let corner = document_point(result, photo, Point { x: base.offset[0] as f32, y: base.offset[1] as f32 });
    assert_eq!(corner, Point { x: 0., y: 0. }, "the kept photo starts at the new canvas corner");
    let images = result.artwork.images().unwrap();
    assert!(images.contains_key(&base.image.id()) && !images.contains_key(&original.image.id()), "a save omits the excluded samples");
    let data = paint_source(result, photo).raster.wait_data().unwrap();
    for (key, tile) in &data.tiles {
        let bytes = tile.wait_backing().unwrap().decode().unwrap();
        let bpp = tile.descriptor().bytes_per_pixel().unwrap();
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let local = Point { x: (key.coordinate[0] * TILE_SIZE + x) as f32 + 0.5, y: (key.coordinate[1] * TILE_SIZE + y) as f32 + 0.5 };
                let at = document_point(result, photo, local);
                let inside = at.x > 0. && at.y > 0. && at.x < 150. && at.y < 100.;
                let i = ((y * TILE_SIZE + x) as usize) * bpp;
                if !inside { assert!(bytes[i..i + bpp].iter().all(|v| *v == 0), "paint outside the crop is deleted at {at:?}"); }
            }
        }
    }
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
    assert!(editor.document().artwork.images().unwrap().contains_key(&original.image.id()), "undo keeps the original photo");
}

#[test]
fn deleting_cropped_pixels_beside_a_photo_drops_its_base_and_a_whole_photo_moves_unchanged() {
    let (doc, photo) = photo_document();
    let beside = doc.canvas_geometry_plan(&deleting([320, 0], [192, 256]), limits()).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(remapped(&doc, beside).edits)).unwrap();
    assert!(paint_source(editor.document(), photo).base.is_none(), "no photo sample survives the crop");
    let original = paint_source(&doc, photo).base.clone().unwrap();
    let mut whole = doc.clone();
    whole.artwork.paint.get_mut(match photo { SourceTarget::Paint(h) => h, _ => unreachable!() }).unwrap().raster = RasterRevision::default();
    let plan = whole.canvas_geometry_plan(&deleting([0, 0], [400, 256]), limits()).unwrap();
    assert!(plan.remaps.is_empty(), "nothing to cut from a photo inside the crop");
    let mut editor = Editor::new(whole.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    assert!(paint_source(editor.document(), photo).base.as_ref().unwrap().image.same_owner(&original.image));
}

#[test]
fn an_oriented_photo_beyond_the_size_limit_is_refused_before_anything_changes() {
    let (mut doc, _) = photo_document();
    fixture::paint_mut(&mut doc, "Photo").domain = [1024, 256];
    let tight = GeometryLimits { project: ProjectLimits { dimension: 600, ..Default::default() }, device_dimension: 8192 };
    let error = doc.canvas_geometry_plan(&CanvasGeometry::orient([512, 256], ImageOrientation::RotateLeft), tight).unwrap_err();
    assert_eq!(error, CanvasGeometryError::ExtentTooLarge { limit: 600 });
}

#[test]
fn a_whole_photo_layer_flips_and_turns_exactly_with_its_linked_mask() {
    let (mut doc, photo) = photo_document();
    let owner = fixture::id(&doc, "Photo");
    fixture::occurrence_mut(&mut doc, "Photo").offset = [30, -20];
    let mask = fixture::add_mask(&mut doc, owner, [512, 256], [0; 2]);
    let color = doc.composition().color;
    doc.artwork.coverage.get_mut(mask).unwrap().raster = raster(RasterPlane::Mask, color, &[[0, 0]]);
    let flip = LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(Affine([-1., 0., 0., 1., 300., 0.])) };
    let plan = doc.exact_layer_transform_plan(photo, &flip, limits()).unwrap().unwrap();
    assert!(plan.operations.is_empty());
    assert_eq!(plan.remaps.iter().map(|spec| spec.target).collect::<Vec<_>>(), [photo, SourceTarget::Coverage(mask)]);
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(remapped(&doc, plan).edits)).unwrap();
    let result = editor.document();
    assert!(result.extents_cover_canvas());
    let base = paint_source(result, photo).base.clone().unwrap();
    for p in [[0u32, 0u32], [299, 199], [17, 140]] {
        let before = Point { x: p[0] as f32 + 0.5, y: p[1] as f32 + 0.5 };
        let target = document_point(&doc, photo, Point { x: 300. - before.x, y: before.y });
        let local = result.local_to_document(photo).inverse().unwrap().map(target);
        let image = [local.x - base.offset[0] as f32, local.y - base.offset[1] as f32].map(|v| v.floor() as u32);
        assert_eq!(photo_sample(&base.image, image), [p[0] as u16, p[1] as u16], "sample {p:?}");
    }
    let mask_corner = |doc: &Document| doc.scene().mask_origin(owner).unwrap();
    let flipped_mask = mask_corner(result);
    assert_ne!(flipped_mask, mask_corner(&doc), "the linked mask moves with the layer");
    let quarter = LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(Affine([0., 1., -1., 0., 200., 0.])) };
    assert!(doc.exact_layer_transform_plan(photo, &quarter, limits()).unwrap().is_ok());
    let fractional = LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(Affine([-1., 0., 0., 1., 300.5, 0.])) };
    assert!(doc.exact_layer_transform_plan(photo, &fractional, limits()).is_none(), "a half-pixel flip resamples instead");
    assert!(doc.exact_layer_transform_plan(fixture::target(&doc, "Current ink"), &flip, limits()).is_none(), "paint without a photo uses the ordinary transform");
    assert!(editor.undo().unwrap());
    same_state(editor.document(), &doc);
}

#[test]
fn the_widest_admitted_photo_turns_exactly_and_a_wider_turn_is_refused_atomically() {
    let width = MAX_EXTENT;
    let wide = GeometryLimits { project: ProjectLimits::default(), device_dimension: MAX_EXTENT };
    let mut doc = fixture::document([width, 256], &["Current ink"]);
    fixture::insert_paint(&mut doc, "Photo", 0, None);
    let photo = fixture::target(&doc, "Photo");
    let image = {
        use color::source::*;
        let mut builder = SourceBuilder::new([width, 256], SourceInterpretation { channels: SourceChannels::Rgb, depth: color::SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 64 * 1024 * 1024).unwrap();
        for y in 0..256u32 { builder.push_row(&(0..width).flat_map(|x| [(x / 256) as u8, y as u8, 7]).collect::<Vec<_>>()).unwrap(); }
        Arc::new(builder.finish().unwrap())
    };
    fixture::paint_mut(&mut doc, "Photo").base = Some(PaintBase::new(image.into()));
    fixture::paint_mut(&mut doc, "Photo").domain = [width, 256];
    let plan = remapped(&doc, doc.canvas_geometry_plan(&CanvasGeometry::orient([width, 256], ImageOrientation::RotateLeft), wide).unwrap());
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    let base = paint_source(editor.document(), photo).base.clone().unwrap();
    assert_eq!(base.image.extent, [256, width]);
    let mut row = vec![0; base.image.row_bytes()];
    base.image.rows().read(width - 1, &mut row).unwrap();
    assert_eq!(&row[..3], &[0, 0, 7], "the left column of the photo is now its bottom row");
    let mut wider = doc.clone();
    fixture::occurrence_mut(&mut wider, "Photo").offset = [-1, 0];
    let before = wider.clone();
    assert_eq!(wider.canvas_geometry_plan(&CanvasGeometry::orient([width, 256], ImageOrientation::RotateLeft), wide).unwrap_err(),
        CanvasGeometryError::ExtentTooLarge { limit: MAX_EXTENT });
    assert_eq!(wider, before);
}

fn cmyk_source(extent: [u32; 2]) -> color::source::SourceImage {
    use color::source::*;
    let mut builder = SourceBuilder::new(extent, SourceInterpretation { channels: SourceChannels::Cmyk, depth: color::SampleDepth::U8,
        profile: color::ColorProfile::Icc(vec![3; 64].into()), profile_assumed: false }, 1 << 24).unwrap();
    for y in 0..extent[1] { builder.push_row(&(0..extent[0]).flat_map(|x| [x as u8, y as u8, 9, 0]).collect::<Vec<_>>()).unwrap(); }
    builder.finish().unwrap()
}

#[test]
fn resampled_photos_fold_into_paint_unless_their_own_samples_can_keep_their_interpretation() {
    let resize = CanvasGeometry::resize([512, 256], [256, 128], Interpolation::Bicubic);
    let cases: [(&str, fn(&mut Document)); 3] = [
        ("CMYK", |doc| fixture::paint_mut(doc, "Photo").base = Some(PaintBase::new(Arc::new(cmyk_source([300, 200])).into()))),
        ("working pixels", |doc| fixture::paint_mut(doc, "Photo").base.as_mut().unwrap().policy = PaintBasePolicy::WorkingPixels),
        ("grayscale layer", |doc| fixture::paint_mut(doc, "Photo").color_mode = color::LayerColorMode::Grayscale),
    ];
    for (what, change) in cases {
        let (mut doc, photo) = with_photo();
        change(&mut doc);
        let plan = doc.canvas_geometry_plan(&resize, limits()).unwrap();
        assert!(plan.remaps.is_empty(), "{what}: no retained resample");
        let (_, folded) = transforms(&plan).into_iter().find(|(id, _)| *id == photo).expect(what);
        assert_eq!(folded.source_base, paint_source(&doc, photo).base.clone(), "{what}: the photo folds into working paint");
    }
    let (mut doc, photo) = with_photo();
    fixture::paint_mut(&mut doc, "Photo").base = Some(PaintBase::new(Arc::new(cmyk_source([300, 200])).into()));
    let plan = remapped(&doc, doc.canvas_geometry_plan(&CanvasGeometry::orient([512, 256], ImageOrientation::RotateRight), limits()).unwrap());
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(plan.edits)).unwrap();
    let base = paint_source(editor.document(), photo).base.clone().unwrap();
    assert_eq!(base.image.interpretation.channels, color::source::SourceChannels::Cmyk, "an exact turn keeps CMYK samples");
    assert_eq!(base.image.extent, [200, 300]);
}

#[test]
fn a_photo_shared_with_an_image_object_resamples_without_touching_the_object_image() {
    let (mut doc, photo) = with_photo();
    let image = paint_source(&doc, photo).base.as_ref().unwrap().image.clone();
    let (layer, edit) = doc.create_object_layer_edit("Images", None, 0).unwrap();
    doc.apply(edit).unwrap();
    let (object, edit) = doc.add_image_object_edit(layer, ImageObject::new(image.clone(), "Shared"), 0).unwrap();
    doc.apply(edit).unwrap();
    for geometry in [CanvasGeometry::resize([512, 256], [256, 128], Interpolation::Bicubic), CanvasGeometry::orient([512, 256], ImageOrientation::RotateLeft)] {
        let plan = remapped(&doc, doc.canvas_geometry_plan(&geometry, limits()).unwrap());
        let mut editor = Editor::new(doc.clone());
        editor.perform(Edit::Batch(plan.edits)).unwrap();
        let result = editor.document();
        assert!(!paint_source(result, photo).base.as_ref().unwrap().image.same_owner(&image), "the photo layer gets its own image");
        let shared = &result.artwork.objects.get(object).unwrap().image;
        assert!(shared.same_owner(&image) && shared.id() == image.id(), "the object keeps the original image");
        assert_eq!(result.artwork.objects.get(object).unwrap().affine, geometry.to_canvas64().compose(Affine64::default()));
    }
}
