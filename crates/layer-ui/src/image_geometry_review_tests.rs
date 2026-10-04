#[test]
fn retained_move_uses_only_checked_geometry_source_frame_and_mask_linkage() {
    for warped in [false,true] {
        let mut s=session(Platform::Gtk);
        let checked=occurrence_handle(1).unwrap();
        layer(&mut s,LayerAction::AddMask{id:1,replace:false});
        let mut occurrence=s.engine.document().scene().occurrence(checked).unwrap().clone();
        occurrence.placement=layer_core::LayerPlacement::from_projective(layer_core::Projective([1.2,0.1,12.,-0.1,0.8,17.,0.0002,-0.0001,1.]));
        occurrence.mask.as_mut().unwrap().linked=false;
        if warped {occurrence.placement.mesh=Some(std::sync::Arc::new(layer_core::MeshMap::identity(layer_core::Rect::from_extent([37,29]),[3,3]).unwrap().move_node(5,Point{x:2.,y:-1.}).unwrap()));}
        s.engine.apply_edit(layer_core::Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences,checked,Some(occurrence.clone())).unwrap())).unwrap();
        let paint=s.engine.document().scene().source_target(checked).unwrap();
        let SourceTarget::Paint(paint)=paint else{panic!("paint")};
        let mut source=s.engine.document().artwork.paint.get(paint).unwrap().clone();source.domain=[37,29];
        s.engine.apply_edit(layer_core::Edit::Paint(RecordChange::replace(&s.engine.document().artwork.paint,paint,Some(source)).unwrap())).unwrap();
        layer(&mut s,LayerAction::New{group:false,clipped:false});
        let active=s.engine.document().working.occurrence.unwrap();
        layer(&mut s,LayerAction::Lock{id:occurrence_token(active),value:true});
        layer(&mut s,LayerAction::ToggleSelection{id:1});layer(&mut s,LayerAction::ToggleSelection{id:occurrence_token(active)});
        assert!(s.move_refusal().is_none());
        let before=s.engine.document().clone();
        let checks=s.selected_layers().clone();
        s.begin_move_transform(Point{x:200.,y:200.},false).unwrap();
        assert_live_artwork_eq(s.engine.document(),&before);
        let bounds=s.transform_document_bounds().unwrap();assert!(bounds[2]<100.&&bounds[3]<100.,"{bounds:?}");
        assert_eq!(s.engine.document().working.occurrence,Some(active));
        s.cancel_transform().unwrap();assert_live_artwork_eq(s.engine.document(),&before);assert_eq!(s.selected_layers(),&checks);
        s.begin_move_transform(Point{x:200.,y:200.},false).unwrap();
        let mut up=event(&s,10,PenPhase::Up,1.);up.surface_position=on_surface(&s,Point{x:225.,y:214.});
        s.layer_pen(up).unwrap();
        let after=s.engine.document().clone();
        assert_eq!(after.scene().occurrence(active),before.scene().occurrence(active));
        assert_eq!(after.scene().occurrence(checked).unwrap().mask,occurrence.mask);
        assert_ne!(after.scene().occurrence(checked).unwrap().placement,occurrence.placement);
        invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
        invoke(&mut s,CommandId::Redo);assert_live_artwork_eq(s.engine.document(),&after);
    }
}

#[test]
fn retained_transform_measures_sole_checked_layer_instead_of_unchecked_active_layer() {
    let mut s=session(Platform::Gtk);
    layer(&mut s,LayerAction::New{group:false,clipped:false});let active=s.engine.document().working.occurrence.unwrap();
    layer(&mut s,LayerAction::ToggleSelection{id:1});layer(&mut s,LayerAction::ToggleSelection{id:occurrence_token(active)});
    s.begin_transform().unwrap();
    assert_eq!(s.engine.backend().bounds_requests.last().unwrap().scope,layer_core::ContentScope::Target(s.engine.document().scene().source_target(occurrence_handle(1).unwrap()).unwrap()));
    reply_bounds(&mut s,[10.,20.,45.,60.]);assert!(s.operation.active());
    assert_eq!(s.transform_document_bounds(),Some([10.,20.,45.,60.]));
    s.cancel_transform().unwrap();
}

#[test]
fn pending_pixel_move_replays_pointer_up_after_bounds_as_one_undo_step() {
    let mut s = filled_selection_session();
    s.layer_interaction.tool = LayerCanvasTool::Move;
    let before = s.engine.document().clone();
    s.begin_move_transform(Point { x: 150., y: 150. }, false).unwrap();
    assert!(s.content_bounds.busy());
    assert!(s.content_bounds.moving.is_some());
    assert!(!s.operation.moving_pixels());
    let mut up = event(&s, 10, PenPhase::Up, 1.);
    up.surface_position = on_surface(&s, Point { x: 177., y: 164. });
    s.layer_pen(up).unwrap();
    assert_eq!(s.engine.document(), &before, "lift while waiting publishes nothing");
    reply_bounds(&mut s, [100., 100., 300., 300.]);
    assert!(s.content_bounds.moving.is_none());
    assert!(!s.operation.moving_pixels());
    assert!(s.layer_interaction.path.is_empty());
    let after = s.engine.document().clone();
    assert_eq!(after.working.selection, before.working.selection.as_ref().map(|selection| selection.translated(Point { x: 27., y: 14. })));
    assert_ne!(after.scene().raster(after.working.target.unwrap()), before.scene().raster(before.working.target.unwrap()));
    s.engine.undo().unwrap();
    assert_live_artwork_eq(s.engine.document(), &before);
    assert_eq!(s.engine.document().working.selection, before.working.selection);
    s.engine.redo().unwrap();
    assert_live_artwork_eq(s.engine.document(), &after);
    assert_eq!(s.engine.document().working.selection, after.working.selection);
}

#[test]
fn pending_pixel_move_cancel_discards_saved_lift_and_result() {
    let mut s = filled_selection_session();
    s.layer_interaction.tool = LayerCanvasTool::Move;
    let before = s.engine.document().clone();
    s.begin_move_transform(Point { x: 150., y: 150. }, false).unwrap();
    let mut up = event(&s, 10, PenPhase::Up, 1.);
    up.surface_position = on_surface(&s, Point { x: 177., y: 164. });
    s.layer_pen(up).unwrap();
    assert!(s.cancel_layer_gesture().unwrap());
    assert!(!s.content_bounds.busy());
    assert!(s.content_bounds.moving.is_none());
    reply_bounds(&mut s, [100., 100., 300., 300.]);
    assert_eq!(s.engine.document(), &before);
    assert!(!s.operation.moving_pixels());
    assert!(s.layer_interaction.path.is_empty());
}

#[test]
fn renderer_replacement_discards_cached_and_pending_bounds_for_the_same_document() {
    let mut s = filled_selection_session();
    s.request_content_bounds(super::image_geometry::ContentUse::PrepareMove).unwrap();
    reply_bounds(&mut s, [100., 100., 300., 300.]);
    assert!(s.measured_target_bounds().is_some());
    let before = s.engine.document().clone();
    let renderer = Recorder::default();
    s.replace_renderer(renderer).unwrap();
    assert!(s.measured_target_bounds().is_none());
    assert_eq!(s.engine.document(), &before);
    s.request_content_bounds(super::image_geometry::ContentUse::Transform).unwrap();
    assert!(s.content_bounds.busy());
    s.replace_renderer(Recorder::default()).unwrap();
    assert!(!s.content_bounds.busy());
    assert!(s.measured_target_bounds().is_none());
    reply_bounds(&mut s, [100., 100., 300., 300.]);
    assert!(!s.operation.active());
    assert_eq!(s.engine.document(), &before);
}

#[test]
fn smaller_transparent_imports_keep_original_photo_frames_and_original_size_handles() {
    let transparent = |extent| std::sync::Arc::unwrap_or_clone(
        layer_core::color::source::rgba8_source(extent, |_, _| [0; 4]));
    for batch in [false, true] {
        let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
            Document::new(PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
        let mut sources = vec![("Small transparent photo".into(), transparent([20, 10]))];
        if batch { sources.push(("Tall transparent photo".into(), transparent([8, 30]))); }
        s.place_layer_sources(sources, Some(Point { x: 75., y: 55. }), None).unwrap();
        let expected = if batch { [65., 40., 85., 70.] } else { [65., 50., 85., 60.] };
        assert_eq!(s.transform_document_bounds(), Some(expected), "the provisional frame uses the original source extent");
        assert!(!s.content_bounds.busy(), "transparent imports retain their full original frame without an alpha query");
        assert!(s.engine.backend().bounds_requests.is_empty());
        assert_eq!(s.engine.document().target_extent(s.engine.document().working.target.unwrap()), [200, 150],
            "the editable extent is larger than the original photo frame");
        let original_sources: Vec<_> = s.engine.document().artwork.paint.iter().filter_map(|(_, _, p)| p.original.clone()).collect();
        s.set_transform_control("transform_width", 2.).unwrap();
        assert_ne!(s.transform_document_bounds(), Some(expected));
        assert!(s.command(CommandId::PlacementOriginalSize).enabled);
        invoke(&mut s, CommandId::PlacementOriginalSize);
        assert_eq!(s.transform_document_bounds(), Some(expected), "Original Size restores the photo or batch handle frame");
        for &handle in s.engine.document().scene().order() {
            let Some(source) = s.engine.document().scene().paint_source(handle).filter(|p| p.original.is_some()) else { continue; };
            assert_eq!(s.engine.document().scene().occurrence(handle).unwrap().placement.as_affine().unwrap().0[..4], [1., 0., 0., 1.]);
            assert!(source.raster.is_empty());
        }
        assert_eq!(s.engine.document().artwork.paint.iter().filter_map(|(_, _, p)| p.original.clone()).collect::<Vec<_>>(), original_sources);
        assert!(!s.engine.can_undo(), "provisional placement publishes no history");
        assert_eq!(s.engine.document().composition().size, [200, 150]);
    }
}

#[test]
fn cached_empty_photo_bounds_keep_repeated_transform_refused_without_history() {
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
    let original = s.engine.document().clone();
    let source = layer_core::color::source::rgba8_source([20, 10], |_, _| [0; 4]);
    s.place_layer_source("Transparent photo", std::sync::Arc::unwrap_or_clone(source), None).unwrap();
    invoke(&mut s, CommandId::ApplyTransform);
    assert!(!s.operation.active());
    let accepted = s.engine.document().clone();
    s.begin_transform().unwrap();
    assert!(s.content_bounds.busy());
    reply_bounds(&mut s, [0.; 4]);
    assert_eq!(s.measured_target_bounds(), Some(layer_core::Rect::EMPTY));
    assert!(!s.operation.active());
    assert_eq!(s.engine.document(), &accepted);
    let requests = s.engine.backend().bounds_requests.len();
    assert!(s.begin_transform().is_err(), "a cached empty result cannot admit an empty placement");
    assert!(!s.content_bounds.busy());
    assert!(!s.operation.active());
    assert_eq!(s.engine.backend().bounds_requests.len(), requests, "the cached refusal needs no new query");
    assert_eq!(s.engine.document(), &accepted);
    invoke(&mut s, CommandId::Undo);
    assert_live_artwork_eq(s.engine.document(), &original);
    assert!(!s.engine.can_undo(), "one undo removes the import; failed transforms add no history");
    invoke(&mut s, CommandId::Redo);
    assert_live_artwork_eq(s.engine.document(), &accepted);
}

fn linked_bounds_session(primary_mask: bool) -> UiSession<Recorder> {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};
    let mut document = Document::new(PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut bytes = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
    for y in 20..40 { for x in 20..40 {
        bytes[((y * TILE_SIZE + x) * 4) as usize..][..4].copy_from_slice(&[255; 4]);
    }}
    let SourceTarget::Paint(paint) = document.working.target.unwrap() else { panic!("paint") };
    document.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] },
            RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(document.composition().color), &bytes).unwrap()))].into(),
        ..Default::default()
    });
    let mask = document.artwork.coverage.insert(PortableId::random(), CoverageSource {
        domain: document.composition().size, raster: Default::default(), initial: Some(rectangle([80., 80., 100., 100.])),
        default_coverage: 0., operations: Default::default(),
    }).unwrap();
    let owner = document.working.occurrence.unwrap();
    let occurrence = document.artwork.occurrences.get_mut(owner).unwrap();
    occurrence.translation = Point { x: 10., y: 12. };
    occurrence.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([1.5, 0., 0., 0.75, 0., 0.]));
    occurrence.mask = Some(MaskUse { source: mask, enabled: true, linked: true, inverted: false,
        translation: Point { x: 24., y: 30. }, placement: layer_core::Projective::from_affine(layer_core::Affine([0.75, 0., 0., 1.5, 0., 0.])) });
    document.working.target = Some(if primary_mask { SourceTarget::Coverage(mask) } else { SourceTarget::Paint(paint) });
    document.working.inspect_mask = primary_mask.then_some(owner);
    if !primary_mask { document.working.selection = Some(layer_core::Selection::polygon(vec![Point { x: 0., y: 0. }, Point { x: 200., y: 0. }, Point { x: 200., y: 200. }, Point { x: 0., y: 200. }]).unwrap()); }
    let working = document.working.clone();
    let mut document = Document::from_artwork(document.artwork).unwrap();
    document.working = working;
    UiSession::new(Recorder::default(), document, [800, 600], Platform::Gtk).unwrap()
}

#[test]
fn linked_transform_waits_for_both_actual_bounds_and_warp_covers_the_registered_pair() {
    for primary_mask in [false, true] {
        let mut s = linked_bounds_session(primary_mask);
        let doc = s.engine.document();
        let primary = doc.active_target().unwrap();
        let owner = doc.working.occurrence.unwrap();
        let companion = if primary_mask { doc.scene().source_target(owner).unwrap() } else { SourceTarget::Coverage(doc.scene().occurrence(owner).unwrap().mask.as_ref().unwrap().source) };
        let paint_bounds = layer_core::Rect { min: Point { x: 20., y: 20. }, max: Point { x: 40., y: 40. } };
        let mask_bounds = layer_core::Rect { min: Point { x: 80., y: 80. }, max: Point { x: 100., y: 100. } };
        let tight = if primary_mask { mask_bounds } else { paint_bounds };
        let other = if primary_mask { paint_bounds } else { mask_bounds };
        let to = doc.affine_edit_transform(companion).unwrap().then(doc.affine_edit_transform(primary).unwrap().inverse().unwrap());
        let expected = tight.union(to.bounds(other));
        s.begin_transform().unwrap();
        assert_eq!(s.engine.backend().bounds_requests.last().unwrap().scope, layer_core::ContentScope::Target(primary));
        reply_bounds(&mut s, if primary_mask { [80., 80., 100., 100.] } else { [20., 20., 40., 40.] });
        assert!(s.content_bounds.busy(), "the linked companion still needs actual bounds");
        assert!(!s.operation.active());
        assert!(s.measured_target_bounds().is_none());
        assert_eq!(s.engine.backend().bounds_requests.last().unwrap().scope, layer_core::ContentScope::Target(companion));
        assert_eq!(s.engine.backend().bounds_requests.len(), 2);
        reply_bounds(&mut s, if primary_mask { [20., 20., 40., 40.] } else { [80., 80., 100., 100.] });
        assert!(!s.content_bounds.busy());
        assert!(s.operation.active());
        assert_eq!(s.measured_target_bounds(), Some(expected));
        s.set_transform_mode(super::operation::TransformMode::Warp, false).unwrap();
        s.frame(102, 102).unwrap();
        let preview = s.engine.backend().transform.as_ref().unwrap();
        let mesh = preview.transform.placement.mesh.as_ref().expect("Warp must emit a mesh");
        assert_eq!(mesh.frame.bounds(layer_core::Rect::from_extent([1, 1])), expected,
            "the mesh domain contains both independently measured targets in primary-local coordinates");
        let paired = preview.companion(s.engine.document().scene()).unwrap();
        for point in other.corners() {
            assert!(paired.transform.map(point).is_some(), "the registered companion remains inside the warp domain");
        }
    }
}

#[test]
fn retained_whole_photo_bounds_do_not_query_a_linked_destructive_companion() {
    let mut document = Document::new(PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = document.working.target.unwrap();
    let owner = document.working.occurrence.unwrap();
    let mask = document.artwork.coverage.insert(PortableId::random(), CoverageSource {
        domain: document.composition().size, raster: Default::default(), initial: None, default_coverage: 1., operations: Default::default(),
    }).unwrap();
    let mask_id = SourceTarget::Coverage(mask);
    document.artwork.occurrences.get_mut(owner).unwrap().mask = Some(MaskUse { source: mask, enabled: true, linked: true,
        inverted: false, translation: Point::default(), placement: layer_core::Projective::IDENTITY });
    let SourceTarget::Paint(paint) = photo else { panic!("paint") };
    document.artwork.paint.get_mut(paint).unwrap().original = Some(layer_core::color::source::rgba8_source([20, 10], |_, _| [0; 4]));
    let working = document.working.clone();
    let mut document = Document::from_artwork(document.artwork).unwrap();
    document.working = working;
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, document, [800, 600], Platform::Gtk).unwrap();
    s.begin_transform().unwrap();
    reply_bounds(&mut s, [2., 2., 18., 8.]);
    assert!(s.operation.placing());
    assert!(!s.content_bounds.busy());
    assert_eq!(s.engine.backend().bounds_requests.len(), 1);
    assert_eq!(s.engine.backend().bounds_requests[0].scope, layer_core::ContentScope::Target(photo));
    assert!(s.engine.backend().bounds_requests.iter().all(|r| r.scope != layer_core::ContentScope::Target(mask_id)));
}

#[test]
fn cancelling_linked_transform_preserves_measured_pair_without_another_request() {
    for primary_mask in [false, true] {
        let mut s = linked_bounds_session(primary_mask);
        let before = s.engine.document().clone();
        s.begin_transform().unwrap();
        reply_bounds(&mut s, if primary_mask { [80., 80., 100., 100.] } else { [20., 20., 40., 40.] });
        reply_bounds(&mut s, if primary_mask { [20., 20., 40., 40.] } else { [80., 80., 100., 100.] });
        let measured = s.measured_target_bounds().expect("the registered pair is cached");
        assert_eq!(s.engine.backend().bounds_requests.len(), 2);
        assert!(s.operation.active());
        invoke(&mut s, CommandId::CancelTransform);
        assert!(!s.operation.active());
        assert_eq!(s.engine.document(), &before);
        assert_eq!(s.measured_target_bounds(), Some(measured));
        s.begin_transform().unwrap();
        assert!(s.operation.active(), "the cached pair starts Transform immediately");
        assert!(!s.content_bounds.busy());
        assert_eq!(s.engine.backend().bounds_requests.len(), 2, "neither target needs another query after Cancel");
        assert_eq!(s.measured_target_bounds(), Some(measured));
        invoke(&mut s, CommandId::CancelTransform);
        assert_eq!(s.engine.document(), &before);
        assert!(!s.engine.can_undo());
    }
}

#[test]
fn changing_active_target_during_companion_query_rejects_the_frozen_pair() {
    let mut s = linked_bounds_session(false);
    s.begin_transform().unwrap();
    reply_bounds(&mut s, [20., 20., 40., 40.]);
    assert!(s.content_bounds.busy());
    assert_eq!(s.engine.backend().bounds_requests.len(), 2);
    let before = s.engine.document().clone();
    let mut working = s.engine.document().working.clone();
    let owner = working.occurrence.unwrap();
    working.target = Some(SourceTarget::Coverage(s.engine.document().scene().occurrence(owner).unwrap().mask.as_ref().unwrap().source));
    working.inspect_mask = Some(owner);
    s.engine.apply_edit(layer_core::Edit::Working(working)).unwrap();
    let changed = s.engine.document().clone();
    reply_bounds(&mut s, [80., 80., 100., 100.]);
    assert!(!s.content_bounds.busy());
    assert!(!s.operation.active());
    assert_eq!(s.engine.document(), &changed);
    assert_live_artwork_eq(s.engine.document(), &before);
    assert!(s.engine.backend().bounds_cancels > 0);
}

#[test]
fn four_child_group_bounds_complete_once_include_hidden_paint_and_cancel_stale_inputs() {
    let seeded = linked_bounds_session(false);
    let mut document = seeded.engine.document().clone();
    document.working.selection = None;
    let old = document.working.occurrence.unwrap();
    let mut template = document.scene().occurrence(old).unwrap().clone();
    template.mask = None;
    let source = document.scene().paint_source(old).unwrap().clone();
    let old_id = document.artwork.occurrences.id(old).unwrap();
    document.artwork.occurrences.change(old, old_id, None).unwrap();
    let nested = document.artwork.stacks.insert(PortableId::random(), Stack::default()).unwrap();
    let mut group = Occurrence::new(OccurrenceContent::Stack(nested), "Group");
    group.translation = Point { x: 50., y: 10. };
    let group_id = document.artwork.occurrences.insert(PortableId::random(), group).unwrap();
    let mut children = Vec::new();
    let mut targets = Vec::new();
    for (index, offset) in [[0., 0.], [40., 10.], [-10., 80.], [70., -20.]].into_iter().enumerate() {
        let paint = document.artwork.paint.insert(PortableId::random(), source.clone()).unwrap();
        let mut child = template.clone();
        child.content = OccurrenceContent::Paint(paint);
        child.translation = Point { x: offset[0], y: offset[1] };
        child.visible = index != 2;
        let handle = document.artwork.occurrences.insert(PortableId::random(), child).unwrap();
        children.push(handle);
        targets.push(SourceTarget::Paint(paint));
    }
    document.artwork.stacks.get_mut(nested).unwrap().entries = children.clone();
    let outside_paint = document.artwork.paint.insert(PortableId::random(), source).unwrap();
    let mut outside = template;
    outside.content = OccurrenceContent::Paint(outside_paint);
    let outside_id = document.artwork.occurrences.insert(PortableId::random(), outside).unwrap();
    let root = document.composition().result;
    let mut entries = document.artwork.stacks.get(root).unwrap().entries.clone();
    entries.retain(|h| *h != old);
    entries.extend([outside_id, group_id]);
    document.artwork.stacks.get_mut(root).unwrap().entries = entries;
    let mut document = Document::from_artwork(document.artwork).unwrap();
    document.apply(document.select_occurrence_edit(group_id).unwrap()).unwrap();
    let make = || {
        let mut session = UiSession::new(Recorder::default(), document.clone(), [800, 600], Platform::Gtk).unwrap();
        session.set_selected_layers(std::collections::BTreeSet::from([group_id])).unwrap();
        session
    };
    let mut s = make();
    let before = s.engine.document().clone();
    s.begin_transform().unwrap();
    for count in 1..=4 {
        assert!(s.content_bounds.busy());
        assert_eq!(s.engine.backend().bounds_requests.len(), count);
        reply_bounds(&mut s, [20., 20., 40., 40.]);
        assert_eq!(s.operation.active(), count == 4);
    }
    assert!(!s.content_bounds.busy());
    assert_eq!(s.engine.backend().bounds_requests.iter().map(|request| request.scope).collect::<Vec<_>>(),
        targets.iter().copied().map(layer_core::ContentScope::Target).collect::<Vec<_>>());
    assert_eq!(s.measured_target_bounds(), Some(layer_core::Rect { min: Point { x: 70., y: 5. }, max: Point { x: 180., y: 120. } }));
    invoke(&mut s, CommandId::CancelTransform);
    assert_eq!(s.engine.document(), &before);
    s.begin_transform().unwrap();
    assert!(s.operation.active());
    assert_eq!(s.engine.backend().bounds_requests.len(), 4, "all member measurements remain cached together");
    invoke(&mut s, CommandId::CancelTransform);

    let mut stale = make();
    stale.begin_transform().unwrap();
    reply_bounds(&mut stale, [20., 20., 40., 40.]);
    let mut changed = stale.engine.document().scene().occurrence(children[3]).unwrap().clone();
    changed.opacity = 0.5;
    stale.engine.apply_edit(layer_core::Edit::Occurrence(RecordChange::replace(&stale.engine.document().artwork.occurrences, children[3], Some(changed)).unwrap())).unwrap();
    reply_bounds(&mut stale, [20., 20., 40., 40.]);
    assert!(!stale.operation.active());
    assert!(!stale.content_bounds.busy());
    assert_eq!(stale.engine.backend().bounds_requests.len(), 2);

    let mut selection_stale = make();
    selection_stale.begin_transform().unwrap();
    reply_bounds(&mut selection_stale, [20., 20., 40., 40.]);
    let old_document = selection_stale.engine.document().clone();
    selection_stale.dispatch(UiAction::Layer { action: LayerAction::ToggleSelection { id: occurrence_token(outside_id) } }).unwrap();
    assert_live_artwork_eq(selection_stale.engine.document(), &old_document);
    assert_eq!(selection_stale.selected_layers(), &std::collections::BTreeSet::from([group_id, outside_id]));
    assert_eq!(selection_stale.engine.document().working.occurrence, old_document.working.occurrence);
    reply_bounds(&mut selection_stale, [20., 20., 40., 40.]);
    assert!(!selection_stale.operation.active());
    assert!(!selection_stale.content_bounds.busy());
    assert!(!selection_stale.engine.can_undo());
    assert_eq!(selection_stale.engine.backend().bounds_requests.len(), 2);

}
