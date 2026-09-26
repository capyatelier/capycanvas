fn bar_area() -> Bounds {
    Bounds { x: 0., y: 0., width: 1000., height: 800. }
}

fn object(x: f32, y: f32, width: f32, height: f32) -> Option<Bounds> {
    Some(Bounds { x, y, width, height })
}

#[test]
fn canvas_bar_goes_below_the_object_then_above_then_to_the_bottom_edge() {
    let size = [300., 48.];
    let near = CanvasBarPlacement::NearObject;
    let (below, side) = place_canvas_bar(bar_area(), &[], object(400., 200., 200., 200.), size, near);
    assert_eq!(side, CanvasBarSide::Below);
    assert_eq!(below.y, 400. + canvas_bar::CANVAS_BAR_MARGIN);
    assert_eq!(below.x + below.width * 0.5, 500.);
    let (above, side) = place_canvas_bar(bar_area(), &[], object(400., 600., 200., 150.), size, near);
    assert_eq!(side, CanvasBarSide::Above);
    assert!(above.y + above.height <= 600.);
    let (_, side) = place_canvas_bar(bar_area(), &[], object(50., 50., 900., 700.), size, near);
    assert_eq!(side, CanvasBarSide::BottomEdge, "an object covering most of the area");
    let (_, side) = place_canvas_bar(bar_area(), &[], object(2000., 50., 100., 100.), size, near);
    assert_eq!(side, CanvasBarSide::BottomEdge, "an off-screen object");
    let (_, side) = place_canvas_bar(bar_area(), &[], None, size, near);
    assert_eq!(side, CanvasBarSide::BottomEdge);
    let (edge, side) = place_canvas_bar(bar_area(), &[], object(400., 200., 200., 200.), size, CanvasBarPlacement::BottomEdge);
    assert_eq!(side, CanvasBarSide::BottomEdge);
    assert_eq!(edge.y + edge.height, 800. - canvas_bar::CANVAS_BAR_MARGIN);
    let narrow = Bounds { width: 500., ..bar_area() };
    let (_, side) = place_canvas_bar(narrow, &[], object(100., 100., 100., 100.), size, near);
    assert_eq!(side, CanvasBarSide::BottomEdge, "narrow work areas use the bottom edge");
}

#[test]
fn canvas_bar_avoids_floating_panels_and_stays_inside_the_area() {
    let size = [300., 48.];
    let near = CanvasBarPlacement::NearObject;
    let panel = Bounds { x: 350., y: 400., width: 300., height: 200. };
    let (bar, side) = place_canvas_bar(bar_area(), &[panel], object(400., 200., 200., 180.), size, near);
    assert_eq!(side, CanvasBarSide::Above);
    assert!(bar.y + bar.height <= 200.);
    let (bar, _) = place_canvas_bar(bar_area(), &[], object(-150., 200., 200., 200.), size, near);
    assert!(bar.x >= canvas_bar::CANVAS_BAR_MARGIN, "clamped to the left edge");
    let (bar, _) = place_canvas_bar(bar_area(), &[], object(950., 200., 200., 200.), size, near);
    assert!(bar.x + bar.width <= 1000. - canvas_bar::CANVAS_BAR_MARGIN, "clamped to the right edge");
}

fn filled_selection_session() -> UiSession<Recorder> {
    use layer_core::Selection;
    let mut s = session();
    let selection = Selection::polygon(vec![
        Point { x: 100., y: 100. },
        Point { x: 300., y: 100. },
        Point { x: 300., y: 300. },
        Point { x: 100., y: 300. },
    ])
    .unwrap();
    s.fill_selection(selection.clone()).unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s
}

fn bar_commands(items: &[CanvasBarItem]) -> Vec<CommandId> {
    items
        .iter()
        .filter_map(|item| match &item.option {
            ToolOption::Action { state, .. } => Some(state.id),
            _ => None,
        })
        .collect()
}

#[test]
fn transform_publishes_a_bar_whose_edits_expire_with_the_transform() {
    let mut s = filled_selection_session();
    assert!(s.state.canvas_bar.is_none());
    let change = invoke(&mut s, CommandId::ScaleRotate);
    assert_ne!(change.regions & regions::CANVAS_BAR, 0);
    let bar = s.state.canvas_bar.clone().expect("transform bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Transform);
    assert_eq!(bar_commands(&bar.items), [CommandId::TransformAspect, CommandId::TransformFlipHorizontal, CommandId::TransformFlipVertical, CommandId::TransformRotateLeft, CommandId::TransformRotateRight, CommandId::ResetTransform]);
    assert_eq!(bar_commands(&bar.completion), [CommandId::CancelTransform, CommandId::ApplyTransform]);
    assert_eq!(bar.placement, CanvasBarPlacement::NearObject);
    assert_eq!(bar.anchor, s.transform_document_bounds());
    assert!(s
        .dispatch(UiAction::CanvasBarEdit {
            context: bar.context,
            action: Box::new(UiAction::Invoke { command: CommandId::Undo }),
        })
        .is_err(), "only the bar's own actions are accepted");
    s.dispatch(UiAction::CanvasBarEdit {
        context: bar.context,
        action: Box::new(UiAction::Invoke { command: CommandId::ApplyTransform }),
    })
    .unwrap();
    assert!(!s.operation.active());
    assert_eq!(
        s.state.canvas_bar.as_ref().map(|b| b.context.kind),
        Some(CanvasBarKind::Selection),
        "the moved selection's bar follows the transform"
    );
    invoke(&mut s, CommandId::ScaleRotate);
    let next = s.state.canvas_bar.clone().unwrap();
    assert_ne!(next.context, bar.context);
    assert!(s
        .dispatch(UiAction::CanvasBarEdit {
            context: bar.context,
            action: Box::new(UiAction::Invoke { command: CommandId::CancelTransform }),
        })
        .is_err(), "a previous transform's bar is stale");
    assert!(s.operation.active());
}

#[test]
fn canvas_bar_keeps_its_view_during_a_handle_drag_and_follows_the_object_after() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    let before = s.state.canvas_bar.clone().unwrap();
    s.transform_pen(event(&s, 1, PenPhase::Down, 1.), Point { x: 200., y: 200. }).unwrap();
    s.transform_pen(event(&s, 2, PenPhase::Move, 1.), Point { x: 400., y: 260. }).unwrap();
    s.frame(2, 2).unwrap();
    assert_eq!(s.state.canvas_bar.as_ref().unwrap().anchor, before.anchor);
    s.transform_pen(event(&s, 3, PenPhase::Up, 1.), Point { x: 400., y: 260. }).unwrap();
    s.frame(3, 3).unwrap();
    let after = s.state.canvas_bar.as_ref().unwrap();
    assert_eq!(after.context, before.context, "the same transform keeps its bar");
    let [x0, _, _, _] = after.anchor.unwrap();
    assert!(x0 > before.anchor.unwrap()[0] + 150.);
}

#[test]
fn canvas_bar_input_reply_hides_the_bar_during_canvas_contacts() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    let pointer = |phase| UiInput::Pointer {
        id: 7,
        phase,
        kind: PointerKind::Mouse,
        button: PointerButton::Primary,
        position: [500., 500.],
    };
    assert!(s.input(pointer(ContactPhase::Down)).unwrap().canvas_bar_hidden);
    assert!(!s.input(pointer(ContactPhase::Up)).unwrap().canvas_bar_hidden);
}

#[test]
fn hiding_the_canvas_bar_keeps_apply_and_cancel_at_the_bottom_edge() {
    let mut s = filled_selection_session();
    assert!(s.command(CommandId::ShowCanvasActionBar).selected);
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    assert!(!s.state.workspace.layout.canvas_bar.visible);
    assert!(!s.command(CommandId::ShowCanvasActionBar).selected);
    invoke(&mut s, CommandId::ScaleRotate);
    let bar = s.state.canvas_bar.clone().unwrap();
    assert!(bar.items.is_empty());
    assert_eq!(bar_commands(&bar.completion), [CommandId::CancelTransform, CommandId::ApplyTransform]);
    assert_eq!(bar.placement, CanvasBarPlacement::BottomEdge);
    invoke(&mut s, CommandId::CancelTransform);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert!(s.state.workspace.layout.canvas_bar.visible, "the toggle is one workspace history step");
}

#[test]
fn canvas_bar_layout_fits_items_and_clears_the_transform_handles() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    let bar = s.state.canvas_bar.clone().unwrap();
    let measure = |width: f32| CanvasBarMeasure {
        context: bar.context,
        label: 0.,
        items: vec![width; 6],
        completion: vec![80., 80.],
        more: 40.,
        height: 48.,
        gap: 4.,
        padding: 6.,
    };
    let layout = s.canvas_bar_layout(&measure(100.)).expect("current context");
    assert_eq!(layout.items, 6);
    assert_eq!(layout.side, CanvasBarSide::Below);
    let lowest = s
        .transform_handle_points()
        .into_iter()
        .map(|[_, y]| y)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(layout.bounds.y > lowest + canvas_bar::CANVAS_BAR_MARGIN);
    assert_eq!(s.canvas_bar_layout(&measure(5000.)).unwrap().items, 0, "items that do not fit go to More");
    let stale = CanvasBarMeasure {
        context: CanvasBarContext { generation: bar.context.generation + 1, ..bar.context },
        ..measure(100.)
    };
    assert!(s.canvas_bar_layout(&stale).is_none());
    let menu = s.canvas_bar_menu(bar.context, 0).unwrap();
    let first = &menu.sections[0][0];
    assert_eq!(
        first.action,
        Some(UiAction::CanvasBarEdit {
            context: bar.context,
            action: Box::new(UiAction::Invoke { command: CommandId::TransformAspect }),
        })
    );
    assert!(menu
        .sections
        .iter()
        .flatten()
        .any(|item| item.action == Some(UiAction::Invoke { command: CommandId::ShowCanvasActionBar })));
}

#[test]
fn photo_placement_bar_offers_original_size_and_counts_a_batch() {
    use layer_core::color::{SampleDepth, source::*};
    let source = || {
        let mut builder = SourceBuilder::new([20, 10], SourceInterpretation {
            channels: SourceChannels::Rgba, depth: SampleDepth::U8,
            profile: Default::default(), profile_assumed: false,
        }, 1024 * 1024).unwrap();
        for _ in 0..10 { builder.push_row(&[255; 80]).unwrap(); }
        builder.finish().unwrap()
    };
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new("bar placement", 200, 150), [800, 600]).unwrap();
    s.place_layer_source("Photo", source(), None).unwrap();
    s.frame(0, 0).unwrap();
    let bar = s.state.canvas_bar.clone().expect("placement bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Placement);
    assert_eq!(
        bar_commands(&bar.items),
        [
            CommandId::TransformAspect,
            CommandId::PlacementOriginalSize,
            CommandId::TransformFlipHorizontal,
            CommandId::TransformFlipVertical,
            CommandId::TransformRotateLeft,
            CommandId::TransformRotateRight,
            CommandId::ResetTransform,
        ]
    );
    assert_eq!(bar.label, None);
    s.set_platform(Platform::Mac);
    s.frame(1, 1).unwrap();
    assert!(s.state.canvas_bar.is_none(), "hosts without the bar keep their own controls");
}

#[test]
fn transform_flips_quarter_turns_and_reset_keep_the_box_centred() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    s.dispatch(UiAction::SetToolSetting { id: "transform_angle".into(), value: 0.3 }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "transform_skew".into(), value: 0.2 }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "transform_x".into(), value: 25. }).unwrap();
    s.frame(2, 2).unwrap();
    let preview = |s: &mut UiSession<Recorder>| s.renderer_mut().transform.clone().unwrap().transform.affine;
    let settled = preview(&mut s);
    let [x0, y0, x1, y1] = s.transform_document_bounds().unwrap();
    let centre = Point { x: (x0 + x1) * 0.5, y: (y0 + y1) * 0.5 };
    let close = |a: layer_core::Affine, b: layer_core::Affine| a.0.iter().zip(b.0).all(|(x, y)| (x - y).abs() < 1e-3);
    for (command, times) in [
        (CommandId::TransformFlipHorizontal, 2),
        (CommandId::TransformFlipVertical, 2),
        (CommandId::TransformRotateRight, 4),
        (CommandId::TransformRotateLeft, 4),
    ] {
        for _ in 0..times {
            invoke(&mut s, command);
        }
        s.frame(3, 3).unwrap();
        assert!(close(preview(&mut s), settled), "{command:?} returns after {times}");
    }
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    s.frame(4, 4).unwrap();
    let flipped = preview(&mut s);
    let mirror = layer_core::Affine::translation(Point { x: -centre.x, y: -centre.y })
        .then(layer_core::Affine([-1., 0., 0., 1., 0., 0.]))
        .then(layer_core::Affine::translation(centre));
    assert!(close(flipped, settled.then(mirror)), "flip mirrors about the box centre: {flipped:?}");
    invoke(&mut s, CommandId::ResetTransform);
    s.frame(5, 5).unwrap();
    assert_eq!(preview(&mut s), layer_core::Affine::IDENTITY, "reset returns to the session start");
    assert!(s.operation.active());
}

#[test]
fn flipping_a_placement_stays_lossless_and_applies_as_one_step() {
    use layer_core::color::{SampleDepth, source::*};
    let mut builder = SourceBuilder::new([20, 10], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U8,
        profile: Default::default(), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    for _ in 0..10 { builder.push_row(&[255; 80]).unwrap(); }
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new("flip placement", 200, 150), [800, 600]).unwrap();
    s.place_layer_source("Photo", builder.finish().unwrap(), None).unwrap();
    invoke(&mut s, CommandId::ApplyTransform);
    let placed = s.engine.document().clone();
    invoke(&mut s, CommandId::ScaleRotate);
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    invoke(&mut s, CommandId::TransformRotateRight);
    invoke(&mut s, CommandId::ApplyTransform);
    let layer = s.engine.document().layer(s.engine.document().active_layer).unwrap();
    assert!(layer.source.is_some(), "the retained photo is kept");
    let [a, b, c, d, _, _] = layer.properties.placement.0;
    assert!(a.abs() < 1e-4 && d.abs() < 1e-4 && (b * c) > 0.9, "a mirrored quarter turn: {a} {b} {c} {d}");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers, placed.layers, "one undo step restores the placement");
}

fn rectangle_selection(s: &mut UiSession<Recorder>, [x0, y0, x1, y1]: [f32; 4]) {
    let selection = layer_core::Selection::polygon(vec![
        Point { x: x0, y: y0 },
        Point { x: x1, y: y0 },
        Point { x: x1, y: y1 },
        Point { x: x0, y: y1 },
    ])
    .unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
    s.frame(1, 1).unwrap();
}

#[test]
fn selection_bar_follows_selection_tools_commands_and_history() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    invoke(&mut s, CommandId::RectangleSelect);
    rectangle_selection(&mut s, [100., 100., 300., 250.]);
    let bar = s.state.canvas_bar.clone().expect("selection bar with a selection tool");
    assert_eq!(bar.context.kind, CanvasBarKind::Selection);
    assert_eq!(bar.placement, CanvasBarPlacement::NearObject);
    let [x0, y0, x1, y1] = bar.anchor.unwrap();
    assert!(x0 <= 100. && y0 <= 100. && x1 >= 300. && y1 >= 250.);
    assert_eq!(
        bar_commands(&bar.items),
        [
            CommandId::Deselect,
            CommandId::InvertSelection,
            CommandId::ScaleRotate,
            CommandId::MaskSelection,
            CommandId::FillSelection,
            CommandId::QuickMask,
            CommandId::SaveSelectionLayer,
        ]
    );
    let menu = s.canvas_bar_menu(bar.context, bar.items.len()).unwrap();
    assert!(menu.sections.iter().flatten().any(|i| i.label.starts_with("Grow")), "More includes the Select menu");
    invoke(&mut s, CommandId::Brush);
    assert!(s.state.canvas_bar.is_none(), "painting inside a selection shows no bar");
    invoke(&mut s, CommandId::Move);
    assert!(s.state.canvas_bar.is_some(), "Move offers the selection bar");
    invoke(&mut s, CommandId::Eyedropper);
    assert!(s.state.canvas_bar.is_none());
    invoke(&mut s, CommandId::SelectAll);
    assert!(s.state.canvas_bar.is_some(), "a selection command arms the bar until the tool changes");
    invoke(&mut s, CommandId::Brush);
    assert!(s.state.canvas_bar.is_none());
    invoke(&mut s, CommandId::Eyedropper);
    assert!(s.state.canvas_bar.is_none(), "a tool change disarms it");
    invoke(&mut s, CommandId::Deselect);
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().selection.is_some());
    assert!(s.state.canvas_bar.is_none(), "undo restores the selection without offering the bar");
    invoke(&mut s, CommandId::InvertSelection);
    invoke(&mut s, CommandId::RectangleSelect);
    let inverted = s.state.canvas_bar.clone().unwrap();
    assert_eq!(inverted.placement, CanvasBarPlacement::BottomEdge, "an inverted selection surrounds the view");
    s.dispatch(UiAction::CanvasBarEdit {
        context: inverted.context,
        action: Box::new(UiAction::Invoke { command: CommandId::Deselect }),
    })
    .unwrap();
    assert!(s.engine.document().selection.is_none());
    assert!(s.state.canvas_bar.is_none());
}

#[test]
fn selection_bar_masks_the_active_layer_in_one_step() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::Move);
    let bar = s.state.canvas_bar.clone().unwrap();
    let before = s.engine.document().clone();
    s.dispatch(UiAction::CanvasBarEdit {
        context: bar.context,
        action: Box::new(UiAction::Invoke { command: CommandId::MaskSelection }),
    })
    .unwrap();
    s.frame(2, 2).unwrap();
    let doc = s.engine.document();
    assert!(doc.layer(doc.active_layer).unwrap().mask.is_some());
    assert!(doc.selection.is_none(), "masking consumes the selection");
    invoke(&mut s, CommandId::Undo);
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.document().layers, before.layers);
    assert_eq!(s.engine.document().selection, before.selection);
}
