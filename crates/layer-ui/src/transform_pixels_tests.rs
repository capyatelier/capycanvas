pub(super) fn raw_revision(color: layer_core::color::DocumentColor, planes: &[layer_core::raster::RasterPlane], seed: u8) -> layer_core::raster::RasterRevision {
    use layer_core::raster::{RasterData, RasterTile, TileBlob, TileKey, TILE_SIZE};
    layer_core::raster::RasterRevision::backed(RasterData {
        tiles: planes.iter().enumerate().map(|(i, plane)| {
            let descriptor = plane.descriptor(color);
            let bytes = vec![seed + i as u8; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
            (TileKey { plane: *plane, coordinate: [0, 0] }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))
        }).collect(), watercolor: None,
    })
}

fn bake_session(linked: bool) -> UiSession<Recorder> {
    use layer_core::color::source::*;
    use layer_core::raster::RasterPlane;
    let mut doc = Document::new("bake", 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut source = SourceBuilder::new([16, 8], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: layer_core::color::SampleDepth::U8,
        profile: Default::default(), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    for _ in 0..8 { source.push_row(&[120; 16 * 4]).unwrap(); }
    doc.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    doc.layers[0].properties.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([2., 0., 0., 3., -17., 13.]));
    doc.layers[0].raster = raw_revision(doc.color, &[RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness], 20);
    let mut mask = layer_core::LayerMask::reveal_all(doc.allocate_layer_id(), Point { x: 5., y: 7. });
    mask.linked = linked;
    mask.placement = layer_core::Projective::from_affine(layer_core::Affine([1., 0., 0., 2., 11., 13.]));
    mask.raster = raw_revision(doc.color, &[RasterPlane::Mask], 90);
    mask.initial = Some(layer_core::Selection::polygon(vec![Point { x: 2., y: 3. }, Point { x: 7., y: 3. }, Point { x: 7., y: 9. }]).unwrap());
    doc.layers[0].mask = Some(mask);
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s
}

fn frozen_plan(s: &UiSession<Recorder>) -> layer_core::TransformPixelsPlan {
    match s.engine.backend().snapshot_requests.last().unwrap() {
        layer_render::SnapshotRequest::TransformPixels(plan) => plan.clone(),
        _ => panic!("expected pixel bake request"),
    }
}

fn completed_bake(plan: &layer_core::TransformPixelsPlan) -> layer_core::Layer {
    use layer_core::raster::RasterPlane;
    let mut layer = plan.output.clone();
    layer.raster = raw_revision(plan.input.document.color, &[RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness], 150);
    if matches!(plan.scope, layer_core::TransformPixelsScope::Paint { linked_mask: true }) {
        layer.mask.as_mut().unwrap().raster = raw_revision(plan.input.document.color, &[RasterPlane::Mask], 210);
    }
    layer
}

fn bake_reply(s: &mut UiSession<Recorder>, reply: Result<layer_render::SnapshotResult, layer_render::BackendError>) {
    s.engine.backend_mut().snapshot_reply = Some(reply);
    s.frame(20, 20).unwrap();
    s.frame(21, 21).unwrap();
}

#[test]
fn apply_transform_pixels_is_pending_then_one_atomic_undo_restores_all_native_roots() {
    for linked in [false, true] {
        let mut s = bake_session(linked);
        let before = s.engine.document().clone();
        assert!(s.command(CommandId::ApplyTransformPixels).enabled);
        s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels }).unwrap();
        assert!(s.content_bounds.busy());
        assert_eq!(s.engine.document(), &before);
        assert!(!s.engine.can_undo());
        assert!(s.engine.backend().bounds_requests.is_empty());
        let plan = frozen_plan(&s);
        let output = completed_bake(&plan);
        bake_reply(&mut s, Ok(layer_render::SnapshotResult::TransformPixels(Box::new(output.clone()))));
        assert!(!s.content_bounds.busy());
        assert_eq!(s.engine.document().layer(output.id).unwrap(), &output);
        assert!(!s.command(CommandId::ApplyTransformPixels).enabled);
        let after = s.engine.document().clone();
        assert!(s.engine.undo().unwrap());
        assert_eq!(s.engine.document().layers, before.layers);
        assert!(std::sync::Arc::ptr_eq(s.engine.document().layers[0].source.as_ref().unwrap(), before.layers[0].source.as_ref().unwrap()));
        assert!(!s.engine.can_undo(), "one undo restores color, scalar planes, source and mask together");
        assert!(s.engine.redo().unwrap());
        assert_eq!(s.engine.document().layers, after.layers);
    }
}

#[test]
fn apply_transform_pixels_retries_unaccepted_work_and_cancel_discards_late_result() {
    let mut s = bake_session(true);
    let before = s.engine.document().clone();
    s.engine.backend_mut().snapshot_wait = true;
    s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels }).unwrap();
    s.frame(2, 2).unwrap();
    assert!(s.content_bounds.busy());
    assert!(s.engine.backend().snapshot_requests.is_empty());
    s.engine.backend_mut().snapshot_wait = false;
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    let output = completed_bake(&frozen_plan(&s));
    s.dispatch(UiAction::Invoke { command: CommandId::CancelTransform }).unwrap();
    assert!(!s.content_bounds.busy());
    assert!(s.engine.backend().snapshot_cancels > 0);
    bake_reply(&mut s, Ok(layer_render::SnapshotResult::TransformPixels(Box::new(output))));
    assert_eq!(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
}

#[test]
fn apply_transform_pixels_rejects_stale_target_and_renderer_results() {
    for invalidation in 0..4 {
        let mut s = bake_session(true);
        s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels }).unwrap();
        let output = completed_bake(&frozen_plan(&s));
        match invalidation {
            0 => { s.replace_renderer(Recorder { tiled_sources: true, ..Default::default() }).unwrap(); },
            1 => { s.engine.apply_edit(layer_core::Edit::SetMaskTarget(true)).unwrap(); },
            2 => {
                let paper = s.engine.document().layers.iter().find(|layer| layer.kind == layer_core::LayerKind::Background).unwrap().id;
                s.engine.apply_edit(layer_core::Edit::SetActiveLayer { id: paper }).unwrap();
            },
            _ => { s.state.document_file.epoch += 1; },
        }
        let changed = s.engine.document().clone();
        bake_reply(&mut s, Ok(layer_render::SnapshotResult::TransformPixels(Box::new(output))));
        assert!(!s.content_bounds.busy());
        assert_eq!(s.engine.document(), &changed);
        assert!(!s.engine.can_undo());
    }
}

#[test]
fn apply_transform_pixels_failures_preserve_existing_redo_and_original_backing() {
    for failure in 0..5 {
        let mut s = bake_session(true);
        let mut edited = s.engine.document().layers[0].clone();
        edited.name = "redoable".into();
        s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(edited))).unwrap();
        s.engine.undo().unwrap();
        assert!(s.engine.can_redo());
        let before = s.engine.document().clone();
        if failure == 0 { s.engine.backend_mut().snapshot_fails = true; }
        if failure == 1 { s.engine.backend_mut().snapshot_wait = true; }
        let started = s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels });
        if failure == 0 { assert!(started.is_err()); } else { started.unwrap(); }
        if failure == 1 {
            s.engine.backend_mut().snapshot_fails = true;
            s.frame(2, 2).unwrap();
        } else if failure >= 2 {
            let mut output = completed_bake(&frozen_plan(&s));
            let result = match failure {
                2 => Err(layer_render::BackendError("bake failed")),
                3 => Ok(layer_render::SnapshotResult::Bounds(layer_core::Rect::EMPTY)),
                _ => { output.id = layer_core::LayerId(999); Ok(layer_render::SnapshotResult::TransformPixels(Box::new(output))) },
            };
            bake_reply(&mut s, result);
        }
        assert!(!s.content_bounds.busy());
        assert_eq!(s.engine.document(), &before);
        assert!(s.engine.can_redo(), "failed bake must retain redo");
        assert!(s.engine.redo().unwrap());
        assert_eq!(s.engine.document().layers[0].name.as_ref(), "redoable");
    }
}

#[test]
fn renderer_replacement_cancels_the_accepted_bake_on_the_retired_renderer() {
    let mut s = bake_session(true);
    let before = s.engine.document().clone();
    s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels }).unwrap();
    assert_eq!(s.engine.backend().snapshot_requests.len(), 1);
    let (retired, _) = s.replace_renderer(Recorder { tiled_sources: true, ..Default::default() }).unwrap();
    assert!(retired.snapshot_cancels > 0, "cancel the worker on its owning renderer before retiring it");
    assert!(!s.content_bounds.busy());
    assert_eq!(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
}

#[test]
fn source_less_move_rejected_motion_and_release_commit_last_valid_preview_once() {
    let seeded = bake_session(true);
    let mut doc = seeded.engine.document().clone();
    doc.layers[0].source = None;
    doc.layers[0].properties.placement = layer_core::LayerPlacement::IDENTITY;
    let mut s = UiSession::new(Recorder { max_dimension: Some(512), ..Default::default() }, doc,
        [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s.layer_interaction.tool = LayerCanvasTool::Move;
    s.state.layer_tools.tool = LayerCanvasTool::Move;
    let before = s.engine.document().clone();
    let send = |s: &mut UiSession<Recorder>, sequence, phase, point| {
        let mut input = event(s, sequence, phase, 1.);
        input.surface_position = on_surface(s, point);
        s.layer_pen(input)
    };
    send(&mut s, 1, PenPhase::Down, Point { x: 20., y: 20. }).unwrap();
    if s.content_bounds.busy() { reply_bounds(&mut s, [0., 0., 32., 32.]); }
    send(&mut s, 2, PenPhase::Move, Point { x: 30., y: 30. }).unwrap();
    let valid = s.engine.document().layers[0].clone();
    let moved_origin = s.engine.document().layer_geometry(before.active_target()).map(Point { x: 0., y: 0. }).unwrap();
    let original_origin = before.layer_geometry(before.active_target()).map(Point { x: 0., y: 0. }).unwrap();
    assert_eq!(moved_origin, Point { x: original_origin.x + 10., y: original_origin.y + 10. });
    assert!(!s.engine.can_undo());
    send(&mut s, 3, PenPhase::Move, Point { x: 100000., y: 100000. }).unwrap();
    assert_eq!(s.engine.document().layers[0], valid);
    send(&mut s, 4, PenPhase::Up, Point { x: 100000., y: 100000. }).unwrap();
    assert!(s.layer_interaction.path.is_empty());
    assert!(!s.operation.active());
    let owner = before.layers[0].id;
    let mask = before.layers[0].mask.as_ref().unwrap().id;
    for target in [owner, mask] {
        let old = before.target_raster(target).unwrap().wait_data().unwrap();
        let new = s.engine.document().target_raster(target).unwrap().wait_data().unwrap();
        assert_eq!(old.tiles.len(), new.tiles.len());
        for (key, tile) in &old.tiles {
            let moved = new.tiles.iter().find(|(next, value)| next.plane == key.plane && value.same_capture(tile)).unwrap().0;
            let original_world = before.affine_edit_transform(target).unwrap().map(Point { x: key.coordinate[0] as f32 * 256., y: key.coordinate[1] as f32 * 256. });
            let moved_world = s.engine.document().affine_edit_transform(target).unwrap().map(Point { x: moved.coordinate[0] as f32 * 256., y: moved.coordinate[1] as f32 * 256. });
            assert!((moved_world.x - original_world.x - 10.).abs() < 0.001);
            assert!((moved_world.y - original_world.y - 10.).abs() < 0.001);
        }
    }
    assert!(s.engine.document().layers[0].source.is_none());
    assert!(s.engine.document().layers[0].properties.extent.unwrap().iter().all(|axis| *axis <= 512));
    let after = s.engine.document().clone();
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().layers, before.layers);
    assert!(!s.engine.can_undo());
    assert!(s.engine.redo().unwrap());
    assert_eq!(s.engine.document().layers, after.layers);
}

#[test]
fn source_less_move_click_keeps_unstored_canvas_extent_and_existing_redo() {
    let mut doc = Document::new("move click", 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.layers[0].raster = raw_revision(doc.color, &[layer_core::raster::RasterPlane::Color], 30);
    let mut s = UiSession::new(Recorder::default(), doc, [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    let mut renamed = s.engine.document().layers[0].clone();
    renamed.name = "redoable".into();
    s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(renamed))).unwrap();
    s.engine.undo().unwrap();
    assert!(s.engine.can_redo());
    let before = s.engine.document().clone();
    assert!(before.layers[0].properties.extent.is_none());
    s.layer_interaction.tool = LayerCanvasTool::Move;
    s.state.layer_tools.tool = LayerCanvasTool::Move;
    for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Up)] {
        let mut input = event(&s, sequence, phase, 1.);
        input.surface_position = on_surface(&s, Point { x: 20., y: 20. });
        s.layer_pen(input).unwrap();
    }
    assert!(s.layer_interaction.path.is_empty());
    assert_eq!(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
    assert!(s.engine.can_redo());
    assert!(s.engine.redo().unwrap());
    assert_eq!(s.engine.document().layers[0].name.as_ref(), "redoable");
}
