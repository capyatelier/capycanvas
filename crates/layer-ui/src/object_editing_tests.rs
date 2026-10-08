use super::*;
use crate::session::test_support::{Recorder, event, invoke, key, pen_at, select, rectangle};
use layer_core::{Document, DocumentNames, ImageObject, PortableId, SceneScope, color::source::rgba8_source};

fn translate(x: f64, y: f64) -> Affine64 { Affine64([1., 0., 0., 1., x, y]) }
fn session() -> UiSession<Recorder> {
    UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(PortableId::random(), 1000, 1000, DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [1000, 1000], Platform::Gtk).unwrap()
}
fn image(extent: [u32; 2]) -> layer_core::Image { rgba8_source(extent, |x, y| [x as u8, y as u8, 7, 255]).into() }
fn object_layer(s: &mut UiSession<Recorder>, name: &str, parent: Option<OccurrenceHandle>) -> OccurrenceHandle {
    let (layer, edit) = s.engine.document().create_object_layer_edit(name, parent, 0).unwrap();
    s.engine.apply_edit(edit).unwrap();
    layer
}
fn add(s: &mut UiSession<Recorder>, layer: OccurrenceHandle, name: &str, affine: Affine64, at: usize) -> ImageObjectHandle {
    let mut object = ImageObject::new(image([100, 100]), name);
    object.affine = affine;
    let (handle, edit) = s.engine.document().add_image_object_edit(layer, object, at).unwrap();
    s.engine.apply_edit(edit).unwrap();
    handle
}
fn activate(s: &mut UiSession<Recorder>, layer: OccurrenceHandle) {
    s.layer_action(LayerAction::Select { id: occurrence_token(layer), mask: false }).unwrap();
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Move }).unwrap();
    s.refresh_document();
}
fn fixture() -> (UiSession<Recorder>, OccurrenceHandle, [ImageObjectHandle; 2]) {
    let mut s = session();
    let layer = object_layer(&mut s, "Images", None);
    let back = add(&mut s, layer, "Back", translate(100., 100.), 0);
    let front = add(&mut s, layer, "Front", translate(160., 100.), 0);
    activate(&mut s, layer);
    (s, layer, [front, back])
}
fn affine(s: &UiSession<Recorder>, object: ImageObjectHandle) -> Affine64 { s.engine.document().scene().object(object).unwrap().affine }
fn live(artwork: &layer_core::Artwork) -> String {
    format!("{:?}", (artwork.stacks.iter().collect::<Vec<_>>(), artwork.occurrences.iter().collect::<Vec<_>>(), artwork.object_layers.iter().collect::<Vec<_>>(),
        artwork.objects.iter().map(|(h, id, o)| (h, id, o.name.clone(), o.affine, o.visible)).collect::<Vec<_>>(), artwork.coverage.iter().count()))
}
fn near(actual: Affine64, expected: Affine64) { assert!(actual.0.iter().zip(expected.0).all(|(a, b)| (a - b).abs() < 1e-3), "{actual:?} != {expected:?}"); }
fn selected(s: &UiSession<Recorder>) -> BTreeSet<ImageObjectHandle> { s.engine.document().working.objects.clone() }
fn pen(s: &mut UiSession<Recorder>, phase: PenPhase, at: [f32; 2]) {
    pen_at(s, 1, phase, at);
    s.frame(1, 1).unwrap();
}
fn click(s: &mut UiSession<Recorder>, at: [f32; 2]) {
    pen(s, PenPhase::Down, at);
    pen(s, PenPhase::Up, at);
}
fn drag(s: &mut UiSession<Recorder>, from: [f32; 2], to: [f32; 2]) {
    pen(s, PenPhase::Down, from);
    pen(s, PenPhase::Move, [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5]);
    pen(s, PenPhase::Move, to);
    pen(s, PenPhase::Up, to);
}

#[test]
fn move_picks_front_to_back_across_layers_and_a_miss_clears_only_object_selection() {
    let (mut s, layer, [front, back]) = fixture();
    let other = object_layer(&mut s, "Other", None);
    let far = add(&mut s, other, "Far", translate(600., 600.), 0);
    activate(&mut s, layer);
    select(&mut s, rectangle([0., 0., 10., 10.]));
    click(&mut s, [230., 180.]);
    assert_eq!(selected(&s), [front].into());
    click(&mut s, [120., 180.]);
    assert_eq!(selected(&s), [back].into());
    click(&mut s, [650., 650.]);
    assert_eq!(s.engine.document().working.occurrence, Some(other));
    assert_eq!(selected(&s), [far].into());
    click(&mut s, [900., 50.]);
    assert!(selected(&s).is_empty());
    assert_eq!(s.engine.document().working.occurrence, Some(other));
    assert!(s.engine.document().working.selection.is_some(), "a canvas miss keeps the pixel selection");
    assert_eq!(affine(&s, far), translate(600., 600.));
}

#[test]
fn a_drag_whose_first_moves_arrive_before_a_frame_moves_the_image() {
    let (mut s, _, [front, _]) = fixture();
    let checkpoint = s.engine.checkpoint();
    pen_at(&mut s, 1, PenPhase::Down, [230., 180.]);
    pen_at(&mut s, 1, PenPhase::Move, [240., 185.]);
    pen_at(&mut s, 1, PenPhase::Move, [250., 190.]);
    s.frame(1, 1).unwrap();
    pen(&mut s, PenPhase::Up, [250., 190.]);
    near(affine(&s, front), translate(180., 110.));
    assert_ne!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn shift_click_adds_and_removes_within_the_active_layer_only() {
    let (mut s, layer, [front, back]) = fixture();
    let other = object_layer(&mut s, "Other", None);
    let far = add(&mut s, other, "Far", translate(600., 600.), 0);
    activate(&mut s, layer);
    for (at, expected) in [([230., 180.], BTreeSet::from([front])), ([120., 180.], BTreeSet::from([front, back])), ([230., 180.], BTreeSet::from([back]))] {
        s.interaction.modifiers = Modifiers { shift: !selected(&s).is_empty(), ..Default::default() };
        click(&mut s, at);
        assert_eq!(selected(&s), expected);
    }
    s.interaction.modifiers = Modifiers { shift: true, ..Default::default() };
    click(&mut s, [650., 650.]);
    assert_eq!((s.engine.document().working.occurrence, selected(&s)), (Some(layer), BTreeSet::from([back])), "Shift never switches layers");
    assert_eq!(affine(&s, far), translate(600., 600.));
}

#[test]
fn a_drag_previews_from_binary64_starts_and_commits_one_undo_step() {
    let (mut s, _, [front, back]) = fixture();
    s.layer_edit(Edit::Batch(vec![s.engine.document().set_image_object_affines_edit(&[(front, translate(160.25, 100.125))]).unwrap()])).unwrap();
    let checkpoint = s.engine.checkpoint();
    drag(&mut s, [230., 180.], [240., 205.]);
    near(affine(&s, front), translate(170.25, 125.125));
    assert_eq!(affine(&s, back), translate(100., 100.));
    assert_eq!(s.engine.backend().moving_layer, None);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(affine(&s, front), translate(160.25, 100.125));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    invoke(&mut s, CommandId::Redo);
    near(affine(&s, front), translate(170.25, 125.125));
}

#[test]
fn a_cancelled_drag_and_cancelled_alt_copy_leave_no_records_or_history() {
    let (mut s, _, [front, _]) = fixture();
    click(&mut s, [230., 180.]);
    let artwork = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    pen(&mut s, PenPhase::Down, [230., 180.]);
    pen(&mut s, PenPhase::Move, [250., 180.]);
    assert_eq!(s.engine.backend().moving_layer, s.engine.document().working.occurrence);
    pen(&mut s, PenPhase::Cancel, [250., 180.]);
    assert_eq!(live(&s.engine.document().artwork), live(&artwork));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    s.interaction.modifiers = Modifiers { alt: true, ..Default::default() };
    pen(&mut s, PenPhase::Down, [230., 180.]);
    pen(&mut s, PenPhase::Move, [270., 180.]);
    assert_eq!(s.engine.document().artwork.objects.iter().count(), 3);
    pen(&mut s, PenPhase::Cancel, [270., 180.]);
    assert_eq!(live(&s.engine.document().artwork), live(&artwork));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    pen(&mut s, PenPhase::Down, [230., 180.]);
    pen(&mut s, PenPhase::Move, [270., 180.]);
    pen(&mut s, PenPhase::Up, [270., 180.]);
    let copy = *selected(&s).iter().next().unwrap();
    assert_ne!(copy, front);
    assert_eq!(affine(&s, front), translate(160., 100.));
    near(affine(&s, copy), translate(200., 100.));
    assert!(s.engine.document().scene().object(copy).unwrap().image.same_owner(&s.engine.document().scene().object(front).unwrap().image));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(live(&s.engine.document().artwork), live(&artwork));
}

#[test]
fn held_arrow_nudges_coalesce_into_one_step_and_escape_cancels() {
    let (mut s, layer, [front, _]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    let checkpoint = s.engine.checkpoint();
    for (shift, repeat) in [(false, false), (false, true), (true, true)] {
        s.input(UiInput::Key { key: "ArrowRight".into(), pressed: true, repeat, modifiers: Modifiers { shift, ..Default::default() }, editing: false, divider: None }).unwrap();
    }
    assert_eq!(affine(&s, front), translate(172., 100.));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    key(&mut s, "ArrowRight", false, false, false);
    assert_ne!(s.engine.checkpoint(), checkpoint);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(affine(&s, front), translate(160., 100.));
    key(&mut s, "ArrowDown", true, false, false);
    assert_eq!(affine(&s, front), translate(160., 101.));
    key(&mut s, "Escape", true, false, false);
    assert_eq!(affine(&s, front), translate(160., 100.));
    assert_eq!(selected(&s), [front].into(), "Escape cancels the live nudge before clearing the selection");
    key(&mut s, "ArrowDown", false, false, false);
    key(&mut s, "Escape", false, false, false);
    key(&mut s, "Escape", true, false, false);
    assert!(selected(&s).is_empty());
}

#[test]
fn shortcut_commands_follow_the_visible_object_or_pixel_target() {
    let (mut s, layer, [front, back]) = fixture();
    let hidden = add(&mut s, layer, "Hidden", translate(400., 400.), 2);
    s.layer_edit(s.engine.document().set_image_object_visible_edit(hidden, false).unwrap()).unwrap();
    select(&mut s, rectangle([0., 0., 50., 50.]));
    let pixels = s.engine.document().working.selection.clone();
    assert_eq!(s.command(CommandId::SelectAll).label.as_ref(), s.localization().text(MessageId::OBJECTS_SELECT_ALL_IMAGES).as_ref());
    invoke(&mut s, CommandId::SelectAll);
    assert_eq!(selected(&s), [front, back, hidden].into());
    assert_eq!(s.engine.document().working.selection, pixels);
    invoke(&mut s, CommandId::Deselect);
    assert!(selected(&s).is_empty());
    assert_eq!(s.engine.document().working.selection, pixels);
    assert!(!s.command(CommandId::ClearSelected).enabled);
    assert!(!s.command(CommandId::CutSelectionToLayer).enabled);
    assert_eq!(s.command(CommandId::CutSelectionToLayer).disabled_reason.as_deref(), Some(s.localization().text(MessageId::OBJECTS_CUT_TO_LAYER_UNAVAILABLE).as_ref()));
    s.select_objects(layer, [front].into()).unwrap();
    invoke(&mut s, CommandId::CopySelectionToLayer);
    let copy = *selected(&s).iter().next().unwrap();
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[copy, front, back, hidden]);
    assert_eq!(s.engine.document().scene().occurrence(layer).map(|l| l.name.clone()).unwrap().as_ref(), "Images");
    invoke(&mut s, CommandId::ClearSelected);
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[front, back, hidden]);
    assert!(s.engine.document().scene().occurrence(layer).is_some(), "Delete never removes the object layer");
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Selection { kind: SelectionTool::Rectangle } }).unwrap();
    assert!(s.object_target().is_none());
    invoke(&mut s, CommandId::SelectAll);
    assert!(selected(&s).is_empty());
    assert_ne!(s.engine.document().working.selection, pixels);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Move }).unwrap();
    assert_eq!(s.object_target(), Some(layer));
}

#[test]
fn transform_commands_apply_exact_document_space_maps_about_the_pivot() {
    let (mut s, layer, [front, back]) = fixture();
    s.select_objects(layer, [front, back].into()).unwrap();
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    assert_eq!(affine(&s, front), Affine64([-1., 0., 0., 1., 200., 100.]));
    assert_eq!(affine(&s, back), Affine64([-1., 0., 0., 1., 260., 100.]));
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    invoke(&mut s, CommandId::TransformRotateRight);
    assert_eq!(affine(&s, front), Affine64([0., 1., -1., 0., 230., 130.]));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(affine(&s, front), translate(160., 100.));
    s.select_objects(layer, [front].into()).unwrap();
    drag(&mut s, [230., 180.], [250., 180.]);
    s.select_objects(layer, [back].into()).unwrap();
    invoke(&mut s, CommandId::TransformAgain);
    near(affine(&s, back), translate(120., 100.));
    s.layer_edit(s.engine.document().set_image_object_affines_edit(&[(back, Affine64([0., -3., 2., 0., 10., 20.]))]).unwrap()).unwrap();
    invoke(&mut s, CommandId::PlacementOriginalSize);
    let restored = affine(&s, back);
    let centre = |a: Affine64| a.map([50., 50.]);
    assert_eq!(centre(restored), centre(Affine64([0., -3., 2., 0., 10., 20.])));
    assert!((restored.0[0] - 0.).abs() < 1e-12 && (restored.0[1] + 1.).abs() < 1e-12 && (restored.0[2] - 1.).abs() < 1e-12 && restored.0[3].abs() < 1e-12);
    invoke(&mut s, CommandId::TransformNearest);
    assert_eq!(s.engine.document().scene().object(back).unwrap().interpolation, ImageInterpolation::Nearest);
    assert!(s.command(CommandId::TransformNearest).selected);
    assert!(!s.command(CommandId::TransformBicubic).enabled);
    assert!(!s.command(CommandId::TransformDistort).enabled);
}

#[test]
fn original_size_uses_the_orthogonal_polar_factor_and_keeps_mirrors() {
    for affine in [Affine64([2., 0., 0., 3., 5., 7.]), Affine64([0.5, 1., -2., 0.25, -3., 4.]), Affine64([-2., 0.5, 0.5, 3., 1., 1.])] {
        let result = original_size(affine, [10, 6]);
        let [a, b, c, d, _, _] = result.0;
        assert!((a * a + b * b - 1.).abs() < 1e-12 && (c * c + d * d - 1.).abs() < 1e-12 && (a * c + b * d).abs() < 1e-12);
        let original_det = affine.0[0] * affine.0[3] - affine.0[1] * affine.0[2];
        assert_eq!((a * d - b * c).signum(), original_det.signum());
        let [x, y] = result.map([5., 3.]);
        let [u, v] = affine.map([5., 3.]);
        assert!((x - u).abs() < 1e-12 && (y - v).abs() < 1e-12);
    }
}

#[test]
fn the_object_canvas_bar_names_image_commands() {
    let (mut s, _, [front, _]) = fixture();
    s.object_action(ObjectAction::Select { id: object_token(front), extend: false }).unwrap();
    s.frame(1, 1).unwrap();
    let bar = s.state.canvas_bar.clone().expect("an image canvas bar");
    let labels: Vec<String> = bar.items.iter().flat_map(|item| match &item.option {
        ToolOption::Choice { items, .. } => items.iter().map(|choice| choice.label.to_string()).collect(),
        _ => vec![item.label.to_string()],
    }).collect();
    let l = s.localization();
    for expected in [MessageId::OBJECTS_DUPLICATE_IMAGES, MessageId::OBJECTS_DELETE_IMAGES, MessageId::OBJECTS_INTERPOLATION_LINEAR, MessageId::OBJECTS_ORIGINAL_SIZE] {
        assert!(labels.iter().any(|label| *label == *l.text(expected)), "{expected:?} in {labels:?}");
    }
    assert!(!labels.iter().any(|label| *label == *CommandId::CopySelectionToLayer.localized_label(l)), "{labels:?}");
}

#[test]
fn object_rows_project_children_and_reorder_only_inside_their_layer() {
    let (mut s, layer, [front, back]) = fixture();
    let other = object_layer(&mut s, "Other", None);
    let far = add(&mut s, other, "", translate(600., 600.), 0);
    s.object_action(ObjectAction::Expand { layer: occurrence_token(layer), expanded: true }).unwrap();
    s.object_action(ObjectAction::Expand { layer: occurrence_token(other), expanded: true }).unwrap();
    s.refresh_document();
    let row = |s: &UiSession<Recorder>, layer| s.state.layers.iter().find(|row| row.id == occurrence_token(layer)).unwrap().clone();
    let rows = row(&s, layer);
    assert_eq!((rows.object_count, rows.expanded), (2, true));
    assert_eq!(rows.description, "2 images");
    assert_ne!(rows.objects[0].thumbnail_revision, rows.objects[1].thumbnail_revision, "each image previews its own pixels");
    assert_eq!(layer_render::ThumbnailTarget::from_wire_id(rows.objects[0].id), Some(layer_render::ThumbnailTarget::Object(front)));
    assert_eq!(rows.objects.iter().map(|r| (r.id, r.label.as_str(), r.can_raise, r.can_lower)).collect::<Vec<_>>(),
        vec![(object_token(front), "Front", false, true), (object_token(back), "Back", true, false)]);
    assert_eq!(row(&s, other).objects[0].label, s.localization().text(MessageId::OBJECTS_UNNAMED_IMAGE).as_ref());
    assert!(object_handle(occurrence_token(layer)).is_err());
    assert!(occurrence_handle(object_token(front)).is_err());
    s.object_action(ObjectAction::Select { id: object_token(back), extend: false }).unwrap();
    assert_eq!(selected(&s), [back].into());
    s.object_action(ObjectAction::Order { order: ObjectOrder::Front }).unwrap();
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[back, front]);
    s.object_action(ObjectAction::Drop { id: object_token(back), target: object_token(front), below: true }).unwrap();
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[front, back]);
    assert!(s.object_action(ObjectAction::Drop { id: object_token(back), target: object_token(far), below: false }).is_err());
    s.object_action(ObjectAction::Visibility { id: object_token(front), visible: false }).unwrap();
    s.refresh_document();
    assert!(!row(&s, layer).objects[0].visible);
    s.object_action(ObjectAction::Select { id: object_token(far), extend: true }).unwrap();
    assert_eq!((s.engine.document().working.occurrence, selected(&s)), (Some(other), BTreeSet::from([far])));
    let menu = s.object_menu(object_token(far)).unwrap();
    assert!(menu.sections.iter().flatten().any(|item| item.action == Some(UiAction::Object { action: ObjectAction::Delete }) && item.enabled));
}

#[test]
fn placement_inserts_objects_with_one_commit_and_cancel_leaves_no_records() {
    let mut s = session();
    let source = || rgba8_source([200, 100], |x, _| [x as u8, 0, 0, 255]);
    let artwork = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    s.place_layer_sources(vec![("Photo".into(), std::sync::Arc::unwrap_or_clone(source()))], Some(Point { x: 300., y: 300. }), None).unwrap();
    assert!(s.objects.placing());
    let layer = s.engine.document().working.occurrence.unwrap();
    let object = *selected(&s).iter().next().unwrap();
    assert_eq!(affine(&s, object), translate(200., 250.));
    drag(&mut s, [250., 320.], [260., 320.]);
    near(affine(&s, object), translate(210., 250.));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    invoke(&mut s, CommandId::CancelTransform);
    assert!(!s.objects.placing());
    assert_eq!(live(&s.engine.document().artwork), live(&artwork));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    s.place_layer_sources(vec![("Photo".into(), std::sync::Arc::unwrap_or_clone(source()))], Some(Point { x: 300., y: 300. }), None).unwrap();
    drag(&mut s, [250., 320.], [260., 320.]);
    invoke(&mut s, CommandId::ApplyTransform);
    let layer = s.engine.document().working.occurrence.unwrap_or(layer);
    let first = *selected(&s).iter().next().unwrap();
    near(affine(&s, first), translate(210., 250.));
    assert_eq!(s.engine.document().scene().object_layer(layer).unwrap().children, vec![first]);
    s.place_layer_sources(vec![("Second".into(), std::sync::Arc::unwrap_or_clone(source()))], None, None).unwrap();
    invoke(&mut s, CommandId::ApplyTransform);
    assert_eq!(s.engine.document().scene().object_layer(layer).unwrap().children.len(), 2, "a second paste enters the active object layer");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().scene().object_layer(layer).unwrap().children, vec![first]);
    s.place_layer_sources(vec![("Third".into(), std::sync::Arc::unwrap_or_clone(source()))], None, None).unwrap();
    invoke(&mut s, CommandId::Undo);
    assert!(!s.objects.placing());
    assert_eq!(s.engine.document().scene().object_layer(layer).unwrap().children, vec![first]);
}

#[test]
fn touch_lands_on_unselected_images_and_object_layers_join_snap_candidates() {
    let (mut s, layer, [front, _]) = fixture();
    let surface = |s: &UiSession<Recorder>, p: [f32; 2]| { let q = crate::session::test_support::on_surface(s, Point { x: p[0], y: p[1] }); [q.x, q.y] };
    assert!(s.object_touch_hit(surface(&s, [230., 180.])));
    assert!(!s.object_touch_hit(surface(&s, [900., 900.])));
    let other = object_layer(&mut s, "Other", None);
    add(&mut s, other, "Far", translate(600., 600.), 0);
    activate(&mut s, layer);
    s.select_objects(layer, [front].into()).unwrap();
    let candidates = s.measured_snap_bounds();
    assert!(candidates.iter().any(|(id, bounds)| *id == other && bounds.min == Point { x: 600., y: 600. } && bounds.max == Point { x: 700., y: 700. }));
    assert!(candidates.iter().all(|(id, _)| *id != layer));
}

fn pending_copy(s: &UiSession<Recorder>) -> u32 {
    s.state.requests.iter().find_map(|r| match &r.kind {
        HostRequestKind::Document { request: DocumentRequest::Copy { .. } } => Some(r.id),
        _ => None,
    }).expect("a copy request")
}

#[test]
fn painting_on_an_image_layer_offers_mask_paint_and_rasterize_actions_that_revalidate() {
    let (mut s, layer, _) = fixture();
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Paint }).unwrap();
    assert_eq!(s.engine.document().drawing_refusal(), Some(layer_core::DrawingRefusal::Object));
    pen(&mut s, PenPhase::Down, [600., 600.]);
    pen(&mut s, PenPhase::Up, [600., 600.]);
    let notice = s.state.notice.clone().expect("a refusal notice");
    let mut args = FluentArgs::new(); args.set("layer", "Images");
    assert_eq!(notice.text, s.localization().format(MessageId::OBJECTS_REFUSAL_PAINT, &args));
    assert_eq!(notice.actions.iter().map(|a| (a.id, a.enabled)).collect::<Vec<_>>(),
        vec![(NoticeActionId::AddMask, true), (NoticeActionId::NewPaintLayer, true), (NoticeActionId::RasterizeLayer, true)]);
    assert!(s.engine.document().scene().object_layer(layer).is_some_and(|l| l.children.len() == 2), "no stroke reached the images");
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::RasterizeLayer) }).unwrap();
    assert!(matches!(s.engine.backend().snapshot_requests.last(), Some(layer_render::SnapshotRequest::Image(capture)) if capture.scope == SceneScope::RawObjects(layer)));
    assert!(s.engine.document().scene().object_layer(layer).is_some(), "rasterizing waits for its capture");
    assert!(s.cancel_conversion());
    s.notify_drawing_refusal();
    let notice = s.state.notice.clone().unwrap();
    s.set_localization(Localizer::shared(UiLanguage::Japanese));
    let localized = s.state.notice.clone().unwrap();
    assert_eq!((localized.id, localized.actions.len()), (notice.id, 3));
    assert_ne!(localized.actions[0].label, notice.actions[0].label);
    s.set_localization(Localizer::shared(UiLanguage::English));
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::AddMask) }).unwrap();
    let doc = s.engine.document();
    let mask = doc.scene().occurrence(layer).unwrap().mask.clone().expect("a reveal-all mask");
    assert_eq!(doc.working.target, Some(SourceTarget::Coverage(mask.source)));
    assert_eq!(doc.artwork.coverage.get(mask.source).unwrap().default_coverage, 1.);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Paint);
    s.layer_action(LayerAction::Select { id: occurrence_token(layer), mask: false }).unwrap();
    s.notify_drawing_refusal();
    let notice = s.state.notice.clone().unwrap();
    assert_eq!(notice.actions[0].id, NoticeActionId::EditMask);
    let paint = *s.engine.document().scene().order().iter().find(|h| s.engine.document().scene().paint_source(**h).is_some()).unwrap();
    s.layer_action(LayerAction::Select { id: occurrence_token(paint), mask: false }).unwrap();
    assert!(s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::NewPaintLayer) }).is_err(), "the target changed");
    s.layer_action(LayerAction::Select { id: occurrence_token(layer), mask: false }).unwrap();
    s.notify_drawing_refusal();
    let notice = s.state.notice.clone().unwrap();
    let before = s.engine.document().scene().order().len();
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::NewPaintLayer) }).unwrap();
    let doc = s.engine.document();
    assert_eq!(doc.scene().order().len(), before + 1);
    let created = doc.working.occurrence.unwrap();
    assert!(doc.scene().paint_source(created).is_some());
    assert_eq!(doc.scene().position(created).unwrap() + 1, doc.scene().position(layer).unwrap());
}

#[test]
fn copying_images_publishes_a_structured_clip_and_cut_removes_them_only_after_success() {
    let (mut s, layer, [front, back]) = fixture();
    s.layer_edit(s.engine.document().set_image_object_affines_edit(&[(front, Affine64([0., 2., -2., 0., -50.5, 30.25]))]).unwrap()).unwrap();
    s.select_objects(layer, [front, back].into()).unwrap();
    assert!(s.command(CommandId::Copy).enabled);
    invoke(&mut s, CommandId::Copy);
    let id = pending_copy(&s);
    let capture = s.capture_clipboard(id).unwrap();
    assert_eq!(capture.origin, [-251, 30]);
    assert_eq!(capture.crop, [0, 0, 451, 201]);
    assert_eq!(capture.scope, SceneScope::RawObjects(layer));
    assert_eq!(capture.window, Some(([-251, 30], [451, 201])), "the copy evaluates a signed window");
    let view = capture.scene.view();
    assert_eq!(view.composition().size, [1000, 1000], "the copy keeps the authored frame");
    assert_eq!(view.object_layer(layer).unwrap().children, vec![front, back]);
    let objects = capture.objects.clone().unwrap();
    assert_eq!(objects.objects.iter().map(|o| o.affine).collect::<Vec<_>>(), vec![Affine64([0., 2., -2., 0., -50.5, 30.25]), translate(100., 100.)]);
    let clip = capture.finish("nonce".into(), rgba8_source([451, 201], |_, _| [0; 4]), vec![1, 2, 3]).unwrap();
    s.complete_document_request(id, Ok(true)).unwrap();
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[front, back]);
    s.paste_clip(&clip, PasteMode::InPlace).unwrap();
    let doc = s.engine.document();
    let children = doc.object_layer_children(layer).unwrap().to_vec();
    assert_eq!(children.len(), 4);
    let pasted: Vec<_> = children.iter().copied().filter(|h| doc.working.objects.contains(h)).collect();
    assert_eq!(pasted.len(), 2);
    assert!(pasted.iter().all(|h| ![front, back].contains(h)));
    assert_eq!(doc.scene().object(pasted[0]).unwrap().affine, Affine64([0., 2., -2., 0., -50.5, 30.25]));
    assert_eq!(doc.scene().object(pasted[0]).unwrap().image.id(), doc.scene().object(front).unwrap().image.id());
    assert!(doc.scene().object(pasted[0]).unwrap().image.same_owner(&doc.scene().object(front).unwrap().image));
    invoke(&mut s, CommandId::Undo);
    s.select_objects(layer, [back].into()).unwrap();
    invoke(&mut s, CommandId::Cut);
    let id = pending_copy(&s);
    s.capture_clipboard(id).unwrap();
    s.complete_document_request(id, Ok(false)).unwrap();
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[front, back], "a failed Cut keeps the images");
    invoke(&mut s, CommandId::Cut);
    let id = pending_copy(&s);
    s.capture_clipboard(id).unwrap();
    s.complete_document_request(id, Ok(true)).unwrap();
    assert_eq!(s.engine.document().object_layer_children(layer).unwrap(), &[front]);
}

#[test]
fn pasting_a_clip_from_another_drawing_remaps_images_into_the_destination_layer_frame() {
    let (mut source, layer, [front, _]) = fixture();
    source.select_objects(layer, [front].into()).unwrap();
    invoke(&mut source, CommandId::Copy);
    let id = pending_copy(&source);
    let clip = source.capture_clipboard(id).unwrap().finish("nonce".into(), rgba8_source([100, 100], |_, _| [0; 4]), vec![]).unwrap();
    let mut s = session();
    let edit = s.engine.document().group_layers_edit(&[s.engine.document().scene().order()[0]], layer_core::LayerBlend::Normal, "Group").unwrap();
    s.engine.apply_edit(edit).unwrap();
    let group = s.engine.document().scene().order()[0];
    let mut occurrence = s.engine.document().scene().occurrence(group).unwrap().clone();
    occurrence.offset = [40, -10];
    s.engine.apply_edit(Edit::Occurrence(layer_core::RecordChange::replace(&s.engine.document().artwork.occurrences, group, Some(occurrence)).unwrap())).unwrap();
    let destination = object_layer(&mut s, "Destination", Some(group));
    activate(&mut s, destination);
    s.paste_clip(&clip, PasteMode::InPlace).unwrap();
    let doc = s.engine.document();
    let pasted = *doc.working.objects.iter().next().unwrap();
    assert_eq!(doc.scene().object_owner(pasted), Some(destination));
    assert_eq!(doc.scene().object(pasted).unwrap().affine, translate(120., 110.));
    assert_eq!(doc.object_document_affine(pasted).unwrap(), translate(160., 100.));
    assert_ne!(doc.scene().object(pasted).unwrap().image.id(), source.engine.document().scene().object(front).unwrap().image.id());
}

#[test]
fn paste_into_a_selection_makes_a_masked_object_layer_and_moving_the_image_keeps_the_mask() {
    let mut s = session();
    select(&mut s, rectangle([100., 100., 300., 300.]));
    let context = s.image_placement_context(None, None).unwrap();
    s.paste_layer_sources(vec![("Photo".into(), std::sync::Arc::unwrap_or_clone(rgba8_source([160, 120], |_, _| [9; 4])))], PasteMode::Into, &context).unwrap();
    let doc = s.engine.document();
    let layer = doc.working.occurrence.unwrap();
    let object = *doc.working.objects.iter().next().unwrap();
    assert!(doc.scene().object_layer(layer).is_some());
    let mask = doc.scene().occurrence(layer).unwrap().mask.clone().expect("a mask from the selection");
    assert!(doc.working.selection.is_none());
    let start = affine(&s, object);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Move }).unwrap();
    let inside = start.map([30., 30.]);
    drag(&mut s, [inside[0] as f32, inside[1] as f32], [inside[0] as f32 + 20., inside[1] as f32]);
    near(affine(&s, object), Affine64([1., 0., 0., 1., start.0[4] + 20., start.0[5]]));
    assert_eq!(s.engine.document().scene().occurrence(layer).unwrap().mask, Some(mask));
}

#[test]
fn paste_into_centres_pixel_external_and_image_clips_on_the_selection() {
    let (mut s, layer, [front, back]) = fixture();
    s.select_objects(layer, [front, back].into()).unwrap();
    invoke(&mut s, CommandId::Copy);
    let id = pending_copy(&s);
    let images = s.capture_clipboard(id).unwrap().finish("images".into(), rgba8_source([160, 100], |_, _| [0; 4]), vec![]).unwrap();
    s.complete_document_request(id, Ok(true)).unwrap();
    let pixels = PixelClip { objects: None, origin: [10, 10], ..images.clone() };
    let paste_into = |s: &mut UiSession<Recorder>, paste: &dyn Fn(&mut UiSession<Recorder>) -> Result<(), String>| {
        select(s, rectangle([600., 500., 700., 560.]));
        paste(s).unwrap();
        let doc = s.engine.document();
        let layer = doc.working.occurrence.unwrap();
        assert!(doc.scene().occurrence(layer).unwrap().mask.is_some(), "Paste Into masks a new image layer");
        doc.object_layer_children(layer).unwrap().iter().map(|h| doc.object_document_affine(*h).unwrap()).collect::<Vec<_>>()
    };
    assert_eq!(paste_into(&mut s, &|s| s.paste_clip(&images, PasteMode::Into)), vec![translate(630., 480.), translate(570., 480.)]);
    assert_eq!(paste_into(&mut s, &|s| s.paste_clip(&pixels, PasteMode::Into)), vec![translate(570., 480.)]);
    let photo = || vec![("Photo".to_string(), std::sync::Arc::unwrap_or_clone(rgba8_source([40, 20], |_, _| [9; 4])))];
    assert_eq!(paste_into(&mut s, &|s| s.image_placement_context(None, None).and_then(|context| s.paste_layer_sources(photo(), PasteMode::Into, &context))),
        vec![translate(630., 520.)]);
}

#[test]
fn pixel_clear_and_cut_on_image_content_raise_the_image_refusal() {
    let (mut s, layer, _) = fixture();
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Selection { kind: SelectionTool::Rectangle } }).unwrap();
    select(&mut s, rectangle([120., 120., 180., 180.]));
    assert!(s.object_target().is_none());
    let before = live(&s.engine.document().artwork);
    let checkpoint = s.engine.checkpoint();
    let refused = |s: &mut UiSession<Recorder>, command: CommandId| {
        s.dismiss_notice();
        assert!(s.command(command).enabled, "{command:?} refuses with the image actions");
        if command == CommandId::ClearSelected {
            key(s, "delete", true, false, false);
            key(s, "delete", false, false, false);
        } else {
            invoke(s, command);
        }
        let notice = s.state.notice.clone().unwrap_or_else(|| panic!("{command:?} raises a notice"));
        assert_eq!(notice.actions.iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![NoticeActionId::AddMask, NoticeActionId::NewPaintLayer, NoticeActionId::RasterizeLayer], "{command:?}");
    };
    for command in [CommandId::ClearSelected, CommandId::ClearOutside, CommandId::Cut, CommandId::CutSelectionToLayer] {
        refused(&mut s, command);
    }
    assert_eq!(live(&s.engine.document().artwork), before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert!(!s.state.requests.iter().any(|r| matches!(r.kind, HostRequestKind::Document { request: DocumentRequest::Copy { .. } })), "a refused Cut publishes nothing");
    invoke(&mut s, CommandId::Copy);
    let capture = s.capture_clipboard(pending_copy(&s)).unwrap();
    assert_eq!((capture.scope, capture.objects.is_none()), (SceneScope::RawObjects(layer), true), "pixel Copy reads the images");
}

#[test]
fn fill_clear_layer_and_frequency_separation_on_images_raise_the_image_refusal() {
    let (mut s, _, _) = fixture();
    invoke(&mut s, CommandId::BlendPerceptual);
    select(&mut s, rectangle([120., 120., 180., 180.]));
    let before = live(&s.engine.document().artwork);
    let checkpoint = s.engine.checkpoint();
    for tool in [LayerCanvasTool::Move, LayerCanvasTool::Selection { kind: SelectionTool::Rectangle }] {
        s.layer_action(LayerAction::Tool { tool }).unwrap();
        for command in [CommandId::FillSelection, CommandId::ClearLayer, CommandId::ClearOutside, CommandId::FrequencySeparation] {
            s.dismiss_notice();
            assert!(s.command(command).enabled, "{tool:?} {command:?} refuses with the image actions");
            invoke(&mut s, command);
            assert_eq!(s.state.notice.as_ref().map(|n| n.actions.iter().map(|a| a.id).collect::<Vec<_>>()),
                Some(vec![NoticeActionId::AddMask, NoticeActionId::NewPaintLayer, NoticeActionId::RasterizeLayer]), "{tool:?} {command:?}");
        }
    }
    assert!(s.frequency_separation_view().is_none());
    assert_eq!(live(&s.engine.document().artwork), before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn choosing_an_image_while_its_layer_mask_is_edited_moves_the_image_behind_the_mask() {
    let (mut s, layer, [front, back]) = fixture();
    let id = occurrence_token(layer);
    s.select_objects(layer, [front].into()).unwrap();
    s.layer_action(LayerAction::AddMask { id, replace: false }).unwrap();
    s.layer_action(LayerAction::LinkMask { id, value: false }).unwrap();
    let mask = s.engine.document().scene().occurrence(layer).unwrap().mask.clone().unwrap();
    for (image, start) in [(front, translate(160., 100.)), (back, translate(100., 100.))] {
        s.layer_action(LayerAction::Select { id, mask: true }).unwrap();
        assert_eq!(s.engine.document().working.target, Some(SourceTarget::Coverage(mask.source)));
        s.object_action(ObjectAction::Select { id: object_token(image), extend: false }).unwrap();
        assert_eq!((s.engine.document().working.target, s.object_target()), (None, Some(layer)), "choosing an image edits the images");
        let grab = start.map([30., 70.]);
        drag(&mut s, [grab[0] as f32, grab[1] as f32], [grab[0] as f32 + 30., grab[1] as f32 + 20.]);
        near(affine(&s, image), Affine64([1., 0., 0., 1., start.0[4] + 30., start.0[5] + 20.]));
        assert_eq!(s.engine.document().scene().occurrence(layer).unwrap().mask, Some(mask.clone()), "the mask stays in place");
    }
}

#[test]
fn input_held_for_painting_readiness_keeps_saving_waiting() {
    let (mut s, layer, [front, _]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    s.set_input_held(true);
    for command in [CommandId::SaveDocument, CommandId::SaveDocumentAs] { assert!(!s.command(command).enabled, "{command:?} waits for the held input"); }
    for command in [CommandId::Undo, CommandId::TransformFlipVertical] { assert!(s.command(command).enabled, "{command:?} is not a document snapshot"); }
    s.set_input_held(false);
    assert!(s.command(CommandId::SaveDocumentAs).enabled);
}

#[test]
fn dragging_the_pivot_moves_the_turn_centre_without_history() {
    let (mut s, layer, [front, _]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    let checkpoint = s.engine.checkpoint();
    drag(&mut s, [210., 150.], [160., 100.]);
    assert_eq!(affine(&s, front), translate(160., 100.));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    invoke(&mut s, CommandId::TransformRotateRight);
    near(affine(&s, front), Affine64([0., 1., -1., 0., 160., 100.]));
}

#[test]
fn rotating_and_scaling_handles_edit_the_selection_about_its_frame() {
    let (mut s, layer, [front, _]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    drag(&mut s, [260., 200.], [310., 250.]);
    near(affine(&s, front), Affine64([1.5, 0., 0., 1.5, 160., 100.]));
    invoke(&mut s, CommandId::Undo);
    s.interaction.modifiers = Modifiers { alt: true, ..Default::default() };
    drag(&mut s, [260., 150.], [310., 150.]);
    near(affine(&s, front), Affine64([2., 0., 0., 1., 110., 100.]));
    s.interaction.modifiers = Modifiers::default();
    invoke(&mut s, CommandId::Undo);
    let reach = f64::from(s.ruler_reach());
    let rotate = [210., 100. - reach * 2.5];
    drag(&mut s, [rotate[0] as f32, rotate[1] as f32], [260., 150.]);
    near(affine(&s, front), Affine64([0., 1., -1., 0., 260., 100.]));
}

#[test]
fn reset_restores_inserted_affines_and_move_layer_keeps_an_explicit_integer_target() {
    let mut s = session();
    s.place_layer_sources(vec![("Photo".into(), std::sync::Arc::unwrap_or_clone(rgba8_source([200, 100], |_, _| [1; 4])))], Some(Point { x: 300., y: 300. }), None).unwrap();
    let object = *selected(&s).iter().next().unwrap();
    drag(&mut s, [250., 320.], [280., 340.]);
    assert_ne!(affine(&s, object), translate(200., 250.));
    invoke(&mut s, CommandId::ResetTransform);
    assert_eq!(affine(&s, object), translate(200., 250.));
    invoke(&mut s, CommandId::ApplyTransform);
    let layer = s.engine.document().scene().object_owner(object).unwrap();
    s.layer_action(LayerAction::MoveLayer { id: occurrence_token(layer) }).unwrap();
    assert!(s.object_target().is_none(), "Move Layer is a layer target, not object picking");
    drag(&mut s, [250., 320.], [262.4, 331.6]);
    assert_eq!(affine(&s, object), translate(200., 250.), "a layer move never edits an object affine");
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Move }).unwrap();
    assert_eq!(s.object_target(), Some(layer));
}

fn layer_drag(s: &mut UiSession<Recorder>, sequence: u64, from: [f32; 2], to: [f32; 2]) {
    for (i, (phase, at)) in [(PenPhase::Down, from), (PenPhase::Move, to), (PenPhase::Up, to)].into_iter().enumerate() {
        pen_at(s, sequence + i as u64, phase, at);
        s.frame(sequence + i as u64, sequence + i as u64).unwrap();
    }
}

#[test]
fn move_layer_moves_image_layers_and_their_groups_by_whole_pixels_with_mask_linkage() {
    let mut s = session();
    let layer = object_layer(&mut s, "Images", None);
    let object = add(&mut s, layer, "Photo", translate(100., 100.), 0);
    s.layer_action(LayerAction::AddMask { id: occurrence_token(layer), replace: false }).unwrap();
    let mask = SourceTarget::Coverage(s.engine.document().scene().occurrence(layer).unwrap().mask.as_ref().unwrap().source);
    let origin = |s: &UiSession<Recorder>| s.engine.document().scene().target_origin(mask);
    let offset = |s: &UiSession<Recorder>, h: OccurrenceHandle| s.engine.document().scene().occurrence(h).unwrap().offset;
    let linked = origin(&s);
    s.layer_action(LayerAction::MoveLayer { id: occurrence_token(layer) }).unwrap();
    layer_drag(&mut s, 1, [150., 150.], [137.6, 141.6]);
    assert_eq!(offset(&s, layer), [-12, -8]);
    assert_eq!(affine(&s, object), translate(100., 100.), "the layer moves, not its image");
    assert_eq!(origin(&s), [linked[0] - 12, linked[1] - 8], "a linked mask follows its image layer");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(offset(&s, layer), [0, 0]);

    let mut occurrence = s.engine.document().scene().occurrence(layer).unwrap().clone();
    let owner = occurrence.clone();
    occurrence.mask.as_mut().unwrap().set_linked(false, &owner).unwrap();
    s.engine.apply_edit(Edit::Occurrence(layer_core::RecordChange::replace(&s.engine.document().artwork.occurrences, layer, Some(occurrence)).unwrap())).unwrap();
    let unlinked = origin(&s);
    s.layer_action(LayerAction::MoveLayer { id: occurrence_token(layer) }).unwrap();
    layer_drag(&mut s, 10, [150., 150.], [145., 157.]);
    assert_eq!(offset(&s, layer), [-5, 7]);
    assert_eq!(origin(&s), unlinked, "an unlinked mask stays");

    let edit = s.engine.document().group_layers_edit(&[layer], layer_core::LayerBlend::Normal, "Group").unwrap();
    s.engine.apply_edit(edit).unwrap();
    let group = s.engine.document().scene().parent(layer).unwrap();
    s.layer_action(LayerAction::MoveLayer { id: occurrence_token(group) }).unwrap();
    layer_drag(&mut s, 20, [150., 150.], [130., 120.]);
    assert_eq!(offset(&s, group), [-20, -30]);
    assert_eq!(offset(&s, layer), [-5, 7]);
    assert_eq!(origin(&s), [unlinked[0] - 20, unlinked[1] - 30], "an unlinked mask follows its containing group");
    assert_eq!(affine(&s, object), translate(100., 100.));
}

fn surface_pen(s: &mut UiSession<Recorder>, sequence: u64, phase: PenPhase, at: [f32; 2]) {
    let mut e = event(s, sequence, phase, 1.);
    e.surface_position = Point { x: at[0], y: at[1] };
    s.pen(e).unwrap();
    s.frame(sequence, sequence).unwrap();
}

#[test]
fn far_drags_and_snaps_use_camera_relative_binary64_input() {
    let mut s = session();
    let layer = object_layer(&mut s, "Far", None);
    let moving = add(&mut s, layer, "Moving", translate(1e7, 1e7), 0);
    let other = object_layer(&mut s, "Edge", None);
    add(&mut s, other, "Edge", translate(1e7 + 200.5, 1e7), 0);
    activate(&mut s, layer);
    s.select_objects(layer, [moving].into()).unwrap();
    let camera = &mut s.state.camera;
    (camera.zoom, camera.rotation, camera.flipped, camera.translation) = (1., 0., [false; 2], [-9_999_500.; 2]);
    camera.revision += 1;
    s.sync_camera();
    surface_pen(&mut s, 1, PenPhase::Down, [520.25, 540.5]);
    surface_pen(&mut s, 2, PenPhase::Move, [530.75, 540.5]);
    surface_pen(&mut s, 3, PenPhase::Up, [530.75, 540.5]);
    assert!((affine(&s, moving).0[4] - (1e7 + 10.5)).abs() < 1e-3, "{:?}", affine(&s, moving));
    s.operation.snapping = true;
    surface_pen(&mut s, 4, PenPhase::Down, [540.25, 540.5]);
    surface_pen(&mut s, 5, PenPhase::Move, [628.25, 540.5]);
    surface_pen(&mut s, 6, PenPhase::Up, [628.25, 540.5]);
    let snapped = affine(&s, moving);
    assert!((snapped.0[4] - (1e7 + 100.5)).abs() < 1e-3 && (snapped.0[5] - 1e7).abs() < 1e-3, "the right edge meets the far edge exactly: {snapped:?}");
}

#[test]
fn scaling_through_a_flip_never_publishes_a_singular_pose() {
    let (mut s, layer, [front, _]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    pen(&mut s, PenPhase::Down, [260., 150.]);
    for x in [210., 160., 160.000_01, 159.999_99, 110.] {
        pen(&mut s, PenPhase::Move, [x, 150.]);
        let [a, b, c, d, ..] = affine(&s, front).0;
        assert!((a * d - b * c).abs() >= 0.01 - 1e-12, "{x}: {:?}", affine(&s, front));
    }
    pen(&mut s, PenPhase::Move, [60., 150.]);
    pen(&mut s, PenPhase::Up, [60., 150.]);
    near(affine(&s, front), Affine64([-1., 0., 0., 1., 160., 100.]));
}

#[test]
fn reset_restores_the_starting_affines_of_an_ordinary_object_session() {
    let (mut s, layer, [front, back]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    s.refresh_document();
    assert!(!s.command(CommandId::ResetTransform).enabled);
    drag(&mut s, [230., 180.], [250., 180.]);
    drag(&mut s, [250., 180.], [260., 190.]);
    assert!(s.command(CommandId::ResetTransform).enabled);
    invoke(&mut s, CommandId::ResetTransform);
    assert_eq!(affine(&s, front), translate(160., 100.));
    assert!(!s.command(CommandId::ResetTransform).enabled);
    invoke(&mut s, CommandId::Undo);
    near(affine(&s, front), translate(190., 110.));
    s.select_objects(layer, [back].into()).unwrap();
    s.refresh_document();
    assert!(!s.command(CommandId::ResetTransform).enabled, "a new selection starts a new session");
}

#[test]
fn shift_scales_from_a_handle_and_adds_only_from_the_active_layer() {
    let (mut s, layer, [front, back]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    s.interaction.modifiers = Modifiers { shift: true, ..Default::default() };
    drag(&mut s, [260., 200.], [310., 220.]);
    near(affine(&s, front), Affine64([1.5, 0., 0., 1.5, 160., 100.]));
    assert_eq!(selected(&s), [front].into(), "a handle wins over additive selection");
    invoke(&mut s, CommandId::Undo);
    let cover = object_layer(&mut s, "Cover", None);
    add(&mut s, cover, "Cover", translate(100., 100.), 0);
    activate(&mut s, layer);
    s.select_objects(layer, [front].into()).unwrap();
    s.interaction.modifiers = Modifiers { shift: true, ..Default::default() };
    click(&mut s, [120., 180.]);
    assert_eq!((s.engine.document().working.occurrence, selected(&s)), (Some(layer), BTreeSet::from([front, back])), "Shift picks inside the active layer");
    click(&mut s, [600., 600.]);
    assert_eq!(selected(&s), [front, back].into(), "a Shift miss keeps the selection");
    let hidden = object_layer(&mut s, "Elsewhere", None);
    add(&mut s, hidden, "Elsewhere", translate(600., 600.), 0);
    activate(&mut s, layer);
    s.select_objects(layer, [front].into()).unwrap();
    s.interaction.modifiers = Modifiers { shift: true, ..Default::default() };
    click(&mut s, [650., 650.]);
    assert_eq!((s.engine.document().working.occurrence, selected(&s)), (Some(layer), BTreeSet::from([front])));
    assert_eq!(s.state.notice.as_ref().map(|n| n.text.clone()), Some(s.localization().text(MessageId::OBJECTS_SHIFT_SAME_LAYER).to_string()));
}

#[test]
fn transform_again_follows_the_history_of_its_own_drawing() {
    let (mut s, layer, [front, back]) = fixture();
    s.select_objects(layer, [front].into()).unwrap();
    drag(&mut s, [230., 180.], [250., 180.]);
    s.select_objects(layer, [back].into()).unwrap();
    assert!(s.command(CommandId::TransformAgain).enabled, "selecting other images keeps the last transform");
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::Undo);
    s.select_objects(layer, [back].into()).unwrap();
    assert!(!s.command(CommandId::TransformAgain).enabled, "an undone transform cannot be repeated");
    assert!(s.dispatch(UiAction::Invoke { command: CommandId::TransformAgain }).is_err());
    let (_, checkpoint, delta) = s.objects.last.unwrap();
    s.objects.last = Some((s.engine.document().owner.wrapping_add(1), checkpoint, delta));
    assert!(s.last_transform().is_none(), "another drawing's transform is never reapplied");
}

#[test]
fn delete_or_deselect_with_no_images_selected_does_nothing_and_copy_pixels_copies_the_layer_as_pixels() {
    let (mut s, layer, [front, _]) = fixture();
    let before = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    assert!(key(&mut s, "Delete", true, false, false).handled);
    key(&mut s, "Delete", false, false, false);
    s.object_action(ObjectAction::Delete).unwrap();
    s.object_action(ObjectAction::Deselect).unwrap();
    assert_eq!(s.engine.document(), &before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert!(s.state.notice.is_none(), "an empty image selection deletes nothing and says nothing");
    assert!(s.command(CommandId::CopyPixels).enabled, "an image layer can always be copied as pixels");
    assert_eq!(s.command(CommandId::CopyPixels).label.as_ref(), "Copy Pixels");
    invoke(&mut s, CommandId::CopyPixels);
    let (id, request) = s.state.requests.iter().find_map(|r| match &r.kind {
        HostRequestKind::Document { request } => Some((r.id, request.clone())),
        _ => None,
    }).unwrap();
    assert!(matches!(request, DocumentRequest::Copy { merged: false, cut: false, pixels: true }));
    let capture = s.capture_clipboard(id).unwrap();
    assert_eq!((capture.scope, capture.crop, capture.window, capture.objects.is_none()), (SceneScope::RawObjects(layer), [0, 0, 1000, 1000], None, true));
    s.complete_document_request(id, Ok(true)).unwrap();
    s.select_objects(layer, [front].into()).unwrap();
    invoke(&mut s, CommandId::Copy);
    let capture = s.capture_clipboard(pending_copy(&s)).unwrap();
    assert!(capture.objects.is_some(), "Copy keeps the structured image flavour");
}

#[test]
fn object_snapping_excludes_the_moving_layer_and_its_groups() {
    let mut s = session();
    let layer = object_layer(&mut s, "Moving", None);
    let moving = add(&mut s, layer, "Moving", translate(10., 10.), 0);
    let edit = s.engine.document().group_layers_edit(&[layer], layer_core::LayerBlend::Normal, "Group").unwrap();
    s.engine.apply_edit(edit).unwrap();
    let group = s.engine.document().scene().parent(layer).unwrap();
    let sibling = object_layer(&mut s, "Sibling", None);
    add(&mut s, sibling, "Sibling", translate(300., 300.), 0);
    let paint = *s.engine.document().scene().order().iter().find(|h| s.engine.document().scene().paint_source(**h).is_some()).unwrap();
    s.layer_action(LayerAction::Select { id: occurrence_token(paint), mask: false }).unwrap();
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Move }).unwrap();
    s.operation.snapping = true;
    for tick in 1..10 {
        s.prepare_transform_snapping().unwrap();
        if !s.content_bounds.busy() { break; }
        s.engine.backend_mut().bounds_reply = Some(Ok(Rect::from_extent([200, 200])));
        s.frame(tick, tick).unwrap();
    }
    assert!(s.measured_snap_bounds().iter().any(|(h, _)| *h == group), "the group is a candidate while its images are not moving");
    activate(&mut s, layer);
    s.select_objects(layer, [moving].into()).unwrap();
    let mut working = s.engine.document().working.clone();
    working.layer_selection = [sibling].into();
    s.layer_edit(Edit::Working(working)).unwrap();
    let targets: Vec<_> = s.object_snap_targets().into_iter().map(|(h, _)| h).collect();
    assert!(!targets.contains(&Some(group)) && !targets.contains(&Some(layer)), "{targets:?}");
    assert!(targets.contains(&Some(sibling)));
}

#[test]
fn image_help_describes_pasting_and_copying_images() {
    let l = Localizer::shared(UiLanguage::English);
    assert!(l.text(MessageId::COMMANDS_HELP_PASTE_INTO).contains("image layer"));
    assert!(l.text(MessageId::COMMANDS_HELP_PASTE_IN_PLACE).contains("original size"));
    assert!(l.text(MessageId::COMMANDS_HELP_COPY).contains("selected images"));
    assert!(l.text(MessageId::COMMANDS_HELP_CUT).contains("selected images"));
    assert!(l.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_IMPORT_IMAGE).contains("move, scale and rotate"));
    assert!(!l.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PASTE_INTO).contains("new layer"));
}
