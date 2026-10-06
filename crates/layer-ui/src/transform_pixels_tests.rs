use layer_core::authored::{CoverageHandle, OccurrenceHandle, PaintHandle, PortableId, RecordChange, SourceTarget};

pub(super) fn raw_revision(color: layer_core::color::DocumentColor, planes: &[layer_core::raster::RasterPlane], seed: u8) -> layer_core::raster::RasterRevision {
    use layer_core::raster::{RasterData, RasterTile, TileBlob, TileKey, TILE_SIZE};
    layer_core::raster::RasterRevision::backed(RasterData {
        tiles: planes.iter().enumerate().map(|(i, plane)| {
            let descriptor = plane.descriptor(color);
            let bytes = vec![seed + i as u8; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
            (TileKey { plane: *plane, coordinate: [0, 0] }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))
        }).collect(),
        watercolor: planes.contains(&layer_core::raster::RasterPlane::WatercolorWetness)
            .then_some(layer_core::raster::RasterWatercolor { wet_edge: 0.5, burnt_edge: 0.5, edge_width: 2. }),
    })
}

fn bake_owner(doc: &Document) -> OccurrenceHandle {
    doc.scene().order().iter().copied().find(|h| doc.scene().paint_source(*h).is_some()).unwrap()
}
fn bake_paint_handle(doc: &Document) -> PaintHandle {
    match doc.scene().source_target(bake_owner(doc)).unwrap() { SourceTarget::Paint(h) => h, _ => unreachable!() }
}
fn bake_paint(doc: &Document) -> &layer_core::authored::PaintSource {
    doc.artwork.paint.get(bake_paint_handle(doc)).unwrap()
}
fn rename_bake(s: &mut UiSession<Recorder>) {
    let doc = s.engine.document();
    let h = bake_owner(doc);
    let mut occurrence = doc.artwork.occurrences.get(h).unwrap().clone();
    occurrence.name = "redoable".into();
    let edit = layer_core::Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,h,Some(occurrence)).unwrap());
    s.engine.apply_edit(edit).unwrap();
}

fn bake_session(linked: bool) -> UiSession<Recorder> {
    use layer_core::color::source::*;
    use layer_core::raster::RasterPlane;
    let mut doc = Document::new(PortableId::random(), 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut source = SourceBuilder::new([16, 8], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: layer_core::color::SampleDepth::U8,
        profile: Default::default(), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    for _ in 0..8 { source.push_row(&[120; 16 * 4]).unwrap(); }
    let owner = bake_owner(&doc);
    let paint = bake_paint_handle(&doc);
    let color = doc.composition().color;
    let p = doc.artwork.paint.get_mut(paint).unwrap();
    p.base = Some(layer_core::PaintBase::new(layer_core::Image::new(std::sync::Arc::new(source.finish().unwrap()))));
    p.domain = [16, 8];
    p.raster = raw_revision(color, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 20);
    doc.artwork.occurrences.get_mut(owner).unwrap().offset = [-17, 13];
    let mask_handle = doc.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(mask_handle, [16, 8], [5, 7]);
    mask.use_.linked = linked;
    mask.source.raster = raw_revision(color, &[RasterPlane::Mask], 90);
    assert_eq!(doc.artwork.coverage.insert(PortableId::random(),mask.source).unwrap(),mask_handle);
    doc.artwork.occurrences.get_mut(owner).unwrap().mask = Some(mask.use_);
    let working = doc.working.clone();
    let mut doc = Document::from_artwork(doc.artwork).unwrap();
    doc.working = working;
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s
}

fn start_bake(s: &mut UiSession<Recorder>) -> Result<(), String> {
    invoke(s, CommandId::ScaleRotate);
    if s.content_bounds.busy() { reply_bounds(s, [0., 0., 16., 8.]); }
    s.set_transform_control("transform_width", 2.)?;
    s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransform }).map(drop)
}

fn frozen_plan(s: &UiSession<Recorder>) -> layer_core::TransformPixelsPlan {
    match s.engine.backend().snapshot_requests.last().unwrap() {
        layer_render::SnapshotRequest::TransformPixels(plan) => plan.clone(),
        _ => panic!("expected pixel bake request"),
    }
}

fn completed_bake(plan: &layer_core::TransformPixelsPlan) -> layer_core::Edit {
    use layer_core::raster::RasterPlane;
    fn complete(edit: &mut layer_core::Edit, color: layer_core::color::DocumentColor, paint: Option<PaintHandle>, coverage: Option<CoverageHandle>) {
        match edit {
            layer_core::Edit::Paint(change) if Some(change.handle) == paint => {
                change.value.as_mut().unwrap().raster = raw_revision(color, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 150);
            }
            layer_core::Edit::Coverage(change) if Some(change.handle) == coverage => {
                change.value.as_mut().unwrap().raster = raw_revision(color, &[RasterPlane::Mask], 210);
            }
            layer_core::Edit::Batch(edits) => for edit in edits { complete(edit,color,paint,coverage); },
            _ => {}
        }
    }
    let mut output = plan.output.clone();
    complete(&mut output,plan.scene.view().composition().color,plan.paint,plan.coverage);
    output
}
fn corrupt_bake_destination(edit: &mut layer_core::Edit) {
    match edit {
        layer_core::Edit::Paint(change) => change.handle = PaintHandle::from_index(999),
        layer_core::Edit::Batch(edits) => for edit in edits { corrupt_bake_destination(edit); },
        _ => {}
    }
}

fn bake_reply(s: &mut UiSession<Recorder>, reply: Result<layer_render::SnapshotResult, layer_render::BackendError>) {
    s.engine.backend_mut().snapshot_reply = Some(reply);
    s.frame(20, 20).unwrap();
    s.frame(21, 21).unwrap();
}

#[test]
fn a_distorted_stroke_commits_though_its_perspective_folds_beyond_the_layer() {
    use layer_core::raster::RasterPlane;
    let mut doc = Document::new(PortableId::random(), 2000, 1500, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let color = doc.composition().color;
    doc.artwork.paint.get_mut(bake_paint_handle(&doc)).unwrap().raster = raw_revision(color, &[RasterPlane::Color], 20);
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [1600, 1000], Platform::Gtk).unwrap();
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate }).unwrap();
    reply_bounds(&mut s, [100., 100., 160., 140.]);
    invoke(&mut s, CommandId::TransformDistort);
    for (corner, to) in [(1, Point { x: 160., y: 115. }), (2, Point { x: 160., y: 125. })] {
        let from = s.operation.quad()[corner];
        s.transform_pen(event(&s, 1, PenPhase::Down, 1.), from).unwrap();
        s.transform_pen(event(&s, 2, PenPhase::Move, 1.), to).unwrap();
        s.transform_pen(event(&s, 3, PenPhase::Up, 1.), to).unwrap();
    }
    s.dispatch(UiAction::Invoke { command: CommandId::ApplyTransform }).unwrap();
    let plan = frozen_plan(&s);
    assert_eq!(plan.source, Some(layer_core::Rect { min: Point { x: 100., y: 100. }, max: Point { x: 160., y: 140. } }));
    assert!(plan.extent.iter().all(|n| *n <= 4000), "the output covers the moved content, not the folded layer: {:?}", plan.extent);
}

#[test]
fn layer_transform_is_pending_then_one_atomic_undo_restores_all_native_roots() {
    for linked in [false, true] {
        let mut s = bake_session(linked);
        let before = s.engine.document().clone();
        start_bake(&mut s).unwrap();
        assert!(s.content_bounds.busy());
        assert!(s.operation.transforming(), "the preview stays until the resampled pixels arrive");
        assert_eq!(s.engine.document(), &before);
        assert!(!s.engine.can_undo());
        let plan = frozen_plan(&s);
        let output = completed_bake(&plan);
        bake_reply(&mut s, Ok(layer_render::SnapshotResult::TransformPixels(output.clone())));
        assert!(!s.content_bounds.busy());
        let mut expected = before.clone();
        expected.apply(output).unwrap();
        assert_live_artwork_eq(s.engine.document(), &expected);
        assert!(!s.operation.transforming());
        assert!(bake_paint(s.engine.document()).base.is_none());
        let after = s.engine.document().clone();
        assert!(s.engine.undo().unwrap());
        assert_live_artwork_eq(s.engine.document(), &before);
        assert!(std::sync::Arc::ptr_eq(bake_paint(s.engine.document()).base.as_ref().unwrap().image.storage(), bake_paint(&before).base.as_ref().unwrap().image.storage()));
        assert!(!s.engine.can_undo(), "one undo restores color, scalar planes, source and mask together");
        assert!(s.engine.redo().unwrap());
        assert_live_artwork_eq(s.engine.document(), &after);
    }
}

#[test]
fn layer_transform_retries_unaccepted_work_and_cancel_discards_late_result() {
    let mut s = bake_session(true);
    let before = s.engine.document().clone();
    s.engine.backend_mut().snapshot_wait = true;
    start_bake(&mut s).unwrap();
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
    bake_reply(&mut s, Ok(layer_render::SnapshotResult::TransformPixels(output)));
    assert_eq!(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
}

#[test]
fn layer_transform_rejects_stale_target_and_renderer_results() {
    for invalidation in 0..4 {
        let mut s = bake_session(true);
        start_bake(&mut s).unwrap();
        let output = completed_bake(&frozen_plan(&s));
        match invalidation {
            0 => { s.replace_renderer(Recorder { tiled_sources: true, ..Default::default() }).unwrap(); },
            1 => {
                let mut working = s.engine.document().working.clone();
                let owner = bake_owner(s.engine.document());
                working.target = Some(SourceTarget::Coverage(s.engine.document().artwork.occurrences.get(owner).unwrap().mask.as_ref().unwrap().source));
                s.engine.apply_edit(layer_core::Edit::Working(working)).unwrap();
            },
            2 => {
                let doc = s.engine.document();
                let paper = occurrence_handle(2).unwrap();
                let mut working = doc.working.clone();
                working.occurrence = Some(paper);
                working.target = None;
                s.engine.apply_edit(layer_core::Edit::Working(working)).unwrap();
            },
            _ => { s.state.document_file.epoch += 1; },
        }
        let changed = s.engine.document().clone();
        bake_reply(&mut s, Ok(layer_render::SnapshotResult::TransformPixels(output)));
        assert!(!s.content_bounds.busy());
        assert_eq!(s.engine.document(), &changed);
        assert!(!s.engine.can_undo());
    }
}

#[test]
fn layer_transform_failures_preserve_existing_redo_and_original_backing() {
    for failure in 0..5 {
        let mut s = bake_session(true);
        rename_bake(&mut s);
        s.engine.undo().unwrap();
        assert!(s.engine.can_redo());
        let before = s.engine.document().clone();
        if failure == 0 { s.engine.backend_mut().snapshot_fails = true; }
        if failure == 1 { s.engine.backend_mut().snapshot_wait = true; }
        let started = start_bake(&mut s);
        if failure == 0 { assert!(started.is_err()); } else { started.unwrap(); }
        if failure == 1 {
            s.engine.backend_mut().snapshot_fails = true;
            s.frame(2, 2).unwrap();
        } else if failure >= 2 {
            let mut output = completed_bake(&frozen_plan(&s));
            let result = match failure {
                2 => Err(layer_render::BackendError("bake failed")),
                3 => Ok(layer_render::SnapshotResult::Bounds(layer_core::Rect::EMPTY)),
                _ => { corrupt_bake_destination(&mut output); Ok(layer_render::SnapshotResult::TransformPixels(output)) },
            };
            bake_reply(&mut s, result);
        }
        assert!(!s.content_bounds.busy());
        s.dispatch(UiAction::Invoke { command: CommandId::CancelTransform }).ok();
        assert_eq!(s.engine.document(), &before);
        assert!(s.engine.can_redo(), "failed bake must retain redo");
        assert!(s.engine.redo().unwrap());
        assert_eq!(s.engine.document().artwork.occurrences.get(bake_owner(s.engine.document())).unwrap().name.as_ref(), "redoable");
    }
}

#[test]
fn renderer_replacement_cancels_the_accepted_bake_on_the_retired_renderer() {
    let mut s = bake_session(true);
    let before = s.engine.document().clone();
    start_bake(&mut s).unwrap();
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
    let owner = bake_owner(&doc);
    let paint = bake_paint_handle(&doc);
    let domain = doc.composition().size;
    let source = doc.artwork.paint.get_mut(paint).unwrap();
    source.base = None;
    source.domain = domain;
    let coverage = doc.artwork.occurrences.get(owner).unwrap().mask.as_ref().unwrap().source;
    doc.artwork.coverage.get_mut(coverage).unwrap().domain = domain;
    doc.artwork.occurrences.get_mut(owner).unwrap().offset = [0, 0];
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
    let valid = s.engine.document().artwork.occurrences.get(bake_owner(s.engine.document())).unwrap().clone();
    let moved_origin = s.engine.document().target_offset(before.active_target().unwrap());
    let original_origin = before.target_offset(before.active_target().unwrap());
    assert_eq!(moved_origin, original_origin.map(|v| v + 10));
    assert!(!s.engine.can_undo());
    send(&mut s, 3, PenPhase::Move, Point { x: 100000., y: 100000. }).unwrap();
    assert_eq!(s.engine.document().artwork.occurrences.get(bake_owner(s.engine.document())).unwrap(), &valid);
    send(&mut s, 4, PenPhase::Up, Point { x: 100000., y: 100000. }).unwrap();
    assert!(s.layer_interaction.path.is_empty());
    assert!(!s.operation.active());
    let owner = SourceTarget::Paint(bake_paint_handle(&before));
    let mask = SourceTarget::Coverage(before.artwork.occurrences.get(bake_owner(&before)).unwrap().mask.as_ref().unwrap().source);
    for target in [owner, mask] {
        let old = before.target_raster(target).unwrap().wait_data().unwrap();
        let new = s.engine.document().target_raster(target).unwrap().wait_data().unwrap();
        assert_eq!(old.tiles.len(), new.tiles.len());
        for (key, tile) in &old.tiles {
            let moved = new.tiles.iter().find(|(next, value)| next.plane == key.plane && value.same_capture(tile)).unwrap().0;
            let original_world = before.local_to_document(target).map(Point { x: key.coordinate[0] as f32 * 256., y: key.coordinate[1] as f32 * 256. });
            let moved_world = s.engine.document().local_to_document(target).map(Point { x: moved.coordinate[0] as f32 * 256., y: moved.coordinate[1] as f32 * 256. });
            assert!((moved_world.x - original_world.x - 10.).abs() < 0.001);
            assert!((moved_world.y - original_world.y - 10.).abs() < 0.001);
        }
    }
    assert!(bake_paint(s.engine.document()).base.is_none());
    assert!(bake_paint(s.engine.document()).domain.iter().all(|axis| *axis <= 512));
    let after = s.engine.document().clone();
    assert!(s.engine.undo().unwrap());
    assert_live_artwork_eq(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
    assert!(s.engine.redo().unwrap());
    assert_live_artwork_eq(s.engine.document(), &after);
}

#[test]
fn source_less_move_click_keeps_canvas_domain_and_existing_redo() {
    let mut doc = Document::new(PortableId::random(), 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let h = bake_paint_handle(&doc);
    let color = doc.composition().color;
    doc.artwork.paint.get_mut(h).unwrap().raster = raw_revision(color, &[layer_core::raster::RasterPlane::Color], 30);
    let mut s = UiSession::new(Recorder::default(), doc, [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    rename_bake(&mut s);
    s.engine.undo().unwrap();
    assert!(s.engine.can_redo());
    let before = s.engine.document().clone();
    assert_eq!(bake_paint(&before).domain, before.composition().size);
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
    assert_eq!(s.engine.document().artwork.occurrences.get(bake_owner(s.engine.document())).unwrap().name.as_ref(), "redoable");
}
