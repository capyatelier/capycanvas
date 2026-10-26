fn vector_session() -> (UiSession<Recorder>, OccurrenceHandle) { vector_session_on(Platform::Gtk) }
fn vector_session_on(platform: Platform) -> (UiSession<Recorder>, OccurrenceHandle) {
    let mut s = session(platform);
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    layer(&mut s, LayerAction::NewVector);
    let layer = s.engine.document().working.occurrence.unwrap();
    assert!(s.engine.document().scene().vector_layer(layer).is_some());
    (s, layer)
}
fn saved_settings(s: &UiSession<Recorder>) -> Option<crate::Settings> {
    s.state.requests.iter().rev().find_map(|r| match &r.kind { HostRequestKind::SaveSettings { settings } => Some((**settings).clone()), _ => None })
}
fn point_edit_session(platform: Platform) -> (UiSession<Recorder>, OccurrenceHandle) {
    use layer_core::authored::vector::*;
    let (mut s, layer) = vector_session_on(platform);
    let paths = [300., 700.].into_iter().enumerate().map(|(i, y)| {
        let geometry = PathGeometry::new(vec![Subpath::new(&[
            Anchor::new([100., y], [0.; 2], [100., -100.], AnchorKind::CORNER),
            Anchor::new([500., y], [-100., -100.], [0.; 2], AnchorKind::CORNER),
        ], false)]).unwrap();
        Record::Path(PathItem::new(ItemId(i as u64 + 1), Arc::new(geometry), Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(Paint::color(layer_core::color::RgbColor::BLACK), 4.))]))))
    }).collect();
    let edit = s.engine.document().vector_object_edit(layer, VectorObject::from_records(paths)).unwrap();
    s.layer_edit(edit).unwrap();
    s.select_vector_items(layer, [ItemId(1)].into()).unwrap();
    (s, layer)
}

#[test]
fn vector_point_handles_belong_to_the_selected_paths_and_current_layer() {
    use layer_core::authored::vector::ItemId;
    for platform in Platform::ALL {
        let (mut s, layer) = point_edit_session(platform);
        invoke(&mut s, CommandId::EditPoints);
        let click = surface_at(&s, [100., 300.]);
        press(&mut s, 100, &[click, click]);
        invoke(&mut s, CommandId::Move);
        s.select_vector_items(layer, [ItemId(2)].into()).unwrap();
        invoke(&mut s, CommandId::EditPoints);
        let before = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let points = [[200., 200.], [225., 175.], [250., 150.]].map(|p| surface_at(&s, p));
        press(&mut s, 300, &points);
        assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &before, "{platform:?}: invisible handles cannot edit a deselected path");
        s.dispatch(UiAction::Layer { action: LayerAction::NewVector }).unwrap();
        let other = s.engine.document().working.occurrence.unwrap();
        let edit = s.engine.document().vector_object_edit(other, before.clone()).unwrap();
        s.layer_edit(edit).unwrap();
        let points = [[200., 200.], [225., 175.], [250., 150.]].map(|p| surface_at(&s, p));
        press(&mut s, 500, &points);
        assert_eq!(s.engine.document().scene().vector_layer(other).unwrap(), &before, "{platform:?}: local path IDs do not carry anchor selection between layers");
    }
}

#[test]
fn vector_point_undo_does_not_retarget_delete_and_unchanged_drags_add_no_history() {
    for platform in Platform::ALL {
        let (mut s, layer) = point_edit_session(platform);
        invoke(&mut s, CommandId::EditPoints);
        let before = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let checkpoint = s.engine.checkpoint();
        let points = [[100., 300.], [150., 350.], [100., 300.]].map(|p| surface_at(&s, p));
        press(&mut s, 100, &points);
        assert_eq!(s.engine.checkpoint(), checkpoint, "{platform:?}: returning an anchor adds no edit");
        let on_curve = surface_at(&s, [300., 225.]);
        press(&mut s, 200, &[on_curve, on_curve]);
        invoke(&mut s, CommandId::Undo);
        invoke(&mut s, CommandId::ClearSelected);
        assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &before, "{platform:?}: Undo cannot transfer selected anchor indices to another anchor");
    }
}

#[test]
fn vector_point_drags_back_preserve_live_shapes_and_smooth_segments() {
    use layer_core::authored::vector::*;
    for shape in [ShapeKind::Rectangle { radii: [20.; 4] }, ShapeKind::Ellipse] {
        let (mut s, layer) = point_edit_session(Platform::Gtk);
        let object = s.engine.document().scene().vector_layer(layer).unwrap();
        let (position, path) = object.paths().next().unwrap();
        let path = path.with_shape(LiveShape { kind: shape, center: [300., 300.], size: [200., 160.], rotation: 0. }).unwrap();
        let edit = s.engine.document().vector_object_edit(layer, object.replace([(position, Some(Record::Path(path)))])).unwrap();
        s.layer_edit(edit).unwrap();
        invoke(&mut s, CommandId::EditPoints);
        let before = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let subpath = &before.paths().next().unwrap().1.geometry.subpaths()[0];
        for at in [subpath.anchor(0).point(), subpath.segment(0).point(0.5)] {
            let checkpoint = s.engine.checkpoint();
            let points = [at, [at[0] + 50., at[1] + 50.], at].map(|p| surface_at(&s, p));
            press(&mut s, 100, &points);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &before, "a returned drag preserves the live shape and anchor kinds");
        }
    }
}

#[test]
fn vector_move_double_clicks_require_the_same_layer_and_location() {
    for platform in Platform::ALL {
        let (mut s, layer) = point_edit_session(platform);
        invoke(&mut s, CommandId::Move);
        let first = surface_at(&s, [200., 243.]);
        press(&mut s, 100, &[first, first]);
        let far = surface_at(&s, [400., 243.]);
        press(&mut s, 120, &[far, far]);
        assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move, "{platform:?}: distant clicks are not double-clicks");
        let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        s.dispatch(UiAction::Layer { action: LayerAction::NewVector }).unwrap();
        let other = s.engine.document().working.occurrence.unwrap();
        let edit = s.engine.document().vector_object_edit(other, object).unwrap();
        s.layer_edit(edit).unwrap();
        invoke(&mut s, CommandId::Move);
        press(&mut s, 130, &[far, far]);
        assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move, "{platform:?}: clicks on different layers are separate");
    }
}

#[test]
fn vector_transform_handles_leave_history_alone_when_returned() {
    let (mut s, layer) = point_edit_session(Platform::Gtk);
    invoke(&mut s, CommandId::Move);
    let before = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let checkpoint = s.engine.checkpoint();
    let frame = s.vector_frame().unwrap();
    let at = frame.unit.map([1., 1.]);
    let points = [at, [at[0] + 50., at[1] + 50.], at].map(|p| surface_at(&s, p));
    press(&mut s, 100, &points);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &before);
}

#[test]
fn rapid_vector_drags_remain_drags_on_every_platform() {
    for platform in Platform::ALL {
        for tool in [CommandId::Move, CommandId::EditPoints] {
            let (mut s, layer) = point_edit_session(platform);
            invoke(&mut s, tool);
            let start = if tool == CommandId::Move { [190.625, 243.75] } else { [100., 300.] };
            for (first, delta) in [(100, 0.), (120, 40.)] {
                let points = [[start[0] + delta, start[1] + delta], [start[0] + delta + 20., start[1] + delta + 20.], [start[0] + delta + 40., start[1] + delta + 40.]].map(|p| surface_at(&s, p));
                press(&mut s, first, &points);
            }
            let object = s.engine.document().scene().vector_layer(layer).unwrap();
            let anchor = object.paths().find(|(_, p)| p.id.0 == 1).unwrap().1.geometry.subpaths()[0].anchor(0);
            assert_eq!(anchor.point(), [180., 380.], "{platform:?} {tool:?}: both drags move");
            assert_eq!(anchor.out_handle(), [100., -100.], "{platform:?} {tool:?}: a drag does not retract handles");
        }
    }
}

#[test]
fn vector_path_handles_follow_the_release_back_to_the_anchor() {
    for platform in Platform::ALL {
        let (mut s, layer) = vector_session_on(platform);
        s.object_tool(LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline }).unwrap();
        let points = [[100., 100.], [200., 100.], [100., 100.]].map(|p| surface_at(&s, p));
        press(&mut s, 100, &points);
        let end = surface_at(&s, [100., 400.]);
        press(&mut s, 200, &[end, end]);
        invoke(&mut s, CommandId::FinishPath);
        let object = s.engine.document().scene().vector_layer(layer).unwrap();
        let first = object.paths().next().unwrap().1.geometry.subpaths()[0].anchor(0);
        assert_eq!((first.in_handle(), first.out_handle()), ([0.; 2], [0.; 2]), "{platform:?}: release at the anchor retracts handles");
    }
}
fn draw_line(s: &mut UiSession<Recorder>, first: u64, samples: u64) {
    for i in 0..samples {
        let phase = if i == 0 { PenPhase::Down } else if i + 1 == samples { PenPhase::Up } else { PenPhase::Move };
        let mut e = event(s, first + i, phase, 0.3 + 0.6 * (i as f32 / samples as f32 * 3.1).sin());
        e.surface_position.y += (i as f32 * 0.15).sin() * 60.;
        e.surface_position.x = 150. + i as f32 * 6.;
        s.pen(e);
        s.frame((first + i) * 10_000_000, (first + i) * 10_000_000).unwrap();
    }
}

#[test]
fn vector_layers_draw_fitted_lines_with_one_undo_step() {
    let (mut s, layer) = vector_session();
    let revision = s.engine.document().revision;
    draw_line(&mut s, 1, 80);
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    assert_eq!(object.path_count(), 1, "a pen stroke becomes one path");
    let (_, path) = object.paths().next().unwrap();
    let anchors = path.geometry.anchor_count();
    let stroke = path.style.strokes().next().unwrap();
    assert!((2..=8).contains(&anchors), "fitting keeps few anchors: {anchors}");
    assert!(stroke.profile.is_some(), "pressure becomes a width profile");
    assert_ne!(s.engine.document().revision, revision);
    assert_eq!(s.state.notice, None);
    draw_line(&mut s, 200, 40);
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 2);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 1, "undo removes the last line only");
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    s.frame(5_100_000_000, 5_100_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 2);
}

#[test]
fn vector_lines_land_under_the_pen_on_moved_layers() {
    use layer_core::{Edit, authored::RecordChange};
    let (mut s, layer) = vector_session();
    let document = s.engine.document();
    let mut occurrence = document.scene().occurrence(layer).unwrap().clone();
    occurrence.offset = [40, -25];
    let edit = Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, layer, Some(occurrence)).unwrap());
    s.engine.apply_edit(edit).unwrap();
    let points: Vec<Point> = (0..=12).map(|i| surface_at(&s, [300. + f64::from(i) * 10., 200. + f64::from(i) * 5.])).collect();
    press(&mut s, 1, &points);
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let (_, path) = object.paths().next().unwrap();
    let [ox, oy] = s.engine.document().scene().occurrence_offset64(layer);
    let anchors = path.geometry.subpaths()[0].to_vec();
    let [start, end] = [anchors[0].point(), anchors[anchors.len() - 1].point()].map(|[x, y]| [x + ox, y + oy]);
    assert!((start[0] - 300.).hypot(start[1] - 200.) < 1.5, "{start:?}");
    assert!((end[0] - 420.).hypot(end[1] - 260.) < 1.5, "{end:?}");
}

#[test]
fn late_pressure_corrections_reshape_the_drawn_line_in_its_undo_step() {
    let (mut s, layer) = vector_session();
    let estimated = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
    let events: Vec<PenEvent> = (0..40u64).map(|i| {
        let phase = if i == 0 { PenPhase::Down } else if i == 39 { PenPhase::Up } else { PenPhase::Move };
        let mut e = event(&s, i + 1, phase, 0.15);
        e.surface_position.x = 150. + i as f32 * 6.;
        e.flags = estimated;
        e
    }).collect();
    for (i, e) in events.iter().enumerate() {
        s.pen(*e);
        s.frame((i as u64 + 1) * 10_000_000, (i as u64 + 1) * 10_000_000).unwrap();
    }
    let widest = |s: &UiSession<Recorder>| {
        let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        assert_eq!(object.path_count(), 1);
        let (_, path) = object.paths().next().unwrap();
        path.style.strokes().next().unwrap().width
    };
    let before = widest(&s);
    for (i, e) in events.iter().enumerate() {
        s.pen(PenEvent { pressure: 0.9, flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::CORRECTION.0), ..*e });
        s.frame((50 + i as u64) * 10_000_000, (50 + i as u64) * 10_000_000).unwrap();
    }
    assert!(widest(&s) > before * 2., "{} vs {before}", widest(&s));
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 0, "the line and its corrections are one step");
}

#[test]
fn vector_line_previews_reach_predicted_samples_without_drawing_them() {
    let (mut s, layer) = vector_session_on(Platform::Android);
    s.set_platform_prediction_available(true);
    let farthest = |path: &layer_core::authored::vector::PathItem, axis: usize, sign: f64| path.geometry.subpaths()[0].to_vec().iter().map(|anchor| anchor.point()[axis] * sign).fold(f64::MIN, f64::max) * sign;
    let document = |s: &UiSession<Recorder>, point: Point| { let p = s.state.camera.input_transform().map(point); [f64::from(p.x), f64::from(p.y)] };
    let (mut real, mut predicted_reach) = ([0.; 2], [0.; 2]);
    for i in 0..30u64 {
        let mut e = event(&s, i + 1, if i == 0 { PenPhase::Down } else { PenPhase::Move }, 0.6);
        e.surface_position.x = 150. + i as f32 * 6.;
        s.pen(e);
        let mut predicted = PenEvent { sequence: e.sequence + 1_000, timestamp_ns: e.timestamp_ns + 10_000_000, flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0), ..e };
        predicted.surface_position.x += 6.;
        predicted.surface_position.y -= if i >= 20 { 6. } else { 0. };
        if i > 0 { s.pen(predicted); }
        s.frame(e.timestamp_ns, predicted.timestamp_ns).unwrap();
        real = document(&s, e.surface_position);
        predicted_reach = document(&s, predicted.surface_position);
    }
    let preview = &s.engine.backend().vector_preview.as_ref().expect("the line is previewed").path;
    let shown = [farthest(preview, 0, 1.), farthest(preview, 1, -1.)];
    assert!(shown[0] > real[0] + (predicted_reach[0] - real[0]) * 0.5, "the preview reaches ahead: {shown:?} vs {real:?}..{predicted_reach:?}");
    assert!(shown[1] < real[1] + (predicted_reach[1] - real[1]) * 0.5, "the preview turns with the platform prediction: {shown:?} vs {real:?}..{predicted_reach:?}");
    let up = PenEvent { phase: PenPhase::Up, ..event(&s, 31, PenPhase::Up, 0.6) };
    s.pen(PenEvent { surface_position: Point { x: 150. + 29. * 6., y: 300. }, ..up });
    s.frame(up.timestamp_ns, up.timestamp_ns).unwrap();
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let (_, path) = object.paths().next().unwrap();
    let drawn = [farthest(path, 0, 1.), farthest(path, 1, -1.)];
    assert!((drawn[0] - real[0]).abs() < 1. && (drawn[1] - real[1]).abs() < 0.5, "the drawn line ends at the pen: {drawn:?} vs {real:?}");
    assert!(s.engine.backend().vector_preview.is_none());
}

fn surface_at(s: &UiSession<Recorder>, doc: [f64; 2]) -> Point {
    let [a, b, c, d, x, y] = s.state.camera.view().document_to_surface;
    let [px, py] = doc.map(|v| v as f32);
    Point { x: a * px + c * py + x, y: b * px + d * py + y }
}
fn press(s: &mut UiSession<Recorder>, first: u64, points: &[Point]) {
    for (i, point) in points.iter().enumerate() {
        let phase = if i == 0 { PenPhase::Down } else if i + 1 == points.len() { PenPhase::Up } else { PenPhase::Move };
        let mut e = event(s, first + i as u64, phase, 0.5);
        e.surface_position = *point;
        s.pen(e);
        s.frame((first + i as u64) * 10_000_000, (first + i as u64) * 10_000_000).unwrap();
    }
}

#[test]
fn the_eraser_cuts_or_removes_vector_paths_in_one_undo_step() {
    let (mut s, layer) = vector_session();
    let prohibited = |s: &mut UiSession<Recorder>| { s.cursor_input(Some(event(s, 1, PenPhase::Hover, 0.))); s.canvas_cursor().is_some_and(|c| c.segments.iter().any(|segment| segment.marker == 6.)) };
    assert!(!prohibited(&mut s), "G-Pen draws on a vector layer");
    let revision = |s: &UiSession<Recorder>| s.state.layers.iter().find(|row| row.id == occurrence_token(layer)).unwrap().paint_revision;
    let before = revision(&s);
    draw_line(&mut s, 1, 30);
    assert_ne!(revision(&s), before, "a new path refreshes the layer thumbnail");
    let drawn = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let subpath = &drawn.paths().next().unwrap().1.geometry.subpaths()[0];
    let middle = subpath.segment(subpath.segment_count() / 2).point(0.5);
    invoke(&mut s, CommandId::Eraser);
    s.dispatch(UiAction::SetBrushSize { value: 8. }).unwrap();
    assert!(!prohibited(&mut s), "the eraser works on vector layers");
    let modes: Vec<_> = s.state.tool_panels.tools.subtools.iter().take(3).map(|item| (item.action.clone(), item.selected)).collect();
    assert_eq!(modes, [CommandId::EraseTouchedPart, CommandId::EraseWholePaths, CommandId::EraseToIntersection].map(|command| (UiAction::Invoke { command }, command == CommandId::EraseTouchedPart)), "the eraser modes lead the tool panel");
    let across = |s: &UiSession<Recorder>| [-40., 0., 40.].map(|dy| surface_at(s, [middle[0], middle[1] + dy]));
    let points = across(&s);
    press(&mut s, 100, &points);
    let cut = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    assert_eq!(cut.path_count(), 2, "Touched Part splits the line where the eraser crosses it");
    assert_eq!(s.state.notice, None);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &drawn, "one undo step restores the line");
    invoke(&mut s, CommandId::EraseWholePaths);
    assert_eq!(s.state.settings.vector_eraser, crate::settings::VectorEraser::WholePaths);
    let points = across(&s);
    press(&mut s, 200, &points);
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 0, "Whole Path removes the touched line");
}

#[test]
fn move_tool_selects_drags_and_deletes_vector_paths() {
    let (mut s, layer) = vector_session();
    draw_line(&mut s, 1, 40);
    draw_line(&mut s, 200, 40);
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let (front, back) = { let mut paths = object.paths().map(|(_, p)| p.clone()); (paths.next().unwrap(), paths.next().unwrap()) };
    invoke(&mut s, CommandId::Move);
    let on_front = front.geometry.subpaths()[0].anchor(0).point();
    let start = surface_at(&s, on_front);
    let revision = s.engine.document().revision;
    press(&mut s, 400, &[start, Point { x: start.x + 20., y: start.y }, Point { x: start.x + 40., y: start.y + 10. }]);
    let selected = s.engine.document().working.vector_items.clone().expect("pressing a path selects it");
    assert_eq!(*selected.items, [front.id].into());
    let moved = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let delta = |id: layer_core::authored::vector::ItemId| { let p = moved.record(moved.find(id).unwrap()).unwrap().path().unwrap().geometry.subpaths()[0].anchor(0).point(); let q = if id == front.id { on_front } else { back.geometry.subpaths()[0].anchor(0).point() }; [p[0] - q[0], p[1] - q[1]] };
    let scale = f64::from(s.state.camera.input_transform().surface_to_document[0]);
    let grid = 0.5 / layer_core::authored::vector::GRID;
    assert!((delta(front.id)[0] - 40. * scale).abs() <= grid && (delta(front.id)[1] - 10. * scale).abs() <= grid, "{:?}", delta(front.id));
    assert_eq!(delta(back.id), [0., 0.], "other paths stay");
    assert_eq!(moved.path_count(), 2);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(6_000_000_000, 6_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &object, "one undo step restores the move");
    assert!(s.engine.document().revision > revision);
    let empty = surface_at(&s, [3000., 2000.]);
    press(&mut s, 500, &[empty, empty]);
    assert!(s.engine.document().working.vector_items.is_none(), "a click on empty canvas clears the selection");
    let [x0, y0, x1, y1] = back.bounds();
    let corners = [surface_at(&s, [x0 - 5., y0 - 5.]), surface_at(&s, [x1 + 5., y1 + 5.])];
    press(&mut s, 600, &corners);
    let marquee = s.engine.document().working.vector_items.clone().expect("a marquee selects enclosed paths");
    assert!(marquee.items.contains(&back.id));
    invoke(&mut s, CommandId::ClearSelected);
    let left = s.engine.document().scene().vector_layer(layer).unwrap();
    assert!(left.find(back.id).is_none() && left.path_count() == 2 - marquee.items.len());
    assert!(s.engine.document().working.vector_items.is_none());
}

#[test]
fn arrange_commands_restack_selected_paths_one_step_at_a_time() {
    let (mut s, layer) = vector_session();
    for first in [1, 100, 200] { draw_line(&mut s, first, 30); }
    let order = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().paths().map(|(_, p)| p.id).collect::<Vec<_>>();
    let drawn = order(&s);
    invoke(&mut s, CommandId::Move);
    assert!(!s.command(CommandId::BringToFront).enabled, "arranging needs selected paths");
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let at = surface_at(&s, object.paths().next().unwrap().1.geometry.subpaths()[0].anchor(0).point());
    press(&mut s, 300, &[at, at]);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|selection| (*selection.items).clone()), Some([drawn[0]].into()), "a click picks the front path");
    assert!(s.command(CommandId::BringToFront).enabled);
    assert!(s.state.tool_actions.iter().any(|action| action.command == CommandId::SendToBack), "the Move tool offers arranging");
    invoke(&mut s, CommandId::SendBackward);
    assert_eq!(order(&s), [drawn[1], drawn[0], drawn[2]]);
    invoke(&mut s, CommandId::SendToBack);
    assert_eq!(order(&s), [drawn[1], drawn[2], drawn[0]]);
    invoke(&mut s, CommandId::BringToFront);
    assert_eq!(order(&s), drawn);
    invoke(&mut s, CommandId::BringForward);
    assert_eq!(order(&s), drawn, "the front path stays in front");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(order(&s), [drawn[1], drawn[2], drawn[0]], "each arrange is one undo step");
}

#[test]
fn figure_shapes_on_vector_layers_become_selected_editable_paths() {
    use layer_core::authored::vector::{Cap, Join, PaintEntry, ShapeKind};
    let (mut s, layer) = vector_session();
    let prohibited = |s: &mut UiSession<Recorder>| { s.cursor_input(Some(event(s, 1, PenPhase::Hover, 0.))); s.canvas_cursor().is_some_and(|c| c.segments.iter().any(|segment| segment.marker == 6.)) };
    let drag = |s: &mut UiSession<Recorder>, first: u64, from: [f64; 2], to: [f64; 2]| { let points = [surface_at(s, from), surface_at(s, [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5]), surface_at(s, to)]; press(s, first, &points); };
    let front = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().paths().next().map(|(_, p)| p.clone());
    for (shape, paint, first) in [(FigureShape::Rectangle, FigurePaint::Both, 100), (FigureShape::Ellipse, FigurePaint::Fill, 200), (FigureShape::Line, FigurePaint::Outline, 300), (FigureShape::Polygon, FigurePaint::Outline, 400), (FigureShape::Star, FigurePaint::Fill, 500)] {
        s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape, paint } } }).unwrap();
        assert!(!prohibited(&mut s), "{shape:?} works on a vector layer");
        drag(&mut s, first, [100., 120.], [260., 200.]);
        let path = front(&s).expect("the figure becomes a path");
        assert_eq!(s.engine.document().working.vector_items.as_ref().map(|selection| (*selection.items).clone()), Some([path.id].into()), "the new path is selected");
        let [x0, y0, x1, y1] = path.geometry.hull();
        let spans = (x0 - 100.).abs() < 1e-3 && (x1 - 260.).abs() < 1e-3 && (y1 - 200.).abs() < 1e-3 || matches!(shape, FigureShape::Polygon | FigureShape::Star) && x0 >= 100. && x1 <= 260. && y1 <= 200.;
        assert!((y0 - 120.).abs() < 1e-3 && spans, "{shape:?} spans the drag: {:?}", path.geometry.hull());
        match shape {
            FigureShape::Rectangle => {
                assert!(matches!(path.shape.as_deref(), Some(layer_core::authored::vector::LiveShape { kind: ShapeKind::Rectangle { .. }, .. })));
                assert!(matches!(path.style.entries[..], [PaintEntry::Fill(_), PaintEntry::Stroke(ref s)] if s.join == Join::Miter), "Both fills below the outline");
            }
            FigureShape::Ellipse => {
                assert!(matches!(path.shape.as_deref(), Some(layer_core::authored::vector::LiveShape { kind: ShapeKind::Ellipse, .. })));
                assert!(matches!(path.style.entries[..], [PaintEntry::Fill(_)]));
            }
            FigureShape::Polygon => assert!(matches!(path.shape.as_deref(), Some(layer_core::authored::vector::LiveShape { kind: ShapeKind::Polygon { corners: 5 }, .. })) && path.geometry.anchor_count() == 5),
            FigureShape::Star => assert!(matches!(path.shape.as_deref(), Some(layer_core::authored::vector::LiveShape { kind: ShapeKind::Star { points: 5, .. }, .. })) && path.geometry.anchor_count() == 10),
            FigureShape::Line | FigureShape::Path => assert!(path.shape.is_none() && matches!(path.style.entries[..], [PaintEntry::Stroke(ref s)] if s.cap == Cap::Round)),
        }
    }
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 5);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(9_000_000_000, 9_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap().path_count(), 4, "each figure is one undo step");
}

#[test]
fn move_tool_nudges_duplicates_and_copies_paths() {
    let (mut s, layer) = vector_session();
    draw_line(&mut s, 1, 30);
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let start = |s: &UiSession<Recorder>, id| { let o = object(s); o.record(o.find(id).unwrap()).unwrap().path().unwrap().geometry.subpaths()[0].anchor(0).point() };
    let id = object(&s).paths().next().unwrap().1.id;
    let origin = start(&s, id);
    invoke(&mut s, CommandId::Move);
    let at = surface_at(&s, origin);
    press(&mut s, 100, &[at, at]);
    let nudge = |s: &mut UiSession<Recorder>, shift: bool| for pressed in [true, false] {
        s.input(UiInput::Key { key: "arrowright".into(), pressed, repeat: false, modifiers: Modifiers { shift, ..Modifiers::default() }, editing: false, divider: None }).unwrap();
    };
    nudge(&mut s, false);
    nudge(&mut s, true);
    assert_eq!(start(&s, id), [origin[0] + 11., origin[1]], "arrows nudge by one pixel, ten with Shift");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(start(&s, id), [origin[0] + 1., origin[1]], "each nudge is one undo step");

    let middle = |s: &UiSession<Recorder>, id| { let o = object(s); let subpath = &o.record(o.find(id).unwrap()).unwrap().path().unwrap().geometry.subpaths()[0]; subpath.segment(subpath.segment_count() / 2).point(0.5) };
    let at = surface_at(&s, middle(&s, id));
    s.interaction.modifiers = Modifiers { alt: true, ..Modifiers::default() };
    press(&mut s, 200, &[at, Point { x: at.x + 10., y: at.y }, Point { x: at.x + 30., y: at.y + 20. }]);
    s.interaction.modifiers = Modifiers::default();
    let copied = object(&s);
    assert_eq!(copied.path_count(), 2, "Alt-drag leaves the original and drags a copy");
    assert_eq!(start(&s, id), [origin[0] + 1., origin[1]], "the original stays");
    let copy = *s.engine.document().working.vector_items.as_ref().unwrap().items.iter().next().unwrap();
    assert_ne!(copy, id, "the copy is selected");
    assert_eq!(copied.paths().next().unwrap().1.id, copy, "the copy is in front");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    assert_eq!(object(&s).path_count(), 1, "one undo removes the dragged copy");

    let at = surface_at(&s, middle(&s, id));
    press(&mut s, 300, &[at, at]);
    let request = |s: &UiSession<Recorder>| s.state.requests.iter().find_map(|r| match &r.kind { HostRequestKind::Document { request } => Some((r.id, request.clone())), _ => None }).unwrap().0;
    let copy_clip = |s: &mut UiSession<Recorder>, command| {
        invoke(s, command);
        let id = request(s);
        let capture = s.capture_clipboard(id).unwrap();
        let extent = [capture.crop[2], capture.crop[3]];
        let clip = capture.finish("paths".into(), layer_core::color::source::rgba8_source(extent, |_, _| [0; 4]), vec![]).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        clip
    };
    let clip = copy_clip(&mut s, CommandId::Copy);
    assert!(clip.layers.as_ref().is_some_and(|layers| layers.paths && layers.scene.view().vector_layer(layers.roots[0]).unwrap().path_count() == 1));
    s.paste_clip(&clip, PasteMode::InPlace).unwrap();
    let pasted = object(&s);
    assert_eq!(pasted.path_count(), 2, "pasting onto a vector layer adds the paths");
    let new = *s.engine.document().working.vector_items.as_ref().unwrap().items.iter().next().unwrap();
    assert_ne!(new, id);
    assert_eq!(start(&s, new), start(&s, id), "Paste in Place keeps the position");
    copy_clip(&mut s, CommandId::Cut);
    assert_eq!(object(&s).path_count(), 1, "Cut removes the selected paths");
    assert!(object(&s).find(new).is_none());
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
    s.paste_clip(&clip, PasteMode::InPlace).unwrap();
    let pasted_layer = s.engine.document().working.occurrence.unwrap();
    assert_ne!(pasted_layer, layer);
    assert_eq!(s.engine.document().scene().vector_layer(pasted_layer).map(|o| o.path_count()), Some(1), "pasting elsewhere makes a vector layer");
}

#[test]
fn properties_restyle_every_selected_path_in_one_undo_step() {
    use layer_core::{EffectValue, authored::vector::{Cap, Join, PaintEntry}};
    let (mut s, layer) = vector_session();
    for (first, size) in [(1, 10.), (100, 30.)] {
        s.dispatch(UiAction::SetBrushSize { value: size }).unwrap();
        draw_line(&mut s, first, 30);
    }
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().clone();
    assert!(s.state.layer_properties.controls.iter().all(|c| !c.key.starts_with("path_")), "path properties need selected paths");
    let label = |s: &UiSession<Recorder>, key: &str| s.state.layer_properties.controls.iter().find(|c| c.key == key).map(|c| c.label.clone()).unwrap();
    let keys_of = |s: &UiSession<Recorder>| s.state.layer_properties.controls.iter().map(|c| c.key.clone()).collect::<Vec<_>>();
    let ids: std::collections::BTreeSet<_> = object(&s).paths().map(|(_, p)| p.id).collect();
    invoke(&mut s, CommandId::Move);
    let corners = { let [x0, y0, x1, y1] = object(&s).bounds().unwrap(); [surface_at(&s, [x0 - 5., y0 - 5.]), surface_at(&s, [x1 + 5., y1 + 5.])] };
    press(&mut s, 50, &corners);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|selection| (*selection.items).clone()), Some(ids.clone()));
    let keys: Vec<&str> = s.state.layer_properties.controls.iter().map(|c| c.key.as_str()).filter(|k| k.starts_with("path_")).collect();
    assert_eq!(keys, ["path_fill", "path_outline", "path_outline_color", "path_width", "path_ends", "path_corners"]);
    assert_eq!((label(&s, "path_width"), label(&s, "path_outline_color")), ("Outline width (mixed)".to_string(), "Outline color".to_string()), "differing values say so");
    let token = occurrence_token(layer);
    let set = |s: &mut UiSession<Recorder>, key: &str, value: EffectValue| s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: token, key: key.into(), value } }).unwrap();
    let red = layer_core::color::RgbColor::new(Default::default(), [0.9, 0.1, 0.1, 1.]).unwrap();
    set(&mut s, "path_outline_color", EffectValue::Color(red));
    set(&mut s, "path_corners", EffectValue::Choice(2));
    set(&mut s, "path_fill", EffectValue::Toggle(true));
    let styled = object(&s);
    for (_, path) in styled.paths() {
        assert!(matches!(path.style.entries[..], [PaintEntry::Fill(_), PaintEntry::Stroke(ref stroke)] if stroke.paint.rgb() == Some(red) && stroke.join == Join::Bevel && stroke.cap == Cap::Round), "{:?}", path.style);
    }
    for (phase, width) in [(ContactPhase::Down, 6.), (ContactPhase::Move, 9.), (ContactPhase::Up, 12.)] {
        s.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Set { layer: token, key: "path_width".into(), value: EffectValue::Number(width) }) } }).unwrap();
    }
    assert!(object(&s).paths().all(|(_, p)| p.style.strokes().last().unwrap().width == 12.), "a width drag sets every selected path");
    assert_eq!(label(&s, "path_width"), "Outline width");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(8_000_000_000, 8_000_000_000).unwrap();
    assert_eq!(object(&s), styled, "a drag is one undo step");
    set(&mut s, "path_overlaps", EffectValue::Choice(1));
    assert!(object(&s).paths().all(|(_, p)| p.fill_rule == layer_core::authored::vector::FillRule::EvenOdd), "Overlaps: Holes cuts overlaps out of every selected fill");
    assert!(!keys_of(&s).contains(&"path_miter_limit".to_string()), "round corners have no limit");
    set(&mut s, "path_corners", EffectValue::Choice(0));
    assert!(keys_of(&s).contains(&"path_miter_limit".to_string()), "sharp corners show their limit");
    set(&mut s, "path_miter_limit", EffectValue::Number(8.));
    assert!(object(&s).paths().all(|(_, p)| p.style.strokes().last().unwrap().miter_limit == 8.));
    set(&mut s, "path_outline", EffectValue::Toggle(false));
    assert!(object(&s).paths().all(|(_, p)| matches!(p.style.entries[..], [PaintEntry::Fill(_)])), "turning the outline off keeps the fill");
}

#[test]
fn rasterize_layer_turns_a_vector_layer_into_paint_in_one_undo_step() {
    let (mut s, layer) = vector_session();
    draw_line(&mut s, 1, 30);
    assert!(s.command(CommandId::RasterizeLayer).enabled);
    invoke(&mut s, CommandId::RasterizeLayer);
    for t in 0..20 { s.frame(1_000_000_000 + t * 16_000_000, 1_000_000_000 + t * 16_000_000).unwrap(); }
    let document = s.engine.document();
    assert!(document.scene().paint_source(layer).is_some() && document.scene().vector_layer(layer).is_none(), "the layer holds paint");
    assert!(document.artwork.vectors.is_empty());
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).map(|o| o.path_count()), Some(1), "undo restores the paths");
}

#[test]
fn monoline_draws_constant_width_vector_lines() {
    let (mut s, layer) = vector_session();
    s.select_brush(DefaultBrushPreset::Monoline as u32).unwrap();
    draw_line(&mut s, 1, 40);
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    assert_eq!(object.path_count(), 1);
    let (_, path) = object.paths().next().unwrap();
    let stroke = path.style.strokes().next().unwrap();
    assert!(stroke.profile.is_none(), "pressure does not change Monoline's width");
    assert_eq!(stroke.width, f64::from(s.engine.configured_brush().diameter));
}

#[test]
fn move_frame_scales_and_rotates_selected_paths_in_one_undo_step() {
    let (mut s, layer) = vector_session();
    draw_line(&mut s, 1, 30);
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let drawn = object(&s);
    let id = drawn.paths().next().unwrap().1.id;
    let bounds = |s: &UiSession<Recorder>| object(s).paths().find(|(_, p)| p.id == id).unwrap().1.visual_bounds();
    invoke(&mut s, CommandId::Move);
    let at = surface_at(&s, drawn.paths().next().unwrap().1.geometry.subpaths()[0].anchor(0).point());
    press(&mut s, 50, &[at, at]);
    let mut overlay = Vec::new();
    s.append_layer_overlay(&mut overlay);
    assert_eq!(overlay.iter().filter(|segment| segment.marker == 2.).count(), 9, "the selection shows eight scale handles and a rotate handle");
    let [x0, y0, x1, y1] = bounds(&s);
    let corner = surface_at(&s, [x1, y1]);
    let far = surface_at(&s, [x1 + (x1 - x0), y1 + (y1 - y0)]);
    press(&mut s, 100, &[corner, Point { x: (corner.x + far.x) * 0.5, y: (corner.y + far.y) * 0.5 }, far]);
    let [a0, b0, a1, b1] = bounds(&s);
    assert!((a0 - x0).abs() < 0.5 && (b0 - y0).abs() < 0.5, "the opposite corner stays put");
    assert!(((a1 - a0) / (x1 - x0) - 2.).abs() < 0.05 && ((b1 - b0) / (y1 - y0) - 2.).abs() < 0.05, "dragging a corner scales: {:?} -> {:?}", [x0, y0, x1, y1], [a0, b0, a1, b1]);
    let stroke = |s: &UiSession<Recorder>| object(s).paths().next().unwrap().1.style.strokes().next().unwrap().width;
    assert!(stroke(&s) > drawn.paths().next().unwrap().1.style.strokes().next().unwrap().width * 1.9, "strokes scale with the paths");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(object(&s), drawn, "a frame drag is one undo step");
    let [x0, y0, x1, _] = bounds(&s);
    let reach = f64::from(s.ruler_reach());
    let rotate = [(x0 + x1) * 0.5, y0 - reach * 2.5];
    let centre = { let [x0, y0, x1, y1] = bounds(&s); [(x0 + x1) * 0.5, (y0 + y1) * 0.5] };
    let quarter = [centre[0] + (centre[1] - rotate[1]), centre[1]];
    let turn = [surface_at(&s, rotate), surface_at(&s, [rotate[0] + 10., rotate[1]]), surface_at(&s, quarter)];
    press(&mut s, 200, &turn);
    let first = |o: &layer_core::authored::vector::VectorObject| o.paths().next().unwrap().1.geometry.subpaths()[0].anchor(0).point();
    let (before, after) = (first(&drawn), first(&object(&s)));
    let expected = [centre[0] - (before[1] - centre[1]), centre[1] + (before[0] - centre[0])];
    assert!((after[0] - expected[0]).abs() < 0.5 && (after[1] - expected[1]).abs() < 0.5, "the rotate handle turns the paths about their centre: {after:?} vs {expected:?}");
}

#[test]
fn path_tool_builds_open_and_closed_paths_anchor_by_anchor() {
    use layer_core::authored::vector::AnchorKind;
    let (mut s, layer) = vector_session();
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } } }).unwrap();
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    click(&mut s, 10, [100., 100.]);
    let (smooth, handle) = (surface_at(&s, [200., 150.]), surface_at(&s, [240., 150.]));
    press(&mut s, 20, &[smooth, handle]);
    click(&mut s, 30, [300., 100.]);
    click(&mut s, 40, [350., 300.]);
    assert_eq!(object(&s).path_count(), 0, "nothing is added while drawing");
    assert!(s.command(CommandId::FinishPath).enabled && s.command(CommandId::RemoveSelectionPoint).enabled);
    let mut overlay = Vec::new();
    s.append_layer_overlay(&mut overlay);
    assert!(overlay.iter().filter(|segment| segment.marker == 2.).count() >= 4, "anchors and handles show while drawing");
    invoke(&mut s, CommandId::RemoveSelectionPoint);
    key(&mut s, "enter", true, false, false);
    let open = object(&s);
    assert_eq!(open.path_count(), 1);
    let (_, path) = open.paths().next().unwrap();
    let subpath = &path.geometry.subpaths()[0];
    assert!(!subpath.closed && subpath.len() == 3, "Remove Point drops the last anchor and Enter finishes an open path");
    assert_eq!(subpath.anchor(1).kind, AnchorKind::SYMMETRIC, "dragging makes a smooth anchor");
    assert!((subpath.anchor(1).out_handle()[0] - 40.).abs() < 0.5, "{:?}", subpath.anchor(1).out_handle());
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|selection| (*selection.items).clone()), Some([path.id].into()));
    for (i, at) in [[400., 400.], [500., 400.], [450., 480.]].into_iter().enumerate() { click(&mut s, 100 + i as u64 * 5, at); }
    click(&mut s, 200, [401., 401.]);
    let closed = object(&s);
    assert_eq!(closed.path_count(), 2);
    assert!(closed.paths().next().unwrap().1.geometry.subpaths()[0].closed, "clicking the first anchor closes the path");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(9_000_000_000, 9_000_000_000).unwrap();
    assert_eq!(object(&s).path_count(), 1, "each path is one undo step");
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } } }).unwrap();
    let paint = s.engine.document().drawing_content().unwrap();
    let operations = |s: &UiSession<Recorder>| s.engine.document().target_operations(paint).map_or(0, <[_]>::len);
    let before = operations(&s);
    click(&mut s, 300, [100., 100.]);
    click(&mut s, 310, [300., 120.]);
    click(&mut s, 320, [200., 300.]);
    let mut overlay = Vec::new();
    s.append_layer_overlay(&mut overlay);
    assert!(overlay.iter().any(|segment| segment.marker == 0.), "the drafted outline shows on a paint layer");
    key(&mut s, "enter", true, false, false);
    assert_eq!(s.state.notice, None);
    assert_eq!(operations(&s), before + 1, "Finish paints the path into the paint layer");
    assert!(matches!(s.engine.document().target_operations(paint).unwrap().last().unwrap().kind, layer_core::RasterOperationKind::Bake { .. }));
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(12_000_000_000, 12_000_000_000).unwrap();
    assert_eq!(operations(&s), before, "one undo step");
}

#[test]
fn edit_points_moves_adds_deletes_and_switches_anchors() {
    use layer_core::authored::vector::AnchorKind;
    let (mut s, layer) = vector_session();
    draw_line(&mut s, 1, 30);
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let id = object(&s).paths().next().unwrap().1.id;
    let subpath = |s: &UiSession<Recorder>| { let o = object(s); o.record(o.find(id).unwrap()).unwrap().path().unwrap().geometry.subpaths()[0].clone() };
    invoke(&mut s, CommandId::Move);
    let middle = { let sp = subpath(&s); sp.segment(sp.segment_count() / 2).point(0.5) };
    let at = surface_at(&s, middle);
    press(&mut s, 50, &[at, at]);
    invoke(&mut s, CommandId::EditPoints);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::EditPoints);
    let count = subpath(&s).len();
    let mut overlay = Vec::new();
    s.append_layer_overlay(&mut overlay);
    assert_eq!(overlay.iter().filter(|segment| segment.marker == 1.).count(), count * 4, "every anchor of the selected path shows");
    let first = subpath(&s).anchor(0).point();
    let from = surface_at(&s, first);
    let to = surface_at(&s, [first[0] + 30., first[1] + 20.]);
    press(&mut s, 100, &[from, Point { x: (from.x + to.x) * 0.5, y: (from.y + to.y) * 0.5 }, to]);
    let moved = subpath(&s).anchor(0).point();
    assert!((moved[0] - first[0] - 30.).abs() < 0.5 && (moved[1] - first[1] - 20.).abs() < 0.5, "dragging an anchor moves it: {moved:?}");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(subpath(&s).anchor(0).point(), first, "an anchor drag is one undo step");
    let on_segment = { let sp = subpath(&s); sp.segment(0).point(0.5) };
    let click = surface_at(&s, on_segment);
    press(&mut s, 200, &[click, click]);
    assert_eq!(subpath(&s).len(), count + 1, "clicking a segment adds an anchor");
    invoke(&mut s, CommandId::ClearSelected);
    assert_eq!(subpath(&s).len(), count, "Delete removes the selected anchor");
    let second = surface_at(&s, subpath(&s).anchor(1).point());
    let kind = subpath(&s).anchor(1).kind;
    press(&mut s, 300, &[second, second]);
    press(&mut s, 302, &[second, second]);
    assert_ne!(subpath(&s).anchor(1).kind, kind, "a double click switches corner and smooth");
    assert!(matches!(subpath(&s).anchor(1).kind, AnchorKind::CORNER | AnchorKind::SMOOTH));
    let [x0, y0, x1, y1] = object(&s).items_bounds(&[id].into()).unwrap();
    let corners = [surface_at(&s, [x0 - 20., y0 - 20.]), surface_at(&s, [(x0 + x1) * 0.5, y1 + 20.])];
    press(&mut s, 400, &corners);
    let expected = subpath(&s).anchors().filter(|a| a.point()[0] <= (x0 + x1) * 0.5).count();
    invoke(&mut s, CommandId::ClearSelected);
    assert_eq!(subpath(&s).len(), count - expected, "a marquee selects the anchors it encloses");
}

#[test]
#[ignore = "crowded vector layer session costs"]
fn crowded_vector_layer_session_costs() {
    use layer_core::authored::vector::*;
    for count in [10_000u64, 50_000] {
        let (mut s, layer) = vector_session();
        let records: Vec<Record> = (0..count).map(|i| {
            let x = (i % 400) as f64 * 10.;
            let y = (i / 400) as f64 * 6.;
            let anchors: Vec<_> = (0..4).map(|k| Anchor::new([x + k as f64 * 12., y + (k % 2) as f64 * 5.], [-4., 0.], [4., 0.], AnchorKind::SMOOTH)).collect();
            let mut stroke = Stroke::new(Paint::color(layer_core::color::RgbColor::BLACK), 3.);
            stroke.cap = Cap::Round; stroke.join = Join::Round;
            Record::Path(PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
        }).collect();
        let edit = s.engine.document().vector_object_edit(layer, VectorObject::from_records(records)).unwrap();
        let start = std::time::Instant::now();
        s.engine.apply_edit(edit).unwrap();
        let populate = start.elapsed().as_secs_f64() * 1e3;
        let mut commits = Vec::new();
        for line in 0..20u64 {
            let first = 1000 + line * 100;
            for i in 0..30u64 {
                let phase = if i == 0 { PenPhase::Down } else if i + 1 == 30 { PenPhase::Up } else { PenPhase::Move };
                let mut e = event(&s, first + i, phase, 0.7);
                e.surface_position.x = 150. + i as f32 * 6.;
                e.surface_position.y = 100. + line as f32 * 20.;
                let start = std::time::Instant::now();
                s.pen(e);
                s.frame((first + i) * 10_000_000, (first + i) * 10_000_000).unwrap();
                if phase == PenPhase::Up { commits.push(start.elapsed().as_secs_f64() * 1e3); }
            }
        }
        commits.sort_by(f64::total_cmp);
        let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let mut path = object.paths().next().unwrap().1.clone();
        path.id = object.allocate_id();
        let edit = s.engine.document().vector_object_edit(layer, object.insert_front(vec![Record::Path(path)])).unwrap();
        let mut document = s.engine.document().clone();
        let start = std::time::Instant::now();
        document.apply(edit.clone()).unwrap();
        let apply = start.elapsed().as_secs_f64() * 1e3;
        let start = std::time::Instant::now();
        s.engine.apply_edit(edit).unwrap();
        let engine_apply = start.elapsed().as_secs_f64() * 1e3;
        let start = std::time::Instant::now();
        s.frame(9_000_000_000, 9_000_000_000).unwrap();
        let frame = start.elapsed().as_secs_f64() * 1e3;
        let start = std::time::Instant::now();
        s.frame(9_010_000_000, 9_010_000_000).unwrap();
        let idle = start.elapsed().as_secs_f64() * 1e3;
        eprintln!("document apply {apply:.2} ms, engine apply {engine_apply:.2} ms, next frame {frame:.2} ms, idle frame {idle:.2} ms");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let start = std::time::Instant::now();
        let capture = s.engine.capture_session(0).unwrap();
        let prepared = layer_core::package::session::PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
        let autosave = start.elapsed().as_secs_f64() * 1e3;
        let start = std::time::Instant::now();
        let capture = s.engine.capture_session(0).unwrap();
        layer_core::package::session::PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
        let again = start.elapsed().as_secs_f64() * 1e3;
        let start = std::time::Instant::now();
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        let undo = start.elapsed().as_secs_f64() * 1e3;
        drop(prepared);
        eprintln!("{count} paths: populate {populate:.1} ms; pen-up commit p50 {:.2} max {:.2} ms; autosave {autosave:.1} ms, repeated {again:.1} ms; undo {undo:.2} ms",
            commits[commits.len() / 2], commits[commits.len() - 1]);
    }
}

#[test]
fn vector_layer_masks_take_brush_strokes_while_paths_stay() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 30);
    let drawn = s.engine.document().scene().vector_layer(handle).unwrap().clone();
    let id = occurrence_token(handle);
    layer(&mut s, LayerAction::AddMask { id, replace: false });
    layer(&mut s, LayerAction::Select { id, mask: true });
    assert!(matches!(s.engine.document().working.target, Some(SourceTarget::Coverage(_))));
    let revision = s.engine.document().revision;
    draw_line(&mut s, 100, 30);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap(), &drawn, "mask strokes leave the paths");
    assert_ne!(s.engine.document().revision, revision, "the stroke paints the mask");
    assert_eq!(s.state.notice, None);
    invoke(&mut s, CommandId::Move);
    let start = surface_at(&s, drawn.paths().next().unwrap().1.geometry.subpaths()[0].anchor(0).point());
    press(&mut s, 200, &[start, start]);
    assert!(s.engine.document().working.vector_items.is_none(), "Move on the mask leaves the paths unselected");
}

#[test]
fn move_and_edit_points_reach_paths_while_pixels_are_selected() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 40);
    let path = s.engine.document().scene().vector_layer(handle).unwrap().paths().next().unwrap().1.clone();
    select(&mut s, rectangle([0., 0., 50., 50.]));
    invoke(&mut s, CommandId::Move);
    let start = surface_at(&s, path.geometry.subpaths()[0].anchor(0).point());
    press(&mut s, 100, &[start, start]);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|selected| (*selected.items).clone()), Some([path.id].into()));
    assert_eq!(s.state.notice, None);
    invoke(&mut s, CommandId::EditPoints);
    let mut overlay = Vec::new();
    s.append_layer_overlay(&mut overlay);
    assert_eq!(overlay.iter().filter(|segment| segment.marker == 1.).count(), path.geometry.subpaths()[0].len() * 4, "Edit Points shows the anchors");
}

#[test]
fn clear_layer_removes_every_path_in_one_undo_step() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 30);
    draw_line(&mut s, 100, 30);
    let drawn = s.engine.document().scene().vector_layer(handle).unwrap().clone();
    assert!(s.command(CommandId::ClearLayer).enabled);
    invoke(&mut s, CommandId::ClearLayer);
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(handle).map(|object| object.path_count()), Some(0), "the layer stays a vector layer");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(4_000_000_000, 4_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap(), &drawn, "one undo step restores the paths");
    select(&mut s, rectangle([0., 0., 500., 500.]));
    assert!(!s.command(CommandId::FillSelection).enabled, "vector layers hold no pixels to fill");
}

#[test]
fn copy_and_the_wand_read_what_a_vector_layer_shows() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 30);
    select(&mut s, rectangle([0., 0., 2000., 2000.]));
    invoke(&mut s, CommandId::Copy);
    assert_eq!(s.state.notice, None);
    let copy = s.state.requests.iter().find_map(|r| match &r.kind {
        HostRequestKind::Document { request: DocumentRequest::Copy { .. } } => Some(r.id),
        _ => None,
    }).expect("a copy request");
    assert_eq!(s.capture_clipboard(copy).unwrap().scope, SceneScope::RawObjects(handle));
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: false, source: crate::RegionSource::Editing } }).unwrap();
    let point = surface_at(&s, [600., 600.]);
    press(&mut s, 100, &[point, point]);
    let request = s.renderer_mut().region_requests.last().cloned().expect("the wand reads the layer");
    assert!(matches!(request.source, layer_render::RegionSource::Scene { scope: SceneScope::RawObjects(layer), .. } if layer == handle));
}

#[test]
fn new_vector_layers_switch_to_a_pen_and_paint_layers_switch_back() {
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    let paint = s.engine.document().working.occurrence.unwrap();
    let command = s.command(CommandId::NewVectorLayer);
    assert!(command.enabled);
    assert_eq!((command.icon, command.shortcut.as_str()), (Some("add-vector-layer"), "Ctrl+Alt+Shift+N"));
    invoke(&mut s, CommandId::NewVectorLayer);
    let vector = s.engine.document().working.occurrence.unwrap();
    assert!(s.engine.document().scene().vector_layer(vector).is_some());
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32, "a new vector layer picks a pen that draws paths");
    let row = s.state.layers.iter().find(|row| row.id == occurrence_token(vector)).unwrap();
    assert_eq!(row.content_icon.as_deref(), Some("layer-path-symbolic"));
    assert_eq!(s.state.layer_properties.layer_type, s.localization().text(MessageId::RESOURCES_LAYER_TYPE_VECTOR).as_ref());
    assert_eq!(s.state.layer_properties.title, row.label);
    let usable = |s: &UiSession<Recorder>, preset: DefaultBrushPreset| s.state.tool_set.subtools.iter()
        .find(|item| item.action == UiAction::SelectBrush { id: preset as u32 }).map(|item| item.disabled_reason.is_none());
    assert_eq!((usable(&s, DefaultBrushPreset::GPen), usable(&s, DefaultBrushPreset::Monoline), usable(&s, DefaultBrushPreset::RoughGPen)), (Some(true), Some(true), Some(false)));
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Pencil as u32, "a paint layer brings the painting brush back");
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    assert_eq!(usable(&s, DefaultBrushPreset::RoughGPen), Some(true), "paint layers take every brush");
    layer(&mut s, LayerAction::Select { id: occurrence_token(vector), mask: false });
    s.select_brush(DefaultBrushPreset::Monoline as u32).unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Monoline as u32, "a pen the painter chose stays");
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(vector), mask: false });
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Monoline as u32, "the last pen used on paths comes back");
}

#[test]
fn the_layer_menu_offers_new_vector_layer_in_every_state() {
    let mut s = session(Platform::Gtk);
    let finds = |s: &UiSession<Recorder>| s.application_menu(ApplicationMenu::Layer).sections.iter().flatten()
        .flat_map(|item| item.sections.iter().flatten())
        .find(|item| item.action == Some(UiAction::Layer { action: LayerAction::NewVector }))
        .map(|item| (item.label.clone(), item.enabled));
    let label = CommandId::NewVectorLayer.localized_label(s.localization()).to_string();
    assert_eq!(finds(&s), Some((label.clone(), true)));
    let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::AddMask { id, replace: false });
    layer(&mut s, LayerAction::Select { id, mask: true });
    assert_eq!(finds(&s), Some((label.clone(), true)), "while a mask is edited");
    layer(&mut s, LayerAction::Select { id, mask: false });
    select(&mut s, rectangle([10., 10., 60., 60.]));
    invoke(&mut s, CommandId::NewSelectionLayer);
    assert!(s.engine.document().working.occurrence.and_then(|h| s.engine.document().scene().occurrence(h)).is_some_and(|l| l.kind() == LayerKind::Selection));
    assert_eq!(finds(&s), Some((label.clone(), true)), "while a selection layer is active");
    let mut working = s.engine.document().working.clone();
    working.occurrence = None;
    working.target = None;
    s.layer_edit(layer_core::Edit::Working(working)).unwrap();
    assert_eq!(finds(&s), Some((label, true)), "with no active layer");
    invoke(&mut s, CommandId::NewVectorLayer);
    assert!(s.engine.document().working.occurrence.is_some_and(|h| s.engine.document().scene().vector_layer(h).is_some()));
}

#[test]
fn painting_brushes_on_vector_layers_offer_a_pen_a_paint_layer_or_rasterizing() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 20);
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    let refused = |s: &mut UiSession<Recorder>, first: u64| {
        let revision = s.engine.document().revision;
        draw_line(s, first, 10);
        assert_eq!(s.engine.document().revision, revision, "nothing reaches the paths");
        s.state.notice.clone().expect("a vector refusal")
    };
    let notice = refused(&mut s, 100);
    assert_eq!(notice.text, "Pencil doesn’t work on vector layers.");
    assert!(notice.actions[0].accent && notice.actions[1..].iter().all(|a| !a.accent), "the suggested way forward comes first");
    assert_eq!(notice.actions.iter().map(|a| (a.id, a.label.as_str(), a.enabled)).collect::<Vec<_>>(),
        [(NoticeActionId::UseVectorPen, "Use G-Pen", true), (NoticeActionId::NewPaintLayer, "New Paint Layer", true), (NoticeActionId::RasterizeLayer, "Rasterize Layer", true)]);
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::UseVectorPen) }).unwrap();
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32);
    draw_line(&mut s, 200, 20);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 2, "the pen draws a path");
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    let notice = refused(&mut s, 300);
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::NewPaintLayer) }).unwrap();
    let paint = s.engine.document().working.occurrence.unwrap();
    assert!(s.engine.document().scene().paint_source(paint).is_some());
    assert_eq!(s.engine.document().scene().children(None).iter().position(|h| *h == paint).unwrap() + 1,
        s.engine.document().scene().children(None).iter().position(|h| *h == handle).unwrap(), "the paint layer sits above the lines");
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Pencil as u32, "the brush stays");
    layer(&mut s, LayerAction::Select { id: occurrence_token(handle), mask: false });
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    let notice = refused(&mut s, 400);
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::RasterizeLayer) }).unwrap();
    for t in 0..20 { s.frame(9_000_000_000 + t * 16_000_000, 9_000_000_000 + t * 16_000_000).unwrap(); }
    assert!(s.engine.document().scene().paint_source(handle).is_some(), "the lines become paint");
}

#[test]
fn a_pen_setting_that_stops_paths_is_named_and_reset_from_the_refusal() {
    let (mut s, handle) = vector_session();
    s.dispatch(UiAction::SetToolSetting { id: "size_jitter".into(), value: 0.5 }).unwrap();
    assert_eq!(s.state.tool_set.subtools.iter().find(|item| item.action == UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).map(|item| item.disabled_reason.is_some()), Some(true));
    let revision = s.engine.document().revision;
    draw_line(&mut s, 1, 10);
    assert_eq!(s.engine.document().revision, revision);
    let notice = s.state.notice.clone().expect("a vector refusal");
    let label = s.state.tool_settings.iter().find(|setting| setting.id == "size_jitter").unwrap().label.clone();
    let mut args = FluentArgs::new(); args.set("layer", s.engine.document().scene().occurrence(handle).unwrap().name.to_string()); args.set("setting", label.as_ref()); args.set("brush", "G-Pen");
    assert_eq!(notice.text, s.localization().format(MessageId::OBJECTS_VECTOR_REFUSAL_SETTING, &args));
    assert_eq!(notice.actions[0].id, NoticeActionId::ResetSetting);
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::ResetSetting) }).unwrap();
    draw_line(&mut s, 100, 20);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 1);
}

#[test]
fn fill_tools_on_vector_layers_offer_a_paint_layer_below() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 20);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Gradient { shape: layer_core::GradientShape::Linear } }).unwrap();
    let start = surface_at(&s, [100., 100.]);
    press(&mut s, 100, &[start, Point { x: start.x + 80., y: start.y }]);
    let notice = s.state.notice.clone().expect("a vector refusal");
    assert_eq!(notice.text, "Gradient doesn’t work on vector layers.");
    assert_eq!(notice.actions.iter().map(|a| a.id).collect::<Vec<_>>(), [NoticeActionId::NewPaintLayerBelow, NoticeActionId::RasterizeLayer]);
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::NewPaintLayerBelow) }).unwrap();
    let paint = s.engine.document().working.occurrence.unwrap();
    let order = s.engine.document().scene().children(None).to_vec();
    assert!(s.engine.document().scene().paint_source(paint).is_some());
    assert_eq!(order.iter().position(|h| *h == paint), order.iter().position(|h| *h == handle).map(|i| i + 1), "the paint layer sits under the lines");
    assert!(matches!(s.layer_interaction.tool, LayerCanvasTool::Gradient { .. }), "the tool stays");
}

#[test]
fn select_all_transform_flip_escape_and_delete_act_on_paths() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 30);
    draw_line(&mut s, 100, 30);
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().clone();
    let drawn = object(&s);
    let all: std::collections::BTreeSet<_> = drawn.paths().map(|(_, path)| path.id).collect();
    assert!(!s.command(CommandId::Deselect).enabled);
    invoke(&mut s, CommandId::SelectAll);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|v| (*v.items).clone()), Some(all.clone()));
    assert!(s.engine.document().working.selection.is_none(), "Select All selects paths, not pixels");
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move);
    assert_eq!(s.command(CommandId::ClearSelected).label.as_ref(), "Delete");
    for command in [CommandId::TransformFlipHorizontal, CommandId::TransformFlipVertical, CommandId::TransformRotateLeft, CommandId::TransformRotateRight, CommandId::ScaleOutlineWidths] {
        assert!(s.state.tool_actions.iter().any(|action| action.command == command), "{command:?} is a Move option for paths");
    }
    assert!(s.command(CommandId::ScaleOutlineWidths).selected, "outline widths scale by default");
    let [x0, _, x1, _] = drawn.paths().map(|(_, path)| path.visual_bounds()).fold(None, |bounds, path| Some(layer_core::authored::vector::union(bounds, path))).unwrap();
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    let flipped = object(&s);
    for ((_, before), (_, after)) in drawn.paths().zip(flipped.paths()) {
        let [a, b] = [before.geometry.subpaths()[0].anchor(0).point(), after.geometry.subpaths()[0].anchor(0).point()];
        assert!((b[0] - (x0 + x1 - a[0])).abs() <= 1. / 256. && (b[1] - a[1]).abs() < 1e-6, "{a:?} mirrors to {b:?}");
    }
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert_eq!(object(&s), drawn, "one undo step restores the flip");
    invoke(&mut s, CommandId::SelectAll);
    key(&mut s, "escape", true, false, false);
    key(&mut s, "escape", false, false, false);
    assert!(s.engine.document().working.vector_items.is_none(), "Escape clears the path selection");
    invoke(&mut s, CommandId::SelectAll);
    assert!(s.command(CommandId::Deselect).enabled);
    invoke(&mut s, CommandId::Deselect);
    assert!(s.engine.document().working.vector_items.is_none());
    let start = surface_at(&s, drawn.paths().next().unwrap().1.geometry.subpaths()[0].segment(0).point(0.3));
    press(&mut s, 300, &[start, start]);
    key(&mut s, "enter", true, false, false);
    key(&mut s, "enter", false, false, false);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::EditPoints, "Enter edits the selected path's points");
    key(&mut s, "escape", true, false, false);
    key(&mut s, "escape", false, false, false);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move, "Escape leaves Edit Points");
    assert!(s.engine.document().working.vector_items.is_some());
    let frame = s.vector_frame().unwrap();
    let body = drawn.paths().next().unwrap().1.geometry.subpaths()[0].segments()
        .flat_map(|segment| (1..10).map(move |i| segment.point(f64::from(i) / 10.)))
        .find(|point| !matches!(frame.hit(*point, f64::from(s.ruler_reach())), Some(super::transform_frame::FrameHandle::Scale(_) | super::transform_frame::FrameHandle::Rotate))).unwrap();
    let start = surface_at(&s, body);
    press(&mut s, 400, &[start, start]);
    press(&mut s, 402, &[start, start]);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::EditPoints, "a double-click edits the path's points");
    invoke(&mut s, CommandId::Move);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::ClearSelected);
    assert_eq!(object(&s).path_count(), 0, "Delete removes the selected paths");
}

#[test]
fn edit_transform_shows_the_move_frame_and_outline_widths_can_stay() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 30);
    let width = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().paths().next().unwrap().1.style.strokes().next().unwrap().width;
    let before = width(&s);
    assert!(s.command(CommandId::ScaleRotate).enabled, "Scale/rotate picks paths before any are selected");
    invoke(&mut s, CommandId::SelectAll);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Paint }).unwrap();
    assert!(s.command(CommandId::ScaleRotate).enabled);
    invoke(&mut s, CommandId::ScaleRotate);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move, "Edit › Transform shows the Move frame");
    invoke(&mut s, CommandId::ScaleOutlineWidths);
    assert!(!s.command(CommandId::ScaleOutlineWidths).selected);
    assert_eq!(saved_settings(&s).map(|saved| saved.keep_outline_widths), Some(true), "the choice is saved");
    let frame = s.vector_frame().unwrap();
    let corner = frame.unit.map([1., 1.]);
    let from = surface_at(&s, corner);
    let to = surface_at(&s, [corner[0] + (corner[0] - frame.unit.map([0., 0.])[0]), corner[1] + (corner[1] - frame.unit.map([0., 0.])[1])]);
    press(&mut s, 100, &[from, Point { x: (from.x + to.x) * 0.5, y: (from.y + to.y) * 0.5 }, to]);
    assert_eq!(width(&s), before, "outline widths stay while the path doubles");
    invoke(&mut s, CommandId::ScaleOutlineWidths);
    let frame = s.vector_frame().unwrap();
    let corner = frame.unit.map([1., 1.]);
    let from = surface_at(&s, corner);
    let to = surface_at(&s, [corner[0] + (corner[0] - frame.unit.map([0., 0.])[0]), corner[1] + (corner[1] - frame.unit.map([0., 0.])[1])]);
    press(&mut s, 200, &[from, Point { x: (from.x + to.x) * 0.5, y: (from.y + to.y) * 0.5 }, to]);
    assert!((width(&s) - before * 2.).abs() < before * 0.05, "{} scales with the path", width(&s));
}

#[test]
fn path_commands_are_disabled_on_paint_layers() {
    let commands = [CommandId::JoinPaths, CommandId::ClosePath, CommandId::SplitPath,
        CommandId::ReversePath, CommandId::MakeCompoundPath, CommandId::ReleaseCompoundPath];
    for platform in Platform::ALL {
        let mut s = session(platform);
        let checkpoint = s.engine.checkpoint();
        assert_eq!(commands.map(|command| s.command(command).enabled), [false; 6], "{platform:?}");
        let reason = s.localization().text(MessageId::COMMANDS_SELECT_PATHS_ON_A_VECTOR_LAYER).to_string();
        for command in commands {
            assert_eq!(s.command(command).disabled_reason.as_deref(), Some(reason.as_str()), "{platform:?}: {command:?}");
            assert_eq!(s.dispatch(UiAction::Invoke { command }).err(), Some(reason.clone()), "{platform:?}: {command:?}");
            assert_eq!(s.engine.checkpoint(), checkpoint, "{platform:?}: {command:?}");
        }
    }
}

#[test]
fn path_command_eligibility_keeps_open_closed_and_compound_selections() {
    use layer_core::authored::vector::PathItem;
    let commands = [CommandId::JoinPaths, CommandId::ClosePath, CommandId::SplitPath,
        CommandId::ReversePath, CommandId::MakeCompoundPath, CommandId::ReleaseCompoundPath];
    for platform in Platform::ALL {
        let (mut s, handle) = vector_session_on(platform);
        let open = straight(1, &[[100., 300.], [300., 300.], [500., 300.]], false);
        let other_open = straight(2, &[[600., 300.], [700., 300.], [700., 500.]], false);
        let closed = straight(3, &[[100., 500.], [300., 500.], [200., 650.]], true);
        let other_closed = straight(4, &[[600., 600.], [800., 600.], [700., 750.]], true);
        let cases = [
            ("empty", vec![], [false; 6]),
            ("one open", vec![open.clone()], [true, true, false, true, false, false]),
            ("one closed", vec![closed.clone()], [false, false, false, true, false, false]),
            ("two open", vec![open.clone(), other_open.clone()], [true, true, false, true, true, false]),
            ("two closed", vec![closed.clone(), other_closed.clone()], [false, false, false, true, true, false]),
            ("mixed", vec![open.clone(), closed.clone()], [false, true, false, true, true, false]),
            ("three paths", vec![open.clone(), other_open.clone(), closed.clone()], [false, true, false, true, true, false]),
            ("open compound", vec![PathItem::compound(&[open.clone(), other_open]).unwrap()], [true, true, false, true, false, true]),
            ("closed compound", vec![PathItem::compound(&[closed, other_closed]).unwrap()], [false, false, false, true, false, true]),
        ];
        for (name, paths, expected) in cases {
            put_paths(&mut s, handle, paths);
            let items = path_ids(&s, handle).into_iter().collect();
            s.select_vector_items(handle, items).unwrap();
            assert_eq!(commands.map(|command| s.command(command).enabled), expected, "{platform:?}: {name}");
        }
        put_paths(&mut s, handle, vec![open]);
        s.select_vector_items(handle, path_ids(&s, handle).into_iter().collect()).unwrap();
        invoke(&mut s, CommandId::EditPoints);
        let middle = surface_at(&s, [300., 300.]);
        press(&mut s, 100, &[middle, middle]);
        assert_eq!(commands.map(|command| s.command(command).enabled), [true, true, true, true, false, false], "{platform:?}: selected interior anchor");
    }
}

#[test]
fn path_commands_join_split_close_reverse_and_compound_selected_paths() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 30);
    draw_line(&mut s, 100, 30);
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().clone();
    let edit = s.application_menu(ApplicationMenu::Edit);
    let path_menu = edit.sections.iter().flatten().find(|item| item.label == s.localization().text(MessageId::MENU_PATH).as_ref()).expect("Edit › Path");
    let commands: Vec<_> = path_menu.sections.iter().flatten().filter_map(|item| match item.action { Some(UiAction::Invoke { command }) => Some(command), _ => None }).collect();
    assert_eq!(commands, [CommandId::JoinPaths, CommandId::ClosePath, CommandId::SplitPath, CommandId::ReversePath, CommandId::MakeCompoundPath, CommandId::ReleaseCompoundPath]);
    assert!(!s.command(CommandId::JoinPaths).enabled);
    invoke(&mut s, CommandId::SelectAll);
    for (command, enabled) in [(CommandId::JoinPaths, true), (CommandId::ClosePath, true), (CommandId::SplitPath, false), (CommandId::ReversePath, true), (CommandId::MakeCompoundPath, true), (CommandId::ReleaseCompoundPath, false)] {
        assert_eq!(s.command(command).enabled, enabled, "{command:?}");
    }
    let before = object(&s);
    invoke(&mut s, CommandId::JoinPaths);
    let joined = object(&s);
    assert_eq!(joined.path_count(), 1, "two open lines join into one");
    let path = joined.paths().next().unwrap().1.clone();
    let anchors = path.geometry.subpaths()[0].len();
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|v| (*v.items).clone()), Some([path.id].into()));
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert_eq!(object(&s), before, "one undo step restores both lines");
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    s.frame(5_100_000_000, 5_100_000_000).unwrap();
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::ReversePath);
    let reversed = object(&s).paths().next().unwrap().1.clone();
    assert_eq!(reversed.geometry.subpaths()[0].anchor(0).point(), path.geometry.subpaths()[0].anchor(anchors - 1).point());
    invoke(&mut s, CommandId::EditPoints);
    let middle = reversed.geometry.subpaths()[0].anchor(anchors / 2).point();
    let at = surface_at(&s, middle);
    press(&mut s, 300, &[at, at]);
    assert!(s.command(CommandId::SplitPath).enabled, "a selected anchor can be split");
    invoke(&mut s, CommandId::SplitPath);
    assert_eq!(object(&s).path_count(), 2);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|v| v.items.len()), Some(2), "both pieces stay selected");
    invoke(&mut s, CommandId::Move);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::MakeCompoundPath);
    assert_eq!(object(&s).path_count(), 1);
    assert!(s.command(CommandId::ReleaseCompoundPath).enabled);
    invoke(&mut s, CommandId::ReleaseCompoundPath);
    assert_eq!(object(&s).path_count(), 2);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::ClosePath);
    assert!(object(&s).paths().all(|(_, path)| path.geometry.subpaths()[0].closed));
}

#[test]
fn pens_and_the_path_tool_continue_a_selected_paths_open_end() {
    let (mut s, handle) = vector_session();
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().clone();
    let stroke = |s: &mut UiSession<Recorder>, first: u64, from: [f64; 2], to: [f64; 2]| {
        let points: Vec<Point> = (0..=20).map(|i| { let u = f64::from(i) / 20.; surface_at(s, [from[0] + (to[0] - from[0]) * u, from[1] + (to[1] - from[1]) * u + (u * 3.1).sin() * 20.]) }).collect();
        press(s, first, &points);
    };
    stroke(&mut s, 1, [200., 200.], [600., 200.]);
    let first = object(&s).paths().next().unwrap().1.clone();
    let [_, end] = first.open_ends().unwrap();
    stroke(&mut s, 100, end, [900., 400.]);
    assert_eq!(object(&s).path_count(), 2, "without a selection a new line starts");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    invoke(&mut s, CommandId::Move);
    let on = surface_at(&s, first.geometry.subpaths()[0].anchor(0).point());
    press(&mut s, 200, &[on, on]);
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    stroke(&mut s, 300, end, [900., 400.]);
    let continued = object(&s);
    assert_eq!(continued.path_count(), 1, "a line from the selected path's end continues it");
    let path = continued.paths().next().unwrap().1;
    assert_eq!(path.id, first.id);
    assert!(path.geometry.subpaths()[0].len() > first.geometry.subpaths()[0].len());
    let [start, far] = path.open_ends().unwrap();
    assert_eq!(start, first.open_ends().unwrap()[0]);
    assert!((far[0] - 900.).abs() < 2. && (far[1] - 400.).abs() < 2., "{far:?}");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    assert_eq!(object(&s).paths().next().unwrap().1, &first, "one undo step restores the line");
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } } }).unwrap();
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    click(&mut s, 400, end);
    click(&mut s, 410, [700., 500.]);
    click(&mut s, 420, first.open_ends().unwrap()[0]);
    let closed = object(&s);
    assert_eq!(closed.path_count(), 1, "the Path tool continues the selected path");
    let path = closed.paths().next().unwrap().1;
    assert!(path.geometry.subpaths()[0].closed, "clicking the path's other end closes it");
    assert_eq!(path.geometry.subpaths()[0].len(), first.geometry.subpaths()[0].len() + 1);
}

#[test]
fn figure_options_shape_rectangles_polygons_and_stars_on_both_layer_kinds() {
    use layer_core::authored::vector::{LiveShape, ShapeKind};
    let (mut s, layer) = vector_session();
    let ids = |s: &UiSession<Recorder>| s.state.tool_settings.iter().map(|c| c.id).filter(|id| id.starts_with("figure_")).collect::<Vec<_>>();
    let tool = |s: &mut UiSession<Recorder>, shape| s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape, paint: FigurePaint::Outline } } }).unwrap();
    let drag = |s: &mut UiSession<Recorder>, first: u64| { let points = [surface_at(s, [100., 100.]), surface_at(s, [200., 160.]), surface_at(s, [300., 260.])]; press(s, first, &points); };
    let front = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().paths().next().unwrap().1.shape.as_deref().copied();
    tool(&mut s, FigureShape::Rectangle);
    assert_eq!(ids(&s), ["figure_corner_radius"]);
    s.dispatch(UiAction::SetToolSetting { id: "figure_corner_radius".into(), value: 24. }).unwrap();
    drag(&mut s, 10);
    assert!(matches!(front(&s), Some(LiveShape { kind: ShapeKind::Rectangle { radii: [24., 24., 24., 24.] }, .. })));
    tool(&mut s, FigureShape::Polygon);
    assert_eq!(ids(&s), ["figure_corners"]);
    s.dispatch(UiAction::SetToolSetting { id: "figure_corners".into(), value: 7. }).unwrap();
    assert!(s.dispatch(UiAction::SetToolSetting { id: "figure_corners".into(), value: 2. }).is_err());
    drag(&mut s, 30);
    assert!(matches!(front(&s), Some(LiveShape { kind: ShapeKind::Polygon { corners: 7 }, .. })));
    tool(&mut s, FigureShape::Star);
    assert_eq!(ids(&s), ["figure_points", "figure_inner_radius"]);
    s.dispatch(UiAction::SetToolSetting { id: "figure_inner_radius".into(), value: 0.3 }).unwrap();
    s.dispatch(UiAction::ResetToolSetting { id: "figure_points".into() }).unwrap();
    drag(&mut s, 40);
    assert!(matches!(front(&s), Some(LiveShape { kind: ShapeKind::Star { points: 5, inner }, .. }) if (inner - 0.3).abs() < 1e-6));
    s.layer_action(LayerAction::New { group: false, clipped: false }).unwrap();
    let paint = s.engine.document().drawing_content().unwrap();
    let last = |s: &UiSession<Recorder>| s.engine.backend().pending_operations.last().filter(|(target, _)| *target == paint).map(|(_, op)| op.kind.clone());
    for (shape, first) in [(FigureShape::Polygon, 50), (FigureShape::Star, 60), (FigureShape::Rectangle, 70)] {
        tool(&mut s, shape);
        drag(&mut s, first);
        assert!(matches!(last(&s), Some(layer_core::RasterOperationKind::Bake { .. })), "{shape:?} paints through a bake");
    }
    s.dispatch(UiAction::SetToolSetting { id: "figure_corner_radius".into(), value: 0. }).unwrap();
    drag(&mut s, 80);
    assert!(matches!(last(&s), Some(layer_core::RasterOperationKind::Figure(_))), "square rectangles keep the raster figure");
}

#[test]
fn erasing_up_to_intersection_stops_at_crossing_lines() {
    let (mut s, handle) = vector_session();
    let line = |s: &mut UiSession<Recorder>, first: u64, from: [f64; 2], to: [f64; 2]| {
        let points: Vec<Point> = (0..=12).map(|i| { let u = f64::from(i) / 12.; surface_at(s, [from[0] + (to[0] - from[0]) * u, from[1] + (to[1] - from[1]) * u]) }).collect();
        press(s, first, &points);
    };
    line(&mut s, 1, [100., 300.], [700., 300.]);
    line(&mut s, 100, [300., 100.], [300., 500.]);
    line(&mut s, 200, [500., 100.], [500., 500.]);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 3);
    invoke(&mut s, CommandId::Eraser);
    invoke(&mut s, CommandId::EraseToIntersection);
    assert!(s.command(CommandId::EraseToIntersection).selected && !s.command(CommandId::EraseTouchedPart).selected);
    assert_eq!(saved_settings(&s).map(|saved| saved.vector_eraser), Some(crate::VectorEraser::ToIntersection), "the eraser mode is saved");
    let highlighted: Vec<_> = s.state.tool_panels.tools.subtools.iter().take(3).map(|item| item.selected).collect();
    assert_eq!(highlighted, [false, false, true], "the Tools list highlights the chosen mode");
    s.dispatch(UiAction::SetBrushSize { value: 6. }).unwrap();
    line(&mut s, 300, [400., 290.], [400., 310.]);
    let object = s.engine.document().scene().vector_layer(handle).unwrap().clone();
    let spans: Vec<[f64; 4]> = object.paths().filter(|(_, p)| { let h = p.geometry.hull(); (h[1] - h[3]).abs() < 5. }).map(|(_, p)| p.visual_bounds()).collect();
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert!(spans.iter().any(|h| (h[2] - 300.).abs() < 2.) && spans.iter().any(|h| (h[0] - 500.).abs() < 2.), "the line loses only the span between the crossings: {spans:?}");
}

#[test]
fn fill_colors_paths_or_fills_a_new_paint_layer_below_from_the_lines() {
    let (mut s, handle) = vector_session();
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Ellipse, paint: FigurePaint::Outline } } }).unwrap();
    let drag = [surface_at(&s, [200., 200.]), surface_at(&s, [300., 260.]), surface_at(&s, [400., 320.])];
    press(&mut s, 1, &drag);
    let ellipse = s.engine.document().scene().vector_layer(handle).unwrap().paths().next().unwrap().1.clone();
    s.dispatch(UiAction::SetColor { rgba: [0.2, 0.6, 0.3, 1.] }).unwrap();
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } }).unwrap();
    let prohibited = |s: &mut UiSession<Recorder>| { s.cursor_input(Some(event(s, 1, PenPhase::Hover, 0.))); s.canvas_cursor().is_some_and(|c| c.segments.iter().any(|segment| segment.marker == 6.)) };
    assert!(!prohibited(&mut s), "Fill works on a vector layer");
    let layers = s.engine.document().scene().children(None).len();
    let inside = surface_at(&s, [300., 260.]);
    press(&mut s, 10, &[inside, inside]);
    let filled = s.engine.document().scene().vector_layer(handle).unwrap().find(ellipse.id).map(|p| s.engine.document().scene().vector_layer(handle).unwrap().record(p).unwrap().path().unwrap().clone()).unwrap();
    assert!(matches!(filled.style.entries[..], [layer_core::authored::vector::PaintEntry::Fill(_), layer_core::authored::vector::PaintEntry::Stroke(_)]), "a click inside a closed path fills it under its outline");
    assert_eq!(s.engine.document().scene().children(None).len(), layers, "no layer is added");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    assert!(s.engine.document().scene().vector_layer(handle).unwrap().paths().next().unwrap().1.style.entries.len() == 1, "one undo step");
    let requests = s.renderer_mut().region_requests.len();
    let outside = surface_at(&s, [700., 600.]);
    press(&mut s, 20, &[outside, outside]);
    let paint = s.engine.document().working.occurrence.unwrap();
    let order = s.engine.document().scene().children(None).to_vec();
    assert!(s.engine.document().scene().paint_source(paint).is_some());
    assert_eq!(order.iter().position(|h| *h == paint), order.iter().position(|h| *h == handle).map(|i| i + 1), "elsewhere fills a new paint layer below the lines");
    assert_eq!(s.renderer_mut().region_requests.len(), requests + 1);
    let request = s.renderer_mut().region_requests.last().unwrap().clone();
    assert!(matches!(request.source, layer_render::RegionSource::Scene { scope: SceneScope::All, .. }), "Visible fill reads the visible artwork");
}

#[test]
fn lasso_fill_adds_a_filled_path_on_vector_layers() {
    let (mut s, handle) = vector_session();
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::LassoFill } }).unwrap();
    s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.)));
    assert!(!s.canvas_cursor().is_some_and(|c| c.segments.iter().any(|segment| segment.marker == 6.)), "Lasso Fill works on a vector layer");
    let corners = [[300., 200.], [500., 200.], [500., 400.], [300., 400.], [300., 200.]];
    let ring: Vec<Point> = corners.windows(2).flat_map(|w| (0..20).map(move |i| { let u = f64::from(i) / 20.; [w[0][0] + (w[1][0] - w[0][0]) * u, w[0][1] + (w[1][1] - w[0][1]) * u] })).map(|p| surface_at(&s, p)).collect();
    press(&mut s, 1, &ring);
    let object = s.engine.document().scene().vector_layer(handle).unwrap().clone();
    assert_eq!(object.path_count(), 1);
    let path = object.paths().next().unwrap().1;
    assert!(path.geometry.subpaths()[0].closed && matches!(path.style.entries[..], [layer_core::authored::vector::PaintEntry::Fill(_)]));
    assert!(path.geometry.anchor_count() <= 6, "straight sides keep only their corners: {}", path.geometry.anchor_count());
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|v| (*v.items).clone()), Some([path.id].into()));
}

#[test]
fn merging_vector_layers_keeps_paths_and_baking_them_asks_first() {
    let (mut s, lower) = vector_session();
    draw_line(&mut s, 1, 20);
    layer(&mut s, LayerAction::NewVector);
    draw_line(&mut s, 100, 20);
    invoke(&mut s, CommandId::MergeDown);
    assert_eq!(s.state.notice, None);
    let merged = s.engine.document().scene().vector_layer(lower).expect("vector onto vector stays vector");
    assert_eq!(merged.path_count(), 2);
    s.layer_action(LayerAction::NewBelow).unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(lower), mask: false });
    invoke(&mut s, CommandId::MergeDown);
    let notice = s.state.notice.clone().expect("merging into paint asks first");
    let name = s.engine.document().scene().occurrence(lower).unwrap().name.to_string();
    assert!(notice.text.contains(&name), "{}", notice.text);
    assert_eq!(notice.actions.iter().map(|a| (a.id, a.label.as_str())).collect::<Vec<_>>(), [(NoticeActionId::Merge, CommandId::MergeDown.localized_label(s.localization()).as_ref())]);
    assert!(s.engine.document().scene().vector_layer(lower).is_some(), "nothing changes before the answer");
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::Merge) }).unwrap();
    assert!(s.engine.document().scene().vector_layer(lower).is_none(), "accepting bakes the paths into the paint layer");
}

fn straight(id: u64, points: &[[f64; 2]], closed: bool) -> layer_core::authored::vector::PathItem {
    use layer_core::authored::vector::*;
    let anchors: Vec<Anchor> = points.iter().map(|p| Anchor::corner(*p)).collect();
    let stroke = Stroke::new(Paint::color(layer_core::color::RgbColor::BLACK), 4.);
    PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, closed)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])))
}
fn put_paths(s: &mut UiSession<Recorder>, layer: OccurrenceHandle, paths: Vec<layer_core::authored::vector::PathItem>) {
    let object = layer_core::authored::vector::VectorObject::from_records(paths.into_iter().map(layer_core::authored::vector::Record::Path).collect());
    let edit = s.engine.document().vector_object_edit(layer, object).unwrap();
    s.layer_edit(edit).unwrap();
    s.frame(1, 1).unwrap();
}
fn path_ids(s: &UiSession<Recorder>, layer: OccurrenceHandle) -> Vec<layer_core::authored::vector::ItemId> {
    s.engine.document().scene().vector_layer(layer).unwrap().paths().map(|(_, path)| path.id).collect()
}

#[test]
fn fill_ignores_clicks_off_the_canvas_and_fills_that_change_nothing() {
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 100.], [400., 100.], [400., 400.], [100., 400.]], true)]);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } }).unwrap();
    let steps = |s: &UiSession<Recorder>| (s.engine.document().scene().children(None).len(), s.engine.checkpoint());
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    let before = steps(&s);
    click(&mut s, 10, [-50., 250.]);
    assert_eq!(steps(&s), before, "a click off the canvas adds no layer");
    s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Transparent } }).unwrap();
    click(&mut s, 20, [250., 250.]);
    assert_eq!(steps(&s), before, "transparent paint inside an unfilled outline has nothing to remove");
    s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Foreground } }).unwrap();
    click(&mut s, 30, [250., 250.]);
    let filled = steps(&s);
    assert!(filled.0 == before.0 && filled.1 != before.1, "a click inside fills the outline");
    click(&mut s, 40, [250., 250.]);
    assert_eq!(steps(&s), filled, "the same fill again adds no undo step");
}

#[test]
fn select_all_takes_only_the_paths_the_move_tool_can_pick() {
    use layer_core::authored::vector::ItemId;
    let (mut s, handle) = vector_session();
    let mut hidden = straight(2, &[[100., 400.], [600., 400.]], false);
    hidden.hidden = true;
    let mut locked = straight(3, &[[100., 500.], [600., 500.]], false);
    locked.locked = true;
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [600., 300.]], false), hidden, locked]);
    invoke(&mut s, CommandId::SelectAll);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|v| (*v.items).clone()), Some([ItemId(1)].into()));
    invoke(&mut s, CommandId::ClearSelected);
    assert_eq!(path_ids(&s, handle), [ItemId(2), ItemId(3)], "Delete leaves hidden and locked paths");
    invoke(&mut s, CommandId::SelectAll);
    assert!(s.engine.document().working.vector_items.is_none() && s.engine.document().working.selection.is_some(), "with nothing to pick Select All selects pixels");
}

#[test]
fn delete_removes_paths_only_while_their_unlocked_layer_is_edited() {
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [300., 300.], [500., 300.], [700., 300.]], false), straight(2, &[[100., 500.], [700., 500.]], false)]);
    let id = occurrence_token(handle);
    let anchors = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().anchor_count();
    invoke(&mut s, CommandId::SelectAll);
    select(&mut s, rectangle([0., 0., 50., 50.]));
    assert_eq!(s.command(CommandId::ClearSelected).label.as_ref(), "Delete", "with pixels selected too, Delete names what it removes");
    layer(&mut s, LayerAction::Lock { id, value: true });
    let command = s.command(CommandId::ClearSelected);
    assert_eq!((command.enabled, command.disabled_reason.as_deref()), (false, Some(s.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).as_ref())));
    layer(&mut s, LayerAction::Lock { id, value: false });
    layer(&mut s, LayerAction::AddMask { id, replace: false });
    layer(&mut s, LayerAction::Select { id, mask: true });
    let _ = s.dispatch(UiAction::Invoke { command: CommandId::ClearSelected });
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(anchors(&s), 6, "editing the mask leaves the paths alone");
    layer(&mut s, LayerAction::Select { id, mask: false });
    invoke(&mut s, CommandId::EditPoints);
    let at = surface_at(&s, [300., 300.]);
    press(&mut s, 10, &[at, at]);
    invoke(&mut s, CommandId::Deselect);
    select(&mut s, rectangle([0., 0., 50., 50.]));
    let _ = s.dispatch(UiAction::Invoke { command: CommandId::ClearSelected });
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    assert_eq!(anchors(&s), 6, "an anchor of a path that is no longer selected stays");
}

#[test]
fn undo_waits_for_vector_lines_erasing_and_drags_to_finish() {
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [700., 300.]], false), straight(2, &[[100., 500.], [700., 500.]], false)]);
    let hold = |s: &mut UiSession<Recorder>, first: u64, points: &[[f64; 2]]| for (i, point) in points.iter().enumerate() {
        let mut e = event(s, first + i as u64, if i == 0 { PenPhase::Down } else { PenPhase::Move }, 0.5);
        e.surface_position = surface_at(s, *point);
        s.pen(e);
        s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
    };
    let release = |s: &mut UiSession<Recorder>, sequence: u64, point: [f64; 2]| {
        let mut e = event(s, sequence, PenPhase::Up, 0.5);
        e.surface_position = surface_at(s, point);
        s.pen(e);
        s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
    };
    let held = |s: &UiSession<Recorder>| !s.command(CommandId::Undo).enabled && !s.command(CommandId::SelectAll).enabled;
    hold(&mut s, 10, &[[200., 700.], [300., 720.], [400., 740.]]);
    assert!(held(&s), "drawing a path holds Undo");
    release(&mut s, 13, [500., 750.]);
    assert_eq!(path_ids(&s, handle).len(), 3);
    invoke(&mut s, CommandId::Eraser);
    invoke(&mut s, CommandId::EraseWholePaths);
    hold(&mut s, 20, &[[400., 250.], [400., 350.]]);
    assert!(held(&s), "erasing holds Undo");
    release(&mut s, 22, [400., 360.]);
    invoke(&mut s, CommandId::Move);
    hold(&mut s, 30, &[[100., 500.], [150., 520.]]);
    assert!(held(&s), "dragging paths holds Undo");
    release(&mut s, 32, [200., 540.]);
    let ids = path_ids(&s, handle);
    assert_eq!(ids.len(), 2, "the erased line is gone");
    for count in [2, 3, 2] {
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        s.frame(9_000_000_000, 9_000_000_000).unwrap();
        assert_eq!(path_ids(&s, handle).len(), count, "each gesture is one undo step in order");
    }
}

#[test]
fn split_needs_an_anchor_inside_a_path_and_reverse_takes_a_dot() {
    let (mut s, handle) = vector_session();
    let at = surface_at(&s, [800., 800.]);
    press(&mut s, 1, &[at, at]);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().anchor_count(), 1, "a tap draws a dot");
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::ReversePath);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().anchor_count(), 1, "a dot reverses to itself");
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [300., 300.], [500., 300.]], false)]);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::EditPoints);
    let click = |s: &mut UiSession<Recorder>, first: u64, point: [f64; 2]| { let p = surface_at(s, point); press(s, first, &[p, p]); };
    click(&mut s, 10, [100., 300.]);
    assert!(!s.command(CommandId::SplitPath).enabled, "an open end is not a place to split");
    click(&mut s, 20, [300., 300.]);
    assert!(s.command(CommandId::SplitPath).enabled);
    invoke(&mut s, CommandId::SplitPath);
    assert_eq!(path_ids(&s, handle).len(), 2);
    put_paths(&mut s, handle, vec![straight(1, &[[500., 500.]], false)]);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::EditPoints);
    let checkpoint = s.engine.checkpoint();
    click(&mut s, 30, [500., 500.]);
    click(&mut s, 32, [500., 500.]);
    assert_eq!(s.engine.checkpoint(), checkpoint, "a dot's anchor has no smooth form, so a double-click adds no undo step");
}

#[test]
fn a_setting_refusal_closes_when_another_brush_is_chosen() {
    let (mut s, _) = vector_session();
    s.select_brush(DefaultBrushPreset::Monoline as u32).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "size_jitter".into(), value: 0.4 }).unwrap();
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "size_jitter".into(), value: 0.5 }).unwrap();
    draw_line(&mut s, 1, 10);
    let notice = s.state.notice.clone().expect("a vector refusal");
    assert_eq!(notice.actions[0].id, NoticeActionId::ResetSetting);
    s.select_brush(DefaultBrushPreset::Monoline as u32).unwrap();
    assert_eq!(s.state.notice, None, "the refusal names G-Pen's setting");
    assert!(s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::ResetSetting) }).is_err());
    assert_eq!(s.tools.brush(DefaultBrushPreset::Monoline).shape.size_jitter, 0.4, "Monoline keeps its own setting");
}

#[test]
fn every_vector_tool_result_reopens_from_a_package_unchanged() {
    use layer_core::{EffectValue, authored::vector::PathItem};
    let (mut s, handle) = vector_session();
    let token = occurrence_token(handle);
    let drag = |s: &mut UiSession<Recorder>, first: u64, from: [f64; 2], to: [f64; 2]| { let points = [surface_at(s, from), surface_at(s, [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5]), surface_at(s, to)]; press(s, first, &points); };
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    let pen_line = |s: &UiSession<Recorder>| -> PathItem { s.engine.document().scene().vector_layer(handle).unwrap().paths().map(|(_, p)| p.clone()).find(|p| p.style.strokes().next().is_some_and(|s| s.profile.is_some())).expect("the pen line") };
    draw_line(&mut s, 1, 60);
    click(&mut s, 70, [900., 900.]);
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Both } } }).unwrap();
    for (i, at) in [[600., 100.], [700., 160.], [650., 240.]].into_iter().enumerate() { click(&mut s, 80 + i as u64 * 5, at); }
    click(&mut s, 99, [601., 101.]);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::MakeCompoundPath);
    invoke(&mut s, CommandId::ReleaseCompoundPath);
    for (shape, paint, first) in [(FigureShape::Rectangle, FigurePaint::Both, 100), (FigureShape::Ellipse, FigurePaint::Outline, 110), (FigureShape::Polygon, FigurePaint::Fill, 120), (FigureShape::Star, FigurePaint::Both, 130), (FigureShape::Line, FigurePaint::Outline, 140)] {
        s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape, paint } } }).unwrap();
        if shape == FigureShape::Rectangle { s.dispatch(UiAction::SetToolSetting { id: "figure_corner_radius".into(), value: 12. }).unwrap(); }
        drag(&mut s, first, [20. + (first - 100) as f64 * 9., 600.], [100. + (first - 100) as f64 * 9., 680.]);
    }
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::TransformFlipVertical);
    invoke(&mut s, CommandId::TransformRotateRight);
    for (key, value) in [("path_corners", EffectValue::Choice(0)), ("path_miter_limit", EffectValue::Number(9.)), ("path_overlaps", EffectValue::Choice(1))] {
        s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: token, key: key.into(), value } }).unwrap();
    }
    invoke(&mut s, CommandId::EditPoints);
    let line = pen_line(&s);
    click(&mut s, 300, line.geometry.subpaths()[0].anchor(line.geometry.subpaths()[0].len() / 2).point());
    invoke(&mut s, CommandId::SplitPath);
    invoke(&mut s, CommandId::Eraser);
    invoke(&mut s, CommandId::EraseTouchedPart);
    s.dispatch(UiAction::SetBrushSize { value: 4. }).unwrap();
    let [x, y] = { let line = pen_line(&s); line.geometry.subpaths()[0].segment(0).point(0.5) };
    let across = [surface_at(&s, [x - 10., y - 10.]), surface_at(&s, [x + 10., y + 10.])];
    press(&mut s, 400, &across);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::LassoFill }).unwrap();
    let ring: Vec<Point> = [[700., 700.], [820., 700.], [700., 820.], [820., 820.], [700., 700.]].windows(2).flat_map(|w| (0..10).map(move |i| { let u = f64::from(i) / 10.; [w[0][0] + (w[1][0] - w[0][0]) * u, w[0][1] + (w[1][1] - w[0][1]) * u] })).map(|p| surface_at(&s, p)).collect();
    press(&mut s, 500, &ring);
    for t in 0..20 { s.frame(9_000_000_000 + t * 16_000_000, 9_000_000_000 + t * 16_000_000).unwrap(); }
    let document = s.engine.document();
    let object = document.scene().vector_layer(handle).unwrap();
    object.validate().unwrap();
    let count = |test: fn(&PathItem) -> bool| object.paths().filter(|(_, p)| test(p)).count();
    assert!(count(|p| p.shape.is_some()) == 4 && count(|p| p.geometry.anchor_count() == 1) == 1 && count(|p| p.style.strokes().next().is_some_and(|s| s.profile.is_some())) >= 3 && count(|p| p.fill_rule == layer_core::authored::vector::FillRule::EvenOdd) >= 1,
        "shapes, a dot, cut pen lines and holes are all in the layer: {} paths", object.path_count());
    let reopened = package_roundtrip(document);
    assert_eq!(reopened.scene().order().iter().find_map(|h| reopened.scene().vector_layer(*h)), Some(object), "the paths reopen unchanged");
}

#[test]
fn every_tool_shows_the_refusal_cursor_on_a_locked_vector_layer() {
    let (mut s, handle) = vector_session();
    let prohibited = |s: &mut UiSession<Recorder>| { s.cursor_input(Some(event(s, 1, PenPhase::Hover, 0.))); s.canvas_cursor().is_some_and(|c| c.segments.iter().any(|segment| segment.marker == 6.)) };
    let tools: [fn(&mut UiSession<Recorder>); 5] = [
        |s| s.select_brush(DefaultBrushPreset::GPen as u32).unwrap(),
        |s| { invoke(s, CommandId::Eraser); },
        |s| s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Outline } }).unwrap(),
        |s| s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::LassoFill }).unwrap(),
        |s| s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } }).unwrap(),
    ];
    for (i, tool) in tools.iter().enumerate() {
        tool(&mut s);
        assert!(!prohibited(&mut s), "tool {i} works on the unlocked layer");
        layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: true });
        assert!(prohibited(&mut s), "tool {i} shows the refusal before contact on the locked layer");
        layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: false });
    }
}

#[test]
fn cancelled_vector_gestures_leave_the_paths_and_history_alone() {
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [400., 300.], [700., 300.]], false), straight(2, &[[100., 500.], [700., 500.]], false)]);
    let before = (s.engine.document().scene().vector_layer(handle).unwrap().clone(), s.engine.checkpoint());
    let state = |s: &UiSession<Recorder>| (s.engine.document().scene().vector_layer(handle).unwrap().clone(), s.engine.checkpoint());
    let cancel_after = |s: &mut UiSession<Recorder>, first: u64, points: &[[f64; 2]]| {
        for (i, point) in points.iter().enumerate() {
            let mut e = event(s, first + i as u64, if i == 0 { PenPhase::Down } else { PenPhase::Move }, 0.5);
            e.surface_position = surface_at(s, *point);
            s.pen(e);
            s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
        }
        let mut e = event(s, first + points.len() as u64, PenPhase::Cancel, 0.5);
        e.surface_position = surface_at(s, points[points.len() - 1]);
        s.pen(e);
        s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
    };
    cancel_after(&mut s, 10, &[[100., 700.], [300., 720.], [500., 760.]]);
    assert_eq!(state(&s), before, "a cancelled line adds no path");
    assert!(s.engine.backend().vector_preview.is_none());
    invoke(&mut s, CommandId::Eraser);
    invoke(&mut s, CommandId::EraseWholePaths);
    cancel_after(&mut s, 20, &[[400., 250.], [400., 550.]]);
    assert_eq!(state(&s), before, "a cancelled erase puts the paths back");
    invoke(&mut s, CommandId::Move);
    s.interaction.modifiers = Modifiers { alt: true, ..Modifiers::default() };
    cancel_after(&mut s, 30, &[[100., 500.], [200., 560.]]);
    s.interaction.modifiers = Modifiers::default();
    assert_eq!(state(&s), before, "a cancelled Alt-drag leaves no copies");
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::EditPoints);
    cancel_after(&mut s, 40, &[[400., 300.], [450., 350.], [500., 400.]]);
    assert_eq!(state(&s), before, "a cancelled anchor drag puts the anchor back");
    assert!(s.command(CommandId::Undo).enabled, "the canvas is idle again");
}

#[test]
fn join_paths_merges_ends_within_six_screen_pixels() {
    for (gap, anchors) in [(4.5, 3), (8., 4)] {
        let (mut s, handle) = vector_session();
        let px = s.document_per_surface_pixel();
        put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [400., 300.]], false), straight(2, &[[400. + gap * px, 300.], [700., 300.]], false)]);
        invoke(&mut s, CommandId::SelectAll);
        invoke(&mut s, CommandId::JoinPaths);
        assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().anchor_count(), anchors, "ends {gap} screen pixels apart");
    }
}

#[test]
fn polygon_and_star_options_hold_three_to_a_hundred_corners_and_inner_radii_up_to_one() {
    use layer_core::authored::vector::{LiveShape, ShapeKind};
    let (mut s, handle) = vector_session();
    let set = |s: &mut UiSession<Recorder>, id: &str, value: f32| s.dispatch(UiAction::SetToolSetting { id: id.into(), value }).is_ok();
    let drag = |s: &mut UiSession<Recorder>, first: u64| { let points = [surface_at(s, [100., 100.]), surface_at(s, [200., 160.]), surface_at(s, [300., 260.])]; press(s, first, &points); };
    let front = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().paths().next().unwrap().1.clone();
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Polygon, paint: FigurePaint::Outline } } }).unwrap();
    assert_eq!([2.4, 3., 100., 100.6, 0.].map(|v| set(&mut s, "figure_corners", v)), [false, true, true, false, false]);
    drag(&mut s, 10);
    assert!(matches!(front(&s).shape.as_deref(), Some(LiveShape { kind: ShapeKind::Polygon { corners: 100 }, .. })) && front(&s).geometry.anchor_count() == 100);
    assert!(set(&mut s, "figure_corners", 6.6));
    drag(&mut s, 20);
    assert!(matches!(front(&s).shape.as_deref(), Some(LiveShape { kind: ShapeKind::Polygon { corners: 7 }, .. })), "corners round to whole numbers");
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Star, paint: FigurePaint::Outline } } }).unwrap();
    assert_eq!([0., 0.04, 1., 1.01].map(|v| set(&mut s, "figure_inner_radius", v)), [false, false, true, false]);
    assert!(set(&mut s, "figure_points", 100.));
    drag(&mut s, 30);
    let star = front(&s);
    assert!(matches!(star.shape.as_deref(), Some(LiveShape { kind: ShapeKind::Star { points: 100, inner }, .. }) if *inner == 1.) && star.geometry.anchor_count() == 200);
    star.validate().unwrap();
}

#[test]
fn the_eraser_mode_and_outline_width_choice_come_back_with_the_preferences() {
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [700., 300.]], false), straight(2, &[[400., 100.], [400., 500.]], false)]);
    invoke(&mut s, CommandId::Eraser);
    invoke(&mut s, CommandId::EraseToIntersection);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::ScaleOutlineWidths);
    let saved = serde_json::to_string(&saved_settings(&s).expect("the choices are saved")).unwrap();
    let (mut restored, layer) = vector_session();
    restored.dispatch(UiAction::RestoreSettings { settings: Box::new(serde_json::from_str(&saved).unwrap()) }).unwrap();
    put_paths(&mut restored, layer, vec![straight(1, &[[100., 300.], [700., 300.]], false), straight(2, &[[400., 100.], [400., 500.]], false)]);
    invoke(&mut restored, CommandId::Eraser);
    assert!(restored.command(CommandId::EraseToIntersection).selected);
    restored.dispatch(UiAction::SetBrushSize { value: 6. }).unwrap();
    let across = [surface_at(&restored, [200., 290.]), surface_at(&restored, [200., 310.])];
    press(&mut restored, 10, &across);
    let spans: Vec<[f64; 4]> = restored.engine.document().scene().vector_layer(layer).unwrap().paths().map(|(_, p)| p.geometry.hull()).filter(|h| h[1] == h[3]).collect();
    assert!(spans.len() == 1 && (spans[0][0] - 400.).abs() < 0.5, "the restored eraser stops at the crossing: {spans:?}");
    invoke(&mut restored, CommandId::SelectAll);
    assert!(!restored.command(CommandId::ScaleOutlineWidths).selected, "outline widths stay as chosen");
}

#[test]
fn finishing_a_path_whose_layer_changed_explains_why_and_adds_nothing() {
    let (mut s, handle) = vector_session();
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    let draft = |s: &mut UiSession<Recorder>, first: u64| {
        s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } } }).unwrap();
        for (i, at) in [[100., 100.], [300., 120.], [200., 300.]].into_iter().enumerate() { click(s, first + i as u64 * 5, at); }
        assert!(s.command(CommandId::FinishPath).enabled);
    };
    let text = |s: &UiSession<Recorder>, id| s.localization().text(id).to_string();
    let checkpoint = s.engine.checkpoint();
    draft(&mut s, 10);
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: true });
    assert_eq!(s.dispatch(UiAction::Invoke { command: CommandId::FinishPath }).err(), Some(text(&s, MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED)));
    assert!(!s.command(CommandId::FinishPath).enabled, "the draft is gone");
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: false });
    draft(&mut s, 40);
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let paint = s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.dispatch(UiAction::Invoke { command: CommandId::FinishPath }).err(), Some(text(&s, MessageId::OBJECTS_NOTICE_STALE)));
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 0);
    draft(&mut s, 70);
    layer(&mut s, LayerAction::Lock { id: occurrence_token(paint), value: true });
    assert_eq!(s.dispatch(UiAction::Invoke { command: CommandId::FinishPath }).err(), Some(text(&s, MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED)));
    layer(&mut s, LayerAction::Lock { id: occurrence_token(paint), value: false });
    draft(&mut s, 100);
    layer(&mut s, LayerAction::Select { id: occurrence_token(handle), mask: false });
    assert_eq!(s.dispatch(UiAction::Invoke { command: CommandId::FinishPath }).err(), Some(text(&s, MessageId::OBJECTS_NOTICE_STALE)));
    assert!(s.engine.document().target_operations(s.engine.document().scene().source_target(paint).unwrap()).is_none_or(<[_]>::is_empty), "nothing is painted");
    let _ = checkpoint;
}

#[test]
fn vector_refusal_actions_check_the_layer_when_pressed() {
    let (mut s, handle) = vector_session();
    let text = |s: &UiSession<Recorder>, id| s.localization().text(id).to_string();
    draw_line(&mut s, 1, 20);
    let refusal = |s: &mut UiSession<Recorder>, first: u64| {
        s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
        draw_line(s, first, 10);
        s.state.notice.clone().expect("a vector refusal")
    };
    let notice = refusal(&mut s, 100);
    let count = s.engine.document().scene().children(None).len();
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: true });
    let change = s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::RasterizeLayer) }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    assert_eq!(s.state.notice.as_ref().map(|n| n.text.clone()), Some(text(&s, MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED)));
    assert!(s.engine.document().scene().vector_layer(handle).is_some(), "a locked layer keeps its paths");
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: false });
    let notice = refusal(&mut s, 200);
    let revision = s.engine.document().revision;
    layer(&mut s, LayerAction::Delete { id: occurrence_token(handle) });
    let deleted = s.engine.document().clone();
    let change = s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::NewPaintLayer) }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    let stale = s.state.notice.clone().expect("the stale action explains itself");
    assert_eq!(stale.text, text(&s, MessageId::OBJECTS_NOTICE_STALE));
    assert!(stale.actions.is_empty());
    for action in [NoticeActionId::UseVectorPen, NoticeActionId::NewPaintLayer, NoticeActionId::RasterizeLayer] {
        assert!(s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(action) }).is_err(), "the old notice is gone");
    }
    assert_ne!(deleted.revision, revision);
    assert_eq!(s.engine.document().scene().children(None).len(), count - 1, "no action reaches a deleted layer");
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Pencil as u32);
}

#[test]
fn path_tools_with_transparent_paint_ask_for_a_color() {
    let (mut s, handle) = vector_session();
    s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Transparent } }).unwrap();
    let ask = s.localization().text(MessageId::COMMANDS_PATHS_NEED_A_COLOR).to_string();
    for (first, tool) in [(10, LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Outline }), (20, LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline }), (30, LayerCanvasTool::LassoFill)] {
        s.layer_action(LayerAction::Tool { tool }).unwrap();
        let points = [surface_at(&s, [100., 100.]), surface_at(&s, [200., 160.]), surface_at(&s, [300., 260.])];
        press(&mut s, first, &points);
        assert_eq!(s.state.notice.as_ref().map(|n| (n.text.as_str(), n.actions.len())), Some((ask.as_str(), 0)), "{tool:?}");
        s.dismiss_notice();
    }
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 0);
}

#[test]
fn the_pens_eraser_end_erases_paths_and_hands_back_the_pen() {
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [700., 300.]], false)]);
    for (i, y) in [250., 300., 350.].into_iter().enumerate() {
        let phase = [PenPhase::Down, PenPhase::Move, PenPhase::Up][i];
        let mut e = PenEvent { tool: ToolKind::Eraser, ..event(&s, 10 + i as u64, phase, 0.8) };
        e.surface_position = surface_at(&s, [400., y]);
        s.pen(e);
        s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
    }
    s.pen(PenEvent { tool: ToolKind::Eraser, ..event(&s, 20, PenPhase::Hover, 0.) });
    s.pen(event(&s, 21, PenPhase::Hover, 0.));
    s.frame(300_000_000, 300_000_000).unwrap();
    assert_eq!(path_ids(&s, handle).len(), 2, "the eraser end cuts the path");
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32, "the pen tip draws paths again");
    draw_line(&mut s, 100, 20);
    assert_eq!(path_ids(&s, handle).len(), 3);
}

#[test]
fn soft_or_light_pens_name_hardness_or_flow_in_the_refusal_and_other_settings_still_draw() {
    for (setting, value) in [("hardness", 0.5), ("flow", 0.6)] {
        let (mut s, handle) = vector_session();
        s.dispatch(UiAction::SetToolSetting { id: setting.into(), value }).unwrap();
        assert_eq!(s.state.tool_set.subtools.iter().find(|item| item.action == UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).map(|item| item.disabled_reason.is_some()), Some(true), "{setting} dims G-Pen");
        draw_line(&mut s, 1, 10);
        let notice = s.state.notice.clone().expect("a vector refusal");
        let label = s.state.tool_settings.iter().find(|control| control.id == setting).unwrap().label.clone();
        let mut args = FluentArgs::new(); args.set("layer", s.engine.document().scene().occurrence(handle).unwrap().name.to_string()); args.set("setting", label.as_ref()); args.set("brush", "G-Pen");
        assert_eq!(notice.text, s.localization().format(MessageId::OBJECTS_VECTOR_REFUSAL_SETTING, &args));
        assert_eq!(notice.actions[0].id, NoticeActionId::ResetSetting);
        s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::ResetSetting) }).unwrap();
        draw_line(&mut s, 100, 20);
        assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 1, "after Reset G-Pen draws a path");
    }
    let (mut s, handle) = vector_session();
    let paint = s.engine.document().scene().children(None).iter().copied().find(|h| s.engine.document().scene().paint_source(*h).is_some()).unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    s.dispatch(UiAction::SetToolSetting { id: "spacing".into(), value: 0.5 }).unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(handle), mask: false });
    s.dispatch(UiAction::SetToolSetting { id: "hardness".into(), value: 1. }).unwrap();
    draw_line(&mut s, 1, 20);
    assert_eq!((s.engine.document().scene().vector_layer(handle).unwrap().path_count(), s.state.notice.as_ref()), (1, None), "wider spacing and harder edges still draw paths");
}

#[test]
fn fill_takes_the_innermost_path_around_the_click() {
    use layer_core::authored::vector::{ItemId, PaintEntry};
    let (mut s, handle) = vector_session();
    let square = |id: u64, [x0, y0, x1, y1]: [f64; 4]| straight(id, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], true);
    put_paths(&mut s, handle, vec![square(1, [100., 100., 600., 600.]), square(2, [200., 200., 300., 300.]), square(3, [400., 400., 700., 700.])]);
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } }).unwrap();
    let filled = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(handle).unwrap().paths()
        .filter(|(_, p)| p.style.entries.iter().any(|e| matches!(e, PaintEntry::Fill(_)))).map(|(_, p)| p.id).collect::<Vec<_>>();
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    click(&mut s, 10, [250., 250.]);
    assert_eq!(filled(&s), [ItemId(2)], "a click inside a nested path fills it, though the outer path is in front");
    click(&mut s, 20, [150., 150.]);
    assert_eq!(filled(&s), [ItemId(1), ItemId(2)]);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(5_000_000_000, 5_000_000_000).unwrap();
    click(&mut s, 30, [500., 500.]);
    assert_eq!(filled(&s), [ItemId(2), ItemId(3)], "where paths overlap, the smaller one around the click fills");
}

#[test]
fn paint_layers_under_a_clipped_vector_layer_stay_in_its_clip() {
    let mut s = session(Platform::Gtk);
    let base = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::NewVector);
    let vector = s.engine.document().working.occurrence.unwrap();
    let document = s.engine.document();
    let mut clipped = document.scene().occurrence(vector).unwrap().clone();
    clipped.attachment = layer_core::Attachment::Clip;
    s.engine.apply_edit(layer_core::Edit::Occurrence(layer_core::authored::RecordChange::replace(&document.artwork.occurrences, vector, Some(clipped)).unwrap())).unwrap();
    s.refresh_document();
    put_paths(&mut s, vector, vec![straight(1, &[[100., 100.], [400., 100.], [400., 400.], [100., 400.]], true)]);
    let placed = |s: &UiSession<Recorder>| {
        let doc = s.engine.document();
        let new = doc.working.occurrence.unwrap();
        let order = doc.scene().children(None).to_vec();
        let at = |h| order.iter().position(|o| *o == h).unwrap();
        assert!(doc.scene().paint_source(new).is_some());
        assert_eq!(at(new), at(vector) + 1, "the paint layer sits directly under the lines");
        assert_eq!(doc.clipping_base(new), Some(base), "and clips to the same base");
    };
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } }).unwrap();
    let outside = surface_at(&s, [600., 600.]);
    press(&mut s, 10, &[outside, outside]);
    placed(&s);
    layer(&mut s, LayerAction::Select { id: occurrence_token(vector), mask: false });
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Gradient { shape: layer_core::GradientShape::Linear } }).unwrap();
    let start = surface_at(&s, [100., 100.]);
    press(&mut s, 20, &[start, Point { x: start.x + 80., y: start.y }]);
    let notice = s.state.notice.clone().expect("a vector refusal");
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::NewPaintLayerBelow) }).unwrap();
    placed(&s);
}

#[test]
fn one_flatten_notice_names_hidden_layers_and_vector_layers() {
    let (mut s, vector) = vector_session();
    draw_line(&mut s, 1, 20);
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let hidden = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::Visibility { id: occurrence_token(hidden), value: false });
    invoke(&mut s, CommandId::FlattenImage);
    let notice = s.state.notice.clone().expect("Flatten asks first");
    let mut args = FluentArgs::new();
    args.set("count", 1);
    args.set("layers", s.engine.document().scene().occurrence(vector).unwrap().name.to_string());
    assert_eq!(notice.text, s.localization().format(MessageId::OBJECTS_FLATTEN_DISCARDS_HIDDEN_LAYERS_AND_PATHS, &args));
    assert_eq!(notice.actions.iter().map(|a| a.id).collect::<Vec<_>>(), [NoticeActionId::Flatten]);
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::Flatten) }).unwrap();
    assert_eq!(s.state.notice, None, "accepting flattens without asking again");
    assert!(s.engine.document().scene().vector_layer(vector).is_none());
}

#[test]
fn choosing_a_tool_that_cannot_change_a_vector_layer_explains_at_once() {
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    let paint = s.engine.document().working.occurrence.unwrap();
    invoke(&mut s, CommandId::Move);
    layer(&mut s, LayerAction::NewVector);
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Pencil as u32, "the Move tool keeps the brush");
    assert_eq!(s.state.notice, None, "making the layer explains nothing");
    let actions = |s: &UiSession<Recorder>| s.state.notice.as_ref().map(|n| n.actions.iter().map(|a| a.id).collect::<Vec<_>>());
    let brush = Some(vec![NoticeActionId::UseVectorPen, NoticeActionId::NewPaintLayer, NoticeActionId::RasterizeLayer]);
    layer(&mut s, LayerAction::Tool { tool: LayerCanvasTool::Paint });
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::Pencil as u32, "the chosen brush stays");
    assert_eq!(actions(&s), brush, "before any stroke");
    invoke(&mut s, CommandId::Pen);
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32);
    assert_eq!(s.state.notice, None, "a pen that draws paths clears the explanation");
    for (command, expected) in [(CommandId::Brush, brush.clone()), (CommandId::Sculpt, brush.clone()), (CommandId::Pen, None), (CommandId::Brush, brush.clone()),
        (CommandId::Gradient, Some(vec![NoticeActionId::NewPaintLayerBelow, NoticeActionId::RasterizeLayer])), (CommandId::Eraser, None),
        (CommandId::Airbrush, brush.clone()), (CommandId::Fill, None), (CommandId::Brush, brush), (CommandId::Figure, None), (CommandId::Move, None), (CommandId::EditPoints, None)] {
        invoke(&mut s, command);
        assert_eq!(actions(&s), expected, "{command:?}");
    }
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    invoke(&mut s, CommandId::Brush);
    assert_eq!(s.state.notice, None, "paint layers take every brush");
}

#[test]
fn choosing_pen_after_leaving_a_vector_layer_keeps_the_pen() {
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    let paint = s.engine.document().working.occurrence.unwrap();
    invoke(&mut s, CommandId::NewVectorLayer);
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32, "a new vector layer picks a pen that draws paths");
    invoke(&mut s, CommandId::Move);
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    invoke(&mut s, CommandId::Pen);
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32, "Pen gives the pen, not the brush held before the vector layer");
}

#[test]
fn filling_outside_paths_names_the_new_paint_layer_and_leads_back() {
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
    invoke(&mut s, CommandId::NewVectorLayer);
    let vector = s.engine.document().working.occurrence.unwrap();
    s.dispatch(UiAction::SetColor { rgba: [0.2, 0.6, 0.3, 1.] }).unwrap();
    layer(&mut s, LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } });
    let outside = surface_at(&s, [700., 600.]);
    press(&mut s, 20, &[outside, outside]);
    let filled = s.engine.document().working.occurrence.unwrap();
    assert!(s.engine.document().scene().paint_source(filled).is_some());
    assert_eq!(s.state.notice, None, "the notice waits for the fill to land");
    let request = s.renderer_mut().region_requests.last().unwrap().clone();
    let pixels = Arc::new(layer_core::SelectionPixels::bytes([8, 1], [0, 0, 8, 1], vec![0xffffffff; 2]).unwrap());
    s.renderer_mut().region_reply = Some(layer_render::RegionResult { tonal_sample: None, placement: layer_core::Affine::IDENTITY, request_id: request.request_id, pixels });
    s.frame(6_000_000_000, 6_000_000_000).unwrap();
    s.frame(6_016_000_000, 6_016_000_000).unwrap();
    let notice = s.state.notice.clone().expect("the fill says where it went");
    let name = s.engine.document().scene().occurrence(vector).unwrap().name.to_string();
    let mut args = FluentArgs::new(); args.set("layer", name.as_str());
    assert_eq!(notice.text, s.localization().format(MessageId::OBJECTS_VECTOR_FILLED_BELOW, &args));
    assert_eq!(notice.actions.iter().map(|a| (a.id, a.label.clone())).collect::<Vec<_>>(), [(NoticeActionId::BackToLayer, format!("Back to “{name}”"))]);
    invoke(&mut s, CommandId::Pen);
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::GPen as u32, "Pen stays the pen on the new layer");
    s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::BackToLayer) }).unwrap();
    assert_eq!(s.engine.document().working.occurrence, Some(vector));
    let again = surface_at(&s, [650., 650.]);
    layer(&mut s, LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Visible } });
    layer(&mut s, LayerAction::Select { id: occurrence_token(filled), mask: false });
    press(&mut s, 40, &[again, again]);
    assert_eq!(s.state.notice, None, "filling the paint layer adds no layer and says nothing");
}

#[test]
fn sketch_reaches_paths_from_select_and_scale_rotate() {
    let (mut s, handle) = vector_session();
    let paint = s.engine.document().scene().children(None).iter().copied().find(|h| s.engine.document().scene().paint_source(*h).is_some()).unwrap();
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }) }).unwrap();
    draw_line(&mut s, 1, 20);
    assert!(s.command(CommandId::ScaleRotate).enabled, "Scale/rotate works before any path is selected");
    invoke(&mut s, CommandId::Select);
    let rows = |s: &UiSession<Recorder>| s.state.tool_panels.tools.subtools.iter().map(|item| (item.label.to_string(), item.action.clone())).collect::<Vec<_>>();
    assert_eq!(rows(&s)[..2], [("Select paths".to_string(), UiAction::Invoke { command: CommandId::Move }), ("Edit points".to_string(), UiAction::Invoke { command: CommandId::EditPoints })]);
    s.dispatch(rows(&s)[0].1.clone()).unwrap();
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move);
    assert!(s.command(CommandId::ScaleRotate).selected, "Scale/rotate shows the path tool");
    let path = s.engine.document().scene().vector_layer(handle).unwrap().paths().next().unwrap().1.clone();
    let on = surface_at(&s, path.geometry.subpaths()[0].segment(0).point(0.3));
    press(&mut s, 100, &[on, on]);
    assert!(s.selected_vector_items().is_some(), "a tap picks the path");
    invoke(&mut s, CommandId::EditPoints);
    assert!(s.command(CommandId::ScaleRotate).selected);
    invoke(&mut s, CommandId::DrawingBrush);
    assert!(!s.command(CommandId::ScaleRotate).selected);
    invoke(&mut s, CommandId::Deselect);
    invoke(&mut s, CommandId::ScaleRotate);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move, "Scale/rotate picks paths");
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: true });
    let locked = s.command(CommandId::ScaleRotate);
    assert_eq!((locked.enabled, locked.disabled_reason.as_deref()), (false, Some(s.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).as_ref())));
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    invoke(&mut s, CommandId::Select);
    assert!(rows(&s).iter().all(|(_, action)| !matches!(action, UiAction::Invoke { command: CommandId::Move | CommandId::EditPoints })), "paint layers list only selection tools");
}

#[test]
fn dimmed_tools_rows_tiles_and_title_bar_items_say_why() {
    let (mut s, handle) = vector_session();
    let row = |s: &UiSession<Recorder>, preset: DefaultBrushPreset| s.state.tool_set.subtools.iter()
        .find(|item| item.action == UiAction::SelectBrush { id: preset as u32 }).map(|item| (item.enabled, item.disabled_reason.as_deref().map(str::to_string)));
    let unavailable = s.localization().text(MessageId::OBJECTS_VECTOR_UNAVAILABLE).to_string();
    assert_eq!(row(&s, DefaultBrushPreset::RoughGPen), Some((true, Some(unavailable.clone()))), "dimmed but still choosable");
    assert_eq!(row(&s, DefaultBrushPreset::GPen), Some((true, None)));
    let dimmed: Vec<_> = s.state.tool_panels.brush_sets.groups.iter().filter(|group| group.disabled_reason.is_some()).collect();
    assert!(!dimmed.is_empty() && dimmed.iter().all(|group| group.enabled && group.disabled_reason.as_deref() == Some(unavailable.as_str())), "dimmed brush groups say why and stay choosable");
    let rough = s.state.tool_set.subtools.iter().find(|item| item.action == UiAction::SelectBrush { id: DefaultBrushPreset::RoughGPen as u32 }).unwrap().action.clone();
    s.dispatch(rough).unwrap();
    assert_eq!(s.state.brush.preset, DefaultBrushPreset::RoughGPen as u32, "choosing a dimmed pen selects it");
    assert_eq!(s.state.notice.as_ref().map(|n| n.actions[0].id), Some(NoticeActionId::UseVectorPen), "and explains at once");
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "size_jitter".into(), value: 0.5 }).unwrap();
    let mut args = FluentArgs::new(); args.set("setting", s.state.tool_settings.iter().find(|setting| setting.id == "size_jitter").unwrap().label.as_ref());
    assert_eq!(row(&s, DefaultBrushPreset::GPen), Some((true, Some(s.localization().format(MessageId::OBJECTS_VECTOR_UNAVAILABLE_SETTING, &args)))), "a pen a setting stops names it");
    s.dispatch(UiAction::ResetToolSetting { id: "size_jitter".into() }).unwrap();
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: true });
    let locked = s.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string();
    let scale_rotate = ToolbarControl::Command { command: CommandId::ScaleRotate };
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Illustrator.layout(Platform::Gtk), ..WorkspaceState::default() }) }).unwrap();
    let tile = s.panel_view(Panel::Commands).unwrap().tiles.into_iter().find(|tile| tile.resolved_control == scale_rotate).unwrap();
    assert_eq!((tile.enabled, tile.disabled_reason.as_deref()), (false, Some(locked.as_str())));
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }) }).unwrap();
    let item = s.header_view().items.into_iter().find(|item| item.resolved_control == Some(scale_rotate)).unwrap();
    assert_eq!((item.enabled, item.disabled_reason.as_deref()), (false, Some(locked.as_str())));
    layer(&mut s, LayerAction::Lock { id: occurrence_token(handle), value: false });
    let item = s.header_view().items.into_iter().find(|item| item.resolved_control == Some(scale_rotate)).unwrap();
    assert_eq!((item.enabled, item.disabled_reason), (true, None), "enabled items keep their plain tooltip");
}

#[test]
fn pen_and_eraser_settings_on_vector_layers_show_only_what_shapes_paths() {
    let (mut s, handle) = vector_session();
    let ids = |s: &UiSession<Recorder>| s.state.tool_settings.iter().map(|setting| setting.id).collect::<Vec<_>>();
    let pen = ids(&s);
    assert!(pen.contains(&"size") && pen.contains(&"size_jitter") && pen.contains(&"hardness"), "{pen:?}");
    assert!(!pen.iter().any(|id| matches!(*id, "spacing" | "angle" | "rotation_jitter")), "{pen:?}");
    invoke(&mut s, CommandId::Eraser);
    assert_eq!(ids(&s), ["size"]);
    layer(&mut s, LayerAction::AddMask { id: occurrence_token(handle), replace: false });
    layer(&mut s, LayerAction::Select { id: occurrence_token(handle), mask: true });
    assert!(ids(&s).contains(&"spacing"), "a vector layer's mask takes the whole eraser");
    layer(&mut s, LayerAction::Select { id: occurrence_token(handle), mask: false });
    invoke(&mut s, CommandId::Pen);
    let paint = s.engine.document().scene().children(None).iter().copied().find(|h| s.engine.document().scene().paint_source(*h).is_some()).unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(paint), mask: false });
    assert!(ids(&s).contains(&"spacing"), "paint layers keep every pen setting");
}

#[test]
fn undo_while_drafting_a_path_removes_its_last_point_without_touching_history() {
    let (mut s, handle) = vector_session();
    draw_line(&mut s, 1, 20);
    let (drawn, checkpoint) = (s.engine.document().scene().vector_layer(handle).unwrap().clone(), s.engine.checkpoint());
    s.dispatch(UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } } }).unwrap();
    let click = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); };
    for (i, at) in [[100., 100.], [300., 120.], [200., 300.]].into_iter().enumerate() { click(&mut s, 100 + i as u64 * 5, at); }
    let mut e = event(&s, 120, PenPhase::Down, 0.5);
    e.surface_position = surface_at(&s, [400., 400.]);
    s.pen(e);
    s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
    assert!(!s.command(CommandId::Undo).enabled, "Undo waits while an anchor is being placed");
    s.pen(PenEvent { phase: PenPhase::Up, ..e });
    s.frame(e.timestamp_ns + 1, e.timestamp_ns + 1).unwrap();
    let anchors = |s: &mut UiSession<Recorder>| { let mut overlay = Vec::new(); s.append_layer_overlay(&mut overlay); overlay.iter().filter(|segment| segment.marker == 2.).count() };
    let placed = anchors(&mut s);
    for remaining in (0..4).rev() {
        assert!(s.command(CommandId::Undo).enabled);
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert_eq!((s.engine.checkpoint(), s.engine.document().scene().vector_layer(handle).unwrap()), (checkpoint, &drawn), "no history step is undone");
        assert_eq!(s.command(CommandId::FinishPath).enabled || s.command(CommandId::CancelPath).enabled, remaining > 0, "the draft goes with its last point");
        if remaining > 0 { assert!(anchors(&mut s) < placed); }
    }
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(9_000_000_000, 9_000_000_000).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().path_count(), 0, "with the draft gone Undo undoes the line");
}

#[test]
fn path_commands_say_what_to_select_when_they_cannot_apply() {
    let (mut s, handle) = vector_session();
    let triangle = straight(2, &[[100., 500.], [300., 500.], [200., 650.]], true);
    put_paths(&mut s, handle, vec![straight(1, &[[100., 300.], [300., 300.], [500., 300.]], false), triangle.clone(), straight(3, &[[600., 300.], [700., 300.]], false)]);
    let reason = |s: &UiSession<Recorder>, command| { let state = s.command(command); (!state.enabled).then(|| state.disabled_reason.unwrap_or_default().to_string()) };
    let text = |s: &UiSession<Recorder>, id| Some(s.localization().text(id).to_string());
    invoke(&mut s, CommandId::SelectAll);
    assert_eq!(reason(&s, CommandId::JoinPaths), text(&s, MessageId::COMMANDS_SELECT_OPEN_PATHS_TO_JOIN), "three paths are too many to join");
    assert_eq!(reason(&s, CommandId::ReleaseCompoundPath), text(&s, MessageId::COMMANDS_SELECT_A_COMPOUND_PATH_TO_RELEASE));
    assert_eq!(reason(&s, CommandId::SplitPath), text(&s, MessageId::COMMANDS_SELECT_AN_ANCHOR_INSIDE_A_PATH_TO_SPLIT));
    invoke(&mut s, CommandId::Deselect);
    let at = surface_at(&s, [200., 500.]);
    press(&mut s, 10, &[at, at]);
    assert_eq!(s.engine.document().working.vector_items.as_ref().map(|v| (*v.items).clone()), Some([triangle.id].into()));
    assert_eq!(reason(&s, CommandId::ClosePath), text(&s, MessageId::COMMANDS_SELECT_AN_OPEN_PATH_TO_CLOSE));
    assert_eq!(reason(&s, CommandId::MakeCompoundPath), text(&s, MessageId::COMMANDS_SELECT_PATHS_TO_MAKE_A_COMPOUND_PATH));
    assert_eq!(reason(&s, CommandId::JoinPaths), text(&s, MessageId::COMMANDS_SELECT_OPEN_PATHS_TO_JOIN), "a closed path has no ends to join");
    assert_eq!(s.dispatch(UiAction::Invoke { command: CommandId::ClosePath }).err(), text(&s, MessageId::COMMANDS_SELECT_AN_OPEN_PATH_TO_CLOSE));
    invoke(&mut s, CommandId::Deselect);
    assert_eq!(reason(&s, CommandId::JoinPaths), text(&s, MessageId::COMMANDS_SELECT_PATHS_ON_A_VECTOR_LAYER));
}

#[test]
fn reversing_flipping_or_turning_a_lone_dot_adds_no_undo_step() {
    let (mut s, handle) = vector_session();
    let at = surface_at(&s, [413.7, 288.1]);
    press(&mut s, 1, &[at, at]);
    assert_eq!(s.engine.document().scene().vector_layer(handle).unwrap().anchor_count(), 1);
    invoke(&mut s, CommandId::SelectAll);
    let checkpoint = s.engine.checkpoint();
    for command in [CommandId::ReversePath, CommandId::TransformFlipHorizontal, CommandId::TransformFlipVertical, CommandId::TransformRotateLeft, CommandId::TransformRotateRight] {
        assert!(s.command(command).enabled, "{command:?}");
        s.dispatch(UiAction::Invoke { command }).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint, "{command:?} leaves a lone dot as it was");
    }
    put_paths(&mut s, handle, vec![straight(1, &[[100., 100.], [300., 160.]], false)]);
    invoke(&mut s, CommandId::SelectAll);
    let checkpoint = s.engine.checkpoint();
    invoke(&mut s, CommandId::ReversePath);
    assert_ne!(s.engine.checkpoint(), checkpoint, "a line reverses in one step");
}

#[test]
fn double_clicking_a_selected_path_edits_the_points_of_the_whole_selection() {
    use layer_core::authored::vector::ItemId;
    let (mut s, handle) = vector_session();
    put_paths(&mut s, handle, vec![straight(1, &[[100., 250.], [500., 350.]], false), straight(2, &[[100., 550.], [500., 450.]], false), straight(3, &[[100., 700.], [500., 800.]], false)]);
    let selected = |s: &UiSession<Recorder>| s.engine.document().working.vector_items.as_ref().map(|v| (*v.items).clone());
    let double = |s: &mut UiSession<Recorder>, first: u64, at: [f64; 2]| { let p = surface_at(s, at); press(s, first, &[p, p]); press(s, first + 2, &[p, p]); };
    invoke(&mut s, CommandId::Move);
    let first = surface_at(&s, [300., 300.]);
    press(&mut s, 10, &[first, first]);
    s.interaction.modifiers = Modifiers { shift: true, ..Modifiers::default() };
    let second = surface_at(&s, [300., 500.]);
    press(&mut s, 20, &[second, second]);
    s.interaction.modifiers = Modifiers::default();
    assert_eq!(selected(&s), Some([ItemId(1), ItemId(2)].into()));
    double(&mut s, 30, [300., 300.]);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::EditPoints);
    assert_eq!(selected(&s), Some([ItemId(1), ItemId(2)].into()), "Edit Points opens for the whole selection");
    invoke(&mut s, CommandId::Move);
    double(&mut s, 40, [300., 750.]);
    assert_eq!((s.layer_interaction.tool, selected(&s)), (LayerCanvasTool::EditPoints, Some([ItemId(3)].into())), "a path outside the selection is edited alone");
}

fn crowded_hatching(extent: [f64; 2], count: u64) -> Vec<layer_core::authored::vector::Record> {
    use layer_core::authored::vector::*;
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed >> 11) as f64 / (1u64 << 53) as f64 };
    (0..count).map(|i| {
        let start = [next() * extent[0], next() * extent[1]];
        let (angle, length, bend) = (0.4 + 0.5 * next(), 40. + 180. * next(), 40. * next() - 20.);
        let anchors: Vec<Anchor> = (0..4).map(|k| { let t = f64::from(k) / 3.; Anchor::corner([start[0] + angle.cos() * length * t - angle.sin() * bend * t * (1. - t) * 4., start[1] + angle.sin() * length * t + angle.cos() * bend * t * (1. - t) * 4.]) }).collect();
        let mut stroke = Stroke::new(Paint::color(layer_core::color::RgbColor::BLACK), 2. + 4. * next());
        (stroke.cap, stroke.join) = (Cap::Round, Join::Round);
        stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.1), WidthKnot::new(1, 0.5, 1., 1.), WidthKnot::new(2, 1., 0.05, 0.05)]).unwrap()));
        Record::Path(PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
    }).collect()
}

#[test]
#[ignore = "crowded vector gesture costs"]
fn crowded_vector_gesture_costs() {
    use layer_core::authored::vector::*;
    const SAMPLE_NS: u64 = 5_000_000;
    const FRAME_NS: u64 = 16_666_667;
    let crowded = |selected: usize| {
        let document = Document::new(layer_core::PortableId::random(), 6000, 4000, layer_core::DocumentNames { paint: "Base".into(), paper: "Paper".into() });
        let mut s = UiSession::new(Recorder::default(), document, [1920, 1200], Platform::Android).unwrap();
        s.set_viewport([1920., 1200.], [1920, 1200]).unwrap();
        s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
        layer(&mut s, LayerAction::NewVector);
        let layer = s.engine.document().working.occurrence.unwrap();
        let edit = s.engine.document().vector_object_edit(layer, VectorObject::from_records(crowded_hatching([6000., 4000.], 20_000))).unwrap();
        s.engine.apply_edit(edit).unwrap();
        s.frame(1, 1).unwrap();
        invoke(&mut s, CommandId::Move);
        let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let mut ids: Vec<(f64, ItemId)> = object.paths().map(|(_, p)| { let [x0, y0, x1, y1] = p.bounds(); ((x0 + x1 - 6000.).hypot(y0 + y1 - 4000.), p.id) }).collect();
        ids.sort_by(|a, b| a.0.total_cmp(&b.0));
        if selected > 0 { s.select_vector_items(layer, ids.iter().take(selected).map(|(_, id)| *id).collect()).unwrap(); }
        s.frame(2, 2).unwrap();
        (s, layer)
    };
    struct Costs { input: f64, frames: f64, publish: f64, inputs: usize, count: usize, published: usize, release: f64 }
    type InputStep = Box<dyn FnMut(&mut UiSession<Recorder>, usize, u64)>;
    let run = |s: &mut UiSession<Recorder>, inputs: usize, mut input: InputStep| {
        let mut costs = Costs { input: 0., frames: 0., publish: 0., inputs: 0, count: 0, published: 0, release: 0. };
        let mut model = s.workspace_model_revision();
        let mut now = 10_000_000_000u64;
        let mut next_frame = now;
        for i in 0..inputs {
            let start = std::time::Instant::now();
            input(s, i, now);
            let elapsed = start.elapsed().as_secs_f64() * 1e3;
            if i + 1 == inputs { costs.release += elapsed; } else { costs.input += elapsed; costs.inputs += 1; }
            now += SAMPLE_NS;
            if now >= next_frame || i + 1 == inputs {
                next_frame += FRAME_NS;
                let start = std::time::Instant::now();
                s.frame(now, now + FRAME_NS).unwrap();
                let mut overlay = Vec::new();
                s.append_layer_overlay(&mut overlay);
                let elapsed = start.elapsed().as_secs_f64() * 1e3;
                let start = std::time::Instant::now();
                if s.workspace_model_revision() != model {
                    model = s.workspace_model_revision();
                    costs.published += 1;
                    for menu in ApplicationMenu::ALL { std::hint::black_box(s.application_menu(menu)); }
                    std::hint::black_box(serde_json::to_vec(s.state()).unwrap());
                }
                let publish = start.elapsed().as_secs_f64() * 1e3;
                if i + 1 == inputs { costs.release += elapsed + publish; } else { costs.frames += elapsed; costs.publish += publish; costs.count += 1; }
            }
        }
        costs
    };
    let pen = |s: &mut UiSession<Recorder>, phase: PenPhase, at: [f64; 2], now: u64| {
        let mut e = event(s, now / SAMPLE_NS, phase, 0.5);
        e.timestamp_ns = now;
        e.surface_position = surface_at(s, at);
        s.pen(e);
    };
    let drag = move |from: [f64; 2], to: [f64; 2], samples: usize| Box::new(move |s: &mut UiSession<Recorder>, i: usize, now: u64| {
        let t = i as f64 / (samples - 1) as f64;
        let phase = if i == 0 { PenPhase::Down } else if i + 1 == samples { PenPhase::Up } else { PenPhase::Move };
        pen(s, phase, [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t + 40. * (t * 9.).sin()], now);
    }) as Box<dyn FnMut(&mut UiSession<Recorder>, usize, u64)>;
    let report = |name: &str, c: Costs| eprintln!("{name:<34} per input {:7.3} ms ({:4} inputs) | per frame: frame {:7.3} ms, publish {:7.3} ms, total CPU {:7.3} ms ({:3} frames, {:3} published) | release {:8.2} ms",
        c.input / c.inputs.max(1) as f64, c.inputs, c.frames / c.count.max(1) as f64, c.publish / c.count.max(1) as f64, (c.input + c.frames + c.publish) / c.count.max(1) as f64, c.count, c.published, c.release);
    const SAMPLES: usize = 200;
    for selected in [100usize, 5000] {
        for rotate in [false, true] {
            let (mut s, _) = crowded(selected);
            let frame = s.vector_frame().unwrap();
            let (from, to) = if rotate {
                let [x, y] = frame.unit.map([0.5, 0.]);
                ([x, y - f64::from(s.ruler_reach()) * 2.5], [x + 600., y + 300.])
            } else { let corner = frame.unit.map([1., 1.]); (corner, [corner[0] + 400., corner[1] + 250.]) };
            let costs = run(&mut s, SAMPLES, drag(from, to, SAMPLES));
            report(&format!("{} {selected} paths", if rotate { "rotate" } else { "scale" }), costs);
        }
        let (mut s, layer) = crowded(selected);
        let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let items = s.selected_vector_items().unwrap().items.clone();
        let path = object.selected(&items).into_iter().next().unwrap();
        let start = path.geometry.subpaths()[0].anchor(1).point();
        let costs = run(&mut s, SAMPLES, drag(start, [start[0] + 300., start[1] + 200.], SAMPLES));
        report(&format!("move {selected} paths"), costs);
        let (mut s, layer) = crowded(selected);
        let token = occurrence_token(layer);
        let costs = run(&mut s, 120, Box::new(move |s: &mut UiSession<Recorder>, i: usize, _| {
            let phase = if i == 0 { ContactPhase::Down } else if i + 1 == 120 { ContactPhase::Up } else { ContactPhase::Move };
            s.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Set { layer: token, key: "path_width".into(), value: layer_core::EffectValue::Number(2. + i as f32 * 0.1) }) } }).unwrap();
        }));
        report(&format!("outline width scrub {selected} paths"), costs);
        let (mut s, layer) = crowded(selected);
        let token = occurrence_token(layer);
        let costs = run(&mut s, 120, Box::new(move |s: &mut UiSession<Recorder>, i: usize, now: u64| {
            let phase = if i == 0 { ContactPhase::Down } else if i + 1 == 120 { ContactPhase::Up } else { ContactPhase::Move };
            s.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Number { layer: token, key: "path_width".into(), operation: crate::NumericOperation::Value { value: 4. + 3. * (i as f64 * 0.005 * std::f64::consts::PI).sin() } }) } }).unwrap();
            s.frame(now, now + FRAME_NS).unwrap();
        }));
        report(&format!("width scrub, frame per update, {selected}"), costs);
        let (mut s, _) = crowded(selected);
        let costs = run(&mut s, 120, Box::new(move |s: &mut UiSession<Recorder>, i: usize, _| {
            let change = s.gesture([960., 600.], [960. + if i.is_multiple_of(2) { 3. } else { -3. }, 600.], 1., 0.).unwrap();
            std::hint::black_box(change);
        }));
        report(&format!("pan, {selected} paths selected"), costs);
    }
    let (mut s, layer) = crowded(1);
    invoke(&mut s, CommandId::EditPoints);
    let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let path = object.selected(&s.selected_vector_items().unwrap().items).into_iter().next().unwrap();
    let anchor = path.geometry.subpaths()[0].anchor(1).point();
    let [ox, oy] = s.engine.document().scene().occurrence_offset64(layer);
    let anchor = [anchor[0] + ox, anchor[1] + oy];
    let costs = run(&mut s, SAMPLES, drag(anchor, [anchor[0] + 300., anchor[1] + 200.], SAMPLES));
    report("edit points anchor drag, 1 path", costs);
    for mode in [CommandId::EraseTouchedPart, CommandId::EraseWholePaths, CommandId::EraseToIntersection] {
        let (mut s, _) = crowded(0);
        invoke(&mut s, CommandId::Eraser);
        invoke(&mut s, mode);
        s.dispatch(UiAction::SetBrushSize { value: 64. }).unwrap();
        let costs = run(&mut s, SAMPLES, drag([2000., 1500.], [4000., 2500.], SAMPLES));
        report(&format!("eraser {mode:?}"), costs);
    }
}

fn hatched_session(count: u64) -> (UiSession<Recorder>, OccurrenceHandle) {
    let (mut s, layer) = vector_session();
    let object = layer_core::authored::vector::VectorObject::from_records(crowded_hatching([1000., 1000.], count));
    let edit = s.engine.document().vector_object_edit(layer, object).unwrap();
    s.layer_edit(edit).unwrap();
    s.frame(1, 1).unwrap();
    (s, layer)
}
fn drag_in_bursts(s: &mut UiSession<Recorder>, first: u64, points: &[[f64; 2]], per_frame: usize) {
    for (i, point) in points.iter().enumerate() {
        let phase = if i == 0 { PenPhase::Down } else if i + 1 == points.len() { PenPhase::Up } else { PenPhase::Move };
        let mut e = event(s, first + i as u64, phase, 0.5);
        e.surface_position = surface_at(s, *point);
        let revision = s.engine.document().revision;
        s.pen(e);
        if phase == PenPhase::Move { assert_eq!(s.engine.document().revision, revision, "a sample waits for the next frame"); }
        if (i + 1) % per_frame == 0 || phase == PenPhase::Up {
            let revision = s.engine.document().revision;
            s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
            assert!(s.engine.document().revision <= revision + 1, "a frame previews once");
        }
    }
}

#[test]
fn sample_bursts_preview_once_per_frame_and_commit_what_frame_by_frame_previews_commit() {
    use layer_core::authored::vector::*;
    let (mut s, layer) = hatched_session(400);
    let object = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let original = object(&s);
    let line = |from: [f64; 2], to: [f64; 2]| (0..=24).map(|i| { let t = f64::from(i) / 24.; [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t + 30. * (t * 7.).sin()] }).collect::<Vec<_>>();
    let committed = |s: &mut UiSession<Recorder>, first: u64, points: &[[f64; 2]]| -> [VectorObject; 2] {
        [1, 5].map(|per_frame| {
            drag_in_bursts(s, first + per_frame as u64 * 100, points, per_frame);
            let result = object(s);
            s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
            s.frame(first * 1_000_000_000, first * 1_000_000_000).unwrap();
            assert_eq!(object(s), original, "each gesture is one undo step");
            result
        })
    };
    invoke(&mut s, CommandId::Move);
    let chosen: std::collections::BTreeSet<ItemId> = original.paths().filter(|(_, p)| { let [x0, y0, x1, y1] = p.bounds(); x0 > 300. && y0 > 300. && x1 < 700. && y1 < 700. }).map(|(_, p)| p.id).collect();
    assert!(chosen.len() > 20);
    s.select_vector_items(layer, chosen.clone()).unwrap();
    s.frame(2, 2).unwrap();
    let frame = s.vector_frame().unwrap();
    let corner = frame.unit.map([1., 1.]);
    let [scaled, scaled_in_bursts] = committed(&mut s, 1_000, &line(corner, [corner[0] + 150., corner[1] + 90.]));
    assert_ne!(scaled, original);
    assert_eq!(scaled, scaled_in_bursts, "scaling commits the same paths however samples share frames");
    let [x, y] = frame.unit.map([0.5, 0.]);
    let handle = [x, y - f64::from(s.ruler_reach()) * 2.5];
    let [turned, turned_in_bursts] = committed(&mut s, 2_000, &line(handle, [handle[0] + 300., handle[1] + 200.]));
    assert_ne!(turned, original);
    assert_eq!(turned, turned_in_bursts, "rotating commits the same paths however samples share frames");
    let anchor = original.selected(&chosen)[0].geometry.subpaths()[0].anchor(1).point();
    s.select_vector_items(layer, [original.selected(&chosen)[0].id].into()).unwrap();
    invoke(&mut s, CommandId::EditPoints);
    let [edited, edited_in_bursts] = committed(&mut s, 3_000, &line(anchor, [anchor[0] + 60., anchor[1] + 40.]));
    assert_ne!(edited, original);
    assert_eq!(edited, edited_in_bursts, "an anchor drag commits the same path however samples share frames");
    invoke(&mut s, CommandId::Eraser);
    invoke(&mut s, CommandId::EraseToIntersection);
    s.dispatch(UiAction::SetBrushSize { value: 12. }).unwrap();
    let shapes = |o: &VectorObject| o.paths().map(|(_, p)| (p.geometry.clone(), p.style.clone())).collect::<Vec<_>>();
    let [erased, erased_in_bursts] = committed(&mut s, 4_000, &line([250., 250.], [750., 700.]));
    assert!(erased.path_count() != original.path_count() || shapes(&erased) != shapes(&original));
    assert_eq!(shapes(&erased), shapes(&erased_in_bursts), "erasing cuts along every sample however samples share frames");
    s.select_vector_items(layer, chosen.clone()).unwrap();
    s.frame(3, 3).unwrap();
    let token = occurrence_token(layer);
    let scrub = |s: &mut UiSession<Recorder>, every: usize| {
        for i in 0..12 {
            let phase = if i == 0 { ContactPhase::Down } else if i == 11 { ContactPhase::Up } else { ContactPhase::Move };
            let revision = s.engine.document().revision;
            s.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Set { layer: token, key: "path_width".into(), value: layer_core::EffectValue::Number(2. + i as f32) }) } }).unwrap();
            if phase == ContactPhase::Move { assert_eq!(s.engine.document().revision, revision, "a width update waits for the next frame"); }
            if i % every == 0 {
                let revision = s.engine.document().revision;
                s.frame(5_000 + i as u64, 5_000 + i as u64).unwrap();
                assert!(s.engine.document().revision <= revision + 1, "a frame previews once");
            }
        }
        let result = object(s);
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        s.frame(6_000, 6_000).unwrap();
        result
    };
    let [widened, widened_in_bursts] = [1, 4].map(|every| scrub(&mut s, every));
    assert!(widened.selected(&chosen).iter().all(|p| p.style.strokes().last().unwrap().width == 13.));
    assert_eq!(widened, widened_in_bursts, "a width scrub commits its last value however updates share frames");
}

#[test]
fn moving_or_turning_a_large_path_selection_republishes_nothing_until_release() {
    let (mut s, layer) = hatched_session(2_000);
    invoke(&mut s, CommandId::SelectAll);
    s.frame(2, 2).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::ScaleOutlineWidths }).unwrap();
    assert!(s.state.settings.keep_outline_widths);
    for (first, scale) in [(100, false), (200, true)] {
        let object = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        let from = if scale { s.vector_frame().unwrap().unit.map([1., 1.]) } else {
            object.paths().map(|(_, p)| p.geometry.subpaths()[0].anchor(0).point()).find(|p| p.iter().all(|v| (300. ..700.).contains(v))).unwrap()
        };
        let to = [from[0] + 80., from[1] + 50.];
        let revision = s.engine.document().revision;
        let mut published = None;
        for i in 0..=20u64 {
            let phase = if i == 0 { PenPhase::Down } else if i == 20 { PenPhase::Up } else { PenPhase::Move };
            let t = i as f64 / 20.;
            let mut e = event(&s, first + i, phase, 0.5);
            e.surface_position = surface_at(&s, [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]);
            s.pen(e);
            let change = s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
            if phase == PenPhase::Move {
                assert_eq!(change.regions & (regions::DOCUMENT | regions::COMMANDS | regions::LAYOUT), 0, "sample {i} of the drag from {from:?}");
                assert_eq!(*published.get_or_insert(s.workspace_model_revision()), s.workspace_model_revision(), "the menus, layers and Properties stay published as they were");
            }
        }
        assert_ne!(s.engine.document().revision, revision, "the drag commits");
        assert_ne!(published, Some(s.workspace_model_revision()), "release publishes the result");
    }
}

#[test]
fn scaling_and_turning_paths_preview_through_their_motion_and_commit_at_release() {
    let (mut s, layer) = hatched_session(400);
    invoke(&mut s, CommandId::Move);
    let original = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let chosen: std::collections::BTreeSet<_> = original.paths().filter(|(_, p)| { let [x0, y0, x1, y1] = p.bounds(); x0 > 300. && y0 > 300. && x1 < 700. && y1 < 700. }).map(|(_, p)| p.id).collect();
    let items = s.select_vector_items(layer, chosen.clone()).unwrap();
    s.frame(2, 2).unwrap();
    for (first, rotate) in [(100, false), (200, true)] {
        let frame = s.vector_frame().unwrap();
        let from = if rotate { let [x, y] = frame.unit.map([0.5, 0.]); [x, y - f64::from(s.ruler_reach()) * 2.5] } else { frame.unit.map([1., 1.]) };
        let to = [from[0] + 140., from[1] + 90.];
        let revision = s.engine.document().revision;
        let mut shown = None;
        for i in 0..=12u64 {
            let phase = if i == 0 { PenPhase::Down } else if i == 12 { PenPhase::Up } else { PenPhase::Move };
            let t = (i.min(11) as f64) / 11.;
            let mut e = event(&s, first + i, phase, 0.5);
            e.surface_position = surface_at(&s, [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]);
            s.pen(e);
            s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
            if phase == PenPhase::Up { break; }
            assert_eq!(s.engine.document().revision, revision, "the paths stay as they were until release");
            let motion = s.engine.backend().vector_motion.clone().expect("the selection moves as one image");
            assert!(motion.owner == layer && Arc::ptr_eq(&motion.items, &items));
            shown = Some(motion.transform);
        }
        let shown = shown.unwrap();
        assert_ne!(shown, layer_core::authored::Affine64([1., 0., 0., 1., 0., 0.]), "the frame's affine moves the image");
        assert!(s.engine.backend().vector_motion.is_none(), "release ends the motion");
        let committed = s.engine.document().scene().vector_layer(layer).unwrap().clone();
        assert_eq!(committed, original.transform_items(&chosen, shown, true).unwrap(), "release derives the paths the image showed");
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        s.frame(first * 1_000_000_000, first * 1_000_000_000).unwrap();
        assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &original, "one undo step");
    }
}

#[test]
fn path_scrubs_and_anchor_drags_preview_through_motion_paths_until_release() {
    use layer_core::authored::vector::{ItemId, PathItem};
    let (mut s, layer) = hatched_session(400);
    invoke(&mut s, CommandId::Move);
    let original = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let chosen: std::collections::BTreeSet<ItemId> = original.paths().filter(|(_, p)| { let [x0, y0, x1, y1] = p.bounds(); x0 > 300. && y0 > 300. && x1 < 700. && y1 < 700. }).map(|(_, p)| p.id).collect();
    let items = s.select_vector_items(layer, chosen.clone()).unwrap();
    s.frame(2, 2).unwrap();
    let token = occurrence_token(layer);
    let width = |path: &PathItem| path.style.strokes().last().unwrap().width;
    let revision = s.engine.document().revision;
    let updates = s.engine.backend().motion_path_updates;
    let mut frames = 0;
    for i in 0..=12 {
        let phase = if i == 0 { ContactPhase::Down } else if i == 12 { ContactPhase::Up } else { ContactPhase::Move };
        s.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Set { layer: token, key: "path_width".into(), value: layer_core::EffectValue::Number(if i == 12 { 9. } else { 2. + i as f32 * 0.5 }) }) } }).unwrap();
        if phase == ContactPhase::Up { break; }
        assert_eq!(s.engine.document().revision, revision, "update {i} leaves the layer as it was");
        if i % 4 == 3 {
            s.frame(10 + i as u64, 10 + i as u64).unwrap();
            frames += 1;
            assert_eq!(s.engine.document().revision, revision, "frame {i} previews without editing the layer");
            let shown = s.engine.backend().vector_motion_paths.clone().expect("the selection draws its preview paths");
            assert!(shown.iter().map(|p| p.id).eq(original.selected(&chosen).iter().map(|p| p.id)) && shown.iter().all(|p| width(p) == 2. + i as f64 * 0.5));
            assert!(s.engine.backend().vector_motion.as_ref().is_some_and(|motion| motion.owner == layer && Arc::ptr_eq(&motion.items, &items)));
        }
    }
    assert!(s.engine.backend().motion_path_updates - updates <= frames + 1, "one preview for the press and one per frame");
    assert!(s.engine.backend().vector_motion.is_none() && s.engine.backend().vector_motion_paths.is_none(), "release ends the preview");
    let scrubbed = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    assert!(scrubbed.selected(&chosen).iter().all(|p| width(p) == 9.));
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(100, 100).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &original, "the scrub is one undo step");
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: token, key: "path_width".into(), value: layer_core::EffectValue::Number(9.) } }).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &scrubbed, "a scrub commits what setting its last value commits");
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(200, 200).unwrap();
    let path = original.selected(&chosen)[0].clone();
    s.select_vector_items(layer, [path.id].into()).unwrap();
    invoke(&mut s, CommandId::EditPoints);
    let anchor = path.geometry.subpaths()[0].anchor(1).point();
    let revision = s.engine.document().revision;
    let updates = s.engine.backend().motion_path_updates;
    let mut frames = 0;
    for i in 0..=20u64 {
        let phase = if i == 0 { PenPhase::Down } else if i == 20 { PenPhase::Up } else { PenPhase::Move };
        let t = i.min(19) as f64 / 19.;
        let mut e = event(&s, 300 + i, phase, 0.5);
        e.surface_position = surface_at(&s, [anchor[0] + 60. * t, anchor[1] + 40. * t]);
        s.pen(e);
        if phase == PenPhase::Up { break; }
        assert_eq!(s.engine.document().revision, revision, "sample {i} leaves the layer as it was");
        if i % 5 == 4 || i == 19 {
            s.frame(e.timestamp_ns, e.timestamp_ns).unwrap();
            frames += 1;
            assert_eq!(s.engine.document().revision, revision, "frame {i} previews without editing the layer");
        }
    }
    let shown = s.engine.backend().vector_motion_paths.clone();
    assert!(s.engine.backend().motion_path_updates - updates <= frames, "at most one preview per frame");
    let committed = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    assert_ne!(committed, original);
    assert!(s.engine.backend().vector_motion.is_none() && s.engine.backend().vector_motion_paths.is_none(), "release ends the preview");
    assert!(shown.is_none(), "release clears the preview in the same frame");
    let moved = committed.selected(&[path.id].into());
    assert!((moved[0].geometry.subpaths()[0].anchor(1).point()[0] - anchor[0] - 60.).abs() < 0.5, "the anchor lands where it was dragged");
}

#[test]
fn scrub_values_rebuild_the_preview_once_per_presentation_target() {
    use crate::NumericOperation;
    let (mut s, layer) = hatched_session(400);
    invoke(&mut s, CommandId::Move);
    let original = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    let chosen: std::collections::BTreeSet<_> = original.paths().filter(|(_, p)| { let [x0, y0, x1, y1] = p.bounds(); x0 > 200. && y0 > 200. && x1 < 800. && y1 < 800. }).map(|(_, p)| p.id).collect();
    s.select_vector_items(layer, chosen.clone()).unwrap();
    s.frame(2, 2).unwrap();
    let token = occurrence_token(layer);
    let number = |value: f64| EffectAction::Number { layer: token, key: "path_width".into(), operation: NumericOperation::Value { value } };
    let gesture = |phase, value| UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(number(value)) } };
    let width = |i: u64| 4. + 3. * (i as f64 * 0.005 * std::f64::consts::PI).sin();
    let revision = s.engine.document().revision;
    const VSYNC_120: u64 = 8_333_333;
    let start = 1_000_000_000u64;
    let updates = s.engine.backend().motion_path_updates;
    s.dispatch(gesture(ContactPhase::Down, 4.)).unwrap();
    for k in 1..=60u64 {
        s.dispatch(gesture(ContactPhase::Move, width(k))).unwrap();
        let now = start + k * VSYNC_120 + [2_900_000, 200_000, 2_700_000, 400_000][k as usize % 4];
        s.frame(now, start + (k + 1) * VSYNC_120).unwrap();
        assert_eq!(s.engine.document().revision, revision, "frame {k} leaves the layer as it was");
    }
    assert_eq!(s.engine.backend().motion_path_updates - updates, 61, "at 120 Hz the press and every frame build, however callbacks jitter");
    s.dispatch(gesture(ContactPhase::Up, width(60))).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(start * 2, start * 2).unwrap();
    const VSYNC_60: u64 = 16_666_667;
    let start = start * 3;
    let revision = s.engine.document().revision;
    let updates = s.engine.backend().motion_path_updates;
    let mut targets = std::collections::BTreeSet::new();
    s.dispatch(gesture(ContactPhase::Down, 4.)).unwrap();
    for i in 1..=120u64 {
        let now = start + i * 5_000_000;
        let update = if i % 7 == 0 { UiAction::Effect { action: EffectAction::Set { layer: token, key: "path_width".into(), value: layer_core::EffectValue::Number(width(i) as f32) } } } else { gesture(ContactPhase::Move, width(i)) };
        s.dispatch(update).unwrap();
        let target = (now / VSYNC_60 + 1) * VSYNC_60;
        targets.insert(target);
        s.frame(now, target).unwrap();
        assert_eq!(s.engine.document().revision, revision, "update {i} leaves the layer as it was");
    }
    assert_eq!(s.engine.backend().motion_path_updates - updates, targets.len() + 1, "frames after each 5 ms update build once per vsync target");
    s.dispatch(gesture(ContactPhase::Up, width(120))).unwrap();
    assert!(s.engine.backend().vector_motion_paths.is_none());
    let scrubbed = s.engine.document().scene().vector_layer(layer).unwrap().clone();
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    s.frame(start * 2, start * 2).unwrap();
    s.dispatch(UiAction::Effect { action: number(width(120)) }).unwrap();
    assert_eq!(s.engine.document().scene().vector_layer(layer).unwrap(), &scrubbed, "the scrub commits its last value once");
}

#[test]
fn vector_fill_honors_the_selected_source_on_every_platform() {
    for platform in Platform::ALL {
        for source in [crate::RegionSource::Visible, crate::RegionSource::Editing, crate::RegionSource::Reference] {
            let (mut s, vector) = vector_session_on(platform);
            put_paths(&mut s, vector, vec![straight(1, &[[100., 100.], [400., 100.], [400., 400.], [100., 400.]], true)]);
            let original = s.engine.document().scene().vector_layer(vector).unwrap().clone();
            let reference = *s.engine.document().scene().children(None).iter().find(|h| **h != vector).unwrap();
            layer(&mut s, LayerAction::Reference { id: occurrence_token(reference) });
            s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source } }).unwrap();
            let p = surface_at(&s, if source == crate::RegionSource::Reference { [250., 250.] } else { [700., 600.] });
            press(&mut s, 10, &[p, p]);
            assert_eq!(s.engine.document().scene().vector_layer(vector).unwrap(), &original, "{platform:?}: source-based fills do not restyle the vector path");
            let request = s.renderer_mut().region_requests.last().unwrap();
            let layer_render::RegionSource::Scene { scope, .. } = &request.source else { panic!("a scene source is required") };
            let expected = match source {
                crate::RegionSource::Visible => SceneScope::All,
                crate::RegionSource::Editing => SceneScope::RawObjects(vector),
                crate::RegionSource::Reference => SceneScope::Members(vec![reference].into()),
            };
            assert_eq!(scope, &expected, "{platform:?}: {source:?}");
        }
        let (mut s, _) = vector_session_on(platform);
        s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: crate::RegionSource::Reference } }).unwrap();
        let before = s.engine.checkpoint();
        let p = surface_at(&s, [700., 600.]);
        press(&mut s, 10, &[p, p]);
        assert_eq!(s.engine.checkpoint(), before, "{platform:?}: a missing reference cannot add a paint layer");
        assert!(s.renderer_mut().region_requests.is_empty());
        assert!(s.state.notice.is_some());
    }
}

#[test]
fn path_figure_cursors_match_transparent_paint_refusals_on_every_platform() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Transparent } }).unwrap();
        for shape in [FigureShape::Path, FigureShape::Polygon, FigureShape::Star, FigureShape::Rectangle, FigureShape::Ellipse, FigureShape::Line] {
            for radius in [0., 20.] {
                s.figure_options.edit("figure_corner_radius", radius, &s.state.localization).unwrap();
                s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Figure { shape, paint: FigurePaint::Outline } }).unwrap();
                s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.)));
                let blocked = s.canvas_cursor().unwrap().segments.iter().any(|segment| segment.marker == 6.);
                assert_eq!(blocked, s.figure_options.needs_path(shape), "{platform:?}: {shape:?} radius {radius}");
            }
        }
    }
}
