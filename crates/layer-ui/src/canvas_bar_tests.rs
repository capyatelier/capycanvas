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
    let mut s = session(Platform::Gtk);
    let selection = rectangle([100., 100., 300., 300.]);
    s.fill_selection(selection.clone()).unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s
}

fn mode_choice(bar: &CanvasBarView) -> Vec<CommandId> {
    match &bar.items[0].option {
        ToolOption::Choice { items, .. } => items
            .iter()
            .map(|i| match i.action {
                UiAction::Invoke { command } => command,
                _ => panic!("mode items are commands"),
            })
            .collect(),
        _ => panic!("the transform bar starts with its mode choice"),
    }
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

fn placed_photo(name: &str) -> UiSession<Recorder> {
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(name, 200, 150), [800, 600], Platform::Gtk).unwrap();
    let photo = layer_core::color::source::rgba8_source([20, 10], |_, _| [255; 4]);
    s.place_layer_source("Photo", std::sync::Arc::unwrap_or_clone(photo), None).unwrap();
    s
}

fn measure(bar: &CanvasBarView, item: f32) -> CanvasBarMeasure {
    CanvasBarMeasure {
        context: bar.context,
        label: 0.,
        items: vec![item; bar.items.len()],
        completion: vec![80., 80.],
        more: 40.,
        height: 48.,
        gap: 4.,
        padding: 6.,
    }
}

#[test]
fn every_command_on_a_transform_bar_is_a_tool_action() {
    let mut transform = filled_selection_session();
    invoke(&mut transform, CommandId::ScaleRotate);
    let mut placement = placed_photo("placement actions");
    for (s, mode) in [(&mut transform, CommandId::TransformDistort), (&mut placement, CommandId::TransformUniform)] {
        for mode in [mode, CommandId::TransformWarp] {
            let _ = s.dispatch(UiAction::Invoke { command: mode });
            let bar = s.state.canvas_bar.clone().unwrap();
            for item in bar.items.iter().chain(&bar.completion) {
                let commands = match &item.option {
                    ToolOption::Action { state, .. } => vec![state.id],
                    ToolOption::Choice { items, .. } => items.iter().filter_map(|i| match i.action {
                        UiAction::Invoke { command } => Some(command),
                        _ => None,
                    }).collect(),
                    ToolOption::Numeric(_) | ToolOption::Range { .. } => Vec::new(),
                };
                for command in commands {
                    assert!(s.state.tool_actions.iter().any(|a| a.command == command), "{mode:?}: {command:?}");
                }
            }
        }
    }
}

#[test]
fn transform_publishes_a_bar_whose_edits_expire_with_the_transform() {
    let mut s = filled_selection_session();
    assert!(s.state.canvas_bar.is_none());
    let change = invoke(&mut s, CommandId::ScaleRotate);
    assert_ne!(change.regions & regions::CANVAS_BAR, 0);
    let bar = s.state.canvas_bar.clone().expect("transform bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Transform);
    assert_eq!(mode_choice(&bar), [CommandId::TransformFree, CommandId::TransformUniform, CommandId::TransformDistort, CommandId::TransformWarp]);
    assert_eq!(bar_commands(&bar.items), [CommandId::TransformFlipHorizontal, CommandId::TransformFlipVertical, CommandId::TransformRotateLeft, CommandId::TransformRotateRight, CommandId::ResetTransform]);
    assert_eq!(bar_commands(&bar.completion), [CommandId::CancelTransform, CommandId::ApplyTransform]);
    assert!(bar.items.iter().all(|item| !item.accent));
    assert_eq!(bar.completion.iter().map(|item| item.accent).collect::<Vec<_>>(), [false, true], "Apply is the accented step");
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
fn the_canvas_bar_hold_covers_contacts_camera_moves_and_floating_drags() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    let pointer = |phase| pointer_input(7, phase, PointerKind::Mouse, PointerButton::Primary, [500., 500.], 0);
    let idle = s.canvas_bar_hold();
    assert_eq!(idle % 2, 0);
    s.input(pointer(ContactPhase::Down)).unwrap();
    assert_eq!(s.canvas_bar_hold(), idle + 1, "a canvas contact holds the bar hidden");
    s.input(pointer(ContactPhase::Up)).unwrap();
    assert_eq!(s.canvas_bar_hold() % 2, 0, "it returns once the contact ends");
    let settled = s.canvas_bar_hold();
    invoke(&mut s, CommandId::ZoomIn);
    let zoomed = s.canvas_bar_hold();
    assert!(zoomed != settled && zoomed.is_multiple_of(2), "a camera move hides a bar beside the object until it settles");

    let viewport = [1600., 1000.];
    let group = s.layout(viewport).groups[0].id;
    s.dispatch(UiAction::MoveGroup { group, target: DockTarget::Float { position: [900., 300.] }, viewport }).unwrap();
    let float = s.layout(viewport).groups.into_iter().find(|g| g.id == group && g.floating).unwrap().bounds;
    let grab = [float.x + 20., float.y + 10.];
    let before = s.canvas_bar_hold();
    drag(&mut s, DockItem::Group { group }, ContactPhase::Down, grab, viewport);
    drag(&mut s, DockItem::Group { group }, ContactPhase::Move, [grab[0] + 40., grab[1]], viewport);
    assert_eq!(s.canvas_bar_hold(), before | 1, "moving a floating group holds the bar hidden");
    drag(&mut s, DockItem::Group { group }, ContactPhase::Up, [grab[0] + 40., grab[1]], viewport);
    assert_eq!(s.canvas_bar_hold() % 2, 0);

    invoke(&mut s, CommandId::ShowCanvasActionBar);
    assert_eq!(s.state.canvas_bar.as_ref().unwrap().placement, CanvasBarPlacement::BottomEdge);
    let edge = s.canvas_bar_hold();
    invoke(&mut s, CommandId::ZoomIn);
    s.input(pointer(ContactPhase::Down)).unwrap();
    assert_eq!(s.canvas_bar_hold(), edge, "a bar on the bottom edge stays through contacts and camera moves");
    s.input(pointer(ContactPhase::Up)).unwrap();
}

#[test]
fn hiding_the_canvas_bar_keeps_apply_and_cancel_at_the_bottom_edge() {
    let mut s = filled_selection_session();
    assert!(s.command(CommandId::ShowCanvasActionBar).selected);
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    assert!(!s.state.workspace.layout.canvas_bar);
    assert!(!s.command(CommandId::ShowCanvasActionBar).selected);
    invoke(&mut s, CommandId::ScaleRotate);
    let bar = s.state.canvas_bar.clone().unwrap();
    assert!(bar.items.is_empty());
    assert_eq!(bar_commands(&bar.completion), [CommandId::CancelTransform, CommandId::ApplyTransform]);
    assert_eq!(bar.placement, CanvasBarPlacement::BottomEdge);
    invoke(&mut s, CommandId::CancelTransform);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert!(s.state.workspace.layout.canvas_bar, "the toggle is one workspace history step");
}

#[test]
fn canvas_bar_layout_fits_items_and_clears_the_transform_handles() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    let bar = s.state.canvas_bar.clone().unwrap();
    let layout = s.canvas_bar_layout(&measure(&bar, 100.)).expect("current context");
    assert_eq!(layout.items, bar.items.len());
    assert_eq!(layout.side, CanvasBarSide::Below);
    let lowest = s
        .transform_handle_points()
        .into_iter()
        .map(|[_, y]| y)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(layout.bounds.y > lowest + canvas_bar::CANVAS_BAR_MARGIN);
    assert_eq!(s.canvas_bar_layout(&measure(&bar, 5000.)).unwrap().items, 0, "items that do not fit go to More");
    let stale = CanvasBarMeasure {
        context: CanvasBarContext { generation: bar.context.generation + 1, ..bar.context },
        ..measure(&bar, 100.)
    };
    assert!(s.canvas_bar_layout(&stale).is_none());
    let menu = s.canvas_bar_menu(bar.context, 0).unwrap();
    let first = &menu.sections[0][0];
    assert_eq!(first.label, "Mode");
    assert_eq!(
        first.sections[0][0].action,
        Some(UiAction::CanvasBarEdit {
            context: bar.context,
            action: Box::new(UiAction::Invoke { command: CommandId::TransformFree }),
        })
    );
    assert!(menu
        .sections
        .iter()
        .flatten()
        .any(|item| item.action == Some(UiAction::Invoke { command: CommandId::ShowCanvasActionBar })));
}

#[test]
fn canvas_bar_stays_in_the_window_when_docks_leave_no_work_area() {
    let mut s = filled_selection_session();
    s.set_viewport([360., 640.], [360, 640]).unwrap();
    assert!(s.layout([360., 640.]).work_area.width < 200., "the docks fill a phone-width window");
    invoke(&mut s, CommandId::ScaleRotate);
    let bar = s.state.canvas_bar.clone().unwrap();
    let layout = s.canvas_bar_layout(&measure(&bar, 90.)).unwrap();
    assert_eq!(layout.side, CanvasBarSide::BottomEdge);
    assert!(layout.bounds.x >= 0. && layout.bounds.x + layout.bounds.width <= 360., "{layout:?}");
    assert!(layout.items < bar.items.len(), "items that do not fit the window go to More");
}

#[test]
fn canvas_bar_uses_the_window_width_before_it_would_show_only_more() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::RectangleSelect);
    let bar = s.state.canvas_bar.clone().unwrap();
    assert_eq!(bar.context.kind, CanvasBarKind::Selection);
    let work = s.layout([1600., 1000.]).work_area;
    let wide = CanvasBarMeasure { completion: Vec::new(), ..measure(&bar, work.width - 60.) };
    let margin = canvas_bar::CANVAS_BAR_MARGIN;
    assert!(2. * (wide.padding + margin) + wide.more <= work.width, "More alone fits the work area");
    let layout = s.canvas_bar_layout(&wide).unwrap();
    assert_eq!(layout.items, 1, "the bar leaves the work area to show its first item");
    assert_eq!(layout.side, CanvasBarSide::BottomEdge, "a bar across the docks keeps to the bottom edge");
    assert!(layout.bounds.width > work.width - 2. * margin, "{layout:?} {work:?}");
    assert!(layout.bounds.x >= 0. && layout.bounds.x + layout.bounds.width <= 1600., "{layout:?}");
}

#[test]
fn placements_hint_the_layer_their_drags_may_move() {
    let mut s = placed_photo("hinted placement");
    let photo = s.engine.document().active_layer;
    assert_eq!(s.engine.backend().moving_layer, Some(photo), "a placement hints its photo");
    invoke(&mut s, CommandId::ApplyTransform);
    assert_eq!(s.engine.backend().moving_layer, None, "finishing a placement clears the hint");
}

#[test]
fn photo_placement_bar_offers_original_size_and_counts_a_batch() {
    let mut s = placed_photo("bar placement");
    s.frame(0, 0).unwrap();
    let bar = s.state.canvas_bar.clone().expect("placement bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Placement);
    assert_eq!(
        bar_commands(&bar.items),
        [
            CommandId::PlacementOriginalSize,
            CommandId::TransformFlipHorizontal,
            CommandId::TransformFlipVertical,
            CommandId::TransformRotateLeft,
            CommandId::TransformRotateRight,
            CommandId::ResetTransform,
        ]
    );
    assert_eq!(bar.label, None);
}

#[test]
fn transform_flips_quarter_turns_and_reset_keep_the_box_centred() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    s.dispatch(UiAction::SetToolSetting { id: "transform_angle".into(), value: 0.3 }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "transform_skew".into(), value: 0.2 }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "transform_x".into(), value: 25. }).unwrap();
    s.frame(2, 2).unwrap();
    let preview = |s: &mut UiSession<Recorder>| s.renderer_mut().transform.clone().unwrap().transform.as_affine().unwrap();
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
    let mut s = placed_photo("flip placement");
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
    let selection = rectangle([x0, y0, x1, y1]);
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
    s.frame(1, 1).unwrap();
}

#[test]
fn selection_bar_follows_selection_tools_commands_and_history() {
    let mut s = session(Platform::Gtk);
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
        bar.items.iter().map(|item| (
            bar_commands(std::slice::from_ref(item)).first().copied(), item.label, item.menu,
        )).collect::<Vec<_>>(),
        [
            (Some(CommandId::Deselect), "Deselect", None),
            (Some(CommandId::InvertSelection), "Invert", None),
            (Some(CommandId::CopySelectionToLayer), "Copy to Layer", Some(CanvasBarMenu::CopyToLayer)),
            (Some(CommandId::Copy), "Copy", Some(CanvasBarMenu::Copy)),
            (Some(CommandId::ScaleRotate), "Transform", None),
            (Some(CommandId::FeatherSelection), "Refine", Some(CanvasBarMenu::Refine)),
            (Some(CommandId::MaskSelection), "Mask", None),
            (None, "Adjust", Some(CanvasBarMenu::Adjust)),
            (Some(CommandId::FillSelection), "Fill", None),
            (Some(CommandId::ClearSelected), "Clear", Some(CanvasBarMenu::Clear)),
            (Some(CommandId::CropCanvasToSelection), "Crop", None),
            (Some(CommandId::QuickMask), "Quick Mask", None),
            (Some(CommandId::SaveSelectionLayer), "Save", None),
        ],
        "the selection bar follows its priority order"
    );
    assert_eq!(bar.items.iter().find(|i| i.menu == Some(CanvasBarMenu::Copy)).unwrap().icon, Some("copy"));
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

#[test]
fn a_stroke_that_misses_the_transform_publishes_nothing() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    s.frame(2, 2).unwrap();
    let before = s.operation.quad();
    let outside = Point { x: 900., y: 800. };
    s.transform_pen(event(&s, 1, PenPhase::Down, 1.), outside).unwrap();
    for (sequence, x) in [(2, 910.), (3, 930.)] {
        s.transform_pen(event(&s, sequence, PenPhase::Move, 1.), Point { x, y: 800. }).unwrap();
        assert_eq!(s.frame(sequence, sequence).unwrap().regions, 0);
    }
    s.transform_pen(event(&s, 4, PenPhase::Up, 1.), Point { x: 930., y: 800. }).unwrap();
    assert_eq!(s.frame(4, 4).unwrap().regions, 0);
    assert_eq!(s.operation.quad(), before);
}

#[test]
fn handle_drags_publish_values_and_the_document_only_on_release() {
    for placing in [false, true] {
        let mut s = if placing { placed_photo("placement drag") } else { filled_selection_session() };
        if !placing {
            invoke(&mut s, CommandId::ScaleRotate);
        }
        s.frame(2, 2).unwrap();
        let value = |s: &UiSession<Recorder>| s.state.tool_settings.iter().find(|c| c.id == "transform_x").unwrap().value;
        let placement = |s: &UiSession<Recorder>| s.engine.document().layer(s.engine.document().active_layer).unwrap().properties.placement;
        let before = (value(&s), placement(&s));
        let quad = s.operation.quad();
        let centre = Point { x: (quad[0].x + quad[2].x) * 0.5, y: (quad[0].y + quad[2].y) * 0.5 };
        let moved = Point { x: centre.x + 40., y: centre.y + 20. };
        assert!(!s.hold_canvas_backdrop());
        s.transform_pen(event(&s, 1, PenPhase::Down, 1.), centre).unwrap();
        assert_eq!(s.frame(3, 3).unwrap().regions, 0, "a drag starts without publishing");
        assert!(s.hold_canvas_backdrop());
        assert!(!s.command(CommandId::CancelSelection).enabled, "a transform drag is not a selection gesture");
        s.transform_pen(event(&s, 2, PenPhase::Move, 1.), moved).unwrap();
        let change = s.frame(4, 4).unwrap();
        assert_eq!(change.regions & (regions::BRUSH | regions::DOCUMENT), 0, "drag samples leave the panels alone");
        assert_eq!(value(&s), before.0);
        assert!(placing || s.renderer_mut().transform.clone().unwrap().moving);
        assert!(!placing || placement(&s) != before.1, "the preview still moves the photo");
        s.transform_pen(event(&s, 3, PenPhase::Up, 1.), moved).unwrap();
        let change = s.frame(5, 5).unwrap();
        assert!(!s.hold_canvas_backdrop());
        assert_ne!(change.regions & regions::BRUSH, 0, "release publishes the new values");
        assert!(!placing || change.regions & regions::DOCUMENT != 0, "release publishes the placed photo");
        assert!(placing || change.regions & regions::COMMANDS == 0, "release publishes values, not availability");
        assert!((value(&s) - before.0 - 40.).abs() < 0.01, "{} -> {}", before.0, value(&s));
    }
}

#[test]
fn a_finger_reaches_the_handles_of_every_transform() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    let [x0, y0, x1, y1] = s.transform_document_bounds().unwrap();
    let m = s.state.camera.document_to_surface();
    let surface = |x: f32, y: f32| [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
    let touch = |id, phase, position| pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, position, 0);
    let inside = surface((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    assert!(s.input(touch(1, ContactPhase::Down, inside)).unwrap().paint, "a finger inside the box moves it");
    s.input(touch(1, ContactPhase::Up, inside)).unwrap();
    let outside = surface(x1 + 300., y1 + 300.);
    assert!(!s.input(touch(2, ContactPhase::Down, outside)).unwrap().paint, "elsewhere a finger navigates");
    s.input(touch(2, ContactPhase::Up, outside)).unwrap();
}

#[test]
fn distort_moves_corners_folds_back_and_resets() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    s.frame(2, 2).unwrap();
    let preview = |s: &mut UiSession<Recorder>| s.renderer_mut().transform.clone().unwrap().transform.map;
    invoke(&mut s, CommandId::TransformDistort);
    assert!(s.command(CommandId::TransformDistort).selected);
    let bar = s.state.canvas_bar.clone().unwrap();
    assert!(bar_commands(&bar.items).contains(&CommandId::TransformPerspective), "Perspective appears with Distort");
    let quad = s.operation.quad();
    let target = Point { x: quad[1].x + 40., y: quad[1].y - 30. };
    drag_to(&mut s, quad[1], target);
    s.frame(3, 3).unwrap();
    assert!(matches!(preview(&mut s), layer_core::TransformMap::Projective(_)), "a lone corner drag is perspective");
    let moved = s.operation.quad();
    for (i, corner) in moved.iter().enumerate() {
        let expected = if i == 1 { target } else { quad[i] };
        assert!((corner.x - expected.x).abs() < 0.01 && (corner.y - expected.y).abs() < 0.01, "corner {i}: {corner:?}");
    }
    invoke(&mut s, CommandId::TransformFree);
    assert!(s.operation.distorted(), "a perspective quad stays folded under Free");
    invoke(&mut s, CommandId::ResetTransform);
    s.frame(4, 4).unwrap();
    assert!(s.command(CommandId::TransformFree).selected);
    assert_eq!(preview(&mut s), layer_core::TransformMap::Affine(layer_core::Affine::IDENTITY));
    invoke(&mut s, CommandId::TransformDistort);
    let quad = s.operation.quad();
    let edge = Point { x: (quad[0].x + quad[1].x) * 0.5, y: (quad[0].y + quad[1].y) * 0.5 };
    drag_to(&mut s, edge, Point { x: edge.x + 50., y: edge.y });
    invoke(&mut s, CommandId::TransformFree);
    s.frame(5, 5).unwrap();
    assert!(!s.operation.distorted(), "a parallelogram folds back into the pose exactly");
    assert!(s.operation.shear().abs() > 0.1, "an edge drag skews: {}", s.operation.shear());
    invoke(&mut s, CommandId::CancelTransform);
}

#[test]
fn perspective_mirrors_a_corner_drag_onto_its_neighbour() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    invoke(&mut s, CommandId::TransformDistort);
    invoke(&mut s, CommandId::TransformPerspective);
    let quad = s.operation.quad();
    s.transform_pen(event(&s, 1, PenPhase::Down, 1.), quad[0]).unwrap();
    s.transform_pen(event(&s, 2, PenPhase::Move, 1.), Point { x: quad[0].x + 30., y: quad[0].y + 4. }).unwrap();
    let moved = s.operation.quad();
    assert!((moved[0].x - quad[0].x - 30.).abs() < 0.01 && (moved[0].y - quad[0].y).abs() < 0.01);
    assert!((moved[1].x - quad[1].x + 30.).abs() < 0.01, "the top edge narrows symmetrically");
    assert!((moved[2].x - quad[2].x).abs() < 0.01 && (moved[3].x - quad[3].x).abs() < 0.01);
}

#[test]
fn distort_and_warp_are_refused_on_photo_placements_with_the_route_that_works() {
    let mut s = placed_photo("distort placement");
    for command in [CommandId::TransformDistort, CommandId::TransformWarp] {
        assert!(!s.command(command).enabled);
        assert_eq!(s.command_disabled_reason(command).as_deref(), Some(operation::DISTORT_PLACEMENT));
        assert!(s.dispatch(UiAction::Invoke { command }).is_err());
    }
    assert!(!s.command(CommandId::WarpGridFour).enabled);
    assert!(interpolation_choice(&s).is_none(), "placed photos keep their pixels");
    assert!(!s.command(CommandId::TransformBicubic).enabled);
    assert!(s.dispatch(UiAction::Invoke { command: CommandId::TransformNearest }).is_err());
}

fn interpolation_choice(s: &UiSession<Recorder>) -> Option<(bool, Vec<(&'static str, bool)>)> {
    s.state.canvas_bar.as_ref()?.items.iter().find_map(|item| match &item.option {
        ToolOption::Choice { id: "transform-interpolation", segmented, items, .. } => {
            Some((*segmented, items.iter().map(|i| (i.label, i.selected)).collect()))
        }
        _ => None,
    })
}

#[test]
fn interpolation_follows_the_mode_until_chosen_and_stays_chosen() {
    use layer_core::Interpolation;
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    s.frame(2, 2).unwrap();
    let preview = |s: &mut UiSession<Recorder>| s.renderer_mut().transform.clone().unwrap().transform.interpolation;
    assert_eq!(preview(&mut s), Interpolation::Linear);
    assert_eq!(
        interpolation_choice(&s),
        Some((false, vec![("Nearest", false), ("Bilinear", true), ("Bicubic", false), ("Lanczos", false)]))
    );
    invoke(&mut s, CommandId::TransformDistort);
    s.frame(3, 3).unwrap();
    assert_eq!(preview(&mut s), Interpolation::Bicubic, "Distort defaults to Bicubic");
    assert!(s.command(CommandId::TransformBicubic).selected);
    let quad = s.operation.quad();
    s.transform_pen(event(&s, 1, PenPhase::Down, 1.), quad[2]).unwrap();
    s.transform_pen(event(&s, 2, PenPhase::Move, 1.), Point { x: quad[2].x + 20., y: quad[2].y + 10. }).unwrap();
    s.frame(4, 4).unwrap();
    assert!(s.renderer_mut().transform.clone().unwrap().moving, "a dragged preview is moving");
    s.transform_pen(event(&s, 3, PenPhase::Up, 1.), Point { x: quad[2].x + 20., y: quad[2].y + 10. }).unwrap();
    s.frame(5, 5).unwrap();
    assert!(!s.renderer_mut().transform.clone().unwrap().moving, "the still preview draws the chosen filter");
    let bar = s.state.canvas_bar.clone().unwrap();
    let menu = s.canvas_bar_choice_menu(bar.context, "transform-interpolation").unwrap();
    assert_eq!(
        menu.sections[0].iter().map(|i| (i.label.as_str(), i.selected)).collect::<Vec<_>>(),
        [("Nearest", Some(false)), ("Bilinear", Some(false)), ("Bicubic", Some(true)), ("Lanczos", Some(false))]
    );
    s.dispatch(menu.sections[0][3].action.clone().unwrap()).unwrap();
    s.frame(6, 6).unwrap();
    assert_eq!(preview(&mut s), Interpolation::Lanczos);
    assert!(s.command(CommandId::TransformLanczos).selected);
    s.dispatch(menu.sections[0][0].action.clone().unwrap()).unwrap();
    s.frame(6, 6).unwrap();
    assert_eq!(preview(&mut s), Interpolation::Nearest);
    invoke(&mut s, CommandId::TransformFree);
    s.frame(7, 7).unwrap();
    assert_eq!(preview(&mut s), Interpolation::Nearest, "a chosen interpolation outlasts mode changes");
    invoke(&mut s, CommandId::CancelTransform);
    invoke(&mut s, CommandId::ScaleRotate);
    s.frame(8, 8).unwrap();
    assert_eq!(preview(&mut s), Interpolation::Nearest, "and later transforms");
    assert!(s.state.tool_actions.iter().any(|a| a.command == CommandId::TransformBicubic));
}

fn distorted_pixel_selection() -> UiSession<Recorder> {
    let mut s = filled_selection_session();
    let doc = s.engine.document();
    let extent = doc.target_extent(doc.active_layer);
    let row = extent[0].div_ceil(4) as usize;
    let mut words = vec![0u32; row * extent[1] as usize];
    for y in 100..300 {
        for x in (100..300).step_by(4) {
            words[y * row + x / 4] = u32::MAX;
        }
    }
    let pixels = layer_core::SelectionPixels::bytes(extent, [100, 100, 300, 300], words).unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(layer_core::Selection::pixels(std::sync::Arc::new(pixels)))))
        .unwrap();
    invoke(&mut s, CommandId::ScaleRotate);
    s.frame(2, 2).unwrap();
    invoke(&mut s, CommandId::TransformDistort);
    let quad = s.operation.quad();
    drag_to(&mut s, quad[1], Point { x: quad[1].x + 40., y: quad[1].y - 30. });
    s.frame(3, 3).unwrap();
    s
}

fn resampled_reply(s: &mut UiSession<Recorder>) -> layer_render::RegionResult {
    let request = s.renderer_mut().region_requests.last().cloned().expect("a coverage request");
    assert!(matches!(request.source, layer_render::RegionSource::TransformedSelection { .. }));
    let doc = s.engine.document();
    let extent = doc.target_extent(doc.active_layer);
    let words = vec![u32::MAX; extent[0].div_ceil(4) as usize * extent[1] as usize];
    layer_render::RegionResult {
        tonal_sample: None,
        placement: layer_core::Affine::IDENTITY,
        request_id: request.request_id,
        pixels: std::sync::Arc::new(layer_core::SelectionPixels::bytes(extent, [0, 0, extent[0], extent[1]], words).unwrap()),
    }
}

#[test]
fn applying_a_distorted_pixel_selection_waits_for_its_resampled_coverage() {
    let mut s = distorted_pixel_selection();
    let before = s.engine.document().layers.clone();
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(4, 4).unwrap();
    assert!(s.operation.active(), "the transform stays open until its coverage returns");
    assert!(!s.command(CommandId::ApplyTransform).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::ApplyTransform).as_deref(), Some("Applying the transform"));
    assert!(s.command(CommandId::CancelTransform).enabled);
    let reply = resampled_reply(&mut s);
    s.renderer_mut().region_reply = Some(reply);
    let change = s.frame(5, 5).unwrap();
    assert!(!s.operation.active());
    assert_ne!(change.regions & regions::DOCUMENT, 0);
    assert_ne!(change.regions & regions::BRUSH, 0, "Tool Options follow the Apply in the same frame");
    assert!(s.engine.document().selection.is_some());
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers, before, "Apply is one undo step");
}

#[test]
fn cancelling_or_editing_a_pending_apply_discards_its_coverage() {
    let mut s = distorted_pixel_selection();
    let before = s.engine.document().clone();
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(4, 4).unwrap();
    let reply = resampled_reply(&mut s);
    invoke(&mut s, CommandId::CancelTransform);
    s.renderer_mut().region_reply = Some(reply);
    s.frame(5, 5).unwrap();
    assert!(!s.operation.active());
    assert_eq!(s.engine.document().layers, before.layers, "a cancelled Apply edits nothing");

    let mut s = distorted_pixel_selection();
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(4, 4).unwrap();
    let reply = resampled_reply(&mut s);
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    assert!(s.command(CommandId::ApplyTransform).enabled, "an edit supersedes the pending Apply");
    s.renderer_mut().region_reply = Some(reply);
    s.frame(5, 5).unwrap();
    assert!(s.operation.active(), "the stale coverage is discarded");
}

#[test]
fn a_press_that_moves_nothing_keeps_the_pending_apply() {
    let mut s = distorted_pixel_selection();
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(4, 4).unwrap();
    let reply = resampled_reply(&mut s);
    let quad = s.operation.quad();
    let centre = Point { x: (quad[0].x + quad[2].x) * 0.5, y: (quad[0].y + quad[2].y) * 0.5 };
    for corner in [centre, quad[2]] {
        drag_to(&mut s, corner, corner);
        s.frame(5, 5).unwrap();
        assert!(!s.command(CommandId::ApplyTransform).enabled, "the Apply is still pending");
    }
    s.renderer_mut().region_reply = Some(reply);
    s.frame(6, 6).unwrap();
    assert!(!s.operation.active(), "the pending Apply completes");
}

#[test]
fn a_failed_apply_leaves_the_transform_ready_to_apply_again() {
    let mut s = distorted_pixel_selection();
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(4, 4).unwrap();
    let reply = resampled_reply(&mut s);
    s.renderer_mut().region_reply = Some(reply.clone());
    s.renderer_mut().region_fails = true;
    assert!(s.frame(5, 5).is_err(), "the failed readback is reported");
    s.renderer_mut().region_fails = false;
    assert!(s.command(CommandId::ApplyTransform).enabled, "a failed readback ends the pending Apply");
    s.frame(6, 6).unwrap();
    assert!(s.state.commands.iter().any(|c| c.id == CommandId::ApplyTransform && c.enabled));

    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(7, 7).unwrap();
    let pixels = layer_core::SelectionPixels::bytes([4, 4], [0, 0, 4, 4], vec![u32::MAX; 4]).unwrap();
    s.renderer_mut().region_reply = Some(layer_render::RegionResult {
        request_id: s.renderer_mut().region_requests.last().unwrap().request_id,
        pixels: std::sync::Arc::new(pixels),
        ..reply
    });
    let change = s.frame(8, 8).unwrap();
    assert!(s.operation.active() && s.state.host_error.is_some(), "a refused result keeps the transform open");
    assert_ne!(change.regions & regions::COMMANDS, 0, "the same frame publishes Apply again");
    assert!(s.state.commands.iter().any(|c| c.id == CommandId::ApplyTransform && c.enabled));
}

fn preview_map(s: &mut UiSession<Recorder>) -> layer_core::TransformMap {
    s.renderer_mut().transform.clone().unwrap().transform.map
}

fn close(a: Point, b: Point, tolerance: f32) -> bool {
    (a.x - b.x).abs() <= tolerance && (a.y - b.y).abs() <= tolerance
}

fn drag_to(s: &mut UiSession<Recorder>, from: Point, to: Point) {
    s.transform_pen(event(s, 1, PenPhase::Down, 1.), from).unwrap();
    s.transform_pen(event(s, 2, PenPhase::Move, 1.), to).unwrap();
    s.transform_pen(event(s, 3, PenPhase::Up, 1.), to).unwrap();
}

#[test]
fn warp_seeds_from_the_transform_and_bends_through_nodes_and_tangents() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    invoke(&mut s, CommandId::TransformRotateRight);
    s.frame(2, 2).unwrap();
    let rotated = preview_map(&mut s);
    invoke(&mut s, CommandId::TransformWarp);
    s.frame(3, 3).unwrap();
    assert!(s.command(CommandId::TransformWarp).selected);
    assert!(s.command(CommandId::WarpGridThree).selected, "three by three cells by default");
    assert_eq!(s.command_disabled_reason(CommandId::TransformPerspective).as_deref(), Some("Choose Distort first"));
    assert_eq!(s.transform_interpolation(), Some(layer_core::Interpolation::Bicubic));
    let mesh = s.operation.mesh().expect("Warp seeds a mesh");
    assert_eq!(mesh.node_count(), 16);
    for corner in [Point { x: 150., y: 150. }, Point { x: 250., y: 180. }, Point { x: 200., y: 250. }] {
        assert!(close(mesh.map(corner).unwrap(), rotated.map(corner).unwrap(), 0.01), "the seed keeps the quarter turn");
    }
    let bar = s.state.canvas_bar.clone().unwrap();
    assert!(bar.items.iter().any(|i| matches!(i.option, ToolOption::Choice { id: "transform-warp-grid", .. })));
    assert!(s.state.tool_settings.is_empty(), "the pose fields do not apply to a mesh");

    let node = mesh.node(5).unwrap();
    let moved = Point { x: node.x + 30., y: node.y + 20. };
    drag_to(&mut s, node, moved);
    s.frame(4, 4).unwrap();
    let mesh = s.operation.mesh().unwrap();
    assert!(close(mesh.node(5).unwrap(), moved, 0.01), "a node follows the pen");
    assert!(matches!(preview_map(&mut s), layer_core::TransformMap::Mesh(_)));
    let tangent = mesh.tangent(5, 0).expect("the pressed node shows its tangents");
    let pulled = Point { x: tangent.x, y: tangent.y + 25. };
    drag_to(&mut s, tangent, pulled);
    assert!(close(s.operation.mesh().unwrap().tangent(5, 0).unwrap(), pulled, 0.01), "a tangent handle follows the pen");

    let probe = Point { x: 200., y: 200. };
    let before = s.operation.mesh().unwrap().map(probe).unwrap();
    invoke(&mut s, CommandId::WarpGridFour);
    let refit = s.operation.mesh().unwrap();
    assert_eq!(refit.node_count(), 25);
    assert!(close(refit.map(probe).unwrap(), before, 0.5), "a new grid keeps the shape");

    invoke(&mut s, CommandId::TransformFree);
    s.frame(5, 5).unwrap();
    assert!(s.operation.mesh().is_some(), "leaving Warp keeps the mesh");
    let hull = s.operation.quad();
    let centre = Point { x: (hull[0].x + hull[2].x) * 0.5, y: (hull[0].y + hull[2].y) * 0.5 };
    drag_to(&mut s, centre, Point { x: centre.x + 10., y: centre.y });
    s.frame(6, 6).unwrap();
    let layer_core::TransformMap::Mesh(moved) = preview_map(&mut s) else { panic!("the mesh stays under the box") };
    assert!(close(moved.map(probe).unwrap(), Point { x: before.x + 10., y: before.y }, 0.5), "the box moves the warped content");

    invoke(&mut s, CommandId::TransformWarp);
    invoke(&mut s, CommandId::TransformFlipHorizontal);
    let flipped = s.operation.mesh().unwrap();
    let hull = flipped.bounds();
    assert!(close(flipped.map(probe).unwrap(), Point { x: hull.min.x + hull.max.x - before.x - 10., y: before.y }, 0.5), "a flip mirrors the mesh about its hull");

    invoke(&mut s, CommandId::ResetTransform);
    s.frame(7, 7).unwrap();
    assert!(s.operation.mesh().is_none());
    assert!(s.command(CommandId::TransformFree).selected);
    invoke(&mut s, CommandId::TransformWarp);
    invoke(&mut s, CommandId::ApplyTransform);
    assert!(!s.operation.active(), "an unbent warp applies at once");
}

fn menu_labels(menu: &ContextMenu) -> Vec<Vec<&str>> {
    menu.sections.iter().map(|section| section.iter().map(|i| i.label.as_str()).collect()).collect()
}

fn find_item<'a>(sections: &'a [Vec<ContextMenuItem>], label: &str) -> Option<&'a ContextMenuItem> {
    sections.iter().flatten().find_map(|item| {
        if item.label == label {
            Some(item)
        } else {
            find_item(&item.sections, label)
        }
    })
}

#[test]
fn selection_bar_menus_list_their_commands_and_refuse_stale_edits() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::Move);
    let bar = s.state.canvas_bar.clone().unwrap();
    let wrapped = |command| {
        Some(UiAction::CanvasBarEdit {
            context: bar.context,
            action: Box::new(UiAction::Invoke { command }),
        })
    };
    let copy = s.canvas_bar_choice_menu(bar.context, "copy_to_layer").unwrap();
    assert_eq!(menu_labels(&copy), [["Copy Selection to New Layer", "Cut Selection to New Layer"]]);
    assert_eq!(copy.sections[0][0].action, wrapped(CommandId::CopySelectionToLayer));
    assert_eq!(copy.sections[0][0].hint, "Ctrl+J", "menus show the command's keys");
    let clear = s.canvas_bar_choice_menu(bar.context, "clear").unwrap();
    assert_eq!(menu_labels(&clear), [["Clear Selected Pixels", "Clear Outside Selection"]]);
    assert_eq!(clear.sections[0][1].action, wrapped(CommandId::ClearOutside));
    let refine = s.canvas_bar_choice_menu(bar.context, "refine").unwrap();
    assert_eq!(
        menu_labels(&refine),
        [vec!["Grow…", "Shrink…", "Feather…", "Border…", "Smooth…"], vec!["Transform Outline"]]
    );
    assert_eq!(refine.sections[0][2].action, wrapped(CommandId::FeatherSelection));
    assert_eq!(refine.sections[1][0].action, wrapped(CommandId::TransformSelectionOutline));
    assert!(refine.sections.concat().iter().all(|i| i.enabled));
    let primary = bar.items.iter().find(|i| i.menu == Some(CanvasBarMenu::Refine)).unwrap();
    assert!(
        matches!(&primary.option, ToolOption::Action { state, .. } if state.id == CommandId::FeatherSelection),
        "a host that cannot open the menu runs Feather"
    );
    let adjust = s.canvas_bar_choice_menu(bar.context, "adjust").unwrap();
    assert_eq!(
        adjust.sections[0].iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        s.application_menu(ApplicationMenu::Filter).sections[0].iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        "Adjust lists the Filter menu's categories"
    );
    let curves = find_item(&adjust.sections, "Curves").expect("Curves in its category");
    assert!(matches!(
        curves.action.as_ref(),
        Some(UiAction::CanvasBarEdit { action, .. }) if matches!(**action, UiAction::Effect { .. })
    ));
    let copy = s.canvas_bar_choice_menu(bar.context, "copy").unwrap();
    assert_eq!(menu_labels(&copy), [["Copy", "Copy Merged", "Cut"]]);
    for item in &bar.items {
        if let (Some(menu), ToolOption::Choice { id, items, segmented, .. }) = (item.menu, &item.option) {
            assert_eq!((*id, items.is_empty(), *segmented), (menu.id(), true, false), "a menu without a primary command");
        }
    }

    let more = s.canvas_bar_menu(bar.context, 0).unwrap();
    let overflow = find_item(&more.sections, "Clear").expect("an overflowed menu is a submenu of More");
    assert_eq!(
        overflow.sections.concat().iter().map(|i| (&i.label, &i.action)).collect::<Vec<_>>(),
        clear.sections.concat().iter().map(|i| (&i.label, &i.action)).collect::<Vec<_>>()
    );

    let reject = |s: &mut UiSession<Recorder>, context, action| {
        s.dispatch(UiAction::CanvasBarEdit { context, action: Box::new(action) }).is_err()
    };
    assert!(reject(&mut s, bar.context, UiAction::Invoke { command: CommandId::ClearLayer }), "only the bar's own actions");
    let stale = CanvasBarContext { generation: bar.context.generation + 1, ..bar.context };
    assert!(reject(&mut s, stale, UiAction::Invoke { command: CommandId::ClearOutside }), "another bar's menu");
    assert!(s.canvas_bar_choice_menu(stale, "clear").is_none());
    let before = s.engine.document().layers.clone();
    s.dispatch(clear.sections[0][1].action.clone().unwrap()).unwrap();
    s.frame(3, 3).unwrap();
    let operations = &s.engine.backend().pending_operations;
    assert!(matches!(operations[..], [(_, layer_core::LayerOperation { kind: layer_core::LayerOperationKind::Erase { .. }, .. })]));
    assert!(operations[0].1.coverage.initial.as_ref().unwrap().inverted, "Clear Outside erases the inverse");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers, before);
    invoke(&mut s, CommandId::Deselect);
    assert!(s.state.canvas_bar.is_none());
    assert!(reject(&mut s, bar.context, UiAction::Invoke { command: CommandId::ClearSelected }), "a bar that has gone");
}

#[test]
fn adjust_on_the_selection_bar_masks_the_new_effect_to_the_selection() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::Move);
    let bar = s.state.canvas_bar.clone().unwrap();
    let before = s.engine.document().clone();
    let adjust = s.canvas_bar_choice_menu(bar.context, "adjust").unwrap();
    let curves = find_item(&adjust.sections, "Curves").unwrap().action.clone().unwrap();
    s.dispatch(curves).unwrap();
    s.frame(2, 2).unwrap();
    let doc = s.engine.document();
    let effect = doc.layer(doc.active_layer).unwrap();
    assert_eq!(effect.kind, LayerKind::Effect);
    let mask = effect.mask.as_ref().expect("the selection becomes the effect's mask");
    assert_eq!(mask.initial, before.selection);
    assert_eq!(mask.default_coverage, 0.);
    assert!(doc.selection.is_none(), "the mask consumes the selection");
    assert!(s.command(CommandId::Reselect).enabled);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers, before.layers, "one undo step removes the masked effect");
    assert_eq!(s.engine.document().selection, before.selection, "and restores the selection");
}

fn bar_edit(s: &mut UiSession<Recorder>, command: CommandId) {
    let context = s.state.canvas_bar.as_ref().expect("a canvas bar").context;
    s.dispatch(UiAction::CanvasBarEdit { context, action: Box::new(UiAction::Invoke { command }) }).unwrap();
    s.frame(9, 9).unwrap();
}

fn bar_items(items: &[CanvasBarItem]) -> Vec<(CommandId, &'static str, bool)> {
    items
        .iter()
        .map(|item| match &item.option {
            ToolOption::Action { state, checkable } => (state.id, item.label, *checkable),
            ToolOption::Choice { .. } => (CommandId::SearchCommands, item.label, false),
            ToolOption::Numeric(_) | ToolOption::Range { .. } => unreachable!("bars hold no values"),
        })
        .collect()
}

fn mode_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    rectangle_selection(&mut s, [100., 100., 300., 250.]);
    s
}

#[test]
fn quick_mask_bar_offers_its_actions_and_exit_under_painting_tools() {
    let mut s = mode_session();
    invoke(&mut s, CommandId::QuickMask);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Paint, "Quick Mask paints");
    let bar = s.state.canvas_bar.clone().expect("the Quick Mask bar");
    assert_eq!(bar.context.kind, CanvasBarKind::QuickMask);
    assert_eq!(bar.label.as_deref(), Some("Quick Mask"));
    assert_eq!((bar.placement, bar.anchor), (CanvasBarPlacement::BottomEdge, None));
    assert_eq!(
        bar_items(&bar.items),
        [
            (CommandId::InvertSelection, "Invert", false),
            (CommandId::FillSelectionMask, "Fill", false),
            (CommandId::ClearSelectionMask, "Clear", false),
            (CommandId::FeatherSelection, "Refine", false),
            (CommandId::SaveSelectionLayer, "Save", false),
        ]
    );
    assert_eq!(bar.items[3].menu, Some(CanvasBarMenu::Refine));
    assert_eq!(bar_items(&bar.completion), [(CommandId::ReturnToArtwork, "Exit", false)]);
    assert!(bar.completion[0].accent, "the exit finishes the mode");
    let more = s.canvas_bar_menu(bar.context, bar.items.len()).unwrap();
    assert!(find_item(&more.sections, "Swap Mask Colors").is_none());
    assert!(find_item(&more.sections, "Return to Artwork").is_some(), "More lists the Quick Mask menu");

    invoke(&mut s, CommandId::Eraser);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::QuickMask), "any painting tool");
    let covered = s.current_selection().unwrap();
    let context = s.state.canvas_bar.as_ref().unwrap().context;
    bar_edit(&mut s, CommandId::InvertSelection);
    assert!(s.selection_masks.quick(), "Invert stays in Quick Mask");
    assert_eq!(s.current_selection().unwrap().inverted, !covered.inverted);
    assert_eq!(s.state.canvas_bar.as_ref().unwrap().context, context, "an edit keeps the bar");
    bar_edit(&mut s, CommandId::ReturnToArtwork);
    assert!(s.selection_masks.target().is_none());
    assert_ne!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::QuickMask));
}

#[test]
fn mode_bars_hide_when_the_bar_is_turned_off() {
    let mut s = mode_session();
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    invoke(&mut s, CommandId::QuickMask);
    assert!(s.state.canvas_bar.is_none(), "unlike a transform's Apply and Cancel, a mode's exit hides with the bar");
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::QuickMask));
    invoke(&mut s, CommandId::SaveSelectionLayer);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::SelectionLayer));
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    assert!(s.state.canvas_bar.is_none());
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    invoke(&mut s, CommandId::ReturnToArtwork);
    invoke(&mut s, CommandId::MaskSelection);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::LayerMask));
    invoke(&mut s, CommandId::ShowCanvasActionBar);
    assert!(s.state.canvas_bar.is_none());
}

#[test]
fn selection_layer_bar_inverts_loads_and_returns_to_artwork() {
    let mut s = mode_session();
    let artwork = s.engine.document().active_layer;
    invoke(&mut s, CommandId::SaveSelectionLayer);
    let id = s.engine.document().active_layer;
    let name = s.engine.document().layer(id).unwrap().name.to_string();
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Paint);
    let bar = s.state.canvas_bar.clone().expect("the Selection Layer bar");
    assert_eq!(bar.context.kind, CanvasBarKind::SelectionLayer);
    assert_eq!(bar.label, Some(format!("Editing {name}")));
    assert_eq!(bar.placement, CanvasBarPlacement::BottomEdge);
    assert_eq!(
        bar_items(&bar.items),
        [(CommandId::LoadSelectionLayer, "Load", false), (CommandId::InvertSelectionLayer, "Invert", false)]
    );
    assert_eq!(bar_items(&bar.completion), [(CommandId::ReturnToArtwork, "Return to Artwork", false)]);
    let menu = s.layer_menu(id.0, false).unwrap();
    assert_eq!(
        find_item(&menu.sections, "Invert").and_then(|i| i.action.clone()),
        Some(UiAction::Invoke { command: CommandId::InvertSelectionLayer }),
        "the edited layer's menu routes to the commands"
    );
    assert_eq!(
        find_item(&menu.sections, "Load Selection").and_then(|i| i.sections.first()).map(|l| l[0].action.clone()),
        Some(Some(UiAction::Invoke { command: CommandId::LoadSelectionLayer }))
    );
    let stored = s.engine.document().saved_selection(id).unwrap();

    bar_edit(&mut s, CommandId::InvertSelectionLayer);
    assert_eq!(s.selection_masks.target(), Some(layer_core::SelectionTarget::Saved(id)), "Invert stays in the mode");
    assert_eq!(s.engine.document().saved_selection(id).unwrap().inverted, !stored.inverted);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().saved_selection(id).unwrap(), stored, "one undo step");
    s.layer_action(LayerAction::Select { id: id.0, mask: false }).unwrap();

    invoke(&mut s, CommandId::Deselect);
    s.layer_action(LayerAction::Select { id: id.0, mask: false }).unwrap();
    s.frame(8, 8).unwrap();
    bar_edit(&mut s, CommandId::LoadSelectionLayer);
    assert!(s.selection_masks.target().is_none(), "Load returns to the artwork");
    assert_eq!(s.engine.document().active_layer, artwork);
    assert_eq!(s.engine.document().selection.as_ref(), Some(&stored));
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().selection.is_none(), "one undo step");

    s.layer_action(LayerAction::Select { id: id.0, mask: false }).unwrap();
    s.frame(8, 8).unwrap();
    bar_edit(&mut s, CommandId::ReturnToArtwork);
    assert!(s.selection_masks.target().is_none());
    assert!(s.state.canvas_bar.is_none());
    assert!(!s.command(CommandId::LoadSelectionLayer).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::InvertSelectionLayer).as_deref(), Some("Edit a Selection Layer first"));
}

#[test]
fn layer_mask_bar_inverts_disables_applies_and_edits_content() {
    let mut s = mode_session();
    let id = s.engine.document().active_layer;
    invoke(&mut s, CommandId::MaskSelection);
    let mask = |s: &UiSession<Recorder>| s.engine.document().layer(id).unwrap().mask.clone();
    assert!(s.engine.document().active_mask);
    let name = s.engine.document().layer(id).unwrap().name.to_string();
    let bar = s.state.canvas_bar.clone().expect("the mask bar");
    assert_eq!(bar.context.kind, CanvasBarKind::LayerMask);
    assert_eq!(bar.label, Some(format!("Editing {name} mask")));
    assert_eq!(bar.placement, CanvasBarPlacement::BottomEdge);
    assert_eq!(
        bar_items(&bar.items),
        [
            (CommandId::InvertLayerMask, "Invert", false),
            (CommandId::LayerMaskEnabled, "Disable", false),
            (CommandId::ApplyLayerMask, "Apply Mask", false),
        ]
    );
    assert_eq!(bar_items(&bar.completion), [(CommandId::EditLayerContent, "Edit Content", false)]);
    assert!(s.command(CommandId::LayerMaskEnabled).checkable && s.command(CommandId::LayerMaskEnabled).selected);
    let menu = s.layer_menu(id.0, true).unwrap();
    for (label, command) in [
        ("Edit layer content", CommandId::EditLayerContent),
        ("Enable mask", CommandId::LayerMaskEnabled),
        ("Invert mask", CommandId::InvertLayerMask),
        ("Apply mask to layer", CommandId::ApplyLayerMask),
    ] {
        let item = find_item(&menu.sections, label).unwrap();
        assert_eq!(item.action, Some(UiAction::Invoke { command }), "{label}");
    }
    assert_eq!(find_item(&menu.sections, "Enable mask").unwrap().selected, Some(true));

    let before = mask(&s).unwrap();
    bar_edit(&mut s, CommandId::InvertLayerMask);
    assert_eq!(mask(&s).unwrap().inverted, !before.inverted);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(mask(&s), Some(before.clone()), "Invert is one undo step");

    bar_edit(&mut s, CommandId::LayerMaskEnabled);
    assert!(!mask(&s).unwrap().enabled);
    let disabled = s.state.canvas_bar.clone().unwrap();
    assert_eq!(disabled.context, bar.context);
    assert_eq!(disabled.items[1].label, "Enable");
    assert_eq!(s.command_disabled_reason(CommandId::ApplyLayerMask).as_deref(), Some("Enable the mask before applying it"));
    invoke(&mut s, CommandId::Undo);
    assert!(mask(&s).unwrap().enabled, "Disable is one undo step");

    let layers = s.engine.document().layers.clone();
    bar_edit(&mut s, CommandId::ApplyLayerMask);
    assert!(mask(&s).is_none());
    assert!(s
        .engine
        .backend()
        .pending_operations
        .iter()
        .any(|(layer, o)| *layer == id && matches!(o.kind, layer_core::LayerOperationKind::ApplyMask)));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers, layers, "Apply Mask is one undo step");

    s.layer_action(LayerAction::Select { id: id.0, mask: true }).unwrap();
    s.frame(8, 8).unwrap();
    let steps = s.engine.can_undo();
    bar_edit(&mut s, CommandId::EditLayerContent);
    assert!(!s.engine.document().active_mask);
    assert!(s.state.canvas_bar.is_none());
    assert_eq!(s.command_disabled_reason(CommandId::EditLayerContent).as_deref(), Some("Already editing the layer content"));
    let content = s.layer_menu(id.0, false).unwrap();
    let edit = find_item(&content.sections, "Edit mask").unwrap();
    assert_eq!(edit.action, Some(UiAction::Invoke { command: CommandId::EditLayerMask }));
    invoke(&mut s, CommandId::EditLayerMask);
    assert!(s.engine.document().active_mask);
    assert_eq!(s.engine.can_undo(), steps, "switching the target is navigation, not history");
    assert_eq!(s.command_disabled_reason(CommandId::EditLayerMask).as_deref(), Some("Already editing the layer mask"));
}

#[test]
fn escape_leaves_selection_layers_and_masks_after_cancelling_a_gesture() {
    let mut s = mode_session();
    invoke(&mut s, CommandId::SaveSelectionLayer);
    assert!(s.selection_masks.target().is_some());
    invoke(&mut s, CommandId::Gradient);
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
    assert!(key(&mut s, "Escape", true, false, false).handled);
    key(&mut s, "Escape", false, false, false);
    assert!(s.layer_interaction.path.is_empty(), "Escape cancels the gradient");
    assert!(s.selection_masks.target().is_some(), "and only the gradient");
    s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
    s.frame(3, 3).unwrap();
    assert!(key(&mut s, "Escape", true, false, false).handled);
    key(&mut s, "Escape", false, false, false);
    assert!(s.selection_masks.target().is_none(), "a second Escape leaves Selection Layer editing");

    invoke(&mut s, CommandId::MaskSelection);
    invoke(&mut s, CommandId::ScaleRotate);
    assert!(s.operation.active());
    key(&mut s, "Escape", true, false, false);
    key(&mut s, "Escape", false, false, false);
    assert!(!s.operation.active(), "Escape cancels the mask transform");
    assert!(s.engine.document().active_mask, "without leaving the mask");
    s.state.customization.header_editing = true;
    key(&mut s, "Escape", true, false, false);
    key(&mut s, "Escape", false, false, false);
    assert!(s.engine.document().active_mask, "Escape keeps closing other things first");
    s.state.customization.header_editing = false;
    assert!(key(&mut s, "Escape", true, false, true).change.regions & regions::DOCUMENT == 0);
    key(&mut s, "Escape", false, false, true);
    assert!(s.engine.document().active_mask, "a focused text field keeps Escape");
    assert!(key(&mut s, "Escape", true, false, false).handled);
    key(&mut s, "Escape", false, false, false);
    assert!(!s.engine.document().active_mask, "Escape returns to the layer content");
}

fn guide_pen(s: &mut UiSession<Recorder>, sequence: u64, phase: PenPhase, [x, y]: [f32; 2]) {
    let m = s.state.camera.document_to_surface();
    let mut e = event(s, sequence, phase, 1.);
    e.surface_position = Point { x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5] };
    s.pen(e).unwrap();
    s.frame(sequence, sequence).unwrap();
}

#[test]
fn guide_bar_anchors_to_the_selected_guide_under_ruler_and_move() {
    let mut s = mode_session();
    invoke(&mut s, CommandId::Ruler);
    guide_pen(&mut s, 1, PenPhase::Down, [200., 400.]);
    guide_pen(&mut s, 2, PenPhase::Move, [300., 450.]);
    guide_pen(&mut s, 3, PenPhase::Up, [400., 500.]);
    let bar = s.state.canvas_bar.clone().expect("the guide bar");
    assert_eq!(bar.context.kind, CanvasBarKind::Guide);
    assert_eq!(bar.label, None);
    assert_eq!(bar.placement, CanvasBarPlacement::NearObject);
    let [x0, y0, x1, y1] = bar.anchor.unwrap();
    assert!((x0 - 200.).abs() < 0.01 && (y0 - 400.).abs() < 0.01 && (x1 - 400.).abs() < 0.01 && (y1 - 500.).abs() < 0.01);
    assert_eq!(
        bar_items(&bar.items),
        [
            (CommandId::DeleteRuler, "Delete", false),
            (CommandId::SnapRulers, "Snap", true),
            (CommandId::ShowRulers, "Guides", true),
            (CommandId::StraightenToGuide, "Straighten", false),
        ]
    );
    assert!(bar.completion.is_empty());
    let layout = s.canvas_bar_layout(&CanvasBarMeasure { completion: Vec::new(), ..measure(&bar, 80.) }).unwrap();
    let (a, b) = s.engine.document().rulers[0].geometry.handles();
    let lowest = [a, b.unwrap()].map(|p| s.document_to_logical()(p)[1]).into_iter().fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(layout.side, CanvasBarSide::Below);
    assert!(layout.bounds.y > lowest + crate::session::rulers::HIT_DISTANCE, "clear of the lower handle");
    let actions = |s: &UiSession<Recorder>| s.state.tool_actions.iter().map(|a| a.command).collect::<Vec<_>>();
    assert_eq!(actions(&s), [CommandId::ShowRulers, CommandId::SnapRulers, CommandId::DeleteRuler]);

    invoke(&mut s, CommandId::Brush);
    assert!(s.state.canvas_bar.is_none(), "painting tools leave the guide alone");
    assert!(actions(&s).is_empty(), "guide actions leave Tool Options under unrelated tools");
    invoke(&mut s, CommandId::Move);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::Guide), "above the selection bar");
    assert_eq!(actions(&s), [CommandId::MoveLeaveCopy, CommandId::ShowRulers, CommandId::SnapRulers, CommandId::DeleteRuler]);
    let overlay = |s: &UiSession<Recorder>| {
        let mut segments = Vec::new();
        s.append_ruler_overlay(&mut segments);
        segments.iter().filter(|segment| segment.marker == 1.).count()
    };
    assert!(overlay(&s) > 0, "Move highlights the selected guide");

    guide_pen(&mut s, 4, PenPhase::Down, [700., 150.]);
    guide_pen(&mut s, 5, PenPhase::Up, [700., 150.]);
    assert!(s.rulers.selected.is_none(), "a Move click that misses the guide deselects it");
    assert_eq!(overlay(&s), 0);
    assert_eq!(actions(&s), [CommandId::MoveLeaveCopy]);
    assert_eq!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::Selection));

    invoke(&mut s, CommandId::Ruler);
    guide_pen(&mut s, 6, PenPhase::Down, [400., 500.]);
    guide_pen(&mut s, 7, PenPhase::Up, [400., 500.]);
    let reselected = s.state.canvas_bar.clone().unwrap();
    assert_eq!(reselected.context.kind, CanvasBarKind::Guide);
    let rulers = s.engine.document().rulers.clone();
    bar_edit(&mut s, CommandId::DeleteRuler);
    assert!(s.engine.document().rulers.is_empty());
    assert_ne!(s.state.canvas_bar.as_ref().map(|b| b.context.kind), Some(CanvasBarKind::Guide));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().rulers, rulers, "Delete is one undo step");
    assert!(s
        .dispatch(UiAction::CanvasBarEdit {
            context: reselected.context,
            action: Box::new(UiAction::Invoke { command: CommandId::DeleteRuler }),
        })
        .is_err(), "a deleted guide's bar is stale");
}

#[test]
fn new_mode_commands_follow_the_command_checklist() {
    let s = session(Platform::Gtk);
    let rows = crate::shortcuts::definitions(Platform::Gtk);
    for (command, section) in [
        (CommandId::LoadSelectionLayer, "Select"),
        (CommandId::InvertSelectionLayer, "Select"),
        (CommandId::InvertLayerMask, "Layer"),
        (CommandId::LayerMaskEnabled, "Layer"),
        (CommandId::ApplyLayerMask, "Layer"),
        (CommandId::EditLayerMask, "Layer"),
        (CommandId::EditLayerContent, "Layer"),
        (CommandId::LassoFill, "Tools"),
    ] {
        let row = rows.iter().find(|(d, _)| d.id == command.shortcut_id()).unwrap();
        assert_eq!(row.1, section, "{command:?}");
        assert!(s.state.settings.command_keys(command).is_empty(), "{command:?} has no default keys");
        assert!(!crate::customization::tool_choice(ToolbarControl::Command { command }).description.is_empty());
        let entry = s.command_catalog().into_iter().find(|d| d.id == command_catalog::identity(&UiAction::Invoke { command })).unwrap();
        assert!(!entry.description.is_empty(), "{command:?}");
        assert!(entry.disabled_reason.as_deref().is_none_or(|r| r != "Unavailable in the current tool or edit target"), "{command:?}");
    }
}

#[test]
fn lasso_fill_is_a_tool_command() {
    let mut s = session(Platform::Gtk);
    assert!(CommandId::TOOLS.contains(&CommandId::LassoFill));
    invoke(&mut s, CommandId::LassoFill);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::LassoFill);
    assert!(s.command(CommandId::LassoFill).selected);
    let action = UiAction::Layer { action: LayerAction::Tool { tool: LayerCanvasTool::LassoFill } };
    assert_eq!(crate::shortcuts::tool_command(&action), Some(CommandId::LassoFill));
    invoke(&mut s, CommandId::QuickMask);
    assert_eq!(s.command_disabled_reason(CommandId::LassoFill).as_deref(), Some("Return to the artwork first"));
}
