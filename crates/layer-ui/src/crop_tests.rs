/// A 1000×800 drawing whose paint layer fills every tile, fitted in a
/// 1600×1000 view.
fn crop_session() -> UiSession<Recorder> {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};
    let mut doc = Document::new("crop", 1000, 800);
    let descriptor = RasterPlane::Color.descriptor(doc.color);
    let bytes = vec![90; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
    let tile = RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap());
    doc.layers[0].raster = RasterRevision::backed(RasterData {
        tiles: (0..4)
            .flat_map(|y| (0..4).map(move |x| [x, y]))
            .map(|coordinate| (TileKey { plane: RasterPlane::Color, coordinate }, tile.clone()))
            .collect(),
        watercolor: None,
    });
    let mut s = UiSession::new(Recorder::default(), doc, [1600, 1000], Platform::Gtk).unwrap();
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s
}

fn crop_frame(s: &UiSession<Recorder>) -> crop::CropFrame {
    s.operation.crop.as_ref().expect("an open crop").frame()
}

fn crop_contact(s: &mut UiSession<Recorder>, sequence: u64, phase: PenPhase, at: Point, tool: layer_engine::ToolKind) {
    let mut e = event(s, sequence, phase, 1.);
    e.surface_position = on_surface(s, at);
    e.tool = tool;
    s.pen(e).unwrap();
    s.frame(sequence, sequence).unwrap();
}

fn crop_drag(s: &mut UiSession<Recorder>, from: Point, to: Point) {
    let pen = layer_engine::ToolKind::Pen;
    crop_contact(s, 10, PenPhase::Down, from, pen);
    crop_contact(s, 11, PenPhase::Move, Point { x: (from.x + to.x) / 2., y: (from.y + to.y) / 2. }, pen);
    crop_contact(s, 12, PenPhase::Move, to, pen);
    crop_contact(s, 13, PenPhase::Up, to, pen);
}

fn near_point(a: Point, b: Point, tolerance: f32) {
    assert!((a.x - b.x).abs() <= tolerance && (a.y - b.y).abs() <= tolerance, "{a:?} != {b:?}");
}

fn near_size(size: [f32; 2], expected: [f32; 2]) {
    assert!((size[0] - expected[0]).abs() < 1e-3 && (size[1] - expected[1]).abs() < 1e-3, "{size:?} != {expected:?}");
}

fn crop_bar_choice(s: &mut UiSession<Recorder>, group: &str, command: CommandId) {
    let bar = s.state.canvas_bar.clone().expect("the crop bar");
    let menu = s.canvas_bar_choice_menu(bar.context, group).expect("a bar dropdown");
    let item = menu.sections.iter().flatten().find(|i| {
        matches!(&i.action, Some(UiAction::CanvasBarEdit { action, .. }) if **action == UiAction::Invoke { command })
    });
    s.dispatch(item.expect("the choice").action.clone().unwrap()).unwrap();
    s.frame(20, 20).unwrap();
}

#[test]
fn crop_starts_at_the_canvas_with_its_bar_and_blocks_other_edits() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Brush);
    invoke(&mut s, CommandId::Crop);
    s.frame(2, 2).unwrap();
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Crop);
    assert!(s.operation.active() && !s.operation.transforming());
    let frame = crop_frame(&s);
    assert_eq!((frame.center, frame.size, frame.angle), (Point { x: 500., y: 400. }, [1000., 800.], 0.));
    assert!(s.command(CommandId::Crop).selected);
    assert_eq!(s.workspace_working_state().canvas_tool, LayerCanvasTool::Paint, "a saved workspace keeps the tool the crop returns to");
    let bar = s.state.canvas_bar.clone().expect("the crop bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Crop);
    assert_eq!((bar.placement, bar.anchor), (CanvasBarPlacement::BottomEdge, None));
    let items: Vec<_> = bar
        .items
        .iter()
        .map(|item| match &item.option {
            ToolOption::Choice { id, items, .. } => (*id, items.iter().map(|i| i.label).collect::<Vec<_>>().join(" ")),
            ToolOption::Action { state, .. } => (state.label, item.label.to_string()),
            ToolOption::Numeric(_) | ToolOption::Range { .. } => unreachable!("bars hold no values"),
        })
        .collect();
    assert_eq!(items, [
        ("crop-ratio", "Free Original 1:1 4:5 2:3 5:7 16:9".to_string()),
        ("Swap crop orientation", String::new()),
        ("Fit Crop to Content", "Fit Content".into()),
        ("crop-overlay", "Thirds Grid Diagonal Golden Ratio".to_string()),
        ("Straighten", "Straighten".into()),
        ("Delete Cropped Pixels", "Delete Cropped".into()),
        ("Reset crop", "Reset".into()),
    ]);
    assert_eq!(bar_items(&bar.completion), [
        (CommandId::CancelTransform, "Cancel", false),
        (CommandId::ApplyTransform, "Apply", false),
    ]);
    assert_eq!(s.command(CommandId::ApplyTransform).label, "Apply crop");
    let actions: Vec<_> = s.state.tool_actions.iter().map(|a| a.command).collect();
    assert!(actions.contains(&CommandId::ApplyTransform) && actions.contains(&CommandId::CropStraighten), "Tool Options shows the bar's items");
    let settings: Vec<_> = s.state.tool_settings.iter().map(|c| (c.id, c.value)).collect();
    assert_eq!(settings, [("crop_width", 1000.), ("crop_height", 800.), ("crop_angle", 0.)]);
    assert!(s.command(CommandId::CropRatioFree).selected && s.command(CommandId::CropOverlayThirds).selected);
    assert!(!s.command(CommandId::CropDeleteCroppedPixels).selected, "cropped pixels are kept by default");
    for command in [CommandId::ClearLayer, CommandId::CanvasSize, CommandId::SelectAll, CommandId::ScaleRotate, CommandId::TransformFlipHorizontal] {
        assert!(!s.command(command).enabled, "{command:?}");
    }
    assert_eq!(s.command_disabled_reason(CommandId::CanvasSize).as_deref(), Some("Apply or cancel the crop first"));
    let overlay = s.renderer_mut().crop_overlay.expect("the shield");
    near_point(overlay.to_crop.map(Point { x: 1000., y: 800. }), Point { x: 1., y: 1. }, 1e-4);
    let mut segments = Vec::new();
    s.append_layer_overlay(&mut segments);
    assert_eq!(segments.iter().filter(|g| g.marker == 2.).count(), 8, "eight handles");
    assert_eq!(segments.iter().filter(|g| g.marker == 0.).count(), 4, "thirds");

    invoke(&mut s, CommandId::Undo);
    assert!(!s.cropping(), "Undo cancels the crop");
    assert!(!s.engine.can_undo(), "and changes nothing else");
    invoke(&mut s, CommandId::Crop);
    let revision = s.engine.document().revision;
    invoke(&mut s, CommandId::CancelTransform);
    s.frame(3, 3).unwrap();
    assert!(!s.operation.active());
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Paint, "the previous tool returns");
    assert!(s.renderer_mut().crop_overlay.is_none());
    assert_eq!(s.engine.document().revision, revision);
    assert!(!s.engine.can_undo());
}

#[test]
fn a_ratio_from_the_bar_constrains_handle_drags_that_grow_the_canvas() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Crop);
    s.frame(2, 2).unwrap();
    crop_bar_choice(&mut s, "crop-ratio", CommandId::CropRatioSquare);
    assert!(s.command(CommandId::CropRatioSquare).selected);
    let frame = crop_frame(&s);
    assert_eq!((frame.center, frame.size), (Point { x: 500., y: 400. }, [800., 800.]), "the largest square in the canvas");
    crop_drag(&mut s, Point { x: 900., y: 800. }, Point { x: 1100., y: 950. });
    let frame = crop_frame(&s);
    assert!((frame.size[0] - frame.size[1]).abs() < 1e-3, "{:?}", frame.size);
    near_point(frame.corners()[0], Point { x: 100., y: 0. }, 1e-3);
    assert!(frame.corners()[2].x > 1000. && frame.corners()[2].y > 800., "past the canvas");
    crop_drag(&mut s, Point { x: 100., y: frame.center.y }, Point { x: 50., y: frame.center.y });
    let frame = crop_frame(&s);
    assert!((frame.size[0] - frame.size[1]).abs() < 1e-3, "an edge drag keeps the ratio too");

    let paint = s.engine.document().layers[0].id;
    let tiles = |s: &UiSession<Recorder>| s.engine.document().layers[0].raster.wait_data().unwrap().tiles.len();
    let before = tiles(&s);
    let [x0, y0] = [frame.corners()[0].x.round(), frame.corners()[0].y.round()];
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(4, 4).unwrap();
    assert!(!s.operation.active());
    let doc = s.engine.document();
    let size = frame.size[0].round() as u32;
    assert_eq!([doc.width, doc.height], [size, size]);
    assert_eq!(doc.layer(paint).unwrap().properties.offset.x, -x0);
    assert!(y0 < 0. && size as f32 + y0 > 800., "the new area is transparent canvas");
    assert!(s.renderer_mut().pending_operations.is_empty(), "keeping the pixels changes only metadata");
    assert_eq!(tiles(&s), before);
    invoke(&mut s, CommandId::Undo);
    assert_eq!([s.engine.document().width, s.engine.document().height], [1000, 800]);
    assert!(!s.engine.can_undo(), "one undo step");
}

#[test]
fn swap_orientation_and_the_ratio_menu_follow_the_frame() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Crop);
    invoke(&mut s, CommandId::CropRatioTwoThree);
    near_size(crop_frame(&s).size, [1000., 1000. / 1.5]);
    invoke(&mut s, CommandId::CropSwapOrientation);
    near_size(crop_frame(&s).size, [800. / 1.5, 800.]);
    invoke(&mut s, CommandId::CropRatioFree);
    invoke(&mut s, CommandId::ResetTransform);
    assert_eq!(crop_frame(&s).size, [1000., 800.]);
    s.dispatch(UiAction::SetToolSetting { id: "crop_width".into(), value: 600. }).unwrap();
    assert_eq!(crop_frame(&s).size, [600., 800.]);
}

#[test]
fn handles_drag_immediately_with_every_device_and_a_finger_inside_pans() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Crop);
    s.frame(2, 2).unwrap();
    let touch = |id, phase, position: Point| pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, [position.x, position.y], 0);
    let corner = on_surface(&s, Point { x: 1000., y: 800. });
    assert!(s.input(touch(1, ContactPhase::Down, corner)).unwrap().paint, "a finger drags a handle");
    s.input(touch(1, ContactPhase::Up, corner)).unwrap();
    let inside = on_surface(&s, Point { x: 500., y: 400. });
    assert!(!s.input(touch(2, ContactPhase::Down, inside)).unwrap().paint, "a finger inside the frame navigates");
    s.input(touch(2, ContactPhase::Up, inside)).unwrap();
    for (sequence, tool) in [(30, layer_engine::ToolKind::Finger), (40, layer_engine::ToolKind::Mouse), (50, layer_engine::ToolKind::Pen)] {
        let before = crop_frame(&s);
        let corner = before.corners()[2];
        let moved = Point { x: corner.x - 50., y: corner.y - 40. };
        crop_contact(&mut s, sequence, PenPhase::Down, corner, tool);
        crop_contact(&mut s, sequence + 1, PenPhase::Move, moved, tool);
        assert!(s.operation.dragging(), "{tool:?} drags without a hold");
        crop_contact(&mut s, sequence + 2, PenPhase::Up, moved, tool);
        near_size(crop_frame(&s).size, [before.size[0] - 50., before.size[1] - 40.]);
    }
    let frame = crop_frame(&s);
    crop_drag(&mut s, frame.center, Point { x: frame.center.x - 30., y: frame.center.y + 10. });
    near_point(crop_frame(&s).center, Point { x: frame.center.x - 30., y: frame.center.y + 10. }, 1e-3);
}

#[test]
fn blur_rolls_back_a_crop_drag_and_keeps_the_crop() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Crop);
    let pen = layer_engine::ToolKind::Pen;
    crop_contact(&mut s, 5, PenPhase::Down, Point { x: 1000., y: 400. }, pen);
    crop_contact(&mut s, 6, PenPhase::Move, Point { x: 700., y: 400. }, pen);
    assert_eq!(crop_frame(&s).size[0], 700.);
    s.input(UiInput::Blur).unwrap();
    assert!(s.cropping(), "the crop survives focus loss");
    assert_eq!(crop_frame(&s).size[0], 1000., "only the drag rolls back");
    assert!(s.require_idle().is_ok());
}

#[test]
fn o_cycles_the_overlay_only_while_cropping() {
    let mut s = crop_session();
    key(&mut s, "o", true, false, false);
    key(&mut s, "o", false, false, false);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Move, "O keeps choosing Operation elsewhere");
    key(&mut s, "c", true, false, false);
    key(&mut s, "c", false, false, false);
    assert!(s.cropping(), "C chooses Crop");
    for expected in [CommandId::CropOverlayGrid, CommandId::CropOverlayDiagonal, CommandId::CropOverlayGolden, CommandId::CropOverlayThirds] {
        key(&mut s, "o", true, false, false);
        key(&mut s, "o", false, false, false);
        assert!(s.command(expected).selected, "{expected:?}");
    }
    assert!(s.cropping(), "cycling keeps the crop open");
    key(&mut s, "Escape", true, false, false);
    assert!(!s.cropping(), "Escape cancels");
}

#[test]
fn delete_cropped_pixels_erases_outside_in_the_same_undo_step() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Crop);
    invoke(&mut s, CommandId::CropDeleteCroppedPixels);
    assert!(s.command(CommandId::CropDeleteCroppedPixels).selected);
    crop_drag(&mut s, Point { x: 0., y: 0. }, Point { x: 300., y: 260. });
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(30, 30).unwrap();
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [700, 540]);
    let paint = doc.layers[0].id;
    let operations = s.renderer_mut().pending_operations.clone();
    assert_eq!(operations.len(), 4, "four edge strips");
    assert!(operations.iter().all(|(id, op)| *id == paint
        && matches!(op.kind, layer_core::LayerOperationKind::Erase { alpha_locked: false })));
    invoke(&mut s, CommandId::Undo);
    assert_eq!([s.engine.document().width, s.engine.document().height], [1000, 800]);
    assert!(!s.engine.can_undo(), "one undo step");
    invoke(&mut s, CommandId::Crop);
    assert!(s.command(CommandId::CropDeleteCroppedPixels).selected, "the choice is remembered");
}

#[test]
fn straighten_levels_a_drawn_line_and_applies_one_resampling_step_on_locked_layers() {
    let mut s = crop_session();
    let paint = s.engine.document().layers[0].id;
    let mut locked = s.engine.document().layers[0].clone();
    locked.properties.locked = true;
    s.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(locked))).unwrap();
    rectangle_selection(&mut s, [100., 100., 300., 250.]);
    let ruler = layer_core::Ruler {
        id: 9,
        geometry: layer_core::RulerGeometry::Straight { start: Point { x: 10., y: 20. }, end: Point { x: 400., y: 50. } },
    };
    s.layer_edit(layer_core::Edit::SetRulers(vec![ruler])).unwrap();
    s.frame(2, 2).unwrap();
    let history = s.engine.document().clone();
    invoke(&mut s, CommandId::Crop);
    invoke(&mut s, CommandId::CropStraighten);
    assert!(s.command(CommandId::CropStraighten).selected);
    let angle = 0.12f32;
    let from = Point { x: 200., y: 300. };
    let to = Point { x: 200. + 500. * angle.cos(), y: 300. + 500. * angle.sin() };
    crop_drag(&mut s, from, to);
    assert!(!s.command(CommandId::CropStraighten).selected, "one line per arming");
    let frame = crop_frame(&s);
    assert!((frame.angle - angle).abs() < 1e-4, "{}", frame.angle);
    for corner in frame.corners() {
        assert!(corner.x >= -0.01 && corner.y >= -0.01 && corner.x <= 1000.01 && corner.y <= 800.01, "{corner:?} fits the canvas");
    }
    assert!((frame.size[0] / frame.size[1] - 1.25).abs() < 1e-4, "the shape is kept");
    let before = s.engine.document().clone();
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(40, 40).unwrap();
    let doc = s.engine.document();
    let geometry = frame.geometry(false);
    assert_eq!([doc.width, doc.height], geometry.rect.size);
    let to_canvas = geometry.to_canvas();
    let operations = s.renderer_mut().pending_operations.clone();
    let [(id, op)] = operations.as_slice() else { panic!("one resample: {operations:?}") };
    assert_eq!(*id, paint, "the locked layer follows");
    let layer_core::LayerOperationKind::Transform(transform) = &op.kind else { panic!("a resample") };
    assert_eq!(transform.interpolation, layer_core::Interpolation::Bicubic);
    let map = transform.as_affine().unwrap();
    let after = s.engine.document().layer_transform(paint).inverse().unwrap();
    for p in [Point { x: 0., y: 0. }, Point { x: 1000., y: 800. }] {
        near_point(map.map(p), after.map(to_canvas.map(before.layer_transform(paint).map(p))), 0.01);
    }
    let doc = s.engine.document();
    assert_eq!(doc.selection, Some(before.selection.as_ref().unwrap().transformed(to_canvas).unwrap()));
    assert_eq!(doc.rulers[0].geometry, before.rulers[0].geometry.transformed(to_canvas));
    invoke(&mut s, CommandId::Undo);
    s.frame(41, 41).unwrap();
    assert_eq!(s.engine.document().layers, history.layers);
    assert_eq!([s.engine.document().width, s.engine.document().height], [1000, 800]);
}

#[test]
fn shift_snaps_straighten_lines_to_fifteen_degrees_and_the_angle_is_typed_too() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Crop);
    invoke(&mut s, CommandId::CropStraighten);
    s.input(UiInput::Key {
        key: "Shift_L".into(),
        pressed: true,
        repeat: false,
        modifiers: Modifiers { shift: true, ..Default::default() },
        editing: false,
        divider: None,
    })
    .unwrap();
    crop_drag(&mut s, Point { x: 100., y: 100. }, Point { x: 600., y: 200. });
    assert!((crop_frame(&s).angle - 15f32.to_radians()).abs() < 1e-4, "{}", crop_frame(&s).angle);
    s.dispatch(UiAction::SetToolSetting { id: "crop_angle".into(), value: -0.05 }).unwrap();
    assert!((crop_frame(&s).angle + 0.05).abs() < 1e-6);
    assert!(s.dispatch(UiAction::SetToolSetting { id: "crop_angle".into(), value: 1.2 }).is_err());
    s.dispatch(UiAction::ResetToolSetting { id: "crop_angle".into() }).unwrap();
    assert_eq!(crop_frame(&s).angle, 0.);
}

#[test]
fn straighten_image_to_guide_opens_a_level_crop_from_the_guide_bar() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::Ruler);
    let pen = layer_engine::ToolKind::Pen;
    crop_contact(&mut s, 2, PenPhase::Down, Point { x: 100., y: 400. }, pen);
    crop_contact(&mut s, 3, PenPhase::Move, Point { x: 400., y: 420. }, pen);
    crop_contact(&mut s, 4, PenPhase::Up, Point { x: 700., y: 440. }, pen);
    let bar = s.state.canvas_bar.clone().expect("the guide bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Guide);
    assert!(s.command(CommandId::StraightenToGuide).enabled);
    bar_edit(&mut s, CommandId::StraightenToGuide);
    assert!(s.cropping());
    assert!((crop_frame(&s).angle - (40f32 / 600.).atan()).abs() < 1e-3, "{}", crop_frame(&s).angle);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::Crop));
    invoke(&mut s, CommandId::CancelTransform);
    s.layer_edit(layer_core::Edit::SetRulers(vec![layer_core::Ruler {
        id: 3,
        geometry: layer_core::RulerGeometry::Radial { center: Point { x: 10., y: 10. } },
    }]))
    .unwrap();
    s.rulers.selected = Some(3);
    assert_eq!(s.command_disabled_reason(CommandId::StraightenToGuide).as_deref(), Some("Select a straight guide first"));
}

#[test]
fn crop_keys_follow_the_shortcuts_table_and_the_photo_workspace_offers_the_tool() {
    let c = KeyChord { key: "c".into(), command: false, alt: false, shift: false };
    let shift_c = KeyChord { shift: true, ..c.clone() };
    for platform in [Platform::Gtk, Platform::Web] {
        assert!(c.available(platform) && shift_c.available(platform));
    }
    for (preset, expected) in [("capy", &c), ("photoshop", &c), ("affinity", &c), ("gimp", &shift_c)] {
        let mut settings = Settings::default();
        crate::keymaps::select(&mut settings, preset).unwrap();
        assert_eq!(settings.keys(&CommandId::Crop.shortcut_id()), std::slice::from_ref(expected), "{preset}");
    }
    let photo = WorkspacePreset::Photographer.layout(Platform::Gtk);
    let tools: Vec<_> = photo
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .iter()
        .filter_map(|t| match t.control {
            ToolbarControl::Command { command } => Some(command),
            _ => None,
        })
        .collect();
    let crop = tools.iter().position(|c| *c == CommandId::Crop).expect("Crop in the Photo toolbar");
    assert_eq!(tools[crop - 1], CommandId::Move);
    assert_eq!(UiSession::<Recorder>::tool_category(LayerCanvasTool::Crop, Tool::Pen), ToolCategory::MoveTransform);
}
