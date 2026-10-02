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
    assert_eq!(after.selection, before.selection.as_ref().map(|selection| selection.translated(Point { x: 27., y: 14. })));
    assert_ne!(after.layers[0].raster, before.layers[0].raster);
    s.engine.undo().unwrap();
    assert_eq!(s.engine.document().layers, before.layers);
    assert_eq!(s.engine.document().selection, before.selection);
    s.engine.redo().unwrap();
    assert_eq!(s.engine.document().layers, after.layers);
    assert_eq!(s.engine.document().selection, after.selection);
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
            Document::new("small transparent imports", 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
        let mut sources = vec![("Small transparent photo".into(), transparent([20, 10]))];
        if batch { sources.push(("Tall transparent photo".into(), transparent([8, 30]))); }
        s.place_layer_sources(sources, Some(Point { x: 75., y: 55. }), None).unwrap();
        let expected = if batch { [65., 40., 85., 70.] } else { [65., 50., 85., 60.] };
        assert_eq!(s.transform_document_bounds(), Some(expected), "the provisional frame uses the original source extent");
        assert!(!s.content_bounds.busy(), "transparent imports retain their full original frame without an alpha query");
        assert!(s.engine.backend().bounds_requests.is_empty());
        assert_eq!(s.engine.document().target_extent(s.engine.document().active_layer), [200, 150],
            "the editable extent is larger than the original photo frame");
        let original_sources: Vec<_> = s.engine.document().layers.iter().filter_map(|l| l.source.clone()).collect();
        s.set_transform_control("transform_width", 2.).unwrap();
        assert_ne!(s.transform_document_bounds(), Some(expected));
        assert!(s.command(CommandId::PlacementOriginalSize).enabled);
        invoke(&mut s, CommandId::PlacementOriginalSize);
        assert_eq!(s.transform_document_bounds(), Some(expected), "Original Size restores the photo or batch handle frame");
        for layer in s.engine.document().layers.iter().filter(|l| l.source.is_some()) {
            assert_eq!(layer.properties.placement.as_affine().unwrap().0[..4], [1., 0., 0., 1.]);
            assert!(layer.raster.is_empty());
        }
        assert_eq!(s.engine.document().layers.iter().filter_map(|l| l.source.clone()).collect::<Vec<_>>(), original_sources);
        assert!(!s.engine.can_undo(), "provisional placement publishes no history");
        assert_eq!([s.engine.document().width, s.engine.document().height], [200, 150]);
    }
}

#[test]
fn cached_empty_photo_bounds_keep_repeated_transform_refused_without_history() {
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new("accepted transparent photo", 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
    let original_layers = s.engine.document().layers.clone();
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
    assert_eq!(s.engine.document().layers, original_layers, "one undo removes the import; failed transforms add no history");
    invoke(&mut s, CommandId::Redo);
    assert_eq!(s.engine.document().layers, accepted.layers);
}

fn linked_bounds_session(primary_mask: bool) -> UiSession<Recorder> {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};
    let mut document = Document::new("paired bounds", 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut bytes = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
    for y in 20..40 { for x in 20..40 {
        bytes[((y * TILE_SIZE + x) * 4) as usize..][..4].copy_from_slice(&[255; 4]);
    }}
    document.layers[0].raster = RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] },
            RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(document.color), &bytes).unwrap()))].into(),
        ..Default::default()
    });
    let mut mask = layer_core::LayerMask::reveal_all(document.allocate_layer_id(), Point { x: 24., y: 30. });
    mask.default_coverage = 0.;
    mask.initial = Some(rectangle([80., 80., 100., 100.]));
    mask.placement = layer_core::Projective::from_affine(layer_core::Affine([0.75, 0., 0., 1.5, 0., 0.]));
    document.layers[0].properties.offset = Point { x: 10., y: 12. };
    document.layers[0].properties.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([1.5, 0., 0., 0.75, 0., 0.]));
    document.layers[0].mask = Some(mask);
    document.active_mask = primary_mask;
    if !primary_mask { document.selection = Some(layer_core::Selection::polygon(vec![Point { x: 0., y: 0. }, Point { x: 200., y: 0. }, Point { x: 200., y: 200. }, Point { x: 0., y: 200. }]).unwrap()); }
    UiSession::new(Recorder::default(), document, [800, 600], Platform::Gtk).unwrap()
}

#[test]
fn linked_transform_waits_for_both_actual_bounds_and_warp_covers_the_registered_pair() {
    for primary_mask in [false, true] {
        let mut s = linked_bounds_session(primary_mask);
        let doc = s.engine.document();
        let primary = doc.active_target();
        let companion = if primary_mask { doc.active_layer } else { doc.layers[0].mask.as_ref().unwrap().id };
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
        let paired = preview.companion(&s.engine.document().layers).unwrap();
        for point in other.corners() {
            assert!(paired.transform.map(point).is_some(), "the registered companion remains inside the warp domain");
        }
    }
}

#[test]
fn retained_whole_photo_bounds_do_not_query_a_linked_destructive_companion() {
    let mut document = Document::new("retained photo bounds", 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = document.layers[0].id;
    let mask = layer_core::LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    let mask_id = mask.id;
    document.layers[0].mask = Some(mask);
    document.layers[0].source = Some(layer_core::color::source::rgba8_source([20, 10], |_, _| [0; 4]));
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
    s.engine.apply_edit(layer_core::Edit::SetMaskTarget(true)).unwrap();
    let changed = s.engine.document().clone();
    reply_bounds(&mut s, [80., 80., 100., 100.]);
    assert!(!s.content_bounds.busy());
    assert!(!s.operation.active());
    assert_eq!(s.engine.document(), &changed);
    assert_eq!(s.engine.document().layers, before.layers);
    assert!(s.engine.backend().bounds_cancels > 0);
}

#[test]
fn four_child_group_bounds_complete_once_include_hidden_paint_and_cancel_stale_inputs() {
    let seeded = linked_bounds_session(false);
    let mut document = seeded.engine.document().clone();
    document.selection = None;
    let template = document.layers[0].clone();
    document.layers.retain(|layer| layer.id != template.id);
    let group_id = document.allocate_layer_id();
    let mut group = layer_core::Layer::paint(group_id, "Group");
    group.kind = layer_core::LayerKind::Group;
    group.properties.offset = Point { x: 50., y: 10. };
    let mut children = Vec::new();
    for (index, offset) in [[0., 0.], [40., 10.], [-10., 80.], [70., -20.]].into_iter().enumerate() {
        let mut child = template.clone();
        child.id = document.allocate_layer_id();
        child.mask = None;
        child.properties.parent = Some(group_id);
        child.properties.offset = Point { x: offset[0], y: offset[1] };
        child.visible = index != 2;
        children.push(child.id);
        document.layers.insert(index, child);
    }
    let outside_id = document.allocate_layer_id();
    let mut outside = template.clone();
    outside.id = outside_id;
    outside.mask = None;
    outside.properties.parent = None;
    document.layers.push(outside);
    document.layers.push(group);
    document.active_layer = group_id;
    document.active_mask = false;
    let make = || {
        let mut session = UiSession::new(Recorder::default(), document.clone(), [800, 600], Platform::Gtk).unwrap();
        session.layer_interaction.selected = std::collections::BTreeSet::from([group_id]);
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
        children.iter().copied().map(layer_core::ContentScope::Target).collect::<Vec<_>>());
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
    let mut changed = stale.engine.document().layer(children[3]).unwrap().clone();
    changed.opacity = 0.5;
    stale.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(changed))).unwrap();
    reply_bounds(&mut stale, [20., 20., 40., 40.]);
    assert!(!stale.operation.active());
    assert!(!stale.content_bounds.busy());
    assert_eq!(stale.engine.backend().bounds_requests.len(), 2);

    let mut selection_stale = make();
    selection_stale.begin_transform().unwrap();
    reply_bounds(&mut selection_stale, [20., 20., 40., 40.]);
    let old_document = selection_stale.engine.document().clone();
    selection_stale.dispatch(UiAction::Layer { action: LayerAction::ToggleSelection { id: outside_id.0 } }).unwrap();
    assert_eq!(selection_stale.engine.document(), &old_document, "root selection does not revise artwork");
    reply_bounds(&mut selection_stale, [20., 20., 40., 40.]);
    assert!(!selection_stale.operation.active());
    assert!(!selection_stale.content_bounds.busy());
    assert!(!selection_stale.engine.can_undo());
    assert_eq!(selection_stale.engine.backend().bounds_requests.len(), 2);

}
