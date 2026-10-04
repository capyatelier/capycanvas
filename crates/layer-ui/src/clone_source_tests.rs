fn clone_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.set_viewport([1000., 1000.], [1000, 1000]).unwrap();
    invoke(&mut s, CommandId::Clone);
    s
}

fn contact(s: &mut UiSession<Recorder>, id: u64, kind: PointerKind, phase: ContactPhase, position: [f32; 2]) -> InputReply {
    s.input(pointer_input(id, phase, kind, PointerButton::Primary, position, 0)).unwrap()
}

fn on_document(s: &UiSession<Recorder>, [x, y]: [f32; 2]) -> Point {
    s.state.camera.input_transform().map(Point { x, y })
}

fn disc(s: &UiSession<Recorder>) -> [f32; 2] {
    let p = on_surface(s, s.engine.clone_source().point.unwrap());
    [p.x, p.y]
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

#[test]
fn clone_joins_the_sculpting_tools_with_its_options_and_a_source_in_view() {
    let mut s = clone_session();
    assert_eq!(UiSession::<Recorder>::tool_category(s.layer_interaction.tool, s.state.brush.tool), ToolCategory::Retouching);
    assert!(s.state.tool_panels.sculpt_sets.groups.iter().any(|g| g.label.as_ref() == "Clone" && g.selected));
    assert!(s.command(CommandId::Sculpt).selected && s.command(CommandId::Clone).selected);
    let actions: Vec<_> = s.state.tool_actions.iter().map(|a| a.command).collect();
    assert_eq!(actions, [
        CommandId::CloneAligned,
        CommandId::SelectionReference,
        CommandId::SelectionEditing,
        CommandId::CloneFlipHorizontal,
        CommandId::CloneFlipVertical,
        CommandId::CloneResetOffset,
        CommandId::CloneSourceArm,
    ]);
    let source = s.engine.clone_source().point.unwrap();
    let [x, y, width, height] = s.state.camera.work_area;
    assert!(near(source, on_document(&s, [x + width / 2., y + height / 2.])), "a new source sits in the middle of the view: {source:?}");
    assert!(s.command(CommandId::CloneAligned).selected, "sources are aligned by default");
    assert!(s.command(CommandId::SelectionReference).selected && !s.command(CommandId::SelectionEditing).selected);
    assert_eq!(s.command_disabled_reason(CommandId::CloneResetOffset).as_deref(), Some("Clone an aligned stroke first"));

    invoke(&mut s, CommandId::SelectionEditing);
    assert!(s.command(CommandId::SelectionEditing).selected && !s.command(CommandId::SelectionReference).selected);
    invoke(&mut s, CommandId::CloneFlipHorizontal);
    invoke(&mut s, CommandId::CloneAligned);
    assert_eq!((s.engine.clone_source().flip, s.engine.clone_source().aligned), ([true, false], false));
    assert!(s.command(CommandId::CloneFlipHorizontal).selected && !s.command(CommandId::CloneAligned).selected);
    invoke(&mut s, CommandId::AutoSelect);
    assert!(s.command(CommandId::SelectionVisible).selected, "the region tools keep their own source");
    assert!(!s.command(CommandId::CloneFlipHorizontal).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::CloneSourceArm).as_deref(), Some("Choose a retouching tool first"));

    invoke(&mut s, CommandId::Pen);
    key(&mut s, "s", true, false, false);
    assert_eq!(s.state.brush.tool, Tool::Clone, "S selects the retouching tools");
    assert!(s.command(CommandId::SelectionEditing).selected, "the retouching source is remembered");
    assert_eq!(s.engine.clone_source().point, Some(source), "the document keeps its source");
}

#[test]
fn set_source_follows_alt_its_button_and_a_barrel_button_but_never_a_finger() {
    let mut s = clone_session();
    let away = |s: &UiSession<Recorder>, dx: f32| {
        let [x, y] = disc(s);
        [x - dx, y - 150.]
    };
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(s.command(CommandId::CloneSourceArm).selected);
    for (id, dx) in [(1, 200.), (2, -120.)] {
        let at = away(&s, dx);
        let reply = contact(&mut s, id, PointerKind::Pen, ContactPhase::Down, at);
        assert!(reply.handled && !reply.paint, "a pen sets the source instead of painting");
        contact(&mut s, id, PointerKind::Pen, ContactPhase::Up, at);
        assert!(near(s.engine.clone_source().point.unwrap(), on_document(&s, at)));
    }
    assert!(s.command(CommandId::CloneSourceArm).selected, "a held Set Source sets every click");
    held_key(&mut s, "Alt_L", false, alt());
    assert!(!s.command(CommandId::CloneSourceArm).selected);

    invoke(&mut s, CommandId::CloneSourceArm);
    let before = s.engine.clone_source();
    let at = away(&s, 180.);
    assert!(!contact(&mut s, 3, PointerKind::Touch, ContactPhase::Down, at).paint);
    contact(&mut s, 3, PointerKind::Touch, ContactPhase::Up, at);
    assert_eq!(s.engine.clone_source(), before, "a finger navigates");
    assert!(s.command(CommandId::CloneSourceArm).selected);
    let at = away(&s, 60.);
    assert!(!contact(&mut s, 4, PointerKind::Mouse, ContactPhase::Down, at).paint);
    let dragged = [at[0] + 30., at[1] + 10.];
    contact(&mut s, 4, PointerKind::Mouse, ContactPhase::Move, dragged);
    contact(&mut s, 4, PointerKind::Mouse, ContactPhase::Up, dragged);
    assert!(near(s.engine.clone_source().point.unwrap(), on_document(&s, dragged)), "the source follows the contact");
    assert!(!s.command(CommandId::CloneSourceArm).selected, "an armed button sets one source");
    let at = away(&s, 100.);
    assert!(contact(&mut s, 5, PointerKind::Mouse, ContactPhase::Down, at).paint, "then the mouse paints again");
    contact(&mut s, 5, PointerKind::Mouse, ContactPhase::Cancel, at);

    crate::shortcut_page::set_pen_button(
        &mut s.state.settings,
        Platform::Gtk,
        "pen.button.primary",
        Some(ToolCategory::Retouching),
        "command.CloneSourceArm",
    )
    .unwrap();
    let barrel = |s: &mut UiSession<Recorder>, pressed| s.input(UiInput::PenButton { button: PenButton::Primary, pressed }).unwrap();
    assert!(barrel(&mut s, true).handled);
    assert!(s.command(CommandId::CloneSourceArm).selected);
    let at = away(&s, -200.);
    assert!(!contact(&mut s, 6, PointerKind::Pen, ContactPhase::Down, at).paint);
    contact(&mut s, 6, PointerKind::Pen, ContactPhase::Up, at);
    assert!(near(s.engine.clone_source().point.unwrap(), on_document(&s, at)));
    barrel(&mut s, false);
    assert!(!s.command(CommandId::CloneSourceArm).selected);
}

#[test]
fn the_source_disc_drags_at_once_with_every_device_and_a_tap_shows_its_bar() {
    let mut s = clone_session();
    let moves = [(PointerKind::Pen, [40., 25.]), (PointerKind::Mouse, [-30., 50.]), (PointerKind::Touch, [60., -20.])];
    for (i, (kind, [dx, dy])) in moves.into_iter().enumerate() {
        let id = 10 + i as u64;
        let start = s.engine.clone_source().point.unwrap();
        let at = disc(&s);
        let grab = [at[0] + 3., at[1] - 2.];
        let reply = contact(&mut s, id, kind, ContactPhase::Down, grab);
        assert!(reply.handled && !reply.paint, "{kind:?} grabs the disc");
        assert_eq!(s.canvas_bar_hold() % 2, 1, "the bar stays hidden during the contact");
        let to = [grab[0] + dx, grab[1] + dy];
        contact(&mut s, id, kind, ContactPhase::Move, to);
        let moved = on_document(&s, [at[0] + dx, at[1] + dy]);
        assert!(near(s.engine.clone_source().point.unwrap(), moved), "{kind:?}");
        contact(&mut s, id, kind, ContactPhase::Up, to);
        assert!(s.state.canvas_bar.is_none(), "{kind:?}: a drag is not a tap");
        assert_ne!(Some(start), s.engine.clone_source().point);
    }
    assert!(s.state.camera.revision > 0);
    let camera = s.state.camera.clone();
    let before = s.engine.clone_source();
    let at = disc(&s);
    contact(&mut s, 20, PointerKind::Touch, ContactPhase::Down, at);
    contact(&mut s, 20, PointerKind::Touch, ContactPhase::Cancel, [0., 0.]);
    assert_eq!(s.engine.clone_source(), before, "a cancelled drag restores the source");
    assert_eq!(s.state.camera.view(), camera.view(), "the disc never navigates");

    let at = disc(&s);
    contact(&mut s, 21, PointerKind::Pen, ContactPhase::Down, at);
    contact(&mut s, 21, PointerKind::Pen, ContactPhase::Up, [at[0] + 1., at[1]]);
    let bar = s.state.canvas_bar.clone().expect("a tap shows the disc's bar");
    assert_eq!(bar.context.kind, CanvasBarKind::CloneSource);
    assert_eq!(bar_commands(&bar.items), [
        CommandId::CloneAligned,
        CommandId::CloneFlipHorizontal,
        CommandId::CloneFlipVertical,
        CommandId::CloneResetOffset,
        CommandId::CloneSourceArm,
    ]);
    let ToolOption::Choice { label, items, .. } = &bar.items[1].option else { panic!("Source is a choice") };
    assert_eq!(label.as_ref(), "Source");
    assert_eq!(items.iter().map(|i| (i.label.as_ref(), i.selected)).collect::<Vec<_>>(), [("Reference layers", true), ("Editing layer", false)]);
    let [x0, y0, x1, y1] = bar.anchor.unwrap();
    let center = s.engine.clone_source().point.unwrap();
    assert!(x0 < center.x && center.x < x1 && y0 < center.y && center.y < y1, "the bar is anchored on the disc");
    s.dispatch(UiAction::CanvasBarEdit { context: bar.context, action: Box::new(UiAction::Invoke { command: CommandId::CloneFlipVertical }) })
        .unwrap();
    assert_eq!(s.engine.clone_source().flip, [false, true]);
    s.dispatch(UiAction::CanvasBarEdit { context: bar.context, action: Box::new(items[1].action.clone()) }).unwrap();
    assert!(s.command(CommandId::SelectionEditing).selected, "the bar's Source chooses what retouching copies");

    let hold = s.canvas_bar_hold();
    assert!(contact(&mut s, 22, PointerKind::Pen, ContactPhase::Down, [at[0] - 200., at[1] + 200.]).paint);
    assert_eq!(s.canvas_bar_hold() % 2, 1, "painting hides the bar");
    contact(&mut s, 22, PointerKind::Pen, ContactPhase::Up, [at[0] - 200., at[1] + 200.]);
    assert!(s.canvas_bar_hold() == hold && s.state.canvas_bar.is_some(), "and it returns after");

    let at = disc(&s);
    contact(&mut s, 23, PointerKind::Touch, ContactPhase::Down, at);
    contact(&mut s, 23, PointerKind::Touch, ContactPhase::Up, at);
    assert!(s.state.canvas_bar.is_none(), "a second tap hides the bar");
    contact(&mut s, 24, PointerKind::Mouse, ContactPhase::Down, at);
    contact(&mut s, 24, PointerKind::Mouse, ContactPhase::Up, at);
    assert!(s.state.canvas_bar.is_some());
    invoke(&mut s, CommandId::Brush);
    assert!(s.state.canvas_bar.is_none(), "leaving the tool closes the bar");
    let mut segments = Vec::new();
    s.append_layer_overlay(&mut segments);
    assert!(segments.is_empty(), "and hides the disc");
    invoke(&mut s, CommandId::Clone);
    s.append_layer_overlay(&mut segments);
    assert!(segments.iter().any(|g| g.marker == 5.) && segments.iter().filter(|g| g.marker == 1.).count() > 20, "a ring with a sight");
}

#[test]
fn clone_strokes_explain_a_turned_layer() {
    let mut s = clone_session();
    let handle = s.engine.document().working.occurrence.unwrap();
    let mut occurrence = s.engine.document().scene().occurrence(handle).unwrap().clone();
    occurrence.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([0., 1., -1., 0., 0., 0.]));
    let change = layer_core::RecordChange::replace(&s.engine.document().artwork.occurrences, handle, Some(occurrence)).unwrap();
    s.engine.apply_edit(layer_core::Edit::Occurrence(change)).unwrap();
    let revision = s.engine.document().revision;
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
    s.frame(3, 3).unwrap();
    assert_eq!(s.state.notice.as_ref().map(|n| n.text.as_str()), Some("This layer is scaled or rotated, so it can't be retouched directly. Retouch on a new layer above it."));
    assert_eq!(s.engine.document().revision, revision);
}

#[test]
fn healing_brushes_join_the_retouching_tools_and_spot_healing_needs_no_disc() {
    let mut s = clone_session();
    for tool in [Tool::Heal, Tool::SpotHeal, Tool::Clone] {
        key(&mut s, "s", true, false, false);
        key(&mut s, "s", false, false, false);
        assert_eq!(s.state.brush.tool, tool, "S cycles the retouching tools");
    }
    assert_eq!(
        s.state.tool_panels.sculpt_sets.groups.iter().map(|g| g.label.as_ref()).collect::<Vec<_>>(),
        ["Blend", "Liquify", "Clone", "Heal", "Spot Heal"]
    );

    invoke(&mut s, CommandId::Heal);
    assert_eq!(s.engine.configured_brush().execution, layer_core::BrushExecution::Heal);
    assert_eq!(UiSession::<Recorder>::tool_category(s.layer_interaction.tool, s.state.brush.tool), ToolCategory::Retouching);
    assert!(s.clone_disc().is_some(), "healing copies from the source disc");
    assert_eq!(s.state.tool_actions.len(), 7);
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(s.command(CommandId::CloneSourceArm).selected, "Alt sets the healing source");
    held_key(&mut s, "Alt_L", false, alt());

    invoke(&mut s, CommandId::SpotHeal);
    assert_eq!(s.engine.configured_brush().execution, layer_core::BrushExecution::SpotHeal);
    assert!(s.clone_disc().is_none(), "spot healing has no disc");
    let actions: Vec<_> = s.state.tool_actions.iter().map(|a| a.command).collect();
    assert_eq!(actions, [CommandId::SelectionReference, CommandId::SelectionEditing]);
    assert!(s.command(CommandId::SelectionReference).enabled && s.command(CommandId::SelectionReference).selected);
    for command in [CommandId::CloneSourceArm, CommandId::CloneAligned, CommandId::CloneResetOffset] {
        assert!(!s.command(command).enabled);
        assert_eq!(s.command_disabled_reason(command).as_deref(), Some("Spot Healing finds its own source"), "{command:?}");
    }
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(!s.command(CommandId::CloneSourceArm).selected, "Alt does not arm a source spot healing never uses");
    held_key(&mut s, "Alt_L", false, alt());
    let at = disc(&s);
    let reply = contact(&mut s, 1, PointerKind::Pen, ContactPhase::Down, at);
    assert!(reply.paint, "the pen paints wherever the Clone disc was");
    contact(&mut s, 1, PointerKind::Pen, ContactPhase::Cancel, at);
}

#[test]
fn photo_keymaps_bind_the_healing_tools() {
    let mut settings = Settings::default();
    for (keymap, healing) in [
        ("photoshop", [("j", false, CommandId::SpotHeal), ("j", true, CommandId::Heal)]),
        ("affinity", [("j", false, CommandId::SpotHeal), ("j", true, CommandId::Heal)]),
    ] {
        crate::keymaps::select(&mut settings, keymap).unwrap();
        for (letter, shift, command) in healing {
            assert_eq!(bound(&settings, &chord(letter, false, shift, false)), Some(command.shortcut_id()), "{keymap} {letter}");
        }
    }
    crate::keymaps::select(&mut settings, "gimp").unwrap();
    assert_eq!(bound(&settings, &chord("h", false, false, false)), Some(CommandId::Heal.shortcut_id()));
    let photo = WorkspacePreset::Photographer.layout(Platform::Gtk);
    let tools: Vec<_> = photo.panel(Panel::Toolbar).unwrap().tiles().iter().map(|t| t.control).collect();
    let clone = tools.iter().position(|c| *c == ToolbarControl::Command { command: CommandId::Clone }).unwrap();
    assert_eq!(
        tools[clone..clone + 2],
        [ToolbarControl::Command { command: CommandId::Clone }, ToolbarControl::ToolSlot { slot: ToolSlotId::Healing }]
    );
    assert_eq!(ToolSlotId::Healing.variants(), [CommandId::SpotHeal, CommandId::Heal].map(|command| ToolVariant::Command { command }));
}
